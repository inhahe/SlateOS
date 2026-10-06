//! The ALSA PCM device, spoken to as Linux's is: `/dev/snd/pcmC0D0p`
//! configured with `HW_PARAMS`, armed with `PREPARE`, fed with `write(2)`,
//! and watched with `STATUS` until what was written has played.
//!
//! # A protocol and a device
//!
//! Split as the compositor's DRM code is
//! (`gui/compositor/src/present/drm/sys.rs`): everything about *what* is
//! said -- the payloads' bytes, the order of the calls, what a reply means
//! -- is here, and is tested on the build machine against a fake device
//! ([`PcmSys`]); the calls that say it are in `device`, compiled for the
//! target alone, through the C library (its docs say why not the system
//! call). The payloads are built and read as bytes at their offsets rather
//! than as `#[repr(C)]` structs, so nothing here needs `unsafe`, and a test
//! pins every offset to the kernel's layout (`kernel/src/audio_alsa.rs`,
//! `SndPcmHwParams`, `SndPcmStatus`).
//!
//! # Where it plays today
//!
//! On a Linux host, through ALSA. On SlateOS, not yet: a native program has
//! no way to the PCM device, so the first `ioctl` answers `ENOTTY`
//! ([`PcmError::Configure`]); and the kernel's mixer is not yet emptied into
//! a sound card, so a stream that could be opened would stop draining after
//! its first ring ([`PcmError::Stalled`]). Both are the kernel's to add
//! (`requests/e-ad-no-application-can-reach-the-sound-device.md`); this
//! plays as soon as they are, unchanged.
//!
//! # What the kernel takes
//!
//! One configuration: 48 kHz, signed 16-bit little-endian, two channels,
//! interleaved -- the mixer's own. A request for anything else is answered
//! with that, so [`Playback::open`] asks for exactly it and refuses a reply
//! that says otherwise, rather than play the wrong format at the wrong
//! speed.
//!
//! The device's ring is the mixer's: 16 KiB a stream, 4096 frames, 85 ms.
//! A write takes what fits and says how much; a full ring answers `EAGAIN`
//! whether or not the device was opened non-blocking, so
//! [`Playback::write`] waits and tries again -- and gives up when the ring
//! stops draining, as it would with no sound card behind the mixer. And
//! closing the device empties its ring, so [`Playback::finish`] waits for
//! `STATUS`'s `delay` -- the frames still queued -- to reach nought before
//! the device is let go.

use std::fmt;

/// The playback device: card 0, device 0.
pub const DEVICE_PATH: &str = "/dev/snd/pcmC0D0p";

/// Frames a second, the mixer's.
pub const RATE: u32 = 48_000;

/// Samples a frame, the mixer's.
pub const CHANNELS: u32 = 2;

/// Bytes a frame: two 16-bit samples.
pub const FRAME_BYTES: usize = 4;

/// Frames the ring holds -- the mixer's 16 KiB a stream -- and so the
/// buffer this asks for.
pub const RING_FRAMES: u32 = 4096;

/// Frames a period: a quarter of the ring.
pub const PERIOD_FRAMES: u32 = 1024;

/// Periods in the ring.
const PERIODS: u32 = RING_FRAMES / PERIOD_FRAMES;
/// Bytes a period.
const PERIOD_BYTES: u32 = PERIOD_FRAMES * 4;
/// Bytes the ring holds.
const RING_BYTES: u32 = RING_FRAMES * 4;

/// How long a writer waits for the ring to drain before trying again, and
/// a finisher between looks at `STATUS`, in milliseconds.
pub const WAIT_MS: u64 = 10;

/// How long the ring may go without draining before the device is taken to
/// be playing nothing, in milliseconds: six times what a full ring takes to
/// play.
pub const STALL_MS: u64 = 500;

/// A Linux error number, as the kernel returns it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Errno(pub i32);

impl Errno {
    /// No such thing: an element a card does not have.
    pub const ENOENT: Self = Self(2);
    /// Interrupted: try again.
    pub const EINTR: Self = Self(4);
    /// Not a device this can use.
    pub const EBADF: Self = Self(9);
    /// The ring is full: wait, and try again.
    pub const EAGAIN: Self = Self(11);
    /// No device.
    pub const ENODEV: Self = Self(19);
    /// The request was refused.
    pub const EINVAL: Self = Self(22);
    /// Not a device this request is for: what a native program's `ioctl`
    /// on a sound device answers on SlateOS today.
    pub const ENOTTY: Self = Self(25);
    /// Not this system's.
    pub const ENOSYS: Self = Self(38);
}

impl fmt::Display for Errno {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "errno {}", self.0)
    }
}

// ---------------------------------------------------------------------------
// The ioctls, and their payloads' layouts
// ---------------------------------------------------------------------------

/// `sizeof(struct snd_pcm_hw_params)` on x86-64.
pub const HW_PARAMS_SIZE: usize = 608;
/// `sizeof(struct snd_pcm_status)` on x86-64 (64-bit time).
pub const STATUS_SIZE: usize = 152;

// The requests, as `asm-generic/ioctl.h`'s `_IOC(dir, 'A', nr, size)`
// encodes them: the direction in bits 30-31, the payload's size in 16-29,
// the type `'A'` in 8-15, the number in 0-7. Written out rather than
// computed, and each held to the formula and to the kernel's own asserted
// values by a test (`kernel/src/audio_alsa.rs`, `self_test`).

/// `SNDRV_PCM_IOCTL_HW_PARAMS` (`_IOWR`, 0x11, 608 bytes): commit a
/// configuration.
pub const IOCTL_HW_PARAMS: u32 = 0xC260_4111;
/// `SNDRV_PCM_IOCTL_PREPARE` (`_IO`, 0x40).
pub const IOCTL_PREPARE: u32 = 0x0000_4140;
/// `SNDRV_PCM_IOCTL_DROP` (`_IO`, 0x43): stop, discarding what is queued.
pub const IOCTL_DROP: u32 = 0x0000_4143;
/// `SNDRV_PCM_IOCTL_DRAIN` (`_IO`, 0x44): stop once what is queued has
/// played.
pub const IOCTL_DRAIN: u32 = 0x0000_4144;
/// `SNDRV_PCM_IOCTL_STATUS` (`_IOR`, 0x20, 152 bytes).
pub const IOCTL_STATUS: u32 = 0x8098_4120;

/// Where `struct snd_pcm_hw_params`'s fields are.
mod hw {
    /// The three parameter masks: access, format, subformat, 32 bytes each.
    pub const MASKS: usize = 4;
    pub const MASK_BYTES: usize = 32;
    /// The twelve parameter intervals, sample bits to tick time, 12 bytes
    /// each: `min`, `max`, flags.
    pub const INTERVALS: usize = 260;
    pub const INTERVAL_BYTES: usize = 12;
    /// Which parameters to refine.
    pub const RMASK: usize = 512;
}

/// The masks, by their place.
const MASK_ACCESS: usize = 0;
const MASK_FORMAT: usize = 1;
const MASK_SUBFORMAT: usize = 2;

/// The intervals, by their place: `SNDRV_PCM_HW_PARAM_*` less
/// `SNDRV_PCM_HW_PARAM_SAMPLE_BITS`.
const IV_SAMPLE_BITS: usize = 0;
const IV_FRAME_BITS: usize = 1;
const IV_CHANNELS: usize = 2;
const IV_RATE: usize = 3;
const IV_PERIOD_TIME: usize = 4;
const IV_PERIOD_SIZE: usize = 5;
const IV_PERIOD_BYTES: usize = 6;
const IV_PERIODS: usize = 7;
const IV_BUFFER_TIME: usize = 8;
const IV_BUFFER_SIZE: usize = 9;
const IV_BUFFER_BYTES: usize = 10;
const IV_TICK_TIME: usize = 11;

/// `SNDRV_PCM_ACCESS_RW_INTERLEAVED`.
const ACCESS_RW_INTERLEAVED: u32 = 3;
/// `SNDRV_PCM_FORMAT_S16_LE`.
const FORMAT_S16_LE: u32 = 2;
/// `SNDRV_PCM_SUBFORMAT_STD`.
const SUBFORMAT_STD: u32 = 0;
/// `snd_interval`'s `integer` flag.
const INTERVAL_INTEGER: u32 = 0b100;

/// Where `struct snd_pcm_status`'s fields are.
mod st {
    pub const STATE: usize = 0;
    pub const DELAY: usize = 56;
    pub const AVAIL: usize = 64;
}

fn put_u32(buf: &mut [u8], at: usize, v: u32) {
    if let Some(dst) = buf.get_mut(at..at.saturating_add(4)) {
        dst.copy_from_slice(&v.to_le_bytes());
    }
}

fn get_u32(buf: &[u8], at: usize) -> Option<u32> {
    let bytes: [u8; 4] = buf.get(at..at.checked_add(4)?)?.try_into().ok()?;
    Some(u32::from_le_bytes(bytes))
}

fn get_u64(buf: &[u8], at: usize) -> Option<u64> {
    let bytes: [u8; 8] = buf.get(at..at.checked_add(8)?)?.try_into().ok()?;
    Some(u64::from_le_bytes(bytes))
}

/// Where mask `k` begins.
fn mask_at(k: usize) -> usize {
    hw::MASKS.saturating_add(k.saturating_mul(hw::MASK_BYTES))
}

/// Where interval `k` begins.
fn interval_at(k: usize) -> usize {
    hw::INTERVALS.saturating_add(k.saturating_mul(hw::INTERVAL_BYTES))
}

/// Mask `k` holding `value` alone.
fn set_mask(buf: &mut [u8], k: usize, value: u32) {
    let at = mask_at(k);
    if let Some(mask) = buf.get_mut(at..at.saturating_add(hw::MASK_BYTES)) {
        mask.fill(0);
    }
    let word = usize::try_from(value / 32).unwrap_or(usize::MAX);
    let bit = 1u32.checked_shl(value % 32).unwrap_or(0);
    put_u32(buf, at.saturating_add(word.saturating_mul(4)), bit);
}

/// The one value mask `k` holds, or `None` if it holds none or several.
fn mask_value(buf: &[u8], k: usize) -> Option<u32> {
    let at = mask_at(k);
    let mut found = None;
    for word in 0..8u32 {
        let offset = usize::try_from(word).ok()?.checked_mul(4)?;
        let bits = get_u32(buf, at.checked_add(offset)?)?;
        if bits == 0 {
            continue;
        }
        if found.is_some() || bits.count_ones() != 1 {
            return None;
        }
        found = Some(word.checked_mul(32)?.checked_add(bits.trailing_zeros())?);
    }
    found
}

/// Interval `k` as the one whole number `v`.
fn set_fixed(buf: &mut [u8], k: usize, v: u32) {
    let at = interval_at(k);
    put_u32(buf, at, v);
    put_u32(buf, at.saturating_add(4), v);
    put_u32(buf, at.saturating_add(8), INTERVAL_INTEGER);
}

/// Interval `k` as anything at all, as ALSA-lib's `snd_interval_any`.
fn set_any(buf: &mut [u8], k: usize) {
    let at = interval_at(k);
    put_u32(buf, at, 0);
    put_u32(buf, at.saturating_add(4), u32::MAX);
    put_u32(buf, at.saturating_add(8), 0);
}

/// Interval `k`'s bounds.
fn interval(buf: &[u8], k: usize) -> Option<(u32, u32)> {
    let at = interval_at(k);
    Some((get_u32(buf, at)?, get_u32(buf, at.checked_add(4)?)?))
}

/// The `HW_PARAMS` payload asking for the mixer's configuration, with the
/// ring the mixer has: [`RING_FRAMES`] in periods of [`PERIOD_FRAMES`].
#[must_use]
pub fn hw_params_request() -> [u8; HW_PARAMS_SIZE] {
    let mut p = [0u8; HW_PARAMS_SIZE];
    // Every parameter is to be settled, as ALSA-lib asks.
    put_u32(&mut p, hw::RMASK, u32::MAX);
    set_mask(&mut p, MASK_ACCESS, ACCESS_RW_INTERLEAVED);
    set_mask(&mut p, MASK_FORMAT, FORMAT_S16_LE);
    set_mask(&mut p, MASK_SUBFORMAT, SUBFORMAT_STD);
    set_fixed(&mut p, IV_SAMPLE_BITS, 16);
    set_fixed(&mut p, IV_FRAME_BITS, 32);
    set_fixed(&mut p, IV_CHANNELS, CHANNELS);
    set_fixed(&mut p, IV_RATE, RATE);
    set_fixed(&mut p, IV_PERIOD_SIZE, PERIOD_FRAMES);
    set_fixed(&mut p, IV_PERIOD_BYTES, PERIOD_BYTES);
    set_fixed(&mut p, IV_PERIODS, PERIODS);
    set_fixed(&mut p, IV_BUFFER_SIZE, RING_FRAMES);
    set_fixed(&mut p, IV_BUFFER_BYTES, RING_BYTES);
    // Times follow from the sizes: left to the device.
    set_any(&mut p, IV_PERIOD_TIME);
    set_any(&mut p, IV_BUFFER_TIME);
    set_any(&mut p, IV_TICK_TIME);
    p
}

/// What a `HW_PARAMS` reply settled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Configured {
    /// `SNDRV_PCM_ACCESS_*`, if one.
    pub access: Option<u32>,
    /// `SNDRV_PCM_FORMAT_*`, if one.
    pub format: Option<u32>,
    /// The channels' bounds.
    pub channels: (u32, u32),
    /// The rate's bounds.
    pub rate: (u32, u32),
    /// The ring's size, in frames.
    pub buffer_frames: u32,
}

impl Configured {
    /// The reply in `payload`, or `None` for one too short to be one.
    #[must_use]
    pub fn read(payload: &[u8]) -> Option<Self> {
        if payload.len() < HW_PARAMS_SIZE {
            return None;
        }
        Some(Self {
            access: mask_value(payload, MASK_ACCESS),
            format: mask_value(payload, MASK_FORMAT),
            channels: interval(payload, IV_CHANNELS)?,
            rate: interval(payload, IV_RATE)?,
            buffer_frames: interval(payload, IV_BUFFER_SIZE)?.0,
        })
    }

    /// Whether this is the mixer's configuration, the one this writes.
    #[must_use]
    pub fn is_native(&self) -> bool {
        self.access == Some(ACCESS_RW_INTERLEAVED)
            && self.format == Some(FORMAT_S16_LE)
            && self.channels == (CHANNELS, CHANNELS)
            && self.rate == (RATE, RATE)
    }
}

/// What a `STATUS` reply says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Status {
    /// `SNDRV_PCM_STATE_*`.
    pub state: u32,
    /// Frames written and not yet played.
    pub delay: i64,
    /// Frames the ring has room for.
    pub avail: u64,
}

impl Status {
    /// The reply in `payload`, or `None` for one too short to be one.
    #[must_use]
    pub fn read(payload: &[u8]) -> Option<Self> {
        if payload.len() < STATUS_SIZE {
            return None;
        }
        Some(Self {
            state: get_u32(payload, st::STATE)?,
            delay: i64::from_le_bytes(get_u64(payload, st::DELAY)?.to_le_bytes()),
            avail: get_u64(payload, st::AVAIL)?,
        })
    }
}

// ---------------------------------------------------------------------------
// Talking to a device
// ---------------------------------------------------------------------------

/// What playing needs from the system: the device's `ioctl` and `write`,
/// and a moment's wait. The real one is the device file; a test's is a
/// fake that records what it was told and answers as the kernel would.
pub trait PcmSys {
    /// `ioctl(device, request, payload)`: `payload` is `request`'s size, or
    /// empty for a request that carries none.
    ///
    /// # Errors
    ///
    /// The kernel's errno.
    fn ioctl(&mut self, request: u32, payload: &mut [u8]) -> Result<(), Errno>;

    /// `write(device, bytes)`: how many bytes the ring took.
    ///
    /// # Errors
    ///
    /// The kernel's errno -- [`Errno::EAGAIN`] for a full ring.
    fn write(&mut self, bytes: &[u8]) -> Result<usize, Errno>;

    /// Wait `ms` milliseconds: while the ring drains.
    fn pause(&mut self, ms: u64);
}

/// Why the device did not play.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PcmError {
    /// The device would not open.
    Open(Errno),
    /// `HW_PARAMS` was refused.
    Configure(Errno),
    /// `HW_PARAMS`' reply could not be read.
    Garbled,
    /// The device settled on another configuration than the mixer's.
    NotNative(Configured),
    /// `PREPARE` was refused.
    Prepare(Errno),
    /// A write was refused.
    Write(Errno),
    /// `STATUS` was refused, or its reply could not be read.
    Status(Errno),
    /// The ring stopped draining for [`STALL_MS`]: nothing is playing it.
    Stalled,
}

impl fmt::Display for PcmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Open(e) => write!(f, "the sound device would not open ({e})"),
            Self::Configure(e) => write!(f, "the sound device refused its configuration ({e})"),
            Self::Garbled => write!(f, "the sound device's configuration could not be read"),
            Self::NotNative(c) => write!(
                f,
                "the sound device settled on another configuration: {c:?}"
            ),
            Self::Prepare(e) => write!(f, "the sound device would not prepare ({e})"),
            Self::Write(e) => write!(f, "the sound device refused the sound ({e})"),
            Self::Status(e) => write!(
                f,
                "the sound device would not say how far it has played ({e})"
            ),
            Self::Stalled => write!(f, "the sound device stopped playing"),
        }
    }
}

impl std::error::Error for PcmError {}

/// A configured device, playing.
#[derive(Debug)]
pub struct Playback<S: PcmSys> {
    sys: S,
}

impl<S: PcmSys> Playback<S> {
    /// Configure `sys` for the mixer's format and prepare it.
    ///
    /// # Errors
    ///
    /// [`PcmError::Configure`], [`PcmError::Garbled`] or
    /// [`PcmError::NotNative`] for a configuration refused or not the
    /// mixer's; [`PcmError::Prepare`] for a refused `PREPARE`.
    pub fn open(sys: S) -> Result<Self, PcmError> {
        let mut sys = sys;
        let mut params = hw_params_request();
        sys.ioctl(IOCTL_HW_PARAMS, &mut params)
            .map_err(PcmError::Configure)?;
        let settled = Configured::read(&params).ok_or(PcmError::Garbled)?;
        if !settled.is_native() {
            return Err(PcmError::NotNative(settled));
        }
        sys.ioctl(IOCTL_PREPARE, &mut [])
            .map_err(PcmError::Prepare)?;
        Ok(Self { sys })
    }

    /// Write `samples` -- interleaved left and right, 48 kHz -- as the ring
    /// takes them: what does not fit waits for it to drain.
    ///
    /// # Errors
    ///
    /// [`PcmError::Write`] for a refused write; [`PcmError::Stalled`] when
    /// the ring takes nothing for [`STALL_MS`].
    pub fn write(&mut self, samples: &[i16]) -> Result<(), PcmError> {
        let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        let mut at = 0usize;
        let mut idle = 0u64;
        while let Some(rest) = bytes.get(at..).filter(|rest| !rest.is_empty()) {
            match self.sys.write(rest) {
                Ok(taken) if taken > 0 => {
                    at = at.saturating_add(taken.min(rest.len()));
                    idle = 0;
                }
                Ok(_) | Err(Errno::EAGAIN) => {
                    if idle >= STALL_MS {
                        // Stop what is queued: nothing will play it.
                        let _ = self.sys.ioctl(IOCTL_DROP, &mut []);
                        return Err(PcmError::Stalled);
                    }
                    self.sys.pause(WAIT_MS);
                    idle = idle.saturating_add(WAIT_MS);
                }
                Err(Errno::EINTR) => {}
                Err(e) => return Err(PcmError::Write(e)),
            }
        }
        Ok(())
    }

    /// Wait for what was written to play, then stop the device: closing it
    /// sooner would empty its ring with the end of the sound still in it.
    ///
    /// # Errors
    ///
    /// [`PcmError::Status`] if the device will not say how far it has
    /// played; [`PcmError::Stalled`] when it stops draining for
    /// [`STALL_MS`].
    pub fn finish(mut self) -> Result<(), PcmError> {
        let mut idle = 0u64;
        let mut last = i64::MAX;
        loop {
            let mut payload = [0u8; STATUS_SIZE];
            self.sys
                .ioctl(IOCTL_STATUS, &mut payload)
                .map_err(PcmError::Status)?;
            let status = Status::read(&payload).ok_or(PcmError::Status(Errno::EINVAL))?;
            if status.delay <= 0 {
                break;
            }
            if status.delay < last {
                idle = 0;
            } else if idle >= STALL_MS {
                let _ = self.sys.ioctl(IOCTL_DROP, &mut []);
                return Err(PcmError::Stalled);
            }
            last = status.delay;
            self.sys.pause(WAIT_MS);
            idle = idle.saturating_add(WAIT_MS);
        }
        // Played out: back to a stopped device. A refusal here changes
        // nothing a listener hears -- the sound has played.
        let _ = self.sys.ioctl(IOCTL_DRAIN, &mut []);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// The device itself
// ---------------------------------------------------------------------------

/// Whether this build has a device to play on: the SlateOS target, which
/// is `target_os = "linux"` (`toolchain/x86_64-slateos.json`), or a Linux
/// host, whose ALSA speaks the same protocol. (Whether the device answers
/// is another matter: see the module's "Where it plays today".)
pub const DEVICE_SUPPORTED: bool = cfg!(all(target_os = "linux", target_arch = "x86_64"));

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub use device::Device;

/// The playback device, opened.
///
/// # Errors
///
/// [`PcmError::Open`] with the kernel's errno.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub fn open_device() -> Result<Device, PcmError> {
    Device::open().map_err(PcmError::Open)
}

/// No device: this build's system is not one this crate plays on.
#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
#[derive(Debug)]
pub struct Unavailable;

#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
impl PcmSys for Unavailable {
    fn ioctl(&mut self, _request: u32, _payload: &mut [u8]) -> Result<(), Errno> {
        Err(Errno::ENOSYS)
    }

    fn write(&mut self, _bytes: &[u8]) -> Result<usize, Errno> {
        Err(Errno::ENOSYS)
    }

    fn pause(&mut self, _ms: u64) {}
}

/// The playback device: never, on a system this crate does not play on.
///
/// # Errors
///
/// Always [`PcmError::Open`] with [`Errno::ENOSYS`].
#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
pub fn open_device() -> Result<Unavailable, PcmError> {
    Err(PcmError::Open(Errno::ENOSYS))
}

/// The real thing: the device file, through the C library -- opened,
/// written and closed by `std`, and controlled by the C library's `ioctl`,
/// which `std` does not offer.
///
/// # Why the C library's `ioctl`, and not the system call
///
/// SlateOS runs two system-call tables, and which one a process's `syscall`
/// instruction reaches is decided when its binary is loaded
/// (`kernel/src/syscall/entry.rs`, `AbiMode`): a binary marked as Linux's --
/// the GNU OS/ABI tag, a Linux loader, a `PT_GNU_PROPERTY` header
/// (`kernel/src/proc/elf.rs`, `detect_linux_abi`) -- gets Linux's numbering,
/// and every other gets SlateOS's own. The shell and every program in this
/// tree are the other kind. There, number 16 is not `ioctl` but
/// `clock_adjtime` (`kernel/src/syscall/number.rs`): a raw `ioctl` would set
/// the clock with a file descriptor for a clock and an ioctl number for a
/// pointer. The C library knows which table its process has. On Linux its
/// `ioctl` reaches ALSA; in a native SlateOS program it reaches lane D's
/// (`posix/src/ioctl.rs`), which answers `ENOTTY` for a sound request until
/// the kernel gives native programs a way to the PCM device
/// (`requests/e-ad-no-application-can-reach-the-sound-device.md`, the route
/// lanes A and D agreed) -- and then works unchanged.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
mod device {
    use super::{DEVICE_PATH, Errno, PcmSys};
    use std::io::Write;
    use std::os::fd::AsRawFd;

    /// `/dev/snd/pcmC0D0p`, open for writing.
    #[derive(Debug)]
    pub struct Device {
        file: std::fs::File,
    }

    impl Device {
        /// Open the playback device.
        ///
        /// # Errors
        ///
        /// The errno the open met.
        pub fn open() -> Result<Self, Errno> {
            std::fs::OpenOptions::new()
                .write(true)
                .open(DEVICE_PATH)
                .map(|file| Self { file })
                .map_err(|e| Errno(e.raw_os_error().unwrap_or(Errno::ENODEV.0)))
        }
    }

    impl PcmSys for Device {
        fn ioctl(&mut self, request: u32, payload: &mut [u8]) -> Result<(), Errno> {
            // The payloads are built at the sizes the requests encode
            // (`HW_PARAMS_SIZE`, `STATUS_SIZE`, none); `sys::ioctl` checks.
            crate::sys::ioctl(self.file.as_raw_fd(), request, payload)
        }

        fn write(&mut self, bytes: &[u8]) -> Result<usize, Errno> {
            self.file
                .write(bytes)
                .map_err(|e| Errno(e.raw_os_error().unwrap_or(Errno::EINVAL.0)))
        }

        fn pause(&mut self, ms: u64) {
            std::thread::sleep(std::time::Duration::from_millis(ms));
        }
    }
}

#[cfg(test)]
#[path = "pcm_tests.rs"]
mod tests;
