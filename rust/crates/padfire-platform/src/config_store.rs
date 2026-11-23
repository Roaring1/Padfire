//! Secure v5 JSON configuration storage.
//!
//! Unix: private 0700 directory, owner-only regular files, no symlink/hardlink
//! reads, bounded input, same-directory atomic replacement and directory fsync.
//! Windows: per-user AppData directory with a protected owner/SYSTEM/Administrators
//! DACL, handle-based reparse-point and hardlink rejection, and same-directory replacement.

use padfire_core::{AppConfig, ConfigError, MAX_CONFIG_BYTES};
use std::fmt;
#[cfg(unix)]
use std::fs::File;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug)]
pub enum StoreError {
    Io(std::io::Error),
    Config(ConfigError),
    Json(serde_json::Error),
    UnsafePath(String),
    TooLarge { actual: usize, maximum: usize },
    TempNameExhausted,
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "configuration I/O failed: {error}"),
            Self::Config(error) => write!(f, "configuration is invalid: {error}"),
            Self::Json(error) => write!(f, "configuration serialization failed: {error}"),
            Self::UnsafePath(message) => f.write_str(message),
            Self::TooLarge { actual, maximum } => {
                write!(f, "configuration is {actual} bytes; maximum is {maximum}")
            }
            Self::TempNameExhausted => {
                f.write_str("could not allocate a unique same-directory temporary file")
            }
        }
    }
}

impl std::error::Error for StoreError {}
impl From<std::io::Error> for StoreError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}
impl From<ConfigError> for StoreError {
    fn from(value: ConfigError) -> Self {
        Self::Config(value)
    }
}
impl From<serde_json::Error> for StoreError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadState {
    Loaded,
    ImportedLegacy,
    CreatedDefault,
    DefaultAfterError,
}

#[derive(Debug)]
pub struct ConfigLoad {
    pub config: AppConfig,
    pub state: LoadState,
    pub warning: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ConfigStore {
    directory: PathBuf,
    config_path: PathBuf,
    backup_path: PathBuf,
    legacy_path: Option<PathBuf>,
}

impl ConfigStore {
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        let directory = directory.into();
        Self {
            config_path: directory.join("padfire.json"),
            backup_path: directory.join("padfire.json.bak"),
            directory,
            legacy_path: None,
        }
    }

    /// Resolve the current user's platform configuration directory.
    pub fn for_current_user() -> Result<Self, StoreError> {
        #[cfg(target_os = "linux")]
        {
            let home = std::env::var_os("HOME")
                .map(PathBuf::from)
                .ok_or_else(|| StoreError::UnsafePath("HOME is not set".to_owned()))?;
            let home = resolve_directory_path(&home, "HOME")?;
            let base = std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
                .unwrap_or_else(|| home.join(".config"));
            let base = resolve_directory_path(&base, "XDG_CONFIG_HOME")?;
            let mut store = Self::new(base.join("padfire"));
            store.legacy_path = Some(home.join(".config/roaring/padfire.json"));
            Ok(store)
        }
        #[cfg(windows)]
        {
            let base = std::env::var_os("APPDATA")
                .map(PathBuf::from)
                .or_else(|| std::env::var_os("LOCALAPPDATA").map(PathBuf::from))
                .ok_or_else(|| {
                    StoreError::UnsafePath(
                        "APPDATA and LOCALAPPDATA are both unavailable".to_owned(),
                    )
                })?;
            let base = resolve_directory_path(&base, "APPDATA/LOCALAPPDATA")?;
            Ok(Self::new(base.join("Padfire")))
        }
        #[cfg(not(any(target_os = "linux", windows)))]
        {
            Err(StoreError::UnsafePath(
                "Padfire config storage is not implemented for this operating system".to_owned(),
            ))
        }
    }

    /// Development-only namespace for side-by-side testing while v2.4.0
    /// remains installed. It reads/imports the released config once but all
    /// subsequent saves and IPC use separate filenames and cannot race the
    /// production process.
    pub fn for_development() -> Result<Self, StoreError> {
        let production = Self::for_current_user()?;
        let production_directory = production.directory.clone();
        let directory = production_directory.with_file_name("padfire-rust");
        Ok(Self {
            config_path: directory.join("padfire.json"),
            backup_path: directory.join("padfire.json.bak"),
            directory,
            legacy_path: Some(production_directory.join("padfire.json")),
        })
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }
    pub fn config_path(&self) -> &Path {
        &self.config_path
    }
    pub fn backup_path(&self) -> &Path {
        &self.backup_path
    }

    /// Ensure the final directory is real and controlled by the current user.
    pub fn ensure_private_directory(&self) -> Result<(), StoreError> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::{MetadataExt, PermissionsExt};
            fs::create_dir_all(&self.directory)?;
            let metadata = fs::symlink_metadata(&self.directory)?;
            if !metadata.is_dir() || metadata.file_type().is_symlink() {
                return Err(StoreError::UnsafePath(format!(
                    "Padfire configuration directory is not a real directory: {}",
                    self.directory.display()
                )));
            }
            if metadata.uid() != nix::unistd::Uid::current().as_raw() {
                return Err(StoreError::UnsafePath(format!(
                    "Padfire configuration directory is owned by another user: {}",
                    self.directory.display()
                )));
            }
            fs::set_permissions(&self.directory, fs::Permissions::from_mode(0o700))?;
        }

        #[cfg(windows)]
        ensure_windows_private_directory(&self.directory)?;

        Ok(())
    }

    /// Load active v5 config, importing the former Roaring path once if needed.
    pub fn load(&self) -> ConfigLoad {
        if let Err(error) = self.ensure_private_directory() {
            return ConfigLoad {
                config: AppConfig::default(),
                state: LoadState::DefaultAfterError,
                warning: Some(error.to_string()),
            };
        }

        // Tighten legacy backup permissions on Unix, but never follow a link.
        if let Err(error) = read_private_file(&self.backup_path, MAX_CONFIG_BYTES)
            && !is_not_found(&error)
        {
            eprintln!("[config] existing backup was not accepted safely: {error}");
        }

        match read_private_file(&self.config_path, MAX_CONFIG_BYTES) {
            Ok(bytes) => match AppConfig::from_json(&bytes) {
                Ok(config) => ConfigLoad {
                    config,
                    state: LoadState::Loaded,
                    warning: None,
                },
                Err(error) => ConfigLoad {
                    config: AppConfig::default(),
                    state: LoadState::DefaultAfterError,
                    warning: Some(error.to_string()),
                },
            },
            Err(error) if is_not_found(&error) => {
                if let Some(legacy_path) = &self.legacy_path {
                    match read_private_file(legacy_path, MAX_CONFIG_BYTES) {
                        Ok(bytes) => {
                            let import = write_atomic(&self.config_path, &bytes).and_then(|()| {
                                AppConfig::from_json(&bytes).map_err(StoreError::from)
                            });
                            return match import {
                                Ok(config) => ConfigLoad {
                                    config,
                                    state: LoadState::ImportedLegacy,
                                    warning: None,
                                },
                                Err(import_error) => ConfigLoad {
                                    config: AppConfig::default(),
                                    state: LoadState::DefaultAfterError,
                                    warning: Some(import_error.to_string()),
                                },
                            };
                        }
                        Err(legacy_error) if is_not_found(&legacy_error) => {}
                        Err(legacy_error) => {
                            return ConfigLoad {
                                config: AppConfig::default(),
                                state: LoadState::DefaultAfterError,
                                warning: Some(format!(
                                    "legacy configuration was not imported: {legacy_error}"
                                )),
                            };
                        }
                    }
                }
                ConfigLoad {
                    config: AppConfig::default(),
                    state: LoadState::CreatedDefault,
                    warning: None,
                }
            }
            Err(error) => ConfigLoad {
                config: AppConfig::default(),
                state: LoadState::DefaultAfterError,
                warning: Some(error.to_string()),
            },
        }
    }

    /// Save a v5 config with a rolling private backup and atomic replacement.
    pub fn save(&self, config: &AppConfig) -> Result<(), StoreError> {
        self.ensure_private_directory()?;
        let payload = config.to_pretty_json()?;
        if payload.len() > MAX_CONFIG_BYTES {
            return Err(StoreError::TooLarge {
                actual: payload.len(),
                maximum: MAX_CONFIG_BYTES,
            });
        }

        // Match the released app: do not back up an invalid/symlinked/special
        // existing path, but safely replace the directory entry itself.
        if let Ok(previous) = read_private_file(&self.config_path, MAX_CONFIG_BYTES) {
            write_atomic(&self.backup_path, &previous)?;
        }
        write_atomic(&self.config_path, &payload)?;
        sync_directory(&self.directory)?;
        Ok(())
    }

    pub fn load_backup(&self) -> Result<Option<AppConfig>, StoreError> {
        match read_private_file(&self.backup_path, MAX_CONFIG_BYTES) {
            Ok(bytes) => Ok(Some(AppConfig::from_json(&bytes)?)),
            Err(error) if is_not_found(&error) => Ok(None),
            Err(error) => Err(error),
        }
    }
}

/// Resolve an OS-provided directory to an absolute physical path. Missing
/// suffix components are retained for later creation; existing ancestors are
/// canonicalized so symlink and parent-directory components cannot remain in
/// the path used for config files or the single-instance IPC socket.
fn resolve_directory_path(path: &Path, label: &str) -> Result<PathBuf, StoreError> {
    use std::path::Component;

    if !path.is_absolute() {
        return Err(StoreError::UnsafePath(format!(
            "{label} must be an absolute path"
        )));
    }
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(StoreError::UnsafePath(format!(
            "{label} must not contain parent-directory components"
        )));
    }

    let mut existing = path.to_path_buf();
    let mut missing = Vec::<std::ffi::OsString>::new();
    loop {
        match fs::metadata(&existing) {
            Ok(metadata) => {
                if !metadata.is_dir() {
                    return Err(StoreError::UnsafePath(format!(
                        "{label} is not a directory: {}",
                        existing.display()
                    )));
                }
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let Some(name) = existing.file_name() else {
                    return Err(StoreError::Io(error));
                };
                missing.push(name.to_os_string());
                if !existing.pop() {
                    return Err(StoreError::Io(error));
                }
            }
            Err(error) => return Err(StoreError::Io(error)),
        }
    }

    let mut resolved = existing.canonicalize()?;
    for component in missing.iter().rev() {
        resolved.push(component);
    }
    Ok(resolved)
}

fn is_not_found(error: &StoreError) -> bool {
    matches!(error, StoreError::Io(io_error) if io_error.kind() == std::io::ErrorKind::NotFound)
}

fn read_private_file(path: &Path, max_bytes: usize) -> Result<Vec<u8>, StoreError> {
    let parent = path.parent().ok_or_else(|| {
        StoreError::UnsafePath("Padfire config file has no parent directory".to_owned())
    })?;
    let file_name = path
        .file_name()
        .ok_or_else(|| StoreError::UnsafePath("Padfire config file has no filename".to_owned()))?;
    let parent = parent.canonicalize()?;
    let parent_metadata = fs::metadata(&parent)?;
    if !parent_metadata.is_dir() {
        return Err(StoreError::UnsafePath(format!(
            "Padfire config parent is not a directory: {}",
            parent.display()
        )));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if parent_metadata.uid() != nix::unistd::Uid::current().as_raw() {
            return Err(StoreError::UnsafePath(format!(
                "Padfire config parent is owned by another user: {}",
                parent.display()
            )));
        }
    }
    let resolved_path = parent.join(file_name);

    let mut options = OpenOptions::new();
    options.read(true);

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }

    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Foundation::GENERIC_READ;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_OPEN_REPARSE_POINT, READ_CONTROL, WRITE_DAC,
        };
        options
            .access_mode(GENERIC_READ | READ_CONTROL | WRITE_DAC)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }

    let file = options.open(&resolved_path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(StoreError::UnsafePath(format!(
            "Padfire config path is not a regular file: {}",
            path.display()
        )));
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if metadata.nlink() != 1 {
            return Err(StoreError::UnsafePath(format!(
                "hard-linked Padfire config files are not accepted: {}",
                path.display()
            )));
        }
        if metadata.uid() != nix::unistd::Uid::current().as_raw() {
            return Err(StoreError::UnsafePath(format!(
                "Padfire config file is owned by another user: {}",
                path.display()
            )));
        }
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
    }

    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use std::os::windows::io::AsRawHandle;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(StoreError::UnsafePath(format!(
                "Padfire config file is a reparse point: {}",
                path.display()
            )));
        }
        if windows_number_of_links(&file)? != 1 {
            return Err(StoreError::UnsafePath(format!(
                "hard-linked Padfire config files are not accepted: {}",
                path.display()
            )));
        }
        harden_windows_owner_only_handle(file.as_raw_handle(), path)?;
    }

    if metadata.len() > max_bytes as u64 {
        return Err(StoreError::TooLarge {
            actual: metadata.len() as usize,
            maximum: max_bytes,
        });
    }

    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take((max_bytes + 1) as u64).read_to_end(&mut bytes)?;
    if bytes.len() > max_bytes {
        return Err(StoreError::TooLarge {
            actual: bytes.len(),
            maximum: max_bytes,
        });
    }
    Ok(bytes)
}

fn write_atomic(path: &Path, payload: &[u8]) -> Result<(), StoreError> {
    let parent = path.parent().ok_or_else(|| {
        StoreError::UnsafePath(format!("missing parent directory for {}", path.display()))
    })?;

    let mut temp_path = None;
    let mut temp_file = None;
    for _ in 0..64 {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let name = format!(
            ".{}.{}.{}.{}.tmp",
            path.file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("padfire"),
            std::process::id(),
            nanos,
            sequence
        );
        let candidate = parent.join(name);
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);

        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options
                .mode(0o600)
                .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
        }

        match options.open(&candidate) {
            Ok(file) => {
                temp_path = Some(candidate);
                temp_file = Some(file);
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(StoreError::Io(error)),
        }
    }

    let temp_path = temp_path.ok_or(StoreError::TempNameExhausted)?;
    let mut file = temp_file.expect("temp path and file are allocated together");
    let result = (|| -> Result<(), StoreError> {
        file.write_all(payload)?;
        file.flush()?;
        file.sync_all()?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(fs::Permissions::from_mode(0o600))?;
        }

        drop(file);
        #[cfg(unix)]
        fs::rename(&temp_path, path)?;
        #[cfg(windows)]
        replace_windows_file(&temp_path, path)?;
        sync_directory(parent)?;
        Ok(())
    })();

    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    result
}

#[cfg(windows)]
#[allow(unsafe_code)]
fn replace_windows_file(source: &Path, destination: &Path) -> Result<(), StoreError> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };

    // Both files are created in the same protected directory, so this is a
    // same-volume rename. WRITE_THROUGH asks Windows not to return until the
    // move operation is flushed; the temporary file itself was sync_all'd
    // before this point. Unlike delete-then-rename, replacement never removes
    // the last valid config before the new file is ready.
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
    let flags = MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH;
    // SAFETY: both pointers are valid NUL-terminated UTF-16 paths for this call.
    let succeeded = unsafe { MoveFileExW(source.as_ptr(), destination.as_ptr(), flags) };
    if succeeded == 0 {
        return Err(StoreError::Io(std::io::Error::last_os_error()));
    }
    Ok(())
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<(), StoreError> {
    File::open(path)?.sync_all()?;
    Ok(())
}

#[cfg(windows)]
#[allow(unsafe_code)]
fn ensure_windows_private_directory(path: &Path) -> Result<(), StoreError> {
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::FromRawHandle;
    use windows_sys::Win32::Foundation::{
        ERROR_ALREADY_EXISTS, ERROR_FILE_EXISTS, INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, CreateDirectoryW, CreateFileW, FILE_ATTRIBUTE_DIRECTORY,
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
        GetFileInformationByHandle, OPEN_EXISTING, READ_CONTROL, WRITE_DAC,
    };

    // Create the final component with the private DACL from the first instant.
    // No config bytes are written until the directory has been opened and its
    // identity and owner have been checked through that stable handle.
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
        verify_windows_directory_parent(parent)?;
    }
    let descriptor = windows_owner_only_security_descriptor()?;
    let attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0,
        bInheritHandle: 0,
    };
    let wide_path = path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let created = unsafe { CreateDirectoryW(wide_path.as_ptr(), &attributes) };
    if created == 0 {
        let error = std::io::Error::last_os_error();
        if !matches!(error.raw_os_error(), Some(code) if code == ERROR_ALREADY_EXISTS as i32 || code == ERROR_FILE_EXISTS as i32)
        {
            return Err(StoreError::Io(error));
        }
    }

    let handle = unsafe {
        CreateFileW(
            wide_path.as_ptr(),
            READ_CONTROL | WRITE_DAC | FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(StoreError::Io(std::io::Error::last_os_error()));
    }
    // SAFETY: CreateFileW returned a new owned handle; File closes it on all paths.
    let directory = unsafe { fs::File::from_raw_handle(handle as _) };
    let mut information: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    if unsafe { GetFileInformationByHandle(handle, &mut information) } == 0 {
        return Err(StoreError::Io(std::io::Error::last_os_error()));
    }
    if information.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(StoreError::UnsafePath(format!(
            "Padfire configuration directory is a reparse point: {}",
            path.display()
        )));
    }
    if information.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY == 0 {
        return Err(StoreError::UnsafePath(format!(
            "Padfire configuration path is not a directory: {}",
            path.display()
        )));
    }

    harden_windows_owner_only_handle(handle, path)?;
    drop(directory);
    Ok(())
}

#[cfg(windows)]
#[allow(unsafe_code)]
fn verify_windows_directory_parent(path: &Path) -> Result<(), StoreError> {
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::FromRawHandle;
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, CreateFileW, FILE_ATTRIBUTE_DIRECTORY,
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
        GetFileInformationByHandle, OPEN_EXISTING, READ_CONTROL,
    };

    let wide_path = path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let handle = unsafe {
        CreateFileW(
            wide_path.as_ptr(),
            READ_CONTROL | FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(StoreError::Io(std::io::Error::last_os_error()));
    }
    // SAFETY: CreateFileW returned a newly owned directory handle.
    let parent = unsafe { fs::File::from_raw_handle(handle as _) };
    let mut information: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    if unsafe { GetFileInformationByHandle(handle, &mut information) } == 0 {
        return Err(StoreError::Io(std::io::Error::last_os_error()));
    }
    if information.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(StoreError::UnsafePath(format!(
            "Padfire configuration parent is a reparse point: {}",
            path.display()
        )));
    }
    if information.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY == 0 {
        return Err(StoreError::UnsafePath(format!(
            "Padfire configuration parent is not a directory: {}",
            path.display()
        )));
    }
    windows_assert_owned_by_current_user(handle, path)?;
    drop(parent);
    Ok(())
}

#[cfg(windows)]
#[allow(unsafe_code)]
fn windows_owner_only_security_descriptor() -> Result<WindowsLocalSecurityDescriptor, StoreError> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Security::Authorization::{
        ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
    };
    const OWNER_ONLY_SDDL: &str = "D:P(A;OICI;FA;;;OW)(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)";
    let wide = std::ffi::OsStr::new(OWNER_ONLY_SDDL)
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let mut descriptor = std::ptr::null_mut();
    let succeeded = unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            wide.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            std::ptr::null_mut(),
        )
    };
    if succeeded == 0 {
        return Err(StoreError::Io(std::io::Error::last_os_error()));
    }
    Ok(WindowsLocalSecurityDescriptor(descriptor))
}

#[cfg(windows)]
struct WindowsLocalSecurityDescriptor(windows_sys::Win32::Security::PSECURITY_DESCRIPTOR);

#[cfg(windows)]
#[allow(unsafe_code)]
impl Drop for WindowsLocalSecurityDescriptor {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::LocalFree(
                self.0 as windows_sys::Win32::Foundation::HLOCAL,
            );
        }
    }
}

#[cfg(windows)]
#[allow(unsafe_code)]
fn windows_token_info(
    class: windows_sys::Win32::Security::TOKEN_INFORMATION_CLASS,
) -> Result<Vec<u64>, StoreError> {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::Security::{GetTokenInformation, TOKEN_QUERY};
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    struct Token(HANDLE);
    impl Drop for Token {
        fn drop(&mut self) {
            unsafe { CloseHandle(self.0) };
        }
    }
    let mut raw = std::ptr::null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw) } == 0 {
        return Err(StoreError::Io(std::io::Error::last_os_error()));
    }
    let token = Token(raw);
    let mut length = 0u32;
    unsafe { GetTokenInformation(token.0, class, std::ptr::null_mut(), 0, &mut length) };
    if length == 0 {
        return Err(StoreError::Io(std::io::Error::last_os_error()));
    }
    let mut buffer = vec![0u64; (length as usize).div_ceil(std::mem::size_of::<u64>())];
    if unsafe {
        GetTokenInformation(
            token.0,
            class,
            buffer.as_mut_ptr() as *mut _,
            length,
            &mut length,
        )
    } == 0
    {
        return Err(StoreError::Io(std::io::Error::last_os_error()));
    }
    Ok(buffer)
}

#[cfg(windows)]
#[allow(unsafe_code)]
fn windows_assert_owned_by_current_user(
    handle: windows_sys::Win32::Foundation::HANDLE,
    path: &Path,
) -> Result<(), StoreError> {
    use windows_sys::Win32::Foundation::{ERROR_SUCCESS, HLOCAL, LocalFree};
    use windows_sys::Win32::Security::Authorization::{GetSecurityInfo, SE_FILE_OBJECT};
    use windows_sys::Win32::Security::{
        EqualSid, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID, TOKEN_OWNER, TOKEN_USER,
        TokenOwner, TokenUser,
    };

    let user = windows_token_info(TokenUser)?;
    let default_owner = windows_token_info(TokenOwner)?;
    // SAFETY: GetTokenInformation filled aligned buffers for the matching
    // classes; embedded SID pointers remain valid while each Vec is alive.
    let user_sid = unsafe { (*(user.as_ptr() as *const TOKEN_USER)).User.Sid };
    let default_owner_sid = unsafe { (*(default_owner.as_ptr() as *const TOKEN_OWNER)).Owner };
    let mut owner: PSID = std::ptr::null_mut();
    let mut security_descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
    let status = unsafe {
        GetSecurityInfo(
            handle,
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION,
            &mut owner,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut security_descriptor,
        )
    };
    if status != ERROR_SUCCESS {
        return Err(StoreError::Io(std::io::Error::from_raw_os_error(
            status as i32,
        )));
    }
    struct LocalDescriptor(PSECURITY_DESCRIPTOR);
    impl Drop for LocalDescriptor {
        fn drop(&mut self) {
            unsafe { LocalFree(self.0 as HLOCAL) };
        }
    }
    let _descriptor = LocalDescriptor(security_descriptor);
    let owned = !owner.is_null()
        && (unsafe { EqualSid(owner, user_sid) } != 0
            || unsafe { EqualSid(owner, default_owner_sid) } != 0);
    if !owned {
        return Err(StoreError::UnsafePath(format!(
            "refusing to change permissions on a Padfire path owned by another account: {}",
            path.display()
        )));
    }
    Ok(())
}

#[cfg(windows)]
#[allow(unsafe_code)]
fn harden_windows_owner_only_handle(
    handle: windows_sys::Win32::Foundation::HANDLE,
    path: &Path,
) -> Result<(), StoreError> {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::Security::Authorization::{SE_FILE_OBJECT, SetSecurityInfo};
    use windows_sys::Win32::Security::{
        ACL, DACL_SECURITY_INFORMATION, GetSecurityDescriptorDacl,
        PROTECTED_DACL_SECURITY_INFORMATION,
    };

    windows_assert_owned_by_current_user(handle, path)?;
    let descriptor = windows_owner_only_security_descriptor()?;
    let mut dacl_present = 0i32;
    let mut dacl: *mut ACL = std::ptr::null_mut();
    let mut dacl_defaulted = 0i32;
    if unsafe {
        GetSecurityDescriptorDacl(
            descriptor.0,
            &mut dacl_present,
            &mut dacl,
            &mut dacl_defaulted,
        )
    } == 0
    {
        return Err(StoreError::Io(std::io::Error::last_os_error()));
    }
    if dacl_present == 0 || dacl.is_null() {
        return Err(StoreError::UnsafePath(
            "owner-only Windows DACL could not be constructed".to_owned(),
        ));
    }
    let status = unsafe {
        SetSecurityInfo(
            handle,
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            dacl,
            std::ptr::null_mut(),
        )
    };
    if status != ERROR_SUCCESS {
        return Err(StoreError::Io(std::io::Error::from_raw_os_error(
            status as i32,
        )));
    }
    Ok(())
}

#[cfg(windows)]
#[allow(unsafe_code)]
fn windows_number_of_links(file: &fs::File) -> Result<u32, StoreError> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };

    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: file owns a live Windows handle for this call, and information
    // is a valid writable output structure of the documented size.
    let succeeded = unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut information) };
    if succeeded == 0 {
        return Err(StoreError::Io(std::io::Error::last_os_error()));
    }
    Ok(information.nNumberOfLinks)
}

#[cfg(windows)]
fn sync_directory(_path: &Path) -> Result<(), StoreError> {
    // `replace_windows_file` uses MOVEFILE_WRITE_THROUGH after the temporary
    // file has been sync_all'd. Windows does not expose the same portable
    // directory-fsync contract as Unix, so do not pretend this is a directory
    // handle flush. The write-through rename is the durability boundary here.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_ID: AtomicU64 = AtomicU64::new(0);

    fn test_store() -> ConfigStore {
        let id = TEST_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("padfire-store-test-{}-{id}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        ConfigStore::new(path)
    }

    #[cfg(windows)]
    #[allow(unsafe_code)]
    fn windows_sddl(path: &Path, directory: bool) -> Result<String, StoreError> {
        use std::os::windows::ffi::OsStrExt;
        use std::os::windows::io::FromRawHandle;
        use windows_sys::Win32::Foundation::{
            ERROR_SUCCESS, HLOCAL, INVALID_HANDLE_VALUE, LocalFree,
        };
        use windows_sys::Win32::Security::Authorization::{
            ConvertSecurityDescriptorToStringSecurityDescriptorW, GetSecurityInfo, SDDL_REVISION_1,
            SE_FILE_OBJECT,
        };
        use windows_sys::Win32::Security::{
            DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
        };
        use windows_sys::Win32::Storage::FileSystem::{
            CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
            FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
            OPEN_EXISTING, READ_CONTROL,
        };

        let wide = path
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        let mut flags = FILE_FLAG_OPEN_REPARSE_POINT;
        if directory {
            flags |= FILE_FLAG_BACKUP_SEMANTICS;
        }
        let handle = unsafe {
            CreateFileW(
                wide.as_ptr(),
                READ_CONTROL | FILE_READ_ATTRIBUTES,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                std::ptr::null(),
                OPEN_EXISTING,
                flags,
                std::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(StoreError::Io(std::io::Error::last_os_error()));
        }
        // SAFETY: this is an owned CreateFileW handle and File closes it.
        let _file = unsafe { fs::File::from_raw_handle(handle as _) };
        let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
        let status = unsafe {
            GetSecurityInfo(
                handle,
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | OWNER_SECURITY_INFORMATION,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut descriptor,
            )
        };
        if status != ERROR_SUCCESS {
            return Err(StoreError::Io(std::io::Error::from_raw_os_error(
                status as i32,
            )));
        }
        let descriptor = WindowsLocalSecurityDescriptor(descriptor);
        let mut sddl: *mut u16 = std::ptr::null_mut();
        let mut length = 0u32;
        let succeeded = unsafe {
            ConvertSecurityDescriptorToStringSecurityDescriptorW(
                descriptor.0,
                SDDL_REVISION_1,
                DACL_SECURITY_INFORMATION | OWNER_SECURITY_INFORMATION,
                &mut sddl,
                &mut length,
            )
        };
        if succeeded == 0 {
            return Err(StoreError::Io(std::io::Error::last_os_error()));
        }
        let units = unsafe { std::slice::from_raw_parts(sddl, length as usize) };
        let result = String::from_utf16_lossy(units)
            .trim_end_matches('\0')
            .to_owned();
        unsafe { LocalFree(sddl as HLOCAL) };
        Ok(result)
    }

    #[cfg(windows)]
    fn assert_private_windows_sddl(sddl: &str) {
        assert!(sddl.contains("D:P"), "DACL is not protected: {sddl}");
        assert!(sddl.contains("OW"), "owner rights ACE is missing: {sddl}");
        assert!(sddl.contains("SY"), "SYSTEM ACE is missing: {sddl}");
        assert!(sddl.contains("BA"), "Administrators ACE is missing: {sddl}");
        assert!(!sddl.contains("WD"), "Everyone has an ACE: {sddl}");
        assert!(
            !sddl.contains("AU"),
            "Authenticated Users has an ACE: {sddl}"
        );
        assert!(!sddl.contains(";;;BU"), "Builtin Users has an ACE: {sddl}");
    }

    #[test]
    fn missing_config_loads_defaults_and_save_creates_private_v5_file() {
        let store = test_store();
        let loaded = store.load();
        assert_eq!(loaded.state, LoadState::CreatedDefault);
        assert_eq!(loaded.config.version, 5);

        store.save(&loaded.config).unwrap();
        let reloaded = store.load();
        assert_eq!(reloaded.state, LoadState::Loaded);
        assert_eq!(reloaded.config, loaded.config);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(store.directory())
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
            assert_eq!(
                fs::metadata(store.config_path())
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        let _ = fs::remove_dir_all(store.directory());
    }

    #[test]
    fn second_save_keeps_previous_generation_as_backup() {
        let store = test_store();
        let first = AppConfig::default();
        store.save(&first).unwrap();

        let mut second = first.clone();
        second.retrigger = "toggle".to_owned();
        store.save(&second).unwrap();

        assert_eq!(store.load().config.retrigger, "toggle");
        assert_eq!(store.load_backup().unwrap().unwrap().retrigger, "restart");
        let _ = fs::remove_dir_all(store.directory());
    }

    #[cfg(windows)]
    #[test]
    fn windows_movefile_replace_keeps_backup_and_private_dacl() {
        let store = test_store();
        let previous = AppConfig {
            default_sink: "previous-output".to_owned(),
            ..AppConfig::default()
        };
        let next = AppConfig {
            default_sink: "next-output".to_owned(),
            ..AppConfig::default()
        };

        store.save(&previous).unwrap();
        store.save(&next).unwrap();
        assert_eq!(store.load().config, next);
        assert_eq!(store.load_backup().unwrap(), Some(previous));

        let current_sddl = windows_sddl(store.config_path(), false).unwrap();
        let backup_sddl = windows_sddl(store.backup_path(), false).unwrap();
        assert_private_windows_sddl(&current_sddl);
        assert_private_windows_sddl(&backup_sddl);

        let leftovers = fs::read_dir(store.directory())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
            .count();
        assert_eq!(leftovers, 0, "successful replacement left temporary files");
        let _ = fs::remove_dir_all(store.directory());
    }

    #[cfg(windows)]
    #[test]
    fn windows_replace_failure_preserves_current_config_and_cleans_temp() {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{FILE_SHARE_READ, FILE_SHARE_WRITE};

        let store = test_store();
        let previous = AppConfig {
            default_sink: "locked-previous-output".to_owned(),
            ..AppConfig::default()
        };
        let attempted = AppConfig {
            default_sink: "should-not-replace-locked-output".to_owned(),
            ..AppConfig::default()
        };
        store.save(&previous).unwrap();
        let locked = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .open(store.config_path())
            .unwrap();

        let result = store.save(&attempted);
        assert!(
            result.is_err(),
            "replacement unexpectedly succeeded while delete sharing was denied"
        );
        drop(locked);

        assert_eq!(store.load().config, previous);
        assert_eq!(store.load_backup().unwrap(), Some(previous));
        let leftovers = fs::read_dir(store.directory())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
            .count();
        assert_eq!(leftovers, 0, "failed replacement left temporary files");

        store.save(&attempted).unwrap();
        assert_eq!(store.load().config, attempted);
        let _ = fs::remove_dir_all(store.directory());
    }

    #[test]
    fn oversized_input_is_rejected_before_replacing_previous_file() {
        let store = test_store();
        let original = AppConfig {
            default_sink: "keep-me".to_owned(),
            ..AppConfig::default()
        };
        store.save(&original).unwrap();

        let bytes = vec![b'x'; MAX_CONFIG_BYTES + 1];
        assert!(AppConfig::from_json(&bytes).is_err());
        assert_eq!(store.load().config.default_sink, "keep-me");
        let _ = fs::remove_dir_all(store.directory());
    }

    #[cfg(unix)]
    #[test]
    fn symbolic_link_config_is_not_followed_and_is_replaced_atomically() {
        use std::os::unix::fs::symlink;

        let store = test_store();
        store.ensure_private_directory().unwrap();
        let target = store.directory().join("outside-target.json");
        fs::write(&target, br#"{"version":5,"default_sink":"outside"}"#).unwrap();
        symlink(&target, store.config_path()).unwrap();

        let loaded = store.load();
        assert_eq!(loaded.state, LoadState::DefaultAfterError);
        assert!(loaded.warning.is_some());
        store.save(&AppConfig::default()).unwrap();
        assert!(
            !fs::symlink_metadata(store.config_path())
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(
            fs::read_to_string(&target).unwrap(),
            r#"{"version":5,"default_sink":"outside"}"#
        );
        let _ = fs::remove_dir_all(store.directory());
    }

    #[cfg(unix)]
    #[test]
    fn hardlinked_config_is_rejected() {
        let store = test_store();
        store.ensure_private_directory().unwrap();
        let target = store.directory().join("source.json");
        fs::write(&target, br#"{"version":5}"#).unwrap();
        fs::hard_link(&target, store.config_path()).unwrap();

        let loaded = store.load();
        assert_eq!(loaded.state, LoadState::DefaultAfterError);
        assert!(loaded.warning.unwrap().contains("hard-linked"));
        let _ = fs::remove_dir_all(store.directory());
    }

    #[test]
    fn environment_directory_resolution_requires_absolute_paths_and_normalizes_symlinks() {
        use std::time::{SystemTime, UNIX_EPOCH};

        assert!(matches!(
            resolve_directory_path(Path::new("relative/config"), "XDG_CONFIG_HOME"),
            Err(StoreError::UnsafePath(_))
        ));
        assert!(matches!(
            resolve_directory_path(Path::new("/tmp/../etc"), "XDG_CONFIG_HOME"),
            Err(StoreError::UnsafePath(_))
        ));

        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "padfire-store-root-resolution-{}-{nonce}",
            std::process::id()
        ));
        let target = root.join("actual-config");
        #[cfg(unix)]
        let link = root.join("config-link");
        fs::create_dir_all(&target).unwrap();

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&target, &link).unwrap();
        }
        #[cfg(unix)]
        let candidate = link.as_path();
        #[cfg(windows)]
        let candidate = target.as_path();

        let resolved = resolve_directory_path(candidate, "XDG_CONFIG_HOME").unwrap();
        assert_eq!(resolved, target.canonicalize().unwrap());
        assert!(resolved.is_absolute());

        let missing =
            resolve_directory_path(&target.join("new/nested"), "XDG_CONFIG_HOME").unwrap();
        assert_eq!(missing, target.canonicalize().unwrap().join("new/nested"));
        assert!(!missing.exists());

        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn config_directory_symlink_is_rejected() {
        use std::os::unix::fs::symlink;
        let store = test_store();
        let target = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("padfire-store-target-{}", std::process::id()));
        fs::create_dir_all(&target).unwrap();
        let _ = fs::remove_dir_all(store.directory());
        symlink(&target, store.directory()).unwrap();
        assert!(store.ensure_private_directory().is_err());
        let _ = fs::remove_file(store.directory());
        let _ = fs::remove_dir_all(target);
    }

    #[cfg(windows)]
    #[test]
    fn config_directory_and_file_have_protected_owner_only_dacls() {
        let store = test_store();
        store.ensure_private_directory().unwrap();
        let directory_sddl = windows_sddl(store.directory(), true).unwrap();
        assert_private_windows_sddl(&directory_sddl);

        store.save(&AppConfig::default()).unwrap();
        assert_eq!(store.load().state, LoadState::Loaded);
        let file_sddl = windows_sddl(store.config_path(), false).unwrap();
        assert_private_windows_sddl(&file_sddl);
    }

    #[cfg(windows)]
    #[test]
    fn configuration_directory_junction_is_rejected() {
        let base = test_store().directory;
        let target = base.with_extension("junction-target");
        let junction = base.with_extension("junction");
        fs::create_dir_all(&target).unwrap();
        let junction_text = junction.to_string_lossy().into_owned();
        let target_text = target.to_string_lossy().into_owned();
        let output = std::process::Command::new("cmd.exe")
            .args(["/C", "mklink", "/J", &junction_text, &target_text])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "could not create test junction: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );

        let result = ConfigStore::new(&junction).ensure_private_directory();
        let _ = fs::remove_dir(&junction);
        let _ = fs::remove_dir_all(&target);
        assert!(
            matches!(result, Err(StoreError::UnsafePath(_))),
            "junction was not rejected: {result:?}"
        );
    }

    #[cfg(windows)]
    #[test]
    fn configuration_directory_with_junction_parent_is_rejected() {
        let base = test_store().directory;
        let target = base.with_extension("junction-parent-target");
        let junction = base.with_extension("junction-parent");
        fs::create_dir_all(&target).unwrap();
        let junction_text = junction.to_string_lossy().into_owned();
        let target_text = target.to_string_lossy().into_owned();
        let output = std::process::Command::new("cmd.exe")
            .args(["/C", "mklink", "/J", &junction_text, &target_text])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "could not create parent test junction: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );

        let store = ConfigStore::new(junction.join("Padfire"));
        let result = store.ensure_private_directory();
        let _ = fs::remove_dir(&junction);
        let _ = fs::remove_dir_all(&target);
        assert!(
            matches!(result, Err(StoreError::UnsafePath(_))),
            "junction parent was not rejected: {result:?}"
        );
    }

    #[cfg(windows)]
    #[test]
    fn hardlinked_config_is_rejected() {
        let store = test_store();
        store.ensure_private_directory().unwrap();
        let target = store.directory().join("source.json");
        fs::write(&target, br#"{"version":5}"#).unwrap();
        fs::hard_link(&target, store.config_path()).unwrap();

        let loaded = store.load();
        assert_eq!(loaded.state, LoadState::DefaultAfterError);
        assert!(loaded.warning.unwrap().contains("hard-linked"));
        let _ = fs::remove_dir_all(store.directory());
    }
}
