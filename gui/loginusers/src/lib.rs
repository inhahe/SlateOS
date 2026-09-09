//! Which accounts this machine offers at a lock or login screen.
//!
//! # Why this is a crate and not a function in each screen
//!
//! There are two screens that ask the question — `apps/lockscreen` and the
//! desktop shell's `login_screen` — and they had, or were about to have, a copy
//! of the answer each. The two copies are not equally harmless in the way
//! duplicated *rendering* is: they decide **which names a person standing at
//! the machine is offered**, so when they drift, an account is offered at one
//! screen and missing at the other, and the person cannot tell whether it was
//! deleted, hidden, or never there. The failure is silent at both ends.
//!
//! It is deliberately *only* the offer policy. Deciding whether a typed
//! password is right belongs to [`authlib`], which reads the same store and
//! carries the shared failure tally; this crate must never grow a second
//! opinion about that. What it does guarantee is that the name on the screen
//! came from the file the authenticator will resolve against — that is why
//! [`DEFAULT_USERS_YAML`] is re-exported from `authlib` rather than spelled out
//! again here.
//!
//! # What each screen still owns
//!
//! [`Account`] describes what the database says. It is not what either screen
//! draws: the lock screen wants initials and a password hint, the login screen
//! wants an avatar, an account-type label and an autologin flag. Each builds
//! its own view type from an [`Account`], which keeps the shared thing
//! genuinely shared and leaves presentation where presentation belongs.

#![forbid(unsafe_code)]

use std::path::Path;

pub use authlib::DEFAULT_USERS_YAML;

/// The uid range that describes a person rather than a service.
///
/// The *upper* bound is the half that is easy to leave out and wrong to.
/// `nobody` is conventionally uid 65534 — above the range, not below it — so a
/// bare `uid >= 1000` filter drops `daemon` and keeps `nobody`, which is the
/// account the filter most obviously exists to hide.
pub const HUMAN_UIDS: core::ops::RangeInclusive<u32> = 1000..=60_000;

/// One account a lock or login screen will offer.
///
/// Every field is what the database says, not what a screen shows. In
/// particular `display_name` falls back to the username rather than to an empty
/// string, because a row with no label is a row nobody can identify.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Account {
    /// The numeric id, or `None` for a record that does not carry one.
    ///
    /// Kept as an `Option` rather than defaulted, because "the file does not
    /// say" and "the file says 0" are different facts and only one of them is
    /// root.
    pub uid: Option<u32>,
    /// The name typed at the prompt, and the name the authenticator is asked
    /// about.
    pub username: String,
    /// The name shown on the row. Falls back to `username`.
    pub display_name: String,
    /// Whether a password must be typed. See [`record_has_password`] for why a
    /// locked account answers `true`.
    pub has_password: bool,
    /// Whether the account is administrative, for a screen that labels it.
    pub is_admin: bool,
    /// Whether this account logs in without being chosen.
    pub auto_login: bool,
    /// When this account last logged in, for a screen that offers the most
    /// recent user first. Zero means never, which sorts last.
    pub last_login: u64,
    /// A per-account avatar identifier, if the record names one.
    pub avatar: Option<String>,
    /// Whether an administrator has locked the account.
    ///
    /// A locked account is still *offered* — see [`offered`] — so a screen may
    /// want to mark it. Nothing here decides what happens when its password is
    /// typed; that is the authenticator's verdict.
    pub is_locked: bool,
}

/// Whether `record` has a password that must be typed.
///
/// A locked account keeps its hash so that unlocking restores the old password.
/// It has one; it just will not open. So this answers `true`, the screen prompts
/// for it, and the authenticator gets to say `Locked` — which is a refusal the
/// person can act on, where an account that appeared to have no password and
/// then refused an empty one would not be.
#[must_use]
pub fn record_has_password(record: &userdb::Record) -> bool {
    if record.is_locked() {
        return true;
    }
    !record
        .get(userdb::field::PASSWORD_HASH)
        .unwrap_or_default()
        .is_empty()
}

/// The accounts the machine will offer, read from the database at `users_yaml`.
///
/// Read rather than cached: an account added while the screen is up appears the
/// next time this is called.
///
/// # What is filtered, and what deliberately is not
///
/// System accounts are dropped by [`HUMAN_UIDS`]. A record with **no readable
/// uid is kept**, because a hand-edited `users.yaml` that omits the field
/// describes a person far more often than it describes a daemon, and dropping
/// the machine's only account is a worse failure than listing one extra.
///
/// **Locked accounts stay on the list.** Hiding them would make an
/// administrator's `usermod -L` look like the account had been deleted, and the
/// person standing at the screen learns nothing from its absence that they do
/// not learn from being refused.
///
/// A record with no username is dropped, because there is nothing to ask the
/// authenticator about.
///
/// # When the database cannot be read
///
/// An empty list. There is nothing else to enumerate from: `/etc/passwd` and
/// `/etc/shadow` are *generated* from this file (`design-decisions.md` §353),
/// so a machine where it cannot be read has no account list at all, and
/// inventing one would offer names that cannot answer.
#[must_use]
pub fn offered(users_yaml: &Path) -> Vec<Account> {
    let Ok(db) = userdb::UserDb::load(users_yaml) else {
        return Vec::new();
    };
    db.records()
        .iter()
        .filter(|record| record.uid().is_none_or(|uid| HUMAN_UIDS.contains(&uid)))
        .filter_map(|record| {
            let username = record.username()?;
            let display_name = record.display_name().unwrap_or_else(|| username.clone());
            Some(Account {
                uid: record.uid(),
                username,
                display_name,
                has_password: record_has_password(record),
                is_admin: record.is_admin(),
                auto_login: record.auto_login(),
                last_login: record.last_login(),
                avatar: record.avatar(),
                is_locked: record.is_locked(),
            })
        })
        .collect()
}

/// The accounts the machine will offer, from the system database.
///
/// The same file [`authlib`] resolves against, which is the property that
/// matters: a screen enumerating one store while the authenticator reads
/// another offers names that cannot answer.
#[must_use]
pub fn offered_from_system() -> Vec<Account> {
    offered(Path::new(DEFAULT_USERS_YAML))
}

/// The index of the account to select first, or `None` for an empty list.
///
/// The most recently used account, falling back to the first. Shared because
/// both screens want it and both would otherwise write the same `max_by_key`
/// — and a `max_by_key` over `last_login` returns the *last* maximum on ties,
/// which on a fresh machine where every account has never logged in would
/// select the bottom row rather than the top one.
#[must_use]
pub fn most_recent(accounts: &[Account]) -> Option<usize> {
    if accounts.is_empty() {
        return None;
    }
    let mut best = 0;
    for (i, account) in accounts.iter().enumerate() {
        if account.last_login > accounts.get(best).map_or(0, |a| a.last_login) {
            best = i;
        }
    }
    Some(best)
}

#[cfg(test)]
mod tests {
    // A test module's job is to fail loudly the instant the code under test is
    // wrong, so the defensive lints that forbid exactly that in production code
    // are off here — as `CLAUDE.md` prescribes.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]

    use super::*;

    /// Writes a `users.yaml` into a directory of its own, so two tests cannot
    /// assert against each other's data.
    fn db_with(body: &str) -> (scratchdir::ScratchDir, std::path::PathBuf) {
        let dir = scratchdir::ScratchDir::new("loginusers");
        let path = dir.path("users.yaml");
        std::fs::write(&path, body).unwrap();
        (dir, path)
    }

    #[test]
    fn a_service_account_is_not_offered_and_a_person_is() {
        let (_dir, path) = db_with(
            "users:\n\
             - username: daemon\n   uid: 1\n   password_hash: x\n\
             - username: alice\n   uid: 1000\n   password_hash: x\n",
        );
        let names: Vec<String> = offered(&path).into_iter().map(|a| a.username).collect();
        assert_eq!(names, vec!["alice".to_string()]);
    }

    /// `nobody` is uid 65534 — *above* the range. A filter written as
    /// `uid >= 1000` keeps it, which is the account the filter most obviously
    /// exists to hide.
    #[test]
    fn the_upper_bound_is_what_hides_nobody() {
        let (_dir, path) = db_with(
            "users:\n\
             - username: nobody\n   uid: 65534\n\
             - username: alice\n   uid: 1000\n   password_hash: x\n",
        );
        let names: Vec<String> = offered(&path).into_iter().map(|a| a.username).collect();
        assert_eq!(names, vec!["alice".to_string()]);
    }

    /// A hand-edited file that omits the uid describes a person far more often
    /// than a daemon, and dropping the machine's only account is the worse
    /// failure.
    #[test]
    fn a_record_with_no_uid_is_kept() {
        let (_dir, path) = db_with("users:\n- username: alice\n   password_hash: x\n");
        let accounts = offered(&path);
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].uid, None);
    }

    /// Hiding a locked account would make `usermod -L` look like a deletion.
    #[test]
    fn a_locked_account_is_still_offered_and_still_asks_for_a_password() {
        let (_dir, path) = db_with(
            "users:\n- username: alice\n   uid: 1000\n   password_hash: '!x'\n   locked: true\n",
        );
        let accounts = offered(&path);
        assert_eq!(accounts.len(), 1);
        assert!(accounts[0].is_locked);
        assert!(
            accounts[0].has_password,
            "a locked account keeps its hash, so the screen must prompt and let \
             the authenticator answer Locked"
        );
    }

    #[test]
    fn an_account_with_no_hash_needs_no_password() {
        let (_dir, path) = db_with("users:\n- username: alice\n   uid: 1000\n");
        assert!(!offered(&path)[0].has_password);
    }

    #[test]
    fn the_display_name_falls_back_to_the_username() {
        let (_dir, path) = db_with("users:\n- username: alice\n   uid: 1000\n");
        assert_eq!(offered(&path)[0].display_name, "alice");
    }

    /// There is nothing to ask the authenticator about.
    #[test]
    fn a_record_with_no_username_is_dropped() {
        let (_dir, path) = db_with("users:\n- uid: 1000\n   password_hash: x\n");
        assert!(offered(&path).is_empty());
    }

    /// `/etc/passwd` is generated from this file, so there is no second store
    /// to fall back to and inventing a list would offer names that cannot
    /// answer.
    #[test]
    fn an_unreadable_database_offers_nobody() {
        assert!(offered(Path::new("/nonexistent/users.yaml")).is_empty());
    }

    #[test]
    fn the_most_recent_account_is_the_one_selected_first() {
        let (_dir, path) = db_with(
            "users:\n\
             - username: alice\n   uid: 1000\n   last_login_timestamp: 100\n\
             - username: bob\n   uid: 1001\n   last_login_timestamp: 500\n",
        );
        let accounts = offered(&path);
        assert_eq!(accounts[most_recent(&accounts).unwrap()].username, "bob");
    }

    /// On a fresh machine every account has never logged in. A `max_by_key`
    /// returns the *last* maximum on ties and would select the bottom row.
    #[test]
    fn with_no_logins_at_all_the_first_account_is_selected() {
        let (_dir, path) = db_with(
            "users:\n\
             - username: alice\n   uid: 1000\n\
             - username: bob\n   uid: 1001\n",
        );
        let accounts = offered(&path);
        assert_eq!(accounts[most_recent(&accounts).unwrap()].username, "alice");
    }

    #[test]
    fn an_empty_list_selects_nothing() {
        assert_eq!(most_recent(&[]), None);
    }
}
