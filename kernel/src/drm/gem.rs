//! GEM (Graphics Execution Manager) buffer objects.
//!
//! A GEM object is a GPU memory allocation.  For now, all buffers
//! live in system RAM (allocated via the frame allocator).  When real
//! GPU drivers with dedicated VRAM are added, the GEM allocator will
//! manage VRAM address space as well.
//!
//! ## Handle Namespace
//!
//! GEM handles are per-DRM-device.  In the future, when userspace
//! processes open DRM devices, handles will be per-process (like
//! Linux flink/prime).  For now in kernel mode, handles are global
//! per-device.
//!
//! ## References
//!
//! - Linux `drivers/gpu/drm/drm_gem.c`
//! - Linux `include/drm/drm_gem.h`

extern crate alloc;
use alloc::vec::Vec;

use core::sync::atomic::{AtomicU32, Ordering};

use crate::mm::frame::{self, PhysFrame, FRAME_SIZE};
use crate::mm::page_table;
use crate::error::{KernelError, KernelResult};

use super::DrmObjectId;
use super::mode::PixelFormat;

// ---------------------------------------------------------------------------
// GEM object
// ---------------------------------------------------------------------------

/// Next GEM handle to allocate (global, monotonic).
static NEXT_HANDLE: AtomicU32 = AtomicU32::new(1);

/// Allocate a fresh GEM handle.
pub fn alloc_handle() -> u32 {
    NEXT_HANDLE.fetch_add(1, Ordering::Relaxed)
}

/// A GPU buffer object.
pub struct GemObject {
    /// Per-device handle (userspace-visible identifier).
    pub handle: u32,
    /// Device-global object ID.
    pub id: DrmObjectId,
    /// Total size in bytes.
    pub size: usize,
    /// Backing physical frames.
    ///
    /// For system-RAM buffers, these are standard 16 KiB frames from
    /// the buddy allocator.  For VRAM buffers (future), this would be
    /// VRAM page descriptors.
    pub phys_frames: Vec<PhysFrame>,
    /// Pixel format (if this buffer is intended as a framebuffer).
    pub format: PixelFormat,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Bytes per row.
    pub pitch: u32,
}

impl GemObject {
    /// Allocate a new GEM buffer for a 2D surface.
    ///
    /// Allocates enough physical frames to hold `height` rows of
    /// `pitch` bytes each.  The buffer is zeroed.
    #[allow(clippy::arithmetic_side_effects)]
    pub fn alloc_2d(
        dev_id_alloc: &dyn Fn() -> DrmObjectId,
        width: u32,
        height: u32,
        format: PixelFormat,
    ) -> KernelResult<Self> {
        let pitch = format.pitch(width);
        let size = (pitch as usize)
            .checked_mul(height as usize)
            .ok_or(KernelError::InvalidArgument)?;
        if size == 0 {
            return Err(KernelError::InvalidArgument);
        }

        // How many frames we need.
        let frame_count = size.div_ceil(FRAME_SIZE);

        // Allocate frames.
        let mut phys_frames = Vec::with_capacity(frame_count);
        for _ in 0..frame_count {
            match frame::alloc_frame() {
                Ok(f) => phys_frames.push(f),
                Err(e) => {
                    // Cleanup: free already-allocated frames.
                    for pf in phys_frames {
                        // SAFETY: we just allocated these frames.
                        unsafe { let _ = frame::free_frame(pf); }
                    }
                    return Err(e);
                }
            }
        }

        // Zero the buffer via HHDM.
        let hhdm = page_table::hhdm().ok_or(KernelError::NotSupported)?;
        for pf in &phys_frames {
            let virt = pf.addr().checked_add(hhdm)
                .ok_or(KernelError::InvalidAddress)?;
            // SAFETY: The frame was just allocated (we own it exclusively)
            // and the HHDM mapping covers all physical memory.
            unsafe {
                core::ptr::write_bytes(virt as *mut u8, 0, FRAME_SIZE);
            }
        }

        let handle = alloc_handle();
        let id = dev_id_alloc();

        Ok(Self {
            handle,
            id,
            size,
            phys_frames,
            format,
            width,
            height,
            pitch,
        })
    }

    /// Get a kernel-virtual pointer to the buffer's first byte.
    ///
    /// This works because the HHDM maps all physical memory.  The
    /// pointer is valid as long as the GEM object exists.
    ///
    /// Note: if the buffer spans multiple non-contiguous frames, only
    /// the first frame is directly accessible through this pointer.
    /// For buffers within one frame, this is the entire buffer.
    /// For multi-frame buffers, callers must handle frame boundaries.
    pub fn virt_addr(&self) -> KernelResult<*mut u8> {
        let first = self.phys_frames.first()
            .ok_or(KernelError::InternalError)?;
        let hhdm = page_table::hhdm().ok_or(KernelError::NotSupported)?;
        let virt = first.addr().checked_add(hhdm)
            .ok_or(KernelError::InvalidAddress)?;
        Ok(virt as *mut u8)
    }

    /// Free all backing physical frames.
    pub fn free_backing(&mut self) {
        for pf in self.phys_frames.drain(..) {
            // SAFETY: We allocated these frames in alloc_2d and own them.
            unsafe { let _ = frame::free_frame(pf); }
        }
    }
}
