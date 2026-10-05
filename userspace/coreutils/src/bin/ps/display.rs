//! `display.c`: reading the process table and printing it -- in `/proc`'s
//! order (`simple_spew`), or sorted and perhaps as a forest (`fancy_spew`).

use crate::formats::{self, Pr};
use crate::items::{self, ASCEND, Item, Stack};
use crate::{Exit, FormatNode, HEAD_MULTI, HEAD_NONE, Ps, SortNode, sel, tf};

impl Ps {
    /// `check_headers`.
    fn check_headers(&mut self) {
        if self.header_type == HEAD_MULTI {
            self.header_gap = self.screen_rows.wrapping_sub(1);
            return;
        }
        if self.header_type == HEAD_NONE {
            self.lines_to_next_header = -1;
            return;
        }
        let head_normal = self
            .format_list
            .iter()
            .filter(|n| !n.name.is_empty() && n.pr.is_some())
            .count();
        if head_normal == 0 {
            self.lines_to_next_header = -1;
        }
    }

    /// `lists_and_needs`: the process and thread column lists, which differ
    /// only when both are shown (`m`, `-m`): a thread-only column prints `-`
    /// on a process line, and a process-only one on a thread line.
    pub fn lists_and_needs(&mut self) -> Result<(), Exit> {
        self.check_headers();
        if self.thread_flags & tf::SHOW_BOTH == 0 {
            self.proc_format_list = self.format_list.clone();
            self.task_format_list = self.format_list.clone();
            return Ok(());
        }
        let mut procs = Vec::new();
        let mut tasks = Vec::new();
        for node in &mut self.format_list {
            let mut t = node.clone();
            match node.flags & formats::PRINT_MASK {
                formats::TO => node.pr = Some(Pr::Nop),
                formats::PO => t.pr = Some(Pr::Nop),
                formats::AN | formats::ET => {}
                _ => {
                    return Err(self_catastrophe());
                }
            }
            procs.push(node.clone());
            tasks.push(t);
        }
        self.proc_format_list = procs;
        self.task_format_list = tasks;
        Ok(())
    }

    /// Read the table, or `fatal library error, reap`.
    fn reap_or_die(
        &mut self,
        threads: bool,
        pids: Option<&[u32]>,
        need_some: bool,
    ) -> Result<Vec<Stack>, Exit> {
        if let Some(list) = pids
            && list.len() > 255
        {
            self.eprint(b"fatal library error, reap\n");
            return Err(Exit::Status(1));
        }
        match self.reap(threads, pids) {
            Some(stacks) if !need_some || !stacks.is_empty() => Ok(stacks),
            _ => {
                self.eprint(b"fatal library error, reap\n");
                Err(Exit::Status(1))
            }
        }
    }

    /// `simple_spew`: every wanted process, in the order `/proc` lists them.
    pub fn simple_spew(&mut self) -> Result<(), Exit> {
        let threads = self.thread_flags & (tf::LOOSE_TASKS | tf::SHOW_TASK) != 0;
        let stacks = match self.selection_list.first() {
            Some(node) if node.typecode == sel::PID_QUICK => {
                let pids: Vec<u32> = node
                    .u
                    .iter()
                    .map(|v| u32::try_from(v.num & 0xffff_ffff).unwrap_or(0))
                    .collect();
                // procps_pids_select: a list of none found is not an error.
                self.reap_or_die(threads, Some(&pids), false)?
            }
            _ => self.reap_or_die(threads, None, true)?,
        };
        let mode = self.thread_flags & (tf::SHOW_PROC | tf::LOOSE_TASKS | tf::SHOW_TASK);
        if mode == tf::SHOW_PROC {
            let fmt = self.proc_format_list.clone();
            for buf in &stacks {
                if self.want_this_proc(buf) {
                    self.show_one_proc(Some(buf), &fmt)?;
                }
            }
        } else if mode == tf::SHOW_TASK || mode == tf::SHOW_PROC | tf::LOOSE_TASKS {
            let fmt = self.task_format_list.clone();
            for buf in &stacks {
                if self.want_this_proc(buf) {
                    self.show_one_proc(Some(buf), &fmt)?;
                }
            }
        } else if mode == tf::SHOW_PROC | tf::SHOW_TASK {
            let mut idx: Vec<usize> = (0..stacks.len()).collect();
            items::sort(&self.registry, &stacks, &mut idx, Item::TicsBegan, ASCEND);
            items::sort(&self.registry, &stacks, &mut idx, Item::IdTgid, ASCEND);
            let pfmt = self.proc_format_list.clone();
            let tfmt = self.task_format_list.clone();
            let get = |k: usize| idx.get(k).and_then(|&i| stacks.get(i));
            let mut i = 0usize;
            while let Some(buf) = get(i) {
                if !self.want_this_proc(buf) {
                    i = i.saturating_add(1);
                    continue;
                }
                let me = self.rsv(Item::IdPid, buf).s_int();
                self.show_one_proc(Some(buf), &pfmt)?;
                // Upstream's inner loop starts at the same entry, so the
                // process's own thread is shown as its first thread. An
                // entry that is not its group's leader would send upstream
                // round this loop forever; here it is passed over.
                let mut shown = false;
                while let Some(t) = get(i) {
                    if self.rsv(Item::IdTgid, t).s_int() != me {
                        break;
                    }
                    self.show_one_proc(Some(t), &tfmt)?;
                    shown = true;
                    i = i.saturating_add(1);
                }
                if !shown {
                    i = i.saturating_add(1);
                }
            }
        }
        Ok(())
    }

    /// `prep_forest_sort`: a forest is sorted by start time, after parent if
    /// nothing else was asked for.
    fn prep_forest_sort(&mut self) {
        let key = |name: &[u8]| {
            formats::search_format(name).map(|f| SortNode {
                sr: f.sr,
                xe: f.pr,
                reverse: ASCEND,
            })
        };
        if self.sort_list.is_empty()
            && let Some(n) = key(b"ppid")
        {
            self.sort_list.insert(0, n);
        }
        if let Some(n) = key(b"start_time") {
            self.sort_list.insert(0, n);
        }
    }

    /// `fancy_spew`: sorted, and as a forest if asked.
    pub fn fancy_spew(&mut self) -> Result<(), Exit> {
        let threads = self.thread_flags & tf::LOOSE_TASKS != 0;
        let stacks = self.reap_or_die(threads, None, true)?;
        let mut idx: Vec<usize> = (0..stacks.len())
            .filter(|&i| stacks.get(i).is_some_and(|b| self.want_this_proc(b)))
            .collect();
        if idx.is_empty() {
            return Ok(());
        }
        if self.forest_type != 0 {
            self.prep_forest_sort();
        }
        let keys = std::mem::take(&mut self.sort_list);
        for k in &keys {
            items::sort(&self.registry, &stacks, &mut idx, k.sr, k.reverse);
        }
        let procs: Vec<&Stack> = idx.iter().filter_map(|&i| stacks.get(i)).collect();
        if self.forest_type != 0 {
            self.show_forest(&procs)?;
        } else {
            let fmt = self.proc_format_list.clone();
            for p in &procs {
                self.show_one_proc(Some(p), &fmt)?;
            }
        }
        Ok(())
    }

    /// `show_forest`: each process whose parent is not shown is a root, and
    /// the roots are found from the *end* of the sorted list.
    fn show_forest(&mut self, procs: &[&Stack]) -> Result<(), Exit> {
        let fmt = self.format_list.clone();
        for i in (0..procs.len()).rev() {
            let Some(p) = procs.get(i) else { continue };
            let ppid = self.rsv(Item::IdPpid, p).s_int();
            let has_parent = procs
                .iter()
                .any(|q| self.rsv(Item::IdPid, q).s_int() == ppid);
            if !has_parent {
                self.show_tree(procs, &fmt, i, 0, false)?;
            }
        }
        Ok(())
    }

    /// `show_tree`: a process, then its children -- which must follow each
    /// other in the sorted list, since only the first run of them is found.
    /// The children of PID 1 are not indented except under `-H`.
    fn show_tree(
        &mut self,
        procs: &[&Stack],
        fmt: &[FormatNode],
        me: usize,
        level: usize,
        sibling: bool,
    ) -> Result<(), Exit> {
        let plen = self.forest_prefix.len();
        let safe = |l: usize| l < plen;
        if !safe(level) {
            return Err(self.catastrophic("display.c", 407, "please report this bug"));
        }
        if level > 0
            && let Some(c) = self.forest_prefix.get_mut(level.saturating_sub(1))
        {
            *c = if sibling { b'+' } else { b'L' };
        }
        if let Some(c) = self.forest_prefix.get_mut(level) {
            *c = 0;
        }
        let Some(p) = procs.get(me) else {
            return Ok(());
        };
        self.show_one_proc(Some(p), fmt)?;
        let my_pid = self.rsv(Item::IdPid, p).s_int();
        let Some(mut i) = procs
            .iter()
            .position(|q| self.rsv(Item::IdPpid, q).s_int() == my_pid)
        else {
            return Ok(());
        };
        if level > 0
            && let Some(c) = self.forest_prefix.get_mut(level.saturating_sub(1))
        {
            *c = if sibling { b'|' } else { b' ' };
        }
        if let Some(c) = self.forest_prefix.get_mut(level) {
            *c = 0;
        }
        while i < procs.len() {
            let more = procs
                .get(i.saturating_add(1))
                .is_some_and(|q| self.rsv(Item::IdPpid, q).s_int() == my_pid);
            let next_level = if my_pid == 1 && self.forest_type != b'u' {
                level
            } else if safe(level.saturating_add(1)) {
                level.saturating_add(1)
            } else {
                level
            };
            self.show_tree(procs, fmt, i, next_level, more)?;
            i = i.saturating_add(1);
            if !more {
                break;
            }
        }
        if let Some(c) = self.forest_prefix.get_mut(level) {
            *c = 0;
        }
        Ok(())
    }

    /// `show_one_proc ((proc_t *) -1, format_list)`: if nothing was printed,
    /// the header (if one is due) and exit status 1.
    pub fn show_end(&mut self) -> Result<(), Exit> {
        if self.did_stuff {
            return Ok(());
        }
        self.lines_to_next_header = self.lines_to_next_header.wrapping_sub(1);
        if self.lines_to_next_header == 0 {
            self.lines_to_next_header = self.header_gap;
            let fmt = self.format_list.clone();
            self.show_one_proc(None, &fmt)?;
        }
        Err(Exit::Status(1))
    }
}

/// `lists_and_needs`' catastrophe: a column with no print flag, which no
/// specifier has.
fn self_catastrophe() -> Exit {
    Exit::Status(1)
}
