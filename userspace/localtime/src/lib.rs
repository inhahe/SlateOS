//! The local timezone, resolved once, and the calendar it implies.
//!
//! [`tzrules`] answers one question — *what offset is in force at this
//! instant?* — and stops there, deliberately: it is `no_std` and links into the
//! libc, so it can neither read a file nor allocate. Everything between that
//! answer and a printed timestamp was therefore left to the caller, and by the
//! time this crate was written three callers had each supplied it:
//!
//! | Caller | What it wrote privately |
//! |---|---|
//! | `userspace/oils` | `ShellZone`/`Zone`, `zoneinfo_path`, a `strftime` |
//! | `userspace/hwclock` | `TZ` parsing, no file support at all |
//! | `posix/src/tz.rs` | `resolve_tz_value`, for the libc's own `tzset` |
//!
//! That is the shape of every duplication this tree has found so far: not one
//! program doing it wrong, but several doing it *differently*, so that `date`,
//! `ls -l` and the shell's `\t` prompt can disagree about what time it is on
//! the same machine at the same instant. This crate is the one copy.
//!
//! # What "resolve `TZ`" actually means
//!
//! What glibc 2.39's `tzset` does, which is not guessable and not POSIX's —
//! the `tzset` module is the port, function by function, and says why each
//! detail is visible:
//!
//! 1. **Unset `TZ` is not UTC** — it is `/etc/localtime`, a TZif file, and UTC
//!    (named `UTC`) only if that cannot be read. A program that answers UTC for
//!    an unset `TZ` prints the wrong hour on every desktop in the world, and
//!    prints it *silently*.
//! 2. **Empty `TZ` is the name `Universal`**, which is then resolved like any
//!    other value: the zoneinfo file of that name if there is one, else UTC
//!    under that name.
//! 3. **A leading `:` is dropped**, and means nothing more: `:EST5EDT` and
//!    `EST5EDT` are the same value.
//! 4. **A file is tried before a rule.** The order is observable, because
//!    `EST5EDT` is *both* a POSIX rule and a file in every zoneinfo tree — and
//!    the two do not agree, since the rule cannot know that the United States
//!    moved the start of daylight saving in 2007.
//! 5. **A rule is glibc's even where it is odd**: what parses of it is kept
//!    (`Foo/Bar` is UTC named `Foo`), a DST name with no dates borrows
//!    `posixrules`' history, and every year up to 1970 changes on 1970's dates.
//! 6. **A zone name with a `..` component is never read as a file** — here
//!    always, where glibc does so only in a setuid program. It falls through to
//!    the POSIX rule instead, as a missing file does.
//!
//! # Why the calendar arithmetic lives here too
//!
//! Because [`Tm`] is what a `strftime` needs, and a `strftime` is what every
//! caller was really trying to write. Splitting the two would leave each caller
//! to turn "seconds east of Greenwich" into a day of the week — which is where
//! the off-by-one lives, since 1970-01-01 was a **Thursday** and the obvious
//! `days % 7` makes it a Sunday.
//!
//! # And `mktime`, the way glibc does it
//!
//! [`Zone::epoch`] inverts [`Zone::local`] for callers that want an instant
//! that always exists. A caller porting C that calls `mktime` wants something
//! else — glibc's answers for the skipped and repeated hours, its handling of
//! an explicit `tm_isdst`, and its failures — and gets them from
//! [`Zone::mktime`] and [`Zone::localtime_r`] over a C-shaped [`StructTm`]. See
//! the `mktime` module for why each of those is observable.

use std::path::{Path, PathBuf};
use std::sync::{PoisonError, RwLock};

use tzrules::{TzInfo, TzName};

mod mktime;
pub use mktime::{StructTm, with_mktime_offset};

mod strftime;
pub use strftime::{nstrftime, nstrftime_z, strftime};

mod tzset;
use tzset::Engine;

/// Where a bare zone name is looked up when `TZDIR` says nothing.
pub const TZDIR_DEFAULT: &str = "/usr/share/zoneinfo";

/// The file an unset `TZ` means.
pub const LOCALTIME_PATH: &str = "/etc/localtime";

/// An owned timezone, as glibc holds one: POSIX rules, or a zoneinfo file's
/// tables, copied out of the file when it is read.
///
/// Owned because the something a utility wants is a value it can keep for the
/// length of a listing — and copied rather than re-read from the file's bytes
/// per lookup, because `ls -l` asks once per line.
///
/// # When it is read, and read again
///
/// A zone made from a `TZ` value is read at its first use, not when it is
/// made, and can be read again in place — because glibc's reads are
/// observable, in their order and their number. Its one zone state is filled
/// in at the first conversion, re-read whenever `TZ` changes (every `TZ="…"`
/// in a date string changes it twice), and re-read by every `mktime` if the
/// zone came from `posixrules`. Each read can move the process-wide value the
/// next `posixrules` zone is anchored by; see the `tzset` module. So:
///
/// * [`Zone::lookup`] and everything built on it read the zone if nothing has
///   yet, as `localtime_r` does;
/// * [`Zone::tzset`] is glibc's `tzset ()`, which [`Zone::mktime`] calls as
///   glibc's `mktime` does;
/// * [`Zone::reread`] is a switch of `TZ` to this zone, and [`Zone::switched`]
///   is gnulib's pair of them around one conversion in another zone.
#[derive(Debug)]
pub struct Zone {
    /// `None` until the zone is first read.
    engine: RwLock<Option<Engine>>,
    source: Source,
}

/// What a [`Zone`] is read from.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Source {
    /// A `TZ` value (`None`: unset), and the zoneinfo tree and default file it
    /// is read against.
    Tz {
        tz: Option<Vec<u8>>,
        dir: String,
        localtime: PathBuf,
    },
    /// Not a `TZ` value — [`Zone::utc`], [`Zone::from_file`] — and so never
    /// read again.
    Fixed,
}

impl Clone for Zone {
    fn clone(&self) -> Self {
        let engine = self
            .engine
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        Self {
            engine: RwLock::new(engine),
            source: self.source.clone(),
        }
    }
}

impl Default for Zone {
    fn default() -> Self {
        Self::utc()
    }
}

impl Zone {
    /// A zone that is exactly `engine`, and is never read again.
    fn fixed(engine: Engine) -> Self {
        Self {
            engine: RwLock::new(Some(engine)),
            source: Source::Fixed,
        }
    }

    /// UTC, named `UTC`: what glibc falls back to when neither `TZ` nor
    /// `/etc/localtime` gives it anything to read.
    #[must_use]
    pub fn utc() -> Self {
        Self::fixed(Engine::Rules(tzset::Rules::utc()))
    }

    /// Read the zone from its source: glibc's `tzset_internal`.
    fn read(&self) -> Engine {
        match &self.source {
            Source::Tz { tz, dir, localtime } => tzset::resolve(tz.as_deref(), dir, localtime),
            // Unreachable: a fixed zone is built with its engine, and
            // `reread` leaves it alone.
            Source::Fixed => Engine::Rules(tzset::Rules::utc()),
        }
    }

    /// Run `f` on the zone state, reading the zone first if nothing has —
    /// `tzset_internal (0)`, which is what `localtime_r` does.
    fn with_engine<R>(&self, f: impl FnOnce(&Engine) -> R) -> R {
        {
            let guard = self.engine.read().unwrap_or_else(PoisonError::into_inner);
            if let Some(engine) = guard.as_ref() {
                return f(engine);
            }
        }
        let mut guard = self.engine.write().unwrap_or_else(PoisonError::into_inner);
        if guard.is_none() {
            *guard = Some(self.read());
        }
        match guard.as_ref() {
            Some(engine) => f(engine),
            // Just filled in, under the same lock.
            None => f(&Engine::Rules(tzset::Rules::utc())),
        }
    }

    /// Read this zone again, as glibc does whenever `TZ` is changed to name
    /// it. A zone that is not a `TZ` value is left alone.
    pub fn reread(&self) {
        if self.source == Source::Fixed {
            return;
        }
        let fresh = self.read();
        *self.engine.write().unwrap_or_else(PoisonError::into_inner) = Some(fresh);
    }

    /// glibc's `tzset ()` while `TZ` names this zone: read it if nothing has,
    /// and read it again if it came from `posixrules` — glibc leaves nothing
    /// to compare the next `TZ` with after one, so it re-reads every time.
    /// Otherwise nothing: the value has not changed.
    pub fn tzset(&self) {
        let stale = self
            .engine
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .is_none_or(Engine::stale);
        if stale {
            self.reread();
        }
    }

    /// Whether `self` and `other` are the same `TZ` value — gnulib's test in
    /// `set_tz` for whether a conversion in `self`, in a process whose `TZ`
    /// names `other`, needs `TZ` changed at all. Only as bytes, as `strcmp`
    /// compares them: `EST5EDT` and `:EST5EDT` differ.
    #[must_use]
    pub fn same_tz(&self, other: &Zone) -> bool {
        match (&self.source, &other.source) {
            (Source::Tz { tz: a, .. }, Source::Tz { tz: b, .. }) => a == b,
            _ => false,
        }
    }

    /// Whether glibc's `tzset ()`, finding `other`'s `TZ` where `self`'s was,
    /// would see no change and keep the zone it has — the two values compared
    /// as `tzset_internal` compares them, after an empty value has become
    /// `Universal` and one leading `:` has been dropped. So `:EST5EDT` after
    /// `EST5EDT` is no change here, where to [`Zone::same_tz`] (gnulib's
    /// comparison) it is one.
    ///
    /// Two unset values are the same: glibc re-reads `/etc/localtime` every
    /// time, but its stat cache finds the file it read before and changes
    /// nothing. A zone built from `posixrules` is re-read whatever this says;
    /// that is [`Zone::tzset`]'s business.
    #[must_use]
    pub fn tzset_would_keep(&self, other: &Zone) -> bool {
        fn value(tz: Option<&[u8]>) -> Option<&[u8]> {
            tz.map(|v| {
                let v: &[u8] = if v.is_empty() { b"Universal" } else { v };
                v.strip_prefix(b":").unwrap_or(v)
            })
        }
        match (&self.source, &other.source) {
            (Source::Tz { tz: a, .. }, Source::Tz { tz: b, .. }) => {
                value(a.as_deref()) == value(b.as_deref())
            }
            _ => false,
        }
    }

    /// gnulib's `set_tz` and `revert_tz` around `f`, a conversion in `self` by
    /// a process whose own zone is `process`: unless the two are the same
    /// value, `TZ` is switched to `self` (which reads it) and back (which
    /// reads `process` again).
    pub fn switched<R>(&self, process: &Zone, f: impl FnOnce() -> R) -> R {
        if self.same_tz(process) {
            return f();
        }
        self.reread();
        let out = f();
        process.reread();
        out
    }

    /// The zone this process is running in, from `TZ`, `TZDIR` and
    /// `/etc/localtime`.
    ///
    /// This is the call a utility wants. It reads the environment exactly once,
    /// and the zone file once, at the first conversion — which matters for
    /// more than speed: `ls -l` renders a timestamp per file, and re-resolving
    /// per file would open `/etc/localtime` once per line.
    #[must_use]
    pub fn from_env() -> Self {
        let tz = std::env::var_os("TZ");
        // Bytes, not a `String`: `TZ` is environment data, and environment data
        // is not required to be text. A non-UTF-8 `TZ` names no zone, but it
        // must reach the "names no zone" path rather than panic on the way.
        let raw = tz.as_ref().map(|v| os_bytes(v));
        Self::from_tz(raw.as_deref())
    }

    /// The zone a process would run in if its `TZ` were `tz` — resolved
    /// against this process's `TZDIR` and `/etc/localtime`, as [`from_env`]
    /// resolves the real one.
    ///
    /// This is gnulib's `tzalloc` followed by a `localtime_rz`: what a date
    /// string's own `TZ="…"` prefix means to `parse_datetime`, which must
    /// resolve that value exactly as it would have resolved the environment's.
    ///
    /// [`from_env`]: Zone::from_env
    #[must_use]
    pub fn from_tz(tz: Option<&[u8]>) -> Self {
        let dir = std::env::var_os("TZDIR")
            .and_then(|d| d.into_string().ok())
            .filter(|d| !d.is_empty())
            .unwrap_or_else(|| TZDIR_DEFAULT.to_string());
        Self::resolve(tz, &dir, Path::new(LOCALTIME_PATH))
    }

    /// Resolve an explicit `TZ` value against an explicit zoneinfo tree and
    /// default file — glibc's `tzset` with `TZDIR` and `TZDEFAULT` given.
    ///
    /// Split out from [`Zone::from_env`] so it can be tested without a process
    /// environment, and so a caller that keeps its own variables — a shell —
    /// can pass its own rather than the ones it happened to inherit.
    ///
    /// `None` means "the machine's zone": `localtime` is read. An empty value
    /// is the name `Universal`. See the module docs for the rest.
    #[must_use]
    pub fn resolve(tz: Option<&[u8]>, dir: &str, localtime: &Path) -> Self {
        Self {
            engine: RwLock::new(None),
            source: Source::Tz {
                tz: tz.map(<[u8]>::to_vec),
                dir: dir.to_string(),
                localtime: localtime.to_path_buf(),
            },
        }
    }

    /// The zoneinfo file `name` names under `dir` (or at `name`, if it is
    /// absolute), falling back to UTC. No POSIX rule is tried; for what `TZ`
    /// means, use [`Zone::resolve`].
    #[must_use]
    pub fn from_name(name: &[u8], dir: &str) -> Self {
        match zoneinfo_path(name, dir) {
            Some(path) => Self::from_file(&path),
            None => Self::utc(),
        }
    }

    /// Read a zoneinfo file, falling back to UTC.
    ///
    /// The file is parsed *here*, so one that is not TZif never becomes a zone
    /// and a listing cannot silently switch to UTC halfway down.
    #[must_use]
    pub fn from_file(path: &Path) -> Self {
        tzset::read_capped(path)
            .and_then(|bytes| tzset::Table::from_tzif(&bytes))
            .map_or_else(Self::utc, |table| Self::fixed(Engine::Table(table)))
    }

    /// The zone state in force at UTC instant `t`.
    ///
    /// Total, where glibc is not: for a POSIX-rule zone and an instant whose
    /// UTC year does not fit in `tm_year`, glibc's `localtime` fails, and this
    /// answers with the rule's standard half. [`Zone::localtime_r`] is the
    /// glibc-faithful form.
    #[must_use]
    pub fn lookup(&self, t: i64) -> TzInfo {
        self.with_engine(|engine| engine.state_or_standard(t).info())
    }

    /// `lookup`, `None` where glibc's `localtime_r` fails: for a POSIX-rule
    /// zone and an instant whose UTC year does not fit in `tm_year`.
    fn lookup_r(&self, t: i64) -> Option<TzInfo> {
        self.with_engine(|engine| engine.state(t).map(|state| state.info()))
    }

    /// glibc's `localtime` — not `localtime_r` — for the programs that call
    /// it (`find -printf`, `ps`, `tar`, `who`): the same answer, after a
    /// `tzset ()`, which re-reads a zone made from `posixrules` on every call.
    #[must_use]
    pub fn localtime(&self, t: i64, nanos: u32) -> Tm {
        self.tzset();
        self.local(t, nanos)
    }

    /// Break UTC instant `t` (plus `nanos`) down into this zone's calendar.
    #[must_use]
    pub fn local(&self, t: i64, nanos: u32) -> Tm {
        Tm::from_utc(t, nanos, self.lookup(t))
    }

    /// The inverse of [`Zone::local`]: a civil local time to a UTC instant.
    ///
    /// This is `mktime`. It returns the instant *and* the normalised [`Tm`],
    /// because a caller that hands in `2024-02-31` or `month: 13` needs to know
    /// what that resolved to — which is the same reason C's `mktime` writes
    /// back through its argument.
    ///
    /// # Why it iterates
    ///
    /// The offset depends on the instant and the instant depends on the offset.
    /// So it starts from the UTC guess and applies the offset in force there,
    /// repeating until it settles. Three rounds converge for every real zone,
    /// because an offset change is never larger than a day and never happens
    /// twice within one.
    ///
    /// A local time that a spring-forward skipped **does not exist**, and this
    /// resolves it to a nearby instant rather than failing — which is what
    /// glibc does with `tm_isdst = -1`. An ambiguous time in a fall-back hour
    /// picks one of the two, likewise as glibc does.
    ///
    /// # Why it lives here
    ///
    /// `userspace/coreutils/src/bin/cal.rs` carried this, under a comment
    /// saying "there is no inverse of `Zone::local` in the `localtime` crate,
    /// so this is it". Three other files carry private copies of the
    /// [`days_from_civil`] half alone. That is the same shape as the
    /// duplication this crate was created to end — see the module docs, where
    /// `unix_secs_to_datetime` is recorded as the fourth copy of the *forward*
    /// arithmetic. This is the first copy of the reverse.
    #[must_use]
    pub fn epoch(&self, civil: &Civil) -> (i64, Tm) {
        // Normalise the month first, so `days_from_civil` sees 1..=12 and any
        // day-of-month overflow (31 February) is left for it to carry.
        let year = civil
            .year
            .saturating_add((civil.month.saturating_sub(1)).div_euclid(12));
        let month = (civil.month.saturating_sub(1))
            .rem_euclid(12)
            .saturating_add(1);

        let days = days_from_civil(year, month, civil.day);
        let local_secs = days
            .saturating_mul(86_400)
            .saturating_add(civil.hour.saturating_mul(3_600))
            .saturating_add(civil.minute.saturating_mul(60))
            .saturating_add(civil.second);

        let mut t = local_secs;
        for _ in 0..3 {
            let off = i64::from(self.lookup(t).gmtoff);
            let next = local_secs.saturating_sub(off);
            if next == t {
                break;
            }
            t = next;
        }
        (t, self.local(t, 0))
    }
}

/// A civil (wall-clock) local time, with fields allowed **out of range**.
///
/// Out-of-range is the point: it is what lets a caller say "the 32nd of March"
/// or "month 13" and have [`Zone::epoch`] carry it, which is how `date -d` and
/// `cal` resolve `tomorrow` without special-casing month ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Civil {
    pub year: i64,
    /// 1..=12 nominally; outside that range it carries into the year.
    pub month: i64,
    pub day: i64,
    pub hour: i64,
    pub minute: i64,
    pub second: i64,
}

/// Days since 1970-01-01 for a proleptic-Gregorian civil date.
///
/// Howard Hinnant's `days_from_civil`, which is what the C library's `mktime`
/// computes by a longer road. It is exact for every year in `i64` and needs no
/// table.
///
/// Note that `m` and `d` are *not* range-checked: this is the arithmetic half,
/// and [`Zone::epoch`] is where normalisation happens.
#[must_use]
pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y.saturating_sub(1) } else { y };
    // Hinnant writes this as `(y >= 0 ? y : y-399) / 400`, which is how you
    // get a FLOOR division out of C's truncating one. `div_euclid` already
    // floors, so the `-399` must NOT be carried over as well — doing both
    // double-corrects and moves every date in a negative year by an era
    // (y = -100 lands in era -2 instead of -1).
    let era = y.div_euclid(400);
    let yoe = y.saturating_sub(era.saturating_mul(400));
    let mp = (m.saturating_add(9)).rem_euclid(12);
    let doy = (mp.saturating_mul(153).saturating_add(2))
        .div_euclid(5)
        .saturating_add(d)
        .saturating_sub(1);
    let doe = yoe
        .saturating_mul(365)
        .saturating_add(yoe.div_euclid(4))
        .saturating_sub(yoe.div_euclid(100))
        .saturating_add(doy);
    era.saturating_mul(146_097)
        .saturating_add(doe)
        .saturating_sub(719_468)
}

/// Build the path of the zoneinfo file `name` names, or `None` for a name that
/// must not be read as a file.
///
/// Absolute names are used as given; others are under `dir`, as glibc's
/// `TZDIR/NAME`. A name with a `..` component is refused (rule 6 in the module
/// docs), where glibc refuses one only in a setuid program: `TZ` is inherited
/// from whoever started the process, and the libc (`posix/src/tz.rs`) refuses
/// the same names — two readers of one `TZ` must not disagree about what it
/// means.
#[must_use]
pub fn zoneinfo_path(name: &[u8], dir: &str) -> Option<PathBuf> {
    if name.is_empty() || name.contains(&0) {
        return None;
    }
    // A `..` *inside* a component is not a traversal; only a whole component is.
    if name.split(|&b| b == b'/').any(|part| part == b"..") {
        return None;
    }
    let path = bytes_path(name)?;
    if name.starts_with(b"/") {
        return Some(path);
    }
    Some(Path::new(dir).join(path))
}

/// A path from bytes: exact on Unix, where a path is bytes.
#[cfg(unix)]
fn bytes_path(name: &[u8]) -> Option<PathBuf> {
    Some(PathBuf::from(
        <std::ffi::OsStr as std::os::unix::ffi::OsStrExt>::from_bytes(name),
    ))
}

/// A path from bytes on the Windows host build, where only UTF-8 round-trips.
#[cfg(not(unix))]
fn bytes_path(name: &[u8]) -> Option<PathBuf> {
    std::str::from_utf8(name).ok().map(PathBuf::from)
}

/// Broken-down local time: what `struct tm` carries, plus what it does not.
///
/// The two extras are the reason this is not `libc::tm`. `epoch` is the
/// unshifted instant, which `%s` needs and which cannot be recovered from the
/// fields once the offset has been folded in; `nanos` is the sub-second part,
/// which `ls --time-style=full-iso` prints and `struct tm` has nowhere to put.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tm {
    /// The full proleptic-Gregorian year — 2026, not 126.
    pub year: i64,
    /// 1–12.
    pub month: u32,
    /// 1–31.
    pub day: u32,
    /// 0–23.
    pub hour: u32,
    /// 0–59.
    pub minute: u32,
    /// 0–60; 60 never occurs here, since our clock does not carry leap seconds.
    pub second: u32,
    /// Days since Sunday, 0–6.
    pub wday: u32,
    /// Days since January 1st, 0–365.
    pub yday: u32,
    /// The unshifted UTC instant, for `%s`.
    pub epoch: i64,
    /// Nanoseconds within the second, for `%N`.
    pub nanos: u32,
    /// Seconds east of Greenwich, for `%z`.
    pub gmtoff: i32,
    /// Whether daylight saving was in force.
    pub is_dst: bool,
    /// The zone abbreviation, for `%Z`.
    pub abbr: TzName,
}

impl Tm {
    /// Break a UTC instant down into the calendar `info` describes.
    #[must_use]
    pub fn from_utc(t: i64, nanos: u32, info: TzInfo) -> Self {
        let local = t.saturating_add(i64::from(info.gmtoff));
        // Euclidean, not truncating: an instant before 1970 has a negative
        // `local`, and `-1 / 86400` is 0 while the day it falls in is -1.
        let days = local.div_euclid(86_400);
        let secs = local.rem_euclid(86_400);
        let (year, month, day) = tzrules::civil_from_days(days);
        Self {
            year,
            month,
            day,
            hour: (secs / 3600) as u32,
            minute: ((secs / 60) % 60) as u32,
            second: (secs % 60) as u32,
            // 1970-01-01 was a **Thursday**, so day 0 is weekday 4. `rem_euclid`
            // again, for the same reason, and because a date before 1970 is not
            // exotic — `ls -l` on a restored archive prints them.
            wday: (days.saturating_add(4).rem_euclid(7)) as u32,
            yday: days.saturating_sub(tzrules::days_from_civil(year, 1, 1)) as u32,
            epoch: t,
            nanos,
            gmtoff: info.gmtoff,
            is_dst: info.is_dst,
            abbr: info.name,
        }
    }
}

/// Abbreviated weekday names, C locale, indexed by [`Tm::wday`].
pub const WDAY_ABBR: [&[u8]; 7] = [b"Sun", b"Mon", b"Tue", b"Wed", b"Thu", b"Fri", b"Sat"];
/// Full weekday names, C locale.
pub const WDAY_FULL: [&[u8]; 7] = [
    b"Sunday",
    b"Monday",
    b"Tuesday",
    b"Wednesday",
    b"Thursday",
    b"Friday",
    b"Saturday",
];
/// Abbreviated month names, C locale, indexed by `month - 1`.
pub const MON_ABBR: [&[u8]; 12] = [
    b"Jan", b"Feb", b"Mar", b"Apr", b"May", b"Jun", b"Jul", b"Aug", b"Sep", b"Oct", b"Nov", b"Dec",
];
/// Full month names, C locale.
pub const MON_FULL: [&[u8]; 12] = [
    b"January",
    b"February",
    b"March",
    b"April",
    b"May",
    b"June",
    b"July",
    b"August",
    b"September",
    b"October",
    b"November",
    b"December",
];

/// An `OsStr`'s bytes, on both the host and the target.
#[cfg(unix)]
fn os_bytes(s: &std::ffi::OsStr) -> Vec<u8> {
    std::os::unix::ffi::OsStrExt::as_bytes(s).to_vec()
}

/// An `OsStr`'s bytes on Windows, where the only lossless route out of the
/// UTF-16-ish encoding is through `to_string_lossy`.
///
/// This is the host build only; a `TZ` that is not UTF-16-representable cannot
/// be set on Windows in the first place, so nothing is lost that could exist.
#[cfg(not(unix))]
fn os_bytes(s: &std::ffi::OsStr) -> Vec<u8> {
    s.to_string_lossy().into_owned().into_bytes()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn utc_tm(t: i64) -> Tm {
        Zone::utc().local(t, 0)
    }

    /// gnulib's `nstrftime`: every expectation in this module was measured
    /// through coreutils' `date`, which formats with it.
    fn fmt(f: &str, tm: &Tm) -> String {
        String::from_utf8(nstrftime(f.as_bytes(), tm)).unwrap()
    }

    /// The flags and widths GNU allows between `%` and the conversion.
    ///
    /// Every expectation here was run against coreutils 9.4 as
    /// `date -d @1000000000 +'[%X]'` with TZ=UTC, not recalled. Before this,
    /// `strftime` read exactly one byte after the `%`, so every one of these
    /// came out as the literal text of the directive.
    #[test]
    fn strftime_honours_the_pad_flags() {
        let tm = utc_tm(1_000_000_000);
        assert_eq!(fmt("%d", &tm), "09");
        assert_eq!(fmt("%-d", &tm), "9", "`-` means no padding");
        assert_eq!(fmt("%_d", &tm), " 9", "`_` means space padding");
        assert_eq!(fmt("%0d", &tm), "09", "`0` means zero padding");
        // `0` overrides a field whose default pad is a space, and `_` a field
        // whose default is a zero -- the flag wins either way round.
        assert_eq!(fmt("%e", &tm), " 9");
        assert_eq!(fmt("%0e", &tm), "09");
        assert_eq!(fmt("%S", &tm), "40");
        assert_eq!(fmt("%_5S", &tm), "   40");
    }

    /// A width REPLACES the field's default width; it is not a minimum.
    ///
    /// This is the rule an implementation gets wrong by building the obvious
    /// way: render `%d` as `09` and then pad to the requested width, and `%1d`
    /// comes back `09`. GNU says `9`.
    #[test]
    fn strftime_width_replaces_the_default_width() {
        let tm = utc_tm(1_000_000_000);
        assert_eq!(
            fmt("%1d", &tm),
            "9",
            "a width of 1 is narrower than %d's default"
        );
        assert_eq!(fmt("%2d", &tm), "09");
        assert_eq!(fmt("%5S", &tm), "00040");
        assert_eq!(fmt("%3H", &tm), "001");
        assert_eq!(fmt("%12N", &tm), "000000000000");
        // `-` discards the width along with the padding.
        assert_eq!(fmt("%-5S", &tm), "40");
    }

    /// The LAST flag wins, so the two orderings differ.
    #[test]
    fn strftime_takes_the_last_flag() {
        let tm = utc_tm(1_000_000_000);
        assert_eq!(fmt("%-0d", &tm), "09");
        assert_eq!(fmt("%0-d", &tm), "9");
    }

    /// A string field pads with SPACE whatever the pad flag says, and `^`/`#`
    /// change its case.
    #[test]
    fn strftime_cases_and_pads_a_string_field() {
        let tm = utc_tm(1_000_000_000);
        assert_eq!(fmt("%a", &tm), "Sun");
        assert_eq!(fmt("%5a", &tm), "  Sun", "strings pad with space, not zero");
        assert_eq!(fmt("%10B", &tm), " September");
        assert_eq!(
            fmt("%0a", &tm),
            "Sun",
            "a pad flag does nothing to a string"
        );
        assert_eq!(fmt("%^a", &tm), "SUN");
        assert_eq!(fmt("%^B", &tm), "SEPTEMBER");
    }

    /// `#` flips the case of the FIELD, with the direction taken from the text.
    ///
    /// A per-character swap -- the obvious reading of "opposite case" -- gives
    /// `sUN` for the first of these, and is wrong. `%P` is exempt because it is
    /// already the flipped spelling of `%p`.
    #[test]
    fn strftime_hash_flips_the_whole_field() {
        let tm = utc_tm(1_000_000_000);
        assert_eq!(fmt("%#a", &tm), "SUN", "not sUN");
        assert_eq!(fmt("%#B", &tm), "SEPTEMBER");
        assert_eq!(fmt("%#p", &tm), "am", "an upper-case field goes down");
        assert_eq!(fmt("%#Z", &tm), "utc");
        assert_eq!(fmt("%#P", &tm), "am", "%P is already the flipped spelling");
        assert_eq!(fmt("%^p", &tm), "AM");
    }

    /// An unrecognised specifier still comes back verbatim, flags and all --
    /// the behaviour the flag parsing must not have broken.
    #[test]
    fn strftime_still_passes_an_unknown_directive_through() {
        let tm = utc_tm(1_000_000_000);
        // `%Q`, not `%q`: the latter became REAL on 2026-09-14 (quarter of
        // the year) and these two tests are what noticed. `%Q` is
        // measured-unknown -- GNU prints it literally.
        assert_eq!(fmt("%Q", &tm), "%Q");
        assert_eq!(fmt("%%", &tm), "%");
        assert_eq!(fmt("100%", &tm), "100%");
        assert_eq!(
            fmt("%Y-%m-%d", &tm),
            "2001-09-09",
            "plain formats unchanged"
        );
    }

    #[test]
    fn the_epoch_was_a_thursday() {
        // The single most-often-wrong constant in a hand-written calendar: the
        // obvious `days % 7` makes day zero a Sunday.
        let tm = utc_tm(0);
        assert_eq!(tm.wday, 4);
        assert_eq!(fmt("%a %Y-%m-%d %H:%M:%S", &tm), "Thu 1970-01-01 00:00:00");
    }

    #[test]
    fn a_moment_before_the_epoch_is_the_day_before() {
        // Truncating division would put this on 1970-01-01 at hour 0.
        let tm = utc_tm(-1);
        assert_eq!((tm.year, tm.month, tm.day), (1969, 12, 31));
        assert_eq!((tm.hour, tm.minute, tm.second), (23, 59, 59));
        assert_eq!(tm.wday, 3);
    }

    #[test]
    fn yday_counts_from_zero_and_j_counts_from_one() {
        let tm = utc_tm(0);
        assert_eq!(tm.yday, 0);
        assert_eq!(fmt("%j", &tm), "001");
        // 2024 is a leap year, so December 31st is day 365 (0-based).
        let dec31 = tzrules::days_from_civil(2024, 12, 31) * 86_400;
        assert_eq!(utc_tm(dec31).yday, 365);
        assert_eq!(fmt("%j", &utc_tm(dec31)), "366");
    }

    #[test]
    fn the_padding_characters_differ_between_d_and_e() {
        let tm = utc_tm(tzrules::days_from_civil(2019, 3, 4) * 86_400);
        assert_eq!(fmt("%d", &tm), "04");
        assert_eq!(fmt("%e", &tm), " 4");
        // This pair is exactly what `ls -l`'s two locale formats are built
        // from: `Mar  4  2019` needs `%e`, and `%d` would print `Mar 04`.
        assert_eq!(fmt("%b %e  %Y", &tm), "Mar  4  2019");
    }

    #[test]
    fn twelve_hour_midnight_and_noon_are_both_twelve() {
        let midnight = utc_tm(0);
        assert_eq!(fmt("%I %p", &midnight), "12 AM");
        assert_eq!(fmt("%l %p", &midnight), "12 AM");
        let noon = utc_tm(12 * 3600);
        assert_eq!(fmt("%I %p", &noon), "12 PM");
        let one_pm = utc_tm(13 * 3600);
        assert_eq!(fmt("%I %p", &one_pm), "01 PM");
        assert_eq!(fmt("%l %p", &one_pm), " 1 PM");
    }

    #[test]
    fn days_from_civil_known_points() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(1969, 12, 31), -1);
        assert_eq!(days_from_civil(1970, 1, 2), 1);
        assert_eq!(days_from_civil(2000, 3, 1), 11017);
        // A leap day, and the day either side of it.
        assert_eq!(
            days_from_civil(2024, 2, 29) - days_from_civil(2024, 2, 28),
            1
        );
        assert_eq!(
            days_from_civil(2024, 3, 1) - days_from_civil(2024, 2, 29),
            1
        );
        // 1900 was NOT a leap year; 2000 was.
        assert_eq!(
            days_from_civil(1900, 3, 1) - days_from_civil(1900, 2, 28),
            1
        );
        assert_eq!(
            days_from_civil(2000, 3, 1) - days_from_civil(2000, 2, 28),
            2
        );
    }

    #[test]
    fn days_from_civil_round_trips_against_the_forward_conversion() {
        // The forward direction (`Tm::from_utc`) is already trusted -- it is
        // what every timestamp in the tree is rendered through -- so it serves
        // as the oracle for the inverse. Sweeping a range beats hand-picked
        // points here: the error I actually made while writing this was in the
        // ERA term, which only shows up in years far from 1970 and would have
        // passed every plausible spot check around the epoch.
        let utc = Zone::utc();
        let mut day = -800_000i64; // well before year 0
        while day < 800_000 {
            let t = day.saturating_mul(86_400);
            let tm = utc.local(t, 0);
            assert_eq!(
                days_from_civil(tm.year, i64::from(tm.month), i64::from(tm.day)),
                day,
                "round trip failed at day {day} ({}-{}-{})",
                tm.year,
                tm.month,
                tm.day
            );
            day += 997; // a prime stride, so the sweep does not align to weeks,
            // months or leap cycles
        }
    }

    #[test]
    fn zone_epoch_is_the_inverse_of_zone_local() {
        let utc = Zone::utc();
        for t in [0i64, 1, -1, 1_000_000_000, -1_000_000_000, 1_614_834_367] {
            let tm = utc.local(t, 0);
            let civil = Civil {
                year: tm.year,
                month: i64::from(tm.month),
                day: i64::from(tm.day),
                hour: i64::from(tm.hour),
                minute: i64::from(tm.minute),
                second: i64::from(tm.second),
            };
            assert_eq!(utc.epoch(&civil).0, t, "round trip failed for {t}");
        }
    }

    #[test]
    fn zone_epoch_normalises_out_of_range_fields() {
        let utc = Zone::utc();
        // The 32nd of March is the 1st of April, and month 13 is next January.
        // This is the behaviour `date -d tomorrow` and `cal` rely on, so it is
        // load-bearing rather than a curiosity.
        let (_, tm) = utc.epoch(&Civil {
            year: 2021,
            month: 3,
            day: 32,
            ..Civil::default()
        });
        assert_eq!((tm.year, tm.month, tm.day), (2021, 4, 1));

        let (_, tm) = utc.epoch(&Civil {
            year: 2021,
            month: 13,
            day: 1,
            ..Civil::default()
        });
        assert_eq!((tm.year, tm.month, tm.day), (2022, 1, 1));

        // February 30th in a leap year is March 1st.
        let (_, tm) = utc.epoch(&Civil {
            year: 2024,
            month: 2,
            day: 30,
            ..Civil::default()
        });
        assert_eq!((tm.year, tm.month, tm.day), (2024, 3, 1));
    }

    #[test]
    fn quarter_of_the_year() {
        // Measured across all twelve months against GNU date 9.4 rather than
        // derived, because the boundaries are the whole content of the
        // specifier and an off-by-one would still look plausible.
        let mut tm = utc_tm(0);
        for (month, want) in [
            (1, "1"),
            (2, "1"),
            (3, "1"),
            (4, "2"),
            (5, "2"),
            (6, "2"),
            (7, "3"),
            (8, "3"),
            (9, "3"),
            (10, "4"),
            (11, "4"),
            (12, "4"),
        ] {
            tm.month = month;
            assert_eq!(fmt("%q", &tm), want, "month {month}");
        }

        // Width and padding: the default is width 1, and widening pads with
        // ZEROES, not spaces. `%3q` -> `001`. Both measured; the zero default
        // is the part that is not obvious, since a one-digit field has no
        // visible default padding to infer it from.
        tm.month = 11;
        assert_eq!(fmt("%3q", &tm), "004");
        assert_eq!(fmt("%-q", &tm), "4");
        assert_eq!(fmt("%_q", &tm), "4");
    }

    #[test]
    fn an_unknown_specifier_survives_whole() {
        // glibc emits it literally. A renderer that dropped it would silently
        // delete two characters of a format the user typed.
        let tm = utc_tm(0);
        // `%Q`, not `%q`: the latter became REAL on 2026-09-14 (quarter of
        // the year) and these two tests are what noticed. `%Q` is
        // measured-unknown -- GNU prints it literally.
        assert_eq!(fmt("%Q", &tm), "%Q");
        assert_eq!(fmt("a%", &tm), "a%");
        assert_eq!(fmt("%%", &tm), "%");
    }

    #[test]
    fn n_is_a_newline_in_the_format_not_a_format_of_its_own() {
        // The distinction `ls --time-style=+%Y%n%H` turns on: `%n` reaches
        // `strftime` as two bytes and leaves as one newline.
        let tm = utc_tm(0);
        assert_eq!(fmt("%Y%n%H", &tm), "1970\n00");
    }

    #[test]
    fn the_three_week_numbers_are_three_different_questions() {
        // Every row measured from GNU `date -u -d DATE +'%U %W %V %G %g %u'`
        // under `LC_ALL=C TZ=UTC`. The dates are the ones where the three
        // definitions come apart: a year whose first days belong to the
        // previous ISO year, a year with 53 ISO weeks, and a Sunday — which is
        // week 0 for `%W` and week 1 for `%U` in the same breath.
        for &(y, m, d, u, w, v, g, g2, iso_dow) in &[
            (1970, 1, 1, "00", "00", "01", "1970", "70", "4"),
            (2000, 1, 1, "00", "00", "52", "1999", "99", "6"),
            (2015, 12, 28, "52", "52", "53", "2015", "15", "1"),
            (2016, 1, 3, "01", "00", "53", "2015", "15", "7"),
            (2019, 12, 30, "52", "52", "01", "2020", "20", "1"),
            (2020, 12, 31, "52", "52", "53", "2020", "20", "4"),
            (2021, 1, 1, "00", "00", "53", "2020", "20", "5"),
            (2021, 1, 3, "01", "00", "53", "2020", "20", "7"),
            (2024, 12, 29, "52", "52", "52", "2024", "24", "7"),
            (2026, 8, 23, "34", "33", "34", "2026", "26", "7"),
        ] {
            let tm = utc_tm(tzrules::days_from_civil(y, m, d) * 86_400);
            let got = fmt("%U %W %V %G %g %u", &tm);
            assert_eq!(
                got,
                format!("{u} {w} {v} {g} {g2} {iso_dow}"),
                "{y:04}-{m:02}-{d:02}"
            );
        }
    }

    #[test]
    fn a_long_iso_year_is_the_one_that_gains_a_week() {
        // 53 when the year starts on a Thursday (2015, 2020) or is a leap year
        // starting on a Wednesday (2020 again, 1992); 52 otherwise. The
        // constant this guards is the `p(y-1) == 3` half, which a naive
        // implementation drops because it never fires on a non-leap year.
        for &(y, n) in &[
            (1992, 53),
            (2015, 53),
            (2016, 52),
            (2020, 53),
            (2021, 52),
            (2024, 52),
            (2026, 53),
        ] {
            // December 28th is always in its year's last ISO week, so its
            // `%V` is the year's week count.
            let dec28 = utc_tm(tzrules::days_from_civil(y, 12, 28) * 86_400);
            assert_eq!(fmt("%V", &dec28), n.to_string(), "{y}");
        }
    }

    #[test]
    fn nanoseconds_are_nine_digits() {
        let tm = Zone::utc().local(0, 42);
        assert_eq!(fmt("%N", &tm), "000000042");
    }

    #[test]
    fn the_offset_sign_does_not_leak_into_the_minutes() {
        // `-0400`, never `-04-00`. The bug this guards is dividing a negative
        // offset before taking its magnitude.
        let info = TzInfo {
            gmtoff: -4 * 3600 - 30 * 60,
            is_dst: false,
            name: TzName::UTC,
        };
        let tm = Tm::from_utc(0, 0, info);
        assert_eq!(fmt("%z", &tm), "-0430");
        let east = Tm::from_utc(
            0,
            0,
            TzInfo {
                gmtoff: 5 * 3600 + 45 * 60,
                is_dst: false,
                name: TzName::UTC,
            },
        );
        assert_eq!(fmt("%z", &east), "+0545");
    }

    #[test]
    fn s_is_the_unshifted_instant() {
        // Not the local clock: the offset is folded into the fields and must
        // not be folded into `%s` as well.
        let info = TzInfo {
            gmtoff: -5 * 3600,
            is_dst: false,
            name: TzName::UTC,
        };
        let tm = Tm::from_utc(1_551_693_967, 0, info);
        assert_eq!(fmt("%s", &tm), "1551693967");
        assert_eq!(fmt("%Y-%m-%d %H:%M", &tm), "2019-03-04 05:06");
    }

    #[test]
    fn an_absent_tz_is_the_machines_zone_and_an_empty_one_is_universal() {
        // The rule that is wrong in the obvious implementation, and wrong
        // silently: an unset `TZ` means `/etc/localtime`, not Greenwich. Here
        // the file does not exist, so the fallback is UTC named `UTC` — what
        // is being asserted is that it was *looked for*, since a rule would
        // have been named after the value.
        let missing = Path::new("/nonexistent-localtime-for-this-test");
        let dir = "/nonexistent-zoneinfo-dir";
        let name = |tz: Option<&[u8]>| Zone::resolve(tz, dir, missing).lookup(0).name;
        assert_eq!(name(None).as_bytes(), b"UTC");
        // Empty is glibc's `Universal`: no such file here, so the rule.
        assert_eq!(name(Some(b"")).as_bytes(), b"Universal");
    }

    #[test]
    fn with_no_file_of_that_name_a_posix_rule_is_read() {
        // `EST5EDT` is both a rule and a file. glibc tries the file first
        // (see the `tzset` tests); with no zoneinfo tree, the rule answers.
        let zone = Zone::resolve(
            Some(b"EST5EDT"),
            "/nonexistent-zoneinfo-dir",
            Path::new("/nonexistent-localtime-for-this-test"),
        );
        // Midsummer: EDT, four hours west.
        let summer = zone.lookup(tzrules::days_from_civil(2020, 7, 1) * 86_400);
        assert_eq!(summer.gmtoff, -4 * 3600);
        assert!(summer.is_dst);
    }

    #[test]
    fn a_leading_colon_is_only_dropped() {
        // glibc reads `:EST5EDT` exactly as `EST5EDT`: a file if there is one,
        // else the rule.
        let zone = Zone::resolve(
            Some(b":EST5EDT"),
            "/nonexistent-zoneinfo-dir",
            Path::new("/nonexistent-localtime-for-this-test"),
        );
        assert_eq!(zone.lookup(0).gmtoff, -5 * 3600);
    }

    #[test]
    fn a_dotdot_component_is_refused_but_a_dotdot_inside_a_name_is_not() {
        assert!(zoneinfo_path(b"../../etc/shadow", TZDIR_DEFAULT).is_none());
        assert!(zoneinfo_path(b"..", TZDIR_DEFAULT).is_none());
        assert!(zoneinfo_path(b"America/..", TZDIR_DEFAULT).is_none());
        // Not a traversal: `..` has to be a whole component.
        assert!(zoneinfo_path(b"a..b", TZDIR_DEFAULT).is_some());
        assert!(zoneinfo_path(b"", TZDIR_DEFAULT).is_none());
        assert!(zoneinfo_path(b"a\0b", TZDIR_DEFAULT).is_none());
    }

    #[test]
    fn an_absolute_zone_name_is_used_as_given() {
        // `TZ=/etc/localtime` is a thing people set, and joining it onto TZDIR
        // would look for `/usr/share/zoneinfo/etc/localtime`.
        assert_eq!(
            zoneinfo_path(b"/etc/localtime", TZDIR_DEFAULT).unwrap(),
            PathBuf::from("/etc/localtime")
        );
    }

    #[test]
    fn a_file_that_is_not_tzif_is_not_a_zone() {
        // Rejected at load, so a listing cannot switch to UTC halfway down.
        let zone = Zone::from_file(Path::new("Cargo.toml"));
        assert!(zone.with_engine(|e| matches!(e, Engine::Rules(_))));
        assert_eq!(zone.lookup(0).gmtoff, 0);
    }

    #[test]
    fn a_zone_is_read_at_first_use_not_when_made() {
        let zone = Zone::resolve(Some(b"EST5"), "/nonexistent-zoneinfo-dir", Path::new("/x"));
        assert!(zone.engine.read().unwrap().is_none());
        assert_eq!(zone.lookup(0).gmtoff, -5 * 3600);
        assert!(zone.engine.read().unwrap().is_some());
    }

    #[test]
    fn tzset_would_keep_compares_as_glibc_does() {
        let at = |tz: Option<&[u8]>| Zone::resolve(tz, "/d", Path::new("/x"));
        // glibc drops one `:` and reads empty as `Universal` before comparing.
        assert!(at(Some(b"EST5")).tzset_would_keep(&at(Some(b":EST5"))));
        assert!(!at(Some(b"EST5")).tzset_would_keep(&at(Some(b"::EST5"))));
        assert!(at(Some(b"Universal")).tzset_would_keep(&at(Some(b""))));
        assert!(at(None).tzset_would_keep(&at(None)));
        assert!(!at(None).tzset_would_keep(&at(Some(b"UTC"))));
        assert!(!at(Some(b"EST5")).tzset_would_keep(&at(Some(b"EST6"))));
        assert!(!Zone::utc().tzset_would_keep(&Zone::utc()));
        // gnulib's `set_tz` compares the values as written.
        assert!(!at(Some(b"EST5")).same_tz(&at(Some(b":EST5"))));
    }

    #[test]
    fn same_tz_is_a_comparison_of_the_values_as_written() {
        let at = |tz: Option<&[u8]>| Zone::resolve(tz, "/d", Path::new("/x"));
        assert!(at(Some(b"EST5")).same_tz(&at(Some(b"EST5"))));
        assert!(!at(Some(b"EST5")).same_tz(&at(Some(b":EST5"))));
        assert!(at(None).same_tz(&at(None)));
        assert!(!at(None).same_tz(&at(Some(b""))));
        assert!(!Zone::utc().same_tz(&Zone::utc()));
    }
}
