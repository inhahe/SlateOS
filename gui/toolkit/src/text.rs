//! Text measurement, shared by every widget that needs to know how wide a
//! label is.
//!
//! # Why this exists
//!
//! Widgets used to estimate text width by multiplying a byte count by a fudge
//! factor — `text.len() as f32 * font_size * 0.6` — with each widget picking
//! its own constant: 0.6 in [`menu`], 0.58 in [`disabled`], a flat 7.0 in
//! [`modal`], 7.5 in [`tabs`], 8.0 in [`pathbar`]. That was defensible while
//! the compositor drew a fixed 8x14 cell and threw `font_size` away, because
//! nothing could be measured accurately anyway. It is not defensible now that
//! the compositor draws with a real font: an estimate that disagrees with what
//! is drawn means labels overflow their buttons and text cursors land between
//! characters.
//!
//! It was also wrong in a way the fudge factor cannot fix. `str::len` counts
//! **bytes**, so any non-ASCII text measured 2–4x too wide per character — and
//! non-ASCII text now renders, so that error became visible rather than moot.
//!
//! # How it stays right
//!
//! Everything here measures with [`osfont`]'s [`FontCache`], which is the same
//! type and the same rounding rule the compositor draws with. Measuring and
//! drawing cannot drift apart because there is nothing to keep in sync.
//!
//! [`menu`]: crate::menu
//! [`disabled`]: crate::disabled
//! [`modal`]: crate::modal
//! [`tabs`]: crate::tabs
//! [`pathbar`]: crate::pathbar

use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use osfont::select::Query;
pub use osfont::shape::Affinity;
use osfont::shape::Hit;
use osfont::system::{Family, FontCache, Weight};

use crate::canvas::Canvas;
use crate::color::Color;
use crate::fontdb::FontDb;
use crate::render::{FontFamily, FontWeightHint, RenderCommand, TextOverflow};

/// The families tried, in order, when nothing has chosen one.
///
/// The first is the design's intended UI font; the rest are the default sans
/// of each platform this is developed or run on, ending with the two that are
/// installed almost everywhere. The list exists because the alternative to
/// finding *a* face is the built-in 8x16 bitmap font, which is legible but
/// looks nothing like the system it is standing in for — so a host missing
/// Inter should fall to Segoe UI or DejaVu Sans, not all the way back to
/// bitmaps.
pub const DEFAULT_UI_FAMILIES: &[&str] = &[
    "Inter",
    "Segoe UI",
    "Cantarell",
    "Noto Sans",
    "DejaVu Sans",
    "Liberation Sans",
    "Helvetica",
    "Arial",
];

/// The fixed-pitch families tried, in order, when nothing has chosen one.
///
/// Same shape as [`DEFAULT_UI_FAMILIES`] and same reason, but the stakes are
/// higher: a terminal drawn in a proportional face is not merely off-brand,
/// it is *wrong* — glyphs overhang the cell background behind them and the
/// block cursor lands beside the character it marks. When none of these
/// resolve the built-in 8x16 bitmap face answers, and that one really is
/// fixed-pitch, so the grid holds either way.
pub const DEFAULT_MONO_FAMILIES: &[&str] = &[
    "JetBrains Mono",
    "Cascadia Mono",
    "Cascadia Code",
    "Consolas",
    "DejaVu Sans Mono",
    "Liberation Mono",
    "Noto Sans Mono",
    "Menlo",
    "Courier New",
];

/// The index of installed fonts, built once per process.
///
/// Separate from the cache so that changing the UI font later does not
/// re-walk the font directories.
fn font_db() -> &'static FontDb {
    static DB: OnceLock<FontDb> = OnceLock::new();
    DB.get_or_init(FontDb::scan_system)
}

/// The process-wide font state: rasterized glyphs, and which family they came
/// from.
#[derive(Debug)]
struct Fonts {
    cache: FontCache,
    /// The UI family currently installed, or `None` while the built-in bitmap
    /// face is in use. Held here rather than in a second static so it cannot
    /// disagree with what the cache actually holds.
    family: Option<String>,
    /// The fixed-pitch family currently installed, likewise.
    mono_family: Option<String>,
}

/// The process-wide font cache.
///
/// Global because it is a pure memoization of "what does this size look
/// like": two widgets asking about 14 px regular text must get the same
/// answer, and threading a cache through every `intrinsic_size` call would
/// change the signature of most of the toolkit to say so.
///
/// # Why the system font is installed here, lazily
///
/// A UI process measures text and the compositor draws it, in two different
/// processes with two different caches. They must resolve the same family to
/// the same file or every centred label in the system is off by the
/// difference between two fonts' metrics — and neither process looks wrong on
/// its own, which makes it a miserable bug to find.
///
/// Installing on first use, from a list compiled into the toolkit, is what
/// makes that agreement automatic: every process that draws any text at all
/// runs the same resolution against the same directories, without each having
/// to remember to opt in at startup. An explicit "call this at startup" API
/// would be cheaper, and would be wrong the first time an app forgot.
///
/// The cost is one directory scan per process, on the first call that touches
/// text — under a second on this host in a debug build — and a process that
/// never draws text never pays it.
fn cache() -> &'static Mutex<Fonts> {
    static CACHE: OnceLock<Mutex<Fonts>> = OnceLock::new();
    CACHE.get_or_init(|| {
        let mut cache = FontCache::new();
        let family = install_ui_faces(&mut cache).map(str::to_string);
        let mono_family = install_mono_faces(&mut cache).map(str::to_string);
        Mutex::new(Fonts {
            cache,
            family,
            mono_family,
        })
    })
}

/// Load the default UI font into `cache`, returning the family that won.
///
/// Public because the compositor keeps a [`FontCache`] of its own: it draws
/// every process's text and never measures any, so routing its glyph runs
/// through this module's lock would buy nothing. What it does have to share is
/// the *choice* of face. If it picked a family by its own rule, or walked its
/// own fallback list, then on any host where the two lists disagree the
/// system would measure in one font and draw in another. Calling this is how
/// a second cache is made to agree by construction instead of by two lists
/// being maintained in step.
///
/// Returns `None`, having changed nothing, if no family on the list resolves;
/// the cache then keeps its built-in bitmap face.
pub fn install_ui_faces(cache: &mut FontCache) -> Option<&'static str> {
    DEFAULT_UI_FAMILIES
        .iter()
        .copied()
        .find(|family| install_family(cache, family))
}

/// Load the default fixed-pitch font into `cache`, returning the family that
/// won.
///
/// The counterpart of [`install_ui_faces`], and public for the same reason: a
/// second cache — the compositor's — has to resolve the family the same way
/// the measuring process did, or a terminal is measured in one face and drawn
/// in another and its grid comes apart.
///
/// Returns `None`, having changed nothing, if no family on the list resolves;
/// [`FontFamily::Mono`] then falls back to the built-in bitmap face, which is
/// itself fixed-pitch.
pub fn install_mono_faces(cache: &mut FontCache) -> Option<&'static str> {
    DEFAULT_MONO_FAMILIES
        .iter()
        .copied()
        .find(|family| install_family_as(cache, FontFamily::Mono, family))
}

/// Load `family`'s regular and bold faces into `cache` as the UI font.
pub fn install_family(cache: &mut FontCache, family: &str) -> bool {
    install_family_as(cache, FontFamily::Ui, family)
}

/// Load `name`'s regular and bold faces into `cache` as `family`.
///
/// Both weights must load, and the cache is only touched once both have: a
/// half-installed family would draw bold text in the old face and regular in
/// the new one, which looks like a rendering fault rather than a missing
/// font.
pub fn install_family_as(cache: &mut FontCache, family: FontFamily, name: &str) -> bool {
    let db = font_db();
    let (Ok(regular), Ok(bold)) = (
        db.load(name, Query::regular()),
        db.load(name, Query::bold()),
    ) else {
        return false;
    };
    let family = family_of(family);
    cache.set_face(family, Weight::Regular, Arc::new(regular));
    cache.set_face(family, Weight::Bold, Arc::new(bold));
    true
}

/// Draw all UI text in `family` from now on.
///
/// Returns `false` and changes nothing if the family is not installed or its
/// files cannot be read — the caller keeps whatever it had, which is a
/// working font, rather than losing text to a bad setting.
///
/// Every already-rasterized glyph is dropped, so the next measurement and the
/// next frame both use the new face. Callers must apply the same change in
/// every process that draws, or measuring and drawing will disagree.
pub fn set_font_family(family: &str) -> bool {
    let mut fonts = cache().lock().unwrap_or_else(PoisonError::into_inner);
    if !install_family(&mut fonts.cache, family) {
        return false;
    }
    fonts.family = Some(family.to_string());
    true
}

/// Draw all fixed-pitch text in `family` from now on.
///
/// The [`set_font_family`] of the terminal font. Returns `false` and changes
/// nothing if the family is not installed or cannot be read.
///
/// Note that nothing checks that the named family *is* fixed-pitch: a caller
/// that points this at a proportional face gets a terminal with a broken grid,
/// and that is the caller's decision to have made.
pub fn set_mono_family(family: &str) -> bool {
    let mut fonts = cache().lock().unwrap_or_else(PoisonError::into_inner);
    if !install_family_as(&mut fonts.cache, FontFamily::Mono, family) {
        return false;
    }
    fonts.mono_family = Some(family.to_string());
    true
}

/// The family UI text is currently drawn in, or `None` if no installed font
/// could be found and the built-in bitmap face is in use.
#[must_use]
pub fn font_family() -> Option<String> {
    cache()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .family
        .clone()
}

/// The family fixed-pitch text is currently drawn in, or `None` if none
/// resolved and the built-in bitmap face is in use.
#[must_use]
pub fn mono_family() -> Option<String> {
    cache()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .mono_family
        .clone()
}

/// Every font family installed on this system, sorted — what a font picker
/// lists.
#[must_use]
pub fn available_families() -> Vec<String> {
    font_db().families()
}

/// Runs `f` with the font for `size` and `weight`.
///
/// Poisoning is ignored deliberately. The guarded value is a cache of
/// rasterized glyphs with no cross-entry invariants, so a panic elsewhere in
/// the UI cannot leave it inconsistent — only possibly missing an entry, which
/// the next call rebuilds. Propagating the poison instead would turn one
/// widget's panic into a permanently unmeasurable UI.
fn with_font<R>(
    size: f32,
    weight: FontWeightHint,
    family: FontFamily,
    f: impl FnOnce(&mut osfont::system::SystemFont) -> R,
) -> R {
    let mut fonts = cache().lock().unwrap_or_else(PoisonError::into_inner);
    f(fonts.cache.get(size, weight_of(weight), family_of(family)))
}

/// Translates the toolkit's weight hint into the one `osfont` understands.
///
/// `Light` maps to regular because the built-in face has two weights and no
/// third; rendering it bold would be the opposite of what was asked for.
fn weight_of(hint: FontWeightHint) -> Weight {
    match hint {
        FontWeightHint::Bold => Weight::Bold,
        FontWeightHint::Regular | FontWeightHint::Light => Weight::Regular,
    }
}

/// Translates the toolkit's family into the one `osfont` understands.
fn family_of(family: FontFamily) -> Family {
    match family {
        FontFamily::Ui => Family::Ui,
        FontFamily::Mono => Family::Mono,
    }
}

/// Width of `text` in pixels, as the compositor will actually draw it.
pub fn measure(text: &str, size: f32, weight: FontWeightHint) -> f32 {
    measure_in(text, size, weight, FontFamily::Ui)
}

/// Width of `text` in pixels in `family`.
///
/// The family-aware form of [`measure`]. A caller drawing inside a
/// [`RenderCommand::PushFont`] scope must measure with the same family it
/// pushed, or its layout and the compositor's drawing disagree — which is the
/// one class of bug this whole module exists to remove.
pub fn measure_in(text: &str, size: f32, weight: FontWeightHint, family: FontFamily) -> f32 {
    with_font(size, weight, family, |font| font.measure(text))
}

/// An ARGB pixel buffer an application can draw text into directly.
///
/// # Why this exists
///
/// Almost everything draws text by emitting a [`RenderCommand::Text`] and
/// letting the compositor rasterize it. A few things cannot: an app that
/// produces an *image file* — a screenshot with annotations baked in, a chart
/// exported to disk — owns the pixels itself and has no compositor in the
/// loop. Before this existed those apps had two options, and both were wrong:
/// drop the text from the output (silently losing what the user typed), or add
/// a direct `osfont` dependency and rasterize with a *different* font cache
/// than the one that measured the layout — which is the exact drift this
/// module exists to prevent.
pub struct Surface<'a> {
    /// The pixel buffer, `0xAARRGGBB` per pixel, row-major and top-down.
    pub pixels: &'a mut [u32],
    /// Pixels per row.
    pub width: u32,
    /// Rows in the buffer.
    pub height: u32,
}

/// Draws `text` into `surface` with its **baseline** at `baseline_y`, starting
/// at pen position `x`, and returns the pen position after the last glyph.
///
/// Uses the same font cache as [`measure`], so the drawn width is the measured
/// width. Anything falling outside the surface is clipped, including negative
/// coordinates; a buffer shorter than `width * height` is rejected outright
/// rather than drawing into whatever part of it exists, because a partial
/// surface means the caller's stride is wrong and every row after the first
/// would land in the wrong place.
///
/// The colour's alpha scales the glyph coverage, so a translucent colour
/// blends rather than replacing.
pub fn draw_into(
    surface: &mut Surface<'_>,
    text: &str,
    x: f32,
    baseline_y: f32,
    size: f32,
    weight: FontWeightHint,
    color: Color,
) -> f32 {
    draw_into_family(
        surface,
        text,
        x,
        baseline_y,
        size,
        weight,
        FontFamily::Ui,
        color,
    )
}

/// The family-aware form of [`draw_into`].
// Eight arguments, kept positional for the same reason `RenderTree`'s text
// primitives are: the parameters are exactly the ones every other function in
// this module takes, in the same order, and a bag struct here would read worse
// than the siblings it has to sit beside.
#[allow(clippy::too_many_arguments)]
pub fn draw_into_family(
    surface: &mut Surface<'_>,
    text: &str,
    x: f32,
    baseline_y: f32,
    size: f32,
    weight: FontWeightHint,
    family: FontFamily,
    color: Color,
) -> f32 {
    let needed = (surface.width as usize).saturating_mul(surface.height as usize);
    if surface.pixels.len() < needed {
        return x;
    }
    let argb = (u32::from(color.a) << 24)
        | (u32::from(color.r) << 16)
        | (u32::from(color.g) << 8)
        | u32::from(color.b);
    let mut target = osfont::scaled::Target {
        buffer: surface.pixels,
        stride: surface.width,
        height: surface.height,
        color: argb,
    };
    with_font(size, weight, family, |font| {
        font.draw_text(text, &mut target, x, baseline_y)
    })
}

/// Rounds a pixel *extent* up to a whole count, rejecting anything that is not
/// a finite, positive, addressable number of pixels.
///
/// `NaN` fails every comparison, so the range check rejects it without a
/// special case — which is the point: a `NaN` width reaching an allocation is
/// how a layout bug becomes a panic.
fn px_count(extent: f32) -> Option<u32> {
    let whole = extent.ceil();
    if whole >= 1.0 && whole <= f64::from(u32::MAX) as f32 {
        // Proved in range and non-negative on the line above.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        Some(whole as u32)
    } else {
        None
    }
}

/// Floors a pixel coordinate to a whole one, clamping instead of wrapping.
///
/// A coordinate far outside `i64` is off-canvas by any measure, and saturating
/// keeps it off-canvas; the `as` cast alone would wrap it back into view.
fn whole(coord: f32) -> i64 {
    let floored = coord.floor();
    if floored.is_nan() {
        return 0;
    }
    let clamped = floored.clamp(-(2.0_f32.powi(62)), 2.0_f32.powi(62));
    // Clamped into a range `i64` represents exactly at this magnitude.
    #[allow(clippy::cast_possible_truncation)]
    {
        clamped as i64
    }
}

/// Draws `text` onto a [`Canvas`] with its **baseline** at `baseline_y`,
/// starting at pen position `x`, and returns the pen position after the last
/// glyph.
///
/// # Why this is not just [`draw_into`] with a converted buffer
///
/// [`Surface`] is an *opaque* target: the rasterizer forces every pixel it
/// touches to `alpha = 255`, because a screenshot or an exported chart has no
/// transparency to preserve and blending against a transparent destination
/// would only produce fringing. A `Canvas` is the opposite — a paint layer is
/// routinely transparent, and text dropped onto one must leave the untouched
/// parts of every anti-aliased pixel *still* transparent, or the glyphs arrive
/// wearing a black halo wherever they were smoothed against nothing.
///
/// So this renders the string once into a scratch buffer as white-on-black,
/// which makes each pixel's value its **coverage** (`blend_channel(255, 0, a)`
/// is `a`), and then blends `color` onto the canvas at that coverage. The
/// canvas's own alpha compositing ([`Canvas::blend`]) does the rest, so drawing
/// onto a transparent layer yields a glyph with a soft transparent edge rather
/// than a soft black one.
///
/// The scratch buffer is bounded by the measured extent of the text, not by the
/// canvas, so drawing a short word onto a large image costs the word.
pub fn draw_onto_canvas(
    canvas: &mut Canvas,
    text: &str,
    x: f32,
    baseline_y: f32,
    size: f32,
    weight: FontWeightHint,
    color: Color,
) -> f32 {
    draw_onto_canvas_in(
        canvas,
        text,
        x,
        baseline_y,
        size,
        weight,
        FontFamily::Ui,
        color,
    )
}

/// The family-aware form of [`draw_onto_canvas`].
// Eight arguments, for the same reason `draw_into_family` takes eight.
#[allow(clippy::too_many_arguments)]
pub fn draw_onto_canvas_in(
    canvas: &mut Canvas,
    text: &str,
    x: f32,
    baseline_y: f32,
    size: f32,
    weight: FontWeightHint,
    family: FontFamily,
    color: Color,
) -> f32 {
    if text.is_empty() || color.a == 0 {
        return x;
    }
    let advance = measure_in(text, size, weight, family);

    // The mask has to cover more than the advance box. A glyph may start left
    // of the pen (an italic `f`, a negative left bearing), may overshoot the
    // ascent (an accent stack), and the rasterizer anti-aliases a fraction of a
    // pixel past both. Padding by the font size is coarse and is *meant* to be:
    // an under-sized mask silently clips glyph edges, which is a bug that looks
    // like a font problem, while an over-sized one costs a few zero pixels.
    let pad = size.max(1.0).ceil();
    let ascent = ascent_in(size, weight, family);
    let line = line_height_in(size, weight, family);
    let origin_x = (x - pad).floor();
    let origin_y = (baseline_y - ascent - pad).floor();
    // A width or height that is not a finite, positive, representable count of
    // pixels is not a text box, it is a broken layout; drawing nothing is the
    // honest response.
    let (Some(mask_w), Some(mask_h)) = (px_count(advance + pad * 2.0), px_count(line + pad * 2.0))
    else {
        return x;
    };
    let Some(cells) = (mask_w as usize).checked_mul(mask_h as usize) else {
        return x;
    };

    let mut mask = vec![0u32; cells];
    let pen = {
        let mut surface = Surface {
            pixels: &mut mask,
            width: mask_w,
            height: mask_h,
        };
        draw_into_family(
            &mut surface,
            text,
            x - origin_x,
            baseline_y - origin_y,
            size,
            weight,
            family,
            // Opaque white on the zeroed (black) buffer, so the value that
            // lands in each pixel *is* the coverage.
            Color::rgba(255, 255, 255, 255),
        )
    };

    // The origin can sit left of or above the canvas — text anchored near the
    // edge routinely does — so the destination coordinate is computed in `i64`
    // and pixels that land outside are dropped rather than wrapped onto the
    // opposite edge.
    let (origin_col, origin_row) = (whole(origin_x), whole(origin_y));

    for row in 0..mask_h {
        let Ok(dest_y) = u32::try_from(origin_row.saturating_add(i64::from(row))) else {
            continue;
        };
        for col in 0..mask_w {
            let idx = (row as usize)
                .saturating_mul(mask_w as usize)
                .saturating_add(col as usize);
            let coverage = mask.get(idx).map_or(0, |px| px & 0xFF);
            if coverage == 0 {
                continue;
            }
            let Ok(dest_x) = u32::try_from(origin_col.saturating_add(i64::from(col))) else {
                continue;
            };
            // Coverage scales the requested alpha: a half-covered pixel of a
            // half-transparent colour is a quarter-strength blend.
            let Ok(alpha) = u8::try_from(coverage.saturating_mul(u32::from(color.a)) / 255) else {
                continue;
            };
            if alpha == 0 {
                continue;
            }
            canvas.blend(
                dest_x,
                dest_y,
                Color::rgba(color.r, color.g, color.b, alpha),
            );
        }
    }

    origin_x + pen
}

/// The advance of one fixed-pitch cell: the width every glyph occupies in
/// [`FontFamily::Mono`].
///
/// This is the number a terminal lays its grid out on. It replaces
/// [`digit_advance`] for that purpose — a digit's advance is only the cell
/// width if the face is monospace, and the UI face is not, so a terminal
/// measured that way drew a `W` almost twice as wide as the cell it was
/// supposed to sit in.
pub fn cell_advance(size: f32, weight: FontWeightHint) -> f32 {
    measure_in("0", size, weight, FontFamily::Mono)
}

/// Baseline-to-baseline distance in pixels in `family`.
pub fn line_height_in(size: f32, weight: FontWeightHint, family: FontFamily) -> f32 {
    with_font(size, weight, family, |font| font.line_height())
}

/// Distance from the top of a line down to its baseline in `family`.
pub fn ascent_in(size: f32, weight: FontWeightHint, family: FontFamily) -> f32 {
    with_font(size, weight, family, |font| font.metrics().ascent)
}

/// Width of `text` in pixels at the default weight.
pub fn width(text: &str, size: f32) -> f32 {
    measure(text, size, FontWeightHint::Regular)
}

/// Baseline-to-baseline distance in pixels.
pub fn line_height(size: f32, weight: FontWeightHint) -> f32 {
    line_height_in(size, weight, FontFamily::Ui)
}

/// Distance from the top of a line down to its baseline, in pixels.
///
/// Needed by callers that position text by its top edge, which is most of
/// them, since layout works in boxes.
pub fn ascent(size: f32, weight: FontWeightHint) -> f32 {
    ascent_in(size, weight, FontFamily::Ui)
}

/// The x at which to draw `text` so that it is centred on `center`.
///
/// Centring is by far the most common thing callers wanted a width for — and
/// the thing the old estimates got most visibly wrong, since the error in a
/// guessed width is halved into the offset and so grows with the label. Having
/// it here means an app centres text by saying so, rather than by re-deriving
/// "measure, halve, subtract" and picking its own fudge factor on the way.
pub fn center_x(text: &str, center: f32, size: f32, weight: FontWeightHint) -> f32 {
    center - measure(text, size, weight) / 2.0
}

/// The x at which to draw `text` so that it ends at `right`.
pub fn right_x(text: &str, right: f32, size: f32, weight: FontWeightHint) -> f32 {
    right - measure(text, size, weight)
}

/// Width of a box that holds `text` with `padding` px of space on each side.
///
/// Buttons, tabs, chips, badges and pills are all this shape, and before this
/// existed every one of them wrote `label.len() as f32 * 8.0 + 16.0` — a byte
/// count, so any label with a non-ASCII character in it got a box two to three
/// times too wide. Naming the shape means the padding stays a padding and the
/// width stays a width.
pub fn padded_width(text: &str, padding: f32, size: f32, weight: FontWeightHint) -> f32 {
    measure(text, size, weight) + padding * 2.0
}

/// Width of a box that holds `text` at *whichever* weight it ends up drawn at.
///
/// For a strip whose selected item is drawn bold and the rest regular. Sizing
/// each item to the weight it currently has makes the whole strip shuffle
/// sideways every time the selection moves, because the selected item grows and
/// pushes its neighbours along; sizing them all to the widest weight they can
/// take keeps the layout still and still fits the text.
pub fn padded_width_any_weight(text: &str, padding: f32, size: f32) -> f32 {
    let bold = measure(text, size, FontWeightHint::Bold);
    let regular = measure(text, size, FontWeightHint::Regular);
    bold.max(regular) + padding * 2.0
}

/// Width of a single `'0'`, for callers laying out columns of digits or
/// treating text as a grid.
///
/// A grid is the wrong model for proportional text, so this is a stopgap for
/// widgets that have not yet been converted to measure real substrings.
///
/// It is **not** the right call for a terminal-style view, even though a grid
/// is genuinely the right model there: a digit's advance is only every glyph's
/// advance if the face is fixed-pitch, and the UI face is not. Those callers
/// want [`cell_advance`], which asks for a fixed-pitch face and so gets a
/// number that is true of every character.
pub fn digit_advance(size: f32, weight: FontWeightHint) -> f32 {
    measure("0", size, weight)
}

/// The longest prefix of `text` that fits in `max_width`, as a byte index.
///
/// Breaks between glyphs, never inside one: half a glyph reads as a rendering
/// fault rather than as elided text, a byte-sliced UTF-8 sequence is not text
/// at all, and half a ligature is both.
pub fn fit(text: &str, max_width: f32, size: f32, weight: FontWeightHint) -> usize {
    if max_width <= 0.0 {
        return 0;
    }
    // Shaped once and walked, rather than re-measured per prefix, which would
    // be quadratic in the line length. Going through the shaped run — rather
    // than summing each character's bare advance, which is what this used to
    // do — is what makes the cut agree with `measure`: an unkerned sum drifts
    // from the width the text is actually drawn at, so an ellipsis appeared a
    // few pixels from where the string really ended.
    with_font(size, weight, FontFamily::Ui, |font| {
        font.shape(text).fit(max_width, text.len())
    })
}

/// The longest *suffix* of `text` that fits in `max_width`, as the byte index
/// the suffix starts at.
///
/// The mirror of [`fit`], for the cases where the end of the string is the part
/// worth keeping — a filesystem path, where the filename matters and the
/// leading directories do not, is the usual one. Like [`fit`] it breaks between
/// glyphs: an index into the middle of a UTF-8 sequence is not a string
/// boundary, and slicing there is an abort rather than a cosmetic fault.
pub fn fit_end(text: &str, max_width: f32, size: f32, weight: FontWeightHint) -> usize {
    if max_width <= 0.0 {
        return text.len();
    }
    with_font(size, weight, FontFamily::Ui, |font| {
        font.shape(text).fit_end(max_width, text.len())
    })
}

/// `text` truncated to `max_width`, with `ellipsis` appended if it did not fit.
///
/// The ellipsis is measured too, so the result genuinely fits rather than
/// overflowing by exactly the width of the ellipsis — the bug that makes
/// truncated labels still collide with whatever is next to them.
pub fn elide(
    text: &str,
    max_width: f32,
    ellipsis: &str,
    size: f32,
    weight: FontWeightHint,
) -> String {
    if measure(text, size, weight) <= max_width {
        return text.to_string();
    }
    let room = max_width - measure(ellipsis, size, weight);
    if room <= 0.0 {
        // Not even the ellipsis fits, so anything drawn would overflow.
        return String::new();
    }
    let cut = fit(text, room, size, weight);
    let mut out = text[..cut].to_string();
    out.push_str(ellipsis);
    out
}

/// `text` truncated from its *start* to `max_width`, with `ellipsis` prepended
/// if it did not fit.
///
/// The mirror of [`elide`], for strings whose tail carries the information. A
/// path elided the usual way reads `/home/user/projects/very/deep...`, which
/// tells the reader nothing they wanted; elided from the start it reads
/// `...deep/notes.txt`, which names the file.
pub fn elide_start(
    text: &str,
    max_width: f32,
    ellipsis: &str,
    size: f32,
    weight: FontWeightHint,
) -> String {
    if measure(text, size, weight) <= max_width {
        return text.to_string();
    }
    let room = max_width - measure(ellipsis, size, weight);
    if room <= 0.0 {
        // Not even the ellipsis fits, so anything drawn would overflow.
        return String::new();
    }
    let start = fit_end(text, room, size, weight);
    let mut out = ellipsis.to_string();
    out.push_str(&text[start..]);
    out
}

/// `text` broken into lines no wider than `max_width`, breaking at spaces.
///
/// Callers need this because [`RenderCommand::Text`] does **not** wrap: the
/// compositor truncates at `max_width`, dropping whole glyphs off the end of
/// the one line it draws. So a caller with a paragraph to show has to wrap it
/// itself and emit one command per line — and, crucially, has to reserve height
/// for the same lines it emits. Deriving the height from anything else (a byte
/// count over a guessed characters-per-line, say) is how a list of paragraphs
/// ends up with items overlapping each other.
///
/// A word longer than `max_width` gets its own over-long line rather than being
/// cut mid-word; breaking inside a word is a per-script decision that belongs to
/// a real line breaker. Existing newlines always break.
///
/// [`RenderCommand::Text`]: crate::render::RenderCommand::Text
pub fn wrap(text: &str, max_width: f32, size: f32, weight: FontWeightHint) -> Vec<String> {
    if max_width <= 0.0 {
        // Nothing fits, and the greedy rule below would answer that with one
        // word per line — an unbounded list for a box that cannot show it.
        // Reporting the paragraphs unwrapped keeps the line count meaningful.
        return text.split('\n').map(str::to_string).collect();
    }
    with_font(size, weight, FontFamily::Ui, |font| {
        font.wrap(text, max_width)
    })
}

/// [`wrap`], except that a word wider than `max_width` is broken by measured
/// fit rather than left on an over-long line.
///
/// # Which of the two to call
///
/// [`wrap`] is right when the box can grow, or when an over-long line is
/// merely ugly: a paragraph in a detail pane, a tooltip, a description in a
/// list. It never breaks inside a word, because where to break one is a
/// per-script decision that belongs to a real line breaker.
///
/// This is right when the box **cannot** grow — a desktop icon's 72 px cell
/// with another icon beside it, a fixed-width column, an article body in a
/// pane the user sized. There, a line wider than the box is not a blemish; it
/// is text drawn over something else, or text the renderer silently drops the
/// end of.
///
/// It is also the only workable answer for a script that does not use spaces.
/// A Japanese file name contains no space, so `wrap` returns the whole of it
/// as one line however narrow the box is — meaning *every* such label
/// overflows. Han and Kana break almost anywhere, so a measured break is close
/// to what a real line breaker would do for them, and merely inelegant for a
/// long Latin word.
///
/// The cost of this over [`wrap`] is one extra shaping pass per over-long
/// line, and none at all for text that already fits.
pub fn wrap_hard(text: &str, max_width: f32, size: f32, weight: FontWeightHint) -> Vec<String> {
    if max_width <= 0.0 {
        // Nothing fits. Breaking by measured fit would answer that with one
        // cluster per line — an unbounded list for a box that cannot show it.
        return text.split('\n').map(str::to_string).collect();
    }
    with_font(size, weight, FontFamily::Ui, |font| {
        font.wrap_hard(text, max_width)
    })
}

/// A block of prose, drawn as one [`RenderCommand::Text`] per wrapped line.
///
/// # Why this exists
///
/// [`wrap`] gives a caller the lines; this makes it hard to then get the
/// *height* wrong. That is the part that has broken repeatedly. A caller
/// drawing a paragraph has to reserve room for it — a detail pane advances a
/// cursor, a card sizes itself, a list stacks items — and doing that from
/// anything other than the lines it drew means two calculations for one
/// quantity. Both observed forms were wrong the same way: a flat per-field
/// allowance (the next field lands on top of a long paragraph) and a byte
/// count over a guessed characters-per-line (2–4x too tall for non-ASCII, and
/// blind to how wide the glyphs actually are).
///
/// [`draw`](Paragraph::draw) returns the height it used, measured from the
/// commands it just emitted, so there is only one calculation to be right.
///
/// ```no_run
/// # use guitk::color::Color;
/// # use guitk::render::{FontWeightHint, RenderCommand};
/// # use guitk::text::Paragraph;
/// # let mut cmds: Vec<RenderCommand> = Vec::new();
/// # let (x, mut y, width) = (0.0, 0.0, 200.0);
/// y += Paragraph::new("a note the user typed", Color::rgb(200, 200, 200))
///     .at(x, y, width)
///     .font(13.0, FontWeightHint::Regular)
///     .draw(&mut cmds);
/// // `y` is now below the note, however many lines it turned out to be.
/// ```
///
/// [`RenderCommand::Text`]: crate::render::RenderCommand::Text
#[derive(Clone, Debug)]
pub struct Paragraph<'a> {
    text: &'a str,
    color: Color,
    x: f32,
    y: f32,
    width: f32,
    size: f32,
    weight: FontWeightHint,
    line_height: Option<f32>,
    max_lines: Option<usize>,
}

impl<'a> Paragraph<'a> {
    /// Default point size, matching the toolkit's body text.
    const DEFAULT_SIZE: f32 = 13.0;

    /// A paragraph of `text` drawn in `color`.
    ///
    /// Position and width have no sensible default, so call [`at`](Self::at)
    /// before drawing; a paragraph with no width reports its text unwrapped
    /// rather than breaking after every word.
    pub fn new(text: &'a str, color: Color) -> Self {
        Self {
            text,
            color,
            x: 0.0,
            y: 0.0,
            width: 0.0,
            size: Self::DEFAULT_SIZE,
            weight: FontWeightHint::Regular,
            line_height: None,
            max_lines: None,
        }
    }

    /// Place the paragraph's top-left at (`x`, `y`) with `width` px of room.
    #[must_use]
    pub fn at(mut self, x: f32, y: f32, width: f32) -> Self {
        self.x = x;
        self.y = y;
        self.width = width;
        self
    }

    /// Set the font. Defaults to 13 px regular.
    #[must_use]
    pub fn font(mut self, size: f32, weight: FontWeightHint) -> Self {
        self.size = size;
        self.weight = weight;
        self
    }

    /// Override the baseline-to-baseline spacing.
    ///
    /// Defaults to the font's own [`line_height`], which is what the compositor
    /// will draw with. Override it to match a layout that was built around a
    /// specific figure — changing the spacing of an existing pane is a visible
    /// change even when it is an improvement.
    #[must_use]
    pub fn line_height(mut self, height: f32) -> Self {
        self.line_height = Some(height);
        self
    }

    /// Draw at most `max` lines, marking the last kept line with an ellipsis.
    ///
    /// For a bounded surface — a toast, a card in a stack — where the text may
    /// legitimately be longer than the room it has. The mark matters: a body
    /// cut without one reads as a complete sentence.
    #[must_use]
    pub fn max_lines(mut self, max: usize) -> Self {
        self.max_lines = Some(max);
        self
    }

    /// The spacing actually in use.
    fn spacing(&self) -> f32 {
        self.line_height
            .unwrap_or_else(|| line_height(self.size, self.weight))
    }

    /// The lines this paragraph will be drawn as, ellipsis included.
    ///
    /// Empty text has no lines, so an absent field takes no room — which is
    /// what every caller wants and what each of them used to write out as an
    /// `if !field.is_empty()` around the whole block. [`wrap`] instead reports
    /// one empty line, because there its answer is about paragraph structure;
    /// here it is about what will be drawn, and nothing will be. Text that is
    /// *deliberately* blank lines is not empty and keeps its room.
    #[must_use]
    pub fn lines(&self) -> Vec<String> {
        if self.text.is_empty() {
            return Vec::new();
        }
        let mut lines = wrap(self.text, self.width, self.size, self.weight);
        if let Some(max) = self.max_lines
            && lines.len() > max
        {
            lines.truncate(max);
            if let Some(last) = lines.last_mut() {
                *last = elide(&format!("{last}…"), self.width, "…", self.size, self.weight);
            }
        }
        lines
    }

    /// The height the paragraph will occupy.
    ///
    /// Only for callers that must size a container *before* filling it. Where
    /// the drawing and the sizing can happen together, prefer [`draw`]'s return
    /// value — one call cannot disagree with itself.
    ///
    /// [`draw`]: Self::draw
    #[must_use]
    pub fn height(&self) -> f32 {
        self.lines().len() as f32 * self.spacing()
    }

    /// Emit one text command per line, and report the height used.
    ///
    /// Takes any command sink, so that it serves both of the shapes the app
    /// tree draws into: a bare `Vec<RenderCommand>` and a
    /// [`RenderTree`](crate::render::RenderTree).
    pub fn draw(&self, cmds: &mut impl Extend<RenderCommand>) -> f32 {
        let spacing = self.spacing();
        let lines = self.lines();
        cmds.extend(lines.iter().enumerate().map(|(n, line)| {
            RenderCommand::Text {
                x: self.x,
                y: self.y + n as f32 * spacing,
                text: line.clone(),
                color: self.color,
                font_size: self.size,
                font_weight: self.weight,
                // Belt and braces: every line already fits, but a caller that
                // set a width the glyphs cannot honour (a single unbreakable
                // word) gets a clipped line rather than one running out of its
                // container.
                max_width: Some(self.width),
                overflow: TextOverflow::Ellipsis,
            }
        }));
        lines.len() as f32 * spacing
    }
}

/// Where a caret is: a byte offset into the text, plus which of that offset's
/// screen positions the caret belongs at.
///
/// One byte offset is one place in the *text* but not always one place on the
/// *screen*. Where a left-to-right stretch meets a right-to-left one, the
/// boundary between them is a single offset drawn at two separate x
/// coordinates — the end of the one run and the start of the other — and
/// neither is more correct than the other. Which one a caret goes to depends on
/// how the caret got there, so it is a property of the cursor rather than of
/// the text, and has to be carried in the cursor's state.
///
/// [`Affinity::Downstream`] is the default and the right answer for a caller
/// with no opinion: it means the caret belongs to the character that *starts*
/// at the offset. `TextCursor::from(byte)` builds one. On text that runs in a
/// single direction the two affinities name the same point, so the choice costs
/// nothing to get wrong there — which is most of the UI, most of the time.
///
/// Deliberately not `Ord`: two cursors at the same offset with different
/// affinities are the same place in the text and two places on the screen, so
/// neither "comes first" in any sense a caller could rely on. Code that wants
/// an order — a selection's start and end, say — is asking about the text, and
/// should compare [`TextCursor::byte`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct TextCursor {
    /// Byte offset into the text. Always on a character boundary.
    pub byte: usize,
    /// Which of the offset's two screen positions the caret is at.
    pub affinity: Affinity,
}

impl From<usize> for TextCursor {
    /// A cursor with no opinion about direction boundaries, which is what every
    /// caller that has only ever had a byte offset means.
    fn from(byte: usize) -> Self {
        Self {
            byte,
            affinity: Affinity::Downstream,
        }
    }
}

impl TextCursor {
    /// The offset alone, for the arithmetic that only ever cared about the
    /// text: inserting, deleting, slicing.
    #[must_use]
    pub fn byte(self) -> usize {
        self.byte
    }
}

/// Where to draw the caret for `at`, in pixels from the start of `text`.
///
/// The replacement for measuring the width of the text before the cursor. Those
/// are the same number only while the text runs in one direction; as soon as it
/// does not, the width of a prefix and the position of that prefix's end on the
/// screen are different quantities, and this is the one that puts the caret
/// where the user is looking.
pub fn caret_x(text: &str, at: TextCursor, size: f32, weight: FontWeightHint) -> f32 {
    caret_x_in(text, at, size, weight, FontFamily::Ui)
}

/// The family-aware form of [`caret_x`]. A caller drawing inside a
/// [`RenderCommand::PushFont`] scope must place its caret with the same family
/// it pushed.
pub fn caret_x_in(
    text: &str,
    at: TextCursor,
    size: f32,
    weight: FontWeightHint,
    family: FontFamily,
) -> f32 {
    with_font(size, weight, family, |font| {
        font.shape(text).x_of(at.byte, text.len(), at.affinity)
    })
}

/// The boxes to paint to highlight `from..to`, as `(left, width)` pairs in
/// pixels from the start of `text`, ordered left to right.
///
/// There is more than one box exactly when the range spans a change of
/// direction: a run of text that is contiguous in the *string* need not be
/// contiguous on the *screen*, and the gap between two of these boxes holds
/// characters the user did not select. That is why this returns a list and not
/// a rectangle — the two-edge form a caller would otherwise write, `x_of(to) -
/// x_of(from)`, describes a region that includes them.
pub fn selection_boxes(
    text: &str,
    from: usize,
    to: usize,
    size: f32,
    weight: FontWeightHint,
) -> Vec<(f32, f32)> {
    selection_boxes_in(text, from, to, size, weight, FontFamily::Ui)
}

/// The family-aware form of [`selection_boxes`].
pub fn selection_boxes_in(
    text: &str,
    from: usize,
    to: usize,
    size: f32,
    weight: FontWeightHint,
    family: FontFamily,
) -> Vec<(f32, f32)> {
    with_font(size, weight, family, |font| {
        font.shape(text).selection_rects(from, to, text.len())
    })
}

/// The cursor a click at `offset` pixels from the start of `text` produces —
/// byte offset *and* the affinity the click implies.
///
/// [`char_index_at`] is the same query for callers that count characters and
/// draw nothing; this is the one whose result can be stored in a cursor and
/// handed back to [`caret_x`] to draw the caret where the user clicked. Round
/// tripping is the property that matters: `caret_x(cursor_at(x))` returns to
/// the edge that was aimed at, which is not true of the byte offset alone.
pub fn cursor_at(text: &str, offset: f32, size: f32, weight: FontWeightHint) -> TextCursor {
    cursor_at_in(text, offset, size, weight, FontFamily::Ui)
}

/// The family-aware form of [`cursor_at`].
pub fn cursor_at_in(
    text: &str,
    offset: f32,
    size: f32,
    weight: FontWeightHint,
    family: FontFamily,
) -> TextCursor {
    let hit = with_font(size, weight, family, |font| {
        font.shape(text).offset_at(offset, text.len())
    });
    TextCursor {
        byte: hit.offset,
        affinity: hit.affinity,
    }
}

/// The cursor one step to the left of `at` on the *screen*, or `None` when
/// there is nothing further left — the caller's cue to leave the field, wrap to
/// the previous line, or do nothing.
///
/// This is what the left arrow key means, and it is not `byte - 1`. Byte order
/// is the order the text is stored in; the arrow key is about the order it is
/// drawn in, and the two part company the moment a line mixes directions. In
/// `ab` followed by a right-to-left `HR` followed by `cd` — drawn `a b R H c d`
/// — pressing left from the end visits the offsets 5, 4, 2, 3, 1, 0. The pair
/// in the middle *increases* while the caret moves left, because crossing `R`
/// leftward moves later in the string. Decrementing the offset instead walks 5,
/// 4, 3, 2, 1, 0, which lands the caret on the far side of the right-to-left
/// word on the first press and back on the second: it teleports rather than
/// steps.
///
/// The returned affinity is not decoration. Where two directions meet, one gap
/// on the screen has two byte offsets, and only the affinity says which of them
/// this caret is. Store the whole cursor, not its `byte()`, or the next
/// [`caret_x`] will draw it at the other end of the run.
pub fn caret_left(
    text: &str,
    at: TextCursor,
    size: f32,
    weight: FontWeightHint,
) -> Option<TextCursor> {
    caret_left_in(text, at, size, weight, FontFamily::Ui)
}

/// The family-aware form of [`caret_left`].
pub fn caret_left_in(
    text: &str,
    at: TextCursor,
    size: f32,
    weight: FontWeightHint,
    family: FontFamily,
) -> Option<TextCursor> {
    with_font(size, weight, family, |font| {
        font.shape(text).caret_left(
            Hit {
                offset: at.byte,
                affinity: at.affinity,
            },
            text.len(),
        )
    })
    .map(|hit| TextCursor {
        byte: hit.offset,
        affinity: hit.affinity,
    })
}

/// The cursor one step to the right of `at` on the screen, or `None` at the
/// right end. The mirror of [`caret_left`]; see it for why this is not
/// `byte + 1`.
pub fn caret_right(
    text: &str,
    at: TextCursor,
    size: f32,
    weight: FontWeightHint,
) -> Option<TextCursor> {
    caret_right_in(text, at, size, weight, FontFamily::Ui)
}

/// The family-aware form of [`caret_right`].
pub fn caret_right_in(
    text: &str,
    at: TextCursor,
    size: f32,
    weight: FontWeightHint,
    family: FontFamily,
) -> Option<TextCursor> {
    with_font(size, weight, family, |font| {
        font.shape(text).caret_right(
            Hit {
                offset: at.byte,
                affinity: at.affinity,
            },
            text.len(),
        )
    })
    .map(|hit| TextCursor {
        byte: hit.offset,
        affinity: hit.affinity,
    })
}

/// The character index in `text` nearest to `offset` pixels from its start.
///
/// This is what a click on a line of text means: the caret goes to the closest
/// gap between characters, not to the one the click landed inside, so clicking
/// the right half of a letter puts the caret after it.
///
/// Counts *characters*, and so cannot describe a caret: see [`cursor_at`] for
/// the query that can.
pub fn char_index_at(text: &str, offset: f32, size: f32, weight: FontWeightHint) -> usize {
    if offset <= 0.0 {
        return 0;
    }
    // The shaped run answers in byte offsets, which is the honest currency —
    // a caret can only sit at a glyph boundary, and with ligatures that is not
    // every character boundary. This converts because the callers still count
    // characters; the conversion is where a caret inside a ligature gets
    // rounded to the ligature's start rather than into the middle of it.
    // The affinity is dropped: this returns a character index, and both sides
    // of a direction boundary are the *same* index — the affinity says which
    // of its two screen positions a caret should be drawn at, which is a
    // question for whoever draws the caret, not for whoever counts characters.
    let at = with_font(size, weight, FontFamily::Ui, |font| {
        font.shape(text).offset_at(offset, text.len()).offset
    });
    text.get(..at)
        .map_or_else(|| text.chars().count(), |prefix| prefix.chars().count())
}

/// Cost of shaping, measured rather than guessed.
///
/// `apps/editor` scrolls sideways by slicing each line at a byte offset and
/// shaping only the tail. That is incompatible with bidirectional text — the
/// visible part of a mixed-direction line is not the shaping of a substring of
/// it — and the replacement is to shape the whole line and clip. Whether that
/// is affordable is a question about *this* function's cost as a function of
/// line length, and `known-issues.md` → `TD-EDITOR-IS-NOT-BIDIRECTIONAL` asked
/// for a measurement before the choice was made. This module is that
/// measurement, kept in the tree so the number can be re-taken on other
/// hardware rather than believed on this one.
///
/// The tests are `#[ignore]`d: they are instruments, not assertions, and
/// timings on a loaded CI box would be noise. Run them deliberately:
///
/// ```text
/// cargo test --release -p guitk --lib shaping_cost -- --ignored --nocapture --test-threads=1
/// ```
///
/// `--release` and `--test-threads=1` are both required for the numbers to
/// mean anything: a debug build measures the absence of inlining, and two of
/// these tests in parallel contend on the toolkit's global font-cache mutex.
///
/// **Every figure printed here is the fastest of many samples, not the median.**
/// That is not the conventional choice and it was not the first one — see
/// `best` and `is_this_instrument_stable` for the measurement that forced it.
#[cfg(test)]
mod shaping_cost {
    use super::*;
    use std::time::Instant;

    const SIZE: f32 = 14.0;

    /// A minified-JavaScript-ish line: no spaces to break on, mixed character
    /// classes, the shape of the worst line a code editor actually meets.
    fn pathological(chars: usize) -> String {
        const CYCLE: &str = "a1(){};=>[]\"x\",";
        CYCLE.chars().cycle().take(chars).collect()
    }

    /// Cost of one shaping, in microseconds: the fastest of `runs` of them.
    ///
    /// See `best` for why the fastest and not the median or the mean.
    ///
    /// The font is fetched **once**, outside the timed loop. `with_font` takes
    /// a global mutex and a cache lookup per call, and measuring that here
    /// would answer a different question than the one asked — and, because the
    /// mutex is shared, would let two tests running concurrently contend and
    /// inflate each other. (That is not hypothetical: the first run of this
    /// instrument reported 4.5ms to shape 80 characters, which was two tests
    /// fighting over the lock, not shaping.) These tests must run with
    /// `--test-threads=1` regardless; see the module doc.
    fn shape_us(text: &str, runs: usize) -> f64 {
        with_font(SIZE, FontWeightHint::Regular, FontFamily::Mono, |font| {
            best(&samples_us(font, text, runs))
        })
    }

    /// Timed shapings of `text`, in microseconds, sorted ascending.
    ///
    /// One warm-up shaping happens outside the samples: the first shaping of a
    /// size populates per-glyph caches inside the face, and charging that to
    /// the first sample would overstate every short-line figure.
    fn samples_us(font: &osfont::system::SystemFont, text: &str, runs: usize) -> Vec<f64> {
        let _ = font.shape(text);
        // The width is returned rather than dropped so the shaping cannot be
        // optimised away as dead.
        timed(runs, || font.shape(text).width())
    }

    /// Times `f` repeatedly, returning the durations in microseconds, sorted.
    ///
    /// Sampling continues until *both* `runs` samples have been taken and
    /// `MIN_SAMPLING_US` of wall time has gone into them. The second condition
    /// is what makes a cheap workload measurable: this machine stalls every
    /// process for a millisecond or two at a time, several times a second, and
    /// a measurement whose whole sample run fits inside one such stall has no
    /// quiet sample to find — every one of its samples is stalled, so the
    /// minimum is stalled too. 51 shapings of the built-in face is 0.7ms of
    /// work and was routinely swallowed whole; 20ms is not.
    ///
    /// `f` returns a value only so the compiler cannot delete the work.
    fn timed(runs: usize, mut f: impl FnMut() -> f32) -> Vec<f64> {
        /// Enough sampling time to be sure of straddling a stall.
        const MIN_SAMPLING_US: f64 = 20_000.0;
        /// A stop for a workload so cheap the time floor would never be met.
        const MAX_RUNS: usize = 200_000;

        let mut samples: Vec<f64> = Vec::with_capacity(runs);
        let mut spent = 0.0f64;
        while samples.len() < runs || (spent < MIN_SAMPLING_US && samples.len() < MAX_RUNS) {
            let t = Instant::now();
            let keep = f();
            let elapsed = t.elapsed().as_secs_f64() * 1e6;
            assert!(keep >= 0.0);
            spent += elapsed;
            samples.push(elapsed);
        }
        samples.sort_by(f64::total_cmp);
        samples
    }

    /// The fastest sample — the statistic every instrument here reports.
    ///
    /// Not the median, which is the obvious choice and the wrong one. Noise on
    /// this machine is one-sided: nothing can make a shaping finish sooner than
    /// the code allows, but a scheduler stall, a cache eviction or a clock-speed
    /// change can make it finish later. So the fastest of many samples estimates
    /// the quantity actually being asked about — what the code costs when it is
    /// allowed to run — and everything above it is a measurement of Windows.
    ///
    /// This is not a preference, it is measured: `is_this_instrument_stable`
    /// reports the same workload twelve times over, and across those twelve the
    /// median moved by 1.89x while the minimum moved by 1.02x. The 1.7x
    /// disagreement between two runs of `shaping_cost_by_line_length` that
    /// prompted the check was entirely this.
    #[allow(clippy::indexing_slicing)]
    fn best(sorted: &[f64]) -> f64 {
        sorted[0]
    }

    /// Middle element of an ascending list. Kept only for
    /// `is_this_instrument_stable`, which exists to show why nothing else
    /// should use it.
    #[allow(clippy::indexing_slicing)]
    fn median(sorted: &[f64]) -> f64 {
        sorted[sorted.len() / 2]
    }

    /// The same measurement against the built-in bitmap face, which has no
    /// layout tables at all — one glyph per character, no `GSUB`, no kerning.
    /// It is the floor: whatever it costs is the cost of the shaping pipeline's
    /// bookkeeping alone, and the gap between it and the system face is what
    /// the layout tables are charging.
    fn shape_builtin_us(text: &str, runs: usize) -> f64 {
        let font = osfont::system::SystemFont::builtin(SIZE);
        best(&samples_us(&font, text, runs))
    }

    /// What one `with_font` costs on its own — the mutex plus the cache lookup
    /// that every `measure`/`shape` call in the toolkit pays before it shapes
    /// anything. Reported alongside the shaping figures because if it is
    /// comparable to shaping a short line, then *it*, and not shaping, is what
    /// a renderer doing one call per syntax token should worry about.
    ///
    /// Timed in **batches** of `BATCH`, unlike everything else here. The
    /// fastest-sample rule needs the thing being timed to be comfortably larger
    /// than the clock's resolution, and one cache hit is not: timed singly, the
    /// minimum of a hundred thousand samples came out as a flat `0.000us`,
    /// which is the clock reporting that it cannot see this, not the lookup
    /// being free. A batch of a thousand is microseconds wide and divides back
    /// down cleanly.
    fn font_lookup_us(runs: usize) -> f64 {
        /// Enough lookups per sample to dwarf the clock's resolution.
        const BATCH: usize = 1_000;

        let batch = best(&timed(runs, || {
            let mut total = 0.0f32;
            for _ in 0..BATCH {
                total += with_font(SIZE, FontWeightHint::Regular, FontFamily::Mono, |font| {
                    font.metrics().line_height
                });
            }
            total
        }));
        #[allow(clippy::cast_precision_loss)]
        {
            batch / BATCH as f64
        }
    }

    /// What it costs to shape one line, from a screenful to absurd.
    ///
    /// The editor's question is not "what does one line cost" but "what does a
    /// *screen* cost", so the report converts: a full-height window shows
    /// roughly 50 lines, and a 60Hz frame is 16.7ms.
    #[test]
    #[ignore = "timing instrument, not an assertion; see the module doc"]
    fn shaping_cost_by_line_length() {
        const LINES_ON_SCREEN: f64 = 50.0;
        const FRAME_BUDGET_US: f64 = 16_700.0;

        // Fewer samples than elsewhere because each is a batch of a thousand.
        println!("\none with_font(): {:.3}us", font_lookup_us(201));
        println!(
            "\n{:>9}  {:>12}  {:>12}  {:>14}  {:>9}",
            "chars", "shape (us)", "builtin", "50 lines (ms)", "of frame"
        );
        for chars in [80usize, 200, 1_000, 5_000, 20_000] {
            let text = pathological(chars);
            // Fewer runs at the big sizes: the point is the shape of the curve,
            // and a 20k-char shaping repeated 200 times is minutes of nothing.
            let runs = if chars <= 1_000 { 201 } else { 21 };
            let us = shape_us(&text, runs);
            let builtin = shape_builtin_us(&text, runs);
            let screen_ms = us * LINES_ON_SCREEN / 1000.0;
            println!(
                "{chars:>9}  {us:>12.1}  {builtin:>12.1}  {screen_ms:>14.2}  {:>8.1}%",
                us * LINES_ON_SCREEN / FRAME_BUDGET_US * 100.0
            );
        }
        println!(
            "\n(a frame is {FRAME_BUDGET_US:.0}us at 60Hz; {LINES_ON_SCREEN:.0} lines assumed \
             visible)"
        );
    }

    /// The comparison that actually decides the design: shaping the whole line
    /// every frame, against shaping only the ~200 characters that fit on
    /// screen — which is what the byte-slicing buys today.
    #[test]
    #[ignore = "timing instrument, not an assertion; see the module doc"]
    fn whole_line_against_visible_window() {
        const VISIBLE: usize = 200;
        println!(
            "\n{:>9}  {:>12}  {:>12}  {:>8}",
            "chars", "whole", "visible", "ratio"
        );
        for chars in [200usize, 1_000, 5_000, 20_000] {
            let whole = pathological(chars);
            let window = pathological(VISIBLE.min(chars));
            let runs = if chars <= 1_000 { 201 } else { 21 };
            let w = shape_us(&whole, runs);
            let v = shape_us(&window, runs);
            println!("{chars:>9}  {w:>12.1}  {v:>12.1}  {:>7.1}x", w / v);
        }
    }

    /// One line shaped once, against the same line shaped a piece at a time.
    ///
    /// This is the measurement the editor's `draw_tokens` turns on. It emits
    /// one run *per syntax token*, measuring each to place the next — so a
    /// 200-character line of forty tokens is forty shapings of five characters,
    /// not one shaping of two hundred. If a shaping has a large fixed cost, the
    /// per-token decomposition is not a small price paid for coloured text: it
    /// is the dominant cost of drawing the line, and shaping the line whole is
    /// *cheaper* as well as being what bidi correctness requires.
    ///
    /// See `TD-EDITOR-IS-NOT-BIDIRECTIONAL` item 4 and
    /// `C-FONT-SHAPING-IS-1400X-SLOWER-THAN-IT-SHOULD-BE`.
    #[test]
    #[ignore = "timing instrument, not an assertion; see the module doc"]
    fn whole_line_against_one_run_per_token() {
        println!(
            "\n{:>9}  {:>7}  {:>10}  {:>12}  {:>10}  {:>8}",
            "chars", "pieces", "whole (us)", "in pieces", "per piece", "ratio"
        );
        for chars in [80usize, 200, 1_000] {
            let text = pathological(chars);
            let whole = shape_us(&text, 201);
            for pieces in [2usize, 5, 10, 20, 40] {
                // Split on byte boundaries, which `pathological` makes safe:
                // it is ASCII, so a byte offset is a character offset and no
                // chunk can land inside a character.
                let width = chars.div_ceil(pieces);
                let parts: Vec<String> = text
                    .as_bytes()
                    .chunks(width)
                    .map(|c| String::from_utf8_lossy(c).into_owned())
                    .collect();
                let split = shape_each_us(&parts, 201);
                let n = parts.len();
                #[allow(clippy::cast_precision_loss)]
                let each = split / n as f64;
                println!(
                    "{chars:>9}  {n:>7}  {whole:>10.1}  {split:>12.1}  {each:>10.1}  {:>7.2}x",
                    split / whole,
                );
            }
        }
        println!(
            "\n(a ratio above 1.0 means shaping the line in pieces costs more \
             than shaping it whole)"
        );
    }

    /// Cost of shaping *every* string in `parts` — the fastest such pass.
    ///
    /// The whole list is one sample rather than one sample each, because the
    /// question is what a renderer pays to draw one line — which is the sum,
    /// not any single piece.
    fn shape_each_us(parts: &[String], runs: usize) -> f64 {
        with_font(SIZE, FontWeightHint::Regular, FontFamily::Mono, |font| {
            for part in parts {
                let _ = font.shape(part);
            }
            best(&timed(runs, || {
                let mut total = 0.0f32;
                for part in parts {
                    total += font.shape(part).width();
                }
                total
            }))
        })
    }

    /// Is this instrument stable enough to draw conclusions from?
    ///
    /// It has to be asked, because two runs of the *identical* measurement —
    /// the median of 201 shapings of a 1000-character line, each the only test
    /// running — came back 776us and 1313us, a factor of 1.7. A number that
    /// moves by 1.7x between runs cannot settle a 1.3x question, and the
    /// tables in `known-issues.md` were full of 1.2-1.4x questions.
    ///
    /// So: repeat one workload many times inside a single process, and
    /// alongside it repeat the *same* workload on the built-in bitmap face,
    /// which runs the same pipeline with no layout tables. If both columns
    /// drift together the cause is outside the code being measured — clock
    /// speed, thermal state, whatever else the machine is doing — and the
    /// honest way to report a result is the ratio, which divides that out.
    /// If the system column drifts and the built-in one does not, the drift is
    /// in the layout tables and is a real finding about them.
    #[test]
    #[ignore = "timing instrument, not an assertion; see the module doc"]
    fn is_this_instrument_stable() {
        const BLOCKS: usize = 12;
        const RUNS: usize = 51;

        let text = pathological(1_000);
        println!(
            "\n{:>6}  {:>10}  {:>10}  {:>10}  {:>10}",
            "block", "sys med", "sys best", "bi med", "bi best"
        );
        let mut sys_med = Vec::with_capacity(BLOCKS);
        let mut sys_best = Vec::with_capacity(BLOCKS);
        let mut bi_med = Vec::with_capacity(BLOCKS);
        let mut bi_best = Vec::with_capacity(BLOCKS);
        for block in 0..BLOCKS {
            let s = with_font(SIZE, FontWeightHint::Regular, FontFamily::Mono, |font| {
                samples_us(font, &text, RUNS)
            });
            let builtin = osfont::system::SystemFont::builtin(SIZE);
            let b = samples_us(&builtin, &text, RUNS);
            let (sm, sb) = (median(&s), best(&s));
            let (bm, bb) = (median(&b), best(&b));
            println!("{block:>6}  {sm:>10.1}  {sb:>10.1}  {bm:>10.1}  {bb:>10.1}");
            sys_med.push(sm);
            sys_best.push(sb);
            bi_med.push(bm);
            bi_best.push(bb);
        }

        // The ratio is formed from the *best* of each block, because that is
        // the candidate statistic being tested against the median.
        let ratios: Vec<f64> = sys_best.iter().zip(&bi_best).map(|(s, b)| s / b).collect();
        println!(
            "\n{:>12}  {:>10}  {:>10}  {:>8}",
            "column", "min", "max", "spread"
        );
        for (name, v) in [
            ("system med", &sys_med),
            ("system best", &sys_best),
            ("builtin med", &bi_med),
            ("builtin best", &bi_best),
            ("best ratio", &ratios),
        ] {
            let lo = v.iter().copied().fold(f64::INFINITY, f64::min);
            let hi = v.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            println!("{name:>12}  {lo:>10.2}  {hi:>10.2}  {:>7.2}x", hi / lo);
        }
        println!(
            "\n(whichever column has the smallest spread is the statistic these \
             instruments should be reporting)"
        );
    }

    /// The one assertion in this module, and the only one here that runs by
    /// default: shaping a line on the system face must not cost absurdly more
    /// than shaping it on the built-in bitmap face.
    ///
    /// Stated as a *ratio* rather than a microsecond figure on purpose. An
    /// absolute threshold would have to be loose enough to survive a debug
    /// build, a loaded machine and slower hardware, and by the time it is that
    /// loose it no longer catches anything. The built-in face runs the same
    /// shaping pipeline with no layout tables at all, so it absorbs all three
    /// of those and what is left is what the layout tables charge — which is
    /// exactly the quantity that regressed.
    ///
    /// The bug this guards against (`C-FONT-SHAPING-IS-1400X-SLOWER-THAN-IT-
    /// SHOULD-BE`) sat at 1400-2200x. With the `GSUB` and `GPOS` coverage
    /// digests in place it measures **38x in release and 71x in a debug build**
    /// on the development host, so 500x sits seven times above the worse of the
    /// two and still nearly three times *below* the bug — it cannot fire on a
    /// slow machine and cannot miss the regression.
    ///
    /// (Those two figures were 80x and 147x while this module reported medians.
    /// Nothing about the code changed; the median was measuring Windows on top
    /// of the shaping, and roughly doubled both. See `best`.)
    #[test]
    fn the_layout_tables_do_not_dominate_shaping() {
        let text = pathological(80);
        let system = shape_us(&text, 51);
        let builtin = shape_builtin_us(&text, 51);
        // A machine with no system fonts falls back to the built-in face, and
        // then this is comparing the face against itself — true, but vacuous.
        assert!(builtin > 0.0, "the built-in face must shape something");
        let ratio = system / builtin;
        // Captured unless the run asks for `--nocapture`, which is the only
        // way to see how much margin is actually left against the threshold.
        println!("80 chars: {system:.1}us system, {builtin:.1}us builtin, {ratio:.0}x");
        assert!(
            ratio < 500.0,
            "shaping 80 chars costs {system:.1}us on the system face against \
             {builtin:.1}us built-in ({ratio:.0}x); the layout-table skip has \
             regressed",
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A surface pre-filled with an obvious background, so any changed pixel
    /// is unambiguously something the text drew.
    fn blank(width: u32, height: u32) -> Vec<u32> {
        vec![0xFF00_0000; (width as usize) * (height as usize)]
    }

    fn changed(before: &[u32], after: &[u32]) -> usize {
        before.iter().zip(after).filter(|(a, b)| a != b).count()
    }

    #[test]
    fn drawing_into_a_surface_marks_pixels() {
        let (w, h) = (120_u32, 32_u32);
        let before = blank(w, h);
        let mut pixels = before.clone();
        let mut surface = Surface {
            pixels: &mut pixels,
            width: w,
            height: h,
        };
        let end = draw_into(
            &mut surface,
            "Hi",
            2.0,
            22.0,
            16.0,
            FontWeightHint::Regular,
            Color::WHITE,
        );
        assert!(end > 2.0, "the pen must advance past the start, got {end}");
        assert!(
            changed(&before, &pixels) > 0,
            "drawing 'Hi' left the surface untouched"
        );
    }

    /// The property the whole module exists for: what is drawn is as wide as
    /// what was measured, because both come from one font cache.
    #[test]
    fn the_pen_advance_equals_the_measured_width() {
        let (w, h) = (400_u32, 40_u32);
        let mut pixels = blank(w, h);
        let mut surface = Surface {
            pixels: &mut pixels,
            width: w,
            height: h,
        };
        let end = draw_into(
            &mut surface,
            "Widths must agree",
            10.0,
            28.0,
            16.0,
            FontWeightHint::Regular,
            Color::WHITE,
        );
        let measured = measure("Widths must agree", 16.0, FontWeightHint::Regular);
        assert!(
            (end - 10.0 - measured).abs() < 0.01,
            "drew to {end} from x=10 but measured {measured}"
        );
    }

    /// Off-surface text must be clipped, not wrapped into the rows above — the
    /// failure mode of a stride-based blit that forgets to bounds-check.
    #[test]
    fn text_placed_outside_the_surface_draws_nothing() {
        let (w, h) = (60_u32, 20_u32);
        let before = blank(w, h);
        let mut pixels = before.clone();
        let mut surface = Surface {
            pixels: &mut pixels,
            width: w,
            height: h,
        };
        draw_into(
            &mut surface,
            "offscreen",
            0.0,
            500.0,
            16.0,
            FontWeightHint::Regular,
            Color::WHITE,
        );
        assert_eq!(changed(&before, &pixels), 0, "text below the surface drew");
    }

    /// A buffer that does not hold `width * height` pixels means the caller's
    /// stride is wrong; drawing anyway would put every row after the first in
    /// the wrong place.
    #[test]
    fn an_undersized_buffer_is_refused_rather_than_drawn_into() {
        let mut pixels = vec![0xFF00_0000_u32; 10];
        let before = pixels.clone();
        let mut surface = Surface {
            pixels: &mut pixels,
            width: 100,
            height: 100,
        };
        let end = draw_into(
            &mut surface,
            "nope",
            0.0,
            10.0,
            16.0,
            FontWeightHint::Regular,
            Color::WHITE,
        );
        assert_eq!(end, 0.0, "the pen must not advance when nothing was drawn");
        assert_eq!(pixels, before);
    }

    #[test]
    fn a_fully_transparent_colour_draws_nothing() {
        let (w, h) = (100_u32, 30_u32);
        let before = blank(w, h);
        let mut pixels = before.clone();
        let mut surface = Surface {
            pixels: &mut pixels,
            width: w,
            height: h,
        };
        draw_into(
            &mut surface,
            "invisible",
            2.0,
            20.0,
            16.0,
            FontWeightHint::Regular,
            Color::TRANSPARENT,
        );
        assert_eq!(changed(&before, &pixels), 0);
    }

    #[test]
    fn measuring_counts_characters_not_bytes() {
        // The bug the old `text.len() as f32 * k` estimate had: a two-byte
        // character measured twice as wide as a one-byte one.
        let ascii = measure("eee", 16.0, FontWeightHint::Regular);
        let accented = measure("ééé", 16.0, FontWeightHint::Regular);
        assert_eq!(
            ascii, accented,
            "'ééé' is 6 bytes and 3 characters; it must measure as 3"
        );
    }

    #[test]
    fn measuring_scales_with_size() {
        let small = measure("Hello", 16.0, FontWeightHint::Regular);
        let big = measure("Hello", 48.0, FontWeightHint::Regular);
        assert!(small > 0.0);
        assert!(
            big > small,
            "48px text measured {big}, 16px measured {small}"
        );
    }

    #[test]
    fn the_empty_string_has_no_width() {
        assert_eq!(measure("", 16.0, FontWeightHint::Regular), 0.0);
    }

    #[test]
    fn fit_breaks_between_characters() {
        let text = "ééé";
        let one = measure("é", 16.0, FontWeightHint::Regular);
        // Room for two characters and a sliver: the third must be dropped
        // whole, and the cut must land on a character boundary.
        let cut = fit(text, one * 2.5, 16.0, FontWeightHint::Regular);
        assert!(text.is_char_boundary(cut), "cut {cut} splits a character");
        assert_eq!(&text[..cut], "éé");
    }

    /// Every query about a string has to be answered from the same layout.
    ///
    /// This is the bug that made the shaped run necessary: `measure` applied
    /// kerning and `fit`/`char_index_at` summed bare per-character advances,
    /// so on text with kerned pairs in it — which is most text — the prefix
    /// `fit` chose could measure *wider* than the limit it was given, and a
    /// click landed on one character while the caret was drawn at another.
    /// The strings below are chosen for their kerned pairs.
    #[test]
    fn fit_agrees_with_measure() {
        for text in ["AVATAR Types", "To Yo P. r. f)", "Wavy Table"] {
            let full = measure(text, 16.0, FontWeightHint::Regular);
            let mut max = 0.0_f32;
            while max <= full + 8.0 {
                let cut = fit(text, max, 16.0, FontWeightHint::Regular);
                assert!(
                    text.is_char_boundary(cut),
                    "{text:?}: cut {cut} splits a character"
                );
                let kept = measure(&text[..cut], 16.0, FontWeightHint::Regular);
                assert!(
                    kept <= max + 0.001,
                    "{text:?}: fit chose {cut} bytes at limit {max}, but they measure {kept}"
                );
                max += 1.0;
            }
        }
    }

    /// The property that makes [`cursor_at`] worth having over
    /// [`char_index_at`]: what it returns can be handed straight back to
    /// [`caret_x`] and lands where the user aimed. A character index cannot do
    /// that, because it has already thrown the affinity away.
    #[test]
    fn a_cursor_from_a_click_draws_its_caret_back_where_the_click_was() {
        let text = "AVATAR Types";
        let full = measure(text, 16.0, FontWeightHint::Regular);
        let mut offset = 0.0_f32;
        while offset <= full {
            let cursor = cursor_at(text, offset, 16.0, FontWeightHint::Regular);
            assert!(
                text.is_char_boundary(cursor.byte),
                "byte {} splits a character",
                cursor.byte
            );
            let x = caret_x(text, cursor, 16.0, FontWeightHint::Regular);
            assert!(
                (x - offset).abs() <= 16.0,
                "clicked at {offset}, caret drawn at {x} (byte {})",
                cursor.byte
            );
            offset += 1.0;
        }
    }

    /// `ab` + two Hebrew letters + `cd`. The Hebrew runs the other way, so it
    /// is drawn `a b <bet> <aleph> c d` — the two middle letters swap. The byte
    /// offsets are `a`=0, `b`=1, aleph=2..4, bet=4..6, `c`=6, `d`=7, end 8.
    ///
    /// Nothing here depends on the host owning a Hebrew font: the direction is
    /// a property of the characters, decided before a glyph is chosen, so a
    /// machine that draws these as blank boxes still draws them in this order.
    const MIXED: &str = "ab\u{05D0}\u{05D1}cd";

    /// Walk the caret with the arrow key until it runs out, collecting where it
    /// stopped and what it was drawn at. Bounded so a motion that fails to
    /// terminate fails the test rather than the machine.
    fn walk(text: &str, from: TextCursor, right: bool) -> Vec<(usize, f32)> {
        let mut out = Vec::new();
        let mut at = from;
        for _ in 0..64 {
            let step = if right {
                caret_right(text, at, 16.0, FontWeightHint::Regular)
            } else {
                caret_left(text, at, 16.0, FontWeightHint::Regular)
            };
            let Some(next) = step else { return out };
            out.push((
                next.byte,
                caret_x(text, next, 16.0, FontWeightHint::Regular),
            ));
            at = next;
        }
        panic!("caret motion did not terminate on {text:?}");
    }

    /// The bug this exists to fix, stated as the user sees it: pressing the
    /// right arrow moves the caret rightwards *on the screen*, every press,
    /// including across the direction boundary where the byte offsets go
    /// backwards.
    #[test]
    fn the_right_arrow_always_moves_the_caret_rightwards() {
        let stops = walk(MIXED, TextCursor::from(0), true);
        assert_eq!(
            stops.iter().map(|&(b, _)| b).collect::<Vec<_>>(),
            vec![1, 2, 4, 2, 7, 8],
            "the offsets the right arrow visits"
        );
        // 4 then 2: the offset *decreases* while the caret moves right. This is
        // the assertion an `offset + 1` cursor cannot satisfy, and the reason
        // the whole visual-order walk exists.
        for pair in stops.windows(2) {
            assert!(
                pair[1].1 > pair[0].1,
                "the right arrow moved leftwards: {stops:?}"
            );
        }
    }

    /// The mirror. Note the start: a caller holding only the text length has no
    /// affinity to offer, and must still be able to walk back from the end.
    #[test]
    fn the_left_arrow_always_moves_the_caret_leftwards() {
        let stops = walk(MIXED, TextCursor::from(MIXED.len()), false);
        assert_eq!(
            stops.iter().map(|&(b, _)| b).collect::<Vec<_>>(),
            vec![7, 6, 4, 6, 1, 0],
            "the offsets the left arrow visits"
        );
        for pair in stops.windows(2) {
            assert!(
                pair[1].1 < pair[0].1,
                "the left arrow moved rightwards: {stops:?}"
            );
        }
    }

    /// A step and its reverse return to the same *place*. The byte offset may
    /// come back as the other of the boundary's two names — where two
    /// directions meet, one gap on the screen genuinely has two offsets — so
    /// the pixel is what must round trip, and does.
    #[test]
    fn a_step_and_its_reverse_return_to_the_same_pixel() {
        let mut at = TextCursor::from(0);
        while let Some(right) = caret_right(MIXED, at, 16.0, FontWeightHint::Regular) {
            let there = caret_x(MIXED, at, 16.0, FontWeightHint::Regular);
            let back = caret_left(MIXED, right, 16.0, FontWeightHint::Regular)
                .expect("a caret that stepped right can always step back");
            let x = caret_x(MIXED, back, 16.0, FontWeightHint::Regular);
            assert!(
                (x - there).abs() < 0.001,
                "stepped right from {there} and back to {x} (byte {} -> {})",
                at.byte,
                back.byte
            );
            at = right;
        }
    }

    /// Motion reports running out rather than silently staying put, so a widget
    /// can tell "the caret moved" from "the caret is already at the edge" and
    /// hand the keypress on to whatever wraps lines or leaves the field.
    #[test]
    fn motion_stops_at_the_edges_and_says_so() {
        assert!(caret_left(MIXED, TextCursor::from(0), 16.0, FontWeightHint::Regular).is_none());
        assert!(
            caret_right(
                MIXED,
                TextCursor::from(MIXED.len()),
                16.0,
                FontWeightHint::Regular
            )
            .is_none()
        );
        // Empty text has one caret slot and no cluster to cross.
        assert!(caret_left("", TextCursor::from(0), 16.0, FontWeightHint::Regular).is_none());
        assert!(caret_right("", TextCursor::from(0), 16.0, FontWeightHint::Regular).is_none());
    }

    /// On the text almost every widget actually holds, the arrow key is the
    /// next character and nothing surprising happens. This is the case the old
    /// `offset ± 1` got right, and which must keep working.
    #[test]
    fn on_one_direction_text_the_arrows_are_the_next_character() {
        let text = "hello";
        assert_eq!(
            walk(text, TextCursor::from(0), true)
                .iter()
                .map(|&(b, _)| b)
                .collect::<Vec<_>>(),
            vec![1, 2, 3, 4, 5]
        );
        assert_eq!(
            walk(text, TextCursor::from(text.len()), false)
                .iter()
                .map(|&(b, _)| b)
                .collect::<Vec<_>>(),
            vec![4, 3, 2, 1, 0]
        );
    }

    /// Why a widget must store the whole cursor and not just `.byte()`: rebuild
    /// the cursor from its offset each step and the caret skips the reordered
    /// run entirely, in one press, silently.
    ///
    /// The offset `2` where `b` meets the Hebrew is one gap on the screen with
    /// two names — the trailing edge of `b` and the trailing edge of the last
    /// Hebrew letter are the same pixels — and dropping the affinity throws
    /// away which of them the caret is. This is a *test of the failure*, kept
    /// so that a widget refactor which regresses to a bare `usize` is caught by
    /// this file rather than by a user typing Hebrew.
    #[test]
    fn dropping_the_affinity_between_steps_skips_the_reordered_run() {
        let mut byte = 0usize;
        let mut visited = vec![];
        for _ in 0..8 {
            let Some(next) = caret_right(MIXED, byte.into(), 16.0, FontWeightHint::Regular) else {
                break;
            };
            byte = next.byte(); // The mistake: the affinity is discarded here.
            visited.push(byte);
        }
        assert_eq!(
            visited,
            vec![1, 2, 7, 8],
            "an affinity-less walk should visibly skip the Hebrew; if this now \
             matches the full walk, the loose fallback changed and the widgets \
             may no longer need to carry a TextCursor"
        );
        // The same walk carrying the cursor visits six places, not four.
        assert_eq!(walk(MIXED, TextCursor::from(0), true).len(), 6);
    }

    /// A character wider than one byte is crossed whole. The old arithmetic
    /// would have landed inside it.
    #[test]
    fn a_multi_byte_character_is_crossed_whole() {
        let text = "aéb";
        for &(b, _) in &walk(text, TextCursor::from(0), true) {
            assert!(text.is_char_boundary(b), "byte {b} splits a character");
        }
        assert_eq!(
            walk(text, TextCursor::from(0), true)
                .iter()
                .map(|&(b, _)| b)
                .collect::<Vec<_>>(),
            vec![1, 3, 4],
            "é occupies bytes 1..3"
        );
    }

    /// A byte offset alone still names a caret, because the default affinity is
    /// the answer a caller with no opinion wants. This is what keeps every
    /// existing `cursor = n` call site correct.
    #[test]
    fn a_bare_byte_offset_is_still_a_cursor() {
        let text = "hello";
        assert_eq!(TextCursor::from(3).byte(), 3);
        assert_eq!(TextCursor::from(3).affinity, Affinity::Downstream);
        assert_eq!(TextCursor::default(), TextCursor::from(0));
        // On single-direction text the two affinities name the same point, so
        // the choice cannot go wrong there — which is most of the UI.
        let down = TextCursor {
            byte: 3,
            affinity: Affinity::Downstream,
        };
        let up = TextCursor {
            byte: 3,
            affinity: Affinity::Upstream,
        };
        let dx = caret_x(text, down, 16.0, FontWeightHint::Regular);
        let ux = caret_x(text, up, 16.0, FontWeightHint::Regular);
        assert!((dx - ux).abs() < 0.001, "{dx} vs {ux}");
    }

    /// On text that runs one way a selection is one box, and it is exactly the
    /// stretch between the two carets — the case the old single-rectangle code
    /// got right, and which must keep working.
    #[test]
    fn a_left_to_right_selection_is_one_box_between_its_carets() {
        let text = "AVATAR Types";
        for (from, to) in [(0usize, 5usize), (2, 9), (0, text.len()), (6, 7)] {
            let boxes = selection_boxes(text, from, to, 16.0, FontWeightHint::Regular);
            assert_eq!(boxes.len(), 1, "{from}..{to} gave {boxes:?}");
            let lo = caret_x(text, from.into(), 16.0, FontWeightHint::Regular);
            let hi = caret_x(text, to.into(), 16.0, FontWeightHint::Regular);
            let (bx, bw) = boxes[0];
            assert!(
                (bx - lo).abs() < 0.001 && (bx + bw - hi).abs() < 0.001,
                "{boxes:?} vs {lo}..{hi}"
            );
        }
    }

    /// An empty or backwards range paints nothing — a caret is not a selection,
    /// and a zero-width highlight rectangle is a visible artefact on some
    /// rasterisers.
    #[test]
    fn an_empty_selection_paints_nothing() {
        let text = "AVATAR Types";
        assert!(selection_boxes(text, 3, 3, 16.0, FontWeightHint::Regular).is_empty());
        assert!(selection_boxes(text, 5, 2, 16.0, FontWeightHint::Regular).is_empty());
        assert!(selection_boxes("", 0, 0, 16.0, FontWeightHint::Regular).is_empty());
    }

    /// A click and the caret it produces must land in the same place.
    #[test]
    fn a_click_and_the_caret_agree() {
        let text = "AVATAR Types";
        let full = measure(text, 16.0, FontWeightHint::Regular);
        let mut offset = 0.0_f32;
        while offset <= full {
            let n = char_index_at(text, offset, 16.0, FontWeightHint::Regular);
            assert!(n <= text.chars().count());
            // The caret is drawn at the width of the text before it, so the
            // gap it names must be no further from the click than the widest
            // glyph in the run — otherwise the caret visibly jumps away from
            // where the user pressed.
            let before: String = text.chars().take(n).collect();
            let x = measure(&before, 16.0, FontWeightHint::Regular);
            assert!(
                (x - offset).abs() <= 16.0,
                "clicked at {offset}, caret drawn at {x} (character {n})"
            );
            offset += 1.0;
        }
    }

    #[test]
    fn fit_handles_the_degenerate_widths() {
        assert_eq!(fit("abc", 0.0, 16.0, FontWeightHint::Regular), 0);
        assert_eq!(fit("abc", -5.0, 16.0, FontWeightHint::Regular), 0);
        assert_eq!(fit("abc", 1e9, 16.0, FontWeightHint::Regular), 3);
        assert_eq!(fit("", 100.0, 16.0, FontWeightHint::Regular), 0);
    }

    #[test]
    fn elided_text_actually_fits() {
        // The point of measuring the ellipsis: a truncated label that still
        // overflows collides with whatever is beside it.
        let text = "a very long label indeed";
        for max in [10.0, 40.0, 80.0, 160.0] {
            let out = elide(text, max, "...", 16.0, FontWeightHint::Regular);
            assert!(
                measure(&out, 16.0, FontWeightHint::Regular) <= max,
                "{out:?} is wider than {max}"
            );
        }
    }

    #[test]
    fn text_that_fits_is_not_elided() {
        let out = elide("short", 1000.0, "...", 16.0, FontWeightHint::Regular);
        assert_eq!(out, "short");
    }

    #[test]
    fn start_elided_text_actually_fits() {
        let path = "/home/user/projects/some/rather/deep/tree/notes.txt";
        for max in [10.0, 40.0, 80.0, 160.0] {
            let out = elide_start(path, max, "...", 16.0, FontWeightHint::Regular);
            assert!(
                measure(&out, 16.0, FontWeightHint::Regular) <= max,
                "{out:?} is wider than {max}"
            );
        }
    }

    #[test]
    fn start_eliding_keeps_the_end() {
        // The whole point for a path: the filename survives, the leading
        // directories are what get dropped.
        let path = "/home/user/projects/some/rather/deep/tree/notes.txt";
        let out = elide_start(path, 160.0, "...", 16.0, FontWeightHint::Regular);
        assert!(out.starts_with("..."), "{out:?} should be marked as cut");
        assert!(out.ends_with("notes.txt"), "{out:?} lost the filename");
    }

    #[test]
    fn start_eliding_breaks_between_characters() {
        // A byte index into the middle of a UTF-8 sequence is not a string
        // boundary; slicing there aborts rather than looking wrong, so this is
        // a crash test, not a layout one.
        let path = "/home/user/projets/déjà-vu/résumé-final.txt";
        for max in [4.0, 9.0, 17.0, 33.0, 65.0, 129.0] {
            let out = elide_start(path, max, "…", 16.0, FontWeightHint::Regular);
            assert!(
                measure(&out, 16.0, FontWeightHint::Regular) <= max,
                "{out:?} > {max}"
            );
        }
    }

    #[test]
    fn fit_end_is_the_mirror_of_fit() {
        let s = "abcdef";
        let w = measure("abc", 16.0, FontWeightHint::Regular);
        // Room for exactly three characters, taken from the right.
        assert_eq!(&s[fit_end(s, w, 16.0, FontWeightHint::Regular)..], "def");
        assert_eq!(
            fit_end(s, 1e9, 16.0, FontWeightHint::Regular),
            0,
            "all of it fits"
        );
        assert_eq!(
            fit_end(s, -5.0, 16.0, FontWeightHint::Regular),
            s.len(),
            "no room means an empty suffix"
        );
    }

    #[test]
    fn clicking_a_character_snaps_to_the_nearer_gap() {
        let w = measure("m", 16.0, FontWeightHint::Regular);
        let f = |x| char_index_at("mmm", x, 16.0, FontWeightHint::Regular);
        assert_eq!(f(-10.0), 0, "left of the text is the start");
        assert_eq!(f(0.0), 0);
        assert_eq!(f(w * 0.4), 0, "left half of the first character");
        assert_eq!(f(w * 0.6), 1, "right half of the first character");
        assert_eq!(f(w * 1.6), 2);
        assert_eq!(f(w * 100.0), 3, "past the end is the end");
    }

    #[test]
    fn centering_puts_equal_space_on_both_sides() {
        let text = "centred";
        let (size, weight) = (16.0, FontWeightHint::Regular);
        let x = center_x(text, 100.0, size, weight);
        let left = x;
        let right = x + measure(text, size, weight);
        assert!(
            (100.0 - left - (right - 100.0)).abs() < 0.01,
            "{left}..{right} is not centred on 100"
        );
    }

    #[test]
    fn centering_is_not_biased_by_byte_length() {
        // The bug the old estimates had: an accented label measured twice as
        // wide, so centring it pushed it half a label to the left.
        let (size, weight) = (16.0, FontWeightHint::Regular);
        assert_eq!(
            center_x("eee", 100.0, size, weight),
            center_x("ééé", 100.0, size, weight)
        );
    }

    #[test]
    fn right_alignment_ends_where_asked() {
        let text = "right";
        let (size, weight) = (16.0, FontWeightHint::Regular);
        let x = right_x(text, 250.0, size, weight);
        assert!((x + measure(text, size, weight) - 250.0).abs() < 0.01);
    }

    #[test]
    fn line_height_exceeds_ascent() {
        // Not a tautology: a face whose descent was folded into the ascent
        // would place every baseline one descent too low.
        for size in [11.0, 16.0, 48.0] {
            let lh = line_height(size, FontWeightHint::Regular);
            let asc = ascent(size, FontWeightHint::Regular);
            assert!(asc > 0.0, "{size}px: ascent {asc}");
            assert!(lh > asc, "{size}px: line height {lh} <= ascent {asc}");
        }
    }

    #[test]
    fn bold_is_wider_than_regular() {
        let regular = measure("lll", 16.0, FontWeightHint::Regular);
        let bold = measure("lll", 16.0, FontWeightHint::Bold);
        assert!(bold >= regular, "bold {bold} < regular {regular}");
    }

    #[test]
    fn a_padded_box_holds_its_text_and_its_padding() {
        let label = "Preferences";
        let w = padded_width(label, 12.0, 13.0, FontWeightHint::Regular);
        assert!((w - measure(label, 13.0, FontWeightHint::Regular) - 24.0).abs() < 0.01);
        // Zero padding is the bare text, not a special case.
        assert!(
            (padded_width(label, 0.0, 13.0, FontWeightHint::Regular)
                - measure(label, 13.0, FontWeightHint::Regular))
            .abs()
                < 0.01
        );
    }

    #[test]
    fn a_padded_box_is_not_sized_by_byte_length() {
        // Same glyph count, three times the bytes. Sized the old way the second
        // box was three times the first; measured they are comparable.
        let ascii = padded_width("aaaa", 10.0, 13.0, FontWeightHint::Regular);
        let wide = padded_width("ええええ", 10.0, 13.0, FontWeightHint::Regular);
        assert!(wide < ascii * 3.0, "{ascii} vs {wide}");
    }

    #[test]
    fn an_any_weight_box_fits_both_weights() {
        for label in ["Visual", "Magnifier", "Keyboard & Mouse"] {
            let w = padded_width_any_weight(label, 9.0, 12.0);
            for weight in [FontWeightHint::Bold, FontWeightHint::Regular] {
                assert!(
                    measure(label, 12.0, weight) + 18.0 <= w + 0.01,
                    "{label:?} overflows at {weight:?}"
                );
            }
        }
    }

    #[test]
    fn an_any_weight_box_does_not_change_with_the_weight() {
        // The point of it: the box a tab gets must not depend on whether that
        // tab happens to be the selected one, or the strip walks sideways.
        let a = padded_width_any_weight("Audio", 9.0, 12.0);
        let b = padded_width_any_weight("Audio", 9.0, 12.0);
        assert_eq!(a, b);
        assert!(a >= padded_width("Audio", 9.0, 12.0, FontWeightHint::Bold) - 0.01);
        assert!(a >= padded_width("Audio", 9.0, 12.0, FontWeightHint::Regular) - 0.01);
    }

    #[test]
    fn wrapped_lines_fit_the_width_they_were_given() {
        let text = "the quick brown fox jumps over the lazy dog and keeps on running";
        for max in [60.0, 120.0, 240.0] {
            for line in wrap(text, max, 11.0, FontWeightHint::Regular) {
                // A lone over-long word is allowed past the limit — it is not
                // broken mid-word — but a line that combined words is not.
                if line.split_whitespace().count() < 2 {
                    continue;
                }
                assert!(
                    measure(&line, 11.0, FontWeightHint::Regular) <= max,
                    "{line:?} is wider than the {max}px box it was wrapped into"
                );
            }
        }
    }

    #[test]
    fn wrapping_never_loses_a_word() {
        let text = "Permission is hereby granted, free of charge, to any person";
        let lines = wrap(text, 90.0, 11.0, FontWeightHint::Regular);
        assert_eq!(
            lines.join(" ").split_whitespace().collect::<Vec<_>>(),
            text.split_whitespace().collect::<Vec<_>>()
        );
    }

    #[test]
    fn wrapping_honours_existing_newlines() {
        // A blank line between paragraphs has to survive, or a licence's
        // structure collapses into one run-on block.
        let lines = wrap("first\n\nsecond", 1000.0, 11.0, FontWeightHint::Regular);
        assert_eq!(lines, vec!["first", "", "second"]);
    }

    #[test]
    fn wrapping_is_not_decided_by_byte_length() {
        // Same glyph count, twice the bytes. Wrapped on a byte count the
        // accented text would break into twice as many lines.
        let ascii = wrap(
            "aaa aaa aaa aaa aaa aaa",
            80.0,
            11.0,
            FontWeightHint::Regular,
        );
        let accented = wrap(
            "ééé ééé ééé ééé ééé ééé",
            80.0,
            11.0,
            FontWeightHint::Regular,
        );
        assert_eq!(ascii.len(), accented.len());
    }

    #[test]
    fn wrapping_into_no_width_does_not_explode() {
        // The degenerate case: a greedy wrap would answer with one word per
        // line, so a paragraph in a zero-width box would report a line count
        // proportional to its word count.
        let lines = wrap("a b c d e f g", 0.0, 11.0, FontWeightHint::Regular);
        assert_eq!(lines, vec!["a b c d e f g"]);
    }

    /// The case `wrap` is documented not to handle: a run with no space in it.
    /// `wrap` returns it as one over-long line; `wrap_hard` breaks it, and
    /// every piece fits.
    #[test]
    fn hard_wrapping_breaks_a_run_that_has_no_spaces() {
        let cjk = "日本語のファイル名がとても長い場合の折り返しです";
        let soft = wrap(cjk, 72.0, 11.0, FontWeightHint::Regular);
        assert_eq!(soft.len(), 1, "wrap has no break opportunity to use");
        assert!(
            measure(cjk, 11.0, FontWeightHint::Regular) > 72.0,
            "the sample must actually overflow or this test asserts nothing"
        );

        let hard = wrap_hard(cjk, 72.0, 11.0, FontWeightHint::Regular);
        assert!(hard.len() > 1, "{hard:?}");
        assert_eq!(hard.concat(), cjk, "breaking must not lose or add text");
        for line in &hard {
            let w = measure(line, 11.0, FontWeightHint::Regular);
            assert!(w <= 72.0, "line {line:?} measures {w} in a 72 px box");
        }
    }

    /// Text that already fits comes back untouched, so `wrap_hard` is a safe
    /// substitution for `wrap` rather than a different layout.
    #[test]
    fn hard_wrapping_leaves_fitting_text_alone() {
        let prose = "one two three four five six seven eight";
        assert_eq!(
            wrap_hard(prose, 200.0, 11.0, FontWeightHint::Regular),
            wrap(prose, 200.0, 11.0, FontWeightHint::Regular)
        );
    }

    /// A word longer than the box is broken, and — the property that makes
    /// this usable on remote text — broken from a single shaping pass, so a
    /// pathological input is a long list rather than a hang. Fifty thousand
    /// characters is what an article body from an unfriendly feed looks like.
    #[test]
    fn hard_wrapping_a_pathological_word_is_linear() {
        let word = "λ".repeat(50_000);
        let lines = wrap_hard(&word, 200.0, 11.0, FontWeightHint::Regular);
        assert!(lines.len() > 100, "{} lines", lines.len());
        assert_eq!(lines.concat(), word);
        for line in &lines {
            assert!(measure(line, 11.0, FontWeightHint::Regular) <= 200.0);
        }
    }

    /// A single glyph wider than the box has nowhere to go, so it takes a line
    /// and overflows it. The alternative — refusing to emit it — would be an
    /// unbounded loop in whatever called this.
    #[test]
    fn hard_wrapping_a_box_narrower_than_one_glyph_terminates() {
        let lines = wrap_hard("mmmm", 1.0, 11.0, FontWeightHint::Regular);
        assert_eq!(lines.len(), 4, "{lines:?}");
        assert_eq!(lines.concat(), "mmmm");
    }

    /// Counts pixels the canvas actually changed.
    fn touched(canvas: &Canvas) -> usize {
        canvas.pixels().iter().filter(|c| c.a != 0).count()
    }

    #[test]
    fn text_drawn_onto_a_canvas_marks_pixels_where_the_glyphs_are() {
        let mut canvas = Canvas::transparent(120, 40);
        let pen = draw_onto_canvas(
            &mut canvas,
            "Hello",
            10.0,
            28.0,
            16.0,
            FontWeightHint::Regular,
            Color::rgba(255, 0, 0, 255),
        );
        assert!(touched(&canvas) > 0, "the text left no mark at all");
        let expected = 10.0 + measure("Hello", 16.0, FontWeightHint::Regular);
        assert!(
            (pen - expected).abs() < 1.0,
            "the pen ended at {pen}, but the text measures to {expected}"
        );
    }

    #[test]
    fn a_glyph_edge_on_a_transparent_layer_stays_transparent() {
        // The whole reason this does not reuse `draw_into`: that target forces
        // every pixel it touches opaque, so anti-aliased edges on a transparent
        // paint layer would arrive as a black halo.
        let mut canvas = Canvas::transparent(120, 40);
        draw_onto_canvas(
            &mut canvas,
            "Hello",
            10.0,
            28.0,
            16.0,
            FontWeightHint::Regular,
            Color::rgba(255, 0, 0, 255),
        );
        let mut partial = 0;
        for pixel in canvas.pixels() {
            if pixel.a == 0 {
                continue;
            }
            // Every touched pixel is the colour asked for; only its alpha
            // varies with coverage. A halo would show up as a pixel that is
            // partly opaque *and* not red.
            assert_eq!(
                (pixel.r, pixel.g, pixel.b),
                (255, 0, 0),
                "a blended-against-nothing pixel crept in: {pixel:?}"
            );
            if pixel.a < 255 {
                partial += 1;
            }
        }
        assert!(
            partial > 0,
            "no partially-covered pixels at all — the text is not anti-aliased, \
             so this test would not catch a halo"
        );
    }

    #[test]
    fn drawing_off_the_canvas_edge_clips_rather_than_wraps() {
        // A negative anchor must lose the glyphs to the left of the canvas, not
        // wrap them onto the right-hand edge.
        let mut canvas = Canvas::transparent(60, 30);
        draw_onto_canvas(
            &mut canvas,
            "Wide text here",
            -40.0,
            20.0,
            16.0,
            FontWeightHint::Regular,
            Color::rgba(0, 0, 255, 255),
        );
        let right_edge_touched = (0..canvas.height())
            .filter_map(|y| canvas.get(canvas.width() - 1, y))
            .any(|c| c.a != 0);
        assert!(
            !right_edge_touched || touched(&canvas) > 0,
            "clipping is not wrapping"
        );

        // And an anchor entirely off-canvas leaves it untouched.
        let mut far = Canvas::transparent(60, 30);
        draw_onto_canvas(
            &mut far,
            "Hello",
            -10_000.0,
            20.0,
            16.0,
            FontWeightHint::Regular,
            Color::rgba(0, 0, 255, 255),
        );
        assert_eq!(touched(&far), 0, "text far off-canvas still painted");
    }

    #[test]
    fn a_transparent_colour_draws_nothing_and_costs_nothing() {
        let mut canvas = Canvas::transparent(60, 30);
        let pen = draw_onto_canvas(
            &mut canvas,
            "Hello",
            5.0,
            20.0,
            16.0,
            FontWeightHint::Regular,
            Color::rgba(255, 0, 0, 0),
        );
        assert_eq!(touched(&canvas), 0);
        assert!((pen - 5.0).abs() < f32::EPSILON, "the pen should not move");
    }

    #[test]
    fn a_non_finite_anchor_does_not_panic() {
        // `NaN` reaching an allocation is how a layout bug becomes a crash.
        let mut canvas = Canvas::transparent(40, 20);
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let _ = draw_onto_canvas(
                &mut canvas,
                "x",
                bad,
                10.0,
                16.0,
                FontWeightHint::Regular,
                Color::rgba(0, 0, 0, 255),
            );
            let _ = draw_onto_canvas(
                &mut canvas,
                "x",
                5.0,
                bad,
                16.0,
                FontWeightHint::Regular,
                Color::rgba(0, 0, 0, 255),
            );
            let _ = draw_onto_canvas(
                &mut canvas,
                "x",
                5.0,
                10.0,
                bad,
                FontWeightHint::Regular,
                Color::rgba(0, 0, 0, 255),
            );
        }
    }

    #[test]
    fn a_narrower_box_never_needs_fewer_lines() {
        let text = "the quick brown fox jumps over the lazy dog";
        let mut previous = usize::MAX;
        for max in [400.0, 200.0, 100.0, 50.0] {
            let n = wrap(text, max, 11.0, FontWeightHint::Regular).len();
            assert!(n >= 1);
            assert!(
                n >= previous || previous == usize::MAX,
                "{max}px needed {n} lines, but a wider box needed {previous}"
            );
            previous = n;
        }
    }

    const PROSE: &str = "The height a paragraph reserves has to come from the \
        lines it actually drew, because anything else is a second calculation \
        of the same quantity and the two will drift apart.";

    fn ink() -> Color {
        Color::rgb(205, 214, 244)
    }

    /// The `(y, text)` of every text command in `cmds`.
    fn drawn(cmds: &[RenderCommand]) -> Vec<(f32, String)> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::Text { y, text, .. } => Some((*y, text.clone())),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_paragraph_reserves_exactly_the_height_it_drew() {
        // The whole point of `draw` returning the height: a caller cannot end
        // up reserving room for a different number of lines than it emitted.
        let mut cmds = Vec::new();
        let p = Paragraph::new(PROSE, ink())
            .at(10.0, 20.0, 180.0)
            .font(13.0, FontWeightHint::Regular)
            .line_height(18.0);
        let used = p.draw(&mut cmds);

        let lines = drawn(&cmds);
        assert!(lines.len() > 1, "the prose was not wrapped");
        assert!((used - lines.len() as f32 * 18.0).abs() < 0.01);
        assert!(
            (used - p.height()).abs() < 0.01,
            "height() disagrees with draw()"
        );

        // Each line sits one spacing below the last, starting at the top.
        for (n, (y, _)) in lines.iter().enumerate() {
            assert!((y - (20.0 + n as f32 * 18.0)).abs() < 0.01);
        }
    }

    #[test]
    fn a_paragraph_loses_no_words() {
        let mut cmds = Vec::new();
        Paragraph::new(PROSE, ink())
            .at(0.0, 0.0, 150.0)
            .draw(&mut cmds);
        let joined = drawn(&cmds)
            .iter()
            .map(|(_, t)| t.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        for word in PROSE.split_whitespace() {
            assert!(joined.contains(word), "the paragraph lost {word:?}");
        }
    }

    #[test]
    fn every_drawn_line_fits_the_width_it_was_given() {
        let mut cmds = Vec::new();
        Paragraph::new(PROSE, ink())
            .at(0.0, 0.0, 160.0)
            .font(12.0, FontWeightHint::Regular)
            .draw(&mut cmds);
        for (_, line) in drawn(&cmds) {
            assert!(
                measure(&line, 12.0, FontWeightHint::Regular) <= 160.0 + 0.01,
                "{line:?} is wider than the 160px it was given"
            );
        }
    }

    #[test]
    fn a_capped_paragraph_marks_the_cut() {
        // A body cut without a mark reads as a complete sentence.
        let mut cmds = Vec::new();
        let used = Paragraph::new(PROSE, ink())
            .at(0.0, 0.0, 120.0)
            .line_height(16.0)
            .max_lines(2)
            .draw(&mut cmds);

        let lines = drawn(&cmds);
        assert_eq!(lines.len(), 2);
        assert!(
            (used - 32.0).abs() < 0.01,
            "a capped paragraph reserved {used}"
        );
        let last = &lines[1].1;
        assert!(last.ends_with('…'), "the cut was not marked: {last:?}");
        assert!(
            measure(last, Paragraph::DEFAULT_SIZE, FontWeightHint::Regular) <= 120.01,
            "the ellipsis pushed the last line out of its box: {last:?}"
        );
    }

    #[test]
    fn a_cap_longer_than_the_text_changes_nothing() {
        let uncapped = Paragraph::new(PROSE, ink()).at(0.0, 0.0, 200.0);
        let capped = uncapped.clone().max_lines(500);
        assert_eq!(uncapped.lines(), capped.lines());
    }

    #[test]
    fn an_empty_paragraph_draws_nothing_and_takes_no_room() {
        let mut cmds = Vec::new();
        let used = Paragraph::new("", ink())
            .at(0.0, 0.0, 200.0)
            .draw(&mut cmds);
        assert!(drawn(&cmds).is_empty());
        assert!(used.abs() < 0.01, "an empty paragraph reserved {used}");
    }

    #[test]
    fn light_measures_as_regular() {
        // It renders as regular, so it must measure as regular; the two
        // disagreeing is exactly the class of bug this module removes.
        assert_eq!(
            measure("Light", 16.0, FontWeightHint::Light),
            measure("Light", 16.0, FontWeightHint::Regular)
        );
    }
}
