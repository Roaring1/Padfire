use padfire_core::{
    AppConfig, Control, LED_AMBER, LED_DIM, LED_GREEN, LED_OFF, LED_ORANGE, LED_PG_HAS, LED_RED,
    MIDI_PAGE_CC_FIRST, MIDI_PAGE_CC_LAST, MidiEvent, MidiPacket, PadAction, PadAssignment, PadKey,
    RetriggerMode, decode_midi, is_supported_launchpad_port, led_velocity,
};

#[test]
fn default_config_matches_released_v5_contract() {
    let config = AppConfig::default();
    assert_eq!(config.version, 5);
    assert_eq!(config.pages.len(), 8);
    assert_eq!(config.default_sink, "");
    assert!(!config.monitor_enabled);
    assert_eq!(config.monitor_sink, "");
    assert_eq!(config.retrigger, "restart");
    assert_eq!(config.ui_scale_percent, 100);
}

#[test]
fn v1_migration_adds_eight_pages_and_v5_defaults() {
    let input = br#"{"pages":[{"0":{"file":"airhorn.wav","vol":80}}]}"#;
    let config = AppConfig::from_json(input).unwrap();
    assert_eq!(config.version, 5);
    assert_eq!(config.pages.len(), 8);
    assert_eq!(config.pages[0]["0"].action, "play");
    assert_eq!(config.retrigger, "restart");
    assert!(!config.monitor_enabled);
    assert_eq!(config.ui_scale_percent, 100);
}

#[test]
fn ui_scale_is_bounded_and_round_trips() {
    let minimum = AppConfig::from_json(br#"{"ui_scale_percent": 1}"#).unwrap();
    assert_eq!(minimum.ui_scale_percent, 75);

    let maximum = AppConfig::from_json(br#"{"ui_scale_percent": 250}"#).unwrap();
    assert_eq!(maximum.ui_scale_percent, 125);

    let invalid = AppConfig::from_json(br#"{"ui_scale_percent": "large"}"#).unwrap();
    assert_eq!(invalid.ui_scale_percent, 100);

    let saved_config = AppConfig {
        ui_scale_percent: 115,
        ..AppConfig::default()
    };
    let saved = saved_config.to_pretty_json().unwrap();
    let restored = AppConfig::from_json(&saved).unwrap();
    assert_eq!(restored.ui_scale_percent, 115);
}

#[test]
fn migration_drops_only_known_retired_keys_and_preserves_extensions() {
    let input = br#"{
      "version": 3,
      "device": "old-device-field",
      "vendor_extension": {"keep": true},
      "pages": [{
        "17": {
          "file": "airhorn.wav",
          "mode": "retired-mode",
          "future_pad_field": {"keep": 7}
        }
      }]
    }"#;
    let config = AppConfig::from_json(input).unwrap();
    assert!(!config.extra.contains_key("device"));
    assert!(config.extra.contains_key("vendor_extension"));
    assert_eq!(config.pages[0]["17"].action, "play");
    assert!(!config.pages[0]["17"].extra.contains_key("mode"));
    assert_eq!(
        config.pages[0]["17"].extra["future_pad_field"]["keep"],
        serde_json::json!(7)
    );
}

#[test]
fn migration_preserves_valid_global_retrigger_and_resets_invalid_values() {
    let toggle = AppConfig::from_json(br#"{"version":5,"retrigger":"toggle"}"#).unwrap();
    assert_eq!(toggle.retrigger, "toggle");

    for invalid in [r#"{"retrigger":"typo"}"#, r#"{"retrigger":null}"#] {
        let config = AppConfig::from_json(invalid.as_bytes()).unwrap();
        assert_eq!(config.retrigger, "restart");
    }
}

#[test]
fn malformed_shapes_are_normalized_without_crashing() {
    let config = AppConfig::from_json(br#"{"version":5,"pages":[null,7,{"0":false}]}"#).unwrap();
    assert_eq!(config.pages.len(), 8);
    assert!(config.pages[0].is_empty());
    assert!(config.pages[1].is_empty());
    assert_eq!(config.pages[2]["0"].action, "play");
}

#[test]
fn oversized_config_is_rejected_before_json_parsing() {
    let input = vec![b' '; 4 * 1024 * 1024 + 1];
    assert!(AppConfig::from_json(&input).is_err());
}

#[test]
fn bounded_json_parser_matches_serde_for_escapes_unicode_numbers_and_duplicate_keys() {
    let input = r#"{
        "version": 5,
        "vendor_extension": {
            "text": "array [] object {} colon : quote \" slash \\ newline \n control \u0001 emoji \uD83D\uDE80 raw café",
            "numbers": [0, -0, 1, -1, 1.5, 3.141592653589793, 1e300, 1E-9, 9223372036854775807, 18446744073709551615],
            "duplicate": {"same": 1, "same": 2}
        }
    }"#.as_bytes();
    let expected: serde_json::Value = serde_json::from_slice(input).unwrap();
    let config = AppConfig::from_json(input).unwrap();

    assert_eq!(
        config.extra["vendor_extension"],
        expected["vendor_extension"]
    );
}

#[test]
fn bounded_json_parser_rejects_malformed_number_and_trailing_input() {
    for input in [
        br#"{"version":05}"#.as_slice(),
        br#"{"version":1.}"#.as_slice(),
        br#"{"version":1e}"#.as_slice(),
        br#"{"version":5} trailing"#.as_slice(),
        br#"{"version":5,"extension":[1,]}"#.as_slice(),
    ] {
        assert!(AppConfig::from_json(input).is_err(), "accepted {input:?}");
    }
}

#[test]
fn configuration_bounds_reject_excessive_json_depth_and_node_count() {
    let mut nested = "null".to_owned();
    for _ in 0..65 {
        nested = format!("[{nested}]");
    }
    let deep = format!(r#"{{"version":5,"vendor_extension":{nested}}}"#);
    assert!(AppConfig::from_json(deep.as_bytes()).is_err());

    let many_nodes = serde_json::json!({
        "version": 5,
        "vendor_extension": vec![serde_json::Value::Null; 100_001]
    });
    assert!(AppConfig::from_json(&serde_json::to_vec(&many_nodes).unwrap()).is_err());
}

#[test]
fn configuration_bounds_reject_excessive_pages_and_pad_entries() {
    let too_many_pages = serde_json::json!({
        "version": 5,
        "pages": vec![serde_json::json!({}); 257]
    });
    assert!(AppConfig::from_json(&serde_json::to_vec(&too_many_pages).unwrap()).is_err());

    let mut too_many_pads = serde_json::Map::new();
    for note in 0..129 {
        too_many_pads.insert(note.to_string(), serde_json::json!({"action":"stop_sfx"}));
    }
    let too_many_pads = serde_json::json!({
        "version": 5,
        "pages": [serde_json::Value::Object(too_many_pads)]
    });
    assert!(AppConfig::from_json(&serde_json::to_vec(&too_many_pads).unwrap()).is_err());
}

#[test]
fn configuration_bounds_reject_overlong_command_paths_and_nul_bytes() {
    let overlong_file = serde_json::json!({
        "version": 5,
        "pages": [{"0": {"action":"play", "file":"x".repeat(4097)}}]
    });
    assert!(AppConfig::from_json(&serde_json::to_vec(&overlong_file).unwrap()).is_err());

    let overlong_command = serde_json::json!({
        "version": 5,
        "pages": [{"0": {"action":"script", "cmd":"x".repeat(8193)}}]
    });
    assert!(AppConfig::from_json(&serde_json::to_vec(&overlong_command).unwrap()).is_err());

    let nul_path = serde_json::json!({
        "version": 5,
        "pages": [{"0": {"action":"play", "file":"audio.wav\0ignored"}}]
    });
    assert!(AppConfig::from_json(&serde_json::to_vec(&nul_path).unwrap()).is_err());
}

#[test]
fn configuration_bounds_keep_ordinary_extra_pages_volume_and_extension_fields() {
    let mut page = serde_json::Map::new();
    page.insert(
        "17".to_owned(),
        serde_json::json!({
            "action":"play",
            "file":"sound.wav",
            "vol":999,
            "future_pad_field":{"keep":true}
        }),
    );
    let input = serde_json::json!({
        "version":5,
        "vendor_extension":{"keep":true},
        "pages":[{}, {}, {}, {}, {}, {}, {}, {}, serde_json::Value::Object(page)]
    });
    let config = AppConfig::from_json(&serde_json::to_vec(&input).unwrap()).unwrap();
    assert_eq!(config.pages.len(), 9);
    assert_eq!(config.pages[8]["17"].vol, 999);
    assert_eq!(
        config.pages[8]["17"].extra["future_pad_field"]["keep"],
        serde_json::json!(true)
    );
    assert_eq!(
        config.extra["vendor_extension"]["keep"],
        serde_json::json!(true)
    );
}

#[test]
fn volume_is_not_rewritten_but_playback_volume_is_bounded() {
    let config = AppConfig::from_json(
        br#"{"version":5,"pages":[{"0":{"action":"play","file":"x.wav","vol":999}}]}"#,
    )
    .unwrap();
    assert_eq!(config.pages[0]["0"].vol, 999);
    assert_eq!(config.pages[0]["0"].effective_volume(), 150);
}

#[test]
fn all_current_and_unknown_actions_are_classified_safely() {
    let cases = [
        ("play", PadAction::Play),
        ("script", PadAction::Script),
        ("stop_sfx", PadAction::StopSfx),
        ("stop_all", PadAction::StopAll),
        ("stop_key", PadAction::StopKey),
        ("crosshair_toggle", PadAction::CrosshairToggle),
        (
            "future_action",
            PadAction::Unsupported("future_action".to_owned()),
        ),
    ];
    for (name, expected) in cases {
        assert_eq!(PadAction::from_config(name), expected);
    }
}

#[test]
fn per_pad_retrigger_uses_global_only_for_global_override() {
    assert_eq!(
        RetriggerMode::resolve_pad_override(None, "toggle"),
        RetriggerMode::Toggle
    );
    assert_eq!(
        RetriggerMode::resolve_pad_override(Some("global"), "toggle"),
        RetriggerMode::Toggle
    );
    assert_eq!(
        RetriggerMode::resolve_pad_override(Some("restart"), "toggle"),
        RetriggerMode::Restart
    );
    assert_eq!(
        RetriggerMode::resolve_pad_override(Some("toggle"), "restart"),
        RetriggerMode::Toggle
    );
    assert_eq!(
        RetriggerMode::resolve_pad_override(Some("bad-value"), "toggle"),
        RetriggerMode::Restart
    );
}

#[test]
fn pad_control_note_mapping_covers_grid_and_right_side() {
    for row in 0..8 {
        for column in 0..8 {
            let control = Control::Grid { row, column };
            assert_eq!(Control::from_note(control.note()), Some(control));
        }
        let side = Control::Side { row };
        assert_eq!(Control::from_note(side.note()), Some(side));
    }
    assert_eq!(Control::from_note(9), None);
    assert_eq!(Control::from_note(127), None);
}

#[test]
fn midi_note_on_zero_velocity_is_release_and_top_row_selects_pages() {
    assert_eq!(
        decode_midi(MidiPacket::NoteOn {
            note: 17,
            velocity: 90
        }),
        MidiEvent::PadPressed(Control::Grid { row: 1, column: 1 })
    );
    assert_eq!(
        decode_midi(MidiPacket::NoteOn {
            note: 17,
            velocity: 0
        }),
        MidiEvent::PadReleased(Control::Grid { row: 1, column: 1 })
    );
    assert_eq!(
        decode_midi(MidiPacket::NoteOff { note: 8 }),
        MidiEvent::PadReleased(Control::Side { row: 0 })
    );
    assert_eq!(
        decode_midi(MidiPacket::ControlChange {
            controller: MIDI_PAGE_CC_FIRST,
            value: 127
        }),
        MidiEvent::PageSelected(0)
    );
    assert_eq!(
        decode_midi(MidiPacket::ControlChange {
            controller: MIDI_PAGE_CC_LAST,
            value: 127
        }),
        MidiEvent::PageSelected(7)
    );
    assert_eq!(
        decode_midi(MidiPacket::ControlChange {
            controller: MIDI_PAGE_CC_LAST + 1,
            value: 127
        }),
        MidiEvent::Ignored
    );
    assert_eq!(
        decode_midi(MidiPacket::ControlChange {
            controller: MIDI_PAGE_CC_FIRST,
            value: 0
        }),
        MidiEvent::Ignored
    );
}

#[test]
fn mk3_is_never_selected_but_mk1_and_mk2_names_are_accepted() {
    assert!(is_supported_launchpad_port("Launchpad Mini MIDI 1"));
    assert!(is_supported_launchpad_port("Novation Launchpad Mini MK1"));
    assert!(is_supported_launchpad_port(
        "Novation Launchpad Mini mk2 MIDI"
    ));
    assert!(!is_supported_launchpad_port("Launchpad Mini MK3 MIDI 1"));
    assert!(!is_supported_launchpad_port("Launchpad Mini mk3"));
    assert!(!is_supported_launchpad_port("Launchpad Pro"));
}

#[test]
fn every_published_led_constant_retains_double_buffer_flags() {
    for value in [
        LED_OFF, LED_DIM, LED_GREEN, LED_RED, LED_ORANGE, LED_AMBER, LED_PG_HAS,
    ] {
        assert_eq!(value & 0x0c, 0x0c);
    }
}

#[test]
fn led_encoding_masks_each_channel_and_forces_required_flags() {
    for red in 0..=u8::MAX {
        for green in 0..=u8::MAX {
            let value = led_velocity(red, green);
            assert_eq!(value & 0x0c, 0x0c);
            assert_eq!(value & 0x03, red & 0x03);
            assert_eq!((value >> 4) & 0x03, green & 0x03);
        }
    }
}

#[test]
fn legacy_stop_key_with_embedded_display_label_is_accepted() {
    assert_eq!(
        PadKey::parse_legacy("0:17  (Airhorn)"),
        Some(PadKey::new(0, 17))
    );
    assert_eq!(PadKey::parse_legacy("2:120"), Some(PadKey::new(2, 120)));
    assert_eq!(PadKey::parse_legacy("not-a-key"), None);
    assert_eq!(PadKey::parse_legacy("8:17"), None);
}

#[test]
fn config_round_trip_preserves_unknown_root_and_pad_fields() {
    let raw = br#"{
      "version":5,
      "pages":[{"0":{"action":"play","file":"x.wav","future":true}}],
      "new_root_field":"retained"
    }"#;
    let first = AppConfig::from_json(raw).unwrap();
    let saved = first.to_pretty_json().unwrap();
    let second = AppConfig::from_json(&saved).unwrap();
    assert_eq!(first.extra, second.extra);
    assert_eq!(first.pages[0]["0"].extra, second.pages[0]["0"].extra);
}

#[test]
fn pad_assignment_uses_file_stem_for_default_label() {
    let pad = PadAssignment {
        file: "/audio/Long-Airhorn.wav".to_owned(),
        ..PadAssignment::default()
    };
    assert_eq!(pad.effective_label(), "Long-Airhorn");
}
