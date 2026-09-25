//! Which zone a `TZ` value names: the one rule every reader of `TZ` on this
//! system applies, stated once.
//!
//! Three programs read `TZ` and had each written the order out for itself --
//! the libc (`posix/src/tz.rs`), the shell (`userspace/oils`) and, as of
//! 2026-09-25, the desktop (`gui/datetimesettings`), which needs the machine's
//! zone to draw its clock. The order is not obvious and a copy that drifts is a
//! program that disagrees with `date` about the time, so the *decision* lives
//! here, in the crate all three already link.
//!
//! Only the decision. Reading the file it names is left to the caller, because
//! the three cannot share that part: the libc reads into a static page before
//! its allocator works, and a program with `std` reads a `Vec`. [`TzSource`]
//! says what to read; [`TzFile`](crate::TzFile) parses what was read.
//!
//! # The order
//!
//! Glibc's, which is what every ported program expects:
//!
//! 1. **Unset** -- the machine's own zone, [`LOCALTIME`].
//! 2. **Empty** -- UTC, explicitly. Not the same as unset: a program that
//!    clears `TZ` is asking for UTC, not for the machine's zone, and scripts
//!    rely on the difference.
//! 3. **A leading `:`** -- the rest is a zoneinfo file name, whatever it looks
//!    like. POSIX reserves the prefix for implementation-defined forms, and
//!    every libc spells it this way.
//! 4. **A POSIX rule** (`EST5EDT,M3.2.0,M11.1.0`) -- used as it is. Tried
//!    before the file because `EST5EDT` is both a rule and a file in the
//!    zoneinfo tree, and the rule is the cheaper and more predictable of the
//!    two.
//! 5. **Anything else** -- a zoneinfo file name: an absolute path, or a name
//!    under the zoneinfo directory (`TZDIR`, else [`ZONEINFO_DIR`]).
//!
//! A file name with a `..` component, or a NUL, is [refused](TzSource::Refused)
//! and resolves to UTC: `TZ` is inherited from whoever started the program, and
//! without the check `TZ=../../../etc/shadow` would open an arbitrary file and
//! report whether it parses -- a small oracle, but a free one.
//!
//! What a caller does *not* get from here is the libc's extra refusal of any
//! path at all in a set-user-ID program: whether the program is one is a fact
//! about the process, which this crate cannot see.

use crate::Tz;

/// Where the zoneinfo tree is when `TZDIR` does not say.
pub const ZONEINFO_DIR: &[u8] = b"/usr/share/zoneinfo";

/// The machine's own zone -- a TZif file, or a link into the zoneinfo tree --
/// followed when `TZ` is unset.
pub const LOCALTIME: &[u8] = b"/etc/localtime";

/// What a `TZ` value names; see the [module documentation](self) for the
/// order in which it is decided.
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

/// What the `TZ` value `value` names. `None` is `TZ` unset.
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

/// A zoneinfo file name, or its refusal.
fn file_source(name: &[u8]) -> TzSource<'_> {
    // `..` as a whole component only: `Europe/Bu..dapest` is a name, and
    // refusing it would be a surprise.
    let escapes = name.split(|&b| b == b'/').any(|part| part == b"..");
    if name.is_empty() || name.contains(&0) || escapes {
        TzSource::Refused
    } else if name.starts_with(b"/") {
        TzSource::Path(name)
    } else {
        TzSource::Named(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unset_is_the_machines_zone_and_empty_is_utc() {
        assert_eq!(tz_source(None), TzSource::System);
        assert_eq!(tz_source(Some(b"")), TzSource::Utc);
    }

    #[test]
    fn a_rule_is_used_as_it_is() {
        let rule = Tz::parse(b"CET-1CEST,M3.5.0,M10.5.0/3").unwrap();
        assert_eq!(
            tz_source(Some(b"CET-1CEST,M3.5.0,M10.5.0/3")),
            TzSource::Rule(rule)
        );
    }

    /// `EST5EDT` is a rule and a file; the rule wins unless a `:` asks for the
    /// file.
    #[test]
    fn a_rule_is_tried_before_a_file_unless_a_colon_says_file() {
        assert!(matches!(tz_source(Some(b"EST5EDT")), TzSource::Rule(_)));
        assert_eq!(tz_source(Some(b":EST5EDT")), TzSource::Named(b"EST5EDT"));
    }

    #[test]
    fn a_name_is_a_file_under_the_zoneinfo_directory_and_a_path_is_a_path() {
        assert_eq!(
            tz_source(Some(b"America/New_York")),
            TzSource::Named(b"America/New_York")
        );
        assert_eq!(
            tz_source(Some(b":/etc/zones/home")),
            TzSource::Path(b"/etc/zones/home")
        );
        assert_eq!(
            tz_source(Some(b"/etc/zones/home")),
            TzSource::Path(b"/etc/zones/home")
        );
    }

    /// A name that would leave the zoneinfo tree, or that a NUL would cut
    /// short, is not opened.
    #[test]
    fn a_name_that_escapes_or_is_cut_short_is_refused() {
        for bad in [
            &b"../../../etc/shadow"[..],
            b":../x",
            b"/usr/share/zoneinfo/../../etc/shadow",
            b"Europe/..",
            b"a\0b",
            b":",
        ] {
            assert_eq!(tz_source(Some(bad)), TzSource::Refused, "{bad:?}");
        }
        // Two dots inside a component are a name, not a step up.
        assert_eq!(
            tz_source(Some(b"Europe/Bu..dapest")),
            TzSource::Named(b"Europe/Bu..dapest")
        );
    }
}
