//! The screen's brightness as the kernel reports it and sets it, for the
//! notification pane's brightness row.
//!
//! # Read, and set where the desktop may
//!
//! The kernel keeps each display's brightness (`kernel/src/fs/brightness.rs`)
//! and reports it in `/proc/brightness`, one row a display under
//! `Displays:`. Setting it is `SYS_BRIGHTNESS_SET` (lane A,
//! `requests/c-a-brightness-has-setters-and-no-door.md`), for a process
//! holding the `SET_BRIGHTNESS` right -- which nothing gives the desktop yet
//! (`known-issues/TD-C-THE-DESKTOP-CANNOT-SET-THE-BRIGHTNESS-IT-SHOWS.md`).
//!
//! So each time the pane opens, the first display's level is read and set
//! to itself: a setting no one can see, which answers whether the desktop
//! may. Where it may, the pane's slider is the screen's; where it may not,
//! the pane shows the level and says it cannot be changed -- rather than a
//! slider that moves a number and nothing on the screen, as the volume's did
//! (design-decisions §1485). The day the right is given, the slider comes
//! back with no change here.
//!
//! # Where no screen was asked for
//!
//! A test, a harness, a picture of a theme ask for none
//! ([`Backlight::own`]), and the slider is the pane's own number, as before.
//! The `desktop` binary attaches the kernel's ([`Backlight::kernel`]).

use std::path::PathBuf;

/// Where the kernel reports the displays' brightness.
pub const REPORT_PATH: &str = "/proc/brightness";

/// What the pane says in the slider's place with the level read and the
/// desktop not allowed to set it.
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

/// Setting a display's brightness.
pub trait Setter {
    /// Set display `display` to `level`, 0 to 100.
    ///
    /// # Errors
    ///
    /// The kernel's errno: `EPERM` without the right, `ENOSYS` where there
    /// is no such call.
    fn set(&mut self, display: u32, level: u8) -> Result<(), i32>;
}

/// The kernel's `SYS_BRIGHTNESS_SET`, on SlateOS; `ENOSYS` anywhere else.
#[derive(Clone, Copy, Debug, Default)]
pub struct KernelSetter;

/// `ENOSYS`: no such call here.
const ENOSYS: i32 = 38;

impl Setter for KernelSetter {
    #[cfg(all(target_os = "linux", target_vendor = "slateos"))]
    fn set(&mut self, display: u32, level: u8) -> Result<(), i32> {
        sys::brightness_set(display, level)
    }

    #[cfg(not(all(target_os = "linux", target_vendor = "slateos")))]
    fn set(&mut self, _display: u32, _level: u8) -> Result<(), i32> {
        Err(ENOSYS)
    }
}

/// The system call, on SlateOS.
#[cfg(all(target_os = "linux", target_vendor = "slateos"))]
mod sys {
    use std::arch::asm;

    /// `SYS_BRIGHTNESS_SET` (`kernel/src/syscall/number.rs`), as the `rax`
    /// it goes in and comes back out of.
    const SYS_BRIGHTNESS_SET: i64 = 1075;

    /// `SYS_BRIGHTNESS_SET(display, level)`.
    pub fn brightness_set(display: u32, level: u8) -> Result<(), i32> {
        let ret: i64;
        // SAFETY: the `syscall` instruction clobbers `rcx` and `r11`, declared
        // below, and returns in `rax`; the arguments go in `rdi` and `rsi`,
        // the convention this system's calls share with Linux's (as
        // `gui/remote`'s channel makes its own). Both are plain integers --
        // a display's number and a level -- so no memory is lent the kernel.
        unsafe {
            asm!(
                "syscall",
                inlateout("rax") SYS_BRIGHTNESS_SET => ret,
                in("rdi") u64::from(display),
                in("rsi") u64::from(level),
                lateout("rcx") _,
                lateout("r11") _,
                options(nostack),
            );
        }
        if (-4095..0).contains(&ret) {
            Err(ret
                .checked_neg()
                .and_then(|e| i32::try_from(e).ok())
                .unwrap_or(super::ENOSYS))
        } else {
            Ok(())
        }
    }
}

/// What a reading of the screen's brightness found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reading {
    /// No screen was asked for: the slider is the pane's own number.
    Own,
    /// The level, which the desktop may set.
    Live(u8),
    /// The level if there is one to read, and why the pane cannot change it.
    Fixed(Option<u8>, &'static str),
}

/// The screen's brightness, as the desktop reads and sets it.
pub struct Backlight {
    /// The kernel's report -- `None` where no screen was asked for.
    report: Option<PathBuf>,
    /// What sets it.
    setter: Box<dyn Setter>,
    /// The display last read, and the level it was last read or set at.
    known: Option<(u32, u8)>,
    /// Whether the last setting was accepted.
    settable: bool,
}

impl std::fmt::Debug for Backlight {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Backlight")
            .field("report", &self.report)
            .field("known", &self.known)
            .field("settable", &self.settable)
            .finish_non_exhaustive()
    }
}

impl Backlight {
    /// No screen: the pane's own number, as a test must have.
    #[must_use]
    pub fn own() -> Self {
        Self {
            report: None,
            setter: Box::new(KernelSetter),
            known: None,
            settable: false,
        }
    }

    /// The kernel's report and its call: what the `desktop` binary attaches.
    #[must_use]
    pub fn kernel() -> Self {
        Self::with(PathBuf::from(REPORT_PATH), Box::new(KernelSetter))
    }

    /// The report at `report`, set through `setter`.
    #[must_use]
    pub fn with(report: PathBuf, setter: Box<dyn Setter>) -> Self {
        Self {
            report: Some(report),
            setter,
            known: None,
            settable: false,
        }
    }

    /// The first display's level, read now -- and set to itself, which no
    /// one sees and which answers whether the desktop may set it.
    pub fn read(&mut self) -> Reading {
        let Some(path) = &self.report else {
            return Reading::Own;
        };
        // A report that cannot be read is a screen out of reach, shown as
        // such: not an error to stop the pane for.
        let first = std::fs::read(path)
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .and_then(|text| parse(&text).into_iter().next());
        let Some(display) = first else {
            self.known = None;
            self.settable = false;
            return Reading::Fixed(None, NO_REPORT);
        };
        self.known = Some((display.id, display.level));
        self.settable = self.setter.set(display.id, display.level).is_ok();
        if self.settable {
            Reading::Live(display.level)
        } else {
            Reading::Fixed(Some(display.level), CANNOT_SET)
        }
    }

    /// Whether the pane's slider is the screen's now: a display was read and
    /// the desktop may set it.
    #[must_use]
    pub const fn is_live(&self) -> bool {
        self.settable && self.known.is_some()
    }

    /// Set the display last read to `level`, if it differs from what it was
    /// last known to be. A refusal makes it fixed from then on, until the
    /// next reading.
    ///
    /// # Errors
    ///
    /// Why the pane cannot change it.
    pub fn write(&mut self, level: u8) -> Result<(), &'static str> {
        let Some((display, was)) = self.known.filter(|_| self.settable) else {
            return Err(CANNOT_SET);
        };
        if was == level {
            return Ok(());
        }
        if self.setter.set(display, level).is_ok() {
            self.known = Some((display, level));
            Ok(())
        } else {
            self.settable = false;
            Err(CANNOT_SET)
        }
    }
}

#[cfg(test)]
#[path = "backlight_tests.rs"]
pub(crate) mod tests;
