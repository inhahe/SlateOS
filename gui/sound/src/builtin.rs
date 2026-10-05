//! The sounds played for a theme that brings none of its own: short chimes
//! and clicks, synthesized here rather than shipped as files.
//!
//! # Why synthesized
//!
//! A fresh system has no sound theme installed, and a desktop that makes no
//! sound when a message arrives is one whose notifications go unnoticed.
//! Shipping files would mean choosing, licensing and packaging someone's
//! recordings; a chime is a few sine waves and an envelope, and these are
//! rendered on demand in a millisecond or two. A theme with sounds of its
//! own replaces them event by event.
//!
//! # What they sound like
//!
//! Each tone is a soft bell: a fundamental with three quieter overtones,
//! each fading faster than the one below it, after a three-millisecond rise
//! that keeps the start from clicking. Arrivals rise and departures fall --
//! a message, a device plugged in and logging in go up; an error, a device
//! taken out and logging out come down -- and the recycle bin and the
//! screenshot are noise, shaped like what they name. Every sound peaks at
//! -6 dBFS ([`PEAK`]) and ends in a short fade, so none ends on a click.
//!
//! # Which events
//!
//! By the freedesktop sound naming specification's names ([`BuiltIn::for_event`]),
//! with its fallback: an event with no sound of its own takes the sound of
//! its name cut at the last hyphen -- `dialog-error-serious` sounds as
//! `dialog-error`.

use crate::Audio;
use crate::convert::ChannelOrder;
use std::f32::consts::TAU;

/// The rate the sounds are rendered at: the mixer's, so nothing resamples
/// them.
pub const RATE: u32 = crate::pcm::RATE;

/// The loudest any sound gets, 1.0 full scale: -6 dBFS.
pub const PEAK: f32 = 0.5;

/// How long a tone takes to rise, in seconds: long enough not to click.
const ATTACK: f32 = 0.003;

/// How long every sound fades at its end, in seconds.
const FADE_OUT: f32 = 0.010;

/// A tone's overtones: their ratio to the fundamental and their level.
/// Slightly stretched at the top, as a struck bar's are.
const PARTIALS: [(f32, f32); 4] = [(1.0, 1.0), (2.0, 0.35), (3.0, 0.12), (4.07, 0.05)];

/// A built-in sound.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BuiltIn {
    /// A message arrived: `message-new-instant`, `message`.
    Message,
    /// `dialog-information`.
    Information,
    /// `dialog-question`.
    Question,
    /// `dialog-warning`.
    Warning,
    /// `dialog-error`.
    Error,
    /// `bell`.
    Bell,
    /// Something finished: `complete`.
    Complete,
    /// `trash-empty`.
    TrashEmpty,
    /// `screen-capture`.
    ScreenCapture,
    /// `device-added`.
    DeviceAdded,
    /// `device-removed`.
    DeviceRemoved,
    /// `desktop-login`.
    Login,
    /// `desktop-logout`.
    Logout,
    /// `power-plug`.
    PowerPlug,
    /// `power-unplug`.
    PowerUnplug,
    /// `battery-low`.
    BatteryLow,
    /// `audio-volume-change`: the tick a volume control makes.
    VolumeChange,
}

impl BuiltIn {
    /// Every built-in sound.
    pub const ALL: [Self; 17] = [
        Self::Message,
        Self::Information,
        Self::Question,
        Self::Warning,
        Self::Error,
        Self::Bell,
        Self::Complete,
        Self::TrashEmpty,
        Self::ScreenCapture,
        Self::DeviceAdded,
        Self::DeviceRemoved,
        Self::Login,
        Self::Logout,
        Self::PowerPlug,
        Self::PowerUnplug,
        Self::BatteryLow,
        Self::VolumeChange,
    ];

    /// The sound's own name in the freedesktop sound naming specification.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Message => "message-new-instant",
            Self::Information => "dialog-information",
            Self::Question => "dialog-question",
            Self::Warning => "dialog-warning",
            Self::Error => "dialog-error",
            Self::Bell => "bell",
            Self::Complete => "complete",
            Self::TrashEmpty => "trash-empty",
            Self::ScreenCapture => "screen-capture",
            Self::DeviceAdded => "device-added",
            Self::DeviceRemoved => "device-removed",
            Self::Login => "desktop-login",
            Self::Logout => "desktop-logout",
            Self::PowerPlug => "power-plug",
            Self::PowerUnplug => "power-unplug",
            Self::BatteryLow => "battery-low",
            Self::VolumeChange => "audio-volume-change",
        }
    }

    /// The built-in sound for the event `name` -- a sound naming
    /// specification name -- or for the name cut at its last hyphen, and so
    /// on; `None` if no cut of it has one.
    #[must_use]
    pub fn for_event(name: &str) -> Option<Self> {
        let mut name = name.trim();
        loop {
            if let Some(found) = Self::exactly(name) {
                return Some(found);
            }
            name = name.get(..name.rfind('-')?)?;
        }
    }

    /// The built-in sound for exactly `name`: its own, and the names the
    /// specification gives events that sound alike.
    fn exactly(name: &str) -> Option<Self> {
        Some(match name {
            "message-new-instant" | "message" | "message-new-email" => Self::Message,
            "dialog-information" => Self::Information,
            "dialog-question" => Self::Question,
            "dialog-warning" => Self::Warning,
            "dialog-error" => Self::Error,
            "bell" | "bell-terminal" | "bell-window-system" => Self::Bell,
            "complete" => Self::Complete,
            "trash-empty" => Self::TrashEmpty,
            "screen-capture" | "camera-shutter" => Self::ScreenCapture,
            "device-added" => Self::DeviceAdded,
            "device-removed" => Self::DeviceRemoved,
            "desktop-login" | "service-login" | "system-ready" | "system-bootup" => Self::Login,
            "desktop-logout" | "service-logout" | "system-shutdown" => Self::Logout,
            "power-plug" => Self::PowerPlug,
            "power-unplug" => Self::PowerUnplug,
            "battery-low" | "battery-caution" => Self::BatteryLow,
            "audio-volume-change" => Self::VolumeChange,
            _ => return None,
        })
    }

    /// The sound, rendered: two channels at [`RATE`].
    #[must_use]
    pub fn render(self) -> Audio {
        // Frequencies of the notes used, equal-tempered from A4 = 440 Hz.
        const A4: f32 = 440.0;
        const AS4: f32 = 466.16;
        const F4: f32 = 349.23;
        const C5: f32 = 523.25;
        const D5: f32 = 587.33;
        const E5: f32 = 659.26;
        const G5: f32 = 783.99;
        const A5: f32 = 880.0;
        const C6: f32 = 1046.5;
        const D6: f32 = 1174.66;
        const E6: f32 = 1318.51;
        const G6: f32 = 1567.98;
        let note = |at: f32, freq: f32, decay: f32| Note {
            at,
            freq,
            decay,
            gain: 1.0,
        };
        let mono = match self {
            Self::Message => tones(&[note(0.0, A5, 0.25), note(0.10, E6, 0.4)], 0.65, 0.8),
            Self::Information => tones(&[note(0.0, G5, 0.45)], 0.6, 0.7),
            Self::Question => tones(&[note(0.0, C5, 0.22), note(0.12, G5, 0.42)], 0.65, 0.7),
            Self::Warning => tones(&[note(0.0, A4, 0.18), note(0.16, A4, 0.3)], 0.55, 1.0),
            Self::Error => tones(&[note(0.0, AS4, 0.18), note(0.12, F4, 0.42)], 0.65, 0.9),
            Self::Bell => tones(&[note(0.0, E6, 0.12)], 0.25, 0.6),
            Self::Complete => tones(
                &[
                    note(0.0, C6, 0.22),
                    note(0.07, E6, 0.22),
                    note(0.14, G6, 0.4),
                ],
                0.6,
                0.7,
            ),
            Self::TrashEmpty => noise(
                &[
                    Burst::new(0.0, 0.07, 0.03, 2600.0),
                    Burst::new(0.06, 0.09, 0.04, 3200.0),
                    Burst::new(0.15, 0.12, 0.05, 2200.0),
                ],
                0.4,
            ),
            Self::ScreenCapture => noise(
                &[
                    Burst::new(0.0, 0.012, 0.006, 6000.0),
                    Burst::new(0.09, 0.02, 0.01, 3500.0),
                ],
                0.22,
            ),
            Self::DeviceAdded => tones(&[note(0.0, D5, 0.18), note(0.08, A5, 0.3)], 0.45, 0.7),
            Self::DeviceRemoved => tones(&[note(0.0, A5, 0.18), note(0.08, D5, 0.3)], 0.45, 0.7),
            Self::Login => tones(
                &[
                    note(0.0, C5, 0.6),
                    note(0.11, E5, 0.7),
                    note(0.22, G5, 0.8),
                    note(0.33, C6, 1.0),
                ],
                1.3,
                0.6,
            ),
            Self::Logout => tones(
                &[
                    note(0.0, C6, 0.6),
                    note(0.11, G5, 0.7),
                    note(0.22, E5, 0.8),
                    note(0.33, C5, 1.0),
                ],
                1.3,
                0.6,
            ),
            Self::PowerPlug => tones(&[note(0.0, G5, 0.18), note(0.09, D6, 0.3)], 0.45, 0.7),
            Self::PowerUnplug => tones(&[note(0.0, D6, 0.18), note(0.09, G5, 0.3)], 0.45, 0.7),
            Self::BatteryLow => tones(
                &[
                    note(0.0, A4, 0.12),
                    note(0.18, A4, 0.12),
                    note(0.36, A4, 0.16),
                ],
                0.6,
                1.0,
            ),
            Self::VolumeChange => tones(&[note(0.0, C6, 0.035)], 0.08, 0.5),
        };
        Audio {
            rate: RATE,
            channels: 2,
            samples: mono.iter().flat_map(|&s| [s, s]).collect(),
            order: ChannelOrder::Wav,
        }
    }
}

/// One tone of a chime.
#[derive(Clone, Copy, Debug)]
struct Note {
    /// When it is struck, in seconds from the start.
    at: f32,
    /// Its fundamental, in hertz.
    freq: f32,
    /// How long its fundamental takes to fall to 1/e, in seconds.
    decay: f32,
    /// Its level against the chime's other notes.
    gain: f32,
}

/// Frames in `seconds`.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "a few seconds of frames, non-negative and finite"
)]
fn frames(seconds: f32) -> usize {
    (seconds.max(0.0) * RATE as f32).round() as usize
}

/// `i` frames, in seconds.
#[allow(clippy::cast_precision_loss, reason = "a few seconds of frames")]
fn seconds(i: usize) -> f32 {
    i as f32 / RATE as f32
}

/// `notes` struck as a chime `length` seconds long; `bright` (0 to 1) is
/// how much of the overtones sound.
fn tones(notes: &[Note], length: f32, bright: f32) -> Vec<f32> {
    let n = frames(length);
    let mut out = vec![0.0f32; n];
    for note in notes {
        let start = frames(note.at);
        for (k, &(ratio, level)) in PARTIALS.iter().enumerate() {
            #[allow(
                clippy::cast_precision_loss,
                reason = "an overtone's index, under five"
            )]
            let k = k as f32;
            let level = level * bright.clamp(0.0, 1.0).powf(k) * note.gain;
            // Each overtone fades faster than the one below it.
            let decay = note.decay / (1.0 + 0.7 * k);
            let freq = note.freq * ratio;
            for (i, sample) in out.iter_mut().enumerate().skip(start) {
                let t = seconds(i.saturating_sub(start));
                // Past seven decay times the tone is below a thousandth.
                if t > decay * 7.0 {
                    break;
                }
                let rise = (t / ATTACK).min(1.0);
                *sample += level * rise * (-t / decay).exp() * (TAU * freq * t).sin();
            }
        }
    }
    finish(out)
}

/// A burst of noise: when, how long, how fast it fades, and how bright.
#[derive(Clone, Copy, Debug)]
struct Burst {
    at: f32,
    length: f32,
    decay: f32,
    cutoff: f32,
}

impl Burst {
    fn new(at: f32, length: f32, decay: f32, cutoff: f32) -> Self {
        Self {
            at,
            length,
            decay,
            cutoff,
        }
    }
}

/// `bursts` of shaped noise, `length` seconds in all: each low-passed at
/// its cutoff, and the whole high-passed at 200 Hz so it does not thump.
fn noise(bursts: &[Burst], length: f32) -> Vec<f32> {
    let n = frames(length);
    let mut out = vec![0.0f32; n];
    // Seeded: a sound renders the same every time.
    let mut state: u32 = 0x9E37_79B9;
    for burst in bursts {
        let start = frames(burst.at);
        let end = start.saturating_add(frames(burst.length)).min(n);
        // A one-pole low-pass: the coefficient for its cutoff.
        let pole = (-TAU * burst.cutoff / RATE_F).exp();
        let mut low = 0.0f32;
        for (i, sample) in out.iter_mut().enumerate().take(end).skip(start) {
            state ^= state.wrapping_shl(13);
            state ^= state.wrapping_shr(17);
            state ^= state.wrapping_shl(5);
            #[allow(clippy::cast_precision_loss, reason = "the top 24 bits fit an f32")]
            let white = state.wrapping_shr(8) as f32 / 8_388_608.0 - 1.0;
            low = white * (1.0 - pole) + low * pole;
            let t = seconds(i.saturating_sub(start));
            let rise = (t / 0.001).min(1.0);
            *sample += low * rise * (-t / burst.decay).exp();
        }
    }
    // The high-pass, over the whole: a one-pole's difference form.
    let pole = (-TAU * 200.0 / RATE_F).exp();
    let (mut last_in, mut last_out) = (0.0f32, 0.0f32);
    for sample in &mut out {
        let x = *sample;
        last_out = pole * (last_out + x - last_in);
        last_in = x;
        *sample = last_out;
    }
    finish(out)
}

/// The rate, as a float, for the filters' coefficients.
#[allow(clippy::cast_precision_loss, reason = "48 000 is exact in an f32")]
const RATE_F: f32 = RATE as f32;

/// `mono` brought to [`PEAK`] and faded at its end.
fn finish(mut mono: Vec<f32>) -> Vec<f32> {
    let loudest = mono.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    let scale = if loudest > 0.0 { PEAK / loudest } else { 0.0 };
    let fade = frames(FADE_OUT).max(1);
    let len = mono.len();
    for (i, sample) in mono.iter_mut().enumerate() {
        let left = len.saturating_sub(i);
        #[allow(clippy::cast_precision_loss, reason = "a fade of a few hundred frames")]
        let tail = if left < fade {
            left as f32 / fade as f32
        } else {
            1.0
        };
        *sample *= scale * tail;
    }
    mono
}

#[cfg(test)]
#[path = "builtin_tests.rs"]
mod tests;
