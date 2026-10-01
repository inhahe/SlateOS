//! util-linux 2.39.3's `lib/ismounted.c`: whether a device is mounted (and
//! where), a swap area in use, or held open exclusively -- what `blkid -o
//! list`, `mkswap` and `wipefs` ask before they print or touch a device.
//!
//! The mount tables are read as glibc's `getmntent` reads them (lines of at
//! most 4095 bytes, `\040`-style escapes decoded), and `/proc/swaps` as
//! `fgets` into a 1024-byte buffer does. A regular file counts as mounted
//! when a mounted loop device is backed by it (`lib/loopdev.c`'s
//! `loopdev_is_used`, the part of it this needs).

use crate::{errno_of, major, path_of};

/// `MF_MOUNTED`.
pub const MF_MOUNTED: i32 = 1;
/// `MF_ISROOT`.
pub const MF_ISROOT: i32 = 2;
/// `MF_READONLY`.
pub const MF_READONLY: i32 = 4;
/// `MF_SWAP`.
pub const MF_SWAP: i32 = 8;
/// `MF_BUSY`: opened exclusively by someone else.
pub const MF_BUSY: i32 = 16;

/// `LOOPDEV_MAJOR`.
const LOOPDEV_MAJOR: u32 = 7;
/// `ENOENT`.
const ENOENT: i32 = 2;
/// `EBUSY`.
#[cfg(unix)]
const EBUSY: i32 = 16;
/// `EROFS`.
const EROFS: i32 = 30;

/// What `stat(2)` says, the parts used here.
#[derive(Clone, Copy, Default)]
struct Stat {
    blk: bool,
    dev: u64,
    ino: u64,
    rdev: u64,
}

/// `stat(path)`, following links.
fn stat(path: &[u8]) -> Result<Stat, i32> {
    let m = std::fs::metadata(path_of(path)).map_err(|e| errno_of(&e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::{FileTypeExt, MetadataExt};
        Ok(Stat {
            blk: m.file_type().is_block_device(),
            dev: m.dev(),
            ino: m.ino(),
            rdev: m.rdev(),
        })
    }
    #[cfg(not(unix))]
    {
        let _ = m;
        Ok(Stat::default())
    }
}

/// `fgets(buf, size, f)` over a file's bytes: the next chunk -- through a
/// newline, or `size - 1` bytes, or to the end -- and the cursor moved past
/// it. `None` at the end.
fn fgets<'a>(data: &'a [u8], pos: &mut usize, size: usize) -> Option<&'a [u8]> {
    let rest = data.get(*pos..)?;
    if rest.is_empty() {
        return None;
    }
    let max = size.saturating_sub(1).min(rest.len());
    let n = rest
        .get(..max)?
        .iter()
        .position(|&b| b == b'\n')
        .map_or(max, |i| i.saturating_add(1));
    *pos = pos.saturating_add(n);
    rest.get(..n)
}

/// A C string: up to the first NUL.
fn c_str(b: &[u8]) -> &[u8] {
    crate::c_str(b)
}

/// glibc's `decode_name`: `\040` a space, `\011` a tab, `\012` a newline,
/// `\134` and `\\` a backslash.
fn decode_name(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0usize;
    while i < s.len() {
        let rest = s.get(i..).unwrap_or_default();
        let (b, skip) = if rest.starts_with(b"\\040") {
            (b' ', 4)
        } else if rest.starts_with(b"\\011") {
            (b'\t', 4)
        } else if rest.starts_with(b"\\012") {
            (b'\n', 4)
        } else if rest.starts_with(b"\\\\") {
            (b'\\', 2)
        } else if rest.starts_with(b"\\134") {
            (b'\\', 4)
        } else {
            (rest.first().copied().unwrap_or(0), 1)
        };
        out.push(b);
        i = i.saturating_add(skip);
    }
    out
}

/// A `struct mntent`: the fields read here.
struct MntEnt {
    fsname: Vec<u8>,
    dir: Vec<u8>,
    opts: Vec<u8>,
}

/// `__strsep(&head, " \t")` after skipping leading blanks: the next field,
/// and what is left (`None` once a field ran to the end).
fn next_field(head: Option<&[u8]>) -> (Option<&[u8]>, Option<&[u8]>) {
    let Some(h) = head else {
        return (None, None);
    };
    let start = h
        .iter()
        .position(|&b| b != b' ' && b != b'\t')
        .unwrap_or(h.len());
    let h = h.get(start..).unwrap_or_default();
    match h.iter().position(|&b| b == b' ' || b == b'\t') {
        Some(i) => (h.get(..i), h.get(i.saturating_add(1)..)),
        None => (Some(h), None),
    }
}

/// glibc's `getmntent`, over a whole table, with its 4096-byte buffer.
fn getmntents(data: &[u8]) -> Vec<MntEnt> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    'lines: loop {
        let head: &[u8] = loop {
            let Some(chunk) = fgets(data, &mut pos, 4096) else {
                break 'lines;
            };
            let buffer = c_str(chunk);
            let line = match buffer.iter().position(|&b| b == b'\n') {
                Some(nl) => {
                    // Trailing blanks off, but never past the start.
                    let mut end = nl;
                    while end > 0
                        && buffer
                            .get(end.saturating_sub(1))
                            .is_some_and(|&b| b == b' ' || b == b'\t')
                    {
                        end = end.saturating_sub(1);
                    }
                    buffer.get(..end).unwrap_or_default()
                }
                None => {
                    // Not the whole line was read: read on to its end, and
                    // forget it.
                    while let Some(tmp) = fgets(data, &mut pos, 1024) {
                        if c_str(tmp).contains(&b'\n') {
                            break;
                        }
                    }
                    buffer
                }
            };
            let skip = line
                .iter()
                .position(|&b| b != b' ' && b != b'\t')
                .unwrap_or(line.len());
            let head = line.get(skip..).unwrap_or_default();
            if !(head.is_empty() || head.first() == Some(&b'#')) {
                break head;
            }
        };
        let (fsname, rest) = next_field(Some(head));
        let (dir, rest) = next_field(rest);
        let (_ty, rest) = next_field(rest);
        let (opts, _) = next_field(rest);
        out.push(MntEnt {
            fsname: fsname.map(decode_name).unwrap_or_default(),
            dir: dir.map(decode_name).unwrap_or_default(),
            opts: opts.map(decode_name).unwrap_or_default(),
        });
    }
    out
}

/// glibc's `hasmntopt(mnt, opt)`: `opt` as a whole option, alone or with a
/// value.
fn hasmntopt(opts: &[u8], opt: &[u8]) -> bool {
    let mut rest = 0usize;
    while let Some(found) = opts
        .get(rest..)
        .and_then(|r| r.windows(opt.len().max(1)).position(|w| w == opt))
    {
        let p = rest.saturating_add(found);
        let before_ok = p == rest || opts.get(p.saturating_sub(1)) == Some(&b',');
        let after = opts.get(p.saturating_add(opt.len())).copied().unwrap_or(0);
        if before_ok && matches!(after, 0 | b'=' | b',') {
            return true;
        }
        match opts
            .get(p..)
            .and_then(|r| r.iter().position(|&b| b == b','))
        {
            Some(c) => rest = p.saturating_add(c).saturating_add(1),
            None => return false,
        }
    }
    false
}

/// `xstrncpy(dest, src, n)`: at most `n - 1` bytes.
fn xstrncpy(src: &[u8], n: usize) -> Vec<u8> {
    let s = c_str(src);
    s.get(..s.len().min(n.saturating_sub(1)))
        .unwrap_or_default()
        .to_vec()
}

/// `loopdev_is_used(device, filename, 0, 0, 0)`: the loop device is backed
/// by the file -- by device and inode when the kernel says them, else by
/// name.
fn loopdev_is_used(device: &[u8], filename: &[u8]) -> bool {
    let st = stat(filename).ok();
    if let Some(st) = st
        && let Some((dev, ino, _)) = crate::loopdev::loop_info(device)
    {
        // Device and inode known: the name is not looked at.
        return ino == st.ino && dev == st.dev;
    }
    crate::loopdev::backing_file(device).is_some_and(|name| name == filename)
}

/// What a mount-table check found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MountCheck {
    /// `MF_*`.
    pub flags: i32,
    /// Where it is mounted (`[SWAP]` for a swap area), truncated as the
    /// caller's buffer truncates it.
    pub mtpt: Vec<u8>,
}

/// `TEST_FILE`: made (and removed) in `/` to learn whether the root is
/// writable.
const TEST_FILE: &[u8] = b"/.ismount-test-file";

/// The root filesystem: is it read-only? Upstream creates and removes a
/// file in `/` to find out.
fn check_root(flags: &mut i32) {
    *flags |= MF_ISROOT;
    let mut opts = std::fs::OpenOptions::new();
    opts.read(true).write(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    if let Err(e) = opts.open(path_of(TEST_FILE))
        && errno_of(&e) == EROFS
    {
        *flags |= MF_READONLY;
    }
    // Upstream ignores whether the unlink worked: the file may never have
    // been made.
    let _ = std::fs::remove_file(path_of(TEST_FILE));
}

/// `check_mntent_file(mtab_file, file, &mount_flags, mtpt, mtlen)`: 0 and
/// what was found, or the `errno` of a table that cannot be read or of a
/// mount point that cannot be `stat`ed.
fn check_mntent_file(mtab_file: &[u8], file: &[u8], mtlen: usize, mc: &mut MountCheck) -> i32 {
    mc.flags = 0;
    let data = match std::fs::read(path_of(mtab_file)) {
        Ok(d) => d,
        Err(e) => return errno_of(&e),
    };
    let (mut file_dev, mut file_rdev, mut file_ino) = (0u64, 0u64, 0u64);
    if let Ok(st) = stat(file) {
        if st.blk {
            file_rdev = st.rdev;
        } else {
            file_dev = st.dev;
            file_ino = st.ino;
        }
    }
    let ents = getmntents(&data);
    let found = ents.iter().find(|mnt| {
        if mnt.fsname.first() != Some(&b'/') {
            return false;
        }
        if file == mnt.fsname.as_slice() {
            return true;
        }
        let Ok(st) = stat(&mnt.fsname) else {
            return false;
        };
        if st.blk {
            (file_rdev != 0 && file_rdev == st.rdev)
                || (file_dev != 0
                    && major(st.rdev) == LOOPDEV_MAJOR
                    && loopdev_is_used(&mnt.fsname, file))
        } else {
            file_dev != 0 && file_dev == st.dev && file_ino == st.ino
        }
    });
    let Some(mnt) = found else {
        // `/proc/mounts` may call the root `/dev/root`: compare the device
        // with the one `/` is on.
        if file_rdev != 0 && stat(b"/").is_ok_and(|st| st.dev == file_rdev) {
            mc.flags = MF_MOUNTED;
            mc.mtpt = xstrncpy(b"/", mtlen);
            check_root(&mut mc.flags);
        }
        return 0;
    };
    // The table may be out of date: the mount point must exist, and be on
    // the device.
    match stat(&mnt.dir) {
        Err(e) => return if e == ENOENT { 0 } else { e },
        Ok(st) => {
            if file_rdev != 0 && st.dev != file_rdev {
                return 0;
            }
        }
    }
    mc.flags = MF_MOUNTED;
    if hasmntopt(&mnt.opts, b"ro") {
        mc.flags |= MF_READONLY;
    }
    mc.mtpt = xstrncpy(&mnt.dir, mtlen);
    if mnt.dir == b"/" {
        check_root(&mut mc.flags);
    }
    0
}

/// `check_mntent(file, ...)`: `/proc/mounts`, or `/etc/mtab` where that
/// cannot be read.
fn check_mntent(file: &[u8], mtlen: usize, mc: &mut MountCheck) -> i32 {
    let retval = check_mntent_file(b"/proc/mounts", file, mtlen, mc);
    if retval == 0 && mc.flags != 0 {
        return 0;
    }
    if std::fs::File::open(path_of(b"/proc/mounts")).is_ok() {
        mc.flags = 0;
        return retval;
    }
    check_mntent_file(b"/etc/mtab", file, mtlen, mc)
}

/// `is_swap_device(file)`: listed in `/proc/swaps`, by name or by device
/// number.
fn is_swap_device(file: &[u8]) -> bool {
    let file_dev = stat(file).ok().filter(|st| st.blk).map_or(0, |st| st.rdev);
    let Ok(data) = std::fs::read(path_of(b"/proc/swaps")) else {
        return false;
    };
    let mut pos = 0usize;
    let Some(first) = fgets(&data, &mut pos, 1024) else {
        return false;
    };
    // Linux 2.6.19 and older could leave out the header.
    let mut pending =
        (!c_str(first).is_empty() && !first.starts_with(b"Filename\t")).then_some(first);
    loop {
        let buf = match pending.take() {
            Some(b) => b,
            None => match fgets(&data, &mut pos, 1024) {
                Some(b) => b,
                None => return false,
            },
        };
        let mut name = c_str(buf);
        if let Some(i) = name.iter().position(|&b| b == b' ') {
            name = name.get(..i).unwrap_or_default();
        }
        if let Some(i) = name.iter().position(|&b| b == b'\t') {
            name = name.get(..i).unwrap_or_default();
        }
        if name == file {
            return true;
        }
        if file_dev != 0 && stat(name).is_ok_and(|st| st.blk && st.rdev == file_dev) {
            return true;
        }
    }
}

/// `open(device, O_RDONLY|O_EXCL|O_CLOEXEC|O_NONBLOCK)` failing with
/// `EBUSY`: someone holds the block device exclusively.
fn is_busy(device: &[u8]) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // O_EXCL | O_NONBLOCK: Linux's values, which SlateOS's C library
        // shares.
        let mut opts = std::fs::OpenOptions::new();
        opts.read(true).custom_flags(0o200 | 0o4000);
        matches!(opts.open(path_of(device)), Err(e) if errno_of(&e) == EBUSY)
    }
    #[cfg(not(unix))]
    {
        let _ = device;
        false
    }
}

/// `check_mount_point(device, &mount_flags, mtpt, mtlen)`: whether the
/// device is mounted -- where, read-only, as the root -- a swap area in
/// use, or busy. `mtlen` is the size of the caller's buffer, as upstream
/// truncates the mount point to it.
///
/// # Errors
///
/// The `errno` of a mount table that cannot be read, or of a listed mount
/// point that cannot be `stat`ed for a reason other than not existing.
pub fn check_mount_point(device: &[u8], mtlen: usize) -> Result<MountCheck, i32> {
    let mut mc = MountCheck::default();
    if is_swap_device(device) {
        mc.flags = MF_MOUNTED | MF_SWAP;
        if mtlen != 0 {
            mc.mtpt = xstrncpy(b"[SWAP]", mtlen);
        }
    } else {
        let retval = check_mntent(device, mtlen, &mut mc);
        if retval != 0 {
            return Err(retval);
        }
    }
    if stat(device).is_ok_and(|st| st.blk) && is_busy(device) {
        mc.flags |= MF_BUSY;
    }
    Ok(mc)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "tests unwrap and index what they built"
)]
mod tests {
    use super::*;

    #[test]
    fn mount_tables_parse_as_glibc() {
        let t = b"# comment\n\n  /dev/sda1 /  ext4 rw,relatime 0 0\n/dev/sdb1\t/mnt/my\\040disk vfat ro 0 0   \n/x";
        let e = getmntents(t);
        assert_eq!(e.len(), 3);
        assert_eq!(e[0].fsname, b"/dev/sda1");
        assert_eq!(e[0].dir, b"/");
        assert_eq!(e[1].dir, b"/mnt/my disk");
        assert_eq!(e[1].opts, b"ro");
        assert_eq!(e[2].fsname, b"/x");
        assert!(e[2].dir.is_empty());
        // A line of 4095 bytes or more: its first 4095, the rest dropped.
        let mut long = vec![b'a'; 5000];
        long.extend_from_slice(b" /d\n/y /z\n");
        let e = getmntents(&long);
        assert_eq!(e.len(), 2);
        assert_eq!(e[0].fsname.len(), 4095);
        assert_eq!(e[1].fsname, b"/y");
    }

    #[test]
    fn names_decode_as_glibc() {
        assert_eq!(
            decode_name(b"a\\040b\\011c\\012d\\134e\\\\f\\x"),
            b"a b\tc\nd\\e\\f\\x"
        );
    }

    #[test]
    fn options_are_found_whole() {
        assert!(hasmntopt(b"rw,ro", b"ro"));
        assert!(hasmntopt(b"ro", b"ro"));
        assert!(hasmntopt(b"ro=1,rw", b"ro"));
        assert!(!hasmntopt(b"rw,rootcontext", b"ro"));
        assert!(!hasmntopt(b"errors=remount-ro", b"ro"));
        assert!(!hasmntopt(b"", b"ro"));
    }

    #[test]
    fn copies_truncate_as_xstrncpy() {
        assert_eq!(xstrncpy(b"/mnt/data", 5), b"/mnt");
        assert_eq!(xstrncpy(b"/", 80), b"/");
        assert!(xstrncpy(b"/x", 0).is_empty());
    }

    #[test]
    fn a_missing_device_is_not_mounted() {
        let mc = check_mount_point(b"/nonexistent-ulsysfs/dev", 80);
        // Either no table is readable (an errno) or nothing matched.
        if let Ok(mc) = mc {
            assert_eq!(mc.flags & (MF_MOUNTED | MF_BUSY), 0);
        }
    }
}
