//! Which drive a file is on, and which drives there are.
//!
//! A drive is named by where it is mounted. On SlateOS the mounts are in
//! `/proc/mounts`, in Linux's format: one mount a line, `source mount-point
//! type options 0 0`, with a space, a tab, a newline or a backslash in the
//! mount point written as a three-digit octal escape (`\040`). The kernel
//! writes every byte that is not printable ASCII the same way
//! (`kernel/src/fs/escape.rs`), so the mount point read back is the exact
//! bytes of the path -- a mount point need not be text.
//!
//! A machine that has no `/proc/mounts` -- the development host -- has no
//! drives as far as this is concerned, and every file is on the home drive:
//! one bin, as before there were more.
//!
//! [`Drives`] is a trait so the bins can be tested with drives of the test's
//! own making; [`SystemDrives`] is the machine's.

use std::fs;
use std::path::{Path, PathBuf};

/// Where the machine lists its mounts.
const MOUNTS: &str = "/proc/mounts";

/// The most of `/proc/mounts` that is read. A machine with tens of thousands
/// of mounts is not one this is for, and a file without an end -- a broken
/// `/proc` -- must not be read without one.
const MOUNTS_MAX: usize = 1 << 20;

/// Filesystem types that are not drives: views of the kernel and of devices,
/// which hold no file of a user's, so no bin.
const NOT_DRIVES: &[&str] = &[
    "autofs",
    "binfmt_misc",
    "bpf",
    "cgroup",
    "cgroup2",
    "configfs",
    "debugfs",
    "devfs",
    "devpts",
    "devtmpfs",
    "fusectl",
    "hugetlbfs",
    "mqueue",
    "proc",
    "procfs",
    "pstore",
    "securityfs",
    "sysfs",
    "tracefs",
];

/// The drives a machine has.
pub trait Drives: Send + Sync {
    /// Where each drive is mounted now.
    fn mounted(&self) -> Vec<PathBuf>;

    /// Where the drive holding `path` is mounted, or `None` when that cannot
    /// be told.
    fn mount_point_of(&self, path: &Path) -> Option<PathBuf>;
}

/// This machine's drives, from `/proc/mounts`.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemDrives;

impl Drives for SystemDrives {
    fn mounted(&self) -> Vec<PathBuf> {
        match safeio::read_capped(Path::new(MOUNTS), MOUNTS_MAX) {
            // Cut short, the last line may be half a mount point -- a prefix
            // of a real one, naming a drive that is not there -- so it goes.
            Ok(read) if read.truncated => {
                let whole_lines = read
                    .bytes
                    .iter()
                    .rposition(|b| *b == b'\n')
                    .map_or(&[][..], |end| read.bytes.get(..end).unwrap_or_default());
                parse_mounts(whole_lines)
            }
            Ok(read) => parse_mounts(&read.bytes),
            // No `/proc/mounts`: the development host, or a machine whose
            // `/proc` is not mounted. Either way there is nothing to list, and
            // every file is taken to be on the home drive.
            Err(_) => Vec::new(),
        }
    }

    /// The longest mount point the path is under, after its folder is
    /// resolved: a path may reach a drive through a link (`~/stick` naming
    /// `/media/stick`), and it is where the file *is* that decides which bin
    /// it can be moved to without a copy. The folder, not the file: the file
    /// may itself be a link, and what is recycled is the link, which is where
    /// its folder is.
    fn mount_point_of(&self, path: &Path) -> Option<PathBuf> {
        let folder = path.parent().filter(|p| !p.as_os_str().is_empty())?;
        let resolved = fs::canonicalize(folder).ok()?;
        longest_mount(&self.mounted(), &resolved)
    }
}

/// The mount points in a `/proc/mounts`, less those of filesystems that are
/// not drives ([`NOT_DRIVES`]). A line that is not one -- too few fields, a
/// mount point that is not absolute -- is skipped.
#[must_use]
pub fn parse_mounts(text: &[u8]) -> Vec<PathBuf> {
    let mut mounts = Vec::new();
    for line in text.split(|b| *b == b'\n') {
        let mut fields = line
            .split(|b| *b == b' ' || *b == b'\t')
            .filter(|f| !f.is_empty());
        let (Some(_source), Some(point), Some(kind)) =
            (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        let kind = unescape_octal(kind);
        if NOT_DRIVES.iter().any(|n| n.as_bytes() == kind.as_slice()) {
            continue;
        }
        let point = unescape_octal(point);
        if point.first() != Some(&b'/') {
            continue;
        }
        let point = PathBuf::from(pathcodec::os_string_from_bytes(point));
        if !mounts.contains(&point) {
            mounts.push(point);
        }
    }
    mounts
}

/// A `/proc/mounts` field with its `\ooo` escapes decoded. A backslash not
/// followed by three octal digits is kept as it is -- the kernel writes none,
/// and a reader that guessed would read a different path.
fn unescape_octal(field: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(field.len());
    let mut rest = field;
    while let Some((&b, tail)) = rest.split_first() {
        // Three octal digits that make a byte: `\777` does not, and is kept.
        if b == b'\\'
            && let Some(digits) = tail.get(..3)
            && digits.iter().all(|d| (b'0'..=b'7').contains(d))
            && let Some(byte) = std::str::from_utf8(digits)
                .ok()
                .and_then(|text| u8::from_str_radix(text, 8).ok())
        {
            out.push(byte);
            rest = tail.get(3..).unwrap_or_default();
            continue;
        }
        out.push(b);
        rest = tail;
    }
    out
}

/// Of `mounts`, the one `path` is under that is longest -- the drive it is
/// on, since a drive mounted inside another's folder holds what is under its
/// own mount point. Component by component: `/media/stick2` is not under
/// `/media/stick`.
#[must_use]
pub fn longest_mount(mounts: &[PathBuf], path: &Path) -> Option<PathBuf> {
    mounts
        .iter()
        .filter(|m| path.starts_with(m))
        .max_by_key(|m| m.components().count())
        .cloned()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::*;

    const MOUNTS_TEXT: &[u8] = b"none / ext4 rw 0 0\n\
none /proc procfs rw 0 0\n\
none /dev devfs rw 0 0\n\
none /tmp tmpfs rw 0 0\n\
none /media/stick fat32 rw 0 0\n\
none /media/my\\040photos ext4 ro 0 0\n\
garbage\n\
none relative ext4 rw 0 0\n";

    #[test]
    fn the_drives_are_read_from_the_mount_table() {
        let mounts = parse_mounts(MOUNTS_TEXT);
        assert_eq!(
            mounts,
            [
                PathBuf::from("/"),
                PathBuf::from("/tmp"),
                PathBuf::from("/media/stick"),
                PathBuf::from("/media/my photos"),
            ],
            "a view of the kernel was taken for a drive, or an escape was not read"
        );
    }

    #[test]
    fn an_escape_is_read_as_its_byte_and_nothing_else_is() {
        assert_eq!(unescape_octal(b"a\\040b"), b"a b");
        assert_eq!(unescape_octal(b"tab\\011here"), b"tab\there");
        assert_eq!(unescape_octal(b"back\\134slash"), b"back\\slash");
        assert_eq!(unescape_octal(b"caf\\351"), b"caf\xe9");
        assert_eq!(unescape_octal(b"not\\08"), b"not\\08", "8 is not octal");
        assert_eq!(unescape_octal(b"end\\04"), b"end\\04");
        assert_eq!(unescape_octal(b"\\777"), b"\\777", "past a byte");
    }

    #[test]
    fn a_file_is_on_the_drive_mounted_nearest_it() {
        let mounts = parse_mounts(MOUNTS_TEXT);
        let on = |path: &str| longest_mount(&mounts, Path::new(path));
        assert_eq!(
            on("/media/stick/a.txt"),
            Some(PathBuf::from("/media/stick"))
        );
        assert_eq!(on("/media/stick2/a.txt"), Some(PathBuf::from("/")));
        assert_eq!(on("/home/u/a.txt"), Some(PathBuf::from("/")));
        assert_eq!(on("/tmp/x"), Some(PathBuf::from("/tmp")));
        assert_eq!(longest_mount(&[], Path::new("/a")), None);
    }
}
