//! One character cell, as the wide-character ncurses keeps it: `cchar_t`,
//! with the attribute bits and the macros of `curses.priv.h` that read and
//! write it.
//!
//! A cell holds up to [`CCHARW_MAX`] wide characters -- a spacing one and
//! the non-spacing ones combined with it -- its attributes, and its colour
//! pair. With extended colours (ncurses 6's ABI) the pair lives in
//! `ext_color`, and the attribute's own colour bits keep a copy of it cut to
//! 255 for programs that read pairs from attributes; [`Cell::pair`] prefers
//! `ext_color` and falls back to those bits, as `GetPair` does.
//!
//! The attribute's low eight bits, which a `chtype` uses for the character,
//! are free in a `cchar_t`; ncurses uses them to mark the cells of a
//! character wider than one column -- 1 for its first cell, 2 and up for the
//! cells after it ([`Cell::widec_ext`]).

/// `attr_t`: an attribute word, 32 bits as `chtype` is in ncurses 6's ABI.
pub type Attr = u32;

/// `NCURSES_ATTR_SHIFT`: where the attributes start, above the character.
const ATTR_SHIFT: u32 = 8;

/// `NCURSES_BITS (mask, shift)`.
const fn bits(mask: u32, shift: u32) -> Attr {
    mask.wrapping_shl(shift.wrapping_add(ATTR_SHIFT))
}

/// `A_NORMAL`.
pub const A_NORMAL: Attr = 0;
/// `A_ATTRIBUTES`: every bit above the character's.
pub const A_ATTRIBUTES: Attr = bits(!0, 0);
/// `A_CHARTEXT`: the character's bits.
pub const A_CHARTEXT: Attr = bits(1, 0) - 1;
/// `A_COLOR`: the colour pair's bits.
pub const A_COLOR: Attr = bits((1 << 8) - 1, 0);
/// `A_STANDOUT`.
pub const A_STANDOUT: Attr = bits(1, 8);
/// `A_UNDERLINE`.
pub const A_UNDERLINE: Attr = bits(1, 9);
/// `A_REVERSE`.
pub const A_REVERSE: Attr = bits(1, 10);
/// `A_BLINK`.
pub const A_BLINK: Attr = bits(1, 11);
/// `A_DIM`.
pub const A_DIM: Attr = bits(1, 12);
/// `A_BOLD`.
pub const A_BOLD: Attr = bits(1, 13);
/// `A_ALTCHARSET`.
pub const A_ALTCHARSET: Attr = bits(1, 14);
/// `A_INVIS`.
pub const A_INVIS: Attr = bits(1, 15);
/// `A_PROTECT`.
pub const A_PROTECT: Attr = bits(1, 16);
/// `A_HORIZONTAL`.
pub const A_HORIZONTAL: Attr = bits(1, 17);
/// `A_LEFT`.
pub const A_LEFT: Attr = bits(1, 18);
/// `A_LOW`.
pub const A_LOW: Attr = bits(1, 19);
/// `A_RIGHT`.
pub const A_RIGHT: Attr = bits(1, 20);
/// `A_TOP`.
pub const A_TOP: Attr = bits(1, 21);
/// `A_VERTICAL`.
pub const A_VERTICAL: Attr = bits(1, 22);
/// `A_ITALIC`, an ncurses extension.
pub const A_ITALIC: Attr = bits(1, 23);

/// `ALL_BUT_COLOR`.
pub const ALL_BUT_COLOR: Attr = !A_COLOR;
/// `NONBLANK_ATTR`: what changes how a blank looks.
pub const NONBLANK_ATTR: Attr = A_BOLD | A_DIM | A_BLINK | A_ITALIC;
/// `TPARM_ATTR`: what `set_attributes` (`sgr`) can set.
pub const TPARM_ATTR: Attr = A_STANDOUT
    | A_UNDERLINE
    | A_REVERSE
    | A_BLINK
    | A_DIM
    | A_BOLD
    | A_ALTCHARSET
    | A_INVIS
    | A_PROTECT;
/// `XMC_CONFLICT`: what a magic-cookie terminal cannot place freely.
pub const XMC_CONFLICT: Attr = A_STANDOUT
    | A_UNDERLINE
    | A_REVERSE
    | A_BLINK
    | A_DIM
    | A_BOLD
    | A_INVIS
    | A_PROTECT
    | A_ITALIC;

/// `CCHARW_MAX`: the characters one cell holds.
pub const CCHARW_MAX: usize = 5;

/// `wchar_t`, a signed 32-bit integer in glibc as in ours.
pub type WChar = i32;

/// `COLOR_PAIR (n)` / `ColorPair (n)`: the pair's bits in an attribute
/// word, as `(chtype) n` shifted into place -- a pair past 255 keeps only
/// its low eight bits there.
#[must_use]
pub const fn color_pair(n: i32) -> Attr {
    bits(n.cast_unsigned(), 0) & A_COLOR
}

/// `PAIR_NUMBER (a)` / `PairNumber (a)`.
#[must_use]
pub const fn pair_number(a: Attr) -> i32 {
    // At most 255 after the mask and shift, so the conversion is exact.
    ((a & A_COLOR) >> ATTR_SHIFT).cast_signed()
}

/// `oldColor (p)`: a pair as the attribute bits can hold it.
const fn old_color(p: i32) -> i32 {
    if p > 255 { 255 } else { p }
}

/// `cchar_t`: one character cell.
///
/// Equality is `CharEq`: the attribute word, all five characters and the
/// colour pair, field by field.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Cell {
    /// `attr`: the attributes, the colour pair's low bits, and in the
    /// character bits the wide-character marker ([`Cell::widec_ext`]).
    pub attr: Attr,
    /// `chars`: the spacing character and any non-spacing ones, ended by a
    /// zero when there are fewer than [`CCHARW_MAX`].
    pub chars: [WChar; CCHARW_MAX],
    /// `ext_color`: the colour pair, whole.
    pub ext_color: i32,
}

/// `BLANK_TEXT`.
pub const BLANK_TEXT: WChar = 0x20;

impl Cell {
    /// `NewChar2 (c, a)`: the character `c` with the attribute word `a` as
    /// it stands, colour pair 0 in `ext_color`.
    #[must_use]
    pub const fn new2(c: WChar, a: Attr) -> Self {
        Self {
            attr: a,
            chars: [c, 0, 0, 0, 0],
            ext_color: 0,
        }
    }

    /// `BLANK`: a space with no attributes.
    #[must_use]
    pub const fn blank() -> Self {
        Self::new2(BLANK_TEXT, A_NORMAL)
    }

    /// `SetChar (ch, c, a)`: everything cleared, then the character `c`,
    /// the attributes `a`, and the pair `a` carries.
    pub fn set_char(&mut self, c: WChar, a: Attr) {
        *self = Self::default();
        self.chars[0] = c;
        self.attr = a;
        self.set_pair(pair_number(a));
    }

    /// The same as [`Cell::set_char`], as a new cell.
    #[must_use]
    pub fn with_char(c: WChar, a: Attr) -> Self {
        let mut cell = Self::default();
        cell.set_char(c, a);
        cell
    }

    /// `SetChar2 (wch, ch)`: from a `chtype` -- its character and its
    /// attributes.
    #[must_use]
    pub fn from_chtype(ch: Attr) -> Self {
        Self::with_char((ch & A_CHARTEXT).cast_signed(), ch & A_ATTRIBUTES)
    }

    /// `CharOf (c)`: the spacing character.
    #[must_use]
    pub const fn ch(&self) -> WChar {
        self.chars[0]
    }

    /// `GetPair (value)`: `ext_color`, or when that is 0 the pair in the
    /// attribute bits.
    #[must_use]
    pub const fn pair(&self) -> i32 {
        if self.ext_color != 0 {
            self.ext_color
        } else {
            pair_number(self.attr)
        }
    }

    /// `SetPair (value, p)`: `p` in `ext_color`, and as much of it as fits in
    /// the attribute bits.
    pub const fn set_pair(&mut self, p: i32) {
        self.ext_color = p;
        self.attr = (self.attr & ALL_BUT_COLOR) | color_pair(old_color(p));
    }

    /// `AddAttr (c, a)`.
    pub const fn add_attr(&mut self, a: Attr) {
        self.attr |= a & A_ATTRIBUTES;
    }

    /// `RemAttr (c, a)`.
    pub const fn rem_attr(&mut self, a: Attr) {
        self.attr &= !(a & A_ATTRIBUTES);
    }

    /// `SetAttr (c, a)`: the attributes replaced, the wide-character marker
    /// kept.
    pub const fn set_attr(&mut self, a: Attr) {
        self.attr = (a & A_ATTRIBUTES) | self.attr & A_CHARTEXT;
    }

    /// `unColor (c)`: the attribute word without its colour bits.
    #[must_use]
    pub const fn uncolored(&self) -> Attr {
        self.attr & ALL_BUT_COLOR
    }

    /// `WidecExt (ch)`: 0 for an ordinary cell, 1 for the first cell of a
    /// wide character, 2 and up for the cells after it.
    #[must_use]
    pub const fn widec_ext(&self) -> i32 {
        (self.attr & A_CHARTEXT).cast_signed()
    }

    /// `isWidecBase (ch)`.
    #[must_use]
    pub const fn is_widec_base(&self) -> bool {
        self.widec_ext() == 1
    }

    /// `isWidecExt (ch)`: a cell after the first of a wide character.
    #[must_use]
    pub const fn is_widec_ext(&self) -> bool {
        let e = self.widec_ext();
        e > 1 && e < 32
    }

    /// `SetWidecExt (dst, ext)`: mark the cell as the `ext`th (from 0) of a
    /// wide character.
    pub const fn set_widec_ext(&mut self, ext: i32) {
        self.attr &= !A_CHARTEXT;
        self.attr |= ext.wrapping_add(1).cast_unsigned();
    }

    /// `ISBLANK (ch)`: a space with nothing combined.
    #[must_use]
    pub const fn is_blank(&self) -> bool {
        self.chars[0] == BLANK_TEXT && self.chars[1] == 0
    }

    /// `SameAttrOf (a, b)`: the same attribute word and the same pair.
    #[must_use]
    pub const fn same_attr(&self, other: &Self) -> bool {
        self.attr == other.attr && self.pair() == other.pair()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bits_are_ncurses_6s() {
        // From the curses.h a Debian-configured ncursesw 6.4 generates.
        assert_eq!(A_ATTRIBUTES, 0xffff_ff00);
        assert_eq!(A_CHARTEXT, 0xff);
        assert_eq!(A_COLOR, 0xff00);
        assert_eq!(A_STANDOUT, 1 << 16);
        assert_eq!(A_REVERSE, 1 << 18);
        assert_eq!(A_BOLD, 1 << 21);
        assert_eq!(A_ALTCHARSET, 1 << 22);
        assert_eq!(A_ITALIC, 1 << 31);
        assert_eq!(color_pair(3), 0x300);
        assert_eq!(color_pair(256), 0, "only the low eight bits fit");
        assert_eq!(pair_number(0x1234_5600 | A_BOLD), 0x56);
    }

    #[test]
    fn a_pair_lives_whole_in_ext_color_and_cut_in_the_attributes() {
        let mut c = Cell::blank();
        c.set_pair(300);
        assert_eq!(c.ext_color, 300);
        assert_eq!(pair_number(c.attr), 255);
        assert_eq!(c.pair(), 300);
        c.set_pair(0);
        assert_eq!(c.pair(), 0);
        // A pair only in the attribute bits is still read.
        let c = Cell::new2(0x41, color_pair(7));
        assert_eq!(c.ext_color, 0);
        assert_eq!(c.pair(), 7);
    }

    #[test]
    fn set_char_takes_the_pair_from_the_attributes() {
        let c = Cell::with_char(0x41, A_BOLD | color_pair(5));
        assert_eq!(c.ch(), 0x41);
        assert_eq!(c.ext_color, 5);
        assert_eq!(c.attr, A_BOLD | color_pair(5));
        let d = Cell::from_chtype(0x42 | A_REVERSE);
        assert_eq!((d.ch(), d.attr, d.pair()), (0x42, A_REVERSE, 0));
    }

    #[test]
    fn wide_characters_are_marked_in_the_character_bits() {
        let mut c = Cell::with_char(0x4e2d, A_BOLD);
        assert_eq!(c.widec_ext(), 0);
        c.set_widec_ext(0);
        assert!(c.is_widec_base() && !c.is_widec_ext());
        c.set_widec_ext(1);
        assert!(c.is_widec_ext() && !c.is_widec_base());
        // Setting the attributes keeps the marker.
        c.set_attr(A_REVERSE);
        assert_eq!(c.attr, A_REVERSE | 2);
    }

    #[test]
    fn blanks_and_equality() {
        assert!(Cell::blank().is_blank());
        let mut combined = Cell::blank();
        combined.chars[1] = 0x301;
        assert!(!combined.is_blank());
        assert_ne!(combined, Cell::blank());
        let mut colored = Cell::blank();
        colored.ext_color = 2;
        assert_ne!(colored, Cell::blank(), "the pair counts");
        assert!(!colored.same_attr(&Cell::blank()));
    }
}
