//! The part of util-linux 2.39.3's libmount that reads mount tables:
//! `/proc/self/mountinfo`, `/proc/mounts`, `/etc/fstab` (and
//! `/etc/fstab.d`), `/proc/swaps` and `/run/mount/utab`, into tables of
//! filesystems that can be searched, matched and walked as a tree.
//!
//! Ported file by file with upstream's names, for `findmnt` first:
//!
//! | Module | Upstream |
//! |---|---|
//! | [`mangle`] | `lib/mangle.c`: `\040` escapes in table fields |
//! | [`utils`] | `utils.c`, `lib/match.c`, `streq_paths`, tag strings |
//! | [`optmap`] | `optmap.c`: the options libmount knows by name |
//! | [`optstr`] | `optstr.c`: option strings, split and matched |
//! | [`fs`] | `fs.c`: one table line |
//! | [`tab`] | `tab.c`: a table, walked as a list or a tree, and searched |
//! | [`tab_parse`] | `tab_parse.c`: fstab, mountinfo, swaps and utab files |
//! | [`tab_diff`] | `tab_diff.c`: what changed between two readings |
//! | [`cache`] | `cache.c`, `lib/canonicalize.c`: canonical paths and tags |
//! | [`blkid`] | the part of libblkid a mount table needs: tags by udev link and by probing |
//! | [`blkid_cache`] | libblkid's device cache, `/run/blkid/blkid.tab` |
//! | [`udev`] | the part of libudev `findmnt` reads LABEL and UUID with |
//!
//! Upstream's quirks are kept where they show in what a program prints: a
//! pseudo filesystem's source is compared as bytes and any other's as a
//! path; `defaults` is no option at all once split; an option the kernel's
//! map knows as a flag but that carries a value is the filesystem's own.

pub mod blkid;
pub mod blkid_cache;
pub mod cache;
pub mod fs;
pub mod mangle;
pub mod optmap;
pub mod optstr;
pub mod tab;
pub mod tab_diff;
pub mod tab_parse;
pub mod udev;
pub mod utils;

/// A C string: `bytes` up to its first NUL.
#[must_use]
pub fn c_str(bytes: &[u8]) -> &[u8] {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    bytes.get(..end).unwrap_or_default()
}
