//! Submatches, for a pattern without back-references: given where the
//! leftmost-longest match is, where every group is, by the standard's rule --
//! each subpattern, left to right, as long as the rest still lets the whole
//! match be what it is.
//!
//! This is Henry Spencer's "dissect" (4.4BSD's regex), over the tree. A node
//! is known to match exactly `[i, j)`, and is taken apart top-down:
//!
//! - a group reports `[i, j)`, and clears the groups inside it, which report
//!   only within it (XSH regexec: "within the substring reported in
//!   pmatch[j]");
//! - of an alternation's branches, the first that matches `[i, j)` exactly;
//! - a concatenation's elements, each ending as far right as leaves the rest
//!   able to reach `j`. One backward run of the concatenation from `j` marks,
//!   at every position, which of its suffixes can match from there; then a
//!   forward run of each element finds its ends, and the largest marked one
//!   is the boundary;
//! - a repetition's iterations likewise, each as long as the rest allows --
//!   an iteration past the minimum count non-empty -- and only the last is
//!   taken apart further, since only it is reported. One that matched
//!   nothing reports a single empty iteration if it can, "a null string ...
//!   longer than no match at all" (XBD 9.1).
//!
//! Every run is the Thompson simulation of `prog.rs`, over the one fragment
//! asked about, so a submatch costs time linear in what its node spans, not
//! exponential in the pattern; and the work is a stack, not a recursion.

use super::parse::{NONE, Node};
use super::prog::{MarkTable, Positions, Program, Run, Subject, Vm};
use crate::list::{List, NoMem};

/// One match's taking apart: the program, the subject, and the work.
struct Dissect<'p, 'v> {
    p: &'p Program,
    sub: &'p Subject<'p>,
    vm: &'v mut Vm,
    /// Nodes with the text each matches, still to take apart.
    work: List<(u32, usize, usize)>,
    ends: Positions,
}

impl<'p> Dissect<'p, '_> {
    fn fwd(&self, n: u32) -> Run<'p> {
        let f = self.p.info(n).fwd;
        Run {
            code: &self.p.fwd,
            sets: &self.p.tree.sets,
            entry: f.0,
            exit: f.1,
            owner: NONE,
            sub: self.sub,
        }
    }

    fn rev(&self, n: u32) -> Run<'p> {
        let f = self.p.info(n).rev;
        Run {
            code: &self.p.rev,
            sets: &self.p.tree.sets,
            entry: f.0,
            exit: f.1,
            owner: n,
            sub: self.sub,
        }
    }

    /// Where node `n`, begun at `i`, can end, up to `j`: into `self.ends`.
    fn ends_of(&mut self, n: u32, i: usize, j: usize) -> Result<(), NoMem> {
        let r = self.fwd(n);
        self.vm.ends(r, i, j, &mut self.ends)
    }

    /// Node `n` matches `[i, j)` exactly.
    fn exact(&mut self, n: u32, i: usize, j: usize) -> Result<bool, NoMem> {
        self.ends_of(n, i, j)?;
        Ok(self.ends.has(j))
    }

    /// For each position of `[i, j]`, which of node `n`'s boundaries the rest
    /// of it can be matched to `j` from.
    fn marks_of(&mut self, n: u32, i: usize, j: usize, slots: usize) -> Result<MarkTable, NoMem> {
        let mut table = MarkTable::new(i, j, slots)?;
        let r = self.rev(n);
        self.vm.marks(r, i, j, &mut table)?;
        Ok(table)
    }

    /// A concatenation of `items` matching `[i, j)`: each element as long as
    /// the rest allows, pushed if it holds a group.
    fn cat(&mut self, n: u32, items: &[u32], i: usize, j: usize) -> Result<(), NoMem> {
        let count = items.len();
        let p = self.p;
        let Some(last) = items.iter().rposition(|&k| p.info(k).has_group) else {
            return Ok(());
        };
        let table = self.marks_of(n, i, j, count)?;
        let mut pos = i;
        for (k, &item) in items.iter().enumerate().take(last.wrapping_add(1)) {
            self.ends_of(item, pos, j)?;
            let next = k.wrapping_add(1);
            let m = if next == count {
                self.ends.has(j).then_some(j)
            } else {
                self.ends.max_where(|m| table.any(m, next, next))
            };
            let Some(m) = m else {
                break;
            };
            if p.info(item).has_group {
                self.work.push((item, pos, m))?;
            }
            pos = m;
        }
        Ok(())
    }

    /// A repetition of `body`, `min` to `max` times, matching `[i, j)`: its
    /// iterations, pushed to be taken apart.
    ///
    /// Only the last, when the body is a group: every iteration goes through
    /// that group, and the last one's report replaces the others'. But a
    /// body that is itself a repetition -- `(a)*{2}` -- can make an iteration
    /// that goes through no group at all (an empty one, with none of the
    /// inner repetition's), and then the group's last match is an earlier
    /// iteration's: so all of them are pushed, and taken apart in order.
    fn rep(
        &mut self,
        n: u32,
        (body, min, max): (u32, u32, u32),
        i: usize,
        j: usize,
    ) -> Result<(), NoMem> {
        let b = self.p.info(body);
        let every = !matches!(self.p.node(body), Node::Group { .. });
        if i == j {
            // Nothing matched: `min` empty iterations, or -- with none
            // required -- one, if the body can match nothing here, rather
            // than none. All alike, so one stands for the rest.
            if min >= 1 || self.exact(body, i, j)? {
                self.work.push((body, i, j))?;
            }
            return Ok(());
        }
        if b.min_len == b.max_len && b.min_len > 0 {
            // Every iteration the same length: the last is the last `w`
            // bytes -- and, none being empty, every one goes through the
            // body's groups, so it is the only one that needs taking apart.
            self.work.push((body, j.wrapping_sub(b.min_len), j))?;
            return Ok(());
        }
        // Slot k of the backward run: k iterations match from that position
        // to `j` -- with no limit, the last slot, `min`, is "at least min".
        // No more than `min + (j - i)` iterations can fit, those past `min`
        // being non-empty, so no slot past that is ever asked about.
        let top = if max == NONE {
            min as usize
        } else {
            (max as usize).min((min as usize).saturating_add(j.wrapping_sub(i)))
        };
        let table = self.marks_of(n, i, j, top.wrapping_add(1))?;
        let mut pos = i;
        let mut t: u32 = 0;
        // With `every`, the iterations found so far, pushed at the end in
        // reverse so that the first is taken apart first.
        let mut spans: List<(usize, usize)> = List::new();
        loop {
            t = t.saturating_add(1);
            self.ends_of(body, pos, j)?;
            // The iterations left after this one: at least `min - t`, at
            // most `max - t`.
            let lo = min.saturating_sub(t) as usize;
            let hi = if max == NONE {
                min as usize
            } else {
                (max.saturating_sub(t) as usize).min(top)
            };
            let m = self
                .ends
                .max_where(|m| (m > pos || t <= min) && table.any(m, lo, hi));
            let Some(m) = m else {
                break;
            };
            if every {
                spans.push((pos, m))?;
            }
            if m == j {
                // The last iteration: this one, or -- when the minimum is not
                // yet reached -- the empty ones still owed, at `j` (all
                // alike, so one of them stands for the rest).
                if t < min {
                    if every {
                        spans.push((j, j))?;
                    } else {
                        self.work.push((body, j, j))?;
                    }
                } else if !every {
                    self.work.push((body, pos, j))?;
                }
                break;
            }
            pos = m;
        }
        while let Some((a, b)) = spans.pop() {
            self.work.push((body, a, b))?;
        }
        Ok(())
    }
}

/// `pm[1..]` filled in for the match `[start, end)`; `pm[0]` is the caller's.
pub(super) fn dissect(
    p: &Program,
    vm: &mut Vm,
    sub: &Subject<'_>,
    start: usize,
    end: usize,
    pm: &mut [(isize, isize)],
) -> Result<(), NoMem> {
    let mut d = Dissect {
        p,
        sub,
        vm,
        work: List::new(),
        ends: Positions::new(),
    };
    // A group met a second time -- in a later iteration of a repetition
    // taken apart iteration by iteration -- clears the groups inside it
    // first; met once, there is nothing inside it yet to clear, and skipping
    // that keeps a chain of nested groups linear.
    let mut seen = List::filled(pm.len(), false)?;
    d.work.push((p.tree.root, start, end))?;
    while let Some((n, i, j)) = d.work.pop() {
        let info = p.info(n);
        if !info.has_group {
            continue;
        }
        match p.node(n) {
            Node::Group { index, body } => {
                if let Some(s) = seen.get_mut(index as usize) {
                    if *s {
                        for g in index.wrapping_add(1)..=info.max_group {
                            if let Some(slot) = pm.get_mut(g as usize) {
                                *slot = (-1, -1);
                            }
                        }
                    }
                    *s = true;
                }
                if let Some(slot) = pm.get_mut(index as usize) {
                    *slot = (i.cast_signed(), j.cast_signed());
                }
                if body != NONE {
                    d.work.push((body, i, j))?;
                }
            }
            Node::Alt { first, len } => {
                for &b in p.kids(first, len) {
                    if d.exact(b, i, j)? {
                        d.work.push((b, i, j))?;
                        break;
                    }
                }
            }
            Node::Cat { first, len } => d.cat(n, p.kids(first, len), i, j)?,
            Node::Rep { body, min, max } => d.rep(n, (body, min, max), i, j)?,
            Node::Empty | Node::Set(_) | Node::Assert(_) | Node::BackRef(_) => {}
        }
    }
    Ok(())
}
