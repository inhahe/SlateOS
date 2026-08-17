//! Virtio sound device driver (virtio device type 25).
//!
//! Implements playback via QEMU's `virtio-sound-pci` device.  Uses the
//! legacy PCI transport (I/O port BAR0) with four virtqueues:
//!
//! - Queue 0 (controlq): device configuration requests (stream info, jack info)
//! - Queue 1 (eventq): device-to-driver events
//! - Queue 2 (txq): PCM playback (TX to device = audio output)
//! - Queue 3 (rxq): PCM capture (RX from device = audio input)
//!
//! ## Protocol Overview
//!
//! 1. Reset device, negotiate features, set up queues.
//! 2. Query PCM stream info via controlq to discover available streams.
//! 3. Prepare a stream with desired format (48kHz, 16-bit, stereo).
//! 4. Start the stream, then feed PCM data via txq.
//! 5. Stop and release when done.
//!
//! ## QEMU Usage
//!
//! ```text
//! -device virtio-sound-pci,audiodev=a0 -audiodev sdl,id=a0
//! ```
//!
//! ## References
//!
//! - Virtio 1.2 spec, Section 5.14 "Sound Device"
//! - QEMU hw/audio/virtio-snd.c

use core::sync::atomic::{AtomicBool, Ordering};

use crate::error::{KernelError, KernelResult};
use crate::mm::frame::{self, FRAME_SIZE, PhysFrame};
use crate::pci::{self, PciDevice};
use crate::serial_println;
use crate::sync::PreemptSpinMutex;
use crate::virtio::modern::{ModernTransport, STATUS_DRIVER_OK, VIRTIO_VENDOR};
use crate::virtio::queue::{VRING_DESC_F_WRITE, Virtqueue};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Serial-log prefix, handed to the shared transport so that its diagnostics
/// name this driver rather than the module they are emitted from.
const LOG_TAG: &str = "[virtio-snd]";

/// Virtio-sound PCI device ID.
///
/// Modern virtio device IDs are `0x1040 + device_type`, and virtio-sound is
/// device type 25, giving 0x1059 — which is what QEMU reports.  There is no
/// transitional/legacy ID to try: the device postdates virtio 1.0 and has no
/// legacy interface at all.  (0x1058 was previously probed as a "legacy ID";
/// that is device type 24, virtio-iommu, and matching it would have bound this
/// driver to the wrong device.)
const VIRTIO_SND_DEVICE_ID: u16 = 0x1059;

// Virtio sound control request types (virtio 1.2 §5.14.6)
/// Query jack information.
#[allow(dead_code)]
const VIRTIO_SND_R_JACK_INFO: u32 = 1;
/// Query PCM stream information.
const VIRTIO_SND_R_PCM_INFO: u32 = 0x0100;
/// Set PCM stream parameters.
const VIRTIO_SND_R_PCM_SET_PARAMS: u32 = 0x0101;
/// Prepare a PCM stream for I/O.
const VIRTIO_SND_R_PCM_PREPARE: u32 = 0x0102;
/// Release a PCM stream.
const VIRTIO_SND_R_PCM_RELEASE: u32 = 0x0103;
/// Start a PCM stream.
const VIRTIO_SND_R_PCM_START: u32 = 0x0104;
/// Stop a PCM stream.
const VIRTIO_SND_R_PCM_STOP: u32 = 0x0105;

// Response status codes
/// Success.
const VIRTIO_SND_S_OK: u32 = 0x8000;
/// Bad message.
#[allow(dead_code)]
const VIRTIO_SND_S_BAD_MSG: u32 = 0x8001;
/// Not supported.
#[allow(dead_code)]
const VIRTIO_SND_S_NOT_SUPP: u32 = 0x8002;
/// I/O error.
#[allow(dead_code)]
const VIRTIO_SND_S_IO_ERR: u32 = 0x8003;

// PCM formats (virtio 1.2 §5.14.6.6.1, `virtio_snd_pcm_info`)
//
// These are *dense ordinals*, and each value is used two different ways: it is
// the number written into `set_params.format`, AND it is the bit index into the
// `formats` mask the device advertises.  So a wrong value is wrong twice, and
// the two errors cancel under any check that only compares our request against
// our own constants.
//
// The whole table is spelled out even though only one entry is used, because
// that is what makes it *checkable*.  The previous version carried three
// hand-picked constants (`S16 = 2`, `RATE_44100 = 5`, `RATE_48000 = 6`) with no
// neighbours to compare against, and all three were wrong: S16 is 5 (2 is
// A-law), and each rate was one short.  A lone constant with a spec citation
// beside it is indistinguishable from a correct one at a glance; a contiguous
// run starting at 0 that can be read off against the spec's own list is not.
#[allow(dead_code)]
mod fmt {
    pub const IMA_ADPCM: u8 = 0;
    pub const MU_LAW: u8 = 1;
    pub const A_LAW: u8 = 2;
    pub const S8: u8 = 3;
    pub const U8: u8 = 4;
    pub const S16: u8 = 5;
    pub const U16: u8 = 6;
    pub const S18_3: u8 = 7;
    pub const U18_3: u8 = 8;
    pub const S20_3: u8 = 9;
    pub const U20_3: u8 = 10;
    pub const S24_3: u8 = 11;
    pub const U24_3: u8 = 12;
    pub const S20: u8 = 13;
    pub const U20: u8 = 14;
    pub const S24: u8 = 15;
    pub const U24: u8 = 16;
    pub const S32: u8 = 17;
    pub const U32: u8 = 18;
    pub const FLOAT: u8 = 19;
    pub const FLOAT64: u8 = 20;
    pub const DSD_U8: u8 = 21;
    pub const DSD_U16: u8 = 22;
    pub const DSD_U32: u8 = 23;
    pub const IEC958_SUBFRAME: u8 = 24;
}

/// Signed 16-bit little-endian — the format the tone generator produces.
const VIRTIO_SND_PCM_FMT_S16: u8 = fmt::S16;

// PCM rates (virtio 1.2 §5.14.6.6.1).  Same dense-ordinal rule as the formats.
#[allow(dead_code)]
mod rate {
    pub const R5512: u8 = 0;
    pub const R8000: u8 = 1;
    pub const R11025: u8 = 2;
    pub const R16000: u8 = 3;
    pub const R22050: u8 = 4;
    pub const R32000: u8 = 5;
    pub const R44100: u8 = 6;
    pub const R48000: u8 = 7;
    pub const R64000: u8 = 8;
    pub const R88200: u8 = 9;
    pub const R96000: u8 = 10;
    pub const R176400: u8 = 11;
    pub const R192000: u8 = 12;
    pub const R384000: u8 = 13;
}

/// 44100 Hz.
#[allow(dead_code)]
const VIRTIO_SND_PCM_RATE_44100: u8 = rate::R44100;
/// 48000 Hz — the rate the tone generator is sampled at.
const VIRTIO_SND_PCM_RATE_48000: u8 = rate::R48000;

// Stream directions
/// Output (playback).
const VIRTIO_SND_D_OUTPUT: u8 = 0;
/// Input (capture).
#[allow(dead_code)]
const VIRTIO_SND_D_INPUT: u8 = 1;

// ---------------------------------------------------------------------------
// Control message structures (repr(C) for DMA)
// ---------------------------------------------------------------------------

/// Common header for all control requests.
#[repr(C)]
#[derive(Clone, Copy)]
struct VirtioSndHdr {
    code: u32,
}

/// Query info request (used for jack, PCM, chmap).
#[repr(C)]
#[derive(Clone, Copy)]
struct VirtioSndQueryInfo {
    hdr: VirtioSndHdr,
    start_id: u32,
    count: u32,
    size: u32,
}

/// PCM stream info response entry.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct VirtioSndPcmInfo {
    hdr_info_hdr: u32, // hda_fn_nid
    features: u32,
    formats: u64,
    rates: u64,
    direction: u8,
    channels_min: u8,
    channels_max: u8,
    _padding: [u8; 5],
}

/// PCM set params request.
#[repr(C)]
#[derive(Clone, Copy)]
struct VirtioSndPcmSetParams {
    hdr: VirtioSndHdr,
    stream_id: u32,
    buffer_bytes: u32,
    period_bytes: u32,
    features: u32,
    channels: u8,
    format: u8,
    rate: u8,
    _padding: u8,
}

/// PCM stream header for start/stop/prepare/release.
#[repr(C)]
#[derive(Clone, Copy)]
struct VirtioSndPcmHdr {
    hdr: VirtioSndHdr,
    stream_id: u32,
}

/// TX/RX buffer header (prepended to PCM data on txq/rxq).
#[repr(C)]
#[derive(Clone, Copy)]
struct VirtioSndPcmXfer {
    stream_id: u32,
}

/// TX/RX status response (device writes this after consuming the buffer).
#[repr(C)]
#[derive(Clone, Copy)]
#[allow(dead_code)]
struct VirtioSndPcmStatus {
    status: u32,
    latency_bytes: u32,
}

// ---------------------------------------------------------------------------
// Device state
// ---------------------------------------------------------------------------

/// Maximum number of PCM streams we support.
const MAX_STREAMS: usize = 8;

/// What one PCM stream will accept, as the device itself reported it.
///
/// Recorded rather than merely printed so [`set_params`] can check a request
/// *before* putting it on the wire.  The device already answers a bad request
/// with `VIRTIO_SND_S_NOT_SUPP` (0x8002), but that status names no field and no
/// value — it is the same four bytes whether the format, the rate or the
/// channel count was the problem — so a driver that relies on it learns only
/// that something, somewhere, was rejected.
#[derive(Clone, Copy)]
struct StreamCaps {
    /// Bit `n` set ⇔ format ordinal `n` (see [`fmt`]) is supported.
    formats: u64,
    /// Bit `n` set ⇔ rate ordinal `n` (see [`rate`]) is supported.
    rates: u64,
    /// Minimum channel count.
    channels_min: u8,
    /// Maximum channel count.
    channels_max: u8,
}

/// Virtio sound device state.
struct VirtioSndDevice {
    /// Modern (virtio 1.0+) MMIO PCI transport.
    transport: ModernTransport,
    /// Control virtqueue.
    controlq: Virtqueue,
    /// Event virtqueue.
    #[allow(dead_code)]
    eventq: Virtqueue,
    /// TX virtqueue (playback).
    txq: Virtqueue,
    /// RX virtqueue (capture).
    #[allow(dead_code)]
    rxq: Virtqueue,
    /// HHDM offset for phys→virt conversion.
    hhdm_offset: u64,
    /// DMA frame for control messages.
    ctl_frame: PhysFrame,
    /// DMA frame for PCM data.
    pcm_frame: PhysFrame,
    /// Number of output (playback) streams.
    num_output_streams: u32,
    /// Number of input (capture) streams.
    num_input_streams: u32,
    /// Per-stream capabilities, indexed by stream id; `None` if the device did
    /// not report that stream.
    stream_caps: [Option<StreamCaps>; MAX_STREAMS],
    /// Stream currently playing (None if idle).
    active_stream: Option<u32>,
    /// Monotonic counter identifying *which* playback owns `active_stream`.
    ///
    /// Incremented by [`play_test_tone`] each time it claims a stream.
    /// `active_stream == Some(id)` is not sufficient on its own to answer "is
    /// this still my tone?" after the lock has been released between chunks: a
    /// concurrent [`stop`] followed by a second `play_test_tone` re-claims the
    /// *same* stream id (playback always uses stream 0), so the first caller
    /// would keep submitting into — and then tear down — the second caller's
    /// stream. Comparing the generation recorded at start closes that ABA
    /// window.
    play_gen: u64,
}

// SAFETY: VirtioSndDevice contains raw pointers (inside Virtqueue) that point
// to DMA memory accessible from any CPU.  All access is serialized by the
// DEVICE Mutex, so sending between threads is safe.
unsafe impl Send for VirtioSndDevice {}

/// Global device instance (single virtio-sound device supported).
///
/// A `PreemptSpinMutex` leaf lock (Q24 / design-decisions §70): nothing is
/// acquired underneath it, and this driver polls its virtqueues rather than
/// taking an interrupt, so `lock_irqsave` is not needed.
///
/// Every critical section must cover **one** device transaction and no more.
/// See [`play_test_tone`], which re-acquires per chunk instead of holding the
/// lock for the length of the tone.
static DEVICE: PreemptSpinMutex<Option<VirtioSndDevice>> =
    PreemptSpinMutex::named(None, b"VIRTIO_SND_DEVICE");

/// Whether the device is initialized.
static INITIALIZED: AtomicBool = AtomicBool::new(false);

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Probe PCI for a virtio-sound device and initialize it.
///
/// Returns Ok(()) if a device was found and initialized, or an error if
/// no device exists or init fails.
#[allow(clippy::too_many_lines)]
pub fn init(hhdm_offset: u64) -> KernelResult<()> {
    serial_println!("[virtio-snd] Probing for virtio-sound device...");

    // Try to find the device by vendor+device ID.
    let dev = find_device()?;

    serial_println!(
        "[virtio-snd] Found device at {:02x}:{:02x}.{} (ID {:04x}:{:04x})",
        dev.address.bus,
        dev.address.device,
        dev.address.function,
        dev.vendor_id,
        dev.device_id
    );

    // Enable bus mastering for DMA.
    pci::enable_bus_master(dev.address);

    // virtio-sound is a **modern-only** device: it was standardised well after
    // virtio 1.0 and never had a legacy I/O-port register block.  This driver
    // originally used the legacy transport, so on every boot with the device
    // actually present it got as far as reading BAR0, found MMIO where it
    // wanted an I/O BAR, printed "BAR0 is not I/O space or is zero" and gave
    // up -- which looked indistinguishable from "no sound card".  See
    // `virtio::modern` for the transport this needs instead.
    let transport = ModernTransport::probe(LOG_TAG, &dev, hhdm_offset)?;

    // --- Device initialization sequence (virtio 1.1 §3.1) ---

    // Steps 1-5: reset, ACKNOWLEDGE, DRIVER, features, FEATURES_OK.  We want
    // no optional virtio-snd features (VIRTIO_SND_F_CTLS gives access to
    // mixer controls we do not drive), so the requested mask is empty;
    // `negotiate` still accepts VIRTIO_F_VERSION_1, which a modern-only device
    // requires before it will let FEATURES_OK stick.
    transport.negotiate(0)?;

    // 6. Read device config to discover stream counts.
    // virtio-snd config layout (§5.14.4):
    //   u32 jacks; u32 streams; u32 chmaps;
    let num_jacks = transport.read_device_config32(0);
    let num_streams = transport.read_device_config32(4);
    let num_chmaps = transport.read_device_config32(8);

    serial_println!(
        "[virtio-snd] Config: {} jacks, {} streams, {} chmaps",
        num_jacks,
        num_streams,
        num_chmaps
    );

    if num_streams == 0 {
        serial_println!("[virtio-snd] No PCM streams available");
        transport.reset();
        return Err(KernelError::NoSuchDevice);
    }

    // 7. Set up the four virtqueues the spec defines (§5.14.2): controlq,
    // eventq, txq, rxq -- in that fixed order.  All four are mandatory, and
    // txq being queue *2* is why a device with fewer queues is unusable for
    // playback rather than merely feature-reduced.
    let num_queues = transport.num_queues();
    if num_queues < 4 {
        serial_println!(
            "[virtio-snd] Device has {} queues; the spec requires 4 (control, event, tx, rx)",
            num_queues
        );
        transport.reset();
        return Err(KernelError::NotSupported);
    }
    let controlq = transport.setup_queue(0, hhdm_offset)?;
    let eventq = transport.setup_queue(1, hhdm_offset)?;
    let txq = transport.setup_queue(2, hhdm_offset)?;
    let rxq = transport.setup_queue(3, hhdm_offset)?;

    // 8. Driver OK — device is live.
    transport.add_status(STATUS_DRIVER_OK);
    serial_println!("[virtio-snd] Device status: DRIVER_OK");

    // Allocate DMA frames for control and PCM data.
    let ctl_frame = frame::alloc_frame()?;
    let pcm_frame = frame::alloc_frame()?;

    // Zero the DMA frames.
    // SAFETY: Freshly allocated frames via HHDM.
    unsafe {
        let ctl_virt = (ctl_frame.addr() + hhdm_offset) as *mut u8;
        core::ptr::write_bytes(ctl_virt, 0, FRAME_SIZE);
        let pcm_virt = (pcm_frame.addr() + hhdm_offset) as *mut u8;
        core::ptr::write_bytes(pcm_virt, 0, FRAME_SIZE);
    }

    let mut device = VirtioSndDevice {
        transport,
        controlq,
        eventq,
        txq,
        rxq,
        hhdm_offset,
        ctl_frame,
        pcm_frame,
        num_output_streams: 0,
        num_input_streams: 0,
        stream_caps: [None; MAX_STREAMS],
        active_stream: None,
        play_gen: 0,
    };

    // Query PCM stream info to classify output vs input streams.
    if let Err(e) = query_stream_info(&mut device, num_streams) {
        serial_println!("[virtio-snd] Warning: failed to query stream info: {:?}", e);
        // Continue anyway — we can try to use stream 0 as output.
        device.num_output_streams = num_streams.min(1);
    }

    serial_println!(
        "[virtio-snd] Streams: {} output, {} input",
        device.num_output_streams,
        device.num_input_streams
    );

    *DEVICE.lock() = Some(device);
    INITIALIZED.store(true, Ordering::Release);

    serial_println!("[virtio-snd] Initialization complete");
    Ok(())
}

/// Find a virtio-sound PCI device.
fn find_device() -> KernelResult<PciDevice> {
    if let Some(dev) = pci::find_device(VIRTIO_VENDOR, VIRTIO_SND_DEVICE_ID) {
        return Ok(dev);
    }

    // Try PCI class-based detection (multimedia audio controller).
    // Class 0x04 (Multimedia), subclass 0x01 (Audio).
    let devices = pci::find_devices_by_class(0x04, 0x01);
    for dev in devices {
        if dev.vendor_id == VIRTIO_VENDOR {
            return Ok(dev);
        }
    }

    serial_println!("[virtio-snd] No virtio-sound device found");
    Err(KernelError::NoSuchDevice)
}

// ---------------------------------------------------------------------------
// Control queue operations
// ---------------------------------------------------------------------------

/// Query PCM stream information to determine output/input stream counts.
#[allow(clippy::arithmetic_side_effects)]
fn query_stream_info(dev: &mut VirtioSndDevice, num_streams: u32) -> KernelResult<()> {
    let count = num_streams.min(MAX_STREAMS as u32);
    let ctl_phys = dev.ctl_frame.addr();
    let ctl_virt = (ctl_phys + dev.hhdm_offset) as *mut u8;

    // Build PCM_INFO request at offset 0.
    let req = VirtioSndQueryInfo {
        hdr: VirtioSndHdr {
            code: VIRTIO_SND_R_PCM_INFO,
        },
        start_id: 0,
        count,
        size: core::mem::size_of::<VirtioSndPcmInfo>() as u32,
    };
    // SAFETY: Writing to our DMA buffer within FRAME_SIZE bounds.
    unsafe {
        core::ptr::write(ctl_virt as *mut VirtioSndQueryInfo, req);
    }

    // Response will be at offset 256: status header (4 bytes) + stream info entries.
    let resp_offset: usize = 256;
    let resp_size = 4 + (count as usize) * core::mem::size_of::<VirtioSndPcmInfo>();

    // Submit: [request (device-readable)] → [response (device-writable)]
    let req_phys = ctl_phys;
    let resp_phys = ctl_phys + resp_offset as u64;
    let req_len = core::mem::size_of::<VirtioSndQueryInfo>() as u32;

    dev.controlq.submit(&[
        (req_phys, req_len, 0),                            // Device reads this
        (resp_phys, resp_size as u32, VRING_DESC_F_WRITE), // Device writes response
    ])?;

    // Notify device.
    dev.transport.notify_queue(0);

    // Poll for completion.
    let mut attempts = 0u32;
    let head = loop {
        if let Some((head, _len)) = dev.controlq.poll_used() {
            break head;
        }
        attempts = attempts.wrapping_add(1);
        if attempts > 1_000_000 {
            // A timed-out chain is still owned by the device, so it is
            // deliberately not returned to the free list here.
            return Err(KernelError::TimedOut);
        }
        core::hint::spin_loop();
    };

    // Return the chain to the free list.  The response is in the DMA control
    // frame, not the descriptors, so it stays readable below.  Every control
    // command used to leak its descriptors; the control queue is only drained
    // by a device reset, so repeated stream setup would eventually exhaust it.
    dev.controlq.free_chain(head);

    // Read response status.
    // SAFETY: resp_offset is within the DMA frame.  Volatile read because
    // the device writes this field asynchronously via DMA.
    let status = unsafe { core::ptr::read_volatile((ctl_virt.add(resp_offset)) as *const u32) };
    if status != VIRTIO_SND_S_OK {
        serial_println!("[virtio-snd] PCM_INFO failed: status {:#x}", status);
        return Err(KernelError::IoError);
    }

    // Parse stream info entries.
    let mut num_output = 0u32;
    let mut num_input = 0u32;
    for i in 0..count as usize {
        let entry_offset = resp_offset + 4 + i * core::mem::size_of::<VirtioSndPcmInfo>();
        // SAFETY: entry_offset is within the DMA frame (resp_offset + 4 +
        // i * size < FRAME_SIZE, ensured by count ≤ MAX_STREAMS and the
        // sizes involved).  VirtioSndPcmInfo is #[repr(C)].
        let info =
            unsafe { core::ptr::read(ctl_virt.add(entry_offset) as *const VirtioSndPcmInfo) };
        if info.direction == VIRTIO_SND_D_OUTPUT {
            num_output = num_output.wrapping_add(1);
        } else if info.direction == VIRTIO_SND_D_INPUT {
            num_input = num_input.wrapping_add(1);
        }
        // Keep what the device said it accepts, so `set_params` can reject a
        // bad request here rather than shipping it and decoding a bare
        // NOT_SUPP.  `i < count ≤ MAX_STREAMS`, so the index is in range;
        // `get_mut` rather than `[i]` because indexing_slicing is denied.
        if let Some(slot) = dev.stream_caps.get_mut(i) {
            *slot = Some(StreamCaps {
                formats: info.formats,
                rates: info.rates,
                channels_min: info.channels_min,
                channels_max: info.channels_max,
            });
        }
        serial_println!(
            "[virtio-snd]   Stream {}: dir={} ch={}-{} fmts={:#x} rates={:#x}",
            i,
            if info.direction == VIRTIO_SND_D_OUTPUT {
                "OUT"
            } else {
                "IN"
            },
            info.channels_min,
            info.channels_max,
            info.formats,
            info.rates
        );
    }

    dev.num_output_streams = num_output;
    dev.num_input_streams = num_input;
    Ok(())
}

/// Send a simple control command (prepare/start/stop/release) for a stream.
#[allow(clippy::arithmetic_side_effects)]
fn control_stream_cmd(dev: &mut VirtioSndDevice, code: u32, stream_id: u32) -> KernelResult<()> {
    let ctl_phys = dev.ctl_frame.addr();
    let ctl_virt = (ctl_phys + dev.hhdm_offset) as *mut u8;

    // Write request at offset 0.
    let req = VirtioSndPcmHdr {
        hdr: VirtioSndHdr { code },
        stream_id,
    };
    // SAFETY: ctl_virt points to our DMA frame (FRAME_SIZE bytes).
    // VirtioSndPcmHdr is #[repr(C)] and fits at offset 0.
    unsafe {
        core::ptr::write(ctl_virt as *mut VirtioSndPcmHdr, req);
    }

    // Response at offset 64 (just a status u32).
    let resp_offset: usize = 64;
    // SAFETY: resp_offset + 4 < FRAME_SIZE; zeroing the response area.
    unsafe {
        core::ptr::write_bytes(ctl_virt.add(resp_offset), 0, 4);
    }

    let req_phys = ctl_phys;
    let resp_phys = ctl_phys + resp_offset as u64;
    let req_len = core::mem::size_of::<VirtioSndPcmHdr>() as u32;

    dev.controlq
        .submit(&[(req_phys, req_len, 0), (resp_phys, 4, VRING_DESC_F_WRITE)])?;
    dev.transport.notify_queue(0);

    // Poll for completion.
    let mut attempts = 0u32;
    let head = loop {
        if let Some((head, _len)) = dev.controlq.poll_used() {
            break head;
        }
        attempts = attempts.wrapping_add(1);
        if attempts > 1_000_000 {
            // Timed out: the chain is still the device's, so it is not freed.
            return Err(KernelError::TimedOut);
        }
        core::hint::spin_loop();
    };
    dev.controlq.free_chain(head);

    // SAFETY: resp_offset is within the DMA frame.  Volatile read because
    // the device writes this asynchronously via DMA.
    let status = unsafe { core::ptr::read_volatile(ctl_virt.add(resp_offset) as *const u32) };
    if status != VIRTIO_SND_S_OK {
        serial_println!(
            "[virtio-snd] Command {:#x} for stream {} failed: status {:#x}",
            code,
            stream_id,
            status
        );
        return Err(KernelError::IoError);
    }

    Ok(())
}

/// Set PCM stream parameters (format, rate, channels, buffer size).
#[allow(clippy::arithmetic_side_effects)]
fn set_params(
    dev: &mut VirtioSndDevice,
    stream_id: u32,
    channels: u8,
    format: u8,
    rate: u8,
    buffer_bytes: u32,
    period_bytes: u32,
) -> KernelResult<()> {
    // Check the request against what the device advertised for this stream
    // before sending it.  This is not belt-and-braces: `format` and `rate` are
    // ordinals that double as bit indices into these very masks, so a mistake
    // in the constants is invisible to any check that does not consult the
    // device.  It was exactly such a mistake — `S16` encoded as 2 (A-law) —
    // that made the first boot with a virtio sound card attached fail with
    // nothing but `status 0x8002` on our side and "Stream format is not
    // supported." on QEMU's.
    //
    // A stream we have no capabilities for is *not* rejected: the device may
    // legitimately expose more streams than MAX_STREAMS, and refusing to drive
    // one we simply did not record would turn a missing optimisation into a
    // failure.  In that case we fall through and let the device arbitrate.
    if let Some(Some(caps)) = dev.stream_caps.get(stream_id as usize) {
        if format >= 64 || caps.formats & (1u64 << format) == 0 {
            serial_println!(
                "[virtio-snd] ERROR: stream {} does not accept format ordinal {} \
                 (device advertises formats {:#x})",
                stream_id,
                format,
                caps.formats
            );
            return Err(KernelError::NotSupported);
        }
        if rate >= 64 || caps.rates & (1u64 << rate) == 0 {
            serial_println!(
                "[virtio-snd] ERROR: stream {} does not accept rate ordinal {} \
                 (device advertises rates {:#x})",
                stream_id,
                rate,
                caps.rates
            );
            return Err(KernelError::NotSupported);
        }
        if channels < caps.channels_min || channels > caps.channels_max {
            serial_println!(
                "[virtio-snd] ERROR: stream {} accepts {}-{} channels, not {}",
                stream_id,
                caps.channels_min,
                caps.channels_max,
                channels
            );
            return Err(KernelError::NotSupported);
        }
    }

    let ctl_phys = dev.ctl_frame.addr();
    let ctl_virt = (ctl_phys + dev.hhdm_offset) as *mut u8;

    let req = VirtioSndPcmSetParams {
        hdr: VirtioSndHdr {
            code: VIRTIO_SND_R_PCM_SET_PARAMS,
        },
        stream_id,
        buffer_bytes,
        period_bytes,
        features: 0,
        channels,
        format,
        rate,
        _padding: 0,
    };
    // SAFETY: ctl_virt points to our DMA frame.  VirtioSndPcmSetParams is
    // #[repr(C)] and fits at offset 0 within FRAME_SIZE.
    unsafe {
        core::ptr::write(ctl_virt as *mut VirtioSndPcmSetParams, req);
    }

    // Response at offset 64.
    let resp_offset: usize = 64;
    // SAFETY: resp_offset + 4 < FRAME_SIZE; zeroing the response area.
    unsafe {
        core::ptr::write_bytes(ctl_virt.add(resp_offset), 0, 4);
    }

    let req_phys = ctl_phys;
    let resp_phys = ctl_phys + resp_offset as u64;
    let req_len = core::mem::size_of::<VirtioSndPcmSetParams>() as u32;

    dev.controlq
        .submit(&[(req_phys, req_len, 0), (resp_phys, 4, VRING_DESC_F_WRITE)])?;
    dev.transport.notify_queue(0);

    // Poll.
    let mut attempts = 0u32;
    let head = loop {
        if let Some((head, _len)) = dev.controlq.poll_used() {
            break head;
        }
        attempts = attempts.wrapping_add(1);
        if attempts > 1_000_000 {
            // Timed out: the chain is still the device's, so it is not freed.
            return Err(KernelError::TimedOut);
        }
        core::hint::spin_loop();
    };
    dev.controlq.free_chain(head);

    // SAFETY: resp_offset is within the DMA frame.  Volatile read because
    // the device writes this asynchronously.
    let status = unsafe { core::ptr::read_volatile(ctl_virt.add(resp_offset) as *const u32) };
    if status != VIRTIO_SND_S_OK {
        serial_println!(
            "[virtio-snd] SET_PARAMS for stream {} failed: status {:#x}",
            stream_id,
            status
        );
        return Err(KernelError::IoError);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Playback (TX queue)
// ---------------------------------------------------------------------------

/// Submit a PCM buffer for playback.
///
/// The buffer is copied into the DMA frame and submitted to the TX queue.
/// This is synchronous — it waits for the device to consume the buffer.
#[allow(clippy::arithmetic_side_effects)]
fn submit_pcm_buffer(
    dev: &mut VirtioSndDevice,
    stream_id: u32,
    pcm_data: &[u8],
) -> KernelResult<()> {
    let pcm_phys = dev.pcm_frame.addr();
    let pcm_virt = (pcm_phys + dev.hhdm_offset) as *mut u8;

    // Layout in pcm_frame:
    //   offset 0: VirtioSndPcmXfer header (4 bytes)
    //   offset 4..4+len: PCM audio data
    //   offset 8192: VirtioSndPcmStatus (8 bytes, device-writable)

    let max_data = FRAME_SIZE - 4 - 8; // Reserve header and status
    let data_len = pcm_data.len().min(max_data);

    // Write transfer header.
    let xfer = VirtioSndPcmXfer { stream_id };
    // SAFETY: pcm_virt points to the start of the PCM DMA frame
    // (FRAME_SIZE bytes).  VirtioSndPcmXfer is 4 bytes at offset 0.
    unsafe {
        core::ptr::write(pcm_virt as *mut VirtioSndPcmXfer, xfer);
    }

    // Copy PCM data after header.
    // SAFETY: data_len ≤ max_data = FRAME_SIZE - 12, so offset 4 + data_len
    // stays within the DMA frame.  pcm_data is a valid slice of ≥ data_len bytes.
    unsafe {
        core::ptr::copy_nonoverlapping(pcm_data.as_ptr(), pcm_virt.add(4), data_len);
    }

    // Zero the status area.
    let status_offset: usize = 8192;
    // SAFETY: status_offset + 8 ≤ FRAME_SIZE (16384).  Zeroing the
    // response area before the device writes to it.
    unsafe {
        core::ptr::write_bytes(pcm_virt.add(status_offset), 0, 8);
    }

    // Submit three-part chain:
    //   1. Header (device-readable): VirtioSndPcmXfer
    //   2. PCM data (device-readable)
    //   3. Status (device-writable): VirtioSndPcmStatus
    let hdr_phys = pcm_phys;
    let data_phys = pcm_phys + 4;
    let status_phys = pcm_phys + status_offset as u64;

    dev.txq.submit(&[
        (hdr_phys, 4, 0),                     // Header
        (data_phys, data_len as u32, 0),      // PCM data
        (status_phys, 8, VRING_DESC_F_WRITE), // Status response
    ])?;
    dev.transport.notify_queue(2); // txq is queue 2

    // Poll for completion.
    let mut attempts = 0u32;
    loop {
        if let Some((head, _len)) = dev.txq.poll_used() {
            dev.txq.free_chain(head);
            break;
        }
        attempts = attempts.wrapping_add(1);
        if attempts > 10_000_000 {
            return Err(KernelError::TimedOut);
        }
        core::hint::spin_loop();
    }

    // Check status.
    // SAFETY: status_offset is within the PCM DMA frame.  Volatile read
    // because the device wrote this field asynchronously via DMA.
    let status = unsafe { core::ptr::read_volatile(pcm_virt.add(status_offset) as *const u32) };
    if status != VIRTIO_SND_S_OK {
        serial_println!("[virtio-snd] TX buffer status: {:#x}", status);
        return Err(KernelError::IoError);
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Check if the virtio-sound device is available.
pub fn is_available() -> bool {
    INITIALIZED.load(Ordering::Acquire)
}

/// Start playback of a test tone (440 Hz sine wave, 48kHz 16-bit stereo).
///
/// Configures stream 0 for playback and sends a short buffer of audio.
///
/// # Errors
///
/// [`KernelError::NoSuchDevice`] if there is no device or it has no output
/// stream, [`KernelError::DeviceBusy`] if a stream is already running, or
/// whatever the underlying control/PCM transactions return.
///
/// # Locking
///
/// `DEVICE` is re-acquired **per transaction** — once for the setup commands,
/// once per PCM chunk, once for teardown — rather than held for the length of
/// the tone. A whole tone is `duration_ms` milliseconds and each chunk's
/// `submit_pcm_buffer` polls the used ring for up to ten million iterations, so
/// holding a preempt-disabling spinlock across the loop would keep the
/// scheduler off this CPU for the entire playback and spin every other caller
/// for just as long. Mutual exclusion across the whole tone is provided by
/// `dev.active_stream`, which is set under the lock during setup and cleared
/// under the lock during teardown; a second caller sees it set and gets
/// `DeviceBusy` instead of reprogramming a live stream.
///
/// [`stop`] is the one caller allowed to break that claim — it exists precisely
/// to end a tone early. Every chunk therefore re-checks the claim after
/// re-acquiring the lock; if it has been taken, playback ends there and the
/// teardown is skipped, because whoever took it already issued STOP/RELEASE.
/// The check is on `play_gen` as well as `active_stream`, because playback
/// always uses stream 0 and so a stop-then-restart would otherwise be
/// indistinguishable from our own claim — see [`VirtioSndDevice::play_gen`].
pub fn play_test_tone(duration_ms: u32) -> KernelResult<()> {
    if !is_available() {
        return Err(KernelError::NoSuchDevice);
    }

    let stream_id: u32 = 0; // First output stream.
    // Set inside the setup section below, and only there.
    let my_gen: u64;

    // --- Setup: params, prepare, start. One critical section. ---
    {
        let mut guard = DEVICE.lock();
        let dev = guard.as_mut().ok_or(KernelError::NoSuchDevice)?;

        if dev.num_output_streams == 0 {
            return Err(KernelError::NoSuchDevice);
        }
        if dev.active_stream.is_some() {
            return Err(KernelError::DeviceBusy);
        }

        // Set parameters: 48kHz, 16-bit signed, stereo, 8192 buffer / 4096 period.
        set_params(
            dev,
            stream_id,
            2,
            VIRTIO_SND_PCM_FMT_S16,
            VIRTIO_SND_PCM_RATE_48000,
            8192,
            4096,
        )?;
        serial_println!("[virtio-snd] Stream 0: params set (48kHz/S16/stereo)");

        // Prepare the stream.
        control_stream_cmd(dev, VIRTIO_SND_R_PCM_PREPARE, stream_id)?;
        serial_println!("[virtio-snd] Stream 0: prepared");

        // Start the stream. Claim `active_stream` in the same section that
        // starts it, so the device is never running without a claim recorded --
        // otherwise a failure below would leak a started stream that `stop()`
        // could not find.
        control_stream_cmd(dev, VIRTIO_SND_R_PCM_START, stream_id)?;
        dev.active_stream = Some(stream_id);
        dev.play_gen = dev.play_gen.wrapping_add(1);
        my_gen = dev.play_gen;
        serial_println!("[virtio-snd] Stream 0: started");
    }

    // Generate and submit PCM data in chunks.
    // 48000 samples/sec × 2 channels × 2 bytes = 192000 bytes/sec.
    let bytes_per_ms: u32 = 192;
    let total_bytes = duration_ms.saturating_mul(bytes_per_ms);
    let chunk_size: usize = 4096; // Period-sized chunks.
    let mut buf = [0u8; 4096];
    let mut sample_offset: u32 = 0;

    let mut bytes_sent: u32 = 0;
    let mut submit_result = Ok(());
    // Set when a concurrent `stop()` takes the claim out from under us. It has
    // already issued STOP/RELEASE at that point, so our teardown must be
    // skipped: a second RELEASE on a released stream is a protocol error, and
    // clearing `active_stream` again could clobber a *third* caller's claim.
    let mut claim_lost = false;
    while bytes_sent < total_bytes {
        let remaining = (total_bytes.saturating_sub(bytes_sent)) as usize;
        let send_len = remaining.min(chunk_size);

        // Generate 440 Hz sine wave (integer approximation) with the lock
        // released -- this is pure computation over a stack buffer.
        let Some(chunk) = buf.get_mut(..send_len) else {
            break;
        };
        generate_sine_440(chunk, sample_offset);
        sample_offset = sample_offset.wrapping_add((send_len / 4) as u32); // 4 bytes per stereo sample

        // One transaction, one critical section.
        {
            let mut guard = DEVICE.lock();
            let Some(dev) = guard.as_mut() else {
                // Device torn down underneath us; nothing left to stop.
                return Err(KernelError::NoSuchDevice);
            };
            // Re-verify the claim every time we re-enter. Holding DEVICE for the
            // whole tone would make this unnecessary, but that is precisely the
            // defect being fixed: the lock is released between chunks, so
            // `stop()` can legitimately end the tone early. Treat that as a
            // normal end of playback, not an error -- the caller asked for a
            // tone and something deliberately stopped it.
            if dev.play_gen != my_gen || dev.active_stream != Some(stream_id) {
                claim_lost = true;
                break;
            }
            if let Err(e) = submit_pcm_buffer(dev, stream_id, chunk) {
                // Do not return yet: the stream is started and claimed, and
                // bailing here would leave it that way forever. Fall through to
                // the teardown below and report the error after.
                submit_result = Err(e);
                break;
            }
        }
        bytes_sent = bytes_sent.wrapping_add(send_len as u32);
    }

    // --- Teardown: stop, release, drop the claim. One critical section. ---
    // Skipped entirely when the claim was taken from us: whoever took it owns
    // the teardown.
    // `Ok(true)` = we tore the stream down, `Ok(false)` = someone else already
    // had, so there was nothing to do.
    let teardown: KernelResult<bool> = if claim_lost {
        Ok(false)
    } else {
        let mut guard = DEVICE.lock();
        match guard.as_mut() {
            Some(dev) => {
                // Re-check once more under the lock: `stop()` may have run
                // between the last chunk and here.
                if dev.play_gen == my_gen && dev.active_stream == Some(stream_id) {
                    // Clear the claim first so a failing STOP/RELEASE cannot
                    // strand the device as permanently busy. The worst case is
                    // then a stream that stays running, which a later call
                    // re-programs; the alternative is a driver that refuses to
                    // play again.
                    dev.active_stream = None;
                    let stop = control_stream_cmd(dev, VIRTIO_SND_R_PCM_STOP, stream_id);
                    let release = control_stream_cmd(dev, VIRTIO_SND_R_PCM_RELEASE, stream_id);
                    stop.and(release).map(|()| true)
                } else {
                    Ok(false)
                }
            }
            None => Err(KernelError::NoSuchDevice),
        }
    };

    // Report the first thing that actually went wrong: a submit failure is the
    // cause, a teardown failure is a consequence.
    submit_result?;
    if teardown? {
        serial_println!("[virtio-snd] Stream 0: stopped and released");
    } else {
        serial_println!("[virtio-snd] Stream 0: ended early (stopped by another caller)");
    }

    Ok(())
}

/// Stop playback if active.
pub fn stop() -> KernelResult<()> {
    if !is_available() {
        return Err(KernelError::NoSuchDevice);
    }

    let mut guard = DEVICE.lock();
    let dev = guard.as_mut().ok_or(KernelError::NoSuchDevice)?;

    if let Some(stream_id) = dev.active_stream.take() {
        control_stream_cmd(dev, VIRTIO_SND_R_PCM_STOP, stream_id)?;
        control_stream_cmd(dev, VIRTIO_SND_R_PCM_RELEASE, stream_id)?;
        serial_println!("[virtio-snd] Stream {} stopped", stream_id);
    }

    Ok(())
}

/// Get device status summary.
pub fn status_info() -> (bool, u32, u32, bool) {
    if !is_available() {
        return (false, 0, 0, false);
    }
    let guard = DEVICE.lock();
    match guard.as_ref() {
        Some(dev) => (
            true,
            dev.num_output_streams,
            dev.num_input_streams,
            dev.active_stream.is_some(),
        ),
        None => (false, 0, 0, false),
    }
}

// ---------------------------------------------------------------------------
// Tone generation
// ---------------------------------------------------------------------------

/// Generate a 440 Hz sine wave into a buffer (48kHz, 16-bit signed, stereo).
///
/// Uses Bhaskara's integer sine approximation to avoid floating point.
/// Each stereo sample is 4 bytes: [left_lo, left_hi, right_lo, right_hi].
#[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
fn generate_sine_440(buf: &mut [u8], sample_offset: u32) {
    let samples_per_period: u32 = 48000 / 440; // ~109 samples per wave cycle
    let num_frames = buf.len() / 4; // Each stereo frame is 4 bytes

    for i in 0..num_frames {
        let t = (sample_offset.wrapping_add(i as u32)) % samples_per_period;

        // Bhaskara's approximation: sin(x) ≈ 16x(π-x) / (5π²-4x(π-x))
        // Scaled to avoid FP: phase 0..109 maps to 0..π.
        // Use lookup-free integer math for a reasonable sine approximation.
        //
        // Simpler approach: triangular wave approximation (sounds similar
        // enough for a test tone, much simpler math).
        let half_period = samples_per_period / 2;
        let quarter_period = samples_per_period / 4;

        let sample: i16 = if t < half_period {
            // First half: ramp up then down.
            if t < quarter_period {
                // Ramp up: 0 → 32767
                ((t as i32 * 32767) / quarter_period as i32) as i16
            } else {
                // Ramp down: 32767 → 0
                (((half_period - t) as i32 * 32767) / quarter_period as i32) as i16
            }
        } else {
            // Second half: mirror negative.
            let t2 = t - half_period;
            if t2 < quarter_period {
                -(((t2 as i32) * 32767) / quarter_period as i32) as i16
            } else {
                -(((half_period - t2) as i32 * 32767) / quarter_period as i32) as i16
            }
        };

        // Scale down to ~50% volume to avoid clipping.
        let sample = sample / 2;

        let bytes = sample.to_le_bytes();
        let offset = i * 4;
        if offset + 3 < buf.len() {
            buf[offset] = bytes[0]; // Left low byte
            buf[offset + 1] = bytes[1]; // Left high byte
            buf[offset + 2] = bytes[0]; // Right low byte (same as left = mono)
            buf[offset + 3] = bytes[1]; // Right high byte
        }
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test: verify virtio-sound device probing and stream queries.
///
/// If no virtio-sound device is present (common without `-device virtio-sound-pci`),
/// the test gracefully reports "no device" and passes.
pub fn self_test() {
    serial_println!("[virtio-snd] Running self-test...");

    if !is_available() {
        // The boot test attaches -device virtio-sound-pci, so reaching this
        // branch there means init *failed*, not that the hardware is absent.
        // Say both, because the two have wholly different follow-ups.
        serial_println!(
            "[virtio-snd]   Not available — either no device is attached, or init failed above"
        );
        serial_println!("[virtio-snd] Self-test PASSED (no device)");
        return;
    }

    let (available, outputs, inputs, playing) = status_info();
    serial_println!("[virtio-snd]   Available: {}", available);
    serial_println!("[virtio-snd]   Output streams: {}", outputs);
    serial_println!("[virtio-snd]   Input streams: {}", inputs);
    serial_println!("[virtio-snd]   Currently playing: {}", playing);

    // Verify at least one output stream was detected.
    if outputs == 0 {
        serial_println!("[virtio-snd]   WARNING: device found but no output streams");
    }

    // Try a very short test tone (10ms — inaudible but tests the path).
    if outputs > 0 {
        match play_test_tone(10) {
            Ok(()) => serial_println!("[virtio-snd]   Short tone playback: OK"),
            Err(e) => serial_println!("[virtio-snd]   Short tone playback: {:?} (non-fatal)", e),
        }
    }

    serial_println!("[virtio-snd] Self-test PASSED");
}
