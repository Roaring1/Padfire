//! Native audio output actor.
//!
//! A single dedicated thread owns CPAL/rodio streams and Player handles. The
//! UI/MIDI threads only send bounded commands. When no clips are playing the
//! actor blocks on its receiver and releases all device streams.

use crate::output_meter::OutputLoopbackMeter;
use cpal::traits::{DeviceTrait, HostTrait};
use padfire_core::PadKey;
use padfire_core::audio::{OutputDevice, OutputResolution, resolve_route_pair};
use padfire_core::runtime::{GenerationTracker, OperationToken};
use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Player, Source};
use std::collections::HashMap;
use std::fs::File;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AudioEvent {
    Ready,
    PlaybackStarted { pad: PadKey },
    PlaybackFinished { pad: PadKey },
    RouteFallback { requested: String, selected: String },
    DeviceUnavailable { requested: String },
    PlaybackFailed { pad: PadKey, message: String },
    BackendUnavailable(String),
    Stopped,
}

#[derive(Debug, Clone)]
pub struct PlaybackRequest {
    pub token: OperationToken,
    pub pad: PadKey,
    pub file: String,
    pub sink: String,
    pub volume_percent: u8,
    pub looping: bool,
    pub monitor_sink: Option<String>,
}

enum AudioCommand {
    Play(PlaybackRequest),
    Stop { pad: PadKey },
    StopAll,
    Shutdown,
}

struct ActivePlayers {
    pad: PadKey,
    players: Vec<Player>,
}

pub struct AudioEngine {
    sender: Sender<AudioCommand>,
    worker: Option<JoinHandle<()>>,
    meter: Arc<MeterAccumulator>,
    loopback_meter: OutputLoopbackMeter,
}

impl AudioEngine {
    pub fn spawn(
        generations: Arc<Mutex<GenerationTracker>>,
        events: Sender<AudioEvent>,
    ) -> Result<Self, String> {
        let (sender, receiver) = mpsc::channel();
        let meter = Arc::new(MeterAccumulator::default());
        let worker_meter = meter.clone();
        let worker = thread::Builder::new()
            .name("padfire-audio".to_owned())
            .spawn(move || {
                AudioActor::new(generations, events, worker_meter).run(receiver);
            })
            .map_err(|error| format!("could not start audio worker: {error}"))?;
        Ok(Self {
            sender,
            worker: Some(worker),
            meter,
            loopback_meter: OutputLoopbackMeter::new(),
        })
    }

    pub fn play(&self, request: PlaybackRequest) -> Result<(), String> {
        self.sender
            .send(AudioCommand::Play(request))
            .map_err(|_| "audio worker is not running".to_owned())
    }

    pub fn stop(&self, pad: PadKey) -> Result<(), String> {
        self.sender
            .send(AudioCommand::Stop { pad })
            .map_err(|_| "audio worker is not running".to_owned())
    }

    pub fn stop_all(&self) -> Result<(), String> {
        self.sender
            .send(AudioCommand::StopAll)
            .map_err(|_| "audio worker is not running".to_owned())
    }

    pub fn meter_level(&self) -> f32 {
        // Drain the Padfire-mix fallback on every UI read even while native
        // loopback is healthy, so its atomic sample window cannot accumulate
        // stale energy/counts in the background.
        let mix_level = self.meter.take_level();
        self.loopback_meter.level().unwrap_or(mix_level)
    }

    pub fn set_meter_capture_target(&mut self, target: Option<String>) {
        self.loopback_meter.set_target(target);
    }

    pub fn shutdown(&mut self) {
        self.loopback_meter.shutdown();
        let _ = self.sender.send(AudioCommand::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for AudioEngine {
    fn drop(&mut self) {
        self.shutdown();
    }
}

struct AudioActor {
    generations: Arc<Mutex<GenerationTracker>>,
    events: Sender<AudioEvent>,
    meter: Arc<MeterAccumulator>,
    streams: HashMap<String, MixerDeviceSink>,
    active: HashMap<String, ActivePlayers>,
}

impl AudioActor {
    fn new(
        generations: Arc<Mutex<GenerationTracker>>,
        events: Sender<AudioEvent>,
        meter: Arc<MeterAccumulator>,
    ) -> Self {
        Self {
            generations,
            events,
            meter,
            streams: HashMap::new(),
            active: HashMap::new(),
        }
    }

    fn run(mut self, receiver: Receiver<AudioCommand>) {
        let _ = self.events.send(AudioEvent::Ready);

        loop {
            let message = if self.active.is_empty() {
                receiver.recv().map_err(|_| RecvTimeoutError::Disconnected)
            } else {
                receiver.recv_timeout(Duration::from_millis(100))
            };

            match message {
                Ok(AudioCommand::Play(request)) => self.start_playback(request),
                Ok(AudioCommand::Stop { pad }) => self.stop_pad(pad),
                Ok(AudioCommand::StopAll) => self.stop_everything(),
                Ok(AudioCommand::Shutdown) | Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) => self.reap_finished(),
            }
        }

        self.stop_everything();
        let _ = self.events.send(AudioEvent::Stopped);
    }

    fn is_current(&self, token: &OperationToken) -> bool {
        self.generations
            .lock()
            .map(|tracker| tracker.is_current(token))
            .unwrap_or(false)
    }

    fn start_playback(&mut self, request: PlaybackRequest) {
        if !self.is_current(&request.token) {
            return;
        }

        let endpoints = match enumerate_output_devices() {
            Ok(endpoints) => endpoints,
            Err(error) => {
                self.fail(request.pad, error);
                return;
            }
        };
        let routes = resolve_route_pair(
            &request.sink,
            request.monitor_sink.is_some(),
            request.monitor_sink.as_deref().unwrap_or_default(),
            &endpoints,
        );

        let primary = match &routes.primary {
            OutputResolution::Default { device } => device.clone(),
            OutputResolution::Matched { device, .. } => Some(device.clone()),
            OutputResolution::Missing {
                requested,
                fallback,
            } => {
                let _ = self.events.send(AudioEvent::DeviceUnavailable {
                    requested: requested.clone(),
                });
                fallback.clone()
            }
        };

        let Some(primary) = primary else {
            self.fail(
                request.pad,
                "no usable audio output device is available".to_owned(),
            );
            return;
        };

        if let OutputResolution::Missing { requested, .. } = &routes.primary {
            let _ = self.events.send(AudioEvent::RouteFallback {
                requested: requested.clone(),
                selected: primary.name.clone(),
            });
        }

        let monitor = if routes.separate_monitor_stream {
            routes
                .monitor
                .as_ref()
                .and_then(OutputResolution::device)
                .cloned()
        } else {
            None
        };

        if let Some(OutputResolution::Missing {
            requested,
            fallback,
        }) = routes.monitor.as_ref()
        {
            let selected = fallback
                .as_ref()
                .map(|device| device.name.clone())
                .unwrap_or_default();
            let _ = self.events.send(AudioEvent::RouteFallback {
                requested: requested.clone(),
                selected,
            });
        }

        // Decode before creating a stream. Stop may arrive while decoding, so
        // the generation is rechecked before opening any endpoint.
        let primary_file = match File::open(&request.file) {
            Ok(file) => file,
            Err(error) => {
                self.fail(request.pad, format!("could not open sound file: {error}"));
                return;
            }
        };
        let primary_decoder = if request.looping {
            match Decoder::new_looped(std::io::BufReader::new(primary_file)) {
                Ok(decoder) => DecodedSource::Looped(decoder),
                Err(error) => {
                    self.fail(request.pad, format!("could not decode sound file: {error}"));
                    return;
                }
            }
        } else {
            match Decoder::try_from(primary_file) {
                Ok(decoder) => DecodedSource::Single(decoder),
                Err(error) => {
                    self.fail(request.pad, format!("could not decode sound file: {error}"));
                    return;
                }
            }
        };

        let monitor_decoder = if monitor.is_some() {
            let monitor_file = match File::open(&request.file) {
                Ok(file) => file,
                Err(error) => {
                    self.fail(
                        request.pad,
                        format!("could not reopen sound for monitor: {error}"),
                    );
                    return;
                }
            };
            if request.looping {
                match Decoder::new_looped(std::io::BufReader::new(monitor_file)) {
                    Ok(decoder) => Some(DecodedSource::Looped(decoder)),
                    Err(error) => {
                        self.fail(
                            request.pad,
                            format!("could not decode monitor sound: {error}"),
                        );
                        return;
                    }
                }
            } else {
                match Decoder::try_from(monitor_file) {
                    Ok(decoder) => Some(DecodedSource::Single(decoder)),
                    Err(error) => {
                        self.fail(
                            request.pad,
                            format!("could not decode monitor sound: {error}"),
                        );
                        return;
                    }
                }
            }
        } else {
            None
        };

        if !self.is_current(&request.token) {
            return;
        }

        if let Err(error) = self.ensure_stream(&primary.id) {
            self.fail(
                request.pad,
                format!("could not open output '{}': {error}", primary.name),
            );
            return;
        }

        if let Some(monitor_device) = &monitor
            && let Err(error) = self.ensure_stream(&monitor_device.id)
        {
            self.fail(
                request.pad,
                format!(
                    "could not open monitor output '{}': {error}",
                    monitor_device.name
                ),
            );
            return;
        }

        // A concurrent Stop can invalidate the token while a native device
        // stream is opening. Do not publish players for a cancelled request.
        if !self.is_current(&request.token) {
            self.release_idle_streams();
            return;
        }

        self.cancel_previous_pad(request.pad);
        let mut players = Vec::with_capacity(if monitor.is_some() { 2 } else { 1 });

        if let Some(stream) = self.streams.get(&primary.id) {
            let player = Player::connect_new(stream.mixer());
            player.set_volume(f32::from(request.volume_percent.min(150)) / 100.0);
            primary_decoder.append_to(
                &player,
                Some(self.meter.clone()),
                f32::from(request.volume_percent.min(150)) / 100.0,
            );
            players.push(player);
        }

        if let (Some(monitor_device), Some(decoder)) = (&monitor, monitor_decoder)
            && let Some(stream) = self.streams.get(&monitor_device.id)
        {
            let player = Player::connect_new(stream.mixer());
            player.set_volume(f32::from(request.volume_percent.min(150)) / 100.0);
            decoder.append_to(&player, None, 1.0);
            players.push(player);
        }

        let pad_key = request.pad.to_key();
        self.active.insert(
            pad_key,
            ActivePlayers {
                pad: request.pad,
                players,
            },
        );
        let _ = self
            .events
            .send(AudioEvent::PlaybackStarted { pad: request.pad });
    }

    fn ensure_stream(&mut self, device_id: &str) -> Result<(), String> {
        if self.streams.contains_key(device_id) {
            return Ok(());
        }

        let host = selected_host()?;
        let mut devices = host.output_devices().map_err(|error| error.to_string())?;
        let device = devices
            .find(|device| device.id().is_ok_and(|id| id.to_string() == device_id))
            .ok_or_else(|| "device disappeared during stream creation".to_owned())?;

        let mut stream = DeviceSinkBuilder::from_device(device)
            .map_err(|error| error.to_string())?
            .open_stream()
            .map_err(|error| error.to_string())?;
        stream.log_on_drop(false);
        self.streams.insert(device_id.to_owned(), stream);
        Ok(())
    }

    fn cancel_previous_pad(&mut self, pad: PadKey) {
        if let Some(active) = self.active.remove(&pad.to_key()) {
            for player in active.players {
                player.stop();
            }
            let _ = self.events.send(AudioEvent::PlaybackFinished { pad });
        }
    }

    fn stop_pad(&mut self, pad: PadKey) {
        self.cancel_previous_pad(pad);
        self.release_idle_streams();
    }

    fn stop_everything(&mut self) {
        for (_, active) in self.active.drain() {
            for player in active.players {
                player.stop();
            }
            let _ = self
                .events
                .send(AudioEvent::PlaybackFinished { pad: active.pad });
        }
        self.streams.clear();
    }

    fn reap_finished(&mut self) {
        let finished: Vec<(String, PadKey)> = self
            .active
            .iter()
            .filter(|(_, active)| active.players.iter().all(Player::empty))
            .map(|(key, active)| (key.clone(), active.pad))
            .collect();

        for (key, pad) in finished {
            self.active.remove(&key);
            let _ = self.events.send(AudioEvent::PlaybackFinished { pad });
        }
        self.release_idle_streams();
    }

    fn release_idle_streams(&mut self) {
        if self.active.is_empty() {
            self.streams.clear();
        }
    }

    fn fail(&self, pad: PadKey, message: String) {
        let _ = self
            .events
            .send(AudioEvent::PlaybackFailed { pad, message });
    }
}

#[derive(Default)]
struct MeterAccumulator {
    // One atomic word keeps energy and sample count from different windows
    // from being accidentally paired by the UI reader.
    window: AtomicU64,
}

impl MeterAccumulator {
    fn add(&self, sum_squares: f64, sample_count: u32) {
        const ENERGY_SCALE: f64 = 1_000.0;
        let energy = (sum_squares * ENERGY_SCALE)
            .round()
            .clamp(0.0, f64::from(u32::MAX)) as u32;
        let mut current = self.window.load(Ordering::Relaxed);
        loop {
            let old_energy = (current >> 32) as u32;
            let old_count = current as u32;
            let next = (u64::from(old_energy.saturating_add(energy)) << 32)
                | u64::from(old_count.saturating_add(sample_count));
            match self.window.compare_exchange_weak(
                current,
                next,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(actual) => current = actual,
            }
        }
    }

    fn take_level(&self) -> f32 {
        let snapshot = self.window.swap(0, Ordering::AcqRel);
        let energy = (snapshot >> 32) as u32;
        let count = snapshot as u32;
        if count == 0 {
            return 0.0;
        }

        let mean_square = (f64::from(energy) / 1_000.0) / f64::from(count);
        let rms = mean_square.sqrt();
        let db = if rms > 1.0e-7 {
            20.0 * rms.log10()
        } else {
            -96.0
        };
        ((db + 60.0) / 60.0).clamp(0.0, 1.0) as f32
    }
}

struct MeterSource<S> {
    source: S,
    meter: Arc<MeterAccumulator>,
    volume_scale: f64,
    sum_squares: f64,
    sample_count: u32,
}

impl<S> MeterSource<S> {
    fn new(source: S, meter: Arc<MeterAccumulator>, volume_scale: f32) -> Self {
        Self {
            source,
            meter,
            volume_scale: f64::from(volume_scale),
            sum_squares: 0.0,
            sample_count: 0,
        }
    }

    fn flush(&mut self) {
        if self.sample_count == 0 {
            return;
        }
        self.meter.add(self.sum_squares, self.sample_count);
        self.sum_squares = 0.0;
        self.sample_count = 0;
    }
}

impl<S: Source<Item = f32>> Iterator for MeterSource<S> {
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        let Some(sample) = self.source.next() else {
            self.flush();
            return None;
        };
        if sample.is_finite() {
            let scaled = f64::from(sample) * self.volume_scale;
            self.sum_squares += scaled * scaled;
            self.sample_count += 1;
            if self.sample_count >= 256 {
                self.flush();
            }
        }
        Some(sample)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.source.size_hint()
    }
}

impl<S: Source<Item = f32>> Source for MeterSource<S> {
    fn current_span_len(&self) -> Option<usize> {
        self.source.current_span_len()
    }

    fn channels(&self) -> rodio::ChannelCount {
        self.source.channels()
    }

    fn sample_rate(&self) -> rodio::SampleRate {
        self.source.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.source.total_duration()
    }
}

fn append_decoded<S>(
    source: S,
    player: &Player,
    meter: Option<Arc<MeterAccumulator>>,
    volume_scale: f32,
) where
    S: Source<Item = f32> + Send + 'static,
{
    if let Some(meter) = meter {
        player.append(MeterSource::new(source, meter, volume_scale));
    } else {
        player.append(source);
    }
}

enum DecodedSource {
    Single(Decoder<std::io::BufReader<File>>),
    Looped(rodio::decoder::LoopedDecoder<std::io::BufReader<File>>),
}

impl DecodedSource {
    fn append_to(self, player: &Player, meter: Option<Arc<MeterAccumulator>>, volume_scale: f32) {
        match self {
            Self::Single(source) => append_decoded(source, player, meter, volume_scale),
            Self::Looped(source) => append_decoded(source, player, meter, volume_scale),
        }
    }
}

fn selected_host() -> Result<cpal::Host, String> {
    #[cfg(target_os = "linux")]
    {
        // Preserve the released Linux behavior: select PipeWire's supported
        // PulseAudio compatibility host so named sink identities stay usable.
        let pulse = cpal::available_hosts()
            .into_iter()
            .find(|id| id.to_string().to_ascii_lowercase().contains("pulseaudio"));
        if let Some(id) = pulse {
            return cpal::host_from_id(id).map_err(|error| error.to_string());
        }
    }
    Ok(cpal::default_host())
}

pub fn enumerate_output_devices() -> Result<Vec<OutputDevice>, String> {
    let host = selected_host()?;
    let default_id = host
        .default_output_device()
        .and_then(|device| device.id().ok())
        .map(|id| id.to_string());

    let devices = host.output_devices().map_err(|error| error.to_string())?;
    let mut results = Vec::new();
    for device in devices {
        let id = match device.id() {
            Ok(id) => id.to_string(),
            Err(_) => continue,
        };
        let name = device
            .description()
            .map(|description| description.name().to_owned())
            .unwrap_or_else(|_| device.to_string());
        results.push(OutputDevice {
            is_default: default_id.as_deref() == Some(id.as_str()),
            id,
            name,
        });
    }
    Ok(results)
}

pub fn audio_backend_name() -> String {
    selected_host()
        .map(|host| host.id().to_string())
        .unwrap_or_else(|error| format!("Unavailable: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn measure_sine_level(volume_scale: f32) -> f32 {
        let meter = Arc::new(MeterAccumulator::default());
        let mut source = MeterSource::new(
            rodio::source::SineWave::new(440.0),
            meter.clone(),
            volume_scale,
        );
        for _ in 0..1024 {
            let _ = source.next();
        }
        meter.take_level()
    }

    #[test]
    fn decoded_source_types_remain_distinct_for_loop_and_single_playback() {
        let _: fn(DecodedSource, &Player, Option<Arc<MeterAccumulator>>, f32) =
            DecodedSource::append_to;
        assert!(!audio_backend_name().is_empty());
    }

    #[test]
    fn meter_tracks_playback_rms_and_accounts_for_pad_volume() {
        let full_volume = measure_sine_level(1.0);
        let half_volume = measure_sine_level(0.5);
        assert!(full_volume > 0.0);
        assert!(half_volume > 0.0);
        assert!(full_volume > half_volume);
    }

    #[test]
    fn meter_aggregates_multiple_sources_and_resets_when_idle() {
        let meter = Arc::new(MeterAccumulator::default());
        for _ in 0..2 {
            let mut source =
                MeterSource::new(rodio::source::SineWave::new(440.0), meter.clone(), 1.0);
            for _ in 0..1024 {
                let _ = source.next();
            }
        }
        assert!(meter.take_level() > 0.0);
        assert_eq!(meter.take_level(), 0.0);
    }
}
