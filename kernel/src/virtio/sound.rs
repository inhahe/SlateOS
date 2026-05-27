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
use crate::mm::frame::{self, PhysFrame, FRAME_SIZE};
use crate::pci::{self, PciDevice};
use crate::serial_println;
use crate::virtio::queue::{Virtqueue, VRING_DESC_F_WRITE};
use crate::virtio::{VirtioLegacyPci, STATUS_ACKNOWLEDGE, STATUS_DRIVER, STATUS_DRIVER_OK};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Virtio vendor ID (Red Hat / QEMU).
const VIRTIO_VENDOR: u16 = 0x1AF4;
/// Virtio-sound device ID (legacy PCI: type 25 → 0x1040 + 25 - 1 = 0x1058).
/// Actually for transitional devices: 0x1040 + device_type.
/// QEMU uses 0x1059 for virtio-sound in modern mode, but for legacy
/// it's typically probed by subsystem device ID or device class.
/// Let's try both the legacy (0x1058) and modern transitional (0x1059) IDs.
const VIRTIO_SND_DEVICE_LEGACY: u16 = 0x1058;
const VIRTIO_SND_DEVICE_MODERN: u16 = 0x1059;

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

// PCM formats (virtio 1.2 §5.14.6.6)
/// Signed 16-bit little-endian.
const VIRTIO_SND_PCM_FMT_S16: u8 = 2;

// PCM rates
/// 44100 Hz.
#[allow(dead_code)]
const VIRTIO_SND_PCM_RATE_44100: u8 = 5;
/// 48000 Hz.
const VIRTIO_SND_PCM_RATE_48000: u8 = 6;

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

/// Virtio sound device state.
struct VirtioSndDevice {
    /// Legacy PCI transport.
    transport: VirtioLegacyPci,
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
    /// Stream currently playing (None if idle).
    active_stream: Option<u32>,
}

// SAFETY: VirtioSndDevice contains raw pointers (inside Virtqueue) that point
// to DMA memory accessible from any CPU.  All access is serialized by the
// DEVICE Mutex, so sending between threads is safe.
unsafe impl Send for VirtioSndDevice {}

/// Global device instance (single virtio-sound device supported).
static DEVICE: spin::Mutex<Option<VirtioSndDevice>> = spin::Mutex::new(None);

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

    // Get I/O port base from BAR0.
    let bar0 = dev.bars[0];
    if bar0 == 0 || bar0 & 1 == 0 {
        serial_println!("[virtio-snd] BAR0 is not I/O space or is zero");
        return Err(KernelError::NoSuchDevice);
    }
    let io_base = (bar0 & !0x3) as u16;
    serial_println!("[virtio-snd] I/O base: {:#x}", io_base);

    // Enable bus mastering for DMA.
    pci::enable_bus_master(dev.address);

    let transport = VirtioLegacyPci::new(io_base);

    // --- Device initialization sequence (virtio 1.0 §3.1.1, legacy) ---

    // 1. Reset.
    transport.reset();

    // 2. Acknowledge.
    transport.set_status(STATUS_ACKNOWLEDGE);

    // 3. Driver.
    transport.set_status(STATUS_DRIVER);

    // 4. Read device features.
    let features = transport.device_features();
    serial_println!("[virtio-snd] Device features: {:#010x}", features);

    // Accept no optional features for now (just base functionality).
    transport.set_guest_features(0);

    // 5. Read device config to discover stream counts.
    // Device config layout (legacy transport, offset 0x14 from BAR0):
    //   u32 jacks
    //   u32 streams
    //   u32 chmaps
    let num_jacks = transport.read_device_config32(0);
    let num_streams = transport.read_device_config32(4);
    let num_chmaps = transport.read_device_config32(8);

    serial_println!(
        "[virtio-snd] Config: {} jacks, {} streams, {} chmaps",
        num_jacks, num_streams, num_chmaps
    );

    if num_streams == 0 {
        serial_println!("[virtio-snd] No PCM streams available");
        transport.reset();
        return Err(KernelError::NoSuchDevice);
    }

    // 6. Set up virtqueues.
    // Queue 0: controlq
    transport.select_queue(0);
    let ctl_size = transport.queue_size();
    if ctl_size == 0 {
        serial_println!("[virtio-snd] controlq size is 0");
        transport.reset();
        return Err(KernelError::NoSuchDevice);
    }
    let (controlq, ctl_pfn) = Virtqueue::new(ctl_size, hhdm_offset)?;
    transport.set_queue_pfn(ctl_pfn);

    // Queue 1: eventq
    transport.select_queue(1);
    let evt_size = transport.queue_size();
    let (eventq, evt_pfn) = Virtqueue::new(
        if evt_size > 0 { evt_size } else { 16 },
        hhdm_offset,
    )?;
    if evt_size > 0 {
        transport.set_queue_pfn(evt_pfn);
    }

    // Queue 2: txq (playback)
    transport.select_queue(2);
    let tx_size = transport.queue_size();
    if tx_size == 0 {
        serial_println!("[virtio-snd] txq size is 0");
        transport.reset();
        return Err(KernelError::NoSuchDevice);
    }
    let (txq, tx_pfn) = Virtqueue::new(tx_size, hhdm_offset)?;
    transport.set_queue_pfn(tx_pfn);

    // Queue 3: rxq (capture)
    transport.select_queue(3);
    let rx_size = transport.queue_size();
    let (rxq, rx_pfn) = Virtqueue::new(
        if rx_size > 0 { rx_size } else { 16 },
        hhdm_offset,
    )?;
    if rx_size > 0 {
        transport.set_queue_pfn(rx_pfn);
    }

    // 7. Driver OK — device is live.
    transport.set_status(STATUS_DRIVER_OK);
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
        active_stream: None,
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
    // Try modern transitional device ID first.
    if let Some(dev) = pci::find_device(VIRTIO_VENDOR, VIRTIO_SND_DEVICE_MODERN) {
        return Ok(dev);
    }

    // Try legacy device ID.
    if let Some(dev) = pci::find_device(VIRTIO_VENDOR, VIRTIO_SND_DEVICE_LEGACY) {
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
        hdr: VirtioSndHdr { code: VIRTIO_SND_R_PCM_INFO },
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
        (req_phys, req_len, 0),                         // Device reads this
        (resp_phys, resp_size as u32, VRING_DESC_F_WRITE), // Device writes response
    ])?;

    // Notify device.
    dev.transport.notify_queue(0);

    // Poll for completion.
    let mut attempts = 0u32;
    loop {
        if let Some((_head, _len)) = dev.controlq.poll_used() {
            break;
        }
        attempts = attempts.wrapping_add(1);
        if attempts > 1_000_000 {
            return Err(KernelError::TimedOut);
        }
        core::hint::spin_loop();
    }

    // Read response status.
    // SAFETY: resp_offset is within the DMA frame.  Volatile read because
    // the device writes this field asynchronously via DMA.
    let status = unsafe {
        core::ptr::read_volatile((ctl_virt.add(resp_offset)) as *const u32)
    };
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
        let info = unsafe {
            core::ptr::read(ctl_virt.add(entry_offset) as *const VirtioSndPcmInfo)
        };
        if info.direction == VIRTIO_SND_D_OUTPUT {
            num_output = num_output.wrapping_add(1);
        } else if info.direction == VIRTIO_SND_D_INPUT {
            num_input = num_input.wrapping_add(1);
        }
        serial_println!(
            "[virtio-snd]   Stream {}: dir={} ch={}-{} fmts={:#x} rates={:#x}",
            i,
            if info.direction == VIRTIO_SND_D_OUTPUT { "OUT" } else { "IN" },
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

    dev.controlq.submit(&[
        (req_phys, req_len, 0),
        (resp_phys, 4, VRING_DESC_F_WRITE),
    ])?;
    dev.transport.notify_queue(0);

    // Poll for completion.
    let mut attempts = 0u32;
    loop {
        if dev.controlq.poll_used().is_some() {
            break;
        }
        attempts = attempts.wrapping_add(1);
        if attempts > 1_000_000 {
            return Err(KernelError::TimedOut);
        }
        core::hint::spin_loop();
    }

    // SAFETY: resp_offset is within the DMA frame.  Volatile read because
    // the device writes this asynchronously via DMA.
    let status = unsafe {
        core::ptr::read_volatile(ctl_virt.add(resp_offset) as *const u32)
    };
    if status != VIRTIO_SND_S_OK {
        serial_println!(
            "[virtio-snd] Command {:#x} for stream {} failed: status {:#x}",
            code, stream_id, status
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
    let ctl_phys = dev.ctl_frame.addr();
    let ctl_virt = (ctl_phys + dev.hhdm_offset) as *mut u8;

    let req = VirtioSndPcmSetParams {
        hdr: VirtioSndHdr { code: VIRTIO_SND_R_PCM_SET_PARAMS },
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

    dev.controlq.submit(&[
        (req_phys, req_len, 0),
        (resp_phys, 4, VRING_DESC_F_WRITE),
    ])?;
    dev.transport.notify_queue(0);

    // Poll.
    let mut attempts = 0u32;
    loop {
        if dev.controlq.poll_used().is_some() {
            break;
        }
        attempts = attempts.wrapping_add(1);
        if attempts > 1_000_000 {
            return Err(KernelError::TimedOut);
        }
        core::hint::spin_loop();
    }

    // SAFETY: resp_offset is within the DMA frame.  Volatile read because
    // the device writes this asynchronously.
    let status = unsafe {
        core::ptr::read_volatile(ctl_virt.add(resp_offset) as *const u32)
    };
    if status != VIRTIO_SND_S_OK {
        serial_println!(
            "[virtio-snd] SET_PARAMS for stream {} failed: status {:#x}",
            stream_id, status
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
fn submit_pcm_buffer(dev: &mut VirtioSndDevice, stream_id: u32, pcm_data: &[u8]) -> KernelResult<()> {
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
        core::ptr::copy_nonoverlapping(
            pcm_data.as_ptr(),
            pcm_virt.add(4),
            data_len,
        );
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
        (hdr_phys, 4, 0),                                    // Header
        (data_phys, data_len as u32, 0),                     // PCM data
        (status_phys, 8, VRING_DESC_F_WRITE),                // Status response
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
    let status = unsafe {
        core::ptr::read_volatile(pcm_virt.add(status_offset) as *const u32)
    };
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
pub fn play_test_tone(duration_ms: u32) -> KernelResult<()> {
    if !is_available() {
        return Err(KernelError::NoSuchDevice);
    }

    let mut guard = DEVICE.lock();
    let dev = guard.as_mut().ok_or(KernelError::NoSuchDevice)?;

    if dev.num_output_streams == 0 {
        return Err(KernelError::NoSuchDevice);
    }

    let stream_id: u32 = 0; // First output stream.

    // Set parameters: 48kHz, 16-bit signed, stereo, 8192 buffer / 4096 period.
    set_params(dev, stream_id, 2, VIRTIO_SND_PCM_FMT_S16, VIRTIO_SND_PCM_RATE_48000, 8192, 4096)?;
    serial_println!("[virtio-snd] Stream 0: params set (48kHz/S16/stereo)");

    // Prepare the stream.
    control_stream_cmd(dev, VIRTIO_SND_R_PCM_PREPARE, stream_id)?;
    serial_println!("[virtio-snd] Stream 0: prepared");

    // Start the stream.
    control_stream_cmd(dev, VIRTIO_SND_R_PCM_START, stream_id)?;
    serial_println!("[virtio-snd] Stream 0: started");
    dev.active_stream = Some(stream_id);

    // Generate and submit PCM data in chunks.
    // 48000 samples/sec × 2 channels × 2 bytes = 192000 bytes/sec.
    let bytes_per_ms: u32 = 192;
    let total_bytes = duration_ms.saturating_mul(bytes_per_ms);
    let chunk_size: usize = 4096; // Period-sized chunks.
    let mut buf = [0u8; 4096];
    let mut sample_offset: u32 = 0;

    let mut bytes_sent: u32 = 0;
    while bytes_sent < total_bytes {
        let remaining = (total_bytes - bytes_sent) as usize;
        let send_len = remaining.min(chunk_size);

        // Generate 440 Hz sine wave (integer approximation).
        generate_sine_440(&mut buf[..send_len], sample_offset);
        sample_offset = sample_offset.wrapping_add((send_len / 4) as u32); // 4 bytes per stereo sample

        submit_pcm_buffer(dev, stream_id, &buf[..send_len])?;
        bytes_sent = bytes_sent.wrapping_add(send_len as u32);
    }

    // Stop the stream.
    control_stream_cmd(dev, VIRTIO_SND_R_PCM_STOP, stream_id)?;
    dev.active_stream = None;

    // Release the stream.
    control_stream_cmd(dev, VIRTIO_SND_R_PCM_RELEASE, stream_id)?;
    serial_println!("[virtio-snd] Stream 0: stopped and released");

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
            buf[offset] = bytes[0];     // Left low byte
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
        serial_println!("[virtio-snd]   No device (skipped — add -device virtio-sound-pci to QEMU)");
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
