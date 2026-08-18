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

use std::fmt::Write as _;

/// Where the database lives.
pub const DEFAULT_PATH: &str = "/etc/users.yaml";

/// The method new passwords are hashed with.
const PASSWORD_METHOD: posix::crypt::Method = posix::crypt::Method::Sha512;

/// Canonical field names, and the aliases accepted when reading.
pub mod field {
    /// Numeric user id. Also the key the record's `- ` line carries.
    pub const UID: &str = "uid";
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

    /// The salt fields the two old writers used. Read to recognise a legacy
    /// entry; never written, because a `crypt(3)` entry carries its own salt
    /// and a salt stored beside it is a second copy that can disagree.
    pub const LEGACY_SALT: [&str; 2] = ["password_salt", "salt"];
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

    /// Write the database to `path`, atomically.
    ///
    /// The text goes to a sibling temporary file which is then renamed over
    /// the target, so a crash or a full disk leaves the previous database
    /// intact. Writing in place would give a window in which `/etc/users.yaml`
    /// is truncated, and a machine that loses power in that window has no
    /// accounts and no way to log in and fix it.
    ///
    /// # Errors
    ///
    /// Any I/O error from creating, writing or renaming the temporary file.
    pub fn save(&self, path: impl AsRef<std::path::Path>) -> std::io::Result<()> {
        let path = path.as_ref();
        let mut temp = path.as_os_str().to_os_string();
        temp.push(".tmp");
        let temp = std::path::PathBuf::from(temp);
        std::fs::write(&temp, self.to_text())?;
        match std::fs::rename(&temp, path) {
            Ok(()) => Ok(()),
            Err(e) => {
                // Leaving the temporary file behind would make the next save
                // look like it had already half-succeeded.
                let _ = std::fs::remove_file(&temp);
                Err(e)
            }
        }
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
    use super::*;

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
        let dir = std::env::temp_dir().join("userdb-load-test");
        std::fs::create_dir_all(&dir).expect("temp dir");
        let missing = dir.join("does-not-exist.yaml");
        let _ = std::fs::remove_file(&missing);
        assert!(
            UserDb::load(&missing)
                .expect("missing is empty")
                .records()
                .is_empty()
        );

        // A directory stands in for "present but unreadable": every platform
        // refuses to read one as a file, and it is not a `NotFound`.
        assert!(UserDb::load(&dir).is_err());
    }

    #[test]
    fn a_save_replaces_the_file_and_leaves_no_temporary_behind() {
        let dir = std::env::temp_dir().join("userdb-save-test");
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("users.yaml");
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
        let _ = std::fs::remove_file(&path);
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
