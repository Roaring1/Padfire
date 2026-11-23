//! Single-instance local IPC transports.
//!
//! Linux uses an owner-only Unix-domain socket in the per-user config
//! directory. Windows uses a per-user named pipe (implemented separately).
//! Every request is fixed-command, newline-framed, bounded and deadline-limited.

#[cfg(unix)]
use padfire_core::ipc::{IpcCommand, MAX_IPC_CLIENTS, decode_request, encode_response};
#[cfg(unix)]
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::sync::Arc;
#[cfg(unix)]
use std::thread::JoinHandle;

#[cfg(unix)]
mod unix_transport {
    use super::*;
    use nix::unistd::Uid;
    use std::fs;
    use std::io::{Read, Write};
    use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    const REQUEST_TIMEOUT: Duration = Duration::from_secs(2);

    #[derive(Debug)]
    pub struct IpcServer {
        path: PathBuf,
        stop: Arc<AtomicBool>,
        worker: Option<JoinHandle<()>>,
    }

    impl IpcServer {
        pub fn spawn<F>(path: impl Into<PathBuf>, handler: F) -> Result<Self, String>
        where
            F: Fn(IpcCommand) -> String + Send + Sync + 'static,
        {
            let requested_path = path.into();
            let parent = requested_path
                .parent()
                .ok_or("IPC socket path has no parent")?;
            validate_socket_parent(parent)?;
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            validate_socket_parent(parent)?;
            let canonical_parent = parent
                .canonicalize()
                .map_err(|error| format!("could not canonicalize IPC socket directory: {error}"))?;
            let file_name = requested_path
                .file_name()
                .ok_or("IPC socket path has no filename")?;
            let path = canonical_parent.join(file_name);

            let stop = Arc::new(AtomicBool::new(false));
            let thread_stop = stop.clone();
            let handler = Arc::new(handler);
            let thread_path = path.clone();
            let (ready_tx, ready_rx) = mpsc::sync_channel::<Result<(), String>>(1);

            let worker = thread::Builder::new()
                .name("padfire-ipc".to_owned())
                .spawn(move || {
                    let (listener, socket_dev, socket_ino) =
                        match bind_private_listener(&thread_path) {
                            Ok(bound) => {
                                let _ = ready_tx.send(Ok(()));
                                bound
                            }
                            Err(error) => {
                                let _ = ready_tx.send(Err(error));
                                return;
                            }
                        };
                    serve(
                        listener,
                        thread_path,
                        socket_dev,
                        socket_ino,
                        thread_stop,
                        handler,
                    );
                })
                .map_err(|error| format!("could not start IPC thread: {error}"))?;

            match ready_rx.recv_timeout(Duration::from_secs(3)) {
                Ok(Ok(())) => Ok(Self {
                    path,
                    stop,
                    worker: Some(worker),
                }),
                Ok(Err(error)) => {
                    let _ = worker.join();
                    Err(error)
                }
                Err(error) => {
                    stop.store(true, Ordering::Release);
                    let _ = UnixStream::connect(&path);
                    let _ = worker.join();
                    Err(format!("IPC listener did not become ready: {error}"))
                }
            }
        }

        pub fn path(&self) -> &Path {
            &self.path
        }

        pub fn shutdown(&mut self) {
            self.stop.store(true, Ordering::Release);
            // Wake the blocking accept call. The server checks the stop flag
            // before dispatching a request from this connection.
            let _ = UnixStream::connect(&self.path);
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

    fn validate_socket_parent(parent: &Path) -> Result<(), String> {
        use std::path::Component;

        if !parent.is_absolute() {
            return Err("IPC socket directory must be absolute".to_owned());
        }
        if parent
            .components()
            .any(|component| matches!(component, Component::ParentDir))
        {
            return Err(
                "IPC socket directory must not contain parent-directory components".to_owned(),
            );
        }

        let mut current = PathBuf::new();
        for component in parent.components() {
            current.push(component.as_os_str());
            match fs::symlink_metadata(&current) {
                Ok(metadata) => {
                    if !metadata.is_dir() || metadata.file_type().is_symlink() {
                        return Err(format!(
                            "refusing a non-directory or symlink in IPC socket path: {}",
                            current.display()
                        ));
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
                Err(error) => {
                    return Err(format!(
                        "could not inspect IPC socket path component {}: {error}",
                        current.display()
                    ));
                }
            }
        }
        Ok(())
    }

    fn bind_private_listener(path: &Path) -> Result<(UnixListener, u64, u64), String> {
        if let Ok(previous) = fs::symlink_metadata(path) {
            if !previous.file_type().is_socket() {
                return Err(format!(
                    "refusing to replace non-socket IPC path: {}",
                    path.display()
                ));
            }
            if previous.uid() != Uid::current().as_raw() {
                return Err(format!(
                    "refusing socket owned by another user: {}",
                    path.display()
                ));
            }

            match UnixStream::connect(path) {
                Ok(_) => {
                    return Err("another Padfire instance already owns the IPC socket".to_owned());
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::NotFound
                    ) =>
                {
                    let current = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
                    if !current.file_type().is_socket()
                        || current.dev() != previous.dev()
                        || current.ino() != previous.ino()
                        || current.uid() != Uid::current().as_raw()
                    {
                        return Err("IPC socket changed during stale-path inspection".to_owned());
                    }
                    fs::remove_file(path).map_err(|e| e.to_string())?;
                }
                Err(error) => {
                    return Err(format!(
                        "could not safely inspect existing IPC socket: {error}"
                    ));
                }
            }
        }

        let listener = UnixListener::bind(path)
            .map_err(|error| format!("could not bind IPC socket {}: {error}", path.display()))?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .map_err(|error| format!("could not secure IPC socket permissions: {error}"))?;
        let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
        if !metadata.file_type().is_socket() || metadata.uid() != Uid::current().as_raw() {
            return Err("new IPC socket failed owner/type verification".to_owned());
        }
        Ok((listener, metadata.dev(), metadata.ino()))
    }

    fn serve<F>(
        listener: UnixListener,
        path: PathBuf,
        socket_dev: u64,
        socket_ino: u64,
        stop: Arc<AtomicBool>,
        handler: Arc<F>,
    ) where
        F: Fn(IpcCommand) -> String + Send + Sync + 'static,
    {
        let active = Arc::new(AtomicUsize::new(0));
        let mut clients: Vec<JoinHandle<()>> = Vec::new();

        while !stop.load(Ordering::Acquire) {
            reap_clients(&mut clients);
            match listener.accept() {
                Ok((stream, _address)) => {
                    if stop.load(Ordering::Acquire) {
                        break;
                    }
                    if active.load(Ordering::Acquire) >= MAX_IPC_CLIENTS {
                        let mut stream = stream;
                        let _ = stream.write_all(b"busy\n");
                        continue;
                    }

                    active.fetch_add(1, Ordering::AcqRel);
                    let active_client = active.clone();
                    let handler = handler.clone();
                    let client = thread::Builder::new()
                        .name("padfire-ipc-client".to_owned())
                        .spawn(move || {
                            let _active_guard = ActiveGuard(active_client);
                            handle_client(stream, handler);
                        });
                    if let Ok(client) = client {
                        clients.push(client);
                    } else {
                        active.fetch_sub(1, Ordering::AcqRel);
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => {
                    eprintln!("[ipc] accept failed: {error}");
                    break;
                }
            }
        }

        for client in clients {
            let _ = client.join();
        }

        // Remove only the socket inode this server created. Never unlink a
        // replacement path created by another process during shutdown.
        if let Ok(current) = fs::symlink_metadata(&path)
            && current.file_type().is_socket()
            && current.uid() == Uid::current().as_raw()
            && current.dev() == socket_dev
            && current.ino() == socket_ino
        {
            let _ = fs::remove_file(&path);
        }
    }

    struct ActiveGuard(Arc<AtomicUsize>);

    impl Drop for ActiveGuard {
        fn drop(&mut self) {
            self.0.fetch_sub(1, Ordering::AcqRel);
        }
    }

    fn reap_clients(clients: &mut Vec<JoinHandle<()>>) {
        let mut index = 0;
        while index < clients.len() {
            if clients[index].is_finished() {
                let client = clients.swap_remove(index);
                let _ = client.join();
            } else {
                index += 1;
            }
        }
    }

    fn handle_client<F>(mut stream: UnixStream, handler: Arc<F>)
    where
        F: Fn(IpcCommand) -> String,
    {
        let _ = stream.set_read_timeout(Some(REQUEST_TIMEOUT));
        let _ = stream.set_write_timeout(Some(REQUEST_TIMEOUT));

        let mut frame = Vec::with_capacity(32);
        let mut byte = [0_u8; 1];
        loop {
            match stream.read(&mut byte) {
                Ok(0) => return,
                Ok(_) => {
                    frame.push(byte[0]);
                    if frame.len() > 257 {
                        let _ = stream.write_all(b"error:too-large\n");
                        return;
                    }
                    if byte[0] == b'\n' {
                        break;
                    }
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::Interrupted | std::io::ErrorKind::WouldBlock
                    ) =>
                {
                    continue;
                }
                Err(_) => return,
            }
        }

        let response = match decode_request(&frame) {
            Ok(command) => handler(command),
            Err(error) => format!("error:{error}"),
        };
        let Ok(response) = encode_response(&response) else {
            let _ = stream.write_all(b"error:response-too-large\n");
            return;
        };
        let _ = stream.write_all(&response);
    }

    pub fn send(path: &Path, command: IpcCommand) -> Result<Option<String>, String> {
        let parent = path.parent().ok_or("IPC socket path has no parent")?;
        if !parent.exists() {
            return Ok(None);
        }
        validate_socket_parent(parent)?;
        let canonical_parent = parent
            .canonicalize()
            .map_err(|error| format!("could not canonicalize IPC socket directory: {error}"))?;
        let file_name = path.file_name().ok_or("IPC socket path has no filename")?;
        let path = canonical_parent.join(file_name);
        let mut stream = match UnixStream::connect(&path) {
            Ok(stream) => stream,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) if error.kind() == std::io::ErrorKind::ConnectionRefused => return Ok(None),
            Err(error) => return Err(error.to_string()),
        };
        stream
            .set_read_timeout(Some(REQUEST_TIMEOUT))
            .map_err(|e| e.to_string())?;
        stream
            .set_write_timeout(Some(REQUEST_TIMEOUT))
            .map_err(|e| e.to_string())?;
        stream
            .write_all(format!("{}\n", command.as_str()).as_bytes())
            .map_err(|error| error.to_string())?;

        let mut response = Vec::with_capacity(128);
        let mut byte = [0_u8; 1];
        loop {
            match stream.read(&mut byte) {
                Ok(0) => break,
                Ok(_) => {
                    response.push(byte[0]);
                    if response.len() > 64 * 1024 {
                        return Err("IPC response exceeds the configured size limit".to_owned());
                    }
                    if byte[0] == b'\n' {
                        break;
                    }
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::Interrupted | std::io::ErrorKind::WouldBlock
                    ) =>
                {
                    continue;
                }
                Err(error) => return Err(error.to_string()),
            }
        }
        if response.is_empty() {
            return Ok(None);
        }
        let response = String::from_utf8_lossy(&response);
        Ok(Some(response.trim().to_owned()))
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::sync::atomic::{AtomicU64, Ordering};
        use std::time::SystemTime;

        static NEXT_ID: AtomicU64 = AtomicU64::new(0);

        fn test_path() -> PathBuf {
            let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
            std::env::temp_dir().join(format!(
                "padfire-ipc-{}-{}-{}.sock",
                std::process::id(),
                SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                id
            ))
        }

        #[test]
        fn socket_paths_reject_relative_and_parent_traversal_directories() {
            assert!(validate_socket_parent(Path::new("relative")).is_err());
            assert!(validate_socket_parent(Path::new("/tmp/../var/tmp")).is_err());
        }

        #[cfg(unix)]
        #[test]
        fn socket_paths_reject_symlinked_parent_directories() {
            use std::os::unix::fs::symlink;

            let root = test_path().with_extension("dir");
            let target = root.join("target");
            let alias = root.join("alias");
            fs::create_dir_all(&target).unwrap();
            symlink(&target, &alias).unwrap();

            assert!(validate_socket_parent(&alias).is_err());
            assert_eq!(fs::read_dir(&target).unwrap().count(), 0);

            let _ = fs::remove_dir_all(root);
        }

        #[test]
        fn owner_only_socket_serves_fixed_commands_and_removes_itself_on_shutdown() {
            let path = test_path();
            let mut server = IpcServer::spawn(&path, |command| match command {
                IpcCommand::Status => r#"{"connected":false,"page":0}"#.to_owned(),
                _ => "ok".to_owned(),
            })
            .unwrap();

            let response = send(&path, IpcCommand::Status).unwrap().unwrap();
            assert_eq!(response, r#"{"connected":false,"page":0}"#);
            assert_eq!(
                send(&path, IpcCommand::StopAll).unwrap().as_deref(),
                Some("ok")
            );

            let metadata = fs::symlink_metadata(&path).unwrap();
            assert!(metadata.file_type().is_socket());
            assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
            assert_eq!(metadata.uid(), Uid::current().as_raw());

            server.shutdown();
            assert!(!path.exists());
        }

        #[test]
        fn live_second_instance_is_refused() {
            let path = test_path();
            let mut first = IpcServer::spawn(&path, |_| "ok".to_owned()).unwrap();
            let second = IpcServer::spawn(&path, |_| "wrong".to_owned());
            assert!(second.unwrap_err().contains("already owns"));
            first.shutdown();
        }

        #[test]
        fn stale_socket_is_removed_but_non_socket_paths_are_never_unlinked() {
            let stale = test_path();
            let _listener = UnixListener::bind(&stale).unwrap();
            drop(_listener);
            let mut server = IpcServer::spawn(&stale, |_| "ok".to_owned()).unwrap();
            server.shutdown();

            let protected = test_path();
            fs::write(&protected, b"keep").unwrap();
            assert!(IpcServer::spawn(&protected, |_| "bad".to_owned()).is_err());
            assert_eq!(fs::read(&protected).unwrap(), b"keep");
            let _ = fs::remove_file(protected);
        }

        #[test]
        fn incomplete_or_unknown_requests_are_not_dispatched() {
            let path = test_path();
            let calls = Arc::new(AtomicUsize::new(0));
            let target = calls.clone();
            let mut server = IpcServer::spawn(&path, move |_| {
                target.fetch_add(1, Ordering::Relaxed);
                "ok".to_owned()
            })
            .unwrap();

            let mut stream = UnixStream::connect(&path).unwrap();
            stream.write_all(b"not-a-command\n").unwrap();
            let mut response = String::new();
            stream.read_to_string(&mut response).unwrap();
            assert!(response.starts_with("error:unknown IPC command"));
            assert_eq!(calls.load(Ordering::Relaxed), 0);
            server.shutdown();
        }
    }
}

#[cfg(windows)]
#[allow(unsafe_code)]
#[path = "ipc_server_windows.rs"]
mod windows_transport;

#[cfg(not(any(unix, windows)))]
mod unsupported_transport {
    use super::*;
    pub struct IpcServer;
    impl IpcServer {
        pub fn spawn<F>(_path: impl Into<PathBuf>, _handler: F) -> Result<Self, String>
        where
            F: Fn(IpcCommand) -> String + Send + Sync + 'static,
        {
            Err("local IPC is unsupported on this platform".to_owned())
        }
        pub fn path(&self) -> &Path {
            Path::new("")
        }
        pub fn shutdown(&mut self) {}
    }
    pub fn send(_path: &Path, _command: IpcCommand) -> Result<Option<String>, String> {
        Err("local IPC is unsupported on this platform".to_owned())
    }
}

#[cfg(unix)]
pub use unix_transport::{IpcServer, send};
#[cfg(not(any(unix, windows)))]
pub use unsupported_transport::{IpcServer, send};
#[cfg(windows)]
pub use windows_transport::{IpcServer, send};
