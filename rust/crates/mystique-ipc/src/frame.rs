//! Newline-delimited JSON framing: one message per line, split at `\n`.
//!
//! Nothing here opens a socket. [`encode`] produces the bytes a peer writes, and
//! [`LineDecoder`] turns arbitrary read boundaries back into messages. The decoder is
//! the only place that needs to be correct about split reads, so it is the only place
//! that owns a buffer.

use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::error::IpcError;

/// Largest accepted line, including the newline. Upload previews carry a small PNG data
/// URL, so this is generous for real traffic and still bounded against a client that
/// never sends a newline.
pub const DEFAULT_MAX_LINE: usize = 8 * 1024 * 1024;

/// Serialises one message and appends the frame delimiter.
pub fn encode<T: Serialize>(message: &T) -> Result<Vec<u8>, IpcError> {
    let mut line = serde_json::to_vec(message)?;
    line.push(b'\n');
    Ok(line)
}

/// The same bytes as [`encode`], as a `String`. Useful for logging and tests; sockets
/// use the byte form.
pub fn encode_line<T: Serialize>(message: &T) -> Result<String, IpcError> {
    let mut line = serde_json::to_string(message)?;
    line.push('\n');
    Ok(line)
}

/// Decodes one complete line. Leading/trailing whitespace and a `\r` are tolerated.
pub fn decode<T: DeserializeOwned>(line: &[u8]) -> Result<T, IpcError> {
    let line = line.trim_ascii();
    Ok(serde_json::from_slice(line)?)
}

/// A byte-stream-to-messages buffer.
///
/// A read may contain half a message, several messages, or a message split inside a
/// multi-byte UTF-8 sequence; [`Self::feed`] yields only complete lines. After an error
/// the decoder must not be reused: its buffer is cleared so a caller that ignores the
/// error cannot accidentally parse a fragment.
#[derive(Debug)]
pub struct LineDecoder {
    buffer: Vec<u8>,
    max_line: usize,
}

impl Default for LineDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl LineDecoder {
    pub fn new() -> Self {
        Self::with_limit(DEFAULT_MAX_LINE)
    }

    pub fn with_limit(max_line: usize) -> Self {
        Self {
            buffer: Vec::new(),
            max_line: max_line.max(1),
        }
    }

    /// Appends bytes and returns every complete message now available.
    pub fn feed<T: DeserializeOwned>(&mut self, bytes: &[u8]) -> Result<Vec<T>, IpcError> {
        self.buffer.extend_from_slice(bytes);

        let mut messages = Vec::new();
        let mut start = 0usize;
        while let Some(offset) = self.buffer[start..].iter().position(|byte| *byte == b'\n') {
            let end = start + offset;
            let line = &self.buffer[start..end];
            start = end + 1;
            let line = line.trim_ascii();
            if line.is_empty() {
                continue;
            }
            if line.len() > self.max_line {
                let len = line.len();
                let limit = self.max_line;
                self.buffer.clear();
                return Err(IpcError::TooLong { len, limit });
            }
            match serde_json::from_slice(line) {
                Ok(message) => messages.push(message),
                Err(error) => {
                    self.buffer.clear();
                    return Err(IpcError::Json(error));
                }
            }
        }
        if start > 0 {
            self.buffer.drain(..start);
        }

        if self.buffer.len() > self.max_line {
            let len = self.buffer.len();
            let limit = self.max_line;
            self.buffer.clear();
            return Err(IpcError::TooLong { len, limit });
        }
        Ok(messages)
    }

    /// Bytes of a message that has not ended yet.
    pub fn buffered(&self) -> usize {
        self.buffer.len()
    }

    pub fn clear(&mut self) {
        self.buffer.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request::Request;

    fn request(id: u64) -> Request {
        Request::new(id, "status")
    }

    #[test]
    fn a_message_split_at_every_byte_boundary_is_reassembled() {
        let bytes = encode(&request(1)).unwrap();
        let mut decoder = LineDecoder::new();
        let mut messages = Vec::new();
        for byte in &bytes {
            let mut fresh = decoder.feed::<Request>(&[*byte]).unwrap();
            messages.append(&mut fresh);
            assert!(messages.is_empty() || *byte == b'\n');
        }
        assert_eq!(messages, [request(1)]);
        assert_eq!(decoder.buffered(), 0);
    }

    #[test]
    fn several_messages_in_one_read_and_a_partial_tail() {
        let mut bytes = encode(&request(1)).unwrap();
        bytes.extend_from_slice(&encode(&request(2)).unwrap());
        let partial = encode(&request(3)).unwrap();
        bytes.extend_from_slice(&partial[..5]);

        let mut decoder = LineDecoder::new();
        let messages = decoder.feed::<Request>(&bytes).unwrap();
        assert_eq!(messages, [request(1), request(2)]);
        assert_eq!(decoder.buffered(), 5);

        let rest = decoder.feed::<Request>(&partial[5..]).unwrap();
        assert_eq!(rest, [request(3)]);
        assert_eq!(decoder.buffered(), 0);
    }

    #[test]
    fn blank_lines_and_crlf_are_tolerated() {
        let mut decoder = LineDecoder::new();
        let messages = decoder
            .feed::<Request>(b"\r\n\n{\"id\":5,\"cmd\":\"status\",\"args\":{}}\r\n\n")
            .unwrap();
        assert_eq!(messages, [request(5)]);
    }

    #[test]
    fn a_line_that_never_ends_is_rejected_at_the_limit() {
        let mut decoder = LineDecoder::with_limit(32);
        let error = decoder.feed::<Request>(&[b'x'; 33]).unwrap_err();
        assert!(matches!(error, IpcError::TooLong { len: 33, limit: 32 }));
        assert_eq!(decoder.buffered(), 0, "the decoder is cleared on error");

        // One more byte after a complete message is still a partial message, not a
        // second message.
        let mut decoder = LineDecoder::with_limit(64);
        let mut bytes = encode(&request(1)).unwrap();
        bytes.extend_from_slice(&[b'x'; 65]);
        assert!(decoder.feed::<Request>(&bytes).is_err());
    }

    #[test]
    fn invalid_json_is_an_error_and_not_a_silent_drop() {
        let mut decoder = LineDecoder::new();
        let error = decoder
            .feed::<Request>(b"{\"id\":1,\"cmd\":\n")
            .unwrap_err();
        assert!(matches!(error, IpcError::Json(_)));
        assert_eq!(decoder.buffered(), 0);
    }

    #[test]
    fn a_split_inside_a_utf8_string_is_fine() {
        let request = Request::with_args(
            11,
            crate::request::cmd::PLAN_UPLOAD,
            &crate::request::args::Upload {
                path: "C:\\禅\\panel.png".to_owned(),
                fit: None,
            },
        )
        .unwrap();
        let bytes = encode(&request).unwrap();
        let split = bytes.iter().position(|byte| *byte >= 0x80).unwrap() + 1;
        let mut decoder = LineDecoder::new();
        assert!(decoder.feed::<Request>(&bytes[..split]).unwrap().is_empty());
        assert_eq!(decoder.feed::<Request>(&bytes[split..]).unwrap(), [request]);
    }
}
