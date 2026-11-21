//! Shared bounded local IPC contract.
//!
//! Platform transports are Unix-domain sockets on Linux and current-user-only
//! named pipes on Windows. This module owns framing and payload semantics, not
//! the transport implementation.

use serde::Serialize;
use std::fmt;

pub const MAX_IPC_REQUEST_BYTES: usize = 256;
pub const MAX_IPC_RESPONSE_BYTES: usize = 64 * 1024;
pub const MAX_IPC_CLIENTS: usize = 8;
pub const IPC_TIMEOUT_MILLIS: u64 = 2_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IpcCommand {
    Status,
    StopAll,
    Show,
    Reload,
    Quit,
}

impl IpcCommand {
    pub fn parse(value: &str) -> Result<Self, IpcError> {
        match value {
            "status" => Ok(Self::Status),
            "stop_all" => Ok(Self::StopAll),
            "show" => Ok(Self::Show),
            "reload" => Ok(Self::Reload),
            "quit" => Ok(Self::Quit),
            _ => Err(IpcError::UnknownCommand),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Status => "status",
            Self::StopAll => "stop_all",
            Self::Show => "show",
            Self::Reload => "reload",
            Self::Quit => "quit",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IpcError {
    TooLarge,
    MissingDelimiter,
    ExtraFrameData,
    InvalidUtf8,
    UnknownCommand,
    ResponseTooLarge,
}

impl fmt::Display for IpcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::TooLarge => "IPC request exceeds the configured size limit",
            Self::MissingDelimiter => "IPC request is missing its newline delimiter",
            Self::ExtraFrameData => "IPC request contains data after its frame delimiter",
            Self::InvalidUtf8 => "IPC request is not valid UTF-8",
            Self::UnknownCommand => "unknown IPC command",
            Self::ResponseTooLarge => "IPC response exceeds the configured size limit",
        };
        f.write_str(message)
    }
}

impl std::error::Error for IpcError {}

/// Decode exactly one newline-terminated request. Additional bytes after the
/// delimiter are rejected instead of being silently ignored.
pub fn decode_request(frame: &[u8]) -> Result<IpcCommand, IpcError> {
    let Some(delimiter) = frame.iter().position(|byte| *byte == b'\n') else {
        return if frame.len() > MAX_IPC_REQUEST_BYTES {
            Err(IpcError::TooLarge)
        } else {
            Err(IpcError::MissingDelimiter)
        };
    };
    // The released IPC cap applies to command bytes, not the required newline.
    if delimiter > MAX_IPC_REQUEST_BYTES {
        return Err(IpcError::TooLarge);
    }
    if delimiter + 1 != frame.len() {
        return Err(IpcError::ExtraFrameData);
    }
    let command = std::str::from_utf8(&frame[..delimiter])
        .map_err(|_| IpcError::InvalidUtf8)?
        .trim();
    IpcCommand::parse(command)
}

/// Encode one newline-terminated response with the same defensive size cap.
pub fn encode_response(response: &str) -> Result<Vec<u8>, IpcError> {
    let mut bytes = response.as_bytes().to_vec();
    bytes.push(b'\n');
    if bytes.len() > MAX_IPC_RESPONSE_BYTES {
        return Err(IpcError::ResponseTooLarge);
    }
    Ok(bytes)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlayingInfo {
    pub key: String,
    pub label: String,
    pub sink: String,
    pub page: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RunningStatus {
    pub connected: bool,
    pub page: u8,
    pub playing: Vec<String>,
    pub playing_info: Vec<PlayingInfo>,
    pub device: String,
    pub version: String,
    pub monitor_enabled: bool,
    pub monitor_sink: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StoppedStatus {
    pub running: bool,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum StatusResponse {
    Running(RunningStatus),
    Stopped(StoppedStatus),
}

impl StatusResponse {
    pub fn stopped(version: impl Into<String>) -> Self {
        Self::Stopped(StoppedStatus {
            running: false,
            version: version.into(),
        })
    }

    pub fn running(status: RunningStatus) -> Self {
        Self::Running(status)
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliAction {
    LaunchOrFocus,
    StopAll,
    Reload,
    Quit,
    Status,
    Version,
    Install,
}

impl CliAction {
    pub fn parse_args<'a>(
        args: impl IntoIterator<Item = &'a str>,
    ) -> Result<Option<Self>, &'static str> {
        let mut selected = None;
        for arg in args {
            let action = match arg {
                "--show" => Self::LaunchOrFocus,
                "--stop" => Self::StopAll,
                "--reload" => Self::Reload,
                "--quit" => Self::Quit,
                "--status" => Self::Status,
                "--version" => Self::Version,
                "--install" => Self::Install,
                _ => return Err("unknown argument"),
            };
            if selected.replace(action).is_some() {
                return Err("only one action flag may be supplied");
            }
        }
        Ok(selected)
    }

    pub fn ipc_command(self) -> Option<IpcCommand> {
        match self {
            Self::LaunchOrFocus => Some(IpcCommand::Show),
            Self::StopAll => Some(IpcCommand::StopAll),
            Self::Status => Some(IpcCommand::Status),
            Self::Reload => Some(IpcCommand::Reload),
            Self::Quit => Some(IpcCommand::Quit),
            Self::Version | Self::Install => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_fixed_command_round_trips() {
        for command in [
            IpcCommand::Status,
            IpcCommand::StopAll,
            IpcCommand::Show,
            IpcCommand::Reload,
            IpcCommand::Quit,
        ] {
            let mut frame = command.as_str().as_bytes().to_vec();
            frame.push(b'\n');
            assert_eq!(decode_request(&frame), Ok(command));
        }
    }

    #[test]
    fn malformed_unterminated_oversized_and_extra_frames_are_rejected() {
        assert_eq!(decode_request(b"status"), Err(IpcError::MissingDelimiter));
        assert_eq!(decode_request(b"status\nquit\n"), Err(IpcError::ExtraFrameData));
        assert_eq!(decode_request(&[b'a'; MAX_IPC_REQUEST_BYTES + 1]), Err(IpcError::TooLarge));
        assert_eq!(decode_request(b"\xff\n"), Err(IpcError::InvalidUtf8));
        assert_eq!(decode_request(b"shell:true\n"), Err(IpcError::UnknownCommand));
    }

    #[test]
    fn response_framing_is_newline_terminated_and_bounded() {
        assert_eq!(encode_response("ok").unwrap(), b"ok\n");
        assert_eq!(
            encode_response(&"x".repeat(MAX_IPC_RESPONSE_BYTES)),
            Err(IpcError::ResponseTooLarge)
        );
    }

    #[test]
    fn stopped_status_matches_the_existing_cli_shape() {
        let status = StatusResponse::stopped("3.0.0-alpha.1")
            .to_json()
            .unwrap();
        assert_eq!(
            status,
            r#"{"running":false,"version":"3.0.0-alpha.1"}"#
        );
    }

    #[test]
    fn running_status_matches_the_existing_ipc_shape() {
        let status = RunningStatus {
            connected: true,
            page: 3,
            playing: vec!["3:17".to_owned()],
            playing_info: vec![PlayingInfo {
                key: "3:17".to_owned(),
                label: "Airhorn".to_owned(),
                sink: "speakers".to_owned(),
                page: 3,
            }],
            device: "Launchpad Mini".to_owned(),
            version: "3.0.0-alpha.1".to_owned(),
            monitor_enabled: true,
            monitor_sink: "headset".to_owned(),
        };
        let json = StatusResponse::running(status).to_json().unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["connected"], true);
        assert_eq!(parsed["page"], 3);
        assert_eq!(parsed["playing"][0], "3:17");
        assert_eq!(parsed["playing_info"][0]["label"], "Airhorn");
        assert_eq!(parsed["device"], "Launchpad Mini");
        assert_eq!(parsed["monitor_enabled"], true);
        assert!(parsed.get("running").is_none());
    }

    #[test]
    fn cli_accepts_only_one_known_flag() {
        assert_eq!(
            CliAction::parse_args(["--status"]).unwrap(),
            Some(CliAction::Status)
        );
        assert!(CliAction::parse_args(["--status", "--quit"]).is_err());
        assert!(CliAction::parse_args(["--unknown"]).is_err());
        assert_eq!(CliAction::parse_args([]).unwrap(), None);
    }

    #[test]
    fn cli_actions_map_to_the_fixed_ipc_protocol() {
        assert_eq!(
            CliAction::StopAll.ipc_command(),
            Some(IpcCommand::StopAll)
        );
        assert_eq!(
            CliAction::LaunchOrFocus.ipc_command(),
            Some(IpcCommand::Show)
        );
        assert_eq!(CliAction::Status.ipc_command(), Some(IpcCommand::Status));
        assert_eq!(CliAction::Version.ipc_command(), None);
    }
}
