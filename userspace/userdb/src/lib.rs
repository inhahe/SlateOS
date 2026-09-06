//! The SlateOS user database, `/etc/users.yaml`.
//!
//! This file is SlateOS's own account store, separate from the POSIX
//! `/etc/passwd` and `/etc/shadow`. Seven programs read it — the graphical
//! login manager, `useradm`, `su`, `sudo`, `polkit`, `chown` and `chroot` —
//! and two of them write it. Before this crate existed each of the seven had
//! its own parser, and the two writers had drifted into **mutually
//! incompatible schemas**: one wrote the password salt as `salt:` and hashed
//! the salt's hexadecimal *text*, the other wrote `password_salt:` and hashed
//! the salt's decoded *bytes*. A password set with `useradm passwd` therefore
//! could not be used to log in, and each writer deleted the other's fields —
//! including the group memberships `sudo` and `polkit` authorise from —
//! every time it rewrote the file.
//!
//! That is the same failure as the `/etc/shadow` one recorded in
//! `design-decisions.md` §329, and it has the same cause: a format with more
//! than one implementation is a format with no definition. So this crate is
//! the definition.
//!
//! # What it guarantees
//!
//! **A field this crate does not know about survives a rewrite.** A record is
//! kept as its own lines, and writing a value splices it into the line that
//! holds it; comments, blank lines, key order and unrecognised keys all come
//! back out as they went in. That property is the whole reason the file has a
//! parser rather than a serializer — `design.txt` requires configuration to be
//! "processed with a library that preserves comments and formatting", and a
//! writer that emits only the fields *it* has a struct field for is precisely
//! how the two schemas silently deleted each other's data.
//!
//! (The general form of this is the `yamldoc` crate, which this would use if
//! it could. `yamldoc` addresses values by a path of mapping keys, and
//! `users.yaml` is a *sequence* of mappings — `users[2].uid` is not something
//! its path syntax can name. Extending it is the better long-run answer; see
//! `known-issues.md`.)
//!
//! **There is one password hash, and it is `crypt(3)`.** Both old
//! constructions were a single unsalted-iteration `sha256(salt ‖ password)`:
//! no work factor, so an attacker holding the file tries candidate passwords
//! as fast as the hardware can hash. Passwords now go through
//! `posix::crypt` — SHA-512-crypt, 5000 rounds — which is the same code
//! `/etc/shadow` uses, so the system contains one password-hash
//! implementation rather than three.
//!
//! # Reading both dialects, writing one
//!
//! Files written by either old tool are read correctly: where the two chose
//! different names for the same field (`home` / `home_dir`, `admin` /
//! `is_admin`, `avatar` / `avatar_path`), both are accepted on the way in and
//! the longer, more explicit one is written on the way out. The two never
//! collide, because no tool wrote both.
//!
//! Password *entries* in the old format are a different matter: they are
//! refused rather than migrated, because the two writers disagreed about what
//! the bytes meant, so there is no single thing they can be said to be. See
//! [`Auth::Unusable`].
//!
//! # The flat files are generated from this one
//!
//! `design-decisions.md` §353 settles which of the machine's two account
//! stores is the real one: this file is, and `/etc/passwd` and `/etc/shadow`
//! are *generated* from it, for the benefit of ported software that reads the
//! flat files directly. So [`UserDb::save`] writes three files, not one — the
//! database and both of its renderings, staged to temporaries and renamed
//! together — and there is no way to write the database without regenerating
//! them, because a caller that could forget is a caller that eventually does.
//!
//! What that buys is the end of the defect §353 was decided to end: a user
//! created with `useradd` who could log in over SSH and did not exist to the
//! graphical login screen, or the reverse. What it costs is that a hand-edit
//! of `/etc/passwd` survives only until the next account change. The generated
//! header comment naming this file is the only warning anyone gets, which is
//! why it says what happens rather than merely that the file is generated.
//!
//! A record that cannot be written as a colon-separated line fails the whole
//! save ([`GenerateError`]) rather than being skipped. Skipping would put a
//! user in one file and not the other, which is the thing being fixed.
//!
//! Because the generated file is the only `/etc/shadow` there is, everything
//! that file can say has to be sayable here — so the database carries the
//! password-aging policy too ([`Aging`]), and not as a convenience. `passwd`
//! and `chage` are largely *about* those six numbers; had they been left out,
//! moving those two tools onto this crate would have silently erased every
//! expiry policy on the machine at the first save.

use std::fmt::Write as _;

/// Where the database lives.
pub const DEFAULT_PATH: &str = "/etc/users.yaml";

/// The generated POSIX account file, named relative to the database's own
/// directory. See [`UserDb::to_passwd_text`].
pub const PASSWD_NAME: &str = "passwd";

/// The generated POSIX password file, named relative to the database's own
/// directory. See [`UserDb::to_shadow_text`].
pub const SHADOW_NAME: &str = "shadow";

/// The method new passwords are hashed with.
const PASSWORD_METHOD: posix::crypt::Method = posix::crypt::Method::Sha512;

/// Canonical field names, and the aliases accepted when reading.
pub mod field {
    /// Numeric user id. Also the key the record's `- ` line carries.
    pub const UID: &str = "uid";
    /// Numeric primary group id. Optional: a record without one is generated
    /// into `/etc/passwd` with its uid as its gid, the user-private-group
    /// convention. See [`crate::Record::gid`].
    pub const GID: &str = "gid";
    /// Login name.
    pub const USERNAME: &str = "username";
    /// Name shown on the login screen.
    pub const DISPLAY_NAME: &str = "display_name";
    /// The full `crypt(3)` entry, salt included.
    pub const PASSWORD_HASH: &str = "password_hash";
    /// Login shell.
    pub const SHELL: &str = "shell";
    /// Home directory. `useradm` called this `home`.
    pub const HOME: &str = "home_dir";
    /// Home directory, as `useradm` spelled it.
    pub const HOME_ALIAS: &str = "home";
    /// Group memberships, a flow sequence.
    pub const GROUPS: &str = "groups";
    /// Administrator flag. `useradm` called this `admin`.
    pub const IS_ADMIN: &str = "is_admin";
    /// Administrator flag, as `useradm` spelled it.
    pub const IS_ADMIN_ALIAS: &str = "admin";
    /// Whether the account is barred from logging in.
    pub const LOCKED: &str = "locked";
    /// Avatar image path. `useradm` called this `avatar`.
    pub const AVATAR: &str = "avatar_path";
    /// Avatar image path, as `useradm` spelled it.
    pub const AVATAR_ALIAS: &str = "avatar";
    /// Whether this account logs in without being asked.
    pub const AUTO_LOGIN: &str = "auto_login";
    /// Unix timestamp of the last successful login.
    pub const LAST_LOGIN: &str = "last_login_timestamp";
    /// Count of successful logins.
    pub const LOGIN_COUNT: &str = "login_count";

    // ---- Password aging: `/etc/shadow` fields 3 through 8 ----
    //
    // Six numbers that together are one policy, so they are read and written
    // together through [`crate::Aging`]. Each is *optional*, and absent is
    // not zero: the shadow file spells "no policy for this" as an empty
    // field, and a zero in `MIN_DAYS` means "may be changed again at once"
    // while an empty one means the question was never asked. A record that
    // has never carried aging generates the same eight empty fields it
    // always did.

    /// Days since the Unix epoch on which the password was last changed.
    /// Maintained by [`crate::Record::set_password`]; see there for why a
    /// password whose age is unknown is one no policy can act on.
    pub const PASSWORD_CHANGED: &str = "password_changed";
    /// Days that must pass before the password may be changed again.
    pub const PASSWORD_MIN_DAYS: &str = "password_min_days";
    /// Days a password stays valid before it must be changed.
    pub const PASSWORD_MAX_DAYS: &str = "password_max_days";
    /// Days before expiry that the user starts being warned.
    pub const PASSWORD_WARN_DAYS: &str = "password_warn_days";
    /// Days after expiry before the account stops accepting the password.
    pub const PASSWORD_INACTIVE_DAYS: &str = "password_inactive_days";
    /// Days since the Unix epoch on which the account itself expires.
    pub const ACCOUNT_EXPIRES: &str = "account_expires";

    /// The salt fields the two old writers used. Read to recognise a legacy
    /// entry; never written, because a `crypt(3)` entry carries its own salt
    /// and a salt stored beside it is a second copy that can disagree.
    pub const LEGACY_SALT: [&str; 2] = ["password_salt", "salt"];
}

/// A record's password-aging policy: `/etc/shadow` fields 3 through 8.
///
/// Every member is optional, and `None` means the field is *empty* in the
/// generated file — the shadow format's spelling of "no policy", which is not
/// the same as zero. In particular `None` is how "never expires" is spelled
/// here; `-1` is **not**, even though `chage(1)` and `passwd(1)` accept `-1`
/// on their command lines to mean it. Those two translate at their own edge,
/// because a `-1` that reached the file would be read back by glibc as a date
/// one day before the epoch rather than as "never".
///
/// [`Record::aging`] reads the whole policy and [`Record::set_aging`] writes
/// it, so a caller changing one member cannot drop the other five.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Aging {
    /// Days since the Unix epoch on which the password was last changed.
    pub changed: Option<i64>,
    /// Days that must pass before the password may be changed again.
    pub min_days: Option<i64>,
    /// Days a password stays valid before it must be changed.
    pub max_days: Option<i64>,
    /// Days before expiry that the user starts being warned.
    pub warn_days: Option<i64>,
    /// Days after expiry before the account stops accepting the password.
    pub inactive_days: Option<i64>,
    /// Days since the Unix epoch on which the account itself expires.
    pub expires: Option<i64>,
}

/// Today, as days since the Unix epoch, or `None` if the clock is before it.
///
/// The clock going backwards is not fatal here: the one caller stamps a
/// password change, and a stamp it cannot compute is left absent — which
/// reads as "the age of this password is unknown", the truthful answer.
#[must_use]
pub fn today() -> Option<i64> {
    let since = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?;
    i64::try_from(since.as_secs() / 86_400).ok()
}

/// One line of a record, kept as close to as-written as it can be.
#[derive(Debug, Clone)]
enum Line {
    /// `key: value`, indexed and editable.
    Field {
        /// Leading whitespace, reproduced verbatim.
        indent: String,
        /// The key, unquoted.
        key: String,
        /// The value as it appeared, quoting included.
        raw_value: String,
    },
    /// A comment, a blank line, or anything this does not model. Emitted
    /// back byte for byte so that a file using a construct we do not
    /// understand is preserved rather than corrupted.
    Other(String),
}

/// One user's entry.
#[derive(Debug, Clone, Default)]
pub struct Record {
    /// Indentation of the `- ` that opens the record.
    dash_indent: String,
    /// Indentation of the record's continuation lines.
    field_indent: String,
    lines: Vec<Line>,
}

impl Record {
    /// A new, empty record with the conventional two/four-space indentation.
    #[must_use]
    pub fn new() -> Self {
        Self {
            dash_indent: "  ".to_string(),
            field_indent: "    ".to_string(),
            lines: Vec::new(),
        }
    }

    /// The raw text of `key`, with quoting removed, or `None` if absent.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<String> {
        self.lines.iter().find_map(|line| match line {
            Line::Field {
                key: k, raw_value, ..
            } if k == key => Some(unquote(raw_value)),
            _ => None,
        })
    }

    /// The first of `keys` that is present. Used for the fields the two old
    /// writers named differently.
    #[must_use]
    pub fn get_any(&self, keys: &[&str]) -> Option<String> {
        keys.iter().find_map(|k| self.get(k))
    }

    /// Set `key`, replacing the existing line if there is one and appending
    /// otherwise. Every other line is left exactly as it was.
    pub fn set(&mut self, key: &str, value: &str) {
        let raw_value = quote(value);
        for line in &mut self.lines {
            if let Line::Field {
                key: k,
                raw_value: v,
                ..
            } = line
                && k == key
            {
                *v = raw_value;
                return;
            }
        }
        let indent = self.field_indent.clone();
        self.lines.push(Line::Field {
            indent,
            key: key.to_string(),
            raw_value,
        });
    }

    /// Set `key` to a value that must not be quoted — a number, a boolean, or
    /// `null`. YAML would read `"true"` as the string, not the boolean, and
    /// the readers that have not migrated yet compare the raw text.
    fn set_bare(&mut self, key: &str, value: &str) {
        for line in &mut self.lines {
            if let Line::Field {
                key: k,
                raw_value: v,
                ..
            } = line
                && k == key
            {
                value.clone_into(v);
                return;
            }
        }
        let indent = self.field_indent.clone();
        self.lines.push(Line::Field {
            indent,
            key: key.to_string(),
            raw_value: value.to_string(),
        });
    }

    /// Set *every* spelling of a field that this record already uses, or the
    /// canonical one — `keys[0]` — if it uses none.
    ///
    /// Updating only the first spelling found would leave the other one saying
    /// something else, and a reader that consulted the other would act on the
    /// stale value. That is precisely the failure this crate exists to remove,
    /// so it is not reintroduced one level down.
    pub fn set_any(&mut self, keys: &[&str], value: &str) {
        self.set_any_raw(keys, &quote(value));
    }

    /// [`Record::set_any`] for a value that must not be quoted.
    fn set_bare_any(&mut self, keys: &[&str], value: &str) {
        self.set_any_raw(keys, value);
    }

    fn set_any_raw(&mut self, keys: &[&str], raw_value: &str) {
        let mut found = false;
        for line in &mut self.lines {
            if let Line::Field {
                key: k,
                raw_value: v,
                ..
            } = line
                && keys.contains(&k.as_str())
            {
                raw_value.clone_into(v);
                found = true;
            }
        }
        if !found && let Some(first) = keys.first() {
            let indent = self.field_indent.clone();
            self.lines.push(Line::Field {
                indent,
                key: (*first).to_string(),
                raw_value: raw_value.to_string(),
            });
        }
    }

    /// Remove `key`, reporting whether it was there.
    pub fn remove(&mut self, key: &str) -> bool {
        let before = self.lines.len();
        self.lines
            .retain(|line| !matches!(line, Line::Field { key: k, .. } if k == key));
        self.lines.len() != before
    }

    /// Whether `key` is present.
    #[must_use]
    pub fn contains(&self, key: &str) -> bool {
        self.lines
            .iter()
            .any(|line| matches!(line, Line::Field { key: k, .. } if k == key))
    }

    // ---- Typed views of the known fields ----

    /// The numeric user id.
    #[must_use]
    pub fn uid(&self) -> Option<u32> {
        self.get(field::UID)?.trim().parse().ok()
    }

    /// Set the numeric user id.
    pub fn set_uid(&mut self, uid: u32) {
        let mut buf = String::new();
        // `write!` to a `String` is infallible; the result is discarded
        // rather than unwrapped so that no formatting path can panic.
        let _ = write!(buf, "{uid}");
        self.set_bare(field::UID, &buf);
    }

    /// The numeric primary group id, or `None` if the record does not name
    /// one.
    ///
    /// `None` rather than a baked-in default, for the same reason
    /// [`Record::shell`] gives: the sensible fallback belongs where it can be
    /// seen. The one place that needs it today is the generated
    /// `/etc/passwd`, which uses the uid — the user-private-group convention,
    /// and the only choice that cannot collide with another account's private
    /// group, because uids do not collide either.
    #[must_use]
    pub fn gid(&self) -> Option<u32> {
        self.get(field::GID)?.trim().parse().ok()
    }

    /// Set the numeric primary group id.
    pub fn set_gid(&mut self, gid: u32) {
        let mut buf = String::new();
        // `write!` to a `String` is infallible; the result is discarded
        // rather than unwrapped so that no formatting path can panic.
        let _ = write!(buf, "{gid}");
        self.set_bare(field::GID, &buf);
    }

    /// The login name.
    #[must_use]
    pub fn username(&self) -> Option<String> {
        self.get(field::USERNAME)
    }

    /// The name shown to a human, falling back to the login name.
    ///
    /// The fallback is here rather than at each call site because a record
    /// with no `display_name` is normal — `useradd` does not require one — and
    /// a caller that forgot the fallback would print an empty column.
    #[must_use]
    pub fn display_name(&self) -> Option<String> {
        match self.get(field::DISPLAY_NAME) {
            Some(name) if !name.is_empty() => Some(name),
            _ => self.username(),
        }
    }

    /// The login shell.
    ///
    /// Returns `None` rather than a default: the sensible default differs by
    /// caller — `su` wants the target's shell or `/bin/sh`, a display manager
    /// wants the session's — and a default baked in here would be invisible at
    /// the point where it mattered.
    #[must_use]
    pub fn shell(&self) -> Option<String> {
        match self.get(field::SHELL) {
            Some(shell) if !shell.is_empty() => Some(shell),
            _ => None,
        }
    }

    /// The home directory, under either spelling.
    #[must_use]
    pub fn home(&self) -> Option<String> {
        self.get_any(&[field::HOME, field::HOME_ALIAS])
    }

    /// Set the home directory, under whichever spelling the record already
    /// uses.
    pub fn set_home(&mut self, home: &str) {
        self.set_any(&[field::HOME, field::HOME_ALIAS], home);
    }

    /// The avatar path, under either spelling. A literal `null` reads as
    /// absent, which is what the login manager writes for "no avatar".
    #[must_use]
    pub fn avatar(&self) -> Option<String> {
        let value = self.get_any(&[field::AVATAR, field::AVATAR_ALIAS])?;
        if value.trim() == "null" || value.is_empty() {
            None
        } else {
            Some(value)
        }
    }

    /// Whether the account is an administrator, under either spelling.
    #[must_use]
    pub fn is_admin(&self) -> bool {
        self.get_any(&[field::IS_ADMIN, field::IS_ADMIN_ALIAS])
            .is_some_and(|v| v.trim() == "true")
    }

    /// Set the administrator flag, under whichever spelling the record already
    /// uses.
    pub fn set_admin(&mut self, admin: bool) {
        self.set_bare_any(
            &[field::IS_ADMIN, field::IS_ADMIN_ALIAS],
            if admin { "true" } else { "false" },
        );
    }

    /// Set the avatar path, under whichever spelling the record already uses.
    /// An empty path is stored as `null`, the login manager's spelling of "no
    /// avatar", rather than as an empty string that reads back as a path to
    /// the root of the filesystem.
    pub fn set_avatar(&mut self, path: &str) {
        if path.is_empty() {
            self.set_bare_any(&[field::AVATAR, field::AVATAR_ALIAS], "null");
        } else {
            self.set_any(&[field::AVATAR, field::AVATAR_ALIAS], path);
        }
    }

    /// The password-aging policy, as one value.
    ///
    /// The six numbers are read together rather than one at a time because
    /// they *are* one policy — `chage -l` prints all six, `passwd -S` prints
    /// four of them, and a caller changing one has to write back the other
    /// five unchanged or silently drop them. See [`Aging`].
    #[must_use]
    pub fn aging(&self) -> Aging {
        Aging {
            changed: self.day(field::PASSWORD_CHANGED),
            min_days: self.day(field::PASSWORD_MIN_DAYS),
            max_days: self.day(field::PASSWORD_MAX_DAYS),
            warn_days: self.day(field::PASSWORD_WARN_DAYS),
            inactive_days: self.day(field::PASSWORD_INACTIVE_DAYS),
            expires: self.day(field::ACCOUNT_EXPIRES),
        }
    }

    /// Replace the password-aging policy.
    ///
    /// A `None` **removes** the field rather than writing a zero, because the
    /// shadow file distinguishes the two and so does every tool that reads
    /// it: an empty `max` field is "this password does not expire", a `0` is
    /// "it expired the day it was set".
    pub fn set_aging(&mut self, aging: &Aging) {
        self.set_day(field::PASSWORD_CHANGED, aging.changed);
        self.set_day(field::PASSWORD_MIN_DAYS, aging.min_days);
        self.set_day(field::PASSWORD_MAX_DAYS, aging.max_days);
        self.set_day(field::PASSWORD_WARN_DAYS, aging.warn_days);
        self.set_day(field::PASSWORD_INACTIVE_DAYS, aging.inactive_days);
        self.set_day(field::ACCOUNT_EXPIRES, aging.expires);
    }

    /// One aging field, or `None` if it is absent, empty or not a number.
    ///
    /// A value that will not parse reads as absent rather than as an error,
    /// because the alternative is that one typo in a hand-edited file makes
    /// every later save of the whole database fail. Absent is also the state
    /// the field was in before anyone set a policy, so the degradation is to
    /// the status quo and not to a surprise.
    fn day(&self, key: &str) -> Option<i64> {
        self.get(key)?.trim().parse().ok()
    }

    /// Write one aging field, removing it when `value` is `None`.
    fn set_day(&mut self, key: &str, value: Option<i64>) {
        match value {
            Some(days) => {
                let mut buf = String::new();
                // `write!` to a `String` is infallible; the result is
                // discarded rather than unwrapped so no path can panic.
                let _ = write!(buf, "{days}");
                self.set_bare(key, &buf);
            }
            None => {
                self.remove(key);
            }
        }
    }

    /// Whether the account is barred from logging in.
    ///
    /// Two things can say so: an explicit `locked: true`, or a password entry
    /// prefixed with `!`, which is how `/etc/shadow` spells the same thing.
    /// Both are honoured, because a lock that only one tool can see is not a
    /// lock.
    #[must_use]
    pub fn is_locked(&self) -> bool {
        if self.get(field::LOCKED).is_some_and(|v| v.trim() == "true") {
            return true;
        }
        self.get(field::PASSWORD_HASH)
            .is_some_and(|h| h.starts_with('!') || h.starts_with('*'))
    }

    /// Set the locked flag.
    pub fn set_locked(&mut self, locked: bool) {
        self.set_bare(field::LOCKED, if locked { "true" } else { "false" });
    }

    /// Group memberships, parsed from the flow sequence `[a, b]`.
    #[must_use]
    pub fn groups(&self) -> Vec<String> {
        let Some(raw) = self.get(field::GROUPS) else {
            return Vec::new();
        };
        raw.trim()
            .trim_start_matches('[')
            .trim_end_matches(']')
            .split(',')
            .map(|g| unquote(g.trim()))
            .filter(|g| !g.is_empty())
            .collect()
    }

    /// Replace the group memberships.
    pub fn set_groups(&mut self, groups: &[String]) {
        let mut buf = String::from("[");
        for (i, g) in groups.iter().enumerate() {
            if i > 0 {
                buf.push_str(", ");
            }
            buf.push_str(&quote(g));
        }
        buf.push(']');
        self.set_bare(field::GROUPS, &buf);
    }

    /// Whether this account logs in without being asked.
    #[must_use]
    pub fn auto_login(&self) -> bool {
        self.get(field::AUTO_LOGIN)
            .is_some_and(|v| v.trim() == "true")
    }

    /// Set the auto-login flag.
    pub fn set_auto_login(&mut self, auto: bool) {
        self.set_bare(field::AUTO_LOGIN, if auto { "true" } else { "false" });
    }

    /// Unix timestamp of the last successful login, or 0 if never.
    #[must_use]
    pub fn last_login(&self) -> u64 {
        self.get(field::LAST_LOGIN)
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(0)
    }

    /// Total successful logins.
    #[must_use]
    pub fn login_count(&self) -> u32 {
        self.get(field::LOGIN_COUNT)
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(0)
    }

    /// Record a successful login at `timestamp`.
    ///
    /// The count saturates rather than wrapping: a counter that returns to
    /// zero after four billion logins is worse than one that stops, because a
    /// reader cannot tell the two apart.
    pub fn record_login(&mut self, timestamp: u64) {
        let mut buf = String::new();
        let _ = write!(buf, "{timestamp}");
        self.set_bare(field::LAST_LOGIN, &buf);
        let next = self.login_count().saturating_add(1);
        buf.clear();
        let _ = write!(buf, "{next}");
        self.set_bare(field::LOGIN_COUNT, &buf);
    }

    // ---- Passwords ----

    /// Check `password` against this record.
    ///
    /// Takes the stored entry as `crypt`'s *setting* and recomputes it, which
    /// is why nothing here parses a salt or slices a hash: a stored entry is
    /// a valid setting, so a correct password reproduces it byte for byte.
    /// Every one of the three bugs this crate replaces was in code that took
    /// the entry apart by hand.
    #[must_use]
    pub fn check_password(&self, password: &str) -> Auth {
        if self.is_locked() {
            return Auth::Locked;
        }
        let Some(stored) = self.get(field::PASSWORD_HASH) else {
            return Auth::NoPassword;
        };
        if stored.is_empty() {
            return Auth::NoPassword;
        }
        if posix::crypt::stored_method(stored.as_bytes()).is_none() {
            return Auth::Unusable;
        }
        if posix::crypt::verify(password.as_bytes(), stored.as_bytes()) {
            Auth::Accepted
        } else {
            Auth::Rejected
        }
    }

    /// Store `password`, hashed, using `salt`.
    ///
    /// The salt is a parameter rather than drawn inside, so that a caller's
    /// tests can pin a known answer. A hash function that supplies its own
    /// randomness can only be tested against itself — which is the test that
    /// let all three of the constructions this replaces pass while being
    /// wrong. Most callers want [`Record::set_password`].
    ///
    /// # Errors
    ///
    /// [`PasswordError::Salt`] if `salt` is not something `crypt` can store
    /// verbatim, and [`PasswordError::Hash`] if hashing fails.
    pub fn set_password_with_salt(
        &mut self,
        password: &str,
        salt: &str,
    ) -> Result<(), PasswordError> {
        let mut setting_buf = posix::crypt::buf();
        let setting =
            posix::crypt::setting_into(PASSWORD_METHOD, salt.as_bytes(), &mut setting_buf)
                .ok_or(PasswordError::Salt)?;
        let mut hash_buf = posix::crypt::buf();
        let hashed =
            posix::crypt::hash_into(password.as_bytes(), setting.as_bytes(), &mut hash_buf)
                .ok_or(PasswordError::Hash)?
                .to_string();
        self.set(field::PASSWORD_HASH, &hashed);
        // A `crypt` entry carries its own salt. A salt stored beside it is a
        // second copy of the same fact, and the two disagreeing is how the
        // old format broke, so the legacy fields go rather than linger.
        for key in field::LEGACY_SALT {
            self.remove(key);
        }
        // Stamp the change here rather than leaving it to the caller. A
        // password whose age is unknown is one that no aging policy can act
        // on: `max_days` measures from this field, so a writer that forgot it
        // would not merely lose a date, it would quietly exempt that one
        // account from expiry for good. Every writer wanting that is a worse
        // bet than every writer remembering. If the clock cannot say what day
        // it is the field is left as it was, since a wrong date is worse than
        // a stale one.
        if let Some(day) = today() {
            self.set_day(field::PASSWORD_CHANGED, Some(day));
        }
        Ok(())
    }

    /// Store `password`, hashed with a salt read from `/dev/urandom`.
    ///
    /// # Errors
    ///
    /// [`PasswordError::NoRandomness`] if `/dev/urandom` cannot be read.
    /// There is deliberately no fallback: a generated-from-the-clock salt is a
    /// salt in shape only — whatever seeds it is public, so one precomputed
    /// table covers every account salted alongside it, which is the exact
    /// property a salt exists to deny.
    pub fn set_password(&mut self, password: &str) -> Result<(), PasswordError> {
        let salt = random_salt().ok_or(PasswordError::NoRandomness)?;
        self.set_password_with_salt(password, &salt)
    }

    /// Whether the stored entry is one of the two formats this crate
    /// replaced, which no password can be checked against.
    #[must_use]
    pub fn has_legacy_password(&self) -> bool {
        match self.get(field::PASSWORD_HASH) {
            Some(h) if !h.is_empty() => posix::crypt::stored_method(h.as_bytes()).is_none(),
            _ => false,
        }
    }

    /// Render the record's lines.
    fn write_to(&self, out: &mut String) {
        let mut first_field = true;
        for line in &self.lines {
            match line {
                Line::Field {
                    indent,
                    key,
                    raw_value,
                } => {
                    if first_field {
                        out.push_str(&self.dash_indent);
                        out.push_str("- ");
                        first_field = false;
                    } else {
                        out.push_str(indent);
                    }
                    out.push_str(key);
                    out.push_str(": ");
                    out.push_str(raw_value);
                    out.push('\n');
                }
                Line::Other(text) => {
                    out.push_str(text);
                    out.push('\n');
                }
            }
        }
    }
}

/// The outcome of checking a password.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Auth {
    /// The password is correct.
    Accepted,
    /// The password is wrong.
    Rejected,
    /// The account is locked; no password is accepted.
    Locked,
    /// The account has no password set at all.
    NoPassword,
    /// The stored entry is in one of the pre-`crypt(3)` formats. No password
    /// can be checked against it — the two tools that wrote that format
    /// disagreed about what its bytes meant, so there is no single thing it
    /// can be said to be. An administrator must set a new password.
    Unusable,
}

/// Why a password could not be stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasswordError {
    /// `/dev/urandom` could not be read, so there is no salt.
    NoRandomness,
    /// The salt is not something `crypt` can store verbatim.
    Salt,
    /// Hashing failed.
    Hash,
}

impl core::fmt::Display for PasswordError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let text = match self {
            Self::NoRandomness => {
                "cannot read `/dev/urandom', so there is no random salt to store this \
                 password with; refusing to write a password without one"
            }
            Self::Salt => "the generated salt is not one crypt can store verbatim",
            Self::Hash => "the password could not be hashed",
        };
        f.write_str(text)
    }
}

impl std::error::Error for PasswordError {}

/// The whole database.
#[derive(Debug, Clone, Default)]
pub struct UserDb {
    /// Everything before the first record, including the `users:` line and
    /// any comments above it.
    preamble: Vec<String>,
    records: Vec<Record>,
}

impl UserDb {
    /// An empty database with the conventional header.
    #[must_use]
    pub fn new() -> Self {
        Self {
            preamble: vec![
                "# Slate OS user database".to_string(),
                "# Managed by useradm and the login manager".to_string(),
                "users:".to_string(),
            ],
            records: Vec::new(),
        }
    }

    /// Parse a database. Infallible: a line this does not understand is kept
    /// as text, because the useful answer to an unreadable field is the same
    /// as to an absent one, and corrupting the file is never an improvement.
    #[must_use]
    pub fn parse(text: &str) -> Self {
        let mut preamble = Vec::new();
        let mut records: Vec<Record> = Vec::new();

        for line in text.lines() {
            let trimmed = line.trim_start();
            let indent_len = line.len().saturating_sub(trimmed.len());
            let indent = line.get(..indent_len).unwrap_or("").to_string();

            if let Some(rest) = trimmed.strip_prefix("- ") {
                let mut record = Record {
                    dash_indent: indent.clone(),
                    // Until a second line arrives, guess the conventional
                    // "two more than the dash".
                    field_indent: format!("{indent}  "),
                    lines: Vec::new(),
                };
                match split_field(rest) {
                    Some((key, raw_value)) => record.lines.push(Line::Field {
                        indent: record.field_indent.clone(),
                        key,
                        raw_value,
                    }),
                    None => record.lines.push(Line::Other(line.to_string())),
                }
                records.push(record);
                continue;
            }

            let Some(record) = records.last_mut() else {
                preamble.push(line.to_string());
                continue;
            };

            if trimmed.is_empty() || trimmed.starts_with('#') {
                record.lines.push(Line::Other(line.to_string()));
                continue;
            }

            match split_field(trimmed) {
                Some((key, raw_value)) => {
                    record.field_indent.clone_from(&indent);
                    record.lines.push(Line::Field {
                        indent,
                        key,
                        raw_value,
                    });
                }
                None => record.lines.push(Line::Other(line.to_string())),
            }
        }

        Self { preamble, records }
    }

    /// Render the database. Anything the caller did not change comes out as
    /// it went in.
    #[must_use]
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        for line in &self.preamble {
            out.push_str(line);
            out.push('\n');
        }
        for record in &self.records {
            record.write_to(&mut out);
        }
        out
    }

    /// Read the database from `path`.
    ///
    /// A file that does not exist is an empty database; every *other* failure
    /// is an error. The distinction is the whole point of returning a
    /// `Result`: both writers previously read with `Err(_) => Vec::new()`, so
    /// running one as a user who may not read `/etc/users.yaml` produced an
    /// empty database, and the next write then saved that empty database over
    /// the real one — one permission error away from deleting every account on
    /// the machine.
    ///
    /// # Errors
    ///
    /// Any I/O error other than "not found", and a file that is not UTF-8.
    /// YAML is defined to be UTF-8, so refusing is better than transcoding: a
    /// lossy read would rewrite the file with U+FFFD where a display name used
    /// to be.
    pub fn load(path: impl AsRef<std::path::Path>) -> std::io::Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => Ok(Self::parse(&text)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::new()),
            Err(e) => Err(e),
        }
    }

    /// Write the database to `path`, and regenerate the two POSIX flat files
    /// beside it, atomically.
    ///
    /// Each of the three texts goes to a sibling temporary file, and only when
    /// all three are on disk are they renamed into place. A crash or a full
    /// disk therefore leaves the previous three intact, and the window in
    /// which they can disagree with each other is three renames wide rather
    /// than three writes wide. Writing in place would give a window in which
    /// `/etc/users.yaml` is truncated, and a machine that loses power in that
    /// window has no accounts and no way to log in and fix it.
    ///
    /// The flat files are `passwd` and `shadow` in `path`'s own directory —
    /// derived rather than passed, so that generation cannot be forgotten by a
    /// caller and a test saving into a scratch directory cannot write over the
    /// real `/etc/passwd`. See [`UserDb::to_passwd_text`] for what §353 asks
    /// of them.
    ///
    /// # Errors
    ///
    /// [`std::io::ErrorKind::InvalidData`] wrapping a [`GenerateError`] if a
    /// record cannot be written to `/etc/passwd` — checked *before* anything
    /// is written, so such a database fails the save with all three files
    /// untouched rather than after the YAML has already moved. Otherwise any
    /// I/O error from creating, writing or renaming a temporary file.
    pub fn save(&self, path: impl AsRef<std::path::Path>) -> std::io::Result<()> {
        let path = path.as_ref();
        let passwd = self.to_passwd_text(path).map_err(unrepresentable)?;
        let shadow = self.to_shadow_text(path).map_err(unrepresentable)?;
        let dir = path.parent().unwrap_or_else(|| std::path::Path::new(""));

        let plan = [
            (path.to_path_buf(), self.to_text(), Access::Shared),
            (dir.join(PASSWD_NAME), passwd, Access::Shared),
            (dir.join(SHADOW_NAME), shadow, Access::Private),
        ];

        let mut staged: Vec<Staged> = Vec::with_capacity(plan.len());
        for (target, text, access) in plan {
            match Staged::write(&target, &text, access) {
                Ok(one) => staged.push(one),
                Err(e) => {
                    for done in staged {
                        done.abandon();
                    }
                    return Err(e);
                }
            }
        }

        // The database is renamed first, so that a rename that fails partway
        // leaves the truth behind its derived files rather than ahead of them:
        // a stale `/etc/passwd` is an account that has not appeared yet, while
        // a stale `/etc/users.yaml` would be an account that appears in
        // `/etc/passwd` and does not exist.
        //
        // Every rename is attempted even after one fails, because stopping
        // would leave the rest stale as well, and the *first* error is
        // reported rather than the last: it is the one that says which file
        // the divergence starts at.
        let mut first_error = None;
        for one in staged {
            if let Err(e) = one.commit()
                && first_error.is_none()
            {
                first_error = Some(e);
            }
        }
        match first_error {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    /// Render `/etc/passwd` from the database, naming `source` in its header.
    ///
    /// `design-decisions.md` §353 makes this file *generated*: the YAML is the
    /// truth and this is a copy of it kept for ported software that reads the
    /// flat file directly. It is read-only in intent and not in permission, so
    /// the header comment naming the source is the only warning a person
    /// hand-editing it will get.
    ///
    /// A record with no `gid` is written with its uid as its gid — see
    /// [`Record::gid`].
    ///
    /// # Errors
    ///
    /// [`GenerateError`] if any record cannot be represented as a
    /// colon-separated line. This is a hard failure and not a skipped record
    /// on purpose: the entire point of §353 is that the two files agree, and a
    /// user quietly present in one and absent from the other is the exact
    /// defect it was decided to end.
    pub fn to_passwd_text(&self, source: &std::path::Path) -> Result<String, GenerateError> {
        let mut out = generated_header(source);
        for (index, record) in self.records.iter().enumerate() {
            let username = login_name(record, index)?;
            let uid = record.uid().ok_or(GenerateError::NoUid {
                username: username.clone(),
            })?;
            let gid = record.gid().unwrap_or(uid);
            // `display_name`'s username fallback is deliberately not used: an
            // absent display name means the record has none, and the GECOS
            // field's own spelling of that is empty, not a second copy of the
            // login name.
            let gecos = writable(
                record.get(field::DISPLAY_NAME).unwrap_or_default(),
                &username,
                field::DISPLAY_NAME,
            )?;
            let home = writable(record.home().unwrap_or_default(), &username, field::HOME)?;
            let shell = writable(record.shell().unwrap_or_default(), &username, field::SHELL)?;
            // The password field is always `x`: the hash lives in `shadow`,
            // which is what the two-file split is for. `writeln!` to a
            // `String` is infallible; the result is discarded rather than
            // unwrapped so that no formatting path can panic.
            let _ = writeln!(out, "{username}:x:{uid}:{gid}:{gecos}:{home}:{shell}");
        }
        Ok(out)
    }

    /// Render `/etc/shadow` from the database, naming `source` in its header.
    ///
    /// See [`UserDb::to_passwd_text`]; this is the other half of the same
    /// generation, and carries the password entries the account file does not.
    ///
    /// # Errors
    ///
    /// [`GenerateError`], on the same terms as [`UserDb::to_passwd_text`].
    pub fn to_shadow_text(&self, source: &std::path::Path) -> Result<String, GenerateError> {
        let mut out = generated_header(source);
        for (index, record) in self.records.iter().enumerate() {
            let username = login_name(record, index)?;
            let entry = writable(shadow_entry(record), &username, field::PASSWORD_HASH)?;
            let a = record.aging();
            // `login:password:lastchg:min:max:warn:inactive:expire:reserved`.
            // A field the record does not carry is written empty, which is
            // `/etc/shadow`'s own spelling of "no policy for this" — not
            // filled in with a plausible default. `0:0:99999:7` is the one
            // `useradd` writes, and writing it here would present an
            // invention as a fact, hiding the fact that nothing has set a
            // policy for this account. The trailing field is reserved and has
            // been empty in every implementation since the format was
            // defined. `writeln!` to a `String` is infallible; the result is
            // discarded rather than unwrapped so no path can panic.
            let _ = writeln!(
                out,
                "{username}:{entry}:{}:{}:{}:{}:{}:{}:",
                day(a.changed),
                day(a.min_days),
                day(a.max_days),
                day(a.warn_days),
                day(a.inactive_days),
                day(a.expires),
            );
        }
        Ok(out)
    }

    /// The records, in file order.
    #[must_use]
    pub fn records(&self) -> &[Record] {
        &self.records
    }

    /// The records, mutably.
    pub fn records_mut(&mut self) -> &mut Vec<Record> {
        &mut self.records
    }

    /// The record for `username`, if there is one.
    #[must_use]
    pub fn find(&self, username: &str) -> Option<&Record> {
        self.records
            .iter()
            .find(|r| r.username().as_deref() == Some(username))
    }

    /// The record for `username`, mutably.
    pub fn find_mut(&mut self, username: &str) -> Option<&mut Record> {
        self.records
            .iter_mut()
            .find(|r| r.username().as_deref() == Some(username))
    }

    /// The record for `uid`, if there is one.
    #[must_use]
    pub fn find_uid(&self, uid: u32) -> Option<&Record> {
        self.records.iter().find(|r| r.uid() == Some(uid))
    }

    /// Append a record.
    pub fn push(&mut self, record: Record) {
        self.records.push(record);
    }

    /// Remove the record for `username`, reporting whether there was one.
    pub fn remove(&mut self, username: &str) -> bool {
        let before = self.records.len();
        self.records
            .retain(|r| r.username().as_deref() != Some(username));
        self.records.len() != before
    }
}

// ============================================================================
// Generating the POSIX flat files (`design-decisions.md` §353)
// ============================================================================

/// Why the flat files could not be generated from the database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GenerateError {
    /// The record at this position — counting from one, in file order — has
    /// no `username`, so there is nothing to put in the first field of either
    /// file.
    Nameless {
        /// The record's position in the file, counting from one.
        index: usize,
    },
    /// The record at this position has a `username` holding a byte the
    /// colon-separated format cannot carry.
    ///
    /// Reported by position rather than by name, unlike every other variant,
    /// because a name that cannot be written to `/etc/passwd` is a name whose
    /// bytes should not be written to a terminal either: a control character
    /// in it is an escape sequence in the error message.
    UnwritableName {
        /// The record's position in the file, counting from one.
        index: usize,
    },
    /// The record has no `uid`, or one that is not a number in range.
    NoUid {
        /// The account the record names.
        username: String,
    },
    /// A field holds a byte the colon-separated format cannot carry.
    Unwritable {
        /// The account the record names.
        username: String,
        /// The database field, by its canonical name.
        field: &'static str,
    },
}

impl core::fmt::Display for GenerateError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Nameless { index } => write!(
                f,
                "record {index} of the user database has no `username', so it cannot be \
                 written to `/etc/passwd'"
            ),
            Self::UnwritableName { index } => write!(
                f,
                "record {index} of the user database has a `username' holding a colon or a \
                 control character, which `/etc/passwd' cannot carry"
            ),
            Self::NoUid { username } => write!(
                f,
                "user `{username}' has no numeric `uid', so it cannot be written to \
                 `/etc/passwd'"
            ),
            Self::Unwritable { username, field } => write!(
                f,
                "user `{username}' has a `{field}' holding a colon or a control character, \
                 which `/etc/passwd' cannot carry"
            ),
        }
    }
}

impl std::error::Error for GenerateError {}

/// Wrap a [`GenerateError`] as the I/O error [`UserDb::save`] reports.
///
/// `InvalidData` and not `Other`: the failure is a property of the bytes being
/// written, and a caller that distinguishes kinds should be able to tell it
/// from a full disk.
/// Render one aging field: the number, or nothing at all when unset.
fn day(value: Option<i64>) -> String {
    let mut buf = String::new();
    if let Some(days) = value {
        // `write!` to a `String` is infallible; the result is discarded
        // rather than unwrapped so that no formatting path can panic.
        let _ = write!(buf, "{days}");
    }
    buf
}

fn unrepresentable(e: GenerateError) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, e)
}

/// Whether `value` can be one field of a colon-separated line.
///
/// A colon would shift every field after it — a display name of `a:b` moves
/// the home directory into the shell — and a newline would end the line early,
/// making the rest of the record a second, forged one. Every other control
/// character is refused with them because the file is read by programs that
/// print what they find, and a terminal escape in a login name is a login name
/// that can rewrite the screen it is printed on. Bytes at or above `0x80` are
/// fine: a display name is allowed to be in a language.
fn writable(value: String, username: &str, field: &'static str) -> Result<String, GenerateError> {
    if value.bytes().any(|b| b == b':' || b < 0x20 || b == 0x7f) {
        return Err(GenerateError::Unwritable {
            username: username.to_string(),
            field,
        });
    }
    Ok(value)
}

/// The login name a record contributes to both files, or why it cannot.
fn login_name(record: &Record, index: usize) -> Result<String, GenerateError> {
    // Counting from one: the error is read by a person looking at a file, and
    // the first record in a file is the first one, not the zeroth.
    let position = index.saturating_add(1);
    let name = record.username().unwrap_or_default();
    if name.is_empty() {
        return Err(GenerateError::Nameless { index: position });
    }
    match writable(name, "", field::USERNAME) {
        Ok(name) => Ok(name),
        Err(_) => Err(GenerateError::UnwritableName { index: position }),
    }
}

/// The header both generated files open with.
///
/// §353 item 4 is explicit that this comment is the whole mitigation for the
/// one genuinely surprising thing about the decision — that these files look
/// writable and are not — so it says what happens to an edit, not merely that
/// the file is generated.
fn generated_header(source: &std::path::Path) -> String {
    let shown = source.display().to_string();
    // A path holding a control character would end the comment line and turn
    // the rest of it into a forged `passwd` record. Such a path is not
    // repaired, for the reason a repaired name is worse than an absent one:
    // it looks real.
    let named = if shown.bytes().any(|b| b < 0x20 || b == 0x7f) {
        "the user database".to_string()
    } else {
        format!("`{shown}'")
    };
    format!(
        "# Generated from {named} -- do not edit.\n\
         # Every change to an account rewrites this file from that one, so an edit\n\
         # made here survives only until the next one and is then silently undone.\n"
    )
}

/// The `/etc/shadow` password entry a record generates.
///
/// Three translations happen here, and each is a place the two files could
/// otherwise come to disagree:
///
/// * **A lock recorded only in the YAML grows a marker.** `locked: true` is
///   this database's spelling; a `!` before the entry is `/etc/shadow`'s. A
///   record locked by the former alone would generate an unlocked shadow line,
///   and a lock that only one of two files records is not a lock.
/// * **An existing marker is preserved exactly.** `!` and `*` are not
///   synonyms — `!` is a lock an administrator applied and can lift, `*` is an
///   account that never had a password — and normalising them would erase
///   which of the two an account is.
/// * **A pre-`crypt(3)` entry becomes `*`.** Those entries (§329) have no
///   `$id$` prefix, so writing one out verbatim would have `crypt` read it as
///   a DES *setting* and compare candidate passwords against a hash of their
///   first eight characters — turning "no password can be checked against
///   this" into "some password can". `*` is the flat file's own spelling of
///   the former, and it does not put a legacy secret-derived value into a
///   second file on the way.
fn shadow_entry(record: &Record) -> String {
    let stored = record.get(field::PASSWORD_HASH).unwrap_or_default();
    let bare = stored.trim_start_matches(['!', '*']);
    let marker_len = stored.len().saturating_sub(bare.len());
    let mut marker = stored.get(..marker_len).unwrap_or_default().to_string();
    if marker.is_empty() && record.is_locked() {
        marker.push('!');
    }

    if bare.is_empty() {
        // No password at all. An empty entry is `/etc/shadow`'s spelling of
        // exactly that; what it is then permitted to do is §346's policy, not
        // this function's.
        return marker;
    }
    if posix::crypt::stored_method(bare.as_bytes()).is_none() {
        return "*".to_string();
    }
    let mut out = marker;
    out.push_str(bare);
    out
}

/// Who may read a file this crate writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Access {
    /// Whatever the process umask says. `/etc/passwd` is read by every program
    /// that turns a uid into a name, most of them unprivileged.
    Shared,
    /// The owner only. `/etc/shadow` exists precisely so that the password
    /// hashes are *not* in the world-readable account file, and generating it
    /// with the default permissions would put them back where the split was
    /// invented to remove them from.
    Private,
}

/// Make `path` readable only by its owner.
#[cfg(unix)]
fn restrict(path: &std::path::Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
}

/// On a platform with no Unix permission bits there is nothing to restrict.
/// Present so that the development host runs the same code path the target
/// does, up to this one call.
#[cfg(not(unix))]
// The signature is fixed by the Unix arm above, which genuinely can fail; this
// arm has to match it or every caller would need a `cfg` of its own.
#[allow(clippy::unnecessary_wraps)]
fn restrict(_path: &std::path::Path) -> std::io::Result<()> {
    Ok(())
}

/// A file written to its temporary sibling and waiting to be renamed into
/// place.
///
/// The type exists so that a multi-file write can stage everything before
/// committing anything: with three files to keep in step, a failure partway
/// through the *writes* must not leave a rename already done.
struct Staged {
    temp: std::path::PathBuf,
    target: std::path::PathBuf,
}

impl Staged {
    /// Write `text` to `target`'s temporary sibling.
    fn write(target: &std::path::Path, text: &str, access: Access) -> std::io::Result<Self> {
        let mut temp = target.as_os_str().to_os_string();
        temp.push(".tmp");
        let temp = std::path::PathBuf::from(temp);
        std::fs::write(&temp, text)?;
        if access == Access::Private {
            // Restricted before the rename, so the file is never visible at
            // its real name with the wrong permissions -- a window in which
            // `/etc/shadow` is world-readable is all an attacker needs.
            if let Err(e) = restrict(&temp) {
                let _ = std::fs::remove_file(&temp);
                return Err(e);
            }
        }
        Ok(Self {
            temp,
            target: target.to_path_buf(),
        })
    }

    /// Rename the temporary over its target.
    fn commit(self) -> std::io::Result<()> {
        match std::fs::rename(&self.temp, &self.target) {
            Ok(()) => Ok(()),
            Err(e) => {
                self.abandon();
                Err(e)
            }
        }
    }

    /// Remove the temporary without committing it. Leaving one behind would
    /// make the next save look like it had already half-succeeded.
    fn abandon(self) {
        let _ = std::fs::remove_file(&self.temp);
    }
}

/// Draw a salt from `/dev/urandom`, or `None` if it cannot be read.
///
/// `& 0x3f` is an unbiased reduction and not the usual modulo mistake: 256 is
/// exactly four times 64, so every alphabet character is the image of exactly
/// four byte values.
#[must_use]
pub fn random_salt() -> Option<String> {
    const ALPHABET: &[u8; 64] = b"./0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    let len = PASSWORD_METHOD.salt_max();
    let data = std::fs::read("/dev/urandom").ok()?;
    Some(
        data.get(..len)?
            .iter()
            .map(|b| char::from(*ALPHABET.get(usize::from(*b & 0x3f)).unwrap_or(&b'.')))
            .collect(),
    )
}

/// Split `key: value`, leaving the value's quoting alone.
fn split_field(text: &str) -> Option<(String, String)> {
    let colon = text.find(':')?;
    let key = text.get(..colon)?.trim();
    if key.is_empty() || key.contains(' ') || key.contains('"') {
        return None;
    }
    let raw_value = text.get(colon.checked_add(1)?..)?.trim().to_string();
    Some((key.to_string(), raw_value))
}

/// Remove one layer of quoting, undoing the escapes [`quote`] adds.
fn unquote(raw: &str) -> String {
    let trimmed = raw.trim();
    let Some(inner) = trimmed.strip_prefix('"').and_then(|s| s.strip_suffix('"')) else {
        return trimmed.trim_matches('\'').to_string();
    };
    let mut out = String::with_capacity(inner.len());
    let mut escaped = false;
    for ch in inner.chars() {
        if escaped {
            out.push(match ch {
                'n' => '\n',
                't' => '\t',
                other => other,
            });
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else {
            out.push(ch);
        }
    }
    if escaped {
        out.push('\\');
    }
    out
}

/// Quote a value for writing.
///
/// Escaping is not optional here even though the old writers omitted it: a
/// display name containing a quotation mark — or a newline, which a careless
/// caller can pass — would otherwise end the value early and corrupt every
/// record after it in the file.
fn quote(value: &str) -> String {
    let mut out = String::with_capacity(value.len().saturating_add(2));
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

// The workspace's defensive lints are for production code; a test that indexes
// a fixture it just built is asserting, and an assertion that fails by
// panicking is a test doing its job.
#[allow(
    clippy::indexing_slicing,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects
)]
#[cfg(test)]
mod tests {
    // Scratch directories come from the shared guard: a fixed name races
    // between concurrent test binaries and the clock is not a unique id.
    use super::*;
    use scratchdir::ScratchDir;

    /// A database as the login manager used to write it.
    const LOGIN_MANAGER_DIALECT: &str = "\
# Slate OS User Database
# DO NOT EDIT MANUALLY

users:
  - uid: 0
    username: \"root\"
    display_name: \"Administrator\"
    password_hash: \"deadbeef\"
    password_salt: \"00112233445566778899aabbccddeeff\"
    avatar_path: null
    shell: \"/bin/nush\"
    home_dir: \"/root\"
    is_admin: true
    auto_login: false
    last_login_timestamp: 1699999999
    login_count: 42
";

    /// The same file as `useradm` used to write it.
    const USERADM_DIALECT: &str = "\
# Slate OS user database
# Managed by useradm — do not edit manually
users:
  - uid: 1000
    username: \"alice\"
    display_name: \"Alice\"
    password_hash: \"cafebabe\"
    salt: \"ffeeddccbbaa99887766554433221100\"
    shell: \"/bin/sh\"
    home: \"/home/alice\"
    groups: [\"users\", \"wheel\"]
    admin: false
    locked: false
    avatar: \"/usr/share/avatars/alice.png\"
";

    #[test]
    fn a_file_round_trips_byte_for_byte_when_nothing_is_changed() {
        for text in [LOGIN_MANAGER_DIALECT, USERADM_DIALECT] {
            assert_eq!(UserDb::parse(text).to_text(), text);
        }
    }

    /// The property the two old writers broke: a field one tool does not know
    /// about must survive the other tool rewriting the file.
    #[test]
    fn a_write_preserves_the_fields_the_writer_does_not_know_about() {
        let mut db = UserDb::parse(LOGIN_MANAGER_DIALECT);
        db.find_mut("root").unwrap().set(field::SHELL, "/bin/osh");
        let text = db.to_text();

        assert!(text.contains("shell: \"/bin/osh\""), "{text}");
        for kept in [
            "last_login_timestamp: 1699999999",
            "login_count: 42",
            "auto_login: false",
            "avatar_path: null",
        ] {
            assert!(text.contains(kept), "lost {kept}:\n{text}");
        }
        assert!(text.contains("# DO NOT EDIT MANUALLY"), "{text}");
    }

    #[test]
    fn a_write_preserves_useradms_fields_too() {
        let mut db = UserDb::parse(USERADM_DIALECT);
        db.find_mut("alice").unwrap().set_locked(true);
        let text = db.to_text();
        assert!(text.contains("locked: true"), "{text}");
        assert!(text.contains("groups: [\"users\", \"wheel\"]"), "{text}");
        assert!(text.contains("# Managed by useradm"), "{text}");
    }

    /// Both spellings of the fields the two writers named differently are
    /// read; neither tool wrote both, so there is nothing to disambiguate.
    #[test]
    fn both_dialects_read_the_same_way() {
        let login = UserDb::parse(LOGIN_MANAGER_DIALECT);
        let root = login.find("root").unwrap();
        assert_eq!(root.uid(), Some(0));
        assert_eq!(root.home().as_deref(), Some("/root"));
        assert!(root.is_admin());
        assert_eq!(root.avatar(), None);

        let useradm = UserDb::parse(USERADM_DIALECT);
        let alice = useradm.find("alice").unwrap();
        assert_eq!(alice.uid(), Some(1000));
        assert_eq!(alice.home().as_deref(), Some("/home/alice"));
        assert!(!alice.is_admin());
        assert_eq!(
            alice.avatar().as_deref(),
            Some("/usr/share/avatars/alice.png")
        );
        assert_eq!(alice.groups(), vec!["users", "wheel"]);
    }

    /// The bug this crate exists for, stated as a test: a password set once is
    /// accepted once, by the same code both writers now call.
    #[test]
    fn a_password_set_here_is_accepted_here() {
        let mut record = Record::new();
        record.set(field::USERNAME, "alice");
        record
            .set_password_with_salt("correct horse", "saltsalt")
            .expect("set");
        assert_eq!(record.check_password("correct horse"), Auth::Accepted);
        assert_eq!(record.check_password("Correct horse"), Auth::Rejected);
        assert_eq!(record.check_password(""), Auth::Rejected);
    }

    /// A known answer, not merely a self-consistent one. The three
    /// constructions this replaces all passed determinism and difference
    /// tests, which are true of any function written by accident.
    #[test]
    fn the_stored_entry_matches_a_published_vector() {
        let mut record = Record::new();
        record
            .set_password_with_salt("Hello world!", "saltstring")
            .expect("set");
        assert_eq!(
            record.get(field::PASSWORD_HASH).as_deref(),
            Some(
                "$6$saltstring$svn8UoSVapNtMuq1ukKS4tPQd8iKwSMHWjl/O817G3uBnIFNjnQJu\
                 esI68u4OTLiBFdcbYEdFCoEOfaS35inz1"
            )
        );
    }

    #[test]
    fn a_password_written_and_reparsed_still_verifies() {
        let mut db = UserDb::parse(USERADM_DIALECT);
        db.find_mut("alice")
            .unwrap()
            .set_password_with_salt("hunter2", "abcdefgh")
            .expect("set");
        let reparsed = UserDb::parse(&db.to_text());
        assert_eq!(
            reparsed.find("alice").unwrap().check_password("hunter2"),
            Auth::Accepted
        );
    }

    /// Setting a password removes the legacy salt field: a `crypt` entry
    /// carries its own salt, and two copies of one fact can disagree.
    #[test]
    fn setting_a_password_drops_the_legacy_salt_fields() {
        for text in [LOGIN_MANAGER_DIALECT, USERADM_DIALECT] {
            let mut db = UserDb::parse(text);
            let name = db.records()[0].username().unwrap();
            db.find_mut(&name)
                .unwrap()
                .set_password_with_salt("pw", "abcdefgh")
                .expect("set");
            let out = db.to_text();
            assert!(!out.contains("password_salt:"), "{out}");
            assert!(!out.contains("\n    salt:"), "{out}");
        }
    }

    #[test]
    fn the_old_formats_are_recognised_and_refuse_every_password() {
        for text in [LOGIN_MANAGER_DIALECT, USERADM_DIALECT] {
            let db = UserDb::parse(text);
            let record = &db.records()[0];
            assert!(record.has_legacy_password());
            assert_eq!(record.check_password("anything"), Auth::Unusable);
            assert_eq!(record.check_password(""), Auth::Unusable);
        }
    }

    #[test]
    fn a_locked_account_accepts_nothing() {
        let mut record = Record::new();
        record.set(field::USERNAME, "bob");
        record
            .set_password_with_salt("pw", "abcdefgh")
            .expect("set");
        assert_eq!(record.check_password("pw"), Auth::Accepted);
        record.set_locked(true);
        assert_eq!(record.check_password("pw"), Auth::Locked);
        record.set_locked(false);
        assert_eq!(record.check_password("pw"), Auth::Accepted);
    }

    /// `!` in front of the entry is how `/etc/shadow` spells a lock, and a
    /// lock only one tool can see is not a lock. The check is a prefix test,
    /// not an equality test, so `!$6$…` cannot fall through and be verified
    /// with `!` as its salt.
    #[test]
    fn a_bang_prefixed_entry_is_locked_not_verified() {
        let mut record = Record::new();
        record
            .set_password_with_salt("pw", "abcdefgh")
            .expect("set");
        let hash = record.get(field::PASSWORD_HASH).unwrap();
        record.set(field::PASSWORD_HASH, &format!("!{hash}"));
        assert_eq!(record.check_password("pw"), Auth::Locked);
    }

    #[test]
    fn an_account_with_no_entry_is_not_an_account_with_no_password_check() {
        let record = Record::new();
        assert_eq!(record.check_password(""), Auth::NoPassword);
        assert_eq!(record.check_password("x"), Auth::NoPassword);
    }

    #[test]
    fn a_salt_crypt_cannot_carry_is_refused_rather_than_truncated() {
        let mut record = Record::new();
        assert_eq!(
            record.set_password_with_salt("pw", "has$dollar"),
            Err(PasswordError::Salt)
        );
        assert_eq!(
            record.set_password_with_salt("pw", ""),
            Err(PasswordError::Salt)
        );
        // 17 characters, one past SHA-crypt's maximum.
        assert_eq!(
            record.set_password_with_salt("pw", "abcdefghijklmnopq"),
            Err(PasswordError::Salt)
        );
        assert!(!record.contains(field::PASSWORD_HASH));
    }

    /// A quotation mark in a display name used to end the value early and
    /// corrupt every record after it.
    #[test]
    fn a_value_containing_quotes_survives_a_round_trip() {
        let mut db = UserDb::parse(USERADM_DIALECT);
        let awkward = "Alice \"the admin\" O'Brien\\";
        db.find_mut("alice")
            .unwrap()
            .set(field::DISPLAY_NAME, awkward);
        let reparsed = UserDb::parse(&db.to_text());
        assert_eq!(
            reparsed
                .find("alice")
                .unwrap()
                .get(field::DISPLAY_NAME)
                .as_deref(),
            Some(awkward)
        );
        // And the record after it is still intact.
        assert_eq!(reparsed.find("alice").unwrap().uid(), Some(1000));
    }

    #[test]
    fn a_newline_in_a_value_cannot_forge_a_field() {
        let mut record = Record::new();
        record.set(field::DISPLAY_NAME, "Eve\n    is_admin: true");
        let mut out = String::new();
        record.write_to(&mut out);
        assert_eq!(out.lines().count(), 1, "{out}");
        let reparsed = UserDb::parse(&format!("users:\n{out}"));
        assert!(!reparsed.records()[0].is_admin(), "{out}");
    }

    /// A file that spells a field both ways must not come out of a write
    /// spelling it two *different* ways — that is the original bug, one level
    /// down.
    #[test]
    fn setting_a_dual_spelled_field_updates_every_spelling_present() {
        let mut record =
            UserDb::parse("users:\n  - uid: 5\n    home_dir: \"/home/a\"\n    home: \"/home/a\"\n")
                .records_mut()
                .remove(0);
        record.set_home("/home/b");
        let mut out = String::new();
        record.write_to(&mut out);
        assert!(out.contains("home_dir: \"/home/b\""), "{out}");
        assert!(out.contains("\n    home: \"/home/b\""), "{out}");
        assert_eq!(record.home().as_deref(), Some("/home/b"));
    }

    /// The alias is used when the record already uses it, so useradm's own
    /// files keep their shape rather than growing a second field.
    #[test]
    fn setting_a_dual_spelled_field_keeps_the_records_own_spelling() {
        let mut record =
            UserDb::parse("users:\n  - uid: 5\n    home: \"/home/a\"\n    admin: false\n")
                .records_mut()
                .remove(0);
        record.set_home("/home/b");
        record.set_admin(true);
        let mut out = String::new();
        record.write_to(&mut out);
        assert!(!out.contains("home_dir"), "{out}");
        assert!(!out.contains("is_admin"), "{out}");
        assert!(out.contains("home: \"/home/b\""), "{out}");
        assert!(out.contains("admin: true"), "{out}");
        assert!(record.is_admin());
    }

    /// An empty avatar is `null`, not `""` — an empty string reads back as a
    /// path, and a path to nowhere is not the same as no avatar.
    #[test]
    fn clearing_the_avatar_writes_null_rather_than_an_empty_path() {
        let mut record = Record::new();
        record.set_avatar("/usr/share/faces/a.png");
        assert_eq!(record.avatar().as_deref(), Some("/usr/share/faces/a.png"));
        record.set_avatar("");
        assert_eq!(record.avatar(), None);
        let mut out = String::new();
        record.write_to(&mut out);
        assert!(out.contains("avatar_path: null"), "{out}");
    }

    /// A database that cannot be read must not be reported as a database with
    /// no accounts in it, because the caller's next act is to write it back.
    #[test]
    fn an_unreadable_file_is_an_error_and_a_missing_one_is_empty() {
        let scratch = ScratchDir::new("userdb-load-test");
        let missing = scratch.path("does-not-exist.yaml");
        assert!(
            UserDb::load(&missing)
                .expect("missing is empty")
                .records()
                .is_empty()
        );

        // A directory stands in for "present but unreadable": every platform
        // refuses to read one as a file, and it is not a `NotFound`.
        assert!(UserDb::load(scratch.dir()).is_err());
    }

    #[test]
    fn a_save_replaces_the_file_and_leaves_no_temporary_behind() {
        let scratch = ScratchDir::new("userdb-save-test");
        let path = scratch.path("users.yaml");
        let mut db = UserDb::new();
        let mut record = Record::new();
        record.set_uid(1000);
        record.set(field::USERNAME, "dave");
        db.push(record);
        db.save(&path).expect("save");

        let reloaded = UserDb::load(&path).expect("load");
        assert_eq!(reloaded.find("dave").and_then(Record::uid), Some(1000));
        let mut temp = path.as_os_str().to_os_string();
        temp.push(".tmp");
        assert!(!std::path::Path::new(&temp).exists());
    }

    // ---- The generated flat files (§353) ----

    /// A database with one ordinary account, ready to be generated from.
    fn one_account() -> UserDb {
        let mut db = UserDb::new();
        let mut record = Record::new();
        record.set_uid(1000);
        record.set(field::USERNAME, "dave");
        record.set(field::DISPLAY_NAME, "Dave Lister");
        record.set(field::HOME, "/home/dave");
        record.set(field::SHELL, "/bin/nush");
        record
            .set_password_with_salt("hunter2", "abcdefgh")
            .expect("hash");
        db.push(record);
        db
    }

    /// The one `/etc/passwd` record `text` holds, read back with the crate the
    /// rest of the system reads that file with.
    fn only_passwd_record(text: &str) -> pwdb::User {
        let mut users = pwdb::users(text.as_bytes());
        assert_eq!(users.len(), 1, "expected exactly one record in {text:?}");
        users.remove(0)
    }

    #[test]
    fn a_generated_passwd_line_reads_back_through_the_reader_the_system_uses() {
        let db = one_account();
        let text = db
            .to_passwd_text(std::path::Path::new(DEFAULT_PATH))
            .expect("generate");
        let user = only_passwd_record(&text);

        assert_eq!(user.name, b"dave");
        assert_eq!(user.uid, 1000);
        assert_eq!(user.gecos, b"Dave Lister");
        assert_eq!(user.dir, b"/home/dave");
        assert_eq!(user.shell, b"/bin/nush");
        // The hash belongs in `shadow`, and `x` is how the account file says
        // so. A generated `passwd` carrying the hash itself would undo the
        // entire point of the two-file split.
        assert_eq!(user.passwd, b"x");
    }

    #[test]
    fn an_account_with_no_gid_is_generated_into_its_own_private_group() {
        let db = one_account();
        let text = db
            .to_passwd_text(std::path::Path::new(DEFAULT_PATH))
            .expect("generate");
        assert_eq!(only_passwd_record(&text).gid, 1000);
    }

    #[test]
    fn an_explicit_gid_is_used_in_preference_to_the_uid() {
        let mut db = one_account();
        db.find_mut("dave").expect("record").set_gid(50);
        let text = db
            .to_passwd_text(std::path::Path::new(DEFAULT_PATH))
            .expect("generate");
        let user = only_passwd_record(&text);
        assert_eq!((user.uid, user.gid), (1000, 50));
    }

    #[test]
    fn the_generated_header_names_the_file_it_was_generated_from() {
        let db = one_account();
        for text in [
            db.to_passwd_text(std::path::Path::new(DEFAULT_PATH))
                .expect("passwd"),
            db.to_shadow_text(std::path::Path::new(DEFAULT_PATH))
                .expect("shadow"),
        ] {
            let first = text.lines().next().unwrap_or_default();
            assert!(first.starts_with('#'), "{first:?}");
            assert!(first.contains(DEFAULT_PATH), "{first:?}");
            // §353 item 4: the comment is the whole mitigation for the file
            // looking writable, so it must say what an edit costs.
            assert!(text.contains("silently undone"), "{text:?}");
        }
    }

    #[test]
    fn the_header_is_a_comment_the_systems_own_reader_skips() {
        // A header that the reader took for a record would add a phantom
        // account to every machine, which is a worse failure than having no
        // header at all -- so this is checked against `pwdb` rather than
        // assumed from the leading `#`.
        let text = UserDb::new()
            .to_passwd_text(std::path::Path::new(DEFAULT_PATH))
            .expect("generate");
        assert!(text.starts_with('#'));
        assert!(pwdb::users(text.as_bytes()).is_empty(), "{text:?}");
    }

    #[test]
    fn a_generated_shadow_line_has_the_nine_fields_the_format_defines() {
        let db = one_account();
        let text = db
            .to_shadow_text(std::path::Path::new(DEFAULT_PATH))
            .expect("generate");
        let line = text
            .lines()
            .find(|l| !l.starts_with('#'))
            .expect("one record");
        let fields: Vec<&str> = line.split(':').collect();
        assert_eq!(fields.len(), 9, "{line:?}");
        assert_eq!(fields.first(), Some(&"dave"));
        // The entry is the `crypt(3)` one the database holds, verbatim: the
        // whole reason this crate stores a full setting is that nothing has to
        // take it apart.
        let stored = db
            .find("dave")
            .and_then(|r| r.get(field::PASSWORD_HASH))
            .expect("hash");
        assert_eq!(fields.get(1), Some(&stored.as_str()));
        // Field 3 is stamped, because a password was set. The remaining six
        // are empty: nothing has set a policy for this account, and an empty
        // field is this format's way of saying so.
        assert_eq!(fields.get(2).map(|f| f.parse().ok()), Some(today()));
        assert!(
            fields
                .get(3..)
                .is_some_and(|rest| rest.iter().all(|f| f.is_empty())),
            "{line:?}"
        );
    }

    #[test]
    fn setting_a_password_records_the_day_it_was_set() {
        // Without this, `max_days` has nothing to measure from, and an account
        // whose password was set by a writer that forgot the stamp is exempt
        // from expiry for good rather than merely late.
        let db = one_account();
        assert_eq!(
            db.find("dave").map(|r| r.aging().changed),
            Some(today()),
            "a set password should stamp its own date"
        );
    }

    #[test]
    fn an_aging_policy_reaches_the_generated_shadow_in_the_order_the_format_defines() {
        let mut db = one_account();
        db.find_mut("dave").expect("record").set_aging(&Aging {
            changed: Some(19_000),
            min_days: Some(1),
            max_days: Some(90),
            warn_days: Some(7),
            inactive_days: Some(14),
            expires: Some(20_000),
        });
        let text = db
            .to_shadow_text(std::path::Path::new(DEFAULT_PATH))
            .expect("generate");
        // The order is the whole point: a policy written in the wrong columns
        // is not a wrong policy, it is a different one -- 90 in the `warn`
        // column warns for longer than the password lasts.
        assert!(text.contains(":19000:1:90:7:14:20000:"), "{text:?}");
    }

    #[test]
    fn an_unset_aging_field_is_generated_empty_rather_than_zero() {
        // Zero and empty are different answers in this format: an empty `max`
        // is "this password does not expire", a `0` is "it expired the day it
        // was set". A generator that filled unset fields with zeroes would
        // expire every account on the machine at once.
        let mut db = one_account();
        let record = db.find_mut("dave").expect("record");
        record.set_aging(&Aging {
            max_days: Some(90),
            ..Aging::default()
        });
        assert_eq!(record.aging().min_days, None);
        let text = db
            .to_shadow_text(std::path::Path::new(DEFAULT_PATH))
            .expect("generate");
        assert!(text.contains("::90::::"), "{text:?}");
    }

    #[test]
    fn clearing_one_aging_field_leaves_the_rest_standing() {
        let mut db = one_account();
        let record = db.find_mut("dave").expect("record");
        record.set_aging(&Aging {
            min_days: Some(1),
            max_days: Some(90),
            ..Aging::default()
        });
        // The read-modify-write a caller like `passwd -x` performs. Reading
        // the policy whole is what stops it dropping the five it did not name.
        let mut aging = record.aging();
        aging.max_days = None;
        record.set_aging(&aging);
        assert_eq!(record.aging().min_days, Some(1));
        assert_eq!(record.aging().max_days, None);
        // ...and the field is gone from the file, not left holding its old
        // value under a key nothing writes any more.
        assert!(!db.to_text().contains(field::PASSWORD_MAX_DAYS));
    }

    #[test]
    fn an_aging_field_that_is_not_a_number_reads_as_no_policy() {
        // A hand-edited typo must not make every later save of the whole
        // database fail; absent is where the field was before anyone set it.
        let mut db = one_account();
        db.find_mut("dave")
            .expect("record")
            .set(field::PASSWORD_MAX_DAYS, "soon");
        assert_eq!(db.find("dave").map(|r| r.aging().max_days), Some(None));
        db.to_shadow_text(std::path::Path::new(DEFAULT_PATH))
            .expect("generate");
    }

    #[test]
    fn a_lock_recorded_only_in_the_database_reaches_the_generated_shadow() {
        let mut db = one_account();
        let record = db.find_mut("dave").expect("record");
        record.set_locked(true);
        let stored = record.get(field::PASSWORD_HASH).expect("hash");
        let text = db
            .to_shadow_text(std::path::Path::new(DEFAULT_PATH))
            .expect("generate");

        // `locked: true` carries no `!` in the YAML, so a generator that
        // copied the entry across would produce an unlocked shadow line and
        // the account would still be usable from every flat-file reader.
        assert!(text.contains(&format!("dave:!{stored}:")), "{text:?}");
    }

    #[test]
    fn a_star_entry_is_not_normalised_into_a_bang() {
        // `*` is "never had a password" and `!` is "an administrator locked
        // this"; both deny login, and only one of them can be lifted by
        // removing a character. A generator that treated them as synonyms
        // would erase which of the two an account is.
        let mut db = one_account();
        db.find_mut("dave")
            .expect("record")
            .set(field::PASSWORD_HASH, "*");
        let text = db
            .to_shadow_text(std::path::Path::new(DEFAULT_PATH))
            .expect("generate");
        assert!(text.contains("dave:*:"), "{text:?}");
    }

    #[test]
    fn an_account_with_no_password_generates_an_empty_shadow_entry() {
        let mut db = one_account();
        db.find_mut("dave")
            .expect("record")
            .set(field::PASSWORD_HASH, "");
        let text = db
            .to_shadow_text(std::path::Path::new(DEFAULT_PATH))
            .expect("generate");
        let line = text
            .lines()
            .find(|l| !l.starts_with('#'))
            .expect("one record");
        let fields: Vec<&str> = line.split(':').collect();
        assert_eq!(fields.first(), Some(&"dave"));
        // Empty, not `*` or `!`: this account logs in without being asked for
        // anything, which is a state the format has a spelling for and which
        // is not the same as either kind of denial.
        assert_eq!(fields.get(1), Some(&""));
    }

    #[test]
    fn a_pre_crypt_entry_becomes_a_star_rather_than_a_des_setting() {
        // The legacy entries (§329) are bare hex with no `$id$`. Written out
        // verbatim, `crypt` reads such a string as a DES *setting* and
        // compares candidates against a hash of their first eight characters
        // -- so "no password can be checked against this" would silently
        // become "some password can".
        let db = UserDb::parse(LOGIN_MANAGER_DIALECT);
        assert_eq!(db.find("root").map(Record::has_legacy_password), Some(true));
        let text = db
            .to_shadow_text(std::path::Path::new(DEFAULT_PATH))
            .expect("generate");
        assert!(text.contains("root:*:"), "{text:?}");
        assert!(!text.contains("deadbeef"), "{text:?}");
    }

    #[test]
    fn a_field_holding_a_colon_fails_the_generation_rather_than_shifting_the_line() {
        for (field, expected) in [
            (field::DISPLAY_NAME, field::DISPLAY_NAME),
            (field::HOME, field::HOME),
            (field::SHELL, field::SHELL),
        ] {
            let mut db = one_account();
            db.find_mut("dave").expect("record").set(field, "a:/bin/sh");
            assert_eq!(
                db.to_passwd_text(std::path::Path::new(DEFAULT_PATH)),
                Err(GenerateError::Unwritable {
                    username: "dave".to_string(),
                    field: expected,
                })
            );
        }
    }

    #[test]
    fn a_newline_in_a_field_cannot_forge_a_second_account() {
        let mut db = one_account();
        db.find_mut("dave")
            .expect("record")
            .set(field::DISPLAY_NAME, "Dave\nroot2:x:0:0::/root:/bin/sh");
        assert!(matches!(
            db.to_passwd_text(std::path::Path::new(DEFAULT_PATH)),
            Err(GenerateError::Unwritable { .. })
        ));
    }

    #[test]
    fn an_unwritable_username_is_reported_by_position_and_not_by_its_bytes() {
        // A name that cannot go into `/etc/passwd` holds a control character,
        // and a control character in an error message is an escape sequence
        // aimed at whoever reads it.
        let mut db = one_account();
        db.find_mut("dave")
            .expect("record")
            .set(field::USERNAME, "da\u{1b}[2Jve");
        assert_eq!(
            db.to_passwd_text(std::path::Path::new(DEFAULT_PATH)),
            Err(GenerateError::UnwritableName { index: 1 })
        );
        assert!(!format!("{}", GenerateError::UnwritableName { index: 1 }).contains('\u{1b}'));
    }

    #[test]
    fn a_record_with_no_name_or_no_uid_fails_the_generation() {
        let mut nameless = UserDb::new();
        let mut record = Record::new();
        record.set_uid(7);
        nameless.push(record);
        assert_eq!(
            nameless.to_passwd_text(std::path::Path::new(DEFAULT_PATH)),
            Err(GenerateError::Nameless { index: 1 })
        );

        let mut uidless = UserDb::new();
        let mut record = Record::new();
        record.set(field::USERNAME, "dave");
        uidless.push(record);
        assert_eq!(
            uidless.to_passwd_text(std::path::Path::new(DEFAULT_PATH)),
            Err(GenerateError::NoUid {
                username: "dave".to_string(),
            })
        );
    }

    #[test]
    fn a_save_writes_all_three_files_and_they_agree() {
        let scratch = ScratchDir::new("userdb-generate-test");
        let path = scratch.path("users.yaml");
        one_account().save(&path).expect("save");

        let reloaded = UserDb::load(&path).expect("load");
        let passwd = std::fs::read(scratch.path(PASSWD_NAME)).expect("generated passwd");
        let shadow = std::fs::read_to_string(scratch.path(SHADOW_NAME)).expect("generated shadow");

        let user = only_passwd_record(&String::from_utf8(passwd).expect("utf-8"));
        assert_eq!(
            user.uid,
            reloaded.find("dave").and_then(Record::uid).unwrap_or(0)
        );
        assert_eq!(user.dir, b"/home/dave");
        let stored = reloaded
            .find("dave")
            .and_then(|r| r.get(field::PASSWORD_HASH))
            .expect("hash");
        assert!(shadow.contains(&format!("dave:{stored}:")), "{shadow:?}");
    }

    #[test]
    fn a_save_that_cannot_generate_leaves_every_file_untouched() {
        let scratch = ScratchDir::new("userdb-generate-refuse-test");
        let path = scratch.path("users.yaml");
        one_account().save(&path).expect("first save");
        let before = std::fs::read_to_string(&path).expect("read back");

        let mut db = UserDb::load(&path).expect("load");
        db.find_mut("dave")
            .expect("record")
            .set(field::SHELL, "/bin/sh:extra");
        let err = db.save(&path).expect_err("must refuse");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);

        // The database still says what it said: a save that cannot produce the
        // flat files must not move the truth ahead of them.
        assert_eq!(std::fs::read_to_string(&path).expect("read back"), before);
        let mut temp = path.as_os_str().to_os_string();
        temp.push(".tmp");
        assert!(!std::path::Path::new(&temp).exists());
    }

    #[test]
    fn a_removed_account_disappears_from_the_generated_files_too() {
        // The failure §353 exists to end is an account present in one store
        // and absent from the other, and deletion is the direction that leaves
        // a usable login behind rather than merely a missing one.
        let scratch = ScratchDir::new("userdb-generate-remove-test");
        let path = scratch.path("users.yaml");
        one_account().save(&path).expect("save");

        let mut db = UserDb::load(&path).expect("load");
        assert!(db.remove("dave"));
        db.save(&path).expect("save again");

        let passwd = std::fs::read(scratch.path(PASSWD_NAME)).expect("generated passwd");
        assert!(pwdb::users(&passwd).is_empty());
        let shadow = std::fs::read_to_string(scratch.path(SHADOW_NAME)).expect("generated shadow");
        assert!(!shadow.contains("dave"), "{shadow:?}");
    }

    #[test]
    fn records_can_be_added_and_removed() {
        let mut db = UserDb::new();
        let mut record = Record::new();
        record.set_uid(1001);
        record.set(field::USERNAME, "carol");
        record.set_groups(&["users".to_string()]);
        db.push(record);

        assert_eq!(db.find("carol").unwrap().uid(), Some(1001));
        assert_eq!(
            db.find_uid(1001).unwrap().username().as_deref(),
            Some("carol")
        );

        let text = db.to_text();
        assert!(text.contains("  - uid: 1001"), "{text}");
        assert_eq!(
            UserDb::parse(&text).find("carol").unwrap().uid(),
            Some(1001)
        );

        assert!(db.remove("carol"));
        assert!(!db.remove("carol"));
        assert!(db.find("carol").is_none());
    }

    #[test]
    fn a_file_of_nonsense_parses_and_re_serialises_to_itself() {
        let nonsense = "not a database\n\tjust: some words\n  - and: a dash\n";
        assert_eq!(UserDb::parse(nonsense).to_text(), nonsense);
    }

    #[test]
    fn an_empty_file_is_an_empty_database() {
        let db = UserDb::parse("");
        assert!(db.records().is_empty());
        assert_eq!(db.to_text(), "");
    }
}
