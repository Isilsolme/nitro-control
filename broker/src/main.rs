#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::ffi::{c_void, OsString};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::os::windows::io::FromRawHandle;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use nitro_core::protocol::{ClientRequest, ServerResponse};
use nitro_core::{Request, Telemetry};
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    CloseHandle, GetLastError, LocalFree, ERROR_PIPE_CONNECTED, FALSE, HANDLE, HLOCAL,
    INVALID_HANDLE_VALUE,
};
use windows::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW;
use windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
use windows::Win32::Storage::FileSystem::PIPE_ACCESS_DUPLEX;
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, GetNamedPipeClientProcessId, PIPE_READMODE_MESSAGE,
    PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_MESSAGE, PIPE_WAIT,
};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows_service::service::{
    ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus, ServiceType,
};
use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
use windows_service::{define_windows_service, service_dispatcher};

const SERVICE_NAME: &str = "NitroControlBroker";
const SERVICE_TYPE: ServiceType = ServiceType::OWN_PROCESS;
const PIPE_NAME: &str = r"\\.\pipe\nitro-control";
const MAX_REQUEST: usize = 4096;
const MAX_INSTANCES: u32 = 8;
const SET_MODE_MIN_INTERVAL: Duration = Duration::from_millis(300);
const IDLE_TIMEOUT: Duration = Duration::from_secs(300);

fn main() -> windows_service::Result<()> {
    if std::env::args().any(|arg| arg == "--console") {
        let tx = nitro_core::spawn();
        let stop = Arc::new(AtomicBool::new(false));
        pipe_loop(
            tx,
            stop,
            Arc::new(AtomicUsize::new(0)),
            Arc::new(Mutex::new(Instant::now())),
        );
        return Ok(());
    }
    service_dispatcher::start(SERVICE_NAME, ffi_service_main)
}

define_windows_service!(ffi_service_main, service_main);

fn service_main(_arguments: Vec<OsString>) {
    if let Err(error) = run_service() {
        log(&format!("service: fatal: {error}"));
    }
}

fn run_service() -> windows_service::Result<()> {
    let (stop_tx, stop_rx) = mpsc::channel();
    let event_handler = move |event| -> ServiceControlHandlerResult {
        match event {
            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            ServiceControl::Stop => {
                let _ = stop_tx.send(());
                ServiceControlHandlerResult::NoError
            }
            _ => ServiceControlHandlerResult::NotImplemented,
        }
    };

    let status_handle = service_control_handler::register(SERVICE_NAME, event_handler)?;
    status_handle.set_service_status(ServiceStatus {
        service_type: SERVICE_TYPE,
        current_state: ServiceState::Running,
        controls_accepted: ServiceControlAccept::STOP,
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: Duration::default(),
        process_id: None,
    })?;

    let tx = nitro_core::spawn();
    let stop = Arc::new(AtomicBool::new(false));
    let active = Arc::new(AtomicUsize::new(0));
    let last_activity = Arc::new(Mutex::new(Instant::now()));
    let pipe_stop = stop.clone();
    let pipe_active = active.clone();
    let pipe_activity = last_activity.clone();
    std::thread::spawn(move || pipe_loop(tx, pipe_stop, pipe_active, pipe_activity));

    loop {
        match stop_rx.recv_timeout(Duration::from_secs(30)) {
            Ok(_) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let idle = last_activity.lock().map(|t| t.elapsed()).unwrap_or_default();
                if active.load(Ordering::SeqCst) == 0 && idle > IDLE_TIMEOUT {
                    log("service: idle stop");
                    break;
                }
            }
        }
    }
    stop.store(true, Ordering::SeqCst);
    wake_pipe();

    status_handle.set_service_status(ServiceStatus {
        service_type: SERVICE_TYPE,
        current_state: ServiceState::Stopped,
        controls_accepted: ServiceControlAccept::empty(),
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: Duration::default(),
        process_id: None,
    })?;
    Ok(())
}

fn pipe_loop(
    tx: Sender<Request>,
    stop: Arc<AtomicBool>,
    active: Arc<AtomicUsize>,
    last_activity: Arc<Mutex<Instant>>,
) {
    log("pipe: start");
    while !stop.load(Ordering::SeqCst) {
        if let Err(error) = serve(&tx, &stop, &active, &last_activity) {
            log(&format!("pipe: serve error: {error}"));
            std::thread::sleep(Duration::from_millis(500));
        }
    }
    log("pipe: stopped");
}

fn serve(
    tx: &Sender<Request>,
    stop: &AtomicBool,
    active: &Arc<AtomicUsize>,
    last_activity: &Arc<Mutex<Instant>>,
) -> std::io::Result<()> {
    let sddl = match allowed_sid() {
        Some(sid) => format!(
            "D:(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;{sid})S:(ML;;NW;;;ME)"
        ),
        None => {
            log("pipe: user SID missing; only SYSTEM/Administrators may connect");
            "D:(A;;GA;;;SY)(A;;GA;;;BA)S:(ML;;NW;;;ME)".to_string()
        }
    };
    let (security, descriptor) = make_security(&sddl);

    let handle = unsafe {
        CreateNamedPipeW(
            windows::core::w!("\\\\.\\pipe\\nitro-control"),
            PIPE_ACCESS_DUPLEX,
            PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            MAX_INSTANCES,
            4096,
            4096,
            0,
            Some(&security),
        )
    };
    unsafe {
        LocalFree(Some(HLOCAL(descriptor.0)));
    }
    if handle == INVALID_HANDLE_VALUE {
        return Err(std::io::Error::last_os_error());
    }

    let connected = unsafe { ConnectNamedPipe(handle, None) };
    if connected.is_err() {
        let error = unsafe { GetLastError() };
        if error != ERROR_PIPE_CONNECTED {
            let _ = unsafe { CloseHandle(handle) };
            return Err(std::io::Error::last_os_error());
        }
    }

    if stop.load(Ordering::SeqCst) {
        let _ = unsafe { CloseHandle(handle) };
        return Ok(());
    }

    if !client_allowed(handle) {
        log("pipe: rejected client (path mismatch)");
        let _ = unsafe { CloseHandle(handle) };
        return Ok(());
    }

    let raw = handle.0 as usize;
    let tx = tx.clone();
    let active = active.clone();
    let last_activity = last_activity.clone();
    active.fetch_add(1, Ordering::SeqCst);
    if let Ok(mut last) = last_activity.lock() {
        *last = Instant::now();
    }
    std::thread::spawn(move || {
        let handle = HANDLE(raw as *mut c_void);
        if let Err(error) = serve_client(handle, &tx, &last_activity) {
            log(&format!("pipe: client error: {error}"));
        }
        active.fetch_sub(1, Ordering::SeqCst);
    });
    Ok(())
}

fn serve_client(
    handle: HANDLE,
    tx: &Sender<Request>,
    last_activity: &Arc<Mutex<Instant>>,
) -> std::io::Result<()> {
    let file = unsafe { File::from_raw_handle(handle.0) };
    let mut reader = BufReader::new(file.try_clone()?);
    let mut writer = BufWriter::new(file);
    log("pipe: client connected");

    let mut last_set = Instant::now() - SET_MODE_MIN_INTERVAL;
    loop {
        let line = match read_request(&mut reader)? {
            Some(line) => line,
            None => break,
        };
        if let Ok(mut last) = last_activity.lock() {
            *last = Instant::now();
        }
        let response = match serde_json::from_str::<ClientRequest>(&line) {
            Ok(ClientRequest::SetMode { mode }) => {
                if !matches!(mode, 0 | 1 | 4 | 5 | 6) {
                    ServerResponse::err("invalid mode")
                } else if last_set.elapsed() < SET_MODE_MIN_INTERVAL {
                    ServerResponse::err("rate limited")
                } else {
                    last_set = Instant::now();
                    handle_set_mode(tx, mode)
                }
            }
            Ok(ClientRequest::Telemetry) => handle_telemetry(tx),
            Err(error) => ServerResponse::err(format!("bad request: {error}")),
        };
        let json = serde_json::to_string(&response).unwrap_or_else(|_| "{\"ok\":false}".into());
        writeln!(writer, "{json}")?;
        writer.flush()?;
    }
    log("pipe: client disconnected");
    Ok(())
}

fn handle_telemetry(tx: &Sender<Request>) -> ServerResponse {
    let (reply_tx, reply_rx) = mpsc::channel::<Telemetry>();
    if tx.send(Request::Telemetry(reply_tx)).is_err() {
        return ServerResponse::err("wmi worker gone");
    }
    match reply_rx.recv_timeout(Duration::from_secs(2)) {
        Ok(telemetry) => ServerResponse::ok(telemetry),
        Err(error) => ServerResponse::err(format!("telemetry timeout: {error}")),
    }
}

fn handle_set_mode(tx: &Sender<Request>, mode: u8) -> ServerResponse {
    let (reply_tx, reply_rx) = mpsc::channel::<Result<(), String>>();
    if tx.send(Request::SetMode(mode, reply_tx)).is_err() {
        return ServerResponse::err("wmi worker gone");
    }
    match reply_rx.recv_timeout(Duration::from_secs(3)) {
        Ok(Ok(())) => ServerResponse::done(),
        Ok(Err(error)) => ServerResponse::err(error),
        Err(error) => ServerResponse::err(format!("set_mode timeout: {error}")),
    }
}

fn read_request(reader: &mut BufReader<File>) -> std::io::Result<Option<String>> {
    let mut data = Vec::new();
    loop {
        let (take, newline) = {
            let available = reader.fill_buf()?;
            if available.is_empty() {
                return if data.is_empty() {
                    Ok(None)
                } else {
                    Err(std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        "unterminated request",
                    ))
                };
            }
            let newline = available.iter().position(|&byte| byte == b'\n');
            let take = newline.unwrap_or(available.len());
            if data.len() + take > MAX_REQUEST {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "request too large",
                ));
            }
            data.extend_from_slice(&available[..take]);
            (take + usize::from(newline.is_some()), newline)
        };
        reader.consume(take);
        if newline.is_some() {
            break;
        }
    }
    String::from_utf8(data)
        .map(Some)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))
}

fn client_allowed(handle: HANDLE) -> bool {
    let mut pid = 0u32;
    if unsafe { GetNamedPipeClientProcessId(handle, &mut pid) }.is_err() {
        return false;
    }
    let process = match unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) } {
        Ok(process) => process,
        Err(_) => return false,
    };

    let mut buffer = vec![0u16; 32768];
    let mut size = buffer.len() as u32;
    let result = unsafe {
        QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut size,
        )
    };
    unsafe {
        let _ = CloseHandle(process);
    }
    if result.is_err() {
        return false;
    }

    let actual = String::from_utf16_lossy(&buffer[..size as usize]);
    let Some(expected) = expected_client_path() else {
        return false;
    };
    normalize(&actual) == normalize(&expected.to_string_lossy())
}

fn expected_client_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    Some(exe.parent()?.join("nitro-widget.exe"))
}

fn normalize(path: &str) -> String {
    path.trim_start_matches(r"\\?\").replace('/', "\\").to_lowercase()
}

fn allowed_sid() -> Option<String> {
    let path = std::env::var("ProgramData").ok()?;
    let path = Path::new(&path).join("NitroControl").join("user.sid");
    let sid = std::fs::read_to_string(path).ok()?.trim().to_string();
    let rest = sid.strip_prefix("S-")?;
    if !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit() || c == '-') {
        Some(sid)
    } else {
        None
    }
}

fn make_security(sddl: &str) -> (SECURITY_ATTRIBUTES, PSECURITY_DESCRIPTOR) {
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    let sddl_wide = windows::core::HSTRING::from(sddl);
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(sddl_wide.as_ptr()),
            1,
            &mut descriptor,
            None,
        )
        .expect("invalid pipe SDDL");
    }
    let attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0,
        bInheritHandle: FALSE,
    };
    (attributes, descriptor)
}

fn wake_pipe() {
    let _ = OpenOptions::new().read(true).write(true).open(PIPE_NAME);
}

fn log(message: &str) {
    let dir = std::env::var("ProgramData")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir())
        .join("NitroControl");
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("broker.log"))
    {
        let _ = writeln!(file, "{message}");
    }
}
