//! The audio output pump: what empties the software mixer into a sound card.
//!
//! ## Why this exists
//!
//! [`crate::audio_mixer`] adds every program's sound together, but until
//! 2026-10-02 nothing ever asked it for the sum: `mix_output` had no caller,
//! so a playback stream's ring (about 85 ms) filled once and stayed full, and
//! every program that played sound stalled after its first tenth of a second
//! (`requests/e-ad-no-application-can-reach-the-sound-device.md`). This is the
//! missing half: a kernel task that pulls the mix at the card's own rate.
//!
//! ## How
//!
//! At boot ([`init`]) the first usable card becomes the sink: Intel HD Audio,
//! else virtio-sound, else AC'97 running at the mixer's 48 kHz. HDA and AC'97
//! run a DMA engine round a cyclic buffer, and the pump keeps that buffer
//! filled a lead ahead of where the hardware is reading, waking every
//! [`TICK_NS`] to see how far it moved:
//!
//! ```text
//!     played (hardware)                    written (pump)
//!          |<--------------- lead --------------->|
//!   -------+--------------------------------------+------>  bytes, round the buffer
//! ```
//!
//! virtio-sound takes messages instead -- a period of frames each -- and the
//! pump keeps three in flight, refilling each as the device hands it back
//! (`virtio::sound::stream_fill`): the same 64 ms ahead, counted in periods.
//!
//! After each round it wakes the tasks waiting for room in a ring (blocked
//! writers, `DRAIN`, pollers): what it took is room for them. A round that
//! finds the hardware past what was written -- the pump ran late -- counts an
//! underrun and carries on from where the hardware is; the card played stale
//! samples for that stretch, as any late writer's card does.
//!
//! The card runs only while a stream is open. With none, the pump lets
//! [`IDLE_STOP_NS`] of silence play out, stops the engine, and parks until
//! [`kick`] -- which `audio_mixer::open_stream` calls -- wakes it.
//!
//! With no usable card, [`has_sink`] is false and opening a PCM device answers
//! `ENODEV`, so a program can say "no sound device" instead of seeming to play
//! into nothing.
//!
//! ## Locking
//!
//! The pump holds no lock of its own across a call. It takes the mixer's
//! locks inside `mix_output`, then a driver's device lock inside its
//! `stream_*` calls, one after the other, never nested.

use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};

use crate::audio_mixer::{self, FRAME_SIZE_BYTES};
use crate::error::{KernelError, KernelResult};
use crate::serial_println;

/// How often the pump looks at the hardware's position while the card runs.
pub const TICK_NS: u64 = 5_000_000;

/// How far ahead of the hardware the pump keeps the buffer filled: three
/// mixer periods (64 ms at 48 kHz), or half the card's buffer if that is
/// smaller. The pump can run this late before the card underruns.
const LEAD_BYTES: usize = 3 * 4096;

/// How long the card runs on silence after the last stream closes before the
/// pump stops it -- long enough that a player moving to its next track does
/// not stop and restart the card in between.
pub const IDLE_STOP_NS: u64 = 2_000_000_000;

/// How long the parked pump sleeps between looks when nothing kicks it: a
/// lost kick costs at most this long.
const PARK_NS: u64 = 1_000_000_000;

/// The pump's scheduling priority: the real-time media class design.txt
/// names ("Realtime (for audio/video playback)"), above every ordinary task
/// (16) so a busy system does not starve the card, below the top levels.
const PUMP_PRIORITY: u8 = 4;

/// Which card is the sink.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sink {
    /// Intel HD Audio.
    Hda,
    /// AC'97.
    Ac97,
    /// virtio-sound (a VM's).
    Virtio,
}

impl Sink {
    const fn code(self) -> u8 {
        match self {
            Self::Hda => 1,
            Self::Ac97 => 2,
            Self::Virtio => 3,
        }
    }

    fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::Hda),
            2 => Some(Self::Ac97),
            3 => Some(Self::Virtio),
            _ => None,
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Hda => "Intel HD Audio",
            Self::Ac97 => "AC'97",
            Self::Virtio => "virtio-sound",
        }
    }
}

/// The sink chosen at [`init`]: 0 none, else [`Sink::code`].
static SINK: AtomicU8 = AtomicU8::new(0);

/// HDA's cyclic buffer length, recorded when [`init`] configured it once.
static HDA_BUFFER_BYTES: AtomicU64 = AtomicU64::new(0);

/// The pump task's id, 0 until [`init`] starts it.
static PUMP_TID: AtomicU64 = AtomicU64::new(0);

/// Set by [`kick`], cleared by the pump before it looks: a kick that lands
/// while the pump is busy is not lost.
static KICKED: AtomicBool = AtomicBool::new(false);

/// Whether the card is running now.
static RUNNING: AtomicBool = AtomicBool::new(false);

/// Bytes of mixed output written to the card since boot.
static BYTES_PUMPED: AtomicU64 = AtomicU64::new(0);

/// Rounds that found the hardware past what had been written.
static UNDERRUNS: AtomicU64 = AtomicU64::new(0);

/// Times the card has been started.
static STARTS: AtomicU64 = AtomicU64::new(0);

/// Whether a sound card was found to play through. Without one, opening a PCM
/// device answers `ENODEV`.
#[must_use]
pub fn has_sink() -> bool {
    SINK.load(Ordering::Acquire) != 0
}

/// The pump's counters: bytes written to the card, underruns, starts, and
/// whether it is running now.
#[must_use]
pub fn stats() -> (u64, u64, u64, bool) {
    (
        BYTES_PUMPED.load(Ordering::Relaxed),
        UNDERRUNS.load(Ordering::Relaxed),
        STARTS.load(Ordering::Relaxed),
        RUNNING.load(Ordering::Acquire),
    )
}

/// Wake the pump: a stream has opened. Harmless before [`init`] (there is no
/// pump to wake) and while it runs.
pub fn kick() {
    KICKED.store(true, Ordering::Release);
    let tid = PUMP_TID.load(Ordering::Acquire);
    if tid != 0 {
        crate::sched::try_wake(tid);
    }
}

/// Choose the sink and start the pump. Called once at boot, after the audio
/// drivers' own self-tests, which reconfigure the cards and play through them
/// directly.
pub fn init() {
    let sink = if crate::hda::output_ready() {
        match crate::hda::stream_configure() {
            Ok(len) if len >= FRAME_SIZE_BYTES * 2 => {
                HDA_BUFFER_BYTES.store(len as u64, Ordering::Release);
                Some(Sink::Hda)
            }
            Ok(len) => {
                serial_println!("[audio-out] HDA buffer of {} bytes is too small", len);
                None
            }
            Err(e) => {
                serial_println!("[audio-out] HDA output would not configure: {:?}", e);
                None
            }
        }
    } else {
        None
    };
    let sink = sink
        .or_else(|| crate::virtio::sound::output_ready().then_some(Sink::Virtio))
        .or_else(|| crate::ac97::output_ready().then_some(Sink::Ac97));
    let Some(sink) = sink else {
        serial_println!("[audio-out] No sound card to play through; PCM devices answer ENODEV");
        return;
    };
    SINK.store(sink.code(), Ordering::Release);
    let pml4 = crate::mm::page_table::active_pml4_phys();
    match crate::sched::spawn(b"audio-pump", PUMP_PRIORITY, pump_task, 0, pml4) {
        Ok(tid) => {
            PUMP_TID.store(tid, Ordering::Release);
            serial_println!(
                "[audio-out] Playing through {} (pump task {})",
                sink.name(),
                tid
            );
        }
        Err(e) => {
            // No pump, no sound: say so as no card does.
            SINK.store(0, Ordering::Release);
            serial_println!("[audio-out] Pump task would not start: {:?}; no sound", e);
        }
    }
}

/// The card while it runs.
struct Running {
    sink: Sink,
    /// The cyclic buffer's length in bytes (0 for virtio-sound, which has
    /// none).
    buf_len: u64,
    /// Where the hardware was when the card started: byte 0 of the counts.
    origin: u64,
    /// Where the hardware was at the last look.
    last_pos: u64,
    /// Bytes the hardware has read since the start.
    played: u64,
    /// Bytes written since the start.
    written: u64,
    /// How far ahead to keep `written`.
    lead: u64,
    /// AC'97's or virtio-sound's claim on its stream, for their
    /// `stream_stop` (and virtio-sound's `stream_fill`).
    claim: u64,
    /// When the last stream closed (`hrtimer::now_ns`), while none is open.
    idle_since: Option<u64>,
}

// Byte counts and offsets: a counter would take years at 192 KB/s to near
// u64::MAX, buffer offsets are below `buf_len` (64 KiB at most), and the
// conversions between them are within one buffer on a 64-bit kernel.
#[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
impl Running {
    /// Start `sink` over silence.
    fn start(sink: Sink) -> KernelResult<Self> {
        let (buf_len, pos, claim) = match sink {
            Sink::Hda => {
                let len = HDA_BUFFER_BYTES.load(Ordering::Acquire);
                let pos = crate::hda::stream_start()?;
                (len, pos as u64, 0)
            }
            Sink::Ac97 => {
                let (len, pos, claim) = crate::ac97::stream_start()?;
                (len as u64, pos as u64, claim)
            }
            Sink::Virtio => (0, 0, crate::virtio::sound::stream_start()?),
        };
        if sink != Sink::Virtio && buf_len < (FRAME_SIZE_BYTES as u64) * 2 {
            return Err(KernelError::InternalError);
        }
        let frame = FRAME_SIZE_BYTES as u64;
        // Whole frames: the hardware's position and every write stay on a
        // frame boundary.
        let origin = pos - pos % frame;
        let half = buf_len / 2;
        let lead = (LEAD_BYTES as u64).min(half - half % frame);
        STARTS.fetch_add(1, Ordering::Relaxed);
        RUNNING.store(true, Ordering::Release);
        Ok(Self {
            sink,
            buf_len,
            origin,
            last_pos: pos,
            played: 0,
            written: 0,
            lead,
            claim,
            idle_since: None,
        })
    }

    fn position(&self) -> Option<u64> {
        match self.sink {
            Sink::Hda => crate::hda::stream_position(),
            Sink::Ac97 => crate::ac97::stream_position(),
            Sink::Virtio => None,
        }
        .map(|p| p as u64)
    }

    fn write(&self, offset: usize, data: &[u8]) -> KernelResult<()> {
        match self.sink {
            Sink::Hda => crate::hda::stream_write(offset, data),
            Sink::Ac97 => crate::ac97::stream_write(offset, data),
            Sink::Virtio => Err(KernelError::NotSupported),
        }
    }

    fn stop(&self) {
        let stopped = match self.sink {
            Sink::Hda => crate::hda::stream_stop(),
            Sink::Ac97 => crate::ac97::stream_stop(self.claim),
            Sink::Virtio => crate::virtio::sound::stream_stop(self.claim),
        };
        if let Err(e) = stopped {
            serial_println!("[audio-out] {} did not stop: {:?}", self.sink.name(), e);
        }
        RUNNING.store(false, Ordering::Release);
    }

    /// One look: account for what the hardware read since the last, and fill
    /// the buffer back up to the lead ahead of it from the mixer. Returns
    /// whether the card is still the pump's -- false when virtio-sound's
    /// stream was taken by someone else (its `stop`), so the pump starts it
    /// again.
    fn round(&mut self, chunk: &mut [u8]) -> bool {
        if self.sink == Sink::Virtio {
            return match crate::virtio::sound::stream_fill(self.claim, &mut |buf| {
                audio_mixer::mix_output(buf)
            }) {
                Ok((submitted, ran_dry)) => {
                    if ran_dry {
                        UNDERRUNS.fetch_add(1, Ordering::Relaxed);
                    }
                    BYTES_PUMPED.fetch_add(submitted as u64, Ordering::Relaxed);
                    true
                }
                Err(e) => {
                    serial_println!("[audio-out] virtio-sound stopped streaming: {:?}", e);
                    RUNNING.store(false, Ordering::Release);
                    false
                }
            };
        }
        let Some(pos) = self.position() else {
            return true;
        };
        let pos = pos % self.buf_len;
        let moved = (pos + self.buf_len - self.last_pos) % self.buf_len;
        self.last_pos = pos;
        self.played += moved;
        if self.written < self.played {
            UNDERRUNS.fetch_add(1, Ordering::Relaxed);
            let frame = FRAME_SIZE_BYTES as u64;
            self.written = self.played - self.played % frame;
        }
        let target = self.played + self.lead;
        while self.written < target {
            let offset = (self.origin + self.written) % self.buf_len;
            let to_end = self.buf_len - offset;
            let want = (target - self.written).min(to_end).min(chunk.len() as u64);
            let want = (want - want % FRAME_SIZE_BYTES as u64) as usize;
            if want == 0 {
                break;
            }
            let Some(out) = chunk.get_mut(..want) else {
                break;
            };
            let got = audio_mixer::mix_output(out);
            let Some(mixed) = chunk.get(..got) else {
                break;
            };
            if got == 0 || self.write(offset as usize, mixed).is_err() {
                break;
            }
            self.written += got as u64;
            BYTES_PUMPED.fetch_add(got as u64, Ordering::Relaxed);
        }
        true
    }
}

/// The pump: start the card when a stream opens, keep it fed while it runs,
/// stop it once nothing has played for [`IDLE_STOP_NS`].
#[allow(clippy::arithmetic_side_effects)] // byte counts far below u64::MAX
extern "C" fn pump_task(_arg: u64) {
    let Some(sink) = Sink::from_code(SINK.load(Ordering::Acquire)) else {
        return;
    };
    // One mixer period at a time, off the heap's hot path.
    let mut chunk = [0u8; 4096];
    let mut running: Option<Running> = None;
    loop {
        KICKED.store(false, Ordering::Release);
        let open = audio_mixer::active_streams() > 0;
        if running.is_none() && open {
            match Running::start(sink) {
                Ok(r) => running = Some(r),
                Err(e) => {
                    serial_println!("[audio-out] {} would not start: {:?}", sink.name(), e);
                }
            }
        }
        if let Some(r) = running.as_mut() {
            if !r.round(&mut chunk) {
                running = None;
                continue;
            }
            let now = crate::hrtimer::now_ns();
            if open {
                r.idle_since = None;
            } else {
                let since = *r.idle_since.get_or_insert(now);
                if now.saturating_sub(since) >= IDLE_STOP_NS {
                    r.stop();
                    running = None;
                }
            }
        }
        // What was taken is room for whoever waits on it.
        audio_mixer::wake_room_waiters();
        if running.is_some() {
            crate::sched::sleep_ns_interruptible(TICK_NS);
        } else if !KICKED.load(Ordering::Acquire) {
            crate::sched::sleep_ns_interruptible(PARK_NS);
        }
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// End to end through the card: a stream's full ring empties within two
/// seconds, the card is started for it, and the pump's count of bytes written
/// to the card grows by at least what was queued. Skipped without a card.
///
/// # Errors
///
/// `InternalError` when the ring does not drain or nothing reaches the card.
pub fn self_test() -> KernelResult<()> {
    serial_println!("[audio-out] Running self-test...");
    if !has_sink() {
        serial_println!("[audio-out]   SKIP: no sound card");
        return Ok(());
    }
    let (pumped_before, _, starts_before, _) = stats();
    let sid = audio_mixer::open_stream("audio-out-selftest")?;
    // A quiet square wave, a whole ring of it (85 ms).
    let mut block = [0u8; 4096];
    for (i, b) in block.iter_mut().enumerate() {
        *b = if (i / 192) % 2 == 0 { 0x10 } else { 0xF0 };
    }
    let mut queued = 0usize;
    for _ in 0..4 {
        queued = queued.saturating_add(audio_mixer::write_pcm(sid, &block).unwrap_or(0));
    }
    let deadline = crate::hrtimer::now_ns().saturating_add(2_000_000_000);
    while audio_mixer::buffered(sid) > 0 && crate::hrtimer::now_ns() < deadline {
        crate::sched::sleep_ms(10);
    }
    let left = audio_mixer::buffered(sid);
    audio_mixer::close_stream(sid);
    let (pumped_after, underruns, starts_after, _) = stats();
    let pumped = pumped_after.saturating_sub(pumped_before);
    let started = starts_after > starts_before || RUNNING.load(Ordering::Acquire);
    if left > 0 || pumped < queued as u64 || !started {
        serial_println!(
            "[audio-out]   FAIL: {} of {} queued bytes still in the ring after 2 s; {} bytes \
             reached the card; starts {} -> {}; {} underruns",
            left,
            queued,
            pumped,
            starts_before,
            starts_after,
            underruns
        );
        return Err(KernelError::InternalError);
    }
    serial_println!(
        "[audio-out]   {} bytes queued, all played out; {} bytes to the card, {} underruns",
        queued,
        pumped,
        underruns
    );
    pcm_through_the_pump()?;
    serial_println!("[audio-out] Self-test PASSED");
    Ok(())
}

/// A PCM substream end to end, as a blocking descriptor drives it: two rings'
/// worth written at once (the write must wait for the pump to make room, and
/// must take it all), a pause that stops the ring draining and a resume that
/// lets it, then a drain that returns only when everything has played and
/// leaves the substream in `SETUP`.
fn pcm_through_the_pump() -> KernelResult<()> {
    use crate::ipc::alsa_pcm;
    let h = alsa_pcm::create(false);
    let result = (|| -> Result<(), &'static str> {
        alsa_pcm::hw_params(
            h,
            crate::audio_alsa::SNDRV_PCM_FORMAT_S16_LE,
            crate::audio_alsa::MIXER_RATE,
            crate::audio_alsa::MIXER_CHANNELS,
        )
        .map_err(|_| "HW_PARAMS refused the native format")?;
        alsa_pcm::prepare(h).map_err(|_| "PREPARE failed")?;
        // Two rings: the second half can only go in as the pump plays the first.
        let frames = alloc::vec![0u8; 2 * 16384];
        let wrote =
            alsa_pcm::write_frames_blocking(h, &frames).map_err(|_| "a blocking write failed")?;
        if wrote != frames.len() {
            return Err("a blocking write took less than all of it");
        }
        alsa_pcm::pause(h, true).map_err(|_| "PAUSE failed")?;
        let sid_left = |h| alsa_pcm::sync_position(h).map_or(0, |p| p.delay);
        let before = sid_left(h);
        crate::sched::sleep_ms(60);
        if sid_left(h) != before || before == 0 {
            return Err("a paused substream's queue moved, or there was none to hold");
        }
        alsa_pcm::pause(h, false).map_err(|_| "resume failed")?;
        alsa_pcm::drain(h, false).map_err(|_| "a blocking DRAIN failed")?;
        if alsa_pcm::state(h) != Some(alsa_pcm::STATE_SETUP) || sid_left(h) != 0 {
            return Err("DRAIN returned before the queue played out, or not in SETUP");
        }
        Ok(())
    })();
    alsa_pcm::close(h);
    if let Err(why) = result {
        serial_println!("[audio-out]   FAIL: {}", why);
        return Err(KernelError::InternalError);
    }
    serial_println!(
        "[audio-out]   PCM: a two-ring blocking write, pause holding the queue, a blocking drain: OK"
    );
    Ok(())
}
