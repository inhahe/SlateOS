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
use crate::sync::PreemptSpinMutex as Mutex;

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
    /// (the ALSA `appl_ptr` application pointer, before the boundary reduction
    /// `SYNC_PTR` reporting applies).
    frames_written: u64,
    /// Pointer wrap-around boundary from `SW_PARAMS` (a large multiple of the
    /// buffer size).  `0` means "not set yet"; the position reporting then
    /// returns the raw counters without a modular reduction.
    boundary: u64,
    /// Minimum available frames for a poll wakeup, from `SW_PARAMS`.  Stored so
    /// `SYNC_PTR` can echo it back to the client; we wake on any space today.
    avail_min: u64,
    /// Ring buffer size in frames, from `HW_PARAMS` (`BUFFER_SIZE` interval).
    /// `0` until negotiated; drives the `avail` field of the `STATUS` snapshot.
    buffer_frames: u64,
    /// Monotonic time (ns since boot) the substream last transitioned to
    /// `RUNNING` via `START`; reported as `trigger_tstamp` in `STATUS`.  `0`
    /// until the stream has been started at least once.
    trigger_time_ns: u64,
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
            boundary: 0,
            avail_min: 0,
            buffer_frames: 0,
            trigger_time_ns: 0,
            refcount: 1,
        }
    }
}

/// A position snapshot for `SYNC_PTR` / `STATUS` reporting.
///
/// All pointers are already reduced modulo the substream's boundary (or left
/// raw if no boundary has been set via `SW_PARAMS`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PcmPosition {
    /// Current `STATE_*` state-machine state.
    pub state: u32,
    /// Hardware pointer: frames the mixer has already consumed (`appl_ptr`
    /// minus the frames still queued in the mixer ring).
    pub hw_ptr: u64,
    /// Application pointer: total frames the app has submitted.
    pub appl_ptr: u64,
    /// Minimum available frames for a wakeup (echoed from `SW_PARAMS`).
    pub avail_min: u64,
    /// Current delay in frames: `appl_ptr - hw_ptr` (absolute, pre-boundary
    /// reduction) — i.e. frames still queued in the mixer, what
    /// `snd_pcm_delay(3)` reports.
    pub delay: u64,
    /// Frames available for the next transfer.  Playback: free buffer space
    /// (`buffer_frames - delay`).  Capture: `buffer_frames` (our capture path
    /// is always-ready synthesised silence).  `0` if the buffer size is not
    /// yet negotiated.
    pub avail: u64,
    /// Ring buffer size in frames (`0` if `HW_PARAMS` has not set it).
    pub buffer_frames: u64,
    /// `true` for a capture substream, `false` for playback.
    pub capture: bool,
    /// Monotonic ns of the last `START`, for `trigger_tstamp` (`0` if never
    /// started).
    pub trigger_time_ns: u64,
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
// State-machine transitions (driven by the PCM ioctls)
// ---------------------------------------------------------------------------
//
// These mutate the per-open state in response to `SNDRV_PCM_IOCTL_*`.  They
// operate purely on the instance state and the mixer; the syscall layer owns
// all user-memory copies and translates the returned `KernelError` to an errno.
//
// Lock discipline: any function that must touch the mixer first fetches the
// mixer `StreamId` (and validates the transition) under the table lock, then
// drops the lock before calling into [`crate::audio_mixer`].  No mixer call is
// ever made while `ALSA_PCM_TABLE` is held (see the module "Lock ordering"
// note), so the leaf-lock invariant holds.

/// Commit a hardware configuration (`HW_PARAMS`): validate `(format, rate,
/// channels)` against the mixer's native pipeline, reserve a mixer slot for a
/// playback substream, and move it to `SETUP`.
///
/// Idempotent across repeated `HW_PARAMS` without an intervening `HW_FREE`: the
/// already-reserved mixer slot is reused rather than leaked.
///
/// # Errors
///
/// - [`KernelError::InvalidHandle`] if the instance is stale.
/// - [`KernelError::InvalidArgument`] if the configuration is not the mixer's
///   native 48 kHz / S16_LE / stereo (the wiring advertises only that through
///   `HW_REFINE`, so a conforming ALSA-lib never reaches this).
/// - [`KernelError::WouldBlock`] if no mixer slot is free (propagated from
///   [`crate::audio_mixer::open_stream`]).
pub fn hw_params(
    handle: AlsaPcmHandle,
    format: u32,
    rate: u32,
    channels: u32,
) -> KernelResult<()> {
    if !crate::audio_alsa::mixer_accepts_directly(format, rate, channels) {
        // Confirm the handle exists so a stale fd still reports InvalidHandle
        // rather than masking it as a config error.
        if !exists(handle) {
            return Err(KernelError::InvalidHandle);
        }
        return Err(KernelError::InvalidArgument);
    }

    // Decide whether we still need to reserve a mixer slot, under the lock.
    let (need_stream, capture) = {
        let table = ALSA_PCM_TABLE.lock();
        let pcm = table.get(&handle.id()).ok_or(KernelError::InvalidHandle)?;
        (pcm.mixer_stream.is_none(), pcm.capture)
    };

    // Playback substreams feed the mixer; capture substreams have no mixer
    // slot (the mixer is output-only — capture reads produce silence).
    let new_stream = if need_stream && !capture {
        Some(audio_mixer::open_stream("alsa-pcm")?)
    } else {
        None
    };

    // Re-acquire and apply.  If the instance vanished while the lock was
    // dropped (a concurrent close), release any freshly-opened slot.
    //
    // A redundant slot can also arise without a close: two `hw_params` calls
    // racing on the same handle both observe `mixer_stream == None` under
    // lock #1 and both open a slot.  Whichever re-acquires lock #2 second must
    // *not* overwrite the slot the first one already stored (that would leak
    // it); it keeps the existing slot and frees its own after the lock drops.
    let mut redundant_stream: Option<StreamId> = None;
    {
        let mut table = ALSA_PCM_TABLE.lock();
        let Some(pcm) = table.get_mut(&handle.id()) else {
            drop(table);
            if let Some(sid) = new_stream {
                audio_mixer::close_stream(sid);
            }
            return Err(KernelError::InvalidHandle);
        };
        if let Some(sid) = new_stream {
            if pcm.mixer_stream.is_none() {
                pcm.mixer_stream = Some(sid);
            } else {
                // Lost the race: a concurrent hw_params already reserved a slot.
                redundant_stream = Some(sid);
            }
        }
        pcm.format = Some(format);
        pcm.rate = Some(rate);
        pcm.channels = Some(channels);
        pcm.state = STATE_SETUP;
        pcm.frames_written = 0;
    }
    // Free the redundant slot with the table lock released (leaf-lock invariant).
    if let Some(sid) = redundant_stream {
        audio_mixer::close_stream(sid);
    }
    Ok(())
}

/// Release the hardware configuration (`HW_FREE`): drop the mixer slot and
/// return the substream to `OPEN`.
///
/// The mixer slot is released after the table lock is dropped, preserving the
/// leaf-lock invariant.
///
/// # Errors
///
/// [`KernelError::InvalidHandle`] if the instance is stale.
pub fn hw_free(handle: AlsaPcmHandle) -> KernelResult<()> {
    let mixer_to_free = {
        let mut table = ALSA_PCM_TABLE.lock();
        let pcm = table.get_mut(&handle.id()).ok_or(KernelError::InvalidHandle)?;
        let stream = pcm.mixer_stream.take();
        pcm.format = None;
        pcm.rate = None;
        pcm.channels = None;
        pcm.state = STATE_OPEN;
        pcm.frames_written = 0;
        stream
    };
    if let Some(sid) = mixer_to_free {
        audio_mixer::close_stream(sid);
    }
    Ok(())
}

/// Move the substream to `PREPARED` (`PREPARE`): valid from any configured
/// state, it clears the mixer ring and resets the application pointer.
///
/// # Errors
///
/// - [`KernelError::InvalidHandle`] if the instance is stale.
/// - [`KernelError::InvalidArgument`] if the substream has no committed
///   configuration yet (still `OPEN`).
pub fn prepare(handle: AlsaPcmHandle) -> KernelResult<()> {
    let mixer_to_clear = {
        let mut table = ALSA_PCM_TABLE.lock();
        let pcm = table.get_mut(&handle.id()).ok_or(KernelError::InvalidHandle)?;
        if pcm.state == STATE_OPEN {
            return Err(KernelError::InvalidArgument);
        }
        pcm.state = STATE_PREPARED;
        pcm.frames_written = 0;
        pcm.mixer_stream
    };
    if let Some(sid) = mixer_to_clear {
        audio_mixer::clear(sid);
    }
    Ok(())
}

/// Start the substream running (`START`): `PREPARED` → `RUNNING`.
///
/// # Errors
///
/// - [`KernelError::InvalidHandle`] if the instance is stale.
/// - [`KernelError::InvalidArgument`] if the substream is not `PREPARED`
///   (ALSA returns `-EBADFD` here; we surface it as `EINVAL`).
pub fn start(handle: AlsaPcmHandle) -> KernelResult<()> {
    let mut table = ALSA_PCM_TABLE.lock();
    let pcm = table.get_mut(&handle.id()).ok_or(KernelError::InvalidHandle)?;
    if pcm.state != STATE_PREPARED {
        return Err(KernelError::InvalidArgument);
    }
    pcm.state = STATE_RUNNING;
    // Record the trigger time for the STATUS snapshot's `trigger_tstamp`.
    pcm.trigger_time_ns = crate::timekeeping::clock_monotonic();
    Ok(())
}

/// Stop the substream immediately and discard buffered frames (`DROP`):
/// any configured state → `SETUP`.
///
/// # Errors
///
/// [`KernelError::InvalidHandle`] if the instance is stale.
pub fn drop_stream(handle: AlsaPcmHandle) -> KernelResult<()> {
    let mixer_to_clear = {
        let mut table = ALSA_PCM_TABLE.lock();
        let pcm = table.get_mut(&handle.id()).ok_or(KernelError::InvalidHandle)?;
        // DROP from OPEN is a no-op success in ALSA; only reconfigured states
        // move to SETUP.
        if pcm.state != STATE_OPEN {
            pcm.state = STATE_SETUP;
        }
        pcm.frames_written = 0;
        pcm.mixer_stream
    };
    if let Some(sid) = mixer_to_clear {
        audio_mixer::clear(sid);
    }
    Ok(())
}

/// Drain the substream (`DRAIN`): stop accepting new frames and return to
/// `SETUP` once the buffered frames have been consumed.
///
/// Our mixer pulls frames asynchronously, so we do a non-blocking drain: the
/// already-buffered frames remain queued for the mixer and the state returns to
/// `SETUP`.  (A blocking drain that waits for the ring to empty is a future
/// refinement; non-blocking is correct for the `SND_PCM_NONBLOCK` path ALSA-lib
/// uses for event-driven clients.)
///
/// # Errors
///
/// [`KernelError::InvalidHandle`] if the instance is stale.
pub fn drain(handle: AlsaPcmHandle) -> KernelResult<()> {
    let mut table = ALSA_PCM_TABLE.lock();
    let pcm = table.get_mut(&handle.id()).ok_or(KernelError::InvalidHandle)?;
    if pcm.state != STATE_OPEN {
        pcm.state = STATE_SETUP;
    }
    Ok(())
}

/// Pause (`enable == true`) or resume (`enable == false`) a running substream.
///
/// # Errors
///
/// - [`KernelError::InvalidHandle`] if the instance is stale.
/// - [`KernelError::InvalidArgument`] for an illegal pause/resume transition
///   (pause requires `RUNNING`, resume requires `PAUSED`).
pub fn pause(handle: AlsaPcmHandle, enable: bool) -> KernelResult<()> {
    let mut table = ALSA_PCM_TABLE.lock();
    let pcm = table.get_mut(&handle.id()).ok_or(KernelError::InvalidHandle)?;
    match (enable, pcm.state) {
        (true, STATE_RUNNING) => {
            pcm.state = STATE_PAUSED;
            Ok(())
        }
        (false, STATE_PAUSED) => {
            pcm.state = STATE_RUNNING;
            Ok(())
        }
        _ => Err(KernelError::InvalidArgument),
    }
}

/// Reset the substream position (`RESET`): zero the application pointer and
/// clear the mixer ring without changing the running state.
///
/// # Errors
///
/// [`KernelError::InvalidHandle`] if the instance is stale.
pub fn reset(handle: AlsaPcmHandle) -> KernelResult<()> {
    let mixer_to_clear = {
        let mut table = ALSA_PCM_TABLE.lock();
        let pcm = table.get_mut(&handle.id()).ok_or(KernelError::InvalidHandle)?;
        pcm.frames_written = 0;
        pcm.mixer_stream
    };
    if let Some(sid) = mixer_to_clear {
        audio_mixer::clear(sid);
    }
    Ok(())
}

/// Record the software parameters that affect position reporting (`SW_PARAMS`):
/// the pointer wrap-around `boundary` and the `avail_min` wakeup threshold.
///
/// Our fixed pipeline imposes no other software-parameter constraints, so the
/// remaining fields (thresholds, silence) are accepted and echoed unchanged by
/// the syscall layer; only these two influence the `SYNC_PTR` reply.
///
/// # Errors
///
/// [`KernelError::InvalidHandle`] if the instance is stale.
pub fn set_sw_params(handle: AlsaPcmHandle, boundary: u64, avail_min: u64) -> KernelResult<()> {
    let mut table = ALSA_PCM_TABLE.lock();
    let pcm = table.get_mut(&handle.id()).ok_or(KernelError::InvalidHandle)?;
    pcm.boundary = boundary;
    pcm.avail_min = avail_min;
    Ok(())
}

/// Adopt an application pointer pushed by `SYNC_PTR` (the `!APPL` case, where
/// the client tells the kernel its current `appl_ptr`).
///
/// A stale handle is ignored — the caller has already validated existence via
/// [`sync_position`] (which returns `None` for a dead instance).
pub fn set_appl_ptr(handle: AlsaPcmHandle, appl_ptr: u64) {
    if let Some(pcm) = ALSA_PCM_TABLE.lock().get_mut(&handle.id()) {
        pcm.frames_written = appl_ptr;
    }
}

/// Adopt an `avail_min` pushed by `SYNC_PTR` (the `!AVAIL_MIN` case).
pub fn set_avail_min(handle: AlsaPcmHandle, avail_min: u64) {
    if let Some(pcm) = ALSA_PCM_TABLE.lock().get_mut(&handle.id()) {
        pcm.avail_min = avail_min;
    }
}

/// Record the ring buffer size (frames) negotiated at `HW_PARAMS`.
///
/// Used to compute the `avail` field of the `STATUS` snapshot.  A stale handle
/// is ignored (existence is validated separately by the ioctl path).  A zero
/// `frames` (client did not pin the `BUFFER_SIZE` interval) is stored as-is and
/// simply yields `avail == 0` in the snapshot.
pub fn set_buffer_size(handle: AlsaPcmHandle, frames: u64) {
    if let Some(pcm) = ALSA_PCM_TABLE.lock().get_mut(&handle.id()) {
        pcm.buffer_frames = frames;
    }
}

/// Snapshot the substream position for `SYNC_PTR` / `STATUS` reporting.
///
/// Computes the hardware pointer (frames the mixer has consumed) as the
/// application pointer minus the frames still queued in the mixer ring, and
/// reduces both pointers modulo the substream's boundary.  Returns `None` for a
/// stale handle.
///
/// Lock discipline: the per-open fields are read under the table lock, the lock
/// is dropped, and *then* the mixer's buffered-frame count is queried — so no
/// mixer call is made while `ALSA_PCM_TABLE` is held.
#[must_use]
pub fn sync_position(handle: AlsaPcmHandle) -> Option<PcmPosition> {
    let (state, frames_written, boundary, avail_min, mixer_stream, buffer_frames, capture, trigger_time_ns) = {
        let table = ALSA_PCM_TABLE.lock();
        let pcm = table.get(&handle.id())?;
        (
            pcm.state,
            pcm.frames_written,
            pcm.boundary,
            pcm.avail_min,
            pcm.mixer_stream,
            pcm.buffer_frames,
            pcm.capture,
            pcm.trigger_time_ns,
        )
    };

    // Frames still queued in the mixer ring (not yet played) — fetched with the
    // table lock released to preserve the leaf-lock invariant.
    let buffered_frames = match mixer_stream {
        Some(sid) => (audio_mixer::buffered(sid))
            .checked_div(audio_mixer::FRAME_SIZE_BYTES)
            .unwrap_or(0) as u64,
        None => 0,
    };
    let hw_abs = frames_written.saturating_sub(buffered_frames);

    // `delay` is the absolute queued-frame count (pre-boundary), matching
    // `snd_pcm_delay(3)`.  `avail` is derived from the negotiated buffer size:
    // playback reports free space, capture reports the whole buffer (silence is
    // always readable).  A not-yet-negotiated buffer (0) yields avail 0.
    let delay = buffered_frames;
    let avail = if capture {
        buffer_frames
    } else {
        buffer_frames.saturating_sub(delay)
    };

    // Reduce modulo the boundary; a zero boundary (not yet set) means "report
    // the raw counter".  `checked_rem` returns `None` on a zero divisor.
    let reduce = |v: u64| v.checked_rem(boundary).unwrap_or(v);
    Some(PcmPosition {
        state,
        hw_ptr: reduce(hw_abs),
        appl_ptr: reduce(frames_written),
        avail_min,
        delay,
        avail,
        buffer_frames,
        capture,
        trigger_time_ns,
    })
}

/// Produce capture (record) frames into `dst` (`READI_FRAMES` / `read(2)`).
///
/// The mixer is output-only, so a capture substream reads synthesised silence:
/// `dst` is zero-filled.  Returns the number of **bytes** produced (always all
/// of `dst` for a live capture instance).
///
/// # Errors
///
/// [`KernelError::InvalidArgument`] if this is not a live capture substream
/// (a playback substream or a stale handle).
pub fn read_frames(handle: AlsaPcmHandle, dst: &mut [u8]) -> KernelResult<usize> {
    if !readable(handle) {
        return Err(KernelError::InvalidArgument);
    }
    dst.fill(0);
    Ok(dst.len())
}

/// Submit interleaved playback frames (`WRITEI_FRAMES` / `write(2)`).
///
/// `data` is native-format PCM (S16_LE stereo at 48 kHz, 4 bytes/frame) already
/// copied into the kernel by the caller.  A first write from `PREPARED`
/// auto-starts the substream (`PREPARED` → `RUNNING`), mirroring ALSA's
/// `start_threshold` behaviour for the common single-shot writer.  Returns the
/// number of **bytes** accepted by the mixer (the caller converts to a frame
/// count for the ioctl reply).
///
/// # Errors
///
/// - [`KernelError::InvalidHandle`] if the instance is stale.
/// - [`KernelError::InvalidArgument`] if this is a capture substream, has no
///   committed configuration, or is in a state that cannot accept frames.
/// - [`KernelError::WouldBlock`] if the mixer ring is full (no frame accepted);
///   the caller maps this to `-EAGAIN` so userspace can `poll(POLLOUT)`.
pub fn write_frames(handle: AlsaPcmHandle, data: &[u8]) -> KernelResult<usize> {
    // Phase 1: validate the transition and fetch the mixer slot under the lock.
    let mixer_id = {
        let mut table = ALSA_PCM_TABLE.lock();
        let pcm = table.get_mut(&handle.id()).ok_or(KernelError::InvalidHandle)?;
        if pcm.capture {
            return Err(KernelError::InvalidArgument);
        }
        let id = pcm.mixer_stream.ok_or(KernelError::InvalidArgument)?;
        match pcm.state {
            STATE_PREPARED => pcm.state = STATE_RUNNING, // auto-start
            STATE_RUNNING | STATE_PAUSED => {}
            _ => return Err(KernelError::InvalidArgument),
        }
        id
    };

    // Phase 2: push to the mixer with the table lock released.
    let written = audio_mixer::write_pcm(mixer_id, data)?;
    if written == 0 && !data.is_empty() {
        return Err(KernelError::WouldBlock);
    }

    // Phase 3: advance the application pointer.
    if let Some(pcm) = ALSA_PCM_TABLE.lock().get_mut(&handle.id()) {
        let frames = written.checked_div(audio_mixer::FRAME_SIZE_BYTES).unwrap_or(0) as u64;
        pcm.frames_written = pcm.frames_written.saturating_add(frames);
    }
    Ok(written)
}

/// Is a substream writable right now (does its mixer ring have room)?
///
/// Backs `POLLOUT` readiness.  Capture substreams are never writable; a
/// playback substream without a mixer slot (still `OPEN`) reports writable so a
/// poll before `HW_PARAMS` does not hang (matching ALSA, where an unconfigured
/// fd is immediately ready to be configured).
#[must_use]
pub fn writable(handle: AlsaPcmHandle) -> bool {
    let table = ALSA_PCM_TABLE.lock();
    match table.get(&handle.id()) {
        None => false,
        Some(pcm) if pcm.capture => false,
        Some(pcm) => match pcm.mixer_stream {
            Some(sid) => audio_mixer::writable(sid),
            None => true,
        },
    }
}

/// Is a capture substream readable right now?
///
/// The mixer is output-only, so capture produces synthesised silence and is
/// always immediately readable once it has been opened.  Playback substreams
/// are never readable.
#[must_use]
pub fn readable(handle: AlsaPcmHandle) -> bool {
    ALSA_PCM_TABLE.lock().get(&handle.id()).is_some_and(|p| p.capture)
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

    // --- playback state machine ----------------------------------------
    // Native config: S16_LE (2) / 48000 Hz / stereo.
    let p = create(false);
    check!(state(p) == Some(STATE_OPEN), "fresh playback OPEN");
    // A non-native config is rejected without leaving SETUP.
    check!(
        hw_params(p, 2, 44100, 2) == Err(KernelError::InvalidArgument),
        "non-native rate rejected"
    );
    check!(state(p) == Some(STATE_OPEN), "rejected hw_params leaves OPEN");
    // Native config is accepted: OPEN -> SETUP, mixer slot reserved.
    hw_params(p, 2, 48000, 2)?;
    check!(state(p) == Some(STATE_SETUP), "hw_params -> SETUP");
    check!(params(p) == Some((2, 48000, 2)), "params stored");
    // Idempotent repeat: a second HW_PARAMS reuses the already-reserved mixer
    // slot (need_stream == false) rather than opening/leaking another.
    hw_params(p, 2, 48000, 2)?;
    check!(state(p) == Some(STATE_SETUP), "repeat hw_params stays SETUP");
    check!(params(p) == Some((2, 48000, 2)), "repeat hw_params keeps params");
    // PREPARE -> PREPARED, then a write auto-starts to RUNNING.
    prepare(p)?;
    check!(state(p) == Some(STATE_PREPARED), "prepare -> PREPARED");
    let frame = [0u8; 8]; // two native frames
    let n = write_frames(p, &frame)?;
    check!(n == 8, "two frames accepted");
    check!(state(p) == Some(STATE_RUNNING), "write auto-starts RUNNING");
    check!(frames_written(p) == Some(2), "appl ptr advanced by 2 frames");
    check!(writable(p), "running playback is writable");
    check!(!readable(p), "playback is not readable");
    // SW_PARAMS records the boundary + avail_min for SYNC_PTR reporting.
    set_sw_params(p, 1 << 20, 64)?;
    // SYNC_PTR snapshot: 2 frames submitted; the mixer pull thread does not run
    // during the boot self-test, so all 2 are still ring-buffered and the
    // hardware pointer has not advanced (hw_ptr = appl_ptr - buffered = 0).
    check!(
        sync_position(p)
            == Some(PcmPosition {
                state: STATE_RUNNING,
                hw_ptr: 0,
                appl_ptr: 2,
                avail_min: 64,
                // 2 frames still ring-buffered → delay = 2.  Buffer size not yet
                // negotiated (HW_PARAMS via the kernel path did not set it), so
                // avail = 0.  Auto-start (write) does not stamp trigger_time.
                delay: 2,
                avail: 0,
                buffer_frames: 0,
                capture: false,
                trigger_time_ns: 0,
            }),
        "sync position snapshot (appl=2, hw=0, delay=2)"
    );
    // STATUS-field coverage: negotiate a buffer size and START explicitly, then
    // confirm delay/avail/buffer_frames and a stamped trigger time.
    set_buffer_size(p, 1024);
    let snap = sync_position(p).ok_or(KernelError::InternalError)?;
    check!(snap.buffer_frames == 1024, "buffer size stored for STATUS");
    // Still 2 frames queued (mixer pull idle): avail = 1024 - 2 = 1022.
    check!(snap.delay == 2, "STATUS delay = queued frames");
    check!(snap.avail == 1022, "STATUS avail = buffer_frames - delay");
    check!(!snap.capture, "playback snapshot capture flag false");
    // Auto-start left trigger unset; an explicit prepare+start stamps it.
    check!(snap.trigger_time_ns == 0, "trigger unstamped before explicit start");
    // A pushed application pointer is adopted (the !APPL SYNC_PTR case).
    set_appl_ptr(p, 7);
    check!(frames_written(p) == Some(7), "set_appl_ptr adopted");
    set_avail_min(p, 128);
    check!(
        sync_position(p).map(|pp| pp.avail_min) == Some(128),
        "set_avail_min adopted"
    );
    // PAUSE / resume round-trip.
    pause(p, true)?;
    check!(state(p) == Some(STATE_PAUSED), "pause -> PAUSED");
    check!(pause(p, true) == Err(KernelError::InvalidArgument), "double pause errors");
    pause(p, false)?;
    check!(state(p) == Some(STATE_RUNNING), "resume -> RUNNING");
    // DRAIN -> SETUP, then re-prepare and START explicitly.
    drain(p)?;
    check!(state(p) == Some(STATE_SETUP), "drain -> SETUP");
    prepare(p)?;
    start(p)?;
    check!(state(p) == Some(STATE_RUNNING), "explicit start -> RUNNING");
    // Explicit START now stamps the trigger time for STATUS's trigger_tstamp.
    check!(
        sync_position(p).is_some_and(|pp| pp.trigger_time_ns != 0),
        "explicit start stamps trigger_time"
    );
    // DROP -> SETUP and resets the appl ptr.
    drop_stream(p)?;
    check!(state(p) == Some(STATE_SETUP), "drop -> SETUP");
    check!(frames_written(p) == Some(0), "drop resets appl ptr");
    // HW_FREE releases the slot and returns to OPEN.
    hw_free(p)?;
    check!(state(p) == Some(STATE_OPEN), "hw_free -> OPEN");
    check!(params(p).is_none(), "hw_free clears params");
    // A start from OPEN is illegal; a write without config is illegal.
    check!(start(p) == Err(KernelError::InvalidArgument), "start from OPEN errors");
    check!(
        write_frames(p, &frame) == Err(KernelError::InvalidArgument),
        "write without config errors"
    );
    close(p);
    check!(!exists(p), "playback instance freed");

    // --- capture substream ---------------------------------------------
    let cap = create(true);
    hw_params(cap, 2, 48000, 2)?;
    check!(state(cap) == Some(STATE_SETUP), "capture hw_params -> SETUP");
    check!(readable(cap), "configured capture is readable");
    check!(!writable(cap), "capture is not writable");
    check!(
        write_frames(cap, &frame) == Err(KernelError::InvalidArgument),
        "write to capture errors"
    );
    // READI_FRAMES on a capture substream yields silence (mixer is output-only).
    let mut rbuf = [0xAAu8; 8];
    check!(read_frames(cap, &mut rbuf) == Ok(8), "capture read produces full buffer");
    check!(rbuf == [0u8; 8], "capture read is silence");
    // A read on the (now-stale) playback handle is rejected.
    check!(
        read_frames(p, &mut rbuf) == Err(KernelError::InvalidArgument),
        "read on a non-capture/stale handle errors"
    );
    close(cap);

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
