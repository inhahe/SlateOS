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

/// The only PCM subformat in general use — "standard" linear samples.
pub const SNDRV_PCM_SUBFORMAT_STD: u32 = 0;

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
/// Release a previously committed hardware configuration (no payload).
pub const SNDRV_PCM_IOCTL_HW_FREE: u32 = io(0x12);
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
// Byte-exact `#[repr(C)]` mirrors of the `asound.h` PCM payload structs
// ---------------------------------------------------------------------------
//
// The struct-carrying ioctls below encode `sizeof(struct)` in their request
// number, so these layouts must be byte-identical to Linux's on a 64-bit
// target or real ALSA-lib's ioctl number never matches ours.  Each struct's
// size is asserted against its authoritative Linux value in `self_test`, and
// the ioctl numbers are derived from `size_of` (not hand-typed) so they stay
// consistent with the layout.  `snd_pcm_uframes_t` / `snd_pcm_sframes_t` are
// `unsigned long` / `signed long` = 8 bytes here.

/// `snd_pcm_uframes_t` — an unsigned frame count/position (`unsigned long`).
pub type SndPcmUframes = u64;
/// `snd_pcm_sframes_t` — a signed frame count/delay (`signed long`).
pub type SndPcmSframes = i64;

/// `struct snd_mask` — a configuration-space bitmask (256 bits → `u32[8]`,
/// 32 bytes).  Used for the ACCESS/FORMAT/SUBFORMAT parameter masks.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SndMask {
    /// 256 candidate bits, one per possible enum value of the parameter.
    pub bits: [u32; 8],
}

/// `struct snd_interval` — a `[min, max]` range with open/closed/integer/
/// empty flags packed into one word (12 bytes).  Used for the numeric
/// hardware parameters (rate, channels, period size, …).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SndInterval {
    /// Inclusive (unless `openmin`) lower bound.
    pub min: u32,
    /// Inclusive (unless `openmax`) upper bound.
    pub max: u32,
    /// Bitfield word: bit0 `openmin`, bit1 `openmax`, bit2 `integer`,
    /// bit3 `empty`.  Modelled as a plain word for ABI exactness.
    pub flags: u32,
}

/// Index of the first/last mask parameter (`ACCESS`..=`SUBFORMAT`) → 3
/// masks; and the first/last interval parameter (`SAMPLE_BITS`..=
/// `TICK_TIME`) → 12 intervals.  Encoded as array lengths below.
///
/// `struct snd_pcm_hw_params` — the hardware-parameter negotiation
/// payload for `HW_REFINE` / `HW_PARAMS` (608 bytes).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SndPcmHwParams {
    /// `SNDRV_PCM_HW_PARAMS_*` flags.
    pub flags: u32,
    /// Parameter masks (`ACCESS`, `FORMAT`, `SUBFORMAT`).
    pub masks: [SndMask; 3],
    /// Reserved masks.
    pub mres: [SndMask; 5],
    /// Numeric parameter intervals (`SAMPLE_BITS`..=`TICK_TIME`).
    pub intervals: [SndInterval; 12],
    /// Reserved intervals.
    pub ires: [SndInterval; 9],
    /// Mask of parameters to refine (request).
    pub rmask: u32,
    /// Mask of parameters that changed (reply).
    pub cmask: u32,
    /// `SNDRV_PCM_INFO_*` capability flags (reply).
    pub info: u32,
    /// Significant bits in each sample (reply).
    pub msbits: u32,
    /// Exact rate numerator (reply).
    pub rate_num: u32,
    /// Exact rate denominator (reply).
    pub rate_den: u32,
    /// Hardware FIFO size in frames (reply).
    pub fifo_size: SndPcmUframes,
    /// Reserved, must be zero.
    pub reserved: [u8; 64],
}

/// `struct snd_pcm_sw_params` — software-parameter payload for
/// `SW_PARAMS` (136 bytes).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SndPcmSwParams {
    /// Timestamp mode.
    pub tstamp_mode: i32,
    /// Period step.
    pub period_step: u32,
    /// Obsolete minimum sleep ticks.
    pub sleep_min: u32,
    /// Minimum available frames for a wakeup.
    pub avail_min: SndPcmUframes,
    /// Obsolete transfer alignment.
    pub xfer_align: SndPcmUframes,
    /// Minimum `hw_avail` frames for automatic start.
    pub start_threshold: SndPcmUframes,
    /// Minimum available frames for automatic stop.
    pub stop_threshold: SndPcmUframes,
    /// Distance from noise for silence filling.
    pub silence_threshold: SndPcmUframes,
    /// Silence block size.
    pub silence_size: SndPcmUframes,
    /// Pointer wrap-around boundary.
    pub boundary: SndPcmUframes,
    /// Protocol version.
    pub proto: u32,
    /// Timestamp type.
    pub tstamp_type: u32,
    /// Reserved, must be zero.
    pub reserved: [u8; 56],
}

/// `struct snd_xferi` — the interleaved read/write payload for
/// `WRITEI_FRAMES` / `READI_FRAMES` (24 bytes).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SndXferi {
    /// Frames transferred (reply) — `snd_pcm_sframes_t`.
    pub result: SndPcmSframes,
    /// User pointer to the frame buffer (stored as an integer address).
    pub buf: u64,
    /// Frames requested (request).
    pub frames: SndPcmUframes,
}

/// `struct snd_pcm_info` — device-identification payload for `INFO`
/// (288 bytes).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SndPcmInfo {
    /// Device number.
    pub device: u32,
    /// Subdevice number.
    pub subdevice: u32,
    /// Stream direction (`SNDRV_PCM_STREAM_*`).
    pub stream: i32,
    /// Card number.
    pub card: i32,
    /// User-selectable ID string.
    pub id: [u8; 64],
    /// Device name.
    pub name: [u8; 80],
    /// Subdevice name.
    pub subname: [u8; 32],
    /// `SNDRV_PCM_CLASS_*`.
    pub dev_class: i32,
    /// `SNDRV_PCM_SUBCLASS_*`.
    pub dev_subclass: i32,
    /// Total subdevices.
    pub subdevices_count: u32,
    /// Available subdevices.
    pub subdevices_avail: u32,
    /// Hardware sync ID (`union snd_pcm_sync_id`, 16 bytes).
    pub sync: [u8; 16],
    /// Reserved, must be zero.
    pub reserved: [u8; 64],
}

/// `struct snd_pcm_mmap_status` — the read-only status page ALSA exposes for a
/// substream: the current state, the hardware pointer, and reference
/// timestamps (56 bytes on a 64-bit target).
///
/// Only `state` and `hw_ptr` are meaningful to us; the timestamps are zeroed
/// (we do not yet drive a monotonic audio clock).  The struct is always
/// embedded in a 64-byte union inside [`SndPcmSyncPtr`], so its exact tail
/// layout never affects an ioctl request number — but we mirror it faithfully
/// so a future status-timestamp implementation drops in without an ABI change.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SndPcmMmapStatus {
    /// `SNDRV_PCM_STATE_*` (RO).
    pub state: i32,
    /// Padding for 64-bit alignment of `hw_ptr`.
    pub pad1: i32,
    /// Hardware pointer, `0..boundary` (RO) — frames the device has consumed.
    pub hw_ptr: SndPcmUframes,
    /// Reference timestamp (`struct timespec`: 2 × 64-bit on this target).
    pub tstamp: [u64; 2],
    /// Suspended-stream state (RO).
    pub suspended_state: i32,
    /// Padding for 64-bit alignment of `audio_tstamp`.
    pub pad2: i32,
    /// Audio timestamp (`struct timespec`).
    pub audio_tstamp: [u64; 2],
}

/// `struct snd_pcm_mmap_control` — the read/write control page: the
/// application pointer and the wakeup threshold (16 bytes).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SndPcmMmapControl {
    /// Application pointer, `0..boundary` (RW) — frames the app has submitted.
    pub appl_ptr: SndPcmUframes,
    /// Minimum available frames for a poll wakeup (RW).
    pub avail_min: SndPcmUframes,
}

/// `struct snd_pcm_sync_ptr` — the `SYNC_PTR` payload: a flags word plus the
/// status and control pages, each padded out to a 64-byte union (136 bytes).
///
/// ALSA-lib's kernel-backed PCM plugin issues this ioctl on every period to
/// pull the hardware pointer and push/pull the application pointer when it
/// cannot mmap the status/control pages directly (our case — we do not export
/// those pages).  Because both pages sit in a `union { …; unsigned char
/// reserved[64]; }`, the overall size is independent of the timestamp ABI, so
/// this layout is byte-exact on every 64-bit target.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SndPcmSyncPtr {
    /// `SNDRV_PCM_SYNC_PTR_*` request flags.
    pub flags: u32,
    /// Padding to 8-byte-align the status union.
    pub pad: u32,
    /// `union s`: the status page (RO fields filled by the kernel).
    pub status: SndPcmMmapStatus,
    /// Tail padding of `union s` to a full 64 bytes.
    pub status_pad: [u8; 8],
    /// `union c`: the control page (RW appl_ptr / avail_min).
    pub control: SndPcmMmapControl,
    /// Tail padding of `union c` to a full 64 bytes.
    pub control_pad: [u8; 48],
}

/// `SYNC_PTR` flag: execute a hardware-pointer sync before reading.
pub const SNDRV_PCM_SYNC_PTR_HWSYNC: u32 = 1 << 0;
/// `SYNC_PTR` flag: return the kernel's `appl_ptr` instead of adopting the
/// caller's (a read of the application pointer rather than a push).
pub const SNDRV_PCM_SYNC_PTR_APPL: u32 = 1 << 1;
/// `SYNC_PTR` flag: return the kernel's `avail_min` instead of adopting the
/// caller's.
pub const SNDRV_PCM_SYNC_PTR_AVAIL_MIN: u32 = 1 << 2;

/// `struct snd_pcm_status` — the `STATUS` / `STATUS_EXT` payload (152 bytes on
/// a 64-bit-`time_t` target).
///
/// Unlike [`SndPcmSyncPtr`] (whose pages sit in 64-byte unions), this struct
/// embeds bare `struct timespec`s directly, so its `sizeof` — and therefore the
/// `_IOR`/`_IOWR` request number — depends on the `time_t` width.  SlateOS is a
/// **time64** OS (64-bit `time_t`, so `struct timespec` is 16 bytes), which is
/// the only sane choice for a new 64-bit target (32-bit `time_t` is the
/// Y2038-unsafe legacy path).  A modern 64-bit ALSA-lib is compiled against
/// exactly this layout, so it is byte-for-byte compatible with the request
/// numbers below.  The upstream userspace definition (`asound.h`):
///
/// ```c
/// struct snd_pcm_status {
///   snd_pcm_state_t   state;              // int
///   __time_pad        pad1;               // pad[sizeof(time_t)-sizeof(int)] = 4
///   struct timespec   trigger_tstamp;     // 16
///   struct timespec   tstamp;             // 16  (reference)
///   snd_pcm_uframes_t appl_ptr;           // 8
///   snd_pcm_uframes_t hw_ptr;             // 8
///   snd_pcm_sframes_t delay;              // 8   (current delay in frames)
///   snd_pcm_uframes_t avail;              // 8
///   snd_pcm_uframes_t avail_max;          // 8
///   snd_pcm_uframes_t overrange;          // 8
///   snd_pcm_state_t   suspended_state;    // int
///   __u32             audio_tstamp_data;  // 4
///   struct timespec   audio_tstamp;       // 16
///   struct timespec   driver_tstamp;      // 16
///   __u32             audio_tstamp_accuracy; // 4
///   unsigned char     reserved[52-2*sizeof(struct timespec)]; // 20
/// };
/// ```
///
/// `struct timespec` is modelled as `[u64; 2]` (`tv_sec`, `tv_nsec`), matching
/// the rest of this file (see [`SndPcmMmapStatus::tstamp`]).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SndPcmStatus {
    /// `SNDRV_PCM_STATE_*` stream state.
    pub state: i32,
    /// `__time_pad`: `sizeof(time_t) - sizeof(int)` = 4 bytes, aligning the
    /// following `timespec` to 8.
    pub pad1: i32,
    /// Time the stream was last started/stopped/paused (`struct timespec`).
    pub trigger_tstamp: [u64; 2],
    /// Reference timestamp for `hw_ptr` (`struct timespec`).
    pub tstamp: [u64; 2],
    /// Application pointer (frames submitted, reduced modulo boundary).
    pub appl_ptr: SndPcmUframes,
    /// Hardware pointer (frames consumed, reduced modulo boundary).
    pub hw_ptr: SndPcmUframes,
    /// Current delay in frames (`appl_ptr - hw_ptr`, i.e. frames still queued).
    pub delay: SndPcmSframes,
    /// Frames available (playback: free buffer space; capture: readable frames).
    pub avail: SndPcmUframes,
    /// Max frames available since the last status (peak `avail`).
    pub avail_max: SndPcmUframes,
    /// Count of capture overrange detections since the last status (always 0 —
    /// our capture path is synthesised silence, never overruns).
    pub overrange: SndPcmUframes,
    /// `SNDRV_PCM_STATE_*` suspended state.
    pub suspended_state: i32,
    /// Audio-timestamp report/config selector (`STATUS_EXT`); echoed back.
    pub audio_tstamp_data: u32,
    /// Sample-counter / wall-clock audio timestamp (`struct timespec`).
    pub audio_tstamp: [u64; 2],
    /// Driver timestamp (`struct timespec`).
    pub driver_tstamp: [u64; 2],
    /// Audio-timestamp accuracy in ns (0 — not reported).
    pub audio_tstamp_accuracy: u32,
    /// Reserved tail (`52 - 2*sizeof(struct timespec)` = 20 bytes), zeroed.
    pub reserved: [u8; 20],
}

/// Interval slot for `SNDRV_PCM_HW_PARAM_BUFFER_SIZE` (ring size in frames).
///
/// The intervals array is indexed by `SNDRV_PCM_HW_PARAM_<x> -
/// SNDRV_PCM_HW_PARAM_SAMPLE_BITS`; `BUFFER_SIZE` (17) − `SAMPLE_BITS` (8) = 9.
const IV_BUFFER_SIZE: usize = 9;

/// Extract the client's chosen ring buffer size (in frames) from a committed
/// `HW_PARAMS` payload.
///
/// [`refine_to_native`] deliberately leaves the buffer/period intervals
/// untouched (the client picks them freely), so at `HW_PARAMS` commit time the
/// `BUFFER_SIZE` interval has been narrowed by the client to a single value.
/// We report `min` (== `max` for a fixed interval); `0` means "not negotiated"
/// and the caller should treat the buffer size as unknown.
#[must_use]
pub fn buffer_size_frames(params: &SndPcmHwParams) -> u64 {
    match params.intervals.get(IV_BUFFER_SIZE) {
        Some(iv) => u64::from(iv.min),
        None => 0,
    }
}

/// Size of an ALSA payload struct as a `u32` for ioctl-number encoding.
///
/// Every struct here is well under the 14-bit `_IOC` size field (max
/// 16383 bytes), so the cast cannot truncate; the bound is checked in
/// `self_test`.
#[allow(clippy::cast_possible_truncation)]
const fn struct_size<T>() -> u32 {
    core::mem::size_of::<T>() as u32
}

// --- Struct-carrying PCM ioctls (request number includes sizeof) ----------

/// `HW_REFINE` — probe/refine the hardware-parameter space (`_IOWR`).
pub const SNDRV_PCM_IOCTL_HW_REFINE: u32 = ioc(
    IOC_READ | IOC_WRITE,
    SNDRV_PCM_IOCTL_MAGIC,
    0x10,
    struct_size::<SndPcmHwParams>(),
);
/// `HW_PARAMS` — commit a hardware configuration (`_IOWR`).
pub const SNDRV_PCM_IOCTL_HW_PARAMS: u32 = ioc(
    IOC_READ | IOC_WRITE,
    SNDRV_PCM_IOCTL_MAGIC,
    0x11,
    struct_size::<SndPcmHwParams>(),
);
/// `SW_PARAMS` — set software parameters (`_IOWR`).
pub const SNDRV_PCM_IOCTL_SW_PARAMS: u32 = ioc(
    IOC_READ | IOC_WRITE,
    SNDRV_PCM_IOCTL_MAGIC,
    0x13,
    struct_size::<SndPcmSwParams>(),
);
/// `WRITEI_FRAMES` — write interleaved frames (`_IOW`).
pub const SNDRV_PCM_IOCTL_WRITEI_FRAMES: u32 =
    ioc(IOC_WRITE, SNDRV_PCM_IOCTL_MAGIC, 0x50, struct_size::<SndXferi>());
/// `READI_FRAMES` — read interleaved frames (`_IOR`).
pub const SNDRV_PCM_IOCTL_READI_FRAMES: u32 =
    ioc(IOC_READ, SNDRV_PCM_IOCTL_MAGIC, 0x51, struct_size::<SndXferi>());
/// `INFO` — query device identification (`_IOR`).
pub const SNDRV_PCM_IOCTL_INFO: u32 =
    ioc(IOC_READ, SNDRV_PCM_IOCTL_MAGIC, 0x01, struct_size::<SndPcmInfo>());
/// `SYNC_PTR` — exchange the application/hardware pointers (`_IOWR`).
pub const SNDRV_PCM_IOCTL_SYNC_PTR: u32 =
    ioc(IOC_READ | IOC_WRITE, SNDRV_PCM_IOCTL_MAGIC, 0x23, struct_size::<SndPcmSyncPtr>());
/// `STATUS` — read the full stream status snapshot (`_IOR`).
///
/// Time64 layout → `sizeof(snd_pcm_status)` = 152, so this encodes to
/// `0x8098_4120` (asserted in [`self_test`]).
pub const SNDRV_PCM_IOCTL_STATUS: u32 =
    ioc(IOC_READ, SNDRV_PCM_IOCTL_MAGIC, 0x20, struct_size::<SndPcmStatus>());
/// `STATUS_EXT` — like `STATUS` but read-write (the client selects an
/// audio-timestamp type in `audio_tstamp_data`); `_IOWR`, `0xC098_4124`.
pub const SNDRV_PCM_IOCTL_STATUS_EXT: u32 =
    ioc(IOC_READ | IOC_WRITE, SNDRV_PCM_IOCTL_MAGIC, 0x24, struct_size::<SndPcmStatus>());

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
// HW_REFINE / HW_PARAMS configuration-space refinement
// ---------------------------------------------------------------------------
//
// ALSA-lib negotiates a hardware configuration by sending a `snd_pcm_hw_params`
// whose masks/intervals describe the *space* of configurations it can accept,
// then asking the kernel to narrow that space (`HW_REFINE`) and finally to
// commit a single point in it (`HW_PARAMS`).  Because our backend is the fixed
// 48 kHz / S16_LE / stereo software mixer, every refine collapses the space to
// exactly that one configuration; a conforming client then converges its own
// parameters onto it.  [`refine_to_native`] performs that collapse in place.
//
// The masks index 0/1/2 are ACCESS / FORMAT / SUBFORMAT; the intervals array is
// indexed by `SNDRV_PCM_HW_PARAM_<x> - SNDRV_PCM_HW_PARAM_SAMPLE_BITS`, so the
// first four interval slots are SAMPLE_BITS / FRAME_BITS / CHANNELS / RATE.

/// Interval slot for `SNDRV_PCM_HW_PARAM_SAMPLE_BITS` (bits per sample).
const IV_SAMPLE_BITS: usize = 0;
/// Interval slot for `SNDRV_PCM_HW_PARAM_FRAME_BITS` (bits per frame).
const IV_FRAME_BITS: usize = 1;
/// Interval slot for `SNDRV_PCM_HW_PARAM_CHANNELS`.
const IV_CHANNELS: usize = 2;
/// Interval slot for `SNDRV_PCM_HW_PARAM_RATE` (Hz).
const IV_RATE: usize = 3;

/// `snd_interval` flag bit marking the interval as integer-valued.
const SNDRV_INTERVAL_INTEGER: u32 = 0b100;

/// Capability flags we advertise for the substream (`SNDRV_PCM_INFO_*`):
/// interleaved RW transfer, block (period) transfer, and pause support.
/// `INTERLEAVED (0x100) | BLOCK_TRANSFER (0x10000) | PAUSE (0x80000)`.
const PCM_INFO_NATIVE: u32 = 0x0009_0100;

/// A `snd_mask` with exactly one candidate bit set.
///
/// `bit` is the enum value of the parameter (e.g. `SNDRV_PCM_FORMAT_S16_LE`).
/// All values we set are well under 256, so the word index is always in range;
/// an out-of-range bit yields an all-zero (empty) mask rather than panicking.
fn mask_bit(bit: u32) -> SndMask {
    let mut m = SndMask { bits: [0u32; 8] };
    let word = (bit / 32) as usize;
    let shift = bit % 32;
    if let Some(w) = m.bits.get_mut(word) {
        *w = 1u32 << shift;
    }
    m
}

/// Pin one numeric interval to a single integer value `[v, v]`.
fn set_interval_fixed(params: &mut SndPcmHwParams, slot: usize, v: u32) {
    if let Some(iv) = params.intervals.get_mut(slot) {
        iv.min = v;
        iv.max = v;
        iv.flags = SNDRV_INTERVAL_INTEGER;
    }
}

/// Collapse a `snd_pcm_hw_params` configuration space onto the mixer's native
/// 48 kHz / S16_LE / stereo, interleaved-RW configuration, in place.
///
/// This is the shared core of both `HW_REFINE` (probe) and `HW_PARAMS`
/// (commit): it fixes ACCESS to RW-interleaved, FORMAT to S16_LE, SUBFORMAT to
/// STD, and the sample-bits / frame-bits / channels / rate intervals to their
/// single native values, then fills the reply fields (`cmask`, `info`,
/// `msbits`, exact rate).  Period- and buffer-sizing intervals are deliberately
/// left untouched so the client picks them freely within its own bounds — our
/// write path accepts whatever period/buffer geometry results.
pub fn refine_to_native(params: &mut SndPcmHwParams) {
    if let Some(m) = params.masks.get_mut(0) {
        *m = mask_bit(SNDRV_PCM_ACCESS_RW_INTERLEAVED);
    }
    if let Some(m) = params.masks.get_mut(1) {
        *m = mask_bit(SNDRV_PCM_FORMAT_S16_LE);
    }
    if let Some(m) = params.masks.get_mut(2) {
        *m = mask_bit(SNDRV_PCM_SUBFORMAT_STD);
    }
    set_interval_fixed(params, IV_SAMPLE_BITS, 16);
    set_interval_fixed(params, IV_FRAME_BITS, 32);
    set_interval_fixed(params, IV_CHANNELS, MIXER_CHANNELS);
    set_interval_fixed(params, IV_RATE, MIXER_RATE);
    // Report that the refine touched the requested parameters and advertise
    // the fixed exact rate and sample width.
    params.cmask = params.rmask;
    params.info = PCM_INFO_NATIVE;
    params.msbits = 16;
    params.rate_num = MIXER_RATE;
    params.rate_den = 1;
    params.fifo_size = 0;
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

    // --- byte-exact struct layouts vs Linux asound.h --------------------
    use core::mem::size_of;
    check!(size_of::<SndMask>() == 32, "snd_mask size {}", size_of::<SndMask>());
    check!(
        size_of::<SndInterval>() == 12,
        "snd_interval size {}",
        size_of::<SndInterval>()
    );
    check!(
        size_of::<SndPcmHwParams>() == 608,
        "snd_pcm_hw_params size {}",
        size_of::<SndPcmHwParams>()
    );
    check!(
        size_of::<SndPcmSwParams>() == 136,
        "snd_pcm_sw_params size {}",
        size_of::<SndPcmSwParams>()
    );
    check!(size_of::<SndXferi>() == 24, "snd_xferi size {}", size_of::<SndXferi>());
    check!(
        size_of::<SndPcmInfo>() == 288,
        "snd_pcm_info size {}",
        size_of::<SndPcmInfo>()
    );
    check!(
        size_of::<SndPcmMmapStatus>() == 56,
        "snd_pcm_mmap_status size {}",
        size_of::<SndPcmMmapStatus>()
    );
    check!(
        size_of::<SndPcmMmapControl>() == 16,
        "snd_pcm_mmap_control size {}",
        size_of::<SndPcmMmapControl>()
    );
    check!(
        size_of::<SndPcmSyncPtr>() == 136,
        "snd_pcm_sync_ptr size {}",
        size_of::<SndPcmSyncPtr>()
    );
    // time64 layout: 4+4 + 16+16 + 8*3 + 8*3 + 4+4 + 16+16 + 4 + 20 = 152.
    check!(
        size_of::<SndPcmStatus>() == 152,
        "snd_pcm_status size {}",
        size_of::<SndPcmStatus>()
    );

    // --- struct-carrying ioctls vs known Linux hex (size-derived) -------
    check!(
        SNDRV_PCM_IOCTL_HW_REFINE == 0xC260_4110,
        "HW_REFINE enc {:#x}",
        SNDRV_PCM_IOCTL_HW_REFINE
    );
    check!(
        SNDRV_PCM_IOCTL_HW_PARAMS == 0xC260_4111,
        "HW_PARAMS enc {:#x}",
        SNDRV_PCM_IOCTL_HW_PARAMS
    );
    check!(
        SNDRV_PCM_IOCTL_SW_PARAMS == 0xC088_4113,
        "SW_PARAMS enc {:#x}",
        SNDRV_PCM_IOCTL_SW_PARAMS
    );
    check!(
        SNDRV_PCM_IOCTL_WRITEI_FRAMES == 0x4018_4150,
        "WRITEI_FRAMES enc {:#x}",
        SNDRV_PCM_IOCTL_WRITEI_FRAMES
    );
    check!(
        SNDRV_PCM_IOCTL_READI_FRAMES == 0x8018_4151,
        "READI_FRAMES enc {:#x}",
        SNDRV_PCM_IOCTL_READI_FRAMES
    );
    check!(
        SNDRV_PCM_IOCTL_INFO == 0x8120_4101,
        "INFO enc {:#x}",
        SNDRV_PCM_IOCTL_INFO
    );
    check!(
        SNDRV_PCM_IOCTL_SYNC_PTR == 0xC088_4123,
        "SYNC_PTR enc {:#x}",
        SNDRV_PCM_IOCTL_SYNC_PTR
    );
    check!(
        SNDRV_PCM_IOCTL_STATUS == 0x8098_4120,
        "STATUS enc {:#x}",
        SNDRV_PCM_IOCTL_STATUS
    );
    check!(
        SNDRV_PCM_IOCTL_STATUS_EXT == 0xC098_4124,
        "STATUS_EXT enc {:#x}",
        SNDRV_PCM_IOCTL_STATUS_EXT
    );

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

    // --- HW_REFINE collapse onto native config --------------------------
    // Start from a fully-open params (all-ones masks, wide intervals) as
    // ALSA-lib's first refine pass would, then collapse it.
    // SAFETY: SndPcmHwParams is a plain `#[repr(C)]` aggregate of integers and
    // integer arrays with no padding invariants or niches, so an all-zero bit
    // pattern is a valid, fully-initialised value.
    let mut hwp: SndPcmHwParams = unsafe { core::mem::zeroed() };
    for m in &mut hwp.masks {
        m.bits = [0xffff_ffff; 8];
    }
    for iv in &mut hwp.intervals {
        iv.min = 1;
        iv.max = u32::MAX;
        iv.flags = 0;
    }
    hwp.rmask = 0xffff_ffff;
    refine_to_native(&mut hwp);
    // Helper: a refined interval pinned to [v, v] and marked integer.
    let interval_pinned = |slot: usize, v: u32| -> bool {
        hwp.intervals.get(slot).is_some_and(|iv| {
            iv.min == v && iv.max == v && iv.flags & SNDRV_INTERVAL_INTEGER != 0
        })
    };
    let mask_is = |slot: usize, bit: u32| -> bool {
        hwp.masks.get(slot).is_some_and(|m| m.bits == mask_bit(bit).bits)
    };
    check!(
        mask_is(0, SNDRV_PCM_ACCESS_RW_INTERLEAVED),
        "ACCESS collapsed to RW_INTERLEAVED"
    );
    check!(mask_is(1, SNDRV_PCM_FORMAT_S16_LE), "FORMAT collapsed to S16_LE");
    check!(mask_is(2, SNDRV_PCM_SUBFORMAT_STD), "SUBFORMAT collapsed to STD");
    check!(interval_pinned(IV_RATE, 48000), "RATE pinned to 48000 (integer)");
    check!(interval_pinned(IV_CHANNELS, 2), "CHANNELS pinned to 2");
    check!(interval_pinned(IV_SAMPLE_BITS, 16), "SAMPLE_BITS pinned to 16");
    check!(interval_pinned(IV_FRAME_BITS, 32), "FRAME_BITS pinned to 32");
    check!(hwp.cmask == 0xffff_ffff, "cmask echoes rmask");
    check!(hwp.msbits == 16, "msbits = 16");
    check!(hwp.rate_num == 48000 && hwp.rate_den == 1, "exact rate 48000/1");

    serial_println!("[alsa] ALSA PCM ABI self-test PASSED");
    Ok(())
}
