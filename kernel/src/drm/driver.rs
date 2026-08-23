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
use crate::mm::frame::{FRAME_SIZE, PhysFrame};
use crate::mm::page_table;

use super::DrmObjectId;
use super::connector::{ConnectorStatus, ConnectorType, DrmConnector};
use super::crtc::DrmCrtc;
use super::encoder::{DrmEncoder, EncoderType};
use super::framebuffer::DrmFramebuffer;
use super::gem::GemObject;
use super::mode::{DrmMode, DrmModeFlags, PixelFormat};
use super::plane::{DrmPlane, PlaneType};

/// The full set of DRM objects a driver exposes for one device: connectors,
/// CRTCs, planes, and encoders.
pub type DrmObjectSet = (
    Vec<DrmConnector>,
    Vec<DrmCrtc>,
    Vec<DrmPlane>,
    Vec<DrmEncoder>,
);

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
    fn enumerate(&mut self, alloc_id: &dyn Fn() -> DrmObjectId) -> KernelResult<DrmObjectSet>;

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

    /// Program a display mode on a CRTC and scan `fb` out of it.
    ///
    /// A backend that cannot retime must still implement this, and must refuse
    /// with `InvalidArgument` any mode other than the one it has — never
    /// succeed while continuing to scan out the old one, which a client cannot
    /// distinguish from a real mode change.
    fn set_mode(
        &mut self,
        crtc_id: DrmObjectId,
        mode: &DrmMode,
        fb: &DrmFramebuffer,
        gem: &GemObject,
    ) -> KernelResult<()>;

    /// Stop scanning out on a CRTC.
    ///
    /// Must be idempotent: disabling a CRTC that is already off is how a
    /// client that lost track of its own state gets back to a known one.
    fn disable_crtc(&mut self, crtc_id: DrmObjectId) -> KernelResult<()>;

    /// Swap the displayed framebuffer on a CRTC, within the mode it is in.
    ///
    /// Must not retime. `fb` is guaranteed by [`super::DrmDevice::page_flip`]
    /// to be exactly the size of the CRTC's programmed mode.
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
    pub fn enumerate(&mut self, alloc_id: &dyn Fn() -> DrmObjectId) -> KernelResult<DrmObjectSet> {
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
        gem.free_backing()
    }

    /// Get kernel-virtual pointer to GEM memory.
    pub fn gem_mmap(&self, gem: &GemObject) -> KernelResult<*mut u8> {
        gem.virt_addr()
    }

    /// Total addressable bytes of the firmware framebuffer.
    ///
    /// Unlike the virtio-gpu scanout this region really is linear — the
    /// bootloader programmed the display engine to scan a contiguous aperture
    /// — so a byte offset is all a blit needs. What it still needs is an
    /// *end*, which is what this provides.
    fn fb_len(&self) -> usize {
        (self.pitch as usize).saturating_mul(self.height as usize)
    }

    /// Page flip: copy GEM backing to the Limine framebuffer.
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
        // These backends only ever create system-RAM objects, so a VRAM-resident
        // one arriving here means it came from another driver's device — refused
        // rather than misread, since its "frames" would be BAR addresses.
        let frames = gem.ram_frames()?;

        let dst = self.fb_addr as *mut u8;
        let dst_len = self.fb_len();
        let dst_pitch = self.pitch as usize;
        let copy_h = fb.height.min(self.height) as usize;
        let row_bytes =
            (fb.width.min(self.width) as usize).saturating_mul(fb.format.bpp() as usize);

        for row in 0..copy_h {
            // SAFETY: `frames` are HHDM-mapped and owned by this GEM object;
            // `dst` is the bootloader framebuffer mapped at init and is a
            // different region, so the two cannot alias.
            unsafe {
                blit_run_flat(
                    dst,
                    dst_len,
                    frames,
                    hhdm,
                    row.saturating_mul(fb.pitch as usize),
                    row.saturating_mul(dst_pitch),
                    row_bytes,
                );
            }
        }
        Ok(())
    }

    /// Program a display mode on this CRTC.
    ///
    /// A firmware framebuffer has exactly one timing — the one the bootloader
    /// left the hardware in — and this driver has no register access with which
    /// to change it. So the only mode it can accept is the one it already has,
    /// and any other is refused.
    ///
    /// The refusal is the point. The alternative is to accept the request and
    /// carry on scanning out the old size, which a client cannot distinguish
    /// from success and which leaves it drawing at a resolution the display
    /// does not have.
    ///
    /// # Errors
    ///
    /// `InvalidArgument` if `mode` is not the boot mode; otherwise propagates
    /// [`Self::page_flip`], which makes `fb` visible.
    pub fn set_mode(
        &mut self,
        crtc_id: DrmObjectId,
        mode: &DrmMode,
        fb: &DrmFramebuffer,
        gem: &GemObject,
    ) -> KernelResult<()> {
        if mode.hdisplay != self.width || mode.vdisplay != self.height {
            return Err(KernelError::InvalidArgument);
        }
        self.page_flip(crtc_id, fb, gem)
    }

    /// Turn this CRTC off.
    ///
    /// Deliberately a no-op that reports success. There is no register to
    /// clear: the display is driven by whatever the firmware programmed, and
    /// this driver can only write pixels into it. Blanking it by zeroing the
    /// framebuffer was considered and rejected — that surface is shared with
    /// the kernel console, so "the compositor shut down cleanly" would erase
    /// the panic output that explains why.
    ///
    /// The DRM object model still records the CRTC as inactive with no mode,
    /// so a client asking what is configured gets the truth; what it cannot
    /// have is a dark screen.
    ///
    /// # Errors
    ///
    /// None; the signature matches the other backends'.
    #[allow(clippy::unnecessary_wraps)]
    pub fn disable_crtc(&mut self, _crtc_id: DrmObjectId) -> KernelResult<()> {
        Ok(())
    }

    /// Flush region: same as page_flip but for a sub-rectangle.
    ///
    /// The rectangle is clamped on every side against both the source
    /// framebuffer and the display, so a caller that asks for more than either
    /// holds gets the intersection rather than a copy that runs off the end of
    /// one of them. The right-edge clamp in particular is not cosmetic: without
    /// it an over-wide rectangle on the last row wrote past the end of the
    /// firmware framebuffer.
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
        // These backends only ever create system-RAM objects, so a VRAM-resident
        // one arriving here means it came from another driver's device — refused
        // rather than misread, since its "frames" would be BAR addresses.
        let frames = gem.ram_frames()?;

        let dst = self.fb_addr as *mut u8;
        let dst_len = self.fb_len();
        let dst_pitch = self.pitch as usize;
        let bpp = fb.format.bpp() as usize;

        let y_end = y.saturating_add(h).min(fb.height).min(self.height);
        let x_start = x.min(fb.width).min(self.width) as usize;
        // Right edge: never past the narrower of source and display.
        let max_w = (fb.width.min(self.width) as usize).saturating_sub(x_start);
        let row_bytes = (w as usize).min(max_w).saturating_mul(bpp);

        for row in (y as usize)..(y_end as usize) {
            // SAFETY: `frames` are HHDM-mapped and owned by this GEM object;
            // `dst` is the bootloader framebuffer mapped at init and is a
            // different region, so the two cannot alias. `blit_run_flat` bounds
            // both sides itself.
            unsafe {
                blit_run_flat(
                    dst,
                    dst_len,
                    frames,
                    hhdm,
                    row.saturating_mul(fb.pitch as usize)
                        .saturating_add(x_start.saturating_mul(bpp)),
                    row.saturating_mul(dst_pitch)
                        .saturating_add(x_start.saturating_mul(bpp)),
                    row_bytes,
                );
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
    pub fn enumerate(&mut self, alloc_id: &dyn Fn() -> DrmObjectId) -> KernelResult<DrmObjectSet> {
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
        gem.free_backing()
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
    pub fn page_flip(
        &mut self,
        _crtc_id: DrmObjectId,
        fb: &DrmFramebuffer,
        gem: &GemObject,
    ) -> KernelResult<()> {
        if !self.available {
            return Err(KernelError::NotSupported);
        }

        let hhdm = page_table::hhdm().ok_or(KernelError::NotSupported)?;
        // These backends only ever create system-RAM objects, so a VRAM-resident
        // one arriving here means it came from another driver's device — refused
        // rather than misread, since its "frames" would be BAR addresses.
        let frames = gem.ram_frames()?;
        let bpp = fb.format.bpp() as usize;

        crate::virtio::gpu::with_scanout(|sc| {
            let copy_h = fb.height.min(sc.height()) as usize;
            let row_bytes = (fb.width.min(sc.width()) as usize).saturating_mul(bpp);
            let dst_pitch = sc.pitch();
            for row in 0..copy_h {
                // SAFETY: `frames` are HHDM-mapped and owned by this GEM
                // object, and the scanout is a distinct allocation, so source
                // and destination cannot alias.
                unsafe {
                    blit_run(
                        sc,
                        frames,
                        hhdm,
                        row.saturating_mul(fb.pitch as usize),
                        row.saturating_mul(dst_pitch),
                        row_bytes,
                    );
                }
            }
        })
        .ok_or(KernelError::NotSupported)?;

        crate::virtio::gpu::flush_full()
    }

    /// Program a display mode on this CRTC.
    ///
    /// virtio-gpu *can* change resolution — `RESOURCE_CREATE_2D` at the new
    /// size followed by `SET_SCANOUT` — but this driver does not yet do it:
    /// the scanout resource is created once at probe from `GET_DISPLAY_INFO`
    /// and never replaced.  So the only mode this backend can honestly serve
    /// is the one it booted with, which is also the only mode its connector
    /// advertises, and every other request is refused.
    ///
    /// `InvalidArgument` is the honest answer here and is deliberately
    /// preferred over a silent no-op: a client that asked for 1280×720 and
    /// got `Ok(())` would then draw 1280×720 into a 1920×1080 scanout and see
    /// a corrupt quarter-screen with no way to tell what went wrong.
    ///
    /// When the driver grows a resource-recreate path this is the entry point
    /// that gains it; nothing else has to change.
    ///
    /// # Errors
    ///
    /// `NotSupported` if the device is absent, `InvalidArgument` if `mode` is
    /// not the boot mode; otherwise propagates [`Self::page_flip`], which
    /// makes `fb` visible.
    pub fn set_mode(
        &mut self,
        crtc_id: DrmObjectId,
        mode: &DrmMode,
        fb: &DrmFramebuffer,
        gem: &GemObject,
    ) -> KernelResult<()> {
        if !self.available {
            return Err(KernelError::NotSupported);
        }
        if mode.hdisplay != self.width || mode.vdisplay != self.height {
            return Err(KernelError::InvalidArgument);
        }
        self.page_flip(crtc_id, fb, gem)
    }

    /// Turn this CRTC off.
    ///
    /// A no-op that reports success, for the same reason as the Limine
    /// backend: the only thing this driver could do to darken the display is
    /// zero the framebuffer, and that surface is shared with the kernel
    /// console — so "the compositor shut down cleanly" would erase the panic
    /// output explaining why it did.  The DRM object model still records the
    /// CRTC as inactive with no mode, so a client asking what is configured
    /// gets the truth.
    ///
    /// # Errors
    ///
    /// `NotSupported` if the virtio-gpu device is absent.
    pub fn disable_crtc(&mut self, _crtc_id: DrmObjectId) -> KernelResult<()> {
        if !self.available {
            return Err(KernelError::NotSupported);
        }
        Ok(())
    }

    /// Flush a sub-region: bulk memcpy + partial host transfer.
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
        // These backends only ever create system-RAM objects, so a VRAM-resident
        // one arriving here means it came from another driver's device — refused
        // rather than misread, since its "frames" would be BAR addresses.
        let frames = gem.ram_frames()?;
        let bpp = fb.format.bpp() as usize;

        crate::virtio::gpu::with_scanout(|sc| {
            let dst_pitch = sc.pitch();
            let x_start = x.min(fb.width).min(sc.width()) as usize;
            let y_end = y.saturating_add(h).min(fb.height).min(sc.height()) as usize;
            // Clamp the run to the right edge of *both* surfaces. Without this
            // an over-wide rectangle wraps onto the next row's left edge, which
            // is a wrong picture rather than a wrong write — but there is no
            // reason to draw it.
            let max_w = (fb.width.min(sc.width()) as usize).saturating_sub(x_start);
            let row_bytes = (w as usize).min(max_w).saturating_mul(bpp);
            let x_bytes = x_start.saturating_mul(bpp);

            for row in (y as usize)..y_end {
                // SAFETY: as in `page_flip` — the GEM frames are HHDM-mapped
                // and are a different allocation from the scanout.
                unsafe {
                    blit_run(
                        sc,
                        frames,
                        hhdm,
                        row.saturating_mul(fb.pitch as usize)
                            .saturating_add(x_bytes),
                        row.saturating_mul(dst_pitch).saturating_add(x_bytes),
                        row_bytes,
                    );
                }
            }
        })
        .ok_or(KernelError::NotSupported)?;

        crate::virtio::gpu::flush_rect(x, y, w, h)
    }
}

/// Copy `len` bytes out of a GEM object's frame list into the virtio-gpu
/// scanout, at byte offsets `src_off` and `dst_off` respectively.
///
/// Both sides are discontiguous — a GEM buffer is a list of unrelated frames
/// and so is the scanout — and the two frame boundaries do not line up, so the
/// copy is split by the source's boundaries here and by the destination's
/// inside [`ScanoutMem::write_at`]. Neither side is assumed to be flat, which
/// is the bug this function exists to make unrepresentable: the previous code
/// handled at most *one* source boundary per row and treated the destination
/// as a single 4 MiB buffer that does not exist.
///
/// A run that reaches past the end of either side is truncated, not wrapped.
///
/// # Safety
///
/// `frames` must be HHDM-mapped through `hhdm` and must not alias the scanout.
#[allow(clippy::arithmetic_side_effects)]
unsafe fn blit_run(
    sc: &crate::virtio::gpu::ScanoutMem<'_>,
    frames: &[PhysFrame],
    hhdm: u64,
    src_off: usize,
    dst_off: usize,
    len: usize,
) {
    let mut done = 0usize;
    while done < len {
        let so = src_off.saturating_add(done);
        let idx = so / FRAME_SIZE;
        let within = so % FRAME_SIZE;
        let Some(pf) = frames.get(idx) else {
            return;
        };
        let chunk = (FRAME_SIZE - within).min(len - done);
        // SAFETY: `within < FRAME_SIZE` and `chunk <= FRAME_SIZE - within`, so
        // the read stays inside frame `idx`, which the caller guarantees is
        // HHDM-mapped. `write_at` bounds the destination itself.
        let written = unsafe {
            sc.write_at(
                dst_off.saturating_add(done),
                (pf.addr() + hhdm + within as u64) as *const u8,
                chunk,
            )
        };
        if written == 0 {
            // Past the end of the scanout; nothing further in this run can land.
            return;
        }
        done += written;
    }
}

/// Copy `len` bytes out of a GEM object's frame list into a *flat* destination
/// of `dst_len` bytes, at byte offsets `src_off` and `dst_off`.
///
/// The counterpart to [`blit_run`] for the firmware framebuffer, whose
/// destination genuinely is one linear aperture. Only the source needs
/// splitting — but it needs splitting *generally*, once per frame boundary the
/// run crosses, not the single crossing the previous code allowed. A row wider
/// than one 16 KiB frame (4096 px at 32bpp) crossed two or more, and every
/// crossing past the first was silently dropped.
///
/// The destination is bounded here rather than trusted, so an over-wide
/// rectangle is truncated at the end of the framebuffer instead of writing into
/// whatever the firmware mapped next. The clamp discards, matching
/// [`crate::virtio::gpu::ScanoutMem::write_at`]: a wrong picture is a reportable
/// bug, memory corruption three subsystems away is not.
///
/// # Safety
///
/// `frames` must be HHDM-mapped through `hhdm`. `dst` must be valid for writes
/// of `dst_len` bytes and must not alias `frames`.
#[allow(clippy::arithmetic_side_effects)]
unsafe fn blit_run_flat(
    dst: *mut u8,
    dst_len: usize,
    frames: &[PhysFrame],
    hhdm: u64,
    src_off: usize,
    dst_off: usize,
    len: usize,
) {
    if dst_off >= dst_len {
        return;
    }
    // Truncate the run at the end of the destination before copying any of it.
    let len = len.min(dst_len - dst_off);
    let mut done = 0usize;
    while done < len {
        let so = src_off.saturating_add(done);
        let idx = so / FRAME_SIZE;
        let within = so % FRAME_SIZE;
        let Some(pf) = frames.get(idx) else {
            // Source exhausted: the GEM object is smaller than the rectangle
            // claims. Stop rather than read whatever follows the frame list.
            return;
        };
        let chunk = (FRAME_SIZE - within).min(len - done);
        // SAFETY: `within < FRAME_SIZE` and `chunk <= FRAME_SIZE - within`, so
        // the read stays inside frame `idx`, which the caller guarantees is
        // HHDM-mapped. `dst_off + done + chunk <= dst_len` because `len` was
        // clamped above, so the write stays inside the destination. The caller
        // guarantees the two regions do not alias.
        unsafe {
            core::ptr::copy_nonoverlapping(
                (pf.addr() + hhdm + within as u64) as *const u8,
                dst.add(dst_off + done),
                chunk,
            );
        }
        done += chunk;
    }
}
