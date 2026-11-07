use serde::{Deserialize, Serialize};

use crate::Telemetry;

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum ClientRequest {
    Telemetry,
    SetMode { mode: u8 },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ServerResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub telemetry: Option<Telemetry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl ServerResponse {
    pub fn ok(telemetry: Telemetry) -> Self {
        Self { ok: true, telemetry: Some(telemetry), error: None }
    }

    pub fn done() -> Self {
        Self { ok: true, telemetry: None, error: None }
    }

    pub fn err(message: impl Into<String>) -> Self {
        Self { ok: false, telemetry: None, error: Some(message.into()) }
    }
}
