#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

slint::include_modules!();

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::time::Duration;

use nitro_core::protocol::{ClientRequest, ServerResponse};

const PIPE: &str = r"\\.\pipe\nitro-control";

struct Client {
    reader: BufReader<File>,
    writer: File,
}

impl Client {
    fn connect() -> std::io::Result<Self> {
        let mut last_error = None;
        for _ in 0..40 {
            match OpenOptions::new().read(true).write(true).open(PIPE) {
                Ok(file) => {
                    let writer = file.try_clone()?;
                    return Ok(Self {
                        reader: BufReader::new(file),
                        writer,
                    });
                }
                Err(error) if error.raw_os_error() == Some(231) => {
                    last_error = Some(error);
                    std::thread::sleep(Duration::from_millis(100));
                }
                Err(error) => return Err(error),
            }
        }
        Err(last_error.unwrap_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::TimedOut, "pipe busy")
        }))
    }

    fn request(&mut self, request: &ClientRequest) -> std::io::Result<ServerResponse> {
        let payload = serde_json::to_string(request)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        writeln!(self.writer, "{payload}")?;
        self.writer.flush()?;

        let mut line = String::new();
        if self.reader.read_line(&mut line)? == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "broker closed the pipe",
            ));
        }
        serde_json::from_str(&line)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }
}

fn log_ui(message: &str) {
    use std::io::Write;
    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(std::env::temp_dir().join("nitro-widget.log"))
    {
        let _ = writeln!(file, "{message}");
    }
}

fn main() -> Result<(), slint::PlatformError> {
    std::env::set_var("SLINT_BACKEND", "winit-software");
    let ui = AppWindow::new()?;
    ui.set_connected(false);
    ui.set_service_up(false);
    ui.set_status_text("连接中…".into());

    let weak = ui.as_weak();
    std::thread::Builder::new()
        .name("nitro-poll".into())
        .spawn(move || poll_loop(weak))
        .expect("poll thread");

    let weak_mode = ui.as_weak();
    ui.on_set_mode(move |mode| {
        log_ui(&format!("on_set_mode {mode}"));
        if let Some(ui) = weak_mode.upgrade() {
            ui.set_mode(mode);
        }
        std::thread::spawn(move || {
            match Client::connect() {
                Ok(mut client) => match client.request(&ClientRequest::SetMode { mode: mode as u8 }) {
                    Ok(response) => log_ui(&format!("set_mode result ok={} err={:?}", response.ok, response.error)),
                    Err(e) => log_ui(&format!("set_mode transport: {e}")),
                },
                Err(e) => log_ui(&format!("set_mode connect: {e}")),
            }
        });
    });

    ui.on_install_requested(|| {
        start_service();
    });

    ui.on_close_requested(|| {
        let _ = slint::quit_event_loop();
    });

    ui.show()?;
    position_window(&ui);
    slint::run_event_loop()
}

fn position_window(ui: &AppWindow) {
    use windows::Win32::UI::HiDpi::GetDpiForSystem;
    use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};
    let screen_w = unsafe { GetSystemMetrics(SM_CXSCREEN) };
    let screen_h = unsafe { GetSystemMetrics(SM_CYSCREEN) };
    let scale = unsafe { GetDpiForSystem() } as f32 / 96.0;
    let size = ui.window().size();
    let w = (size.width as f32 * scale) as i32;
    let h = (size.height as f32 * scale) as i32;
    let x = screen_w - w - 30;
    let y = screen_h - h - 80;
    ui.window().set_position(slint::PhysicalPosition::new(x.max(0), y.max(0)));
}

fn poll_loop(weak: slint::Weak<AppWindow>) {
    loop {
        start_service();
        match Client::connect() {
            Ok(mut client) => {
                loop {
                    match client.request(&ClientRequest::Telemetry) {
                        Ok(response) => {
                            let weak = weak.clone();
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(ui) = weak.upgrade() {
                                    apply(&ui, response);
                                }
                            });
                        }
                        Err(_) => break,
                    }
                    std::thread::sleep(Duration::from_millis(1000));
                }
            }
            Err(_) => {}
        }

        let weak = weak.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = weak.upgrade() {
                ui.set_service_up(false);
                ui.set_connected(false);
                ui.set_status_text("未连接".into());
            }
        });
        std::thread::sleep(Duration::from_millis(1000));
    }
}

fn apply(ui: &AppWindow, response: ServerResponse) {
    if let Some(t) = response.telemetry {
        ui.set_cpu_temp(t.cpu_temp.map(|v| v as i32).unwrap_or(-1));
        ui.set_cpu_load(t.cpu_load as i32);
        ui.set_cpu_fan(t.cpu_fan.map(|v| v as i32).unwrap_or(-1));
        ui.set_gpu_temp(t.gpu_temp.map(|v| v as i32).unwrap_or(-1));
        ui.set_gpu_load(t.gpu_load.map(|v| v as i32).unwrap_or(-1));
        ui.set_gpu_fan(t.gpu_fan.map(|v| v as i32).unwrap_or(-1));
        ui.set_mode(t.mode as i32);
        ui.set_fan_mode(t.fan_mode.map(|v| v as i32).unwrap_or(1));
        ui.set_service_up(true);
        ui.set_connected(t.wmi_ok);
        ui.set_status_text("实时".into());
    } else {
        ui.set_service_up(false);
        ui.set_connected(false);
        ui.set_status_text(response.error.unwrap_or_else(|| "服务不可用".into()).into());
    }
}

fn start_service() {
    use windows::Win32::System::Services::{
        CloseServiceHandle, OpenSCManagerW, OpenServiceW, StartServiceW, SC_MANAGER_CONNECT,
        SERVICE_QUERY_STATUS, SERVICE_START,
    };
    unsafe {
        if let Ok(manager) = OpenSCManagerW(None, None, SC_MANAGER_CONNECT) {
            if let Ok(service) = OpenServiceW(
                manager,
                windows::core::w!("NitroControlBroker"),
                SERVICE_START | SERVICE_QUERY_STATUS,
            ) {
                let _ = StartServiceW(service, None);
                let _ = CloseServiceHandle(service);
            }
            let _ = CloseServiceHandle(manager);
        }
    }
}

