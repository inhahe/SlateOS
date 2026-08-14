//! A font face pinned to one pixel size, with a glyph cache.
//!
//! [`sfnt::Face`] answers questions in *font units* and [`raster`] turns one
//! outline into one mask. Neither is what a caller wants: a toolkit wants to
//! say "draw this string at 13 px" and have it happen, without re-flattening
//! the same 'e' for every word on the screen. [`ScaledFont`] is that layer.
//!
//! # Why the cache is behind `&mut self`
//!
//! Drawing mutates the cache, and the signature says so. The obvious
//! alternative — `&self` plus a `RefCell`/`Mutex` inside — buys the ability
//! to share one `ScaledFont` immutably at the cost of either giving up `Sync`
//! or paying for a lock on every glyph. Callers that genuinely share a font
//! across threads already have a lock around their draw state; callers that
//! don't (the common case: one cache per rendering thread) should not pay for
//! one. So the type stays a plain owned value and the borrow checker enforces
//! the exclusion.
//!
//! # Coordinates
//!
//! Everything here is in pixels with y increasing downward, matching the
//! framebuffer. A glyph's `top` is relative to the baseline, so it is
//! negative for the part of a glyph above the baseline — the usual
//! convention, and the one [`raster::GlyphMask`] already uses.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;

use crate::FontMetrics;
use crate::gsub::SubGlyph;
use crate::joining::{self, Form};
use crate::norm;
use crate::raster::{GlyphMask, rasterize};
use crate::script::{self, ScriptTags};
use crate::sfnt::{Face, PathCmd, SfntError};
use crate::shape::{GlyphKey, ShapedGlyph, ShapedRun, TAB_WIDTH_IN_SPACES};

/// How many rasterized glyphs one [`ScaledFont`] keeps before it starts
/// evicting.
///
/// A screenful of Latin text touches on the order of 100 distinct glyphs;
/// 512 leaves room for punctuation, accents and a little CJK without letting
/// a hostile string (say, a scroll through every codepoint in a CJK face)
/// grow the cache without bound. At 13 px a cached mask is a couple of
/// hundred bytes, so the ceiling is well under a megabyte.
pub const GLYPH_CACHE_LIMIT: usize = 512;

/// Why a [`ScaledFont`] could not be built or could not draw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScaledFontError {
    /// The underlying face could not be read.
    Sfnt(SfntError),
    /// The requested pixel size is zero, negative, or not a number.
    InvalidSize,
}

impl From<SfntError> for ScaledFontError {
    fn from(e: SfntError) -> Self {
        Self::Sfnt(e)
    }
}

impl core::fmt::Display for ScaledFontError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Sfnt(e) => write!(f, "{e}"),
            Self::InvalidSize => f.write_str("pixel size must be finite and positive"),
        }
    }
}

impl core::error::Error for ScaledFontError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            Self::Sfnt(e) => Some(e),
            Self::InvalidSize => None,
        }
    }
}

/// One glyph, rasterized and ready to blit.
#[derive(Clone, Debug)]
pub struct Glyph {
    /// Anti-aliased coverage and its offset from the pen position.
    pub mask: GlyphMask,
    /// How far the pen advances after drawing this glyph, in pixels.
    pub advance: f32,
}

/// A face pinned to a pixel size, caching the glyphs it has drawn.
pub struct ScaledFont {
    /// Shared because a UI wants the same face at several sizes at once — a
    /// label, a title, a tooltip — and a `Face` owns the whole font file.
    /// Holding it by value meant a megabyte of `Vec<u8>` per size, and
    /// re-parsing the tables each time to get it.
    face: Arc<Face>,
    px_per_em: f32,
    scale: f32,
    metrics: FontMetrics,
    /// Keyed by glyph id, not character: two characters that map to the same
    /// glyph (and there are many — the space-like codepoints, the various
    /// hyphens in some faces) must not each get their own entry.
    cache: BTreeMap<u16, Glyph>,
    /// Insertion order, for eviction. A true LRU would need to reorder on
    /// every hit; text rendering hits the same small working set over and
    /// over, so first-in-first-out evicts almost the same entries for none of
    /// the bookkeeping.
    order: Vec<u16>,
}

impl core::fmt::Debug for ScaledFont {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ScaledFont")
            .field("px_per_em", &self.px_per_em)
            .field("units_per_em", &self.face.units_per_em())
            .field("num_glyphs", &self.face.num_glyphs())
            .field("cached", &self.cache.len())
            // The face bytes and the masks are megabytes; naming them here
            // would make `{:?}` on a font unusable.
            .finish_non_exhaustive()
    }
}

impl ScaledFont {
    /// Pin `face` to `px_per_em` pixels per em.
    ///
    /// # Errors
    ///
    /// [`ScaledFontError::InvalidSize`] if `px_per_em` is not finite and
    /// positive.
    pub fn new(face: Face, px_per_em: f32) -> Result<Self, ScaledFontError> {
        Self::shared(Arc::new(face), px_per_em)
    }

    /// Pin an already-parsed, shared `face` to `px_per_em` pixels per em.
    ///
    /// This is the constructor a font cache wants: parsing a face is the
    /// expensive part and its result is immutable, so several sizes of the same
    /// family should share one. Only the rasterized glyphs differ per size, and
    /// those are this type's own.
    ///
    /// # Errors
    ///
    /// [`ScaledFontError::InvalidSize`] if `px_per_em` is not finite and
    /// positive.
    pub fn shared(face: Arc<Face>, px_per_em: f32) -> Result<Self, ScaledFontError> {
        if !px_per_em.is_finite() || px_per_em <= 0.0 {
            return Err(ScaledFontError::InvalidSize);
        }
        let scale = face.scale_for_px(px_per_em);
        let metrics = Self::derive_metrics(&face, scale);
        Ok(Self {
            face,
            px_per_em,
            scale,
            metrics,
            cache: BTreeMap::new(),
            order: Vec::new(),
        })
    }

    /// The face these glyphs come from, for sharing with another size.
    #[must_use]
    pub fn shared_face(&self) -> Arc<Face> {
        Arc::clone(&self.face)
    }

    /// Read a font file and pin it to a size in one step.
    ///
    /// # Errors
    ///
    /// Whatever [`Face::parse`] rejects the file with, or
    /// [`ScaledFontError::InvalidSize`].
    pub fn from_bytes(data: Vec<u8>, px_per_em: f32) -> Result<Self, ScaledFontError> {
        Self::new(Face::parse(data)?, px_per_em)
    }

    /// Translate the face's font-unit metrics into pixels.
    ///
    /// `cap_height` and `x_height` are measured from the outlines of 'H' and
    /// 'x' rather than read from `OS/2`, because `OS/2` is optional, is
    /// frequently absent from older and from bare `glyf` faces, and carries
    /// zeros in those fields in plenty of the faces that do have it. The
    /// outline is the ground truth and we already have a parser for it.
    fn derive_metrics(face: &Face, scale: f32) -> FontMetrics {
        let fm = face.metrics();
        let ascent = f32::from(fm.ascender) * scale;
        // `descender` is negative per spec; `FontMetrics::descent` is
        // positive-downward, so flip it.
        let descent = -f32::from(fm.descender) * scale;
        #[allow(clippy::cast_precision_loss)]
        // line_height() is a sum of three i16s, far inside f32's exact range.
        let line_height = fm.line_height() as f32 * scale;
        let max_advance = f32::from(fm.advance_width_max) * scale;

        let cap_height = Self::glyph_top(face, 'H', scale).unwrap_or(ascent * 0.7);
        let x_height = Self::glyph_top(face, 'x', scale).unwrap_or(ascent * 0.5);
        let average_advance = Self::average_advance(face, scale);

        FontMetrics {
            ascent,
            descent,
            line_height,
            max_advance,
            average_advance,
            cap_height,
            x_height,
        }
    }

    /// Height of `ch`'s outline above the baseline, in pixels.
    ///
    /// `None` when the face has no such glyph or the glyph is blank, so the
    /// caller can fall back rather than record a height of zero.
    fn glyph_top(face: &Face, ch: char, scale: f32) -> Option<f32> {
        let gid = face.glyph_index(ch)?;
        let outline = face.outline(gid).ok()?;
        let mut top = f32::NEG_INFINITY;
        let mut note = |y: f32| {
            if y.is_finite() {
                top = top.max(y);
            }
        };
        for cmd in &outline.commands {
            // A curve's control points are an upper bound on the curve, never
            // points on it, so only the endpoint counts — which is why every
            // arm below looks at the endpoint and nothing else.
            match *cmd {
                PathCmd::MoveTo(p)
                | PathCmd::LineTo(p)
                | PathCmd::QuadTo(_, p)
                | PathCmd::CurveTo(_, _, p) => note(p.y),
                PathCmd::Close => {}
            }
        }
        (top > 0.0).then_some(top * scale)
    }

    /// Mean advance across the characters ordinary text is mostly made of.
    ///
    /// Averaging over *every* glyph would be dominated by whatever exotica
    /// the face happens to include; averaging over lowercase Latin plus space
    /// is what a caller asking "how wide is a character, roughly" means.
    fn average_advance(face: &Face, scale: f32) -> f32 {
        let mut total = 0.0_f32;
        let mut count = 0.0_f32;
        for ch in "abcdefghijklmnopqrstuvwxyz ".chars() {
            let Some(gid) = face.glyph_index(ch) else {
                continue;
            };
            let Ok(adv) = face.advance(gid) else { continue };
            total += f32::from(adv);
            count += 1.0;
        }
        if count == 0.0 {
            // A face with no Latin lowercase at all — a CJK or symbol face.
            // Its declared maximum is a better guess than zero.
            return f32::from(face.metrics().advance_width_max) * scale;
        }
        total / count * scale
    }

    /// The size this font was pinned to.
    #[must_use]
    pub fn px_per_em(&self) -> f32 {
        self.px_per_em
    }

    /// Pixel-space metrics for the face.
    #[must_use]
    pub fn metrics(&self) -> &FontMetrics {
        &self.metrics
    }

    /// The underlying face, for callers that need font-unit data.
    #[must_use]
    pub fn face(&self) -> &Face {
        &self.face
    }

    /// How many glyphs are currently rasterized and held.
    #[must_use]
    pub fn cached_glyphs(&self) -> usize {
        self.cache.len()
    }

    /// Drop every cached glyph. Useful when memory is tight.
    pub fn clear_cache(&mut self) {
        self.cache.clear();
        self.order.clear();
    }

    /// The glyph id `ch` maps to, or glyph 0 (`.notdef`) if the face has no
    /// mapping for it.
    ///
    /// Substituting `.notdef` rather than returning `None` is deliberate:
    /// `.notdef` is the empty box every font draws for "I don't have this",
    /// which is exactly what should appear on screen, and it keeps the
    /// advance non-zero so the rest of the line does not collapse onto it.
    #[must_use]
    pub fn glyph_id(&self, ch: char) -> u16 {
        self.face.glyph_index(ch).unwrap_or(0)
    }

    /// Rasterize `gid` at this font's size, or return the cached result.
    ///
    /// # Errors
    ///
    /// [`ScaledFontError::Sfnt`] if the glyph cannot be decoded. A glyph that
    /// decodes but rasterizes to nothing (a space, a zero-area contour) is
    /// not an error — it yields an empty mask.
    pub fn glyph(&mut self, gid: u16) -> Result<&Glyph, ScaledFontError> {
        if !self.cache.contains_key(&gid) {
            let entry = self.rasterize_glyph(gid)?;
            self.insert(gid, entry);
        }
        self.cache
            .get(&gid)
            .ok_or(ScaledFontError::Sfnt(SfntError::GlyphOutOfRange))
    }

    /// The coverage bitmap for one glyph of a [`ShapedRun`] this font
    /// produced.
    ///
    /// `None` when the glyph cannot be decoded, which is not something a
    /// caller can act on — a run is drawn glyph by glyph and one that will not
    /// rasterize is skipped, the pen still advancing so the rest of the line
    /// stays where it was measured to be.
    ///
    /// This is the only way to get from a [`GlyphKey`] to pixels, and it is
    /// deliberately the only way: the key is opaque so that a caller cannot
    /// start treating an outline run and a bitmap run differently. See
    /// [`SystemFont::glyph_mask`](crate::system::SystemFont::glyph_mask) for
    /// the backend-agnostic form.
    pub fn glyph_mask(&mut self, key: GlyphKey) -> Option<&GlyphMask> {
        Some(&self.glyph(key.gid()).ok()?.mask)
    }

    fn rasterize_glyph(&self, gid: u16) -> Result<Glyph, ScaledFontError> {
        let outline = self.face.outline(gid)?;
        let advance = f32::from(self.face.advance(gid)?) * self.scale;
        // Every rasterizer failure is swallowed, and deliberately: a glyph
        // whose outline is absurd or malformed still has to occupy its
        // advance, or every following glyph on the line shifts left. Draw
        // nothing, keep the space — exactly what a blank glyph does.
        // (`InvalidScale` cannot occur at all here; `new` validated it.)
        let mask = rasterize(&outline, self.scale).unwrap_or_default();
        Ok(Glyph { mask, advance })
    }

    fn insert(&mut self, gid: u16, glyph: Glyph) {
        if self.cache.len() >= GLYPH_CACHE_LIMIT {
            // Evict the oldest still-present entry. The loop (rather than a
            // single `remove(0)`) skips ids already gone via `clear_cache`.
            while !self.order.is_empty() {
                let victim = self.order.remove(0);
                if self.cache.remove(&victim).is_some() {
                    break;
                }
            }
        }
        self.cache.insert(gid, glyph);
        self.order.push(gid);
    }

    /// The gap to add between two glyphs, in pixels, on top of the first
    /// one's advance. Negative for the pairs that need pulling together.
    ///
    /// Separate from `glyph` because a caller that draws one glyph at a time
    /// (the compositor does, so that it can blend coverage through its own
    /// clip stack) has to be able to ask about a pair without giving up the
    /// per-glyph loop. Callers that use `measure` or `draw_text` get it
    /// applied for them.
    #[must_use]
    pub fn kern(&self, left: u16, right: u16) -> f32 {
        self.kern_across(left, right, &[])
    }

    /// The same, for a pair with `between` standing between them — the marks a
    /// face's "ignore marks" kerning is meant to be read across.
    #[must_use]
    pub fn kern_across(&self, left: u16, right: u16, between: &[u16]) -> f32 {
        f32::from(self.face.kern_across(left, right, between)) * self.scale
    }

    /// The glyphs `text` turns into, with final advances.
    ///
    /// Everything that walks text goes through here — measuring, drawing,
    /// hit-testing, truncating — so that they cannot come to different
    /// answers about the same string. See [`shape`](crate::shape) for why
    /// that is not a theoretical concern.
    ///
    /// Does not rasterize anything and does not touch the glyph cache: this
    /// needs `cmap`, `hmtx` and the layout tables only, which is what lets a
    /// widget measure its label without paying to draw it.
    #[must_use]
    pub fn shape(&self, text: &str) -> ShapedRun {
        // Five passes, because each one needs all of the previous one's
        // output. Normalization settles *which characters there are* and so
        // must finish before any of them is looked up in `cmap`; `GSUB`
        // decides which glyphs there are, and cannot run while characters are
        // still arriving; kerning applies to the glyphs that *survive*
        // substitution, so `fi` must be kerned as the single glyph it became,
        // not as the `f` and `i` it was; and a mark's placement is measured
        // from a pen that kerning is still moving.
        let space = self.glyph_id(' ');
        let pieces = norm::pieces(text, |ch| self.face.glyph_index(ch).is_some());
        // Which cursive form each character takes, decided from the characters
        // rather than the glyphs because it is a property of the *text*: what
        // a letter joins to does not depend on which face is drawing it. Empty
        // for text that does not join, which is nearly all of it.
        let mut forms: Vec<Option<Form>> = Vec::new();
        joining::forms(&pieces, &mut forms);
        let mut glyphs: Vec<SubGlyph> = Vec::with_capacity(pieces.len());
        let mut tabs: Vec<bool> = Vec::with_capacity(pieces.len());
        for (i, &(ch, cluster)) in pieces.iter().enumerate() {
            // A tab has no glyph. Drawn through `cmap` it comes out as the
            // missing-glyph box, one space wide; the width every caller wants
            // is several spaces of nothing. Substituting the space glyph gets
            // both — it draws blank, and its advance is the unit to multiply.
            let tab = ch == '\t';
            let gid = if tab { space } else { self.glyph_id(ch) };
            glyphs.push(SubGlyph::cursive(
                gid,
                cluster,
                forms.get(i).copied().flatten(),
            ));
            tabs.push(tab);
        }

        if self.face.has_substitutions() {
            // Glyphs are still one per piece here, so a run boundary counted
            // in pieces is a boundary counted in glyphs. That stops being true
            // the moment anything ligates, which is why the split happens now.
            self.substitute_runs(&script::runs(&pieces), &mut glyphs, &mut tabs);
        }

        let marked = self.face.has_marks();
        let mut out: Vec<ShapedGlyph> = Vec::with_capacity(glyphs.len());
        // Where in `out` the left half of the next kerning pair sits, and the
        // glyphs standing between it and the position being filled. A tab is
        // never a left half: its advance is a layout decision, not a glyph
        // width, and a face that kerns after a space would quietly narrow it.
        let mut kern_left: Option<usize> = None;
        let mut between: Vec<u16> = Vec::new();
        for (i, glyph) in glyphs.iter().enumerate() {
            let tab = tabs.get(i).copied().unwrap_or(false);
            let gid = glyph.gid;
            // A combining mark is not part of the spacing, and real faces mark
            // their kerning lookups "ignore marks" so that `A` and `V` still
            // kern with an accent between them. The mark goes into `between`
            // and the face decides from its own lookup flags whether to read
            // across it; kerning *against* the mark instead would shove the
            // accent off the letter it belongs to.
            let mark = marked && !tab && self.face.is_mark(gid);
            // Kerning is part of the width, not a drawing-time flourish: a
            // measurement that leaves it out is one that disagrees with what
            // the compositor puts on the screen, which is how a label ends up
            // centred half a pixel off in every button on the desktop. It is
            // charged to the pair's *left* glyph — not to whatever was pushed
            // last — so that the advances still sum to the run's width when
            // the pair was read across a mark.
            if !tab
                && !mark
                && let Some(last) = kern_left.and_then(|at| out.get_mut(at))
            {
                let kern = self.kern_across(last.key.gid(), gid, &between);
                last.advance += kern;
                last.kern_next = kern;
            }
            let advance = self
                .face
                .advance(gid)
                .map_or(0.0, |a| f32::from(a) * self.scale);
            out.push(ShapedGlyph {
                key: GlyphKey::outline(gid),
                // Substitution carried this along: a ligature reports its
                // first component's byte offset, so a caret or a truncation
                // can land before or after it but never inside it — there is
                // no boundary there to find.
                cluster: glyph.cluster,
                // A combining mark takes no room, whatever `hmtx` says. Many
                // faces give U+0301 a real advance — Segoe UI's is over half
                // an `e` — because the same outline doubles as the spacing
                // acute; honouring it would put a gap after every accented
                // letter and make `é` measure wider than `e`. HarfBuzz zeroes
                // mark advances for the same reason.
                advance: if mark {
                    0.0
                } else if tab {
                    advance * TAB_WIDTH_IN_SPACES
                } else {
                    advance
                },
                kern_next: 0.0,
                offset: (0.0, 0.0),
            });
            if mark {
                // Keep the mark in the run between the pair, but only while
                // there is a pair to read across: a mark with no letter before
                // it starts nothing.
                if kern_left.is_some() {
                    between.push(gid);
                }
            } else {
                between.clear();
                // A tab ends the pair rather than starting one, so the glyph
                // after it kerns against nothing.
                kern_left = (!tab).then(|| out.len().saturating_sub(1));
            }
        }

        if marked {
            self.attach_marks(&mut out);
        }
        ShapedRun::new(out)
    }

    /// Substitute each stretch of `glyphs` between tabs and script changes,
    /// separately and each under its own script.
    ///
    /// Two boundaries, for two reasons.
    ///
    /// A substitution may not reach across a **tab**. The tab is not a glyph
    /// the font knows about, so joining what sits either side of it would
    /// silently swallow the gap it exists to make — and a `GSUB` lookup, which
    /// is handed a whole run and matches anywhere in it, has no way to be told
    /// about a boundary except by not being shown across it.
    ///
    /// A substitution may not reach across a **script change** either, and
    /// here the boundary is not only about reach but about *which rules*: the
    /// features on each side are chosen by different script tags, and a face
    /// that registers both Arabic and Latin has two features called `liga`
    /// that mean different things. Shaping the whole string under one script —
    /// which is what a single call would do, and what HarfBuzz's
    /// `guess_segment_properties` does — applies one writing system's rules to
    /// the other's half of the string.
    ///
    /// `runs` is [`script::runs`]'s output over the pieces these glyphs came
    /// from, so its ends are glyph indices for as long as nothing has ligated
    /// yet — which is why this is the pass that consumes them.
    ///
    /// Both vectors are rewritten, since a run that ligates comes out shorter
    /// and `tabs` has to keep lining up with it.
    fn substitute_runs(
        &self,
        runs: &[(usize, Option<ScriptTags>)],
        glyphs: &mut Vec<SubGlyph>,
        tabs: &mut Vec<bool>,
    ) {
        let mut out: Vec<SubGlyph> = Vec::with_capacity(glyphs.len());
        let mut out_tabs: Vec<bool> = Vec::with_capacity(tabs.len());
        let mut run: Vec<SubGlyph> = Vec::new();
        // Which script run `i` falls in. Advanced rather than searched: `i`
        // only moves forward, and there are a handful of runs at most.
        let mut at = 0usize;
        // The script the *open* stretch was collected under, which is not the
        // script at `i` once a boundary has been crossed. Keeping it separate
        // is what makes a stretch shaped under the script that opened it.
        let mut open: Option<ScriptTags> = None;
        // One past the end, where there is no glyph and `tabs` reads `true`,
        // so that the last stretch is flushed by the same code as every
        // stretch a tab ends — including the whole of a run with no tabs at
        // all, which is nearly every run.
        for i in 0..=glyphs.len() {
            // Ends are exclusive: `end == i` means the run stopped *before*
            // this glyph, so the stretch closes and `i` opens the next one.
            if runs.get(at).is_some_and(|&(end, _)| end <= i) {
                if !run.is_empty() {
                    self.face.substitute(open, &mut run);
                    out_tabs.extend(core::iter::repeat_n(false, run.len()));
                    out.append(&mut run);
                }
                while runs.get(at).is_some_and(|&(end, _)| end <= i) {
                    at = at.saturating_add(1);
                }
            }
            open = runs.get(at).and_then(|&(_, script)| script);
            if !tabs.get(i).copied().unwrap_or(true) {
                if let Some(glyph) = glyphs.get(i) {
                    run.push(*glyph);
                }
                continue;
            }
            if !run.is_empty() {
                self.face.substitute(open, &mut run);
                out_tabs.extend(core::iter::repeat_n(false, run.len()));
                out.append(&mut run);
            }
            if let Some(glyph) = glyphs.get(i) {
                out.push(*glyph);
                out_tabs.push(true);
            }
        }
        *glyphs = out;
        *tabs = out_tabs;
    }

    /// Displace every combining mark in `glyphs` onto the glyph it belongs to.
    ///
    /// Runs after advances are final, because a mark's placement is expressed
    /// relative to its base glyph's *origin* while the mark is drawn at the
    /// pen — and the distance between those two is the sum of the advances in
    /// between, kerning included.
    ///
    /// Marks whose face offers no anchor for them are left at the pen. That
    /// is visibly wrong, but it is wrong in the way the font asked for: the
    /// alternative is inventing a placement, which would be wrong in a way
    /// nobody could trace back to the font.
    fn attach_marks(&self, glyphs: &mut [ShapedGlyph]) {
        // Where each glyph's pen sits, which is what an offset is measured
        // against.
        let mut pen = 0.0f32;
        let mut pens: Vec<f32> = Vec::with_capacity(glyphs.len());
        for glyph in glyphs.iter() {
            pens.push(pen);
            pen += glyph.advance;
        }

        // What a mark attaches to: the last ordinary glyph for the first mark
        // of a stack, the mark before it for the rest.
        let mut base: Option<usize> = None;
        let mut stacked: Option<usize> = None;
        for i in 0..glyphs.len() {
            let Some(gid) = glyphs.get(i).map(|g| g.key.gid()) else {
                break;
            };
            if !self.face.is_mark(gid) {
                base = Some(i);
                stacked = None;
                continue;
            }
            // Stacking first: the second accent of a pair belongs above the
            // first, not on the letter. A face with `mark` but no `mkmk` falls
            // back to the base, which puts both accents in one place — its
            // own tables ask for nothing better.
            let onto = stacked
                .and_then(|at| {
                    let below = glyphs.get(at)?.key.gid();
                    Some((at, self.face.mark_on_mark(below, gid)?))
                })
                .or_else(|| {
                    let at = base?;
                    let under = glyphs.get(at)?.key.gid();
                    Some((at, self.face.mark_on_base(under, gid)?))
                });
            if let Some((at, (dx, dy))) = onto {
                // The anchor glyph may itself be a displaced mark, so start
                // from where it actually ended up rather than from its pen.
                let from = glyphs.get(at).map_or((0.0, 0.0), |g| g.offset);
                let back =
                    pens.get(at).copied().unwrap_or(0.0) - pens.get(i).copied().unwrap_or(0.0);
                if let Some(glyph) = glyphs.get_mut(i) {
                    glyph.offset = (
                        from.0 + f32::from(dx) * self.scale + back,
                        from.1 + f32::from(dy) * self.scale,
                    );
                }
            }
            stacked = Some(i);
        }
    }

    /// Width of `text` in pixels, ignoring line breaks.
    #[must_use]
    pub fn measure(&self, text: &str) -> f32 {
        self.shape(text).width()
    }

    /// Break `text` into lines no wider than `max_width`, at whitespace.
    ///
    /// A single word longer than `max_width` is left on its own over-long
    /// line rather than being cut mid-word: breaking inside a word is a
    /// per-script decision (it is wrong for Latin, required for CJK) that
    /// belongs to a real line breaker, not here.
    #[must_use]
    pub fn wrap(&self, text: &str, max_width: f32) -> Vec<String> {
        let mut lines = Vec::new();
        for para in text.split('\n') {
            let mut line = String::new();
            for word in para.split(' ') {
                if line.is_empty() {
                    line.push_str(word);
                    continue;
                }
                let mut candidate = line.clone();
                candidate.push(' ');
                candidate.push_str(word);
                if self.measure(&candidate) <= max_width {
                    line = candidate;
                } else {
                    lines.push(core::mem::take(&mut line));
                    line.push_str(word);
                }
            }
            lines.push(line);
        }
        lines
    }

    /// Draw `text` with its baseline at `y`, starting at pen position `x`.
    ///
    /// Returns the pen position after the last glyph, so callers can chain
    /// runs (a bold span after a regular one, say) without re-measuring.
    ///
    /// `color` is `0xAARRGGBB`. Each glyph's coverage is multiplied into the
    /// alpha, so a fully opaque colour still anti-aliases.
    pub fn draw_text(&mut self, text: &str, target: &mut Target<'_>, x: f32, y: f32) -> f32 {
        let mut pen = x;
        // Shaped up front, and into a local, because the loop needs `&mut
        // self` to rasterize while it walks the run.
        let run = self.shape(text);
        for shaped in run.glyphs() {
            let advance = shaped.advance;
            let Ok(glyph) = self.glyph(shaped.key.gid()) else {
                pen += advance;
                continue;
            };
            // A mask's left/top come from a bitmap bounded by
            // `MAX_GLYPH_PIXELS`, so they are small integers; the pen and
            // baseline are caller-supplied and may be anything, which is why
            // the sum goes through `pixel_coord` (which rejects the
            // degenerate cases) and then `blit_mask` (which clips the rest).
            // `offset` is zero except on an attached combining mark, and its
            // `y` points up where the screen's points down.
            #[allow(clippy::cast_precision_loss)]
            let placed = (
                pixel_coord(pen + shaped.offset.0 + glyph.mask.left as f32),
                pixel_coord(y - shaped.offset.1 + glyph.mask.top as f32),
            );
            if let (Some(gx), Some(gy)) = placed {
                blit_mask(&glyph.mask, target, gx, gy);
            }
            pen += advance;
        }
        pen
    }
}

/// An ARGB surface to draw into, plus the colour to draw with.
///
/// Bundled into one struct because the alternative is a seven-argument
/// function that is easy to call wrongly — `stride` and `height` are both
/// `u32` and transposing them silently corrupts memory bounds.
pub struct Target<'a> {
    /// The pixel buffer, `0xAARRGGBB` per pixel, row-major.
    pub buffer: &'a mut [u32],
    /// Pixels per row. May exceed the visible width.
    pub stride: u32,
    /// Rows in the buffer.
    pub height: u32,
    /// Foreground colour, `0xAARRGGBB`.
    pub color: u32,
}

/// A colour split into 8-bit channels, so the blend loop does not have to
/// re-extract them for every pixel.
#[derive(Clone, Copy)]
struct Channels {
    red: u32,
    green: u32,
    blue: u32,
}

impl Channels {
    fn from_argb(argb: u32) -> Self {
        Self {
            red: (argb >> 16) & 0xFF,
            green: (argb >> 8) & 0xFF,
            blue: argb & 0xFF,
        }
    }
}

/// Blend one 8-bit channel: `src` over `dst` at `alpha`/255.
///
/// Every input is masked to 8 bits by its caller, so the weighted sum
/// `src * alpha + dst * (255 - alpha)` peaks at `65_025` — far inside a
/// `u32`, so it cannot overflow. The saturating
/// forms state that invariant in the code rather than leaving the reader to
/// re-derive it.
fn blend_channel(src: u32, dst: u32, alpha: u32) -> u32 {
    let inv = 255_u32.saturating_sub(alpha);
    src.saturating_mul(alpha)
        .saturating_add(dst.saturating_mul(inv))
        / 255
}

/// Converts a pen position to a whole-pixel coordinate, or `None` when it is
/// not a real number.
///
/// Rust's `as` maps NaN to `0` and saturates infinities, so feeding a
/// degenerate layout straight into `blit_mask` would stamp the text at the
/// origin instead of dropping it — a corrupt or uninitialised position would
/// paint garbage over the top-left of the screen rather than doing nothing
/// visible, which is far harder to diagnose. Infinities are excluded too:
/// they saturate to `i32::MIN`/`MAX`, which clips correctly today only
/// because `blit_mask` adds to them with `saturating_add`.
pub(crate) fn pixel_coord(v: f32) -> Option<i32> {
    if !v.is_finite() {
        return None;
    }
    // Guarded by `is_finite` above, and the range check keeps the cast from
    // saturating, so the truncation is exactly the intended floor-toward-zero.
    #[allow(clippy::cast_possible_truncation)]
    let clamped = (v >= -2_147_483_648.0 && v <= 2_147_483_647.0).then_some(v as i32);
    clamped
}

/// Blend one anti-aliased mask onto a surface at `(x, y)`.
///
/// `x` and `y` are the mask's top-left corner in surface coordinates and may
/// be negative; anything outside the surface is clipped.
pub fn blit_mask(mask: &GlyphMask, target: &mut Target<'_>, x: i32, y: i32) {
    let src_alpha = (target.color >> 24) & 0xFF;
    if src_alpha == 0 {
        return;
    }
    let src = Channels::from_argb(target.color);
    let max_y = i32::try_from(target.height).unwrap_or(i32::MAX);
    let max_x = i32::try_from(target.stride).unwrap_or(i32::MAX);

    for row in 0..mask.height {
        let Ok(dy) = i32::try_from(row) else { break };
        let py = y.saturating_add(dy);
        if py < 0 || py >= max_y {
            continue;
        }
        for col in 0..mask.width {
            let coverage = u32::from(mask.at(col, row));
            if coverage == 0 {
                continue;
            }
            let Ok(dx) = i32::try_from(col) else { break };
            let px = x.saturating_add(dx);
            if px < 0 || px >= max_x {
                continue;
            }
            // Coverage scales the colour's own alpha: a 50%-covered pixel of
            // a 50%-transparent colour is 25% opaque.
            let alpha = src_alpha.saturating_mul(coverage) / 255;
            if alpha == 0 {
                continue;
            }
            let (Ok(px), Ok(py)) = (u32::try_from(px), u32::try_from(py)) else {
                continue;
            };
            let Some(idx) = py
                .checked_mul(target.stride)
                .and_then(|start| start.checked_add(px))
                .map(|i| i as usize)
            else {
                continue;
            };
            let Some(dest) = target.buffer.get_mut(idx) else {
                continue;
            };
            let under = Channels::from_argb(*dest);
            *dest = 0xFF00_0000
                | (blend_channel(src.red, under.red, alpha) << 16)
                | (blend_channel(src.green, under.green, alpha) << 8)
                | blend_channel(src.blue, under.blue, alpha);
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
mod tests {
    use super::*;
    use crate::sfnt::tests::build_test_font;

    fn font(px: f32) -> ScaledFont {
        ScaledFont::from_bytes(build_test_font(), px).unwrap()
    }

    #[test]
    fn rejects_a_nonsense_size() {
        for px in [0.0_f32, -12.0, f32::NAN, f32::INFINITY] {
            assert_eq!(
                ScaledFont::from_bytes(build_test_font(), px).err(),
                Some(ScaledFontError::InvalidSize),
                "{px} should be rejected"
            );
        }
    }

    #[test]
    fn metrics_scale_with_size() {
        // The fixture declares ascender 800, descender -200, lineGap 100 on a
        // 1000-unit em, so at 100 px those are exactly 80, 20 and 110.
        let f = font(100.0);
        let m = f.metrics();
        assert!((m.ascent - 80.0).abs() < 0.01, "ascent {}", m.ascent);
        assert!((m.descent - 20.0).abs() < 0.01, "descent {}", m.descent);
        assert!(
            (m.line_height - 110.0).abs() < 0.01,
            "line_height {}",
            m.line_height
        );
        // advanceWidthMax is 600 font units.
        assert!(
            (m.max_advance - 60.0).abs() < 0.01,
            "max_advance {}",
            m.max_advance
        );

        // Half the size, half the metrics.
        let h = font(50.0);
        assert!((h.metrics().ascent - 40.0).abs() < 0.01);
        assert!((h.metrics().line_height - 55.0).abs() < 0.01);
    }

    #[test]
    fn unmapped_characters_become_notdef_not_a_hole() {
        let f = font(32.0);
        // The fixture maps only A, B and C.
        assert_eq!(f.glyph_id('A'), 1);
        assert_eq!(f.glyph_id('\u{4e2d}'), 0);
        assert_eq!(f.glyph_id('z'), 0);
    }

    #[test]
    fn a_glyph_rasterizes_and_is_cached() {
        let mut f = font(64.0);
        assert_eq!(f.cached_glyphs(), 0);
        let gid = f.glyph_id('A');
        let first = f.glyph(gid).unwrap().clone();
        assert_eq!(f.cached_glyphs(), 1);
        assert!(!first.mask.is_empty(), "'A' should produce ink");

        // A second request must not re-rasterize, and must be identical.
        let second = f.glyph(gid).unwrap().clone();
        assert_eq!(f.cached_glyphs(), 1);
        assert_eq!(second.mask.width, first.mask.width);
        assert_eq!(second.mask.coverage, first.mask.coverage);
    }

    #[test]
    fn the_square_glyph_is_the_size_the_font_says_it_is() {
        // Glyph 1 of the fixture is a 100x100 unit square on a 1000-unit em,
        // so at 200 px per em it is exactly 20x20 pixels.
        let mut f = font(200.0);
        let gid = f.glyph_id('A');
        let g = f.glyph(gid).unwrap();
        assert_eq!(
            (g.mask.width, g.mask.height),
            (20, 20),
            "got {}x{}",
            g.mask.width,
            g.mask.height
        );
        // Fully inside, so fully covered.
        assert_eq!(g.mask.at(10, 10), 255);
    }

    #[test]
    fn advance_scales_and_measure_agrees_with_it() {
        let mut f = font(100.0);
        let gid = f.glyph_id('A');
        let adv = f.glyph(gid).unwrap().advance;
        assert!(adv > 0.0, "advance should be positive, got {adv}");
        assert!((f.measure("A") - adv).abs() < 0.01);
        // Three of them is three times as wide.
        assert!((f.measure("AAA") - adv * 3.0).abs() < 0.05);
        assert!((f.measure("") - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn measure_does_not_disturb_the_cache() {
        let f = font(24.0);
        let _ = f.measure("ABCABC");
        assert_eq!(f.cached_glyphs(), 0, "measuring must not rasterize");
    }

    #[test]
    fn the_cache_stops_growing_at_its_limit() {
        let mut f = font(8.0);
        // The fixture has only 4 glyphs, so drive the cache directly to prove
        // the eviction path rather than the glyph count.
        for gid in 0..u16::try_from(GLYPH_CACHE_LIMIT + 10).unwrap() {
            f.insert(
                gid,
                Glyph {
                    mask: GlyphMask::default(),
                    advance: 1.0,
                },
            );
        }
        assert_eq!(f.cached_glyphs(), GLYPH_CACHE_LIMIT);
        // The oldest entries went first.
        let newest = u16::try_from(GLYPH_CACHE_LIMIT + 9).unwrap();
        assert!(!f.cache.contains_key(&0));
        assert!(f.cache.contains_key(&newest));
    }

    #[test]
    fn clearing_the_cache_empties_it() {
        let mut f = font(16.0);
        let gid = f.glyph_id('A');
        let _ = f.glyph(gid).unwrap();
        assert_eq!(f.cached_glyphs(), 1);
        f.clear_cache();
        assert_eq!(f.cached_glyphs(), 0);
        // And it still works afterwards.
        let _ = f.glyph(gid).unwrap();
        assert_eq!(f.cached_glyphs(), 1);
    }

    #[test]
    fn wrapping_breaks_at_spaces_and_keeps_long_words_whole() {
        let f = font(100.0);
        let one = f.measure("A");
        // Room for three glyphs plus their spaces.
        let lines = f.wrap("A A A A A A", one * 5.0);
        assert!(lines.len() > 1, "should have wrapped: {lines:?}");
        for line in &lines {
            assert!(!line.is_empty(), "no empty lines: {lines:?}");
        }
        // Nothing is lost.
        assert_eq!(lines.join(" "), "A A A A A A");

        // A word wider than the limit survives intact on its own line.
        let lines = f.wrap("AAAAAAAA", one * 2.0);
        assert_eq!(lines, ["AAAAAAAA"]);

        // Explicit newlines are honoured.
        let lines = f.wrap("A\nA", one * 100.0);
        assert_eq!(lines, ["A", "A"]);
    }

    #[test]
    fn drawing_puts_ink_in_the_buffer_and_returns_the_pen() {
        let mut f = font(64.0);
        let mut buf = alloc::vec![0xFF00_0000_u32; 128 * 128];
        let mut target = Target {
            buffer: &mut buf,
            stride: 128,
            height: 128,
            color: 0xFFFF_FFFF,
        };
        let end = f.draw_text("A", &mut target, 10.0, 64.0);
        assert!(end > 10.0, "pen should have advanced, ended at {end}");
        assert!((end - (10.0 + f.measure("A"))).abs() < 0.01);

        let lit = buf.iter().filter(|&&p| p != 0xFF00_0000).count();
        assert!(lit > 0, "drawing produced no visible pixels");
    }

    #[test]
    fn drawing_off_surface_clips_instead_of_corrupting() {
        let mut f = font(64.0);
        let mut buf = alloc::vec![0u32; 32 * 32];
        // Far off every edge, in turn. None of these may panic, and the
        // wildly-out-of-range ones must not touch the buffer.
        for (x, y) in [
            (-1000.0_f32, 16.0_f32),
            (1000.0, 16.0),
            (16.0, -1000.0),
            (16.0, 1000.0),
        ] {
            buf.fill(0);
            let mut target = Target {
                buffer: &mut buf,
                stride: 32,
                height: 32,
                color: 0xFFFF_FFFF,
            };
            f.draw_text("A", &mut target, x, y);
            assert!(
                buf.iter().all(|&p| p == 0),
                "({x},{y}) should have been clipped entirely"
            );
        }
    }

    #[test]
    fn a_transparent_colour_draws_nothing() {
        let mut f = font(64.0);
        let mut buf = alloc::vec![0u32; 128 * 128];
        let mut target = Target {
            buffer: &mut buf,
            stride: 128,
            height: 128,
            color: 0x0000_0000,
        };
        f.draw_text("A", &mut target, 10.0, 64.0);
        assert!(buf.iter().all(|&p| p == 0));
    }

    #[test]
    fn coverage_blends_rather_than_replacing() {
        // A half-transparent white over black must land near mid grey, not at
        // either extreme — that is the whole point of the coverage multiply.
        let mask = GlyphMask {
            width: 1,
            height: 1,
            left: 0,
            top: 0,
            coverage: alloc::vec![128],
        };
        let mut buf = alloc::vec![0xFF00_0000_u32; 4];
        let mut target = Target {
            buffer: &mut buf,
            stride: 2,
            height: 2,
            color: 0xFFFF_FFFF,
        };
        blit_mask(&mask, &mut target, 0, 0);
        let grey = buf[0] & 0xFF;
        assert!((100..=160).contains(&grey), "expected mid grey, got {grey}");
    }
}
