//! The terminal as a screen drives it: the description, the output buffer,
//! where the cursor really is and what attributes are really on, and the
//! colour tables -- the `SCREEN` fields `lib_tputs.c`, `lib_vid_attr.c`,
//! `lib_color.c` and `lib_dft_fgbg.c` work on.
//!
//! Output goes into a buffer of `(2 + lines) * (6 + columns)` bytes, flushed
//! with `write (2)` when it fills and when the library says so, as
//! `_nc_outch` and `_nc_flush` do; the bytes are the same either way, only
//! when they leave changes.
//!
//! Colour pairs keep their two colours and whether they are in use. Upstream
//! also threads them on a list, and indexes them in a tree, for
//! `alloc_pair`, `find_pair` and `free_pair`, which this crate does not
//! offer; nothing a program can see of `init_pair` depends on either.

use terminfo::{Entry, Outc, Padding, Tparm};

use crate::caps::{boolean, number, string};
use crate::cell::{
    A_ALTCHARSET, A_ATTRIBUTES, A_BLINK, A_BOLD, A_DIM, A_HORIZONTAL, A_INVIS, A_ITALIC, A_LEFT,
    A_LOW, A_NORMAL, A_PROTECT, A_REVERSE, A_RIGHT, A_STANDOUT, A_TOP, A_UNDERLINE, A_VERTICAL,
    ALL_BUT_COLOR, Attr, Cell, TPARM_ATTR,
};

/// `COLOR_DEFAULT`: a pair's colour that is the terminal's own.
pub const COLOR_DEFAULT: i32 = -1;
/// `COLOR_BLACK`.
pub const COLOR_BLACK: i32 = 0;
/// `COLOR_WHITE`.
pub const COLOR_WHITE: i32 = 7;

/// `isDefaultColor (c)`.
const fn is_default_color(c: i32) -> bool {
    c < 0
}

/// `color_t`: one colour of the palette.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Color {
    /// What `color_content` answers.
    pub red: i32,
    pub green: i32,
    pub blue: i32,
    /// What `init_color` was given.
    pub r: i32,
    pub g: i32,
    pub b: i32,
    /// `init_color` set it.
    pub init: bool,
}

const fn data(r: i32, g: i32, b: i32) -> Color {
    Color {
        red: r,
        green: g,
        blue: b,
        r: 0,
        g: 0,
        b: 0,
        init: false,
    }
}

/// `RGB_ON`.
const RGB_ON: i32 = 680;

/// `cga_palette`.
const CGA_PALETTE: [Color; 8] = [
    data(0, 0, 0),
    data(RGB_ON, 0, 0),
    data(0, RGB_ON, 0),
    data(RGB_ON, RGB_ON, 0),
    data(0, 0, RGB_ON),
    data(RGB_ON, 0, RGB_ON),
    data(0, RGB_ON, RGB_ON),
    data(RGB_ON, RGB_ON, RGB_ON),
];

/// `hls_palette`.
const HLS_PALETTE: [Color; 8] = [
    data(0, 0, 0),
    data(120, 50, 100),
    data(240, 50, 100),
    data(180, 50, 100),
    data(330, 50, 100),
    data(60, 50, 100),
    data(300, 50, 100),
    data(0, 50, 100),
];

/// `cpFREE`, `cpINIT`, `cpKEEP`: a pair's use.
pub const CP_FREE: i32 = 0;
pub const CP_INIT: i32 = 1;
pub const CP_KEEP: i32 = -1;

/// `colorpair_t`, as much of it as this crate needs (module docs).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ColorPair {
    pub fg: i32,
    pub bg: i32,
    pub mode: i32,
}

/// `isSamePair (a, b)`.
const fn is_same_pair(a: ColorPair, b: ColorPair) -> bool {
    a.fg == b.fg && a.bg == b.bg
}

/// `ACS_LEN`: the size of the alternate character set's maps, indexed by
/// the `acsc` letter.
pub const ACS_LEN: usize = 128;

/// `INFINITY` in the cost computations: an operation not to be used.
pub const INFINITY: i32 = 1_000_000;

/// The movement and update costs `_nc_mvcur_init` computes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(
    missing_docs,
    reason = "each is upstream's `SCREEN` field of the same name"
)]
pub struct Costs {
    pub char_padding: i32,
    pub cr_cost: i32,
    pub cup_cost: i32,
    pub home_cost: i32,
    pub ll_cost: i32,
    pub cub1_cost: i32,
    pub cuf1_cost: i32,
    pub cud1_cost: i32,
    pub cuu1_cost: i32,
    pub cub_cost: i32,
    pub cuf_cost: i32,
    pub cud_cost: i32,
    pub cuu_cost: i32,
    pub hpa_cost: i32,
    pub vpa_cost: i32,
    pub ed_cost: i32,
    pub el_cost: i32,
    pub el1_cost: i32,
    pub dch1_cost: i32,
    pub ich1_cost: i32,
    pub dch_cost: i32,
    pub ich_cost: i32,
    pub ech_cost: i32,
    pub rep_cost: i32,
    pub hpa_ch_cost: i32,
    pub cup_ch_cost: i32,
    pub cuf_ch_cost: i32,
    pub inline_cost: i32,
    pub smir_cost: i32,
    pub rmir_cost: i32,
    pub ip_cost: i32,
}

/// Where a screen's output goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Sink {
    /// `write (2)` to this descriptor: upstream's `_ofd`.
    Fd(i32),
    /// Kept, every byte in order: for rendering a screen nobody is shown --
    /// the tests' way to see exactly what a terminal would be sent.
    Memory(Vec<u8>),
}

/// A screen's output: where it goes and the buffer it is collected in --
/// `_ofd`, `out_buffer`, `out_inuse` and `out_limit`.
///
/// A struct of its own so that what is written to it can be borrowed from
/// the rest of the [`Term`] -- a capability from the entry, an expansion
/// from `tparm`'s buffer -- rather than copied out first: copies the
/// borrow checker would otherwise force, and that a signal handler writing
/// the screen cannot afford, the allocator being perhaps the very thing it
/// interrupted (TD-B-CURSES-SIGNAL-WORK-ALLOCATES-IN-A-HANDLER).
pub struct Output {
    /// Where the bytes go: `_ofd`, or memory.
    pub sink: Sink,
    /// `out_buffer` (its length `out_inuse`), reserved at `out_limit` when
    /// the screen is sized and never grown past it.
    pub buf: Vec<u8>,
    /// `out_limit`; 0 before the screen is sized, when every byte is
    /// written as it comes.
    pub limit: usize,
}

impl Output {
    /// `_nc_outch (ch)`: one byte into the buffer, which is flushed first
    /// when it is full.
    pub fn outch(&mut self, b: u8) {
        if self.limit == 0 {
            self.write_out(&[b]);
            return;
        }
        if self.buf.len().saturating_add(1) >= self.limit {
            self.flush();
        }
        self.buf.push(b);
    }

    /// `_nc_flush ()`: the buffer written, every byte, retrying an
    /// interrupted or would-block write; abandoned at any other error. The
    /// buffer keeps its allocation.
    pub fn flush(&mut self) {
        match &mut self.sink {
            Sink::Fd(fd) => write_all(*fd, &self.buf),
            Sink::Memory(kept) => kept.extend_from_slice(&self.buf),
        }
        self.buf.clear();
    }

    /// `write (_ofd, …)` of bytes that bypass the buffer, until they are all
    /// out or it fails.
    fn write_out(&mut self, buf: &[u8]) {
        match &mut self.sink {
            Sink::Fd(fd) => write_all(*fd, buf),
            Sink::Memory(kept) => kept.extend_from_slice(buf),
        }
    }

    /// `tputs (string, affcnt, _nc_outch)` into this output, padded as
    /// `padding` says; `delays` is whether a delay that is not mandatory
    /// applies (the screen's to work out: [`Term::tputs_always`]).
    pub fn tputs(&mut self, string: &[u8], affcnt: i32, padding: &Padding, delays: bool) {
        terminfo::tputs_with(string, affcnt, padding, self, delays);
    }
}

/// Bytes on their way to a screen's buffer, for `tputs`.
impl Outc for Output {
    fn put(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.outch(b);
        }
    }

    fn flush(&mut self) {
        Output::flush(self);
    }
}

/// The terminal side of a screen.
#[allow(
    clippy::struct_excessive_bools,
    reason = "upstream's SCREEN, field for field"
)]
pub struct Term {
    /// The description, with the screen's size in `lines` and `cols`, and
    /// the capabilities the screen cancels cancelled.
    pub entry: Entry,
    /// `tparm`'s static variables, and the buffer it expands into.
    pub tparm: Tparm,
    /// How delays are padded: speed, `PC`, `npc`.
    pub padding: Padding,
    /// `_no_padding`: `NCURSES_NO_PADDING` was set.
    pub no_padding: bool,
    /// Where the output goes, and its buffer.
    pub output: Output,
    /// `screen_lines`, `screen_columns`.
    pub lines: i32,
    pub columns: i32,
    /// `_cursrow`, `_curscol`: where the cursor really is; -1 for unknown.
    pub cursrow: i32,
    pub curscol: i32,
    /// `*_current_attr`: the attributes and pair really on.
    pub current_attr: Cell,
    /// `_cursor`: the cursor's visibility as `curs_set` set it, -1 unknown.
    pub cursor: i32,
    pub costs: Costs,
    /// `_address_cursor`: `cup`, else `mrcup` -- the index of the one the
    /// terminal has, as upstream points at the terminal's own string.
    pub address_cursor: Option<usize>,
    /// `_scrolling`: the terminal can scroll a region at all.
    pub scrolling: bool,
    /// `_use_rmso`, `_use_rmul`, `_use_ritm`: those differ from `sgr0`.
    pub use_rmso: bool,
    pub use_rmul: bool,
    pub use_ritm: bool,
    /// `_ok_attributes`.
    pub ok_attributes: Attr,
    /// `_xmc_suppress`, `_xmc_triggers`.
    pub xmc_suppress: Attr,
    pub xmc_triggers: Attr,
    /// `_acs_map`, `_screen_acs_map`.
    pub acs_map: Vec<Attr>,
    pub screen_acs_map: Vec<bool>,
    /// `_nc_wacs`: the line-drawing characters as Unicode has them, by
    /// their `acsc` letters.
    pub wacs: Vec<Cell>,
    /// `_screen_unicode`, `_screen_acs_fix`, `_legacy_coding`.
    pub screen_unicode: bool,
    pub screen_acs_fix: bool,
    pub legacy_coding: i32,
    /// `_nc_sp_idlok`, `_nc_sp_idcok`.
    pub idlok: bool,
    pub idcok: bool,
    /// `_coloron`, `_color_defs`.
    pub coloron: bool,
    pub color_defs: i32,
    /// `_color_table`, `_color_count`.
    pub color_table: Vec<Color>,
    pub color_count: i32,
    /// `_color_pairs` (its length `_pair_alloc`), `_pair_count`,
    /// `_pair_limit`.
    pub color_pairs: Vec<ColorPair>,
    pub pair_count: i32,
    pub pair_limit: i32,
    /// `_pairs_used`.
    pub pairs_used: i32,
    /// `_direct_color`: bits of red, green, blue in a direct colour.
    pub direct_color: (u8, u8, u8),
    /// `_assumed_color`, `_default_color`, `_has_sgr_39_49`.
    pub assumed_color: bool,
    pub default_color: bool,
    pub has_sgr_39_49: bool,
    /// `_default_fg`, `_default_bg`, `_default_pairs`.
    pub default_fg: i32,
    pub default_bg: i32,
    pub default_pairs: i32,
}

impl Term {
    /// The terminal `entry`, padded as `padding` says, written to `sink` --
    /// with everything else as a new `SCREEN` has it: the zeros
    /// `_nc_alloc_screen_sp` allocates, `SP_PRE_INIT`'s unknown cursor
    /// position and visibility, idcok on, and the default colours this build
    /// assumes (`USE_ASSUMED_COLOR`: white on black). The output buffer is
    /// sized later, once the screen's size is known.
    #[must_use]
    pub fn new(entry: Entry, padding: Padding, sink: Sink) -> Self {
        Self {
            entry,
            tparm: Tparm::new(),
            padding,
            no_padding: false,
            output: Output {
                sink,
                buf: Vec::new(),
                limit: 0,
            },
            lines: 0,
            columns: 0,
            cursrow: -1,
            curscol: -1,
            current_attr: Cell::default(),
            cursor: -1,
            costs: Costs::default(),
            address_cursor: None,
            scrolling: false,
            use_rmso: false,
            use_rmul: false,
            use_ritm: false,
            ok_attributes: 0,
            xmc_suppress: 0,
            xmc_triggers: 0,
            acs_map: vec![0; ACS_LEN],
            screen_acs_map: vec![false; ACS_LEN],
            wacs: vec![Cell::default(); ACS_LEN],
            screen_unicode: false,
            screen_acs_fix: false,
            legacy_coding: 0,
            idlok: false,
            idcok: true,
            coloron: false,
            color_defs: 0,
            color_table: Vec::new(),
            color_count: 0,
            color_pairs: Vec::new(),
            pair_count: 0,
            pair_limit: 0,
            pairs_used: 0,
            direct_color: (0, 0, 0),
            assumed_color: false,
            default_color: false,
            has_sgr_39_49: false,
            default_fg: COLOR_WHITE,
            default_bg: COLOR_BLACK,
            default_pairs: 0,
        }
    }

    /// `out_limit` set, and the buffer made that large: `malloc
    /// (sp->out_limit)` in `_nc_setupscreen`. Reserved once, so that adding
    /// to it never allocates -- which keeps a signal handler that writes
    /// through it (`endwin` from `handle_SIGINT`) off the allocator.
    pub fn set_out_limit(&mut self, limit: usize) {
        self.output.limit = limit;
        self.output.buf = Vec::with_capacity(limit);
    }

    /// Whether the terminal has a string capability.
    #[must_use]
    pub fn has(&self, index: usize) -> bool {
        self.entry.string(index).is_some()
    }

    /// A boolean capability.
    #[must_use]
    pub fn flag(&self, index: usize) -> bool {
        self.entry.flag(index)
    }

    /// A numeric capability (-1 when absent, as `ABSENT_NUMERIC`).
    #[must_use]
    pub fn num(&self, index: usize) -> i32 {
        self.entry.number(index)
    }

    /// `_nc_outch (ch)`: one byte into the buffer, which is flushed first
    /// when it is full.
    pub fn outch(&mut self, b: u8) {
        self.output.outch(b);
    }

    /// `_nc_flush ()`: the buffer written; see [`Output::flush`].
    pub fn flush(&mut self) {
        self.output.flush();
    }

    /// What a [`Sink::Memory`] screen has written so far, taken: the bytes a
    /// terminal would have been sent, flushed or not.
    pub fn take_written(&mut self) -> Vec<u8> {
        self.flush();
        match &mut self.output.sink {
            Sink::Fd(_) => Vec::new(),
            Sink::Memory(kept) => std::mem::take(kept),
        }
    }

    /// Whether a delay that is not mandatory is padded: `normal_delay` --
    /// no `xon`, a `pb` that is not 0 (an absent one is -1, which C counts as
    /// true), no `NCURSES_NO_PADDING`, and a speed at least `pb`.
    fn normal_delay(&self) -> bool {
        let pb = self.num(number::PADDING_BAUD_RATE);
        !self.flag(boolean::XON_XOFF) && pb != 0 && !self.no_padding && self.padding.baud >= pb
    }

    /// `tputs (string, affcnt, _nc_outch)`, through the screen. `always`
    /// is upstream's `string == bell || string == flash_screen`: the
    /// terminal's own `bell` or `flash` string, whose delays are always
    /// padded.
    pub fn tputs_always(&mut self, string: &[u8], affcnt: i32, always: bool) {
        let apply = always || self.normal_delay();
        let padding = self.padding;
        self.output.tputs(string, affcnt, &padding, apply);
    }

    /// `tputs (string, affcnt, _nc_outch)`.
    pub fn tputs(&mut self, string: &[u8], affcnt: i32) {
        self.tputs_always(string, affcnt, false);
    }

    /// `NCURSES_PUTP2 (name, value)`: a string sent, if there is one.
    pub fn putp(&mut self, value: Option<&[u8]>) {
        if let Some(v) = value {
            self.tputs(v, 1);
        }
    }

    /// `NCURSES_PUTP2` of the string capability at `index`: sent, straight
    /// from the entry, if the terminal has it -- and whether it does.
    pub fn putp_cap(&mut self, index: usize) -> bool {
        self.tputs_cap(index, 1)
    }

    /// `tputs (cap, affcnt, _nc_outch)` of the string capability at
    /// `index`, straight from the entry, if the terminal has it -- and
    /// whether it does.
    pub fn tputs_cap(&mut self, index: usize, affcnt: i32) -> bool {
        self.tputs_cap_always(index, affcnt, false)
    }

    /// [`Term::tputs_cap`], with every delay padded when `always` -- for
    /// `bel` and `flash`, as [`Term::tputs_always`].
    pub fn tputs_cap_always(&mut self, index: usize, affcnt: i32, always: bool) -> bool {
        let apply = always || self.normal_delay();
        let padding = self.padding;
        match self.entry.string(index) {
            Some(v) => {
                self.output.tputs(v, affcnt, &padding, apply);
                true
            }
            None => false,
        }
    }

    /// `TPUTS (TIPARM_n (cap, …), affcnt)` of the string capability at
    /// `index`, the expansion sent straight from the buffer `tparm` keeps:
    /// whether there was one -- false when the terminal does not have the
    /// capability, or it cannot be expanded.
    pub fn put_tiparm(&mut self, index: usize, params: &[i64], affcnt: i32) -> bool {
        let apply = self.normal_delay();
        let padding = self.padding;
        let Some(cap) = self.entry.string(index) else {
            return false;
        };
        match self.tparm.expand(&self.entry, cap, params) {
            Some(s) => {
                self.output.tputs(s, affcnt, &padding, apply);
                true
            }
            None => false,
        }
    }

    /// `TIPARM_n (cap, …)` of the string capability at `index`, copied:
    /// `None` when the terminal does not have it or it cannot be expanded.
    /// For setting up a screen, where a copy is kept; the writing paths use
    /// [`Term::put_tiparm`] and [`Term::expand`], which copy nothing.
    pub fn tiparm(&mut self, index: usize, params: &[i64]) -> Option<Vec<u8>> {
        let s = self.entry.string(index)?;
        self.tparm
            .expand(&self.entry, s, params)
            .map(<[u8]>::to_vec)
    }

    /// `TIPARM_n (cap, …)` of the string capability at `index`, in the
    /// buffer `tparm` keeps until the next expansion: `None` when the
    /// terminal does not have it or it cannot be expanded.
    pub fn expand(&mut self, index: usize, params: &[i64]) -> Option<&[u8]> {
        let s = self.entry.string(index)?;
        self.tparm.expand(&self.entry, s, params)
    }

    /// `_nc_baudrate (ospeed)` of the screen's terminal.
    #[must_use]
    pub const fn baudrate(&self) -> i32 {
        self.padding.baud
    }

    /// `termattrs ()`: the attributes the terminal can show.
    #[must_use]
    pub fn termattrs(&self) -> Attr {
        let mut attrs = A_NORMAL;
        if self.has(string::ENTER_ALT_CHARSET_MODE) {
            attrs |= A_ALTCHARSET;
        }
        if self.has(string::ENTER_BLINK_MODE) {
            attrs |= A_BLINK;
        }
        if self.has(string::ENTER_BOLD_MODE) {
            attrs |= A_BOLD;
        }
        if self.has(string::ENTER_DIM_MODE) {
            attrs |= A_DIM;
        }
        if self.has(string::ENTER_REVERSE_MODE) {
            attrs |= A_REVERSE;
        }
        if self.has(string::ENTER_STANDOUT_MODE) {
            attrs |= A_STANDOUT;
        }
        if self.has(string::ENTER_PROTECTED_MODE) {
            attrs |= A_PROTECT;
        }
        if self.has(string::ENTER_SECURE_MODE) {
            attrs |= A_INVIS;
        }
        if self.has(string::ENTER_UNDERLINE_MODE) {
            attrs |= A_UNDERLINE;
        }
        if self.has(string::ENTER_ITALICS_MODE) {
            attrs |= A_ITALIC;
        }
        attrs
    }

    /// `has_colors ()`.
    #[must_use]
    pub fn has_colors(&self) -> bool {
        // `VALID_NUMERIC`: present and not cancelled.
        self.num(number::MAX_COLORS) >= 0
            && self.num(number::MAX_PAIRS) >= 0
            && ((self.has(string::SET_FOREGROUND) && self.has(string::SET_BACKGROUND))
                || (self.has(string::SET_A_FOREGROUND) && self.has(string::SET_A_BACKGROUND))
                || self.has(string::SET_COLOR_PAIR))
    }

    /// `can_change_color ()`.
    #[must_use]
    pub fn can_change_color(&self) -> bool {
        self.flag(boolean::CAN_CHANGE)
    }

    /// `ValidPair (sp, pair)`.
    #[must_use]
    pub const fn valid_pair(&self, pair: i32) -> bool {
        pair >= 0 && pair < self.pair_limit && self.coloron
    }

    /// `_nc_reserve_pairs (sp, want)`, as `ReservePairs` calls it: the
    /// table grown, doubling, to hold pair `want`.
    pub fn reserve_pairs(&mut self, want: i32) {
        let alloc = i32::try_from(self.color_pairs.len()).unwrap_or(i32::MAX);
        if !(self.color_pairs.is_empty() || want >= alloc) {
            return;
        }
        let mut have = alloc.max(1);
        while have <= want {
            have = have.saturating_mul(2);
        }
        if have > self.pair_limit {
            have = self.pair_limit;
        }
        if have > alloc {
            self.color_pairs
                .resize(usize::try_from(have).unwrap_or(0), ColorPair::default());
        }
    }

    /// The pair `pair`, as stored.
    #[must_use]
    pub fn color_pair(&self, pair: i32) -> ColorPair {
        usize::try_from(pair)
            .ok()
            .and_then(|p| self.color_pairs.get(p))
            .copied()
            .unwrap_or_default()
    }

    /// `_nc_pair_content (sp, pair, &f, &b)`: its colours, -1 for the
    /// terminal's own; `None` for no valid pair.
    pub fn pair_content(&mut self, pair: i32) -> Option<(i32, i32)> {
        if !self.valid_pair(pair) {
            return None;
        }
        self.reserve_pairs(pair);
        let p = self.color_pair(pair);
        let fg = if is_default_color(p.fg) { -1 } else { p.fg };
        let bg = if is_default_color(p.bg) { -1 } else { p.bg };
        Some((fg, bg))
    }

    /// `reset_color_pair ()`: `op`, if the terminal has it.
    fn reset_color_pair(&mut self) -> bool {
        self.putp_cap(string::ORIG_PAIR)
    }

    /// `_nc_reset_colors ()`.
    pub fn reset_colors(&mut self) -> bool {
        if self.color_defs > 0 {
            self.color_defs = self.color_defs.wrapping_neg();
        }
        let pair = self.reset_color_pair();
        let colors = self.putp_cap(string::ORIG_COLORS);
        pair || colors
    }

    /// `toggled_colors (c)`: SVr4's order of `setf`'s colours.
    const fn toggled_colors(c: i32) -> i32 {
        const TABLE: [i32; 16] = [0, 4, 2, 6, 1, 5, 3, 7, 8, 12, 10, 14, 9, 13, 11, 15];
        if c >= 0 && c < 16 {
            #[allow(clippy::indexing_slicing, reason = "0..16, checked above")]
            TABLE[c.cast_unsigned() as usize]
        } else {
            c
        }
    }

    /// `set_background_color (bg)`.
    fn set_background_color(&mut self, bg: i32) {
        if self.has(string::SET_A_BACKGROUND) {
            self.put_tiparm(string::SET_A_BACKGROUND, &[i64::from(bg)], 1);
        } else {
            self.put_tiparm(
                string::SET_BACKGROUND,
                &[i64::from(Self::toggled_colors(bg))],
                1,
            );
        }
    }

    /// `set_foreground_color (fg)`.
    fn set_foreground_color(&mut self, fg: i32) {
        if self.has(string::SET_A_FOREGROUND) {
            self.put_tiparm(string::SET_A_FOREGROUND, &[i64::from(fg)], 1);
        } else {
            self.put_tiparm(
                string::SET_FOREGROUND,
                &[i64::from(Self::toggled_colors(fg))],
                1,
            );
        }
    }

    /// `default_fg ()`, `default_bg ()`.
    const fn default_fg(&self) -> i32 {
        self.default_fg
    }

    const fn default_bg(&self) -> i32 {
        self.default_bg
    }

    /// `_nc_do_color (old_pair, pair, reverse, _nc_outch)`: the colours of
    /// `pair` on, coming from `old_pair`'s.
    pub fn do_color(&mut self, old_pair: i32, pair: i32, reverse: bool) {
        let mut fg = COLOR_DEFAULT;
        let mut bg = COLOR_DEFAULT;
        if !self.valid_pair(pair) {
            return;
        } else if pair != 0 {
            if self.has(string::SET_COLOR_PAIR) {
                self.put_tiparm(string::SET_COLOR_PAIR, &[i64::from(pair)], 1);
                return;
            }
            match self.pair_content(pair) {
                Some((f, b)) => (fg, bg) = (f, b),
                None => return,
            }
        }
        let old = if old_pair >= 0 {
            self.pair_content(old_pair)
        } else {
            None
        };
        if let Some((old_fg, old_bg)) = old {
            if (is_default_color(fg) && !is_default_color(old_fg))
                || (is_default_color(bg) && !is_default_color(old_bg))
            {
                // "If "AX" is specified in the terminal description, treat it
                // as screen's indicator of ECMA SGR 39 and SGR 49, and assume
                // the two sequences are independent."
                if self.has_sgr_39_49 && is_default_color(old_bg) && !is_default_color(old_fg) {
                    self.tputs(b"\x1b[39m", 1);
                } else if self.has_sgr_39_49
                    && is_default_color(old_fg)
                    && !is_default_color(old_bg)
                {
                    self.tputs(b"\x1b[49m", 1);
                } else {
                    self.reset_color_pair();
                }
            }
        } else {
            self.reset_color_pair();
            if old_pair < 0 && pair <= 0 {
                return;
            }
        }
        if is_default_color(fg) {
            fg = self.default_fg();
        }
        if is_default_color(bg) {
            bg = self.default_bg();
        }
        if reverse {
            std::mem::swap(&mut fg, &mut bg);
        }
        if !is_default_color(fg) {
            self.set_foreground_color(fg);
        }
        if !is_default_color(bg) {
            self.set_background_color(bg);
        }
    }

    /// `vid_puts (newmode, pair, NULL, _nc_outch)`: the attributes and pair
    /// really on made these, by the cheapest strings the terminal has.
    #[allow(clippy::too_many_lines)]
    pub fn vid_puts(&mut self, newmode: Attr, pair: i32) {
        let color_pair = pair;
        let can_color = self.coloron;
        let fix_pair0 = self.coloron && !self.default_color;
        let mut newmode = newmode & A_ATTRIBUTES;
        let mut previous_attr = self.current_attr.attr;
        let mut previous_pair = self.current_attr.pair();
        let mut reverse = false;

        // `!USE_XMC_SUPPORT`.
        if self.num(number::MAGIC_COOKIE_GLITCH) > 0 {
            newmode &= !self.xmc_suppress;
        }
        // "If we have a terminal that cannot combine color with video
        // attributes, use the colors in preference."
        let ncv = self.num(number::NO_COLOR_VIDEO);
        if (color_pair != 0 || fix_pair0) && ncv > 0 {
            let value = ncv.cast_unsigned();
            let mask: Attr = ((value & 63) | ((value & 192) << 1) | ((value & 256) >> 2)) << 16;
            let mut mask = mask;
            if mask & A_REVERSE != 0 && newmode & A_REVERSE != 0 {
                reverse = true;
                mask &= !A_REVERSE;
            }
            newmode &= !mask;
        }
        if newmode == previous_attr && color_pair == previous_pair {
            return;
        }
        if reverse {
            newmode &= !A_REVERSE;
        }
        let mut turn_off = (!newmode & previous_attr) & ALL_BUT_COLOR;
        let mut turn_on = (newmode & !(previous_attr & TPARM_ATTR)) & ALL_BUT_COLOR;

        // `SetColorsIf (why, old_attr, old_pair)`.
        macro_rules! set_colors_if {
            ($why:expr) => {
                if can_color && $why {
                    if color_pair != previous_pair
                        || (fix_pair0 && color_pair == 0)
                        || (reverse ^ (previous_attr & A_REVERSE != 0))
                    {
                        self.do_color(previous_pair, color_pair, reverse);
                    }
                }
            };
        }

        set_colors_if!(color_pair == 0 && !fix_pair0);

        if newmode == A_NORMAL {
            if previous_attr & A_ALTCHARSET != 0 && self.has(string::EXIT_ALT_CHARSET_MODE) {
                self.putp_cap(string::EXIT_ALT_CHARSET_MODE);
                previous_attr &= !A_ALTCHARSET;
            }
            if previous_attr != 0 {
                if self.has(string::EXIT_ATTRIBUTE_MODE) {
                    self.putp_cap(string::EXIT_ATTRIBUTE_MODE);
                } else {
                    if self.use_rmul
                        && turn_off & A_UNDERLINE != 0
                        && self.has(string::EXIT_UNDERLINE_MODE)
                    {
                        self.putp_cap(string::EXIT_UNDERLINE_MODE);
                    }
                    if self.use_rmso
                        && turn_off & A_STANDOUT != 0
                        && self.has(string::EXIT_STANDOUT_MODE)
                    {
                        self.putp_cap(string::EXIT_STANDOUT_MODE);
                    }
                    if self.use_ritm
                        && turn_off & A_ITALIC != 0
                        && self.has(string::EXIT_ITALICS_MODE)
                    {
                        self.putp_cap(string::EXIT_ITALICS_MODE);
                    }
                }
                previous_attr &= ALL_BUT_COLOR;
                previous_pair = 0;
            }
            set_colors_if!(color_pair != 0 || fix_pair0);
        } else if self.has(string::SET_ATTRIBUTES) {
            if turn_on != 0 || turn_off != 0 {
                let flag = |a: Attr| i64::from(newmode & a != 0);
                let params = [
                    flag(A_STANDOUT),
                    flag(A_UNDERLINE),
                    flag(A_REVERSE),
                    flag(A_BLINK),
                    flag(A_DIM),
                    flag(A_BOLD),
                    flag(A_INVIS),
                    flag(A_PROTECT),
                    flag(A_ALTCHARSET),
                ];
                self.put_tiparm(string::SET_ATTRIBUTES, &params, 1);
                previous_attr &= ALL_BUT_COLOR;
                previous_pair = 0;
            }
            if self.use_ritm {
                if turn_on & A_ITALIC != 0 {
                    if self.has(string::ENTER_ITALICS_MODE) {
                        self.putp_cap(string::ENTER_ITALICS_MODE);
                    }
                } else if turn_off & A_ITALIC != 0 && self.has(string::EXIT_ITALICS_MODE) {
                    self.putp_cap(string::EXIT_ITALICS_MODE);
                }
            }
            set_colors_if!(color_pair != 0 || fix_pair0);
        } else {
            if turn_off & A_ALTCHARSET != 0 && self.has(string::EXIT_ALT_CHARSET_MODE) {
                self.putp_cap(string::EXIT_ALT_CHARSET_MODE);
                turn_off &= !A_ALTCHARSET;
            }
            if self.use_rmul && turn_off & A_UNDERLINE != 0 && self.has(string::EXIT_UNDERLINE_MODE)
            {
                self.putp_cap(string::EXIT_UNDERLINE_MODE);
                turn_off &= !A_UNDERLINE;
            }
            if self.use_rmso && turn_off & A_STANDOUT != 0 && self.has(string::EXIT_STANDOUT_MODE) {
                self.putp_cap(string::EXIT_STANDOUT_MODE);
                turn_off &= !A_STANDOUT;
            }
            if self.use_ritm && turn_off & A_ITALIC != 0 && self.has(string::EXIT_ITALICS_MODE) {
                self.putp_cap(string::EXIT_ITALICS_MODE);
                turn_off &= !A_ITALIC;
            }
            if turn_off != 0 && self.has(string::EXIT_ATTRIBUTE_MODE) {
                self.putp_cap(string::EXIT_ATTRIBUTE_MODE);
                turn_on |= newmode & ALL_BUT_COLOR;
                previous_attr &= ALL_BUT_COLOR;
                previous_pair = 0;
            }
            set_colors_if!(color_pair != 0 || fix_pair0);
            for (mask, cap) in [
                (A_ALTCHARSET, string::ENTER_ALT_CHARSET_MODE),
                (A_BLINK, string::ENTER_BLINK_MODE),
                (A_BOLD, string::ENTER_BOLD_MODE),
                (A_DIM, string::ENTER_DIM_MODE),
                (A_REVERSE, string::ENTER_REVERSE_MODE),
                (A_STANDOUT, string::ENTER_STANDOUT_MODE),
                (A_PROTECT, string::ENTER_PROTECTED_MODE),
                (A_INVIS, string::ENTER_SECURE_MODE),
                (A_UNDERLINE, string::ENTER_UNDERLINE_MODE),
                (A_ITALIC, string::ENTER_ITALICS_MODE),
                (A_HORIZONTAL, string::ENTER_HORIZONTAL_HL_MODE),
                (A_LEFT, string::ENTER_LEFT_HL_MODE),
                (A_LOW, string::ENTER_LOW_HL_MODE),
                (A_RIGHT, string::ENTER_RIGHT_HL_MODE),
                (A_TOP, string::ENTER_TOP_HL_MODE),
                (A_VERTICAL, string::ENTER_VERTICAL_HL_MODE),
            ] {
                if turn_on & mask != 0 && self.has(cap) {
                    self.putp_cap(cap);
                }
            }
        }
        if reverse {
            newmode |= A_REVERSE;
        }
        self.current_attr.set_attr(newmode);
        self.current_attr.set_pair(color_pair);
    }

    /// `UpdateAttrs (sp, c)`: [`Term::vid_puts`] when the cell's attributes
    /// or pair are not those on.
    pub fn update_attrs(&mut self, c: &Cell) {
        if !self.current_attr.same_attr(c) {
            self.vid_puts(c.attr, c.pair());
        }
    }

    /// `start_color ()`: the colour tables made, pair 0 the default
    /// colours, `COLORS` and `COLOR_PAIRS` known.
    pub fn start_color(&mut self) -> bool {
        if self.coloron {
            return true;
        }
        let maxpairs = self.num(number::MAX_PAIRS);
        let maxcolors = self.num(number::MAX_COLORS);
        if !self.reset_color_pair() {
            let (fg, bg) = (self.default_fg(), self.default_bg());
            self.set_foreground_color(fg);
            self.set_background_color(bg);
        }
        if maxpairs > 0 && maxcolors > 0 {
            // "If using default colors, allocate extra space in table to
            // allow for default-color as a component of a color-pair."
            self.pair_limit = maxpairs
                .saturating_add(1)
                .saturating_add(maxcolors.saturating_mul(2));
            self.pair_count = maxpairs;
            self.color_count = maxcolors;
            self.color_pairs.clear();
            // `ReservePairs (sp, 16)`, before colours are on: the table is
            // made whatever `ValidPair` would say.
            self.coloron = true;
            self.reserve_pairs(16);
            self.coloron = false;
            if self.init_direct_colors() {
                self.coloron = true;
            } else {
                self.color_table = vec![Color::default(); usize::try_from(maxcolors).unwrap_or(0)];
                if let Some(p0) = self.color_pairs.first_mut() {
                    p0.fg = self.default_fg;
                    p0.bg = self.default_bg;
                }
                self.init_color_table();
                self.coloron = true;
            }
        }
        true
    }

    /// `init_color_table ()`: the palette the terminal starts with.
    fn init_color_table(&mut self) {
        let hls = self.flag(boolean::HUE_LIGHTNESS_SATURATION);
        let palette = if hls { &HLS_PALETTE } else { &CGA_PALETTE };
        for (n, slot) in self.color_table.iter_mut().enumerate() {
            #[allow(clippy::indexing_slicing, reason = "n % 8 is within the palette")]
            let base = palette[n % 8];
            *slot = base;
            if n >= 8 {
                if hls {
                    slot.green = 100;
                } else {
                    for c in [&mut slot.red, &mut slot.green, &mut slot.blue] {
                        if *c != 0 {
                            *c = 1000;
                        }
                    }
                }
            }
        }
    }

    /// `init_direct_colors ()`: whether the terminal takes colours as RGB
    /// values (`RGB`), and how many bits of each.
    fn init_direct_colors(&mut self) -> bool {
        self.direct_color = (0, 0, 0);
        let colors = self.color_count;
        if colors >= 8 {
            let mut width: i32 = 0;
            while 1i64.wrapping_shl(width.cast_unsigned()).wrapping_sub(1)
                < i64::from(colors).wrapping_sub(1)
            {
                width = width.wrapping_add(1);
            }
            let narrow = |v: i32| u8::try_from(v & 0xff).unwrap_or(0);
            if self.entry.tigetflag(b"RGB") > 0 {
                let n = width.wrapping_add(2) / 3;
                self.direct_color = (
                    narrow(n),
                    narrow(n),
                    narrow(width.wrapping_sub(n.wrapping_mul(2))),
                );
            } else {
                let n = self.entry.tigetnum(b"RGB");
                if n > 0 {
                    self.direct_color = (narrow(n), narrow(n), narrow(n));
                } else if let Some(s) = self.entry.ext_string(b"RGB") {
                    let mut vals = [n, n, width.wrapping_sub(n.wrapping_mul(2))];
                    let got = scan_slash_ints(s, &mut vals);
                    // The switch falls through from what was not read.
                    if got < 1 {
                        vals[2] = width.wrapping_sub(n.wrapping_mul(2));
                    }
                    if got < 2 {
                        vals[1] = n;
                    }
                    if got < 3 {
                        vals[0] = n;
                    }
                    self.direct_color = (narrow(vals[0]), narrow(vals[1]), narrow(vals[2]));
                }
            }
        }
        self.direct_color != (0, 0, 0)
    }

    /// `_nc_init_color (sp, color, r, g, b)`.
    pub fn init_color(&mut self, color: i32, r: i32, g: i32, b: i32) -> bool {
        if self.direct_color != (0, 0, 0) {
            return false;
        }
        let ok_rgb = |n: i32| (0..=1000).contains(&n);
        let maxcolors = self.num(number::MAX_COLORS);
        if !(self.has(string::INITIALIZE_COLOR)
            && self.coloron
            && color >= 0
            && color < self.color_count
            && color < maxcolors
            && ok_rgb(r)
            && ok_rgb(g)
            && ok_rgb(b))
        {
            return false;
        }
        let hls = self.flag(boolean::HUE_LIGHTNESS_SATURATION);
        if let Some(entry) = usize::try_from(color)
            .ok()
            .and_then(|c| self.color_table.get_mut(c))
        {
            entry.init = true;
            entry.r = r;
            entry.g = g;
            entry.b = b;
            if hls {
                let (h, l, s) = rgb2hls(r, g, b);
                entry.red = h;
                entry.green = l;
                entry.blue = s;
            } else {
                entry.red = r;
                entry.green = g;
                entry.blue = b;
            }
        }
        let params = [i64::from(color), i64::from(r), i64::from(g), i64::from(b)];
        self.put_tiparm(string::INITIALIZE_COLOR, &params, 1);
        self.color_defs = self.color_defs.max(color.saturating_add(1));
        true
    }

    /// `_nc_init_pair`'s checks and its table update, given whether the
    /// pair's old colours were set (`previous`); the caller repaints the
    /// cells of a pair that changed (`_nc_change_pair`), which needs the
    /// screen. `None` (`ERR`) for a pair or colour out of range; else
    /// whether the cells must be repainted.
    pub fn init_pair(&mut self, pair: i32, f: i32, b: i32) -> Option<bool> {
        if !self.valid_pair(pair) {
            return None;
        }
        let maxcolors = self.num(number::MAX_COLORS);
        let ok_color_hi = |n: i32, count: i32| n < count && n < maxcolors;
        self.reserve_pairs(pair);
        let previous = self.color_pair(pair);
        let (mut f, mut b) = (f, b);
        if self.default_color || self.assumed_color {
            let mut is_default = false;
            let mut default_pairs = self.default_pairs;
            if is_default_color(f) {
                f = COLOR_DEFAULT;
                is_default = true;
            } else if !ok_color_hi(f, self.color_count) {
                return None;
            }
            if is_default_color(b) {
                b = COLOR_DEFAULT;
                is_default = true;
            } else if !ok_color_hi(b, self.color_count) {
                return None;
            }
            let was_default = is_default_color(previous.fg) || is_default_color(previous.bg);
            if is_default && !was_default {
                default_pairs = default_pairs.wrapping_add(1);
            } else if was_default && !is_default {
                default_pairs = default_pairs.wrapping_sub(1);
            }
            if pair > self.pair_count.saturating_add(default_pairs) {
                return None;
            }
            self.default_pairs = default_pairs;
        } else if f < 0
            || !ok_color_hi(f, self.color_count)
            || b < 0
            || !ok_color_hi(b, self.color_count)
            || pair < 1
        {
            return None;
        }
        let result = ColorPair {
            fg: f,
            bg: b,
            mode: CP_FREE,
        };
        let repaint = (previous.fg != 0 || previous.bg != 0) && !is_same_pair(previous, result);
        // `_nc_set_color_pair (sp, pair, cpINIT)`.
        if let Some(p0) = self.color_pairs.first_mut() {
            p0.mode = CP_KEEP;
        }
        let was_free = previous.mode <= CP_FREE;
        if let Some(slot) = usize::try_from(pair)
            .ok()
            .and_then(|p| self.color_pairs.get_mut(p))
        {
            *slot = ColorPair {
                fg: f,
                bg: b,
                mode: CP_INIT,
            };
        }
        if pair == 0 {
            if let Some(p0) = self.color_pairs.first_mut() {
                p0.mode = CP_INIT;
            }
        }
        if was_free {
            self.pairs_used = self.pairs_used.wrapping_add(1);
        }
        if self.current_attr.pair() == pair {
            // "force attribute update"
            self.current_attr.set_pair(-1);
        }
        if self.has(string::INITIALIZE_PAIR) && (0..8).contains(&f) && (0..8).contains(&b) {
            let hls = self.flag(boolean::HUE_LIGHTNESS_SATURATION);
            let tp = if hls { &HLS_PALETTE } else { &CGA_PALETTE };
            #[allow(clippy::indexing_slicing, reason = "f and b are 0..8, checked above")]
            let (fc, bc) = (
                tp[f.cast_unsigned() as usize],
                tp[b.cast_unsigned() as usize],
            );
            let params =
                [pair, fc.red, fc.green, fc.blue, bc.red, bc.green, bc.blue].map(i64::from);
            self.put_tiparm(string::INITIALIZE_PAIR, &params, 1);
        }
        Some(repaint)
    }

    /// `assume_default_colors`'s settings, before its `init_pair (0, …)`:
    /// `false` (`ERR`) for a terminal that cannot reset its colours.
    pub fn assume_default_colors(&mut self, fg: i32, bg: i32) -> bool {
        if !((self.has(string::ORIG_PAIR) || self.has(string::ORIG_COLORS))
            && !self.has(string::INITIALIZE_PAIR))
        {
            return false;
        }
        self.default_color = is_default_color(fg) || is_default_color(bg);
        self.has_sgr_39_49 = self.entry.tigetflag(b"AX") == 1;
        self.default_fg = if is_default_color(fg) {
            COLOR_DEFAULT
        } else {
            fg
        };
        self.default_bg = if is_default_color(bg) {
            COLOR_DEFAULT
        } else {
            bg
        };
        true
    }
}

/// `write (fd, ...)` until all of `buf` is out: an interrupted or
/// would-block write is tried again, a write of nothing or any other error
/// abandons the rest, as `_nc_flush` does.
fn write_all(fd: i32, mut buf: &[u8]) {
    while !buf.is_empty() {
        match libcall::fd::write(fd, buf) {
            Ok(0) => break,
            Ok(n) => buf = buf.get(n..).unwrap_or_default(),
            Err(e) if e == libcall::EINTR || e == libcall::EAGAIN => {}
            Err(_) => break,
        }
    }
}

/// `sscanf (s, "%d/%d/%d", &r, &g, &b)`: how many it read, into `vals`.
fn scan_slash_ints(s: &[u8], vals: &mut [i32; 3]) -> usize {
    let mut at = 0usize;
    let mut got = 0usize;
    for (i, slot) in vals.iter_mut().enumerate() {
        if i > 0 {
            if s.get(at) != Some(&b'/') {
                return got;
            }
            at = at.saturating_add(1);
        }
        while s.get(at).is_some_and(u8::is_ascii_whitespace) {
            at = at.saturating_add(1);
        }
        let start = at;
        if matches!(s.get(at), Some(b'-' | b'+')) {
            at = at.saturating_add(1);
        }
        let digits = s
            .get(at..)
            .unwrap_or_default()
            .iter()
            .take_while(|b| b.is_ascii_digit())
            .count();
        if digits == 0 {
            return got;
        }
        at = at.saturating_add(digits);
        let text = std::str::from_utf8(s.get(start..at).unwrap_or_default()).unwrap_or("0");
        *slot = text
            .parse::<i64>()
            .map_or(0, |v| i32::try_from(v).unwrap_or(-1));
        got = got.saturating_add(1);
    }
    got
}

/// `rgb2hls (r, g, b, &h, &l, &s)`.
fn rgb2hls(r: i32, g: i32, b: i32) -> (i32, i32, i32) {
    let mut min = if g < r { g } else { r };
    if min > b {
        min = b;
    }
    let mut max = if g > r { g } else { r };
    if max < b {
        max = b;
    }
    let l = min.wrapping_add(max) / 20;
    if min == max {
        return (0, l, 0);
    }
    let s = if l < 50 {
        max.wrapping_sub(min)
            .wrapping_mul(100)
            .checked_div(max.wrapping_add(min))
            .unwrap_or(0)
    } else {
        max.wrapping_sub(min)
            .wrapping_mul(100)
            .checked_div(2000i32.wrapping_sub(max).wrapping_sub(min))
            .unwrap_or(0)
    };
    let t = if r == max {
        120i32.wrapping_add(
            g.wrapping_sub(b)
                .wrapping_mul(60)
                .checked_div(max.wrapping_sub(min))
                .unwrap_or(0),
        )
    } else if g == max {
        240i32.wrapping_add(
            b.wrapping_sub(r)
                .wrapping_mul(60)
                .checked_div(max.wrapping_sub(min))
                .unwrap_or(0),
        )
    } else {
        360i32.wrapping_add(
            r.wrapping_sub(g)
                .wrapping_mul(60)
                .checked_div(max.wrapping_sub(min))
                .unwrap_or(0),
        )
    };
    (t % 360, l, s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hls_is_upstreams_arithmetic() {
        assert_eq!(rgb2hls(0, 0, 0), (0, 0, 0));
        assert_eq!(rgb2hls(1000, 1000, 1000), (0, 100, 0));
        assert_eq!(rgb2hls(1000, 0, 0), (120, 50, 100));
        assert_eq!(rgb2hls(0, 1000, 0), (240, 50, 100));
    }

    #[test]
    fn svr4_swaps_red_and_blue() {
        assert_eq!(Term::toggled_colors(1), 4);
        assert_eq!(Term::toggled_colors(4), 1);
        assert_eq!(Term::toggled_colors(3), 6);
        assert_eq!(Term::toggled_colors(16), 16);
    }

    #[test]
    fn rgb_strings_are_scanned_as_sscanf_does() {
        let mut v = [9, 9, 9];
        assert_eq!(scan_slash_ints(b"8/8/8", &mut v), 3);
        assert_eq!(v, [8, 8, 8]);
        let mut v = [9, 9, 9];
        assert_eq!(scan_slash_ints(b"5/x", &mut v), 1);
        assert_eq!(v, [5, 9, 9]);
    }
}
