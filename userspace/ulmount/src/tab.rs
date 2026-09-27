//! libmount's `tab.c`: a mount table -- the filesystems of one file, in
//! file order -- walked as a list or, for the kernel's mountinfo, as the
//! tree its parent IDs make, and searched by mount point, source or tag.
//!
//! Entries are addressed by index. Upstream's iterator is a list cursor;
//! here a direction and a position do the same work.

use crate::cache::{self, Cache};
use crate::fs::Fs;
use crate::utils;

/// `MNT_FMT_*`: which kind of file a table was read from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Fmt {
    #[default]
    Guess,
    Fstab,
    Mtab,
    Mountinfo,
    Utab,
    Swaps,
}

/// `MNT_ITER_FORWARD` / `MNT_ITER_BACKWARD`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Direction {
    #[default]
    Forward,
    Backward,
}

/// `struct libmnt_table`.
#[derive(Clone, Debug, Default)]
pub struct Table {
    pub fmt: Fmt,
    pub ents: Vec<Fs>,
}

/// `struct libmnt_iter`: a direction, and how many entries it has passed.
///
/// Upstream's iterator is a cursor into the table's list; a count does the
/// same job as long as the table is not changed while it is walked, which
/// no caller here does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Iter {
    pub direction: Direction,
    passed: usize,
}

impl Iter {
    /// `mnt_new_iter(direction)`.
    #[must_use]
    pub fn new(direction: Direction) -> Self {
        Iter {
            direction,
            passed: 0,
        }
    }

    /// `mnt_reset_iter(itr, direction)`: back to the start; `None` keeps
    /// the direction (upstream's `-1`).
    pub fn reset(&mut self, direction: Option<Direction>) {
        if let Some(d) = direction {
            self.direction = d;
        }
        self.passed = 0;
    }
}

impl Table {
    /// `mnt_new_table()`.
    #[must_use]
    pub fn new() -> Self {
        Table::default()
    }

    /// `mnt_table_get_nents(tb)`.
    #[must_use]
    pub fn nents(&self) -> usize {
        self.ents.len()
    }

    /// The entries' indices in `dir`'s order.
    #[must_use]
    pub fn order(&self, dir: Direction) -> Vec<usize> {
        let n = self.ents.len();
        match dir {
            Direction::Forward => (0..n).collect(),
            Direction::Backward => (0..n).rev().collect(),
        }
    }

    /// `mnt_table_next_fs(tb, itr, &fs)`: the next entry in `itr`'s
    /// direction.
    pub fn next_fs(&self, itr: &mut Iter) -> Option<usize> {
        let n = self.ents.len();
        if itr.passed >= n {
            return None;
        }
        let i = match itr.direction {
            Direction::Forward => itr.passed,
            Direction::Backward => n.saturating_sub(1).saturating_sub(itr.passed),
        };
        itr.passed = itr.passed.saturating_add(1);
        Some(i)
    }

    /// `mnt_table_find_next_fs(tb, itr, match_func, data, &fs)`: the next
    /// entry `matches` accepts.
    pub fn find_next_fs(
        &self,
        itr: &mut Iter,
        mut matches: impl FnMut(&Table, usize) -> bool,
    ) -> Option<usize> {
        while let Some(i) = self.next_fs(itr) {
            if matches(self, i) {
                return Some(i);
            }
        }
        None
    }

    /// `mnt_table_set_iter(tb, itr, fs)`: the next entry `itr` visits is
    /// entry `i`.
    pub fn set_iter(&self, itr: &mut Iter, i: usize) {
        itr.passed = match itr.direction {
            Direction::Forward => i,
            Direction::Backward => self.ents.len().saturating_sub(1).saturating_sub(i),
        };
    }

    /// `mnt_reset_table(tb)`: every entry removed.
    pub fn reset(&mut self) {
        self.ents.clear();
    }

    /// `mnt_table_find_pair(tb, source, target, direction)`: the entry
    /// whose source matches `source` and whose mount point matches
    /// `target`; neither may be empty.
    pub fn find_pair(
        &self,
        source: Option<&[u8]>,
        target: Option<&[u8]>,
        dir: Direction,
        mut cache: Option<&mut Cache>,
    ) -> Option<usize> {
        let (Some(source), Some(target)) = (source, target) else {
            return None;
        };
        if source.is_empty() || target.is_empty() {
            return None;
        }
        self.order(dir).into_iter().find(|&i| {
            self.ents.get(i).is_some_and(|fs| {
                fs_match_target(fs, target, cache.as_deref_mut())
                    && fs_match_source(fs, Some(source), cache.as_deref_mut())
            })
        })
    }

    /// `is_mountinfo(tb)`: the first entry came from the kernel and has a
    /// root -- the table is mountinfo, whose parent IDs make a tree.
    #[must_use]
    pub fn is_mountinfo(&self) -> bool {
        self.ents
            .first()
            .is_some_and(|fs| fs.is_kernel() && fs.root.is_some())
    }

    /// `get_parent_fs(tb, fs)`: the first entry whose ID is `fs`'s parent.
    fn parent_fs(&self, i: usize) -> Option<usize> {
        let parent = self.ents.get(i)?.parent;
        self.ents.iter().position(|x| x.id == parent)
    }

    /// `mnt_table_get_root_fs(tb, &root)`: the entry with the smallest
    /// parent ID, then up its parents while they are in the table.
    #[must_use]
    pub fn root_fs(&self) -> Option<usize> {
        if !self.is_mountinfo() {
            return None;
        }
        let mut root: Option<usize> = None;
        let mut root_id = 0i32;
        for (i, fs) in self.ents.iter().enumerate() {
            if root.is_none() || fs.parent < root_id {
                root = Some(i);
                root_id = fs.parent;
            }
        }
        let mut root = root?;
        loop {
            match self.parent_fs(root) {
                Some(x) if x != root => root = x,
                _ => break,
            }
        }
        Some(root)
    }

    /// `mnt_table_next_child_fs(tb, itr, parent, &chld)`: after `last` (the
    /// child returned before, if any), the next child of `parent` in mount
    /// order (by ID) -- or, backward, the one before it.
    #[must_use]
    pub fn next_child_fs(
        &self,
        parent: usize,
        last: Option<usize>,
        dir: Direction,
    ) -> Option<usize> {
        if !self.is_mountinfo() {
            return None;
        }
        let parent_id = self.ents.get(parent)?.id;
        let lastchld_id = last.and_then(|l| self.ents.get(l)).map_or(0, |f| f.id);
        let mut chfs: Option<usize> = None;
        let mut chld_id = 0i32;
        for i in self.order(dir) {
            let Some(fs) = self.ents.get(i) else {
                continue;
            };
            if fs.parent != parent_id {
                continue;
            }
            let id = fs.id;
            // A filesystem that is its own parent (early userspace's
            // rootfs) is not its own child.
            if id == parent_id {
                continue;
            }
            let better = match dir {
                Direction::Forward => {
                    (lastchld_id == 0 || id > lastchld_id) && (chfs.is_none() || id < chld_id)
                }
                Direction::Backward => {
                    (lastchld_id == 0 || id < lastchld_id) && (chfs.is_none() || id > chld_id)
                }
            };
            if better {
                chfs = Some(i);
                chld_id = id;
            }
        }
        chfs
    }

    /// `mnt_table_over_fs(tb, parent, &child)`: the entry mounted on top of
    /// `parent`, at its own mount point.
    #[must_use]
    pub fn over_fs(&self, parent: usize) -> Option<usize> {
        if !self.is_mountinfo() {
            return None;
        }
        let p = self.ents.get(parent)?;
        let (id, tgt) = (p.id, p.target.clone());
        self.ents
            .iter()
            .position(|fs| fs.parent == id && fs.streq_target(tgt.as_deref()))
    }

    /// `mnt_table_find_target(tb, path, direction)`: by mount point -- as
    /// given, made absolute, canonicalized, and against each entry's own
    /// mount point canonicalized.
    pub fn find_target(
        &self,
        path: &[u8],
        dir: Direction,
        cache: Option<&mut Cache>,
    ) -> Option<usize> {
        if path.is_empty() {
            return None;
        }
        if let Some(i) = self.order(dir).into_iter().find(|&i| {
            self.ents
                .get(i)
                .is_some_and(|fs| fs.streq_target(Some(path)))
        }) {
            return Some(i);
        }
        if path.first() != Some(&b'/')
            && let Some(cn) = cache::absolute_path(path)
            && let Some(i) = self.order(dir).into_iter().find(|&i| {
                self.ents
                    .get(i)
                    .is_some_and(|fs| fs.streq_target(Some(&cn)))
            })
        {
            return Some(i);
        }
        let cache = cache?;
        let cn = cache.resolve_path(path)?;
        if let Some(i) = self.order(dir).into_iter().find(|&i| {
            self.ents
                .get(i)
                .is_some_and(|fs| fs.streq_target(Some(&cn)))
        }) {
            return Some(i);
        }
        // Mount points written other than canonically (the kernel's are
        // already canonical).
        for i in self.order(dir) {
            let Some(fs) = self.ents.get(i) else {
                continue;
            };
            let Some(t) = &fs.target else {
                continue;
            };
            if fs.is_swaparea() || fs.is_kernel() || t.as_slice() == b"/" {
                continue;
            }
            if cache.resolve_target(t).is_some_and(|p| p == cn) {
                return Some(i);
            }
        }
        None
    }

    /// `mnt_table_find_mountpoint(tb, path, direction)`: the mount point
    /// `path` is on -- `path` itself if it is one, else its nearest parent
    /// that is, else `/`. `path` must exist.
    pub fn find_mountpoint(
        &self,
        path: &[u8],
        dir: Direction,
        mut cache: Option<&mut Cache>,
    ) -> Option<usize> {
        if path.is_empty() || std::fs::metadata(quoting::os_from_bytes(path)).is_err() {
            return None;
        }
        let mut mnt = path.to_vec();
        loop {
            if let Some(i) = self.find_target(&mnt, dir, cache.as_deref_mut()) {
                return Some(i);
            }
            // `stripoff_last_component`.
            let Some(slash) = mnt.iter().rposition(|&b| b == b'/') else {
                break;
            };
            mnt.truncate(slash);
            if mnt.len() <= 1 {
                break;
            }
        }
        self.find_target(b"/", dir, cache)
    }

    /// `mnt_table_find_srcpath(tb, path, direction)`: by source path --
    /// as given, canonicalized, through tags that evaluate to it, and
    /// against each entry's own source canonicalized.
    pub fn find_srcpath(
        &self,
        path: &[u8],
        dir: Direction,
        cache: Option<&mut Cache>,
    ) -> Option<usize> {
        if path.is_empty() {
            return None;
        }
        let mut ntags = 0usize;
        for i in self.order(dir) {
            let Some(fs) = self.ents.get(i) else {
                continue;
            };
            if fs.streq_srcpath(Some(path)) {
                return Some(i);
            }
            if fs.tag().is_some() {
                ntags = ntags.saturating_add(1);
            }
        }
        let cache = cache?;
        let cn = cache.resolve_path(path)?;
        let nents = self.nents();
        if ntags < nents
            && let Some(i) = self.order(dir).into_iter().find(|&i| {
                self.ents
                    .get(i)
                    .is_some_and(|fs| fs.streq_srcpath(Some(&cn)))
            })
        {
            return Some(i);
        }
        if ntags > 0 {
            match cache.read_tags(&cn) {
                Ok(_) => {
                    for i in self.order(dir) {
                        let Some((t, v)) = self.ents.get(i).and_then(Fs::tag) else {
                            continue;
                        };
                        if cache.device_has_tag(&cn, t, v) {
                            return Some(i);
                        }
                    }
                }
                Err(crate::blkid::ProbeFail::Open(13)) => {
                    // EACCES: the device cannot be read; evaluate every tag
                    // through udev's links instead.
                    for i in self.order(dir) {
                        let Some((t, v)) = self.ents.get(i).and_then(Fs::tag) else {
                            continue;
                        };
                        let (t, v) = (t.to_vec(), v.to_vec());
                        if cache.resolve_tag(&t, &v).is_some_and(|x| x == cn) {
                            return Some(i);
                        }
                    }
                }
                Err(_) => {}
            }
        }
        if ntags <= nents {
            for i in self.order(dir) {
                let Some(fs) = self.ents.get(i) else {
                    continue;
                };
                if fs.is_netfs() || fs.is_pseudofs() {
                    continue;
                }
                let Some(src) = fs.srcpath().map(<[u8]>::to_vec) else {
                    continue;
                };
                if cache.resolve_path(&src).is_some_and(|p| p == cn) {
                    return Some(i);
                }
            }
        }
        None
    }

    /// `mnt_table_find_tag(tb, tag, val, direction)`: an entry with the tag,
    /// else one whose source is the device the tag names.
    pub fn find_tag(
        &self,
        tag: &[u8],
        val: &[u8],
        dir: Direction,
        cache: Option<&mut Cache>,
    ) -> Option<usize> {
        if tag.is_empty() {
            return None;
        }
        if let Some(i) = self.order(dir).into_iter().find(|&i| {
            self.ents.get(i).is_some_and(|fs| {
                fs.tagname.as_deref() == Some(tag) && fs.tagval.as_deref() == Some(val)
            })
        }) {
            return Some(i);
        }
        let cache = cache?;
        let cn = cache.resolve_tag(tag, val)?;
        self.find_srcpath(&cn, dir, Some(cache))
    }

    /// `mnt_table_find_source(tb, source, direction)`: by tag if `source`
    /// is one, else by source path.
    pub fn find_source(
        &self,
        source: &[u8],
        dir: Direction,
        cache: Option<&mut Cache>,
    ) -> Option<usize> {
        match utils::parse_tag_string(source) {
            Some((t, v)) if utils::valid_tagname(&t) => self.find_tag(&t, &v, dir, cache),
            _ => self.find_srcpath(source, dir, cache),
        }
    }

    /// `mnt_table_uniq_fs(tb, flags, cmp)`: each entry for which `same`
    /// says an entry before it (in `dir`'s order) is the same goes; with
    /// `keeptree`, its children are given its parent.
    pub fn uniq_fs(
        &mut self,
        dir: Direction,
        keeptree: bool,
        mut same: impl FnMut(&Fs, &Fs) -> bool,
    ) {
        if self.ents.is_empty() {
            return;
        }
        let keeptree = keeptree && self.is_mountinfo();
        // `i` walks in `dir`'s order; the entries already walked (and kept)
        // are the ones compared against.
        let mut i = match dir {
            Direction::Forward => 0,
            Direction::Backward => self.ents.len(),
        };
        loop {
            match dir {
                Direction::Forward if i >= self.ents.len() => break,
                Direction::Backward if i == 0 => break,
                Direction::Backward => i = i.saturating_sub(1),
                Direction::Forward => {}
            }
            let before: Vec<usize> = match dir {
                Direction::Forward => (0..i).collect(),
                Direction::Backward => ((i.saturating_add(1))..self.ents.len()).rev().collect(),
            };
            let dup = before.iter().any(|&x| {
                matches!((self.ents.get(x), self.ents.get(i)), (Some(a), Some(b)) if same(a, b))
            });
            if dup {
                if keeptree && let Some(fs) = self.ents.get(i) {
                    let (old, new) = (fs.id, fs.parent);
                    for e in &mut self.ents {
                        if e.parent == old {
                            e.parent = new;
                        }
                    }
                }
                self.ents.remove(i);
                // Forward, the next entry has moved into `i`.
                continue;
            }
            if dir == Direction::Forward {
                i = i.saturating_add(1);
            }
        }
    }
}

/// `mnt_fs_match_target(fs, target, cache)`: the mount point is `target`
/// -- as given, canonicalized, or (for an entry not from the kernel) with
/// both canonicalized.
pub fn fs_match_target(fs: &Fs, target: &[u8], cache: Option<&mut Cache>) -> bool {
    if fs.target.is_none() {
        return false;
    }
    if fs.streq_target(Some(target)) {
        return true;
    }
    let Some(cache) = cache else {
        return false;
    };
    let cn = cache.resolve_target(target);
    if cn.as_deref().is_some_and(|c| fs.streq_target(Some(c))) {
        return true;
    }
    if let Some(cn) = cn
        && !fs.is_kernel()
        && !fs.is_swaparea()
        && let Some(t) = &fs.target
    {
        return cache.resolve_target(t).is_some_and(|tcn| tcn == cn);
    }
    false
}

/// `mnt_fs_match_source(fs, source, cache)`: the source is `source` -- as
/// given, as a tag written the same, canonicalized, or as the device a tag
/// of the entry's names.
pub fn fs_match_source(fs: &Fs, source: Option<&[u8]>, cache: Option<&mut Cache>) -> bool {
    if fs.streq_srcpath(source) {
        return true;
    }
    let (Some(source), Some(fs_source)) = (source, fs.source.as_deref()) else {
        return false;
    };
    if fs.tagname.is_some() && source == fs_source {
        return true;
    }
    let Some(cache) = cache else {
        return false;
    };
    if fs.is_netfs() || fs.is_pseudofs() {
        return false;
    }
    let Some(cn) = cache.resolve_spec(source) else {
        return false;
    };
    let src = fs.srcpath();
    if src.is_some() && fs.streq_srcpath(Some(&cn)) {
        return true;
    }
    if let Some(src) = src {
        return cache.resolve_path(src).is_some_and(|s| s == cn);
    }
    let Some((t, v)) = fs.tag() else {
        return false;
    };
    match cache.read_tags(&cn) {
        Ok(_) => cache.device_has_tag(&cn, t, v),
        Err(crate::blkid::ProbeFail::Open(13)) => {
            let (t, v) = (t.to_vec(), v.to_vec());
            cache.resolve_tag(&t, &v).is_some_and(|x| x == cn)
        }
        Err(_) => false,
    }
}

#[cfg(test)]
#[allow(
    clippy::indexing_slicing,
    clippy::unwrap_used,
    reason = "tests index what they built"
)]
mod tests {
    use super::*;
    use crate::fs::MNT_FS_KERNEL;

    fn fs(id: i32, parent: i32, target: &str) -> Fs {
        Fs {
            id,
            parent,
            target: Some(target.as_bytes().to_vec()),
            root: Some(b"/".to_vec()),
            flags: MNT_FS_KERNEL,
            ..Fs::default()
        }
    }

    fn table() -> Table {
        Table {
            fmt: Fmt::Mountinfo,
            ents: vec![
                fs(21, 1, "/"),
                fs(30, 21, "/proc"),
                fs(25, 21, "/sys"),
                fs(40, 25, "/sys/fs"),
                fs(41, 21, "/sys"),
            ],
        }
    }

    #[test]
    fn the_tree_is_walked_in_mount_order() {
        let tb = table();
        assert_eq!(tb.root_fs(), Some(0));
        let mut children = Vec::new();
        let mut last = None;
        while let Some(c) = tb.next_child_fs(0, last, Direction::Forward) {
            children.push(tb.ents[c].id);
            last = Some(c);
        }
        assert_eq!(children, vec![25, 30, 41]);
        let mut last = None;
        let mut back = Vec::new();
        while let Some(c) = tb.next_child_fs(0, last, Direction::Backward) {
            back.push(tb.ents[c].id);
            last = Some(c);
        }
        assert_eq!(back, vec![41, 30, 25]);
        // /sys (25) is over-mounted by 41? No: 41's parent is 21.
        assert_eq!(tb.over_fs(2), None);
    }

    #[test]
    fn targets_are_found_as_paths() {
        let tb = table();
        assert_eq!(tb.find_target(b"/sys/", Direction::Forward, None), Some(2));
        assert_eq!(tb.find_target(b"/sys", Direction::Backward, None), Some(4));
        assert_eq!(tb.find_target(b"/nope", Direction::Forward, None), None);
    }

    #[test]
    fn duplicates_go_and_their_children_move_up() {
        let mut tb = table();
        tb.uniq_fs(Direction::Backward, true, |a, b| {
            a.streq_target(b.target.as_deref())
        });
        let ids: Vec<i32> = tb.ents.iter().map(|f| f.id).collect();
        // Backward: 41 (/sys) is seen first, so 25 is the duplicate; its
        // child 40 is given 25's parent.
        assert_eq!(ids, vec![21, 30, 40, 41]);
        assert_eq!(tb.ents[2].parent, 21);
    }
}
