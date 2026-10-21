//! `table.c`: a list of partitions, independent of any label.
//!
//! A [`Table`] owns its partitions, in order. Upstream's tables share
//! reference-counted partitions; here a table holds its own copies, and a
//! position in the table stands for C's pointer to an entry.

use crate::context::Context;
use crate::errno::{EINVAL, ENOSYS};
use crate::partition::Partition;
use crate::utils::div;
use crate::{ALIGN_UP, Sector, diff};

/// `struct fdisk_table`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Table {
    parts: Vec<Partition>,
}

impl Table {
    /// `fdisk_new_table ()`.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `fdisk_reset_table`.
    pub fn reset(&mut self) {
        self.parts.clear();
    }

    /// `fdisk_table_is_empty`.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.parts.is_empty()
    }

    /// `fdisk_table_get_nents`.
    #[must_use]
    pub fn get_nents(&self) -> usize {
        self.parts.len()
    }

    /// The entries in order (`fdisk_table_next_partition` forward).
    pub fn iter(&self) -> std::slice::Iter<'_, Partition> {
        self.parts.iter()
    }

    /// The entries in order, for changing.
    pub fn iter_mut(&mut self) -> std::slice::IterMut<'_, Partition> {
        self.parts.iter_mut()
    }

    /// `fdisk_table_get_partition (tb, n)`: the `n`th entry.
    #[must_use]
    pub fn get_partition(&self, n: usize) -> Option<&Partition> {
        self.parts.get(n)
    }

    /// The `n`th entry, for changing.
    pub fn get_partition_mut(&mut self, n: usize) -> Option<&mut Partition> {
        self.parts.get_mut(n)
    }

    /// `fdisk_table_get_partition_by_partno (tb, partno)`: the position of
    /// the first entry numbered `partno`.
    #[must_use]
    pub fn get_partition_by_partno(&self, partno: usize) -> Option<usize> {
        self.parts.iter().position(|p| p.partno == partno)
    }

    /// `fdisk_table_add_partition (tb, pa)`: at the end.
    pub fn add_partition(&mut self, pa: Partition) {
        self.parts.push(pa);
    }

    /// `table_insert_partition (tb, poz, pa)`: after entry `poz`, or first.
    fn insert_partition(&mut self, poz: Option<usize>, pa: Partition) {
        let at = poz.map_or(0, |p| p.saturating_add(1)).min(self.parts.len());
        self.parts.insert(at, pa);
    }

    /// `fdisk_table_remove_partition (tb, pa)`: the entry at `n`.
    pub fn remove_partition(&mut self, n: usize) -> Option<Partition> {
        (n < self.parts.len()).then(|| self.parts.remove(n))
    }

    /// `fdisk_table_sort_partitions (tb, cmp)`: a stable sort, as the
    /// kernel's `list_sort` is, by a C-style comparison.
    pub fn sort_partitions(&mut self, cmp: fn(&Partition, &Partition) -> i32) {
        self.parts.sort_by(|a, b| cmp(a, b).cmp(&0));
    }

    /// `fdisk_table_wrong_order (tb)`: whether the starts are out of order
    /// (whole-disk entries aside).
    #[must_use]
    pub fn wrong_order(&self) -> bool {
        let mut last = 0;
        for pa in &self.parts {
            if !pa.has_start() || pa.is_wholedisk() {
                continue;
            }
            if pa.start < last {
                return true;
            }
            last = pa.start;
        }
        false
    }
}

impl<'a> IntoIterator for &'a Table {
    type Item = &'a Partition;
    type IntoIter = std::slice::Iter<'a, Partition>;
    fn into_iter(self) -> Self::IntoIter {
        self.parts.iter()
    }
}

impl<'a> IntoIterator for &'a mut Table {
    type Item = &'a mut Partition;
    type IntoIter = std::slice::IterMut<'a, Partition>;
    fn into_iter(self) -> Self::IntoIter {
        self.parts.iter_mut()
    }
}

/// `fdisk_diff_tables (a, b, itr, &res, &change)` as an iteration: the
/// changes from `a` (the old table) to `b` (the new), in upstream's order
/// -- every numbered entry of `a` (removed, moved, resized or unchanged),
/// then the entries only `b` has (added) -- each with the entry it
/// concerns.
#[must_use]
pub fn diff_tables(a: Option<&Table>, b: Option<&Table>) -> Vec<(i32, Partition)> {
    let mut out = Vec::new();
    if let Some(a) = a {
        for pa in a.iter().filter(|p| p.has_partno()) {
            let pb = b.and_then(|b| {
                b.get_partition_by_partno(pa.partno)
                    .and_then(|i| b.get_partition(i))
            });
            out.push(match pb {
                None => (diff::REMOVED, pa.clone()),
                Some(pb) if pb.start != pa.start => (diff::MOVED, pb.clone()),
                Some(pb) if pb.size != pa.size => (diff::RESIZED, pb.clone()),
                Some(_) => (diff::UNCHANGED, pa.clone()),
            });
        }
    }
    if let Some(b) = b {
        for pb in b.iter().filter(|p| p.has_partno()) {
            if a.is_none_or(|a| a.get_partition_by_partno(pb.partno).is_none()) {
                out.push((diff::ADDED, pb.clone()));
            }
        }
    }
    out
}

impl Context {
    /// `fdisk_get_partitions (cxt, &tb)`: every used partition of the
    /// label, added to `tb`.
    pub fn get_partitions(&mut self, tb: &mut Table) -> i32 {
        let Some(lb) = self.label_ref() else {
            return -EINVAL;
        };
        if lb.op.get_part.is_none() {
            return -ENOSYS;
        }
        let nmax = lb.nparts_max;
        for i in 0..nmax {
            if let Ok(pa) = self.get_partition(i) {
                if pa.is_used() {
                    tb.add_partition(pa);
                }
            }
        }
        0
    }

    /// `new_freespace (cxt, start, end, parent, &pa)`: an entry for the
    /// free space from `start` to `end`, aligned; `None` when it is empty.
    fn new_freespace(
        &mut self,
        start: Sector,
        end: Sector,
        parent: Option<&Partition>,
    ) -> Option<Partition> {
        if start == end {
            return None;
        }
        let aligned_start = self.align_lba_in_range(start, start, end);
        let size = end.wrapping_sub(aligned_start).wrapping_add(1);
        if size == 0 {
            return None;
        }
        let mut pa = Partition::new();
        pa.freespace = true;
        pa.start = aligned_start;
        pa.size = size;
        if let Some(parent) = parent {
            pa.parent_partno = parent.partno;
        }
        Some(pa)
    }

    /// `table_add_freespace (cxt, tb, start, end, parent)`: a free-space
    /// entry, put after the entry that ends nearest before it (or after its
    /// container).
    fn table_add_freespace(
        &mut self,
        tb: &mut Table,
        start: Sector,
        end: Sector,
        parent: Option<&Partition>,
    ) {
        let Some(pa) = self.new_freespace(start, end, parent) else {
            return;
        };
        let mut from = 0;
        let mut real_parent = None;
        if let Some(parent) = parent.filter(|p| p.has_partno()) {
            match tb
                .iter()
                .position(|x| x.has_partno() && x.partno == parent.partno)
            {
                Some(i) => {
                    real_parent = Some(i);
                    // The search for the best goes on from after the parent.
                    from = i.saturating_add(1);
                }
                None => from = 0,
            }
        }
        let mut best: Option<usize> = None;
        for (i, x) in tb.iter().enumerate().skip(from) {
            if !x.has_end() {
                continue;
            }
            let the_end = x.get_end();
            let best_end = best
                .and_then(|b| tb.get_partition(b))
                .map_or(0, Partition::get_end);
            if the_end < pa.start && (best.is_none() || best_end < the_end) {
                best = Some(i);
            }
        }
        if best.is_none() && real_parent.is_some() {
            best = real_parent;
        }
        tb.insert_partition(best, pa);
    }

    /// `check_container_freespace (cxt, parts, tb, cont)`: the gaps between
    /// the logical partitions of the extended partition `cont` (`parts`
    /// sorted by start), added to `tb`.
    fn check_container_freespace(&mut self, parts: &Table, tb: &mut Table, cont: &Partition) {
        let mut last = cont.start;
        let grain = if self.grain > self.sector_size {
            div(self.grain, self.sector_size)
        } else {
            1
        };
        for pa in parts.iter() {
            if !pa.used || !pa.is_nested() || !pa.has_start() {
                continue;
            }
            let lastplusoff = last.wrapping_add(self.first_lba);
            if pa.start > lastplusoff && pa.start.wrapping_sub(lastplusoff) > grain {
                self.table_add_freespace(tb, lastplusoff, pa.start, Some(cont));
            }
            last = pa.get_end();
        }
        // Free space left at the end of the extended partition.
        let x = cont.start.wrapping_add(cont.size).wrapping_sub(1);
        let lastplusoff = last.wrapping_add(self.first_lba);
        if lastplusoff < x && x.wrapping_sub(lastplusoff) > grain {
            self.table_add_freespace(tb, lastplusoff, x, Some(cont));
        }
    }

    /// `fdisk_get_freespaces (cxt, &tb)`: the free spaces on the disk --
    /// those smaller than the grain left out -- added to `tb`.
    pub fn get_freespaces(&mut self, tb: &mut Table) -> i32 {
        if self.label.is_none() {
            return -EINVAL;
        }
        let mut parts = Table::new();
        let rc = self.get_partitions(&mut parts);
        if rc != 0 {
            return rc;
        }
        parts.sort_partitions(Partition::cmp_start);
        let mut last = self.first_lba;
        let grain = if self.grain > self.sector_size {
            div(self.grain, self.sector_size)
        } else {
            1
        };
        let mut nparts: usize = 0;

        // The gaps between partitions.
        let entries: Vec<Partition> = parts.iter().cloned().collect();
        for pa in &entries {
            if !pa.used || pa.wholedisk || pa.is_nested() || !pa.has_start() {
                continue;
            }
            // Gaps smaller than the grain are left out, to keep partitions
            // aligned -- but not one before the first partition when the
            // first LBA is aligned.
            if last.wrapping_add(grain) < pa.start
                || (nparts == 0 && self.align_lba(last, ALIGN_UP) < pa.start)
            {
                let from = last.wrapping_add(u64::from(nparts != 0));
                self.table_add_freespace(tb, from, pa.start.wrapping_sub(1), None);
            }
            // The gaps between logical partitions.
            if pa.is_container() {
                self.check_container_freespace(&parts, tb, pa);
            }
            if pa.has_end() {
                let pa_end = pa.get_end();
                if pa_end > last {
                    last = pa_end;
                }
            }
            nparts = nparts.wrapping_add(1);
        }

        // Free space after the last partition, at the end of the table.
        if last.wrapping_add(grain) < self.last_lba.wrapping_sub(1) {
            let from = last.wrapping_add(u64::from(last > self.first_lba || nparts != 0));
            let end = self.last_lba;
            if let Some(pa) = self.new_freespace(from, end, None) {
                tb.add_partition(pa);
            }
        }
        0
    }

    /// `fdisk_apply_table (cxt, tb)`: adds `tb`'s partitions to the label,
    /// those with neither a start nor a default start aside.
    pub fn apply_table(&mut self, tb: &mut Table) -> i32 {
        for pa in tb.iter_mut() {
            if !pa.has_start() && !pa.start_follow_default {
                continue;
            }
            let rc = self.add_partition(Some(pa), None);
            if rc != 0 {
                return rc;
            }
        }
        0
    }
}
