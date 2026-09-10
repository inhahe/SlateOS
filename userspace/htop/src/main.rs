//! Slate OS Interactive Process Viewer
//!
//! A full-screen interactive process viewer inspired by htop.  Provides
//! per-core CPU bars, memory/swap bars, a sortable/filterable process list,
//! tree view, process kill, and nice adjustment -- all rendered with VT100
//! escape sequences and raw-mode character input.
//!
//! # Usage
//!
//! ```text
//! htop                   Interactive mode (default 1s refresh)
//! htop -d <secs>         Set refresh interval
//! htop -s <field>        Initial sort: cpu, mem, pid, user, cmd, time
//! htop -t                Start in tree view
//! htop -h, --help        Show help
//! ```

use std::env;
use std::fmt::Write as FmtWrite;
use std::fs;
use std::io::{self, Read, Write};
use std::process;

// ============================================================================
// Constants
// ============================================================================

const VERSION: &str = "0.1.0";

// `PAGE_SIZE_KB` used to live here, a private copy of a fact about the kernel.
// It is `procinfo::PAGE_SIZE_KIB` now, and this program no longer converts
// pages to KiB itself -- `ProcessStat::rss_kib` and `ProcessStatm::shared_kib`
// do. `userspace/ps` still has its own copy; see `known-issues.md` ->
// TD-B-TEN-PROC-PARSERS-IN-USERSPACE-AND-ONE-CRATE.

/// Assumed tick rate (ticks per second).
const TICKS_PER_SEC: u64 = 100;

// ANSI color codes.
const GREEN: &str = "\x1b[32m";
const RED: &str = "\x1b[31m";
const BLUE: &str = "\x1b[34m";
const CYAN: &str = "\x1b[36m";
const YELLOW: &str = "\x1b[33m";
const MAGENTA: &str = "\x1b[35m";
const WHITE: &str = "\x1b[37m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";
const REVERSE: &str = "\x1b[7m";
const BOLD_WHITE: &str = "\x1b[1;37m";
const BOLD_GREEN: &str = "\x1b[1;32m";
const BOLD_RED: &str = "\x1b[1;31m";
// BOLD_BLUE is available for future use (e.g. extended theme support).
#[allow(dead_code)]
const BOLD_BLUE: &str = "\x1b[1;34m";
const BOLD_CYAN: &str = "\x1b[1;36m";
const BOLD_YELLOW: &str = "\x1b[1;33m";

// Syscall numbers for process management.

/// Force-terminate a process.
///
/// arg0 = target PID, arg1 = exit code, arg2 = unused.
const SYS_PROCESS_KILL: u64 = 506;

// POSIX priority target type.
const PRIO_PROCESS: i32 = 0;

// ============================================================================
// Syscall interface
// ============================================================================

/// Issue a three-argument syscall using the x86-64 `syscall` instruction.
///
/// Register mapping follows the Slate OS syscall ABI:
///   rax = syscall number, rdi = arg1, rsi = arg2, rdx = arg3
///   Return value in rax. rcx and r11 are clobbered by the CPU.
#[cfg(target_vendor = "slateos")]
unsafe fn syscall3(nr: u64, a1: u64, a2: u64, a3: u64) -> i64 {
    let ret: i64;
    // SAFETY: Caller ensures arguments are valid for the given syscall number.
    // The `syscall` instruction is the defined kernel entry point on x86-64.
    // rcx and r11 are marked as clobbered per the hardware specification.
    unsafe {
        core::arch::asm!(
            "syscall",
            inlateout("rax") nr as i64 => ret,
            in("rdi") a1,
            in("rsi") a2,
            in("rdx") a3,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    ret
}

// Host stub — see the gated definition above. A development host has no
// SlateOS kernel to ask, so answer ENOSYS rather than entering the host's.
#[cfg(not(target_vendor = "slateos"))]
unsafe fn syscall3(_nr: u64, _a1: u64, _a2: u64, _a3: u64) -> i64 {
    -38 // ENOSYS
}

// ============================================================================
// POSIX FFI for setpriority (provided by the POSIX compatibility layer)
// ============================================================================

#[cfg(unix)]
unsafe extern "C" {
    fn setpriority(which: i32, who: u32, prio: i32) -> i32;
    fn getpriority(which: i32, who: u32) -> i32;
}

// Fallback for non-POSIX hosts, in the same spirit as `syscall3` above: this
// crate targets SlateOS and Linux, but the whole workspace is type-checked and
// tested on the Windows host, where these two symbols do not exist and the link
// fails. Declaring them unconditionally cost `cargo test --workspace` — nobody
// can run it while a crate refuses to link, and a gate nobody runs is a gate
// that has stopped checking.
//
// Both report failure rather than a plausible value. `renice` then says
// "Failed to renice", which is the truth on a host that has no `setpriority`;
// a stub returning 0 would have the host confirm a renice that did not happen.
#[cfg(not(unix))]
unsafe fn setpriority(_which: i32, _who: u32, _prio: i32) -> i32 {
    -1
}
#[cfg(not(unix))]
unsafe fn getpriority(_which: i32, _who: u32) -> i32 {
    -1
}

// ============================================================================
// Minimal libc bindings for termios (raw terminal mode)
// ============================================================================

#[cfg(unix)]
mod libc {
    //! Minimal libc bindings for termios -- just enough for raw mode.
    //! Our POSIX layer provides the real implementation.

    pub type TcflagT = u32;
    pub type CcT = u8;
    pub type SpeedT = u32;

    pub const NCCS: usize = 32;

    #[repr(C)]
    #[derive(Copy, Clone)]
    pub struct termios {
        pub c_iflag: TcflagT,
        pub c_oflag: TcflagT,
        pub c_cflag: TcflagT,
        pub c_lflag: TcflagT,
        pub c_line: CcT,
        pub c_cc: [CcT; NCCS],
        pub c_ispeed: SpeedT,
        pub c_ospeed: SpeedT,
    }

    // Input flags
    pub const BRKINT: TcflagT = 0o000002;
    pub const ICRNL: TcflagT = 0o000400;
    pub const INPCK: TcflagT = 0o000020;
    pub const ISTRIP: TcflagT = 0o000040;
    pub const IXON: TcflagT = 0o002000;

    // Output flags
    pub const OPOST: TcflagT = 0o000001;

    // Control flags
    pub const CS8: TcflagT = 0o000060;

    // Local flags
    pub const ECHO: TcflagT = 0o000010;
    pub const ICANON: TcflagT = 0o000002;
    pub const ISIG: TcflagT = 0o000001;
    pub const IEXTEN: TcflagT = 0o100000;

    // c_cc indices
    pub const VMIN: usize = 6;
    pub const VTIME: usize = 5;

    // tcsetattr actions
    pub const TCSAFLUSH: i32 = 2;

    unsafe extern "C" {
        pub fn tcgetattr(fd: i32, termios_p: *mut termios) -> i32;
        pub fn tcsetattr(fd: i32, action: i32, termios_p: *const termios) -> i32;
    }
}

// ============================================================================
// Terminal I/O
// ============================================================================

/// Read a single byte from stdin, returning `None` on EOF/error.
fn read_byte() -> Option<u8> {
    let mut buf = [0u8; 1];
    match io::stdin().lock().read(&mut buf) {
        Ok(1) => Some(buf[0]),
        _ => None,
    }
}

/// Read a byte eagerly (for escape sequence continuation).
fn read_byte_eager() -> Option<u8> {
    read_byte()
}

/// Flush stdout.
fn flush() {
    let _ = io::stdout().flush();
}

/// Write a string to stdout.
fn write_str(s: &str) {
    let _ = io::stdout().write_all(s.as_bytes());
}

/// Write formatted args to stdout.
#[allow(dead_code)] // Available for non-FmtWrite rendering paths.
fn write_fmt(args: std::fmt::Arguments<'_>) {
    let _ = io::stdout().write_fmt(args);
}

// ============================================================================
// VT100 escape helpers
// ============================================================================

#[allow(dead_code)] // Available for overlay/prompt positioning.
fn cursor_to(row: usize, col: usize) {
    write_fmt(format_args!("\x1b[{};{}H", row + 1, col + 1));
}

fn clear_screen() {
    write_str("\x1b[2J");
}

#[allow(dead_code)] // Available for partial-line clearing.
fn clear_to_eol() {
    write_str("\x1b[K");
}

fn hide_cursor() {
    write_str("\x1b[?25l");
}

fn show_cursor() {
    write_str("\x1b[?25h");
}

fn enter_alternate_screen() {
    write_str("\x1b[?1049h");
}

fn leave_alternate_screen() {
    write_str("\x1b[?1049l");
}

// ============================================================================
// Raw terminal mode
// ============================================================================

/// Enable raw mode via the POSIX termios interface.
fn enable_raw_mode() -> Option<RawModeGuard> {
    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        let fd = io::stdin().as_raw_fd();
        let mut orig: libc::termios = unsafe { std::mem::zeroed() };
        // SAFETY: fd 0 is stdin; termios is plain-old-data safe to zero-init
        // and pass to the kernel via tcgetattr/tcsetattr.
        if unsafe { libc::tcgetattr(fd, &mut orig) } != 0 {
            return None;
        }
        let mut raw = orig;
        raw.c_iflag &= !(libc::BRKINT | libc::ICRNL | libc::INPCK | libc::ISTRIP | libc::IXON);
        raw.c_oflag &= !libc::OPOST;
        raw.c_cflag |= libc::CS8;
        raw.c_lflag &= !(libc::ECHO | libc::ICANON | libc::ISIG | libc::IEXTEN);
        // Non-blocking: return after VTIME tenths of a second even if no byte.
        // This allows the refresh loop to proceed without blocking forever.
        raw.c_cc[libc::VMIN] = 0;
        raw.c_cc[libc::VTIME] = 1; // 0.1 second timeout
        // SAFETY: Same plain-integer arguments as tcgetattr.
        if unsafe { libc::tcsetattr(fd, libc::TCSAFLUSH, &raw) } != 0 {
            return None;
        }
        Some(RawModeGuard { orig })
    }
    #[cfg(not(unix))]
    {
        None
    }
}

/// RAII guard that restores the original terminal mode on drop.
struct RawModeGuard {
    #[cfg(unix)]
    orig: libc::termios,
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            use std::os::unix::io::AsRawFd;
            let fd = io::stdin().as_raw_fd();
            // SAFETY: Restoring the saved termios to its original state.
            let _ = unsafe { libc::tcsetattr(fd, libc::TCSAFLUSH, &self.orig) };
        }
    }
}

// ============================================================================
// Terminal size detection
// ============================================================================

/// Query terminal size. Falls back to 80x24 if unavailable.
fn terminal_size() -> (usize, usize) {
    // Try COLUMNS/LINES env vars first.
    let cols = env::var("COLUMNS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(0);
    let rows = env::var("LINES")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(0);
    if cols > 0 && rows > 0 {
        return (rows, cols);
    }

    // Try ioctl TIOCGWINSZ.
    #[cfg(unix)]
    {
        #[repr(C)]
        struct Winsize {
            ws_row: u16,
            ws_col: u16,
            ws_xpixel: u16,
            ws_ypixel: u16,
        }
        use std::os::unix::io::AsRawFd;
        let fd = io::stdout().as_raw_fd();
        let mut ws: Winsize = unsafe { std::mem::zeroed() };
        // SAFETY: TIOCGWINSZ (0x5413) queries terminal dimensions into ws.
        let ret = unsafe {
            unsafe extern "C" {
                fn ioctl(fd: i32, request: u64, ...) -> i32;
            }
            ioctl(fd, 0x5413, &mut ws as *mut Winsize)
        };
        if ret == 0 && ws.ws_row > 0 && ws.ws_col > 0 {
            return (ws.ws_row as usize, ws.ws_col as usize);
        }
    }

    // Fallback: probe by moving cursor to bottom-right and querying position.
    // This handshake blocks reading the terminal's reply from stdin, so it must
    // only run on a real interactive terminal — under a pipe/redirect/test the
    // reply never arrives and the read would hang forever.
    use std::io::IsTerminal;
    if !(io::stdin().is_terminal() && io::stdout().is_terminal()) {
        return (24, 80);
    }
    write_str("\x1b[999;999H\x1b[6n");
    flush();
    let mut resp = Vec::new();
    loop {
        match read_byte() {
            Some(b'R') => break,
            Some(c) => resp.push(c),
            None => break,
        }
        if resp.len() > 32 {
            break;
        }
    }
    let s = String::from_utf8_lossy(&resp);
    if let Some(coords) = s.strip_prefix("\x1b[")
        && let Some((r, c)) = coords.split_once(';')
    {
        let rows_parsed = r.parse::<usize>().unwrap_or(24);
        let cols_parsed = c.parse::<usize>().unwrap_or(80);
        return (rows_parsed, cols_parsed);
    }

    (24, 80)
}

// ============================================================================
// Key representation
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Key {
    Char(char),
    Ctrl(char),
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Delete,
    Backspace,
    Enter,
    Tab,
    Escape,
    F(u8),
    /// No key available (timeout).
    None,
    /// Unknown or unhandled escape sequence.
    Unknown,
}

/// Read one keypress, decoding VT100 escape sequences.
/// Returns Key::None on timeout (non-blocking).
fn read_key() -> Key {
    let Some(b) = read_byte() else {
        return Key::None;
    };
    match b {
        0 => Key::Ctrl('@'),
        1..=6 => Key::Ctrl((b'a' + b - 1) as char),
        7 => Key::Ctrl('g'),
        8 => Key::Backspace,
        9 => Key::Tab,
        10 => Key::Enter,
        11 => Key::Ctrl('k'),
        12 => Key::Ctrl('l'),
        13 => Key::Enter,
        14..=26 => Key::Ctrl((b'a' + b - 1) as char),
        27 => {
            let Some(b2) = read_byte_eager() else {
                return Key::Escape;
            };
            match b2 {
                b'[' => {
                    let Some(b3) = read_byte_eager() else {
                        return Key::Escape;
                    };
                    match b3 {
                        b'A' => Key::Up,
                        b'B' => Key::Down,
                        b'C' => Key::Right,
                        b'D' => Key::Left,
                        b'H' => Key::Home,
                        b'F' => Key::End,
                        b'0'..=b'9' => {
                            // Collect all digits.
                            let mut num1 = (b3 - b'0') as u16;
                            let mut next = read_byte_eager();
                            while let Some(d) = next {
                                if d.is_ascii_digit() {
                                    num1 =
                                        num1.saturating_mul(10).saturating_add((d - b'0') as u16);
                                    next = read_byte_eager();
                                } else {
                                    break;
                                }
                            }
                            match next {
                                Some(b'~') => match num1 {
                                    1 => Key::Home,
                                    3 => Key::Delete,
                                    4 => Key::End,
                                    5 => Key::PageUp,
                                    6 => Key::PageDown,
                                    7 => Key::Home,
                                    8 => Key::End,
                                    11 => Key::F(1),
                                    12 => Key::F(2),
                                    13 => Key::F(3),
                                    14 => Key::F(4),
                                    15 => Key::F(5),
                                    17 => Key::F(6),
                                    18 => Key::F(7),
                                    19 => Key::F(8),
                                    20 => Key::F(9),
                                    21 => Key::F(10),
                                    _ => Key::Unknown,
                                },
                                Some(b';') => {
                                    // Modifier sequence like \x1b[1;5A -- consume the rest.
                                    let _ = read_byte_eager(); // modifier digit
                                    let trail = read_byte_eager();
                                    match trail {
                                        Some(b'A') => Key::Up,
                                        Some(b'B') => Key::Down,
                                        Some(b'C') => Key::Right,
                                        Some(b'D') => Key::Left,
                                        _ => Key::Unknown,
                                    }
                                }
                                _ => Key::Unknown,
                            }
                        }
                        _ => Key::Unknown,
                    }
                }
                b'O' => {
                    let Some(b3) = read_byte_eager() else {
                        return Key::Escape;
                    };
                    match b3 {
                        b'H' => Key::Home,
                        b'F' => Key::End,
                        b'P' => Key::F(1),
                        b'Q' => Key::F(2),
                        b'R' => Key::F(3),
                        b'S' => Key::F(4),
                        _ => Key::Unknown,
                    }
                }
                _ => Key::Unknown,
            }
        }
        28 => Key::Ctrl('\\'),
        29 => Key::Ctrl(']'),
        30 => Key::Ctrl('^'),
        31 => Key::Ctrl('_'),
        127 => Key::Backspace,
        32..=126 => Key::Char(b as char),
        _ => Key::Unknown,
    }
}

// ============================================================================
// Data structures
// ============================================================================

// `CpuStat` used to be declared here, with seven of the ten fields
// `/proc/stat` publishes and a `total()` that omitted `steal`. It is
// `procinfo::CpuTimes` now -- see that type for why the missing fields
// mattered under QEMU.

/// Per-process information scraped from /proc/<pid>/.
#[derive(Clone)]
struct ProcessInfo {
    pid: u32,
    name: String,
    cmdline: String,
    state: char,
    ppid: u32,
    #[allow(dead_code)] // Retained for filtering/sorting by numeric UID.
    uid: u32,
    user: String,
    priority: i32,
    nice: i32,
    threads: u32,
    /// Virtual memory size in KiB.
    vsize_kb: u64,
    /// Resident set size in KiB.
    rss_kb: u64,
    /// Shared memory in KiB.
    shr_kb: u64,
    /// CPU time in ticks (utime + stime).
    cpu_ticks: u64,
    /// CPU usage percentage (computed between snapshots).
    cpu_pct: f64,
    /// Memory usage percentage.
    mem_pct: f64,
    /// Total CPU time as formatted string.
    time_str: String,
}

// `MemInfo` used to be declared here, with `u64` fields that were zero
// both when the kernel did not export a figure and when the figure was
// genuinely zero. It is `procinfo::MemInfo` now, whose fields are
// `Option<u64>` for that reason; this program unwraps to 0 at each use,
// which is what it always did -- but the unwrap is now visible.

#[derive(Clone, Copy, PartialEq, Eq)]
enum SortField {
    Pid,
    User,
    Priority,
    Nice,
    Virt,
    Res,
    Shr,
    State,
    Cpu,
    Mem,
    Time,
    Command,
}

impl SortField {
    fn label(self) -> &'static str {
        match self {
            Self::Pid => "PID",
            Self::User => "USER",
            Self::Priority => "PRI",
            Self::Nice => "NI",
            Self::Virt => "VIRT",
            Self::Res => "RES",
            Self::Shr => "SHR",
            Self::State => "S",
            Self::Cpu => "%CPU",
            Self::Mem => "%MEM",
            Self::Time => "TIME+",
            Self::Command => "COMMAND",
        }
    }

    fn all() -> &'static [SortField] {
        &[
            Self::Pid,
            Self::User,
            Self::Priority,
            Self::Nice,
            Self::Virt,
            Self::Res,
            Self::Shr,
            Self::State,
            Self::Cpu,
            Self::Mem,
            Self::Time,
            Self::Command,
        ]
    }
}

/// Which overlay mode the UI is in.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Overlay {
    /// Normal process view.
    Normal,
    /// Help screen (F1).
    Help,
    /// Sort field selection (F6).
    SortPicker,
    /// Kill signal selection (F9).
    KillConfirm,
    /// Search/filter input (F3 or /).
    Search,
}

struct Config {
    delay_secs: u64,
    sort_field: SortField,
    tree_view: bool,
}

// ============================================================================
// System readers -- thin wrappers over `procinfo`, plus /etc/passwd
// ============================================================================

/// Read a small text file, trimmed.
///
/// The only caller left is [`uid_to_user`], which reads `/etc/passwd`.
/// Everything under `/proc` goes through [`procinfo`] now, which reads
/// bytes rather than a `String` -- the distinction that stopped this
/// program silently dropping processes whose names are not UTF-8.
fn read_file(path: &str) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

/// What one CPU's bar shows: the four drawn fractions of the interval.
///
/// Fractions of the *interval*, so they sum to at most one, and together they
/// are the busy fraction -- [`procinfo::CpuTimes::busy`] is
/// `total - idle - iowait`, which is exactly these four.
///
/// # The fourth one, and why it was missing
///
/// `overhead` is `irq + softirq + steal`. Until 2026-09-10 the bar drew three
/// segments and the percentage beside it summed those three, so **a CPU doing
/// nothing but servicing interrupts read 0% busy**. Under QEMU `steal` is the
/// field that is reliably non-zero and a network- or disk-heavy workload puts
/// real time in `softirq`; both are the machine being unavailable to the user,
/// and both read as idle.
///
/// It stayed that way deliberately for a while: the percentage is the sum of
/// what is *drawn*, and a number that disagrees with the picture beside it is
/// worse than one that under-reports consistently. So the number could not
/// grow a fourth category until the bar grew a fourth segment. This is that
/// change -- both halves at once, which is the only way it is honest.
///
/// # Why the three are one segment rather than three
///
/// Real `htop` gives `irq`, `softirq` and `steal` separate colours. They are
/// grouped here because they answer one question -- "how much of this
/// interval was the CPU busy with something that is not a process?" -- and
/// because three more segments on a bar that is often twenty characters wide
/// is three more roundings to zero. If a use appears for telling them apart,
/// the split is a change to this struct and to `render_cpu_bar`, not to what
/// counts as busy.
#[derive(Clone, Copy, Debug, PartialEq)]
struct CpuBar {
    user: f64,
    system: f64,
    nice: f64,
    /// `irq + softirq + steal`: busy time not attributable to a process.
    overhead: f64,
}

impl CpuBar {
    /// The number printed beside the bar: the sum of what is drawn.
    ///
    /// Still the sum of the drawn segments and not a second, independent
    /// definition of "busy" -- that is what keeps the number and the picture
    /// agreeing. It now *equals* [`procinfo::CpuTimes::busy`] over the same
    /// interval, and `percent_matches_the_crates_definition_of_busy` asserts
    /// it does. The two are computed separately on purpose: if either
    /// definition drifts, that test says so instead of one silently absorbing
    /// the difference.
    fn percent(self) -> f64 {
        (self.user + self.system + self.nice + self.overhead) * 100.0
    }
}

/// The fractions for one refresh interval, or `None` when there is no interval.
///
/// A zero-length interval is "no data yet", not "idle": it is what the first
/// refresh sees, and what a CPU that has just come back online sees for one
/// frame. Drawing an empty bar for it is right; dividing by it is not, which is
/// what the previous `total().max(1)` did -- turning a missing measurement into
/// a confident 0.0%.
fn cpu_bar_fractions(delta: &procinfo::CpuTimes) -> Option<CpuBar> {
    let total = delta.total();
    if total == 0 {
        return None;
    }
    let total = total as f64;
    Some(CpuBar {
        user: delta.user as f64 / total,
        system: delta.system as f64 / total,
        nice: delta.nice as f64 / total,
        // Summed as integers before the division: three separate divisions
        // would each round, and the bar's own rounding is already the reason
        // small categories vanish.
        overhead: delta
            .irq
            .saturating_add(delta.softirq)
            .saturating_add(delta.steal) as f64
            / total,
    })
}

/// Memory and swap, through [`procinfo`].
fn read_meminfo() -> procinfo::MemInfo {
    procinfo::ProcFs::new()
        .memory()
        .ok()
        .flatten()
        .unwrap_or_default()
}

/// Uptime in whole seconds, through [`procinfo`].
fn read_uptime() -> u64 {
    procinfo::ProcFs::new()
        .uptime()
        .ok()
        .flatten()
        .map_or(0, |u| u.up.as_secs())
}

/// The three load averages, formatted for the header.
///
/// `procinfo` parses these as `f64`; the two decimal places are this program's
/// choice about how to show them, which is why the formatting is here and the
/// parsing is not.
fn read_loadavg() -> (String, String, String) {
    procinfo::ProcFs::new()
        .load_average()
        .ok()
        .flatten()
        .map_or_else(
            || ("0.00".to_string(), "0.00".to_string(), "0.00".to_string()),
            |l| {
                (
                    format!("{:.2}", l.one),
                    format!("{:.2}", l.five),
                    format!("{:.2}", l.fifteen),
                )
            },
        )
}

/// Per-CPU times, through [`procinfo`].
///
/// One read of `/proc/stat`, not two: the reader this replaces opened the file
/// a second time when it found no `cpuN` lines, so on a single-CPU machine it
/// compared two different instants.
fn read_cpu_times() -> Vec<procinfo::CpuTimes> {
    procinfo::ProcFs::new()
        .cpu_stats()
        .ok()
        .flatten()
        .map(|s| s.per_cpu_or_total())
        .unwrap_or_default()
}

/// Resolve a UID to a username via /etc/passwd.  Falls back to the numeric UID.
fn uid_to_user(uid: u32) -> String {
    // Cache is not feasible in a single-pass design without statics, so we
    // just do a quick scan each time.  With few processes this is adequate.
    if let Some(passwd) = read_file("/etc/passwd") {
        for line in passwd.lines() {
            let fields: Vec<&str> = line.split(':').collect();
            if fields.len() >= 3
                && let Ok(entry_uid) = fields[2].parse::<u32>()
                && entry_uid == uid
            {
                return fields[0].to_string();
            }
        }
    }
    uid.to_string()
}

/// Read information about a single process, through [`procinfo`].
///
/// The parsing this used to do itself now lives in `procinfo`, so that
/// `apps/procexplorer` gets the same answers -- including the two things this
/// function got wrong on its own: a `comm` containing spaces and parentheses
/// (handled by finding the last `)`, which this did correctly) and a name that
/// is not UTF-8 (which it did not, dropping the process).
fn read_process(procfs: &procinfo::ProcFs, pid: u32, mem_total_kb: u64) -> Option<ProcessInfo> {
    let stat = procfs.process_stat(u64::from(pid)).ok()??;

    let name = procinfo::display_bytes(&stat.comm);
    let rss_kb = stat.rss_kib();
    let cpu_ticks = stat.cpu_ticks();

    let mem_pct = if mem_total_kb > 0 {
        (rss_kb as f64 / mem_total_kb as f64) * 100.0
    } else {
        0.0
    };

    // Format CPU time as H:MM:SS.cc (hundredths).
    let total_secs = cpu_ticks / TICKS_PER_SEC;
    let centis = cpu_ticks % TICKS_PER_SEC;
    let hours = total_secs / 3600;
    let mins = (total_secs % 3600) / 60;
    let secs = total_secs % 60;
    let time_str = if hours > 0 {
        format!("{hours}:{mins:02}:{secs:02}.{centis:02}")
    } else {
        format!("{mins}:{secs:02}.{centis:02}")
    };

    let uid = procfs
        .process_status(u64::from(pid))
        .ok()
        .flatten()
        .and_then(|st| st.uid)
        .unwrap_or(0);

    let shr_kb = procfs
        .process_statm(u64::from(pid))
        .ok()
        .flatten()
        .map_or(0, |m| m.shared_kib());

    // An empty argument list is a kernel thread, which is why it shows as
    // `[name]` rather than blank. `procinfo` distinguishes that from a process
    // that has exited, which arrives here as `None` and gets the same display
    // -- the difference does not matter to a viewer, only to the reader.
    let cmdline = procfs
        .process_cmdline(u64::from(pid))
        .ok()
        .flatten()
        .filter(|args| !args.is_empty())
        .map_or_else(
            || format!("[{name}]"),
            |args| {
                args.iter()
                    .map(|a| procinfo::display_bytes(a))
                    .collect::<Vec<_>>()
                    .join(" ")
            },
        );

    let user = uid_to_user(uid);

    Some(ProcessInfo {
        pid,
        name,
        cmdline,
        state: char::from(stat.state),
        ppid: u32::try_from(stat.ppid).unwrap_or(0),
        uid,
        user,
        priority: i32::try_from(stat.priority).unwrap_or(0),
        nice: i32::try_from(stat.nice).unwrap_or(0),
        threads: u32::try_from(stat.num_threads).unwrap_or(1),
        vsize_kb: stat.vsize_kib(),
        rss_kb,
        shr_kb,
        cpu_ticks,
        cpu_pct: 0.0,
        mem_pct,
        time_str,
    })
}

/// Enumerate all processes from /proc.
fn read_all_processes(mem_total_kb: u64) -> Vec<ProcessInfo> {
    let procfs = procinfo::ProcFs::new();
    let Ok(pids) = procfs.process_ids() else {
        return Vec::new();
    };
    pids.into_iter()
        .filter_map(|pid| u32::try_from(pid).ok())
        .filter_map(|pid| read_process(&procfs, pid, mem_total_kb))
        .collect()
}

/// Compute CPU usage percentages by comparing two snapshots.
fn compute_cpu_usage(current: &mut [ProcessInfo], prev: &[(u32, u64)], total_cpu_delta: u64) {
    for proc_info in current.iter_mut() {
        let prev_ticks = prev
            .iter()
            .find(|(pid, _)| *pid == proc_info.pid)
            .map(|(_, t)| *t)
            .unwrap_or(0);

        let delta = proc_info.cpu_ticks.saturating_sub(prev_ticks);

        proc_info.cpu_pct = if total_cpu_delta > 0 {
            (delta as f64 / total_cpu_delta as f64) * 100.0
        } else {
            0.0
        };
    }
}

// ============================================================================
// Formatting helpers
// ============================================================================

/// Format KiB value as human-readable (K/M/G).
fn format_kb(kb: u64) -> String {
    if kb >= 1_048_576 {
        format!("{:.1}G", kb as f64 / 1_048_576.0)
    } else if kb >= 1024 {
        format!("{:.0}M", kb as f64 / 1024.0)
    } else {
        format!("{kb}K")
    }
}

/// Format KiB as MiB string for the header bars.
fn format_mib(kb: u64) -> String {
    if kb >= 1_048_576 {
        format!("{:.1}G", kb as f64 / 1_048_576.0)
    } else {
        format!("{}M", kb / 1024)
    }
}

/// Format uptime as "X days, HH:MM:SS".
fn format_uptime(secs: u64) -> String {
    let days = secs / 86400;
    let hours = (secs % 86400) / 3600;
    let mins = (secs % 3600) / 60;
    let s = secs % 60;
    if days > 0 {
        format!(
            "{days} day{}, {hours:02}:{mins:02}:{s:02}",
            if days == 1 { "" } else { "s" }
        )
    } else {
        format!("{hours:02}:{mins:02}:{s:02}")
    }
}

/// State character color.
fn state_color(state: char) -> &'static str {
    match state {
        'R' => GREEN,
        'S' | 'I' => CYAN,
        'Z' => RED,
        'T' | 't' => YELLOW,
        'D' => BOLD_RED,
        _ => WHITE,
    }
}

// ============================================================================
// Sorting
// ============================================================================

fn sort_processes(procs: &mut [ProcessInfo], field: SortField) {
    procs.sort_by(|a, b| {
        let cmp = match field {
            SortField::Pid => a.pid.cmp(&b.pid),
            SortField::User => a.user.to_lowercase().cmp(&b.user.to_lowercase()),
            SortField::Priority => a.priority.cmp(&b.priority),
            SortField::Nice => a.nice.cmp(&b.nice),
            SortField::Virt => a.vsize_kb.cmp(&b.vsize_kb),
            SortField::Res => a.rss_kb.cmp(&b.rss_kb),
            SortField::Shr => a.shr_kb.cmp(&b.shr_kb),
            SortField::State => a.state.cmp(&b.state),
            SortField::Cpu => a
                .cpu_pct
                .partial_cmp(&b.cpu_pct)
                .unwrap_or(std::cmp::Ordering::Equal),
            SortField::Mem => a
                .mem_pct
                .partial_cmp(&b.mem_pct)
                .unwrap_or(std::cmp::Ordering::Equal),
            SortField::Time => a.cpu_ticks.cmp(&b.cpu_ticks),
            SortField::Command => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        };

        // Descending for numeric fields, ascending for name/user.
        match field {
            SortField::User | SortField::Command | SortField::Pid | SortField::State => cmp,
            _ => cmp.reverse(),
        }
    });
}

// ============================================================================
// Tree view
// ============================================================================

/// Build a tree-ordered list from a flat process list, adding indent prefixes.
fn build_tree(procs: &[ProcessInfo]) -> Vec<(ProcessInfo, usize)> {
    // Find all root processes (ppid == 0 or ppid not in the list).
    let pids: Vec<u32> = procs.iter().map(|p| p.pid).collect();

    let mut result = Vec::with_capacity(procs.len());

    // Collect roots.
    let mut roots: Vec<&ProcessInfo> = procs
        .iter()
        .filter(|p| p.ppid == 0 || !pids.contains(&p.ppid))
        .collect();
    roots.sort_by_key(|p| p.pid);

    for root in &roots {
        add_children(&mut result, procs, root, 0);
    }

    // Add any processes we missed (shouldn't happen, but safety net).
    let in_result: Vec<u32> = result.iter().map(|(p, _)| p.pid).collect();
    for p in procs {
        if !in_result.contains(&p.pid) {
            result.push((p.clone(), 0));
        }
    }

    result
}

fn add_children(
    result: &mut Vec<(ProcessInfo, usize)>,
    all: &[ProcessInfo],
    parent: &ProcessInfo,
    depth: usize,
) {
    result.push((parent.clone(), depth));

    let mut children: Vec<&ProcessInfo> = all
        .iter()
        .filter(|p| p.ppid == parent.pid && p.pid != parent.pid)
        .collect();
    children.sort_by_key(|p| p.pid);

    for child in children {
        add_children(result, all, child, depth + 1);
    }
}

// ============================================================================
// Application state
// ============================================================================

struct App {
    /// Terminal rows.
    term_rows: usize,
    /// Terminal columns.
    term_cols: usize,
    /// Configuration.
    config: Config,
    /// Current overlay mode.
    overlay: Overlay,
    /// Current list of processes.
    processes: Vec<ProcessInfo>,
    /// Previous snapshot for CPU delta computation.
    prev_ticks: Vec<(u32, u64)>,
    /// Previous aggregate CPU total.
    prev_cpu_total: u64,
    /// Per-CPU stats (previous snapshot).
    prev_cpu_stats: Vec<procinfo::CpuTimes>,
    /// Memory info.
    mem: procinfo::MemInfo,
    /// Cursor position in the process list (0-based).
    cursor: usize,
    /// Scroll offset (first visible process index).
    scroll: usize,
    /// Search/filter string.
    filter: String,
    /// Status message (shown briefly).
    status_msg: String,
    /// Sort field selector cursor (for F6 overlay).
    sort_cursor: usize,
    /// Whether to run.
    running: bool,
    /// Uptime seconds.
    uptime: u64,
    /// Load averages.
    load: (String, String, String),
    /// Number of CPUs.
    num_cpus: usize,
    /// Per-CPU time accumulated **since the previous refresh**.
    ///
    /// Not the counters themselves. `/proc/stat` counts since boot, so a bar
    /// drawn from the raw sample shows how the machine has spent its life and
    /// barely moves after a few hours of uptime. This holds the difference
    /// between the last two samples, which is what the bars are supposed to
    /// mean. Empty until the second refresh, and the renderer treats a
    /// zero-length interval as "no data yet" rather than as an idle CPU.
    cpu_delta: Vec<procinfo::CpuTimes>,
}

impl App {
    fn new(config: Config) -> Self {
        let (rows, cols) = terminal_size();
        let cpu_stats = read_cpu_times();
        let num_cpus = cpu_stats.len().max(1);
        Self {
            term_rows: rows,
            term_cols: cols,
            config,
            overlay: Overlay::Normal,
            processes: Vec::new(),
            prev_ticks: Vec::new(),
            prev_cpu_total: 0,
            prev_cpu_stats: cpu_stats,
            mem: procinfo::MemInfo::default(),
            cursor: 0,
            scroll: 0,
            filter: String::new(),
            status_msg: String::new(),
            sort_cursor: 0,
            running: true,
            uptime: 0,
            load: ("0.00".to_string(), "0.00".to_string(), "0.00".to_string()),
            num_cpus,
            cpu_delta: Vec::new(),
        }
    }

    /// Number of rows available for the process list.
    fn list_rows(&self) -> usize {
        // Header area: CPU bars (num_cpus lines) + mem bar + swap bar + blank + table header = num_cpus + 4
        // Footer: function key bar = 1
        let header_lines = self.num_cpus + 4;
        let footer_lines = 1;
        self.term_rows.saturating_sub(header_lines + footer_lines)
    }

    /// Number of header lines above the process table.
    fn header_height(&self) -> usize {
        self.num_cpus + 4
    }

    // ========================================================================
    // Data refresh
    // ========================================================================

    fn refresh_data(&mut self) {
        self.mem = read_meminfo();
        self.uptime = read_uptime();
        self.load = read_loadavg();

        let cpu_stats = read_cpu_times();
        self.num_cpus = cpu_stats.len().max(1);

        let mut procs = read_all_processes(self.mem.total_kib.unwrap_or(0));

        // Compute aggregate CPU delta for per-process CPU%.
        let current_total: u64 = cpu_stats.iter().map(|c| c.total()).sum();
        let prev_total: u64 = self.prev_cpu_stats.iter().map(|c| c.total()).sum();
        let cpu_delta = current_total.saturating_sub(prev_total);

        compute_cpu_usage(&mut procs, &self.prev_ticks, cpu_delta);

        // The per-CPU interval, for the bars. Same two samples the aggregate
        // above uses; `zip` drops any CPU that appeared or disappeared between
        // them, which is the honest answer for one refresh.
        self.cpu_delta = cpu_stats
            .iter()
            .zip(self.prev_cpu_stats.iter())
            .map(|(now, before)| now.since(before))
            .collect();

        // Save for next delta.
        self.prev_ticks = procs.iter().map(|p| (p.pid, p.cpu_ticks)).collect();
        self.prev_cpu_stats = cpu_stats;
        self.prev_cpu_total = current_total;

        self.processes = procs;
    }

    /// Return the filtered + sorted process list for display.
    fn display_processes(&self) -> Vec<(ProcessInfo, usize)> {
        let mut filtered: Vec<ProcessInfo> = if self.filter.is_empty() {
            self.processes.clone()
        } else {
            let needle = self.filter.to_lowercase();
            self.processes
                .iter()
                .filter(|p| {
                    p.name.to_lowercase().contains(&needle)
                        || p.cmdline.to_lowercase().contains(&needle)
                        || p.pid.to_string().contains(&needle)
                        || p.user.to_lowercase().contains(&needle)
                })
                .cloned()
                .collect()
        };

        sort_processes(&mut filtered, self.config.sort_field);

        if self.config.tree_view {
            build_tree(&filtered)
        } else {
            filtered.into_iter().map(|p| (p, 0)).collect()
        }
    }

    /// Ensure cursor and scroll are within bounds.
    fn clamp_cursor(&mut self, list_len: usize) {
        if list_len == 0 {
            self.cursor = 0;
            self.scroll = 0;
            return;
        }
        if self.cursor >= list_len {
            self.cursor = list_len.saturating_sub(1);
        }
        let visible = self.list_rows();
        if self.cursor < self.scroll {
            self.scroll = self.cursor;
        }
        if self.cursor >= self.scroll + visible {
            self.scroll = self.cursor.saturating_sub(visible.saturating_sub(1));
        }
    }

    // ========================================================================
    // Rendering
    // ========================================================================

    fn render(&mut self) {
        let display = self.display_processes();
        self.clamp_cursor(display.len());

        let mut buf = String::with_capacity(self.term_rows * self.term_cols * 3);

        hide_cursor();

        // Move to top-left.
        let _ = write!(buf, "\x1b[H");

        match self.overlay {
            Overlay::Normal | Overlay::Search => {
                self.render_header(&mut buf);
                self.render_table_header(&mut buf);
                self.render_process_list(&mut buf, &display);
                self.render_footer(&mut buf);
            }
            Overlay::Help => {
                self.render_help(&mut buf);
            }
            Overlay::SortPicker => {
                self.render_header(&mut buf);
                self.render_sort_picker(&mut buf);
            }
            Overlay::KillConfirm => {
                self.render_header(&mut buf);
                self.render_table_header(&mut buf);
                self.render_process_list(&mut buf, &display);
                self.render_kill_prompt(&mut buf);
            }
        }

        write_str(&buf);
        flush();
    }

    fn render_header(&self, buf: &mut String) {
        // The interval, not the counters -- see `App::cpu_delta`.
        let cpu_stats = &self.cpu_delta;
        let half_cols = self.term_cols / 2;

        // Render CPU bars and system info in a two-column layout.
        // Left column: CPU bars.  Right column: Mem, Swap, tasks, load, uptime.
        let right_lines = self.build_right_header();

        for i in 0..self.num_cpus {
            // CPU bar on the left.
            let _ = write!(buf, "\x1b[{};1H\x1b[K", i + 1);

            let cpu_label = format!("{:>3}", i);
            let _ = write!(buf, " {BOLD_CYAN}{cpu_label}{RESET}");

            if let Some(bar) = cpu_stats.get(i).and_then(cpu_bar_fractions) {
                // Bar width: half_cols minus label and brackets.
                let bar_width = half_cols.saturating_sub(8);
                Self::render_cpu_bar(buf, bar_width, &bar);
                let pct = bar.percent();
                let _ = write!(buf, " {BOLD_WHITE}{pct:4.1}%{RESET}");
            }

            // Right column content on the same line.
            if i < right_lines.len() {
                let right_col_start = half_cols + 1;
                let _ = write!(buf, "\x1b[{};{}H", i + 1, right_col_start);
                let _ = write!(buf, "{}", right_lines[i]);
            }
        }

        // If there are more right-column lines than CPU lines, print them.
        for (offset, line) in right_lines.iter().enumerate().skip(self.num_cpus) {
            let right_col_start = half_cols + 1;
            let _ = write!(buf, "\x1b[{};1H\x1b[K", offset + 1);
            let _ = write!(buf, "\x1b[{};{}H", offset + 1, right_col_start);
            let _ = write!(buf, "{line}");
        }

        // Fill remaining header lines if any.
        let header_h = self.header_height();
        let used_lines = self.num_cpus.max(right_lines.len());
        for i in used_lines..header_h.saturating_sub(1) {
            let _ = write!(buf, "\x1b[{};1H\x1b[K", i + 1);
        }
    }

    fn build_right_header(&self) -> Vec<String> {
        let mut lines = Vec::new();
        let half_cols = self.term_cols / 2;

        // Line 0: Memory bar.
        {
            let mut s = String::new();
            let _ = write!(s, "{BOLD_CYAN}Mem{RESET}");
            let bar_width = half_cols.saturating_sub(20);
            let total = self.mem.total_kib.unwrap_or(0).max(1) as f64;
            let used_frac = self.mem.used_excluding_cache_kib().unwrap_or(0) as f64 / total;
            let buf_frac = self.mem.buffers_kib.unwrap_or(0) as f64 / total;
            let cache_frac = self.mem.cached_kib.unwrap_or(0) as f64 / total;
            self.render_mem_bar_into(&mut s, bar_width, used_frac, buf_frac, cache_frac);
            let _ = write!(
                s,
                " {}/{}",
                format_mib(self.mem.used_excluding_cache_kib().unwrap_or(0)),
                format_mib(self.mem.total_kib.unwrap_or(0))
            );
            lines.push(s);
        }

        // Line 1: Swap bar.
        {
            let mut s = String::new();
            let _ = write!(s, "{BOLD_CYAN}Swp{RESET}");
            let bar_width = half_cols.saturating_sub(20);
            let total = self.mem.swap_total_kib.unwrap_or(0).max(1) as f64;
            let used_frac = self.mem.swap_used_kib().unwrap_or(0) as f64 / total;
            self.render_swap_bar_into(&mut s, bar_width, used_frac);
            let _ = write!(
                s,
                " {}/{}",
                format_mib(self.mem.swap_used_kib().unwrap_or(0)),
                format_mib(self.mem.swap_total_kib.unwrap_or(0))
            );
            lines.push(s);
        }

        // Line 2: Tasks and load.
        {
            let total = self.processes.len();
            let running = self.processes.iter().filter(|p| p.state == 'R').count();
            let threads: u32 = self.processes.iter().map(|p| p.threads).sum();
            let mut s = String::new();
            let _ = write!(
                s,
                "{BOLD}Tasks:{RESET} {BOLD_WHITE}{total}{RESET}, {GREEN}{running} running{RESET}; {DIM}{threads} thr{RESET}"
            );
            lines.push(s);
        }

        // Line 3: Load average and uptime.
        {
            let mut s = String::new();
            let _ = write!(
                s,
                "{BOLD}Load:{RESET} {} {} {} {BOLD}Uptime:{RESET} {}",
                self.load.0,
                self.load.1,
                self.load.2,
                format_uptime(self.uptime)
            );
            lines.push(s);
        }

        lines
    }

    /// Draw one CPU's bar.
    ///
    /// Takes the [`CpuBar`] rather than its fields: four `f64` arguments in a
    /// row is a swap waiting to happen, and swapping two of them produces a
    /// bar that still looks plausible.
    /// An associated function, not a method: it reads nothing from `self`,
    /// and taking `&self` would have kept the clamping below unreachable from
    /// a test without building an `App`.
    fn render_cpu_bar(buf: &mut String, width: usize, bar: &CpuBar) {
        let _ = write!(buf, "[");

        let chars = |fraction: f64| (fraction * width as f64).round() as usize;
        let user_chars = chars(bar.user);
        let system_chars = chars(bar.system);
        let nice_chars = chars(bar.nice);
        let overhead_chars = chars(bar.overhead);
        let filled = (user_chars + system_chars + nice_chars + overhead_chars).min(width);
        let empty = width.saturating_sub(filled);

        // User = green, System = red, Nice = blue, irq/softirq/steal = magenta
        // (which is what real htop uses for interrupt time).
        //
        // Each segment is clamped to what the ones before it left, so the four
        // together can never write more than `width` cells however they round.
        let mut used = 0usize;
        for (colour, want) in [
            (GREEN, user_chars),
            (RED, system_chars),
            (BLUE, nice_chars),
            (MAGENTA, overhead_chars),
        ] {
            let _ = write!(buf, "{colour}");
            let room = width.saturating_sub(used);
            let n = want.min(room);
            for _ in 0..n {
                let _ = write!(buf, "|");
            }
            used = used.saturating_add(n);
        }
        let _ = write!(buf, "{DIM}");
        for _ in 0..empty {
            let _ = write!(buf, " ");
        }
        let _ = write!(buf, "{RESET}]");
    }

    fn render_mem_bar_into(
        &self,
        buf: &mut String,
        width: usize,
        used: f64,
        buffers: f64,
        cache: f64,
    ) {
        let _ = write!(buf, "[");

        let used_chars = (used * width as f64).round() as usize;
        let buf_chars = (buffers * width as f64).round() as usize;
        let cache_chars = (cache * width as f64).round() as usize;
        let filled = (used_chars + buf_chars + cache_chars).min(width);
        let empty = width.saturating_sub(filled);

        // Used = green, Buffers = blue, Cache = yellow.
        let _ = write!(buf, "{GREEN}");
        for _ in 0..used_chars.min(width) {
            let _ = write!(buf, "|");
        }
        let _ = write!(buf, "{BLUE}");
        for _ in 0..buf_chars.min(width.saturating_sub(used_chars)) {
            let _ = write!(buf, "|");
        }
        let _ = write!(buf, "{YELLOW}");
        for _ in 0..cache_chars.min(width.saturating_sub(used_chars + buf_chars)) {
            let _ = write!(buf, "|");
        }
        let _ = write!(buf, "{DIM}");
        for _ in 0..empty {
            let _ = write!(buf, " ");
        }
        let _ = write!(buf, "{RESET}]");
    }

    fn render_swap_bar_into(&self, buf: &mut String, width: usize, used: f64) {
        let _ = write!(buf, "[");

        let used_chars = (used * width as f64).round() as usize;
        let empty = width.saturating_sub(used_chars);

        let _ = write!(buf, "{RED}");
        for _ in 0..used_chars.min(width) {
            let _ = write!(buf, "|");
        }
        let _ = write!(buf, "{DIM}");
        for _ in 0..empty {
            let _ = write!(buf, " ");
        }
        let _ = write!(buf, "{RESET}]");
    }

    fn render_table_header(&self, buf: &mut String) {
        let row = self.header_height();
        let _ = write!(buf, "\x1b[{};1H\x1b[K", row);

        // Column headers with sort indicator.
        let headers: &[(SortField, usize)] = &[
            (SortField::Pid, 7),
            (SortField::User, 8),
            (SortField::Priority, 4),
            (SortField::Nice, 4),
            (SortField::Virt, 7),
            (SortField::Res, 7),
            (SortField::Shr, 7),
            (SortField::State, 1),
            (SortField::Cpu, 5),
            (SortField::Mem, 5),
            (SortField::Time, 10),
            (SortField::Command, 0), // takes remaining space
        ];

        let _ = write!(buf, "{BOLD}{REVERSE}");

        let mut col_pos = 0;
        for (field, width) in headers {
            let label = field.label();
            let is_sort = *field == self.config.sort_field;

            if *field == SortField::Command {
                // Fill remaining width.
                let remaining = self.term_cols.saturating_sub(col_pos);
                if is_sort {
                    let _ = write!(buf, "{BOLD_GREEN}{REVERSE}");
                }
                let _ = write!(buf, " {label:<width$}", width = remaining.saturating_sub(1));
                if is_sort {
                    let _ = write!(buf, "{RESET}{BOLD}{REVERSE}");
                }
            } else {
                if is_sort {
                    let _ = write!(buf, "{RESET}{BOLD_GREEN}{REVERSE}");
                }
                let _ = write!(buf, "{label:>width$}", width = width);
                if is_sort {
                    let _ = write!(buf, "{RESET}{BOLD}{REVERSE}");
                }
                let _ = write!(buf, " ");
                col_pos += width + 1;
            }
        }

        let _ = write!(buf, "{RESET}");
    }

    fn render_process_list(&self, buf: &mut String, display: &[(ProcessInfo, usize)]) {
        let start_row = self.header_height() + 1;
        let visible = self.list_rows();

        for i in 0..visible {
            let screen_row = start_row + i;
            let _ = write!(buf, "\x1b[{};1H\x1b[K", screen_row);

            let idx = self.scroll + i;
            if idx >= display.len() {
                continue;
            }

            let (ref proc_info, depth) = display[idx];
            let is_selected = idx == self.cursor;

            if is_selected {
                let _ = write!(buf, "{REVERSE}");
            }

            // PID.
            let _ = write!(buf, "{:>7} ", proc_info.pid);

            // USER (truncated to 8 chars).
            let user_display = if proc_info.user.len() > 8 {
                &proc_info.user[..8]
            } else {
                &proc_info.user
            };
            let _ = write!(buf, "{user_display:<8} ");

            // PRI.
            let _ = write!(buf, "{:>3} ", proc_info.priority);

            // NI.
            let _ = write!(buf, "{:>3} ", proc_info.nice);

            // VIRT.
            let _ = write!(buf, "{:>7} ", format_kb(proc_info.vsize_kb));

            // RES.
            let _ = write!(buf, "{:>7} ", format_kb(proc_info.rss_kb));

            // SHR.
            let _ = write!(buf, "{:>7} ", format_kb(proc_info.shr_kb));

            // S (state) -- colored.
            if !is_selected {
                let _ = write!(buf, "{}", state_color(proc_info.state));
            }
            let _ = write!(buf, "{} ", proc_info.state);
            if !is_selected {
                let _ = write!(buf, "{RESET}");
                if is_selected {
                    let _ = write!(buf, "{REVERSE}");
                }
            }

            // %CPU.
            if proc_info.cpu_pct > 50.0 && !is_selected {
                let _ = write!(buf, "{BOLD_RED}");
            } else if proc_info.cpu_pct > 10.0 && !is_selected {
                let _ = write!(buf, "{BOLD_YELLOW}");
            }
            let _ = write!(buf, "{:>5.1} ", proc_info.cpu_pct);
            if !is_selected {
                let _ = write!(buf, "{RESET}");
            }

            // %MEM.
            let _ = write!(buf, "{:>5.1} ", proc_info.mem_pct);

            // TIME+.
            let _ = write!(buf, "{:>10} ", proc_info.time_str);

            // COMMAND -- with tree indent if applicable.
            let remaining = self.term_cols.saturating_sub(72); // approximate used columns
            let indent = if self.config.tree_view && depth > 0 {
                let indent_str: String = "  ".repeat(depth.min(10));
                let connector = if depth > 0 { "|- " } else { "" };
                format!("{indent_str}{connector}")
            } else {
                String::new()
            };

            let cmd = &proc_info.cmdline;
            let display_cmd = format!("{indent}{cmd}");
            if display_cmd.len() > remaining {
                let _ = write!(buf, "{}", &display_cmd[..remaining]);
            } else {
                let _ = write!(buf, "{display_cmd}");
            }

            if is_selected {
                // Pad to fill the line for the reverse-video highlight.
                let line_len = 72 + display_cmd.len().min(remaining);
                if line_len < self.term_cols {
                    for _ in 0..(self.term_cols - line_len) {
                        let _ = write!(buf, " ");
                    }
                }
                let _ = write!(buf, "{RESET}");
            }
        }
    }

    fn render_footer(&self, buf: &mut String) {
        let row = self.term_rows;
        let _ = write!(buf, "\x1b[{row};1H\x1b[K");

        // If in search mode, show the search prompt.
        if self.overlay == Overlay::Search {
            let _ = write!(buf, "{BOLD}Filter: {RESET}{}", self.filter);
            show_cursor();
            return;
        }

        // Status message overrides the function key bar.
        if !self.status_msg.is_empty() {
            let _ = write!(buf, " {BOLD}{}{RESET}", self.status_msg);
            return;
        }

        // Function key bar.
        let keys = [
            ("F1", "Help"),
            ("F2", "Setup"),
            ("F3", "Search"),
            ("F4", "Filter"),
            ("F5", "Tree"),
            ("F6", "Sort"),
            ("F7", "Nice-"),
            ("F8", "Nice+"),
            ("F9", "Kill"),
            ("F10", "Quit"),
        ];

        let item_width = self.term_cols / keys.len().max(1);
        for (key, label) in &keys {
            let _ = write!(buf, "{REVERSE}{BOLD}{key}{RESET}");
            let max_label = item_width.saturating_sub(key.len() + 1);
            let display_label = if label.len() > max_label {
                &label[..max_label]
            } else {
                label
            };
            let _ = write!(buf, "{display_label:<width$}", width = max_label);
        }
    }

    fn render_help(&self, buf: &mut String) {
        let _ = write!(buf, "\x1b[2J\x1b[H");
        let _ = write!(
            buf,
            "{BOLD}{REVERSE} htop {VERSION} -- Slate OS Interactive Process Viewer "
        );
        // Pad to full width.
        let title_len = 47;
        for _ in title_len..self.term_cols {
            let _ = write!(buf, " ");
        }
        let _ = write!(buf, "{RESET}\r\n\r\n");

        let help_lines = [
            "  CPU bars: user(green) system(red) nice(blue) irq/steal(magenta)",
            "  Mem bar:  used(green) buffers(blue) cache(yellow)",
            "  Swap bar: used(red)",
            "",
            "  Navigation:",
            "    Up/Down/PgUp/PgDn  Move cursor",
            "    Home/End           Jump to first/last",
            "",
            "  Actions:",
            "    F1           This help screen",
            "    F3, /        Search/filter by name",
            "    F4           Clear filter",
            "    F5           Toggle tree view",
            "    F6           Choose sort column",
            "    F7           Decrease nice (higher priority)",
            "    F8           Increase nice (lower priority)",
            "    F9           Kill selected process",
            "    F10, q       Quit htop",
            "",
            "  Sorting shortcuts:",
            "    P            Sort by CPU%",
            "    M            Sort by MEM%",
            "    T            Sort by TIME+",
            "    N            Sort by PID",
            "",
            "  Process state colors:",
            "    R = Running (green)",
            "    S = Sleeping (cyan)",
            "    Z = Zombie (red)",
            "    T = Stopped (yellow)",
            "    D = Disk sleep (bold red)",
            "",
            "  Press any key to return.",
        ];

        for line in &help_lines {
            let _ = write!(buf, "  {line}\r\n");
        }
    }

    fn render_sort_picker(&self, buf: &mut String) {
        let row = self.header_height();
        let _ = write!(buf, "\x1b[{};1H\x1b[K", row);
        let _ = write!(buf, " {BOLD}Sort by:{RESET}");

        let fields = SortField::all();
        for (i, field) in fields.iter().enumerate() {
            let screen_row = row + 1 + i;
            let _ = write!(buf, "\x1b[{screen_row};1H\x1b[K");
            let marker = if i == self.sort_cursor { ">" } else { " " };
            let highlight = if *field == self.config.sort_field {
                BOLD_GREEN
            } else {
                ""
            };
            let _ = write!(
                buf,
                " {BOLD}{marker}{RESET} {highlight}{}{RESET}",
                field.label()
            );
        }

        // Clear remaining lines.
        let used = row + 1 + fields.len();
        for r in used..self.term_rows {
            let _ = write!(buf, "\x1b[{};1H\x1b[K", r + 1);
        }

        // Footer.
        let _ = write!(buf, "\x1b[{};1H\x1b[K", self.term_rows);
        let _ = write!(
            buf,
            " {DIM}Up/Down to select, Enter to confirm, Esc to cancel{RESET}"
        );
    }

    fn render_kill_prompt(&self, buf: &mut String) {
        let row = self.term_rows;
        let _ = write!(buf, "\x1b[{row};1H\x1b[K");

        let display = self.display_processes();
        if self.cursor < display.len() {
            let pid = display[self.cursor].0.pid;
            let name = &display[self.cursor].0.name;
            let _ = write!(
                buf,
                " {BOLD_RED}Kill{RESET} PID {BOLD}{pid}{RESET} ({name})? [y/N] "
            );
        } else {
            let _ = write!(buf, " {DIM}No process selected{RESET}");
        }
        show_cursor();
    }

    // ========================================================================
    // Input handling
    // ========================================================================

    fn handle_key(&mut self, key: Key) {
        match self.overlay {
            Overlay::Normal => self.handle_normal_key(key),
            Overlay::Help => {
                // Any key dismisses help.
                if key != Key::None {
                    self.overlay = Overlay::Normal;
                }
            }
            Overlay::SortPicker => self.handle_sort_picker_key(key),
            Overlay::KillConfirm => self.handle_kill_confirm_key(key),
            Overlay::Search => self.handle_search_key(key),
        }
    }

    fn handle_normal_key(&mut self, key: Key) {
        let list_len = self.display_processes().len();

        match key {
            Key::None => {}
            Key::F(10) | Key::Char('q') | Key::Ctrl('c') => {
                self.running = false;
            }
            Key::F(1) | Key::Char('?') => {
                self.overlay = Overlay::Help;
            }
            Key::F(3) | Key::Char('/') => {
                self.overlay = Overlay::Search;
            }
            Key::F(4) => {
                // Clear filter.
                self.filter.clear();
                self.cursor = 0;
                self.scroll = 0;
                self.status_msg = "Filter cleared".to_string();
            }
            Key::F(5) | Key::Char('t') => {
                self.config.tree_view = !self.config.tree_view;
                self.status_msg = if self.config.tree_view {
                    "Tree view enabled".to_string()
                } else {
                    "Tree view disabled".to_string()
                };
            }
            Key::F(6) => {
                self.sort_cursor = SortField::all()
                    .iter()
                    .position(|f| *f == self.config.sort_field)
                    .unwrap_or(0);
                self.overlay = Overlay::SortPicker;
            }
            Key::F(7) => {
                // Decrease nice (higher priority) for selected process.
                self.adjust_nice(-1);
            }
            Key::F(8) => {
                // Increase nice (lower priority) for selected process.
                self.adjust_nice(1);
            }
            Key::F(9) | Key::Char('k') => {
                self.overlay = Overlay::KillConfirm;
            }
            Key::Up if self.cursor > 0 => {
                self.cursor -= 1;
            }
            Key::Down if self.cursor + 1 < list_len => {
                self.cursor += 1;
            }
            Key::PageUp => {
                let page = self.list_rows();
                self.cursor = self.cursor.saturating_sub(page);
            }
            Key::PageDown => {
                let page = self.list_rows();
                self.cursor = (self.cursor + page).min(list_len.saturating_sub(1));
            }
            Key::Home => {
                self.cursor = 0;
                self.scroll = 0;
            }
            Key::End => {
                self.cursor = list_len.saturating_sub(1);
            }

            // Sort shortcuts.
            Key::Char('P') => {
                self.config.sort_field = SortField::Cpu;
                self.status_msg = "Sort: %CPU".to_string();
            }
            Key::Char('M') => {
                self.config.sort_field = SortField::Mem;
                self.status_msg = "Sort: %MEM".to_string();
            }
            Key::Char('T') => {
                self.config.sort_field = SortField::Time;
                self.status_msg = "Sort: TIME+".to_string();
            }
            Key::Char('N') => {
                self.config.sort_field = SortField::Pid;
                self.status_msg = "Sort: PID".to_string();
            }

            // Refresh terminal size.
            Key::Ctrl('l') => {
                let (rows, cols) = terminal_size();
                self.term_rows = rows;
                self.term_cols = cols;
                clear_screen();
            }

            _ => {}
        }
    }

    fn handle_sort_picker_key(&mut self, key: Key) {
        let fields = SortField::all();
        match key {
            Key::Up if self.sort_cursor > 0 => {
                self.sort_cursor -= 1;
            }
            Key::Down if self.sort_cursor + 1 < fields.len() => {
                self.sort_cursor += 1;
            }
            Key::Enter => {
                if self.sort_cursor < fields.len() {
                    self.config.sort_field = fields[self.sort_cursor];
                    self.status_msg = format!("Sort: {}", fields[self.sort_cursor].label());
                }
                self.overlay = Overlay::Normal;
            }
            Key::Escape | Key::F(6) => {
                self.overlay = Overlay::Normal;
            }
            _ => {}
        }
    }

    fn handle_kill_confirm_key(&mut self, key: Key) {
        match key {
            Key::Char('y') | Key::Char('Y') => {
                self.kill_selected();
                self.overlay = Overlay::Normal;
            }
            Key::Char('n') | Key::Char('N') | Key::Escape | Key::Enter => {
                self.overlay = Overlay::Normal;
                self.status_msg = "Kill cancelled".to_string();
            }
            _ => {}
        }
    }

    fn handle_search_key(&mut self, key: Key) {
        match key {
            Key::Enter | Key::Escape => {
                self.overlay = Overlay::Normal;
                if self.filter.is_empty() {
                    self.status_msg.clear();
                } else {
                    self.status_msg = format!("Filter: {}", self.filter);
                }
                self.cursor = 0;
                self.scroll = 0;
            }
            Key::Backspace => {
                self.filter.pop();
            }
            Key::Char(c) => {
                self.filter.push(c);
            }
            Key::Ctrl('u') => {
                self.filter.clear();
            }
            _ => {}
        }
    }

    // ========================================================================
    // Process actions
    // ========================================================================

    fn kill_selected(&mut self) {
        let display = self.display_processes();
        if self.cursor >= display.len() {
            self.status_msg = "No process selected".to_string();
            return;
        }

        let pid = display[self.cursor].0.pid;
        let name = display[self.cursor].0.name.clone();

        // SAFETY: SYS_PROCESS_KILL takes the target PID and exit code.
        // We pass exit code 9 (SIGKILL equivalent) and 0 for unused arg3.
        let ret = unsafe { syscall3(SYS_PROCESS_KILL, u64::from(pid), 9, 0) };

        if ret >= 0 {
            self.status_msg = format!("Killed PID {pid} ({name})");
        } else {
            self.status_msg = format!("Failed to kill PID {pid}: error {ret}");
        }
    }

    fn adjust_nice(&mut self, delta: i32) {
        let display = self.display_processes();
        if self.cursor >= display.len() {
            self.status_msg = "No process selected".to_string();
            return;
        }

        let pid = display[self.cursor].0.pid;
        let name = display[self.cursor].0.name.clone();

        // SAFETY: getpriority/setpriority are provided by the POSIX layer.
        // PRIO_PROCESS with the pid targets a single process.
        let current = unsafe { getpriority(PRIO_PROCESS, pid) };
        let new_nice = (current + delta).clamp(-20, 19);
        // SAFETY: Same FFI call with validated parameters.
        let ret = unsafe { setpriority(PRIO_PROCESS, pid, new_nice) };

        if ret == 0 {
            self.status_msg = format!("PID {pid} ({name}): nice {current} -> {new_nice}");
        } else {
            self.status_msg = format!("Failed to renice PID {pid}: permission denied");
        }
    }

    // ========================================================================
    // Main loop
    // ========================================================================

    fn run(&mut self) {
        enter_alternate_screen();
        clear_screen();

        // Initial data load.
        self.refresh_data();

        // Track time for periodic refresh.
        let mut tick_counter: u64 = 0;
        // With VTIME=1 (0.1s timeout), we need delay_secs * 10 ticks per refresh.
        let ticks_per_refresh = self.config.delay_secs.saturating_mul(10).max(1);

        while self.running {
            self.render();

            let key = read_key();

            // Clear status message after the first render following the action.
            if key != Key::None
                && !self.status_msg.is_empty()
                && self.overlay != Overlay::KillConfirm
            {
                self.status_msg.clear();
            }

            if key != Key::None {
                self.handle_key(key);
                tick_counter = 0; // Reset timer on user interaction.
            } else {
                tick_counter += 1;
            }

            // Periodic data refresh.
            if tick_counter >= ticks_per_refresh {
                self.refresh_data();

                // Re-detect terminal size on each refresh.
                let (rows, cols) = terminal_size();
                if rows != self.term_rows || cols != self.term_cols {
                    self.term_rows = rows;
                    self.term_cols = cols;
                    clear_screen();
                }

                tick_counter = 0;
            }
        }

        leave_alternate_screen();
        show_cursor();
        write_str(RESET);
        flush();
    }
}

// ============================================================================
// CLI parsing and entry point
// ============================================================================

fn parse_sort_field(s: &str) -> Option<SortField> {
    match s.to_lowercase().as_str() {
        "cpu" | "%cpu" => Some(SortField::Cpu),
        "mem" | "%mem" | "memory" => Some(SortField::Mem),
        "pid" => Some(SortField::Pid),
        "user" => Some(SortField::User),
        "cmd" | "command" | "name" => Some(SortField::Command),
        "time" | "time+" => Some(SortField::Time),
        "pri" | "priority" => Some(SortField::Priority),
        "ni" | "nice" => Some(SortField::Nice),
        "virt" | "vsize" => Some(SortField::Virt),
        "res" | "rss" => Some(SortField::Res),
        "shr" => Some(SortField::Shr),
        "state" | "s" => Some(SortField::State),
        _ => None,
    }
}

fn print_usage() {
    println!("htop {VERSION} -- Slate OS Interactive Process Viewer");
    println!();
    println!("Usage: htop [OPTIONS]");
    println!();
    println!("Options:");
    println!("  -d <secs>      Refresh delay in seconds (default: 1)");
    println!("  -s <field>     Initial sort field: cpu, mem, pid, user, cmd, time");
    println!("  -t             Start in tree view mode");
    println!("  -h, --help     Show this help");
    println!();
    println!("Interactive keys:");
    println!("  F1       Help         F5  Tree      F9  Kill");
    println!("  F3, /    Search       F6  Sort      F10 Quit");
    println!("  F7       Nice-        F8  Nice+");
    println!("  P/M/T/N  Sort by CPU/MEM/TIME/PID");
    println!("  q        Quit");
}

fn main() {
    let args: Vec<String> = env::args().collect();

    let mut config = Config {
        delay_secs: 1,
        sort_field: SortField::Cpu,
        tree_view: false,
    };

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-d" => {
                if i + 1 >= args.len() {
                    eprintln!("error: -d requires a value");
                    process::exit(1);
                }
                config.delay_secs = args[i + 1].parse().unwrap_or(1);
                if config.delay_secs == 0 {
                    config.delay_secs = 1;
                }
                i += 2;
            }
            "-s" => {
                if i + 1 >= args.len() {
                    eprintln!("error: -s requires a field name");
                    process::exit(1);
                }
                config.sort_field = match parse_sort_field(&args[i + 1]) {
                    Some(f) => f,
                    None => {
                        eprintln!("error: unknown sort field: {}", args[i + 1]);
                        eprintln!(
                            "Valid: cpu, mem, pid, user, cmd, time, pri, ni, virt, res, shr, state"
                        );
                        process::exit(1);
                    }
                };
                i += 2;
            }
            "-t" => {
                config.tree_view = true;
                i += 1;
            }
            "-h" | "--help" | "help" => {
                print_usage();
                process::exit(0);
            }
            other => {
                eprintln!("unknown option: {other}");
                eprintln!("Run 'htop --help' for usage.");
                process::exit(1);
            }
        }
    }

    // Enable raw terminal mode.
    let _raw_guard = enable_raw_mode();

    let mut app = App::new(config);
    app.run();
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::{App, CpuBar, cpu_bar_fractions};
    use procinfo::CpuTimes;

    /// The bars divide the **interval**, which is the whole point of the
    /// change that introduced this function. A CPU that was 90% idle since
    /// boot but has been flat out since the last refresh must read ~100%.
    #[test]
    fn the_bar_measures_the_interval_it_is_given() {
        let interval = CpuTimes {
            user: 90,
            system: 10,
            ..CpuTimes::default()
        };
        let bar = cpu_bar_fractions(&interval).unwrap();
        assert!((bar.user - 0.9).abs() < 1e-9);
        assert!((bar.system - 0.1).abs() < 1e-9);
        assert!((bar.percent() - 100.0).abs() < 1e-9);
    }

    /// A zero-length interval has no answer, and `None` is that answer.
    ///
    /// The code this replaced divided by `total().max(1)` and printed 0.0%,
    /// which is a confident claim that the CPU is idle -- said about the first
    /// refresh, before any measurement exists.
    #[test]
    fn a_zero_interval_has_no_reading_rather_than_zero_percent() {
        assert!(cpu_bar_fractions(&CpuTimes::default()).is_none());
    }

    /// Idle time is in the denominator and not drawn, so a quiet CPU reads low
    /// rather than reading nothing.
    #[test]
    fn idle_time_lowers_the_reading_without_hiding_it() {
        let interval = CpuTimes {
            user: 25,
            idle: 75,
            ..CpuTimes::default()
        };
        let bar = cpu_bar_fractions(&interval).unwrap();
        assert!((bar.percent() - 25.0).abs() < 1e-9);
    }

    /// **The under-report this test used to pin.**
    ///
    /// It read `interrupt_and_stolen_time_are_not_counted_yet` and asserted
    /// that a CPU doing nothing but servicing interrupts reads **0%** -- the
    /// real behaviour, deliberately pinned so that fixing it would be a
    /// change with a failing test rather than a silent one. It failed on
    /// 2026-09-10, which is what a pinning test is for, and this is its
    /// inverse.
    #[test]
    fn interrupt_and_stolen_time_are_counted() {
        let interval = CpuTimes {
            irq: 50,
            softirq: 30,
            steal: 20,
            ..CpuTimes::default()
        };
        let bar = cpu_bar_fractions(&interval).expect("a non-empty interval");
        assert!(
            (bar.percent() - 100.0).abs() < 1e-9,
            "a CPU spending the whole interval on interrupts and stolen time \
             must read 100%, not 0%; got {}",
            bar.percent()
        );
        assert!(
            (bar.overhead - 1.0).abs() < 1e-9,
            "all of it belongs to the fourth segment"
        );
        assert_eq!(interval.busy(), 100);
    }

    /// **The number beside the bar and the crate's own idea of busy must
    /// agree.**
    ///
    /// They are computed separately and on purpose: `percent()` sums the four
    /// drawn segments, `busy()` is `total - idle - iowait`. Deriving one from
    /// the other would make them agree by construction and hide a drift --
    /// if `busy()` ever started counting `iowait`, a derived `percent()` would
    /// silently follow it and the bar would stop matching the picture. This
    /// test is what says so instead.
    #[test]
    fn percent_matches_the_crates_definition_of_busy() {
        let interval = CpuTimes {
            user: 20,
            nice: 5,
            system: 10,
            idle: 40,
            iowait: 5,
            irq: 8,
            softirq: 7,
            steal: 5,
            ..CpuTimes::default()
        };
        let bar = cpu_bar_fractions(&interval).expect("a non-empty interval");
        let want = interval.busy() as f64 / interval.total() as f64 * 100.0;
        assert!(
            (bar.percent() - want).abs() < 1e-9,
            "bar says {:.6}%, busy() says {want:.6}%",
            bar.percent()
        );
        // And `iowait` is on neither side: waiting for a disk is not the CPU
        // being busy, which is the one category this deliberately excludes.
        assert!(bar.percent() < 100.0);
    }

    /// **The four segments together never overrun the bar.**
    ///
    /// Each fraction is rounded independently, so four of them can each round
    /// up and ask for more cells than exist -- and a bar that writes past its
    /// own width corrupts the line beside it, which on this screen is the
    /// memory gauge. The clamping is a running total rather than four
    /// independent `min(width)` calls, which is what would let the second
    /// segment reuse the first's room.
    #[test]
    fn the_bar_never_draws_more_cells_than_it_has() {
        // Every segment 0.3 of a 10-cell bar: 3 cells each, 12 wanted, 10
        // available.
        let greedy = CpuBar {
            user: 0.3,
            system: 0.3,
            nice: 0.3,
            overhead: 0.3,
        };
        let mut buf = String::new();
        App::render_cpu_bar(&mut buf, 10, &greedy);

        let cells = buf.chars().filter(|c| *c == '|' || *c == ' ').count();
        let filled = buf.chars().filter(|c| *c == '|').count();
        assert_eq!(filled, 10, "a full bar fills exactly its width");
        assert_eq!(cells, 10, "bars plus padding is exactly the width");
        assert!(buf.starts_with('['), "and it is still bracketed");
        assert!(buf.ends_with(']'));
    }

    /// A bar with room to spare pads the remainder and still measures its
    /// width exactly -- the other side of the test above, since a clamp that
    /// was too eager would show here instead.
    #[test]
    fn a_partly_idle_bar_pads_the_rest() {
        let quiet = CpuBar {
            user: 0.2,
            system: 0.1,
            nice: 0.0,
            overhead: 0.0,
        };
        let mut buf = String::new();
        App::render_cpu_bar(&mut buf, 10, &quiet);

        assert_eq!(buf.chars().filter(|c| *c == '|').count(), 3);
        assert_eq!(buf.chars().filter(|c| *c == ' ').count(), 7);
    }
}
