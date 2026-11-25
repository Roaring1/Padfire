//! Platform-independent Padfire behavior.
//!
//! This crate deliberately has no GUI, audio backend, MIDI backend,
//! filesystem, shell, or operating-system dependencies. It is the stable
//! contract shared by Windows and Linux adapters.

pub mod actions;
pub mod audio;
pub mod config;
pub mod ipc;
pub mod midi;
pub mod runtime;

pub use actions::{ActionEffect, PadActivity, plan_pad_press};
pub use config::{
    AppConfig, CONFIG_VERSION, ConfigError, DEFAULT_UI_SCALE_PERCENT, MAX_CONFIG_BYTES, PAGE_COUNT,
    PadAction, PadAssignment, PadPage, RetriggerMode, UI_SCALE_MAX_PERCENT, UI_SCALE_MIN_PERCENT,
    ValidationIssue,
};
pub use midi::{
    Control, LED_AMBER, LED_DIM, LED_GREEN, LED_OFF, LED_ORANGE, LED_PG_HAS, LED_RED,
    MIDI_PAGE_CC_FIRST, MIDI_PAGE_CC_LAST, MidiEvent, MidiPacket, PadKey, SIDE_COLUMN, decode_midi,
    is_supported_launchpad_port, led_velocity,
};
pub use runtime::{GenerationTracker, OperationKey, OperationKind, OperationToken};
