//! `lib/sysfs.c`: the `sysfs_blkdev_*` dialect of [`PathCxt`], and the
//! conversions between device numbers, kernel names and `/dev` paths.

use std::rc::Rc;

use crate::path::{Blkdev, DirEntry, PathCxt};
use crate::{
    PATH_MAX, PATH_SYS_BLOCK, PATH_SYS_DEVBLOCK, PATH_SYS_DEVCHAR, R_OK, c_str, majmin, major,
    minor, path_of,
};

/// `sysfs_devname_sys_to_dev`: sysfs spells a `/` in a device name `!`.
#[must_use]
pub fn devname_sys_to_dev(name: &[u8]) -> Vec<u8> {
    name.iter()
        .map(|&b| if b == b'!' { b'/' } else { b })
        .collect()
}

/// `sysfs_devname_dev_to_sys`: the reverse.
#[must_use]
pub fn devname_dev_to_sys(name: &[u8]) -> Vec<u8> {
    name.iter()
        .map(|&b| if b == b'/' { b'!' } else { b })
        .collect()
}

/// `stripoff_last_component(path)`: cut `path` at its last `/`, returning
/// what followed it; `None` (and `path` untouched) with no `/`.
pub fn stripoff_last_component(path: &mut Vec<u8>) -> Option<Vec<u8>> {
    let slash = path.iter().rposition(|&b| b == b'/')?;
    let last = path
        .get(slash.saturating_add(1)..)
        .unwrap_or_default()
        .to_vec();
    path.truncate(slash);
    Some(last)
}

/// `xstrncpy(dest, src, n)`: at most `n - 1` bytes of `src`.
fn xstrncpy(src: &[u8], n: usize) -> Vec<u8> {
    let src = c_str(src);
    src.get(..src.len().min(n.saturating_sub(1)))
        .unwrap_or_default()
        .to_vec()
}

/// `ul_new_sysfs_path(devno, parent, prefix)`: `/sys/dev/block/MAJ:MIN`
/// (under `prefix`), which must open, with the `sysfs_blkdev` dialect
/// attached. `None` when the directory does not open.
#[must_use]
pub fn new_sysfs_path(
    devno: u64,
    parent: Option<Rc<PathCxt>>,
    prefix: Option<&[u8]>,
) -> Option<PathCxt> {
    let mut pc = PathCxt::new(None);
    pc.set_prefix(prefix);
    pc.blkdev_init(devno, parent).ok()?;
    Some(pc)
}

impl PathCxt {
    /// `sysfs_blkdev_init_path(pc, devno, parent)`.
    ///
    /// # Errors
    ///
    /// The `errno` of opening the directory.
    pub fn blkdev_init(&mut self, devno: u64, parent: Option<Rc<PathCxt>>) -> Result<(), i32> {
        let mut dir = PATH_SYS_DEVBLOCK.to_vec();
        dir.push(b'/');
        dir.extend_from_slice(majmin(devno).as_bytes());
        self.set_dir(Some(&dir));
        self.get_dirfd()?;
        let blk = self.blk.get_or_insert_with(Blkdev::default);
        blk.devno = devno;
        blk.parent = parent;
        Ok(())
    }

    /// `sysfs_blkdev_set_parent`.
    pub fn blkdev_set_parent(&mut self, parent: Option<Rc<PathCxt>>) {
        if let Some(blk) = self.blk.as_mut() {
            blk.parent = parent;
        }
    }

    /// `sysfs_blkdev_get_parent`.
    #[must_use]
    pub fn blkdev_parent(&self) -> Option<&Rc<PathCxt>> {
        self.blk.as_ref()?.parent.as_ref()
    }

    /// `sysfs_blkdev_get_devno`.
    #[must_use]
    pub fn blkdev_devno(&self) -> u64 {
        self.blk.as_ref().map_or(0, |b| b.devno)
    }

    /// `sysfs_blkdev_get_name(pc, buf, bufsiz)`: the kernel name, the last
    /// component of the directory's link with `!` read as `/`. `None` if it
    /// would not fit `bufsiz` with its NUL.
    #[must_use]
    pub fn blkdev_name(&self, bufsiz: usize) -> Option<Vec<u8>> {
        let link = self.readlink(None).ok()?;
        let slash = link.iter().rposition(|&b| b == b'/')?;
        let name = c_str(link.get(slash.saturating_add(1)..)?);
        if name.len().saturating_add(1) > bufsiz {
            return None;
        }
        Some(devname_sys_to_dev(name))
    }

    /// `sysfs_blkdev_get_path`: `/dev/NAME`, if that is a block device with
    /// this number.
    #[must_use]
    pub fn blkdev_path(&self) -> Option<Vec<u8>> {
        let name = self.blkdev_name(PATH_MAX)?;
        if name.len().saturating_add(b"/dev/".len()).saturating_add(1) > PATH_MAX {
            return None;
        }
        let mut p = b"/dev/".to_vec();
        p.extend_from_slice(&name);
        is_block_numbered(&p, self.blkdev_devno()).then_some(p)
    }

    /// `sysfs_blkdev_count_partitions(pc, devname)`.
    #[must_use]
    pub fn blkdev_count_partitions(&self, devname: Option<&[u8]>) -> usize {
        let Ok(dir) = self.get_dirfd() else {
            return 0;
        };
        let Ok(entries) = self.opendir(None) else {
            return 0;
        };
        entries
            .iter()
            .filter(|d| is_partition_dirent(&dir, d, devname))
            .count()
    }

    /// `sysfs_blkdev_partno_to_devno(pc, partno)`: the partition of this
    /// disk whose `partition` file holds `partno`, 0 if none.
    #[must_use]
    pub fn blkdev_partno_to_devno(&self, partno: i32) -> u64 {
        let Ok(dir) = self.get_dirfd() else {
            return 0;
        };
        let Ok(entries) = self.opendir(None) else {
            return 0;
        };
        for d in &entries {
            if !is_partition_dirent(&dir, d, None) {
                continue;
            }
            let mut p = d.name.clone();
            p.extend_from_slice(b"/partition");
            let Some(n) = self.read_s32(&p) else {
                continue;
            };
            if n == partno {
                let mut p = d.name.clone();
                p.extend_from_slice(b"/dev");
                if let Some(devno) = self.read_majmin(&p) {
                    return devno;
                }
            }
        }
        0
    }

    /// `sysfs_blkdev_get_slave`: the one entry of `slaves/`, `None` when
    /// there are none or several.
    #[must_use]
    pub fn blkdev_slave(&self) -> Option<Vec<u8>> {
        let entries = self.opendir(Some(b"slaves")).ok()?;
        match entries.as_slice() {
            [one] => Some(one.name.clone()),
            _ => None,
        }
    }

    /// `sysfs_blkdev_is_removable`: the `removable` attribute.
    #[must_use]
    pub fn blkdev_is_removable(&self) -> bool {
        self.read_s32(b"removable").is_some_and(|v| v != 0)
    }

    /// `get_dm_wholedisk`: a device-mapper partition's disk is its only
    /// slave.
    fn dm_wholedisk(&self, len: usize) -> Option<(Vec<u8>, u64)> {
        let name = self.blkdev_slave()?;
        let diskname = xstrncpy(&name, len);
        let devno = devname_to_devno_in(self.prefix(), &name, None);
        if devno == 0 {
            return None;
        }
        Some((diskname, devno))
    }

    /// `sysfs_blkdev_get_wholedisk(pc, diskname, len, &diskdevno)`: the
    /// whole disk this device is part of -- itself, when it is not a
    /// partition -- as its name (at most `len - 1` bytes, empty for
    /// `len == 0`) and its number. `None` when the name or number cannot be
    /// found.
    #[must_use]
    pub fn blkdev_wholedisk(&self, len: usize) -> Option<(Vec<u8>, u64)> {
        let mut is_part = self.access(crate::F_OK, b"partition").is_ok();
        if !is_part {
            // A partition mapped by device-mapper has no `partition` file,
            // but its DM UUID starts "part".
            let uuid = self.read_string(b"dm/uuid").ok().flatten();
            let prefix = uuid
                .as_deref()
                .map(|u| u.split(|&b| b == b'-').next().unwrap_or_default());
            if prefix.is_some_and(|p| {
                p.len() >= 4 && p.get(..4).is_some_and(|h| h.eq_ignore_ascii_case(b"part"))
            }) {
                is_part = true;
            }
            if is_part && let Some((name, devno)) = self.dm_wholedisk(len) {
                return Some((name, devno));
            }
            is_part = false;
        }
        if !is_part {
            let name = if len > 0 {
                self.blkdev_name(len)?
            } else {
                Vec::new()
            };
            return Some((name, self.blkdev_devno()));
        }
        // readlink /sys/dev/block/8:1 = ../../block/sda/sda1; its dirname's
        // basename, sda, is the disk.
        let mut link = self.readlink(None).ok()?;
        stripoff_last_component(&mut link);
        let name = stripoff_last_component(&mut link)?;
        let name = devname_sys_to_dev(&name);
        let diskname = if len > 0 {
            xstrncpy(&name, len)
        } else {
            Vec::new()
        };
        let devno = devname_to_devno_in(self.prefix(), &name, None);
        if devno == 0 {
            return None;
        }
        Some((diskname, devno))
    }
}

/// `S_ISBLK(stat(path))` with `st_rdev == devno`.
fn is_block_numbered(path: &[u8], devno: u64) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{FileTypeExt, MetadataExt};
        std::fs::metadata(path_of(path))
            .is_ok_and(|m| m.file_type().is_block_device() && m.rdev() == devno)
    }
    #[cfg(not(unix))]
    {
        let _ = (path, devno);
        false
    }
}

/// `sysfs_blkdev_is_partition_dirent(dir, d, parent_name)`: a directory (or
/// link, or unknown) named `PARENT<digit>` or `PARENTp<digit>` -- or, named
/// otherwise, with a readable `start` file. `dir` is the directory `d` is
/// in.
#[must_use]
pub fn is_partition_dirent(dir: &[u8], d: &DirEntry, parent_name: Option<&[u8]>) -> bool {
    if let Some(ft) = d.file_type
        && !ft.is_dir()
        && !ft.is_symlink()
    {
        return false;
    }
    let mut len = 0usize;
    if let Some(parent) = parent_name {
        // `/dev/sda` -> `sda`.
        let p = if parent.first() == Some(&b'/') {
            match parent.iter().rposition(|&b| b == b'/') {
                Some(i) => parent.get(i.saturating_add(1)..).unwrap_or_default(),
                None => return false,
            }
        } else {
            parent
        };
        len = p.len();
        if d.name.len() <= len || !d.name.starts_with(p) {
            len = 0;
        }
    }
    if len > 0 {
        let c = d.name.get(len).copied().unwrap_or(0);
        let next = d.name.get(len.saturating_add(1)).copied().unwrap_or(0);
        return (c == b'p' && next.is_ascii_digit()) || c.is_ascii_digit();
    }
    let mut p = dir.to_vec();
    p.push(b'/');
    p.extend_from_slice(&d.name);
    p.extend_from_slice(b"/start");
    let c = std::ffi::CString::new(p.clone());
    #[cfg(unix)]
    {
        unsafe extern "C" {
            fn access(path: *const std::ffi::c_char, mode: i32) -> i32;
        }
        let Ok(c) = c else {
            return false;
        };
        // SAFETY: `c` is NUL-terminated and outlives the call, which only
        // reads it.
        unsafe { access(c.as_ptr(), R_OK) == 0 }
    }
    #[cfg(not(unix))]
    {
        let _ = (c, R_OK);
        std::fs::metadata(path_of(&p)).is_ok()
    }
}

/// `sysfs_devno_to_wholedisk(devno, diskname, len, &diskdevno)`: `None`
/// for devno 0, a device sysfs does not have, or a disk that cannot be
/// found.
#[must_use]
pub fn devno_to_wholedisk(devno: u64, len: usize) -> Option<(Vec<u8>, u64)> {
    if devno == 0 {
        return None;
    }
    new_sysfs_path(devno, None, None)?.blkdev_wholedisk(len)
}

/// `sysfs_devno_is_dm_private(devno, &uuid)`: a private device-mapper
/// device -- an LVM `LVM-<uuid>-<name>` one, or a private Stratis one --
/// and the DM UUID, whatever it says.
#[must_use]
pub fn devno_is_dm_private(devno: u64) -> (bool, Option<Vec<u8>>) {
    let Some(pc) = new_sysfs_path(devno, None, None) else {
        return (false, None);
    };
    let Ok(Some(id)) = pc.read_string(b"dm/uuid") else {
        return (false, None);
    };
    let private = if let Some(rest) = id.strip_prefix(b"LVM-") {
        rest.iter()
            .rposition(|&b| b == b'-')
            .is_some_and(|p| p.saturating_add(1) < rest.len())
    } else {
        id.starts_with(b"stratis-1-private")
    };
    (private, Some(id))
}

/// `sysfs_devno_is_wholedisk`: `Some(true)` for a whole disk, `None` when it
/// cannot be told.
#[must_use]
pub fn devno_is_wholedisk(devno: u64) -> Option<bool> {
    let (_, disk) = devno_to_wholedisk(devno, 0)?;
    Some(devno == disk)
}

/// `read_devno(path)`: `fscanf("%d:%d")` of a `dev` file, 0 on failure.
fn read_devno(path: &[u8]) -> u64 {
    std::fs::read(path_of(path))
        .ok()
        .and_then(|t| crate::path::scan_majmin(&t))
        .unwrap_or(0)
}

/// `sysfs_devname_is_hidden(prefix, name)`: `/sys/block/NAME/hidden`.
#[must_use]
pub fn devname_is_hidden(prefix: Option<&[u8]>, name: &[u8]) -> bool {
    if name.starts_with(b"/dev/") {
        return false;
    }
    let mut p = prefix.unwrap_or_default().to_vec();
    p.extend_from_slice(PATH_SYS_BLOCK);
    p.push(b'/');
    p.extend_from_slice(name);
    p.extend_from_slice(b"/hidden");
    if p.len().saturating_add(1) > PATH_MAX {
        return false;
    }
    std::fs::read(path_of(&p))
        .ok()
        .and_then(|t| crate::path::scan_int(&t, &mut 0))
        .is_some_and(|h| h != 0)
}

/// `__sysfs_devname_to_devno(prefix, name, parent)`: a device's number --
/// from `stat` for a `/dev/` name that exists, else from sysfs:
/// `/sys/block/PARENT/NAME/dev` for a partition named with its parent,
/// else `/sys/block/NAME/dev`, `/sys/block/PARENT/NAME/dev`,
/// `/sys/block/NAME/device/dev`. 0 when none says.
#[must_use]
pub fn devname_to_devno_in(prefix: Option<&[u8]>, name: &[u8], parent: Option<&[u8]>) -> u64 {
    let prefix = prefix.unwrap_or_default();
    let mut name = name;
    if let Some(rest) = name.strip_prefix(b"/dev/") {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if let Ok(m) = std::fs::metadata(path_of(name)) {
                return m.rdev();
            }
        }
        name = rest;
    }
    let sys_name = devname_dev_to_sys(name);
    let under_block = |parts: &[&[u8]]| -> Option<Vec<u8>> {
        let mut p = prefix.to_vec();
        p.extend_from_slice(PATH_SYS_BLOCK);
        for part in parts {
            p.push(b'/');
            p.extend_from_slice(part);
        }
        (p.len() < PATH_MAX).then_some(p)
    };
    if let Some(parent) = parent
        && !name.starts_with(b"dm-")
    {
        let sys_parent = devname_dev_to_sys(parent);
        return under_block(&[&sys_parent, &sys_name, b"dev"]).map_or(0, |p| read_devno(&p));
    }
    let Some(p) = under_block(&[&sys_name, b"dev"]) else {
        return 0;
    };
    let mut dev = read_devno(&p);
    if dev == 0
        && let Some(parent) = parent
        && name.starts_with(parent)
    {
        // `_parent` here is the copy that was never converted to sysfs'
        // spelling -- upstream converts it only in the branch above.
        match under_block(&[parent, &sys_name, b"dev"]) {
            Some(p) => dev = read_devno(&p),
            None => return 0,
        }
    }
    if dev == 0 {
        match under_block(&[&sys_name, b"device", b"dev"]) {
            Some(p) => dev = read_devno(&p),
            None => return 0,
        }
    }
    dev
}

/// `sysfs_devname_to_devno(name)`.
#[must_use]
pub fn devname_to_devno(name: &[u8]) -> u64 {
    devname_to_devno_in(None, name, None)
}

/// `sysfs_devno_to_devpath(devno)`: `/dev/NAME`, if that is the device.
#[must_use]
pub fn devno_to_devpath(devno: u64) -> Option<Vec<u8>> {
    new_sysfs_path(devno, None, None)?.blkdev_path()
}

/// `sysfs_devno_to_devname(devno, buf, bufsiz)`: the kernel name.
#[must_use]
pub fn devno_to_devname(devno: u64, bufsiz: usize) -> Option<Vec<u8>> {
    new_sysfs_path(devno, None, None)?.blkdev_name(bufsiz)
}

/// `sysfs_devno_count_partitions(devno)`.
#[must_use]
pub fn devno_count_partitions(devno: u64) -> usize {
    let Some(pc) = new_sysfs_path(devno, None, None) else {
        return 0;
    };
    let name = pc.blkdev_name(PATH_MAX.saturating_add(1));
    pc.blkdev_count_partitions(name.as_deref())
}

/// `sysfs_chrdev_devno_to_devname(devno, buf, bufsiz)`: a character
/// device's kernel name, from where `/sys/dev/char/MAJ:MIN` links.
#[must_use]
pub fn chrdev_devno_to_devname(devno: u64, bufsiz: usize) -> Option<Vec<u8>> {
    let mut dir = PATH_SYS_DEVCHAR.to_vec();
    // `"%u:%u"` here, unlike the block side's `%d:%d`.
    dir.extend_from_slice(format!("/{}:{}", major(devno), minor(devno)).as_bytes());
    let pc = PathCxt::new(Some(&dir));
    let link = pc.readlink(None).ok()?;
    let slash = link.iter().rposition(|&b| b == b'/')?;
    let name = c_str(link.get(slash.saturating_add(1)..)?);
    if name.len().saturating_add(1) > bufsiz {
        return None;
    }
    Some(devname_sys_to_dev(name))
}
