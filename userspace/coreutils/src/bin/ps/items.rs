//! procps' `<pids>` interface, as `ps` registers it.
//!
//! The items, their values and the sort are the library's
//! ([`coreutils::procps::pids`], shared with `top`). What is `ps`'s own is how
//! it asks for them. Each column's print function first *registers* the items
//! it needs (`setREL1` .. `setREL4`, called with a NULL buffer from
//! `finalize_stacks`); the library then reads each process into a "stack" --
//! one value per registered item, in registration order -- and the print
//! functions read their values back out by position. Two of upstream's
//! behaviours follow from that design and are kept here because they show:
//!
//! * **At most 70 items** (`PIDSITEMS`). An item registered after the 70th is
//!   given the `noop` slot, which always reads as zero (or a NULL string), so
//!   a very long `-o` list prints zeros in its last columns.
//! * **A sort key must be registered to sort.** `procps_pids_sort` finds the
//!   key's slot and returns without sorting if it has none. Most keys are
//!   registered by their column's own print function, but not all: `fgid`'s
//!   sort key is `FLAGS` and `utime`'s is `TICS_USER`, which nothing
//!   registers, so `--sort=utime` leaves the order alone.

use coreutils::procps::readproc::{Fill, Proc};
use coreutils::procps::{pids, sysinfo};
use std::path::Path;

pub use coreutils::procps::pids::{ASCEND, DESCEND, Item, ItemCtx, Stack, Val};

/// `PIDSITEMS`: how many items a stack holds.
pub const PIDSITEMS: usize = 70;

/// The registered items, in order: `Pids_items` and the `rel_*` indices.
#[derive(Default)]
pub struct Registry {
    items: Vec<Item>,
    rel: Vec<(Item, usize)>,
    noop: Option<usize>,
}

impl Registry {
    /// `chkREL`: give `item` a slot if it has none. Past [`PIDSITEMS`] it
    /// shares the `noop` slot.
    pub fn chk(&mut self, item: Item) {
        if self.rel.iter().any(|&(i, _)| i == item) {
            return;
        }
        let at = if self.items.len() < PIDSITEMS {
            self.items.push(item);
            self.items.len().saturating_sub(1)
        } else {
            // `rel_noop`, which `finalize_stacks` registers early.
            self.noop.unwrap_or(0)
        };
        if item == Item::Noop {
            self.noop = Some(at);
        }
        self.rel.push((item, at));
    }

    /// The registered items, in slot order.
    #[must_use]
    pub fn items(&self) -> &[Item] {
        &self.items
    }

    /// `rel_ITEM`: the slot `item` reads from.
    #[must_use]
    pub fn rel(&self, item: Item) -> Option<usize> {
        self.rel
            .iter()
            .find(|&&(i, _)| i == item)
            .map(|&(_, at)| at)
    }

    /// `pids_libflags_set`: the files the registered items need.
    #[must_use]
    pub fn fill(&self) -> Fill {
        pids::fill_for(&self.items).0
    }
}

// C's `double` conversions, shared with `pgrep` in the library.
pub use coreutils::procps::cvt::{cvt_u32, cvt_u64, dbl};

/// `pids_assign_results`: the stack for `p`.
#[must_use]
pub fn assign(reg: &Registry, p: &Proc, ctx: &mut ItemCtx) -> Stack {
    pids::assign(reg.items(), p, ctx)
}

/// `procps_pids_sort`: the stacks named by `idx` sorted on `item` -- or left
/// alone if `item` has no slot, or the direction is neither [`ASCEND`] nor
/// [`DESCEND`], which `ps` does not check for.
pub fn sort(reg: &Registry, stacks: &[Stack], idx: &mut [usize], item: Item, dir: i32) {
    // `ps` calls `procps_pids_sort` for its side effect and ignores the NULL
    // it returns for an unregistered key: the order is simply left as it was.
    let _ = pids::sort(reg.items(), stacks, idx, item, dir);
}

/// `boot_time()` and `memory_total()`: each kept once it is known, and
/// read again while it is 0 -- upstream's `static` is its own "not yet".
pub struct SysCache {
    root: std::path::PathBuf,
    boot: u32,
    mem: u64,
}

impl SysCache {
    /// Nothing read yet.
    #[must_use]
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            boot: 0,
            mem: 0,
        }
    }

    /// `boot_time`: `btime` as an `unsigned int`, or `None` when `/proc/stat`
    /// cannot give it. Upstream reads again while it is 0.
    pub fn boot_time(&mut self) -> Option<u32> {
        if self.boot != 0 {
            return Some(self.boot);
        }
        let b = sysinfo::boot_time(&self.root).map(|v| {
            let bytes = v.to_le_bytes();
            u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
        })?;
        self.boot = b;
        Some(b)
    }

    /// `memory_total`: `MemTotal`, or `None` when `/proc/meminfo` cannot
    /// give it. Upstream reads again while it is 0.
    pub fn memory_total(&mut self) -> Option<u64> {
        if self.mem != 0 {
            return Some(self.mem);
        }
        let m = sysinfo::memory_total(&self.root)?;
        self.mem = m;
        Some(m)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn the_seventy_first_item_shares_the_noop_slot() {
        let mut r = Registry::default();
        r.chk(Item::Noop);
        let fillers = [
            Item::Cmd,
            Item::IdEgid,
            Item::IdEuid,
            Item::IdFgid,
            Item::IdFuid,
            Item::IdPid,
            Item::IdPpid,
            Item::IdRgid,
            Item::IdRuid,
            Item::IdSession,
            Item::IdSgid,
            Item::IdSuid,
            Item::IdTgid,
            Item::State,
            Item::Tty,
            Item::IdPgrp,
            Item::IdTpgid,
            Item::Nice,
            Item::Nlwp,
            Item::Rss,
            Item::VmRssLocked,
            Item::Sigblocked,
            Item::Sigcatch,
            Item::Sigignore,
            Item::Signals,
            Item::Sigpending,
            Item::TicsAll,
            Item::TicsAllC,
            Item::TimeAll,
            Item::TimeElapsed,
            Item::TicsBegan,
            Item::Extra,
            Item::AddrCodeEnd,
            Item::AddrCodeStart,
            Item::AddrCurrEip,
            Item::AddrCurrEsp,
            Item::AddrStackStart,
            Item::AutogrpId,
            Item::AutogrpNice,
            Item::Cgname,
            Item::Cgroup,
            Item::Cmdline,
            Item::Environ,
            Item::Exe,
            Item::Flags,
            Item::FltMaj,
            Item::FltMajC,
            Item::FltMin,
            Item::FltMinC,
            Item::IdEgroup,
            Item::IdEuser,
            Item::IdFgroup,
            Item::IdFuser,
            Item::IdLogin,
            Item::IdRgroup,
            Item::IdRuser,
            Item::IdSgroup,
            Item::IdSuser,
            Item::IoReadBytes,
            Item::IoReadChars,
            Item::IoReadOps,
            Item::IoWriteBytes,
            Item::IoWriteCbytes,
            Item::IoWriteChars,
            Item::IoWriteOps,
            Item::Lxcname,
            Item::NsCgroup,
            Item::NsIpc,
            Item::NsMnt,
        ];
        for f in fillers {
            r.chk(f);
        }
        assert_eq!(r.items().len(), PIDSITEMS);
        r.chk(Item::NsNet);
        assert_eq!(r.items().len(), PIDSITEMS);
        assert_eq!(r.rel(Item::NsNet), Some(0));
        assert_eq!(r.rel(Item::NsMnt), Some(69));
    }
}
