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
