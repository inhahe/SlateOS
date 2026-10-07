//! The process's timezone, as glibc 2.39 keeps it: `tzset`, the POSIX-rule
//! engine, the zoneinfo reader, and `mktime`'s search -- `time/tzset.c`,
//! `time/tzfile.c` and `time/mktime.c`, ported function by function, with
//! the same process-wide state (design-decisions §1165).
//!
//! Why glibc's code and not a reading of POSIX: the answers a program can see
//! -- which zone `TZ` names, what `tzname` holds after a call, which instant
//! an ambiguous wall-clock time is -- live in glibc's details, and every
//! program the system ports was written against them. `date` already gives
//! glibc's answers (`userspace/localtime`, design-decisions §1032); a C
//! program on the same machine must give the same ones
//! (requests/b-d-the-libc-reads-tz-unlike-glibc-and-now-unlike-date.md).
//! `posix/tools/oracle/tz_harness.py` records glibc 2.39's answers, and the
//! tests replay every one (`tz_oracle.txt`).
//!
//! | glibc | here | what it decides |
//! |---|---|---|
//! | `tzset_internal` | [`tzset_internal`] | unset is `/etc/localtime`, empty is `Universal`, a `:` is dropped, a file is tried before a rule, an unchanged `TZ` is not read again |
//! | `__tzset_parse_tz` and its parsers | [`tzset_parse_tz`] | a rule that parses in part keeps the part; `sscanf`'s `%hu` |
//! | `__tzfile_read` | [`tzfile_read`] | the file's tables, and `tzname`, `timezone`, `daylight` from its last transitions |
//! | `__tzfile_default` | [`tzfile_default`] | a DST name with no dates borrows `posixrules`' history, re-anchored by the process-wide `rule_dstoff` |
//! | `__tzfile_compute` | [`tzfile_compute`] | the type in force, and `tzname` moved to the names around the instant |
//! | `compute_change`, `__tz_compute` | the same names | the year is the instant's UTC year; every year up to 1970 changes as 1970 does |
//! | `__tz_convert` | [`convert`] | `localtime` reads `TZ` again, `localtime_r` does not |
//! | `__mktime_internal` | [`mktime`] | which instant a wall-clock time is, searched from the offset the last call found |
//!
//! # The state, as glibc has it
//!
//! glibc's statics are one [`State`] here, behind one lock, as glibc's are
//! behind `tzset_lock`. Three of them make the same `TZ` give different
//! answers in one process, and all three are kept, because a program that
//! ran on glibc saw them:
//!
//! * **`rule_dstoff`**: `__tzfile_default` re-anchors `posixrules`' DST
//!   transitions by the difference between the user's DST offset and this --
//!   which only a file with no transitions, or an earlier default, has set.
//! * **`computed_for`**, `compute_change`'s per-rule cache, starts at 0 for a
//!   half that did not parse, so the first lookup in the year 0 uses that
//!   half's zeroed change time.
//! * **`localtime_offset`**, the offset `mktime`'s search starts from: the
//!   one the previous call found. It decides which of an autumn hour's two
//!   instants comes back.
//!
//! And `tzname`, `timezone` and `daylight` move as glibc moves them: a
//! `localtime_r` in a zoneinfo zone sets `tzname` to the names in force
//! around the instant converted, and one past the last transition sets all
//! three from the file's footer rule.
//!
//! On the host the state is per test thread, as the environment it reads
//! `TZ` from is ([`crate::perprocess`]): libtest runs every test on its own
//! thread of one process, and the zone one test sets must not be the zone
//! the tests beside it read. A test that sets none reads a fresh process's,
//! UTC. The target, one process, has one.
//!
//! # Where it differs, each on purpose
//!
//! * **A `TZ` naming a file through a `..` component is never read**, where
//!   glibc refuses one only in a set-user-ID program: `TZ` is inherited from
//!   whoever started the program, and the file read is otherwise an oracle
//!   for "does this path exist and parse". `userspace/localtime` refuses it
//!   too, so the two readers of one `TZ` agree. The value then reads as a
//!   rule, exactly as a file that is not there does.
//! * **Leap seconds are not applied.** A `right/` zone's records are skipped,
//!   as `tzrules` skips them; glibc would shift the clock by the accumulated
//!   correction.
//! * **A footer with a DST name and no dates** gets the US rules: glibc would
//!   run `__tzfile_default` from inside a lookup and replace the zone mid-call.
//!   `zic` never writes one.
//! * **`TZ` values over [`OLD_TZ_CAP`] bytes are not remembered**, so `tzset`
//!   reads such a value again even when it has not changed.
//! * **A zoneinfo file over 16 KiB is refused**, read into a fixed buffer;
//!   glibc reads any size. tzdata's largest is under 4 KiB.
//! * **The names live in a fixed arena** ([`ARENA_CAP`] bytes), not in
//!   `malloc`ed blocks: a name that no longer fits is what a failed `malloc`
//!   is in glibc -- the rule or the file naming it is refused.

use core::sync::atomic::AtomicI32;

use tzrules::TzFile;

use crate::time::Tm;

// ---------------------------------------------------------------------------
// The lock
// ---------------------------------------------------------------------------

/// glibc's `tzset_lock`, which guards everything below.
static TZ_LOCK: AtomicI32 = AtomicI32::new(0);

/// Holds [`TZ_LOCK`]; the state is reached through it.
struct Locked;

impl Locked {
    fn take() -> Self {
        crate::lowlevellock::lll_lock(&TZ_LOCK);
        Self
    }

    #[allow(clippy::unused_self)]
    fn state(&mut self) -> &mut State {
        // SAFETY: the lock is held, so this is the only reference to the
        // state -- the process's, or on the host this test thread's.
        unsafe { &mut *state_storage() }
    }
}

impl Drop for Locked {
    fn drop(&mut self) {
        crate::lowlevellock::lll_unlock(&TZ_LOCK);
    }
}

// ---------------------------------------------------------------------------
// Names: __tzstring
// ---------------------------------------------------------------------------

/// Bytes of the name arena ([`arena_storage`]): every distinct zone name a
/// process meets, kept for the life of the process as glibc keeps them.
/// tzdata has under 300 distinct designations, and suffixes are shared, so
/// this is many times enough.
const ARENA_CAP: usize = 8 * 1024;

crate::perprocess::process_global! {
    /// The names, each NUL-terminated, one after another: the process's, or
    /// on the host this test thread's, as the rest of the state is.
    fn arena_storage() -> [u8; ARENA_CAP] = [0; ARENA_CAP];
    /// How much of [`arena_storage`] is in use.
    fn arena_used_storage() -> usize = 0;
}

/// A NUL-terminated name with the life of the process: in the arena
/// ([`arena_storage`]), or one of the literals below. `tzname[]` and
/// `tm_zone` hand these out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Name(*const u8);

// SAFETY: a `Name` points at bytes that are never written again (the arena
// only ever grows past what it hands out), in storage that lives as long as
// anything able to hold the name: the process, or on the host the test
// thread whose arena it is, which hands no name to another thread.
unsafe impl Send for Name {}
// SAFETY: as above.
unsafe impl Sync for Name {}

static LIT_EMPTY: &[u8] = b"\0";
static LIT_UTC: &[u8] = b"UTC\0";
static LIT_GMT: &[u8] = b"GMT\0";

impl Name {
    /// No name: a `tz_rule` glibc has cleared.
    const NULL: Self = Self(core::ptr::null());
    /// `""`, which `__tzset_parse_tz` starts both halves with.
    fn empty() -> Self {
        Self(LIT_EMPTY.as_ptr())
    }
    /// `"UTC"`, the fallback zone's.
    fn utc() -> Self {
        Self(LIT_UTC.as_ptr())
    }
    /// `"GMT"`, what `tzname` holds before any zone is read.
    pub fn gmt() -> Self {
        Self(LIT_GMT.as_ptr())
    }

    /// The pointer C sees.
    #[must_use]
    pub fn ptr(self) -> *const u8 {
        self.0
    }
}

/// `__tzstring_len`: a permanent home for `s`, shared with an earlier name
/// that `s` is the end of; `None` when the arena is full -- glibc's failed
/// `malloc`.
fn tzstring(s: &[u8]) -> Option<Name> {
    // SAFETY: called only with the lock held, which is the arena's guard.
    let (arena, used) = unsafe { (&mut *arena_storage(), &mut *arena_used_storage()) };
    let s = s.split(|&b| b == 0).next().unwrap_or(&[]);
    let mut start = 0usize;
    while start < *used {
        let len = arena.get(start..*used)?.iter().position(|&b| b == 0)?;
        if s.len() <= len {
            let from = start.checked_add(len)?.checked_sub(s.len())?;
            if arena.get(from..from.checked_add(s.len())?) == Some(s) {
                return Some(Name(arena.get(from..)?.as_ptr()));
            }
        }
        start = start.checked_add(len)?.checked_add(1)?;
    }
    let end = used.checked_add(s.len())?.checked_add(1)?;
    let slot = arena.get_mut(*used..end)?;
    let (body, nul) = slot.split_at_mut(s.len());
    body.copy_from_slice(s);
    *nul.first_mut()? = 0;
    let name = Name(slot.as_ptr());
    *used = end;
    Some(name)
}

// ---------------------------------------------------------------------------
// tz_rule
// ---------------------------------------------------------------------------

/// `enum { J0, J1, M } type`; the `memset`'s zero is `J0`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// `n`: day of the year, from 0, counting February 29th.
    J0,
    /// `Jn`: day of the year, from 1, never counting February 29th.
    J1,
    /// `Mm.n.d`: day `d` of week `n` of month `m`.
    M,
}

/// glibc's `tz_rule`: one half of a POSIX `TZ` string, with
/// `compute_change`'s cache.
#[derive(Clone, Copy)]
struct Rule {
    name: Name,
    kind: Kind,
    m: u16,
    n: u16,
    d: u16,
    /// The time of day of the change, in seconds; negative or past a day is
    /// allowed, and nothing clamps it.
    secs: i32,
    /// Seconds east of UT.
    offset: i32,
    change: i64,
    computed_for: i32,
}

/// A `tz_rule` after `memset (…, 0, …)`.
const RULE_ZERO: Rule = Rule {
    name: Name::NULL,
    kind: Kind::J0,
    m: 0,
    n: 0,
    d: 0,
    secs: 0,
    offset: 0,
    change: 0,
    computed_for: 0,
};

// ---------------------------------------------------------------------------
// The state
// ---------------------------------------------------------------------------

/// Bytes of a `TZ` value [`tzset_internal`] remembers to compare the next one
/// with. A longer one is read again at every `tzset` (see the module docs).
const OLD_TZ_CAP: usize = 1024;

/// `tzname`, `timezone` and `daylight` as glibc's code leaves them; written
/// out to the C-visible variables in [`crate::time`] when a call returns.
#[derive(Clone, Copy)]
struct Globals {
    tzname: [Name; 2],
    timezone: i64,
    daylight: i32,
}

/// What `__tzfile_default` made of `posixrules`: the user's two types, and
/// the shifts its transitions take (see [`State::transition`]).
#[derive(Clone, Copy)]
struct Defaulted {
    names: [Name; 2],
    offsets: [i32; 2],
    /// `stdoff - rule_stdoff` and `dstoff - rule_dstoff` as they were then.
    shift_std: i64,
    shift_dst: i64,
}

/// glibc's statics, from `tzset.c`, `tzfile.c` and `mktime.c`.
struct State {
    // tzset.c
    rules: [Rule; 2],
    old_tz: [u8; OLD_TZ_CAP],
    /// The length of `old_tz`, or `None` when glibc's `old_tz` is NULL (or
    /// the value did not fit).
    old_tz_len: Option<usize>,
    is_initialized: bool,
    globals: Globals,
    // tzfile.c
    use_tzfile: bool,
    file_id: Option<(u64, u64, i64)>,
    file: Option<TzFile<'static>>,
    defaulted: Option<Defaulted>,
    tzspec: Option<&'static [u8]>,
    rule_stdoff: i64,
    rule_dstoff: i64,
    daylight_saved: bool,
    // mktime.c
    localtime_offset: i64,
}

crate::perprocess::process_global! {
    /// glibc's statics: the process's, or on the host this test thread's.
    fn state_storage() -> State = State::FRESH;
}

impl State {
    /// A fresh process's.
    const FRESH: Self = Self {
        rules: [RULE_ZERO, RULE_ZERO],
        old_tz: [0; OLD_TZ_CAP],
        old_tz_len: None,
        is_initialized: false,
        globals: Globals {
            tzname: [Name(LIT_GMT_PTR), Name(LIT_GMT_PTR)],
            timezone: 0,
            daylight: 0,
        },
        use_tzfile: false,
        file_id: None,
        file: None,
        defaulted: None,
        tzspec: None,
        rule_stdoff: 0,
        rule_dstoff: 0,
        daylight_saved: false,
        localtime_offset: 0,
    };
}

/// `"GMT"` for the constant initialiser above, which cannot call a function.
const LIT_GMT_PTR: *const u8 = b"GMT\0".as_ptr();

/// `pair[second]`, glibc's `x[isdst]` on a two-element array.
fn pick<T: Copy>(pair: [T; 2], second: bool) -> T {
    if second { pair[1] } else { pair[0] }
}

/// `&mut pair[second]`.
fn slot<T>(pair: &mut [T; 2], second: bool) -> &mut T {
    let [first, other] = pair;
    if second { other } else { first }
}

// ---------------------------------------------------------------------------
// sscanf, as far as glibc's TZ parser uses it
// ---------------------------------------------------------------------------

/// The byte at `i`, or NUL past the end -- a C string's view of a slice.
fn at(s: &[u8], i: usize) -> u8 {
    s.get(i).copied().unwrap_or(0)
}

/// `isspace` in the C locale.
fn is_c_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// One directive of the two formats the parser gives `sscanf`.
#[derive(Clone, Copy)]
enum Directive {
    /// An ordinary character, which must match the input exactly.
    Lit(u8),
    /// `%hu`.
    Hu,
    /// `%n`: how many bytes have been consumed so far.
    N,
}

/// `"%hu%n:%hu%n:%hu%n"`: hours, minutes, seconds.
const HMS: [Directive; 8] = [
    Directive::Hu,
    Directive::N,
    Directive::Lit(b':'),
    Directive::Hu,
    Directive::N,
    Directive::Lit(b':'),
    Directive::Hu,
    Directive::N,
];

/// `"M%hu.%hu.%hu%n"`: month, week, day.
const MND: [Directive; 7] = [
    Directive::Lit(b'M'),
    Directive::Hu,
    Directive::Lit(b'.'),
    Directive::Hu,
    Directive::Lit(b'.'),
    Directive::Hu,
    Directive::N,
];

/// What one `sscanf` call did.
struct Scanned {
    /// Conversions made: `sscanf`'s return value, but 0 for its `EOF`, which
    /// no caller tells apart.
    count: usize,
    /// The converted values; only the first `count` were assigned.
    values: [u16; 3],
    /// The last `%n` reached, if any.
    consumed: Option<usize>,
}

impl Scanned {
    /// Value `i` if it was assigned, else the variable's prior value `old`.
    fn value_or(&self, i: usize, old: u16) -> u16 {
        if i < self.count {
            self.values.get(i).copied().unwrap_or(old)
        } else {
            old
        }
    }
}

/// `sscanf (s, format, ...)` for the directives above: `%hu` skips white
/// space, takes one sign, needs a digit, and stores `strtoul`'s value cut to
/// 16 bits; `%n` skips nothing; an ordinary character must match exactly.
/// The first failure ends the scan.
fn sscanf(s: &[u8], format: &[Directive]) -> Scanned {
    let mut out = Scanned {
        count: 0,
        values: [0; 3],
        consumed: None,
    };
    let mut p = 0usize;
    for directive in format {
        match *directive {
            Directive::Lit(c) => {
                if at(s, p) != c {
                    break;
                }
                p = p.saturating_add(1);
            }
            Directive::N => out.consumed = Some(p),
            Directive::Hu => {
                let Some((value, next)) = scan_hu(s, p) else {
                    break;
                };
                if let Some(slot) = out.values.get_mut(out.count) {
                    *slot = value;
                }
                out.count = out.count.saturating_add(1);
                p = next;
            }
        }
    }
    out
}

/// One `%hu` from `p`: the stored value and where it stopped, or `None` for
/// an input or matching failure.
fn scan_hu(s: &[u8], mut p: usize) -> Option<(u16, usize)> {
    while is_c_space(at(s, p)) {
        p = p.saturating_add(1);
    }
    let sign = at(s, p);
    if sign == 0 {
        return None;
    }
    let negative = sign == b'-';
    if sign == b'-' || sign == b'+' {
        p = p.saturating_add(1);
    }
    let start = p;
    let mut magnitude: Option<u64> = Some(0);
    while at(s, p).is_ascii_digit() {
        let digit = u64::from(at(s, p).wrapping_sub(b'0'));
        magnitude = magnitude
            .and_then(|m| m.checked_mul(10))
            .and_then(|m| m.checked_add(digit));
        p = p.saturating_add(1);
    }
    if p == start {
        return None;
    }
    // `strtoul`: ULONG_MAX on overflow whatever the sign, else the magnitude,
    // negated modulo 2^64 under a minus. Then `(unsigned short)`.
    let value = match magnitude {
        None => u64::MAX,
        Some(m) if negative => m.wrapping_neg(),
        Some(m) => m,
    };
    let [lo, hi, ..] = value.to_le_bytes();
    Some((u16::from_le_bytes([lo, hi]), p))
}

/// `strtoul (s + p, &end, 10)` where `s[p]` is a digit: the value, saturating
/// as `strtoul` does, and where it stopped.
fn strtoul_digits(s: &[u8], mut p: usize) -> (u64, usize) {
    let mut value: u64 = 0;
    while at(s, p).is_ascii_digit() {
        let digit = u64::from(at(s, p).wrapping_sub(b'0'));
        value = value.saturating_mul(10).saturating_add(digit);
        p = p.saturating_add(1);
    }
    (value, p)
}

// ---------------------------------------------------------------------------
// The POSIX rule string: __tzset_parse_tz
// ---------------------------------------------------------------------------

impl State {
    /// `update_vars`.
    fn update_vars(&mut self) {
        let [std, dst] = &self.rules;
        self.globals.daylight = i32::from(std.offset != dst.offset);
        self.globals.timezone = i64::from(std.offset).wrapping_neg();
        self.globals.tzname = [std.name, dst.name];
    }

    /// `parse_tzname`: three or more letters, or `<` three or more letters,
    /// digits, `+` and `-`, then `>`.
    fn parse_tzname(&mut self, s: &[u8], pos: &mut usize, which: usize) -> bool {
        let start = *pos;
        let mut p = start;
        while at(s, p).is_ascii_alphabetic() {
            p = p.saturating_add(1);
        }
        let (from, len) = if p.saturating_sub(start) >= 3 {
            (start, p.saturating_sub(start))
        } else {
            p = start;
            if at(s, p) != b'<' {
                return false;
            }
            p = p.saturating_add(1);
            let from = p;
            while matches!(at(s, p), b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'+' | b'-') {
                p = p.saturating_add(1);
            }
            let len = p.saturating_sub(from);
            if at(s, p) != b'>' || len < 3 {
                return false;
            }
            p = p.saturating_add(1);
            (from, len)
        };
        let Some(name) = s.get(from..from.saturating_add(len)).and_then(tzstring) else {
            return false;
        };
        if let Some(rule) = self.rules.get_mut(which) {
            rule.name = name;
        }
        *pos = p;
        true
    }

    /// `parse_offset`: the standard half needs a sign or a digit; the DST
    /// half may have neither, and is then an hour ahead of standard time.
    /// The sign is POSIX's, inverted: `EST5` is five hours *west*.
    fn parse_offset(&mut self, s: &[u8], pos: &mut usize, which: usize) -> bool {
        let mut p = *pos;
        let c = at(s, p);
        if which == 0 && (c == 0 || (c != b'+' && c != b'-' && !c.is_ascii_digit())) {
            return false;
        }
        let sign: i32 = if c == b'-' || c == b'+' {
            p = p.saturating_add(1);
            if c == b'-' { 1 } else { -1 }
        } else {
            -1
        };
        *pos = p;

        let scanned = sscanf(s.get(p..).unwrap_or(&[]), &HMS);
        let std_offset = self.rules[0].offset;
        let Some(rule) = self.rules.get_mut(which) else {
            return false;
        };
        if scanned.count > 0 {
            let hh = scanned.value_or(0, 0);
            let mm = scanned.value_or(1, 0);
            let ss = scanned.value_or(2, 0);
            rule.offset = sign.wrapping_mul(compute_offset(ss, mm, hh));
        } else if which == 0 {
            rule.offset = 0;
            return false;
        } else {
            rule.offset = std_offset.wrapping_add(3600);
        }
        *pos = p.saturating_add(scanned.consumed.unwrap_or(0));
        true
    }

    /// `parse_rule` for half `which`: a date (`Jn`, `n`, `Mm.n.d`, or nothing
    /// at all for the US rules), then an optional `/time`. On failure the
    /// fields already written stay written, as glibc parses straight into the
    /// `tz_rule`.
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "`hh * 3600 + mm * 60 + ss` of three `u16`s is at most 239923635, which `int` holds"
    )]
    fn parse_rule(&mut self, s: &[u8], pos: &mut usize, which: usize) -> bool {
        let mut p = *pos;
        let Some(rule) = self.rules.get_mut(which) else {
            return false;
        };
        // "Ignore comma to support string following the incorrect
        // specification in early POSIX.1 printings."
        if at(s, p) == b',' {
            p = p.saturating_add(1);
        }

        let c = at(s, p);
        if c == b'J' || c.is_ascii_digit() {
            rule.kind = if c == b'J' { Kind::J1 } else { Kind::J0 };
            if rule.kind == Kind::J1 {
                p = p.saturating_add(1);
                if !at(s, p).is_ascii_digit() {
                    return false;
                }
            }
            let (d, end) = strtoul_digits(s, p);
            if end == p || d > 365 {
                return false;
            }
            if rule.kind == Kind::J1 && d == 0 {
                return false;
            }
            rule.d = u16::try_from(d).unwrap_or(u16::MAX);
            p = end;
        } else if c == b'M' {
            rule.kind = Kind::M;
            let scanned = sscanf(s.get(p..).unwrap_or(&[]), &MND);
            rule.m = scanned.value_or(0, rule.m);
            rule.n = scanned.value_or(1, rule.n);
            rule.d = scanned.value_or(2, rule.d);
            if scanned.count != 3
                || rule.m < 1
                || rule.m > 12
                || rule.n < 1
                || rule.n > 5
                || rule.d > 6
            {
                return false;
            }
            p = p.saturating_add(scanned.consumed.unwrap_or(0));
        } else if c == 0 {
            // The Energy Policy Act of 2005's dates: "M3.2.0,M11.1.0".
            rule.kind = Kind::M;
            (rule.m, rule.n, rule.d) = if which == 0 { (3, 2, 0) } else { (11, 1, 0) };
        } else {
            return false;
        }

        match at(s, p) {
            0 | b',' => rule.secs = 2 * 3600,
            b'/' => {
                p = p.saturating_add(1);
                if at(s, p) == 0 {
                    return false;
                }
                let negative = at(s, p) == b'-';
                if negative {
                    p = p.saturating_add(1);
                }
                // Unassigned fields keep these, so `/x` is 02:00:00.
                let scanned = sscanf(s.get(p..).unwrap_or(&[]), &HMS);
                let hh = i32::from(scanned.value_or(0, 2));
                let mm = i32::from(scanned.value_or(1, 0));
                let ss = i32::from(scanned.value_or(2, 0));
                p = p.saturating_add(scanned.consumed.unwrap_or(0));
                let secs = hh * 3600 + mm * 60 + ss;
                rule.secs = if negative { -secs } else { secs };
            }
            _ => return false,
        }

        rule.computed_for = -1;
        *pos = p;
        true
    }

    /// `__tzset_parse_tz`: what glibc makes of a `TZ` value that names no
    /// file -- and, from [`tzfile_compute`], of a zoneinfo file's footer,
    /// for which `allow_default` is false (see the module docs).
    fn tzset_parse_tz(&mut self, tz: &[u8], allow_default: bool) {
        let s = tz.split(|&b| b == 0).next().unwrap_or(&[]);
        // "Clear out old state and reset to unnamed UTC."
        self.rules = [RULE_ZERO, RULE_ZERO];
        self.rules[0].name = Name::empty();
        self.rules[1].name = Name::empty();

        let mut p = 0usize;
        if self.parse_tzname(s, &mut p, 0) && self.parse_offset(s, &mut p, 0) {
            if at(s, p) == 0 {
                // "There is no DST."
                self.rules[1].name = self.rules[0].name;
                self.rules[1].offset = self.rules[0].offset;
            } else {
                if self.parse_tzname(s, &mut p, 1) {
                    self.parse_offset(s, &mut p, 1);
                    if allow_default
                        && (at(s, p) == 0 || (at(s, p) == b',' && at(s, p.saturating_add(1)) == 0))
                    {
                        // "There is no rule. See if there is a default rule
                        // file."
                        let [std, dst] = self.rules;
                        self.tzfile_default(std.name, dst.name, std.offset, dst.offset);
                        if self.use_tzfile {
                            self.old_tz_len = None;
                            return;
                        }
                    }
                }
                if self.parse_rule(s, &mut p, 0) {
                    self.parse_rule(s, &mut p, 1);
                }
            }
        }
        self.update_vars();
    }
}

/// `compute_offset`: each field clamped, then summed.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "each field is clamped first: at most 89999"
)]
fn compute_offset(ss: u16, mm: u16, hh: u16) -> i32 {
    i32::from(ss.min(59)) + i32::from(mm.min(59)) * 60 + i32::from(hh.min(24)) * 3600
}

// ---------------------------------------------------------------------------
// compute_change and __tz_compute
// ---------------------------------------------------------------------------

/// `SECSPERDAY`.
const SECSPERDAY: i64 = 86_400;

/// `__mon_yday`, flattened: days before each month of a normal year, then of
/// a leap year. glibc indexes it as `&__mon_yday[leap][m]` and reads `[-1]`
/// and `[0]` from there, so a malformed month walks into the other row before
/// it walks off the table. See [`mon_yday`].
const MON_YDAY: [u16; 26] = [
    0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334, 365, //
    0, 31, 60, 91, 121, 152, 182, 213, 244, 274, 305, 335, 366,
];

/// `__isleap`.
fn is_leap(year: i32) -> bool {
    year.rem_euclid(4) == 0 && (year.rem_euclid(100) != 0 || year.rem_euclid(400) == 0)
}

/// `(&__mon_yday[leap][m])[k]`, reading across the two rows as C does and
/// yielding 0 off either end of the table -- which only a rule that already
/// failed to parse can reach (glibc reads out of bounds there).
fn mon_yday(leap: bool, m: u16, k: i64) -> i64 {
    let row = if leap { 13 } else { 0 };
    i64::from(m)
        .checked_add(row)
        .and_then(|i| i.checked_add(k))
        .and_then(|i| usize::try_from(i).ok())
        .and_then(|i| MON_YDAY.get(i))
        .map_or(0, |&d| i64::from(d))
}

/// `compute_change`: when, in `year`, the change `rule` describes happens.
///
/// C's `int` arithmetic is kept, wrapping where C would overflow, because the
/// day count is computed in `int` before it is widened. And every year up to
/// 1970 starts from 1970-01-01 -- `t = 0` -- with only the weekday and the
/// leap day taken from the year itself.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "C's `int` arithmetic on bounded values: a month and weekday below 65536, and a year whose own additions are the wrapping ones"
)]
fn compute_change(rule: &mut Rule, year: i32) {
    if year != -1 && rule.computed_for == year {
        return;
    }
    let mut t: i64 = if year > 1970 {
        let y = year;
        let days = y
            .wrapping_sub(1970)
            .wrapping_mul(365)
            .wrapping_add(y.wrapping_sub(1) / 4 - 1970 / 4)
            .wrapping_sub(y.wrapping_sub(1) / 100 - 1970 / 100)
            .wrapping_add(y.wrapping_sub(1) / 400 - 1970 / 400);
        i64::from(days).wrapping_mul(SECSPERDAY)
    } else {
        0
    };
    let leap = is_leap(year);
    match rule.kind {
        Kind::J1 => {
            t = t.wrapping_add((i64::from(rule.d) - 1).wrapping_mul(SECSPERDAY));
            if rule.d >= 60 && leap {
                t = t.wrapping_add(SECSPERDAY);
            }
        }
        Kind::J0 => t = t.wrapping_add(i64::from(rule.d).wrapping_mul(SECSPERDAY)),
        Kind::M => {
            t = t.wrapping_add(mon_yday(leap, rule.m, -1).wrapping_mul(SECSPERDAY));
            // Zeller's congruence for the weekday of the month's first day.
            let m = i32::from(rule.m);
            let m1 = (m + 9) % 12 + 1;
            let yy0 = if rule.m <= 2 {
                year.wrapping_sub(1)
            } else {
                year
            };
            let yy1 = yy0 / 100;
            let yy2 = yy0 % 100;
            let mut dow = ((26 * m1 - 2) / 10 + 1)
                .wrapping_add(yy2)
                .wrapping_add(yy2 / 4)
                .wrapping_add(yy1 / 4)
                .wrapping_sub(yy1.wrapping_mul(2))
                % 7;
            if dow < 0 {
                dow += 7;
            }
            // The day of the month, from 0, of the first `d`-day; then a week
            // at a time while the month still has one.
            let mut d = i32::from(rule.d) - dow;
            if d < 0 {
                d += 7;
            }
            let month_days = mon_yday(leap, rule.m, 0) - mon_yday(leap, rule.m, -1);
            let mut i: u32 = 1;
            while i < u32::from(rule.n) {
                if i64::from(d) + 7 >= month_days {
                    break;
                }
                d = d.wrapping_add(7);
                i = i.saturating_add(1);
            }
            t = t.wrapping_add(i64::from(d).wrapping_mul(SECSPERDAY));
        }
    }
    rule.change = t
        .wrapping_sub(i64::from(rule.offset))
        .wrapping_add(i64::from(rule.secs));
    rule.computed_for = year;
}

impl State {
    /// `__tz_compute (timer, tm, 1)` -- after the `__offtime (timer, 0, tm)`
    /// that left `tm`'s year in place -- with only what the local half
    /// writes: `tm_isdst`, `tm_zone` (from `tzname`, not from the rule) and
    /// `tm_gmtoff`.
    fn tz_compute(&mut self, timer: i64, tm: &mut Tm) {
        let year = 1900i32.wrapping_add(tm.tm_year);
        compute_change(&mut self.rules[0], year);
        compute_change(&mut self.rules[1], year);
        let (c0, c1) = (self.rules[0].change, self.rules[1].change);
        // "We have to distinguish between northern and southern hemisphere.
        // For the latter the daylight saving time ends in the next year."
        let isdst = if c0 > c1 {
            timer < c1 || timer >= c0
        } else {
            timer >= c0 && timer < c1
        };
        tm.tm_isdst = i32::from(isdst);
        tm.tm_zone = pick(self.globals.tzname, isdst).ptr();
        tm.tm_gmtoff = i64::from(pick(self.rules, isdst).offset);
    }
}

// ---------------------------------------------------------------------------
// tzfile
// ---------------------------------------------------------------------------

/// `TZDEFRULES`.
const TZDEFRULES: &[u8] = b"posixrules";

impl State {
    /// `num_transitions`.
    fn num_transitions(&self) -> usize {
        self.file.map_or(0, |f| f.transition_count())
    }

    /// `num_types`: the file's, or 2 once `__tzfile_default` has rewritten it.
    fn num_types(&self) -> usize {
        if self.defaulted.is_some() {
            return 2;
        }
        self.file.map_or(0, |f| f.type_count())
    }

    /// `type_idxs[i]`.
    fn type_idx(&self, i: usize) -> usize {
        let Some(file) = self.file else { return 0 };
        let Some((_, ty)) = file.transition(i) else {
            return 0;
        };
        if self.defaulted.is_some() {
            return usize::from(file.local_type(ty).is_some_and(|t| t.is_dst));
        }
        ty
    }

    /// `transitions[i]`: the file's, or as `__tzfile_default` shifted it --
    /// worked out here rather than stored, since the shift depends only on
    /// this transition's type and the previous one's DST flag.
    fn transition(&self, i: usize) -> i64 {
        let Some(file) = self.file else { return 0 };
        let Some((t, ty)) = file.transition(i) else {
            return 0;
        };
        let Some(def) = self.defaulted else { return t };
        let Some(this) = file.local_type(ty) else {
            return t;
        };
        let prev_dst = i
            .checked_sub(1)
            .and_then(|j| file.transition(j))
            .and_then(|(_, p)| file.local_type(p))
            .is_some_and(|p| p.is_dst);
        if this.is_ut {
            // "The transition time is in GMT. No correction to apply."
            t
        } else if prev_dst && !this.is_std {
            t.wrapping_add(def.shift_dst)
        } else {
            t.wrapping_add(def.shift_std)
        }
    }

    /// `types[j]`: its offset, DST flag and name.
    fn ttype(&self, j: usize) -> (i32, bool, Option<Name>) {
        if let Some(def) = self.defaulted {
            let dst = j >= 1;
            return (pick(def.offsets, dst), dst, Some(pick(def.names, dst)));
        }
        let Some(lt) = self.file.and_then(|f| f.local_type(j)) else {
            return (0, false, None);
        };
        (lt.utoff, lt.is_dst, tzstring(lt.name.as_bytes()))
    }

    /// `__tzfile_read (file, 0, NULL)`: read the zoneinfo file `name`
    /// names, absolute or under `TZDIR`; `use_tzfile` says whether it was.
    /// `None` is `TZDEFAULT`.
    fn tzfile_read(&mut self, name: Option<&[u8]>) {
        let was_using_tzfile = self.use_tzfile;
        self.use_tzfile = false;
        let name = match name {
            None => tzrules::LOCALTIME,
            // "User specified the empty string; use UTC with no leap seconds."
            Some([]) => return self.free_transitions(),
            Some(n) => n,
        };
        let Some(path) = zone_path(name) else {
            return self.free_transitions();
        };
        // "If we were already using tzfile, check whether the file changed."
        let id = file_id(path.as_bytes());
        if was_using_tzfile && id.is_some() && id == self.file_id {
            self.use_tzfile = true;
            return;
        }
        let Some((file, id)) = load_zone_file(path.as_bytes()) else {
            return self.free_transitions();
        };
        self.file = Some(file);
        self.file_id = Some(id);
        self.defaulted = None;
        self.tzspec = file.footer().filter(|f| !f.is_empty());

        // "First 'register' all time zone abbreviations."
        for j in 0..file.type_count() {
            if self.ttype(j).2.is_none() {
                return self.free_transitions();
            }
        }

        // "Find the standard and daylight time offsets used by the rule file.
        // We choose the offsets in the types of each flavor that are
        // transitioned to earliest in time."
        let n = self.num_transitions();
        let mut tzname: [Option<Name>; 2] = [None, None];
        let mut i = n;
        while i > 0 {
            i = i.saturating_sub(1);
            let (_, dst, name) = self.ttype(self.type_idx(i));
            if slot(&mut tzname, dst).is_none() {
                *slot(&mut tzname, dst) = name;
                if pick(tzname, !dst).is_some() {
                    break;
                }
            }
        }
        let first = self.ttype(0).2.unwrap_or_else(Name::empty);
        let t0 = tzname[0].unwrap_or(first);
        let t1 = tzname[1].unwrap_or(t0);
        self.globals.tzname = [t0, t1];

        self.daylight_saved = false;
        if n == 0 {
            // "Use the first rule (which should also be the only one)."
            let offset = i64::from(self.ttype(0).0);
            self.rule_stdoff = offset;
            self.rule_dstoff = offset;
        } else {
            self.rule_stdoff = 0;
            // "Search for the last rule with a standard time offset."
            let mut i = n;
            let mut found = false;
            while i > 0 {
                i = i.saturating_sub(1);
                let (offset, dst, _) = self.ttype(self.type_idx(i));
                if !dst {
                    self.rule_stdoff = i64::from(offset);
                    found = true;
                    break;
                }
                self.daylight_saved = true;
            }
            // "Keep searching to see if there is a DST rule."
            if found {
                while i > 0 && !self.daylight_saved {
                    i = i.saturating_sub(1);
                    self.daylight_saved = self.ttype(self.type_idx(i)).1;
                }
            }
        }
        self.globals.daylight = i32::from(self.daylight_saved);
        self.globals.timezone = self.rule_stdoff.wrapping_neg();
        self.use_tzfile = true;
    }

    /// `ret_free_transitions`: no file is in use.
    fn free_transitions(&mut self) {
        self.file = None;
        self.file_id = None;
        self.defaulted = None;
        self.tzspec = None;
    }

    /// `__tzfile_default`: `posixrules`' history under the user's names and
    /// offsets, if that file can stand in (two types or more).
    fn tzfile_default(&mut self, std: Name, dst: Name, stdoff: i32, dstoff: i32) {
        self.tzfile_read(Some(TZDEFRULES));
        if !self.use_tzfile {
            return;
        }
        if self.num_types() < 2 {
            self.use_tzfile = false;
            return;
        }
        self.defaulted = Some(Defaulted {
            names: [std, dst],
            offsets: [stdoff, dstoff],
            shift_std: i64::from(stdoff).wrapping_sub(self.rule_stdoff),
            shift_dst: i64::from(dstoff).wrapping_sub(self.rule_dstoff),
        });
        self.rule_stdoff = i64::from(stdoff);
        self.rule_dstoff = i64::from(dstoff);
        self.globals.tzname = [std, dst];
        self.globals.timezone = i64::from(stdoff).wrapping_neg();
        // "Invalidate the tzfile attribute cache to force rereading
        // TZDEFRULES the next time it is used."
        self.file_id = None;
    }

    /// `__tzfile_compute (timer, 1, …, tp)` without the leap seconds (see the
    /// module docs): the type in force at `timer`, into `tp`'s zone fields,
    /// and `tzname` moved to the names around it. `false` where glibc's
    /// footer path runs `__offtime` and it fails -- which the caller then
    /// fails too.
    fn tzfile_compute(&mut self, timer: i64, tp: &mut Tm) {
        let n = self.num_transitions();
        let ntypes = self.num_types();
        let mut tzname: [Option<Name>; 2] = [None, None];
        let i: usize;
        if n == 0 || timer < self.transition(0) {
            // "TIMER is before any transition (or there are no transitions).
            // Choose the first non-DST type (or the first if they're all DST
            // types)."
            let mut k = 0usize;
            while k < ntypes && self.ttype(k).1 {
                if tzname[1].is_none() {
                    tzname[1] = self.ttype(k).2;
                }
                k = k.saturating_add(1);
            }
            if k == ntypes {
                k = 0;
            }
            tzname[0] = self.ttype(k).2;
            if tzname[1].is_none() {
                let mut j = k;
                while j < ntypes {
                    if self.ttype(j).1 {
                        tzname[1] = self.ttype(j).2;
                        break;
                    }
                    j = j.saturating_add(1);
                }
            }
            i = k;
        } else if timer >= self.transition(n.saturating_sub(1)) {
            if let Some(spec) = self.tzspec {
                // "Parse the POSIX TZ-style string."
                self.tzset_parse_tz(spec, false);
                // "Convert to broken down structure. If this fails do not use
                // the string."
                if let Some(b) = crate::time::Broken::of(timer, 0) {
                    b.store(tp);
                    // "Use the rules from the TZ string to compute the
                    // change."
                    self.tz_compute(timer, tp);
                    // glibc then compares `zone_names` with an address its
                    // allocator never gives it, to put back a `posixrules`
                    // zone's own names -- so it never does.
                    return;
                }
            }
            // `use_last`.
            i = self.found(n, &mut tzname);
        } else {
            let k = self.search(timer);
            i = self.found(k, &mut tzname);
        }

        let (offset, isdst, name) = self.ttype(i);
        self.globals.daylight = i32::from(self.daylight_saved);
        self.globals.timezone = self.rule_stdoff.wrapping_neg();
        let t0 = tzname[0].or(tzname[1]).or(name).unwrap_or_else(Name::empty);
        let t1 = tzname[1].unwrap_or(t0);
        self.globals.tzname = [t0, t1];
        tp.tm_isdst = i32::from(isdst);
        tp.tm_zone = pick(self.globals.tzname, isdst).ptr();
        tp.tm_gmtoff = i64::from(offset);
    }

    /// `found:` with `i` the first transition after the instant: `tzname`
    /// from the type before it and those after, and that type's index.
    fn found(&self, i: usize, tzname: &mut [Option<Name>; 2]) -> usize {
        let Some(before) = i.checked_sub(1) else {
            return 0;
        };
        let ty = self.type_idx(before);
        let (_, dst, name) = self.ttype(ty);
        *slot(tzname, dst) = name;
        let n = self.num_transitions();
        let mut j = i;
        while j < n {
            let (_, dst, name) = self.ttype(self.type_idx(j));
            if slot(tzname, dst).is_none() {
                *slot(tzname, dst) = name;
                if pick(*tzname, !dst).is_some() {
                    break;
                }
            }
            j = j.saturating_add(1);
        }
        if tzname[0].is_none() {
            tzname[0] = tzname[1];
        }
        ty
    }

    /// The index `i` with `transitions[i - 1] <= t < transitions[i]`, by
    /// `__tzfile_compute`'s own search: a guess from "DST changes twice a
    /// year", a linear walk if it is within ten, a binary search otherwise.
    /// As written, so that a table `__tzfile_default` left out of order gets
    /// glibc's answer. Requires `transitions[0] <= t < transitions[n - 1]`.
    fn search(&self, t: i64) -> usize {
        let n = self.num_transitions();
        let get = |i: usize| if i < n { self.transition(i) } else { i64::MAX };
        let mut lo = 0usize;
        let mut hi = n.saturating_sub(1);
        let last = get(n.saturating_sub(1));
        // `size_t` of a possibly negative difference: huge, failing `< n`.
        let guess = usize::try_from(last.wrapping_sub(t) / 15_778_476).unwrap_or(usize::MAX);
        if guess < n {
            let mut i = n.saturating_sub(1).saturating_sub(guess);
            if t < get(i) {
                if i < 10 || t >= get(i.saturating_sub(10)) {
                    while i > 0 && t < get(i.saturating_sub(1)) {
                        i = i.saturating_sub(1);
                    }
                    return i;
                }
                hi = i.saturating_sub(10);
            } else {
                if i.saturating_add(10) >= n || t < get(i.saturating_add(10)) {
                    while i < n && t >= get(i) {
                        i = i.saturating_add(1);
                    }
                    return i;
                }
                lo = i.saturating_add(10);
            }
        }
        while lo.saturating_add(1) < hi {
            let i = lo.saturating_add(hi) / 2;
            if t < get(i) {
                hi = i;
            } else {
                lo = i;
            }
        }
        hi
    }
}

// ---------------------------------------------------------------------------
// Reading a zoneinfo file
// ---------------------------------------------------------------------------

/// A NUL-terminated path, built in place.
struct ZonePath {
    buf: [u8; crate::unistd::PATH_MAX],
    len: usize,
}

impl ZonePath {
    fn as_bytes(&self) -> &[u8] {
        self.buf.get(..=self.len).unwrap_or(&[])
    }
}

/// The path glibc opens for `name`: itself when absolute, else under
/// `TZDIR` -- or the default directory, when `TZDIR` is unset or empty --
/// joined with one `/`, a trailing one in `TZDIR` kept as glibc keeps it.
/// `None` for a name this library refuses: one with a `..` component, and in
/// a set-user-ID program (`AT_SECURE`) any absolute path but `/etc/localtime`
/// and the default directory's, with `TZDIR` ignored there as the loader
/// ignores it.
fn zone_path(name: &[u8]) -> Option<ZonePath> {
    if name.contains(&0) || name.split(|&b| b == b'/').any(|part| part == b"..") {
        return None;
    }
    let secure = crate::crt::getauxval(crate::linux_auxv_types::AT_SECURE.into()) != 0;
    let mut out = ZonePath {
        buf: [0; crate::unistd::PATH_MAX],
        len: 0,
    };
    let mut push = |bytes: &[u8]| -> Option<()> {
        let end = out.len.checked_add(bytes.len())?;
        out.buf.get_mut(out.len..end)?.copy_from_slice(bytes);
        out.len = end;
        Some(())
    };
    if name.first() == Some(&b'/') {
        if secure && name != tzrules::LOCALTIME && !name.starts_with(tzrules::ZONEINFO_DIR) {
            return None;
        }
        push(name)?;
    } else {
        let dir = crate::environ::getenv_bytes(b"TZDIR")
            .filter(|d| !d.is_empty() && !secure)
            .unwrap_or(tzrules::ZONEINFO_DIR);
        push(dir)?;
        push(b"/")?;
        push(name)?;
    }
    *out.buf.get_mut(out.len)? = 0;
    Some(out)
}

/// Capacity of [`zone_file`]: four times tzdata's largest file. A larger one
/// is refused rather than cut short: half a transition table renders times
/// confidently wrong.
const ZONE_FILE_CAP: usize = 16 * 1024;

crate::perprocess::process_global! {
    /// The bytes of the zoneinfo file in use, which [`TzFile`] reads in
    /// place: the process's, or on the host this test thread's.
    fn zone_file() -> [u8; ZONE_FILE_CAP] = [0; ZONE_FILE_CAP];
}

/// A file's identity, as glibc compares one: device, inode, modification time.
type FileId = (u64, u64, i64);

/// The host build's file calls are `ENOSYS`; its tests read from a table.
#[cfg(test)]
use tests::{file_id, load_zone_file};

/// The identity of the file at the NUL-terminated `path`, if it can be
/// `stat`ed.
#[cfg(not(test))]
fn file_id(path: &[u8]) -> Option<FileId> {
    let mut st = crate::stat::Stat::zeroed();
    if crate::file::stat(path.as_ptr(), &raw mut st) != 0 {
        return None;
    }
    Some((st.st_dev, st.st_ino, st.st_mtim.tv_sec))
}

/// Read the NUL-terminated `path` into [`zone_file`] and parse it: the view,
/// and the file's identity. `None` if it cannot be read, does not fit, or is
/// not TZif.
#[cfg(not(test))]
fn load_zone_file(path: &[u8]) -> Option<(TzFile<'static>, FileId)> {
    let fd = crate::file::open(
        path.as_ptr(),
        crate::fcntl::O_RDONLY | crate::fcntl::O_CLOEXEC,
        0,
    );
    if fd < 0 {
        return None;
    }
    let mut st = crate::stat::Stat::zeroed();
    let id = (crate::file::fstat(fd, &raw mut st) == 0).then_some((
        st.st_dev,
        st.st_ino,
        st.st_mtim.tv_sec,
    ));
    // SAFETY: called with the lock held, which guards the buffer.
    let buf = unsafe { &mut *zone_file() };
    let mut len = 0usize;
    let ok = loop {
        let Some(rest) = buf.get_mut(len..) else {
            break false;
        };
        if rest.is_empty() {
            // Full: one more byte means too big.
            let mut probe = [0u8; 1];
            break crate::file::read(fd, probe.as_mut_ptr(), 1) == 0;
        }
        let n = crate::file::read(fd, rest.as_mut_ptr(), rest.len());
        let Ok(n) = usize::try_from(n) else {
            break false;
        };
        if n == 0 {
            break true;
        }
        match len.checked_add(n) {
            Some(next) if next <= buf.len() => len = next,
            _ => break false,
        }
    };
    crate::file::close(fd);
    if !ok {
        return None;
    }
    Some((parse_zone_file(len)?, id?))
}

/// Parse the first `len` bytes of [`zone_file`].
fn parse_zone_file(len: usize) -> Option<TzFile<'static>> {
    // SAFETY: an immutable view of the buffer the loader just filled, which
    // the next load -- under the same lock -- is the only thing to change.
    let bytes = unsafe { &*zone_file() };
    TzFile::parse(bytes.get(..len)?)
}

// ---------------------------------------------------------------------------
// tzset_internal, tzset, __tz_convert
// ---------------------------------------------------------------------------

impl State {
    /// `tzset_internal (always)`.
    fn tzset_internal(&mut self, always: bool) {
        if self.is_initialized && !always {
            return;
        }
        self.is_initialized = true;

        let env = crate::environ::getenv_bytes(b"TZ");
        let tz: Option<&[u8]> = match env {
            // "User specified the empty string; use UTC explicitly."
            Some([]) => Some(b"Universal"),
            other => other,
        };
        // "A leading colon means 'implementation defined syntax'. We ignore
        // the colon and always use the same algorithm: try a data file, and if
        // none exists parse the 1003.1 syntax."
        let tz = tz.map(|v| v.strip_prefix(b":").unwrap_or(v));

        // "Check whether the value changed since the last run."
        if let (Some(v), Some(len)) = (tz, self.old_tz_len)
            && self.old_tz.get(..len) == Some(v)
        {
            return;
        }

        self.rules[0].name = Name::NULL;
        self.rules[1].name = Name::NULL;

        // "Save the value of `tz'." -- TZDEFAULT's name when unset.
        let saved = tz.unwrap_or(tzrules::LOCALTIME);
        self.old_tz_len = self.old_tz.get_mut(..saved.len()).map(|slot| {
            slot.copy_from_slice(saved);
            saved.len()
        });

        // "Try to read a data file."
        self.tzfile_read(tz);
        if self.use_tzfile {
            return;
        }

        // "No data file found. Default to UTC if nothing specified."
        let name = tz.unwrap_or(tzrules::LOCALTIME);
        if name.is_empty() || name == tzrules::LOCALTIME {
            self.rules = [RULE_ZERO, RULE_ZERO];
            self.rules[0].name = Name::utc();
            self.rules[1].name = Name::utc();
            self.rules[0].change = -1;
            self.rules[1].change = -1;
            self.update_vars();
            return;
        }
        self.tzset_parse_tz(name, true);
    }
}

/// Write the zone's globals out to the variables C reads.
fn publish(globals: &Globals) {
    crate::time::set_tz_globals(
        globals.tzname[0].ptr(),
        globals.tzname[1].ptr(),
        globals.timezone,
        globals.daylight,
    );
}

/// `tzset`: read `TZ` again, and set `tzname`, `timezone` and `daylight`.
pub fn tzset() {
    let mut lock = Locked::take();
    let state = lock.state();
    state.tzset_internal(true);
    if !state.use_tzfile {
        // "Set `tzname'."
        state.globals.tzname = [state.rules[0].name, state.rules[1].name];
    }
    publish(&state.globals);
}

/// `__tz_convert (timer, 1, tp)`: `timer` as local time, into `tm`. `always`
/// is `localtime`'s re-reading of `TZ`, which `localtime_r` does not do.
/// `false`, with `tm` partly written as glibc leaves it, when the year does
/// not fit `tm_year` -- glibc's `EOVERFLOW`, for the caller to set.
pub fn localtime(timer: i64, tm: &mut Tm, always: bool) -> bool {
    let mut lock = Locked::take();
    let ok = lock.state().localtime_locked(timer, tm, always);
    publish(&lock.state().globals);
    ok
}

/// What `gmtime` does to the zone, as `__tz_convert (timer, 0, tp)` does it:
/// read it, if it never has been -- which sets `tzname` -- and, in a rule
/// zone, work out the changes of `timer`'s year, which moves
/// `compute_change`'s cache.
pub fn gmtime_touch(timer: i64) {
    let mut lock = Locked::take();
    let state = lock.state();
    state.tzset_internal(false);
    if !state.use_tzfile
        && let Some(b) = crate::time::Broken::of(timer, 0)
    {
        let mut tm = Tm::ZERO;
        b.store(&mut tm);
        let year = 1900i32.wrapping_add(tm.tm_year);
        compute_change(&mut state.rules[0], year);
        compute_change(&mut state.rules[1], year);
    }
    publish(&state.globals);
}

impl State {
    fn localtime_locked(&mut self, timer: i64, tm: &mut Tm, always: bool) -> bool {
        self.tzset_internal(always);
        if self.use_tzfile {
            self.tzfile_compute(timer, tm);
        } else {
            let Some(b) = crate::time::Broken::of(timer, 0) else {
                return false;
            };
            b.store(tm);
            self.tz_compute(timer, tm);
        }
        // `__offtime (timer, tp->tm_gmtoff - leap_correction, tp)`.
        match crate::time::Broken::of(timer, tm.tm_gmtoff) {
            Some(b) => {
                b.store(tm);
                true
            }
            None => false,
        }
    }
}

// ---------------------------------------------------------------------------
// mktime: __mktime_internal
// ---------------------------------------------------------------------------

/// `TM_YEAR_BASE` and `EPOCH_YEAR`.
const TM_YEAR_BASE: i64 = 1900;
const EPOCH_YEAR: i64 = 1970;

/// `__mon_yday` as `mktime.c` declares it.
const MKTIME_MON_YDAY: [[i64; 13]; 2] = [
    [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334, 365],
    [0, 31, 60, 91, 121, 152, 182, 213, 244, 274, 305, 335, 366],
];

/// `leapyear (year)`: is `year + 1900` a leap year, without the addition.
fn leapyear(year: i64) -> bool {
    year.trailing_zeros() >= 2
        && (year % 100 != 0 || ((year / 100) & 3) == (-(TM_YEAR_BASE / 100) & 3))
}

/// `isdst_differ`: one zero and the other positive.
fn isdst_differ(a: i32, b: i32) -> bool {
    (a == 0) != (b == 0) && a >= 0 && b >= 0
}

/// `shr`: an arithmetic right shift, which Rust's `>>` on `i64` is.
fn shr(a: i64, b: u32) -> i64 {
    a >> b
}

/// `ydhms_diff`: seconds from (`year0`, `yday0`, …) to (`year1`, `yday1`, …),
/// counting the leap days between, with no clock changes assumed.
#[allow(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::too_many_arguments,
    reason = "glibc's own arithmetic, and its signature,, whose bounds mktime.c states and keeps: years within int, and the products in long"
)]
fn ydhms_diff(
    year1: i64,
    yday1: i64,
    hour1: i32,
    min1: i32,
    sec1: i32,
    year0: i32,
    yday0: i32,
    hour0: i32,
    min0: i32,
    sec0: i32,
) -> i64 {
    // "Compute intervening leap days correctly even if year is negative."
    let a4 = (shr(year1, 2) + shr(TM_YEAR_BASE, 2) - i64::from(year1.trailing_zeros() >= 2)) as i32;
    let b4 = (shr(i64::from(year0), 2) + shr(TM_YEAR_BASE, 2)
        - i64::from(year0.trailing_zeros() >= 2)) as i32;
    let a100 = (a4 + i32::from(a4 < 0)) / 25 - i32::from(a4 < 0);
    let b100 = (b4 + i32::from(b4 < 0)) / 25 - i32::from(b4 < 0);
    let a400 = a100 >> 2;
    let b400 = b100 >> 2;
    let intervening = i64::from((a4 - b4) - (a100 - b100) + (a400 - b400));
    let years = year1 - i64::from(year0);
    let days = 365 * years + yday1 - i64::from(yday0) + intervening;
    let hours = 24 * days + i64::from(hour1) - i64::from(hour0);
    let minutes = 60 * hours + i64::from(min1) - i64::from(min0);
    60 * minutes + i64::from(sec1) - i64::from(sec0)
}

/// `long_int_avg`, rounding toward positive infinity.
#[allow(clippy::arithmetic_side_effects, reason = "halves cannot overflow")]
fn long_int_avg(a: i64, b: i64) -> i64 {
    shr(a, 1) + shr(b, 1) + ((a | b) & 1)
}

/// `tm_diff`.
fn tm_diff(year: i64, yday: i64, hour: i32, min: i32, sec: i32, tp: &Tm) -> i64 {
    ydhms_diff(
        year, yday, hour, min, sec, tp.tm_year, tp.tm_yday, tp.tm_hour, tp.tm_min, tp.tm_sec,
    )
}

impl State {
    /// `convert_time` with `__localtime64_r`.
    fn convert(&mut self, t: i64, tm: &mut Tm) -> bool {
        self.localtime_locked(t, tm, false)
    }

    /// `ranged_convert`: `t` converted, or the nearest instant that converts;
    /// `None` on failure. Every failure here is `EOVERFLOW`.
    fn ranged_convert(&mut self, t: &mut i64, tp: &mut Tm) -> bool {
        if self.convert(*t, tp) {
            return true;
        }
        let mut bad = *t;
        let mut ok = 0i64;
        let mut oktm: Option<Tm> = None;
        // "Use binary search to narrow the range between BAD and OK until
        // they differ by 1."
        loop {
            let mid = long_int_avg(ok, bad);
            if mid == ok || mid == bad {
                break;
            }
            if self.convert(mid, tp) {
                ok = mid;
                oktm = Some(*tp);
            } else {
                bad = mid;
            }
        }
        match oktm {
            Some(found) => {
                *t = ok;
                *tp = found;
                true
            }
            None => false,
        }
    }

    /// `__mktime_internal (tp, __localtime64_r, &localtime_offset)`: the
    /// instant `tp` names, `tp` normalised; `None`, `tp` untouched, for
    /// glibc's `EOVERFLOW`.
    #[allow(
        clippy::arithmetic_side_effects,
        clippy::too_many_lines,
        reason = "glibc's function, kept whole and with its own arithmetic"
    )]
    fn mktime(&mut self, tp: &mut Tm) -> Option<i64> {
        let mut tm = *tp;
        let mut remaining_probes = 6;

        let mut sec = tp.tm_sec;
        let min = tp.tm_min;
        let hour = tp.tm_hour;
        let mday = tp.tm_mday;
        let mon = tp.tm_mon;
        let year_requested = tp.tm_year;
        let isdst = tp.tm_isdst;

        let mut dst2 = false;

        // "Ensure that mon is in range, and set year accordingly."
        let mon_remainder = mon % 12;
        let negative_mon_remainder = mon_remainder < 0;
        let mon_years = mon / 12 - i32::from(negative_mon_remainder);
        let year = i64::from(year_requested) + i64::from(mon_years);

        // "Calculate day of year from year, month, and day of month."
        let month_index =
            usize::try_from(mon_remainder + 12 * i32::from(negative_mon_remainder)).ok()?;
        let mon_yday = MKTIME_MON_YDAY
            .get(usize::from(leapyear(year)))?
            .get(month_index)
            .copied()?
            - 1;
        let yday = mon_yday + i64::from(mday);

        let off = self.localtime_offset;
        let sec_requested = sec;
        // LEAP_SECONDS_POSSIBLE: "Handle out-of-range seconds specially."
        sec = sec.clamp(0, 59);

        // "Invert CONVERT by probing. First assume the same offset as last
        // time."
        let negative_offset_guess = (0i64.wrapping_sub(off)) as i32;
        let t0 = ydhms_diff(
            year,
            yday,
            hour,
            min,
            sec,
            (EPOCH_YEAR - TM_YEAR_BASE) as i32,
            0,
            0,
            0,
            negative_offset_guess,
        );
        let (mut t, mut t1, mut t2) = (t0, t0, t0);

        let mut matched = false;
        loop {
            if !self.ranged_convert(&mut t, &mut tm) {
                return None;
            }
            let dt = tm_diff(year, yday, hour, min, sec, &tm);
            if dt == 0 {
                matched = true;
                break;
            }
            if t == t1
                && t != t2
                && (tm.tm_isdst < 0
                    || (if isdst < 0 {
                        dst2 <= (tm.tm_isdst != 0)
                    } else {
                        (isdst != 0) != (tm.tm_isdst != 0)
                    }))
            {
                // "We can't possibly find a match, as we are oscillating
                // between two values. The requested time probably falls
                // within a spring-forward gap of size DT."
                break;
            }
            remaining_probes -= 1;
            if remaining_probes == 0 {
                return None;
            }
            t1 = t2;
            t2 = t;
            t += dt;
            dst2 = tm.tm_isdst != 0;
        }

        // "We have a match. Check whether tm.tm_isdst has the requested
        // value, if any."
        if matched && isdst_differ(isdst, tm.tm_isdst) {
            let dst_difference = i64::from(isdst == 0) - i64::from(tm.tm_isdst == 0);
            let stride: i64 = 601_200;
            let duration_max: i64 = 457_243_209;
            let delta_bound = duration_max / 2 + stride;
            let mut found = false;
            let mut delta = stride;
            'probe: while delta < delta_bound {
                for direction in [-1i64, 1] {
                    let Some(mut ot) = t.checked_add(delta * direction) else {
                        continue;
                    };
                    let mut otm = tm;
                    if !self.ranged_convert(&mut ot, &mut otm) {
                        return None;
                    }
                    if !isdst_differ(isdst, otm.tm_isdst) {
                        // "We found the desired tm_isdst. Extrapolate back
                        // to the desired time."
                        let Some(gt) = ot.checked_add(tm_diff(year, yday, hour, min, sec, &otm))
                        else {
                            continue;
                        };
                        if self.convert(gt, &mut tm) {
                            t = gt;
                            found = true;
                            break 'probe;
                        }
                    }
                }
                delta += stride;
            }
            if !found {
                // "No unusual DST offset was found nearby. Assume one-hour
                // DST."
                t += 3600 * dst_difference;
                if !self.convert(t, &mut tm) {
                    return None;
                }
            }
        }

        // "Set *OFFSET to the low-order bits of T - T0 - NEGATIVE_OFFSET_GUESS."
        self.localtime_offset = t
            .wrapping_sub(t0)
            .wrapping_sub(i64::from(negative_offset_guess));

        if sec_requested != tm.tm_sec {
            // "Adjust time to reflect the tm_sec requested, not the
            // normalized value."
            let mut sec_adjustment = i64::from(sec == 0 && tm.tm_sec == 60);
            sec_adjustment -= i64::from(sec);
            sec_adjustment += i64::from(sec_requested);
            t = t.checked_add(sec_adjustment)?;
            if !self.convert(t, &mut tm) {
                return None;
            }
        }
        *tp = tm;
        Some(t)
    }
}

/// `mktime`: `tzset`, then the search, with `tzname` left where its last
/// conversion moved it. `None`, `tm` untouched, for `EOVERFLOW`.
pub fn mktime(tm: &mut Tm) -> Option<i64> {
    tzset();
    let mut lock = Locked::take();
    let result = lock.state().mktime(tm);
    publish(&lock.state().globals);
    result
}

// ---------------------------------------------------------------------------
// Tests' access
// ---------------------------------------------------------------------------

/// Put the state back to a fresh process's, for tests that replay glibc's
/// answers one process at a time.
#[cfg(test)]
pub(crate) fn reset_for_test() {
    let mut lock = Locked::take();
    *lock.state() = State::FRESH;
    // SAFETY: the lock is held; the arena's names from earlier tests are
    // dropped with the state that pointed at them.
    unsafe {
        *arena_used_storage() = 0;
    }
    publish(&lock.state().globals);
}

#[cfg(test)]
mod tests;
