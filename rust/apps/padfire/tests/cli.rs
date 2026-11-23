use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

struct IsolatedConfig {
    root: PathBuf,
}

impl IsolatedConfig {
    fn new(label: &str) -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let root = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "padfire-cli-{label}-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("create isolated CLI test directory");
        Self { root }
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_padfire"));
        command.args(args).env("PADFIRE_DISABLE_HARDWARE", "1");

        #[cfg(target_os = "linux")]
        {
            command
                .env("HOME", &self.root)
                .env("XDG_CONFIG_HOME", self.root.join("xdg-config"))
                .env("XDG_DATA_HOME", self.root.join("xdg-data"))
                .env("XDG_RUNTIME_DIR", self.root.join("xdg-runtime"));
        }

        #[cfg(windows)]
        {
            command
                .env("APPDATA", self.root.join("AppData"))
                .env("LOCALAPPDATA", self.root.join("LocalAppData"))
                .env("USERPROFILE", &self.root);
        }

        command
    }

    fn run(&self, args: &[&str]) -> Output {
        self.command(args)
            .output()
            .expect("run isolated Padfire CLI")
    }
}

impl Drop for IsolatedConfig {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn status_when_stopped_returns_stable_json_and_success() {
    let isolated = IsolatedConfig::new("stopped-status");
    let output = isolated.run(&["--status"]);

    assert!(
        output.status.success(),
        "--status should succeed when stopped: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("status JSON is UTF-8");
    assert!(
        stdout.contains("\"running\":false"),
        "unexpected status: {stdout}"
    );
    assert!(
        stdout.contains("\"version\":\""),
        "status response omitted the version: {stdout}"
    );
}

#[test]
fn control_commands_fail_with_nonzero_exit_when_no_instance_is_running() {
    for action in ["--show", "--stop", "--reload", "--quit"] {
        let isolated = IsolatedConfig::new(action.trim_start_matches('-'));
        let output = isolated.run(&[action]);
        assert_eq!(
            output.status.code(),
            Some(1),
            "{action} should return exit code 1 when no Padfire instance exists; stdout={}, stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("not running"),
            "{action} did not explain that Padfire is stopped"
        );
    }
}
