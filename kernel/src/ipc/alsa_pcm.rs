//! ALSA PCM instance objects — the per-open kernel state behind a
//! `/dev/snd/pcmC0D0p` file descriptor.
//!
//! When a Linux audio application (ALSA-lib, and everything layered on it —
//! PulseAudio, PipeWire, JACK, SDL, WINE) opens `/dev/snd/pcmC0D0p` it gets a
//! file descriptor that it then drives entirely through `ioctl(2)`:
//! `SNDRV_PCM_IOCTL_HW_REFINE` / `HW_PARAMS` to negotiate a hardware
//! configuration, `PREPARE` / `START` to arm the stream, `WRITEI_FRAMES`
//! (or plain `write(2)`) to push interleaved PCM frames, and
//! `DROP` / `DRAIN` to stop.  Each open is an independent *substream* with its
//! own state machine and its own slot in the software mixer.
//!
//! This module owns the **instance object** that a `HandleKind::AlsaPcm`
//! [`crate::proc::linux_fd::FdEntry`] points at: the PCM state-machine state,
//! the negotiated frame format, and the mixer [`StreamId`] this substream
//! feeds.  It mirrors the refcounted-instance pattern used by
//! [`crate::ipc::timerfd`] / [`crate::ipc::eventfd`] / [`crate::ipc::signalfd`]:
//! `create()` starts the count at 1, `dup()` bumps it (so `fork` can let parent
//! and child share one substream), and only the final `close()` (count → 0)
//! tears the object down — releasing the mixer stream it held.
//!
//! ## What lives here vs. in `audio_alsa`
//!
//! [`crate::audio_alsa`] holds the *ABI*: the ioctl numbers, the `#[repr(C)]`
//! hardware-parameter / sw-parameter / xfer structs, and the format-translation
//! helpers.  This module holds the *live state* of one open substream.  The
//! ioctl-dispatch glue that reads a request struct, validates it, and drives
//! the transitions here lands in the syscall layer (a later commit); commit 3
//! is just the instance object plus its fd-family wiring.
//!
//! ## State machine
//!
//! A playback substream walks the ALSA state graph:
//!
//! ```text
//!   OPEN ──HW_PARAMS──▶ SETUP ──PREPARE──▶ PREPARED ──START──▶ RUNNING
//!     ▲                   ▲                    │                  │
//!     │                HW_FREE             (write frames)     DROP/DRAIN
//!     └───────────────────┴────────────────────┴──────────────────┘
//! ```
//!
//! The mixer [`StreamId`] is acquired when the substream leaves `OPEN` for
//! `SETUP` (a successful `HW_PARAMS`) and released on the final `close()` (or an
//! explicit `HW_FREE` back to `OPEN`).  Holding the slot across the whole
//! configured lifetime — rather than per-`write` — matches how ALSA hardware
//! substreams reserve a DMA channel for the duration they are set up.
//!
//! ## Lock ordering
//!
//! `ALSA_PCM_TABLE` is a leaf lock with one deliberate exception: the mixer
//! must be released when an instance is destroyed.  To avoid nesting the mixer
//! lock under the table lock, [`close`] removes the dying instance from the
//! table, drops the table lock, and *then* calls
//! [`crate::audio_mixer::close_stream`].  No path holds `ALSA_PCM_TABLE` across
//! a mixer call.
//!
//! Some state constants and accessors here are consumed by the ioctl-dispatch
//! glue that lands in a later commit, so the whole module allows `dead_code`
//! until that wiring is in place (mirroring [`crate::audio_alsa`]).
#![allow(dead_code)]

use alloc::collections::BTreeMap;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

use crate::audio_mixer::{self, StreamId};
use crate::error::{KernelError, KernelResult};
use crate::serial_println;

// ---------------------------------------------------------------------------
// PCM states (mirrors SNDRV_PCM_STATE_* — see audio_alsa)
// ---------------------------------------------------------------------------

/// `SNDRV_PCM_STATE_OPEN` — freshly opened, no hw params yet.
pub const STATE_OPEN: u32 = 0;
/// `SNDRV_PCM_STATE_SETUP` — hw params accepted, not yet prepared.
pub const STATE_SETUP: u32 = 1;
/// `SNDRV_PCM_STATE_PREPARED` — ready to start.
pub const STATE_PREPARED: u32 = 2;
/// `SNDRV_PCM_STATE_RUNNING` — transferring frames.
pub const STATE_RUNNING: u32 = 3;
/// `SNDRV_PCM_STATE_XRUN` — buffer under/overran; needs re-prepare.
pub const STATE_XRUN: u32 = 4;
/// `SNDRV_PCM_STATE_DRAINING` — playing out the remaining buffered frames.
pub const STATE_DRAINING: u32 = 5;
/// `SNDRV_PCM_STATE_PAUSED` — running but paused.
pub const STATE_PAUSED: u32 = 6;

// ---------------------------------------------------------------------------
// Handle
// ---------------------------------------------------------------------------

/// Unique ID for an ALSA PCM instance (the handle IS the ID).
type AlsaPcmId = u64;

/// Monotonic ID generator.  Starts at 1 so 0 is never a valid handle.
static NEXT_ALSA_PCM_ID: AtomicU64 = AtomicU64::new(1);

fn alloc_alsa_pcm_id() -> AlsaPcmId {
    NEXT_ALSA_PCM_ID.fetch_add(1, Ordering::Relaxed)
}

/// A handle to an ALSA PCM substream instance.
///
/// Wraps the instance ID.  Stored in a Linux `FdEntry` as a raw `u64` (the
/// `HandleKind::AlsaPcm` variant); the syscall layer reconstructs it with
/// [`AlsaPcmHandle::from_raw`] on each ioctl / write / poll / close.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AlsaPcmHandle(u64);

impl AlsaPcmHandle {
    /// Reconstruct a handle from its raw `u64` representation.
    #[must_use]
    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    /// The raw `u64` representation (what gets stored in an `FdEntry`).
    #[must_use]
    pub const fn raw(self) -> u64 {
        self.0
    }

    fn id(self) -> AlsaPcmId {
        self.0
    }
}

// ---------------------------------------------------------------------------
// Instance
// ---------------------------------------------------------------------------

/// A kernel ALSA PCM substream instance.
struct PcmStream {
    /// Whether this is a playback (false) or capture (true) substream.
    capture: bool,
    /// Current ALSA state machine state (`STATE_*`).
    state: u32,
    /// Negotiated sample format (`SNDRV_PCM_FORMAT_*`); `None` until `HW_PARAMS`.
    format: Option<u32>,
    /// Negotiated sample rate in Hz; `None` until `HW_PARAMS`.
    rate: Option<u32>,
    /// Negotiated channel count; `None` until `HW_PARAMS`.
    channels: Option<u32>,
    /// Mixer slot this substream feeds; `Some` once configured (`HW_PARAMS`),
    /// `None` in the `OPEN` / freed state.
    mixer_stream: Option<StreamId>,
    /// Total frames the application has submitted since the last `PREPARE`
    /// (the ALSA `appl_ptr` hardware pointer, modulo the boundary).  Used by
    /// `STATUS` / `SYNC_PTR` reporting in a later commit.
    frames_written: u64,
    /// Reference count: `create` = 1, each `dup` +1, each `close` −1.
    refcount: u32,
}

impl PcmStream {
    const fn new(capture: bool) -> Self {
        Self {
            capture,
            state: STATE_OPEN,
            format: None,
            rate: None,
            channels: None,
            mixer_stream: None,
            frames_written: 0,
            refcount: 1,
        }
    }
}

// ---------------------------------------------------------------------------
// Global table
// ---------------------------------------------------------------------------

/// Global table of all live ALSA PCM instances, keyed by ID.
///
/// Leaf lock — never held across a call into [`crate::audio_mixer`] (see the
/// module-level "Lock ordering" note).
static ALSA_PCM_TABLE: Mutex<BTreeMap<AlsaPcmId, PcmStream>> = Mutex::new(BTreeMap::new());

// ---------------------------------------------------------------------------
// Lifetime API
// ---------------------------------------------------------------------------

/// Create a new ALSA PCM substream instance in the `OPEN` state.
///
/// `capture` selects the stream direction (`false` = playback). The returned
/// handle owns one reference; the caller must `close()` it (directly or via
/// process-exit cleanup) exactly once for that reference.
#[must_use]
pub fn create(capture: bool) -> AlsaPcmHandle {
    let id = alloc_alsa_pcm_id();
    ALSA_PCM_TABLE.lock().insert(id, PcmStream::new(capture));
    AlsaPcmHandle(id)
}

/// Add one reference to a PCM instance, returning the same handle.
///
/// Used when `fork` duplicates the inheriting fd: parent and child then each
/// hold a reference to the *same* substream (shared state and mixer slot), and
/// neither one's `close()` invalidates the other's.
///
/// # Errors
///
/// [`KernelError::InvalidHandle`] if the instance no longer exists (already
/// fully closed) or the reference count would overflow `u32::MAX`.
pub fn dup(handle: AlsaPcmHandle) -> KernelResult<AlsaPcmHandle> {
    let mut table = ALSA_PCM_TABLE.lock();
    let pcm = table
        .get_mut(&handle.id())
        .ok_or(KernelError::InvalidHandle)?;
    pcm.refcount = pcm.refcount.checked_add(1).ok_or(KernelError::InvalidHandle)?;
    Ok(handle)
}

/// Drop one reference to a PCM instance.
///
/// Only the final `close()` (refcount → 0) removes the instance and releases
/// its mixer slot.  A double-close is harmless: the saturating decrement floors
/// at 0 and an unknown handle is simply ignored.
///
/// The mixer stream is released *after* the table lock is dropped, so this path
/// never nests the mixer lock under `ALSA_PCM_TABLE`.
pub fn close(handle: AlsaPcmHandle) {
    // Decide what to do under the table lock, but defer the mixer call.
    let mixer_to_free: Option<StreamId> = {
        let mut table = ALSA_PCM_TABLE.lock();
        match table.get_mut(&handle.id()) {
            None => None,
            Some(pcm) => {
                pcm.refcount = pcm.refcount.saturating_sub(1);
                if pcm.refcount == 0 {
                    // Take ownership of the instance so the mixer stream is
                    // released exactly once, then remove it from the table.
                    let stream = pcm.mixer_stream.take();
                    table.remove(&handle.id());
                    stream
                } else {
                    None
                }
            }
        }
    };
    if let Some(sid) = mixer_to_free {
        audio_mixer::close_stream(sid);
    }
}

/// Does this handle refer to a live PCM instance?
#[must_use]
pub fn exists(handle: AlsaPcmHandle) -> bool {
    ALSA_PCM_TABLE.lock().contains_key(&handle.id())
}

/// The direction of a PCM instance (`true` = capture), or `None` if stale.
#[must_use]
pub fn is_capture(handle: AlsaPcmHandle) -> Option<bool> {
    ALSA_PCM_TABLE.lock().get(&handle.id()).map(|p| p.capture)
}

// ---------------------------------------------------------------------------
// State accessors
// ---------------------------------------------------------------------------

/// The current ALSA state-machine state (`STATE_*`), or `None` if stale.
#[must_use]
pub fn state(handle: AlsaPcmHandle) -> Option<u32> {
    ALSA_PCM_TABLE.lock().get(&handle.id()).map(|p| p.state)
}

/// The total frames submitted since the last `PREPARE`, or `None` if stale.
#[must_use]
pub fn frames_written(handle: AlsaPcmHandle) -> Option<u64> {
    ALSA_PCM_TABLE
        .lock()
        .get(&handle.id())
        .map(|p| p.frames_written)
}

/// The negotiated `(format, rate, channels)`, or `None` if not yet configured.
#[must_use]
pub fn params(handle: AlsaPcmHandle) -> Option<(u32, u32, u32)> {
    let table = ALSA_PCM_TABLE.lock();
    let pcm = table.get(&handle.id())?;
    match (pcm.format, pcm.rate, pcm.channels) {
        (Some(f), Some(r), Some(c)) => Some((f, r, c)),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Boot-time self-test of the PCM instance lifecycle.
///
/// Exercises create → dup → close (twice) refcounting and the `exists` /
/// `state` / direction accessors, leaving no instances behind.  Run from the
/// kernel boot self-test sequence; a failure halts the boot so the regression
/// is caught immediately rather than shipping a broken audio path.
///
/// # Errors
///
/// Returns [`KernelError::InternalError`] on the first failed invariant.
pub fn self_test() -> KernelResult<()> {
    macro_rules! check {
        ($cond:expr, $msg:expr) => {
            if !($cond) {
                serial_println!("[alsa_pcm] SELF-TEST FAILED: {}", $msg);
                return Err(KernelError::InternalError);
            }
        };
    }

    // Fresh playback instance starts OPEN and alive.
    let h = create(false);
    check!(exists(h), "new instance must exist");
    check!(state(h) == Some(STATE_OPEN), "new instance must be OPEN");
    check!(is_capture(h) == Some(false), "playback direction");
    check!(frames_written(h) == Some(0), "no frames written yet");
    check!(params(h).is_none(), "no params before HW_PARAMS");

    // dup bumps the refcount: it then takes two closes to free.
    let h2 = dup(h)?;
    check!(h2 == h, "dup returns the same handle");
    close(h);
    check!(exists(h), "still alive after one of two closes");
    close(h);
    check!(!exists(h), "freed after the second close");

    // Capture direction round-trips.
    let c = create(true);
    check!(is_capture(c) == Some(true), "capture direction");
    close(c);
    check!(!exists(c), "capture instance freed");

    // Stale-handle accessors are all None, never a panic.
    check!(state(h).is_none(), "stale state is None");
    check!(is_capture(h).is_none(), "stale direction is None");
    check!(dup(h).is_err(), "dup of a stale handle errors");

    serial_println!("[alsa_pcm] PCM instance lifecycle self-test PASSED");
    Ok(())
}
