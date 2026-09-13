//! The desktop's colour vocabulary: which roles exist, and what a box is.
//!
//! This lives in the toolkit rather than above it, and design-decisions 838
//! records why. A widget cannot reach `gui/appearance`, because `appearance`
//! depends on *this* crate; so for as long as the palette lived up there, the
//! shared widgets -- dialogs, menus, tab strips -- each kept a private table
//! of dark colours, and a light-theme user opening a folder in the file
//! manager got a dark modal on a light window.
//!
//! The alternative -- a second colour table down here -- was tried and
//! deleted: design-decisions 810 removed a `Theme` of 32 widget-role colours
//! whose hand-written table disagreed with its own comments. So the *type*
//! moved down instead, and `gui/appearance` keeps everything that makes a
//! palette out of a preference: the YAML file, the fourteen accents, the
//! high-contrast schemes, the transparency levels. It re-exports what is here,
//! so `use appearance::Palette;` is unchanged in 153 files.
//!
//! The seam between the two is [`PaletteSource`]: this crate says what it
//! needs to know about a settings object, and `appearance::AppearanceSettings`
//! answers. That is what lets `Palette::from_settings(&settings)` keep its
//! spelling at 458 call sites without this crate knowing what a setting is.

use crate::color::Color;
use crate::theme::{contrast_ratio, relative_luminance, with_alpha};

pub const BASE: Color = Color::from_hex(0x1E1E2E);
pub const MANTLE: Color = Color::from_hex(0x181825);
pub const CRUST: Color = Color::from_hex(0x11111B);
pub const SURFACE0: Color = Color::from_hex(0x313244);
pub const SURFACE1: Color = Color::from_hex(0x45475A);
pub const SURFACE2: Color = Color::from_hex(0x585B70);
pub const TEXT: Color = Color::from_hex(0xCDD6F4);
pub const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
pub const SUBTEXT1: Color = Color::from_hex(0xBAC2DE);

/// Dark mode's link colour. See [`LIGHT_LINK`].
pub const LINK: Color = BLUE;

/// Dark mode's border. The mirror of [`LIGHT_BORDER`]: "the strongest mark
/// available" is near-white here and black there, the same flip the whole
/// ladder makes.
pub const BORDER: Color = TEXT;
pub const BLUE: Color = Color::from_hex(0x89B4FA);
pub const GREEN: Color = Color::from_hex(0xA6E3A1);
pub const RED: Color = Color::from_hex(0xF38BA8);
pub const YELLOW: Color = Color::from_hex(0xF9E2AF);
pub const PEACH: Color = Color::from_hex(0xFAB387);
pub const LAVENDER: Color = Color::from_hex(0xB4BEFE);
pub const OVERLAY0: Color = Color::from_hex(0x6C7086);
pub const TEAL: Color = Color::from_hex(0x94E2D5);
pub const PINK: Color = Color::from_hex(0xF5C2E7);
pub const MAUVE: Color = Color::from_hex(0xCBA6F7);
pub const ROSEWATER: Color = Color::from_hex(0xF5E0DC);
pub const FLAMINGO: Color = Color::from_hex(0xF2CDCD);
pub const MAROON: Color = Color::from_hex(0xEBA0AC);
// `0x89DCEB`, not `0x89DCFE`. This carried a transposed byte pair from the day
// it was written, so `AccentColor::Sky.color()` returned a colour Catppuccin
// does not contain — and, being the crate that owns the answer, it had already
// propagated into `apps/alarmclock` and `apps/emojipicker`. Found by comparing
// every dark constant here against the published Mocha palette; it was the
// only mismatch, which is why a copy that agrees with 2,000 others is not
// evidence of anything. See known-issues.md
// TD-C-EVERY-APPLICATION-CARRIES-ITS-OWN-COPY-OF-THE-PALETTE-TOO.
pub const SKY: Color = Color::from_hex(0x89DCEB);
pub const SAPPHIRE: Color = Color::from_hex(0x74C7EC);

// ============================================================================
// Light-background accents — Catppuccin Latte hues, darkened to read as text
// ============================================================================
//
// An accent is not one colour, it is a *role*: "the hue this desktop is themed
// around". Mocha's accents are pastels tuned to sit on a near-black base, and
// reusing them on a near-white one is not a stylistic compromise but an
// unreadable result — Mocha blue `#89B4FA` on Latte base `#EFF1F5` is a
// contrast ratio of about 1.9:1, against the 4.5:1 that body text needs.
//
// Catppuccin's own Latte accents are the right hues but are still published
// for *decoration*, and measured against the Latte base most of them do not
// carry text either: yellow 2.31:1, pink 2.34:1, rosewater 2.34:1, sky 2.47:1,
// lavender 2.81:1 — only blue, mauve and red clear 4.5:1 unaided. The shell
// draws the accent as text (the start glyph, the start-menu heading), so each
// value below is its Latte hue scaled toward black by the smallest factor that
// reaches 4.6:1 on `#EFF1F5`. Scaling all three channels together holds the
// hue, so these still read as the colours Catppuccin named; blue, mauve and
// red are barely touched because they already passed.
//
// The dark palette needs no such treatment — every Mocha accent is already
// between 7:1 and 13:1 on the Mocha base.

/// The default accent, chosen by the operator on 2026-09-09 (C-Q10, §826).
///
/// `#1D62EC` before, which measured 4.63:1 on the page and **2.42:1 on the
/// greyest card** — the same page-only tuning as the greys above. This clears
/// the floor on every surface (9.03 on the page, 4.72 on `surface2`).
///
/// **The other thirteen accents have not been touched and all still fail on
/// every card**, between 2.33:1 and 3.52:1. They were each tuned to land just
/// over 4.5 on the page and nowhere else, so a user who picks any accent but
/// blue still gets unreadable accent text on a card. Logged as
/// `TD-C-THIRTEEN-LIGHT-ACCENTS-STILL-FAIL-ON-CARDS`; the fix wants one rule
/// applied to all fourteen rather than thirteen more hand-picked values, and
/// that is the operator's call.
pub const LIGHT_BLUE: Color = Color::from_hex(0x0036A3);
pub const LIGHT_LAVENDER: Color = Color::from_hex(0x5565BE);
pub const LIGHT_TEAL: Color = Color::from_hex(0x13787E);
pub const LIGHT_GREEN: Color = Color::from_hex(0x317B21);
pub const LIGHT_YELLOW: Color = Color::from_hex(0x976014);
pub const LIGHT_PEACH: Color = Color::from_hex(0xB94908);
pub const LIGHT_PINK: Color = Color::from_hex(0x9F508A);
pub const LIGHT_MAUVE: Color = Color::from_hex(0x8839EF);
pub const LIGHT_RED: Color = Color::from_hex(0xD20F39);
pub const LIGHT_ROSEWATER: Color = Color::from_hex(0x965E52);
pub const LIGHT_FLAMINGO: Color = Color::from_hex(0xA05757);
pub const LIGHT_MAROON: Color = Color::from_hex(0xC33B47);
pub const LIGHT_SKY: Color = Color::from_hex(0x0374A1);
pub const LIGHT_SAPPHIRE: Color = Color::from_hex(0x187788);

// ============================================================================
// Catppuccin Latte surface ladder — the light mode's backgrounds and text
// ============================================================================
//
// Role for role these are the counterparts of the Mocha constants above, and
// they are ordered the same way: `crust` is the layer *behind* the window,
// `base` is the window, `surface0`–`surface2` are things raised off it, and
// `overlay0`, `subtext0`, `subtext1`, `text` are marks drawn on it in
// increasing prominence. Only the direction reverses — in Mocha "raised" means
// lighter, in Latte it means darker — which is precisely why a renderer must
// name the role rather than the colour. Code that says `SURFACE1` keeps
// working when the mode flips; code that says `0x45475A` does not.
//
// These are Catppuccin's published Latte values with one exception, marked
// below. The values are also, individually, already in the tree: they are what
// `DecorationColors::for_mode(true)` and `DesktopTheme::light()` were each
// spelling out in hex. Naming them here is what lets those stop.

pub const LIGHT_CRUST: Color = Color::from_hex(0xDCE0E8);
pub const LIGHT_MANTLE: Color = Color::from_hex(0xE6E9EF);
pub const LIGHT_BASE: Color = Color::from_hex(0xEFF1F5);
pub const LIGHT_SURFACE0: Color = Color::from_hex(0xCCD0DA);
pub const LIGHT_SURFACE1: Color = Color::from_hex(0xBCC0CC);
pub const LIGHT_SURFACE2: Color = Color::from_hex(0xACB0BE);
pub const LIGHT_OVERLAY0: Color = Color::from_hex(0x9CA0B0);

// The light theme's three text inks, chosen by the operator on 2026-09-09 in
// answer to `open-questions.md` C-Q10. Recorded in `design-decisions.md` §826.
//
// The problem they solve: every ink here was picked against the *page* and
// never against a card, so all three failed the 4.5:1 floor the moment they
// were drawn on one — secondary text measured 2.42:1 on the greyest card, in
// roughly 850 places. The four options this crate's own notes offered all had
// a cost; the operator supplied values instead, and they are better than any
// of them, because they clear the floor on *every* surface in the theme while
// keeping the three inks distinguishable by weight.
//
// Measured here rather than asserted — `light_inks_clear_the_contrast_floor`
// checks every ink against every surface, so these numbers cannot rot:
//
//              page     surface0  surface1  surface2
//   text       18.57    13.60     11.55      9.71
//   subtext0   10.50     7.69      6.53      5.49
//   accent      9.03     6.61      5.62      4.72
//
// Separation between inks is what the earlier candidate fix would have
// destroyed: darkening the old greys far enough to clear the greyest card put
// all three at *identical* luminance (1.00), so body text, captions and links
// would have weighed the same and a link would have stopped looking like one
// to anyone reading by brightness. These keep 1.77 between text and subtext0.

/// The third grey, below [`LIGHT_SUBTEXT1`].
///
/// **Not one of the operator's three values, and derived under protest.** The
/// palette names two greys and this theme has three roles, so a value had to
/// be found for the third — and on a surface as dark as the cards, there is
/// almost nowhere for it to go. Walking this hue lighter one step at a time,
/// the *last* value that still clears 4.5:1 on `surface2` is `#3D3D3F`, one
/// step further fails at 4.42. So the third grey is 1.10 away from the second
/// — the same luminance to any eye.
///
/// That is a property of the surface, not of the choice: `surface2` gives only
/// 9.71:1 against pure black, so every ink clearing 4.5 on it is crowded into
/// the top of that range. A three-level grey hierarchy cannot exist on a card
/// that dark; what exists here is two levels and a formality. Raised with the
/// operator alongside the card question.
///
/// Was `#686B80`, which cleared the floor on the bare page (4.64) and nothing
/// else — measured once, against the one surface it happened to be tried on.
/// Secondary text. Cerulean since 2026-09-11 (§829): in the bordered theme the
/// same value marks the selected outline and a switch that is on, so secondary
/// text, selection and "on" read as one family.
///
/// Holds the same value as [`LIGHT_SUBTEXT1`] and is deliberately still a role
/// of its own -- see §831. 1,087 call sites name this one and 161 name the
/// other; splitting them later is a one-line change here, and would be a
/// thousand-site audit if the roles had been merged.
pub const LIGHT_SUBTEXT0: Color = Color::from_hex(0x00688B);

/// Secondary text: the second line of a list row, a caption, a hint.
///
/// The operator's "Secondary text". This role, not [`LIGHT_SUBTEXT0`], is the
/// one this crate's own struct documents with those words — a distinction the
/// first draft of this change got backwards, and which the surface-ladder
/// invariant caught.
///
/// Was `#5C5F77`: 5.53:1 on the page, **2.89:1** on the greyest card, drawn in
/// 58 places and never measured against anything but the page.
/// Secondary text, the more prominent rung. Same value as [`LIGHT_SUBTEXT0`];
/// see §831 for why both names survive that.
pub const LIGHT_SUBTEXT1: Color = Color::from_hex(0x00688B);

/// Main text.
pub const LIGHT_TEXT: Color = Color::from_hex(0x000000);

/// Anything you can click through to. Its own role rather than the accent
/// reused (§832): the accent marks *selection and state*, a link marks
/// *navigation*, and once the accent became cerulean those stopped being one
/// colour. Never the only mark on a link -- WCAG 1.4.1 forbids colour as the
/// sole indicator, so a link is underlined as well.
pub const LIGHT_LINK: Color = LIGHT_BLUE;

/// The outline that carries structure in the bordered theme (§829), where a box
/// is told apart by its edge rather than by being filled.
///
/// Black, at the operator's instruction. A border needs 3:1 against its
/// background to be a perceivable UI component, not the 4.5 text needs --
/// nothing is drawn *on* a border -- so this is far above what it must clear.
pub const LIGHT_BORDER: Color = LIGHT_TEXT;

/// The pale end of the two answers [`readable_on`] can give.
///
/// Equal to [`LIGHT_BASE`] and to nothing else on purpose — it is a separate
/// constant because it means a different thing. `LIGHT_BASE` is the Latte
/// palette's page; this is "as pale as this desktop ever goes", the value you
/// want when the background is not a palette surface at all. If Latte's base
/// were ever retuned, this must not follow it.
pub const LIGHT_EXTREME: Color = Color::from_hex(0xEFF1F5);

/// The dark end of the two answers [`readable_on`] can give.
///
/// Shares its value with Mocha [`CRUST`], and that coincidence has a cost
/// worth knowing about: the shell's conversion sweep must allow this value in
/// a *light* render, which means it cannot tell a deliberate dark extreme from
/// a leftover `CRUST` constant. See `gui/desktop/src/palette_check.rs`.
pub const DARK_EXTREME: Color = Color::from_hex(0x11111B);

/// Black-ish or white-ish, whichever can be read on `bg`.
///
/// The endpoints are the palettes' own extremes rather than pure `#000`/`#fff`
/// so that accented surfaces still look like part of this desktop.
///
/// **The choice is measured, not estimated.** This asks
/// [`contrast_ratio`] which of the two extremes is further from `bg` and
/// returns that one, so the answer is the better one *by the same metric the
/// accessibility floors are stated in* — there is no second definition of
/// "bright" that could disagree with the ratio a test then measures.
///
/// That is worth stating because the obvious cheaper answer is wrong. This
/// used to threshold a luma sum at 140, and a luma sum is not a monotone
/// function of contrast: `#00EE02`, a perfectly legal custom accent, has a
/// luma of 139.9 and so was called dark and inked pale, at **1.40:1**. The
/// contrast comparison has no such corner — the worst background in the
/// entire 24-bit cube is `#B82EE5`, and even there the returned ink reaches
/// **4.07:1**, which is the floor this function guarantees for *any* colour a
/// user can choose. A threshold cannot make that promise; it is exactly as
/// good as the values that happen to be in the palette on the day it is tuned.
///
/// Deliberately not [`crate::theme::contrast_text`], which answers the same
/// question with pure black and pure white. That is the right answer for a
/// widget that may be drawn on any background; this is the right answer for a
/// surface that belongs to a specific palette. The two now share their
/// arithmetic — [`contrast_ratio`] *is* the toolkit's — so the only thing that
/// differs between them is the pair of inks they choose between.
#[must_use]
pub fn readable_on(bg: Color) -> Color {
    // `>=` rather than `>`: at the exact crossover both inks are equally
    // legible, and preferring the dark one keeps the answer stable for the
    // palettes' own surfaces, which are overwhelmingly pale.
    if contrast_ratio(bg, DARK_EXTREME) >= contrast_ratio(bg, LIGHT_EXTREME) {
        DARK_EXTREME
    } else {
        LIGHT_EXTREME
    }
}

/// A visibly different shade of `color`, for the pressed state of a control
/// whose resting state is already `color`.
///
/// Moves away from whichever extreme `color` is nearer, so the emphasis is
/// visible on both a pale and a deep accent instead of vanishing at one end.
#[must_use]
pub fn emphasized(color: Color) -> Color {
    let toward = readable_on(color);
    color.lerp(toward, 0.25)
}

/// The contrast a body-text ink owes its background: WCAG SC 1.4.3, level AA.
pub const TEXT_CONTRAST_FLOOR: f32 = 4.5;

/// `ink`, moved away from `bg` only as far as the contrast floor requires.
///
/// Returns `ink` unchanged when it already clears the floor, so it is a no-op
/// for the great majority of pairs and cannot drift a palette that is already
/// correct.
///
/// # Why a ratio rather than a fixed table of darker accents
///
/// The user may choose any accent, including one this project has never seen.
/// A table can only be as good as the values in it on the day it was written;
/// a ratio is a promise about every colour in the cube. The same reasoning
/// [`readable_on`] gives for preferring a comparison to a luminance threshold.
///
/// # Why toward black or white rather than toward `bg`'s opposite
///
/// Scaling toward an extreme preserves hue: `#317B21` (green) darkened to
/// clear 4.5 on a card is `#245A18`, still plainly green, and still plainly
/// not `#0036A3`. Moving *away from `bg` in RGB* would drag hues around and
/// two accents could converge. Measured over the fourteen: the closest pair
/// after adjustment is teal/sapphire, which are near-duplicates in the source
/// palette to begin with.
#[must_use]
pub fn legible_on(ink: Color, bg: Color) -> Color {
    if contrast_ratio(ink, bg) >= TEXT_CONTRAST_FLOOR {
        return ink;
    }
    // Toward whichever pole is legible *on this ground* -- not "away from the
    // ground", which is what this said first and which is wrong whenever the
    // ink is on the same side of the crossover as the ground. A pale yellow on
    // the pale page has to become dark; retreating further into pale takes it
    // from 1.13:1 to 1.00:1. A user can choose `#FFFFE0`, so this is not a
    // corner case, it is a corner case someone will hit.
    //
    // True black and true white, not [`DARK_EXTREME`] and [`LIGHT_EXTREME`].
    // Those two are the *palette's* bounds -- `#11111B` and `#EFF1F5` -- and
    // between them they leave a gap: a background whose relative luminance
    // falls in roughly 0.152..0.208 clears 4.5 against neither, so the
    // bisection would converge on an extreme that still fails and this
    // function would quietly return something under the floor. Pure black and
    // pure white have no such gap -- black clears 4.5 for every background
    // above 0.175 and white for every one below 0.183, and those two ranges
    // overlap -- so one of them always works. (Pure black is not foreign to
    // the palette either: `LIGHT_TEXT` is exactly it.)
    let (black, white) = (Color::rgb(0, 0, 0), Color::rgb(255, 255, 255));
    let toward = if contrast_ratio(bg, black) >= contrast_ratio(bg, white) {
        black
    } else {
        white
    };
    // Bisection on the mix, 24 rounds -- finer than the 8 bits a channel has,
    // so the answer is the nearest representable colour that clears the floor
    // rather than an arbitrary one past it.
    let (mut lo, mut hi) = (0.0_f32, 1.0_f32);
    for _ in 0..24 {
        let mid = f32::midpoint(lo, hi);
        if contrast_ratio(ink.lerp(toward, mid), bg) >= TEXT_CONTRAST_FLOOR {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    ink.lerp(toward, hi)
}

// ============================================================================
// The resolved palette
// ============================================================================

/// Every colour the shell paints with, resolved for one set of choices.
///
/// **What this is for.** The desktop shell had 549 `const … : Color` of its
/// own, spread over 49 modules, and every one of them was a Catppuccin Mocha
/// value written out by hand. `TEXT` was declared 31 separate times;
/// `0x89B4FA` appeared 47 times under four different names. None of them were
/// *wrong* — they all agreed with what the dark theme happens to be — and that
/// is exactly the problem: they agreed by coincidence, so the user's light/dark
/// choice, accent and transparency level reached none of them. See
/// known-issues.md `TD-C-FORTY-NINE-SHELL-MODULES-CARRY-THEIR-OWN-COPY-OF-THE-PALETTE`.
///
/// **Why a resolved struct rather than a lookup.** By the time a colour is in
/// here the mode, the accent and the transparency level have all been folded
/// in, so a render function does nothing but read a field. A renderer handed
/// `AppearanceSettings` instead would re-derive the same colour at every
/// frame and would be free to derive it slightly differently in each of the
/// dozens of places it is drawn — which is the duplication above, relocated
/// rather than removed. This is the same argument `DecorationColors` and
/// `DesktopTheme` already make; this type is the one they are both built from.
///
/// **Roles, not colours.** The fields are named for what a colour *does* in a
/// layout, not for what it looks like. The surface ladder runs
/// [`crust`](Self::crust) (behind the window) → [`base`](Self::base) (the
/// window) → [`surface0`](Self::surface0)…[`surface2`](Self::surface2) (things
/// raised off it) → [`overlay0`](Self::overlay0),
/// [`subtext0`](Self::subtext0), [`subtext1`](Self::subtext1),
/// [`text`](Self::text) (marks on it, in increasing prominence). In dark mode
/// "raised" means lighter and in light mode it means darker, so a caller that
/// names the role keeps working across the flip and a caller that names a hex
/// value does not.
///
/// **Why the named hues are still here, next to `accent`.** The shell uses
/// `BLUE` for two unrelated jobs: as "the colour this desktop is themed
/// around" — selection, focus, the start glyph — and as "the blue one" in a
/// fixed set of category colours, next to green and peach in a resource graph
/// legend. Collapsing both onto [`accent`](Self::accent) would give a user who
/// picks Red a graph whose CPU line and temperature line are the same colour,
/// which is not a theme, it is a lost distinction. So the two stay separate:
/// [`accent`](Self::accent) follows the setting, and the named hues are a
/// fixed categorical set that merely changes value between modes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    /// Behind the window — the desktop, and the recessed well of a text input.
    pub crust: Color,
    /// One step behind [`base`](Self::base) — a sidebar beside a content pane.
    pub mantle: Color,
    /// The window itself: the default background of any surface the shell owns.
    pub base: Color,
    /// Raised one step off [`base`](Self::base) — a card, a header row.
    pub surface0: Color,
    /// Raised two steps — a button at rest, a selected row.
    pub surface1: Color,
    /// Raised three steps — a hovered button, a scrollbar thumb.
    pub surface2: Color,
    /// The faintest legible mark: separators, disabled text, placeholder text.
    ///
    /// Deliberately *not* required to carry body text — it measures about
    /// 3.4:1 on [`base`](Self::base) in dark mode and 2.3:1 in light. Anything
    /// a user has to read is [`subtext0`](Self::subtext0) or brighter.
    pub overlay0: Color,
    /// Secondary text: the second line of a list row, a caption, a hint.
    pub subtext0: Color,
    /// Text that is secondary but load-bearing — a column heading.
    pub subtext1: Color,
    /// Primary text.
    pub text: Color,
    /// Anything you can click through to. Always drawn underlined as well,
    /// because colour alone is not a sufficient mark for a link.
    pub link: Color,
    /// The outline that carries structure under [`SurfaceStyle::Borders`].
    pub border: Color,
    /// Whether boxes are outlined or filled.
    ///
    /// Carried here rather than passed alongside, and that is the whole reason
    /// the conversion is tractable: a `Palette` is already threaded to every
    /// draw site in the tree, so a site needs no new argument to honour the
    /// user's choice. `panel_alpha` and `light` are here for the same reason --
    /// settings-derived values every drawer needs.
    ///
    /// Private, with [`set_surface_style`](Self::set_surface_style), because
    /// changing it changes which surfaces carry text and therefore what the
    /// text-only inks have to be. A plain `pub` field let a caller -- in
    /// practice a test -- switch the theme and leave `subtext0` at the value
    /// the *previous* theme needed, which reads at 4.10:1 on a dark card. An
    /// invariant a field assignment can break is not an invariant.
    surface_style: SurfaceStyle,
    /// Whether a toolbar or status bar is a band or a hairline.
    ///
    /// Carried on the palette for the reason `surface_style` is: a palette
    /// already reaches every draw site, so a site needs no new argument.
    ///
    /// Private for the same reason as `surface_style` above: a filled strip is
    /// a `mantle` band with labels on it, and a separated one is not, so which
    /// grounds exist depends on this.
    strip_style: StripStyle,
    /// The blue of the categorical set. See the type's note on hues.
    pub blue: Color,
    /// Green — also "this succeeded", "this is allowed", "this is safe".
    pub green: Color,
    /// Red — also "this failed", "this is denied", "this is dangerous".
    pub red: Color,
    /// Yellow — also "this needs attention".
    pub yellow: Color,
    /// Peach — also the step between [`yellow`](Self::yellow) and
    /// [`red`](Self::red) on a severity scale.
    pub peach: Color,
    /// Lavender.
    pub lavender: Color,
    /// Mauve.
    pub mauve: Color,
    /// Sapphire.
    pub sapphire: Color,
    /// Teal.
    ///
    /// Here because the applications need it, not because the shell does — 86
    /// `const TEAL` declarations across `apps/`, against 0 in `gui/desktop`.
    /// Added while the light ladder was being written rather than when
    /// `apps/` is converted, because a hue added later has to be re-checked
    /// against every mode-flip and legibility sweep in this module, and a
    /// sweep that silently skips a field is the failure those sweeps exist to
    /// catch. See known-issues.md
    /// `TD-C-EVERY-APPLICATION-CARRIES-ITS-OWN-COPY-OF-THE-PALETTE-TOO`.
    pub teal: Color,
    /// Sky. Present for the same reason as [`teal`](Self::teal).
    pub sky: Color,
    /// Pink, rosewater, flamingo and maroon: the four hues the accent list
    /// offers that this struct did not.
    ///
    /// Added 2026-09-13, and the reason is a pattern rather than a request.
    /// Five applications converted under 822 -- calendar, emojipicker,
    /// unitconverter, diagram, alarmclock -- each kept exactly these
    /// constants and no others, because there was no rung to map them to;
    /// `procexplorer` and `pdfviewer` had to map theirs away or delete them.
    /// A palette that cannot name a colour the user is allowed to *choose as
    /// their accent* is incomplete, and the incompleteness showed up as a
    /// residue in every app that wanted one.
    pub pink: Color,
    /// See [`pink`](Self::pink).
    pub rosewater: Color,
    /// See [`pink`](Self::pink).
    pub flamingo: Color,
    /// See [`pink`](Self::pink).
    pub maroon: Color,
    /// The colour this desktop is themed around, as the user chose it.
    ///
    /// Already resolved for the mode and for a custom colour — this is
    /// `AppearanceSettings::effective_accent`, not the enum.
    pub accent: Color,
    /// How opaque a floating surface is: `TransparencyLevel::panel_alpha`.
    ///
    /// Carried rather than applied to every field because most surfaces are
    /// *not* floating. A list row inside a panel must stay opaque no matter
    /// what the panel behind it does, or the desktop shows through the row and
    /// not through its own container.
    pub panel_alpha: u8,
    /// Whether this is the light palette.
    ///
    /// Present so a caller with a genuinely mode-dependent decision — an icon
    /// with a light and a dark artwork, say — can ask, instead of guessing
    /// from the luma of a field it happens to have.
    pub light: bool,
}

/// Fixed alphas for the washes derived from [`Palette::accent`].
///
/// One place, because the shell had six: a selection at 50, a marquee at 30, a
/// snap zone at 50 and its highlight at 90, and two borders at 150 and 160 that
/// meant the same thing and differed by a rounding nobody chose. Alpha is what
/// distinguishes these from one another, so if it is written per module the
/// modules are the definition and drift is silent.
mod wash {
    /// A hint the pointer is currently drawing — a rubber-band marquee.
    pub const HINT: u8 = 30;
    /// A committed selection, or a snap zone at rest.
    pub const FILL: u8 = 50;
    /// The one zone or item the pointer is over.
    pub const HIGHLIGHT: u8 = 90;
    /// The outline of a hint.
    pub const HINT_EDGE: u8 = 120;
    /// The outline of a selection or a highlighted zone.
    pub const EDGE: u8 = 150;
}

impl Palette {
    /// The palette for a mode, before any of the user's other choices apply.
    ///
    /// [`accent`](Self::accent) is the mode's blue, which is what the accent
    /// setting defaults to; [`panel_alpha`](Self::panel_alpha) is opaque.
    /// Callers that have an `AppearanceSettings` should use
    /// [`from_settings`](Self::from_settings) instead — this exists for the
    /// two places that legitimately have only a mode: a test asserting a
    /// property of one palette, and a preview swatch.
    #[must_use]
    pub fn for_mode(light: bool) -> Self {
        let mut chosen = if light {
            Self {
                crust: LIGHT_CRUST,
                mantle: LIGHT_MANTLE,
                base: LIGHT_BASE,
                surface0: LIGHT_SURFACE0,
                surface1: LIGHT_SURFACE1,
                surface2: LIGHT_SURFACE2,
                overlay0: LIGHT_OVERLAY0,
                subtext0: LIGHT_SUBTEXT0,
                subtext1: LIGHT_SUBTEXT1,
                text: LIGHT_TEXT,
                link: LIGHT_LINK,
                border: LIGHT_BORDER,
                surface_style: SurfaceStyle::Borders,
                strip_style: StripStyle::Filled,
                blue: LIGHT_BLUE,
                green: LIGHT_GREEN,
                red: LIGHT_RED,
                yellow: LIGHT_YELLOW,
                peach: LIGHT_PEACH,
                lavender: LIGHT_LAVENDER,
                mauve: LIGHT_MAUVE,
                sapphire: LIGHT_SAPPHIRE,
                teal: LIGHT_TEAL,
                sky: LIGHT_SKY,
                pink: LIGHT_PINK,
                rosewater: LIGHT_ROSEWATER,
                flamingo: LIGHT_FLAMINGO,
                maroon: LIGHT_MAROON,
                accent: LIGHT_BLUE,
                panel_alpha: 255,
                light: true,
            }
        } else {
            Self {
                crust: CRUST,
                mantle: MANTLE,
                base: BASE,
                surface0: SURFACE0,
                surface1: SURFACE1,
                surface2: SURFACE2,
                overlay0: OVERLAY0,
                subtext0: SUBTEXT0,
                subtext1: SUBTEXT1,
                text: TEXT,
                link: LINK,
                border: BORDER,
                surface_style: SurfaceStyle::Borders,
                strip_style: StripStyle::Filled,
                blue: BLUE,
                green: GREEN,
                red: RED,
                yellow: YELLOW,
                peach: PEACH,
                lavender: LAVENDER,
                mauve: MAUVE,
                sapphire: SAPPHIRE,
                teal: TEAL,
                sky: SKY,
                pink: PINK,
                rosewater: ROSEWATER,
                flamingo: FLAMINGO,
                maroon: MAROON,
                accent: BLUE,
                panel_alpha: 255,
                light: false,
            }
        };
        chosen.apply_text_floor();
        chosen
    }

    /// `color`, made legible wherever this theme could put text in it.
    ///
    /// The operator's requirement, given on 2026-09-11 alongside the choice of
    /// the bordered theme: *"we still have to figure out how to color them so
    /// that contrast is always >=4.50."*
    ///
    /// # Why the surfaces are computed rather than listed
    ///
    /// Which surfaces carry text is a consequence of the two style settings,
    /// not a fixed fact. Under [`SurfaceStyle::Borders`] a card is an outline,
    /// so nothing is drawn on `surface0` at all and the page is very nearly
    /// the only ground there is; under [`SurfaceStyle::Cards`] a selected row
    /// is a `surface1` fill with a label on it. A hard-coded list would be
    /// wrong for one of the two, and silently.
    ///
    /// # Why it is applied here and not at each draw site
    ///
    /// 315 sites draw text in an accent or a named colour. Adjusting at the
    /// site means 315 chances to forget, and a forgotten one is invisible --
    /// it renders, it just cannot be read. Adjusting once, where the palette
    /// is built, is the same argument [`surface_paint`](Self::surface_paint)
    /// makes about boxes.
    ///
    /// The cost, stated plainly: the accent a user picked renders slightly
    /// deeper than the swatch they picked it from. Under the shipped theme
    /// that is at most 6.6 units of RGB distance and invisible; under the
    /// optional card theme it is real and deliberate, because the alternative
    /// is accent text at 2.9:1 on a card, which is the defect this removes.
    ///
    /// `overlay0` is deliberately *not* adjusted. It is the muted ink -- the
    /// placeholder in an empty field, the label of a disabled control -- and
    /// WCAG 1.4.3 exempts inactive controls. Making it legible would make a
    /// disabled control look enabled, which is a worse failure than a faint
    /// one.
    #[must_use]
    pub fn ink(&self, color: Color) -> Color {
        let grounds = self.text_grounds();
        let mut out = color;
        for ground in grounds.iter().flatten() {
            out = legible_on(out, *ground);
        }
        out
    }

    /// Whether boxes are outlined or filled.
    #[must_use]
    pub const fn surface_style(&self) -> SurfaceStyle {
        self.surface_style
    }

    /// Whether a toolbar or status bar is a band or a hairline.
    #[must_use]
    pub const fn strip_style(&self) -> StripStyle {
        self.strip_style
    }

    /// Choose how boxes are drawn, re-resolving the inks that depend on it.
    pub fn set_surface_style(&mut self, style: SurfaceStyle) {
        self.surface_style = style;
        self.apply_text_floor();
    }

    /// Choose how strips are drawn, re-resolving the inks that depend on it.
    pub fn set_strip_style(&mut self, style: StripStyle) {
        self.strip_style = style;
        self.apply_text_floor();
    }

    /// Raise the text-only inks to the floor for the theme now set.
    ///
    /// # Why these three and not the rest
    ///
    /// `subtext0`, `subtext1` and `link` exist *to be read*. They are not
    /// fills, they are not badges, and the user does not choose them -- so
    /// resolving them against the floor breaks no promise and costs no draw
    /// site a change. That is 546 of the 861 sites that put an ink on screen.
    ///
    /// The remaining roles are dual-use: an accent is also a switch that is
    /// on, `red` is also an error bar, `green` is also a progress fill. Moving
    /// those in the palette would silently restyle things that are not text,
    /// so they stay as chosen and a site that draws *text* in one asks
    /// [`ink`](Self::ink) for it.
    ///
    /// (`link`'s one non-text use is its own underline -- the rule under it,
    /// which must be the same colour as the text above it. Moving them
    /// together is the point, not an exception.)
    ///
    /// # Why it starts from the constants
    ///
    /// So that it is idempotent under a change of theme. Resolving from the
    /// current field would compound: a palette settled for filled strips and
    /// then switched to separated ones would keep the deeper ink it no longer
    /// needs, and a palette re-resolved every frame would drift.
    fn apply_text_floor(&mut self) {
        let (sub0, sub1, link) = if self.light {
            (LIGHT_SUBTEXT0, LIGHT_SUBTEXT1, LIGHT_LINK)
        } else {
            (SUBTEXT0, SUBTEXT1, LINK)
        };
        self.subtext0 = self.ink(sub0);
        self.subtext1 = self.ink(sub1);
        self.link = self.ink(link);
    }

    /// As [`ink`](Self::ink), for a caller that knows which ground its text
    /// lands on.
    ///
    /// Preferred where it is known, because [`ink`](Self::ink) has to assume
    /// the worst ground the theme can produce and so darkens a label on the
    /// page further than that label needs. Identical under the bordered theme,
    /// where there is essentially one ground.
    #[must_use]
    pub fn ink_on(&self, color: Color, ground: Color) -> Color {
        legible_on(color, ground)
    }

    /// The surfaces this theme actually draws text on.
    ///
    /// A fixed array of slots rather than a `Vec`, so that resolving a
    /// palette allocates nothing. Callers iterate with `.flatten()`.
    ///
    /// Public since 838, when the palette moved down here and the test that
    /// measures every ink against every ground stayed in `gui/appearance`.
    /// It is the right thing to expose anyway: anyone checking a colour for
    /// legibility needs the same list this uses, and a second hand-written
    /// one would be a second list to keep in step. Which grounds exist
    /// depends on [`SurfaceStyle`], so it cannot be a constant.
    #[must_use]
    pub fn text_grounds(&self) -> [Option<Color>; 5] {
        let strips = (self.strip_style == StripStyle::Filled).then_some(self.mantle);
        let cards = self.surface_style == SurfaceStyle::Cards;
        [
            // The page, always -- it is under everything.
            Some(self.base),
            // A toolbar is a band of `mantle` with labels on it. Also a panel
            // under the card theme, which is why either condition supplies it.
            strips.or_else(|| cards.then_some(self.mantle)),
            // A card and a selected row. `surface2` is not here: it is the
            // control track, and a groove has a control drawn over it rather
            // than a label on it.
            cards.then_some(self.surface0),
            cards.then_some(self.surface1),
            // A sidebar.
            cards.then_some(self.crust),
        ]
    }

    /// Resolve the whole palette from what the user chose.
    #[must_use]
    pub fn from_settings<S: PaletteSource>(settings: &S) -> Self {
        // Before the mode, not after: a high-contrast palette replaces the
        // theme rather than adjusting it, so there is nothing from the
        // light/dark branch to keep. Transparency is dropped with it -- see
        // `high_contrast`.
        if let Some((bg, fg)) = settings.high_contrast() {
            let mut palette = Self::high_contrast(bg, fg, settings.accent_on(bg));
            // The surface style survives high contrast, where the colours do
            // not. It is a layout decision -- whether a box has an outline or a
            // fill -- and a high-contrast scheme has an opinion about colour,
            // not about that. Someone who chose outlined boxes and then turned
            // on high contrast has not asked for filled ones.
            //
            // Assigned rather than set, because the setters re-derive the
            // text-only inks from the *mode's* constants -- and a high-contrast
            // palette's inks are the scheme's, at 7:1 or better by
            // construction. Re-deriving them put `subtext1` back to a value
            // that reads at 6.26:1, which
            // `every_text_role_clears_seven_to_one_on_every_surface` caught
            // within the minute.
            palette.surface_style = settings.surface_style();
            palette.strip_style = settings.strip_style();
            return palette;
        }
        let mut palette = Self::for_mode(settings.is_light());
        palette.accent = settings.accent();
        palette.panel_alpha = settings.panel_alpha();
        // The setters re-resolve the text-only inks, which is the whole
        // reason they are setters: which grounds exist depends on the styles.
        palette.set_surface_style(settings.surface_style());
        palette.set_strip_style(settings.strip_style());
        palette
    }

    /// The palette for a high-contrast scheme.
    ///
    /// # What it does to the neutrals, and why
    ///
    /// Every background role -- `crust`, `mantle`, `base`, `surface0..2` --
    /// becomes the scheme's background, and every text role becomes its text
    /// colour. That is not laziness; it is the feature. An ordinary palette
    /// ranks surfaces by *raising* them a shade and text by *fading* it, and
    /// both gradients are invisible to a user who needs this mode. Structure
    /// is carried by borders instead, which is how every high-contrast
    /// implementation works.
    ///
    /// `overlay0` matters most and is the clearest case. It is documented as
    /// "the faintest legible mark", measuring about 3.4:1 in dark mode -- a
    /// role defined by being hard to see, which is exactly what this mode
    /// exists to abolish. It becomes the text colour.
    ///
    /// # What it does not flatten
    ///
    /// The categorical hues -- `red`, `green`, `yellow` and the rest -- keep
    /// their meanings. "Red means this failed" is information, not decoration,
    /// and collapsing it into the text colour would delete it. They are taken
    /// from whichever ordinary mode suits the scheme's background, so they
    /// stay bright on a dark scheme and dark on a light one.
    ///
    /// # The accent
    ///
    /// Follows the user's Appearance setting, because `design-decisions.md`
    /// §816 requires the highlight to be configurable and a scheme-fixed
    /// accent would make it the one colour this mode does not let you change.
    ///
    /// For a *named* accent the hue is kept and the better-contrasting of its
    /// two values is used. That is not an override: both values are the same
    /// hue, and choosing between them by background is what `for_mode` already
    /// does for every other role. A `Custom` accent is used exactly as given,
    /// because there is no second value to choose and an exact colour is an
    /// exact request.
    #[must_use]
    pub fn high_contrast(bg: Color, fg: Color, accent: Color) -> Self {
        // Takes the scheme's two colours rather than the scheme, and the
        // already-chosen accent rather than the settings: which schemes exist
        // and how an accent is picked for one are preferences, and this crate
        // does not model preferences (838). `PaletteSource::accent_on` is the
        // half that moved out.
        let light = relative_luminance(bg) > 0.5;

        Self {
            crust: bg,
            mantle: bg,
            base: bg,
            surface0: bg,
            surface1: bg,
            surface2: bg,
            overlay0: fg,
            subtext0: fg,
            subtext1: fg,
            text: fg,
            accent,
            // Transparency blends a surface with whatever is behind it, which
            // lowers contrast by construction. A mode whose entire purpose is
            // contrast does not get to be see-through.
            panel_alpha: 255,
            light,
            // The categorical hues, from the mode that suits this background.
            ..Self::for_mode(light)
        }
    }

    /// Every field of this palette, paired with its name.
    ///
    /// Public rather than test-only because two different sweeps need it and
    /// the alternative is two hand-written lists that must be kept in step —
    /// which is the shape of the bug this whole crate exists to remove. The
    /// second caller is the shell's conversion sweep, which asserts that a
    /// module's render output is drawn from the palette it was handed and
    /// nothing else; a colour constant left behind in a converted module is a
    /// Mocha value, so it is absent from the *light* palette's roles and the
    /// sweep names it. See known-issues.md
    /// `TD-C-FORTY-NINE-SHELL-MODULES-CARRY-THEIR-OWN-COPY-OF-THE-PALETTE`.
    ///
    /// Written out by hand, and deliberately so: the point of the sweeps that
    /// consume this is that a field added later is *not* silently skipped, and
    /// a macro or a reflection trick would skip it for exactly the same reason
    /// the renderer would.
    ///
    /// What holds it to that is the destructure in the body, **not** the array
    /// length in the signature. This comment used to claim the length did the
    /// job, and it does not: the length only catches the reverse mistake, an
    /// entry added to the array without the count being changed. Adding a
    /// twenty-second `Color` to [`Palette`] leaves this a perfectly valid array
    /// of twenty-one — checked, not assumed, and the only two errors are
    /// `E0063` at the struct literals in [`for_mode`](Self::for_mode). Fix
    /// those, and the compiler falls silent with the new colour absent from
    /// every sweep that reads this. A guarantee that is documented but not real
    /// is worse than none, because it is the reason nobody looks.
    #[must_use]
    pub fn roles(&self) -> [(&'static str, Color); 27] {
        // A struct pattern with no `..` is exhaustive, so this stops compiling
        // the moment `Palette` grows a field: a new colour cannot reach the
        // palette without someone deciding, right here, whether it is a role.
        // The two non-colours are named and discarded rather than swept up by
        // `..`, so that decision is on the record as well -- and so that a
        // future non-colour field does not slip in behind them.
        let Self {
            crust,
            mantle,
            base,
            surface0,
            surface1,
            surface2,
            overlay0,
            subtext0,
            subtext1,
            text,
            link,
            border,
            red,
            green,
            yellow,
            peach,
            blue,
            lavender,
            mauve,
            sapphire,
            teal,
            sky,
            pink,
            rosewater,
            flamingo,
            maroon,
            accent,
            panel_alpha: _,
            light: _,
            // Not a colour, so not a role. Named and discarded rather than
            // swept up by `..`, on the same terms as the two above it.
            surface_style: _,
            // Not a colour either. Same terms as the three above it.
            strip_style: _,
        } = *self;
        [
            ("crust", crust),
            ("mantle", mantle),
            ("base", base),
            ("surface0", surface0),
            ("surface1", surface1),
            ("surface2", surface2),
            ("overlay0", overlay0),
            ("subtext0", subtext0),
            ("subtext1", subtext1),
            ("text", text),
            ("link", link),
            // `border` is a role and belongs in this list, but note for anyone
            // sweeping it: it is the one entry here that is never drawn *on*.
            // A border is a UI component, so its bar is 3:1 against its
            // background, not the 4.5 every other entry has to clear. A sweep
            // that applies the text floor uniformly will report it as failing
            // when it is not.
            ("border", border),
            ("red", red),
            ("green", green),
            ("yellow", yellow),
            ("peach", peach),
            ("blue", blue),
            ("lavender", lavender),
            ("mauve", mauve),
            ("sapphire", sapphire),
            ("teal", teal),
            ("sky", sky),
            ("pink", pink),
            ("rosewater", rosewater),
            ("flamingo", flamingo),
            ("maroon", maroon),
            ("accent", accent),
        ]
    }

    /// Text that can be read on [`accent`](Self::accent).
    ///
    /// The accent is the one colour in the palette whose brightness the user
    /// controls, so nothing drawn on it can have a fixed foreground.
    #[must_use]
    pub fn on_accent(&self) -> Color {
        readable_on(self.accent)
    }

    /// A floating surface: a popup, a menu, the launcher.
    ///
    /// [`base`](Self::base) at [`panel_alpha`](Self::panel_alpha) — the only
    /// place transparency is applied, because a panel is the only thing that
    /// floats over something the user might want to keep seeing.
    #[must_use]
    pub fn panel_bg(&self) -> Color {
        with_alpha(self.base, self.panel_alpha)
    }

    /// The hovered row of a floating surface.
    ///
    /// Translucent to the same degree as the panel it sits in. A hover
    /// highlight that stayed opaque inside a translucent menu would read as a
    /// solid tile skating over the wallpaper.
    #[must_use]
    pub fn panel_hover(&self) -> Color {
        with_alpha(self.surface1, self.panel_alpha)
    }

    /// A dim layer over everything behind a modal.
    ///
    /// Black in both modes, for the same reason [`shadow`](Self::shadow) is:
    /// a scrim is an absence of light rather than a colour, and its job is to
    /// push the background back. The light palette makes that argument
    /// necessary rather than merely tidy — the shell used to dim with its own
    /// `base` at alpha, and Latte's base is `#EFF1F5`, so in light mode that
    /// would have *lightened* the desktop and left the dialog with nothing to
    /// stand out against.
    #[must_use]
    pub fn scrim(&self) -> Color {
        Color::rgba(0, 0, 0, 140)
    }

    /// The drop shadow under a floating surface, at its strongest.
    ///
    /// One value, where the shell had three — 100, 120 and 160 in three
    /// modules that all draw the same kind of popup. None of the three was
    /// chosen against the others; they were each chosen alone. A renderer that
    /// fades the shadow outward starts here and falls to nothing.
    ///
    /// Distinct from `DecorationColors::shadow`, which is the shadow under a
    /// *window* and is weaker: a window sits on the desktop, a popup sits on
    /// top of a window, and the second wants more separation than the first.
    #[must_use]
    pub fn shadow(&self) -> Color {
        Color::rgba(0, 0, 0, 120)
    }

    /// The hard shadow behind text drawn straight onto the wallpaper.
    ///
    /// Much stronger than [`shadow`](Self::shadow) and deliberately not
    /// theme-dependent: a desktop icon's label lands on an arbitrary
    /// photograph, so its legibility cannot come from the palette. This is the
    /// one colour here that is not a design choice but a floor.
    #[must_use]
    pub fn text_shadow(&self) -> Color {
        Color::rgba(0, 0, 0, 180)
    }

    /// Text drawn straight onto the wallpaper — a desktop icon's label.
    ///
    /// The companion to [`text_shadow`](Self::text_shadow), and pale in *both*
    /// modes for the same reason that one is black in both: this text does not
    /// land on the palette, it lands on whatever photograph the user chose. A
    /// label that followed the mode would be dark on Latte, and dark text
    /// under a black shadow is not legible on anything — the shadow stops
    /// being a floor and becomes a smudge. Pale-on-black-shadow survives a
    /// light wallpaper and a dark one, which is the whole job.
    ///
    /// This is why the icon layer does not read [`text`](Self::text): the
    /// wallpaper is not a surface this crate knows the colour of, so the one
    /// safe choice is the same one at every setting. Compare
    /// [`scrim`](Self::scrim) — same argument, same conclusion (§525 decision
    /// 3).
    #[must_use]
    pub fn on_wallpaper(&self) -> Color {
        LIGHT_EXTREME
    }

    /// A wallpaper label that is *not* the one being pointed at.
    ///
    /// Dimmed with alpha rather than with a darker colour, because darkening
    /// toward the wallpaper is exactly the move that fails on a dark
    /// wallpaper. Alpha lets the shadow keep doing the work.
    #[must_use]
    pub fn on_wallpaper_dim(&self) -> Color {
        with_alpha(LIGHT_EXTREME, 200)
    }

    /// The interior of a committed selection, or of a snap zone at rest.
    #[must_use]
    pub fn selection_fill(&self) -> Color {
        with_alpha(self.accent, wash::FILL)
    }

    /// The outline of a selection or of a highlighted snap zone.
    #[must_use]
    pub fn selection_border(&self) -> Color {
        with_alpha(self.accent, wash::EDGE)
    }

    /// The interior of something the pointer is still drawing — a marquee.
    #[must_use]
    pub fn hint_fill(&self) -> Color {
        with_alpha(self.accent, wash::HINT)
    }

    /// The outline of a marquee.
    #[must_use]
    pub fn hint_border(&self) -> Color {
        with_alpha(self.accent, wash::HINT_EDGE)
    }

    /// The one zone or item the pointer is currently over.
    #[must_use]
    pub fn highlight_fill(&self) -> Color {
        with_alpha(self.accent, wash::HIGHLIGHT)
    }

    /// Where a drag would land if it were released now.
    ///
    /// Green rather than the accent, and that is not decoration: a drop target
    /// and a selection are shown at the same moment during a drag, so they
    /// have to differ by hue and not merely by alpha.
    #[must_use]
    pub fn drop_target(&self) -> Color {
        with_alpha(self.green, 60)
    }
}

/// How a box is told apart from what is behind it. See [`crate::surface`].
///
/// `Borders` is the default as of §829. `Cards` is the arrangement that shipped
/// before it, kept because the operator asked for it to remain available -- and
/// because keeping it is what forced the decision into one place instead of
/// leaving 790 draw sites each making it again.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SurfaceStyle {
    /// Boxes are outlined. The default.
    #[default]
    Borders,
    /// Boxes are filled from the surface ladder. The optional theme.
    Cards,
}

/// How a full-width structural band -- a toolbar, a status bar, a tab strip --
/// is told apart from the content beside it. See [`crate::surface::Surface::Strip`].
///
/// A separate setting from [`SurfaceStyle`] rather than a third variant of it,
/// because the two are orthogonal: someone may want outlined boxes with banded
/// chrome, or filled cards with hairline chrome. Folding them into one enum
/// would offer four combinations as two. §835.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum StripStyle {
    /// A pale band, as shipped. The default (§835).
    #[default]
    Filled,
    /// No band; a hairline along the edge facing the content.
    Separator,
}

// ---------------------------------------------------------------------------
// The seam
// ---------------------------------------------------------------------------

/// What a palette needs to know about the user's saved appearance settings.
///
/// Declared here and implemented in `gui/appearance`, which is the only way
/// round that does not make a cycle: `appearance` already depends on this
/// crate. The method list is deliberately the *whole* of what
/// [`Palette::from_settings`] reads -- eight fields, not a settings object --
/// so that adding a preference does not oblige the toolkit to learn about it.
pub trait PaletteSource {
    /// Is the chosen mode a light one?
    fn is_light(&self) -> bool;

    /// The accent the user chose, already resolved for the mode.
    fn accent(&self) -> Color;

    /// The accent to use against a specific background.
    ///
    /// Separate from [`accent`](Self::accent) because a high-contrast scheme
    /// picks its own background, and the accent that reads best on black is
    /// not the one that reads best on white. A custom accent is returned
    /// unchanged: the user named a colour, not a preference for legibility.
    fn accent_on(&self, background: Color) -> Color;

    /// Whether boxes are outlined or filled.
    fn surface_style(&self) -> SurfaceStyle;

    /// Whether a full-width strip is filled or separated by a hairline.
    fn strip_style(&self) -> StripStyle;

    /// How opaque a floating panel is, 0-255.
    fn panel_alpha(&self) -> u8;

    /// The background and text of the chosen high-contrast scheme, if any.
    ///
    /// Resolved colours rather than the scheme itself, because the scheme is a
    /// user preference and this crate does not model preferences.
    fn high_contrast(&self) -> Option<(Color, Color)>;
}
