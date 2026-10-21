//! What a program asks of a file beyond its contents and status: its
//! extended attributes (`llistxattr`, `lgetxattr`), the type of the file
//! system it lives on (`statfs`'s `f_type`), and the two ioctls that clone
//! a file's extents into another (`FICLONE`) and map them
//! (`FS_IOC_FIEMAP`) -- through the linked C library, as `hardlink` asks
//! them.
//!
//! Each call is shaped as C's: a buffer of `None` asks only how much room
//! the answer needs, as a null buffer of size 0 does there. Off Unix every
//! call answers [`ENOSYS`](crate::ENOSYS).

#[cfg(not(unix))]
use crate::ENOSYS;
#[cfg(unix)]
use crate::last_errno;
use core::ffi::CStr;

/// `FICLONE`: `_IOW(0x94, 9, int)`.
pub const FICLONE: u64 = 0x4004_9409;
/// `FS_IOC_FIEMAP`: `_IOWR('f', 11, struct fiemap)`.
pub const FS_IOC_FIEMAP: u64 = 0xc020_660b;
/// `FIEMAP_FLAG_SYNC`: write the file out before mapping it.
pub const FIEMAP_FLAG_SYNC: u32 = 0x1;
/// `FIEMAP_EXTENT_LAST`: the file's last extent.
pub const FIEMAP_EXTENT_LAST: u32 = 0x1;
/// `FIEMAP_EXTENT_SHARED`: an extent another file holds too.
pub const FIEMAP_EXTENT_SHARED: u32 = 0x2000;

/// `sizeof (struct fiemap)` and `sizeof (struct fiemap_extent)`, in
/// 8-byte words.
const FIEMAP_HEADER_WORDS: usize = 4;
const FIEMAP_EXTENT_WORDS: usize = 7;

#[cfg(unix)]
mod sys {
    use core::ffi::c_char;

    unsafe extern "C" {
        pub fn llistxattr(path: *const c_char, list: *mut c_char, size: usize) -> isize;
        pub fn lgetxattr(
            path: *const c_char,
            name: *const c_char,
            value: *mut core::ffi::c_void,
            size: usize,
        ) -> isize;
        pub fn statfs(path: *const c_char, buf: *mut i64) -> i32;
        // Variadic, as C declares it; see `pty.rs` on why one declaration
        // serves both our library and glibc.
        pub fn ioctl(fd: i32, request: u64, ...) -> i32;
    }
}

/// `llistxattr (path, list, size)`: the names of the extended attributes
/// of `path` (not following a symbolic link), each with its NUL, into
/// `list` -- or, for `None`, only how many bytes they take.
///
/// # Errors
///
/// `ENOTSUP` where the file system keeps none, `ERANGE` when `list` is too
/// short, `ENOENT` and the rest; [`ENOSYS`] off Unix.
pub fn llistxattr(path: &CStr, list: Option<&mut [u8]>) -> Result<usize, i32> {
    #[cfg(unix)]
    {
        let (ptr, size) = list.map_or((core::ptr::null_mut(), 0), |l| (l.as_mut_ptr(), l.len()));
        // SAFETY: `path` is NUL-terminated; `ptr` is null with a size of 0,
        // or a buffer writable for `size` bytes, which is what the call is
        // told and all it writes.
        let n = unsafe { sys::llistxattr(path.as_ptr(), ptr.cast(), size) };
        usize::try_from(n).map_err(|_| last_errno())
    }
    #[cfg(not(unix))]
    {
        let _ = (path, list);
        Err(ENOSYS)
    }
}

/// `lgetxattr (path, name, value, size)`: the value of the extended
/// attribute `name` of `path` (not following a symbolic link) into `value`
/// -- or, for `None`, only its length.
///
/// # Errors
///
/// `ENODATA` for an attribute it has not, `ERANGE` when `value` is too
/// short, `ENOTSUP`; [`ENOSYS`] off Unix.
pub fn lgetxattr(path: &CStr, name: &CStr, value: Option<&mut [u8]>) -> Result<usize, i32> {
    #[cfg(unix)]
    {
        let (ptr, size) = value.map_or((core::ptr::null_mut(), 0), |v| (v.as_mut_ptr(), v.len()));
        // SAFETY: both strings are NUL-terminated; `ptr` is null with a size
        // of 0, or a buffer writable for `size` bytes, which is what the call
        // is told and all it writes.
        let n = unsafe { sys::lgetxattr(path.as_ptr(), name.as_ptr(), ptr.cast(), size) };
        usize::try_from(n).map_err(|_| last_errno())
    }
    #[cfg(not(unix))]
    {
        let _ = (path, name, value);
        Err(ENOSYS)
    }
}

/// `statfs (path, &vfs)`'s `f_type`: the magic number of the file system
/// `path` lives on (`0xef53` for ext4, `0x9123683e` for btrfs).
///
/// # Errors
///
/// What `statfs` reports: `ENOENT`, `EACCES` and the rest; [`ENOSYS`] off
/// Unix.
pub fn statfs_type(path: &CStr) -> Result<i64, i32> {
    #[cfg(unix)]
    {
        // `struct statfs` is 120 bytes on x86-64 Linux, `f_type` its first
        // word; this is room to spare.
        let mut buf = [0i64; 32];
        // SAFETY: `path` is NUL-terminated and `buf` is writable for 256
        // bytes, more than the structure the call fills.
        if unsafe { sys::statfs(path.as_ptr(), buf.as_mut_ptr()) } != 0 {
            return Err(last_errno());
        }
        Ok(buf.first().copied().unwrap_or(0))
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Err(ENOSYS)
    }
}

/// `ioctl (dest, FICLONE, src)`: `dest`'s contents made `src`'s, sharing
/// its extents.
///
/// # Errors
///
/// `EOPNOTSUPP` or `EINVAL` where the file system cannot, `EXDEV` across
/// file systems, `EBADF`; [`ENOSYS`] off Unix.
pub fn ficlone(dest: i32, src: i32) -> Result<(), i32> {
    #[cfg(unix)]
    {
        // SAFETY: `FICLONE` takes the source descriptor as an `int`, by
        // value; nothing is read or written through it.
        if unsafe { sys::ioctl(dest, FICLONE, src) } != 0 {
            return Err(last_errno());
        }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = (dest, src);
        Err(ENOSYS)
    }
}

/// One `struct fiemap_extent`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Extent {
    /// `fe_logical`: where it starts in the file.
    pub logical: u64,
    /// `fe_physical`: where it starts on the device.
    pub physical: u64,
    /// `fe_length`.
    pub length: u64,
    /// `fe_flags`: `FIEMAP_EXTENT_LAST`, `FIEMAP_EXTENT_SHARED` and the rest.
    pub flags: u32,
}

/// `ioctl (fd, FS_IOC_FIEMAP, map)`, `map` a `struct fiemap` of
/// `map.len() * 8` bytes -- as many extents as fit after the header --
/// asking for those from `start` on, all of them (`fm_length` all ones),
/// with `flags`: how many extents the file system answered with
/// (`fm_mapped_extents`), each then read by [`fiemap_extent`].
///
/// # Errors
///
/// `EOPNOTSUPP` where the file system cannot map, `EBADF`; [`ENOSYS`] off
/// Unix.
pub fn fiemap(fd: i32, start: u64, flags: u32, map: &mut [u64]) -> Result<usize, i32> {
    let count = map
        .len()
        .saturating_sub(FIEMAP_HEADER_WORDS)
        .checked_div(FIEMAP_EXTENT_WORDS)
        .unwrap_or(0);
    // fm_start, fm_length, then fm_flags and fm_mapped_extents in one
    // word, fm_extent_count and fm_reserved in the next.
    let header = [
        start,
        u64::MAX,
        u64::from(flags),
        u64::try_from(count).unwrap_or(0),
    ];
    let Some(head) = map.get_mut(..FIEMAP_HEADER_WORDS) else {
        return Err(crate::EINVAL);
    };
    head.copy_from_slice(&header);
    #[cfg(unix)]
    {
        // SAFETY: `map` is a `struct fiemap`, 8-byte aligned, with room for
        // `count` extents after its header, which is what `fm_extent_count`
        // tells the call; it writes within that.
        if unsafe { sys::ioctl(fd, FS_IOC_FIEMAP, map.as_mut_ptr()) } != 0 {
            return Err(last_errno());
        }
        // fm_mapped_extents: the high half of the third word.
        let mapped = map.get(2).map_or(0, |w| w >> 32);
        Ok(usize::try_from(mapped).unwrap_or(0).min(count))
    }
    #[cfg(not(unix))]
    {
        let _ = fd;
        Err(ENOSYS)
    }
}

/// Extent `i` of a map [`fiemap`] filled; `None` past its room.
#[must_use]
pub fn fiemap_extent(map: &[u64], i: usize) -> Option<Extent> {
    let base = i
        .checked_mul(FIEMAP_EXTENT_WORDS)?
        .checked_add(FIEMAP_HEADER_WORDS)?;
    let words = map.get(base..base.checked_add(FIEMAP_EXTENT_WORDS)?)?;
    let [logical, physical, length, _, _, flags, _] = *words else {
        return None;
    };
    Some(Extent {
        logical,
        physical,
        length,
        // After two reserved words: fe_flags, the low half.
        flags: u32::try_from(flags & 0xffff_ffff).unwrap_or(0),
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    extern crate std;
    #[cfg(target_os = "linux")]
    use std::format;

    use super::*;

    const FIEMAP_SHARED_LAST: u32 = FIEMAP_EXTENT_SHARED | FIEMAP_EXTENT_LAST;

    #[test]
    fn the_numbers_are_linux_s() {
        // `_IOW`/`_IOWR`: direction, size, type, number.
        let io =
            |dir: u64, ty: u64, nr: u64, size: u64| (dir << 30) | (size << 16) | (ty << 8) | nr;
        assert_eq!(FICLONE, io(1, 0x94, 9, 4));
        assert_eq!(FS_IOC_FIEMAP, io(3, u64::from(b'f'), 11, 32));
        let mut map = [0u64; FIEMAP_HEADER_WORDS + 2 * FIEMAP_EXTENT_WORDS];
        map[FIEMAP_HEADER_WORDS + FIEMAP_EXTENT_WORDS] = 7;
        map[FIEMAP_HEADER_WORDS + FIEMAP_EXTENT_WORDS + 5] = 0x1_0000_2001;
        let e = fiemap_extent(&map, 1).unwrap();
        assert_eq!((e.logical, e.flags), (7, FIEMAP_SHARED_LAST));
        assert!(fiemap_extent(&map, 2).is_none(), "past the map's room");
    }

    /// A file of two extents, a hole between them, mapped as two -- where
    /// the file system maps at all (tmpfs does not).
    #[cfg(target_os = "linux")]
    #[test]
    fn a_sparse_file_maps_as_its_extents() {
        use std::io::{Seek, SeekFrom, Write};
        use std::os::fd::AsRawFd;
        let dir = std::env::temp_dir().join(format!("libcall-fiemap-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut f = std::fs::File::create(dir.join("sparse")).unwrap();
        f.write_all(&[1u8; 4096]).unwrap();
        f.seek(SeekFrom::Start(1 << 20)).unwrap();
        f.write_all(&[2u8; 4096]).unwrap();
        let mut map = [0u64; FIEMAP_HEADER_WORDS + 8 * FIEMAP_EXTENT_WORDS];
        match fiemap(f.as_raw_fd(), 0, FIEMAP_FLAG_SYNC, &mut map) {
            Ok(n) => {
                assert_eq!(n, 2, "two extents and a hole");
                let second = fiemap_extent(&map, 1).unwrap();
                assert_eq!(second.logical, 1 << 20);
                assert_ne!(second.flags & FIEMAP_EXTENT_LAST, 0);
            }
            Err(e) => assert_eq!(
                e,
                crate::EOPNOTSUPP,
                "only a file system that cannot map may refuse"
            ),
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_file_without_attributes_lists_none_and_has_a_file_system() {
        let dir = std::env::temp_dir().join(format!("libcall-xattr-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("f");
        std::fs::write(&file, b"x").unwrap();
        let c = std::ffi::CString::new(file.to_str().unwrap()).unwrap();
        assert!(matches!(llistxattr(&c, None), Ok(0) | Err(_)));
        assert!(statfs_type(&c).unwrap() != 0);
        let name = std::ffi::CString::new("user.nosuch").unwrap();
        assert!(lgetxattr(&c, &name, None).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
