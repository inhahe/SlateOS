//! util-linux 2.39.3's `lib/loopdev.c`: a loop device -- what is known of
//! it, the iteration over the used or free ones, and setting one up,
//! detaching it and changing it -- as `losetup` and the libraries ask.
//!
//! [`LoopCxt`] is `struct loopdev_cxt`. `loopcxt_init` decides once how it
//! learns things: from sysfs when `/sys/block` is a directory -- and then,
//! on any kernel since 2.6.37, never through the `LOOP_GET_STATUS64` ioctl
//! for what sysfs says (`LOOPDEV_FL_NOIOCTL`) -- and through the ioctl when
//! there is no sysfs. The kernel-version half of that test is taken as
//! passed: no kernel this runs on with a `/sys/block` predates 2.6.37.
//!
//! Where upstream reports a failure through `errno` -- which `losetup`
//! then prints -- the methods here return it, as the `Err` of a `Result`:
//! the `errno` upstream's caller would find.
//!
//! On SlateOS the loop ioctls do not reach a kernel that has loop devices,
//! and there is no `/sys/block`: every device is "not a loop device".

use crate::{PathCxt, path_of};
use std::fs::File;
#[cfg(unix)]
use std::os::fd::AsRawFd;

/// `ENOENT`, `EINVAL`, `ENODEV`, `EBUSY`, `EAGAIN`, `ENOTTY`, `ENOSYS`,
/// `ERANGE`, `EACCES`, `EROFS`.
const ENOENT: i32 = 2;
const EINVAL: i32 = 22;
const ENODEV: i32 = 19;
const EBUSY: i32 = 16;
const EAGAIN: i32 = 11;
const ENOTTY: i32 = 25;
const ENOSYS: i32 = 38;
const ERANGE: i32 = 34;
const EACCES: i32 = 13;
const EROFS: i32 = 30;

/// `LO_CRYPT_NONE`: no (long-deprecated) encryption.
pub const LO_CRYPT_NONE: u32 = 0;

/// `LO_FLAGS_*`: the kernel's flags of a loop device.
pub const LO_FLAGS_READ_ONLY: u32 = 1;
/// `LO_FLAGS_AUTOCLEAR`: detached when its last user closes it.
pub const LO_FLAGS_AUTOCLEAR: u32 = 4;
/// `LO_FLAGS_PARTSCAN`: its partition table read.
pub const LO_FLAGS_PARTSCAN: u32 = 8;
/// `LO_FLAGS_DIRECT_IO`: the backing file read and written with `O_DIRECT`.
pub const LO_FLAGS_DIRECT_IO: u32 = 16;

/// `LO_NAME_SIZE`: the bytes `struct loop_info64` keeps of a file's name.
pub const LO_NAME_SIZE: usize = 64;

/// `LOOPDEV_MAJOR`: the loop driver's major number.
pub const LOOPDEV_MAJOR: u32 = 7;
/// `LOOPDEV_DEFAULT_NNODES`: the loop devices tried first, `loop0` to `loop7`.
const LOOPDEV_DEFAULT_NNODES: i32 = 8;
/// `LOOPDEV_MAX_TRIES`: how often an ioctl answering `EAGAIN` is tried again.
const LOOPDEV_MAX_TRIES: u32 = 10;

/// `LOOPITER_FL_FREE` and `LOOPITER_FL_USED`: what the iterator looks for.
pub const LOOPITER_FL_FREE: i32 = 1;
/// See [`LOOPITER_FL_FREE`].
pub const LOOPITER_FL_USED: i32 = 2;

/// `LOOPDEV_FL_*`: how the context works.
pub const LOOPDEV_FL_RDONLY: i32 = 1;
/// The device opened read-write, which setting it up needs.
pub const LOOPDEV_FL_RDWR: i32 = 1 << 1;
/// `LOOPDEV_FL_OFFSET`: an offset was given (to [`LoopCxt::is_used`]).
pub const LOOPDEV_FL_OFFSET: i32 = 1 << 4;
/// `LOOPDEV_FL_NOSYSFS`: there is no `/sys/block`.
pub const LOOPDEV_FL_NOSYSFS: i32 = 1 << 5;
/// `LOOPDEV_FL_NOIOCTL`: sysfs answers what the ioctls would.
pub const LOOPDEV_FL_NOIOCTL: i32 = 1 << 6;
/// `LOOPDEV_FL_DEVSUBDIR`: the nodes are `/dev/loop/N`.
pub const LOOPDEV_FL_DEVSUBDIR: i32 = 1 << 7;
/// `LOOPDEV_FL_CONTROL`: there is a `/dev/loop-control`.
pub const LOOPDEV_FL_CONTROL: i32 = 1 << 8;
/// `LOOPDEV_FL_SIZELIMIT`: a size limit was given (with an offset).
pub const LOOPDEV_FL_SIZELIMIT: i32 = 1 << 9;

/// The ioctls: `LOOP_*` on a loop device, `LOOP_CTL_*` on
/// `/dev/loop-control`, and the block device's own two.
#[cfg(unix)]
mod nr {
    pub const LOOP_SET_FD: u64 = 0x4C00;
    pub const LOOP_CLR_FD: u64 = 0x4C01;
    pub const LOOP_SET_STATUS64: u64 = 0x4C04;
    pub const LOOP_GET_STATUS64: u64 = 0x4C05;
    pub const LOOP_SET_CAPACITY: u64 = 0x4C07;
    pub const LOOP_SET_DIRECT_IO: u64 = 0x4C08;
    pub const LOOP_SET_BLOCK_SIZE: u64 = 0x4C09;
    pub const LOOP_CONFIGURE: u64 = 0x4C0A;
    pub const LOOP_CTL_ADD: u64 = 0x4C80;
    pub const LOOP_CTL_GET_FREE: u64 = 0x4C82;
    /// `BLKSSZGET`: the logical sector size, an `int`.
    pub const BLKSSZGET: u64 = 0x1268;
    /// `BLKGETSIZE64`: the size in bytes, a `uint64_t`.
    pub const BLKGETSIZE64: u64 = 0x8008_1272;
}

#[cfg(unix)]
mod sys {
    unsafe extern "C" {
        // Variadic, as C declares it: the third argument is a pointer, an
        // `int` or an `unsigned long` by request.
        pub fn ioctl(fd: i32, request: u64, ...) -> i32;
    }
}

/// The calling thread's `errno`.
#[cfg(unix)]
fn last_errno() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(EINVAL)
}

/// `struct loop_info64`: 232 bytes, as the kernel lays it out.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct LoopInfo64 {
    /// `lo_device`: the backing file's device.
    pub lo_device: u64,
    /// `lo_inode`: the backing file's inode.
    pub lo_inode: u64,
    pub lo_rdevice: u64,
    /// `lo_offset`: where in the file the device starts.
    pub lo_offset: u64,
    /// `lo_sizelimit`: how much of it the device has, 0 for all.
    pub lo_sizelimit: u64,
    pub lo_number: u32,
    /// `lo_encrypt_type`: [`LO_CRYPT_NONE`] or a deprecated cipher.
    pub lo_encrypt_type: u32,
    pub lo_encrypt_key_size: u32,
    /// `lo_flags`: `LO_FLAGS_*`.
    pub lo_flags: u32,
    /// `lo_file_name`: the start of the backing file's name.
    pub lo_file_name: [u8; LO_NAME_SIZE],
    /// `lo_crypt_name`.
    pub lo_crypt_name: [u8; LO_NAME_SIZE],
    pub lo_encrypt_key: [u8; 32],
    pub lo_init: [u64; 2],
}

impl Default for LoopInfo64 {
    fn default() -> Self {
        LoopInfo64 {
            lo_device: 0,
            lo_inode: 0,
            lo_rdevice: 0,
            lo_offset: 0,
            lo_sizelimit: 0,
            lo_number: 0,
            lo_encrypt_type: 0,
            lo_encrypt_key_size: 0,
            lo_flags: 0,
            lo_file_name: [0; LO_NAME_SIZE],
            lo_crypt_name: [0; LO_NAME_SIZE],
            lo_encrypt_key: [0; 32],
            lo_init: [0; 2],
        }
    }
}

/// `struct loop_config`: what `LOOP_CONFIGURE` takes, 304 bytes.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct LoopConfig {
    /// `fd`: the backing file.
    pub fd: u32,
    /// `block_size`: the logical sector size, 0 for the default.
    pub block_size: u32,
    /// `info`.
    pub info: LoopInfo64,
    reserved: [u64; 8],
}

/// `struct loopdev_iter`: where an iteration over loop devices is.
#[derive(Debug, Default)]
struct LoopIter {
    /// `/proc/partitions`, read whole when first needed, and the next line.
    proc: Option<(Vec<Vec<u8>>, usize)>,
    /// `/sys/block`'s entries, read when first needed, and the next one.
    sysblock: Option<(Vec<Vec<u8>>, usize)>,
    /// `ncur`: the position in the default eight or in `minors`.
    ncur: i32,
    /// `minors`: the numbers of `/dev/loopN` past the first eight.
    minors: Option<Vec<i32>>,
    done: bool,
    default_check: bool,
    /// `LOOPITER_FL_*`.
    flags: i32,
}

/// `struct loopdev_cxt`: one loop device, what is known of it, and an
/// iteration over the others.
#[derive(Debug)]
pub struct LoopCxt {
    /// `device`: its path, at most 127 bytes.
    device: Vec<u8>,
    /// `filename`: the backing file to set it up with, made canonical.
    filename: Option<Vec<u8>>,
    /// `fd`: the device, open.
    fd: Option<File>,
    /// `mode`: what it is open for, `O_RDONLY` (0) or `O_RDWR` (2).
    mode: i32,
    /// `blocksize`: the logical sector size to set it up with.
    blocksize: u64,
    /// `LOOPDEV_FL_*`.
    flags: i32,
    has_info: bool,
    extra_check: bool,
    info_failed: bool,
    control_ok: bool,
    /// `sysfs`: `/sys/dev/block/MAJ:MIN/`, found when first asked.
    sysfs: Option<PathCxt>,
    /// `config`: what `LOOP_GET_STATUS64` answered, or what will be set.
    config: LoopConfig,
    iter: LoopIter,
}

/// `O_RDONLY`, `O_RDWR`, `O_CLOEXEC`, `O_DIRECT`.
const O_RDONLY: i32 = 0;
const O_RDWR: i32 = 2;
#[cfg(unix)]
const O_CLOEXEC: i32 = 0o2_000_000;
#[cfg(unix)]
const O_DIRECT: i32 = 0o40_000;

/// `_PATH_DEV_LOOPCTL`, `_PATH_DEV_LOOP`, `_PATH_PROC_PARTITIONS`.
const PATH_DEV_LOOPCTL: &[u8] = b"/dev/loop-control";
const PATH_DEV_LOOP: &[u8] = b"/dev/loop";
const PATH_PROC_PARTITIONS: &[u8] = b"/proc/partitions";

/// `open (path, flags)`: the file, or the `errno`.
#[cfg(unix)]
fn open_flags(path: &[u8], flags: i32) -> Result<File, i32> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut o = std::fs::OpenOptions::new();
    match flags & 3 {
        O_RDWR => o.read(true).write(true),
        1 => o.write(true),
        _ => o.read(true),
    };
    o.custom_flags(flags & !3);
    o.open(path_of(path)).map_err(|e| e.raw_os_error().unwrap_or(EINVAL))
}

#[cfg(not(unix))]
fn open_flags(_path: &[u8], _flags: i32) -> Result<File, i32> {
    Err(ENOSYS)
}

/// `ioctl (fd, request, arg)` with an integer argument: its answer, or the
/// `errno`.
#[cfg(unix)]
fn ioctl_int(fd: &File, request: u64, arg: u64) -> Result<i32, i32> {
    // SAFETY: the request takes its argument by value (a descriptor, a
    // number or a flag); nothing is read or written through it.
    let rc = unsafe { sys::ioctl(fd.as_raw_fd(), request, arg) };
    if rc < 0 { Err(last_errno()) } else { Ok(rc) }
}

#[cfg(not(unix))]
fn ioctl_int(_fd: &File, _request: u64, _arg: u64) -> Result<i32, i32> {
    Err(ENOSYS)
}

/// `ioctl (fd, request, &value)`: `value` read or filled by the call, which
/// is told nothing of its size -- the request names the type, and the
/// caller passes that type.
#[cfg(unix)]
fn ioctl_ptr<T>(fd: &File, request: u64, value: &mut T) -> Result<(), i32> {
    // SAFETY: `value` is a live, exclusively borrowed `T`, and `T` is the
    // `#[repr(C)]` type (or plain integer) the request reads and writes:
    // the callers pair them -- `LoopInfo64` with the status requests,
    // `LoopConfig` with `LOOP_CONFIGURE`, `i32` with `BLKSSZGET`, `u64` with
    // `BLKGETSIZE64`.
    let rc = unsafe { sys::ioctl(fd.as_raw_fd(), request, std::ptr::from_mut(value)) };
    if rc < 0 { Err(last_errno()) } else { Ok(()) }
}

#[cfg(not(unix))]
fn ioctl_ptr<T>(_fd: &File, _request: u64, _value: &mut T) -> Result<(), i32> {
    Err(ENOSYS)
}

/// `repeat_on_eagain (x)`: `x` again a quarter of a second after each
/// `EAGAIN`, ten times at most.
fn repeat_on_eagain<T>(mut x: impl FnMut() -> Result<T, i32>) -> Result<T, i32> {
    let mut tries = 0u32;
    loop {
        match x() {
            Err(EAGAIN) if tries < LOOPDEV_MAX_TRIES => {
                std::thread::sleep(std::time::Duration::from_millis(250));
                tries = tries.saturating_add(1);
            }
            r => return r,
        }
    }
}

/// `stat (path)`: the status, following links, or `None`.
fn stat(path: &[u8]) -> Option<std::fs::Metadata> {
    std::fs::metadata(path_of(path)).ok()
}

/// Whether `m` is a block device, and its device number.
fn block_rdev(m: &std::fs::Metadata) -> Option<u64> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{FileTypeExt, MetadataExt};
        m.file_type().is_block_device().then(|| m.rdev())
    }
    #[cfg(not(unix))]
    {
        let _ = m;
        None
    }
}

/// `is_loopdev (device)`: whether `device` is a loop device's node -- the
/// loop major, or a whole disk sysfs calls `loopN` -- or `ENODEV`.
///
/// # Errors
///
/// `ENODEV` for anything else, as upstream sets.
pub fn is_loopdev(device: &[u8]) -> Result<(), i32> {
    let Some(rdev) = stat(device).as_ref().and_then(block_rdev) else {
        return Err(ENODEV);
    };
    if crate::major(rdev) == LOOPDEV_MAJOR {
        return Ok(());
    }
    if crate::devno_is_wholedisk(rdev) == Some(true) {
        let mut name = crate::PATH_SYS_DEVBLOCK.to_vec();
        name.extend_from_slice(
            format!("/{}:{}", crate::major(rdev), crate::minor(rdev)).as_bytes(),
        );
        if let Some(mut cn) = crate::canonicalize::canonicalize_path(&name)
            && let Some(p) = crate::stripoff_last_component(&mut cn)
            && p.starts_with(b"loop")
        {
            return Ok(());
        }
    }
    Err(ENODEV)
}

/// `sscanf (s, "loop%u", &n)`: the number after `loop` -- `%u`'s, which
/// skips white space and takes a sign.
fn scan_loop_number(name: &[u8]) -> Option<u32> {
    let rest = name.strip_prefix(b"loop")?;
    let mut at = 0usize;
    let n = crate::path::scan_ulong(rest, &mut at)?;
    // `%u` stores the `unsigned long` cut to an `unsigned int`.
    Some(u32::try_from(n & u64::from(u32::MAX)).unwrap_or(u32::MAX))
}

/// `loop_scandir (dirname, &ary, hasprefix)`: the numbers of the loop
/// devices in `dir` past the default eight, in order -- `loopN` there, or
/// with `hasprefix` false, plain `N` (`/dev/loop/N`).
fn loop_scandir(dir: &[u8], hasprefix: bool) -> Vec<i32> {
    let Ok(entries) = std::fs::read_dir(path_of(dir)) else {
        return Vec::new();
    };
    let mut ary = Vec::new();
    for e in entries.flatten() {
        // Block devices, links, and what readdir did not type.
        if let Ok(ft) = e.file_type() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::FileTypeExt;
                if !ft.is_block_device() && !ft.is_symlink() {
                    continue;
                }
            }
            #[cfg(not(unix))]
            let _ = ft;
        }
        let name = quoting::os_bytes(&e.file_name()).into_owned();
        let n = if hasprefix {
            match scan_loop_number(&name) {
                Some(n) => n,
                None => continue,
            }
        } else {
            match std::str::from_utf8(&name).ok().and_then(|s| s.parse::<i64>().ok()) {
                Some(n) => u32::try_from(n & i64::from(u32::MAX)).unwrap_or(0),
                None => continue,
            }
        };
        // Stored in an `int`, compared as the `unsigned` it was read as.
        if n < LOOPDEV_DEFAULT_NNODES.unsigned_abs() {
            continue;
        }
        ary.push(n.cast_signed());
    }
    ary.sort_unstable();
    ary
}

impl LoopCxt {
    /// `loopcxt_init (lc, flags)`: a context with no device, and how it
    /// will learn things decided.
    #[must_use]
    pub fn new(flags: i32) -> Self {
        let mut lc = LoopCxt {
            device: Vec::new(),
            filename: None,
            fd: None,
            mode: 0,
            blocksize: 0,
            flags,
            has_info: false,
            extra_check: false,
            info_failed: false,
            control_ok: false,
            sysfs: None,
            config: LoopConfig::default(),
            iter: LoopIter::default(),
        };
        let sys_block = stat(crate::PATH_SYS_BLOCK).is_some_and(|m| m.is_dir());
        if !sys_block {
            lc.flags |= LOOPDEV_FL_NOSYSFS;
            lc.flags &= !LOOPDEV_FL_NOIOCTL;
        }
        if lc.flags & LOOPDEV_FL_NOSYSFS == 0 {
            // "Use only sysfs for basic information about loop devices"
            lc.flags |= LOOPDEV_FL_NOIOCTL;
        }
        if lc.flags & LOOPDEV_FL_CONTROL == 0 && stat(PATH_DEV_LOOPCTL).is_some() {
            lc.flags |= LOOPDEV_FL_CONTROL;
        }
        lc
    }

    fn ioctl_enabled(&self) -> bool {
        self.flags & LOOPDEV_FL_NOIOCTL == 0
    }

    fn sysfs_available(&self) -> bool {
        self.flags & LOOPDEV_FL_NOSYSFS == 0 && !self.ioctl_enabled()
    }

    /// `loopcxt_set_device (lc, device)`: the device to work on -- a name
    /// without a leading `/` is one in `/dev` (`/dev/loop/N` for `loopN`
    /// where the nodes are in a directory) -- with everything known of the
    /// last one forgotten, or none.
    ///
    /// # Errors
    ///
    /// `-1`'s case: a name too short to be `loopN` where the nodes are in
    /// `/dev/loop/`.
    pub fn set_device(&mut self, device: Option<&[u8]>) -> Result<(), i32> {
        self.fd = None;
        self.mode = 0;
        self.blocksize = 0;
        self.has_info = false;
        self.info_failed = false;
        self.device.clear();
        self.config = LoopConfig::default();
        if let Some(device) = device {
            if device.first() == Some(&b'/') {
                self.device = device.to_vec();
            } else {
                let (dir, name): (&[u8], &[u8]) = if self.flags & LOOPDEV_FL_DEVSUBDIR != 0 {
                    if device.len() < 5 {
                        return Err(EINVAL);
                    }
                    (b"/dev/loop/", device.get(4..).unwrap_or_default())
                } else {
                    (b"/dev/", device)
                };
                self.device = [dir, name].concat();
            }
            // `char device[128]`: snprintf and xstrncpy keep 127 bytes.
            self.device.truncate(127);
        }
        self.sysfs = None;
        Ok(())
    }

    /// `loopcxt_has_device (lc)`.
    #[must_use]
    pub fn has_device(&self) -> bool {
        !self.device.is_empty()
    }

    /// `loopcxt_get_device (lc)`: the device's path, if there is one.
    #[must_use]
    pub fn device(&self) -> Option<&[u8]> {
        (!self.device.is_empty()).then_some(self.device.as_slice())
    }

    /// `loopcxt_get_sysfs (lc)`: the device's sysfs directory, found by its
    /// number -- or the `errno` of not finding it.
    fn sysfs(&mut self) -> Result<&PathCxt, i32> {
        if self.device.is_empty() || self.flags & LOOPDEV_FL_NOSYSFS != 0 {
            return Err(ENOENT);
        }
        if self.sysfs.is_none() {
            let devno = crate::devname_to_devno(&self.device);
            if devno == 0 {
                return Err(ENOENT);
            }
            self.sysfs = crate::new_sysfs_path(devno, None, None);
        }
        self.sysfs.as_ref().ok_or(ENOENT)
    }

    /// `loopcxt_get_fd (lc)`: the device, opened read-only -- read-write
    /// with `LOOPDEV_FL_RDWR` -- if it is not open yet.
    ///
    /// # Errors
    ///
    /// `EINVAL` without a device; the `errno` of the `open`.
    pub fn fd(&mut self) -> Result<&File, i32> {
        if self.device.is_empty() {
            return Err(EINVAL);
        }
        if self.fd.is_none() {
            self.mode = if self.flags & LOOPDEV_FL_RDWR != 0 {
                O_RDWR
            } else {
                O_RDONLY
            };
            #[cfg(unix)]
            let flags = self.mode | O_CLOEXEC;
            #[cfg(not(unix))]
            let flags = self.mode;
            self.fd = Some(open_flags(&self.device, flags)?);
        }
        self.fd.as_ref().ok_or(EINVAL)
    }

    /// `loopcxt_init_iterator (lc, flags)`: an iteration over the used
    /// ([`LOOPITER_FL_USED`]) or free ([`LOOPITER_FL_FREE`]) devices begun.
    pub fn init_iterator(&mut self, flags: i32) {
        self.iter = LoopIter {
            ncur: -1,
            flags,
            default_check: true,
            ..LoopIter::default()
        };
        if !self.extra_check {
            // "Check for /dev/loop/<N> subdirectory"
            if self.flags & LOOPDEV_FL_DEVSUBDIR == 0
                && stat(PATH_DEV_LOOP).is_some_and(|m| m.is_dir())
            {
                self.flags |= LOOPDEV_FL_DEVSUBDIR;
            }
            self.extra_check = true;
        }
    }

    /// `loopcxt_deinit_iterator (lc)`.
    pub fn deinit_iterator(&mut self) {
        self.iter = LoopIter::default();
    }

    /// `loopiter_set_device (lc, device)`: `device` made the current one if
    /// it is what the iteration looks for -- used or free -- `Ok(false)` if
    /// it is a loop device that is not.
    fn iter_set_device(&mut self, device: &[u8]) -> Result<bool, i32> {
        self.set_device(Some(device))?;
        if self.iter.flags & (LOOPITER_FL_USED | LOOPITER_FL_FREE) == 0 {
            return Ok(true);
        }
        is_loopdev(&self.device)?;
        let used = self.offset().is_ok();
        if (self.iter.flags & LOOPITER_FL_USED != 0 && used)
            || (self.iter.flags & LOOPITER_FL_FREE != 0 && !used)
        {
            return Ok(true);
        }
        // "failed to use %s device" -- a context is left without one.
        let _ = self.set_device(None);
        Ok(false)
    }

    /// `loopcxt_next_from_proc (lc)`: the next used device
    /// `/proc/partitions` lists.
    fn next_from_proc(&mut self) -> bool {
        if self.iter.proc.is_none() {
            let Ok(text) = std::fs::read(path_of(PATH_PROC_PARTITIONS)) else {
                return false;
            };
            // `fgets (buf, BUFSIZ, f)`: a line, or 8191 bytes of one.
            let mut lines = Vec::new();
            for line in text.split_inclusive(|&b| b == b'\n') {
                for chunk in line.chunks(8191) {
                    lines.push(chunk.to_vec());
                }
            }
            self.iter.proc = Some((lines, 0));
        }
        loop {
            let Some((lines, at)) = self.iter.proc.as_mut() else {
                return false;
            };
            let Some(line) = lines.get(*at).cloned() else {
                return false;
            };
            *at = at.saturating_add(1);
            // " %u %*s %*s %128[^\n ]"
            let words: Vec<&[u8]> = line
                .split(|b| b.is_ascii_whitespace())
                .filter(|w| !w.is_empty())
                .collect();
            let (Some(m), Some(name)) = (words.first(), words.get(3)) else {
                continue;
            };
            let mut at_m = 0usize;
            let major = crate::path::scan_ulong(m, &mut at_m).map(|n| n & u64::from(u32::MAX));
            if major != Some(u64::from(LOOPDEV_MAJOR)) {
                continue;
            }
            let name: Vec<u8> = name.iter().take(128).copied().collect();
            if matches!(self.iter_set_device(&name), Ok(true)) {
                return true;
            }
        }
    }

    /// `loopcxt_next_from_sysfs (lc)`: the next device in `/sys/block` that
    /// has a backing file.
    fn next_from_sysfs(&mut self) -> bool {
        if self.iter.sysblock.is_none() {
            let Ok(entries) = std::fs::read_dir(path_of(crate::PATH_SYS_BLOCK)) else {
                return false;
            };
            let names = entries
                .flatten()
                .map(|e| quoting::os_bytes(&e.file_name()).into_owned())
                .collect();
            self.iter.sysblock = Some((names, 0));
        }
        loop {
            let Some((names, at)) = self.iter.sysblock.as_mut() else {
                return false;
            };
            let Some(name) = names.get(*at).cloned() else {
                return false;
            };
            *at = at.saturating_add(1);
            if !name.starts_with(b"loop") {
                continue;
            }
            let mut file = crate::PATH_SYS_BLOCK.to_vec();
            file.push(b'/');
            file.extend_from_slice(&name);
            file.extend_from_slice(b"/loop/backing_file");
            if stat(&file).is_none() {
                continue;
            }
            if matches!(self.iter_set_device(&name), Ok(true)) {
                return true;
            }
        }
    }

    /// `loopcxt_next (lc)`: the next device the iteration looks for made
    /// the current one -- `false` at the end, which also ends the iteration.
    pub fn next_device(&mut self) -> bool {
        if self.iter.done {
            return false;
        }
        if self.iter.flags & LOOPITER_FL_USED != 0 {
            let found = if self.sysfs_available() {
                self.next_from_sysfs()
            } else {
                self.next_from_proc()
            };
            if !found {
                self.deinit_iterator();
            }
            return found;
        }
        // "Classic way, try first eight loop devices"
        if self.iter.default_check {
            self.iter.ncur = self.iter.ncur.saturating_add(1);
            while self.iter.ncur < LOOPDEV_DEFAULT_NNODES {
                let name = format!("loop{}", self.iter.ncur);
                if matches!(self.iter_set_device(name.as_bytes()), Ok(true)) {
                    return true;
                }
                self.iter.ncur = self.iter.ncur.saturating_add(1);
            }
            self.iter.default_check = false;
        }
        // "the worst possibility, scan whole /dev or /dev/loop/<N>"
        if self.iter.minors.is_none() {
            self.iter.minors = Some(if self.flags & LOOPDEV_FL_DEVSUBDIR != 0 {
                loop_scandir(PATH_DEV_LOOP, false)
            } else {
                loop_scandir(b"/dev", true)
            });
            self.iter.ncur = -1;
        }
        self.iter.ncur = self.iter.ncur.saturating_add(1);
        loop {
            let at = usize::try_from(self.iter.ncur).unwrap_or(usize::MAX);
            let Some(n) = self.iter.minors.as_ref().and_then(|m| m.get(at)).copied() else {
                break;
            };
            let name = format!("loop{n}");
            if matches!(self.iter_set_device(name.as_bytes()), Ok(true)) {
                return true;
            }
            self.iter.ncur = self.iter.ncur.saturating_add(1);
        }
        self.deinit_iterator();
        false
    }

    /// `loopcxt_get_info (lc)`: the device's `struct loop_info64`, asked of
    /// the kernel once and kept.
    ///
    /// # Errors
    ///
    /// `EINVAL` once it has failed; the `errno` of the `open` or the ioctl.
    pub fn info(&mut self) -> Result<&LoopInfo64, i32> {
        if self.info_failed {
            return Err(EINVAL);
        }
        if self.has_info {
            return Ok(&self.config.info);
        }
        let mut info = LoopInfo64::default();
        let fd = self.fd()?;
        #[cfg(unix)]
        let got = ioctl_ptr(fd, nr::LOOP_GET_STATUS64, &mut info);
        #[cfg(not(unix))]
        let got = ioctl_ptr(fd, 0, &mut info);
        match got {
            Ok(()) => {
                self.config.info = info;
                self.has_info = true;
                self.info_failed = false;
                Ok(&self.config.info)
            }
            Err(e) => {
                self.info_failed = true;
                Err(e)
            }
        }
    }

    /// `loopcxt_get_backing_file (lc)`: the file behind the device -- from
    /// sysfs, whole; else the 64 bytes the kernel keeps of its name, the cut
    /// marked with a `*`.
    ///
    /// # Errors
    ///
    /// The `errno` of the last read tried.
    pub fn backing_file(&mut self) -> Result<Vec<u8>, i32> {
        let mut err = ENOENT;
        match self.sysfs() {
            Ok(sysfs) => match sysfs.read_string(b"loop/backing_file") {
                Ok(Some(s)) => return Ok(s),
                Ok(None) => {}
                Err(e) => err = e,
            },
            Err(e) => err = e,
        }
        if self.ioctl_enabled() {
            let mut name = self.info()?.lo_file_name;
            if let Some(b) = name.get_mut(LO_NAME_SIZE.saturating_sub(2)) {
                *b = b'*';
            }
            if let Some(b) = name.get_mut(LO_NAME_SIZE.saturating_sub(1)) {
                *b = 0;
            }
            return Ok(crate::c_str(&name).to_vec());
        }
        Err(err)
    }

    /// A number from the device's sysfs directory, else from its info.
    fn sysfs_or_info(&mut self, attr: &[u8], from_info: fn(&LoopInfo64) -> u64) -> Result<u64, i32> {
        if let Ok(sysfs) = self.sysfs()
            && let Some(n) = sysfs.read_u64(attr)
        {
            return Ok(n);
        }
        if self.ioctl_enabled() {
            return self.info().map(from_info);
        }
        Err(EINVAL)
    }

    /// `loopcxt_get_offset (lc, &offset)`.
    ///
    /// # Errors
    ///
    /// `EINVAL` when sysfs has none and ioctls are not used; the info's
    /// `errno`.
    pub fn offset(&mut self) -> Result<u64, i32> {
        self.sysfs_or_info(b"loop/offset", |i| i.lo_offset)
    }

    /// `loopcxt_get_sizelimit (lc, &size)`.
    ///
    /// # Errors
    ///
    /// As [`LoopCxt::offset`].
    pub fn sizelimit(&mut self) -> Result<u64, i32> {
        self.sysfs_or_info(b"loop/sizelimit", |i| i.lo_sizelimit)
    }

    /// `loopcxt_get_blocksize (lc, &blocksize)`: the logical sector size,
    /// from sysfs or `BLKSSZGET`.
    ///
    /// # Errors
    ///
    /// `EINVAL` when the device cannot be opened; the ioctl's `errno`.
    pub fn blocksize(&mut self) -> Result<u64, i32> {
        if let Ok(sysfs) = self.sysfs()
            && let Some(n) = sysfs.read_u64(b"queue/logical_block_size")
        {
            return Ok(n);
        }
        let Ok(fd) = self.fd() else {
            return Err(EINVAL);
        };
        let mut sz = 0i32;
        #[cfg(unix)]
        ioctl_ptr(fd, nr::BLKSSZGET, &mut sz)?;
        #[cfg(not(unix))]
        ioctl_ptr(fd, 0, &mut sz)?;
        Ok(u64::try_from(sz).unwrap_or(0))
    }

    /// `loopcxt_get_encrypt_type (lc, &type)`: from the info only.
    ///
    /// # Errors
    ///
    /// The info's `errno`.
    pub fn encrypt_type(&mut self) -> Result<u32, i32> {
        self.info().map(|i| i.lo_encrypt_type)
    }

    /// `loopcxt_get_crypt_name (lc)`.
    pub fn crypt_name(&mut self) -> Option<Vec<u8>> {
        self.info().ok().map(|i| crate::c_str(&i.lo_crypt_name).to_vec())
    }

    /// `loopcxt_get_backing_devno (lc, &devno)`: from the info only.
    ///
    /// # Errors
    ///
    /// The info's `errno`.
    pub fn backing_devno(&mut self) -> Result<u64, i32> {
        self.info().map(|i| i.lo_device)
    }

    /// `loopcxt_get_backing_inode (lc, &ino)`: from the info only.
    ///
    /// # Errors
    ///
    /// The info's `errno`.
    pub fn backing_inode(&mut self) -> Result<u64, i32> {
        self.info().map(|i| i.lo_inode)
    }

    /// A flag from the device's sysfs directory, else from its info.
    fn sysfs_flag(&mut self, attr: &[u8], flag: u32) -> i32 {
        if let Ok(sysfs) = self.sysfs()
            && let Some(fl) = sysfs.read_s32(attr)
        {
            return fl;
        }
        if self.ioctl_enabled()
            && let Ok(info) = self.info()
        {
            return i32::try_from(info.lo_flags & flag).unwrap_or(0);
        }
        0
    }

    /// `loopcxt_is_partscan (lc)`: sysfs's flag, else the module's
    /// support for it -- every kernel since 3.2 has it.
    pub fn is_partscan(&mut self) -> i32 {
        if let Ok(sysfs) = self.sysfs()
            && let Some(fl) = sysfs.read_s32(b"loop/partscan")
        {
            return fl;
        }
        1
    }

    /// `loopcxt_is_autoclear (lc)`.
    pub fn is_autoclear(&mut self) -> i32 {
        self.sysfs_flag(b"loop/autoclear", LO_FLAGS_AUTOCLEAR)
    }

    /// `loopcxt_is_readonly (lc)`.
    pub fn is_readonly(&mut self) -> i32 {
        self.sysfs_flag(b"ro", LO_FLAGS_READ_ONLY)
    }

    /// `loopcxt_is_dio (lc)`.
    pub fn is_dio(&mut self) -> i32 {
        self.sysfs_flag(b"loop/dio", LO_FLAGS_DIRECT_IO)
    }

    /// `loopcxt_is_used (lc, st, backing_file, offset, sizelimit, flags)`:
    /// whether the current device is backed by the file `st` is the status
    /// of -- by its device and inode, which the info gives -- or, without
    /// them, by `backing_file`'s name; and at `offset` (and `sizelimit`)
    /// with `LOOPDEV_FL_OFFSET` (and `LOOPDEV_FL_SIZELIMIT`).
    pub fn is_used(
        &mut self,
        st: Option<(u64, u64)>,
        backing_file: Option<&[u8]>,
        offset: u64,
        sizelimit: u64,
        flags: i32,
    ) -> bool {
        let mut found = false;
        let mut decided = false;
        if let Some((dev, ino)) = st
            && let Ok(i) = self.backing_inode()
            && let Ok(d) = self.backing_devno()
        {
            // "don't use filename if we have devno and inode"
            found = i == ino && d == dev;
            decided = true;
        }
        if !decided
            && let Some(name) = backing_file
            && self.backing_file().is_ok_and(|b| b == name)
        {
            found = true;
        }
        if !found {
            return false;
        }
        if flags & LOOPDEV_FL_OFFSET != 0 {
            let at = self.offset().is_ok_and(|off| off == offset);
            if at && flags & LOOPDEV_FL_SIZELIMIT != 0 {
                return self.sizelimit().is_ok_and(|sz| sz == sizelimit);
            }
            return at;
        }
        true
    }

    /// `loopcxt_set_offset (lc, offset)`: for the setup.
    pub fn set_offset(&mut self, offset: u64) {
        self.config.info.lo_offset = offset;
    }

    /// `loopcxt_set_sizelimit (lc, sizelimit)`: for the setup.
    pub fn set_sizelimit(&mut self, sizelimit: u64) {
        self.config.info.lo_sizelimit = sizelimit;
    }

    /// `loopcxt_set_blocksize (lc, blocksize)`: for the setup.
    pub fn set_blocksize(&mut self, blocksize: u64) {
        self.blocksize = blocksize;
    }

    /// `loopcxt_set_flags (lc, flags)`: `LO_FLAGS_*`, for the setup.
    pub fn set_flags(&mut self, flags: u32) {
        self.config.info.lo_flags = flags;
    }

    /// `lc->config.info.lo_flags &= ~flag`: a flag taken away again.
    pub fn clear_flag(&mut self, flag: u32) {
        self.config.info.lo_flags &= !flag;
    }

    /// `loopcxt_set_backing_file (lc, filename)`: the file to set up with,
    /// made canonical, and the start of its name for the info.
    ///
    /// # Errors
    ///
    /// `EINVAL` for an empty name, which has no canonical form.
    pub fn set_backing_file(&mut self, filename: &[u8]) -> Result<(), i32> {
        let canonical = crate::canonicalize::canonicalize_path(filename).ok_or(EINVAL)?;
        // `xstrncpy (lo_file_name, filename, LO_NAME_SIZE)`: 63 bytes and a NUL.
        let mut name = [0u8; LO_NAME_SIZE];
        for (dst, src) in name.iter_mut().zip(canonical.iter().take(LO_NAME_SIZE.saturating_sub(1))) {
            *dst = *src;
        }
        self.config.info.lo_file_name = name;
        self.filename = Some(canonical);
        Ok(())
    }

    /// `loopcxt_check_size (lc, file_fd)`: with an offset or size limit, the
    /// device's size made what they make of the file's -- by
    /// `LOOP_SET_CAPACITY` where the kernel did not do it itself.
    fn check_size(&mut self, file: &File) -> Result<(), i32> {
        let (offset, sizelimit) = (self.config.info.lo_offset, self.config.info.lo_sizelimit);
        if offset == 0 && sizelimit == 0 {
            return Ok(());
        }
        let st = file.metadata().map_err(|e| e.raw_os_error().unwrap_or(EINVAL))?;
        let mut expected = if block_rdev(&st).is_some() {
            blkdev_size(file)?
        } else {
            st.len()
        };
        if expected == 0 || expected <= offset {
            // "ignore this error"
            return Ok(());
        }
        if offset > 0 {
            expected = expected.saturating_sub(offset);
        }
        if sizelimit > 0 && sizelimit < expected {
            expected = sizelimit;
        }
        let size = blkdev_size(self.fd()?)?;
        // "It's block device, so, align to 512-byte sectors"
        expected = (expected >> 9) << 9;
        if expected != size {
            if let Err(e) = self.ioctl_capacity() {
                // "ioctl not available"
                return Err(if e == ENOTTY || e == EINVAL { ERANGE } else { e });
            }
            let size = blkdev_size(self.fd()?)?;
            if expected != size {
                return Err(ERANGE);
            }
        }
        Ok(())
    }

    /// `loopcxt_setup_device (lc)`: the device set up on the backing file --
    /// read-write unless `LO_FLAGS_READ_ONLY`, or the file cannot be written
    /// -- by `LOOP_CONFIGURE`, or on an older kernel `LOOP_SET_FD` and
    /// `LOOP_SET_STATUS64`.
    ///
    /// # Errors
    ///
    /// The `errno` upstream leaves: of opening the file or the device, or
    /// of the ioctl that failed (and the device is detached again, unless
    /// it was busy).
    pub fn setup_device(&mut self) -> Result<(), i32> {
        let Some(filename) = self.filename.clone() else {
            return Err(EINVAL);
        };
        if self.device.is_empty() {
            return Err(EINVAL);
        }
        let mut mode = if self.config.info.lo_flags & LO_FLAGS_READ_ONLY != 0 {
            O_RDONLY
        } else {
            O_RDWR
        };
        #[cfg(unix)]
        let mut extra = O_CLOEXEC;
        #[cfg(not(unix))]
        let extra = 0;
        #[cfg(unix)]
        if self.config.info.lo_flags & LO_FLAGS_DIRECT_IO != 0 {
            extra |= O_DIRECT;
        }
        let file = match open_flags(&filename, mode | extra) {
            Ok(f) => f,
            Err(e) if mode != O_RDONLY && (e == EROFS || e == EACCES) => {
                mode = O_RDONLY;
                open_flags(&filename, mode | extra)?
            }
            Err(e) => return Err(e),
        };
        if self.fd.is_some() && self.mode != mode {
            // "closing already open device (mode mismatch)"
            self.fd = None;
            self.mode = 0;
        }
        if mode == O_RDONLY {
            self.flags |= LOOPDEV_FL_RDONLY;
            self.config.info.lo_flags |= LO_FLAGS_READ_ONLY;
        } else {
            self.flags |= LOOPDEV_FL_RDWR;
            self.config.info.lo_flags &= !LO_FLAGS_READ_ONLY;
            self.flags &= !LOOPDEV_FL_RDONLY;
        }
        let mut cnt = 0u32;
        let opened = loop {
            let open = self.fd().map(|_| ());
            match open {
                Ok(()) => break Ok(()),
                Err(e) if self.control_ok && (e == EACCES || e == ENOENT) && cnt < 16 => {
                    // udev has probably not set the node's owner yet.
                    std::thread::sleep(std::time::Duration::from_millis(25));
                    cnt = cnt.saturating_add(1);
                }
                Err(e) => break Err(e),
            }
        };
        opened?;
        let result = self.configure(&file);
        if let Err(e) = result {
            if e != EBUSY
                && let Some(fd) = self.fd.as_ref()
            {
                // The half-done setup undone; its own failure changes
                // nothing that can still be put right.
                #[cfg(unix)]
                let _ = ioctl_int(fd, nr::LOOP_CLR_FD, 0);
                #[cfg(not(unix))]
                let _ = fd;
            }
            return Err(e);
        }
        self.config = LoopConfig::default();
        self.has_info = false;
        self.info_failed = false;
        Ok(())
    }

    /// The ioctls of [`LoopCxt::setup_device`], on the open device.
    fn configure(&mut self, file: &File) -> Result<(), i32> {
        #[cfg(unix)]
        {
            self.config.fd = u32::try_from(file.as_raw_fd()).unwrap_or(0);
        }
        if self.blocksize > 0 {
            self.config.block_size = u32::try_from(self.blocksize).unwrap_or(u32::MAX);
        }
        let mut config = self.config;
        let dev = self.fd()?;
        #[cfg(unix)]
        let configured = repeat_on_eagain(|| ioctl_ptr(dev, nr::LOOP_CONFIGURE, &mut config));
        #[cfg(not(unix))]
        let configured = repeat_on_eagain(|| ioctl_ptr(dev, 0, &mut config));
        match configured {
            Ok(()) => {}
            Err(e) if e == EINVAL || e == ENOTTY || e == ENOSYS => {
                // "Old deprecated way; first assign backing file FD and then
                // in the second step set loop device properties."
                #[cfg(unix)]
                ioctl_int(dev, nr::LOOP_SET_FD, u64::try_from(file.as_raw_fd()).unwrap_or(0))?;
                #[cfg(not(unix))]
                ioctl_int(dev, 0, 0)?;
                if self.blocksize > 0 {
                    self.ioctl_blocksize(self.blocksize)?;
                }
                self.ioctl_status()?;
            }
            Err(e) => return Err(e),
        }
        self.check_size(file)
    }

    /// `loopcxt_ioctl_status (lc)`: `LOOP_SET_STATUS64` with the info the
    /// context holds.
    ///
    /// # Errors
    ///
    /// The `errno` of the `open` or the ioctl.
    pub fn ioctl_status(&mut self) -> Result<(), i32> {
        let mut info = self.config.info;
        let fd = self.fd()?;
        #[cfg(unix)]
        let r = repeat_on_eagain(|| ioctl_ptr(fd, nr::LOOP_SET_STATUS64, &mut info));
        #[cfg(not(unix))]
        let r = repeat_on_eagain(|| ioctl_ptr(fd, 0, &mut info));
        r
    }

    /// One integer ioctl on the device, again after `EAGAIN`. A device that
    /// cannot be opened answers the `open`'s `errno`: upstream returns
    /// `-EINVAL` then, but `errno` -- what `losetup` prints -- is the open's.
    fn device_ioctl(&mut self, request: u64, arg: u64) -> Result<(), i32> {
        let fd = self.fd()?;
        repeat_on_eagain(|| ioctl_int(fd, request, arg)).map(|_| ())
    }

    /// `loopcxt_ioctl_capacity (lc)`: `LOOP_SET_CAPACITY`.
    ///
    /// # Errors
    ///
    /// The `errno` of opening the device, or of the ioctl.
    pub fn ioctl_capacity(&mut self) -> Result<(), i32> {
        #[cfg(unix)]
        return self.device_ioctl(nr::LOOP_SET_CAPACITY, 0);
        #[cfg(not(unix))]
        self.device_ioctl(0, 0)
    }

    /// `loopcxt_ioctl_dio (lc, use_dio)`: `LOOP_SET_DIRECT_IO`.
    ///
    /// # Errors
    ///
    /// As [`LoopCxt::ioctl_capacity`].
    pub fn ioctl_dio(&mut self, use_dio: u64) -> Result<(), i32> {
        #[cfg(unix)]
        return self.device_ioctl(nr::LOOP_SET_DIRECT_IO, use_dio);
        #[cfg(not(unix))]
        self.device_ioctl(0, use_dio)
    }

    /// `loopcxt_ioctl_blocksize (lc, blocksize)`: `LOOP_SET_BLOCK_SIZE`.
    ///
    /// # Errors
    ///
    /// As [`LoopCxt::ioctl_capacity`].
    pub fn ioctl_blocksize(&mut self, blocksize: u64) -> Result<(), i32> {
        #[cfg(unix)]
        return self.device_ioctl(nr::LOOP_SET_BLOCK_SIZE, blocksize);
        #[cfg(not(unix))]
        self.device_ioctl(0, blocksize)
    }

    /// `loopcxt_delete_device (lc)`: `LOOP_CLR_FD` -- the device detached.
    ///
    /// # Errors
    ///
    /// As [`LoopCxt::ioctl_capacity`].
    pub fn delete_device(&mut self) -> Result<(), i32> {
        #[cfg(unix)]
        return self.device_ioctl(nr::LOOP_CLR_FD, 0);
        #[cfg(not(unix))]
        self.device_ioctl(0, 0)
    }

    /// `loopcxt_add_device (lc)`: the device made by `LOOP_CTL_ADD`, its
    /// number taken from its name.
    ///
    /// # Errors
    ///
    /// `EINVAL` for a name with no number, `ENOSYS` without
    /// `/dev/loop-control`, the ioctl's `errno`.
    pub fn add_device(&mut self) -> Result<(), i32> {
        if self.device.is_empty() {
            return Err(EINVAL);
        }
        if self.flags & LOOPDEV_FL_CONTROL == 0 {
            return Err(ENOSYS);
        }
        let slash = self.device.iter().rposition(|&b| b == b'/').ok_or(EINVAL)?;
        let tail = self.device.get(slash.saturating_add(1)..).unwrap_or_default();
        let nr = scan_loop_number(tail)
            .map(u32::cast_signed)
            .or_else(|| {
                let mut at = 0usize;
                crate::path::scan_int(tail, &mut at)
            })
            .filter(|&n| n >= 0)
            .ok_or(EINVAL)?;
        let result = open_flags(PATH_DEV_LOOPCTL, add_cloexec(O_RDWR)).and_then(|ctl| {
            #[cfg(unix)]
            return ioctl_int(&ctl, nr::LOOP_CTL_ADD, u64::try_from(nr).unwrap_or(0));
            #[cfg(not(unix))]
            ioctl_int(&ctl, 0, u64::try_from(nr).unwrap_or(0))
        });
        self.control_ok = result.is_ok();
        result.map(|_| ())
    }

    /// `loopcxt_find_unused (lc)`: a free device made the current one --
    /// `LOOP_CTL_GET_FREE`'s, or else the first free one a scan finds.
    ///
    /// # Errors
    ///
    /// `ENOENT` when the scan finds none.
    pub fn find_unused(&mut self) -> Result<(), i32> {
        let mut found = false;
        if self.flags & LOOPDEV_FL_CONTROL != 0 {
            let ctl = open_flags(PATH_DEV_LOOPCTL, add_cloexec(O_RDWR));
            let free = ctl.as_ref().map_err(|&e| e).and_then(|ctl| {
                #[cfg(unix)]
                return ioctl_int(ctl, nr::LOOP_CTL_GET_FREE, 0);
                #[cfg(not(unix))]
                ioctl_int(ctl, 0, 0)
            });
            if let Ok(n) = free {
                let name = format!("loop{n}");
                found = matches!(self.iter_set_device(name.as_bytes()), Ok(true));
            }
            self.control_ok = ctl.is_ok() && found;
        }
        if !found {
            self.init_iterator(LOOPITER_FL_FREE);
            let next = self.next_device();
            self.deinit_iterator();
            if !next {
                return Err(ENOENT);
            }
        }
        Ok(())
    }

    /// `loopcxt_find_overlap (lc, filename, offset, sizelimit)`: a used
    /// device on `filename` whose part of it overlaps `offset` and
    /// `sizelimit` -- `2` when they are the same, `1` when they overlap, `0`
    /// when none does; the device found left the current one.
    pub fn find_overlap(&mut self, filename: &[u8], offset: u64, sizelimit: u64) -> i32 {
        let st = stat(filename).map(|m| dev_ino(&m));
        self.init_iterator(LOOPITER_FL_USED);
        let mut rc = 0;
        while self.next_device() {
            if !self.is_used(st, Some(filename), offset, sizelimit, 0) {
                continue;
            }
            let Ok(lc_offset) = self.offset() else {
                continue;
            };
            let Ok(lc_sizelimit) = self.sizelimit() else {
                continue;
            };
            // "full match"
            if lc_sizelimit == sizelimit && lc_offset == offset {
                rc = 2;
                break;
            }
            // "overlap"
            if lc_sizelimit != 0 && offset >= lc_offset.wrapping_add(lc_sizelimit) {
                continue;
            }
            if sizelimit != 0 && offset.wrapping_add(sizelimit) <= lc_offset {
                continue;
            }
            rc = 1;
            break;
        }
        self.deinit_iterator();
        rc
    }
}

/// `flags | O_CLOEXEC` where there is one.
fn add_cloexec(flags: i32) -> i32 {
    #[cfg(unix)]
    return flags | O_CLOEXEC;
    #[cfg(not(unix))]
    flags
}

/// The device and inode of `m`: what `loopcxt_is_used` compares a backing
/// file's `stat` by.
#[must_use]
pub fn dev_ino(m: &std::fs::Metadata) -> (u64, u64) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        (m.dev(), m.ino())
    }
    #[cfg(not(unix))]
    {
        let _ = m;
        (0, 0)
    }
}

/// `blkdev_get_size (fd, &size)`: a block device's size in bytes,
/// `BLKGETSIZE64`.
fn blkdev_size(fd: &File) -> Result<u64, i32> {
    let mut size = 0u64;
    #[cfg(unix)]
    ioctl_ptr(fd, nr::BLKGETSIZE64, &mut size)?;
    #[cfg(not(unix))]
    ioctl_ptr(fd, 0, &mut size)?;
    Ok(size)
}

/// `loopcxt_get_info`'s `lo_device`, `lo_inode` and `lo_file_name` for the
/// loop device `device` -- what `ismounted` asks -- or `None`.
pub(crate) fn loop_info(device: &[u8]) -> Option<(u64, u64, Vec<u8>)> {
    let mut lc = LoopCxt::new(0);
    lc.set_device(Some(device)).ok()?;
    let info = lc.info().ok()?;
    let mut name = info.lo_file_name;
    if let Some(b) = name.get_mut(LO_NAME_SIZE.saturating_sub(2)) {
        *b = b'*';
    }
    if let Some(b) = name.get_mut(LO_NAME_SIZE.saturating_sub(1)) {
        *b = 0;
    }
    Some((info.lo_device, info.lo_inode, crate::c_str(&name).to_vec()))
}

/// `loopdev_get_backing_file (device)`: the file behind the loop device.
#[must_use]
pub fn backing_file(device: &[u8]) -> Option<Vec<u8>> {
    if device.is_empty() {
        return None;
    }
    let mut lc = LoopCxt::new(0);
    lc.set_device(Some(device)).ok()?;
    lc.backing_file().ok()
}

/// `loopdev_has_backing_file (device)`.
#[must_use]
pub fn has_backing_file(device: &[u8]) -> bool {
    backing_file(device).is_some()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "a test fails loudly")]
mod tests {
    use super::*;

    #[test]
    fn the_kernel_s_structures_are_their_sizes() {
        assert_eq!(std::mem::size_of::<LoopInfo64>(), 232);
        assert_eq!(std::mem::size_of::<LoopConfig>(), 304);
    }

    #[test]
    fn a_device_name_is_one_in_dev_cut_to_127_bytes() {
        let mut lc = LoopCxt::new(LOOPDEV_FL_NOSYSFS);
        lc.set_device(Some(b"loop3")).unwrap();
        assert_eq!(lc.device(), Some(&b"/dev/loop3"[..]));
        lc.set_device(Some(b"/x/loop9")).unwrap();
        assert_eq!(lc.device(), Some(&b"/x/loop9"[..]));
        lc.set_device(Some(&[b'a'; 300])).unwrap();
        assert_eq!(lc.device().map(<[u8]>::len), Some(127));
        lc.set_device(None).unwrap();
        assert!(!lc.has_device());
        let mut sub = LoopCxt::new(LOOPDEV_FL_DEVSUBDIR | LOOPDEV_FL_NOSYSFS);
        sub.set_device(Some(b"loop12")).unwrap();
        assert_eq!(sub.device(), Some(&b"/dev/loop/12"[..]));
        assert_eq!(sub.set_device(Some(b"lo")), Err(EINVAL));
    }

    #[test]
    fn loop_numbers_are_read_as_sscanf_reads_them() {
        assert_eq!(scan_loop_number(b"loop12"), Some(12));
        assert_eq!(scan_loop_number(b"loop 7x"), Some(7));
        assert_eq!(scan_loop_number(b"loop-1"), Some(u32::MAX));
        assert_eq!(scan_loop_number(b"loopx"), None);
        assert_eq!(scan_loop_number(b"lop1"), None);
    }

    #[test]
    fn an_eagain_is_tried_again_and_another_error_is_not() {
        let mut n = 0;
        assert_eq!(
            repeat_on_eagain(|| {
                n += 1;
                if n < 3 { Err(EAGAIN) } else { Ok(n) }
            }),
            Ok(3)
        );
        let mut m = 0;
        assert_eq!(
            repeat_on_eagain(|| -> Result<(), i32> {
                m += 1;
                Err(EBUSY)
            }),
            Err(EBUSY)
        );
        assert_eq!(m, 1);
    }

    #[test]
    fn a_name_that_is_no_device_is_no_loop_device() {
        assert_eq!(is_loopdev(b"/nonexistent/loop0"), Err(ENODEV));
        assert_eq!(is_loopdev(b""), Err(ENODEV));
    }
}
