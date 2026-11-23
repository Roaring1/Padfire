mod runtime;

use eframe::egui::{self, Align, Color32, Context, Grid, Layout, RichText, Stroke, Vec2};
use padfire_core::audio::OutputDevice;
use padfire_core::ipc::{CliAction, IpcCommand, StatusResponse};
use padfire_core::{AppConfig, PAGE_COUNT, PadAction, PadAssignment, PadKey};
use padfire_platform::audio_engine::enumerate_output_devices;
use padfire_platform::config_store::{ConfigStore, LoadState};
use padfire_platform::ipc_server;
use padfire_platform::loudness::LoudnessResult;
use runtime::AppRuntime;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn command_for_script_path(path: &Path) -> Result<String, String> {
    let path = path
        .to_str()
        .ok_or_else(|| "Selected script path is not valid Unicode".to_owned())?;
    if path.contains(['\n', '\r', '\0']) {
        return Err("Selected script path contains a control character".to_owned());
    }

    #[cfg(target_os = "linux")]
    {
        let quote = |value: &str| {
            let escaped_quote = ['\'', '\\', '\'', '\''].iter().collect::<String>();
            format!("'{}'", value.replace('\'', &escaped_quote))
        };
        let extension = Path::new(path)
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let command = match extension.as_str() {
            "sh" | "bash" => format!("sh {}", quote(path)),
            "py" => format!("python3 {}", quote(path)),
            _ => quote(path),
        };
        Ok(command)
    }

    #[cfg(windows)]
    {
        // cmd.exe expands percent-delimited environment variables even inside
        // double quotes. Reject ambiguous shell metacharacters rather than
        // creating a browse action that can run a different path than selected.
        if path.contains(['%', '!', '^', '"']) {
            return Err(
                "This Windows path contains cmd.exe expansion characters; enter a trusted command manually".to_owned(),
            );
        }
        let quoted = format!("\"{path}\"");
        let extension = Path::new(path)
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let command = match extension.as_str() {
            "bat" | "cmd" => format!("call {quoted}"),
            "ps1" => format!("powershell.exe -NoProfile -ExecutionPolicy Bypass -File {quoted}"),
            _ => quoted,
        };
        Ok(command)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EscapeBehavior {
    HideToTray,
    Minimize,
}

fn should_hide_on_close(close_requested: bool, quit_requested: bool, tray_available: bool) -> bool {
    close_requested && !quit_requested && tray_available
}

fn escape_behavior(tray_available: bool) -> EscapeBehavior {
    if tray_available {
        EscapeBehavior::HideToTray
    } else {
        EscapeBehavior::Minimize
    }
}

fn page_shortcut_index(keyboard_is_editing: bool, pressed: [bool; 8]) -> Option<usize> {
    if keyboard_is_editing {
        None
    } else {
        pressed.iter().position(|is_pressed| *is_pressed)
    }
}

fn stop_all_shortcut_enabled(keyboard_is_editing: bool, space_pressed: bool) -> bool {
    !keyboard_is_editing && space_pressed
}

fn main() {
    if let Err(error) = run() {
        eprintln!("Padfire: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.as_slice() == ["--help"] || args.as_slice() == ["-h"] {
        print_help();
        return Ok(());
    }
    if args.as_slice() == ["--version"] {
        println!("Padfire {VERSION}");
        return Ok(());
    }

    let action = CliAction::parse_args(args.iter().map(String::as_str)).map_err(str::to_owned)?;

    if action == Some(CliAction::Install) {
        let path = padfire_platform::installer::install_current()?;
        println!("Padfire Rust installed to {}", path.display());
        return Ok(());
    }
    if action == Some(CliAction::Uninstall) {
        println!("{}", padfire_platform::installer::uninstall_current()?);
        return Ok(());
    }

    let store = ConfigStore::for_development().map_err(|error| error.to_string())?;
    let socket_path = store.directory().join("padfire-rust.sock");

    if let Some(action) = action {
        if action == CliAction::Version {
            println!("Padfire {VERSION}");
            return Ok(());
        }
        let Some(command) = action.ipc_command() else {
            return Err("invalid command".to_owned());
        };
        match ipc_server::send(&socket_path, command)? {
            Some(response) => println!("{response}"),
            None if command == IpcCommand::Status => {
                println!(
                    "{}",
                    StatusResponse::stopped(VERSION)
                        .to_json()
                        .map_err(|error| error.to_string())?
                );
            }
            None => {
                println!("not running");
                std::process::exit(1);
            }
        }
        return Ok(());
    }

    let loaded = store.load();
    let warning = loaded.warning.clone();
    let startup_status = warning.unwrap_or_else(|| match loaded.state {
        LoadState::Loaded => "Ready".to_owned(),
        LoadState::ImportedLegacy => "Imported the previous Padfire configuration".to_owned(),
        LoadState::CreatedDefault => {
            "New configuration - right-click a pad to assign it".to_owned()
        }
        LoadState::DefaultAfterError => "Configuration could not be loaded safely".to_owned(),
    });

    let app = match PadfireApp::new(loaded.config, store.clone(), startup_status) {
        Ok(app) => app,
        Err(error) => {
            // If another instance won the socket race, focus it rather than
            // replacing its config or creating duplicate device connections.
            if let Ok(Some(response)) = ipc_server::send(&socket_path, IpcCommand::Show)
                && response == "ok"
            {
                return Ok(());
            }
            return Err(error);
        }
    };

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(format!("Padfire {VERSION}"))
            .with_inner_size([540.0, 586.0])
            .with_min_inner_size([490.0, 510.0])
            .with_resizable(true),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };

    eframe::run_native(
        "Padfire",
        options,
        Box::new(move |_creation_context| Ok(Box::new(app))),
    )
    .map_err(|error| error.to_string())
}

fn print_help() {
    println!(
        "Padfire {VERSION}\n\
         \n\
         Usage: padfire [OPTION]\n\
         \n\
         --show       Bring the running window to the front\n\
         --stop       Stop all audio playback\n\
         --reload     Reload padfire.json without restarting\n\
         --quit       Quit the running instance\n\
         --status     Print machine-readable JSON status\n\
         --version    Print the application version\n\
         --install    Install Padfire Rust alongside the existing app\n         --uninstall  Remove Padfire Rust without deleting user configuration\n"
    );
}

#[derive(Debug, Clone, Copy)]
struct PadDragPayload {
    page: usize,
    note: u8,
}

struct PadfireApp {
    runtime: AppRuntime,
    config: AppConfig,
    page: usize,
    selected_note: Option<u8>,
    editor_open: bool,
    settings_open: bool,
    editor: PadAssignment,
    settings_backup: Option<AppConfig>,
    output_devices: Vec<OutputDevice>,
    output_error: Option<String>,
    status: String,
    loudness_sender: mpsc::Sender<Result<Option<LoudnessResult>, String>>,
    loudness_receiver: mpsc::Receiver<Result<Option<LoudnessResult>, String>>,
    loudness_cancel: Option<Arc<AtomicBool>>,
    loudness_worker: Option<JoinHandle<()>>,
    loudness_busy: bool,
    loudness_result: Option<LoudnessResult>,
    loudness_message: String,
    meter_level_display: f32,
    meter_last_update: Instant,
    tray_initialized: bool,
    quit_requested: bool,
    #[cfg(any(target_os = "linux", windows))]
    tray_icon: Option<tray_icon::TrayIcon>,
}

impl PadfireApp {
    fn new(config: AppConfig, store: ConfigStore, status: String) -> Result<Self, String> {
        let runtime = AppRuntime::new(store, &config, VERSION)?;
        let (loudness_sender, loudness_receiver) = mpsc::channel();
        Ok(Self {
            runtime,
            config,
            page: 0,
            selected_note: None,
            editor_open: false,
            settings_open: false,
            editor: PadAssignment::default(),
            settings_backup: None,
            output_devices: Vec::new(),
            output_error: None,
            status,
            loudness_sender,
            loudness_receiver,
            loudness_cancel: None,
            loudness_worker: None,
            loudness_busy: false,
            loudness_result: None,
            loudness_message: String::new(),
            meter_level_display: 0.0,
            meter_last_update: Instant::now(),
            tray_initialized: false,
            quit_requested: false,
            #[cfg(any(target_os = "linux", windows))]
            tray_icon: None,
        })
    }

    fn start_loudness_analysis(&mut self) {
        if self.loudness_busy {
            return;
        }
        let filepath = self.editor.file.trim().to_owned();
        if filepath.is_empty() {
            self.loudness_message = "Choose an audio file first".to_owned();
            return;
        }

        self.loudness_result = None;
        self.loudness_message = "Analyzing first 120 seconds...".to_owned();
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = cancelled.clone();
        let sender = self.loudness_sender.clone();
        let worker = std::thread::Builder::new()
            .name("padfire-loudness".to_owned())
            .spawn(move || {
                let result = padfire_platform::loudness::analyze_lufs(&filepath, &worker_cancelled)
                    .map_err(|error| error.to_string());
                let _ = sender.send(result);
            });

        match worker {
            Ok(worker) => {
                self.loudness_cancel = Some(cancelled);
                self.loudness_worker = Some(worker);
                self.loudness_busy = true;
            }
            Err(error) => {
                self.loudness_message = format!("Could not start analysis: {error}");
            }
        }
    }

    fn poll_loudness_analysis(&mut self) {
        let Ok(result) = self.loudness_receiver.try_recv() else {
            return;
        };
        let was_cancelled = self
            .loudness_cancel
            .as_ref()
            .is_some_and(|token| token.load(Ordering::Acquire));
        self.loudness_busy = false;
        self.loudness_cancel = None;
        if let Some(worker) = self.loudness_worker.take() {
            let _ = worker.join();
        }

        match result {
            Ok(Some(result)) => {
                self.loudness_message = format!(
                    "{:.1} LUFS, {:.1} dBFS true peak",
                    result.integrated_lufs, result.true_peak_dbfs
                );
                self.loudness_result = Some(result);
            }
            Ok(None) if was_cancelled => {
                self.loudness_message = "Analysis cancelled".to_owned();
            }
            Ok(None) => {
                self.loudness_message = "File is silent; no gain suggestion".to_owned();
            }
            Err(error) => {
                self.loudness_message = format!("Loudness analysis failed: {error}");
            }
        }
    }

    fn cancel_loudness_analysis(&mut self) {
        if let Some(cancelled) = self.loudness_cancel.take() {
            cancelled.store(true, Ordering::Release);
        }
        if let Some(worker) = self.loudness_worker.take() {
            let _ = worker.join();
        }
        while self.loudness_receiver.try_recv().is_ok() {}
        self.loudness_busy = false;
        self.loudness_result = None;
        self.loudness_message.clear();
    }

    #[cfg(any(target_os = "linux", windows))]
    fn ensure_tray(&mut self) {
        if self.tray_initialized {
            return;
        }
        self.tray_initialized = true;
        match build_tray_icon() {
            Ok(icon) => self.tray_icon = Some(icon),
            Err(error) => {
                eprintln!("Padfire tray unavailable: {error}");
            }
        }
    }

    #[cfg(any(target_os = "linux", windows))]
    fn handle_tray_events(&mut self, context: &Context) {
        use tray_icon::menu::MenuEvent;

        for event in MenuEvent::receiver().try_iter() {
            match event.id.0.as_str() {
                "padfire-show" => {
                    context.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    context.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    context.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                "padfire-stop" => {
                    self.runtime.stop_audio();
                    self.status = "Audio playback stopped".to_owned();
                }
                "padfire-quit" => self.request_quit(context),
                _ => {}
            }
        }
    }

    fn tray_available(&self) -> bool {
        #[cfg(any(target_os = "linux", windows))]
        {
            self.tray_icon.is_some()
        }
        #[cfg(not(any(target_os = "linux", windows)))]
        {
            false
        }
    }

    fn request_quit(&mut self, context: &Context) {
        self.quit_requested = true;
        context.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    fn handle_window_close_request(&mut self, context: &Context) {
        let close_requested = context.input(|input| input.viewport().close_requested());
        if should_hide_on_close(close_requested, self.quit_requested, self.tray_available()) {
            // Closing the titlebar should leave the app and its device workers
            // alive in the tray. Explicit Quit paths set quit_requested first.
            context.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            context.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            self.status = "Padfire is running in the tray".to_owned();
        }
    }

    fn begin_edit(&mut self, note: u8) {
        self.cancel_loudness_analysis();
        let _ = self.runtime.stop_preview();
        self.selected_note = Some(note);
        self.editor = self.config.pages[self.page]
            .get(&note.to_string())
            .cloned()
            .unwrap_or_default();
        self.editor_open = true;
    }

    fn save_edit(&mut self) {
        self.cancel_loudness_analysis();
        let _ = self.runtime.stop_preview();
        if let Some(note) = self.selected_note {
            self.config.pages[self.page].insert(note.to_string(), self.editor.clone());
            match self.runtime.save_config(&self.config) {
                Ok(()) => self.status = format!("Saved page {} pad {note}", self.page + 1),
                Err(error) => self.status = format!("Could not save configuration: {error}"),
            }
        }
        self.editor_open = false;
    }

    fn clear_pad(&mut self, note: u8) {
        self.cancel_loudness_analysis();
        let _ = self.runtime.stop_preview();
        self.runtime.stop_pad(PadKey::new(self.page as u8, note));
        if let Some(page) = self.config.pages.get_mut(self.page) {
            page.remove(&note.to_string());
        }
        match self.runtime.save_config(&self.config) {
            Ok(()) => self.status = format!("Cleared page {} pad {note}", self.page + 1),
            Err(error) => self.status = format!("Could not save configuration: {error}"),
        }
        self.editor_open = false;
    }

    fn change_page(&mut self, page: usize) {
        let page = page.min(PAGE_COUNT - 1);
        if page == self.page {
            return;
        }
        self.page = page;
        if self.config.stop_on_page_change {
            self.runtime.stop_audio();
        }
        self.runtime.set_page(self.page);
        self.status = format!("Page {}", self.page + 1);
    }

    fn pad_name(&self, note: u8) -> String {
        let Some(pad) = self.config.pages[self.page].get(&note.to_string()) else {
            return String::new();
        };
        if !pad.label.trim().is_empty() {
            return pad.label.trim().to_owned();
        }
        if !pad.file.trim().is_empty() {
            return Path::new(&pad.file)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(&pad.file)
                .to_owned();
        }
        match pad.action_kind() {
            PadAction::Script => "Script".to_owned(),
            PadAction::StopSfx => "Stop SFX".to_owned(),
            PadAction::StopAll => "Stop All".to_owned(),
            PadAction::StopKey => "Stop".to_owned(),
            PadAction::CrosshairToggle => "Crosshair".to_owned(),
            PadAction::Unsupported(_) => "Unsupported".to_owned(),
            PadAction::Play => String::new(),
        }
    }

    fn pad_color(&self, note: u8) -> Color32 {
        let color_name = self.config.pages[self.page]
            .get(&note.to_string())
            .map(|pad| pad.color.as_str())
            .unwrap_or("default");
        match color_name {
            "red" => Color32::from_rgb(90, 28, 30),
            "orange" => Color32::from_rgb(100, 53, 21),
            "amber" => Color32::from_rgb(96, 73, 17),
            "green" => Color32::from_rgb(26, 78, 40),
            "dim green" => Color32::from_rgb(25, 55, 35),
            _ => Color32::from_rgb(43, 48, 56),
        }
    }

    fn open_settings(&mut self) {
        self.settings_backup = Some(self.config.clone());
        self.refresh_output_devices();
        self.settings_open = true;
    }

    fn refresh_output_devices(&mut self) {
        match enumerate_output_devices() {
            Ok(devices) => {
                self.output_devices = devices;
                self.output_error = None;
            }
            Err(error) => {
                self.output_devices.clear();
                self.output_error = Some(error);
            }
        }
    }

    fn save_settings(&mut self) {
        match self.runtime.save_config(&self.config) {
            Ok(()) => self.status = "Settings saved".to_owned(),
            Err(error) => self.status = format!("Could not save settings: {error}"),
        }
        self.settings_backup = None;
        self.settings_open = false;
    }

    fn cancel_settings(&mut self) {
        if let Some(original) = self.settings_backup.take() {
            self.config = original;
        }
        self.settings_open = false;
    }

    fn choose_script_file(&mut self) {
        let dialog = rfd::FileDialog::new().add_filter(
            "Scripts and programs",
            &["sh", "bash", "py", "bat", "cmd", "ps1", "exe"],
        );
        if let Some(path) = dialog.pick_file() {
            match command_for_script_path(&path) {
                Ok(command) => self.editor.cmd = command,
                Err(error) => self.status = error,
            }
        }
    }

    fn choose_audio_file(&mut self) {
        let path = rfd::FileDialog::new()
            .add_filter(
                "Audio",
                &["wav", "mp3", "flac", "ogg", "opus", "m4a", "aac"],
            )
            .pick_file();
        if let Some(path) = path {
            self.cancel_loudness_analysis();
            let _ = self.runtime.stop_preview();
            self.editor.file = path.display().to_string();
            if self.editor.label.trim().is_empty() {
                self.editor.label = path
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .unwrap_or_default()
                    .to_owned();
            }
        }
    }

    fn draw_editor(&mut self, context: &Context) {
        if !self.editor_open {
            return;
        }
        let was_open = self.editor_open;
        let previous_file = self.editor.file.clone();
        let previous_action = self.editor.action.clone();
        let mut open = self.editor_open;
        egui::Window::new("Pad assignment")
            .open(&mut open)
            .resizable(true)
            .collapsible(false)
            .default_width(390.0)
            .show(context, |ui| {
                egui::ComboBox::from_label("Action")
                    .selected_text(action_label(&self.editor.action))
                    .show_ui(ui, |ui| {
                        for (id, label) in [
                            ("play", "Play sound"),
                            ("script", "Run a script"),
                            ("stop_sfx", "Stop sounds"),
                            ("stop_all", "Stop all sounds and scripts"),
                            ("stop_key", "Stop a specific pad"),
                            ("crosshair_toggle", "Toggle Crosshair"),
                        ] {
                            ui.selectable_value(&mut self.editor.action, id.to_owned(), label);
                        }
                    });

                ui.separator();
                match self.editor.action.as_str() {
                    "play" => {
                        ui.label("Audio file");
                        ui.horizontal(|ui| {
                            ui.add_enabled(
                                !self.loudness_busy,
                                egui::TextEdit::singleline(&mut self.editor.file)
                                    .desired_width(270.0)
                                    .hint_text("Path to WAV, MP3, FLAC, OGG or M4A"),
                            );
                            if ui
                                .add_enabled(!self.loudness_busy, egui::Button::new("Browse"))
                                .clicked()
                            {
                                self.choose_audio_file();
                            }
                        });
                        ui.horizontal(|ui| {
                            if ui
                                .add_enabled(
                                    !self.editor.file.trim().is_empty() && !self.loudness_busy,
                                    egui::Button::new("Preview"),
                                )
                                .clicked()
                            {
                                match self.runtime.preview_audio(&self.config, &self.editor) {
                                    Ok(()) => self.status = "Audio preview started".to_owned(),
                                    Err(error) => self.status = error,
                                }
                            }
                            if ui.button("Stop preview").clicked() {
                                match self.runtime.stop_preview() {
                                    Ok(()) => self.status = "Audio preview stopped".to_owned(),
                                    Err(error) => self.status = error,
                                }
                            }
                        });
                        ui.label("Output device - blank uses the global output");
                        ui.text_edit_singleline(&mut self.editor.sink);
                        ui.add(
                            egui::Slider::new(&mut self.editor.vol, 0..=150)
                                .suffix("%")
                                .text("Volume"),
                        );
                        ui.horizontal(|ui| {
                            let label = if self.loudness_busy {
                                "Analyzing..."
                            } else {
                                "Check loudness"
                            };
                            if ui
                                .add_enabled(!self.loudness_busy, egui::Button::new(label))
                                .clicked()
                            {
                                self.start_loudness_analysis();
                            }
                            if let Some(result) = self.loudness_result
                                && ui
                                    .button(format!(
                                        "Apply {}%",
                                        result.suggested_volume_percent
                                    ))
                                    .clicked()
                            {
                                self.editor.vol = i64::from(result.suggested_volume_percent);
                                self.loudness_message = "Suggested volume applied".to_owned();
                            }
                        });
                        if !self.loudness_message.is_empty() {
                            ui.small(&self.loudness_message);
                        }
                        ui.checkbox(&mut self.editor.loop_enabled, "Loop continuously");
                        egui::ComboBox::from_label("Second press")
                            .selected_text(retrigger_label(
                                self.editor.retrigger.as_deref().unwrap_or("global"),
                            ))
                            .show_ui(ui, |ui| {
                                for (id, label) in [
                                    ("global", "Use global setting"),
                                    ("restart", "Restart from start"),
                                    ("toggle", "Stop on second press"),
                                ] {
                                    ui.selectable_value(
                                        &mut self.editor.retrigger,
                                        Some(id.to_owned()),
                                        label,
                                    );
                                }
                            });
                    }
                    "script" => {
                        ui.horizontal(|ui| {
                            ui.label("Command or script path");
                            if ui.button("Browse").clicked() {
                                self.choose_script_file();
                            }
                        });
                        ui.text_edit_multiline(&mut self.editor.cmd);
                        ui.label("Optional PID file");
                        ui.text_edit_singleline(&mut self.editor.pid_file);
                        ui.colored_label(
                            Color32::from_rgb(200, 160, 90),
                            "Trusted command: scripts run through your shell. Only assign commands you trust.",
                        );
                        if ui.button("Test Run").clicked() {
                            match self.runtime.test_script(
                                self.page,
                                self.selected_note.unwrap_or(0),
                                self.editor.cmd.clone(),
                            ) {
                                Ok(()) => self.status = "Script test started (30-second limit)".to_owned(),
                                Err(error) => self.status = format!("Could not start script test: {error}"),
                            }
                        }
                    }
                    "stop_key" => {
                        ui.label("Target pad key (page:note)");
                        ui.text_edit_singleline(&mut self.editor.stop_key);
                    }
                    "stop_sfx" | "stop_all" | "crosshair_toggle" => {
                        ui.label(action_description(&self.editor.action));
                        if self.editor.action == "crosshair_toggle" {
                            ui.label("Crosshair is located on PATH or in its user-local install directory.");
                        }
                    }
                    _ => {
                        ui.colored_label(Color32::LIGHT_RED, "This action is not recognized.");
                    }
                }

                ui.separator();
                ui.label("Pad label");
                ui.text_edit_singleline(&mut self.editor.label);
                egui::ComboBox::from_label("Pad color")
                    .selected_text(&self.editor.color)
                    .show_ui(ui, |ui| {
                        for color in ["default", "green", "dim green", "orange", "red", "amber"] {
                            ui.selectable_value(
                                &mut self.editor.color,
                                color.to_owned(),
                                color,
                            );
                        }
                    });
                ui.label("Notes");
                ui.text_edit_multiline(&mut self.editor.notes);
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        self.save_edit();
                    }
                    if ui.button("Cancel").clicked() {
                        self.cancel_loudness_analysis();
                        let _ = self.runtime.stop_preview();
                        self.editor_open = false;
                    }
                    if self.selected_note.is_some_and(|note| {
                        self.config.pages[self.page].contains_key(&note.to_string())
                    }) && ui.button("Clear pad").clicked()
                        && let Some(note) = self.selected_note {
                            self.clear_pad(note);
                        }
                });
            });
        self.editor_open = open && self.editor_open;
        if previous_file != self.editor.file
            || (previous_action == "play" && self.editor.action != "play")
            || (was_open && !self.editor_open)
        {
            let _ = self.runtime.stop_preview();
        }
        if was_open && !self.editor_open {
            self.cancel_loudness_analysis();
        }
    }

    fn draw_settings(&mut self, context: &Context) {
        if !self.settings_open {
            return;
        }
        let mut open = self.settings_open;
        egui::Window::new("Settings")
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .default_width(380.0)
            .show(context, |ui| {
                ui.label("Default output");
                draw_output_selector(
                    ui,
                    "default_output",
                    &mut self.config.default_sink,
                    &self.output_devices,
                );

                if let Some(error) = &self.output_error {
                    ui.colored_label(Color32::LIGHT_RED, format!("Audio device discovery: {error}"));
                }
                if ui.button("Refresh audio devices").clicked() {
                    self.refresh_output_devices();
                }

                egui::ComboBox::from_label("Global second press")
                    .selected_text(retrigger_label(&self.config.retrigger))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut self.config.retrigger,
                            "restart".to_owned(),
                            "Restart from start",
                        );
                        ui.selectable_value(
                            &mut self.config.retrigger,
                            "toggle".to_owned(),
                            "Stop on second press",
                        );
                    });

                ui.checkbox(
                    &mut self.config.stop_on_page_change,
                    "Stop audio when changing pages",
                );

                ui.separator();
                ui.checkbox(
                    &mut self.config.monitor_enabled,
                    "Mirror audio to a second output",
                );
                ui.add_enabled_ui(self.config.monitor_enabled, |ui| {
                    draw_output_selector(
                        ui,
                        "monitor_output",
                        &mut self.config.monitor_sink,
                        &self.output_devices,
                    );
                });
                ui.label(
                    RichText::new("A second stream is opened only when the selected output is different from the primary output.")
                        .size(10.0)
                        .color(Color32::from_rgb(150, 157, 168)),
                );

                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        self.save_settings();
                    }
                    if ui.button("Cancel").clicked() {
                        self.cancel_settings();
                    }
                });
            });
        if !open && self.settings_open {
            self.cancel_settings();
        }
        self.settings_open = open && self.settings_open;
    }

    fn draw_top_bar(&mut self, ui: &mut egui::Ui) {
        let elapsed = self.meter_last_update.elapsed().min(Duration::from_secs(1));
        self.meter_last_update = Instant::now();
        let measured_level = self.runtime.meter_level();
        self.meter_level_display =
            smooth_meter_level(self.meter_level_display, measured_level, elapsed);

        ui.horizontal_centered(|ui| {
            ui.add_space(6.0);
            ui.label(RichText::new("PADFIRE").strong().size(14.0));
            ui.separator();
            let state = if self.runtime.midi_connected() {
                "Launchpad connected"
            } else {
                "Launchpad unavailable"
            };
            let color = if self.runtime.midi_connected() {
                Color32::from_rgb(110, 185, 130)
            } else {
                Color32::from_rgb(155, 160, 170)
            };
            ui.colored_label(color, state);
            ui.add(
                egui::ProgressBar::new(self.meter_level_display)
                    .desired_width(84.0)
                    .desired_height(4.0)
                    .fill(Color32::from_rgb(100, 175, 215)),
            )
            .on_hover_text("Selected output system-loopback level while Padfire audio is active; falls back to Padfire's own mix when loopback is unavailable");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("Settings").clicked() {
                    self.open_settings();
                }
                let mut monitor = self.config.monitor_enabled;
                if ui.checkbox(&mut monitor, "Mon").changed() {
                    self.config.monitor_enabled = monitor;
                    match self.runtime.save_config(&self.config) {
                        Ok(()) => {
                            self.status = if monitor {
                                "Audio monitoring enabled".to_owned()
                            } else {
                                "Audio monitoring disabled".to_owned()
                            }
                        }
                        Err(error) => self.status = format!("Could not save settings: {error}"),
                    }
                }
                if ui.button("Stop").clicked() {
                    self.runtime.stop_audio();
                    self.status = "Audio playback stopped".to_owned();
                }
            });
        });
    }

    fn draw_page_selector(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            for index in 0..PAGE_COUNT {
                let selected = self.page == index;
                let mut button =
                    egui::Button::new(RichText::new((index + 1).to_string()).size(12.0).strong())
                        .min_size(Vec2::new(43.0, 25.0));
                if selected {
                    button = button.fill(Color32::from_rgb(36, 91, 147));
                }
                if ui.add(button).clicked() {
                    self.change_page(index);
                }
            }
        });
    }

    fn draw_grid(&mut self, ui: &mut egui::Ui) {
        Grid::new("launchpad_grid")
            .num_columns(9)
            .spacing(Vec2::new(3.0, 3.0))
            .show(ui, |ui| {
                for row in 0..8_u8 {
                    for column in 0..8_u8 {
                        self.draw_pad(ui, row * 16 + column);
                    }
                    self.draw_pad(ui, row * 16 + 8);
                    ui.end_row();
                }
            });
    }

    fn draw_pad(&mut self, ui: &mut egui::Ui, note: u8) {
        let label = self.pad_name(note);
        let color = self.pad_color(note);
        let text = if label.is_empty() { " " } else { &label };
        let response = ui
            .add_sized(
                Vec2::new(49.0, 49.0),
                egui::Button::new(
                    RichText::new(text)
                        .size(9.0)
                        .color(Color32::from_rgb(226, 231, 237)),
                )
                .fill(color)
                .stroke(Stroke::new(1.0, Color32::from_rgb(65, 71, 80))),
            )
            .interact(egui::Sense::click_and_drag());

        response.dnd_set_drag_payload(PadDragPayload {
            page: self.page,
            note,
        });
        if response.dnd_hover_payload::<PadDragPayload>().is_some() {
            ui.painter().rect_stroke(
                response.rect,
                3.0,
                Stroke::new(2.0, Color32::from_rgb(90, 155, 225)),
                egui::StrokeKind::Outside,
            );
        }
        if let Some(source) = response.dnd_release_payload::<PadDragPayload>()
            && (source.page != self.page || source.note != note)
        {
            let source_assignment = self
                .config
                .pages
                .get(source.page)
                .and_then(|page| page.get(&source.note.to_string()))
                .cloned();
            if let Some(assignment) = source_assignment {
                self.runtime.stop_pad(PadKey::new(self.page as u8, note));
                let copy = ui.ctx().input(|input| input.modifiers.ctrl);
                if !copy {
                    self.runtime
                        .stop_pad(PadKey::new(source.page as u8, source.note));
                    self.config.pages[source.page].remove(&source.note.to_string());
                }
                self.config.pages[self.page].insert(note.to_string(), assignment);
                match self.runtime.save_config(&self.config) {
                    Ok(()) => {
                        self.status = if copy {
                            format!(
                                "Copied pad {} to {}",
                                PadKey::new(source.page as u8, source.note).to_key(),
                                PadKey::new(self.page as u8, note).to_key()
                            )
                        } else {
                            format!(
                                "Moved pad to {}",
                                PadKey::new(self.page as u8, note).to_key()
                            )
                        }
                    }
                    Err(error) => self.status = format!("Could not save pad move: {error}"),
                }
            }
        }

        if response.clicked() {
            if self.config.pages[self.page].contains_key(&note.to_string()) {
                self.runtime
                    .trigger_pad(&self.config, self.page, note, &mut self.status);
            } else {
                self.begin_edit(note);
            }
        }

        let mut menu_edit = false;
        let mut menu_trigger = false;
        let mut menu_stop = false;
        let mut menu_clear = false;
        response.context_menu(|ui| {
            if ui.button("Edit...").clicked() {
                menu_edit = true;
                ui.close();
            }
            if ui.button("Trigger").clicked() {
                menu_trigger = true;
                ui.close();
            }
            if ui.button("Stop this pad").clicked() {
                menu_stop = true;
                ui.close();
            }
            ui.separator();
            if ui.button("Clear pad").clicked() {
                menu_clear = true;
                ui.close();
            }
        });

        if menu_edit {
            self.begin_edit(note);
        }
        if menu_trigger && self.config.pages[self.page].contains_key(&note.to_string()) {
            self.runtime
                .trigger_pad(&self.config, self.page, note, &mut self.status);
        }
        if menu_stop {
            self.runtime.stop_pad(PadKey::new(self.page as u8, note));
            self.status = format!(
                "Stopped pad {}",
                PadKey::new(self.page as u8, note).to_key()
            );
        }
        if menu_clear {
            self.clear_pad(note);
        }

        // File drag-and-drop assigns a new audio source to the pad under the
        // pointer. The pad editor opens so volume, output and retrigger can be
        // reviewed before the assignment is saved.
        let dropped_path = if response.hovered() {
            ui.ctx().input(|input| {
                input
                    .raw
                    .dropped_files
                    .first()
                    .map(|file| file.path().to_path_buf())
            })
        } else {
            None
        };
        if let Some(path) = dropped_path {
            let file = path.display().to_string();
            let mut assignment = self.config.pages[self.page]
                .get(&note.to_string())
                .cloned()
                .unwrap_or_default();
            assignment.action = "play".to_owned();
            assignment.file = file;
            if assignment.label.trim().is_empty() {
                assignment.label = path
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .unwrap_or_default()
                    .to_owned();
            }
            self.selected_note = Some(note);
            self.editor = assignment;
            self.editor_open = true;
        }

        response.on_hover_text(format!("Pad {} / note {}", note, note));
    }

    fn handle_keyboard(&mut self, context: &Context) {
        let keyboard_is_editing = context.egui_wants_keyboard_input();
        let (number_keys, space_pressed, quit, escape) = context.input(|input| {
            (
                [
                    egui::Key::Num1,
                    egui::Key::Num2,
                    egui::Key::Num3,
                    egui::Key::Num4,
                    egui::Key::Num5,
                    egui::Key::Num6,
                    egui::Key::Num7,
                    egui::Key::Num8,
                ]
                .map(|key| input.key_pressed(key)),
                input.key_pressed(egui::Key::Space),
                input.modifiers.ctrl && input.key_pressed(egui::Key::Q),
                input.key_pressed(egui::Key::Escape),
            )
        });
        let number_page = page_shortcut_index(keyboard_is_editing, number_keys);
        let stop_audio = stop_all_shortcut_enabled(keyboard_is_editing, space_pressed);

        if let Some(page) = number_page {
            self.change_page(page);
        }
        if stop_audio {
            self.runtime.stop_audio();
            self.status = "Audio playback stopped".to_owned();
        }
        if quit {
            self.request_quit(context);
        } else if escape {
            match escape_behavior(self.tray_available()) {
                EscapeBehavior::HideToTray => {
                    context.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                    self.status = "Padfire is running in the tray".to_owned();
                }
                EscapeBehavior::Minimize => {
                    context.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                }
            }
        }
    }
}

impl eframe::App for PadfireApp {
    fn ui(&mut self, root_ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let context = root_ui.ctx().clone();
        context.set_visuals(egui::Visuals::dark());
        // IPC, MIDI and process/audio workers post events from other threads.
        // eframe does not continuously call ui() while idle, so schedule a
        // low-frequency poll instead of leaving those commands stranded.
        context.request_repaint_after(std::time::Duration::from_millis(200));

        #[cfg(any(target_os = "linux", windows))]
        {
            self.ensure_tray();
            self.handle_tray_events(&context);
        }

        self.runtime
            .poll(&mut self.config, &mut self.page, &mut self.status);
        self.poll_loudness_analysis();
        let signals = self.runtime.take_signals();
        if signals.show_requested {
            context.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            context.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            context.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
        if signals.quit_requested {
            self.request_quit(&context);
        }
        self.handle_keyboard(&context);
        self.handle_window_close_request(&context);

        egui::Panel::top("top_bar")
            .exact_size(40.0)
            .show(root_ui, |ui| self.draw_top_bar(ui));

        egui::Panel::bottom("status_bar")
            .exact_size(25.0)
            .show(root_ui, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.label(
                        RichText::new(&self.status)
                            .size(10.0)
                            .color(Color32::from_rgb(155, 162, 172)),
                    );
                });
            });

        egui::CentralPanel::default().show(root_ui, |ui| {
            self.draw_page_selector(ui);
            ui.add_space(7.0);
            self.draw_grid(ui);
            ui.add_space(7.0);
            ui.label(
                RichText::new(
                    "Click an assigned pad to play. Right-click an empty or assigned pad to edit.",
                )
                .size(10.0)
                .color(Color32::from_rgb(143, 150, 161)),
            );
        });

        self.draw_editor(&context);
        self.draw_settings(&context);
    }
}

fn draw_output_selector(
    ui: &mut egui::Ui,
    id: &'static str,
    selected: &mut String,
    devices: &[OutputDevice],
) {
    let selected_label = if selected.trim().is_empty() {
        "System default".to_owned()
    } else {
        devices
            .iter()
            .find(|device| device.id == *selected || device.name == *selected)
            .map(|device| device.name.clone())
            .unwrap_or_else(|| format!("Unavailable: {selected}"))
    };
    egui::ComboBox::from_id_salt(id)
        .selected_text(selected_label)
        .show_ui(ui, |ui| {
            ui.selectable_value(selected, String::new(), "System default");
            for device in devices {
                ui.selectable_value(selected, device.id.clone(), device.name.clone());
            }
        });
}

fn smooth_meter_level(current: f32, target: f32, elapsed: Duration) -> f32 {
    let time_constant = if target > current { 0.035 } else { 0.24 };
    let alpha = 1.0 - (-elapsed.as_secs_f32() / time_constant).exp();
    current + (target - current) * alpha
}

impl Drop for PadfireApp {
    fn drop(&mut self) {
        self.cancel_loudness_analysis();
    }
}

#[cfg(any(target_os = "linux", windows))]
fn build_tray_icon() -> Result<tray_icon::TrayIcon, String> {
    use tray_icon::{
        Icon, TrayIconBuilder,
        menu::{Menu, MenuItem},
    };

    let show = MenuItem::with_id("padfire-show", "Show Padfire", true, None);
    let stop = MenuItem::with_id("padfire-stop", "Stop audio", true, None);
    let quit = MenuItem::with_id("padfire-quit", "Quit Padfire", true, None);
    let menu = Menu::with_items(&[&show, &stop, &quit])
        .map_err(|error| format!("could not create tray menu: {error}"))?;
    let icon = Icon::from_rgba(padfire_tray_pixels(), 32, 32)
        .map_err(|error| format!("could not create tray icon: {error}"))?;

    TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("Padfire")
        .with_icon(icon)
        .build()
        .map_err(|error| format!("could not register tray icon: {error}"))
}

#[cfg(any(target_os = "linux", windows))]
fn padfire_tray_pixels() -> Vec<u8> {
    let mut pixels = vec![0_u8; 32 * 32 * 4];
    for y in 0_i32..32 {
        for x in 0_i32..32 {
            let nearest_x = x.clamp(8, 23);
            let nearest_y = y.clamp(8, 23);
            let dx = x - nearest_x;
            let dy = y - nearest_y;
            let inside = (4..28).contains(&x) && (4..28).contains(&y) && dx * dx + dy * dy <= 16;
            if !inside {
                continue;
            }

            let is_p = (9..13).contains(&x) && (8..24).contains(&y)
                || (9..22).contains(&x) && (8..12).contains(&y)
                || (9..21).contains(&x) && (15..19).contains(&y)
                || (18..22).contains(&x) && (8..19).contains(&y);
            let color = if is_p {
                [116, 190, 224, 255]
            } else {
                [27, 36, 48, 255]
            };
            let offset = ((y * 32 + x) * 4) as usize;
            pixels[offset..offset + 4].copy_from_slice(&color);
        }
    }
    pixels
}

fn action_label(action: &str) -> &str {
    match action {
        "play" => "Play sound",
        "script" => "Run a script",
        "stop_sfx" => "Stop sounds",
        "stop_all" => "Stop all sounds and scripts",
        "stop_key" => "Stop a specific pad",
        "crosshair_toggle" => "Toggle Crosshair",
        _ => "Unsupported action",
    }
}

fn action_description(action: &str) -> &str {
    match action {
        "stop_sfx" => "Stop all current audio playback. Script pads are not affected.",
        "stop_all" => "Stop audio playback and terminate all Padfire-managed script processes.",
        "crosshair_toggle" => {
            "Run crosshair --toggle using a direct process launch, without a shell."
        }
        _ => "",
    }
}

fn retrigger_label(value: &str) -> &str {
    match value {
        "toggle" => "Stop on second press",
        "restart" => "Restart from start",
        _ => "Use global setting",
    }
}

#[cfg(test)]
mod meter_smoothing_tests {
    use super::*;

    #[test]
    fn meter_attack_is_faster_than_release() {
        let interval = Duration::from_millis(35);
        let attack = smooth_meter_level(0.0, 1.0, interval);
        let release = 1.0 - smooth_meter_level(1.0, 0.0, interval);
        assert!(attack > 0.6);
        assert!(release > 0.1 && release < 0.2);
        assert!(attack > release);
    }

    #[test]
    fn zero_elapsed_time_does_not_change_the_meter() {
        assert_eq!(smooth_meter_level(0.42, 0.9, Duration::ZERO), 0.42);
    }
}

#[cfg(test)]
mod script_picker_tests {
    use super::*;

    #[cfg(target_os = "linux")]
    #[test]
    fn script_picker_quotes_shell_paths_with_spaces_and_apostrophes() {
        use std::process::Command;

        let path = "/tmp/a user's script.sh";
        let command = command_for_script_path(Path::new(path)).unwrap();
        assert!(command.starts_with("sh "));
        let quoted_argument = command.strip_prefix("sh ").unwrap();
        let output = Command::new("/bin/sh")
            .args(["-c", &format!("printf %s {quoted_argument}")])
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(String::from_utf8(output.stdout).unwrap(), path);

        let python = command_for_script_path(Path::new("/tmp/test script.py")).unwrap();
        assert!(python.ends_with("'/tmp/test script.py'"));
    }

    #[cfg(windows)]
    #[test]
    fn script_picker_quotes_batch_and_powershell_paths() {
        assert_eq!(
            command_for_script_path(Path::new(r"C:\Tools\My script.cmd")).unwrap(),
            r#"call "C:\Tools\My script.cmd""#
        );
        assert_eq!(
            command_for_script_path(Path::new(r"C:\Tools\My script.ps1")).unwrap(),
            r#"powershell.exe -NoProfile -ExecutionPolicy Bypass -File "C:\Tools\My script.ps1""#
        );
    }

    #[cfg(windows)]
    #[test]
    fn browsed_batch_file_with_spaces_executes_through_cmd() {
        use std::fs;
        use std::process::Command;

        let script =
            std::env::temp_dir().join(format!("Padfire script picker {}.cmd", std::process::id()));
        fs::write(
            &script,
            "@echo off\r\necho PADFIRE_PICKER_OK\r\nexit /b 0\r\n",
        )
        .unwrap();
        let command = command_for_script_path(&script).unwrap();
        use std::os::windows::process::CommandExt;
        let output = Command::new("cmd.exe")
            .arg("/D")
            .arg("/C")
            .raw_arg(&command)
            .output()
            .unwrap();
        let _ = fs::remove_file(&script);

        assert!(
            output.status.success(),
            "browsed batch command failed: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("PADFIRE_PICKER_OK"));
    }

    #[cfg(windows)]
    #[test]
    fn script_picker_rejects_cmd_expansion_paths() {
        assert!(command_for_script_path(Path::new(r"C:\Tools\100%script.cmd")).is_err());
        assert!(command_for_script_path(Path::new(r"C:\Tools\!script.cmd")).is_err());
    }
}

#[cfg(test)]
mod ui_policy_tests {
    use super::*;

    #[test]
    fn titlebar_close_hides_only_when_tray_exists_and_quit_was_not_requested() {
        assert!(should_hide_on_close(true, false, true));
        assert!(!should_hide_on_close(true, false, false));
        assert!(!should_hide_on_close(true, true, true));
        assert!(!should_hide_on_close(false, false, true));
    }

    #[test]
    fn escape_hides_to_tray_or_minimizes_without_one() {
        assert_eq!(escape_behavior(true), EscapeBehavior::HideToTray);
        assert_eq!(escape_behavior(false), EscapeBehavior::Minimize);
    }

    #[test]
    fn page_and_space_shortcuts_do_not_intercept_text_editing() {
        let page_three = [false, false, true, false, false, false, false, false];
        assert_eq!(page_shortcut_index(false, page_three), Some(2));
        assert_eq!(page_shortcut_index(true, page_three), None);
        assert!(stop_all_shortcut_enabled(false, true));
        assert!(!stop_all_shortcut_enabled(true, true));
        assert!(!stop_all_shortcut_enabled(false, false));
    }

    #[test]
    fn keypad_numbers_resolve_to_the_same_page_shortcuts() {
        for (index, name) in [
            "Numpad1", "Numpad2", "Numpad3", "Numpad4", "Numpad5", "Numpad6", "Numpad7", "Numpad8",
        ]
        .into_iter()
        .enumerate()
        {
            let key = egui::Key::from_name(name).expect("egui recognizes numpad key names");
            assert_eq!(
                key,
                egui::Key::from_name(&format!("Digit{}", index + 1)).unwrap()
            );
        }
    }
}
