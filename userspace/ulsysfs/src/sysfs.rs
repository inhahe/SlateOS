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

/// `_PATH_SYS_CLASS`.
const PATH_SYS_CLASS: &[u8] = b"/sys/class";
/// `_PATH_SYS_SCSI`.
const PATH_SYS_SCSI: &[u8] = b"/sys/bus/scsi";

/// `readlink(path, buf, PATH_MAX - 1)`: the link's target, at most
/// `PATH_MAX - 1` bytes of it, or `None`.
fn readlink_bytes(path: &[u8]) -> Option<Vec<u8>> {
    let link = std::fs::read_link(path_of(path)).ok()?;
    let mut bytes = quoting::os_bytes(link.as_os_str()).into_owned();
    bytes.truncate(PATH_MAX.saturating_sub(1));
    Some(bytes)
}

/// `get_subsystem(chain, buf, bufsz)`: the subsystem of the deepest
/// directory of `chain` that has a `subsystem` link -- the link's last
/// component -- with `chain` cut, component by component, to that
/// directory's parent. `None`, and `chain` cut to nothing, when no
/// directory left in it has one.
fn get_subsystem(chain: &mut Vec<u8>) -> Option<Vec<u8>> {
    const LINKNAME: &[u8] = b"/subsystem";
    if chain.is_empty() {
        return None;
    }
    // `len + sizeof(SUBSYSTEM_LINKNAME) > PATH_MAX`: the terminating NUL
    // counted.
    if chain.len().saturating_add(LINKNAME.len()).saturating_add(1) > PATH_MAX {
        return None;
    }
    loop {
        let mut link = chain.clone();
        link.extend_from_slice(LINKNAME);
        let target = readlink_bytes(&link);
        // The last component off the chain.
        let slash = chain.iter().rposition(|&b| b == b'/');
        if let Some(s) = slash {
            chain.truncate(s);
        }
        if let Some(t) = target.filter(|t| !t.is_empty()) {
            // `basename(buf)`: what follows the last `/`.
            let start = t
                .iter()
                .rposition(|&b| b == b'/')
                .map_or(0, |i| i.saturating_add(1));
            return Some(t.get(start..).unwrap_or_default().to_vec());
        }
        slash?;
    }
}

/// `sysfs_devchain_is_removable(chain)`: the deepest `removable` file in
/// the chain that says `fixed` (no) or `removable` (yes) -- compared, as
/// upstream compares it, over no more bytes than the file held.
fn devchain_is_removable(chain: &[u8]) -> bool {
    const FILENAME: &[u8] = b"/removable";
    if chain.is_empty() || chain.len().saturating_add(FILENAME.len()).saturating_add(1) > PATH_MAX
    {
        return false;
    }
    let mut chain = chain.to_vec();
    loop {
        let mut p = chain.clone();
        p.extend_from_slice(FILENAME);
        if let Ok(mut f) = std::fs::File::open(path_of(&p))
            && let Ok(buf) = crate::path::read_all(&mut f, 20)
            && !buf.is_empty()
        {
            let starts = |word: &[u8]| {
                let n = buf.len().min(word.len());
                buf.get(..n) == word.get(..n)
            };
            if starts(b"fixed") {
                return false;
            } else if starts(b"removable") {
                return true;
            }
        }
        let slash = chain.iter().rposition(|&b| b == b'/');
        match slash {
            Some(s) => chain.truncate(s),
            None => return false,
        }
    }
}

impl PathCxt {
    /// `sysfs_blkdev_get_devchain(pc, buf, PATH_MAX)`: the device's whole
    /// path in sysfs, as `/sys/dev/block/` (under the prefix) followed by
    /// where the device's directory links -- every subsystem it hangs from
    /// is a directory of it.
    ///
    /// Upstream copies the result into the caller's buffer without its
    /// terminating NUL, which the shorter link read into that buffer
    /// before left behind it -- so its string runs on into whatever the
    /// stack held. This one ends where the path does.
    #[must_use]
    pub fn blkdev_devchain(&self) -> Option<Vec<u8>> {
        let link = self.readlink(None).ok().filter(|l| !l.is_empty())?;
        let mut chain = self.prefix().unwrap_or_default().to_vec();
        chain.extend_from_slice(PATH_SYS_DEVBLOCK);
        chain.push(b'/');
        chain.extend_from_slice(&link);
        (chain.len() < PATH_MAX).then_some(chain)
    }

    /// `sysfs_blkdev_is_hotpluggable`: some device in the chain says it is
    /// removable before one says it is fixed.
    #[must_use]
    pub fn blkdev_is_hotpluggable(&self) -> bool {
        self.blkdev_devchain()
            .is_some_and(|chain| devchain_is_removable(&chain))
    }

    /// `sysfs_blkdev_scsi_get_hctl(pc, &h, &c, &t, &l)`: host, channel,
    /// target and LUN, read once from where the `device` link leads
    /// (`.../H:C:T:L`) and kept. A device that has none is asked only once:
    /// every later call fails at once.
    ///
    /// # Errors
    ///
    /// The `errno` of the link's read, or `EINVAL` -- for a directory with
    /// no `sysfs_blkdev` dialect, a link not ending in four numbers, or a
    /// device already found to have none.
    pub fn blkdev_scsi_hctl(&self) -> Result<[i32; 4], i32> {
        use crate::path::Hctl;
        let blk = self.blk.as_ref().ok_or(crate::EINVAL)?;
        match blk.hctl.get() {
            Hctl::Failed => return Err(crate::EINVAL),
            Hctl::Known(h) => return Ok(h),
            Hctl::Unknown => {}
        }
        blk.hctl.set(Hctl::Failed);
        let link = self.readlink(Some(b"device"))?;
        let slash = link
            .iter()
            .rposition(|&b| b == b'/')
            .ok_or(crate::EINVAL)?;
        let hctl = scan_hctl(link.get(slash.saturating_add(1)..).unwrap_or_default())
            .ok_or(crate::EINVAL)?;
        blk.hctl.set(Hctl::Known(hctl));
        Ok(hctl)
    }

    /// `scsi_host_attribute_path(pc, type, buf, bufsz, attr)`:
    /// `/sys/class/TYPE_host/hostH[/ATTR]` under the prefix, if it fits in
    /// `bufsz` with its NUL.
    fn scsi_host_attribute_path(&self, ty: &[u8], bufsz: usize, attr: Option<&[u8]>) -> Option<Vec<u8>> {
        let [host, ..] = self.blkdev_scsi_hctl().ok()?;
        let mut p = self.prefix().unwrap_or_default().to_vec();
        p.extend_from_slice(PATH_SYS_CLASS);
        p.push(b'/');
        p.extend_from_slice(ty);
        p.extend_from_slice(format!("_host/host{host}").as_bytes());
        if let Some(a) = attr {
            p.push(b'/');
            p.extend_from_slice(a);
        }
        (p.len() < bufsz).then_some(p)
    }

    /// `sysfs_blkdev_scsi_host_strdup_attribute(pc, type, attr)`: the first
    /// line of the SCSI host's attribute (1023 bytes of it at most) --
    /// `None` if it is empty.
    #[must_use]
    pub fn blkdev_scsi_host_attribute(&self, ty: &[u8], attr: &[u8]) -> Option<Vec<u8>> {
        let path = self.scsi_host_attribute_path(ty, 1024, Some(attr))?;
        let text = std::fs::read(path_of(&path)).ok()?;
        // `fscanf(f, "%1023[^\n]", buf)`: at least one byte before the
        // first newline, or nothing matched; then `strdup(buf)`, which
        // stops at a NUL among them.
        let line = text.split(|&b| b == b'\n').next().unwrap_or_default();
        let line = line.get(..line.len().min(1023)).unwrap_or_default();
        (!line.is_empty()).then(|| c_str(line).to_vec())
    }

    /// `sysfs_blkdev_scsi_host_is(pc, type)`: whether the device's SCSI host
    /// is of that class -- `/sys/class/TYPE_host/hostH` is a directory.
    #[must_use]
    pub fn blkdev_scsi_host_is(&self, ty: &[u8]) -> bool {
        self.scsi_host_attribute_path(ty, PATH_MAX, None)
            .and_then(|p| std::fs::metadata(path_of(&p)).ok())
            .is_some_and(|m| m.is_dir())
    }

    /// `scsi_attribute_path(pc, buf, bufsz, attr)`:
    /// `/sys/bus/scsi/devices/H:C:T:L[/ATTR]` under the prefix.
    fn scsi_attribute_path(&self, attr: Option<&[u8]>) -> Option<Vec<u8>> {
        let [h, c, t, l] = self.blkdev_scsi_hctl().ok()?;
        let mut p = self.prefix().unwrap_or_default().to_vec();
        p.extend_from_slice(PATH_SYS_SCSI);
        p.extend_from_slice(format!("/devices/{h}:{c}:{t}:{l}").as_bytes());
        if let Some(a) = attr {
            p.push(b'/');
            p.extend_from_slice(a);
        }
        (p.len() < PATH_MAX).then_some(p)
    }

    /// `sysfs_blkdev_scsi_has_attribute(pc, attr)`: the SCSI device has it.
    #[must_use]
    pub fn blkdev_scsi_has_attribute(&self, attr: &[u8]) -> bool {
        self.scsi_attribute_path(Some(attr))
            .is_some_and(|p| std::fs::metadata(path_of(&p)).is_ok())
    }

    /// `sysfs_blkdev_scsi_path_contains(pc, pattern)`: where the SCSI
    /// device's directory links holds `pattern`.
    #[must_use]
    pub fn blkdev_scsi_path_contains(&self, pattern: &[u8]) -> bool {
        let Some(p) = self.scsi_attribute_path(None) else {
            return false;
        };
        if std::fs::metadata(path_of(&p)).is_err() {
            return false;
        }
        readlink_bytes(&p).is_some_and(|link| {
            pattern.is_empty() || link.windows(pattern.len()).any(|w| w == pattern)
        })
    }
}

/// `sysfs_blkdev_next_subsystem(pc, devchain, &subsys)`: the next subsystem
/// up the chain [`PathCxt::blkdev_devchain`] gave, which it cuts as it
/// goes; `None` at the end of it.
#[must_use]
pub fn next_subsystem(devchain: &mut Vec<u8>) -> Option<Vec<u8>> {
    get_subsystem(devchain)
}

/// `sscanf(hctl, "%u:%u:%u:%u", ...)` into four `int`s: each number as
/// `strtoul` reads it (a `-` negating it, overflow clamping), its low 32
/// bits stored, and printed back signed as upstream's `%d` does.
pub(crate) fn scan_hctl(text: &[u8]) -> Option<[i32; 4]> {
    let mut pos = 0usize;
    let mut out = [0i32; 4];
    for (i, slot) in out.iter_mut().enumerate() {
        if i > 0 {
            if text.get(pos) != Some(&b':') {
                return None;
            }
            pos = pos.saturating_add(1);
        }
        let v = crate::path::scan_ulong(text, &mut pos)?;
        // `*(unsigned int *) = num.ul`, then read back as the `int` it is.
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_possible_wrap,
            reason = "C stores the unsigned long in an int"
        )]
        {
            *slot = v as u32 as i32;
        }
    }
    Some(out)
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
