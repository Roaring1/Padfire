//! Side-by-side per-user installation for the Rust candidate.
//!
//! The installer deliberately uses a distinct "Padfire Rust" identity and
//! never touches the v2.4.0 application or user configuration. It creates a
//! desktop/Start Menu entry only; it never registers autostart or a service.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
#[cfg(windows)]
use std::process::{Command, Stdio};
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

#[derive(Debug)]
struct IntegrationBackup {
    original: PathBuf,
    backup: PathBuf,
}

fn backup_existing_integration(path: &Path) -> Result<Option<IntegrationBackup>, String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!(
                "could not inspect existing integration {}: {error}",
                path.display()
            ));
        }
    };
    if metadata.file_type().is_dir() {
        return Err(format!(
            "refusing to replace an integration directory: {}",
            path.display()
        ));
    }

    let backup = temporary_path(path).with_extension("backup");
    fs::rename(path, &backup).map_err(|error| {
        format!(
            "could not preserve existing integration {}: {error}",
            path.display()
        )
    })?;
    Ok(Some(IntegrationBackup {
        original: path.to_path_buf(),
        backup,
    }))
}

fn restore_integration_backups(backups: &[IntegrationBackup]) -> Result<(), String> {
    let mut failures = Vec::new();
    for backup in backups.iter().rev() {
        if let Err(error) = fs::rename(&backup.backup, &backup.original) {
            failures.push(format!(
                "could not restore {} from {}: {error}",
                backup.original.display(),
                backup.backup.display()
            ));
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("; "))
    }
}

fn rollback_install_integration(
    paths: &InstallPaths,
    backups: &[IntegrationBackup],
) -> Result<(), String> {
    let mut failures = Vec::new();
    for path in std::iter::once(&paths.integration).chain(paths.icon.iter()) {
        if let Err(error) = remove_if_exists(path) {
            failures.push(error);
        }
    }
    if let Err(error) = restore_integration_backups(backups) {
        failures.push(error);
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("; "))
    }
}

fn validate_install_root(root: &Path) -> Result<bool, String> {
    let metadata = match fs::symlink_metadata(root) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(format!(
                "could not inspect install root {}: {error}",
                root.display()
            ));
        }
    };
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(format!(
            "refusing to install through a non-directory or symlink install root: {}",
            root.display()
        ));
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(format!(
                "refusing to install through a reparse-point install root: {}",
                root.display()
            ));
        }
    }
    Ok(true)
}

fn error_with_rollback(original: String, rollback: Result<(), String>) -> String {
    match rollback {
        Ok(()) => original,
        Err(rollback_error) => format!("{original}; rollback failed: {rollback_error}"),
    }
}

fn install_from(source: &Path, paths: &InstallPaths) -> Result<(), String> {
    if !source.is_file() {
        return Err(format!(
            "Padfire executable does not exist: {}",
            source.display()
        ));
    }
    let had_root = validate_install_root(&paths.root)?;
    let mut backups = Vec::with_capacity(2);
    for path in paths.icon.iter().chain(std::iter::once(&paths.integration)) {
        match backup_existing_integration(path) {
            Ok(Some(backup)) => backups.push(backup),
            Ok(None) => {}
            Err(error) => {
                let restore = restore_integration_backups(&backups);
                return match restore {
                    Ok(()) => Err(error),
                    Err(restore_error) => {
                        Err(format!("{error}; rollback also failed: {restore_error}"))
                    }
                };
            }
        }
    }

    // Preserve the old desktop integration until the executable has been
    // published successfully. A failed upgrade restores the previous desktop
    // entry/shortcut and icon byte-for-byte instead of leaving a broken link.
    #[cfg(target_os = "linux")]
    {
        if let Some(icon) = &paths.icon
            && let Err(error) = write_atomic(icon, LINUX_ICON.as_bytes())
        {
            let rollback = rollback_install_integration(paths, &backups);
            return Err(error_with_rollback(error, rollback));
        }
        let desktop = desktop_entry(&paths.executable);
        if let Err(error) = write_atomic(&paths.integration, desktop.as_bytes()) {
            let rollback = rollback_install_integration(paths, &backups);
            return Err(error_with_rollback(error, rollback));
        }
    }

    #[cfg(windows)]
    if let Err(error) = create_windows_shortcut(&paths.integration, &paths.executable, &paths.root)
    {
        let rollback = rollback_install_integration(paths, &backups);
        return Err(error_with_rollback(error, rollback));
    }

    if let Err(error) = copy_executable(source, &paths.executable) {
        let rollback = rollback_install_integration(paths, &backups);
        if !had_root {
            let _ = remove_if_exists(&paths.root);
        }
        return Err(error_with_rollback(error, rollback));
    }

    // Installation is now committed. Old integration backups are no longer
    // needed; cleanup failure is non-fatal because the candidate is usable.
    for backup in backups {
        let _ = remove_if_exists(&backup.backup);
    }

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
        #[cfg(windows)]
        replace_windows_executable(&temporary, destination)?;
        #[cfg(not(windows))]
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

#[cfg(windows)]
#[allow(unsafe_code)]
fn replace_windows_executable(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };

    let source = source
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    // The staged binary has already been sync_all'd. This same-volume move
    // atomically replaces an older candidate and requests write-through. If
    // the old executable is locked without delete sharing, Windows rejects
    // the replacement and the old file remains intact.
    let flags = MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH;
    // SAFETY: both arguments are valid, NUL-terminated UTF-16 paths.
    if unsafe { MoveFileExW(source.as_ptr(), destination.as_ptr(), flags) } == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
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
        .replace('$', "\\$")
        .replace('%', "%%");
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

    let log_path = root.with_extension("uninstall.log");
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let script = format!(
        "$ErrorActionPreference = 'SilentlyContinue'\n\
         $root = '{}'\n\
         $log = '{}'\n\
         [System.IO.File]::WriteAllText($log, 'started' + [Environment]::NewLine)\n\
         for ($attempt = 0; $attempt -lt 30; $attempt++) {{\n\
           try {{ [System.IO.Directory]::Delete($root, $true) }} catch {{ [System.IO.File]::AppendAllText($log, $_.Exception.ToString() + [Environment]::NewLine) }}\n\
           if (-not (Test-Path -LiteralPath $root)) {{ break }}\n\
           Start-Sleep -Milliseconds 500\n\
         }}\n\
         if (Test-Path -LiteralPath $root) {{ [System.IO.File]::AppendAllText($log, 'install root still exists after retries' + [Environment]::NewLine) }} else {{ [System.IO.File]::Delete($log) }}\n\
         Remove-Item -LiteralPath $PSCommandPath -Force -ErrorAction SilentlyContinue\n",
        powershell_quote(root),
        powershell_quote(&log_path),
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
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
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
    fn copy_executable_replaces_an_existing_candidate_without_temp_leaks() {
        let root = temp_root();
        fs::create_dir_all(&root).unwrap();
        let source = root.join("source.bin");
        let destination = root.join("padfire-rust");
        fs::write(&source, b"new executable contents").unwrap();
        fs::write(&destination, b"old executable contents").unwrap();

        copy_executable(&source, &destination).unwrap();

        assert_eq!(fs::read(&destination).unwrap(), b"new executable contents");
        let leftovers = fs::read_dir(&root)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
            .count();
        assert_eq!(
            leftovers, 0,
            "successful executable upgrade left a temp file"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(windows)]
    #[test]
    fn windows_executable_upgrade_preserves_locked_old_binary_then_replaces_it() {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{FILE_SHARE_READ, FILE_SHARE_WRITE};

        let root = temp_root();
        fs::create_dir_all(&root).unwrap();
        let source = root.join("source.bin");
        let destination = root.join("padfire-rust.exe");
        fs::write(&source, b"new executable contents").unwrap();
        fs::write(&destination, b"old executable contents").unwrap();

        // Deliberately deny FILE_SHARE_DELETE so the candidate cannot be
        // atomically replaced until the old image is no longer in use.
        let locked = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .open(&destination)
            .unwrap();
        assert!(copy_executable(&source, &destination).is_err());
        assert_eq!(fs::read(&destination).unwrap(), b"old executable contents");
        drop(locked);

        copy_executable(&source, &destination).unwrap();
        assert_eq!(fs::read(&destination).unwrap(), b"new executable contents");
        let leftovers = fs::read_dir(&root)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
            .count();
        assert_eq!(
            leftovers, 0,
            "failed/retried executable upgrade left temp files"
        );
        let _ = fs::remove_dir_all(root);
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
    fn installer_rejects_symlink_install_roots_without_following_them() {
        use std::os::unix::fs::symlink;

        let root = temp_root();
        let target = root.with_extension("install-target");
        let integration_root = root.with_extension("install-link-integration");
        fs::create_dir_all(&target).unwrap();
        symlink(&target, &root).unwrap();
        let paths = InstallPaths::for_test(&root, &integration_root);
        let source = std::env::current_exe().unwrap();

        let result = install_from(&source, &paths);
        assert!(result.is_err(), "installer followed a symlink install root");
        assert!(!paths.integration.exists());
        assert_eq!(fs::read_dir(&target).unwrap().count(), 0);
        let _ = fs::remove_file(root);
        let _ = fs::remove_dir_all(target);
        let _ = fs::remove_dir_all(integration_root);
    }

    #[test]
    fn failed_executable_copy_rolls_back_new_integration_and_preserves_blocker() {
        let root = temp_root();
        let integration_root = root.with_extension("copy-failure-integration");
        let paths = InstallPaths::for_test(&root, &integration_root);
        let source = std::env::current_exe().unwrap();
        fs::create_dir_all(&paths.executable).unwrap();
        fs::write(
            paths.executable.join("keep.txt"),
            b"pre-existing executable path blocker",
        )
        .unwrap();

        let result = install_from(&source, &paths);
        assert!(
            result.is_err(),
            "copy into a non-directory install root unexpectedly succeeded"
        );
        assert!(
            !paths.integration.exists(),
            "new integration survived a failed executable copy"
        );
        if let Some(icon) = &paths.icon {
            assert!(!icon.exists(), "new icon survived a failed executable copy");
        }
        assert_eq!(
            fs::read(paths.executable.join("keep.txt")).unwrap(),
            b"pre-existing executable path blocker",
            "installer removed or changed the pre-existing path blocker"
        );
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(integration_root);
    }

    #[test]
    fn failed_executable_copy_restores_existing_integration_and_icon() {
        let root = temp_root();
        let integration_root = root.with_extension("preserve-integration");
        let paths = InstallPaths::for_test(&root, &integration_root);
        let source = std::env::current_exe().unwrap();
        let original_integration = b"custom user desktop integration";
        let original_icon = b"custom user icon";
        fs::create_dir_all(paths.integration.parent().unwrap()).unwrap();
        fs::write(&paths.integration, original_integration).unwrap();
        if let Some(icon) = &paths.icon {
            fs::create_dir_all(icon.parent().unwrap()).unwrap();
            fs::write(icon, original_icon).unwrap();
        }
        // A directory at the executable path forces the final rename to fail
        // after the candidate integration has already been created successfully.
        fs::create_dir_all(&paths.executable).unwrap();
        fs::write(
            paths.executable.join("keep.txt"),
            b"pre-existing install-root blocker",
        )
        .unwrap();

        let result = install_from(&source, &paths);
        assert!(
            result.is_err(),
            "copy into a blocked install root unexpectedly succeeded"
        );
        assert_eq!(fs::read(&paths.integration).unwrap(), original_integration);
        if let Some(icon) = &paths.icon {
            assert_eq!(fs::read(icon).unwrap(), original_icon);
        }
        assert_eq!(
            fs::read(paths.executable.join("keep.txt")).unwrap(),
            b"pre-existing install-root blocker"
        );
        let _ = fs::remove_dir_all(&root);
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

    #[cfg(target_os = "linux")]
    #[test]
    fn desktop_exec_escapes_literal_percent_field_codes_and_special_characters() {
        let entry = desktop_entry(Path::new(
            r#"/home/test user/100% $pad/\quoted\name/padfire-rust"#,
        ));
        assert!(entry.contains("100%%"));
        assert!(entry.contains(r"\$pad"));
        assert!(entry.contains(r"\\quoted\\name"));
    }

    #[test]
    fn failed_desktop_integration_does_not_leave_a_partial_install() {
        let root = temp_root();
        let integration_root = root.with_extension("integration");
        let paths = InstallPaths::for_test(&root, &integration_root);
        let source = std::env::current_exe().unwrap();
        fs::write(&integration_root, b"block the integration parent path").unwrap();

        let result = install_from(&source, &paths);
        assert!(result.is_err());
        assert!(
            !paths.executable.exists(),
            "failed desktop integration left an installed executable"
        );
        assert!(
            !paths.root.exists(),
            "failed desktop integration created the candidate install root"
        );
        assert_eq!(
            fs::read(&integration_root).unwrap(),
            b"block the integration parent path"
        );
        let _ = fs::remove_file(integration_root);
    }

    #[cfg(windows)]
    #[test]
    fn installer_rejects_reparse_point_install_roots() {
        let root = temp_root();
        let target = root.with_extension("install-target");
        let integration_root = root.with_extension("install-junction-integration");
        fs::create_dir_all(&target).unwrap();
        let root_text = root.to_string_lossy().into_owned();
        let target_text = target.to_string_lossy().into_owned();
        let output = Command::new("cmd.exe")
            .args(["/C", "mklink", "/J", &root_text, &target_text])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "could not create install-root junction: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );

        let paths = InstallPaths::for_test(&root, &integration_root);
        let source = std::env::current_exe().unwrap();
        let result = install_from(&source, &paths);
        assert!(
            result.is_err(),
            "installer followed a reparse-point install root"
        );
        assert_eq!(fs::read_dir(&target).unwrap().count(), 0);
        assert!(!paths.integration.exists());

        let _ = fs::remove_dir(&root);
        let _ = fs::remove_dir_all(&target);
        let _ = fs::remove_dir_all(integration_root);
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
    fn scheduled_uninstall_retries_locked_files_and_preserves_config() {
        use std::os::windows::fs::OpenOptionsExt;

        let root = temp_root();
        let config = root.with_extension("config");
        let log_path = root.with_extension("uninstall.log");
        let locked_file = root.join("in-use.dat");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("padfire-rust.exe"), b"candidate").unwrap();
        fs::write(&locked_file, b"hold this file open without delete sharing").unwrap();
        fs::create_dir_all(&config).unwrap();
        fs::write(config.join("padfire.json"), b"user config").unwrap();

        let locked_handle = OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&locked_file)
            .unwrap();
        schedule_windows_uninstall(&root).unwrap();

        let retry_deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut observed_locked_retry = false;
        while std::time::Instant::now() < retry_deadline {
            let log_length = fs::metadata(&log_path)
                .map(|metadata| metadata.len())
                .unwrap_or(0);
            if log_length > "started\r\n".len() as u64 {
                observed_locked_retry = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        assert!(
            root.exists(),
            "install root was removed while a file handle denied delete sharing"
        );
        drop(locked_handle);

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while root.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let removed = !root.exists();
        let helper_log = fs::read_to_string(&log_path)
            .unwrap_or_else(|error| format!("no helper log available: {error}"));
        let config_contents = fs::read(config.join("padfire.json")).unwrap();

        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&config);
        let _ = fs::remove_file(&log_path);

        assert!(
            observed_locked_retry,
            "cleanup helper did not report a locked-file retry; helper log: {helper_log}"
        );
        assert!(
            removed,
            "scheduled uninstall did not remove its install root after the file lock closed; helper log: {helper_log}"
        );
        assert_eq!(config_contents, b"user config");
    }
}
