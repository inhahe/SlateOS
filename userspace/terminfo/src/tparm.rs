//! `tparm`: ncurses 6.4's `lib_tparm.c`, the stack machine a parameterised
//! capability runs on.
//!
//! A capability such as `sgr` is a little program: `%p1` pushes the first
//! parameter, `%d` pops and prints it, `%?`...`%t`...`%e`...`%;` is an
//! if-then-else, `%{12}` and `%'c'` push constants, `%+` and its kin pop
//! two and push one, `%P`/`%g` store and fetch variables -- `A`-`Z` kept
//! for the terminal between calls, `a`-`z` cleared once per call. Upstream's
//! quirks are kept, each where it arises: a NUL `%c` is written as `\200`,
//! a pop from an empty stack is 0 (or an empty string), the stack holds 20,
//! `%i` bumps the first two parameters once, and the result is a C string,
//! so it ends at the first NUL any `%c` of 256 put in it.
//!
//! [`Tparm::nc_tiparm`] is `_nc_tiparm`, the entry point the library itself
//! uses, with its checks of the parameter count; [`analyze`] is
//! `_nc_tparm_analyze`.
//!
//! # No allocation once warm
//!
//! [`Tparm::expand`] and [`Tparm::nc_expand`] answer with a slice of a
//! buffer the `Tparm` keeps -- upstream's `out_buff` -- and the format of a
//! `%d` is built in another it keeps, `fmt_buff`; both grow only when an
//! expansion needs more than any before it, and the stack is twenty slots
//! of fixed size. So once a terminal's capabilities have each been expanded,
//! expanding them again allocates nothing: what lets curses repaint from a
//! signal handler without reaching for the allocator, which the program it
//! interrupted may be holding. The forms that return a `Vec` copy that slice
//! for callers to keep.

use crate::Entry;
use crate::string as cap;

/// `STACKSIZE`.
const STACKSIZE: usize = 20;
/// `NUM_PARM`.
pub const NUM_PARM: usize = 9;
/// `NUM_VARS`.
const NUM_VARS: usize = 26;

/// One slot of the stack: a number, or a string parameter -- which is the
/// only string a capability can push, so it is kept as the parameter's
/// index, as upstream keeps a pointer to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Item {
    Num(i32),
    Str(usize),
}

/// What `_nc_tparm_analyze` finds in a capability.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Analysis {
    /// `num_parsed`: the pops it counts the termcap way, at most 9.
    pub parsed: usize,
    /// `num_popped`: the highest `%pN`, at most 9.
    pub popped: usize,
    /// `p_is_s`: which parameters a `%s` or `%l` takes as strings.
    pub is_string: [bool; NUM_PARM],
}

impl Analysis {
    /// `num_actual`: how many parameters the capability takes.
    #[must_use]
    pub fn actual(&self) -> usize {
        self.parsed.max(self.popped)
    }

    /// `tparm_type`: a bit for each string parameter among them.
    #[must_use]
    pub fn tparm_type(&self) -> u32 {
        (0..self.actual())
            .filter(|&n| self.is_string.get(n).copied().unwrap_or(false))
            .fold(0, |t, n| t | 1u32.wrapping_shl(n as u32))
    }
}

/// The byte at `i`, or the NUL that ends a C string.
fn at(s: &[u8], i: usize) -> u8 {
    s.get(i).copied().unwrap_or(0)
}

/// `parse_format`: the printf format a `%` introduces -- its flags, width
/// and precision as written, and the conversion -- written into `format`
/// when there is one to write it into (upstream's `fmt_buff`, which the
/// analysis passes as null); and where the conversion (or whatever stopped
/// the scan) is.
fn parse_format(s: &[u8], mut i: usize, mut format: Option<&mut Vec<u8>>) -> usize {
    let mut put = |c: u8| {
        if let Some(f) = format.as_deref_mut() {
            f.push(c);
        }
    };
    put(b'%');
    let mut done = false;
    let mut allowminus = false;
    let mut dot = false;
    let mut err = false;
    let mut value: i32 = 0;
    while at(s, i) != 0 && !done {
        let c = at(s, i);
        match c {
            b'c' | b'd' | b'o' | b'x' | b'X' | b's' => {
                put(c);
                done = true;
            }
            b'.' => {
                put(c);
                i = i.saturating_add(1);
                if dot {
                    err = true;
                } else {
                    dot = true;
                }
                value = 0;
            }
            b'#' | b' ' => {
                put(c);
                i = i.saturating_add(1);
            }
            b':' => {
                i = i.saturating_add(1);
                allowminus = true;
            }
            b'-' => {
                if allowminus {
                    put(c);
                    i = i.saturating_add(1);
                } else {
                    done = true;
                }
            }
            _ if c.is_ascii_digit() => {
                value = value
                    .wrapping_mul(10)
                    .wrapping_add(i32::from(c.wrapping_sub(b'0')));
                if value > 10000 {
                    err = true;
                }
                put(c);
                i = i.saturating_add(1);
            }
            _ => done = true,
        }
    }
    if err && let Some(f) = format {
        // "If we found an error, ignore (and remove) the flags."
        f.truncate(1);
        if at(s, i) != 0 {
            f.push(at(s, i));
        }
    }
    i
}

/// `_nc_tparm_analyze`.
#[must_use]
pub fn analyze(string: &[u8]) -> Analysis {
    let mut a = Analysis::default();
    let mut lastpop: i32 = -1;
    let mut number: usize = 0;
    let mut level: i32 = -1;
    let mut popcount: usize = 0;
    let len = string.iter().position(|&b| b == 0).unwrap_or(string.len());
    let bump = |level: i32, number: &mut usize| {
        if level < 0 && *number < 2 {
            *number = number.saturating_add(1);
        }
    };
    let mut cp = 0usize;
    while cp < len {
        if at(string, cp) == b'%' {
            cp = cp.saturating_add(1);
            cp = parse_format(string, cp, None);
            match at(string, cp) {
                b'd' | b'o' | b'x' | b'X' | b'c' => {
                    if lastpop <= 0 {
                        bump(level, &mut number);
                    }
                    level = level.saturating_sub(1);
                    lastpop = -1;
                }
                b'l' | b's' => {
                    if lastpop > 0 {
                        level = level.saturating_sub(1);
                        if let Some(slot) = usize::try_from(lastpop.wrapping_sub(1))
                            .ok()
                            .and_then(|k| a.is_string.get_mut(k))
                        {
                            *slot = true;
                        }
                    }
                    bump(level, &mut number);
                }
                b'p' => {
                    cp = cp.saturating_add(1);
                    let i = i32::from(at(string, cp)).wrapping_sub(i32::from(b'0'));
                    if (0..=9).contains(&i) {
                        level = level.saturating_add(1);
                        lastpop = i;
                        popcount = popcount.max(usize::try_from(i).unwrap_or(0));
                    }
                }
                b'P' => cp = cp.saturating_add(1),
                b'g' => {
                    level = level.saturating_add(1);
                    cp = cp.saturating_add(1);
                }
                b'\'' => {
                    level = level.saturating_add(1);
                    cp = cp.saturating_add(2);
                    lastpop = -1;
                }
                b'{' => {
                    level = level.saturating_add(1);
                    cp = cp.saturating_add(1);
                    while at(string, cp).is_ascii_digit() {
                        cp = cp.saturating_add(1);
                    }
                }
                b'+' | b'-' | b'*' | b'/' | b'm' | b'A' | b'O' | b'&' | b'|' | b'^' | b'='
                | b'<' | b'>' => {
                    bump(level, &mut number);
                    level = level.saturating_sub(1);
                    lastpop = -1;
                }
                b'!' | b'~' => {
                    bump(level, &mut number);
                    lastpop = -1;
                }
                _ => {}
            }
        }
        if at(string, cp) != 0 {
            cp = cp.saturating_add(1);
        }
    }
    a.parsed = number.min(NUM_PARM);
    a.popped = popcount.min(NUM_PARM);
    a
}

/// The digits of `v` in `base` -- 8, 10 or 16, the letters capitals when
/// `upper` -- written to the end of `buf`, which holds the most there can be
/// (eleven, in octal): the slice they fill.
fn digits(v: u32, base: u32, upper: bool, buf: &mut [u8; 11]) -> &[u8] {
    let set: &[u8; 16] = if upper {
        b"0123456789ABCDEF"
    } else {
        b"0123456789abcdef"
    };
    let mut v = v;
    let mut at = buf.len();
    loop {
        at = at.saturating_sub(1);
        let d = usize::try_from(v.checked_rem(base).unwrap_or(0)).unwrap_or(0);
        if let (Some(slot), Some(&c)) = (buf.get_mut(at), set.get(d)) {
            *slot = c;
        }
        v = v.checked_div(base).unwrap_or(0);
        if v == 0 || at == 0 {
            break;
        }
    }
    buf.get(at..).unwrap_or_default()
}

/// The `printf` of one `%d`, `%o`, `%x` or `%X` -- or of `%s` -- with the
/// flags, width and precision `parse_format` copied, as glibc prints them,
/// added to `out`. A format glibc would not take as a conversion is printed
/// as it is, as glibc prints one it does not know.
fn printf(out: &mut Vec<u8>, format: &[u8], num: i32, text: &[u8]) {
    let Some((&conv, spec)) = format.split_last() else {
        return;
    };
    let spec = spec.get(1..).unwrap_or_default();
    let (mut left, mut zero, mut space, mut alt) = (false, false, false, false);
    let mut i = 0usize;
    while let Some(&c) = spec.get(i) {
        match c {
            b'-' => left = true,
            b'0' => zero = true,
            b' ' => space = true,
            b'#' => alt = true,
            _ => break,
        }
        i = i.saturating_add(1);
    }
    let mut width = 0usize;
    while let Some(&c) = spec.get(i).filter(|c| c.is_ascii_digit()) {
        width = width
            .saturating_mul(10)
            .saturating_add(usize::from(c.wrapping_sub(b'0')));
        i = i.saturating_add(1);
    }
    let mut precision: Option<usize> = None;
    if spec.get(i) == Some(&b'.') {
        i = i.saturating_add(1);
        let mut p = 0usize;
        while let Some(&c) = spec.get(i).filter(|c| c.is_ascii_digit()) {
            p = p
                .saturating_mul(10)
                .saturating_add(usize::from(c.wrapping_sub(b'0')));
            i = i.saturating_add(1);
        }
        precision = Some(p);
    }
    if i != spec.len() {
        out.extend_from_slice(format);
        return;
    }

    // What is printed, in three parts: the sign or `0x`, the zeros a
    // precision or `%#o` asks for, and the digits -- or for `%s`, the text.
    let mut scratch = [0u8; 11];
    let (prefix, zeros, body): (&[u8], usize, &[u8]) = if conv == b's' {
        let n = precision.map_or(text.len(), |p| p.min(text.len()));
        (b"", 0, text.get(..n).unwrap_or_default())
    } else {
        let unsigned = u32::from_ne_bytes(num.to_ne_bytes());
        let mut body = match conv {
            b'd' => digits(num.unsigned_abs(), 10, false, &mut scratch),
            b'o' => digits(unsigned, 8, false, &mut scratch),
            b'x' => digits(unsigned, 16, false, &mut scratch),
            _ => digits(unsigned, 16, true, &mut scratch),
        };
        if precision == Some(0) && num == 0 {
            body = b"";
        }
        let mut zeros = precision.map_or(0, |p| p.saturating_sub(body.len()));
        // `%#o` makes the first digit a 0 when no other zero already does.
        if conv == b'o' && alt && zeros == 0 && body.first() != Some(&b'0') {
            zeros = 1;
        }
        let prefix: &[u8] = match conv {
            b'd' if num < 0 => b"-",
            b'd' if space => b" ",
            b'x' if alt && num != 0 => b"0x",
            b'X' if alt && num != 0 => b"0X",
            _ => b"",
        };
        (prefix, zeros, body)
    };

    let len = prefix
        .len()
        .saturating_add(zeros)
        .saturating_add(body.len());
    let pad = width.saturating_sub(len);
    let put = |out: &mut Vec<u8>, c: u8, n: usize| out.resize(out.len().saturating_add(n), c);
    if left {
        out.extend_from_slice(prefix);
        put(out, b'0', zeros);
        out.extend_from_slice(body);
        put(out, b' ', pad);
    } else if zero && precision.is_none() && conv != b's' {
        out.extend_from_slice(prefix);
        put(out, b'0', pad.saturating_add(zeros));
        out.extend_from_slice(body);
    } else {
        put(out, b' ', pad);
        out.extend_from_slice(prefix);
        put(out, b'0', zeros);
        out.extend_from_slice(body);
    }
}

/// The state `tparm` keeps for a terminal from one call to the next: the
/// static variables `A` to `Z`, zero when the terminal was set up -- and the
/// room its expansions are made in.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tparm {
    static_vars: [i32; NUM_VARS],
    /// `_nc_tparm_err`: the last expansion's errors -- a pop from an empty
    /// stack, a push onto a full one, or (when there was no other) items
    /// left on the stack at the end. Cleared as each expansion starts.
    pub err: u32,
    scratch: Scratch,
}

/// `out_buff` and `fmt_buff`: the result of the last expansion, and the
/// format of the conversion it was printing. Kept from call to call so that
/// they are allocated once and only grown; not state, so two `Tparm`s that
/// differ only here are equal.
#[derive(Clone, Debug, Default)]
struct Scratch {
    out: Vec<u8>,
    fmt: Vec<u8>,
}

impl PartialEq for Scratch {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl Eq for Scratch {}

/// The parameters of one call: numbers, and strings borrowed from the
/// caller, as upstream keeps pointers to them.
struct Data<'a> {
    analysis: Analysis,
    /// `param`: `TPARM_ARG`, a `long`.
    param: [i64; NUM_PARM],
    strings: [Option<&'a [u8]>; NUM_PARM],
}

impl<'a> Data<'a> {
    fn new(analysis: Analysis) -> Self {
        Self {
            analysis,
            param: [0; NUM_PARM],
            strings: [None; NUM_PARM],
        }
    }

    /// The `i`th parameter as `%p` pushes it: the string, where the
    /// analysis reads it as one and it was given; the number cut to an
    /// `int` otherwise.
    fn item(&self, i: usize) -> Item {
        let is_string = self.analysis.is_string.get(i).copied().unwrap_or(false);
        match self.strings.get(i).copied().flatten() {
            Some(_) if is_string => Item::Str(i),
            _ => Item::Num(as_int(self.param.get(i).copied().unwrap_or(0))),
        }
    }

    /// The string an item stands for: a string parameter's text, or the
    /// empty string upstream's `spop` gives for a number.
    fn text(&self, item: Option<Item>) -> &'a [u8] {
        match item {
            Some(Item::Str(i)) => self.strings.get(i).copied().flatten().unwrap_or_default(),
            Some(Item::Num(_)) | None => b"",
        }
    }
}

/// One of `tparm`'s variable arguments: a `long`, or a string pointer
/// (`None` is a null one, which upstream reads as an empty string).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arg<'a> {
    /// `TPARM_ARG`, a `long`.
    Num(i64),
    /// `char *`.
    Str(Option<&'a [u8]>),
}

/// `check_string_caps`: whether `string`, whose analysis gives string
/// parameters `tparm_type`, is one of the few capabilities that take them --
/// its value the terminal's `pfkey`, `pfloc`, `pfx` or `pln` (a number and
/// a string), `pfxl` (a number and two strings), or its own `Cs` (a string)
/// or `Ms` (two).
fn check_string_caps(entry: &Entry, string: &[u8], tparm_type: u32) -> bool {
    let is = |name: &str| match entry.tigetstr(name.as_bytes()) {
        crate::TiString::Value(v) => v == string,
        crate::TiString::Absent | crate::TiString::NotAString => false,
    };
    let mut want: u32 = 0;
    if is("pfkey") || is("pfloc") || is("pfx") || is("pln") {
        want = 2;
    } else if is("pfxl") {
        want = 6;
    } else {
        if is("Cs") {
            want = 1;
        }
        if is("Ms") {
            want = 3;
        }
    }
    want == tparm_type
}

impl Tparm {
    /// A terminal's, fresh: what `setupterm` leaves. (`tput` calls
    /// `_nc_reset_tparm (NULL)` before each capability, but a null terminal
    /// there is the no-terminal state, not the one set up: the static
    /// variables a capability sets are still set for the next.)
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `_nc_reset_tparm (term)`: the static variables cleared.
    pub fn reset(&mut self) {
        self.static_vars = [0; NUM_VARS];
    }

    /// `tiparm (string, ...)` -- numbers only, as `TIPARM_9` passes them:
    /// [`Tparm::expand`], copied for the caller to keep.
    #[must_use]
    pub fn tiparm(&mut self, entry: &Entry, string: &[u8], params: &[i64]) -> Option<Vec<u8>> {
        self.expand(entry, string, params).map(<[u8]>::to_vec)
    }

    /// `tiparm (string, ...)` -- numbers only, as `TIPARM_9` passes them --
    /// answered in the buffer this keeps, until the next expansion: as
    /// [`Tparm::tparm`] with every argument a number, and allocating nothing
    /// once the buffers have grown to the expansion (module docs).
    pub fn expand(&mut self, entry: &Entry, string: &[u8], params: &[i64]) -> Option<&[u8]> {
        self.err = 0;
        let analysis = analyze(string);
        let tparm_type = analysis.tparm_type();
        // `ValidCap (TRUE)`.
        if tparm_type != 0 && !check_string_caps(entry, string, tparm_type) {
            return None;
        }
        let mut data = Data::new(analysis);
        for k in 0..analysis.actual().min(NUM_PARM) {
            if analysis.is_string.get(k).copied().unwrap_or(false) {
                // A number where a string is read: upstream reads the one as
                // the other, which is undefined; here the string is empty.
                if let Some(slot) = data.strings.get_mut(k) {
                    *slot = Some(b"");
                }
            } else if let Some(slot) = data.param.get_mut(k) {
                *slot = params.get(k).copied().unwrap_or(0);
            }
        }
        self.tparam_internal(string, &mut data);
        Some(&self.scratch.out)
    }

    /// `tparm (string, ...)` -- the variable-argument form -- for the
    /// terminal `entry`: `None` for a capability that takes string
    /// parameters and is not one of those that may. The first
    /// `num_actual` arguments are taken, each as the analysis says: a string
    /// where `%s` or `%l` reads one, a `long` elsewhere.
    #[must_use]
    pub fn tparm(&mut self, entry: &Entry, string: &[u8], args: &[Arg<'_>]) -> Option<Vec<u8>> {
        self.err = 0;
        let analysis = analyze(string);
        let tparm_type = analysis.tparm_type();
        // `ValidCap (TRUE)`.
        if tparm_type != 0 && !check_string_caps(entry, string, tparm_type) {
            return None;
        }
        let mut data = Data::new(analysis);
        // `tparm_copy_valist (&myData, TRUE, ap)`.
        for k in 0..analysis.actual().min(NUM_PARM) {
            let wants_string = analysis.is_string.get(k).copied().unwrap_or(false);
            match (wants_string, args.get(k)) {
                (true, Some(Arg::Str(s))) => {
                    if let Some(slot) = data.strings.get_mut(k) {
                        *slot = Some(s.unwrap_or_default());
                    }
                }
                (false, Some(Arg::Num(n))) => {
                    if let Some(slot) = data.param.get_mut(k) {
                        *slot = *n;
                    }
                }
                // A number where a string is read, or the reverse: upstream
                // reads the one as the other, which is undefined; here a
                // string is empty and a number 0. Past the arguments given,
                // the same.
                (true, _) => {
                    if let Some(slot) = data.strings.get_mut(k) {
                        *slot = Some(b"");
                    }
                }
                (false, _) => {}
            }
        }
        self.tparam_internal(string, &mut data);
        Some(self.scratch.out.clone())
    }

    /// `_nc_tiparm (expected, string, ...)` for the terminal `entry`, with
    /// numeric parameters: [`Tparm::nc_expand`], copied for the caller to
    /// keep.
    #[must_use]
    pub fn nc_tiparm(
        &mut self,
        entry: &Entry,
        expected: usize,
        string: &[u8],
        params: &[i32],
    ) -> Option<Vec<u8>> {
        self.nc_expand(entry, expected, string, params)
            .map(<[u8]>::to_vec)
    }

    /// `_nc_tiparm (expected, string, ...)` for the terminal `entry`, with
    /// numeric parameters, answered in the buffer this keeps until the next
    /// expansion: `None` where upstream returns a null pointer -- a
    /// capability with string parameters, or one that takes no parameters,
    /// or more than `expected`, or (unless `expected` is 9, for `sgr`) a
    /// different number.
    pub fn nc_expand(
        &mut self,
        entry: &Entry,
        expected: usize,
        string: &[u8],
        params: &[i32],
    ) -> Option<&[u8]> {
        self.err = 0;
        let analysis = analyze(string);
        // `ValidCap (FALSE)`: numbers only.
        if analysis.tparm_type() != 0 {
            return None;
        }
        let actual = analysis.actual();
        let mut expected = expected;
        if actual != expected {
            let same = |s: Option<&[u8]>| s.is_some_and(|s| s == string);
            let mut needed = expected;
            if same(entry.string(cap::TO_STATUS_LINE))
                || same(entry.string(cap::SET_A_BACKGROUND))
                || same(entry.string(cap::SET_A_FOREGROUND))
                || same(entry.string(cap::SET_BACKGROUND))
                || same(entry.string(cap::SET_FOREGROUND))
            {
                needed = 0;
            } else {
                if same(entry.ext_string(b"xm")) {
                    needed = 3;
                }
                if same(entry.ext_string(b"S0")) {
                    needed = 0;
                }
            }
            if actual >= needed && actual <= expected {
                expected = actual;
            }
        }
        if (actual == 0 && expected != 0)
            || actual > expected
            || (expected != NUM_PARM && actual != expected)
        {
            return None;
        }
        let mut data = Data::new(analysis);
        for (k, slot) in data.param.iter_mut().enumerate().take(actual) {
            *slot = i64::from(params.get(k).copied().unwrap_or(0));
        }
        self.tparam_internal(string, &mut data);
        Some(&self.scratch.out)
    }

    /// `tparam_internal`: the expansion, into the output buffer.
    #[allow(
        clippy::too_many_lines,
        reason = "upstream's tparam_internal, in one piece so it reads against it"
    )]
    fn tparam_internal(&mut self, string: &[u8], data: &mut Data<'_>) {
        let Self {
            static_vars,
            err,
            scratch: Scratch { out, fmt },
        } = self;
        out.clear();
        let mut stack = Stack::default();
        // `(char) ((c == 0) ? 0200 : c)`.
        let save_char = |out: &mut Vec<u8>, c: i32| {
            out.push(if c == 0 {
                0o200
            } else {
                c.to_le_bytes().first().copied().unwrap_or(0)
            });
        };

        // `tparm_tc_compat`: with no `%p` at all, the parameters go on the
        // stack so that successive pops take them in order.
        let termcap_hack = data.analysis.popped == 0;
        if termcap_hack {
            for i in (0..data.analysis.parsed).rev() {
                stack.push(data.item(i));
            }
        }
        let mut dynamic: Option<[i32; NUM_VARS]> = None;
        let mut incremented_two = false;
        let len = string.iter().position(|&b| b == 0).unwrap_or(string.len());

        let mut cp = 0usize;
        while cp < len {
            if at(string, cp) == b'%' {
                cp = cp.saturating_add(1);
                fmt.clear();
                cp = parse_format(string, cp, Some(&mut *fmt));
                match at(string, cp) {
                    b'%' => save_char(out, i32::from(b'%')),
                    b'd' | b'o' | b'x' | b'X' => {
                        let x = stack.npop();
                        printf(out, fmt, x, b"");
                    }
                    b'c' => {
                        let x = stack.npop();
                        save_char(out, x);
                    }
                    b'l' => {
                        let s = data.text(stack.spop());
                        stack.push(Item::Num(i32::try_from(s.len()).unwrap_or(i32::MAX)));
                    }
                    b's' => {
                        let s = data.text(stack.spop());
                        printf(out, fmt, 0, s);
                    }
                    b'p' => {
                        cp = cp.saturating_add(1);
                        let i = i32::from(at(string, cp)).wrapping_sub(i32::from(b'1'));
                        if let Ok(i) = usize::try_from(i)
                            && i < NUM_PARM
                        {
                            stack.push(data.item(i));
                        }
                    }
                    b'P' => {
                        cp = cp.saturating_add(1);
                        let c = at(string, cp);
                        if c.is_ascii_uppercase() {
                            let x = stack.npop();
                            if let Some(v) = static_vars.get_mut(usize::from(c.wrapping_sub(b'A')))
                            {
                                *v = x;
                            }
                        } else if c.is_ascii_lowercase() {
                            let x = stack.npop();
                            let vars = dynamic.get_or_insert([0; NUM_VARS]);
                            if let Some(v) = vars.get_mut(usize::from(c.wrapping_sub(b'a'))) {
                                *v = x;
                            }
                        }
                    }
                    b'g' => {
                        cp = cp.saturating_add(1);
                        let c = at(string, cp);
                        if c.is_ascii_uppercase() {
                            let v = static_vars
                                .get(usize::from(c.wrapping_sub(b'A')))
                                .copied()
                                .unwrap_or(0);
                            stack.push(Item::Num(v));
                        } else if c.is_ascii_lowercase() {
                            let vars = dynamic.get_or_insert([0; NUM_VARS]);
                            let v = vars
                                .get(usize::from(c.wrapping_sub(b'a')))
                                .copied()
                                .unwrap_or(0);
                            stack.push(Item::Num(v));
                        }
                    }
                    b'\'' => {
                        cp = cp.saturating_add(1);
                        stack.push(Item::Num(i32::from(at(string, cp))));
                        cp = cp.saturating_add(1);
                    }
                    b'{' => {
                        let mut number: i32 = 0;
                        cp = cp.saturating_add(1);
                        while at(string, cp).is_ascii_digit() {
                            number = number
                                .wrapping_mul(10)
                                .wrapping_add(i32::from(at(string, cp).wrapping_sub(b'0')));
                            cp = cp.saturating_add(1);
                        }
                        stack.push(Item::Num(number));
                    }
                    op @ (b'+' | b'-' | b'*' | b'/' | b'm' | b'A' | b'O' | b'&' | b'|' | b'^'
                    | b'=' | b'<' | b'>') => {
                        let y = stack.npop();
                        let x = stack.npop();
                        let r = match op {
                            b'+' => x.wrapping_add(y),
                            b'-' => x.wrapping_sub(y),
                            b'*' => x.wrapping_mul(y),
                            // `y ? (x / y) : 0`; the one quotient that
                            // overflows, INT_MIN / -1, wraps (upstream's traps).
                            b'/' => match x.checked_div(y) {
                                Some(q) => q,
                                None if y == 0 => 0,
                                None => x.wrapping_neg(),
                            },
                            b'm' => x.checked_rem(y).unwrap_or(0),
                            b'A' => i32::from(y != 0 && x != 0),
                            b'O' => i32::from(y != 0 || x != 0),
                            b'&' => x & y,
                            b'|' => x | y,
                            b'^' => x ^ y,
                            b'=' => i32::from(x == y),
                            b'<' => i32::from(x < y),
                            _ => i32::from(x > y),
                        };
                        stack.push(Item::Num(r));
                    }
                    b'!' => {
                        let x = stack.npop();
                        stack.push(Item::Num(i32::from(x == 0)));
                    }
                    b'~' => {
                        let x = stack.npop();
                        stack.push(Item::Num(!x));
                    }
                    b'i' => {
                        // The first two parameters, if numbers, once; with
                        // the termcap hack the stack's bottom two slots are
                        // rewritten too, as upstream rewrites them.
                        if !incremented_two {
                            incremented_two = true;
                            for k in 0..2 {
                                if !data.analysis.is_string.get(k).copied().unwrap_or(false)
                                    && let Some(p) = data.param.get_mut(k)
                                {
                                    *p = p.wrapping_add(1);
                                    let v = as_int(*p);
                                    if termcap_hack {
                                        stack.rewrite(k, v);
                                    }
                                }
                            }
                        }
                    }
                    b't' => {
                        let x = stack.npop();
                        if x == 0 {
                            // Forward to the `%e` or `%;` at this level.
                            cp = cp.saturating_add(1);
                            let mut level: u32 = 0;
                            while at(string, cp) != 0 {
                                if at(string, cp) == b'%' {
                                    cp = cp.saturating_add(1);
                                    match at(string, cp) {
                                        b'?' => level = level.saturating_add(1),
                                        b';' => {
                                            if level > 0 {
                                                level = level.saturating_sub(1);
                                            } else {
                                                break;
                                            }
                                        }
                                        b'e' if level == 0 => break,
                                        _ => {}
                                    }
                                }
                                if at(string, cp) != 0 {
                                    cp = cp.saturating_add(1);
                                }
                            }
                        }
                    }
                    b'e' => {
                        // Forward to the `%;` at this level.
                        cp = cp.saturating_add(1);
                        let mut level: u32 = 0;
                        while at(string, cp) != 0 {
                            if at(string, cp) == b'%' {
                                cp = cp.saturating_add(1);
                                if at(string, cp) == b'?' {
                                    level = level.saturating_add(1);
                                } else if at(string, cp) == b';' {
                                    if level > 0 {
                                        level = level.saturating_sub(1);
                                    } else {
                                        break;
                                    }
                                }
                            }
                            if at(string, cp) != 0 {
                                cp = cp.saturating_add(1);
                            }
                        }
                    }
                    _ => {}
                }
            } else {
                save_char(out, i32::from(at(string, cp)));
            }
            if at(string, cp) == 0 {
                break;
            }
            cp = cp.saturating_add(1);
        }
        // "tparm: stack has items on return" counts when nothing else did.
        if stack.len != 0 && stack.err == 0 {
            stack.err = 1;
        }
        *err = err.saturating_add(stack.err);
        // The result is a C string.
        if let Some(nul) = out.iter().position(|&b| b == 0) {
            out.truncate(nul);
        }
    }
}

/// `TPS (stack)`: the stack -- twenty slots, as upstream's -- and the
/// errors `npush`, `npop` and the others count into `_nc_tparm_err`.
struct Stack {
    items: [Item; STACKSIZE],
    len: usize,
    err: u32,
}

impl Default for Stack {
    fn default() -> Self {
        Self {
            items: [Item::Num(0); STACKSIZE],
            len: 0,
            err: 0,
        }
    }
}

impl Stack {
    /// `npush`, `spush`: onto the stack, or an error when it is full.
    fn push(&mut self, item: Item) {
        match self.items.get_mut(self.len) {
            Some(slot) => {
                *slot = item;
                self.len = self.len.saturating_add(1);
            }
            None => self.err = self.err.saturating_add(1),
        }
    }

    /// The top item, taken; an error and nothing when the stack is empty.
    fn pop(&mut self) -> Option<Item> {
        if self.len == 0 {
            self.err = self.err.saturating_add(1);
            return None;
        }
        self.len = self.len.saturating_sub(1);
        self.items.get(self.len).copied()
    }

    /// `npop`: a number -- 0 for a string, and for an empty stack, which
    /// is also an error.
    fn npop(&mut self) -> i32 {
        match self.pop() {
            Some(Item::Num(n)) => n,
            Some(Item::Str(_)) | None => 0,
        }
    }

    /// `spop`: the item a string is read from -- see [`Data::text`] -- and
    /// an error when the stack is empty.
    fn spop(&mut self) -> Option<Item> {
        self.pop()
    }

    /// The `k`th item from the bottom, made the number `v` if it is one:
    /// `%i` on the parameters the termcap hack pushed.
    fn rewrite(&mut self, k: usize, v: i32) {
        if k < self.len
            && let Some(Item::Num(slot)) = self.items.get_mut(k)
        {
            *slot = v;
        }
    }
}

/// `(int) param`: a `long` parameter cut to an `int`, as C cuts it.
fn as_int(v: i64) -> i32 {
    i32::from_le_bytes(
        v.to_le_bytes()
            .get(..4)
            .and_then(|b| <[u8; 4]>::try_from(b).ok())
            .unwrap_or([0; 4]),
    )
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
mod tests {
    use super::*;

    /// xterm-256color's `sgr`.
    const XTERM_SGR: &[u8] = b"%?%p9%t\x1b(0%e\x1b(B%;\x1b[0%?%p6%t;1%;%?%p5%t;2%;%?%p2%t;4%;%?%p1%p3%|%t;7%;%?%p4%t;5%;%?%p7%t;8%;m";

    fn expand(string: &[u8], params: &[i32]) -> Option<Vec<u8>> {
        Tparm::new().nc_tiparm(&Entry::default(), 9, string, params)
    }

    #[test]
    fn xterm_sgr_expands_as_ncurses_expands_it() {
        assert_eq!(expand(XTERM_SGR, &[0; 9]).unwrap(), b"\x1b(B\x1b[0m");
        assert_eq!(
            expand(XTERM_SGR, &[0, 0, 0, 0, 0, 0, 0, 0, 1]).unwrap(),
            b"\x1b(0\x1b[0m"
        );
        assert_eq!(
            expand(XTERM_SGR, &[1, 1, 0, 0, 0, 1, 0, 0, 0]).unwrap(),
            b"\x1b(B\x1b[0;1;4;7m"
        );
    }

    #[test]
    fn analysis_counts_parameters_and_strings() {
        let a = analyze(XTERM_SGR);
        assert_eq!(a.popped, 9);
        assert_eq!(a.actual(), 9);
        assert_eq!(a.tparm_type(), 0);
        let a = analyze(b"%p1%d;%p2%s");
        assert_eq!(a.popped, 2);
        assert_eq!(a.tparm_type(), 0b10);
        // Termcap style: no %p, two pops.
        let a = analyze(b"\x1b[%i%d;%dH");
        assert_eq!((a.popped, a.parsed), (0, 2));
    }

    /// The expansion of `s` with numbers `p`, by a `Tparm` of its own.
    fn run(tp: &mut Tparm, s: &[u8], p: &[i32]) -> Vec<u8> {
        let mut data = Data::new(analyze(s));
        for (k, v) in p.iter().enumerate() {
            data.param[k] = i64::from(*v);
        }
        tp.tparam_internal(s, &mut data);
        tp.scratch.out.clone()
    }

    /// `printf`'s answer, alone.
    fn print(format: &[u8], num: i32, text: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        printf(&mut out, format, num, text);
        out
    }

    #[test]
    fn arithmetic_variables_and_conditions() {
        let t = |s: &[u8], p: &[i32]| -> Vec<u8> { run(&mut Tparm::new(), s, p) };
        assert_eq!(t(b"%p1%p2%+%d", &[3, 4]), b"7");
        assert_eq!(t(b"%p1%{10}%/%d", &[42]), b"4");
        assert_eq!(t(b"%p1%{0}%/%d", &[42]), b"0");
        assert_eq!(t(b"%p1%{10}%m%d", &[42]), b"2");
        assert_eq!(t(b"%p1%Pa%ga%ga%*%d", &[6]), b"36");
        assert_eq!(t(b"%?%p1%{2}%>%tbig%esmall%;", &[3]), b"big");
        assert_eq!(t(b"%?%p1%{2}%>%tbig%esmall%;", &[1]), b"small");
        assert_eq!(t(b"%'A'%c%p1%c", &[0]), b"A\x80");
        // `%-` without `:` is subtraction: 0 - 5 is left on the stack, and
        // `02d` is text.
        assert_eq!(
            t(b"%p1%2d|%p1%-02d|%p1%:-3d|%p1%03x|%p1%#x|%p1%#o", &[5]),
            b" 5|02d|5  |005|0x5|05"
        );
        assert_eq!(t(b"%p1%{256}%+%c!", &[0]), b"");
        // An empty stack pops 0.
        assert_eq!(t(b"%d", &[]), b"0");
        // Termcap style: %i bumps the first two, the pops take them in turn.
        assert_eq!(t(b"\x1b[%i%d;%dH", &[4, 9]), b"\x1b[10;5H");
        // Static variables outlive the call; dynamic ones do not.
        let mut tp = Tparm::new();
        run(&mut tp, b"%{7}%PA", &[]);
        assert_eq!(run(&mut tp, b"%gA%d", &[]), b"7");
    }

    #[test]
    fn printf_follows_glibc() {
        assert_eq!(print(b"%d", -42, b""), b"-42");
        assert_eq!(print(b"% d", 42, b""), b" 42");
        assert_eq!(print(b"%.0d", 0, b""), b"");
        assert_eq!(print(b"%.3d", -7, b""), b"-007");
        assert_eq!(print(b"%05d", -7, b""), b"-0007");
        assert_eq!(print(b"%x", -1, b""), b"ffffffff");
        assert_eq!(print(b"%#X", 255, b""), b"0XFF");
        assert_eq!(print(b"%#x", 0, b""), b"0");
        assert_eq!(print(b"%#.0o", 0, b""), b"0");
        assert_eq!(print(b"%-4s", 0, b"ab"), b"ab  ");
        assert_eq!(print(b"%.1s", 0, b"ab"), b"a");
        assert_eq!(print(b"%5#x", 1, b""), b"%5#x");
        // The extremes of each base, and the flags against each other.
        assert_eq!(print(b"%o", -1, b""), b"37777777777");
        assert_eq!(print(b"%d", i32::MIN, b""), b"-2147483648");
        assert_eq!(print(b"%#o", 8, b""), b"010");
        assert_eq!(print(b"%#05o", 8, b""), b"00010");
        assert_eq!(print(b"%#.4o", 8, b""), b"0010");
        assert_eq!(print(b"%-6.3d", -5, b""), b"-005  ");
        assert_eq!(print(b"%06.3d", 5, b""), b"   005");
        assert_eq!(print(b"%#8X", 0xbeef, b""), b"  0XBEEF");
        assert_eq!(print(b"%-#8x", 0xbeef, b""), b"0xbeef  ");
    }

    /// Strings pushed by `%p` are the caller's, read by `%s` and measured by
    /// `%l`; a number popped as a string is empty.
    #[test]
    fn string_parameters_are_read_where_they_lie() {
        let e = Entry::default();
        let mut tp = Tparm::new();
        assert_eq!(
            tp.tparm(&e, b"%p1%s=%p1%l%d", &[Arg::Str(Some(b"abc"))]),
            None,
            "a capability with string parameters must be one that may"
        );
        let mut data = Data::new(analyze(b"[%p1%s|%p1%l%d|%p2%s]"));
        data.strings[0] = Some(b"abc");
        data.param[1] = 7;
        tp.tparam_internal(b"[%p1%s|%p1%l%d|%p2%s]", &mut data);
        assert_eq!(tp.scratch.out, b"[abc|3|]");
    }

    /// Once its buffers have grown, an expansion reuses them: the second
    /// run of a capability asks the allocator for nothing.
    #[test]
    fn a_warm_expansion_reuses_its_buffers() {
        let e = Entry::default();
        let mut tp = Tparm::new();
        let first = tp
            .expand(&e, XTERM_SGR, &[1, 1, 0, 0, 0, 1, 0, 0, 0])
            .map(<[u8]>::as_ptr);
        let again = tp
            .expand(&e, XTERM_SGR, &[0, 0, 1, 0, 0, 0, 0, 0, 1])
            .map(<[u8]>::as_ptr);
        assert_eq!(first, again, "the same buffer, not a new one");
        assert_eq!(
            tp.expand(&e, XTERM_SGR, &[1, 1, 0, 0, 0, 1, 0, 0, 0]),
            Some(&b"\x1b(B\x1b[0;1;4;7m"[..])
        );
        // The stack is fixed: twenty-one pushes overflow it, as upstream's.
        let full = [b"%{1}".as_slice(); 21].concat();
        assert_eq!(tp.expand(&e, &full, &[]), Some(&b""[..]));
        assert_eq!(
            tp.err, 1,
            "one push too many; the items left count only when nothing else did"
        );
    }

    #[test]
    fn nc_tiparm_checks_the_count() {
        let e = Entry::default();
        // No parameters at all, though up to 9 were expected.
        assert_eq!(Tparm::new().nc_tiparm(&e, 9, b"\x1b[m", &[]), None);
        // Two expected, one taken.
        assert_eq!(Tparm::new().nc_tiparm(&e, 2, b"%p1%d", &[1]), None);
        assert_eq!(
            Tparm::new().nc_tiparm(&e, 1, b"%p1%d", &[12]),
            Some(b"12".to_vec())
        );
        // A string parameter is refused.
        assert_eq!(Tparm::new().nc_tiparm(&e, 9, b"%p1%s", &[1]), None);
    }
}
