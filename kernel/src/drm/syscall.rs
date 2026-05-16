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

use super::mode::PixelFormat;
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
