//! KDE Wayland window visibility integration.
//!
//! Winit deliberately does not implement `set_visible(false)` or
//! `set_minimized(false)` on Wayland. KWin exposes the missing operations
//! through its supported scripting API, so on KDE Wayland we ask the compositor
//! to minimize/restore this exact process and remove it from taskbar/switcher.
//! X11 and other supported window backends continue to use eframe viewport
//! commands.

use eframe::egui::{Context, ViewportCommand};

#[cfg(target_os = "linux")]
use std::fs::{self, OpenOptions};
#[cfg(target_os = "linux")]
use std::io::Write;
#[cfg(target_os = "linux")]
use std::process::Command;
#[cfg(target_os = "linux")]
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(target_os = "linux")]
static NEXT_SCRIPT_ID: AtomicU64 = AtomicU64::new(1);

pub fn hide_to_tray(context: &Context) {
    #[cfg(target_os = "linux")]
    {
        if is_kde_wayland() {
            if let Err(error) = run_kwin_window_action(true) {
                eprintln!("Padfire could not hide through KWin: {error}");
                context.send_viewport_cmd(ViewportCommand::Minimized(true));
            }
            return;
        }

        if is_wayland() {
            // Other Wayland compositors do not expose a standard request to
            // unmap a toplevel. Minimize is the only portable hide request.
            context.send_viewport_cmd(ViewportCommand::Minimized(true));
        } else {
            context.send_viewport_cmd(ViewportCommand::Visible(false));
        }
    }

    #[cfg(not(target_os = "linux"))]
    context.send_viewport_cmd(ViewportCommand::Visible(false));
}

pub fn show_from_tray(context: &Context) {
    #[cfg(target_os = "linux")]
    {
        if is_kde_wayland() {
            if let Err(error) = run_kwin_window_action(false) {
                eprintln!("Padfire could not restore through KWin: {error}");
                context.send_viewport_cmd(ViewportCommand::Visible(true));
                context.send_viewport_cmd(ViewportCommand::Minimized(false));
                context.send_viewport_cmd(ViewportCommand::Focus);
            }
            context.request_repaint();
            return;
        }
    }

    context.send_viewport_cmd(ViewportCommand::Visible(true));
    context.send_viewport_cmd(ViewportCommand::Minimized(false));
    context.send_viewport_cmd(ViewportCommand::Focus);
    context.request_repaint();
}

#[cfg(target_os = "linux")]
fn is_wayland() -> bool {
    std::env::var("XDG_SESSION_TYPE").is_ok_and(|session| session.eq_ignore_ascii_case("wayland"))
        || std::env::var_os("WAYLAND_DISPLAY").is_some()
}

#[cfg(target_os = "linux")]
fn is_kde_wayland() -> bool {
    if !is_wayland() {
        return false;
    }

    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    desktop
        .split(':')
        .any(|entry| entry.eq_ignore_ascii_case("kde"))
        || std::env::var_os("KDE_FULL_SESSION").is_some()
}

#[cfg(target_os = "linux")]
fn run_kwin_window_action(hidden: bool) -> Result<(), String> {
    let pid = std::process::id();
    let sequence = NEXT_SCRIPT_ID.fetch_add(1, Ordering::Relaxed);
    let plugin_name = format!("padfire-window-control-{pid}-{sequence}");
    let script_path = std::env::temp_dir().join(format!("{plugin_name}.js"));

    let script = render_kwin_script(pid, hidden);
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }

    let mut file = options
        .open(&script_path)
        .map_err(|error| format!("could not create temporary KWin script: {error}"))?;
    let write_result = file
        .write_all(script.as_bytes())
        .map_err(|error| format!("could not write temporary KWin script: {error}"));
    drop(file);
    write_result?;

    let load = Command::new("qdbus")
        .args([
            "org.kde.KWin",
            "/Scripting",
            "org.kde.kwin.Scripting.loadScript",
        ])
        .arg(&script_path)
        .arg(&plugin_name)
        .output()
        .map_err(|error| format!("could not call KWin scripting service (qdbus): {error}"));

    let result = match load {
        Ok(output) if output.status.success() => {
            let script_id = String::from_utf8_lossy(&output.stdout)
                .trim()
                .parse::<i32>()
                .map_err(|error| {
                    format!(
                        "KWin returned an invalid script id: {error}; {}",
                        String::from_utf8_lossy(&output.stderr).trim()
                    )
                })?;

            if script_id < 0 {
                Err(format!(
                    "KWin refused the Padfire window script: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ))
            } else {
                let run = Command::new("qdbus")
                    .args([
                        "org.kde.KWin",
                        &format!("/Scripting/Script{script_id}"),
                        "org.kde.kwin.Script.run",
                    ])
                    .output()
                    .map_err(|error| format!("could not run the KWin window script: {error}"));

                // Always unload the one-shot script; it is not a persistent
                // KWin extension and has no background event handlers.
                let _ = Command::new("qdbus")
                    .args([
                        "org.kde.KWin",
                        "/Scripting",
                        "org.kde.kwin.Scripting.unloadScript",
                        &plugin_name,
                    ])
                    .output();

                run.and_then(|output| {
                    if output.status.success() {
                        Ok(())
                    } else {
                        Err(format!(
                            "KWin window action failed: {}",
                            String::from_utf8_lossy(&output.stderr).trim()
                        ))
                    }
                })
            }
        }
        Ok(output) => Err(format!(
            "could not load the KWin window script: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )),
        Err(error) => Err(error),
    };

    let _ = fs::remove_file(script_path);
    result
}

#[cfg(target_os = "linux")]
fn render_kwin_script(pid: u32, hidden: bool) -> String {
    format!(
        r#"const targetPid = {pid};
const hideWindow = {hidden};

workspace.windowList().forEach((window) => {{
    if (window.pid === targetPid && window.resourceClass === "Padfire") {{
        window.skipTaskbar = hideWindow;
        window.skipPager = hideWindow;
        window.skipSwitcher = hideWindow;
        window.minimized = hideWindow;
        if (!hideWindow) {{
            workspace.activeWindow = window;
        }}
    }}
}});
"#
    )
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn kwin_hide_script_targets_only_this_padfire_process() {
        let script = render_kwin_script(12345, true);
        assert!(script.contains("const targetPid = 12345;"));
        assert!(script.contains("const hideWindow = true;"));
        assert!(script.contains("window.resourceClass === \"Padfire\""));
        assert!(script.contains("window.skipTaskbar = hideWindow;"));
        assert!(script.contains("window.skipPager = hideWindow;"));
        assert!(script.contains("window.skipSwitcher = hideWindow;"));
        assert!(script.contains("window.minimized = hideWindow;"));
    }

    #[test]
    fn kwin_show_script_restores_and_activates_this_padfire_process() {
        let script = render_kwin_script(54321, false);
        assert!(script.contains("const targetPid = 54321;"));
        assert!(script.contains("const hideWindow = false;"));
        assert!(script.contains("if (!hideWindow)"));
        assert!(script.contains("workspace.activeWindow = window;"));
    }
}
