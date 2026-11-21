//! Process supervision for trusted script pads and Crosshair.
//!
//! User-authored script commands intentionally run through the platform shell.
//! Each gets a process group (Unix) or Job Object (Windows), a generation
//! token, a hard deadline and bounded output capture. Crosshair is different:
//! it is always invoked directly with the --toggle argument, never through a shell.

use padfire_core::runtime::{GenerationTracker, OperationToken};
use padfire_core::PadKey;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessKind {
    Script,
    ScriptTest,
    Crosshair,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessEvent {
    Started { pad: Option<PadKey>, kind: ProcessKind },
    Output { pad: Option<PadKey>, kind: ProcessKind, line: String },
    Exited { pad: Option<PadKey>, kind: ProcessKind, code: Option<i32> },
    TimedOut { pad: Option<PadKey>, kind: ProcessKind },
    Stopped { pad: Option<PadKey>, kind: ProcessKind },
    Failed { pad: Option<PadKey>, kind: ProcessKind, message: String },
}

enum ProcessCommand {
    StartScript { token: OperationToken, pad: PadKey, command: String, test_run: bool },
    StopScript { pad: PadKey },
    StopAllScripts,
    ToggleCrosshair { pad: PadKey },
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
        Ok(Self { sender, worker: Some(worker) })
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
            token, pad, command: command.into(), test_run,
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
        Self { generations, events, active: HashMap::new(), next_id: 0 }
    }

    fn run(mut self, receiver: Receiver<ProcessCommand>) {
        loop {
            let message = if self.active.is_empty() {
                receiver.recv().map_err(|_| RecvTimeoutError::Disconnected)
            } else {
                receiver.recv_timeout(POLL_INTERVAL)
            };
            match message {
                Ok(ProcessCommand::StartScript { token, pad, command, test_run }) => {
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
        self.generations.lock()
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
            command.args(["/D", "/S", "/C", command_line])
                .creation_flags(0x0000_0200);
            command
        };

        command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());

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
                self.fail(Some(pad), ProcessKind::Script, format!("could not start script: {error}"));
                return;
            }
        };

        #[cfg(windows)]
        if let Err(error) = job.assign(&child) {
            let _ = child.kill();
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

        let kind = if test_run { ProcessKind::ScriptTest } else { ProcessKind::Script };
        let deadline = Instant::now() + if test_run { TEST_TIMEOUT } else { SCRIPT_TIMEOUT };
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        self.active.insert(key, ManagedProcess {
            pad: Some(pad), kind, child, deadline,
            #[cfg(windows)]
            job,
        });

        if let Some(stdout) = stdout {
            spawn_output_reader(stdout, Some(pad), kind, self.events.clone());
        }
        if let Some(stderr) = stderr {
            spawn_output_reader(stderr, Some(pad), kind, self.events.clone());
        }
        let _ = self.events.send(ProcessEvent::Started { pad: Some(pad), kind });
    }

    fn toggle_crosshair(&mut self, pad: PadKey) {
        let Some(executable) = resolve_crosshair_executable() else {
            self.fail(Some(pad), ProcessKind::Crosshair,
                "Crosshair CLI was not found on PATH or in its user-local install path");
            return;
        };

        let key = format!("crosshair:{}:{}", pad.to_key(), self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        let mut command = Command::new(&executable);
        command.arg("--toggle").stdin(Stdio::null())
            .stdout(Stdio::piped()).stderr(Stdio::piped());

        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0000_0200);
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
                self.fail(Some(pad), ProcessKind::Crosshair,
                    format!("could not start {}: {error}", executable.display()));
                return;
            }
        };

        #[cfg(windows)]
        if let Err(error) = job.assign(&child) {
            let _ = child.kill();
            let _ = child.wait();
            self.fail(Some(pad), ProcessKind::Crosshair, error);
            return;
        }

        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        self.active.insert(key, ManagedProcess {
            pad: Some(pad), kind: ProcessKind::Crosshair, child,
            deadline: Instant::now() + CROSSHAIR_TIMEOUT,
            #[cfg(windows)]
            job,
        });
        if let Some(stdout) = stdout {
            spawn_output_reader(stdout, Some(pad), ProcessKind::Crosshair, self.events.clone());
        }
        if let Some(stderr) = stderr {
            spawn_output_reader(stderr, Some(pad), ProcessKind::Crosshair, self.events.clone());
        }
        let _ = self.events.send(ProcessEvent::Started {
            pad: Some(pad), kind: ProcessKind::Crosshair,
        });
    }

    fn stop_script(&mut self, pad: PadKey) {
        self.terminate_by_key(&format!("script:{}", pad.to_key()));
    }

    fn stop_all_scripts(&mut self) {
        let keys: Vec<String> = self.active.iter()
            .filter(|(_, process)| matches!(process.kind, ProcessKind::Script | ProcessKind::ScriptTest))
            .map(|(key, _)| key.clone()).collect();
        for key in keys { self.terminate_by_key(&key); }
    }

    fn stop_all(&mut self) {
        let keys: Vec<String> = self.active.keys().cloned().collect();
        for key in keys { self.terminate_by_key(&key); }
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
            let Some(process) = self.active.get_mut(&key) else { continue };

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
                            pad, kind, code: status.code(),
                        });
                    }
                }
                Ok(None) => {}
                Err(error) => {
                    if let Some(mut process) = self.active.remove(&key) {
                        let pad = process.pad;
                        let kind = process.kind;
                        terminate_managed(&mut process);
                        self.fail(pad, kind, format!("could not query child process state: {error}"));
                    }
                }
            }
        }
    }

    fn fail(&self, pad: Option<PadKey>, kind: ProcessKind, message: impl Into<String>) {
        let _ = self.events.send(ProcessEvent::Failed { pad, kind, message: message.into() });
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
) where R: std::io::Read + Send + 'static {
    let _ = thread::Builder::new().name("padfire-script-output".to_owned()).spawn(move || {
        let mut reader = BufReader::new(reader);
        let mut buffer = Vec::with_capacity(MAX_OUTPUT_LINE_CHARS);
        for index in 0.. {
            let line = match read_bounded_line(&mut reader, &mut buffer, MAX_OUTPUT_LINE_CHARS) {
                Ok(Some(line)) => line,
                Ok(None) | Err(_) => break,
            };
            if index >= MAX_OUTPUT_LINES {
                continue; // keep draining the pipe without retaining more output
            }
            let _ = events.send(ProcessEvent::Output { pad, kind, line });
        }
    });
}

pub fn resolve_crosshair_executable() -> Option<std::path::PathBuf> {
    let name = if cfg!(windows) { "crosshair.exe" } else { "crosshair" };
    if let Some(path) = find_on_path(name) { return Some(path); }

    #[cfg(unix)]
    if let Some(home) = std::env::var_os("HOME") {
        let candidate = std::path::PathBuf::from(home).join(".local/bin").join(name);
        if is_executable_file(&candidate) { return Some(candidate); }
    }

    #[cfg(windows)]
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        let candidate = std::path::PathBuf::from(local)
            .join("Programs/Crosshair").join(name);
        if candidate.is_file() { return Some(candidate); }
    }
    None
}

fn find_on_path(name: &str) -> Option<std::path::PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|directory| directory.join(name))
        .find(|candidate| is_executable_file(candidate))
}

fn is_executable_file(path: &std::path::Path) -> bool {
    if !path.is_file() { return false; }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path).ok()
            .is_some_and(|metadata| metadata.permissions().mode() & 0o111 != 0)
    }
    #[cfg(windows)]
    { true }
}

fn terminate_managed(process: &mut ManagedProcess) {
    let group = process_group_id(process.child.id());
    terminate_child(&mut process.child, group);
    #[cfg(windows)]
    process.job.terminate();
}

#[cfg(unix)]
fn process_group_id(child_id: u32) -> Option<i32> {
    i32::try_from(child_id).ok()
}

#[cfg(not(unix))]
fn process_group_id(_child_id: u32) -> Option<i32> { None }

#[cfg(unix)]
fn terminate_child(child: &mut Child, group: Option<i32>) {
    use nix::sys::signal::{killpg, Signal};
    use nix::unistd::Pid;

    if let Some(group) = group {
        let _ = killpg(Pid::from_raw(group), Signal::SIGTERM);
    } else {
        let _ = child.kill();
    }
    let until = Instant::now() + Duration::from_millis(300);
    while Instant::now() < until {
        if child.try_wait().ok().flatten().is_some() { break; }
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
        AssignProcessToJobObject, CreateJobObjectW, SetInformationJobObject, TerminateJobObject,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JobObjectExtendedLimitInformation,
    };

    pub struct JobGuard { handle: HANDLE }

    impl JobGuard {
        pub fn new() -> Result<Self, String> {
            // SAFETY: Null attributes/name request a new unnamed job. Handle is checked below.
            let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
            if handle.is_null() { return Err(std::io::Error::last_os_error().to_string()); }

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
            let result = unsafe {
                AssignProcessToJobObject(self.handle, child.as_raw_handle() as HANDLE)
            };
            if result == 0 {
                Err(std::io::Error::last_os_error().to_string())
            } else {
                Ok(())
            }
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let first = read_bounded_line(&mut reader, &mut buffer, 32).unwrap().unwrap();
        assert_eq!(first, "x".repeat(32));
        let second = read_bounded_line(&mut reader, &mut buffer, 32).unwrap().unwrap();
        assert_eq!(second, "next");
        assert!(read_bounded_line(&mut reader, &mut buffer, 32).unwrap().is_none());
    }

    #[test]
    fn bounded_line_reader_handles_crlf_and_unterminated_eof() {
        let mut reader = std::io::Cursor::new(b"one\r\ntwo".to_vec());
        let mut buffer = Vec::new();
        assert_eq!(read_bounded_line(&mut reader, &mut buffer, 8).unwrap().as_deref(), Some("one"));
        assert_eq!(read_bounded_line(&mut reader, &mut buffer, 8).unwrap().as_deref(), Some("two"));
        assert!(read_bounded_line(&mut reader, &mut buffer, 8).unwrap().is_none());
    }

    #[cfg(unix)]
    #[test]
    fn script_process_starts_emits_output_and_stop_terminates_its_process_group() {
        use padfire_core::runtime::{GenerationTracker, OperationKey, OperationKind};
        use std::sync::mpsc::channel;

        let pad = PadKey { page: 0, note: 17 };
        let generations = Arc::new(Mutex::new(GenerationTracker::default()));
        let token = generations.lock().unwrap().begin(OperationKey::new(
            OperationKind::Script,
            pad.to_key(),
        ));
        let (events_tx, events_rx) = channel();
        let mut engine = ProcessEngine::spawn(generations, events_tx).unwrap();
        engine.start_script(token, pad, "printf 'padfire-process-test\\n'; sleep 30", true).unwrap();

        let deadline = Instant::now() + Duration::from_secs(3);
        let mut saw_started = false;
        let mut saw_output = false;
        while Instant::now() < deadline && !(saw_started && saw_output) {
            if let Ok(event) = events_rx.recv_timeout(Duration::from_millis(250)) {
                match event {
                    ProcessEvent::Started { kind: ProcessKind::ScriptTest, .. } => saw_started = true,
                    ProcessEvent::Output { line, .. } if line == "padfire-process-test" => saw_output = true,
                    other => panic!("unexpected process event: {other:?}"),
                }
            }
        }
        assert!(saw_started, "process actor never reported start");
        assert!(saw_output, "bounded output reader did not capture stdout");

        engine.stop_script(pad).unwrap();
        let stopped = events_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        assert_eq!(stopped, ProcessEvent::Stopped { pad: Some(pad), kind: ProcessKind::ScriptTest });
        engine.shutdown();
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
        engine.start_script(token, pad, "touch /tmp/padfire-should-not-run", true).unwrap();
        assert!(events_rx.recv_timeout(Duration::from_millis(350)).is_err());
        engine.shutdown();
        assert!(!std::path::Path::new("/tmp/padfire-should-not-run").exists());
    }
}
