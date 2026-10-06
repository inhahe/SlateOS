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
//!
//! # What a group's settings name
//!
//! Some settings name files kept elsewhere: `appearance.yaml` chooses its
//! themes by name, and a theme is a folder of files in the user's data
//! directory; it shows a picture from wherever the user keeps one. Editing
//! either changes what the settings give without changing a byte of the
//! settings file, so watching the settings folder alone would never tell
//! anyone. A group may therefore name paths to be followed with it
//! ([`Dependents`]) -- a folder whole, or a file by its name in its folder --
//! and any change there -- renamed in, out or away, deleted, or written in
//! place and gone quiet; a followed folder itself going -- is announced as a
//! change to the group. Each process then compares what it reads
//! (`settingsfile::Watcher::with_dependencies`), so a change that alters
//! nothing it uses costs it a look and no more. The paths are asked for again
//! each time the group is announced, which is when they can change; when
//! they do, the watch is made afresh -- the old one read out first, so
//! nothing seen in between is lost.

use std::ffi::OsString;
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
                self.settle(settings, now);
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

    /// Take one event, seen at `now`, from a folder the group `group`
    /// depends on ([`Dependents`]), about a name the group follows there --
    /// any name in a folder followed whole (a theme's file, its pictures,
    /// the folders in it), or the one a picture is followed by. The folder
    /// is followed for what it holds, not for settings files: anything
    /// renamed in, out or away, or deleted, announces the group at once, the
    /// folder itself going included; anything written in place, once it has
    /// gone quiet.
    pub fn dependent_event(&mut self, mask: u32, group: SettingsName, now: Instant) -> Decision {
        if mask & IN_Q_OVERFLOW != 0 {
            self.settling.clear();
            return Decision::Everything;
        }
        let at_once =
            IN_MOVED_TO | IN_MOVED_FROM | IN_DELETE | IN_DELETE_SELF | IN_MOVE_SELF | IN_IGNORED;
        if mask & at_once != 0 {
            self.settling.retain(|(n, _)| *n != group);
            Decision::Announce(vec![group])
        } else if mask & (IN_MODIFY | IN_CREATE) != 0 {
            self.settle(group, now);
            Decision::Wait
        } else {
            Decision::Wait
        }
    }

    /// Wait for `name` to be quiet from `now` before announcing it: a write
    /// after another restarts the wait.
    fn settle(&mut self, name: SettingsName, now: Instant) {
        let due = now.checked_add(self.quiet).unwrap_or(now);
        match self.settling.iter_mut().find(|(n, _)| *n == name) {
            Some(entry) => entry.1 = due,
            None => self.settling.push((name, due)),
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

/// What a settings group's settings name besides themselves, followed with
/// the settings folder and announced as the group -- the themes
/// `appearance.yaml` chooses, the pictures it shows -- so that editing one
/// reaches every window as an edit of the settings would. See the module
/// documentation.
#[derive(Clone, Copy, Debug)]
pub struct Dependents {
    /// The group the paths belong to.
    pub name: SettingsName,
    /// The paths to follow, as the group's settings stand now. A folder is
    /// followed whole: anything in it renamed in, out or away, deleted, or
    /// written. Any other path -- a picture, which may sit in a folder of
    /// thousands -- is followed by its name in the folder holding it, so its
    /// neighbours changing costs nothing; it need not be there yet, but its
    /// folder must. Asked when watching starts and again each time the group
    /// is announced, by its own file or by one of these, since that is when
    /// the answer can change.
    pub paths: fn() -> Vec<PathBuf>,
}

/// A folder followed for a group: its watch, the group, and the names in it
/// that count -- `None` for all of them.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Followed {
    wd: i32,
    group: SettingsName,
    names: Option<Vec<OsString>>,
}

impl Followed {
    /// Whether an event about `name` in the folder counts: one about the
    /// folder itself, which has no name, always does.
    fn counts(&self, name: &[u8]) -> bool {
        name.is_empty()
            || self.names.as_ref().is_none_or(|names| {
                names
                    .iter()
                    .any(|followed| followed.as_encoded_bytes() == name)
            })
    }
}

/// The folders to watch for `paths` -- each whole, or for the names in it --
/// as [`Dependents::paths`] says: a folder named whole and by a name in it is
/// watched whole, and two names in one folder are one watch.
fn folders_for(paths: &[PathBuf]) -> Vec<(PathBuf, Option<Vec<OsString>>)> {
    let mut folders: Vec<(PathBuf, Option<Vec<OsString>>)> = Vec::new();
    for path in paths {
        let (folder, name) = if path.is_dir() {
            (path.clone(), None)
        } else {
            match (path.parent(), path.file_name()) {
                (Some(parent), Some(name)) if parent.is_dir() => {
                    (parent.to_path_buf(), Some(name.to_os_string()))
                }
                // With no folder to hold it there is nothing to watch: the
                // group is listed again when it is next announced.
                _ => continue,
            }
        };
        match folders.iter_mut().find(|(f, _)| *f == folder) {
            Some((_, names)) => match (names.as_mut(), name) {
                (Some(list), Some(name)) => {
                    if !list.contains(&name) {
                        list.push(name);
                    }
                }
                // Followed whole once is followed whole.
                _ => *names = None,
            },
            None => folders.push((folder, name.map(|name| vec![name]))),
        }
    }
    folders
}

/// One instance of the watch: the settings folder's and each followed
/// folder's, made together and replaced together.
struct Watch {
    inotify: inotify::Inotify,
    /// The settings folder's watch.
    own: i32,
    followed: Vec<Followed>,
    /// What each group listed when this was made: what it is compared with
    /// when the group is announced, to see whether to watch afresh.
    listed: Vec<(SettingsName, Vec<PathBuf>)>,
}

impl Watch {
    /// A new instance watching `dir`, made first if it is not there -- a
    /// program that has not saved anything yet has not made it, and the
    /// first save must still be seen -- and every path of `dependents` that
    /// can be followed.
    fn start(dir: &Path, dependents: &[Dependents]) -> Result<Self, WatchError> {
        let inotify = inotify::Inotify::new().map_err(|e| os("inotify_init1", e))?;
        std::fs::create_dir_all(dir).map_err(WatchError::Folder)?;
        let own = inotify
            .watch(&c_path(dir)?, MASK)
            .map_err(|e| os("inotify_add_watch", e))?;
        let mut followed: Vec<Followed> = Vec::new();
        let mut listed: Vec<(SettingsName, Vec<PathBuf>)> = Vec::new();
        for dependent in dependents {
            let paths = (dependent.paths)();
            for (folder, names) in folders_for(&paths) {
                let Ok(path) = c_path(&folder) else {
                    continue;
                };
                // A folder that went between being listed and being watched
                // was a change; the folder above it, followed too, said so,
                // and the next announcement lists the paths again.
                if let Ok(wd) = inotify.watch(&path, MASK) {
                    followed.push(Followed {
                        wd,
                        group: dependent.name,
                        names,
                    });
                }
            }
            listed.push((dependent.name, paths));
        }
        Ok(Self {
            inotify,
            own,
            followed,
            listed,
        })
    }

    /// What `group` listed when this was made.
    fn listed_for(&self, group: SettingsName) -> Option<&[PathBuf]> {
        self.listed
            .iter()
            .find(|(name, _)| *name == group)
            .map(|(_, paths)| paths.as_slice())
    }

    /// The groups an event from the watch `wd` about `name` counts for:
    /// `None` when `wd` is no followed folder's -- the settings folder's, or
    /// the instance's own when events were lost.
    fn groups_for(&self, wd: i32, name: &[u8]) -> Option<Vec<SettingsName>> {
        if wd == self.own {
            return None;
        }
        let mut groups: Vec<SettingsName> = Vec::new();
        let mut followed = false;
        for f in self.followed.iter().filter(|f| f.wd == wd) {
            followed = true;
            if f.counts(name) && !groups.contains(&f.group) {
                groups.push(f.group);
            }
        }
        followed.then_some(groups)
    }
}

/// The settings folder, watched, with the folders its groups depend on.
pub struct Watcher {
    dir: PathBuf,
    dependents: Vec<Dependents>,
    watch: Watch,
    policy: Policy,
    buf: Vec<u8>,
}

impl Watcher {
    /// Watch `dir`, the settings folder, alone.
    ///
    /// # Errors
    ///
    /// [`WatchError::Unsupported`] where there is no inotify, else as the
    /// folder or the watch fails.
    pub fn new(dir: &Path) -> Result<Self, WatchError> {
        Self::following(dir, Vec::new())
    }

    /// Watch `dir`, the settings folder, and the paths each of `dependents`
    /// names.
    ///
    /// # Errors
    ///
    /// [`WatchError::Unsupported`] where there is no inotify, else as the
    /// settings folder or its watch fails. A followed path that cannot be
    /// watched is passed over; see [`Dependents::paths`].
    pub fn following(dir: &Path, dependents: Vec<Dependents>) -> Result<Self, WatchError> {
        let watch = Watch::start(dir, &dependents)?;
        Ok(Self {
            dir: dir.to_path_buf(),
            dependents,
            watch,
            policy: Policy::new(QUIET),
            // A record with the longest name a file can have.
            buf: vec![0; inotify::HEADER.saturating_add(256).saturating_mul(16)],
        })
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
        let ready = self.watch.inotify.wait(ms).map_err(|e| os("poll", e))?;
        let mut out: Vec<SettingsName> = Vec::new();
        let mut rewatch = false;
        if ready {
            rewatch = self.read_out(&mut out)?;
        }
        out.extend(self.policy.due(Instant::now()));
        // A group announced may name other paths now: a theme chosen in
        // place of another, one removed, one installed to stand in for it,
        // another picture.
        let refollow = self.dependents.iter().any(|dependent| {
            out.contains(&dependent.name)
                && self.watch.listed_for(dependent.name) != Some((dependent.paths)().as_slice())
        });
        if rewatch || refollow {
            self.restart(&mut out)?;
        }
        if rewatch {
            // The folder went: made again and watched again, and everything
            // in it announced, since nobody knows what changed meanwhile.
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

    /// Read what the current instance has, deciding each event; answers
    /// whether the settings folder's own watch is gone.
    fn read_out(&mut self, out: &mut Vec<SettingsName>) -> Result<bool, WatchError> {
        let now = Instant::now();
        let events = self
            .watch
            .inotify
            .read(&mut self.buf)
            .map_err(|e| os("read", e))?;
        let mut rewatch = false;
        for event in events {
            // A record cut short ends what this read can say; what it would
            // have said is found on the next change, or never.
            let Ok(event) = event else { break };
            let decisions = match self.watch.groups_for(event.wd, event.name) {
                // A followed folder's: one decision for each group the name
                // counts for -- none, for a neighbour of a followed picture.
                Some(groups) => groups
                    .into_iter()
                    .map(|group| self.policy.dependent_event(event.mask, group, now))
                    .collect(),
                // The settings folder's -- or an overflow, which names no
                // watch and is the instance's: the settings folder's rule
                // covers it.
                None => vec![self.policy.event(event.mask, event.name, now)],
            };
            for decision in decisions {
                match decision {
                    Decision::Announce(names) => out.extend(names),
                    Decision::Everything => {
                        out.extend(all_in(&self.dir));
                        // Lost events may have been a followed folder's.
                        out.extend(self.dependents.iter().map(|d| d.name));
                    }
                    Decision::Rewatch => rewatch = true,
                    Decision::Wait => {}
                }
            }
        }
        Ok(rewatch)
    }

    /// Watch afresh -- the settings folder made again if it went, and each
    /// group's paths as they are now -- reading out what the old instance
    /// saw before the new one was watching, so nothing between the two is
    /// lost. Every group with paths is announced: what they hold may have
    /// changed while they were not followed.
    fn restart(&mut self, out: &mut Vec<SettingsName>) -> Result<(), WatchError> {
        let fresh = Watch::start(&self.dir, &self.dependents)?;
        // Read out before the swap: an event the old instance holds was seen
        // before the new one existed; one after is the new one's too.
        let rewatch = self.read_out(out)?;
        let _old = std::mem::replace(&mut self.watch, fresh);
        if rewatch {
            out.extend(all_in(&self.dir));
        }
        out.extend(self.dependents.iter().map(|d| d.name));
        Ok(())
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

/// Watch `dir`, and the folders `dependents` name, and hand every batch of
/// names to `announce`, until it answers `false`. For a thread of its own:
/// it waits in the kernel between changes.
///
/// # Errors
///
/// As [`Watcher::following`] and [`Watcher::next`]; the caller logs it and
/// goes on without announcements, which costs liveness and nothing else.
pub fn run(
    dir: &Path,
    dependents: Vec<Dependents>,
    mut announce: impl FnMut(&[SettingsName]) -> bool,
) -> Result<(), WatchError> {
    let mut watcher = Watcher::following(dir, dependents)?;
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
            run(dir.dir(), Vec::new(), |_| true),
            Err(WatchError::Unsupported)
        ));
    }

    /// A followed folder's every name counts: a rename, a deletion or the
    /// folder going announces its group at once; a write in place, once
    /// quiet -- and the group's own rename after writes is one change.
    #[test]
    fn a_followed_folders_changes_are_its_groups() {
        let group = name("appearance");
        let t = Instant::now();
        for mask in [
            IN_MOVED_TO,
            IN_MOVED_FROM,
            IN_DELETE,
            IN_DELETE_SELF,
            IN_MOVE_SELF,
            IN_IGNORED,
            IN_MOVED_TO | IN_ISDIR,
        ] {
            let mut p = Policy::new(QUIET);
            assert_eq!(
                p.dependent_event(mask, group, t),
                Decision::Announce(vec![group]),
                "{mask:#x}"
            );
        }
        let mut p = Policy::new(QUIET);
        assert_eq!(p.dependent_event(IN_CREATE, group, t), Decision::Wait);
        assert_eq!(p.dependent_event(IN_MODIFY, group, t), Decision::Wait);
        assert_eq!(p.due(t + QUIET / 2), Vec::new());
        assert_eq!(p.due(t + QUIET), vec![group]);
        // Written, then renamed into place: told once, at the rename.
        p.dependent_event(IN_MODIFY, group, t);
        assert_eq!(
            p.dependent_event(IN_MOVED_TO, group, t),
            Decision::Announce(vec![group])
        );
        assert_eq!(p.due(t + QUIET * 2), Vec::new());
        // Lost events are everything's, and what settles is nothing else.
        assert_eq!(
            p.dependent_event(IN_Q_OVERFLOW, group, t),
            Decision::Everything
        );
        assert_eq!(
            p.dependent_event(libcall::inotify::IN_ATTRIB, group, t),
            Decision::Wait
        );
        assert_eq!(p.next_due(), None);
    }

    /// A folder is followed whole and a file by its name in its folder; a
    /// folder named whole and by a name in it is followed whole, two names in
    /// one folder are one watch, and a file with no folder to hold it is
    /// nothing to watch.
    #[test]
    fn each_path_is_followed_whole_or_by_its_name() {
        let theme = ScratchDir::new("settingswatch-plan-theme");
        let pictures = ScratchDir::new("settingswatch-plan-pictures");
        let day = pictures.path("day.png");
        let night = pictures.path("night.png");
        std::fs::write(&day, b"x").unwrap();
        let planned = folders_for(&[
            theme.dir().to_path_buf(),
            day.clone(),
            // Not there yet: its folder is, so it is followed by name.
            night.clone(),
            day.clone(),
            pictures.path("gone/deeper.png"),
        ]);
        assert_eq!(
            planned,
            [
                (theme.dir().to_path_buf(), None),
                (
                    pictures.dir().to_path_buf(),
                    Some(vec![OsString::from("day.png"), OsString::from("night.png")])
                ),
            ]
        );
        // The folder itself named too: followed whole.
        let whole = folders_for(&[day, pictures.dir().to_path_buf(), night]);
        assert_eq!(whole, [(pictures.dir().to_path_buf(), None)]);
    }

    /// An event about a followed name counts, and one about the folder
    /// itself; a neighbour's does not, unless the folder is followed whole.
    #[test]
    fn a_neighbour_of_a_followed_picture_does_not_count() {
        let group = name("appearance");
        let picture = Followed {
            wd: 3,
            group,
            names: Some(vec![OsString::from("day.png")]),
        };
        assert!(picture.counts(b"day.png"));
        assert!(picture.counts(b""), "the folder itself");
        assert!(!picture.counts(b"holiday.png"));
        assert!(!picture.counts(b"day.png.new"));
        let whole = Followed {
            wd: 4,
            group,
            names: None,
        };
        assert!(whole.counts(b"anything"));
    }

    /// The paths the test below has its group name: a static, since
    /// [`Dependents::paths`] is a plain function, and that one test the only
    /// one to set it.
    #[cfg(unix)]
    static FOLLOWED: std::sync::Mutex<Vec<PathBuf>> = std::sync::Mutex::new(Vec::new());

    #[cfg(unix)]
    fn followed_for_test() -> Vec<PathBuf> {
        FOLLOWED
            .lock()
            .map(|folders| folders.clone())
            .unwrap_or_default()
    }

    #[cfg(unix)]
    fn follow(folders: &[&Path]) {
        *FOLLOWED.lock().unwrap() = folders.iter().map(|f| f.to_path_buf()).collect();
    }

    /// Wait up to a second for `watcher` to announce something.
    #[cfg(unix)]
    fn announced(watcher: &mut Watcher) -> Vec<SettingsName> {
        let mut all = Vec::new();
        let until = Instant::now().checked_add(Duration::from_secs(1)).unwrap();
        while Instant::now() < until {
            let names = watcher.next(Duration::from_millis(400)).unwrap();
            if !names.is_empty() {
                all.extend(names);
                break;
            }
        }
        all
    }

    /// On a system with inotify: a settings file saved is announced; a file
    /// renamed into a followed folder -- a theme saved -- is announced as its
    /// group; and when the group comes to name another folder, that one is
    /// followed from then on, and the old one no longer.
    #[cfg(unix)]
    #[test]
    fn a_followed_folder_is_announced_as_its_group_and_refollowed() {
        let settings = ScratchDir::new("settingswatch-settings");
        let theme = ScratchDir::new("settingswatch-theme");
        let other = ScratchDir::new("settingswatch-other");
        follow(&[theme.dir()]);
        let group = name("appearance");
        let mut watcher = Watcher::following(
            settings.dir(),
            vec![Dependents {
                name: group,
                paths: followed_for_test,
            }],
        )
        .unwrap();

        std::fs::write(settings.path("notes.yaml.new"), "a: 1\n").unwrap();
        std::fs::rename(settings.path("notes.yaml.new"), settings.path("notes.yaml")).unwrap();
        assert_eq!(announced(&mut watcher), vec![name("notes")]);

        std::fs::write(theme.path("theme.yaml.new"), "colors:\n").unwrap();
        std::fs::rename(theme.path("theme.yaml.new"), theme.path("theme.yaml")).unwrap();
        assert_eq!(announced(&mut watcher), vec![group]);

        // The group now names the other folder as well: announced, it is
        // followed afresh.
        follow(&[theme.dir(), other.dir()]);
        std::fs::write(theme.path("x.new"), "x").unwrap();
        std::fs::rename(theme.path("x.new"), theme.path("x")).unwrap();
        assert_eq!(announced(&mut watcher), vec![group]);
        std::fs::write(other.path("y.new"), "y").unwrap();
        std::fs::rename(other.path("y.new"), other.path("y")).unwrap();
        assert_eq!(announced(&mut watcher), vec![group]);

        // And no longer the first: a change there is nobody's.
        follow(&[other.dir()]);
        std::fs::write(other.path("z.new"), "z").unwrap();
        std::fs::rename(other.path("z.new"), other.path("z")).unwrap();
        assert_eq!(announced(&mut watcher), vec![group]);
        std::fs::write(theme.path("w.new"), "w").unwrap();
        std::fs::rename(theme.path("w.new"), theme.path("w")).unwrap();
        assert_eq!(announced(&mut watcher), Vec::new());

        // A picture is followed by its name in its folder: saved over, it
        // is the group's; a neighbour saved beside it is nobody's.
        let pictures = ScratchDir::new("settingswatch-pictures");
        let day = pictures.path("day.png");
        follow(&[other.dir(), day.as_path()]);
        std::fs::write(other.path("v.new"), "v").unwrap();
        std::fs::rename(other.path("v.new"), other.path("v")).unwrap();
        assert_eq!(announced(&mut watcher), vec![group], "and followed afresh");
        std::fs::write(pictures.path("holiday.png.new"), "h").unwrap();
        std::fs::rename(
            pictures.path("holiday.png.new"),
            pictures.path("holiday.png"),
        )
        .unwrap();
        assert_eq!(announced(&mut watcher), Vec::new());
        std::fs::write(pictures.path("day.png.new"), "d").unwrap();
        std::fs::rename(pictures.path("day.png.new"), pictures.path("day.png")).unwrap();
        assert_eq!(announced(&mut watcher), vec![group]);
        follow(&[]);
    }
}
