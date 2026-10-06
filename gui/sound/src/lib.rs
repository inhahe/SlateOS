//! System sounds: a sound file decoded ([`decode`]), made the kernel mixer's
//! stream ([`convert`]), and played through the ALSA PCM device ([`pcm`]) --
//! and the sounds played for a theme that brings none ([`builtin`]). And the
//! card's master volume and mute, through its ALSA control device
//! ([`mixer`]), for the desktop's volume control and a settings page's.
//!
//! # The path a sound takes
//!
//! The kernel mixes every program's sound itself, at one rate in one format:
//! 48 kHz, signed 16-bit, two channels (`kernel/src/audio_mixer.rs`). Its
//! ALSA device answers any other request with that one
//! (`kernel/src/audio_alsa.rs`, `refine_to_native`), so the conversion is
//! the player's: a file's samples are decoded to floating point
//! ([`Audio`]), spread or folded to two channels, resampled to 48 kHz,
//! scaled by the volume and quantized with dither ([`convert::to_native`]),
//! then written to `/dev/snd/pcmC0D0p` as the ring takes them, and waited
//! on until they have played ([`pcm::Playback`]).
//!
//! # Where the files come from
//!
//! Not from here. Which file plays for which event is the sound theme's
//! business -- the freedesktop sound theme specification's names
//! (`message-new-instant`, `dialog-error`, `trash-empty`) looked up in the
//! theme the user chose, as the appearance crate looks up icons and
//! cursors. This crate plays what it is given: a file, or one of the
//! sounds it synthesizes itself ([`builtin::BuiltIn`]), which a theme with
//! no sounds of its own gets, so a fresh system is not silent.
//!
//! # Playing without waiting
//!
//! [`play`] decodes, converts and plays on a thread of its own and returns
//! at once: a shell's event loop must not stop for a second while a chime
//! rings. At most [`MAX_PLAYING`] play at a time; one asked for past that
//! is not played -- an event's sound is for its moment, and a queue of
//! them would ring out long after the moment had gone.

pub mod builtin;
pub mod convert;
pub mod decode;
pub mod mixer;
pub mod pcm;
pub mod wav;

#[cfg(test)]
mod fake;
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
mod sys;

use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

pub use builtin::BuiltIn;
pub use convert::ChannelOrder;
pub use decode::DecodeError;
pub use pcm::{Errno, PcmError, PcmSys, Playback};

/// Sound as decoded: interleaved samples, 1.0 full scale, at `rate` frames
/// a second.
#[derive(Clone, Debug, PartialEq)]
pub struct Audio {
    /// Frames a second.
    pub rate: u32,
    /// Samples a frame.
    pub channels: u16,
    /// Interleaved samples, `channels` a frame, nominally within -1.0..=1.0.
    pub samples: Vec<f32>,
    /// Which channel is which past the first two.
    pub order: ChannelOrder,
}

impl Audio {
    /// How many frames it holds.
    #[must_use]
    pub fn frames(&self) -> usize {
        self.samples
            .len()
            .checked_div(usize::from(self.channels))
            .unwrap_or(0)
    }

    /// How long it plays, in milliseconds.
    #[must_use]
    pub fn duration_ms(&self) -> u64 {
        let frames = u64::try_from(self.frames()).unwrap_or(u64::MAX);
        frames
            .saturating_mul(1000)
            .checked_div(u64::from(self.rate))
            .unwrap_or(0)
    }
}

/// The largest sound file read, in bytes. A system sound is a few seconds;
/// a theme naming a whole album -- by mistake, or to annoy -- is refused
/// before it is read into memory.
pub const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;

/// The longest a sound plays, in seconds: what is decoded past it is not.
/// The same reasoning as [`MAX_FILE_BYTES`], for a file small and long --
/// Opus at a few kilobits a second holds an hour in a megabyte.
pub const MAX_SECONDS: u32 = 30;

/// How many sounds play at once; [`play`] declines more.
pub const MAX_PLAYING: usize = 4;

/// A sound to play.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Sound {
    /// A sound file: Ogg Vorbis, Ogg Opus or WAV.
    File(PathBuf),
    /// One of the sounds this crate synthesizes.
    BuiltIn(BuiltIn),
}

/// Why a sound did not play.
#[derive(Debug)]
pub enum PlayError {
    /// The file could not be read.
    Read(std::io::ErrorKind),
    /// The file is larger than [`MAX_FILE_BYTES`].
    TooLarge,
    /// The file is not a sound this crate decodes.
    Decode(DecodeError),
    /// The sound device would not play it.
    Device(PcmError),
    /// [`MAX_PLAYING`] sounds are playing already.
    Busy,
    /// No thread could be started to play it.
    NoThread(std::io::ErrorKind),
    /// This system has no sound device this crate speaks to.
    Unsupported,
}

impl fmt::Display for PlayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(kind) => write!(f, "the sound file cannot be read: {kind}"),
            Self::TooLarge => write!(f, "the sound file is larger than {MAX_FILE_BYTES} bytes"),
            Self::Decode(e) => write!(f, "{e}"),
            Self::Device(e) => write!(f, "{e}"),
            Self::Busy => write!(f, "{MAX_PLAYING} sounds are playing already"),
            Self::NoThread(kind) => write!(f, "no thread to play the sound on: {kind}"),
            Self::Unsupported => write!(f, "this system has no sound device to play on"),
        }
    }
}

impl std::error::Error for PlayError {}

impl From<DecodeError> for PlayError {
    fn from(e: DecodeError) -> Self {
        Self::Decode(e)
    }
}

impl From<PcmError> for PlayError {
    fn from(e: PcmError) -> Self {
        Self::Device(e)
    }
}

/// The file at `path`, read whole if it is no larger than
/// [`MAX_FILE_BYTES`].
///
/// # Errors
///
/// [`PlayError::Read`] if it cannot be read, [`PlayError::TooLarge`] if it
/// is too large.
pub fn read_file(path: &Path) -> Result<Vec<u8>, PlayError> {
    read_at_most(path, MAX_FILE_BYTES)
}

/// The file at `path`, read whole if it is no larger than `limit` bytes.
fn read_at_most(path: &Path, limit: u64) -> Result<Vec<u8>, PlayError> {
    let file = std::fs::File::open(path).map_err(|e| PlayError::Read(e.kind()))?;
    let mut bytes = Vec::new();
    // One byte past the limit is read, so a file of exactly the limit is
    // told from one larger.
    file.take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|e| PlayError::Read(e.kind()))?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > limit {
        return Err(PlayError::TooLarge);
    }
    Ok(bytes)
}

/// Whether `volume` plays nothing: nought or less, or not a number.
fn silent(volume: f32) -> bool {
    volume.is_nan() || volume <= 0.0
}

/// `sound` as the samples the kernel mixer plays: 48 kHz stereo, scaled by
/// `volume` (0 to 1) and quantized ([`convert::to_native`]).
///
/// # Errors
///
/// Whatever reading or decoding the file met.
pub fn render(sound: &Sound, volume: f32) -> Result<Vec<i16>, PlayError> {
    let audio = match sound {
        Sound::File(path) => decode::decode(&read_file(path)?)?,
        Sound::BuiltIn(builtin) => builtin.render(),
    };
    Ok(convert::to_native(&audio, volume))
}

/// Play `sound` at `volume` (0 to 1) on `sys`, here and to the end: render
/// it, write it as the ring takes it, and wait until it has played. What
/// [`play`] does on a thread of its own; public so a host with a device of
/// its own -- and a test, with a fake -- drives the whole path.
///
/// A volume of nought plays nothing and opens nothing.
///
/// # Errors
///
/// Whatever rendering or the device met.
pub fn play_on<S: PcmSys>(sys: S, sound: &Sound, volume: f32) -> Result<(), PlayError> {
    if silent(volume) {
        return Ok(());
    }
    let samples = render(sound, volume)?;
    let mut playback = Playback::open(sys)?;
    playback.write(&samples)?;
    playback.finish()?;
    Ok(())
}

/// How many sounds [`play`] has playing.
static PLAYING: AtomicUsize = AtomicUsize::new(0);

/// One of [`PLAYING`]'s places, given back when dropped -- on every way out
/// of a playing thread, a panic's included.
struct Place;

impl Place {
    /// A place, if one of [`MAX_PLAYING`] is free.
    fn take() -> Option<Self> {
        PLAYING
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < MAX_PLAYING).then(|| n.saturating_add(1))
            })
            .ok()
            .map(|_| Self)
    }
}

impl Drop for Place {
    fn drop(&mut self) {
        // Taken by `take`, so the count is at least one.
        PLAYING.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Play `sound` at `volume` (0 to 1) on this system's sound device, on a
/// thread of its own, and return at once.
///
/// What went wrong after the sound started is the thread's to swallow: an
/// event's sound that does not play is not the event's failure. What can
/// be told at once is returned.
///
/// # Errors
///
/// [`PlayError::Busy`] if [`MAX_PLAYING`] sounds are playing,
/// [`PlayError::NoThread`] if no thread could be started, and
/// [`PlayError::Unsupported`] where this crate has no device to play on.
pub fn play(sound: Sound, volume: f32) -> Result<(), PlayError> {
    if silent(volume) {
        return Ok(());
    }
    if !pcm::DEVICE_SUPPORTED {
        return Err(PlayError::Unsupported);
    }
    let place = Place::take().ok_or(PlayError::Busy)?;
    std::thread::Builder::new()
        .name("sound".to_string())
        .spawn(move || {
            let _place = place;
            // Nothing to tell: see the doc above. A sound that fails fails
            // silently, which is what it would have been anyway.
            let _outcome = pcm::open_device()
                .map_err(PlayError::Device)
                .and_then(|device| play_on(device, &sound, volume));
        })
        .map(|_detached| ())
        .map_err(|e| PlayError::NoThread(e.kind()))
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
