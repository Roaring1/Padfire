//! Secure v5 JSON configuration storage.
//!
//! Unix: private 0700 directory, owner-only regular files, no symlink/hardlink
//! reads, bounded input, same-directory atomic replacement and directory fsync.
//! Windows: per-user AppData directory, reparse-point and hardlink rejection.
//! A Windows ACL audit/explicit DACL remains a required integration gate.

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
            let base = std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
                .unwrap_or_else(|| home.join(".config"));
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
        fs::create_dir_all(&self.directory)?;
        let metadata = fs::symlink_metadata(&self.directory)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(StoreError::UnsafePath(format!(
                "Padfire configuration directory is not a real directory: {}",
                self.directory.display()
            )));
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::{MetadataExt, PermissionsExt};
            if metadata.uid() != nix::unistd::Uid::current().as_raw() {
                return Err(StoreError::UnsafePath(format!(
                    "Padfire configuration directory is owned by another user: {}",
                    self.directory.display()
                )));
            }
            fs::set_permissions(&self.directory, fs::Permissions::from_mode(0o700))?;
        }

        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
            if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                return Err(StoreError::UnsafePath(format!(
                    "Padfire configuration directory is a reparse point: {}",
                    self.directory.display()
                )));
            }
        }
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

fn is_not_found(error: &StoreError) -> bool {
    matches!(error, StoreError::Io(io_error) if io_error.kind() == std::io::ErrorKind::NotFound)
}

fn read_private_file(path: &Path, max_bytes: usize) -> Result<Vec<u8>, StoreError> {
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
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }

    let file = options.open(path)?;
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
        fs::rename(&temp_path, path)?;
        sync_directory(parent)?;
        Ok(())
    })();

    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    result
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<(), StoreError> {
    File::open(path)?.sync_all()?;
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
    // Windows directory durability is platform-specific; flush the file
    // before replacement and add an explicit native API implementation later.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_ID: AtomicU64 = AtomicU64::new(0);

    fn test_store() -> ConfigStore {
        let id = TEST_ID.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("padfire-store-test-{}-{id}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        ConfigStore::new(path)
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

    #[cfg(unix)]
    #[test]
    fn config_directory_symlink_is_rejected() {
        use std::os::unix::fs::symlink;
        let store = test_store();
        let target =
            std::env::temp_dir().join(format!("padfire-store-target-{}", std::process::id()));
        fs::create_dir_all(&target).unwrap();
        let _ = fs::remove_dir_all(store.directory());
        symlink(&target, store.directory()).unwrap();
        assert!(store.ensure_private_directory().is_err());
        let _ = fs::remove_file(store.directory());
        let _ = fs::remove_dir_all(target);
    }
}
