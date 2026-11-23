//! Lazy system-output loopback meter.
//!
//! The capture stream exists only while Padfire is playing audio. Linux opens
//! the PulseAudio/PipeWire monitor source for the selected sink; Windows opens
//! a WASAPI loopback capture stream against the selected render endpoint.
//! Samples are reduced to a single RMS value in the real-time callback. No
//! samples are retained, written to disk, or forwarded to another process.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, DeviceId, FromSample, Sample, SampleFormat, Stream, StreamConfig};
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const RETRY_INTERVAL: Duration = Duration::from_secs(5);
const SOURCE_REFRESH_INTERVAL: Duration = Duration::from_secs(5);
const WORKER_POLL_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug)]
enum Command {
    SetTarget(String),
    Shutdown,
}

/// A controller for one optional loopback capture worker.
///
/// `None` means idle. `Some("")` means follow the current system default
/// output; a non-empty value is a stable output-device ID from CPAL.
pub struct OutputLoopbackMeter {
    target: Option<String>,
    commands: Option<Sender<Command>>,
    worker: Option<JoinHandle<()>>,
    level: Arc<AtomicU32>,
    capturing: Arc<AtomicBool>,
}

impl OutputLoopbackMeter {
    pub fn new() -> Self {
        Self {
            target: None,
            commands: None,
            worker: None,
            level: Arc::new(AtomicU32::new(0.0f32.to_bits())),
            capturing: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Starts capture for a changed target, or tears the worker down when idle.
    /// Repeated calls with an unchanged target are allocation- and I/O-free.
    pub fn set_target(&mut self, target: Option<String>) {
        if self.target == target {
            return;
        }

        match target {
            None => {
                self.stop_worker();
                self.target = None;
            }
            Some(target) => {
                if let Some(commands) = &self.commands
                    && commands.send(Command::SetTarget(target.clone())).is_ok()
                {
                    self.target = Some(target);
                    return;
                }
                self.stop_worker();
                let (commands, receiver) = mpsc::channel();
                let level = Arc::clone(&self.level);
                let capturing = Arc::clone(&self.capturing);
                let initial_target = target.clone();
                match thread::Builder::new()
                    .name("padfire-output-loopback".to_owned())
                    .spawn(move || run_worker(receiver, initial_target, level, capturing))
                {
                    Ok(worker) => {
                        self.commands = Some(commands);
                        self.worker = Some(worker);
                        self.target = Some(target);
                    }
                    Err(_) => {
                        self.target = Some(target);
                        self.commands = None;
                        self.worker = None;
                        self.capturing.store(false, Ordering::Release);
                        self.level.store(0.0f32.to_bits(), Ordering::Release);
                    }
                }
            }
        }
    }

    /// Returns `Some(level)` only while native loopback capture is live.
    /// Callers may use the Padfire-mix meter as a fallback when this is `None`.
    pub fn level(&self) -> Option<f32> {
        if !self.capturing.load(Ordering::Acquire) {
            return None;
        }
        Some(f32::from_bits(self.level.load(Ordering::Relaxed)).clamp(0.0, 1.0))
    }

    pub fn shutdown(&mut self) {
        self.stop_worker();
        self.target = None;
    }

    fn stop_worker(&mut self) {
        if let Some(commands) = self.commands.take() {
            let _ = commands.send(Command::Shutdown);
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        self.capturing.store(false, Ordering::Release);
        self.level.store(0.0f32.to_bits(), Ordering::Release);
    }
}

impl Default for OutputLoopbackMeter {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for OutputLoopbackMeter {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn run_worker(
    receiver: Receiver<Command>,
    mut target: String,
    level: Arc<AtomicU32>,
    capturing: Arc<AtomicBool>,
) {
    let mut stream: Option<Stream> = None;
    let mut active_device_id = String::new();
    let mut retry_at = Instant::now();
    let mut next_source_check = Instant::now();
    let stream_error = Arc::new(AtomicBool::new(false));

    loop {
        match receiver.recv_timeout(WORKER_POLL_INTERVAL) {
            Ok(Command::SetTarget(next_target)) => {
                if next_target != target {
                    capturing.store(false, Ordering::Release);
                    level.store(0.0f32.to_bits(), Ordering::Release);
                    stream.take();
                    active_device_id.clear();
                    target = next_target;
                    retry_at = Instant::now();
                }
            }
            Ok(Command::Shutdown) | Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {}
        }

        if stream_error.swap(false, Ordering::AcqRel) {
            capturing.store(false, Ordering::Release);
            level.store(0.0f32.to_bits(), Ordering::Release);
            stream.take();
            active_device_id.clear();
            retry_at = Instant::now() + RETRY_INTERVAL;
        }

        if stream.is_some() && Instant::now() >= next_source_check {
            next_source_check = Instant::now() + SOURCE_REFRESH_INTERVAL;
            match resolve_loopback_device(&target) {
                Ok((_, _, current_id)) if current_id == active_device_id => {}
                _ => {
                    capturing.store(false, Ordering::Release);
                    level.store(0.0f32.to_bits(), Ordering::Release);
                    stream.take();
                    active_device_id.clear();
                    retry_at = Instant::now();
                }
            }
        }

        if stream.is_none() && Instant::now() >= retry_at {
            match build_loopback_stream(&target, Arc::clone(&level), Arc::clone(&stream_error)) {
                Ok((next_stream, device_id)) => match next_stream.play() {
                    Ok(()) => {
                        stream = Some(next_stream);
                        active_device_id = device_id;
                        next_source_check = Instant::now() + SOURCE_REFRESH_INTERVAL;
                        capturing.store(true, Ordering::Release);
                    }
                    Err(_) => {
                        capturing.store(false, Ordering::Release);
                        level.store(0.0f32.to_bits(), Ordering::Release);
                        stream.take();
                        retry_at = Instant::now() + RETRY_INTERVAL;
                    }
                },
                Err(_) => {
                    capturing.store(false, Ordering::Release);
                    level.store(0.0f32.to_bits(), Ordering::Release);
                    retry_at = Instant::now() + RETRY_INTERVAL;
                }
            }
        }
    }

    capturing.store(false, Ordering::Release);
    level.store(0.0f32.to_bits(), Ordering::Release);
    stream.take();
}

fn resolve_loopback_device(requested: &str) -> Result<(cpal::Host, Device, String), String> {
    let requested = requested.trim();
    let parsed_id = if requested.is_empty() {
        None
    } else {
        DeviceId::from_str(requested).ok()
    };
    let host = match parsed_id.as_ref() {
        Some(id) => cpal::host_from_id(id.host()).or_else(|_| selected_host())?,
        None => selected_host()?,
    };

    if let Some(id) = parsed_id
        && host.device_by_id(&id).is_some()
    {
        return resolve_loopback_device_by_id(host, id);
    }

    if !requested.is_empty() {
        let devices = host.output_devices().map_err(|error| error.to_string())?;
        if let Some(id) = devices.into_iter().find_map(|device| {
            let id = device.id().ok()?;
            let name = device
                .description()
                .map(|description| description.name().to_owned())
                .unwrap_or_else(|_| device.to_string());
            (id.to_string().eq_ignore_ascii_case(requested) || name.eq_ignore_ascii_case(requested))
                .then_some(id)
        }) {
            return resolve_loopback_device_by_id(host, id);
        }
    }

    // Playback resolution falls back to the selected host's default output if
    // a saved ID/name has disappeared. Keep the VU attached to that same sink.
    let fallback = host
        .default_output_device()
        .ok_or_else(|| "no default output device is available for loopback capture".to_owned())?;
    let id = fallback.id().map_err(|error| error.to_string())?;
    resolve_loopback_device_by_id(host, id)
}

fn resolve_loopback_device_by_id(
    host: cpal::Host,
    output_id: DeviceId,
) -> Result<(cpal::Host, Device, String), String> {
    let capture_id = loopback_capture_id(&output_id)?;
    let capture_device = host
        .device_by_id(&capture_id)
        .ok_or_else(|| format!("loopback source is unavailable for {}", output_id))?;
    Ok((host, capture_device, capture_id.to_string()))
}

fn loopback_capture_id(output_id: &DeviceId) -> Result<DeviceId, String> {
    #[cfg(target_os = "linux")]
    {
        if !output_id
            .host()
            .to_string()
            .eq_ignore_ascii_case("pulseaudio")
        {
            return Err(
                "system loopback requires the PulseAudio/PipeWire compatibility host".to_owned(),
            );
        }
        Ok(DeviceId::new(
            output_id.host(),
            format!("{}.monitor", output_id.id()),
        ))
    }

    #[cfg(windows)]
    {
        // WASAPI render endpoints expose their mix format for a loopback input
        // stream. The capture builder below uses default_output_config for
        // that endpoint, then build_input_stream enables LOOPBACK mode.
        Ok(output_id.clone())
    }
}

fn selected_host() -> Result<cpal::Host, String> {
    #[cfg(target_os = "linux")]
    {
        if let Some(id) = cpal::available_hosts()
            .into_iter()
            .find(|id| id.to_string().eq_ignore_ascii_case("pulseaudio"))
        {
            return cpal::host_from_id(id).map_err(|error| error.to_string());
        }
    }
    Ok(cpal::default_host())
}

fn build_loopback_stream(
    requested: &str,
    level: Arc<AtomicU32>,
    stream_error: Arc<AtomicBool>,
) -> Result<(Stream, String), String> {
    let (_host, device, device_id) = resolve_loopback_device(requested)?;

    #[cfg(target_os = "linux")]
    let supported = device
        .default_input_config()
        .map_err(|error| error.to_string())?;
    #[cfg(windows)]
    let supported = device
        .default_output_config()
        .map_err(|error| error.to_string())?;

    let sample_format = supported.sample_format();
    let config: StreamConfig = supported.into();
    let stream = match sample_format {
        SampleFormat::I8 => build_typed_stream::<i8>(&device, config, level, stream_error),
        SampleFormat::I16 => build_typed_stream::<i16>(&device, config, level, stream_error),
        SampleFormat::I24 => build_typed_stream::<cpal::I24>(&device, config, level, stream_error),
        SampleFormat::I32 => build_typed_stream::<i32>(&device, config, level, stream_error),
        SampleFormat::I64 => build_typed_stream::<i64>(&device, config, level, stream_error),
        SampleFormat::U8 => build_typed_stream::<u8>(&device, config, level, stream_error),
        SampleFormat::U16 => build_typed_stream::<u16>(&device, config, level, stream_error),
        SampleFormat::U24 => build_typed_stream::<cpal::U24>(&device, config, level, stream_error),
        SampleFormat::U32 => build_typed_stream::<u32>(&device, config, level, stream_error),
        SampleFormat::U64 => build_typed_stream::<u64>(&device, config, level, stream_error),
        SampleFormat::F32 => build_typed_stream::<f32>(&device, config, level, stream_error),
        SampleFormat::F64 => build_typed_stream::<f64>(&device, config, level, stream_error),
        _ => Err("loopback device uses an unsupported DSD sample format".to_owned()),
    }?;
    Ok((stream, device_id))
}

fn build_typed_stream<T>(
    device: &Device,
    config: StreamConfig,
    level: Arc<AtomicU32>,
    stream_error: Arc<AtomicBool>,
) -> Result<Stream, String>
where
    T: cpal::Sample + cpal::SizedSample + Send + 'static,
    f32: FromSample<T>,
{
    let callback_level = Arc::clone(&level);
    let callback_error = Arc::clone(&stream_error);
    device
        .build_input_stream::<T, _, _>(
            config,
            move |samples, _| store_rms_level(samples, &callback_level),
            move |_| callback_error.store(true, Ordering::Release),
            Some(Duration::from_secs(2)),
        )
        .map_err(|error| error.to_string())
}

fn store_rms_level<T>(samples: &[T], level: &AtomicU32)
where
    T: Sample,
    f32: FromSample<T>,
{
    if samples.is_empty() {
        level.store(0.0f32.to_bits(), Ordering::Relaxed);
        return;
    }
    let mut sum_squares = 0.0f64;
    let mut count = 0u64;
    for sample in samples {
        let value = f32::from_sample(*sample);
        if value.is_finite() {
            let value = f64::from(value.clamp(-1.0, 1.0));
            sum_squares += value * value;
            count += 1;
        }
    }
    let rms = if count == 0 {
        0.0
    } else {
        (sum_squares / count as f64).sqrt()
    };
    let db = if rms > 1.0e-7 {
        20.0 * rms.log10()
    } else {
        -96.0
    };
    let normalized = (((db + 60.0) / 60.0).clamp(0.0, 1.0)) as f32;
    level.store(normalized.to_bits(), Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_meter_is_lazy_and_reports_no_capture_when_idle() {
        let meter = OutputLoopbackMeter::new();
        assert!(meter.worker.is_none());
        assert!(meter.commands.is_none());
        assert_eq!(meter.level(), None);
    }

    #[test]
    fn selected_pulseaudio_sink_maps_to_its_monitor_source() {
        #[cfg(target_os = "linux")]
        {
            let output = DeviceId::new(
                cpal::HostId::PulseAudio,
                "alsa_output.pci-0000_00_1f.3.analog-stereo",
            );
            let capture = loopback_capture_id(&output).unwrap();
            assert_eq!(
                capture.id(),
                "alsa_output.pci-0000_00_1f.3.analog-stereo.monitor"
            );
        }
    }

    #[test]
    fn output_level_mapping_matches_legacy_minus_sixty_to_zero_db_scale() {
        let level_for = |rms: f32| {
            let samples = [rms];
            let level = AtomicU32::new(0);
            store_rms_level(&samples, &level);
            f32::from_bits(level.load(Ordering::Relaxed))
        };
        assert_eq!(level_for(0.0), 0.0);
        assert!((level_for(0.001) - 0.0).abs() < 1.0e-5);
        assert!((level_for(1.0) - 1.0).abs() < 1.0e-5);
    }
}
