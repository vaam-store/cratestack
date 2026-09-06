use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::ClientError;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum RuntimeCodecConfig {
    #[default]
    Cbor,
    Json,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum RuntimeEnvelopeConfig {
    #[default]
    None,
    CoseSign1,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct RuntimeTransportConfig {
    pub codec: RuntimeCodecConfig,
    pub envelope: RuntimeEnvelopeConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeHeader {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeRequestWire {
    pub method: String,
    pub path: String,
    pub canonical_query: Option<String>,
    pub headers: Vec<RuntimeHeader>,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeResponseWire {
    pub status_code: u16,
    pub headers: Vec<RuntimeHeader>,
    pub body: Vec<u8>,
}

#[repr(u32)]
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum RuntimeErrorCode {
    Transport = 1,
    Codec = 2,
    State = 3,
    InvalidResponse = 4,
    Remote = 5,
    BadInput = 6,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeErrorWire {
    pub code: RuntimeErrorCode,
    pub http_status: Option<u16>,
    pub message: String,
    pub remote_code: Option<String>,
    pub remote_body: Option<Vec<u8>>,
}

impl From<ClientError> for RuntimeErrorWire {
    fn from(value: ClientError) -> Self {
        match value {
            ClientError::Transport(error) => Self {
                code: RuntimeErrorCode::Transport,
                http_status: None,
                message: error.to_string(),
                remote_code: None,
                remote_body: None,
            },
            // cratestack#926. Deliberately reported as `Transport`
            // rather than a seventh `RuntimeErrorCode`: this enum's
            // discriminants are a serialized FFI contract every bridge
            // consumer (Dart, Swift, Kotlin) switches on, so adding a
            // code breaks them, and from the far side of the bridge a
            // middleware failure *is* "the HTTP call did not complete"
            // — retries exhausted, a circuit breaker open, a token
            // refresh that failed. The middleware's own message is
            // preserved verbatim in `message`.
            #[cfg(feature = "middleware")]
            ClientError::Middleware(error) => Self {
                code: RuntimeErrorCode::Transport,
                http_status: None,
                message: error.to_string(),
                remote_code: None,
                remote_body: None,
            },
            ClientError::Codec(error) => Self {
                code: RuntimeErrorCode::Codec,
                http_status: Some(error.status_code().as_u16()),
                message: error.to_string(),
                remote_code: Some(error.code().to_owned()),
                remote_body: None,
            },
            ClientError::State(message) => Self {
                code: RuntimeErrorCode::State,
                http_status: None,
                message,
                remote_code: None,
                remote_body: None,
            },
            ClientError::InvalidResponse(message) => Self {
                code: RuntimeErrorCode::InvalidResponse,
                http_status: None,
                message,
                remote_code: None,
                remote_body: None,
            },
            ClientError::BadInput(message) => Self {
                code: RuntimeErrorCode::BadInput,
                http_status: None,
                message,
                remote_code: None,
                remote_body: None,
            },
            ClientError::Remote {
                status,
                error,
                message,
            } => Self {
                code: RuntimeErrorCode::Remote,
                http_status: Some(status.as_u16()),
                remote_code: error.as_ref().map(|value| value.code.clone()),
                remote_body: error
                    .as_ref()
                    .and_then(|value| serde_json::to_vec(value).ok()),
                message,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeStateStoreConfig {
    InMemory,
    JsonFile { path: PathBuf },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeConfigWire {
    pub base_url: String,
    pub state_store: RuntimeStateStoreConfig,
    pub transport: RuntimeTransportConfig,
}
