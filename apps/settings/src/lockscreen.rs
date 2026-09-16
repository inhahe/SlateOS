//! How long the session waits before locking itself.
//!
//! Writes the `session` settings group; `gui/desktop/src/idle_lock.rs` reads
//! it. The shell claims an idle watch with whatever it finds, the compositor
//! reports when the session has been that quiet, and the shell runs the lock
//! screen -- so a row on this page is a setting with an effect, which is what
//! design-decisions 856 asks of a page before it is built at all.
//!
//! # The group name is duplicated, deliberately
//!
//! [`CONFIG_NAME`] repeats the constant `gui/desktop` declares. Neither can
//! import the other's: this is a binary, and the shell is a library this
//! binary must not depend on. The same trade as `fileassoc`, and worth saying
//! again because the failure is quiet -- if the two drift, this page writes a
//! delay the shell never reads and nothing fails to compile.

/// The settings group the delay lives in. Must match `idle_lock::CONFIG_NAME`.
pub const CONFIG_NAME: &str = "session";

/// The key holding the delay, in whole minutes.
const LOCK_AFTER_MINUTES: [&str; 2] = ["lock", "after_minutes"];

/// The delays offered, in minutes. Nought is "Never".
///
/// A fixed list rather than a free number: the value is a policy the user
/// picks, not a measurement they take, and a text field would invite "5 min"
/// and "0.5" and other things this has to then refuse. Same reasoning as the
/// display-scale and cursor-size pickers on the pages beside this one.
pub const CHOICES: [u32; 7] = [0, 1, 5, 10, 15, 30, 60];

/// How a delay is written on the row.
#[must_use]
pub fn label(minutes: u32) -> String {
    match minutes {
        0 => "Never".to_string(),
        1 => "After 1 minute".to_string(),
        n => format!("After {n} minutes"),
    }
}

/// The delay currently stored, in minutes. Nought for never.
///
/// Anything unreadable, negative or absent reads as nought, for the reason
/// `idle_lock::lock_after` gives: all of them mean "do not lock", and a
/// settings page that refused to open because a file held `"soon"` would be
/// worse than one that shows "Never".
#[must_use]
pub fn stored_minutes() -> u32 {
    let doc = settingsfile::load(CONFIG_NAME);
    doc.get_i64(&LOCK_AFTER_MINUTES)
        .and_then(|n| u32::try_from(n).ok())
        .unwrap_or(0)
}

/// Store `minutes`, returning whether the file was written.
///
/// # Errors
///
/// The `io::Error` from writing the settings file.
pub fn store_minutes(minutes: u32) -> std::io::Result<()> {
    let mut doc = settingsfile::load(CONFIG_NAME);
    doc.set_i64(&LOCK_AFTER_MINUTES, i64::from(minutes));
    settingsfile::store(CONFIG_NAME, &doc)
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

    /// What is written is what is read back.
    #[test]
    fn a_delay_round_trips() {
        settingsfile::testing::with_scratch_config("settings-lock-roundtrip", |_root| {
            store_minutes(15).expect("the scratch config is writable");
            assert_eq!(stored_minutes(), 15);
        });
    }

    /// Never is stored as nought, not as an absent key.
    ///
    /// Absent and nought both read as "Never" here, but they are not the same
    /// to the shell on its first run: an absent key is a machine nobody has
    /// configured, and writing the choice makes "Never" something the user
    /// said rather than something nobody said.
    #[test]
    fn never_is_written_not_omitted() {
        settingsfile::testing::with_scratch_config("settings-lock-never", |_root| {
            store_minutes(0).expect("the scratch config is writable");
            let doc = settingsfile::load(CONFIG_NAME);
            assert_eq!(doc.get_i64(&LOCK_AFTER_MINUTES), Some(0));
        });
    }

    /// An unset machine reads as Never rather than panicking.
    #[test]
    fn an_unset_machine_reads_as_never() {
        settingsfile::testing::with_scratch_config("settings-lock-unset", |_root| {
            assert_eq!(stored_minutes(), 0);
        });
    }

    /// Every offered delay survives the round trip.
    ///
    /// The list and the store are separate things, and a choice the page
    /// offers that the file cannot hold would be a row that silently does
    /// nothing.
    #[test]
    fn every_offered_choice_round_trips() {
        settingsfile::testing::with_scratch_config("settings-lock-choices", |_root| {
            for minutes in CHOICES {
                store_minutes(minutes).expect("the scratch config is writable");
                assert_eq!(stored_minutes(), minutes, "{minutes} did not survive");
            }
        });
    }

    /// The labels say what the numbers mean.
    #[test]
    fn labels_read_as_english() {
        assert_eq!(label(0), "Never");
        assert_eq!(label(1), "After 1 minute");
        assert_eq!(label(15), "After 15 minutes");
    }
}
