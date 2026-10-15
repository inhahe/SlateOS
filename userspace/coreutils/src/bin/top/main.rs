//! Display the system's tasks, a screen at a time, below a summary of the
//! system: procps-ng 4.0.4's `top`, ported.
//!
//! `src/top/top.c`, transcribed, with the hundred-odd interactive commands
//! of the original: four windows of field groups, sorting, filtering, the
//! forest view, the inspect screens, the bottom window, colours, and the
//! rcfile they are all saved in.
//!
//! Built as Ubuntu builds it (`--disable-modern-top`): the original defaults
//! -- a three second delay, one `%Cpu(s)` line, memory in MiB, the classic
//! columns -- and none of the development switches in `top.h`.
//!
//! # How it is put together
//!
//! Upstream's globals are the fields of [`Top`], and its functions are
//! methods on it, grouped as `top.c` groups them:
//!
//! | Module | `top.c` section |
//! |---|---|
//! | `term` | the terminal: capabilities, output, keys, line input |
//! | `fmt`, `utf8` | basic formatting, the multi-byte helpers |
//! | `fields` | the fields table, column headers, the Fields screen |
//! | `insp` | the inspect (`Y`) screens |
//! | `config` | the rcfile, read and written; the command line |
//! | `wins` | the windows, the forest view, the bottom window, filters |
//! | `keys` | every command |
//! | `show` | the summary, the task rows, the windows |
//!
//! The process table comes from `coreutils::procps::pids` (upstream's
//! `library/pids.c`), the CPU figures from `procps::stat`, the memory ones
//! from `procps::meminfo` -- the library layers `ps`, `free` and `vmstat`
//! read through as well.
//!
//! # Deliberate differences
//!
//! 1. **Signals are acted on where the program waits, not where they land.**
//!    Upstream ends the program, stops it or reports an unexpected signal
//!    from inside its handlers, calling `exit` and stdio there. Here a
//!    handler only records the signal and wakes the main loop (through a
//!    pipe the loop polls beside the keyboard); the loop then does exactly
//!    what upstream's handler did. Top spends its life waiting, so the two
//!    agree except for a signal that arrives mid-frame, which here takes
//!    effect when the frame is out.
//! 2. **A fault of the program's own is left to the system.** `SIGSEGV`,
//!    `SIGBUS`, `SIGFPE` and `SIGILL` keep their default action; upstream
//!    catches them too, to restore the terminal and print its "signal %d
//!    was caught" note. A handler that returns from a real fault would fault
//!    again for ever, and the only safe one would print nothing upstream's
//!    prints.
//! 3. **The version is this tree's**: `top from SlateOS coreutils 0.1.0`,
//!    and the same name on the help screen, where upstream names
//!    `procps-ng 4.0.4`.
//! 4. **No NUMA nodes.** Without `libnuma` there are none, as upstream's
//!    reference build has none either: `2` and `3` say the extensions are
//!    unavailable.

mod cfmt;
mod config;
mod defs;
mod fields;
mod fmt;
mod insp;
mod keys;
mod nls;
mod show;
mod term;
mod utf8;
mod wins;

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

use coreutils::procps::meminfo::{Mem, MemInfo};
use coreutils::procps::pids::{Counts, Fetch, Item, Pids};
use coreutils::procps::stat::{Reaped, StatInfo, TicStack};

use cfmt::{Arg, sprintf};
use defs::{
    BREAK_OFF, BREAK_SCREEN, BREAK_SIG, FIELDSTAB_LEN, Field, GROUPSMAX, LIB_USLEEP, MEDBUFSIZ,
    PFLAGSSIZ, PROC_XTRA, Rcf, Rcw, SHOW_TASKON, VIEW_CPUNOD, VIEW_CPUSUM, VIEW_SCROLL,
};
use fmt::Autox;
use term::{Caps, IoKey, IoLine, Out, Tinfo};

/// `PACKAGE_STRING`, as this tree names itself.
pub const PACKAGE_STRING: &str = "SlateOS coreutils 0.1.0";

/// Where the process table is read: `/proc`.
const PROC: &str = "/proc";

/// `BOT_ITEMMAX`: the bottom window's items, its fencepost among them.
pub const BOT_ITEMMAX: usize = 10;
/// `BOT_MSGSMAX`: the messages the log (`^L`) keeps.
pub const BOT_MSGSMAX: usize = 10;

/// `BOT_DELIMIT`, `BOT_ITEM_NS`, `BOT_MSG_LOG`, `BOT_UNFOCUS`: the bottom
/// window's fencepost, its two pseudo items, and "no tab focus".
pub const BOT_DELIMIT: i32 = -1;
pub const BOT_ITEM_NS: i32 = -2;
pub const BOT_MSG_LOG: i32 = -3;
pub const BOT_UNFOCUS: i32 = -1;

// ---------------------------------------------------------------------------
// Signals: recorded by the handlers, acted on by the main loop.
// ---------------------------------------------------------------------------

/// `Frames_signal`: why the next frame must rebuild its headers.
static FRAMES_SIGNAL: AtomicI32 = AtomicI32::new(BREAK_OFF);
/// `sig_endpgm` is due.
static PENDING_END: AtomicBool = AtomicBool::new(false);
/// `sig_paused` is due.
static PENDING_PAUSE: AtomicBool = AtomicBool::new(false);
/// `sig_abexit` is due, for this signal (0: none).
static PENDING_ABEXIT: AtomicI32 = AtomicI32::new(0);
/// The pipe's write end, which a handler writes a byte to so that the wait
/// it interrupted -- or is about to begin -- ends at once.
static WAKE_WRITE: AtomicI32 = AtomicI32::new(-1);

/// `Frames_signal`.
pub fn frames() -> i32 {
    FRAMES_SIGNAL.load(Ordering::SeqCst)
}

/// `Frames_signal = v`.
pub fn set_frames(v: i32) {
    FRAMES_SIGNAL.store(v, Ordering::SeqCst);
}

/// Wake the main loop's wait.
fn wake() {
    let fd = WAKE_WRITE.load(Ordering::SeqCst);
    if fd >= 0 {
        // A full pipe already wakes the loop; a failure changes nothing.
        let _ = libcall::fd::write(fd, &[1]);
    }
}

/// `sig_endpgm`'s handler half: SIGALRM, SIGHUP, SIGINT, SIGPIPE, SIGQUIT,
/// SIGTERM, SIGUSR1 and SIGUSR2.
extern "C" fn on_endpgm(_sig: i32) {
    let _errno = libcall::signal::ErrnoGuard::save();
    FRAMES_SIGNAL.store(BREAK_SIG, Ordering::SeqCst);
    PENDING_END.store(true, Ordering::SeqCst);
    wake();
}

/// `sig_paused`'s handler half: SIGTSTP, SIGTTIN and SIGTTOU.
extern "C" fn on_paused(_sig: i32) {
    let _errno = libcall::signal::ErrnoGuard::save();
    PENDING_PAUSE.store(true, Ordering::SeqCst);
    wake();
}

/// `sig_resize`: SIGCONT and SIGWINCH. (Upstream's `tcdrain` first only
/// waits for output already written.)
extern "C" fn on_resize(_sig: i32) {
    let _errno = libcall::signal::ErrnoGuard::save();
    FRAMES_SIGNAL.store(BREAK_SIG, Ordering::SeqCst);
    wake();
}

/// `sig_endpgm`, for the one place that puts it back by hand: the inspect
/// pipe, which ignores `SIGINT` while it runs.
#[must_use]
pub fn sig_endpgm_handler() -> libcall::signal::Handler {
    on_endpgm
}

/// A path or argument of bytes as the OS takes it: exact on the target, and
/// on the host (whose `OsStr` is not bytes) read as UTF-8.
#[must_use]
pub fn os_of(b: &[u8]) -> std::ffi::OsString {
    #[cfg(unix)]
    {
        std::os::unix::ffi::OsStringExt::from_vec(b.to_vec())
    }
    #[cfg(not(unix))]
    {
        std::ffi::OsString::from(String::from_utf8_lossy(b).into_owned())
    }
}

/// `sig_abexit`'s handler half: every other signal that can be caught.
extern "C" fn on_abexit(sig: i32) {
    let _errno = libcall::signal::ErrnoGuard::save();
    let _ = PENDING_ABEXIT.compare_exchange(0, sig, Ordering::SeqCst, Ordering::SeqCst);
    wake();
}

// ---------------------------------------------------------------------------
// The state
// ---------------------------------------------------------------------------

/// `struct osel_s`: one "other filter" of a window.
#[derive(Clone, Debug)]
pub struct Osel {
    /// `typ`: `o` (case ignored) or `O`.
    pub typ: u8,
    /// Include (`true`) or exclude.
    pub inc: bool,
    /// The field filtered.
    pub enu: usize,
    /// `<`, `>` or `=`.
    pub ops: u8,
    /// The value: as typed for `=`, else justified as the column is.
    pub val: Vec<u8>,
    /// What the user typed, to spot a duplicate and to save.
    pub raw: Vec<u8>,
}

/// `WIN_t`: one window (field group).
#[derive(Clone, Debug)]
pub struct Win {
    /// All active fields, the sort column's highlight pseudo fields among
    /// them.
    pub pflgsall: [i32; PFLAGSSIZ],
    /// The fields shown, from `begpflg` on.
    pub procflgs: [i32; PFLAGSSIZ],
    pub rc: Rcw,
    pub winnum: i32,
    pub winlines: i32,
    pub maxpflgs: i32,
    pub totpflgs: i32,
    pub begpflg: i32,
    pub endpflg: i32,
    pub begtask: i32,
    pub begnext: i32,
    pub varcolbeg: i32,
    pub varcolsz: i32,
    pub usrseluid: i32,
    pub usrseltyp: i32,
    pub usrselflg: i32,
    pub hdrcaplen: i32,
    pub capclr_sum: Vec<u8>,
    pub capclr_msg: Vec<u8>,
    pub capclr_pmt: Vec<u8>,
    pub capclr_hdr: Vec<u8>,
    pub capclr_rowhigh: Vec<u8>,
    pub capclr_rownorm: Vec<u8>,
    pub cap_bold: Vec<u8>,
    pub grpname: Vec<u8>,
    pub columnhdr: Vec<u8>,
    /// The other filters, first to be checked first (`osel_1st` on).
    pub osel: Vec<Osel>,
    pub findstr: Vec<u8>,
    pub findlen: i32,
    pub focus_pid: i32,
    pub focus_beg: i32,
    pub focus_end: i32,
    /// `ppt`: indices into the process table's stacks, in this window's
    /// order. It only grows; entries past the reading's task count are
    /// what earlier readings left there.
    pub ppt: Vec<usize>,
}

impl Win {
    fn new(rc: Rcw) -> Self {
        Self {
            pflgsall: [0; PFLAGSSIZ],
            procflgs: [0; PFLAGSSIZ],
            rc,
            winnum: 0,
            winlines: 0,
            maxpflgs: 0,
            totpflgs: 0,
            begpflg: 0,
            endpflg: 0,
            begtask: 0,
            begnext: 0,
            varcolbeg: 0,
            varcolsz: 0,
            usrseluid: 0,
            usrseltyp: 0,
            usrselflg: 0,
            hdrcaplen: 0,
            capclr_sum: Vec::new(),
            capclr_msg: Vec::new(),
            capclr_pmt: Vec::new(),
            capclr_hdr: Vec::new(),
            capclr_rowhigh: Vec::new(),
            capclr_rownorm: Vec::new(),
            cap_bold: Vec::new(),
            grpname: Vec::new(),
            columnhdr: Vec::new(),
            osel: Vec::new(),
            findstr: Vec::new(),
            findlen: 0,
            focus_pid: 0,
            focus_beg: 0,
            focus_end: 0,
            ppt: Vec::new(),
        }
    }

    /// `CHKw (q, f)`.
    #[must_use]
    pub fn chk(&self, f: i32) -> bool {
        self.rc.winflags & f != 0
    }

    /// `TOGw (q, f)`.
    pub fn tog(&mut self, f: i32) {
        self.rc.winflags ^= f;
    }

    /// `SETw (q, f)`.
    pub fn set(&mut self, f: i32) {
        self.rc.winflags |= f;
    }

    /// `OFFw (q, f)`.
    pub fn off(&mut self, f: i32) {
        self.rc.winflags &= !f;
    }

    /// `osel_tot`.
    #[must_use]
    pub fn osel_tot(&self) -> i32 {
        i32::try_from(self.osel.len()).unwrap_or(i32::MAX)
    }

    /// `captab[n]`: what `show_special`'s `~n` switches to.
    #[must_use]
    pub fn captab<'a>(&'a self, n: usize, norm: &'a [u8]) -> &'a [u8] {
        match n {
            2 => &self.cap_bold,
            3 => &self.capclr_sum,
            4 => &self.capclr_msg,
            5 => &self.capclr_pmt,
            6 => &self.capclr_hdr,
            7 => &self.capclr_rowhigh,
            8 => &self.capclr_rownorm,
            _ => norm,
        }
    }
}

/// `Winstk`: the four windows, indexed by a window's number less one.
///
/// Every index comes from `curwin`, a walk over the four, or
/// [`Top::next_win`] and [`Top::prev_win`] -- so is below four -- and is
/// taken modulo four besides, so that none can fall outside.
#[derive(Clone, Debug)]
pub struct Wins(pub [Win; GROUPSMAX]);

impl Wins {
    /// The four, in order.
    pub fn iter(&self) -> std::slice::Iter<'_, Win> {
        self.0.iter()
    }

    /// The four, in order, to change.
    pub fn iter_mut(&mut self) -> std::slice::IterMut<'_, Win> {
        self.0.iter_mut()
    }
}

impl<'a> IntoIterator for &'a Wins {
    type Item = &'a Win;
    type IntoIter = std::slice::Iter<'a, Win>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl<'a> IntoIterator for &'a mut Wins {
    type Item = &'a mut Win;
    type IntoIter = std::slice::IterMut<'a, Win>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter_mut()
    }
}

impl std::ops::Index<usize> for Wins {
    type Output = Win;

    fn index(&self, i: usize) -> &Win {
        let [a, b, c, d] = &self.0;
        match i % GROUPSMAX {
            0 => a,
            1 => b,
            2 => c,
            _ => d,
        }
    }
}

impl std::ops::IndexMut<usize> for Wins {
    fn index_mut(&mut self, i: usize) -> &mut Win {
        let [a, b, c, d] = &mut self.0;
        match i % GROUPSMAX {
            0 => a,
            1 => b,
            2 => c,
            _ => d,
        }
    }
}

/// `struct graph_parms`: the bars of the `t` and `m` toggles.
#[derive(Clone, Copy, Debug, Default)]
pub struct Graph {
    pub adjust: f32,
    pub length: i32,
    pub style: i32,
    pub total: i64,
    pub part1: i64,
    pub part2: i64,
}

/// Which way the bottom window draws: one string cut at a separator, or a
/// vector.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BotFocus {
    Str,
    Strv,
}

/// The bottom window: `Bot_*`.
#[derive(Clone, Debug)]
pub struct Bot {
    pub task: i32,
    pub what: i32,
    pub rsvd: i32,
    pub indx: i32,
    pub item: [i32; BOT_ITEMMAX],
    pub sep: u8,
    pub head: &'static str,
    pub buf: Vec<u8>,
    pub focus: BotFocus,
    /// `Bot_show_func != NULL`.
    pub show: bool,
    /// `bot_focus_strv`'s `nsav`.
    pub nsav: i32,
}

impl Default for Bot {
    fn default() -> Self {
        let mut item = [0; BOT_ITEMMAX];
        item[0] = BOT_DELIMIT;
        Self {
            task: 0,
            what: 0,
            rsvd: 0,
            indx: BOT_UNFOCUS,
            item,
            sep: 0,
            head: "",
            buf: Vec::new(),
            focus: BotFocus::Str,
            show: false,
            nsav: 0,
        }
    }
}

impl Bot {
    /// `BOT_PRESENT`.
    #[must_use]
    pub fn present(&self) -> bool {
        self.what != 0
    }

    /// `BOT_TOSS`.
    pub fn toss(&mut self) {
        self.show = false;
        self.item[0] = BOT_DELIMIT;
        self.task = 0;
        self.rsvd = 0;
        self.what = 0;
        self.indx = BOT_UNFOCUS;
    }
}

/// `Rel_memitems`: the memory figures `MEM_VAL` reads.
#[derive(Clone, Copy, Debug)]
pub enum MemItem {
    Fre,
    Use,
    Tot,
    Que,
    Buf,
    Avl,
    SwpTot,
    SwpFre,
    SwpUse,
}

/// The statics of upstream's functions that the frame logic keeps.
#[derive(Debug, Default)]
pub struct Statics {
    /// `tasks_refresh`: `uptime_sav`, `n_alloc`.
    pub uptime_sav: f64,
    pub n_alloc: i32,
    /// `memory_refresh`: `sav_secs`.
    pub mem_sav_secs: i64,
    /// `adj_geometry`: `w_set`, `w_cols`, `w_rows`.
    pub w_set: bool,
    pub w_cols: i32,
    pub w_rows: i32,
    /// `display_fields`: `col_sav`, `row_sav`.
    pub col_sav: i32,
    pub row_sav: i32,
    /// `sum_see`: `row`, `tog`.
    pub see_row: Vec<u8>,
    pub see_tog: i32,
    /// `sum_unify`: the accumulated stack, `ix`, `beg`.
    pub unify: TicStack,
    pub unify_ix: i32,
    pub unify_beg: i32,
    /// `forest_begin`: `hwmsav` is `tree_ppt`'s length.
    pub tree_ppt: Vec<usize>,
    pub tree_idx: i32,
    /// `Hide_pid`, `Hide_tot`.
    pub hide_pid: Vec<i32>,
    pub hide_tot: i32,
    /// `keys_task`'s `i`: `w_sav` (a window number, or none) and `beg_sav`.
    pub i_w_sav: Option<usize>,
    pub i_beg_sav: i32,
    /// `find_string`'s and `insp_find_str`'s `found`.
    pub find_found: bool,
    pub insp_found: bool,
    /// `inspection_utility`'s `sel`.
    pub insp_sel: i32,
    /// `zap_fieldstab`'s `once`.
    pub zap_once: bool,
    /// `wins_clrhlp`'s saved colours: flags, summary, messages, headers,
    /// tasks.
    pub clrsav: [i32; 5],
}

/// One inspect choice: `struct I_ent`.
#[derive(Clone, Debug)]
pub struct IEnt {
    pub func: insp::Func,
    pub typ: Vec<u8>,
    pub name: Vec<u8>,
    pub fmts: Vec<u8>,
    pub caps: &'static [u8],
    pub fstr: Vec<u8>,
    pub flen: i32,
}

/// `struct I_struc`: the inspect choices.
#[derive(Clone, Debug, Default)]
pub struct Inspect {
    pub demo: bool,
    pub total: i32,
    /// Every inspect line of the rcfile, comments and blanks too, for `W`.
    pub raw: Option<Vec<u8>>,
    pub tab: Vec<IEnt>,
}

/// What an inspect choice produced: `Insp_buf` and the rest.
#[derive(Clone, Debug, Default)]
pub struct InspView {
    /// `Insp_p`: where each line starts in `buf`, and one past the last.
    pub p: Vec<usize>,
    pub nl: i32,
    pub utf8: bool,
    /// `Insp_buf`: what was read, then a NUL, then the zeroed rest of the
    /// allocation.
    pub buf: Vec<u8>,
    pub bufrd: usize,
    /// `Insp_sel`, an index into the choices.
    pub sel: usize,
}

/// Everything upstream keeps in globals.
pub struct Top {
    pub out: Out,
    pub ti: Tinfo,
    pub caps: Caps,
    pub iokey: IoKey,
    pub ioline: IoLine,
    /// `Tty_original`, `Tty_raw`, `Ttychanged`.
    pub tty_original: libcall::termios::Termios,
    pub tty_raw: libcall::termios::Termios,
    pub ttychanged: bool,
    /// `Cursor_state`.
    pub cursor_state: Vec<u8>,
    pub myname: Vec<u8>,
    pub rc_name: Vec<u8>,
    pub rc: Rcf,
    pub rc_questions: bool,
    pub rc_compatibility: bool,
    pub hertz: i64,
    pub cpu_cnt: i32,
    pub cpu_pmax: f32,
    pub cpu_states_fmts: &'static str,
    pub monpids: [i32; defs::MONPIDMAX + 1],
    pub monpidsidx: i32,
    pub screen_cols: i32,
    pub screen_rows: i32,
    pub max_lines: i32,
    pub bot: Bot,
    pub msg_row: i32,
    pub batch: bool,
    pub loops: i32,
    pub secure_mode: bool,
    pub width_mode: i32,
    pub thread_mode: bool,
    pub winstk: Wins,
    /// `Curwin`, as an index into `winstk`.
    pub curwin: usize,
    pub frame_etscale: f32,
    pub autox: Autox,
    pub numa_node_tot: i32,
    pub numa_node_sel: i32,
    pub graph_cpus: Graph,
    pub graph_mems: Graph,
    pub fieldstab: [Field; FIELDSTAB_LEN],
    pub pids_itms: Vec<Item>,
    pub pids: Pids,
    /// `Pids_reap->counts`, of the last reading.
    pub counts: Counts,
    pub stat: Option<StatInfo>,
    pub stat_reap: Reaped,
    pub mem: Option<MemInfo>,
    pub mem_stack: Mem,
    pub restrict_some: bool,
    /// `Msg_tab` and `Msg_this`.
    pub msg_tab: Vec<Vec<u8>>,
    pub msg_this: usize,
    pub inspect: Inspect,
    pub insp: InspView,
    pub st: Statics,
    /// The locale's decimal point, for reading a number the user typed.
    pub radix: u8,
    /// The pipe the signal handlers wake the main loop through: its read
    /// end, or -1.
    pub wake_read: i32,
    /// The time zone `localtime_r` converts the uptime line's clock with,
    /// read once as glibc reads it.
    pub zone: Option<localtime::Zone>,
}

impl Top {
    /// `Curwin`.
    #[must_use]
    pub fn cw(&self) -> &Win {
        &self.winstk[self.curwin]
    }

    /// `Curwin`, to change.
    pub fn cwm(&mut self) -> &mut Win {
        &mut self.winstk[self.curwin]
    }

    /// `w->next`.
    #[must_use]
    pub fn next_win(i: usize) -> usize {
        i.wrapping_add(1) % GROUPSMAX
    }

    /// `w->prev`.
    #[must_use]
    pub fn prev_win(i: usize) -> usize {
        i.wrapping_add(GROUPSMAX - 1) % GROUPSMAX
    }

    /// `PIDSmaxt`: the last reading's task count.
    #[must_use]
    pub fn pidsmaxt(&self) -> i32 {
        self.counts.total
    }

    /// `SCREEN_ROWS`: the rows above the bottom window.
    #[must_use]
    pub fn screen_rows_bot(&self) -> i32 {
        self.screen_rows.saturating_sub(self.bot.rsvd)
    }

    /// `MEM_VAL (e)`.
    #[must_use]
    pub fn mem_val(&self, e: MemItem) -> u64 {
        show::mem_val_of(&self.mem_stack, e)
    }

    /// The stack window `w`'s row `idx` shows: `q->ppt[idx]`.
    #[must_use]
    pub fn stack_of(&self, w: usize, idx: i32) -> Option<&coreutils::procps::pids::Stack> {
        let i = usize::try_from(idx).ok()?;
        let at = *self.winstk[w].ppt.get(i)?;
        self.pids.stack_any(at)
    }

    /// `PID_VAL (e, s_int, q->ppt[idx])`.
    #[must_use]
    pub fn pv_int(&self, w: usize, idx: i32, e: usize) -> i32 {
        self.stack_of(w, idx)
            .and_then(|s| s.get(e))
            .map_or(0, coreutils::procps::pids::Val::s_int)
    }

    /// `PID_VAL (e, str, q->ppt[idx])`, a NULL as empty.
    #[must_use]
    pub fn pv_str(&self, w: usize, idx: i32, e: usize) -> Vec<u8> {
        self.stack_of(w, idx)
            .and_then(|s| s.get(e))
            .and_then(coreutils::procps::pids::Val::str)
            .map(<[u8]>::to_vec)
            .unwrap_or_default()
    }

    /// `fmtmk (fmts, ...)`: a format and its arguments, in upstream's
    /// 2048-byte buffer.
    #[must_use]
    pub fn fmtmk(fmt: &str, args: &[Arg<'_>]) -> Vec<u8> {
        cfmt::snprintf(defs::BIGBUFSIZ, fmt.as_bytes(), args).0
    }

    // -----------------------------------------------------------------------
    // Startup
    // -----------------------------------------------------------------------

    /// `before (me)`: the program's name, the locale, `/proc`, the CPU and
    /// memory contexts and the process table's -- everything that does not
    /// depend on the rcfile or the command line.
    fn before(me: &[u8]) -> Self {
        // `procps_linux_version`, read before anything else.
        let linux_version_code = linux_version();

        let myname = match me.iter().rposition(|&b| b == b'/') {
            Some(at) => me.get(at.saturating_add(1)..).unwrap_or_default().to_vec(),
            None => me.to_vec(),
        };

        // `initialize_nls`: `setlocale (LC_ALL, "")`.
        let mut name = [0u8; 256];
        let _ = libcall::locale::select_from_env(libcall::locale::LC_ALL, &mut name);
        let mut point = [0u8; 16];
        let radix = libcall::locale::radix(&mut point)
            .and_then(|n| point.get(..n))
            .and_then(|p| p.first().copied())
            .unwrap_or(b'.');
        cfmt::set_radix(radix);
        let utf8 = {
            let mut cs = [0u8; 64];
            libcall::locale::codeset(&mut cs)
                .and_then(|n| cs.get(..n))
                .is_some_and(|c| c.eq_ignore_ascii_case(b"UTF-8"))
        };

        // `fatal_proc_unmounted (NULL, 0)`: `look_up_our_self`.
        let root = PathBuf::from(PROC);
        if !coreutils::procps::readproc::self_stat_readable(&root) {
            let _ = libcall::fd::write(2, b"Error, do this: mount -t proc proc /proc\n");
            exit_now(47);
        }

        // The stderr redirect: what reaches descriptor 2 during the run is
        // discarded until `at_eoj` puts it back, and what goes through the
        // `stderr` stream waits for the exit (`Out::err`).
        let stderr_save = term::redirect_stderr();

        let hertz = coreutils::procps::hertz();
        let cpu_states_fmts = if linux_version_code >= linux_code(2, 6, 11) {
            nls::STATE_LIN2X7_FMT
        } else {
            nls::STATE_LIN2X6_FMT
        };

        let mut restrict_some = false;
        let cpu_cnt;
        let mut numa_node_tot = 0;
        let mut stat_reap = Reaped::default();
        let stat = match StatInfo::new(&root) {
            Err(_) => {
                restrict_some = true;
                cpu_cnt = 1;
                None
            }
            Ok(mut s) => match s.reap(true) {
                Ok(r) => {
                    numa_node_tot = i32::try_from(r.numa.len()).unwrap_or(0);
                    cpu_cnt = i32::try_from(r.cpus.len()).unwrap_or(0);
                    stat_reap = r;
                    Some(s)
                }
                Err(e) => {
                    let text = Self::fmtmk(
                        nls::LIB_ERRORCPU_FMT,
                        &[Arg::I(i64::from(line!())), Arg::S(&strerror_of(&e))],
                    );
                    term::restore_stderr(stderr_save);
                    early_exit(&myname, &text);
                }
            },
        };

        let mem = match MemInfo::new() {
            Ok(m) => Some(m),
            Err(_) => {
                restrict_some = true;
                None
            }
        };

        let pids_itms = vec![Item::Noop; FIELDSTAB_LEN];
        let page_size = libcall::conf::page_size();
        let pids = Pids::new(
            &root,
            &pids_itms,
            u64::try_from(hertz).unwrap_or(100),
            page_size,
            utf8,
        );

        let rc = defs::def_rcfile();
        let winstk = Wins(rc.win.clone().map(Win::new));
        let top = Self {
            out: Out::new(stderr_save),
            ti: Tinfo::default(),
            caps: Caps::default(),
            iokey: IoKey::default(),
            ioline: IoLine::default(),
            tty_original: libcall::termios::Termios::default(),
            tty_raw: libcall::termios::Termios::default(),
            ttychanged: false,
            cursor_state: Vec::new(),
            myname,
            rc_name: Vec::new(),
            rc,
            rc_questions: false,
            rc_compatibility: false,
            hertz,
            cpu_cnt,
            cpu_pmax: 0.0,
            cpu_states_fmts,
            monpids: [0; defs::MONPIDMAX + 1],
            monpidsidx: 0,
            screen_cols: 0,
            screen_rows: 0,
            max_lines: 0,
            bot: Bot::default(),
            msg_row: 0,
            batch: false,
            loops: -1,
            secure_mode: false,
            width_mode: 0,
            thread_mode: false,
            winstk,
            curwin: 0,
            frame_etscale: 0.0,
            autox: Autox::default(),
            numa_node_tot,
            numa_node_sel: -1,
            graph_cpus: Graph::default(),
            graph_mems: Graph::default(),
            fieldstab: defs::fieldstab(),
            pids_itms,
            pids,
            counts: Counts::default(),
            stat,
            stat_reap,
            mem,
            mem_stack: Mem::default(),
            restrict_some,
            msg_tab: vec![Vec::new(); BOT_MSGSMAX],
            msg_this: 0,
            inspect: Inspect::default(),
            insp: InspView::default(),
            st: Statics {
                n_alloc: -1,
                ..Statics::default()
            },
            radix,
            wake_read: -1,
            zone: None,
        };

        // "don't distort startup cpu(s) display ..."
        nap(LIB_USLEEP);
        top
    }

    /// `wins_stage_1`: each window its number and its rcfile defaults.
    fn wins_stage_1(&mut self) {
        for (i, w) in self.winstk.iter_mut().enumerate() {
            w.winnum = i32::try_from(i).unwrap_or(0).saturating_add(1);
            w.rc = self.rc.win.get(i).cloned().unwrap_or_else(|| w.rc.clone());
        }
        self.curwin = 0;
    }

    /// `wins_stage_2`: what is left once the rcfile and the command line
    /// have had their say -- names, colours, the fields table, the first
    /// visible row.
    fn wins_stage_2(&mut self) {
        for i in 0..GROUPSMAX {
            let name = self.winstk[i].rc.winname.clone();
            self.win_names(i, &name);
            self.capsmk(i);
            self.winstk[i].findstr.clear();
            self.winstk[i].findlen = 0;
            if self.winstk[i].rc.combine_cpus >= self.cpu_cnt {
                self.winstk[i].rc.combine_cpus = 0;
            }
            if self.winstk[i].chk(VIEW_CPUSUM | VIEW_CPUNOD) {
                self.winstk[i].rc.double_up = 0;
            }
        }
        if self.batch {
            self.cwm().off(VIEW_SCROLL);
            // "allow running under nohup"
            let _ = libcall::signal::ignore(libcall::signal::SIGHUP);
        } else {
            self.cursor_state = self.caps.curs_hide.clone();
            let s = self.cursor_state.clone();
            self.out.putp(&s);
        }
        self.zap_fieldstab();
        // `mkVIZrow1`.
        let w = self.cwm();
        w.begnext = 1;
        w.begtask = w.begtask.saturating_sub(1);
    }

    /// `signals_set`: every signal that can be caught, caught -- and a pipe
    /// for the handlers to wake the main loop through.
    fn signals_set(&mut self) {
        use libcall::signal as sig;
        if let Ok((r, w)) = libcall::fd::pipe() {
            // Neither end may reach a child, and a full pipe must not
            // block a handler.
            let _ = libcall::fd::set_close_on_exec(r, true);
            let _ = libcall::fd::set_close_on_exec(w, true);
            if let Ok(fl) = libcall::termios::status_flags(w) {
                let _ = libcall::termios::set_status_flags(w, fl | O_NONBLOCK);
            }
            if let Ok(fl) = libcall::termios::status_flags(r) {
                let _ = libcall::termios::set_status_flags(r, fl | O_NONBLOCK);
            }
            self.wake_read = r;
            WAKE_WRITE.store(w, Ordering::SeqCst);
        }
        for i in (1..=libcall::sigrtmax()).rev() {
            let handler: sig::Handler = match i {
                libcall::signal::SIGHUP => {
                    if self.batch {
                        let _ = sig::ignore(i);
                        continue;
                    }
                    on_endpgm
                }
                sig::SIGALRM
                | sig::SIGINT
                | libcall::signal::SIGPIPE
                | sig::SIGQUIT
                | libcall::signal::SIGTERM
                | SIGUSR1
                | SIGUSR2 => on_endpgm,
                sig::SIGTSTP | sig::SIGTTIN | sig::SIGTTOU => on_paused,
                libcall::signal::SIGCONT | sig::SIGWINCH => on_resize,
                libcall::signal::SIGKILL | libcall::signal::SIGSTOP | sig::SIGCHLD => continue,
                // Deliberate difference 2: the program's own faults.
                SIGILL | SIGBUS | SIGFPE | SIGSEGV => continue,
                _ => on_abexit,
            };
            // "with user position preserved through SIGWINCH, we must avoid
            // SA_RESTART". glibc refuses its own two signals; so does this.
            let _ = sig::set_handler(i, handler, false);
        }
    }

    // -----------------------------------------------------------------------
    // Exit and interrupt routines
    // -----------------------------------------------------------------------

    /// Act on whatever the signal handlers recorded since the last look:
    /// what their upstream counterparts did on the spot.
    pub fn check_signals(&mut self) {
        loop {
            // Drain the wake-up bytes.
            if self.wake_read >= 0 {
                let mut buf = [0u8; 64];
                while let Ok(n) = libcall::fd::read(self.wake_read, &mut buf) {
                    if n < buf.len() {
                        break;
                    }
                }
            }
            let ab = PENDING_ABEXIT.swap(0, Ordering::SeqCst);
            if ab != 0 {
                self.sig_abexit(ab);
            }
            if PENDING_END.swap(false, Ordering::SeqCst) {
                // `sig_endpgm`.
                set_frames(BREAK_SIG);
                self.bye_bye(None);
            }
            if !PENDING_PAUSE.swap(false, Ordering::SeqCst) {
                return;
            }
            self.sig_paused();
            // What arrived while the program was stopped -- the `SIGCONT`
            // that woke it, at least -- upstream's handlers have acted on
            // by the time `sig_paused` returns, nested inside it: their
            // wake-up bytes must not buy the loop a second frame.
        }
    }

    /// `bye_bye (str)`: the terminal restored, then the end -- with `str`
    /// on standard error and a failure, or (in batch mode) a last newline
    /// and success.
    pub fn bye_bye(&mut self, s: Option<&[u8]>) -> ! {
        block_all_signals();
        self.at_eoj();
        if let Some(s) = s {
            self.out.err(s);
            self.exit_with(ExitCode::FAILURE);
        }
        if self.batch {
            // An async-signal-safe `write`, past stdio.
            let _ = libcall::fd::write(1, b"\n");
        }
        self.exit_with(ExitCode::SUCCESS);
    }

    /// `exit (status)`: procps' `close_stdout` has the last word.
    fn exit_with(&mut self, earned: ExitCode) -> ! {
        // `stderr` flushed first: `close_stdout`'s complaint would follow
        // what it holds in the same buffer.
        self.out.err_flush();
        let status = match self.out.stream.take() {
            Some(stream) => coreutils::stdfd::close_stdout_procps(&self.myname, stream, earned),
            None => earned,
        };
        std::process::exit(i32::from(status != ExitCode::SUCCESS));
    }

    /// `error_exit (str)`: `top: str` and a failure.
    pub fn error_exit(&mut self, s: &[u8]) -> ! {
        set_frames(BREAK_OFF);
        let buf = cfmt::snprintf(MEDBUFSIZ, b"%s: %s\n", &[Arg::S(&self.myname), Arg::S(s)]).0;
        self.bye_bye(Some(&buf));
    }

    /// `sig_abexit (sig)`: the terminal restored, a note on standard error,
    /// and the signal raised again with its default action.
    fn sig_abexit(&mut self, sig: i32) -> ! {
        block_all_signals();
        self.at_eoj();
        let name = coreutils::procps::signals::signal_number_to_name(sig, libcall::sigrtmin());
        let note = sprintf(
            nls::EXIT_SIGNALS_FMT.as_bytes(),
            &[
                Arg::I(i64::from(sig)),
                Arg::S(name.as_bytes()),
                Arg::S(&self.myname),
            ],
        );
        // Held in the buffered `stderr`, and lost with it: `raise` and
        // `_exit` flush nothing.
        self.out.err(&note);
        let _ = libcall::signal::set_default(sig);
        let mut one = libcall::signal::SigSet::empty();
        let _ = one.add(sig);
        let _ = libcall::signal::unblock(&one);
        let _ = libcall::signal::raise(sig);
        exit_now(1);
    }

    /// `sig_paused`: the terminal as it was, the process stopped; on
    /// resuming, the terminal as top keeps it and a full redraw.
    fn sig_paused(&mut self) {
        use libcall::termios::{TCSAFLUSH, set_attr};
        if let Err(e) = set_attr(0, TCSAFLUSH, &self.tty_original) {
            let msg = Self::fmtmk(nls::FAIL_TTY_SET_FMT, &[Arg::S(&strerror_no(e))]);
            self.error_exit(&msg);
        }
        if let Some(kl) = self.ti.keypad_local.clone() {
            self.out.putp(&kl);
        }
        let g = self.tg2(0, self.screen_rows);
        self.out.putp(&g);
        let c = self.caps.curs_norm.clone();
        self.out.putp(&c);
        let c = self.caps.smam.clone();
        self.out.putp(&c);
        self.out.flush();
        let _ = libcall::signal::raise(libcall::signal::SIGSTOP);
        // "later, after SIGCONT..."
        if let Err(e) = set_attr(0, TCSAFLUSH, &self.tty_raw) {
            let msg = Self::fmtmk(nls::FAIL_TTY_SET_FMT, &[Arg::S(&strerror_no(e))]);
            self.error_exit(&msg);
        }
        let c = self.caps.rmam.clone();
        self.out.putp(&c);
        if let Some(kx) = self.ti.keypad_xmit.clone() {
            self.out.putp(&kx);
        }
        let c = self.cursor_state.clone();
        self.out.putp(&c);
        set_frames(BREAK_SIG);
    }

    // -----------------------------------------------------------------------
    // The library
    // -----------------------------------------------------------------------

    /// `cpus_refresh`: the CPU (and node) ticks since the last reading.
    pub fn cpus_refresh(&mut self) {
        let numa_too = self.cw().chk(VIEW_CPUNOD);
        let Some(stat) = self.stat.as_mut() else {
            return;
        };
        match stat.reap(numa_too) {
            Ok(r) => self.stat_reap = r,
            Err(e) => {
                let msg = Self::fmtmk(
                    nls::LIB_ERRORCPU_FMT,
                    &[Arg::I(i64::from(line!())), Arg::S(&strerror_of(&e))],
                );
                self.error_exit(&msg);
            }
        }
        let numa = i32::try_from(self.stat_reap.numa.len()).unwrap_or(0);
        if numa != 0 && numa != self.numa_node_tot {
            self.numa_node_tot = numa;
            self.numa_node_sel = -1;
        }
        let cpus = i32::try_from(self.stat_reap.cpus.len()).unwrap_or(0);
        if cpus != 0 && cpus != self.cpu_cnt {
            self.cpu_cnt = cpus;
        }
    }

    /// `memory_refresh`: `/proc/meminfo`, at most every three seconds
    /// unless a frame was interrupted.
    pub fn memory_refresh(&mut self) {
        if frames() != BREAK_OFF {
            self.st.mem_sav_secs = 0;
        }
        let cur_secs = now_secs();
        if 3 <= cur_secs.saturating_sub(self.st.mem_sav_secs) {
            let Some(mem) = self.mem.as_mut() else {
                return;
            };
            match mem.select() {
                Ok(m) => self.mem_stack = m,
                Err(e) => {
                    let msg = Self::fmtmk(
                        nls::LIB_ERRORMEM_FMT,
                        &[Arg::I(i64::from(line!())), Arg::S(&strerror_of(&e))],
                    );
                    self.error_exit(&msg);
                }
            }
            self.st.mem_sav_secs = cur_secs;
        }
    }

    /// `tasks_refresh`: a new reading of the tasks (or threads), the frame's
    /// CPU scale, and every window's view of it reset to the reading's
    /// order.
    pub fn tasks_refresh(&mut self) {
        let uptime_cur = coreutils::procps::sysinfo::uptime(std::path::Path::new(PROC));
        // `float et = uptime_cur - uptime_sav`.
        let mut et = c_float(uptime_cur - self.st.uptime_sav);
        if et < 0.01 {
            et = 0.005;
        }
        self.st.uptime_sav = uptime_cur;
        // `100.0f / ((float)Hertz * (float)et * (Rc.mode_irixps ? 1 : Cpu_cnt))`
        let per = if self.rc.mode_irixps != 0 {
            1.0f32
        } else {
            c_float(f64::from(self.cpu_cnt))
        };
        self.frame_etscale = 100.0f32 / (c_float_i(self.hertz) * et * per);

        let fetch = if self.thread_mode {
            Fetch::ThreadsToo
        } else {
            Fetch::TasksOnly
        };
        let reaped = if self.monpidsidx > 0 {
            let n = usize::try_from(self.monpidsidx).unwrap_or(0);
            let list: Vec<u32> = self
                .monpids
                .iter()
                .take(n)
                .map(|&p| p.cast_unsigned())
                .collect();
            self.pids.select(&list, fetch)
        } else {
            self.pids.reap(fetch)
        };
        let Some(counts) = reaped else {
            let msg = Self::fmtmk(
                nls::LIB_ERRORPID_FMT,
                &[
                    Arg::I(i64::from(line!())),
                    Arg::S(&strerror_no(last_errno_or(5))),
                ],
            );
            self.error_exit(&msg);
        };
        self.counts = counts;

        let n_reap = counts.total;
        let total = usize::try_from(n_reap).unwrap_or(0);
        if self.st.n_alloc < n_reap {
            // `nALGN2 (n_reap, 128)`.
            self.st.n_alloc = n_reap.saturating_add(127) & !127;
            let n_alloc = usize::try_from(self.st.n_alloc).unwrap_or(0);
            for w in &mut self.winstk {
                if w.ppt.len() < n_alloc {
                    w.ppt.resize(n_alloc, 0);
                }
            }
        }
        for w in &mut self.winstk {
            for (i, slot) in w.ppt.iter_mut().take(total).enumerate() {
                *slot = i;
            }
        }
    }

    // -----------------------------------------------------------------------
    // The frame
    // -----------------------------------------------------------------------

    /// `frame_hlp (wix, max)`: window `wix`'s share of the rows left.
    fn frame_hlp(&mut self, wix: usize, max: i32) {
        let mut wins = i32::try_from(
            self.winstk
                .iter()
                .skip(wix)
                .filter(|w| w.chk(SHOW_TASKON))
                .count(),
        )
        .unwrap_or(1);
        if wins == 0 {
            wins = 1;
        }
        let size = max.wrapping_sub(wins).checked_div(wins).unwrap_or(0);
        let w = &mut self.winstk[wix];
        w.winlines = if w.rc.maxtasks != 0 {
            w.rc.maxtasks
        } else {
            size
        };
    }

    /// `frame_make`: one full screen -- the summary, then each window.
    pub fn frame_make(&mut self) {
        if self.rc.fixed_widest < 0 && self.autox.found {
            self.widths_resize();
        }
        let fs = frames();
        if fs != BREAK_OFF {
            if fs == BREAK_SIG || fs == BREAK_SCREEN {
                self.bot.toss();
            }
            self.zap_fieldstab();
        }

        self.tasks_refresh();
        if !self.restrict_some {
            self.cpus_refresh();
            self.memory_refresh();
        }

        // "whoa either first time or thread/task mode change, (re)prime the
        // pump..."
        if self.out.pseudo_row == PROC_XTRA {
            nap(LIB_USLEEP);
            self.tasks_refresh();
            let c = self.caps.clr_scr.clone();
            self.out.putp(&c);
        } else if self.batch {
            self.out.putp(b"\n\n");
        } else {
            let c = self.caps.home.clone();
            self.out.putp(&c);
        }

        self.st.tree_idx = 0;
        self.out.pseudo_row = 0;
        self.msg_row = 0;
        let mut scrlins = 0;
        self.summary_show();
        self.max_lines = self
            .screen_rows_bot()
            .wrapping_sub(self.msg_row)
            .wrapping_sub(1);

        // "we're now on Msg_row so clear out any residual messages ..."
        let c = self.caps.clr_eol.clone();
        self.out.putp(&c);

        if self.rc.mode_altscr == 0 {
            let w = self.curwin;
            let ml = self.max_lines;
            self.winstk[w].winlines = if self.winstk[w].rc.maxtasks != 0 {
                self.winstk[w].rc.maxtasks
            } else {
                ml
            };
            scrlins = self.window_show(w, ml);
        } else {
            for i in 0..GROUPSMAX {
                if self.winstk[i].chk(SHOW_TASKON) {
                    self.frame_hlp(i, self.max_lines.wrapping_sub(scrlins));
                    let got = self.window_show(i, self.max_lines.wrapping_sub(scrlins));
                    scrlins = scrlins.wrapping_add(got);
                }
                if self.max_lines <= scrlins {
                    break;
                }
            }
        }

        // "clear to end-of-screen - critical if last window is 'idleps off'"
        if !self.batch && scrlins < self.max_lines {
            if self.bot.present() {
                let from = scrlins.wrapping_add(self.msg_row).wrapping_add(1);
                for i in from..self.screen_rows_bot() {
                    let g = self.tg2(0, i);
                    self.out.putp(&g);
                    let c = self.caps.clr_eol.clone();
                    self.out.putp(&c);
                }
            } else {
                let c = self.caps.nl_clreos.clone();
                self.out.putp(&c);
            }
            let r = self.out.pseudo_row;
            self.out.psu_clreos(r);
        }

        if self.cw().chk(VIEW_SCROLL) && self.viz_is(self.curwin) {
            self.show_scroll();
        }
        if self.bot.show {
            self.bot_item_show();
        }
        self.out.flush();

        // "we'll deem any terminal not supporting tgoto as dumb and disable
        // the normal non-interactive output optimization..."
        if !self.ti.can_goto {
            self.out.psu_clreos(0);
        }
    }

    /// `VIZISw (w)`: whether window `w` shows tasks.
    #[must_use]
    pub fn viz_is(&self, w: usize) -> bool {
        self.rc.mode_altscr == 0 || self.winstk[w].chk(SHOW_TASKON)
    }

    /// The main loop: a frame, then the delay -- or a key.
    fn run(&mut self) -> ! {
        loop {
            self.frame_make();
            if 0 < self.loops {
                self.loops = self.loops.wrapping_sub(1);
            }
            if self.loops == 0 {
                self.bye_bye(None);
            }
            let ms = delay_ms(self.rc.delay_time);
            if self.batch {
                self.wait_signals(ms);
            } else if self.ioa(Some(ms)) {
                let k = self.iokey(term::IOKEY_ONCE);
                self.do_key(k);
            }
        }
    }

    /// The batch mode's `pselect (0, NULL, NULL, NULL, &ts, NULL)`: the
    /// delay, cut short by a signal.
    fn wait_signals(&mut self, ms: i32) {
        if self.wake_read >= 0 {
            let _ = libcall::fd::wait_readable(self.wake_read, ms);
        } else {
            nap(u64::try_from(ms).unwrap_or(0).saturating_mul(1000));
        }
        self.check_signals();
    }
}

/// `SIGUSR1`, `SIGUSR2`, and the fault signals left at their default.
const SIGUSR1: i32 = 10;
const SIGUSR2: i32 = 12;
const SIGILL: i32 = 4;
const SIGBUS: i32 = 7;
const SIGFPE: i32 = 8;
const SIGSEGV: i32 = 11;
/// `O_NONBLOCK`.
const O_NONBLOCK: i32 = 0o4000;

/// `_exit (status)`: out at once, nothing flushed and no `atexit` handler.
fn exit_now(status: i32) -> ! {
    #[cfg(unix)]
    {
        libcall::process::exit_immediately(status)
    }
    #[cfg(not(unix))]
    {
        // No signal is ever caught off Unix, and `/proc` is never there.
        std::process::exit(status)
    }
}

/// `sigfillset` then `sigprocmask (SIG_BLOCK)`.
fn block_all_signals() {
    let mut all = libcall::signal::SigSet::empty();
    for s in 1..=libcall::sigrtmax() {
        // glibc refuses its own two; they stay as they are.
        let _ = all.add(s);
    }
    let _ = libcall::signal::block(&all);
}

/// `usleep (us)`: cut short by a signal, as upstream's is.
pub fn nap(us: u64) {
    let secs = us / 1_000_000;
    let nanos = u32::try_from((us % 1_000_000).wrapping_mul(1000)).unwrap_or(0);
    let _ = libcall::clock::nanosleep(secs, nanos);
}

/// `time (NULL)`.
fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// The delay as `pselect`'s timeout, in milliseconds: `tv_sec` the whole
/// seconds of the `float`, `tv_nsec` the rest.
// `(int)` of a non-negative `float` below `INT_MAX`: truncation, which `as`
// performs; the rcfile and `d` keep the delay in that range.
#[allow(clippy::cast_possible_truncation)]
fn delay_ms(delay: f32) -> i32 {
    let whole = delay as i64;
    let frac = (f64::from(delay) - whole as f64) * 1_000_000_000.0;
    let nanos = frac as i64;
    let ms = whole
        .saturating_mul(1000)
        .saturating_add(nanos.div_euclid(1_000_000));
    // A part millisecond still waits.
    let ms = if nanos % 1_000_000 != 0 {
        ms.saturating_add(1)
    } else {
        ms
    };
    i32::try_from(ms).unwrap_or(i32::MAX)
}

/// `(float) d`: a `double` rounded to the nearest `float`, as C converts.
// The point of the function: C's conversion is exactly this rounding.
#[allow(clippy::cast_possible_truncation)]
#[must_use]
pub fn c_float(d: f64) -> f32 {
    d as f32
}

/// `(float) n` of a `long`.
// C rounds a `long` to the nearest `float`; so does `as`.
#[allow(clippy::cast_precision_loss)]
#[must_use]
pub fn c_float_i(n: i64) -> f32 {
    n as f32
}

/// `LINUX_VERSION (x, y, z)`.
const fn linux_code(x: i32, y: i32, z: i32) -> i32 {
    x.wrapping_mul(0x10000)
        .wrapping_add(y.wrapping_mul(0x100))
        .wrapping_add(z)
}

/// `procps_linux_version`: the kernel release from
/// `/proc/sys/kernel/osrelease` as `LINUX_VERSION` numbers it, or a
/// negative number when it cannot be read or makes no sense.
fn linux_version() -> i32 {
    let Ok(text) = std::fs::read("/proc/sys/kernel/osrelease") else {
        return -2;
    };
    // `fgets (buf, 256, fp)`, then `sscanf (buf, "%d.%d.%d", &x, &y, &z)`.
    let line: Vec<u8> = text
        .iter()
        .take(255)
        .take_while(|&&b| b != b'\n')
        .copied()
        .collect();
    let mut scan = coreutils::procps::scanf::Scan::new(&line);
    let mut parts = [0i32; 3];
    let mut depth = 0i32;
    for (k, part) in parts.iter_mut().enumerate() {
        if k > 0 && scan.lit(b".").is_none() {
            break;
        }
        match scan.int() {
            Some(v) => {
                *part = v;
                depth = depth.wrapping_add(1);
            }
            None => break,
        }
    }
    let [x, y, z] = parts;
    if depth < 2 || (depth < 3 && x < 3) {
        return -34;
    }
    linux_code(x, y, z)
}

/// `strerror (errno)` of an I/O error.
#[must_use]
pub fn strerror_of(e: &std::io::Error) -> Vec<u8> {
    errmsg::strerror(e).into_bytes()
}

/// `strerror (e)`.
#[must_use]
pub fn strerror_no(e: i32) -> Vec<u8> {
    errmsg::strerror(&std::io::Error::from_raw_os_error(e)).into_bytes()
}

/// `errno` now, or `fallback` when nothing set it.
fn last_errno_or(fallback: i32) -> i32 {
    std::io::Error::last_os_error()
        .raw_os_error()
        .filter(|&e| e != 0)
        .unwrap_or(fallback)
}

/// An `error_exit` before the program state exists: `top: text` and a
/// failure.
fn early_exit(myname: &[u8], text: &[u8]) -> ! {
    let line = [myname, b": ", text, b"\n"].concat();
    let _ = libcall::fd::write(2, &line);
    std::process::exit(1);
}

fn main() {
    let args: Vec<Vec<u8>> = std::env::args_os()
        .map(std::ffi::OsString::into_encoded_bytes)
        .collect();
    let me = args.first().cloned().unwrap_or_else(|| b"top".to_vec());
    let mut top = Top::before(&me);
    top.wins_stage_1();
    top.configs_reads();
    top.parse_args(&args);
    top.signals_set();
    top.whack_terminal();
    top.wins_stage_2();
    top.run();
}
