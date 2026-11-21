//! Side-by-side per-user installation for the Rust candidate.
//!
//! The installer deliberately uses a distinct "Padfire Rust" identity and
//! never touches the v2.4.0 application or user configuration. It creates a
//! desktop/Start Menu entry only; it never registers autostart or a service.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
#[cfg(windows)]
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(target_os = "linux")]
const LINUX_ICON: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32">
  <rect x="2" y="2" width="28" height="28" rx="6" fill="#202b38"/>
  <path d="M8 9h5v5h6V9h5v14h-5v-5h-6v5H8z" fill="#79b9d5"/>
  <circle cx="16" cy="16" r="2" fill="#202b38"/>
</svg>
"##;

#[derive(Debug, Clone)]
struct InstallPaths {
    root: PathBuf,
    executable: PathBuf,
    integration: PathBuf,
    icon: Option<PathBuf>,
}

impl InstallPaths {
    fn for_current_user() -> Result<Self, String> {
        let root = if let Some(root) = std::env::var_os("PADFIRE_INSTALL_ROOT") {
            PathBuf::from(root)
        } else {
            default_install_root()?
        };
        let executable = root.join(if cfg!(windows) {
            "padfire-rust.exe"
        } else {
            "padfire-rust"
        });

        #[cfg(target_os = "linux")]
        {
            let data_root = std::env::var_os("XDG_DATA_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    std::env::var_os("HOME")
                        .map(PathBuf::from)
                        .unwrap_or_else(|| PathBuf::from("."))
                        .join(".local/share")
                });
            Ok(Self {
                root,
                executable,
                integration: data_root.join("applications").join("padfire-rust.desktop"),
                icon: Some(
                    data_root
                        .join("icons/hicolor/scalable/apps")
                        .join("padfire-rust.svg"),
                ),
            })
        }

        #[cfg(windows)]
        {
            let app_data = std::env::var_os("APPDATA")
                .map(PathBuf::from)
                .ok_or_else(|| "APPDATA is not set; cannot locate the Start Menu".to_owned())?;
            let start_menu = std::env::var_os("PADFIRE_START_MENU_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| app_data.join("Microsoft/Windows/Start Menu/Programs"));
            Ok(Self {
                root,
                executable,
                integration: start_menu.join("Padfire Rust.lnk"),
                icon: None,
            })
        }
    }

    #[cfg(test)]
    fn for_test(root: &Path, integration_root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            executable: root.join(if cfg!(windows) {
                "padfire-rust.exe"
            } else {
                "padfire-rust"
            }),
            integration: if cfg!(windows) {
                integration_root.join("Padfire Rust.lnk")
            } else {
                integration_root.join("applications/padfire-rust.desktop")
            },
            icon: if cfg!(windows) {
                None
            } else {
                Some(
                    integration_root
                        .join("icons/hicolor/scalable/apps")
                        .join("padfire-rust.svg"),
                )
            },
        }
    }
}

fn default_install_root() -> Result<PathBuf, String> {
    #[cfg(target_os = "linux")]
    {
        let home = std::env::var_os("HOME").map(PathBuf::from).ok_or_else(|| {
            "HOME is not set; cannot locate the per-user install directory".to_owned()
        })?;
        Ok(home.join(".local/opt/padfire-rust"))
    }
    #[cfg(windows)]
    {
        let local = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .ok_or_else(|| {
                "LOCALAPPDATA is not set; cannot locate the per-user install directory".to_owned()
            })?;
        Ok(local.join("Programs/Padfire Rust"))
    }
}

pub fn install_current() -> Result<PathBuf, String> {
    let source = std::env::current_exe()
        .map_err(|error| format!("could not locate the running Padfire executable: {error}"))?;
    let paths = InstallPaths::for_current_user()?;
    install_from(&source, &paths)?;
    Ok(paths.executable)
}

pub fn uninstall_current() -> Result<String, String> {
    let paths = InstallPaths::for_current_user()?;
    #[cfg(windows)]
    {
        let current = std::env::current_exe()
            .unwrap_or_default()
            .canonicalize()
            .unwrap_or_default();
        let root = paths
            .root
            .canonicalize()
            .unwrap_or_else(|_| paths.root.clone());
        if current.starts_with(&root) {
            remove_if_exists(&paths.integration)?;
            schedule_windows_uninstall(&paths.root)?;
            return Ok(
                "Padfire Rust uninstall scheduled; user configuration was preserved".to_owned(),
            );
        }
    }

    uninstall_from(&paths)?;
    Ok("Padfire Rust removed; user configuration was preserved".to_owned())
}

fn install_from(source: &Path, paths: &InstallPaths) -> Result<(), String> {
    if !source.is_file() {
        return Err(format!(
            "Padfire executable does not exist: {}",
            source.display()
        ));
    }
    fs::create_dir_all(&paths.root)
        .map_err(|error| format!("could not create install directory: {error}"))?;
    copy_executable(source, &paths.executable)?;

    #[cfg(target_os = "linux")]
    {
        let desktop = desktop_entry(&paths.executable);
        write_atomic(&paths.integration, desktop.as_bytes())?;
        if let Some(icon) = &paths.icon {
            write_atomic(icon, LINUX_ICON.as_bytes())?;
        }
    }

    #[cfg(windows)]
    create_windows_shortcut(&paths.integration, &paths.executable, &paths.root)?;

    Ok(())
}

fn uninstall_from(paths: &InstallPaths) -> Result<(), String> {
    remove_if_exists(&paths.integration)?;
    if let Some(icon) = &paths.icon {
        remove_if_exists(icon)?;
    }
    remove_if_exists(&paths.root)?;
    Ok(())
}

fn copy_executable(source: &Path, destination: &Path) -> Result<(), String> {
    if same_file(source, destination) {
        return Ok(());
    }
    let parent = destination
        .parent()
        .ok_or_else(|| "install executable path has no parent directory".to_owned())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("could not create executable directory: {error}"))?;

    let temporary = temporary_path(destination);
    let result = (|| -> io::Result<()> {
        let mut input = File::open(source)?;
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        io::copy(&mut input, &mut output)?;
        output.flush()?;
        output.sync_all()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&temporary, fs::Permissions::from_mode(0o755))?;
        }
        fs::rename(&temporary, destination)?;
        sync_directory(parent)?;
        Ok(())
    })();
    if let Err(error) = result {
        let _ = fs::remove_file(&temporary);
        return Err(format!("could not install Padfire executable: {error}"));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn write_atomic(destination: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = destination
        .parent()
        .ok_or_else(|| "integration file path has no parent directory".to_owned())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("could not create integration directory: {error}"))?;
    let temporary = temporary_path(destination);
    let result = (|| -> io::Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(bytes)?;
        file.flush()?;
        file.sync_all()?;
        fs::rename(&temporary, destination)?;
        sync_directory(parent)?;
        Ok(())
    })();
    if let Err(error) = result {
        let _ = fs::remove_file(&temporary);
        return Err(format!("could not write Padfire integration file: {error}"));
    }
    Ok(())
}

fn temporary_path(destination: &Path) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("padfire");
    destination.with_file_name(format!(".{name}.{}.{}.tmp", std::process::id(), nonce))
}

fn same_file(first: &Path, second: &Path) -> bool {
    first
        .canonicalize()
        .ok()
        .zip(second.canonicalize().ok())
        .is_some_and(|(first, second)| first == second)
}

fn remove_if_exists(path: &Path) -> Result<(), String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("could not inspect {}: {error}", path.display())),
    };
    if metadata.file_type().is_dir() {
        fs::remove_dir_all(path)
            .map_err(|error| format!("could not remove {}: {error}", path.display()))
    } else {
        fs::remove_file(path)
            .map_err(|error| format!("could not remove {}: {error}", path.display()))
    }
}

#[cfg(target_os = "linux")]
fn sync_directory(path: &Path) -> io::Result<()> {
    File::open(path)?.sync_all()
}

#[cfg(windows)]
fn sync_directory(_path: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(target_os = "linux")]
fn desktop_entry(executable: &Path) -> String {
    let path = executable.to_string_lossy();
    let escaped = path
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('$', "\\$");
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Padfire Rust\n\
         Comment=Padfire Rust candidate\n\
         Exec=\"{escaped}\"\n\
         Icon=padfire-rust\n\
         Terminal=false\n\
         Categories=AudioVideo;Audio;\n\
         StartupNotify=true\n"
    )
}

#[cfg(windows)]
fn create_windows_shortcut(
    shortcut: &Path,
    executable: &Path,
    working_directory: &Path,
) -> Result<(), String> {
    let script = format!(
        "$ErrorActionPreference = 'Stop'\n\
         $shell = New-Object -ComObject WScript.Shell\n\
         $shortcut = $shell.CreateShortcut('{}')\n\
         $shortcut.TargetPath = '{}'\n\
         $shortcut.WorkingDirectory = '{}'\n\
         $shortcut.Description = 'Padfire Rust'\n\
         $shortcut.Save()\n",
        powershell_quote(shortcut),
        powershell_quote(executable),
        powershell_quote(working_directory),
    );
    fs::create_dir_all(
        shortcut
            .parent()
            .ok_or_else(|| "Start Menu shortcut has no parent directory".to_owned())?,
    )
    .map_err(|error| format!("could not create Start Menu directory: {error}"))?;
    run_powershell_script(&script)
}

#[cfg(windows)]
fn schedule_windows_uninstall(root: &Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt;

    let script = format!(
        "$ErrorActionPreference = 'SilentlyContinue'\n\
         $root = '{}'\n\
         for ($attempt = 0; $attempt -lt 30; $attempt++) {{\n\
           try {{ [System.IO.Directory]::Delete($root, $true) }} catch {{ }}\n\
           if (-not (Test-Path -LiteralPath $root)) {{ break }}\n\
           Start-Sleep -Milliseconds 500\n\
         }}\n\
         Remove-Item -LiteralPath $PSCommandPath -Force -ErrorAction SilentlyContinue\n",
        powershell_quote(root),
    );
    let script_path = write_powershell_temp(&script)?;
    Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(&script_path)
        .creation_flags(0x0000_0008 | 0x0000_0200)
        .spawn()
        .map_err(|error| format!("could not schedule Padfire Rust removal: {error}"))?;
    Ok(())
}

#[cfg(windows)]
fn powershell_quote(path: &Path) -> String {
    path.to_string_lossy().replace('\'', "''")
}

#[cfg(windows)]
fn run_powershell_script(script: &str) -> Result<(), String> {
    let path = write_powershell_temp(script)?;
    let result = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(&path)
        .status()
        .map_err(|error| format!("could not run Start Menu integration script: {error}"));
    let _ = fs::remove_file(&path);
    let status = result?;
    if !status.success() {
        return Err(format!(
            "Start Menu shortcut creation failed (exit code {:?})",
            status.code()
        ));
    }
    Ok(())
}

#[cfg(windows)]
fn write_powershell_temp(script: &str) -> Result<PathBuf, String> {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "padfire-install-{}-{nonce}.ps1",
        std::process::id()
    ));
    let mut bytes = vec![0xFF, 0xFE];
    for unit in script.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    fs::write(&path, bytes)
        .map_err(|error| format!("could not write temporary installer script: {error}"))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir().join(format!("padfire-installer-{}-{nonce}", std::process::id()))
    }

    #[test]
    fn install_is_idempotent_and_uninstall_preserves_user_data() {
        let root = temp_root();
        let integration_root = root.with_extension("integration");
        let paths = InstallPaths::for_test(&root, &integration_root);
        let source = std::env::current_exe().unwrap();
        let user_config = root.with_extension("config");
        fs::create_dir_all(&user_config).unwrap();
        fs::write(user_config.join("padfire.json"), b"user config").unwrap();

        install_from(&source, &paths).unwrap();
        assert!(paths.executable.is_file());
        assert!(paths.integration.is_file());
        if let Some(icon) = &paths.icon {
            assert!(icon.is_file());
        }

        install_from(&source, &paths).unwrap();
        assert!(paths.executable.is_file());

        uninstall_from(&paths).unwrap();
        assert!(!paths.root.exists());
        assert!(!paths.integration.exists());
        if let Some(icon) = &paths.icon {
            assert!(!icon.exists());
        }
        assert_eq!(
            fs::read(user_config.join("padfire.json")).unwrap(),
            b"user config"
        );
        let _ = fs::remove_dir_all(user_config);
        let _ = fs::remove_dir_all(integration_root);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn desktop_exec_quotes_paths_with_spaces() {
        let entry = desktop_entry(Path::new(
            "/home/test user/.local/opt/padfire-rust/padfire-rust",
        ));
        assert!(entry.contains("Exec=\"/home/test user/.local/opt/padfire-rust/padfire-rust\""));
        assert!(!entry.contains("Autostart"));
    }

    #[cfg(windows)]
    #[test]
    fn windows_start_menu_shortcut_is_created_and_removed() {
        let root = temp_root();
        let start_menu = root.with_extension("start-menu");
        let paths = InstallPaths::for_test(&root, &start_menu);
        let source = std::env::current_exe().unwrap();
        install_from(&source, &paths).unwrap();
        assert!(paths.integration.is_file());
        uninstall_from(&paths).unwrap();
        assert!(!paths.integration.exists());
        let _ = fs::remove_dir_all(start_menu);
    }

    #[cfg(windows)]
    #[test]
    fn scheduled_uninstall_removes_install_root_but_preserves_config() {
        let root = temp_root();
        let config = root.with_extension("config");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("padfire-rust.exe"), b"candidate").unwrap();
        fs::create_dir_all(&config).unwrap();
        fs::write(config.join("padfire.json"), b"user config").unwrap();

        schedule_windows_uninstall(&root).unwrap();

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while root.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        assert!(
            !root.exists(),
            "scheduled uninstall did not remove its install root"
        );
        assert_eq!(
            fs::read(config.join("padfire.json")).unwrap(),
            b"user config"
        );
        let _ = fs::remove_dir_all(config);
    }
}
