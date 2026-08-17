//! Virtio GPU device driver (virtio device type 16).
//!
//! Implements 2D framebuffer operations via QEMU's `virtio-gpu-pci` device.
//! Uses the **modern** virtio PCI transport (MMIO BARs + PCI capabilities)
//! because virtio-gpu only supports modern mode (no legacy I/O port access).
//!
//! Two virtqueues:
//! - Queue 0 (controlq): all 2D commands (create resource, attach backing,
//!   set scanout, transfer, flush)
//! - Queue 1 (cursorq): hardware cursor updates
//!
//! ## Protocol Overview
//!
//! 1. Reset device, negotiate features, set up queues.
//! 2. `GET_DISPLAY_INFO` → learn display resolution and scanout count.
//! 3. `RESOURCE_CREATE_2D` → create a host-side 2D resource.
//! 4. `RESOURCE_ATTACH_BACKING` → map guest physical memory to the resource.
//! 5. `SET_SCANOUT` → wire the resource to a display output.
//! 6. For each frame update:
//!    a. Write pixels to guest memory (the attached backing).
//!    b. `TRANSFER_TO_HOST_2D` → push dirty region to host resource.
//!    c. `RESOURCE_FLUSH` → signal host to display the resource.
//!
//! ## Modern Virtio PCI Transport
//!
//! The modern transport uses PCI vendor-specific capabilities (ID 0x09) to
//! locate configuration regions in MMIO BARs:
//! - cfg_type 1: Common configuration (device status, features, queue config)
//! - cfg_type 2: Notifications (queue kick doorbell)
//! - cfg_type 3: ISR status
//! - cfg_type 4: Device-specific configuration
//!
//! ## QEMU Usage
//!
//! ```text
//! -device virtio-gpu-pci
//! ```
//!
//! ## References
//!
//! - Virtio 1.1+ spec, Section 5.7 "GPU Device"
//! - Virtio 1.1+ spec, Section 4.1 "PCI Transport"
//! - QEMU hw/display/virtio-gpu.c

use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use crate::error::{KernelError, KernelResult};
use crate::mm::frame::{self, FRAME_SIZE, PhysFrame};
use crate::pci::{self, PciDevice};
use crate::serial_println;
use crate::virtio::modern::{ModernTransport, STATUS_DRIVER_OK, VIRTIO_VENDOR};
use crate::virtio::queue::{VRING_DESC_F_WRITE, Virtqueue};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Serial-log prefix, passed to the shared transport so its diagnostics name
/// this driver rather than the module they are emitted from.
const LOG_TAG: &str = "[virtio-gpu]";

/// Virtio-GPU transitional PCI device ID (type 16 → 0x1040 + 16 = 0x1050).
const VIRTIO_GPU_DEVICE_ID: u16 = 0x1050;

/// Maximum number of scanouts (displays) the spec allows.
const VIRTIO_GPU_MAX_SCANOUTS: usize = 16;

// ---------------------------------------------------------------------------
// GPU command types (virtio 1.1 §5.7.6.7)
// ---------------------------------------------------------------------------

const VIRTIO_GPU_CMD_GET_DISPLAY_INFO: u32 = 0x0100;
const VIRTIO_GPU_CMD_RESOURCE_CREATE_2D: u32 = 0x0101;
#[allow(dead_code)]
const VIRTIO_GPU_CMD_RESOURCE_UNREF: u32 = 0x0102;
const VIRTIO_GPU_CMD_SET_SCANOUT: u32 = 0x0103;
const VIRTIO_GPU_CMD_RESOURCE_FLUSH: u32 = 0x0104;
const VIRTIO_GPU_CMD_TRANSFER_TO_HOST_2D: u32 = 0x0105;
const VIRTIO_GPU_CMD_RESOURCE_ATTACH_BACKING: u32 = 0x0106;

// GPU response types.
const VIRTIO_GPU_RESP_OK_NODATA: u32 = 0x1100;
const VIRTIO_GPU_RESP_OK_DISPLAY_INFO: u32 = 0x1101;

// GPU pixel formats.
const VIRTIO_GPU_FORMAT_B8G8R8A8_UNORM: u32 = 1;

// ---------------------------------------------------------------------------
// GPU control message structures (repr(C) for DMA)
// ---------------------------------------------------------------------------

/// Common control header (24 bytes).
#[repr(C)]
#[derive(Clone, Copy)]
struct VirtioGpuCtrlHdr {
    hdr_type: u32,
    flags: u32,
    fence_id: u64,
    ctx_id: u32,
    _padding: u32,
}

impl VirtioGpuCtrlHdr {
    const fn new(cmd_type: u32) -> Self {
        Self {
            hdr_type: cmd_type,
            flags: 0,
            fence_id: 0,
            ctx_id: 0,
            _padding: 0,
        }
    }
}

/// A rectangle.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct VirtioGpuRect {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

/// Display info for one scanout.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct VirtioGpuDisplayOne {
    r: VirtioGpuRect,
    enabled: u32,
    flags: u32,
}

/// Full GET_DISPLAY_INFO response.
#[repr(C)]
#[derive(Clone, Copy)]
struct VirtioGpuRespDisplayInfo {
    hdr: VirtioGpuCtrlHdr,
    pmodes: [VirtioGpuDisplayOne; VIRTIO_GPU_MAX_SCANOUTS],
}

/// RESOURCE_CREATE_2D request.
#[repr(C)]
#[derive(Clone, Copy)]
struct VirtioGpuResourceCreate2d {
    hdr: VirtioGpuCtrlHdr,
    resource_id: u32,
    format: u32,
    width: u32,
    height: u32,
}

/// RESOURCE_ATTACH_BACKING request header.
#[repr(C)]
#[derive(Clone, Copy)]
struct VirtioGpuResourceAttachBacking {
    hdr: VirtioGpuCtrlHdr,
    resource_id: u32,
    nr_entries: u32,
}

/// A single memory entry for attach_backing.
#[repr(C)]
#[derive(Clone, Copy)]
struct VirtioGpuMemEntry {
    addr: u64,
    length: u32,
    _padding: u32,
}

/// SET_SCANOUT request.
#[repr(C)]
#[derive(Clone, Copy)]
struct VirtioGpuSetScanout {
    hdr: VirtioGpuCtrlHdr,
    r: VirtioGpuRect,
    scanout_id: u32,
    resource_id: u32,
}

/// TRANSFER_TO_HOST_2D request.
#[repr(C)]
#[derive(Clone, Copy)]
struct VirtioGpuTransferToHost2d {
    hdr: VirtioGpuCtrlHdr,
    r: VirtioGpuRect,
    offset: u64,
    resource_id: u32,
    _padding: u32,
}

/// RESOURCE_FLUSH request.
#[repr(C)]
#[derive(Clone, Copy)]
struct VirtioGpuResourceFlush {
    hdr: VirtioGpuCtrlHdr,
    r: VirtioGpuRect,
    resource_id: u32,
    _padding: u32,
}

// ---------------------------------------------------------------------------
// Device state
// ---------------------------------------------------------------------------

/// Virtio GPU device state.
struct VirtioGpuDevice {
    /// Modern PCI transport.
    transport: ModernTransport,
    /// Control virtqueue (queue 0).
    controlq: Virtqueue,
    /// Cursor virtqueue (queue 1).
    #[allow(dead_code)]
    cursorq: Virtqueue,
    /// HHDM offset for phys→virt conversion.
    hhdm_offset: u64,
    /// DMA frame for control request/response messages.
    ctl_frame: PhysFrame,
    /// Physical frames backing the framebuffer resource.
    fb_frames: alloc::vec::Vec<PhysFrame>,
    /// Width of the active display (pixels).
    width: u32,
    /// Height of the active display (pixels).
    height: u32,
    /// Resource ID for the primary framebuffer (0 = none).
    resource_id: u32,
    /// Next resource ID to allocate.
    next_resource_id: u32,
}

// SAFETY: VirtioGpuDevice contains raw pointers (inside Virtqueue and transport)
// that point to DMA/MMIO memory accessible from any CPU.  All access is
// serialized by the DEVICE Mutex, so sending between threads is safe.
unsafe impl Send for VirtioGpuDevice {}

/// Global device instance.
static DEVICE: spin::Mutex<Option<VirtioGpuDevice>> = spin::Mutex::new(None);

/// Whether the device is initialized and a framebuffer is active.
static INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Display width (cached for fast lock-free query).
static DISPLAY_WIDTH: AtomicU32 = AtomicU32::new(0);

/// Display height (cached for fast lock-free query).
static DISPLAY_HEIGHT: AtomicU32 = AtomicU32::new(0);

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Probe PCI for a virtio-gpu device and initialize it.
///
/// Uses the modern PCI transport (MMIO BARs via PCI capabilities).
/// Returns Ok(()) on success, or an error if no device or init fails.
#[allow(clippy::too_many_lines)]
pub fn init(hhdm_offset: u64) -> KernelResult<()> {
    serial_println!("[virtio-gpu] Probing for virtio-gpu device...");

    let dev = find_device()?;
    serial_println!(
        "[virtio-gpu] Found at {:02x}:{:02x}.{} (ID {:04x}:{:04x})",
        dev.address.bus,
        dev.address.device,
        dev.address.function,
        dev.vendor_id,
        dev.device_id
    );

    // Enable bus mastering and memory space.
    pci::enable_bus_master(dev.address);

    // Parse PCI capabilities to locate virtio config regions.
    let transport = ModernTransport::probe(LOG_TAG, &dev, hhdm_offset)?;

    // --- Device initialization sequence (modern transport §3.1) ---

    // Steps 1-5 (reset, ACKNOWLEDGE, DRIVER, feature negotiation, FEATURES_OK).
    // We want no optional GPU features -- 2D only, no VIRGL, no EDID -- so the
    // requested page-0 mask is empty; `negotiate` still accepts
    // VIRTIO_F_VERSION_1 on our behalf, which the previous open-coded sequence
    // here did not.  QEMU's virtio-gpu tolerated that omission; a stricter
    // device would not have.
    transport.negotiate(0)?;

    // 6. Read device config.
    let _events_read = transport.read_device_config32(0);
    let num_scanouts = transport.read_device_config32(8);
    serial_println!("[virtio-gpu] Config: num_scanouts={}", num_scanouts);

    // 7. Set up virtqueues.
    let num_queues = transport.num_queues();
    serial_println!("[virtio-gpu] {} queues available", num_queues);

    if num_queues < 2 {
        serial_println!("[virtio-gpu] Need at least 2 queues (controlq + cursorq)");
        transport.reset();
        return Err(KernelError::NoSuchDevice);
    }

    // Setup controlq (queue 0).
    let controlq = transport.setup_queue(0, hhdm_offset)?;
    // Setup cursorq (queue 1).
    let cursorq = transport.setup_queue(1, hhdm_offset)?;

    // 8. Set DRIVER_OK — device is live.
    transport.add_status(STATUS_DRIVER_OK);
    serial_println!(
        "[virtio-gpu] Device status: DRIVER_OK ({:#x})",
        transport.status()
    );

    // Allocate DMA frame for control messages.
    let ctl_frame = frame::alloc_frame()?;
    // SAFETY: We just allocated this frame; the HHDM maps it as writable
    // kernel memory.  Zeroing the entire frame is within bounds.
    unsafe {
        let ctl_virt = (ctl_frame.addr() + hhdm_offset) as *mut u8;
        core::ptr::write_bytes(ctl_virt, 0, FRAME_SIZE);
    }

    let mut device = VirtioGpuDevice {
        transport,
        controlq,
        cursorq,
        hhdm_offset,
        ctl_frame,
        fb_frames: alloc::vec::Vec::new(),
        width: 0,
        height: 0,
        resource_id: 0,
        next_resource_id: 1,
    };

    // Query display info.
    let (width, height) = get_display_info(&mut device)?;
    serial_println!("[virtio-gpu] Display 0: {}x{}", width, height);
    device.width = width;
    device.height = height;

    // Create a 2D resource.
    let resource_id = create_resource_2d(&mut device, width, height)?;
    device.resource_id = resource_id;
    serial_println!(
        "[virtio-gpu] Created resource {} ({}x{})",
        resource_id,
        width,
        height
    );

    // Allocate framebuffer backing memory.
    let fb_bytes = (width as usize)
        .checked_mul(height as usize)
        .and_then(|v| v.checked_mul(4))
        .ok_or(KernelError::InvalidArgument)?;
    let frames_needed = fb_bytes.div_ceil(FRAME_SIZE);

    let mut fb_frames = alloc::vec::Vec::new();
    for i in 0..frames_needed {
        let f = frame::alloc_frame().inspect_err(|&e| {
            serial_println!("[virtio-gpu] Failed to alloc FB frame {}: {:?}", i, e);
            // Free already allocated.
            for frame in &fb_frames {
                // SAFETY: These frames were allocated above and are not
                // aliased.  Failure to free is logged but not fatal.
                unsafe {
                    let _ = frame::free_frame(*frame);
                }
            }
        })?;
        // Zero the frame.
        // SAFETY: Just allocated; HHDM maps it writable.  Zeroing one
        // FRAME_SIZE region stays within bounds of the allocated frame.
        unsafe {
            let virt = (f.addr() + hhdm_offset) as *mut u8;
            core::ptr::write_bytes(virt, 0, FRAME_SIZE);
        }
        fb_frames.push(f);
    }
    serial_println!(
        "[virtio-gpu] Allocated {} frames ({} KiB) for {}x{} framebuffer",
        frames_needed,
        frames_needed * FRAME_SIZE / 1024,
        width,
        height
    );
    device.fb_frames = fb_frames;

    // Attach backing memory.
    attach_backing(&mut device, resource_id)?;
    serial_println!("[virtio-gpu] Attached backing memory");

    // Set scanout.
    set_scanout(&mut device, 0, resource_id, width, height)?;
    serial_println!("[virtio-gpu] Scanout 0 configured");

    // Initial transfer + flush (show black screen).
    transfer_to_host_2d(&mut device, resource_id, 0, 0, width, height)?;
    resource_flush(&mut device, resource_id, 0, 0, width, height)?;

    DISPLAY_WIDTH.store(width, Ordering::Release);
    DISPLAY_HEIGHT.store(height, Ordering::Release);

    *DEVICE.lock() = Some(device);
    INITIALIZED.store(true, Ordering::Release);

    serial_println!(
        "[virtio-gpu] Initialization complete ({}x{} active)",
        width,
        height
    );
    Ok(())
}

/// Find the virtio-gpu PCI device.
fn find_device() -> KernelResult<PciDevice> {
    if let Some(dev) = pci::find_device(VIRTIO_VENDOR, VIRTIO_GPU_DEVICE_ID) {
        return Ok(dev);
    }
    // Class 0x03 (Display), subclass 0x00 or 0x80.
    for &subclass in &[0x00u8, 0x80] {
        let devices = pci::find_devices_by_class(0x03, subclass);
        for dev in devices {
            if dev.vendor_id == VIRTIO_VENDOR {
                return Ok(dev);
            }
        }
    }
    serial_println!("[virtio-gpu] No virtio-gpu device found");
    Err(KernelError::NoSuchDevice)
}

// ---------------------------------------------------------------------------
// Control queue commands
// ---------------------------------------------------------------------------

/// Send a control command and wait for the response.
#[allow(clippy::arithmetic_side_effects)]
fn send_ctrl_cmd(
    dev: &mut VirtioGpuDevice,
    req_data: &[u8],
    resp_offset: usize,
    resp_size: usize,
) -> KernelResult<u32> {
    let ctl_phys = dev.ctl_frame.addr();
    let ctl_virt = (ctl_phys + dev.hhdm_offset) as *mut u8;

    // Write request at offset 0.
    // SAFETY: ctl_virt points to the DMA control frame (FRAME_SIZE bytes,
    // HHDM-mapped).  req_data.len() < FRAME_SIZE (all GPU commands are
    // small), so this stays in bounds.  No aliasing — we own ctl_frame.
    unsafe {
        core::ptr::copy_nonoverlapping(req_data.as_ptr(), ctl_virt, req_data.len());
    }
    // Zero response area.
    // SAFETY: resp_offset + resp_size <= FRAME_SIZE (checked by callers
    // that compute offsets from known struct sizes).
    unsafe {
        core::ptr::write_bytes(ctl_virt.add(resp_offset), 0, resp_size);
    }

    let req_phys = ctl_phys;
    let resp_phys = ctl_phys + resp_offset as u64;

    dev.controlq.submit(&[
        (req_phys, req_data.len() as u32, 0),
        (resp_phys, resp_size as u32, VRING_DESC_F_WRITE),
    ])?;
    dev.transport.notify_queue(0);

    // Poll for completion.
    let mut attempts = 0u32;
    let head = loop {
        if let Some((head, _len)) = dev.controlq.poll_used() {
            break head;
        }
        attempts = attempts.wrapping_add(1);
        if attempts > 5_000_000 {
            // Timing out leaves this chain owned by the device, so its
            // descriptors are deliberately *not* freed here — the device may
            // still write to them.  The queue is down two descriptors until
            // something resets it, which is the right trade against handing a
            // live DMA target back to the allocator.
            return Err(KernelError::TimedOut);
        }
        core::hint::spin_loop();
    };

    // Return the chain to the free list.  The response lives in the DMA
    // control frame, not in the descriptors, so it is still readable below.
    //
    // Without this, every control command leaked its two descriptors.  That
    // was survivable for the handful issued at init, but flush_rect and
    // flush_full each run two commands per screen update, so the control queue
    // lost four descriptors per frame and submit() started returning WouldBlock
    // after roughly sixteen flushes — the display would come up and then
    // freeze.
    dev.controlq.free_chain(head);

    // Read response type.
    // SAFETY: The device has written the response at resp_offset within
    // the DMA frame.  Volatile read because the device writes asynchronously.
    let resp_type = unsafe { core::ptr::read_volatile(ctl_virt.add(resp_offset) as *const u32) };
    Ok(resp_type)
}

/// Query display information.
fn get_display_info(dev: &mut VirtioGpuDevice) -> KernelResult<(u32, u32)> {
    let hdr = VirtioGpuCtrlHdr::new(VIRTIO_GPU_CMD_GET_DISPLAY_INFO);
    // SAFETY: VirtioGpuCtrlHdr is #[repr(C)], so its byte layout is
    // well-defined and matches the virtio wire format.  The slice covers
    // exactly size_of::<VirtioGpuCtrlHdr>() bytes from a valid local.
    let req_bytes = unsafe {
        core::slice::from_raw_parts(
            &hdr as *const _ as *const u8,
            core::mem::size_of::<VirtioGpuCtrlHdr>(),
        )
    };

    let resp_offset = 256;
    let resp_size = core::mem::size_of::<VirtioGpuRespDisplayInfo>();
    let resp_type = send_ctrl_cmd(dev, req_bytes, resp_offset, resp_size)?;

    if resp_type != VIRTIO_GPU_RESP_OK_DISPLAY_INFO {
        serial_println!("[virtio-gpu] GET_DISPLAY_INFO: resp={:#x}", resp_type);
        return Err(KernelError::IoError);
    }

    let ctl_virt = (dev.ctl_frame.addr() + dev.hhdm_offset) as *mut u8;
    // SAFETY: The device wrote a VirtioGpuRespDisplayInfo at resp_offset
    // within the DMA frame.  The struct is #[repr(C)] and the response
    // type was validated above.  Reading it fully is within frame bounds
    // (resp_offset + size_of::<VirtioGpuRespDisplayInfo>() < FRAME_SIZE).
    let resp =
        unsafe { core::ptr::read(ctl_virt.add(resp_offset) as *const VirtioGpuRespDisplayInfo) };

    for (i, pmode) in resp.pmodes.iter().enumerate() {
        if pmode.enabled != 0 && pmode.r.width > 0 && pmode.r.height > 0 {
            serial_println!(
                "[virtio-gpu]   Scanout {}: {}x{} enabled",
                i,
                pmode.r.width,
                pmode.r.height
            );
            return Ok((pmode.r.width, pmode.r.height));
        }
    }

    serial_println!("[virtio-gpu] No enabled scanout, using 1280x800");
    Ok((1280, 800))
}

/// Create a 2D resource.
fn create_resource_2d(dev: &mut VirtioGpuDevice, width: u32, height: u32) -> KernelResult<u32> {
    let resource_id = dev.next_resource_id;
    dev.next_resource_id = dev.next_resource_id.wrapping_add(1);

    let req = VirtioGpuResourceCreate2d {
        hdr: VirtioGpuCtrlHdr::new(VIRTIO_GPU_CMD_RESOURCE_CREATE_2D),
        resource_id,
        format: VIRTIO_GPU_FORMAT_B8G8R8A8_UNORM,
        width,
        height,
    };
    // SAFETY: VirtioGpuResourceCreate2d is #[repr(C)], so its byte layout
    // matches the virtio wire format.  The slice covers exactly its size.
    let req_bytes = unsafe {
        core::slice::from_raw_parts(
            &req as *const _ as *const u8,
            core::mem::size_of::<VirtioGpuResourceCreate2d>(),
        )
    };

    let resp_type = send_ctrl_cmd(
        dev,
        req_bytes,
        512,
        core::mem::size_of::<VirtioGpuCtrlHdr>(),
    )?;

    if resp_type != VIRTIO_GPU_RESP_OK_NODATA {
        serial_println!("[virtio-gpu] RESOURCE_CREATE_2D: resp={:#x}", resp_type);
        return Err(KernelError::IoError);
    }
    Ok(resource_id)
}

/// Attach guest memory backing to a resource.
#[allow(clippy::arithmetic_side_effects)]
fn attach_backing(dev: &mut VirtioGpuDevice, resource_id: u32) -> KernelResult<()> {
    let num_frames = dev.fb_frames.len();
    let ctl_phys = dev.ctl_frame.addr();
    let ctl_virt = (ctl_phys + dev.hhdm_offset) as *mut u8;

    let header_size = core::mem::size_of::<VirtioGpuResourceAttachBacking>();
    let entry_size = core::mem::size_of::<VirtioGpuMemEntry>();
    let entries_size = num_frames * entry_size;
    let total_req_size = header_size + entries_size;
    let resp_offset = align_up(total_req_size, 64);
    let resp_size = core::mem::size_of::<VirtioGpuCtrlHdr>();

    if resp_offset + resp_size > FRAME_SIZE {
        serial_println!("[virtio-gpu] attach_backing: too many frames for DMA frame");
        return Err(KernelError::InvalidArgument);
    }

    // Write header.
    let req_hdr = VirtioGpuResourceAttachBacking {
        hdr: VirtioGpuCtrlHdr::new(VIRTIO_GPU_CMD_RESOURCE_ATTACH_BACKING),
        resource_id,
        nr_entries: num_frames as u32,
    };
    // SAFETY: ctl_virt points to the start of the DMA control frame.
    // VirtioGpuResourceAttachBacking is #[repr(C)] and fits within the
    // frame (header_size < FRAME_SIZE).
    unsafe {
        core::ptr::write(ctl_virt as *mut VirtioGpuResourceAttachBacking, req_hdr);
    }

    // Write memory entries.
    for (i, frame) in dev.fb_frames.iter().enumerate() {
        let entry = VirtioGpuMemEntry {
            addr: frame.addr(),
            length: FRAME_SIZE as u32,
            _padding: 0,
        };
        // SAFETY: header_size + i * entry_size < total_req_size, which was
        // checked against FRAME_SIZE above.  VirtioGpuMemEntry is #[repr(C)].
        unsafe {
            core::ptr::write(
                ctl_virt.add(header_size + i * entry_size) as *mut VirtioGpuMemEntry,
                entry,
            );
        }
    }

    // Zero response.
    // SAFETY: resp_offset + resp_size <= FRAME_SIZE (checked above).
    unsafe {
        core::ptr::write_bytes(ctl_virt.add(resp_offset), 0, resp_size);
    }

    // Submit.
    let resp_phys = ctl_phys + resp_offset as u64;
    dev.controlq.submit(&[
        (ctl_phys, total_req_size as u32, 0),
        (resp_phys, resp_size as u32, VRING_DESC_F_WRITE),
    ])?;
    dev.transport.notify_queue(0);

    // Poll.
    let mut attempts = 0u32;
    let head = loop {
        if let Some((head, _len)) = dev.controlq.poll_used() {
            break head;
        }
        attempts = attempts.wrapping_add(1);
        if attempts > 5_000_000 {
            // As in send_ctrl_cmd: a timed-out chain is still the device's, so
            // it is not returned to the free list.
            return Err(KernelError::TimedOut);
        }
        core::hint::spin_loop();
    };

    // Return the chain to the free list; the response is in the DMA frame.
    dev.controlq.free_chain(head);

    // SAFETY: Device wrote the response at resp_offset within the DMA
    // frame.  Volatile read because the device writes asynchronously.
    let resp_type = unsafe { core::ptr::read_volatile(ctl_virt.add(resp_offset) as *const u32) };
    if resp_type != VIRTIO_GPU_RESP_OK_NODATA {
        serial_println!("[virtio-gpu] ATTACH_BACKING: resp={:#x}", resp_type);
        return Err(KernelError::IoError);
    }
    Ok(())
}

/// Set scanout.
fn set_scanout(
    dev: &mut VirtioGpuDevice,
    scanout_id: u32,
    resource_id: u32,
    width: u32,
    height: u32,
) -> KernelResult<()> {
    let req = VirtioGpuSetScanout {
        hdr: VirtioGpuCtrlHdr::new(VIRTIO_GPU_CMD_SET_SCANOUT),
        r: VirtioGpuRect {
            x: 0,
            y: 0,
            width,
            height,
        },
        scanout_id,
        resource_id,
    };
    // SAFETY: VirtioGpuSetScanout is #[repr(C)]; the slice covers
    // exactly its size from a valid stack local.
    let req_bytes = unsafe {
        core::slice::from_raw_parts(
            &req as *const _ as *const u8,
            core::mem::size_of::<VirtioGpuSetScanout>(),
        )
    };
    let resp_type = send_ctrl_cmd(
        dev,
        req_bytes,
        512,
        core::mem::size_of::<VirtioGpuCtrlHdr>(),
    )?;
    if resp_type != VIRTIO_GPU_RESP_OK_NODATA {
        serial_println!("[virtio-gpu] SET_SCANOUT: resp={:#x}", resp_type);
        return Err(KernelError::IoError);
    }
    Ok(())
}

/// Transfer a region from guest memory to the host resource.
fn transfer_to_host_2d(
    dev: &mut VirtioGpuDevice,
    resource_id: u32,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
) -> KernelResult<()> {
    let req = VirtioGpuTransferToHost2d {
        hdr: VirtioGpuCtrlHdr::new(VIRTIO_GPU_CMD_TRANSFER_TO_HOST_2D),
        r: VirtioGpuRect {
            x,
            y,
            width,
            height,
        },
        offset: 0,
        resource_id,
        _padding: 0,
    };
    // SAFETY: VirtioGpuTransferToHost2d is #[repr(C)]; the slice covers
    // exactly its size from a valid stack local.
    let req_bytes = unsafe {
        core::slice::from_raw_parts(
            &req as *const _ as *const u8,
            core::mem::size_of::<VirtioGpuTransferToHost2d>(),
        )
    };
    let resp_type = send_ctrl_cmd(
        dev,
        req_bytes,
        512,
        core::mem::size_of::<VirtioGpuCtrlHdr>(),
    )?;
    if resp_type != VIRTIO_GPU_RESP_OK_NODATA {
        serial_println!("[virtio-gpu] TRANSFER_TO_HOST_2D: resp={:#x}", resp_type);
        return Err(KernelError::IoError);
    }
    Ok(())
}

/// Flush a region.
fn resource_flush(
    dev: &mut VirtioGpuDevice,
    resource_id: u32,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
) -> KernelResult<()> {
    let req = VirtioGpuResourceFlush {
        hdr: VirtioGpuCtrlHdr::new(VIRTIO_GPU_CMD_RESOURCE_FLUSH),
        r: VirtioGpuRect {
            x,
            y,
            width,
            height,
        },
        resource_id,
        _padding: 0,
    };
    // SAFETY: VirtioGpuResourceFlush is #[repr(C)]; the slice covers
    // exactly its size from a valid stack local.
    let req_bytes = unsafe {
        core::slice::from_raw_parts(
            &req as *const _ as *const u8,
            core::mem::size_of::<VirtioGpuResourceFlush>(),
        )
    };
    let resp_type = send_ctrl_cmd(
        dev,
        req_bytes,
        512,
        core::mem::size_of::<VirtioGpuCtrlHdr>(),
    )?;
    if resp_type != VIRTIO_GPU_RESP_OK_NODATA {
        serial_println!("[virtio-gpu] RESOURCE_FLUSH: resp={:#x}", resp_type);
        return Err(KernelError::IoError);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Is the virtio-gpu device active?
pub fn is_available() -> bool {
    INITIALIZED.load(Ordering::Acquire)
}

/// Get display dimensions: (width, height).
pub fn dimensions() -> (u32, u32) {
    (
        DISPLAY_WIDTH.load(Ordering::Acquire),
        DISPLAY_HEIGHT.load(Ordering::Acquire),
    )
}

/// Get the virtual address of the first framebuffer byte.
pub fn framebuffer_addr() -> Option<u64> {
    let guard = DEVICE.lock();
    let dev = guard.as_ref()?;
    let first = dev.fb_frames.first()?;
    Some(first.addr() + dev.hhdm_offset)
}

/// Write a pixel. Does NOT auto-flush.
#[allow(clippy::arithmetic_side_effects)]
pub fn set_pixel(x: u32, y: u32, color: u32) {
    let guard = DEVICE.lock();
    let dev = match guard.as_ref() {
        Some(d) => d,
        None => return,
    };
    if x >= dev.width || y >= dev.height {
        return;
    }

    let offset = ((y as usize) * (dev.width as usize) + (x as usize)) * 4;
    let frame_idx = offset / FRAME_SIZE;
    let frame_offset = offset % FRAME_SIZE;

    if let Some(frame) = dev.fb_frames.get(frame_idx) {
        let virt = (frame.addr() + dev.hhdm_offset) as *mut u8;
        // SAFETY: frame_offset < FRAME_SIZE (since offset < total_bytes =
        // width*height*4, and frame_idx/frame_offset partition it into
        // FRAME_SIZE chunks).  We use volatile because the framebuffer
        // memory is accessed by the GPU device.
        unsafe {
            core::ptr::write_volatile(virt.add(frame_offset) as *mut u32, color);
        }
    }
}

/// Flush a rectangular region to host.
pub fn flush_rect(x: u32, y: u32, width: u32, height: u32) -> KernelResult<()> {
    let mut guard = DEVICE.lock();
    let dev = guard.as_mut().ok_or(KernelError::NoSuchDevice)?;
    let rid = dev.resource_id;
    if rid == 0 {
        return Err(KernelError::NoSuchDevice);
    }
    transfer_to_host_2d(dev, rid, x, y, width, height)?;
    resource_flush(dev, rid, x, y, width, height)
}

/// Flush the entire display.
pub fn flush_full() -> KernelResult<()> {
    let (w, h) = dimensions();
    if w == 0 || h == 0 {
        return Err(KernelError::NoSuchDevice);
    }
    flush_rect(0, 0, w, h)
}

/// Fill with a solid color and flush.
#[allow(clippy::arithmetic_side_effects)]
pub fn fill(color: u32) -> KernelResult<()> {
    let mut guard = DEVICE.lock();
    let dev = guard.as_mut().ok_or(KernelError::NoSuchDevice)?;
    let rid = dev.resource_id;
    let (width, height) = (dev.width, dev.height);
    if rid == 0 {
        return Err(KernelError::NoSuchDevice);
    }

    let total_pixels = (width as usize) * (height as usize);
    let total_bytes = total_pixels * 4;
    let mut filled = 0usize;

    for frame in &dev.fb_frames {
        let virt = (frame.addr() + dev.hhdm_offset) as *mut u32;
        let remaining = total_bytes.saturating_sub(filled);
        let pixels = remaining.min(FRAME_SIZE) / 4;
        // SAFETY: pixels * 4 <= FRAME_SIZE, so all writes stay within
        // the allocated frame.  Volatile because the GPU reads this memory.
        unsafe {
            for p in 0..pixels {
                core::ptr::write_volatile(virt.add(p), color);
            }
        }
        filled = filled.wrapping_add(pixels * 4);
    }

    transfer_to_host_2d(dev, rid, 0, 0, width, height)?;
    resource_flush(dev, rid, 0, 0, width, height)
}

/// Status string for diagnostics.
#[allow(dead_code)]
pub fn status_info() -> &'static str {
    if INITIALIZED.load(Ordering::Acquire) {
        "virtio-gpu: active"
    } else {
        "virtio-gpu: not present"
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test: verify detection and basic operations.
pub fn self_test() {
    serial_println!("[virtio-gpu] Running self-test...");

    let available = is_available();
    serial_println!("[virtio-gpu]   is_available: {}", available);

    if !available {
        serial_println!("[virtio-gpu] Self-test PASSED (no device)");
        return;
    }

    let (w, h) = dimensions();
    serial_println!("[virtio-gpu]   Dimensions: {}x{}", w, h);

    if let Some(addr) = framebuffer_addr() {
        serial_println!("[virtio-gpu]   FB addr: {:#x}", addr);

        // Write a pixel, read back.
        set_pixel(0, 0, 0xFF_FF0000);
        // SAFETY: addr is the HHDM-mapped framebuffer base returned by
        // framebuffer_addr().  Reading the first u32 is within the first
        // frame's bounds.
        let pixel = unsafe { core::ptr::read_volatile(addr as *const u32) };
        if pixel == 0xFF_FF0000 {
            serial_println!("[virtio-gpu]   Pixel write/read: OK");
        } else {
            serial_println!("[virtio-gpu]   Pixel write/read: FAIL ({:#x})", pixel);
        }
        set_pixel(0, 0, 0xFF_000000);
    }

    match flush_rect(0, 0, 1, 1) {
        Ok(()) => serial_println!("[virtio-gpu]   flush_rect: OK"),
        Err(e) => serial_println!("[virtio-gpu]   flush_rect: {:?}", e),
    }

    serial_println!("[virtio-gpu] Self-test PASSED");
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

#[allow(clippy::arithmetic_side_effects)]
const fn align_up(value: usize, align: usize) -> usize {
    (value + align - 1) & !(align - 1)
}

