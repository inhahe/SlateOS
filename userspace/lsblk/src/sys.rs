//! The C library calls `lsblk` makes that Rust's standard library does not
//! wrap, reached through the C ABI as the one-libc rule requires
//! (design-decisions §768): `getuid`, the user and group names of a device
//! node's owner, `statvfs` for the FSSIZE/FSAVAIL/FSUSED/FSUSE% columns, and
//! the `BLKROGET` ioctl for a read-only device without a `ro` attribute.
//!
//! On a host that is not Unix none of these exist: nobody is root, no user
//! or group has a name, `statvfs` fails with `ENOSYS` (the columns are then
//! empty, as upstream's are when it fails) and the ioctl fails. The
//! differential harness runs under WSL, where `cfg(unix)` holds.

/// What `lsblk` reads of `struct stat`: `st_rdev`, `st_uid`, `st_gid` and
/// `st_mode`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stat {
    pub rdev: u64,
    pub uid: u32,
    pub gid: u32,
    pub mode: u32,
}

/// `stat(path, &st)`, following links.
pub fn stat(path: &[u8]) -> Option<Stat> {
    let m = std::fs::metadata(quoting::os_from_bytes(path)).ok()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Some(Stat {
            rdev: m.rdev(),
            uid: m.uid(),
            gid: m.gid(),
            mode: m.mode(),
        })
    }
    #[cfg(not(unix))]
    {
        let _ = m;
        Some(Stat::default())
    }
}

/// What `lsblk` reads of `struct statvfs`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Statvfs {
    pub f_frsize: u64,
    pub f_blocks: u64,
    pub f_bfree: u64,
    pub f_bavail: u64,
}

#[cfg(unix)]
mod imp {
    use std::ffi::c_char;

    /// The first field of `struct passwd` and of `struct group`, which both
    /// C libraries put first.
    #[repr(C)]
    pub struct Named {
        pub name: *const c_char,
    }

    /// `struct statvfs` as glibc and SlateOS's C library lay it out on
    /// x86_64: the fields read here come first but for `f_bsize`, and the
    /// tail is generous, since `statvfs` takes no length and a buffer
    /// shorter than the C library's struct would be written past.
    #[repr(C)]
    #[derive(Default)]
    pub struct CStatvfs {
        pub f_bsize: u64,
        pub f_frsize: u64,
        pub f_blocks: u64,
        pub f_bfree: u64,
        pub f_bavail: u64,
        pub tail: [u64; 16],
    }

    unsafe extern "C" {
        pub fn access(path: *const u8, mode: i32) -> i32;
        pub fn getuid() -> u32;
        pub fn getpwuid(uid: u32) -> *const Named;
        pub fn getgrgid(gid: u32) -> *const Named;
        pub fn statvfs(path: *const u8, buf: *mut CStatvfs) -> i32;
        pub fn ioctl(fd: i32, request: u64, arg: *mut i32) -> i32;
    }
}

/// `access(path, R_OK)`: `Err(errno)` when the caller may not read it.
pub fn access_r(path: &[u8]) -> Result<(), i32> {
    if path.contains(&0) {
        return Err(2);
    }
    #[cfg(unix)]
    {
        const R_OK: i32 = 4;
        let mut c = path.to_vec();
        c.push(0);
        // SAFETY: `c` is NUL-terminated and outlives the call, which only
        // reads it.
        if unsafe { imp::access(c.as_ptr(), R_OK) } == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error().raw_os_error().unwrap_or(5))
        }
    }
    #[cfg(not(unix))]
    {
        // No permission bits to ask about: existence is the answer.
        std::fs::metadata(quoting::os_from_bytes(path))
            .map(drop)
            .map_err(|e| e.raw_os_error().unwrap_or(2))
    }
}

/// `getuid()`.
#[must_use]
pub fn getuid() -> u32 {
    #[cfg(unix)]
    {
        // SAFETY: getuid takes nothing and cannot fail.
        unsafe { imp::getuid() }
    }
    #[cfg(not(unix))]
    {
        u32::MAX
    }
}

/// The name a `getpwuid`/`getgrgid` entry carries first, copied out.
#[cfg(unix)]
fn name_of(entry: *const imp::Named) -> Option<Vec<u8>> {
    if entry.is_null() {
        return None;
    }
    // SAFETY: `entry` is non-null and points to the C library's static
    // entry, valid until the next such call, which comes after this copy.
    let name = unsafe { (*entry).name };
    if name.is_null() {
        return None;
    }
    // SAFETY: the name is a NUL-terminated string owned by the entry.
    Some(
        unsafe { std::ffi::CStr::from_ptr(name) }
            .to_bytes()
            .to_vec(),
    )
}

/// `getpwuid(uid)->pw_name`.
#[must_use]
pub fn user_name(uid: u32) -> Option<Vec<u8>> {
    #[cfg(unix)]
    {
        // SAFETY: getpwuid returns null or a pointer to a static entry,
        // which `name_of` reads at once.
        name_of(unsafe { imp::getpwuid(uid) })
    }
    #[cfg(not(unix))]
    {
        let _ = uid;
        None
    }
}

/// `getgrgid(gid)->gr_name`.
#[must_use]
pub fn group_name(gid: u32) -> Option<Vec<u8>> {
    #[cfg(unix)]
    {
        // SAFETY: getgrgid returns null or a pointer to a static entry,
        // which `name_of` reads at once.
        name_of(unsafe { imp::getgrgid(gid) })
    }
    #[cfg(not(unix))]
    {
        let _ = gid;
        None
    }
}

/// `statvfs(path, &buf)`; `None` when it fails (or the path holds a NUL,
/// which C would have cut there -- no mount point holds one).
#[must_use]
pub fn stat_vfs(path: &[u8]) -> Option<Statvfs> {
    if path.contains(&0) {
        return None;
    }
    #[cfg(unix)]
    {
        let mut c = path.to_vec();
        c.push(0);
        let mut buf = imp::CStatvfs::default();
        // SAFETY: `c` is NUL-terminated and outlives the call; `buf` is a
        // live, writable allocation at least as large as the C library's
        // `struct statvfs` (see `CStatvfs`'s tail).
        let rc = unsafe { imp::statvfs(c.as_ptr(), &raw mut buf) };
        if rc != 0 {
            return None;
        }
        Some(Statvfs {
            f_frsize: buf.f_frsize,
            f_blocks: buf.f_blocks,
            f_bfree: buf.f_bfree,
            f_bavail: buf.f_bavail,
        })
    }
    #[cfg(not(unix))]
    {
        None
    }
}

/// `BLKROGET`: `_IO(0x12, 94)`.
#[cfg(unix)]
const BLKROGET: u64 = 0x125e;

/// `open(path, O_RDONLY)` and `ioctl(fd, BLKROGET, &ro)`: the device's
/// read-only flag, or `None` when the open or the ioctl fails.
#[must_use]
pub fn blkroget(path: &[u8]) -> Option<i32> {
    let f = std::fs::File::open(quoting::os_from_bytes(path)).ok()?;
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        let mut ro: i32 = 0;
        // SAFETY: BLKROGET writes one int through the pointer, which `ro`
        // is for the whole call; the descriptor is open for it.
        let rc = unsafe { imp::ioctl(f.as_raw_fd(), BLKROGET, &raw mut ro) };
        (rc == 0).then_some(ro)
    }
    #[cfg(not(unix))]
    {
        let _ = f;
        None
    }
}
