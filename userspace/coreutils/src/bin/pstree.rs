//! `pstree` -- display a tree of processes: psmisc 23.7's, ported.
//!
//! ```text
//! pstree [-acglpsStTuZ] [ -h | -H PID ] [ -n | -N type ] [ -A | -G | -U ] [ PID | USER ]
//! ```
//!
//! A transcription of `src/pstree.c` as Ubuntu 24.04 builds it (no patches;
//! SELinux support, which there finds SELinux off). Every process in
//! `/proc` -- and every thread, as `{name}` under its process -- hangs under
//! its parent, the children sorted by name and uid (or by pid, `-n`), and
//! the tree is drawn from `init`, from a pid, or from each top-most process
//! of a user. What upstream does and this keeps:
//!
//! - **The reading** (`read_proc`): `/proc` in its own order; a process's
//!   name between the first `(` and the last `)` of its `stat`; the owner of
//!   its directory as its uid; its threads, from `task/`, before the process
//!   itself -- so a thread's parent is first a placeholder `?`, renamed when
//!   the process arrives, with one bubble pass over its siblings; with `-a`,
//!   its command line, read no further than the line can show. Whatever has
//!   no parent in the list ends up under the root.
//! - **The drawing** (`dump_tree`): identical sibling subtrees compacted to
//!   `N*[name]` (threads too, `N*[{name}]`); `-p`, `-g`, `-u`, `-S` and `-Z`
//!   in parentheses, or after a comma under `-a`; names escaped (`\ooo`,
//!   `\\`); the line cut at the terminal's width (`COLUMNS`, else the
//!   window's, else 132) with a `+`, a UTF-8 character counted by its first
//!   byte -- unless `-l`. Upstream's arithmetic too: `out_int` writes
//!   nothing at all for 0 (a process group of 0 under `-g` is `name()`),
//!   and the brackets `-a` closes a compacted thread with come one short,
//!   because `while (closing--)` leaves -1 behind it.
//! - **The symbols**: UTF-8 line drawing when standard output is a terminal
//!   and the locale's codeset is UTF-8, else ASCII; `-A`, `-G` (VT100) and
//!   `-U` choose. On a terminal with another codeset, `setupterm` is asked
//!   (and its answer not used -- upstream settles on ASCII either way), so
//!   an unknown, generic or hard-copy `TERM` ends the run with its
//!   complaint. `-h` and `-H PID` embolden a process and its ancestors with
//!   the terminal's `md` and `me` as `tgetent` and `tputs` give them
//!   (userspace/terminfo), and `-C age` colours each by its age.
//! - **`-N TYPE`** draws one tree per namespace of that type, each headed by
//!   its inode number; **`-s PID`** draws only PID's ancestors and itself.
//! - **Standard output is stdio's**, its failure unreported, as upstream
//!   never checks it; `-V` and the usage go to standard error. Run as
//!   `pstree.x11`, it waits for a line of input before it exits.
//!
//! # Deliberate differences
//!
//! - `-V` names SlateOS coreutils, as every program here does.
//! - **No SELinux library is asked** for `-Z`: SlateOS has none, so the
//!   context is `/proc/PID/attr/current`'s -- upstream's fallback when
//!   `libselinux` says SELinux is off, as it does on the reference.
//! - **Numbers past nine digits** in `-u`'s uid fallback: upstream's
//!   `out_int` multiplies its divisor until it wraps, prints the digits that
//!   arithmetic gives, and divides by zero for some; here the wrapping is
//!   kept and the division by zero ends the number instead.

use std::ffi::OsString;
use std::io::Read;
use std::process::ExitCode;

use coreutils::getopt::{Opt, Program, Takes};
use coreutils::procps::scanf;
use coreutils::quote::{escape_unprintable, os_bytes};
use coreutils::stdfd;

coreutils::guard_std_fds!();

/// The parser. Its complaints name the program by `argv[0]`, as glibc's do.
const PSTREE: Program = Program::new("pstree", 1);

/// Upstream's `getopt_long` string.
const SHORT_OPTIONS: &str = "aAcC:GhH:nN:pglsStTuUVZ";

/// Upstream's `options`, in its order.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("arguments", Takes::Nothing),
    ("ascii", Takes::Nothing),
    ("compact-not", Takes::Nothing),
    ("color", Takes::Required),
    ("vt100", Takes::Nothing),
    ("highlight-all", Takes::Nothing),
    ("highlight-pid", Takes::Required),
    ("long", Takes::Nothing),
    ("numeric-sort", Takes::Nothing),
    ("ns-sort", Takes::Required),
    ("show-pids", Takes::Nothing),
    ("show-pgids", Takes::Nothing),
    ("show-parents", Takes::Nothing),
    ("ns-changes", Takes::Nothing),
    ("thread-names", Takes::Nothing),
    ("hide-threads", Takes::Nothing),
    ("uid-changes", Takes::Nothing),
    ("unicode", Takes::Nothing),
    ("version", Takes::Nothing),
    ("security-context", Takes::Nothing),
];

/// `COMM_LEN`.
const COMM_LEN: usize = 64;
/// What `strncpy (comm, src, COMM_LEN + 2)` and the NUL at `COMM_LEN + 1`
/// leave of a name.
const COMM_MAX: usize = COMM_LEN + 1;
/// `BUFSIZ`.
const BUFSIZ: usize = 8192;
/// `DEFAULT_ROOT_PID` on Linux.
const DEFAULT_ROOT_PID: i32 = 1;

/// The namespace types, in `ns_names[]`'s order.
const NS_NAMES: [&str; 8] = ["cgroup", "ipc", "mnt", "net", "pid", "user", "uts", "time"];
/// `NUM_NS`.
const NUM_NS: usize = NS_NAMES.len();

/// `PFLAG_HILIGHT`.
const PFLAG_HILIGHT: u8 = 0x01;
/// `PFLAG_THREAD`.
const PFLAG_THREAD: u8 = 0x02;

/// One of the three symbol sets.
struct Sym {
    empty_2: &'static [u8],
    branch_2: &'static [u8],
    vert_2: &'static [u8],
    last_2: &'static [u8],
    single_3: &'static [u8],
    first_3: &'static [u8],
}

static SYM_ASCII: Sym = Sym {
    empty_2: b"  ",
    branch_2: b"|-",
    vert_2: b"| ",
    last_2: b"`-",
    single_3: b"---",
    first_3: b"-+-",
};

/// U+2502, U+251C, U+2500, U+2514 and U+252C.
static SYM_UTF: Sym = Sym {
    empty_2: b"  ",
    branch_2: "\u{251c}\u{2500}".as_bytes(),
    vert_2: "\u{2502} ".as_bytes(),
    last_2: "\u{2514}\u{2500}".as_bytes(),
    single_3: "\u{2500}\u{2500}\u{2500}".as_bytes(),
    first_3: "\u{2500}\u{252c}\u{2500}".as_bytes(),
};

/// The VT100 graphic set, switched to and back for each symbol.
static SYM_VT100: Sym = Sym {
    empty_2: b"  ",
    branch_2: b"\x1b(0\x0ftq\x1b(B",
    vert_2: b"\x1b(0\x0fx\x1b(B ",
    last_2: b"\x1b(0\x0fmq\x1b(B",
    single_3: b"\x1b(0\x0fqqq\x1b(B",
    first_3: b"\x1b(0\x0fqwq\x1b(B",
};

/// `age_to_color[]`: green under a minute, yellow under an hour, red after.
const AGE_TO_COLOR: [(u32, &[u8]); 3] = [(60, b"\x1b[32m"), (3600, b"\x1b[33m"), (0, b"\x1b[31m")];

/// `struct _proc`.
#[derive(Clone)]
struct Proc {
    comm: Vec<u8>,
    /// `argv`: with `-a`, the arguments after the first.
    argv: Vec<Vec<u8>>,
    /// `argc`: their number, or -1 for a process whose command line is
    /// empty -- one "swapped out", in upstream's word.
    argc: i32,
    pid: i32,
    pgid: i32,
    uid: u32,
    ns: [u64; NUM_NS],
    flags: u8,
    age: f64,
    /// `children`, in the `CHILD` list's order.
    children: Vec<usize>,
    parent: Option<usize>,
}

/// `struct ns_entry`.
struct NsEntry {
    number: u64,
    children: Vec<usize>,
}

/// A run that has ended with this status, its diagnostic already out.
#[derive(Debug)]
struct Die(u8);

/// The run: upstream's file-scope statics, and the standard output.
#[allow(
    clippy::struct_excessive_bools,
    reason = "upstream's flags, one each, kept as it has them"
)]
struct Pstree<'o> {
    /// Every process, in the order `new_proc` made them. Upstream's `list`
    /// is this, newest first.
    procs: Vec<Proc>,
    print_args: bool,
    compact: bool,
    user_change: bool,
    pids: bool,
    pgids: bool,
    by_pid: bool,
    trunc: bool,
    ns_change: bool,
    thread_names: bool,
    hide_threads: bool,
    show_scontext: bool,
    color_age: bool,
    output_width: i32,
    cur_x: i32,
    charlen: i32,
    width: Vec<i32>,
    more: Vec<i32>,
    dumped: bool,
    sym: &'static Sym,
    /// The terminal `tgetstr` asks: the last one `setupterm` or `tgetent`
    /// set up.
    terminal: Option<terminfo::Termcap>,
    passwd: Option<pwdb::Db>,
    out: &'o mut ulclosestream::Stdout,
}

impl<'o> Pstree<'o> {
    /// A run with upstream's defaults, writing to `out`.
    fn new(out: &'o mut ulclosestream::Stdout, output_width: i32) -> Self {
        Pstree {
            procs: Vec::new(),
            print_args: false,
            compact: true,
            user_change: false,
            pids: false,
            pgids: false,
            by_pid: false,
            trunc: true,
            ns_change: false,
            thread_names: false,
            hide_threads: false,
            show_scontext: false,
            color_age: false,
            output_width,
            cur_x: 1,
            charlen: 0,
            width: Vec::new(),
            more: Vec::new(),
            dumped: false,
            sym: &SYM_ASCII,
            terminal: None,
            passwd: None,
            out,
        }
    }

    /// `putchar`, and every other write that does not count columns.
    fn put_bytes(&mut self, s: &[u8]) {
        self.out.write(s);
    }

    /// `out_char`: one byte, its column counted once per character, and the
    /// line cut with a `+` at the width.
    fn out_char(&mut self, c: u8) {
        if self.charlen == 0 {
            self.charlen = if c & 0x80 == 0 {
                1
            } else if c & 0xe0 == 0xc0 {
                2
            } else if c & 0xf0 == 0xe0 {
                3
            } else if c & 0xf8 == 0xf0 {
                4
            } else {
                1
            };
            // "count first byte of whatever it is only"
            self.cur_x = self.cur_x.saturating_add(1);
        }
        self.charlen = self.charlen.saturating_sub(1);
        if !self.trunc || self.cur_x <= self.output_width {
            self.put_bytes(&[c]);
        } else if self.cur_x == self.output_width.saturating_add(1) {
            self.put_bytes(b"+");
        }
    }

    fn out_string(&mut self, s: &[u8]) {
        for &c in s {
            self.out_char(c);
        }
    }

    /// `out_int`: "non-negative integers only", with C's `int` arithmetic
    /// -- 0 counts a digit and writes none. The digits it counted.
    fn out_int(&mut self, x: i32) -> i32 {
        let mut digits: i32 = 0;
        let mut div: i32 = 1;
        // `for (div = 1; x / div; div *= 10)`: the divisor wraps as an `int`
        // does; where it wraps to 0 upstream divides by zero, and the count
        // stops here.
        while x.checked_div(div).is_some_and(|q| q != 0) {
            digits = digits.saturating_add(1);
            div = div.wrapping_mul(10);
        }
        if digits == 0 {
            digits = 1;
        }
        div = div.wrapping_div(10);
        while div != 0 {
            // `'0' + (x / div) % 10`, for a remainder from -9 to 9.
            let d = x.checked_div(div).unwrap_or(0).wrapping_rem(10);
            self.out_char(48i32.wrapping_add(d) as u8);
            div = div.wrapping_div(10);
        }
        digits
    }

    /// `out_newline`.
    fn out_newline(&mut self) {
        self.put_bytes(b"\n");
        self.cur_x = 1;
    }

    /// `reset_color`.
    fn reset_color(&mut self) {
        if self.color_age {
            self.put_bytes(b"\x1b[0m");
        }
    }

    /// `print_proc_color`: the colour of a process `process_age` seconds
    /// old -- compared, as upstream compares it, as an `unsigned`.
    fn print_proc_color(&mut self, process_age: i32) {
        if !self.color_age {
            return;
        }
        let age = process_age as u32;
        let color = AGE_TO_COLOR
            .iter()
            .find(|(secs, _)| *secs == 0 || age < *secs)
            .map_or(&b""[..], |(_, c)| c);
        self.put_bytes(color);
    }

    /// `tgetstr (id)` through `tputs`: what the highlight writes -- padding
    /// and all -- or nothing with no terminal set up.
    fn termcap_string(&self, index: usize) -> Option<Vec<u8>> {
        let terminal = self.terminal.as_ref()?;
        Some(terminal.tputs(terminal.string(index)?))
    }

    /// `out_args`: `\\` for a backslash, printable ASCII as it is, and
    /// every other byte as `\ooo`. The columns it took.
    fn out_args(&mut self, s: &[u8]) -> i32 {
        let mut count: i32 = 0;
        for &c in c_str(s) {
            if c == b'\\' {
                self.out_string(b"\\\\");
                count = count.saturating_add(2);
            } else if (b' '..=b'~').contains(&c) {
                self.out_char(c);
                count = count.saturating_add(1);
            } else {
                self.out_string(format!("\\{c:03o}").as_bytes());
                count = count.saturating_add(4);
            }
        }
        count
    }

    /// `out_scontext`: the security context, in backquote and quote.
    fn out_scontext(&mut self, pid: i32) {
        self.out_string(b"`");
        if let Ok(data) = std::fs::read(format!("/proc/{pid}/attr/current")) {
            // `fgets (readbuf, BUFSIZ, file)`, and its last byte dropped.
            let mut line = fgets_line(&data);
            if !line.is_empty() {
                line.truncate(c_str(&line).len());
                line.pop();
                self.out_string(&line);
            }
        }
        self.out_string(b"'");
    }

    /// `find_proc`: the newest with that pid.
    fn find_proc(&self, pid: i32) -> Option<usize> {
        self.procs.iter().rposition(|p| p.pid == pid)
    }

    /// `new_proc`, with `new_proc_ns`.
    fn new_proc(&mut self, comm: &[u8], pid: i32, uid: u32) -> usize {
        let mut ns = [0u64; NUM_NS];
        for (slot, name) in ns.iter_mut().zip(NS_NAMES) {
            *slot = std::fs::metadata(format!("/proc/{pid}/ns/{name}"))
                .map(|m| inode_of(&m))
                .unwrap_or(0);
        }
        self.procs.push(Proc {
            comm: cut_comm(comm),
            argv: Vec::new(),
            argc: 0,
            pid,
            pgid: 0,
            uid,
            ns,
            flags: 0,
            age: 0.0,
            children: Vec::new(),
            parent: None,
        });
        self.procs.len().saturating_sub(1)
    }

    /// `add_child`: `child` among `parent`'s children, before the first
    /// that sorts after it.
    fn add_child(&mut self, parent: usize, child: usize) {
        let Some(c) = self.procs.get(child) else {
            return;
        };
        let (cpid, ccomm, cuid) = (c.pid, c.comm.clone(), c.uid);
        let siblings = self
            .procs
            .get(parent)
            .map(|p| p.children.clone())
            .unwrap_or_default();
        let at = siblings
            .iter()
            .position(|&s| {
                self.procs.get(s).is_some_and(|w| {
                    if self.by_pid {
                        w.pid > cpid
                    } else {
                        match w.comm.as_slice().cmp(ccomm.as_slice()) {
                            std::cmp::Ordering::Greater => true,
                            std::cmp::Ordering::Equal => w.uid > cuid,
                            std::cmp::Ordering::Less => false,
                        }
                    }
                })
            })
            .unwrap_or(siblings.len());
        if let Some(p) = self.procs.get_mut(parent) {
            p.children.insert(at, child);
        }
    }

    /// `set_args`: the arguments after the first, from a command line of
    /// `size` bytes ending in a NUL.
    fn set_args(&mut self, this: usize, args: &[u8], size: usize) {
        let Some(p) = self.procs.get_mut(this) else {
            return;
        };
        if size == 0 {
            p.argc = -1;
            return;
        }
        let args = args.get(..size).unwrap_or(args);
        let last = size.saturating_sub(1);
        let mut argc: i32 = 0;
        let mut i = 0usize;
        while i < last {
            if args.get(i) == Some(&0) {
                argc = argc.saturating_add(1);
                // "now skip consecutive NUL"
                while args.get(i) == Some(&0) && i < last {
                    i = i.saturating_add(1);
                }
            }
            i = i.saturating_add(1);
        }
        p.argc = argc;
        if argc == 0 {
            return;
        }
        // `start = strchr (args, 0) + 1`, and each word's NUL after it.
        let first_nul = args.iter().position(|&b| b == 0).unwrap_or(args.len());
        let mut rest = args.get(first_nul.saturating_add(1)..).unwrap_or_default();
        let mut argv = Vec::new();
        for _ in 0..argc {
            let end = rest.iter().position(|&b| b == 0).unwrap_or(rest.len());
            argv.push(rest.get(..end).unwrap_or_default().to_vec());
            rest = rest.get(end.saturating_add(1)..).unwrap_or_default();
        }
        p.argv = argv;
    }

    /// `rename_proc`: the placeholder named, and one bubble pass over its
    /// siblings by name.
    fn rename_proc(&mut self, this: usize, comm: &[u8], uid: u32) {
        let Some(p) = self.procs.get_mut(this) else {
            return;
        };
        p.comm = cut_comm(comm);
        p.uid = uid;
        let parent = p.parent;
        let Some(parent) = parent.filter(|_| !self.by_pid) else {
            return;
        };
        let mut kids = self
            .procs
            .get(parent)
            .map(|p| p.children.clone())
            .unwrap_or_default();
        for k in 1..kids.len() {
            let a = kids
                .get(k.saturating_sub(1))
                .and_then(|&a| self.procs.get(a));
            let b = kids.get(k).and_then(|&b| self.procs.get(b));
            if let (Some(a), Some(b)) = (a, b)
                && a.comm > b.comm
            {
                kids.swap(k.saturating_sub(1), k);
            }
        }
        if let Some(p) = self.procs.get_mut(parent) {
            p.children = kids;
        }
    }

    /// `add_proc`.
    #[allow(clippy::too_many_arguments, reason = "upstream's signature")]
    fn add_proc(
        &mut self,
        comm: &[u8],
        pid: i32,
        mut ppid: i32,
        pgid: i32,
        uid: u32,
        args: Option<(&[u8], usize)>,
        isthread: bool,
        age: f64,
    ) {
        let this = match self.find_proc(pid) {
            Some(t) => {
                self.rename_proc(t, comm, uid);
                t
            }
            None => self.new_proc(comm, pid, uid),
        };
        if let Some((a, size)) = args {
            self.set_args(this, a, size);
        }
        if pid == ppid {
            ppid = 0;
        }
        if let Some(p) = self.procs.get_mut(this) {
            p.pgid = pgid;
            p.age = age;
            if isthread {
                p.flags |= PFLAG_THREAD;
            }
        }
        let parent = match self.find_proc(ppid) {
            Some(p) => p,
            None => self.new_proc(b"?", ppid, 0),
        };
        if pid != 0 {
            self.add_child(parent, this);
            if let Some(p) = self.procs.get_mut(this) {
                p.parent = Some(parent);
            }
        }
    }

    /// `tree_equal`.
    fn tree_equal(&self, a: usize, b: usize) -> bool {
        let (Some(pa), Some(pb)) = (self.procs.get(a), self.procs.get(b)) else {
            return false;
        };
        pa.comm == pb.comm
            && (!self.user_change || pa.uid == pb.uid)
            && (!self.ns_change || pa.ns == pb.ns)
            && pa.children.len() == pb.children.len()
            && pa
                .children
                .iter()
                .zip(&pb.children)
                .all(|(&x, &y)| self.tree_equal(x, y))
    }

    /// The compaction scan: the children after the `from`th of `parent`
    /// that are the same tree as it, taken out of the list. How many.
    fn compact_after(&mut self, parent: usize, from: usize) -> i32 {
        let kids = self
            .procs
            .get(parent)
            .map(|p| p.children.clone())
            .unwrap_or_default();
        let Some(&first) = kids.get(from) else {
            return 0;
        };
        let mut kept = kids.get(..=from).unwrap_or_default().to_vec();
        let mut count: i32 = 0;
        for &k in kids.get(from.saturating_add(1)..).unwrap_or_default() {
            if self.tree_equal(first, k) {
                count = count.saturating_add(1);
            } else {
                kept.push(k);
            }
        }
        if let Some(p) = self.procs.get_mut(parent) {
            p.children = kept;
        }
        count
    }

    /// `ensure_buffer_capacity`, for a `width` and `more` that grow as Rust
    /// grows them.
    fn ensure(&mut self, index: usize) {
        if index >= self.width.len() {
            self.width.resize(index.saturating_add(1), 0);
            self.more.resize(index.saturating_add(1), 0);
        }
    }

    /// `dump_tree`.
    #[allow(
        clippy::too_many_lines,
        clippy::too_many_arguments,
        reason = "upstream's dump_tree, in one piece so it reads against it"
    )]
    fn dump_tree(
        &mut self,
        current: Option<usize>,
        level: usize,
        rep: i32,
        leaf: bool,
        last: bool,
        prev_uid: u32,
        closing: i32,
    ) {
        let Some(cur) = current else {
            return;
        };
        let Some(proc_) = self.procs.get(cur).cloned() else {
            return;
        };
        let mut closing = closing;
        if !leaf {
            for lvl in 0..level {
                let w = self.width.get(lvl).copied().unwrap_or(0);
                for _ in 0..w.saturating_add(1).max(0) {
                    self.out_char(b' ');
                }
                let sym = self.sym;
                let s = if lvl.saturating_add(1) == level {
                    if last { sym.last_2 } else { sym.branch_2 }
                } else if self.more.get(lvl.saturating_add(1)).copied().unwrap_or(0) != 0 {
                    sym.vert_2
                } else {
                    sym.empty_2
                };
                self.out_string(s);
            }
        }

        let add = if rep < 2 {
            0
        } else {
            let digits = self.out_int(rep);
            self.out_string(b"*[");
            digits.saturating_add(2)
        };
        // `print_proc_color (current->age)`: the double made an `int`.
        self.print_proc_color(proc_.age as i32);
        let hilight = proc_.flags & PFLAG_HILIGHT != 0;
        if hilight && let Some(bold) = self.termcap_string(terminfo::string::ENTER_BOLD_MODE) {
            self.put_bytes(&bold);
        }
        let swapped = self.print_args;
        let mut info: i32 = i32::from(self.print_args);
        if swapped && proc_.argc < 0 {
            self.out_char(b'(');
        }
        let comm_len = self.out_args(&proc_.comm);
        let offset = self.cur_x;
        if self.pids {
            self.separator(&mut info);
            self.out_int(proc_.pid);
        }
        if self.pgids {
            self.separator(&mut info);
            self.out_int(proc_.pgid);
        }
        if self.user_change && prev_uid != proc_.uid {
            self.separator(&mut info);
            let name = self
                .passwd
                .get_or_insert_with(pwdb::Db::load)
                .user_by_uid(proc_.uid)
                .map(|u| u.name.clone());
            match name {
                Some(n) => self.out_string(&n),
                None => {
                    self.out_int(proc_.uid as i32);
                }
            }
        }
        if self.ns_change
            && let Some(parent) = proc_.parent
        {
            let pns = self.procs.get(parent).map(|p| p.ns).unwrap_or_default();
            for ((&mine, &theirs), name) in proc_.ns.iter().zip(&pns).zip(NS_NAMES) {
                if mine == 0 || theirs == 0 {
                    continue;
                }
                if mine != theirs {
                    self.separator(&mut info);
                    self.out_string(name.as_bytes());
                }
            }
        }
        if self.show_scontext {
            self.separator(&mut info);
            self.out_scontext(proc_.pid);
        }
        if (swapped && self.print_args && proc_.argc < 0) || (!swapped && info > 0) {
            self.out_char(b')');
        }
        if hilight && let Some(sgr0) = self.termcap_string(terminfo::string::EXIT_ATTRIBUTE_MODE) {
            self.put_bytes(&sgr0);
        }
        if self.print_args {
            let argc = usize::try_from(proc_.argc).unwrap_or(0);
            for (i, arg) in proc_.argv.iter().enumerate().take(argc) {
                let last_arg = i.saturating_add(1) == argc;
                if !last_arg {
                    // "Space between words but not at the end of last"
                    self.out_char(b' ');
                }
                let len = c_str(arg)
                    .iter()
                    .map(|&c| if (b' '..=b'~').contains(&c) { 1i32 } else { 4 })
                    .fold(0i32, i32::saturating_add);
                let room = self
                    .output_width
                    .saturating_sub(if last_arg { 0 } else { 4 });
                if self.cur_x.saturating_add(len) <= room || !self.trunc {
                    self.out_args(arg);
                } else {
                    self.out_string(b"...");
                    break;
                }
            }
        }
        self.reset_color();
        if self.show_scontext || self.print_args || proc_.children.is_empty() {
            while closing > 0 {
                self.out_char(b']');
                closing = closing.saturating_sub(1);
            }
            // `while (closing--)` leaves -1 behind it, which the threads
            // below are given their brackets on top of.
            closing = -1;
            self.out_newline();
        }
        self.ensure(level);
        if let Some(m) = self.more.get_mut(level) {
            *m = i32::from(!last);
        }

        let below = level.saturating_add(1);
        if self.show_scontext || self.print_args {
            if let Some(w) = self.width.get_mut(level) {
                *w = i32::from(swapped).saturating_add(if comm_len > 1 { 0 } else { -1 });
            }
            let mut k = 0usize;
            loop {
                let kids = self
                    .procs
                    .get(cur)
                    .map(|p| p.children.clone())
                    .unwrap_or_default();
                let Some(&child) = kids.get(k) else {
                    break;
                };
                let is_thread = self
                    .procs
                    .get(child)
                    .is_some_and(|p| p.flags & PFLAG_THREAD != 0);
                if self.compact && is_thread {
                    let count = self.compact_after(cur, k);
                    let n = self.procs.get(cur).map_or(0, |p| p.children.len());
                    self.dump_tree(
                        Some(child),
                        below,
                        count.saturating_add(1),
                        false,
                        k.saturating_add(1) >= n,
                        proc_.uid,
                        closing.saturating_add(if count > 0 { 2 } else { 1 }),
                    );
                } else {
                    self.dump_tree(
                        Some(child),
                        below,
                        1,
                        false,
                        k.saturating_add(1) >= kids.len(),
                        proc_.uid,
                        0,
                    );
                }
                k = k.saturating_add(1);
            }
            return;
        }
        let w = comm_len
            .saturating_add(self.cur_x)
            .saturating_sub(offset)
            .saturating_add(add);
        if let Some(slot) = self.width.get_mut(level) {
            *slot = w;
        }
        if self.cur_x >= self.output_width && self.trunc {
            let first3 = self.sym.first_3;
            self.out_string(first3);
            self.out_string(b"+");
            self.out_newline();
            return;
        }
        let mut first = true;
        let mut k = 0usize;
        loop {
            if k >= self.procs.get(cur).map_or(0, |p| p.children.len()) {
                break;
            }
            let count = if self.compact {
                self.compact_after(cur, k)
            } else {
                0
            };
            let kids = self
                .procs
                .get(cur)
                .map(|p| p.children.clone())
                .unwrap_or_default();
            let Some(&child) = kids.get(k) else {
                break;
            };
            let has_next = k.saturating_add(1) < kids.len();
            if first {
                let s = if has_next {
                    self.sym.first_3
                } else {
                    self.sym.single_3
                };
                self.out_string(s);
                first = false;
            }
            self.dump_tree(
                Some(child),
                below,
                count.saturating_add(1),
                k == 0,
                !has_next,
                proc_.uid,
                closing.saturating_add(i32::from(count > 0)),
            );
            k = k.saturating_add(1);
        }
    }

    /// `out_char (info++ ? ',' : '(')`: the separator before an attribute.
    fn separator(&mut self, info: &mut i32) {
        self.out_char(if *info > 0 { b',' } else { b'(' });
        *info = info.saturating_add(1);
    }

    /// `dump_by_user`.
    fn dump_by_user(&mut self, current: Option<usize>, uid: u32) {
        let Some(cur) = current else {
            return;
        };
        if self.procs.get(cur).is_some_and(|p| p.uid == uid) {
            if self.dumped {
                self.put_bytes(b"\n");
            }
            self.dump_tree(Some(cur), 0, 1, true, true, uid, 0);
            self.dumped = true;
            return;
        }
        let kids = self
            .procs
            .get(cur)
            .map(|p| p.children.clone())
            .unwrap_or_default();
        for k in kids {
            self.dump_by_user(Some(k), uid);
        }
    }

    /// `find_ns_and_add`: `r` under its namespace's entry, out of its
    /// parent's children.
    fn find_ns_and_add(&mut self, root: &mut Vec<NsEntry>, r: usize, id: usize) {
        let number = self
            .procs
            .get(r)
            .and_then(|p| p.ns.get(id).copied())
            .unwrap_or(0);
        match root.iter_mut().find(|e| e.number == number) {
            Some(e) => e.children.push(r),
            None => root.push(NsEntry {
                number,
                children: vec![r],
            }),
        }
        if let Some(parent) = self.procs.get(r).and_then(|p| p.parent) {
            if let Some(p) = self.procs.get_mut(parent)
                && let Some(i) = p.children.iter().position(|&c| c == r)
            {
                p.children.remove(i);
            }
            if let Some(p) = self.procs.get_mut(r) {
                p.parent = None;
            }
        }
    }

    /// `sort_by_namespace`.
    fn sort_by_namespace(&mut self, r: Option<usize>, id: usize, root: &mut Vec<NsEntry>) {
        // "first run, find the first process"
        let Some(r) = r.or_else(|| self.find_proc(1)) else {
            return;
        };
        let differs = match self.procs.get(r).and_then(|p| p.parent) {
            None => true,
            Some(parent) => {
                let theirs = self.procs.get(parent).and_then(|p| p.ns.get(id).copied());
                let mine = self.procs.get(r).and_then(|p| p.ns.get(id).copied());
                theirs != mine
            }
        };
        if differs {
            self.find_ns_and_add(root, r, id);
        }
        let kids = self
            .procs
            .get(r)
            .map(|p| p.children.clone())
            .unwrap_or_default();
        for k in kids {
            self.sort_by_namespace(Some(k), id, root);
        }
    }

    /// `dump_by_namespace`: each namespace's number, then its trees -- the
    /// number written through `out_string`, so that its newline is counted
    /// as a column the first line then lacks.
    fn dump_by_namespace(&mut self, root: &[NsEntry]) {
        for e in root {
            // `snprintf (buff, 14, "[%li]\n", (long int) number)`.
            let mut head = format!("[{}]\n", e.number as i64).into_bytes();
            head.truncate(13);
            self.out_string(&head);
            for &c in &e.children {
                self.dump_tree(Some(c), 0, 1, true, true, 0, 0);
            }
        }
    }

    /// `trim_tree_by_parent`.
    fn trim_tree_by_parent(&mut self, current: usize) {
        let mut cur = current;
        while let Some(parent) = self.procs.get(cur).and_then(|p| p.parent) {
            if let Some(p) = self.procs.get_mut(parent) {
                p.children.clear();
            }
            self.add_child(parent, cur);
            cur = parent;
        }
    }

    /// `fix_orphans`: what has no parent hung under the root, newest first.
    fn fix_orphans(&mut self, root_pid: i32) {
        let root = match self.find_proc(root_pid) {
            Some(r) => r,
            None => self.new_proc(b"?", root_pid, 0),
        };
        for walk in (0..self.procs.len()).rev() {
            let Some(p) = self.procs.get(walk) else {
                continue;
            };
            if p.pid == 1 || p.pid == 0 || p.parent.is_some() {
                continue;
            }
            self.add_child(root, walk);
            if let Some(p) = self.procs.get_mut(walk) {
                p.parent = Some(root);
            }
        }
    }

    /// `get_threadname`: `{name}`, from the thread's own `stat` with `-t`.
    fn get_threadname(&self, pid: i32, tid: i32, comm: &[u8]) -> Vec<u8> {
        let braced = |name: &[u8]| -> Vec<u8> {
            let name = c_str(name);
            let name = name.get(..name.len().min(COMM_LEN)).unwrap_or(name);
            [&b"{"[..], name, b"}"].concat()
        };
        if self.thread_names
            && let Ok(data) = std::fs::read(format!("/proc/{pid}/task/{tid}/stat"))
        {
            let line = fgets_line(&data);
            let line = c_str(&line);
            if let Some((open, close)) = comm_bounds(line) {
                return braced(line.get(open.saturating_add(1)..close).unwrap_or_default());
            }
        }
        braced(comm)
    }

    /// `read_proc`.
    #[allow(
        clippy::too_many_lines,
        reason = "upstream's read_proc, in one piece so it reads against it"
    )]
    fn read_proc(&mut self, root_pid: i32) -> Result<(), Die> {
        let buffer_size = if self.trunc {
            usize::try_from(self.output_width)
                .unwrap_or(0)
                .saturating_add(1)
        } else {
            BUFSIZ.saturating_add(1)
        };
        let dir = match std::fs::read_dir("/proc") {
            Ok(d) => d,
            Err(e) => {
                perror(b"/proc", &e);
                return Err(Die(1));
            }
        };
        let mut empty = true;
        for de in dir.flatten() {
            let name = os_bytes(&de.file_name()).into_owned();
            // `strtol (d_name, &endptr, 10)`, the whole name a number.
            let (number, used) = cstrtol::strtol(&name, 10);
            if used == 0 || used != name.len() {
                continue;
            }
            let pid = number as i32;
            let Ok(file) = std::fs::File::open(format!("/proc/{pid}/stat")) else {
                continue;
            };
            empty = false;
            let Ok(st) = std::fs::metadata(format!("/proc/{pid}")) else {
                continue;
            };
            let uid = uid_of(&st);
            let mut readbuf = Vec::with_capacity(BUFSIZ);
            if file.take(BUFSIZ as u64).read_to_end(&mut readbuf).is_err() {
                continue;
            }
            let readbuf = c_str(&readbuf);
            // "commands may have spaces or ) in them. so don't trust
            // anything from the ( to the last )"
            let Some((open, close)) = comm_bounds(readbuf) else {
                continue;
            };
            let comm = readbuf
                .get(open.saturating_add(1)..close)
                .unwrap_or_default()
                .to_vec();
            let rest = readbuf.get(close.saturating_add(2)..).unwrap_or_default();
            let Some((ppid, pgid, starttime)) = scan_stat(rest) else {
                continue;
            };
            let age = process_age(starttime)?;

            // "handle process threads"
            if !self.hide_threads
                && let Ok(tasks) = std::fs::read_dir(format!("/proc/{pid}/task"))
            {
                for dt in tasks.flatten() {
                    let thread = atoi(&os_bytes(&dt.file_name()));
                    if thread == 0 || thread == pid {
                        continue;
                    }
                    let threadname = self.get_threadname(pid, thread, &comm);
                    let mut args = threadname.clone();
                    args.push(0);
                    let size = args.len();
                    let args = self.print_args.then_some((args.as_slice(), size));
                    self.add_proc(&threadname, thread, pid, pgid, uid, args, true, age);
                }
            }

            // "handle process"
            if self.print_args {
                // "If this fails then the process is gone."
                let Ok(cmdline) = std::fs::File::open(format!("/proc/{pid}/cmdline")) else {
                    continue;
                };
                let mut buffer = Vec::new();
                if cmdline
                    .take(buffer_size as u64)
                    .read_to_end(&mut buffer)
                    .is_err()
                {
                    continue;
                }
                // "If we have read the maximum screen length of args, bring
                // it back by one to stop overflow"
                let mut size = buffer.len();
                if size >= buffer_size {
                    size = size.saturating_sub(1);
                }
                buffer.truncate(size);
                if size > 0 {
                    buffer.push(0);
                    size = size.saturating_add(1);
                }
                self.add_proc(
                    &comm,
                    pid,
                    ppid,
                    pgid,
                    uid,
                    Some((&buffer, size)),
                    false,
                    age,
                );
            } else {
                self.add_proc(&comm, pid, ppid, pgid, uid, None, false, age);
            }
        }
        self.fix_orphans(root_pid);
        if empty {
            ulclosestream::stderr_write(b"/proc is empty (not mounted ?)\n");
            return Err(Die(1));
        }
        Ok(())
    }
}

/// The C string at the start of `s`.
fn c_str(s: &[u8]) -> &[u8] {
    s.get(..s.iter().position(|&b| b == 0).unwrap_or(s.len()))
        .unwrap_or_default()
}

/// `fgets (buf, BUFSIZ, file)`: the first line, newline and all, of at
/// most `BUFSIZ - 1` bytes.
fn fgets_line(data: &[u8]) -> Vec<u8> {
    let end = data
        .iter()
        .position(|&b| b == b'\n')
        .map_or(data.len(), |i| i.saturating_add(1));
    data.get(..end.min(BUFSIZ.saturating_sub(1)))
        .unwrap_or_default()
        .to_vec()
}

/// `strchr (buf, '(')` and `strrchr` of the `)` after it.
fn comm_bounds(s: &[u8]) -> Option<(usize, usize)> {
    let open = s.iter().position(|&b| b == b'(')?;
    let close = s.iter().rposition(|&b| b == b')').filter(|&c| c > open)?;
    Some((open, close))
}

/// `strncpy (comm, src, COMM_LEN + 2)` and the NUL at `COMM_LEN + 1`.
fn cut_comm(src: &[u8]) -> Vec<u8> {
    let s = c_str(src);
    s.get(..s.len().min(COMM_MAX)).unwrap_or(s).to_vec()
}

/// `atoi`: `(int) strtol (s, NULL, 10)`.
fn atoi(s: &[u8]) -> i32 {
    scanf::atoi(s)
}

/// `sscanf (rest, "%*c %d %d %*s ... %Lu") == 3`, through the sscanf the
/// procps port reads `stat` with: the parent, the group, and the start
/// time in jiffies, sixteen fields after the group. Each conversion skips
/// the blanks before it, as the format's blanks would.
fn scan_stat(rest: &[u8]) -> Option<(i32, i32, u64)> {
    let mut sc = scanf::Scan::new(rest);
    sc.ch()?;
    let ppid = sc.int()?;
    let pgid = sc.int()?;
    for _ in 0..16 {
        sc.skip_word()?;
    }
    let start = sc.ulong()?;
    Some((ppid, pgid, start))
}

/// `uptime ()`: `/proc/uptime`'s first number, or the end of the run.
fn uptime() -> Result<f64, Die> {
    let Ok(text) = std::fs::read("/proc/uptime") else {
        ulclosestream::stderr_write(b"pstree: error opening uptime file\n");
        return Err(Die(1));
    };
    let word = text
        .split(u8::is_ascii_whitespace)
        .find(|w| !w.is_empty())
        .unwrap_or_default();
    Ok(atof(word))
}

/// `atof` of `/proc/uptime`'s plain decimal: its longest prefix that is a
/// number, else 0.
fn atof(s: &[u8]) -> f64 {
    let s = std::str::from_utf8(s).unwrap_or("");
    let mut best = 0.0;
    for (i, _) in s.char_indices() {
        if let Some(v) = s.get(..=i).and_then(|p| p.parse::<f64>().ok()) {
            best = v;
        }
    }
    best
}

/// `process_age (jf)`: seconds since the start, from jiffies and uptime.
fn process_age(jf: u64) -> Result<f64, Die> {
    // `sysconf (_SC_CLK_TCK)`: 100 on Linux, and on SlateOS.
    const CLK_TCK: f64 = 100.0;
    let age = uptime()? - jf as f64 / CLK_TCK;
    Ok(if age < 0.0 { 0.0 } else { age })
}

/// `st_ino`.
#[cfg(unix)]
fn inode_of(m: &std::fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    m.ino()
}

#[cfg(not(unix))]
fn inode_of(_m: &std::fs::Metadata) -> u64 {
    0
}

/// `st_uid`.
#[cfg(unix)]
fn uid_of(m: &std::fs::Metadata) -> u32 {
    use std::os::unix::fs::MetadataExt;
    m.uid()
}

#[cfg(not(unix))]
fn uid_of(_m: &std::fs::Metadata) -> u32 {
    0
}

/// `perror (what)`.
fn perror(what: &[u8], e: &std::io::Error) {
    let mut m = what.to_vec();
    m.extend_from_slice(b": ");
    m.extend_from_slice(coreutils::errmsg::strerror(e).as_bytes());
    m.push(b'\n');
    ulclosestream::stderr_write(&m);
}

/// `get_output_width`: `COLUMNS`, else the window's, else 132.
fn get_output_width() -> i32 {
    if let Some(c) = std::env::var_os("COLUMNS") {
        let c = os_bytes(&c).into_owned();
        if !c.is_empty() {
            let (t, used) = cstrtol::strtol(&c, 0);
            if used == c.len() && t > 0 && t < 0x7fff_ffff {
                return t as i32;
            }
        }
    }
    match libcall::pty::window_size(1) {
        Ok(size) if size.cols != 0 => i32::from(size.cols),
        _ => 132,
    }
}

/// `usage ()`: to standard error, status 1.
fn usage() -> Die {
    ulclosestream::stderr_write(
        concat!(
            "Usage: pstree [-acglpsStTuZ] [ -h | -H PID ] [ -n | -N type ]\n",
            "              [ -A | -G | -U ] [ PID | USER ]\n",
            "   or: pstree -V\n",
            "\n",
            "Display a tree of processes.\n\n",
            "  -a, --arguments     show command line arguments\n",
            "  -A, --ascii         use ASCII line drawing characters\n",
            "  -c, --compact-not   don't compact identical subtrees\n",
            "  -C, --color=TYPE    color process by attribute\n",
            "                      (age)\n",
            "  -g, --show-pgids    show process group ids; implies -c\n",
            "  -G, --vt100         use VT100 line drawing characters\n",
            "  -h, --highlight-all highlight current process and its ancestors\n",
            "  -H PID, --highlight-pid=PID\n",
            "                      highlight this process and its ancestors\n",
            "  -l, --long          don't truncate long lines\n",
            "  -n, --numeric-sort  sort output by PID\n",
            "  -N TYPE, --ns-sort=TYPE\n",
            "                      sort output by this namespace type\n",
            "                              (cgroup, ipc, mnt, net, pid, time, user, uts)\n",
            "  -p, --show-pids     show PIDs; implies -c\n",
            "  -s, --show-parents  show parents of the selected process\n",
            "  -S, --ns-changes    show namespace transitions\n",
            "  -t, --thread-names  show full thread names\n",
            "  -T, --hide-threads  hide threads, show only processes\n",
            "  -u, --uid-changes   show uid transitions\n",
            "  -U, --unicode       use UTF-8 (Unicode) line drawing characters\n",
            "  -V, --version       display version information\n",
            "  -Z, --security-context\n",
            "                      show security attributes\n",
            "\n",
            "  PID    start at this PID; default is 1 (init)\n",
            "  USER   show only trees rooted at processes of this user\n\n",
        )
        .as_bytes(),
    );
    Die(1)
}

fn main() -> ExitCode {
    stdfd::close_stderr(run_main(), 1)
}

fn run_main() -> ExitCode {
    stdfd::restore();
    let argv: Vec<OsString> = std::env::args_os().collect();
    let argv0 = argv
        .first()
        .map(|a| os_bytes(a).into_owned())
        .unwrap_or_default();
    // `__progname`: what follows the last slash.
    let progname = argv0
        .iter()
        .rposition(|&c| c == b'/')
        .map_or(argv0.as_slice(), |i| {
            argv0.get(i.saturating_add(1)..).unwrap_or_default()
        });
    let wait_end = progname == b"pstree.x11";
    let mut out = ulclosestream::Stdout::new(1);
    let status = match run(argv.get(1..).unwrap_or_default(), &argv0, &mut out) {
        Ok(code) => {
            if wait_end {
                ulclosestream::stderr_write(b"Press return to close\n");
                // `(void) getchar ()`: whatever it reads, or nothing -- the
                // answer, or its failure, is not looked at; it only waits.
                let mut byte = [0u8; 1];
                let _ = std::io::stdin().read(&mut byte);
            }
            code
        }
        Err(Die(code)) => code,
    };
    // `exit`'s own flush, whose failure nobody hears of.
    out.flush_at_exit();
    ExitCode::from(status)
}

/// The value of `TERM`, if set.
fn term_var() -> Option<Vec<u8>> {
    std::env::var_os("TERM").map(|t| os_bytes(&t).into_owned())
}

/// Upstream's `main`, up to its `return 0`.
#[allow(
    clippy::too_many_lines,
    reason = "upstream's main, in one piece so it reads against it"
)]
fn run(argv: &[OsString], argv0: &[u8], out: &mut ulclosestream::Stdout) -> Result<u8, Die> {
    let output_width = get_output_width();
    // `find_root_pid`: 0 where there is a /proc/0, as in LXC.
    let root_pid = if std::fs::metadata("/proc/0").is_ok() {
        0
    } else {
        DEFAULT_ROOT_PID
    };
    let mut pid = root_pid;
    let mut highlight: i32 = 0;
    let mut pw_uid: Option<u32> = None;
    let mut pid_set = false;
    let mut nsid: Option<usize> = None;
    let mut show_parents = false;
    let tenv = terminfo::Env::from_process();
    let mut st = Pstree::new(out, output_width);

    // "Attempt to figure out a good default symbol set."
    let tty = stdfd::is_tty(1);
    if tty && coreutils::locale::ctype_is_utf8() {
        st.sym = &SYM_UTF;
    } else if tty && let Some(term) = term_var().filter(|t| !t.is_empty()) {
        // `setupterm (NULL, 1, NULL)`, which with nowhere to put its answer
        // complains and exits; what it says of `acsc` changes nothing, as
        // upstream settles on ASCII either way.
        let setup = terminfo::setupterm(None, Some(&term), &tenv);
        if let Some(complaint) = setup.complaint {
            ulclosestream::stderr_write(&complaint);
            return Err(Die(1));
        }
        let padding = setup.padding();
        st.terminal = setup
            .entry
            .map(|e| terminfo::Termcap::untrimmed(e, padding));
    }

    let mut operands: Vec<Vec<u8>> = Vec::new();
    for item in PSTREE.parse(argv, SHORT_OPTIONS, LONG_OPTIONS) {
        let opt = match item {
            Ok(o) => o,
            Err(e) => {
                let mut m = escape_unprintable(argv0).into_bytes();
                m.extend_from_slice(b": ");
                m.extend_from_slice(e.sentence.as_bytes());
                m.push(b'\n');
                ulclosestream::stderr_write(&m);
                return Err(usage());
            }
        };
        let arg = |v: Option<OsString>| v.as_deref().map(os_bytes).unwrap_or_default().into_owned();
        match opt {
            Opt::Short(b'a', _) | Opt::Long("arguments", _) => st.print_args = true,
            Opt::Short(b'A', _) | Opt::Long("ascii", _) => st.sym = &SYM_ASCII,
            Opt::Short(b'c', _) | Opt::Long("compact-not", _) => st.compact = false,
            Opt::Short(b'C', v) | Opt::Long("color", v) => {
                if !arg(v).eq_ignore_ascii_case(b"age") {
                    return Err(usage());
                }
                st.color_age = true;
            }
            Opt::Short(b'G', _) | Opt::Long("vt100", _) => st.sym = &SYM_VT100,
            Opt::Short(b'h', _) | Opt::Long("highlight-all", _) => {
                if highlight != 0 {
                    return Err(usage());
                }
                if let Some(term) = term_var()
                    && let Ok(tc) = terminfo::tgetent(&term, &tenv)
                {
                    st.terminal = Some(tc);
                    highlight = std::process::id() as i32;
                }
            }
            Opt::Short(b'H', v) | Opt::Long("highlight-pid", v) => {
                if highlight != 0 {
                    return Err(usage());
                }
                let Some(term) = term_var() else {
                    ulclosestream::stderr_write(b"TERM is not set\n");
                    return Err(Die(1));
                };
                match terminfo::tgetent(&term, &tenv) {
                    Ok(tc) => st.terminal = Some(tc),
                    Err(_) => {
                        ulclosestream::stderr_write(b"Can't get terminal capabilities\n");
                        return Err(Die(1));
                    }
                }
                highlight = atoi(&arg(v));
                if highlight == 0 {
                    return Err(usage());
                }
            }
            Opt::Short(b'l', _) | Opt::Long("long", _) => st.trunc = false,
            Opt::Short(b'n', _) | Opt::Long("numeric-sort", _) => st.by_pid = true,
            Opt::Short(b'N', v) | Opt::Long("ns-sort", v) => {
                let name = arg(v);
                let Some((id, ns)) = NS_NAMES
                    .iter()
                    .enumerate()
                    .find(|(_, n)| n.as_bytes() == name.as_slice())
                else {
                    return Err(usage());
                };
                // `verify_ns`: this process's own file for the type.
                if std::fs::metadata(format!("/proc/{}/ns/{ns}", std::process::id())).is_err() {
                    let mut m = b"procfs file for ".to_vec();
                    m.extend_from_slice(&name);
                    m.extend_from_slice(b" namespace not available\n");
                    ulclosestream::stderr_write(&m);
                    return Err(Die(1));
                }
                nsid = Some(id);
            }
            Opt::Short(b'p', _) | Opt::Long("show-pids", _) => {
                st.pids = true;
                st.compact = false;
            }
            Opt::Short(b'g', _) | Opt::Long("show-pgids", _) => st.pgids = true,
            Opt::Short(b's', _) | Opt::Long("show-parents", _) => show_parents = true,
            Opt::Short(b'S', _) | Opt::Long("ns-changes", _) => st.ns_change = true,
            Opt::Short(b't', _) | Opt::Long("thread-names", _) => st.thread_names = true,
            Opt::Short(b'T', _) | Opt::Long("hide-threads", _) => st.hide_threads = true,
            Opt::Short(b'u', _) | Opt::Long("uid-changes", _) => st.user_change = true,
            Opt::Short(b'U', _) | Opt::Long("unicode", _) => st.sym = &SYM_UTF,
            Opt::Short(b'V', _) | Opt::Long("version", _) => {
                ulclosestream::stderr_write(b"pstree (SlateOS coreutils) 0.1.0\n");
                return Ok(0);
            }
            Opt::Short(b'Z', _) | Opt::Long("security-context", _) => st.show_scontext = true,
            Opt::Operand(v) => operands.push(os_bytes(v).into_owned()),
            Opt::Short(..) | Opt::Long(..) => return Err(usage()),
        }
    }

    match operands.as_slice() {
        [] => {}
        [one] => {
            if one.first().is_some_and(u8::is_ascii_digit) {
                let (n, used) = cstrtol::strtol(one, 10);
                pid = n as i32;
                pid_set = true;
                if used != one.len() {
                    return Err(usage());
                }
            } else {
                match pwdb::Db::load().user_by_name(one) {
                    Some(u) => pw_uid = Some(u.uid),
                    None => {
                        let mut m = b"No such user name: ".to_vec();
                        m.extend_from_slice(one);
                        m.push(b'\n');
                        ulclosestream::stderr_write(&m);
                        return Err(Die(1));
                    }
                }
            }
        }
        _ => return Err(usage()),
    }

    st.read_proc(root_pid)?;
    let mut cur = st.find_proc(highlight);
    while let Some(c) = cur {
        cur = st.procs.get_mut(c).and_then(|p| {
            p.flags |= PFLAG_HILIGHT;
            p.parent
        });
    }

    if show_parents && pid_set {
        let Some(child) = st.find_proc(pid) else {
            ulclosestream::stderr_write(format!("Process {pid} not found.\n").as_bytes());
            return Err(Die(1));
        };
        st.trim_tree_by_parent(child);
        pid = root_pid;
    }

    if let Some(id) = nsid {
        let mut root = Vec::new();
        st.sort_by_namespace(None, id, &mut root);
        st.dump_by_namespace(&root);
    } else if let Some(uid) = pw_uid {
        let r = st.find_proc(root_pid);
        st.dump_by_user(r, uid);
        if !st.dumped {
            ulclosestream::stderr_write(b"No processes found.\n");
            return Err(Die(1));
        }
    } else {
        let p = st.find_proc(pid);
        st.dump_tree(p, 0, 1, true, true, 0, 0);
    }
    Ok(0)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
mod tests {
    use super::*;

    #[test]
    fn command_lines_split_as_set_args_splits_them() {
        let mut out = ulclosestream::Stdout::new(1);
        let mut st = Pstree::new(&mut out, 80);
        st.print_args = true;
        let p = st.new_proc(b"ls", 999_999, 0);
        // `ls -l /tmp`, its NUL, and the one `read_proc` adds.
        let args = b"ls\0-l\0/tmp\0\0";
        st.set_args(p, args, args.len());
        assert_eq!(st.procs[p].argc, 3);
        assert_eq!(
            st.procs[p].argv,
            vec![b"-l".to_vec(), b"/tmp".to_vec(), Vec::new()]
        );
        // Nothing read: swapped.
        st.set_args(p, b"", 0);
        assert_eq!(st.procs[p].argc, -1);
        // One word: no arguments after it.
        st.set_args(p, b"init\0\0", 6);
        assert_eq!(st.procs[p].argc, 1);
    }

    #[test]
    fn out_int_writes_what_upstream_s_loops_write() {
        let mut out = ulclosestream::Stdout::new(1);
        let mut st = Pstree::new(&mut out, 1000);
        assert_eq!(st.out_int(0), 1);
        assert_eq!(st.cur_x, 1);
        assert_eq!(st.out_int(7), 1);
        assert_eq!(st.out_int(1234), 4);
        assert_eq!(st.cur_x, 1 + 1 + 4);
        // Past nine digits the divisor wraps, as upstream's does: 10^10
        // wraps to 1410065408, which 10^9 does not reach -- ten digits
        // counted, nine written.
        assert_eq!(st.out_int(1_000_000_000), 10);
        assert_eq!(st.cur_x, 1 + 1 + 4 + 9);
        st.out.discard();
    }

    #[test]
    fn stat_fields_are_scanned_as_sscanf_scans_them() {
        let rest = b"S 1 42 42 0 -1 4194560 100 0 0 0 1 2 0 0 20 0 1 0 12345 999";
        assert_eq!(scan_stat(rest), Some((1, 42, 12345)));
        assert_eq!(scan_stat(b"S 1"), None);
        // `%d` stops at the first byte that is not a digit, and the next
        // conversion starts there: `12x` is a parent of 12 and no group.
        assert_eq!(scan_stat(b"S 12x 3 4"), None);
        // A field too wide for `int` keeps its low 32 bits.
        let wide = format!("S 4294967297 -1 {} 7", ["0"; 16].join(" "));
        assert_eq!(scan_stat(wide.as_bytes()), Some((1, -1, 7)));
    }

    #[test]
    fn numbers_read_as_atof_reads_them() {
        assert!((atof(b"1234.56") - 1234.56).abs() < 1e-9);
        assert!((atof(b"12abc") - 12.0).abs() < 1e-9);
        assert!(atof(b"x").abs() < 1e-9);
    }

    #[test]
    fn names_are_cut_where_strncpy_cuts_them() {
        assert_eq!(cut_comm(&[b'a'; 70]).len(), 65);
        assert_eq!(cut_comm(b"bash\0junk"), b"bash");
        assert_eq!(comm_bounds(b"1 (a) b) S"), Some((2, 7)));
        assert_eq!(comm_bounds(b") ("), None);
    }
}
