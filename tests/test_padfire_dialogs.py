#!/usr/bin/env python3
"""GTK layout regressions for Padfire's global and per-pad dialogs.

Run under Xvfb. Dialogs are cancelled automatically and use a stub audio
engine, so these tests never open MIDI devices or play/stop real audio.
"""
import os
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
TEST_HOME = Path(tempfile.mkdtemp(prefix="padfire-dialog-test-home-"))
os.environ["HOME"] = str(TEST_HOME)

import gi
gi.require_version("Gtk", "3.0")
gi.require_version("Gdk", "3.0")
from gi.repository import Gtk, Gdk, GLib

import importlib.util

spec = importlib.util.spec_from_file_location("padfire_dialogs_under_test", ROOT / "padfire.py")
assert spec and spec.loader
padfire = importlib.util.module_from_spec(spec)
spec.loader.exec_module(padfire)

Gtk.init([])
screen = Gdk.Screen.get_default()
if screen is None:
    raise SystemExit("GTK dialog tests require a display (use xvfb-run)")
provider = Gtk.CssProvider()
provider.load_from_data(padfire.CSS.encode())
Gtk.StyleContext.add_provider_for_screen(
    screen, provider, Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION
)
Gtk.Settings.get_default().set_property("gtk-application-prefer-dark-theme", True)


class StubAudio:
    def __init__(self):
        self.play_calls = 0
        self.stop_calls = 0

    def play(self, *_args, **_kwargs):
        self.play_calls += 1

    def stop(self, *_args, **_kwargs):
        self.stop_calls += 1


def widget_tree(root):
    found = [root]
    if isinstance(root, Gtk.Container):
        children = []
        root.foreach(children.append)
        for child in children:
            found.extend(widget_tree(child))
    return found


def capture_modal(title, invoke, response=Gtk.ResponseType.CANCEL, configure=None):
    """Run a modal dialog, measure its controls, then respond automatically."""
    captured = {}
    attempts = [0]

    def capture_and_close():
        dialogs = [
            window for window in Gtk.Window.list_toplevels()
            if window.get_title() == title
        ]
        if not dialogs:
            attempts[0] += 1
            return attempts[0] < 40

        dialog = dialogs[0]
        dialog.get_preferred_size()
        captured["size"] = dialog.get_size()
        widgets = widget_tree(dialog)
        captured["switch_widths"] = [
            widget.get_allocated_width()
            for widget in widgets
            if isinstance(widget, Gtk.Switch) and widget.get_visible()
        ]
        captured["scale_widths"] = [
            widget.get_allocated_width()
            for widget in widgets
            if isinstance(widget, Gtk.Scale) and widget.get_visible()
        ]
        captured["file_chooser_buttons"] = sum(
            isinstance(widget, Gtk.FileChooserButton) and widget.get_visible()
            for widget in widgets
        )
        if configure is not None:
            configure(dialog, widgets)
        dialog.response(response)
        return False

    GLib.timeout_add(40, capture_and_close)
    result = invoke()
    if not captured:
        raise AssertionError(f"{title} dialog was not captured")
    return result, captured


class PadfireDialogLayoutTests(unittest.TestCase):
    def setUp(self):
        self.parent = Gtk.Window(title="Padfire dialog test parent")
        self.sinks = [
            "alsa_output.usb-very-long-manufacturer-product-name.stereo-output",
            "vm_game",
            "vm_chat",
            "vm_music",
        ]

    def tearDown(self):
        self.parent.destroy()
        while Gtk.events_pending():
            Gtk.main_iteration_do(False)

    def _edit(self, action):
        audio = StubAudio()
        pad = {
            "action": action,
            "label": "Sample pad",
            "file": "",
            "cmd": "/usr/bin/true",
            "vol": 80,
            "color": "green",
            "notes": "optional metadata",
        }
        result, captured = capture_modal(
            "Edit Pad",
            lambda: padfire.pad_edit_dialog(
                self.parent, pad, self.sinks, "", audio, ["(none)"]
            ),
        )
        self.assertIsNone(result)
        self.assertEqual(audio.play_calls, 0)
        self.assertEqual(audio.stop_calls, 1)
        width, height = captured["size"]
        self.assertLessEqual(width, 390, f"{action} editor width was {width}px")
        self.assertEqual(captured["file_chooser_buttons"], 0)
        return height, captured

    def test_play_editor_is_compact_and_volume_control_is_bounded(self):
        height, captured = self._edit("play")
        self.assertLessEqual(height, 510, f"Play editor height was {height}px")
        self.assertTrue(captured["scale_widths"])
        self.assertLessEqual(max(captured["scale_widths"]), 180)

    def test_audio_path_entry_accepts_a_pasted_file_and_saves_it(self):
        audio_file = TEST_HOME / "typed-audio.wav"
        audio_file.write_bytes(b"test audio placeholder")

        def enter_path(_dialog, widgets):
            entries = [
                widget for widget in widgets
                if isinstance(widget, Gtk.Entry)
                and widget.get_placeholder_text() == "Choose an audio file…"
            ]
            self.assertEqual(len(entries), 1)
            entries[0].set_text(str(audio_file))

        audio = StubAudio()
        pad = {
            "action": "play", "label": "Typed path", "file": "", "vol": 80,
            "color": "default", "notes": "",
        }
        result, _captured = capture_modal(
            "Edit Pad",
            lambda: padfire.pad_edit_dialog(
                self.parent, pad, self.sinks, "", audio, ["(none)"]
            ),
            response=Gtk.ResponseType.OK,
            configure=enter_path,
        )
        self.assertIsNotNone(result)
        self.assertEqual(result["action"], "play")
        self.assertEqual(result["file"], str(audio_file))
        self.assertEqual(audio.play_calls, 0)
        self.assertEqual(audio.stop_calls, 1)

    def test_optional_notes_remain_editable_and_are_saved(self):
        pad = {
            "action": "stop_all", "label": "Stop all", "file": "", "vol": 80,
            "color": "default", "notes": "old notes",
        }

        def edit_notes(_dialog, widgets):
            expander = next(
                widget for widget in widgets
                if isinstance(widget, Gtk.Expander)
                and widget.get_label() == "Notes (optional)"
            )
            self.assertFalse(expander.get_expanded())
            expander.set_expanded(True)
            notes_view = next(
                widget for widget in widget_tree(expander)
                if isinstance(widget, Gtk.TextView)
            )
            notes_view.get_buffer().set_text("updated optional notes")

        result, _captured = capture_modal(
            "Edit Pad",
            lambda: padfire.pad_edit_dialog(
                self.parent, pad, self.sinks, "", StubAudio(), ["(none)"]
            ),
            response=Gtk.ResponseType.OK,
            configure=edit_notes,
        )
        self.assertIsNotNone(result)
        self.assertEqual(result["notes"], "updated optional notes")

    def test_script_editor_uses_its_shorter_visible_form(self):
        height, _captured = self._edit("script")
        self.assertLessEqual(height, 400, f"Script editor height was {height}px")

    def test_stop_action_editor_does_not_inherit_play_layout_height(self):
        height, _captured = self._edit("stop_all")
        self.assertLessEqual(height, 290, f"Stop editor height was {height}px")

    def test_global_settings_switches_keep_their_native_width(self):
        result, captured = capture_modal(
            "Settings",
            lambda: padfire.settings_dialog(
                self.parent, dict(padfire.DEFAULT_CONFIG), self.sinks
            ),
        )
        self.assertIsNone(result)
        width, height = captured["size"]
        self.assertLessEqual(width, 390, f"Settings width was {width}px")
        self.assertLessEqual(height, 390, f"Settings height was {height}px")
        self.assertEqual(len(captured["switch_widths"]), 2)
        self.assertLessEqual(max(captured["switch_widths"]), 80)

    def test_global_settings_save_preserves_sink_and_toggle_choices(self):
        expected_sink = sorted(set(self.sinks), key=str.casefold)[0]

        def select_options(_dialog, widgets):
            combos = [
                widget for widget in widgets
                if isinstance(widget, Gtk.ComboBoxText)
            ]
            switches = [
                widget for widget in widgets
                if isinstance(widget, Gtk.Switch)
            ]
            self.assertEqual(len(combos), 3)
            self.assertEqual(len(switches), 2)
            combos[0].set_active(1)
            combos[1].set_active(1)
            combos[2].set_active(1)
            switches[0].set_active(True)
            switches[1].set_active(True)

        result, _captured = capture_modal(
            "Settings",
            lambda: padfire.settings_dialog(
                self.parent, dict(padfire.DEFAULT_CONFIG), self.sinks
            ),
            response=Gtk.ResponseType.OK,
            configure=select_options,
        )
        self.assertIsNotNone(result)
        self.assertEqual(result["default_sink"], expected_sink)
        self.assertEqual(result["retrigger"], "toggle")
        self.assertTrue(result["stop_on_page_change"])
        self.assertTrue(result["monitor_enabled"])
        self.assertEqual(result["monitor_sink"], expected_sink)


if __name__ == "__main__":
    unittest.main(verbosity=2)
