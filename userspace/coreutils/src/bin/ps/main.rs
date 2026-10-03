//! `ps` — report a snapshot of the current processes.
//!
//! ```text
//! ps [options]
//! ```
//!
//! A transcription of procps-ng 4.0.4's `src/ps/` -- `display.c`,
//! `global.c`, `parser.c`, `select.c`, `sortformat.c`, `output.c`,
//! `help.c` and `signames.c` -- with the parts of `libproc2` it reads in
//! [`coreutils::procps`]. Measured against that release, built without logind
//! and without libnuma as SlateOS is, by `scripts/ps-diff.sh`, which gives both
//! programs the same made-up `/proc` (see the harness for how).
//!
//! # Three syntaxes
//!
//! `ps` reads its arguments as Unix options (`-ef`), BSD options (`aux`, no
//! dash) and GNU long options (`--sort=pid`), in any mix -- and when the
//! first reading fails, it reads them all again as BSD. That second reading
//! is why `ps -aux` works: `-u x` names a user `x`, there is none, so the
//! line is read again with the dash ignored, as `aux`. The error printed when
//! both readings fail is the first's.
//!
//! # Columns
//!
//! Every column is one of 275 format specifiers (`ps L` lists the ones that
//! print something), with a header, a width, a justification and a sort key.
//! The preset formats (`-f`, `-l`, `u`, `j`, `s`, `v`, ...) are lists of them,
//! and `-o` takes any list, with `=HEADER` and `:WIDTH` overrides. Only the
//! last column may run past its width; any other that overflows pushes the
//! rest of the line right, and later columns take the space back where they
//! can.
//!
//! # Deliberate differences from procps-ng 4.0.4
//!
//! 1. **`--version`** says `ps from SlateOS coreutils 0.1.0`, in procps'
//!    shape.
//! 2. **`--info`** names this program's build where upstream names its
//!    compiler and C library (`Compiled with: glibc 2.39, gcc 13.2`).
//! 3. **A signal arrives as one.** Upstream catches every signal to print a
//!    "please report this bug" line before dying of it; a fault here is a bug
//!    in this port, which dies with Rust's own report. A closed pipe still
//!    ends `ps` quietly with status 0, as `ps | head` needs.
//! 4. **No undefined behaviour is reproduced.** Where upstream overruns a
//!    buffer -- a process name of 64 bytes or more in `stat`, more than 70
//!    distinct columns -- or divides by zero (`%mem` with no `MemTotal`), or
//!    reads an uninitialised `double` (`/proc/uptime` missing), this port does
//!    the nearest well-defined thing and says so where it does it.
//! 5. **`-m` cannot loop forever.** Upstream's thread listing assumes that the
//!    first entry of each thread group, sorted by start time, is the group's
//!    leader; when it is not (a thread that reports an earlier start than its
//!    process), upstream prints that entry's process line over and over and
//!    never ends. Here the entry is printed once and passed over.
//!
//! # Where the rest of it lives
//!
//! The reading of `/proc` -- `stat2proc`, `status2proc`, the terminal namer,
//! the password cache, `pid_max`, `btime`, `MemTotal` -- is
//! [`coreutils::procps`], shared with `w` and `uptime`. This program is the
//! part of procps that is `ps`'s alone: the option parser (`parser`), the
//! format and sort lists (`sortformat`), selection (`select`), the table and
//! forest (`display`), the 275 columns (`formats`, `output`), and the `<pids>`
//! layer between them and the library (`items`).

mod display;
mod formats;
mod help;
mod items;
mod output;
mod parser;
mod select;
mod signames;
mod sortformat;

use std::io::Write;
use std::path::PathBuf;

use coreutils::procps::devname::Devname;
use coreutils::procps::pwcache::Pwcache;
use coreutils::procps::readproc::{Fill, Reader};
use localtime::Zone;

use formats::Pr;
use items::{Item, ItemCtx, Registry, Stack, SysCache};

/// `OUTBUF_SIZE`: the output line buffer, and the "unlimited" width.
pub const OUTBUF_SIZE: i32 = 2 * 64 * 1024;

/// `SEL_*`: what a selection list holds.
pub mod sel {
    pub const RUID: i32 = 1;
    pub const EUID: i32 = 2;
    pub const SUID: i32 = 3;
    pub const FUID: i32 = 4;
    pub const RGID: i32 = 5;
    pub const EGID: i32 = 6;
    pub const SGID: i32 = 7;
    pub const FGID: i32 = 8;
    pub const PGRP: i32 = 9;
    pub const PID: i32 = 10;
    pub const TTY: i32 = 11;
    pub const SESS: i32 = 12;
    pub const COMM: i32 = 13;
    pub const PPID: i32 = 14;
    pub const PID_QUICK: i32 = 15;
}

/// `thread_flags`.
pub mod tf {
    pub const B_H: u32 = 0x0001;
    pub const B_M: u32 = 0x0002;
    pub const U_M: u32 = 0x0004;
    pub const U_T: u32 = 0x0008;
    pub const U_L: u32 = 0x0010;
    pub const SHOW_PROC: u32 = 0x0100;
    pub const SHOW_TASK: u32 = 0x0200;
    pub const SHOW_BOTH: u32 = 0x0400;
    pub const LOOSE_TASKS: u32 = 0x0800;
    pub const NO_SORT: u32 = 0x1000;
    pub const MUST_USE: u32 = 0x4000;
}

/// `personality`.
pub mod per {
    pub const BROKEN_O: u32 = 0x0001;
    pub const BSD_H: u32 = 0x0002;
    pub const BSD_M: u32 = 0x0004;
    pub const IRIX_L: u32 = 0x0008;
    pub const FORCE_BSD: u32 = 0x0010;
    pub const GOOD_O: u32 = 0x0020;
    pub const OLD_M: u32 = 0x0040;
    pub const NO_DEFAULT_G: u32 = 0x0080;
    pub const ZAP_ADDR: u32 = 0x0100;
    pub const SANE_USER: u32 = 0x0200;
    pub const HPUX_X: u32 = 0x0400;
    pub const SVR4_X: u32 = 0x0800;
}

/// `simple_select`.
pub mod ss {
    pub const B_X: u32 = 0x01;
    pub const B_G: u32 = 0x02;
    pub const U_D: u32 = 0x04;
    pub const U_A: u32 = 0x08;
    pub const B_A: u32 = 0x10;
}

/// `format_flags`.
pub mod ff {
    pub const UF: u32 = 0x0001;
    pub const UJ: u32 = 0x0002;
    pub const UL: u32 = 0x0004;
    pub const BJ: u32 = 0x0008;
    pub const BL: u32 = 0x0010;
    pub const BS: u32 = 0x0020;
    pub const BU: u32 = 0x0040;
    pub const BV: u32 = 0x0080;
    pub const LX: u32 = 0x0100;
    pub const LM: u32 = 0x0200;
    pub const FC: u32 = 0x0400;
}

/// `format_modifiers`.
pub mod fm {
    pub const C: u32 = 0x0001;
    pub const J: u32 = 0x0002;
    pub const Y: u32 = 0x0004;
    pub const P: u32 = 0x0010;
    pub const M: u32 = 0x0020;
    pub const F: u32 = 0x0080;
}

/// `header_type`.
pub const HEAD_SINGLE: i32 = 0;
pub const HEAD_NONE: i32 = 1;
pub const HEAD_MULTI: i32 = 2;

/// One column: `format_node`. `pr` is `None` for literal text (an AIX format's
/// spaces and words, `-ly`'s `:`).
#[derive(Clone, Debug)]
pub struct FormatNode {
    pub name: Vec<u8>,
    pub pr: Option<Pr>,
    pub width: i32,
    pub vendor: i32,
    pub flags: u32,
}

/// One sort key: `sort_node`.
#[derive(Clone, Debug)]
pub struct SortNode {
    pub sr: Item,
    /// The column's print function, called only to register its items.
    pub xe: Pr,
    /// `ASCEND`, `DESCEND`, or 0 (not sorted on).
    pub reverse: i32,
}

/// One member of a selection list (`sel_union`).
#[derive(Clone, Debug, Default)]
pub struct SelVal {
    /// A PID, UID, GID or terminal device number.
    pub num: u64,
    /// `-C`'s command name: at most 63 bytes.
    pub cmd: Vec<u8>,
}

/// `selection_node`.
#[derive(Clone, Debug)]
pub struct SelectionNode {
    pub typecode: i32,
    pub u: Vec<SelVal>,
}

/// A fatal error: what to print and how to exit.
pub enum Exit {
    /// Exit with this status; everything has been printed.
    Status(i32),
}

/// All of `ps`'s state: upstream's globals, gathered.
pub struct Ps {
    // --- the process ---
    /// `/proc`, or a test's.
    pub root: PathBuf,
    /// `program_invocation_name`: `argv[0]` as given, for error messages.
    pub argv0: Vec<u8>,
    /// `myname`: `argv[0]` after its last `/`.
    pub myname: Vec<u8>,
    /// `Hertz`.
    pub hertz: i64,
    pub utf8: bool,
    pub zone: Zone,
    pub page_size: i64,
    pub args: Vec<Vec<u8>>,

    // --- global.c ---
    pub all_processes: bool,
    pub bsd_j_format: &'static [u8],
    pub bsd_l_format: &'static [u8],
    pub bsd_s_format: &'static [u8],
    pub bsd_u_format: &'static [u8],
    pub bsd_v_format: &'static [u8],
    pub bsd_c_option: bool,
    pub bsd_e_option: bool,
    pub cached_euid: u32,
    pub cached_tty: i32,
    pub forest_prefix: Vec<u8>,
    pub forest_type: u8,
    pub format_flags: u32,
    pub format_list: Vec<FormatNode>,
    pub format_modifiers: u32,
    pub header_gap: i32,
    pub header_type: i32,
    pub include_dead_children: bool,
    pub lines_to_next_header: i32,
    pub lstart_format: Option<Vec<u8>>,
    pub negate_selection: bool,
    pub personality: u32,
    pub prefer_bsd_defaults: bool,
    pub running_only: bool,
    pub screen_cols: i32,
    pub screen_rows: i32,
    /// Front first, as upstream's list is walked.
    pub selection_list: Vec<SelectionNode>,
    pub simple_select: u32,
    /// Front first: applied in this order.
    pub sort_list: Vec<SortNode>,
    pub sysv_f_format: Option<&'static [u8]>,
    pub sysv_fl_format: Option<&'static [u8]>,
    pub sysv_j_format: Option<&'static [u8]>,
    pub sysv_l_format: Option<&'static [u8]>,
    pub thread_flags: u32,
    pub unix_f_option: bool,
    pub user_is_number: bool,
    pub wchan_is_number: bool,
    pub signal_names: bool,
    pub saved_personality_text: Vec<u8>,

    // --- parser.c ---
    pub w_count: i32,
    pub force_bsd: bool,
    pub thisarg: usize,

    // --- sortformat.c ---
    pub sf_list: Vec<sortformat::SfNode>,
    pub have_gnu_sort: bool,
    pub already_parsed_sort: bool,
    pub already_parsed_format: bool,
    /// `format_parse`'s static `errbuf`: written once, never cleared.
    pub errbuf: Option<Vec<u8>>,

    // --- select.c ---
    pub select_bits: u32,

    // --- output.c ---
    pub max_rightward: u32,
    pub max_leftward: u32,
    pub wide_signals: bool,
    pub seconds_since_1970: i64,
    pub active_cols: u32,
    pub did_stuff: bool,

    // --- display.c ---
    pub proc_format_list: Vec<FormatNode>,
    pub task_format_list: Vec<FormatNode>,

    // --- the library ---
    pub registry: Registry,
    pub pw: Option<Pwcache>,
    pub sys: SysCache,
    pub out: Vec<u8>,
}

impl Ps {
    /// A `ps` with everything at upstream's static initial values.
    #[must_use]
    pub fn new(root: PathBuf, argv: Vec<Vec<u8>>, utf8: bool, zone: Zone) -> Self {
        let argv0 = argv.first().cloned().unwrap_or_default();
        let myname = match argv0.iter().rposition(|&b| b == b'/') {
            Some(at) => argv0
                .get(at.saturating_add(1)..)
                .unwrap_or_default()
                .to_vec(),
            None => argv0.clone(),
        };
        let sys = SysCache::new(&root);
        Self {
            root,
            argv0,
            myname,
            hertz: coreutils::procps::hertz(),
            utf8,
            zone,
            page_size: page_size(),
            args: argv,
            all_processes: false,
            bsd_j_format: b"",
            bsd_l_format: b"",
            bsd_s_format: b"",
            bsd_u_format: b"",
            bsd_v_format: b"",
            bsd_c_option: false,
            bsd_e_option: false,
            cached_euid: u32::MAX,
            cached_tty: -1,
            forest_prefix: vec![0u8; 4 * 32 * 1024 + 100],
            forest_type: 0,
            format_flags: 0,
            format_list: Vec::new(),
            format_modifiers: 0,
            header_gap: -1,
            header_type: -1,
            include_dead_children: false,
            lines_to_next_header: -1,
            lstart_format: None,
            negate_selection: false,
            personality: 0,
            prefer_bsd_defaults: false,
            running_only: false,
            screen_cols: -1,
            screen_rows: -1,
            selection_list: Vec::new(),
            simple_select: 0,
            sort_list: Vec::new(),
            sysv_f_format: None,
            sysv_fl_format: None,
            sysv_j_format: None,
            sysv_l_format: None,
            thread_flags: 0,
            unix_f_option: false,
            user_is_number: false,
            wchan_is_number: false,
            signal_names: false,
            saved_personality_text: b"You found a bug!".to_vec(),
            w_count: 0,
            force_bsd: false,
            thisarg: 0,
            sf_list: Vec::new(),
            have_gnu_sort: false,
            already_parsed_sort: false,
            already_parsed_format: false,
            errbuf: None,
            select_bits: 0,
            max_rightward: u32::try_from(OUTBUF_SIZE - 1).unwrap_or(0),
            max_leftward: u32::try_from(OUTBUF_SIZE - 1).unwrap_or(0),
            wide_signals: false,
            seconds_since_1970: 0,
            active_cols: 0,
            did_stuff: false,
            proc_format_list: Vec::new(),
            task_format_list: Vec::new(),
            registry: Registry::default(),
            pw: None,
            sys,
            out: Vec::new(),
        }
    }

    /// The password database, read the first time it is needed.
    pub fn pw(&mut self) -> &mut Pwcache {
        self.pw.get_or_insert_with(Pwcache::load)
    }

    /// glibc's `error (0, 0, …)`: `argv[0]: message` on standard error, after
    /// standard output is flushed.
    pub fn error(&mut self, msg: &[u8]) {
        self.flush();
        let mut line = self.argv0.clone();
        line.extend_from_slice(b": ");
        line.extend_from_slice(msg);
        line.push(b'\n');
        // Standard error is the last place a failure could be reported.
        let _ = std::io::stderr().write_all(&line);
    }

    /// `catastrophic_failure`: glibc's `error_at_line`, then exit 1.
    pub fn catastrophic(&mut self, file: &str, line: u32, msg: &str) -> Exit {
        self.flush();
        let mut text = self.argv0.clone();
        text.extend_from_slice(format!(":{file}:{line}: {msg}\n").as_bytes());
        // As above: nowhere else to report a failure to write it.
        let _ = std::io::stderr().write_all(&text);
        Exit::Status(1)
    }

    /// Write `fprintf (stderr, …)` text.
    pub fn eprint(&mut self, text: &[u8]) {
        self.flush();
        // As above.
        let _ = std::io::stderr().write_all(text);
    }

    /// Write what has been printed so far. A closed pipe ends `ps` with
    /// status 0 (upstream's SIGPIPE handler); any other write error is
    /// `close_stdout`'s `write error`, status 1.
    pub fn flush(&mut self) {
        if self.out.is_empty() {
            return;
        }
        let data = std::mem::take(&mut self.out);
        let mut stdout = std::io::stdout().lock();
        if let Err(e) = stdout.write_all(&data).and_then(|()| stdout.flush()) {
            if e.kind() == std::io::ErrorKind::BrokenPipe {
                std::process::exit(0);
            }
            let mut msg = self.argv0.clone();
            msg.extend_from_slice(
                format!(": write error: {}\n", coreutils::errmsg::strerror(&e)).as_bytes(),
            );
            // The write that failed was the report channel's sibling; this is
            // the last place to say so.
            let _ = std::io::stderr().write_all(&msg);
            std::process::exit(1);
        }
    }

    /// A reader of `/proc` for the registered items.
    fn reader(&mut self, fill: Fill) -> Reader {
        let pw = self.pw.take().unwrap_or_else(Pwcache::load);
        Reader::new(self.root.clone(), fill, self.utf8, pw)
    }

    /// The item context for one reading: `boot_tics` from `/proc/uptime`
    /// now, as `procps_pids_reap` reads it.
    fn item_ctx(&self) -> ItemCtx {
        let hz = u64::try_from(self.hertz).unwrap_or(100);
        let up = coreutils::procps::sysinfo::uptime(&self.root);
        ItemCtx {
            hertz: hz,
            boot_tics: items::cvt_u64(up * items::dbl(hz)),
            devname: Devname::new(self.root.clone()),
            root: self.root.clone(),
        }
    }

    /// `rSv(item, …, stack)`: the value of `item` in `stack`.
    #[must_use]
    pub fn rsv<'s>(&self, item: Item, stack: &'s Stack) -> &'s items::Val {
        static ZERO: items::Val = items::Val::Zero;
        self.registry
            .rel(item)
            .and_then(|at| stack.get(at))
            .unwrap_or(&ZERO)
    }

    /// `reset_global`.
    pub fn reset_global(&mut self) -> Result<(), Exit> {
        self.selection_list.clear();

        // `fatal_proc_unmounted (Pids_info, 1)`: `/proc/self/stat` must be
        // readable, and this process must be found under its own PID.
        if !coreutils::procps::readproc::self_stat_readable(&self.root) {
            self.eprint(b"Error, do this: mount -t proc proc /proc\n");
            self.flush();
            std::process::exit(47);
        }
        let mut reg = Registry::default();
        reg.chk(Item::Tty);
        let mut reader = self.reader(reg.fill());
        let me = reader.select(&[std::process::id()]);
        self.pw = Some(reader.pw);
        let Some(me) = me.first() else {
            self.eprint(b"fatal library error, lookup self\n");
            return Err(Exit::Status(1));
        };
        let cached_tty = me.tty;

        self.set_screen_size();
        // Its error is upstream's to ignore too.
        let _ = self.set_personality();

        self.all_processes = false;
        self.bsd_c_option = false;
        self.bsd_e_option = false;
        self.cached_euid = effective_uid();
        self.cached_tty = cached_tty;
        self.forest_type = 0;
        self.format_flags = 0;
        self.format_list.clear();
        self.format_modifiers = 0;
        self.header_gap = -1;
        self.header_type = HEAD_SINGLE;
        self.include_dead_children = false;
        self.lines_to_next_header = 1;
        self.negate_selection = false;
        self.running_only = false;
        self.selection_list.clear();
        self.simple_select = 0;
        self.sort_list.clear();
        self.thread_flags = 0;
        self.unix_f_option = false;
        self.user_is_number = false;
        self.wchan_is_number = false;
        Ok(())
    }

    /// `set_screen_size`: the terminal's size, else 80x24; `COLUMNS` and
    /// `LINES` over that; and unlimited width when standard output is not a
    /// terminal.
    fn set_screen_size(&mut self) {
        let size = [1, 2, 0]
            .into_iter()
            .find_map(|fd| window_size(fd).filter(|&(c, r)| c > 0 && r > 0))
            .or_else(|| tty_window_size().filter(|&(c, r)| c > 0 && r > 0))
            .unwrap_or((80, 24));
        self.screen_cols = i32::from(size.0);
        self.screen_rows = i32::from(size.1);
        if !is_a_tty(1) {
            self.screen_cols = OUTBUF_SIZE;
        }
        if let Some(t) = env_number("COLUMNS") {
            self.screen_cols = t;
        }
        if let Some(t) = env_number("LINES") {
            self.screen_rows = t;
        }
        if self.screen_cols < 9 || self.screen_rows < 2 {
            let msg = format!(
                "your {}x{} screen size is bogus. expect trouble\n",
                self.screen_cols, self.screen_rows
            );
            self.eprint(msg.as_bytes());
        }
    }

    /// `set_personality`: `PS_PERSONALITY` (or `CMD_ENV`) chooses defaults.
    /// The error it can return is ignored by its only caller, as upstream's.
    fn set_personality(&mut self) -> Result<(), &'static str> {
        self.personality = 0;
        self.prefer_bsd_defaults = false;
        self.bsd_j_format = b"OL_j";
        self.bsd_l_format = b"OL_l";
        self.bsd_s_format = b"OL_s";
        self.bsd_u_format = b"OL_u";
        self.bsd_v_format = b"OL_v";
        self.sysv_f_format = None;
        self.sysv_fl_format = None;
        self.sysv_j_format = None;
        self.sysv_l_format = None;

        let var = |name: &str| env_bytes(name).filter(|v| !v.is_empty());
        let mut s = var("PS_PERSONALITY")
            .or_else(|| var("CMD_ENV"))
            .unwrap_or_else(|| b"unknown".to_vec());
        if env_bytes("I_WANT_A_BROKEN_PS").is_some() {
            s = b"old".to_vec();
        }
        if s.len() > 15 {
            return Err("environment specified an unknown personality");
        }
        self.saved_personality_text.clone_from(&s);
        let name = s.to_ascii_lowercase();
        match name.as_slice() {
            b"bsd" => {
                self.personality = per::FORCE_BSD | per::BSD_H | per::BSD_M;
                self.prefer_bsd_defaults = true;
                self.bsd_j_format = b"FB_j";
                self.bsd_l_format = b"FB_l";
                self.bsd_u_format = b"FB_u";
                self.bsd_v_format = b"FB_v";
            }
            b"old" => {
                self.personality = per::FORCE_BSD | per::OLD_M;
                self.prefer_bsd_defaults = true;
            }
            b"debian" | b"gnu" => {
                self.personality = per::GOOD_O | per::OLD_M;
                self.prefer_bsd_defaults = true;
                self.sysv_f_format = Some(b"RD_f");
                self.sysv_j_format = Some(b"RD_j");
                self.sysv_l_format = Some(b"RD_l");
            }
            b"linux" => {
                self.personality = per::GOOD_O | per::ZAP_ADDR | per::SANE_USER;
            }
            b"default" | b"unknown" | b"posix" | b"solaris2" | b"unix95" | b"unix98" | b"unix" => {}
            b"aix" => {
                self.bsd_j_format = b"FB_j";
                self.bsd_l_format = b"FB_l";
                self.bsd_u_format = b"FB_u";
                self.bsd_v_format = b"FB_v";
            }
            b"tru64" | b"compaq" | b"digital" => {
                self.personality = per::GOOD_O | per::BSD_H;
                self.prefer_bsd_defaults = true;
                self.sysv_f_format = Some(b"F5FMT");
                self.sysv_fl_format = Some(b"FL5FMT");
                self.sysv_j_format = Some(b"JFMT");
                self.sysv_l_format = Some(b"L5FMT");
                self.bsd_j_format = b"JFMT";
                self.bsd_l_format = b"LFMT";
                self.bsd_s_format = b"SFMT";
                self.bsd_u_format = b"UFMT";
                self.bsd_v_format = b"VFMT";
            }
            b"sunos4" => {
                self.personality = per::NO_DEFAULT_G;
                self.prefer_bsd_defaults = true;
                self.bsd_j_format = b"FB_j";
                self.bsd_l_format = b"FB_l";
                self.bsd_u_format = b"FB_u";
                self.bsd_v_format = b"FB_v";
            }
            b"irix" | b"sgi" => {
                let xpg = env_bytes("_XPG").unwrap_or_default();
                if !matches!(xpg.first(), Some(b'1'..=b'9')) {
                    self.personality = per::IRIX_L;
                }
            }
            b"os390" | b"s390" | b"390" => {
                self.sysv_j_format = Some(b"J390");
            }
            b"hp" | b"hpux" => self.personality = per::HPUX_X,
            b"svr4" | b"sysv" | b"sco" => self.personality = per::SVR4_X,
            _ => return Err("environment specified an unknown personality"),
        }
        Ok(())
    }

    /// `self_info`: `--info`.
    fn self_info(&mut self) {
        let show = |f: &[u8]| {
            String::from_utf8_lossy(if f.is_empty() { b"(none)" } else { f }).into_owned()
        };
        let opt = |f: Option<&[u8]>| show(f.unwrap_or(b""));
        let text = format!(
            "BSD j    {}\nBSD l    {}\nBSD s    {}\nBSD u    {}\nBSD v    {}\n\
             SysV -f  {}\nSysV -fl {}\nSysV -j  {}\nSysV -l  {}\n\n\
             SlateOS coreutils version 0.1.0\n\
             Compiled with: rustc, against SlateOS's C library\n\n\
             header_gap={} lines_to_next_header={}\nscreen_cols={} screen_rows={}\n\n\
             personality=0x{:08x} (from \"{}\")\nEUID={} TTY={},{} page_size={}\n\
             sizeof(proc_t)={} sizeof(long)=8 sizeof(long)=8\narchdefs: x86_64\n",
            show(self.bsd_j_format),
            show(self.bsd_l_format),
            show(self.bsd_s_format),
            show(self.bsd_u_format),
            show(self.bsd_v_format),
            opt(self.sysv_f_format),
            opt(self.sysv_fl_format),
            opt(self.sysv_j_format),
            opt(self.sysv_l_format),
            self.header_gap,
            self.lines_to_next_header,
            self.screen_cols,
            self.screen_rows,
            self.personality,
            String::from_utf8_lossy(&self.saved_personality_text),
            i32::from_le_bytes(self.cached_euid.to_le_bytes()),
            coreutils::procps::devname::major(u64::from(u32::from_le_bytes(
                self.cached_tty.to_le_bytes()
            ))),
            coreutils::procps::devname::minor(u64::from(u32::from_le_bytes(
                self.cached_tty.to_le_bytes()
            ))),
            self.page_size,
            std::mem::size_of::<items::Val>().saturating_mul(70),
        );
        self.eprint(text.as_bytes());
    }

    /// `finalize_stacks`: register what selection, state, sorting and every
    /// column need.
    fn finalize_stacks(&mut self) {
        let mut reg = Registry::default();
        for item in [
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
            Item::Noop,
        ] {
            reg.chk(item);
        }
        for node in &self.format_list {
            if let Some(pr) = node.pr {
                for &item in pr.rels() {
                    reg.chk(item);
                }
            }
        }
        for node in &self.sort_list {
            for &item in node.xe.rels() {
                reg.chk(item);
            }
        }
        self.registry = reg;
    }

    /// Read every process (or thread) for the registered items, as stacks.
    fn reap(&mut self, threads: bool, pids: Option<&[u32]>) -> Option<Vec<Stack>> {
        let fill = self.registry.fill();
        let mut ctx = self.item_ctx();
        let mut reader = self.reader(fill);
        let procs = if threads {
            reader.reap_threads(pids).ok()
        } else {
            match pids {
                Some(list) => Some(reader.select(list)),
                None => reader.reap().ok(),
            }
        };
        self.pw = Some(reader.pw);
        let procs = procs?;
        Some(
            procs
                .iter()
                .map(|p| items::assign(&self.registry, p, &mut ctx))
                .collect(),
        )
    }

    /// `main`, after the environment is read.
    fn run(&mut self) -> Result<(), Exit> {
        self.reset_global()?;
        self.arg_parse()?;
        self.arg_check_conflicts()?;
        self.init_output();
        self.finalize_stacks();
        self.lists_and_needs()?;
        if self.forest_type != 0 || !self.sort_list.is_empty() {
            self.fancy_spew()?;
        } else {
            self.simple_spew()?;
        }
        self.show_end()?;
        Ok(())
    }
}

/// `geteuid`.
#[cfg(unix)]
fn effective_uid() -> u32 {
    unsafe extern "C" {
        fn geteuid() -> u32;
    }
    // SAFETY: `geteuid` takes no arguments, cannot fail and touches no memory
    // of ours.
    unsafe { geteuid() }
}

/// The host build has no users.
#[cfg(not(unix))]
fn effective_uid() -> u32 {
    0
}

/// `getpagesize`: `sysconf (_SC_PAGESIZE)`.
fn page_size() -> i64 {
    const SC_PAGESIZE: i32 = 30;
    let p = libcall::conf::sysconf(SC_PAGESIZE);
    if p > 0 { p } else { 4096 }
}

/// `ioctl (fd, TIOCGWINSZ)`: (columns, rows).
fn window_size(fd: i32) -> Option<(u16, u16)> {
    libcall::pty::window_size(fd).ok().map(|w| (w.cols, w.rows))
}

/// The size of `/dev/tty`, opened `O_NOCTTY | O_NONBLOCK | O_RDONLY`.
#[cfg(unix)]
fn tty_window_size() -> Option<(u16, u16)> {
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::OpenOptionsExt;
    const O_NOCTTY: i32 = 0o400;
    const O_NONBLOCK: i32 = 0o4000;
    let f = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(O_NOCTTY | O_NONBLOCK)
        .open("/dev/tty")
        .ok()?;
    window_size(f.as_raw_fd())
}

/// The host build has no terminals.
#[cfg(not(unix))]
fn tty_window_size() -> Option<(u16, u16)> {
    None
}

/// `isatty`: whether `tcgetattr` succeeds.
fn is_a_tty(fd: i32) -> bool {
    libcall::termios::get_attr(fd).is_ok()
}

/// An environment variable as bytes, set or not.
fn env_bytes(name: &str) -> Option<Vec<u8>> {
    std::env::var_os(name).map(|v| coreutils::quote::os_bytes(&v).into_owned())
}

/// `COLUMNS` and `LINES` as `set_screen_size` takes them: non-empty, all of
/// it a number `strtol` reads in base 0, positive and below `OUTBUF_SIZE`.
fn env_number(name: &str) -> Option<i32> {
    let v = env_bytes(name).filter(|v| !v.is_empty())?;
    let (t, used) = parser::strtol_base0(&v);
    (used == v.len() && t > 0 && t < i64::from(OUTBUF_SIZE)).then(|| i32::try_from(t).unwrap_or(0))
}

#[cfg(unix)]
fn main() -> std::process::ExitCode {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;
    coreutils::guard_std_fds!();
    let argv: Vec<Vec<u8>> = std::env::args_os().map(OsString::into_vec).collect();
    // `setenv ("TZ", ":/etc/localtime", 0)`.
    let tz = env_bytes("TZ").unwrap_or_else(|| b":/etc/localtime".to_vec());
    let zone = Zone::from_tz(Some(&tz));
    let utf8 = coreutils::locale::ctype_is_utf8();
    let mut ps = Ps::new(PathBuf::from("/proc"), argv, utf8, zone);
    let code = match ps.run() {
        Ok(()) => 0,
        Err(Exit::Status(c)) => c,
    };
    ps.flush();
    std::process::ExitCode::from(u8::try_from(code).unwrap_or(1))
}

#[cfg(not(unix))]
fn main() -> std::process::ExitCode {
    // The host build is for the unit tests; `ps` itself needs a `/proc`.
    let _ = (Ps::new, Ps::run);
    eprintln!("ps: not supported on this host");
    std::process::ExitCode::FAILURE
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use std::fs;

    /// A fake `/proc` holding this test process (as `ps`, on terminal 34817,
    /// root's) and three others, with `self` a plain directory rather than a
    /// link so the tree works on the development host too.
    fn world(name: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("lane-b-ps-main-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let me = std::process::id();
        let put = |path: &str, text: &str| {
            let p = root.join(path);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, text).unwrap();
        };
        let proc_files = |dir: &str,
                          pid: u32,
                          comm: &str,
                          tty: u32,
                          uid: u32,
                          utime: u64,
                          cmdline: &str| {
            put(
                &format!("{dir}/stat"),
                &format!(
                    "{pid} ({comm}) S 1 {pid} {pid} {tty} {pid} 4194560 0 0 0 0 {utime} 0 0 0 20 0 1 0 100 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 17 0 0 0 0 0 0\n"
                ),
            );
            put(
                &format!("{dir}/status"),
                &format!(
                    "Name:\t{comm}\nPid:\t{pid}\nPPid:\t1\nUid:\t{uid}\t{uid}\t{uid}\t{uid}\nGid:\t0\t0\t0\t0\nVmRSS:\t100 kB\n"
                ),
            );
            put(&format!("{dir}/cmdline"), cmdline);
        };
        proc_files(&me.to_string(), me, "ps", 34817, 0, 0, "ps\0");
        proc_files("self", me, "ps", 34817, 0, 0, "ps\0");
        proc_files("100", 100, "bash", 34817, 0, 250, "-bash\0");
        proc_files("200", 200, "daemon", 0, 0, 12, "/usr/sbin/daemon\0--fork\0");
        proc_files("300", 300, "other", 34817, 1000, 0, "");
        put("uptime", "1000.00 2000.00\n");
        put(
            "stat",
            "cpu  1 2 3 4 5 6 7 8 0 0\ncpu0 1 2 3 4 5 6 7 8 0 0\nbtime 1700000000\n",
        );
        put("meminfo", "MemTotal: 1000 kB\n");
        put("sys/kernel/pid_max", "32768\n");
        root
    }

    fn ps(root: &Path, args: &[&str]) -> (String, i32) {
        let mut argv = vec![b"ps".to_vec()];
        argv.extend(args.iter().map(|a| a.as_bytes().to_vec()));
        let mut ps = Ps::new(root.to_path_buf(), argv, true, Zone::utc());
        let code = match ps.run() {
            Ok(()) => 0,
            Err(Exit::Status(c)) => c,
        };
        (
            String::from_utf8(std::mem::take(&mut ps.out)).unwrap(),
            code,
        )
    }

    use std::path::Path;

    #[test]
    fn selected_processes_in_a_chosen_format_and_order() {
        let root = world("select");
        let (out, code) = ps(
            &root,
            &["-p", "300,100,200", "-o", "pid,comm,args", "--sort=-pid"],
        );
        assert_eq!(code, 0);
        assert_eq!(
            out,
            "  PID COMMAND         COMMAND\n  \
             300 other           [other]\n  \
             200 daemon          /usr/sbin/daemon --fork\n  \
             100 bash            -bash\n"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn cpu_time_and_ownership_columns() {
        let root = world("cpu");
        let (out, _) = ps(
            &root,
            &[
                "-p",
                "100,200",
                "--sort=pid",
                "-o",
                "pid,time,bsdtime,uid,user",
            ],
        );
        // No password database on the test host: the user is the number.
        assert_eq!(
            out,
            "  PID     TIME   TIME   UID USER\n  \
             100 00:00:02   0:02     0 0\n  \
             200 00:00:00   0:00     0 0\n"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn the_default_selection_is_ours_on_our_terminal() {
        let root = world("default");
        let (out, _) = ps(&root, &["-o", "pid=", "--sort=pid"]);
        let me = std::process::id();
        // 300 is someone else's; 200 has no terminal.
        let mut want = vec![100u32, me];
        want.sort_unstable();
        let got: Vec<u32> = out.lines().map(|l| l.trim().parse().unwrap()).collect();
        assert_eq!(got, want);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn nothing_selected_is_the_header_and_status_one() {
        let root = world("none");
        let (out, code) = ps(&root, &["-p", "4242"]);
        assert_eq!(code, 1);
        assert_eq!(out, "  PID TTY          TIME CMD\n");
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_failed_first_reading_is_read_again_as_bsd() {
        let root = world("bsd");
        // `-p` needs a list; `-pzz` is not one, and as BSD `pzz` is not either:
        // the error printed is the first reading's.
        let (out, code) = ps(&root, &["-pzz"]);
        assert_eq!(code, 1);
        assert_eq!(out, "");
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn version_and_help_print_and_end() {
        let root = world("version");
        let (out, code) = ps(&root, &["V"]);
        assert_eq!(
            (out.as_str(), code),
            ("ps from SlateOS coreutils 0.1.0\n", 0)
        );
        let (out, code) = ps(&root, &["--help", "t"]);
        assert_eq!(code, 0);
        assert!(out.starts_with("\nUsage:\n ps [options]\n\nShow threads:\n"));
        assert!(out.ends_with("\nFor more details see ps(1).\n"));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_forest_draws_its_branches() {
        let root = world("forest");
        let me = std::process::id();
        // Re-parent 300 under 100.
        let stat = fs::read_to_string(root.join("300/stat"))
            .unwrap()
            .replacen(" S 1 ", " S 100 ", 1);
        fs::write(root.join("300/stat"), stat).unwrap();
        let status = fs::read_to_string(root.join("300/status"))
            .unwrap()
            .replace("PPid:\t1", "PPid:\t100");
        fs::write(root.join("300/status"), status).unwrap();
        let (out, _) = ps(&root, &["-e", "--forest", "-o", "pid,args"]);
        assert!(out.contains("  100 -bash\n  300  \\_ [other]\n"), "{out}");
        assert!(out.contains(&format!("{me:>5} ps\n")), "{out}");
        fs::remove_dir_all(&root).unwrap();
    }
}
