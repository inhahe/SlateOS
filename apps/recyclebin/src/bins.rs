//! Every drive's recycle bin, and which one a file goes to.
//!
//! The operator's answer to E-Q4 (design-decisions §1238): a file goes to the
//! bin on its own drive. Deleting from a USB stick is then a rename on the
//! stick -- at once, with nothing copied onto the system disk -- and what is
//! in the stick's bin goes where the stick goes.
//!
//! # Where the bins are
//!
//! - The drive the home folder is on keeps the **home bin**, `~/.recycle`,
//!   where the one bin always was, so nothing deleted before is stranded.
//! - Every other drive keeps its bin at its top, `<mount point>/.recycle-<uid>`:
//!   one for each user -- the number is that of the owner of their home
//!   folder -- so one person's deletions are not in another's bin on a drive
//!   they share. It is made readable by its owner alone.
//!
//! A drive's bin that is found already there is used only if it is a folder,
//! not a link, and belongs to the user or to the administrator: one another
//! user made could be read by them, and a link could take what is deleted
//! anywhere. (A drive whose files have no owners -- a FAT stick -- shows them
//! all as one owner, the administrator's or the user's, and is used.)
//!
//! A machine that cannot say which drive a file is on -- no `/proc/mounts`,
//! or a home folder it cannot find on any drive -- sends every file to the
//! home bin, as every file went before there were more.

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use pathtext::ShowPath as _;

use crate::drives::{Drives, SystemDrives};
use crate::limits::Limits;
use crate::{Pruned, RecycleBin};

/// The administrator's user number, whose folders a user may trust.
#[cfg(unix)]
const ROOT_UID: u32 = 0;

/// No drives: every file is on the home drive ([`Bins::single`]).
struct NoDrives;

impl Drives for NoDrives {
    fn mounted(&self) -> Vec<PathBuf> {
        Vec::new()
    }

    fn mount_point_of(&self, _path: &Path) -> Option<PathBuf> {
        None
    }
}

/// One drive's bin.
#[derive(Clone, Debug, PartialEq)]
pub struct DriveBin {
    /// Where its drive is mounted, if that is known.
    pub drive: Option<PathBuf>,
    /// Whether this is the home bin.
    pub home: bool,
    /// The bin.
    pub bin: RecycleBin,
}

impl DriveBin {
    /// The drive as the user knows it: "Home drive" for the home bin, with
    /// where it is mounted when that is known, and the mount point for the
    /// others.
    #[must_use]
    pub fn label(&self) -> String {
        match (&self.drive, self.home) {
            (Some(drive), true) => format!("Home drive ({})", drive.shown()),
            (None, true) => String::from("Home drive"),
            (Some(drive), false) => drive.shown().to_string(),
            (None, false) => String::from("A drive"),
        }
    }
}

/// Every drive's bin, and which one a file goes to.
pub struct Bins {
    /// The home bin.
    home: RecycleBin,
    /// Where the home folder's drive is mounted, if that is known.
    home_drive: Option<PathBuf>,
    /// What a drive's bin is called at its top: `.recycle-<uid>`.
    name: OsString,
    /// The user whose bins these are, where users have numbers.
    owner: Option<u32>,
    /// The machine's drives.
    drives: Box<dyn Drives>,
}

impl Bins {
    /// The user's bins on this machine: the home bin in `$HOME`, and one on
    /// each other drive.
    ///
    /// With `HOME` unset the home bin is in `/tmp`, as it always was -- a
    /// hazard of its own, kept apart from this change so that the two are not
    /// mistaken for each other.
    #[must_use]
    pub fn system() -> Self {
        let home = std::env::var_os("HOME").map_or_else(|| PathBuf::from("/tmp"), PathBuf::from);
        let owner = owner_of(&home);
        Self::new(home.join(".recycle"), Box::new(SystemDrives), owner)
    }

    /// One bin, at `home_bin`, for every file: a machine whose drives cannot
    /// be told apart. For a program's tests, whose files are all in one
    /// scratch folder.
    #[must_use]
    pub fn single(home_bin: PathBuf) -> Self {
        Self::new(home_bin, Box::new(NoDrives), None)
    }

    /// Bins with the home bin at `home_bin`, on `drives`, belonging to user
    /// `owner`.
    #[must_use]
    pub fn new(home_bin: PathBuf, drives: Box<dyn Drives>, owner: Option<u32>) -> Self {
        let home_drive = drives.mount_point_of(&home_bin);
        let name = match owner {
            Some(uid) => OsString::from(format!(".recycle-{uid}")),
            None => OsString::from(".recycle"),
        };
        Self {
            home: RecycleBin::new(home_bin),
            home_drive,
            name,
            owner,
            drives,
        }
    }

    /// The home bin.
    #[must_use]
    pub fn home(&self) -> DriveBin {
        DriveBin {
            drive: self.home_drive.clone(),
            home: true,
            bin: self.home.clone(),
        }
    }

    /// The bin a file at `path` goes to: its drive's. The bin need not exist
    /// yet; [`recycle`](Self::recycle) makes it.
    #[must_use]
    pub fn bin_for(&self, path: &Path) -> DriveBin {
        match (self.drives.mount_point_of(path), &self.home_drive) {
            (Some(drive), Some(home)) if drive != *home => DriveBin {
                bin: RecycleBin::new(drive.join(&self.name)),
                drive: Some(drive),
                home: false,
            },
            _ => self.home(),
        }
    }

    /// Move `path` to its drive's bin, making the bin if the drive has none.
    /// Returns the bin it went to, and its entry there.
    ///
    /// # Errors
    ///
    /// A drive's bin that is there but is not the user's (see the module
    /// docs) is `PermissionDenied`, and the file is left where it was;
    /// otherwise what [`RecycleBin::recycle`] meets.
    pub fn recycle(&self, path: &Path) -> io::Result<(DriveBin, String)> {
        let target = self.bin_for(path);
        if !target.home {
            self.claim(target.bin.root())?;
        }
        let id = target.bin.recycle(path)?;
        Ok((target, id))
    }

    /// Every bin there is now: the home bin, then each other drive's that the
    /// drive has -- a drive nothing was ever deleted from has none, and is
    /// not listed.
    #[must_use]
    pub fn reachable(&self) -> Vec<DriveBin> {
        let mut bins = vec![self.home()];
        for drive in self.drives.mounted() {
            if self.home_drive.as_ref() == Some(&drive) {
                continue;
            }
            let root = drive.join(&self.name);
            if fs::symlink_metadata(&root).is_ok_and(|meta| self.is_ours(&meta)) {
                bins.push(DriveBin {
                    drive: Some(drive),
                    home: false,
                    bin: RecycleBin::new(root),
                });
            }
        }
        bins
    }

    /// Hold every bin there is to its limits -- its own, or `default` where
    /// it has none ([`RecycleBin::limits`]) -- as of `now`. For when the bins
    /// are looked at, and when a drive appears: a stick that was away for a
    /// month is pruned the moment it is back, from the times recorded with
    /// what is in it.
    ///
    /// Every bin is tried; each answers for itself.
    pub fn prune_all(
        &self,
        default: &Limits,
        now: SystemTime,
    ) -> Vec<(DriveBin, io::Result<Pruned>)> {
        self.reachable()
            .into_iter()
            .map(|drive| {
                let limits = drive.bin.limits(default);
                let pruned = drive.bin.prune(&limits, now);
                (drive, pruned)
            })
            .collect()
    }

    /// Make sure the drive's bin at `root` is there and is the user's.
    fn claim(&self, root: &Path) -> io::Result<()> {
        match fs::symlink_metadata(root) {
            Ok(meta) if self.is_ours(&meta) => Ok(()),
            Ok(_) => Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "{} is not a recycle bin of yours, so nothing was moved into it",
                    root.shown()
                ),
            )),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                match make_private_dir(root) {
                    Ok(()) => Ok(()),
                    // Made a moment ago by another window of the user's, or
                    // by someone else: which, the check says.
                    Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                        let meta = fs::symlink_metadata(root)?;
                        if self.is_ours(&meta) {
                            Ok(())
                        } else {
                            Err(io::Error::new(
                                io::ErrorKind::PermissionDenied,
                                format!("{} is not a recycle bin of yours", root.shown()),
                            ))
                        }
                    }
                    Err(e) => Err(e),
                }
            }
            Err(e) => Err(e),
        }
    }

    /// Whether a drive's bin with metadata `meta` (not followed through a
    /// link) is one the user may use: a folder, and the user's own or the
    /// administrator's.
    fn is_ours(&self, meta: &fs::Metadata) -> bool {
        meta.is_dir()
            && !meta.file_type().is_symlink()
            && self.owner.is_none_or(|owner| owned_by(meta, owner))
    }
}

/// Whether what `meta` describes belongs to user `owner` or to the
/// administrator.
#[cfg(unix)]
fn owned_by(meta: &fs::Metadata, owner: u32) -> bool {
    use std::os::unix::fs::MetadataExt as _;
    meta.uid() == owner || meta.uid() == ROOT_UID
}

/// Where users have no numbers there is no owner to check.
#[cfg(not(unix))]
fn owned_by(_meta: &fs::Metadata, _owner: u32) -> bool {
    true
}

/// The number of the user who owns `path`.
#[cfg(unix)]
fn owner_of(path: &Path) -> Option<u32> {
    use std::os::unix::fs::MetadataExt as _;
    fs::metadata(path).ok().map(|meta| meta.uid())
}

/// Where users have no numbers, none.
#[cfg(not(unix))]
fn owner_of(_path: &Path) -> Option<u32> {
    None
}

/// Make the folder `path`, readable and writable by its owner alone.
fn make_private_dir(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        fs::DirBuilder::new().mode(0o700).create(path)
    }
    #[cfg(not(unix))]
    {
        fs::create_dir(path)
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic,
        clippy::arithmetic_side_effects
    )]

    use super::*;
    use crate::drives::longest_mount;
    use scratchdir::ScratchDir;
    use std::time::Duration;

    /// Drives of a test's own making: folders standing for mount points.
    struct Pretend(Vec<PathBuf>);

    impl Drives for Pretend {
        fn mounted(&self) -> Vec<PathBuf> {
            self.0.clone()
        }
        fn mount_point_of(&self, path: &Path) -> Option<PathBuf> {
            longest_mount(&self.0, path)
        }
    }

    /// A machine with two drives: the system drive at the scratch folder,
    /// holding `home`, and a stick at `stick` -- and the bins on them.
    struct Machine {
        _scratch: ScratchDir,
        root: PathBuf,
        stick: PathBuf,
        bins: Bins,
    }

    fn machine(label: &str) -> Machine {
        let scratch = ScratchDir::new(&format!("recyclebin_bins_{label}"));
        let root = scratch.dir().to_path_buf();
        let stick = root.join("stick");
        fs::create_dir_all(root.join("home")).unwrap();
        fs::create_dir_all(&stick).unwrap();
        let bins = Bins::new(
            root.join("home").join(".recycle"),
            Box::new(Pretend(vec![root.clone(), stick.clone()])),
            owner_of(&root.join("home")),
        );
        Machine {
            _scratch: scratch,
            root,
            stick,
            bins,
        }
    }

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    /// **A file goes to the bin on its own drive**, so a delete from a stick
    /// is a rename on the stick and what is in its bin goes with it; and it
    /// is put back from there.
    #[test]
    fn a_file_goes_to_the_bin_on_its_own_drive() {
        let m = machine("own_drive");
        let letter = m.root.join("home").join("letter.txt");
        let photo = m.stick.join("photo.jpg");
        write(&letter, "dear");
        write(&photo, "jpeg");

        let (home, _) = m.bins.recycle(&letter).unwrap();
        assert!(home.home);
        assert_eq!(home.bin.root(), m.root.join("home").join(".recycle"));

        let (stick, id) = m.bins.recycle(&photo).unwrap();
        assert!(!stick.home, "the stick's file went to the home bin");
        assert!(
            stick.bin.root().starts_with(&m.stick),
            "{:?}",
            stick.bin.root()
        );
        assert_eq!(stick.drive.as_deref(), Some(m.stick.as_path()));
        assert!(!photo.exists());

        let reachable = m.bins.reachable();
        assert_eq!(reachable.len(), 2);
        assert!(reachable[0].home);
        assert_eq!(reachable[1], stick);
        assert!(reachable[0].label().starts_with("Home drive"));
        assert_eq!(reachable[1].bin.list().unwrap().len(), 1);

        assert_eq!(stick.bin.restore(&id).unwrap(), photo);
        assert_eq!(fs::read_to_string(&photo).unwrap(), "jpeg");
    }

    /// A drive nothing was deleted from has no bin, and none is listed.
    #[test]
    fn a_drive_with_nothing_deleted_has_no_bin_listed() {
        let m = machine("no_bin");
        let reachable = m.bins.reachable();
        assert_eq!(reachable.len(), 1);
        assert!(reachable[0].home);
        assert!(
            fs::read_dir(&m.stick).unwrap().next().is_none(),
            "looking made a bin"
        );
    }

    /// **A drive's bin that is not a folder is not used**, and the file
    /// stays where it was: what is moved into a bin must not land where
    /// someone else chose.
    #[test]
    fn a_bin_that_is_not_a_folder_is_not_used() {
        let m = machine("not_a_folder");
        let photo = m.stick.join("photo.jpg");
        write(&photo, "jpeg");
        let squatter = m.bins.bin_for(&photo);
        write(squatter.bin.root(), "not a bin");

        let err = m.bins.recycle(&photo).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied, "{err}");
        assert!(photo.exists(), "the file left with nowhere to go");
        assert_eq!(m.bins.reachable().len(), 1, "a file was listed as a bin");
    }

    /// With no way to tell drives apart, every file goes to the home bin, as
    /// every file always did.
    #[test]
    fn when_drives_cannot_be_told_apart_every_file_goes_home() {
        let scratch = ScratchDir::new("recyclebin_bins_no_drives");
        let root = scratch.dir().to_path_buf();
        let bins = Bins::new(
            root.join("home/.recycle"),
            Box::new(Pretend(Vec::new())),
            None,
        );
        let photo = root.join("stick/photo.jpg");
        write(&photo, "jpeg");
        let (went, _) = bins.recycle(&photo).unwrap();
        assert!(went.home);
        assert_eq!(went.drive, None);
        assert_eq!(went.label(), "Home drive");

        // The stick is known, the home folder's drive is not: which drive is
        // "another" cannot be told, so the file still goes home rather than
        // to a bin at the top of what may be the home drive itself.
        let bins = Bins::new(
            root.join("home/.recycle"),
            Box::new(Pretend(vec![root.join("stick")])),
            None,
        );
        let song = root.join("stick/song.ogg");
        write(&song, "ogg");
        let (went, _) = bins.recycle(&song).unwrap();
        assert!(
            went.home,
            "a file went to a drive's bin with the home drive unknown"
        );
    }

    /// **Each bin is held to its own limits**, the others to the default.
    #[test]
    fn each_bin_is_held_to_its_own_limits() {
        let m = machine("own_limits");
        for name in ["a.txt", "b.txt"] {
            let at_home = m.root.join("home").join(name);
            let on_stick = m.stick.join(name);
            write(&at_home, "x");
            write(&on_stick, "x");
            m.bins.recycle(&at_home).unwrap();
            m.bins.recycle(&on_stick).unwrap();
        }
        let stick = m.bins.reachable().remove(1);
        stick
            .bin
            .set_limits(Some(&Limits {
                max_items: Some(1),
                ..Limits::NONE
            }))
            .unwrap();

        let outcomes = m.bins.prune_all(&Limits::default(), SystemTime::now());
        assert_eq!(outcomes.len(), 2);
        for (drive, pruned) in &outcomes {
            let pruned = pruned.as_ref().unwrap();
            let expected = if drive.home { 0 } else { 1 };
            assert_eq!(pruned.deleted, expected, "{}", drive.label());
        }
        assert_eq!(m.bins.home().bin.list().unwrap().len(), 2);
        assert_eq!(stick.bin.list().unwrap().len(), 1);

        // A day on, the default's thirty days take nothing at home.
        let later = SystemTime::now() + Duration::from_hours(24);
        assert_eq!(
            m.bins.prune_all(&Limits::default(), later)[0]
                .1
                .as_ref()
                .unwrap()
                .deleted,
            0
        );
    }
}
