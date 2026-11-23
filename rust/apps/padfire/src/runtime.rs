//! Application runtime: one shared action dispatcher for UI, MIDI and IPC.

use padfire_core::ipc::{IpcCommand, PlayingInfo, RunningStatus, StatusResponse};
use padfire_core::{
    ActionEffect, AppConfig, Control, GenerationTracker, LED_AMBER, LED_DIM, LED_GREEN, LED_OFF,
    LED_ORANGE, LED_PG_HAS, LED_RED, MidiEvent, OperationKey, OperationKind, PAGE_COUNT, PadAction,
    PadActivity, PadAssignment, PadKey, plan_pad_press,
};
use padfire_platform::audio_engine::{AudioEngine, AudioEvent, PlaybackRequest};
use padfire_platform::config_store::{ConfigStore, LoadState};
use padfire_platform::ipc_server::IpcServer;
use padfire_platform::midi::{MidiConnectionState, MidiSupervisor};
use padfire_platform::process_manager::{
    ProcessEngine, ProcessEvent, ProcessKind, process_ids_alive,
};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io::Read;
use std::path::Path;
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

// MIDI notes are 0..127 and Padfire pages are 0..7. This identity is reserved
// exclusively for the editor's audio preview and can never collide with a
// physical pad or a persisted stop target.
const PREVIEW_PAD: PadKey = PadKey {
    page: u8::MAX,
    note: u8::MAX,
};

const PID_FILE_POLL_INTERVAL: Duration = Duration::from_millis(750);
const PID_FILE_MAX_BYTES: u64 = 4096;
const PID_FILE_MAX_ENTRIES: usize = 64;

type PidFileRequest = (u64, Vec<(PadKey, String)>);
type PidFileResult = (u64, HashMap<PadKey, bool>);

/// Keeps the legacy external-PID LED behavior off the UI/MIDI thread.
/// Requests are bounded to one pending snapshot; stale results are discarded.
struct PidFileMonitor {
    requests: Option<SyncSender<PidFileRequest>>,
    results: Option<Receiver<PidFileResult>>,
    worker: Option<JoinHandle<()>>,
    generation: u64,
    last_paths: Vec<(PadKey, String)>,
    active: HashMap<PadKey, bool>,
    last_requested: Instant,
    unavailable: bool,
}

impl PidFileMonitor {
    fn new() -> Self {
        Self {
            requests: None,
            results: None,
            worker: None,
            generation: 0,
            last_paths: Vec::new(),
            active: HashMap::new(),
            last_requested: Instant::now() - PID_FILE_POLL_INTERVAL,
            unavailable: false,
        }
    }

    fn ensure_worker(&mut self) -> bool {
        if self.requests.is_some() {
            return true;
        }
        if self.unavailable {
            return false;
        }

        let (request_tx, request_rx) = mpsc::sync_channel::<PidFileRequest>(1);
        let (result_tx, result_rx) = mpsc::channel::<PidFileResult>();
        let worker = match thread::Builder::new()
            .name("padfire-pid-file-watch".to_owned())
            .spawn(move || {
                while let Ok((generation, files)) = request_rx.recv() {
                    let files = files
                        .into_iter()
                        .map(|(pad, path)| (pad, pid_file_process_ids(&path)))
                        .collect::<Vec<_>>();
                    let process_ids = files
                        .iter()
                        .flat_map(|(_, process_ids)| process_ids.iter().copied())
                        .collect::<Vec<_>>();
                    // One OS process snapshot covers all configured PID files.
                    let alive = process_ids_alive(&process_ids);
                    let active = files
                        .into_iter()
                        .map(|(pad, process_ids)| {
                            (
                                pad,
                                process_ids
                                    .iter()
                                    .any(|process_id| alive.contains(process_id)),
                            )
                        })
                        .collect();
                    if result_tx.send((generation, active)).is_err() {
                        break;
                    }
                }
            }) {
            Ok(worker) => worker,
            Err(_) => {
                self.unavailable = true;
                return false;
            }
        };

        self.requests = Some(request_tx);
        self.results = Some(result_rx);
        self.worker = Some(worker);
        true
    }

    fn poll(&mut self, config: &AppConfig) {
        if let Some(results) = &self.results {
            while let Ok((generation, active)) = results.try_recv() {
                if generation == self.generation {
                    self.active = active;
                }
            }
        }

        // Config traversal and path allocation happen at the same low cadence
        // as process checks, not on every egui frame.
        if self.last_requested.elapsed() < PID_FILE_POLL_INTERVAL {
            return;
        }
        self.last_requested = Instant::now();

        let files = config
            .pages
            .iter()
            .enumerate()
            .flat_map(|(page, assignments)| {
                assignments.iter().filter_map(move |(note, assignment)| {
                    let path = assignment.pid_file.trim();
                    (assignment.action_kind() == PadAction::Script && !path.is_empty())
                        .then(|| {
                            note.parse::<u8>()
                                .ok()
                                .map(|note| (PadKey::new(page as u8, note), path.to_owned()))
                        })
                        .flatten()
                })
            })
            .collect::<Vec<_>>();

        if files != self.last_paths {
            self.generation = self.generation.wrapping_add(1).max(1);
            self.last_paths = files.clone();
            // Drop all results for a previous path snapshot immediately. The
            // next accepted worker result repopulates the external-process LED.
            self.active.clear();
        }

        if files.is_empty() || !self.ensure_worker() {
            return;
        }

        if let Some(requests) = &self.requests {
            match requests.try_send((self.generation, files)) {
                Ok(()) => {}
                Err(TrySendError::Full(_)) => {
                    // Rate-limit retries if a slow or remote PID file is still
                    // being read. The UI/MIDI thread must never spin on it.
                }
                Err(TrySendError::Disconnected(_)) => {
                    self.active.clear();
                    self.requests = None;
                    self.results = None;
                    self.unavailable = true;
                }
            }
        }
    }

    fn is_active(&self, pad: PadKey) -> bool {
        self.active.get(&pad).copied().unwrap_or(false)
    }

    fn shutdown(&mut self) {
        self.requests.take();
        self.results.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for PidFileMonitor {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn pid_file_process_ids(path: &str) -> Vec<u32> {
    let path = Path::new(path);
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return Vec::new();
    };
    if !metadata.file_type().is_file() || metadata.len() > PID_FILE_MAX_BYTES {
        return Vec::new();
    }

    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
        use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
        // Open the reparse point itself, then reject it using handle metadata.
        let Ok(file) = options.open(path) else {
            return Vec::new();
        };
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
        let Ok(opened_metadata) = file.metadata() else {
            return Vec::new();
        };
        if !opened_metadata.is_file()
            || opened_metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
            || opened_metadata.len() > PID_FILE_MAX_BYTES
        {
            return Vec::new();
        }
        read_pid_ids(file)
    }

    #[cfg(not(windows))]
    {
        let Ok(file) = options.open(path) else {
            return Vec::new();
        };
        let Ok(opened_metadata) = file.metadata() else {
            return Vec::new();
        };
        // This second check is performed on the opened file, closing the
        // symlink/FIFO replacement window between metadata and open.
        if !opened_metadata.is_file() || opened_metadata.len() > PID_FILE_MAX_BYTES {
            return Vec::new();
        }
        read_pid_ids(file)
    }
}

fn read_pid_ids(file: fs::File) -> Vec<u32> {
    let mut contents = String::new();
    if file
        .take(PID_FILE_MAX_BYTES + 1)
        .read_to_string(&mut contents)
        .is_err()
        || contents.len() as u64 > PID_FILE_MAX_BYTES
    {
        return Vec::new();
    }
    contents
        .split_whitespace()
        .take(PID_FILE_MAX_ENTRIES)
        .filter_map(|entry| entry.parse::<u32>().ok())
        .filter(|process_id| *process_id != 0)
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppCommand {
    Show,
    StopAll,
    Reload,
    Quit,
}

#[derive(Debug, Default)]
pub struct RuntimeSignals {
    pub show_requested: bool,
    pub quit_requested: bool,
    /// Pad notes pressed by the physical Launchpad since the last UI frame.
    pub pressed_pads: Vec<u8>,
}

impl RuntimeSignals {
    fn record_pad_press(&mut self, note: u8) {
        self.pressed_pads.push(note);
    }
}

#[cfg(test)]
mod runtime_signal_tests {
    use super::*;

    #[test]
    fn physical_midi_pad_press_is_queued_for_the_next_ui_frame() {
        let mut signals = RuntimeSignals::default();
        signals.record_pad_press(36);
        signals.record_pad_press(37);
        assert_eq!(signals.pressed_pads, vec![36, 37]);
    }
}

pub struct AppRuntime {
    store: ConfigStore,
    generations: Arc<Mutex<GenerationTracker>>,
    audio: AudioEngine,
    processes: ProcessEngine,
    midi: MidiSupervisor,
    midi_events: Receiver<MidiEvent>,
    midi_status_events: Receiver<MidiConnectionState>,
    audio_events: Receiver<AudioEvent>,
    process_events: Receiver<ProcessEvent>,
    pid_file_monitor: PidFileMonitor,
    app_commands: Receiver<AppCommand>,
    ipc: Option<IpcServer>,
    status_snapshot: Arc<Mutex<RunningStatus>>,
    playing_audio: HashSet<PadKey>,
    preview_active: bool,
    running_scripts: HashSet<PadKey>,
    midi_connected: bool,
    midi_device: String,
    last_pad_leds: HashMap<u8, u8>,
    last_page_leds: [u8; PAGE_COUNT],
    current_page: usize,
    version: String,
    signals: RuntimeSignals,
}

impl AppRuntime {
    pub fn new(
        store: ConfigStore,
        config: &AppConfig,
        version: impl Into<String>,
    ) -> Result<Self, String> {
        let version = version.into();
        let generations = Arc::new(Mutex::new(GenerationTracker::default()));

        let (midi_tx, midi_events) = mpsc::sync_channel(128);
        let (midi_status_tx, midi_status_events) = mpsc::channel();
        let midi = MidiSupervisor::spawn(midi_tx, midi_status_tx)?;

        let (audio_event_tx, audio_events) = mpsc::channel();
        let audio = AudioEngine::spawn(generations.clone(), audio_event_tx)?;

        let (process_event_tx, process_events) = mpsc::channel();
        let processes = ProcessEngine::spawn(generations.clone(), process_event_tx)?;

        let (app_command_sender, app_commands) = mpsc::channel();
        let pid_file_monitor = PidFileMonitor::new();

        let initial_status = RunningStatus {
            connected: false,
            page: 0,
            playing: Vec::new(),
            playing_info: Vec::new(),
            device: "Launchpad Mini".to_owned(),
            version: version.clone(),
            monitor_enabled: config.monitor_enabled,
            monitor_sink: config.monitor_sink.clone(),
        };
        let status_snapshot = Arc::new(Mutex::new(initial_status));
        let ipc_commands = app_command_sender.clone();
        let ipc_snapshot = status_snapshot.clone();
        let ipc_version = version.clone();
        let ipc_path = store.directory().join("padfire-rust.sock");
        let ipc = match IpcServer::spawn(ipc_path, move |command| match command {
            IpcCommand::Status => ipc_snapshot
                .lock()
                .map(|status| {
                    StatusResponse::running(status.clone())
                        .to_json()
                        .unwrap_or_else(|_| "{}".to_owned())
                })
                .unwrap_or_else(|_| "{}".to_owned()),
            IpcCommand::StopAll => {
                let _ = ipc_commands.send(AppCommand::StopAll);
                "ok".to_owned()
            }
            IpcCommand::Show => {
                let _ = ipc_commands.send(AppCommand::Show);
                "ok".to_owned()
            }
            IpcCommand::Reload => {
                let _ = ipc_commands.send(AppCommand::Reload);
                "ok".to_owned()
            }
            IpcCommand::Quit => {
                let _ = ipc_commands.send(AppCommand::Quit);
                "ok".to_owned()
            }
        }) {
            Ok(server) => Some(server),
            Err(error) => {
                // Clean up workers created above if single-instance setup fails.
                drop(processes);
                drop(audio);
                drop(midi);
                return Err(format!("could not start Padfire IPC: {error}"));
            }
        };

        let _ = status_snapshot.lock().map(|mut status| {
            status.version = ipc_version;
        });

        let mut runtime = Self {
            store,
            generations,
            audio,
            processes,
            midi,
            midi_events,
            midi_status_events,
            audio_events,
            process_events,
            pid_file_monitor,
            app_commands,
            ipc,
            status_snapshot,
            playing_audio: HashSet::new(),
            preview_active: false,
            running_scripts: HashSet::new(),
            midi_connected: false,
            midi_device: "Launchpad Mini".to_owned(),
            last_pad_leds: HashMap::new(),
            last_page_leds: [u8::MAX; PAGE_COUNT],
            current_page: 0,
            version,
            signals: RuntimeSignals::default(),
        };
        runtime.refresh_leds(config, 0);
        Ok(runtime)
    }

    pub fn midi_connected(&self) -> bool {
        self.midi_connected
    }

    pub fn meter_level(&self) -> f32 {
        self.audio.meter_level()
    }

    pub fn set_page(&mut self, page: usize) {
        self.current_page = page.min(PAGE_COUNT - 1);
        self.last_pad_leds.clear();
        self.last_page_leds = [u8::MAX; PAGE_COUNT];
    }

    pub fn stop_audio(&mut self) {
        self.stop_audio_only();
    }

    pub fn take_signals(&mut self) -> RuntimeSignals {
        std::mem::take(&mut self.signals)
    }

    pub fn save_config(&self, config: &AppConfig) -> Result<(), String> {
        self.store.save(config).map_err(|error| error.to_string())
    }

    pub fn poll(&mut self, config: &mut AppConfig, page: &mut usize, status: &mut String) {
        self.poll_commands(config, status);
        self.poll_pid_files(config);
        self.poll_midi(config, page, status);
        self.poll_audio(status);
        self.sync_output_meter(config);
        self.poll_processes(status);
        self.current_page = (*page).min(PAGE_COUNT - 1);
        self.update_status_snapshot(config, *page);
        self.refresh_leds(config, *page);
    }

    fn poll_commands(&mut self, config: &mut AppConfig, status: &mut String) {
        loop {
            match self.app_commands.try_recv() {
                Ok(AppCommand::Show) => self.signals.show_requested = true,
                Ok(AppCommand::StopAll) => {
                    self.stop_audio_only();
                    *status = "All audio playback stopped".to_owned();
                }
                Ok(AppCommand::Reload) => {
                    let loaded = self.store.load();
                    *config = loaded.config;
                    *status = loaded.warning.unwrap_or_else(|| match loaded.state {
                        LoadState::Loaded | LoadState::ImportedLegacy => {
                            "Configuration reloaded".to_owned()
                        }
                        LoadState::CreatedDefault => {
                            "Created a new default configuration".to_owned()
                        }
                        LoadState::DefaultAfterError => {
                            "Configuration could not be loaded safely".to_owned()
                        }
                    });
                    self.last_pad_leds.clear();
                    self.last_page_leds = [u8::MAX; PAGE_COUNT];
                }
                Ok(AppCommand::Quit) => self.signals.quit_requested = true,
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
            }
        }
    }

    fn poll_midi(&mut self, config: &AppConfig, page: &mut usize, status: &mut String) {
        while let Ok(state) = self.midi_status_events.try_recv() {
            match state {
                MidiConnectionState::Connected { input, output } => {
                    self.midi_connected = true;
                    self.midi_device = input.clone();
                    *status = format!("Launchpad connected: {input}");
                    if !input.eq_ignore_ascii_case(&output) {
                        *status = format!("Launchpad connected: {input} / {output}");
                    }
                    self.last_pad_leds.clear();
                    self.last_page_leds = [u8::MAX; PAGE_COUNT];
                }
                MidiConnectionState::NoSupportedDevice => {
                    self.midi_connected = false;
                    *status = "Launchpad Mini MK1/MK2 not detected".to_owned();
                }
                MidiConnectionState::Disconnected => {
                    self.midi_connected = false;
                    *status = "Launchpad disconnected - reconnecting".to_owned();
                    self.last_pad_leds.clear();
                }
                MidiConnectionState::Unavailable(error) => {
                    self.midi_connected = false;
                    *status = format!("MIDI unavailable: {error}");
                }
            }
        }

        loop {
            match self.midi_events.try_recv() {
                Ok(MidiEvent::PadPressed(control)) => {
                    let note = control.note();
                    self.signals.record_pad_press(note);
                    self.trigger_pad(config, *page, note, status);
                }
                Ok(MidiEvent::PadReleased(_)) => {}
                Ok(MidiEvent::PageSelected(new_page)) => {
                    let new_page = usize::from(new_page).min(PAGE_COUNT - 1);
                    if config.stop_on_page_change && new_page != *page {
                        self.stop_audio_only();
                    }
                    *page = new_page;
                    self.current_page = new_page;
                    self.last_page_leds = [u8::MAX; PAGE_COUNT];
                }
                Ok(MidiEvent::Ignored) => {}
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
            }
        }
    }

    fn sync_output_meter(&mut self, config: &AppConfig) {
        let audio_active = !self.playing_audio.is_empty() || self.preview_active;
        let target = audio_active.then(|| {
            if config.monitor_enabled && !config.monitor_sink.trim().is_empty() {
                config.monitor_sink.trim().to_owned()
            } else {
                config.default_sink.trim().to_owned()
            }
        });
        self.audio.set_meter_capture_target(target);
    }

    fn poll_audio(&mut self, status: &mut String) {
        loop {
            match self.audio_events.try_recv() {
                Ok(AudioEvent::PlaybackStarted { pad }) => {
                    if pad == PREVIEW_PAD {
                        self.preview_active = true;
                        *status = "Preview playing".to_owned();
                    } else {
                        self.playing_audio.insert(pad);
                        *status = format!("Playing pad {}", pad.to_key());
                        self.last_pad_leds.remove(&pad.note);
                    }
                }
                Ok(AudioEvent::PlaybackFinished { pad }) => {
                    if pad == PREVIEW_PAD {
                        self.preview_active = false;
                        *status = "Preview stopped".to_owned();
                    } else {
                        self.playing_audio.remove(&pad);
                        self.last_pad_leds.remove(&pad.note);
                    }
                }
                Ok(AudioEvent::RouteFallback {
                    requested,
                    selected,
                }) => {
                    *status = format!("Output '{requested}' unavailable - using '{selected}'");
                }
                Ok(AudioEvent::DeviceUnavailable { requested }) => {
                    *status = format!("Output unavailable: {requested}");
                }
                Ok(AudioEvent::PlaybackFailed { pad, message }) => {
                    if pad == PREVIEW_PAD {
                        self.preview_active = false;
                        *status = format!("Preview failed: {message}");
                    } else {
                        self.playing_audio.remove(&pad);
                        *status = format!("Pad {} failed: {message}", pad.to_key());
                        self.last_pad_leds.remove(&pad.note);
                    }
                }
                Ok(AudioEvent::BackendUnavailable(message)) => {
                    *status = format!("Audio unavailable: {message}");
                }
                Ok(AudioEvent::Ready | AudioEvent::Stopped) => {}
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
            }
        }
    }

    fn poll_pid_files(&mut self, config: &AppConfig) {
        self.pid_file_monitor.poll(config);
    }

    fn poll_processes(&mut self, status: &mut String) {
        loop {
            match self.process_events.try_recv() {
                Ok(ProcessEvent::Started {
                    pad: Some(pad),
                    kind,
                }) => {
                    if matches!(kind, ProcessKind::Script | ProcessKind::ScriptTest) {
                        self.running_scripts.insert(pad);
                    }
                    *status = format!("{} started on {}", process_kind_label(kind), pad.to_key());
                    self.last_pad_leds.remove(&pad.note);
                }
                Ok(ProcessEvent::Output {
                    pad: Some(pad),
                    kind,
                    line,
                }) => {
                    *status = format!("{} {}: {}", process_kind_label(kind), pad.to_key(), line);
                }
                Ok(ProcessEvent::Exited { pad, kind, code }) => {
                    if let Some(pad) = pad {
                        if matches!(kind, ProcessKind::Script | ProcessKind::ScriptTest) {
                            self.running_scripts.remove(&pad);
                        }
                        self.last_pad_leds.remove(&pad.note);
                        *status = format!(
                            "{} exited ({})",
                            process_kind_label(kind),
                            code.unwrap_or(-1)
                        );
                    }
                }
                Ok(ProcessEvent::Stopped { pad, kind }) => {
                    if let Some(pad) = pad {
                        if matches!(kind, ProcessKind::Script | ProcessKind::ScriptTest) {
                            self.running_scripts.remove(&pad);
                        }
                        self.last_pad_leds.remove(&pad.note);
                        *status = format!("{} stopped", process_kind_label(kind));
                    }
                }
                Ok(ProcessEvent::TimedOut { pad, kind }) => {
                    if let Some(pad) = pad {
                        self.running_scripts.remove(&pad);
                        self.last_pad_leds.remove(&pad.note);
                        *status =
                            format!("{} timed out on {}", process_kind_label(kind), pad.to_key());
                    }
                }
                Ok(ProcessEvent::Failed { pad, kind, message }) => {
                    if let Some(pad) = pad {
                        self.running_scripts.remove(&pad);
                        self.last_pad_leds.remove(&pad.note);
                        *status = format!(
                            "{} failed on {}: {message}",
                            process_kind_label(kind),
                            pad.to_key()
                        );
                    } else {
                        *status = format!("{} failed: {message}", process_kind_label(kind));
                    }
                }
                Ok(_) => {}
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
            }
        }
    }

    fn preview_request(
        token: padfire_core::OperationToken,
        config: &AppConfig,
        assignment: &PadAssignment,
    ) -> Result<PlaybackRequest, String> {
        let file = assignment.file.trim();
        if file.is_empty() {
            return Err("Choose an audio file before previewing".to_owned());
        }
        Ok(PlaybackRequest {
            token,
            pad: PREVIEW_PAD,
            file: file.to_owned(),
            sink: if assignment.sink.trim().is_empty() {
                config.default_sink.clone()
            } else {
                assignment.sink.clone()
            },
            volume_percent: assignment.vol.clamp(0, 150) as u8,
            looping: assignment.loop_enabled,
            monitor_sink: config.monitor_enabled.then(|| config.monitor_sink.clone()),
        })
    }

    pub fn preview_audio(
        &mut self,
        config: &AppConfig,
        assignment: &PadAssignment,
    ) -> Result<(), String> {
        // Validate the new request before invalidating the previous preview.
        // Once it is valid, enqueue Stop before Play so a failed replacement
        // cannot leave an older looping audition alive.
        if assignment.file.trim().is_empty() {
            return Err("Choose an audio file before previewing".to_owned());
        }
        let token = self
            .generations
            .lock()
            .map_err(|_| "Audio generation dispatcher is unavailable".to_owned())?
            .begin(OperationKey::new(
                OperationKind::Audio,
                PREVIEW_PAD.to_key(),
            ));
        let request = Self::preview_request(token, config, assignment)?;
        self.audio
            .stop(PREVIEW_PAD)
            .map_err(|error| format!("Could not replace preview: {error}"))?;
        self.audio
            .play(request)
            .map_err(|error| format!("Could not queue preview: {error}"))
    }

    pub fn stop_preview(&mut self) -> Result<(), String> {
        self.generations
            .lock()
            .map_err(|_| "Audio generation dispatcher is unavailable".to_owned())?
            .cancel(OperationKey::new(
                OperationKind::Audio,
                PREVIEW_PAD.to_key(),
            ));
        self.audio
            .stop(PREVIEW_PAD)
            .map_err(|error| format!("Could not stop preview: {error}"))
    }

    pub fn trigger_pad(&mut self, config: &AppConfig, page: usize, note: u8, status: &mut String) {
        let Some(pad) = config
            .pages
            .get(page)
            .and_then(|page| page.get(&note.to_string()))
        else {
            *status = "Empty pad - right-click to assign".to_owned();
            return;
        };

        let key = PadKey::new(page as u8, note);
        let activity = PadActivity {
            audio_playing: self.playing_audio.contains(&key),
            script_running: self.running_scripts.contains(&key),
        };
        let effects = match self.generations.lock() {
            Ok(mut generations) => plan_pad_press(key, pad, config, activity, &mut generations),
            Err(_) => {
                *status = "Pad action dispatcher is unavailable".to_owned();
                return;
            }
        };
        self.dispatch_effects(effects, status);
        self.last_pad_leds.remove(&note);
    }

    fn dispatch_effects(&self, effects: Vec<ActionEffect>, status: &mut String) {
        for effect in effects {
            match effect {
                ActionEffect::StartPlayback {
                    token,
                    pad,
                    file,
                    sink,
                    volume_percent,
                    looping,
                    monitor_sink,
                } => {
                    let request = PlaybackRequest {
                        token,
                        pad,
                        file,
                        sink,
                        volume_percent,
                        looping,
                        monitor_sink,
                    };
                    if let Err(error) = self.audio.play(request) {
                        *status = format!("Could not queue playback: {error}");
                    } else {
                        *status = format!("Starting pad {}", pad.to_key());
                    }
                }
                ActionEffect::StopPlayback { pad, .. } => {
                    if let Err(error) = self.audio.stop(pad) {
                        *status = error;
                    } else {
                        *status = format!("Stopping pad {}", pad.to_key());
                    }
                }
                ActionEffect::StopAllPlayback { .. } => {
                    if let Err(error) = self.audio.stop_all() {
                        *status = error;
                    } else {
                        *status = "All sound playback stopped".to_owned();
                    }
                }
                ActionEffect::StartScript {
                    token,
                    pad,
                    command,
                } => {
                    if let Err(error) = self.processes.start_script(token, pad, command, false) {
                        *status = format!("Could not queue script: {error}");
                    } else {
                        *status = format!("Starting script on {}", pad.to_key());
                    }
                }
                ActionEffect::StopScript { pad, .. } => {
                    if let Err(error) = self.processes.stop_script(pad) {
                        *status = error;
                    } else {
                        *status = format!("Stopping script on {}", pad.to_key());
                    }
                }
                ActionEffect::StopAllScripts { .. } => {
                    if let Err(error) = self.processes.stop_all_scripts() {
                        *status = error;
                    }
                }
                ActionEffect::StopEverything { .. } => {
                    if let Err(error) = self.audio.stop_all() {
                        *status = error;
                    }
                    if let Err(error) = self.processes.stop_all_scripts() {
                        *status = error;
                    }
                    *status = "All sounds and scripts stopped".to_owned();
                }
                ActionEffect::ToggleCrosshair { pad } => {
                    if let Err(error) = self.processes.toggle_crosshair(pad) {
                        *status = format!("Crosshair: {error}");
                    } else {
                        *status = "Toggling Crosshair".to_owned();
                    }
                }
                ActionEffect::InvalidAssignment { reason, .. } => {
                    *status = reason;
                }
            }
        }
    }

    pub fn test_script(
        &mut self,
        page: usize,
        note: u8,
        command: impl Into<String>,
    ) -> Result<(), String> {
        let pad = PadKey::new(page as u8, note);
        let token = self
            .generations
            .lock()
            .map_err(|_| "script generation tracker is unavailable".to_owned())?
            .begin(OperationKey::new(OperationKind::Script, pad.to_key()));
        self.processes.start_script(token, pad, command, true)
    }

    pub fn stop_pad(&mut self, pad: PadKey) {
        if let Ok(mut generations) = self.generations.lock() {
            generations.cancel(OperationKey::new(OperationKind::Audio, pad.to_key()));
            generations.cancel(OperationKey::new(OperationKind::Script, pad.to_key()));
        }
        let _ = self.audio.stop(pad);
        let _ = self.processes.stop_script(pad);
    }

    pub fn stop_everything(&mut self) {
        if let Ok(mut generations) = self.generations.lock() {
            generations.cancel_all();
        }
        let _ = self.audio.stop_all();
        let _ = self.processes.stop_all_scripts();
        self.playing_audio.clear();
        self.running_scripts.clear();
        self.last_pad_leds.clear();
    }

    fn stop_audio_only(&mut self) {
        if let Ok(mut generations) = self.generations.lock() {
            generations.cancel_kind(OperationKind::Audio);
        }
        let _ = self.audio.stop_all();
        self.playing_audio.clear();
        self.last_pad_leds.clear();
    }

    pub fn refresh_leds(&mut self, config: &AppConfig, page: usize) {
        if !self.midi_connected {
            return;
        }
        let page = page.min(PAGE_COUNT - 1);
        let current = config.pages.get(page);
        for row in 0..8_u8 {
            for column in 0..8_u8 {
                let note = row * 16 + column;
                self.refresh_pad_led(current, page, note);
            }
            let side_note = row * 16 + 8;
            self.refresh_pad_led(current, page, side_note);
        }

        for page_index in 0..PAGE_COUNT {
            let velocity = if page_index == page {
                LED_AMBER
            } else if config
                .pages
                .get(page_index)
                .is_some_and(|pads| !pads.is_empty())
            {
                LED_PG_HAS
            } else {
                LED_OFF
            };
            if self.last_page_leds[page_index] != velocity
                && self.midi.set_page_led(page_index as u8, velocity).is_ok()
            {
                self.last_page_leds[page_index] = velocity;
            }
        }
    }

    fn refresh_pad_led(
        &mut self,
        pads: Option<&BTreeMap<String, PadAssignment>>,
        page: usize,
        note: u8,
    ) {
        let velocity = pads
            .and_then(|pads| pads.get(&note.to_string()))
            .map(|pad| self.pad_led_velocity(pad, PadKey::new(page as u8, note)))
            .unwrap_or(LED_OFF);
        if self.last_pad_leds.get(&note).copied() != Some(velocity)
            && self
                .midi
                .set_led(Control::from_note(note).unwrap(), velocity)
                .is_ok()
        {
            self.last_pad_leds.insert(note, velocity);
        }
    }

    fn pad_led_velocity(&self, pad: &PadAssignment, key: PadKey) -> u8 {
        let configured = match pad.color.as_str() {
            "green" => LED_GREEN,
            "orange" => LED_ORANGE,
            "red" => LED_RED,
            "amber" => LED_AMBER,
            "dim green" => LED_DIM,
            _ => 0,
        };
        match pad.action_kind() {
            PadAction::StopSfx | PadAction::StopAll | PadAction::StopKey => {
                if configured != 0 {
                    configured
                } else {
                    LED_RED
                }
            }
            PadAction::CrosshairToggle => {
                if configured != 0 {
                    configured
                } else {
                    LED_AMBER
                }
            }
            PadAction::Script => {
                if self.running_scripts.contains(&key) || self.pid_file_monitor.is_active(key) {
                    LED_GREEN
                } else if configured != 0 {
                    configured
                } else {
                    LED_ORANGE
                }
            }
            PadAction::Play => {
                if self.playing_audio.contains(&key) {
                    LED_GREEN
                } else if configured != 0 {
                    configured
                } else {
                    LED_DIM
                }
            }
            PadAction::Unsupported(_) => LED_RED,
        }
    }

    fn update_status_snapshot(&self, config: &AppConfig, page: usize) {
        let playing_info: Vec<PlayingInfo> = self
            .playing_audio
            .iter()
            .map(|key| {
                let assignment = config
                    .pages
                    .get(key.page as usize)
                    .and_then(|page| page.get(&key.note.to_string()));
                let label = assignment
                    .map(|pad| {
                        if !pad.label.trim().is_empty() {
                            pad.label.trim().to_owned()
                        } else if !pad.file.trim().is_empty() {
                            std::path::Path::new(&pad.file)
                                .file_stem()
                                .and_then(|stem| stem.to_str())
                                .unwrap_or(&pad.file)
                                .to_owned()
                        } else {
                            key.to_key()
                        }
                    })
                    .unwrap_or_else(|| key.to_key());
                let sink = assignment
                    .map(|pad| {
                        if pad.sink.trim().is_empty() {
                            config.default_sink.clone()
                        } else {
                            pad.sink.clone()
                        }
                    })
                    .unwrap_or_default();
                PlayingInfo {
                    key: key.to_key(),
                    label,
                    sink,
                    page: key.page,
                }
            })
            .collect();

        let mut playing: Vec<String> = self.playing_audio.iter().map(|key| key.to_key()).collect();
        playing.sort();
        let mut playing_info = playing_info;
        playing_info.sort_by(|a, b| a.key.cmp(&b.key));

        // The IPC socket reads this data through its shared lock; status remains
        // consistent even while the UI is drawing or a device callback runs.
        let snapshot = RunningStatus {
            connected: self.midi_connected,
            page: page.min(PAGE_COUNT - 1) as u8,
            playing,
            playing_info,
            device: self.midi_device.clone(),
            version: self.version.clone(),
            monitor_enabled: config.monitor_enabled,
            monitor_sink: config.monitor_sink.clone(),
        };
        // Status is updated via the snapshot cell below.
        self.write_snapshot(snapshot);
    }

    fn write_snapshot(&self, snapshot: RunningStatus) {
        if let Ok(mut shared) = self.status_snapshot.lock() {
            *shared = snapshot;
        }
    }

    pub fn shutdown(&mut self) {
        self.stop_everything();
        self.pid_file_monitor.shutdown();
        let _ = self.midi.clear_all_leds();
        self.midi.shutdown();
        self.audio.shutdown();
        self.processes.shutdown();
        if let Some(mut ipc) = self.ipc.take() {
            ipc.shutdown();
        }
    }
}

fn process_kind_label(kind: ProcessKind) -> &'static str {
    match kind {
        ProcessKind::Script => "Script",
        ProcessKind::ScriptTest => "Script test",
        ProcessKind::Crosshair => "Crosshair",
    }
}

impl Drop for AppRuntime {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod preview_tests {
    use super::*;

    #[test]
    fn pid_file_monitor_does_not_spawn_a_worker_without_configured_files() {
        let mut monitor = PidFileMonitor::new();
        monitor.poll(&AppConfig::default());
        assert!(monitor.worker.is_none());
        assert!(monitor.requests.is_none());
    }

    #[test]
    fn pid_file_config_scans_are_throttled_to_the_poll_interval() {
        let mut monitor = PidFileMonitor::new();
        let mut config = AppConfig::default();
        config.pages[0].insert(
            "0".to_owned(),
            PadAssignment {
                action: "script".to_owned(),
                pid_file: "/tmp/padfire-test.pid".to_owned(),
                ..PadAssignment::default()
            },
        );
        monitor.poll(&config);
        let generation = monitor.generation;
        monitor.poll(&config);
        assert_eq!(monitor.generation, generation);
        monitor.shutdown();
    }

    #[test]
    fn pid_file_monitor_spawns_only_after_a_script_uses_a_pid_file() {
        let mut monitor = PidFileMonitor::new();
        let mut config = AppConfig::default();
        config.pages[0].insert(
            "0".to_owned(),
            PadAssignment {
                action: "script".to_owned(),
                pid_file: "/tmp/padfire-test.pid".to_owned(),
                ..PadAssignment::default()
            },
        );
        monitor.poll(&config);
        assert!(monitor.worker.is_some());
        assert!(monitor.requests.is_some());
        monitor.shutdown();
    }

    #[test]
    fn pid_file_reports_current_process_and_ignores_invalid_entries() {
        let path = std::env::temp_dir().join(format!(
            "padfire-pid-file-{}-{}.txt",
            std::process::id(),
            Instant::now().elapsed().as_nanos()
        ));
        fs::write(&path, format!("not-a-pid 0 {}\n", std::process::id())).unwrap();
        let process_ids = pid_file_process_ids(&path.to_string_lossy());
        assert_eq!(process_ids, vec![std::process::id()]);
        assert!(process_ids_alive(&process_ids).contains(&std::process::id()));
        fs::write(&path, "0 4294967295 not-a-pid\n").unwrap();
        let process_ids = pid_file_process_ids(&path.to_string_lossy());
        assert_eq!(process_ids, vec![u32::MAX]);
        assert!(!process_ids_alive(&process_ids).contains(&u32::MAX));
        let _ = fs::remove_file(path);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn pid_file_rejects_symlinks_and_directories() {
        use std::os::unix::fs::symlink;

        let base = std::env::temp_dir().join(format!("padfire-pid-safety-{}", std::process::id()));
        let target = base.with_extension("target");
        let link = base.with_extension("link");
        let directory = base.with_extension("directory");
        fs::write(&target, std::process::id().to_string()).unwrap();
        symlink(&target, &link).unwrap();
        fs::create_dir(&directory).unwrap();

        assert!(pid_file_process_ids(&link.to_string_lossy()).is_empty());
        assert!(pid_file_process_ids(&directory.to_string_lossy()).is_empty());

        let _ = fs::remove_file(link);
        let _ = fs::remove_file(target);
        let _ = fs::remove_dir(directory);
    }

    #[test]
    fn pid_file_rejects_oversized_and_non_regular_paths() {
        let path =
            std::env::temp_dir().join(format!("padfire-pid-file-large-{}.txt", std::process::id()));
        fs::write(&path, vec![b'1'; (PID_FILE_MAX_BYTES + 1) as usize]).unwrap();
        assert!(pid_file_process_ids(&path.to_string_lossy()).is_empty());
        let _ = fs::remove_file(path);
    }

    #[test]
    fn preview_uses_reserved_identity_and_editor_playback_settings() {
        let config = AppConfig {
            default_sink: "global-output".to_owned(),
            monitor_enabled: true,
            monitor_sink: "monitor-output".to_owned(),
            ..AppConfig::default()
        };
        let assignment = PadAssignment {
            file: " /tmp/test-tone.wav ".to_owned(),
            sink: String::new(),
            vol: 150,
            loop_enabled: true,
            ..PadAssignment::default()
        };
        let key = OperationKey::new(OperationKind::Audio, PREVIEW_PAD.to_key());
        let mut generations = GenerationTracker::default();
        let token = generations.begin(key);
        let request = AppRuntime::preview_request(token.clone(), &config, &assignment).unwrap();
        assert_eq!(request.pad, PREVIEW_PAD);
        assert_eq!(request.pad.page, u8::MAX);
        assert_eq!(request.pad.note, u8::MAX);
        assert_eq!(request.file, "/tmp/test-tone.wav");
        assert_eq!(request.sink, "global-output");
        assert_eq!(request.volume_percent, 150);
        assert!(request.looping);
        assert_eq!(request.monitor_sink.as_deref(), Some("monitor-output"));
        assert!(generations.is_current(&token));
    }

    #[test]
    fn preview_uses_pad_sink_clamps_volume_and_can_disable_monitor() {
        let config = AppConfig {
            default_sink: "global-output".to_owned(),
            monitor_enabled: false,
            monitor_sink: "unused-monitor".to_owned(),
            ..AppConfig::default()
        };
        let assignment = PadAssignment {
            file: "/tmp/test-tone.wav".to_owned(),
            sink: "pad-output".to_owned(),
            vol: 999,
            loop_enabled: false,
            ..PadAssignment::default()
        };
        let token = GenerationTracker::default().begin(OperationKey::new(
            OperationKind::Audio,
            PREVIEW_PAD.to_key(),
        ));
        let request = AppRuntime::preview_request(token, &config, &assignment).unwrap();
        assert_eq!(request.sink, "pad-output");
        assert_eq!(request.volume_percent, 150);
        assert!(!request.looping);
        assert!(request.monitor_sink.is_none());
    }

    #[test]
    fn starting_a_new_preview_and_stop_both_invalidate_old_generations() {
        let key = OperationKey::new(OperationKind::Audio, PREVIEW_PAD.to_key());
        let mut generations = GenerationTracker::default();
        let first = generations.begin(key.clone());
        let second = generations.begin(key.clone());
        assert!(!generations.is_current(&first));
        assert!(generations.is_current(&second));

        generations.cancel(key);
        assert!(!generations.is_current(&second));
    }

    #[test]
    fn empty_preview_file_is_rejected_before_audio_is_queued() {
        let token = GenerationTracker::default().begin(OperationKey::new(
            OperationKind::Audio,
            PREVIEW_PAD.to_key(),
        ));
        let result =
            AppRuntime::preview_request(token, &AppConfig::default(), &PadAssignment::default());
        assert_eq!(
            result.unwrap_err(),
            "Choose an audio file before previewing"
        );
    }
}
