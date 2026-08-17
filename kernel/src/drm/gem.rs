//! GEM (Graphics Execution Manager) buffer objects.
//!
//! A GEM object is a GPU memory allocation. Where those bytes actually live
//! depends on the driver holding the object, and the two cases are genuinely
//! different kinds of memory rather than two addresses for the same kind — see
//! [`GemBacking`].
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

use crate::error::{KernelError, KernelResult};
use crate::mm::frame::{self, FRAME_SIZE, PhysFrame};
use crate::mm::page_table;

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

/// Where a GEM object's bytes actually live.
///
/// ## Why this is an enum and not a pointer
///
/// The two variants are not two ways of naming the same storage; they differ
/// in who may free them, how the CPU reaches them, and whether they are
/// contiguous. Collapsing them into one representation — a `Vec<PhysFrame>`
/// that happens to hold BAR addresses for the VRAM case — is a corruption bug
/// waiting to be written, because [`GemObject::free_backing`] hands frames to
/// the buddy allocator. Doing that with card memory does not leak, it gives
/// the system allocator addresses that were never RAM, and the damage surfaces
/// arbitrarily far away from the driver that caused it.
///
/// Keeping the distinction in the type means every consumer of the backing has
/// to say which case it can handle, and the compiler asks. That is the point.
pub enum GemBacking {
    /// 16 KiB frames from the buddy allocator, reachable through the HHDM.
    ///
    /// Discontiguous in general: a buffer larger than one frame is a list of
    /// unrelated frames, which is why every consumer walks it per row rather
    /// than treating the first address as a base.
    SystemRam(Vec<PhysFrame>),
    /// A run of a display card's own video memory.
    ///
    /// `offset` is in bytes from the base of VRAM — the same origin the CRTC's
    /// scanout register and the card's VRAM suballocator use, so the value can
    /// be handed to the hardware without conversion. `len` is the reservation,
    /// which the owning driver needs in order to give it back.
    ///
    /// There is no [`PhysFrame`] here on purpose. These bytes are behind a PCI
    /// BAR, are not in the frame allocator's map, and are reachable only
    /// through the mapping the owning driver holds — so they are that driver's
    /// to hand out and its to reclaim, and nobody else's.
    Vram {
        /// Byte offset from the base of the card's video memory.
        offset: u32,
        /// Length of the reservation, in bytes.
        len: u32,
    },
}

/// A GPU buffer object.
pub struct GemObject {
    /// Per-device handle (userspace-visible identifier).
    pub handle: u32,
    /// Device-global object ID.
    pub id: DrmObjectId,
    /// Total size in bytes.
    pub size: usize,
    /// Where the bytes live, and who owns them.
    pub backing: GemBacking,
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
                        unsafe {
                            let _ = frame::free_frame(pf);
                        }
                    }
                    return Err(e);
                }
            }
        }

        // Zero the buffer via HHDM.
        let hhdm = page_table::hhdm().ok_or(KernelError::NotSupported)?;
        for pf in &phys_frames {
            let virt = pf
                .addr()
                .checked_add(hhdm)
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
            backing: GemBacking::SystemRam(phys_frames),
            format,
            width,
            height,
            pitch,
        })
    }

    /// Describe a buffer that lives in a display card's video memory.
    ///
    /// Allocates nothing: the caller's VRAM suballocator has already reserved
    /// `offset..offset + len`, and this only records that fact. Nor does it
    /// zero the buffer — the caller holds the only mapping of the aperture, so
    /// it is the only code that *can*, and doing it there keeps the one slow
    /// uncached pass over the pixels in the driver that knows how slow it is.
    ///
    /// # Errors
    ///
    /// `InvalidArgument` if the reservation is empty or too small for
    /// `height` rows of `pitch` bytes. Checked here rather than trusted
    /// because the consequence of a short reservation is a CRTC scanning out
    /// past the end of it, which is another buffer's pixels or none at all.
    pub fn from_vram(
        dev_id_alloc: &dyn Fn() -> DrmObjectId,
        offset: u32,
        len: u32,
        width: u32,
        height: u32,
        pitch: u32,
        format: PixelFormat,
    ) -> KernelResult<Self> {
        let size = (pitch as usize)
            .checked_mul(height as usize)
            .ok_or(KernelError::InvalidArgument)?;
        if size == 0 || len == 0 || (len as usize) < size {
            return Err(KernelError::InvalidArgument);
        }
        Ok(Self {
            handle: alloc_handle(),
            id: dev_id_alloc(),
            size,
            backing: GemBacking::Vram { offset, len },
            format,
            width,
            height,
            pitch,
        })
    }

    /// The system-RAM frames backing this object.
    ///
    /// # Errors
    ///
    /// `NotSupported` for a VRAM-resident object. That is not a gap to be
    /// filled in later: such an object has no frames, because its bytes are on
    /// the card and are reachable only through the owning driver's aperture.
    /// Callers that walk frames are asking for something that does not exist,
    /// and an error says so where an empty slice would quietly read as "a
    /// zero-byte buffer".
    pub fn ram_frames(&self) -> KernelResult<&[PhysFrame]> {
        match &self.backing {
            GemBacking::SystemRam(frames) => Ok(frames),
            GemBacking::Vram { .. } => Err(KernelError::NotSupported),
        }
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
    ///
    /// # Errors
    ///
    /// `NotSupported` for a VRAM-resident object — ask the owning driver's
    /// `gem_mmap` instead, which is the only code holding a mapping of the
    /// card's aperture.
    pub fn virt_addr(&self) -> KernelResult<*mut u8> {
        let first = self
            .ram_frames()?
            .first()
            .ok_or(KernelError::InternalError)?;
        let hhdm = page_table::hhdm().ok_or(KernelError::NotSupported)?;
        let virt = first
            .addr()
            .checked_add(hhdm)
            .ok_or(KernelError::InvalidAddress)?;
        Ok(virt as *mut u8)
    }

    /// Free system-RAM backing, returning the frames to the buddy allocator.
    ///
    /// # Errors
    ///
    /// `NotSupported` for a VRAM-resident object, whose reservation belongs to
    /// the owning driver's suballocator. Refusing is the whole reason
    /// [`GemBacking`] is a type: freeing card memory *through here* would push
    /// BAR addresses into the system frame allocator, which is silent
    /// corruption rather than a leak. A driver that holds VRAM objects must
    /// release them in its own `gem_destroy`; if this error is ever seen, one
    /// forgot.
    pub fn free_backing(&mut self) -> KernelResult<()> {
        match &mut self.backing {
            GemBacking::SystemRam(frames) => {
                for pf in frames.drain(..) {
                    // SAFETY: We allocated these frames in alloc_2d and own them.
                    unsafe {
                        let _ = frame::free_frame(pf);
                    }
                }
                Ok(())
            }
            GemBacking::Vram { .. } => Err(KernelError::NotSupported),
        }
    }
}
