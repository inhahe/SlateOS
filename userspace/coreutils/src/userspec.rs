//! gnulib's `userspec.c`: reading an `OWNER[:GROUP]` operand.
//!
//! `chown alice:staff f` and `id alice` are asking the same question of the
//! same database, and upstream answers both with one function —
//! `parse_user_spec`, in gnulib rather than in either utility. It is here for
//! the same reason: the grammar has enough corners that two copies would
//! certainly differ on one of them, and a difference would mean `chown` and
//! `id` disagreeing about which account a word names.
//!
//! The corners, all measured against GNU 9.4:
//!
//! * The number is `strtoul`, not `str::parse` — see [`numeric_id`], which
//!   accepts `" +007"` and rejects `"0x10"`, `"1000 "` and `4294967295`.
//! * A leading `+` skips the *name* lookup, which is the only way to mean uid
//!   1000 on a machine that also has an account literally called `1000`.
//! * A trailing separator means "and that account's login group", so it is an
//!   error after a number — a uid has no login group.
//! * `.` is a separator only as a fallback, tried after the colon-less reading
//!   has failed in full, so an account genuinely named `a.b` is found rather
//!   than split.
//! * The name fields of [`Spec`] are the text *as typed*, not the resolved
//!   account's name, and they are present only when a lookup succeeded. That
//!   asymmetry is what `chown -v`'s wording keys off.

use pwdb::Db;

/// gnulib's `xstrtoul (s, nullptr, 10, &n, "")`, which is what decides whether
/// a spec that named no account is nonetheless a number.
///
/// It is `strtoul` with the whole string required, so it is emphatically **not**
/// `str::parse::<u32>()`, and every difference is observable. All measured:
///
/// | Input | GNU | Why |
/// |---|---|---|
/// | `" 1000"` | 1000 | `strtoul` skips leading whitespace |
/// | `"+1000"` | 1000 | and accepts a `+` sign |
/// | `"-0"` | `invalid user` | but not a `-` one, for an unsigned conversion |
/// | `"1000 "` | `invalid user` | the empty suffix list means "consume it all" |
/// | `"0x10"` | `invalid user` | base 10 is explicit, so no `0x` |
/// | `"007"` | 7 | and no octal either |
/// | `"4294967295"` | `invalid user` | see below |
///
/// The last is not an overflow: `(uid_t)-1` is POSIX's "leave this field
/// alone" sentinel, so accepting it would turn an explicit request into a
/// silent no-op. 4294967294 is fine.
#[must_use]
pub fn numeric_id(text: &[u8]) -> Option<u32> {
    let mut rest = text;
    while let Some((first, tail)) = rest.split_first() {
        if first.is_ascii_whitespace() {
            rest = tail;
        } else {
            break;
        }
    }
    if let Some(tail) = rest.strip_prefix(b"+") {
        rest = tail;
    }
    if rest.is_empty() || !rest.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let mut value: u64 = 0;
    for digit in rest {
        value = value
            .checked_mul(10)?
            .checked_add(u64::from(*digit).saturating_sub(u64::from(b'0')))?;
        if value > u64::from(u32::MAX) {
            return None;
        }
    }
    let value = u32::try_from(value).ok()?;
    if value == u32::MAX { None } else { Some(value) }
}

/// What an `OWNER[:GROUP]` spec resolved to.
///
/// The two name fields are **not** the resolved account's name — they are the
/// text as typed, present only when a lookup succeeded on it. That asymmetry is
/// gnulib's and it is what `chown -v`'s wording keys off; see the module docs.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct Spec {
    /// The uid to set, or `None` for "leave it alone".
    pub uid: Option<u32>,
    /// The gid to set, or `None` for "leave it alone".
    pub gid: Option<u32>,
    /// The user field as typed, when it named an account that exists.
    pub user_name: Option<Vec<u8>>,
    /// The group field as typed, when it named a group that exists — or, for a
    /// trailing colon, the *login group's* name, which is the one case where
    /// this is a resolution rather than an echo.
    pub group_name: Option<Vec<u8>>,
}

/// gnulib's `parse_with_separator`: read a spec whose separator is already
/// located (or known absent).
///
/// # Errors
///
/// `"invalid spec"` for a trailing separator after something that is not an
/// account (there is no login group to look up), `"invalid user"` or
/// `"invalid group"` for a field that is neither a name nor a number.
pub fn parse_with_separator(
    spec: &[u8],
    sep: Option<usize>,
    db: &Db,
) -> Result<Spec, &'static str> {
    let (user, group): (Option<&[u8]>, Option<&[u8]>) = match sep {
        None => (Some(spec).filter(|s| !s.is_empty()), None),
        Some(at) => {
            let head = spec.get(..at).unwrap_or_default();
            let tail = spec.get(at.saturating_add(1)..).unwrap_or_default();
            (
                Some(head).filter(|s| !s.is_empty()),
                Some(tail).filter(|s| !s.is_empty()),
            )
        }
    };

    let mut out = Spec::default();
    if let Some(user) = user {
        // A leading `+` skips the lookup outright. That is how you name uid
        // 1000 on a system that also has an account literally called `1000`,
        // and it is why `+alice` is an error rather than a synonym for `alice`.
        let found = if user.first() == Some(&b'+') {
            None
        } else {
            db.user_by_name(user)
        };
        match found {
            None => {
                if sep.is_some() && group.is_none() {
                    // `1000:` — the trailing colon asks for "and the owner's
                    // login group", which is a property of an account. A number
                    // does not have one, so this is not a uid-only change; it
                    // is a spec that cannot be honoured.
                    return Err("invalid spec");
                }
                out.uid = Some(numeric_id(user).ok_or("invalid user")?);
                // `user_name` stays `None`: nothing was resolved, so there is
                // no name for a diagnostic to print instead of the number.
            }
            Some(account) => {
                out.uid = Some(account.uid);
                out.user_name = Some(user.to_vec());
                if group.is_none() && sep.is_some() {
                    out.gid = Some(account.gid);
                    out.group_name = Some(match db.group_by_gid(account.gid) {
                        Some(found) => found.name.clone(),
                        // A gid with no `/etc/group` line still has a name for
                        // reporting purposes: its number.
                        None => account.gid.to_string().into_bytes(),
                    });
                }
            }
        }
    }
    if let Some(group) = group {
        let found = if group.first() == Some(&b'+') {
            None
        } else {
            db.group_by_name(group)
        };
        match found {
            None => out.gid = Some(numeric_id(group).ok_or("invalid group")?),
            Some(entry) => {
                out.gid = Some(entry.gid);
                out.group_name = Some(group.to_vec());
            }
        }
    }
    Ok(out)
}

/// gnulib's `parse_user_spec_warn`. Returns the resolution and whether a `.`
/// had to be read as the separator.
///
/// The `.` fallback is a POSIX-compatible *extension*, and the order matters:
/// the colon-less reading is tried first and in full, so an account genuinely
/// called `a.b` is found rather than split. Only when that fails does the first
/// `.` become a separator, and then the caller warns.
///
/// # Errors
///
/// As [`parse_with_separator`]. When the dot fallback also fails, the error
/// reported is the *first* attempt's, which is why `chown a.b.c` says
/// `invalid user` about the whole spec rather than about `a`.
pub fn parse_user_spec(spec: &[u8], db: &Db) -> Result<(Spec, bool), &'static str> {
    let colon = spec.iter().position(|c| *c == b':');
    let first = parse_with_separator(spec, colon, db);
    let Err(error) = first else {
        return first.map(|s| (s, false));
    };
    if colon.is_some() {
        return Err(error);
    }
    let Some(dot) = spec.iter().position(|c| *c == b'.') else {
        return Err(error);
    };
    match parse_with_separator(spec, Some(dot), db) {
        Ok(out) => Ok((out, true)),
        Err(_) => Err(error),
    }
}

/// `chown-core.c`'s `uid_to_name`: the account's name, or the number when the
/// database does not know it.
///
/// Used wherever a name must be produced unconditionally — the *old* ownership
/// in a `chown -v` line, or `id -un` for an account with no `/etc/passwd`
/// entry — which is why it always yields something rather than an `Option`.
#[must_use]
pub fn uid_to_name(db: &Db, uid: u32) -> Vec<u8> {
    match db.user_by_uid(uid) {
        Some(found) => found.name.clone(),
        None => uid.to_string().into_bytes(),
    }
}

/// `gid_to_name`, as [`uid_to_name`].
#[must_use]
pub fn gid_to_name(db: &Db, gid: u32) -> Vec<u8> {
    match db.group_by_gid(gid) {
        Some(found) => found.name.clone(),
        None => gid.to_string().into_bytes(),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    /// A small, self-contained account database. `Db::from_bytes` exists for
    /// exactly this: the development host has no `/etc/passwd` at all, so a
    /// test that read the real one would be a test that only runs on the target.
    fn db() -> Db {
        Db::from_bytes(
            b"root:x:0:0:root:/root:/bin/sh\n\
              alice:x:1000:1000:Alice:/home/alice:/bin/sh\n\
              bob:x:1001:2000:Bob:/home/bob:/bin/sh\n\
              1000:x:4000:4000:confusing:/:/bin/sh\n\
              a.b:x:5000:5000:dotted:/:/bin/sh\n",
            b"root:x:0:\n\
              alice:x:1000:\n\
              staff:x:2000:alice\n\
              nogroupfile:x:9999:\n",
        )
    }

    fn spec(text: &str) -> Spec {
        parse_user_spec(text.as_bytes(), &db()).unwrap().0
    }

    fn spec_err(text: &str) -> &'static str {
        parse_user_spec(text.as_bytes(), &db()).unwrap_err()
    }

    // ---------------- numeric_id ----------------

    #[test]
    fn numeric_id_is_strtoul_not_parse() {
        assert_eq!(numeric_id(b"1000"), Some(1000));
        assert_eq!(numeric_id(b"007"), Some(7));
        assert_eq!(numeric_id(b" 1000"), Some(1000));
        assert_eq!(numeric_id(b"\t\n 1000"), Some(1000));
        assert_eq!(numeric_id(b"+1000"), Some(1000));
        assert_eq!(numeric_id(b"1000 "), None);
        assert_eq!(numeric_id(b"-0"), None);
        assert_eq!(numeric_id(b"-1"), None);
        assert_eq!(numeric_id(b"++1"), None);
        assert_eq!(numeric_id(b"0x10"), None);
        assert_eq!(numeric_id(b"1e3"), None);
        assert_eq!(numeric_id(b""), None);
        assert_eq!(numeric_id(b"+"), None);
    }

    /// `(uid_t)-1` is the kernel's "leave this alone", so asking for it would be
    /// indistinguishable from asking for nothing. GNU refuses it; one less is
    /// fine. Both measured.
    #[test]
    fn numeric_id_refuses_the_unchanged_sentinel() {
        assert_eq!(numeric_id(b"4294967294"), Some(4_294_967_294));
        assert_eq!(numeric_id(b"4294967295"), None);
        assert_eq!(numeric_id(b"4294967296"), None);
        assert_eq!(numeric_id(b"99999999999999999999"), None);
    }

    // ---------------- parse_user_spec ----------------

    #[test]
    fn spec_name_resolves_through_the_account_database() {
        // This is the capability chown's old parser declared missing while `ls`
        // was already using it.
        let a = spec("alice");
        assert_eq!(a.uid, Some(1000));
        assert_eq!(a.gid, None);
        assert_eq!(a.user_name.as_deref(), Some(&b"alice"[..]));
        assert_eq!(a.group_name, None);
    }

    #[test]
    fn spec_name_and_group() {
        let a = spec("alice:staff");
        assert_eq!((a.uid, a.gid), (Some(1000), Some(2000)));
        assert_eq!(a.user_name.as_deref(), Some(&b"alice"[..]));
        assert_eq!(a.group_name.as_deref(), Some(&b"staff"[..]));
    }

    /// A trailing colon means "and the owner's login group".
    #[test]
    fn spec_trailing_colon_takes_the_login_group() {
        let a = spec("bob:");
        assert_eq!((a.uid, a.gid), (Some(1001), Some(2000)));
        // The group *name*, resolved from the gid rather than echoed.
        assert_eq!(a.group_name.as_deref(), Some(&b"staff"[..]));
    }

    /// ...and a gid with no `/etc/group` line still reports as its number.
    #[test]
    fn spec_login_group_without_a_group_line_falls_back_to_digits() {
        let db = Db::from_bytes(b"carol:x:1:12345::/:/bin/sh\n", b"");
        let (a, _) = parse_user_spec(b"carol:", &db).unwrap();
        assert_eq!(a.gid, Some(12345));
        assert_eq!(a.group_name.as_deref(), Some(&b"12345"[..]));
    }

    /// A number has no login group, so a trailing colon after one is not a
    /// uid-only change — it is a spec that cannot be honoured. Measured:
    /// `chown 1000: f` is `invalid spec`, and this is the only place that
    /// message comes from.
    #[test]
    fn spec_trailing_colon_after_a_number_is_invalid_spec() {
        assert_eq!(spec_err("1234:"), "invalid spec");
        assert_eq!(spec_err("+1000:"), "invalid spec");
    }

    #[test]
    fn spec_group_only() {
        let a = spec(":staff");
        assert_eq!((a.uid, a.gid), (None, Some(2000)));
        assert_eq!(a.user_name, None);
        assert_eq!(a.group_name.as_deref(), Some(&b"staff"[..]));
    }

    #[test]
    fn spec_empty_and_bare_colon_change_nothing() {
        for text in ["", ":"] {
            let a = spec(text);
            assert_eq!((a.uid, a.gid), (None, None), "{text}");
            assert_eq!((a.user_name, a.group_name), (None, None), "{text}");
        }
    }

    /// A number resolves but contributes no *name*, which is what makes
    /// `chown :0` and `chown :root` print different lines.
    #[test]
    fn spec_numbers_resolve_without_names() {
        let a = spec("1234:5678");
        assert_eq!((a.uid, a.gid), (Some(1234), Some(5678)));
        assert_eq!((a.user_name, a.group_name), (None, None));
        let b = spec(":0");
        assert_eq!(b.gid, Some(0));
        assert_eq!(b.group_name, None);
        let c = spec(":root");
        assert_eq!(c.gid, Some(0));
        assert_eq!(c.group_name.as_deref(), Some(&b"root"[..]));
    }

    /// `+` skips the lookup, which is the only way to mean uid 1000 on a system
    /// that also has an account named `1000` — and this database has one.
    #[test]
    fn spec_plus_skips_the_name_lookup() {
        assert_eq!(spec("1000").uid, Some(4000));
        assert_eq!(spec("1000").user_name.as_deref(), Some(&b"1000"[..]));
        assert_eq!(spec("+1000").uid, Some(1000));
        assert_eq!(spec("+1000").user_name, None);
        assert_eq!(spec_err("+alice"), "invalid user");
    }

    #[test]
    fn spec_unknown_names_are_rejected() {
        assert_eq!(spec_err("nosuchuser"), "invalid user");
        assert_eq!(spec_err("alice:nosuchgroup"), "invalid group");
        assert_eq!(spec_err("nosuchuser:staff"), "invalid user");
    }

    /// The `.` separator is a compatible extension, tried only after the
    /// colon-less reading fails — so an account genuinely called `a.b` wins.
    #[test]
    fn spec_dot_separator_is_the_fallback_not_the_rule() {
        let (dotted, warned) = parse_user_spec(b"alice.staff", &db()).unwrap();
        assert!(warned);
        assert_eq!((dotted.uid, dotted.gid), (Some(1000), Some(2000)));

        let (literal, warned) = parse_user_spec(b"a.b", &db()).unwrap();
        assert!(!warned, "an account called a.b must not be split");
        assert_eq!(literal.uid, Some(5000));
    }

    /// With a colon present the dot is just a character, and a spec that fails
    /// both readings reports the *first* attempt's error.
    #[test]
    fn spec_dot_fallback_is_skipped_when_a_colon_exists() {
        assert_eq!(spec_err("nosuch.user:staff"), "invalid user");
        assert_eq!(spec_err("a.b.c"), "invalid user");
    }

    // ---------------- uid_to_name / gid_to_name ----------------

    #[test]
    fn names_fall_back_to_the_number() {
        assert_eq!(uid_to_name(&db(), 1000), b"alice".to_vec());
        assert_eq!(uid_to_name(&db(), 31337), b"31337".to_vec());
        assert_eq!(gid_to_name(&db(), 2000), b"staff".to_vec());
        assert_eq!(gid_to_name(&db(), 31337), b"31337".to_vec());
    }
}
