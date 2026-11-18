//! The server-to-client message union.

use serde::{Deserialize, Serialize};

use crate::error::IpcError;
use crate::event::Event;
use crate::response::Response;

/// Exactly one of the two shapes the daemon can send, untagged on the wire: a
/// `Response` has `id` and `ok`, an `Event` has `event`. Serialising this enum adds no
/// wrapper, and decoding is unambiguous because each variant requires a field the other
/// cannot have.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ServerMessage {
    Response(Response),
    Event(Event),
}

impl ServerMessage {
    /// One newline-terminated JSON line, ready for the socket.
    pub fn encode(&self) -> Result<Vec<u8>, IpcError> {
        crate::frame::encode(self)
    }
}

impl From<Response> for ServerMessage {
    fn from(response: Response) -> Self {
        Self::Response(response)
    }
}

impl From<Event> for ServerMessage {
    fn from(event: Event) -> Self {
        Self::Event(event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_response_and_an_event_are_distinguished_without_a_wrapper() {
        let response = ServerMessage::Response(Response::success(3, &"ok").unwrap());
        let json = serde_json::to_value(&response).unwrap();
        assert_eq!(json["id"], 3);
        assert!(json.get("event").is_none());

        let event = ServerMessage::Event(Event::telemetry(crate::event::FrameValues {
            cpu_temperature_c: Some(1),
            cpu_usage_percent: None,
            memory_load_percent: None,
            gpu_temperature_c: None,
            cpu_clock_mhz: None,
            missing: vec!["cpu_usage_percent".to_owned()],
            source: "test".to_owned(),
        }));
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["event"], "telemetry");
        assert!(json.get("id").is_none());

        let decoded: ServerMessage = serde_json::from_value(json).unwrap();
        assert_eq!(decoded, event);
    }
}
