//! Noticing a settings file being rewritten, so that every open window can be
//! told (`design-decisions.md` §1418, the operator's answer to C-Q26).
//!
//! Each program keeps its settings in `<name>.yaml` in the settings folder
//! and writes that file itself. What makes a change show at once in the
//! program's *other* windows -- and in it, when the file was edited by hand
//! or by another program -- is an announcement to every window, which the
//! compositor relays (`AnnounceSettings`, `Event::SettingsChanged`). The
//! operator asked for it "not as the same function that saves": saving is
//! the program writing its file, and this, beside it, notices the write. A
//! watcher that is down, slow or wrong can lose an announcement; it cannot
//! lose a setting.
//!
//! The desktop shell runs [`run`] on a thread for the whole session and
//! sends each name it answers to the compositor.
//!
//! # What counts as a change
//!
//! SlateOS's inotify has no `IN_CLOSE_WRITE` (the kernel has no close
//! hooks), so "the writer has finished" is read from what it does send:
//!
//! | Event | Meaning | Announced |
//! |---|---|---|
//! | `IN_MOVED_TO` | a file renamed into place -- how `settingsfile` saves | at once |
//! | `IN_DELETE`, `IN_MOVED_FROM` | the file is gone; its program falls back to its defaults | at once |
//! | `IN_MODIFY`, `IN_CREATE` | a file written in place, as an editor may | once it has been [`QUIET`] for a while |
//! | `IN_Q_OVERFLOW` | events were lost | every settings file in the folder |
//!
//! A name that is not `<settings name>.yaml` -- `settingsfile`'s own
//! `<name>.yaml.new` on the way to being renamed, an editor's swap file, a
//! directory -- is nothing to announce. [`Policy`] is that table, with no
//! system calls in it, and is tested as such; [`Watcher`] puts the folder's
//! events through it.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use libcall::inotify::{
    self, IN_CREATE, IN_DELETE, IN_DELETE_SELF, IN_IGNORED, IN_ISDIR, IN_MODIFY, IN_MOVE_SELF,
    IN_MOVED_FROM, IN_MOVED_TO, IN_ONLYDIR, IN_Q_OVERFLOW,
};
pub use settingsname::SettingsName;

/// How long a file written in place must go unwritten before it is
/// announced. Long enough to cover one save's several writes; short enough
/// that a person who saved does not wait to see it.
pub const QUIET: Duration = Duration::from_millis(250);

/// The events the folder is watched for.
pub const MASK: u32 = IN_MOVED_TO | IN_MOVED_FROM | IN_DELETE | IN_MODIFY | IN_CREATE | IN_ONLYDIR;

/// The settings name of a file in the folder: `<name>.yaml`, with a settings
/// name before the suffix.
#[must_use]
pub fn settings_name_of(file_name: &[u8]) -> Option<SettingsName> {
    SettingsName::new(file_name.strip_suffix(b".yaml")?)
}

/// What one event asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Seen {
    /// Announce this file now.
    Now(SettingsName),
    /// Announce this file once it has been quiet.
    Settling(SettingsName),
    /// Events were lost: announce every settings file there is.
    Everything,
    /// The folder itself went away, or its watch did.
    FolderGone,
    /// Nothing to announce.
    Nothing,
}

/// What an event with `mask`, about `name`, asks for.
#[must_use]
pub fn classify(mask: u32, name: &[u8]) -> Seen {
    if mask & IN_Q_OVERFLOW != 0 {
        return Seen::Everything;
    }
    if mask & (IN_DELETE_SELF | IN_MOVE_SELF | IN_IGNORED) != 0 {
        return Seen::FolderGone;
    }
    if mask & IN_ISDIR != 0 {
        return Seen::Nothing;
    }
    let Some(settings) = settings_name_of(name) else {
        return Seen::Nothing;
    };
    if mask & (IN_MOVED_TO | IN_DELETE | IN_MOVED_FROM) != 0 {
        Seen::Now(settings)
    } else if mask & (IN_MODIFY | IN_CREATE) != 0 {
        Seen::Settling(settings)
    } else {
        Seen::Nothing
    }
}

/// The table in the module documentation, and the files still settling.
#[derive(Clone, Debug)]
pub struct Policy {
    quiet: Duration,
    /// Each file written in place and not yet announced, with when it will
    /// have been quiet long enough.
    settling: Vec<(SettingsName, Instant)>,
}

/// What a [`Policy`] decided about one event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decision {
    /// Announce these, now.
    Announce(Vec<SettingsName>),
    /// Announce every settings file in the folder.
    Everything,
    /// The folder's watch is gone; watch it again.
    Rewatch,
    /// Nothing yet.
    Wait,
}

impl Policy {
    /// A policy that waits `quiet` for a file written in place.
    #[must_use]
    pub const fn new(quiet: Duration) -> Self {
        Self {
            quiet,
            settling: Vec::new(),
        }
    }

    /// Take one event, seen at `now`.
    pub fn event(&mut self, mask: u32, name: &[u8], now: Instant) -> Decision {
        match classify(mask, name) {
            Seen::Now(settings) => {
                // Announced now, so its quiet period is over too: a rename
                // after some writes in place is one change, told once.
                self.settling.retain(|(n, _)| *n != settings);
                Decision::Announce(vec![settings])
            }
            Seen::Settling(settings) => {
                let due = now.checked_add(self.quiet).unwrap_or(now);
                match self.settling.iter_mut().find(|(n, _)| *n == settings) {
                    Some(entry) => entry.1 = due,
                    None => self.settling.push((settings, due)),
                }
                Decision::Wait
            }
            Seen::Everything => {
                self.settling.clear();
                Decision::Everything
            }
            Seen::FolderGone => Decision::Rewatch,
            Seen::Nothing => Decision::Wait,
        }
    }

    /// The files that have been quiet long enough by `now`, oldest first;
    /// each is announced once.
    pub fn due(&mut self, now: Instant) -> Vec<SettingsName> {
        let mut due = Vec::new();
        self.settling.retain(|&(name, at)| {
            if at <= now {
                due.push(name);
                false
            } else {
                true
            }
        });
        due
    }

    /// When the next file settles, if one is waiting.
    #[must_use]
    pub fn next_due(&self) -> Option<Instant> {
        self.settling.iter().map(|&(_, at)| at).min()
    }
}

/// Every settings file in `dir`, in name order: what to announce after
/// events were lost. A folder that cannot be read has none.
#[must_use]
pub fn all_in(dir: &Path) -> Vec<SettingsName> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<SettingsName> = entries
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|e| settings_name_of(e.file_name().as_encoded_bytes()))
        .collect();
    names.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
    names.dedup();
    names
}

/// Why the folder cannot be watched.
#[derive(Debug)]
pub enum WatchError {
    /// This system has no inotify (`ENOSYS`): a host build, or a kernel
    /// without the watch API.
    Unsupported,
    /// The folder could not be created.
    Folder(std::io::Error),
    /// A system call failed.
    Os {
        /// Which.
        call: &'static str,
        /// Its `errno`.
        errno: i32,
    },
}

impl std::fmt::Display for WatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported => f.write_str("this system cannot watch a folder (no inotify)"),
            Self::Folder(e) => write!(f, "cannot create the settings folder: {e}"),
            Self::Os { call, errno } => write!(f, "{call} failed (errno {errno})"),
        }
    }
}

impl std::error::Error for WatchError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Folder(e) => Some(e),
            Self::Unsupported | Self::Os { .. } => None,
        }
    }
}

fn os(call: &'static str, errno: i32) -> WatchError {
    if errno == libcall::ENOSYS {
        WatchError::Unsupported
    } else {
        WatchError::Os { call, errno }
    }
}

/// The settings folder, watched.
pub struct Watcher {
    dir: PathBuf,
    inotify: inotify::Inotify,
    policy: Policy,
    buf: Vec<u8>,
}

impl Watcher {
    /// Watch `dir`, creating it first if it does not exist: a program that
    /// has not saved anything yet has not made it, and the first save must
    /// still be seen.
    ///
    /// # Errors
    ///
    /// [`WatchError::Unsupported`] where there is no inotify, else as the
    /// folder or the watch fails.
    pub fn new(dir: &Path) -> Result<Self, WatchError> {
        let inotify = inotify::Inotify::new().map_err(|e| os("inotify_init1", e))?;
        let watcher = Self {
            dir: dir.to_path_buf(),
            inotify,
            policy: Policy::new(QUIET),
            // A record with the longest name a file can have.
            buf: vec![0; inotify::HEADER.saturating_add(256).saturating_mul(16)],
        };
        watcher.watch()?;
        Ok(watcher)
    }

    fn watch(&self) -> Result<(), WatchError> {
        std::fs::create_dir_all(&self.dir).map_err(WatchError::Folder)?;
        let path = c_path(&self.dir)?;
        self.inotify
            .watch(&path, MASK)
            .map(|_| ())
            .map_err(|e| os("inotify_add_watch", e))
    }

    /// Wait up to `longest` for something to announce, answering it -- each
    /// name once, in the order seen -- or nothing when nothing did.
    ///
    /// # Errors
    ///
    /// As the watch fails, or cannot be made again after the folder went.
    pub fn next(&mut self, longest: Duration) -> Result<Vec<SettingsName>, WatchError> {
        let now = Instant::now();
        let deadline = now.checked_add(longest).unwrap_or(now);
        let wake = self
            .policy
            .next_due()
            .map_or(deadline, |due| due.min(deadline));
        let wait = wake.saturating_duration_since(now);
        let ms = i32::try_from(wait.as_millis()).unwrap_or(i32::MAX);
        let ready = self.inotify.wait(ms).map_err(|e| os("poll", e))?;
        let mut out: Vec<SettingsName> = Vec::new();
        let mut rewatch = false;
        if ready {
            let now = Instant::now();
            let events = self
                .inotify
                .read(&mut self.buf)
                .map_err(|e| os("read", e))?;
            for event in events {
                // A record cut short ends what this read can say; what it
                // would have said is found on the next change, or never.
                let Ok(event) = event else { break };
                match self.policy.event(event.mask, event.name, now) {
                    Decision::Announce(names) => out.extend(names),
                    Decision::Everything => out.extend(all_in(&self.dir)),
                    Decision::Rewatch => rewatch = true,
                    Decision::Wait => {}
                }
            }
        }
        out.extend(self.policy.due(Instant::now()));
        if rewatch {
            // The folder went: made again and watched again, and everything
            // in it announced, since nobody knows what changed meanwhile.
            self.watch()?;
            out.extend(all_in(&self.dir));
        }
        let mut seen = Vec::new();
        out.retain(|n| {
            let first = !seen.contains(n);
            seen.push(*n);
            first
        });
        Ok(out)
    }
}

#[cfg(unix)]
fn c_path(path: &Path) -> Result<std::ffi::CString, WatchError> {
    use std::os::unix::ffi::OsStrExt;
    std::ffi::CString::new(path.as_os_str().as_bytes()).map_err(|_| WatchError::Os {
        call: "the settings folder's path, which holds a NUL",
        errno: libcall::EINVAL,
    })
}

#[cfg(not(unix))]
fn c_path(_path: &Path) -> Result<std::ffi::CString, WatchError> {
    Err(WatchError::Unsupported)
}

/// Watch `dir` and hand every batch of names to `announce`, until it answers
/// `false`. For a thread of its own: it waits in the kernel between changes.
///
/// # Errors
///
/// As [`Watcher::new`] and [`Watcher::next`]; the caller logs it and goes
/// on without announcements, which costs liveness and nothing else.
pub fn run(
    dir: &Path,
    mut announce: impl FnMut(&[SettingsName]) -> bool,
) -> Result<(), WatchError> {
    let mut watcher = Watcher::new(dir)?;
    loop {
        // An hour at a time: long enough that an idle session costs nothing,
        // and short of any limit a timeout in milliseconds could reach.
        let names = watcher.next(Duration::from_hours(1))?;
        if !names.is_empty() && !announce(&names) {
            return Ok(());
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "a test that cannot set up its folder has nothing else useful to do"
)]
mod tests {
    use super::*;
    use scratchdir::ScratchDir;

    fn name(s: &str) -> SettingsName {
        SettingsName::new(s.as_bytes()).expect("a settings name")
    }

    #[test]
    fn a_settings_file_is_a_settings_name_and_yaml() {
        assert_eq!(settings_name_of(b"notes.yaml"), Some(name("notes")));
        for other in [
            &b"notes.yaml.new"[..],
            b".notes.yaml.swp",
            b"Notes.yaml",
            b"notes",
            b".yaml",
            b"notes.yml",
            b"a b.yaml",
        ] {
            assert_eq!(settings_name_of(other), None, "{other:?}");
        }
    }

    /// Every row of the table in the module documentation.
    #[test]
    fn each_event_asks_for_what_the_table_says() {
        let n = name("notes");
        for (mask, want) in [
            (IN_MOVED_TO, Seen::Now(n)),
            (IN_DELETE, Seen::Now(n)),
            (IN_MOVED_FROM, Seen::Now(n)),
            (IN_MODIFY, Seen::Settling(n)),
            (IN_CREATE, Seen::Settling(n)),
            (IN_Q_OVERFLOW, Seen::Everything),
            (IN_DELETE_SELF, Seen::FolderGone),
            (IN_MOVE_SELF, Seen::FolderGone),
            (IN_IGNORED, Seen::FolderGone),
            (IN_CREATE | IN_ISDIR, Seen::Nothing),
            (libcall::inotify::IN_ATTRIB, Seen::Nothing),
        ] {
            assert_eq!(classify(mask, b"notes.yaml"), want, "{mask:#x}");
        }
        // settingsfile's own temporary, on its way to being renamed.
        assert_eq!(classify(IN_MODIFY, b"notes.yaml.new"), Seen::Nothing);
        assert_eq!(classify(IN_MOVED_TO, b"notes.yaml.new"), Seen::Nothing);
    }

    /// A file saved the way `settingsfile` saves it -- the temporary written,
    /// then renamed over the file -- is announced once, at the rename.
    #[test]
    fn a_save_by_rename_is_announced_once_at_the_rename() {
        let mut p = Policy::new(QUIET);
        let t = Instant::now();
        assert_eq!(p.event(IN_CREATE, b"notes.yaml.new", t), Decision::Wait);
        assert_eq!(p.event(IN_MODIFY, b"notes.yaml.new", t), Decision::Wait);
        assert_eq!(
            p.event(IN_MOVED_TO, b"notes.yaml", t),
            Decision::Announce(vec![name("notes")])
        );
        assert_eq!(p.due(t + QUIET * 10), Vec::new(), "and not again later");
    }

    /// A file written in place is announced once, after its last write has
    /// gone quiet -- not after its first.
    #[test]
    fn a_file_written_in_place_is_announced_when_it_goes_quiet() {
        let mut p = Policy::new(QUIET);
        let t = Instant::now();
        p.event(IN_MODIFY, b"notes.yaml", t);
        let later = t + QUIET / 2;
        p.event(IN_MODIFY, b"notes.yaml", later);
        assert_eq!(p.next_due(), Some(later + QUIET));
        assert_eq!(
            p.due(t + QUIET),
            Vec::new(),
            "the second write restarted the wait"
        );
        assert_eq!(p.due(later + QUIET), vec![name("notes")]);
        assert_eq!(p.due(later + QUIET * 5), Vec::new(), "once");
        assert_eq!(p.next_due(), None);
    }

    /// Writes in place followed by a rename are one change, told once.
    #[test]
    fn a_rename_ends_a_files_quiet_period() {
        let mut p = Policy::new(QUIET);
        let t = Instant::now();
        p.event(IN_MODIFY, b"notes.yaml", t);
        assert_eq!(
            p.event(IN_MOVED_TO, b"notes.yaml", t),
            Decision::Announce(vec![name("notes")])
        );
        assert_eq!(p.due(t + QUIET * 2), Vec::new());
    }

    #[test]
    fn lost_events_ask_for_everything_and_a_lost_folder_for_a_new_watch() {
        let mut p = Policy::new(QUIET);
        let t = Instant::now();
        p.event(IN_MODIFY, b"notes.yaml", t);
        assert_eq!(p.event(IN_Q_OVERFLOW, b"", t), Decision::Everything);
        assert_eq!(
            p.due(t + QUIET * 2),
            Vec::new(),
            "everything covers what was settling"
        );
        assert_eq!(p.event(IN_IGNORED, b"", t), Decision::Rewatch);
    }

    #[test]
    fn everything_is_every_settings_file_in_the_folder_and_nothing_else() {
        let dir = ScratchDir::new("settingswatch-all");
        for file in [
            "notes.yaml",
            "calendar.yaml",
            "notes.yaml.new",
            "README",
            "Upper.yaml",
        ] {
            std::fs::write(dir.path(file), "a: 1\n").expect("write");
        }
        std::fs::create_dir(dir.path("folder.yaml")).expect("mkdir");
        assert_eq!(all_in(dir.dir()), vec![name("calendar"), name("notes")]);
        assert_eq!(all_in(&dir.path("missing")), Vec::new());
    }

    /// On a host with no Slate kernel the watch says so, rather than
    /// pretending to watch.
    #[cfg(not(unix))]
    #[test]
    fn without_inotify_the_watch_says_it_is_unsupported() {
        let dir = ScratchDir::new("settingswatch-host");
        assert!(matches!(
            Watcher::new(dir.dir()),
            Err(WatchError::Unsupported)
        ));
        assert!(matches!(
            run(dir.dir(), |_| true),
            Err(WatchError::Unsupported)
        ));
    }
}
