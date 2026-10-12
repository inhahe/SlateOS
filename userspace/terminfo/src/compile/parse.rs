//! `parse_entry.c`: one entry of a source compiled, `use=` clauses
//! recorded but not followed.
//!
//! An entry is its names line and every capability up to the next names
//! line. A name the tables do not know may be an alias termcap or terminfo
//! once had (`font0` for `s0ds`), a terminfo long name (`bell`), or -- with
//! `-x`, `_nc_user_definable` -- an extended capability the entry defines
//! for itself. A termcap entry then gets what termcap left to defaults
//! (`cr` is `^M`, `bel` is `^G`, unless cancelled) and its obsolete
//! capabilities translated (`ko`, the XENIX box characters); a terminfo
//! one, AIX's box characters.

use super::Abort;
use super::caps::{b, n, s};
use super::captoinfo::captoinfo;
use super::entry::{Entry, MAX_USES, StrBuf, StrDesc, Use, capcmp, init_entry, visbuf, wrap_entry};
use super::scan::{SYN_TERMCAP, Scanner, TokenType, cstr};
use super::tables::{self, first_name};
use crate::Kind;
use crate::captab::{self, PARAMETRIZED};
use crate::entry::{ABSENT_NUMERIC, BOOLCOUNT, CANCELLED_NUMERIC, NUMCOUNT, STRCOUNT};
use crate::termtype::{CANCELLED_BOOLEAN, Str, TermType, open};

/// `MAX_ALIAS`: a name longer than this is warned of.
const MAX_ALIAS: usize = 32;
/// `MAX_LINE`.
const MAX_LINE: usize = 132;
/// `MAX_TERMCAP_LENGTH`.
const MAX_TERMCAP_LENGTH: usize = 1023;
/// `VT_ACSC`: a VT100's alternate characters, the default where an entry
/// can switch to them but does not say what they are.
pub const VT_ACSC: &[u8] = b"``aaffggiijjkkllmmnnooppqqrrssttuuvvwwxxyyzz{{||}}~~";

/// What `_nc_parse_entry` returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Parsed {
    /// `OK`: an entry.
    Ok,
    /// `EOF`: no more.
    Eof,
    /// `ERR`: the names could not be saved.
    Err,
}

/// The compiler: the scanner, and the state the rest of the compiler keeps
/// in globals.
pub struct Compiler<'d> {
    /// The scanner, and the warnings' context.
    pub scan: Scanner<'d>,
    /// `stringbuf`, `next_free`.
    pub strbuf: StrBuf,
    /// `_nc_user_definable`: extended capabilities (`-x`).
    pub user_definable: bool,
    /// The entries read: `_nc_head` to `_nc_tail`.
    pub entries: Vec<Entry>,
    /// The entries `use=` brought in from the database.
    pub disk: Vec<Entry>,
    /// The environment the database is found by.
    pub env: crate::Env,
    /// `_nc_tic_dir`: the directory `tic` writes to, looked in first.
    pub tic_dir: Option<Vec<u8>>,
    /// `_nc_check_termtype2`, when `tic` has replaced it.
    pub check_termtype: Option<Box<dyn CheckTermtype + 'd>>,
    /// `_nc_tracing`: `tic -vN`'s level.
    pub tracing: u32,
}

/// `_nc_check_termtype2`: what checks each entry once its `use=` are
/// merged -- `tic`'s own checks when it asks for them.
pub trait CheckTermtype {
    /// Check `tp`, the entry `scan` names in its warnings.
    fn check(&mut self, scan: &mut Scanner<'_>, tp: &mut TermType, literal: bool);
}

/// `DEBUG_LEVEL (1)`: the tracing level from which `tic` names each
/// extended capability it makes.
pub const DEBUG_LEVEL_1: u32 = 1 << 13;

/// What a capability name was found to be: `struct name_table_entry`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Found {
    pub(crate) kind: Kind,
    pub(crate) index: usize,
}

impl From<&captab::Cap> for Found {
    fn from(c: &captab::Cap) -> Self {
        Self {
            kind: c.kind,
            index: c.index,
        }
    }
}

/// `valid_entryname`: printable ASCII but for `/\|=,:`, and `#` or `@`
/// only first.
fn valid_entryname(name: &[u8]) -> bool {
    for (i, &ch) in cstr(name).iter().enumerate() {
        if ch <= b' ' || ch > b'~' || b"/\\|=,:".contains(&ch) {
            return false;
        }
        if i != 0 && b"#@".contains(&ch) {
            return false;
        }
    }
    true
}

/// `usertype2s`.
fn usertype2s(mask: u32) -> &'static str {
    if mask & 1 != 0 {
        "boolean"
    } else if mask & 2 != 0 {
        "number"
    } else if mask & 4 != 0 {
        "string"
    } else {
        "unknown"
    }
}

/// `1 << token_type`.
fn type_bit(t: TokenType) -> u32 {
    match t {
        TokenType::Boolean => 1,
        TokenType::Number => 2,
        TokenType::String => 4,
        TokenType::Cancel => 8,
        TokenType::Names => 16,
        TokenType::Undef => 32,
        // `EOF` is -1: `1 << -1` is no bit any mask has.
        TokenType::Eof => 0,
    }
}

/// `isgraph` in the C locale.
fn isgraph(c: u8) -> bool {
    (0x21..0x7f).contains(&c)
}

/// The string capability `i`.
fn str_at(tp: &TermType, i: usize) -> &Str {
    tp.strings.get(i).unwrap_or(&Str::Absent)
}

/// Set the string capability `i`.
fn set_str(tp: &mut TermType, i: usize, v: Str) {
    if let Some(slot) = tp.strings.get_mut(i) {
        *slot = v;
    }
}

/// The number capability `i`.
fn num_at(tp: &TermType, i: usize) -> i32 {
    tp.numbers.get(i).copied().unwrap_or(ABSENT_NUMERIC)
}

/// The boolean capability `i`.
fn bool_at(tp: &TermType, i: usize) -> i8 {
    tp.booleans.get(i).copied().unwrap_or(0)
}

/// A C-style `"%s$<%d>"`.
fn delayed(base: &[u8], delay: i32) -> Vec<u8> {
    let mut v = base.to_vec();
    v.extend_from_slice(format!("$<{delay}>").as_bytes());
    v
}

/// `ko_xlate`: termcap's `ko` names, and the key capability each means.
const KO_XLATE: [(&str, &str); 19] = [
    ("al", "kil1"),
    ("bt", "kcbt"),
    ("cd", "ked"),
    ("ce", "kel"),
    ("cl", "kclr"),
    ("ct", "tbc"),
    ("dc", "kdch1"),
    ("dl", "kdl1"),
    ("do", "kcud1"),
    ("ei", "krmir"),
    ("ho", "khome"),
    ("ic", "kich1"),
    ("im", "kIC"),
    ("le", "kcub1"),
    ("nd", "kcuf1"),
    ("nl", "kent"),
    ("st", "khts"),
    ("ta", ""),
    ("up", "kcuu1"),
];

/// `append_acs0 (dst, code, src, off)`.
fn append_acs0(dst: &mut StrDesc, code: u8, src: &Str, off: usize) {
    if let Some(src) = src.valid().map(cstr)
        && let Some(&c) = src.get(off)
    {
        dst.cat(Some(&[code, c]));
    }
}

/// `append_acs (dst, code, src)`: one character of a one-character string.
fn append_acs(dst: &mut StrDesc, code: u8, src: &Str) {
    if src.valid().map(cstr).is_some_and(|v| v.len() == 1) {
        append_acs0(dst, code, src, 0);
    }
}

impl<'d> Compiler<'d> {
    /// A compiler with its messages to `diag`.
    pub fn new(diag: &'d mut dyn super::Diagnostics) -> Self {
        Self {
            scan: Scanner::new(diag),
            strbuf: StrBuf::default(),
            user_definable: true,
            entries: Vec::new(),
            disk: Vec::new(),
            env: crate::Env::default(),
            tic_dir: None,
            check_termtype: None,
            tracing: 0,
        }
    }

    /// `_nc_save_str`.
    fn save(&mut self, string: &[u8]) -> Option<Vec<u8>> {
        self.strbuf.save(&mut self.scan, string)
    }

    /// `_nc_save_str` into a string capability.
    fn save_value(&mut self, string: &[u8]) -> Str {
        self.strbuf.save_value(&mut self.scan, string)
    }

    /// `_nc_extend_names (entryp, name, token_type)`: the extended
    /// capability `name` of that type, made if the entry does not have it.
    fn extend_names(
        &mut self,
        tp: &mut TermType,
        name: &[u8],
        token_type: TokenType,
    ) -> Option<Found> {
        let (kind, first, last, mut offset, mut tindex) = match token_type {
            TokenType::Boolean => (
                Kind::Boolean,
                0,
                tp.ext_booleans,
                tp.ext_booleans,
                tp.booleans.len(),
            ),
            TokenType::Number => (
                Kind::Number,
                tp.ext_booleans,
                tp.ext_numbers.saturating_add(tp.ext_booleans),
                tp.ext_booleans.saturating_add(tp.ext_numbers),
                tp.numbers.len(),
            ),
            TokenType::String => {
                let first = tp.ext_booleans.saturating_add(tp.ext_numbers);
                (
                    Kind::String,
                    first,
                    tp.ext_strings.saturating_add(first),
                    tp.num_ext_names(),
                    tp.strings.len(),
                )
            }
            TokenType::Cancel => {
                for n in 0..tp.num_ext_names() {
                    if tp.ext_name(n) == name {
                        // Upstream's tests are off by one at each boundary.
                        let t = if n > tp.ext_booleans.saturating_add(tp.ext_numbers) {
                            TokenType::String
                        } else if n > tp.ext_booleans {
                            TokenType::Number
                        } else {
                            TokenType::Boolean
                        };
                        return self.extend_names(tp, name, t);
                    }
                }
                // "we are given a cancel for a name that we don't recognize"
                return self.extend_names(tp, name, TokenType::String);
            }
            _ => return None,
        };

        // "Adjust the 'offset' (insertion-point) to keep the lists of
        // extended names sorted."
        let mut found = false;
        for nn in first..last {
            let cmp = tp.ext_name(nn).cmp(name);
            if cmp == std::cmp::Ordering::Equal {
                found = true;
            }
            if cmp != std::cmp::Ordering::Less {
                offset = nn;
                tindex = nn.saturating_sub(first).saturating_add(match kind {
                    Kind::Boolean => BOOLCOUNT,
                    Kind::Number => NUMCOUNT,
                    Kind::String => STRCOUNT,
                });
                break;
            }
        }

        if !found {
            let saved = self.save(name)?;
            // The value at `tindex` is left as it was -- the caller sets it.
            match kind {
                Kind::Boolean => {
                    tp.ext_booleans = tp.ext_booleans.saturating_add(1);
                    open(&mut tp.booleans, tindex);
                }
                Kind::Number => {
                    tp.ext_numbers = tp.ext_numbers.saturating_add(1);
                    open(&mut tp.numbers, tindex);
                }
                Kind::String => {
                    tp.ext_strings = tp.ext_strings.saturating_add(1);
                    open(&mut tp.strings, tindex);
                }
            }
            let at = offset.min(tp.ext_names.len());
            tp.ext_names.insert(at, Some(saved));
        }
        Some(Found {
            kind,
            index: tindex,
        })
    }

    /// `expected_type (name, token_type, silent)`: false, with a warning,
    /// for a user-definable capability ncurses knows to be of another type.
    fn expected_type(&mut self, name: &[u8], token_type: TokenType, silent: bool) -> bool {
        if let Some(entry) = tables::find_user_entry(name)
            && token_type != TokenType::Cancel
        {
            let have = type_bit(token_type);
            let kinds = u32::from(entry.kinds);
            if kinds & have == 0 {
                if !silent {
                    let mut m = format!("expected {}-type for ", usertype2s(kinds)).into_bytes();
                    m.extend_from_slice(name);
                    m.extend_from_slice(format!(", have {}", usertype2s(have)).as_bytes());
                    self.scan.warning(&m);
                }
                return false;
            }
        }
        true
    }

    /// `_nc_parse_entry (entryp, literal, silent)`.
    ///
    /// # Errors
    ///
    /// An error that ends the compile, its message written.
    #[allow(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        reason = "upstream's _nc_parse_entry, in one piece so it reads against it"
    )]
    pub fn parse_entry(
        &mut self,
        ep: &mut Entry,
        literal: bool,
        silent: bool,
    ) -> Result<Parsed, Abort> {
        let mut token_type = self.scan.get_token(silent)?;
        if token_type == TokenType::Eof {
            return Ok(Parsed::Eof);
        }
        if token_type != TokenType::Names {
            return Err(self
                .scan
                .err_abort(b"Entry does not start with terminal names in column one"));
        }

        init_entry(&mut self.strbuf, ep);
        ep.cstart = self.scan.comment_start;
        ep.cend = self.scan.comment_end;
        ep.startline = self.scan.start_line;

        // "Strip off the 2-character termcap name, if present."
        let mut names = self.scan.curr_token.name.clone();
        if self.scan.syntax == SYN_TERMCAP && !self.user_definable {
            let ok_tc2 = |c: u8| isgraph(c) && c != b'|';
            if names.first().is_some_and(|&c| ok_tc2(c))
                && names.get(1).is_some_and(|&c| ok_tc2(c))
                && names.get(2) == Some(&b'|')
            {
                names.drain(..3);
            }
        }
        let Some(saved) = self.save(&names) else {
            return Ok(Parsed::Err);
        };
        ep.tterm.term_names = saved;

        // "the one-token lookahead in the parse loop results in the
        // terminal type getting prematurely set to correspond to that of the
        // next entry"
        let mut name = first_name(&ep.tterm.term_names);
        if !valid_entryname(&name) {
            let mut m = b"invalid entry name \"".to_vec();
            m.extend_from_slice(&name);
            m.push(b'"');
            self.scan.warning(&m);
            name = b"invalid".to_vec();
        }
        self.scan.set_type(&name);

        // "check for overly-long names and aliases"
        let field = ep.tterm.term_names.clone();
        let mut base = 0usize;
        while let Some(bar) = field
            .get(base..)
            .and_then(|r| r.iter().position(|&c| c == b'|'))
        {
            if bar > MAX_ALIAS {
                let mut m = if base == 0 {
                    b"primary name `".to_vec()
                } else {
                    b"alias `".to_vec()
                };
                m.extend_from_slice(
                    field
                        .get(base..base.saturating_add(bar))
                        .unwrap_or_default(),
                );
                m.extend_from_slice(b"' may be too long");
                self.scan.warning(&m);
            }
            base = base.saturating_add(bar).saturating_add(1);
        }

        ep.nuses = 0;
        let mut bad_tc_usage = false;
        let bad_tc = |scan: &mut Scanner<'_>, bad: &mut bool| {
            if !*bad {
                *bad = true;
                scan.warning(b"Legacy termcap allows only a trailing tc= clause");
            }
        };

        loop {
            token_type = self.scan.get_token(silent)?;
            if token_type == TokenType::Eof || token_type == TokenType::Names {
                break;
            }
            let tk_name = self.scan.curr_token.name.clone();
            let valstring = self.scan.curr_token.valstring.clone();
            let is_use = tk_name == b"use";
            let is_tc = !is_use && tk_name == b"tc";
            if is_use || is_tc {
                let value = valstring.as_deref().map(cstr);
                match value {
                    None | Some([]) => {
                        self.scan.warning(b"missing name for use-clause");
                        continue;
                    }
                    Some(v) if !valid_entryname(v) => {
                        let mut m = b"invalid name for use-clause \"".to_vec();
                        m.extend_from_slice(v);
                        m.push(b'"');
                        self.scan.warning(&m);
                        continue;
                    }
                    Some(v) if ep.nuses >= MAX_USES => {
                        let mut m = b"too many use-clauses, ignored \"".to_vec();
                        m.extend_from_slice(v);
                        m.push(b'"');
                        self.scan.warning(&m);
                        continue;
                    }
                    Some(v) => {
                        if let Some(saved) = self.save(v) {
                            let line = i64::from(self.scan.curr_line);
                            let i = ep.nuses;
                            *ep.use_mut(i) = Use {
                                name: Some(saved),
                                link: None,
                                line,
                            };
                            ep.nuses = ep.nuses.saturating_add(1);
                            if ep.nuses > 1 && is_tc {
                                bad_tc(&mut self.scan, &mut bad_tc_usage);
                            }
                        }
                    }
                }
                continue;
            }

            // normal token lookup
            let termcap = self.scan.syntax == SYN_TERMCAP;
            let mut entry_ptr: Option<Found> =
                tables::find_entry(&tk_name, termcap).map(Found::from);

            // "Our kluge to handle aliasing."
            if entry_ptr.is_none() {
                if termcap && ep.nuses != 0 {
                    bad_tc(&mut self.scan, &mut bad_tc_usage);
                }
                let what = if termcap { "termcap" } else { "terminfo" };
                let mut ignored = false;
                if let Some(ap) = tables::aliases(termcap)
                    .iter()
                    .find(|a| a.from.as_bytes() == tk_name.as_slice())
                {
                    match ap.to {
                        None => {
                            let m = format!("{} ({} {what} extension) ignored", ap.from, ap.source);
                            self.scan.warning(m.as_bytes());
                            ignored = true;
                        }
                        Some(to) => {
                            entry_ptr = tables::find_entry(to.as_bytes(), termcap).map(Found::from);
                            if entry_ptr.is_some() && !silent {
                                let m = format!(
                                    "{} ({} {what} extension) aliased to {to}",
                                    ap.from, ap.source
                                );
                                self.scan.warning(m.as_bytes());
                            }
                        }
                    }
                }
                if ignored {
                    continue;
                }
                if !termcap && entry_ptr.is_none() {
                    entry_ptr = tables::lookup_fullname(&tk_name).map(Found::from);
                }
            }

            // "If we have extended-names active, we will automatically define
            // a name based on its context."
            if entry_ptr.is_none() && self.user_definable {
                if self.expected_type(&tk_name, token_type, silent) {
                    entry_ptr = self.extend_names(&mut ep.tterm, &tk_name, token_type);
                    if entry_ptr.is_some() && self.tracing >= DEBUG_LEVEL_1 {
                        let mut m = b"extended capability '".to_vec();
                        m.extend_from_slice(&tk_name);
                        m.push(b'\'');
                        self.scan.warning(&m);
                    }
                } else {
                    // "ignore it: we have already printed error message"
                    continue;
                }
            }

            // "can't find this cap name, not even as an alias"
            let Some(mut found) = entry_ptr else {
                if !silent {
                    let mut m = b"unknown capability '".to_vec();
                    m.extend_from_slice(&tk_name);
                    m.push(b'\'');
                    self.scan.warning(&m);
                }
                continue;
            };

            // "deal with bad type/value combinations."
            if token_type == TokenType::Cancel {
                // "Prefer terminfo in this (long-obsolete) ambiguity"
                if tk_name == b"ma"
                    && let Some(c) = tables::find_type_entry(b"ma", Kind::Number, termcap)
                {
                    found = Found::from(c);
                }
            } else if Some(found.kind) != kind_of(token_type) {
                if token_type == TokenType::Number && tk_name == b"ma" {
                    // "tell max_attributes from arrow_key_map"
                    if let Some(c) = tables::find_type_entry(b"ma", Kind::Number, termcap) {
                        found = Found::from(c);
                    }
                } else if token_type == TokenType::String && tk_name == b"MT" {
                    // "map terminfo's string MT to MT"
                    if let Some(c) = tables::find_type_entry(b"MT", Kind::String, termcap) {
                        found = Found::from(c);
                    }
                } else if token_type == TokenType::Boolean && found.kind == Kind::String {
                    // "treat strings without following "=" as empty strings"
                    token_type = TokenType::String;
                } else {
                    // "we couldn't recover; skip this token"
                    if !silent {
                        let type_name = match found.kind {
                            Kind::Boolean => "boolean",
                            Kind::String => "string",
                            Kind::Number => "numeric",
                        };
                        let mut m =
                            format!("wrong type used for {type_name} capability '").into_bytes();
                        m.extend_from_slice(&tk_name);
                        m.push(b'\'');
                        self.scan.warning(&m);
                    }
                    continue;
                }
            }

            // "now we know that the type/value combination is OK"
            let i = found.index;
            match token_type {
                TokenType::Cancel => match found.kind {
                    Kind::Boolean => {
                        if let Some(v) = ep.tterm.booleans.get_mut(i) {
                            *v = CANCELLED_BOOLEAN;
                        }
                    }
                    Kind::Number => {
                        if let Some(v) = ep.tterm.numbers.get_mut(i) {
                            *v = CANCELLED_NUMERIC;
                        }
                    }
                    Kind::String => set_str(&mut ep.tterm, i, Str::Cancelled),
                },
                TokenType::Boolean => {
                    if let Some(v) = ep.tterm.booleans.get_mut(i) {
                        *v = 1;
                    }
                }
                TokenType::Number => {
                    let value = self.scan.curr_token.valnumber;
                    if let Some(v) = ep.tterm.numbers.get_mut(i) {
                        *v = value;
                    }
                }
                TokenType::String => {
                    let raw = valstring.unwrap_or_default();
                    let value = if termcap {
                        let p = PARAMETRIZED.get(i).copied().map_or(0, i32::from);
                        captoinfo(&mut self.scan, &tk_name, &raw, p)
                    } else {
                        cstr(&raw).to_vec()
                    };
                    let saved = self.save_value(&value);
                    set_str(&mut ep.tterm, i, saved);
                }
                _ => {
                    if !silent {
                        self.scan.warning(b"unknown token type");
                    }
                    self.scan.panic_mode(if termcap { b':' } else { b',' })?;
                }
            }
        }

        self.scan.push_token(token_type);
        let first = first_name(&ep.tterm.term_names);
        self.scan.set_type(&first);

        // "Try to deduce as much as possible from extension capabilities
        // (this includes obsolete BSD capabilities)."
        if !literal {
            if self.scan.syntax == SYN_TERMCAP {
                let has_base_entry = ep.tterm.term_names.contains(&b'+')
                    || ep
                        .uses
                        .iter()
                        .take(ep.nuses)
                        .any(|u| u.name.as_ref().is_some_and(|nm| !nm.contains(&b'+')));
                self.postprocess_termcap(&mut ep.tterm, has_base_entry)?;
            } else {
                self.postprocess_terminfo(&mut ep.tterm);
            }
        }
        wrap_entry(&mut self.strbuf, &mut self.scan, ep, false)?;
        Ok(Parsed::Ok)
    }

    /// `postprocess_termcap (tp, has_base)`: termcap's defaults, and its
    /// obsolete capabilities made terminfo's.
    #[allow(
        clippy::too_many_lines,
        reason = "upstream's postprocess_termcap, in one piece so it reads against it"
    )]
    fn postprocess_termcap(&mut self, tp: &mut TermType, has_base: bool) -> Result<(), Abort> {
        const C_CR: &[u8] = b"\r";
        const C_LF: &[u8] = b"\n";
        const C_BS: &[u8] = b"\x08";
        const C_HT: &[u8] = b"\t";

        // "if there was a tc entry, assume we picked up defaults via that"
        if !has_base {
            if str_at(tp, s::INIT_3STRING).wanted()
                && let Some(v) = str_at(tp, s::TERMCAP_INIT2).valid().map(<[u8]>::to_vec)
            {
                let saved = self.save_value(&v);
                set_str(tp, s::INIT_3STRING, saved);
            }
            if str_at(tp, s::RESET_2STRING).wanted()
                && let Some(v) = str_at(tp, s::TERMCAP_RESET).valid().map(<[u8]>::to_vec)
            {
                let saved = self.save_value(&v);
                set_str(tp, s::RESET_2STRING, saved);
            }
            if str_at(tp, s::CARRIAGE_RETURN).wanted() {
                let delay = num_at(tp, n::CARRIAGE_RETURN_DELAY);
                let v = if delay > 0 {
                    delayed(C_CR, delay)
                } else {
                    C_CR.to_vec()
                };
                let saved = self.save_value(&v);
                set_str(tp, s::CARRIAGE_RETURN, saved);
            }
            if str_at(tp, s::CURSOR_LEFT).wanted() {
                let delay = num_at(tp, n::BACKSPACE_DELAY);
                if delay > 0 {
                    let saved = self.save_value(&delayed(C_BS, delay));
                    set_str(tp, s::CURSOR_LEFT, saved);
                } else if bool_at(tp, b::BACKSPACES_WITH_BS) == 1 {
                    let saved = self.save_value(C_BS);
                    set_str(tp, s::CURSOR_LEFT, saved);
                } else if str_at(tp, s::BACKSPACE_IF_NOT_BS).present() {
                    let v = str_at(tp, s::BACKSPACE_IF_NOT_BS).clone();
                    set_str(tp, s::CURSOR_LEFT, v);
                }
            }
            // "vi doesn't use "do", but it does seem to use nl (or '\n')
            // instead"
            if str_at(tp, s::CURSOR_DOWN).wanted() {
                if str_at(tp, s::LINEFEED_IF_NOT_LF).present() {
                    let v = str_at(tp, s::LINEFEED_IF_NOT_LF).clone();
                    set_str(tp, s::CURSOR_DOWN, v);
                } else if bool_at(tp, b::LINEFEED_IS_NEWLINE) != 1 {
                    let delay = num_at(tp, n::NEW_LINE_DELAY);
                    let v = if delay > 0 {
                        delayed(C_LF, delay)
                    } else {
                        C_LF.to_vec()
                    };
                    let saved = self.save_value(&v);
                    set_str(tp, s::CURSOR_DOWN, saved);
                }
            }
            if str_at(tp, s::SCROLL_FORWARD).wanted() && bool_at(tp, b::CRT_NO_SCROLLING) != 1 {
                if str_at(tp, s::LINEFEED_IF_NOT_LF).present() {
                    // Upstream sets cursor_down here, not scroll_forward.
                    let v = str_at(tp, s::LINEFEED_IF_NOT_LF).clone();
                    set_str(tp, s::CURSOR_DOWN, v);
                } else if bool_at(tp, b::LINEFEED_IS_NEWLINE) != 1 {
                    let delay = num_at(tp, n::NEW_LINE_DELAY);
                    let v = if delay > 0 {
                        delayed(C_LF, delay)
                    } else {
                        C_LF.to_vec()
                    };
                    let saved = self.save_value(&v);
                    set_str(tp, s::SCROLL_FORWARD, saved);
                }
            }
            if str_at(tp, s::NEWLINE).wanted() {
                if bool_at(tp, b::LINEFEED_IS_NEWLINE) == 1 {
                    let delay = num_at(tp, n::NEW_LINE_DELAY);
                    let v = if delay > 0 {
                        delayed(C_LF, delay)
                    } else {
                        C_LF.to_vec()
                    };
                    let saved = self.save_value(&v);
                    set_str(tp, s::NEWLINE, saved);
                } else {
                    let cr = str_at(tp, s::CARRIAGE_RETURN).clone();
                    let second = if cr.present() && str_at(tp, s::SCROLL_FORWARD).present() {
                        Some(str_at(tp, s::SCROLL_FORWARD).clone())
                    } else if cr.present() && str_at(tp, s::CURSOR_DOWN).present() {
                        Some(str_at(tp, s::CURSOR_DOWN).clone())
                    } else {
                        None
                    };
                    if let Some(second) = second {
                        let mut result = StrDesc::new(MAX_LINE.saturating_mul(2).saturating_add(2));
                        if result.cat(cr.valid()) && result.cat(second.valid()) {
                            let saved = self.save_value(&result.buf);
                            set_str(tp, s::NEWLINE, saved);
                        }
                    }
                }
            }
        }

        // "TERMCAP-TO TERMINFO MAPPINGS FOR SOURCE TRANSLATION"
        if !has_base {
            if bool_at(tp, b::RETURN_DOES_CLR_EOL) == 1
                || bool_at(tp, b::NO_CORRECTLY_WORKING_CR) == 1
            {
                set_str(tp, s::CARRIAGE_RETURN, Str::Absent);
            }
            if str_at(tp, s::TAB).wanted() {
                let delay = num_at(tp, n::HORIZONTAL_TAB_DELAY);
                let v = if delay > 0 {
                    delayed(C_HT, delay)
                } else {
                    C_HT.to_vec()
                };
                let saved = self.save_value(&v);
                set_str(tp, s::TAB, saved);
            }
            if num_at(tp, n::INIT_TABS) == ABSENT_NUMERIC && bool_at(tp, b::HAS_HARDWARE_TABS) == 1
            {
                if let Some(v) = tp.numbers.get_mut(n::INIT_TABS) {
                    *v = 8;
                }
            }
            // "Assume we can beep with ^G unless we're given bl@."
            if str_at(tp, s::BELL).wanted() {
                let saved = self.save_value(b"\x07");
                set_str(tp, s::BELL, saved);
            }
        }

        // "Translate the old termcap :pt: capability to it#8 + ht=\t"
        if bool_at(tp, b::HAS_HARDWARE_TABS) == 1 {
            let it = num_at(tp, n::INIT_TABS);
            if it != 8 && it != ABSENT_NUMERIC {
                let m = format!("hardware tabs with a width other than 8: {it}");
                self.scan.warning(m.as_bytes());
            } else {
                let tab = str_at(tp, s::TAB).clone();
                if tab.present() && capcmp(tab.valid(), Some(C_HT)) != 0 {
                    let mut m = b"hardware tabs with a non-^I tab string ".to_vec();
                    m.extend_from_slice(&visbuf(&tab));
                    self.scan.warning(&m);
                } else {
                    if tab.wanted() {
                        let saved = self.save_value(C_HT);
                        set_str(tp, s::TAB, saved);
                    }
                    if let Some(v) = tp.numbers.get_mut(n::INIT_TABS) {
                        *v = 8;
                    }
                }
            }
        }

        // "Now translate the ko capability, if there is one."
        if let Some(ko) = str_at(tp, s::OTHER_NON_FUNCTION_KEYS)
            .valid()
            .map(<[u8]>::to_vec)
        {
            let ko = cstr(&ko).to_vec();
            // "we're going to use this for a special case later"
            let foundim = ko
                .iter()
                .position(|&c| c == b'i')
                .is_some_and(|at| ko.get(at.saturating_add(1)) == Some(&b'm'));
            let mut base = 0usize;
            // Each name followed by a comma: the last, with none after it,
            // is not looked at.
            while let Some(len) = ko
                .get(base..)
                .and_then(|r| r.iter().position(|&c| c == b','))
            {
                let item = ko
                    .get(base..base.saturating_add(len))
                    .unwrap_or_default()
                    .to_vec();
                base = base.saturating_add(len).saturating_add(1);
                let Some(&(from, to)) = KO_XLATE
                    .iter()
                    .find(|(f, _)| f.as_bytes() == item.as_slice())
                else {
                    let mut m = b"unknown capability `".to_vec();
                    m.extend_from_slice(&item);
                    m.extend_from_slice(b"' in ko string");
                    self.scan.warning(&m);
                    continue;
                };
                if to.is_empty() {
                    // "ignore it"
                    continue;
                }
                // "now we know we found a match in ko_table, so..."
                let from_ptr = tables::find_entry(from.as_bytes(), true);
                let to_ptr = tables::find_entry(to.as_bytes(), false);
                let (Some(from_ptr), Some(to_ptr)) = (from_ptr, to_ptr) else {
                    return Err(self
                        .scan
                        .err_abort(b"ko translation table is invalid, I give up"));
                };
                let from_value = str_at(tp, from_ptr.index).clone();
                if from_value.wanted() {
                    let m = format!("no value for ko capability {from}");
                    self.scan.warning(m.as_bytes());
                    continue;
                }
                let to_value = str_at(tp, to_ptr.index).clone();
                if to_value != Str::Absent {
                    // "There's no point in warning about it if it is the same
                    // string; that's just an inefficiency."
                    if let (Some(sv), Some(tv)) = (from_value.valid(), to_value.valid())
                        && cstr(sv) != cstr(tv)
                    {
                        let mut m =
                            format!("{to} ({from}) already has an explicit value ").into_bytes();
                        m.extend_from_slice(cstr(tv));
                        m.extend_from_slice(b", ignoring ko");
                        self.scan.warning(&m);
                    }
                    continue;
                }
                // "The magic moment -- copy the mapped key string over,
                // stripping out padding."
                match from_value.valid().map(cstr) {
                    Some(bp) => {
                        let mut buf2 = Vec::new();
                        let mut k = 0usize;
                        while let Some(&c) = bp.get(k) {
                            if c == b'$' && bp.get(k.saturating_add(1)) == Some(&b'<') {
                                while bp.get(k).is_some_and(|&c| c != b'>') {
                                    k = k.saturating_add(1);
                                }
                            } else {
                                buf2.push(c);
                            }
                            k = k.saturating_add(1);
                        }
                        let saved = self.save_value(&buf2);
                        set_str(tp, to_ptr.index, saved);
                    }
                    None => set_str(tp, to_ptr.index, from_value),
                }
            }
            // "ko=im and ko=ic both want to grab the `Insert' keycap."
            if foundim && str_at(tp, s::KEY_IC).wanted() && str_at(tp, s::KEY_SIC).present() {
                let v = str_at(tp, s::KEY_SIC).clone();
                set_str(tp, s::KEY_IC, v);
                set_str(tp, s::KEY_SIC, Str::Absent);
            }
        }

        if !has_base && bool_at(tp, b::HARD_COPY) == 0 {
            if str_at(tp, s::KEY_BACKSPACE).wanted() {
                let saved = self.save_value(C_BS);
                set_str(tp, s::KEY_BACKSPACE, saved);
            }
            if str_at(tp, s::KEY_LEFT).wanted() {
                let saved = self.save_value(C_BS);
                set_str(tp, s::KEY_LEFT, saved);
            }
            if str_at(tp, s::KEY_DOWN).wanted() {
                let saved = self.save_value(C_LF);
                set_str(tp, s::KEY_DOWN, saved);
            }
        }

        // "Translate XENIX forms characters."
        let xenix = [
            s::ACS_ULCORNER,
            s::ACS_LLCORNER,
            s::ACS_URCORNER,
            s::ACS_LRCORNER,
            s::ACS_LTEE,
            s::ACS_RTEE,
            s::ACS_BTEE,
            s::ACS_TTEE,
            s::ACS_HLINE,
            s::ACS_VLINE,
            s::ACS_PLUS,
        ];
        if xenix.iter().any(|&i| str_at(tp, i).present()) {
            let mut result = StrDesc::new(MAX_TERMCAP_LENGTH);
            result.cat(str_at(tp, s::ACS_CHARS).valid());
            for (code, i) in [
                (b'j', s::ACS_LRCORNER),
                (b'k', s::ACS_URCORNER),
                (b'l', s::ACS_ULCORNER),
                (b'm', s::ACS_LLCORNER),
                (b'n', s::ACS_PLUS),
                (b'q', s::ACS_HLINE),
                (b't', s::ACS_LTEE),
                (b'u', s::ACS_RTEE),
                (b'v', s::ACS_BTEE),
                (b'w', s::ACS_TTEE),
                (b'x', s::ACS_VLINE),
            ] {
                let src = str_at(tp, i).clone();
                append_acs(&mut result, code, &src);
            }
            if !result.buf.is_empty() {
                let saved = self.save_value(&result.buf);
                set_str(tp, s::ACS_CHARS, saved);
                self.scan
                    .warning(b"acsc string synthesized from XENIX capabilities");
            }
        } else if str_at(tp, s::ACS_CHARS).wanted()
            && str_at(tp, s::ENTER_ALT_CHARSET_MODE).present()
            && str_at(tp, s::EXIT_ALT_CHARSET_MODE).present()
        {
            let saved = self.save_value(VT_ACSC);
            set_str(tp, s::ACS_CHARS, saved);
        }
        Ok(())
    }

    /// `postprocess_terminfo (tp)`: AIX's box characters made `acsc`.
    fn postprocess_terminfo(&mut self, tp: &mut TermType) {
        let box1 = str_at(tp, s::BOX_CHARS_1).clone();
        if box1.present() {
            let mut result = StrDesc::new(MAX_TERMCAP_LENGTH);
            result.cat(str_at(tp, s::ACS_CHARS).valid());
            for (off, code) in [
                (0, b'l'),
                (1, b'q'),
                (2, b'k'),
                (3, b'x'),
                (4, b'j'),
                (5, b'm'),
                (6, b'w'),
                (7, b'u'),
                (8, b'v'),
                (9, b't'),
                (10, b'n'),
            ] {
                append_acs0(&mut result, code, &box1, off);
            }
            if !result.buf.is_empty() {
                let saved = self.save_value(&result.buf);
                set_str(tp, s::ACS_CHARS, saved);
                self.scan
                    .warning(b"acsc string synthesized from AIX capabilities");
                set_str(tp, s::BOX_CHARS_1, Str::Absent);
            }
        }
    }
}

/// The capability type a token gives a value of.
fn kind_of(t: TokenType) -> Option<Kind> {
    match t {
        TokenType::Boolean => Some(Kind::Boolean),
        TokenType::Number => Some(Kind::Number),
        TokenType::String => Some(Kind::String),
        _ => None,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    /// Every entry of `source`, parsed as `_nc_read_entry_source` would
    /// (but without its checks), and what was said.
    fn parse(source: &[u8], user_definable: bool) -> (Vec<Entry>, String) {
        let mut diag = Vec::new();
        let mut out = Vec::new();
        {
            let mut c = Compiler::new(&mut diag);
            c.user_definable = user_definable;
            c.scan.set_source(Some(b"t.src"));
            c.scan
                .reset_input_file(Box::new(std::io::Cursor::new(source.to_vec())), Some(0));
            loop {
                let mut e = Entry::default();
                match c.parse_entry(&mut e, false, false).unwrap() {
                    Parsed::Ok => out.push(e),
                    Parsed::Eof | Parsed::Err => break,
                }
            }
        }
        (out, String::from_utf8(diag).unwrap())
    }

    #[test]
    fn a_terminfo_entry_parses_to_its_values() {
        let (e, diag) = parse(
            b"x|xx|test terminal,\n\tam, cols#80, bel=^G, kf1@, use=vt100,\n",
            false,
        );
        assert_eq!(diag, "");
        assert_eq!(e.len(), 1);
        let t = &e[0].tterm;
        assert_eq!(t.term_names, b"x|xx|test terminal");
        assert_eq!(t.booleans[1], 1);
        assert_eq!(t.numbers[0], 80);
        assert_eq!(t.strings[s::BELL], Str::Value(b"\x07".to_vec()));
        assert_eq!(e[0].nuses, 1);
        assert_eq!(e[0].uses[0].name.as_deref(), Some(&b"vt100"[..]));
    }

    #[test]
    fn an_unknown_name_is_warned_of_or_made_extended() {
        let (_, diag) = parse(b"x|xx|test terminal,\n\tXT, U8#1,\n", false);
        assert_eq!(
            diag,
            // Measured against the reference: the column is where the
            // comma after each was read.
            "\"t.src\", line 2, col 11, terminal 'x': unknown capability 'XT'\n\
             \"t.src\", line 2, col 17, terminal 'x': unknown capability 'U8'\n"
        );
        let (e, diag) = parse(b"x|xx|test terminal,\n\tXT, U8#1, Ms=a,\n", true);
        assert_eq!(diag, "");
        let t = &e[0].tterm;
        let names: Vec<&[u8]> = (0..t.num_ext_names()).map(|i| t.ext_name(i)).collect();
        assert_eq!(names, [&b"XT"[..], b"U8", b"Ms"]);
        assert_eq!(t.booleans[BOOLCOUNT], 1);
        assert_eq!(t.numbers[NUMCOUNT], 1);
        assert_eq!(t.strings[STRCOUNT], Str::Value(b"a".to_vec()));
    }

    #[test]
    fn termcap_gets_its_defaults() {
        let (e, _) = parse(b"xx|x|test terminal:\\\n\t:co#80:bs:pt:\n", false);
        let t = &e[0].tterm;
        assert_eq!(t.term_names, b"x|test terminal");
        assert_eq!(t.strings[s::CARRIAGE_RETURN], Str::Value(b"\r".to_vec()));
        assert_eq!(t.strings[s::CURSOR_LEFT], Str::Value(b"\x08".to_vec()));
        assert_eq!(t.strings[s::BELL], Str::Value(b"\x07".to_vec()));
        assert_eq!(t.strings[s::TAB], Str::Value(b"\t".to_vec()));
        assert_eq!(t.numbers[n::INIT_TABS], 8);
    }

    #[test]
    fn entry_names_are_checked() {
        assert!(valid_entryname(b"xterm-256color"));
        assert!(valid_entryname(b"#x"));
        assert!(!valid_entryname(b"x#"));
        assert!(!valid_entryname(b"a b"));
        assert!(!valid_entryname(b"a/b"));
    }
}
