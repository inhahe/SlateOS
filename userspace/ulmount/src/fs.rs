//! libmount's `fs.c`: one line of a mount table -- fstab, mountinfo,
//! `/proc/mounts`, `/proc/swaps` or utab -- and what can be asked of it.

use crate::optmap::{self, MNT_INVERT, MS_PRIVATE, MS_SHARED, MS_SLAVE, MS_UNBINDABLE, Map};
use crate::optstr;
use crate::utils;

/// `MNT_FS_PSEUDO`: a pseudo filesystem (proc, tmpfs, ...).
pub const MNT_FS_PSEUDO: u32 = 1 << 1;
/// `MNT_FS_NET`: a network filesystem.
pub const MNT_FS_NET: u32 = 1 << 2;
/// `MNT_FS_SWAP`: a swap area.
pub const MNT_FS_SWAP: u32 = 1 << 3;
/// `MNT_FS_KERNEL`: read from the kernel (mountinfo, `/proc/mounts`).
pub const MNT_FS_KERNEL: u32 = 1 << 4;
/// `MNT_FS_MERGED`: utab's options already merged in.
pub const MNT_FS_MERGED: u32 = 1 << 5;

/// glibc's `makedev`, `major` and `minor`, from lib/sysfs.c's port.
pub use ulsysfs::{major, makedev, minor};

/// `struct libmnt_fs`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Fs {
    /// mountinfo's mount ID.
    pub id: i32,
    /// mountinfo's parent ID.
    pub parent: i32,
    /// mountinfo's `maj:min`.
    pub devno: u64,
    /// utab's full source path of a bind mount.
    pub bindsrc: Option<Vec<u8>>,
    /// The source: a device, a file, a directory, or `NAME=value`.
    pub source: Option<Vec<u8>>,
    /// The tag's name, when the source is one (`LABEL`, `UUID`, ...).
    pub tagname: Option<Vec<u8>>,
    /// The tag's value.
    pub tagval: Option<Vec<u8>>,
    /// mountinfo's root of the mount within its filesystem.
    pub root: Option<Vec<u8>>,
    /// The mount point.
    pub target: Option<Vec<u8>>,
    pub fstype: Option<Vec<u8>>,
    /// fstab's options, or mountinfo's VFS and FS options merged.
    pub optstr: Option<Vec<u8>>,
    /// mountinfo's VFS (filesystem-independent) options.
    pub vfs_optstr: Option<Vec<u8>>,
    /// mountinfo's optional fields (`shared:1 master:2`).
    pub opt_fields: Option<Vec<u8>>,
    /// mountinfo's filesystem-specific options.
    pub fs_optstr: Option<Vec<u8>>,
    /// Userspace-only options (`noauto`, `x-...`).
    pub user_optstr: Option<Vec<u8>>,
    pub attrs: Option<Vec<u8>>,
    pub freq: i32,
    pub passno: i32,
    /// `/proc/swaps`' type column.
    pub swaptype: Option<Vec<u8>>,
    pub size: i64,
    pub usedsize: i64,
    pub priority: i32,
    /// `MNT_FS_*`.
    pub flags: u32,
    /// The thread whose `/proc/<tid>/mountinfo` this came from.
    pub tid: i32,
    pub comment: Option<Vec<u8>>,
}

impl Fs {
    /// `__mnt_fs_set_source_ptr(fs, source)`: the source, and its tag if it
    /// is `NAME=value` with a name libmount knows -- another name with an
    /// `=` is a plain source.
    pub fn set_source(&mut self, source: Option<Vec<u8>>) {
        let tag = source
            .as_deref()
            .and_then(utils::parse_tag_string)
            .filter(|(name, _)| utils::valid_tagname(name));
        let (t, v) = match tag {
            Some((t, v)) => (Some(t), Some(v)),
            None => (None, None),
        };
        self.source = source;
        self.tagname = t;
        self.tagval = v;
    }

    /// `mnt_fs_get_srcpath(fs)`: the source unless it is a tag.
    #[must_use]
    pub fn srcpath(&self) -> Option<&[u8]> {
        if self.tagname.is_some() {
            return None;
        }
        self.source.as_deref()
    }

    /// `mnt_fs_get_tag(fs, &name, &value)`.
    #[must_use]
    pub fn tag(&self) -> Option<(&[u8], &[u8])> {
        Some((
            self.tagname.as_deref()?,
            self.tagval.as_deref().unwrap_or_default(),
        ))
    }

    /// `__mnt_fs_set_fstype_ptr(fs, fstype)`: the type, and the flags it
    /// implies.
    pub fn set_fstype(&mut self, fstype: Option<Vec<u8>>) {
        self.flags &= !(MNT_FS_PSEUDO | MNT_FS_NET | MNT_FS_SWAP);
        if let Some(t) = &fstype {
            if utils::fstype_is_pseudofs(t) {
                self.flags |= MNT_FS_PSEUDO;
            } else if utils::fstype_is_netfs(t) {
                self.flags |= MNT_FS_NET;
            } else if t == b"swap" {
                self.flags |= MNT_FS_SWAP;
            }
        }
        self.fstype = fstype;
    }

    #[must_use]
    pub fn is_kernel(&self) -> bool {
        self.flags & MNT_FS_KERNEL != 0
    }

    #[must_use]
    pub fn is_swaparea(&self) -> bool {
        self.flags & MNT_FS_SWAP != 0
    }

    #[must_use]
    pub fn is_pseudofs(&self) -> bool {
        self.flags & MNT_FS_PSEUDO != 0
    }

    #[must_use]
    pub fn is_netfs(&self) -> bool {
        self.flags & MNT_FS_NET != 0
    }

    /// `mnt_fs_set_options(fs, optstr)`: the options, split into VFS,
    /// filesystem and userspace ones as fstab's are.
    ///
    /// # Errors
    ///
    /// The options do not scan.
    pub fn set_options(&mut self, opts: Option<&[u8]>) -> Result<(), optstr::Invalid> {
        let (u, v, f, n) = match opts {
            Some(o) => {
                let (u, v, f) = optstr::split_optstr(o, 0, 0)?;
                (u, v, f, Some(o.to_vec()))
            }
            None => (None, None, None, None),
        };
        self.fs_optstr = f;
        self.vfs_optstr = v;
        self.user_optstr = u;
        self.optstr = n;
        Ok(())
    }

    /// `mnt_fs_append_options(fs, optstr)`.
    ///
    /// # Errors
    ///
    /// The options do not scan.
    pub fn append_options(&mut self, opts: Option<&[u8]>) -> Result<(), optstr::Invalid> {
        let Some(o) = opts else {
            return Ok(());
        };
        let (u, v, f) = optstr::split_optstr(o, 0, 0)?;
        if let Some(v) = v {
            optstr::append_option(&mut self.vfs_optstr, &v, None);
        }
        if let Some(f) = f {
            optstr::append_option(&mut self.fs_optstr, &f, None);
        }
        if let Some(u) = u {
            optstr::append_option(&mut self.user_optstr, &u, None);
        }
        optstr::append_option(&mut self.optstr, o, None);
        Ok(())
    }

    /// `mnt_fs_strdup_options(fs)`: the options as one string -- fstab's as
    /// they are, mountinfo's VFS and FS options merged (`merge_optstr`) with
    /// the userspace ones after.
    #[must_use]
    pub fn strdup_options(&self) -> Option<Vec<u8>> {
        if let Some(o) = &self.optstr {
            return Some(o.clone());
        }
        let mut res = merge_optstr(self.vfs_optstr.as_deref(), self.fs_optstr.as_deref());
        if let Some(u) = &self.user_optstr {
            optstr::append_option(&mut res, u, None);
        }
        res
    }

    /// `mnt_fs_get_option(fs, name, &value, &valsz)`: the option in the
    /// filesystem's options, else the VFS ones, else the userspace ones --
    /// `Some(value)` when it is there (`value` `None` for one without `=`).
    ///
    /// # Errors
    ///
    /// An option string that does not parse (an unclosed quote).
    pub fn get_option(&self, name: &[u8]) -> Result<Option<Option<&[u8]>>, optstr::Invalid> {
        for s in [&self.fs_optstr, &self.vfs_optstr, &self.user_optstr]
            .into_iter()
            .flatten()
        {
            if let Some(v) = optstr::get_option(s, name)? {
                return Ok(Some(v));
            }
        }
        Ok(None)
    }

    /// `mnt_fs_get_options(fs)`.
    #[must_use]
    pub fn options(&self) -> Option<&[u8]> {
        self.optstr.as_deref()
    }

    /// `mnt_fs_get_vfs_options_all(fs)`: every VFS flag's state spelled
    /// out -- `rw,exec,suid,...` -- as the kernel's defaults and the
    /// options set them.
    #[must_use]
    pub fn vfs_options_all(&self) -> Option<Vec<u8>> {
        let opts = self.options()?;
        let flags = optstr::get_flags(opts, Map::Linux, 0);
        let mut result: Option<Vec<u8>> = None;
        for ent in optmap::LINUX_MAP {
            if ent.id & flags != 0 {
                if ent.mask & MNT_INVERT == 0 {
                    optstr::append_option(&mut result, ent.name, None);
                }
            } else if ent.mask & MNT_INVERT != 0 {
                optstr::append_option(&mut result, ent.name, None);
            }
        }
        result
    }

    /// `mnt_fs_get_propagation(fs, &flags)`: from mountinfo's optional
    /// fields.
    #[must_use]
    pub fn propagation(&self) -> u64 {
        let Some(f) = &self.opt_fields else {
            return 0;
        };
        let has = |w: &[u8]| utils_strstr(f, w);
        let mut flags = if has(b"shared:") {
            MS_SHARED
        } else {
            MS_PRIVATE
        };
        if has(b"master:") {
            flags |= MS_SLAVE;
        }
        if has(b"unbindable") {
            flags |= MS_UNBINDABLE;
        }
        flags
    }

    /// `mnt_fs_streq_srcpath(fs, path)`: the source path is `path` --
    /// compared as paths, except a pseudo filesystem's, compared as bytes.
    #[must_use]
    pub fn streq_srcpath(&self, path: Option<&[u8]>) -> bool {
        let p = self.srcpath();
        if !self.is_pseudofs() {
            return utils::streq_paths(p, path);
        }
        match (p, path) {
            (None, None) => true,
            (Some(a), Some(b)) => a == b,
            _ => false,
        }
    }

    /// `mnt_fs_streq_target(fs, path)`.
    #[must_use]
    pub fn streq_target(&self, path: Option<&[u8]>) -> bool {
        utils::streq_paths(self.target.as_deref(), path)
    }

    /// `mnt_fs_match_fstype(fs, types)`.
    #[must_use]
    pub fn match_fstype(&self, types: Option<&[u8]>) -> bool {
        utils::match_fstype(self.fstype.as_deref(), types)
    }

    /// `mnt_fs_match_options(fs, options)`.
    #[must_use]
    pub fn match_options(&self, options: Option<&[u8]>) -> bool {
        optstr::match_options(self.options(), options)
    }
}

/// `strstr(hay, needle) != NULL`.
fn utils_strstr(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

/// `merge_optstr(vfs, fs)`: the kernel's VFS and filesystem options as one
/// string, with one `rw` or `ro` in front: `ro` if either says `ro` and
/// they do not both say `rw`.
#[must_use]
pub fn merge_optstr(vfs: Option<&[u8]>, fs: Option<&[u8]>) -> Option<Vec<u8>> {
    let (vfs, fs) = match (vfs, fs) {
        (None, None) => return None,
        (Some(v), None) => return Some(v.to_vec()),
        (None, Some(f)) => return Some(f.to_vec()),
        (Some(v), Some(f)) => (v, f),
    };
    if vfs == fs {
        return Some(vfs.to_vec());
    }
    let mut p = vfs.to_vec();
    p.push(b',');
    p.extend_from_slice(fs);
    let mut rw = 0i32;
    let mut ro = 0i32;
    // `!mnt_optstr_remove_option(&p, "rw")`: 1 when removed. A scan error
    // (which the merged string cannot have) counts as not removed.
    let remove = |p: &mut Vec<u8>, name: &[u8]| -> i32 {
        i32::from(optstr::remove_option(p, name) == Ok(true))
    };
    rw = rw.saturating_add(remove(&mut p, b"rw"));
    rw = rw.saturating_add(remove(&mut p, b"rw"));
    if rw != 2 {
        ro = ro.saturating_add(remove(&mut p, b"ro"));
        if ro.saturating_add(rw) < 2 {
            ro = ro.saturating_add(remove(&mut p, b"ro"));
        }
    }
    let mut res = if ro > 0 {
        b"ro".to_vec()
    } else {
        b"rw".to_vec()
    };
    if !p.is_empty() {
        res.push(b',');
        res.extend_from_slice(&p);
    }
    Some(res)
}

#[cfg(test)]
#[allow(
    clippy::indexing_slicing,
    clippy::unwrap_used,
    reason = "tests index what they built"
)]
mod tests {
    use super::*;

    #[test]
    fn devices_are_glibcs() {
        let d = makedev(8, 1);
        assert_eq!(d, 0x801);
        assert_eq!((major(d), minor(d)), (8, 1));
        let d = makedev(259, 300);
        assert_eq!((major(d), minor(d)), (259, 300));
    }

    #[test]
    fn tags_are_recognised() {
        let mut fs = Fs::default();
        fs.set_source(Some(b"LABEL=root".to_vec()));
        assert_eq!(fs.tag(), Some((&b"LABEL"[..], &b"root"[..])));
        assert_eq!(fs.srcpath(), None);
        fs.set_source(Some(b"FOO=bar".to_vec()));
        assert_eq!(fs.tag(), None);
        assert_eq!(fs.srcpath(), Some(&b"FOO=bar"[..]));
    }

    #[test]
    fn options_merge_as_upstream() {
        let m = |v: &str, f: &str| merge_optstr(Some(v.as_bytes()), Some(f.as_bytes()));
        assert_eq!(
            m("rw,noatime", "rw,data=ordered").as_deref(),
            Some(&b"rw,noatime,data=ordered"[..])
        );
        assert_eq!(m("ro,noatime", "rw").as_deref(), Some(&b"ro,noatime"[..]));
        assert_eq!(m("rw", "rw").as_deref(), Some(&b"rw"[..]));
        assert_eq!(m("rw,relatime", "ro").as_deref(), Some(&b"ro,relatime"[..]));
        assert_eq!(merge_optstr(None, Some(b"x")).as_deref(), Some(&b"x"[..]));
    }

    #[test]
    fn types_set_their_flags() {
        let mut fs = Fs::default();
        fs.set_fstype(Some(b"proc".to_vec()));
        assert!(fs.is_pseudofs());
        fs.set_fstype(Some(b"nfs4".to_vec()));
        assert!(fs.is_netfs() && !fs.is_pseudofs());
        fs.set_fstype(Some(b"swap".to_vec()));
        assert!(fs.is_swaparea());
    }

    #[test]
    fn propagation_is_read_from_the_fields() {
        let mut fs = Fs::default();
        assert_eq!(fs.propagation(), 0);
        fs.opt_fields = Some(b"shared:1 master:2".to_vec());
        assert_eq!(fs.propagation(), MS_SHARED | MS_SLAVE);
        fs.opt_fields = Some(b"unbindable".to_vec());
        assert_eq!(fs.propagation(), MS_PRIVATE | MS_UNBINDABLE);
    }
}
