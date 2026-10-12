//! `captoinfo.c`: a string capability translated between termcap's
//! notation and terminfo's -- `_nc_captoinfo` as a termcap source is
//! compiled, `_nc_infotocap` as an entry is written out as termcap.
//!
//! Termcap writes parameters with `%` codes that work on an implicit
//! "current parameter" (`%d` prints it and moves on, `%+x` adds `x` and
//! prints it as a character, `%r` swaps the first two, `%i` adds one to
//! both); terminfo pushes each parameter explicitly (`%p1%d`). The
//! translation is Ross Ridge's from mytinfo, as ncurses keeps it -- and,
//! going the other way, recognises the terminfo idioms that termcap can
//! say, by `sscanf` against templates.
//!
//! Upstream's quirks are kept, because what `tic -C` and `infocmp -C`
//! print depends on them:
//!
//! - A `sscanf` template matches as far as its last conversion: the text
//!   after it need not be there. And a failed match still stores what it
//!   converted before failing, which later templates' tests can see.
//! - `_nc_infotocap`'s `ch1` and `ch2` keep their values from one step to
//!   the next, so a character read by an earlier step can stand in for a
//!   number read by a later one.
//! - Padding at the end (`$<5>`) is looked for by walking back from the
//!   last character, and the walk does what it does even where the string
//!   only looks like padding.

use super::scan::{Scanner, cstr, unctrl};

/// `MAX_PUSHED`: how deep `_nc_captoinfo`'s stack goes.
const MAX_PUSHED: usize = 16;
/// `MAX_TC_FIXUPS`.
const MAX_TC_FIXUPS: usize = 10;
/// `MIN_TC_FIXUPS`.
const MIN_TC_FIXUPS: i64 = 4;

/// The byte at `i` of a C string: NUL past its end -- and before its start,
/// which upstream reads where the string follows another in a string table,
/// whose terminating NUL is there.
fn at(s: &[u8], i: i64) -> u8 {
    usize::try_from(i)
        .ok()
        .and_then(|i| s.get(i))
        .copied()
        .unwrap_or(0)
}

/// `isdigit`.
fn isdigit(c: u8) -> bool {
    c.is_ascii_digit()
}

/// `isoctal`.
fn isoctal(c: u8) -> bool {
    (b'0'..=b'7').contains(&c)
}

/// `isgraph` in the C locale.
fn isgraph(c: u8) -> bool {
    (0x21..0x7f).contains(&c)
}

/// `%#x`: `0` alone for zero, as C prints it.
fn hash_x(v: u32) -> String {
    if v == 0 {
        "0".to_owned()
    } else {
        format!("{v:#x}")
    }
}

/// `save_char`: a NUL saves nothing -- it is `save_string` of an empty
/// string.
fn save_char(out: &mut Vec<u8>, c: u8) {
    if c != 0 {
        out.push(c);
    }
}

/// The low byte of `'0' + n`, as `(char)` makes it.
fn digit_char(n: i32) -> u8 {
    i32::from(b'0')
        .wrapping_add(n)
        .to_le_bytes()
        .first()
        .copied()
        .unwrap_or(0)
}

/// `_nc_captoinfo`'s state: upstream's statics, for one call.
struct CapToInfo<'s, 'd> {
    scan: &'s mut Scanner<'d>,
    stack: [i32; MAX_PUSHED],
    stackptr: usize,
    onstack: i32,
    seenm: i32,
    seenn: i32,
    seenr: i32,
    param: i32,
    dp: Vec<u8>,
}

impl CapToInfo<'_, '_> {
    fn save(&mut self, s: &[u8]) {
        self.dp.extend_from_slice(cstr(s));
    }

    /// `push`.
    fn push(&mut self) {
        if self.stackptr >= MAX_PUSHED {
            self.scan.warning(b"string too complex to convert");
        } else if let Some(slot) = self.stack.get_mut(self.stackptr) {
            *slot = self.onstack;
            self.stackptr = self.stackptr.saturating_add(1);
        }
    }

    /// `pop`.
    fn pop(&mut self) {
        if self.stackptr == 0 {
            if self.onstack == 0 {
                self.scan.warning(b"I'm confused");
            } else {
                self.onstack = 0;
            }
        } else {
            self.stackptr = self.stackptr.saturating_sub(1);
            self.onstack = self.stack.get(self.stackptr).copied().unwrap_or(0);
        }
        self.param = self.param.wrapping_add(1);
    }

    /// `cvtchar (sp)`: the character at `i` of `s`, in termcap's notation,
    /// pushed as a terminfo constant; how many bytes it took.
    fn cvtchar(&mut self, s: &[u8], i: i64) -> i64 {
        let b = |k: i64| at(s, i.wrapping_add(k));
        let (c, len): (u8, i64) = match b(0) {
            b'\\' => match b(1) {
                q @ (b'\'' | b'$' | b'\\' | b'%') => (q, 2),
                0 => (b'\\', 1),
                b'0'..=b'3' => {
                    let mut c: u8 = 0;
                    let mut len: i64 = 1;
                    let mut k = 1;
                    while isdigit(b(k)) {
                        c = c.wrapping_mul(8).wrapping_add(b(k).wrapping_sub(b'0'));
                        k = k.wrapping_add(1);
                        len = len.wrapping_add(1);
                    }
                    (c, len)
                }
                other => (other, 2),
            },
            b'^' => match b(1) {
                b'?' => (127, 2),
                0 => (0, 1),
                c => (c & 0x1f, 2),
            },
            c => (c, i64::from(c != 0)),
        };
        if isgraph(c) && c != b',' && c != b'\'' && c != b'\\' && c != b':' {
            self.save(b"%'");
            save_char(&mut self.dp, c);
            save_char(&mut self.dp, b'\'');
        } else if c != 0 {
            self.save(b"%{");
            // Each a decimal digit, so the additions cannot overflow.
            if c > 99 {
                save_char(&mut self.dp, (c / 100).wrapping_add(b'0'));
            }
            if c > 9 {
                save_char(&mut self.dp, ((c / 10) % 10).wrapping_add(b'0'));
            }
            save_char(&mut self.dp, (c % 10).wrapping_add(b'0'));
            save_char(&mut self.dp, b'}');
        }
        len
    }

    /// `getparm (parm, n)`: `n` pushes of parameter `parm`.
    fn getparm(&mut self, parm: i32, n: i32) {
        let mut parm = parm;
        if self.seenr != 0 {
            if parm == 1 {
                parm = 2;
            } else if parm == 2 {
                parm = 1;
            }
        }
        for _ in 0..n {
            self.save(b"%p");
            save_char(&mut self.dp, digit_char(parm));
        }
        if self.onstack == parm {
            if n > 1 {
                self.scan.warning(b"string may not be optimal");
                self.save(b"%Pa");
                for _ in 0..n {
                    self.save(b"%ga");
                }
            }
            return;
        }
        if self.onstack != 0 {
            self.push();
        }
        self.onstack = parm;
        if self.seenn != 0 && parm < 3 {
            self.save(b"%{96}%^");
        }
        if self.seenm != 0 && parm < 3 {
            self.save(b"%{127}%^");
        }
    }

    /// A warning naming the capability.
    fn twice(&mut self, code: u8, cap: &[u8]) {
        let mut m = b"saw %".to_vec();
        m.push(code);
        m.extend_from_slice(b" twice in ");
        m.extend_from_slice(cap);
        self.scan.warning(&m);
    }
}

/// `_nc_captoinfo (cap, s, parameterized)`: the termcap string `s` of the
/// capability `cap` in terminfo's notation. `parameterized` is
/// [`crate::captab::PARAMETRIZED`]'s: -1 to translate nothing, 0 the
/// leading padding only, 1 the `%` codes too.
#[allow(
    clippy::too_many_lines,
    reason = "upstream's _nc_captoinfo, in one piece so it reads against it"
)]
pub fn captoinfo(scan: &mut Scanner<'_>, cap: &[u8], s: &[u8], parameterized: i32) -> Vec<u8> {
    let s = cstr(s);
    let mut cv = CapToInfo {
        scan,
        stack: [0; MAX_PUSHED],
        stackptr: 0,
        onstack: 0,
        seenm: 0,
        seenn: 0,
        seenr: 0,
        param: 1,
        dp: Vec::new(),
    };
    let mut i: i64 = 0;

    // "skip the initial padding (if we haven't been told not to)"
    let mut capstart: Option<i64> = None;
    if parameterized >= 0 && isdigit(at(s, 0)) {
        capstart = Some(0);
        while at(s, i) != 0 {
            let c = at(s, i);
            if !(isdigit(c) || c == b'*' || c == b'.') {
                break;
            }
            i = i.wrapping_add(1);
        }
    }

    while at(s, i) != 0 {
        if at(s, i) != b'%' {
            save_char(&mut cv.dp, at(s, i));
            i = i.wrapping_add(1);
            continue;
        }
        i = i.wrapping_add(1);
        if parameterized < 1 {
            save_char(&mut cv.dp, b'%');
            continue;
        }
        let code = at(s, i);
        i = i.wrapping_add(1);
        match code {
            b'%' => cv.save(b"%%"),
            b'r' => {
                if cv.seenr == 1 {
                    cv.twice(b'r', cap);
                }
                cv.seenr = cv.seenr.wrapping_add(1);
            }
            b'm' => {
                if cv.seenm == 1 {
                    cv.twice(b'm', cap);
                }
                cv.seenm = cv.seenm.wrapping_add(1);
            }
            b'n' => {
                if cv.seenn == 1 {
                    cv.twice(b'n', cap);
                }
                cv.seenn = cv.seenn.wrapping_add(1);
            }
            b'i' => cv.save(b"%i"),
            b'6' | b'B' => {
                cv.getparm(cv.param, 1);
                cv.save(b"%{10}%/%{16}%*");
                cv.getparm(cv.param, 1);
                cv.save(b"%{10}%m%+");
            }
            b'8' | b'D' => {
                cv.getparm(cv.param, 2);
                cv.save(b"%{2}%*%-");
            }
            b'>' => {
                // %?%{x}%>%t%{y}%+%;
                if at(s, i) != 0 && at(s, i.wrapping_add(1)) != 0 {
                    cv.getparm(cv.param, 2);
                    cv.save(b"%?");
                    i = i.wrapping_add(cv.cvtchar(s, i));
                    cv.save(b"%>%t");
                    i = i.wrapping_add(cv.cvtchar(s, i));
                    cv.save(b"%+%;");
                } else {
                    cv.scan.warning(b"expected two characters after %>");
                    cv.save(b"%>");
                }
            }
            b'a' => {
                let op = at(s, i);
                if matches!(op, b'=' | b'+' | b'-' | b'*' | b'/')
                    && matches!(at(s, i.wrapping_add(1)), b'p' | b'c')
                    && at(s, i.wrapping_add(2)) != 0
                {
                    let mut l: i64 = 2;
                    if op != b'=' {
                        cv.getparm(cv.param, 1);
                    }
                    if at(s, i.wrapping_add(1)) == b'p' {
                        // `param + s[2] - '@'`, the char signed.
                        let c2 = i32::from(i8::from_le_bytes([at(s, i.wrapping_add(2))]));
                        let p = cv.param.wrapping_add(c2).wrapping_sub(i32::from(b'@'));
                        cv.getparm(p, 1);
                        if cv.param != cv.onstack {
                            cv.pop();
                            cv.param = cv.param.wrapping_sub(1);
                        }
                        l = l.wrapping_add(1);
                    } else {
                        l = l.wrapping_add(cv.cvtchar(s, i.wrapping_add(2)));
                    }
                    match op {
                        b'+' => cv.save(b"%+"),
                        b'-' => cv.save(b"%-"),
                        b'*' => cv.save(b"%*"),
                        b'/' => cv.save(b"%/"),
                        _ => {
                            // '='
                            cv.onstack = if cv.seenr != 0 {
                                match cv.param {
                                    1 => 2,
                                    2 => 1,
                                    p => p,
                                }
                            } else {
                                cv.param
                            };
                        }
                    }
                    i = i.wrapping_add(l);
                } else {
                    cv.getparm(cv.param, 1);
                    i = i.wrapping_add(cv.cvtchar(s, i));
                    cv.save(b"%+");
                }
            }
            b'+' => {
                cv.getparm(cv.param, 1);
                i = i.wrapping_add(cv.cvtchar(s, i));
                cv.save(b"%+%c");
                cv.pop();
            }
            b's' => {
                cv.getparm(cv.param, 1);
                cv.save(b"%s");
                cv.pop();
            }
            b'-' => {
                i = i.wrapping_add(cv.cvtchar(s, i));
                cv.getparm(cv.param, 1);
                cv.save(b"%-%c");
                cv.pop();
            }
            b'.' => {
                cv.getparm(cv.param, 1);
                cv.save(b"%c");
                cv.pop();
            }
            b'0' if at(s, i) == b'3' || at(s, i) == b'2' => {
                let width = at(s, i);
                i = i.wrapping_add(1);
                cv.getparm(cv.param, 1);
                cv.save(if width == b'3' { b"%3d" } else { b"%2d" });
                cv.pop();
            }
            b'2' => {
                cv.getparm(cv.param, 1);
                cv.save(b"%2d");
                cv.pop();
            }
            b'3' => {
                cv.getparm(cv.param, 1);
                cv.save(b"%3d");
                cv.pop();
            }
            b'd' => {
                cv.getparm(cv.param, 1);
                cv.save(b"%d");
                cv.pop();
            }
            b'f' => cv.param = cv.param.wrapping_add(1),
            b'b' => cv.param = cv.param.wrapping_sub(1),
            b'\\' => cv.save(b"%\\"),
            _ => {
                // invalid: the code is given back, to be copied as it is.
                save_char(&mut cv.dp, b'%');
                i = i.wrapping_sub(1);
                let c = at(s, i);
                let mut m = b"unknown % code ".to_vec();
                m.extend_from_slice(unctrl(c).as_bytes());
                m.extend_from_slice(format!(" ({}) in ", hash_x(u32::from(c))).as_bytes());
                m.extend_from_slice(cap);
                cv.scan.warning(&m);
            }
        }
    }

    // "if we stripped off some leading padding, add it at the end of the
    // string as mandatory padding"
    if let Some(start) = capstart {
        cv.save(b"$<");
        let mut k = start;
        while at(s, k) != 0 {
            let c = at(s, k);
            if isdigit(c) || c == b'*' || c == b'.' {
                save_char(&mut cv.dp, c);
            } else {
                break;
            }
            k = k.wrapping_add(1);
        }
        cv.save(b"/>");
    }
    cv.dp
}

// ---- info-to-cap -----------------------------------------------------------

/// One step of a `sscanf` template.
#[derive(Clone, Copy)]
enum Fmt {
    /// A byte that must be there.
    Lit(u8),
    /// `%d`.
    Int,
    /// `%c`.
    Char,
}

/// A value `sscanf` converted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Got {
    Int(i32),
    Char(u8),
}

/// A template in `sscanf`'s notation, as steps.
fn template(fmt: &[u8]) -> Vec<Fmt> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while let Some(&c) = fmt.get(i) {
        if c == b'%' {
            match fmt.get(i.saturating_add(1)) {
                Some(b'd') => out.push(Fmt::Int),
                Some(b'c') => out.push(Fmt::Char),
                _ => out.push(Fmt::Lit(b'%')),
            }
            i = i.saturating_add(2);
        } else {
            out.push(Fmt::Lit(c));
            i = i.saturating_add(1);
        }
    }
    out
}

/// `sscanf (s, fmt, ...)`: the values it converts, in order, up to the
/// first mismatch -- what it stores, whether or not the whole template
/// matched; its count is their number.
fn sscanf(s: &[u8], fmt: &[u8]) -> Vec<Got> {
    let mut got = Vec::new();
    let mut i: i64 = 0;
    for step in template(fmt) {
        match step {
            Fmt::Lit(c) => {
                if at(s, i) != c {
                    break;
                }
                i = i.wrapping_add(1);
            }
            Fmt::Char => {
                let c = at(s, i);
                if c == 0 {
                    break;
                }
                got.push(Got::Char(c));
                i = i.wrapping_add(1);
            }
            Fmt::Int => {
                // White space, a sign, then at least one digit; what
                // overflows `int` keeps its low bits, as glibc stores it.
                while matches!(at(s, i), b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r') {
                    i = i.wrapping_add(1);
                }
                let start = i;
                if matches!(at(s, i), b'+' | b'-') {
                    i = i.wrapping_add(1);
                }
                if !isdigit(at(s, i)) {
                    let _ = start;
                    break;
                }
                let neg = at(s, start) == b'-';
                let mut v: i64 = 0;
                while isdigit(at(s, i)) {
                    v = v
                        .saturating_mul(10)
                        .saturating_add(i64::from(at(s, i).wrapping_sub(b'0')));
                    i = i.wrapping_add(1);
                }
                let v = if neg { v.saturating_neg() } else { v };
                got.push(Got::Int(cstrtol::low_i32(v)));
            }
        }
    }
    got
}

/// `save_tc_char`: a character as termcap writes one.
fn save_tc_char(out: &mut Vec<u8>, c1: i32) {
    let printable = (0x20..0x7f).contains(&c1);
    if printable {
        let b = u8::try_from(c1).unwrap_or(0);
        if b == b':' || b == b'\\' {
            out.push(b'\\');
        }
        save_char(out, b);
    } else if c1 == (c1 & 0x1f) {
        // "iscntrl() returns T on 255"
        let b = u8::try_from(c1).unwrap_or(0);
        out.extend_from_slice(unctrl(b).as_bytes());
    } else {
        out.extend_from_slice(format!("\\{c1:03o}").as_bytes());
    }
}

/// `save_tc_inequality`.
fn save_tc_inequality(out: &mut Vec<u8>, c1: i32, c2: i32) {
    out.extend_from_slice(b"%>");
    save_tc_char(out, c1);
    save_tc_char(out, c2);
}

/// `bcd_expression (str)`: 28 when `str` starts with terminfo's spelling of
/// `%B` -- or as much of it as `sscanf` checks -- else 0.
fn bcd_expression(s: &[u8]) -> i64 {
    let got = sscanf(s, b"%%p%c%%{10}%%/%%{16}%%*%%p%c%%{10}%%m%%+");
    match got.as_slice() {
        [Got::Char(c1), Got::Char(c2)] if isdigit(*c1) && isdigit(*c2) && c1 == c2 => 28,
        _ => 0,
    }
}

/// A `%c` value as upstream's `char` holds it.
fn signed(c: u8) -> i32 {
    i32::from(i8::from_le_bytes([c]))
}

/// `_nc_infotocap (cap, str, parameterized)`: the terminfo string `str` in
/// termcap's notation, or `None` where termcap cannot say it. `strict_bsd`
/// is `_nc_strict_bsd` (`-K`).
#[allow(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    reason = "upstream's _nc_infotocap, in one piece so it reads against it"
)]
#[must_use]
pub fn infotocap(str_: &[u8], parameterized: i32, strict_bsd: bool) -> Option<Vec<u8>> {
    let s = cstr(str_);
    let len = i64::try_from(s.len()).unwrap_or(i64::MAX);
    let mut seenone = false;
    let mut seentwo = false;
    let mut saw_m = 0i32;
    let mut saw_n = 0i32;
    let mut trimmed: Option<i64> = None;
    let (mut ch1, mut ch2): (u8, u8) = (0, 0);
    let (mut c1, mut c2): (i32, i32) = (0, 0);
    let mut buf: Vec<u8> = Vec::new();
    let mut syntax_error = false;
    let mut fixups: Vec<(i32, usize)> = Vec::new();
    let mut myfix = 0usize;

    // "we may have to move some trailing mandatory padding up front"
    let mut padding = len.wrapping_sub(1);
    if padding > 0 && at(s, padding) == b'>' {
        if padding > 1 {
            padding = padding.wrapping_sub(1);
            if at(s, padding) == b'/' {
                padding = padding.wrapping_sub(1);
            }
        }
        while isdigit(at(s, padding)) || at(s, padding) == b'.' || at(s, padding) == b'*' {
            padding = padding.wrapping_sub(1);
        }
        if padding > 0 && at(s, padding) == b'<' {
            padding = padding.wrapping_sub(1);
            if at(s, padding) == b'$' {
                trimmed = Some(padding);
            }
        }
        padding = padding.wrapping_add(2);
        while isdigit(at(s, padding)) || at(s, padding) == b'.' || at(s, padding) == b'*' {
            save_char(&mut buf, at(s, padding));
            padding = padding.wrapping_add(1);
        }
    }

    let mut q: i64 = 0;
    while !syntax_error && at(s, q) != 0 && trimmed.is_none_or(|t| q < t) {
        let here = s
            .get(usize::try_from(q).unwrap_or(usize::MAX)..)
            .unwrap_or_default();
        let is_trimmed = |k: i64| trimmed == Some(k);
        let xterm = sscanf(
            here,
            b"[%%?%%p1%%{8}%%<%%t%d%%p1%%d%%e%%p1%%{16}%%<%%t%d%%p1%%{8}%%-%%d%%e%d;5;%%p1%%d%%;m",
        );
        if at(s, q) == b'^' {
            if at(s, q.wrapping_add(1)) == 0 || is_trimmed(q.wrapping_add(1)) {
                buf.extend_from_slice(b"\\136");
                q = q.wrapping_add(1);
            } else if at(s, q.wrapping_add(1)) == b'?' {
                buf.extend_from_slice(b"\\177");
                q = q.wrapping_add(1);
            } else {
                save_char(&mut buf, at(s, q));
                q = q.wrapping_add(1);
                save_char(&mut buf, at(s, q));
            }
        } else if at(s, q) == b':' {
            buf.extend_from_slice(b"\\072");
        } else if at(s, q) == b'\\' {
            if at(s, q.wrapping_add(1)) == 0 || is_trimmed(q.wrapping_add(1)) {
                buf.extend_from_slice(b"\\134");
                q = q.wrapping_add(1);
            } else if at(s, q.wrapping_add(1)) == b'^' {
                buf.extend_from_slice(b"\\136");
                q = q.wrapping_add(1);
            } else if at(s, q.wrapping_add(1)) == b',' {
                q = q.wrapping_add(1);
                save_char(&mut buf, at(s, q));
            } else {
                save_char(&mut buf, at(s, q));
                q = q.wrapping_add(1);
                let mut xx1 = at(s, q);
                let offset = buf.len().saturating_sub(1);
                if strict_bsd {
                    if isoctal(xx1) {
                        let mut pad: i32 = 0;
                        if !isoctal(at(s, q.wrapping_add(1))) {
                            pad = 2;
                        } else if at(s, q.wrapping_add(1)) != 0
                            && !isoctal(at(s, q.wrapping_add(2)))
                        {
                            pad = 1;
                        }
                        // "Test for "\0", "\00" or "\000" and transform
                        // those into "\200"."
                        let mut xx2;
                        if xx1 == b'0'
                            && (pad == 2 || at(s, q.wrapping_add(1)) == b'0')
                            && (pad >= 1 || at(s, q.wrapping_add(2)) == b'0')
                        {
                            xx2 = b'2';
                        } else {
                            xx2 = b'0';
                            pad = 0;
                        }
                        let mut fix = 0;
                        let mut ch: i32 = 0;
                        if myfix < MAX_TC_FIXUPS {
                            fix = 3i32.wrapping_sub(pad);
                        }
                        while pad > 0 {
                            pad = pad.wrapping_sub(1);
                            save_char(&mut buf, xx2);
                            if myfix < MAX_TC_FIXUPS {
                                ch = ch.wrapping_shl(3) | i32::from(xx2.wrapping_sub(b'0'));
                            }
                            xx2 = b'0';
                        }
                        if myfix < MAX_TC_FIXUPS {
                            for n in 0..i64::from(fix) {
                                ch = ch.wrapping_shl(3)
                                    | i32::from(at(s, q.wrapping_add(n)))
                                        .wrapping_sub(i32::from(b'0'));
                            }
                            set_fixup(&mut fixups, myfix, ch, offset);
                            if ch < 32 {
                                myfix = myfix.wrapping_add(1);
                            }
                        }
                    } else if !b"E\\nrtbf".contains(&xx1) {
                        match xx1 {
                            b'e' => xx1 = b'E',
                            b'l' => xx1 = b'n',
                            b's' => {
                                save_char(&mut buf, b'0');
                                save_char(&mut buf, b'4');
                                xx1 = b'0';
                            }
                            b':' => {
                                save_char(&mut buf, b'0');
                                save_char(&mut buf, b'7');
                                xx1 = b'2';
                            }
                            _ => {
                                // "should not happen, but handle this anyway"
                                let octal = format!("{xx1:03o}").into_bytes();
                                save_char(&mut buf, octal.first().copied().unwrap_or(0));
                                save_char(&mut buf, octal.get(1).copied().unwrap_or(0));
                                xx1 = octal.get(2).copied().unwrap_or(0);
                            }
                        }
                    }
                } else if myfix < MAX_TC_FIXUPS && isoctal(xx1) {
                    let mut will_fix = true;
                    let mut ch: i32 = 0;
                    for n in 0..3 {
                        let digit = at(s, q.wrapping_add(n));
                        if isoctal(digit) {
                            ch = ch.wrapping_shl(3) | i32::from(digit.wrapping_sub(b'0'));
                        } else {
                            will_fix = false;
                            break;
                        }
                    }
                    set_fixup(&mut fixups, myfix, ch, offset);
                    if will_fix && ch < 32 {
                        myfix = myfix.wrapping_add(1);
                    }
                }
                save_char(&mut buf, xx1);
            }
        } else if at(s, q) == b'$' && at(s, q.wrapping_add(1)) == b'<' {
            // discard padding
            q = q.wrapping_add(2);
            while matches!(at(s, q), b'0'..=b'9' | b'.' | b'*' | b'/' | b'>') {
                q = q.wrapping_add(1);
            }
            q = q.wrapping_sub(1);
        } else if let [Got::Int(in0), Got::Int(in1), Got::Int(in2)] = xterm.as_slice()
            && ((*in0, *in1, *in2) == (4, 10, 48) || (*in0, *in1, *in2) == (3, 9, 38))
        {
            // "dumb-down an optimized case from xterm-256color for termcap"
            let Some(m) = here.windows(2).position(|w| w == b";m") else {
                break;
            };
            q = q.wrapping_add(i64::try_from(m).unwrap_or(0).wrapping_add(1));
            if *in2 == 48 {
                buf.extend_from_slice(b"[48;5;%dm");
            } else {
                buf.extend_from_slice(b"[38;5;%dm");
            }
        } else if at(s, q) == b'%' && at(s, q.wrapping_add(1)) == b'%' {
            buf.extend_from_slice(b"%%");
            q = q.wrapping_add(1);
        } else if at(s, q) != b'%' || parameterized < 1 {
            save_char(&mut buf, at(s, q));
        } else {
            // The inequality templates: each stores what it converts, as
            // far as it gets, whether or not it matches.
            let mut matched = false;
            let forms: [(&[u8], bool, bool); 4] = [
                (b"%%?%%{%d}%%>%%t%%{%d}%%+%%;", false, false),
                (b"%%?%%{%d}%%>%%t%%'%c'%%+%%;", false, true),
                (b"%%?%%'%c'%%>%%t%%{%d}%%+%%;", true, false),
                (b"%%?%%'%c'%%>%%t%%'%c'%%+%%;", true, true),
            ];
            for (fmt, first_char, second_char) in forms {
                let got = sscanf(here, fmt);
                for (k, g) in got.iter().enumerate() {
                    match (k, g) {
                        (0, Got::Int(v)) => c1 = *v,
                        (0, Got::Char(v)) => ch1 = *v,
                        (1, Got::Int(v)) => c2 = *v,
                        (1, Got::Char(v)) => ch2 = *v,
                        _ => {}
                    }
                }
                if got.len() == 2 {
                    let a = if first_char { signed(ch1) } else { c1 };
                    let b = if second_char { signed(ch2) } else { c2 };
                    match here.iter().position(|&c| c == b';') {
                        Some(semi) => q = q.wrapping_add(i64::try_from(semi).unwrap_or(0)),
                        None => q = len,
                    }
                    save_tc_inequality(&mut buf, a, b);
                    matched = true;
                    break;
                }
            }
            if !matched {
                let bcd = bcd_expression(here);
                if bcd != 0 {
                    q = q.wrapping_add(bcd);
                    buf.extend_from_slice(b"%B");
                } else {
                    let first = sscanf(here, b"%%{%d}%%+%%%c");
                    for (k, g) in first.iter().enumerate() {
                        match (k, g) {
                            (0, Got::Int(v)) => c1 = *v,
                            (1, Got::Char(v)) => ch2 = *v,
                            _ => {}
                        }
                    }
                    let mut either = first.len() == 2;
                    if !either {
                        let second = sscanf(here, b"%%'%c'%%+%%%c");
                        for (k, g) in second.iter().enumerate() {
                            match (k, g) {
                                (0, Got::Char(v)) => ch1 = *v,
                                (1, Got::Char(v)) => ch2 = *v,
                                _ => {}
                            }
                        }
                        either = second.len() == 2;
                    }
                    let plus = here.iter().position(|&c| c == b'+');
                    if either && ch2 == b'c' && plus.is_some() {
                        q = q.wrapping_add(
                            i64::try_from(plus.unwrap_or(0))
                                .unwrap_or(0)
                                .wrapping_add(2),
                        );
                        buf.extend_from_slice(b"%+");
                        if ch1 != 0 {
                            c1 = signed(ch1);
                        }
                        save_tc_char(&mut buf, c1);
                    } else if here.starts_with(b"%{2}%*%-") {
                        // "FIXME: this "works" for 'delta'"
                        q = q.wrapping_add(7);
                        buf.extend_from_slice(b"%D");
                    } else if here.starts_with(b"%{96}%^") {
                        q = q.wrapping_add(6);
                        if saw_m == 0 {
                            buf.extend_from_slice(b"%n");
                        }
                        saw_m = saw_m.wrapping_add(1);
                    } else if here.starts_with(b"%{127}%^") {
                        q = q.wrapping_add(7);
                        if saw_n == 0 {
                            buf.extend_from_slice(b"%m");
                        }
                        saw_n = saw_n.wrapping_add(1);
                    } else {
                        // cm-style format element
                        q = q.wrapping_add(1);
                        match at(s, q) {
                            b'%' => save_char(&mut buf, b'%'),
                            b'0'..=b'9' => {
                                save_char(&mut buf, b'%');
                                ch1 = 0;
                                ch2 = 0;
                                let mut digits = 0i32;
                                while isdigit(at(s, q)) {
                                    digits = digits.wrapping_add(1);
                                    if digits > 2 {
                                        syntax_error = true;
                                        break;
                                    }
                                    ch2 = ch1;
                                    ch1 = at(s, q);
                                    q = q.wrapping_add(1);
                                    if digits == 2 && ch2 != b'0' {
                                        syntax_error = true;
                                        break;
                                    } else if strict_bsd {
                                        if ch1 > b'3' {
                                            syntax_error = true;
                                            break;
                                        }
                                    } else {
                                        save_char(&mut buf, ch1);
                                    }
                                }
                                if !syntax_error {
                                    // "Convert %02 to %2 and %03 to %3"
                                    if ch2 == b'0' && !strict_bsd {
                                        ch2 = 0;
                                        if let Some(last) = buf.pop() {
                                            buf.pop();
                                            buf.push(last);
                                        }
                                    }
                                    if strict_bsd {
                                        if ch2 != 0 && ch2 != b'0' {
                                            syntax_error = true;
                                        } else if ch1 < b'2' {
                                            ch1 = b'd';
                                        }
                                        save_char(&mut buf, ch1);
                                    }
                                    if b"oxX.\0".contains(&at(s, q)) {
                                        // "termcap doesn't have octal, hex"
                                        syntax_error = true;
                                    }
                                }
                            }
                            b'd' => buf.extend_from_slice(b"%d"),
                            b'c' => buf.extend_from_slice(b"%."),
                            b's' => {
                                if strict_bsd {
                                    syntax_error = true;
                                } else {
                                    buf.extend_from_slice(b"%s");
                                }
                            }
                            b'p' => {
                                q = q.wrapping_add(1);
                                match at(s, q) {
                                    b'1' => seenone = true,
                                    b'2' => {
                                        if !seenone && !seentwo {
                                            buf.extend_from_slice(b"%r");
                                            seentwo = true;
                                        }
                                    }
                                    // `*str >= '3'`, the char signed.
                                    c if signed(c) >= i32::from(b'3') => syntax_error = true,
                                    _ => {}
                                }
                            }
                            b'i' => buf.extend_from_slice(b"%i"),
                            c => {
                                save_char(&mut buf, c);
                                syntax_error = true;
                            }
                        }
                    }
                }
            }
        }
        // "'str' always points to the end of what was scanned in this
        // step, but that may not be the end of the string."
        if at(s, q) == 0 {
            break;
        }
        q = q.wrapping_add(1);
    }

    if !syntax_error
        && myfix > 0
        && i64::try_from(buf.len())
            .unwrap_or(i64::MAX)
            .wrapping_sub(i64::try_from(myfix).unwrap_or(0).wrapping_mul(4))
            < MIN_TC_FIXUPS
    {
        for &(ch, offset) in fixups.iter().take(myfix).rev() {
            // "\ddd" becomes "^X": two bytes written over the four, the
            // rest moved down two.
            let caret = (ch | i32::from(b'@'))
                .to_le_bytes()
                .first()
                .copied()
                .unwrap_or(0);
            if offset.saturating_add(4) <= buf.len() {
                buf.splice(offset..offset.saturating_add(4), [b'^', caret]);
            } else {
                buf.truncate(offset);
                buf.extend_from_slice(&[b'^', caret]);
            }
        }
    }
    (!syntax_error).then_some(buf)
}

/// Record fixup `n`: the value of a `\ddd` escape and where its backslash
/// is.
fn set_fixup(fixups: &mut Vec<(i32, usize)>, n: usize, ch: i32, offset: usize) {
    if let Some(slot) = fixups.get_mut(n) {
        *slot = (ch, offset);
    } else {
        fixups.push((ch, offset));
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn to_info(s: &[u8], p: i32) -> (Vec<u8>, Vec<u8>) {
        let mut diag = Vec::new();
        let out = {
            let mut scan = Scanner::new(&mut diag);
            captoinfo(&mut scan, b"cm", s, p)
        };
        (out, diag)
    }

    #[test]
    fn cursor_motion_translates_to_terminfo() {
        assert_eq!(to_info(b"\\E[%i%d;%dH", 1).0, b"\\E[%i%p1%d;%p2%dH");
        // Measured: a space is no `isgraph` character, so it is pushed as a
        // number.
        assert_eq!(to_info(b"\\E=%+ %+ ", 1).0, b"\\E=%p1%{32}%+%c%p2%{32}%+%c");
        assert_eq!(to_info(b"%+A", 1).0, b"%p1%'A'%+%c");
        assert_eq!(to_info(b"%r%d,%d", 1).0, b"%p2%d,%p1%d");
    }

    #[test]
    fn leading_padding_becomes_mandatory_trailing_padding() {
        assert_eq!(to_info(b"5*\\E[H", 0).0, b"\\E[H$<5*/>");
        assert_eq!(to_info(b"5\\E[H", -1).0, b"5\\E[H");
    }

    #[test]
    fn an_unknown_code_is_warned_of_and_kept() {
        let (out, diag) = to_info(b"%z", 1);
        assert_eq!(out, b"%z");
        assert_eq!(
            String::from_utf8(diag).unwrap(),
            "\"?\": unknown % code z (0x7a) in cm\n"
        );
    }

    #[test]
    fn terminfo_goes_back_to_termcap() {
        assert_eq!(
            infotocap(b"\\E[%i%p1%d;%p2%dH", 1, false).unwrap(),
            b"\\E[%i%d;%dH"
        );
        assert_eq!(infotocap(b"\\E[H$<5>", 1, false).unwrap(), b"5\\E[H");
        assert_eq!(infotocap(b"a:b", 1, false).unwrap(), b"a\\072b");
        assert_eq!(infotocap(b"%p1%{8}%x", 1, false), None);
    }

    #[test]
    fn sscanf_stops_at_its_last_conversion() {
        assert_eq!(sscanf(b"%{5}xx", b"%%{%d}%%+%%%c"), vec![Got::Int(5)]);
        assert_eq!(
            sscanf(b"%{5}%+%c", b"%%{%d}%%+%%%c"),
            vec![Got::Int(5), Got::Char(b'c')]
        );
        assert_eq!(sscanf(b"%{ -7}", b"%%{%d}"), vec![Got::Int(-7)]);
    }
}
