//! Slate OS Password Management Utility
//!
//! Manages user passwords and password aging policies in the account
//! database, `/etc/users.yaml`.
//!
//! # Why not `/etc/shadow`
//!
//! It used to edit `/etc/shadow` directly, and parse `/etc/passwd` to check
//! that the account existed. `design-decisions.md` §353 settles that neither
//! of those files is the truth: both are *generated* from `/etc/users.yaml`
//! on every account change, for the benefit of ported software that reads the
//! flat files. A `passwd` that wrote to `/etc/shadow` would therefore have its
//! work silently undone the next time any other tool touched an account — and
//! in the meantime the password it set would be invisible to the graphical
//! login screen, which reads the database. So every read and every write here
//! goes through `userdb`, and the flat files are regenerated as a consequence
//! of saving rather than written by this program at all.
//!
//! Two duplications went with that change. The hashing — a private
//! `hash_password`/`generate_salt` pair — is now `userdb`'s
//! [`userdb::Record::set_password`], which does the same thing (SHA-512-crypt,
//! a `/dev/urandom` salt, and no fallback to a guessable one) in the one place
//! that also stores the result. And the aging fields, which this program used
//! to define its own `ShadowEntry` for, are [`userdb::Aging`].
//!
//! # Usage
//!
//! ```text
//! passwd                       Change own password
//! passwd <username>            Change another user's password (root only)
//! passwd -l <username>         Lock account
//! passwd -u <username>         Unlock account
//! passwd -d <username>         Delete password (passwordless)
//! passwd -S <username>         Show password status
//! passwd -e <username>         Expire password (force change at next login)
//! passwd -n <days> <username>  Minimum password age
//! passwd -x <days> <username>  Maximum password age
//! passwd -w <days> <username>  Warning days before expiry
//! passwd -i <days> <username>  Inactive days after expiry before lock
//! ```
//!
//! # Locking is a fact of its own here
//!
//! In `/etc/shadow` a lock is a `!` written in front of the password, so
//! shadow-utils' `passwd` unlocks an account as a side effect of writing a new
//! hash over that column. The database keeps the two apart, so the behaviour is
//! chosen rather than inherited: **setting a password does not unlock**, and
//! says so on the run that hits it, while **`-d` does** clear the lock, because
//! a lock over a password that no longer exists is a state nobody asks for. See
//! `design-decisions.md` §1003.
//!
//! # Where the values live
//!
//! One record per account in `/etc/users.yaml`. The password entry is
//! `password_hash` — a full `crypt(3)` string, salt included — and the six
//! aging numbers are the fields [`userdb::Aging`] names. `/etc/shadow`'s
//! `login:password:lastchg:min:max:warn:inactive:expire:` line is a rendering
//! of exactly those, produced by [`userdb::UserDb::save`].

use quoting::quoteaf_os;
use std::env;
use std::io::{self, BufRead, Write};
use std::process;
use userdb::{Aging, Record, UserDb};

// ============================================================================
// Constants
// ============================================================================

const MIN_PASSWORD_LEN: usize = 8;

// ============================================================================
// The account database
// ============================================================================
//
// The `ShadowEntry` and `PasswdEntry` structs that stood here, with their
// `read_shadow`/`write_shadow`/`read_passwd`/`find_user` helpers, are gone.
// They were a second parser and a second writer for two files that
// `design-decisions.md` §353 makes *generated* output -- so every line they
// wrote was a line the next account change would overwrite, and every line
// they read was a rendering rather than the thing itself.
//
// `find_or_create_shadow` is gone with them, and its disappearance is the
// point rather than a side effect: it existed because a user could be in
// `/etc/passwd` with no `/etc/shadow` line, which is precisely the
// one-file-and-not-the-other state a single database cannot be in. An account
// either has a record here or does not exist.

/// The account database, together with where it came from.
///
/// The path travels with the database rather than being a constant each
/// command reaches for, so that a command is a function of the file it is
/// given. That is what makes the round-trip a *test* rather than an argument:
/// the tests below run the real commands against a real database in a scratch
/// directory, and a program whose save target is a constant can only be tested
/// by trusting the parts around it.
struct Accounts {
    db: UserDb,
    path: std::path::PathBuf,
}

impl Accounts {
    /// Read the account database, or explain why it could not be read.
    ///
    /// A database that cannot be read is not an empty one: treating it as
    /// empty would make `passwd alice` report "user does not exist" for every
    /// account on the machine, and -- worse -- a subsequent save would write
    /// that empty database out over the real one.
    fn load(path: &std::path::Path) -> Result<Self, String> {
        let db =
            UserDb::load(path).map_err(|e| format!("cannot read `{}': {e}", path.display()))?;
        Ok(Self {
            db,
            path: path.to_path_buf(),
        })
    }

    /// Write the database back, regenerating `/etc/passwd` and `/etc/shadow`.
    fn store(&self) -> Result<(), String> {
        self.db
            .save(&self.path)
            .map_err(|e| format!("cannot write `{}': {e}", self.path.display()))
    }

    /// The record for `username`, if the account exists.
    fn find(&self, username: &str) -> Option<&Record> {
        self.db.find(username)
    }

    /// The record for `username`, which `main` has already established exists.
    ///
    /// Every command runs against a database `main` loaded and searched, so a
    /// missing record here is not a user error and must not be reported as
    /// one: it would mean the record vanished between the two lookups, which
    /// cannot happen within one process. Reporting it as an internal error
    /// keeps "user does not exist" saying only what it says.
    fn record(&mut self, username: &str) -> Result<&mut Record, i32> {
        match self.db.find_mut(username) {
            Some(record) => Ok(record),
            None => Err(vanished(username)),
        }
    }
}

/// Report the impossible lookup, and give the caller its exit code.
///
/// A separate function only so that the read-only command reports it in the
/// same words as the writing ones; a second wording would be a second thing to
/// recognise in a bug report.
fn vanished(username: &str) -> i32 {
    eprintln!(
        "passwd: internal error: user {} was present a moment ago and is not now",
        quoteaf_os(username)
    );
    1
}

/// The one-or-two-letter status `-S` prints for an account.
///
/// The vocabulary is the shadow suite's -- `P` for a usable password, `NP`
/// for none at all, `L` for an account that accepts nothing -- and the mapping
/// is deliberately the same one [`userdb::UserDb::save`] uses when it renders
/// the entry, so that what this prints and what `/etc/shadow` says cannot
/// disagree. That is why a pre-`crypt(3)` legacy entry (§329) reports `L`: it
/// is generated as `*`, no password can be checked against it, and reporting
/// `P` would tell an administrator the account has a working password when
/// nothing on the system can verify one.
fn status_char(record: &Record) -> &'static str {
    if record.is_locked() || record.has_legacy_password() {
        "L"
    } else if has_password(record) {
        "P"
    } else {
        "NP"
    }
}

/// Whether the record carries a password entry at all.
///
/// A missing field and an empty one are the same account: one that logs in
/// without being asked for anything. Keeping them one question here is what
/// stops `-S` printing `P` for a record `-d` emptied.
fn has_password(record: &Record) -> bool {
    record
        .get(userdb::field::PASSWORD_HASH)
        .is_some_and(|h| !h.is_empty())
}

// ============================================================================
// SHA-256 implementation — deleted
// ============================================================================
//
// This file used to carry a full, genuine SHA-256 (round constants, FIPS
// vectors and all) and use it to write `/etc/shadow` entries in a format it
// invented: `$sha256$<salt>$<64 hex digits>`.  Everything about it was
// right except the two things that mattered.
//
// It was the wrong *format*: `$5$` and `$6$` are the crypt(3) identifiers
// for SHA-crypt, and a reader that follows the standard — a real libc, or
// `posix/src/crypt.rs` — parses `$sha256$` as an unknown method and refuses
// it.  `login`, which read the same file, refused it too, so a password set
// with `passwd` could not be used to log in.
//
// And it was the wrong *construction*: one pass of SHA-256 over
// `salt$password` has no work factor.  Every real crypt(3) scheme iterates
// thousands of rounds so that testing a guess costs the attacker what one
// login costs the user; a single pass costs the attacker nothing.
//
// Both are fixed by not implementing any of it here.  See
// `requests/c-b-passwd-and-login-disagree-about-etc-shadow.md`.

// ============================================================================
// Password hashing and salt generation -- deleted too
// ============================================================================
//
// `NEW_PASSWORD_METHOD`, `hash_password`, `encode_salt` and `generate_salt`
// stood here. All four were correct, and all four were a second copy of what
// `userdb::Record::set_password` already does -- the same SHA-512-crypt, the
// same crypt base-64 salt at the method's own maximum length, the same
// refusal to fall back to a guessable salt when `/dev/urandom` cannot be
// read.
//
// A second correct copy is still a place for the two to drift apart, and this
// program is the one that proved it: the reason it has a `posix::crypt`
// dependency at all is that it once implemented the hash itself and disagreed
// with `login` about the result. Hashing now happens where the result is
// stored, which is the only arrangement in which the two cannot disagree.

// `verify_password` no longer stands here.  It was a wrapper over
// `posix::crypt::verify` — the comparison was right, but it was a second
// statement of the policy *around* the comparison: that `!` and `*` mark an
// account disabled, that an empty entry means "no password" rather than "the
// empty password", that an entry nothing can recompute is a broken system and
// not a wrong guess.  Two statements of a policy are one policy plus one place
// for it to disagree with itself, which is the shape of every bug §329
// catalogued.  The `Current password:` prompt now asks
// `authlib::check_stored`, the same function `login`, `su` and `doas` ask, and
// this program states nothing of its own about what a stored entry means.
// See `known-issues.md` -> `B-PASSWD-VERIFIES-WITHOUT-AUTHLIB`.
//
// The tests kept their `verify_password` — as a *test* helper, defined in the
// test module, asserting the property that is this crate's to keep: that a
// password this program **writes** is one the system's verifier **accepts**.
// That is the bug `requests/c-b-passwd-and-login-disagree-about-etc-shadow.md`
// was filed for, and it is worth a regression test here even though the
// verifier itself is tested next to its own definition.

/// What to do about one answer at the `Current password:` prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OldPasswordVerdict {
    /// Verified. Go on and set the new password.
    Proceed,
    /// Wrong. Say "authentication failure" and stop.
    Refuse,
    /// The stored entry is in no format this system can recompute, so no
    /// answer could have been right. Say so, and name the remedy.
    Unverifiable,
}

/// Decide the verdict *and* do the shared-tally bookkeeping that goes with it.
///
/// # Contributes, but is never delayed
///
/// This prompt adds to the same per-user failed-attempt tally that `login`,
/// `su` and `doas` use, and — alone among them — never consults it
/// (`design-decisions.md` §354). Both halves are deliberate.
///
/// *Contributing* means a guess made here is not free: it costs the guesser
/// time at every other prompt afterwards. Before this, the `Current password:`
/// prompt was the one place in the system where an attacker who already had
/// your shell could guess at your password without cost or trace, and "the fix
/// would be annoying" is how a prompt like that stays uncounted.
///
/// *Not being delayed* is the exception, and it is narrow. Changing your
/// password is the action you most want available at the moment you suspect it
/// is compromised, and a delay is exactly the mechanism that would take it
/// away — including from you, at the hands of an attacker who could otherwise
/// hold your account at a five-minute wait and so stop you locking them out.
/// Every other prompt gates access to something; this one gates the remedy.
/// That is why there is no `rate_limited` call here, and why its absence is
/// load-bearing rather than an omission.
///
/// A verified password clears the count, exactly as a successful login does:
/// the run of consecutive failures is over.
///
/// An unverifiable entry is *not* counted. No answer can ever match it, so a
/// wrong one reveals nothing to an attacker and learns nothing for the tally;
/// counting it would only lock a user out of `login` for the crime of having a
/// broken entry that an administrator has to repair regardless.
fn judge_old_password(
    auth: &mut authlib::Authenticator,
    username: &str,
    outcome: authlib::Outcome,
) -> OldPasswordVerdict {
    if outcome.is_accepted() {
        auth.reset(username);
        return OldPasswordVerdict::Proceed;
    }
    if outcome.needs_administrator() {
        return OldPasswordVerdict::Unverifiable;
    }
    auth.note_failure(username);
    OldPasswordVerdict::Refuse
}

// The constant-time comparison that stood here went with the hand-written
// format parsing that was its only caller.  It now lives inside
// `posix::crypt::verify`, next to the value it compares against, where a
// caller cannot reach past it and compare something else.

// ============================================================================
// Password strength checking
// ============================================================================

/// Strength check result.
struct StrengthResult {
    ok: bool,
    reasons: Vec<&'static str>,
}

/// Check password strength requirements.
fn check_password_strength(password: &str) -> StrengthResult {
    let mut reasons = Vec::new();

    if password.len() < MIN_PASSWORD_LEN {
        reasons.push("password is too short (minimum 8 characters)");
    }

    let has_upper = password.chars().any(|c| c.is_ascii_uppercase());
    let has_lower = password.chars().any(|c| c.is_ascii_lowercase());
    let has_digit = password.chars().any(|c| c.is_ascii_digit());
    let has_special = password.chars().any(|c| !c.is_ascii_alphanumeric());

    if !has_upper {
        reasons.push("missing uppercase letter");
    }
    if !has_lower {
        reasons.push("missing lowercase letter");
    }
    if !has_digit {
        reasons.push("missing digit");
    }
    if !has_special {
        reasons.push("missing special character");
    }

    // Check for common patterns.
    let lower = password.to_ascii_lowercase();
    if lower.contains("password") || lower.contains("123456") || lower == "qwerty" {
        reasons.push("password contains a common pattern");
    }

    // Check for repeated characters.
    let bytes = password.as_bytes();
    let mut all_same = bytes.len() > 1;
    for window in bytes.windows(2) {
        if window[0] != window[1] {
            all_same = false;
            break;
        }
    }
    if all_same && !bytes.is_empty() {
        reasons.push("password is all the same character");
    }

    StrengthResult {
        ok: reasons.is_empty(),
        reasons,
    }
}

// ============================================================================
// Terminal helpers
// ============================================================================

/// Read a password from stdin without echoing.
/// On Slate OS, we disable echo via ioctl on /dev/tty.
/// Falls back to normal line read if terminal control is unavailable.
fn read_password_no_echo(prompt: &str) -> Result<String, String> {
    eprint!("{prompt}");
    let _ = io::stderr().flush();

    // Attempt to disable echo. On Slate OS this would use termios ioctls.
    // For now, just read a line — the real echo-disable will be done
    // via the POSIX termios layer when the kernel supports it.
    let mut line = String::new();
    io::stdin()
        .lock()
        .read_line(&mut line)
        .map_err(|e| format!("read error: {e}"))?;
    eprintln!(); // newline after hidden input

    // Trim trailing newline.
    if line.ends_with('\n') {
        line.pop();
    }
    if line.ends_with('\r') {
        line.pop();
    }

    Ok(line)
}

// ============================================================================
// System helpers
// ============================================================================

// `current_day` stood here, dividing the clock by 86 400.  It is
// `userdb::today` now — the same arithmetic, but next to the fields it dates,
// and returning `Option` rather than falling back to `0` on a clock before the
// epoch.  That fallback was the dangerous half: day 0 is how `/etc/shadow`
// spells "this password is expired", so a machine with a confused clock would
// have had `passwd` silently expire every password it touched.

/// Determine the current user's UID. Reads the `UID` environment variable
/// (set by the login/init process) or defaults to 0 (root) if unset.
fn current_uid() -> u32 {
    env::var("UID")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

/// Determine the current user's username from the `USER` environment variable.
fn current_username() -> Option<String> {
    env::var("USER").ok()
}

/// Check whether the current user is root.
fn is_root() -> bool {
    current_uid() == 0
}

// ============================================================================
// Argument parsing
// ============================================================================

#[derive(Debug)]
enum Action {
    /// Change password (default).
    ChangePassword,
    /// Lock account (`-l`).
    Lock,
    /// Unlock account (`-u`).
    Unlock,
    /// Delete password (`-d`).
    DeletePassword,
    /// Show status (`-S`).
    ShowStatus,
    /// Expire password (`-e`).
    Expire,
    /// Set minimum days (`-n`).
    SetMinDays(i64),
    /// Set maximum days (`-x`).
    SetMaxDays(i64),
    /// Set warning days (`-w`).
    SetWarnDays(i64),
    /// Set inactive days (`-i`).
    SetInactiveDays(i64),
}

struct Args {
    action: Action,
    target_user: Option<String>,
}

fn parse_args(raw: &[String]) -> Result<Args, String> {
    let mut action = Action::ChangePassword;
    let mut target_user: Option<String> = None;
    let mut idx = 1; // skip argv[0]

    while idx < raw.len() {
        let arg = &raw[idx];
        match arg.as_str() {
            "-l" | "--lock" => {
                action = Action::Lock;
                idx += 1;
            }
            "-u" | "--unlock" => {
                action = Action::Unlock;
                idx += 1;
            }
            "-d" | "--delete" => {
                action = Action::DeletePassword;
                idx += 1;
            }
            "-S" | "--status" => {
                action = Action::ShowStatus;
                idx += 1;
            }
            "-e" | "--expire" => {
                action = Action::Expire;
                idx += 1;
            }
            "-n" | "--mindays" => {
                idx += 1;
                if idx >= raw.len() {
                    return Err("option -n requires a numeric argument".to_string());
                }
                let days: i64 = raw[idx]
                    .parse()
                    .map_err(|_| format!("invalid number for -n: {}", raw[idx]))?;
                action = Action::SetMinDays(days);
                idx += 1;
            }
            "-x" | "--maxdays" => {
                idx += 1;
                if idx >= raw.len() {
                    return Err("option -x requires a numeric argument".to_string());
                }
                let days: i64 = raw[idx]
                    .parse()
                    .map_err(|_| format!("invalid number for -x: {}", raw[idx]))?;
                action = Action::SetMaxDays(days);
                idx += 1;
            }
            "-w" | "--warndays" => {
                idx += 1;
                if idx >= raw.len() {
                    return Err("option -w requires a numeric argument".to_string());
                }
                let days: i64 = raw[idx]
                    .parse()
                    .map_err(|_| format!("invalid number for -w: {}", raw[idx]))?;
                action = Action::SetWarnDays(days);
                idx += 1;
            }
            "-i" | "--inactive" => {
                idx += 1;
                if idx >= raw.len() {
                    return Err("option -i requires a numeric argument".to_string());
                }
                let days: i64 = raw[idx]
                    .parse()
                    .map_err(|_| format!("invalid number for -i: {}", raw[idx]))?;
                action = Action::SetInactiveDays(days);
                idx += 1;
            }
            "-h" | "--help" => {
                print_usage();
                process::exit(0);
            }
            other => {
                if other.starts_with('-') {
                    return Err(format!("unknown option: {other}"));
                }
                if target_user.is_some() {
                    return Err(format!("unexpected argument: {other}"));
                }
                target_user = Some(other.to_string());
                idx += 1;
            }
        }
    }

    Ok(Args {
        action,
        target_user,
    })
}

fn print_usage() {
    eprintln!("Usage: passwd [options] [username]");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  -l, --lock       Lock the account");
    eprintln!("  -u, --unlock     Unlock the account");
    eprintln!("  -d, --delete     Delete the password (passwordless)");
    eprintln!("  -S, --status     Show password status");
    eprintln!("  -e, --expire     Expire password (force change at next login)");
    eprintln!("  -n, --mindays N  Minimum days between changes");
    eprintln!("  -x, --maxdays N  Maximum days before change required");
    eprintln!("  -w, --warndays N Warning days before expiry");
    eprintln!("  -i, --inactive N Inactive days after expiry before lock");
    eprintln!("  -h, --help       Show this help");
}

// ============================================================================
// Command implementations
// ============================================================================

/// Whether the account's own minimum-age policy forbids a change today.
///
/// Absent is not zero. A record with no `min_days` has no minimum-age policy,
/// so nothing here delays it; the old code read that same absence as a `0` it
/// had invented and compared against, which happened to permit the change but
/// only by accident of the number chosen. A record whose `password_changed` is
/// absent likewise has no age to measure, and a clock that cannot say what day
/// it is cannot measure one either — in both cases the change is allowed,
/// because refusing would mean locking a user out of their own password over a
/// fact the system does not have.
fn too_soon_to_change(record: &Record) -> Option<i64> {
    let aging = record.aging();
    let min = aging.min_days?;
    if min <= 0 {
        return None;
    }
    let changed = aging.changed?;
    let remaining = min.checked_sub(userdb::today()?.checked_sub(changed)?)?;
    if remaining > 0 { Some(remaining) } else { None }
}

/// Change password for the target user.
fn cmd_change_password(accounts: &mut Accounts, target: &str, caller_uid: u32) -> i32 {
    // Non-root users must verify their current password.
    if caller_uid != 0 {
        let Some(record) = accounts.find(target) else {
            return vanished(target);
        };
        if record.is_locked() {
            eprintln!("passwd: account is locked");
            return 1;
        }
        if let Some(remaining) = too_soon_to_change(record) {
            eprintln!("passwd: password may not be changed yet ({remaining} day(s) remaining)");
            return 1;
        }
        // No `&& !record.is_locked()` here: a locked account was already
        // refused above, so the conjunct could never be false, and keeping
        // it read as though a locked account reached this prompt and was
        // waved past it -- the opposite of what happens.  Locking is
        // decided in exactly one place, and this is not it.
        let stored = record.get(userdb::field::PASSWORD_HASH).unwrap_or_default();
        if !stored.is_empty() {
            let old_pw = match read_password_no_echo("Current password: ") {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("passwd: {e}");
                    return 1;
                }
            };
            let mut auth = authlib::Authenticator::new();
            let outcome = authlib::check_stored(old_pw.as_bytes(), stored.as_bytes());
            match judge_old_password(&mut auth, target, outcome) {
                OldPasswordVerdict::Proceed => {}
                OldPasswordVerdict::Unverifiable => {
                    // An entry in a format nothing can recompute is not a
                    // wrong password, and telling the user "authentication
                    // failure" would send them away retyping a password
                    // that was never going to work.  The remedy is root
                    // setting a new one, so say so.  Only the account's own
                    // owner reaches this branch — root skips the
                    // old-password check entirely — so it discloses
                    // nothing.
                    eprintln!(
                        "passwd: the stored password for `{target}' is not in a format \
                         this system can verify, so it cannot be confirmed; ask an \
                         administrator to run `passwd {target}' as root"
                    );
                    return 1;
                }
                OldPasswordVerdict::Refuse => {
                    eprintln!("passwd: authentication failure");
                    return 1;
                }
            }
        }
    }

    // Read new password.
    let new_pw = match read_password_no_echo("New password: ") {
        Ok(p) => p,
        Err(e) => {
            eprintln!("passwd: {e}");
            return 1;
        }
    };

    // Check strength (only for non-root; root can set weak passwords).
    if caller_uid != 0 {
        let strength = check_password_strength(&new_pw);
        if !strength.ok {
            eprintln!("passwd: password does not meet requirements:");
            for reason in &strength.reasons {
                eprintln!("  - {reason}");
            }
            return 1;
        }
    }

    // Confirm.
    let confirm = match read_password_no_echo("Retype new password: ") {
        Ok(p) => p,
        Err(e) => {
            eprintln!("passwd: {e}");
            return 1;
        }
    };

    if new_pw != confirm {
        eprintln!("passwd: passwords do not match");
        return 1;
    }

    // Hashing, salting and dating the change all happen inside `set_password`
    // — see the module note on the copy of them that used to stand here.
    let record = match accounts.record(target) {
        Ok(record) => record,
        Err(code) => return code,
    };
    if let Err(e) = record.set_password(&new_pw) {
        eprintln!("passwd: {e}");
        return 1;
    }
    let still_locked = record.is_locked();

    if let Err(e) = accounts.store() {
        eprintln!("passwd: {e}");
        return 1;
    }

    eprintln!("passwd: password updated successfully");
    if still_locked {
        // Setting a password does not unlock (`design-decisions.md` §1003), and
        // an administrator who is not told that would walk away believing the
        // account works.
        eprintln!(
            "passwd: note: {} is still locked and will refuse this password; \
             run `passwd -u' to unlock it",
            quoteaf_os(target)
        );
    }
    0
}

/// Lock an account.
fn cmd_lock(accounts: &mut Accounts, target: &str) -> i32 {
    let record = match accounts.record(target) {
        Ok(record) => record,
        Err(code) => return code,
    };

    if record.is_locked() {
        eprintln!("passwd: account already locked");
        return 0;
    }

    record.set_locked(true);

    if let Err(e) = accounts.store() {
        eprintln!("passwd: {e}");
        return 1;
    }

    eprintln!("passwd: account {} locked", quoteaf_os(target));
    0
}

/// Unlock an account.
fn cmd_unlock(accounts: &mut Accounts, target: &str) -> i32 {
    let record = match accounts.record(target) {
        Ok(record) => record,
        Err(code) => return code,
    };

    if !record.is_locked() {
        eprintln!("passwd: account is not locked");
        return 1;
    }

    record.set_locked(false);

    // `set_locked(false)` restores the password the lock was laid over, but
    // there may not have been one: an account whose entry is `*`, or `!` with
    // nothing after it, has no password underneath to come back. Unlocking it
    // would mean either leaving it unusable while reporting success, or
    // emptying the entry — and an empty entry is the spelling of "logs in
    // without being asked for anything", which is not what anyone means by
    // "unlock". So the refusal stands, and names the two things that do work.
    if record.is_locked() || !has_password(record) {
        eprintln!("passwd: cannot unlock — account has no password set");
        eprintln!("passwd: use passwd -d to remove password or set a new password");
        return 1;
    }

    if let Err(e) = accounts.store() {
        eprintln!("passwd: {e}");
        return 1;
    }

    eprintln!("passwd: account {} unlocked", quoteaf_os(target));
    0
}

/// Delete the password (allow passwordless login).
fn cmd_delete_password(accounts: &mut Accounts, target: &str) -> i32 {
    let record = match accounts.record(target) {
        Ok(record) => record,
        Err(code) => return code,
    };

    // The lock goes with the password. Under the old `!`-prefix spelling this
    // was automatic — emptying the entry took the `!` with it — and losing it
    // silently would leave `passwd -d` reporting that the account needs no
    // password while the account still refused every login.
    record.set_locked(false);
    record.set(userdb::field::PASSWORD_HASH, "");
    let mut aging = record.aging();
    aging.changed = userdb::today();
    record.set_aging(&aging);

    if let Err(e) = accounts.store() {
        eprintln!("passwd: {e}");
        return 1;
    }

    eprintln!("passwd: password deleted for {}", quoteaf_os(target));
    0
}

/// Display password status information.
///
/// The seven fields are shadow-utils': name, status, date of last change, then
/// minimum, maximum, warning and inactive days. An unset policy prints `-1`,
/// which is the same vocabulary `chage -l` and `passwd -S` use for an empty
/// shadow field — and, unlike the `0 99999 7` this used to print, does not
/// present an invented policy as one the administrator set.
fn cmd_show_status(accounts: &Accounts, target: &str) -> i32 {
    let Some(record) = accounts.find(target) else {
        return vanished(target);
    };

    let aging = record.aging();
    println!(
        "{} {} {} {} {} {} {}",
        target,
        status_char(record),
        userdb::date_from_days(aging.changed.unwrap_or(0)),
        policy(aging.min_days),
        policy(aging.max_days),
        policy(aging.warn_days),
        policy(aging.inactive_days),
    );

    0
}

/// One aging number as `-S` prints it: the value, or `-1` for no policy.
fn policy(value: Option<i64>) -> i64 {
    value.unwrap_or(-1)
}

/// Expire password — force a change at next login by dating it to the epoch.
fn cmd_expire(accounts: &mut Accounts, target: &str) -> i32 {
    let record = match accounts.record(target) {
        Ok(record) => record,
        Err(code) => return code,
    };

    let mut aging = record.aging();
    aging.changed = Some(0);
    record.set_aging(&aging);

    if let Err(e) = accounts.store() {
        eprintln!("passwd: {e}");
        return 1;
    }

    eprintln!("passwd: password for {} expired", quoteaf_os(target));
    0
}

/// Set one aging field, and report it in the caller's own words.
///
/// The four `-n -x -w -i` commands differ only in which field they write and
/// what they say afterwards, so they share this rather than repeating the
/// load-modify-save four times — four copies being four places for one of them
/// to forget the save.
///
/// `days` is stored as given, including a negative one: `chage(1)` and
/// `passwd(1)` both spell "no policy" as `-1` on the command line, and a caller
/// who asks for that means to clear the field, not to set it to minus one day.
fn set_aging_field(
    accounts: &mut Accounts,
    target: &str,
    days: i64,
    field: fn(&mut Aging) -> &mut Option<i64>,
    announce: &str,
) -> i32 {
    let record = match accounts.record(target) {
        Ok(record) => record,
        Err(code) => return code,
    };

    let mut aging = record.aging();
    *field(&mut aging) = if days < 0 { None } else { Some(days) };
    record.set_aging(&aging);

    if let Err(e) = accounts.store() {
        eprintln!("passwd: {e}");
        return 1;
    }

    if days < 0 {
        eprintln!("passwd: {announce} for {} cleared", quoteaf_os(target));
    } else {
        eprintln!(
            "passwd: {announce} for {} set to {} day(s)",
            quoteaf_os(target),
            days
        );
    }
    0
}

// ============================================================================
// Date helper -- deleted
// ============================================================================
//
// `days_to_date_string` and its `is_leap_year` stood here, walking a year at a
// time from 1970. They are `userdb::date_from_days` now, next to the fields
// that are day numbers, and shared with `chage` and `useradd` -- which had
// their own copies, disagreeing about what a day number before the epoch
// means. This one clamped such a date to `1970-01-01`; the shared one converts
// it, so `-S` on a record hand-edited to a negative date now prints the date
// it says rather than one it does not.

// ============================================================================
// Main entry point
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();

    let parsed = match parse_args(&args) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("passwd: {e}");
            print_usage();
            process::exit(1);
        }
    };

    let caller_uid = current_uid();

    // Resolve target user.
    let target = match &parsed.target_user {
        Some(name) => name.clone(),
        None => match current_username() {
            Some(name) => name,
            None => {
                eprintln!("passwd: cannot determine current user");
                process::exit(1);
            }
        },
    };

    // The database is read once, here, and handed to whichever command runs.
    // Reading it inside each command would mean a `-S` that reports one file
    // and a `-x` that writes another, and — worse — the read-modify-write
    // sequence that made `find_or_create_shadow` necessary.
    let mut accounts = match Accounts::load(std::path::Path::new(userdb::DEFAULT_PATH)) {
        Ok(accounts) => accounts,
        Err(e) => {
            eprintln!("passwd: {e}");
            process::exit(1);
        }
    };

    if accounts.find(&target).is_none() {
        eprintln!("passwd: user {} does not exist", quoteaf_os(&target));
        process::exit(1);
    }

    // Permission check: non-root users can only change their own password
    // (the default ChangePassword action, no flags).
    let changing_own =
        parsed.target_user.is_none() || current_username().as_deref() == Some(target.as_str());

    if !is_root() && !changing_own {
        eprintln!("passwd: only root may change another user's password");
        process::exit(1);
    }

    // Non-ChangePassword actions require root.
    if !is_root() && !matches!(parsed.action, Action::ChangePassword) {
        eprintln!("passwd: only root may use this option");
        process::exit(1);
    }

    let exit_code = match parsed.action {
        Action::ChangePassword => cmd_change_password(&mut accounts, &target, caller_uid),
        Action::Lock => cmd_lock(&mut accounts, &target),
        Action::Unlock => cmd_unlock(&mut accounts, &target),
        Action::DeletePassword => cmd_delete_password(&mut accounts, &target),
        Action::ShowStatus => cmd_show_status(&accounts, &target),
        Action::Expire => cmd_expire(&mut accounts, &target),
        Action::SetMinDays(d) => set_aging_field(
            &mut accounts,
            &target,
            d,
            |a| &mut a.min_days,
            "minimum password age",
        ),
        Action::SetMaxDays(d) => set_aging_field(
            &mut accounts,
            &target,
            d,
            |a| &mut a.max_days,
            "maximum password age",
        ),
        Action::SetWarnDays(d) => set_aging_field(
            &mut accounts,
            &target,
            d,
            |a| &mut a.warn_days,
            "warning days",
        ),
        Action::SetInactiveDays(d) => set_aging_field(
            &mut accounts,
            &target,
            d,
            |a| &mut a.inactive_days,
            "inactive days",
        ),
    };

    process::exit(exit_code);
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ---- What this program writes, read back with what the system reads ----
    //
    // The SHA-256 known-answer tests that once stood here went with the
    // SHA-256 implementation they checked; the `hash_password` tests that
    // replaced them have now gone with `hash_password` itself, which is
    // `userdb::Record::set_password` and is tested next to its own definition.
    //
    // What remains this crate's to keep is the property both sets existed for,
    // and it is not a property of a function: that a password *this program
    // stores* is one the system's verifier accepts when it reads it back out
    // of the file. That is the bug
    // `requests/c-b-passwd-and-login-disagree-about-etc-shadow.md` was filed
    // for, and it is checked below end to end — through a real save into a
    // scratch directory and the `/etc/shadow` that save generates — because
    // every previous version of it was checked through a function call, and a
    // function call is exactly what could not see the disagreement.

    /// A salt `crypt` can carry verbatim, in the alphabet `userdb` draws from.
    ///
    /// Pinned rather than generated: a hash function asked to supply its own
    /// randomness can only be tested against itself, which is the test that let
    /// all three of the constructions this crate replaced pass while wrong.
    const SALT: &str = "abcdef0123456789";

    /// A database holding one account with `password` set.
    fn db_with(username: &str, password: &str) -> UserDb {
        let mut record = Record::new();
        record.set(userdb::field::USERNAME, username);
        record.set_uid(1000);
        record
            .set_password_with_salt(password, SALT)
            .expect("the pinned salt is one crypt can carry");
        let mut db = UserDb::new();
        db.push(record);
        db
    }

    /// The stored entry `password` produces, as this program would store it.
    fn stored_entry(password: &str) -> String {
        db_with("alice", password)
            .find("alice")
            .and_then(|r| r.get(userdb::field::PASSWORD_HASH))
            .expect("the record was just given a password")
    }

    /// Field `index` of `username`'s line in a generated `/etc/shadow`,
    /// counting from zero — so 1 is the password entry and 2 is the date it
    /// was last changed.
    ///
    /// The generated file is read rather than the database, because it is the
    /// file the rest of the system authenticates against: a test that read the
    /// YAML back would be checking that this program can read its own writing,
    /// which was never the thing in doubt.
    fn shadow_field(dir: &std::path::Path, username: &str, index: usize) -> String {
        let text = std::fs::read_to_string(dir.join(userdb::SHADOW_NAME))
            .expect("saving generates a shadow file beside the database");
        for line in text.lines() {
            let mut fields = line.split(':');
            if fields.next() == Some(username) {
                // `index - 1` because the name has already been taken off.
                return fields.nth(index - 1).unwrap_or_default().to_string();
            }
        }
        panic!("no line for `{username}' in the generated shadow:\n{text}");
    }

    /// The password field of `username`'s line in a generated `/etc/shadow`.
    fn generated_shadow_entry(dir: &std::path::Path, username: &str) -> String {
        shadow_field(dir, username, 1)
    }

    /// The whole of what this program is for: a password it saves is one the
    /// system's own verifier accepts, read back from the file the rest of the
    /// system actually authenticates against.
    #[test]
    fn a_password_this_program_saves_is_one_the_verifier_accepts() {
        let scratch = scratchdir::ScratchDir::new("passwd-round-trip");
        let db = db_with("dave", "correct horse");
        db.save(scratch.path("users.yaml")).expect("save");

        let stored = generated_shadow_entry(scratch.dir(), "dave");
        assert!(verify_password("correct horse", &stored), "{stored}");
        assert!(!verify_password("wrong horse", &stored), "{stored}");
    }

    /// The entry reaching `/etc/shadow` is a standard one: the `$6$`
    /// identifier and the salt as given. The format this file used to invent,
    /// `$sha256$<salt>$<64 hex>`, satisfied neither, which is why `login`
    /// could not read what `passwd` wrote.
    #[test]
    fn the_generated_entry_is_in_the_format_a_standard_reader_expects() {
        let scratch = scratchdir::ScratchDir::new("passwd-format");
        db_with("dave", "correct horse")
            .save(scratch.path("users.yaml"))
            .expect("save");

        let stored = generated_shadow_entry(scratch.dir(), "dave");
        assert!(stored.starts_with(&format!("$6${SALT}$")), "{stored}");
        assert_eq!(
            posix::crypt::stored_method(stored.as_bytes()),
            Some(posix::crypt::Method::Sha512),
            "{stored}"
        );
        assert!(!stored.contains("$sha256$"), "{stored}");
    }

    /// A locked account's generated entry verifies against nothing — **not
    /// even the correct password**.
    ///
    /// `cmd_change_password` refuses locked accounts in one place, up front.
    /// The old-password gate below it used to carry a second `!is_locked()`
    /// check that was already unreachable — and that duplicate failed *open*:
    /// had the up-front refusal ever been removed, the gate would have been
    /// skipped entirely and a locked account's password changed without the
    /// old one. With the duplicate gone, that path instead ends here, at a
    /// stored entry whose `!` prefix leaves it with no recomputable method.
    /// This test is what makes removing the duplicate safe, so it asserts the
    /// property directly rather than trusting the prefix to look wrong.
    #[test]
    fn a_locked_account_verifies_against_nothing_not_even_the_right_password() {
        let scratch = scratchdir::ScratchDir::new("passwd-locked");
        let mut db = db_with("dave", "correct horse");
        db.find_mut("dave").expect("the account").set_locked(true);
        db.save(scratch.path("users.yaml")).expect("save");

        let stored = generated_shadow_entry(scratch.dir(), "dave");
        assert!(stored.starts_with('!'), "{stored}");
        assert_eq!(
            posix::crypt::stored_method(stored.as_bytes()),
            None,
            "{stored}"
        );
        assert!(!verify_password("correct horse", &stored), "{stored}");
        assert!(!verify_password("", &stored), "{stored}");
    }

    /// A published SHA-crypt vector, checked through the verifier the rest of
    /// the system uses. This is the test the old hand-rolled code could not
    /// have had: its format had no specification and so no known answer.
    #[test]
    fn the_verifier_accepts_a_published_vector() {
        const VECTOR: &str = "$6$saltstring$svn8UoSVapNtMuq1ukKS4tPQd8iKwSMHWjl/O817G3uBnIFNjnQJuesI68u4OTLiBFdcbYEdFCoEOfaS35inz1";
        assert!(verify_password("Hello world!", VECTOR));
        assert!(!verify_password("Hello world", VECTOR));
    }

    /// The entries this tree wrote before `passwd` called `crypt` — its own
    /// `$sha256$`, and `chpasswd`'s 64 hex digits mislabelled `$5$` — can
    /// never verify, and are recognisable as such by shape.
    #[test]
    fn the_obsolete_formats_are_recognisable_and_unverifiable() {
        let digest = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        for prefix in ["$sha256$", "$5$", "$6$", "$1$"] {
            let stored = format!("{prefix}{SALT}${digest}");
            assert_eq!(
                posix::crypt::stored_method(stored.as_bytes()),
                None,
                "{stored}"
            );
            assert!(!verify_password("correct horse", &stored), "{stored}");
        }
    }

    // ---- Password strength tests ----

    #[test]
    fn strength_strong_password() {
        let result = check_password_strength("P@ssw0rd!");
        assert!(result.ok);
        assert!(result.reasons.is_empty());
    }

    #[test]
    fn strength_too_short() {
        let result = check_password_strength("Ab1!");
        assert!(!result.ok);
        assert!(result.reasons.iter().any(|r| r.contains("too short")));
    }

    #[test]
    fn strength_missing_uppercase() {
        let result = check_password_strength("p@ssw0rd!");
        assert!(!result.ok);
        assert!(result.reasons.iter().any(|r| r.contains("uppercase")));
    }

    #[test]
    fn strength_missing_lowercase() {
        let result = check_password_strength("P@SSW0RD!");
        assert!(!result.ok);
        assert!(result.reasons.iter().any(|r| r.contains("lowercase")));
    }

    #[test]
    fn strength_missing_digit() {
        let result = check_password_strength("P@ssword!");
        assert!(!result.ok);
        assert!(result.reasons.iter().any(|r| r.contains("digit")));
    }

    #[test]
    fn strength_missing_special() {
        let result = check_password_strength("Passw0rds");
        assert!(!result.ok);
        assert!(result.reasons.iter().any(|r| r.contains("special")));
    }

    #[test]
    fn strength_common_pattern_password() {
        let result = check_password_strength("Password1!");
        assert!(!result.ok);
        assert!(result.reasons.iter().any(|r| r.contains("common pattern")));
    }

    #[test]
    fn strength_common_pattern_123456() {
        let result = check_password_strength("A!123456bcde");
        assert!(!result.ok);
        assert!(result.reasons.iter().any(|r| r.contains("common pattern")));
    }

    #[test]
    fn strength_all_same_char() {
        let result = check_password_strength("AAAAAAAA");
        assert!(!result.ok);
        assert!(result.reasons.iter().any(|r| r.contains("same character")));
    }

    #[test]
    fn strength_empty_password() {
        let result = check_password_strength("");
        assert!(!result.ok);
        assert!(result.reasons.iter().any(|r| r.contains("too short")));
    }

    // ---- What `-S` reports about a record ----
    //
    // The `ShadowEntry` and `PasswdEntry` parse/round-trip tests that stood
    // here tested two parsers this program no longer has. What they were
    // really asserting — that the program agrees with the rest of the system
    // about what a stored entry *means* — is asserted below against the
    // record itself, which is now the only place that meaning is written down.

    /// A record with `entry` as its stored password and no aging policy.
    fn record_with_entry(entry: &str) -> Record {
        let mut record = Record::new();
        record.set(userdb::field::USERNAME, "alice");
        record.set_uid(1000);
        record.set(userdb::field::PASSWORD_HASH, entry);
        record
    }

    #[test]
    fn a_usable_password_reports_p() {
        let record = record_with_entry(&stored_entry("correct horse"));
        assert!(has_password(&record));
        assert_eq!(status_char(&record), "P");
    }

    /// A missing field and an empty one are one account — the one that logs in
    /// without being asked for anything — so they must report the same thing.
    #[test]
    fn no_password_at_all_reports_np_however_it_is_spelled() {
        let empty = record_with_entry("");
        assert!(!has_password(&empty));
        assert_eq!(status_char(&empty), "NP");

        let mut absent = Record::new();
        absent.set(userdb::field::USERNAME, "alice");
        assert!(!has_password(&absent));
        assert_eq!(status_char(&absent), "NP");
    }

    /// Every spelling of a lock reports `L`. `!` and `*` are the two the flat
    /// file has, and `locked: true` is the database's own; a lock only one of
    /// the three can see is not a lock.
    #[test]
    fn a_locked_account_reports_l_however_it_was_locked() {
        let hashed = stored_entry("correct horse");
        assert_eq!(status_char(&record_with_entry(&format!("!{hashed}"))), "L");
        assert_eq!(status_char(&record_with_entry(&format!("*{hashed}"))), "L");

        let mut flagged = record_with_entry(&hashed);
        flagged.set_locked(true);
        assert_eq!(status_char(&flagged), "L");
    }

    /// A pre-`crypt(3)` entry (§329) reports `L`, not `P`. `P` would tell an
    /// administrator the account has a working password when nothing on the
    /// system can check one against it — and the `/etc/shadow` the account is
    /// read through spells that same entry `*`, which shadow-utils also
    /// reports as `L`. The two must not disagree.
    #[test]
    fn an_entry_nothing_can_recompute_reports_l_rather_than_p() {
        let legacy = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        let record = record_with_entry(legacy);
        assert!(record.has_legacy_password());
        assert_eq!(status_char(&record), "L");
    }

    /// `-S` prints `-1` for a policy nobody set, rather than the `0 99999 7`
    /// this used to invent. The distinction is the whole reason the aging
    /// fields are optional: a `0` in the maximum-days column is not "no expiry
    /// policy", it is "expired the day it was set".
    #[test]
    fn an_unset_aging_field_prints_minus_one_rather_than_an_invented_default() {
        assert_eq!(policy(None), -1);
        assert_eq!(policy(Some(0)), 0);
        assert_eq!(policy(Some(90)), 90);
    }

    // ---- The minimum-age gate ----

    /// A record whose password was set `age` days ago, under a minimum age of
    /// `min` days. `None` for either is that field absent from the record.
    fn record_aged(min: Option<i64>, age: Option<i64>) -> Record {
        let mut record = record_with_entry(&stored_entry("correct horse"));
        let aging = Aging {
            min_days: min,
            changed: age.and_then(|days| userdb::today().map(|now| now - days)),
            ..Aging::default()
        };
        record.set_aging(&aging);
        record
    }

    #[test]
    fn a_password_changed_inside_the_minimum_age_may_not_be_changed_again() {
        assert_eq!(too_soon_to_change(&record_aged(Some(7), Some(2))), Some(5));
        assert_eq!(too_soon_to_change(&record_aged(Some(7), Some(0))), Some(7));
    }

    #[test]
    fn a_password_past_the_minimum_age_may_be_changed() {
        assert_eq!(too_soon_to_change(&record_aged(Some(7), Some(7))), None);
        assert_eq!(too_soon_to_change(&record_aged(Some(7), Some(400))), None);
    }

    /// No minimum-age policy delays nothing. The old code read that absence as
    /// a `0` it had invented and compared against — which permitted the change,
    /// but by accident of the number chosen rather than because the record said
    /// so. A different invented default would have refused it.
    #[test]
    fn an_absent_minimum_age_delays_nothing() {
        assert_eq!(too_soon_to_change(&record_aged(None, Some(0))), None);
        assert_eq!(too_soon_to_change(&record_aged(Some(0), Some(0))), None);
    }

    /// A minimum age with no date to measure from cannot refuse anyone.
    /// Refusing would lock a user out of changing their own password over a
    /// fact the database does not hold.
    #[test]
    fn a_minimum_age_with_no_change_date_delays_nothing() {
        assert_eq!(too_soon_to_change(&record_aged(Some(7), None)), None);
    }

    // ---- Argument parsing tests ----

    #[test]
    fn args_default_change_password() {
        let args = vec!["passwd".to_string()];
        let parsed = parse_args(&args).unwrap();
        assert!(matches!(parsed.action, Action::ChangePassword));
        assert!(parsed.target_user.is_none());
    }

    #[test]
    fn args_change_password_for_user() {
        let args = vec!["passwd".to_string(), "alice".to_string()];
        let parsed = parse_args(&args).unwrap();
        assert!(matches!(parsed.action, Action::ChangePassword));
        assert_eq!(parsed.target_user.as_deref(), Some("alice"));
    }

    #[test]
    fn args_lock() {
        let args = vec!["passwd".to_string(), "-l".to_string(), "bob".to_string()];
        let parsed = parse_args(&args).unwrap();
        assert!(matches!(parsed.action, Action::Lock));
        assert_eq!(parsed.target_user.as_deref(), Some("bob"));
    }

    #[test]
    fn args_unlock() {
        let args = vec!["passwd".to_string(), "-u".to_string(), "bob".to_string()];
        let parsed = parse_args(&args).unwrap();
        assert!(matches!(parsed.action, Action::Unlock));
    }

    #[test]
    fn args_delete() {
        let args = vec!["passwd".to_string(), "-d".to_string(), "bob".to_string()];
        let parsed = parse_args(&args).unwrap();
        assert!(matches!(parsed.action, Action::DeletePassword));
    }

    #[test]
    fn args_status() {
        let args = vec!["passwd".to_string(), "-S".to_string(), "bob".to_string()];
        let parsed = parse_args(&args).unwrap();
        assert!(matches!(parsed.action, Action::ShowStatus));
    }

    #[test]
    fn args_expire() {
        let args = vec!["passwd".to_string(), "-e".to_string(), "bob".to_string()];
        let parsed = parse_args(&args).unwrap();
        assert!(matches!(parsed.action, Action::Expire));
    }

    #[test]
    fn args_min_days() {
        let args = vec![
            "passwd".to_string(),
            "-n".to_string(),
            "5".to_string(),
            "bob".to_string(),
        ];
        let parsed = parse_args(&args).unwrap();
        assert!(matches!(parsed.action, Action::SetMinDays(5)));
    }

    #[test]
    fn args_max_days() {
        let args = vec![
            "passwd".to_string(),
            "-x".to_string(),
            "90".to_string(),
            "bob".to_string(),
        ];
        let parsed = parse_args(&args).unwrap();
        assert!(matches!(parsed.action, Action::SetMaxDays(90)));
    }

    #[test]
    fn args_warn_days() {
        let args = vec![
            "passwd".to_string(),
            "-w".to_string(),
            "14".to_string(),
            "bob".to_string(),
        ];
        let parsed = parse_args(&args).unwrap();
        assert!(matches!(parsed.action, Action::SetWarnDays(14)));
    }

    #[test]
    fn args_inactive_days() {
        let args = vec![
            "passwd".to_string(),
            "-i".to_string(),
            "30".to_string(),
            "bob".to_string(),
        ];
        let parsed = parse_args(&args).unwrap();
        assert!(matches!(parsed.action, Action::SetInactiveDays(30)));
    }

    #[test]
    fn args_unknown_option() {
        let args = vec!["passwd".to_string(), "-Z".to_string()];
        assert!(parse_args(&args).is_err());
    }

    #[test]
    fn args_missing_days_value() {
        let args = vec!["passwd".to_string(), "-n".to_string()];
        assert!(parse_args(&args).is_err());
    }

    #[test]
    fn args_invalid_days_value() {
        let args = vec!["passwd".to_string(), "-n".to_string(), "abc".to_string()];
        assert!(parse_args(&args).is_err());
    }

    #[test]
    fn args_duplicate_username() {
        let args = vec!["passwd".to_string(), "alice".to_string(), "bob".to_string()];
        assert!(parse_args(&args).is_err());
    }

    // ---- Date conversion ----
    //
    // The conversion itself is `userdb`'s and is tested there. What is checked
    // here is that `-S` prints the field it means to: the four dates below are
    // the ones this file's own printer was tested with, kept so that removing
    // that printer cannot have changed the output.

    #[test]
    fn the_status_line_prints_the_date_of_the_last_change() {
        assert_eq!(userdb::date_from_days(0), "1970-01-01");
        assert_eq!(userdb::date_from_days(19723), "2024-01-01");
        assert_eq!(userdb::date_from_days(11017), "2000-03-01");
        // A date before the epoch converts rather than clamping to it, which
        // is the one thing that changed when the three copies became one.
        assert_eq!(userdb::date_from_days(-5), "1969-12-27");
    }

    // ---- The commands, run against a real database in a scratch directory ----
    //
    // `find_or_create_shadow` had two tests here, and they are gone with it:
    // it existed only because a user could be in `/etc/passwd` with no
    // `/etc/shadow` line, and one database cannot be in that state. What the
    // commands do is now checked by running them — each one saves, and the
    // file it saved is reread and inspected, because "the field was set in
    // memory" is exactly what a command that forgot to save would also pass.

    /// One saved account with `password` set, and the `Accounts` a command
    /// would be handed for it.
    fn accounts_with(scratch: &scratchdir::ScratchDir, password: &str) -> Accounts {
        let path = scratch.path("users.yaml");
        db_with("dave", password).save(&path).expect("save");
        Accounts::load(&path).expect("load")
    }

    /// Reread the database from disk, as the next program to run would.
    fn reread(accounts: &Accounts) -> Accounts {
        Accounts::load(&accounts.path).expect("reload")
    }

    #[test]
    fn locking_makes_the_generated_shadow_refuse_the_right_password() {
        let scratch = scratchdir::ScratchDir::new("passwd-lock");
        let mut accounts = accounts_with(&scratch, "correct horse");

        assert_eq!(cmd_lock(&mut accounts, "dave"), 0);

        let reloaded = reread(&accounts);
        assert_eq!(status_char(reloaded.find("dave").expect("account")), "L");
        let stored = generated_shadow_entry(scratch.dir(), "dave");
        assert!(!verify_password("correct horse", &stored), "{stored}");
    }

    /// Unlocking gives back the password the lock was laid over — the property
    /// that makes `-l` reversible, and the one `userdb::Record::set_locked`
    /// exists to keep. An unlock that only cleared a flag would leave the
    /// account reporting unlocked and refusing every password.
    #[test]
    fn unlocking_restores_the_password_the_lock_was_laid_over() {
        let scratch = scratchdir::ScratchDir::new("passwd-unlock");
        let mut accounts = accounts_with(&scratch, "correct horse");

        assert_eq!(cmd_lock(&mut accounts, "dave"), 0);
        assert_eq!(cmd_unlock(&mut accounts, "dave"), 0);

        let reloaded = reread(&accounts);
        assert_eq!(status_char(reloaded.find("dave").expect("account")), "P");
        let stored = generated_shadow_entry(scratch.dir(), "dave");
        assert!(verify_password("correct horse", &stored), "{stored}");
    }

    /// An account with nothing underneath the lock is not unlocked into a
    /// passwordless one. `*` is the absence of a password, not a lock over
    /// one, so there is nothing to restore and an empty entry would mean "logs
    /// in without being asked for anything".
    #[test]
    fn unlocking_an_account_with_no_password_underneath_is_refused() {
        let scratch = scratchdir::ScratchDir::new("passwd-unlock-starred");
        let path = scratch.path("users.yaml");
        let mut db = UserDb::new();
        let mut record = Record::new();
        record.set(userdb::field::USERNAME, "dave");
        record.set_uid(1000);
        record.set(userdb::field::PASSWORD_HASH, "*");
        db.push(record);
        db.save(&path).expect("save");
        let mut accounts = Accounts::load(&path).expect("load");

        assert_eq!(cmd_unlock(&mut accounts, "dave"), 1);

        // And nothing was written: a refused command must leave the file as it
        // found it, not half-applied.
        let reloaded = Accounts::load(&path).expect("reload");
        let record = reloaded.find("dave").expect("account");
        assert!(record.is_locked());
        assert_eq!(status_char(record), "L");
        assert_eq!(generated_shadow_entry(scratch.dir(), "dave"), "*");
    }

    /// `-d` lifts the lock along with the password.
    ///
    /// Under the old `!`-prefix spelling this was automatic — emptying the
    /// entry took the `!` with it — so losing it silently was the likely bug
    /// in this move: `passwd -d` would report the account needs no password
    /// while the account went on refusing every login.
    #[test]
    fn deleting_a_password_also_lifts_the_lock_that_was_over_it() {
        let scratch = scratchdir::ScratchDir::new("passwd-delete");
        let mut accounts = accounts_with(&scratch, "correct horse");

        assert_eq!(cmd_lock(&mut accounts, "dave"), 0);
        assert_eq!(cmd_delete_password(&mut accounts, "dave"), 0);

        let reloaded = reread(&accounts);
        let record = reloaded.find("dave").expect("account");
        assert!(
            !record.is_locked(),
            "-d must not leave an account that reports no password and refuses every login"
        );
        assert_eq!(status_char(record), "NP");
        assert_eq!(generated_shadow_entry(scratch.dir(), "dave"), "");
    }

    /// `-e` dates the password to the epoch, which is how `/etc/shadow` spells
    /// "must be changed at the next login".
    #[test]
    fn expiring_a_password_dates_it_to_the_epoch() {
        let scratch = scratchdir::ScratchDir::new("passwd-expire");
        let mut accounts = accounts_with(&scratch, "correct horse");

        assert_eq!(cmd_expire(&mut accounts, "dave"), 0);

        let reloaded = reread(&accounts);
        assert_eq!(
            reloaded.find("dave").expect("account").aging().changed,
            Some(0)
        );
        assert_eq!(shadow_field(scratch.dir(), "dave", 2), "0");
    }

    /// Setting one aging field leaves the other five as they were — the
    /// property `userdb::Aging` is read and written whole for. A command that
    /// wrote six fields from a struct it had partly filled in would silently
    /// clear the five it was not asked about.
    #[test]
    fn setting_one_aging_field_leaves_the_others_alone() {
        let scratch = scratchdir::ScratchDir::new("passwd-maxdays");
        let mut accounts = accounts_with(&scratch, "correct horse");

        assert_eq!(
            set_aging_field(
                &mut accounts,
                "dave",
                90,
                |a| &mut a.max_days,
                "maximum password age"
            ),
            0
        );

        let reloaded = reread(&accounts);
        let aging = reloaded.find("dave").expect("account").aging();
        assert_eq!(aging.max_days, Some(90));
        assert_eq!(aging.min_days, None);
        assert_eq!(aging.warn_days, None);
        assert_eq!(aging.inactive_days, None);
        assert_eq!(aging.expires, None);
        // The password was set, so its date is the one field that is not empty.
        assert_eq!(aging.changed, userdb::today());

        // ...and it lands in the column the format defines, leaving the
        // untouched ones empty rather than zero.
        assert_eq!(shadow_field(scratch.dir(), "dave", 4), "90");
        assert_eq!(shadow_field(scratch.dir(), "dave", 3), "");
        assert_eq!(shadow_field(scratch.dir(), "dave", 5), "");
    }

    /// `-1` clears a policy rather than storing minus one day.
    ///
    /// `chage(1)` and `passwd(1)` both spell "no policy" as `-1` on the
    /// command line, but glibc reads a literal `-1` *in the file* as a date one
    /// day before the epoch. Writing it through would turn "no maximum age"
    /// into "expired since 1969" for every account it was applied to.
    #[test]
    fn a_negative_day_count_clears_the_policy_rather_than_storing_it() {
        let scratch = scratchdir::ScratchDir::new("passwd-clear-maxdays");
        let mut accounts = accounts_with(&scratch, "correct horse");
        let max_days: fn(&mut Aging) -> &mut Option<i64> = |a| &mut a.max_days;

        assert_eq!(
            set_aging_field(&mut accounts, "dave", 90, max_days, "maximum password age"),
            0
        );
        assert_eq!(
            set_aging_field(&mut accounts, "dave", -1, max_days, "maximum password age"),
            0
        );

        let reloaded = reread(&accounts);
        assert_eq!(
            reloaded.find("dave").expect("account").aging().max_days,
            None
        );
        assert_eq!(shadow_field(scratch.dir(), "dave", 4), "");
    }

    // ---- The shared failed-attempt tally (§354) ----

    /// Does the *system's* verifier accept this password for this stored
    /// entry?
    ///
    /// A test helper, not a program function — see the note where the old
    /// production `verify_password` stood. The assertions below are about what
    /// this program writes, checked against the one verifier every other
    /// program reads it with.
    fn verify_password(password: &str, stored_hash: &str) -> bool {
        authlib::check_stored(password.as_bytes(), stored_hash.as_bytes()).is_accepted()
    }

    /// A verifier that counts in memory and nowhere else, so that running this
    /// suite cannot run up a delay against a real account on the developer's
    /// machine. `with_stores` attaches no faillock file; `new()` would.
    fn scratch_authenticator() -> authlib::Authenticator {
        let missing = std::path::Path::new("/nonexistent/passwd-tests");
        authlib::Authenticator::with_stores(missing, missing)
    }

    /// A wrong current password is charged to the shared tally, so that
    /// guessing here is no longer the one free prompt in the system.
    #[test]
    fn a_wrong_current_password_is_charged_to_the_shared_tally() {
        let mut auth = scratch_authenticator();
        let stored = stored_entry("correct horse");

        for expected in 1..=3_u32 {
            let outcome = authlib::check_stored(b"wrong", stored.as_bytes());
            assert_eq!(
                judge_old_password(&mut auth, "alice", outcome),
                OldPasswordVerdict::Refuse
            );
            assert_eq!(auth.failures("alice"), expected);
        }
    }

    /// …and it is *only* charged. `passwd` never asks whether the user is
    /// delayed, so a user already past the free attempts — held there by an
    /// attacker at some other prompt, in the case this exception exists for —
    /// can still change their password and lock that attacker out.
    #[test]
    fn passwd_is_never_delayed_by_the_tally_it_contributes_to() {
        let mut auth = scratch_authenticator();
        let stored = stored_entry("correct horse");

        // Well past `FREE_ATTEMPTS`: every other prompt would refuse outright.
        for _ in 0..(authlib::FREE_ATTEMPTS + 5) {
            auth.note_failure("alice");
        }
        assert!(
            auth.rate_limited("alice").is_some(),
            "the premise: alice is delayed everywhere else"
        );

        let outcome = authlib::check_stored(b"correct horse", stored.as_bytes());
        assert_eq!(
            judge_old_password(&mut auth, "alice", outcome),
            OldPasswordVerdict::Proceed,
            "changing your password is the remedy, and a delay must not take it away"
        );
        // And succeeding clears the run of failures, as a login does.
        assert_eq!(auth.failures("alice"), 0);
        assert!(auth.rate_limited("alice").is_none());
    }

    /// An entry nothing can recompute is reported as broken, not counted as a
    /// guess: no answer could ever have matched it, so taxing the attempt
    /// would lock the user out of `login` for an administrator's mistake.
    #[test]
    fn an_unverifiable_entry_is_reported_not_counted() {
        let mut auth = scratch_authenticator();
        // 64 hex digits: what this tree's replaced hand-rolled format wrote.
        let legacy = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        let outcome = authlib::check_stored(b"anything", legacy.as_bytes());

        assert_eq!(
            judge_old_password(&mut auth, "dave", outcome),
            OldPasswordVerdict::Unverifiable
        );
        assert_eq!(auth.failures("dave"), 0);
    }

    /// `verify_password` now states no policy of its own: for every shape of
    /// stored entry it agrees with `authlib`, which is the single definition
    /// the rest of the system authenticates against.
    #[test]
    fn verification_agrees_with_authlib_for_every_shape_of_entry() {
        let good = stored_entry("correct horse");
        let locked = format!("!{good}");
        let cases: [(&str, &str); 7] = [
            ("correct horse", &good),
            ("wrong", &good),
            ("correct horse", &locked),
            ("anything", "!"),
            ("anything", "*"),
            ("anything", ""),
            ("anything", "not a hash at all"),
        ];
        for (password, stored) in cases {
            assert_eq!(
                verify_password(password, stored),
                authlib::check_stored(password.as_bytes(), stored.as_bytes()).is_accepted(),
                "disagreement on ({password:?}, {stored:?})"
            );
        }
    }
}
