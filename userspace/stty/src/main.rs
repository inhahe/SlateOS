//! OurOS `stty` — Terminal Settings Utility
//!
//! Reads and writes terminal (termios) settings for OurOS userspace processes.
//! Communicates with the kernel tty driver via ioctl syscalls.
//!
//! # Usage
//!
//! ```text
//! stty                         Show a summary of current settings
//! stty -a / --all              Show all settings in human-readable form
//! stty -g / --save             Print settings in stty-restorable form
//! stty -F DEV / --file=DEV     Operate on device DEV instead of stdin
//! stty size                    Print rows and columns
//! stty rows N                  Set terminal height to N rows
//! stty cols N / columns N      Set terminal width to N columns
//! stty ispeed N                Set input baud rate
//! stty ospeed N                Set output baud rate
//! stty N                       Set both input and output baud rate
//! stty SETTING [SETTING...]    Apply one or more named settings
//! ```

#![deny(clippy::all, clippy::pedantic)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_possible_wrap)]
#![allow(clippy::module_name_repetitions)]

use std::env;
use std::fs::File;
#[cfg(unix)]
use std::os::fd::IntoRawFd;
use std::process;

// ============================================================================
// ioctl request codes
// ============================================================================

const TCGETS: u64 = 0x5401;
const TCSETS: u64 = 0x5402;
const TIOCGWINSZ: u64 = 0x5413;
const TIOCSWINSZ: u64 = 0x5414;

// ============================================================================
// Termios flag bit constants — input modes (c_iflag)
// ============================================================================

const IGNBRK: u32 = 0x0000_0001;
const BRKINT: u32 = 0x0000_0002;
const IGNPAR: u32 = 0x0000_0004;
const PARMRK: u32 = 0x0000_0008;
const INPCK: u32 = 0x0000_0010;
const ISTRIP: u32 = 0x0000_0020;
const INLCR: u32 = 0x0000_0040;
const IGNCR: u32 = 0x0000_0080;
const ICRNL: u32 = 0x0000_0100;
const IUCLC: u32 = 0x0000_0200;
const IXON: u32 = 0x0000_0400;
const IXANY: u32 = 0x0000_0800;
const IXOFF: u32 = 0x0000_1000;
const IMAXBEL: u32 = 0x0000_2000;
const IUTF8: u32 = 0x0000_4000;

// ============================================================================
// Termios flag bit constants — output modes (c_oflag)
// ============================================================================

const OPOST: u32 = 0x0000_0001;
const OLCUC: u32 = 0x0000_0002;
const ONLCR: u32 = 0x0000_0004;
const OCRNL: u32 = 0x0000_0008;
const ONOCR: u32 = 0x0000_0010;
const ONLRET: u32 = 0x0000_0020;
const OFILL: u32 = 0x0000_0040;
const OFDEL: u32 = 0x0000_0080;

// ============================================================================
// Termios flag bit constants — control modes (c_cflag)
// ============================================================================

const CBAUD: u32 = 0x0000_100F;
const CSIZE: u32 = 0x0000_0030;
const CS5: u32 = 0x0000_0000;
const CS6: u32 = 0x0000_0010;
const CS7: u32 = 0x0000_0020;
const CS8: u32 = 0x0000_0030;
const CSTOPB: u32 = 0x0000_0040;
const CREAD: u32 = 0x0000_0080;
const PARENB: u32 = 0x0000_0100;
const PARODD: u32 = 0x0000_0200;
const HUPCL: u32 = 0x0000_0400;
const CLOCAL: u32 = 0x0000_0800;
const CRTSCTS: u32 = 0x8000_0000;

// ============================================================================
// Termios flag bit constants — local modes (c_lflag)
// ============================================================================

const ISIG: u32 = 0x0000_0001;
const ICANON: u32 = 0x0000_0002;
const XCASE: u32 = 0x0000_0004;
const ECHO: u32 = 0x0000_0008;
const ECHOE: u32 = 0x0000_0010;
const ECHOK: u32 = 0x0000_0020;
const ECHONL: u32 = 0x0000_0040;
const NOFLSH: u32 = 0x0000_0080;
const TOSTOP: u32 = 0x0000_0100;
const IEXTEN: u32 = 0x0000_8000;

// ============================================================================
// Control character indices (c_cc)
// ============================================================================

const VINTR: usize = 0;
const VQUIT: usize = 1;
const VERASE: usize = 2;
const VKILL: usize = 3;
const VEOF: usize = 4;
const VTIME: usize = 5;
const VMIN: usize = 6;
const VSWTC: usize = 7;
const VSTART: usize = 8;
const VSTOP: usize = 9;
const VSUSP: usize = 10;
const VEOL: usize = 11;
const VREPRINT: usize = 12;
const VDISCARD: usize = 13;
const VWERASE: usize = 14;
const VLNEXT: usize = 15;
const VEOL2: usize = 16;

// ============================================================================
// Baud rate encoding table (Linux/POSIX convention)
// ============================================================================

/// Map a numeric baud rate to its c_cflag CBAUD encoding.
const BAUD_TABLE: &[(u32, u32)] = &[
    (0, 0x0000_0000),
    (50, 0x0000_0001),
    (75, 0x0000_0002),
    (110, 0x0000_0003),
    (134, 0x0000_0004),
    (150, 0x0000_0005),
    (200, 0x0000_0006),
    (300, 0x0000_0007),
    (600, 0x0000_0008),
    (1200, 0x0000_0009),
    (1800, 0x0000_000A),
    (2400, 0x0000_000B),
    (4800, 0x0000_000C),
    (9600, 0x0000_000D),
    (19200, 0x0000_000E),
    (38400, 0x0000_000F),
    (57600, 0x0000_1001),
    (115200, 0x0000_1002),
    (230400, 0x0000_1003),
    (460800, 0x0000_1004),
    (500000, 0x0000_1005),
    (576000, 0x0000_1006),
    (921600, 0x0000_1007),
    (1000000, 0x0000_1008),
    (1152000, 0x0000_1009),
    (1500000, 0x0000_100A),
    (2000000, 0x0000_100B),
    (2500000, 0x0000_100C),
    (3000000, 0x0000_100D),
    (3500000, 0x0000_100E),
    (4000000, 0x0000_100F),
];

/// Encode a numeric baud rate to its CBAUD value. Returns `None` if unknown.
fn baud_encode(rate: u32) -> Option<u32> {
    BAUD_TABLE.iter().find(|(r, _)| *r == rate).map(|(_, enc)| *enc)
}

/// Decode a CBAUD value to a numeric baud rate. Returns 0 if unknown.
fn baud_decode(encoded: u32) -> u32 {
    // The CBAUD field combines low nibble and extended bit.
    let masked = encoded & CBAUD;
    BAUD_TABLE
        .iter()
        .find(|(_, enc)| *enc == masked)
        .map(|(r, _)| *r)
        .unwrap_or(0)
}

// ============================================================================
// Termios and Winsize structures
// ============================================================================

/// Terminal I/O settings as used by TCGETS/TCSETS ioctls.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Termios {
    c_iflag: u32,
    c_oflag: u32,
    c_cflag: u32,
    c_lflag: u32,
    c_line: u8,
    c_cc: [u8; 32],
    c_ispeed: u32,
    c_ospeed: u32,
}

/// Terminal window size as used by TIOCGWINSZ/TIOCSWINSZ ioctls.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Winsize {
    ws_row: u16,
    ws_col: u16,
    ws_xpixel: u16,
    ws_ypixel: u16,
}

// ============================================================================
// libc bindings
// ============================================================================
//
// Terminal settings go through the OurOS posix libc `ioctl()` symbol, which
// dispatches TCGETS/TCSETS/TIOCGWINSZ/TIOCSWINSZ to the kernel tty path.  We
// must NOT hand-roll a raw `syscall` here: the native ABI has no SYS_IOCTL —
// syscall number 16 is SYS_CLOCK_ADJTIME, so a raw `ioctl` would step the
// system clock with the termios pointer reinterpreted as a signed nanosecond
// delta.  Likewise fd close goes through the libc `close()` symbol (native
// syscall 3 is unassigned).  The posix Termios/Winsize structs are
// `#[repr(C)]` with identical layout to ours (NCCS = 32), so the pointers are
// ABI-compatible.

#[cfg(unix)]
unsafe extern "C" {
    /// posix libc `ioctl` symbol — dispatches terminal control requests.
    fn ioctl(fd: i32, request: u64, arg: *mut u8) -> i32;
    /// posix libc `close` symbol — releases a file descriptor.
    fn close(fd: i32) -> i32;
}

// Host (test-only, non-Unix) stubs so the crate still compiles and unit tests
// for the pure-logic functions run.  These are never reached on the real OS.
#[cfg(not(unix))]
unsafe fn ioctl(_fd: i32, _request: u64, _arg: *mut u8) -> i32 {
    -1
}
#[cfg(not(unix))]
unsafe fn close(_fd: i32) -> i32 {
    -1
}

// ============================================================================
// ioctl wrappers
// ============================================================================

/// Read the current termios settings for file descriptor `fd`.
///
/// Returns `Err` with an OS error message on failure.
fn tcgets(fd: i32) -> Result<Termios, String> {
    let mut t = Termios::default();
    // SAFETY: TCGETS reads a Termios struct; the pointer is valid, sized
    // correctly, and ABI-identical to the posix libc Termios.
    let rc = unsafe { ioctl(fd, TCGETS, (&raw mut t).cast::<u8>()) };
    if rc < 0 {
        Err(format!("TCGETS failed: error {}", -rc))
    } else {
        Ok(t)
    }
}

/// Write new termios settings for file descriptor `fd`.
fn tcsets(fd: i32, t: &Termios) -> Result<(), String> {
    // SAFETY: TCSETS reads a Termios struct; the pointer is valid for the
    // duration of the call.  The posix ioctl handler does not mutate it.
    let rc = unsafe { ioctl(fd, TCSETS, core::ptr::from_ref(t).cast::<u8>().cast_mut()) };
    if rc < 0 {
        Err(format!("TCSETS failed: error {}", -rc))
    } else {
        Ok(())
    }
}

/// Read the window size for file descriptor `fd`.
fn tiocgwinsz(fd: i32) -> Result<Winsize, String> {
    let mut ws = Winsize::default();
    // SAFETY: TIOCGWINSZ reads a Winsize struct; the pointer is valid, sized
    // correctly, and ABI-identical to the posix libc Winsize.
    let rc = unsafe { ioctl(fd, TIOCGWINSZ, (&raw mut ws).cast::<u8>()) };
    if rc < 0 {
        Err(format!("TIOCGWINSZ failed: error {}", -rc))
    } else {
        Ok(ws)
    }
}

/// Write new window size for file descriptor `fd`.
fn tiocswinsz(fd: i32, ws: &Winsize) -> Result<(), String> {
    // SAFETY: TIOCSWINSZ reads a Winsize struct; the pointer is valid for the
    // duration of the call.
    let rc = unsafe { ioctl(fd, TIOCSWINSZ, core::ptr::from_ref(ws).cast::<u8>().cast_mut()) };
    if rc < 0 {
        Err(format!("TIOCSWINSZ failed: error {}", -rc))
    } else {
        Ok(())
    }
}

// ============================================================================
// Control character display helpers
// ============================================================================

/// Format a control character value as a human-readable string.
///
/// - `0x00` (`\0`) → `^@` (or `<undef>` for special slots)
/// - `0x01`–`0x1F` → `^A`–`^_`
/// - `0x7F` → `^?`
/// - printable → the character itself
/// - `0xFF` (POSIX `_POSIX_VDISABLE`) → `<undef>`
fn fmt_cc(value: u8) -> String {
    if value == 0xFF {
        return "<undef>".to_string();
    }
    if value == 0x00 {
        return "^@".to_string();
    }
    if value == 0x7F {
        return "^?".to_string();
    }
    if value < 0x20 {
        return format!("^{}", (b'@' + value) as char);
    }
    format!("{}", value as char)
}

/// Parse a control character specification from a string.
///
/// Accepts:
/// - `^X` (caret notation) → `X - 0x40`
/// - Bare single character → its ASCII value
/// - Decimal number → that byte value
/// - `undef` / `<undef>` → `0xFF`
fn parse_cc(s: &str) -> Result<u8, String> {
    if s.eq_ignore_ascii_case("undef") || s == "<undef>" {
        return Ok(0xFF);
    }
    if let Some(rest) = s.strip_prefix('^') {
        let ch = rest.chars().next().ok_or("empty caret notation")?;
        let upper = ch.to_ascii_uppercase();
        if upper >= '@' && upper <= '_' {
            return Ok(upper as u8 - b'@');
        }
        if upper == '?' {
            return Ok(0x7F);
        }
        return Err(format!("invalid caret sequence: ^{ch}"));
    }
    // Try numeric parse first (so "0", "65", "255" are treated as byte values,
    // not as the ASCII characters '0', 'A', etc.).  Single non-digit characters
    // fall through to the single-char branch below.
    if s.chars().all(|c| c.is_ascii_digit()) {
        return s
            .parse::<u8>()
            .map_err(|_| format!("invalid control character (out of range 0-255): '{s}'"));
    }
    if s.len() == 1 {
        return Ok(s.as_bytes()[0]);
    }
    Err(format!("invalid control character: '{s}'"))
}

// ============================================================================
// Display: summary (default) and --all
// ============================================================================

/// A named flag within one of the termios flag words.
struct FlagDef {
    name: &'static str,
    mask: u32,
    /// The "positive" value of the flag when set (usually same as mask).
    on_value: u32,
}

impl FlagDef {
    const fn new(name: &'static str, mask: u32) -> Self {
        Self {
            name,
            mask,
            on_value: mask,
        }
    }
}

/// Input mode flag definitions.
const IFLAG_DEFS: &[FlagDef] = &[
    FlagDef::new("ignbrk", IGNBRK),
    FlagDef::new("brkint", BRKINT),
    FlagDef::new("ignpar", IGNPAR),
    FlagDef::new("parmrk", PARMRK),
    FlagDef::new("inpck", INPCK),
    FlagDef::new("istrip", ISTRIP),
    FlagDef::new("inlcr", INLCR),
    FlagDef::new("igncr", IGNCR),
    FlagDef::new("icrnl", ICRNL),
    FlagDef::new("iuclc", IUCLC),
    FlagDef::new("ixon", IXON),
    FlagDef::new("ixany", IXANY),
    FlagDef::new("ixoff", IXOFF),
    FlagDef::new("imaxbel", IMAXBEL),
    FlagDef::new("iutf8", IUTF8),
];

/// Output mode flag definitions.
const OFLAG_DEFS: &[FlagDef] = &[
    FlagDef::new("opost", OPOST),
    FlagDef::new("olcuc", OLCUC),
    FlagDef::new("onlcr", ONLCR),
    FlagDef::new("ocrnl", OCRNL),
    FlagDef::new("onocr", ONOCR),
    FlagDef::new("onlret", ONLRET),
    FlagDef::new("ofill", OFILL),
    FlagDef::new("ofdel", OFDEL),
];

/// Control mode flag definitions (excluding CSIZE which is multi-bit).
const CFLAG_DEFS: &[FlagDef] = &[
    FlagDef::new("cstopb", CSTOPB),
    FlagDef::new("cread", CREAD),
    FlagDef::new("parenb", PARENB),
    FlagDef::new("parodd", PARODD),
    FlagDef::new("hupcl", HUPCL),
    FlagDef::new("clocal", CLOCAL),
    FlagDef::new("crtscts", CRTSCTS),
];

/// Local mode flag definitions.
const LFLAG_DEFS: &[FlagDef] = &[
    FlagDef::new("isig", ISIG),
    FlagDef::new("icanon", ICANON),
    FlagDef::new("xcase", XCASE),
    FlagDef::new("echo", ECHO),
    FlagDef::new("echoe", ECHOE),
    FlagDef::new("echok", ECHOK),
    FlagDef::new("echonl", ECHONL),
    FlagDef::new("noflsh", NOFLSH),
    FlagDef::new("tostop", TOSTOP),
    FlagDef::new("iexten", IEXTEN),
];

/// Render the flags of one word as a space-separated list.
///
/// Set flags are printed as-is; unset flags are printed with a `-` prefix
/// when `show_negated` is true (used for `--all`).
fn render_flag_word(value: u32, defs: &[FlagDef], show_negated: bool) -> String {
    let mut parts = Vec::new();
    for def in defs {
        let set = (value & def.mask) == def.on_value;
        if set {
            parts.push(def.name.to_string());
        } else if show_negated {
            parts.push(format!("-{}", def.name));
        }
    }
    parts.join(" ")
}

/// Render the c_cflag character-size field as cs5/cs6/cs7/cs8.
fn render_csize(cflag: u32) -> &'static str {
    match cflag & CSIZE {
        CS5 => "cs5",
        CS6 => "cs6",
        CS7 => "cs7",
        _ => "cs8",
    }
}

/// Render control characters for `--all` display.
fn render_cc_all(t: &Termios) -> String {
    let cc_names: &[(&str, usize)] = &[
        ("intr", VINTR),
        ("quit", VQUIT),
        ("erase", VERASE),
        ("kill", VKILL),
        ("eof", VEOF),
        ("eol", VEOL),
        ("eol2", VEOL2),
        ("swtch", VSWTC),
        ("start", VSTART),
        ("stop", VSTOP),
        ("susp", VSUSP),
        ("rprnt", VREPRINT),
        ("werase", VWERASE),
        ("lnext", VLNEXT),
        ("discard", VDISCARD),
        ("min", VMIN),
        ("time", VTIME),
    ];

    let mut pairs: Vec<String> = Vec::new();
    for (name, idx) in cc_names {
        let val = if *idx < t.c_cc.len() { t.c_cc[*idx] } else { 0xFF };
        // min and time are displayed numerically, not as control chars.
        if *name == "min" || *name == "time" {
            pairs.push(format!("{name} = {val}"));
        } else {
            pairs.push(format!("{name} = {}", fmt_cc(val)));
        }
    }
    pairs.join("; ")
}

/// Print a brief summary of current settings (default mode).
fn print_summary(t: &Termios, ws: &Winsize) {
    let ispeed = baud_decode(t.c_ispeed);
    let ospeed = baud_decode(t.c_ospeed);
    if ispeed == ospeed {
        println!("speed {ispeed} baud; rows {}; columns {}", ws.ws_row, ws.ws_col);
    } else {
        println!(
            "ispeed {ispeed} baud; ospeed {ospeed} baud; rows {}; columns {}",
            ws.ws_row, ws.ws_col
        );
    }

    // Line settings that differ from a typical sane default.
    let mut flags: Vec<String> = Vec::new();

    // Check notable input flags.
    let notable_iflags: &[(u32, &str, bool)] = &[
        (ICRNL, "icrnl", true),
        (IXON, "ixon", true),
        (IXOFF, "ixoff", false),
        (ISTRIP, "istrip", false),
        (INLCR, "inlcr", false),
        (IGNCR, "igncr", false),
    ];
    for (bit, name, typical_on) in notable_iflags {
        let on = (t.c_iflag & bit) != 0;
        if on != *typical_on {
            flags.push(if on {
                name.to_string()
            } else {
                format!("-{name}")
            });
        }
    }

    // Check notable output flags.
    if (t.c_oflag & OPOST) == 0 {
        flags.push("-opost".to_string());
    }
    if (t.c_oflag & ONLCR) == 0 {
        flags.push("-onlcr".to_string());
    }

    // Check notable local flags.
    let notable_lflags: &[(u32, &str, bool)] = &[
        (ECHO, "echo", true),
        (ICANON, "icanon", true),
        (ISIG, "isig", true),
        (IEXTEN, "iexten", true),
    ];
    for (bit, name, typical_on) in notable_lflags {
        let on = (t.c_lflag & bit) != 0;
        if on != *typical_on {
            flags.push(if on {
                name.to_string()
            } else {
                format!("-{name}")
            });
        }
    }

    if !flags.is_empty() {
        println!("{}", flags.join(" "));
    }
}

/// Print all settings in human-readable form (`--all`).
fn print_all(t: &Termios, ws: &Winsize) {
    let ispeed = baud_decode(t.c_ispeed);
    let ospeed = baud_decode(t.c_ospeed);
    println!(
        "speed {ispeed} baud; rows {}; columns {}; line = {};",
        ws.ws_row, ws.ws_col, t.c_line
    );
    if ispeed != ospeed {
        println!("ispeed {ispeed} baud; ospeed {ospeed} baud;");
    }
    println!("{}", render_cc_all(t));
    println!(
        "{}; {};",
        render_flag_word(t.c_iflag, IFLAG_DEFS, true),
        render_flag_word(t.c_oflag, OFLAG_DEFS, true)
    );
    println!(
        "{} {}; {};",
        render_csize(t.c_cflag),
        render_flag_word(t.c_cflag, CFLAG_DEFS, true),
        render_flag_word(t.c_lflag, LFLAG_DEFS, true)
    );
}

/// Print settings in stty-restorable form (`--save` / `-g`).
///
/// Output format: colon-separated hex values matching the Termios fields,
/// compatible with feeding back to `stty`.
fn print_save(t: &Termios) {
    // Format: iflag:oflag:cflag:lflag:line:ispeed:ospeed:cc[0..31]
    let cc_hex: Vec<String> = t.c_cc.iter().map(|b| format!("{b:02x}")).collect();
    println!(
        "{:08x}:{:08x}:{:08x}:{:08x}:{:02x}:{:08x}:{:08x}:{}",
        t.c_iflag,
        t.c_oflag,
        t.c_cflag,
        t.c_lflag,
        t.c_line,
        t.c_ispeed,
        t.c_ospeed,
        cc_hex.join(":")
    );
}

// ============================================================================
// Saved-settings restoration
// ============================================================================

/// Parse a saved settings string (as produced by `print_save`) back into a
/// `Termios`. Returns `Err` if the format is invalid.
fn parse_save(s: &str) -> Result<Termios, String> {
    let parts: Vec<&str> = s.split(':').collect();
    // Expected: iflag + oflag + cflag + lflag + line + ispeed + ospeed + 32 cc bytes = 39 fields.
    if parts.len() < 39 {
        return Err(format!(
            "saved settings string has {} fields, expected at least 39",
            parts.len()
        ));
    }
    let parse_hex_u32 = |s: &str| -> Result<u32, String> {
        u32::from_str_radix(s, 16).map_err(|_| format!("invalid hex u32: '{s}'"))
    };
    let parse_hex_u8 = |s: &str| -> Result<u8, String> {
        u8::from_str_radix(s, 16).map_err(|_| format!("invalid hex u8: '{s}'"))
    };

    let c_iflag = parse_hex_u32(parts[0])?;
    let c_oflag = parse_hex_u32(parts[1])?;
    let c_cflag = parse_hex_u32(parts[2])?;
    let c_lflag = parse_hex_u32(parts[3])?;
    let c_line = u8::from_str_radix(parts[4], 16)
        .map_err(|_| format!("invalid hex u8: '{}'", parts[4]))?;
    let c_ispeed = parse_hex_u32(parts[5])?;
    let c_ospeed = parse_hex_u32(parts[6])?;

    let mut c_cc = [0u8; 32];
    for (i, slot) in c_cc.iter_mut().enumerate() {
        *slot = parse_hex_u8(parts[7 + i])?;
    }

    Ok(Termios {
        c_iflag,
        c_oflag,
        c_cflag,
        c_lflag,
        c_line,
        c_cc,
        c_ispeed,
        c_ospeed,
    })
}

// ============================================================================
// Setting application
// ============================================================================

/// Apply a single named setting (or its negation via `-`) to a `Termios`.
///
/// Returns `Ok(true)` if the setting was recognised and applied, `Ok(false)`
/// if the token should be consumed as a value by the preceding setting keyword
/// (e.g., `ispeed 9600`), or `Err` on parse failure.
fn apply_setting(t: &mut Termios, setting: &str) -> Result<(), String> {
    // Negated flag: starts with '-' but is not a bare number.
    let (negated, name) = if setting.starts_with('-') && !setting[1..].starts_with(|c: char| c.is_ascii_digit()) {
        (true, &setting[1..])
    } else {
        (false, setting)
    };

    // ---- input flags ----
    match name {
        "ignbrk" => { toggle_flag(&mut t.c_iflag, IGNBRK, !negated); return Ok(()); }
        "brkint" => { toggle_flag(&mut t.c_iflag, BRKINT, !negated); return Ok(()); }
        "ignpar" => { toggle_flag(&mut t.c_iflag, IGNPAR, !negated); return Ok(()); }
        "parmrk" => { toggle_flag(&mut t.c_iflag, PARMRK, !negated); return Ok(()); }
        "inpck"  => { toggle_flag(&mut t.c_iflag, INPCK,  !negated); return Ok(()); }
        "istrip" => { toggle_flag(&mut t.c_iflag, ISTRIP, !negated); return Ok(()); }
        "inlcr"  => { toggle_flag(&mut t.c_iflag, INLCR,  !negated); return Ok(()); }
        "igncr"  => { toggle_flag(&mut t.c_iflag, IGNCR,  !negated); return Ok(()); }
        "icrnl"  => { toggle_flag(&mut t.c_iflag, ICRNL,  !negated); return Ok(()); }
        "iuclc"  => { toggle_flag(&mut t.c_iflag, IUCLC,  !negated); return Ok(()); }
        "ixon"   => { toggle_flag(&mut t.c_iflag, IXON,   !negated); return Ok(()); }
        "ixany"  => { toggle_flag(&mut t.c_iflag, IXANY,  !negated); return Ok(()); }
        "ixoff"  => { toggle_flag(&mut t.c_iflag, IXOFF,  !negated); return Ok(()); }
        "imaxbel"=> { toggle_flag(&mut t.c_iflag, IMAXBEL,!negated); return Ok(()); }
        "iutf8"  => { toggle_flag(&mut t.c_iflag, IUTF8,  !negated); return Ok(()); }

        // ---- output flags ----
        "opost"  => { toggle_flag(&mut t.c_oflag, OPOST,  !negated); return Ok(()); }
        "olcuc"  => { toggle_flag(&mut t.c_oflag, OLCUC,  !negated); return Ok(()); }
        "onlcr"  => { toggle_flag(&mut t.c_oflag, ONLCR,  !negated); return Ok(()); }
        "ocrnl"  => { toggle_flag(&mut t.c_oflag, OCRNL,  !negated); return Ok(()); }
        "onocr"  => { toggle_flag(&mut t.c_oflag, ONOCR,  !negated); return Ok(()); }
        "onlret" => { toggle_flag(&mut t.c_oflag, ONLRET, !negated); return Ok(()); }
        "ofill"  => { toggle_flag(&mut t.c_oflag, OFILL,  !negated); return Ok(()); }
        "ofdel"  => { toggle_flag(&mut t.c_oflag, OFDEL,  !negated); return Ok(()); }

        // ---- control flags ----
        "cs5"    => { t.c_cflag = (t.c_cflag & !CSIZE) | CS5;  return Ok(()); }
        "cs6"    => { t.c_cflag = (t.c_cflag & !CSIZE) | CS6;  return Ok(()); }
        "cs7"    => { t.c_cflag = (t.c_cflag & !CSIZE) | CS7;  return Ok(()); }
        "cs8"    => { t.c_cflag = (t.c_cflag & !CSIZE) | CS8;  return Ok(()); }
        "cstopb" => { toggle_flag(&mut t.c_cflag, CSTOPB, !negated); return Ok(()); }
        "cread"  => { toggle_flag(&mut t.c_cflag, CREAD,  !negated); return Ok(()); }
        "parenb" => { toggle_flag(&mut t.c_cflag, PARENB, !negated); return Ok(()); }
        "parodd" => { toggle_flag(&mut t.c_cflag, PARODD, !negated); return Ok(()); }
        "hupcl"  => { toggle_flag(&mut t.c_cflag, HUPCL,  !negated); return Ok(()); }
        "clocal" => { toggle_flag(&mut t.c_cflag, CLOCAL, !negated); return Ok(()); }
        "crtscts"=> { toggle_flag(&mut t.c_cflag, CRTSCTS,!negated); return Ok(()); }

        // ---- local flags ----
        "isig"   => { toggle_flag(&mut t.c_lflag, ISIG,   !negated); return Ok(()); }
        "icanon" => { toggle_flag(&mut t.c_lflag, ICANON, !negated); return Ok(()); }
        "xcase"  => { toggle_flag(&mut t.c_lflag, XCASE,  !negated); return Ok(()); }
        "echo"   => { toggle_flag(&mut t.c_lflag, ECHO,   !negated); return Ok(()); }
        "echoe"  => { toggle_flag(&mut t.c_lflag, ECHOE,  !negated); return Ok(()); }
        "echok"  => { toggle_flag(&mut t.c_lflag, ECHOK,  !negated); return Ok(()); }
        "echonl" => { toggle_flag(&mut t.c_lflag, ECHONL, !negated); return Ok(()); }
        "noflsh" => { toggle_flag(&mut t.c_lflag, NOFLSH, !negated); return Ok(()); }
        "tostop" => { toggle_flag(&mut t.c_lflag, TOSTOP, !negated); return Ok(()); }
        "iexten" => { toggle_flag(&mut t.c_lflag, IEXTEN, !negated); return Ok(()); }

        // ---- combo settings ----
        "raw" => {
            // Disable all input/output processing; raw mode.
            t.c_iflag &= !(BRKINT | ICRNL | IGNBRK | IGNCR | IGNPAR | INLCR
                | INPCK | ISTRIP | IXANY | IXOFF | IXON | PARMRK);
            t.c_oflag &= !OPOST;
            t.c_lflag &= !(ECHO | ECHOE | ECHOK | ECHONL | ICANON | IEXTEN | ISIG | NOFLSH | TOSTOP | XCASE);
            t.c_cflag &= !PARENB;
            t.c_cflag = (t.c_cflag & !CSIZE) | CS8;
            // VMIN=1, VTIME=0 for raw byte-at-a-time reads.
            t.c_cc[VMIN] = 1;
            t.c_cc[VTIME] = 0;
            return Ok(());
        }
        "cooked" | "sane" => {
            // Reset to sensible interactive defaults.
            apply_sane(t);
            return Ok(());
        }
        "evenp" | "parity" => {
            // 7-bit, even parity.
            t.c_cflag = (t.c_cflag & !CSIZE) | CS7;
            toggle_flag(&mut t.c_cflag, PARENB, true);
            toggle_flag(&mut t.c_cflag, PARODD, false);
            toggle_flag(&mut t.c_iflag, INPCK, true);
            toggle_flag(&mut t.c_iflag, ISTRIP, true);
            return Ok(());
        }
        "oddp" => {
            // 7-bit, odd parity.
            t.c_cflag = (t.c_cflag & !CSIZE) | CS7;
            toggle_flag(&mut t.c_cflag, PARENB, true);
            toggle_flag(&mut t.c_cflag, PARODD, true);
            toggle_flag(&mut t.c_iflag, INPCK, true);
            toggle_flag(&mut t.c_iflag, ISTRIP, true);
            return Ok(());
        }
        "-parity" | "-evenp" => {
            // Cancel parity.
            t.c_cflag = (t.c_cflag & !CSIZE) | CS8;
            toggle_flag(&mut t.c_cflag, PARENB, false);
            toggle_flag(&mut t.c_cflag, PARODD, false);
            toggle_flag(&mut t.c_iflag, INPCK, false);
            toggle_flag(&mut t.c_iflag, ISTRIP, false);
            return Ok(());
        }
        "-oddp" => {
            t.c_cflag = (t.c_cflag & !CSIZE) | CS8;
            toggle_flag(&mut t.c_cflag, PARENB, false);
            toggle_flag(&mut t.c_cflag, PARODD, false);
            toggle_flag(&mut t.c_iflag, INPCK, false);
            toggle_flag(&mut t.c_iflag, ISTRIP, false);
            return Ok(());
        }

        _ => {}
    }

    // Bare numeric baud rate.
    if let Ok(rate) = setting.parse::<u32>() {
        let enc = baud_encode(rate)
            .ok_or_else(|| format!("unknown baud rate: {rate}"))?;
        t.c_ispeed = enc;
        t.c_ospeed = enc;
        return Ok(());
    }

    Err(format!("unknown setting: '{setting}'"))
}

/// Set or clear a single bit in a flag word.
fn toggle_flag(word: &mut u32, bit: u32, on: bool) {
    if on {
        *word |= bit;
    } else {
        *word &= !bit;
    }
}

/// Reset termios to sane / cooked defaults.
fn apply_sane(t: &mut Termios) {
    // Input: translate CR to NL, enable XON/XOFF flow control.
    t.c_iflag = BRKINT | ICRNL | IXON | IMAXBEL;
    // Output: post-process, translate NL to CR-NL.
    t.c_oflag = OPOST | ONLCR;
    // Control: 8-bit, cread, hupcl.
    t.c_cflag = CS8 | CREAD | HUPCL;
    // Local: canonical, echo, signals, extended processing.
    t.c_lflag = ECHO | ECHOE | ECHOK | ICANON | ISIG | IEXTEN;

    // Standard control characters.
    t.c_cc = [0u8; 32];
    t.c_cc[VINTR] = 0x03;    // ^C
    t.c_cc[VQUIT] = 0x1C;    // ^\
    t.c_cc[VERASE] = 0x7F;   // DEL / ^?
    t.c_cc[VKILL] = 0x15;    // ^U
    t.c_cc[VEOF] = 0x04;     // ^D
    t.c_cc[VSTART] = 0x11;   // ^Q
    t.c_cc[VSTOP] = 0x13;    // ^S
    t.c_cc[VSUSP] = 0x1A;    // ^Z
    t.c_cc[VEOL] = 0x00;
    t.c_cc[VREPRINT] = 0x12; // ^R
    t.c_cc[VDISCARD] = 0x0F; // ^O
    t.c_cc[VWERASE] = 0x17;  // ^W
    t.c_cc[VLNEXT] = 0x16;   // ^V
    t.c_cc[VMIN] = 1;
    t.c_cc[VTIME] = 0;
}

// ============================================================================
// Control-character setting keywords
// ============================================================================

/// Returns Some(cc_index) if `name` is a control-character keyword.
fn cc_index(name: &str) -> Option<usize> {
    match name {
        "intr" => Some(VINTR),
        "quit" => Some(VQUIT),
        "erase" => Some(VERASE),
        "kill" => Some(VKILL),
        "eof" => Some(VEOF),
        "eol" => Some(VEOL),
        "eol2" => Some(VEOL2),
        "swtch" => Some(VSWTC),
        "start" => Some(VSTART),
        "stop" => Some(VSTOP),
        "susp" => Some(VSUSP),
        "rprnt" => Some(VREPRINT),
        "werase" => Some(VWERASE),
        "lnext" => Some(VLNEXT),
        "discard" => Some(VDISCARD),
        "min" => Some(VMIN),
        "time" => Some(VTIME),
        _ => None,
    }
}

// ============================================================================
// Command-line parsing
// ============================================================================

/// The action to perform, as parsed from argv.
enum Action {
    /// Print a brief summary (default).
    Summary,
    /// Print all settings in human-readable form.
    All,
    /// Print settings in stty-restorable form.
    Save,
    /// Restore from a saved settings string.
    Restore(String),
    /// Print rows and columns only.
    Size,
    /// Set terminal height in rows.
    SetRows(u16),
    /// Set terminal width in columns.
    SetCols(u16),
    /// Apply a list of settings (and cc pairs).
    Apply(Vec<Token>),
}

/// A parsed settings token from the command line.
enum Token {
    /// A named flag, optional baud rate, or combo like `raw`.
    Setting(String),
    /// A control-character assignment: (cc_index, value).
    CcAssign { index: usize, value: u8 },
    /// `ispeed N` — set input baud rate encoding.
    Ispeed(u32),
    /// `ospeed N` — set output baud rate encoding.
    Ospeed(u32),
}

/// Configuration: which fd to operate on.
struct Config {
    /// File descriptor to operate on (default: stdin = 0).
    fd: i32,
    /// If non-empty, a path to open instead of stdin.
    device: String,
    /// The action to perform.
    action: Action,
}

/// Parse command-line arguments.
///
/// Returns `Err` on unrecognised options or missing arguments.
fn parse_args(args: &[String]) -> Result<Config, String> {
    let mut device = String::new();
    let mut i = 1usize;

    // Pull off -F / --file options first, then dispatch on remaining args.
    while i < args.len() {
        let arg = &args[i];
        if arg == "-F" || arg == "--file" {
            i += 1;
            device = args.get(i).ok_or("-F requires a device argument")?.clone();
            i += 1;
        } else if let Some(rest) = arg.strip_prefix("--file=") {
            device = rest.to_string();
            i += 1;
        } else if let Some(rest) = arg.strip_prefix("-F") {
            // -F/dev/tty style (no space)
            device = rest.to_string();
            i += 1;
        } else {
            break;
        }
    }

    let remaining: &[String] = &args[i..];

    let action = parse_action(remaining)?;
    Ok(Config { fd: 0, device, action })
}

/// Parse the action from the remaining (post-device) arguments.
fn parse_action(args: &[String]) -> Result<Action, String> {
    if args.is_empty() {
        return Ok(Action::Summary);
    }

    // Single-flag options.
    if args.len() == 1 {
        match args[0].as_str() {
            "-a" | "--all" => return Ok(Action::All),
            "-g" | "--save" | "--gs" => return Ok(Action::Save),
            "size" => return Ok(Action::Size),
            s if looks_like_saved(s) => return Ok(Action::Restore(s.to_string())),
            _ => {}
        }
    }

    // Multi-word actions.
    let mut tokens: Vec<Token> = Vec::new();
    let mut j = 0usize;
    while j < args.len() {
        let word = &args[j];
        match word.as_str() {
            "rows" => {
                j += 1;
                let n: u16 = args
                    .get(j)
                    .ok_or("rows requires a value")?
                    .parse()
                    .map_err(|_| format!("invalid row count: '{}'", args[j]))?;
                // Treat a lone `rows N` as SetRows only if it is the only setting.
                if args.len() == 2 {
                    return Ok(Action::SetRows(n));
                }
                // Otherwise fall through and emit as tokens (treated as SetRows in apply).
                tokens.push(Token::Setting(format!("rows={n}")));
                j += 1;
            }
            "cols" | "columns" => {
                j += 1;
                let n: u16 = args
                    .get(j)
                    .ok_or("cols requires a value")?
                    .parse()
                    .map_err(|_| format!("invalid column count: '{}'", args[j]))?;
                if args.len() == 2 {
                    return Ok(Action::SetCols(n));
                }
                tokens.push(Token::Setting(format!("cols={n}")));
                j += 1;
            }
            "ispeed" => {
                j += 1;
                let rate: u32 = args
                    .get(j)
                    .ok_or("ispeed requires a baud rate")?
                    .parse()
                    .map_err(|_| "ispeed: invalid baud rate".to_string())?;
                let enc = baud_encode(rate)
                    .ok_or_else(|| format!("ispeed: unknown baud rate {rate}"))?;
                tokens.push(Token::Ispeed(enc));
                j += 1;
            }
            "ospeed" => {
                j += 1;
                let rate: u32 = args
                    .get(j)
                    .ok_or("ospeed requires a baud rate")?
                    .parse()
                    .map_err(|_| "ospeed: invalid baud rate".to_string())?;
                let enc = baud_encode(rate)
                    .ok_or_else(|| format!("ospeed: unknown baud rate {rate}"))?;
                tokens.push(Token::Ospeed(enc));
                j += 1;
            }
            cc_name if cc_index(cc_name).is_some() => {
                let idx = cc_index(cc_name).expect("checked above");
                j += 1;
                let val_str = args.get(j).ok_or_else(|| format!("{cc_name} requires a value"))?;
                let val = parse_cc(val_str)?;
                tokens.push(Token::CcAssign { index: idx, value: val });
                j += 1;
            }
            other => {
                tokens.push(Token::Setting(other.to_string()));
                j += 1;
            }
        }
    }

    Ok(Action::Apply(tokens))
}

/// Heuristic: does this string look like the output of `stty --save`?
///
/// Saved strings are colon-separated hex fields: `iflag:oflag:cflag:lflag:...`
fn looks_like_saved(s: &str) -> bool {
    // Must contain at least 6 colons and consist only of hex digits + colons.
    let colon_count = s.chars().filter(|c| *c == ':').count();
    colon_count >= 6 && s.chars().all(|c| c.is_ascii_hexdigit() || c == ':')
}

// ============================================================================
// FD management
// ============================================================================

/// Open a device file and return a raw fd.
///
/// The caller is responsible for closing the fd.
fn open_device(path: &str) -> Result<i32, String> {
    // Use Rust's std::fs::File::open so we don't have to deal with
    // null-terminated strings in inline asm.
    let f = File::open(path).map_err(|e| format!("cannot open '{path}': {e}"))?;
    // SAFETY: IntoRawFd transfers ownership of the fd; close_fd will close it.
    #[cfg(unix)]
    let raw = f.into_raw_fd();
    // On non-Unix hosts (test compilation only), return a placeholder.
    #[cfg(not(unix))]
    let raw = {
        let _ = f;
        -1i32
    };
    Ok(raw)
}

/// Close a raw fd opened by `open_device`.
fn close_fd(fd: i32) {
    // SAFETY: fd was obtained from open_device (a live posix fd); close()
    // releases it via the libc symbol (native syscall 3 is unassigned, so a
    // raw syscall here would be a no-op error).
    unsafe { close(fd) };
}

// ============================================================================
// Top-level run
// ============================================================================

/// Execute the parsed action on the given fd.
///
/// Errors are returned as strings; the caller prints them and exits.
fn run(fd: i32, action: Action) -> Result<(), String> {
    match action {
        Action::Summary => {
            let t = tcgets(fd)?;
            let ws = tiocgwinsz(fd).unwrap_or_default();
            print_summary(&t, &ws);
        }
        Action::All => {
            let t = tcgets(fd)?;
            let ws = tiocgwinsz(fd).unwrap_or_default();
            print_all(&t, &ws);
        }
        Action::Save => {
            let t = tcgets(fd)?;
            print_save(&t);
        }
        Action::Restore(s) => {
            let t = parse_save(&s)?;
            tcsets(fd, &t)?;
        }
        Action::Size => {
            let ws = tiocgwinsz(fd)?;
            println!("{} {}", ws.ws_row, ws.ws_col);
        }
        Action::SetRows(n) => {
            let mut ws = tiocgwinsz(fd).unwrap_or_default();
            ws.ws_row = n;
            tiocswinsz(fd, &ws)?;
        }
        Action::SetCols(n) => {
            let mut ws = tiocgwinsz(fd).unwrap_or_default();
            ws.ws_col = n;
            tiocswinsz(fd, &ws)?;
        }
        Action::Apply(tokens) => {
            let mut t = tcgets(fd)?;
            let mut ws = tiocgwinsz(fd).unwrap_or_default();
            let mut ws_changed = false;

            for token in tokens {
                match token {
                    Token::Setting(s) => {
                        // Handle inline rows=/cols= tokens from multi-setting parse.
                        if let Some(rest) = s.strip_prefix("rows=") {
                            ws.ws_row = rest
                                .parse()
                                .map_err(|_| format!("invalid row count: '{rest}'"))?;
                            ws_changed = true;
                        } else if let Some(rest) = s.strip_prefix("cols=") {
                            ws.ws_col = rest
                                .parse()
                                .map_err(|_| format!("invalid col count: '{rest}'"))?;
                            ws_changed = true;
                        } else {
                            apply_setting(&mut t, &s)?;
                        }
                    }
                    Token::CcAssign { index, value } => {
                        if index < t.c_cc.len() {
                            t.c_cc[index] = value;
                        }
                    }
                    Token::Ispeed(enc) => {
                        t.c_ispeed = enc;
                    }
                    Token::Ospeed(enc) => {
                        t.c_ospeed = enc;
                    }
                }
            }
            tcsets(fd, &t)?;
            if ws_changed {
                tiocswinsz(fd, &ws)?;
            }
        }
    }
    Ok(())
}

// ============================================================================
// Entry point
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();

    let config = match parse_args(&args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("stty: {e}");
            eprintln!("Try 'stty --help' for usage.");
            process::exit(1);
        }
    };

    // Resolve the file descriptor.
    let (fd, owned_fd) = if config.device.is_empty() {
        (config.fd, false)
    } else {
        match open_device(&config.device) {
            Ok(fd) => (fd, true),
            Err(e) => {
                eprintln!("stty: {e}");
                process::exit(1);
            }
        }
    };

    let result = run(fd, config.action);

    if owned_fd {
        close_fd(fd);
    }

    if let Err(e) = result {
        eprintln!("stty: {e}");
        process::exit(1);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ---- baud rate encoding / decoding ----

    #[test]
    fn test_baud_encode_known_rates() {
        assert!(baud_encode(9600).is_some());
        assert!(baud_encode(115200).is_some());
        assert!(baud_encode(38400).is_some());
        assert!(baud_encode(0).is_some());
    }

    #[test]
    fn test_baud_encode_unknown_returns_none() {
        assert!(baud_encode(12345).is_none());
        assert!(baud_encode(99999).is_none());
    }

    #[test]
    fn test_baud_roundtrip() {
        for &(rate, _) in BAUD_TABLE {
            let enc = baud_encode(rate).expect("all table rates must encode");
            assert_eq!(
                baud_decode(enc),
                rate,
                "roundtrip failed for rate {rate}"
            );
        }
    }

    // ---- control character formatting ----

    #[test]
    fn test_fmt_cc_undef() {
        assert_eq!(fmt_cc(0xFF), "<undef>");
    }

    #[test]
    fn test_fmt_cc_nul() {
        assert_eq!(fmt_cc(0x00), "^@");
    }

    #[test]
    fn test_fmt_cc_del() {
        assert_eq!(fmt_cc(0x7F), "^?");
    }

    #[test]
    fn test_fmt_cc_ctrl_c() {
        assert_eq!(fmt_cc(0x03), "^C");
    }

    #[test]
    fn test_fmt_cc_ctrl_d() {
        assert_eq!(fmt_cc(0x04), "^D");
    }

    #[test]
    fn test_fmt_cc_printable() {
        assert_eq!(fmt_cc(b'A'), "A");
        assert_eq!(fmt_cc(b'z'), "z");
    }

    // ---- control character parsing ----

    #[test]
    fn test_parse_cc_caret_notation() {
        assert_eq!(parse_cc("^C").unwrap(), 0x03);
        assert_eq!(parse_cc("^D").unwrap(), 0x04);
        assert_eq!(parse_cc("^?").unwrap(), 0x7F);
        assert_eq!(parse_cc("^@").unwrap(), 0x00);
    }

    #[test]
    fn test_parse_cc_undef() {
        assert_eq!(parse_cc("undef").unwrap(), 0xFF);
        assert_eq!(parse_cc("<undef>").unwrap(), 0xFF);
    }

    #[test]
    fn test_parse_cc_numeric() {
        assert_eq!(parse_cc("65").unwrap(), 65);
        assert_eq!(parse_cc("0").unwrap(), 0);
        assert_eq!(parse_cc("255").unwrap(), 255);
    }

    #[test]
    fn test_parse_cc_single_char() {
        assert_eq!(parse_cc("A").unwrap(), b'A');
    }

    #[test]
    fn test_parse_cc_invalid() {
        // A bare caret with an out-of-range character (e.g. '!', which is below '@').
        assert!(parse_cc("^!").is_err());
        // Numeric value out of u8 range.
        assert!(parse_cc("999").is_err());
        // "^^" is valid: means 0x1E ('^' is ASCII 0x5E; 0x5E - '@'(0x40) = 0x1E).
        assert_eq!(parse_cc("^^").unwrap(), 0x1E);
        // Multi-word strings that aren't numeric or single char.
        assert!(parse_cc("hello").is_err());
    }

    // ---- toggle_flag ----

    #[test]
    fn test_toggle_flag_set() {
        let mut word = 0u32;
        toggle_flag(&mut word, ECHO, true);
        assert_eq!(word & ECHO, ECHO);
    }

    #[test]
    fn test_toggle_flag_clear() {
        let mut word = 0xFFFF_FFFFu32;
        toggle_flag(&mut word, ECHO, false);
        assert_eq!(word & ECHO, 0);
    }

    // ---- apply_setting ----

    #[test]
    fn test_apply_setting_echo_on_off() {
        let mut t = Termios::default();
        apply_setting(&mut t, "echo").unwrap();
        assert_eq!(t.c_lflag & ECHO, ECHO, "echo should be set");
        apply_setting(&mut t, "-echo").unwrap();
        assert_eq!(t.c_lflag & ECHO, 0, "echo should be cleared");
    }

    #[test]
    fn test_apply_setting_cs_sizes() {
        let mut t = Termios::default();
        for name in &["cs5", "cs6", "cs7", "cs8"] {
            apply_setting(&mut t, name).unwrap();
        }
        // After cs8, CSIZE field should be CS8.
        assert_eq!(t.c_cflag & CSIZE, CS8);
    }

    #[test]
    fn test_apply_setting_raw_mode() {
        let mut t = Termios::default();
        // Set some flags that raw should clear.
        t.c_iflag |= ICRNL | IXON;
        t.c_lflag |= ECHO | ICANON;
        t.c_oflag |= OPOST;
        apply_setting(&mut t, "raw").unwrap();
        assert_eq!(t.c_iflag & ICRNL, 0);
        assert_eq!(t.c_iflag & IXON, 0);
        assert_eq!(t.c_lflag & ECHO, 0);
        assert_eq!(t.c_lflag & ICANON, 0);
        assert_eq!(t.c_oflag & OPOST, 0);
        assert_eq!(t.c_cc[VMIN], 1);
        assert_eq!(t.c_cc[VTIME], 0);
    }

    #[test]
    fn test_apply_setting_sane() {
        let mut t = Termios::default();
        apply_setting(&mut t, "sane").unwrap();
        assert_eq!(t.c_iflag & ICRNL, ICRNL);
        assert_eq!(t.c_oflag & OPOST, OPOST);
        assert_eq!(t.c_lflag & ECHO, ECHO);
        assert_eq!(t.c_lflag & ICANON, ICANON);
        assert_eq!(t.c_cflag & CSIZE, CS8);
    }

    #[test]
    fn test_apply_setting_evenp() {
        let mut t = Termios::default();
        apply_setting(&mut t, "evenp").unwrap();
        assert_eq!(t.c_cflag & CSIZE, CS7);
        assert_eq!(t.c_cflag & PARENB, PARENB);
        assert_eq!(t.c_cflag & PARODD, 0);
    }

    #[test]
    fn test_apply_setting_oddp() {
        let mut t = Termios::default();
        apply_setting(&mut t, "oddp").unwrap();
        assert_eq!(t.c_cflag & CSIZE, CS7);
        assert_eq!(t.c_cflag & PARENB, PARENB);
        assert_eq!(t.c_cflag & PARODD, PARODD);
    }

    #[test]
    fn test_apply_setting_baud_rate() {
        let mut t = Termios::default();
        apply_setting(&mut t, "115200").unwrap();
        assert_eq!(t.c_ispeed, baud_encode(115200).unwrap());
        assert_eq!(t.c_ospeed, baud_encode(115200).unwrap());
    }

    #[test]
    fn test_apply_setting_unknown() {
        let mut t = Termios::default();
        assert!(apply_setting(&mut t, "notasetting").is_err());
    }

    #[test]
    fn test_apply_setting_icrnl_toggle() {
        let mut t = Termios::default();
        apply_setting(&mut t, "icrnl").unwrap();
        assert_eq!(t.c_iflag & ICRNL, ICRNL);
        apply_setting(&mut t, "-icrnl").unwrap();
        assert_eq!(t.c_iflag & ICRNL, 0);
    }

    #[test]
    fn test_apply_setting_opost_toggle() {
        let mut t = Termios::default();
        apply_setting(&mut t, "opost").unwrap();
        assert_eq!(t.c_oflag & OPOST, OPOST);
        apply_setting(&mut t, "-opost").unwrap();
        assert_eq!(t.c_oflag & OPOST, 0);
    }

    // ---- save / restore roundtrip ----

    #[test]
    fn test_save_restore_roundtrip() {
        let mut t = Termios::default();
        apply_setting(&mut t, "sane").unwrap();
        t.c_ispeed = baud_encode(115200).unwrap();
        t.c_ospeed = baud_encode(115200).unwrap();

        // Capture the save string by building it manually.
        let cc_hex: Vec<String> = t.c_cc.iter().map(|b| format!("{b:02x}")).collect();
        let saved = format!(
            "{:08x}:{:08x}:{:08x}:{:08x}:{:02x}:{:08x}:{:08x}:{}",
            t.c_iflag,
            t.c_oflag,
            t.c_cflag,
            t.c_lflag,
            t.c_line,
            t.c_ispeed,
            t.c_ospeed,
            cc_hex.join(":")
        );

        let restored = parse_save(&saved).unwrap();
        assert_eq!(restored.c_iflag, t.c_iflag);
        assert_eq!(restored.c_oflag, t.c_oflag);
        assert_eq!(restored.c_cflag, t.c_cflag);
        assert_eq!(restored.c_lflag, t.c_lflag);
        assert_eq!(restored.c_ispeed, t.c_ispeed);
        assert_eq!(restored.c_ospeed, t.c_ospeed);
        assert_eq!(restored.c_cc, t.c_cc);
    }

    #[test]
    fn test_parse_save_too_few_fields() {
        assert!(parse_save("00000000:00000001").is_err());
    }

    // ---- looks_like_saved ----

    #[test]
    fn test_looks_like_saved_valid() {
        // A minimal string with 6+ colons and hex chars.
        assert!(looks_like_saved("0000abcd:00000001:00000002:00000003:04:00001002:00001002"));
    }

    #[test]
    fn test_looks_like_saved_plain_word() {
        assert!(!looks_like_saved("echo"));
        assert!(!looks_like_saved("-icanon"));
        assert!(!looks_like_saved("115200"));
    }

    // ---- render_csize ----

    #[test]
    fn test_render_csize() {
        assert_eq!(render_csize(CS5), "cs5");
        assert_eq!(render_csize(CS6), "cs6");
        assert_eq!(render_csize(CS7), "cs7");
        assert_eq!(render_csize(CS8), "cs8");
    }

    // ---- cc_index ----

    #[test]
    fn test_cc_index_known() {
        assert_eq!(cc_index("intr"), Some(VINTR));
        assert_eq!(cc_index("eof"), Some(VEOF));
        assert_eq!(cc_index("susp"), Some(VSUSP));
    }

    #[test]
    fn test_cc_index_unknown() {
        assert_eq!(cc_index("unknown"), None);
    }

    // ---- render_flag_word ----

    #[test]
    fn test_render_flag_word_echo_set() {
        let flags = render_flag_word(ECHO, LFLAG_DEFS, false);
        assert!(flags.contains("echo"));
    }

    #[test]
    fn test_render_flag_word_negated_shown() {
        let flags = render_flag_word(0, LFLAG_DEFS, true);
        assert!(flags.contains("-echo"));
        assert!(flags.contains("-icanon"));
    }

    // ---- parse_args ----

    #[test]
    fn test_parse_args_default() {
        let args = vec!["stty".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.device.is_empty());
        assert!(matches!(cfg.action, Action::Summary));
    }

    #[test]
    fn test_parse_args_all_flag() {
        let args = vec!["stty".to_string(), "-a".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert!(matches!(cfg.action, Action::All));
    }

    #[test]
    fn test_parse_args_save_flag() {
        let args = vec!["stty".to_string(), "-g".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert!(matches!(cfg.action, Action::Save));
    }

    #[test]
    fn test_parse_args_size() {
        let args = vec!["stty".to_string(), "size".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert!(matches!(cfg.action, Action::Size));
    }

    #[test]
    fn test_parse_args_rows() {
        let args = vec!["stty".to_string(), "rows".to_string(), "40".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert!(matches!(cfg.action, Action::SetRows(40)));
    }

    #[test]
    fn test_parse_args_cols() {
        let args = vec!["stty".to_string(), "cols".to_string(), "132".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert!(matches!(cfg.action, Action::SetCols(132)));
    }

    #[test]
    fn test_parse_args_device_flag() {
        let args = vec![
            "stty".to_string(),
            "-F".to_string(),
            "/dev/tty".to_string(),
            "-a".to_string(),
        ];
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.device, "/dev/tty");
        assert!(matches!(cfg.action, Action::All));
    }

    #[test]
    fn test_parse_args_echo_setting() {
        let args = vec!["stty".to_string(), "-echo".to_string()];
        let cfg = parse_args(&args).unwrap();
        if let Action::Apply(tokens) = cfg.action {
            assert_eq!(tokens.len(), 1);
            assert!(matches!(&tokens[0], Token::Setting(s) if s == "-echo"));
        } else {
            panic!("expected Apply action");
        }
    }

    #[test]
    fn test_parse_args_ispeed() {
        let args = vec![
            "stty".to_string(),
            "ispeed".to_string(),
            "9600".to_string(),
        ];
        let cfg = parse_args(&args).unwrap();
        if let Action::Apply(tokens) = cfg.action {
            let enc = baud_encode(9600).unwrap();
            assert!(matches!(tokens[0], Token::Ispeed(e) if e == enc));
        } else {
            panic!("expected Apply action");
        }
    }
}
