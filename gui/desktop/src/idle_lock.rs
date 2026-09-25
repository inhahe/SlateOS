//! How long the session waits before locking itself.
//!
//! One integer, read from the `session` settings group. The Settings
//! application writes it; this reads it and hands it to
//! `EventLoop::watch_idle`, after which the compositor says when the delay has
//! passed and [`crate::session`] queues the lock.
//!
//! # Why a settings group and not a crate
//!
//! `appearance` and `inputsettings` are crates because several processes share
//! a large *typed* model and two copies of it would drift. This is one number
//! with two readers, which is the shape `fileassoc` already has: the group is
//! named by a constant on each side and nothing is shared but the name. A
//! crate for one integer would be ceremony, and the cost of the lighter form
//! is stated plainly below.
//!
//! # Why not `gui/desktop/src/power_settings.rs`
//!
//! Because it models exactly this -- screen-off and sleep timeouts in minutes,
//! "0 = never" -- and nothing opens it, nothing persists it and nothing
//! honours it. Putting a real delay beside those would inherit a setting that
//! changes nothing. See `known-issues.md`
//! `TD-C-THREE-MORE-SHELL-SETTINGS-MODULES-ARE-REACHED-BY-NOTHING`.

use std::time::Duration;

/// The settings group the delay lives in.
///
/// Repeated in `apps/settings`, which writes it. Neither side can import the
/// other's constant -- the Settings application is a binary -- so the two
/// agree by convention, and if they ever drift the page will write a delay the
/// shell never reads and nothing will fail to compile. The same trade, and the
/// same warning, as `fileassoc`.
pub const CONFIG_NAME: &str = "session";

/// The key holding the delay, in whole minutes.
const LOCK_AFTER_MINUTES: [&str; 2] = ["lock", "after_minutes"];

/// Minutes before the session locks itself, or `None` for never.
///
/// `None` for absent, unreadable, zero and anything that is not a number. All
/// four mean "do not lock", and the distinction between them is not one this
/// caller can act on: a shell that refused to start because a settings file
/// held `"soon"` would be worse than one that declines to lock.
///
/// Zero is "never" rather than "immediately" for the same reason it is in
/// `power_settings`' timeouts and in `RequestBody::WatchIdle`: a delay of
/// nought as "lock at once" would make a mistyped setting unrecoverable
/// without another machine.
#[must_use]
pub fn lock_after() -> Option<Duration> {
    let doc = settingsfile::load(CONFIG_NAME);
    let minutes = doc.get_i64(&LOCK_AFTER_MINUTES)?;
    let minutes = u64::try_from(minutes).ok()?;
    if minutes == 0 {
        return None;
    }
    Some(Duration::from_secs(minutes.saturating_mul(60)))
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "a test that indexes out of range should fail loudly and point at the line that did it"
)]
mod tests {
    use super::*;

    /// Written and read back as minutes.
    #[test]
    fn a_delay_is_read_as_minutes() {
        settingsfile::testing::with_scratch_config("idle-lock-read", |_root| {
            let mut doc = settingsfile::load(CONFIG_NAME);
            doc.set_i64(&LOCK_AFTER_MINUTES, 15);
            settingsfile::store(CONFIG_NAME, &doc).expect("the scratch config is writable");
            assert_eq!(lock_after(), Some(Duration::from_mins(15)));
        });
    }

    /// Nought is never, not at once.
    ///
    /// The whole reason to pin it: read the other way, a mistyped setting
    /// locks the screen the instant it is saved, and the user cannot reach the
    /// setting that did it.
    #[test]
    fn nought_means_never() {
        settingsfile::testing::with_scratch_config("idle-lock-zero", |_root| {
            let mut doc = settingsfile::load(CONFIG_NAME);
            doc.set_i64(&LOCK_AFTER_MINUTES, 0);
            settingsfile::store(CONFIG_NAME, &doc).expect("the scratch config is writable");
            assert_eq!(lock_after(), None);
        });
    }

    /// An absent setting is never, and does not panic.
    #[test]
    fn an_absent_setting_is_never() {
        settingsfile::testing::with_scratch_config("idle-lock-absent", |_root| {
            assert_eq!(lock_after(), None);
        });
    }

    /// A negative delay is never, rather than an enormous one.
    ///
    /// `u64::try_from` on a negative `i64` fails, and the failure has to mean
    /// "do not lock" -- a wrapping conversion would give roughly six hundred
    /// million years, which reads as a bug nobody can explain rather than as a
    /// setting nobody honours.
    #[test]
    fn a_negative_delay_is_never() {
        settingsfile::testing::with_scratch_config("idle-lock-negative", |_root| {
            let mut doc = settingsfile::load(CONFIG_NAME);
            doc.set_i64(&LOCK_AFTER_MINUTES, -5);
            settingsfile::store(CONFIG_NAME, &doc).expect("the scratch config is writable");
            assert_eq!(lock_after(), None);
        });
    }
}
