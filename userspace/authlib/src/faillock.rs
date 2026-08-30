//! The failure tally, shared between programs rather than owned by one.
//!
//! # Why this is on disk
//!
//! [`Authenticator`](crate::Authenticator) counts failures in memory, which
//! works for a daemon — `sshd` and `ftpd` keep one verifier for the life of the
//! process, so their tallies outlive a connection. It does nothing at all for
//! `doas`, `su` or `login`, which *are* the process: each invocation starts with
//! an empty tally, so the growing delay never grows and guessing at a `doas`
//! prompt is free and unlimited. That is exactly the shape of an attacker who
//! already has a shell as the user and is guessing toward root.
//!
//! One tally per *user*, shared by every program that asks, is the fix. It is
//! also the more correct model on its own terms: the budget belongs to the
//! account being attacked, not to whichever binary happens to be asking, and an
//! attacker who could reset it by switching from `su` to `doas` would have no
//! budget at all.
//!
//! # Why one file and not one file per user
//!
//! The obvious layout — `/var/run/authlib/<user>` — takes its filename from a
//! string typed at a login prompt, which is attacker-controlled input on a path,
//! and lets an attacker create unbounded files by failing against unbounded
//! invented usernames. Encoding the name closes the first hole; nothing closes
//! the second while the table can grow.
//!
//! So the table is a single file with a fixed number of slots. Bounded disk, one
//! atomic rename per update, and — the part that matters for disclosure — an
//! invented username occupies a slot exactly as a real one does, so nothing
//! about the file distinguishes accounts that exist from accounts that do not.
//! Skipping unknown users instead would have been simpler and would have leaked
//! precisely that.
//!
//! # Eviction, and why it is by failure count
//!
//! A full table must drop something, and *which* something is a security
//! property: an attacker who can choose the victim can clear the record of their
//! own attack. Evicting the least-recently-used entry gives them that for the
//! price of [`MAX_SLOTS`] junk attempts.
//!
//! So the victim is the slot with the **fewest failures**, ties broken by the
//! oldest. A user under active attack has the highest count in the table and is
//! therefore the last thing evicted, and the junk entries an attacker would need
//! to flush them are themselves the cheapest slots to reclaim. Flushing a
//! 10-failure entry means landing 10 failures on each of [`MAX_SLOTS`] other
//! names first, every one of them rate-limited by this same table.

use std::collections::BTreeMap;
use std::path::Path;

/// How many users the table remembers at once.
///
/// Bounded because the keys are attacker-supplied (see the module docs). Large
/// enough that evicting a real user's record costs an attacker far more attempts
/// than the record was saving, small enough that the file stays a few tens of
/// kilobytes and is rewritten in one block.
pub(crate) const MAX_SLOTS: usize = 1024;

/// The longest a username may be before the table refuses to record it.
///
/// A tally keyed by a megabyte-long name would let one failure consume the whole
/// file. Names this long do not identify accounts on this system; refusing to
/// *record* one costs an attacker nothing they did not already have (the attempt
/// is still refused, it is simply not remembered) and bounds the file.
pub(crate) const MAX_USERNAME_LEN: usize = 256;

/// First line of the file, so a future format change is detected rather than
/// misparsed into a tally that silently admits people.
const MAGIC: &str = "authlib-tally 1";

/// One user's recent failures.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Tally {
    pub(crate) failures: u32,
    pub(crate) last_failure_secs: u64,
}

/// The whole table, keyed by username.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Table {
    entries: BTreeMap<String, Tally>,
}

impl Table {
    /// What is recorded for `user`, if anything.
    pub(crate) fn get(&self, user: &str) -> Option<Tally> {
        self.entries.get(user).copied()
    }

    /// Forget `user`'s failures.
    pub(crate) fn clear(&mut self, user: &str) {
        self.entries.remove(user);
    }

    /// Record `tally` as `user`'s current count.
    ///
    /// The caller supplies the whole value rather than asking for an increment,
    /// because the number that must be written is the one derived from *both*
    /// halves of the tally — see `Authenticator::authenticate`. A table that
    /// incremented its own copy would restart the escalation at one for every
    /// program that had not failed yet itself.
    ///
    /// Silently does nothing for an over-long name — see [`MAX_USERNAME_LEN`].
    pub(crate) fn set(&mut self, user: &str, tally: Tally) {
        if user.len() > MAX_USERNAME_LEN {
            return;
        }
        if !self.entries.contains_key(user) {
            self.make_room();
        }
        self.entries.insert(user.to_string(), tally);
    }

    /// Drop the least-valuable slot if the table is full.
    ///
    /// Fewest failures first, oldest as the tie-break — the reasoning is in the
    /// module docs, and it is the difference between an attacker needing
    /// [`MAX_SLOTS`] junk attempts to clear a record and needing
    /// `MAX_SLOTS * failures` of them.
    fn make_room(&mut self) {
        while self.entries.len() >= MAX_SLOTS {
            let Some(victim) = self
                .entries
                .iter()
                .min_by_key(|(name, t)| (t.failures, t.last_failure_secs, (*name).clone()))
                .map(|(name, _)| name.clone())
            else {
                return;
            };
            self.entries.remove(&victim);
        }
    }

    /// Render the table as the file's text.
    fn to_text(&self) -> String {
        let mut out = String::from(MAGIC);
        out.push('\n');
        for (user, tally) in &self.entries {
            // The name is hex-encoded, not quoted: a username may contain any
            // byte but `/` and NUL, which includes spaces and newlines, and a
            // tally file that can be reshaped by choosing a username is a tally
            // file an attacker can edit from a login prompt.
            out.push_str(&hex_encode(user.as_bytes()));
            out.push(' ');
            out.push_str(&tally.failures.to_string());
            out.push(' ');
            out.push_str(&tally.last_failure_secs.to_string());
            out.push('\n');
        }
        out
    }

    /// Parse the file's text, ignoring anything malformed.
    ///
    /// A damaged line is dropped rather than failing the whole read: the file is
    /// a rate limit, and the failure mode of losing one user's count is far
    /// better than the failure mode of a verifier that will not run.
    fn from_text(text: &str) -> Self {
        let mut entries = BTreeMap::new();
        let mut lines = text.lines();
        if lines.next() != Some(MAGIC) {
            // Not our format, or a version we do not know. An unrecognised file
            // is treated as empty rather than guessed at.
            return Self::default();
        }
        for line in lines {
            let mut parts = line.split(' ');
            let (Some(name), Some(failures), Some(last), None) =
                (parts.next(), parts.next(), parts.next(), parts.next())
            else {
                continue;
            };
            let (Some(name), Ok(failures), Ok(last)) = (
                hex_decode(name).and_then(|b| String::from_utf8(b).ok()),
                failures.parse::<u32>(),
                last.parse::<u64>(),
            ) else {
                continue;
            };
            if name.len() > MAX_USERNAME_LEN || entries.len() >= MAX_SLOTS {
                continue;
            }
            entries.insert(
                name,
                Tally {
                    failures,
                    last_failure_secs: last,
                },
            );
        }
        Self { entries }
    }

    /// Read the table at `path`. A missing or unreadable file is an empty table.
    ///
    /// Unreadable must mean empty rather than "refuse everyone": this file is
    /// the *rate limit*, not the password store, and a system that cannot read
    /// it should still be able to log its administrator in to fix it.
    pub(crate) fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .map_or_else(|_| Self::default(), |text| Self::from_text(&text))
    }

    /// Write the table to `path`, replacing it atomically.
    ///
    /// Returns whether it was written; the caller carries on either way, because
    /// an in-memory tally still limits a daemon and refusing to authenticate
    /// anyone because `/var/run` is full would be a worse failure than losing the
    /// shared count.
    pub(crate) fn store(&self, path: &Path) -> bool {
        let Some(dir) = path.parent() else {
            return false;
        };
        if !dir.as_os_str().is_empty() {
            if std::fs::create_dir_all(dir).is_err() {
                return false;
            }
            // The directory matters as much as the file. `create_dir_all` uses
            // the process umask, and a setuid program inherits the umask of
            // whoever ran it -- so a user with `umask 0` who is first to
            // trigger a write would get a world-writable `/var/run/authlib`,
            // and could then rename their own file over the tally and clear
            // their failures. Mode is forced rather than inherited.
            restrict_dir(dir);
        }
        // Written beside the target and renamed over it, so a reader never sees
        // a half-written table and a crash mid-write leaves the old one intact.
        // The temp name carries the pid so two programs writing at once do not
        // corrupt each other's scratch file.
        let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
        if std::fs::write(&tmp, self.to_text()).is_err() {
            return false;
        }
        restrict(&tmp);
        if std::fs::rename(&tmp, path).is_err() {
            // Leaving the scratch file behind would accumulate one per failed
            // write, which is the disk exhaustion this module exists to avoid.
            drop(std::fs::remove_file(&tmp));
            return false;
        }
        true
    }
}

/// Make a file readable and writable only by its owner.
///
/// The tally must not be readable by the users it describes: it says which
/// accounts are under attack and when, and a user who could *write* it could
/// clear their own failures, which is the entire budget this module enforces.
#[cfg(unix)]
fn restrict(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    // Best effort: a filesystem without Unix modes is not a reason to refuse to
    // rate-limit. The mode is set on the scratch file *before* the rename, so
    // the table is never briefly world-readable under its real name.
    drop(std::fs::set_permissions(
        path,
        std::fs::Permissions::from_mode(0o600),
    ));
}

/// Make a directory enterable and writable only by its owner.
///
/// Separate from [`restrict`] because the bit that matters is different: 0700
/// on the directory is what stops a user *replacing* the tally by renaming
/// their own file over it, which no permission on the file itself can prevent.
#[cfg(unix)]
fn restrict_dir(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    // Best effort, for the same reason as `restrict`.
    drop(std::fs::set_permissions(
        path,
        std::fs::Permissions::from_mode(0o700),
    ));
}

/// No-op off Unix. The real target is `target-family = ["unix"]`; these arms
/// exist because the tests run on the Windows build host.
#[cfg(not(unix))]
fn restrict(_path: &Path) {}

#[cfg(not(unix))]
fn restrict_dir(_path: &Path) {}

/// Lowercase hex, two characters per byte.
fn hex_encode(bytes: &[u8]) -> String {
    /// A nibble is 0..=15 by construction, so this cannot fail — but it is
    /// written as arithmetic rather than a table lookup so that no reader (and
    /// no lint) has to take that on trust.
    fn digit(nibble: u8) -> char {
        match nibble {
            0..=9 => char::from(b'0'.saturating_add(nibble)),
            _ => char::from(b'a'.saturating_add(nibble.saturating_sub(10))),
        }
    }
    let mut out = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        out.push(digit(byte >> 4));
        out.push(digit(byte & 0x0f));
    }
    out
}

/// Inverse of [`hex_encode`]; `None` for anything that is not even-length hex.
fn hex_decode(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(text.len() / 2);
    for pair in bytes.chunks_exact(2) {
        let hi = char::from(*pair.first()?).to_digit(16)?;
        let lo = char::from(*pair.get(1)?).to_digit(16)?;
        out.push(u8::try_from(hi.checked_mul(16)?.checked_add(lo)?).ok()?);
    }
    Some(out)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trips_every_byte() {
        let all: Vec<u8> = (0..=255).collect();
        assert_eq!(hex_decode(&hex_encode(&all)).unwrap(), all);
    }

    #[test]
    fn hex_rejects_what_is_not_hex() {
        assert!(hex_decode("abc").is_none(), "odd length");
        assert!(hex_decode("zz").is_none(), "not a hex digit");
        assert!(hex_decode("  ").is_none(), "spaces");
    }

    /// A one-failure tally stamped at `now`, for tests that only care that a
    /// row exists.
    fn once(now: u64) -> Tally {
        Tally {
            failures: 1,
            last_failure_secs: now,
        }
    }

    /// A username containing a space or a newline must not be able to forge
    /// extra rows or corrupt neighbouring ones — the reason the name is encoded
    /// rather than written verbatim.
    #[test]
    fn a_username_cannot_inject_rows_into_the_table() {
        let mut table = Table::default();
        table.set("evil 99 0\ndeadbeef", once(7));
        table.set("real", once(7));

        let parsed = Table::from_text(&table.to_text());
        assert_eq!(parsed.get("real").unwrap().failures, 1);
        assert_eq!(parsed.get("evil 99 0\ndeadbeef").unwrap().failures, 1);
        // Two rows recorded, two rows read back: nothing was forged.
        assert_eq!(parsed.entries.len(), 2);
    }

    #[test]
    fn a_table_round_trips_through_text() {
        let mut table = Table::default();
        table.set("alice", once(100));
        table.set(
            "alice",
            Tally {
                failures: 2,
                last_failure_secs: 105,
            },
        );
        table.set("bob", once(200));

        let parsed = Table::from_text(&table.to_text());
        assert_eq!(parsed.get("alice").unwrap().failures, 2);
        assert_eq!(parsed.get("alice").unwrap().last_failure_secs, 105);
        assert_eq!(parsed.get("bob").unwrap().failures, 1);
    }

    #[test]
    fn a_file_without_the_magic_line_is_read_as_empty() {
        assert_eq!(Table::from_text("alice 3 100\n"), Table::default());
        assert_eq!(Table::from_text("authlib-tally 2\n"), Table::default());
        assert_eq!(Table::from_text(""), Table::default());
    }

    #[test]
    fn a_damaged_row_is_dropped_and_the_rest_survive() {
        let text = format!(
            "{MAGIC}\n{} 4 100\ngarbage\n{} notanumber 100\n{} 9 300\n",
            hex_encode(b"alice"),
            hex_encode(b"bob"),
            hex_encode(b"carol"),
        );
        let parsed = Table::from_text(&text);
        assert_eq!(parsed.get("alice").unwrap().failures, 4);
        assert!(parsed.get("bob").is_none());
        assert_eq!(parsed.get("carol").unwrap().failures, 9);
    }

    #[test]
    fn an_over_long_username_is_refused_a_slot() {
        let mut table = Table::default();
        let long = "x".repeat(MAX_USERNAME_LEN + 1);
        table.set(&long, once(1));
        assert!(table.get(&long).is_none());
    }

    #[test]
    fn the_table_never_exceeds_its_slot_count() {
        let mut table = Table::default();
        for i in 0..(MAX_SLOTS * 2) {
            table.set(&format!("user{i}"), once(u64::try_from(i).unwrap()));
        }
        assert_eq!(table.entries.len(), MAX_SLOTS);
    }

    /// The property eviction exists for: an attacker who floods the table with
    /// invented names must not thereby clear the record of the account they are
    /// actually attacking.
    #[test]
    fn flooding_the_table_does_not_evict_the_account_under_attack() {
        let mut table = Table::default();
        table.set(
            "victim",
            Tally {
                failures: 10,
                last_failure_secs: 50,
            },
        );
        assert_eq!(table.get("victim").unwrap().failures, 10);

        // Every junk name gets one failure — far more names than there are
        // slots, all of them arriving later than the victim's.
        for i in 0..(MAX_SLOTS * 3) {
            table.set(&format!("junk{i}"), once(1000 + u64::try_from(i).unwrap()));
        }

        assert_eq!(
            table.get("victim").map(|t| t.failures),
            Some(10),
            "the most-failed entry must be the last one evicted"
        );
    }

    #[test]
    fn a_stored_table_reloads_from_disk() {
        let dir = scratchdir::ScratchDir::new("authlib_faillock_test");
        let path = dir.path("faillock.tbl");

        let mut table = Table::default();
        table.set(
            "alice",
            Tally {
                failures: 2,
                last_failure_secs: 43,
            },
        );
        assert!(table.store(&path));

        let loaded = Table::load(&path);
        assert_eq!(loaded.get("alice").unwrap().failures, 2);
        assert_eq!(loaded.get("alice").unwrap().last_failure_secs, 43);
    }

    #[test]
    fn a_missing_file_loads_as_an_empty_table() {
        let dir = scratchdir::ScratchDir::new("authlib_faillock_test");
        let path = dir.path("does-not-exist.tbl");
        assert_eq!(Table::load(&path), Table::default());
    }

    /// Storing must not leave scratch files behind, since one per failed write
    /// is the disk exhaustion this module is built to avoid.
    #[test]
    fn storing_leaves_no_scratch_file() {
        let dir = scratchdir::ScratchDir::new("authlib_faillock_test");
        let path = dir.path("scratch.tbl");

        let mut table = Table::default();
        table.set("alice", once(1));
        assert!(table.store(&path));

        let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
        assert!(!tmp.exists(), "scratch file survived a successful store");
    }

    /// The default path is `/var/run/authlib/tally`, and nothing else creates
    /// that directory — so a `store` that cannot make its own parent would make
    /// the whole shared tally silently do nothing on a real system, which is
    /// indistinguishable from not having written it.
    #[test]
    fn storing_creates_a_missing_parent_directory_and_locks_it_down() {
        let scratch = scratchdir::ScratchDir::new("authlib_faillock_test");
        let dir = scratch.path("absent-parent/nested");
        let path = dir.join("tally");
        assert!(!dir.exists(), "fixture directory was not clean");

        let mut table = Table::default();
        table.set("alice", once(1));
        assert!(table.store(&path), "store did not create its parent");
        assert_eq!(Table::load(&path).get("alice").unwrap().failures, 1);

        // The permission half only means anything on the real target; the test
        // host has no Unix modes. Asserted rather than skipped so that the
        // check exists the moment the suite runs there.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(&dir), 0o700, "tally directory is not owner-only");
            assert_eq!(mode(&path), 0o600, "tally file is not owner-only");
        }
    }
}
