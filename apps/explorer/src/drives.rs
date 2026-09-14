//! Which storage device a path lives on.
//!
//! One answer, in one place, because more than one thing needs it and they
//! must not disagree. Before this there were **two** copies of the question:
//! `fileops::same_device` and a private `same_device` in [`crate::dropzone`],
//! whose comment said it used *"the same heuristic"* — and both compared the
//! path's **first component**.
//!
//! That heuristic is wrong on the operating system this is written for.
//! SlateOS paths are Unix-shaped, so the first component of every absolute
//! path is `/`, and a mount point is a *directory* rather than a prefix. Every
//! pair of absolute paths therefore looked like one device — which made every
//! drag a **Move**, including a drag off a camera card, which would have taken
//! the photo off the card. The test that covered it asserted the gap instead
//! of closing it: *"on Unix `/` is always the root, so this tests the prefix
//! logic. On our OS different mount points would have different first
//! components."* They would not.
//!
//! # "Don't know" is not one answer
//!
//! [`same_drive`] answers `Option<bool>`, and the `None` is the point. The two
//! callers want *opposite* conservative readings of not knowing:
//!
//! * a **drag** that cannot tell should Copy — a copy never destroys anything,
//!   and the worst case is a duplicate the user deletes;
//! * a **queue** that cannot tell should serialise — running two operations on
//!   one disk is the thing it exists to prevent, and the worst case is that
//!   one waits needlessly.
//!
//! A `bool` would have forced one of them to be wrong in the dangerous
//! direction. Each caller states its own reading at the call site, where the
//! consequence is visible, rather than inheriting one from here.

use std::collections::HashSet;
use std::path::Path;

/// The identity of the device backing a path.
///
/// Opaque and only ever compared: what a device id *is* differs by platform
/// and none of the callers have any business knowing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DriveId(Kind);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Kind {
    /// A device the platform named, by whatever number it uses.
    Device(u64),
    /// A path this machine could not resolve to a device.
    ///
    /// Deliberately **not** equal to itself for the purposes of
    /// [`same_drive`], which reports `None` rather than `true` when either
    /// side is unknown. Two paths we know nothing about are not known to be
    /// on one drive; they are simply not known.
    Unknown,
}

impl DriveId {
    /// Whether this is a real device rather than a shrug.
    #[must_use]
    pub const fn is_known(self) -> bool {
        matches!(self.0, Kind::Device(_))
    }
}

/// The device backing `path`, as far as this machine can tell.
///
/// A destination frequently does not exist yet — that is what a copy is for —
/// so the nearest ancestor that *does* exist is asked instead. A file is on
/// the same device as the directory it is about to be created in, which is the
/// question being asked.
#[must_use]
pub fn drive_of(path: &Path) -> DriveId {
    let mut probe = path;
    loop {
        if let Some(id) = device_of(probe) {
            return DriveId(Kind::Device(id));
        }
        match probe.parent() {
            // `parent()` of `/` is `None` and of `"foo"` is `""`, so the
            // second arm is the one that stops a relative path looping.
            Some(parent) if parent != probe => probe = parent,
            _ => return DriveId(Kind::Unknown),
        }
    }
}

/// Whether two paths are on one device, or `None` if this machine cannot tell.
///
/// See the module docs for why this is not a `bool`.
#[must_use]
pub fn same_drive(a: &Path, b: &Path) -> Option<bool> {
    let (a, b) = (drive_of(a), drive_of(b));
    (a.is_known() && b.is_known()).then(|| a == b)
}

/// Every drive one operation will touch.
///
/// Both ends count. A copy from `D:` to `E:` loads *both*, so it collides with
/// anything already using either; a copy whose source and destination are the
/// same drive loads that drive twice over, which is the case the whole rule
/// most wants to protect.
#[derive(Clone, Debug, Default)]
pub struct DriveSet {
    known: HashSet<DriveId>,
    /// Whether any path in the set could not be resolved to a device.
    ///
    /// **An unresolved path is treated as "might be any drive".** This is the
    /// scheduler's reading of the `None` that [`same_drive`] answers, and it is
    /// the opposite of the drag's: a queue that guesses wrong here runs two
    /// operations on one disk, which is the thing it exists to prevent, while
    /// the cost of guessing the other way is that one operation waits when it
    /// did not have to.
    unknown: bool,
}

impl DriveSet {
    /// The drives backing `paths`, resolved once per directory.
    ///
    /// Per *directory* rather than per path: a copy of ten thousand photos
    /// touches two drives and one `stat` each would be ten thousand of them
    /// for an answer that cannot change within a folder.
    #[must_use]
    pub fn of<'a, I: IntoIterator<Item = &'a Path>>(paths: I) -> Self {
        let mut set = Self::default();
        let mut asked: HashSet<&Path> = HashSet::new();
        for path in paths {
            // The *parent*, because a destination that does not exist yet is
            // on the drive of the directory it is about to appear in.
            let dir = path.parent().unwrap_or(path);
            if asked.insert(dir) {
                set.add(drive_of(dir));
            }
        }
        set
    }

    /// Note one more drive.
    pub fn add(&mut self, id: DriveId) {
        if id.is_known() {
            self.known.insert(id);
        } else {
            self.unknown = true;
        }
    }

    /// Whether this set has nothing in it at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.known.is_empty() && !self.unknown
    }

    /// Whether running these two at once would load one drive twice.
    ///
    /// An empty set shares with nothing -- an operation that touches no drive
    /// cannot be in anything's way. Anything *unresolved* on either side
    /// shares with everything, for the reason on [`DriveSet::unknown`].
    #[must_use]
    pub fn shares_with(&self, other: &Self) -> bool {
        if self.is_empty() || other.is_empty() {
            return false;
        }
        if self.unknown || other.unknown {
            return true;
        }
        !self.known.is_disjoint(&other.known)
    }
}

/// The platform's device number for a path that exists, if it has one.
#[cfg(unix)]
fn device_of(path: &Path) -> Option<u64> {
    use std::os::unix::fs::MetadataExt as _;
    // `metadata`, not `symlink_metadata`: a symlink's own device says where
    // the *link* is, and what is about to be copied is what it points at.
    std::fs::metadata(path).ok().map(|m| m.dev())
}

/// The volume serial number, which on Windows is what a device id means.
///
/// `std` exposes it only behind an unstable feature, so this uses the path's
/// **prefix** instead — and on Windows that is not an approximation: `C:` and
/// `\\server\share` are the two forms a volume takes, and a path carries its
/// own. The number is a hash of that prefix, because [`DriveId`] is `Copy` and
/// a `String` is not; a collision would merge two volumes, which for both
/// callers is the direction that costs throughput rather than data.
#[cfg(not(unix))]
fn device_of(path: &Path) -> Option<u64> {
    use std::hash::{Hash as _, Hasher as _};
    use std::path::Component;

    let Some(Component::Prefix(prefix)) = path.components().next() else {
        return None;
    };
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    // Case-insensitively: `C:\x` and `c:\x` are one volume on Windows, and a
    // drag between the two must not look like a drag between two drives.
    prefix
        .as_os_str()
        .to_string_lossy()
        .to_lowercase()
        .hash(&mut hasher);
    Some(hasher.finish())
}

#[cfg(test)]
mod tests {
    // A test that panics on bad data is doing its job; the defensive lints are
    // for code that runs on a user's files.
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;
    use scratchdir::ScratchDir;
    use std::fs;

    #[test]
    fn two_paths_in_one_directory_are_on_one_drive() {
        let scratch = ScratchDir::new("drives_same");
        let root = scratch.dir();
        fs::write(root.join("a.txt"), "a").unwrap();
        fs::write(root.join("b.txt"), "b").unwrap();

        assert_eq!(
            same_drive(&root.join("a.txt"), &root.join("b.txt")),
            Some(true)
        );
    }

    /// The destination of a copy does not exist yet, and must still resolve.
    #[test]
    fn a_path_that_does_not_exist_yet_resolves_through_its_parent() {
        let scratch = ScratchDir::new("drives_future");
        let root = scratch.dir();
        fs::write(root.join("here.txt"), "x").unwrap();
        let not_yet = root.join("deeper/and/deeper/new.txt");
        assert!(!not_yet.exists());

        assert_eq!(same_drive(&root.join("here.txt"), &not_yet), Some(true));
        assert!(drive_of(&not_yet).is_known(), "no ancestor resolved");
    }

    /// A directory and a file inside it are on one drive.
    #[test]
    fn a_directory_and_its_contents_are_on_one_drive() {
        let scratch = ScratchDir::new("drives_parent");
        let root = scratch.dir();
        fs::create_dir(root.join("sub")).unwrap();
        fs::write(root.join("sub/file.txt"), "x").unwrap();

        assert_eq!(same_drive(root, &root.join("sub/file.txt")), Some(true));
    }

    /// **A path that resolves to nothing says so, rather than guessing.**
    ///
    /// This is the case the old heuristic got wrong in the expensive
    /// direction: it compared first components, found them equal, and reported
    /// a confident "same device" for two paths it knew nothing about.
    #[test]
    fn an_unresolvable_path_is_not_quietly_the_same_as_another() {
        let a = Path::new("no-such-directory-xyzzy/one.txt");
        let b = Path::new("no-such-directory-plugh/two.txt");
        assert!(!drive_of(a).is_known());
        assert!(!drive_of(b).is_known());
        assert_eq!(same_drive(a, b), None, "two shrugs are not an agreement");
        assert_eq!(same_drive(a, a), None, "not even with itself");
    }

    // ---- sets ---------------------------------------------------------

    #[test]
    fn two_operations_under_one_directory_share_its_drive() {
        let scratch = ScratchDir::new("driveset_share");
        let root = scratch.dir();
        fs::write(root.join("a.txt"), "a").unwrap();
        fs::write(root.join("b.txt"), "b").unwrap();

        let one = DriveSet::of([root.join("a.txt").as_path()]);
        let two = DriveSet::of([root.join("b.txt").as_path()]);
        assert!(one.shares_with(&two));
        assert!(two.shares_with(&one), "sharing is not one-way");
    }

    /// An operation that touches nothing is in nothing's way.
    #[test]
    fn an_empty_set_shares_with_nothing() {
        let scratch = ScratchDir::new("driveset_empty");
        let root = scratch.dir();
        fs::write(root.join("a.txt"), "a").unwrap();

        let empty = DriveSet::default();
        let real = DriveSet::of([root.join("a.txt").as_path()]);
        assert!(empty.is_empty());
        assert!(!empty.shares_with(&real));
        assert!(!real.shares_with(&empty));
        assert!(!empty.shares_with(&empty));
    }

    /// **A drive we cannot name is treated as every drive.**
    ///
    /// The scheduler's reading of "don't know", and the opposite of the
    /// drag's: waiting needlessly costs time, and running two operations on
    /// one disk is what the rule exists to prevent.
    #[test]
    fn an_unresolved_path_collides_with_everything() {
        let scratch = ScratchDir::new("driveset_unknown");
        let root = scratch.dir();
        fs::write(root.join("a.txt"), "a").unwrap();

        let real = DriveSet::of([root.join("a.txt").as_path()]);
        let vague = DriveSet::of([Path::new("no-such-dir-xyzzy/f.txt")]);
        assert!(!vague.is_empty(), "an unresolved path is still a path");
        assert!(vague.shares_with(&real));
        assert!(real.shares_with(&vague));
        assert!(vague.shares_with(&vague));
    }

    /// And one known side is not enough either.
    #[test]
    fn one_known_side_still_answers_nothing() {
        let scratch = ScratchDir::new("drives_half");
        let root = scratch.dir();
        fs::write(root.join("real.txt"), "x").unwrap();

        assert_eq!(
            same_drive(&root.join("real.txt"), Path::new("no-such-dir-xyzzy/f")),
            None
        );
    }
}
