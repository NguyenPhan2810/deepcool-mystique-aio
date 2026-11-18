//! The daemon response envelope, the structured command error, and the packet record
//! shared with the Tauri UI.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::IpcError;

/// Stable machine codes. The first fifteen are the Tauri app's; the last two exist
/// only on the daemon.
pub mod codes {
    pub const NOT_OPEN: &str = "not_open";
    pub const NOT_FOUND: &str = "not_found";
    pub const ACCESS_DENIED: &str = "access_denied";
    pub const TIMEOUT: &str = "timeout";
    pub const STALL: &str = "stall";
    pub const BAD_COMMAND: &str = "bad_command";
    pub const DISCONNECTED: &str = "disconnected";
    pub const REJECTED: &str = "rejected";
    pub const PROTOCOL: &str = "protocol";
    pub const RANGE: &str = "range";
    pub const MEDIA: &str = "media";
    pub const UPLOAD: &str = "upload";
    pub const IO: &str = "io";
    pub const WRONG_KIND: &str = "wrong_kind";
    pub const INTERNAL: &str = "internal";

    /// The request named a command this daemon does not implement.
    pub const UNKNOWN_COMMAND: &str = "unknown_command";
    /// The device worker's bounded request queue is full.
    pub const BUSY: &str = "busy";
}

/// One 48-byte command packet, a frame header, or the `dcldfinish` terminator, exactly
/// as it went out. Identical to the Tauri `PacketRecord`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PacketRecord {
    /// e.g. `0x02 display-config` or `frame 1 static header 64B`.
    pub label: String,
    /// The command byte for a 48-byte packet; `null` for a frame header or terminator.
    pub code: Option<u8>,
    /// `init` or `data`.
    pub pipe: String,
    pub payload_hex: String,
    pub packet_hex: String,
    /// `success`, `error: ...`, or `sent (no reply)`.
    pub result: String,
    pub reply_hex: Option<String>,
}

/// The structured failure every command can return. A client shows `code`, `message`
/// and `hint` in place; `packets` holds anything already written before the failure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandError {
    pub code: String,
    pub message: String,
    #[serde(default)]
    pub hint: Option<String>,
    #[serde(default)]
    pub packets: Vec<PacketRecord>,
}

impl CommandError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            hint: None,
            packets: Vec::new(),
        }
    }

    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    pub fn with_packets(mut self, packets: Vec<PacketRecord>) -> Self {
        self.packets = packets;
        self
    }

    pub fn range(message: impl Into<String>) -> Self {
        Self::new(codes::RANGE, message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(codes::INTERNAL, message)
    }

    pub fn not_open() -> Self {
        Self::new(codes::NOT_OPEN, "the device is not open").with_hint(
            "the daemon retries by itself; call open_device to force an immediate attempt",
        )
    }
}

/// One response line. Exactly one of `data` and `error` is present.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    pub id: u64,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<CommandError>,
}

impl Response {
    pub fn success<T: Serialize>(id: u64, data: &T) -> Result<Self, IpcError> {
        Ok(Self {
            id,
            ok: true,
            data: Some(serde_json::to_value(data)?),
            error: None,
        })
    }

    pub fn failure(id: u64, error: CommandError) -> Self {
        Self {
            id,
            ok: false,
            data: None,
            error: Some(error),
        }
    }

    /// Consumes the response and returns the command's payload, or its structured
    /// error. A malformed success payload is reported as `internal`, never ignored.
    pub fn into_data<T: DeserializeOwned>(self) -> Result<T, CommandError> {
        if self.ok {
            let value = self.data.unwrap_or(Value::Null);
            serde_json::from_value(value).map_err(|error| {
                CommandError::internal(format!(
                    "the daemon returned an unreadable payload: {error}"
                ))
            })
        } else {
            Err(self.error.unwrap_or_else(|| {
                CommandError::internal("the daemon returned ok=false without an error")
            }))
        }
    }
}

/// Uppercase, space-separated hex, the spelling the captures and the UI use.
pub fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn found_hex_is_uppercase_and_space_separated() {
        assert_eq!(hex(&[]), "");
        assert_eq!(hex(&[0xAA, 0x2E, 0x05]), "AA 2E 05");
    }

    #[test]
    fn a_success_response_has_data_and_no_error_field() {
        let response = Response::success(4, &serde_json::json!({ "rotation": 2 })).unwrap();
        let json = serde_json::to_value(&response).unwrap();
        assert_eq!(json["id"], 4);
        assert_eq!(json["ok"], true);
        assert_eq!(json["data"]["rotation"], 2);
        assert!(json.get("error").is_none());
    }

    #[test]
    fn a_failure_response_has_an_error_and_no_data_field() {
        let error = CommandError::range("rotation 9 is out of range")
            .with_hint("use 0, 1, 2 or 3")
            .with_packets(vec![PacketRecord {
                label: "0x02 display-config".to_owned(),
                code: Some(2),
                pipe: "init".to_owned(),
                payload_hex: "01 00 00 00 46".to_owned(),
                packet_hex: "AA 2E 02".to_owned(),
                result: "not sent".to_owned(),
                reply_hex: None,
            }]);
        let response = Response::failure(9, error.clone());
        let json = serde_json::to_value(&response).unwrap();
        assert_eq!(json["id"], 9);
        assert_eq!(json["ok"], false);
        assert_eq!(json["error"]["code"], "range");
        assert!(json.get("data").is_none());
        assert_eq!(json["error"]["packets"][0]["code"], 2);

        let decoded: Response = serde_json::from_value(json).unwrap();
        assert_eq!(decoded.into_data::<Value>().unwrap_err(), error);
    }

    #[test]
    fn a_success_payload_deserialises_to_the_expected_type() {
        let response = Response::success(2, &serde_json::json!({ "brightness": 10 })).unwrap();
        let value: serde_json::Value = response.into_data().unwrap();
        assert_eq!(value["brightness"], 10);
    }
}
