//! `tic.c`'s `check_termtype`: what `tic -v` checks of each entry once its
//! `use=` are merged -- key definitions that collide, parameter counts that
//! do not match the capability's, delays where they make no sense, `acsc`,
//! colors, cursor movement, the keypad, paired capabilities, and `sgr`
//! against the single-attribute strings -- before `comp_parse.c`'s own
//! `sanity_check2`.
//!
//! Upstream runs the checks with the entry set up as the current terminal
//! (`_nc_resolve_uses2` makes a fake one of it), so that `tparm`,
//! `tigetflag` and `tigetstr` see it; here that is an [`Entry`] made of it,
//! with its own [`Tparm`] state, fresh for each entry as the fake one is.
//! Capabilities are named by their C variable names, as the warnings name
//! them.
//!
//! Four places differ, each where upstream fails outright -- crashes, or
//! never finishes -- and each reached by `scripts/tic-diff.sh`:
//!
//! - An `sgr` that `tparm` refuses (one that takes no parameters) makes
//!   upstream `strdup` a null pointer and crash. Here the check goes on as
//!   upstream's code means it to: "sgr(0) did not return a value".
//! - `-C`'s termcap check expands each capability as `tparm` would be asked
//!   to, and where `tparm` refuses -- a capability upstream's tables say
//!   takes parameters but that has none (`u7=\E[6n`), or one with a string
//!   parameter `tparm` will not take for it -- upstream `strdup`s the null
//!   pointer too: 834 of the 1828 entries of its own database, each
//!   compiled alone, crash `tic -C -r -v`. Here that expansion is empty.
//! - A delay with no closing `>` (`$<5x>`) stops upstream's `parse_ti_delay`
//!   from moving, and `tic -C -r -v` loops forever. Here the scan moves on
//!   past the `$`.
//! - Inside a delay, two strings that differ at bytes neither delay scan
//!   moves past -- `$<5>` against `$<*5>` -- leave upstream's `similar_sgr`
//!   comparing the same two bytes forever. Here the comparison goes on as
//!   it does outside a delay.

use std::collections::HashMap;
use std::sync::OnceLock;

use terminfo::compile::captoinfo::infotocap;
use terminfo::compile::comp_parse::fixup_acsc;
use terminfo::compile::dump::has_params;
use terminfo::compile::entry::{capcmp, visbuf};
use terminfo::compile::expand::tic_expand;
use terminfo::compile::parse::CheckTermtype;
use terminfo::compile::scan::{SYN_TERMCAP, SYN_TERMINFO, Scanner, cstr, isspace};
use terminfo::compile::tables::{self, first_name};
use terminfo::termtype::{Str, TermType};
use terminfo::{
    Arg, BOOLCOUNT, Entry, Kind, NUM_PARM, NUMCOUNT, STRCOUNT, TiString, Tparm, analyze, captab,
    names, trim_sgr0,
};

/// The capabilities by their C variable names: each one's index among its
/// type's.
fn index() -> &'static HashMap<&'static str, usize> {
    static INDEX: OnceLock<HashMap<&'static str, usize>> = OnceLock::new();
    INDEX.get_or_init(|| {
        let mut m = HashMap::new();
        for list in [
            names::BOOLFNAMES.as_slice(),
            names::NUMFNAMES.as_slice(),
            names::STRFNAMES.as_slice(),
        ] {
            for (i, n) in list.iter().enumerate() {
                m.insert(*n, i);
            }
        }
        m
    })
}

/// The index of the capability whose C variable is `name`, among its
/// type's -- which the caller knows.
fn idx(name: &str) -> usize {
    let i = index().get(name).copied();
    debug_assert!(i.is_some(), "no capability is called {name}");
    i.unwrap_or(usize::MAX)
}

/// `TParams`: how `tparm` is to be called for a capability.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum TParams {
    Other,
    Numbers,
    NumStr,
    NumStrStr,
    Str,
    StrStr,
}

/// `tparm_type (name)`: the call the capability `name` is known to want.
fn tparm_type(name: &[u8]) -> TParams {
    match name {
        b"pkey_key" | b"pfkey" | b"pk" | b"pkey_local" | b"pfloc" | b"pl" | b"pkey_xmit"
        | b"pfx" | b"px" | b"plab_norm" | b"pln" | b"pn" => TParams::NumStr,
        b"pkey_plab" | b"pfxl" | b"xl" => TParams::NumStrStr,
        b"Cs" => TParams::Str,
        b"Ms" => TParams::StrStr,
        _ => TParams::Numbers,
    }
}

/// `guess_tparm_type (nparam, p_is_s)`: the call a capability's analysis
/// suggests.
fn guess_tparm_type(nparam: usize, p_is_s: &[bool; NUM_PARM]) -> TParams {
    let s = |i: usize| p_is_s.get(i).copied().unwrap_or(false);
    let mut result = TParams::Other;
    match nparam {
        0 | 1 => {
            result = if s(0) { TParams::Str } else { TParams::Numbers };
        }
        2 => {
            if !s(0) && !s(1) {
                result = TParams::Numbers;
            }
            if !s(0) && s(1) {
                result = TParams::NumStr;
            }
            if s(0) && s(1) {
                result = TParams::StrStr;
            }
        }
        3 => {
            if !s(0) && !s(1) && !s(2) {
                result = TParams::Numbers;
            }
            if !s(0) && s(1) && s(2) {
                result = TParams::NumStrStr;
            }
        }
        _ => {}
    }
    result
}

/// `sgr_names`: `sgr`'s parameters, 1 to 9, after "none".
const SGR_NAMES: [&str; 10] = [
    "none",
    "standout",
    "underline",
    "reverse",
    "blink",
    "dim",
    "bold",
    "invis",
    "protect",
    "altcharset",
];

/// `tic`'s checks, and the options they depend on.
pub struct Checker {
    /// `debug_level`: `-v`'s level.
    debug_level: u32,
    /// `capdump`: `-C`, which adds the termcap translation's checks.
    capdump: bool,
    /// `using_extensions`: `-x`.
    using_extensions: bool,
    /// `_nc_user_definable`.
    user_definable: bool,
}

impl Checker {
    /// The checks as `tic` runs them with these options.
    pub fn new(
        debug_level: u32,
        capdump: bool,
        using_extensions: bool,
        user_definable: bool,
    ) -> Self {
        Self {
            debug_level,
            capdump,
            using_extensions,
            user_definable,
        }
    }
}

impl CheckTermtype for Checker {
    fn check(&mut self, scan: &mut Scanner<'_>, tp: &mut TermType, literal: bool) {
        // `fake_tm`: the entry, as the terminal `tparm` and `tigetstr` see.
        let term = Entry::from_termtype(tp.clone());
        let mut cx = Cx {
            scan,
            tp,
            term,
            tparm: Tparm::new(),
            ck: self,
        };
        check_termtype(&mut cx);
        // "Finally, do the non-verbose checks": `save_check_termtype`.
        sanity_check2(&mut cx, literal);
    }
}

/// One entry being checked: the description, the terminal it is set up
/// as, and that terminal's `tparm` state.
struct Cx<'s, 'd> {
    scan: &'s mut Scanner<'d>,
    tp: &'s mut TermType,
    term: Entry,
    tparm: Tparm,
    ck: &'s Checker,
}

impl Cx<'_, '_> {
    /// `_nc_warning`.
    fn warn(&mut self, m: impl AsRef<[u8]>) {
        self.scan.warning(m.as_ref());
    }

    /// A string capability by its variable name.
    fn s(&self, name: &str) -> Str {
        self.tp.strings.get(idx(name)).cloned().unwrap_or_default()
    }

    /// The value of a string capability, if `VALID_STRING`.
    fn v(&self, name: &str) -> Option<Vec<u8>> {
        self.tp
            .strings
            .get(idx(name))
            .and_then(Str::valid)
            .map(|v| cstr(v).to_vec())
    }

    /// `PRESENT (name)`.
    fn present(&self, name: &str) -> bool {
        self.tp.strings.get(idx(name)).is_some_and(Str::present)
    }

    /// A number by its variable name.
    fn num(&self, name: &str) -> i32 {
        self.tp.numbers.get(idx(name)).copied().unwrap_or(-1)
    }

    /// A boolean by its variable name, as stored: 1, 0, or cancelled.
    fn flag(&self, name: &str) -> i8 {
        self.tp.booleans.get(idx(name)).copied().unwrap_or(0)
    }

    /// `PAIRED (p, q)`.
    fn paired(&mut self, p: &str, q: &str) {
        if self.present(q) && !self.present(p) {
            self.warn(format!("{q} but no {p}"));
        }
        if self.present(p) && !self.present(q) {
            self.warn(format!("{p} but no {q}"));
        }
    }

    /// `ANDMISSING (p, q)`.
    fn andmissing(&mut self, p: &str, q: &str) {
        if self.present(p) && !self.present(q) {
            self.warn(format!("{p} but no {q}"));
        }
    }

    /// `TIPARM_n (s, ...)`: `_nc_tiparm`.
    fn tiparm(&mut self, expected: usize, s: &[u8], params: &[i32]) -> Option<Vec<u8>> {
        self.tparm.nc_tiparm(&self.term, expected, s, params)
    }

    /// `ExtStrname (tp, j, strnames)`.
    fn str_name(&self, j: usize) -> Vec<u8> {
        match j.checked_sub(STRCOUNT) {
            Some(x) => self
                .tp
                .ext_name(
                    x.saturating_add(self.tp.ext_booleans)
                        .saturating_add(self.tp.ext_numbers),
                )
                .to_vec(),
            None => names::STRNAMES
                .get(j)
                .map(|s| s.as_bytes().to_vec())
                .unwrap_or_default(),
        }
    }

    /// Whether the terminal's primary name has a `+` in it: a building
    /// block, not a terminal.
    fn is_fragment(&self) -> bool {
        first_name(&self.tp.term_names).contains(&b'+')
    }
}

/// `check_termtype (tp, literal)`.
fn check_termtype(cx: &mut Cx<'_, '_>) {
    check_conflict(cx);

    for j in 0..cx.tp.strings.len() {
        let Some(a) = cx
            .tp
            .strings
            .get(j)
            .and_then(Str::valid)
            .map(|v| cstr(v).to_vec())
        else {
            continue;
        };
        let name = cx.str_name(j);
        // "If we expect parameters, or if there might be parameters, check
        // for consistent number of parameters."
        if j >= captab::PARAMETRIZED.len()
            || is_user_capability(cx, &name) >= 0
            || captab::PARAMETRIZED.get(j).is_some_and(|&p| p > 0)
        {
            check_params(cx, &name, &a, j >= STRCOUNT, j);
        }
        check_delays(cx, &name, &a);
        if cx.ck.capdump {
            check_infotocap(cx, j, &a);
        }
    }
    // "in extended mode, verify that each extension is expected type"
    for j in BOOLCOUNT..cx.tp.booleans.len() {
        let name = cx.tp.ext_name(j.saturating_sub(BOOLCOUNT)).to_vec();
        check_user_capability_type(cx, &name, Kind::Boolean);
    }
    for j in NUMCOUNT..cx.tp.numbers.len() {
        let name = cx
            .tp
            .ext_name(
                j.saturating_sub(NUMCOUNT)
                    .saturating_add(cx.tp.ext_booleans),
            )
            .to_vec();
        check_user_capability_type(cx, &name, Kind::Number);
    }
    for j in STRCOUNT..cx.tp.strings.len() {
        let name = cx.str_name(j);
        check_user_capability_type(cx, &name, Kind::String);
    }

    check_acs(cx);
    check_colors(cx);
    check_cursor(cx);
    check_keypad(cx);
    check_printer(cx);
    check_screen(cx);
    check_user_6789(cx);

    // "These are probably both or none."
    cx.paired("parm_index", "parm_rindex");
    cx.paired("parm_ich", "parm_dch");

    // "These may be mismatched because the terminal description relies on
    // restoring the cursor visibility by resetting it."
    cx.andmissing("cursor_invisible", "cursor_normal");
    cx.andmissing("cursor_visible", "cursor_normal");

    if cx.present("cursor_visible")
        && cx.present("cursor_normal")
        && capcmp(
            cx.v("cursor_visible").as_deref(),
            cx.v("cursor_normal").as_deref(),
        ) == 0
    {
        cx.warn("cursor_visible is same as cursor_normal");
    }

    // "From XSI & O'Reilly, we gather that sc/rc are required if csr is
    // given, because the cursor position after the scrolling operation is
    // performed is undefined."
    cx.andmissing("change_scroll_region", "save_cursor");
    cx.andmissing("change_scroll_region", "restore_cursor");

    // "If we can clear tabs, we should be able to initialize them."
    cx.andmissing("clear_all_tabs", "set_tab");

    check_sgr_all(cx);

    if cx.present("exit_attribute_mode") {
        let sgr0 = cx.v("exit_attribute_mode").unwrap_or_default();
        // `_nc_trim_sgr0` answers `exit_attribute_mode` itself where it
        // trims nothing.
        let check_sgr0 = trim_sgr0(&cx.term, &mut cx.tparm).unwrap_or_else(|| sgr0.clone());
        if check_sgr0.is_empty() {
            cx.warn("trimmed sgr0 is empty");
        }
        for name in [
            "exit_italics_mode",
            "exit_standout_mode",
            "exit_underline_mode",
        ] {
            check_exit_attribute(cx, name, &check_sgr0, &sgr0);
        }
    }
    for code in 0..SGR_NAMES.len() {
        for name in [
            "set_a_foreground",
            "set_a_background",
            "set_foreground",
            "set_background",
        ] {
            check_sgr_param(cx, code, name);
        }
    }

    // "Some standard applications (e.g., vi) and some non-curses
    // applications (e.g., jove) get confused if we have both ich1 and
    // smir/rmir.  Let's be nice and warn about that, too, even though
    // ncurses handles it."
    if (cx.present("enter_insert_mode") || cx.present("exit_insert_mode"))
        && cx.present("insert_character")
    {
        cx.warn("non-curses applications may be confused by ich1 with smir/rmir");
    }
}

/// `check_termtype`'s `sgr` checks: `sgr` with each attribute alone against
/// the capability for it.
fn check_sgr_all(cx: &mut Cx<'_, '_>) {
    if cx.present("set_attributes") {
        let zero = if cx.present("exit_attribute_mode") {
            check_sgr(cx, None, 0, "exit_attribute_mode")
        } else {
            let sgr = cx.v("set_attributes").unwrap_or_default();
            cx.tiparm(9, &sgr, &[0; 9])
        };
        check_tparm_err(cx, 0);
        match zero {
            Some(zero) => {
                for (num, name) in [
                    (1, "enter_standout_mode"),
                    (2, "enter_underline_mode"),
                    (3, "enter_reverse_mode"),
                    (4, "enter_blink_mode"),
                    (5, "enter_dim_mode"),
                    (6, "enter_bold_mode"),
                    (7, "enter_secure_mode"),
                    (8, "enter_protected_mode"),
                    (9, "enter_alt_charset_mode"),
                ] {
                    // `check_sgr`'s answer is only needed for `zero`.
                    let _ = check_sgr(cx, Some(&zero), num, name);
                }
            }
            // Upstream crashes here instead (see the module comment).
            None => cx.warn("sgr(0) did not return a value"),
        }
    } else if cx.present("exit_attribute_mode")
        && cx.s("set_attributes") != Str::Cancelled
        && cx.scan.syntax == SYN_TERMINFO
    {
        cx.warn("missing sgr string");
    }
}

/// `sanity_check2 (tp, literal)`: the checks `tic` makes without `-v`, and
/// `fixup_acsc`.
fn sanity_check2(cx: &mut Cx<'_, '_>, literal: bool) {
    if !cx.present("exit_attribute_mode") {
        cx.paired("enter_standout_mode", "exit_standout_mode");
        cx.paired("enter_underline_mode", "exit_underline_mode");
        cx.paired("enter_italics_mode", "exit_italics_mode");
    }

    // "we do this check/fix in postprocess_termcap(), but some packagers
    // prefer to bypass it..."
    if !literal {
        fixup_acsc(cx.tp, literal);
        cx.andmissing("enter_alt_charset_mode", "acs_chars");
        cx.andmissing("exit_alt_charset_mode", "acs_chars");
    }

    // "listed in structure-member order of first argument"
    cx.paired("enter_alt_charset_mode", "exit_alt_charset_mode");
    cx.andmissing("enter_blink_mode", "exit_attribute_mode");
    cx.andmissing("enter_bold_mode", "exit_attribute_mode");
    cx.paired("exit_ca_mode", "enter_ca_mode");
    cx.paired("enter_delete_mode", "exit_delete_mode");
    cx.andmissing("enter_dim_mode", "exit_attribute_mode");
    cx.paired("enter_insert_mode", "exit_insert_mode");
    cx.andmissing("enter_secure_mode", "exit_attribute_mode");
    cx.andmissing("enter_protected_mode", "exit_attribute_mode");
    cx.andmissing("enter_reverse_mode", "exit_attribute_mode");
    cx.paired("from_status_line", "to_status_line");
    cx.paired("meta_off", "meta_on");

    cx.paired("prtr_on", "prtr_off");
    cx.paired("save_cursor", "restore_cursor");
    cx.paired("enter_xon_mode", "exit_xon_mode");
    cx.paired("enter_am_mode", "exit_am_mode");
    cx.andmissing("label_off", "label_on");
    cx.paired("display_clock", "remove_clock");
    cx.andmissing("set_color_pair", "initialize_pair");
}

/// `check_acs (tp)`: whether the alternate character set's capabilities
/// agree.
fn check_acs(cx: &mut Cx<'_, '_>) {
    // "ena_acs is not always necessary, but if it is present, the
    // enter/exit capabilities should be."
    cx.andmissing("ena_acs", "enter_alt_charset_mode");
    cx.andmissing("ena_acs", "exit_alt_charset_mode");
    cx.paired("exit_alt_charset_mode", "exit_alt_charset_mode");

    // "vt100-like is frequently used, but perhaps ena_acs is missing, etc."
    let smacs = cx
        .v("enter_alt_charset_mode")
        .map_or(0, |v| match v.as_slice() {
            b"\x1b(0" => 2,
            b"\x0e" => 1,
            _ => 0,
        });
    let rmacs = cx
        .v("exit_alt_charset_mode")
        .map_or(0, |v| match v.as_slice() {
            b"\x1b(B" => 2,
            b"\x0f" => 1,
            _ => 0,
        });
    let enacs = cx
        .v("ena_acs")
        .map_or(0, |v| if v == b"\x1b(B\x1b)0" { 2 } else { 0 });
    if rmacs != 0 && smacs != 0 && rmacs != smacs {
        cx.warn("rmacs/smacs are inconsistent");
    }
    if rmacs == 2 && smacs == 2 && enacs != 0 {
        cx.warn("rmacs/smacs make enacs redundant");
    }
    if rmacs == 1 && smacs == 1 && enacs == 0 {
        cx.warn("VT100-style rmacs/smacs require enacs");
    }

    if let Some(acsc) = cx.v("acs_chars") {
        let boxes = b"lmkjtuvwqxn";
        let mut mapped = [0u8; 256];
        for pair in acsc.chunks(2) {
            if let [c0, c1] = pair {
                if let Some(slot) = mapped.get_mut(usize::from(*c0)) {
                    *slot = *c1;
                }
            } else {
                cx.warn("acsc has odd number of characters");
                break;
            }
        }
        let m = |c: u8| mapped.get(usize::from(c)).copied().unwrap_or(0);
        if m(b'I') != 0 && m(b'i') == 0 {
            cx.warn("acsc refers to 'I', which is probably an error");
        }
        let missing: Vec<u8> = boxes.iter().copied().filter(|&c| m(c) == 0).collect();
        if !missing.is_empty() && missing.as_slice() != boxes.as_slice() {
            cx.warn(
                [
                    b"acsc is missing some line-drawing mapping: ".as_slice(),
                    &missing,
                ]
                .concat(),
            );
        }
    }
}

/// `same_color (oldcap, newcap, limit)`: whether two color capabilities
/// give the same string for each of the first colors.
fn same_color(cx: &mut Cx<'_, '_>, oldcap: &[u8], newcap: &[u8], limit: i32) -> bool {
    let limit = limit.min(16);
    if limit < 8 {
        return false;
    }
    let mut same: i32 = 0;
    for n in 0..limit {
        let oldvalue = cx.tiparm(1, oldcap, &[n]).unwrap_or_default();
        let newvalue = cx.tiparm(1, newcap, &[n]).unwrap_or_default();
        if oldvalue == newvalue {
            same = same.saturating_add(1);
        }
    }
    same == limit
}

/// glibc's `%d`: white space skipped, a sign, decimal digits -- the value
/// `strtol` makes of them (saturated) stored in an `int` (its low 32 bits),
/// and where the digits end. `None` for no digits.
fn scan_int(s: &[u8], from: usize) -> Option<(i32, usize)> {
    let mut i = from;
    while s.get(i).is_some_and(|&c| isspace(c)) {
        i = i.saturating_add(1);
    }
    let negative = s.get(i) == Some(&b'-');
    if matches!(s.get(i), Some(b'+' | b'-')) {
        i = i.saturating_add(1);
    }
    let start = i;
    let mut value: i64 = 0;
    let mut overflow = false;
    while let Some(d) = s
        .get(i)
        .filter(|c| c.is_ascii_digit())
        .map(|c| i64::from(c.wrapping_sub(b'0')))
    {
        let next = value.checked_mul(10).and_then(|v| {
            if negative {
                v.checked_sub(d)
            } else {
                v.checked_add(d)
            }
        });
        match next {
            Some(v) => value = v,
            None => overflow = true,
        }
        i = i.saturating_add(1);
    }
    if i == start {
        return None;
    }
    if overflow {
        value = if negative { i64::MIN } else { i64::MAX };
    }
    let [a, b, c, d, ..] = value.to_le_bytes();
    Some((i32::from_le_bytes([a, b, c, d]), i))
}

/// `sscanf (value, "%d/%d/%d%c", &r, &g, &b, &bad)`: how many it converts
/// (an input that ends before the first, which is `EOF` upstream, counts 0:
/// the only question asked is whether it is 3), and the numbers.
fn scan_rgb(v: &[u8]) -> (usize, [i32; 3]) {
    let mut vals = [0i32; 3];
    let mut i = 0usize;
    let mut got = 0usize;
    for k in 0..3 {
        if k > 0 {
            if v.get(i) != Some(&b'/') {
                return (got, vals);
            }
            i = i.saturating_add(1);
        }
        let Some((n, end)) = scan_int(v, i) else {
            return (got, vals);
        };
        if let Some(slot) = vals.get_mut(k) {
            *slot = n;
        }
        got = got.saturating_add(1);
        i = end;
    }
    if v.get(i).is_some() {
        got = got.saturating_add(1);
    }
    (got, vals)
}

/// `check_colors (tp)`: whether the color capabilities agree.
fn check_colors(cx: &mut Cx<'_, '_>) {
    let max_colors = cx.num("max_colors");
    let max_pairs = cx.num("max_pairs");
    if (max_colors > 0) != (max_pairs > 0)
        || (max_colors > max_pairs && cx.v("initialize_pair").is_none())
    {
        cx.warn(format!(
            "inconsistent values for max_colors ({max_colors}) and max_pairs ({max_pairs})"
        ));
    }

    cx.paired("set_foreground", "set_background");
    cx.paired("set_a_foreground", "set_a_background");
    cx.paired("set_color_pair", "initialize_pair");

    for (plain, ansi, which) in [
        ("set_foreground", "set_a_foreground", "setf/setaf"),
        ("set_background", "set_a_background", "setb/setab"),
    ] {
        if let (Some(a), Some(b)) = (cx.v(plain), cx.v(ansi)) {
            if capcmp(Some(&a), Some(&b)) == 0 {
                cx.warn(format!("expected {which} to be different"));
            } else if same_color(cx, &a, &b, max_colors) {
                cx.warn(format!("{which} are equivalent"));
            }
        }
    }

    // "see: has_colors()"
    if max_colors >= 0
        && max_pairs >= 0
        && ((cx.v("set_foreground").is_some() && cx.v("set_background").is_some())
            || (cx.v("set_a_foreground").is_some() && cx.v("set_a_background").is_some())
            || cx.s("set_color_pair") != Str::Absent)
        && cx.v("orig_pair").is_none()
        && cx.v("orig_colors").is_none()
    {
        cx.warn("expected either op/oc string for resetting colors");
    }
    if cx.flag("can_change") != 0 {
        if cx.v("initialize_pair").is_none() && cx.v("initialize_color").is_none() {
            cx.warn("expected initc or initp because ccc is given");
        }
    } else if cx.v("initialize_pair").is_some() || cx.v("initialize_color").is_some() {
        cx.warn("expected ccc because initc is given");
    }
    let rgb = match cx.term.tigetstr(b"RGB") {
        TiString::Value(v) => Some(cstr(v).to_vec()),
        TiString::Absent | TiString::NotAString => None,
    };
    if let Some(value) = rgb {
        let (code, [r, g, b]) = scan_rgb(&value);
        if code != 3 || r <= 0 || g <= 0 || b <= 0 {
            cx.warn([b"unexpected value for RGB capability: ".as_slice(), &value].concat());
        }
    }
}

/// `csi_length (value)`: how long the control sequence introducer that
/// begins `value` is -- 2 for `ESC [`, 1 for the 8-bit one, else 0.
fn csi_length(v: &[u8]) -> usize {
    if v.first() == Some(&0x1b) && v.get(1) == Some(&b'[') {
        2
    } else if v.first() == Some(&0x9a) {
        1
    } else {
        0
    }
}

/// `keypad_final (string)`: the last byte of a VT100 application-keypad
/// key (`ESC O x`), else 0.
fn keypad_final(s: Option<&[u8]>) -> u8 {
    match s {
        Some([0x1b, b'O', c]) => *c,
        _ => 0,
    }
}

/// `keypad_index (string)`: where a VT100 application-keypad key is in
/// the keypad's order, else -1.
fn keypad_index(s: Option<&[u8]>) -> i64 {
    let ch = keypad_final(s);
    if ch == 0 {
        return -1;
    }
    // "app-keypad except "Enter""
    b"PQRSwxymtuvlqrsPpn"
        .iter()
        .position(|&c| c == ch)
        .and_then(|p| i64::try_from(p).ok())
        .unwrap_or(-1)
}

/// `check_ansi_cursor (list)`: whether the four cursor movements (down,
/// up, left, right) are ANSI's alike -- "left" may be `^H` and "down"
/// `^J`.
fn check_ansi_cursor(cx: &mut Cx<'_, '_>, list: &[Vec<u8>; 4]) {
    let mut skip = [false; 4];
    let mut repeated = false;
    for (j, lj) in list.iter().enumerate() {
        for lk in list.iter().take(j) {
            if lj == lk {
                let value = tic_expand(Some(lk), true, 0);
                cx.warn([b"repeated cursor control ".as_slice(), &value].concat());
                repeated = true;
            }
        }
    }
    if repeated {
        return;
    }
    let [down, up, left, _] = list;
    let prefix = csi_length(up);
    let mut suffix = prefix;
    if prefix != 0 {
        while up.get(suffix).is_some_and(u8::is_ascii_digit) {
            suffix = suffix.saturating_add(1);
        }
    }
    if prefix == 0 || up.get(suffix) != Some(&b'A') {
        return;
    }
    if let Some(s) = skip.get_mut(1) {
        *s = true;
    }
    if down.as_slice() == b"\n"
        && let Some(s) = skip.get_mut(0)
    {
        *s = true;
    }
    if left.as_slice() == b"\x08"
        && let Some(s) = skip.get_mut(2)
    {
        *s = true;
    }
    for ((item, skipped), want) in list.iter().zip(skip).zip(*b"BADC") {
        if skipped || item.len() == 1 {
            continue;
        }
        let shown = tic_expand(Some(item), true, 0);
        if item.get(..prefix) != up.get(..prefix) {
            cx.warn([b"inconsistent prefix for ".as_slice(), &shown].concat());
            continue;
        }
        if item.len() < suffix {
            let expected = format!(", expected {}", suffix.saturating_add(1));
            cx.warn(
                [
                    b"inconsistent length for ".as_slice(),
                    &shown,
                    expected.as_bytes(),
                ]
                .concat(),
            );
            continue;
        }
        // Where the item ends there, `%c` writes its NUL.
        let have = item.get(suffix).copied().unwrap_or(0);
        if have != want {
            cx.warn(
                [
                    b"inconsistent suffix for ".as_slice(),
                    &shown,
                    b", expected ",
                    &[want],
                    b", have ",
                    &[have],
                ]
                .concat(),
            );
        }
    }
}

/// `check_noaddress (tp, why)`: a terminal that is not one should have no
/// cursor addressing.
fn check_noaddress(cx: &mut Cx<'_, '_>, why: &str) {
    // `row_address` twice, as upstream has it.
    for name in [
        "column_address",
        "cursor_address",
        "cursor_home",
        "cursor_mem_address",
        "cursor_to_ll",
        "row_address",
        "row_address",
    ] {
        if cx.present(name) {
            cx.warn(format!("unexpected {name}, for {why}"));
        }
    }
}

/// `EXPECTED (name)` for each of `names`.
fn expected(cx: &mut Cx<'_, '_>, names: [&str; 4]) {
    for name in names {
        if !cx.present(name) {
            cx.warn(format!("expected {name}"));
        }
    }
}

/// The values of those of `names` that are present, in order.
fn present_values(cx: &Cx<'_, '_>, names: [&str; 4]) -> Vec<Vec<u8>> {
    names
        .iter()
        .filter(|n| cx.present(n))
        .map(|n| cx.v(n).unwrap_or_default())
        .collect()
}

/// `check_cursor (tp)`: whether the terminal can move its cursor, and
/// whether its movements agree.
fn check_cursor(cx: &mut Cx<'_, '_>) {
    if cx.flag("hard_copy") != 0 {
        check_noaddress(cx, "hard_copy");
    } else if cx.flag("generic_type") != 0 {
        check_noaddress(cx, "generic_type");
    } else if !cx.tp.term_names.contains(&b'+') {
        let (mut y, mut x) = (0i32, 0i32);
        if cx.present("column_address") {
            y = y.saturating_add(1);
        }
        if cx.present("cursor_address") {
            y = 10;
            x = 10;
        }
        if cx.present("cursor_home") {
            y = y.saturating_add(1);
            x = x.saturating_add(1);
        }
        if cx.present("cursor_mem_address") {
            y = 10;
            x = 10;
        }
        if cx.present("cursor_to_ll") {
            y = y.saturating_add(1);
            x = x.saturating_add(1);
        }
        if cx.present("row_address") {
            x = x.saturating_add(1);
        }
        if cx.present("cursor_down") {
            y = y.saturating_add(1);
        }
        if cx.present("cursor_up") {
            y = y.saturating_add(1);
        }
        if cx.present("cursor_left") {
            x = x.saturating_add(1);
        }
        if cx.present("cursor_right") {
            x = x.saturating_add(1);
        }
        if x < 2 && y < 2 {
            cx.warn("terminal lacks cursor addressing");
        } else {
            if x < 2 {
                cx.warn("terminal lacks cursor column-addressing");
            }
            if y < 2 {
                cx.warn("terminal lacks cursor row-addressing");
            }
        }
    }

    // "it is rare to have an insert-line feature without a matching
    // delete"
    cx.andmissing("parm_insert_line", "insert_line");
    cx.andmissing("parm_delete_line", "delete_line");
    cx.andmissing("parm_insert_line", "parm_delete_line");

    // "if we have a parameterized form, then the non-parameterized is easy"
    cx.andmissing("parm_down_cursor", "cursor_down");
    cx.andmissing("parm_up_cursor", "cursor_up");
    cx.andmissing("parm_left_cursor", "cursor_left");
    cx.andmissing("parm_right_cursor", "cursor_right");

    // "Given any of a set of cursor movement, the whole set should be
    // present."
    let parm = [
        "parm_down_cursor",
        "parm_up_cursor",
        "parm_left_cursor",
        "parm_right_cursor",
    ];
    match <[Vec<u8>; 4]>::try_from(present_values(cx, parm)) {
        Ok(list) => check_ansi_cursor(cx, &list),
        Err(list) if !list.is_empty() => expected(cx, parm),
        Err(_) => {}
    }

    let plain = ["cursor_down", "cursor_up", "cursor_left", "cursor_right"];
    match <[Vec<u8>; 4]>::try_from(present_values(cx, plain)) {
        Ok(list) => check_ansi_cursor(cx, &list),
        Err(list) if !list.is_empty() => {
            let unusual = (cx.present("cursor_down")
                && cx.v("cursor_down").as_deref() != Some(b"\n"))
                || (cx.present("cursor_left") && cx.v("cursor_left").as_deref() != Some(b"\x08"))
                || (cx.present("cursor_up") && cx.v("cursor_up").is_some_and(|v| v.len() > 1))
                || (cx.present("cursor_right")
                    && cx.v("cursor_right").is_some_and(|v| v.len() > 1));
            if unusual {
                expected(cx, plain);
            }
        }
        Err(_) => {}
    }
}

/// `check_keypad (tp)`: whether a VT100-style keypad's five keys are mapped
/// consistently.
fn check_keypad(cx: &mut Cx<'_, '_>) {
    const KP_NAMES: [&str; 5] = ["ka1", "ka3", "kb2", "kc1", "kc3"];
    let vals: Vec<Option<Vec<u8>>> = ["key_a1", "key_a3", "key_b2", "key_c1", "key_c3"]
        .iter()
        .map(|k| cx.v(k))
        .collect();
    if vals.iter().all(Option::is_some) {
        let finals: Vec<u8> = vals.iter().map(|v| keypad_final(v.as_deref())).collect();
        // "special case: legacy coding using 1,2,3,0,. on the bottom"
        if cstr(&finals) == b"qsrpn" {
            return;
        }
        let list: Vec<i64> = vals.iter().map(|v| keypad_index(v.as_deref())).collect();
        // "check that they're all vt100 keys"
        if list.iter().any(|&l| l < 0) {
            return;
        }
        // "check if they're all in increasing order"
        let increase = list
            .windows(2)
            .filter(|w| matches!(w, [a, b] if b > a))
            .count();
        if increase != KP_NAMES.len().saturating_sub(1) {
            let mut show = Vec::new();
            let mut last: i64 = -1;
            for _ in 0..KP_NAMES.len() {
                let mut kk: Option<usize> = None;
                let mut test: i64 = 100;
                for (k, &l) in list.iter().enumerate() {
                    if l > last && l < test {
                        test = l;
                        kk = Some(k);
                    }
                }
                last = test;
                if let Some(name) = kk.and_then(|k| KP_NAMES.get(k)) {
                    show.push(b' ');
                    show.extend_from_slice(name.as_bytes());
                }
            }
            cx.warn([b"vt100 keypad order inconsistent: ".as_slice(), &show].concat());
        }
    } else if vals.iter().any(Option::is_some) {
        let mut show = Vec::new();
        for (v, name) in vals.iter().zip(KP_NAMES) {
            if keypad_index(v.as_deref()) >= 0 {
                show.push(b' ');
                show.extend_from_slice(name.as_bytes());
            }
        }
        if !show.is_empty() {
            cx.warn([b"vt100 keypad map incomplete:".as_slice(), &show].concat());
        }
    }

    // "These warnings are useful for consistency checks - it is possible
    // that there are real terminals with mismatches in these"
    cx.andmissing("key_ic", "key_dc");
}

/// `check_printer (tp)`.
fn check_printer(cx: &mut Cx<'_, '_>) {
    for (a, b) in [
        ("enter_doublewide_mode", "exit_doublewide_mode"),
        ("enter_italics_mode", "exit_italics_mode"),
        ("enter_leftward_mode", "exit_leftward_mode"),
        ("enter_micro_mode", "exit_micro_mode"),
        ("enter_shadow_mode", "exit_shadow_mode"),
        ("enter_subscript_mode", "exit_subscript_mode"),
        ("enter_superscript_mode", "exit_superscript_mode"),
        ("enter_upward_mode", "exit_upward_mode"),
    ] {
        cx.paired(a, b);
    }
    cx.andmissing("start_char_set_def", "stop_char_set_def");

    // "If we have a parameterized form, then the non-parameterized is easy.
    // note: parameterized/non-parameterized margin settings are unrelated."
    cx.andmissing("parm_down_micro", "micro_down");
    cx.andmissing("parm_left_micro", "micro_left");
    cx.andmissing("parm_right_micro", "micro_right");
    cx.andmissing("parm_up_micro", "micro_up");
}

/// `uses_SGR_39_49 (value)`: whether a string resets both default colors.
fn uses_sgr_39_49(v: &[u8]) -> bool {
    v.windows(5).any(|w| w == b"39;49" || w == b"49;39")
}

/// `check_screen (tp)`: the extensions `screen` uses, consistently.
fn check_screen(cx: &mut Cx<'_, '_>) {
    if !cx.ck.user_definable {
        return;
    }
    // `VALID_BOOLEAN`: 0 or 1; anything else (absent, cancelled) is false.
    let valid = |b: i32| if b == 0 || b == 1 { b } else { 0 };
    let have_xt = valid(cx.term.tigetflag(b"XT"));
    let have_xm = valid(cx.term.tigetflag(b"XM"));
    let have_bce = valid(i32::from(cx.flag("back_color_erase")));
    let name = first_name(&cx.tp.term_names);
    let is_screen = name.starts_with(b"screen");
    let screen_base = is_screen && !name.contains(&b'.');
    let kmous = cx.v("key_mouse");
    let have_kmouse = kmous.as_deref() == Some(b"\x1b[M");
    let mut use_sgr_39_49 = false;
    let mut name_39_49 = "orig_pair or orig_colors";
    if have_bce != 0 {
        if let Some(op) = cx.v("orig_pair") {
            name_39_49 = "orig_pair";
            use_sgr_39_49 = uses_sgr_39_49(&op);
        }
        if !use_sgr_39_49 && let Some(oc) = cx.v("orig_colors") {
            name_39_49 = "orig_colors";
            use_sgr_39_49 = uses_sgr_39_49(&oc);
        }
    }

    if have_xm != 0 && have_xt != 0 {
        cx.warn("screen's XT capability conflicts with XM");
    } else if have_xt != 0 && screen_base {
        cx.warn("screen's \"screen\" entries should not have XT set");
    } else if have_xt != 0 {
        if !have_kmouse && is_screen {
            if kmous.is_some() {
                cx.warn("value of kmous inconsistent with screen's usage");
            } else {
                cx.warn("expected kmous capability with XT");
            }
        }
        if cx.num("max_colors") > 0 {
            if have_bce == 0 {
                cx.warn("expected bce capability with XT");
            } else if !use_sgr_39_49 {
                cx.warn(format!(
                    "expected {name_39_49} capability with XT to have 39/49 parameters"
                ));
            }
        }
        if let Some(tsl) = cx.v("to_status_line")
            && let Some(semi) = tsl.iter().position(|&c| c == b';')
            && tsl.get(semi.saturating_add(1)).is_none()
        {
            cx.warn("\"tsl\" capability is redundant, given XT");
        }
    } else if have_kmouse && have_xm == 0 && !screen_base && !name.contains(&b'+') {
        cx.warn("expected XT to be set, given kmous");
    }
}

/// `expected_params (name)`: how many parameters a capability takes --
/// "function-keys, etc., use none".
fn expected_params(name: &[u8]) -> i32 {
    const TABLE: &[(&str, i32)] = &[
        ("S0", 1),
        ("birep", 2),
        ("chr", 1),
        ("colornm", 1),
        ("cpi", 1),
        ("csnm", 1),
        ("csr", 2),
        ("cub", 1),
        ("cud", 1),
        ("cuf", 1),
        ("cup", 2),
        ("cuu", 1),
        ("cvr", 1),
        ("cwin", 5),
        ("dch", 1),
        ("defc", 3),
        ("dial", 1),
        ("dispc", 1),
        ("dl", 1),
        ("ech", 1),
        ("getm", 1),
        ("hpa", 1),
        ("ich", 1),
        ("il", 1),
        ("indn", 1),
        ("initc", 4),
        ("initp", 7),
        ("lpi", 1),
        ("mc5p", 1),
        ("mrcup", 2),
        ("mvpa", 1),
        ("pfkey", 2),
        ("pfloc", 2),
        ("pfx", 2),
        ("pfxl", 3),
        ("pln", 2),
        ("qdial", 1),
        ("rcsd", 1),
        ("rep", 2),
        ("rin", 1),
        ("sclk", 3),
        ("scp", 1),
        ("scs", 1),
        ("scsd", 2),
        ("setab", 1),
        ("setaf", 1),
        ("setb", 1),
        ("setcolor", 1),
        ("setf", 1),
        ("sgr", 9),
        ("sgr1", 6),
        ("slength", 1),
        ("slines", 1),
        ("smgbp", 1),
        ("smglp", 1),
        ("smglr", 2),
        ("smgrp", 1),
        ("smgtb", 2),
        ("smgtp", 1),
        ("tsl", 1),
        ("u6", -1),
        ("vpa", 1),
        ("wind", 4),
        ("wingo", 1),
    ];
    TABLE
        .iter()
        .find(|(n, _)| n.as_bytes() == name)
        .map_or(0, |&(_, c)| c)
}

/// `lookup_user_capability (name)`: one of the user-definable capabilities
/// ncurses' database uses, keys aside.
fn lookup_user_capability(name: &[u8]) -> Option<&'static captab::UserCap> {
    if name.first() == Some(&b'k') {
        None
    } else {
        tables::find_user_entry(name)
    }
}

/// `is_user_capability (name)`: how many parameters a user capability
/// takes, or -1 for a name that is none. ("ncurses assumes that u6 could be
/// used for getting the cursor-position, but that is not implemented.")
fn is_user_capability(cx: &Cx<'_, '_>, name: &[u8]) -> i32 {
    if let [b'u', d] = name
        && d.is_ascii_digit()
    {
        return if *d == b'6' { 2 } else { 0 };
    }
    if cx.ck.using_extensions
        && let Some(p) = lookup_user_capability(name)
    {
        return i32::try_from(p.argc).unwrap_or(-1);
    }
    -1
}

/// `line_capability (name)`: whether a capability's delay may be
/// proportional to the lines it affects.
fn line_capability(name: &[u8]) -> bool {
    [
        "csr", "clear", "ed", "cwin", "cup", "cud1", "home", "mrcup", "ll", "cuu1", "dl1", "hd",
        "flash", "ff", "il1", "nel", "dl", "cud", "indn", "il", "rin", "cuu", "mc0", "vpa", "ind",
        "ri", "hu",
    ]
    .iter()
    .any(|n| n.as_bytes() == name)
}

/// `check_params (tp, name, value, extended)`: "a quick sanity check for the
/// parameters which are used in the given strings". `index` is the
/// string's, for upstream's `value == set_attributes`.
#[allow(clippy::too_many_lines, reason = "upstream's check_params")]
fn check_params(cx: &mut Cx<'_, '_>, name: &[u8], value: &[u8], extended: bool, index: usize) {
    let mut expected = expected_params(name);
    let mut actual: i32 = 0;
    let mut params = [false; NUM_PARM + 1];

    for (cap, other) in [
        (b"smgrp".as_slice(), "set_left_margin_parm"),
        (b"smglp", "set_right_margin_parm"),
        (b"smgbp", "set_top_margin_parm"),
        (b"smgtp", "set_bottom_margin_parm"),
    ] {
        if name == cap && cx.v(other).is_none() {
            expected = 2;
        }
    }

    let mut s = 0usize;
    while let Some(&c) = value.get(s) {
        if c == b'%' {
            s = s.saturating_add(1);
            match value.get(s) {
                None => {
                    cx.warn([b"expected character after % in ".as_slice(), name].concat());
                    break;
                }
                Some(b'p') => {
                    s = s.saturating_add(1);
                    match value.get(s) {
                        Some(&d) if d.is_ascii_digit() => {
                            let n = d.wrapping_sub(b'0');
                            actual = actual.max(i32::from(n));
                            if let Some(slot) = params.get_mut(usize::from(n)) {
                                *slot = true;
                            }
                        }
                        _ => {
                            cx.warn([b"expected digit after %p in ".as_slice(), name].concat());
                            return;
                        }
                    }
                }
                Some(_) => {}
            }
        }
        s = s.saturating_add(1);
    }

    if extended {
        let check = is_user_capability(cx, name);
        if check != actual && check >= 0 && actual >= 0 {
            cx.warn(
                [
                    b"extended ".as_slice(),
                    name,
                    format!(" capability has {actual} parameters, expected {check}").as_bytes(),
                ]
                .concat(),
            );
        } else if cx.ck.debug_level > 1 {
            cx.warn(
                [
                    b"extended ".as_slice(),
                    name,
                    format!(" capability has {actual} parameters, as expected").as_bytes(),
                ]
                .concat(),
            );
        }
        expected = actual;
    }

    if params.first().copied().unwrap_or(false) {
        cx.warn([name, b" refers to parameter 0 (%p0), which is not allowed"].concat());
    }
    if index == idx("set_attributes") || expected < 0 {
        // `sgr`'s parameters are all optional.
    } else if expected != actual {
        cx.warn(
            [
                name,
                format!(" uses {actual} parameters, expected {expected}").as_bytes(),
            ]
            .concat(),
        );
        for (n, used) in params
            .iter()
            .enumerate()
            .take(usize::try_from(actual).unwrap_or(0))
            .skip(1)
        {
            if !used {
                cx.warn([name, format!(" omits parameter {n}").as_bytes()].concat());
            }
        }
    }

    // "Counting "%p" markers does not account for termcap expressions which
    // may not have been fully translated.  Also, tparm does its own
    // analysis.  Report differences here."
    cx.tparm.reset();
    let a = analyze(value);
    let analyzed = i32::try_from(a.actual()).unwrap_or(i32::MAX);
    if actual != analyzed && expected != analyzed {
        let user_cap = is_user_capability(cx, name);
        if user_cap == analyzed && cx.ck.using_extensions {
            // "ignore"
        } else if user_cap >= 0 {
            cx.warn(
                [
                    format!("tparm will use {analyzed} parameters for ").as_bytes(),
                    name,
                    format!(", expected {user_cap}").as_bytes(),
                ]
                .concat(),
            );
        } else {
            cx.warn(
                [
                    format!("tparm analyzed {analyzed} parameters for ").as_bytes(),
                    name,
                    format!(", expected {actual}").as_bytes(),
                ]
                .concat(),
            );
        }
    } else if expected > 0
        && actual == expected
        && guess_tparm_type(usize::try_from(expected).unwrap_or(0), &a.is_string)
            == TParams::Numbers
    {
        let limit = if matches!(name, b"setf" | b"setb" | b"setaf" | b"setab") {
            cx.num("max_colors").min(256)
        } else if line_capability(name) {
            24
        } else if is_user_capability(cx, name) < 0 {
            80
        } else {
            1
        };
        for n in 0..limit {
            cx.tparm.reset();
            // `(void) TPARM_9 (value, n, ...)`: only the errors matter.
            let _ = cx
                .tparm
                .tparm(&cx.term, value, &[Arg::Num(i64::from(n)); NUM_PARM]);
            let err = cx.tparm.err;
            if err != 0 {
                cx.warn(
                    [
                        format!("problem{} in tparm(", if err == 1 { "" } else { "s" }).as_bytes(),
                        name,
                        format!(", {n}, ...)").as_bytes(),
                    ]
                    .concat(),
                );
                if cx.ck.debug_level < 2 {
                    break;
                }
            }
        }
    }
}

/// `skip_DECSCNM (value, &flag)`: past a VT100 reverse-video control at the
/// start of `value` -- `flag` 1 for set, 0 for reset, -1 for neither -- or
/// 0 where there is none.
fn skip_decscnm(value: &[u8], flag: &mut i32) -> usize {
    *flag = -1;
    let skip = csi_length(value);
    if skip > 0
        && value.get(skip) == Some(&b'?')
        && value.get(skip.saturating_add(1)) == Some(&b'5')
    {
        match value.get(skip.saturating_add(2)) {
            Some(b'h') => *flag = 1,
            Some(b'l') => *flag = 0,
            _ => {}
        }
        return skip.saturating_add(3);
    }
    0
}

/// glibc's `sscanf (s, "%f%c", &f, &c) == 2`: a floating-point number
/// (`inf`, `nan` and hexadecimal ones included) and then any byte -- that
/// byte, or `None` where either is missing. As glibc does, an exponent
/// marker with no digits after it is taken and the number ends before it,
/// and a lone `0x` is no number.
fn float_then_char(s: &[u8]) -> Option<u8> {
    let s = cstr(s);
    let at = |i: usize| s.get(i).copied();
    let lower = |i: usize| at(i).map(|c| c.to_ascii_lowercase());
    let mut i = 0usize;
    while at(i).is_some_and(isspace) {
        i = i.saturating_add(1);
    }
    if matches!(at(i), Some(b'+' | b'-')) {
        i = i.saturating_add(1);
    }
    let word = |from: usize, w: &[u8]| {
        w.iter()
            .enumerate()
            .all(|(k, &c)| lower(from.saturating_add(k)) == Some(c))
    };
    match lower(i) {
        Some(b'n') => {
            return if word(i, b"nan") {
                at(i.saturating_add(3))
            } else {
                None
            };
        }
        Some(b'i') => {
            if !word(i, b"inf") {
                return None;
            }
            i = i.saturating_add(3);
            if lower(i) == Some(b'i') {
                // Begun, "infinity" must be finished.
                if !word(i, b"inity") {
                    return None;
                }
                i = i.saturating_add(5);
            }
            return at(i);
        }
        _ => {}
    }
    let start = i;
    let mut hexa = false;
    let mut got_digit = false;
    let mut got_dot = false;
    let mut got_e = false;
    let mut exp_char = b'e';
    let mut last_added = 0u8;
    if at(i) == Some(b'0') {
        i = i.saturating_add(1);
        last_added = b'0';
        if lower(i) == Some(b'x') {
            hexa = true;
            exp_char = b'p';
            last_added = b'x';
            i = i.saturating_add(1);
        } else {
            got_digit = true;
        }
    }
    while let Some(c) = at(i) {
        if c.is_ascii_digit() || (!got_e && hexa && c.is_ascii_hexdigit()) {
            got_digit = true;
            last_added = c;
        } else if got_e && last_added == exp_char && (c == b'-' || c == b'+') {
            last_added = c;
        } else if got_digit && !got_e && c.to_ascii_lowercase() == exp_char {
            got_e = true;
            got_dot = true;
            last_added = exp_char;
        } else if !got_dot && c == b'.' {
            got_dot = true;
            last_added = c;
        } else {
            break;
        }
        i = i.saturating_add(1);
    }
    // What `strtof` converts: a hexadecimal number always has its "0"; a
    // decimal one needs a digit.
    let read = i.saturating_sub(start);
    let converted = if hexa { read > 2 } else { got_digit };
    if read == 0 || !converted {
        return None;
    }
    at(i)
}

/// `check_delays (tp, name, value)`: whether the delays in a capability are
/// well formed, and belong there.
#[allow(clippy::too_many_lines, reason = "upstream's check_delays")]
fn check_delays(cx: &mut Cx<'_, '_>, name: &[u8], value: &[u8]) {
    let mut first: Option<usize> = None;
    let mut last: usize = 0;
    let mut p = 0usize;
    while p < value.len() {
        if value.get(p) == Some(&b'$') && value.get(p.saturating_add(1)) == Some(&b'<') {
            let base = p.saturating_add(2);
            let mut mark: Option<usize> = None;
            let mut mixed = false;
            let mut proportional = 0u32;
            let mut mandatory = 0u32;
            first = Some(p);
            let mut q = base;
            while let Some(&c) = value.get(q) {
                if c == b'>' {
                    if mark.is_none() {
                        mark = Some(q);
                    }
                    break;
                } else if c == b'*' || c == b'/' {
                    if c == b'*' {
                        proportional = proportional.saturating_add(1);
                    }
                    if c == b'/' {
                        mandatory = mandatory.saturating_add(1);
                    }
                    if mark.is_none() {
                        mark = Some(q);
                    }
                } else if !(c.is_ascii_alphanumeric() || b"+-.".contains(&c)) {
                    break;
                } else if proportional != 0 || mandatory != 0 {
                    mixed = true;
                }
                q = q.saturating_add(1);
            }
            let ended = q >= value.len();
            last = if ended { q } else { q.saturating_add(1) };
            if ended {
                // "restart scan"
                p = q.saturating_sub(1);
            } else {
                let check_c = float_then_char(value.get(base..).unwrap_or_default());
                let mark_c = mark.and_then(|m| value.get(m).copied());
                if check_c.is_none() || (mark_c.is_some() && check_c != mark_c) || mixed {
                    cx.warn(
                        [
                            b"syntax error in ".as_slice(),
                            name,
                            b" delay '",
                            value.get(base..q).unwrap_or_default(),
                            b"'",
                        ]
                        .concat(),
                    );
                } else if name.first() == Some(&b'k') {
                    cx.warn([b"function-key ".as_slice(), name, b" has delay"].concat());
                } else if proportional != 0 && !line_capability(name) {
                    cx.warn(
                        [
                            b"non-line capability using proportional delay: ".as_slice(),
                            name,
                        ]
                        .concat(),
                    );
                } else if cx.flag("xon_xoff") == 0 && mandatory == 0 && !cx.is_fragment() {
                    let what: &[u8] = if proportional != 0 {
                        b"proportional delay"
                    } else {
                        b"delay"
                    };
                    cx.warn([what, b" in ", name, b" is used since no xon/xoff"].concat());
                }
            }
        }
        p = p.saturating_add(1);
    }

    if name == b"flash" || name == b"beep" {
        if let Some(first) = first {
            if first == 0 || value.get(last).is_none() {
                // "Delay is on one end or the other."
                cx.warn([b"expected delay embedded within ".as_slice(), name].concat());
            }
        } else {
            // "Check for missing delay when using VT100 reverse-video.  A
            // real VT100 might not need this, but terminal emulators do."
            let mut flag = -1;
            let after = skip_decscnm(value, &mut flag);
            if flag > 0 {
                skip_decscnm(value.get(after..).unwrap_or_default(), &mut flag);
                if flag == 0 {
                    cx.warn([b"expected a delay in ".as_slice(), name].concat());
                }
            }
        }
    }
}

/// `check_1_infotocap (name, value, count)`: a capability expanded with
/// every parameter `count` (or "XYZ" and `count`, for a string one).
fn check_1_infotocap(cx: &mut Cx<'_, '_>, name: &[u8], value: &[u8], count: i32) -> Vec<u8> {
    let string = format!("XYZ{count}").into_bytes();
    let n = i64::from(count);
    cx.tparm.reset();
    let expect = tparm_type(name);
    let a = analyze(value);
    let mut actual = guess_tparm_type(a.parsed, &a.is_string);
    if expect != actual {
        cx.warn([name, b" has mismatched parameters"].concat());
        actual = TParams::Other;
    }
    cx.tparm.reset();
    let s = Arg::Str(Some(&string));
    let result = match actual {
        TParams::Str => cx.tparm.tparm(&cx.term, value, &[s]),
        TParams::NumStr => cx.tparm.tparm(&cx.term, value, &[Arg::Num(n), s]),
        TParams::StrStr => cx.tparm.tparm(&cx.term, value, &[s, s]),
        TParams::NumStrStr => cx.tparm.tparm(&cx.term, value, &[Arg::Num(n), s, s]),
        TParams::Numbers => cx.tiparm(9, value, &[count; 9]),
        TParams::Other => {
            let args: Vec<Arg<'_>> = a
                .is_string
                .iter()
                .map(|&is| if is { s } else { Arg::Num(n) })
                .collect();
            cx.tparm.tparm(&cx.term, value, &args)
        }
    };
    // Upstream `strdup`s a null answer, and crashes; here it is empty.
    result.unwrap_or_default()
}

/// `parse_delay_value (src, &delays, always)`: where a delay's number and
/// its `*` (and, `slash`, `/`) flags end. Upstream computes the delay too,
/// which none of its callers compares.
fn parse_delay_value(src: &[u8], from: usize, slash: bool) -> usize {
    let mut i = from;
    while src.get(i).is_some_and(u8::is_ascii_digit) {
        i = i.saturating_add(1);
    }
    if src.get(i) == Some(&b'.') {
        i = i.saturating_add(1);
        while src.get(i).is_some_and(u8::is_ascii_digit) {
            i = i.saturating_add(1);
        }
    }
    while let Some(&c) = src.get(i).filter(|&&c| c == b'*' || c == b'/') {
        if !slash && c == b'/' {
            break;
        }
        i = i.saturating_add(1);
    }
    i
}

/// `parse_ti_delay (ti, &delays)` from `from`: the scan of a terminfo
/// string for its delays, which runs to its end.
fn parse_ti_delay(ti: &[u8], from: usize) -> usize {
    let mut i = from;
    while i < ti.len() {
        if ti.get(i) == Some(&b'\\') {
            i = i.saturating_add(1);
        }
        if ti.get(i) == Some(&b'$')
            && ti.get(i.saturating_add(1)) == Some(&b'<')
            && ti
                .get(i.saturating_add(2))
                .is_some_and(|&c| c == b'.' || c.is_ascii_digit())
        {
            let last = parse_delay_value(ti, i.saturating_add(2), true);
            // Upstream stays where it is when the delay is not closed, and
            // never ends (see the module comment).
            i = if ti.get(last) == Some(&b'>') {
                last
            } else {
                i.saturating_add(1)
            };
        } else {
            i = i.saturating_add(1);
        }
    }
    i
}

/// `same_ti_tc (ti, tc, &embedded)`: whether a terminfo string and its
/// termcap translation are the same but for their delays; `embedded` when
/// the terminfo one's delay is inside it, where termcap cannot have one.
fn same_ti_tc(ti: &[u8], tc: &[u8], embedded: &mut bool) -> bool {
    let at = |s: &[u8], i: usize| s.get(i).copied().unwrap_or(0);
    let mut same = true;
    *embedded = false;
    let ti_last = parse_ti_delay(ti, 0);
    let mut t = parse_delay_value(tc, 0, false);
    let mut i = 0usize;
    while i < ti_last && at(tc, t) != 0 {
        if at(ti, i) == b'\\' && at(ti, i.saturating_add(1)).is_ascii_punctuation() {
            i = i.saturating_add(1);
            if at(ti, i) == b'^' && tc.get(t..t.saturating_add(4)) == Some(b"\\136".as_slice()) {
                i = i.saturating_add(1);
                t = t.saturating_add(4);
                continue;
            }
        } else if at(ti, i) == b'$' && at(ti, i.saturating_add(1)) == b'<' {
            let ss = parse_ti_delay(ti, i);
            if ss != i {
                *embedded = true;
                i = ss;
                continue;
            }
        }
        if at(tc, t) == b'\\' && at(tc, t.saturating_add(1)).is_ascii_punctuation() {
            t = t.saturating_add(1);
        }
        let (a, b) = (at(ti, i), at(tc, t));
        i = i.saturating_add(1);
        t = t.saturating_add(1);
        if a != b {
            same = false;
            break;
        }
    }
    if *embedded {
        if same {
            same = false;
        } else {
            // "report only one problem"
            *embedded = false;
        }
    }
    same
}

/// `check_infotocap (tp, i, value)`: whether a capability survives its
/// translation to termcap (`-C`).
fn check_infotocap(cx: &mut Cx<'_, '_>, i: usize, value: &[u8]) {
    let name = cx.str_name(i);
    let ti_value = value.to_vec();
    // Upstream tests the value's first byte, not the name's.
    let params = match captab::PARAMETRIZED.get(i) {
        Some(&p) => i32::from(p),
        None if value.first() == Some(&b'k') => 0,
        None => i32::from(has_params(value, false)),
    };
    let Some(tc_value) = infotocap(&ti_value, params, cx.scan.strict_bsd) else {
        cx.warn([b"tic-conversion of ".as_slice(), &name, b" failed"].concat());
        return;
    };
    if params > 0 {
        let limit = if matches!(name.as_slice(), b"setf" | b"setb" | b"setaf" | b"setab") {
            cx.num("max_colors").min(256)
        } else {
            5
        };
        let mut first = true;
        for count in 0..limit {
            let ti_check = check_1_infotocap(cx, &name, &ti_value, count);
            let tc_check = check_1_infotocap(cx, &name, &tc_value, count);
            if ti_check != tc_check {
                if first {
                    cx.scan.emit(
                        &[
                            b"check_infotocap(".as_slice(),
                            &name,
                            b")\n...ti '",
                            &visbuf(&Str::Value(ti_value.clone())),
                            b"'\n...tc '",
                            &visbuf(&Str::Value(tc_value.clone())),
                            b"'\n",
                        ]
                        .concat(),
                    );
                    first = false;
                }
                cx.warn(
                    [
                        b"tparm-conversion of ".as_slice(),
                        &name,
                        format!("({count}) differs between\n\tterminfo ").as_bytes(),
                        &visbuf(&Str::Value(ti_check)),
                        b"\n\ttermcap  ",
                        &visbuf(&Str::Value(tc_check)),
                    ]
                    .concat(),
                );
            }
        }
    } else if params == 0 {
        let mut embedded = false;
        if !same_ti_tc(cstr(&ti_value), cstr(&tc_value), &mut embedded) {
            if embedded {
                cx.warn(
                    [
                        b"termcap equivalent of ".as_slice(),
                        &name,
                        b" cannot use embedded delay",
                    ]
                    .concat(),
                );
            } else {
                cx.warn(
                    [
                        b"tic-conversion of ".as_slice(),
                        &name,
                        b" changed value\n\tfrom ",
                        cstr(&ti_value),
                        b"\n\tto   ",
                        cstr(&tc_value),
                    ]
                    .concat(),
                );
            }
        }
    }
}

/// `skip_delay (s)`: past a delay's digits and `/`.
fn skip_delay(s: &[u8], from: usize) -> usize {
    let mut i = from;
    while s.get(i).is_some_and(|&c| c == b'/' || c.is_ascii_digit()) {
        i = i.saturating_add(1);
    }
    i
}

/// `ignore_delays (s)`: past a delay altogether -- "when comparing a simple
/// string to sgr, the latter may have a worst-case delay on the end".
fn ignore_delays(s: &[u8], from: usize) -> usize {
    let mut i = from;
    let mut delaying = 0;
    loop {
        match s.get(i).copied().unwrap_or(0) {
            b'$' => {
                if delaying == 0 {
                    delaying = 1;
                }
            }
            b'<' => {
                if delaying == 1 {
                    delaying = 2;
                }
            }
            0 => delaying = 0,
            _ => {
                if delaying != 0 {
                    i = skip_delay(s, i);
                    if s.get(i) == Some(&b'>') {
                        i = i.saturating_add(1);
                    }
                    delaying = 0;
                }
            }
        }
        if delaying == 0 {
            break;
        }
        i = i.saturating_add(1);
    }
    i
}

/// `similar_sgr (num, a, b)`: whether `b` is in `a` -- "An sgr string may
/// contain several settings other than the one we're interested in,
/// essentially sgr0 + rmacs + whatever.  As long as the "whatever" is
/// contained in the sgr string, that is close enough for our sanity check."
fn similar_sgr(cx: &mut Cx<'_, '_>, num: i32, a: &[u8], b: &[u8]) -> bool {
    let at = |s: &[u8], i: usize| s.get(i).copied().unwrap_or(0);
    let (mut ia, mut ib) = (0usize, 0usize);
    let mut delaying = 0;
    while at(b, ib) != 0 {
        while at(a, ia) != at(b, ib) {
            if at(a, ia) == 0 {
                let rest = Str::Value(b.get(ib..).unwrap_or_default().to_vec());
                if num < 0 {
                    // Quietly.
                } else if at(b, ib) == b'$' && at(b, ib.saturating_add(1)) == b'<' {
                    cx.warn([b"did not find delay ".as_slice(), &visbuf(&rest)].concat());
                } else {
                    let which = usize::try_from(num)
                        .ok()
                        .and_then(|n| SGR_NAMES.get(n))
                        .copied()
                        .unwrap_or_default();
                    cx.warn(
                        [
                            format!("checking sgr({which}) ").as_bytes(),
                            &visbuf(&Str::Value(a.to_vec())),
                            b"\n\tcompare to ",
                            &visbuf(&Str::Value(b.to_vec())),
                            b"\n\tunmatched ",
                            &visbuf(&rest),
                        ]
                        .concat(),
                    );
                }
                return false;
            } else if delaying != 0 {
                let (na, nb) = (skip_delay(a, ia), skip_delay(b, ib));
                if (na, nb) == (ia, ib) {
                    // Upstream loops forever here, two delays differing in
                    // a byte neither skips; compare on instead.
                    delaying = 0;
                }
                (ia, ib) = (na, nb);
            } else if (at(b, ib) == b'0' || at(b, ib) == b';') && at(a, ia) == b'm' {
                ib = ib.saturating_add(1);
            } else {
                ia = ia.saturating_add(1);
            }
        }
        match at(a, ia) {
            b'$' => {
                if delaying == 0 {
                    delaying = 1;
                }
            }
            b'<' => {
                if delaying == 1 {
                    delaying = 2;
                }
            }
            _ => delaying = 0,
        }
        ia = ia.saturating_add(1);
        ib = ib.saturating_add(1);
    }
    // "ignore delays on the end of the string"
    let ia = ignore_delays(a, ia);
    num != 0 || at(a, ia) == 0
}

/// `check_tparm_err (num)`.
fn check_tparm_err(cx: &mut Cx<'_, '_>, num: usize) {
    if cx.tparm.err != 0 {
        let which = SGR_NAMES.get(num).copied().unwrap_or_default();
        cx.warn(format!("tparam error in sgr({num}): {which}"));
    }
}

/// `check_sgr (tp, zero, num, cap, name)`: `sgr` with only attribute `num`
/// set, against `name`, the capability for that attribute.
fn check_sgr(cx: &mut Cx<'_, '_>, zero: Option<&[u8]>, num: usize, name: &str) -> Option<Vec<u8>> {
    let sgr = cx.v("set_attributes").unwrap_or_default();
    let params: Vec<i32> = (1..=9).map(|k| i32::from(num == k)).collect();
    let test = cx.tiparm(9, &sgr, &params);
    match &test {
        Some(t) => {
            if let Some(cap) = cx.v(name) {
                if !similar_sgr(cx, i32::try_from(num).unwrap_or(0), t, &cap) {
                    cx.warn(
                        [
                            format!("{name} differs from sgr({num})\n\t{name}=").as_bytes(),
                            &visbuf(&Str::Value(cap)),
                            format!("\n\tsgr({num})=").as_bytes(),
                            &visbuf(&Str::Value(t.clone())),
                        ]
                        .concat(),
                    );
                }
            } else if capcmp(Some(t), zero) != 0 {
                cx.warn(format!("sgr({num}) present, but not {name}"));
            }
        }
        None => {
            if cx.present(name) {
                cx.warn(format!("sgr({num}) missing, but {name} present"));
            }
        }
    }
    check_tparm_err(cx, num);
    test
}

/// `check_exit_attribute (name, test, trimmed, untrimmed)`: "Exiting a
/// video mode should not duplicate sgr0".
fn check_exit_attribute(cx: &mut Cx<'_, '_>, name: &str, trimmed: &[u8], untrimmed: &[u8]) {
    if let Some(test) = cx.v(name)
        && (similar_sgr(cx, -1, trimmed, &test) || similar_sgr(cx, -1, untrimmed, &test))
    {
        cx.warn(format!("{name} matches exit_attribute_mode"));
    }
}

/// `is_sgr_string (value)`: whether a string looks like a standard SGR one
/// -- a CSI, digits and `;`, and perhaps `m` at the end.
fn is_sgr_string(v: &[u8]) -> bool {
    let skip = csi_length(v);
    if skip == 0 {
        return false;
    }
    let rest = v.get(skip..).unwrap_or_default();
    let last = rest.len().saturating_sub(1);
    rest.iter()
        .enumerate()
        .all(|(k, &ch)| ch.is_ascii_digit() || ch == b';' || (ch == b'm' && k == last))
}

/// `tgoto (value, 0, 0)` with no termcap support: `_nc_tiparm` with two
/// parameters, then one, then none.
fn tgoto00(cx: &mut Cx<'_, '_>, value: &[u8]) -> Option<Vec<u8>> {
    if let Some(r) = cx.tiparm(2, value, &[0, 0]) {
        return Some(r);
    }
    if let Some(r) = cx.tiparm(1, value, &[0]) {
        return Some(r);
    }
    cx.tiparm(0, value, &[])
}

/// `check_sgr_param (tp, code, name, value)`: whether a color capability
/// sets SGR attribute `code` too.
fn check_sgr_param(cx: &mut Cx<'_, '_>, code: usize, name: &str) {
    let Some(value) = cx.v(name) else {
        return;
    };
    let ncv = code
        .checked_sub(1)
        .and_then(|c| u32::try_from(c).ok())
        .and_then(|c| 1i32.checked_shl(c))
        .unwrap_or(0);
    let Some(test) = tgoto00(cx, &value) else {
        return;
    };
    let test = cstr(&test);
    if !is_sgr_string(test) {
        return;
    }
    let code_i = i32::try_from(code).unwrap_or(-1);
    let mut param: i32 = 0;
    let mut count = 0u32;
    let mut skips: i32 = 0;
    // Upstream's `value == set_a_foreground || ...`: the four this checks.
    let color = true;
    for &c in test {
        if c.is_ascii_digit() {
            param = param
                .wrapping_mul(10)
                .wrapping_add(i32::from(c.wrapping_sub(b'0')));
            count = count.saturating_add(1);
        } else {
            if count != 0 {
                // "Avoid unnecessary warning for xterm 256color codes."
                if color && (param == 38 || param == 48) {
                    skips = 3;
                }
                let before = skips;
                skips = skips.saturating_sub(1);
                if before <= 0 && param == code_i {
                    break;
                }
            }
            count = 0;
            param = 0;
        }
    }
    if count != 0 && param == code_i {
        let ncv_cap = cx.num("no_color_video");
        if code == 0 || ncv_cap < 0 || (ncv_cap & ncv) == 0 {
            let which = SGR_NAMES.get(code).copied().unwrap_or_default();
            cx.warn(format!("\"{which}\" SGR-attribute used in {name}"));
        }
    }
}

/// `check_user_capability_type (name, actual)`: whether an extended
/// capability is of the type ncurses' own use of it is.
fn check_user_capability_type(cx: &mut Cx<'_, '_>, name: &[u8], actual: Kind) {
    if lookup_user_capability(name).is_some() {
        return;
    }
    let type_name = |k: Kind| -> &'static [u8] {
        match k {
            Kind::Boolean => b"boolean",
            Kind::Number => b"number",
            Kind::String => b"string",
        }
    };
    if let Some(e) = tables::find_entry(name, false) {
        cx.warn(
            [
                b"expected ".as_slice(),
                name,
                b" to be ",
                type_name(actual),
                b", but actually ",
                type_name(e.kind),
            ]
            .concat(),
        );
    } else if name.first() != Some(&b'k') {
        cx.warn(
            [
                b"undocumented ".as_slice(),
                type_name(actual),
                b" capability ",
                name,
            ]
            .concat(),
        );
    }
}

/// `IN_DELAY`: the bytes of a delay's number and flags.
const IN_DELAY: &[u8] = b"0123456789*/.";

/// `check_ANSI_cap (value, nparams, final)`: whether a capability is an ANSI
/// control taking `nparams` numbers and ending (but for a delay) in
/// `final`.
fn check_ansi_cap(value: Option<&[u8]>, nparams: usize, final_: u8) -> bool {
    let Some(value) = value else {
        return false;
    };
    if csi_length(value) == 0 {
        return false;
    }
    let a = analyze(value);
    if a.actual() != nparams || a.is_string.iter().take(nparams).any(|&s| s) {
        return false;
    }
    let mut in_delay = 0;
    let mut p = value.len();
    while let Some(q) = p.checked_sub(1) {
        p = q;
        let ch = value.get(p).copied().unwrap_or(0);
        if ch == final_ {
            return true;
        }
        match in_delay {
            0 => {
                if ch == b'>' {
                    in_delay = 1;
                }
            }
            1 => {
                if IN_DELAY.contains(&ch) {
                    continue;
                }
                if ch != b'<' {
                    p = 0;
                }
                in_delay = 2;
            }
            _ => {
                if ch != b'$' {
                    p = 0;
                }
                in_delay = 0;
            }
        }
    }
    false
}

/// `skip_Delay (value)`: past a delay at `i`, if one is there.
fn skip_delay_at(value: &[u8], i: usize) -> usize {
    if value.get(i) != Some(&b'$') || value.get(i.saturating_add(1)) != Some(&b'<') {
        return i;
    }
    let mut r = i.saturating_add(2);
    while value.get(r).is_some_and(|c| IN_DELAY.contains(c)) {
        r = r.saturating_add(1);
    }
    if value.get(r) == Some(&b'>') {
        r.saturating_add(1)
    } else {
        i
    }
}

/// `isValidEscape (value, expect)`: whether a string is `ESC`, `expect`, and
/// perhaps a delay.
fn is_valid_escape(value: Option<&[u8]>, expect: &[u8]) -> bool {
    let Some(v) = value else {
        return false;
    };
    v.first() == Some(&0x1b)
        && v.get(1..).is_some_and(|rest| rest.starts_with(expect))
        && skip_delay_at(v, expect.len().saturating_add(1)) >= v.len()
}

/// `guess_ANSI_VTxx (tp)`: 1 for a VT100-alike, 0 for an ANSI terminal,
/// -1 for neither.
fn guess_ansi_vtxx(cx: &Cx<'_, '_>) -> i32 {
    let v = |n: &str| cx.v(n);
    let is = |n: &str, s: &[u8]| v(n).as_deref() == Some(s);
    let mut checks = 0;
    // "VT100s have scrolling region, but ANSI (ECMA-48) does not specify"
    if check_ansi_cap(v("change_scroll_region").as_deref(), 2, b'r')
        && (is_valid_escape(v("scroll_forward").as_deref(), b"D")
            || is("scroll_forward", b"\n")
            || is_valid_escape(v("scroll_forward").as_deref(), b"6"))
        && (is_valid_escape(v("scroll_reverse").as_deref(), b"M")
            || is_valid_escape(v("scroll_reverse").as_deref(), b"9"))
    {
        checks |= 2;
    }
    if check_ansi_cap(v("cursor_address").as_deref(), 2, b'H')
        && check_ansi_cap(v("cursor_up").as_deref(), 0, b'A')
        && (check_ansi_cap(v("cursor_down").as_deref(), 0, b'B') || is("cursor_down", b"\n"))
        && check_ansi_cap(v("cursor_right").as_deref(), 0, b'C')
        && (check_ansi_cap(v("cursor_left").as_deref(), 0, b'D') || is("cursor_left", b"\x08"))
        && check_ansi_cap(v("clr_eos").as_deref(), 0, b'J')
        && check_ansi_cap(v("clr_bol").as_deref(), 0, b'K')
        && check_ansi_cap(v("clr_eol").as_deref(), 0, b'K')
    {
        checks |= 1;
    }
    match checks {
        3 => 1,
        1 => 0,
        _ => -1,
    }
}

/// `check_user_6789 (tp)`: the query and response extensions "which most
/// terminals support".
fn check_user_6789(cx: &mut Cx<'_, '_>) {
    // "Check if the terminal is known to not"
    if cx.term.tigetflag(b"NQ") > 0 {
        for (long, short) in [
            ("user6", "u6"),
            ("user7", "u7"),
            ("user8", "u8"),
            ("user9", "u9"),
        ] {
            if cx.present(long) {
                cx.warn(format!("{short} is not supported"));
            }
        }
        return;
    }
    cx.paired("user6", "user7");
    cx.paired("user8", "user9");
    if cx.tp.term_names.contains(&b'+') {
        return;
    }
    let guess = guess_ansi_vtxx(cx);
    if guess == 1 && !cx.present("user8") {
        cx.warn("expected u8/u9 for device-attributes");
    }
    if (guess == 1 || guess == 0) && !cx.present("user6") {
        cx.warn("expected u6/u7 for cursor-position");
    }
}

/// One key the terminal defines: `NAME_VALUE`.
struct NameValue {
    /// The key's code; -1 for an extended one.
    keycode: i64,
    /// The capability's name.
    name: Vec<u8>,
    /// `keyname (keycode)`.
    keyname: &'static str,
    /// The string it sends.
    value: Str,
}

/// `get_fkey_list (tp)`: the standard keys the entry defines, then every
/// extended capability whose name begins with `k`.
fn get_fkey_list(cx: &Cx<'_, '_>) -> Vec<NameValue> {
    let mut result = Vec::new();
    for &(offset, code, keyname) in &captab::TINFO_FKEYS {
        let a = cx.tp.strings.get(offset).cloned().unwrap_or_default();
        if a.present() {
            result.push(NameValue {
                keycode: i64::from(code),
                name: names::STRNAMES
                    .get(offset)
                    .map(|s| s.as_bytes().to_vec())
                    .unwrap_or_default(),
                keyname,
                value: a,
            });
        }
    }
    for j in STRCOUNT..cx.tp.strings.len() {
        let name = cx.str_name(j);
        if name.first() == Some(&b'k') {
            result.push(NameValue {
                keycode: -1,
                name,
                keyname: "",
                value: cx.tp.strings.get(j).cloned().unwrap_or_default(),
            });
        }
    }
    result
}

/// `show_fkey_name (data)`, onto `m`.
fn show_fkey_name(m: &mut Vec<u8>, data: &NameValue) {
    if data.keycode > 0 {
        m.push(b' ');
        m.extend_from_slice(data.keyname.as_bytes());
        m.extend_from_slice(b" (capability \"");
        m.extend_from_slice(&data.name);
        m.extend_from_slice(b"\")");
    } else {
        m.extend_from_slice(b" capability \"");
        m.extend_from_slice(&data.name);
        m.push(b'"');
    }
}

/// `check_conflict (tp)`: "A terminal entry may contain more than one
/// keycode assigned to a given string (e.g., KEY_END and KEY_LL).  But
/// curses will only return one (the last one assigned)."
fn check_conflict(cx: &mut Cx<'_, '_>) {
    // "SVr4 curses defines the "xcurses" names listed above except for the
    // special cases in the "shifted" column.  When using these names for
    // xterm's extensions, that was confusing, and resulted in adding
    // extended capabilities with "2" (shift) suffix.  This check warns
    // about unnecessary use of extensions for this quirk."
    const TABLE: [(&[u8], Option<&[u8]>); 9] = [
        (b"kDC", None),
        (b"kDN", Some(b"kind")),
        (b"kEND", None),
        (b"kHOM", None),
        (b"kLFT", None),
        (b"kNXT", None),
        (b"kPRV", None),
        (b"kRIT", None),
        (b"kUP", Some(b"kri")),
    ];
    if cx.scan.syntax == SYN_TERMCAP && cx.ck.capdump {
        return;
    }
    let given = get_fkey_list(cx);
    let mut check = vec![false; given.len().saturating_add(1)];
    let mut conflict = false;
    for (j, gj) in given.iter().enumerate() {
        let Some(a) = gj.value.valid() else {
            continue;
        };
        let mut first = true;
        let mut line = Vec::new();
        for (k, gk) in given.iter().enumerate().skip(j.saturating_add(1)) {
            let Some(b) = gk.value.valid() else {
                continue;
            };
            if check.get(k).copied().unwrap_or(false) {
                continue;
            }
            if capcmp(Some(a), Some(b)) == 0 {
                for c in [j, k] {
                    if let Some(c) = check.get_mut(c) {
                        *c = true;
                    }
                }
                if first {
                    if !conflict {
                        cx.warn("conflicting key definitions (using the last)");
                        conflict = true;
                    }
                    line.extend_from_slice(b"...");
                    show_fkey_name(&mut line, gj);
                    line.extend_from_slice(b" is the same as");
                    show_fkey_name(&mut line, gk);
                    first = false;
                } else {
                    line.extend_from_slice(b", ");
                    show_fkey_name(&mut line, gk);
                }
            }
        }
        if !first {
            line.push(b'\n');
            cx.scan.emit(&line);
        }
    }
    if !cx.ck.using_extensions {
        return;
    }
    for g in &given {
        if g.value.valid().is_none() {
            continue;
        }
        let find = g.name.as_slice();
        let Some((test, shifted, rest)) = TABLE.iter().find_map(|&(test, shifted)| {
            find.strip_prefix(test)
                .filter(|rest| !rest.is_empty())
                .map(|rest| (test, shifted, rest))
        }) else {
            continue;
        };
        // `sscanf (find + size, "%d%c", &value, &ch)`: 1 is a number alone.
        match scan_int(rest, 0) {
            Some((value, end)) if rest.get(end).is_none() => {
                if value == 2 {
                    cx.warn(
                        [
                            b"expected '".as_slice(),
                            shifted.unwrap_or(test),
                            b"' rather than '",
                            find,
                            b"'",
                        ]
                        .concat(),
                    );
                } else if !(2..=15).contains(&value) {
                    cx.warn([b"expected numeric 2..15 '".as_slice(), find, b"'"].concat());
                }
            }
            _ => {
                cx.warn([b"expected numeric suffix for '".as_slice(), find, b"'"].concat());
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn delays_scan_as_glibc_scanf_does() {
        // Measured against glibc 2.39's `sscanf ("%f%c")`.
        for (s, want) in [
            (b"5>".as_slice(), Some(b'>')),
            (b"5.5>", Some(b'>')),
            (b".5>", Some(b'>')),
            (b"5.>", Some(b'>')),
            (b".>", None),
            (b"+>", None),
            (b"5e>", Some(b'>')),
            (b"5e+>", Some(b'>')),
            (b"5E3*>", Some(b'*')),
            (b"inf>", Some(b'>')),
            (b"infinity>", Some(b'>')),
            (b"infinit>", None),
            (b"inft>", Some(b't')),
            (b"nan(123)>", Some(b'(')),
            (b"0x1p3>", Some(b'>')),
            (b"0x>", None),
            (b"0x.>", Some(b'>')),
            (b"0xg>", None),
            (b"00x5>", Some(b'x')),
            (b"5ms>", Some(b'm')),
            (b"e5>", None),
            (b"i>", None),
            (b"5", None),
            (b"", None),
            (b" 5>", Some(b'>')),
            (b"5.5.5>", Some(b'.')),
            (b"1,5>", Some(b',')),
            (b"-nan>", Some(b'>')),
        ] {
            assert_eq!(float_then_char(s), want, "{s:?}");
        }
    }

    #[test]
    fn integers_scan_as_glibc_scanf_does() {
        assert_eq!(scan_int(b"5x", 0), Some((5, 1)));
        assert_eq!(scan_int(b" 5", 0), Some((5, 2)));
        assert_eq!(scan_int(b"-", 0), None);
        assert_eq!(scan_int(b"x", 0), None);
        assert_eq!(scan_int(b"0x5", 0), Some((0, 1)));
        assert_eq!(scan_int(b"99999999999999999999", 0).unwrap().0, -1);
        assert_eq!(scan_int(b"4294967297", 0).unwrap().0, 1);
        assert_eq!(scan_int(b"2147483648", 0).unwrap().0, i32::MIN);
        assert_eq!(scan_int(b"-2147483649", 0).unwrap().0, i32::MAX);
        assert_eq!(scan_rgb(b"8/8/8"), (3, [8, 8, 8]));
        assert_eq!(scan_rgb(b"8/8/8x").0, 4);
        assert_eq!(scan_rgb(b"8 /8/8").0, 1);
        assert_eq!(scan_rgb(b" 8/ 8/ 8"), (3, [8, 8, 8]));
        assert_eq!(scan_rgb(b"4294967304/8/8"), (3, [8, 8, 8]));
    }

    #[test]
    fn tparm_types() {
        let mut p = [false; NUM_PARM];
        assert_eq!(guess_tparm_type(1, &p), TParams::Numbers);
        p[1] = true;
        assert_eq!(guess_tparm_type(2, &p), TParams::NumStr);
        p[2] = true;
        assert_eq!(guess_tparm_type(3, &p), TParams::NumStrStr);
        assert_eq!(guess_tparm_type(4, &p), TParams::Other);
        assert_eq!(tparm_type(b"pfx"), TParams::NumStr);
        assert_eq!(tparm_type(b"cup"), TParams::Numbers);
    }

    #[test]
    fn delays_inside_terminfo_and_termcap_strings() {
        let mut embedded = false;
        assert!(same_ti_tc(b"\\E[H", b"\\E[H", &mut embedded));
        assert!(!embedded);
        // A trailing delay is at the front in termcap -- and the comparison
        // stops where the termcap string does, before it.
        assert!(same_ti_tc(b"ab$<5>", b"5ab", &mut embedded));
        assert!(!embedded);
        // One inside the string is what termcap cannot say.
        assert!(!same_ti_tc(b"a$<5>b", b"5ab", &mut embedded));
        assert!(embedded);
        assert!(!same_ti_tc(b"ab", b"ac", &mut embedded));
        assert!(!embedded);
        // An unclosed delay ends the scan rather than looping.
        assert_eq!(parse_ti_delay(b"a$<5x>", 0), 6);
    }

    #[test]
    fn sgr_strings_and_ansi_caps() {
        assert!(is_sgr_string(b"\x1b[1;2m"));
        assert!(is_sgr_string(b"\x1b[1;2"));
        assert!(!is_sgr_string(b"\x1b[1mx"));
        assert!(!is_sgr_string(b"1m"));
        assert!(check_ansi_cap(Some(b"\x1b[%i%p1%d;%p2%dH"), 2, b'H'));
        assert!(check_ansi_cap(Some(b"\x1b[A$<5>"), 0, b'A'));
        assert!(!check_ansi_cap(Some(b"\x1b[%p1%dA"), 0, b'A'));
        assert!(is_valid_escape(Some(b"\x1bD"), b"D"));
        assert!(is_valid_escape(Some(b"\x1bD$<2>"), b"D"));
        assert!(!is_valid_escape(Some(b"\x1bDx"), b"D"));
        assert_eq!(ignore_delays(b"$<5/>", 0), 5);
    }

    #[test]
    fn every_name_the_checks_use_is_a_capability() {
        for name in [
            "set_attributes",
            "exit_attribute_mode",
            "acs_chars",
            "max_colors",
            "no_color_video",
            "back_color_erase",
            "hard_copy",
            "generic_type",
            "xon_xoff",
            "can_change",
            "set_left_margin_parm",
            "set_bottom_margin_parm",
            "user6",
            "user9",
            "display_clock",
            "remove_clock",
            "enter_doublewide_mode",
            "stop_char_set_def",
            "parm_up_micro",
            "micro_up",
            "key_a1",
            "key_c3",
            "key_mouse",
            "orig_colors",
            "to_status_line",
        ] {
            assert!(index().contains_key(name), "{name}");
        }
    }
}
