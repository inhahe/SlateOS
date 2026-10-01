//! Which zone a `TZ` value names: the one rule every reader of `TZ` on this
//! system applies, stated once.
//!
//! Three programs read `TZ` and had each written the order out for itself --
//! the libc (`posix/src/tz.rs`), lane B's `userspace/localtime` (which `date`,
//! `ls`, `osh` and the rest read it through) and, as of 2026-09-25, the
//! desktop (`gui/datetimesettings`), which needs the machine's zone to draw
//! its clock. The order is not obvious and a copy that drifts is a program
//! that disagrees with `date` about the time, so the *decision* lives here, in
//! the crate all three already link.
//!
//! Only the decision. Reading the file it names is left to the caller, because
//! the three cannot share that part: the libc reads into a static page before
//! its allocator works, and a program with `std` reads a `Vec`. [`TzPlan`]
//! says what to read and what stands in when it cannot be read;
//! [`TzFile`](crate::TzFile) parses what was read. The order itself, and why,
//! is on [`tz_plan`], where the crate's documentation shows it.

use crate::Tz;

/// Where the zoneinfo tree is when `TZDIR` does not say.
pub const ZONEINFO_DIR: &[u8] = b"/usr/share/zoneinfo";

/// The machine's own zone -- a TZif file, or a link into the zoneinfo tree --
/// followed when `TZ` is unset.
pub const LOCALTIME: &[u8] = b"/etc/localtime";

/// The name an empty `TZ` stands for: glibc's, and a file in the zoneinfo
/// tree (a link to `Etc/UTC`) wherever one is installed.
const UNIVERSAL: &[u8] = b"Universal";

/// A zoneinfo file that a `TZ` value names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZoneFile<'a> {
    /// The machine's own zone, [`LOCALTIME`].
    System,
    /// A name under the zoneinfo directory -- `TZDIR` if the caller honours
    /// it and it is set, else [`ZONEINFO_DIR`].
    Named(&'a [u8]),
    /// An absolute path.
    Path(&'a [u8]),
}

/// What a `TZ` value names, decided in glibc's order ([`tz_plan`] has it): a
/// zoneinfo file to try first, and the rule that stands in when there is no
/// file to try, or it cannot be read or does not parse.
///
/// The caller carries it out, since reading is the caller's:
///
/// ```
/// use tzrules::{Tz, ZoneFile, tz_plan};
///
/// // A caller's reader: here, a machine with no zoneinfo files at all.
/// let read = |_file: ZoneFile<'_>| -> Option<Tz> { None };
///
/// let plan = tz_plan(Some(b"EST5EDT"));
/// assert_eq!(plan.file(), Some(ZoneFile::Named(b"EST5EDT")));
/// let zone = plan.file().and_then(read).unwrap_or_else(|| plan.fallback());
/// assert_eq!(Some(zone), Tz::parse(b"EST5EDT"));
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TzPlan<'a> {
    file: Option<ZoneFile<'a>>,
    rule: Option<&'a [u8]>,
}

impl<'a> TzPlan<'a> {
    /// `TZ` unset, or naming [`LOCALTIME`] itself: that file, else UTC.
    const SYSTEM: Self = Self {
        file: Some(ZoneFile::System),
        rule: None,
    };

    /// UTC, with nothing to read: a bare `:`.
    const UTC: Self = Self {
        file: None,
        rule: None,
    };

    /// The zoneinfo file to try first; `None` when there is none to try.
    #[must_use]
    pub fn file(&self) -> Option<ZoneFile<'a>> {
        self.file
    }

    /// The text to read as a POSIX rule when there is no file to try, or it
    /// cannot be read or does not parse; `None` means UTC.
    ///
    /// It is the `TZ` value itself, less a leading `:` (`Universal` for an
    /// empty one), and it is offered whether or not this crate's engine can
    /// parse it -- for an engine like glibc's, which keeps what the start of
    /// a rule says. [`fallback`](Self::fallback) is this crate's reading.
    #[must_use]
    pub fn rule(&self) -> Option<&'a [u8]> {
        self.rule
    }

    /// What stands in when there is no file, by this crate's rule engine:
    /// [`rule`](Self::rule) if it parses whole, else UTC.
    #[must_use]
    pub fn fallback(&self) -> Tz {
        self.rule.and_then(Tz::parse_rule).unwrap_or_else(Tz::utc)
    }
}

/// What the `TZ` value `value` names, in glibc's order. `None` is `TZ` unset.
///
/// # The order
///
/// glibc 2.39's (`tzset_internal` in `time/tzset.c`), which is what every
/// ported program expects, and what the same program on Linux shows:
///
/// 1. **Unset** -- the machine's own zone, the file [`LOCALTIME`]; UTC if it
///    cannot be read.
/// 2. **Empty** -- the name `Universal`, which is a file in the zoneinfo tree,
///    and from there as any other name: a program that clears `TZ` is asking
///    for UTC, not for the machine's zone, and scripts rely on the difference.
/// 3. **A leading `:`** is dropped, and means nothing more. POSIX reserves it
///    for implementation-defined forms, and glibc's are the same as its plain
///    ones: "We ignore the colon and always use the same algorithm: try a data
///    file, and if none exists parse the 1003.1 syntax." So `:EST5EDT` with no
///    such file is still a rule, and a bare `:` is UTC. One `:` only: in
///    `::EST5EDT` the second is part of the name, which no rule starts with.
/// 4. **A zoneinfo file of that name**, if one can be read -- an absolute path,
///    or a name under the zoneinfo directory (`TZDIR`, else [`ZONEINFO_DIR`]).
///    The file is tried *before* the rule, even for a name that is both:
///    `EST5EDT` is a file carrying the United States' rules as each year had
///    them, and a rule carrying only today's. At 1990-03-20 12:00 UTC the file
///    says 07:00 EST and the rule 08:00 EDT, and before 2026-10-01 this crate
///    took the rule ([`tz_source`]) -- an hour off from Linux
///    (`requests/b-cd-tz-source-tries-the-rule-before-the-file-and-glibc-does-the-opposite.md`).
/// 5. **Otherwise the POSIX rule** (`CET-1CEST,M3.5.0,M10.5.0/3`) -- UTC if the
///    value is not one, or if the file it named was [`LOCALTIME`] itself.
///
/// A file name with a `..` component, or a NUL, is never opened
/// ([`TzPlan::file`] is `None`), and the value goes on to the rule as a name
/// with no file does. `TZ` is inherited from whoever started the program, and
/// without the check `TZ=../../../etc/shadow` would open an arbitrary file and
/// report whether it parses -- a small oracle, but a free one. glibc refuses
/// these only in a set-user-ID program; refusing them always costs nothing a
/// zone needs, since no zoneinfo name has either.
///
/// # What a caller does not get from here
///
/// - **The refusal of every path in a set-user-ID program**, which the libc
///   adds: whether the program is one is a fact about the process, which this
///   crate cannot see.
/// - **`posixrules`.** glibc gives a rule with a daylight-saving name and no
///   dates (`AAA3BBB`) the transitions of the file `posixrules` in the
///   zoneinfo directory, moved to the rule's offsets, when there is one. That
///   is the rule engine's business, not the order's: this crate's engine gives
///   such a rule the United States' rules since 2007, as glibc does with no
///   `posixrules` -- which is SlateOS today, with no zoneinfo tree at all --
///   and lane B's `localtime`, which ports glibc's engine, reads the file.
/// - **A partly parsed rule.** [`TzPlan::rule`] hands over the text, and this
///   crate's engine takes a rule whole or not at all ([`TzPlan::fallback`]), so
///   `TZ=Foo/Bar` with no such file is UTC here and UTC *named* `Foo` in glibc.
#[must_use]
pub fn tz_plan(value: Option<&[u8]>) -> TzPlan<'_> {
    let name = match value {
        None => return TzPlan::SYSTEM,
        // "User specified the empty string; use UTC explicitly" -- by a name,
        // which glibc then looks up like any other.
        Some([]) => UNIVERSAL,
        Some(value) => value.strip_prefix(b":").unwrap_or(value),
    };
    if name.is_empty() {
        return TzPlan::UTC;
    }
    if name == LOCALTIME {
        return TzPlan::SYSTEM;
    }
    TzPlan {
        file: zone_file(name),
        rule: Some(name),
    }
}

/// The zoneinfo file `name` names, or `None` for one that must not be opened.
fn zone_file(name: &[u8]) -> Option<ZoneFile<'_>> {
    // `..` as a whole component only: `Europe/Bu..dapest` is a name, and
    // refusing it would be a surprise.
    let escapes = name.split(|&b| b == b'/').any(|part| part == b"..");
    if name.contains(&0) || escapes {
        None
    } else if name.starts_with(b"/") {
        Some(ZoneFile::Path(name))
    } else {
        Some(ZoneFile::Named(name))
    }
}

/// What a `TZ` value names, in the order this crate used before 2026-10-01,
/// which is **not glibc's**: a POSIX rule is tried before a file of the same
/// name, a leading `:` means "a file, never a rule", and empty is UTC without
/// looking for `Universal`. See [`tz_plan`], which replaces it.
///
/// Kept only while `posix/src/tz.rs` still calls it -- lane D's, asked to
/// move in `requests/c-d-decide-tz-through-tz-plan.md` -- and to be deleted
/// when nothing does. New code calls [`tz_plan`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TzSource<'a> {
    /// UTC, because `TZ` asked for it by being set and empty.
    Utc,
    /// A POSIX rule, already parsed.
    Rule(Tz),
    /// A zoneinfo file, by its name under the zoneinfo directory -- `TZDIR`
    /// if the caller honours it and it is set, else [`ZONEINFO_DIR`].
    Named(&'a [u8]),
    /// A zoneinfo file, by absolute path.
    Path(&'a [u8]),
    /// The machine's own zone: `TZ` is unset, so read [`LOCALTIME`].
    System,
    /// A file name that must not be opened -- a `..` component, a NUL, or
    /// nothing after a `:` -- which resolves to UTC.
    Refused,
}

/// What the `TZ` value `value` names, in the order before 2026-10-01 --
/// superseded by [`tz_plan`]; see [`TzSource`]. `None` is `TZ` unset.
#[must_use]
pub fn tz_source(value: Option<&[u8]>) -> TzSource<'_> {
    let Some(value) = value else {
        return TzSource::System;
    };
    if value.is_empty() {
        return TzSource::Utc;
    }
    if let Some(name) = value.strip_prefix(b":") {
        return file_source(name);
    }
    if let Some(rule) = Tz::parse(value) {
        return TzSource::Rule(rule);
    }
    file_source(value)
}

/// A zoneinfo file name, or its refusal, for [`tz_source`].
fn file_source(name: &[u8]) -> TzSource<'_> {
    match zone_file(name) {
        Some(ZoneFile::Path(path)) => TzSource::Path(path),
        Some(ZoneFile::Named(name)) if !name.is_empty() => TzSource::Named(name),
        _ => TzSource::Refused,
    }
}

#[cfg(test)]
#[path = "source_tests.rs"]
mod tests;
