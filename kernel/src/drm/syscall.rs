//! DRM syscall handlers (1000-1099 range).
//!
//! These handlers bridge userspace applications to the DRM subsystem,
//! enabling GPU buffer allocation, framebuffer management, and display
//! control from ring 3.
//!
//! ## Syscall ABI
//!
//! All DRM syscalls take a device handle in `arg0`.  The handle is
//! obtained from `SYS_DRM_OPEN` and validated on each call.
//!
//! Return values follow the kernel convention: positive = success,
//! negative = error code (from `KernelError::code()`).
//!
//! ## GEM MMAP
//!
//! `SYS_DRM_GEM_MMAP` maps a GEM buffer's (potentially non-contiguous)
//! physical frames into a contiguous virtual address range in the calling
//! process's address space.  The mapping uses the MMAP region
//! (0x60_0000_0000 .. 0x70_0000_0000) via the same bump allocator used
//! by `SYS_MMAP`.

use crate::error::{KernelError, KernelResult};
use crate::mm::frame::FRAME_SIZE;
use crate::mm::page_table::{self, PageFlags, VirtAddr};
use crate::proc::{pcb, thread};
use crate::sched;
use crate::serial_println;
use crate::syscall::dispatch::{SyscallArgs, SyscallResult};

use super::atomic::{AtomicState, ConnectorState, CrtcState, IRect, PlaneState, Rect};
use super::connector::ConnectorType;
use super::mode::{DrmMode, PixelFormat};
use super::DrmObjectId;

// ---------------------------------------------------------------------------
// Device handle validation
// ---------------------------------------------------------------------------

/// Validate a device handle (currently just a device index).
///
/// Returns the device index on success.
fn validate_device(handle: u64) -> KernelResult<usize> {
    let index = handle as usize;
    if index >= super::device_count() {
        return Err(KernelError::NotFound);
    }
    Ok(index)
}

// ---------------------------------------------------------------------------
// SYS_DRM_OPEN (1000)
// ---------------------------------------------------------------------------

/// Open a DRM device.
///
/// For now, device handles are simply device indices.  When per-process
/// DRM file descriptors are added, this will allocate a handle and track
/// the opener's PID for cleanup.
pub fn sys_drm_open(args: &SyscallArgs) -> SyscallResult {
    let index = args.arg0 as usize;
    if index >= super::device_count() {
        return SyscallResult::err(KernelError::NotFound);
    }

    // Verify the device exists and is functional.
    match super::with_device(index, |_dev| Ok(())) {
        Ok(()) => SyscallResult::ok(index as i64),
        Err(e) => SyscallResult::err(e),
    }
}

// ---------------------------------------------------------------------------
// SYS_DRM_CLOSE (1001)
// ---------------------------------------------------------------------------

/// Close a DRM device handle.
///
/// Currently a no-op since handles are just indices.  Will become
/// meaningful when per-process GEM handle namespaces are added.
pub fn sys_drm_close(args: &SyscallArgs) -> SyscallResult {
    match validate_device(args.arg0) {
        Ok(_) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

// ---------------------------------------------------------------------------
// SYS_DRM_DISPLAY_SIZE (1002)
// ---------------------------------------------------------------------------

/// Get display dimensions.
///
/// Returns `width | (height << 32)`.
pub fn sys_drm_display_size(args: &SyscallArgs) -> SyscallResult {
    let index = match validate_device(args.arg0) {
        Ok(i) => i,
        Err(e) => return SyscallResult::err(e),
    };

    match super::with_device(index, |dev| {
        let (w, h) = dev.display_size();
        #[allow(clippy::cast_lossless)]
        Ok((w as u64) | ((h as u64) << 32))
    }) {
        Ok(packed) => SyscallResult::ok(packed as i64),
        Err(e) => SyscallResult::err(e),
    }
}

// ---------------------------------------------------------------------------
// SYS_DRM_GEM_CREATE (1010)
// ---------------------------------------------------------------------------

/// Allocate a GEM buffer object.
pub fn sys_drm_gem_create(args: &SyscallArgs) -> SyscallResult {
    let index = match validate_device(args.arg0) {
        Ok(i) => i,
        Err(e) => return SyscallResult::err(e),
    };

    let width = args.arg1 as u32;
    let height = args.arg2 as u32;
    let format_raw = args.arg3 as u32;

    let format = match PixelFormat::from_raw(format_raw) {
        Some(f) => f,
        None => return SyscallResult::err(KernelError::InvalidArgument),
    };

    if width == 0 || height == 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    match super::with_device_mut(index, |dev| {
        let handle = dev.gem_create(width, height, format)?;
        Ok(handle as i64)
    }) {
        Ok(handle) => SyscallResult::ok(handle),
        Err(e) => SyscallResult::err(e),
    }
}

// ---------------------------------------------------------------------------
// SYS_DRM_GEM_DESTROY (1011)
// ---------------------------------------------------------------------------

/// Free a GEM buffer object.
pub fn sys_drm_gem_destroy(args: &SyscallArgs) -> SyscallResult {
    let index = match validate_device(args.arg0) {
        Ok(i) => i,
        Err(e) => return SyscallResult::err(e),
    };

    let gem_handle = args.arg1 as u32;

    match super::with_device_mut(index, |dev| {
        dev.gem_destroy(gem_handle)
    }) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

// ---------------------------------------------------------------------------
// SYS_DRM_GEM_MMAP (1012)
// ---------------------------------------------------------------------------

/// Map a GEM buffer into the calling process's virtual address space.
///
/// This is the zero-copy path: userspace gets a direct pointer to the
/// GEM backing memory.  The mapping is USER_ACCESSIBLE | WRITABLE |
/// NO_EXECUTE.
///
/// The backing frames may be non-contiguous physically, but the virtual
/// mapping is contiguous.
#[allow(clippy::arithmetic_side_effects)]
pub fn sys_drm_gem_mmap(args: &SyscallArgs) -> SyscallResult {
    let index = match validate_device(args.arg0) {
        Ok(i) => i,
        Err(e) => return SyscallResult::err(e),
    };

    let gem_handle = args.arg1 as u32;

    // Get the GEM object's physical frame addresses and total size.
    let (frame_addrs, _size) = match super::with_device(index, |dev| {
        let addrs = dev.gem_frame_addrs(gem_handle)?;
        // Size in bytes = number of frames * FRAME_SIZE (last frame may be partial).
        let sz = addrs.len() * FRAME_SIZE;
        Ok((addrs, sz))
    }) {
        Ok(v) => v,
        Err(e) => return SyscallResult::err(e),
    };

    if frame_addrs.is_empty() {
        return SyscallResult::err(KernelError::InternalError);
    }

    // Get the calling process's PML4.
    let task_id = sched::current_task_id();
    let pid = thread::owner_process(task_id).unwrap_or(0);
    let pml4_phys = match pcb::get_pml4(pid) {
        Some(pml4) if pml4 != 0 => pml4,
        _ => return SyscallResult::err(KernelError::NoSuchProcess),
    };

    let num_frames = frame_addrs.len();
    let frame_size_u64 = FRAME_SIZE as u64;
    let size_aligned = (num_frames as u64) * frame_size_u64;

    // Allocate a contiguous virtual address range.
    let base_vaddr = crate::syscall::handlers::mmap_alloc_vaddr(size_aligned);
    if base_vaddr == 0 {
        return SyscallResult::err(KernelError::OutOfMemory);
    }

    let page_flags = PageFlags::PRESENT
        | PageFlags::USER_ACCESSIBLE
        | PageFlags::WRITABLE
        | PageFlags::NO_EXECUTE;

    let hhdm = match page_table::hhdm() {
        Some(h) => h,
        None => return SyscallResult::err(KernelError::NotSupported),
    };

    // Map each GEM frame to a consecutive virtual page.
    for (i, &virt_addr) in frame_addrs.iter().enumerate() {
        // frame_addrs contains HHDM virtual addresses.  Convert back to
        // physical addresses for map_frame.
        let phys_addr = virt_addr.wrapping_sub(hhdm);

        let va = base_vaddr + (i as u64) * frame_size_u64;
        let phys = match crate::mm::frame::PhysFrame::from_addr(phys_addr) {
            Some(f) => f,
            None => {
                // Rollback: unmap frames 0..i.
                for j in 0..i {
                    let rv = base_vaddr + (j as u64) * frame_size_u64;
                    // SAFETY: We mapped these frames successfully above.
                    let _ = unsafe {
                        page_table::unmap_frame(pml4_phys, VirtAddr::new(rv))
                    };
                }
                return SyscallResult::err(KernelError::InvalidAddress);
            }
        };

        // SAFETY: pml4_phys is the calling process's valid page table.
        // phys is a GEM-owned frame (valid physical memory).
        // va is in the user mmap region.
        if let Err(e) = unsafe {
            page_table::map_frame(pml4_phys, VirtAddr::new(va), phys, page_flags)
        } {
            serial_println!(
                "[drm] GEM MMAP map failed at va={:#x}: {:?}",
                va, e
            );
            // Rollback: unmap frames 0..i.
            for j in 0..i {
                let rv = base_vaddr + (j as u64) * frame_size_u64;
                // SAFETY: rv was successfully mapped in a prior iteration; pml4 is valid.
                let _ = unsafe {
                    page_table::unmap_frame(pml4_phys, VirtAddr::new(rv))
                };
            }
            return SyscallResult::err(e);
        }
    }

    serial_println!(
        "[drm] GEM MMAP: handle={} → {:#x}..{:#x} ({} frames, pid={})",
        gem_handle,
        base_vaddr,
        base_vaddr + size_aligned,
        num_frames,
        pid,
    );

    SyscallResult::ok(base_vaddr as i64)
}

// ---------------------------------------------------------------------------
// SYS_DRM_FB_CREATE (1020)
// ---------------------------------------------------------------------------

/// Create a DRM framebuffer from a GEM handle.
pub fn sys_drm_fb_create(args: &SyscallArgs) -> SyscallResult {
    let index = match validate_device(args.arg0) {
        Ok(i) => i,
        Err(e) => return SyscallResult::err(e),
    };

    let gem_handle = args.arg1 as u32;
    // Unpack: arg2 = width | (height << 32), arg3 = pitch | (format << 32).
    let width = args.arg2 as u32;
    #[allow(clippy::cast_possible_truncation)]
    let height = (args.arg2 >> 32) as u32;
    let pitch = args.arg3 as u32;
    #[allow(clippy::cast_possible_truncation)]
    let format_raw = (args.arg3 >> 32) as u32;

    let format = match PixelFormat::from_raw(format_raw) {
        Some(f) => f,
        None => return SyscallResult::err(KernelError::InvalidArgument),
    };

    if width == 0 || height == 0 || pitch == 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    match super::with_device_mut(index, |dev| {
        let fb_id = dev.fb_create(gem_handle, width, height, pitch, format)?;
        Ok(fb_id.raw() as i64)
    }) {
        Ok(id) => SyscallResult::ok(id),
        Err(e) => SyscallResult::err(e),
    }
}

// ---------------------------------------------------------------------------
// SYS_DRM_FB_DESTROY (1021)
// ---------------------------------------------------------------------------

/// Destroy a DRM framebuffer.
pub fn sys_drm_fb_destroy(args: &SyscallArgs) -> SyscallResult {
    let index = match validate_device(args.arg0) {
        Ok(i) => i,
        Err(e) => return SyscallResult::err(e),
    };

    let fb_id = DrmObjectId::new(args.arg1 as u32);

    match super::with_device_mut(index, |dev| {
        dev.fb_destroy(fb_id)
    }) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

// ---------------------------------------------------------------------------
// SYS_DRM_PAGE_FLIP (1030)
// ---------------------------------------------------------------------------

/// Page flip: display a framebuffer on a CRTC.
pub fn sys_drm_page_flip(args: &SyscallArgs) -> SyscallResult {
    let index = match validate_device(args.arg0) {
        Ok(i) => i,
        Err(e) => return SyscallResult::err(e),
    };

    let crtc_id = DrmObjectId::new(args.arg1 as u32);
    let fb_id = DrmObjectId::new(args.arg2 as u32);

    match super::with_device_mut(index, |dev| {
        dev.page_flip(crtc_id, fb_id)
    }) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

// ---------------------------------------------------------------------------
// SYS_DRM_FLUSH_REGION (1031)
// ---------------------------------------------------------------------------

/// Flush a dirty sub-region to the display.
pub fn sys_drm_flush_region(args: &SyscallArgs) -> SyscallResult {
    let index = match validate_device(args.arg0) {
        Ok(i) => i,
        Err(e) => return SyscallResult::err(e),
    };

    let fb_id = DrmObjectId::new(args.arg1 as u32);
    // Unpack: arg2 = x | (y << 32), arg3 = w | (h << 32).
    let x = args.arg2 as u32;
    #[allow(clippy::cast_possible_truncation)]
    let y = (args.arg2 >> 32) as u32;
    let w = args.arg3 as u32;
    #[allow(clippy::cast_possible_truncation)]
    let h = (args.arg3 >> 32) as u32;

    match super::with_device_mut(index, |dev| {
        dev.flush_region(fb_id, x, y, w, h)
    }) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

// ---------------------------------------------------------------------------
// SYS_DRM_CONNECTOR_STATUS (1040)
// ---------------------------------------------------------------------------

/// Get connector status and info.
///
/// Returns packed: `status | (type << 8) | (mode_count << 16) | (id << 32)`.
#[allow(clippy::arithmetic_side_effects)]
pub fn sys_drm_connector_status(args: &SyscallArgs) -> SyscallResult {
    let index = match validate_device(args.arg0) {
        Ok(i) => i,
        Err(e) => return SyscallResult::err(e),
    };

    let conn_idx = args.arg1 as usize;

    match super::with_device(index, |dev| {
        let conn = dev.connectors().get(conn_idx)
            .ok_or(KernelError::NotFound)?;

        let status: u64 = match conn.status {
            super::connector::ConnectorStatus::Disconnected => 0,
            super::connector::ConnectorStatus::Connected => 1,
            super::connector::ConnectorStatus::Unknown => 2,
        };

        let ctype: u64 = match conn.connector_type {
            ConnectorType::Virtual => 0,
            ConnectorType::Hdmi => 1,
            ConnectorType::DisplayPort => 2,
            ConnectorType::Vga => 3,
            ConnectorType::Lvds => 4,
            ConnectorType::Dvi => 5,
            ConnectorType::Edp => 6,
        };

        let mode_count = conn.modes.len() as u64;
        let id = conn.id.raw() as u64;

        Ok((status | (ctype << 8) | (mode_count << 16) | (id << 32)) as i64)
    }) {
        Ok(packed) => SyscallResult::ok(packed),
        Err(e) => SyscallResult::err(e),
    }
}

// ---------------------------------------------------------------------------
// SYS_DRM_MODE_GET (1041)
// ---------------------------------------------------------------------------

/// Get display mode resolution and refresh rate.
///
/// Returns `hdisplay | (vdisplay << 16) | (vrefresh << 32)`.
#[allow(clippy::arithmetic_side_effects)]
pub fn sys_drm_mode_get(args: &SyscallArgs) -> SyscallResult {
    let index = match validate_device(args.arg0) {
        Ok(i) => i,
        Err(e) => return SyscallResult::err(e),
    };

    let conn_idx = args.arg1 as usize;
    let mode_idx = args.arg2 as usize;

    match super::with_device(index, |dev| {
        let conn = dev.connectors().get(conn_idx)
            .ok_or(KernelError::NotFound)?;
        let mode = conn.modes.get(mode_idx)
            .ok_or(KernelError::NotFound)?;

        let packed = (mode.hdisplay as u64)
            | ((mode.vdisplay as u64) << 16)
            | ((mode.vrefresh as u64) << 32);
        Ok(packed as i64)
    }) {
        Ok(v) => SyscallResult::ok(v),
        Err(e) => SyscallResult::err(e),
    }
}

// ---------------------------------------------------------------------------
// SYS_DRM_CRTC_INFO (1042)
// ---------------------------------------------------------------------------

/// Get CRTC info.
///
/// Returns `crtc_id | (active << 32) | (has_mode << 33)`.
#[allow(clippy::arithmetic_side_effects)]
pub fn sys_drm_crtc_info(args: &SyscallArgs) -> SyscallResult {
    let index = match validate_device(args.arg0) {
        Ok(i) => i,
        Err(e) => return SyscallResult::err(e),
    };

    let crtc_idx = args.arg1 as usize;

    match super::with_device(index, |dev| {
        let crtc = dev.crtcs().get(crtc_idx)
            .ok_or(KernelError::NotFound)?;

        let id = crtc.id.raw() as u64;
        let active_bit = if crtc.active { 1u64 << 32 } else { 0 };
        let mode_bit = if crtc.mode.is_some() { 1u64 << 33 } else { 0 };

        Ok((id | active_bit | mode_bit) as i64)
    }) {
        Ok(v) => SyscallResult::ok(v),
        Err(e) => SyscallResult::err(e),
    }
}

// ---------------------------------------------------------------------------
// SYS_DRM_CURSOR_SET (1050)
// ---------------------------------------------------------------------------

/// Set cursor image on a CRTC.
#[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
pub fn sys_drm_cursor_set(args: &SyscallArgs) -> SyscallResult {
    let index = match validate_device(args.arg0) {
        Ok(i) => i,
        Err(e) => return SyscallResult::err(e),
    };

    let crtc_id = DrmObjectId::new(args.arg1 as u32);
    let gem_handle = args.arg2 as u32;
    let width = (args.arg3 & 0xFFFF) as u32;
    let height = ((args.arg3 >> 16) & 0xFFFF) as u32;
    let hot_x = ((args.arg3 >> 32) & 0xFFFF) as u32;
    let hot_y = ((args.arg3 >> 48) & 0xFFFF) as u32;

    match super::with_device_mut(index, |dev| {
        dev.cursor_set(crtc_id, gem_handle, width, height, hot_x, hot_y)
    }) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

// ---------------------------------------------------------------------------
// SYS_DRM_CURSOR_MOVE (1051)
// ---------------------------------------------------------------------------

/// Move cursor position.
pub fn sys_drm_cursor_move(args: &SyscallArgs) -> SyscallResult {
    let index = match validate_device(args.arg0) {
        Ok(i) => i,
        Err(e) => return SyscallResult::err(e),
    };

    let crtc_id = DrmObjectId::new(args.arg1 as u32);
    let x = args.arg2 as i32;
    let y = args.arg3 as i32;

    match super::with_device_mut(index, |dev| {
        dev.cursor_move(crtc_id, x, y)
    }) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

// ---------------------------------------------------------------------------
// SYS_DRM_ATOMIC_COMMIT (1060)
// ---------------------------------------------------------------------------

/// Atomic modesetting commit from userspace.
///
/// Reads a serialized `AtomicState` buffer from the calling process's
/// address space, validates it, and commits the changes.
#[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
pub fn sys_drm_atomic_commit(args: &SyscallArgs) -> SyscallResult {
    let index = match validate_device(args.arg0) {
        Ok(i) => i,
        Err(e) => return SyscallResult::err(e),
    };

    let buf_ptr = args.arg1;
    let buf_len = args.arg2 as usize;
    let flags = args.arg3;
    let test_only = (flags & 1) != 0;

    // Read the buffer from userspace.
    if buf_ptr == 0 || buf_len < 12 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    // For now, read the buffer directly from the kernel address space.
    // When userspace is fully isolated, this will need to copy from
    // the user's address space via safe accessor functions.
    //
    // SAFETY: The caller is responsible for passing a valid buffer.
    // In the current kernel-mode testing setup, all addresses are valid.
    let buf = unsafe {
        core::slice::from_raw_parts(buf_ptr as *const u8, buf_len)
    };

    // Parse the header.
    let n_crtc = read_u32(buf, 0) as usize;
    let n_plane = read_u32(buf, 4) as usize;
    let n_conn = read_u32(buf, 8) as usize;

    let mut offset = 12;
    let mut state = AtomicState::new();
    state.test_only = test_only;

    // Parse CRTC changes (12 bytes each).
    for _ in 0..n_crtc {
        if offset + 12 > buf_len {
            return SyscallResult::err(KernelError::InvalidArgument);
        }
        let crtc_id = DrmObjectId::new(read_u32(buf, offset));
        let cflags = read_u32(buf, offset + 4);
        let mode_packed = read_u32(buf, offset + 8);
        offset += 12;

        let active = if (cflags & 1) != 0 {
            Some((cflags & 2) != 0)
        } else {
            None
        };

        let mode = if (cflags & 4) != 0 {
            if (cflags & 8) != 0 {
                // Disable mode.
                Some(None)
            } else {
                // Set mode from packed hdisplay|vdisplay.
                let hdisplay = mode_packed & 0xFFFF;
                let vdisplay = (mode_packed >> 16) & 0xFFFF;
                Some(Some(DrmMode::from_resolution(hdisplay, vdisplay, 60)))
            }
        } else {
            None
        };

        state.add_crtc(CrtcState { id: crtc_id, active, mode });
    }

    // Parse plane changes (32 bytes each).
    for _ in 0..n_plane {
        if offset + 32 > buf_len {
            return SyscallResult::err(KernelError::InvalidArgument);
        }
        let plane_id = DrmObjectId::new(read_u32(buf, offset));
        let pflags = read_u32(buf, offset + 4);
        let fb_raw = read_u32(buf, offset + 8);
        let crtc_raw = read_u32(buf, offset + 12);
        let src_xy = read_u32(buf, offset + 16);
        let src_wh = read_u32(buf, offset + 20);
        let dst_xy = read_u32(buf, offset + 24);
        let dst_wh = read_u32(buf, offset + 28);
        offset += 32;

        let fb_id = if (pflags & 1) != 0 {
            if fb_raw == 0 { Some(None) } else { Some(Some(DrmObjectId::new(fb_raw))) }
        } else {
            None
        };

        let crtc_id = if (pflags & 2) != 0 {
            if crtc_raw == 0 { Some(None) } else { Some(Some(DrmObjectId::new(crtc_raw))) }
        } else {
            None
        };

        let src_rect = if (pflags & 4) != 0 {
            Some(Rect {
                x: src_xy & 0xFFFF,
                y: (src_xy >> 16) & 0xFFFF,
                w: src_wh & 0xFFFF,
                h: (src_wh >> 16) & 0xFFFF,
            })
        } else {
            None
        };

        let dst_rect = if (pflags & 8) != 0 {
            Some(IRect {
                x: (dst_xy & 0xFFFF) as i16 as i32,
                y: ((dst_xy >> 16) & 0xFFFF) as i16 as i32,
                w: dst_wh & 0xFFFF,
                h: (dst_wh >> 16) & 0xFFFF,
            })
        } else {
            None
        };

        state.add_plane(PlaneState {
            id: plane_id,
            fb_id,
            crtc_id,
            src_rect,
            dst_rect,
        });
    }

    // Parse connector changes (8 bytes each).
    for _ in 0..n_conn {
        if offset + 8 > buf_len {
            return SyscallResult::err(KernelError::InvalidArgument);
        }
        let conn_id = DrmObjectId::new(read_u32(buf, offset));
        let crtc_raw = read_u32(buf, offset + 4);
        offset += 8;

        let crtc_id = if crtc_raw == 0xFFFF_FFFF {
            Some(None) // Unbind.
        } else {
            Some(Some(DrmObjectId::new(crtc_raw)))
        };

        state.add_connector(ConnectorState { id: conn_id, crtc_id });
    }

    match super::with_device_mut(index, |dev| {
        super::atomic::atomic_commit(dev, &state)
    }) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// Read a little-endian u32 from a byte slice at a given offset.
#[allow(clippy::arithmetic_side_effects)]
fn read_u32(buf: &[u8], off: usize) -> u32 {
    if off + 4 > buf.len() {
        return 0;
    }
    u32::from_le_bytes([buf[off], buf[off + 1], buf[off + 2], buf[off + 3]])
}
