//! Windows local IPC using a synchronous named pipe.
//!
//! The pipe inherits the creating process token's default DACL (current-user
//! access) and sets PIPE_REJECT_REMOTE_CLIENTS. A first-instance open provides
//! single-instance ownership. The byte stream follows the same fixed command,
//! newline framing and bounded response contract as the Unix socket.

use padfire_core::ipc::{
    IpcCommand, MAX_IPC_REQUEST_BYTES, MAX_IPC_RESPONSE_BYTES, decode_request, encode_response,
};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_FILE_NOT_FOUND, ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED, GENERIC_READ,
    GENERIC_WRITE, GetLastError, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_FLAG_FIRST_PIPE_INSTANCE, OPEN_EXISTING,
    PIPE_ACCESS_DUPLEX, ReadFile, WriteFile,
};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE,
    PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT, PeekNamedPipe,
    WaitNamedPipeW,
};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(2);
const PIPE_BUFFER_BYTES: u32 = MAX_IPC_RESPONSE_BYTES as u32;
const RESPONSE_ACK: &[u8] = b"ack\n";

struct PipeHandle(HANDLE);

impl PipeHandle {
    fn new(handle: HANDLE) -> Result<Self, String> {
        if handle.is_null() || handle == INVALID_HANDLE_VALUE {
            Err(last_error("Windows named-pipe handle creation failed"))
        } else {
            Ok(Self(handle))
        }
    }

    fn raw(&self) -> HANDLE {
        self.0
    }
}

impl Drop for PipeHandle {
    fn drop(&mut self) {
        // SAFETY: This wrapper exclusively owns the handle and closes it once.
        unsafe { CloseHandle(self.0) };
    }
}

#[derive(Debug)]
pub struct IpcServer {
    path: PathBuf,
    name: Vec<u16>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl IpcServer {
    pub fn spawn<F>(path: impl Into<PathBuf>, handler: F) -> Result<Self, String>
    where
        F: Fn(IpcCommand) -> String + Send + Sync + 'static,
    {
        let path = path.into();
        let name = pipe_name(&path);
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let thread_name = name.clone();
        let handler = Arc::new(handler);
        let (ready_tx, ready_rx) = mpsc::sync_channel::<Result<(), String>>(1);

        let worker = thread::Builder::new()
            .name("padfire-ipc-pipe".to_owned())
            .spawn(move || {
                let pipe = match create_server_pipe(&thread_name) {
                    Ok(pipe) => {
                        let _ = ready_tx.send(Ok(()));
                        pipe
                    }
                    Err(error) => {
                        let _ = ready_tx.send(Err(error));
                        return;
                    }
                };
                serve(pipe, thread_stop, handler);
            })
            .map_err(|error| format!("could not start Windows IPC thread: {error}"))?;

        match ready_rx.recv_timeout(Duration::from_secs(3)) {
            Ok(Ok(())) => Ok(Self {
                path,
                name,
                stop,
                worker: Some(worker),
            }),
            Ok(Err(error)) => {
                let _ = worker.join();
                Err(error)
            }
            Err(error) => {
                stop.store(true, Ordering::Release);
                let _ = connect_pipe(&name);
                let _ = worker.join();
                Err(format!(
                    "Windows IPC listener did not become ready: {error}"
                ))
            }
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn shutdown(&mut self) {
        self.stop.store(true, Ordering::Release);
        // Wake ConnectNamedPipe with a local client so the worker sees stop.
        let _ = connect_pipe(&self.name);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for IpcServer {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn pipe_name(path: &Path) -> Vec<u16> {
    // A deterministic, per-user install-path name supports multiple Windows
    // accounts and side-by-side stable/alpha builds without network listeners.
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in path.to_string_lossy().to_ascii_lowercase().bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    let username = std::env::var("USERNAME")
        .unwrap_or_else(|_| "user".to_owned())
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || *character == '-')
        .take(32)
        .collect::<String>();
    format!("\\\\.\\pipe\\Padfire-{username}-{hash:016x}")
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect()
}

fn last_error(context: &str) -> String {
    // SAFETY: GetLastError has no pointer preconditions.
    let code = unsafe { GetLastError() };
    format!("{context} (Windows error {code})")
}

fn create_server_pipe(name: &[u16]) -> Result<PipeHandle, String> {
    let open_mode = PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE;
    let pipe_mode = PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS;

    // SAFETY: name is NUL-terminated UTF-16. A null security attributes
    // pointer uses the creating token's default DACL, scoped to the current
    // user's access token. Remote clients are rejected explicitly.
    let raw = unsafe {
        CreateNamedPipeW(
            name.as_ptr(),
            open_mode,
            pipe_mode,
            PIPE_UNLIMITED_INSTANCES,
            PIPE_BUFFER_BYTES,
            256,
            REQUEST_TIMEOUT.as_millis() as u32,
            std::ptr::null(),
        )
    };
    PipeHandle::new(raw).map_err(|error| {
        if error.contains("Windows error 5") {
            "another Padfire instance already owns the Windows IPC pipe".to_owned()
        } else {
            error
        }
    })
}

fn connect_pipe(name: &[u16]) -> Result<PipeHandle, String> {
    // SAFETY: name is NUL-terminated; null attributes/template are valid.
    let raw = unsafe {
        CreateFileW(
            name.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            0,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            std::ptr::null_mut(),
        )
    };
    if raw != INVALID_HANDLE_VALUE {
        return PipeHandle::new(raw);
    }

    // SAFETY: GetLastError has no pointer preconditions.
    let code = unsafe { GetLastError() };
    if code == ERROR_FILE_NOT_FOUND {
        return Err("Windows IPC pipe is not available".to_owned());
    }
    if code == ERROR_PIPE_BUSY {
        // SAFETY: The pipe name is a valid NUL-terminated UTF-16 string.
        if unsafe { WaitNamedPipeW(name.as_ptr(), REQUEST_TIMEOUT.as_millis() as u32) } == 0 {
            return Err(last_error("timed out waiting for Padfire Windows IPC pipe"));
        }
        // SAFETY: Retry with the same validated name and ordinary client flags.
        let retry = unsafe {
            CreateFileW(
                name.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                0,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                std::ptr::null_mut(),
            )
        };
        return PipeHandle::new(retry);
    }
    Err(format!(
        "could not connect to Windows IPC pipe (error {code})"
    ))
}

fn serve<F>(pipe: PipeHandle, stop: Arc<AtomicBool>, handler: Arc<F>)
where
    F: Fn(IpcCommand) -> String + Send + Sync + 'static,
{
    loop {
        if stop.load(Ordering::Acquire) {
            break;
        }

        // SAFETY: pipe is a live synchronous named-pipe handle.
        let connected = unsafe { ConnectNamedPipe(pipe.raw(), std::ptr::null_mut()) };
        if connected == 0 {
            // A client may connect between pipe creation and this call.
            // ERROR_PIPE_CONNECTED means the instance is already connected.
            // SAFETY: GetLastError has no pointer preconditions.
            let code = unsafe { GetLastError() };
            if code != ERROR_PIPE_CONNECTED {
                if stop.load(Ordering::Acquire) {
                    break;
                }
                thread::sleep(Duration::from_millis(50));
                continue;
            }
        }

        if stop.load(Ordering::Acquire) {
            // SAFETY: This is a live connected pipe instance.
            unsafe { DisconnectNamedPipe(pipe.raw()) };
            break;
        }

        if let Some(frame) = read_request(pipe.raw()) {
            let response = match decode_request(&frame) {
                Ok(command) => handler(command),
                Err(error) => format!("error:{error}"),
            };
            let response = encode_response(&response)
                .unwrap_or_else(|_| b"error:response-too-large\n".to_vec());
            if write_all(pipe.raw(), &response).is_ok() {
                // DisconnectNamedPipe discards unread outbound bytes. Wait for
                // the client ACK with the same bounded request deadline.
                let _ = read_request(pipe.raw());
            }
        }

        // SAFETY: The pipe instance is connected and is reused next iteration.
        unsafe { DisconnectNamedPipe(pipe.raw()) };
    }
}

fn read_request(pipe: HANDLE) -> Option<Vec<u8>> {
    let deadline = Instant::now() + REQUEST_TIMEOUT;
    let mut frame = Vec::with_capacity(32);
    let mut byte = [0_u8; 1];

    loop {
        if Instant::now() >= deadline {
            return None;
        }
        let mut available = 0_u32;
        // SAFETY: Null buffer/read-count pointers are valid when only querying
        // the number of bytes currently buffered in the pipe.
        let peeked = unsafe {
            PeekNamedPipe(
                pipe,
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                &mut available,
                std::ptr::null_mut(),
            )
        };
        if peeked == 0 {
            return None;
        }
        if available == 0 {
            thread::sleep(Duration::from_millis(5));
            continue;
        }

        let mut bytes_read = 0_u32;
        // SAFETY: The one-byte output buffer is valid for the requested byte.
        let read = unsafe {
            ReadFile(
                pipe,
                byte.as_mut_ptr(),
                1,
                &mut bytes_read,
                std::ptr::null_mut(),
            )
        };
        if read == 0 || bytes_read == 0 {
            return None;
        }
        frame.push(byte[0]);
        if frame.len() > MAX_IPC_REQUEST_BYTES + 1 || byte[0] == b'\n' {
            return Some(frame);
        }
    }
}

fn write_all(pipe: HANDLE, bytes: &[u8]) -> Result<(), String> {
    let mut offset = 0;
    while offset < bytes.len() {
        let mut written = 0_u32;
        // SAFETY: The pointer references the remaining bytes and pipe is a
        // live synchronous duplex handle.
        let result = unsafe {
            WriteFile(
                pipe,
                bytes[offset..].as_ptr(),
                (bytes.len() - offset) as u32,
                &mut written,
                std::ptr::null_mut(),
            )
        };
        if result == 0 || written == 0 {
            return Err(last_error("could not write Padfire Windows IPC response"));
        }
        offset += written as usize;
    }
    Ok(())
}

fn read_response(pipe: HANDLE) -> Result<Option<String>, String> {
    let deadline = Instant::now() + REQUEST_TIMEOUT;
    let mut response = Vec::with_capacity(128);

    loop {
        if Instant::now() >= deadline {
            return Err("timed out waiting for the Padfire Windows IPC response".to_owned());
        }
        let mut available = 0_u32;
        // SAFETY: Null output pointers are valid when querying available bytes.
        let peeked = unsafe {
            PeekNamedPipe(
                pipe,
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                &mut available,
                std::ptr::null_mut(),
            )
        };
        if peeked == 0 {
            return if response.is_empty() {
                Ok(None)
            } else {
                Err(last_error(
                    "Windows IPC pipe closed before response delimiter",
                ))
            };
        }
        if available == 0 {
            thread::sleep(Duration::from_millis(5));
            continue;
        }

        let remaining = MAX_IPC_RESPONSE_BYTES + 1 - response.len();
        let amount = (available as usize).min(remaining).min(4096);
        let mut chunk = vec![0_u8; amount];
        let mut bytes_read = 0_u32;
        // SAFETY: chunk is writable for amount bytes; the count pointer is valid.
        let result = unsafe {
            ReadFile(
                pipe,
                chunk.as_mut_ptr(),
                amount as u32,
                &mut bytes_read,
                std::ptr::null_mut(),
            )
        };
        if result == 0 || bytes_read == 0 {
            return Err(last_error("could not read Padfire Windows IPC response"));
        }
        chunk.truncate(bytes_read as usize);
        let has_delimiter = chunk.contains(&b'\n');
        response.extend_from_slice(&chunk);
        if response.len() > MAX_IPC_RESPONSE_BYTES {
            return Err("IPC response exceeds the configured size limit".to_owned());
        }
        if has_delimiter {
            let delimiter = response.iter().position(|byte| *byte == b'\n').unwrap();
            return Ok(Some(
                String::from_utf8_lossy(&response[..delimiter])
                    .trim()
                    .to_owned(),
            ));
        }
    }
}

pub fn send(path: &Path, command: IpcCommand) -> Result<Option<String>, String> {
    let name = pipe_name(path);
    let pipe = match connect_pipe(&name) {
        Ok(pipe) => pipe,
        Err(error) if error == "Windows IPC pipe is not available" => return Ok(None),
        Err(error) => return Err(error),
    };

    let request = format!("{}\n", command.as_str());
    if request.len() > MAX_IPC_REQUEST_BYTES + 1 {
        return Err("IPC request exceeds the configured size limit".to_owned());
    }
    write_all(pipe.raw(), request.as_bytes())?;
    let response = read_response(pipe.raw())?;
    if response.is_some() {
        write_all(pipe.raw(), RESPONSE_ACK)?;
    }
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_PATH: AtomicU64 = AtomicU64::new(1);

    fn test_path() -> PathBuf {
        std::env::temp_dir().join(format!(
            "padfire-pipe-test-{}-{}",
            std::process::id(),
            NEXT_PATH.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn pipe_name_is_stable_for_one_install_and_separate_for_side_by_side_install() {
        let first = Path::new("stable/Padfire/padfire.sock");
        let second = Path::new("alpha/Padfire/padfire.sock");
        assert_eq!(pipe_name(first), pipe_name(first));
        assert_ne!(pipe_name(first), pipe_name(second));
    }

    #[test]
    fn named_pipe_round_trip_and_shutdown_release_instance_ownership() {
        let path = test_path();
        let mut server = IpcServer::spawn(&path, |_| "status-ok".to_owned()).unwrap();
        assert_eq!(
            send(&path, IpcCommand::Status).unwrap().as_deref(),
            Some("status-ok")
        );
        server.shutdown();
        assert_eq!(send(&path, IpcCommand::Status).unwrap(), None);
    }

    #[test]
    fn first_instance_flag_prevents_a_second_server() {
        let path = test_path();
        let mut server = IpcServer::spawn(&path, |_| "ok".to_owned()).unwrap();
        let error = IpcServer::spawn(&path, |_| "unexpected".to_owned()).unwrap_err();
        assert!(error.contains("already owns"));
        server.shutdown();
    }

    #[test]
    fn unknown_command_is_rejected_without_dispatching() {
        let path = test_path();
        let mut server = IpcServer::spawn(&path, |_| "unexpected".to_owned()).unwrap();
        let pipe = connect_pipe(&pipe_name(&path)).unwrap();
        write_all(pipe.raw(), b"not-a-command\n").unwrap();
        let response = read_response(pipe.raw()).unwrap().unwrap();
        assert!(response.starts_with("error:"));
        write_all(pipe.raw(), RESPONSE_ACK).unwrap();
        drop(pipe);
        server.shutdown();
    }
}
