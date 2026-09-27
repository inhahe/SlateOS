//! util-linux 2.39.3's `lib/path.c` and `lib/sysfs.c`: the directory a block
//! device has under `/sys/dev/block/MAJ:MIN`, its attributes read the way
//! util-linux reads them, and the conversions between device numbers, kernel
//! names and `/dev` paths that libblkid, libmount and lsblk all make.
//!
//! Upstream this is `libcommon`, linked into every util-linux library and
//! program; here it is one crate for the same reason. Before it, `ulmount`
//! carried a private `devno_to_devpath` and libblkid's cache a private
//! `sysfs_devname_to_devno`, `devno_is_dm_private` and friends, each a
//! partial copy of the functions below.
//!
//! # The two layers
//!
//! * [`PathCxt`] is `struct path_cxt`: a directory, an optional prefix it is
//!   read under (for tests against a fake `/sys`), and relative reads inside
//!   it -- `ul_path_read_string`, `ul_path_read_u64`, `ul_path_access`,
//!   `ul_path_readlink`, `ul_path_opendir` -- with upstream's return
//!   conventions: `fscanf`-shaped numbers, a trailing newline dropped, 8191
//!   bytes at most for a string.
//! * The `sysfs_blkdev_*` dialect, which [`new_sysfs_path`] attaches: the
//!   device number, and a parent directory (a partition's whole disk) that a
//!   read falls back to when the file is not in the device's own directory --
//!   how `/sys/dev/block/8:1/queue/logical_block_size`, which a partition does
//!   not have, reads its disk's.
//!
//! Upstream holds the directory open and uses `openat` and friends; this port
//! joins the relative path onto the directory's name instead. The difference
//! shows only if the directory is renamed between two reads, which sysfs
//! directories are not.
//!
//! Device numbers are glibc's 64-bit encoding ([`makedev`], [`major`],
//! [`minor`]), which is also SlateOS's C library's.

use std::path::PathBuf;

mod path;
mod sysfs;

#[cfg(test)]
mod tests;

pub use path::{DirEntry, PathCxt, read_all};
pub use sysfs::{
    chrdev_devno_to_devname, devname_dev_to_sys, devname_is_hidden, devname_sys_to_dev,
    devname_to_devno, devname_to_devno_in, devno_count_partitions, devno_is_dm_private,
    devno_is_wholedisk, devno_to_devname, devno_to_devpath, devno_to_wholedisk,
    is_partition_dirent, new_sysfs_path, stripoff_last_component,
};

/// `PATH_MAX`.
pub const PATH_MAX: usize = 4096;
/// `BUFSIZ`: the most `ul_path_read_string` reads, less one.
pub const BUFSIZ: usize = 8192;

/// `_PATH_SYS_BLOCK`.
pub const PATH_SYS_BLOCK: &[u8] = b"/sys/block";
/// `_PATH_SYS_DEVBLOCK`.
pub const PATH_SYS_DEVBLOCK: &[u8] = b"/sys/dev/block";
/// `_PATH_SYS_DEVCHAR`.
pub const PATH_SYS_DEVCHAR: &[u8] = b"/sys/dev/char";

/// `ENOENT`.
pub const ENOENT: i32 = 2;
/// `ENOMEM`.
pub const ENOMEM: i32 = 12;
/// `EINVAL`.
pub const EINVAL: i32 = 22;
/// `ENAMETOOLONG`.
pub const ENAMETOOLONG: i32 = 36;
/// `EIO`, for an I/O error that carries no `errno`.
pub const EIO: i32 = 5;

/// `F_OK` for [`PathCxt::access`].
pub const F_OK: i32 = 0;
/// `R_OK` for [`PathCxt::access`].
pub const R_OK: i32 = 4;

/// glibc's `makedev(major, minor)`.
#[must_use]
pub fn makedev(major: u32, minor: u32) -> u64 {
    let (ma, mi) = (u64::from(major), u64::from(minor));
    ((ma & 0x0000_0fff) << 8)
        | ((ma & 0xffff_f000) << 32)
        | (mi & 0x0000_00ff)
        | ((mi & 0xffff_ff00) << 12)
}

/// glibc's `major(dev)`.
#[must_use]
pub fn major(dev: u64) -> u32 {
    let m = ((dev >> 8) & 0x0000_0fff) | ((dev >> 32) & 0xffff_f000);
    // Both halves are masked into 32 bits, so the value fits.
    u32::try_from(m).unwrap_or(u32::MAX)
}

/// glibc's `minor(dev)`.
#[must_use]
pub fn minor(dev: u64) -> u32 {
    let m = (dev & 0x0000_00ff) | ((dev >> 12) & 0xffff_ff00);
    u32::try_from(m).unwrap_or(u32::MAX)
}

/// `"%d:%d"` of a device number: its two `unsigned int` halves printed
/// signed, as util-linux prints them.
#[must_use]
pub fn majmin(dev: u64) -> String {
    // The `as` casts are C's: an `unsigned int` passed to `%d`.
    #[allow(clippy::cast_possible_wrap, reason = "C prints the unsigned halves with %d")]
    let (ma, mi) = (major(dev) as i32, minor(dev) as i32);
    format!("{ma}:{mi}")
}

/// A path from bytes.
#[must_use]
pub fn path_of(b: &[u8]) -> PathBuf {
    PathBuf::from(quoting::os_from_bytes(b))
}

/// The `errno` of an I/O error.
#[must_use]
pub fn errno_of(e: &std::io::Error) -> i32 {
    e.raw_os_error().unwrap_or(EIO)
}

/// A C string: `b` up to its first NUL.
#[must_use]
pub fn c_str(b: &[u8]) -> &[u8] {
    b.iter()
        .position(|&c| c == 0)
        .map_or(b, |n| b.get(..n).unwrap_or_default())
}
