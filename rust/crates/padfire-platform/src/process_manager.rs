//! Process supervision for trusted script pads and Crosshair.
//!
//! User-authored script commands intentionally run through the platform shell.
//! Each gets a process group (Unix) or Job Object (Windows), a generation
//! token, a hard deadline and bounded output capture. Crosshair is different:
//! it is always invoked directly with the --toggle argument, never through a shell.

use padfire_core::PadKey;
use padfire_core::runtime::{GenerationTracker, OperationToken};
use std::collections::HashMap;
use std::io::{self, BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const SCRIPT_TIMEOUT: Duration = Duration::from_secs(600);
const TEST_TIMEOUT: Duration = Duration::from_secs(30);
const CROSSHAIR_TIMEOUT: Duration = Duration::from_secs(5);
const POLL_INTERVAL: Duration = Duration::from_millis(100);
const MAX_OUTPUT_LINES: usize = 10;
const MAX_OUTPUT_LINE_CHARS: usize = 512;

/// Return the subset of process IDs that currently exist, without signalling or
/// adopting any process. A single Windows snapshot covers the whole batch.
#[allow(unsafe_code)]
pub fn process_ids_alive(process_ids: &[u32]) -> std::collections::HashSet<u32> {
    let wanted = process_ids
        .iter()
        .copied()
        .filter(|process_id| *process_id != 0)
        .collect::<std::collections::HashSet<_>>();
    if wanted.is_empty() {
        return std::collections::HashSet::new();
    }

    #[cfg(target_os = "linux")]
    {
        wanted
            .into_iter()
            .filter(|process_id| {
                if *process_id > i32::MAX as u32 {
                    return false;
                }
                // SAFETY: kill(pid, 0) performs an existence/permission check only.
                let result = unsafe { libc::kill(*process_id as libc::pid_t, 0) };
                result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
            })
            .collect()
    }

    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
        use windows_sys::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
            TH32CS_SNAPPROCESS,
        };

        // Toolhelp enumeration works for protected/differently privileged PIDs
        // without requesting a handle to each target process.
        // SAFETY: snapshot uses the documented process-enumeration flag.
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        if snapshot == INVALID_HANDLE_VALUE {
            return std::collections::HashSet::new();
        }
        struct Snapshot(windows_sys::Win32::Foundation::HANDLE);
        impl Drop for Snapshot {
            fn drop(&mut self) {
                // SAFETY: this wrapper owns the snapshot handle.
                unsafe { CloseHandle(self.0) };
            }
        }
        let _snapshot = Snapshot(snapshot);
        let mut alive = std::collections::HashSet::new();
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        // SAFETY: entry is initialized with the required size and is writable.
        let mut has_entry = unsafe { Process32FirstW(snapshot, &mut entry) } != 0;
        while has_entry {
            if wanted.contains(&entry.th32ProcessID) {
                alive.insert(entry.th32ProcessID);
                if alive.len() == wanted.len() {
                    break;
                }
            }
            // SAFETY: entry remains a valid PROCESSENTRY32W output structure.
            has_entry = unsafe { Process32NextW(snapshot, &mut entry) } != 0;
        }
        alive
    }

    #[cfg(not(any(target_os = "linux", windows)))]
    {
        let _ = wanted;
        std::collections::HashSet::new()
    }
}

pub fn process_id_is_alive(process_id: u32) -> bool {
    process_ids_alive(&[process_id]).contains(&process_id)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessKind {
    Script,
    ScriptTest,
    Crosshair,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessEvent {
    Started {
        pad: Option<PadKey>,
        kind: ProcessKind,
    },
    Output {
        pad: Option<PadKey>,
        kind: ProcessKind,
        line: String,
    },
    Exited {
        pad: Option<PadKey>,
        kind: ProcessKind,
        code: Option<i32>,
    },
    TimedOut {
        pad: Option<PadKey>,
        kind: ProcessKind,
    },
    Stopped {
        pad: Option<PadKey>,
        kind: ProcessKind,
    },
    Failed {
        pad: Option<PadKey>,
        kind: ProcessKind,
        message: String,
    },
}

enum ProcessCommand {
    StartScript {
        token: OperationToken,
        pad: PadKey,
        command: String,
        test_run: bool,
    },
    StopScript {
        pad: PadKey,
    },
    StopAllScripts,
    ToggleCrosshair {
        pad: PadKey,
    },
    Shutdown,
}

pub struct ProcessEngine {
    sender: SyncSender<ProcessCommand>,
    worker: Option<JoinHandle<()>>,
}

impl ProcessEngine {
    pub fn spawn(
        generations: Arc<Mutex<GenerationTracker>>,
        events: Sender<ProcessEvent>,
    ) -> Result<Self, String> {
        let (sender, receiver) = mpsc::sync_channel(64);
        let worker = thread::Builder::new()
            .name("padfire-process".to_owned())
            .spawn(move || ProcessActor::new(generations, events).run(receiver))
            .map_err(|error| format!("could not start process supervisor: {error}"))?;
        Ok(Self {
            sender,
            worker: Some(worker),
        })
    }

    fn enqueue(&self, command: ProcessCommand) -> Result<(), String> {
        self.sender.try_send(command).map_err(|error| match error {
            TrySendError::Full(_) => "process command queue is full".to_owned(),
            TrySendError::Disconnected(_) => "process supervisor is not running".to_owned(),
        })
    }

    pub fn start_script(
        &self,
        token: OperationToken,
        pad: PadKey,
        command: impl Into<String>,
        test_run: bool,
    ) -> Result<(), String> {
        self.enqueue(ProcessCommand::StartScript {
            token,
            pad,
            command: command.into(),
            test_run,
        })
    }

    pub fn stop_script(&self, pad: PadKey) -> Result<(), String> {
        self.enqueue(ProcessCommand::StopScript { pad })
    }

    pub fn stop_all_scripts(&self) -> Result<(), String> {
        self.enqueue(ProcessCommand::StopAllScripts)
    }

    pub fn toggle_crosshair(&self, pad: PadKey) -> Result<(), String> {
        self.enqueue(ProcessCommand::ToggleCrosshair { pad })
    }

    pub fn shutdown(&mut self) {
        let _ = self.sender.send(ProcessCommand::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for ProcessEngine {
    fn drop(&mut self) {
        self.shutdown();
    }
}

struct ManagedProcess {
    pad: Option<PadKey>,
    kind: ProcessKind,
    child: Child,
    deadline: Instant,
    output_readers: Vec<JoinHandle<()>>,
    #[cfg(windows)]
    job: winjob::JobGuard,
}

struct ProcessActor {
    generations: Arc<Mutex<GenerationTracker>>,
    events: Sender<ProcessEvent>,
    active: HashMap<String, ManagedProcess>,
    next_id: u64,
}

impl ProcessActor {
    fn new(generations: Arc<Mutex<GenerationTracker>>, events: Sender<ProcessEvent>) -> Self {
        Self {
            generations,
            events,
            active: HashMap::new(),
            next_id: 0,
        }
    }

    fn run(mut self, receiver: Receiver<ProcessCommand>) {
        loop {
            let message = if self.active.is_empty() {
                receiver.recv().map_err(|_| RecvTimeoutError::Disconnected)
            } else {
                receiver.recv_timeout(POLL_INTERVAL)
            };
            match message {
                Ok(ProcessCommand::StartScript {
                    token,
                    pad,
                    command,
                    test_run,
                }) => {
                    self.start_script(token, pad, command, test_run);
                }
                Ok(ProcessCommand::StopScript { pad }) => self.stop_script(pad),
                Ok(ProcessCommand::StopAllScripts) => self.stop_all_scripts(),
                Ok(ProcessCommand::ToggleCrosshair { pad }) => self.toggle_crosshair(pad),
                Ok(ProcessCommand::Shutdown) | Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) => self.poll_children(),
            }
        }
        self.stop_all();
    }

    fn token_is_current(&self, token: &OperationToken) -> bool {
        self.generations
            .lock()
            .map(|tracker| tracker.is_current(token))
            .unwrap_or(false)
    }

    fn start_script(
        &mut self,
        token: OperationToken,
        pad: PadKey,
        command_line: String,
        test_run: bool,
    ) {
        let key = format!("script:{}", pad.to_key());
        if !self.token_is_current(&token) {
            return;
        }
        self.terminate_by_key(&key);

        let command_line = command_line.trim();
        if command_line.is_empty() {
            self.fail(Some(pad), ProcessKind::Script, "script command is empty");
            return;
        }

        #[cfg(unix)]
        let mut command = {
            use std::os::unix::process::CommandExt;
            let mut command = Command::new("/bin/sh");
            command.arg("-c").arg(command_line).process_group(0);
            command
        };

        #[cfg(windows)]
        let mut command = {
            use std::os::windows::process::CommandExt;
            let mut command = Command::new("cmd.exe");
            // cmd.exe uses a different quote parser from the Windows CRT.
            // Pass the shell payload verbatim instead of allowing std::process
            // to escape embedded quotes as `\"`, which cmd treats literally.
            command
                .arg("/D")
                .arg("/C")
                .raw_arg(command_line)
                .creation_flags(winjob::CREATE_NEW_PROCESS_GROUP | winjob::CREATE_SUSPENDED);
            command
        };

        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        #[cfg(windows)]
        let job = match winjob::JobGuard::new() {
            Ok(job) => job,
            Err(error) => {
                self.fail(Some(pad), ProcessKind::Script, error);
                return;
            }
        };

        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                self.fail(
                    Some(pad),
                    ProcessKind::Script,
                    format!("could not start script: {error}"),
                );
                return;
            }
        };

        #[cfg(windows)]
        if let Err(error) = job.assign(&child) {
            job.terminate();
            let _ = child.wait();
            self.fail(Some(pad), ProcessKind::Script, error);
            return;
        }

        #[cfg(windows)]
        if !self.token_is_current(&token) {
            // The child is still suspended, so a cancelled generation never
            // gets a chance to execute even its first command.
            job.terminate();
            let _ = child.wait();
            return;
        }

        #[cfg(windows)]
        if let Err(error) = job.resume(&child) {
            job.terminate();
            let _ = child.wait();
            self.fail(Some(pad), ProcessKind::Script, error);
            return;
        }

        if !self.token_is_current(&token) {
            let group = process_group_id(child.id());
            terminate_child(&mut child, group);
            #[cfg(windows)]
            drop(job);
            return;
        }

        let kind = if test_run {
            ProcessKind::ScriptTest
        } else {
            ProcessKind::Script
        };
        let deadline = Instant::now()
            + if test_run {
                TEST_TIMEOUT
            } else {
                SCRIPT_TIMEOUT
            };
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        // Publish the state transition before reader threads can deliver output.
        // Otherwise a fast child can race an Output event ahead of Started.
        let _ = self.events.send(ProcessEvent::Started {
            pad: Some(pad),
            kind,
        });
        let mut output_readers = Vec::with_capacity(2);
        if let Some(stdout) = stdout {
            match spawn_output_reader(stdout, Some(pad), kind, self.events.clone()) {
                Ok(reader) => output_readers.push(reader),
                Err(error) => {
                    let group = process_group_id(child.id());
                    terminate_child(&mut child, group);
                    #[cfg(windows)]
                    job.terminate();
                    join_output_readers(&mut output_readers);
                    self.fail(
                        Some(pad),
                        kind,
                        format!("could not start stdout reader: {error}"),
                    );
                    return;
                }
            }
        }
        if let Some(stderr) = stderr {
            match spawn_output_reader(stderr, Some(pad), kind, self.events.clone()) {
                Ok(reader) => output_readers.push(reader),
                Err(error) => {
                    let group = process_group_id(child.id());
                    terminate_child(&mut child, group);
                    #[cfg(windows)]
                    job.terminate();
                    join_output_readers(&mut output_readers);
                    self.fail(
                        Some(pad),
                        kind,
                        format!("could not start stderr reader: {error}"),
                    );
                    return;
                }
            }
        }
        self.active.insert(
            key,
            ManagedProcess {
                pad: Some(pad),
                kind,
                child,
                deadline,
                output_readers,
                #[cfg(windows)]
                job,
            },
        );
    }

    fn toggle_crosshair(&mut self, pad: PadKey) {
        let Some(executable) = resolve_crosshair_executable() else {
            self.fail(
                Some(pad),
                ProcessKind::Crosshair,
                "Crosshair CLI was not found on PATH or in its user-local install path",
            );
            return;
        };

        let key = format!("crosshair:{}:{}", pad.to_key(), self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        let mut command = Command::new(&executable);
        command
            .arg("--toggle")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(
                winjob::CREATE_NO_WINDOW
                    | winjob::CREATE_NEW_PROCESS_GROUP
                    | winjob::CREATE_SUSPENDED,
            );
        }

        #[cfg(windows)]
        let job = match winjob::JobGuard::new() {
            Ok(job) => job,
            Err(error) => {
                self.fail(Some(pad), ProcessKind::Crosshair, error);
                return;
            }
        };

        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                self.fail(
                    Some(pad),
                    ProcessKind::Crosshair,
                    format!("could not start {}: {error}", executable.display()),
                );
                return;
            }
        };

        #[cfg(windows)]
        if let Err(error) = job.assign(&child).and_then(|()| job.resume(&child)) {
            job.terminate();
            let _ = child.kill();
            let _ = child.wait();
            self.fail(Some(pad), ProcessKind::Crosshair, error);
            return;
        }

        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let _ = self.events.send(ProcessEvent::Started {
            pad: Some(pad),
            kind: ProcessKind::Crosshair,
        });
        let mut output_readers = Vec::with_capacity(2);
        if let Some(stdout) = stdout {
            match spawn_output_reader(
                stdout,
                Some(pad),
                ProcessKind::Crosshair,
                self.events.clone(),
            ) {
                Ok(reader) => output_readers.push(reader),
                Err(error) => {
                    let group = process_group_id(child.id());
                    terminate_child(&mut child, group);
                    #[cfg(windows)]
                    job.terminate();
                    join_output_readers(&mut output_readers);
                    self.fail(
                        Some(pad),
                        ProcessKind::Crosshair,
                        format!("could not start stdout reader: {error}"),
                    );
                    return;
                }
            }
        }
        if let Some(stderr) = stderr {
            match spawn_output_reader(
                stderr,
                Some(pad),
                ProcessKind::Crosshair,
                self.events.clone(),
            ) {
                Ok(reader) => output_readers.push(reader),
                Err(error) => {
                    let group = process_group_id(child.id());
                    terminate_child(&mut child, group);
                    #[cfg(windows)]
                    job.terminate();
                    join_output_readers(&mut output_readers);
                    self.fail(
                        Some(pad),
                        ProcessKind::Crosshair,
                        format!("could not start stderr reader: {error}"),
                    );
                    return;
                }
            }
        }
        self.active.insert(
            key,
            ManagedProcess {
                pad: Some(pad),
                kind: ProcessKind::Crosshair,
                child,
                deadline: Instant::now() + CROSSHAIR_TIMEOUT,
                output_readers,
                #[cfg(windows)]
                job,
            },
        );
    }

    fn stop_script(&mut self, pad: PadKey) {
        self.terminate_by_key(&format!("script:{}", pad.to_key()));
    }

    fn stop_all_scripts(&mut self) {
        let keys: Vec<String> = self
            .active
            .iter()
            .filter(|(_, process)| {
                matches!(process.kind, ProcessKind::Script | ProcessKind::ScriptTest)
            })
            .map(|(key, _)| key.clone())
            .collect();
        for key in keys {
            self.terminate_by_key(&key);
        }
    }

    fn stop_all(&mut self) {
        let keys: Vec<String> = self.active.keys().cloned().collect();
        for key in keys {
            self.terminate_by_key(&key);
        }
    }

    fn terminate_by_key(&mut self, key: &str) {
        if let Some(mut process) = self.active.remove(key) {
            let pad = process.pad;
            let kind = process.kind;
            terminate_managed(&mut process);
            let _ = self.events.send(ProcessEvent::Stopped { pad, kind });
        }
    }

    fn poll_children(&mut self) {
        let keys: Vec<String> = self.active.keys().cloned().collect();
        for key in keys {
            let Some(process) = self.active.get_mut(&key) else {
                continue;
            };

            if Instant::now() >= process.deadline {
                if let Some(mut process) = self.active.remove(&key) {
                    let pad = process.pad;
                    let kind = process.kind;
                    terminate_managed(&mut process);
                    let _ = self.events.send(ProcessEvent::TimedOut { pad, kind });
                }
                continue;
            }

            match process.child.try_wait() {
                Ok(Some(status)) => {
                    if let Some(mut process) = self.active.remove(&key) {
                        let pad = process.pad;
                        let kind = process.kind;
                        terminate_managed(&mut process);
                        let _ = self.events.send(ProcessEvent::Exited {
                            pad,
                            kind,
                            code: status.code(),
                        });
                    }
                }
                Ok(None) => {}
                Err(error) => {
                    if let Some(mut process) = self.active.remove(&key) {
                        let pad = process.pad;
                        let kind = process.kind;
                        terminate_managed(&mut process);
                        self.fail(
                            pad,
                            kind,
                            format!("could not query child process state: {error}"),
                        );
                    }
                }
            }
        }
    }

    fn fail(&self, pad: Option<PadKey>, kind: ProcessKind, message: impl Into<String>) {
        let _ = self.events.send(ProcessEvent::Failed {
            pad,
            kind,
            message: message.into(),
        });
    }
}

fn read_bounded_line<R: BufRead>(
    reader: &mut R,
    buffer: &mut Vec<u8>,
    maximum_bytes: usize,
) -> io::Result<Option<String>> {
    buffer.clear();
    let mut saw_anything = false;
    let mut truncated = false;

    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            if !saw_anything {
                return Ok(None);
            }
            break;
        }

        saw_anything = true;
        let newline = available.iter().position(|byte| *byte == b'\n');
        let consumed = newline.map_or(available.len(), |index| index + 1);
        let payload_len = newline.unwrap_or(available.len());
        let room = maximum_bytes.saturating_sub(buffer.len());
        let take = payload_len.min(room);
        buffer.extend_from_slice(&available[..take]);
        if take < payload_len {
            truncated = true;
        }
        reader.consume(consumed);

        if newline.is_some() {
            break;
        }
    }

    if buffer.last() == Some(&b'\r') {
        buffer.pop();
    }
    let mut line = String::from_utf8_lossy(buffer).into_owned();
    if truncated || line.len() > maximum_bytes {
        while line.len() > maximum_bytes {
            line.pop();
        }
    }
    Ok(Some(line))
}

fn spawn_output_reader<R>(
    reader: R,
    pad: Option<PadKey>,
    kind: ProcessKind,
    events: Sender<ProcessEvent>,
) -> io::Result<JoinHandle<()>>
where
    R: std::io::Read + Send + 'static,
{
    thread::Builder::new()
        .name("padfire-script-output".to_owned())
        .spawn(move || {
            let mut reader = BufReader::new(reader);
            let mut buffer = Vec::with_capacity(MAX_OUTPUT_LINE_CHARS);
            for index in 0.. {
                let line = match read_bounded_line(&mut reader, &mut buffer, MAX_OUTPUT_LINE_CHARS)
                {
                    Ok(Some(line)) => line,
                    Ok(None) | Err(_) => break,
                };
                if index >= MAX_OUTPUT_LINES {
                    continue; // keep draining the pipe without retaining more output
                }
                let _ = events.send(ProcessEvent::Output { pad, kind, line });
            }
        })
}

fn join_output_readers(readers: &mut Vec<JoinHandle<()>>) {
    for reader in readers.drain(..) {
        let _ = reader.join();
    }
}

pub fn resolve_crosshair_executable() -> Option<std::path::PathBuf> {
    let name = if cfg!(windows) {
        "crosshair.exe"
    } else {
        "crosshair"
    };
    if let Some(path) = find_on_path(name) {
        return Some(path);
    }

    #[cfg(unix)]
    if let Some(home) = std::env::var_os("HOME") {
        let candidate = std::path::PathBuf::from(home).join(".local/bin").join(name);
        if is_executable_file(&candidate) {
            return Some(candidate);
        }
    }

    #[cfg(windows)]
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        let candidate = std::path::PathBuf::from(local)
            .join("Programs/Crosshair")
            .join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn find_on_path(name: &str) -> Option<std::path::PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|directory| directory.join(name))
        .find(|candidate| is_executable_file(candidate))
}

fn is_executable_file(path: &std::path::Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.is_file()
            && std::fs::metadata(path)
                .ok()
                .is_some_and(|metadata| metadata.permissions().mode() & 0o111 != 0)
    }
    #[cfg(windows)]
    {
        path.is_file()
    }
}

fn terminate_managed(process: &mut ManagedProcess) {
    let group = process_group_id(process.child.id());
    terminate_child(&mut process.child, group);
    #[cfg(windows)]
    process.job.terminate();
    // All writer processes are now stopped, so each pipe reaches EOF. Join
    // both readers before publishing Stopped/Exited/TimedOut or dropping the
    // process record; shutdown therefore owns the full thread lifecycle.
    join_output_readers(&mut process.output_readers);
}

#[cfg(unix)]
fn process_group_id(child_id: u32) -> Option<i32> {
    i32::try_from(child_id).ok()
}

#[cfg(not(unix))]
fn process_group_id(_child_id: u32) -> Option<i32> {
    None
}

#[cfg(unix)]
fn terminate_child(child: &mut Child, group: Option<i32>) {
    use nix::sys::signal::{Signal, killpg};
    use nix::unistd::Pid;

    if let Some(group) = group {
        let _ = killpg(Pid::from_raw(group), Signal::SIGTERM);
    } else {
        let _ = child.kill();
    }
    let until = Instant::now() + Duration::from_millis(300);
    while Instant::now() < until {
        if child.try_wait().ok().flatten().is_some() {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    if let Some(group) = group {
        let _ = killpg(Pid::from_raw(group), Signal::SIGKILL);
    } else {
        let _ = child.kill();
    }
    let _ = child.wait();
}

#[cfg(windows)]
fn terminate_child(child: &mut Child, _group: Option<i32>) {
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(windows)]
mod winjob {
    #![allow(unsafe_code)]

    use std::mem::{size_of, zeroed};
    use std::os::windows::io::AsRawHandle;
    use std::process::Child;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject, TerminateJobObject,
    };

    pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    pub const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    pub const CREATE_SUSPENDED: u32 = 0x0000_0004;

    pub struct JobGuard {
        handle: HANDLE,
    }

    impl JobGuard {
        pub fn new() -> Result<Self, String> {
            // SAFETY: Null attributes/name request a new unnamed job. Handle is checked below.
            let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
            if handle.is_null() {
                return Err(std::io::Error::last_os_error().to_string());
            }

            // SAFETY: The job information structure is a POD type with a valid all-zero state.
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { zeroed() };
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;

            // SAFETY: The pointer and byte count describe the initialized structure for this call.
            let result = unsafe {
                SetInformationJobObject(
                    handle,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const std::ffi::c_void,
                    size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )
            };
            if result == 0 {
                let error = std::io::Error::last_os_error();
                // SAFETY: Handle is a valid Job Object returned by CreateJobObjectW.
                unsafe { CloseHandle(handle) };
                return Err(error.to_string());
            }
            Ok(Self { handle })
        }

        pub fn assign(&self, child: &Child) -> Result<(), String> {
            // SAFETY: Child holds a live process handle for the duration of this call.
            let result =
                unsafe { AssignProcessToJobObject(self.handle, child.as_raw_handle() as HANDLE) };
            if result == 0 {
                Err(std::io::Error::last_os_error().to_string())
            } else {
                Ok(())
            }
        }

        /// Resume the initial thread only after the process belongs to this job.
        /// This closes the spawn/assignment race in which a fast script could
        /// otherwise create grandchildren before Windows applies job limits.
        pub fn resume(&self, child: &Child) -> Result<(), String> {
            resume_suspended_process(child.id())
        }

        pub fn terminate(&self) {
            // SAFETY: The Job Object handle remains valid for this guard.
            unsafe { TerminateJobObject(self.handle, 1) };
        }
    }

    impl Drop for JobGuard {
        fn drop(&mut self) {
            // SAFETY: This owning guard closes its handle exactly once.
            unsafe { CloseHandle(self.handle) };
        }
    }

    #[allow(unsafe_code)]
    fn resume_suspended_process(process_id: u32) -> Result<(), String> {
        use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
        use windows_sys::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
        };
        use windows_sys::Win32::System::Threading::{
            OpenThread, ResumeThread, THREAD_SUSPEND_RESUME,
        };

        struct OwnedHandle(HANDLE);
        impl Drop for OwnedHandle {
            fn drop(&mut self) {
                // SAFETY: This wrapper owns a valid handle and closes it once.
                unsafe { CloseHandle(self.0) };
            }
        }

        // The process is still suspended, so its initial thread is the only
        // thread it can own. Enumerating it lets std::process::Child remain
        // responsible for process lifetime without leaking a primary thread
        // handle out of the platform abstraction.
        // SAFETY: Snapshot flags request the system thread list; process ID is ignored for threads.
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
        if snapshot == INVALID_HANDLE_VALUE {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let _snapshot = OwnedHandle(snapshot);
        let mut entry = THREADENTRY32 {
            dwSize: size_of::<THREADENTRY32>() as u32,
            ..Default::default()
        };
        // SAFETY: entry is initialized with the required size and writable for API output.
        let mut found = unsafe { Thread32First(snapshot, &mut entry) } != 0
            && entry.th32OwnerProcessID == process_id;
        let mut thread_id = if found { entry.th32ThreadID } else { 0 };
        while !found {
            // SAFETY: entry remains a valid THREADENTRY32 output structure.
            if unsafe { Thread32Next(snapshot, &mut entry) } == 0 {
                break;
            }
            if entry.th32OwnerProcessID == process_id {
                thread_id = entry.th32ThreadID;
                found = true;
            }
        }
        if !found {
            return Err(format!(
                "could not locate the initial thread for suspended process {process_id}"
            ));
        }

        // SAFETY: thread_id was obtained from the current system thread snapshot.
        let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, thread_id) };
        if thread.is_null() {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let _thread = OwnedHandle(thread);
        // SAFETY: thread is an open handle with THREAD_SUSPEND_RESUME access.
        let mut previous = unsafe { ResumeThread(thread) };
        if previous == u32::MAX {
            return Err(std::io::Error::last_os_error().to_string());
        }
        while previous > 1 {
            // Remove any additional suspend counts before returning control.
            // SAFETY: same live thread handle and access right as above.
            previous = unsafe { ResumeThread(thread) };
            if previous == u32::MAX {
                return Err(std::io::Error::last_os_error().to_string());
            }
        }
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::fs;
        use std::process::Command;
        use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
        use windows_sys::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
        use windows_sys::Win32::System::Threading::{
            OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject,
        };

        #[test]
        fn terminating_job_terminates_a_spawned_descendant() {
            let marker = std::env::temp_dir().join(format!(
                "padfire-job-child-{}-{}.pid",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
            ));
            let marker_text = marker.to_string_lossy().replace('\'', "''");
            let script = format!(
                "$ErrorActionPreference = 'Stop'; Start-Sleep -Milliseconds 400; $child = Start-Process -FilePath (Join-Path $PSHOME 'powershell.exe') -ArgumentList '-NoProfile -Command Start-Sleep -Seconds 30' -PassThru -WindowStyle Hidden; [System.IO.File]::WriteAllText('{marker_text}', [string]$child.Id); Start-Sleep -Seconds 30"
            );
            let mut command = Command::new("powershell.exe");
            use std::os::windows::process::CommandExt;
            command
                .args(["-NoProfile", "-Command", &script])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP | CREATE_SUSPENDED);

            let job = JobGuard::new().expect("create Job Object");
            let mut parent = command.spawn().expect("spawn suspended PowerShell process");
            if let Err(error) = job.assign(&parent).and_then(|()| job.resume(&parent)) {
                job.terminate();
                let _ = parent.kill();
                let _ = parent.wait();
                panic!("attach and resume suspended process: {error}");
            }

            let deadline = Instant::now() + Duration::from_secs(8);
            let child_pid = loop {
                if let Ok(value) = fs::read_to_string(&marker)
                    && let Ok(pid) = value.trim().parse::<u32>()
                {
                    break pid;
                }
                assert!(
                    Instant::now() < deadline,
                    "PowerShell did not create the child process"
                );
                std::thread::sleep(Duration::from_millis(25));
            };

            // SAFETY: The PID came from the child PowerShell process itself.
            let child_process = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, child_pid) };
            assert!(!child_process.is_null(), "open descendant process handle");
            job.terminate();
            // SAFETY: child_process is a live process handle opened with SYNCHRONIZE.
            let wait = unsafe { WaitForSingleObject(child_process, 5_000) };
            // SAFETY: test owns this handle and closes it exactly once.
            unsafe { CloseHandle(child_process) };
            let _ = parent.wait();
            let _ = fs::remove_file(&marker);
            assert_eq!(
                wait, WAIT_OBJECT_0,
                "Job Object did not terminate the descendant"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_id_liveness_detects_self_and_rejects_invalid_ids() {
        assert!(process_id_is_alive(std::process::id()));
        assert!(!process_id_is_alive(0));
        assert!(!process_id_is_alive(u32::MAX));
    }

    #[test]
    fn output_capture_has_a_small_fixed_bound() {
        assert_eq!(MAX_OUTPUT_LINES, 10);
        assert_eq!(MAX_OUTPUT_LINE_CHARS, 512);
    }

    #[test]
    fn process_timeouts_match_the_released_behavior() {
        assert_eq!(SCRIPT_TIMEOUT, Duration::from_secs(600));
        assert_eq!(TEST_TIMEOUT, Duration::from_secs(30));
        assert_eq!(CROSSHAIR_TIMEOUT, Duration::from_secs(5));
    }

    #[test]
    fn bounded_line_reader_truncates_and_drains_the_full_line() {
        let data = format!("{}\nnext\n", "x".repeat(100_000));
        let mut reader = std::io::Cursor::new(data.into_bytes());
        let mut buffer = Vec::new();
        let first = read_bounded_line(&mut reader, &mut buffer, 32)
            .unwrap()
            .unwrap();
        assert_eq!(first, "x".repeat(32));
        let second = read_bounded_line(&mut reader, &mut buffer, 32)
            .unwrap()
            .unwrap();
        assert_eq!(second, "next");
        assert!(
            read_bounded_line(&mut reader, &mut buffer, 32)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn bounded_line_reader_handles_crlf_and_unterminated_eof() {
        let mut reader = std::io::Cursor::new(b"one\r\ntwo".to_vec());
        let mut buffer = Vec::new();
        assert_eq!(
            read_bounded_line(&mut reader, &mut buffer, 8)
                .unwrap()
                .as_deref(),
            Some("one")
        );
        assert_eq!(
            read_bounded_line(&mut reader, &mut buffer, 8)
                .unwrap()
                .as_deref(),
            Some("two")
        );
        assert!(
            read_bounded_line(&mut reader, &mut buffer, 8)
                .unwrap()
                .is_none()
        );
    }

    #[cfg(unix)]
    #[test]
    fn script_process_starts_emits_output_and_stop_terminates_its_process_group() {
        use padfire_core::runtime::{GenerationTracker, OperationKey, OperationKind};
        use std::sync::mpsc::channel;

        let pad = PadKey { page: 0, note: 17 };
        let generations = Arc::new(Mutex::new(GenerationTracker::default()));
        let token = generations
            .lock()
            .unwrap()
            .begin(OperationKey::new(OperationKind::Script, pad.to_key()));
        let (events_tx, events_rx) = channel();
        let mut engine = ProcessEngine::spawn(generations, events_tx).unwrap();
        engine
            .start_script(
                token,
                pad,
                "printf 'padfire-process-test\\n'; sleep 30",
                true,
            )
            .unwrap();

        let deadline = Instant::now() + Duration::from_secs(3);
        let mut saw_started = false;
        let mut saw_output = false;
        while Instant::now() < deadline && !(saw_started && saw_output) {
            if let Ok(event) = events_rx.recv_timeout(Duration::from_millis(250)) {
                match event {
                    ProcessEvent::Started {
                        kind: ProcessKind::ScriptTest,
                        ..
                    } => saw_started = true,
                    ProcessEvent::Output { line, .. }
                        if line.trim_end() == "padfire-process-test" =>
                    {
                        saw_output = true
                    }
                    other => panic!("unexpected process event: {other:?}"),
                }
            }
        }
        assert!(saw_started, "process actor never reported start");
        assert!(saw_output, "bounded output reader did not capture stdout");

        engine.stop_script(pad).unwrap();
        let stopped = events_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        assert_eq!(
            stopped,
            ProcessEvent::Stopped {
                pad: Some(pad),
                kind: ProcessKind::ScriptTest
            }
        );
        engine.shutdown();
    }

    #[cfg(windows)]
    #[test]
    fn script_process_starts_emits_output_and_stop_terminates_its_job() {
        use padfire_core::runtime::{GenerationTracker, OperationKey, OperationKind};
        use std::sync::mpsc::channel;

        let pad = PadKey { page: 0, note: 17 };
        let generations = Arc::new(Mutex::new(GenerationTracker::default()));
        let token = generations
            .lock()
            .unwrap()
            .begin(OperationKey::new(OperationKind::Script, pad.to_key()));
        let (events_tx, events_rx) = channel();
        let mut engine = ProcessEngine::spawn(generations, events_tx).unwrap();
        engine
            .start_script(
                token,
                pad,
                "echo padfire-process-test & ping -n 31 127.0.0.1 >nul",
                true,
            )
            .unwrap();

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut saw_started = false;
        let mut saw_output = false;
        while Instant::now() < deadline && !(saw_started && saw_output) {
            if let Ok(event) = events_rx.recv_timeout(Duration::from_millis(250)) {
                match event {
                    ProcessEvent::Started {
                        kind: ProcessKind::ScriptTest,
                        ..
                    } => saw_started = true,
                    ProcessEvent::Output { line, .. }
                        if line.trim_end() == "padfire-process-test" =>
                    {
                        saw_output = true
                    }
                    other => panic!("unexpected process event: {other:?}"),
                }
            }
        }
        assert!(saw_started, "Windows process actor never reported start");
        assert!(
            saw_output,
            "Windows bounded output reader did not capture stdout"
        );

        engine.stop_script(pad).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut stopped = false;
        while Instant::now() < deadline && !stopped {
            if let Ok(event) = events_rx.recv_timeout(Duration::from_millis(250)) {
                match event {
                    ProcessEvent::Stopped {
                        pad: Some(stopped_pad),
                        kind: ProcessKind::ScriptTest,
                    } if stopped_pad == pad => stopped = true,
                    ProcessEvent::Output { .. } => {}
                    other => panic!("unexpected process event after stop: {other:?}"),
                }
            }
        }
        assert!(stopped, "Windows process actor did not report Stop");
        engine.shutdown();
    }

    #[cfg(windows)]
    #[test]
    fn stop_all_and_engine_shutdown_terminate_windows_script_jobs() {
        use padfire_core::runtime::{GenerationTracker, OperationKey, OperationKind};
        use std::sync::mpsc::channel;

        let first = PadKey { page: 0, note: 21 };
        let second = PadKey { page: 0, note: 22 };
        let final_pad = PadKey { page: 0, note: 23 };
        let generations = Arc::new(Mutex::new(GenerationTracker::default()));
        let token_for = |pad: PadKey| {
            generations
                .lock()
                .unwrap()
                .begin(OperationKey::new(OperationKind::Script, pad.to_key()))
        };
        let (events_tx, events_rx) = channel();
        let mut engine = ProcessEngine::spawn(Arc::clone(&generations), events_tx).unwrap();
        for (pad, label) in [
            (first, "padfire-stop-all-a"),
            (second, "padfire-stop-all-b"),
        ] {
            engine
                .start_script(
                    token_for(pad),
                    pad,
                    format!("echo {label} & ping -n 31 127.0.0.1 >nul"),
                    true,
                )
                .unwrap();
        }

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut first_started = false;
        let mut second_started = false;
        while Instant::now() < deadline && !(first_started && second_started) {
            if let Ok(ProcessEvent::Started {
                pad: Some(pad),
                kind: ProcessKind::ScriptTest,
            }) = events_rx.recv_timeout(Duration::from_millis(250))
            {
                first_started |= pad == first;
                second_started |= pad == second;
            }
        }
        assert!(
            first_started && second_started,
            "both Windows scripts must start before Stop All"
        );

        engine.stop_all_scripts().unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut first_stopped = false;
        let mut second_stopped = false;
        while Instant::now() < deadline && !(first_stopped && second_stopped) {
            if let Ok(event) = events_rx.recv_timeout(Duration::from_millis(250)) {
                match event {
                    ProcessEvent::Stopped {
                        pad: Some(pad),
                        kind: ProcessKind::ScriptTest,
                    } => {
                        first_stopped |= pad == first;
                        second_stopped |= pad == second;
                    }
                    ProcessEvent::Output { .. } => {}
                    other => panic!("unexpected event during Stop All: {other:?}"),
                }
            }
        }
        assert!(
            first_stopped && second_stopped,
            "Stop All did not stop both Windows jobs"
        );

        engine
            .start_script(
                token_for(final_pad),
                final_pad,
                "echo padfire-shutdown & ping -n 31 127.0.0.1 >nul",
                true,
            )
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut final_started = false;
        while Instant::now() < deadline && !final_started {
            if let Ok(event) = events_rx.recv_timeout(Duration::from_millis(250)) {
                match event {
                    ProcessEvent::Started {
                        pad: Some(pad),
                        kind: ProcessKind::ScriptTest,
                    } if pad == final_pad => final_started = true,
                    ProcessEvent::Output { .. } => {}
                    other => panic!("unexpected event before engine shutdown: {other:?}"),
                }
            }
        }
        assert!(
            final_started,
            "final Windows script did not start before shutdown"
        );
        engine.shutdown();
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut final_stopped = false;
        while Instant::now() < deadline && !final_stopped {
            if let Ok(event) = events_rx.recv_timeout(Duration::from_millis(250)) {
                match event {
                    ProcessEvent::Stopped {
                        pad: Some(pad),
                        kind: ProcessKind::ScriptTest,
                    } if pad == final_pad => final_stopped = true,
                    ProcessEvent::Output { .. } => {}
                    other => panic!("unexpected event during engine shutdown: {other:?}"),
                }
            }
        }
        assert!(
            final_stopped,
            "engine shutdown did not stop the active Windows job"
        );
    }

    #[cfg(windows)]
    #[test]
    fn cancelled_generation_cannot_launch_a_late_script() {
        use padfire_core::runtime::{GenerationTracker, OperationKey, OperationKind};
        use std::sync::mpsc::channel;
        use std::time::{SystemTime, UNIX_EPOCH};

        let pad = PadKey { page: 0, note: 18 };
        let marker = std::env::temp_dir().join(format!(
            "padfire-cancelled-script-{}-{}.txt",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let generations = Arc::new(Mutex::new(GenerationTracker::default()));
        let key = OperationKey::new(OperationKind::Script, pad.to_key());
        let token = generations.lock().unwrap().begin(key.clone());
        generations.lock().unwrap().cancel(key);
        let command = format!("echo executed > \"{}\"", marker.display());
        let (events_tx, events_rx) = channel();
        let mut engine = ProcessEngine::spawn(generations, events_tx).unwrap();
        engine.start_script(token, pad, command, true).unwrap();
        assert!(events_rx.recv_timeout(Duration::from_millis(350)).is_err());
        engine.shutdown();
        assert!(
            !marker.exists(),
            "cancelled Windows script generation executed its command"
        );
        let _ = std::fs::remove_file(marker);
    }

    #[cfg(unix)]
    #[test]
    fn cancelled_generation_cannot_launch_a_late_script() {
        use padfire_core::runtime::{GenerationTracker, OperationKey, OperationKind};
        use std::sync::mpsc::channel;

        let pad = PadKey { page: 0, note: 18 };
        let generations = Arc::new(Mutex::new(GenerationTracker::default()));
        let key = OperationKey::new(OperationKind::Script, pad.to_key());
        let token = generations.lock().unwrap().begin(key.clone());
        generations.lock().unwrap().cancel(key);
        let (events_tx, events_rx) = channel();
        let mut engine = ProcessEngine::spawn(generations, events_tx).unwrap();
        engine
            .start_script(token, pad, "touch /tmp/padfire-should-not-run", true)
            .unwrap();
        assert!(events_rx.recv_timeout(Duration::from_millis(350)).is_err());
        engine.shutdown();
        assert!(!std::path::Path::new("/tmp/padfire-should-not-run").exists());
    }
}
