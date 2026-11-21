//! UI/MIDI/IPC-neutral pad action planning.
//!
//! This module plans effects. It never opens audio/MIDI devices, accesses the
//! filesystem, launches commands or blocks a GUI callback. An app actor applies
//! the returned effects using platform adapters.

use crate::config::{AppConfig, PadAction, PadAssignment, RetriggerMode};
use crate::midi::PadKey;
use crate::runtime::{GenerationTracker, OperationKey, OperationKind, OperationToken};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionEffect {
    StartPlayback {
        token: OperationToken,
        pad: PadKey,
        file: String,
        sink: String,
        volume_percent: u8,
        looping: bool,
        monitor_sink: Option<String>,
    },
    StopPlayback {
        token: OperationToken,
        pad: PadKey,
    },
    StopAllPlayback {
        invalidated: Vec<OperationToken>,
    },
    StartScript {
        token: OperationToken,
        pad: PadKey,
        command: String,
    },
    StopScript {
        token: OperationToken,
        pad: PadKey,
    },
    StopAllScripts {
        invalidated: Vec<OperationToken>,
    },
    StopEverything {
        invalidated_audio: Vec<OperationToken>,
        invalidated_scripts: Vec<OperationToken>,
    },
    ToggleCrosshair {
        pad: PadKey,
    },
    InvalidAssignment {
        pad: PadKey,
        reason: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PadActivity {
    pub audio_playing: bool,
    pub script_running: bool,
}

impl Default for PadActivity {
    fn default() -> Self {
        Self {
            audio_playing: false,
            script_running: false,
        }
    }
}

fn operation_key(kind: OperationKind, pad: PadKey) -> OperationKey {
    OperationKey::new(kind, pad.to_key())
}

fn stop_audio(tracker: &mut GenerationTracker, pad: PadKey) -> ActionEffect {
    ActionEffect::StopPlayback {
        token: tracker.cancel(operation_key(OperationKind::Audio, pad)),
        pad,
    }
}

fn stop_script(tracker: &mut GenerationTracker, pad: PadKey) -> ActionEffect {
    ActionEffect::StopScript {
        token: tracker.cancel(operation_key(OperationKind::Script, pad)),
        pad,
    }
}

/// Convert one pad press into platform-neutral effects.
///
/// A backend must check token freshness immediately before publishing a new
/// process or stream. The caller updates activity only from backend lifecycle
/// events, not merely because a start was requested.
pub fn plan_pad_press(
    pad_key: PadKey,
    pad: &PadAssignment,
    config: &AppConfig,
    activity: PadActivity,
    tracker: &mut GenerationTracker,
) -> Vec<ActionEffect> {
    match pad.action_kind() {
        PadAction::Play => {
            if pad.file.trim().is_empty() {
                return vec![ActionEffect::InvalidAssignment {
                    pad: pad_key,
                    reason: "No sound file is assigned.".to_owned(),
                }];
            }

            let retrigger = pad.effective_retrigger(&config.retrigger);
            if activity.audio_playing && retrigger == RetriggerMode::Toggle {
                return vec![stop_audio(tracker, pad_key)];
            }

            let sink = if pad.sink.trim().is_empty() {
                config.default_sink.clone()
            } else {
                pad.sink.clone()
            };
            let monitor_sink = config
                .monitor_enabled
                .then(|| config.monitor_sink.clone());
            let token = tracker.begin(operation_key(OperationKind::Audio, pad_key));

            vec![ActionEffect::StartPlayback {
                token,
                pad: pad_key,
                file: pad.file.clone(),
                sink,
                volume_percent: pad.effective_volume(),
                looping: pad.loop_enabled,
                monitor_sink,
            }]
        }
        PadAction::Script => {
            if pad.cmd.trim().is_empty() {
                return vec![ActionEffect::InvalidAssignment {
                    pad: pad_key,
                    reason: "No script command is assigned.".to_owned(),
                }];
            }
            let stop = stop_script(tracker, pad_key);
            let token = tracker.begin(operation_key(OperationKind::Script, pad_key));
            vec![
                stop,
                ActionEffect::StartScript {
                    token,
                    pad: pad_key,
                    command: pad.cmd.clone(),
                },
            ]
        }
        PadAction::StopSfx => {
            vec![ActionEffect::StopAllPlayback {
                invalidated: tracker.cancel_kind(OperationKind::Audio),
            }]
        }
        PadAction::StopAll => vec![ActionEffect::StopEverything {
            invalidated_audio: tracker.cancel_kind(OperationKind::Audio),
            invalidated_scripts: tracker.cancel_kind(OperationKind::Script),
        }],
        PadAction::StopKey => {
            let Some(target) = PadKey::parse_legacy(&pad.stop_key) else {
                return vec![ActionEffect::InvalidAssignment {
                    pad: pad_key,
                    reason: "The stop-pad target is missing or invalid.".to_owned(),
                }];
            };
            vec![
                stop_audio(tracker, target),
                stop_script(tracker, target),
            ]
        }
        PadAction::CrosshairToggle => vec![ActionEffect::ToggleCrosshair { pad: pad_key }],
        PadAction::Unsupported(action) => vec![ActionEffect::InvalidAssignment {
            pad: pad_key,
            reason: format!("Unsupported pad action: {action}"),
        }],
    }
}

/// Calculate whether the primary and monitor outputs are actually distinct.
/// Empty means the system default. A platform adapter may resolve aliases
/// before calling this function when it knows the output-device identities.
pub fn monitor_needs_second_stream(primary: &str, monitor: Option<&str>) -> bool {
    match monitor {
        None => false,
        Some(monitor) => monitor != primary,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::GenerationTracker;

    fn config() -> AppConfig {
        AppConfig::from_json(
            br#"{
                "version":5,
                "default_sink":"global-out",
                "retrigger":"restart",
                "monitor_enabled":true,
                "monitor_sink":"monitor-out"
            }"#,
        )
        .unwrap()
    }

    fn pad(action: &str) -> PadAssignment {
        PadAssignment {
            action: action.to_owned(),
            file: "test.wav".to_owned(),
            ..PadAssignment::default()
        }
    }

    fn key() -> PadKey {
        PadKey::new(0, 17)
    }

    #[test]
    fn play_action_uses_per_pad_output_or_global_fallback_and_monitor() {
        let config = config();
        let mut tracker = GenerationTracker::default();
        let assignment = pad("play");
        let effects = plan_pad_press(
            key(),
            &assignment,
            &config,
            PadActivity::default(),
            &mut tracker,
        );

        assert!(matches!(
            &effects[0],
            ActionEffect::StartPlayback {
                sink,
                monitor_sink: Some(monitor),
                volume_percent: 80,
                looping: false,
                ..
            } if sink == "global-out" && monitor == "monitor-out"
        ));
    }

    #[test]
    fn toggle_retrigger_stops_instead_of_starting_another_stream() {
        let config = config();
        let mut tracker = GenerationTracker::default();
        let effects = plan_pad_press(
            key(),
            &pad("play"),
            &config,
            PadActivity {
                audio_playing: true,
                script_running: false,
            },
            &mut tracker,
        );
        assert!(matches!(effects.as_slice(), [ActionEffect::StartPlayback { .. }]));

        let mut config = config;
        config.retrigger = "toggle".to_owned();
        let effects = plan_pad_press(
            key(),
            &pad("play"),
            &config,
            PadActivity {
                audio_playing: true,
                script_running: false,
            },
            &mut tracker,
        );
        assert!(matches!(effects.as_slice(), [ActionEffect::StopPlayback { .. }]));
    }

    #[test]
    fn per_pad_output_and_volume_override_are_preserved() {
        let config = config();
        let assignment = PadAssignment {
            file: "test.wav".to_owned(),
            sink: "pad-out".to_owned(),
            vol: 180,
            loop_enabled: true,
            ..PadAssignment::default()
        };
        let mut tracker = GenerationTracker::default();
        let effects = plan_pad_press(
            key(),
            &assignment,
            &config,
            PadActivity::default(),
            &mut tracker,
        );
        assert!(matches!(
            &effects[0],
            ActionEffect::StartPlayback {
                sink,
                volume_percent: 150,
                looping: true,
                ..
            } if sink == "pad-out"
        ));
    }

    #[test]
    fn stop_sfx_stops_audio_without_stopping_scripts() {
        let config = config();
        let mut tracker = GenerationTracker::default();
        let script = tracker.begin(operation_key(OperationKind::Script, key()));
        let _audio = tracker.begin(operation_key(OperationKind::Audio, key()));
        let effects = plan_pad_press(
            key(),
            &pad("stop_sfx"),
            &config,
            PadActivity::default(),
            &mut tracker,
        );
        assert!(matches!(effects.as_slice(), [ActionEffect::StopAllPlayback { .. }]));
        assert!(tracker.is_current(&script));
    }

    #[test]
    fn stop_all_invalidates_pending_audio_and_script_workers() {
        let config = config();
        let mut tracker = GenerationTracker::default();
        let audio = tracker.begin(operation_key(OperationKind::Audio, key()));
        let script = tracker.begin(operation_key(OperationKind::Script, key()));
        let effects = plan_pad_press(
            key(),
            &pad("stop_all"),
            &config,
            PadActivity::default(),
            &mut tracker,
        );
        assert!(matches!(effects.as_slice(), [ActionEffect::StopEverything { .. }]));
        assert!(!tracker.is_current(&audio));
        assert!(!tracker.is_current(&script));
    }

    #[test]
    fn stop_key_invalidates_both_audio_and_script_targets() {
        let config = config();
        let target = PadKey::new(2, 34);
        let audio = tracker_token(OperationKind::Audio, target);
        let script = tracker_token(OperationKind::Script, target);
        let mut tracker = GenerationTracker::default();
        let old_audio = tracker.begin(audio);
        let old_script = tracker.begin(script);
        let assignment = PadAssignment {
            action: "stop_key".to_owned(),
            stop_key: "2:34  (renamed later)".to_owned(),
            ..PadAssignment::default()
        };
        let effects = plan_pad_press(
            key(),
            &assignment,
            &config,
            PadActivity::default(),
            &mut tracker,
        );
        assert_eq!(effects.len(), 2);
        assert!(!tracker.is_current(&old_audio));
        assert!(!tracker.is_current(&old_script));
    }

    fn tracker_token(kind: OperationKind, pad: PadKey) -> OperationKey {
        operation_key(kind, pad)
    }

    #[test]
    fn empty_play_and_script_assignments_are_rejected() {
        let config = config();
        let mut tracker = GenerationTracker::default();
        let play = PadAssignment::default();
        let script = PadAssignment {
            action: "script".to_owned(),
            ..PadAssignment::default()
        };
        assert!(matches!(
            plan_pad_press(key(), &play, &config, PadActivity::default(), &mut tracker)[0],
            ActionEffect::InvalidAssignment { .. }
        ));
        assert!(matches!(
            plan_pad_press(key(), &script, &config, PadActivity::default(), &mut tracker)[0],
            ActionEffect::InvalidAssignment { .. }
        ));
    }

    #[test]
    fn crosshair_is_a_distinct_effect_not_a_shell_command() {
        let config = config();
        let assignment = pad("crosshair_toggle");
        let mut tracker = GenerationTracker::default();
        let effects = plan_pad_press(
            key(),
            &assignment,
            &config,
            PadActivity::default(),
            &mut tracker,
        );
        assert_eq!(effects, vec![ActionEffect::ToggleCrosshair { pad: key() }]);
    }

    #[test]
    fn monitor_stream_is_omitted_when_it_uses_the_same_sink() {
        assert!(!monitor_needs_second_stream("default", Some("default")));
        assert!(monitor_needs_second_stream("primary", Some("monitor")));
        assert!(!monitor_needs_second_stream("primary", None));
    }
}
