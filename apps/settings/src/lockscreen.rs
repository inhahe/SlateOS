//! The lock screen program's own settings: what its clock shows.
//!
//! When the session locks itself is the `session` group's, beside the
//! displays' and the computer's sleep: see [`crate::idle`].

/// The lock screen program's own settings file, `lockscreen.yaml` -- one file
/// per program (C-Q26, option A; `design-decisions.md` §1418). Repeated in
/// `apps/lockscreen`, which reads it: neither program can depend on the
/// other, so if the two drift this page writes a file nothing reads and
/// nothing fails to compile.
pub const CLOCK_CONFIG: &str = "lockscreen";

/// Whether the lock screen's clock shows seconds.
const CLOCK_SECONDS: [&str; 2] = ["clock", "seconds"];
/// Whether it shows the date under the clock.
const CLOCK_DATE: [&str; 2] = ["clock", "date"];

/// What the lock screen's clock shows: (seconds, date). Unset is the lock
/// screen's own default -- no seconds, the date shown.
#[must_use]
pub fn stored_clock() -> (bool, bool) {
    let doc = settingsfile::load(CLOCK_CONFIG);
    (
        doc.get_bool(&CLOCK_SECONDS).unwrap_or(false),
        doc.get_bool(&CLOCK_DATE).unwrap_or(true),
    )
}

/// Store what the lock screen's clock shows.
///
/// # Errors
///
/// The `io::Error` from writing the settings file.
pub fn store_clock(seconds: bool, date: bool) -> std::io::Result<()> {
    let mut doc = settingsfile::load(CLOCK_CONFIG);
    doc.set_bool(&CLOCK_SECONDS, seconds);
    doc.set_bool(&CLOCK_DATE, date);
    settingsfile::store(CLOCK_CONFIG, &doc)
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    reason = "a scratch config that cannot be written should fail the test loudly"
)]
mod tests {
    use super::*;

    /// What the clock shows is kept, and unset is the lock screen's default.
    #[test]
    fn the_clock_choices_round_trip() {
        settingsfile::testing::with_scratch_config("settings-lock-clock", |_root| {
            assert_eq!(stored_clock(), (false, true), "unset is not the default");
            store_clock(true, false).expect("the scratch config is writable");
            assert_eq!(stored_clock(), (true, false));
        });
    }
}
