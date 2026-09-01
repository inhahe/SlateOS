//! Slate OS Hangman -- a word game with an on-screen keyboard you can
//! actually press, in a real window.
//!
//! 100+ words across five categories, three difficulties, a hint, and the
//! figure drawn a limb at a time. What is new is that all of it is placed
//! from the window the compositor gave us rather than from ten constants
//! describing a 740x560 picture, and that every key, category and button the
//! drawing pass paints also records a hit box, so the alphabet on screen is a
//! keyboard rather than a diagram of one. Before this it was a diagram: the
//! program had no mouse handler at all.
//!
//! The pieces the layout must reconcile are a gallows that wants to be square,
//! a word whose width is the number of letters in it, a three-row keyboard
//! that must stay pressable, and a statistics column. They cannot all have
//! what they want in a small window, so the column is dropped whole and the
//! keyboard is measured from the room left rather than squeezed -- a key too
//! small to hit is worse than no column.

use guitk::color::Color;
use guitk::event::{Event, EventResult, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::{Frame, Rect};
use guitk::probe::Probe;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::rng::{RandomSource, SeededRng, seed_from_system};
use guitk::style::CornerRadii;
use guitk::text;
use oswindow::app::{self, App, Response};
use std::process::ExitCode;

// -- Catppuccin Mocha palette -------------------------------------------
const BASE: Color = Color::from_hex(0x1E1E2E);
const MANTLE: Color = Color::from_hex(0x181825);
const SURFACE0: Color = Color::from_hex(0x313244);
const TEXT_COLOR: Color = Color::from_hex(0xCDD6F4);
const BLUE: Color = Color::from_hex(0x89B4FA);
const GREEN: Color = Color::from_hex(0xA6E3A1);
const RED: Color = Color::from_hex(0xF38BA8);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const PEACH: Color = Color::from_hex(0xFAB387);
const OVERLAY0: Color = Color::from_hex(0x6C7086);
const TEAL: Color = Color::from_hex(0x94E2D5);
const MAUVE: Color = Color::from_hex(0xCBA6F7);
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const LAVENDER: Color = Color::from_hex(0xB4BEFE);

/// The window the program asks for, and the size its tests draw at.
const WINDOW_WIDTH: f32 = 740.0;
const WINDOW_HEIGHT: f32 = 560.0;

/// Maximum wrong guesses before the game is lost.
///
/// The figure has exactly this many parts. `draw_figure` iterates
/// [`FIGURE_PARTS`] rather than testing `wrong_count >= 1 .. >= 6` in six
/// hand-written blocks, so the two cannot disagree about how many strikes a
/// player gets -- which they would have, silently, the moment either changed.
const MAX_WRONG: usize = 6;

/// The three rows of the on-screen keyboard, in the order they are drawn.
const KEY_ROWS: [&[u8]; 3] = [b"QWERTYUIOP", b"ASDFGHJKL", b"ZXCVBNM"];

/// The hint button's label, and the two the result overlay offers.
///
/// Named because the layout measures them to size the buttons and the drawing
/// pass draws them; a string measured in one function and drawn in another is
/// a box sized for a line it does not contain (known-issues lesson 93).
const HINT_LABEL: &str = "Hint";
const HINT_SPENT_LABEL: &str = "Hint used";
const PLAY_AGAIN_LABEL: &str = "Play again";
const MENU_LABEL: &str = "Categories";

/// What the pointer can land on.
///
/// The drawing pass records one of these for every key, category row,
/// difficulty chip and button it paints, and `handle_mouse` asks the frame
/// rather than recomputing the geometry. The program used to have no mouse
/// handler whatever: the alphabet was drawn and could only be typed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    /// A key of the on-screen alphabet, as its lowercase ASCII byte.
    Letter(u8),
    /// The hint button, live only while a hint is still owed.
    Hint,
    /// A row of the category menu.
    Category(usize),
    /// One of the three difficulty chips.
    Difficulty(Difficulty),
    /// Deal a new word.
    PlayAgain,
    /// Back to the category menu.
    Menu,
}

/// Every rectangle and type size the frame is drawn from, solved from the
/// window the compositor actually gave us.
///
/// This replaced ten constants -- `PADDING`, `HEADER_HEIGHT`, `GALLOWS_SIZE`,
/// `WORD_AREA_HEIGHT`, `KEYBOARD_HEIGHT`, `STATS_PANEL_WIDTH` and four font
/// sizes among them. `render` took no width and no height at all and painted
/// the same 740x560 picture into every window there was.
#[derive(Debug, Clone, Copy)]
struct Layout {
    window: Rect,
    /// The bar along the top holding the category, difficulty and hint.
    header: Rect,
    /// The gallows and the figure hanging from it.
    gallows: Rect,
    /// The row of blanks and revealed letters.
    word: Rect,
    /// The three rows of keys.
    keyboard: Rect,
    /// The statistics column, or [`Rect::EMPTY`] when the window cannot pay
    /// for one.
    stats: Rect,
    /// The side of one key, and the gap between two.
    key: f32,
    key_gap: f32,
    pad: f32,
    title: f32,
    font: f32,
    word_font: f32,
    small: f32,
}

impl Layout {
    /// Solve the layout for a window of `w` by `h`.
    ///
    /// The order the room is spent in is the order the parts stop being
    /// usable: the keyboard first, because a key too small to hit ends the
    /// game; the word next, because a word that does not fit cannot be read;
    /// the statistics column last, because it is the only part the game can
    /// be played without. That is why the column is dropped whole rather than
    /// narrowed, and why the gallows takes what is left rather than a fixed
    /// 220 px.
    fn solve(w: f32, h: f32) -> Self {
        let w = w.max(0.0);
        let h = h.max(0.0);
        let window = Rect::new(0.0, 0.0, w, h);
        let pad = (w.min(h) * 0.022).clamp(4.0, 16.0);
        let font = (h / 38.0).clamp(9.0, 16.0);
        let title = (font * 1.4).clamp(12.0, 24.0);
        let small = (font - 2.0).max(7.0);

        let header = Rect::new(pad, pad, (w - pad * 2.0).max(0.0), (font * 3.4).min(h));

        // The keyboard is measured before anything else is given room. Ten
        // keys and their gaps have to fit the width, and three rows have to
        // fit a third of the height; whichever is tighter decides the key.
        let widest_row = KEY_ROWS.iter().map(|r| r.len()).max().unwrap_or(1);
        #[expect(
            clippy::cast_precision_loss,
            reason = "at most ten keys in a row; exact in f32"
        )]
        let cols = widest_row.max(1) as f32;
        let by_width = ((w - pad * 4.0) / cols - pad * 0.4).max(0.0);
        let by_height = (h * 0.30 / 3.0 - pad * 0.4).max(0.0);
        let key = by_width.min(by_height).clamp(0.0, 34.0);
        let key_gap = (key * 0.14).clamp(1.0, 5.0);
        let kb_w = cols * key + (cols - 1.0) * key_gap;
        let kb_h = 3.0f32.mul_add(key, 2.0 * key_gap);
        let keyboard = Rect::new(
            ((w - kb_w) / 2.0).max(0.0),
            (h - pad - kb_h).max(header.bottom()),
            kb_w,
            kb_h,
        );

        // What is left between the header and the keyboard is shared by the
        // gallows, the word and -- if it can be afforded -- the column.
        //
        // The `min` is not belt and braces. `keyboard.y` is floored at the
        // header's bottom, not at the header's bottom plus a pad, so in a
        // window short enough that the header and the keys have already spent
        // the height -- 900x55 is one -- the band that is "what is left"
        // starts a pixel or two *below* where it ends. `free_h` then clamps to
        // zero and hides it, but the word row is placed at `free_y` and would
        // be drawn over the top row of keys. A band cannot begin after it
        // finishes.
        let free_y = (header.bottom() + pad).min(keyboard.y);
        let free_h = (keyboard.y - pad - free_y).max(0.0);

        // The column is worth having only if it can hold its widest line and
        // still leave the gallows a square to hang from.
        let stats_w_min = STATS_LINES
            .iter()
            .fold(0.0f32, |acc, s| {
                acc.max(text::measure(s, small, FontWeightHint::Regular))
            })
            .max(text::measure(STATS_HEADING, font, FontWeightHint::Bold))
            + pad * 2.0;
        let stats_w = if w - stats_w_min - pad * 3.0 >= free_h.max(140.0) {
            stats_w_min
        } else {
            0.0
        };
        let stats = if stats_w > 0.0 {
            Rect::new(w - pad - stats_w, free_y, stats_w, free_h)
        } else {
            Rect::EMPTY
        };

        let main_w = (if stats.w > 0.0 {
            stats.x - pad - pad
        } else {
            w - pad * 2.0
        })
        .max(0.0);
        let word_h = (word_font_for(font) * 2.0).min(free_h);
        let gallows = Rect::new(pad, free_y, main_w, (free_h - word_h).max(0.0));
        let word = Rect::new(pad, gallows.bottom(), main_w, word_h);

        Self {
            window,
            header,
            gallows,
            word,
            keyboard,
            stats,
            key,
            key_gap,
            pad,
            title,
            font,
            word_font: word_font_for(font),
            small,
        }
    }

    /// The rectangle of the key for `letter`, or `None` if there is no key for
    /// it or the keyboard has been squeezed out of existence.
    ///
    /// There is deliberately no inverse of this -- no `key_at(x, y)`. The
    /// drawing pass records a [`Target::Letter`] on every key it paints and a
    /// click is answered by `hit_test`, so a keyboard painted wrongly cannot
    /// be pressed rightly.
    fn key_rect(&self, letter: u8) -> Option<Rect> {
        if self.key <= 0.0 {
            return None;
        }
        let upper = letter.to_ascii_uppercase();
        for (row_i, row) in KEY_ROWS.iter().enumerate() {
            let Some(col) = row.iter().position(|&b| b == upper) else {
                continue;
            };
            #[expect(
                clippy::cast_precision_loss,
                reason = "three rows of at most ten keys; exact in f32"
            )]
            let (rowf, colf) = (row_i as f32, col as f32);
            #[expect(
                clippy::cast_precision_loss,
                reason = "at most ten keys in a row; exact in f32"
            )]
            let indent = KEY_ROWS
                .first()
                .map_or(0, |r| r.len())
                .saturating_sub(row.len()) as f32
                / 2.0;
            let step = self.key + self.key_gap;
            return Some(Rect::new(
                self.keyboard.x + (colf + indent) * step,
                self.keyboard.y + rowf * step,
                self.key,
                self.key,
            ));
        }
        None
    }
}

/// The word is drawn larger than the body text, but not so much larger that a
/// long word stops fitting; one place computes it so the layout and the
/// drawing cannot disagree.
fn word_font_for(font: f32) -> f32 {
    (font * 1.7).clamp(12.0, 30.0)
}

/// The heading over the statistics column.
const STATS_HEADING: &str = "Statistics";

/// The statistics column's lines, at the widest count it is sized for.
///
/// Measured by [`Layout::solve`] to decide whether the column is worth
/// drawing, and drawn -- with the real counts substituted -- by `draw_stats`.
const STATS_LINES: [&str; 6] = [
    "Wins: 888",
    "Losses: 888",
    "Streak: 888",
    "Best: 888",
    "Win Rate: 100%",
    "Games: 888",
];

// -- The gallows and the figure -----------------------------------------

/// The side of the square the gallows drawing was originally authored in.
///
/// Every coordinate in [`GALLOWS_LINES`] and [`FIGURE_PARTS`] is expressed in
/// this space and scaled by `l.gallows.w / GALLOWS_UNITS` at draw time, so the
/// picture keeps its proportions in a window of any size. Previously the
/// numbers were written straight into the drawing code against a fixed
/// 740x560 window, which is why the app could not be resized.
const GALLOWS_UNITS: f32 = 220.0;

/// How thick the rule under each blank letter is.
///
/// Named because the rule is a stroke, and a stroke straddles the line it is
/// drawn on: half of it is below `y`. The check that keeps the rule inside the
/// word row has to know that half, and a literal `2.0` in the check and
/// another in the command are two numbers that agree until one of them moves.
const RULE_WIDTH: f32 = 2.0;

/// The four strokes of the gallows: base, post, beam, rope.
///
/// `(x1, y1, x2, y2)` in the [`GALLOWS_UNITS`] space.
const GALLOWS_LINES: [(f32, f32, f32, f32); 4] = [
    (0.0, 210.0, 120.0, 210.0),
    (30.0, 10.0, 30.0, 210.0),
    (30.0, 10.0, 100.0, 10.0),
    (100.0, 10.0, 100.0, 40.0),
];

/// One piece of the hanged figure, in the [`GALLOWS_UNITS`] space.
#[derive(Clone, Copy)]
enum FigurePart {
    /// The head.
    Circle { cx: f32, cy: f32, r: f32 },
    /// The body and the four limbs.
    Limb { x1: f32, y1: f32, x2: f32, y2: f32 },
}

/// The figure, one part per wrong guess, in the order they appear.
///
/// The array's length is `MAX_WRONG`, so a change to the number of allowed
/// wrong guesses is a compile error here rather than a figure that stops
/// halfway. The old code had six `if wrong_count >= N` blocks instead, each
/// repeating a number that `MAX_WRONG` already held.
const FIGURE_PARTS: [FigurePart; MAX_WRONG] = [
    FigurePart::Circle {
        cx: 100.0,
        cy: 55.0,
        r: 15.0,
    },
    FigurePart::Limb {
        x1: 100.0,
        y1: 70.0,
        x2: 100.0,
        y2: 120.0,
    },
    FigurePart::Limb {
        x1: 100.0,
        y1: 85.0,
        x2: 75.0,
        y2: 105.0,
    },
    FigurePart::Limb {
        x1: 100.0,
        y1: 85.0,
        x2: 125.0,
        y2: 105.0,
    },
    FigurePart::Limb {
        x1: 100.0,
        y1: 120.0,
        x2: 78.0,
        y2: 150.0,
    },
    FigurePart::Limb {
        x1: 100.0,
        y1: 120.0,
        x2: 122.0,
        y2: 150.0,
    },
];

// -- Drawing helpers ----------------------------------------------------

/// Fill `r` with `color`, rounded by `radii`.
///
/// A named helper because a `Rect` is what the layout produces and four
/// separate `x`/`y`/`width`/`height` fields are what the renderer wants; doing
/// that conversion once means a rect cannot be drawn transposed.
fn fill(f: &mut Frame<Target>, r: Rect, color: Color, radii: CornerRadii) {
    f.push(RenderCommand::FillRect {
        x: r.x,
        y: r.y,
        width: r.w,
        height: r.h,
        color,
        corner_radii: radii,
    });
}

/// Draw `s` with its top-left corner at `(x, y)`.
#[expect(
    clippy::too_many_arguments,
    reason = "these are exactly the parameters a text command takes; \
              grouping them into a struct would only move the same list"
)]
fn text_at(
    f: &mut Frame<Target>,
    s: &str,
    x: f32,
    y: f32,
    size: f32,
    weight: FontWeightHint,
    color: Color,
    max_w: Option<f32>,
) {
    f.push(RenderCommand::Text {
        x,
        y,
        text: String::from(s),
        color,
        font_size: size,
        font_weight: weight,
        max_width: max_w,
        overflow: TextOverflow::Ellipsis,
    });
}

/// Draw `s` horizontally centred within `within`, with its top at `y`.
///
/// The centre comes from measuring the string that is about to be drawn, not
/// from a constant guessed for one particular string at one particular font
/// size -- the menu title used to be centred by subtracting a literal 80.0
/// (lesson 93).
fn centred_text(
    f: &mut Frame<Target>,
    s: &str,
    within: Rect,
    y: f32,
    size: f32,
    weight: FontWeightHint,
    color: Color,
) {
    let tw = text::measure(s, size, weight);
    // `.max(within.x)` because centring is not a bound in this direction
    // either: a string wider than the box it is centred in starts to the
    // *left* of the box and hangs the same distance off the other end.
    let x = (within.x + (within.w - tw) / 2.0).max(within.x);
    text_at(f, s, x, y, size, weight, color, Some(within.right() - x));
}

/// The `y` at which a run `size` points tall sits centred in `band`, or `None`
/// when the band is too short to hold it.
///
/// The whole of lesson 109 in four lines. `band.y + (band.h - size) / 2.0`
/// written inline is *above* `band.y` the moment the band is shorter than the
/// line, and hangs the same distance below the band's bottom -- so a heading
/// centred in a strip squeezed by a small window paints on whatever is above
/// and below it. Every vertically-centred run in this program goes through
/// here, rather than through nine copies of the same subtraction, because that
/// is how a rule comes to hold in eight places and not the ninth.
fn centre_line(band: Rect, size: f32) -> Option<f32> {
    (!band.is_empty() && band.h >= size).then(|| band.y + (band.h - size) / 2.0)
}

/// The left edge and the width available to one run of text inside `band`.
///
/// `inset` is the gap from the band's left edge for a left-aligned run; `None`
/// asks for the run to be centred. Answers `None` when the band has no room
/// left at all, so a caller cannot draw into a box of no width.
///
/// The centred arm clamps at `band.x` for the same reason [`centred_text`]
/// does, and the returned width is measured from the run's *actual* left edge
/// rather than being `band.w`: a run inset by a pad and given `band.w` as its
/// `max_width` may elide nothing and still finish a pad past the right edge.
fn span(
    band: Rect,
    s: &str,
    size: f32,
    weight: FontWeightHint,
    inset: Option<f32>,
) -> Option<(f32, f32)> {
    if band.is_empty() {
        return None;
    }
    let x = match inset {
        Some(pad) => band.x + pad,
        None => (band.x + (band.w - text::measure(s, size, weight)) / 2.0).max(band.x),
    };
    let w = band.right() - x;
    (w > 0.0).then_some((x, w))
}

/// Draw one line of text inside `band` and nowhere else.
///
/// Refuses -- draws nothing at all -- when the band cannot hold the line,
/// rather than painting it half outside. That refusal is the point: see
/// [`centre_line`] and [`span`], which are the two halves of it.
fn run_in(
    f: &mut Frame<Target>,
    band: Rect,
    s: &str,
    size: f32,
    weight: FontWeightHint,
    color: Color,
    inset: Option<f32>,
) {
    let Some(y) = centre_line(band, size) else {
        return;
    };
    let Some((x, w)) = span(band, s, size, weight, inset) else {
        return;
    };
    text_at(f, s, x, y, size, weight, color, Some(w));
}

/// Draw one line of a running column at `y`, inset `inset` from `band`'s left
/// edge; answer whether there was room for it.
///
/// A column is the other way a run gets placed in this program: not centred in
/// a band but stacked down one, each line at wherever the last one finished.
/// The guard is the same guard every time -- `y + size > band.bottom()` -- and
/// it was written out five times in [`Self::draw_stats`] and once more in
/// [`Self::draw_menu`], which is five chances to write it and one to forget.
/// The heading of the statistics column is where it was forgotten.
///
/// Answering `false` rather than clamping is deliberate: a column that has run
/// out has run out, and the caller must stop, not squeeze the remaining lines
/// into the last few points.
fn column_line(
    f: &mut Frame<Target>,
    band: Rect,
    y: f32,
    s: &str,
    size: f32,
    weight: FontWeightHint,
    color: Color,
    inset: Option<f32>,
) -> bool {
    if y < band.y || y + size > band.bottom() {
        return false;
    }
    let Some((x, w)) = span(band, s, size, weight, inset) else {
        return false;
    };
    text_at(f, s, x, y, size, weight, color, Some(w));
    true
}

// -- Randomness ---------------------------------------------------------

/// Seed used when the kernel's entropy source cannot be reached.
///
/// A per-crate constant rather than a shared one, so that two programs which
/// lose entropy on the same boot do not then produce correlated streams. The
/// bytes spell `HANGMAN!`.
const FALLBACK_SEED: u64 = 0x4841_4E47_4D41_4E21;

// This crate used to carry its own copy of the LCG that got copied into
// sixteen crates, reducing with `val % bound`. That is the broken reduction:
// the generator's modulus is 2^64, so bit *k* of its state has period 2^(k+1)
// and the low bits are a counter rather than a draw. Any power-of-two bound
// reads only those.
//
// Picking the word escaped it -- the word lists are odd lengths, and an odd
// bound's remainder depends on all 64 bits. Picking which letters to reveal
// did not. `apply_free_reveals` and the hint both draw against
// `unrevealed.len()`, the number of *distinct* letters still hidden, which for
// ordinary English words is most often somewhere between 4 and 8 and so lands
// on a power of two a good part of the time. On those words the "random"
// letter revealed was a fixed function of how many draws the game had made,
// which is to say the same letter every time the game reached that state.
//
// `randrange::below` is Lemire's method: it multiplies by the bound into 128
// bits and keeps the *top* half, so it reads the high bits and never the low
// ones, with a rejection step that makes it exactly uniform.

// -- Category -----------------------------------------------------------
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Category {
    Animals,
    Fruits,
    Countries,
    Sports,
    Technology,
}

impl Category {
    const ALL: [Category; 5] = [
        Category::Animals,
        Category::Fruits,
        Category::Countries,
        Category::Sports,
        Category::Technology,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Animals => "Animals",
            Self::Fruits => "Fruits",
            Self::Countries => "Countries",
            Self::Sports => "Sports",
            Self::Technology => "Technology",
        }
    }

    fn color(self) -> Color {
        match self {
            Self::Animals => PEACH,
            Self::Fruits => GREEN,
            Self::Countries => BLUE,
            Self::Sports => YELLOW,
            Self::Technology => MAUVE,
        }
    }

    fn words(self) -> &'static [&'static str] {
        match self {
            Self::Animals => &ANIMALS,
            Self::Fruits => &FRUITS,
            Self::Countries => &COUNTRIES,
            Self::Sports => &SPORTS,
            Self::Technology => &TECHNOLOGY,
        }
    }

    fn from_index(i: usize) -> Option<Self> {
        match i {
            0 => Some(Self::Animals),
            1 => Some(Self::Fruits),
            2 => Some(Self::Countries),
            3 => Some(Self::Sports),
            4 => Some(Self::Technology),
            _ => None,
        }
    }
}

// -- Word lists (100+ total) --------------------------------------------
const ANIMALS: [&str; 24] = [
    "elephant", "giraffe", "penguin", "dolphin", "kangaroo", "cheetah", "octopus", "flamingo",
    "buffalo", "panther", "leopard", "hamster", "gazelle", "toucan", "walrus", "pelican",
    "lobster", "sparrow", "raccoon", "vulture", "gorilla", "seahorse", "parrot", "falcon",
];

const FRUITS: [&str; 22] = [
    "banana",
    "strawberry",
    "pineapple",
    "blueberry",
    "raspberry",
    "watermelon",
    "tangerine",
    "coconut",
    "avocado",
    "apricot",
    "pomegranate",
    "cranberry",
    "nectarine",
    "dragonfruit",
    "mulberry",
    "blackberry",
    "mandarin",
    "papaya",
    "guava",
    "lychee",
    "mango",
    "cherry",
];

const COUNTRIES: [&str; 22] = [
    "australia",
    "argentina",
    "brazil",
    "canada",
    "denmark",
    "ethiopia",
    "finland",
    "germany",
    "hungary",
    "iceland",
    "jamaica",
    "kenya",
    "malaysia",
    "norway",
    "portugal",
    "romania",
    "singapore",
    "thailand",
    "ukraine",
    "vietnam",
    "colombia",
    "morocco",
];

const SPORTS: [&str; 20] = [
    "basketball",
    "football",
    "baseball",
    "swimming",
    "wrestling",
    "volleyball",
    "badminton",
    "archery",
    "fencing",
    "lacrosse",
    "kayaking",
    "climbing",
    "cycling",
    "handball",
    "softball",
    "triathlon",
    "sprinting",
    "javelin",
    "hurdles",
    "canoeing",
];

const TECHNOLOGY: [&str; 20] = [
    "algorithm",
    "bluetooth",
    "compiler",
    "database",
    "ethernet",
    "firmware",
    "graphics",
    "hardware",
    "internet",
    "javascript",
    "keyboard",
    "terminal",
    "microchip",
    "notebook",
    "software",
    "protocol",
    "robotics",
    "transistor",
    "wireless",
    "processor",
];

// -- Difficulty ---------------------------------------------------------
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Difficulty {
    Easy,
    Medium,
    Hard,
}

impl Difficulty {
    /// Every difficulty, in the order the chips are drawn and the order the
    /// `1`/`2`/`3` keys select. One list, so a fourth could not be typed
    /// without also being drawn.
    const ALL: [Self; 3] = [Self::Easy, Self::Medium, Self::Hard];

    fn label(self) -> &'static str {
        match self {
            Self::Easy => "Easy",
            Self::Medium => "Medium",
            Self::Hard => "Hard",
        }
    }

    /// Minimum word length for this difficulty.
    fn min_length(self) -> usize {
        match self {
            Self::Easy => 3,
            Self::Medium => 6,
            Self::Hard => 8,
        }
    }

    /// Maximum word length for this difficulty.
    fn max_length(self) -> usize {
        match self {
            Self::Easy => 6,
            Self::Medium => 8,
            Self::Hard => 20,
        }
    }

    /// Number of letters revealed at start as a free hint.
    fn free_reveals(self) -> usize {
        match self {
            Self::Easy => 2,
            Self::Medium => 1,
            Self::Hard => 0,
        }
    }
}

// -- Game state ---------------------------------------------------------
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GamePhase {
    /// Player is choosing a category.
    CategorySelect,
    /// Actively playing a round.
    Playing,
    /// Round won.
    Won,
    /// Round lost.
    Lost,
}

// -- Stats --------------------------------------------------------------
#[derive(Clone, Debug, PartialEq, Eq)]
struct Stats {
    wins: u32,
    losses: u32,
    current_streak: u32,
    best_streak: u32,
}

impl Stats {
    fn new() -> Self {
        Self {
            wins: 0,
            losses: 0,
            current_streak: 0,
            best_streak: 0,
        }
    }

    // A counter that saturates rather than wraps: a player who somehow
    // reaches four billion wins keeps the last honest number instead of
    // dropping back to zero.
    fn record_win(&mut self) {
        self.wins = self.wins.saturating_add(1);
        self.current_streak = self.current_streak.saturating_add(1);
        self.best_streak = self.best_streak.max(self.current_streak);
    }

    fn record_loss(&mut self) {
        self.losses = self.losses.saturating_add(1);
        self.current_streak = 0;
    }

    fn total_games(&self) -> u32 {
        self.wins.saturating_add(self.losses)
    }

    fn win_rate_percent(&self) -> u32 {
        let total = self.total_games();
        if total == 0 {
            return 0;
        }
        // `total` is at least `wins`, so the quotient is at most 100 and the
        // product cannot overflow a u64 for any u32 win count.
        let percent = u64::from(self.wins)
            .saturating_mul(100)
            .checked_div(u64::from(total))
            .unwrap_or(0);
        u32::try_from(percent).unwrap_or(100)
    }
}

// -- Main app struct ----------------------------------------------------
struct HangmanApp {
    /// The secret word (lowercase ASCII).
    word: Vec<u8>,
    /// Which letters (a-z) have been guessed. Index 0 = 'a'.
    guessed: [bool; 26],
    /// Number of wrong guesses so far.
    wrong_count: usize,
    /// Current game phase.
    phase: GamePhase,
    /// Selected category.
    category: Category,
    /// Difficulty level.
    difficulty: Difficulty,
    /// Whether the hint has been used this round.
    hint_used: bool,
    /// Persistent stats.
    stats: Stats,
    /// RNG state.
    rng: SeededRng,
    /// Index of the highlighted category in selection screen.
    category_cursor: usize,
    /// The size the last frame was drawn at, in pixels.
    ///
    /// A click arrives without a size, and it has to be resolved against the
    /// same layout the user was looking at -- so the size is recorded when
    /// the frame is drawn and read back when the click lands.
    size: (f32, f32),
}

impl HangmanApp {
    /// Create a new Hangman game with default settings.
    fn new() -> Self {
        // Was `with_seed(42)`: every player, on every machine, got the same
        // word, and then the same word again in the same order for every round
        // after it. Predicting a hangman word costs the user nothing but the
        // game, so this asks the kernel and falls back rather than refusing --
        // see `randrange::seeded_from_system`.
        Self::with_seed(seed_from_system(FALLBACK_SEED))
    }

    /// Create a new Hangman game with a specific RNG seed.
    fn with_seed(seed: u64) -> Self {
        let mut app = Self {
            word: Vec::new(),
            guessed: [false; 26],
            wrong_count: 0,
            phase: GamePhase::CategorySelect,
            category: Category::Animals,
            difficulty: Difficulty::Medium,
            hint_used: false,
            stats: Stats::new(),
            rng: SeededRng::new(seed),
            category_cursor: 0,
            size: (WINDOW_WIDTH, WINDOW_HEIGHT),
        };
        app.pick_word();
        app
    }

    /// Record the size the window is now, so clicks resolve against it.
    fn resize(&mut self, w: f32, h: f32) {
        self.size = (w, h);
    }

    // -- Word selection -------------------------------------------------

    /// Pick a random word from the current category, filtered by difficulty.
    fn pick_word(&mut self) {
        let words = self.category.words();
        let min_len = self.difficulty.min_length();
        let max_len = self.difficulty.max_length();

        // Collect eligible words.
        let eligible: Vec<&str> = words
            .iter()
            .filter(|w| w.len() >= min_len && w.len() <= max_len)
            .copied()
            .collect();

        // If no words match the difficulty filter, use all words.
        let pool = if eligible.is_empty() {
            words
        } else {
            &eligible
        };

        let idx = self.rng.below(pool.len());
        // A category is never empty -- `Category::words` returns a non-empty
        // constant array -- but an empty word is a survivable state (the
        // drawing skips it) and a panic here is not.
        self.word = pool
            .get(idx)
            .map(|w| w.as_bytes().to_vec())
            .unwrap_or_default();

        // Apply free reveals for easy/medium difficulty.
        self.apply_free_reveals();
    }

    /// The distinct letters of the word that have not been revealed yet.
    ///
    /// One definition, used by both the free reveals and the hint. They each
    /// had their own copy of this loop before, which is two places for the
    /// same rule to be wrong in (lesson 92).
    fn unrevealed_letters(&self) -> Vec<u8> {
        let mut out: Vec<u8> = Vec::new();
        for &b in &self.word {
            if letter_index(b).is_some() && !self.is_guessed(b) && !out.contains(&b) {
                out.push(b);
            }
        }
        out
    }

    /// Reveal one unrevealed letter, chosen uniformly.
    ///
    /// Returns false when the word holds nothing left to reveal, which is what
    /// tells the hint that it was not spent.
    fn reveal_one_at_random(&mut self) -> bool {
        let unrevealed = self.unrevealed_letters();
        let pick = self.rng.below(unrevealed.len());
        let Some(&letter) = unrevealed.get(pick) else {
            return false;
        };
        let Some(slot) = letter_index(letter).and_then(|i| self.guessed.get_mut(i)) else {
            return false;
        };
        *slot = true;
        true
    }

    /// Reveal some letters for free at the start based on difficulty.
    fn apply_free_reveals(&mut self) {
        for _ in 0..self.difficulty.free_reveals() {
            if !self.reveal_one_at_random() {
                break;
            }
        }
    }

    /// Start a new round, preserving stats and settings.
    fn new_round(&mut self) {
        self.guessed = [false; 26];
        self.wrong_count = 0;
        self.hint_used = false;
        self.phase = GamePhase::Playing;
        self.pick_word();
    }

    /// Start a new round after returning to category select.
    fn start_from_category(&mut self) {
        self.guessed = [false; 26];
        self.wrong_count = 0;
        self.hint_used = false;
        self.phase = GamePhase::Playing;
        self.pick_word();
    }

    // -- Guess logic ----------------------------------------------------

    /// Attempt to guess a letter. Returns true if the letter was new.
    fn guess_letter(&mut self, letter: u8) -> bool {
        if self.phase != GamePhase::Playing {
            return false;
        }
        let Some(slot) = letter_index(letter).and_then(|i| self.guessed.get_mut(i)) else {
            return false;
        };
        if *slot {
            return false;
        }
        *slot = true;

        let lower = letter.to_ascii_lowercase();
        let in_word = self.word.contains(&lower);
        if !in_word {
            self.wrong_count = self.wrong_count.saturating_add(1);
        }

        // Check for win/loss.
        if self.wrong_count >= MAX_WRONG {
            self.phase = GamePhase::Lost;
            self.stats.record_loss();
        } else if self.is_word_revealed() {
            self.phase = GamePhase::Won;
            self.stats.record_win();
        }

        true
    }

    /// Whether `letter` has already been guessed.
    ///
    /// Anything that is not a letter counts as unguessed. Reading the array
    /// through one accessor keeps the index arithmetic in a single place.
    fn is_guessed(&self, letter: u8) -> bool {
        letter_index(letter).is_some_and(|i| self.guessed.get(i).copied().unwrap_or(false))
    }

    /// Check if every letter in the word has been guessed.
    fn is_word_revealed(&self) -> bool {
        // Non-letter characters (hyphens, etc.) are always shown, and
        // `is_guessed` reports them as unguessed, so they are filtered out
        // rather than tested.
        self.word
            .iter()
            .filter(|&&b| letter_index(b).is_some())
            .all(|&b| self.is_guessed(b))
    }

    /// Use the hint: reveal one unrevealed letter. Only allowed once.
    fn use_hint(&mut self) -> bool {
        if self.phase != GamePhase::Playing || self.hint_used {
            return false;
        }

        if !self.reveal_one_at_random() {
            return false;
        }
        self.hint_used = true;

        // Check for win after hint.
        if self.is_word_revealed() {
            self.phase = GamePhase::Won;
            self.stats.record_win();
        }

        true
    }

    /// Get the full word as a string (for game over reveal).
    fn word_string(&self) -> String {
        String::from_utf8(self.word.clone()).unwrap_or_default()
    }

    /// The letters guessed that are not in the word, in alphabetical order.
    fn incorrect_letters(&self) -> Vec<u8> {
        (b'a'..=b'z')
            .filter(|&letter| self.is_guessed(letter) && !self.word.contains(&letter))
            .collect()
    }

    /// Remaining wrong guesses before loss.
    fn remaining_guesses(&self) -> usize {
        MAX_WRONG.saturating_sub(self.wrong_count)
    }

    // -- Drawing --------------------------------------------------------

    /// Draw one frame for a window of `w` by `h`.
    ///
    /// Everything is placed from `l`, and every clickable thing records its
    /// hit box here, in the drawing pass, so that a picture drawn in one place
    /// cannot be clicked in another.
    fn frame(&self, w: f32, h: f32) -> Frame<Target> {
        let l = Layout::solve(w, h);
        let mut f = Frame::new(w, h);
        fill(&mut f, l.window, BASE, CornerRadii::ZERO);

        match self.phase {
            GamePhase::CategorySelect => self.draw_menu(&mut f, &l),
            GamePhase::Playing | GamePhase::Won | GamePhase::Lost => {
                self.draw_header(&mut f, &l);
                self.draw_gallows(&mut f, &l);
                self.draw_word(&mut f, &l);
                self.draw_keyboard(&mut f, &l);
                self.draw_stats(&mut f, &l);
                if self.phase != GamePhase::Playing {
                    self.draw_result(&mut f, &l);
                }
            }
        }
        f
    }

    /// The category menu: a column of rows and a row of difficulty chips,
    /// both of which are now clickable. They were keyboard-only, which is the
    /// first screen a player sees.
    fn draw_menu(&self, f: &mut Frame<Target>, l: &Layout) {
        let title = "HANGMAN";
        let subtitle = "Choose a Category";

        // Centred by measuring, not by subtracting a guessed 80.0 from half
        // the window -- which is what this did, so the title sat left of
        // centre at the one size it was tuned for and anywhere at all in any
        // other.
        let title_size = l.title * 1.4;
        let mut y = l.window.y + l.pad * 2.0;
        centred_text(
            f,
            title,
            l.window,
            y,
            title_size,
            FontWeightHint::Bold,
            LAVENDER,
        );
        y += title_size * 1.5;
        centred_text(
            f,
            subtitle,
            l.window,
            y,
            l.font,
            FontWeightHint::Regular,
            SUBTEXT0,
        );
        y += l.font * 2.0;

        let btn_w = (l.window.w * 0.45).clamp(0.0, 320.0);
        let btn_h = (l.font * 2.6).max(1.0);
        let btn_gap = l.pad * 0.6;
        let btn_x = l.window.x + (l.window.w - btn_w) / 2.0;

        for (i, cat) in Category::ALL.iter().enumerate() {
            #[expect(clippy::cast_precision_loss, reason = "five categories; exact in f32")]
            let row_y = (i as f32).mul_add(btn_h + btn_gap, y);
            let r = Rect::new(btn_x, row_y, btn_w, btn_h);
            if r.bottom() > l.window.bottom() {
                break;
            }
            let selected = i == self.category_cursor;
            fill(
                f,
                r,
                if selected { SURFACE0 } else { MANTLE },
                CornerRadii::all(6.0),
            );
            f.push(RenderCommand::StrokeRect {
                x: r.x,
                y: r.y,
                width: r.w,
                height: r.h,
                color: if selected { cat.color() } else { OVERLAY0 },
                line_width: if selected { 2.0 } else { 1.0 },
                corner_radii: CornerRadii::all(6.0),
            });
            run_in(
                f,
                r,
                &format!("{} ({})", cat.label(), cat.words().len()),
                l.font,
                if selected {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                if selected { cat.color() } else { TEXT_COLOR },
                Some(l.pad),
            );
            f.hit(Target::Category(i), r);
        }

        // Below the rows, not below a hand-written `5.0 * (btn_h + btn_gap)`:
        // that 5.0 was the number of categories written a second time, forty
        // lines from `Category::ALL`, and a sixth category would have been
        // drawn underneath the difficulty line.
        #[expect(clippy::cast_precision_loss, reason = "five categories; exact in f32")]
        let rows = Category::ALL.len() as f32;
        let mut chip_y = rows.mul_add(btn_h + btn_gap, y) + l.pad;
        if chip_y + btn_h <= l.window.bottom() {
            self.draw_difficulty_chips(f, l, btn_x, chip_y, btn_w);
            chip_y += btn_h + l.pad;
        }

        for (i, line) in [
            "Up/Down: Select category",
            "Enter: Start game",
            "1/2/3: Easy/Medium/Hard",
        ]
        .iter()
        .enumerate()
        {
            #[expect(clippy::cast_precision_loss, reason = "three lines; exact in f32")]
            let ly = (i as f32).mul_add(l.small * 1.4, chip_y);
            if !column_line(
                f,
                Rect::new(btn_x, l.window.y, btn_w, l.window.h),
                ly,
                line,
                l.small,
                FontWeightHint::Light,
                OVERLAY0,
                Some(0.0),
            ) {
                break;
            }
        }
    }

    /// The three difficulty chips, drawn from [`Difficulty::ALL`] and each
    /// recording a hit box, so the `1`/`2`/`3` keys are a shortcut rather than
    /// the only way in.
    fn draw_difficulty_chips(&self, f: &mut Frame<Target>, l: &Layout, x: f32, y: f32, w: f32) {
        #[expect(clippy::cast_precision_loss, reason = "three chips; exact in f32")]
        let n = Difficulty::ALL.len().max(1) as f32;
        let gap = l.pad * 0.4;
        let chip_w = ((w - gap * (n - 1.0)) / n).max(0.0);
        let chip_h = (l.font * 2.0).max(1.0);
        for (i, diff) in Difficulty::ALL.iter().enumerate() {
            #[expect(clippy::cast_precision_loss, reason = "three chips; exact in f32")]
            let cx = (i as f32).mul_add(chip_w + gap, x);
            let r = Rect::new(cx, y, chip_w, chip_h);
            let on = *diff == self.difficulty;
            fill(
                f,
                r,
                if on { SURFACE0 } else { MANTLE },
                CornerRadii::all(4.0),
            );
            run_in(
                f,
                r,
                diff.label(),
                l.small,
                FontWeightHint::Regular,
                if on { TEAL } else { OVERLAY0 },
                None,
            );
            f.hit(Target::Difficulty(*diff), r);
        }
    }

    /// The header: who we are, what category and difficulty are in play, how
    /// many guesses are left, and the hint button.
    fn draw_header(&self, f: &mut Frame<Target>, l: &Layout) {
        fill(f, l.header, MANTLE, CornerRadii::all(4.0));
        let mut x = l.header.x + l.pad;

        // Each item takes the room it measures and the next starts past it,
        // rather than every item starting at a constant offset that was right
        // for one set of words. `PADDING + 220.0` put the difficulty on top of
        // a long category name.
        //
        // Every item is drawn into a band of its own -- `Rect::new(x, ...)`
        // rather than a bare `top` shared by all four -- because a bare offset
        // is a band nothing can refuse. The shared `top` was
        // `header.y + (header.h - font) / 2.0`, which is above the header in
        // any window short enough to squeeze it, and drew all four items into
        // the gallows.
        for (s, size, weight, color) in [
            ("Hangman", l.font, FontWeightHint::Bold, LAVENDER),
            (
                self.category.label(),
                l.small,
                FontWeightHint::Regular,
                self.category.color(),
            ),
            (
                self.difficulty.label(),
                l.small,
                FontWeightHint::Regular,
                SUBTEXT0,
            ),
        ] {
            let tw = text::measure(s, size, weight);
            if x + tw > l.header.right() - l.pad {
                break;
            }
            run_in(
                f,
                Rect::new(x, l.header.y, tw, l.header.h),
                s,
                size,
                weight,
                color,
                Some(0.0),
            );
            x += tw + l.pad;
        }

        // Right-aligned against the header's *right edge*. This read
        // `x: header_w - 60.0`, and `header_w` is a width, not a coordinate:
        // the win rate was drawn one padding to the left of where it belonged
        // and would have left the header entirely in a narrow window.
        let remaining = self.remaining_guesses();
        let rem = format!("Remaining: {remaining}");
        let rem_w = text::measure(&rem, l.small, FontWeightHint::Regular);
        let rate = format!("{}% win", self.stats.win_rate_percent());
        let rate_w = text::measure(&rate, l.small, FontWeightHint::Regular);

        let mut right = l.header.right() - l.pad;
        if right - rate_w > x {
            run_in(
                f,
                Rect::new(right - rate_w, l.header.y, rate_w, l.header.h),
                &rate,
                l.small,
                FontWeightHint::Regular,
                OVERLAY0,
                Some(0.0),
            );
            right -= rate_w + l.pad;
        }
        if right - rem_w > x {
            run_in(
                f,
                Rect::new(right - rem_w, l.header.y, rem_w, l.header.h),
                &rem,
                l.small,
                FontWeightHint::Regular,
                if remaining <= 2 { RED } else { TEAL },
                Some(0.0),
            );
            right -= rem_w + l.pad;
        }

        // The hint. It was a line of text saying "Hint: H key" -- a label
        // describing a keystroke, where a button would do.
        let live = !self.hint_used && self.phase == GamePhase::Playing;
        let label = if self.hint_used {
            HINT_SPENT_LABEL
        } else {
            HINT_LABEL
        };
        let bw = text::measure(HINT_SPENT_LABEL, l.small, FontWeightHint::Bold) + l.pad * 2.0;
        // A *fill* shrinks to the band rather than being refused by it -- a
        // button an inch shorter than nominal is still a button, whereas a
        // line of text drawn at half its height is a smear. So `.min` here and
        // `centre_line` for the label inside it, which are the two halves of
        // lesson 109 that are easy to mistake for one.
        let bh = (l.font * 1.8).min(l.header.h);
        let br = Rect::new(right - bw, l.header.y + (l.header.h - bh) / 2.0, bw, bh);
        if br.x > x {
            fill(
                f,
                br,
                if live { SURFACE0 } else { MANTLE },
                CornerRadii::all(4.0),
            );
            run_in(
                f,
                br,
                label,
                l.small,
                FontWeightHint::Bold,
                if live { YELLOW } else { OVERLAY0 },
                None,
            );
            if live {
                f.hit(Target::Hint, br);
            }
        }
    }

    /// The gallows and however much of the figure has been earned.
    fn draw_gallows(&self, f: &mut Frame<Target>, l: &Layout) {
        // The square is centred in the band on *both* axes, and the padding is
        // taken out of the side before it is centred rather than added to the
        // left edge afterwards. It used to be `w.min(h)` placed at `x + pad`,
        // so whenever the band was wider than it was tall -- which is the
        // common case, the band being what is left beside the statistics
        // column -- the figure was exactly one padding wider than the room it
        // had, and its right-hand upright was drawn over the column.
        let side = (l.gallows.w - l.pad * 2.0).min(l.gallows.h).max(0.0);
        if side <= 0.0 {
            return;
        }
        let s = side / GALLOWS_UNITS;
        let ox = l.gallows.x + (l.gallows.w - side) / 2.0;
        let oy = l.gallows.y + (l.gallows.h - side) / 2.0;
        let at = |x: f32, y: f32| (s.mul_add(x, ox), s.mul_add(y, oy));

        for &(x1, y1, x2, y2) in &GALLOWS_LINES {
            let (ax, ay) = at(x1, y1);
            let (bx, by) = at(x2, y2);
            f.push(RenderCommand::Line {
                x1: ax,
                y1: ay,
                x2: bx,
                y2: by,
                color: SUBTEXT0,
                width: (s * 3.0).max(1.0),
            });
        }

        let body = if self.phase == GamePhase::Lost {
            RED
        } else {
            TEXT_COLOR
        };
        let width = (s * 2.0).max(1.0);
        // Six wrong guesses, six parts, one list: `MAX_WRONG` is the length of
        // `FIGURE_PARTS`, so the count the rules use and the count the picture
        // shows are the same number rather than a `>= 1` .. `>= 6` ladder
        // written out beside it.
        for part in FIGURE_PARTS.iter().take(self.wrong_count.min(MAX_WRONG)) {
            match *part {
                FigurePart::Circle { cx, cy, r } => {
                    // The renderer draws lines, not arcs, so the head is a
                    // twelve-sided polygon. The step is computed from the
                    // segment count rather than written out as a constant
                    // angle, so the two cannot disagree.
                    const SEGMENTS: u8 = 12;
                    let n = f32::from(SEGMENTS);
                    for seg in 0..SEGMENTS {
                        let i0 = f32::from(seg);
                        let i1 = i0 + 1.0;
                        let a1 = i0 * std::f32::consts::TAU / n;
                        let a2 = i1 * std::f32::consts::TAU / n;
                        let (p1x, p1y) = at(r.mul_add(a1.cos(), cx), r.mul_add(a1.sin(), cy));
                        let (p2x, p2y) = at(r.mul_add(a2.cos(), cx), r.mul_add(a2.sin(), cy));
                        f.push(RenderCommand::Line {
                            x1: p1x,
                            y1: p1y,
                            x2: p2x,
                            y2: p2y,
                            color: body,
                            width,
                        });
                    }
                }
                FigurePart::Limb { x1, y1, x2, y2 } => {
                    let (ax, ay) = at(x1, y1);
                    let (bx, by) = at(x2, y2);
                    f.push(RenderCommand::Line {
                        x1: ax,
                        y1: ay,
                        x2: bx,
                        y2: by,
                        color: body,
                        width,
                    });
                }
            }
        }
    }

    /// The word, as blanks and revealed letters, centred in what the gallows
    /// and the column left.
    fn draw_word(&self, f: &mut Frame<Target>, l: &Layout) {
        if self.word.is_empty() || l.word.w <= 0.0 {
            return;
        }
        // The spacing shrinks so a twelve-letter word still fits the room
        // rather than running off the side of it, which a fixed 26 px did.
        #[expect(
            clippy::cast_precision_loss,
            reason = "words are at most twenty letters; exact in f32"
        )]
        let n = self.word.len().max(1) as f32;
        let step = (l.word.w / n).min(l.word_font);
        let total = n * step;
        let x0 = l.word.x + (l.word.w - total) / 2.0;
        // One `centre_line` for the row rather than one per letter: every
        // letter is the same size, so either the row can hold a line of text
        // or none of it can, and asking once says that plainly.
        let Some(baseline) = centre_line(l.word, l.word_font) else {
            return;
        };
        // The rule under each blank is drawn only when it fits *below the
        // glyph and inside the row*. It used to be an unconditional
        // `baseline + word_font * 1.1`, which is outside a row that the layout
        // squeezed under two font sizes tall -- and the row is squeezed
        // exactly when `word_h` clamps against `free_h`, which any short
        // window does.
        let rule_y = l.word_font.mul_add(1.1, baseline);
        let rule = rule_y + RULE_WIDTH / 2.0 <= l.word.bottom();

        for (i, &b) in self.word.iter().enumerate() {
            #[expect(
                clippy::cast_precision_loss,
                reason = "words are at most twenty letters; exact in f32"
            )]
            let x = (i as f32).mul_add(step, x0);
            let (ch, color) = match letter_index(b) {
                _ if self.is_guessed(b) => (b.to_ascii_uppercase() as char, GREEN),
                // A lost game shows the word it was hiding, in red.
                Some(_) if self.phase == GamePhase::Lost => (b.to_ascii_uppercase() as char, RED),
                Some(_) => ('_', OVERLAY0),
                None => (b as char, TEXT_COLOR),
            };
            // Each letter is bounded by its own cell, not by the row: a glyph
            // wider than the step -- which happens as soon as the step is
            // squeezed below the font size -- was centred to the *left* of its
            // cell and given the cell's width as a `max_width` measured from
            // an x it no longer started at.
            let cell = Rect::new(x, l.word.y, step, l.word.h);
            run_in(
                f,
                cell,
                &ch.to_string(),
                l.word_font,
                FontWeightHint::Bold,
                color,
                None,
            );
            if rule {
                f.push(RenderCommand::Line {
                    x1: step.mul_add(0.1, x),
                    y1: rule_y,
                    x2: step.mul_add(0.9, x),
                    y2: rule_y,
                    color: SURFACE0,
                    width: RULE_WIDTH,
                });
            }
        }
    }

    /// The alphabet, and -- new -- a hit box on every key of it.
    ///
    /// There is no `if l.key <= 0.0 { return; }` here. There was, and it was a
    /// second copy of the guard `Layout::key_rect` already holds -- so no test
    /// could reach one of the two, and the mutation harness found it by
    /// deleting the copy in `key_rect` and watching nothing fail (lesson 92).
    /// A window with no room for a keyboard yields no rectangles, and a loop
    /// over no rectangles draws nothing without being told to.
    fn draw_keyboard(&self, f: &mut Frame<Target>, l: &Layout) {
        for row in &KEY_ROWS {
            for &upper in *row {
                let lower = upper.to_ascii_lowercase();
                let Some(r) = l.key_rect(lower) else { continue };
                let (bg, fg) = if self.is_guessed(lower) {
                    if self.word.contains(&lower) {
                        (GREEN, BASE)
                    } else {
                        (RED, BASE)
                    }
                } else {
                    (SURFACE0, TEXT_COLOR)
                };
                fill(f, r, bg, CornerRadii::all(4.0));
                run_in(
                    f,
                    r,
                    &(upper as char).to_string(),
                    l.font,
                    FontWeightHint::Bold,
                    fg,
                    None,
                );
                // A key is pressable only while there is a guess to make.
                // Recording the box in every phase would leave the alphabet
                // live under the result overlay, where a click means nothing.
                if self.phase == GamePhase::Playing && !self.is_guessed(lower) {
                    f.hit(Target::Letter(lower), r);
                }
            }
        }
    }

    /// The statistics column, when the window paid for one.
    fn draw_stats(&self, f: &mut Frame<Target>, l: &Layout) {
        if l.stats.is_empty() {
            return;
        }
        fill(f, l.stats, MANTLE, CornerRadii::all(6.0));
        let x = l.stats.x + l.pad;
        let w = (l.stats.w - l.pad * 2.0).max(0.0);
        let step = l.small * 1.6;
        let mut y = l.stats.y + l.pad;

        if !column_line(
            f,
            l.stats,
            y,
            STATS_HEADING,
            l.font,
            FontWeightHint::Bold,
            LAVENDER,
            Some(l.pad),
        ) {
            return;
        }
        y += l.font * 1.8;

        for (line, color) in [
            (format!("Wins: {}", self.stats.wins), GREEN),
            (format!("Losses: {}", self.stats.losses), RED),
            (format!("Streak: {}", self.stats.current_streak), YELLOW),
            (format!("Best: {}", self.stats.best_streak), PEACH),
            (
                format!("Win Rate: {}%", self.stats.win_rate_percent()),
                TEAL,
            ),
            (format!("Games: {}", self.stats.total_games()), SUBTEXT0),
        ] {
            if !column_line(
                f,
                l.stats,
                y,
                &line,
                l.small,
                FontWeightHint::Regular,
                color,
                Some(l.pad),
            ) {
                return;
            }
            y += step;
        }

        y += l.pad * 0.5;
        if y + l.small * 2.0 > l.stats.bottom() {
            return;
        }
        f.push(RenderCommand::Line {
            x1: x,
            y1: y,
            x2: x + w,
            y2: y,
            color: SURFACE0,
            width: 1.0,
        });
        y += l.pad;

        if !column_line(
            f,
            l.stats,
            y,
            "Wrong:",
            l.small,
            FontWeightHint::Regular,
            OVERLAY0,
            Some(l.pad),
        ) {
            return;
        }
        y += step;
        let wrong = self.incorrect_letters();
        let (line, weight, color) = if wrong.is_empty() {
            (String::from("None yet"), FontWeightHint::Light, OVERLAY0)
        } else {
            (
                wrong
                    .iter()
                    .map(|&b| (b as char).to_ascii_uppercase().to_string())
                    .collect::<Vec<_>>()
                    .join(" "),
                FontWeightHint::Bold,
                RED,
            )
        };
        column_line(f, l.stats, y, &line, l.small, weight, color, Some(l.pad));
    }

    /// The end-of-round card, with the two things a player can do next drawn
    /// as buttons rather than described as keystrokes.
    fn draw_result(&self, f: &mut Frame<Target>, l: &Layout) {
        let over = Rect::new(
            l.gallows.x,
            l.gallows.y,
            l.gallows.w.max(l.word.w),
            (l.word.bottom() - l.gallows.y).max(0.0),
        );
        if over.w <= 0.0 || over.h <= 0.0 {
            return;
        }
        fill(f, over, Color::rgba(17, 17, 27, 200), CornerRadii::ZERO);

        let accent = if self.phase == GamePhase::Won {
            GREEN
        } else {
            RED
        };
        let title = if self.phase == GamePhase::Won {
            "YOU WIN!"
        } else {
            "GAME OVER"
        };
        let word_line = format!("Word: {}", self.word_string().to_uppercase());
        let streak_line = format!("Streak: {}", self.stats.current_streak);

        let btn_h = (l.font * 2.2).max(1.0);
        let box_w = [
            text::measure(title, l.title, FontWeightHint::Bold),
            text::measure(&word_line, l.font, FontWeightHint::Regular),
            text::measure(&streak_line, l.font, FontWeightHint::Regular),
            text::measure(PLAY_AGAIN_LABEL, l.small, FontWeightHint::Bold)
                + text::measure(MENU_LABEL, l.small, FontWeightHint::Bold)
                + l.pad * 4.0,
        ]
        .into_iter()
        .fold(0.0f32, f32::max)
            + l.pad * 4.0;
        let box_w = box_w.min(over.w);
        let box_h = l.title + l.font * 2.0 * 2.0 + btn_h + l.pad * 4.0;
        let box_h = box_h.min(over.h);
        let card = Rect::new(
            over.x + (over.w - box_w) / 2.0,
            over.y + (over.h - box_h) / 2.0,
            box_w,
            box_h,
        );
        fill(f, card, SURFACE0, CornerRadii::all(8.0));
        f.push(RenderCommand::StrokeRect {
            x: card.x,
            y: card.y,
            width: card.w,
            height: card.h,
            color: accent,
            line_width: 2.0,
            corner_radii: CornerRadii::all(8.0),
        });

        // The card's three lines are a column, not three unconditional
        // placements. `box_h` is `.min(over.h)`, so in any window that squeezes
        // the card the nominal stack of title, word, streak and buttons is
        // taller than the card it is stacked in -- and all three of these were
        // drawn regardless, over the keyboard below.
        let mut y = card.y + l.pad;
        for (line, size, weight, color, advance) in [
            (title, l.title, FontWeightHint::Bold, accent, l.title * 1.6),
            (
                word_line.as_str(),
                l.font,
                FontWeightHint::Regular,
                TEXT_COLOR,
                l.font * 1.8,
            ),
            (
                streak_line.as_str(),
                l.font,
                FontWeightHint::Regular,
                YELLOW,
                l.font * 1.8,
            ),
        ] {
            if !column_line(f, card, y, line, size, weight, color, None) {
                return;
            }
            y += advance;
        }

        let gap = l.pad;
        let bw = ((card.w - l.pad * 2.0 - gap) / 2.0).max(0.0);
        for (i, (label, target, color)) in [
            (PLAY_AGAIN_LABEL, Target::PlayAgain, GREEN),
            (MENU_LABEL, Target::Menu, SUBTEXT0),
        ]
        .into_iter()
        .enumerate()
        {
            #[expect(clippy::cast_precision_loss, reason = "two buttons; exact in f32")]
            let bx = (i as f32).mul_add(bw + gap, card.x + l.pad);
            let r = Rect::new(bx, y, bw, btn_h);
            if r.bottom() > card.bottom() || r.is_empty() {
                break;
            }
            fill(f, r, MANTLE, CornerRadii::all(4.0));
            run_in(f, r, label, l.small, FontWeightHint::Bold, color, None);
            f.hit(target, r);
        }
    }

    // -- Event handling -------------------------------------------------

    /// Handle one event, reporting whether it was used.
    ///
    /// The return value is what the window system needs in order to decide
    /// whether to pass the event on; the old signature returned nothing, so
    /// every key the game ignored still counted as handled.
    fn handle_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::Key(ke) if ke.pressed => self.handle_key(ke.key),
            Event::Mouse(me) => self.handle_mouse(me),
            _ => EventResult::Ignored,
        }
    }

    /// Resolve a click through the frame that was drawn for this window size.
    ///
    /// The hit boxes come from the drawing pass, so a button can only be
    /// clicked where it was actually painted. Before this the app had no
    /// mouse handling at all: the on-screen alphabet was a picture of a
    /// keyboard, not a keyboard.
    fn handle_mouse(&mut self, me: &MouseEvent) -> EventResult {
        if me.kind != MouseEventKind::Press(MouseButton::Left) {
            return EventResult::Ignored;
        }
        let (w, h) = self.size;
        let Some(target) = self.frame(w, h).hit_test(me.x, me.y) else {
            return EventResult::Ignored;
        };
        self.activate(target)
    }

    /// Act on a hit target, whichever way it was reached.
    fn activate(&mut self, target: Target) -> EventResult {
        match target {
            Target::Letter(letter) => {
                self.guess_letter(letter);
            }
            Target::Hint => {
                if !self.use_hint() {
                    return EventResult::Ignored;
                }
            }
            Target::Category(index) => {
                let Some(cat) = Category::from_index(index) else {
                    return EventResult::Ignored;
                };
                self.category_cursor = index;
                self.category = cat;
                self.start_from_category();
            }
            Target::Difficulty(diff) => {
                self.difficulty = diff;
            }
            Target::PlayAgain => {
                self.new_round();
            }
            Target::Menu => {
                self.phase = GamePhase::CategorySelect;
            }
        }
        EventResult::Consumed
    }

    fn handle_key(&mut self, key: Key) -> EventResult {
        match self.phase {
            GamePhase::CategorySelect => self.handle_category_key(key),
            GamePhase::Playing => self.handle_playing_key(key),
            GamePhase::Won | GamePhase::Lost => self.handle_result_key(key),
        }
    }

    fn handle_category_key(&mut self, key: Key) -> EventResult {
        let count = Category::ALL.len();
        match key {
            Key::Up => {
                self.category_cursor = match self.category_cursor.checked_sub(1) {
                    Some(prev) => prev,
                    None => count.saturating_sub(1),
                };
            }
            Key::Down => {
                self.category_cursor = self
                    .category_cursor
                    .saturating_add(1)
                    .checked_rem(count)
                    .unwrap_or(0);
            }
            Key::Enter => {
                if let Some(cat) = Category::from_index(self.category_cursor) {
                    self.category = cat;
                }
                self.start_from_category();
            }
            key => {
                let Some(diff) = difficulty_from_key(key) else {
                    return EventResult::Ignored;
                };
                self.difficulty = diff;
            }
        }
        EventResult::Consumed
    }

    fn handle_playing_key(&mut self, key: Key) -> EventResult {
        // A letter key guesses that letter -- except H, which asks for the
        // hint while the hint is still available and H itself unguessed.
        if let Some(letter) = key_to_letter(key) {
            if letter == b'h' && !self.hint_used && !self.is_guessed(b'h') {
                self.use_hint();
            } else {
                self.guess_letter(letter);
            }
            return EventResult::Consumed;
        }

        match key {
            Key::Escape => {
                self.phase = GamePhase::CategorySelect;
            }
            Key::Enter => {
                self.new_round();
            }
            _ => {
                let Some(diff) = difficulty_from_key(key) else {
                    return EventResult::Ignored;
                };
                self.difficulty = diff;
            }
        }
        EventResult::Consumed
    }

    fn handle_result_key(&mut self, key: Key) -> EventResult {
        match key {
            Key::Enter => {
                self.new_round();
            }
            Key::Escape => {
                self.phase = GamePhase::CategorySelect;
            }
            _ => {
                let Some(diff) = difficulty_from_key(key) else {
                    return EventResult::Ignored;
                };
                self.difficulty = diff;
            }
        }
        EventResult::Consumed
    }
}

// -- Utility functions --------------------------------------------------

/// Convert a byte to its 0-25 index (a=0, z=25). Returns None for non-letters.
fn letter_index(b: u8) -> Option<usize> {
    let lower = b.to_ascii_lowercase();
    if lower.is_ascii_lowercase() {
        lower.checked_sub(b'a').map(usize::from)
    } else {
        None
    }
}

/// Convert a Key enum variant to a lowercase ASCII letter byte.
fn key_to_letter(key: Key) -> Option<u8> {
    match key {
        Key::A => Some(b'a'),
        Key::B => Some(b'b'),
        Key::C => Some(b'c'),
        Key::D => Some(b'd'),
        Key::E => Some(b'e'),
        Key::F => Some(b'f'),
        Key::G => Some(b'g'),
        Key::H => Some(b'h'),
        Key::I => Some(b'i'),
        Key::J => Some(b'j'),
        Key::K => Some(b'k'),
        Key::L => Some(b'l'),
        Key::M => Some(b'm'),
        Key::N => Some(b'n'),
        Key::O => Some(b'o'),
        Key::P => Some(b'p'),
        Key::Q => Some(b'q'),
        Key::R => Some(b'r'),
        Key::S => Some(b's'),
        Key::T => Some(b't'),
        Key::U => Some(b'u'),
        Key::V => Some(b'v'),
        Key::W => Some(b'w'),
        Key::X => Some(b'x'),
        Key::Y => Some(b'y'),
        Key::Z => Some(b'z'),
        _ => None,
    }
}

/// Convert a Key to a difficulty level (1=Easy, 2=Medium, 3=Hard).
fn difficulty_from_key(key: Key) -> Option<Difficulty> {
    match key {
        Key::Num1 => Some(Difficulty::Easy),
        Key::Num2 => Some(Difficulty::Medium),
        Key::Num3 => Some(Difficulty::Hard),
        _ => None,
    }
}

// -- Window integration -------------------------------------------------

impl App for HangmanApp {
    fn title(&self) -> String {
        String::from("Hangman")
    }

    fn app_id(&self) -> String {
        String::from("hangman")
    }

    fn initial_size(&self) -> (u32, u32) {
        // The size the drawing was originally authored against; nothing in
        // the app depends on it any more, it is only where the window starts.
        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
    }

    fn on_event(&mut self, event: &Event) -> Response {
        if matches!(event, Event::CloseRequested) {
            return Response::Exit;
        }
        match self.handle_event(event) {
            EventResult::Consumed => Response::Redraw,
            EventResult::Ignored => Response::Idle,
        }
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        // The size the frame is drawn at is the size the next click is read
        // against, which is the only reason it is stored at all.
        self.resize(width, height);
        self.frame(width, height).into_tree()
    }
}

impl Probe for HangmanApp {
    type Target = Target;
    type Outcome = EventResult;

    const SIZE: (f32, f32) = (WINDOW_WIDTH, WINDOW_HEIGHT);

    fn draw(&self, size: (f32, f32)) -> Frame<Self::Target> {
        self.frame(size.0, size.1)
    }

    fn click_at(&mut self, x: f32, y: f32, button: MouseButton, size: (f32, f32)) -> Self::Outcome {
        self.resize(size.0, size.1);
        self.handle_event(&Event::Mouse(MouseEvent {
            x,
            y,
            kind: MouseEventKind::Press(button),
        }))
    }

    fn key_at(&mut self, key: &KeyEvent, size: (f32, f32)) -> Self::Outcome {
        self.resize(size.0, size.1);
        self.handle_event(&Event::Key(key.clone()))
    }

    fn scroll_at(
        &mut self,
        _x: f32,
        _y: f32,
        _dy: f32,
        _size: (f32, f32),
    ) -> Option<Self::Outcome> {
        // Nothing scrolls: every part of the game is sized to the window, and
        // the statistics column is dropped entirely when it will not fit.
        None
    }
}

fn main() -> ExitCode {
    let mut app = HangmanApp::new();
    app::launch("hangman", &mut app)
}

// =====================================================================
// Tests
// =====================================================================
#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "a test that cannot find what it is looking for should fail loudly"
)]
mod tests {
    use super::*;
    use guitk::probe::{click_sized, is_visible_sized, key, press, rect_of_sized};

    /// Helper to create a game with a fixed seed for deterministic tests.
    fn test_app() -> HangmanApp {
        HangmanApp::with_seed(12345)
    }

    // -- Test-only queries ----------------------------------------------
    //
    // These were methods on `HangmanApp` that only the tests ever called.
    // A query nothing draws is dead weight in the program and a hazard in
    // review -- `display_word` in particular was a second copy of the rule
    // `draw_word` owns, so the two could disagree about what a player sees
    // and only the copy nobody looks at would be tested (lesson 92).

    /// How many words the whole corpus holds.
    ///
    /// Derived from `Category::ALL`, not from a hand-written sum of the five
    /// arrays: a sixth category joins this total by existing.
    fn total_word_count() -> usize {
        Category::ALL.iter().map(|c| c.words().len()).sum()
    }

    /// The word as the blanks-and-letters string a player reads.
    fn display_word(app: &HangmanApp) -> String {
        let mut out = String::new();
        for (i, &b) in app.word.iter().enumerate() {
            if i > 0 {
                out.push(' ');
            }
            match letter_index(b) {
                Some(_) if app.is_guessed(b) => out.push(b as char),
                Some(_) => out.push('_'),
                None => out.push(b as char),
            }
        }
        out
    }

    /// The guessed letters that are in the word.
    fn correct_letters(app: &HangmanApp) -> Vec<u8> {
        (b'a'..=b'z')
            .filter(|&c| app.is_guessed(c) && app.word.contains(&c))
            .collect()
    }

    /// How many of the guessed letters are in the word.
    fn correct_count(app: &HangmanApp) -> usize {
        correct_letters(app).len()
    }

    /// How many distinct letters have been guessed, right or wrong.
    fn total_guessed(app: &HangmanApp) -> usize {
        app.guessed.iter().filter(|&&g| g).count()
    }

    /// Helper to create a playing-state game with a known word.
    fn playing_app(word: &str) -> HangmanApp {
        let mut app = test_app();
        app.word = word.as_bytes().to_vec();
        app.guessed = [false; 26];
        app.wrong_count = 0;
        app.hint_used = false;
        app.phase = GamePhase::Playing;
        app
    }

    // -- Construction & initialization ----------------------------------

    #[test]
    fn test_initial_phase_is_category_select() {
        let app = test_app();
        assert_eq!(app.phase, GamePhase::CategorySelect);
    }

    #[test]
    fn test_initial_category_is_animals() {
        let app = test_app();
        assert_eq!(app.category, Category::Animals);
    }

    #[test]
    fn test_initial_difficulty_is_medium() {
        let app = test_app();
        assert_eq!(app.difficulty, Difficulty::Medium);
    }

    #[test]
    fn test_initial_wrong_count_zero() {
        let app = test_app();
        assert_eq!(app.wrong_count, 0);
    }

    #[test]
    fn test_initial_hint_not_used() {
        let app = test_app();
        assert!(!app.hint_used);
    }

    #[test]
    fn test_initial_stats_clean() {
        let app = test_app();
        assert_eq!(app.stats.wins, 0);
        assert_eq!(app.stats.losses, 0);
        assert_eq!(app.stats.current_streak, 0);
        assert_eq!(app.stats.best_streak, 0);
    }

    #[test]
    fn test_initial_word_not_empty() {
        let app = test_app();
        assert!(!app.word.is_empty());
    }

    #[test]
    fn test_initial_category_cursor_zero() {
        let app = test_app();
        assert_eq!(app.category_cursor, 0);
    }

    #[test]
    fn test_new_creates_valid_state() {
        let app = HangmanApp::new();
        assert!(!app.word.is_empty());
        assert_eq!(app.wrong_count, 0);
    }

    // -- Word list validation -------------------------------------------

    #[test]
    fn test_total_word_count_over_100() {
        assert!(total_word_count() >= 100);
    }

    #[test]
    fn test_animals_count() {
        assert!(ANIMALS.len() >= 20);
    }

    #[test]
    fn test_fruits_count() {
        assert!(FRUITS.len() >= 20);
    }

    #[test]
    fn test_countries_count() {
        assert!(COUNTRIES.len() >= 20);
    }

    #[test]
    fn test_sports_count() {
        assert!(SPORTS.len() >= 20);
    }

    #[test]
    fn test_technology_count() {
        assert!(TECHNOLOGY.len() >= 20);
    }

    #[test]
    fn test_all_words_lowercase() {
        for cat in &Category::ALL {
            for word in cat.words() {
                assert!(
                    word.chars().all(|c| c.is_ascii_lowercase()),
                    "Word '{}' in {:?} is not all lowercase",
                    word,
                    cat
                );
            }
        }
    }

    #[test]
    fn test_all_words_non_empty() {
        for cat in &Category::ALL {
            for word in cat.words() {
                assert!(!word.is_empty(), "{:?} contains empty word", cat);
            }
        }
    }

    #[test]
    fn test_all_words_no_duplicates() {
        let mut all_words: Vec<&str> = Vec::new();
        for cat in &Category::ALL {
            for word in cat.words() {
                assert!(!all_words.contains(word), "Duplicate word: {}", word);
                all_words.push(word);
            }
        }
    }

    // -- Category -------------------------------------------------------

    #[test]
    fn test_category_all_has_five() {
        assert_eq!(Category::ALL.len(), 5);
    }

    #[test]
    fn test_category_labels() {
        assert_eq!(Category::Animals.label(), "Animals");
        assert_eq!(Category::Fruits.label(), "Fruits");
        assert_eq!(Category::Countries.label(), "Countries");
        assert_eq!(Category::Sports.label(), "Sports");
        assert_eq!(Category::Technology.label(), "Technology");
    }

    #[test]
    fn test_category_from_index() {
        assert_eq!(Category::from_index(0), Some(Category::Animals));
        assert_eq!(Category::from_index(4), Some(Category::Technology));
        assert_eq!(Category::from_index(5), None);
    }

    #[test]
    fn test_category_words_returns_correct_array() {
        assert_eq!(Category::Animals.words().len(), ANIMALS.len());
        assert_eq!(Category::Technology.words().len(), TECHNOLOGY.len());
    }

    // -- Difficulty -----------------------------------------------------

    #[test]
    fn test_difficulty_labels() {
        assert_eq!(Difficulty::Easy.label(), "Easy");
        assert_eq!(Difficulty::Medium.label(), "Medium");
        assert_eq!(Difficulty::Hard.label(), "Hard");
    }

    #[test]
    fn test_difficulty_length_ranges() {
        // Easy words are shorter.
        assert!(Difficulty::Easy.max_length() <= Difficulty::Medium.max_length());
        assert!(Difficulty::Medium.min_length() >= Difficulty::Easy.min_length());
    }

    #[test]
    fn test_easy_has_free_reveals() {
        assert!(Difficulty::Easy.free_reveals() > 0);
    }

    #[test]
    fn test_hard_has_no_free_reveals() {
        assert_eq!(Difficulty::Hard.free_reveals(), 0);
    }

    #[test]
    fn test_medium_has_one_free_reveal() {
        assert_eq!(Difficulty::Medium.free_reveals(), 1);
    }

    // -- Seeding and the reveal -----------------------------------------

    // The generator's own contract -- determinism under a seed, divergence
    // under two, staying inside its bound -- used to be tested here against the
    // local `Lcg`. It is now tested once, against the shared implementation, in
    // `randrange`. Sixteen crates each testing their own copy is sixteen
    // chances to test a copy that has quietly drifted from the one being
    // shipped. What replaces those tests is about the game.

    /// A fresh game must take its seed from the kernel, not from a literal.
    ///
    /// Phrased as "which seed", not as "two fresh games differ", because a host
    /// test build has no SlateOS kernel: `seed_from_system` correctly takes its
    /// fallback and two fresh games are then identical, exactly as they were
    /// under the old hardcoded `42`. A variety check would therefore pass on
    /// the broken code and fail on the fixed code, which is backwards.
    #[cfg(not(unix))]
    #[test]
    fn a_fresh_game_is_seeded_by_the_system_and_not_by_a_literal() {
        let fresh = HangmanApp::new().word;
        assert_eq!(
            fresh,
            HangmanApp::with_seed(FALLBACK_SEED).word,
            "a fresh game did not use the crate's fallback seed"
        );
        assert_ne!(
            fresh,
            HangmanApp::with_seed(42).word,
            "a fresh game is still seeded by the old hardcoded literal"
        );
    }

    /// Which letter a free reveal picks must not be a short fixed cycle.
    ///
    /// This is the property the old reduction destroyed, and the one place in
    /// this crate where it bit. Picking the word escaped it, because the word
    /// lists are odd lengths and an odd bound's remainder depends on all 64
    /// bits. The reveal draws against `unrevealed.len()` instead -- the number
    /// of *distinct* letters still hidden -- and for ordinary English words
    /// that is very often even, and often a power of two, which is exactly
    /// where `val % bound` reads the generator's low bits. Those are a counter.
    ///
    /// The variation must be looked for **along one generator's stream**, not
    /// across seeds: different seeds have different low bits, so sampling 200
    /// fresh games hides the defect completely -- every letter gets reached and
    /// the test passes on broken code. Consecutive rounds of a single game
    /// share one generator, which is where the counter shows.
    ///
    /// Measured on the old reduction, one game, first pick of each round, on a
    /// four-distinct-letter word: at Easy the index ran 2, 0, 2, 0, 2, 0 for
    /// ever -- two of the four letters could never be revealed first -- and at
    /// Medium it ran 2, 1, 0, 3, 2, 1, 0, 3, a perfect four-cycle. Even a
    /// six-letter word only ever produced *even* indices, since an even bound
    /// inherits the low bit's period of 2.
    ///
    /// So the assertion is about period, not coverage: no cycle of length 4 or
    /// less may reproduce the whole sequence.
    ///
    /// Medium is the case that fails on the old code, and it fails outright --
    /// one letter revealed per round means the round's outcome *is* the draw,
    /// so the sequence is the bare four-cycle. Easy is kept as well even though
    /// the old code survives it: its first draw is the periodic one but its
    /// second is against a bound of 3, whose remainder depends on all 64 bits,
    /// and that second draw is enough to keep the round outcomes from repeating
    /// exactly. It still pins the property down for future edits.
    #[test]
    fn a_free_reveal_is_not_a_short_fixed_cycle() {
        for difficulty in [Difficulty::Easy, Difficulty::Medium] {
            let rounds = reveal_signatures(difficulty, 40);
            for period in 1..=4usize {
                let repeats = rounds
                    .iter()
                    .enumerate()
                    .all(|(i, v)| rounds.get(i % period).is_some_and(|f| f == v));
                assert!(
                    !repeats,
                    "at {difficulty:?} the free reveal repeats with period \
                     {period}: {rounds:?}"
                );
            }
        }
    }

    /// Play `rounds` consecutive rounds on one fixed word from one generator,
    /// returning which distinct letters of the word were revealed each round.
    ///
    /// The word is forced so that the variety being measured is the reveal's
    /// and not the word pick's, and the rounds share one app -- and therefore
    /// one generator -- because that is the only place the defect is visible.
    fn reveal_signatures(difficulty: Difficulty, rounds: usize) -> Vec<Vec<usize>> {
        const WORD: &[u8] = b"TESTER";
        let mut app = HangmanApp::with_seed(7);
        app.difficulty = difficulty;
        let mut distinct: Vec<u8> = Vec::new();
        for &b in WORD {
            if !distinct.contains(&b) {
                distinct.push(b);
            }
        }
        (0..rounds)
            .map(|_| {
                app.word = WORD.to_vec();
                app.guessed = [false; 26];
                app.apply_free_reveals();
                distinct
                    .iter()
                    .enumerate()
                    .filter(|&(_, &b)| letter_index(b).is_some_and(|i| app.guessed[i]))
                    .map(|(n, _)| n)
                    .collect()
            })
            .collect()
    }

    // -- letter_index ---------------------------------------------------

    #[test]
    fn test_letter_index_lowercase() {
        assert_eq!(letter_index(b'a'), Some(0));
        assert_eq!(letter_index(b'z'), Some(25));
        assert_eq!(letter_index(b'm'), Some(12));
    }

    #[test]
    fn test_letter_index_uppercase() {
        assert_eq!(letter_index(b'A'), Some(0));
        assert_eq!(letter_index(b'Z'), Some(25));
    }

    #[test]
    fn test_letter_index_non_letter() {
        assert_eq!(letter_index(b'1'), None);
        assert_eq!(letter_index(b' '), None);
        assert_eq!(letter_index(b'-'), None);
    }

    // -- key_to_letter --------------------------------------------------

    #[test]
    fn test_key_to_letter_a() {
        assert_eq!(key_to_letter(Key::A), Some(b'a'));
    }

    #[test]
    fn test_key_to_letter_z() {
        assert_eq!(key_to_letter(Key::Z), Some(b'z'));
    }

    #[test]
    fn test_key_to_letter_non_letter() {
        assert_eq!(key_to_letter(Key::Enter), None);
        assert_eq!(key_to_letter(Key::Space), None);
        assert_eq!(key_to_letter(Key::Escape), None);
    }

    // -- difficulty_from_key --------------------------------------------

    #[test]
    fn test_difficulty_from_key_digits() {
        assert_eq!(difficulty_from_key(Key::Num1), Some(Difficulty::Easy));
        assert_eq!(difficulty_from_key(Key::Num2), Some(Difficulty::Medium));
        assert_eq!(difficulty_from_key(Key::Num3), Some(Difficulty::Hard));
    }

    #[test]
    fn test_difficulty_from_key_other() {
        assert_eq!(difficulty_from_key(Key::A), None);
        assert_eq!(difficulty_from_key(Key::Enter), None);
    }

    // -- Guessing logic -------------------------------------------------

    #[test]
    fn test_guess_correct_letter() {
        let mut app = playing_app("cat");
        let result = app.guess_letter(b'c');
        assert!(result);
        assert_eq!(app.wrong_count, 0);
    }

    #[test]
    fn test_guess_wrong_letter() {
        let mut app = playing_app("cat");
        let result = app.guess_letter(b'x');
        assert!(result);
        assert_eq!(app.wrong_count, 1);
    }

    #[test]
    fn test_guess_duplicate_rejected() {
        let mut app = playing_app("cat");
        app.guess_letter(b'c');
        let result = app.guess_letter(b'c');
        assert!(!result);
    }

    #[test]
    fn test_guess_in_wrong_phase() {
        let mut app = playing_app("cat");
        app.phase = GamePhase::Won;
        let result = app.guess_letter(b'a');
        assert!(!result);
    }

    #[test]
    fn test_win_detection() {
        let mut app = playing_app("cat");
        app.guess_letter(b'c');
        app.guess_letter(b'a');
        app.guess_letter(b't');
        assert_eq!(app.phase, GamePhase::Won);
    }

    #[test]
    fn test_loss_detection() {
        let mut app = playing_app("cat");
        for &letter in b"xyzqwe" {
            app.guess_letter(letter);
        }
        assert_eq!(app.phase, GamePhase::Lost);
        assert_eq!(app.wrong_count, MAX_WRONG);
    }

    #[test]
    fn test_win_increments_stats() {
        let mut app = playing_app("cat");
        app.guess_letter(b'c');
        app.guess_letter(b'a');
        app.guess_letter(b't');
        assert_eq!(app.stats.wins, 1);
        assert_eq!(app.stats.current_streak, 1);
    }

    #[test]
    fn test_loss_increments_stats() {
        let mut app = playing_app("cat");
        for &letter in b"xyzqwe" {
            app.guess_letter(letter);
        }
        assert_eq!(app.stats.losses, 1);
        assert_eq!(app.stats.current_streak, 0);
    }

    // -- Word display ---------------------------------------------------

    #[test]
    fn test_display_word_all_blanks() {
        let app = playing_app("cat");
        assert_eq!(display_word(&app), "_ _ _");
    }

    #[test]
    fn test_display_word_partial() {
        let mut app = playing_app("cat");
        app.guess_letter(b'c');
        assert_eq!(display_word(&app), "c _ _");
    }

    #[test]
    fn test_display_word_full() {
        let mut app = playing_app("cat");
        app.guess_letter(b'c');
        app.guess_letter(b'a');
        app.guess_letter(b't');
        assert_eq!(display_word(&app), "c a t");
    }

    #[test]
    fn test_word_string() {
        let app = playing_app("dolphin");
        assert_eq!(app.word_string(), "dolphin");
    }

    // -- Incorrect / correct letters ------------------------------------

    #[test]
    fn test_incorrect_letters_empty() {
        let app = playing_app("cat");
        assert!(app.incorrect_letters().is_empty());
    }

    #[test]
    fn test_incorrect_letters_tracked() {
        let mut app = playing_app("cat");
        app.guess_letter(b'x');
        app.guess_letter(b'z');
        let wrong = app.incorrect_letters();
        assert_eq!(wrong.len(), 2);
        assert!(wrong.contains(&b'x'));
        assert!(wrong.contains(&b'z'));
    }

    #[test]
    fn test_correct_letters_tracked() {
        let mut app = playing_app("cat");
        app.guess_letter(b'c');
        app.guess_letter(b'x');
        let correct = correct_letters(&app);
        assert_eq!(correct.len(), 1);
        assert!(correct.contains(&b'c'));
    }

    // -- Hint system ----------------------------------------------------

    #[test]
    fn test_hint_reveals_letter() {
        let mut app = playing_app("cat");
        let before = total_guessed(&app);
        app.use_hint();
        assert!(total_guessed(&app) > before);
    }

    #[test]
    fn test_hint_can_only_be_used_once() {
        let mut app = playing_app("cat");
        assert!(app.use_hint());
        assert!(!app.use_hint());
    }

    #[test]
    fn test_hint_sets_flag() {
        let mut app = playing_app("cat");
        app.use_hint();
        assert!(app.hint_used);
    }

    #[test]
    fn test_hint_not_available_when_won() {
        let mut app = playing_app("cat");
        app.phase = GamePhase::Won;
        assert!(!app.use_hint());
    }

    #[test]
    fn test_hint_not_available_when_lost() {
        let mut app = playing_app("cat");
        app.phase = GamePhase::Lost;
        assert!(!app.use_hint());
    }

    // -- Remaining guesses ----------------------------------------------

    #[test]
    fn test_remaining_guesses_initial() {
        let app = playing_app("cat");
        assert_eq!(app.remaining_guesses(), MAX_WRONG);
    }

    #[test]
    fn test_remaining_guesses_after_wrong() {
        let mut app = playing_app("cat");
        app.guess_letter(b'x');
        assert_eq!(app.remaining_guesses(), MAX_WRONG - 1);
    }

    #[test]
    fn test_remaining_guesses_correct_no_change() {
        let mut app = playing_app("cat");
        app.guess_letter(b'c');
        assert_eq!(app.remaining_guesses(), MAX_WRONG);
    }

    // -- Stats ----------------------------------------------------------

    #[test]
    fn test_stats_record_win() {
        let mut stats = Stats::new();
        stats.record_win();
        assert_eq!(stats.wins, 1);
        assert_eq!(stats.current_streak, 1);
        assert_eq!(stats.best_streak, 1);
    }

    #[test]
    fn test_stats_record_loss_resets_streak() {
        let mut stats = Stats::new();
        stats.record_win();
        stats.record_win();
        stats.record_loss();
        assert_eq!(stats.current_streak, 0);
        assert_eq!(stats.best_streak, 2);
    }

    #[test]
    fn test_stats_best_streak_preserved() {
        let mut stats = Stats::new();
        stats.record_win();
        stats.record_win();
        stats.record_win();
        stats.record_loss();
        stats.record_win();
        assert_eq!(stats.best_streak, 3);
        assert_eq!(stats.current_streak, 1);
    }

    #[test]
    fn test_stats_total_games() {
        let mut stats = Stats::new();
        stats.record_win();
        stats.record_loss();
        assert_eq!(stats.total_games(), 2);
    }

    #[test]
    fn test_stats_win_rate_zero() {
        let stats = Stats::new();
        assert_eq!(stats.win_rate_percent(), 0);
    }

    #[test]
    fn test_stats_win_rate_100() {
        let mut stats = Stats::new();
        stats.record_win();
        assert_eq!(stats.win_rate_percent(), 100);
    }

    #[test]
    fn test_stats_win_rate_50() {
        let mut stats = Stats::new();
        stats.record_win();
        stats.record_loss();
        assert_eq!(stats.win_rate_percent(), 50);
    }

    // -- New round / restart --------------------------------------------

    #[test]
    fn test_new_round_resets_guesses() {
        let mut app = playing_app("cat");
        app.guess_letter(b'c');
        app.guess_letter(b'x');
        app.new_round();
        assert_eq!(app.wrong_count, 0);
        assert_eq!(total_guessed(&app), app.difficulty.free_reveals().min(26));
    }

    #[test]
    fn test_new_round_preserves_stats() {
        let mut app = playing_app("cat");
        app.stats.record_win();
        app.new_round();
        assert_eq!(app.stats.wins, 1);
    }

    #[test]
    fn test_new_round_sets_playing() {
        let mut app = playing_app("cat");
        app.phase = GamePhase::Won;
        app.new_round();
        assert_eq!(app.phase, GamePhase::Playing);
    }

    #[test]
    fn test_new_round_resets_hint() {
        let mut app = playing_app("cat");
        app.hint_used = true;
        app.new_round();
        assert!(!app.hint_used);
    }

    // -- Category selection keys ----------------------------------------

    #[test]
    fn test_category_down_key() {
        let mut app = test_app();
        app.handle_key(Key::Down);
        assert_eq!(app.category_cursor, 1);
    }

    #[test]
    fn test_category_up_key_wraps() {
        let mut app = test_app();
        app.handle_key(Key::Up);
        assert_eq!(app.category_cursor, 4);
    }

    #[test]
    fn test_category_down_wraps() {
        let mut app = test_app();
        app.category_cursor = 4;
        app.handle_key(Key::Down);
        assert_eq!(app.category_cursor, 0);
    }

    #[test]
    fn test_category_enter_starts_game() {
        let mut app = test_app();
        app.handle_key(Key::Enter);
        assert_eq!(app.phase, GamePhase::Playing);
    }

    #[test]
    fn test_category_enter_selects_category() {
        let mut app = test_app();
        app.category_cursor = 2;
        app.handle_key(Key::Enter);
        assert_eq!(app.category, Category::Countries);
    }

    #[test]
    fn test_category_difficulty_change() {
        let mut app = test_app();
        app.handle_key(Key::Num1);
        assert_eq!(app.difficulty, Difficulty::Easy);
        app.handle_key(Key::Num3);
        assert_eq!(app.difficulty, Difficulty::Hard);
    }

    // -- Playing keys ---------------------------------------------------

    #[test]
    fn test_playing_letter_key() {
        let mut app = playing_app("cat");
        app.handle_key(Key::C);
        assert!(app.is_guessed(b'c'));
    }

    #[test]
    fn test_playing_escape_to_category() {
        let mut app = playing_app("cat");
        app.handle_key(Key::Escape);
        assert_eq!(app.phase, GamePhase::CategorySelect);
    }

    #[test]
    fn test_playing_enter_new_round() {
        let mut app = playing_app("cat");
        app.guess_letter(b'c');
        app.handle_key(Key::Enter);
        assert_eq!(app.phase, GamePhase::Playing);
        assert_eq!(app.wrong_count, 0);
    }

    // -- Result keys ----------------------------------------------------

    #[test]
    fn test_result_enter_new_round() {
        let mut app = playing_app("cat");
        app.phase = GamePhase::Won;
        app.handle_key(Key::Enter);
        assert_eq!(app.phase, GamePhase::Playing);
    }

    #[test]
    fn test_result_escape_to_category() {
        let mut app = playing_app("cat");
        app.phase = GamePhase::Lost;
        app.handle_key(Key::Escape);
        assert_eq!(app.phase, GamePhase::CategorySelect);
    }

    // -- Event handling -------------------------------------------------

    #[test]
    fn test_handle_event_key_press() {
        let mut app = playing_app("cat");
        assert_eq!(
            app.handle_event(&Event::Key(press(Key::C))),
            EventResult::Consumed
        );
        assert!(app.is_guessed(b'c'));
    }

    #[test]
    fn test_handle_event_key_release_ignored() {
        let mut app = playing_app("cat");
        let mut release = press(Key::C);
        release.pressed = false;
        assert_eq!(
            app.handle_event(&Event::Key(release)),
            EventResult::Ignored,
            "a key that is only being let go of is not a guess"
        );
        assert!(!app.is_guessed(b'c'));
    }

    // -- is_word_revealed -----------------------------------------------

    #[test]
    fn test_word_not_revealed_initially() {
        let app = playing_app("cat");
        assert!(!app.is_word_revealed());
    }

    #[test]
    fn test_word_revealed_after_all_guessed() {
        let mut app = playing_app("cat");
        app.guess_letter(b'c');
        app.guess_letter(b'a');
        app.guess_letter(b't');
        assert!(app.is_word_revealed());
    }

    #[test]
    fn test_word_with_duplicate_letters() {
        let mut app = playing_app("banana");
        app.guess_letter(b'b');
        app.guess_letter(b'a');
        app.guess_letter(b'n');
        assert!(app.is_word_revealed());
    }

    // -- Rendering smoke tests ------------------------------------------
    //
    // These asked `render()` for a command list and checked it was not empty.
    // They now go through the frame, which is what a window and a click both
    // see; "not empty" is kept as the floor and the interesting checks live
    // in the wiring section below.

    /// The frame every test that does not care about size draws against.
    fn frame_at(app: &HangmanApp, size: (f32, f32)) -> Frame<Target> {
        app.draw(size)
    }

    #[test]
    fn test_render_category_select() {
        let app = test_app();
        assert!(!frame_at(&app, HangmanApp::SIZE).commands().is_empty());
    }

    #[test]
    fn test_render_playing() {
        let mut app = playing_app("cat");
        app.phase = GamePhase::Playing;
        assert!(!frame_at(&app, HangmanApp::SIZE).commands().is_empty());
    }

    #[test]
    fn test_render_won() {
        let mut app = playing_app("cat");
        app.phase = GamePhase::Won;
        assert!(!frame_at(&app, HangmanApp::SIZE).commands().is_empty());
    }

    #[test]
    fn test_render_lost() {
        let mut app = playing_app("cat");
        app.phase = GamePhase::Lost;
        assert!(!frame_at(&app, HangmanApp::SIZE).commands().is_empty());
    }

    #[test]
    fn test_render_with_guesses() {
        let mut app = playing_app("cat");
        app.guess_letter(b'c');
        app.guess_letter(b'x');
        assert!(frame_at(&app, HangmanApp::SIZE).commands().len() > 10);
    }

    #[test]
    fn the_figure_gains_a_part_for_every_wrong_guess() {
        // Counting lines rather than commands: the old test compared total
        // command counts, which a change anywhere else in the frame would
        // have moved just as well.
        let mut app = playing_app("zzz");
        let mut last = 0;
        for wrong in 0..=MAX_WRONG {
            app.wrong_count = wrong;
            let lines = frame_at(&app, HangmanApp::SIZE)
                .commands()
                .iter()
                .filter(|c| matches!(c, RenderCommand::Line { .. }))
                .count();
            if wrong > 0 {
                assert!(
                    lines > last,
                    "wrong guess {wrong} drew no more of the figure than {} did",
                    wrong - 1
                );
            }
            last = lines;
        }
    }

    #[test]
    fn the_figure_stops_at_the_last_part_it_has() {
        // One more wrong guess than the rules allow must not index past the
        // end of `FIGURE_PARTS`; the game is already lost by then.
        let mut app = playing_app("zzz");
        app.wrong_count = MAX_WRONG;
        let at_limit = frame_at(&app, HangmanApp::SIZE).commands().len();
        app.wrong_count = MAX_WRONG + 5;
        assert_eq!(
            frame_at(&app, HangmanApp::SIZE).commands().len(),
            at_limit,
            "the figure grew past the number of parts it has"
        );
    }

    // -- Free reveals ---------------------------------------------------

    #[test]
    fn test_easy_gives_free_reveals() {
        let mut app = test_app();
        app.difficulty = Difficulty::Easy;
        app.guessed = [false; 26];
        app.word = b"cat".to_vec();
        app.apply_free_reveals();
        // Easy gives 2 free reveals.
        let revealed = total_guessed(&app);
        assert_eq!(revealed, 2);
    }

    #[test]
    fn test_hard_no_free_reveals() {
        let mut app = test_app();
        app.difficulty = Difficulty::Hard;
        app.guessed = [false; 26];
        app.word = b"cat".to_vec();
        app.apply_free_reveals();
        assert_eq!(total_guessed(&app), 0);
    }

    // -- Correct/incorrect count ----------------------------------------

    #[test]
    fn test_correct_count() {
        let mut app = playing_app("cat");
        app.guess_letter(b'c');
        app.guess_letter(b'x');
        assert_eq!(correct_count(&app), 1);
    }

    #[test]
    fn test_total_guessed() {
        let mut app = playing_app("cat");
        app.guess_letter(b'c');
        app.guess_letter(b'x');
        assert_eq!(total_guessed(&app), 2);
    }

    // -- H key as hint --------------------------------------------------

    #[test]
    fn test_h_key_triggers_hint() {
        let mut app = playing_app("cat");
        app.handle_key(Key::H);
        assert!(app.hint_used);
    }

    #[test]
    fn test_h_key_after_hint_used_guesses_h() {
        let mut app = playing_app("hat");
        app.hint_used = true;
        app.handle_key(Key::H);
        assert!(app.is_guessed(b'h'));
    }

    // -- Pick word respects difficulty ----------------------------------

    #[test]
    fn test_pick_word_easy_short() {
        let mut app = test_app();
        app.difficulty = Difficulty::Easy;
        app.category = Category::Animals;
        for _ in 0..20 {
            app.guessed = [false; 26];
            app.pick_word();
            assert!(
                app.word.len() <= Difficulty::Easy.max_length(),
                "Easy word '{}' too long (len {})",
                app.word_string(),
                app.word.len()
            );
        }
    }

    #[test]
    fn test_pick_word_hard_long() {
        let mut app = test_app();
        app.difficulty = Difficulty::Hard;
        app.category = Category::Technology;
        for _ in 0..20 {
            app.guessed = [false; 26];
            app.pick_word();
            assert!(
                app.word.len() >= Difficulty::Hard.min_length(),
                "Hard word '{}' too short (len {})",
                app.word_string(),
                app.word.len()
            );
        }
    }

    // -- Misc edge cases ------------------------------------------------

    #[test]
    fn test_guess_non_letter() {
        let mut app = playing_app("cat");
        let result = app.guess_letter(b'1');
        assert!(!result);
    }

    #[test]
    fn test_max_wrong_equals_six() {
        assert_eq!(MAX_WRONG, 6);
    }

    #[test]
    fn test_display_word_repeated_letters() {
        let mut app = playing_app("banana");
        app.guess_letter(b'a');
        // All 'a's should be revealed.
        assert_eq!(display_word(&app), "_ a _ a _ a");
    }

    #[test]
    fn test_start_from_category() {
        let mut app = test_app();
        app.phase = GamePhase::CategorySelect;
        app.category = Category::Sports;
        app.start_from_category();
        assert_eq!(app.phase, GamePhase::Playing);
    }

    #[test]
    fn test_category_color_unique() {
        let colors: Vec<Color> = Category::ALL.iter().map(|c| c.color()).collect();
        for i in 0..colors.len() {
            for j in (i + 1)..colors.len() {
                assert_ne!(
                    colors[i], colors[j],
                    "Categories {} and {} share a color",
                    i, j
                );
            }
        }
    }

    #[test]
    fn test_hint_on_fully_revealed_word() {
        let mut app = playing_app("cat");
        app.guess_letter(b'c');
        app.guess_letter(b'a');
        app.guess_letter(b't');
        // Word is fully revealed; hint should do nothing.
        assert!(!app.use_hint());
    }

    // -- Window wiring ---------------------------------------------------
    //
    // Everything below this line is about the half of the program that did
    // not exist: `main` was `let _app = HangmanApp::new();`, `render` took no
    // size and painted a fixed 740x560 picture, and there was no mouse
    // handler at all -- the on-screen alphabet was a diagram of a keyboard,
    // not a keyboard.

    /// A spread of window sizes, from smaller than anything sane up to a
    /// large one. Every layout test sweeps these rather than checking the
    /// one size the drawing was authored at.
    const SIZES: [(f32, f32); 9] = [
        (240.0, 200.0),
        (320.0, 240.0),
        (480.0, 320.0),
        (640.0, 480.0),
        (740.0, 560.0),
        (900.0, 600.0),
        (1280.0, 720.0),
        (1920.0, 1080.0),
        (600.0, 1000.0),
    ];

    /// Every letter of the alphabet, lowercase.
    fn alphabet() -> impl Iterator<Item = u8> {
        b'a'..=b'z'
    }

    /// A playing app whose word is `word`, drawn at `size`.
    fn playing_at(word: &str, size: (f32, f32)) -> HangmanApp {
        let mut app = playing_app(word);
        app.resize(size.0, size.1);
        app
    }

    #[test]
    fn the_layout_follows_the_window_rather_than_a_constant() {
        // The whole point of the rewrite: two different windows must not
        // produce the same geometry. Below the key-size cap the keyboard
        // tracks the window too, which is the case that matters -- a small
        // window is where a fixed layout does the real damage.
        let a = Layout::solve(320.0, 240.0);
        let b = Layout::solve(740.0, 560.0);
        assert!(
            (a.keyboard.w - b.keyboard.w).abs() > 1.0,
            "the keyboard was the same width in a 320 and a 740 window"
        );
        assert!(
            (a.gallows.w - b.gallows.w).abs() > 1.0,
            "the gallows was the same width in a 320 and a 740 window"
        );
        assert!(
            (a.font - b.font).abs() > 0.5,
            "the type was the same size in a 240-tall and a 560-tall window"
        );
    }

    #[test]
    fn the_keys_stop_growing_before_they_become_billboards() {
        // A 4K window should not get a keyboard of 200px keys. The cap is
        // deliberate, and it is why the keyboard is centred rather than
        // stretched -- worth a test of its own so a later change to the cap
        // is a decision rather than an accident.
        let l = Layout::solve(3840.0, 2160.0);
        assert!(l.key <= 34.0, "a key grew to {} px", l.key);
        let centre_error = (l.keyboard.x + l.keyboard.w / 2.0 - 3840.0 / 2.0).abs();
        assert!(
            centre_error < 1.0,
            "the keyboard sat {centre_error} px off centre in a window far wider than it"
        );
    }

    #[test]
    fn every_part_of_the_layout_stays_inside_the_window() {
        for (w, h) in SIZES {
            let l = Layout::solve(w, h);
            for (name, r) in [
                ("header", l.header),
                ("gallows", l.gallows),
                ("word", l.word),
                ("keyboard", l.keyboard),
                ("stats", l.stats),
            ] {
                if r.is_empty() {
                    continue;
                }
                assert!(
                    r.x >= -0.5 && r.y >= -0.5 && r.right() <= w + 0.5 && r.bottom() <= h + 0.5,
                    "{name} {r:?} leaves a {w}x{h} window"
                );
            }
        }
    }

    #[test]
    fn the_parts_of_the_layout_do_not_sit_on_top_of_each_other() {
        for (w, h) in SIZES {
            let l = Layout::solve(w, h);
            let bands = [
                ("header", l.header),
                ("gallows", l.gallows),
                ("word", l.word),
                ("keyboard", l.keyboard),
            ];
            for pair in bands.windows(2) {
                let [(above, a), (below, b)] = [pair[0], pair[1]];
                if a.is_empty() || b.is_empty() {
                    continue;
                }
                assert!(
                    a.bottom() <= b.y + 0.5,
                    "{above} {a:?} runs into {below} {b:?} at {w}x{h}"
                );
            }
            if !l.stats.is_empty() {
                assert!(
                    l.gallows.right() <= l.stats.x + 0.5,
                    "the gallows runs into the statistics column at {w}x{h}"
                );
            }
        }
    }

    #[test]
    fn the_statistics_column_is_dropped_rather_than_squeezed() {
        // The column is the one part the game can be played without, so a
        // window too narrow for it loses it whole. What must never happen is
        // a column narrower than the line it holds.
        let mut ever_dropped = false;
        let mut ever_kept = false;
        for w in (200..=1600).step_by(20) {
            let l = Layout::solve(f32::from(u16::try_from(w).expect("width fits u16")), 600.0);
            if l.stats.is_empty() {
                ever_dropped = true;
                continue;
            }
            ever_kept = true;
            let widest = STATS_LINES
                .iter()
                .fold(0.0f32, |acc, s| {
                    acc.max(text::measure(s, l.small, FontWeightHint::Regular))
                })
                .max(text::measure(STATS_HEADING, l.font, FontWeightHint::Bold));
            assert!(
                l.stats.w >= widest,
                "a {w}px window kept a {}px column for a {widest}px line",
                l.stats.w
            );
        }
        assert!(ever_dropped, "the column survived every window width");
        assert!(ever_kept, "the column was never drawn at any width");
    }

    #[test]
    fn every_letter_has_a_key_and_the_keys_stay_in_the_keyboard() {
        for (w, h) in SIZES {
            let l = Layout::solve(w, h);
            if l.key <= 0.0 {
                continue;
            }
            for letter in alphabet() {
                let r = l
                    .key_rect(letter)
                    .unwrap_or_else(|| panic!("no key for {} at {w}x{h}", letter as char));
                assert!(
                    r.x >= l.keyboard.x - 0.5
                        && r.y >= l.keyboard.y - 0.5
                        && r.right() <= l.keyboard.right() + 0.5
                        && r.bottom() <= l.keyboard.bottom() + 0.5,
                    "the {} key {r:?} is outside the keyboard {:?} at {w}x{h}",
                    letter as char,
                    l.keyboard
                );
            }
        }
    }

    #[test]
    fn no_two_keys_overlap() {
        let l = Layout::solve(WINDOW_WIDTH, WINDOW_HEIGHT);
        let keys: Vec<(u8, Rect)> = alphabet()
            .filter_map(|c| l.key_rect(c).map(|r| (c, r)))
            .collect();
        assert_eq!(keys.len(), 26, "some letters had no key");
        for (i, &(a, ra)) in keys.iter().enumerate() {
            for &(b, rb) in &keys[i + 1..] {
                assert!(
                    ra.intersect(rb).is_none(),
                    "the {} and {} keys overlap: {ra:?} {rb:?}",
                    a as char,
                    b as char
                );
            }
        }
    }

    #[test]
    fn a_key_is_clicked_where_it_is_drawn() {
        // The one thing the app could not do. Every letter is clicked at the
        // centre of its own key and must arrive as that letter and no other.
        for letter in alphabet() {
            let mut app = playing_at("zzzz", HangmanApp::SIZE);
            let r = rect_of_sized(&app, Target::Letter(letter), HangmanApp::SIZE)
                .unwrap_or_else(|| panic!("the {} key recorded no hit box", letter as char));
            let (cx, cy) = r.centre();
            app.click_at(cx, cy, MouseButton::Left, HangmanApp::SIZE);
            assert!(
                app.is_guessed(letter),
                "clicking the {} key guessed something else",
                letter as char
            );
            for other in alphabet().filter(|&o| o != letter) {
                assert!(
                    !app.is_guessed(other),
                    "clicking {} also guessed {}",
                    letter as char,
                    other as char
                );
            }
        }
    }

    #[test]
    fn a_key_is_clicked_where_it_is_drawn_in_a_window_of_any_size() {
        for size in SIZES {
            let probe = playing_at("zzzz", size);
            if Layout::solve(size.0, size.1).key <= 0.0 {
                continue;
            }
            for letter in [b'a', b'm', b'q', b'p', b'z'] {
                let Some(r) = rect_of_sized(&probe, Target::Letter(letter), size) else {
                    panic!("no {} key at {size:?}", letter as char);
                };
                let mut app = playing_at("zzzz", size);
                let (cx, cy) = r.centre();
                app.click_at(cx, cy, MouseButton::Left, size);
                assert!(
                    app.is_guessed(letter),
                    "the {} key at {size:?} did not answer a click at its own centre",
                    letter as char
                );
            }
        }
    }

    #[test]
    fn a_guessed_key_stops_being_clickable() {
        let mut app = playing_at("cat", HangmanApp::SIZE);
        assert!(is_visible_sized(
            &app,
            Target::Letter(b'c'),
            HangmanApp::SIZE
        ));
        app.guess_letter(b'c');
        assert!(
            !is_visible_sized(&app, Target::Letter(b'c'), HangmanApp::SIZE),
            "a letter already guessed still offered a hit box"
        );
    }

    #[test]
    fn the_alphabet_is_not_live_under_the_result_card() {
        for phase in [GamePhase::Won, GamePhase::Lost, GamePhase::CategorySelect] {
            let mut app = playing_app("cat");
            app.phase = phase;
            app.resize(WINDOW_WIDTH, WINDOW_HEIGHT);
            for letter in alphabet() {
                assert!(
                    !is_visible_sized(&app, Target::Letter(letter), HangmanApp::SIZE),
                    "the {} key was clickable in {phase:?}",
                    letter as char
                );
            }
        }
    }

    #[test]
    fn a_click_on_nothing_changes_nothing() {
        let mut app = playing_at("cat", HangmanApp::SIZE);
        let before = display_word(&app);
        let outcome = app.click_at(1.0, 1.0, MouseButton::Left, HangmanApp::SIZE);
        assert_eq!(outcome, EventResult::Ignored);
        assert_eq!(display_word(&app), before);
    }

    #[test]
    fn only_the_left_button_presses_a_key() {
        let r = rect_of_sized(
            &playing_at("zzzz", HangmanApp::SIZE),
            Target::Letter(b'a'),
            HangmanApp::SIZE,
        )
        .expect("no A key");
        let (cx, cy) = r.centre();
        for button in [MouseButton::Right, MouseButton::Middle] {
            let mut app = playing_at("zzzz", HangmanApp::SIZE);
            assert_eq!(
                app.click_at(cx, cy, button, HangmanApp::SIZE),
                EventResult::Ignored,
                "{button:?} pressed a key"
            );
            assert!(!app.is_guessed(b'a'));
        }
    }

    /// Every string the frame draws, in the order it draws them.
    fn drawn_text(app: &HangmanApp, size: (f32, f32)) -> Vec<String> {
        app.draw(size)
            .commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    // -- The hint --------------------------------------------------------

    #[test]
    fn the_hint_is_a_button_and_pressing_it_reveals_a_letter() {
        // It used to be a line of text reading "Hint: H key" -- a label
        // describing a keystroke, where a button would do.
        let mut app = playing_at("dolphin", HangmanApp::SIZE);
        let before = total_guessed(&app);
        let r = rect_of_sized(&app, Target::Hint, HangmanApp::SIZE).expect("no hint button");
        let (cx, cy) = r.centre();
        assert_eq!(
            app.click_at(cx, cy, MouseButton::Left, HangmanApp::SIZE),
            EventResult::Consumed
        );
        assert!(app.hint_used, "the hint button did not spend the hint");
        assert!(
            total_guessed(&app) > before,
            "the hint button spent the hint without revealing anything"
        );
    }

    #[test]
    fn a_spent_hint_stops_being_clickable() {
        let mut app = playing_at("dolphin", HangmanApp::SIZE);
        assert!(is_visible_sized(&app, Target::Hint, HangmanApp::SIZE));
        app.use_hint();
        assert!(
            !is_visible_sized(&app, Target::Hint, HangmanApp::SIZE),
            "a hint already spent still offered a hit box"
        );
    }

    #[test]
    fn the_hint_button_says_which_state_it_is_in() {
        let mut app = playing_at("dolphin", HangmanApp::SIZE);
        assert!(drawn_text(&app, HangmanApp::SIZE).contains(&String::from(HINT_LABEL)));
        app.use_hint();
        assert!(
            drawn_text(&app, HangmanApp::SIZE).contains(&String::from(HINT_SPENT_LABEL)),
            "a spent hint still offered itself"
        );
    }

    #[test]
    fn the_hint_button_is_gone_once_the_round_is_over() {
        for phase in [GamePhase::Won, GamePhase::Lost] {
            let mut app = playing_app("dolphin");
            app.phase = phase;
            app.resize(WINDOW_WIDTH, WINDOW_HEIGHT);
            assert!(
                !is_visible_sized(&app, Target::Hint, HangmanApp::SIZE),
                "the hint was still live in {phase:?}"
            );
        }
    }

    // -- The category menu ------------------------------------------------

    #[test]
    fn every_category_has_a_row_and_clicking_it_starts_that_category() {
        for (i, cat) in Category::ALL.iter().enumerate() {
            let mut app = test_app();
            app.resize(WINDOW_WIDTH, WINDOW_HEIGHT);
            let r = rect_of_sized(&app, Target::Category(i), HangmanApp::SIZE)
                .unwrap_or_else(|| panic!("category {i} recorded no hit box"));
            let (cx, cy) = r.centre();
            app.click_at(cx, cy, MouseButton::Left, HangmanApp::SIZE);
            assert_eq!(
                app.category, *cat,
                "clicking row {i} chose the wrong category"
            );
            assert_eq!(
                app.phase,
                GamePhase::Playing,
                "clicking a category did not start the game"
            );
            assert!(
                cat.words().iter().any(|w| w.as_bytes() == app.word),
                "the game started on a word from another category"
            );
        }
    }

    #[test]
    fn the_category_rows_do_not_overlap_each_other() {
        let app = test_app();
        let rects: Vec<Rect> = (0..Category::ALL.len())
            .filter_map(|i| rect_of_sized(&app, Target::Category(i), HangmanApp::SIZE))
            .collect();
        assert_eq!(rects.len(), Category::ALL.len());
        for (i, &a) in rects.iter().enumerate() {
            for &b in &rects[i + 1..] {
                assert!(
                    a.intersect(b).is_none(),
                    "category rows overlap: {a:?} {b:?}"
                );
            }
        }
    }

    #[test]
    fn the_category_menu_is_not_clickable_once_the_game_has_started() {
        let app = playing_at("cat", HangmanApp::SIZE);
        for i in 0..Category::ALL.len() {
            assert!(
                !is_visible_sized(&app, Target::Category(i), HangmanApp::SIZE),
                "category {i} was still clickable during play"
            );
        }
    }

    #[test]
    fn every_difficulty_has_a_chip_and_clicking_it_sets_the_difficulty() {
        for diff in Difficulty::ALL {
            let mut app = test_app();
            app.resize(WINDOW_WIDTH, WINDOW_HEIGHT);
            let r = rect_of_sized(&app, Target::Difficulty(diff), HangmanApp::SIZE)
                .unwrap_or_else(|| panic!("{diff:?} recorded no hit box"));
            let (cx, cy) = r.centre();
            app.click_at(cx, cy, MouseButton::Left, HangmanApp::SIZE);
            assert_eq!(app.difficulty, diff, "the {diff:?} chip set something else");
            assert_eq!(
                app.phase,
                GamePhase::CategorySelect,
                "picking a difficulty started the game by itself"
            );
        }
    }

    #[test]
    fn the_difficulty_chips_do_not_overlap_the_category_rows() {
        let app = test_app();
        for diff in Difficulty::ALL {
            let chip = rect_of_sized(&app, Target::Difficulty(diff), HangmanApp::SIZE)
                .unwrap_or_else(|| panic!("{diff:?} recorded no hit box"));
            for i in 0..Category::ALL.len() {
                let row = rect_of_sized(&app, Target::Category(i), HangmanApp::SIZE)
                    .unwrap_or_else(|| panic!("category {i} recorded no hit box"));
                assert!(
                    chip.intersect(row).is_none(),
                    "the {diff:?} chip {chip:?} sits on category row {i} {row:?}"
                );
            }
        }
    }

    #[test]
    fn a_sixth_category_would_still_be_drawn_above_the_chips() {
        // The chip row used to start at a hand-written `5.0 * (btn_h +
        // btn_gap)` below the first row -- the number of categories written
        // a second time, forty lines from `Category::ALL`. This checks the
        // relationship rather than the number: the chips begin below the
        // last row, whatever the count.
        let app = test_app();
        let last = rect_of_sized(
            &app,
            Target::Category(Category::ALL.len() - 1),
            HangmanApp::SIZE,
        )
        .expect("no last category row");
        for diff in Difficulty::ALL {
            let chip = rect_of_sized(&app, Target::Difficulty(diff), HangmanApp::SIZE)
                .expect("no difficulty chip");
            assert!(
                chip.y >= last.bottom(),
                "the {diff:?} chip starts at {} , above the last row's bottom {}",
                chip.y,
                last.bottom()
            );
        }
    }

    // -- The result card --------------------------------------------------

    #[test]
    fn the_result_card_offers_both_ways_out() {
        for phase in [GamePhase::Won, GamePhase::Lost] {
            let mut app = playing_app("cat");
            app.phase = phase;
            app.resize(WINDOW_WIDTH, WINDOW_HEIGHT);
            for target in [Target::PlayAgain, Target::Menu] {
                assert!(
                    is_visible_sized(&app, target, HangmanApp::SIZE),
                    "{target:?} was missing from the {phase:?} card"
                );
            }
        }
    }

    #[test]
    fn play_again_deals_a_new_round() {
        let mut app = playing_app("cat");
        app.phase = GamePhase::Won;
        app.wrong_count = 3;
        app.resize(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert_eq!(
            click_sized(
                &mut app,
                Target::PlayAgain,
                MouseButton::Left,
                HangmanApp::SIZE
            ),
            EventResult::Consumed
        );
        assert_eq!(app.phase, GamePhase::Playing);
        assert_eq!(app.wrong_count, 0, "the new round kept the old strikes");
    }

    #[test]
    fn the_menu_button_goes_back_to_the_categories() {
        let mut app = playing_app("cat");
        app.phase = GamePhase::Lost;
        app.resize(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert_eq!(
            click_sized(&mut app, Target::Menu, MouseButton::Left, HangmanApp::SIZE),
            EventResult::Consumed
        );
        assert_eq!(app.phase, GamePhase::CategorySelect);
    }

    #[test]
    fn the_result_buttons_are_not_live_during_play() {
        let app = playing_at("cat", HangmanApp::SIZE);
        for target in [Target::PlayAgain, Target::Menu] {
            assert!(
                !is_visible_sized(&app, target, HangmanApp::SIZE),
                "{target:?} was clickable mid-game"
            );
        }
    }

    // -- What the frame actually says -------------------------------------

    /// Every text command in the frame, with its position.
    fn drawn_text_at(app: &HangmanApp, size: (f32, f32)) -> Vec<(String, f32, f32)> {
        app.draw(size)
            .commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, x, y, .. } => Some((text.clone(), *x, *y)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn nothing_is_drawn_outside_the_window() {
        // `render` used to paint a fixed 740x560 picture whatever the window
        // was, so in a 400x300 window most of the game was off-screen.
        for size in SIZES {
            for phase in [
                GamePhase::CategorySelect,
                GamePhase::Playing,
                GamePhase::Won,
                GamePhase::Lost,
            ] {
                let mut app = playing_app("dolphin");
                app.phase = phase;
                app.wrong_count = 3;
                for (s, x, y) in drawn_text_at(&app, size) {
                    assert!(
                        x >= -1.0 && y >= -1.0 && x <= size.0 + 1.0 && y + 1.0 <= size.1 + 1.0,
                        "{phase:?} drew {s:?} at ({x}, {y}) in a {size:?} window"
                    );
                }
            }
        }
    }

    #[test]
    fn the_win_rate_is_placed_against_the_right_edge_not_a_width() {
        // It read `x: header_w - 60.0`, and `header_w` is a width, not a
        // coordinate: in a narrow window the rate left the header entirely.
        for size in SIZES {
            let mut app = playing_app("cat");
            app.stats.wins = 3;
            app.stats.losses = 1;
            let l = Layout::solve(size.0, size.1);
            let want = format!("{}% win", app.stats.win_rate_percent());
            let Some((_, x, _)) = drawn_text_at(&app, size)
                .into_iter()
                .find(|(s, _, _)| *s == want)
            else {
                // A window too narrow for the rate drops it rather than
                // drawing it somewhere wrong -- that is the other half of
                // the fix and is checked by the sweep above.
                continue;
            };
            let w = text::measure(&want, l.small, FontWeightHint::Regular);
            assert!(
                (x + w - (l.header.right() - l.pad)).abs() < 1.0,
                "the win rate ended at {} , not against the header's right edge {} , at {size:?}",
                x + w,
                l.header.right() - l.pad
            );
        }
    }

    #[test]
    fn the_header_items_do_not_sit_on_top_of_each_other() {
        // They were drawn at `PADDING + 100.0` and `PADDING + 220.0` --
        // offsets that were right for one set of words. A long category name
        // put the difficulty on top of it.
        for cat in Category::ALL {
            let mut app = playing_app("cat");
            app.category = cat;
            let l = Layout::solve(WINDOW_WIDTH, WINDOW_HEIGHT);
            let drawn = drawn_text_at(&app, HangmanApp::SIZE);
            let find = |want: &str, size: f32, weight: FontWeightHint| {
                drawn
                    .iter()
                    .find(|(s, _, _)| s == want)
                    .map(|&(_, x, _)| (x, x + text::measure(want, size, weight)))
            };
            let Some(title) = find("Hangman", l.font, FontWeightHint::Bold) else {
                panic!("the header lost its title");
            };
            let Some(label) = find(cat.label(), l.small, FontWeightHint::Regular) else {
                panic!("the header lost the {} category", cat.label());
            };
            assert!(
                title.1 <= label.0,
                "{:?} runs into the {} label {:?}",
                title,
                cat.label(),
                label
            );
        }
    }

    #[test]
    fn the_word_shows_what_has_been_guessed_and_hides_what_has_not() {
        let mut app = playing_at("cat", HangmanApp::SIZE);
        app.guess_letter(b'c');
        let drawn = drawn_text(&app, HangmanApp::SIZE);
        assert!(
            drawn.contains(&String::from("C")),
            "a guessed letter was not shown"
        );
        for hidden in ["A", "T"] {
            assert!(
                !drawn.contains(&String::from(hidden)) || drawn.contains(&String::from("_")),
                "an unguessed letter was shown"
            );
        }
        assert!(
            drawn.iter().filter(|s| *s == "_").count() == 2,
            "expected two blanks for the two unguessed letters, got {:?}",
            drawn.iter().filter(|s| *s == "_").count()
        );
    }

    #[test]
    fn a_lost_game_shows_the_word_it_was_hiding() {
        let mut app = playing_app("cat");
        app.phase = GamePhase::Lost;
        app.resize(WINDOW_WIDTH, WINDOW_HEIGHT);
        let drawn = drawn_text(&app, HangmanApp::SIZE);
        for letter in ["C", "A", "T"] {
            assert!(
                drawn.contains(&String::from(letter)),
                "a lost game hid the {letter} of its own word"
            );
        }
        assert!(
            !drawn.contains(&String::from("_")),
            "a lost game still showed blanks"
        );
    }

    #[test]
    fn the_header_counts_down_the_guesses_that_are_left() {
        for wrong in 0..MAX_WRONG {
            let mut app = playing_app("dolphin");
            app.wrong_count = wrong;
            app.resize(WINDOW_WIDTH, WINDOW_HEIGHT);
            let want = format!("Remaining: {}", MAX_WRONG - wrong);
            assert!(
                drawn_text(&app, HangmanApp::SIZE).contains(&want),
                "after {wrong} wrong guesses the header did not say {want:?}"
            );
        }
    }

    // -- The keyboard and the key handler agree ---------------------------

    #[test]
    fn every_letter_the_keyboard_draws_can_also_be_typed() {
        // The alphabet on screen and the alphabet `key_to_letter` accepts are
        // two lists; they have to hold the same letters or a key exists that
        // only one of the two ways of pressing it reaches.
        let drawn: Vec<u8> = KEY_ROWS
            .iter()
            .flat_map(|r| r.iter().map(|&b| b.to_ascii_lowercase()))
            .collect();
        let mut sorted = drawn.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), drawn.len(), "a letter is drawn on two keys");
        assert_eq!(
            sorted,
            alphabet().collect::<Vec<u8>>(),
            "the on-screen keyboard is not the alphabet"
        );
    }

    #[test]
    fn a_typed_letter_and_a_clicked_key_do_the_same_thing() {
        for (k, letter) in [(Key::C, b'c'), (Key::X, b'x'), (Key::Z, b'z')] {
            let mut typed = playing_at("cat", HangmanApp::SIZE);
            key(&mut typed, &press(k));

            let mut clicked = playing_at("cat", HangmanApp::SIZE);
            click_sized(
                &mut clicked,
                Target::Letter(letter),
                MouseButton::Left,
                HangmanApp::SIZE,
            );

            assert_eq!(
                typed.guessed, clicked.guessed,
                "typing and clicking {} guessed different letters",
                letter as char
            );
            assert_eq!(
                typed.wrong_count, clicked.wrong_count,
                "typing and clicking {} cost different numbers of strikes",
                letter as char
            );
        }
    }

    #[test]
    fn h_asks_for_the_hint_only_while_the_hint_is_unspent() {
        let mut app = playing_at("dolphin", HangmanApp::SIZE);
        key(&mut app, &press(Key::H));
        assert!(app.hint_used, "H did not ask for the hint");
        assert!(!app.is_guessed(b'h'), "H was spent as a guess as well");

        // With the hint gone, H is an ordinary letter again.
        key(&mut app, &press(Key::H));
        assert!(
            app.is_guessed(b'h'),
            "H stopped being a letter after the hint"
        );
    }

    #[test]
    fn a_click_uses_the_window_the_player_is_looking_at() {
        // The frame is drawn at one size and clicked at another; the click
        // must resolve against the size that was drawn last, not against the
        // constant the app started with.
        let small = (320.0, 260.0);
        let mut app = playing_app("zzzz");
        let _ = app.render(small.0, small.1);
        let r = rect_of_sized(&app, Target::Letter(b'q'), small).expect("no Q key in a 320 window");
        let (cx, cy) = r.centre();
        assert_eq!(
            app.handle_event(&Event::Mouse(MouseEvent {
                x: cx,
                y: cy,
                kind: MouseEventKind::Press(MouseButton::Left),
            })),
            EventResult::Consumed,
            "a click at the Q key of the drawn window missed"
        );
        assert!(app.is_guessed(b'q'));
    }

    #[test]
    fn the_app_names_itself_the_same_way_twice() {
        let app = test_app();
        assert_eq!(app.app_id(), "hangman");
        assert!(app.title().contains("Hangman"));
        assert_eq!(app.initial_size(), (740, 560));
    }

    #[test]
    fn a_close_request_exits() {
        let mut app = test_app();
        assert!(matches!(
            app.on_event(&Event::CloseRequested),
            Response::Exit
        ));
    }

    #[test]
    fn an_event_the_game_has_no_use_for_leaves_it_idle() {
        let mut app = test_app();
        assert!(matches!(
            app.on_event(&Event::Key(press(Key::F5))),
            Response::Idle
        ));
    }

    // -- Gaps the mutation harness found ----------------------------------

    #[test]
    fn the_keyboard_never_takes_more_than_its_share_of_the_height() {
        // The key is the smaller of what the width allows and what the height
        // allows. Taking the width alone survives every containment test --
        // in a wide, short window it simply eats the game. A 1200x200 window
        // gives the width room for 116px keys and the height room for 18.
        for (w, h) in SIZES
            .iter()
            .copied()
            .chain([(1200.0, 200.0), (2000.0, 260.0)])
        {
            let l = Layout::solve(w, h);
            assert!(
                l.keyboard.h <= h * 0.36,
                "the keyboard took {} of a {h}px-tall window",
                l.keyboard.h
            );
        }
    }

    #[test]
    fn the_word_row_never_pushes_past_the_keyboard() {
        // `word_h` is clamped to the room between the header and the keys.
        // Without the clamp a short window draws the word over them.
        // The clamp only binds in a window short enough that the header, the
        // keyboard and the padding have already spent the height: two lines of
        // word type is 24px at the smallest font, and a 400x130 window still
        // has 46px to spare. These three do not.
        for (w, h) in SIZES
            .iter()
            .copied()
            .chain([(400.0, 80.0), (600.0, 70.0), (900.0, 55.0)])
        {
            let l = Layout::solve(w, h);
            assert!(
                l.word.bottom() <= l.keyboard.y + 0.5,
                "the word row ends at {} , below the keyboard's top {} , at {w}x{h}",
                l.word.bottom(),
                l.keyboard.y
            );
        }
    }

    #[test]
    fn each_row_of_keys_is_centred_in_the_keyboard() {
        // The second and third rows are shorter than the first, and are
        // indented by half the difference. Dropping the indent leaves them
        // left-aligned -- which no overlap or containment test can see,
        // because left-aligned keys are still inside the keyboard and still
        // do not touch each other.
        let l = Layout::solve(WINDOW_WIDTH, WINDOW_HEIGHT);
        for (i, row) in KEY_ROWS.iter().enumerate() {
            let rects: Vec<Rect> = row
                .iter()
                .filter_map(|&b| l.key_rect(b.to_ascii_lowercase()))
                .collect();
            assert_eq!(rects.len(), row.len(), "row {i} lost a key");
            let left = rects.iter().fold(f32::MAX, |acc, r| acc.min(r.x));
            let right = rects.iter().fold(0.0f32, |acc, r| acc.max(r.right()));
            let error = ((left - l.keyboard.x) - (l.keyboard.right() - right)).abs();
            assert!(
                error < 1.0,
                "row {i} has {} px of margin on the left and {} px on the right",
                left - l.keyboard.x,
                l.keyboard.right() - right
            );
        }
    }

    #[test]
    fn a_window_too_small_for_a_keyboard_draws_no_keys_and_offers_none() {
        // The guard lives in `key_rect` and nowhere else, so this reaches it.
        for size in [(20.0, 20.0), (30.0, 12.0), (8.0, 400.0)] {
            let l = Layout::solve(size.0, size.1);
            assert!(l.key <= 0.0, "a {size:?} window found room for a key");
            for letter in alphabet() {
                assert!(
                    l.key_rect(letter).is_none(),
                    "a {size:?} window offered a rectangle for {}",
                    letter as char
                );
            }
            let app = playing_at("cat", size);
            for letter in alphabet() {
                assert!(
                    !is_visible_sized(&app, Target::Letter(letter), size),
                    "the {} key was clickable in a {size:?} window",
                    letter as char
                );
            }
        }
    }
}
