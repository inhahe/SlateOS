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
use osfont::system::{Family, FontCache, Weight};

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

/// The character index in `text` nearest to `offset` pixels from its start.
///
/// This is what a click on a line of text means: the caret goes to the closest
/// gap between characters, not to the one the click landed inside, so clicking
/// the right half of a letter puts the caret after it.
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

#[cfg(test)]
mod tests {
    use super::*;

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
