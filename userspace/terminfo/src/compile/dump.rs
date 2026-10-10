//! `dump_entry.c`: a description written out as source -- what `infocmp`
//! prints, and `tic -I` and `-C` -- and compared, capability by
//! capability, as `infocmp -d`, `-c` and `-n` compare.
//!
//! The capabilities go out in the order of their names (terminfo's,
//! termcap's or the C variables', `-s`), booleans, then numbers, then
//! strings, `separator` between them, wrapped at `width`; in termcap form
//! a string that termcap cannot say goes out commented, as `..name=...`.
//! An entry too long for termcap's 1023 bytes sheds capabilities until it
//! fits: its untranslatable strings, its extended ones, `sgr`, `acsc`,
//! the terminfo-only ones, its labels and its function keys, saying what
//! it removed in `#` lines.
//!
//! Upstream keeps all of this in file-scope statics; here it is the state of
//! a [`Dumper`], whose `stdout` collects what upstream prints to standard
//! output, for the program to write. Its standard-error messages go
//! through the [`Scanner`] each method is given, as do its warnings.

use super::caps::{b, n, s};
use super::captoinfo::infotocap;
use super::expand::tic_expand;
use super::scan::{Scanner, cstr, isspace};
use super::tables::{self, first_name};
use super::write::write_object;
use crate::Kind;
use crate::captab;
use crate::entry::{ABSENT_NUMERIC, BOOLCOUNT, NUMCOUNT, STRCOUNT};
use crate::names;
use crate::termtype::{Str, TermType};

/// `F_TERMINFO`: terminfo names.
pub const F_TERMINFO: i32 = 0;
/// `F_VARIABLE`: C variable names.
pub const F_VARIABLE: i32 = 1;
/// `F_TERMCAP`: termcap names, capabilities converted.
pub const F_TERMCAP: i32 = 2;
/// `F_TCONVERR`: as `F_TERMCAP`, untranslatables kept.
pub const F_TCONVERR: i32 = 3;
/// `F_LITERAL`: as `F_TERMINFO`, without smart defaults.
pub const F_LITERAL: i32 = 4;

/// `S_DEFAULT`.
pub const S_DEFAULT: i32 = 0;
/// `S_NOSORT`: `term.h`'s order.
pub const S_NOSORT: i32 = 1;
/// `S_TERMINFO`: by terminfo name.
pub const S_TERMINFO: i32 = 2;
/// `S_VARIABLE`: by C variable name.
pub const S_VARIABLE: i32 = 3;
/// `S_TERMCAP`: by termcap name.
pub const S_TERMCAP: i32 = 4;

/// `CMP_USE`: the comparison of `use=` clauses, after the capabilities'.
pub const CMP_USE: u32 = 3;

/// `FAIL`: a predicate's "show nothing".
pub const FAIL: i32 = -1;

/// `V_ALLCAPS`: every capability.
pub const V_ALLCAPS: i32 = 0;
/// `V_SVR1`: System V Release 1, Ultrix.
pub const V_SVR1: i32 = 1;
/// `V_HPUX`.
pub const V_HPUX: i32 = 2;
/// `V_AIX`.
pub const V_AIX: i32 = 3;
/// `V_BSD`: termcap's own.
pub const V_BSD: i32 = 4;

/// `WRAPPED`: the narrowest a wrapped string is cut to.
const WRAPPED: i32 = 32;
/// `MAX_TERMINFO_LENGTH`.
const MAX_TERMINFO_LENGTH: usize = 4096;
/// `MAX_TERMCAP_LENGTH`.
const MAX_TERMCAP_LENGTH: i32 = 1023;
/// `EXTRA_CAP`.
const EXTRA_CAP: usize = 20;
/// `MAX_ALIAS`.
const MAX_ALIAS: usize = 32;

/// `wrap_concat`'s modes.
const W_OFF: u32 = 0;
const W_1ST: u32 = 1;
const W_2ND: u32 = 2;
const W_END: u32 = 4;
const W_ERR: u32 = 8;

/// A predicate: given the description being dumped, a type and an index,
/// [`FAIL`] to show nothing, else what to show -- for a boolean, a value
/// of 0 or less shows it cancelled.
pub type Pred<'p> = &'p dyn Fn(&TermType, Kind, usize) -> i32;

/// `compare_entry`'s hook: called with the dumper, a capability's type (or
/// `None` for the `use=` clauses), its index, and its name.
pub type CompareHook<'h> = dyn FnMut(&mut Dumper, Option<Kind>, usize, &[u8]) + 'h;

/// `dump_predicate`: ordinary decompilation.
fn dump_predicate(t: &TermType, kind: Kind, idx: usize) -> i32 {
    match kind {
        Kind::Boolean => match t.booleans.get(idx).copied().unwrap_or(0) {
            0 => FAIL,
            v => i32::from(v),
        },
        Kind::Number => match t.numbers.get(idx).copied().unwrap_or(ABSENT_NUMERIC) {
            ABSENT_NUMERIC => FAIL,
            v => v,
        },
        Kind::String => {
            if t.strings.get(idx).is_some_and(|v| *v != Str::Absent) {
                1
            } else {
                FAIL
            }
        }
    }
}

/// The byte at `i`, NUL past the end.
fn at(s: &[u8], i: usize) -> u8 {
    s.get(i).copied().unwrap_or(0)
}

/// `nametrans (name)`: a terminfo name's termcap name, where termcap has
/// the capability.
#[must_use]
pub fn nametrans(name: &[u8]) -> Option<&'static str> {
    let np = tables::find_entry(name, false)?;
    let i = np.index;
    match np.kind {
        Kind::Boolean => (i <= captab::OK_BOOL_FROM_TERMCAP
            && captab::BOOL_FROM_TERMCAP.get(i).copied().unwrap_or(false))
        .then(|| names::BOOLCODES.get(i).copied())
        .flatten(),
        Kind::Number => (i <= captab::OK_NUM_FROM_TERMCAP
            && captab::NUM_FROM_TERMCAP.get(i).copied().unwrap_or(false))
        .then(|| names::NUMCODES.get(i).copied())
        .flatten(),
        Kind::String => (i <= captab::OK_STR_FROM_TERMCAP
            && captab::STR_FROM_TERMCAP.get(i).copied().unwrap_or(false))
        .then(|| names::STRCODES.get(i).copied())
        .flatten(),
    }
}

/// `has_params (src, formatting)`: whether a string takes parameters --
/// for formatting, only a long one or one with an if-then.
#[must_use]
pub fn has_params(src: &[u8], formatting: bool) -> bool {
    let src = cstr(src);
    let len = src.len();
    let mut ifthen = false;
    let mut params = false;
    let mut result = false;
    for n in 0..len.saturating_sub(1) {
        let pair = src.get(n..n.saturating_add(2)).unwrap_or_default();
        if pair == b"%p" {
            params = true;
        } else if pair == b"%;" {
            ifthen = true;
            result = params;
            break;
        }
    }
    if !ifthen {
        result = if formatting {
            len > 50 && params
        } else {
            params
        };
    }
    result
}

/// `fill_spaces (src)`: every space `\s`.
fn fill_spaces(src: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(src.len());
    for &c in cstr(src) {
        if c == b' ' {
            out.extend_from_slice(b"\\s");
        } else {
            out.push(c);
        }
    }
    out
}

/// `%*s`: `width` spaces' worth of a one-space string.
fn pad(width: i32) -> Vec<u8> {
    vec![b' '; usize::try_from(width.max(1)).unwrap_or(1)]
}

/// The state of `dump_entry.c`.
pub struct Dumper {
    /// `tversion`.
    pub tversion: i32,
    /// `outform`.
    pub outform: i32,
    /// `sortmode`.
    pub sortmode: i32,
    /// `width`.
    pub width: i32,
    /// `height`.
    pub height: i32,
    column: i32,
    oldcol: i32,
    pretty: bool,
    wrapped: bool,
    did_wrap: bool,
    /// `checking`: `tic -c`'s warnings about if-then-else structure.
    pub checking: bool,
    /// `quickdump`: `-Q`'s hex (1) and base-64 (2).
    pub quickdump: i32,
    save_sgr: Str,
    /// `outbuf`: the entry formatted.
    pub outbuf: Vec<u8>,
    tmpbuf: Vec<u8>,
    separator: &'static [u8],
    trailer: &'static [u8],
    indent: i32,
    /// `_nc_user_definable`.
    pub user_definable: bool,
    /// `_nc_progname`.
    pub progname: Vec<u8>,
    /// What upstream prints to standard output, in order.
    pub stdout: Vec<u8>,
    /// Whether `outbuf` has been written to: until it has, upstream's has
    /// no text, and `show_entry` prints nothing.
    touched: bool,
    tparm: crate::Tparm,
}

impl Dumper {
    /// A dumper, its state as upstream's statics start.
    #[must_use]
    pub fn new(progname: &[u8], user_definable: bool) -> Self {
        Self {
            tversion: V_ALLCAPS,
            outform: F_TERMINFO,
            sortmode: S_DEFAULT,
            width: 60,
            height: 65535,
            column: 0,
            oldcol: 0,
            pretty: false,
            wrapped: false,
            did_wrap: false,
            checking: false,
            quickdump: 0,
            save_sgr: Str::Absent,
            outbuf: Vec::new(),
            tmpbuf: Vec::new(),
            separator: b"",
            trailer: b"",
            indent: 8,
            user_definable,
            progname: progname.to_vec(),
            stdout: Vec::new(),
            touched: false,
            tparm: crate::Tparm::new(),
        }
    }

    /// `TcOutput ()`.
    fn tc_output(&self) -> bool {
        self.outform == F_TERMCAP || self.outform == F_TCONVERR
    }

    /// `dump_init (version, mode, sort, wrap_strings, width, height,
    /// traceval, formatted, check, quick)`.
    #[allow(clippy::too_many_arguments, reason = "upstream's dump_init")]
    pub fn init(
        &mut self,
        scan: &mut Scanner<'_>,
        version: Option<&[u8]>,
        mode: i32,
        sort: i32,
        wrap_strings: bool,
        twidth: i32,
        theight: i32,
        traceval: u32,
        formatted: bool,
        check: bool,
        quick: i32,
    ) {
        self.width = twidth;
        self.height = theight;
        self.pretty = formatted;
        self.wrapped = wrap_strings;
        self.checking = check;
        self.quickdump = quick & 3;
        self.did_wrap = self.width <= 0;

        self.tversion = match version {
            None => V_ALLCAPS,
            Some(b"SVr1" | b"SVR1" | b"Ultrix") => V_SVR1,
            Some(b"HP") => V_HPUX,
            Some(b"AIX") => V_AIX,
            Some(b"BSD") => V_BSD,
            Some(_) => V_ALLCAPS,
        };

        self.outform = mode;
        match mode {
            F_LITERAL | F_TERMINFO | F_VARIABLE => {
                self.separator = if twidth > 0 && theight > 1 {
                    b", "
                } else {
                    b","
                };
                self.trailer = b"\n\t";
            }
            F_TERMCAP | F_TCONVERR => {
                self.separator = b":";
                self.trailer = b"\\\n\t:";
            }
            _ => {}
        }
        self.indent = 8;

        self.sortmode = sort;
        let how = match sort {
            S_NOSORT => Some("term structure order"),
            S_TERMINFO => Some("terminfo name order"),
            S_VARIABLE => Some("C variable order"),
            S_TERMCAP => Some("termcap name order"),
            _ => None,
        };
        if traceval != 0 {
            if let Some(how) = how {
                let mut m = self.progname.clone();
                m.extend_from_slice(format!(": sorting by {how}\n").as_bytes());
                scan.emit(&m);
            }
            let mut m = self.progname.clone();
            m.extend_from_slice(
                format!(
                    ": width = {}, tversion = {}, outform = {}\n",
                    self.width, self.tversion, self.outform
                )
                .as_bytes(),
            );
            scan.emit(&m);
        }
    }

    /// The names of booleans, numbers and strings in the output form.
    fn names_of(&self, kind: Kind) -> &'static [&'static str] {
        match (self.outform, kind) {
            (F_VARIABLE, Kind::Boolean) => &names::BOOLFNAMES,
            (F_VARIABLE, Kind::Number) => &names::NUMFNAMES,
            (F_VARIABLE, Kind::String) => &names::STRFNAMES,
            (F_TERMCAP | F_TCONVERR, Kind::Boolean) => &names::BOOLCODES,
            (F_TERMCAP | F_TCONVERR, Kind::Number) => &names::NUMCODES,
            (F_TERMCAP | F_TCONVERR, Kind::String) => &names::STRCODES,
            (_, Kind::Boolean) => &names::BOOLNAMES,
            (_, Kind::Number) => &names::NUMNAMES,
            (_, Kind::String) => &names::STRNAMES,
        }
    }

    /// `BoolIndirect`, `NumIndirect`, `StrIndirect`: the `j`th capability
    /// in the sort order.
    fn indirect(&self, kind: Kind, j: usize) -> usize {
        let count = match kind {
            Kind::Boolean => BOOLCOUNT,
            Kind::Number => NUMCOUNT,
            Kind::String => STRCOUNT,
        };
        if j >= count || self.sortmode == S_NOSORT {
            return j;
        }
        let table: &[usize] = match (self.sortmode, kind) {
            (S_VARIABLE, Kind::Boolean) => &captab::BOOL_VARIABLE_SORT,
            (S_VARIABLE, Kind::Number) => &captab::NUM_VARIABLE_SORT,
            (S_VARIABLE, Kind::String) => &captab::STR_VARIABLE_SORT,
            (S_TERMCAP, Kind::Boolean) => &captab::BOOL_TERMCAP_SORT,
            (S_TERMCAP, Kind::Number) => &captab::NUM_TERMCAP_SORT,
            (S_TERMCAP, Kind::String) => &captab::STR_TERMCAP_SORT,
            (_, Kind::Boolean) => &captab::BOOL_TERMINFO_SORT,
            (_, Kind::Number) => &captab::NUM_TERMINFO_SORT,
            (_, Kind::String) => &captab::STR_TERMINFO_SORT,
        };
        table.get(j).copied().unwrap_or(j)
    }

    /// `ExtBoolname`, `ExtNumname`, `ExtStrname`: the name of capability
    /// `i`, an extended one's from the entry.
    fn ext_name(&self, tp: &TermType, kind: Kind, i: usize) -> Vec<u8> {
        let (count, base) = match kind {
            Kind::Boolean => (BOOLCOUNT, 0),
            Kind::Number => (NUMCOUNT, tp.ext_booleans),
            Kind::String => (STRCOUNT, tp.ext_booleans.saturating_add(tp.ext_numbers)),
        };
        if let Some(x) = i.checked_sub(count) {
            tp.ext_name(x.saturating_add(base)).to_vec()
        } else {
            self.names_of(kind)
                .get(i)
                .map(|s| s.as_bytes().to_vec())
                .unwrap_or_default()
        }
    }

    /// `isObsolete (outform, name)`: an obsolete termcap capability, not
    /// shown in terminfo form unless sorting by variable name or keeping
    /// extended names.
    fn is_obsolete(&self, name: &[u8]) -> bool {
        (self.outform == F_TERMINFO || self.outform == F_VARIABLE)
            && self.sortmode != S_VARIABLE
            && !self.user_definable
            && name.starts_with(b"OT")
    }

    /// `version_filter (type, idx)`: whether the `-R` subset shows the
    /// capability.
    fn version_filter(&self, kind: Kind, idx: usize) -> bool {
        let fnkey = |i: usize| {
            (s::KEY_F0..=s::KEY_F9).contains(&i) || (s::KEY_F11..=s::KEY_F63).contains(&i)
        };
        match self.tversion {
            V_ALLCAPS => true,
            V_SVR1 => match kind {
                Kind::Boolean => idx <= b::XON_XOFF,
                Kind::Number => idx <= n::WIDTH_STATUS_LINE,
                Kind::String => idx <= s::PRTR_NON,
            },
            V_HPUX => match kind {
                Kind::Boolean => idx <= b::XON_XOFF,
                Kind::Number => idx <= n::LABEL_WIDTH,
                Kind::String => {
                    idx <= s::PRTR_NON
                        || fnkey(idx)
                        || idx == s::PLAB_NORM
                        || idx == s::LABEL_ON
                        || idx == s::LABEL_OFF
                }
            },
            V_AIX => match kind {
                Kind::Boolean => idx <= b::XON_XOFF,
                Kind::Number => idx <= n::WIDTH_STATUS_LINE,
                Kind::String => idx <= s::PRTR_NON || fnkey(idx),
            },
            V_BSD => match kind {
                Kind::Boolean => captab::BOOL_FROM_TERMCAP.get(idx).copied().unwrap_or(false),
                Kind::Number => captab::NUM_FROM_TERMCAP.get(idx).copied().unwrap_or(false),
                Kind::String => captab::STR_FROM_TERMCAP.get(idx).copied().unwrap_or(false),
            },
            _ => false,
        }
    }

    /// `trim_trailing`.
    fn trim_trailing(&mut self) {
        while self.outbuf.last() == Some(&b' ') {
            self.outbuf.pop();
        }
    }

    /// `force_wrap`.
    fn force_wrap(&mut self) {
        self.oldcol = self.column;
        self.trim_trailing();
        let t = self.trailer;
        self.outbuf.extend_from_slice(t);
        self.column = self.indent;
    }

    /// `op_length (src, offset)`: how long the `%` operator at `offset` is.
    fn op_length(&self, src: &[u8], offset: usize) -> usize {
        if offset > 0 && at(src, offset.saturating_sub(1)) == b'\\' {
            return 0;
        }
        let mut result = 1usize;
        let ch = at(src, offset.saturating_add(result));
        if self.tc_output() {
            result = result.wrapping_add(match ch {
                b'>' => 3,
                b'+' => 2,
                _ => 1,
            });
        } else if ch == b'\'' {
            result = result.wrapping_add(3);
        } else if ch == b'{' {
            let mut n = result;
            loop {
                let c = at(src, offset.saturating_add(n));
                if c == 0 {
                    break;
                }
                if c == b'}' {
                    n = n.wrapping_add(1);
                    result = n;
                    break;
                }
                n = n.wrapping_add(1);
            }
        } else if b"pPg\0".contains(&ch) {
            result = result.wrapping_add(2);
        } else {
            // "ordinary operator"
            result = result.wrapping_add(1);
        }
        result
    }

    /// `find_split (src, step, size)`: where to cut a wrapped string so as
    /// not to split a backslash sequence or a `%` operator.
    fn find_split(&self, src: &[u8], step: usize, size: usize) -> usize {
        let mut result = size;
        if size > 0 {
            let mut mark = size;
            let mut n = size.saturating_sub(1);
            while n > 0 {
                let ch = at(src, step.saturating_add(n));
                if ch == b'\\' {
                    if at(src, step.saturating_add(n).saturating_sub(1)) == ch {
                        n = n.wrapping_sub(1);
                    }
                    mark = n;
                    break;
                } else if !ch.is_ascii_alphanumeric() {
                    break;
                }
                n = n.wrapping_sub(1);
            }
            if mark < size {
                result = mark;
            } else {
                let mut n = size.saturating_sub(1);
                while n > 0 {
                    if at(src, step.saturating_add(n)) == b'%' {
                        let need = self.op_length(src, step.saturating_add(n));
                        if n.saturating_add(need) > size {
                            mark = n;
                        }
                        break;
                    }
                    n = n.wrapping_sub(1);
                }
                if mark < size {
                    result = mark;
                }
            }
        }
        result
    }

    /// `wrap_concat (src, need, mode)`: `src` onto the line, wrapping first
    /// if it would not fit, and cutting a long string across lines when
    /// asked to (`-W`).
    fn wrap_concat(&mut self, src: &[u8], need: i32, mode: u32) {
        self.touched = true;
        let src = cstr(src).to_vec();
        let gaps = i32::try_from(self.separator.len()).unwrap_or(0);
        let want = gaps.saturating_add(need);
        self.did_wrap = self.width <= 0;
        if mode & W_1ST != 0
            && self.column > self.indent
            && self.column.saturating_add(want) > self.width
        {
            self.force_wrap();
        }
        if mode & W_END != 0
            && mode & W_ERR == 0
            && self.wrapped
            && self.width >= 0
            && self.column.saturating_add(want) > self.width
        {
            let mut step = 0usize;
            let used = self.width.max(WRAPPED);
            let mut base = 0i32;
            let my_t = self.trailer;
            let fill = fill_spaces(&src);
            let last = fill.len();
            let mut need = i32::try_from(last).unwrap_or(i32::MAX);
            if self.tc_output() {
                self.trailer = b"\\\n\t ";
            }
            let align: Vec<u8>;
            if !self.tc_output()
                && let Some(p) = fill.iter().position(|&c| c == b'=')
            {
                base = i32::try_from(p.saturating_add(1)).unwrap_or(0).min(8);
                align = pad(base);
            } else if self.column > 8 {
                base = self.column.wrapping_sub(8).min(8);
                align = pad(base);
            } else {
                align = Vec::new();
            }
            // "pretty" overrides wrapping if it already split the line
            if !self.pretty || !fill.contains(&b'\n') {
                let mut tag = 0i32;
                if self.tc_output() && !self.outbuf.is_empty() && mode & W_1ST == 0 {
                    tag = 3;
                }
                while self.column.saturating_add(need.saturating_add(gaps)) > used {
                    let mut size = used.wrapping_sub(tag);
                    if step != 0 {
                        self.outbuf.extend_from_slice(&align);
                        size = size.wrapping_sub(base);
                    }
                    let left = i32::try_from(last.saturating_sub(step)).unwrap_or(0);
                    if size > left {
                        size = left;
                    }
                    let size = self.find_split(&fill, step, usize::try_from(size).unwrap_or(0));
                    if size == 0 {
                        // Upstream would go round for ever here.
                        break;
                    }
                    let piece = fill
                        .get(step..step.saturating_add(size))
                        .unwrap_or_default();
                    self.outbuf.extend_from_slice(piece);
                    step = step.saturating_add(size);
                    need = need.saturating_sub(i32::try_from(size).unwrap_or(0));
                    if need > 0 {
                        self.force_wrap();
                        self.did_wrap = true;
                        tag = 0;
                    }
                }
            }
            if need > 0 {
                if step != 0 {
                    self.outbuf.extend_from_slice(&align);
                }
                self.outbuf
                    .extend_from_slice(fill.get(step..).unwrap_or_default());
            }
            if mode & W_END != 0 {
                let sep = self.separator;
                self.outbuf.extend_from_slice(sep);
            }
            self.trailer = my_t;
            self.force_wrap();
        } else {
            self.outbuf.extend_from_slice(&src);
            if mode & W_END != 0 {
                let sep = self.separator;
                self.outbuf.extend_from_slice(sep);
            }
            self.column = self
                .column
                .saturating_add(i32::try_from(src.len()).unwrap_or(0));
        }
    }

    /// `wrap_concat1 (src)`.
    fn wrap_concat1(&mut self, src: &[u8]) {
        let need = i32::try_from(cstr(src).len()).unwrap_or(0);
        self.wrap_concat(src, need, W_1ST | W_END);
    }

    /// `wrap_concat3 (name, eqls, value)`.
    fn wrap_concat3(&mut self, name: &[u8], eqls: &[u8], value: &[u8]) {
        let nlen = i32::try_from(name.len()).unwrap_or(0);
        let elen = i32::try_from(eqls.len()).unwrap_or(0);
        let vlen = i32::try_from(cstr(value).len()).unwrap_or(0);
        self.wrap_concat(name, nlen.saturating_add(elen).saturating_add(vlen), W_1ST);
        self.wrap_concat(eqls, elen.saturating_add(vlen), W_2ND);
        self.wrap_concat(value, vlen, W_END);
    }

    /// `indent_DYN (tmpbuf, level)`.
    fn indent_tmp(&mut self, level: i32) {
        for _ in 0..level {
            self.tmpbuf.push(b'\t');
        }
    }

    /// `leading_DYN (tmpbuf, leading)`: whether the current line is only
    /// tabs and `leading`.
    fn leading_tmp(&self, leading: &[u8]) -> bool {
        let used = self.tmpbuf.len();
        let mut need = leading.len();
        if used <= need {
            return false;
        }
        need = used.saturating_sub(need);
        if self.tmpbuf.get(need..) != Some(leading) {
            return false;
        }
        let mut result = true;
        loop {
            need = need.wrapping_sub(1);
            if need == 0 {
                break;
            }
            let c = at(&self.tmpbuf, need);
            if c == b'\n' {
                break;
            }
            if c != b'\t' {
                result = false;
                break;
            }
        }
        result
    }

    /// `tmpbuf.text[tmpbuf.used - 1] = '\n'`.
    fn last_tmp_newline(&mut self) {
        if let Some(c) = self.tmpbuf.last_mut() {
            *c = b'\n';
        }
    }

    /// `fmt_complex (tterm, capability, src, level)`: an if-then-else
    /// string laid out across lines (`-f`); where in `src` it stopped.
    fn fmt_complex(
        &mut self,
        scan: &mut Scanner<'_>,
        names: &[u8],
        capability: &[u8],
        src: &[u8],
        start: usize,
        level: i32,
    ) -> usize {
        let mut percent = false;
        let mut i = start;
        let mut params = has_params(src.get(i..).unwrap_or_default(), true);
        while at(src, i) != 0 {
            let c = at(src, i);
            match c {
                // The character, and -- after the match, as for every other
                // -- the one after it.
                b'^' | b'\\' => {
                    percent = false;
                    self.tmpbuf.push(c);
                    i = i.wrapping_add(1);
                }
                b'%' => percent = true,
                b'?' | b't' | b'e' if percent => {
                    percent = false;
                    self.last_tmp_newline();
                    // "treat a "%e" as else-if, on the same level"
                    if c == b'e' {
                        self.indent_tmp(level);
                        self.tmpbuf.push(b'%');
                        self.tmpbuf.push(c);
                        i = i.wrapping_add(1);
                        params = has_params(src.get(i..).unwrap_or_default(), true);
                        if !params && at(src, i) != 0 && at(src, i) != b'%' {
                            self.tmpbuf.push(b'\n');
                            self.indent_tmp(level.wrapping_add(1));
                        }
                    } else {
                        self.indent_tmp(level.wrapping_add(1));
                        self.tmpbuf.push(b'%');
                        self.tmpbuf.push(c);
                        i = i.wrapping_add(1);
                        if c == b'?' {
                            i = self.fmt_complex(
                                scan,
                                names,
                                capability,
                                src,
                                i,
                                level.wrapping_add(1),
                            );
                            if at(src, i) != 0 && at(src, i) != b'%' {
                                self.tmpbuf.push(b'\n');
                                self.indent_tmp(level.wrapping_add(1));
                            }
                        } else if level == 1 && self.checking {
                            let mut m = first_name(names);
                            m.extend_from_slice(
                                format!(": %{} without %? in ", char::from(at(src, i))).as_bytes(),
                            );
                            m.extend_from_slice(capability);
                            scan.warning(&m);
                        }
                    }
                    continue;
                }
                b';' if percent => {
                    percent = false;
                    if level > 1 {
                        self.last_tmp_newline();
                        self.indent_tmp(level);
                        self.tmpbuf.push(b'%');
                        self.tmpbuf.push(c);
                        i = i.wrapping_add(1);
                        if at(src, i) == b'%'
                            && at(src, i.wrapping_add(1)) != 0
                            && !b"?e;".contains(&at(src, i.wrapping_add(1)))
                        {
                            self.tmpbuf.push(b'\n');
                            self.indent_tmp(level);
                        }
                        return i;
                    }
                    if self.checking {
                        let mut m = first_name(names);
                        m.extend_from_slice(b": %; without %? in ");
                        m.extend_from_slice(capability);
                        scan.warning(&m);
                    }
                }
                b'p' => {
                    if percent && params && !self.leading_tmp(b"%") {
                        self.last_tmp_newline();
                        self.indent_tmp(level.wrapping_add(1));
                        self.tmpbuf.push(b'%');
                    }
                    percent = false;
                }
                b' ' => {
                    self.tmpbuf.extend_from_slice(b"\\s");
                    i = i.wrapping_add(1);
                    continue;
                }
                _ => percent = false,
            }
            if at(src, i) == 0 {
                // A lone `^` or `\` at the end, whose NUL upstream copies and
                // then reads past.
                break;
            }
            self.tmpbuf.push(at(src, i));
            i = i.wrapping_add(1);
        }
        i
    }

    /// `number_format (value)`: hexadecimal for a large number near a power
    /// of two.
    fn number_format(&self, value: i32) -> String {
        if self.outform != F_TERMCAP && value > 255 {
            let lv = u64::try_from(value).unwrap_or(0);
            for nn in 8..64 {
                let mm: u64 = 1 << nn;
                if mm.saturating_sub(16) <= lv && mm.saturating_add(16) > lv {
                    return format!("{value:#x}");
                }
            }
        }
        format!("{value}")
    }

    /// The `sgr0` termcap callers would get: `_nc_trim_sgr0`.
    fn trimmed_sgr0(&mut self, tterm: &TermType) -> Option<Vec<u8>> {
        let mut t = tterm.clone();
        if let Some(slot) = t.strings.get_mut(s::SET_ATTRIBUTES) {
            *slot = self.save_sgr.clone();
        }
        let entry = crate::Entry::from_termtype(t);
        crate::trim_sgr0(&entry, &mut self.tparm)
    }

    /// `fmt_entry (tterm, pred, content_only, suppress_untranslatable,
    /// infodump, numbers)`: the entry formatted into `outbuf`; its length
    /// as compiled (`infodump`) or as text.
    #[allow(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        reason = "upstream's fmt_entry, in one piece so it reads against it"
    )]
    #[allow(clippy::too_many_arguments, reason = "upstream's fmt_entry")]
    pub fn fmt_entry(
        &mut self,
        scan: &mut Scanner<'_>,
        tterm: &mut TermType,
        pred: Option<Pred<'_>>,
        content_only: bool,
        suppress_untranslatable: bool,
        infodump: bool,
        numbers: i32,
    ) -> i32 {
        let pred: Pred<'_> = pred.unwrap_or(&dump_predicate);
        let mut len: i32 = 12;
        let mut num_bools = 0usize;
        let mut num_values = 0usize;
        let mut num_strings = 0usize;
        let mut outcount = false;

        self.outbuf.clear();
        self.touched = true;
        if content_only {
            // "workaround to prevent empty lines"
            self.column = self.indent;
        } else {
            let mut names = cstr(&tterm.term_names).to_vec();
            // "Colon is legal in terminfo descriptions, but not in termcap."
            if !infodump {
                for c in &mut names {
                    if *c == b':' {
                        *c = b'=';
                    }
                }
            }
            self.outbuf.extend_from_slice(&names);
            let sep = self.separator;
            self.outbuf.extend_from_slice(sep);
            self.column = i32::try_from(self.outbuf.len()).unwrap_or(i32::MAX);
            if self.height > 1 {
                self.force_wrap();
            }
        }

        for j in 0..tterm.booleans.len() {
            let i = self.indirect(Kind::Boolean, j);
            let name = self.ext_name(tterm, Kind::Boolean, i);
            if !self.version_filter(Kind::Boolean, i) || self.is_obsolete(&name) {
                continue;
            }
            let predval = pred(tterm, Kind::Boolean, i);
            if predval != FAIL {
                let mut buffer = name;
                if predval <= 0 {
                    buffer.push(b'@');
                } else if i.wrapping_add(1) > num_bools {
                    num_bools = i.wrapping_add(1);
                }
                self.wrap_concat1(&buffer);
                outcount = true;
            }
        }

        if self.column != self.indent && self.height > 1 {
            self.force_wrap();
        }

        for j in 0..tterm.numbers.len() {
            let i = self.indirect(Kind::Number, j);
            let name = self.ext_name(tterm, Kind::Number, i);
            if !self.version_filter(Kind::Number, i) || self.is_obsolete(&name) {
                continue;
            }
            let predval = pred(tterm, Kind::Number, i);
            if predval != FAIL {
                let value = tterm.numbers.get(i).copied().unwrap_or(ABSENT_NUMERIC);
                let mut buffer = name;
                if value < 0 {
                    buffer.push(b'@');
                } else {
                    buffer.push(b'#');
                    buffer.extend_from_slice(self.number_format(value).as_bytes());
                    if i.wrapping_add(1) > num_values {
                        num_values = i.wrapping_add(1);
                    }
                }
                self.wrap_concat1(&buffer);
                outcount = true;
            }
        }

        if self.column != self.indent && self.height > 1 {
            self.force_wrap();
        }

        let names_len = i32::try_from(cstr(&tterm.term_names).len()).unwrap_or(0);
        len = len
            .saturating_add(i32::try_from(num_bools).unwrap_or(0))
            .saturating_add(i32::try_from(num_values.saturating_mul(2)).unwrap_or(0))
            .saturating_add(names_len)
            .saturating_add(1);
        if len & 1 != 0 {
            len = len.wrapping_add(1);
        }

        if self.outform == F_TERMCAP {
            let get = |t: &TermType, i: usize| t.strings.get(i).cloned().unwrap_or_default();
            if let Str::Value(reset) = get(tterm, s::TERMCAP_RESET) {
                if get(tterm, s::INIT_3STRING) == Str::Value(reset.clone())
                    && let Some(slot) = tterm.strings.get_mut(s::INIT_3STRING)
                {
                    *slot = Str::Absent;
                }
                if get(tterm, s::RESET_2STRING) == Str::Value(reset)
                    && let Some(slot) = tterm.strings.get_mut(s::RESET_2STRING)
                {
                    *slot = Str::Absent;
                }
            }
        }

        for j in 0..tterm.strings.len() {
            let i = self.indirect(Kind::String, j);
            let name = self.ext_name(tterm, Kind::String, i);
            let mut capability = tterm.strings.get(i).cloned().unwrap_or_default();

            if !self.version_filter(Kind::String, i) || self.is_obsolete(&name) {
                continue;
            }
            // "Extended names can be longer than 2 characters, but termcap
            // programs cannot read those (filter them out)."
            if self.outform == F_TERMCAP && name.len() > 2 {
                continue;
            }

            if self.outform == F_TERMCAP {
                let present = |t: &TermType, k: usize| t.strings.get(k).is_some_and(Str::present);
                let absent =
                    |t: &TermType, k: usize| t.strings.get(k).is_none_or(|v| *v == Str::Absent);
                // "Some older versions of vi want rmir/smir to be defined for
                // ich/ich1 to work."
                if present(tterm, s::INSERT_CHARACTER) || present(tterm, s::PARM_ICH) {
                    if i == s::ENTER_INSERT_MODE && absent(tterm, s::ENTER_INSERT_MODE) {
                        self.wrap_concat1(b"im=");
                        outcount = true;
                        continue;
                    }
                    if i == s::EXIT_INSERT_MODE && absent(tterm, s::EXIT_INSERT_MODE) {
                        self.wrap_concat1(b"ei=");
                        outcount = true;
                        continue;
                    }
                }
                // "termcap applications such as screen will be confused if
                // sgr0 is translated to a string containing rmacs."
                if present(tterm, s::EXIT_ATTRIBUTE_MODE)
                    && i == s::EXIT_ATTRIBUTE_MODE
                    && let Some(trimmed) = self.trimmed_sgr0(tterm)
                    && capability.valid() != Some(trimmed.as_slice())
                {
                    capability = Str::Value(trimmed);
                }
            }

            let predval = pred(tterm, Kind::String, i);
            if predval != FAIL {
                if capability.present() && i.wrapping_add(1) > num_strings {
                    num_strings = i.wrapping_add(1);
                }
                match capability.valid() {
                    None => {
                        let mut buffer = name.clone();
                        buffer.push(b'@');
                        self.wrap_concat1(&buffer);
                        outcount = true;
                    }
                    Some(cap) if self.tc_output() => {
                        let srccap = tic_expand(Some(cap), true, numbers);
                        let params = match captab::PARAMETRIZED.get(i) {
                            Some(&p) => i32::from(p),
                            None => {
                                if srccap.first() == Some(&b'k') {
                                    0
                                } else {
                                    i32::from(has_params(&srccap, false))
                                }
                            }
                        };
                        match infotocap(&srccap, params, scan.strict_bsd) {
                            None if self.outform == F_TCONVERR => {
                                let mut buffer = name.clone();
                                buffer.extend_from_slice(b"=!!! ");
                                buffer.extend_from_slice(&srccap);
                                buffer.extend_from_slice(b" WILL NOT CONVERT !!!");
                                self.wrap_concat1(&buffer);
                                outcount = true;
                            }
                            None if suppress_untranslatable => continue,
                            None => {
                                let limit = MAX_TERMINFO_LENGTH + EXTRA_CAP;
                                let mut buffer: Vec<u8> = Vec::new();
                                let mut k = 0usize;
                                while let Some(&c) = srccap.get(k) {
                                    k = k.wrapping_add(1);
                                    if buffer.len().saturating_add(2) >= limit {
                                        let mut m = self.progname.clone();
                                        m.extend_from_slice(b": value for ");
                                        m.extend_from_slice(&name);
                                        m.extend_from_slice(b" is too long\n");
                                        scan.emit(&m);
                                        break;
                                    }
                                    if c == b':' {
                                        buffer.extend_from_slice(b"\\:");
                                    } else if c == b'\\' {
                                        buffer.push(c);
                                        match srccap.get(k) {
                                            None => break,
                                            Some(&next) => {
                                                buffer.push(next);
                                                k = k.wrapping_add(1);
                                            }
                                        }
                                    } else {
                                        buffer.push(c);
                                    }
                                }
                                let blen = i32::try_from(buffer.len()).unwrap_or(0);
                                let nlen = i32::try_from(name.len()).unwrap_or(0);
                                let mut need = nlen.saturating_add(blen).saturating_add(3);
                                self.wrap_concat(b"..", need, W_1ST | W_ERR);
                                need = need.wrapping_sub(2);
                                self.wrap_concat(&name, need, W_OFF | W_ERR);
                                need = need.wrapping_sub(nlen);
                                self.wrap_concat(b"=", need, W_2ND | W_ERR);
                                need = need.wrapping_sub(1);
                                self.wrap_concat(&buffer, need, W_END | W_ERR);
                                outcount = true;
                            }
                            Some(cv) => {
                                self.wrap_concat3(&name, b"=", &cv);
                                outcount = true;
                            }
                        }
                        len = len
                            .saturating_add(i32::try_from(cstr(cap).len()).unwrap_or(0))
                            .saturating_add(1);
                    }
                    Some(cap) => {
                        let src = tic_expand(Some(cap), self.outform == F_TERMINFO, numbers);
                        self.tmpbuf.clear();
                        self.tmpbuf.extend_from_slice(&name);
                        self.tmpbuf.push(b'=');
                        if self.pretty && (self.outform == F_TERMINFO || self.outform == F_VARIABLE)
                        {
                            let names = tterm.term_names.clone();
                            self.fmt_complex(scan, &names, &name, &src, 0, 1);
                        } else {
                            self.tmpbuf.extend_from_slice(&src);
                        }
                        len = len
                            .saturating_add(i32::try_from(cstr(cap).len()).unwrap_or(0))
                            .saturating_add(1);
                        let text = std::mem::take(&mut self.tmpbuf);
                        self.wrap_concat1(&text);
                        self.tmpbuf = text;
                        outcount = true;
                    }
                }
            }
        }
        len = len.saturating_add(i32::try_from(num_strings.saturating_mul(2)).unwrap_or(0));

        // "This piece of code should be an effective inverse of the
        // functions postprocess_terminfo() and postprocess_terminfo() in
        // parse_entry.c."
        if self.tversion == V_HPUX {
            for (i, label) in [
                (s::MEMORY_LOCK, &b"meml="[..]),
                (s::MEMORY_UNLOCK, b"memu="),
            ] {
                if let Some(v) = tterm.strings.get(i).and_then(Str::valid) {
                    let mut buffer = label.to_vec();
                    buffer.extend_from_slice(cstr(v));
                    self.wrap_concat1(&buffer);
                    outcount = true;
                }
            }
        } else if self.tversion == V_AIX
            && let Some(acsc) = tterm.strings.get(s::ACS_CHARS).and_then(Str::valid)
        {
            let acsc = cstr(acsc);
            let mut boxchars = Vec::new();
            let mut box_ok = true;
            for &cp in b"lqkxjmwuvtn" {
                match acsc.iter().position(|&c| c == cp) {
                    Some(p) => boxchars.push(at(acsc, p.wrapping_add(1))),
                    None => {
                        box_ok = false;
                        break;
                    }
                }
            }
            if box_ok {
                let tmp = tic_expand(Some(cstr(&boxchars)), self.outform == F_TERMINFO, numbers);
                let mut buffer = b"box1=".to_vec();
                let room = MAX_TERMINFO_LENGTH + EXTRA_CAP - 1;
                let take = room.saturating_sub(buffer.len()).min(tmp.len());
                buffer.extend_from_slice(tmp.get(..take).unwrap_or_default());
                self.wrap_concat1(&buffer);
                outcount = true;
            }
        }

        // "kludge: trim off trailer to avoid an extra blank line in infocmp
        // -u output when there are no string differences"
        if outcount {
            let j = self.outbuf.len();
            let mut trimmed = false;
            if self.wrapped && self.did_wrap {
                // EMPTY
            } else if j >= 2 && self.outbuf.get(j.wrapping_sub(2)..) == Some(&b"\n\t"[..]) {
                self.outbuf.truncate(j.wrapping_sub(2));
                trimmed = true;
            } else if j >= 4 && self.outbuf.get(j.wrapping_sub(4)..) == Some(&b"\\\n\t:"[..]) {
                self.outbuf.truncate(j.wrapping_sub(4));
                trimmed = true;
            }
            if trimmed {
                self.column = self.oldcol;
                self.outbuf.push(b' ');
            }
        }

        if infodump {
            len
        } else {
            i32::try_from(cstr(&self.outbuf).len()).unwrap_or(i32::MAX)
        }
    }

    /// `kill_string`, through `find_string`: the string capability whose
    /// short name is `name` made absent; its length, if it had a value.
    fn kill_named(&self, tterm: &mut TermType, name: &[u8]) -> Option<usize> {
        let found = (0..tterm.strings.len()).find(|&n| {
            self.version_filter(Kind::String, n)
                && names::STRNAMES.get(n).is_some_and(|s| s.as_bytes() == name)
        })?;
        let len = tterm
            .strings
            .get(found)
            .and_then(Str::valid)
            .map(|v| cstr(v).len())?;
        if let Some(slot) = tterm.strings.get_mut(found) {
            *slot = Str::Absent;
        }
        Some(len)
    }

    /// `kill_labels (tterm, target)`: function-key labels removed, until
    /// `target` bytes have gone; how many.
    fn kill_labels(&self, tterm: &mut TermType, target: i32) -> i32 {
        let mut target = target;
        let mut result = 0i32;
        for n in 0..=10 {
            if let Some(len) = self.kill_named(tterm, format!("lf{n}").as_bytes()) {
                target = target.wrapping_sub(i32::try_from(len).unwrap_or(0).saturating_add(5));
                result = result.wrapping_add(1);
                if target < 0 {
                    break;
                }
            }
        }
        result
    }

    /// `kill_fkeys (tterm, target)`: function keys removed, from the last.
    fn kill_fkeys(&self, tterm: &mut TermType, target: i32) -> i32 {
        let mut target = target;
        let mut result = 0i32;
        for n in (0..=60).rev() {
            if let Some(len) = self.kill_named(tterm, format!("kf{n}").as_bytes()) {
                target = target.wrapping_sub(i32::try_from(len).unwrap_or(0).saturating_add(5));
                result = result.wrapping_add(1);
                if target < 0 {
                    break;
                }
            }
        }
        result
    }

    /// `purged_acs (tterm)`: whether the entry has `acsc` -- having, if its
    /// line-drawing map is not one-to-one, dropped `smacs` and `rmacs`.
    fn purged_acs(&mut self, tterm: &mut TermType) -> bool {
        let Some(acsc) = tterm.strings.get(s::ACS_CHARS).and_then(Str::valid) else {
            return false;
        };
        let acsc = cstr(acsc);
        let mut one_one = true;
        let mut k = 0usize;
        while at(acsc, k) != 0 && at(acsc, k.wrapping_add(1)) != 0 {
            if b"lmkjtuvwqxn".contains(&at(acsc, k)) && at(acsc, k) != at(acsc, k.wrapping_add(1)) {
                one_one = false;
                break;
            }
            k = k.wrapping_add(2);
        }
        if !one_one {
            for i in [s::ENTER_ALT_CHARSET_MODE, s::EXIT_ALT_CHARSET_MODE] {
                if let Some(slot) = tterm.strings.get_mut(i) {
                    *slot = Str::Absent;
                }
            }
            self.stdout
                .extend_from_slice(b"# (rmacs/smacs removed for consistency)\n");
        }
        true
    }

    /// `FMT_ENTRY ()`.
    fn fmt_again(
        &mut self,
        scan: &mut Scanner<'_>,
        tterm: &mut TermType,
        pred: Option<Pred<'_>>,
        suppress: bool,
        infodump: bool,
        numbers: i32,
    ) -> i32 {
        self.fmt_entry(scan, tterm, pred, false, suppress, infodump, numbers)
    }

    /// `dump_entry (tterm, suppress_untranslatable, limited, numbers,
    /// pred)`: the entry formatted, cut down to fit termcap when it must
    /// be. What upstream changes in the entry as it cuts stays changed, as
    /// upstream's copy of the description shares its arrays.
    #[allow(
        clippy::too_many_lines,
        reason = "upstream's dump_entry, in one piece so it reads against it"
    )]
    pub fn dump_entry(
        &mut self,
        scan: &mut Scanner<'_>,
        tterm: &mut TermType,
        suppress_untranslatable: bool,
        limited: bool,
        numbers: i32,
        pred: Option<Pred<'_>>,
    ) {
        let mut suppress = suppress_untranslatable;
        if self.quickdump != 0 {
            self.separator = b"";
            self.trailer = b"\n";
            self.indent = 0;
            if let Some(object) = write_object(tterm, self.user_definable, 65536) {
                if self.quickdump & 1 != 0 {
                    if !self.outbuf.is_empty() {
                        self.wrap_concat1(b"\n");
                    }
                    self.wrap_concat1(b"hex:");
                    for byte in &object {
                        self.wrap_concat1(format!("{byte:02X}").as_bytes());
                    }
                }
                if self.quickdump & 2 != 0 {
                    if !self.outbuf.is_empty() {
                        self.wrap_concat1(b"\n");
                    }
                    self.wrap_concat1(b"b64:");
                    let mut saved = 0i32;
                    let count = object.len();
                    for nn in 0..count {
                        let piece = encode_b64(&object, nn, &mut saved);
                        self.wrap_concat1(&piece);
                    }
                    match count % 3 {
                        1 => {
                            let piece = encode_b64(&[0, 0], 1, &mut saved);
                            self.wrap_concat1(&piece);
                            self.wrap_concat1(b"==");
                        }
                        2 => {
                            let piece = encode_b64(&[0, 0], 1, &mut saved);
                            self.wrap_concat1(&piece);
                            self.wrap_concat1(b"=");
                        }
                        _ => {}
                    }
                }
            }
            return;
        }

        let (critlen, legend, infodump) = if self.tc_output() {
            set_obsolete_termcaps(tterm);
            (MAX_TERMCAP_LENGTH, "older termcap", false)
        } else {
            (
                i32::try_from(MAX_TERMINFO_LENGTH).unwrap_or(i32::MAX),
                "terminfo",
                true,
            )
        };

        self.save_sgr = tterm
            .strings
            .get(s::SET_ATTRIBUTES)
            .cloned()
            .unwrap_or_default();

        if self.fmt_again(scan, tterm, pred, suppress, infodump, numbers) > critlen
            && self.tc_output()
            && limited
        {
            if !suppress {
                self.stdout.extend_from_slice(
                    format!(
                        "# (untranslatable capabilities removed to fit entry within {critlen} bytes)\n"
                    )
                    .as_bytes(),
                );
                suppress = true;
            }
            if self.fmt_again(scan, tterm, pred, suppress, infodump, numbers) > critlen {
                // "We pick on sgr because it is a nice long string capability
                // that is really just an optimization hack."
                let mut changed = false;
                for nn in STRCOUNT..tterm.strings.len() {
                    let name = self.ext_name(tterm, Kind::String, nn);
                    if tterm.strings.get(nn).is_some_and(Str::present) {
                        if let Some(slot) = tterm.strings.get_mut(s::SET_ATTRIBUTES) {
                            *slot = Str::Absent;
                        }
                        // "we remove long names anyway - only report the
                        // short"
                        if name.len() <= 2 {
                            let mut m = b"# (".to_vec();
                            m.extend_from_slice(&name);
                            m.extend_from_slice(
                                format!(" removed to fit entry within {critlen} bytes)\n")
                                    .as_bytes(),
                            );
                            self.stdout.extend_from_slice(&m);
                        }
                        changed = true;
                        if self.fmt_again(scan, tterm, pred, suppress, infodump, numbers) <= critlen
                        {
                            break;
                        }
                    }
                }
                if tterm
                    .strings
                    .get(s::SET_ATTRIBUTES)
                    .is_some_and(Str::present)
                {
                    if let Some(slot) = tterm.strings.get_mut(s::SET_ATTRIBUTES) {
                        *slot = Str::Absent;
                    }
                    self.stdout.extend_from_slice(
                        format!("# (sgr removed to fit entry within {critlen} bytes)\n").as_bytes(),
                    );
                    changed = true;
                }
                if (!changed
                    || self.fmt_again(scan, tterm, pred, suppress, infodump, numbers) > critlen)
                    && self.purged_acs(tterm)
                {
                    if let Some(slot) = tterm.strings.get_mut(s::ACS_CHARS) {
                        *slot = Str::Absent;
                    }
                    self.stdout.extend_from_slice(
                        format!("# (acsc removed to fit entry within {critlen} bytes)\n")
                            .as_bytes(),
                    );
                    changed = true;
                }
                if !changed
                    || self.fmt_again(scan, tterm, pred, suppress, infodump, numbers) > critlen
                {
                    let oldversion = self.tversion;
                    self.tversion = V_BSD;
                    self.stdout.extend_from_slice(
                        format!(
                            "# (terminfo-only capabilities suppressed to fit entry within {critlen} bytes)\n"
                        )
                        .as_bytes(),
                    );
                    let mut len = self.fmt_again(scan, tterm, pred, suppress, infodump, numbers);
                    if len > critlen && self.kill_labels(tterm, len.wrapping_sub(critlen)) != 0 {
                        self.stdout.extend_from_slice(
                            format!(
                                "# (some labels capabilities suppressed to fit entry within {critlen} bytes)\n"
                            )
                            .as_bytes(),
                        );
                        len = self.fmt_again(scan, tterm, pred, suppress, infodump, numbers);
                    }
                    if len > critlen && self.kill_fkeys(tterm, len.wrapping_sub(critlen)) != 0 {
                        self.stdout.extend_from_slice(
                            format!(
                                "# (some function-key capabilities suppressed to fit entry within {critlen} bytes)\n"
                            )
                            .as_bytes(),
                        );
                        len = self.fmt_again(scan, tterm, pred, suppress, infodump, numbers);
                    }
                    if len > critlen {
                        let mut m = self.progname.clone();
                        m.extend_from_slice(b": ");
                        m.extend_from_slice(&first_name(&tterm.term_names));
                        m.extend_from_slice(format!(" entry is {len} bytes long\n").as_bytes());
                        scan.emit(&m);
                        self.stdout.extend_from_slice(
                            format!("# WARNING: this entry, {len} bytes long, may core-dump {legend} libraries!\n")
                                .as_bytes(),
                        );
                    }
                    self.tversion = oldversion;
                }
                let sgr = self.save_sgr.clone();
                if let Some(slot) = tterm.strings.get_mut(s::SET_ATTRIBUTES) {
                    *slot = sgr;
                }
            }
        } else if !self.version_filter(Kind::String, s::ACS_CHARS) && self.purged_acs(tterm) {
            self.fmt_again(scan, tterm, pred, suppress, infodump, numbers);
        }
    }

    /// `dump_uses (value, infodump)`: a `use=` (or `tc=`) clause.
    pub fn dump_uses(&mut self, scan: &mut Scanner<'_>, value: Option<&[u8]>, infodump: bool) {
        let cap = if infodump { "use" } else { "tc" };
        if self.tc_output() {
            self.trim_trailing();
        }
        let mut value = value.map(cstr).unwrap_or_default();
        let mut limit = value.len();
        if limit == 0 {
            scan.warning(format!("empty \"{cap}\" field").as_bytes());
            value = b"";
        } else if limit > MAX_ALIAS {
            scan.warning(
                format!("\"{cap}\" field too long ({limit}), limit to {MAX_ALIAS}").as_bytes(),
            );
            limit = MAX_ALIAS;
        }
        let mut buffer = format!("{cap}=").into_bytes();
        buffer.extend_from_slice(value.get(..limit).unwrap_or(value));
        self.wrap_concat1(&buffer);
    }

    /// `show_entry ()`: the entry printed, its trailing separator kept and
    /// any white space after it dropped; its length.
    pub fn show_entry(&mut self) -> i32 {
        if !self.outbuf.is_empty() {
            let infodump = !self.tc_output();
            let delim = if infodump { b',' } else { b':' };
            let mut j = self.outbuf.len().saturating_sub(1);
            while j > 0 {
                let ch = at(&self.outbuf, j);
                if ch == b'\n' {
                    // keep looking
                } else if isspace(ch) || (!infodump && ch == b'\\') {
                    self.outbuf.truncate(j);
                } else if ch == delim && at(&self.outbuf, j.wrapping_sub(1)) != b'\\' {
                    self.outbuf.truncate(j.wrapping_add(1));
                } else {
                    break;
                }
                j = j.wrapping_sub(1);
            }
        }
        if self.touched {
            let text = cstr(&self.outbuf).to_vec();
            self.stdout.extend_from_slice(&text);
            self.stdout.push(b'\n');
        }
        i32::try_from(self.outbuf.len()).unwrap_or(i32::MAX)
    }

    /// `compare_entry (hook, tp, quiet)`: `hook` called for each
    /// capability of `tp`, in the sort order, then for the `use=` clauses.
    pub fn compare_entry(&mut self, hook: &mut CompareHook<'_>, tp: &TermType, quiet: bool) {
        if !quiet {
            self.stdout.extend_from_slice(b"    comparing booleans.\n");
        }
        for j in 0..tp.booleans.len() {
            let i = self.indirect(Kind::Boolean, j);
            let name = self.ext_name(tp, Kind::Boolean, i);
            if self.is_obsolete(&name) {
                continue;
            }
            hook(self, Some(Kind::Boolean), i, &name);
        }
        if !quiet {
            self.stdout.extend_from_slice(b"    comparing numbers.\n");
        }
        for j in 0..tp.numbers.len() {
            let i = self.indirect(Kind::Number, j);
            let name = self.ext_name(tp, Kind::Number, i);
            if self.is_obsolete(&name) {
                continue;
            }
            hook(self, Some(Kind::Number), i, &name);
        }
        if !quiet {
            self.stdout.extend_from_slice(b"    comparing strings.\n");
        }
        for j in 0..tp.strings.len() {
            let i = self.indirect(Kind::String, j);
            let name = self.ext_name(tp, Kind::String, i);
            if self.is_obsolete(&name) {
                continue;
            }
            hook(self, Some(Kind::String), i, &name);
        }
        // `CMP_USE`: the `use=` clauses, compared as a whole.
        hook(self, None, 0, b"use");
    }
}

/// `encode_b64 (target, source, state, &saved)`: the base-64 characters
/// byte `state` of `source` completes.
fn encode_b64(source: &[u8], state: usize, saved: &mut i32) -> Vec<u8> {
    const DATA: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let ch = i32::from(at(source, state));
    let pick = |v: i32| {
        DATA.get(usize::try_from(v & 0o77).unwrap_or(0))
            .copied()
            .unwrap_or(b'A')
    };
    let mut out = Vec::new();
    match state % 3 {
        0 => {
            out.push(pick(ch >> 2));
            *saved = ch << 4;
        }
        1 => {
            out.push(pick((ch >> 4) | *saved));
            *saved = ch << 2;
        }
        _ => {
            out.push(pick((ch >> 6) | *saved));
            out.push(pick(ch));
            *saved = 0;
        }
    }
    out
}

/// `EXTRACT_DELAY (str)`: the number after the first `*` of a string --
/// `atoi`'s, kept to a `short` -- or 0.
fn extract_delay(v: &[u8]) -> i32 {
    let v = cstr(v);
    let Some(star) = v.iter().position(|&c| c == b'*') else {
        return 0;
    };
    let (n, _) = cstrtol::strtol(v.get(star.wrapping_add(1)..).unwrap_or_default(), 10);
    i32::from(i16::from_le_bytes(
        cstrtol::low_i32(n)
            .to_le_bytes()
            .get(..2)
            .and_then(|b| b.try_into().ok())
            .unwrap_or([0, 0]),
    ))
}

/// `set_obsolete_termcaps (tp)` -- `capdefaults.c`: termcap's obsolete
/// capabilities computed back from terminfo's.
fn set_obsolete_termcaps(tp: &mut TermType) {
    let get = |t: &TermType, i: usize| t.strings.get(i).and_then(Str::valid).map(<[u8]>::to_vec);
    let set_num = |t: &mut TermType, i: usize, v: i32| {
        if let Some(slot) = t.numbers.get_mut(i) {
            *slot = v;
        }
    };
    // "current (4.4BSD) capabilities marked obsolete"
    if let Some(cr) = get(tp, s::CARRIAGE_RETURN) {
        let d = extract_delay(&cr);
        if d != 0 {
            set_num(tp, n::CARRIAGE_RETURN_DELAY, d);
        }
    }
    if let Some(nel) = get(tp, s::NEWLINE) {
        let d = extract_delay(&nel);
        if d != 0 {
            set_num(tp, n::NEW_LINE_DELAY, d);
        }
    }
    // "current (4.4BSD) capabilities not obsolete"
    if get(tp, s::TERMCAP_INIT2).is_none()
        && let Some(is3) = get(tp, s::INIT_3STRING)
    {
        if let Some(slot) = tp.strings.get_mut(s::TERMCAP_INIT2) {
            *slot = Str::Value(is3);
        }
        if let Some(slot) = tp.strings.get_mut(s::INIT_3STRING) {
            *slot = Str::Absent;
        }
    }
    if get(tp, s::TERMCAP_RESET).is_none()
        && get(tp, s::RESET_1STRING).is_none()
        && get(tp, s::RESET_3STRING).is_none()
        && let Some(rs2) = get(tp, s::RESET_2STRING)
    {
        if let Some(slot) = tp.strings.get_mut(s::TERMCAP_RESET) {
            *slot = Str::Value(rs2);
        }
        if let Some(slot) = tp.strings.get_mut(s::RESET_2STRING) {
            *slot = Str::Absent;
        }
    }
    let num = |t: &TermType, i: usize| t.numbers.get(i).copied().unwrap_or(ABSENT_NUMERIC);
    if num(tp, n::MAGIC_COOKIE_GLITCH_UL) == ABSENT_NUMERIC
        && num(tp, n::MAGIC_COOKIE_GLITCH) != ABSENT_NUMERIC
        && get(tp, s::ENTER_UNDERLINE_MODE).is_some()
    {
        let v = num(tp, n::MAGIC_COOKIE_GLITCH);
        set_num(tp, n::MAGIC_COOKIE_GLITCH_UL, v);
    }
    // "totally obsolete capabilities"
    let lf = get(tp, s::NEWLINE).is_some_and(|v| cstr(&v) == b"\n");
    if let Some(slot) = tp.booleans.get_mut(b::LINEFEED_IS_NEWLINE) {
        *slot = i8::from(lf);
    }
    if let Some(cub1) = get(tp, s::CURSOR_LEFT) {
        let d = extract_delay(&cub1);
        if d != 0 {
            set_num(tp, n::BACKSPACE_DELAY, d);
        }
    }
    if let Some(ht) = get(tp, s::TAB) {
        let d = extract_delay(&ht);
        if d != 0 {
            set_num(tp, n::HORIZONTAL_TAB_DELAY, d);
        }
    }
}

/// `repair_acsc (tp)`: an `acsc` whose pairs are out of order, or repeat
/// one, sorted and made unique -- an unpaired last character kept at the
/// end.
pub fn repair_acsc(tp: &mut TermType) {
    let Some(acsc) = tp
        .strings
        .get(s::ACS_CHARS)
        .and_then(Str::valid)
        .map(|v| cstr(v).to_vec())
    else {
        return;
    };
    let mut fix_needed = false;
    let mut source = 0u32;
    let mut k = 0usize;
    while at(&acsc, k) != 0 {
        let target = u32::from(at(&acsc, k));
        if source >= target {
            fix_needed = true;
            break;
        }
        source = target;
        if at(&acsc, k.wrapping_add(1)) != 0 {
            k = k.wrapping_add(1);
        }
        k = k.wrapping_add(1);
    }
    if !fix_needed {
        return;
    }
    let mut mapped = [0u8; 256];
    let mut extra = 0u8;
    let mut k = 0usize;
    while at(&acsc, k) != 0 {
        let src = at(&acsc, k);
        let tgt = at(&acsc, k.wrapping_add(1));
        if tgt != 0 {
            if let Some(slot) = mapped.get_mut(usize::from(src)) {
                *slot = tgt;
            }
            k = k.wrapping_add(1);
        } else {
            extra = src;
        }
        k = k.wrapping_add(1);
    }
    let mut out = Vec::new();
    for (c, &m) in mapped.iter().enumerate() {
        if m != 0 {
            out.push(u8::try_from(c).unwrap_or(0));
            out.push(m);
        }
    }
    if extra != 0 {
        // "garbage in, garbage out"
        out.push(extra);
    }
    if let Some(slot) = tp.strings.get_mut(s::ACS_CHARS) {
        *slot = Str::Value(out);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn dumper(mode: i32, sort: i32, width: i32) -> Dumper {
        let mut d = Dumper::new(b"infocmp", false);
        let mut diag = Vec::new();
        let mut scan = Scanner::new(&mut diag);
        d.init(
            &mut scan, None, mode, sort, false, width, 65535, 0, false, false, 0,
        );
        d
    }

    fn sample() -> TermType {
        let mut t = TermType::init();
        t.term_names = b"x|test terminal".to_vec();
        t.booleans[1] = 1; // am
        t.numbers[0] = 80; // cols
        t.numbers[2] = 24; // lines
        t.strings[1] = Str::Value(b"\x07".to_vec()); // bel
        t.strings[2] = Str::Value(b"\r".to_vec()); // cr
        t
    }

    #[test]
    fn an_entry_dumps_as_terminfo_source() {
        let mut d = dumper(F_TERMINFO, S_TERMINFO, 60);
        let mut diag = Vec::new();
        let mut scan = Scanner::new(&mut diag);
        let mut t = sample();
        d.dump_entry(&mut scan, &mut t, false, true, 0, None);
        d.show_entry();
        assert_eq!(
            String::from_utf8(d.stdout.clone()).unwrap(),
            "x|test terminal,\n\tam,\n\tcols#80, lines#24,\n\tbel=^G, cr=\\r,\n"
        );
    }

    #[test]
    fn one_column_puts_each_capability_on_its_line() {
        let mut d = dumper(F_TERMINFO, S_TERMINFO, 0);
        let mut diag = Vec::new();
        let mut scan = Scanner::new(&mut diag);
        let mut t = sample();
        d.dump_entry(&mut scan, &mut t, false, true, 0, None);
        d.show_entry();
        assert_eq!(
            String::from_utf8(d.stdout.clone()).unwrap(),
            "x|test terminal,\n\tam,\n\tcols#80,\n\tlines#24,\n\tbel=^G,\n\tcr=\\r,\n"
        );
    }

    #[test]
    fn termcap_output_uses_termcap_names() {
        let mut d = dumper(F_TERMCAP, S_TERMCAP, 60);
        d.tversion = V_BSD;
        let mut diag = Vec::new();
        let mut scan = Scanner::new(&mut diag);
        let mut t = sample();
        d.dump_entry(&mut scan, &mut t, false, true, 0, None);
        d.show_entry();
        let out = String::from_utf8(d.stdout.clone()).unwrap();
        assert!(
            out.starts_with("x|test terminal:\\\n\t:am:\\\n\t:co#80:li#24:\\\n\t:bl=^G:cr=\\r:"),
            "{out}"
        );
    }

    #[test]
    fn acsc_is_repaired_into_order() {
        let mut t = TermType::init();
        t.strings[s::ACS_CHARS] = Str::Value(b"qqjjaa".to_vec());
        repair_acsc(&mut t);
        assert_eq!(t.strings[s::ACS_CHARS], Str::Value(b"aajjqq".to_vec()));
    }

    #[test]
    fn large_numbers_near_a_power_of_two_show_in_hex() {
        let d = dumper(F_TERMINFO, S_TERMINFO, 60);
        assert_eq!(d.number_format(256), "0x100");
        assert_eq!(d.number_format(32767), "0x7fff");
        assert_eq!(d.number_format(1000), "1000");
        assert_eq!(d.number_format(80), "80");
    }
}
