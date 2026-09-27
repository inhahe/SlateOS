//! Showing a CRTC's planes as they were asked for: design-decisions §976.
//!
//! A DRM plane takes a source rectangle out of its framebuffer and shows it at
//! a destination rectangle on the display. Until 2026-09-27 the kernel stored
//! both rectangles, answered success, and scanned out the primary plane's
//! framebuffer whole and unmoved -- and drew no cursor at all, though
//! `cursor_set`/`cursor_move` recorded one. The operator's answer to A-Q17 was
//! to honour them: every layer is drawn where it was asked, at the size it was
//! asked, or the request is refused. Never accepted and ignored.
//!
//! This module turns a CRTC's state into the layers `planecompose` draws:
//!
//! * the primary plane, opaque, with its rectangles;
//! * any overlay plane bound to the CRTC, then its cursor plane, blended as
//!   premultiplied alpha when their format carries alpha;
//! * the legacy cursor (`cursor_set`), on top, placed by its hot spot.
//!
//! The arithmetic is in the `planecompose` crate, where every pixel of it is
//! tested on the host; what is here is the kernel's side: turning GEM frame
//! lists into the paged views that crate draws from, and the scene rules. The
//! backends supply the scanout (`driver.rs`).
//!
//! A *trivial* scene -- only the primary plane, whole and unmoved, the size of
//! the mode -- keeps the old row-copy page flip, which is the one every client
//! uses today and costs one copy.

extern crate alloc;
use alloc::vec::Vec;

use planecompose::{Blend, FixedRect, Limits, Rect};

use crate::error::{KernelError, KernelResult};
use crate::mm::frame::FRAME_SIZE;

use super::DrmObjectId;
use super::gem::GemObject;
use super::mode::PixelFormat;
use super::plane::{DrmPlane, PlaneType};

/// One layer of a CRTC's scene, by the ids of what it draws from, so building
/// a scene borrows nothing past its own construction.
#[derive(Clone, Copy, Debug)]
pub struct SceneLayer {
    /// Where the pixels come from.
    pub source: LayerSource,
    /// The plane this layer is, whose format list its framebuffer must be
    /// in; `None` for the legacy cursor, which is no plane.
    pub plane: Option<DrmObjectId>,
    /// The source rectangle, 16.16.
    pub src: FixedRect,
    /// Where on the display, in whole pixels.
    pub dst: Rect,
    pub blend: Blend,
}

/// What a layer's pixels are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayerSource {
    /// A framebuffer object.
    Framebuffer(DrmObjectId),
    /// The legacy cursor: a bare GEM buffer holding a `width` x `height`
    /// ARGB image, its rows `GemObject::pitch` apart.
    Gem {
        handle: u32,
        width: u32,
        height: u32,
    },
}

/// A CRTC's scene: the display's extent and its layers, bottom first.
#[derive(Debug)]
pub struct Scene {
    pub screen: Rect,
    pub layers: Vec<SceneLayer>,
    /// Only the primary plane, whole, unmoved and the size of the mode: the
    /// old page flip shows it exactly, with one copy.
    pub trivial: bool,
}

/// The most any plane may ask for: the software compositor's limits, which
/// are the most any backend here can show. A request beyond them is malformed
/// on every backend (`InvalidArgument`). Whether *this* backend can show a
/// request within them is a separate question -- one [`DrmDevice::validate_scene`]
/// answers, with `NotSupported` -- so that a client can tell "never valid"
/// from "not here".
///
/// [`DrmDevice::validate_scene`]: super::DrmDevice::validate_scene
pub const LIMITS: Limits = Limits::SOFTWARE;

/// Whether a framebuffer format can be composed: the 32-bit formats laid out
/// like the scanout (`B, G, R, X/A`). Every plane on today's backends lists
/// only these, so [`DrmDevice::validate_scene`] refuses any other format
/// first, as one its plane does not take. This is the composer's own guard,
/// for a backend whose planes list more -- a hardware plane that can scan an
/// RGB565 buffer out whole still cannot have it composed.
///
/// [`DrmDevice::validate_scene`]: super::DrmDevice::validate_scene
#[must_use]
pub fn composable(format: PixelFormat) -> bool {
    matches!(format, PixelFormat::Xrgb8888 | PixelFormat::Argb8888)
}

/// How a plane of this type and format blends. The primary plane is the
/// bottom of the stack, so it is opaque; the rest blend when they have alpha.
#[must_use]
pub fn blend_for(plane_type: PlaneType, format: PixelFormat) -> Blend {
    if plane_type != PlaneType::Primary && format == PixelFormat::Argb8888 {
        Blend::Premultiplied
    } else {
        Blend::Opaque
    }
}

/// A plane's rectangles as `planecompose` takes them. This ABI carries whole
/// pixels (the native atomic syscall packs 16-bit integers), so the source is
/// shifted into 16.16 here.
///
/// # Errors
///
/// `InvalidArgument` for a source value beyond [`planecompose::MAX_DIMENSION`]
/// -- refused, never narrowed to fit.
pub fn plane_rects(p: &DrmPlane) -> KernelResult<(FixedRect, Rect)> {
    Ok((
        FixedRect::pixels(p.src_x, p.src_y, p.src_w, p.src_h).map_err(map_err)?,
        Rect::new(p.dst_x, p.dst_y, p.dst_w, p.dst_h),
    ))
}

/// Pixels in `r`, for comparing the cost of redrawing rectangles.
fn area(r: Rect) -> u64 {
    u64::from(r.w).saturating_mul(u64::from(r.h))
}

/// Map `planecompose`'s refusals onto the kernel's errors: a rectangle that
/// cannot be shown is the caller's mistake (`InvalidArgument`); a buffer that
/// does not match its own geometry is ours (`InternalError`).
#[must_use]
pub fn map_err(e: planecompose::Error) -> KernelError {
    match e {
        planecompose::Error::EmptyRect
        | planecompose::Error::SourceOutOfBounds
        | planecompose::Error::ScaleOutOfRange
        | planecompose::Error::BadDimension => KernelError::InvalidArgument,
        planecompose::Error::BufferTooSmall | planecompose::Error::Unaligned => {
            KernelError::InternalError
        }
    }
}

/// A GEM object's system-RAM frames as read-only pages, through the HHDM.
///
/// # Errors
///
/// Whatever [`GemObject::ram_frames`] returns (a VRAM-resident object has no
/// frames), or `NotSupported` without an HHDM.
pub fn gem_pages(gem: &GemObject) -> KernelResult<Vec<&[u8]>> {
    let hhdm = crate::mm::page_table::hhdm().ok_or(KernelError::NotSupported)?;
    let frames = gem.ram_frames()?;
    let mut pages = Vec::with_capacity(frames.len());
    for pf in frames {
        let addr = pf
            .addr()
            .checked_add(hhdm)
            .ok_or(KernelError::InternalError)?;
        // SAFETY: `pf` is one of this GEM object's own frames, HHDM-mapped for
        // FRAME_SIZE bytes, and the slice borrows `gem`, so the frames cannot
        // be freed while it lives: `gem_destroy` needs the device mutably, and
        // the device is behind its registry lock for the whole composition.
        //
        // What this does NOT exclude: the buffer is also mapped into the
        // client that owns it, which may write it while it is composed. That
        // is a race on the bytes themselves, exactly as in the raw-pointer
        // copies the trivial page flip has always made (`blit_run`,
        // `blit_run_flat`); scanning out a buffer its owner can still draw into
        // is inherent to the API, which is why real clients double-buffer. Its
        // effect is a torn picture: the slice's bounds are fixed, every u8 is
        // a valid u8, and the slice never outlives `compose`, so no read can
        // leave the frame and no value read can be invalid.
        pages.push(unsafe { core::slice::from_raw_parts(addr as *const u8, FRAME_SIZE) });
    }
    Ok(pages)
}

/// What a pixel no layer covers shows: opaque black.
pub const BACKGROUND: u32 = 0xFF00_0000;

impl super::DrmDevice {
    /// Whether this device's backend composes with the CPU. The firmware
    /// framebuffer and virtio-gpu's 2D path do; a backend that scans hardware
    /// planes out directly composes nothing, and shows only trivial scenes.
    #[must_use]
    pub fn composes_in_software(&self) -> bool {
        matches!(
            self.backend,
            super::DrmBackend::Limine(_) | super::DrmBackend::VirtioGpu(_)
        )
    }

    /// One pixel of what is on screen, read back from a software backend's
    /// scanout: the verification path the composition self-test needs. `None`
    /// off-screen, without a device, or on a backend that scans video memory
    /// out directly (there is no composed image to read).
    #[must_use]
    pub fn scanout_pixel(&self, x: u32, y: u32) -> Option<u32> {
        match &self.backend {
            super::DrmBackend::Limine(b) => b.read_pixel(x, y),
            super::DrmBackend::VirtioGpu(_) => crate::virtio::gpu::read_pixel(x, y),
            super::DrmBackend::Ati(_) => None,
        }
    }

    /// The layers `crtc_id` shows, bottom first; `None` when the display is
    /// not the DRM's to draw -- no mode, or no framebuffer on the primary
    /// plane, which is when the kernel console owns the screen and nothing
    /// here may paint over it.
    ///
    /// # Errors
    ///
    /// `NotFound` for an unknown CRTC, or a plane naming a framebuffer or a
    /// cursor naming a GEM object that no longer exists.
    pub fn scene(&self, crtc_id: DrmObjectId) -> KernelResult<Option<Scene>> {
        let Some(crtc_idx) = self.crtcs.iter().position(|c| c.id == crtc_id) else {
            return Err(KernelError::NotFound);
        };
        let crtc = self.crtcs.get(crtc_idx).ok_or(KernelError::NotFound)?;
        let Some(mode) = crtc.mode else {
            return Ok(None);
        };
        let Some(primary) = self.planes.iter().find(|p| p.id == crtc.primary_plane) else {
            return Ok(None);
        };
        let Some(primary_fb) = primary.fb else {
            return Ok(None);
        };
        let screen = Rect::new(0, 0, mode.hdisplay, mode.vdisplay);
        let fb = self.fb_get(primary_fb).ok_or(KernelError::NotFound)?;
        let (src, dst) = plane_rects(primary)?;
        let mut trivial = FixedRect::whole(fb.width, fb.height).is_ok_and(|whole| whole == src)
            && dst == screen
            && fb.width == mode.hdisplay
            && fb.height == mode.vdisplay;
        let mut layers = alloc::vec![SceneLayer {
            source: LayerSource::Framebuffer(primary_fb),
            plane: Some(primary.id),
            src,
            dst,
            blend: Blend::Opaque,
        }];
        for kind in [PlaneType::Overlay, PlaneType::Cursor] {
            for p in self
                .planes
                .iter()
                .filter(|p| p.plane_type == kind && p.crtc == Some(crtc_id))
            {
                let Some(fb_id) = p.fb else {
                    continue;
                };
                let fb = self.fb_get(fb_id).ok_or(KernelError::NotFound)?;
                let (src, dst) = plane_rects(p)?;
                layers.push(SceneLayer {
                    source: LayerSource::Framebuffer(fb_id),
                    plane: Some(p.id),
                    src,
                    dst,
                    blend: blend_for(kind, fb.format),
                });
                trivial = false;
            }
        }
        // The legacy cursor, which `cursor_set`/`cursor_move` record and which
        // nothing drew before §976. Its image is premultiplied ARGB, as the
        // Linux cursor ioctls define it.
        if let Some(cs) = self.cursor_states.get(crtc_idx)
            && cs.visible
            && cs.gem_handle != 0
            && cs.width != 0
            && cs.height != 0
        {
            if !self.gem_objects.iter().any(|g| g.handle == cs.gem_handle) {
                return Err(KernelError::NotFound);
            }
            layers.push(SceneLayer {
                source: LayerSource::Gem {
                    handle: cs.gem_handle,
                    width: cs.width,
                    height: cs.height,
                },
                plane: None,
                src: FixedRect::whole(cs.width, cs.height).map_err(map_err)?,
                dst: cursor_rect(cs.x, cs.y, cs.hot_x, cs.hot_y, cs.width, cs.height),
                blend: Blend::Premultiplied,
            });
            trivial = false;
        }
        Ok(Some(Scene {
            screen,
            layers,
            trivial,
        }))
    }

    /// Whether `scene` can be shown exactly as it stands on this device --
    /// checked before anything is drawn or programmed, so that a scene that
    /// cannot be is refused whole rather than shown in part.
    ///
    /// Two questions, in this order, so a client can tell "never valid" from
    /// "not here". Is every layer well-formed -- its framebuffer in a format
    /// its plane lists, its source inside that buffer, its scale within
    /// [`LIMITS`]? If not, `InvalidArgument`, on any backend. (The format list
    /// is what `GETPLANE` reports, so a buffer outside it is the client's
    /// mistake, and Linux refuses it the same way. Before 2026-09-27 nothing
    /// checked it, and the flip copied an RGB565 or BGR-ordered buffer's bytes
    /// into an XRGB scanout: garbage, or red and blue swapped.)
    /// Then: can this backend show it? A trivial scene, yes, anywhere.
    /// Anything more must be composed, which needs a software backend and
    /// sources laid out like the scanout (see [`composable`]) from their
    /// first byte; otherwise `NotSupported`.
    ///
    /// # Errors
    ///
    /// `InvalidArgument` for a malformed layer, `NotSupported` for a scene
    /// this backend cannot compose, `NotFound` for a vanished framebuffer.
    pub fn validate_scene(&self, scene: &Scene) -> KernelResult<()> {
        for layer in &scene.layers {
            let (width, height) = match layer.source {
                LayerSource::Framebuffer(id) => {
                    let fb = self.fb_get(id).ok_or(KernelError::NotFound)?;
                    if let Some(plane_id) = layer.plane {
                        let plane = self
                            .planes
                            .iter()
                            .find(|p| p.id == plane_id)
                            .ok_or(KernelError::NotFound)?;
                        if !plane.formats.contains(&fb.format) {
                            return Err(KernelError::InvalidArgument);
                        }
                    }
                    (fb.width, fb.height)
                }
                LayerSource::Gem { width, height, .. } => (width, height),
            };
            planecompose::check_layer(width, height, layer.src, layer.dst, LIMITS)
                .map_err(map_err)?;
        }
        if scene.trivial {
            return Ok(());
        }
        if !self.composes_in_software() {
            return Err(KernelError::NotSupported);
        }
        for layer in &scene.layers {
            // A cursor image is always packed ARGB from its first byte, which
            // `cursor_set` checked its buffer holds.
            if let LayerSource::Framebuffer(id) = layer.source {
                let fb = self.fb_get(id).ok_or(KernelError::NotFound)?;
                if fb.offset != 0 || !composable(fb.format) {
                    return Err(KernelError::NotSupported);
                }
            }
        }
        Ok(())
    }

    /// Show `crtc_id`'s scene inside `a` and inside `b`: as their union when
    /// that costs no more than the two apart (a cursor nudged a few pixels),
    /// and otherwise as two -- so a cursor that jumps across the screen
    /// redraws two cursor-sized squares, not everything between them.
    ///
    /// # Errors
    ///
    /// As [`Self::present`].
    pub fn present_pair(&mut self, crtc_id: DrmObjectId, a: Rect, b: Rect) -> KernelResult<()> {
        let both = a.union(b);
        if area(both) <= area(a).saturating_add(area(b)) {
            self.present(crtc_id, Some(both))
        } else {
            self.present(crtc_id, Some(a))?;
            self.present(crtc_id, Some(b))
        }
    }

    /// Show `crtc_id`'s scene on the display, inside `damage` (`None`: the
    /// whole screen). Does nothing while the display is not the DRM's (see
    /// [`Self::scene`]).
    ///
    /// A trivial scene presented whole is the old page flip, one copy, and a
    /// region of one is the backend's own copy of that region. Anything else
    /// is composed; a backend that cannot compose refuses it, rather than
    /// showing something other than what was asked for.
    ///
    /// # Errors
    ///
    /// `NotSupported` for a non-trivial scene on a backend that cannot compose,
    /// or a layer whose format cannot be composed; `NotFound` for a vanished
    /// object; `InvalidArgument` for a rectangle that cannot be shown; and the
    /// backend's own errors.
    pub fn present(&mut self, crtc_id: DrmObjectId, damage: Option<Rect>) -> KernelResult<()> {
        // An empty damage -- an invisible cursor moving -- changes no pixel,
        // on any backend.
        if damage.is_some_and(Rect::is_empty) {
            return Ok(());
        }
        let Some(scene) = self.scene(crtc_id)? else {
            return Ok(());
        };
        // Before anything is drawn: a scene that cannot be shown whole is not
        // shown in part.
        self.validate_scene(&scene)?;
        if scene.trivial {
            let Some(LayerSource::Framebuffer(fb_id)) = scene.layers.first().map(|l| l.source)
            else {
                return Err(KernelError::InternalError);
            };
            let fb = self
                .framebuffers
                .iter()
                .find(|f| f.id == fb_id)
                .ok_or(KernelError::NotFound)?;
            let gem = self
                .gem_objects
                .iter()
                .find(|g| g.handle == fb.gem_handle)
                .ok_or(KernelError::NotFound)?;
            let Some(damage) = damage else {
                return match &mut self.backend {
                    super::DrmBackend::Limine(b) => b.page_flip(crtc_id, fb, gem),
                    super::DrmBackend::VirtioGpu(b) => b.page_flip(crtc_id, fb, gem),
                    super::DrmBackend::Ati(b) => b.page_flip(crtc_id, fb, gem),
                };
            };
            // The buffer is the screen pixel for pixel, so a region of the
            // screen is the backend's own copy of that region of the buffer --
            // on every backend, including one that composes nothing.
            let Some(r) = damage.intersect(scene.screen) else {
                return Ok(());
            };
            // Inside the screen, whose corner is (0, 0): never negative.
            let x = u32::try_from(r.x).map_err(|_| KernelError::InternalError)?;
            let y = u32::try_from(r.y).map_err(|_| KernelError::InternalError)?;
            return match &mut self.backend {
                super::DrmBackend::Limine(b) => b.flush_region(fb, gem, x, y, r.w, r.h),
                super::DrmBackend::VirtioGpu(b) => b.flush_region(fb, gem, x, y, r.w, r.h),
                super::DrmBackend::Ati(b) => b.flush_region(fb, gem, x, y, r.w, r.h),
            };
        }
        // Composed from here on. `validate_scene` refused this on a backend
        // that cannot compose, and any source it cannot read as the scanout's
        // layout, so what remains cannot fail for either reason.
        let damage = damage.unwrap_or(scene.screen);
        // Split the borrows: the layers read the framebuffers and GEM objects
        // while the backend writes its scanout.
        let super::DrmDevice {
            backend,
            framebuffers,
            gem_objects,
            ..
        } = self;
        let mut pages: Vec<Vec<&[u8]>> = Vec::with_capacity(scene.layers.len());
        let mut geometry: Vec<(u32, u32, u32)> = Vec::with_capacity(scene.layers.len());
        for layer in &scene.layers {
            let (handle, width, height, fb_pitch) = match layer.source {
                LayerSource::Framebuffer(id) => {
                    let fb = framebuffers
                        .iter()
                        .find(|f| f.id == id)
                        .ok_or(KernelError::NotFound)?;
                    (fb.gem_handle, fb.width, fb.height, Some(fb.pitch))
                }
                LayerSource::Gem {
                    handle,
                    width,
                    height,
                } => (handle, width, height, None),
            };
            let gem = gem_objects
                .iter()
                .find(|g| g.handle == handle)
                .ok_or(KernelError::NotFound)?;
            // A cursor image's rows are its GEM object's rows: `gem_create`
            // pads a pitch to 64 bytes, so a cursor whose width is not a
            // multiple of 16 is not packed, and reading it as `width * 4`
            // would shear it.
            let pitch = fb_pitch.unwrap_or(gem.pitch);
            pages.push(gem_pages(gem)?);
            geometry.push((width, height, pitch));
        }
        let mut layers: Vec<planecompose::Layer<'_>> = Vec::with_capacity(scene.layers.len());
        for ((layer, page_list), &(width, height, pitch)) in
            scene.layers.iter().zip(pages.iter()).zip(geometry.iter())
        {
            layers.push(planecompose::Layer {
                source: planecompose::Source {
                    data: planecompose::Paged {
                        pages: page_list,
                        page_size: FRAME_SIZE,
                    },
                    width,
                    height,
                    pitch,
                },
                src: layer.src,
                dst: layer.dst,
                blend: layer.blend,
            });
        }
        match backend {
            super::DrmBackend::Limine(b) => b.compose(damage, &layers),
            super::DrmBackend::VirtioGpu(b) => b.compose(damage, &layers),
            super::DrmBackend::Ati(_) => Err(KernelError::NotSupported),
        }
    }
}

/// The part of the display a legacy cursor state covers: its rectangle when it
/// is visible, empty when it is not -- so the union of the before and after of
/// any change is exactly what that change affects.
#[must_use]
pub fn cursor_damage(cs: &super::atomic::CursorState) -> Rect {
    if cs.visible && cs.gem_handle != 0 {
        cursor_rect(cs.x, cs.y, cs.hot_x, cs.hot_y, cs.width, cs.height)
    } else {
        Rect::new(0, 0, 0, 0)
    }
}

/// The rectangle a legacy cursor covers on the display.
#[must_use]
pub fn cursor_rect(x: i32, y: i32, hot_x: u32, hot_y: u32, width: u32, height: u32) -> Rect {
    let left = i64::from(x).saturating_sub(i64::from(hot_x));
    let top = i64::from(y).saturating_sub(i64::from(hot_y));
    Rect::new(
        i32::try_from(left).unwrap_or(i32::MIN),
        i32::try_from(top).unwrap_or(i32::MIN),
        width,
        height,
    )
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// A pixel's colour, without the byte an XRGB buffer leaves undefined.
const RGB: u32 = 0x00FF_FFFF;

/// Test pattern A: every pixel names its own coordinates, so a wrong pixel
/// says where it came from.
fn pattern_a(x: u32, y: u32) -> u32 {
    0xFF00_0000 | ((x & 0xFFF) << 12) | (y & 0xFFF)
}

/// Test pattern B: A with its colour inverted, so the two never agree.
fn pattern_b(x: u32, y: u32) -> u32 {
    pattern_a(x, y) ^ RGB
}

/// The cursor image: opaque green over its top half, fully transparent below
/// -- premultiplied, as cursor images are.
fn cursor_image(_x: u32, y: u32) -> u32 {
    if y < 8 { 0xFF00_FF00 } else { 0 }
}

/// Write one pixel of a system-RAM GEM object, given its frames' HHDM
/// addresses and its pitch.
fn poke(frames: &[u64], pitch: u32, x: u32, y: u32, value: u32) -> KernelResult<()> {
    let off = u64::from(y)
        .checked_mul(u64::from(pitch))
        .and_then(|row| row.checked_add(u64::from(x).checked_mul(4)?))
        .and_then(|o| usize::try_from(o).ok())
        .ok_or(KernelError::InternalError)?;
    let index = off
        .checked_div(FRAME_SIZE)
        .ok_or(KernelError::InternalError)?;
    let within = off
        .checked_rem(FRAME_SIZE)
        .ok_or(KernelError::InternalError)?;
    let base = *frames.get(index).ok_or(KernelError::InternalError)?;
    let addr = u64::try_from(within)
        .ok()
        .and_then(|w| base.checked_add(w))
        .ok_or(KernelError::InternalError)?;
    // SAFETY: `base` is the HHDM address of one of the object's own frames,
    // FRAME_SIZE bytes long (`gem_frame_addrs`). `within` is below FRAME_SIZE
    // and 4-aligned -- the pitch is a multiple of 64, `x * 4` of 4, and
    // FRAME_SIZE of both -- so the 4-byte write stays inside that frame. The
    // caller holds the device lock, so nothing composes from the buffer while
    // it is written.
    unsafe { core::ptr::write_volatile(addr as *mut u32, value) };
    Ok(())
}

/// Fill GEM object `handle`, `width` x `height`, with `f(x, y)`.
fn fill(
    dev: &super::DrmDevice,
    handle: u32,
    width: u32,
    height: u32,
    f: impl Fn(u32, u32) -> u32,
) -> KernelResult<()> {
    let frames = dev.gem_frame_addrs(handle)?;
    let pitch = dev.gem_pitch(handle)?;
    for y in 0..height {
        for x in 0..width {
            poke(&frames, pitch, x, y, f(x, y))?;
        }
    }
    Ok(())
}

/// The colour on screen at (`x`, `y`).
fn at(dev: &super::DrmDevice, x: u32, y: u32) -> Option<u32> {
    dev.scanout_pixel(x, y).map(|p| p & RGB)
}

/// Whether every screen point `(x, y)` in `points` shows `expect(x, y)`;
/// the first that does not is logged.
fn shows(dev: &super::DrmDevice, points: &[(u32, u32)], expect: impl Fn(u32, u32) -> u32) -> bool {
    for &(x, y) in points {
        let want = expect(x, y) & RGB;
        let got = at(dev, x, y);
        if got != Some(want) {
            crate::serial_println!(
                "[drm]     screen ({}, {}) is {:08x?}, expected {:06x}",
                x,
                y,
                got,
                want,
            );
            return false;
        }
    }
    true
}

/// A plane's framebuffer and rectangles, for "nothing changed" checks.
type PlaneGeometry = (Option<DrmObjectId>, [u32; 4], [i32; 2], [u32; 2]);

fn plane_geometry(dev: &super::DrmDevice, id: DrmObjectId) -> Option<PlaneGeometry> {
    dev.planes().iter().find(|p| p.id == id).map(|p| {
        (
            p.fb,
            [p.src_x, p.src_y, p.src_w, p.src_h],
            [p.dst_x, p.dst_y],
            [p.dst_w, p.dst_h],
        )
    })
}

/// Record the first failure; later ones would only be its consequences.
fn check(fail: &mut Option<&'static str>, cond: bool, what: &'static str) {
    if !cond && fail.is_none() {
        crate::serial_println!("[drm]   FAIL: {}", what);
        *fail = Some(what);
    }
}

/// Objects the test created, released whatever happens.
#[derive(Default)]
struct Created {
    gems: Vec<u32>,
    fbs: Vec<DrmObjectId>,
}

impl Created {
    fn gem(
        &mut self,
        dev: &mut super::DrmDevice,
        w: u32,
        h: u32,
        f: PixelFormat,
    ) -> KernelResult<u32> {
        let handle = dev.gem_create(w, h, f)?;
        self.gems.push(handle);
        Ok(handle)
    }

    fn fb(
        &mut self,
        dev: &mut super::DrmDevice,
        gem: u32,
        w: u32,
        h: u32,
        f: PixelFormat,
    ) -> KernelResult<DrmObjectId> {
        let pitch = dev.gem_pitch(gem)?;
        let id = dev.fb_create(gem, w, h, pitch, f)?;
        self.fbs.push(id);
        Ok(id)
    }

    /// Framebuffers first, then their memory: the order every backend
    /// accepts (the CRTC is already off when this runs).
    fn release(self, dev: &mut super::DrmDevice) -> KernelResult<()> {
        let mut worst = Ok(());
        for id in self.fbs {
            if let Err(e) = dev.fb_destroy(id) {
                worst = Err(e);
            }
        }
        for handle in self.gems {
            if let Err(e) = dev.gem_destroy(handle) {
                worst = Err(e);
            }
        }
        worst
    }
}

/// Boot self-test of design-decisions §976 on the primary display: planes
/// shown where and at the size they were asked for, the cursor drawn and
/// taken away, a flush redrawn where its buffer lands on screen, and what
/// cannot be shown refused -- with the model and the screen left as they
/// were. Every expected pixel is read back out of the scanout.
///
/// Runs after the mode-set test, which leaves the CRTC off, and leaves it off
/// again. On a backend that composes nothing there is no composed image to
/// read, so it reports a skip.
///
/// # Errors
///
/// `InternalError` if any check fails (each is logged), or what creating or
/// releasing the test's buffers returns.
pub(super) fn self_test() -> KernelResult<()> {
    super::with_primary_mut(|dev| {
        if !dev.composes_in_software() {
            crate::serial_println!(
                "[drm]   Plane composition: SKIP ({} scans hardware planes out directly and composes nothing)",
                dev.driver_name(),
            );
            return Ok(());
        }
        let (w, h) = dev.display_size();
        if w < 128 || h < 128 {
            crate::serial_println!(
                "[drm]   Plane composition: SKIP (a {}x{} display is too small to place the test's rectangles)",
                w,
                h,
            );
            return Ok(());
        }
        let crtc_id = dev.first_crtc_id().ok_or(KernelError::InternalError)?;
        let mut created = Created::default();
        let result = run(dev, &mut created, w, h);
        // The CRTC goes back off whatever happened, before its buffers go.
        let off = dev.set_crtc(crtc_id, None, 0, 0, &[], None);
        let released = created.release(dev);
        let fail = result?;
        off?;
        released?;
        if let Some(what) = fail {
            crate::serial_println!("[drm]   Plane composition FAILED: {}", what);
            return Err(KernelError::InternalError);
        }
        crate::serial_println!("[drm]   Plane composition ({}x{}): OK", w, h);
        Ok(())
    })
}

/// The body of [`self_test`]: `Ok(Some(what))` for a failed check, `Err` for
/// a failure to set the test up.
// One scenario, read top to bottom. Its coordinate arithmetic is bounded:
// `self_test` runs it only for 128 <= w, h, and a mode is at most
// `planecompose::MAX_DIMENSION` (2^14) on a side, so no sum, difference or
// doubling here can leave u32 or go below zero.
#[allow(clippy::too_many_lines, clippy::arithmetic_side_effects)]
fn run(
    dev: &mut super::DrmDevice,
    created: &mut Created,
    w: u32,
    h: u32,
) -> KernelResult<Option<&'static str>> {
    use super::atomic::{AtomicState, IRect, PlaneState, Rect as SrcRect, atomic_commit};

    let crtc_id = dev.first_crtc_id().ok_or(KernelError::InternalError)?;
    let conn_id = dev
        .connectors()
        .first()
        .map(|c| c.id)
        .ok_or(KernelError::InternalError)?;
    let mode = dev
        .connectors()
        .first()
        .and_then(|c| c.modes.iter().find(|m| m.hdisplay == w && m.vdisplay == h))
        .copied()
        .ok_or(KernelError::InternalError)?;
    let primary = dev
        .crtcs()
        .iter()
        .find(|c| c.id == crtc_id)
        .map(|c| c.primary_plane)
        .ok_or(KernelError::InternalError)?;

    // A: the display's size. B: larger, for an origin inside it. C: the
    // cursor image. X: a format the plane does not list. S: too small for
    // what it will be said to hold.
    let gem_a = created.gem(dev, w, h, PixelFormat::Xrgb8888)?;
    let fb_a = created.fb(dev, gem_a, w, h, PixelFormat::Xrgb8888)?;
    let (bw, bh) = (w.saturating_add(32), h.saturating_add(16));
    let gem_b = created.gem(dev, bw, bh, PixelFormat::Xrgb8888)?;
    let fb_b = created.fb(dev, gem_b, bw, bh, PixelFormat::Xrgb8888)?;
    let gem_c = created.gem(dev, 16, 16, PixelFormat::Argb8888)?;
    let gem_x = created.gem(dev, 16, 16, PixelFormat::Xbgr8888)?;
    let fb_x = created.fb(dev, gem_x, 16, 16, PixelFormat::Xbgr8888)?;
    let gem_s = created.gem(dev, 16, 16, PixelFormat::Xrgb8888)?;
    fill(dev, gem_a, w, h, pattern_a)?;
    fill(dev, gem_b, bw, bh, pattern_b)?;
    fill(dev, gem_c, 16, 16, cursor_image)?;

    // Put `fb` on the primary plane showing `src` at `dst`, atomically.
    let commit = |dev: &mut super::DrmDevice,
                  fb: DrmObjectId,
                  src: (u32, u32, u32, u32),
                  dst: (i32, i32, u32, u32),
                  test_only: bool| {
        let mut state = AtomicState::new();
        state.test_only = test_only;
        state.add_plane(PlaneState {
            id: primary,
            fb_id: Some(Some(fb)),
            crtc_id: Some(Some(crtc_id)),
            src_rect: Some(SrcRect {
                x: src.0,
                y: src.1,
                w: src.2,
                h: src.3,
            }),
            dst_rect: Some(IRect {
                x: dst.0,
                y: dst.1,
                w: dst.2,
                h: dst.3,
            }),
        });
        atomic_commit(dev, &state)
    };
    let (qw, qh) = (w / 2, h / 2);
    let (qx, qy) = (w / 4, h / 4);
    let (sqx, sqy) = (
        i32::try_from(qx).unwrap_or(0),
        i32::try_from(qy).unwrap_or(0),
    );
    let mut fail: Option<&'static str> = None;

    // (a) A whole, unmoved buffer: the trivial scene, the old flip.
    let r = dev.set_crtc(crtc_id, Some(fb_a), 0, 0, &[conn_id], Some(&mode));
    check(
        &mut fail,
        r.is_ok(),
        "set_crtc refused a whole, unmoved buffer",
    );
    check(
        &mut fail,
        shows(dev, &[(0, 0), (w - 1, h - 1), (w / 2, h / 3)], pattern_a),
        "a whole, unmoved buffer is not what is on screen",
    );

    // (b) SETCRTC's origin inside a larger buffer: composed, not shown from
    //     the corner as it was before §976.
    let r = dev.set_crtc(crtc_id, Some(fb_b), 16, 8, &[conn_id], Some(&mode));
    check(
        &mut fail,
        r.is_ok(),
        "set_crtc refused an origin inside a larger buffer",
    );
    check(
        &mut fail,
        shows(dev, &[(0, 0), (w - 1, h - 1), (w / 2, h / 3)], |x, y| {
            pattern_b(x + 16, y + 8)
        }),
        "set_crtc's x, y did not move the buffer on screen",
    );

    // (c) Atomic: a crop of A, moved, unscaled. Uncovered screen is background.
    let r = commit(dev, fb_a, (8, 4, qw, qh), (sqx, sqy, qw, qh), false);
    check(&mut fail, r.is_ok(), "atomic commit refused a moved crop");
    check(
        &mut fail,
        shows(
            dev,
            &[(qx, qy), (qx + qw - 1, qy + qh - 1), (qx + qw / 2, qy + 3)],
            |x, y| pattern_a(x - qx + 8, y - qy + 4),
        ),
        "a cropped, moved plane is not where it was put",
    );
    check(
        &mut fail,
        shows(dev, &[(0, 0), (w - 1, h - 1), (qx - 1, qy)], |_, _| {
            BACKGROUND
        }),
        "screen no plane covers is not the background",
    );

    // (c2) A flush of the shown buffer is redrawn where it lands on screen:
    //      pixel (13, 10) of A shows at (qx + 5, qy + 6).
    let frames_a = dev.gem_frame_addrs(gem_a)?;
    let pitch_a = dev.gem_pitch(gem_a)?;
    poke(&frames_a, pitch_a, 13, 10, 0xFF12_3456)?;
    let r = dev.flush_region(fb_a, 13, 10, 1, 1);
    check(&mut fail, r.is_ok(), "flush_region refused a shown buffer");
    check(
        &mut fail,
        at(dev, qx + 5, qy + 6) == Some(0x12_3456),
        "a flush of a moved buffer was not redrawn where the buffer is shown",
    );
    poke(&frames_a, pitch_a, 13, 10, pattern_a(13, 10))?;
    let r = dev.flush_region(fb_a, 13, 10, 1, 1);
    check(&mut fail, r.is_ok(), "flush_region refused a shown buffer");

    // (c3) A flush of a buffer nobody shows changes nothing on screen -- it
    //      used to be copied over whatever was.
    let r = dev.flush_region(fb_b, 0, 0, w, h);
    check(
        &mut fail,
        r.is_ok(),
        "flush_region refused a buffer that is not shown",
    );
    check(
        &mut fail,
        shows(dev, &[(0, 0)], |_, _| BACKGROUND) && shows(dev, &[(qx, qy)], |_, _| pattern_a(8, 4)),
        "a flush of a buffer nobody shows drew it on screen",
    );

    // (d) Atomic: a quarter of A, scaled up twice.
    let r = commit(dev, fb_a, (0, 0, qw, qh), (0, 0, qw * 2, qh * 2), false);
    check(&mut fail, r.is_ok(), "atomic commit refused a 2x scale");
    for (i, j) in [(0, 0), (5, 7), (qw - 1, qh - 1)] {
        check(
            &mut fail,
            shows(dev, &[(2 * i, 2 * j), (2 * i + 1, 2 * j + 1)], |_, _| {
                pattern_a(i, j)
            }),
            "a plane scaled 2x does not repeat each source pixel twice",
        );
    }

    // (e) Refusals. Each must leave the plane and the screen as they were.
    let before = plane_geometry(dev, primary);
    let pixel = at(dev, 3, 3);
    let unchanged =
        |dev: &super::DrmDevice| plane_geometry(dev, primary) == before && at(dev, 3, 3) == pixel;
    let r = commit(dev, fb_a, (w - 4, 0, 8, 8), (0, 0, 8, 8), false);
    check(
        &mut fail,
        matches!(r, Err(KernelError::InvalidArgument)),
        "a source leaving its buffer was not refused",
    );
    check(
        &mut fail,
        unchanged(dev),
        "a refused commit changed the plane or the screen",
    );
    let r = commit(dev, fb_a, (0, 0, 1, 1), (0, 0, 17, 17), false);
    check(
        &mut fail,
        matches!(r, Err(KernelError::InvalidArgument)),
        "a 17x scale was not refused",
    );
    let r = commit(dev, fb_a, (0, 0, 20_000, 1), (0, 0, 16, 1), false);
    check(
        &mut fail,
        matches!(r, Err(KernelError::InvalidArgument)),
        "a source wider than any buffer can be was not refused",
    );
    let r = commit(dev, fb_x, (0, 0, 16, 16), (0, 0, 16, 16), false);
    check(
        &mut fail,
        matches!(r, Err(KernelError::InvalidArgument)),
        "a buffer in a format the plane does not list was not refused",
    );
    let r = commit(dev, fb_a, (0, 0, w, h), (0, 0, w, h), true);
    check(&mut fail, r.is_ok(), "a valid TEST_ONLY commit was refused");
    check(
        &mut fail,
        unchanged(dev),
        "a refused or TEST_ONLY commit changed the plane or the screen",
    );
    check(
        &mut fail,
        matches!(
            dev.fb_create(gem_s, 64, 64, 256, PixelFormat::Xrgb8888),
            Err(KernelError::InvalidArgument)
        ),
        "fb_create accepted a framebuffer larger than its memory",
    );
    check(
        &mut fail,
        matches!(
            dev.fb_create(gem_s, 16, 16, 32, PixelFormat::Xrgb8888),
            Err(KernelError::InvalidArgument)
        ),
        "fb_create accepted a pitch narrower than a row",
    );

    // (f) The cursor, over the trivial scene: drawn at its hot spot, blended,
    //     moved (its old place restored from the plane beneath), hidden.
    let r = commit(dev, fb_a, (0, 0, w, h), (0, 0, w, h), false);
    check(
        &mut fail,
        r.is_ok(),
        "atomic commit refused the whole buffer back",
    );
    let (cx, cy) = (w / 8, h / 8);
    let r = dev.cursor_set(crtc_id, gem_c, 16, 16, 2, 3).and_then(|()| {
        dev.cursor_move(
            crtc_id,
            i32::try_from(cx).unwrap_or(0),
            i32::try_from(cy).unwrap_or(0),
        )
    });
    check(
        &mut fail,
        r.is_ok(),
        "the cursor could not be set and moved",
    );
    // The image's top-left is the position less the hot spot.
    let (left, top) = (cx - 2, cy - 3);
    check(
        &mut fail,
        shows(dev, &[(left + 4, top + 1), (left + 15, top + 7)], |_, _| {
            0x0000_FF00
        }),
        "the cursor's opaque half is not drawn at its hot spot",
    );
    check(
        &mut fail,
        shows(
            dev,
            &[(left + 4, top + 12), (left - 1, top), (left + 16, top + 1)],
            pattern_a,
        ),
        "the cursor's transparent half, or the screen beside it, is not the plane beneath",
    );
    let (mx, my) = (w / 2, h / 2);
    let r = dev.cursor_move(
        crtc_id,
        i32::try_from(mx).unwrap_or(0),
        i32::try_from(my).unwrap_or(0),
    );
    check(&mut fail, r.is_ok(), "the cursor could not be moved");
    check(
        &mut fail,
        shows(dev, &[(left + 4, top + 1), (left + 15, top + 7)], pattern_a),
        "where the cursor was is not restored from the plane beneath",
    );
    check(
        &mut fail,
        shows(dev, &[(mx - 2 + 4, my - 3 + 1)], |_, _| 0x0000_FF00),
        "the cursor is not drawn where it moved to",
    );
    check(
        &mut fail,
        matches!(
            dev.cursor_set(crtc_id, gem_s, 64, 64, 0, 0),
            Err(KernelError::InvalidArgument)
        ),
        "a cursor larger than its buffer was not refused",
    );
    let r = dev.cursor_set(crtc_id, 0, 0, 0, 0, 0);
    check(&mut fail, r.is_ok(), "the cursor could not be hidden");
    check(
        &mut fail,
        shows(dev, &[(mx - 2 + 4, my - 3 + 1)], pattern_a),
        "a hidden cursor is still on screen",
    );

    Ok(fail)
}
