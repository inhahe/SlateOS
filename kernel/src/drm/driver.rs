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
use crate::mm::frame::{self, FRAME_SIZE};
use crate::mm::page_table;
use crate::serial_println;

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

    /// Page flip via virtio-gpu: use the existing driver's flush.
    ///
    /// Since virtio-gpu is paravirtualized, "page flip" means
    /// transferring the GEM buffer contents to the host via
    /// `TRANSFER_TO_HOST_2D` + `RESOURCE_FLUSH`.
    ///
    /// For the initial implementation, we use the existing virtio-gpu
    /// driver's framebuffer (it already has a resource + backing set
    /// up during init).  We copy from our GEM buffer to the virtio-gpu
    /// framebuffer and then flush.
    pub fn page_flip(
        &mut self,
        _crtc_id: DrmObjectId,
        fb: &DrmFramebuffer,
        gem: &GemObject,
    ) -> KernelResult<()> {
        if !self.available {
            return Err(KernelError::NotSupported);
        }

        // Copy GEM buffer to the virtio-gpu's own framebuffer, then flush.
        // The virtio-gpu driver maintains its own backing frames that are
        // registered with the host.  We need to copy into those frames
        // because the host only knows about the driver's resource.
        //
        // TODO: In the future, create a new virtio-gpu resource for each
        // GEM object and SET_SCANOUT to it directly — avoiding the copy.
        let hhdm = page_table::hhdm().ok_or(KernelError::NotSupported)?;

        let bpp = fb.format.bpp() as usize;
        #[allow(clippy::arithmetic_side_effects)]
        let copy_w = fb.width.min(self.width);
        #[allow(clippy::arithmetic_side_effects)]
        let copy_h = fb.height.min(self.height);

        for y in 0..copy_h {
            for x in 0..copy_w {
                #[allow(clippy::arithmetic_side_effects)]
                let src_offset = (y as usize) * (fb.pitch as usize) + (x as usize) * bpp;
                let frame_idx = src_offset / FRAME_SIZE;
                let frame_off = src_offset % FRAME_SIZE;

                if let Some(pf) = gem.phys_frames.get(frame_idx) {
                    let src_ptr = (pf.addr() + hhdm + frame_off as u64) as *const u32;
                    // SAFETY: src_ptr is within a valid HHDM-mapped frame.
                    let pixel = unsafe { src_ptr.read() };
                    crate::virtio::gpu::set_pixel(x, y, pixel);
                }
            }
        }

        crate::virtio::gpu::flush_full();
        Ok(())
    }

    /// Flush a sub-region.
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

        let hhdm = page_table::hhdm().ok_or(KernelError::NotSupported)?;
        let bpp = fb.format.bpp() as usize;

        #[allow(clippy::arithmetic_side_effects)]
        let x_end = (x + w).min(fb.width).min(self.width);
        #[allow(clippy::arithmetic_side_effects)]
        let y_end = (y + h).min(fb.height).min(self.height);

        for py in y..y_end {
            for px in x..x_end {
                #[allow(clippy::arithmetic_side_effects)]
                let src_offset = (py as usize) * (fb.pitch as usize) + (px as usize) * bpp;
                let frame_idx = src_offset / FRAME_SIZE;
                let frame_off = src_offset % FRAME_SIZE;

                if let Some(pf) = gem.phys_frames.get(frame_idx) {
                    let src_ptr = (pf.addr() + hhdm + frame_off as u64) as *const u32;
                    // SAFETY: src_ptr is within a valid HHDM-mapped frame.
                    let pixel = unsafe { src_ptr.read() };
                    crate::virtio::gpu::set_pixel(px, py, pixel);
                }
            }
        }

        crate::virtio::gpu::flush_rect(x, y, w, h);
        Ok(())
    }
}
