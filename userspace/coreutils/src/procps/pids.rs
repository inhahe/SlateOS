//! procps-ng 4.0.4's `library/pids.c`: a process's items, their values, the
//! sort, and the history that turns two readings into a change.
//!
//! Neither `ps` nor `top` reads a process's fields directly. Each asks the
//! library for a list of *items* -- `PIDS_ID_PID`, `PIDS_MEM_RES`,
//! `PIDS_TICS_ALL_DELTA` -- and gets back, for every process, a *stack*: one
//! value per item, in the order asked. This module is that layer: [`Item`] is
//! `enum pids_item`, [`value`] is each item's `setDECL` function, [`sort`] is
//! `procps_pids_sort`, and [`Pids`] is the context (`struct pids_info`) that
//! reads `/proc` through [`super::readproc`] and keeps what a second reading
//! needs from the first. `ps` registers its items its own way
//! (`bin/ps/items.rs`); `top` keeps one [`Pids`] for its whole run.
//!
//! # What is kept because it shows
//!
//! - **Stacks are reused.** The library hands out the same stacks reading
//!   after reading and assigns each item afresh -- except where an item's
//!   function assigns nothing: `PIDS_TIME_ELAPSED` and the two
//!   `PIDS_UTILIZATION` items leave the slot alone when the process started
//!   at the very tick being read, and `PIDS_noop` never touches its slot. So
//!   [`Pids`] keeps its stacks between readings, and [`value`] is given the
//!   slot's old value. A reset ([`Pids::reset`]) zeroes every slot, as
//!   `pids_cleanup_stacks_all` does.
//! - **The history** (`pids_make_hist`, "stolen from top"): each reading
//!   records every task's ticks and faults by its id, and a task seen in the
//!   previous reading gets the difference; one not seen gets its *whole*
//!   ticks as its change, and no fault change at all. The differences are C
//!   `int`s, cut from 64 bits.
//! - **A NULL string is not an empty one.** An item read by `STR_set` whose
//!   field is NULL becomes the text `[ duplicate CMD ]` (or whichever item it
//!   is), and a vector one (`VEC_set`) a one-string vector of it.
//! - **The sort is glibc's merge sort**, with the library's comparisons, which
//!   are not always consistent: `NUM_srt` subtracts and can overflow.
//!
//! # NUMA
//!
//! `PIDS_PROCESSOR_NODE` is -1: the library's `numa_node_of_cpu` without
//! `libnuma`, which SlateOS does not have (`--disable-numa`, as `ps`'s
//! reference is built).

use std::cmp::Ordering;
use std::io;
use std::path::{Path, PathBuf};

use super::cvt::{cvt_u64, dbl};
use super::devname::{ABBREV_DEV, ABBREV_PTS, ABBREV_TTY, Devname};
use super::pwcache::Pwcache;
use super::readproc::{
    Fill, Proc, Reader, SMAP_ANON_HUGE_PAGES, SMAP_ANONYMOUS, SMAP_FILE_PMD_MAPPED, SMAP_LAZY_FREE,
    SMAP_LOCKED, SMAP_PRIVATE_CLEAN, SMAP_PRIVATE_DIRTY, SMAP_PRIVATE_HUGETLB, SMAP_PSS,
    SMAP_PSS_ANON, SMAP_PSS_FILE, SMAP_PSS_SHMEM, SMAP_REFERENCED, SMAP_RSS, SMAP_SHARED_CLEAN,
    SMAP_SHARED_DIRTY, SMAP_SHARED_HUGETLB, SMAP_SHMEM_PMD_MAPPED, SMAP_SWAP, SMAP_SWAP_PSS,
};
use super::sysinfo;

/// `enum pids_item`, all of it, in `pids.h`'s order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[allow(missing_docs)]
pub enum Item {
    Noop,
    Extra,
    AddrCodeEnd,
    AddrCodeStart,
    AddrCurrEip,
    AddrCurrEsp,
    AddrStackStart,
    AutogrpId,
    AutogrpNice,
    Cgname,
    Cgroup,
    CgroupV,
    Cmd,
    Cmdline,
    CmdlineV,
    Environ,
    EnvironV,
    Exe,
    ExitSignal,
    Flags,
    FltMaj,
    FltMajC,
    FltMajDelta,
    FltMin,
    FltMinC,
    FltMinDelta,
    IdEgid,
    IdEgroup,
    IdEuid,
    IdEuser,
    IdFgid,
    IdFgroup,
    IdFuid,
    IdFuser,
    IdLogin,
    IdPgrp,
    IdPid,
    IdPpid,
    IdRgid,
    IdRgroup,
    IdRuid,
    IdRuser,
    IdSession,
    IdSgid,
    IdSgroup,
    IdSuid,
    IdSuser,
    IdTgid,
    IdTid,
    IdTpgid,
    IoReadBytes,
    IoReadChars,
    IoReadOps,
    IoWriteBytes,
    IoWriteCbytes,
    IoWriteChars,
    IoWriteOps,
    Lxcname,
    MemCode,
    MemCodePgs,
    MemData,
    MemDataPgs,
    MemRes,
    MemResPgs,
    MemShr,
    MemShrPgs,
    MemVirt,
    MemVirtPgs,
    Nice,
    Nlwp,
    NsCgroup,
    NsIpc,
    NsMnt,
    NsNet,
    NsPid,
    NsTime,
    NsUser,
    NsUts,
    OomAdj,
    OomScore,
    Priority,
    PriorityRt,
    Processor,
    ProcessorNode,
    Rss,
    RssRlim,
    SchedClass,
    SdMach,
    SdOuid,
    SdSeat,
    SdSess,
    SdSlice,
    SdUnit,
    SdUunit,
    Sigblocked,
    Sigcatch,
    Sigignore,
    Signals,
    Sigpending,
    SmapAnonymous,
    SmapHugeAnon,
    SmapHugeFile,
    SmapHugeShmem,
    SmapHugeTlbprv,
    SmapHugeTlbshr,
    SmapLazyFree,
    SmapLocked,
    SmapPrvClean,
    SmapPrvDirty,
    SmapPrvTotal,
    SmapPss,
    SmapPssAnon,
    SmapPssFile,
    SmapPssShmem,
    SmapReferenced,
    SmapRss,
    SmapShrClean,
    SmapShrDirty,
    SmapSwap,
    SmapSwapPss,
    State,
    Supgids,
    Supgroups,
    TicsAll,
    TicsAllC,
    TicsAllDelta,
    TicsBegan,
    TicsBlkio,
    TicsGuest,
    TicsGuestC,
    TicsSystem,
    TicsSystemC,
    TicsUser,
    TicsUserC,
    TimeAll,
    TimeAllC,
    TimeElapsed,
    TimeStart,
    Tty,
    TtyName,
    TtyNumber,
    Utilization,
    UtilizationC,
    VmData,
    VmExe,
    VmLib,
    VmRss,
    VmRssAnon,
    VmRssFile,
    VmRssLocked,
    VmRssShared,
    VmSize,
    VmStack,
    VmSwap,
    VmUsed,
    VsizeBytes,
    WchanName,
}

/// An item's result type, which is also how it sorts (`Item_table`'s
/// `sortfunc`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Noop,
    SCh,
    SInt,
    UInt,
    /// `ul_int` and `ull_int`, which sort alike.
    ULong,
    Real,
    Str,
    StrV,
    StrVers,
}

impl Item {
    /// `Item_table[item].sortfunc`, as a type.
    #[must_use]
    #[allow(clippy::too_many_lines)]
    pub fn kind(self) -> Kind {
        use Item as I;
        match self {
            I::Noop => Kind::Noop,
            I::State => Kind::SCh,
            I::AutogrpId
            | I::AutogrpNice
            | I::ExitSignal
            | I::FltMajDelta
            | I::FltMinDelta
            | I::IdLogin
            | I::IdPgrp
            | I::IdPid
            | I::IdPpid
            | I::IdSession
            | I::IdTgid
            | I::IdTid
            | I::IdTpgid
            | I::Nice
            | I::Nlwp
            | I::OomAdj
            | I::OomScore
            | I::Priority
            | I::PriorityRt
            | I::Processor
            | I::ProcessorNode
            | I::SchedClass
            | I::Tty => Kind::SInt,
            I::IdEgid
            | I::IdEuid
            | I::IdFgid
            | I::IdFuid
            | I::IdRgid
            | I::IdRuid
            | I::IdSgid
            | I::IdSuid
            | I::TicsAllDelta => Kind::UInt,
            I::Extra
            | I::AddrCodeEnd
            | I::AddrCodeStart
            | I::AddrCurrEip
            | I::AddrCurrEsp
            | I::AddrStackStart
            | I::Flags
            | I::FltMaj
            | I::FltMajC
            | I::FltMin
            | I::FltMinC
            | I::IoReadBytes
            | I::IoReadChars
            | I::IoReadOps
            | I::IoWriteBytes
            | I::IoWriteCbytes
            | I::IoWriteChars
            | I::IoWriteOps
            | I::MemCode
            | I::MemCodePgs
            | I::MemData
            | I::MemDataPgs
            | I::MemRes
            | I::MemResPgs
            | I::MemShr
            | I::MemShrPgs
            | I::MemVirt
            | I::MemVirtPgs
            | I::NsCgroup
            | I::NsIpc
            | I::NsMnt
            | I::NsNet
            | I::NsPid
            | I::NsTime
            | I::NsUser
            | I::NsUts
            | I::Rss
            | I::RssRlim
            | I::SmapAnonymous
            | I::SmapHugeAnon
            | I::SmapHugeFile
            | I::SmapHugeShmem
            | I::SmapHugeTlbprv
            | I::SmapHugeTlbshr
            | I::SmapLazyFree
            | I::SmapLocked
            | I::SmapPrvClean
            | I::SmapPrvDirty
            | I::SmapPrvTotal
            | I::SmapPss
            | I::SmapPssAnon
            | I::SmapPssFile
            | I::SmapPssShmem
            | I::SmapReferenced
            | I::SmapRss
            | I::SmapShrClean
            | I::SmapShrDirty
            | I::SmapSwap
            | I::SmapSwapPss
            | I::TicsAll
            | I::TicsAllC
            | I::TicsBegan
            | I::TicsBlkio
            | I::TicsGuest
            | I::TicsGuestC
            | I::TicsSystem
            | I::TicsSystemC
            | I::TicsUser
            | I::TicsUserC
            | I::VmData
            | I::VmExe
            | I::VmLib
            | I::VmRss
            | I::VmRssAnon
            | I::VmRssFile
            | I::VmRssLocked
            | I::VmRssShared
            | I::VmSize
            | I::VmStack
            | I::VmSwap
            | I::VmUsed
            | I::VsizeBytes => Kind::ULong,
            I::TimeAll
            | I::TimeAllC
            | I::TimeElapsed
            | I::TimeStart
            | I::Utilization
            | I::UtilizationC => Kind::Real,
            I::TtyName | I::TtyNumber => Kind::StrVers,
            I::CgroupV | I::CmdlineV | I::EnvironV => Kind::StrV,
            I::Cgname
            | I::Cgroup
            | I::Cmd
            | I::Cmdline
            | I::Environ
            | I::Exe
            | I::IdEgroup
            | I::IdEuser
            | I::IdFgroup
            | I::IdFuser
            | I::IdRgroup
            | I::IdRuser
            | I::IdSgroup
            | I::IdSuser
            | I::Lxcname
            | I::SdMach
            | I::SdOuid
            | I::SdSeat
            | I::SdSess
            | I::SdSlice
            | I::SdUnit
            | I::SdUunit
            | I::Sigblocked
            | I::Sigcatch
            | I::Sigignore
            | I::Signals
            | I::Sigpending
            | I::Supgids
            | I::Supgroups
            | I::WchanName => Kind::Str,
        }
    }

    /// `Item_table[item].oldflags`: which files reading this item needs.
    /// `either` is `f_either`, satisfied by `stat` or `status`.
    #[allow(clippy::too_many_lines)]
    pub fn fill(self, f: &mut Fill, either: &mut bool) {
        use Item as I;
        match self {
            I::Noop
            | I::Extra
            | I::IdEgid
            | I::IdEuid
            | I::IdPid
            | I::IdTgid
            | I::IdTid
            | I::WchanName => {}
            I::AddrCodeEnd
            | I::AddrCodeStart
            | I::AddrCurrEip
            | I::AddrCurrEsp
            | I::AddrStackStart
            | I::ExitSignal
            | I::Flags
            | I::FltMaj
            | I::FltMajC
            | I::FltMajDelta
            | I::FltMin
            | I::FltMinC
            | I::FltMinDelta
            | I::IdPgrp
            | I::IdSession
            | I::IdTpgid
            | I::Nice
            | I::Priority
            | I::PriorityRt
            | I::Processor
            | I::ProcessorNode
            | I::Rss
            | I::RssRlim
            | I::SchedClass
            | I::TicsAll
            | I::TicsAllC
            | I::TicsAllDelta
            | I::TicsBegan
            | I::TicsBlkio
            | I::TicsGuest
            | I::TicsGuestC
            | I::TicsSystem
            | I::TicsSystemC
            | I::TicsUser
            | I::TicsUserC
            | I::TimeAll
            | I::TimeAllC
            | I::TimeElapsed
            | I::TimeStart
            | I::Tty
            | I::TtyName
            | I::TtyNumber
            | I::Utilization
            | I::UtilizationC
            | I::VsizeBytes => f.stat = true,
            I::AutogrpId | I::AutogrpNice => f.autogrp = true,
            I::Cgname | I::Cgroup => f.cgroup = true,
            I::CgroupV => f.cgroup_v = true,
            I::Cmd | I::IdPpid | I::Nlwp | I::State => *either = true,
            I::Cmdline => f.cmdline = true,
            I::CmdlineV => f.cmdline_v = true,
            I::Environ => f.environ = true,
            I::EnvironV => f.environ_v = true,
            I::Exe => f.exe = true,
            I::IdEgroup => f.grp = true,
            I::IdEuser => f.usr = true,
            I::IdFgid
            | I::IdFuid
            | I::IdRgid
            | I::IdRuid
            | I::IdSgid
            | I::IdSuid
            | I::Sigblocked
            | I::Sigcatch
            | I::Sigignore
            | I::Signals
            | I::Sigpending
            | I::Supgids
            | I::VmData
            | I::VmExe
            | I::VmLib
            | I::VmRss
            | I::VmRssAnon
            | I::VmRssFile
            | I::VmRssLocked
            | I::VmRssShared
            | I::VmSize
            | I::VmStack
            | I::VmSwap
            | I::VmUsed => f.status = true,
            I::IdFgroup | I::IdRgroup | I::IdSgroup => {
                f.ogroups = true;
                f.status = true;
            }
            I::IdFuser | I::IdRuser | I::IdSuser => {
                f.ousers = true;
                f.status = true;
            }
            I::Supgroups => {
                f.supgrp = true;
                f.status = true;
            }
            I::IdLogin => f.luid = true,
            I::IoReadBytes
            | I::IoReadChars
            | I::IoReadOps
            | I::IoWriteBytes
            | I::IoWriteCbytes
            | I::IoWriteChars
            | I::IoWriteOps => f.io = true,
            I::Lxcname => f.lxc = true,
            I::MemCode
            | I::MemCodePgs
            | I::MemData
            | I::MemDataPgs
            | I::MemRes
            | I::MemResPgs
            | I::MemShr
            | I::MemShrPgs
            | I::MemVirt
            | I::MemVirtPgs => f.statm = true,
            I::NsCgroup
            | I::NsIpc
            | I::NsMnt
            | I::NsNet
            | I::NsPid
            | I::NsTime
            | I::NsUser
            | I::NsUts => f.ns = true,
            I::OomAdj | I::OomScore => f.oom = true,
            I::SdMach | I::SdOuid | I::SdSeat | I::SdSess | I::SdSlice | I::SdUnit | I::SdUunit => {
                f.systemd = true;
            }
            I::SmapAnonymous
            | I::SmapHugeAnon
            | I::SmapHugeFile
            | I::SmapHugeShmem
            | I::SmapHugeTlbprv
            | I::SmapHugeTlbshr
            | I::SmapLazyFree
            | I::SmapLocked
            | I::SmapPrvClean
            | I::SmapPrvDirty
            | I::SmapPrvTotal
            | I::SmapPss
            | I::SmapPssAnon
            | I::SmapPssFile
            | I::SmapPssShmem
            | I::SmapReferenced
            | I::SmapRss
            | I::SmapShrClean
            | I::SmapShrDirty
            | I::SmapSwap
            | I::SmapSwapPss => f.smaps = true,
        }
    }

    /// `Item_table[item].needhist`: the items computed from the previous
    /// reading.
    #[must_use]
    pub fn needs_history(self) -> bool {
        matches!(
            self,
            Item::FltMajDelta | Item::FltMinDelta | Item::TicsAllDelta
        )
    }

    /// The item's name as `STR_set` and `VEC_set` are given it, for
    /// `[ duplicate … ]`.
    fn enum_name(self) -> &'static str {
        use Item as I;
        match self {
            I::Cgname => "CGNAME",
            I::Cgroup => "CGROUP",
            I::CgroupV => "CGROUP_V",
            I::Cmd => "CMD",
            I::Cmdline => "CMDLINE",
            I::CmdlineV => "CMDLINE_V",
            I::Environ => "ENVIRON",
            I::EnvironV => "ENVIRON_V",
            I::Exe => "EXE",
            I::SdMach => "SD_MACH",
            I::SdOuid => "SD_OUID",
            I::SdSeat => "SD_SEAT",
            I::SdSess => "SD_SESS",
            I::SdSlice => "SD_SLICE",
            I::SdUnit => "SD_UNIT",
            I::SdUunit => "SD_UUNIT",
            I::Supgids => "SUPGIDS",
            I::Supgroups => "SUPGROUPS",
            _ => "",
        }
    }
}

/// `pids_libflags_set`: the files a list of items needs read, and whether
/// any of them needs the history.
#[must_use]
pub fn fill_for(items: &[Item]) -> (Fill, bool) {
    let mut f = Fill::default();
    let mut either = false;
    let mut history = false;
    for item in items {
        item.fill(&mut f, &mut either);
        history |= item.needs_history();
    }
    if either && !f.stat && !f.status {
        f.stat = true;
    }
    (f, history)
}

/// One result: `struct pids_result`'s union, with the member that was set.
#[derive(Clone, Debug, PartialEq)]
pub enum Val {
    /// Never set: every member reads as zero.
    Zero,
    SCh(u8),
    SInt(i32),
    UInt(u32),
    ULong(u64),
    Real(f64),
    /// `None` is a NULL `char *`.
    Str(Option<Vec<u8>>),
    /// `None` is a NULL `char **`.
    StrV(Option<Vec<Vec<u8>>>),
}

impl Val {
    /// The `s_int` member.
    #[must_use]
    pub fn s_int(&self) -> i32 {
        match self {
            Self::SInt(v) => *v,
            _ => 0,
        }
    }

    /// The `s_ch` member, as C's (signed) `char`.
    #[must_use]
    pub fn s_ch(&self) -> u8 {
        match self {
            Self::SCh(v) => *v,
            _ => 0,
        }
    }

    /// The `u_int` member.
    #[must_use]
    pub fn u_int(&self) -> u32 {
        match self {
            Self::UInt(v) => *v,
            _ => 0,
        }
    }

    /// The `ul_int` / `ull_int` member.
    #[must_use]
    pub fn ul(&self) -> u64 {
        match self {
            Self::ULong(v) => *v,
            _ => 0,
        }
    }

    /// The `real` member.
    #[must_use]
    pub fn real(&self) -> f64 {
        match self {
            Self::Real(v) => *v,
            _ => 0.0,
        }
    }

    /// The `str` member: `None` for NULL.
    #[must_use]
    pub fn str(&self) -> Option<&[u8]> {
        match self {
            Self::Str(v) => v.as_deref(),
            _ => None,
        }
    }

    /// The `strv` member: `None` for NULL.
    #[must_use]
    pub fn strv(&self) -> Option<&[Vec<u8>]> {
        match self {
            Self::StrV(v) => v.as_deref(),
            _ => None,
        }
    }
}

/// What the item functions need besides the process: `pids_info`'s
/// `hertz`, `boot_tics` and `pgs2k_shift`, `/proc`, and the terminal namer.
pub struct ItemCtx {
    pub hertz: u64,
    /// `uptime * hertz`, at the time of the read; 0 when the uptime cannot be
    /// read.
    pub boot_tics: u64,
    /// The shift that turns a page count into KiB: 2 for 4 KiB pages.
    pub pgs2k_shift: u32,
    pub devname: Devname,
    pub root: PathBuf,
}

/// `procps_pids_new`'s `pgs2k_shift`: halve the page size until it is a
/// kibibyte, counting.
#[must_use]
pub fn pgs2k_shift(page_size: i64) -> u32 {
    let mut pgsz = page_size;
    let mut shift = 0u32;
    while pgsz > 1024 {
        pgsz >>= 1;
        shift = shift.saturating_add(1);
    }
    shift
}

/// `STR_set`: the field, or `[ duplicate SUPGIDS ]` (the item's name)
/// where it is NULL.
fn str_set(item: Item, v: Option<&Vec<u8>>) -> Val {
    Val::Str(Some(v.cloned().unwrap_or_else(|| {
        format!("[ duplicate {} ]", item.enum_name()).into_bytes()
    })))
}

/// `VEC_set`: the vector, or `vectorize_this_str ("[ duplicate CMDLINE_V ]")`
/// where it is NULL.
fn vec_set(item: Item, v: Option<&Vec<Vec<u8>>>) -> Val {
    Val::StrV(Some(v.cloned().unwrap_or_else(|| {
        vec![format!("[ duplicate {} ]", item.enum_name()).into_bytes()]
    })))
}

/// `REG_set` of a string: the field as it is, NULL included.
fn reg_str(v: Option<&Vec<u8>>) -> Val {
    Val::Str(v.cloned())
}

/// A signal mask string, `DUP_set`'s copy.
fn dup_str(v: &[u8]) -> Val {
    Val::Str(Some(v.to_vec()))
}

/// `CVT_set`: `(long)(pages) << pgs2k_shift`, the page count in KiB.
fn cvt_set(pages: u64, shift: u32) -> Val {
    Val::ULong(pages.checked_shl(shift).unwrap_or(0))
}

/// One of `proc_t`'s `smap_*` fields, by its index in
/// [`super::readproc::SMAPS`].
fn smap(p: &Proc, at: usize) -> Val {
    Val::ULong(p.smap.get(at).copied().unwrap_or(0))
}

/// The item functions (`setDECL(…)`): `item`'s value for process `p`.
///
/// `old` is what the slot held from the previous reading, which the few
/// functions that may assign nothing leave in place: `PIDS_noop` always,
/// `PIDS_TIME_ELAPSED` and the `PIDS_UTILIZATION` pair when the process
/// started at the very tick being read.
#[allow(clippy::too_many_lines)]
pub fn value(item: Item, p: &Proc, ctx: &mut ItemCtx, old: Option<&Val>) -> Val {
    use Item as I;
    let keep = || old.cloned().unwrap_or(Val::Zero);
    // `double t = I->boot_tics - P->start_time`: an unsigned subtraction,
    // which wraps, turned into a `double`.
    let tics_since = |start: u64| dbl(ctx.boot_tics.wrapping_sub(start));
    match item {
        I::Noop => keep(),
        I::Extra => Val::ULong(0),
        I::AddrCodeEnd => Val::ULong(p.end_code),
        I::AddrCodeStart => Val::ULong(p.start_code),
        I::AddrCurrEip => Val::ULong(p.kstk_eip),
        I::AddrCurrEsp => Val::ULong(p.kstk_esp),
        I::AddrStackStart => Val::ULong(p.start_stack),
        I::AutogrpId => Val::SInt(p.autogrp_id),
        I::AutogrpNice => Val::SInt(p.autogrp_nice),
        I::Cgname => str_set(item, p.cgname.as_ref()),
        I::Cgroup => str_set(item, p.cgroup.as_ref()),
        I::CgroupV => vec_set(item, p.cgroup_v.as_ref()),
        I::Cmd => str_set(item, p.cmd.as_ref()),
        I::Cmdline => str_set(item, p.cmdline.as_ref()),
        I::CmdlineV => vec_set(item, p.cmdline_v.as_ref()),
        I::Environ => str_set(item, p.environ.as_ref()),
        I::EnvironV => vec_set(item, p.environ_v.as_ref()),
        I::Exe => str_set(item, p.exe.as_ref()),
        I::ExitSignal => Val::SInt(p.exit_signal),
        I::Flags => Val::ULong(p.flags),
        I::FltMaj => Val::ULong(p.maj_flt),
        I::FltMajC => Val::ULong(p.maj_flt.wrapping_add(p.cmaj_flt)),
        I::FltMajDelta => Val::SInt(p.maj_delta),
        I::FltMin => Val::ULong(p.min_flt),
        I::FltMinC => Val::ULong(p.min_flt.wrapping_add(p.cmin_flt)),
        I::FltMinDelta => Val::SInt(p.min_delta),
        I::IdEgid => Val::UInt(p.egid),
        I::IdEgroup => reg_str(p.egroup.as_ref()),
        I::IdEuid => Val::UInt(p.euid),
        I::IdEuser => reg_str(p.euser.as_ref()),
        I::IdFgid => Val::UInt(p.fgid),
        I::IdFgroup => reg_str(p.fgroup.as_ref()),
        I::IdFuid => Val::UInt(p.fuid),
        I::IdFuser => reg_str(p.fuser.as_ref()),
        I::IdLogin => Val::SInt(p.luid),
        I::IdPgrp => Val::SInt(p.pgrp),
        I::IdPid | I::IdTid => Val::SInt(p.tid),
        I::IdPpid => Val::SInt(p.ppid),
        I::IdRgid => Val::UInt(p.rgid),
        I::IdRgroup => reg_str(p.rgroup.as_ref()),
        I::IdRuid => Val::UInt(p.ruid),
        I::IdRuser => reg_str(p.ruser.as_ref()),
        I::IdSession => Val::SInt(p.session),
        I::IdSgid => Val::UInt(p.sgid),
        I::IdSgroup => reg_str(p.sgroup.as_ref()),
        I::IdSuid => Val::UInt(p.suid),
        I::IdSuser => reg_str(p.suser.as_ref()),
        I::IdTgid => Val::SInt(p.tgid),
        I::IdTpgid => Val::SInt(p.tpgid),
        I::IoReadBytes => Val::ULong(p.read_bytes),
        I::IoReadChars => Val::ULong(p.rchar),
        I::IoReadOps => Val::ULong(p.syscr),
        I::IoWriteBytes => Val::ULong(p.write_bytes),
        I::IoWriteCbytes => Val::ULong(p.cancelled_write_bytes),
        I::IoWriteChars => Val::ULong(p.wchar),
        I::IoWriteOps => Val::ULong(p.syscw),
        I::Lxcname => reg_str(p.lxcname.as_ref()),
        I::MemCode => cvt_set(p.trs, ctx.pgs2k_shift),
        I::MemCodePgs => Val::ULong(p.trs),
        I::MemData => cvt_set(p.drs, ctx.pgs2k_shift),
        I::MemDataPgs => Val::ULong(p.drs),
        I::MemRes => cvt_set(p.resident, ctx.pgs2k_shift),
        I::MemResPgs => Val::ULong(p.resident),
        I::MemShr => cvt_set(p.share, ctx.pgs2k_shift),
        I::MemShrPgs => Val::ULong(p.share),
        I::MemVirt => cvt_set(p.size, ctx.pgs2k_shift),
        I::MemVirtPgs => Val::ULong(p.size),
        I::Nice => Val::SInt(p.nice),
        I::Nlwp => Val::SInt(p.nlwp),
        I::NsCgroup => Val::ULong(p.ns[0]),
        I::NsIpc => Val::ULong(p.ns[1]),
        I::NsMnt => Val::ULong(p.ns[2]),
        I::NsNet => Val::ULong(p.ns[3]),
        I::NsPid => Val::ULong(p.ns[4]),
        I::NsTime => Val::ULong(p.ns[5]),
        I::NsUser => Val::ULong(p.ns[6]),
        I::NsUts => Val::ULong(p.ns[7]),
        I::OomAdj => Val::SInt(p.oom_adj),
        I::OomScore => Val::SInt(p.oom_score),
        I::Priority => Val::SInt(p.priority),
        I::PriorityRt => Val::SInt(p.rtprio),
        I::Processor => Val::SInt(p.processor),
        // `numa_node_of_cpu` without libnuma.
        I::ProcessorNode => Val::SInt(-1),
        I::Rss => Val::ULong(p.rss),
        I::RssRlim => Val::ULong(p.rss_rlim),
        I::SchedClass => Val::SInt(p.sched),
        I::SdMach | I::SdOuid | I::SdSeat | I::SdSess | I::SdSlice | I::SdUnit | I::SdUunit => {
            str_set(item, p.sd.as_ref())
        }
        I::Sigblocked => dup_str(&p.blocked),
        I::Sigcatch => dup_str(&p.sigcatch),
        I::Sigignore => dup_str(&p.sigignore),
        I::Signals => dup_str(&p.signal),
        I::Sigpending => dup_str(&p.sigpnd),
        I::SmapAnonymous => smap(p, SMAP_ANONYMOUS),
        I::SmapHugeAnon => smap(p, SMAP_ANON_HUGE_PAGES),
        I::SmapHugeFile => smap(p, SMAP_FILE_PMD_MAPPED),
        I::SmapHugeShmem => smap(p, SMAP_SHMEM_PMD_MAPPED),
        I::SmapHugeTlbprv => smap(p, SMAP_PRIVATE_HUGETLB),
        I::SmapHugeTlbshr => smap(p, SMAP_SHARED_HUGETLB),
        I::SmapLazyFree => smap(p, SMAP_LAZY_FREE),
        I::SmapLocked => smap(p, SMAP_LOCKED),
        I::SmapPrvClean => smap(p, SMAP_PRIVATE_CLEAN),
        I::SmapPrvDirty => smap(p, SMAP_PRIVATE_DIRTY),
        I::SmapPrvTotal => Val::ULong(
            smap(p, SMAP_PRIVATE_CLEAN)
                .ul()
                .wrapping_add(smap(p, SMAP_PRIVATE_DIRTY).ul()),
        ),
        I::SmapPss => smap(p, SMAP_PSS),
        I::SmapPssAnon => smap(p, SMAP_PSS_ANON),
        I::SmapPssFile => smap(p, SMAP_PSS_FILE),
        I::SmapPssShmem => smap(p, SMAP_PSS_SHMEM),
        I::SmapReferenced => smap(p, SMAP_REFERENCED),
        I::SmapRss => smap(p, SMAP_RSS),
        I::SmapShrClean => smap(p, SMAP_SHARED_CLEAN),
        I::SmapShrDirty => smap(p, SMAP_SHARED_DIRTY),
        I::SmapSwap => smap(p, SMAP_SWAP),
        I::SmapSwapPss => smap(p, SMAP_SWAP_PSS),
        I::State => Val::SCh(p.state),
        I::Supgids => str_set(item, p.supgid.as_ref()),
        I::Supgroups => str_set(item, p.supgrp.as_ref()),
        I::TicsAll => Val::ULong(p.utime.wrapping_add(p.stime)),
        I::TicsAllC => Val::ULong(
            p.utime
                .wrapping_add(p.stime)
                .wrapping_add(p.cutime)
                .wrapping_add(p.cstime),
        ),
        // `REG_set(TICS_ALL_DELTA, u_int, pcpu)`: the `int` taken as unsigned.
        I::TicsAllDelta => Val::UInt(p.pcpu.cast_unsigned()),
        I::TicsBegan => Val::ULong(p.start_time),
        I::TicsBlkio => Val::ULong(p.blkio_tics),
        I::TicsGuest => Val::ULong(p.gtime),
        I::TicsGuestC => Val::ULong(p.gtime.wrapping_add(p.cgtime)),
        I::TicsSystem => Val::ULong(p.stime),
        I::TicsSystemC => Val::ULong(p.stime.wrapping_add(p.cstime)),
        I::TicsUser => Val::ULong(p.utime),
        I::TicsUserC => Val::ULong(p.utime.wrapping_add(p.cutime)),
        I::TimeAll => Val::Real((dbl(p.utime) + dbl(p.stime)) / dbl(ctx.hertz)),
        I::TimeAllC => Val::Real(
            (dbl(p.utime) + dbl(p.stime) + dbl(p.cutime) + dbl(p.cstime)) / dbl(ctx.hertz),
        ),
        I::TimeElapsed => {
            let t = tics_since(p.start_time);
            if t > 0.0 {
                Val::Real(t / dbl(ctx.hertz))
            } else {
                keep()
            }
        }
        I::TimeStart => Val::Real(dbl(p.start_time) / dbl(ctx.hertz)),
        I::Tty => Val::SInt(p.tty),
        I::TtyName => Val::Str(Some(ctx.devname.dev_to_tty(
            64,
            p.tty.cast_unsigned(),
            p.tid,
            ABBREV_DEV,
        ))),
        I::TtyNumber => Val::Str(Some(ctx.devname.dev_to_tty(
            64,
            p.tty.cast_unsigned(),
            p.tid,
            ABBREV_DEV | ABBREV_TTY | ABBREV_PTS,
        ))),
        I::Utilization | I::UtilizationC => {
            let t = tics_since(p.start_time);
            if t > 0.0 {
                let mut used = p.utime.wrapping_add(p.stime);
                if item == I::UtilizationC {
                    used = used.wrapping_add(p.cutime).wrapping_add(p.cstime);
                }
                // `(… ) * 100.0f`: an unsigned long long times a float is a
                // float, so the product is rounded to single precision.
                #[allow(clippy::cast_precision_loss)]
                let product = (used as f32) * 100.0f32;
                Val::Real(f64::from(product) / t)
            } else {
                keep()
            }
        }
        I::VmData => Val::ULong(p.vm_data),
        I::VmExe => Val::ULong(p.vm_exe),
        I::VmLib => Val::ULong(p.vm_lib),
        I::VmRss => Val::ULong(p.vm_rss),
        I::VmRssAnon => Val::ULong(p.vm_rss_anon),
        I::VmRssFile => Val::ULong(p.vm_rss_file),
        I::VmRssLocked => Val::ULong(p.vm_lock),
        I::VmRssShared => Val::ULong(p.vm_rss_shared),
        I::VmSize => Val::ULong(p.vm_size),
        I::VmStack => Val::ULong(p.vm_stack),
        I::VmSwap => Val::ULong(p.vm_swap),
        I::VmUsed => Val::ULong(p.vm_swap.wrapping_add(p.vm_rss)),
        I::VsizeBytes => Val::ULong(p.vsize),
        I::WchanName => Val::Str(Some(sysinfo::lookup_wchan(&ctx.root, p.tid))),
    }
}

/// One process's stack: a value per item, in the items' order.
pub type Stack = Vec<Val>;

/// `pids_assign_results` into a fresh stack: `p`'s value for each of
/// `items`, as the first reading into a newly allocated stack gives them.
#[must_use]
pub fn assign(items: &[Item], p: &Proc, ctx: &mut ItemCtx) -> Stack {
    items
        .iter()
        .map(|&item| value(item, p, ctx, None))
        .collect()
}

/// `PIDS_SORT_ASCEND` and `PIDS_SORT_DESCEND`; a sort node can also carry 0,
/// which `procps_pids_sort` refuses.
pub const ASCEND: i32 = 1;
/// See [`ASCEND`].
pub const DESCEND: i32 = -1;

/// glibc's `strverscmp`: strings compared with their runs of digits as
/// numbers, and leading zeros as fractional parts.
#[must_use]
pub fn strverscmp(s1: &[u8], s2: &[u8]) -> i32 {
    // States: S_N normal, S_I comparing integral part, S_F fractional,
    // S_Z idem but with leading zeros only.
    const S_N: usize = 0x0;
    const S_I: usize = 0x3;
    const S_F: usize = 0x6;
    const S_Z: usize = 0x9;
    const CMP: i32 = 2;
    const LEN: i32 = 3;
    const NEXT_STATE: [usize; 12] = [
        /* S_N */ S_N, S_I, S_Z, /* S_I */ S_N, S_I, S_I, /* S_F */ S_N, S_F, S_F,
        /* S_Z */ S_N, S_F, S_Z,
    ];
    const RESULT_TYPE: [i32; 36] = [
        /* S_N */ CMP, CMP, CMP, CMP, LEN, CMP, CMP, CMP, CMP, /* S_I */ CMP, -1, -1, 1,
        LEN, LEN, 1, LEN, LEN, /* S_F */ CMP, CMP, CMP, CMP, CMP, CMP, CMP, CMP, CMP,
        /* S_Z */ CMP, 1, 1, -1, CMP, CMP, -1, CMP, CMP,
    ];
    let at = |s: &[u8], i: usize| s.get(i).copied().unwrap_or(0);
    let class = |c: u8| match c {
        b'0' => 2usize,
        b'1'..=b'9' => 1,
        _ => 0,
    };
    if s1 == s2 {
        return 0;
    }
    let (mut i, mut j) = (0usize, 0usize);
    let mut c1 = at(s1, i);
    i = i.saturating_add(1);
    let mut c2 = at(s2, j);
    j = j.saturating_add(1);
    let mut state = S_N + class(c1);
    let mut diff = i32::from(c1).wrapping_sub(i32::from(c2));
    while diff == 0 {
        if c1 == 0 {
            return diff;
        }
        state = NEXT_STATE.get(state).copied().unwrap_or(S_N);
        c1 = at(s1, i);
        i = i.saturating_add(1);
        c2 = at(s2, j);
        j = j.saturating_add(1);
        state = state.saturating_add(class(c1));
        diff = i32::from(c1).wrapping_sub(i32::from(c2));
    }
    let rt = RESULT_TYPE
        .get(state.saturating_mul(3).saturating_add(class(c2)))
        .copied()
        .unwrap_or(CMP);
    match rt {
        CMP => diff,
        LEN => loop {
            let a = at(s1, i);
            i = i.saturating_add(1);
            let b = at(s2, j);
            j = j.saturating_add(1);
            if !a.is_ascii_digit() {
                return if b.is_ascii_digit() { -1 } else { diff };
            }
            if !b.is_ascii_digit() {
                return 1;
            }
        },
        other => other,
    }
}

/// The comparison `Item_table`'s `sortfunc` makes on one slot, with C's
/// arithmetic: `NUM_srt` subtracts (and can overflow), `REG_srt` compares.
fn compare(kind: Kind, a: &Val, b: &Val, order: i32) -> i32 {
    let reg = |o: Option<Ordering>| match o {
        Some(Ordering::Greater) => {
            if order > 0 {
                1
            } else {
                -1
            }
        }
        Some(Ordering::Less) => {
            if order > 0 {
                -1
            } else {
                1
            }
        }
        _ => 0,
    };
    // `strcoll` in the C.UTF-8 locale this is measured in is `strcmp`.
    let coll = |x: &[u8], y: &[u8]| match x.cmp(y) {
        Ordering::Less => -1,
        Ordering::Equal => 0,
        Ordering::Greater => 1,
    };
    match kind {
        Kind::Noop => 0,
        Kind::SCh => {
            let (x, y) = (
                i32::from(a.s_ch().cast_signed()),
                i32::from(b.s_ch().cast_signed()),
            );
            order.wrapping_mul(x.wrapping_sub(y))
        }
        Kind::SInt => order.wrapping_mul(a.s_int().wrapping_sub(b.s_int())),
        Kind::UInt => reg(a.u_int().partial_cmp(&b.u_int())),
        Kind::ULong => reg(a.ul().partial_cmp(&b.ul())),
        Kind::Real => reg(a.real().partial_cmp(&b.real())),
        Kind::Str => order.wrapping_mul(coll(
            a.str().unwrap_or_default(),
            b.str().unwrap_or_default(),
        )),
        // `if (!a->result.strv || !b->result.strv) return 0;` -- then the
        // first strings.
        Kind::StrV => match (a.strv(), b.strv()) {
            (Some(x), Some(y)) => order.wrapping_mul(coll(
                x.first().map_or(&[][..], Vec::as_slice),
                y.first().map_or(&[][..], Vec::as_slice),
            )),
            _ => 0,
        },
        Kind::StrVers => order.wrapping_mul(strverscmp(
            a.str().unwrap_or_default(),
            b.str().unwrap_or_default(),
        )),
    }
}

/// glibc's `msort_with_tmp`: a top-down merge sort, the left half `n/2`
/// long, taking from the left run while `cmp <= 0`. Ported rather than
/// replaced by `sort_by` because the comparisons above are not always
/// consistent -- `NUM_srt` overflows -- and then the answer depends on the
/// algorithm.
pub fn msort<T: Clone>(v: &mut [T], cmp: &dyn Fn(&T, &T) -> i32) {
    let n = v.len();
    if n <= 1 {
        return;
    }
    let n1 = n / 2;
    let (left, right) = v.split_at_mut(n1);
    msort(left, cmp);
    msort(right, cmp);
    let mut tmp: Vec<T> = Vec::with_capacity(n);
    let (mut i, mut j) = (0usize, 0usize);
    while let (Some(a), Some(b)) = (left.get(i), right.get(j)) {
        if cmp(a, b) <= 0 {
            tmp.push(a.clone());
            i = i.saturating_add(1);
        } else {
            tmp.push(b.clone());
            j = j.saturating_add(1);
        }
    }
    tmp.extend_from_slice(left.get(i..).unwrap_or_default());
    tmp.extend_from_slice(right.get(j..).unwrap_or_default());
    v.clone_from_slice(&tmp);
}

/// `procps_pids_sort`: the stacks named by `idx` (indices into `stacks`,
/// each stack holding `items`' values) sorted on `item`.
///
/// Returns false, leaving `idx` alone, where upstream returns NULL: `item`
/// has no slot, or `dir` is neither [`ASCEND`] nor [`DESCEND`]. Fewer than
/// two stacks are already sorted.
pub fn sort(items: &[Item], stacks: &[Stack], idx: &mut [usize], item: Item, dir: i32) -> bool {
    if dir != ASCEND && dir != DESCEND {
        return false;
    }
    if idx.len() < 2 {
        return true;
    }
    let Some(at) = items.iter().position(|&i| i == item) else {
        return false;
    };
    let kind = item.kind();
    let get = |k: &usize| {
        stacks
            .get(*k)
            .and_then(|s| s.get(at))
            .cloned()
            .unwrap_or(Val::Zero)
    };
    msort(idx, &|a: &usize, b: &usize| {
        compare(kind, &get(a), &get(b), dir)
    });
    true
}

/// `HHASH_SIZE`: the history's hash buckets.
const HHASH_SIZE: usize = 4096;

/// `HST_t`: one task's ticks and faults as the last reading saw them.
#[derive(Clone, Copy, Debug, Default)]
struct Hst {
    tics: u64,
    maj: u64,
    min: u64,
    pid: i32,
    /// The next entry on this bucket's chain, or -1.
    lnk: i32,
}

/// `struct history_info`: the previous reading's tasks, and the one being
/// made, each with its own hash table, swapped at every reading.
pub struct History {
    sav: Vec<Hst>,
    new: Vec<Hst>,
    hash_sav: Vec<i32>,
    hash_new: Vec<i32>,
}

impl Default for History {
    fn default() -> Self {
        Self::new()
    }
}

/// The low 32 bits of a 64-bit count, as C converts to `int`.
fn low32(x: u64) -> i32 {
    let b = x.to_le_bytes();
    i32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

impl History {
    /// `pids_config_history`: both tables empty.
    #[must_use]
    pub fn new() -> Self {
        Self {
            sav: Vec::new(),
            new: Vec::new(),
            hash_sav: vec![-1; HHASH_SIZE],
            hash_new: vec![-1; HHASH_SIZE],
        }
    }

    /// `_HASH_PID_`.
    fn bucket(pid: i32) -> usize {
        usize::try_from(pid.cast_unsigned()).unwrap_or(0) & (HHASH_SIZE - 1)
    }

    /// `pids_toggle_history`: what was being made is now the previous
    /// reading, and a fresh one begins.
    pub fn toggle(&mut self) {
        std::mem::swap(&mut self.sav, &mut self.new);
        std::mem::swap(&mut self.hash_sav, &mut self.hash_new);
        self.hash_new.fill(-1);
        self.new.clear();
    }

    /// `pids_histget`: the previous reading's entry for `pid`.
    fn get(&self, pid: i32) -> Option<Hst> {
        let mut v = self.hash_sav.get(Self::bucket(pid)).copied().unwrap_or(-1);
        while let Ok(at) = usize::try_from(v) {
            let h = *self.sav.get(at)?;
            if h.pid == pid {
                return Some(h);
            }
            v = h.lnk;
        }
        None
    }

    /// `pids_make_hist`: record `p` in this reading, and give it the change
    /// since the previous one -- its whole ticks if it was not in it.
    pub fn make(&mut self, p: &mut Proc) {
        let tics = p.utime.wrapping_add(p.stime);
        let slot = i32::try_from(self.new.len()).unwrap_or(i32::MAX);
        let bucket = Self::bucket(p.tid);
        let lnk = self.hash_new.get(bucket).copied().unwrap_or(-1);
        self.new.push(Hst {
            tics,
            maj: p.maj_flt,
            min: p.min_flt,
            pid: p.tid,
            lnk,
        });
        if let Some(b) = self.hash_new.get_mut(bucket) {
            *b = slot;
        }
        let mut elapsed = tics;
        if let Some(h) = self.get(p.tid) {
            elapsed = tics.wrapping_sub(h.tics);
            p.maj_delta = low32(p.maj_flt.wrapping_sub(h.maj));
            p.min_delta = low32(p.min_flt.wrapping_sub(h.min));
        }
        p.pcpu = low32(elapsed);
    }
}

/// `struct pids_counts`: how many tasks a reading found, by state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub total: i32,
    pub running: i32,
    pub sleeping: i32,
    pub stopped: i32,
    pub zombied: i32,
    pub other: i32,
}

impl Counts {
    /// `pids_proc_tally`'s count: `R` running; `D` and `S` sleeping; `t` and
    /// `T` stopped; `Z` zombied; anything else (`I`, `P`, `X`) other.
    pub fn tally(&mut self, state: u8) {
        let slot = match state {
            b'R' => &mut self.running,
            b'D' | b'S' => &mut self.sleeping,
            b't' | b'T' => &mut self.stopped,
            b'Z' => &mut self.zombied,
            _ => &mut self.other,
        };
        *slot = slot.saturating_add(1);
        self.total = self.total.saturating_add(1);
    }
}

/// Which tasks a reading walks: `PIDS_FETCH_TASKS_ONLY` or
/// `PIDS_FETCH_THREADS_TOO`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fetch {
    TasksOnly,
    ThreadsToo,
}

/// `struct pids_info`: one program's items, its stacks and its history,
/// kept from reading to reading.
pub struct Pids {
    items: Vec<Item>,
    fill: Fill,
    history_yes: bool,
    history: History,
    ctx: ItemCtx,
    utf8: bool,
    pw: Option<Pwcache>,
    /// Every stack ever handed out, the first `n` of them the last reading's.
    stacks: Vec<Stack>,
    n: usize,
    counts: Counts,
}

impl Pids {
    /// `procps_pids_new`: a context over `root` (`/proc`) for `items`.
    /// `utf8` is whether names are escaped as UTF-8, as the locale says.
    #[must_use]
    pub fn new(root: &Path, items: &[Item], hertz: u64, page_size: i64, utf8: bool) -> Self {
        let (fill, history_yes) = fill_for(items);
        Self {
            items: items.to_vec(),
            fill,
            history_yes,
            history: History::new(),
            ctx: ItemCtx {
                hertz,
                boot_tics: 0,
                pgs2k_shift: pgs2k_shift(page_size),
                devname: Devname::new(root.to_path_buf()),
                root: root.to_path_buf(),
            },
            utf8,
            pw: None,
            stacks: Vec::new(),
            n: 0,
            counts: Counts::default(),
        }
    }

    /// `procps_pids_reset`: new items. Every slot of every stack is zeroed
    /// first, whether or not the items changed (`pids_cleanup_stacks_all`).
    pub fn reset(&mut self, items: &[Item]) {
        for stack in &mut self.stacks {
            stack.clear();
        }
        if self.items != items {
            self.items = items.to_vec();
            (self.fill, self.history_yes) = fill_for(items);
        }
    }

    /// The items, in their slots' order.
    #[must_use]
    pub fn items(&self) -> &[Item] {
        &self.items
    }

    /// The slot `item` reads from, if it was asked for.
    #[must_use]
    pub fn slot(&self, item: Item) -> Option<usize> {
        self.items.iter().position(|&i| i == item)
    }

    /// The last reading's stacks.
    #[must_use]
    pub fn stacks(&self) -> &[Stack] {
        self.stacks.get(..self.n).unwrap_or_default()
    }

    /// Stack `i` of every stack ever handed out, whether or not the last
    /// reading used it -- what a caller holding a pointer past the end of
    /// `stacks` (`top`'s windows keep such) reads: an earlier reading's
    /// values.
    #[must_use]
    pub fn stack_any(&self, i: usize) -> Option<&Stack> {
        self.stacks.get(i)
    }

    /// Stack `i`, to write its `PIDS_extra` slots.
    pub fn stack_any_mut(&mut self, i: usize) -> Option<&mut Stack> {
        self.stacks.get_mut(i)
    }

    /// The last reading's stacks, to write the `PIDS_extra` slots of.
    pub fn stacks_mut(&mut self) -> &mut [Stack] {
        let n = self.n;
        self.stacks.get_mut(..n).unwrap_or_default()
    }

    /// The last reading's counts.
    #[must_use]
    pub fn counts(&self) -> Counts {
        self.counts
    }

    /// `procps_pids_reap`: every task (or thread). `None` where upstream
    /// returns NULL: `/proc` could not be listed, or it held nothing.
    pub fn reap(&mut self, fetch: Fetch) -> Option<Counts> {
        let procs = self.read(fetch, None).ok()?;
        self.fetch(procs);
        (self.counts.total > 0).then_some(self.counts)
    }

    /// `procps_pids_select (…, PIDS_SELECT_PID)`: the tasks listed (or their
    /// threads). Finding none is not an error.
    pub fn select(&mut self, pids: &[u32], fetch: Fetch) -> Option<Counts> {
        let procs = self.read(fetch, Some(pids)).ok()?;
        self.fetch(procs);
        Some(self.counts)
    }

    /// `procps_pids_sort` over the last reading: `idx` holds indices into
    /// [`Self::stacks`].
    pub fn sort(&self, idx: &mut [usize], item: Item, dir: i32) -> bool {
        sort(&self.items, self.stacks(), idx, item, dir)
    }

    /// The `boot_tics` of a reading, and the walk itself.
    fn read(&mut self, fetch: Fetch, pids: Option<&[u32]>) -> io::Result<Vec<Proc>> {
        // `if (0 >= procps_uptime(&up_secs, NULL)) boot_tics = up_secs * hertz`.
        let up = sysinfo::uptime(&self.ctx.root);
        self.ctx.boot_tics = cvt_u64(up * dbl(self.ctx.hertz));
        let pw = self.pw.take().unwrap_or_else(Pwcache::load);
        let mut reader = Reader::new(self.ctx.root.clone(), self.fill, self.utf8, pw);
        let procs = match (fetch, pids) {
            (Fetch::ThreadsToo, _) => reader.reap_threads(pids),
            (Fetch::TasksOnly, Some(list)) => Ok(reader.select(list)),
            (Fetch::TasksOnly, None) => reader.reap(),
        };
        self.pw = Some(reader.pw);
        procs
    }

    /// `pids_stacks_fetch`: the history toggled, the counts kept, and each
    /// task's values assigned into the stack at its position.
    fn fetch(&mut self, procs: Vec<Proc>) {
        self.history.toggle();
        self.counts = Counts::default();
        let n = procs.len();
        if self.stacks.len() < n {
            self.stacks.resize_with(n, Vec::new);
        }
        for (stack, mut p) in self.stacks.iter_mut().zip(procs) {
            self.counts.tally(p.state);
            if self.history_yes {
                self.history.make(&mut p);
            }
            let old = std::mem::take(stack);
            *stack = self
                .items
                .iter()
                .enumerate()
                .map(|(i, &item)| value(item, &p, &mut self.ctx, old.get(i)))
                .collect();
        }
        self.n = n;
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn strverscmp_orders_as_glibc() {
        let lt = |a: &str, b: &str| strverscmp(a.as_bytes(), b.as_bytes()) < 0;
        assert!(lt("pts/2", "pts/10"));
        assert!(lt("tty2", "tty10"));
        assert!(lt("000", "00"));
        assert!(lt("00", "01"));
        assert!(lt("01", "010"));
        assert!(lt("010", "09"));
        assert!(lt("09", "0"));
        assert!(lt("0", "1"));
        assert!(lt("1", "9"));
        assert!(lt("9", "10"));
        assert_eq!(strverscmp(b"abc", b"abc"), 0);
        assert!(lt("?", "pts/0"));
    }

    #[test]
    fn a_sort_on_an_unregistered_item_changes_nothing() {
        let items = [Item::IdPid];
        let stacks = vec![vec![Val::SInt(3)], vec![Val::SInt(1)], vec![Val::SInt(2)]];
        let mut idx = vec![0, 1, 2];
        assert!(!sort(&items, &stacks, &mut idx, Item::TicsUser, ASCEND));
        assert_eq!(idx, [0, 1, 2]);
        assert!(!sort(&items, &stacks, &mut idx, Item::IdPid, 0));
        assert_eq!(idx, [0, 1, 2]);
        assert!(sort(&items, &stacks, &mut idx, Item::IdPid, ASCEND));
        assert_eq!(idx, [1, 2, 0]);
        assert!(sort(&items, &stacks, &mut idx, Item::IdPid, DESCEND));
        assert_eq!(idx, [0, 2, 1]);
    }

    #[test]
    fn int_sorts_overflow_as_c_does() {
        // INT_MIN - 1 wraps to INT_MAX: upstream sorts INT_MIN *after* 1.
        assert!(compare(Kind::SInt, &Val::SInt(i32::MIN), &Val::SInt(1), ASCEND) > 0);
        assert!(compare(Kind::SInt, &Val::SInt(-1), &Val::SInt(1), ASCEND) < 0);
    }

    #[test]
    fn the_merge_sort_is_stable() {
        let mut v = vec![(1, 'a'), (0, 'b'), (1, 'c'), (0, 'd')];
        msort(&mut v, &|x: &(i32, char), y: &(i32, char)| x.0 - y.0);
        assert_eq!(v, [(0, 'b'), (0, 'd'), (1, 'a'), (1, 'c')]);
    }

    #[test]
    fn a_string_vector_sorts_on_its_first_string_and_a_null_one_ties() {
        let a = Val::StrV(Some(vec![b"b".to_vec(), b"a".to_vec()]));
        let b = Val::StrV(Some(vec![b"a".to_vec(), b"z".to_vec()]));
        assert_eq!(compare(Kind::StrV, &a, &b, ASCEND), 1);
        assert_eq!(compare(Kind::StrV, &a, &Val::StrV(None), ASCEND), 0);
    }

    #[test]
    fn page_counts_become_kibibytes_by_the_page_size() {
        assert_eq!(pgs2k_shift(4096), 2);
        assert_eq!(pgs2k_shift(16384), 4);
        assert_eq!(pgs2k_shift(1024), 0);
        assert_eq!(cvt_set(3, 2), Val::ULong(12));
    }

    fn proc_with(tid: i32, utime: u64, maj: u64) -> Proc {
        Proc {
            tid,
            utime,
            stime: 5,
            maj_flt: maj,
            min_flt: maj.saturating_mul(2),
            ..Proc::default()
        }
    }

    #[test]
    fn the_history_gives_a_task_its_change_and_a_new_one_its_whole_ticks() {
        let mut h = History::new();
        h.toggle();
        let mut p = proc_with(42, 100, 7);
        h.make(&mut p);
        assert_eq!((p.pcpu, p.maj_delta, p.min_delta), (105, 0, 0));
        h.toggle();
        let mut p = proc_with(42, 130, 9);
        h.make(&mut p);
        assert_eq!((p.pcpu, p.maj_delta, p.min_delta), (30, 2, 4));
        // Same bucket, different task: not confused with 42.
        let mut q = proc_with(42 + 4096, 50, 1);
        h.make(&mut q);
        assert_eq!(q.pcpu, 55);
    }

    #[test]
    fn a_task_gone_for_one_reading_counts_from_scratch() {
        let mut h = History::new();
        h.toggle();
        h.make(&mut proc_with(1, 10, 0));
        h.toggle();
        h.toggle();
        let mut p = proc_with(1, 20, 0);
        h.make(&mut p);
        assert_eq!(p.pcpu, 25);
    }

    #[test]
    fn states_are_counted_as_the_library_counts_them() {
        let mut c = Counts::default();
        for s in *b"RSDtTZIX" {
            c.tally(s);
        }
        assert_eq!(
            c,
            Counts {
                total: 8,
                running: 1,
                sleeping: 2,
                stopped: 2,
                zombied: 1,
                other: 2,
            }
        );
    }

    fn ctx() -> ItemCtx {
        ItemCtx {
            hertz: 100,
            boot_tics: 1000,
            pgs2k_shift: 2,
            devname: Devname::new(PathBuf::from("/nonexistent-proc")),
            root: PathBuf::from("/nonexistent-proc"),
        }
    }

    #[test]
    fn elapsed_time_keeps_the_old_value_for_a_task_started_this_tick() {
        let mut c = ctx();
        let p = Proc {
            start_time: 1000,
            ..Proc::default()
        };
        let old = Val::Real(7.5);
        assert_eq!(value(Item::TimeElapsed, &p, &mut c, Some(&old)), old);
        assert_eq!(value(Item::TimeElapsed, &p, &mut c, None), Val::Zero);
        let q = Proc {
            start_time: 800,
            ..Proc::default()
        };
        assert_eq!(
            value(Item::TimeElapsed, &q, &mut c, Some(&old)),
            Val::Real(2.0)
        );
    }

    #[test]
    fn null_strings_and_vectors_say_which_item_was_duplicated() {
        let mut c = ctx();
        let p = Proc::default();
        assert_eq!(
            value(Item::Cmdline, &p, &mut c, None),
            Val::Str(Some(b"[ duplicate CMDLINE ]".to_vec()))
        );
        assert_eq!(
            value(Item::EnvironV, &p, &mut c, None),
            Val::StrV(Some(vec![b"[ duplicate ENVIRON_V ]".to_vec()]))
        );
        assert_eq!(value(Item::IdEuser, &p, &mut c, None), Val::Str(None));
    }

    #[test]
    fn memory_items_are_statm_pages_shifted_and_the_tick_change_is_unsigned() {
        let mut c = ctx();
        let p = Proc {
            size: 10,
            resident: 4,
            share: 1,
            vm_rss: 16,
            vm_swap: 3,
            pcpu: -1,
            ..Proc::default()
        };
        assert_eq!(value(Item::MemVirt, &p, &mut c, None), Val::ULong(40));
        assert_eq!(value(Item::MemRes, &p, &mut c, None), Val::ULong(16));
        assert_eq!(value(Item::MemShrPgs, &p, &mut c, None), Val::ULong(1));
        assert_eq!(value(Item::VmUsed, &p, &mut c, None), Val::ULong(19));
        assert_eq!(
            value(Item::TicsAllDelta, &p, &mut c, None),
            Val::UInt(u32::MAX)
        );
        assert_eq!(value(Item::ProcessorNode, &p, &mut c, None), Val::SInt(-1));
    }

    #[test]
    fn the_files_read_follow_the_items() {
        let (f, hist) = fill_for(&[Item::MemRes, Item::TicsAllDelta, Item::CmdlineV]);
        assert!(f.statm && f.stat && f.cmdline_v && !f.status);
        assert!(hist);
        let (f, hist) = fill_for(&[Item::Cmd]);
        assert!(f.stat && !hist, "either is stat when nothing else is read");
        let (f, _) = fill_for(&[Item::Cmd, Item::VmRss]);
        assert!(f.status && !f.stat, "either is satisfied by status");
    }
}
