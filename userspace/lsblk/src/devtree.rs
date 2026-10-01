//! `lsblk-devtree.c`: the devices found, and how they depend on each other.
//!
//! A device can have several parents and several children, and is made
//! once and shared: a dependence is an object of its own, listed in its
//! parent's children and in its child's parents. The tree keeps every
//! device in one list (`devices`, which holds the reference that keeps a
//! device alive) and the roots of the output in another (`roots`, which
//! holds none).
//!
//! Here the devices and the dependences live in arenas and are named by
//! index; the lists are vectors of indices in upstream's list order. A
//! device's reference count is kept, as upstream's, because it decides
//! when a device removed from the tree also loses its dependences (a
//! partition's reference to its disk keeps the disk's). Nothing is ever
//! deallocated -- a freed device is only marked so -- which no reader can
//! tell apart from upstream's, since nothing reaches a freed device.

use std::rc::Rc;

use ulsysfs::PathCxt;

use crate::props::DevProp;

/// A device, by its place in the arena. Only [`Devtree::new_device`] makes
/// one, so every id indexes a device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DevId(usize);

/// A dependence, by its place in the arena.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DepId(usize);

/// `struct lsblk_devdep`: `parent` holds `child`.
#[derive(Clone, Copy, Debug)]
struct Dep {
    parent: DevId,
    child: DevId,
}

/// Where a device's filesystem entry is: `lsblk_device_get_filesystems`
/// points into the mount table or the swaps table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FsRef {
    Mtab(usize),
    Swaps(usize),
}

/// `struct lsblk_device`.
#[derive(Debug, Default)]
pub(crate) struct Device {
    refcount: usize,
    /// Freed: its last reference is gone.
    freed: bool,
    /// `childs`: the dependences this device is the parent of, in order.
    childs: Vec<DepId>,
    /// `parents`: those it is the child of, in order.
    parents: Vec<DepId>,
    /// `wholedisk`: for a partition.
    pub(crate) wholedisk: Option<DevId>,
    /// `scols_line`: its line in the output, once it has one.
    pub(crate) scols_line: Option<smartcols::LineId>,
    pub(crate) properties: Option<DevProp>,
    /// `st`: kept once `st_rdev` is known.
    pub(crate) st: Option<crate::sys::Stat>,
    /// `name`: the kernel's name, as in `/sys/block`.
    pub(crate) name: Vec<u8>,
    pub(crate) dm_name: Option<Vec<u8>>,
    /// `filename`: the device node.
    pub(crate) filename: Option<Vec<u8>>,
    pub(crate) dedupkey: Option<Vec<u8>>,
    pub(crate) sysfs: Option<Rc<PathCxt>>,
    /// `fss`: the filesystems on it, the last mounted first.
    pub(crate) fss: Vec<FsRef>,
    /// `fsstat`: kept once it says the filesystem has blocks.
    pub(crate) fsstat: crate::sys::Statvfs,
    pub(crate) npartitions: usize,
    pub(crate) nholders: usize,
    pub(crate) nslaves: usize,
    pub(crate) maj: u32,
    pub(crate) min: u32,
    /// `discard_granularity`, `u64::MAX` until read.
    pub(crate) discard_granularity: u64,
    pub(crate) size: u64,
    /// `removable`: -1 until known.
    pub(crate) removable: i32,
    pub(crate) is_mounted: bool,
    pub(crate) is_swap: bool,
    pub(crate) is_printed: bool,
    pub(crate) udev_requested: bool,
    pub(crate) blkid_requested: bool,
    pub(crate) file_requested: bool,
}

impl Device {
    /// `device_is_partition`.
    pub(crate) fn is_partition(&self) -> bool {
        self.wholedisk.is_some()
    }
}

/// `struct lsblk_devtree`.
#[derive(Debug, Default)]
pub(crate) struct Devtree {
    devs: Vec<Device>,
    deps: Vec<Dep>,
    /// `roots`, in order.
    roots: Vec<DevId>,
    /// `devices`, in order.
    devices: Vec<DevId>,
    /// `pktcdvd_map`: `(slave, holder)` from `/sys/class/pktcdvd/device_map`,
    /// once read.
    pktcdvd_map: Option<Vec<(u64, u64)>>,
}

impl Devtree {
    /// The device `id` names. Ids are made only by `new_device`, which
    /// pushes the device, and the arena never shrinks.
    pub(crate) fn dev(&self, id: DevId) -> &Device {
        self.devs
            .get(id.0)
            .unwrap_or_else(|| unreachable!("device ids index the arena"))
    }

    /// As [`Devtree::dev`], to change.
    pub(crate) fn dev_mut(&mut self, id: DevId) -> &mut Device {
        self.devs
            .get_mut(id.0)
            .unwrap_or_else(|| unreachable!("device ids index the arena"))
    }

    /// `lsblk_new_device`: one reference, the caller's.
    pub(crate) fn new_device(&mut self) -> DevId {
        let id = DevId(self.devs.len());
        self.devs.push(Device {
            refcount: 1,
            removable: -1,
            discard_granularity: u64::MAX,
            ..Device::default()
        });
        id
    }

    /// `lsblk_ref_device`.
    pub(crate) fn ref_device(&mut self, id: DevId) {
        let d = self.dev_mut(id);
        d.refcount = d.refcount.saturating_add(1);
    }

    /// `lsblk_unref_device`: at the last reference, the device's
    /// dependences go, and its reference to its disk.
    pub(crate) fn unref_device(&mut self, id: DevId) {
        let d = self.dev_mut(id);
        d.refcount = d.refcount.saturating_sub(1);
        if d.refcount > 0 || d.freed {
            return;
        }
        d.freed = true;
        self.device_remove_dependences(id);
        if let Some(disk) = self.dev_mut(id).wholedisk {
            self.unref_device(disk);
        }
    }

    /// `remove_dependence`: out of its parent's children and its child's
    /// parents.
    fn remove_dependence(&mut self, dep: DepId) {
        let Some(&Dep { parent, child }) = self.deps.get(dep.0) else {
            return;
        };
        self.dev_mut(parent).childs.retain(|&d| d != dep);
        self.dev_mut(child).parents.retain(|&d| d != dep);
    }

    /// `device_remove_dependences`.
    fn device_remove_dependences(&mut self, id: DevId) {
        while let Some(&dep) = self.dev(id).childs.first() {
            self.remove_dependence(dep);
        }
        while let Some(&dep) = self.dev(id).parents.first() {
            self.remove_dependence(dep);
        }
    }

    /// The children of `id`, in order (`lsblk_device_next_child`).
    pub(crate) fn children(&self, id: DevId) -> Vec<DevId> {
        self.dev(id)
            .childs
            .iter()
            .filter_map(|d| self.deps.get(d.0).map(|dep| dep.child))
            .collect()
    }

    /// The parents of `id`, in order (`lsblk_device_next_parent`).
    pub(crate) fn parents(&self, id: DevId) -> Vec<DevId> {
        self.dev(id)
            .parents
            .iter()
            .filter_map(|d| self.deps.get(d.0).map(|dep| dep.parent))
            .collect()
    }

    /// `lsblk_device_has_child`.
    fn has_child(&self, id: DevId, child: DevId) -> bool {
        self.children(id).contains(&child)
    }

    /// `lsblk_device_new_dependence(parent, child)`: `false` when the
    /// dependence was there already (upstream's 1).
    pub(crate) fn new_dependence(&mut self, parent: DevId, child: DevId) -> bool {
        if self.has_child(parent, child) {
            return false;
        }
        let dep = DepId(self.deps.len());
        self.deps.push(Dep { parent, child });
        self.dev_mut(parent).childs.push(dep);
        self.dev_mut(child).parents.push(dep);
        true
    }

    /// `lsblk_device_is_last_parent(dev, parent)`.
    pub(crate) fn is_last_parent(&self, id: DevId, parent: Option<DevId>) -> bool {
        self.parents(id).last().copied() == parent && parent.is_some()
    }

    /// The roots, in order (`lsblk_devtree_next_root`).
    pub(crate) fn roots(&self) -> Vec<DevId> {
        self.roots.clone()
    }

    /// `lsblk_devtree_add_root`: at the end of the roots, once; and among
    /// the devices, if it is not.
    pub(crate) fn add_root(&mut self, id: DevId) {
        if self.roots.contains(&id) {
            return;
        }
        if !self.devices.contains(&id) {
            self.add_device(id);
        }
        self.roots.push(id);
    }

    /// `lsblk_devtree_remove_root`.
    pub(crate) fn remove_root(&mut self, id: DevId) {
        self.roots.retain(|&r| r != id);
    }

    /// `lsblk_devtree_add_device`: at the end of the devices, which take a
    /// reference.
    pub(crate) fn add_device(&mut self, id: DevId) {
        self.ref_device(id);
        self.devices.push(id);
    }

    /// `lsblk_devtree_get_device(tr, name)`: the first device so named.
    pub(crate) fn get_device(&self, name: &[u8]) -> Option<DevId> {
        self.devices
            .iter()
            .copied()
            .find(|&d| self.dev(d).name == name)
    }

    /// `lsblk_devtree_remove_device`: out of both lists, and its reference
    /// dropped.
    pub(crate) fn remove_device(&mut self, id: DevId) {
        if !self.devices.contains(&id) {
            return;
        }
        self.roots.retain(|&r| r != id);
        self.devices.retain(|&d| d != id);
        self.unref_device(id);
    }

    /// `lsblk_devtree_pktcdvd_get_mate(tr, devno, is_slave)`: the other end
    /// of a packet-writing device mapping -- the `pktcdvd` device for its
    /// block device (`is_slave`), or the block device for it. 0 for none.
    pub(crate) fn pktcdvd_get_mate(&mut self, devno: u64, is_slave: bool) -> u64 {
        let map = self.pktcdvd_map.get_or_insert_with(read_pktcdvd_map);
        for &(slave, holder) in map.iter() {
            if is_slave && devno == slave {
                return holder;
            }
            if !is_slave && devno == holder {
                return slave;
            }
        }
        0
    }

    /// `device_dedupkey_is_equal(dev, pattern)`.
    fn dedupkey_is_equal(&self, id: DevId, pattern: DevId) -> bool {
        let dev = self.dev(id);
        let Some(key) = dev.dedupkey.as_deref() else {
            return false;
        };
        if id == pattern || Some(key) != self.dev(pattern).dedupkey.as_deref() {
            return false;
        }
        match dev.wholedisk {
            None => true,
            Some(disk) => self.dev(disk).dedupkey.as_deref() != Some(key),
        }
    }

    /// `device_dedup_dependencies(dev, pattern)`: a child that duplicates
    /// the pattern loses its dependence; any other is searched in turn.
    fn dedup_dependencies(&mut self, id: DevId, pattern: DevId) {
        for dep in self.dev(id).childs.clone() {
            // Already removed by a deeper call.
            if !self.dev(id).childs.contains(&dep) {
                continue;
            }
            let Some(child) = self.deps.get(dep.0).map(|d| d.child) else {
                continue;
            };
            if self.dedupkey_is_equal(child, pattern) {
                self.remove_dependence(dep);
            } else {
                self.dedup_dependencies(child, pattern);
            }
        }
    }

    /// `devtree_dedup(tr, pattern)`: a root that duplicates the pattern is
    /// no root any more; the others' trees are searched.
    fn dedup(&mut self, pattern: DevId) {
        for root in self.roots.clone() {
            if !self.roots.contains(&root) {
                continue;
            }
            if self.dedupkey_is_equal(root, pattern) {
                self.roots.retain(|&r| r != root);
            } else {
                self.dedup_dependencies(root, pattern);
            }
        }
    }

    /// `lsblk_devtree_deduplicate_devices`: the devices sorted by number,
    /// and for each key the first device holding it kept and the rest cut
    /// from the tree -- a partition sharing its disk's key excepted.
    pub(crate) fn deduplicate_devices(&mut self) {
        let mut devices = std::mem::take(&mut self.devices);
        // `list_sort` by `makedev(maj, min)`: stable, as Rust's sort is.
        devices.sort_by_key(|&d| ulsysfs::makedev(self.dev(d).maj, self.dev(d).min));
        self.devices = devices;
        let mut last: Option<Vec<u8>> = None;
        for pattern in self.devices.clone() {
            let dev = self.dev(pattern);
            let Some(key) = dev.dedupkey.clone() else {
                continue;
            };
            if let Some(disk) = dev.wholedisk
                && self.dev(disk).dedupkey.as_deref() == Some(key.as_slice())
            {
                continue;
            }
            if last.as_deref() == Some(key.as_slice()) {
                continue;
            }
            self.dedup(pattern);
            last = Some(key);
        }
    }
}

/// `read_pktcdvd_map`: `/sys/class/pktcdvd/device_map`, each line's
/// `NAME PKT_MAJ:PKT_MIN BLK_MAJ:BLK_MIN` as `(block device, pktcdvd
/// device)` -- live, never under `--sysroot`, as upstream reads it.
fn read_pktcdvd_map() -> Vec<(u64, u64)> {
    let mut map = Vec::new();
    let Ok(text) = std::fs::read("/sys/class/pktcdvd/device_map") else {
        return map;
    };
    // `fgets` into PATH_MAX bytes, then `sscanf("%*s %d:%d %d:%d\n")`.
    for line in text.split_inclusive(|&b| b == b'\n') {
        if let Some((pkt, blk)) = scan_devnomap(line) {
            map.push((blk, pkt));
        }
    }
    map
}

/// `sscanf(line, "%*s %d:%d %d:%d")`: a word, then two device numbers.
fn scan_devnomap(line: &[u8]) -> Option<(u64, u64)> {
    let mut pos = 0usize;
    // `%*s`: white space skipped, then a word.
    while line.get(pos).is_some_and(|&b| ulstrutils::c_isspace(b)) {
        pos = pos.saturating_add(1);
    }
    let start = pos;
    while line.get(pos).is_some_and(|&b| !ulstrutils::c_isspace(b)) {
        pos = pos.saturating_add(1);
    }
    if pos == start {
        return None;
    }
    let pkt = scan_majmin(line, &mut pos)?;
    let blk = scan_majmin(line, &mut pos)?;
    Some((pkt, blk))
}

/// `%d:%d`, white space allowed before each number.
fn scan_majmin(text: &[u8], pos: &mut usize) -> Option<u64> {
    let maj = scan_int(text, pos)?;
    if text.get(*pos) != Some(&b':') {
        return None;
    }
    *pos = pos.saturating_add(1);
    let min = scan_int(text, pos)?;
    #[allow(
        clippy::cast_sign_loss,
        reason = "makedev takes the ints as unsigned, as C converts them"
    )]
    Some(ulsysfs::makedev(maj as u32, min as u32))
}

/// `%d`: `strtol`'s value cut to an `int`.
fn scan_int(text: &[u8], pos: &mut usize) -> Option<i32> {
    let sc = ulstrutils::scan_integer(text.get(*pos..)?, 10)?;
    *pos = pos.saturating_add(sc.end);
    let v: i128 = if sc.saturated {
        if sc.negative {
            i128::from(i64::MIN)
        } else {
            i128::from(i64::MAX)
        }
    } else {
        let m = i128::try_from(sc.magnitude).unwrap_or(i128::MAX);
        let v = if sc.negative { m.saturating_neg() } else { m };
        v.clamp(i128::from(i64::MIN), i128::from(i64::MAX))
    };
    #[allow(
        clippy::cast_possible_truncation,
        reason = "C stores the long in an int"
    )]
    Some(v as i64 as i32)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "tests unwrap what they built")]
mod tests {
    use super::*;

    fn named(tr: &mut Devtree, name: &[u8]) -> DevId {
        let id = tr.new_device();
        tr.dev_mut(id).name = name.to_vec();
        tr.add_device(id);
        tr.unref_device(id);
        id
    }

    #[test]
    fn a_device_shared_by_two_parents_is_one_device() {
        let mut tr = Devtree::default();
        let a = named(&mut tr, b"sda1");
        let b = named(&mut tr, b"sdb1");
        let md = named(&mut tr, b"md0");
        assert!(tr.new_dependence(a, md));
        assert!(tr.new_dependence(b, md));
        assert!(!tr.new_dependence(b, md));
        assert_eq!(tr.parents(md), vec![a, b]);
        assert!(tr.is_last_parent(md, Some(b)));
        assert!(!tr.is_last_parent(md, Some(a)));
        assert_eq!(tr.get_device(b"md0"), Some(md));
    }

    #[test]
    fn removing_a_device_drops_its_dependences() {
        let mut tr = Devtree::default();
        let disk = named(&mut tr, b"sda");
        let md = named(&mut tr, b"md0");
        tr.add_root(disk);
        tr.new_dependence(disk, md);
        tr.remove_device(disk);
        assert!(tr.roots().is_empty());
        assert!(tr.parents(md).is_empty());
        assert_eq!(tr.get_device(b"sda"), None);
    }

    #[test]
    fn a_duplicate_by_key_is_cut_from_the_tree() {
        let mut tr = Devtree::default();
        let a = named(&mut tr, b"sda");
        let b = named(&mut tr, b"sdb");
        tr.dev_mut(a).maj = 8;
        tr.dev_mut(b).maj = 8;
        tr.dev_mut(b).min = 16;
        tr.dev_mut(a).dedupkey = Some(b"WWN".to_vec());
        tr.dev_mut(b).dedupkey = Some(b"WWN".to_vec());
        tr.add_root(b);
        tr.add_root(a);
        tr.deduplicate_devices();
        // The lower-numbered device stays.
        assert_eq!(tr.roots(), vec![a]);
    }

    #[test]
    fn pktcdvd_map_lines_scan_as_sscanf() {
        assert_eq!(
            scan_devnomap(b"pktcdvd0 254:0 11:0\n"),
            Some((ulsysfs::makedev(254, 0), ulsysfs::makedev(11, 0)))
        );
        assert_eq!(scan_devnomap(b"pktcdvd0 254:0\n"), None);
        assert_eq!(scan_devnomap(b"\n"), None);
    }
}
