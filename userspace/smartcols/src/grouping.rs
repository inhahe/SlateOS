//! `grouping.c`: lines drawn as groups -- `lsblk --merge`'s RAID members
//! with the array they make, shown once, below the last of them.
//!
//! A group has members, which are ordinary lines of the tree, and children,
//! which are lines with no parent of their own: they are walked after the
//! group's last member, as children of every member at once. The tree
//! column draws a chart of the groups to the left of the tree, three cells
//! a group (`grpset`), updated line by line as the walk goes:
//!
//! ```text
//! ┌┄▶ sda1
//! └┬▶ sdb1
//!  └┄ md0
//! ```
//!
//! As upstream's, the chart's state is changed by the walk that prints the
//! table -- and by each walk that measures a column before it, which only
//! grows `grpset` (nothing is drawn while measuring) so that the tree
//! column can be made wide enough for the chart.

use crate::print::Buf;
use crate::{Error, GState, Group, GroupId, LineId, Table};

/// `SCOLS_GRPSET_CHUNKSIZ`: the cells of the chart each group takes.
pub(crate) const GRPSET_CHUNKSIZ: usize = 3;

impl Table {
    /// `has_groups`.
    pub(crate) fn has_groups(&self) -> bool {
        !self.groups.is_empty()
    }

    fn group(&self, gr: GroupId) -> Option<&Group> {
        self.groups.get(gr.0)
    }

    fn group_state(&self, gr: GroupId) -> GState {
        self.group(gr).map_or(GState::None, |g| g.state)
    }

    fn line_group(&self, ln: LineId) -> Option<GroupId> {
        self.line(ln).and_then(|l| l.group)
    }

    pub(crate) fn line_parent_group(&self, ln: LineId) -> Option<GroupId> {
        self.line(ln).and_then(|l| l.parent_group)
    }

    /// `is_group_member`.
    pub(crate) fn is_group_member(&self, ln: LineId) -> bool {
        self.line_group(ln).is_some()
    }

    /// `is_first_group_member`: first in its group's members list.
    pub(crate) fn is_first_group_member(&self, ln: LineId) -> bool {
        self.line_group(ln)
            .and_then(|gr| self.group(gr))
            .is_some_and(|g| g.members.first() == Some(&ln))
    }

    /// `is_last_group_member`: last in its group's members list.
    pub(crate) fn is_last_group_member(&self, ln: LineId) -> bool {
        self.line_group(ln)
            .and_then(|gr| self.group(gr))
            .is_some_and(|g| g.members.last() == Some(&ln))
    }

    /// `is_group_child`.
    pub(crate) fn is_group_child(&self, ln: LineId) -> bool {
        self.line_parent_group(ln).is_some()
    }

    /// `is_last_group_child`: last in its group's children list.
    pub(crate) fn is_last_group_child(&self, ln: LineId) -> bool {
        self.line_parent_group(ln)
            .and_then(|gr| self.group(gr))
            .is_some_and(|g| g.children.last() == Some(&ln))
    }

    /// `has_group_children`: a member of a group that has children.
    pub(crate) fn has_group_children(&self, ln: LineId) -> bool {
        self.line_group(ln)
            .and_then(|gr| self.group(gr))
            .is_some_and(|g| !g.children.is_empty())
    }

    /// The children of `ln`'s group, if it is a member of one.
    pub(crate) fn group_children_of(&self, ln: LineId) -> Vec<LineId> {
        self.line_group(ln)
            .and_then(|gr| self.group(gr))
            .map(|g| g.children.clone())
            .unwrap_or_default()
    }

    /// `add_member`.
    fn add_member(&mut self, gr: GroupId, ln: LineId) {
        if let Some(line) = self.lines.get_mut(ln.0) {
            line.group = Some(gr);
        }
        if let Some(g) = self.groups.get_mut(gr.0) {
            g.nmembers = g.nmembers.saturating_add(1);
            g.members.push(ln);
        }
    }

    /// `scols_table_group_lines(tb, ln, member, 0)`: `ln` made a member of
    /// `member`'s group -- a new group, with `member` its first member, if
    /// `member` is in none. Without `ln`, only the group is made.
    ///
    /// A line is a member of one group at most; a group's child may be a
    /// member of another.
    ///
    /// # Errors
    ///
    /// A line is not this table's; or `ln` is a member of a group and
    /// `member` of none or of another.
    pub fn group_lines(&mut self, ln: Option<LineId>, member: LineId) -> Result<(), Error> {
        if member.0 >= self.lines.len() || ln.is_some_and(|l| l.0 >= self.lines.len()) {
            return Err(Error::Invalid);
        }
        let member_group = self.line_group(member);
        if let Some(ln) = ln
            && let Some(ln_group) = self.line_group(ln)
            && member_group != Some(ln_group)
        {
            return Err(Error::Invalid);
        }
        let gr = if let Some(gr) = member_group {
            gr
        } else {
            let gr = GroupId(self.groups.len());
            self.groups.push(Group::default());
            self.add_member(gr, member);
            gr
        };
        if let Some(ln) = ln
            && self.line_group(ln).is_none()
        {
            self.add_member(gr, ln);
        }
        Ok(())
    }

    /// `scols_line_link_group(ln, member, 0)`: `ln` made the last child of
    /// `member`'s group.
    ///
    /// # Errors
    ///
    /// A line is not this table's; `member` is in no group; or `ln`
    /// already has a parent, or is already a group's child.
    pub fn line_link_group(&mut self, ln: LineId, member: LineId) -> Result<(), Error> {
        let gr = self.line_group(member).ok_or(Error::Invalid)?;
        let line = self.line(ln).ok_or(Error::Invalid)?;
        // `ln->parent`, then `!list_empty(&ln->ln_children)`: the one link
        // is in its parent's children or in a group's.
        if line.parent.is_some() || line.parent_group.is_some() {
            return Err(Error::Invalid);
        }
        if let Some(g) = self.groups.get_mut(gr.0) {
            g.children.push(ln);
        }
        if let Some(line) = self.lines.get_mut(ln.0) {
            line.parent_group = Some(gr);
        }
        Ok(())
    }

    /// `groups_fix_members_order(ln)`: `ln` and the lines below it put
    /// back in their groups' members lists in the order the tree walks
    /// them; a group's children, once its last member is back, too.
    fn groups_fix_members_order_line(&mut self, ln: LineId) {
        if let Some(gr) = self.line_group(ln)
            && let Some(g) = self.groups.get_mut(gr.0)
        {
            g.members.push(ln);
        }
        let children = self
            .line(ln)
            .map(|l| l.children.clone())
            .unwrap_or_default();
        for child in children {
            self.groups_fix_members_order_line(child);
        }
        // The members list is being rebuilt, so "last" is checked against
        // the count too.
        if let Some(gr) = self.line_group(ln)
            && self.is_last_group_member(ln)
            && self
                .group(gr)
                .is_some_and(|g| g.nmembers == g.members.len())
        {
            for child in self.group_children_of(ln) {
                self.groups_fix_members_order_line(child);
            }
        }
    }

    /// `scols_groups_fix_members_order`: every group's members listed in
    /// the order the tree walks them, which sorting may have changed.
    pub(crate) fn groups_fix_members_order(&mut self) {
        for g in &mut self.groups {
            g.members.clear();
        }
        for ln in self.order.clone() {
            if self.line(ln).is_some_and(|l| l.parent.is_some()) || self.is_group_child(ln) {
                continue;
            }
            self.groups_fix_members_order_line(ln);
        }
    }

    /// `group_state_for_line`: what `gr`'s chart becomes at `ln`.
    fn group_state_for_line(&self, gr: GroupId, ln: LineId) -> GState {
        let state = self.group_state(gr);
        let group = self.line_group(ln);
        let parent_group = self.line_parent_group(ln);
        if state == GState::None && (group != Some(gr) || !self.is_first_group_member(ln)) {
            // NONE becomes FIRST_MEMBER only, and only at the group's first
            // member.
            return GState::None;
        }
        if group != Some(gr) && parent_group != Some(gr) {
            // Not our line: the chart continues past it.
            match state {
                GState::FirstMember | GState::MiddleMember | GState::ContMembers => {
                    return GState::ContMembers;
                }
                GState::LastMember | GState::MiddleChild | GState::ContChildren => {
                    return GState::ContChildren;
                }
                _ => {}
            }
        } else if group == Some(gr) && self.is_first_group_member(ln) {
            return GState::FirstMember;
        } else if group == Some(gr) && self.is_last_group_member(ln) {
            return GState::LastMember;
        } else if group == Some(gr) {
            return GState::MiddleMember;
        } else if parent_group == Some(gr) && self.is_last_group_child(ln) {
            return GState::LastChild;
        } else if parent_group == Some(gr) {
            return GState::MiddleChild;
        }
        GState::None
    }

    /// `grpset_apply_group_state`: `gr`'s slots, from `at`, filled with it
    /// -- or emptied, when it is no longer drawn.
    fn grpset_apply_group_state(&mut self, at: usize, state: GState, gr: GroupId) {
        let slot = (state != GState::None).then_some(gr);
        for i in at..at.saturating_add(GRPSET_CHUNKSIZ) {
            if let Some(s) = self.grpset.get_mut(i) {
                *s = slot;
            }
        }
        if let Some(g) = self.groups.get_mut(gr.0) {
            g.state = state;
        }
    }

    /// `grpset_locate_freespace(tb, 1, 1)`, the only way upstream calls it:
    /// room for one group, the free slots nearest the end -- or, with none,
    /// three new slots in front of the others. (For the first group there
    /// is nothing to prepend to, and upstream's append is the same.)
    fn grpset_locate_freespace(&mut self) -> usize {
        let mut avail = 0usize;
        for i in (0..self.grpset.len()).rev() {
            if self.grpset.get(i).is_some_and(Option::is_none) {
                avail = avail.saturating_add(1);
            } else {
                avail = 0;
            }
            if avail == GRPSET_CHUNKSIZ {
                return i;
            }
        }
        self.grpset
            .splice(0..0, std::iter::repeat_n(None, GRPSET_CHUNKSIZ));
        0
    }

    /// `grpset_locate_group`: where `gr`'s slots begin.
    fn grpset_locate_group(&self, gr: GroupId) -> Option<usize> {
        self.grpset.iter().position(|&s| s == Some(gr))
    }

    /// `grpset_update`: `gr`'s chart at `ln`.
    ///
    /// Upstream aborts the program when the walk meets a group's lines out
    /// of order -- a first member after the group began, a line of it after
    /// its last child, or a member after its last member -- and so does
    /// this. What it had printed is lost with it, as upstream's buffered
    /// output is.
    ///
    /// # Errors
    ///
    /// `gr` is drawn and its slots are not in the chart: upstream's
    /// `-ENOMEM`, which the chart's own bookkeeping never leads to.
    fn grpset_update(&mut self, ln: LineId, gr: GroupId) -> Result<(), Error> {
        let old = self.group_state(gr);
        let state = self.group_state_for_line(gr, ln);
        if state == GState::FirstMember && old != GState::None {
            // "wrong group initialization"
            std::process::abort();
        }
        if state != GState::None && old == GState::LastChild {
            // "wrong group termination"
            std::process::abort();
        }
        if old == GState::LastMember
            && !matches!(
                state,
                GState::LastChild | GState::ContChildren | GState::MiddleChild | GState::None
            )
        {
            // "wrong group member->child order"
            std::process::abort();
        }
        if old == GState::None && state == GState::None {
            return Ok(());
        }
        let at = if self.grpset.is_empty() || old == GState::None {
            self.grpset_locate_freespace()
        } else {
            self.grpset_locate_group(gr).ok_or(Error::Invalid)?
        };
        self.grpset_apply_group_state(at, state, gr);
        Ok(())
    }

    /// `grpset_update_active_groups`: every group in the chart, in the
    /// chart's order, updated for `ln`.
    fn grpset_update_active_groups(&mut self, ln: LineId) -> Result<(), Error> {
        let mut last = None;
        let mut i = 0;
        while i < self.grpset.len() {
            let slot = self.grpset.get(i).copied().flatten();
            i = i.saturating_add(1);
            let Some(gr) = slot else {
                continue;
            };
            if last == Some(gr) {
                continue;
            }
            last = Some(gr);
            self.grpset_update(ln, gr)?;
        }
        Ok(())
    }

    /// `scols_groups_update_grpset`: the chart at `ln` -- the groups drawn
    /// so far, then `ln`'s own if it starts there.
    pub(crate) fn groups_update_grpset(&mut self, ln: LineId) -> Result<(), Error> {
        self.grpset_update_active_groups(ln)?;
        if let Some(gr) = self.line_group(ln)
            && self.group_state(gr) == GState::None
        {
            self.grpset_update(ln, gr)?;
        }
        Ok(())
    }

    /// `scols_groups_reset_state`: no group drawn, the chart empty (but as
    /// long as it was).
    pub(crate) fn groups_reset_state(&mut self) {
        for g in &mut self.groups {
            g.state = GState::None;
        }
        self.grpset.fill(None);
        self.ngrpchlds_pending = 0;
    }

    /// `scols_grpset_get_printable_children`: the last group in the chart
    /// whose children are next -- its last member reached, or its children
    /// begun.
    pub(crate) fn grpset_get_printable_children(&self) -> Option<GroupId> {
        let mut i = self.grpset.len();
        while i > 0 {
            if let Some(gr) = self.grpset.get(i.saturating_sub(1)).copied().flatten()
                && matches!(self.group_state(gr), GState::ContChildren | GState::LastMember)
            {
                return Some(gr);
            }
            i = i.saturating_sub(GRPSET_CHUNKSIZ);
        }
        None
    }

    /// `grpset_is_empty(tb, idx, &rest)`: whether no group is drawn from
    /// slot `idx` on. Each empty slot passed is counted into `rest` --
    /// which the caller does not reset between calls, so a line with two
    /// groups' children counts the empty slots between them too, and draws
    /// its horizontal line that much longer, as upstream's does.
    fn grpset_is_empty(&self, idx: usize, rest: &mut usize) -> bool {
        for slot in self.grpset.iter().skip(idx) {
            if slot.is_some() {
                return false;
            }
            *rest = rest.saturating_add(1);
        }
        true
    }

    /// `groups_ascii_art_to_buffer(tb, ln, buf, empty)`: the groups' chart
    /// at the current line -- or, for the extra lines of a wrapped cell
    /// (`empty`), only the lines that continue past it. Nothing while
    /// measuring.
    pub(crate) fn groups_art(&self, buf: &mut Buf, empty: bool) {
        if !self.has_groups() || self.is_dummy_print {
            return;
        }
        let sym = self.sym();
        let padding = sym.cell_padding.clone();
        let vert = sym.group_vert.clone().unwrap_or_else(|| b"|".to_vec());
        let horz = sym.group_horz.clone().unwrap_or_else(|| b"-".to_vec());
        let mut filler = padding.clone();
        let mut filled = false;
        let mut rest = 0usize;
        let mut i = 0usize;
        while i < self.grpset.len() {
            let chunk = i;
            i = i.saturating_add(GRPSET_CHUNKSIZ);
            let Some(gr) = self.grpset.get(chunk).copied().flatten() else {
                buf.append_ntimes(GRPSET_CHUNKSIZ, &padding);
                continue;
            };
            let state = self.group_state(gr);
            if empty {
                match state {
                    GState::FirstMember | GState::MiddleMember | GState::ContMembers => {
                        buf.append(&vert);
                        buf.append_ntimes(2, &filler);
                    }
                    GState::LastMember | GState::MiddleChild | GState::ContChildren => {
                        buf.append(&filler);
                        buf.append(&vert);
                        buf.append(&filler);
                    }
                    GState::LastChild => buf.append_ntimes(3, &filler),
                    GState::None => {}
                }
                continue;
            }
            match state {
                GState::FirstMember => {
                    buf.append(sym.group_first_member.as_deref().unwrap_or(b",->"));
                }
                GState::MiddleMember => {
                    buf.append(sym.group_middle_member.as_deref().unwrap_or(b"|->"));
                }
                GState::LastMember => {
                    buf.append(sym.group_last_member.as_deref().unwrap_or(b"\\->"));
                }
                GState::ContMembers => {
                    buf.append(&vert);
                    buf.append_ntimes(2, &filler);
                }
                GState::MiddleChild | GState::LastChild => {
                    if state == GState::MiddleChild {
                        buf.append(&filler);
                        buf.append(sym.group_middle_child.as_deref().unwrap_or(b"|-"));
                    } else {
                        buf.append(&padding);
                        buf.append(sym.group_last_child.as_deref().unwrap_or(b"`-"));
                    }
                    if self.grpset_is_empty(i, &mut rest) {
                        buf.append_ntimes(rest.saturating_add(1), &horz);
                        filled = true;
                    }
                    filler.clone_from(&horz);
                }
                GState::ContChildren => {
                    buf.append(&filler);
                    buf.append(&vert);
                    buf.append(&filler);
                }
                GState::None => {}
            }
            if filled {
                break;
            }
        }
        if !filled {
            buf.append(&filler);
        }
    }
}
