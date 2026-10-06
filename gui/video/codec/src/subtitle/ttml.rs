//! TTML -- the W3C's Timed Text Markup Language, the subtitles of broadcast
//! and streaming (IMSC 1 is its profile for them) -- in MP4: ISO/IEC
//! 14496-30's `stpp` sample entry, a whole TTML document a sample, shown
//! for the sample's stretch of time and no longer.
//!
//! **Read as ttconv reads it.** FFmpeg has no TTML decoder. ttconv -- the
//! converter from the IMSC 1 reference implementation's authors -- turns a
//! document into what shows at each moment (TTML's intermediate synchronic
//! documents), and this module follows it rule for rule: its timing (times
//! relative to the parent, a parallel container ending with its last child,
//! a sequential one's children one after another, text lasting as long as
//! its parent); its region association and pruning; its styles (inline over
//! referenced, the last reference over the earlier, inherited from parent
//! and region); and its handling of white space. Where ttconv departs from
//! TTML, TTML is followed: a parallel container that begins late lasts
//! until its last child ends (ttconv sets the child's end, on the
//! container's clock, against the container's begin on its parent's, and
//! ends it early); and an element TTML does not allow where it is is passed
//! over alone (ttconv passes over it, every element after it, and its
//! parent's own styles).
//!
//! **Then said in SRT** by this crate's rules: each paragraph (`p`) a cue
//! for each stretch it shows the same through; its text bold, italic,
//! underlined, struck through and coloured as its spans compute, white
//! being SRT's default; breaks as line breaks, never two together nor at
//! either end; and `{\anN}` for the third of the picture each way the
//! paragraph's anchor -- its region's edge or middle, as `displayAlign` and
//! `textAlign` put it -- falls in, as WebVTT's placement is said. Where this
//! departs from ttconv's own SRT: oblique type is italic, and a line struck
//! through is struck (ttconv's SRT keeps neither); hidden text
//! (`visibility="hidden"`) is not shown; and a colour is written as SRT's
//! `#rrggbb`, its opacity dropped.
//!
//! **What it costs.** ttconv works the whole document out again at every
//! moment anything in it begins or ends: for a document of many paragraphs,
//! its size times their number. Here each paragraph is worked out only where
//! it or something inside it changes ([`Document::showings`]), which says
//! what the whole document says at every moment -- a test holds the two
//! together over thousands of random documents, the whole-document reading
//! kept for it (`Document::at`, in tests). Styles and regions are looked up by a
//! hash. A document that would still cost more than [`WORK_PER_BYTE`] times
//! its size -- only one made to be slow: a paragraph of thousands of spans,
//! each beginning at its own time -- is given up on, and its sample counted
//! as damaged. Times are exact fractions of a second throughout, compared
//! and rounded without overflow however many digits a document gives them.

use std::collections::HashMap;

use super::srt::{Op, Toggle, Writer};
use super::xml::{self, Element, Node, XML_NAMESPACE};

const TT: &str = "http://www.w3.org/ns/ttml";
const TTS: &str = "http://www.w3.org/ns/ttml#styling";
const TTP: &str = "http://www.w3.org/ns/ttml#parameter";

/// An exact count of seconds. TTML counts frames and ticks of rates a
/// document chooses (30000/1001 frames a second), which no float holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Ratio {
    num: i128,
    den: i128,
}

impl Ratio {
    pub(crate) const ZERO: Self = Self { num: 0, den: 1 };

    fn new(num: i128, den: i128) -> Option<Self> {
        if den == 0 {
            return None;
        }
        let (mut num, mut den) = if den < 0 {
            (num.checked_neg()?, den.checked_neg()?)
        } else {
            (num, den)
        };
        let g = gcd(num.unsigned_abs(), den.unsigned_abs());
        if g > 1 {
            let g = i128::try_from(g).ok()?;
            num = num.checked_div(g)?;
            den = den.checked_div(g)?;
        }
        Some(Self { num, den })
    }

    fn whole(n: i128) -> Self {
        Self { num: n, den: 1 }
    }

    fn add(self, other: Self) -> Option<Self> {
        Self::new(
            self.num
                .checked_mul(other.den)?
                .checked_add(other.num.checked_mul(self.den)?)?,
            self.den.checked_mul(other.den)?,
        )
    }

    fn sub(self, other: Self) -> Option<Self> {
        self.add(Self::new(other.num.checked_neg()?, other.den)?)
    }

    fn mul(self, other: Self) -> Option<Self> {
        Self::new(
            self.num.checked_mul(other.num)?,
            self.den.checked_mul(other.den)?,
        )
    }

    fn div(self, other: Self) -> Option<Self> {
        Self::new(
            self.num.checked_mul(other.den)?,
            self.den.checked_mul(other.num)?,
        )
    }

    /// In nanoseconds, the nearest -- a half rounded up -- saturating.
    /// Exactly, however large the numerator and denominator: the whole
    /// seconds and the fraction left are taken apart first, and the
    /// fraction's nanoseconds worked out without a product that could
    /// overflow.
    pub(crate) fn to_ns(self) -> i64 {
        const NS: u128 = 1_000_000_000;
        // `den` is positive, so the remainder is in [0, den).
        let seconds = self.num.div_euclid(self.den);
        let left = self.num.rem_euclid(self.den).unsigned_abs();
        let den = self.den.unsigned_abs();
        let (mut ns, rest) = mul_div(left, NS, den);
        // A half or more of a nanosecond left rounds up; `rest` < `den` <
        // 2^127, so doubling it cannot overflow.
        if rest.saturating_mul(2) >= den {
            ns = ns.saturating_add(1);
        }
        let ns = i128::try_from(ns).unwrap_or(i128::MAX);
        let total = seconds.saturating_mul(1_000_000_000).saturating_add(ns);
        i64::try_from(total).unwrap_or(if total < 0 { i64::MIN } else { i64::MAX })
    }

    /// From nanoseconds.
    #[cfg(test)]
    pub(crate) fn from_ns(ns: i64) -> Self {
        Self::new(i128::from(ns), 1_000_000_000).unwrap_or(Self::ZERO)
    }

    /// `ticks` of a track whose tick is `num / den` seconds, exactly.
    pub(crate) fn exact(ticks: i64, (num, den): (u64, u64)) -> Self {
        Self::new(
            i128::from(ticks).saturating_mul(i128::from(num)),
            i128::from(den),
        )
        .unwrap_or(Self::ZERO)
    }
}

impl PartialOrd for Ratio {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Ratio {
    /// By value, exactly: whole parts first, and where those are equal, the
    /// fractions left -- `r/b` against `s/d`, which order as `d/s` against
    /// `b/r` do, reversed -- as Euclid's algorithm steps. No product is
    /// formed, so no numerator or denominator is too large to compare (a
    /// cross-multiplication saturating would make two different times
    /// equal, and an order that is no order can make a sort panic). Every
    /// ratio is in lowest terms, so this is equal exactly where `==` is.
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        use core::cmp::Ordering;
        let (mut a, mut b, mut c, mut d) = (self.num, self.den, other.num, other.den);
        let mut reversed = false;
        loop {
            // `b` and `d` stay positive: the denominators, then remainders
            // known to be above zero.
            let (whole_ab, left_ab) = (a.div_euclid(b), a.rem_euclid(b));
            let (whole_cd, left_cd) = (c.div_euclid(d), c.rem_euclid(d));
            let order = match whole_ab.cmp(&whole_cd) {
                Ordering::Equal => match (left_ab, left_cd) {
                    (0, 0) => Ordering::Equal,
                    (0, _) => Ordering::Less,
                    (_, 0) => Ordering::Greater,
                    _ => {
                        (a, b, c, d) = (b, left_ab, d, left_cd);
                        reversed = !reversed;
                        continue;
                    }
                },
                order => order,
            };
            return if reversed { order.reverse() } else { order };
        }
    }
}

/// `a * b / d` rounded down, and what is left, for `a` < `d` < 2^127:
/// long multiplication a bit of `b` at a time, the running remainder kept
/// below `d` so that nothing overflows.
fn mul_div(a: u128, b: u128, d: u128) -> (u128, u128) {
    let (mut quotient, mut rest) = (0u128, 0u128);
    for bit in (0..u128::BITS.saturating_sub(b.leading_zeros())).rev() {
        // Twice the running product: `rest` < `d` < 2^127, so it fits.
        quotient = quotient.saturating_mul(2);
        rest = rest.saturating_mul(2);
        if rest >= d {
            rest = rest.saturating_sub(d);
            quotient = quotient.saturating_add(1);
        }
        if (b >> bit) & 1 == 1 {
            // `a` and `rest` are each below `d`, so the sum is below 2d.
            rest = rest.saturating_add(a);
            if rest >= d {
                rest = rest.saturating_sub(d);
                quotient = quotient.saturating_add(1);
            }
        }
    }
    (quotient, rest)
}

fn gcd(mut a: u128, mut b: u128) -> u128 {
    // `b` is not zero inside the loop, so the remainder is always one.
    while let Some(rest) = a.checked_rem(b) {
        (a, b) = (b, rest);
    }
    a
}

/// What a document's time expressions are counted in (`ttp:frameRate` and
/// its multiplier, `ttp:subFrameRate`, `ttp:tickRate`), and what a length in
/// cells or pixels is (`ttp:cellResolution`, the root's `tts:extent`).
struct Parameters {
    /// Frames a second, the multiplier applied.
    frame_rate: Ratio,
    sub_frame_rate: Ratio,
    tick_rate: Ratio,
    /// Columns and rows of the root container's cells.
    cells: (Ratio, Ratio),
    /// The root container's size in pixels, where the document gives one.
    pixels: Option<(Ratio, Ratio)>,
}

impl Parameters {
    /// As ttconv reads them: 30 frames a second, a multiplier of 1, one
    /// sub-frame a frame and one tick a second where none is given -- the
    /// tick rate not following the frame rate, as TTML would have it -- and
    /// 32 by 15 cells.
    fn read(tt: &Element<'_>) -> Self {
        let number = |name: &str| {
            tt.attribute(TTP, name)
                .and_then(|v| v.trim().parse::<u32>().ok())
                .filter(|&n| n > 0)
                .map(|n| Ratio::whole(i128::from(n)))
        };
        let mut frame_rate = number("frameRate").unwrap_or(Ratio::whole(30));
        if let Some(m) = tt.attribute(TTP, "frameRateMultiplier") {
            let mut parts = m.split_ascii_whitespace().map(|p| p.parse::<u32>().ok());
            if let (Some(Some(n)), Some(Some(d)), None) = (parts.next(), parts.next(), parts.next())
                && let Some(multiplied) = Ratio::new(i128::from(n), i128::from(d))
                    .and_then(|r| frame_rate.mul(r))
                    .filter(|r| r.num > 0)
            {
                frame_rate = multiplied;
            }
        }
        let cells = tt
            .attribute(TTP, "cellResolution")
            .and_then(|v| {
                let mut parts = v.split_ascii_whitespace().map(|p| p.parse::<u32>().ok());
                match (parts.next(), parts.next(), parts.next()) {
                    (Some(Some(c)), Some(Some(r)), None) if c > 0 && r > 0 => {
                        Some((Ratio::whole(i128::from(c)), Ratio::whole(i128::from(r))))
                    }
                    _ => None,
                }
            })
            .unwrap_or((Ratio::whole(32), Ratio::whole(15)));
        let pixels = tt.attribute(TTS, "extent").and_then(|v| {
            let (w, h) = pair(v)?;
            match (w, h) {
                (Length::Px(w), Length::Px(h)) if w.num > 0 && h.num > 0 => Some((w, h)),
                _ => None,
            }
        });
        Self {
            frame_rate,
            sub_frame_rate: number("subFrameRate").unwrap_or(Ratio::whole(1)),
            tick_rate: number("tickRate").unwrap_or(Ratio::whole(1)),
            cells,
            pixels,
        }
    }

    /// A time expression (TTML 1, §10.3.1): a clock time
    /// `hh:mm:ss[.fraction | :frames[.sub-frames]]`, or an offset
    /// `count[.fraction](h|m|s|ms|f|t)`.
    fn time(&self, expr: &str) -> Option<Ratio> {
        let expr = expr.trim();
        if expr.contains(':') {
            return self.clock(expr);
        }
        let unit_at = expr.find(|c: char| c.is_ascii_alphabetic())?;
        let (count, unit) = expr.split_at(unit_at);
        let count = decimal(count)?;
        let scale = match unit {
            "h" => Ratio::whole(3600),
            "m" => Ratio::whole(60),
            "s" => Ratio::whole(1),
            "ms" => Ratio::new(1, 1000)?,
            "f" => Ratio::whole(1).div(self.frame_rate)?,
            "t" => Ratio::whole(1).div(self.tick_rate)?,
            _ => return None,
        };
        count.mul(scale)
    }

    fn clock(&self, expr: &str) -> Option<Ratio> {
        let mut parts = expr.splitn(4, ':');
        let hours = parts.next()?;
        let minutes = parts.next()?;
        let seconds = parts.next()?;
        let frames = parts.next();
        let digits = |s: &str, at_least: usize| {
            (s.len() >= at_least && s.bytes().all(|b| b.is_ascii_digit()))
                .then(|| s.parse::<i128>().ok())
                .flatten()
        };
        let h = digits(hours, 2)?;
        let m = digits(minutes, 2).filter(|_| minutes.len() == 2)?;
        let mut total = Ratio::whole(h.checked_mul(3600)?.checked_add(m.checked_mul(60)?)?);
        match frames {
            None => {
                let (whole, fraction) = match seconds.split_once('.') {
                    Some((w, f)) => (w, Some(f)),
                    None => (seconds, None),
                };
                if whole.len() != 2 {
                    return None;
                }
                let s = digits(whole, 2)?;
                total = total.add(Ratio::whole(s))?;
                if let Some(f) = fraction {
                    total = total.add(decimal(&format!("0.{f}"))?)?;
                }
            }
            Some(frames) => {
                let s = digits(seconds, 2).filter(|_| seconds.len() == 2)?;
                total = total.add(Ratio::whole(s))?;
                let (f, sub) = match frames.split_once('.') {
                    Some((f, s)) => (f, Some(s)),
                    None => (frames, None),
                };
                let f = digits(f, 2)?;
                total = total.add(Ratio::whole(f).div(self.frame_rate)?)?;
                if let Some(sub) = sub {
                    let sub = digits(sub, 1)?;
                    total = total
                        .add(Ratio::whole(sub).div(self.frame_rate.mul(self.sub_frame_rate)?)?)?;
                }
            }
        }
        Some(total)
    }
}

/// A decimal number of digits, a point and digits, exactly.
fn decimal(s: &str) -> Option<Ratio> {
    let (whole, fraction) = match s.split_once('.') {
        Some((w, f)) => (w, f),
        None => (s, ""),
    };
    if whole.is_empty()
        || !whole.bytes().all(|b| b.is_ascii_digit())
        || !fraction.bytes().all(|b| b.is_ascii_digit())
        || (s.contains('.') && fraction.is_empty())
        || fraction.len() > 30
    {
        return None;
    }
    let w = whole.parse::<i128>().ok()?;
    let mut value = Ratio::whole(w);
    if !fraction.is_empty() {
        let f = fraction.parse::<i128>().ok()?;
        let scale = 10i128.checked_pow(u32::try_from(fraction.len()).ok()?)?;
        value = value.add(Ratio::new(f, scale)?)?;
    }
    Some(value)
}

/// A length, as TTML gives one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Length {
    Percent(Ratio),
    Px(Ratio),
    Cells(Ratio),
}

fn length(s: &str) -> Option<Length> {
    let s = s.trim();
    if let Some(v) = s.strip_suffix('%') {
        return decimal(v).map(Length::Percent);
    }
    if let Some(v) = s.strip_suffix("px") {
        return decimal(v).map(Length::Px);
    }
    if let Some(v) = s.strip_suffix('c') {
        return decimal(v).map(Length::Cells);
    }
    None
}

/// Two lengths, as `tts:origin` and `tts:extent` give them.
fn pair(s: &str) -> Option<(Length, Length)> {
    let mut parts = s.split_ascii_whitespace();
    let (a, b) = (parts.next()?, parts.next()?);
    if parts.next().is_some() {
        return None;
    }
    Some((length(a)?, length(b)?))
}

/// A colour: red, green, blue and alpha.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Colour([u8; 4]);

const WHITE: Colour = Colour([255, 255, 255, 255]);

/// `tts:color` (TTML 1, §8.2.4): a named colour, `#rrggbb`, `#rrggbbaa`,
/// `rgb(r,g,b)` or `rgba(r,g,b,a)`.
fn colour(s: &str) -> Option<Colour> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix('#') {
        let byte = |i: usize| {
            hex.get(i..i.checked_add(2)?)
                .and_then(|h| u8::from_str_radix(h, 16).ok())
        };
        return match hex.len() {
            6 => Some(Colour([byte(0)?, byte(2)?, byte(4)?, 255])),
            8 => Some(Colour([byte(0)?, byte(2)?, byte(4)?, byte(6)?])),
            _ => None,
        };
    }
    for (prefix, n) in [("rgba(", 4), ("rgb(", 3)] {
        if let Some(rest) = s.strip_prefix(prefix).and_then(|r| r.strip_suffix(')')) {
            let values: Vec<u8> = rest
                .split(',')
                .map(|v| v.trim().parse::<u8>().ok())
                .collect::<Option<_>>()?;
            return match (n, values.as_slice()) {
                (3, [r, g, b]) => Some(Colour([*r, *g, *b, 255])),
                (4, [r, g, b, a]) => Some(Colour([*r, *g, *b, *a])),
                _ => None,
            };
        }
    }
    let named = match s {
        "transparent" => [0, 0, 0, 0],
        "black" => [0, 0, 0, 255],
        "silver" => [192, 192, 192, 255],
        "gray" => [128, 128, 128, 255],
        "white" => [255, 255, 255, 255],
        "maroon" => [128, 0, 0, 255],
        "red" => [255, 0, 0, 255],
        "purple" => [128, 0, 128, 255],
        "fuchsia" | "magenta" => [255, 0, 255, 255],
        "green" => [0, 128, 0, 255],
        "lime" => [0, 255, 0, 255],
        "olive" => [128, 128, 0, 255],
        "yellow" => [255, 255, 0, 255],
        "navy" => [0, 0, 128, 255],
        "blue" => [0, 0, 255, 255],
        "teal" => [0, 128, 128, 255],
        "aqua" | "cyan" => [0, 255, 255, 255],
        _ => return None,
    };
    Some(Colour(named))
}

/// Where a line's text goes across its region (`tts:textAlign`), and where
/// its lines go down it (`tts:displayAlign`): 0 the start, 1 the middle, 2
/// the end.
type Align = u8;

/// The styles this reader says: each `None` where an element does not
/// specify it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Specified {
    colour: Option<Colour>,
    bold: Option<bool>,
    italic: Option<bool>,
    /// `tts:textDecoration`, flag by flag: underline, line-through.
    underline: Option<bool>,
    strike: Option<bool>,
    text_align: Option<Align>,
    display_align: Option<Align>,
    hidden: Option<bool>,
    display_none: Option<bool>,
    origin: Option<(Length, Length)>,
    extent: Option<(Length, Length)>,
}

impl Specified {
    /// The styles `e`'s own `tts:` attributes give.
    fn inline(e: &Element<'_>) -> Self {
        let mut s = Self::default();
        for (name, value) in &e.attributes {
            if *name.namespace != *TTS {
                continue;
            }
            let value = value.trim();
            match name.local {
                "color" => s.colour = colour(value),
                "fontWeight" => {
                    s.bold = match value {
                        "bold" => Some(true),
                        "normal" => Some(false),
                        _ => None,
                    }
                }
                "fontStyle" => {
                    s.italic = match value {
                        "italic" | "oblique" => Some(true),
                        "normal" => Some(false),
                        _ => None,
                    }
                }
                "textDecoration" => {
                    for word in value.split_ascii_whitespace() {
                        match word {
                            "none" => {
                                s.underline = Some(false);
                                s.strike = Some(false);
                            }
                            "underline" => s.underline = Some(true),
                            "noUnderline" => s.underline = Some(false),
                            "lineThrough" => s.strike = Some(true),
                            "noLineThrough" => s.strike = Some(false),
                            _ => {}
                        }
                    }
                }
                "textAlign" => {
                    s.text_align = match value {
                        "left" | "start" | "justify" => Some(0),
                        "center" => Some(1),
                        "right" | "end" => Some(2),
                        _ => None,
                    }
                }
                "displayAlign" => {
                    s.display_align = match value {
                        "before" => Some(0),
                        "center" => Some(1),
                        "after" => Some(2),
                        _ => None,
                    }
                }
                "visibility" => {
                    s.hidden = match value {
                        "hidden" => Some(true),
                        "visible" => Some(false),
                        _ => None,
                    }
                }
                "display" => {
                    s.display_none = match value {
                        "none" => Some(true),
                        "auto" => Some(false),
                        _ => None,
                    }
                }
                "origin" => s.origin = pair(value),
                "extent" => s.extent = pair(value),
                _ => {}
            }
        }
        s
    }

    /// Each of `self`'s unset styles taken from `under`: what referential
    /// styling does, a style referenced adding only what is not yet set.
    fn under(mut self, under: &Self) -> Self {
        self.colour = self.colour.or(under.colour);
        self.bold = self.bold.or(under.bold);
        self.italic = self.italic.or(under.italic);
        self.underline = self.underline.or(under.underline);
        self.strike = self.strike.or(under.strike);
        self.text_align = self.text_align.or(under.text_align);
        self.display_align = self.display_align.or(under.display_align);
        self.hidden = self.hidden.or(under.hidden);
        self.display_none = self.display_none.or(under.display_none);
        self.origin = self.origin.or(under.origin);
        self.extent = self.extent.or(under.extent);
        self
    }
}

/// The styles an element shows with: its own, else its parent's (every one
/// of these inherits but `display`, which prunes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Computed {
    colour: Colour,
    bold: bool,
    italic: bool,
    underline: bool,
    strike: bool,
    text_align: Align,
    hidden: bool,
}

impl Computed {
    /// TTML's initial values: white, normal, start.
    const INITIAL: Self = Self {
        colour: WHITE,
        bold: false,
        italic: false,
        underline: false,
        strike: false,
        text_align: 0,
        hidden: false,
    };

    fn with(self, s: &Specified) -> Self {
        Self {
            colour: s.colour.unwrap_or(self.colour),
            bold: s.bold.unwrap_or(self.bold),
            italic: s.italic.unwrap_or(self.italic),
            underline: s.underline.unwrap_or(self.underline),
            strike: s.strike.unwrap_or(self.strike),
            text_align: s.text_align.unwrap_or(self.text_align),
            hidden: s.hidden.unwrap_or(self.hidden),
        }
    }
}

/// A region: where its paragraphs go, in percent of the root container.
#[derive(Clone, Debug)]
struct Region {
    id: String,
    /// Left, top, width and height, each a fraction of the root's.
    place: [Ratio; 4],
    display_align: Align,
    /// Its styles, for what flows into it to inherit.
    styles: Specified,
}

impl Region {
    /// The root container: the one region of a document that lays out none,
    /// every element in it.
    fn root() -> Self {
        Self {
            id: String::new(),
            place: [Ratio::ZERO, Ratio::ZERO, Ratio::whole(1), Ratio::whole(1)],
            display_align: 0,
            styles: Specified::default(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Kind {
    Body,
    Div,
    P,
    Span,
    Br,
    Text(String),
}

/// A content element, timed as ttconv times it: its begin relative to its
/// parent's (`None`, 0) and its end relative to its parent's begin (`None`,
/// indefinite: its parent's end).
#[derive(Clone, Debug)]
struct Content {
    kind: Kind,
    id: String,
    /// The region its `region` attribute names, where one exists.
    region: Option<usize>,
    begin: Option<Ratio>,
    end: Option<Ratio>,
    styles: Specified,
    preserve: bool,
    children: Vec<Content>,
}

/// A TTML document, read.
#[derive(Debug)]
pub(crate) struct Document {
    regions: Vec<Region>,
    body: Option<Content>,
    /// How many bytes it was read from: what saying it may cost is bounded
    /// by it ([`WORK_PER_BYTE`]).
    size: usize,
}

/// What saying a document's paragraphs may cost, in nodes visited and bytes
/// of text gathered, for each byte of the document: [`Document::showings`]
/// gives up on a document that would take more, rather than be read for
/// minutes. A paragraph is gathered again wherever what it shows may
/// change, so a document's cost is its paragraphs' sizes times their
/// changes: about its size for subtitles, a few times it for a film whose
/// songs are karaoke timed syllable by syllable, and its size squared for
/// one made to be slow -- a paragraph of a hundred thousand spans each
/// beginning at its own time.
const WORK_PER_BYTE: usize = 32;

/// What any document may cost, however small: a short document -- a
/// streaming segment's few seconds -- is read whatever its timing.
const WORK_AT_LEAST: usize = 1 << 22;

/// One paragraph showing: its identifier, and its text as SRT markup.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Shown {
    pub id: String,
    pub text: String,
}

/// The parsing context ttconv carries from parent to child.
struct Context<'a> {
    params: &'a Parameters,
    styles: &'a Styles,
    /// Each region's place in the layout, by `xml:id`: of two with one
    /// identifier, the first.
    regions: &'a HashMap<&'a str, usize>,
}

/// The relative timing of an element being read, as ttconv's parsing
/// context keeps it.
struct Timing {
    seq: bool,
    desired_begin: Ratio,
    /// `None` for indefinite.
    implicit_end: Option<Ratio>,
}

impl Document {
    /// A document from a sample's bytes; `None` where it is no TTML
    /// document.
    pub(crate) fn read(bytes: &[u8]) -> Option<Self> {
        let tt = xml::parse(bytes).ok()?;
        if !tt.name.is(TT, "tt") {
            return None;
        }
        let params = Parameters::read(&tt);
        let head = tt.elements().find(|e| e.name.is(TT, "head"));
        let styles = head.map(read_styles).unwrap_or_default();
        let regions = head
            .map(|h| read_regions(h, &styles, &params))
            .unwrap_or_default();
        let mut by_id = HashMap::new();
        for (i, region) in regions.iter().enumerate() {
            by_id.entry(region.id.as_str()).or_insert(i);
        }
        let context = Context {
            params: &params,
            styles: &styles,
            regions: &by_id,
        };
        let preserve = tt.attribute(XML_NAMESPACE, "space") == Some("preserve");
        let root = Timing {
            seq: false,
            desired_begin: Ratio::ZERO,
            implicit_end: Some(Ratio::ZERO),
        };
        let body = tt
            .elements()
            .find(|e| e.name.is(TT, "body"))
            .and_then(|b| content(b, &context, &root, preserve))
            .map(|(c, _)| c);
        Some(Self {
            regions,
            body,
            size: bytes.len(),
        })
    }

    /// What the document shows from `from` to `to`, paragraph by paragraph:
    /// each stretch in which a paragraph shows the same in one region, as
    /// `(from, to, what)`, in the order they begin -- those beginning
    /// together region by region in the layout's order, then in the
    /// document's, as ttconv's documents of each moment list them. Exactly
    /// what `Document::at` shows at every moment between (a test holds the
    /// two together), at the cost of each paragraph gathered where it may
    /// change rather than the whole document at every change.
    ///
    /// `None` where it would cost more than [`WORK_PER_BYTE`] allows.
    pub(crate) fn showings(&self, from: Ratio, to: Ratio) -> Option<Vec<(Ratio, Ratio, Shown)>> {
        let Some(body) = &self.body else {
            return Some(Vec::new());
        };
        let mut found = Vec::new();
        find(
            body,
            &Ancestors {
                begin: Ratio::ZERO,
                end: None,
                region: None,
                styles: Specified::default(),
            },
            (from, to),
            &mut found,
        );
        let mut budget = self.size.saturating_mul(WORK_PER_BYTE).max(WORK_AT_LEAST);
        let root = Region::root();
        // Each stretch with its place among those beginning together.
        let mut out: Vec<(Ratio, usize, usize, Ratio, Shown)> = Vec::new();
        for (order, f) in found.iter().enumerate() {
            // The stretch it shows in, and inside it where what it shows may
            // change: its descendants' begins and ends.
            let start = f.begin.max(from);
            let stop = f.end.map_or(to, |e| e.min(to));
            let mut points = vec![start];
            let mut cost = 0usize;
            for child in &f.p.children {
                changes_inside(child, f.begin, f.end, (start, stop), &mut points, &mut cost);
            }
            points.sort_unstable();
            points.dedup();
            for selected in self.regions_of(f) {
                let region = selected.and_then(|i| self.regions.get(i)).unwrap_or(&root);
                let style = Computed::INITIAL.with(&region.styles).with(&f.styles);
                let mut last: Option<(Ratio, Ratio, String)> = None;
                for (k, &at) in points.iter().enumerate() {
                    let until = points.get(k.saturating_add(1)).copied().unwrap_or(stop);
                    budget = budget.checked_sub(cost.saturating_add(1))?;
                    let mut pieces = Vec::new();
                    for child in &f.p.children {
                        gather(
                            child,
                            at,
                            selected,
                            f.associated,
                            f.begin,
                            f.end,
                            style,
                            &mut pieces,
                        );
                    }
                    let text = render(pieces, style.text_align, region);
                    match (&mut last, text) {
                        (Some((_, end, shown)), Some(text)) if *shown == text => *end = until,
                        (_, text) => {
                            if let Some((a, b, text)) = last.take() {
                                out.push((a, selected.unwrap_or(0), order, b, f.shown(text)));
                            }
                            last = text.map(|text| (at, until, text));
                        }
                    }
                }
                if let Some((a, b, text)) = last {
                    out.push((a, selected.unwrap_or(0), order, b, f.shown(text)));
                }
            }
        }
        out.sort_unstable_by_key(|x| (x.0, x.1, x.2));
        Some(
            out.into_iter()
                .map(|(a, _, _, b, shown)| (a, b, shown))
                .collect(),
        )
    }

    /// The regions a paragraph shows in, in the layout's order, as ttconv
    /// prunes: with no layout, the root container (`None`); a paragraph
    /// associated with a region, that one; one associated with none, the
    /// regions its spans and breaks name, each showing only what is in it.
    fn regions_of(&self, f: &Found<'_>) -> Vec<Option<usize>> {
        if self.regions.is_empty() {
            return vec![None];
        }
        if let Some(i) = f.associated {
            return vec![Some(i)];
        }
        let mut named = std::collections::BTreeSet::new();
        regions_inside(&f.p.children, &mut named);
        named.into_iter().map(Some).collect()
    }

    /// When what shows changes, in order: every content element's begin and
    /// end (ttconv's significant times).
    #[cfg(test)]
    pub(crate) fn changes(&self) -> Vec<Ratio> {
        let mut times = Vec::new();
        if let Some(body) = &self.body {
            significant(body, Ratio::ZERO, None, &mut times);
        }
        times.sort();
        times.dedup();
        times
    }

    /// What shows at `t`: each paragraph showing, region by region in the
    /// document's order, each region's in the document's order -- ttconv's
    /// document of the moment, the whole document walked for it, as the
    /// reference [`Document::showings`] is held to.
    #[cfg(test)]
    pub(crate) fn at(&self, t: Ratio) -> Vec<Shown> {
        let mut out = Vec::new();
        let Some(body) = &self.body else {
            return out;
        };
        if self.regions.is_empty() {
            // No region: the root container is the one, and every element is
            // in it.
            let root = Region::root();
            show(
                body,
                t,
                None,
                None,
                Ratio::ZERO,
                None,
                Computed::INITIAL,
                &root,
                &mut out,
            );
        } else {
            for (i, region) in self.regions.iter().enumerate() {
                let inherited = Computed::INITIAL.with(&region.styles);
                show(
                    body,
                    t,
                    Some(i),
                    None,
                    Ratio::ZERO,
                    None,
                    inherited,
                    region,
                    &mut out,
                );
            }
        }
        out
    }
}

/// The head's styles by `xml:id`, looked up by a hash: a document's
/// references, each looked up by a walk of every style, would cost their
/// number times the styles'.
type Styles = HashMap<String, Specified>;

/// Every `<style>` of the head's `<styling>`, by `xml:id`: its own styles
/// over those it references -- those before it -- the last of those over
/// the earlier. Of two with one identifier, the first.
fn read_styles(head: &Element<'_>) -> Styles {
    let mut out = Styles::new();
    // The first `<styling>`, as ttconv reads only it.
    for styling in head.elements().filter(|e| e.name.is(TT, "styling")).take(1) {
        for style in styling.elements().filter(|e| e.name.is(TT, "style")) {
            let Some(id) = style.attribute(XML_NAMESPACE, "id") else {
                continue;
            };
            let s = specified(style, &out);
            out.entry(id.to_owned()).or_insert(s);
        }
    }
    out
}

/// An element's styles: its own `tts:` attributes over those its `style`
/// attribute references, the last over the earlier.
fn specified(e: &Element<'_>, styles: &Styles) -> Specified {
    let mut s = Specified::inline(e);
    for referenced in e
        .attribute("", "style")
        .unwrap_or("")
        .split_ascii_whitespace()
        .rev()
    {
        if let Some(r) = styles.get(referenced) {
            s = s.under(r);
        }
    }
    s
}

/// Every `<region>` of the head's `<layout>`, in order: its place in the
/// root container, and its styles -- its own, those it references, and
/// those of `<style>` elements inside it.
fn read_regions(head: &Element<'_>, styles: &Styles, params: &Parameters) -> Vec<Region> {
    let mut out = Vec::new();
    // The first `<layout>`, as ttconv reads only it.
    for layout in head.elements().filter(|e| e.name.is(TT, "layout")).take(1) {
        for region in layout.elements().filter(|e| e.name.is(TT, "region")) {
            let Some(id) = region.attribute(XML_NAMESPACE, "id") else {
                continue;
            };
            let mut s = specified(region, styles);
            for nested in region.elements().filter(|e| e.name.is(TT, "style")) {
                s = s.under(&Specified::inline(nested));
            }
            let fraction = |l: Length, horizontal: bool| -> Option<Ratio> {
                match l {
                    Length::Percent(p) => p.div(Ratio::whole(100)),
                    Length::Px(px) => {
                        let (w, h) = params.pixels?;
                        px.div(if horizontal { w } else { h })
                    }
                    Length::Cells(c) => c.div(if horizontal {
                        params.cells.0
                    } else {
                        params.cells.1
                    }),
                }
            };
            let (left, top) = s
                .origin
                .and_then(|(x, y)| Some((fraction(x, true)?, fraction(y, false)?)))
                .unwrap_or((Ratio::ZERO, Ratio::ZERO));
            let (width, height) = s
                .extent
                .and_then(|(w, h)| Some((fraction(w, true)?, fraction(h, false)?)))
                .unwrap_or((Ratio::whole(1), Ratio::whole(1)));
            out.push(Region {
                id: id.to_owned(),
                place: [left, top, width, height],
                display_align: s.display_align.unwrap_or(0),
                styles: s,
            });
        }
    }
    out
}

/// A content element read, as ttconv's `ContentElement.process` reads it,
/// with its timing relative to `parent`'s; and its desired end, for the
/// parent's. `None` for an element of no kind read here.
#[allow(
    clippy::too_many_lines,
    reason = "ttconv's process, step for step, is easier to hold to it whole"
)]
fn content(
    e: &Element<'_>,
    cx: &Context<'_>,
    parent: &Timing,
    preserve: bool,
) -> Option<(Content, Option<Ratio>)> {
    let kind = if *e.name.namespace != *TT {
        return None;
    } else {
        match e.name.local {
            "body" => Kind::Body,
            "div" => Kind::Div,
            "p" => Kind::P,
            "span" => Kind::Span,
            "br" => Kind::Br,
            _ => return None,
        }
    };
    let preserve = match e.attribute(XML_NAMESPACE, "space") {
        Some("preserve") => true,
        Some("default") => false,
        _ => preserve,
    };
    let time = |name: &str| e.attribute("", name).and_then(|v| cx.params.time(v));
    let explicit_begin = time("begin");
    let explicit_dur = time("dur");
    let explicit_end = time("end");
    let seq = e.attribute("", "timeContainer") == Some("seq");
    let implicit_begin = if parent.seq {
        parent.implicit_end?.sub(parent.desired_begin)?
    } else {
        Ratio::ZERO
    };
    let desired_begin = implicit_begin.add(explicit_begin.unwrap_or(Ratio::ZERO))?;
    let mut timing = Timing {
        seq,
        desired_begin,
        implicit_end: if kind == Kind::Br && !parent.seq {
            None
        } else {
            Some(desired_begin)
        },
    };
    let mixed = matches!(kind, Kind::P | Kind::Span);
    let mut children = Vec::new();
    for node in &e.children {
        match node {
            Node::Text(text) => {
                if mixed && !timing.seq {
                    children.push(Content {
                        kind: Kind::Text(text.as_ref().to_owned()),
                        id: String::new(),
                        region: None,
                        begin: None,
                        end: None,
                        styles: Specified::default(),
                        preserve,
                        children: Vec::new(),
                    });
                    timing.implicit_end = None;
                }
            }
            Node::Element(child) => {
                // What TTML's content model lets this element hold; ttconv
                // drops a child it does not allow with every child after
                // it, and its parent's own styles.
                let allowed = match kind {
                    Kind::Body => &["div"][..],
                    Kind::Div => &["div", "p"][..],
                    Kind::P | Kind::Span => &["span", "br"][..],
                    _ => &[][..],
                };
                if *child.name.namespace != *TT || !allowed.contains(&child.name.local) {
                    continue;
                }
                let Some((c, desired_end)) = content(child, cx, &timing, preserve) else {
                    continue;
                };
                // The child's end is on this element's clock; its implicit
                // end on its parent's. ttconv adds this element's begin for
                // a sequence and not for a parallel container, which ends
                // one that begins late early; TTML adds it to both.
                let end = desired_end.and_then(|end| end.add(timing.desired_begin));
                if timing.seq {
                    timing.implicit_end = end;
                } else {
                    timing.implicit_end = match (timing.implicit_end, end) {
                        (Some(a), Some(b)) => Some(a.max(b)),
                        _ => None,
                    };
                }
                // A child of no temporal extent is left out.
                let child_begin = c.begin.unwrap_or(Ratio::ZERO);
                if c.end != Some(child_begin) {
                    children.push(c);
                }
            }
        }
    }
    let desired_end = match (explicit_end, explicit_dur) {
        (Some(end), Some(dur)) => Some(desired_begin.add(dur)?.min(implicit_begin.add(end)?)),
        (None, Some(dur)) => Some(desired_begin.add(dur)?),
        (Some(end), None) => Some(implicit_begin.add(end)?),
        (None, None) => timing.implicit_end,
    };
    let region = e
        .attribute("", "region")
        .and_then(|id| cx.regions.get(id).copied());
    let c = Content {
        kind,
        id: e.attribute(XML_NAMESPACE, "id").unwrap_or("").to_owned(),
        region,
        begin: (desired_begin != Ratio::ZERO).then_some(desired_begin),
        end: desired_end,
        styles: specified(e, cx.styles),
        preserve,
        children,
    };
    Some((c, desired_end))
}

/// ttconv's `_make_absolute`: an element's begin and end on the document's
/// clock, from its parent's.
fn absolute(
    begin: Option<Ratio>,
    end: Option<Ratio>,
    parent_begin: Ratio,
    parent_end: Option<Ratio>,
) -> (Ratio, Option<Ratio>) {
    let b = parent_begin
        .add(begin.unwrap_or(Ratio::ZERO))
        .unwrap_or(parent_begin);
    let e = match end {
        None => parent_end,
        Some(end) => {
            let e = parent_begin.add(end).unwrap_or(parent_begin);
            Some(parent_end.map_or(e, |p| e.min(p)))
        }
    };
    (b, e)
}

/// What a paragraph's ancestors give it, as the walk down to it carries it.
#[derive(Clone, Copy)]
struct Ancestors {
    /// When the nearest begins and ends, on the document's clock (`None`:
    /// on and on).
    begin: Ratio,
    end: Option<Ratio>,
    /// The region the nearest associated with one is associated with.
    region: Option<usize>,
    /// Their styles, the nearer over the farther.
    styles: Specified,
}

/// A paragraph that may show in the stretch asked about.
struct Found<'a> {
    p: &'a Content,
    /// When it begins and ends on the document's clock, inside its
    /// ancestors' times.
    begin: Ratio,
    end: Option<Ratio>,
    /// The region it is associated with: its own, else its nearest
    /// ancestor's.
    associated: Option<usize>,
    /// Its styles over its ancestors': what it shows with, over its
    /// region's.
    styles: Specified,
}

impl Found<'_> {
    fn shown(&self, text: String) -> Shown {
        Shown {
            id: self.p.id.clone(),
            text,
        }
    }
}

/// The paragraphs in `c` that may show between `window`'s times, in the
/// document's order: of a temporal extent, overlapping the window,
/// displayed, and in no region other than their ancestors' -- as ttconv
/// prunes, an element naming another region than its ancestors' showing in
/// neither (in its own, they are pruned; in theirs, it is).
fn find<'a>(c: &'a Content, up: &Ancestors, window: (Ratio, Ratio), out: &mut Vec<Found<'a>>) {
    let (b, e) = absolute(c.begin, c.end, up.begin, up.end);
    if e.is_some_and(|e| e <= b) || b >= window.1 || e.is_some_and(|e| e <= window.0) {
        return;
    }
    if let (Some(own), Some(theirs)) = (c.region, up.region)
        && own != theirs
    {
        return;
    }
    if c.styles.display_none == Some(true) {
        return;
    }
    let here = Ancestors {
        begin: b,
        end: e,
        region: c.region.or(up.region),
        styles: c.styles.under(&up.styles),
    };
    match c.kind {
        Kind::P => out.push(Found {
            p: c,
            begin: b,
            end: e,
            associated: here.region,
            styles: here.styles,
        }),
        Kind::Body | Kind::Div => {
            for child in &c.children {
                find(child, &here, window, out);
            }
        }
        _ => {}
    }
}

/// The times strictly between `start` and `stop` at which `c` or anything
/// inside it begins or ends -- where what its paragraph shows may change --
/// and what gathering it costs: a node each, and each text's bytes.
fn changes_inside(
    c: &Content,
    parent_begin: Ratio,
    parent_end: Option<Ratio>,
    (start, stop): (Ratio, Ratio),
    points: &mut Vec<Ratio>,
    cost: &mut usize,
) {
    *cost = cost.saturating_add(1);
    if let Kind::Text(text) = &c.kind {
        *cost = cost.saturating_add(text.len());
    }
    let (b, e) = absolute(c.begin, c.end, parent_begin, parent_end);
    if e.is_some_and(|e| e <= b) {
        return;
    }
    for t in [Some(b), e].into_iter().flatten() {
        if start < t && t < stop {
            points.push(t);
        }
    }
    for child in &c.children {
        changes_inside(child, b, e, (start, stop), points, cost);
    }
}

/// The regions named inside a paragraph associated with none, each once: a
/// span's or a break's own. Inside an element naming one, any naming
/// another shows in neither, and is not looked for.
fn regions_inside(children: &[Content], named: &mut std::collections::BTreeSet<usize>) {
    for c in children {
        match c.region {
            Some(i) => {
                named.insert(i);
            }
            None => regions_inside(&c.children, named),
        }
    }
}

/// The times `c` and those within it begin and end.
#[cfg(test)]
fn significant(c: &Content, parent_begin: Ratio, parent_end: Option<Ratio>, out: &mut Vec<Ratio>) {
    let (b, e) = absolute(c.begin, c.end, parent_begin, parent_end);
    if e.is_some_and(|e| e <= b) {
        return;
    }
    out.push(b);
    out.extend(e);
    for child in &c.children {
        significant(child, b, e, out);
    }
}

/// A text node or a break of a paragraph showing, with its styles: what
/// the paragraph's white space is worked on with.
#[derive(Clone, Debug)]
enum Piece {
    Text {
        text: String,
        preserve: bool,
        style: Computed,
    },
    Break,
}

/// `c` at `t` in the region `selected` (`None`: the default region),
/// as ttconv's `_process_element` prunes and styles it; each paragraph
/// showing said into `out`.
#[cfg(test)]
#[allow(
    clippy::too_many_arguments,
    reason = "ttconv's _process_element's state, as it carries it"
)]
fn show(
    c: &Content,
    t: Ratio,
    selected: Option<usize>,
    inherited_region: Option<usize>,
    parent_begin: Ratio,
    parent_end: Option<Ratio>,
    inherited: Computed,
    region: &Region,
    out: &mut Vec<Shown>,
) {
    let (b, e) = absolute(c.begin, c.end, parent_begin, parent_end);
    if e.is_some_and(|e| e <= b) || b > t || e.is_some_and(|e| e <= t) {
        return;
    }
    let associated = c.region.or(inherited_region);
    if associated != selected && (c.children.is_empty() || associated.is_some()) {
        return;
    }
    if c.styles.display_none == Some(true) {
        return;
    }
    let style = inherited.with(&c.styles);
    if c.kind == Kind::P {
        let mut pieces = Vec::new();
        for child in &c.children {
            gather(child, t, selected, associated, b, e, style, &mut pieces);
        }
        if let Some(text) = render(pieces, style.text_align, region) {
            out.push(Shown {
                id: c.id.clone(),
                text,
            });
        }
        return;
    }
    for child in &c.children {
        show(child, t, selected, associated, b, e, style, region, out);
    }
}

/// The text and breaks of a paragraph's child `c` showing at `t`, in
/// document order.
#[allow(
    clippy::too_many_arguments,
    reason = "ttconv's _process_element's state, as it carries it"
)]
fn gather(
    c: &Content,
    t: Ratio,
    selected: Option<usize>,
    inherited_region: Option<usize>,
    parent_begin: Ratio,
    parent_end: Option<Ratio>,
    inherited: Computed,
    out: &mut Vec<Piece>,
) {
    let (b, e) = absolute(c.begin, c.end, parent_begin, parent_end);
    if e.is_some_and(|e| e <= b) || b > t || e.is_some_and(|e| e <= t) {
        return;
    }
    let associated = c.region.or(inherited_region);
    let leaf = matches!(c.kind, Kind::Text(_) | Kind::Br);
    if associated != selected && (leaf || c.children.is_empty() || associated.is_some()) {
        return;
    }
    if c.styles.display_none == Some(true) {
        return;
    }
    let style = if leaf {
        inherited
    } else {
        inherited.with(&c.styles)
    };
    match &c.kind {
        Kind::Text(text) => {
            if !text.is_empty() {
                out.push(Piece::Text {
                    text: text.clone(),
                    preserve: c.preserve,
                    style,
                });
            }
        }
        Kind::Br => out.push(Piece::Break),
        _ => {
            for child in &c.children {
                gather(child, t, selected, associated, b, e, style, out);
            }
        }
    }
}

/// ttconv's `_process_lwsp`, then this crate's SRT: a paragraph's pieces
/// as markup, placed in its region; `None` for one of only white space.
fn render(mut pieces: Vec<Piece>, text_align: Align, region: &Region) -> Option<String> {
    lwsp(&mut pieces);
    // Breaks -- a `<br/>`, or a line feed kept by `xml:space` -- never two
    // together, nor at either end; and hidden text not shown (it keeps its
    // place, where SRT has nowhere to keep it).
    let mut steps: Vec<Piece> = Vec::new();
    let push_break = |steps: &mut Vec<Piece>| {
        if !steps.is_empty() && !matches!(steps.last(), Some(Piece::Break)) {
            steps.push(Piece::Break);
        }
    };
    for piece in pieces {
        match piece {
            Piece::Break => push_break(&mut steps),
            Piece::Text { style, .. } if style.hidden => {}
            Piece::Text {
                text,
                preserve,
                style,
            } => {
                for (i, line) in text.split('\n').enumerate() {
                    if i > 0 {
                        push_break(&mut steps);
                    }
                    if !line.is_empty() {
                        steps.push(Piece::Text {
                            text: line.to_owned(),
                            preserve,
                            style,
                        });
                    }
                }
            }
        }
    }
    while matches!(steps.last(), Some(Piece::Break)) {
        steps.pop();
    }
    let only_space = steps.iter().all(|p| match p {
        Piece::Text { text, .. } => text.chars().all(char::is_whitespace),
        Piece::Break => true,
    });
    if only_space {
        return None;
    }
    let mut writer = Writer::new();
    let an = placement(text_align, region);
    if an != 2 {
        writer.op(Op::Align(an));
    }
    let mut now = Computed::INITIAL;
    for step in steps {
        match step {
            Piece::Break => writer.op(Op::Break),
            Piece::Text { text, style, .. } => {
                // The colour first, the outermost tag, as ttconv's runs
                // have it; then each toggle that differs.
                let rgb = |c: Colour| {
                    let [r, g, b, _] = c.0;
                    (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)
                };
                let (want, have) = (rgb(style.colour), rgb(now.colour));
                if want != have {
                    writer.op(Op::Colour((want != rgb(WHITE)).then_some(want)));
                }
                for (want, have, toggle) in [
                    (style.bold, now.bold, Toggle::Bold),
                    (style.italic, now.italic, Toggle::Italic),
                    (style.underline, now.underline, Toggle::Underline),
                    (style.strike, now.strike, Toggle::Strike),
                ] {
                    if want != have {
                        writer.op(Op::Toggle(toggle, want));
                    }
                }
                now = style;
                writer.op(Op::Text(super::srt::escape(&text, true)));
            }
        }
    }
    Some(writer.finish())
}

/// ttconv's `_process_lwsp`: white space collapsed, and dropped at a
/// paragraph's ends and around its breaks, where `xml:space` is `default`.
fn lwsp(pieces: &mut Vec<Piece>) {
    let is_lwsp = |c: char| matches!(c, '\t' | '\r' | '\n' | ' ');
    // First: runs of white space one space, and a leading one dropped at the
    // paragraph's start or after white space or a break; a piece left empty
    // dropped. One pass, the pieces built afresh: each removed from the
    // middle in its turn would move every piece after it.
    let mut kept: Vec<Piece> = Vec::with_capacity(pieces.len());
    for piece in pieces.drain(..) {
        let Piece::Text {
            text,
            preserve: false,
            style,
        } = piece
        else {
            kept.push(piece);
            continue;
        };
        let after_space = kept.last().is_none_or(|p| match p {
            Piece::Break => true,
            Piece::Text { text, .. } => text.ends_with(is_lwsp),
        });
        let mut collapsed = String::with_capacity(text.len());
        let mut in_space = false;
        for c in text.chars() {
            if is_lwsp(c) {
                if !in_space {
                    collapsed.push(' ');
                }
                in_space = true;
            } else {
                collapsed.push(c);
                in_space = false;
            }
        }
        if after_space && collapsed.starts_with(' ') {
            collapsed.remove(0);
        }
        if !collapsed.is_empty() {
            kept.push(Piece::Text {
                text: collapsed,
                preserve: false,
                style,
            });
        }
    }
    *pieces = kept;
    // Then: a trailing space dropped at the paragraph's end, or before a
    // break or text beginning a line.
    let len = pieces.len();
    for i in 0..len {
        let next_begins_line = pieces.get(i.saturating_add(1)).is_none_or(|p| match p {
            Piece::Break => true,
            Piece::Text { text, .. } => text.starts_with(['\r', '\n']),
        });
        if let Some(Piece::Text { text, preserve, .. }) = pieces.get_mut(i)
            && !*preserve
            && text.ends_with(' ')
            && next_begins_line
        {
            text.pop();
        }
    }
}

/// Where a paragraph goes, as `{\anN}` says it: the third of the picture
/// each way its anchor falls in -- across, its region's left edge, middle
/// or right edge as `textAlign` puts the text; down, its region's top,
/// middle or bottom as `displayAlign` puts the lines.
fn placement(text_align: Align, region: &Region) -> u8 {
    let [left, top, width, height] = region.place;
    let at = |start: Ratio, size: Ratio, align: Align| {
        Ratio::new(i128::from(align), 2)
            .and_then(|f| size.mul(f))
            .and_then(|off| start.add(off))
            .unwrap_or(start)
    };
    let third = |v: Ratio| -> u8 {
        let one = Ratio::new(1, 3).unwrap_or(Ratio::ZERO);
        let two = Ratio::new(2, 3).unwrap_or(Ratio::ZERO);
        if v < one {
            0
        } else if v < two {
            1
        } else {
            2
        }
    };
    let column = third(at(left, width, text_align));
    let row = third(at(top, height, region.display_align));
    // A numeric keypad: 7 8 9 at the top, 1 2 3 at the bottom.
    match (row, column) {
        (0, c) => 7u8.saturating_add(c),
        (1, c) => 4u8.saturating_add(c),
        (_, c) => 1u8.saturating_add(c),
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        clippy::format_collect,
        reason = "a test: a failure should be loud, and its sizes are small"
    )]

    use super::*;

    fn secs(n: i128, d: i128) -> Ratio {
        Ratio::new(n, d).unwrap()
    }

    fn params(tt: &str) -> Parameters {
        let doc = format!(r#"<tt xmlns="{TT}" xmlns:ttp="{TTP}" xmlns:tts="{TTS}" {tt}/>"#);
        Parameters::read(&xml::parse(doc.as_bytes()).unwrap())
    }

    #[test]
    fn time_expressions_are_read_exactly() {
        let p = params(
            r#"ttp:frameRate="30" ttp:frameRateMultiplier="1000 1001" ttp:tickRate="10000000""#,
        );
        assert_eq!(p.time("1.5s"), Some(secs(3, 2)));
        assert_eq!(p.time("2h"), Some(secs(7200, 1)));
        assert_eq!(p.time("90m"), Some(secs(5400, 1)));
        assert_eq!(p.time("250ms"), Some(secs(1, 4)));
        assert_eq!(p.time("30f"), Some(secs(1001, 1000)));
        assert_eq!(p.time("15000000t"), Some(secs(3, 2)));
        assert_eq!(p.time("00:01:02.5"), Some(secs(125, 2)));
        assert_eq!(
            p.time("00:00:01:15"),
            Some(Ratio::whole(1).add(secs(15 * 1001, 30_000)).unwrap())
        );
        assert_eq!(p.time("100:00:00"), Some(secs(360_000, 1)));
        for bad in [
            "",
            "1",
            "1x",
            "s",
            "1.s",
            "0:00:01",
            "00:0:01",
            "00:00:1",
            "00:00:01:",
            "-1s",
        ] {
            assert_eq!(p.time(bad), None, "{bad:?}");
        }
        // ttconv's defaults: 30 frames a second, one tick a second.
        let d = params("");
        assert_eq!(d.time("15f"), Some(secs(1, 2)));
        assert_eq!(d.time("3t"), Some(secs(3, 1)));
    }

    #[test]
    fn nanoseconds_are_the_nearest() {
        assert_eq!(secs(1, 3).to_ns(), 333_333_333);
        assert_eq!(secs(2, 3).to_ns(), 666_666_667);
        assert_eq!(secs(1001, 30_000).to_ns(), 33_366_667);
        assert_eq!(Ratio::from_ns(1_500_000_000), secs(3, 2));
        // A half rounds up, below zero too.
        assert_eq!(secs(1, 2_000_000_000).to_ns(), 1);
        assert_eq!(secs(-1, 2_000_000_000).to_ns(), 0);
        assert_eq!(secs(-3, 2_000_000_000).to_ns(), -1);
        assert_eq!(secs(-1, 3).to_ns(), -333_333_333);
        // Far past what a time can be: the ends of an i64.
        assert_eq!(secs(i128::MAX, 1).to_ns(), i64::MAX);
        assert_eq!(secs(-i128::MAX, 1).to_ns(), i64::MIN);
    }

    #[test]
    fn nanoseconds_are_exact_however_large_the_terms() {
        let ten = |n: u32| 10i128.pow(n);
        // A second and a hair, in thirty digits: a product of the numerator
        // and a billion would overflow, which read it as a fifth of a
        // second.
        let p = params("");
        let hair = p.time("1.000000000000000000000000000001s").unwrap();
        assert_eq!(hair, secs(ten(30) + 1, ten(30)));
        assert_eq!(hair.to_ns(), 1_000_000_000);
        // Half a nanosecond and a hair rounds up; a hair under, down.
        assert_eq!(secs(5 * ten(28) + 1, ten(38)).to_ns(), 1);
        assert_eq!(secs(5 * ten(28) - 1, ten(38)).to_ns(), 0);
        assert_eq!(
            p.time("0.000000000499999999999999999999s").unwrap().to_ns(),
            0
        );
        assert_eq!(
            p.time("0.000000000500000000000000000000s").unwrap().to_ns(),
            1
        );
        assert_eq!(
            p.time("3.999999999500000000000000000000s").unwrap().to_ns(),
            4_000_000_000
        );
        // A denominator near the largest there is.
        assert_eq!(secs(i128::MAX - 1, i128::MAX).to_ns(), 1_000_000_000);
        assert_eq!(secs(1, i128::MAX).to_ns(), 0);
        assert_eq!(mul_div(7, 1_000_000_000, 9), (777_777_777, 7));
        let big = u128::try_from(i128::MAX).unwrap();
        assert_eq!(
            mul_div(big - 1, 1_000_000_000, big),
            (999_999_999, big - 1_000_000_000)
        );
    }

    #[test]
    fn times_order_exactly_however_large_the_terms() {
        use core::cmp::Ordering;
        let ten = |n: u32| 10i128.pow(n);
        // Each product here overflows: saturated, they would all be equal.
        let a = secs(i128::MAX, ten(30));
        let b = secs(i128::MAX - 2, ten(30));
        let c = secs(i128::MAX / 3, ten(30) + 7);
        assert_eq!(a.cmp(&b), Ordering::Greater);
        assert_eq!(b.cmp(&a), Ordering::Less);
        assert_eq!(a.cmp(&a), Ordering::Equal);
        assert_eq!(c.cmp(&b), Ordering::Less);
        assert_eq!(
            secs(ten(30) + 1, ten(30)).cmp(&Ratio::whole(1)),
            Ordering::Greater
        );
        assert_eq!(
            secs(ten(30) - 1, ten(30)).cmp(&Ratio::whole(1)),
            Ordering::Less
        );
        assert_eq!(secs(-1, 3).cmp(&secs(-1, 2)), Ordering::Greater);
        assert_eq!(secs(-7, 2).cmp(&secs(-10, 3)), Ordering::Less);
        // Neighbours of a continued fraction several steps deep.
        assert_eq!(
            secs(355, 113).cmp(&secs(103_993, 33_102)),
            Ordering::Greater
        );
        assert_eq!(
            secs(103_993, 33_102).cmp(&secs(104_348, 33_215)),
            Ordering::Less
        );
        // And a sort of such times is one, as a document's changes are
        // sorted -- each against the others by value.
        let mut times = [
            a,
            c,
            b,
            Ratio::ZERO,
            secs(ten(30) + 1, ten(30)),
            Ratio::whole(1),
            secs(-1, 3),
            secs(ten(30) - 1, ten(30)),
        ];
        times.sort();
        for pair in times.windows(2) {
            assert_eq!(pair[0].cmp(&pair[1]), Ordering::Less, "{pair:?}");
        }
        assert_eq!(times[0], secs(-1, 3));
        assert_eq!(times[7], a);
    }

    #[test]
    fn colours_are_named_hex_and_functions() {
        assert_eq!(colour("red"), Some(Colour([255, 0, 0, 255])));
        assert_eq!(colour("#00ff0080"), Some(Colour([0, 255, 0, 128])));
        assert_eq!(colour("#0000FF"), Some(Colour([0, 0, 255, 255])));
        assert_eq!(colour("rgb(1, 2, 3)"), Some(Colour([1, 2, 3, 255])));
        assert_eq!(colour("rgba(1,2,3,4)"), Some(Colour([1, 2, 3, 4])));
        for bad in ["", "#12345", "rgb(1,2)", "rgb(256,0,0)", "chartreuse"] {
            assert_eq!(colour(bad), None, "{bad:?}");
        }
    }

    fn doc(body: &str) -> Document {
        let text = format!(
            r#"<tt xmlns="{TT}" xmlns:tts="{TTS}" xmlns:ttp="{TTP}"><head><styling><style xml:id="b" tts:fontWeight="bold"/><style xml:id="i" tts:fontStyle="italic"/><style xml:id="red" tts:color="red" tts:fontStyle="normal"/></styling><layout><region xml:id="bottom" tts:origin="10% 80%" tts:extent="80% 15%" tts:displayAlign="after" tts:textAlign="center"/><region xml:id="top" tts:origin="10% 5%" tts:extent="80% 15%" tts:textAlign="center"/></layout></head>{body}</tt>"#
        );
        Document::read(text.as_bytes()).unwrap()
    }

    fn texts(d: &Document, t: Ratio) -> Vec<String> {
        d.at(t).into_iter().map(|s| s.text).collect()
    }

    #[test]
    fn a_paragraph_shows_in_its_region_from_its_begin_to_its_end() {
        let d = doc(
            r#"<body region="bottom"><div><p xml:id="a" begin="1s" end="2s">one</p><p begin="1.5s" dur="1s">two</p></div></body>"#,
        );
        assert_eq!(
            d.changes(),
            [secs(0, 1), secs(1, 1), secs(3, 2), secs(2, 1), secs(5, 2)]
        );
        assert!(texts(&d, secs(1, 2)).is_empty());
        assert_eq!(texts(&d, secs(1, 1)), ["one"]);
        assert_eq!(texts(&d, secs(7, 4)), ["one", "two"]);
        assert_eq!(texts(&d, secs(2, 1)), ["two"]);
        assert!(texts(&d, secs(5, 2)).is_empty());
        assert_eq!(d.at(secs(1, 1))[0].id, "a");
    }

    #[test]
    fn a_paragraph_naming_another_region_than_its_body_shows_in_none() {
        let d = doc(
            r#"<body region="bottom"><div><p begin="0s" end="1s" region="top">lost</p><p begin="0s" end="1s">found</p></div></body>"#,
        );
        assert_eq!(texts(&d, Ratio::ZERO), ["found"]);
    }

    #[test]
    fn a_paragraph_names_its_own_region_where_the_body_names_none() {
        let d = doc(
            r#"<body><div><p begin="0s" end="1s" region="top">up</p><p begin="0s" end="1s" region="bottom">down</p><p begin="0s" end="1s">nowhere</p></div></body>"#,
        );
        // Region by region in the document's order: bottom, then top.
        assert_eq!(texts(&d, Ratio::ZERO), ["down", "{\\an8}up"]);
    }

    #[test]
    fn styles_inline_over_referenced_and_the_last_reference_over_the_first() {
        let d = doc(
            r#"<body region="bottom"><div><p begin="0s" end="1s" style="i red"><span style="b">x</span> <span tts:color="lime">y</span></p></div></body>"#,
        );
        // `red` after `i`: not italic. Bold, then the lime span -- a colour
        // inside the colour before, as the writer switches colours.
        assert_eq!(
            texts(&d, Ratio::ZERO),
            ["<font color=\"#ff0000\"><b>x</b> <font color=\"#00ff00\">y</font></font>"]
        );
    }

    #[test]
    fn white_space_is_ttconvs() {
        let d = doc(concat!(
            r#"<body region="bottom"><div>"#,
            r#"<p begin="0s" end="1s">  two   spaces  </p>"#,
            r#"<p begin="1s" end="2s">before <br/> after</p>"#,
            r#"<p begin="2s" end="3s" xml:space="preserve">  kept   spaces  </p>"#,
            r#"<p begin="3s" end="4s">a<br/><br/>b<br/></p>"#,
            r#"<p begin="4s" end="5s">   </p>"#,
            r#"</div></body>"#
        ));
        assert_eq!(texts(&d, secs(0, 1)), ["two spaces"]);
        assert_eq!(texts(&d, secs(1, 1)), ["before\nafter"]);
        assert_eq!(texts(&d, secs(2, 1)), ["  kept   spaces  "]);
        assert_eq!(texts(&d, secs(3, 1)), ["a\nb"]);
        assert!(texts(&d, secs(4, 1)).is_empty());
    }

    #[test]
    fn text_lasts_as_long_as_its_parent_and_a_seq_puts_children_in_turn() {
        let d = doc(
            r#"<body region="bottom"><div timeContainer="seq"><p dur="1s">first</p><p dur="2s">second</p></div><div begin="10s"><p>whole div</p><p end="1s">a second</p></div></body>"#,
        );
        assert_eq!(texts(&d, secs(1, 2)), ["first"]);
        assert_eq!(texts(&d, secs(5, 2)), ["second"]);
        assert!(texts(&d, secs(3, 1)).is_empty());
        // The div lasts as long as its longest child, a second: text in a
        // paragraph with no time of its own lasts as long as that.
        assert_eq!(texts(&d, secs(10, 1)), ["whole div", "a second"]);
    }

    /// A parallel container that begins late lasts until its last child
    /// ends, on its own clock: TTML's timing, where ttconv ends it at the
    /// child's end read on its parent's clock -- here at 3.5 s, before the
    /// paragraph begins.
    #[test]
    fn a_container_beginning_late_lasts_until_its_last_child_ends() {
        let d = doc(
            r#"<body region="bottom"><div begin="1s"><p begin="2.5s" end="3.5s">late</p></div></body>"#,
        );
        assert_eq!(texts(&d, secs(7, 2)), ["late"]);
        assert_eq!(texts(&d, secs(9, 2)), Vec::<String>::new());
        assert_eq!(d.changes().last(), Some(&secs(9, 2)));
    }

    /// An element where TTML does not allow it is passed over alone: a
    /// paragraph in the body, a span in a div, a div in a paragraph.
    #[test]
    fn an_element_where_ttml_allows_none_is_passed_over_alone() {
        let d = doc(concat!(
            r#"<body region="bottom">"#,
            r#"<p begin="0s" end="1s">in the body</p>"#,
            r#"<div><span begin="0s" end="1s">in a div</span>"#,
            r#"<p begin="0s" end="1s">kept<div>in a paragraph</div> after</p></div>"#,
            r#"</body>"#
        ));
        assert_eq!(texts(&d, Ratio::ZERO), ["kept after"]);
    }

    #[test]
    fn only_the_first_styling_and_layout_are_read() {
        let text = format!(
            r#"<tt xmlns="{TT}" xmlns:tts="{TTS}"><head><styling><style xml:id="s" tts:fontWeight="bold"/></styling><styling><style xml:id="t" tts:fontStyle="italic"/></styling><layout><region xml:id="r" tts:origin="0% 80%" tts:extent="100% 20%" tts:displayAlign="after" tts:textAlign="center"/></layout><layout><region xml:id="q"/></layout></head><body region="r"><div><p begin="0s" end="1s" style="s t">x</p><p begin="0s" end="1s" region="q">y</p></div></body></tt>"#
        );
        let d = Document::read(text.as_bytes()).unwrap();
        // `t`'s italic is in the second styling, not read; and `q` is in the
        // second layout, so `y` names a region that is not there -- as if
        // it named none, it is in its body's.
        assert_eq!(texts(&d, Ratio::ZERO), ["<b>x</b>", "y"]);
    }

    #[test]
    fn a_span_shows_from_its_own_begin() {
        let d = doc(
            r#"<body region="bottom"><div><p begin="0s" end="2s">a<span begin="1s"> b</span></p></div></body>"#,
        );
        assert_eq!(texts(&d, secs(1, 2)), ["a"]);
        assert_eq!(texts(&d, secs(3, 2)), ["a b"]);
        assert!(d.changes().contains(&secs(1, 1)));
    }

    #[test]
    fn a_region_s_place_is_said_in_thirds() {
        let region = |origin: (i128, i128), extent: (i128, i128), display: Align| Region {
            id: String::new(),
            place: [
                secs(origin.0, 100),
                secs(origin.1, 100),
                secs(extent.0, 100),
                secs(extent.1, 100),
            ],
            display_align: display,
            styles: Specified::default(),
        };
        // Bottom centre: no tag.
        assert_eq!(placement(1, &region((10, 80), (80, 15), 2)), 2);
        assert_eq!(placement(1, &region((10, 5), (80, 15), 0)), 8);
        assert_eq!(placement(0, &region((0, 0), (100, 100), 0)), 7);
        assert_eq!(placement(2, &region((0, 40), (100, 20), 1)), 6);
        assert_eq!(placement(0, &region((0, 0), (100, 100), 1)), 4);
        assert_eq!(placement(0, &region((100, 0), (0, 0), 0)), 9);
        // An anchor exactly at a third is in the next: a line centred in a
        // region two thirds wide from the left edge is anchored at 1/3
        // across, and one in a region two thirds high from the top, its
        // lines at the middle, at 1/3 down.
        let thirds = Region {
            id: String::new(),
            place: [Ratio::ZERO, Ratio::ZERO, secs(2, 3), secs(2, 3)],
            display_align: 1,
            styles: Specified::default(),
        };
        assert_eq!(placement(1, &thirds), 5);
        assert_eq!(placement(0, &thirds), 4);
    }

    #[test]
    fn hidden_text_and_display_none_show_nothing() {
        let d = doc(
            r#"<body region="bottom"><div><p begin="0s" end="1s">seen <span tts:visibility="hidden">hidden</span></p><p begin="0s" end="1s" tts:display="none">gone</p></div></body>"#,
        );
        assert_eq!(texts(&d, Ratio::ZERO), ["seen "]);
    }

    #[test]
    fn what_is_not_a_ttml_document_is_none() {
        assert!(Document::read(b"<tt/>").is_none(), "no namespace");
        assert!(Document::read(b"not xml").is_none());
        assert!(Document::read(format!(r#"<tt xmlns="{TT}"/>"#).as_bytes()).is_some());
    }

    /// A small generator of numbers, the same every run.
    struct Rng(u64);

    impl Rng {
        fn below(&mut self, n: usize) -> usize {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            usize::try_from(self.0 % u64::try_from(n).unwrap()).unwrap()
        }

        fn chance(&mut self, percent: usize) -> bool {
            self.below(100) < percent
        }

        fn pick<'a>(&mut self, from: &[&'a str]) -> &'a str {
            from[self.below(from.len())]
        }
    }

    /// Styles an element or a style might give, at random.
    fn random_styles(rng: &mut Rng, percent: usize) -> String {
        let mut out = String::new();
        for (name, values) in [
            ("color", &["red", "white", "#00ff00", "rgb(0,0,255)"][..]),
            ("fontWeight", &["bold", "normal"][..]),
            ("fontStyle", &["italic", "oblique", "normal"][..]),
            (
                "textDecoration",
                &["underline", "lineThrough", "noUnderline", "none"][..],
            ),
            ("visibility", &["hidden", "visible"][..]),
            (
                "textAlign",
                &["start", "center", "end", "left", "right"][..],
            ),
        ] {
            if rng.chance(percent) {
                out += &format!(r#" tts:{name}="{}""#, rng.pick(values));
            }
        }
        out
    }

    /// Timing, a region, styles and white space an element might have.
    fn random_attributes(rng: &mut Rng, regions: usize) -> String {
        const TIMES: [&str; 12] = [
            "0s",
            "1s",
            "1.5s",
            "2s",
            "3s",
            "500ms",
            "4s",
            "25f",
            "15t",
            "00:00:02.5",
            "00:00:01:05",
            "6s",
        ];
        let mut a = String::new();
        match rng.below(7) {
            0 | 1 => {}
            2 => a += &format!(r#" begin="{}""#, rng.pick(&TIMES)),
            3 => a += &format!(r#" end="{}""#, rng.pick(&TIMES)),
            4 => a += &format!(r#" dur="{}""#, rng.pick(&TIMES)),
            5 => {
                a += &format!(
                    r#" begin="{}" end="{}""#,
                    rng.pick(&TIMES),
                    rng.pick(&TIMES)
                );
            }
            _ => {
                a += &format!(
                    r#" begin="{}" dur="{}""#,
                    rng.pick(&TIMES),
                    rng.pick(&TIMES)
                );
            }
        }
        if rng.chance(15) {
            a += r#" timeContainer="seq""#;
        }
        if rng.chance(30) {
            let r = rng.below(regions + 1);
            a += &if r == regions {
                r#" region="zz""#.to_owned()
            } else {
                format!(r#" region="r{r}""#)
            };
        }
        if rng.chance(20) {
            a += &format!(r#" style="s{}""#, rng.below(3));
        }
        a += &random_styles(rng, 8);
        if rng.chance(5) {
            a += r#" tts:display="none""#;
        }
        if rng.chance(10) {
            a += r#" xml:space="preserve""#;
        }
        a
    }

    fn random_element(rng: &mut Rng, kind: &str, depth: usize, regions: usize) -> String {
        const WORDS: [&str; 7] = ["a", " b ", "  c\n d ", "e", "&lt;f&gt;", " ", "g\n"];
        let mut children = String::new();
        let count = match kind {
            "body" | "div" => 1 + rng.below(3),
            _ => rng.below(5),
        };
        for _ in 0..count {
            children += &match kind {
                "body" => random_element(rng, "div", depth + 1, regions),
                "div" if depth < 3 && rng.chance(30) => {
                    random_element(rng, "div", depth + 1, regions)
                }
                "div" => random_element(rng, "p", depth + 1, regions),
                _ => match rng.below(5) {
                    0 | 1 if depth < 6 => random_element(rng, "span", depth + 1, regions),
                    2 => {
                        if rng.chance(20) {
                            format!(r#"<br region="r{}"/>"#, rng.below(regions.max(1)))
                        } else {
                            "<br/>".to_owned()
                        }
                    }
                    _ => rng.pick(&WORDS).to_owned(),
                },
            };
        }
        let mut a = random_attributes(rng, regions);
        if kind == "p" && rng.chance(70) {
            // Now and then two alike.
            a += &format!(r#" xml:id="p{}""#, rng.below(6));
        }
        format!("<{kind}{a}>{children}</{kind}>")
    }

    /// A document of random styles, regions and content, timed at random.
    fn random_document(rng: &mut Rng) -> String {
        let regions = rng.below(4);
        let mut head = String::from("<styling>");
        for i in 0..3 {
            let reference = if i > 0 && rng.chance(40) {
                format!(r#" style="s{}""#, rng.below(i))
            } else {
                String::new()
            };
            head += &format!(
                r#"<style xml:id="s{i}"{reference}{}/>"#,
                random_styles(rng, 30)
            );
        }
        head += "</styling>";
        if regions > 0 {
            head += "<layout>";
            for i in 0..regions {
                let (x, y) = (rng.below(80), rng.below(80));
                let (w, h) = (10 + rng.below(20), 10 + rng.below(20));
                let down = rng.pick(&["before", "center", "after"]);
                head += &format!(
                    r#"<region xml:id="r{i}" tts:origin="{x}% {y}%" tts:extent="{w}% {h}%" tts:displayAlign="{down}"{}/>"#,
                    random_styles(rng, 20)
                );
            }
            head += "</layout>";
        }
        let body = random_element(rng, "body", 0, regions);
        format!(
            r#"<tt xmlns="{TT}" xmlns:tts="{TTS}" xmlns:ttp="{TTP}" ttp:frameRate="25" ttp:tickRate="10"><head>{head}</head>{body}</tt>"#
        )
    }

    /// Whether `part`, in order, is in `whole`.
    fn in_order(part: &[&Shown], whole: &[Shown]) -> bool {
        let mut rest = whole.iter();
        part.iter().all(|p| rest.any(|w| w == *p))
    }

    /// What a document shows paragraph by paragraph is what it shows at
    /// every moment: at each change between the times asked about, the
    /// stretches covering it are ttconv's document of that moment -- the
    /// whole document walked for it -- and those beginning then come in its
    /// order.
    #[test]
    fn a_documents_stretches_are_what_it_shows_at_every_moment() {
        let mut rng = Rng(0x2545_F491_4F6C_DD1D);
        let windows = [
            (Ratio::ZERO, Ratio::whole(20)),
            (Ratio::whole(1), Ratio::whole(3)),
            (secs(5, 2), secs(9, 2)),
            (secs(1, 3), secs(7, 1)),
        ];
        let mut shown = 0usize;
        for _ in 0..4000 {
            let text = random_document(&mut rng);
            let d = Document::read(text.as_bytes()).unwrap();
            for (from, to) in windows {
                let stretches = d.showings(from, to).unwrap();
                for (a, b, _) in &stretches {
                    assert!(from <= *a && a < b && *b <= to, "{text}\n{a:?} {b:?}");
                }
                let mut moments = vec![from];
                moments.extend(d.changes().into_iter().filter(|&c| from < c && c < to));
                for t in moments {
                    let mut want = d.at(t);
                    let beginning: Vec<&Shown> = stretches
                        .iter()
                        .filter(|s| s.0 == t)
                        .map(|s| &s.2)
                        .collect();
                    assert!(
                        in_order(&beginning, &want),
                        "{text}\nat {t:?}: {beginning:?} not in order in {want:?}"
                    );
                    let mut got: Vec<Shown> = stretches
                        .iter()
                        .filter(|s| s.0 <= t && t < s.1)
                        .map(|s| s.2.clone())
                        .collect();
                    let by_text = |x: &Shown, y: &Shown| (&x.id, &x.text).cmp(&(&y.id, &y.text));
                    want.sort_by(by_text);
                    got.sort_by(by_text);
                    assert_eq!(got, want, "{text}\nat {t:?}");
                    shown += got.len();
                }
                // A paragraph's stretches of showing the same are one.
                for (i, x) in stretches.iter().enumerate() {
                    assert!(
                        !stretches[i + 1..]
                            .iter()
                            .any(|y| y.0 == x.1 && y.2 == x.2 && !x.2.id.is_empty()),
                        "{text}\n{x:?} goes on in another stretch"
                    );
                }
            }
        }
        // The documents show something often enough to have tested it.
        assert!(shown > 10_000, "{shown}");
    }

    /// Paragraphs each at its own time cost each its own: a hundred
    /// thousand of them are not the whole document walked at each of their
    /// two hundred thousand changes.
    #[test]
    fn many_paragraphs_cost_each_its_own() {
        let body: String = (0..100_000)
            .map(|i| format!(r#"<p begin="{i}s" end="{}s">line {i}</p>"#, i + 1))
            .collect();
        let text = format!(r#"<tt xmlns="{TT}"><body><div>{body}</div></body></tt>"#);
        let d = Document::read(text.as_bytes()).unwrap();
        let started = std::time::Instant::now();
        let stretches = d.showings(Ratio::ZERO, Ratio::whole(200_000)).unwrap();
        assert_eq!(stretches.len(), 100_000);
        // No layout: the root container's top left.
        assert_eq!(stretches[99_999].2.text, "{\\an7}line 99999");
        assert!(started.elapsed() < std::time::Duration::from_secs(10));
    }

    /// One paragraph of six thousand spans, each beginning at its own time,
    /// would be said at each of their six thousand changes, all of it each
    /// time -- a hundred million steps for 170 KB: a document made to be
    /// slow, given up on.
    #[test]
    fn a_paragraph_made_to_be_slow_is_given_up_on() {
        let spans: String = (0..6000)
            .map(|i| format!(r#"<span begin="{i}s">x</span>"#))
            .collect();
        let text = format!(r#"<tt xmlns="{TT}"><body><div><p>{spans}</p></div></body></tt>"#);
        let d = Document::read(text.as_bytes()).unwrap();
        let started = std::time::Instant::now();
        assert_eq!(d.showings(Ratio::ZERO, Ratio::whole(10_000)), None);
        assert!(started.elapsed() < std::time::Duration::from_secs(10));
        // A few hundred such spans -- karaoke, a syllable at a time -- are
        // said.
        let syllables: String = (0..300)
            .map(|i| format!(r#"<span begin="{i}s">la </span>"#))
            .collect();
        let text = format!(
            r#"<tt xmlns="{TT}"><body><div><p end="400s">{syllables}</p></div></body></tt>"#
        );
        let d = Document::read(text.as_bytes()).unwrap();
        assert_eq!(
            d.showings(Ratio::ZERO, Ratio::whole(400)).unwrap().len(),
            300
        );
    }

    /// Styles and regions are looked up by identifier, not by walking every
    /// one: fifty thousand elements each referencing one of fifty thousand
    /// styles and regions.
    #[test]
    fn many_styles_and_regions_are_looked_up_at_once() {
        let n = 50_000;
        let styles: String = (0..n)
            .map(|i| format!(r#"<style xml:id="s{i}" tts:fontWeight="bold"/>"#))
            .collect();
        let regions: String = (0..n)
            .map(|i| format!(r#"<region xml:id="r{i}"/>"#))
            .collect();
        let body: String = (0..n)
            .map(|i| {
                format!(
                    r#"<p begin="{i}s" end="{}s" style="s{}" region="r{}">x</p>"#,
                    i + 1,
                    n - 1 - i,
                    n - 1 - i
                )
            })
            .collect();
        let text = format!(
            r#"<tt xmlns="{TT}" xmlns:tts="{TTS}"><head><styling>{styles}</styling><layout>{regions}</layout></head><body><div>{body}</div></body></tt>"#
        );
        let started = std::time::Instant::now();
        let d = Document::read(text.as_bytes()).unwrap();
        let stretches = d
            .showings(Ratio::ZERO, Ratio::whole(i128::from(n)))
            .unwrap();
        assert_eq!(stretches.len(), usize::try_from(n).unwrap());
        assert!(stretches[0].2.text.contains("<b>"));
        assert!(started.elapsed() < std::time::Duration::from_secs(10));
    }

    /// White space in a paragraph of a hundred thousand pieces, most of them
    /// white space alone, is worked on at once: each dropped in its turn,
    /// every piece after it moved.
    #[test]
    fn white_space_in_many_pieces_is_worked_on_at_once() {
        let mut pieces = Vec::new();
        for i in 0..100_000 {
            pieces.push(Piece::Text {
                text: if i % 100 == 0 {
                    "word ".to_owned()
                } else {
                    "   ".to_owned()
                },
                preserve: false,
                style: Computed::INITIAL,
            });
        }
        let started = std::time::Instant::now();
        lwsp(&mut pieces);
        assert_eq!(pieces.len(), 1000);
        assert!(started.elapsed() < std::time::Duration::from_secs(10));
    }
}
