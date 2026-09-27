//! `walk.c`: a tree's lines in the order they are printed -- each root in
//! table order, then its children depth first; and after each root, the
//! children of every group whose last member the walk reached below it.
//!
//! Both the width calculation and the printing walk the tree this way, and
//! each walk also moves the groups' chart along line by line (see
//! [`crate::grouping`]), so a column is measured with the chart it will be
//! printed with.

use crate::{Error, LineId, Table};

impl Table {
    /// `is_tree_root`: neither a line's child nor a group's.
    pub(crate) fn is_tree_root(&self, ln: LineId) -> bool {
        self.line(ln)
            .is_some_and(|l| l.parent.is_none() && l.parent_group.is_none())
    }

    /// `is_last_tree_root`.
    pub(crate) fn is_last_tree_root(&self, ln: LineId) -> bool {
        self.walk_last_tree_root == Some(ln)
    }

    /// `is_child`: a line's child.
    pub(crate) fn is_child(&self, ln: LineId) -> bool {
        self.line(ln).is_some_and(|l| l.parent.is_some())
    }

    /// `has_children`: a line's own children, not its group's.
    pub(crate) fn has_children(&self, ln: LineId) -> bool {
        self.line(ln).is_some_and(|l| !l.children.is_empty())
    }

    /// `walk_line`: `ln` (counted as pending its group's children, if it is
    /// the last member of a group that has some), the chart moved to it,
    /// `f` called on it, then its children. The first failure ends the walk.
    fn walk_line<F>(&mut self, ln: LineId, f: &mut F) -> Result<(), Error>
    where
        F: FnMut(&mut Table, LineId) -> Result<(), Error>,
    {
        if self.is_group_member(ln) && self.is_last_group_member(ln) && self.has_group_children(ln)
        {
            self.ngrpchlds_pending = self.ngrpchlds_pending.saturating_add(1);
        }
        if self.has_groups() {
            self.groups_update_grpset(ln)?;
        }
        f(self, ln)?;
        let children = self
            .line(ln)
            .map(|l| l.children.clone())
            .unwrap_or_default();
        for child in children {
            self.walk_line(child, f)?;
        }
        Ok(())
    }

    /// `scols_walk_tree(tb, cl, f, data)`: every line a root leads to, in
    /// print order. A line no root leads to -- one whose ancestry loops --
    /// is not visited.
    ///
    /// # Errors
    ///
    /// `f`'s first failure, which ends the walk.
    pub(crate) fn walk_tree<F>(&mut self, f: &mut F) -> Result<(), Error>
    where
        F: FnMut(&mut Table, LineId) -> Result<(), Error>,
    {
        self.ngrpchlds_pending = 0;
        self.walk_last_tree_root = None;
        self.walk_last_done = false;
        if self.has_groups() {
            self.groups_reset_state();
        }
        // The last root, in table order (or the first line, if none is).
        let order = self.order.clone();
        for &ln in &order {
            if self.walk_last_tree_root.is_none() {
                self.walk_last_tree_root = Some(ln);
            }
            if self.is_child(ln) || self.is_group_child(ln) {
                continue;
            }
            self.walk_last_tree_root = Some(ln);
        }
        let rc = self.walk_roots(&order, f);
        self.ngrpchlds_pending = 0;
        self.walk_last_done = false;
        rc
    }

    /// `scols_walk_tree`'s loop: each root, then the children of the groups
    /// it made ready, the last ready group's first.
    fn walk_roots<F>(&mut self, order: &[LineId], f: &mut F) -> Result<(), Error>
    where
        F: FnMut(&mut Table, LineId) -> Result<(), Error>,
    {
        for &ln in order {
            if !self.is_tree_root(ln) {
                continue;
            }
            if self.walk_last_tree_root == Some(ln) {
                self.walk_last_done = true;
            }
            self.walk_line(ln, f)?;
            while self.ngrpchlds_pending > 0 {
                let Some(gr) = self.grpset_get_printable_children() else {
                    // "ngrpchlds_pending counter invalid"
                    self.ngrpchlds_pending = 0;
                    break;
                };
                self.ngrpchlds_pending = self.ngrpchlds_pending.saturating_sub(1);
                let children = self
                    .groups
                    .get(gr.0)
                    .map(|g| g.children.clone())
                    .unwrap_or_default();
                for child in children {
                    self.walk_line(child, f)?;
                }
            }
        }
        Ok(())
    }

    /// `scols_walk_is_last`: whether `ln` is the last line the walk will
    /// visit -- after which no line separator is printed.
    pub(crate) fn walk_is_last(&self, ln: LineId) -> bool {
        if !self.walk_last_done || self.ngrpchlds_pending > 0 || self.has_children(ln) {
            return false;
        }
        if self.is_tree_root(ln) && !self.is_last_tree_root(ln) {
            return false;
        }
        if self.is_group_member(ln)
            && (!self.is_last_group_member(ln) || self.has_group_children(ln))
        {
            return false;
        }
        if let Some(mut parent) = self.line(ln).and_then(|l| l.parent) {
            if !self.is_last_child(ln) {
                return false;
            }
            // Up to the top, each ancestor the last of its parent's
            // children. (Upstream climbs without end through an ancestry
            // that loops; no line in one is ever walked, so none gets here.)
            for _ in 0..self.lines.len() {
                if self.is_child(parent) && !self.is_last_child(parent) {
                    return false;
                }
                match self.line(parent).and_then(|l| l.parent) {
                    Some(p) => parent = p,
                    None => break,
                }
            }
            if self.is_tree_root(parent) && !self.is_last_tree_root(parent) {
                return false;
            }
        }
        !(self.is_group_child(ln) && !self.is_last_group_child(ln))
    }
}
