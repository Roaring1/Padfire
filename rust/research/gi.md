# GTK / gi replacement research

The Python gi dependency is PyGObject and is the GTK3 UI binding. It also
exposes GLib, GDK and optional AppIndicator integration. Replacing gi means
replacing the entire UI/event-loop/tray integration, not merely finding a
Rust crate with a similar import name.

## Decision

Use one Rust native UI codebase for both Linux and Windows. The selected
proof-of-concept is eframe/egui with the native Glow renderer. It avoids
Python, PySide/Qt, an embedded webview and duplicated Windows/Linux front ends.

For tray support, evaluate tray-icon with the Linux KSNI backend on KDE
Plasma. Its Linux AppIndicator backend requires a GTK event loop, while KSNI
runs its own worker thread. Windows still requires the appropriate native
event-loop integration. Tray support must be tested with the actual event
loop before it is enabled.

## Compatibility work still required

- Compact page selector, 8x8 pad grid and right-side column.
- Pad editor, context menu, clear, file drag/drop, pad drag/drop.
- Global output, retrigger and optional monitor settings.
- Window close/Esc hide behavior; page keys; Stop All; Ctrl+Q.
- Tray menu and live page/playing status.
- Native file pickers and window geometry.
- HiDPI and resize behavior on KDE Wayland and Windows 11.
- No unexpected autostart, focus theft or background process.

References:
- https://docs.rs/eframe/latest/eframe/
- https://github.com/emilk/egui
- https://docs.rs/tray-icon/latest/tray_icon/
