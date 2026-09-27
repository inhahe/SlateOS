//! libmount's calls into libblkid's probing: a device's own tags
//! (`mnt_cache_read_tags`) and its filesystem type (`mnt_get_fstype`).
//! Evaluating a tag to a device is libblkid's too, and is
//! `ulblkid::evaluate` itself.
//!
//! **Probing** is `ulblkid`'s -- the port of libblkid's, every
//! superblock and partition-table prober -- called as libmount and
//! libblkid's cache call it. It reads the device, which needs read access
//! to it: for anyone but root it fails with `EACCES`, as upstream's does.

use std::path::PathBuf;
use std::rc::Rc;

pub use ulblkid::{PARTS_ENTRY_DETAILS, SUBLKS_LABEL, SUBLKS_SECTYPE, SUBLKS_TYPE, SUBLKS_UUID};

/// A path from bytes.
fn path_of(b: &[u8]) -> PathBuf {
    PathBuf::from(quoting::os_from_bytes(b))
}

/// `EINVAL`.
const EINVAL: i32 = 22;
/// `EIO`, for an error without an `errno`.
const EIO: i32 = 5;
/// `O_NONBLOCK`: Linux's value, which SlateOS's C library shares -- so a
/// FIFO named as a device does not hang the open.
#[cfg(unix)]
const O_NONBLOCK: i32 = 0o4000;

/// What a probe found: each value's name and its bytes, in libblkid's
/// order.
pub type Values = Vec<(&'static [u8], Vec<u8>)>;

/// Why a probe has nothing to report: libblkid's return codes, and the
/// `errno` it leaves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProbeFail {
    /// `open` failed (`EACCES` for a user who may not read the device).
    Open(i32),
    /// `blkid_probe_set_device` refused it: a directory, a character device
    /// other than UBI (`EINVAL`), or one whose size cannot be read.
    Device(i32),
    /// A read failed while probing (`-errno`).
    Io(i32),
    /// Nothing on it was recognised (`blkid_do_safeprobe` returned 1).
    Nothing,
    /// More than one prober claimed it (`-2`).
    Ambivalent,
}

impl ProbeFail {
    /// The `errno` the failure leaves, where it left one.
    #[must_use]
    pub fn errno(self) -> Option<i32> {
        match self {
            ProbeFail::Open(e) | ProbeFail::Device(e) | ProbeFail::Io(e) => Some(e),
            ProbeFail::Nothing | ProbeFail::Ambivalent => None,
        }
    }
}

/// The `errno` of an I/O error.
fn errno_of(e: &std::io::Error) -> i32 {
    e.raw_os_error().unwrap_or(EIO)
}

/// A device open for probing: `open(O_RDONLY|O_CLOEXEC|O_NONBLOCK)`.
///
/// # Errors
///
/// The `open`'s `errno`, as [`ProbeFail::Open`].
pub fn open_nonblock(devname: &[u8]) -> Result<std::fs::File, ProbeFail> {
    let mut opts = std::fs::OpenOptions::new();
    opts.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.custom_flags(O_NONBLOCK);
    }
    opts.open(path_of(devname))
        .map_err(|e| ProbeFail::Open(errno_of(&e)))
}

/// `blkid_probe_set_device(pr, fd, 0, 0)` on the device, the superblocks
/// chain with `sb_flags`, the partitions chain -- if `pt_flags` asks for it
/// -- with those, and `blkid_do_safeprobe`: every value found, in
/// libblkid's order, each string without its NUL. Probing is `ulblkid`'s,
/// the port of libblkid's.
///
/// # Errors
///
/// [`ProbeFail::Device`] when the device cannot be probed at all (a
/// directory, a character device other than UBI -- `EINVAL` -- or one
/// whose size cannot be read); [`ProbeFail::Io`] for a read that failed;
/// [`ProbeFail::Nothing`] or [`ProbeFail::Ambivalent`] for what
/// `blkid_do_safeprobe` found.
pub fn probe_file(
    file: std::fs::File,
    sb_flags: u32,
    pt_flags: Option<u32>,
) -> Result<Values, ProbeFail> {
    let mut pr = ulblkid::Probe::new();
    if pr.set_device(Some(Rc::new(file)), 0, 0) != 0 {
        return Err(ProbeFail::Device(if pr.errno != 0 {
            pr.errno
        } else {
            EINVAL
        }));
    }
    pr.enable_superblocks(true);
    pr.set_superblocks_flags(sb_flags);
    if let Some(flags) = pt_flags {
        pr.enable_partitions(true);
        pr.set_partitions_flags(flags);
    }
    match pr.do_safeprobe() {
        ulblkid::PROBE_OK => Ok(pr
            .values()
            .iter()
            .map(|v| (v.name.as_bytes(), v.as_c_str().to_vec()))
            .collect()),
        ulblkid::PROBE_NONE => Err(ProbeFail::Nothing),
        ulblkid::PROBE_AMBIGUOUS => Err(ProbeFail::Ambivalent),
        _ => Err(ProbeFail::Io(if pr.errno != 0 { pr.errno } else { EIO })),
    }
}

/// `mnt_cache_read_tags`' probe (`blkid_new_probe_from_filename`, then
/// superblocks with LABEL, UUID and TYPE, and partitions): the device's
/// LABEL, UUID, TYPE, PARTUUID and PARTLABEL (libblkid's `PART_ENTRY_UUID`
/// and `PART_ENTRY_NAME`), in that order, those it has.
///
/// # Errors
///
/// Why there are none (see [`ProbeFail`]).
pub fn probe_tags(devname: &[u8]) -> Result<Values, ProbeFail> {
    let file = open_nonblock(devname)?;
    let values = probe_file(
        file,
        SUBLKS_LABEL | SUBLKS_UUID | SUBLKS_TYPE,
        Some(PARTS_ENTRY_DETAILS),
    )?;
    // `tags[]` and `blktags[]`: libmount's names for libblkid's values.
    let names: [(&[u8], &'static [u8]); 5] = [
        (b"LABEL", b"LABEL"),
        (b"UUID", b"UUID"),
        (b"TYPE", b"TYPE"),
        (b"PART_ENTRY_UUID", b"PARTUUID"),
        (b"PART_ENTRY_NAME", b"PARTLABEL"),
    ];
    let mut tags = Vec::new();
    for (blk, name) in names {
        if let Some((_, v)) = values.iter().find(|(n, _)| *n == blk) {
            tags.push((name, v.clone()));
        }
    }
    Ok(tags)
}
