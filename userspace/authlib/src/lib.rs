//! `authlib` — the one place SlateOS answers "is this the user's password?"
//!
//! # Why this crate exists
//!
//! Three programs sharing `/etc/shadow` each implemented the hash format
//! separately and disagreed, so a password set with `passwd` could not be used
//! to log in (`design-decisions.md` §329). Five programs sharing
//! `/etc/users.yaml` each implemented the parser separately (§330). The fix
//! both times was the same: one implementation, and callers that cannot have
//! their own.
//!
//! This crate is that fix applied one level up. `posix::crypt` owns the hash
//! and `userdb` owns the YAML; what was still spread around — *which* stored
//! entry answers for a username, what a non-verifiable entry means, and
//! whether the caller is even allowed to ask right now — lives here.
//!
//! # What it is for
//!
//! Every "is this the password?" question on the system: the text-console
//! `login`, `su`/`sudo`, and — the case that prompted it — a desktop lock
//! screen, which must be able to ask without being able to *read* the answer
//! (`requests/c-b-the-lock-screen-has-no-way-to-check-a-real-password.md`).
//! An unprivileged GUI process cannot open `/etc/shadow` and must not be handed
//! a password hash, so it asks a privileged verifier instead. This is that
//! verifier's implementation; the daemon that exposes it over IPC is
//! `userspace/logind`.
//!
//! # The two properties that are part of the interface
//!
//! An oracle that answers "is this the password?" leaks two things unless it is
//! built not to, so both are here rather than left to callers:
//!
//! - **A wrong answer costs the same as a right one, and an account that does
//!   not exist costs the same as one that does.** Every path that does not run
//!   a real verification runs a throwaway one first ([`burn`]), so the call
//!   cannot be timed to enumerate accounts or to spot a locked one.
//! - **Failures are counted per user and answered with a growing delay.**
//!   [`Authenticator`] refuses outright once a user is over budget. A refused
//!   attempt does not extend the window — otherwise an attacker who keeps
//!   calling could keep the real user locked out indefinitely.
//!
//! # What is deliberately *not* decided here
//!
//! [`Outcome::NoPassword`] — an account with no password set — is reported,
//! not resolved. Traditional Unix lets an empty password log such an account
//! in at a console, and `login` still does; a lock screen must not, because
//! "press Enter to unlock" is not a screen lock. The two callers want opposite
//! answers from the same fact, so the fact is what this crate returns.
//! [`Authenticator`] takes the strict side, since it is the *desktop*-facing
//! path.

#![deny(clippy::all, clippy::pedantic)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The native user database (`design.txt`'s YAML rule).
pub const DEFAULT_USERS_YAML: &str = "/etc/users.yaml";

/// The `shadow(5)` password file.
pub const DEFAULT_SHADOW: &str = "/etc/shadow";

/// Failures a user gets before the delay starts.
///
/// Three, because a person mistypes a password two or three times without
/// being an attack, and an attack is not slowed meaningfully by the first
/// three guesses either way.
pub const FREE_ATTEMPTS: u32 = 3;

/// The longest a user is ever refused for.
///
/// Capped rather than unbounded because the tally is keyed by *username*, so
/// anyone who can reach the verifier can run the counter up for someone else.
/// An uncapped delay would turn that into a permanent denial of service against
/// a real account; five minutes makes online guessing hopeless without doing
/// so. An account that must be barred outright is locked (`!`), which is a
/// deliberate administrative act and not a side effect of someone guessing.
pub const MAX_DELAY_SECS: u64 = 300;

/// The salt of the throwaway hash computed on paths that verify nothing.
///
/// Its value is irrelevant — nothing is ever compared against the result. What
/// matters is that it is a *valid* setting for the same method as a real entry,
/// so the work it costs matches the work a real verification costs.
const DUMMY_SALT: &[u8] = b"slateosnobody";

/// The method the throwaway hash uses. The one real entries use, for the same
/// reason: a cheaper method would make the fake path measurably cheaper.
const DUMMY_METHOD: posix::crypt::Method = posix::crypt::Method::Sha512;

// ---------------------------------------------------------------------------
// Outcome
// ---------------------------------------------------------------------------

/// The answer to "is this the password for this user?"
///
/// Six cases rather than a `bool`, because a caller must treat four of them
/// the same way toward the person typing — say one thing, reveal nothing — and
/// differently toward the administrator. An entry that nothing can verify is a
/// broken system, not a mistyped password, and saying so is the only way anyone
/// finds out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The password reproduces the stored entry.
    Accepted,
    /// The entry is one this system can recompute, and the password does not
    /// match it. Also the answer for a user who does not exist — deliberately
    /// indistinguishable, and made so in cost as well as in wording.
    Rejected,
    /// The account is disabled: `!`, `!!`, `*`, a `!`-prefixed hash, or
    /// `locked: true` in the native database. No password opens it.
    Locked,
    /// The account has no password set. Nothing was verified — see the crate
    /// docs for why this is reported rather than resolved.
    NoPassword,
    /// The stored entry is in no format this system can recompute, so no
    /// password can ever match it. An administrator must set a new one.
    Unusable,
    /// Too many recent failures for this user. Nothing was verified, and this
    /// attempt was *not* counted against them.
    RateLimited {
        /// Whole seconds until the next attempt will be looked at.
        retry_after_secs: u64,
    },
}

impl Outcome {
    /// Whether the caller should let the user in.
    ///
    /// The only variant that is a yes, stated as a method so that a caller
    /// cannot write `!= Rejected` and accidentally admit `Locked`.
    #[must_use]
    pub const fn is_accepted(self) -> bool {
        matches!(self, Self::Accepted)
    }

    /// What to show the person who typed the password.
    ///
    /// Four of the six share one wording on purpose: which of them it was is
    /// exactly the thing an attacker wants to learn, and the person typing has
    /// no use for the distinction — every one of them means "this did not
    /// work, and nothing you type next will either, until something changes".
    #[must_use]
    pub const fn user_message(self) -> &'static str {
        match self {
            Self::Accepted => "",
            Self::RateLimited { .. } => "Too many failed attempts. Try again shortly.",
            _ => "Authentication failure",
        }
    }

    /// Whether this outcome is a broken system rather than a wrong password.
    ///
    /// True only for [`Outcome::Unusable`]. A caller that has somewhere to log
    /// or someone to notify should use it; the *user*-facing message must not
    /// change, which is why this is separate from [`Outcome::user_message`].
    #[must_use]
    pub const fn needs_administrator(self) -> bool {
        matches!(self, Self::Unusable)
    }
}

// ---------------------------------------------------------------------------
// The stored-entry policy
// ---------------------------------------------------------------------------

/// Check `password` against one stored password entry.
///
/// The entry is handed to `crypt` as its *setting* and recomputed, which is why
/// nothing here parses a salt or slices a hash: a stored entry is a valid
/// setting, so a correct password reproduces it byte for byte. Every one of the
/// three bugs §329 describes was in code that took the entry apart by hand.
///
/// This is the whole policy, and it is pure — no files, no clock, no counters.
/// [`Authenticator`] is this function plus a store to read the entry from and a
/// rate limit on asking.
#[must_use]
pub fn check_stored(password: &[u8], stored: &[u8]) -> Outcome {
    // A leading `!` or `*` marks the account disabled. `!` is conventionally
    // *prefixed* to an otherwise-valid hash so the password survives an
    // unlock, so this is a prefix test and not an equality test: `!$6$…` must
    // not fall through and be verified as if the `!` were part of the salt.
    if stored.first().is_some_and(|b| *b == b'!' || *b == b'*') {
        burn(password);
        return Outcome::Locked;
    }

    if stored.is_empty() {
        burn(password);
        return Outcome::NoPassword;
    }

    // Asked *before* verifying, so that an entry which can never verify is
    // reported as broken rather than counted as a wrong password. There is no
    // cleartext fallback: an entry that is not a hash is not a password.
    if posix::crypt::stored_method(stored).is_none() {
        burn(password);
        return Outcome::Unusable;
    }

    if posix::crypt::verify(password, stored) {
        Outcome::Accepted
    } else {
        Outcome::Rejected
    }
}

/// Spend what a real verification would have spent, and throw the result away.
///
/// Called on every path that answers without verifying anything. Without it,
/// "no such user" returns in microseconds while a real user's wrong password
/// takes milliseconds, and the difference is an account-enumeration oracle that
/// works over a network.
pub fn burn(password: &[u8]) {
    let mut setting_buf = posix::crypt::buf();
    let Some(setting) = posix::crypt::setting_into(DUMMY_METHOD, DUMMY_SALT, &mut setting_buf)
    else {
        // Unreachable for a constant salt that is valid crypt-base-64, and not
        // worth a panic if the constant is ever edited badly: the cost is lost,
        // the answer is not affected.
        return;
    };
    let setting = setting.to_string();
    let mut hash_buf = posix::crypt::buf();
    let hashed = posix::crypt::hash_into(password, setting.as_bytes(), &mut hash_buf);
    // Observed so the optimiser cannot delete the work that is the point.
    let _ = std::hint::black_box(hashed.is_some());
}

// ---------------------------------------------------------------------------
// shadow(5)
// ---------------------------------------------------------------------------

/// Reading `/etc/shadow`.
///
/// Here rather than in `login` because `login` was not the only reader — it was
/// merely the only one that parsed it correctly.
pub mod shadow {
    use std::path::Path;

    /// One `shadow(5)` line.
    ///
    /// The aging fields are `i64` with `-1` for "unset", which is what the file
    /// means by an empty field.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Entry {
        /// Account name.
        pub username: String,
        /// The password field: a `crypt(3)` entry, a lock marker, or empty.
        pub password_hash: String,
        /// Days since the epoch when the password was last changed.
        pub last_changed: i64,
        /// Days that must pass before it may be changed again.
        pub min_days: i64,
        /// Days after which it must be changed.
        pub max_days: i64,
        /// Days of warning before that.
        pub warn_days: i64,
        /// Days after expiry before the account is disabled.
        pub inactive_days: i64,
        /// Days since the epoch when the account expires.
        pub expire_date: i64,
    }

    fn field(fields: &[&str], n: usize) -> i64 {
        fields.get(n).and_then(|f| f.parse().ok()).unwrap_or(-1)
    }

    /// Parse one line.
    ///
    /// Two fields are enough — a name and a password — and the rest default to
    /// "unset". The parser this replaces required all nine, which meant a
    /// hand-written `alice:$6$…:` line was not merely un-aged but *invisible*:
    /// `login` read it as "no such user" and refused a correct password. Aging
    /// is a policy on top of an account, so a missing policy is not a missing
    /// account.
    #[must_use]
    pub fn parse_line(line: &str) -> Option<Entry> {
        let fields: Vec<&str> = line.split(':').collect();
        let username = (*fields.first()?).to_string();
        if username.is_empty() {
            return None;
        }
        let password_hash = (*fields.get(1)?).to_string();
        Some(Entry {
            username,
            password_hash,
            last_changed: field(&fields, 2),
            min_days: field(&fields, 3),
            max_days: field(&fields, 4),
            warn_days: field(&fields, 5),
            inactive_days: field(&fields, 6),
            expire_date: field(&fields, 7),
        })
    }

    /// Find `username` in the text of a shadow file.
    #[must_use]
    pub fn lookup_in(text: &str, username: &str) -> Option<Entry> {
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(entry) = parse_line(line)
                && entry.username == username
            {
                return Some(entry);
            }
        }
        None
    }

    /// Find `username` in the shadow file at `path`.
    ///
    /// An unreadable file is indistinguishable from an absent user, on purpose:
    /// the caller must not treat "I could not open `/etc/shadow`" as a reason to
    /// admit anyone.
    #[must_use]
    pub fn lookup(path: &Path, username: &str) -> Option<Entry> {
        let text = std::fs::read_to_string(path).ok()?;
        lookup_in(&text, username)
    }
}

// ---------------------------------------------------------------------------
// The verifier
// ---------------------------------------------------------------------------

/// Which store answered for a user, and with what.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Resolved {
    /// A stored password entry, to be handed to [`check_stored`].
    Entry(String),
    /// The account is disabled by something other than its password field —
    /// `locked: true` in the native database, which leaves the hash intact so
    /// that unlocking restores the old password.
    Locked,
    /// No store has this user.
    Unknown,
}

/// One user's recent failures.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Tally {
    failures: u32,
    last_failure_secs: u64,
}

/// How long a user with `failures` failures is refused for.
///
/// Doubling from one second once the free attempts are spent, capped at
/// [`MAX_DELAY_SECS`]. Doubling rather than a fixed delay because the cost to a
/// person who mistyped once is nothing while the cost to a program guessing is
/// the whole point; capped for the reason [`MAX_DELAY_SECS`] gives.
#[must_use]
fn delay_for(failures: u32) -> u64 {
    let over = failures.saturating_sub(FREE_ATTEMPTS);
    if over == 0 {
        return 0;
    }
    let shift = over.saturating_sub(1).min(u32::BITS.saturating_sub(1));
    let delay = 1_u64.checked_shl(shift).unwrap_or(MAX_DELAY_SECS);
    delay.min(MAX_DELAY_SECS)
}

/// Seconds since the Unix epoch, or 0 if the clock cannot be read.
///
/// A stopped clock makes the rate limit useless but never makes it admit
/// anyone, which is the right way round for a failure nobody can do anything
/// about at the point it happens.
#[must_use]
fn wall_clock_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// A privileged password verifier.
///
/// Holds the failure tallies, so it must outlive a single question — one per
/// daemon, not one per request. It reads the store on *every* call rather than
/// caching it, so that a password change or an account lock takes effect at the
/// next attempt rather than at the next restart.
///
/// # Which store answers
///
/// `/etc/users.yaml` first, `/etc/shadow` second, and only for a user the first
/// does not have. **B-Q4 has not been answered** — the system still has two
/// account lists and nothing copies between them — so this order is the one
/// place that guess lives, rather than a guess each caller makes. Whichever way
/// B-Q4 lands, one of the two branches becomes dead code here and no caller
/// changes.
#[derive(Debug, Clone)]
pub struct Authenticator {
    users_yaml: PathBuf,
    shadow: PathBuf,
    now: fn() -> u64,
    tally: BTreeMap<String, Tally>,
}

impl Default for Authenticator {
    fn default() -> Self {
        Self::new()
    }
}

impl Authenticator {
    /// A verifier over the system's real stores.
    #[must_use]
    pub fn new() -> Self {
        Self::with_stores(
            Path::new(DEFAULT_USERS_YAML),
            Path::new(DEFAULT_SHADOW),
        )
    }

    /// A verifier over stores at given paths — for tests, and for a chroot.
    #[must_use]
    pub fn with_stores(users_yaml: &Path, shadow: &Path) -> Self {
        Self {
            users_yaml: users_yaml.to_path_buf(),
            shadow: shadow.to_path_buf(),
            now: wall_clock_secs,
            tally: BTreeMap::new(),
        }
    }

    /// Replace the clock the rate limit reads.
    ///
    /// A rate limit tested against the real clock is a test that sleeps, and a
    /// test that sleeps is a test that flakes on a loaded machine.
    #[must_use]
    pub fn with_clock(mut self, now: fn() -> u64) -> Self {
        self.now = now;
        self
    }

    /// Is `password` the password for `username`?
    ///
    /// See the crate docs for the two properties this call guarantees, and
    /// [`Outcome`] for what the answer distinguishes. `password` is bytes: a
    /// typed password is whatever the user typed, and forcing it through UTF-8
    /// would change some of them.
    pub fn authenticate(&mut self, username: &str, password: &[u8]) -> Outcome {
        let now = (self.now)();

        if let Some(tally) = self.tally.get(username) {
            let ready = tally
                .last_failure_secs
                .saturating_add(delay_for(tally.failures));
            if now < ready {
                // Not counted: an attacker who keeps calling must not be able
                // to hold a real user out by refreshing their own refusal.
                return Outcome::RateLimited {
                    retry_after_secs: ready.saturating_sub(now),
                };
            }
        }

        let outcome = match self.resolve(username) {
            Resolved::Entry(stored) => check_stored(password, stored.as_bytes()),
            Resolved::Locked => {
                burn(password);
                Outcome::Locked
            }
            Resolved::Unknown => {
                burn(password);
                Outcome::Rejected
            }
        };

        if outcome.is_accepted() {
            self.tally.remove(username);
        } else {
            let tally = self.tally.entry(username.to_string()).or_default();
            tally.failures = tally.failures.saturating_add(1);
            tally.last_failure_secs = now;
        }
        outcome
    }

    /// Forget `username`'s failures — the administrative reset behind
    /// `faillock --reset`.
    pub fn reset(&mut self, username: &str) {
        self.tally.remove(username);
    }

    /// How many consecutive failures are recorded for `username`.
    #[must_use]
    pub fn failures(&self, username: &str) -> u32 {
        self.tally.get(username).map_or(0, |t| t.failures)
    }

    /// Which stored entry answers for `username`.
    fn resolve(&self, username: &str) -> Resolved {
        if let Ok(db) = userdb::UserDb::load(&self.users_yaml)
            && let Some(record) = db.find(username)
        {
            if record.is_locked() {
                return Resolved::Locked;
            }
            return Resolved::Entry(
                record
                    .get(userdb::field::PASSWORD_HASH)
                    .unwrap_or_default(),
            );
        }
        match shadow::lookup(&self.shadow, username) {
            Some(entry) => Resolved::Entry(entry.password_hash),
            None => Resolved::Unknown,
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::{
        Authenticator, FREE_ATTEMPTS, MAX_DELAY_SECS, Outcome, check_stored, delay_for, shadow,
    };
    use std::path::PathBuf;

    /// A `$6$` entry for the password `correct horse`, computed here rather
    /// than pasted, so the test cannot drift from the hasher.
    fn entry_for(password: &str) -> String {
        let mut setting_buf = posix::crypt::buf();
        let setting =
            posix::crypt::setting_into(posix::crypt::Method::Sha512, b"testsalt", &mut setting_buf)
                .expect("setting")
                .to_string();
        let mut hash_buf = posix::crypt::buf();
        posix::crypt::hash_into(password.as_bytes(), setting.as_bytes(), &mut hash_buf)
            .expect("hash")
            .to_string()
    }

    fn tmp(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!("authlib_{}_{nanos}_{name}", std::process::id()))
    }

    // ---- check_stored ----

    #[test]
    fn a_correct_password_reproduces_the_entry() {
        let stored = entry_for("correct horse");
        assert_eq!(
            check_stored(b"correct horse", stored.as_bytes()),
            Outcome::Accepted
        );
        assert_eq!(
            check_stored(b"correct hors", stored.as_bytes()),
            Outcome::Rejected
        );
        assert_eq!(check_stored(b"", stored.as_bytes()), Outcome::Rejected);
    }

    #[test]
    fn a_lock_marker_admits_nothing_and_a_locked_hash_is_not_salt() {
        for marker in ["!", "!!", "*"] {
            assert_eq!(
                check_stored(b"anything", marker.as_bytes()),
                Outcome::Locked
            );
        }
        // The prefixed form: the password is still stored, and still refused.
        let locked = format!("!{}", entry_for("correct horse"));
        assert_eq!(
            check_stored(b"correct horse", locked.as_bytes()),
            Outcome::Locked,
            "a `!`-prefixed hash must not fall through and verify"
        );
    }

    #[test]
    fn an_entry_nothing_can_recompute_is_reported_as_broken() {
        // `x` is a passwd(5) marker with no meaning in shadow(5); 64 hex
        // digits under a `$5$` label is what this tree wrote before §329.
        for junk in ["x", "$5$salt$0123456789abcdef", "secret"] {
            let outcome = check_stored(b"secret", junk.as_bytes());
            assert_eq!(outcome, Outcome::Unusable, "{junk}");
            assert!(outcome.needs_administrator());
            assert_eq!(outcome.user_message(), "Authentication failure");
        }
    }

    #[test]
    fn an_empty_entry_verifies_nothing_and_says_so() {
        // Not `Accepted`, even for an empty password: whether a passwordless
        // account may be entered is the caller's policy, not this function's.
        assert_eq!(check_stored(b"", b""), Outcome::NoPassword);
        assert_eq!(check_stored(b"anything", b""), Outcome::NoPassword);
    }

    #[test]
    fn only_accepted_is_a_yes() {
        assert!(Outcome::Accepted.is_accepted());
        for other in [
            Outcome::Rejected,
            Outcome::Locked,
            Outcome::NoPassword,
            Outcome::Unusable,
            Outcome::RateLimited {
                retry_after_secs: 1,
            },
        ] {
            assert!(!other.is_accepted(), "{other:?}");
        }
        // Four of the five share one wording; the rate limit is the exception
        // because "try again" is advice the user can act on.
        assert_eq!(Outcome::Rejected.user_message(), "Authentication failure");
        assert_eq!(Outcome::Locked.user_message(), "Authentication failure");
        assert_eq!(Outcome::NoPassword.user_message(), "Authentication failure");
        assert_eq!(Outcome::Unusable.user_message(), "Authentication failure");
        assert_ne!(
            Outcome::RateLimited {
                retry_after_secs: 1
            }
            .user_message(),
            "Authentication failure"
        );
    }

    // ---- shadow ----

    #[test]
    fn a_shadow_line_needs_a_name_and_a_password_and_nothing_else() {
        let full = shadow::parse_line("alice:$6$a$b:19000:0:99999:7:::").expect("full line");
        assert_eq!(full.username, "alice");
        assert_eq!(full.password_hash, "$6$a$b");
        assert_eq!(full.last_changed, 19000);
        assert_eq!(full.max_days, 99_999);
        assert_eq!(full.inactive_days, -1, "an empty aging field is unset");

        // The case the old nine-field parser dropped on the floor.
        let short = shadow::parse_line("bob:$6$a$b").expect("two fields is an account");
        assert_eq!(short.username, "bob");
        assert_eq!(short.password_hash, "$6$a$b");
        assert_eq!(short.last_changed, -1);

        assert!(shadow::parse_line("nopassword").is_none());
        assert!(shadow::parse_line(":$6$a$b").is_none(), "no name, no entry");
    }

    #[test]
    fn a_lookup_skips_comments_and_blank_lines() {
        let text = "# comment\n\nalice:$6$a$b:1:2:3:4:5:6:\nbob:!:::::::\n";
        assert_eq!(
            shadow::lookup_in(text, "alice").expect("alice").password_hash,
            "$6$a$b"
        );
        assert_eq!(shadow::lookup_in(text, "bob").expect("bob").password_hash, "!");
        assert!(shadow::lookup_in(text, "carol").is_none());
        assert!(
            shadow::lookup_in(text, "# comment").is_none(),
            "a comment is not an account named `#`"
        );
    }

    // ---- Authenticator ----

    fn authenticator_over_shadow(text: &str) -> (Authenticator, PathBuf) {
        let path = tmp("shadow");
        std::fs::write(&path, text).expect("write shadow");
        let auth = Authenticator::with_stores(&tmp("absent.yaml"), &path);
        (auth, path)
    }

    #[test]
    fn the_shadow_store_answers_when_the_native_one_has_no_such_user() {
        let stored = entry_for("correct horse");
        let (mut auth, path) = authenticator_over_shadow(&format!("alice:{stored}:1:2:3:4:5:6:\n"));
        assert_eq!(auth.authenticate("alice", b"correct horse"), Outcome::Accepted);
        assert_eq!(auth.authenticate("alice", b"wrong"), Outcome::Rejected);
        assert_eq!(auth.authenticate("nobody", b"wrong"), Outcome::Rejected);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn the_native_store_wins_when_it_has_the_user() {
        let yaml_path = tmp("users.yaml");
        let mut db = userdb::UserDb::new();
        let mut record = userdb::Record::new();
        record.set("username", "alice");
        record
            .set_password_with_salt("native", "nativesalt")
            .expect("set password");
        db.push(record);
        db.save(&yaml_path).expect("save yaml");

        let shadow_path = tmp("shadow");
        std::fs::write(
            &shadow_path,
            format!("alice:{}:1:2:3:4:5:6:\n", entry_for("shadowy")),
        )
        .expect("write shadow");

        let mut auth = Authenticator::with_stores(&yaml_path, &shadow_path);
        assert_eq!(auth.authenticate("alice", b"native"), Outcome::Accepted);
        assert_eq!(
            auth.authenticate("alice", b"shadowy"),
            Outcome::Rejected,
            "the shadow entry must not be consulted for a user the native store has"
        );
        let _ = std::fs::remove_file(yaml_path);
        let _ = std::fs::remove_file(shadow_path);
    }

    #[test]
    fn a_natively_locked_account_is_locked_even_with_its_password_intact() {
        let yaml_path = tmp("locked.yaml");
        let mut db = userdb::UserDb::new();
        let mut record = userdb::Record::new();
        record.set("username", "alice");
        record
            .set_password_with_salt("correct horse", "nativesalt")
            .expect("set password");
        record.set_locked(true);
        db.push(record);
        db.save(&yaml_path).expect("save yaml");

        let mut auth = Authenticator::with_stores(&yaml_path, &tmp("absent-shadow"));
        assert_eq!(auth.authenticate("alice", b"correct horse"), Outcome::Locked);
        let _ = std::fs::remove_file(yaml_path);
    }

    #[test]
    fn an_unreadable_store_admits_nobody() {
        let mut auth = Authenticator::with_stores(&tmp("absent.yaml"), &tmp("absent-shadow"));
        assert_eq!(auth.authenticate("alice", b""), Outcome::Rejected);
        assert_eq!(auth.authenticate("root", b"toor"), Outcome::Rejected);
    }

    // ---- rate limiting ----

    #[test]
    fn the_delay_doubles_once_the_free_attempts_are_spent_and_then_stops() {
        for failures in 0..=FREE_ATTEMPTS {
            assert_eq!(delay_for(failures), 0, "{failures} failures is still free");
        }
        assert_eq!(delay_for(FREE_ATTEMPTS + 1), 1);
        assert_eq!(delay_for(FREE_ATTEMPTS + 2), 2);
        assert_eq!(delay_for(FREE_ATTEMPTS + 3), 4);
        // Doubling reaches 256 at nine failures over budget and 512 at ten,
        // so ten is the first that the cap actually bites on.
        assert_eq!(delay_for(FREE_ATTEMPTS + 9), 256, "not capped yet");
        assert_eq!(delay_for(FREE_ATTEMPTS + 10), MAX_DELAY_SECS, "capped");
        assert_eq!(delay_for(u32::MAX), MAX_DELAY_SECS, "still capped");
    }

    /// Clocks the rate-limit tests drive.
    ///
    /// `fn()` pointers cannot close over state, so each clock has to be a
    /// static — and therefore **one static per test, never one shared**. Two
    /// tests sharing a static clock are two threads writing the same cell:
    /// `cargo test` runs them concurrently, so one test's `store` lands
    /// between the other's `store` and its `authenticate`, and the second test
    /// fails with a time it never set. That is exactly how these two first
    /// failed. A lock would serialise them but would also hide the coupling;
    /// separate cells mean there is nothing to serialise.
    static FAKE_NOW_BUDGET: std::sync::atomic::AtomicU64 =
        std::sync::atomic::AtomicU64::new(1000);
    static FAKE_NOW_TWO_USERS: std::sync::atomic::AtomicU64 =
        std::sync::atomic::AtomicU64::new(5000);

    fn fake_now_budget() -> u64 {
        FAKE_NOW_BUDGET.load(std::sync::atomic::Ordering::Relaxed)
    }

    fn fake_now_two_users() -> u64 {
        FAKE_NOW_TWO_USERS.load(std::sync::atomic::Ordering::Relaxed)
    }

    #[test]
    fn a_user_over_budget_is_refused_without_being_counted_further() {
        let stored = entry_for("correct horse");
        let path = tmp("ratelimit-shadow");
        std::fs::write(&path, format!("alice:{stored}:1:2:3:4:5:6:\n")).expect("write");
        let mut auth = Authenticator::with_stores(&tmp("absent.yaml"), &path)
            .with_clock(fake_now_budget);

        FAKE_NOW_BUDGET.store(1000, std::sync::atomic::Ordering::Relaxed);
        for n in 1..=FREE_ATTEMPTS {
            assert_eq!(auth.authenticate("alice", b"wrong"), Outcome::Rejected);
            assert_eq!(auth.failures("alice"), n);
        }
        // The fourth failure starts the delay…
        assert_eq!(auth.authenticate("alice", b"wrong"), Outcome::Rejected);
        assert_eq!(auth.failures("alice"), FREE_ATTEMPTS + 1);
        // …and the *correct* password is refused inside it, which is the point.
        assert_eq!(
            auth.authenticate("alice", b"correct horse"),
            Outcome::RateLimited {
                retry_after_secs: 1
            }
        );
        assert_eq!(
            auth.failures("alice"),
            FREE_ATTEMPTS + 1,
            "a refused attempt must not extend the window, or an attacker \
             spinning on the call keeps the real user out forever"
        );

        // Past the window, the correct password works and clears the tally.
        FAKE_NOW_BUDGET.store(1002, std::sync::atomic::Ordering::Relaxed);
        assert_eq!(auth.authenticate("alice", b"correct horse"), Outcome::Accepted);
        assert_eq!(auth.failures("alice"), 0);

        // And an administrative reset does the same without a password.
        FAKE_NOW_BUDGET.store(2000, std::sync::atomic::Ordering::Relaxed);
        for _ in 0..=FREE_ATTEMPTS {
            let _ = auth.authenticate("alice", b"wrong");
        }
        assert!(auth.failures("alice") > FREE_ATTEMPTS);
        auth.reset("alice");
        assert_eq!(auth.failures("alice"), 0);
        assert_eq!(auth.authenticate("alice", b"correct horse"), Outcome::Accepted);

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn one_users_failures_do_not_delay_another() {
        let stored = entry_for("correct horse");
        let path = tmp("two-user-shadow");
        std::fs::write(
            &path,
            format!("alice:{stored}:1:2:3:4:5:6:\nbob:{stored}:1:2:3:4:5:6:\n"),
        )
        .expect("write");
        let mut auth = Authenticator::with_stores(&tmp("absent.yaml"), &path)
            .with_clock(fake_now_two_users);
        FAKE_NOW_TWO_USERS.store(5000, std::sync::atomic::Ordering::Relaxed);
        for _ in 0..=FREE_ATTEMPTS {
            let _ = auth.authenticate("alice", b"wrong");
        }
        assert!(matches!(
            auth.authenticate("alice", b"correct horse"),
            Outcome::RateLimited { .. }
        ));
        assert_eq!(auth.authenticate("bob", b"correct horse"), Outcome::Accepted);
        let _ = std::fs::remove_file(path);
    }
}
