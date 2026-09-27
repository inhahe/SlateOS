//! `lib/loopdev.c`: what is asked here of a loop device -- the file behind
//! it (`lsblk` hides an empty loop device that has none, and `ismounted.c`
//! counts a regular file as mounted when a mounted loop device is backed by
//! it), and the kernel's `struct loop_info64` for it.
//!
//! Upstream reads both through a `struct loopdev_cxt`. `loopcxt_init`
//! decides once how it may learn things: from sysfs when `/sys/block` is a
//! directory -- and then, on any kernel since 2.6.37, never through the
//! `LOOP_GET_STATUS*` ioctls for what sysfs says (`LOOPDEV_FL_NOIOCTL`) --
//! and through the ioctls when there is no sysfs. The kernel-version half
//! of that test is taken as passed: no kernel this runs on with a
//! `/sys/block` predates 2.6.37.

use crate::path_of;

/// `LOOP_GET_STATUS64`.
#[cfg(unix)]
const LOOP_GET_STATUS64: u64 = 0x4C05;
/// `sizeof(struct loop_info64)`.
#[cfg(unix)]
const LOOP_INFO64_SIZE: usize = 232;

/// `loopcxt_get_info`: the loop device's `struct loop_info64` --
/// `lo_device`, `lo_inode` and `lo_file_name` of it -- or `None`. Upstream
/// asks the ioctl for this whether or not ioctls are otherwise in use.
pub(crate) fn loop_info(device: &[u8]) -> Option<(u64, u64, Vec<u8>)> {
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        unsafe extern "C" {
            fn ioctl(fd: i32, request: u64, ...) -> i32;
        }
        let f = std::fs::File::open(path_of(device)).ok()?;
        let mut info = [0u8; LOOP_INFO64_SIZE];
        // SAFETY: LOOP_GET_STATUS64 writes one struct loop_info64 (232
        // bytes) through the pointer, which `info` holds; the descriptor is
        // open for the call.
        let rc = unsafe { ioctl(f.as_raw_fd(), LOOP_GET_STATUS64, info.as_mut_ptr()) };
        if rc < 0 {
            return None;
        }
        let u64_at = |at: usize| {
            let mut w = [0u8; 8];
            for (k, b) in w.iter_mut().enumerate() {
                *b = info.get(at.saturating_add(k)).copied().unwrap_or(0);
            }
            u64::from_ne_bytes(w)
        };
        // lo_device at 0, lo_inode at 8, lo_file_name at 56 (64 bytes) --
        // too small for a long name, so upstream marks the cut with a `*`
        // in its last byte but one, which shows only when the name filled
        // the field.
        let mut name = info.get(56..120).unwrap_or_default().to_vec();
        if let Some(b) = name.get_mut(62) {
            *b = b'*';
        }
        if let Some(b) = name.get_mut(63) {
            *b = 0;
        }
        Some((u64_at(0), u64_at(8), crate::c_str(&name).to_vec()))
    }
    #[cfg(not(unix))]
    {
        let _ = device;
        None
    }
}

/// `loopcxt_set_device(lc, device)`: a name without a leading `/` is one
/// under `/dev/`; the whole is cut, as `lc->device` is, to `PATH_MAX - 1`
/// bytes.
fn device_path(device: &[u8]) -> Vec<u8> {
    let mut p = if device.first() == Some(&b'/') {
        device.to_vec()
    } else {
        [&b"/dev/"[..], device].concat()
    };
    p.truncate(crate::PATH_MAX.saturating_sub(1));
    p
}

/// `loopdev_get_backing_file(device)`: the file behind the loop device --
/// sysfs's `loop/backing_file`, found through the device's number
/// (`sysfs_devname_to_devno`: the node's, else sysfs's for its name);
/// without sysfs, the name the kernel was given with it.
#[must_use]
pub fn backing_file(device: &[u8]) -> Option<Vec<u8>> {
    if device.is_empty() {
        return None;
    }
    let device = device_path(device);
    let sysfs = std::fs::metadata(path_of(crate::PATH_SYS_BLOCK)).is_ok_and(|m| m.is_dir());
    if sysfs {
        let devno = crate::devname_to_devno(&device);
        if devno == 0 {
            return None;
        }
        let pc = crate::new_sysfs_path(devno, None, None)?;
        return pc.read_string(b"loop/backing_file").ok().flatten();
    }
    loop_info(&device).map(|(_, _, name)| name)
}

/// `loopdev_has_backing_file(device)`.
#[must_use]
pub fn has_backing_file(device: &[u8]) -> bool {
    backing_file(device).is_some()
}
