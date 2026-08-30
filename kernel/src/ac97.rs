//! AC'97 (Audio Codec '97) audio controller driver.
//!
//! Supports the Intel ICH AC'97 audio controller commonly emulated by
//! QEMU (`-device AC97` or `-soundhw ac97`).  Uses the legacy PIO
//! transport with two BAR regions:
//!
//! - BAR0: Native Audio Mixer (NAM) — AC97 codec registers (volume, mute, etc.)
//! - BAR1: Native Audio Bus Master (NABM) — DMA control for PCM in/out
//!
//! ## DMA Architecture
//!
//! The AC97 controller uses a Buffer Descriptor List (BDL) — a ring of
//! 32 entries, each pointing to a PCM data buffer.  The controller
//! fetches descriptors and DMA-reads audio data automatically.
//!
//! ```text
//! BDL Entry (8 bytes):
//!   [31:0]  Physical address of PCM buffer
//!   [47:32] Buffer length in samples (not bytes!) — max 65535
//!   [63:62] Flags: BUP (buffer underrun policy), IOC (interrupt on completion)
//! ```
//!
//! ## QEMU Usage
//!
//! ```text
//! -device AC97,audiodev=a0 -audiodev sdl,id=a0
//! ```
//!
//! ## References
//!
//! - Intel ICH/ICH0/ICH2 AC'97 Programmer's Reference Manual (Rev 1.0)
//! - AC'97 Component Specification (Rev 2.3)
//! - OSDev Wiki: AC97
//! - Linux `sound/pci/intel8x0.c`

use core::sync::atomic::{AtomicBool, Ordering};

use crate::error::{KernelError, KernelResult};
use crate::mm::frame::{self, FRAME_SIZE, PhysFrame};
use crate::pci::{self, PciDevice};
use crate::port;
use crate::serial_println;
use crate::sync::PreemptSpinMutex;

// ---------------------------------------------------------------------------
// PCI device IDs
// ---------------------------------------------------------------------------

/// Intel vendor ID.
const INTEL_VENDOR: u16 = 0x8086;

/// Known AC97 controller device IDs (Intel ICH family).
const AC97_DEVICE_IDS: &[(u16, u16)] = &[
    (INTEL_VENDOR, 0x2415), // ICH AC'97 Audio
    (INTEL_VENDOR, 0x2425), // ICH0
    (INTEL_VENDOR, 0x2445), // ICH2
    (INTEL_VENDOR, 0x2485), // ICH3
    (INTEL_VENDOR, 0x24C5), // ICH4
    (INTEL_VENDOR, 0x24D5), // ICH5
    (INTEL_VENDOR, 0x266E), // ICH6
    (INTEL_VENDOR, 0x27DE), // ICH7
    (INTEL_VENDOR, 0x7195), // 440MX
];

// ---------------------------------------------------------------------------
// Native Audio Mixer (NAM) register offsets (BAR0)
// ---------------------------------------------------------------------------

/// Master volume (left/right 6-bit attenuation + mute).
const NAM_MASTER_VOL: u16 = 0x02;
/// PCM out volume.
const NAM_PCM_VOL: u16 = 0x18;
/// Reset register (write anything to reset codec).
const NAM_RESET: u16 = 0x00;
/// Power-down control/status.
const NAM_POWERDOWN: u16 = 0x26;
/// Extended audio ID.
#[allow(dead_code)]
const NAM_EXT_AUDIO_ID: u16 = 0x28;
/// Extended audio control/status.
#[allow(dead_code)]
const NAM_EXT_AUDIO_CTRL: u16 = 0x2A;
/// PCM front DAC rate.
const NAM_PCM_FRONT_DAC_RATE: u16 = 0x2C;
/// Vendor ID 1.
const NAM_VENDOR_ID1: u16 = 0x7C;
/// Vendor ID 2.
const NAM_VENDOR_ID2: u16 = 0x7E;

// ---------------------------------------------------------------------------
// Native Audio Bus Master (NABM) register offsets (BAR1)
// ---------------------------------------------------------------------------

// PCM Output channel registers (offset 0x10 from BAR1).
/// PCM Out Buffer Descriptor List base address (32-bit physical).
const NABM_PCMO_BDBAR: u16 = 0x10;
/// PCM Out Current Index (which BDL entry the controller is on).
#[allow(dead_code)]
const NABM_PCMO_CIV: u16 = 0x14;
/// PCM Out Last Valid Index (last entry the controller should process).
const NABM_PCMO_LVI: u16 = 0x15;
/// PCM Out Status register.
const NABM_PCMO_SR: u16 = 0x16;
/// PCM Out Control register.
const NABM_PCMO_CR: u16 = 0x1B;

// PCM Input channel registers (offset 0x00 from BAR1).
#[allow(dead_code)]
const NABM_PCMI_BDBAR: u16 = 0x00;

// Global control
/// Global control register.
const NABM_GLOB_CTRL: u16 = 0x2C;
/// Global status register.
const NABM_GLOB_STAT: u16 = 0x30;

// Control register bits.
/// Run/pause the DMA engine.
const CR_RPBM: u8 = 0x01;
/// Reset the DMA engine.
const CR_RR: u8 = 0x02;
/// Interrupt on completion enable.
#[allow(dead_code)]
const CR_IOCE: u8 = 0x10;

// Status register bits.
/// DMA controller halted.
#[allow(dead_code)]
const SR_DCH: u16 = 0x01;
/// Buffer completion interrupt.
const SR_BCIS: u16 = 0x08;

// Global control bits.
/// Cold reset.
const GC_COLD_RESET: u32 = 0x02;

/// How long to wait for a channel's self-clearing `CR.RR` reset bit to drop.
///
/// The reset is a register-file reload, not a bus operation, so real hardware
/// clears it in microseconds; 100 ms is "this controller is not responding"
/// rather than a tuned value.
const CHANNEL_RESET_TIMEOUT_US: u32 = 100_000;
/// Warm reset.
#[allow(dead_code)]
const GC_WARM_RESET: u32 = 0x04;

// ---------------------------------------------------------------------------
// Buffer Descriptor List
// ---------------------------------------------------------------------------

/// Number of entries in the Buffer Descriptor List.
const BDL_SIZE: usize = 32;

/// A single BDL entry (8 bytes).
///
/// Layout:
/// - `addr`: 32-bit physical address of audio data buffer
/// - `samples_and_flags`: bits[15:0] = number of samples (16-bit, stereo = 4 bytes/sample)
///   bit[30] = BUP (buffer underrun policy: 0=last valid, 1=zero fill)
///   bit[31] = IOC (interrupt on completion)
#[repr(C)]
#[derive(Clone, Copy)]
struct BdlEntry {
    addr: u32,
    samples_and_flags: u32,
}

/// IOC flag in BDL entry.
const BDL_IOC: u32 = 1 << 31;
/// BUP flag in BDL entry.
#[allow(dead_code)]
const BDL_BUP: u32 = 1 << 30;

// ---------------------------------------------------------------------------
// Device state
// ---------------------------------------------------------------------------

/// AC97 device state.
struct Ac97Device {
    /// Native Audio Mixer I/O base (BAR0).
    nam_base: u16,
    /// Native Audio Bus Master I/O base (BAR1).
    nabm_base: u16,
    /// BDL frame (holds 32 × 8 = 256 bytes of descriptors).
    bdl_frame: PhysFrame,
    /// PCM data frame (holds audio sample buffers).
    pcm_frame: PhysFrame,
    /// HHDM offset for phys→virt.
    hhdm_offset: u64,
    /// Whether playback is active.
    playing: bool,
    /// Monotonic counter identifying *which* tone is currently playing.
    ///
    /// Incremented by [`play_test_tone`] each time it claims the device.
    /// `playing` alone cannot answer "is the tone still mine?" after the lock
    /// has been released across the wait: a concurrent [`stop`] followed by a
    /// second `play_test_tone` leaves `playing == true` again, and the first
    /// caller would then stop the *second* caller's tone. Comparing the
    /// generation it recorded at start closes that ABA window.
    play_gen: u64,
    /// Sample rate (default 48000).
    sample_rate: u32,
    /// Codec vendor ID (for diagnostics).
    codec_vendor: u32,
}

/// Global device instance.
///
/// A `PreemptSpinMutex` leaf lock (Q24 / design-decisions §70): nothing is
/// acquired underneath it and it is unreachable from interrupt context — this
/// driver programs DMA and polls, it wires up no IRQ handler — so a
/// preempt-disabling spinlock is the right weight and `lock_irqsave` is not
/// needed. Every critical section under it must stay short; see
/// [`play_test_tone`], which deliberately drops it across the playback wait.
static DEVICE: PreemptSpinMutex<Option<Ac97Device>> = PreemptSpinMutex::named(None, b"AC97_DEVICE");

/// Whether AC97 is initialized and available.
static INITIALIZED: AtomicBool = AtomicBool::new(false);

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Probe PCI for an AC97 audio controller and initialize it.
pub fn init(hhdm_offset: u64) -> KernelResult<()> {
    serial_println!("[ac97] Probing for AC97 audio controller...");

    let dev = find_device()?;

    serial_println!(
        "[ac97] Found at {:02x}:{:02x}.{} ({:04x}:{:04x})",
        dev.address.bus,
        dev.address.device,
        dev.address.function,
        dev.vendor_id,
        dev.device_id
    );

    // BAR0 = Native Audio Mixer (I/O ports).
    let bar0 = dev.bars[0];
    if bar0 == 0 || bar0 & 1 == 0 {
        serial_println!("[ac97] BAR0 is not I/O space");
        return Err(KernelError::NoSuchDevice);
    }
    let nam_base = (bar0 & !0x3) as u16;

    // BAR1 = Native Audio Bus Master (I/O ports).
    let bar1 = dev.bars[1];
    if bar1 == 0 || bar1 & 1 == 0 {
        serial_println!("[ac97] BAR1 is not I/O space");
        return Err(KernelError::NoSuchDevice);
    }
    let nabm_base = (bar1 & !0x3) as u16;

    serial_println!(
        "[ac97] NAM base: {:#x}, NABM base: {:#x}",
        nam_base,
        nabm_base
    );

    // Enable bus mastering for DMA.
    pci::enable_bus_master(dev.address);

    // --- Controller Reset ---

    // Cold reset: set bit 1 in global control.
    // SAFETY: Writing to device I/O port for AC97 controller init.
    unsafe {
        port::outl(nabm_base.wrapping_add(NABM_GLOB_CTRL), GC_COLD_RESET);
    }
    // Wait for codec ready (poll global status for codec ready bits).
    busy_wait_us(100_000); // 100ms for codec to initialize.

    // SAFETY: nabm_base is the AC97 NABM I/O base from PCI BAR; all port
    // reads/writes in this function target valid AC97 register offsets.
    let glob_stat = unsafe { port::inl(nabm_base.wrapping_add(NABM_GLOB_STAT)) };
    serial_println!("[ac97] Global status after reset: {:#010x}", glob_stat);

    // Check primary codec ready (bit 8).
    if glob_stat & (1 << 8) == 0 {
        serial_println!(
            "[ac97] Warning: primary codec not ready (status {:#x})",
            glob_stat
        );
        // Continue anyway — some emulators don't set this correctly.
    }

    // --- Codec Configuration ---

    // Reset the codec.
    // SAFETY: nam_base is the AC97 NAM I/O base from PCI BAR.  All port
    // reads/writes below target valid AC97 mixer register offsets.
    unsafe {
        port::outw(nam_base.wrapping_add(NAM_RESET), 0);
    }
    busy_wait_us(10_000);

    // Read codec vendor ID.
    let vid1 = unsafe { port::inw(nam_base.wrapping_add(NAM_VENDOR_ID1)) };
    let vid2 = unsafe { port::inw(nam_base.wrapping_add(NAM_VENDOR_ID2)) };
    let codec_vendor = (u32::from(vid1) << 16) | u32::from(vid2);
    serial_println!("[ac97] Codec vendor: {:04x}:{:04x}", vid1, vid2);

    // Set master volume: unmute, moderate volume.
    // Register format: bit 15 = mute, bits [12:8] = left atten, [4:0] = right atten.
    // 0 = max volume, 63 = minimum. Set to ~50% = 0x0808.
    // SAFETY: nam_base is validated AC97 NAM I/O base.
    unsafe {
        port::outw(nam_base.wrapping_add(NAM_MASTER_VOL), 0x0808);
        port::outw(nam_base.wrapping_add(NAM_PCM_VOL), 0x0808);
    }

    // Wait for volume setting to take effect.
    busy_wait_us(1000);

    // Set sample rate to 48000 Hz.
    // First, check if variable rate audio is supported and enable it.
    // SAFETY: nam_base is validated AC97 NAM I/O base.
    let powerdown = unsafe { port::inw(nam_base.wrapping_add(NAM_POWERDOWN)) };
    serial_println!("[ac97] Power status: {:#06x}", powerdown);

    // Try to set sample rate (may not be supported on all codecs).
    // SAFETY: nam_base is validated AC97 NAM I/O base.
    unsafe {
        port::outw(nam_base.wrapping_add(NAM_PCM_FRONT_DAC_RATE), 48000);
    }
    busy_wait_us(1000);
    let actual_rate = unsafe { port::inw(nam_base.wrapping_add(NAM_PCM_FRONT_DAC_RATE)) };
    let sample_rate = if actual_rate == 0 || actual_rate == 48000 {
        48000u32
    } else {
        u32::from(actual_rate)
    };
    serial_println!("[ac97] Sample rate: {} Hz", sample_rate);

    // --- Allocate DMA Buffers ---

    let bdl_frame = frame::alloc_frame()?;
    let pcm_frame = match frame::alloc_frame() {
        Ok(f) => f,
        Err(e) => {
            // `PhysFrame` is a bare newtype over a physical address with no
            // Drop, so nothing reclaims `bdl_frame` on the way out of this
            // function.  Every early return past this point has to hand both
            // frames back explicitly.
            //
            // The free's own Result is dropped deliberately: we are already
            // unwinding on an allocation failure, a failure to give one frame
            // back cannot be acted on here, and reporting it instead of `e`
            // would replace the real cause with a bookkeeping detail.
            // SAFETY: `bdl_frame` was just allocated here, has never been
            // handed to the device (BDBAR is programmed further down), and is
            // not referenced anywhere after this point.
            let _ = unsafe { frame::free_frame(bdl_frame) };
            return Err(e);
        }
    };

    // Zero both frames.
    // SAFETY: Both frames were just allocated; HHDM maps them.
    unsafe {
        let bdl_virt = (bdl_frame.addr() + hhdm_offset) as *mut u8;
        core::ptr::write_bytes(bdl_virt, 0, FRAME_SIZE);
        let pcm_virt = (pcm_frame.addr() + hhdm_offset) as *mut u8;
        core::ptr::write_bytes(pcm_virt, 0, FRAME_SIZE);
    }

    // --- Reset the PCM Out DMA engine, THEN point it at our BDL ---
    //
    // Order matters, and the reverse order is a silent catastrophe.  CR.RR
    // ("reset registers") does not merely stop the channel: per the AC'97
    // controller spec it returns *all* of that channel's registers to their
    // power-on defaults, and BDBAR is one of them.  Programming BDBAR first
    // and resetting afterwards therefore leaves BDBAR reading 0 for the entire
    // life of the driver -- nothing later rewrites it -- so the bus master
    // fetches its buffer descriptors from guest-physical 0 instead of from our
    // BDL frame.  Playback then "works" in the sense that the run bit is set
    // and time passes, while the DMA engine walks whatever bytes happen to
    // live at address 0.  This was invisible until the boot test grew an
    // -device AC97 and self_test read the register back.
    //
    // RR is self-clearing: hardware drops it when the reset completes, so poll
    // for that rather than blind-waiting and then writing 0 (which would also
    // clobber any other CR bits we might later set here).  RR is only honoured
    // while the run bit is clear, which it is at this point -- the channel has
    // never been started.
    // SAFETY: nabm_base is the validated AC97 NABM I/O base and the offset is
    // this channel's control register.
    unsafe {
        port::outb(nabm_base.wrapping_add(NABM_PCMO_CR), CR_RR);
    }
    let mut reset_us = 0u32;
    loop {
        // SAFETY: as above.
        let cr = unsafe { port::inb(nabm_base.wrapping_add(NABM_PCMO_CR)) };
        if cr & CR_RR == 0 {
            break;
        }
        if reset_us >= CHANNEL_RESET_TIMEOUT_US {
            // Not fatal: a controller that will not self-clear RR still has to
            // be told where the BDL is, and the read-back below is what
            // decides whether this device is usable.  Say so rather than
            // failing init, so the mismatch is attributable if it follows.
            serial_println!("[ac97] WARNING: PCM Out reset did not self-clear within 100ms");
            break;
        }
        busy_wait_us(100);
        reset_us = reset_us.wrapping_add(100);
    }

    // --- Set up PCM Out BDL ---
    // Point the PCM Out BDBAR to our BDL frame's physical address.
    let bdl_phys = bdl_frame.addr() as u32; // AC97 is 32-bit DMA.
    // SAFETY: nabm_base is validated AC97 NABM I/O base; this write programmes
    // the channel's buffer-descriptor-list base address.
    unsafe {
        port::outl(nabm_base.wrapping_add(NABM_PCMO_BDBAR), bdl_phys);
    }

    // Read it straight back.  BDBAR is the one register whose corruption is
    // completely silent at runtime -- the DMA engine simply reads the wrong
    // memory -- so it is worth one I/O read at init to turn that into a log
    // line.  The low three bits are hardwired to zero (the BDL is 8-byte
    // aligned), so mask them out of the comparison rather than assuming the
    // allocator handed us an aligned frame and the controller kept every bit.
    // SAFETY: as above.
    let bdbar_readback = unsafe { port::inl(nabm_base.wrapping_add(NABM_PCMO_BDBAR)) };
    if bdbar_readback & !0x7 == bdl_phys & !0x7 {
        serial_println!("[ac97] PCM Out BDBAR: {:#010x}", bdbar_readback);
    } else {
        serial_println!(
            "[ac97] ERROR: BDBAR read-back {:#010x} != written {:#010x}; \
             the DMA engine would fetch descriptors from the wrong address",
            bdbar_readback,
            bdl_phys
        );
        // Same explicit hand-back as the allocation path above: neither frame
        // is reachable from anywhere once we return, and the controller is
        // about to be left alone.
        // SAFETY: both frames were allocated in this function.  The device was
        // told about `bdl_frame` one write ago, but the channel's run bit has
        // never been set, so no DMA has been or can be started against it; and
        // nothing else holds either address.
        let _ = unsafe { frame::free_frame(bdl_frame) };
        let _ = unsafe { frame::free_frame(pcm_frame) };
        return Err(KernelError::IoError);
    }

    let device = Ac97Device {
        nam_base,
        nabm_base,
        bdl_frame,
        pcm_frame,
        hhdm_offset,
        playing: false,
        play_gen: 0,
        sample_rate,
        codec_vendor,
    };

    *DEVICE.lock() = Some(device);
    INITIALIZED.store(true, Ordering::Release);

    serial_println!("[ac97] Initialization complete");
    Ok(())
}

/// Find an AC97 PCI device by vendor/device ID or by class.
fn find_device() -> KernelResult<PciDevice> {
    // Try known device IDs.
    for &(vendor, device) in AC97_DEVICE_IDS {
        if let Some(dev) = pci::find_device(vendor, device) {
            return Ok(dev);
        }
    }

    // Fallback: search by class (multimedia audio controller).
    let devices = pci::find_devices_by_class(0x04, 0x01);
    for dev in devices {
        // Skip virtio devices (already handled by virtio-sound).
        if dev.vendor_id == 0x1AF4 {
            continue;
        }
        // Skip Intel HDA (class 0x04, subclass 0x03 normally, but check anyway).
        if dev.device_id == 0x2668 {
            continue;
        }
        return Ok(dev);
    }

    serial_println!("[ac97] No AC97 device found");
    Err(KernelError::NoSuchDevice)
}

// ---------------------------------------------------------------------------
// Playback
// ---------------------------------------------------------------------------

/// Play a test tone (440 Hz) for the specified duration in milliseconds.
///
/// Fills the PCM buffer with a 440 Hz triangle wave and starts DMA playback,
/// waits out the tone, then stops the engine.
///
/// # Errors
///
/// [`KernelError::NoSuchDevice`] if no AC97 controller was detected, or
/// [`KernelError::DeviceBusy`] if a tone is already playing.
///
/// # Locking
///
/// `DEVICE` is held only to program the BDL and start the engine, and again to
/// stop it — **not** across the wait in between. Holding it there would pin a
/// preempt-disabling spinlock for the whole tone (up to `duration_ms`
/// milliseconds), blocking the scheduler on this CPU and spinning every other
/// caller for the same duration. `dev.playing` is what actually serialises
/// overlapping callers now that the lock no longer spans the wait: it is set
/// under the lock before it is released, so a second caller sees the device as
/// busy rather than reprogramming a live BDL.
#[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
pub fn play_test_tone(duration_ms: u32) -> KernelResult<()> {
    if !is_available() {
        return Err(KernelError::NoSuchDevice);
    }

    let mut guard = DEVICE.lock();
    let dev = guard.as_mut().ok_or(KernelError::NoSuchDevice)?;
    if dev.playing {
        return Err(KernelError::DeviceBusy);
    }

    // Calculate how many bytes we need.
    // 48000 Hz × 2 channels × 2 bytes/sample = 192000 bytes/sec.
    let bytes_per_sec: u32 = dev.sample_rate.saturating_mul(4); // stereo 16-bit
    let total_bytes = (duration_ms as u64)
        .saturating_mul(bytes_per_sec as u64)
        .saturating_div(1000) as usize;

    // We have FRAME_SIZE (16384) bytes of PCM buffer space.
    // Each BDL entry can reference a portion of it.
    // Split our PCM frame into equal chunks for the BDL ring.
    let pcm_phys = dev.pcm_frame.addr();
    let pcm_virt = (pcm_phys + dev.hhdm_offset) as *mut u8;
    let bdl_virt = (dev.bdl_frame.addr() + dev.hhdm_offset) as *mut BdlEntry;

    // Use the full frame for audio data (minus a small reservation).
    let pcm_buf_size = FRAME_SIZE; // 16384 bytes
    let chunk_size = pcm_buf_size / BDL_SIZE; // 512 bytes per chunk
    let samples_per_chunk = chunk_size / 4; // 128 stereo samples (4 bytes each)

    // Generate the waveform into the PCM buffer.
    generate_tone_buffer(pcm_virt, pcm_buf_size, dev.sample_rate);

    // Set up BDL entries — each points to its chunk in the PCM frame.
    for i in 0..BDL_SIZE {
        // SAFETY: bdl_virt points to the BDL frame; i < BDL_SIZE entries fit.
        let entry = unsafe { &mut *bdl_virt.add(i) };
        let offset = (i * chunk_size) as u32;
        entry.addr = (pcm_phys as u32).wrapping_add(offset);
        // Length in samples (stereo pairs), set IOC on last entry.
        let flags = if i == BDL_SIZE - 1 { BDL_IOC } else { 0 };
        entry.samples_and_flags = (samples_per_chunk as u32) | flags;
    }

    // Determine how many BDL entries we need for the desired duration.
    let total_chunks_needed = total_bytes.saturating_div(chunk_size).min(BDL_SIZE);
    let lvi = if total_chunks_needed == 0 {
        0
    } else {
        (total_chunks_needed - 1) as u8
    };

    // SAFETY: dev.nabm_base is validated AC97 NABM I/O base.  The following
    // writes set the last valid index, clear status, and start DMA playback.
    // Set Last Valid Index.
    unsafe {
        port::outb(dev.nabm_base.wrapping_add(NABM_PCMO_LVI), lvi);
    }

    // Clear status bits.
    unsafe {
        port::outw(dev.nabm_base.wrapping_add(NABM_PCMO_SR), SR_BCIS);
    }

    // Start playback (set RPBM bit).
    unsafe {
        port::outb(dev.nabm_base.wrapping_add(NABM_PCMO_CR), CR_RPBM);
    }
    dev.playing = true;
    dev.play_gen = dev.play_gen.wrapping_add(1);
    let my_gen = dev.play_gen;
    // Released before the wait, and before the serial write: see the Locking
    // section above. `playing` is now true, so a concurrent caller gets
    // DeviceBusy instead of reprogramming the BDL out from under this tone.
    drop(guard);

    serial_println!("[ac97] Playback started ({} ms, LVI={})", duration_ms, lvi);

    // Wait for playback to complete (busy-wait), with DEVICE unlocked.
    let wait_us = u64::from(duration_ms).saturating_mul(1000);
    busy_wait_us(wait_us);

    // Stop playback. Re-acquire rather than reuse the old guard; the device may
    // have been torn down while unlocked, in which case there is nothing to
    // stop and `playing` went with it. Stop only if this is still *our* tone —
    // see `play_gen`: a `stop()` plus a fresh `play_test_tone()` during the wait
    // would otherwise have us cut off someone else's playback.
    let stopped = match DEVICE.lock().as_mut() {
        Some(dev) if dev.playing && dev.play_gen == my_gen => {
            stop_playback_inner(dev);
            true
        }
        _ => false,
    };
    if stopped {
        serial_println!("[ac97] Playback complete");
    } else {
        serial_println!("[ac97] Playback ended early (stopped by another caller)");
    }

    Ok(())
}

/// Stop playback immediately.
pub fn stop() -> KernelResult<()> {
    if !is_available() {
        return Err(KernelError::NoSuchDevice);
    }

    let mut guard = DEVICE.lock();
    let dev = guard.as_mut().ok_or(KernelError::NoSuchDevice)?;
    stop_playback_inner(dev);
    Ok(())
}

/// Internal: stop DMA playback.
fn stop_playback_inner(dev: &mut Ac97Device) {
    // SAFETY: dev.nabm_base is validated AC97 NABM I/O base.
    // Clear RPBM bit to stop DMA.
    unsafe {
        port::outb(dev.nabm_base.wrapping_add(NABM_PCMO_CR), 0);
    }
    // Clear status.
    unsafe {
        port::outw(dev.nabm_base.wrapping_add(NABM_PCMO_SR), SR_BCIS);
    }
    dev.playing = false;
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Check if AC97 is initialized and available.
pub fn is_available() -> bool {
    INITIALIZED.load(Ordering::Acquire)
}

/// Get device status info: (available, sample_rate, playing, codec_vendor).
pub fn status_info() -> (bool, u32, bool, u32) {
    if !is_available() {
        return (false, 0, false, 0);
    }
    let guard = DEVICE.lock();
    match guard.as_ref() {
        Some(dev) => (true, dev.sample_rate, dev.playing, dev.codec_vendor),
        None => (false, 0, false, 0),
    }
}

// ---------------------------------------------------------------------------
// Tone generation
// ---------------------------------------------------------------------------

/// Generate a 440 Hz triangle wave into a buffer (stereo 16-bit).
#[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
fn generate_tone_buffer(buf: *mut u8, buf_size: usize, sample_rate: u32) {
    let samples_per_period = sample_rate / 440; // ~109 at 48kHz
    let num_frames = buf_size / 4; // 4 bytes per stereo sample

    for i in 0..num_frames {
        let t = (i as u32) % samples_per_period;
        let half_period = samples_per_period / 2;
        let quarter_period = samples_per_period / 4;

        let sample: i16 = if t < half_period {
            if t < quarter_period {
                ((t as i32 * 32767) / quarter_period as i32) as i16
            } else {
                (((half_period - t) as i32 * 32767) / quarter_period as i32) as i16
            }
        } else {
            let t2 = t - half_period;
            if t2 < quarter_period {
                -(((t2 as i32) * 32767) / quarter_period as i32) as i16
            } else {
                -(((half_period - t2) as i32 * 32767) / quarter_period as i32) as i16
            }
        };

        // Scale to ~50% volume.
        let sample = sample / 2;
        let bytes = sample.to_le_bytes();

        // SAFETY: writing within our allocated buffer.
        unsafe {
            let offset = i * 4;
            if offset + 3 < buf_size {
                *buf.add(offset) = bytes[0];
                *buf.add(offset + 1) = bytes[1];
                *buf.add(offset + 2) = bytes[0]; // Right = Left (mono source)
                *buf.add(offset + 3) = bytes[1];
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Timing helper
// ---------------------------------------------------------------------------

/// Busy-wait for approximately `us` microseconds using TSC.
fn busy_wait_us(us: u64) {
    // Assume ~2 GHz = 2000 cycles per microsecond.
    // SAFETY: _rdtsc is always available on x86_64; no side-effects.
    let start = unsafe { core::arch::x86_64::_rdtsc() };
    let target = us.saturating_mul(2000);
    loop {
        let now = unsafe { core::arch::x86_64::_rdtsc() };
        if now.wrapping_sub(start) >= target {
            break;
        }
        core::hint::spin_loop();
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test: verify AC97 device detection and basic register access.
///
/// If no AC97 device is present, gracefully reports "no device" and passes.
pub fn self_test() {
    serial_println!("[ac97] Running self-test...");

    if !is_available() {
        serial_println!("[ac97]   No device (skipped — add -device AC97 to QEMU)");
        serial_println!("[ac97] Self-test PASSED (no device)");
        return;
    }

    let (available, rate, playing, vendor) = status_info();
    serial_println!("[ac97]   Available: {}", available);
    serial_println!("[ac97]   Sample rate: {} Hz", rate);
    serial_println!("[ac97]   Codec vendor: {:08x}", vendor);
    serial_println!("[ac97]   Playing: {}", playing);

    // Verify we can read the master volume register.
    let guard = DEVICE.lock();
    if let Some(dev) = guard.as_ref() {
        // SAFETY: dev.nam_base/nabm_base are validated AC97 I/O port bases.
        let master_vol = unsafe { port::inw(dev.nam_base.wrapping_add(NAM_MASTER_VOL)) };
        serial_println!("[ac97]   Master volume register: {:#06x}", master_vol);

        // Verify BDL is still programmed.  `init` already checked this once
        // and refuses to bring the device up if it fails, so a mismatch *here*
        // means something clobbered BDBAR after init -- which in practice
        // means a stray CR.RR from the playback path, since that bit reloads
        // the whole channel register file.  Worth re-reading precisely because
        // the failure is otherwise silent: the DMA engine just walks the wrong
        // memory.  Low three bits are hardwired to zero, so mask them.
        let bdbar = unsafe { port::inl(dev.nabm_base.wrapping_add(NABM_PCMO_BDBAR)) };
        let expected = dev.bdl_frame.addr() as u32;
        if bdbar & !0x7 == expected & !0x7 {
            serial_println!("[ac97]   BDBAR: OK ({:#010x})", bdbar);
        } else {
            serial_println!(
                "[ac97]   BDBAR: MISMATCH (got {:#010x}, expected {:#010x}) \
                 -- DMA is reading descriptors from the wrong address",
                bdbar,
                expected
            );
        }
    }
    drop(guard);

    // Try a very short tone (10ms — verifies DMA path without annoying beep).
    match play_test_tone(10) {
        Ok(()) => serial_println!("[ac97]   Short tone: OK"),
        Err(e) => serial_println!("[ac97]   Short tone: {:?} (non-fatal)", e),
    }

    serial_println!("[ac97] Self-test PASSED");
}
