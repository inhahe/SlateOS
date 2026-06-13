//! ALSA (Advanced Linux Sound Architecture) PCM ABI definitions — the
//! foundation of the Linux audio-compatibility shim (roadmap §5.1 /
//! §5095).
//!
//! ## Why this exists
//!
//! Linux audio applications — and the PulseAudio / PipeWire daemons that
//! most desktop apps and WINE talk to — ultimately speak the **ALSA
//! kernel PCM interface**: they `open("/dev/snd/pcmC0D0p")`, negotiate a
//! hardware configuration with a sequence of `ioctl(SNDRV_PCM_IOCTL_*)`
//! calls, and then stream audio frames.  Providing this interface is the
//! single most compatible foundation for Linux audio support, because
//! everything else (PulseAudio, PipeWire, JACK, SDL, OpenAL, raw
//! tinyalsa clients) is layered on top of it.  We therefore build ALSA
//! first and let the higher layers sit on it, mirroring how Linux itself
//! is structured.
//!
//! This module is the **pure ABI layer**: the constant tags, the ioctl
//! request-number encoding, and the format-translation logic that maps an
//! ALSA stream configuration onto the kernel [`crate::audio_mixer`]'s
//! fixed 48 kHz / S16_LE / stereo pipeline.  It deliberately contains no
//! device nodes, no per-fd state, and no `unsafe` — those land in the
//! follow-up commits that wire `/dev/snd/*` into the VFS and route the
//! PCM ioctls/`write` path through here.  Keeping the ABI surface pure
//! and exhaustively self-tested means the wiring layers build on a
//! verified, byte-accurate foundation.
//!
//! ## ABI accuracy
//!
//! The values below are fixed by the Linux UAPI header
//! `include/uapi/sound/asound.h` and must not be renumbered.  The
//! "simple" ioctls encoded here carry no payload struct (`_IO`) or a
//! plain `int` (`_IOR/_IOW` of size 4), so their request numbers are
//! fully determined by the `(direction, type, nr, size)` tuple and are
//! asserted against their known Linux hex values in [`self_test`].  The
//! struct-carrying ioctls (`HW_PARAMS`, `SW_PARAMS`, `WRITEI_FRAMES`,
//! `STATUS`, `INFO`, …) encode `sizeof(struct)` in the request number and
//! are intentionally **not** defined here yet: they require byte-exact
//! `#[repr(C)]` mirrors of the corresponding `asound.h` structures, which
//! the next commit adds together with size assertions.

#![allow(dead_code)] // ABI surface; consumers land in follow-up wiring commits.

use crate::audio_mixer;
use crate::serial_println;

// ---------------------------------------------------------------------------
// PCM stream direction
// ---------------------------------------------------------------------------

/// Playback stream (host → device).  `/dev/snd/pcmC<c>D<d>p`.
pub const SNDRV_PCM_STREAM_PLAYBACK: u32 = 0;
/// Capture stream (device → host).  `/dev/snd/pcmC<c>D<d>c`.
pub const SNDRV_PCM_STREAM_CAPTURE: u32 = 1;

// ---------------------------------------------------------------------------
// PCM access modes (how frames are laid out in the transfer buffer)
// ---------------------------------------------------------------------------

/// mmap'd, channels interleaved within each frame.
pub const SNDRV_PCM_ACCESS_MMAP_INTERLEAVED: u32 = 0;
/// mmap'd, each channel in its own contiguous area.
pub const SNDRV_PCM_ACCESS_MMAP_NONINTERLEAVED: u32 = 1;
/// mmap'd, hardware-defined complex layout.
pub const SNDRV_PCM_ACCESS_MMAP_COMPLEX: u32 = 2;
/// read()/write(), channels interleaved within each frame.
pub const SNDRV_PCM_ACCESS_RW_INTERLEAVED: u32 = 3;
/// read()/write(), each channel in its own buffer (writev/readv style).
pub const SNDRV_PCM_ACCESS_RW_NONINTERLEAVED: u32 = 4;

// ---------------------------------------------------------------------------
// PCM sample formats (the subset relevant to a 16-bit stereo mixer plus
// the common neighbours apps negotiate down from)
// ---------------------------------------------------------------------------

/// Signed 8-bit.
pub const SNDRV_PCM_FORMAT_S8: u32 = 0;
/// Unsigned 8-bit.
pub const SNDRV_PCM_FORMAT_U8: u32 = 1;
/// Signed 16-bit, little-endian. **The mixer's native format.**
pub const SNDRV_PCM_FORMAT_S16_LE: u32 = 2;
/// Signed 16-bit, big-endian.
pub const SNDRV_PCM_FORMAT_S16_BE: u32 = 3;
/// Unsigned 16-bit, little-endian.
pub const SNDRV_PCM_FORMAT_U16_LE: u32 = 4;
/// Unsigned 16-bit, big-endian.
pub const SNDRV_PCM_FORMAT_U16_BE: u32 = 5;
/// Signed 24-bit in the low 3 bytes of a 32-bit word, little-endian.
pub const SNDRV_PCM_FORMAT_S24_LE: u32 = 6;
/// Signed 32-bit, little-endian.
pub const SNDRV_PCM_FORMAT_S32_LE: u32 = 10;
/// Signed 32-bit, big-endian.
pub const SNDRV_PCM_FORMAT_S32_BE: u32 = 11;
/// 32-bit float, little-endian.
pub const SNDRV_PCM_FORMAT_FLOAT_LE: u32 = 14;

// ---------------------------------------------------------------------------
// PCM stream state (returned in snd_pcm_status; mirrored here for the
// state machine the wiring layer will maintain per open stream)
// ---------------------------------------------------------------------------

/// Stream open, no parameters set yet.
pub const SNDRV_PCM_STATE_OPEN: u32 = 0;
/// Parameters set (`HW_PARAMS` done), not yet prepared.
pub const SNDRV_PCM_STATE_SETUP: u32 = 1;
/// Prepared to start (`PREPARE` done).
pub const SNDRV_PCM_STATE_PREPARED: u32 = 2;
/// Running (frames flowing).
pub const SNDRV_PCM_STATE_RUNNING: u32 = 3;
/// Stopped due to underrun/overrun (`XRUN`).
pub const SNDRV_PCM_STATE_XRUN: u32 = 4;
/// Draining (capture only).
pub const SNDRV_PCM_STATE_DRAINING: u32 = 5;
/// Paused.
pub const SNDRV_PCM_STATE_PAUSED: u32 = 6;
/// Hardware suspended.
pub const SNDRV_PCM_STATE_SUSPENDED: u32 = 7;
/// Hardware disconnected.
pub const SNDRV_PCM_STATE_DISCONNECTED: u32 = 8;

// ---------------------------------------------------------------------------
// ioctl request-number encoding (Linux `include/uapi/asm-generic/ioctl.h`)
// ---------------------------------------------------------------------------

/// Number-field width (bits) — the per-driver command index.
const IOC_NRBITS: u32 = 8;
/// Type-field width (bits) — the driver "magic" letter.
const IOC_TYPEBITS: u32 = 8;
/// Size-field width (bits) — `sizeof` the argument struct.
const IOC_SIZEBITS: u32 = 14;

const IOC_NRSHIFT: u32 = 0;
const IOC_TYPESHIFT: u32 = IOC_NRSHIFT + IOC_NRBITS; // 8
const IOC_SIZESHIFT: u32 = IOC_TYPESHIFT + IOC_TYPEBITS; // 16
const IOC_DIRSHIFT: u32 = IOC_SIZESHIFT + IOC_SIZEBITS; // 30

/// Direction: no data transferred (`_IO`).
const IOC_NONE: u32 = 0;
/// Direction: userspace writes to the kernel (`_IOW`).
const IOC_WRITE: u32 = 1;
/// Direction: kernel writes to userspace (`_IOR`).
const IOC_READ: u32 = 2;

/// The ALSA PCM ioctl "magic" letter (`'A'`).
const SNDRV_PCM_IOCTL_MAGIC: u32 = 0x41; // b'A'

/// Encode an ioctl request number from its `(dir, type, nr, size)` tuple,
/// matching Linux's `_IOC(dir, type, nr, size)` macro.
///
/// Pure bit-twiddling — no arithmetic, so it is `const` and clippy-clean.
/// `size` must fit in [`IOC_SIZEBITS`]; callers only pass `0` or `4`
/// (plain-`int` payloads) here, both well within range.
const fn ioc(dir: u32, ty: u32, nr: u32, size: u32) -> u32 {
    (dir << IOC_DIRSHIFT)
        | (ty << IOC_TYPESHIFT)
        | (nr << IOC_NRSHIFT)
        | ((size & ((1 << IOC_SIZEBITS) - 1)) << IOC_SIZESHIFT)
}

/// `_IO('A', nr)` — no payload.
const fn io(nr: u32) -> u32 {
    ioc(IOC_NONE, SNDRV_PCM_IOCTL_MAGIC, nr, 0)
}
/// `_IOR('A', nr, int)` — kernel returns a 4-byte int.
const fn ior_int(nr: u32) -> u32 {
    ioc(IOC_READ, SNDRV_PCM_IOCTL_MAGIC, nr, 4)
}
/// `_IOW('A', nr, int)` — userspace passes a 4-byte int.
const fn iow_int(nr: u32) -> u32 {
    ioc(IOC_WRITE, SNDRV_PCM_IOCTL_MAGIC, nr, 4)
}

// --- Simple (no-struct / plain-int) PCM ioctls ----------------------------
//
// These carry no payload struct, so their request numbers are fully
// determined here.  Struct-carrying ioctls (HW_REFINE/HW_PARAMS/SW_PARAMS/
// WRITEI_FRAMES/READI_FRAMES/STATUS/INFO/SYNC_PTR/CHANNEL_INFO/…) encode
// sizeof(struct) and are added with their #[repr(C)] mirrors in a
// follow-up commit.

/// Query the ALSA protocol version the kernel implements (`int` out).
pub const SNDRV_PCM_IOCTL_PVERSION: u32 = ior_int(0x00);
/// Set the timestamp mode (`int` in).
pub const SNDRV_PCM_IOCTL_TTSTAMP: u32 = iow_int(0x03);
/// Force a hardware-pointer sync point (no payload).
pub const SNDRV_PCM_IOCTL_HWSYNC: u32 = io(0x22);
/// Move the stream to the PREPARED state (no payload).
pub const SNDRV_PCM_IOCTL_PREPARE: u32 = io(0x40);
/// Reset the stream's position/state (no payload).
pub const SNDRV_PCM_IOCTL_RESET: u32 = io(0x41);
/// Start the stream running (no payload).
pub const SNDRV_PCM_IOCTL_START: u32 = io(0x42);
/// Drop (discard) any buffered frames and stop (no payload).
pub const SNDRV_PCM_IOCTL_DROP: u32 = io(0x43);
/// Drain the stream (play out remaining frames, then stop; no payload).
pub const SNDRV_PCM_IOCTL_DRAIN: u32 = io(0x44);
/// Pause (1) / resume (0) the stream (`int` in).
pub const SNDRV_PCM_IOCTL_PAUSE: u32 = iow_int(0x45);
/// Resume from a suspended state (no payload).
pub const SNDRV_PCM_IOCTL_RESUME: u32 = io(0x47);
/// Report an underrun/overrun to move the stream to XRUN (no payload).
pub const SNDRV_PCM_IOCTL_XRUN: u32 = io(0x48);
/// Link this stream to another for synchronised start/stop (`int` in).
pub const SNDRV_PCM_IOCTL_LINK: u32 = iow_int(0x60);
/// Unlink this stream from its sync group (no payload).
pub const SNDRV_PCM_IOCTL_UNLINK: u32 = io(0x61);

/// The ALSA protocol version we advertise through `SNDRV_PCM_IOCTL_PVERSION`.
///
/// `SNDRV_PROTOCOL_VERSION(2, 0, 15)` = `(2 << 16) | (0 << 8) | 15`, the
/// version current ALSA-lib builds expect.  Encoded as a literal to stay
/// clippy-clean (no arithmetic in a `const`).
pub const SNDRV_PCM_VERSION: u32 = 0x0002_000f;

// ---------------------------------------------------------------------------
// Format / configuration translation onto the mixer pipeline
// ---------------------------------------------------------------------------

/// The sample rate the [`crate::audio_mixer`] runs at internally.
pub const MIXER_RATE: u32 = audio_mixer::SAMPLE_RATE; // 48000
/// The channel count the mixer runs at internally (stereo).
pub const MIXER_CHANNELS: u32 = 2;

/// Bytes occupied by one sample of `format`, or `None` if we do not model
/// that format.  (Frame size = `bytes_per_sample × channels`.)
#[must_use]
pub fn format_bytes_per_sample(format: u32) -> Option<u32> {
    match format {
        SNDRV_PCM_FORMAT_S8 | SNDRV_PCM_FORMAT_U8 => Some(1),
        SNDRV_PCM_FORMAT_S16_LE
        | SNDRV_PCM_FORMAT_S16_BE
        | SNDRV_PCM_FORMAT_U16_LE
        | SNDRV_PCM_FORMAT_U16_BE => Some(2),
        SNDRV_PCM_FORMAT_S24_LE => Some(4), // padded to 32-bit container
        SNDRV_PCM_FORMAT_S32_LE
        | SNDRV_PCM_FORMAT_S32_BE
        | SNDRV_PCM_FORMAT_FLOAT_LE => Some(4),
        _ => None,
    }
}

/// Whether a stream configuration maps **directly** onto the mixer with no
/// resampling or format conversion — i.e. the mixer's native
/// 48 kHz / S16_LE / stereo.  Configurations that differ require a
/// conversion stage (added in a later commit); until then the wiring
/// layer advertises only the native configuration through `HW_REFINE` so
/// ALSA-lib negotiates down to it.
#[must_use]
pub fn mixer_accepts_directly(format: u32, rate: u32, channels: u32) -> bool {
    format == SNDRV_PCM_FORMAT_S16_LE && rate == MIXER_RATE && channels == MIXER_CHANNELS
}

/// Size in bytes of `frames` frames at the mixer's native frame size
/// (4 bytes: S16_LE × stereo), or `None` on overflow.  Used by the
/// `write` path to bound a transfer.
#[must_use]
pub fn mixer_frames_to_bytes(frames: u32) -> Option<usize> {
    (frames as usize).checked_mul(audio_mixer::FRAME_SIZE_BYTES)
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Exhaustive boot self-test for the ALSA ABI layer.  Returns `Err` after
/// printing a `FAIL` line so the kernel boot self-test can surface a
/// regression without panicking.
///
/// # Errors
/// Returns [`crate::error::KernelError::InternalError`] if any ABI
/// constant or translation result does not match its expected value.
pub fn self_test() -> crate::error::KernelResult<()> {
    use crate::error::KernelError;

    macro_rules! check {
        ($cond:expr, $($arg:tt)*) => {
            if !($cond) {
                serial_println!("[alsa]   FAIL: {}", format_args!($($arg)*));
                return Err(KernelError::InternalError);
            }
        };
    }

    // --- ioctl encodings against known Linux hex values ------------------
    // PVERSION = _IOR('A', 0x00, int) is the canonical reference value.
    check!(
        SNDRV_PCM_IOCTL_PVERSION == 0x8004_4100,
        "PVERSION enc {:#x} != 0x80044100",
        SNDRV_PCM_IOCTL_PVERSION
    );
    check!(
        SNDRV_PCM_IOCTL_TTSTAMP == 0x4004_4103,
        "TTSTAMP enc {:#x}",
        SNDRV_PCM_IOCTL_TTSTAMP
    );
    check!(SNDRV_PCM_IOCTL_HWSYNC == 0x4122, "HWSYNC enc {:#x}", SNDRV_PCM_IOCTL_HWSYNC);
    check!(SNDRV_PCM_IOCTL_PREPARE == 0x4140, "PREPARE enc {:#x}", SNDRV_PCM_IOCTL_PREPARE);
    check!(SNDRV_PCM_IOCTL_RESET == 0x4141, "RESET enc {:#x}", SNDRV_PCM_IOCTL_RESET);
    check!(SNDRV_PCM_IOCTL_START == 0x4142, "START enc {:#x}", SNDRV_PCM_IOCTL_START);
    check!(SNDRV_PCM_IOCTL_DROP == 0x4143, "DROP enc {:#x}", SNDRV_PCM_IOCTL_DROP);
    check!(SNDRV_PCM_IOCTL_DRAIN == 0x4144, "DRAIN enc {:#x}", SNDRV_PCM_IOCTL_DRAIN);
    check!(
        SNDRV_PCM_IOCTL_PAUSE == 0x4004_4145,
        "PAUSE enc {:#x}",
        SNDRV_PCM_IOCTL_PAUSE
    );
    check!(SNDRV_PCM_IOCTL_RESUME == 0x4147, "RESUME enc {:#x}", SNDRV_PCM_IOCTL_RESUME);
    check!(SNDRV_PCM_IOCTL_XRUN == 0x4148, "XRUN enc {:#x}", SNDRV_PCM_IOCTL_XRUN);
    check!(
        SNDRV_PCM_IOCTL_LINK == 0x4004_4160,
        "LINK enc {:#x}",
        SNDRV_PCM_IOCTL_LINK
    );
    check!(SNDRV_PCM_IOCTL_UNLINK == 0x4161, "UNLINK enc {:#x}", SNDRV_PCM_IOCTL_UNLINK);

    // --- the _IOC helper's field decomposition --------------------------
    // Round-trip: extract dir/size/type/nr back out of PVERSION.
    let pv = SNDRV_PCM_IOCTL_PVERSION;
    check!((pv >> IOC_DIRSHIFT) == IOC_READ, "PVERSION dir wrong");
    check!(
        ((pv >> IOC_SIZESHIFT) & ((1 << IOC_SIZEBITS) - 1)) == 4,
        "PVERSION size wrong"
    );
    check!(((pv >> IOC_TYPESHIFT) & 0xff) == SNDRV_PCM_IOCTL_MAGIC, "PVERSION type wrong");
    check!((pv & 0xff) == 0, "PVERSION nr wrong");

    // --- protocol version literal ---------------------------------------
    // SNDRV_PROTOCOL_VERSION(2,0,15) == (2<<16)|(0<<8)|15.
    check!(SNDRV_PCM_VERSION == 0x0002_000f, "version literal wrong");

    // --- format → bytes-per-sample --------------------------------------
    check!(format_bytes_per_sample(SNDRV_PCM_FORMAT_S8) == Some(1), "S8 bps");
    check!(format_bytes_per_sample(SNDRV_PCM_FORMAT_U8) == Some(1), "U8 bps");
    check!(format_bytes_per_sample(SNDRV_PCM_FORMAT_S16_LE) == Some(2), "S16_LE bps");
    check!(format_bytes_per_sample(SNDRV_PCM_FORMAT_S16_BE) == Some(2), "S16_BE bps");
    check!(format_bytes_per_sample(SNDRV_PCM_FORMAT_S24_LE) == Some(4), "S24_LE bps");
    check!(format_bytes_per_sample(SNDRV_PCM_FORMAT_S32_LE) == Some(4), "S32_LE bps");
    check!(format_bytes_per_sample(SNDRV_PCM_FORMAT_FLOAT_LE) == Some(4), "FLOAT_LE bps");
    check!(format_bytes_per_sample(0xdead).is_none(), "unknown format must be None");

    // --- direct-mixer acceptance ----------------------------------------
    check!(
        mixer_accepts_directly(SNDRV_PCM_FORMAT_S16_LE, 48000, 2),
        "native config must be accepted directly"
    );
    check!(
        !mixer_accepts_directly(SNDRV_PCM_FORMAT_S16_LE, 44100, 2),
        "44.1kHz must require conversion"
    );
    check!(
        !mixer_accepts_directly(SNDRV_PCM_FORMAT_S32_LE, 48000, 2),
        "S32 must require conversion"
    );
    check!(
        !mixer_accepts_directly(SNDRV_PCM_FORMAT_S16_LE, 48000, 1),
        "mono must require conversion"
    );
    check!(MIXER_RATE == 48000 && MIXER_CHANNELS == 2, "mixer config constants");

    // --- frame → byte sizing (native 4-byte frame) ----------------------
    check!(mixer_frames_to_bytes(0) == Some(0), "0 frames");
    check!(mixer_frames_to_bytes(1) == Some(4), "1 frame == 4 bytes");
    check!(mixer_frames_to_bytes(1024) == Some(4096), "1024 frames == 4096 bytes");

    serial_println!("[alsa] ALSA PCM ABI self-test PASSED");
    Ok(())
}
