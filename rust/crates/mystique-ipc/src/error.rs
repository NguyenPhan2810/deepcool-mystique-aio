//! The one error type produced by encoding, decoding and argument parsing.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum IpcError {
    #[error("invalid JSON message: {0}")]
    Json(#[from] serde_json::Error),

    /// A line passed the decoder limit without a newline.
    #[error("message is {len} bytes; the maximum is {limit}")]
    TooLong { len: usize, limit: usize },

    /// The command exists but its `args` object does not fit the command's shape.
    #[error("invalid arguments for {command}: {message}")]
    Arguments { command: String, message: String },
}
