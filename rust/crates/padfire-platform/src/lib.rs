//! Operating-system adapters for Padfire.
//!
//! This crate owns the privileged edges: device discovery, audio streams,
//! secure configuration storage, child processes, tray/window integration,
//! and the native local IPC transport. Application policy stays in
//! padfire-core and UI code never talks directly to platform APIs.

pub mod audio_engine;
pub mod config_store;
pub mod ipc_server;
pub mod loudness;
pub mod midi;
pub mod process_manager;
