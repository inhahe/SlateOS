//! `lib/blkdev.c` and the other block-device `ioctl`s libblkid issues: a
//! device's size and sector size, zones, CD-ROM state, floppies, OPAL locks.
//!
//! Every request number is Linux's on x86-64, which SlateOS's C library
//! shares. A host without the call (the Windows build of the unit tests)
//! answers `ENOTTY` to all of them, as Linux answers for a regular file.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

/// `ENOTTY`: what a host without the calls answers.
#[cfg(not(unix))]
pub const ENOTTY: i32 = 25;

/// `BLKGETSIZE64`: `_IOR(0x12, 114, size_t)`.
pub const BLKGETSIZE64: u64 = 0x8008_1272;
/// `BLKGETSIZE`: `_IO(0x12, 96)`, the size in 512-byte sectors.
pub const BLKGETSIZE: u64 = 0x1260;
/// `BLKSSZGET`: `_IO(0x12, 104)`, the logical sector size.
pub const BLKSSZGET: u64 = 0x1268;
/// `BLKPBSZGET`: `_IO(0x12, 123)`, the physical sector size.
pub const BLKPBSZGET: u64 = 0x127b;
/// `BLKIOMIN`: `_IO(0x12, 120)`.
pub const BLKIOMIN: u64 = 0x1278;
/// `BLKIOOPT`: `_IO(0x12, 121)`.
pub const BLKIOOPT: u64 = 0x1279;
/// `BLKALIGNOFF`: `_IO(0x12, 122)`.
pub const BLKALIGNOFF: u64 = 0x127a;
/// `BLKGETZONESZ`: `_IOR(0x12, 132, __u32)`.
pub const BLKGETZONESZ: u64 = 0x8004_1284;
/// `BLKGETDISKSEQ`: `_IOR(0x12, 128, __u64)`.
pub const BLKGETDISKSEQ: u64 = 0x8008_1280;
/// `CDROM_GET_CAPABILITY`.
pub const CDROM_GET_CAPABILITY: u64 = 0x5331;
/// `CDROM_DRIVE_STATUS`.
pub const CDROM_DRIVE_STATUS: u64 = 0x5326;
/// `CDROM_LAST_WRITTEN`.
pub const CDROM_LAST_WRITTEN: u64 = 0x5395;
/// `CDROMMULTISESSION`.
pub const CDROMMULTISESSION: u64 = 0x5310;
/// `CDSL_CURRENT`: `(int)(~0U >> 1)`.
pub const CDSL_CURRENT: i64 = 0x7fff_ffff;
/// `CDS_NO_DISC`.
pub const CDS_NO_DISC: i32 = 1;
/// `CDS_TRAY_OPEN`.
pub const CDS_TRAY_OPEN: i32 = 2;
/// `CDROM_LBA`.
pub const CDROM_LBA: u8 = 0x01;
/// `FDGETFDCSTAT`: `_IOR(2, 0x15, struct floppy_fdc_state)`, 40 bytes on
/// x86-64.
pub const FDGETFDCSTAT: u64 = 0x8028_0215;
/// `FDGETPRM`: `_IOR(2, 0x04, struct floppy_struct)`, 32 bytes.
pub const FDGETPRM: u64 = 0x8020_0204;
/// `IOC_OPAL_GET_STATUS`: `_IOR('p', 236, struct opal_status)`.
pub const IOC_OPAL_GET_STATUS: u64 = 0x8008_70ec;
/// `OPAL_FL_LOCKED`.
pub const OPAL_FL_LOCKED: u32 = 0x8;
/// `ENOMEDIUM`.
pub const ENOMEDIUM: i32 = 123;

#[cfg(unix)]
mod imp {
    unsafe extern "C" {
        pub fn ioctl(fd: i32, request: u64, ...) -> i32;
    }
}

/// The raw descriptor of `f`, for an `ioctl`.
#[cfg(unix)]
fn raw(f: &File) -> i32 {
    use std::os::fd::AsRawFd;
    f.as_raw_fd()
}

/// `ioctl(fd, request, arg)` with `arg` pointing at `buf`: the call's
/// non-negative return, or `Err(errno)`.
///
/// # Safety
///
/// `request` must write at most `size_of::<T>()` bytes through its argument,
/// and read none it does not also write.
pub unsafe fn ioctl_ptr<T>(f: &File, request: u64, buf: &mut T) -> Result<i32, i32> {
    #[cfg(unix)]
    {
        // SAFETY: `buf` is live, writable memory of the size the caller
        // vouches `request` writes; the descriptor is open for the call.
        let rc = unsafe { imp::ioctl(raw(f), request, std::ptr::from_mut(buf).cast::<u8>()) };
        if rc < 0 {
            return Err(crate::errno_of(&std::io::Error::last_os_error()));
        }
        Ok(rc)
    }
    #[cfg(not(unix))]
    {
        let _ = (f, request, buf);
        Err(ENOTTY)
    }
}

/// `ioctl(fd, request, value)` for a request whose third argument is a
/// number, not a pointer (`CDROM_DRIVE_STATUS`), or is unused.
pub fn ioctl_val(f: &File, request: u64, value: i64) -> Result<i32, i32> {
    #[cfg(unix)]
    {
        // SAFETY: the requests this is called with take their argument by
        // value (or ignore it), so no memory is read or written through it.
        let rc = unsafe { imp::ioctl(raw(f), request, value) };
        if rc < 0 {
            return Err(crate::errno_of(&std::io::Error::last_os_error()));
        }
        Ok(rc)
    }
    #[cfg(not(unix))]
    {
        let _ = (f, request, value);
        Err(ENOTTY)
    }
}

/// `blkdev_valid_offset`: a byte can be read at `offset`.
fn valid_offset(f: &mut File, offset: u64) -> bool {
    if f.seek(SeekFrom::Start(offset)).is_err() {
        return false;
    }
    let mut ch = [0u8; 1];
    matches!(f.read(&mut ch), Ok(1))
}

/// `blkdev_find_size`: the size by binary search over readable offsets --
/// the fallback for a device no `ioctl` will size.
fn find_size(f: &mut File) -> Option<u64> {
    const MAX: u64 = i64::MAX.unsigned_abs();
    let mut low = 0u64;
    let mut high = 1024u64;
    while valid_offset(f, high) {
        if high == MAX {
            return None;
        }
        low = high;
        high = if high >= MAX / 2 {
            MAX
        } else {
            high.saturating_mul(2)
        };
    }
    while low < high.saturating_sub(1) {
        let mid = low.saturating_add(high) / 2;
        if valid_offset(f, mid) {
            low = mid;
        } else {
            high = mid;
        }
    }
    valid_offset(f, 0);
    Some(low.saturating_add(1))
}

/// `blkdev_get_size(fd, &bytes)`: `BLKGETSIZE64`, `BLKGETSIZE`,
/// `FDGETPRM`; a regular file's length; else the binary search.
///
/// # Errors
///
/// `ENOTBLK` for something that is neither a regular file nor a block
/// device; `EFBIG` when the search runs off the end of `off_t`.
pub fn get_size(f: &File) -> Result<u64, i32> {
    let mut bytes = 0u64;
    // SAFETY: BLKGETSIZE64 writes one u64.
    if unsafe { ioctl_ptr(f, BLKGETSIZE64, &mut bytes) }.is_ok() {
        return Ok(bytes);
    }
    let mut sectors: u64 = 0;
    // SAFETY: BLKGETSIZE writes one unsigned long, eight bytes here.
    if unsafe { ioctl_ptr(f, BLKGETSIZE, &mut sectors) }.is_ok() {
        return Ok(sectors << 9);
    }
    let mut floppy = [0u8; 32];
    // SAFETY: FDGETPRM writes a 32-byte `struct floppy_struct`.
    if unsafe { ioctl_ptr(f, FDGETPRM, &mut floppy) }.is_ok() {
        let size = u32::from_ne_bytes([floppy[0], floppy[1], floppy[2], floppy[3]]);
        return Ok(u64::from(size) << 9);
    }
    let meta = f.metadata().map_err(|e| crate::errno_of(&e))?;
    if meta.is_file() {
        return Ok(meta.len());
    }
    if !is_block(&meta) {
        const ENOTBLK: i32 = 15;
        return Err(ENOTBLK);
    }
    let mut dup = f.try_clone().map_err(|e| crate::errno_of(&e))?;
    const EFBIG: i32 = 27;
    find_size(&mut dup).ok_or(EFBIG)
}

/// `blkdev_get_sector_size(fd, &size)`: `BLKSSZGET`.
///
/// # Errors
///
/// The `ioctl`'s `errno`.
pub fn get_sector_size(f: &File) -> Result<u32, i32> {
    let mut sz: i32 = 0;
    // SAFETY: BLKSSZGET writes one int.
    unsafe { ioctl_ptr(f, BLKSSZGET, &mut sz) }?;
    // A negative size is the kernel's to report; it is reinterpreted as
    // upstream's `unsigned int` field reinterprets it.
    #[allow(clippy::cast_sign_loss, reason = "C stores the int in an unsigned int")]
    Ok(sz as u32)
}

/// `S_ISBLK`.
#[must_use]
pub fn is_block(meta: &std::fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileTypeExt;
        meta.file_type().is_block_device()
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        false
    }
}

/// `S_ISCHR`.
#[must_use]
pub fn is_char(meta: &std::fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileTypeExt;
        meta.file_type().is_char_device()
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        false
    }
}

/// `st_rdev`.
#[must_use]
pub fn rdev(meta: &std::fs::Metadata) -> u64 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        meta.rdev()
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        0
    }
}

/// `st_mode`.
#[must_use]
pub fn mode(meta: &std::fs::Metadata) -> u32 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        meta.mode()
    }
    #[cfg(not(unix))]
    {
        if meta.is_dir() {
            crate::S_IFDIR
        } else {
            crate::S_IFREG
        }
    }
}
