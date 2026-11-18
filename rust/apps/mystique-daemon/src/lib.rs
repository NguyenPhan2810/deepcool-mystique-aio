//! The headless MYSTIQUE daemon as a library, so the state machine can be driven in
//! tests without a socket or a panel.
//!
//! The binary in `main.rs` is a thin wrapper: parse flags, acquire the single-instance
//! lock, build the core, start the worker and serve. Everything that can fail in an
//! interesting way lives here and is exercised against [`panel::Panel`] and
//! [`environment::HostEnvironment`] fakes.

#![forbid(unsafe_code)]

// Compiles the inherent `impl Core` command handlers; not part of the public surface.
mod commands;
// One connected client; reached through `server`.
mod client;
// CommandError conversions and packet records.
mod error;

pub mod cli;
pub mod core;
pub mod endpoint;
pub mod environment;
pub mod lock;
pub mod panel;
pub mod server;
pub mod service;
pub mod state;
pub mod upload;
pub mod worker;
