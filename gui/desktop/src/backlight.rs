//! The screen's brightness as the kernel reports it, for the notification
//! pane's brightness row.
//!
//! # Read, and not set
//!
//! The kernel keeps each display's brightness (`kernel/src/fs/brightness.rs`)
//! and reports it in `/proc/brightness`, one row a display under
//! `Displays:`. Setting it is `SYS_BRIGHTNESS_SET`, which lane A built
//! (`requests/c-a-brightness-has-setters-and-no-door.md`) behind a right --
//! `SET_BRIGHTNESS` -- that nothing gives the desktop yet
//! (`known-issues/TD-C-THE-DESKTOP-CANNOT-SET-THE-BRIGHTNESS-IT-SHOWS.md`).
//! So the pane shows the level the screen is at and says it cannot be
//! changed -- rather than a slider that moves a number and nothing on the
//! screen, as the volume's did (design-decisions §1485). Setting it goes
//! here, once the desktop holds the right.
//!
//! # Where no screen was asked for
//!
//! A test, a harness, a picture of a theme ask for none ([`Source::Own`]),
//! and the slider is the pane's own number, as before. The `desktop` binary
//! attaches the kernel's report ([`Source::kernel`]).

use std::path::PathBuf;

/// Where the kernel reports the displays' brightness.
pub const REPORT_PATH: &str = "/proc/brightness";

/// What the pane says in the slider's place with the level read: no program
/// can set it yet.
pub const CANNOT_SET: &str = "Can't be changed yet";

/// What it says with no level to read: no report, or no display in it.
pub const NO_REPORT: &str = "No brightness control reachable";

/// One display's row of the report.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Display {
    /// The kernel's number for it.
    pub id: u32,
    /// Its name, as the kernel gives it.
    pub name: String,
    /// Its brightness, 0 to 100.
    pub level: u8,
    /// The least it may be set to.
    pub min: u8,
    /// How it is being set -- by hand, or by the light around it.
    pub mode: String,
}

/// The displays a report lists, in its order: the rows under `Displays:`,
/// each `id name level% min min% [mode]` -- a name may hold spaces, so a row
/// is read from both ends. A row that does not read so is passed over.
#[must_use]
pub fn parse(report: &str) -> Vec<Display> {
    report
        .lines()
        .skip_while(|line| line.trim() != "Displays:")
        .skip(1)
        .take_while(|line| line.starts_with(' ') || line.starts_with('\t'))
        .filter_map(row)
        .collect()
}

/// One row: `id name level% min min% [mode]`.
fn row(line: &str) -> Option<Display> {
    let line = line.trim();
    let open = line.rfind('[')?;
    let mode = line.get(open.checked_add(1)?..)?.strip_suffix(']')?;
    let rest = line.get(..open)?.trim_end();
    let (rest, min) = rest.rsplit_once(char::is_whitespace)?;
    let min = percent(min)?;
    let rest = rest.trim_end().strip_suffix("min")?.trim_end();
    let (rest, level) = rest.rsplit_once(char::is_whitespace)?;
    let level = percent(level)?;
    let (id, name) = rest.trim().split_once(char::is_whitespace)?;
    Some(Display {
        id: id.parse().ok()?,
        name: name.trim().to_owned(),
        level,
        min,
        mode: mode.to_owned(),
    })
}

/// `NN%` as a level, 0 to 100.
fn percent(word: &str) -> Option<u8> {
    word.strip_suffix('%')?.parse().ok().filter(|&n| n <= 100)
}

/// Where the pane's brightness comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    /// No screen was asked for: the slider is the pane's own number.
    Own,
    /// The kernel's report, read from this file.
    Report(PathBuf),
}

impl Source {
    /// The kernel's report: what the `desktop` binary attaches.
    #[must_use]
    pub fn kernel() -> Self {
        Self::Report(PathBuf::from(REPORT_PATH))
    }

    /// The first display's level, read now, and why it cannot be changed
    /// from the pane -- or no level, and why, where the report cannot be
    /// read or lists no display. `None` where no screen was asked for.
    #[must_use]
    pub fn read(&self) -> Option<(Option<u8>, &'static str)> {
        let Self::Report(path) = self else {
            return None;
        };
        // A report that cannot be read is a screen out of reach, shown as
        // such: not an error to stop the pane for.
        let level = std::fs::read(path)
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .and_then(|text| parse(&text).first().map(|display| display.level));
        Some(match level {
            Some(level) => (Some(level), CANNOT_SET),
            None => (None, NO_REPORT),
        })
    }
}

#[cfg(test)]
#[path = "backlight_tests.rs"]
mod tests;
