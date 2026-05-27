//! DRM driver trait and backend implementations.
//!
//! Each GPU driver implements the methods needed by the DRM core.
//! The concrete types are used through [`super::DrmBackend`] enum
//! dispatch (not `dyn Trait`) for hot-path performance.
//!
//! ## Backends
//!
//! - [`LimineBackend`]: wraps the bootloader framebuffer
//! - [`VirtioGpuBackend`]: wraps the virtio-gpu paravirtualized driver

extern crate alloc;
use alloc::vec;
use alloc::vec::Vec;

use crate::error::{KernelError, KernelResult};
use crate::mm::frame::{FRAME_SIZE};
use crate::mm::page_table;

use super::connector::{ConnectorStatus, ConnectorType, DrmConnector};
use super::crtc::DrmCrtc;
use super::encoder::{DrmEncoder, EncoderType};
use super::framebuffer::DrmFramebuffer;
use super::gem::GemObject;
use super::mode::{DrmMode, DrmModeFlags, PixelFormat};
use super::plane::{DrmPlane, PlaneType};
use super::DrmObjectId;

// ===========================================================================
// DrmDriver trait (documentation / testing interface)
// ===========================================================================

/// Operations a DRM backend must support.
///
/// This trait exists for documentation and potential future use in
/// test mocking.  The actual runtime dispatch goes through the
/// [`super::DrmBackend`] enum for performance.
#[allow(dead_code)]
pub trait DrmDriver: Send {
    /// Human-readable driver name.
    fn name(&self) -> &'static str;

    /// Query hardware and return DRM objects.
    fn enumerate(
        &mut self,
        alloc_id: &dyn Fn() -> DrmObjectId,
    ) -> KernelResult<(Vec<DrmConnector>, Vec<DrmCrtc>, Vec<DrmPlane>, Vec<DrmEncoder>)>;

    /// Allocate a GPU buffer.
    fn gem_create(
        &mut self,
        alloc_id: &dyn Fn() -> DrmObjectId,
        width: u32,
        height: u32,
        format: PixelFormat,
    ) -> KernelResult<GemObject>;

    /// Free a GPU buffer.
    fn gem_destroy(&mut self, gem: GemObject) -> KernelResult<()>;

    /// Get kernel-virtual pointer to GEM backing memory.
    fn gem_mmap(&self, gem: &GemObject) -> KernelResult<*mut u8>;

    /// Swap the displayed framebuffer on a CRTC.
    fn page_flip(
        &mut self,
        crtc_id: DrmObjectId,
        fb: &DrmFramebuffer,
        gem: &GemObject,
    ) -> KernelResult<()>;

    /// Flush a dirty region to the display.
    fn flush_region(
        &mut self,
        fb: &DrmFramebuffer,
        gem: &GemObject,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
    ) -> KernelResult<()>;
}

// ===========================================================================
// Limine framebuffer backend
// ===========================================================================

/// DRM backend for the bootloader-provided framebuffer.
///
/// This is the simplest possible backend: a fixed-resolution,
/// fixed-format, single-CRTC display.  Mode switching is not
/// supported (the resolution is determined by the bootloader).
///
/// Page-flip copies from the GEM backing memory to the Limine
/// framebuffer's memory-mapped region.  This is a CPU-side memcpy
/// until real GPU support is available.
pub struct LimineBackend {
    /// Framebuffer virtual address (HHDM-mapped).
    fb_addr: u64,
    /// Width in pixels.
    width: u32,
    /// Height in pixels.
    height: u32,
    /// Bytes per row.
    pitch: u32,
}

impl LimineBackend {
    /// Create a new Limine backend from the current framebuffer state.
    ///
    /// Reads the framebuffer parameters from [`crate::console`].
    #[must_use]
    pub fn new() -> Self {
        let (fb_addr, width, height, pitch) =
            crate::console::framebuffer_info().unwrap_or((0, 0, 0, 0));
        Self {
            fb_addr,
            width,
            height,
            pitch,
        }
    }

    /// Driver name.
    #[must_use]
    pub fn name(&self) -> &'static str {
        "limine-fb"
    }

    /// Enumerate the single fixed display.
    pub fn enumerate(
        &mut self,
        alloc_id: &dyn Fn() -> DrmObjectId,
    ) -> KernelResult<(Vec<DrmConnector>, Vec<DrmCrtc>, Vec<DrmPlane>, Vec<DrmEncoder>)> {
        if self.fb_addr == 0 || self.width == 0 || self.height == 0 {
            return Err(KernelError::NotSupported);
        }

        let encoder_id = alloc_id();
        let crtc_id = alloc_id();
        let plane_id = alloc_id();
        let connector_id = alloc_id();

        let mode = DrmMode::from_resolution(self.width, self.height, 60);

        let connector = DrmConnector {
            id: connector_id,
            connector_type: ConnectorType::Virtual,
            status: ConnectorStatus::Connected,
            modes: vec![mode],
            current_encoder: Some(encoder_id),
            possible_encoders: vec![encoder_id],
            edid: None, // Limine framebuffer has no EDID.
        };

        let plane = DrmPlane {
            id: plane_id,
            plane_type: PlaneType::Primary,
            possible_crtcs: 1, // bit 0 = CRTC index 0
            formats: vec![PixelFormat::Xrgb8888, PixelFormat::Argb8888],
            fb: None,
            crtc: Some(crtc_id),
            src_x: 0,
            src_y: 0,
            src_w: self.width,
            src_h: self.height,
            dst_x: 0,
            dst_y: 0,
            dst_w: self.width,
            dst_h: self.height,
        };

        let crtc = DrmCrtc {
            id: crtc_id,
            active: true,
            mode: Some(mode),
            primary_plane: plane_id,
            cursor_plane: None,
            gamma_size: 0,
            index: 0,
        };

        let encoder = DrmEncoder {
            id: encoder_id,
            encoder_type: EncoderType::Virtual,
            crtc: Some(crtc_id),
            possible_crtcs: 1,
        };

        Ok((vec![connector], vec![crtc], vec![plane], vec![encoder]))
    }

    /// Allocate a system-RAM GEM buffer.
    pub fn gem_create(
        &mut self,
        alloc_id: &dyn Fn() -> DrmObjectId,
        width: u32,
        height: u32,
        format: PixelFormat,
    ) -> KernelResult<GemObject> {
        GemObject::alloc_2d(alloc_id, width, height, format)
    }

    /// Free a GEM buffer.
    pub fn gem_destroy(&mut self, mut gem: GemObject) -> KernelResult<()> {
        gem.free_backing();
        Ok(())
    }

    /// Get kernel-virtual pointer to GEM memory.
    pub fn gem_mmap(&self, gem: &GemObject) -> KernelResult<*mut u8> {
        gem.virt_addr()
    }

    /// Page flip: copy GEM backing to the Limine framebuffer.
    #[allow(clippy::arithmetic_side_effects)]
    pub fn page_flip(
        &mut self,
        _crtc_id: DrmObjectId,
        fb: &DrmFramebuffer,
        gem: &GemObject,
    ) -> KernelResult<()> {
        if self.fb_addr == 0 {
            return Err(KernelError::NotSupported);
        }
        let hhdm = page_table::hhdm().ok_or(KernelError::NotSupported)?;

        let dst = self.fb_addr as *mut u8;
        let copy_h = fb.height.min(self.height) as usize;
        let copy_w_bytes = (fb.width.min(self.width) as usize) * (fb.format.bpp() as usize);

        for row in 0..copy_h {
            // Source: GEM backing (may span multiple frames).
            let src_byte_offset = row * (fb.pitch as usize);
            let frame_idx = src_byte_offset / FRAME_SIZE;
            let frame_offset = src_byte_offset % FRAME_SIZE;

            if let Some(pf) = gem.phys_frames.get(frame_idx) {
                let src_virt = pf.addr() + hhdm + (frame_offset as u64);
                // SAFETY: dst is the Limine framebuffer base (mapped at boot).
                // row * pitch stays within the framebuffer's linear region.
                let dst_row = unsafe { dst.add(row * (self.pitch as usize)) };

                // How many bytes are available in this frame.
                let avail = FRAME_SIZE - frame_offset;
                let to_copy = copy_w_bytes.min(avail);

                // SAFETY: Both src and dst point to valid mapped memory.
                // src is in the HHDM range (we just allocated the frame).
                // dst is the Limine framebuffer (mapped at init).
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        src_virt as *const u8,
                        dst_row,
                        to_copy,
                    );
                }

                // If the row spans a frame boundary, copy the rest from
                // the next frame.
                if to_copy < copy_w_bytes {
                    if let Some(pf2) = gem.phys_frames.get(frame_idx + 1) {
                        let src2 = pf2.addr() + hhdm;
                        let remaining = copy_w_bytes - to_copy;
                        unsafe {
                            core::ptr::copy_nonoverlapping(
                                src2 as *const u8,
                                dst_row.add(to_copy),
                                remaining,
                            );
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Flush region: same as page_flip but for a sub-rectangle.
    #[allow(clippy::arithmetic_side_effects)]
    pub fn flush_region(
        &mut self,
        fb: &DrmFramebuffer,
        gem: &GemObject,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
    ) -> KernelResult<()> {
        if self.fb_addr == 0 {
            return Err(KernelError::NotSupported);
        }
        let hhdm = page_table::hhdm().ok_or(KernelError::NotSupported)?;

        let dst_base = self.fb_addr as *mut u8;
        let bpp = fb.format.bpp() as usize;
        let copy_w_bytes = (w as usize) * bpp;

        let y_end = (y + h).min(fb.height).min(self.height);
        let x_start = x.min(fb.width).min(self.width) as usize;

        for row in (y as usize)..(y_end as usize) {
            let src_byte_offset = row * (fb.pitch as usize) + x_start * bpp;
            let frame_idx = src_byte_offset / FRAME_SIZE;
            let frame_offset = src_byte_offset % FRAME_SIZE;

            if let Some(pf) = gem.phys_frames.get(frame_idx) {
                let src_virt = pf.addr() + hhdm + (frame_offset as u64);
                let dst_row = unsafe {
                    dst_base.add(row * (self.pitch as usize) + x_start * bpp)
                };

                let avail = FRAME_SIZE - frame_offset;
                let to_copy = copy_w_bytes.min(avail);

                // SAFETY: Both addresses point to valid mapped memory.
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        src_virt as *const u8,
                        dst_row,
                        to_copy,
                    );
                }
            }
        }
        Ok(())
    }
}

// ===========================================================================
// virtio-GPU backend
// ===========================================================================

/// DRM backend for the virtio-gpu paravirtualized device.
///
/// Wraps the existing [`crate::virtio::gpu`] driver, mapping its
/// concepts to DRM objects:
///
/// - virtio scanout → DRM connector + CRTC + encoder
/// - virtio resource → GEM object
/// - `GET_DISPLAY_INFO` → connector mode list
/// - `SET_SCANOUT` + `TRANSFER_TO_HOST_2D` + `RESOURCE_FLUSH` → page flip
pub struct VirtioGpuBackend {
    /// Whether the virtio-gpu device was found and initialized.
    available: bool,
    /// Display width from GET_DISPLAY_INFO.
    width: u32,
    /// Display height from GET_DISPLAY_INFO.
    height: u32,
}

impl VirtioGpuBackend {
    /// Probe for a virtio-gpu device.
    ///
    /// Returns `Some(backend)` if the device is present and initialized.
    pub fn probe() -> Option<Self> {
        // Check if virtio-gpu is initialized by querying its display size.
        let (w, h) = crate::virtio::gpu::dimensions();
        if w > 0 && h > 0 {
            Some(Self {
                available: true,
                width: w,
                height: h,
            })
        } else {
            None
        }
    }

    /// Driver name.
    #[must_use]
    pub fn name(&self) -> &'static str {
        "virtio-gpu"
    }

    /// Enumerate the virtio-gpu display.
    pub fn enumerate(
        &mut self,
        alloc_id: &dyn Fn() -> DrmObjectId,
    ) -> KernelResult<(Vec<DrmConnector>, Vec<DrmCrtc>, Vec<DrmPlane>, Vec<DrmEncoder>)> {
        if !self.available {
            return Err(KernelError::NotSupported);
        }

        let encoder_id = alloc_id();
        let crtc_id = alloc_id();
        let plane_id = alloc_id();
        let connector_id = alloc_id();

        // Build a mode from the display info.
        let mut mode = DrmMode::from_resolution(self.width, self.height, 60);
        mode.flags = DrmModeFlags::PREFERRED;

        let connector = DrmConnector {
            id: connector_id,
            connector_type: ConnectorType::Virtual,
            status: ConnectorStatus::Connected,
            modes: vec![mode],
            current_encoder: Some(encoder_id),
            possible_encoders: vec![encoder_id],
            edid: None, // virtio-gpu: could use GET_EDID in future.
        };

        let plane = DrmPlane {
            id: plane_id,
            plane_type: PlaneType::Primary,
            possible_crtcs: 1,
            formats: vec![PixelFormat::Xrgb8888, PixelFormat::Argb8888],
            fb: None,
            crtc: Some(crtc_id),
            src_x: 0,
            src_y: 0,
            src_w: self.width,
            src_h: self.height,
            dst_x: 0,
            dst_y: 0,
            dst_w: self.width,
            dst_h: self.height,
        };

        let crtc = DrmCrtc {
            id: crtc_id,
            active: true,
            mode: Some(mode),
            primary_plane: plane_id,
            cursor_plane: None,
            gamma_size: 0,
            index: 0,
        };

        let encoder = DrmEncoder {
            id: encoder_id,
            encoder_type: EncoderType::Virtual,
            crtc: Some(crtc_id),
            possible_crtcs: 1,
        };

        Ok((vec![connector], vec![crtc], vec![plane], vec![encoder]))
    }

    /// Allocate a GEM buffer via system RAM.
    ///
    /// In the future, this could create a virtio-gpu resource and
    /// attach backing in one operation.  For now, we use plain frames
    /// and do the transfer on page_flip/flush.
    pub fn gem_create(
        &mut self,
        alloc_id: &dyn Fn() -> DrmObjectId,
        width: u32,
        height: u32,
        format: PixelFormat,
    ) -> KernelResult<GemObject> {
        GemObject::alloc_2d(alloc_id, width, height, format)
    }

    /// Free a GEM buffer.
    pub fn gem_destroy(&mut self, mut gem: GemObject) -> KernelResult<()> {
        gem.free_backing();
        Ok(())
    }

    /// Get kernel-virtual pointer to GEM memory.
    pub fn gem_mmap(&self, gem: &GemObject) -> KernelResult<*mut u8> {
        gem.virt_addr()
    }

    /// Page flip via virtio-gpu: bulk memcpy + host transfer.
    ///
    /// Since virtio-gpu is paravirtualized, "page flip" means copying
    /// the GEM buffer contents into the virtio-gpu driver's backing
    /// memory (which is already registered with the host as a resource),
    /// then issuing `TRANSFER_TO_HOST_2D` + `RESOURCE_FLUSH`.
    ///
    /// OPT: Uses row-level memcpy (copy_nonoverlapping) instead of
    /// per-pixel set_pixel().  For 1920×1080 XRGB8888 this reduces
    /// from ~8M function calls to ~1080 memcpy calls — roughly 100×
    /// faster on real hardware.
    ///
    /// Future: create a new virtio-gpu resource per GEM object and
    /// SET_SCANOUT to it directly — eliminating the copy entirely.
    #[allow(clippy::arithmetic_side_effects)]
    pub fn page_flip(
        &mut self,
        _crtc_id: DrmObjectId,
        fb: &DrmFramebuffer,
        gem: &GemObject,
    ) -> KernelResult<()> {
        if !self.available {
            return Err(KernelError::NotSupported);
        }

        // Get the virtio-gpu framebuffer's virtual address (HHDM-mapped).
        let dst_base = crate::virtio::gpu::framebuffer_addr()
            .ok_or(KernelError::NotSupported)?;

        let hhdm = page_table::hhdm().ok_or(KernelError::NotSupported)?;

        let bpp = fb.format.bpp() as usize;
        let copy_h = fb.height.min(self.height) as usize;
        let copy_w_bytes = (fb.width.min(self.width) as usize) * bpp;
        let dst_pitch = (self.width as usize) * bpp;

        for row in 0..copy_h {
            // Source: GEM backing (may span multiple frames).
            let src_byte_offset = row * (fb.pitch as usize);
            let frame_idx = src_byte_offset / FRAME_SIZE;
            let frame_offset = src_byte_offset % FRAME_SIZE;

            if let Some(pf) = gem.phys_frames.get(frame_idx) {
                let src_virt = pf.addr() + hhdm + (frame_offset as u64);
                // Destination: virtio-gpu driver's framebuffer.
                let dst_row = dst_base + (row * dst_pitch) as u64;

                let avail = FRAME_SIZE - frame_offset;
                let to_copy = copy_w_bytes.min(avail);

                // SAFETY: src is within an HHDM-mapped GEM frame.
                // dst is within the virtio-gpu driver's allocated framebuffer.
                // Both regions are valid and non-overlapping.
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        src_virt as *const u8,
                        dst_row as *mut u8,
                        to_copy,
                    );
                }

                // If the row spans a frame boundary, copy the remainder.
                if to_copy < copy_w_bytes {
                    if let Some(pf2) = gem.phys_frames.get(frame_idx + 1) {
                        let src2 = pf2.addr() + hhdm;
                        let remaining = copy_w_bytes - to_copy;
                        unsafe {
                            core::ptr::copy_nonoverlapping(
                                src2 as *const u8,
                                (dst_row + to_copy as u64) as *mut u8,
                                remaining,
                            );
                        }
                    }
                }
            }
        }

        crate::virtio::gpu::flush_full()
    }

    /// Flush a sub-region: bulk memcpy + partial host transfer.
    #[allow(clippy::arithmetic_side_effects)]
    pub fn flush_region(
        &mut self,
        fb: &DrmFramebuffer,
        gem: &GemObject,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
    ) -> KernelResult<()> {
        if !self.available {
            return Err(KernelError::NotSupported);
        }

        let dst_base = crate::virtio::gpu::framebuffer_addr()
            .ok_or(KernelError::NotSupported)?;

        let hhdm = page_table::hhdm().ok_or(KernelError::NotSupported)?;
        let bpp = fb.format.bpp() as usize;
        let dst_pitch = (self.width as usize) * bpp;

        let y_end = (y + h).min(fb.height).min(self.height);
        let x_start = x.min(fb.width).min(self.width) as usize;
        let copy_w_bytes = (w as usize) * bpp;

        for row in (y as usize)..(y_end as usize) {
            let src_byte_offset = row * (fb.pitch as usize) + x_start * bpp;
            let frame_idx = src_byte_offset / FRAME_SIZE;
            let frame_offset = src_byte_offset % FRAME_SIZE;

            if let Some(pf) = gem.phys_frames.get(frame_idx) {
                let src_virt = pf.addr() + hhdm + (frame_offset as u64);
                let dst_row = dst_base + (row * dst_pitch + x_start * bpp) as u64;

                let avail = FRAME_SIZE - frame_offset;
                let to_copy = copy_w_bytes.min(avail);

                // SAFETY: Both src and dst point to valid mapped memory.
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        src_virt as *const u8,
                        dst_row as *mut u8,
                        to_copy,
                    );
                }
            }
        }

        crate::virtio::gpu::flush_rect(x, y, w, h)
    }
}
