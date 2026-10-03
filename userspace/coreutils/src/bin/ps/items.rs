//! procps' `<pids>` interface, as `ps` uses it: `library/pids.c`'s items,
//! their values, and its sort.
//!
//! `ps` does not read a process's fields directly. Each column's print
//! function first *registers* the items it needs (`setREL1` .. `setREL4`,
//! called with a NULL buffer from `finalize_stacks`); the library then reads
//! each process into a "stack" -- one value per registered item, in
//! registration order -- and the print functions read their values back out
//! by position. Three of upstream's behaviours follow from that design and
//! are kept here because they show:
//!
//! * **At most 70 items** (`PIDSITEMS`). An item registered after the 70th is
//!   given the `noop` slot, which always reads as zero (or a NULL string), so
//!   a very long `-o` list prints zeros in its last columns.
//! * **A sort key must be registered to sort.** `procps_pids_sort` finds the
//!   key's slot and returns without sorting if it has none. Most keys are
//!   registered by their column's own print function, but not all: `fgid`'s
//!   sort key is `FLAGS` and `utime`'s is `TICS_USER`, which nothing
//!   registers, so `--sort=utime` leaves the order alone.
//! * **A NULL string is not an empty one.** An item read by `STR_set` whose
//!   field is NULL becomes the text `[ duplicate CMD ]` (or whichever item
//!   it is) -- which a process with no `status` file shows in its `supgid`
//!   column.

use coreutils::procps::devname::{ABBREV_DEV, ABBREV_PTS, ABBREV_TTY, Devname};
use coreutils::procps::readproc::{Fill, Proc, SMAP_PRIVATE_CLEAN, SMAP_PRIVATE_DIRTY, SMAP_PSS};
use coreutils::procps::sysinfo;
use std::cmp::Ordering;
use std::path::Path;

/// `PIDSITEMS`: how many items a stack holds.
pub const PIDSITEMS: usize = 70;

/// The items `ps` can register or sort by: `enum pids_item`, cut to those.
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
    Cmd,
    Cmdline,
    Environ,
    Exe,
    Flags,
    FltMaj,
    FltMajC,
    FltMin,
    FltMinC,
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
    IdTpgid,
    IoReadBytes,
    IoReadChars,
    IoReadOps,
    IoWriteBytes,
    IoWriteCbytes,
    IoWriteChars,
    IoWriteOps,
    Lxcname,
    MemResPgs,
    MemShrPgs,
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
    SmapPrvTotal,
    SmapPss,
    State,
    Supgids,
    Supgroups,
    TicsAll,
    TicsAllC,
    TicsBegan,
    TicsUser,
    TicsUserC,
    TimeAll,
    TimeElapsed,
    Tty,
    TtyName,
    TtyNumber,
    Utilization,
    UtilizationC,
    VmData,
    VmExe,
    VmLib,
    VmRssLocked,
    VmRss,
    VmSize,
    VmStack,
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
    ULong,
    Real,
    Str,
    StrVers,
}

impl Item {
    /// `Item_table[item].sortfunc`, as a type.
    #[must_use]
    pub fn kind(self) -> Kind {
        use Item as I;
        match self {
            I::Noop => Kind::Noop,
            I::State => Kind::SCh,
            I::AutogrpId
            | I::AutogrpNice
            | I::IdLogin
            | I::IdPgrp
            | I::IdPid
            | I::IdPpid
            | I::IdSession
            | I::IdTgid
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
            | I::IdSuid => Kind::UInt,
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
            | I::MemResPgs
            | I::MemShrPgs
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
            | I::SmapPrvTotal
            | I::SmapPss
            | I::TicsAll
            | I::TicsAllC
            | I::TicsBegan
            | I::TicsUser
            | I::TicsUserC
            | I::VmData
            | I::VmExe
            | I::VmLib
            | I::VmRssLocked
            | I::VmRss
            | I::VmSize
            | I::VmStack
            | I::VsizeBytes => Kind::ULong,
            I::TimeAll | I::TimeElapsed | I::Utilization | I::UtilizationC => Kind::Real,
            I::TtyName | I::TtyNumber => Kind::StrVers,
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
    pub fn fill(self, f: &mut Fill, either: &mut bool) {
        use Item as I;
        match self {
            I::Noop | I::Extra | I::IdEgid | I::IdEuid | I::IdPid | I::IdTgid | I::WchanName => {}
            I::AddrCodeEnd
            | I::AddrCodeStart
            | I::AddrCurrEip
            | I::AddrCurrEsp
            | I::AddrStackStart
            | I::Flags
            | I::FltMaj
            | I::FltMajC
            | I::FltMin
            | I::FltMinC
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
            | I::TicsBegan
            | I::TicsUser
            | I::TicsUserC
            | I::TimeAll
            | I::TimeElapsed
            | I::Tty
            | I::TtyName
            | I::TtyNumber
            | I::Utilization
            | I::UtilizationC
            | I::VsizeBytes => f.stat = true,
            I::AutogrpId | I::AutogrpNice => f.autogrp = true,
            I::Cgname | I::Cgroup => f.cgroup = true,
            I::Cmd | I::IdPpid | I::Nlwp | I::State => *either = true,
            I::Cmdline => f.cmdline = true,
            I::Environ => f.environ = true,
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
            | I::VmRssLocked
            | I::VmRss
            | I::VmSize
            | I::VmStack => f.status = true,
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
            I::MemResPgs | I::MemShrPgs => {}
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
            I::SmapPrvTotal | I::SmapPss => f.smaps = true,
        }
    }

    /// The item's name as `STR_set` is given it, for `[ duplicate … ]`.
    fn enum_name(self) -> &'static str {
        use Item as I;
        match self {
            I::Cgname => "CGNAME",
            I::Cgroup => "CGROUP",
            I::Cmd => "CMD",
            I::Cmdline => "CMDLINE",
            I::Environ => "ENVIRON",
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
}

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
        let mut f = Fill::default();
        let mut either = false;
        for item in &self.items {
            item.fill(&mut f, &mut either);
        }
        if either && !f.stat && !f.status {
            f.stat = true;
        }
        f
    }
}

/// What the item functions need besides the process: `pids_info`'s
/// `hertz` and `boot_tics`, `/proc`, and the terminal namer.
pub struct ItemCtx {
    pub hertz: u64,
    /// `uptime * hertz`, at the time of the read.
    pub boot_tics: u64,
    pub devname: Devname,
    pub root: std::path::PathBuf,
}

/// gcc's x86-64 `double` to `long`: `cvttsd2si`, whose answer outside the
/// range (and for NaN) is `LONG_MIN`.
#[must_use]
pub fn cvt_i64(x: f64) -> i64 {
    // Truncation toward zero, in range by the test above it.
    #[allow(clippy::cast_possible_truncation)]
    if x.is_nan() || x >= 9_223_372_036_854_775_808.0 || x < -9_223_372_036_854_775_808.0 {
        i64::MIN
    } else {
        x as i64
    }
}

/// gcc's x86-64 `double` to `unsigned long`: `cvttsd2si` below 2^63, and
/// above it the same on `x - 2^63` with the top bit put back.
#[must_use]
pub fn cvt_u64(x: f64) -> u64 {
    const TWO63: f64 = 9_223_372_036_854_775_808.0;
    let bits = |v: i64| u64::from_le_bytes(v.to_le_bytes());
    if x < TWO63 {
        bits(cvt_i64(x))
    } else {
        bits(cvt_i64(x - TWO63)) ^ 0x8000_0000_0000_0000
    }
}

/// `double` to `unsigned int`: the low 32 bits of the 64-bit conversion.
#[must_use]
pub fn cvt_u32(x: f64) -> u32 {
    let b = cvt_i64(x).to_le_bytes();
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

/// `u64` as C converts it to `double`: to nearest.
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub fn dbl(v: u64) -> f64 {
    v as f64
}

/// `STR_set`: the field, or `[ duplicate SUPGIDS ]` (the item's name)
/// where it is NULL.
fn str_set(item: Item, v: Option<&Vec<u8>>) -> Val {
    Val::Str(Some(v.cloned().unwrap_or_else(|| {
        format!("[ duplicate {} ]", item.enum_name()).into_bytes()
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

/// The item functions (`setDECL(…)`): `item`'s value for process `p`.
#[allow(clippy::too_many_lines)]
pub fn value(item: Item, p: &Proc, ctx: &mut ItemCtx) -> Val {
    use Item as I;
    let tics_since = |start: u64| dbl(ctx.boot_tics.wrapping_sub(start));
    match item {
        I::Noop => Val::Zero,
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
        I::Cmd => str_set(item, p.cmd.as_ref()),
        I::Cmdline => str_set(item, p.cmdline.as_ref()),
        I::Environ => str_set(item, p.environ.as_ref()),
        I::Exe => str_set(item, p.exe.as_ref()),
        I::Flags => Val::ULong(p.flags),
        I::FltMaj => Val::ULong(p.maj_flt),
        I::FltMajC => Val::ULong(p.maj_flt.wrapping_add(p.cmaj_flt)),
        I::FltMin => Val::ULong(p.min_flt),
        I::FltMinC => Val::ULong(p.min_flt.wrapping_add(p.cmin_flt)),
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
        I::IdPid => Val::SInt(p.tid),
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
        // Never registered by `ps`; there is no `statm` read to give them.
        I::MemResPgs | I::MemShrPgs | I::TicsUser | I::TicsUserC | I::VmExe | I::VmLib => Val::Zero,
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
        I::SmapPrvTotal => {
            Val::ULong(p.smap[SMAP_PRIVATE_CLEAN].wrapping_add(p.smap[SMAP_PRIVATE_DIRTY]))
        }
        I::SmapPss => Val::ULong(p.smap[SMAP_PSS]),
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
        I::TicsBegan => Val::ULong(p.start_time),
        I::TimeAll => Val::Real((dbl(p.utime) + dbl(p.stime)) / dbl(ctx.hertz)),
        I::TimeElapsed => {
            let t = tics_since(p.start_time);
            Val::Real(if t > 0.0 { t / dbl(ctx.hertz) } else { 0.0 })
        }
        I::Tty => Val::SInt(p.tty),
        I::TtyName => Val::Str(Some(ctx.devname.dev_to_tty(
            64,
            u32::from_le_bytes(p.tty.to_le_bytes()),
            p.tid,
            ABBREV_DEV,
        ))),
        I::TtyNumber => Val::Str(Some(ctx.devname.dev_to_tty(
            64,
            u32::from_le_bytes(p.tty.to_le_bytes()),
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
                Val::Real(0.0)
            }
        }
        I::VmData => Val::ULong(p.vm_data),
        I::VmRssLocked => Val::ULong(p.vm_lock),
        I::VmRss => Val::ULong(p.vm_rss),
        I::VmSize => Val::ULong(p.vm_size),
        I::VmStack => Val::ULong(p.vm_stack),
        I::VsizeBytes => Val::ULong(p.vsize),
        I::WchanName => Val::Str(Some(sysinfo::lookup_wchan(&ctx.root, p.tid))),
    }
}

/// One process's stack: a value per registered item.
pub type Stack = Vec<Val>;

/// `pids_assign_results`: the stack for `p`.
#[must_use]
pub fn assign(reg: &Registry, p: &Proc, ctx: &mut ItemCtx) -> Stack {
    reg.items()
        .iter()
        .map(|&item| value(item, p, ctx))
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

/// The comparison `Item_table`'s `sortfunc` makes on slot `at`, with C's
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
    match kind {
        Kind::Noop => 0,
        Kind::SCh => {
            let (x, y) = (i32::from(a.s_ch() as i8), i32::from(b.s_ch() as i8));
            order.wrapping_mul(x.wrapping_sub(y))
        }
        Kind::SInt => order.wrapping_mul(a.s_int().wrapping_sub(b.s_int())),
        Kind::UInt => reg(a.u_int().partial_cmp(&b.u_int())),
        Kind::ULong => reg(a.ul().partial_cmp(&b.ul())),
        Kind::Real => reg(a.real().partial_cmp(&b.real())),
        // `strcoll` in the C.UTF-8 locale this is measured in is `strcmp`.
        Kind::Str => {
            let o = a.str().unwrap_or_default().cmp(b.str().unwrap_or_default());
            order.wrapping_mul(match o {
                Ordering::Less => -1,
                Ordering::Equal => 0,
                Ordering::Greater => 1,
            })
        }
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
fn msort<T: Clone>(v: &mut [T], cmp: &dyn Fn(&T, &T) -> i32) {
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

/// `procps_pids_sort`: the stacks named by `order` (indices into `stacks`)
/// sorted on `item` -- or left alone if `item` has no slot, or the direction
/// is neither [`ASCEND`] nor [`DESCEND`].
pub fn sort(reg: &Registry, stacks: &[Stack], idx: &mut [usize], item: Item, dir: i32) {
    if dir != ASCEND && dir != DESCEND {
        return;
    }
    if idx.len() < 2 {
        return;
    }
    let Some(at) = reg.items().iter().position(|&i| i == item) else {
        return;
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

    #[test]
    fn a_sort_on_an_unregistered_item_changes_nothing() {
        let mut r = Registry::default();
        r.chk(Item::IdPid);
        let stacks = vec![vec![Val::SInt(3)], vec![Val::SInt(1)], vec![Val::SInt(2)]];
        let mut idx = vec![0, 1, 2];
        sort(&r, &stacks, &mut idx, Item::TicsUser, ASCEND);
        assert_eq!(idx, [0, 1, 2]);
        sort(&r, &stacks, &mut idx, Item::IdPid, 0);
        assert_eq!(idx, [0, 1, 2]);
        sort(&r, &stacks, &mut idx, Item::IdPid, ASCEND);
        assert_eq!(idx, [1, 2, 0]);
        sort(&r, &stacks, &mut idx, Item::IdPid, DESCEND);
        assert_eq!(idx, [0, 2, 1]);
    }

    #[test]
    fn int_sorts_overflow_as_c_does() {
        // INT_MIN - 1 wraps to INT_MAX: upstream sorts INT_MIN *after* 1.
        assert!(compare(Kind::SInt, &Val::SInt(i32::MIN), &Val::SInt(1), ASCEND) > 0);
        assert!(compare(Kind::SInt, &Val::SInt(-1), &Val::SInt(1), ASCEND) < 0);
    }

    #[test]
    fn conversions_as_gcc_emits_them() {
        assert_eq!(cvt_u64(1.9), 1);
        assert_eq!(cvt_u64(-1.5), u64::MAX);
        assert_eq!(cvt_u64(1.0e19), 10_000_000_000_000_000_000);
        assert_eq!(cvt_u64(f64::NAN), 0);
        assert_eq!(cvt_u32(4_294_967_297.0), 1);
        assert_eq!(cvt_i64(f64::NAN), i64::MIN);
    }
}
