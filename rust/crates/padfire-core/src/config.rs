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
pub const UI_SCALE_MIN_PERCENT: u16 = 75;
pub const UI_SCALE_MAX_PERCENT: u16 = 125;
pub const DEFAULT_UI_SCALE_PERCENT: u16 = 100;
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

fn default_ui_scale_percent() -> u16 {
    DEFAULT_UI_SCALE_PERCENT
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
    #[serde(default = "default_ui_scale_percent")]
    pub ui_scale_percent: u16,
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
            ui_scale_percent: DEFAULT_UI_SCALE_PERCENT,
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

/// Parse JSON into the extension-preserving Value tree while enforcing
/// limits before allocating attacker-controlled arrays, maps, or strings.
/// This keeps the untrusted byte stream out of serde's general-purpose
/// deserializer; the later typed conversion receives only this bounded tree.
struct BoundedJsonParser<'a> {
    input: &'a [u8],
    cursor: usize,
    nodes: usize,
}

impl<'a> BoundedJsonParser<'a> {
    fn new(input: &'a [u8]) -> Self {
        Self {
            input,
            cursor: 0,
            nodes: 0,
        }
    }

    fn parse(mut self) -> Result<Value, ConfigError> {
        self.skip_whitespace();
        let value = self.parse_value(0)?;
        self.skip_whitespace();
        if self.cursor != self.input.len() {
            return self.invalid("trailing data after the configuration JSON value");
        }
        Ok(value)
    }

    fn parse_value(&mut self, depth: usize) -> Result<Value, ConfigError> {
        if depth > MAX_CONFIG_JSON_DEPTH {
            return self.invalid("configuration JSON exceeds the maximum nesting depth");
        }
        self.nodes += 1;
        if self.nodes > MAX_CONFIG_JSON_NODES {
            return self.invalid("configuration JSON exceeds the node budget");
        }

        self.skip_whitespace();
        match self.peek() {
            Some(b'n') => {
                self.consume_keyword(b"null")?;
                Ok(Value::Null)
            }
            Some(b't') => {
                self.consume_keyword(b"true")?;
                Ok(Value::Bool(true))
            }
            Some(b'f') => {
                self.consume_keyword(b"false")?;
                Ok(Value::Bool(false))
            }
            Some(b'"') => Ok(Value::String(self.parse_string()?)),
            Some(b'[') => self.parse_array(depth),
            Some(b'{') => self.parse_object(depth),
            Some(b'-' | b'0'..=b'9') => self.parse_number(),
            Some(_) => self.invalid("unexpected token in configuration JSON"),
            None => self.invalid("unexpected end of configuration JSON"),
        }
    }

    fn parse_array(&mut self, depth: usize) -> Result<Value, ConfigError> {
        self.cursor += 1; // [
        self.skip_whitespace();
        let mut items = Vec::new();
        if self.peek() == Some(b']') {
            self.cursor += 1;
            return Ok(Value::Array(items));
        }

        loop {
            items.push(self.parse_value(depth + 1)?);
            self.skip_whitespace();
            match self.peek() {
                Some(b',') => {
                    self.cursor += 1;
                    self.skip_whitespace();
                }
                Some(b']') => {
                    self.cursor += 1;
                    break;
                }
                Some(_) => return self.invalid("expected ',' or ']' in configuration array"),
                None => return self.invalid("unterminated configuration array"),
            }
        }
        Ok(Value::Array(items))
    }

    fn parse_object(&mut self, depth: usize) -> Result<Value, ConfigError> {
        self.cursor += 1; // {
        self.skip_whitespace();
        let mut fields = Map::new();
        if self.peek() == Some(b'}') {
            self.cursor += 1;
            return Ok(Value::Object(fields));
        }

        loop {
            if self.peek() != Some(b'"') {
                return self.invalid("expected a quoted key in configuration object");
            }
            let key = self.parse_string()?;
            self.skip_whitespace();
            if self.peek() != Some(b':') {
                return self.invalid("expected ':' after configuration object key");
            }
            self.cursor += 1;
            let value = self.parse_value(depth + 1)?;
            fields.insert(key, value);
            self.skip_whitespace();

            match self.peek() {
                Some(b',') => {
                    self.cursor += 1;
                    self.skip_whitespace();
                }
                Some(b'}') => {
                    self.cursor += 1;
                    break;
                }
                Some(_) => return self.invalid("expected ',' or '}' in configuration object"),
                None => return self.invalid("unterminated configuration object"),
            }
        }
        Ok(Value::Object(fields))
    }

    fn parse_string(&mut self) -> Result<String, ConfigError> {
        if self.peek() != Some(b'"') {
            return self.invalid("expected a JSON string");
        }
        self.cursor += 1;
        let mut output = String::new();
        let mut segment_start = self.cursor;

        while let Some(byte) = self.peek() {
            match byte {
                b'"' => {
                    self.append_utf8_segment(&mut output, segment_start, self.cursor)?;
                    self.cursor += 1;
                    return Ok(output);
                }
                b'\\' => {
                    self.append_utf8_segment(&mut output, segment_start, self.cursor)?;
                    self.cursor += 1;
                    let escaped = self.peek().ok_or_else(|| {
                        ConfigError::InvalidStructure("unterminated JSON escape".to_owned())
                    })?;
                    self.cursor += 1;
                    match escaped {
                        b'"' => output.push('"'),
                        b'\\' => output.push('\\'),
                        b'/' => output.push('/'),
                        b'b' => output.push('\u{0008}'),
                        b'f' => output.push('\u{000C}'),
                        b'n' => output.push('\n'),
                        b'r' => output.push('\r'),
                        b't' => output.push('\t'),
                        b'u' => {
                            let first = self.parse_hex_quad()?;
                            let scalar = match first {
                                0xD800..=0xDBFF => {
                                    if self.input.get(self.cursor..self.cursor + 2) != Some(b"\\u")
                                    {
                                        return self.invalid(
                                            "high surrogate is missing its low surrogate",
                                        );
                                    }
                                    self.cursor += 2;
                                    let second = self.parse_hex_quad()?;
                                    if !(0xDC00..=0xDFFF).contains(&second) {
                                        return self
                                            .invalid("invalid low surrogate in JSON string");
                                    }
                                    0x10000
                                        + (((u32::from(first) - 0xD800) << 10)
                                            | (u32::from(second) - 0xDC00))
                                }
                                0xDC00..=0xDFFF => {
                                    return self.invalid("unpaired low surrogate in JSON string");
                                }
                                value => u32::from(value),
                            };
                            let character = char::from_u32(scalar).ok_or_else(|| {
                                ConfigError::InvalidStructure(
                                    "invalid Unicode scalar in JSON string".to_owned(),
                                )
                            })?;
                            output.push(character);
                        }
                        _ => return self.invalid("invalid escape in configuration JSON string"),
                    }
                    segment_start = self.cursor;
                }
                0x00..=0x1F => {
                    return self.invalid("unescaped control character in JSON string");
                }
                _ => self.cursor += 1,
            }
        }
        self.invalid("unterminated configuration JSON string")
    }

    fn append_utf8_segment(
        &self,
        output: &mut String,
        start: usize,
        end: usize,
    ) -> Result<(), ConfigError> {
        let segment = std::str::from_utf8(&self.input[start..end]).map_err(|_| {
            ConfigError::InvalidStructure("invalid UTF-8 in configuration JSON string".to_owned())
        })?;
        output.push_str(segment);
        Ok(())
    }

    fn parse_hex_quad(&mut self) -> Result<u16, ConfigError> {
        if self.cursor + 4 > self.input.len() {
            return self.invalid("incomplete Unicode escape in configuration JSON string");
        }
        let mut value = 0_u16;
        for _ in 0..4 {
            let digit = match self.input[self.cursor] {
                b'0'..=b'9' => self.input[self.cursor] - b'0',
                b'a'..=b'f' => self.input[self.cursor] - b'a' + 10,
                b'A'..=b'F' => self.input[self.cursor] - b'A' + 10,
                _ => return self.invalid("invalid hexadecimal digit in JSON Unicode escape"),
            };
            value = (value << 4) | u16::from(digit);
            self.cursor += 1;
        }
        Ok(value)
    }

    fn parse_number(&mut self) -> Result<Value, ConfigError> {
        let start = self.cursor;
        while let Some(byte) = self.peek() {
            if matches!(byte, b',' | b']' | b'}' | b' ' | b'\n' | b'\r' | b'\t') {
                break;
            }
            self.cursor += 1;
        }
        let token = std::str::from_utf8(&self.input[start..self.cursor]).map_err(|_| {
            ConfigError::InvalidStructure("invalid UTF-8 in JSON number".to_owned())
        })?;
        let number = token.parse::<serde_json::Number>().map_err(|_| {
            ConfigError::InvalidStructure("invalid JSON number in configuration".to_owned())
        })?;
        Ok(Value::Number(number))
    }

    fn consume_keyword(&mut self, keyword: &[u8]) -> Result<(), ConfigError> {
        let end = self.cursor.saturating_add(keyword.len());
        if self.input.get(self.cursor..end) != Some(keyword) {
            return self.invalid("invalid JSON literal in configuration");
        }
        self.cursor = end;
        Ok(())
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\n' | b'\r' | b'\t')) {
            self.cursor += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.input.get(self.cursor).copied()
    }

    fn invalid<T>(&self, message: &str) -> Result<T, ConfigError> {
        Err(ConfigError::InvalidStructure(message.to_owned()))
    }
}

fn parse_bounded_json(input: &[u8]) -> Result<Value, ConfigError> {
    BoundedJsonParser::new(input).parse()
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

        let mut value = parse_bounded_json(input)?;
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
        let ui_scale_percent = root
            .get("ui_scale_percent")
            .and_then(Value::as_u64)
            .map(|value| {
                value.clamp(
                    u64::from(UI_SCALE_MIN_PERCENT),
                    u64::from(UI_SCALE_MAX_PERCENT),
                ) as u16
            })
            .unwrap_or(DEFAULT_UI_SCALE_PERCENT);
        root.insert("ui_scale_percent".to_owned(), Value::from(ui_scale_percent));
        root.insert("version".to_owned(), Value::from(CONFIG_VERSION));

        Ok(Self::deserialize(value)?)
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
        if !(UI_SCALE_MIN_PERCENT..=UI_SCALE_MAX_PERCENT).contains(&self.ui_scale_percent) {
            issues.push(ValidationIssue {
                path: "ui_scale_percent".to_owned(),
                message: format!(
                    "UI scale must be between {UI_SCALE_MIN_PERCENT}% and {UI_SCALE_MAX_PERCENT}%."
                ),
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
