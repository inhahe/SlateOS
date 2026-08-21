//! The rendering-backend seam: what "composite a frame" means, apart from
//! *how* the pixels get written.
//!
//! # Why this exists
//!
//! Until this module the compositor drew every pixel itself. `compose_frame`
//! called [`Framebuffer`](crate::Framebuffer) methods directly, the render
//! engine walked glyph coverage masks a pixel at a time, and `blit_buffer`
//! reached into a client's shared memory and blended it byte by byte. All of
//! that is correct, and all of it is CPU work — there was no point in the code
//! where a GPU could be handed anything, because there was nothing to hand it
//! but a finished screen.
//!
//! [`RenderTarget`] is that point. It is deliberately drawn at the level a GPU
//! can actually execute — solid quads, textured quads, coverage-mask quads,
//! lines — and *not* at the level the software rasterizer happens to work in
//! (rows and pixels). A seam placed at `blend_pixel` would be a seam no GPU
//! could implement without giving up everything that makes it a GPU.
//!
//! # What stays above the seam
//!
//! Everything that is policy rather than rasterization:
//!
//! - window management, stacking and focus;
//! - damage tracking and the occlusion cull (which rectangles need painting);
//! - the decoration geometry — shadow, border, title bar;
//! - the clip and translate stacks, and the coordinate math that resolves a
//!   client's command into screen space;
//! - text *shaping* — a GPU backend uploads the same [`GlyphMask`]es this one
//!   blends, so choosing the face, kerning the run and deciding where the
//!   ellipsis goes are shared work, not per-backend work.
//!
//! A backend that had to re-derive any of those could disagree with the
//! software one about where a title bar is, which is the exact class of bug the
//! decoration helpers' comments already work hard to prevent. So it does not
//! get the chance.
//!
//! # What lives below it
//!
//! Only the act of putting colour on the surface. The software backend
//! ([`Framebuffer`](crate::Framebuffer)'s implementation, in `lib.rs` next to
//! the buffers it owns) keeps every optimisation it had: parallel row bands,
//! the opaque-blit memcpy path, per-row solid fills, the clipped Bresenham
//! walk. Nothing moved *out* of the fast paths — the seam was cut above them.
//!
//! # Why an enum and not `dyn RenderTarget`
//!
//! Backend selection has to be a runtime decision (the GPU may be absent, or
//! disabled by a boot flag), but the primitives are called hundreds of times a
//! frame and `dyn` would cost an indirect call and a missed inline on each. So
//! the *pipeline* is generic over `T: RenderTarget` — monomorphised, zero
//! dispatch — and [`RenderBackend`] is the one concrete type it is instantiated
//! with, resolving the choice with a single predictable branch per primitive
//! rather than a vtable per pixel.

use crate::buffer::SharedBuffer;
use crate::{CompositorResult, Framebuffer, Rect};
use osfont::raster::GlyphMask;

/// A surface a frame can be composited onto.
///
/// Implementors own pixels; callers own *what* should be drawn. Every method
/// takes screen-space coordinates already resolved by the caller, and every one
/// is total: a primitive that falls entirely outside the surface, or that
/// resolves to zero effective alpha, is dropped rather than reported. That is
/// deliberate — the caller is a display server compositing another process's
/// commands, and there is no useful action it could take on "that rectangle was
/// off-screen".
///
/// ## Clipping
///
/// Two clips apply, and they are not the same thing:
///
/// - The **frame clip** ([`set_frame_clip`](Self::set_frame_clip)) is the
///   compositor's own damage/occlusion cull. It is backend state, applies to
///   every primitive, and exists so a partial recomposite cannot paint outside
///   the region it was asked to repaint.
/// - The **draw clip** (the `clip` argument on the primitives that take one) is
///   the *client's* clip stack, resolved for this one primitive. It is passed
///   per call rather than stored because the caller pushes and pops it far more
///   often than it draws.
///
/// [`fill_rect`](Self::fill_rect) takes no draw clip because its rectangle has
/// already been intersected with the stack — for an axis-aligned quad that
/// intersection *is* the clip, and doing it in the caller lets a fully-clipped
/// fill be skipped before it reaches the backend at all.
pub trait RenderTarget {
    // -- surface ------------------------------------------------------------

    /// Current surface size in pixels, as `(width, height)`.
    fn size(&self) -> (u32, u32);

    /// Resize the surface, discarding its contents.
    ///
    /// # Errors
    ///
    /// [`CompositorError::InvalidDimensions`](crate::CompositorError::InvalidDimensions)
    /// for a zero dimension, or
    /// [`CompositorError::FramebufferTooLarge`](crate::CompositorError::FramebufferTooLarge)
    /// beyond the surface limit.
    fn resize(&mut self, width: u32, height: u32) -> CompositorResult<()>;

    /// Confine every subsequent primitive to `clip`; `None` restores the whole
    /// surface. See the trait docs for how this differs from a draw clip.
    fn set_frame_clip(&mut self, clip: Option<Rect>);

    // -- background ---------------------------------------------------------

    /// Fill the whole surface with `color`, ignoring the frame clip.
    fn clear(&mut self, color: u32);

    /// Fill `rect` with `color`, ignoring the frame clip.
    fn clear_rect(&mut self, rect: &Rect, color: u32);

    /// Fill everything *except* `covered` with `color`, ignoring the frame clip.
    ///
    /// `covered` is the occlusion cull's answer: rectangles that provably get
    /// overwritten with opaque content later in the frame, so clearing them
    /// would be pure overdraw.
    fn clear_except(&mut self, color: u32, covered: &[Rect]);

    // -- primitives ---------------------------------------------------------

    /// A solid axis-aligned quad: `color` over the surface at `opacity`.
    ///
    /// `rect` is final — already intersected with the client's clip stack.
    fn fill_rect(&mut self, rect: Rect, color: u32, opacity: f32);

    /// A one-pixel line from `(x1, y1)` to `(x2, y2)`, clipped to `clip`.
    ///
    /// The endpoints come from a client and are not trusted; an implementation
    /// must not iterate proportionally to their magnitude.
    fn draw_line(
        &mut self,
        x1: i32,
        y1: i32,
        x2: i32,
        y2: i32,
        color: u32,
        opacity: f32,
        clip: Option<&Rect>,
    );

    /// One glyph's coverage mask, tinted `color`, clipped to `clip`.
    ///
    /// `pen`/`baseline` are the pen position on the text baseline; the mask's
    /// own `left`/`top` bearings place it relative to that, and resolving them
    /// here rather than in the caller is what lets a GPU backend keep the
    /// bearings in its atlas metadata instead of baking them into vertices.
    fn draw_glyph(
        &mut self,
        mask: &GlyphMask,
        pen: f32,
        baseline: f32,
        color: u32,
        opacity: f32,
        clip: Option<&Rect>,
    );

    /// A client's shared buffer as a textured quad with its top-left at
    /// `(x, y)`, taking `cols` × `rows` pixels from the buffer's origin.
    ///
    /// The caller has already reduced `cols`/`rows` to the overlap of the
    /// buffer and the window's client area.
    fn blit_buffer(
        &mut self,
        buf: &SharedBuffer,
        x: i32,
        y: i32,
        cols: u32,
        rows: u32,
        opacity: f32,
    );

    // -- presentation -------------------------------------------------------

    /// Finish the frame and make it the one [`presented_pixels`](Self::presented_pixels)
    /// returns.
    fn present(&mut self);

    /// The pixels of the most recently presented frame, row-major ARGB8888.
    fn presented_pixels(&self) -> &[u32];

    /// The pixels currently being composited into, row-major ARGB8888.
    ///
    /// Distinct from [`presented_pixels`](Self::presented_pixels) for a
    /// double-buffered target: this is the frame in progress. Used by the scene
    /// capture and by tests that inspect a composite without presenting it.
    fn working_pixels(&self) -> &[u32];
}

/// The backend a [`Compositor`](crate::Compositor) composites through.
///
/// One variant today. The point of the type is that adding the second one —
/// `Gpu`, once the virtio-gpu render path exists below us — is a change to this
/// file and a new [`RenderTarget`] implementation, not a change to the
/// compositor: nothing above the seam names a `Framebuffer`.
pub enum RenderBackend {
    /// CPU rasterization into a double-buffered [`Framebuffer`].
    Software(Framebuffer),
}

impl RenderBackend {
    /// A software backend over a framebuffer of the given size.
    ///
    /// # Errors
    ///
    /// Propagates [`Framebuffer::new`]'s dimension validation.
    pub fn software(width: u32, height: u32) -> CompositorResult<Self> {
        Ok(Self::Software(Framebuffer::new(width, height)?))
    }

    /// The underlying framebuffer, when this is the software backend.
    ///
    /// The escape hatch for the two things that are genuinely software-only:
    /// tests that read back individual pixels, and the headless present path.
    /// Deliberately an `Option` rather than a field accessor — a caller that
    /// cannot cope with `None` is a caller that would break the moment a GPU
    /// backend existed, and this makes it say so.
    #[must_use]
    pub const fn as_software(&self) -> Option<&Framebuffer> {
        match self {
            Self::Software(fb) => Some(fb),
        }
    }

    /// Mutable counterpart to [`as_software`](Self::as_software).
    #[must_use]
    pub const fn as_software_mut(&mut self) -> Option<&mut Framebuffer> {
        match self {
            Self::Software(fb) => Some(fb),
        }
    }

    /// Surface size in pixels, as `(width, height)`.
    ///
    /// Shadows [`RenderTarget::size`] deliberately: the trait method cannot be
    /// `const`, and this is asked for in const contexts (and in enough hot
    /// paths that a guaranteed-inlined field read is worth having).
    #[must_use]
    pub const fn size(&self) -> (u32, u32) {
        match self {
            Self::Software(fb) => (fb.width, fb.height),
        }
    }

    /// A human-readable name for the active backend, for logs and diagnostics.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Software(_) => "software",
        }
    }
}

// Every method is one match arm and a forward. Written out rather than
// generated by a macro because with one variant the macro would be longer than
// the code, and because the compiler's exhaustiveness check on each arm is what
// will point at this file when a second backend lands.
impl RenderTarget for RenderBackend {
    #[inline]
    fn size(&self) -> (u32, u32) {
        RenderBackend::size(self)
    }

    #[inline]
    fn resize(&mut self, width: u32, height: u32) -> CompositorResult<()> {
        match self {
            Self::Software(fb) => RenderTarget::resize(fb, width, height),
        }
    }

    #[inline]
    fn set_frame_clip(&mut self, clip: Option<Rect>) {
        match self {
            Self::Software(fb) => RenderTarget::set_frame_clip(fb, clip),
        }
    }

    #[inline]
    fn clear(&mut self, color: u32) {
        match self {
            Self::Software(fb) => RenderTarget::clear(fb, color),
        }
    }

    #[inline]
    fn clear_rect(&mut self, rect: &Rect, color: u32) {
        match self {
            Self::Software(fb) => RenderTarget::clear_rect(fb, rect, color),
        }
    }

    #[inline]
    fn clear_except(&mut self, color: u32, covered: &[Rect]) {
        match self {
            Self::Software(fb) => RenderTarget::clear_except(fb, color, covered),
        }
    }

    #[inline]
    fn fill_rect(&mut self, rect: Rect, color: u32, opacity: f32) {
        match self {
            Self::Software(fb) => fb.fill_rect(rect, color, opacity),
        }
    }

    #[inline]
    fn draw_line(
        &mut self,
        x1: i32,
        y1: i32,
        x2: i32,
        y2: i32,
        color: u32,
        opacity: f32,
        clip: Option<&Rect>,
    ) {
        match self {
            Self::Software(fb) => fb.draw_line(x1, y1, x2, y2, color, opacity, clip),
        }
    }

    #[inline]
    fn draw_glyph(
        &mut self,
        mask: &GlyphMask,
        pen: f32,
        baseline: f32,
        color: u32,
        opacity: f32,
        clip: Option<&Rect>,
    ) {
        match self {
            Self::Software(fb) => fb.draw_glyph(mask, pen, baseline, color, opacity, clip),
        }
    }

    #[inline]
    fn blit_buffer(
        &mut self,
        buf: &SharedBuffer,
        x: i32,
        y: i32,
        cols: u32,
        rows: u32,
        opacity: f32,
    ) {
        match self {
            Self::Software(fb) => RenderTarget::blit_buffer(fb, buf, x, y, cols, rows, opacity),
        }
    }

    #[inline]
    fn present(&mut self) {
        match self {
            Self::Software(fb) => RenderTarget::present(fb),
        }
    }

    #[inline]
    fn presented_pixels(&self) -> &[u32] {
        match self {
            Self::Software(fb) => fb.presented_pixels(),
        }
    }

    #[inline]
    fn working_pixels(&self) -> &[u32] {
        match self {
            Self::Software(fb) => fb.working_pixels(),
        }
    }
}

// The five defensive lints the workspace turns on are for production code:
// a test that indexes a fixed-size fixture, or unwraps a value it just
// constructed, is *asserting*, and rewriting that assertion as a `let else`
// only hides which line failed. CLAUDE.md's lint policy says as much.
#[cfg(test)]
#[allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used
)]
mod tests {
    //! Proof that the seam is a seam.
    //!
    //! A one-variant enum forwarding to the only implementation there has ever
    //! been proves nothing on its own: the pipeline above it could still be
    //! quietly software-specific and nobody would find out until a GPU backend
    //! was half-written. So this module supplies a *second* implementation —
    //! one that owns no pixels at all — and drives the real `RenderEngine`
    //! through it.
    //!
    //! What that pins down:
    //!
    //! - the client-command pipeline names no `Framebuffer` and needs none;
    //! - every command that paints arrives as a primitive a GPU could execute
    //!   (a quad, a textured quad, a coverage-mask quad, a line) rather than as
    //!   a stream of pixel writes;
    //! - the clip stack is resolved *above* the seam, so a fully-clipped
    //!   primitive never reaches a backend at all;
    //! - a line is handed over whole, so a client cannot make the compositor
    //!   walk four billion steps on its behalf whichever backend is installed.

    use super::*;
    use crate::RenderEngine;
    use guitk::color::Color;
    use guitk::render::{FontFamily, FontWeightHint, RenderCommand, TextOverflow};
    use guitk::style::CornerRadii;

    /// One primitive as it crossed the seam.
    #[derive(Debug, Clone, PartialEq)]
    enum Primitive {
        FrameClip(Option<Rect>),
        Clear(u32),
        ClearRect(Rect, u32),
        ClearExcept(u32, Vec<Rect>),
        Fill {
            rect: Rect,
            color: u32,
            opacity: f32,
        },
        Line {
            from: (i32, i32),
            to: (i32, i32),
            color: u32,
            clip: Option<Rect>,
        },
        /// A glyph quad, recorded by its extent rather than its coverage — the
        /// mask belongs to the font cache, and what matters here is that one
        /// arrived, placed.
        Glyph {
            pen: f32,
            baseline: f32,
            color: u32,
            size: (u32, u32),
        },
        Blit {
            at: (i32, i32),
            size: (u32, u32),
            opacity: f32,
        },
        Present,
    }

    /// A [`RenderTarget`] that records instead of rasterizing.
    ///
    /// Deliberately *not* a [`RenderBackend`] variant: this is a test double,
    /// and a variant for it would put test scaffolding into the type the
    /// compositor dispatches on. It reaches the same pipeline anyway, because
    /// the pipeline is generic over the trait — which is the property under
    /// test.
    #[derive(Default)]
    struct Recorder {
        ops: Vec<Primitive>,
        /// Nothing is ever composited, so both pixel views are empty. A caller
        /// that reads them gets a consistent (if useless) answer rather than a
        /// panic — the trait promises a slice, not a picture.
        pixels: Vec<u32>,
    }

    impl Recorder {
        fn kinds(&self) -> Vec<&'static str> {
            self.ops
                .iter()
                .map(|op| match op {
                    Primitive::FrameClip(_) => "frame-clip",
                    Primitive::Clear(_) => "clear",
                    Primitive::ClearRect(..) => "clear-rect",
                    Primitive::ClearExcept(..) => "clear-except",
                    Primitive::Fill { .. } => "fill",
                    Primitive::Line { .. } => "line",
                    Primitive::Glyph { .. } => "glyph",
                    Primitive::Blit { .. } => "blit",
                    Primitive::Present => "present",
                })
                .collect()
        }

        fn pens(&self) -> Vec<f32> {
            self.ops
                .iter()
                .filter_map(|op| match op {
                    Primitive::Glyph { pen, .. } => Some(*pen),
                    _ => None,
                })
                .collect()
        }
    }

    impl RenderTarget for Recorder {
        fn size(&self) -> (u32, u32) {
            (800, 600)
        }

        fn resize(&mut self, _width: u32, _height: u32) -> CompositorResult<()> {
            Ok(())
        }

        fn set_frame_clip(&mut self, clip: Option<Rect>) {
            self.ops.push(Primitive::FrameClip(clip));
        }

        fn clear(&mut self, color: u32) {
            self.ops.push(Primitive::Clear(color));
        }

        fn clear_rect(&mut self, rect: &Rect, color: u32) {
            self.ops.push(Primitive::ClearRect(*rect, color));
        }

        fn clear_except(&mut self, color: u32, covered: &[Rect]) {
            self.ops
                .push(Primitive::ClearExcept(color, covered.to_vec()));
        }

        fn fill_rect(&mut self, rect: Rect, color: u32, opacity: f32) {
            self.ops.push(Primitive::Fill {
                rect,
                color,
                opacity,
            });
        }

        fn draw_line(
            &mut self,
            x1: i32,
            y1: i32,
            x2: i32,
            y2: i32,
            color: u32,
            _opacity: f32,
            clip: Option<&Rect>,
        ) {
            self.ops.push(Primitive::Line {
                from: (x1, y1),
                to: (x2, y2),
                color,
                clip: clip.copied(),
            });
        }

        fn draw_glyph(
            &mut self,
            mask: &GlyphMask,
            pen: f32,
            baseline: f32,
            color: u32,
            _opacity: f32,
            _clip: Option<&Rect>,
        ) {
            self.ops.push(Primitive::Glyph {
                pen,
                baseline,
                color,
                size: (mask.width, mask.height),
            });
        }

        fn blit_buffer(
            &mut self,
            _buf: &SharedBuffer,
            x: i32,
            y: i32,
            cols: u32,
            rows: u32,
            opacity: f32,
        ) {
            self.ops.push(Primitive::Blit {
                at: (x, y),
                size: (cols, rows),
                opacity,
            });
        }

        fn present(&mut self) {
            self.ops.push(Primitive::Present);
        }

        fn presented_pixels(&self) -> &[u32] {
            &self.pixels
        }

        fn working_pixels(&self) -> &[u32] {
            &self.pixels
        }
    }

    /// Run a client's command list through the real engine into a recorder,
    /// with the window at (100, 50) and 200x100 of client area.
    fn record(commands: &[RenderCommand]) -> Recorder {
        let mut rec = Recorder::default();
        let mut engine = RenderEngine::new();
        engine.execute(&mut rec, commands, 100, 50, 200, 100, 1.0);
        rec
    }

    fn white() -> Color {
        Color::rgba(255, 255, 255, 255)
    }

    #[test]
    fn a_fill_command_crosses_the_seam_as_one_quad_in_screen_space() {
        let rec = record(&[RenderCommand::FillRect {
            x: 10.0,
            y: 20.0,
            width: 30.0,
            height: 40.0,
            color: white(),
            corner_radii: CornerRadii::ZERO,
        }]);
        // One primitive, already translated by the window origin and already
        // intersected with the client area — the backend is told *where*, not
        // asked to work it out.
        assert_eq!(
            rec.ops,
            vec![Primitive::Fill {
                rect: Rect::new(110, 70, 30, 40),
                color: 0xFF_FF_FF_FF,
                opacity: 1.0,
            }]
        );
    }

    #[test]
    fn a_quad_the_clip_stack_rejects_never_reaches_the_backend() {
        // The clip is a strip on the left; the fill is on the right. Under a
        // seam that passed the clip down instead, this would still be a call
        // the backend had to reason about — on a GPU, a state change and a draw
        // call for nothing.
        let rec = record(&[
            RenderCommand::PushClip {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 100.0,
            },
            RenderCommand::FillRect {
                x: 50.0,
                y: 0.0,
                width: 20.0,
                height: 20.0,
                color: white(),
                corner_radii: CornerRadii::ZERO,
            },
            RenderCommand::PopClip,
        ]);
        assert!(
            rec.ops.is_empty(),
            "a fully-clipped fill reached the backend: {:?}",
            rec.ops
        );
    }

    #[test]
    fn a_stroke_is_four_quads_rather_than_an_outline_the_backend_must_interpret() {
        let rec = record(&[RenderCommand::StrokeRect {
            x: 0.0,
            y: 0.0,
            width: 50.0,
            height: 50.0,
            color: white(),
            line_width: 2.0,
            corner_radii: CornerRadii::ZERO,
        }]);
        assert_eq!(rec.kinds(), vec!["fill"; 4]);
    }

    #[test]
    fn a_line_crosses_the_seam_whole_with_its_clip() {
        // The endpoints are a client's and are absurd on purpose: exactly one
        // primitive crosses regardless, so the walk — and the bound on it — is
        // the backend's business. A seam drawn at the pixel would have handed
        // over a step per pixel, and a hostile client would pick how many.
        let rec = record(&[RenderCommand::Line {
            x1: 0.0,
            y1: 0.0,
            x2: 2.0e9,
            y2: 1.0,
            color: white(),
            width: 1.0,
        }]);
        assert_eq!(rec.ops.len(), 1);
        let Some(Primitive::Line { from, clip, .. }) = rec.ops.first() else {
            panic!("expected one line primitive, got {:?}", rec.ops);
        };
        assert_eq!(*from, (100, 50));
        assert_eq!(*clip, Some(Rect::new(100, 50, 200, 100)));
    }

    #[test]
    fn text_crosses_the_seam_as_one_coverage_quad_per_drawn_glyph() {
        let rec = record(&[RenderCommand::Text {
            x: 0.0,
            y: 0.0,
            text: "abc".to_string(),
            color: white(),
            font_size: 16.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        }]);
        assert_eq!(rec.kinds(), vec!["glyph"; 3]);
        // Pens advance left to right: the engine shaped the run and handed over
        // *placed* masks, which is what a glyph atlas wants and what a
        // per-pixel seam would have thrown away.
        let pens = rec.pens();
        assert!(
            pens.windows(2).all(|w| w[1] > w[0]),
            "glyph pens not monotonically advancing: {pens:?}"
        );
    }

    #[test]
    fn a_command_that_paints_nothing_crosses_nothing() {
        // Clip/translate/font pushes are compositor-side state. A backend that
        // had to track them would be a backend that could disagree with the
        // engine about where the next quad goes.
        let rec = record(&[
            RenderCommand::PushTranslate { dx: 5.0, dy: 5.0 },
            RenderCommand::PushFont {
                family: FontFamily::Ui,
            },
            RenderCommand::PopFont,
            RenderCommand::PopTranslate,
        ]);
        assert!(rec.ops.is_empty(), "{:?}", rec.ops);
    }

    #[test]
    fn the_software_backend_reports_itself_and_keeps_its_double_buffer() {
        let mut backend = RenderBackend::software(64, 32).expect("software backend");
        assert_eq!(backend.name(), "software");
        assert_eq!(RenderBackend::size(&backend), (64, 32));

        // A composite into the working frame is invisible in the presented one
        // until `present`. Double buffering is this backend's property, and the
        // seam preserves it rather than papering over it.
        backend.fill_rect(Rect::new(0, 0, 64, 32), 0xFF_11_22_33, 1.0);
        assert_eq!(backend.working_pixels().first(), Some(&0xFF_11_22_33));
        assert_eq!(backend.presented_pixels().first(), Some(&0xFF_00_00_00));
        backend.present();
        assert_eq!(backend.presented_pixels().first(), Some(&0xFF_11_22_33));

        assert!(backend.as_software().is_some());
        assert!(backend.as_software_mut().is_some());
    }

    #[test]
    fn a_zero_sized_software_backend_is_refused_at_construction() {
        assert!(RenderBackend::software(0, 32).is_err());
        assert!(RenderBackend::software(64, 0).is_err());
    }
}
