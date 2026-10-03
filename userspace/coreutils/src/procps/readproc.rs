//! procps-ng 4.0.4's `library/readproc.c`: one process, read from `/proc`.
//!
//! A [`Proc`] is upstream's `proc_t`, cut to the fields `ps` can ask for, and
//! [`Reader`] fills it the way `simple_readproc` and `simple_readtask` do:
//! `stat` first (a process whose `stat` cannot be read is skipped), then
//! whichever other files the caller's [`Fill`] names. Each parser is a
//! transcription, quirks included, because what `ps` prints for a process is
//! what these functions made of its files:
//!
//! * **`status` overrides `stat`.** It is read second, and its `State:`,
//!   `PPid:`, `Pid:`, `Tgid:` and `Threads:` lines replace what `stat` said --
//!   so a process's PID column is its `status`'s `Pid:` line, and is 0 if
//!   there is none.
//! * **A missing number is not an error.** `stat` is read by one `sscanf`;
//!   the first field it cannot convert ends the scan and every later field
//!   keeps the value it had (zero, mostly). `status` is read a line at a time,
//!   and a line it does not know is skipped -- but the first line whose colon
//!   is not followed by a tab ends the whole file.
//! * **Signal masks are sixteen bytes**, copied as they are: a shorter mask
//!   takes the newline and the start of the next line with it.
//! * **Names are escaped once, here**, into 64 bytes (see
//!   [`super::escape_str`]).
//!
//! [`Reader::reap`], [`Reader::reap_threads`] and [`Reader::select`] are the
//! three walks over `/proc` that `openproc`'s finders make, in directory
//! order: not sorted, because upstream does not sort.

use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use super::pwcache::Pwcache;
use super::scanf::{self, Scan};
use super::{MAX_BUFSZ, escape_str, unvectored};

/// The size `stat2proc` and `status2proc` escape a name into.
const CMD_BUFSZ: usize = 64;

/// One process or thread, as `readproc` leaves it.
///
/// Numbers carry C's types' widths so that a field reads as upstream's does
/// when it overflows; strings are `None` where upstream's pointer is NULL,
/// which several of `ps`'s columns print differently from an empty string.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Proc {
    pub tid: i32,
    pub tgid: i32,
    pub ppid: i32,
    pub pgrp: i32,
    pub session: i32,
    pub tty: i32,
    pub tpgid: i32,
    /// The state letter, a C `char`.
    pub state: u8,
    pub flags: u64,
    pub min_flt: u64,
    pub cmin_flt: u64,
    pub maj_flt: u64,
    pub cmaj_flt: u64,
    pub utime: u64,
    pub stime: u64,
    pub cutime: u64,
    pub cstime: u64,
    pub priority: i32,
    pub nice: i32,
    pub nlwp: i32,
    pub alarm: u64,
    pub start_time: u64,
    pub vsize: u64,
    pub rss: u64,
    pub rss_rlim: u64,
    pub start_code: u64,
    pub end_code: u64,
    pub start_stack: u64,
    pub kstk_esp: u64,
    pub kstk_eip: u64,
    pub wchan: u64,
    pub exit_signal: i32,
    pub processor: i32,
    pub rtprio: i32,
    pub sched: i32,
    pub blkio_tics: u64,
    pub gtime: u64,
    pub cgtime: u64,
    pub ruid: u32,
    pub euid: u32,
    pub suid: u32,
    pub fuid: u32,
    pub rgid: u32,
    pub egid: u32,
    pub sgid: u32,
    pub fgid: u32,
    /// `ShdPnd` (or `SigPnd`, see [`status2proc`]), as text.
    pub signal: Vec<u8>,
    pub blocked: Vec<u8>,
    pub sigcatch: Vec<u8>,
    pub sigignore: Vec<u8>,
    /// `SigPnd`: the thread's own pending signals.
    pub sigpnd: Vec<u8>,
    pub vm_data: u64,
    pub vm_exe: u64,
    pub vm_lock: u64,
    pub vm_lib: u64,
    pub vm_rss: u64,
    pub vm_rss_anon: u64,
    pub vm_rss_file: u64,
    pub vm_rss_shared: u64,
    pub vm_size: u64,
    pub vm_stack: u64,
    pub vm_swap: u64,
    /// The `Groups:` line with commas for spaces, or `-`.
    pub supgid: Option<Vec<u8>>,
    pub supgrp: Option<Vec<u8>>,
    pub rchar: u64,
    pub wchar: u64,
    pub syscr: u64,
    pub syscw: u64,
    pub read_bytes: u64,
    pub write_bytes: u64,
    pub cancelled_write_bytes: u64,
    /// `smaps_rollup`, in [`SMAPS`]'s order.
    pub smap: [u64; SMAPS.len()],
    pub oom_score: i32,
    pub oom_adj: i32,
    /// Namespace inode numbers, in `ns_names`' order; 0 for one that is not
    /// there.
    pub ns: [u64; 8],
    pub cmd: Option<Vec<u8>>,
    pub cmdline: Option<Vec<u8>>,
    pub environ: Option<Vec<u8>>,
    pub cgroup: Option<Vec<u8>>,
    pub cgname: Option<Vec<u8>>,
    pub exe: Option<Vec<u8>>,
    pub lxcname: Option<Vec<u8>>,
    pub luid: i32,
    pub autogrp_id: i32,
    pub autogrp_nice: i32,
    pub euser: Option<Vec<u8>>,
    pub egroup: Option<Vec<u8>>,
    pub ruser: Option<Vec<u8>>,
    pub suser: Option<Vec<u8>>,
    pub fuser: Option<Vec<u8>>,
    pub rgroup: Option<Vec<u8>>,
    pub sgroup: Option<Vec<u8>>,
    pub fgroup: Option<Vec<u8>>,
    /// The `sd_*` columns: `?`, since there is no logind (`sd2proc` built
    /// without systemd).
    pub sd: Option<Vec<u8>>,
}

/// Which files to read beyond `stat`: upstream's `PROC_FILL*` flags.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct Fill {
    pub stat: bool,
    pub status: bool,
    pub io: bool,
    pub smaps: bool,
    pub usr: bool,
    pub grp: bool,
    pub ousers: bool,
    pub ogroups: bool,
    pub supgrp: bool,
    pub environ: bool,
    pub cmdline: bool,
    pub cgroup: bool,
    pub oom: bool,
    pub ns: bool,
    pub systemd: bool,
    pub lxc: bool,
    pub luid: bool,
    pub exe: bool,
    pub autogrp: bool,
}

/// `smaps2proc`'s table, in the order it searches.
pub const SMAPS: [&[u8]; 20] = [
    b"Rss:",
    b"Pss:",
    b"Pss_Anon:",
    b"Pss_File:",
    b"Pss_Shmem:",
    b"Shared_Clean:",
    b"Shared_Dirty:",
    b"Private_Clean:",
    b"Private_Dirty:",
    b"Referenced:",
    b"Anonymous:",
    b"LazyFree:",
    b"AnonHugePages:",
    b"ShmemPmdMapped:",
    b"FilePmdMapped:",
    b"Shared_Hugetlb:",
    b"Private_Hugetlb:",
    b"Swap:",
    b"SwapPss:",
    b"Locked:",
];

/// [`SMAPS`] indices `ps` reads.
pub const SMAP_PSS: usize = 1;
pub const SMAP_PRIVATE_CLEAN: usize = 7;
pub const SMAP_PRIVATE_DIRTY: usize = 8;

/// `ns_names`, in `procps_ns_read_pid`'s order.
pub const NS_NAMES: [&str; 8] = ["cgroup", "ipc", "mnt", "net", "pid", "time", "user", "uts"];

/// A C string's text: everything before the first NUL.
#[must_use]
pub fn cstr(s: &[u8]) -> &[u8] {
    let end = s.iter().position(|&b| b == 0).unwrap_or(s.len());
    s.get(..end).unwrap_or_default()
}

/// Sixteen bytes of a signal mask, as `memcpy` takes them, ending early only
/// where the text does.
fn mask16(s: &[u8]) -> Vec<u8> {
    s.get(..s.len().min(16)).unwrap_or_default().to_vec()
}

/// `stat2proc`: `/proc/<pid>/stat`'s text into `p`.
///
/// The name is the text between the first `(` and the *last* `)`, so a name
/// with parentheses in it survives; it is taken only if `p` has none yet. A
/// stat with no `(`, no `)`, or nothing after the `)` leaves `p` with only the
/// defaults set here.
pub fn stat2proc(text: &[u8], p: &mut Proc, utf8: bool) {
    let s = cstr(text);
    p.processor = 0;
    p.rtprio = -1;
    p.sched = -1;
    p.nlwp = 0;

    let Some(open) = s.iter().position(|&b| b == b'(') else {
        return;
    };
    let after = s.get(open.saturating_add(1)..).unwrap_or_default();
    let Some(close) = after.iter().rposition(|&b| b == b')') else {
        return;
    };
    if after.get(close.saturating_add(1)).is_none() {
        return;
    }
    if p.cmd.is_none() {
        // `raw` is 64 bytes in C; a longer name overflows it there, and is
        // cut to what `escape_str`'s 64-byte buffer keeps here.
        p.cmd = Some(escape_str(
            after.get(..close).unwrap_or_default(),
            CMD_BUFSZ,
            utf8,
        ));
    }
    let rest = after.get(close.saturating_add(2)..).unwrap_or_default();
    let mut sc = Scan::new(rest);

    macro_rules! take {
        ($conv:ident, $($field:ident),+) => {
            $( if let Some(v) = sc.$conv() { p.$field = v; } )+
        };
    }
    if let Some(c) = sc.ch() {
        p.state = c;
    }
    sc.ws();
    take!(int, ppid, pgrp, session, tty, tpgid);
    take!(ulong, flags, min_flt, cmin_flt, maj_flt, cmaj_flt);
    take!(ulong, utime, stime, cutime, cstime);
    take!(int, priority, nice, nlwp);
    take!(ulong, alarm, start_time, vsize, rss);
    take!(
        ulong,
        rss_rlim,
        start_code,
        end_code,
        start_stack,
        kstk_esp,
        kstk_eip
    );
    for _ in 0..4 {
        let _ = sc.skip_word(); // a failure is remembered by `sc`, which is all that matters
    }
    take!(ulong, wchan);
    let _ = sc.skip_uint(); // as above
    let _ = sc.skip_uint();
    take!(int, exit_signal, processor, rtprio, sched);
    take!(ulong, blkio_tics, gtime, cgtime);

    if p.nlwp == 0 {
        p.nlwp = 1;
    }
}

/// The lines `status2proc` knows.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Line {
    Name,
    ShdPnd,
    SigBlk,
    SigCgt,
    SigIgn,
    SigPnd,
    State,
    Tgid,
    Pid,
    PPid,
    Threads,
    Uid,
    Gid,
    VmData,
    VmExe,
    VmLck,
    VmLib,
    VmRSS,
    RssAnon,
    RssFile,
    RssShmem,
    VmSize,
    VmStk,
    VmSwap,
    Groups,
    Ignored,
}

/// The gperf table's lookup, which for the names in it is an exact match.
fn line_kind(name: &[u8]) -> Option<Line> {
    Some(match name {
        b"Name" => Line::Name,
        b"ShdPnd" => Line::ShdPnd,
        b"SigBlk" => Line::SigBlk,
        b"SigCgt" => Line::SigCgt,
        b"SigIgn" => Line::SigIgn,
        b"SigPnd" => Line::SigPnd,
        b"State" => Line::State,
        b"Tgid" => Line::Tgid,
        b"Pid" => Line::Pid,
        b"PPid" => Line::PPid,
        b"Threads" => Line::Threads,
        b"Uid" => Line::Uid,
        b"Gid" => Line::Gid,
        b"VmData" => Line::VmData,
        b"VmExe" => Line::VmExe,
        b"VmLck" => Line::VmLck,
        b"VmLib" => Line::VmLib,
        b"VmRSS" => Line::VmRSS,
        b"RssAnon" => Line::RssAnon,
        b"RssFile" => Line::RssFile,
        b"RssShmem" => Line::RssShmem,
        b"VmSize" => Line::VmSize,
        b"VmStk" => Line::VmStk,
        b"VmSwap" => Line::VmSwap,
        b"Groups" => Line::Groups,
        b"CapBnd" | b"CapEff" | b"CapInh" | b"CapPrm" | b"FDSize" | b"SigQ" | b"VmHWM"
        | b"VmPTE" | b"VmPeak" => Line::Ignored,
        _ => return None,
    })
}

/// `status2proc`: `/proc/<pid>/status`'s text into `p`. `is_proc` is false
/// for a thread, whose pending signals are its own (`SigPnd`) rather than the
/// process's (`ShdPnd`).
#[allow(clippy::too_many_lines)]
pub fn status2proc(text: &[u8], p: &mut Proc, is_proc: bool, utf8: bool) {
    let s = cstr(text);
    let at = |k: usize| s.get(k).copied().unwrap_or(0);
    let mut threads: i64 = 0;
    let mut tgid: i64 = 0;
    let mut pid: i64 = 0;

    // `S`, the position being examined. The first pass starts at `base`,
    // without looking for a newline first.
    let mut i = 0usize;
    let mut first = true;
    loop {
        if !first {
            match s.get(i..).and_then(|r| r.iter().position(|&b| b == b'\n')) {
                Some(n) => i = i.saturating_add(n).saturating_add(1),
                None => break,
            }
        }
        first = false;
        if (0..4).any(|k| at(i.saturating_add(k)) == 0) {
            break;
        }
        let Some(colon) = s
            .get(i..)
            .and_then(|r| r.iter().position(|&b| b == b':'))
            .map(|n| i.saturating_add(n))
        else {
            break;
        };
        if at(colon.saturating_add(1)) != b'\t' {
            break;
        }
        let Some(kind) = line_kind(s.get(i..colon).unwrap_or_default()) else {
            continue;
        };
        i = colon.saturating_add(2);

        // `strtol (S, &S, 10)` from `i`.
        let number = |i: &mut usize| -> i64 {
            let (v, used) = scanf::strtol(s.get(*i..).unwrap_or_default());
            *i = i.saturating_add(used);
            v
        };
        match kind {
            Line::Name => {
                let mut raw = Vec::with_capacity(63);
                while raw.len() < 63 {
                    let mut c = at(i);
                    i = i.saturating_add(1);
                    if c == b'\n' || c == 0 {
                        break;
                    }
                    if c == b'\\' {
                        c = at(i);
                        i = i.saturating_add(1);
                        if c == b'\n' || c == 0 {
                            break;
                        }
                        if c == b'n' {
                            c = b'\n';
                        }
                    }
                    raw.push(c);
                }
                if p.cmd.is_none() {
                    p.cmd = Some(escape_str(&raw, CMD_BUFSZ, utf8));
                }
                // `S--`: the newline (or NUL) goes back for the next search.
                i = i.saturating_sub(1);
            }
            Line::ShdPnd => p.signal = mask16(s.get(i..).unwrap_or_default()),
            Line::SigBlk => p.blocked = mask16(s.get(i..).unwrap_or_default()),
            Line::SigCgt => p.sigcatch = mask16(s.get(i..).unwrap_or_default()),
            Line::SigIgn => p.sigignore = mask16(s.get(i..).unwrap_or_default()),
            Line::SigPnd => p.sigpnd = mask16(s.get(i..).unwrap_or_default()),
            Line::State => p.state = at(i),
            Line::Tgid => tgid = number(&mut i),
            Line::Pid => pid = number(&mut i),
            Line::PPid => p.ppid = scanf::low_i32(number(&mut i)),
            Line::Threads => threads = number(&mut i),
            Line::Uid => {
                p.ruid = scanf::low_u32(number(&mut i));
                p.euid = scanf::low_u32(number(&mut i));
                p.suid = scanf::low_u32(number(&mut i));
                p.fuid = scanf::low_u32(number(&mut i));
            }
            Line::Gid => {
                p.rgid = scanf::low_u32(number(&mut i));
                p.egid = scanf::low_u32(number(&mut i));
                p.sgid = scanf::low_u32(number(&mut i));
                p.fgid = scanf::low_u32(number(&mut i));
            }
            Line::VmData => p.vm_data = scanf::as_ulong(number(&mut i)),
            Line::VmExe => p.vm_exe = scanf::as_ulong(number(&mut i)),
            Line::VmLck => p.vm_lock = scanf::as_ulong(number(&mut i)),
            Line::VmLib => p.vm_lib = scanf::as_ulong(number(&mut i)),
            Line::VmRSS => p.vm_rss = scanf::as_ulong(number(&mut i)),
            Line::RssAnon => p.vm_rss_anon = scanf::as_ulong(number(&mut i)),
            Line::RssFile => p.vm_rss_file = scanf::as_ulong(number(&mut i)),
            Line::RssShmem => p.vm_rss_shared = scanf::as_ulong(number(&mut i)),
            Line::VmSize => p.vm_size = scanf::as_ulong(number(&mut i)),
            Line::VmStk => p.vm_stack = scanf::as_ulong(number(&mut i)),
            Line::VmSwap => p.vm_swap = scanf::as_ulong(number(&mut i)),
            Line::Groups => {
                // A last line with no newline is skipped: `ss >= nl` with
                // `nl` NULL is true.
                let Some(nl) = s
                    .get(i..)
                    .and_then(|r| r.iter().position(|&b| b == b'\n'))
                    .map(|n| i.saturating_add(n))
                else {
                    continue;
                };
                let mut ss = i;
                while matches!(at(ss), b' ' | b'\t') {
                    ss = ss.saturating_add(1);
                }
                if ss >= nl {
                    continue;
                }
                let mut g = s.get(ss..nl).unwrap_or_default().to_vec();
                if g.last() == Some(&b' ') {
                    g.pop();
                }
                // The spaces become commas, all but the first byte's.
                for b in g.iter_mut().skip(1) {
                    if *b == b' ' {
                        *b = b',';
                    }
                }
                p.supgid = Some(g);
            }
            Line::Ignored => {}
        }
    }

    if !is_proc || p.signal.is_empty() {
        p.signal.clone_from(&p.sigpnd);
    }
    if threads != 0 {
        p.nlwp = scanf::low_i32(threads);
        p.tgid = scanf::low_i32(tgid);
        p.tid = scanf::low_i32(pid);
    } else {
        p.nlwp = 1;
        p.tgid = scanf::low_i32(pid);
        p.tid = scanf::low_i32(pid);
    }
    if p.supgid.is_none() {
        p.supgid = Some(b"-".to_vec());
    }
}

/// `supgrps_from_supgids`: the supplementary group *names*, by number.
pub fn supgrps_from_supgids(p: &mut Proc, pw: &mut Pwcache) {
    const MAX: usize = 33 + 2;
    if let Some(ids) = p.supgid.as_deref().filter(|s| s.first() != Some(&b'-')) {
        let mut out: Option<Vec<u8>> = None;
        let mut s = ids;
        loop {
            let skip = s.iter().take_while(|&&b| b == b',').count();
            s = s.get(skip..).unwrap_or_default();
            let (gid, used) = scanf::strtol(s);
            if used == 0 {
                break;
            }
            s = s.get(used..).unwrap_or_default();
            let name = pw.group(scanf::low_u32(gid));
            let buf = out.get_or_insert_with(Vec::new);
            let mut piece = if buf.is_empty() {
                Vec::new()
            } else {
                vec![b',']
            };
            piece.extend_from_slice(&name);
            piece.truncate(MAX.saturating_sub(1));
            buf.extend_from_slice(&piece);
            if s.is_empty() {
                break;
            }
        }
        p.supgrp = out;
    }
    if p.supgrp.is_none() {
        p.supgrp = Some(b"-".to_vec());
    }
}

/// `io2proc`.
pub fn io2proc(text: &[u8], p: &mut Proc) {
    let mut sc = Scan::new(text);
    let fields: [(&[u8], &mut u64); 7] = [
        (b"rchar: ", &mut p.rchar),
        (b" wchar: ", &mut p.wchar),
        (b" syscr: ", &mut p.syscr),
        (b" syscw: ", &mut p.syscw),
        (b" read_bytes: ", &mut p.read_bytes),
        (b" write_bytes: ", &mut p.write_bytes),
        (b" cancelled_write_bytes: ", &mut p.cancelled_write_bytes),
    ];
    for (label, field) in fields {
        if sc.lit(label).is_none() {
            break;
        }
        match sc.ulong() {
            Some(v) => *field = v,
            None => break,
        }
    }
}

/// `smaps2proc`: each name searched for after the previous one found, so a
/// field out of the kernel's order is missed.
pub fn smaps2proc(text: &[u8], p: &mut Proc) {
    let mut s = cstr(text);
    for (slot, key) in p.smap.iter_mut().zip(SMAPS) {
        let Some(at) = s.windows(key.len()).position(|w| w == key) else {
            continue;
        };
        let head = s.get(at.saturating_add(key.len())..).unwrap_or_default();
        let (v, used) = scanf::strtoul(head);
        *slot = v;
        s = head.get(used..).unwrap_or_default();
    }
}

/// `sscanf (S, "%d", &x)`: `x` changed only if there is a number.
fn scan_int(text: &[u8], x: &mut i32) {
    if let Some(v) = Scan::new(text).int() {
        *x = v;
    }
}

/// `escape_str` into `MAX_BUFSZ`: the escaping `readproc` gives a command
/// line, an environment or a cgroup path.
fn escape_big(src: &[u8], utf8: bool) -> Vec<u8> {
    escape_str(src, MAX_BUFSZ, utf8)
}

/// What `read_unvectored` reads: at most `MAX_BUFSZ - 1` bytes, `None` when
/// the file cannot be opened or holds nothing.
fn read_raw(path: &Path) -> Option<Vec<u8>> {
    let file = fs::File::open(path).ok()?;
    let mut raw = Vec::new();
    // A read error mid-file keeps what was read, as upstream's loop does.
    let _ = file
        .take(u64::try_from(MAX_BUFSZ.saturating_sub(1)).unwrap_or(u64::MAX))
        .read_to_end(&mut raw);
    (!raw.is_empty()).then_some(raw)
}

/// `fill_environ_cvt`.
fn environ_cvt(dir: &Path, utf8: bool) -> Vec<u8> {
    let text = read_raw(&dir.join("environ"))
        .and_then(|raw| unvectored(&raw))
        .map(|line| escape_big(&line, utf8))
        .unwrap_or_default();
    if text.is_empty() { b"-".to_vec() } else { text }
}

/// `fill_cgroup_cvt`: the cgroup lines that are not a bare root, escaped and
/// joined with commas, and the `name=` part of the first that has one.
fn cgroup_cvt(dir: &Path, utf8: bool) -> (Vec<u8>, Vec<u8>) {
    let mut out: Vec<u8> = Vec::new();
    if let Some(raw) = read_raw(&dir.join("cgroup")) {
        // `read_unvectored` with a NUL separator: newlines and NULs end
        // strings, and a last byte that is a space is cut.
        let mut buf = raw;
        let keep = buf
            .iter()
            .rposition(|&b| b != 0)
            .map_or(0, |at| at.saturating_add(1));
        for b in buf.iter_mut().take(keep) {
            if *b == b'\n' {
                *b = 0;
            }
        }
        if buf.last() == Some(&b' ')
            && let Some(b) = buf.last_mut()
        {
            *b = 0;
        }
        for grp in buf.split(|&b| b == 0).filter(|g| !g.is_empty()) {
            if grp.last() == Some(&b'/') {
                continue;
            }
            let room = MAX_BUFSZ.saturating_sub(out.len());
            if room <= 1 {
                break;
            }
            if !out.is_empty() {
                if room <= 1 {
                    break;
                }
                out.push(b',');
            }
            let room = MAX_BUFSZ.saturating_sub(out.len());
            out.extend_from_slice(&escape_str(grp, room, utf8));
        }
    }
    let cgroup = if out.is_empty() { b"-".to_vec() } else { out };
    let name = match cgroup.windows(6).position(|w| w == b":name=") {
        Some(at) if cgroup.get(at.saturating_add(6)).is_some() => cgroup
            .get(at.saturating_add(6)..)
            .unwrap_or_default()
            .to_vec(),
        _ => cgroup.clone(),
    };
    (cgroup, name)
}

/// `lxc_containers`: the innermost container name in a task's cgroup, or `-`.
fn lxc_name(dir: &Path) -> Vec<u8> {
    let Some(text) = file2str(dir, "cgroup") else {
        return b"-".to_vec();
    };
    let buf = cstr(&text);
    let find = |hay: &[u8], needle: &[u8]| hay.windows(needle.len()).position(|w| w == needle);
    let mut hit = None;
    for delim in [&b"lxc.payload."[..], b"lxc.payload/", b"lxc/"] {
        if let Some(at) = find(buf, delim) {
            hit = Some((at, delim));
            break;
        }
    }
    let Some((at, delim)) = hit else {
        return b"-".to_vec();
    };
    // The controller's line only, then the last occurrence of the delimiter.
    let line = buf.get(at..).unwrap_or_default();
    let line = line.split(|&b| b == b'\n').next().unwrap_or_default();
    let mut name = line;
    loop {
        name = name.get(delim.len()..).unwrap_or_default();
        match find(name, delim) {
            Some(next) => name = name.get(next..).unwrap_or_default(),
            None => break,
        }
    }
    name.split(|&b| b == b'/')
        .next()
        .unwrap_or_default()
        .to_vec()
}

/// A small file read once, as `read (fd, buf, sizeof buf - 1)` does: at
/// most `max` bytes, `None` for nothing or for a file that cannot be opened.
fn read_small(path: &Path, max: usize) -> Option<Vec<u8>> {
    let mut file = fs::File::open(path).ok()?;
    let mut buf = vec![0u8; max];
    let n = file.read(&mut buf).ok()?;
    if n == 0 {
        return None;
    }
    buf.truncate(n);
    Some(buf)
}

/// `PROCPATHLEN - 1`: what `login_uid` and `autogroup_fill` read at most.
const SMALL_READ: usize = 63;

/// `login_uid`: `loginuid`'s number, or -1.
fn login_uid(dir: &Path) -> i32 {
    read_small(&dir.join("loginuid"), SMALL_READ).map_or(-1, |t| scanf::atoi(cstr(&t)))
}

/// `readlink_exe`: the executable's path, escaped, or `-`.
fn readlink_exe(dir: &Path, utf8: bool) -> Vec<u8> {
    match fs::read_link(dir.join("exe")) {
        Ok(target) => {
            let bytes = crate::quote::os_bytes(target.as_os_str()).into_owned();
            let cut = bytes
                .get(..bytes.len().min(MAX_BUFSZ.saturating_sub(1)))
                .unwrap_or_default();
            if cut.is_empty() {
                b"-".to_vec()
            } else {
                escape_big(cut, utf8)
            }
        }
        Err(_) => b"-".to_vec(),
    }
}

/// `autogroup_fill`.
fn autogroup_fill(dir: &Path, p: &mut Proc) {
    p.autogrp_id = -1;
    if let Some(text) = read_small(&dir.join("autogroup"), SMALL_READ) {
        let mut sc = Scan::new(&text);
        if sc.lit(b"/autogroup-").is_some() {
            if let Some(id) = sc.int() {
                p.autogrp_id = id;
            }
            if sc.lit(b" nice ").is_some()
                && let Some(n) = sc.int()
            {
                p.autogrp_nice = n;
            }
        }
    }
}

/// `stat`'s inode number for `path`, following links.
#[cfg(unix)]
fn inode(path: &Path) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    fs::metadata(path).ok().map(|m| m.ino())
}

/// The host build has no inodes; it never reads a real `/proc`.
#[cfg(not(unix))]
fn inode(_path: &Path) -> Option<u64> {
    None
}

/// `procps_ns_read_pid`: each namespace's inode, 0 where `stat` fails.
fn ns_read_pid(root: &Path, tid: i32, ns: &mut [u64; 8]) {
    if tid < 1 {
        return;
    }
    for (slot, name) in ns.iter_mut().zip(NS_NAMES) {
        *slot = inode(&root.join(format!("{tid}/ns/{name}"))).unwrap_or(0);
    }
}

/// `file2str`: the whole file, or `None` if it cannot be opened or is empty.
#[must_use]
pub fn file2str(dir: &Path, what: &str) -> Option<Vec<u8>> {
    let mut text = Vec::new();
    fs::File::open(dir.join(what))
        .ok()?
        .read_to_end(&mut text)
        .ok()?;
    (!text.is_empty()).then_some(text)
}

/// The owner and group of a directory, which `readproc` takes as the
/// effective IDs until `status` says otherwise.
#[cfg(unix)]
fn owner(meta: &fs::Metadata) -> (u32, u32) {
    use std::os::unix::fs::MetadataExt;
    (meta.uid(), meta.gid())
}

/// The host build has no owners; it never reads a real `/proc`.
#[cfg(not(unix))]
fn owner(_meta: &fs::Metadata) -> (u32, u32) {
    (0, 0)
}

/// `openproc`'s state, and the readers it hands out.
pub struct Reader {
    /// Where `/proc` is: `/proc` itself, but for tests.
    pub root: PathBuf,
    pub fill: Fill,
    /// `LIBPROC_HIDE_KERNEL`: leave out the kernel's threads.
    pub hide_kernel: bool,
    pub utf8: bool,
    pub pw: Pwcache,
}

/// Which processes one walk returns.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Finder<'a> {
    /// `simple_nextpid`: every directory under `/proc` named by a number.
    All,
    /// `listed_nextpid`: these PIDs, in this order.
    Listed(&'a [u32]),
}

impl Reader {
    /// A reader over `root` for the files `fill` names.
    #[must_use]
    pub fn new(root: PathBuf, fill: Fill, utf8: bool, pw: Pwcache) -> Self {
        let mut fill = fill;
        let hide_kernel = std::env::var_os("LIBPROC_HIDE_KERNEL").is_some();
        // The parent PID is needed to hide anything; get it the cheapest way.
        if hide_kernel && !fill.stat && !fill.status {
            fill.stat = true;
        }
        Self {
            root,
            fill,
            hide_kernel,
            utf8,
            pw,
        }
    }

    /// The parts of `simple_readproc` and `simple_readtask` after `stat` that
    /// both share, in upstream's order.
    fn read_status_and_names(&mut self, path: &Path, p: &mut Proc, is_proc: bool) {
        if self.fill.io
            && let Some(text) = file2str(path, "io")
        {
            io2proc(&text, p);
        }
        if self.fill.smaps
            && let Some(text) = file2str(path, "smaps_rollup")
        {
            smaps2proc(&text, p);
        }
        if self.fill.status
            && let Some(text) = file2str(path, "status")
        {
            status2proc(&text, p, is_proc, self.utf8);
            if self.fill.supgrp {
                supgrps_from_supgids(p, &mut self.pw);
            }
            if self.fill.ousers {
                p.ruser = Some(self.pw.user(p.ruid));
                p.suser = Some(self.pw.user(p.suid));
                p.fuser = Some(self.pw.user(p.fuid));
            }
            if self.fill.ogroups {
                p.rgroup = Some(self.pw.group(p.rgid));
                p.sgroup = Some(self.pw.group(p.sgid));
                p.fgroup = Some(self.pw.group(p.fgid));
            }
        }
    }

    /// `simple_readproc`: one process from `path`, or `None` to skip it.
    fn readproc(&mut self, path: &Path, p: &mut Proc) -> bool {
        let Ok(meta) = fs::metadata(path) else {
            return false;
        };
        (p.euid, p.egid) = owner(&meta);
        if self.fill.stat {
            let Some(text) = file2str(path, "stat") else {
                return false;
            };
            stat2proc(&text, p, self.utf8);
        }
        self.read_status_and_names(path, p, true);
        if p.nlwp > 1 {
            p.wchan = u64::MAX;
        }
        if self.fill.usr {
            p.euser = Some(self.pw.user(p.euid));
        }
        if self.fill.grp {
            p.egroup = Some(self.pw.group(p.egid));
        }
        if self.fill.environ {
            p.environ = Some(environ_cvt(path, self.utf8));
        }
        if self.fill.cmdline {
            p.cmdline = Some(self.cmdline_cvt(path, p));
        }
        if self.fill.cgroup {
            let (cg, name) = cgroup_cvt(path, self.utf8);
            p.cgroup = Some(cg);
            p.cgname = Some(name);
        }
        if self.fill.oom {
            if let Some(t) = file2str(path, "oom_score") {
                scan_int(&t, &mut p.oom_score);
            }
            if let Some(t) = file2str(path, "oom_score_adj") {
                scan_int(&t, &mut p.oom_adj);
            }
        }
        if self.fill.ns {
            ns_read_pid(&self.root, p.tid, &mut p.ns);
        }
        if self.fill.systemd {
            p.sd = Some(b"?".to_vec());
        }
        if self.fill.lxc {
            p.lxcname = Some(lxc_name(path));
        }
        if self.fill.luid {
            p.luid = login_uid(path);
        }
        if self.fill.exe {
            p.exe = Some(readlink_exe(path, self.utf8));
        }
        if self.fill.autogrp {
            autogroup_fill(path, p);
        }
        !(self.hide_kernel && (p.ppid == 2 || p.tid == 2))
    }

    /// `simple_readtask`: one thread from `path`. The same files, in an order
    /// that differs from `readproc`'s only where it cannot be seen -- except
    /// that a thread's pending signals are its own.
    fn readtask(&mut self, path: &Path, t: &mut Proc) -> bool {
        let Ok(meta) = fs::metadata(path) else {
            return false;
        };
        (t.euid, t.egid) = owner(&meta);
        if self.fill.stat {
            let Some(text) = file2str(path, "stat") else {
                return false;
            };
            stat2proc(&text, t, self.utf8);
        }
        self.read_status_and_names(path, t, false);
        if self.fill.usr {
            t.euser = Some(self.pw.user(t.euid));
        }
        if self.fill.grp {
            t.egroup = Some(self.pw.group(t.egid));
        }
        if self.fill.cmdline {
            t.cmdline = Some(self.cmdline_cvt(path, t));
        }
        if self.fill.environ {
            t.environ = Some(environ_cvt(path, self.utf8));
        }
        if self.fill.cgroup {
            let (cg, name) = cgroup_cvt(path, self.utf8);
            t.cgroup = Some(cg);
            t.cgname = Some(name);
        }
        if self.fill.systemd {
            t.sd = Some(b"?".to_vec());
        }
        if self.fill.exe {
            t.exe = Some(readlink_exe(path, self.utf8));
        }
        if self.fill.oom {
            if let Some(x) = file2str(path, "oom_score") {
                scan_int(&x, &mut t.oom_score);
            }
            if let Some(x) = file2str(path, "oom_score_adj") {
                scan_int(&x, &mut t.oom_adj);
            }
        }
        if self.fill.ns {
            ns_read_pid(&self.root, t.tid, &mut t.ns);
        }
        if self.fill.lxc {
            t.lxcname = Some(lxc_name(path));
        }
        if self.fill.luid {
            t.luid = login_uid(path);
        }
        if self.fill.autogrp {
            autogroup_fill(path, t);
        }
        !(self.hide_kernel && (t.ppid == 2 || t.tid == 2))
    }

    /// `fill_cmdline_cvt`: the command line, or `[name]` when it is empty or
    /// unreadable -- the two are one case upstream, `read_unvectored`
    /// returning 0 for both. A process with no name at all is C's NULL
    /// there, which `snprintf` spells `(null)`.
    fn cmdline_cvt(&self, path: &Path, p: &Proc) -> Vec<u8> {
        let raw = read_raw(&path.join("cmdline"));
        let name = p.cmd.as_deref().unwrap_or(b"(null)");
        super::cmdline_cvt(raw.as_deref(), name, p.state, self.utf8)
    }

    /// `simple_nextpid`'s names: every entry whose name starts with a digit
    /// from 1 to 9 and is a number `strtoul` can read without overflow --
    /// leading digits are enough, so `12abc` is process 12. The PID is that
    /// number cut to `int`.
    fn pid_entries(&self) -> io::Result<Vec<i32>> {
        let mut out = Vec::new();
        for entry in fs::read_dir(&self.root)? {
            let Ok(entry) = entry else { continue };
            let name = crate::quote::os_bytes(&entry.file_name()).into_owned();
            if !matches!(name.first(), Some(b'1'..=b'9')) {
                continue;
            }
            // `errno` is set only by an overflow, which skips the entry.
            let (n, overflowed) = scanf::strtoul_overflow(&name);
            if !overflowed {
                out.push(scanf::low_i32(i64::from_le_bytes(n.to_le_bytes())));
            }
        }
        Ok(out)
    }

    /// The process directory for a PID: `/proc/%d`.
    fn pid_dir(&self, pid: i32) -> PathBuf {
        self.root.join(pid.to_string())
    }

    /// The processes one finder yields, each read by `readproc`.
    fn walk(&mut self, finder: Finder<'_>) -> io::Result<Vec<Proc>> {
        let mut out = Vec::new();
        for (pid, tgid) in self.found(finder)? {
            let mut p = Proc {
                tid: pid,
                tgid,
                ..Proc::default()
            };
            if self.readproc(&self.pid_dir(pid), &mut p) {
                out.push(p);
            }
        }
        Ok(out)
    }

    /// The `(tid, tgid)` pairs a finder starts each process with.
    fn found(&self, finder: Finder<'_>) -> io::Result<Vec<(i32, i32)>> {
        Ok(match finder {
            Finder::All => self.pid_entries()?.into_iter().map(|p| (p, p)).collect(),
            Finder::Listed(pids) => pids
                .iter()
                .take_while(|&&p| p != 0)
                .map(|&raw| {
                    let pid = scanf::low_i32(i64::from(raw));
                    // `listed_nextpid` looks up the real thread group: the
                    // number after `Tgid:` in `status`, wherever it is.
                    let tgid = file2str(&self.pid_dir(pid), "status")
                        .and_then(|t| {
                            let t = cstr(&t).to_vec();
                            t.windows(5).position(|w| w == b"Tgid:").map(|at| {
                                scanf::atoi(t.get(at.saturating_add(5)..).unwrap_or_default())
                            })
                        })
                        .unwrap_or(pid);
                    (pid, tgid)
                })
                .collect(),
        })
    }

    /// `procps_pids_reap(PIDS_FETCH_TASKS_ONLY)`'s walk: every process.
    ///
    /// # Errors
    ///
    /// `/proc` could not be listed.
    pub fn reap(&mut self) -> io::Result<Vec<Proc>> {
        self.walk(Finder::All)
    }

    /// `procps_pids_select(PIDS_SELECT_PID)`'s walk: the processes listed.
    pub fn select(&mut self, pids: &[u32]) -> Vec<Proc> {
        self.walk(Finder::Listed(pids)).unwrap_or_default()
    }

    /// `readeither`'s walk -- `PIDS_FETCH_THREADS_TOO`, or a selection with
    /// threads -- every thread of each process found, from its `task`
    /// directory; or, when this `/proc` has no `self/task`, each process
    /// alone.
    ///
    /// A thread that cannot be read ends its process: the threads after it
    /// in the directory are not looked at.
    ///
    /// # Errors
    ///
    /// `/proc` could not be listed (only when no PIDs are given).
    pub fn reap_threads(&mut self, pids: Option<&[u32]>) -> io::Result<Vec<Proc>> {
        let finder = pids.map_or(Finder::All, Finder::Listed);
        let task_dir_missing = fs::metadata(self.root.join("self/task")).is_err();
        let mut out = Vec::new();
        for (pid, tgid) in self.found(finder)? {
            if task_dir_missing {
                // `readeither` hands `readproc` its own zeroed record, not
                // the one the finder filled in, so the IDs start at 0 and
                // are only what `status` makes them.
                let _ = (pid, tgid); // as above: the finder's are not used
                let mut p = Proc::default();
                if self.readproc(&self.pid_dir(pid), &mut p) {
                    out.push(p);
                }
                continue;
            }
            let tasks = self.root.join(format!("{tgid}/task"));
            let Ok(dir) = fs::read_dir(&tasks) else {
                continue;
            };
            for entry in dir {
                let Ok(entry) = entry else { break };
                let name = crate::quote::os_bytes(&entry.file_name()).into_owned();
                if !matches!(name.first(), Some(b'1'..=b'9')) {
                    continue;
                }
                let (n, _) = scanf::strtoul(&name);
                let mut t = Proc {
                    tid: scanf::low_i32(i64::from_le_bytes(n.to_le_bytes())),
                    tgid,
                    ..Proc::default()
                };
                // `%.10s`: the path uses at most ten bytes of the name.
                let short = name.get(..name.len().min(10)).unwrap_or_default();
                let path = tasks.join(crate::quote::os_from_bytes(short));
                if !self.readtask(&path, &mut t) {
                    break;
                }
                out.push(t);
            }
        }
        Ok(out)
    }
}

/// `look_up_our_self`: whether `/proc/self/stat` can be read at all -- the
/// test `fatal_proc_unmounted` makes before anything else.
#[must_use]
pub fn self_stat_readable(root: &Path) -> bool {
    file2str(&root.join("self"), "stat").is_some()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    const STAT: &[u8] = b"42 (my (odd) name) S 1 42 42 34817 42 4194560 7 0 3 0 250 150 9 8 20 5 1 0 50000 20000000 1000 18446744073709551615 4194304 4198400 140737488347136 0 0 0 0 0 0 0 0 0 17 3 0 0 11 12 13\n";

    #[test]
    fn stat_fields_land_where_sscanf_puts_them() {
        let mut p = Proc::default();
        stat2proc(STAT, &mut p, true);
        assert_eq!(p.cmd.as_deref(), Some(&b"my (odd) name"[..]));
        assert_eq!(p.state, b'S');
        assert_eq!(
            (p.ppid, p.pgrp, p.session, p.tty, p.tpgid),
            (1, 42, 42, 34817, 42)
        );
        assert_eq!(p.flags, 4_194_560);
        assert_eq!((p.min_flt, p.cmin_flt, p.maj_flt, p.cmaj_flt), (7, 0, 3, 0));
        assert_eq!((p.utime, p.stime, p.cutime, p.cstime), (250, 150, 9, 8));
        assert_eq!((p.priority, p.nice, p.nlwp), (20, 5, 1));
        assert_eq!(p.start_time, 50000);
        assert_eq!((p.vsize, p.rss, p.rss_rlim), (20_000_000, 1000, u64::MAX));
        assert_eq!((p.start_code, p.end_code), (4_194_304, 4_198_400));
        assert_eq!(p.exit_signal, 17);
        assert_eq!(p.processor, 3);
        assert_eq!((p.rtprio, p.sched), (0, 0));
        assert_eq!((p.blkio_tics, p.gtime, p.cgtime), (11, 12, 13));
    }

    #[test]
    fn a_short_stat_keeps_the_defaults_after_where_it_stops() {
        let mut p = Proc::default();
        stat2proc(b"7 (x) R 1 2 junk", &mut p, true);
        assert_eq!(p.state, b'R');
        assert_eq!((p.ppid, p.pgrp, p.session), (1, 2, 0));
        assert_eq!((p.rtprio, p.sched, p.nlwp), (-1, -1, 1));
        // No parentheses: nothing at all, not even the name.
        let mut q = Proc::default();
        stat2proc(b"7 x R 1", &mut q, true);
        assert_eq!(q.cmd, None);
        assert_eq!((q.state, q.nlwp, q.rtprio), (0, 0, -1));
        // A `)` at the very end: the same.
        let mut r = Proc::default();
        stat2proc(b"7 (x)", &mut r, true);
        assert_eq!(r.cmd, None);
    }

    #[test]
    fn stat_numbers_wrap_as_c_types_do() {
        let mut p = Proc::default();
        stat2proc(b"1 (x) S 4294967297 -1 0 0 0 -5", &mut p, true);
        assert_eq!(p.ppid, 1);
        assert_eq!(p.pgrp, -1);
        assert_eq!(p.flags, u64::MAX - 4);
    }

    const STATUS: &[u8] = b"Name:\tbash\\nx\nUmask:\t0022\nState:\tR (running)\nTgid:\t40\nNgid:\t0\nPid:\t41\nPPid:\t1\nUid:\t1000\t1001\t1002\t1003\nGid:\t10\t11\t12\t13\nFDSize:\t256\nGroups:\t4 24 27 \nVmSize:\t  9000 kB\nVmRSS:\t2000 kB\nThreads:\t3\nSigQ:\t0/1\nSigPnd:\t0000000000000001\nShdPnd:\t0000000000000002\nSigBlk:\t0000000000010000\nSigIgn:\t0000000000384004\nSigCgt:\t000000004b813efb\n";

    #[test]
    fn status_lines_override_and_fill() {
        let mut p = Proc {
            state: b'S',
            ppid: 9,
            ..Proc::default()
        };
        status2proc(STATUS, &mut p, true, true);
        assert_eq!(p.cmd.as_deref(), Some(&b"bash?x"[..]));
        assert_eq!(p.state, b'R');
        assert_eq!((p.tgid, p.tid, p.nlwp, p.ppid), (40, 41, 3, 1));
        assert_eq!((p.ruid, p.euid, p.suid, p.fuid), (1000, 1001, 1002, 1003));
        assert_eq!((p.rgid, p.egid, p.sgid, p.fgid), (10, 11, 12, 13));
        assert_eq!(p.supgid.as_deref(), Some(&b"4,24,27"[..]));
        assert_eq!((p.vm_size, p.vm_rss), (9000, 2000));
        assert_eq!(p.signal, b"0000000000000002");
        assert_eq!(p.sigpnd, b"0000000000000001");
        assert_eq!(p.blocked, b"0000000000010000");
        assert_eq!(p.sigignore, b"0000000000384004");
        assert_eq!(p.sigcatch, b"000000004b813efb");
        // A thread reports its own pending signals.
        let mut t = Proc::default();
        status2proc(STATUS, &mut t, false, true);
        assert_eq!(t.signal, b"0000000000000001");
    }

    #[test]
    fn status_without_pid_or_threads_makes_the_pid_zero() {
        let mut p = Proc {
            tid: 5,
            tgid: 5,
            ..Proc::default()
        };
        status2proc(b"Name:\tx\nState:\tS (sleeping)\n", &mut p, true, true);
        assert_eq!((p.tid, p.tgid, p.nlwp), (0, 0, 1));
        assert_eq!(p.supgid.as_deref(), Some(&b"-"[..]));
    }

    #[test]
    fn status_stops_at_a_colon_without_a_tab() {
        let mut p = Proc::default();
        status2proc(b"Pid:\t3\nBroken: 1\nPPid:\t7\n", &mut p, true, true);
        assert_eq!(p.tid, 3);
        assert_eq!(p.ppid, 0);
    }

    #[test]
    fn a_short_signal_mask_takes_the_next_line_with_it() {
        let mut p = Proc::default();
        status2proc(b"SigBlk:\t12\nPid:\t1\n", &mut p, true, true);
        assert_eq!(p.blocked, b"12\nPid:\t1\n");
    }

    #[test]
    fn groups_on_the_last_line_without_a_newline_are_skipped() {
        let mut p = Proc::default();
        status2proc(b"Pid:\t1\nGroups:\t5 6", &mut p, true, true);
        assert_eq!(p.supgid.as_deref(), Some(&b"-"[..]));
        let mut q = Proc::default();
        status2proc(b"Groups:\t\t 5\t6\n", &mut q, true, true);
        assert_eq!(q.supgid.as_deref(), Some(&b"5\t6"[..]));
    }

    #[test]
    fn group_names_from_the_ids() {
        let db = pwdb::Db::from_bytes(b"", b"wheel:x:10:\nstaff:x:50:\n");
        let mut pw = Pwcache::with_db(db);
        let mut p = Proc {
            supgid: Some(b"10,50,77".to_vec()),
            ..Proc::default()
        };
        supgrps_from_supgids(&mut p, &mut pw);
        assert_eq!(p.supgrp.as_deref(), Some(&b"wheel,staff,77"[..]));
        let mut q = Proc {
            supgid: Some(b"-".to_vec()),
            ..Proc::default()
        };
        supgrps_from_supgids(&mut q, &mut pw);
        assert_eq!(q.supgrp.as_deref(), Some(&b"-"[..]));
        let mut r = Proc {
            supgid: Some(b"x".to_vec()),
            ..Proc::default()
        };
        supgrps_from_supgids(&mut r, &mut pw);
        assert_eq!(r.supgrp.as_deref(), Some(&b"-"[..]));
    }

    #[test]
    fn io_and_smaps() {
        let mut p = Proc::default();
        io2proc(
            b"rchar: 1\nwchar: 2\nsyscr: 3\nsyscw: 4\nread_bytes: 5\nwrite_bytes: 6\ncancelled_write_bytes: 7\n",
            &mut p,
        );
        assert_eq!(
            (
                p.rchar,
                p.wchar,
                p.syscr,
                p.syscw,
                p.read_bytes,
                p.write_bytes,
                p.cancelled_write_bytes
            ),
            (1, 2, 3, 4, 5, 6, 7)
        );
        let mut q = Proc::default();
        smaps2proc(
            b"55-66 ---p 0 00:00 0 [rollup]\nRss:  100 kB\nPss:  50 kB\nPrivate_Clean: 3 kB\nPrivate_Dirty: 4 kB\n",
            &mut q,
        );
        assert_eq!(q.smap[0], 100);
        assert_eq!(q.smap[SMAP_PSS], 50);
        assert_eq!(q.smap[SMAP_PRIVATE_CLEAN], 3);
        assert_eq!(q.smap[SMAP_PRIVATE_DIRTY], 4);
        // Out of order: Pss before Rss is never seen, because the search
        // for Pss starts after where Rss was found.
        let mut r = Proc::default();
        smaps2proc(b"Pss: 9 kB\nRss: 8 kB\n", &mut r);
        assert_eq!((r.smap[0], r.smap[SMAP_PSS]), (8, 0));
    }
}
