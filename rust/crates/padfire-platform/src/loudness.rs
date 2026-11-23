use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

pub const TARGET_LUFS: f64 = -18.0;
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LoudnessResult {
    pub integrated_lufs: f64,
    pub true_peak_dbfs: f64,
    pub suggested_volume_percent: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoudnessError {
    Spawn(String),
    Read(String),
    Failed(String),
    InvalidReport(String),
    InvalidInput(String),
    Timeout,
}

impl std::fmt::Display for LoudnessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Spawn(message) => write!(f, "could not start ffmpeg: {message}"),
            Self::Read(message) => write!(f, "could not read ffmpeg output: {message}"),
            Self::Failed(message) => write!(f, "ffmpeg loudness analysis failed: {message}"),
            Self::InvalidReport(message) => write!(f, "invalid loudness report: {message}"),
            Self::InvalidInput(message) => write!(f, "loudness input file was rejected: {message}"),
            Self::Timeout => write!(f, "loudness analysis exceeded its 30-second limit"),
        }
    }
}

impl std::error::Error for LoudnessError {}

pub fn analyze_lufs(
    filepath: &str,
    cancelled: &AtomicBool,
) -> Result<Option<LoudnessResult>, LoudnessError> {
    analyze_lufs_with_timeout(filepath, cancelled, DEFAULT_TIMEOUT)
}

fn analyze_lufs_with_timeout(
    filepath: &str,
    cancelled: &AtomicBool,
    timeout: Duration,
) -> Result<Option<LoudnessResult>, LoudnessError> {
    if cancelled.load(Ordering::Acquire) {
        return Ok(None);
    }

    let input_path = resolve_local_audio_file(filepath)?;
    let mut command = Command::new("ffmpeg");
    command.args([
        "-hide_banner",
        "-t",
        "120",
        "-protocol_whitelist",
        "file",
        "-i",
    ]);
    command.arg(&input_path);
    command.args(["-filter:a", "loudnorm=print_format=json", "-f", "null", "-"]);
    command.stdout(Stdio::null()).stderr(Stdio::piped());

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }

    let mut child = command
        .spawn()
        .map_err(|e| LoudnessError::Spawn(e.to_string()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| LoudnessError::Read("stderr pipe was not created".to_owned()))?;
    let reader = match thread::Builder::new()
        .name("padfire-loudness-output".to_owned())
        .spawn(move || {
            let mut bytes = Vec::new();
            let mut stream = stderr;
            stream.read_to_end(&mut bytes).map(|_| bytes)
        }) {
        Ok(reader) => reader,
        Err(error) => {
            terminate(&mut child);
            return Err(LoudnessError::Read(error.to_string()));
        }
    };

    let deadline = Instant::now() + timeout;
    let status = loop {
        if cancelled.load(Ordering::Acquire) {
            terminate(&mut child);
            let _ = reader.join();
            return Ok(None);
        }
        if Instant::now() >= deadline {
            terminate(&mut child);
            let _ = reader.join();
            return Err(LoudnessError::Timeout);
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => thread::sleep(Duration::from_millis(40)),
            Err(error) => {
                terminate(&mut child);
                let _ = reader.join();
                return Err(LoudnessError::Failed(error.to_string()));
            }
        }
    };

    let output = reader
        .join()
        .map_err(|_| LoudnessError::Read("stderr reader thread panicked".to_owned()))?
        .map_err(|e| LoudnessError::Read(e.to_string()))?;

    if cancelled.load(Ordering::Acquire) {
        return Ok(None);
    }
    if !status.success() {
        let tail = String::from_utf8_lossy(&output);
        let tail = tail.lines().rev().take(4).collect::<Vec<_>>().join(" ");
        return Err(LoudnessError::Failed(if tail.is_empty() {
            format!("process exited with {status}")
        } else {
            tail
        }));
    }
    parse_loudnorm_report(&output)
}

fn resolve_local_audio_file(filepath: &str) -> Result<std::path::PathBuf, LoudnessError> {
    if filepath.trim().is_empty() {
        return Err(LoudnessError::InvalidInput("path is empty".to_owned()));
    }

    let candidate = std::path::Path::new(filepath);
    let canonical = candidate.canonicalize().map_err(|error| {
        LoudnessError::InvalidInput(format!("path could not be resolved: {error}"))
    })?;
    let metadata = std::fs::metadata(&canonical).map_err(|error| {
        LoudnessError::InvalidInput(format!("could not inspect resolved file: {error}"))
    })?;
    if !metadata.is_file() {
        return Err(LoudnessError::InvalidInput(
            "resolved path is not a regular file".to_owned(),
        ));
    }
    Ok(canonical)
}

fn terminate(child: &mut std::process::Child) {
    #[cfg(target_os = "linux")]
    {
        use nix::sys::signal::{Signal, kill};
        use nix::unistd::Pid;

        if matches!(child.try_wait(), Ok(Some(_))) {
            return;
        }
        let process_group = Pid::from_raw(-(child.id() as i32));
        let _ = kill(process_group, Signal::SIGTERM);
        let deadline = Instant::now() + Duration::from_millis(500);
        while Instant::now() < deadline {
            if matches!(child.try_wait(), Ok(Some(_))) {
                return;
            }
            thread::sleep(Duration::from_millis(20));
        }
        let _ = kill(process_group, Signal::SIGKILL);
    }

    #[cfg(windows)]
    {
        let _ = child.kill();
    }

    let _ = child.wait();
}

fn parse_loudnorm_report(output: &[u8]) -> Result<Option<LoudnessResult>, LoudnessError> {
    let text = String::from_utf8_lossy(output);
    let start = text
        .rfind('{')
        .ok_or_else(|| LoudnessError::InvalidReport("JSON object was not found".to_owned()))?;
    let end = text
        .rfind('}')
        .filter(|end| *end >= start)
        .map(|end| end + 1)
        .ok_or_else(|| LoudnessError::InvalidReport("JSON object was incomplete".to_owned()))?;
    let report: serde_json::Value = serde_json::from_str(&text[start..end])
        .map_err(|e| LoudnessError::InvalidReport(e.to_string()))?;
    let measured = report["input_i"]
        .as_str()
        .and_then(|v| v.parse::<f64>().ok())
        .or_else(|| report["input_i"].as_f64())
        .ok_or_else(|| LoudnessError::InvalidReport("input_i is missing".to_owned()))?;
    let peak = report["input_tp"]
        .as_str()
        .and_then(|v| v.parse::<f64>().ok())
        .or_else(|| report["input_tp"].as_f64())
        .ok_or_else(|| LoudnessError::InvalidReport("input_tp is missing".to_owned()))?;

    if !measured.is_finite() || !peak.is_finite() {
        return Ok(None);
    }
    let linear_gain = 10_f64.powf((TARGET_LUFS - measured) / 20.0);
    let suggested = (100.0 * linear_gain).round().clamp(1.0, 150.0) as u8;
    Ok(Some(LoudnessResult {
        integrated_lufs: measured,
        true_peak_dbfs: peak,
        suggested_volume_percent: suggested,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(input_i: &str, input_tp: &str) -> Vec<u8> {
        format!(
            "ffmpeg output\n{{\n  \"input_i\" : \"{input_i}\",\n  \"input_tp\" : \"{input_tp}\"\n}}\n"
        ).into_bytes()
    }

    #[test]
    fn loudness_input_requires_an_existing_local_regular_file_and_returns_canonical_path() {
        use std::time::{SystemTime, UNIX_EPOCH};

        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "padfire-loudness-input-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let file = root.join("-selected-audio.wav");
        std::fs::write(&file, b"bounded test fixture").unwrap();

        let resolved = resolve_local_audio_file(file.to_str().unwrap()).unwrap();
        assert!(resolved.is_absolute());
        assert_eq!(resolved, file.canonicalize().unwrap());

        assert!(matches!(
            resolve_local_audio_file("https://example.invalid/audio.wav"),
            Err(LoudnessError::InvalidInput(_))
        ));
        assert!(matches!(
            resolve_local_audio_file(root.to_str().unwrap()),
            Err(LoudnessError::InvalidInput(_))
        ));
        assert!(matches!(
            resolve_local_audio_file(""),
            Err(LoudnessError::InvalidInput(_))
        ));

        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn loudness_input_resolves_symlinks_to_their_regular_file_target() {
        use std::os::unix::fs::symlink;
        use std::time::{SystemTime, UNIX_EPOCH};

        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "padfire-loudness-link-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let target = root.join("audio.wav");
        let link = root.join("selected-audio.wav");
        std::fs::write(&target, b"fixture").unwrap();
        symlink(&target, &link).unwrap();

        let resolved = resolve_local_audio_file(link.to_str().unwrap()).unwrap();
        assert_eq!(resolved, target.canonicalize().unwrap());
        assert_ne!(resolved, link);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn target_level_suggests_unity_volume() {
        let result = parse_loudnorm_report(&report("-18.0", "-1.0"))
            .unwrap()
            .unwrap();
        assert_eq!(result.integrated_lufs, -18.0);
        assert_eq!(result.true_peak_dbfs, -1.0);
        assert_eq!(result.suggested_volume_percent, 100);
    }

    #[test]
    fn quiet_audio_suggestion_is_clamped_to_150_percent() {
        let result = parse_loudnorm_report(&report("-35.0", "-2.0"))
            .unwrap()
            .unwrap();
        assert_eq!(result.suggested_volume_percent, 150);
    }

    #[test]
    fn loud_audio_suggestion_is_clamped_to_one_percent() {
        let result = parse_loudnorm_report(&report("25.0", "-0.1"))
            .unwrap()
            .unwrap();
        assert_eq!(result.suggested_volume_percent, 1);
    }

    #[test]
    fn silent_input_has_no_gain_suggestion() {
        assert_eq!(
            parse_loudnorm_report(&report("-inf", "-inf")).unwrap(),
            None
        );
    }

    #[test]
    fn malformed_report_is_rejected() {
        assert!(parse_loudnorm_report(b"ffmpeg did not emit a report").is_err());
        assert!(parse_loudnorm_report(b"{\"input_i\":").is_err());
    }

    #[test]
    fn ffmpeg_end_to_end_smoke_when_installed() {
        let probe = Command::new("ffmpeg")
            .arg("-version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        if !probe.is_ok_and(|status| status.success()) {
            return;
        }

        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("padfire-lufs-{}-{nonce}.wav", std::process::id()));
        let generated = Command::new("ffmpeg")
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=1",
                "-c:a",
                "pcm_s16le",
            ])
            .arg(&path)
            .status()
            .expect("ffmpeg was present during the version probe");
        assert!(generated.success(), "could not generate analyzer fixture");
        let cancelled = AtomicBool::new(false);
        let result = analyze_lufs(path.to_str().unwrap(), &cancelled);
        let _ = std::fs::remove_file(&path);
        let result = result.unwrap().expect("sine fixture must not be silent");
        assert!(result.integrated_lufs.is_finite());
        assert!(result.true_peak_dbfs.is_finite());
        assert!((1..=150).contains(&result.suggested_volume_percent));
    }

    #[test]
    fn cancellation_before_spawn_does_not_start_ffmpeg() {
        let cancelled = AtomicBool::new(true);
        assert_eq!(
            analyze_lufs_with_timeout("not-a-real-file", &cancelled, Duration::from_secs(1))
                .unwrap(),
            None
        );
    }
}
