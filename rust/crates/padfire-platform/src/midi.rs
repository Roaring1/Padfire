//! Launchpad Mini input/output adapter.
//!
//! Discovery is read-only. Opening the MIDI output and sending LEDs occurs
//! only after the application explicitly selects a supported MK1/MK2 pair.
//! Every output write is serialized by one mutex.

use midir::{Ignore, MidiInput, MidiInputConnection, MidiOutput, MidiOutputConnection};
use padfire_core::{
    Control, LED_OFF, MIDI_PAGE_CC_FIRST, MIDI_PAGE_CC_LAST, MidiEvent, MidiPacket, decode_midi,
    is_supported_launchpad_port, led_velocity,
};
use std::fmt;
use std::sync::Mutex;
use std::sync::mpsc::{self, Receiver, Sender, SyncSender, TrySendError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MidiEndpoint {
    pub name: String,
    pub input: bool,
    pub output: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MidiStatus {
    Unavailable(String),
    NoSupportedDevice,
    Available(Vec<MidiEndpoint>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MidiSelection {
    pub input_name: String,
    pub output_name: String,
}

#[derive(Debug)]
pub enum MidiBackendError {
    Backend(String),
    NotFound(String),
    Unsupported(String),
    Ambiguous(String),
}

impl fmt::Display for MidiBackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Backend(error) => write!(f, "MIDI backend error: {error}"),
            Self::NotFound(name) => write!(f, "Launchpad endpoint is no longer available: {name}"),
            Self::Unsupported(name) => write!(f, "unsupported Launchpad protocol: {name}"),
            Self::Ambiguous(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for MidiBackendError {}

/// Enumerate only supported Launchpad Mini endpoints. This opens the MIDI
/// subsystem for enumeration but does not create a connection or send output.
pub fn discover() -> MidiStatus {
    let input = match MidiInput::new("Padfire Discovery Input") {
        Ok(input) => input,
        Err(error) => return MidiStatus::Unavailable(error.to_string()),
    };
    let output = match MidiOutput::new("Padfire Discovery Output") {
        Ok(output) => output,
        Err(error) => return MidiStatus::Unavailable(error.to_string()),
    };

    let mut endpoints: Vec<MidiEndpoint> = Vec::new();

    for port in input.ports() {
        if let Ok(name) = input.port_name(&port)
            && is_supported_launchpad_port(&name)
        {
            merge_endpoint(&mut endpoints, name, true, false);
        }
    }
    for port in output.ports() {
        if let Ok(name) = output.port_name(&port)
            && is_supported_launchpad_port(&name)
        {
            merge_endpoint(&mut endpoints, name, false, true);
        }
    }

    if endpoints.is_empty() {
        MidiStatus::NoSupportedDevice
    } else {
        MidiStatus::Available(endpoints)
    }
}

fn merge_endpoint(endpoints: &mut Vec<MidiEndpoint>, name: String, input: bool, output: bool) {
    if let Some(existing) = endpoints
        .iter_mut()
        .find(|endpoint| endpoint.name.eq_ignore_ascii_case(&name))
    {
        existing.input |= input;
        existing.output |= output;
    } else {
        endpoints.push(MidiEndpoint {
            name,
            input,
            output,
        });
    }
}

/// Pair input/output ports conservatively. Prefer exactly one common endpoint;
/// allow distinct endpoint labels only when there is exactly one supported
/// input and one supported output. Multiple controllers are never guessed.
pub fn select_unambiguous_pair(
    inputs: &[String],
    outputs: &[String],
) -> Result<Option<MidiSelection>, MidiBackendError> {
    let supported_inputs: Vec<&String> = inputs
        .iter()
        .filter(|name| is_supported_launchpad_port(name))
        .collect();
    let supported_outputs: Vec<&String> = outputs
        .iter()
        .filter(|name| is_supported_launchpad_port(name))
        .collect();

    if supported_inputs.is_empty() || supported_outputs.is_empty() {
        return Ok(None);
    }

    let mut common = Vec::new();
    for input in &supported_inputs {
        for output in &supported_outputs {
            if input.eq_ignore_ascii_case(output) {
                common.push(((*input).clone(), (*output).clone()));
            }
        }
    }

    if common.len() == 1 {
        let (input_name, output_name) = common.remove(0);
        return Ok(Some(MidiSelection {
            input_name,
            output_name,
        }));
    }

    if supported_inputs.len() == 1 && supported_outputs.len() == 1 {
        return Ok(Some(MidiSelection {
            input_name: supported_inputs[0].to_string(),
            output_name: supported_outputs[0].to_string(),
        }));
    }

    Err(MidiBackendError::Ambiguous(
        "multiple supported Launchpad ports are present; select the controller explicitly"
            .to_owned(),
    ))
}

pub fn parse_midi_packet(message: &[u8]) -> Option<MidiPacket> {
    let (&status, data) = message.split_first()?;
    let channel_status = status & 0xf0;
    match channel_status {
        0x90 if data.len() >= 2 => Some(MidiPacket::NoteOn {
            note: data[0] & 0x7f,
            velocity: data[1] & 0x7f,
        }),
        0x80 if data.len() >= 2 => Some(MidiPacket::NoteOff {
            note: data[0] & 0x7f,
        }),
        0xb0 if data.len() >= 2 => Some(MidiPacket::ControlChange {
            controller: data[0] & 0x7f,
            value: data[1] & 0x7f,
        }),
        _ => None,
    }
}

pub struct ControllerConnection {
    _input: MidiInputConnection<()>,
    output: Mutex<MidiOutputConnection>,
    pub input_name: String,
    pub output_name: String,
}

impl ControllerConnection {
    /// Open exactly the selected supported input/output pair. No LED traffic
    /// is sent automatically during connection.
    pub fn open(
        selection: &MidiSelection,
        event_sender: SyncSender<MidiEvent>,
    ) -> Result<Self, MidiBackendError> {
        if !is_supported_launchpad_port(&selection.input_name)
            || !is_supported_launchpad_port(&selection.output_name)
        {
            return Err(MidiBackendError::Unsupported(format!(
                "{} / {}",
                selection.input_name, selection.output_name
            )));
        }

        let mut midi_input = MidiInput::new("Padfire Launchpad Input")
            .map_err(|error| MidiBackendError::Backend(error.to_string()))?;
        midi_input.ignore(Ignore::None);
        let input_port = midi_input
            .ports()
            .into_iter()
            .find(|port| {
                midi_input
                    .port_name(port)
                    .is_ok_and(|name| name.eq_ignore_ascii_case(&selection.input_name))
            })
            .ok_or_else(|| MidiBackendError::NotFound(selection.input_name.clone()))?;

        let midi_output = MidiOutput::new("Padfire Launchpad Output")
            .map_err(|error| MidiBackendError::Backend(error.to_string()))?;
        let output_port = midi_output
            .ports()
            .into_iter()
            .find(|port| {
                midi_output
                    .port_name(port)
                    .is_ok_and(|name| name.eq_ignore_ascii_case(&selection.output_name))
            })
            .ok_or_else(|| MidiBackendError::NotFound(selection.output_name.clone()))?;

        let input_connection = midi_input
            .connect(
                &input_port,
                "Padfire Launchpad Input",
                move |_timestamp, message, _state| {
                    if let Some(packet) = parse_midi_packet(message) {
                        let event = decode_midi(packet);
                        if event != MidiEvent::Ignored {
                            let _ = event_sender.try_send(event);
                        }
                    }
                },
                (),
            )
            .map_err(|error| MidiBackendError::Backend(error.to_string()))?;

        // If opening output fails, the just-created input connection drops and
        // closes cleanly. Nothing has been written to the controller yet.
        let output_connection = midi_output
            .connect(&output_port, "Padfire Launchpad Output")
            .map_err(|error| MidiBackendError::Backend(error.to_string()))?;

        Ok(Self {
            _input: input_connection,
            output: Mutex::new(output_connection),
            input_name: selection.input_name.clone(),
            output_name: selection.output_name.clone(),
        })
    }

    pub fn set_led(&self, control: Control, red: u8, green: u8) -> Result<(), MidiBackendError> {
        self.set_led_velocity(control, led_velocity(red, green))
    }

    pub fn set_led_velocity(&self, control: Control, velocity: u8) -> Result<(), MidiBackendError> {
        self.output
            .lock()
            .map_err(|_| MidiBackendError::Backend("MIDI output lock is poisoned".to_owned()))?
            .send(&[0x90, control.note(), velocity])
            .map_err(|error| MidiBackendError::Backend(error.to_string()))
    }

    pub fn set_page_led(&self, page: u8, velocity: u8) -> Result<(), MidiBackendError> {
        if page >= 8 {
            return Err(MidiBackendError::Backend(
                "page index must be between 0 and 7".to_owned(),
            ));
        }
        let controller = MIDI_PAGE_CC_FIRST + page;
        self.output
            .lock()
            .map_err(|_| MidiBackendError::Backend("MIDI output lock is poisoned".to_owned()))?
            .send(&[0xb0, controller, velocity])
            .map_err(|error| MidiBackendError::Backend(error.to_string()))
    }

    /// Explicit shutdown helper. Call only when the app is quitting or the
    /// user explicitly requests a full clear; this emits 72 note LED messages.
    pub fn clear_all_leds(&self) -> Result<(), MidiBackendError> {
        let mut output = self
            .output
            .lock()
            .map_err(|_| MidiBackendError::Backend("MIDI output lock is poisoned".to_owned()))?;
        for row in 0..8 {
            for column in 0..8 {
                let note = row * 16 + column;
                output
                    .send(&[0x90, note, LED_OFF])
                    .map_err(|error| MidiBackendError::Backend(error.to_string()))?;
            }
            let side = row * 16 + 8;
            output
                .send(&[0x90, side, LED_OFF])
                .map_err(|error| MidiBackendError::Backend(error.to_string()))?;
        }
        for controller in MIDI_PAGE_CC_FIRST..=MIDI_PAGE_CC_LAST {
            output
                .send(&[0xb0, controller, LED_OFF])
                .map_err(|error| MidiBackendError::Backend(error.to_string()))?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MidiConnectionState {
    Connected { input: String, output: String },
    NoSupportedDevice,
    Disconnected,
    Unavailable(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MidiCommand {
    SetPad(Control, u8),
    SetPage(u8, u8),
    ClearAll,
    Shutdown,
}

/// Owns Launchpad connections and reconnects after device removal. UI and
/// audio callbacks never call midir directly; they submit bounded LED updates.
pub struct MidiSupervisor {
    commands: SyncSender<MidiCommand>,
    worker: Option<JoinHandle<()>>,
}

impl MidiSupervisor {
    pub fn spawn(
        midi_events: SyncSender<MidiEvent>,
        status_events: Sender<MidiConnectionState>,
    ) -> Result<Self, String> {
        let (commands, receiver) = mpsc::sync_channel(256);
        let disable_hardware =
            std::env::var_os("PADFIRE_DISABLE_HARDWARE").is_some_and(|value| value == "1");
        let worker = thread::Builder::new()
            .name("padfire-midi-supervisor".to_owned())
            .spawn(move || {
                midi_supervisor_loop(receiver, midi_events, status_events, disable_hardware)
            })
            .map_err(|error| format!("could not start MIDI supervisor: {error}"))?;
        Ok(Self {
            commands,
            worker: Some(worker),
        })
    }

    pub fn set_led(&self, control: Control, velocity: u8) -> Result<(), String> {
        self.enqueue(MidiCommand::SetPad(control, velocity))
    }

    pub fn set_page_led(&self, page: u8, velocity: u8) -> Result<(), String> {
        if page >= 8 {
            return Err("page index must be between 0 and 7".to_owned());
        }
        self.enqueue(MidiCommand::SetPage(page, velocity))
    }

    pub fn clear_all_leds(&self) -> Result<(), String> {
        self.enqueue(MidiCommand::ClearAll)
    }

    fn enqueue(&self, command: MidiCommand) -> Result<(), String> {
        self.commands
            .try_send(command)
            .map_err(|error| match error {
                TrySendError::Full(_) => "MIDI output queue is full".to_owned(),
                TrySendError::Disconnected(_) => "MIDI supervisor is not running".to_owned(),
            })
    }

    pub fn shutdown(&mut self) {
        let _ = self.commands.send(MidiCommand::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for MidiSupervisor {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn midi_supervisor_loop(
    receiver: Receiver<MidiCommand>,
    midi_events: SyncSender<MidiEvent>,
    status_events: Sender<MidiConnectionState>,
    disable_hardware: bool,
) {
    let mut connection: Option<ControllerConnection> = None;
    let mut selected: Option<MidiSelection> = None;
    let mut reported: Option<MidiConnectionState> = None;
    let mut next_discovery = Instant::now();

    if disable_hardware {
        report_midi_state(
            &mut reported,
            &status_events,
            MidiConnectionState::NoSupportedDevice,
        );
    }

    loop {
        if !disable_hardware && connection.is_none() && Instant::now() >= next_discovery {
            next_discovery = Instant::now() + Duration::from_secs(1);
            match find_supported_pair() {
                Ok(Some(selection)) => {
                    match ControllerConnection::open(&selection, midi_events.clone()) {
                        Ok(opened) => {
                            let state = MidiConnectionState::Connected {
                                input: opened.input_name.clone(),
                                output: opened.output_name.clone(),
                            };
                            connection = Some(opened);
                            selected = Some(selection);
                            report_midi_state(&mut reported, &status_events, state);
                        }
                        Err(error) => {
                            report_midi_state(
                                &mut reported,
                                &status_events,
                                MidiConnectionState::Unavailable(error.to_string()),
                            );
                        }
                    }
                }
                Ok(None) => {
                    report_midi_state(
                        &mut reported,
                        &status_events,
                        MidiConnectionState::NoSupportedDevice,
                    );
                }
                Err(error) => {
                    report_midi_state(
                        &mut reported,
                        &status_events,
                        MidiConnectionState::Unavailable(error.to_string()),
                    );
                }
            }
        }

        match receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(MidiCommand::SetPad(control, velocity)) => {
                if let Some(opened) = &connection
                    && let Err(error) = opened.set_led_velocity(control, velocity)
                {
                    connection = None;
                    selected = None;
                    next_discovery = Instant::now() + Duration::from_millis(250);
                    report_midi_state(
                        &mut reported,
                        &status_events,
                        MidiConnectionState::Unavailable(error.to_string()),
                    );
                }
            }
            Ok(MidiCommand::SetPage(page, velocity)) => {
                if let Some(opened) = &connection
                    && let Err(error) = opened.set_page_led(page, velocity)
                {
                    connection = None;
                    selected = None;
                    next_discovery = Instant::now() + Duration::from_millis(250);
                    report_midi_state(
                        &mut reported,
                        &status_events,
                        MidiConnectionState::Unavailable(error.to_string()),
                    );
                }
            }
            Ok(MidiCommand::ClearAll) => {
                if let Some(opened) = &connection
                    && let Err(error) = opened.clear_all_leds()
                {
                    connection = None;
                    selected = None;
                    next_discovery = Instant::now() + Duration::from_millis(250);
                    report_midi_state(
                        &mut reported,
                        &status_events,
                        MidiConnectionState::Unavailable(error.to_string()),
                    );
                }
            }
            Ok(MidiCommand::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }

        // Re-enumerate without output traffic so unplugging the controller is
        // reflected even when the user is not triggering pads.
        if !disable_hardware && connection.is_some() && Instant::now() >= next_discovery {
            next_discovery = Instant::now() + Duration::from_secs(1);
            let still_present =
                selected
                    .as_ref()
                    .is_some_and(|selection| match find_supported_pair() {
                        Ok(Some(current)) => {
                            current
                                .input_name
                                .eq_ignore_ascii_case(&selection.input_name)
                                && current
                                    .output_name
                                    .eq_ignore_ascii_case(&selection.output_name)
                        }
                        _ => false,
                    });
            if !still_present {
                connection = None;
                selected = None;
                report_midi_state(
                    &mut reported,
                    &status_events,
                    MidiConnectionState::Disconnected,
                );
                next_discovery = Instant::now() + Duration::from_millis(250);
            }
        }
    }

    // Do not send LED reset traffic automatically on disconnect/shutdown.
    drop(connection);
    report_midi_state(
        &mut reported,
        &status_events,
        MidiConnectionState::Disconnected,
    );
}

fn report_midi_state(
    previous: &mut Option<MidiConnectionState>,
    events: &Sender<MidiConnectionState>,
    next: MidiConnectionState,
) {
    if previous.as_ref() != Some(&next) {
        *previous = Some(next.clone());
        let _ = events.send(next);
    }
}

fn find_supported_pair() -> Result<Option<MidiSelection>, MidiBackendError> {
    let input = MidiInput::new("Padfire Pair Discovery Input")
        .map_err(|error| MidiBackendError::Backend(error.to_string()))?;
    let output = MidiOutput::new("Padfire Pair Discovery Output")
        .map_err(|error| MidiBackendError::Backend(error.to_string()))?;
    let inputs: Vec<String> = input
        .ports()
        .into_iter()
        .filter_map(|port| input.port_name(&port).ok())
        .collect();
    let outputs: Vec<String> = output
        .ports()
        .into_iter()
        .filter_map(|port| output.port_name(&port).ok())
        .collect();
    select_unambiguous_pair(&inputs, &outputs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn midi_parser_handles_note_on_zero_velocity_note_off_and_page_cc() {
        assert_eq!(
            parse_midi_packet(&[0x90, 17, 100]),
            Some(MidiPacket::NoteOn {
                note: 17,
                velocity: 100
            })
        );
        assert_eq!(
            parse_midi_packet(&[0x90, 17, 0]),
            Some(MidiPacket::NoteOn {
                note: 17,
                velocity: 0
            })
        );
        assert_eq!(
            parse_midi_packet(&[0x82, 8, 64]),
            Some(MidiPacket::NoteOff { note: 8 })
        );
        assert_eq!(
            parse_midi_packet(&[0xb0, 104, 127]),
            Some(MidiPacket::ControlChange {
                controller: 104,
                value: 127
            })
        );
        assert_eq!(parse_midi_packet(&[0xc0, 0]), None);
        assert_eq!(parse_midi_packet(&[0x90, 17]), None);
    }

    #[test]
    fn endpoint_discovery_logic_excludes_mk3() {
        let inputs = vec![
            "Launchpad Mini MK3 MIDI 1".to_owned(),
            "Launchpad Mini MK2 MIDI 1".to_owned(),
        ];
        let outputs = vec!["Launchpad Mini MK2 MIDI 1".to_owned()];
        let pair = select_unambiguous_pair(&inputs, &outputs).unwrap().unwrap();
        assert_eq!(pair.input_name, "Launchpad Mini MK2 MIDI 1");
        assert_eq!(pair.output_name, "Launchpad Mini MK2 MIDI 1");
    }

    #[test]
    fn unique_input_output_can_have_distinct_backend_labels() {
        let inputs = vec!["Launchpad Mini In".to_owned()];
        let outputs = vec!["Launchpad Mini Out".to_owned()];
        assert_eq!(
            select_unambiguous_pair(&inputs, &outputs).unwrap(),
            Some(MidiSelection {
                input_name: "Launchpad Mini In".to_owned(),
                output_name: "Launchpad Mini Out".to_owned(),
            })
        );
    }

    #[test]
    fn multiple_controller_pairs_are_not_guessed() {
        let inputs = vec![
            "Launchpad Mini MK1 A".to_owned(),
            "Launchpad Mini MK2 B".to_owned(),
        ];
        let outputs = vec![
            "Launchpad Mini MK1 A".to_owned(),
            "Launchpad Mini MK2 B".to_owned(),
        ];
        assert!(matches!(
            select_unambiguous_pair(&inputs, &outputs),
            Err(MidiBackendError::Ambiguous(_))
        ));
    }

    #[test]
    fn mk3_never_forms_a_pair() {
        let inputs = vec!["Launchpad Mini MK3 MIDI".to_owned()];
        let outputs = vec!["Launchpad Mini MK3 MIDI".to_owned()];
        assert_eq!(select_unambiguous_pair(&inputs, &outputs).unwrap(), None);
    }
}
