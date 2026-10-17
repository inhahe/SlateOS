//! What the session does once nobody has touched it for a while: how long
//! before it locks itself, before the displays go to sleep, and before the
//! computer does -- and whether waking the displays locks it.
//!
//! Writes the `session` settings group; `gui/desktop/src/idle_lock.rs` reads
//! it. The shell hands each delay to the compositor's idle watch, the
//! compositor says when the session has been that quiet, and the shell locks,
//! puts the displays to sleep as "Sleep the display" does, or sleeps the
//! computer as the power menu's Sleep does -- so a row for each is a setting
//! with an effect, which is what design-decisions 856 asks of a page before
//! it is built at all.
//!
//! Each choice is written on its own, key by key, so choosing one delay does
//! not write the others: a key nobody chose stays absent, which the shell
//! reads as the default, and "Never" chosen is written as nought.
//!
//! # The group name is duplicated, deliberately
//!
//! [`CONFIG_NAME`] repeats the constant `gui/desktop` declares, and the keys
//! repeat its keys. Neither can import the other's: this is a binary, and the
//! shell is a library this binary must not depend on. The same trade as
//! `fileassoc`, and worth saying again because the failure is quiet -- if the
//! two drift, this page writes a delay the shell never reads and nothing fails
//! to compile.

/// The settings group the delays live in. Must match `idle_lock::CONFIG_NAME`.
pub const CONFIG_NAME: &str = "session";

/// The key saying whether waking the displays locks the session. Must match
/// `idle_lock::LOCK_ON_DISPLAY_WAKE`.
const LOCK_ON_DISPLAY_WAKE: [&str; 2] = ["lock", "on_display_wake"];

/// One of the three things the session does after a quiet spell.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Delay {
    /// The session locks itself.
    Lock,
    /// The displays go to sleep.
    DisplaysOff,
    /// The computer goes to sleep.
    Sleep,
}

impl Delay {
    /// Every delay, in the order the pages show them.
    pub const ALL: [Self; 3] = [Self::Lock, Self::DisplaysOff, Self::Sleep];

    /// The key holding the delay, in whole minutes. Each must match the
    /// shell's (`idle_lock::LOCK_AFTER_MINUTES`, `DISPLAYS_OFF_AFTER_MINUTES`,
    /// `SLEEP_AFTER_MINUTES`).
    const fn key(self) -> [&'static str; 2] {
        match self {
            Self::Lock => ["lock", "after_minutes"],
            Self::DisplaysOff => ["displays", "off_after_minutes"],
            Self::Sleep => ["sleep", "after_minutes"],
        }
    }
}

/// The delays offered, in minutes. Nought is "Never".
///
/// A fixed list rather than a free number: the value is a policy the user
/// picks, not a measurement they take, and a text field would invite "5 min"
/// and "0.5" and other things this has to then refuse. Same reasoning as the
/// display-scale and cursor-size pickers on the pages beside this one. The
/// hours are for the computer's sleep above all, which is usually set longer
/// than the screen's.
pub const CHOICES: [u32; 9] = [0, 1, 5, 10, 15, 30, 60, 120, 180];

/// A delay of `minutes`, as words: "1 minute", "15 minutes", "1 hour",
/// "2 hours".
#[must_use]
pub fn duration(minutes: u32) -> String {
    match minutes {
        1 => "1 minute".to_string(),
        60 => "1 hour".to_string(),
        n if n > 60 && n % 60 == 0 => format!("{} hours", n / 60),
        n => format!("{n} minutes"),
    }
}

/// How a delay is written on its row: "Never" for nought, else "After" and
/// its [`duration`].
#[must_use]
pub fn label(minutes: u32) -> String {
    if minutes == 0 {
        "Never".to_string()
    } else {
        format!("After {}", duration(minutes))
    }
}

/// What the `session` group holds: the three delays in minutes, nought for
/// never, and whether waking the displays locks the session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Idle {
    lock: u32,
    displays_off: u32,
    sleep: u32,
    /// Whether waking the displays from a sleep the user put them in locks
    /// the session.
    pub lock_on_display_wake: bool,
}

impl Default for Idle {
    /// An unset machine: no delay, and waking the displays locks -- the
    /// shell's defaults.
    fn default() -> Self {
        Self {
            lock: 0,
            displays_off: 0,
            sleep: 0,
            lock_on_display_wake: true,
        }
    }
}

impl Idle {
    /// What is stored now.
    ///
    /// A delay unreadable, negative or absent reads as nought, for the reason
    /// `idle_lock::lock_after` gives: all of them mean "never", and a settings
    /// page that refused to open because a file held `"soon"` would be worse
    /// than one that shows "Never". The switch reads as on unless the file
    /// says `false`, as the shell reads it.
    #[must_use]
    pub fn stored() -> Self {
        let doc = settingsfile::load(CONFIG_NAME);
        let minutes = |delay: Delay| {
            doc.get_i64(&delay.key())
                .and_then(|n| u32::try_from(n).ok())
                .unwrap_or(0)
        };
        Self {
            lock: minutes(Delay::Lock),
            displays_off: minutes(Delay::DisplaysOff),
            sleep: minutes(Delay::Sleep),
            lock_on_display_wake: doc.get_bool(&LOCK_ON_DISPLAY_WAKE).unwrap_or(true),
        }
    }

    /// The minutes before `delay`; nought for never.
    #[must_use]
    pub const fn minutes(&self, delay: Delay) -> u32 {
        match delay {
            Delay::Lock => self.lock,
            Delay::DisplaysOff => self.displays_off,
            Delay::Sleep => self.sleep,
        }
    }

    /// Set the minutes before `delay`.
    pub const fn set_minutes(&mut self, delay: Delay, minutes: u32) {
        match delay {
            Delay::Lock => self.lock = minutes,
            Delay::DisplaysOff => self.displays_off = minutes,
            Delay::Sleep => self.sleep = minutes,
        }
    }

    /// Write what differs from `before`, key by key, leaving every other key
    /// as the file has it.
    ///
    /// # Errors
    ///
    /// The `io::Error` from writing the settings file.
    pub fn store_changes(&self, before: &Self) -> std::io::Result<()> {
        if self == before {
            return Ok(());
        }
        let mut doc = settingsfile::load(CONFIG_NAME);
        for delay in Delay::ALL {
            if self.minutes(delay) != before.minutes(delay) {
                doc.set_i64(&delay.key(), i64::from(self.minutes(delay)));
            }
        }
        if self.lock_on_display_wake != before.lock_on_display_wake {
            doc.set_bool(&LOCK_ON_DISPLAY_WAKE, self.lock_on_display_wake);
        }
        settingsfile::store(CONFIG_NAME, &doc)
    }
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

    /// Each delay, and the switch, is written where the shell reads it and
    /// read back as written.
    #[test]
    fn each_delay_round_trips_at_the_shells_key() {
        settingsfile::testing::with_scratch_config("settings-idle-roundtrip", |_root| {
            let before = Idle::stored();
            let mut idle = before;
            idle.set_minutes(Delay::Lock, 15);
            idle.set_minutes(Delay::DisplaysOff, 5);
            idle.set_minutes(Delay::Sleep, 120);
            idle.lock_on_display_wake = false;
            idle.store_changes(&before)
                .expect("the scratch config is writable");
            assert_eq!(Idle::stored(), idle);
            let doc = settingsfile::load(CONFIG_NAME);
            assert_eq!(doc.get_i64(&["lock", "after_minutes"]), Some(15));
            assert_eq!(doc.get_i64(&["displays", "off_after_minutes"]), Some(5));
            assert_eq!(doc.get_i64(&["sleep", "after_minutes"]), Some(120));
            assert_eq!(doc.get_bool(&["lock", "on_display_wake"]), Some(false));
        });
    }

    /// Choosing one delay writes that one alone: the others stay absent,
    /// which is the shell's default rather than a "Never" nobody said.
    #[test]
    fn a_choice_writes_its_own_key_alone() {
        settingsfile::testing::with_scratch_config("settings-idle-alone", |_root| {
            let before = Idle::stored();
            let mut idle = before;
            idle.set_minutes(Delay::DisplaysOff, 10);
            idle.store_changes(&before)
                .expect("the scratch config is writable");
            let doc = settingsfile::load(CONFIG_NAME);
            assert_eq!(doc.get_i64(&["displays", "off_after_minutes"]), Some(10));
            assert_eq!(doc.get_i64(&["lock", "after_minutes"]), None);
            assert_eq!(doc.get_i64(&["sleep", "after_minutes"]), None);
            assert_eq!(doc.get_bool(&["lock", "on_display_wake"]), None);
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
        settingsfile::testing::with_scratch_config("settings-idle-never", |_root| {
            let mut before = Idle::stored();
            before.set_minutes(Delay::Lock, 5);
            before
                .store_changes(&Idle::stored())
                .expect("the scratch config is writable");
            let mut idle = before;
            idle.set_minutes(Delay::Lock, 0);
            idle.store_changes(&before)
                .expect("the scratch config is writable");
            let doc = settingsfile::load(CONFIG_NAME);
            assert_eq!(doc.get_i64(&["lock", "after_minutes"]), Some(0));
        });
    }

    /// An unset machine reads as no delay, and waking the displays locking.
    #[test]
    fn an_unset_machine_reads_as_the_shells_defaults() {
        settingsfile::testing::with_scratch_config("settings-idle-unset", |_root| {
            assert_eq!(Idle::stored(), Idle::default());
            assert!(Idle::default().lock_on_display_wake);
        });
    }

    /// Every offered delay survives the round trip, for each of the three.
    ///
    /// The list and the store are separate things, and a choice the page
    /// offers that the file cannot hold would be a row that silently does
    /// nothing.
    #[test]
    fn every_offered_choice_round_trips() {
        settingsfile::testing::with_scratch_config("settings-idle-choices", |_root| {
            for delay in Delay::ALL {
                for minutes in CHOICES {
                    let before = Idle::stored();
                    let mut idle = before;
                    idle.set_minutes(delay, minutes);
                    idle.store_changes(&before)
                        .expect("the scratch config is writable");
                    assert_eq!(
                        Idle::stored().minutes(delay),
                        minutes,
                        "{delay:?}: {minutes} did not survive"
                    );
                }
            }
        });
    }

    /// The labels say what the numbers mean.
    #[test]
    fn labels_read_as_english() {
        assert_eq!(label(0), "Never");
        assert_eq!(label(1), "After 1 minute");
        assert_eq!(label(15), "After 15 minutes");
        assert_eq!(label(60), "After 1 hour");
        assert_eq!(label(120), "After 2 hours");
        assert_eq!(duration(90), "90 minutes");
    }
}
