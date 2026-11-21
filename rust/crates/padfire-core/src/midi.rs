//! Launchpad Mini MK1/MK2 protocol decoding and LED invariants.

pub const MIDI_PAGE_CC_FIRST: u8 = 104;
pub const MIDI_PAGE_CC_LAST: u8 = 111;
pub const SIDE_COLUMN: u8 = 8;

pub const LED_OFF: u8 = 0x0C;
pub const LED_DIM: u8 = 0x1C;
pub const LED_GREEN: u8 = 0x3C;
pub const LED_RED: u8 = 0x0F;
pub const LED_ORANGE: u8 = 0x1F;
pub const LED_AMBER: u8 = 0x3F;
pub const LED_PG_HAS: u8 = 0x0D;

/// The physical control represented by a supported Launchpad note.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Control {
    Grid { row: u8, column: u8 },
    Side { row: u8 },
}

impl Control {
    pub fn from_note(note: u8) -> Option<Self> {
        let row = note / 16;
        let column = note % 16;
        if row >= 8 {
            return None;
        }
        match column {
            0..=7 => Some(Self::Grid { row, column }),
            SIDE_COLUMN => Some(Self::Side { row }),
            _ => None,
        }
    }

    pub fn note(self) -> u8 {
        match self {
            Self::Grid { row, column } => row * 16 + column,
            Self::Side { row } => row * 16 + SIDE_COLUMN,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MidiPacket {
    NoteOn { note: u8, velocity: u8 },
    NoteOff { note: u8 },
    ControlChange { controller: u8, value: u8 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MidiEvent {
    PadPressed(Control),
    PadReleased(Control),
    PageSelected(u8),
    Ignored,
}

/// Convert Mini MK1/MK2 MIDI messages into UI-neutral events. A velocity-zero
/// NoteOn is a release, as required by common MIDI devices.
pub fn decode_midi(packet: MidiPacket) -> MidiEvent {
    match packet {
        MidiPacket::NoteOn { note, velocity } => {
            let Some(control) = Control::from_note(note) else {
                return MidiEvent::Ignored;
            };
            if velocity == 0 {
                MidiEvent::PadReleased(control)
            } else {
                MidiEvent::PadPressed(control)
            }
        }
        MidiPacket::NoteOff { note } => Control::from_note(note)
            .map(MidiEvent::PadReleased)
            .unwrap_or(MidiEvent::Ignored),
        MidiPacket::ControlChange { controller, value }
            if (MIDI_PAGE_CC_FIRST..=MIDI_PAGE_CC_LAST).contains(&controller) && value > 0 =>
        {
            MidiEvent::PageSelected(controller - MIDI_PAGE_CC_FIRST)
        }
        _ => MidiEvent::Ignored,
    }
}

/// Reject the Mini MK3 explicitly; its MIDI/LED protocol is not compatible.
pub fn is_supported_launchpad_port(name: &str) -> bool {
    let normalized = name.to_ascii_lowercase();
    normalized.contains("launchpad mini") && !normalized.contains("mk3")
}

/// Encode two-bit red/green LED values. The 0x0c bits are mandatory
/// double-buffer flags; omitting them causes visible flicker.
pub const fn led_velocity(red: u8, green: u8) -> u8 {
    ((green & 0x03) << 4) | (red & 0x03) | 0x0c
}

/// A page/note key used by stop-pad assignments and IPC status payloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PadKey {
    pub page: u8,
    pub note: u8,
}

impl PadKey {
    pub fn new(page: u8, note: u8) -> Self {
        Self { page, note }
    }

    pub fn parse_legacy(value: &str) -> Option<Self> {
        // Older configs briefly stored a display label in the stop key.
        let raw = value.split("  (").next().unwrap_or(value).trim();
        let (page, note) = raw.split_once(':')?;
        let key = Self {
            page: page.parse().ok()?,
            note: note.parse().ok()?,
        };
        (key.page < 8 && Control::from_note(key.note).is_some()).then_some(key)
    }

    pub fn to_key(self) -> String {
        format!("{}:{}", self.page, self.note)
    }
}
