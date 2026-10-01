//! `lsblk-mnt.c`: where a device is mounted, or used as swap -- the mount
//! table and `/proc/swaps` (under `--sysroot` if given), each read once,
//! the first time a device asks.

use ulmount::cache::Cache;
use ulmount::fs::Fs;
use ulmount::tab::{Direction, Table};
use ulmount::tab_parse;

use crate::devtree::{DevId, Devtree, FsRef};

/// The tables, read on first use, and the cache they share
/// (`mtab`, `swaps`, `mntcache`).
#[derive(Default)]
pub(crate) struct Mnt {
    mtab: Option<Table>,
    swaps: Option<Table>,
    cache: Option<Cache>,
}

/// `table_parser_errcb`: a line that does not parse is reported and
/// skipped.
fn parser_errcb(short: &[u8]) -> impl FnMut(&[u8], usize) -> i32 + '_ {
    move |filename: &[u8], line: usize| {
        ulclosestream::warnx(
            short,
            &format!(
                "{}: parse error at line {line} -- ignored",
                quoting::escape_unprintable(filename)
            ),
        );
        1
    }
}

impl Mnt {
    /// The entry `r` refers to.
    pub(crate) fn fs(&self, r: FsRef) -> Option<&Fs> {
        match r {
            FsRef::Mtab(i) => self.mtab.as_ref()?.ents.get(i),
            FsRef::Swaps(i) => self.swaps.as_ref()?.ents.get(i),
        }
    }

    /// `get_active_swap(filename)`: the last swap area in `/proc/swaps`
    /// with this source.
    fn get_active_swap(
        &mut self,
        filename: &[u8],
        sysroot: Option<&[u8]>,
        short: &[u8],
    ) -> Option<usize> {
        if self.swaps.is_none() {
            let mut tb = Table::new();
            let path = sysroot.map(|root| [root, &b"/proc/swaps"[..]].concat());
            let mut cb = parser_errcb(short);
            // What parsed before a failure stays in the table; upstream does
            // not look at the result either.
            let _ = tab_parse::parse_swaps(&mut tb, path.as_deref(), Some(&mut cb));
            self.swaps = Some(tb);
        }
        let cache = self.cache.get_or_insert_with(Cache::new);
        self.swaps
            .as_ref()?
            .find_srcpath(filename, Direction::Backward, Some(cache))
    }

    /// `lsblk_device_get_filesystems(dev)`: every mount of the device --
    /// by number or source -- the last mounted first; failing those, its
    /// swap area, or a mount found by the source's canonical path.
    pub(crate) fn get_filesystems(
        &mut self,
        tr: &mut Devtree,
        id: DevId,
        sysroot: Option<&[u8]>,
        short: &[u8],
    ) -> Vec<FsRef> {
        if tr.dev(id).is_mounted {
            return tr.dev(id).fss.clone();
        }
        let dev = tr.dev_mut(id);
        dev.fss.clear();
        dev.is_mounted = false;
        dev.is_swap = false;
        let Some(filename) = dev.filename.clone() else {
            return Vec::new();
        };
        let devno = ulsysfs::makedev(dev.maj, dev.min);
        if self.mtab.is_none() {
            let mut tb = Table::new();
            let path = sysroot.map(|root| [root, &b"/proc/self/mountinfo"[..]].concat());
            let mut cb = parser_errcb(short);
            // As with the swaps: the result is not looked at upstream.
            let _ = tab_parse::parse_mtab(&mut tb, path.as_deref(), Some(&mut cb));
            self.mtab = Some(tb);
        }
        let mut found: Vec<FsRef> = Vec::new();
        if let Some(mtab) = self.mtab.as_ref() {
            for i in mtab.order(Direction::Backward) {
                let Some(fs) = mtab.ents.get(i) else {
                    continue;
                };
                if fs.devno != devno && !fs.streq_srcpath(Some(&filename)) {
                    continue;
                }
                found.push(FsRef::Mtab(i));
            }
        }
        let mut is_swap = false;
        if found.is_empty() {
            let fs = match self.get_active_swap(&filename, sysroot, short) {
                Some(i) => Some(FsRef::Swaps(i)),
                None => {
                    // Upstream marks the device as swap here -- where the
                    // area was *not* found among the swaps -- and not there.
                    let cache = self.cache.get_or_insert_with(Cache::new);
                    let i = self
                        .mtab
                        .as_ref()
                        .and_then(|t| t.find_srcpath(&filename, Direction::Backward, Some(cache)));
                    is_swap = i.is_some();
                    i.map(FsRef::Mtab)
                }
            };
            found.extend(fs);
        }
        let dev = tr.dev_mut(id);
        dev.is_swap = is_swap;
        dev.is_mounted = !found.is_empty();
        dev.fss.clone_from(&found);
        found
    }

    /// `lsblk_device_get_mountpoint(dev)`: where the device is mounted --
    /// the last mount, unless that is of a subdirectory and another mount
    /// is of the filesystem's root -- or `[SWAP]`.
    pub(crate) fn get_mountpoint(
        &mut self,
        tr: &mut Devtree,
        id: DevId,
        sysroot: Option<&[u8]>,
        short: &[u8],
    ) -> Option<Vec<u8>> {
        let fss = self.get_filesystems(tr, id, sysroot, short);
        let first = *fss.first()?;
        let mut fs = self.fs(first)?;
        if fs.root.as_deref().is_some_and(|r| r != b"/") {
            for &r in fss.get(1..).unwrap_or_default() {
                let Some(other) = self.fs(r) else {
                    continue;
                };
                if other.root.as_deref().is_none_or(|root| root == b"/") {
                    fs = other;
                    break;
                }
            }
        }
        if fs.is_swaparea() {
            return Some(b"[SWAP]".to_vec());
        }
        fs.target.clone()
    }
}
