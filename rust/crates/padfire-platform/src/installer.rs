//! Side-by-side per-user installation for the Rust candidate.
//!
//! The installer deliberately uses a distinct "Padfire Rust" identity and
//! never touches the v2.4.0 application or user configuration. It creates a
//! desktop/Start Menu entry and installs the third-party license notice; it
//! never registers autostart or a service.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
#[cfg(windows)]
use std::process::{Command, Stdio};
#[cfg(windows)]
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(windows)]
static POWERSHELL_TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);
use std::time::{SystemTime, UNIX_EPOCH};

const THIRD_PARTY_LICENSES_HTML: &[u8] = include_bytes!("../../../THIRD_PARTY_LICENSES.html");

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
    license_notice: PathBuf,
}

impl InstallPaths {
    fn for_current_user() -> Result<Self, String> {
        // Keep the install target anchored to the OS-defined per-user
        // application directory. An arbitrary environment override allowed a
        // launched process to redirect installation/removal to any path.
        let root = default_install_root()?;
        let executable = root.join(if cfg!(windows) {
            "padfire-rust.exe"
        } else {
            "padfire-rust"
        });

        #[cfg(target_os = "linux")]
        {
            let data_root = match std::env::var_os("XDG_DATA_HOME")
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
            {
                Some(path) => resolve_base_directory(&path, "XDG_DATA_HOME")?,
                None => {
                    let home = std::env::var_os("HOME")
                        .map(PathBuf::from)
                        .ok_or_else(|| {
                            "HOME is not set; cannot locate desktop integration".to_owned()
                        })
                        .and_then(|home| resolve_base_directory(&home, "HOME"))?;
                    home.join(".local/share")
                }
            };
            let license_notice = data_root
                .join("licenses/padfire-rust")
                .join("THIRD_PARTY_LICENSES.html");
            Ok(Self {
                root,
                executable,
                integration: data_root.join("applications").join("padfire-rust.desktop"),
                icon: Some(
                    data_root
                        .join("icons/hicolor/scalable/apps")
                        .join("padfire-rust.svg"),
                ),
                license_notice,
            })
        }

        #[cfg(windows)]
        {
            let app_data = std::env::var_os("APPDATA")
                .map(PathBuf::from)
                .ok_or_else(|| "APPDATA is not set; cannot locate the Start Menu".to_owned())?;
            let app_data = resolve_base_directory(&app_data, "APPDATA")?;
            let start_menu = std::env::var_os("PADFIRE_START_MENU_DIR")
                .map(PathBuf::from)
                .map(|path| resolve_base_directory(&path, "PADFIRE_START_MENU_DIR"))
                .transpose()?
                .unwrap_or_else(|| app_data.join("Microsoft/Windows/Start Menu/Programs"));
            let license_notice = root.join("THIRD_PARTY_LICENSES.html");
            Ok(Self {
                root,
                executable,
                integration: start_menu.join("Padfire Rust.lnk"),
                icon: None,
                license_notice,
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
            license_notice: if cfg!(windows) {
                root.join("THIRD_PARTY_LICENSES.html")
            } else {
                integration_root
                    .join("licenses/padfire-rust")
                    .join("THIRD_PARTY_LICENSES.html")
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
        let home = resolve_base_directory(&home, "HOME")?;
        Ok(home.join(".local/opt/padfire-rust"))
    }
    #[cfg(windows)]
    {
        let local = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .ok_or_else(|| {
                "LOCALAPPDATA is not set; cannot locate the per-user install directory".to_owned()
            })?;
        let local = resolve_base_directory(&local, "LOCALAPPDATA")?;
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
    let _ = validate_install_root(&paths.root)?;
    if let Some(parent) = paths.integration.parent() {
        validate_directory_chain(parent, "desktop integration parent")?;
    }
    if let Some(icon) = &paths.icon
        && let Some(parent) = icon.parent()
    {
        validate_directory_chain(parent, "icon integration parent")?;
    }
    if let Some(parent) = paths.license_notice.parent() {
        validate_directory_chain(parent, "third-party license notice parent")?;
    }
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
            remove_file_if_exists(&paths.integration)?;
            let log_path = schedule_windows_uninstall(&paths.root)?;
            return Ok(format!(
                "Padfire Rust uninstall scheduled; user configuration was preserved; diagnostics: {}",
                log_path.display()
            ));
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
    for path in std::iter::once(&paths.integration)
        .chain(paths.icon.iter())
        .chain(std::iter::once(&paths.license_notice))
    {
        if let Err(error) = remove_file_if_exists(path) {
            failures.push(error);
        }
    }
    if let Err(error) = restore_integration_backups(backups) {
        failures.push(error);
    }
    if let Some(parent) = paths.license_notice.parent() {
        let _ = remove_empty_directory_if_exists(parent);
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("; "))
    }
}

fn validate_absolute_path(path: &Path, label: &str) -> Result<(), String> {
    if !path.is_absolute() {
        return Err(format!("{label} must be an absolute path"));
    }
    if path
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(format!(
            "{label} must not contain parent-directory components"
        ));
    }
    Ok(())
}

#[cfg(windows)]
fn canonicalize_windows_native(path: &Path, label: &str) -> Result<PathBuf, String> {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};

    // Inspect the caller-supplied path before canonicalization so a junction
    // or symlink cannot disappear from view when the physical target is found.
    validate_directory_chain(path, label)?;
    let canonical = path
        .canonicalize()
        .map_err(|error| format!("could not canonicalize {label}: {error}"))?;
    let units = canonical.as_os_str().encode_wide().collect::<Vec<_>>();
    const VERBATIM_PREFIX: [u16; 4] = [0x005C, 0x005C, 0x003F, 0x005C];
    const VERBATIM_UNC_PREFIX: [u16; 8] = [
        0x005C, 0x005C, 0x003F, 0x005C, 0x0055, 0x004E, 0x0043, 0x005C,
    ];

    let normalized = if units.starts_with(&VERBATIM_UNC_PREFIX) {
        let mut ordinary = Vec::with_capacity(units.len() - 6);
        ordinary.extend_from_slice(&[0x005C, 0x005C]);
        ordinary.extend_from_slice(&units[VERBATIM_UNC_PREFIX.len()..]);
        PathBuf::from(std::ffi::OsString::from_wide(&ordinary))
    } else if units.starts_with(&VERBATIM_PREFIX) {
        PathBuf::from(std::ffi::OsString::from_wide(
            &units[VERBATIM_PREFIX.len()..],
        ))
    } else {
        canonical
    };

    if normalized.as_os_str().encode_wide().count() >= 248 {
        return Err(format!(
            "{label} resolves to a path too long for the Windows Shell integration"
        ));
    }
    Ok(normalized)
}

/// Resolve the nearest existing ancestor and preserve any missing suffix.
/// Unix uses the canonical ancestor path. Windows preserves its native Win32
/// spelling for Shell/PowerShell consumers and rejects reparse-point ancestors.
fn resolve_base_directory(path: &Path, label: &str) -> Result<PathBuf, String> {
    validate_absolute_path(path, label)?;
    #[cfg(windows)]
    validate_directory_chain(path, label)?;

    let mut existing = path.to_path_buf();
    let mut missing = Vec::<std::ffi::OsString>::new();
    loop {
        match existing.canonicalize() {
            Ok(canonical) => {
                if !canonical.is_dir() {
                    return Err(format!(
                        "{label} is not a directory: {}",
                        canonical.display()
                    ));
                }
                existing = canonical;
                break;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let Some(name) = existing.file_name() else {
                    return Err(format!("could not resolve {label}: {error}"));
                };
                missing.push(name.to_os_string());
                if !existing.pop() {
                    return Err(format!("could not resolve {label}: {error}"));
                }
            }
            Err(error) => return Err(format!("could not inspect {label}: {error}")),
        }
    }
    #[cfg(windows)]
    let mut resolved = {
        // Use a physical path but strip the extended-length prefix for COM and
        // PowerShell. The prefix is not consistently accepted by shell APIs.
        canonicalize_windows_native(&existing, label)?
    };
    #[cfg(not(windows))]
    let mut resolved = existing;
    for component in missing.iter().rev() {
        resolved.push(component);
    }
    Ok(resolved)
}

/// Reject existing symlink/reparse-point ancestors and non-directory path
/// components. A missing suffix is safe to create after this preflight.
fn validate_directory_chain(path: &Path, label: &str) -> Result<(), String> {
    validate_absolute_path(path, label)?;
    let mut ancestors = path.ancestors().collect::<Vec<_>>();
    ancestors.reverse();
    for current in ancestors {
        if current.as_os_str().is_empty() {
            continue;
        }
        match fs::symlink_metadata(current) {
            Ok(metadata) => {
                if !metadata.is_dir() || metadata.file_type().is_symlink() {
                    return Err(format!(
                        "refusing a non-directory or symlink in {label}: {}",
                        current.display()
                    ));
                }
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt;
                    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
                    if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                        return Err(format!(
                            "refusing a reparse point in {label}: {}",
                            current.display()
                        ));
                    }
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                return Err(format!(
                    "could not inspect {label} component {}: {error}",
                    current.display()
                ));
            }
        }
    }
    Ok(())
}

fn validate_install_root(root: &Path) -> Result<bool, String> {
    validate_absolute_path(root, "install root")?;
    let parent = root
        .parent()
        .ok_or_else(|| "install root has no parent directory".to_owned())?;
    validate_directory_chain(parent, "install-root parent")?;
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
    for path in std::iter::once(&paths.integration)
        .chain(paths.icon.iter())
        .chain(std::iter::once(&paths.license_notice))
    {
        if let Some(parent) = path.parent() {
            validate_directory_chain(parent, "desktop integration parent")?;
        }
    }
    let mut backups = Vec::with_capacity(3);
    for path in paths
        .icon
        .iter()
        .chain(std::iter::once(&paths.integration))
        .chain(std::iter::once(&paths.license_notice))
    {
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

    if let Err(error) = write_atomic(&paths.license_notice, THIRD_PARTY_LICENSES_HTML) {
        let rollback = rollback_install_integration(paths, &backups);
        if !had_root {
            let _ = remove_empty_directory_if_exists(&paths.root);
        }
        return Err(error_with_rollback(error, rollback));
    }

    if let Err(error) = copy_executable(source, &paths.executable) {
        let rollback = rollback_install_integration(paths, &backups);
        if !had_root {
            // The installer created this directory, but another process may
            // have placed data in it during the failed install. Remove it only
            // when it is still empty.
            let _ = remove_empty_directory_if_exists(&paths.root);
        }
        return Err(error_with_rollback(error, rollback));
    }

    // Installation is now committed. Old integration backups are no longer
    // needed; cleanup failure is non-fatal because the candidate is usable.
    for backup in backups {
        let _ = remove_file_if_exists(&backup.backup);
    }

    Ok(())
}

fn uninstall_from(paths: &InstallPaths) -> Result<(), String> {
    // Refuse symlink/reparse-point install and integration roots before
    // touching any files. Remove only the files Padfire owns; custom user
    // files in the install directory are never recursively deleted.
    let _ = validate_install_root(&paths.root)?;
    if let Some(parent) = paths.integration.parent() {
        validate_directory_chain(parent, "desktop integration parent")?;
    }
    if let Some(icon) = &paths.icon
        && let Some(parent) = icon.parent()
    {
        validate_directory_chain(parent, "icon integration parent")?;
    }
    if let Some(parent) = paths.license_notice.parent() {
        validate_directory_chain(parent, "third-party license notice parent")?;
    }
    remove_file_if_exists(&paths.integration)?;
    if let Some(icon) = &paths.icon {
        remove_file_if_exists(icon)?;
    }
    remove_file_if_exists(&paths.license_notice)?;
    if let Some(parent) = paths.license_notice.parent() {
        let _ = remove_empty_directory_if_exists(parent);
    }
    remove_file_if_exists(&paths.executable)?;
    remove_empty_directory_if_exists(&paths.root)?;
    Ok(())
}

fn copy_executable(source: &Path, destination: &Path) -> Result<(), String> {
    if same_file(source, destination) {
        return Ok(());
    }
    let parent = destination
        .parent()
        .ok_or_else(|| "install executable path has no parent directory".to_owned())?;
    validate_directory_chain(parent, "executable destination parent")?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("could not create executable directory: {error}"))?;
    validate_directory_chain(parent, "executable destination parent")?;

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
        replace_windows_file(&temporary, destination)?;
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
fn replace_windows_file(source: &Path, destination: &Path) -> io::Result<()> {
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
    // The staged file has already been sync_all'd. This same-volume move
    // atomically replaces the previous managed asset and requests write-through.
    // If its destination is locked without delete sharing, the previous file
    // remains intact and the caller can roll back other staged integration.
    let flags = MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH;
    // SAFETY: both arguments are valid, NUL-terminated UTF-16 paths.
    if unsafe { MoveFileExW(source.as_ptr(), destination.as_ptr(), flags) } == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn write_atomic(destination: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = destination
        .parent()
        .ok_or_else(|| "integration file path has no parent directory".to_owned())?;
    validate_directory_chain(parent, "integration destination parent")?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("could not create integration directory: {error}"))?;
    validate_directory_chain(parent, "integration destination parent")?;
    let temporary = temporary_path(destination);
    let result = (|| -> io::Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(bytes)?;
        file.flush()?;
        file.sync_all()?;
        #[cfg(windows)]
        replace_windows_file(&temporary, destination)?;
        #[cfg(not(windows))]
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

fn remove_file_if_exists(path: &Path) -> Result<(), String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("could not inspect {}: {error}", path.display())),
    };
    if metadata.file_type().is_dir() {
        return Err(format!(
            "refusing to remove a directory where a Padfire file was expected: {}",
            path.display()
        ));
    }
    fs::remove_file(path).map_err(|error| format!("could not remove {}: {error}", path.display()))
}

fn remove_empty_directory_if_exists(path: &Path) -> Result<(), String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("could not inspect {}: {error}", path.display())),
    };
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(format!(
            "refusing to remove a non-directory or symlink install root: {}",
            path.display()
        ));
    }
    fs::remove_dir(path).map_err(|error| {
        format!(
            "could not remove empty Padfire install directory {}; remaining files were preserved: {error}",
            path.display()
        )
    })
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
    let parent = shortcut
        .parent()
        .ok_or_else(|| "Start Menu shortcut has no parent directory".to_owned())?;
    validate_directory_chain(parent, "Start Menu shortcut parent")?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("could not create Start Menu directory: {error}"))?;
    validate_directory_chain(parent, "Start Menu shortcut parent")?;
    run_powershell_script(&script)
}

#[cfg(windows)]
fn reserve_windows_uninstall_log() -> Result<PathBuf, String> {
    let temp_root =
        canonicalize_windows_native(&std::env::temp_dir(), "uninstall diagnostic directory")?;
    validate_directory_chain(&temp_root, "uninstall diagnostic directory")?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();

    for attempt in 0..32_u8 {
        let candidate = temp_root.join(format!(
            "padfire-uninstall-{}-{nonce}-{attempt}.log",
            std::process::id()
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => {
                file.sync_all().map_err(|error| {
                    let _ = fs::remove_file(&candidate);
                    format!("could not reserve uninstall diagnostic file: {error}")
                })?;
                return Ok(candidate);
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(format!(
                    "could not create uninstall diagnostic file {}: {error}",
                    candidate.display()
                ));
            }
        }
    }
    Err("could not reserve a unique uninstall diagnostic file".to_owned())
}

#[cfg(windows)]
fn schedule_windows_uninstall(root: &Path) -> Result<PathBuf, String> {
    use std::os::windows::process::CommandExt;

    let log_path = reserve_windows_uninstall_log()?;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let executable = root.join("padfire-rust.exe");
    let license_notice = root.join("THIRD_PARTY_LICENSES.html");
    let script = format!(
        "$ErrorActionPreference = 'SilentlyContinue'\n\
         $root = '{}'\n\
         $exe = '{}'\n\
         $license = '{}'\n\
         $log = '{}'\n\
         [System.IO.File]::WriteAllText($log, 'started' + [Environment]::NewLine)\n\
         for ($attempt = 0; $attempt -lt 30; $attempt++) {{\n\
           try {{ [System.IO.File]::Delete($exe) }} catch {{ [System.IO.File]::AppendAllText($log, $_.Exception.ToString() + [Environment]::NewLine) }}\n\
           try {{ [System.IO.File]::Delete($license) }} catch {{ [System.IO.File]::AppendAllText($log, $_.Exception.ToString() + [Environment]::NewLine) }}\n\
           if (-not (Test-Path -LiteralPath $exe) -and -not (Test-Path -LiteralPath $license)) {{\n\
             try {{ [System.IO.Directory]::Delete($root, $false) }} catch {{ [System.IO.File]::AppendAllText($log, ('preserved non-empty install root: ' + $_.Exception.ToString()) + [Environment]::NewLine) }}\n\
           }}\n\
           if (-not (Test-Path -LiteralPath $root)) {{ break }}\n\
           Start-Sleep -Milliseconds 500\n\
         }}\n\
         if (Test-Path -LiteralPath $root) {{ [System.IO.File]::AppendAllText($log, 'install root still exists after retries; unrelated files were preserved' + [Environment]::NewLine) }} else {{ [System.IO.File]::Delete($log) }}\n\
         Remove-Item -LiteralPath $PSCommandPath -Force -ErrorAction SilentlyContinue\n",
        powershell_quote(root),
        powershell_quote(&executable),
        powershell_quote(&license_notice),
        powershell_quote(&log_path),
    );
    let script_path = match write_powershell_temp(&script) {
        Ok(path) => path,
        Err(error) => {
            let _ = fs::remove_file(&log_path);
            return Err(error);
        }
    };
    match Command::new("powershell.exe")
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
    {
        Ok(_) => Ok(log_path),
        Err(error) => {
            let _ = fs::remove_file(&script_path);
            let _ = fs::remove_file(&log_path);
            Err(format!("could not schedule Padfire Rust removal: {error}"))
        }
    }
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
fn write_exclusive_temp_file(path: &Path, bytes: &[u8], description: &str) -> Result<(), String> {
    // A failed create_new means this invocation does not own the path. In
    // particular, never remove an existing file after an AlreadyExists error.
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| format!("could not create {description}: {error}"))?;
    let write_result = (|| -> io::Result<()> {
        file.write_all(bytes)?;
        file.flush()?;
        file.sync_all()?;
        Ok(())
    })();
    if let Err(error) = write_result {
        drop(file);
        let _ = fs::remove_file(path);
        return Err(format!("could not write {description}: {error}"));
    }
    Ok(())
}

#[cfg(windows)]
fn write_powershell_temp(script: &str) -> Result<PathBuf, String> {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    // Windows clock resolution can be coarser than nanoseconds. A sequence
    // number guarantees unique names across parallel installer/test threads;
    // create_new remains the final collision/race guard.
    let sequence = POWERSHELL_TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    // Preserve the native Windows temp path spelling. canonicalize() can
    // produce a \\?\ path that PowerShell's -File argument and shell-facing
    // integrations do not consistently accept.
    let temporary_root = canonicalize_windows_native(
        &std::env::temp_dir(),
        "temporary installer-script directory",
    )?;
    validate_directory_chain(&temporary_root, "temporary installer-script directory")?;
    let path = temporary_root.join(format!(
        "padfire-install-{}-{nonce}-{sequence}.ps1",
        std::process::id()
    ));
    let mut bytes = vec![0xFF, 0xFE];
    for unit in script.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    write_exclusive_temp_file(&path, &bytes, "temporary installer script")?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root() -> (PathBuf, tempfile::TempDir) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("install-root");
        (path, directory)
    }

    #[cfg(windows)]
    #[test]
    fn exclusive_temp_file_collision_preserves_the_existing_owner_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("owned-by-another-operation.ps1");
        fs::write(&path, b"existing installer script").unwrap();

        let error = write_exclusive_temp_file(&path, b"replacement", "test script")
            .expect_err("exclusive create must reject an existing file");

        assert!(error.contains("could not create test script"));
        assert_eq!(
            fs::read(&path).unwrap(),
            b"existing installer script",
            "a failed create_new must never remove or truncate another operation's file"
        );
    }

    #[test]
    fn installer_path_helpers_reject_relative_and_parent_traversal_paths() {
        let base = tempfile::tempdir().unwrap();
        let traversal = base.path().join("..").join("outside");
        let valid = base.path().join("padfire-install");

        assert!(validate_absolute_path(Path::new("relative/install"), "test").is_err());
        assert!(validate_absolute_path(&traversal, "test").is_err());
        assert!(validate_absolute_path(&valid, "test").is_ok());
    }

    #[cfg(windows)]
    #[test]
    fn installer_base_directory_returns_physical_ordinary_win32_path() {
        let base = tempfile::tempdir().unwrap();
        let resolved = resolve_base_directory(base.path(), "TEMP").unwrap();
        assert!(resolved.is_absolute());
        assert!(resolved.is_dir());
        assert!(
            !resolved.to_string_lossy().starts_with("\\\\?\\"),
            "Shell integration path must not use the extended-length prefix"
        );
    }

    #[cfg(windows)]
    #[test]
    fn powershell_temp_file_uses_native_temp_path_and_executes_with_file_argument() {
        use std::process::Command;

        let first = write_powershell_temp("Write-Output 'PadfireTempTest'; exit 0").unwrap();
        let second = write_powershell_temp("Write-Output 'PadfireTempTest'; exit 0").unwrap();
        let native_temp = canonicalize_windows_native(
            &std::env::temp_dir(),
            "temporary installer-script test directory",
        )
        .unwrap();

        assert_ne!(first, second, "parallel scripts must not share a path");
        for path in [&first, &second] {
            assert!(path.starts_with(&native_temp));
            assert!(
                !path.to_string_lossy().starts_with(r"\\?\"),
                "PowerShell script path must use ordinary Win32 spelling"
            );
            assert!(path.is_file());
            let bytes = fs::read(path).unwrap();
            assert!(bytes.starts_with(&[0xFF, 0xFE]));

            let output = Command::new("powershell.exe")
                .args([
                    "-NoProfile",
                    "-NonInteractive",
                    "-ExecutionPolicy",
                    "Bypass",
                    "-File",
                ])
                .arg(path)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "PowerShell could not execute {}: {}",
                path.display(),
                String::from_utf8_lossy(&output.stderr)
            );
        }

        fs::remove_file(first).unwrap();
        fs::remove_file(second).unwrap();

        // Exercise the production helper as well: it must keep the script
        // available until PowerShell has finished reading and executing it.
        run_powershell_script("exit 0").unwrap();
    }

    #[test]
    fn installer_base_directory_resolves_existing_and_missing_suffixes() {
        let (root, _temp) = temp_root();
        let target = root.join("actual-data");
        fs::create_dir_all(&target).unwrap();

        let resolved = resolve_base_directory(&target, "test").unwrap();
        #[cfg(windows)]
        {
            assert!(resolved.is_absolute());
            assert!(resolved.is_dir());
            assert!(
                !resolved.to_string_lossy().starts_with("\\\\?\\"),
                "resolved Shell path must use ordinary Win32 spelling"
            );
        }
        #[cfg(not(windows))]
        assert_eq!(resolved, target.canonicalize().unwrap());

        let missing = resolve_base_directory(&target.join("new/nested"), "test").unwrap();
        assert_eq!(missing, resolved.join("new/nested"));
        assert!(!missing.exists());

        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn installer_directory_chain_rejects_symlink_ancestors() {
        use std::os::unix::fs::symlink;

        let (root, _temp) = temp_root();
        let target = root.join("target");
        let parent = root.join("parent");
        let link = parent.join("linked");
        fs::create_dir_all(&target).unwrap();
        fs::create_dir_all(&parent).unwrap();
        symlink(&target, &link).unwrap();

        assert!(validate_directory_chain(&link.join("nested"), "test").is_err());
        assert!(validate_directory_chain(&parent.join("missing/nested"), "test").is_ok());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn copy_executable_replaces_an_existing_candidate_without_temp_leaks() {
        let (root, _temp) = temp_root();
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

        let (root, _temp) = temp_root();
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
        let (root, _temp) = temp_root();
        let integration_root = root.with_extension("integration");
        let paths = InstallPaths::for_test(&root, &integration_root);
        let source = std::env::current_exe().unwrap();
        let user_config = root.with_extension("config");
        fs::create_dir_all(&user_config).unwrap();
        fs::write(user_config.join("padfire.json"), b"user config").unwrap();

        install_from(&source, &paths).unwrap();
        assert!(paths.executable.is_file());
        assert!(paths.integration.is_file());
        assert!(paths.license_notice.is_file());
        assert_eq!(
            fs::read(&paths.license_notice).unwrap(),
            THIRD_PARTY_LICENSES_HTML
        );
        if let Some(icon) = &paths.icon {
            assert!(icon.is_file());
        }

        install_from(&source, &paths).unwrap();
        assert!(paths.executable.is_file());
        assert_eq!(
            fs::read(&paths.license_notice).unwrap(),
            THIRD_PARTY_LICENSES_HTML
        );

        uninstall_from(&paths).unwrap();
        assert!(!paths.root.exists());
        assert!(!paths.integration.exists());
        assert!(!paths.license_notice.exists());
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

    #[test]
    fn failed_executable_publication_restores_previous_license_notice() {
        let (root, _temp) = temp_root();
        let integration_root = root.with_extension("license-rollback-integration");
        let paths = InstallPaths::for_test(&root, &integration_root);
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(paths.license_notice.parent().unwrap()).unwrap();
        fs::write(&paths.license_notice, b"previous third-party notice").unwrap();
        fs::create_dir_all(&paths.executable).unwrap();
        let source = std::env::current_exe().unwrap();

        assert!(install_from(&source, &paths).is_err());
        assert_eq!(
            fs::read(&paths.license_notice).unwrap(),
            b"previous third-party notice"
        );
        assert!(!paths.integration.exists());
        if let Some(icon) = &paths.icon {
            assert!(!icon.exists());
        }

        let _ = fs::remove_dir_all(&paths.root);
        let _ = fs::remove_dir_all(integration_root);
    }

    #[test]
    fn uninstall_removes_only_padfire_files_and_preserves_unrelated_install_data() {
        let (root, _temp) = temp_root();
        let integration_root = root.with_extension("uninstall-safe-integration");
        let paths = InstallPaths::for_test(&root, &integration_root);
        fs::create_dir_all(&paths.root).unwrap();
        fs::write(&paths.executable, b"installed executable").unwrap();
        fs::create_dir_all(paths.license_notice.parent().unwrap()).unwrap();
        fs::write(&paths.license_notice, b"Padfire third-party licenses").unwrap();
        fs::write(paths.root.join("user-notes.txt"), b"keep this file").unwrap();
        fs::create_dir_all(paths.integration.parent().unwrap()).unwrap();
        fs::write(&paths.integration, b"Padfire desktop entry").unwrap();
        if let Some(icon) = &paths.icon {
            fs::create_dir_all(icon.parent().unwrap()).unwrap();
            fs::write(icon, b"Padfire icon").unwrap();
        }

        let result = uninstall_from(&paths);

        assert!(
            result.is_err(),
            "non-empty install root should be preserved and reported"
        );
        assert!(!paths.executable.exists());
        assert!(!paths.integration.exists());
        assert!(!paths.license_notice.exists());
        if let Some(icon) = &paths.icon {
            assert!(!icon.exists());
        }
        assert_eq!(
            fs::read(paths.root.join("user-notes.txt")).unwrap(),
            b"keep this file"
        );
        assert!(paths.root.is_dir());

        let _ = fs::remove_dir_all(&paths.root);
        let _ = fs::remove_dir_all(integration_root);
    }

    #[test]
    fn uninstall_refuses_to_recursively_remove_a_directory_at_the_integration_path() {
        let (root, _temp) = temp_root();
        let integration_root = root.with_extension("uninstall-directory-integration");
        let paths = InstallPaths::for_test(&root, &integration_root);
        fs::create_dir_all(&paths.root).unwrap();
        fs::write(&paths.executable, b"installed executable").unwrap();
        fs::create_dir_all(&paths.integration).unwrap();
        fs::write(paths.integration.join("user-file.txt"), b"preserve").unwrap();

        assert!(uninstall_from(&paths).is_err());
        assert_eq!(
            fs::read(paths.integration.join("user-file.txt")).unwrap(),
            b"preserve"
        );
        assert!(paths.executable.exists());

        let _ = fs::remove_dir_all(&paths.root);
        let _ = fs::remove_dir_all(integration_root);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn installer_rejects_symlink_install_roots_without_following_them() {
        use std::os::unix::fs::symlink;

        let (root, _temp) = temp_root();
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
        let (root, _temp) = temp_root();
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
        let (root, _temp) = temp_root();
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
        let (root, _temp) = temp_root();
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
        let (root, _temp) = temp_root();
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
        let (root, _temp) = temp_root();
        let start_menu = root.with_extension("start-menu");
        let paths = InstallPaths::for_test(&root, &start_menu);
        let source = std::env::current_exe().unwrap();
        install_from(&source, &paths).unwrap();
        assert!(paths.integration.is_file());
        assert!(paths.license_notice.is_file());
        uninstall_from(&paths).unwrap();
        assert!(!paths.integration.exists());
        assert!(!paths.license_notice.exists());
        let _ = fs::remove_dir_all(start_menu);
    }

    #[cfg(windows)]
    #[test]
    fn scheduled_uninstall_retries_locked_files_and_preserves_config() {
        use std::os::windows::fs::OpenOptionsExt;

        let (root, _temp) = temp_root();
        let config = root.with_extension("config");
        let previous_sibling_log = root.with_extension("uninstall.log");
        fs::write(&previous_sibling_log, b"preserve this unrelated file").unwrap();
        // The delayed helper retries removal of the actual installed image,
        // not arbitrary user files in the install directory.
        let locked_file = root.join("padfire-rust.exe");
        fs::create_dir_all(&root).unwrap();
        fs::write(
            &locked_file,
            b"hold the executable open without delete sharing",
        )
        .unwrap();
        fs::write(
            root.join("THIRD_PARTY_LICENSES.html"),
            b"third-party licenses",
        )
        .unwrap();
        fs::create_dir_all(&config).unwrap();
        fs::write(config.join("padfire.json"), b"user config").unwrap();

        let locked_handle = OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&locked_file)
            .unwrap();
        let log_path = schedule_windows_uninstall(&root).unwrap();
        assert_ne!(log_path, previous_sibling_log);
        assert!(log_path.parent().is_some_and(Path::is_dir));

        let retry_deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
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
        let sibling_log_contents = fs::read(&previous_sibling_log).unwrap();

        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&config);
        let _ = fs::remove_file(&log_path);
        let _ = fs::remove_file(&previous_sibling_log);

        assert!(
            observed_locked_retry,
            "cleanup helper did not report a locked-file retry; helper log: {helper_log}"
        );
        assert!(
            removed,
            "scheduled uninstall did not remove its install root after the file lock closed; helper log: {helper_log}"
        );
        assert_eq!(config_contents, b"user config");
        assert_eq!(sibling_log_contents, b"preserve this unrelated file");
        assert!(!root.join("THIRD_PARTY_LICENSES.html").exists());
    }
}
