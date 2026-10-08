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

use crate::Entry;
use crate::string as cap;

/// `STACKSIZE`.
const STACKSIZE: usize = 20;
/// `NUM_PARM`.
pub const NUM_PARM: usize = 9;
/// `NUM_VARS`.
const NUM_VARS: usize = 26;

/// One slot of the stack.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Item {
    Num(i32),
    Str(Vec<u8>),
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
/// and precision as written, and the conversion -- and where the
/// conversion (or whatever stopped the scan) is.
fn parse_format(s: &[u8], mut i: usize) -> (Vec<u8>, usize) {
    let mut format = vec![b'%'];
    let mut done = false;
    let mut allowminus = false;
    let mut dot = false;
    let mut err = false;
    let mut value: i32 = 0;
    while at(s, i) != 0 && !done {
        let c = at(s, i);
        match c {
            b'c' | b'd' | b'o' | b'x' | b'X' | b's' => {
                format.push(c);
                done = true;
            }
            b'.' => {
                format.push(c);
                i = i.saturating_add(1);
                if dot {
                    err = true;
                } else {
                    dot = true;
                }
                value = 0;
            }
            b'#' | b' ' => {
                format.push(c);
                i = i.saturating_add(1);
            }
            b':' => {
                i = i.saturating_add(1);
                allowminus = true;
            }
            b'-' => {
                if allowminus {
                    format.push(c);
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
                format.push(c);
                i = i.saturating_add(1);
            }
            _ => done = true,
        }
    }
    if err {
        // "If we found an error, ignore (and remove) the flags."
        format.truncate(1);
        if at(s, i) != 0 {
            format.push(at(s, i));
        }
    }
    (format, i)
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
            cp = parse_format(string, cp).1;
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

/// The `printf` of one `%d`, `%o`, `%x` or `%X` -- or of `%s` -- with the
/// flags, width and precision `parse_format` copied, as glibc prints them.
/// A format glibc would not take as a conversion is printed as it is, as
/// glibc prints one it does not know.
fn printf(format: &[u8], num: i32, text: &[u8]) -> Vec<u8> {
    let Some((&conv, spec)) = format.split_last() else {
        return Vec::new();
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
        return format.to_vec();
    }

    let (prefix, body): (Vec<u8>, Vec<u8>) = if conv == b's' {
        let n = precision.map_or(text.len(), |p| p.min(text.len()));
        (Vec::new(), text.get(..n).unwrap_or_default().to_vec())
    } else {
        let unsigned = u32::from_ne_bytes(num.to_ne_bytes());
        let mut digits = match conv {
            b'd' => num.unsigned_abs().to_string().into_bytes(),
            b'o' => format!("{unsigned:o}").into_bytes(),
            b'x' => format!("{unsigned:x}").into_bytes(),
            _ => format!("{unsigned:X}").into_bytes(),
        };
        if precision == Some(0) && num == 0 {
            digits.clear();
        }
        if let Some(p) = precision
            && digits.len() < p
        {
            let mut padded = vec![b'0'; p.saturating_sub(digits.len())];
            padded.extend_from_slice(&digits);
            digits = padded;
        }
        let mut prefix = Vec::new();
        match conv {
            b'd' if num < 0 => prefix.push(b'-'),
            b'd' if space => prefix.push(b' '),
            b'o' if alt && digits.first() != Some(&b'0') => digits.insert(0, b'0'),
            b'x' if alt && num != 0 => prefix.extend_from_slice(b"0x"),
            b'X' if alt && num != 0 => prefix.extend_from_slice(b"0X"),
            _ => {}
        }
        (prefix, digits)
    };

    let len = prefix.len().saturating_add(body.len());
    let pad = width.saturating_sub(len);
    let mut out = Vec::with_capacity(len.saturating_add(pad));
    if left {
        out.extend_from_slice(&prefix);
        out.extend_from_slice(&body);
        out.resize(out.len().saturating_add(pad), b' ');
    } else if zero && precision.is_none() && conv != b's' {
        out.extend_from_slice(&prefix);
        out.resize(out.len().saturating_add(pad), b'0');
        out.extend_from_slice(&body);
    } else {
        out.resize(pad, b' ');
        out.extend_from_slice(&prefix);
        out.extend_from_slice(&body);
    }
    out
}

/// The state `tparm` keeps for a terminal from one call to the next: the
/// static variables `A` to `Z`, zero when the terminal was set up.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tparm {
    static_vars: [i32; NUM_VARS],
}

/// The parameters of one call.
struct Data {
    analysis: Analysis,
    /// `param`: `TPARM_ARG`, a `long`.
    param: [i64; NUM_PARM],
    strings: [Option<Vec<u8>>; NUM_PARM],
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

    /// `tparm (string, ...)` -- the variable-argument form -- for the
    /// terminal `entry`: `None` for a capability that takes string
    /// parameters and is not one of those that may. The first
    /// `num_actual` arguments are taken, each as the analysis says: a string
    /// where `%s` or `%l` reads one, a `long` elsewhere.
    #[must_use]
    pub fn tparm(&mut self, entry: &Entry, string: &[u8], args: &[Arg<'_>]) -> Option<Vec<u8>> {
        let analysis = analyze(string);
        let tparm_type = analysis.tparm_type();
        // `ValidCap (TRUE)`.
        if tparm_type != 0 && !check_string_caps(entry, string, tparm_type) {
            return None;
        }
        let mut data = Data {
            analysis,
            param: [0; NUM_PARM],
            strings: Default::default(),
        };
        // `tparm_copy_valist (&myData, TRUE, ap)`.
        for k in 0..analysis.actual().min(NUM_PARM) {
            let wants_string = analysis.is_string.get(k).copied().unwrap_or(false);
            match (wants_string, args.get(k)) {
                (true, Some(Arg::Str(s))) => {
                    if let Some(slot) = data.strings.get_mut(k) {
                        *slot = Some(s.unwrap_or_default().to_vec());
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
                        *slot = Some(Vec::new());
                    }
                }
                (false, _) => {}
            }
        }
        Some(self.tparam_internal(string, &mut data))
    }

    /// `_nc_tiparm (expected, string, ...)` for the terminal `entry`, with
    /// numeric parameters: `None` where upstream returns a null pointer --
    /// a capability with string parameters, or one that takes no
    /// parameters, or more than `expected`, or (unless `expected` is 9, for
    /// `sgr`) a different number.
    #[must_use]
    pub fn nc_tiparm(
        &mut self,
        entry: &Entry,
        expected: usize,
        string: &[u8],
        params: &[i32],
    ) -> Option<Vec<u8>> {
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
        let mut data = Data {
            analysis,
            param: [0; NUM_PARM],
            strings: Default::default(),
        };
        for (k, slot) in data.param.iter_mut().enumerate().take(actual) {
            *slot = i64::from(params.get(k).copied().unwrap_or(0));
        }
        Some(self.tparam_internal(string, &mut data))
    }

    /// `tparam_internal`: the expansion.
    #[allow(
        clippy::too_many_lines,
        reason = "upstream's tparam_internal, in one piece so it reads against it"
    )]
    fn tparam_internal(&mut self, string: &[u8], data: &mut Data) -> Vec<u8> {
        let mut out: Vec<u8> = Vec::new();
        let mut stack: Vec<Item> = Vec::new();
        let push = |stack: &mut Vec<Item>, item: Item| {
            if stack.len() < STACKSIZE {
                stack.push(item);
            }
        };
        let npop = |stack: &mut Vec<Item>| match stack.pop() {
            Some(Item::Num(n)) => n,
            _ => 0,
        };
        let spop = |stack: &mut Vec<Item>| match stack.pop() {
            Some(Item::Str(s)) => s,
            _ => Vec::new(),
        };
        // `(char) ((c == 0) ? 0200 : c)`.
        let save_char = |out: &mut Vec<u8>, c: i32| {
            out.push(if c == 0 {
                0o200
            } else {
                c.to_le_bytes().first().copied().unwrap_or(0)
            });
        };
        let param_item = |data: &Data, i: usize| -> Item {
            match data.strings.get(i).cloned().flatten() {
                Some(s) if data.analysis.is_string.get(i).copied().unwrap_or(false) => Item::Str(s),
                _ => Item::Num(as_int(data.param.get(i).copied().unwrap_or(0))),
            }
        };

        // `tparm_tc_compat`: with no `%p` at all, the parameters go on the
        // stack so that successive pops take them in order.
        let termcap_hack = data.analysis.popped == 0;
        if termcap_hack {
            for i in (0..data.analysis.parsed).rev() {
                let item = param_item(data, i);
                push(&mut stack, item);
            }
        }
        let mut dynamic: Option<[i32; NUM_VARS]> = None;
        let mut incremented_two = false;
        let len = string.iter().position(|&b| b == 0).unwrap_or(string.len());

        let mut cp = 0usize;
        while cp < len {
            if at(string, cp) == b'%' {
                cp = cp.saturating_add(1);
                let (format, next) = parse_format(string, cp);
                cp = next;
                match at(string, cp) {
                    b'%' => save_char(&mut out, i32::from(b'%')),
                    b'd' | b'o' | b'x' | b'X' => {
                        let x = npop(&mut stack);
                        out.extend_from_slice(&printf(&format, x, b""));
                    }
                    b'c' => {
                        let x = npop(&mut stack);
                        save_char(&mut out, x);
                    }
                    b'l' => {
                        let s = spop(&mut stack);
                        push(
                            &mut stack,
                            Item::Num(i32::try_from(s.len()).unwrap_or(i32::MAX)),
                        );
                    }
                    b's' => {
                        let s = spop(&mut stack);
                        out.extend_from_slice(&printf(&format, 0, &s));
                    }
                    b'p' => {
                        cp = cp.saturating_add(1);
                        let i = i32::from(at(string, cp)).wrapping_sub(i32::from(b'1'));
                        if let Ok(i) = usize::try_from(i)
                            && i < NUM_PARM
                        {
                            let item = param_item(data, i);
                            push(&mut stack, item);
                        }
                    }
                    b'P' => {
                        cp = cp.saturating_add(1);
                        let c = at(string, cp);
                        if c.is_ascii_uppercase() {
                            let x = npop(&mut stack);
                            if let Some(v) =
                                self.static_vars.get_mut(usize::from(c.wrapping_sub(b'A')))
                            {
                                *v = x;
                            }
                        } else if c.is_ascii_lowercase() {
                            let x = npop(&mut stack);
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
                            let v = self
                                .static_vars
                                .get(usize::from(c.wrapping_sub(b'A')))
                                .copied()
                                .unwrap_or(0);
                            push(&mut stack, Item::Num(v));
                        } else if c.is_ascii_lowercase() {
                            let vars = dynamic.get_or_insert([0; NUM_VARS]);
                            let v = vars
                                .get(usize::from(c.wrapping_sub(b'a')))
                                .copied()
                                .unwrap_or(0);
                            push(&mut stack, Item::Num(v));
                        }
                    }
                    b'\'' => {
                        cp = cp.saturating_add(1);
                        push(&mut stack, Item::Num(i32::from(at(string, cp))));
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
                        push(&mut stack, Item::Num(number));
                    }
                    op @ (b'+' | b'-' | b'*' | b'/' | b'm' | b'A' | b'O' | b'&' | b'|' | b'^'
                    | b'=' | b'<' | b'>') => {
                        let y = npop(&mut stack);
                        let x = npop(&mut stack);
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
                        push(&mut stack, Item::Num(r));
                    }
                    b'!' => {
                        let x = npop(&mut stack);
                        push(&mut stack, Item::Num(i32::from(x == 0)));
                    }
                    b'~' => {
                        let x = npop(&mut stack);
                        push(&mut stack, Item::Num(!x));
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
                                    if termcap_hack && let Some(Item::Num(slot)) = stack.get_mut(k)
                                    {
                                        *slot = v;
                                    }
                                }
                            }
                        }
                    }
                    b't' => {
                        let x = npop(&mut stack);
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
                save_char(&mut out, i32::from(at(string, cp)));
            }
            if at(string, cp) == 0 {
                break;
            }
            cp = cp.saturating_add(1);
        }
        // The result is a C string.
        if let Some(nul) = out.iter().position(|&b| b == 0) {
            out.truncate(nul);
        }
        out
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

    #[test]
    fn arithmetic_variables_and_conditions() {
        let t = |s: &[u8], p: &[i32]| -> Vec<u8> {
            let mut tp = Tparm::new();
            let mut data = Data {
                analysis: analyze(s),
                param: [0; NUM_PARM],
                strings: Default::default(),
            };
            for (k, v) in p.iter().enumerate() {
                data.param[k] = i64::from(*v);
            }
            tp.tparam_internal(s, &mut data)
        };
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
        let s = b"%{7}%PA";
        let mut d = Data {
            analysis: analyze(s),
            param: [0; NUM_PARM],
            strings: Default::default(),
        };
        tp.tparam_internal(s, &mut d);
        let s = b"%gA%d";
        let mut d = Data {
            analysis: analyze(s),
            param: [0; NUM_PARM],
            strings: Default::default(),
        };
        assert_eq!(tp.tparam_internal(s, &mut d), b"7");
    }

    #[test]
    fn printf_follows_glibc() {
        assert_eq!(printf(b"%d", -42, b""), b"-42");
        assert_eq!(printf(b"% d", 42, b""), b" 42");
        assert_eq!(printf(b"%.0d", 0, b""), b"");
        assert_eq!(printf(b"%.3d", -7, b""), b"-007");
        assert_eq!(printf(b"%05d", -7, b""), b"-0007");
        assert_eq!(printf(b"%x", -1, b""), b"ffffffff");
        assert_eq!(printf(b"%#X", 255, b""), b"0XFF");
        assert_eq!(printf(b"%#x", 0, b""), b"0");
        assert_eq!(printf(b"%#.0o", 0, b""), b"0");
        assert_eq!(printf(b"%-4s", 0, b"ab"), b"ab  ");
        assert_eq!(printf(b"%.1s", 0, b"ab"), b"a");
        assert_eq!(printf(b"%5#x", 1, b""), b"%5#x");
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
