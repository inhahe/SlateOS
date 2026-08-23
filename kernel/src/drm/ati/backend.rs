//! The ATI card as a DRM device: the pieces of this module wired into
//! [`crate::drm`]'s backend enum.
//!
//! ## What this adds over the pieces it is made of
//!
//! [`super::modeset`], [`super::vram`] and [`super::aperture`] are each usable
//! on their own, and the boot-time exercise in [`super`] drives them directly.
//! What they do not have between them is *state that outlives a function*: the
//! aperture must stay mapped, the allocator must remember what it handed out,
//! and the CRTC must remember what mode it is in. This type is that state, and
//! it is what makes the difference between a driver that can demonstrate a
//! mode-set and one the rest of the system can allocate buffers from.
//!
//! ## Why this backend is worth having even though nothing selects it
//!
//! On the QEMU line this project boots, virtio-gpu is the primary display and
//! this card is an unused second head. So this backend is registered and then,
//! in practice, not composited to. That is deliberate rather than a shortfall:
//! it means the whole path — enumerate, allocate, flip — is exercised on every
//! boot against a real register interface, without any of it being load-bearing
//! for the machine's only screen. A driver first exercised on the day it
//! becomes the primary display is a driver first debugged on a black screen.
//!
//! ## The one structural difference from the other backends
//!
//! [`super::super::driver::LimineBackend`] and the virtio-gpu backend allocate
//! GEM objects from the buddy allocator and page-flip by copying pixels into
//! the display's memory. This one allocates *from the card*, and its page flip
//! is a single register write — no copy at any resolution. That is the whole
//! reason a real GPU driver is faster than a framebuffer shim, and it is also
//! why [`GemBacking`] had to become an enum: these objects have no
//! [`crate::mm::frame::PhysFrame`]s, and freeing them through the system frame
//! allocator would be corruption rather than a leak.

extern crate alloc;
use alloc::vec;
use alloc::vec::Vec;

use crate::error::{KernelError, KernelResult};
use crate::serial_println;

use super::super::DrmObjectId;
use super::super::connector::{ConnectorStatus, ConnectorType, DrmConnector};
use super::super::crtc::DrmCrtc;
use super::super::driver::DrmObjectSet;
use super::super::encoder::{DrmEncoder, EncoderType};
use super::super::framebuffer::DrmFramebuffer;
use super::super::gem::{GemBacking, GemObject};
use super::super::mode::{DrmMode, DrmModeFlags, PixelFormat};
use super::super::plane::{DrmPlane, PlaneType};

use super::aperture::VramAperture;
use super::mmio::AtiDevice;
use super::modeset::{self, ModeSetPlan, SCANOUT_ALIGN};
use super::timing::{self, DMT_MODES, ModeTiming};
use super::vram::VramAllocator;

/// A legacy ATI display card, presented as a DRM device.
pub struct AtiBackend {
    /// The probed card: register aperture, reported VRAM, BAR0 base.
    dev: AtiDevice,
    /// The mapped BAR0 window. Owned here because it must outlive every
    /// buffer allocated out of it, and because there must be exactly one —
    /// see [`VramAperture::map`]'s safety contract.
    ap: VramAperture,
    /// Suballocation of the mapped window.
    ///
    /// Sized from the aperture's mapped length, never the card's reported VRAM,
    /// so it cannot hand out an offset the CPU is unable to reach.
    pool: VramAllocator,
    /// Whether this card holds the console framebuffer.
    ///
    /// Latched at construction rather than asked each time. A card in this
    /// state is enumerated and can have buffers allocated from it, but is never
    /// retimed and never flipped: doing either would blank the operator's only
    /// screen, and the explanation would land in a framebuffer nobody can read.
    owns_console: bool,
    /// The timing currently programmed, if any.
    ///
    /// Kept so a page flip can tell "the CRTC is already in the right mode, so
    /// this is one register write" from "the CRTC is in no mode at all, or the
    /// wrong one, so it must be retimed first". Getting that wrong in the
    /// cheap direction means flipping to a buffer the CRTC reads with the
    /// previous mode's stride, which is a diagonally-sheared picture.
    mode: Option<ModeTiming>,
    /// VRAM offset the CRTC was last pointed at.
    scanout: Option<u32>,
}

impl AtiBackend {
    /// Map the card's video memory and take ownership of it.
    ///
    /// # Errors
    ///
    /// Propagates [`VramAperture::map`] — chiefly `InvalidArgument` for a BAR0
    /// that config space reports as unassigned, and `NotSupported` if nothing
    /// is mapped at the aperture base afterwards.
    ///
    /// # Safety
    ///
    /// `dev.vram_phys` must be this card's BAR0 and must not already be mapped
    /// by another [`VramAperture`]. Both hold when this is called once from
    /// [`super::probe_hardware`], which is the only caller.
    pub unsafe fn new(dev: AtiDevice) -> KernelResult<Self> {
        // SAFETY: forwarded from this function's own contract — `vram_phys` is
        // BAR0 as the device's config space reports it, so it names device
        // memory rather than RAM, and this is the only aperture over it.
        let ap = unsafe { VramAperture::map(dev.vram_phys, u64::from(dev.vram_bytes))? };
        let total = u32::try_from(ap.len()).map_err(|_| KernelError::InvalidArgument)?;
        let pool = VramAllocator::new(total)?;
        let owns_console = dev.owns_console();
        Ok(Self {
            dev,
            ap,
            pool,
            owns_console,
            mode: None,
            scanout: None,
        })
    }

    /// Driver name, as it appears in the DRM device list.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        "ati"
    }

    /// The mapped aperture, for the boot-time display exercise.
    #[must_use]
    pub const fn aperture(&self) -> &VramAperture {
        &self.ap
    }

    /// The card, for the boot-time display exercise.
    #[must_use]
    pub const fn device(&self) -> &AtiDevice {
        &self.dev
    }

    /// The VRAM pool, for the boot-time display exercise.
    pub const fn pool_mut(&mut self) -> &mut VramAllocator {
        &mut self.pool
    }

    /// Whether this card holds the console framebuffer, and so must not be
    /// retimed. See the field of the same name.
    #[must_use]
    pub const fn owns_console(&self) -> bool {
        self.owns_console
    }

    /// Present the card's display objects: one connector, CRTC, plane and
    /// encoder.
    ///
    /// The mode list is [`DMT_MODES`] filtered to those whose scanout buffer
    /// fits in the mapped aperture. Filtering here rather than at mode-set time
    /// is the difference between a mode list that is a menu and one that is a
    /// suggestion: a compositor that picks the largest advertised mode and is
    /// then refused has no good move left, whereas one that is never offered
    /// 1920x1080 on a card that cannot scan it out simply picks the next.
    ///
    /// The connector is a VGA one driven by a DAC, which is what these parts
    /// actually have and what QEMU's `ati-vga` presents. It is reported
    /// `Connected` unconditionally: load-detecting a VGA DAC means driving the
    /// output and reading a comparator back, which is a mode-set in disguise
    /// and not something to do during enumeration.
    ///
    /// # Errors
    ///
    /// `NotSupported` if no DMT mode fits in the mapped aperture, which means
    /// a card with under 1.2 MB of usable video memory.
    pub fn enumerate(&mut self, alloc_id: &dyn Fn() -> DrmObjectId) -> KernelResult<DrmObjectSet> {
        let encoder_id = alloc_id();
        let crtc_id = alloc_id();
        let plane_id = alloc_id();
        let connector_id = alloc_id();

        let total = u32::try_from(self.ap.len()).map_err(|_| KernelError::InvalidArgument)?;
        let mut modes: Vec<DrmMode> = Vec::new();
        for dmt in DMT_MODES {
            // `ModeSetPlan::new` is the authority on whether a mode is settable
            // at all — it also rejects timings this CRTC cannot encode, not
            // merely ones that do not fit. Asking it here means the mode list
            // cannot drift away from what `apply` will accept.
            if ModeSetPlan::new(&dmt.timing, PixelFormat::Xrgb8888, 0, total).is_ok() {
                let mut mode = DrmMode::from_resolution(dmt.width, dmt.height, dmt.refresh);
                mode.clock = dmt.timing.clock_khz;
                mode.htotal = dmt.timing.htotal;
                mode.vtotal = dmt.timing.vtotal;
                modes.push(mode);
            }
        }
        if modes.is_empty() {
            return Err(KernelError::NotSupported);
        }
        // The largest mode that fits is the preferred one. `DMT_MODES` is
        // ordered by area, so that is the last survivor.
        if let Some(last) = modes.last_mut() {
            last.flags = DrmModeFlags::PREFERRED;
        }
        let (pref_w, pref_h) = modes.last().map_or((0, 0), |m| (m.hdisplay, m.vdisplay));

        let connector = DrmConnector {
            id: connector_id,
            connector_type: ConnectorType::Vga,
            status: ConnectorStatus::Connected,
            modes,
            current_encoder: Some(encoder_id),
            possible_encoders: vec![encoder_id],
            // No EDID: reading one needs the DDC lines bit-banged through
            // GPIO registers this driver does not touch yet. Absent is the
            // honest answer; a fabricated one would be believed.
            edid: None,
        };

        let plane = DrmPlane {
            id: plane_id,
            plane_type: PlaneType::Primary,
            possible_crtcs: 1,
            // Only the formats `PixWidth::from_format` can encode. Advertising
            // a format the CRTC has no depth encoding for would put the refusal
            // at mode-set time, which is far too late.
            formats: vec![PixelFormat::Xrgb8888, PixelFormat::Argb8888],
            fb: None,
            crtc: Some(crtc_id),
            src_x: 0,
            src_y: 0,
            src_w: pref_w,
            src_h: pref_h,
            dst_x: 0,
            dst_y: 0,
            dst_w: pref_w,
            dst_h: pref_h,
        };

        let crtc = DrmCrtc {
            id: crtc_id,
            // Not active: nothing has been scanned out yet, and claiming
            // otherwise would have a compositor believe a flip is all that is
            // needed when in fact the CRTC has never been timed.
            active: false,
            mode: None,
            primary_plane: plane_id,
            cursor_plane: None,
            // The hardware LUT is not programmed by this driver, so zero
            // rather than 256: a compositor told there are 256 gamma slots
            // will write them and see no change.
            gamma_size: 0,
            index: 0,
        };

        let encoder = DrmEncoder {
            id: encoder_id,
            encoder_type: EncoderType::Dac,
            crtc: Some(crtc_id),
            possible_crtcs: 1,
        };

        Ok((vec![connector], vec![crtc], vec![plane], vec![encoder]))
    }

    /// Allocate a scanout-capable buffer in the card's own video memory.
    ///
    /// The stride is `width * bpp` with no padding, which is *not* what
    /// [`PixelFormat::pitch`] returns. That helper rounds up to 64 bytes for
    /// cache-line reasons, and this hardware counts stride in 8-pixel
    /// characters — so a byte-padded stride is not generally a whole number of
    /// characters and would be unencodable. Matching [`ModeSetPlan`] exactly is
    /// what keeps a buffer allocated here settable on this CRTC.
    ///
    /// The buffer is zeroed. That costs a full uncached pass over the pixels
    /// and is worth it anyway: video memory holds whatever the last user of the
    /// card left there, and a framebuffer handed out with someone else's image
    /// in it is an information leak with a picture of itself.
    ///
    /// # Errors
    ///
    /// - `NotSupported` for a format this CRTC has no depth encoding for.
    /// - `InvalidArgument` for a degenerate size, or one whose stride
    ///   overflows.
    /// - `OutOfMemory` if the card has no free run large enough.
    pub fn gem_create(
        &mut self,
        alloc_id: &dyn Fn() -> DrmObjectId,
        width: u32,
        height: u32,
        format: PixelFormat,
    ) -> KernelResult<GemObject> {
        // Refuse formats the CRTC cannot scan out, at allocation rather than at
        // mode-set: every buffer this backend hands out is video memory, which
        // is scarce and exists to be scanned out. There is no point holding a
        // megabyte of it in a layout the display engine cannot read.
        super::regs::PixWidth::from_format(format)?;
        if width == 0 || height == 0 {
            return Err(KernelError::InvalidArgument);
        }

        let pitch = width
            .checked_mul(format.bpp())
            .ok_or(KernelError::InvalidArgument)?;
        let size = pitch
            .checked_mul(height)
            .ok_or(KernelError::InvalidArgument)?;

        let offset = self.pool.alloc(size, SCANOUT_ALIGN)?;
        // Zero before the object exists, so a failure here cannot leave a
        // handle to unzeroed video memory in circulation.
        if let Err(e) = self.zero(offset, size) {
            // Give the run back; the caller gets the original error, not a
            // second one about the card slowly filling up.
            let _ = self.pool.free(offset, size);
            return Err(e);
        }

        match GemObject::from_vram(alloc_id, offset, size, width, height, pitch, format) {
            Ok(gem) => Ok(gem),
            Err(e) => {
                let _ = self.pool.free(offset, size);
                Err(e)
            }
        }
    }

    /// Return a buffer's video memory to the pool.
    ///
    /// Deliberately does *not* call [`GemObject::free_backing`], which would
    /// hand BAR addresses to the buddy allocator. That is the hazard this
    /// backend exists to avoid, and it is why the object carries
    /// [`GemBacking::Vram`] rather than a frame list.
    ///
    /// # Errors
    ///
    /// - `NotSupported` if handed a system-RAM object, which would mean an
    ///   object created by another backend reached this device's list.
    /// - `DeviceBusy` if the CRTC is currently scanning the buffer out.
    /// - `AlreadyExists` from the pool if the run is already free, which is a
    ///   double free — reported rather than absorbed, because the second free
    ///   would otherwise make the run available twice over.
    pub fn gem_destroy(&mut self, gem: GemObject) -> KernelResult<()> {
        match gem.backing {
            GemBacking::Vram { offset, len } => {
                // Refuse to release a buffer the CRTC is still reading. The
                // pool would happily hand the run straight out again, and the
                // next allocation would be painted underneath the display.
                if self.scanout == Some(offset) {
                    return Err(KernelError::DeviceBusy);
                }
                self.pool.free(offset, len)
            }
            GemBacking::SystemRam(_) => Err(KernelError::NotSupported),
        }
    }

    /// A kernel-virtual pointer to a VRAM buffer's first byte.
    ///
    /// Unlike the system-RAM backends, this pointer covers the *whole* buffer:
    /// video memory is one contiguous run, so there are no frame boundaries for
    /// the caller to walk. It points at uncacheable memory, so writes through
    /// it reach the card in order and reads through it are slow.
    ///
    /// # Errors
    ///
    /// `NotSupported` for a system-RAM object; `InvalidArgument` if the
    /// object's run is not inside the mapping, which would mean the pool and
    /// the aperture disagree about how large the card is.
    pub fn gem_mmap(&self, gem: &GemObject) -> KernelResult<*mut u8> {
        match gem.backing {
            GemBacking::Vram { offset, len } => self.ap.ptr_at(offset, len),
            GemBacking::SystemRam(_) => Err(KernelError::NotSupported),
        }
    }

    /// Retime the CRTC to `mode` and scan `fb` out of it.
    ///
    /// This is the driver's only entry point that changes a timing.
    /// [`Self::page_flip`] used to carry an implicit mode-set — it looked the
    /// framebuffer's size up in [`DMT_MODES`] and retimed if the answer differed
    /// from [`Self::mode`] — and that was removed on 2026-08-21 in favour of
    /// this. The reason is not tidiness: a page flip that silently changes the
    /// resolution is indistinguishable, to the client issuing it, from one that
    /// does not, and the other two backends answered the same call by cropping
    /// and by doing nothing. See `design-decisions.md` §270.
    ///
    /// The mode is validated by [`ModeSetPlan::new`], which is the same
    /// authority [`Self::enumerate`] filters the advertised mode list through —
    /// so a mode that came out of this connector's list and fits the buffer
    /// cannot be refused here.
    ///
    /// # Errors
    ///
    /// - `NotSupported` if this card holds the console (see
    ///   [`Self::owns_console`]), or if the object is not VRAM-resident.
    /// - `NotFound` if `mode` matches no timing this driver knows. Refused
    ///   rather than approximated: a CRTC programmed with a timing that merely
    ///   resembles the one the monitor expects is the failure that looks like a
    ///   hang.
    /// - `InvalidArgument` if the framebuffer is not the mode's size — the
    ///   CRTC would read it with the wrong stride, which is a diagonally
    ///   sheared picture.
    /// - Propagates [`modeset::apply`] and [`modeset::verify_applied`].
    pub fn set_mode(
        &mut self,
        _crtc_id: DrmObjectId,
        mode: &DrmMode,
        fb: &DrmFramebuffer,
        gem: &GemObject,
    ) -> KernelResult<()> {
        if self.owns_console {
            return Err(KernelError::NotSupported);
        }
        let GemBacking::Vram { offset, len } = gem.backing else {
            return Err(KernelError::NotSupported);
        };
        if fb.width != mode.hdisplay || fb.height != mode.vdisplay {
            return Err(KernelError::InvalidArgument);
        }
        let want = timing::lookup(mode.hdisplay, mode.vdisplay, mode.vrefresh)
            .ok_or(KernelError::NotFound)?;

        // Order matters: the buffer's contents must be in memory before the
        // CRTC is pointed at it, and the CRTC is pointed at it by `apply`.
        self.ap.flush(offset, len)?;

        let total = u32::try_from(self.ap.len()).map_err(|_| KernelError::InvalidArgument)?;
        let plan = ModeSetPlan::new(want, fb.format, offset, total)?;
        modeset::apply(&self.dev.mmio, &plan)?;
        modeset::verify_applied(&self.dev.mmio, &plan)?;
        // Only recorded once the hardware has been read back and agrees. A
        // `self.mode` set from the plan rather than from the registers would
        // make a failed mode-set look like a successful one to the very next
        // page flip, which would then take the one-register-write path.
        self.mode = Some(*want);
        self.scanout = Some(offset);
        Ok(())
    }

    /// Stop scanning out and blank the output.
    ///
    /// Clears [`Self::mode`] as well as the registers, so the next flip cannot
    /// take the "already in the right mode" path against a CRTC that is off.
    /// [`Self::scanout`] is cleared too, which is what releases
    /// [`Self::gem_destroy`]'s `DeviceBusy` hold on the buffer that was being
    /// displayed — a compositor shutting down wants to free its buffers
    /// immediately after turning the CRTC off, and would otherwise be refused.
    ///
    /// # Errors
    ///
    /// `NotSupported` if this card holds the console — blanking it would take
    /// the operator's only screen. Otherwise propagates [`modeset::disable`].
    pub fn disable_crtc(&mut self, _crtc_id: DrmObjectId) -> KernelResult<()> {
        if self.owns_console {
            return Err(KernelError::NotSupported);
        }
        modeset::disable(&self.dev.mmio)?;
        self.mode = None;
        self.scanout = None;
        Ok(())
    }

    /// Display a buffer in the mode the CRTC is already in: one register write.
    ///
    /// Deliberately will **not** retime. A framebuffer whose size differs from
    /// the programmed mode is refused rather than accommodated, and a CRTC that
    /// has never been timed is refused rather than timed here — use
    /// [`Self::set_mode`] for both. [`super::super::DrmDevice::page_flip`]
    /// makes the same check against its own object model before calling this,
    /// so reaching either refusal means the driver's private state and the DRM
    /// object model have diverged; the check is kept anyway, because the cost
    /// of being wrong is the CRTC reading a buffer with the previous mode's
    /// stride.
    ///
    /// # Errors
    ///
    /// - `NotSupported` if this card holds the console (see
    ///   [`Self::owns_console`]), or if the object is not VRAM-resident.
    /// - `InvalidArgument` if the CRTC is in no mode, or if `fb` is not the
    ///   size of the mode it is in.
    /// - Propagates [`modeset::page_flip`].
    pub fn page_flip(
        &mut self,
        _crtc_id: DrmObjectId,
        fb: &DrmFramebuffer,
        gem: &GemObject,
    ) -> KernelResult<()> {
        if self.owns_console {
            return Err(KernelError::NotSupported);
        }
        let GemBacking::Vram { offset, len } = gem.backing else {
            return Err(KernelError::NotSupported);
        };
        let Some(cur) = self.mode else {
            return Err(KernelError::InvalidArgument);
        };
        if fb.width != cur.hdisplay || fb.height != cur.vdisplay {
            return Err(KernelError::InvalidArgument);
        }

        // Everything written through the aperture must be in memory before the
        // CRTC is pointed at it. A no-op on an uncacheable mapping, and not one
        // if the mapping turned out cacheable — which is exactly the case the
        // aperture refuses to assume away.
        self.ap.flush(offset, len)?;

        modeset::page_flip(&self.dev.mmio, offset)?;
        self.scanout = Some(offset);
        Ok(())
    }

    /// Make a sub-rectangle of a buffer visible.
    ///
    /// There is nothing to copy: the CRTC scans this buffer out of video memory
    /// directly, so pixels written through the aperture are already where the
    /// display engine reads them. All this does is order those writes ahead of
    /// whatever the caller does next, which on an uncacheable mapping is a
    /// store fence and on a cacheable one is a write-back of the affected
    /// lines.
    ///
    /// The whole rows spanning the rectangle are flushed rather than the
    /// rectangle itself. Cache lines do not respect a rectangle's left edge,
    /// and flushing a superset is slower but cannot miss a line; flushing a
    /// subset would lose the pixels either side of it.
    ///
    /// # Errors
    ///
    /// `NotSupported` for a system-RAM object; `InvalidArgument` if the
    /// rectangle runs outside the object.
    pub fn flush_region(
        &mut self,
        fb: &DrmFramebuffer,
        gem: &GemObject,
        _x: u32,
        y: u32,
        _w: u32,
        h: u32,
    ) -> KernelResult<()> {
        let GemBacking::Vram { offset, len } = gem.backing else {
            return Err(KernelError::NotSupported);
        };
        let start = y
            .checked_mul(fb.pitch)
            .and_then(|o| offset.checked_add(o))
            .ok_or(KernelError::InvalidArgument)?;
        let span = h
            .checked_mul(fb.pitch)
            .ok_or(KernelError::InvalidArgument)?;
        // Clamp to the object: a rectangle taller than the framebuffer is the
        // caller's error, but flushing past the end of the buffer would be
        // this driver's.
        let end_of_obj = offset
            .checked_add(len)
            .ok_or(KernelError::InvalidArgument)?;
        if start > end_of_obj {
            return Err(KernelError::InvalidArgument);
        }
        let span = span.min(end_of_obj.saturating_sub(start));
        self.ap.flush(start, span)
    }

    /// Write zeroes over a run of video memory.
    ///
    /// Word at a time through [`VramAperture::fill32`] rather than
    /// `write_bytes`: the aperture is uncacheable device memory, and the
    /// compiler is entitled to lower a byte-wise `memset` into whatever access
    /// widths it likes — including ones the card's memory controller handles
    /// differently from the 32-bit accesses everything else here uses.
    fn zero(&self, offset: u32, len: u32) -> KernelResult<()> {
        let words = len.checked_div(4).ok_or(KernelError::InvalidArgument)?;
        self.ap.fill32(offset, words, 0)?;
        self.ap.flush(offset, len)
    }

    /// Log what the card looks like once it has been taken over.
    pub fn report(&self) {
        serial_println!(
            "[ati]   backend ready: {} MiB aperture at {:#x}, {} B free",
            self.ap.len() / (1024 * 1024),
            self.ap.phys(),
            self.pool.free_bytes()
        );
    }
}
