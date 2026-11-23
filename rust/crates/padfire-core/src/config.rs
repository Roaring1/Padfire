//! The v5 JSON configuration contract, migration and validation.
//!
//! The Rust port intentionally reads the existing v2.4.0 config format. The
//! on-disk schema stays at v5 during the port so the old and new binaries can
//! be alternated without a one-way config migration.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::fmt;

pub const CONFIG_VERSION: u64 = 5;
pub const PAGE_COUNT: usize = 8;
pub const MAX_CONFIG_BYTES: usize = 4 * 1024 * 1024;
/// Page keys and runtime PadKey identities use an unsigned byte.
pub const MAX_CONFIG_PAGES: usize = 256;
/// A Launchpad has 128 MIDI note identities per page.
pub const MAX_PADS_PER_PAGE: usize = 128;
const MAX_CONFIG_PATH_BYTES: usize = 4096;
const MAX_CONFIG_COMMAND_BYTES: usize = 8192;
const MAX_CONFIG_LABEL_BYTES: usize = 512;
const MAX_CONFIG_NOTES_BYTES: usize = 16 * 1024;
const MAX_CONFIG_JSON_DEPTH: usize = 64;
const MAX_CONFIG_JSON_NODES: usize = 100_000;

fn default_version() -> u64 {
    1
}

fn default_retrigger() -> String {
    "restart".to_owned()
}

fn default_action() -> String {
    "play".to_owned()
}

fn default_volume() -> i64 {
    80
}

fn default_color() -> String {
    "default".to_owned()
}

fn empty_pages() -> Vec<PadPage> {
    (0..PAGE_COUNT).map(|_| BTreeMap::new()).collect()
}

/// Per-page map keyed by the Launchpad note number encoded as a decimal string.
pub type PadPage = BTreeMap<String, PadAssignment>;

/// Shared configuration. Unknown root and pad fields are retained so this
/// release does not erase data written by a compatible older or newer build.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default = "default_version")]
    pub version: u64,
    #[serde(default)]
    pub default_sink: String,
    #[serde(default)]
    pub stop_on_page_change: bool,
    #[serde(default = "default_retrigger")]
    pub retrigger: String,
    #[serde(default)]
    pub monitor_enabled: bool,
    #[serde(default)]
    pub monitor_sink: String,
    #[serde(default = "empty_pages")]
    pub pages: Vec<PadPage>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION,
            default_sink: String::new(),
            stop_on_page_change: false,
            retrigger: default_retrigger(),
            monitor_enabled: false,
            monitor_sink: String::new(),
            pages: empty_pages(),
            extra: BTreeMap::new(),
        }
    }
}

/// A pad's saved action. Unknown action strings are disabled rather than
/// silently executed as a different action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PadAction {
    Play,
    Script,
    StopSfx,
    StopAll,
    StopKey,
    CrosshairToggle,
    Unsupported(String),
}

impl PadAction {
    pub fn from_config(value: &str) -> Self {
        match value {
            "play" => Self::Play,
            "script" => Self::Script,
            "stop_sfx" => Self::StopSfx,
            "stop_all" => Self::StopAll,
            "stop_key" => Self::StopKey,
            "crosshair_toggle" => Self::CrosshairToggle,
            other => Self::Unsupported(other.to_owned()),
        }
    }

    pub fn as_config_str(&self) -> &str {
        match self {
            Self::Play => "play",
            Self::Script => "script",
            Self::StopSfx => "stop_sfx",
            Self::StopAll => "stop_all",
            Self::StopKey => "stop_key",
            Self::CrosshairToggle => "crosshair_toggle",
            Self::Unsupported(value) => value,
        }
    }
}

/// A pad-level override can inherit the global second-press behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetriggerMode {
    Restart,
    Toggle,
}

impl RetriggerMode {
    pub fn from_global(value: &str) -> Self {
        if value == "toggle" {
            Self::Toggle
        } else {
            Self::Restart
        }
    }

    pub fn resolve_pad_override(override_value: Option<&str>, global: &str) -> Self {
        match override_value.unwrap_or("global") {
            "toggle" => Self::Toggle,
            "global" => Self::from_global(global),
            // The released implementation treats an unknown explicit pad
            // value as restart rather than falling through to global.
            _ => Self::Restart,
        }
    }
}

/// Data for one Launchpad control. Extra properties round-trip so an older
/// build cannot erase forward-compatible data merely by opening and saving.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PadAssignment {
    #[serde(default = "default_action")]
    pub action: String,
    #[serde(default)]
    pub file: String,
    #[serde(default)]
    pub sink: String,
    #[serde(default = "default_volume")]
    pub vol: i64,
    #[serde(default, rename = "loop")]
    pub loop_enabled: bool,
    #[serde(default)]
    pub label: String,
    #[serde(default = "default_color")]
    pub color: String,
    #[serde(default)]
    pub cmd: String,
    #[serde(default)]
    pub pid_file: String,
    #[serde(default)]
    pub stop_key: String,
    #[serde(default)]
    pub retrigger: Option<String>,
    #[serde(default)]
    pub notes: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl Default for PadAssignment {
    fn default() -> Self {
        Self {
            action: default_action(),
            file: String::new(),
            sink: String::new(),
            vol: default_volume(),
            loop_enabled: false,
            label: String::new(),
            color: default_color(),
            cmd: String::new(),
            pid_file: String::new(),
            stop_key: String::new(),
            retrigger: None,
            notes: String::new(),
            extra: BTreeMap::new(),
        }
    }
}

impl PadAssignment {
    pub fn action_kind(&self) -> PadAction {
        PadAction::from_config(&self.action)
    }

    pub fn effective_retrigger(&self, global: &str) -> RetriggerMode {
        RetriggerMode::resolve_pad_override(self.retrigger.as_deref(), global)
    }

    pub fn effective_volume(&self) -> u8 {
        self.vol.clamp(0, 150) as u8
    }

    pub fn is_configured(&self) -> bool {
        !matches!(self.action_kind(), PadAction::Play) || !self.file.trim().is_empty()
    }

    pub fn effective_label(&self) -> String {
        if !self.label.trim().is_empty() {
            return self.label.trim().to_owned();
        }
        if !self.file.trim().is_empty() {
            return std::path::Path::new(&self.file)
                .file_stem()
                .and_then(|part| part.to_str())
                .unwrap_or(&self.file)
                .to_owned();
        }
        match self.action_kind() {
            PadAction::StopSfx => "Stop SFX".to_owned(),
            PadAction::StopAll => "Stop All".to_owned(),
            PadAction::StopKey => "Stop".to_owned(),
            PadAction::CrosshairToggle => "Crosshair".to_owned(),
            PadAction::Script => std::path::Path::new(&self.cmd)
                .file_stem()
                .and_then(|part| part.to_str())
                .unwrap_or("Script")
                .to_owned(),
            PadAction::Unsupported(_) | PadAction::Play => String::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationIssue {
    pub path: String,
    pub message: String,
}

#[derive(Debug)]
pub enum ConfigError {
    TooLarge { actual: usize, maximum: usize },
    Json(serde_json::Error),
    InvalidRoot,
    InvalidStructure(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge { actual, maximum } => {
                write!(f, "configuration is {actual} bytes; maximum is {maximum}")
            }
            Self::Json(error) => write!(f, "invalid configuration JSON: {error}"),
            Self::InvalidRoot => write!(f, "configuration root must be a JSON object"),
            Self::InvalidStructure(message) => {
                write!(f, "configuration structure is unsafe: {message}")
            }
        }
    }
}

impl std::error::Error for ConfigError {}

impl From<serde_json::Error> for ConfigError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

/// Bound the whole extension-preserving JSON tree before typed
/// deserialization. This is iterative so validation itself does not recurse
/// over attacker-controlled nesting.
fn validate_json_budget(value: &Value) -> Result<(), ConfigError> {
    let mut pending = vec![(value, 0_usize)];
    let mut nodes = 0_usize;
    while let Some((current, depth)) = pending.pop() {
        nodes += 1;
        if nodes > MAX_CONFIG_JSON_NODES {
            return Err(ConfigError::InvalidStructure(format!(
                "JSON document exceeds the {MAX_CONFIG_JSON_NODES}-node budget"
            )));
        }
        if depth > MAX_CONFIG_JSON_DEPTH {
            return Err(ConfigError::InvalidStructure(format!(
                "JSON document exceeds the maximum nesting depth of {MAX_CONFIG_JSON_DEPTH}"
            )));
        }
        match current {
            Value::Array(items) => {
                pending.extend(items.iter().map(|item| (item, depth + 1)));
            }
            Value::Object(fields) => {
                pending.extend(fields.values().map(|item| (item, depth + 1)));
            }
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
        }
    }
    Ok(())
}

fn validate_bounded_string(
    object: &Map<String, Value>,
    field: &str,
    maximum_bytes: usize,
) -> Result<(), ConfigError> {
    let Some(value) = object.get(field).and_then(Value::as_str) else {
        // Type errors are still reported by the typed serde conversion below.
        return Ok(());
    };
    if value.len() > maximum_bytes {
        return Err(ConfigError::InvalidStructure(format!(
            "{field} is {} bytes; maximum is {maximum_bytes}",
            value.len()
        )));
    }
    if value.contains('\0') {
        return Err(ConfigError::InvalidStructure(format!(
            "{field} contains a NUL character"
        )));
    }
    Ok(())
}

fn with_config_path(error: ConfigError, prefix: &str) -> ConfigError {
    match error {
        ConfigError::InvalidStructure(message) => {
            ConfigError::InvalidStructure(format!("{prefix}.{message}"))
        }
        other => other,
    }
}

impl AppConfig {
    /// Parse and normalize a v1-v5 Padfire configuration.
    ///
    /// This mirrors the released v2.4.0 migration: it removes only the known
    /// retired fields, appends pages to a minimum of eight, adds the v4 action
    /// default, and repairs invalid global retrigger settings. It deliberately
    /// does not truncate extra pages or rewrite arbitrary volume values.
    pub fn from_json(input: &[u8]) -> Result<Self, ConfigError> {
        if input.len() > MAX_CONFIG_BYTES {
            return Err(ConfigError::TooLarge {
                actual: input.len(),
                maximum: MAX_CONFIG_BYTES,
            });
        }

        let mut value: Value = serde_json::from_slice(input)?;
        validate_json_budget(&value)?;
        let Some(root) = value.as_object_mut() else {
            return Err(ConfigError::InvalidRoot);
        };

        let old_version = root.get("version").and_then(Value::as_u64).unwrap_or(1);
        validate_bounded_string(root, "default_sink", MAX_CONFIG_PATH_BYTES)?;
        validate_bounded_string(root, "monitor_sink", MAX_CONFIG_PATH_BYTES)?;

        // These are the two properties explicitly retired by v2.4.0.
        root.remove("device");

        let pages_value = root
            .entry("pages".to_owned())
            .or_insert_with(|| Value::Array(Vec::new()));
        if !pages_value.is_array() {
            *pages_value = Value::Array(Vec::new());
        }
        let pages = pages_value.as_array_mut().expect("pages normalized above");
        if pages.len() > MAX_CONFIG_PAGES {
            return Err(ConfigError::InvalidStructure(format!(
                "pages contains {}; maximum is {MAX_CONFIG_PAGES}",
                pages.len()
            )));
        }
        while pages.len() < PAGE_COUNT {
            pages.push(Value::Object(Map::new()));
        }

        for (page_index, page) in pages.iter_mut().enumerate() {
            if !page.is_object() {
                *page = Value::Object(Map::new());
            }
            let page_object = page.as_object_mut().expect("page normalized above");
            if page_object.len() > MAX_PADS_PER_PAGE {
                return Err(ConfigError::InvalidStructure(format!(
                    "pages[{page_index}] contains {} pad entries; maximum is {MAX_PADS_PER_PAGE}",
                    page_object.len()
                )));
            }
            for (pad_key, pad) in page_object.iter_mut() {
                if pad_key.len() > 16 {
                    return Err(ConfigError::InvalidStructure(format!(
                        "pages[{page_index}] contains an oversized pad key"
                    )));
                }
                if !pad.is_object() {
                    *pad = Value::Object(Map::new());
                }
                let pad_object = pad.as_object_mut().expect("pad normalized above");
                let pad_path = format!("pages[{page_index}][{pad_key}]");
                validate_bounded_string(pad_object, "action", 64)
                    .map_err(|error| with_config_path(error, &pad_path))?;
                validate_bounded_string(pad_object, "file", MAX_CONFIG_PATH_BYTES)
                    .map_err(|error| with_config_path(error, &pad_path))?;
                validate_bounded_string(pad_object, "sink", MAX_CONFIG_PATH_BYTES)
                    .map_err(|error| with_config_path(error, &pad_path))?;
                validate_bounded_string(pad_object, "cmd", MAX_CONFIG_COMMAND_BYTES)
                    .map_err(|error| with_config_path(error, &pad_path))?;
                validate_bounded_string(pad_object, "pid_file", MAX_CONFIG_PATH_BYTES)
                    .map_err(|error| with_config_path(error, &pad_path))?;
                validate_bounded_string(pad_object, "label", MAX_CONFIG_LABEL_BYTES)
                    .map_err(|error| with_config_path(error, &pad_path))?;
                validate_bounded_string(pad_object, "color", MAX_CONFIG_LABEL_BYTES)
                    .map_err(|error| with_config_path(error, &pad_path))?;
                validate_bounded_string(pad_object, "stop_key", MAX_CONFIG_LABEL_BYTES)
                    .map_err(|error| with_config_path(error, &pad_path))?;
                validate_bounded_string(pad_object, "notes", MAX_CONFIG_NOTES_BYTES)
                    .map_err(|error| with_config_path(error, &pad_path))?;
                validate_bounded_string(pad_object, "retrigger", 64)
                    .map_err(|error| with_config_path(error, &pad_path))?;
                if old_version < 4 && !pad_object.contains_key("action") {
                    pad_object.insert("action".to_owned(), Value::String("play".to_owned()));
                }
                pad_object.remove("mode");
            }
        }

        let retrigger_is_valid = root
            .get("retrigger")
            .and_then(Value::as_str)
            .is_some_and(|value| matches!(value, "restart" | "toggle"));
        if !retrigger_is_valid {
            root.insert("retrigger".to_owned(), Value::String("restart".to_owned()));
        }
        root.insert("version".to_owned(), Value::from(CONFIG_VERSION));

        Ok(serde_json::from_value(value)?)
    }

    pub fn to_pretty_json(&self) -> Result<Vec<u8>, serde_json::Error> {
        let mut normalized = self.clone();
        normalized.version = CONFIG_VERSION;
        let mut bytes = serde_json::to_vec_pretty(&normalized)?;
        bytes.push(b'\n');
        Ok(bytes)
    }

    pub fn validate(&self) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();
        if self.version != CONFIG_VERSION {
            issues.push(ValidationIssue {
                path: "version".to_owned(),
                message: format!("Expected config version {CONFIG_VERSION}."),
            });
        }
        if self.pages.len() < PAGE_COUNT {
            issues.push(ValidationIssue {
                path: "pages".to_owned(),
                message: format!(
                    "Expected at least {PAGE_COUNT} pages; found {}.",
                    self.pages.len()
                ),
            });
        }
        for (page_index, page) in self.pages.iter().enumerate() {
            for (note, pad) in page {
                if matches!(pad.action_kind(), PadAction::Unsupported(_)) {
                    issues.push(ValidationIssue {
                        path: format!("pages[{page_index}][{note}].action"),
                        message: format!("Unknown action '{}' is disabled.", pad.action),
                    });
                }
            }
        }
        issues
    }
}
