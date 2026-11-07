use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::mpsc::{self, Receiver, Sender};

use serde::{Deserialize, Serialize};
use sysinfo::System;
use wmi::{COMLibrary, Variant, WMIConnection};

fn log(msg: &str) {
    use std::io::Write;
    let dir = std::env::var("ProgramData")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir())
        .join("NitroControl");
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("core.log"))
    {
        let _ = writeln!(f, "{}", msg);
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Telemetry {
    pub cpu_temp: Option<u32>,
    pub cpu_load: u32,
    pub cpu_fan: Option<u32>,
    pub gpu_temp: Option<u32>,
    pub gpu_load: Option<u32>,
    pub gpu_fan: Option<u32>,
    pub mode: u8,
    pub fan_mode: Option<u8>,
    pub wmi_ok: bool,
}

impl Default for Telemetry {
    fn default() -> Self {
        Self {
            cpu_temp: None,
            cpu_load: 0,
            cpu_fan: None,
            gpu_temp: None,
            gpu_load: None,
            gpu_fan: None,
            mode: 1,
            fan_mode: None,
            wmi_ok: false,
        }
    }
}

pub enum Request {
    Telemetry(Sender<Telemetry>),
    SetMode(u8, Sender<Result<(), String>>),
}

pub fn spawn() -> Sender<Request> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("nitro-wmi".into())
        .spawn(move || {
            let mut worker = Worker::new();
            worker.run(rx);
        })
        .expect("failed to spawn wmi thread");
    tx
}

#[derive(Deserialize)]
#[allow(non_camel_case_types)]
struct AcerGamingFunction;

#[derive(Serialize)]
struct GmIn {
    #[serde(rename = "gmInput")]
    gm_input: u32,
}

#[derive(Serialize)]
struct GmIn64 {
    #[serde(rename = "gmInput")]
    gm_input: u64,
}

#[derive(Deserialize)]
struct GmOut64 {
    #[serde(rename = "gmOutput")]
    gm_output: u64,
}

#[derive(Deserialize)]
struct GmOut32 {
    #[serde(rename = "gmOutput")]
    gm_output: u32,
}

struct Worker {
    con: Option<WMIConnection>,
    path: Option<String>,
    sys: System,
    nvml: Option<Nvml>,
    last: Telemetry,
}

impl Worker {
    fn new() -> Self {
        log("worker: new");
        let mut sys = System::new();
        sys.refresh_cpu_usage();

        let (con, path) = match connect() {
            Ok((con, path)) => (Some(con), Some(path)),
            Err(e) => {
                log(&format!("worker: connect failed: {e}"));
                eprintln!("nitro-wmi: {e}");
                (None, None)
            }
        };

        Self {
            con,
            path,
            sys,
            nvml: Nvml::new(),
            last: Telemetry::default(),
        }
    }

    fn run(&mut self, rx: Receiver<Request>) {
        for request in rx {
            match request {
                Request::Telemetry(reply) => {
                    let _ = reply.send(self.telemetry());
                }
                Request::SetMode(mode, reply) => {
                    let _ = reply.send(self.set_mode(mode));
                }
            }
        }
    }

    fn telemetry(&mut self) -> Telemetry {
        log("worker: telemetry start");
        let cpu_temp = self.sensor(0x01);
        let cpu_fan = self.sensor(0x02);
        let gpu_temp_wmi = self.sensor(0x0A);
        let gpu_fan = self.sensor(0x06);
        let mode_read = self.get_mode();
        let mode = mode_read.unwrap_or(self.last.mode);
        let fan_mode = self.get_fan_mode();

        self.sys.refresh_cpu_usage();
        let cpu_load = self.sys.global_cpu_usage().round().clamp(0.0, 100.0) as u32;

        let gpu_temp = self
            .nvml
            .as_ref()
            .and_then(|n| n.temperature())
            .or(gpu_temp_wmi);
        let gpu_load = self.nvml.as_ref().and_then(|n| n.utilization());

        let wmi_ok = cpu_temp.is_some()
            || cpu_fan.is_some()
            || gpu_temp_wmi.is_some()
            || gpu_fan.is_some()
            || mode_read.is_some();
        let telemetry = Telemetry {
            cpu_temp,
            cpu_load,
            cpu_fan,
            gpu_temp,
            gpu_load,
            gpu_fan,
            mode,
            fan_mode,
            wmi_ok,
        };
        self.last = telemetry.clone();
        log(&format!("worker: telemetry done ok={} cpu={:?} gpu={:?}", telemetry.wmi_ok, telemetry.cpu_temp, telemetry.gpu_temp));
        telemetry
    }

    fn sensor(&self, id: u32) -> Option<u32> {
        let con = self.con.as_ref()?;
        let path = self.path.as_deref()?;
        let command = 1 | (id << 8);
        let result = con.exec_instance_method::<AcerGamingFunction, _>(
            path,
            "GetGamingSysInfo",
            GmIn { gm_input: command },
        );
        let out: GmOut64 = match result {
            Ok(v) => v,
            Err(e) => {
                log(&format!("sensor 0x{id:X} failed: {e}"));
                return None;
            }
        };
        if out.gm_output & 0xFF != 0 {
            return None;
        }
        Some(((out.gm_output >> 8) & 0xFFFF) as u32)
    }

    fn get_mode(&self) -> Option<u8> {
        let con = self.con.as_ref()?;
        let path = self.path.as_deref()?;
        let out: GmOut64 = con
            .exec_instance_method::<AcerGamingFunction, _>(
                path,
                "GetGamingMiscSetting",
                GmIn { gm_input: 0x0B },
            )
            .ok()?;
        if out.gm_output & 0xFF != 0 {
            return None;
        }
        Some(((out.gm_output >> 8) & 0xFF) as u8)
    }

    fn get_fan_mode(&self) -> Option<u8> {
        let con = self.con.as_ref()?;
        let path = self.path.as_deref()?;
        let out: GmOut64 = con
            .exec_instance_method::<AcerGamingFunction, _>(
                path,
                "GetGamingFanBehavior",
                GmIn { gm_input: 0x09 },
            )
            .ok()?;
        if out.gm_output & 0xFF != 0 {
            return None;
        }
        let cpu = ((out.gm_output >> 8) & 0x3) as u8;
        let gpu = ((out.gm_output >> 14) & 0x3) as u8;
        Some(if cpu == 1 && gpu == 1 {
            1
        } else if cpu == 2 && gpu == 2 {
            2
        } else if cpu == 3 || gpu == 3 {
            3
        } else {
            1
        })
    }

    fn set_mode(&mut self, mode: u8) -> Result<(), String> {
        let con = self.con.as_ref().ok_or("WMI 不可用，请以管理员身份运行")?;
        let path = self.path.as_deref().ok_or("找不到 AcerGamingFunction 实例")?;
        let input = ((mode as u64) << 8) | 0x0B;
        let out: GmOut32 = con
            .exec_instance_method::<AcerGamingFunction, _>(
                path,
                "SetGamingMiscSetting",
                GmIn64 { gm_input: input },
            )
            .map_err(|e| format!("SetGamingMiscSetting: {e}"))?;
        if out.gm_output & 0xFF != 0 {
            return Err(format!("固件返回状态 0x{:02X}", out.gm_output & 0xFF));
        }
        self.last.mode = mode;
        Ok(())
    }
}

fn connect() -> Result<(WMIConnection, String), String> {
    log("connect: com");
    let com = COMLibrary::new().map_err(|e| format!("COM: {e}"))?;
    log("connect: wmi");
    let con = WMIConnection::with_namespace_path("ROOT\\WMI", com)
        .map_err(|e| format!("WMI: {e}"))?;
    let rows: Vec<HashMap<String, Variant>> = con
        .raw_query("SELECT * FROM AcerGamingFunction")
        .map_err(|e| format!("query: {e}"))?;
    log(&format!("connect: rows={}", rows.len()));
    if let Some(row) = rows.first() {
        for (k, v) in row {
            log(&format!("connect: key={k} variant={v:?}"));
        }
    }
    let row = rows.first().ok_or("AcerGamingFunction 没有实例")?;
    let path = match row.get("__Path") {
        Some(Variant::String(s)) => s.clone(),
        _ => match row.get("InstanceName") {
            Some(Variant::String(name)) => {
                let escaped = name.replace('\\', "\\\\");
                format!("AcerGamingFunction.InstanceName=\"{}\"", escaped)
            }
            _ => return Err("实例缺少 __Path / InstanceName".into()),
        },
    };
    log(&format!("connect: path={path}"));
    Ok((con, path))
}

#[repr(C)]
struct NvmlUtilization {
    gpu: u32,
    memory: u32,
}

struct Nvml {
    _lib: libloading::Library,
    handle: *mut c_void,
    get_temp: unsafe extern "C" fn(*mut c_void, u32, *mut u32) -> i32,
    get_util: unsafe extern "C" fn(*mut c_void, *mut NvmlUtilization) -> i32,
}

impl Nvml {
    fn new() -> Option<Self> {
        unsafe {
            let lib = libloading::Library::new("nvml.dll").ok()?;
            let init: libloading::Symbol<unsafe extern "C" fn() -> i32> =
                lib.get(b"nvmlInit_v2").ok()?;
            if init() != 0 {
                return None;
            }
            let get_handle: libloading::Symbol<
                unsafe extern "C" fn(u32, *mut *mut c_void) -> i32,
            > = lib.get(b"nvmlDeviceGetHandleByIndex_v2").ok()?;
            let mut handle: *mut c_void = std::ptr::null_mut();
            if get_handle(0, &mut handle) != 0 || handle.is_null() {
                return None;
            }
            let get_temp = *lib
                .get::<unsafe extern "C" fn(*mut c_void, u32, *mut u32) -> i32>(
                    b"nvmlDeviceGetTemperature",
                )
                .ok()?;
            let get_util = *lib
                .get::<unsafe extern "C" fn(*mut c_void, *mut NvmlUtilization) -> i32>(
                    b"nvmlDeviceGetUtilizationRates",
                )
                .ok()?;
            Some(Self {
                _lib: lib,
                handle,
                get_temp,
                get_util,
            })
        }
    }

    fn temperature(&self) -> Option<u32> {
        let mut value = 0u32;
        let status = unsafe { (self.get_temp)(self.handle, 0, &mut value) };
        (status == 0).then_some(value)
    }

    fn utilization(&self) -> Option<u32> {
        let mut value = NvmlUtilization { gpu: 0, memory: 0 };
        let status = unsafe { (self.get_util)(self.handle, &mut value) };
        (status == 0).then_some(value.gpu)
    }
}
