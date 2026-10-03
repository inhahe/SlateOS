//! `stty` — change and print terminal line settings.
//!
//! ```text
//! stty [-F DEVICE | --file=DEVICE] [SETTING]...
//! stty [-F DEVICE | --file=DEVICE] [-a|--all]
//! stty [-F DEVICE | --file=DEVICE] [-g|--save]
//! ```
//!
//! A port of GNU coreutils 9.4's `src/stty.c`, with its tables as the
//! preprocessor resolves them for Linux and glibc -- which flags and control
//! characters exist, at which bits and slots -- since those, not the source's
//! dozens of `#ifdef` arms, are what the program is. The terminal is read and
//! written through the C library (`libcall::termios`), so the speed
//! arithmetic is the library's own: on glibc the input and output speeds share
//! one field, which is why `ispeed 9600 ospeed 38400` is refused as
//! asymmetric. Checked by `scripts/stty-diff.sh`.
//!
//! # Two passes over the command line
//!
//! Upstream's `getopt_long` loop runs in return-in-order mode with errors
//! silenced, and anything it does not recognise -- an operand, an unknown
//! option, `-echo` -- it leaves in argv for a second pass and restarts
//! `getopt` after it, so settings and options may be mixed (`stty -F tty
//! -echo raw`). What the first pass consumed it erases from argv. The second
//! pass, `apply_settings`, then runs twice: once against a blank record to
//! refuse a bad command line before the terminal is touched, and once for
//! real. [`scan`] is the first pass, transcribed with the same restarts, so
//! the quirks come with it: `stty -- echo` prints the settings rather than
//! applying `echo`, because `getopt` stops at the `--` before any setting has
//! been seen.
//!
//! # Replaces `userspace/stty`
//!
//! `userspace/stty` was written from the manual: its own option set, `ioctl`
//! by hand, its own idea of the output -- 1,847 lines, none of them
//! `stty.c`'s. Measured by `scripts/stty-diff.sh` it agreed with GNU 9.4 on
//! 110 of 312 cases -- it read every terminal's speed as `speed 0 baud` -- and
//! this port agrees on all 312.
//!
//! # Deliberate differences from GNU coreutils 9.4
//!
//! The family's two: `--version` is `stty (SlateOS coreutils) 0.1.0`, and
//! `--help` ends before GNU's block of links.
//!
//! `-F DEVICE` is opened and used where it is, rather than reopened over
//! standard input: nothing else here reads standard input, so the two are the
//! same program.

// The host build stops at the `main` below that refuses to run.
#![cfg_attr(not(unix), allow(dead_code))]

use coreutils::getopt::{Opt, Program, Takes};
use libcall::pty::WinSize;
use libcall::termios::{self, Termios};
use std::ffi::OsString;

coreutils::guard_std_fds!();

/// `usage (EXIT_FAILURE)` is status 1.
const STTY: Program = Program::new("stty", 1);

/// Upstream's `getopt_long` string: return-in-order (`-`), so every operand
/// comes back to the loop in its place.
const SHORT_OPTIONS: &str = "-agF:";

/// Upstream's `longopts[]`, in its order. `-debug` is spelled `---debug`.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("all", Takes::Nothing),
    ("save", Takes::Nothing),
    ("file", Takes::Required),
    ("-debug", Takes::Nothing),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

// ---------------------------------------------------------------------------
// The tables, as glibc on Linux resolves them
// ---------------------------------------------------------------------------

/// Which member of `struct termios` a mode lives in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ModeType {
    Control,
    Input,
    Output,
    Local,
    /// A setting made of others (`raw`, `sane`, `evenp`, …), with no bits of
    /// its own.
    Combination,
}

/// Set in `sane` mode.
const SANE_SET: u8 = 1;
/// Unset in `sane` mode.
const SANE_UNSET: u8 = 2;
/// Can be turned off by a leading `-`.
const REV: u8 = 4;
/// Not displayed.
const OMIT: u8 = 8;
/// Not set through `tcsetattr`. No mode on Linux has it; kept so the tests
/// below read upstream's loops as they are.
const NO_SETATTR: u8 = 16;

/// `struct mode_info`.
struct ModeInfo {
    name: &'static str,
    kind: ModeType,
    flags: u8,
    /// The bits the mode sets.
    bits: u32,
    /// The other bits of its field, when it is one of a set (`cs5`..`cs8`).
    mask: u32,
}

const fn m(name: &'static str, kind: ModeType, flags: u8, bits: u32, mask: u32) -> ModeInfo {
    ModeInfo {
        name,
        kind,
        flags,
        bits,
        mask,
    }
}

use ModeType::{Combination as Comb, Control as Ctl, Input as In, Local as Loc, Output as Out};

/// `mode_info[]`, in upstream's order -- which is the display order.
#[rustfmt::skip]
const MODE_INFO: &[ModeInfo] = &[
    m("parenb", Ctl, REV, 0o000400, 0),
    m("parodd", Ctl, REV, 0o001000, 0),
    m("cmspar", Ctl, REV, 0o10000000000, 0),
    m("cs5", Ctl, 0, 0o000000, 0o000060),
    m("cs6", Ctl, 0, 0o000020, 0o000060),
    m("cs7", Ctl, 0, 0o000040, 0o000060),
    m("cs8", Ctl, 0, 0o000060, 0o000060),
    m("hupcl", Ctl, REV, 0o002000, 0),
    m("hup", Ctl, REV | OMIT, 0o002000, 0),
    m("cstopb", Ctl, REV, 0o000100, 0),
    m("cread", Ctl, SANE_SET | REV, 0o000200, 0),
    m("clocal", Ctl, REV, 0o004000, 0),
    m("crtscts", Ctl, REV, 0o20000000000, 0),
    m("ignbrk", In, SANE_UNSET | REV, 0o000001, 0),
    m("brkint", In, SANE_SET | REV, 0o000002, 0),
    m("ignpar", In, REV, 0o000004, 0),
    m("parmrk", In, REV, 0o000010, 0),
    m("inpck", In, REV, 0o000020, 0),
    m("istrip", In, REV, 0o000040, 0),
    m("inlcr", In, SANE_UNSET | REV, 0o000100, 0),
    m("igncr", In, SANE_UNSET | REV, 0o000200, 0),
    m("icrnl", In, SANE_SET | REV, 0o000400, 0),
    m("ixon", In, REV, 0o002000, 0),
    m("ixoff", In, SANE_UNSET | REV, 0o010000, 0),
    m("tandem", In, REV | OMIT, 0o010000, 0),
    m("iuclc", In, SANE_UNSET | REV, 0o001000, 0),
    m("ixany", In, SANE_UNSET | REV, 0o004000, 0),
    m("imaxbel", In, SANE_SET | REV, 0o020000, 0),
    m("iutf8", In, SANE_UNSET | REV, 0o040000, 0),
    m("opost", Out, SANE_SET | REV, 0o000001, 0),
    m("olcuc", Out, SANE_UNSET | REV, 0o000002, 0),
    m("ocrnl", Out, SANE_UNSET | REV, 0o000010, 0),
    m("onlcr", Out, SANE_SET | REV, 0o000004, 0),
    m("onocr", Out, SANE_UNSET | REV, 0o000020, 0),
    m("onlret", Out, SANE_UNSET | REV, 0o000040, 0),
    m("ofill", Out, SANE_UNSET | REV, 0o000100, 0),
    m("ofdel", Out, SANE_UNSET | REV, 0o000200, 0),
    m("nl1", Out, SANE_UNSET, 0o000400, 0o000400),
    m("nl0", Out, SANE_SET, 0o000000, 0o000400),
    m("cr3", Out, SANE_UNSET, 0o003000, 0o003000),
    m("cr2", Out, SANE_UNSET, 0o002000, 0o003000),
    m("cr1", Out, SANE_UNSET, 0o001000, 0o003000),
    m("cr0", Out, SANE_SET, 0o000000, 0o003000),
    m("tab3", Out, SANE_UNSET, 0o014000, 0o014000),
    m("tab2", Out, SANE_UNSET, 0o010000, 0o014000),
    m("tab1", Out, SANE_UNSET, 0o004000, 0o014000),
    m("tab0", Out, SANE_SET, 0o000000, 0o014000),
    m("bs1", Out, SANE_UNSET, 0o020000, 0o020000),
    m("bs0", Out, SANE_SET, 0o000000, 0o020000),
    m("vt1", Out, SANE_UNSET, 0o040000, 0o040000),
    m("vt0", Out, SANE_SET, 0o000000, 0o040000),
    m("ff1", Out, SANE_UNSET, 0o100000, 0o100000),
    m("ff0", Out, SANE_SET, 0o000000, 0o100000),
    m("isig", Loc, SANE_SET | REV, 0o000001, 0),
    m("icanon", Loc, SANE_SET | REV, 0o000002, 0),
    m("iexten", Loc, SANE_SET | REV, 0o100000, 0),
    m("echo", Loc, SANE_SET | REV, 0o000010, 0),
    m("echoe", Loc, SANE_SET | REV, 0o000020, 0),
    m("crterase", Loc, REV | OMIT, 0o000020, 0),
    m("echok", Loc, SANE_SET | REV, 0o000040, 0),
    m("echonl", Loc, SANE_UNSET | REV, 0o000100, 0),
    m("noflsh", Loc, SANE_UNSET | REV, 0o000200, 0),
    m("xcase", Loc, SANE_UNSET | REV, 0o000004, 0),
    m("tostop", Loc, SANE_UNSET | REV, 0o000400, 0),
    m("echoprt", Loc, SANE_UNSET | REV, 0o002000, 0),
    m("prterase", Loc, REV | OMIT, 0o002000, 0),
    m("echoctl", Loc, SANE_SET | REV, 0o001000, 0),
    m("ctlecho", Loc, REV | OMIT, 0o001000, 0),
    m("echoke", Loc, SANE_SET | REV, 0o004000, 0),
    m("crtkill", Loc, REV | OMIT, 0o004000, 0),
    m("flusho", Loc, SANE_UNSET | REV, 0o010000, 0),
    m("extproc", Loc, SANE_UNSET | REV, 0o200000, 0),
    m("evenp", Comb, REV | OMIT, 0, 0),
    m("parity", Comb, REV | OMIT, 0, 0),
    m("oddp", Comb, REV | OMIT, 0, 0),
    m("nl", Comb, REV | OMIT, 0, 0),
    m("ek", Comb, OMIT, 0, 0),
    m("sane", Comb, OMIT, 0, 0),
    m("cooked", Comb, REV | OMIT, 0, 0),
    m("raw", Comb, REV | OMIT, 0, 0),
    m("pass8", Comb, REV | OMIT, 0, 0),
    m("litout", Comb, REV | OMIT, 0, 0),
    m("cbreak", Comb, REV | OMIT, 0, 0),
    m("decctlq", Comb, REV | OMIT, 0, 0),
    m("tabs", Comb, REV | OMIT, 0, 0),
    m("lcase", Comb, REV | OMIT, 0, 0),
    m("LCASE", Comb, REV | OMIT, 0, 0),
    m("crt", Comb, OMIT, 0, 0),
    m("dec", Comb, OMIT, 0, 0),
];

/// `struct control_info`: a control character's name, its `sane` value, and
/// its slot in `c_cc`.
struct ControlInfo {
    name: &'static str,
    saneval: u8,
    offset: usize,
}

const fn c(name: &'static str, saneval: u8, offset: usize) -> ControlInfo {
    ControlInfo {
        name,
        saneval,
        offset,
    }
}

/// `control_info[]`, in upstream's order. Everything before `min` is a
/// character; `min` and `time` are numbers.
const CONTROL_INFO: &[ControlInfo] = &[
    c("intr", 0x03, 0),
    c("quit", 0o034, 1),
    c("erase", 0o177, 2),
    c("kill", 0x15, 3),
    c("eof", 0x04, 4),
    c("eol", 0, 11),
    c("eol2", 0, 16),
    c("swtch", 0, 7),
    c("start", 0x11, 8),
    c("stop", 0x13, 9),
    c("susp", 0x1a, 10),
    c("rprnt", 0x12, 12),
    c("werase", 0x17, 14),
    c("lnext", 0x16, 15),
    c("flush", 0x0f, 13),
    c("discard", 0x0f, 13),
    c("min", 1, 6),
    c("time", 0, 5),
];

/// `VMIN` and `VTIME`.
const VMIN: usize = 6;
const VTIME: usize = 5;
/// `VINTR`, `VERASE`, `VKILL`.
const VINTR: usize = 0;
const VERASE: usize = 2;
const VKILL: usize = 3;

/// `speeds[]`: a speed as typed, its `B*` constant, and its baud rate.
#[rustfmt::skip]
const SPEEDS: &[(&str, u32, u64)] = &[
    ("0", 0o000000, 0), ("50", 0o000001, 50), ("75", 0o000002, 75),
    ("110", 0o000003, 110), ("134", 0o000004, 134), ("134.5", 0o000004, 134),
    ("150", 0o000005, 150), ("200", 0o000006, 200), ("300", 0o000007, 300),
    ("600", 0o000010, 600), ("1200", 0o000011, 1200), ("1800", 0o000012, 1800),
    ("2400", 0o000013, 2400), ("4800", 0o000014, 4800), ("9600", 0o000015, 9600),
    ("19200", 0o000016, 19200), ("38400", 0o000017, 38400),
    ("exta", 0o000016, 19200), ("extb", 0o000017, 38400),
    ("57600", 0o010001, 57600), ("115200", 0o010002, 115200),
    ("230400", 0o010003, 230400), ("460800", 0o010004, 460800),
    ("500000", 0o010005, 500000), ("576000", 0o010006, 576000),
    ("921600", 0o010007, 921600), ("1000000", 0o010010, 1000000),
    ("1152000", 0o010011, 1152000), ("1500000", 0o010012, 1500000),
    ("2000000", 0o010013, 2000000), ("2500000", 0o010014, 2500000),
    ("3000000", 0o010015, 3000000), ("3500000", 0o010016, 3500000),
    ("4000000", 0o010017, 4000000),
];

// The flag bits the combination settings name.
const PARENB: u32 = 0o000400;
const PARODD: u32 = 0o001000;
const CSIZE: u32 = 0o000060;
const CS7: u32 = 0o000040;
const CS8: u32 = 0o000060;
const ICRNL: u32 = 0o000400;
const INLCR: u32 = 0o000100;
const IGNCR: u32 = 0o000200;
const ISTRIP: u32 = 0o000040;
const BRKINT: u32 = 0o000002;
const IGNPAR: u32 = 0o000004;
const IXON: u32 = 0o002000;
const IXANY: u32 = 0o004000;
const IUCLC: u32 = 0o001000;
const OPOST: u32 = 0o000001;
const ONLCR: u32 = 0o000004;
const OCRNL: u32 = 0o000010;
const ONLRET: u32 = 0o000040;
const OLCUC: u32 = 0o000002;
const TABDLY: u32 = 0o014000;
const TAB3: u32 = 0o014000;
const ISIG: u32 = 0o000001;
const ICANON: u32 = 0o000002;
const XCASE: u32 = 0o000004;
const ECHOE: u32 = 0o000020;
const ECHOCTL: u32 = 0o001000;
const ECHOKE: u32 = 0o004000;

/// `string_to_baud`: the `B*` constant for a speed as typed.
fn string_to_baud(arg: &[u8]) -> Option<u32> {
    SPEEDS
        .iter()
        .find(|(s, _, _)| s.as_bytes() == arg)
        .map(|&(_, speed, _)| speed)
}

/// `baud_to_value`: the baud rate of a `B*` constant, or 0.
fn baud_to_value(speed: u32) -> u64 {
    SPEEDS
        .iter()
        .find(|&&(_, s, _)| s == speed)
        .map_or(0, |&(_, _, value)| value)
}

/// `mode->c_cc[offset]`. Every offset in the tables is below `NCCS`.
fn cc(mode: &Termios, offset: usize) -> u8 {
    mode.c_cc.get(offset).copied().unwrap_or(0)
}

/// `mode->c_cc[offset] = value`.
fn set_cc(mode: &mut Termios, offset: usize, value: u8) {
    if let Some(slot) = mode.c_cc.get_mut(offset) {
        *slot = value;
    }
}

/// `mode_type_flag`: the field a mode lives in, or `None` for a combination.
fn mode_type_flag(kind: ModeType, mode: &mut Termios) -> Option<&mut u32> {
    match kind {
        ModeType::Control => Some(&mut mode.c_cflag),
        ModeType::Input => Some(&mut mode.c_iflag),
        ModeType::Output => Some(&mut mode.c_oflag),
        ModeType::Local => Some(&mut mode.c_lflag),
        ModeType::Combination => None,
    }
}

/// The same, read-only, for the display loops.
fn mode_type_value(kind: ModeType, mode: &Termios) -> Option<u32> {
    match kind {
        ModeType::Control => Some(mode.c_cflag),
        ModeType::Input => Some(mode.c_iflag),
        ModeType::Output => Some(mode.c_oflag),
        ModeType::Local => Some(mode.c_lflag),
        ModeType::Combination => None,
    }
}

// ---------------------------------------------------------------------------
// Changing a record
// ---------------------------------------------------------------------------

/// `set_mode`: apply one mode, or its reverse. `false` when it has no reverse.
fn set_mode(info: &ModeInfo, reversed: bool, mode: &mut Termios) -> bool {
    if reversed && info.flags & REV == 0 {
        return false;
    }
    if let Some(bits) = mode_type_flag(info.kind, mode) {
        *bits = if reversed {
            *bits & !info.mask & !info.bits
        } else {
            (*bits & !info.mask) | info.bits
        };
        return true;
    }
    match info.name {
        "evenp" | "parity" => {
            mode.c_cflag = if reversed {
                (mode.c_cflag & !PARENB & !CSIZE) | CS8
            } else {
                (mode.c_cflag & !PARODD & !CSIZE) | PARENB | CS7
            };
        }
        "oddp" => {
            mode.c_cflag = if reversed {
                (mode.c_cflag & !PARENB & !CSIZE) | CS8
            } else {
                (mode.c_cflag & !CSIZE) | CS7 | PARODD | PARENB
            };
        }
        "nl" => {
            if reversed {
                mode.c_iflag = (mode.c_iflag | ICRNL) & !INLCR & !IGNCR;
                mode.c_oflag = (mode.c_oflag | ONLCR) & !OCRNL & !ONLRET;
            } else {
                mode.c_iflag &= !ICRNL;
                mode.c_oflag &= !ONLCR;
            }
        }
        "ek" => {
            mode.c_cc[VERASE] = 0o177;
            mode.c_cc[VKILL] = 0x15;
        }
        "sane" => sane_mode(mode),
        "cbreak" => {
            if reversed {
                mode.c_lflag |= ICANON;
            } else {
                mode.c_lflag &= !ICANON;
            }
        }
        "pass8" => {
            if reversed {
                mode.c_cflag = (mode.c_cflag & !CSIZE) | CS7 | PARENB;
                mode.c_iflag |= ISTRIP;
            } else {
                mode.c_cflag = (mode.c_cflag & !PARENB & !CSIZE) | CS8;
                mode.c_iflag &= !ISTRIP;
            }
        }
        "litout" => {
            if reversed {
                mode.c_cflag = (mode.c_cflag & !CSIZE) | CS7 | PARENB;
                mode.c_iflag |= ISTRIP;
                mode.c_oflag |= OPOST;
            } else {
                mode.c_cflag = (mode.c_cflag & !PARENB & !CSIZE) | CS8;
                mode.c_iflag &= !ISTRIP;
                mode.c_oflag &= !OPOST;
            }
        }
        "raw" | "cooked" => {
            let cooked = (info.name == "raw") == reversed;
            if cooked {
                mode.c_iflag |= BRKINT | IGNPAR | ISTRIP | ICRNL | IXON;
                mode.c_oflag |= OPOST;
                mode.c_lflag |= ISIG | ICANON;
                // (VMIN is not VEOF on Linux, nor VTIME VEOL, so upstream's
                // two `#if`s that would reset them here are compiled out.)
            } else {
                mode.c_iflag = 0;
                mode.c_oflag &= !OPOST;
                mode.c_lflag &= !(ISIG | ICANON | XCASE);
                mode.c_cc[VMIN] = 1;
                mode.c_cc[VTIME] = 0;
            }
        }
        "decctlq" => {
            if reversed {
                mode.c_iflag |= IXANY;
            } else {
                mode.c_iflag &= !IXANY;
            }
        }
        "tabs" => {
            mode.c_oflag = (mode.c_oflag & !TABDLY) | if reversed { TAB3 } else { 0 };
        }
        "lcase" | "LCASE" => {
            if reversed {
                mode.c_lflag &= !XCASE;
                mode.c_iflag &= !IUCLC;
                mode.c_oflag &= !OLCUC;
            } else {
                mode.c_lflag |= XCASE;
                mode.c_iflag |= IUCLC;
                mode.c_oflag |= OLCUC;
            }
        }
        "crt" => mode.c_lflag |= ECHOE | ECHOCTL | ECHOKE,
        "dec" => {
            mode.c_cc[VINTR] = 3;
            mode.c_cc[VERASE] = 127;
            mode.c_cc[VKILL] = 21;
            mode.c_lflag |= ECHOE | ECHOCTL | ECHOKE;
            mode.c_iflag &= !IXANY;
        }
        // Every combination in the table is named above.
        _ => {}
    }
    true
}

/// `sane_mode`: every control character to its default, every flag to its
/// `sane` setting.
fn sane_mode(mode: &mut Termios) {
    for info in CONTROL_INFO {
        set_cc(mode, info.offset, info.saneval);
    }
    for info in MODE_INFO {
        if info.flags & NO_SETATTR != 0 {
            continue;
        }
        if info.flags & SANE_SET != 0 {
            if let Some(bits) = mode_type_flag(info.kind, mode) {
                *bits = (*bits & !info.mask) | info.bits;
            }
        } else if info.flags & SANE_UNSET != 0 {
            if let Some(bits) = mode_type_flag(info.kind, mode) {
                *bits = *bits & !info.mask & !info.bits;
            }
        }
    }
}

/// `integer_arg`: a number in C's bases, with `b` (512) and `B` (1024)
/// suffixes, at most `max` -- or gnulib's diagnostic.
fn integer_arg(s: &[u8], max: u64) -> Result<u64, String> {
    coreutils::xnum::xnumtoumax(s, 0, 0, max, Some(b"bB"), "invalid integer argument")
}

/// `set_control_char`: `^c`, `^?`, `^-`, `undef`, a single character, or a
/// number -- or for `min` and `time`, only a number.
fn set_control_char(info: &ControlInfo, arg: &[u8], mode: &mut Termios) -> Result<(), String> {
    let value: u64 = if info.name == "min" || info.name == "time" {
        integer_arg(arg, 255)?
    } else if arg.len() <= 1 {
        u64::from(arg.first().copied().unwrap_or(0))
    } else if arg == b"^-" || arg == b"undef" {
        0
    } else if arg.first() == Some(&b'^') {
        match arg.get(1) {
            Some(b'?') => 127,
            Some(&ch) => u64::from(ch & !0o140),
            None => 0,
        }
    } else {
        integer_arg(arg, 255)?
    };
    // `cc_t`: the low byte, which `integer_arg`'s limit has kept it within.
    set_cc(mode, info.offset, u8::try_from(value & 0xff).unwrap_or(0));
    Ok(())
}

/// `strtoul (s, &p, 16)`, glibc's: the value, the bytes it spans, and whether
/// it overflowed (`ERANGE`). Leading C spaces, a sign -- `-` negates, modulo
/// 2^64 -- and an optional `0x`; with no digits after the `0x`, it is the `0`
/// alone.
fn strtoul16(s: &[u8]) -> (u64, usize, bool) {
    let mut i = s
        .iter()
        .take_while(|&&b| matches!(b, b' ' | 0x09..=0x0d))
        .count();
    let negative = s.get(i) == Some(&b'-');
    if matches!(s.get(i), Some(b'-' | b'+')) {
        i = i.saturating_add(1);
    }
    let mut start = i;
    if s.get(i) == Some(&b'0')
        && matches!(s.get(i.saturating_add(1)), Some(b'x' | b'X'))
        && s.get(i.saturating_add(2))
            .is_some_and(u8::is_ascii_hexdigit)
    {
        start = i.saturating_add(2);
    }
    let mut value: u64 = 0;
    let mut overflow = false;
    let mut end = start;
    while let Some(d) = s.get(end).and_then(|&b| char::from(b).to_digit(16)) {
        match value
            .checked_mul(16)
            .and_then(|v| v.checked_add(u64::from(d)))
        {
            Some(v) => value = v,
            None => overflow = true,
        }
        end = end.saturating_add(1);
    }
    if end == start {
        // No conversion: the end is the start of the whole string.
        return (0, 0, false);
    }
    if overflow {
        return (u64::MAX, end, true);
    }
    (
        if negative {
            value.wrapping_neg()
        } else {
            value
        },
        end,
        false,
    )
}

/// `recover_mode`: a record from `-g`'s output -- four flag words and every
/// control character, in hexadecimal, separated by `:`.
fn recover_mode(arg: &[u8], mode: &mut Termios) -> bool {
    let mut rest = arg;
    let mut flags = [0u32; 4];
    for flag in &mut flags {
        let (value, used, erange) = strtoul16(rest);
        if erange || used == 0 || rest.get(used) != Some(&b':') {
            return false;
        }
        let Ok(v) = u32::try_from(value) else {
            return false;
        };
        *flag = v;
        rest = rest.get(used.saturating_add(1)..).unwrap_or_default();
    }
    let mut cc = [0u8; termios::NCCS];
    for (i, slot) in cc.iter_mut().enumerate() {
        let (value, used, erange) = strtoul16(rest);
        let delim = if i.saturating_add(1) < termios::NCCS {
            Some(&b':')
        } else {
            None
        };
        if erange || used == 0 || rest.get(used) != delim {
            return false;
        }
        let Ok(v) = u8::try_from(value) else {
            return false;
        };
        *slot = v;
        rest = rest.get(used.saturating_add(1)..).unwrap_or_default();
    }
    // Only now, as upstream assigns the flags before reading the characters
    // but returns false without having done either visibly: its `mode` is
    // the caller's working copy, written in full on success.
    mode.c_iflag = flags[0];
    mode.c_oflag = flags[1];
    mode.c_cflag = flags[2];
    mode.c_lflag = flags[3];
    mode.c_cc = cc;
    true
}

/// `eq_mode`: whether two records say the same, speeds included.
fn eq_mode(a: &Termios, b: &Termios) -> bool {
    a.c_iflag == b.c_iflag
        && a.c_oflag == b.c_oflag
        && a.c_cflag == b.c_cflag
        && a.c_lflag == b.c_lflag
        && a.c_line == b.c_line
        && a.c_cc == b.c_cc
        && termios::input_speed(a) == termios::input_speed(b)
        && termios::output_speed(a) == termios::output_speed(b)
}

// ---------------------------------------------------------------------------
// Printing a record
// ---------------------------------------------------------------------------

/// `visible`: a control character as `stty -a` shows it -- `^C`, `^?`, and
/// `M-` before the same forms of a byte with its top bit set.
fn visible(ch: u8) -> String {
    match ch {
        0 => "<undef>".to_string(),
        // `ch + 64`, which for these is setting the 0x40 bit.
        1..=31 => format!("^{}", char::from(ch | 0x40)),
        32..=126 => char::from(ch).to_string(),
        127 => "^?".to_string(),
        // `ch - 128 + 64`.
        128..=159 => format!("M-^{}", char::from((ch & 0x7f) | 0x40)),
        // `ch - 128`.
        160..=254 => format!("M-{}", char::from(ch & 0x7f)),
        255 => "M-^?".to_string(),
    }
}

/// What a run printed, in order: standard output, and the warnings that
/// upstream's `error (0, …)` writes between it -- after flushing standard
/// output, so a terminal sees them in the order they happened.
#[derive(Debug, PartialEq, Eq)]
enum Piece {
    Out(Vec<u8>),
    Warn(String),
}

/// The output, wrapped as `wrapf` wraps it: each item on the current line,
/// separated by a space, unless it would reach `max_col`.
struct Wrap {
    out: Vec<u8>,
    max_col: i64,
    current_col: i64,
    /// What came before the last warning, and the warnings.
    pieces: Vec<Piece>,
}

impl Wrap {
    fn new(max_col: i64) -> Self {
        Wrap {
            out: Vec::new(),
            max_col,
            current_col: 0,
            pieces: Vec::new(),
        }
    }

    /// `error (0, 0, …)`: a warning, after whatever was printed before it.
    fn warn(&mut self, message: String) {
        if !self.out.is_empty() {
            self.pieces.push(Piece::Out(std::mem::take(&mut self.out)));
        }
        self.pieces.push(Piece::Warn(message));
    }

    /// Everything, in order.
    fn finish(mut self) -> Vec<Piece> {
        if !self.out.is_empty() {
            self.pieces.push(Piece::Out(std::mem::take(&mut self.out)));
        }
        self.pieces
    }

    /// `wrapf`.
    fn item(&mut self, text: &str) {
        let len = i64::try_from(text.len()).unwrap_or(i64::MAX);
        if self.current_col > 0 {
            if self.max_col.saturating_sub(self.current_col) <= len {
                self.out.push(b'\n');
                self.current_col = 0;
            } else {
                self.out.push(b' ');
                self.current_col = self.current_col.saturating_add(1);
            }
        }
        self.out.extend_from_slice(text.as_bytes());
        self.current_col = self.current_col.saturating_add(len);
    }

    /// `putchar ('\n')`.
    fn newline(&mut self) {
        self.out.push(b'\n');
    }
}

/// `display_speed`.
fn display_speed(w: &mut Wrap, mode: &Termios, fancy: bool) {
    let ispeed = termios::input_speed(mode);
    let ospeed = termios::output_speed(mode);
    if ispeed == 0 || ispeed == ospeed {
        let v = baud_to_value(ospeed);
        w.item(&if fancy {
            format!("speed {v} baud;")
        } else {
            format!("{v}\n")
        });
    } else {
        let (i, o) = (baud_to_value(ispeed), baud_to_value(ospeed));
        w.item(&if fancy {
            format!("ispeed {i} baud; ospeed {o} baud;")
        } else {
            format!("{i} {o}\n")
        });
    }
    if !fancy {
        w.current_col = 0;
    }
}

/// `display_changed`: the speed and line, then what differs from `sane`.
fn display_changed(w: &mut Wrap, mode: &Termios) {
    display_speed(w, mode, true);
    w.item(&format!("line = {};", mode.c_line));
    w.newline();
    w.current_col = 0;

    let mut empty_line = true;
    for info in CONTROL_INFO.iter().take_while(|i| i.name != "min") {
        if cc(mode, info.offset) == info.saneval || info.name == "flush" {
            continue;
        }
        empty_line = false;
        w.item(&format!(
            "{} = {};",
            info.name,
            visible(cc(mode, info.offset))
        ));
    }
    if mode.c_lflag & ICANON == 0 {
        w.item(&format!(
            "min = {}; time = {};\n",
            mode.c_cc[VMIN], mode.c_cc[VTIME]
        ));
    } else if !empty_line {
        w.newline();
    }
    w.current_col = 0;

    let mut empty_line = true;
    let mut prev_type = ModeType::Control;
    for info in MODE_INFO {
        if info.flags & OMIT != 0 {
            continue;
        }
        if info.kind != prev_type {
            if !empty_line {
                w.newline();
                w.current_col = 0;
                empty_line = true;
            }
            prev_type = info.kind;
        }
        let Some(bits) = mode_type_value(info.kind, mode) else {
            continue;
        };
        let mask = if info.mask != 0 { info.mask } else { info.bits };
        if bits & mask == info.bits {
            if info.flags & SANE_UNSET != 0 {
                w.item(info.name);
                empty_line = false;
            }
        } else if info.flags & (SANE_SET | REV) == SANE_SET | REV {
            w.item(&format!("-{}", info.name));
            empty_line = false;
        }
    }
    if !empty_line {
        w.newline();
    }
    w.current_col = 0;
}

/// `display_all`: every setting.
///
/// # Errors
///
/// The terminal's size could not be read for a reason other than having none.
fn display_all(
    w: &mut Wrap,
    mode: &Termios,
    term: &mut dyn Terminal,
    device: &[u8],
) -> Result<(), Refusal> {
    display_speed(w, mode, true);
    display_window_size(w, term, true, device)?;
    w.item(&format!("line = {};", mode.c_line));
    w.newline();
    w.current_col = 0;

    for info in CONTROL_INFO.iter().take_while(|i| i.name != "min") {
        if info.name == "flush" {
            continue;
        }
        w.item(&format!(
            "{} = {};",
            info.name,
            visible(cc(mode, info.offset))
        ));
    }
    w.item(&format!(
        "min = {}; time = {};",
        mode.c_cc[VMIN], mode.c_cc[VTIME]
    ));
    if w.current_col != 0 {
        w.newline();
    }
    w.current_col = 0;

    let mut prev_type = ModeType::Control;
    for info in MODE_INFO {
        if info.flags & OMIT != 0 {
            continue;
        }
        if info.kind != prev_type {
            w.newline();
            w.current_col = 0;
            prev_type = info.kind;
        }
        let Some(bits) = mode_type_value(info.kind, mode) else {
            continue;
        };
        let mask = if info.mask != 0 { info.mask } else { info.bits };
        if bits & mask == info.bits {
            w.item(info.name);
        } else if info.flags & REV != 0 {
            w.item(&format!("-{}", info.name));
        }
    }
    w.newline();
    w.current_col = 0;
    Ok(())
}

/// `display_recoverable`: `-g`'s one line.
fn display_recoverable(mode: &Termios) -> String {
    let mut s = format!(
        "{:x}:{:x}:{:x}:{:x}",
        mode.c_iflag, mode.c_oflag, mode.c_cflag, mode.c_lflag
    );
    for ch in mode.c_cc {
        s.push_str(&format!(":{ch:x}"));
    }
    s.push('\n');
    s
}

// ---------------------------------------------------------------------------
// The command line
// ---------------------------------------------------------------------------

/// Which display upstream's flags ask for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Output {
    Changed,
    All,
    Recoverable,
}

/// What `main`'s `getopt_long` loop leaves behind.
#[derive(Debug, PartialEq, Eq)]
struct Scan {
    output: Output,
    verbose: bool,
    recoverable: bool,
    file: Option<OsString>,
    dev_debug: bool,
    /// Whether every word the loop handed back was `drain` or `-drain`.
    noargs: bool,
    /// argv after the loop: `None` for each word it consumed.
    settings: Vec<Option<OsString>>,
}

/// A way the first pass ends early.
#[derive(Debug, PartialEq, Eq)]
enum Early {
    Help,
    Version,
    /// `error (EXIT_FAILURE, 0, …)`.
    Fail(String),
}

/// `main`'s first pass: `getopt_long (argc - argi, argv + argi, "-agF:", …)`
/// with errors silenced, restarted after each word it does not recognise.
///
/// Every word the loop hands back as an operand or an error is a setting: it
/// stays in argv for the second pass, makes `noargs` false unless it is
/// `drain` or `-drain`, and `getopt` starts again after it, with it as the
/// new `argv[0]`. The words a recognised option used are erased. At a `--`
/// the loop is over -- `getopt` returns -1 there -- and neither the `--` nor
/// anything after it is erased or looked at.
fn scan(args: &[OsString]) -> Result<Scan, Early> {
    let mut found = Scan {
        output: Output::Changed,
        verbose: false,
        recoverable: false,
        file: None,
        dev_debug: false,
        noargs: true,
        settings: args.iter().cloned().map(Some).collect(),
    };
    // The first word of this run of `getopt`: upstream's `argi + 1`.
    let mut start = 0usize;
    'runs: while let Some(window) = args.get(start..) {
        let mut parser = STTY.parse(window, SHORT_OPTIONS, LONG_OPTIONS);
        // The first word of the window not yet erased: upstream's `opti - 1`.
        let mut opti = 0usize;
        loop {
            let Some(item) = parser.next() else {
                break 'runs;
            };
            if parser.stopped() && matches!(item, Ok(Opt::Operand(_))) {
                // Past a `--`: upstream's `getopt` has returned -1.
                break 'runs;
            }
            match item {
                Ok(Opt::Short(b'a', _) | Opt::Long("all", _)) => {
                    found.verbose = true;
                    found.output = Output::All;
                }
                Ok(Opt::Short(b'g', _) | Opt::Long("save", _)) => {
                    found.recoverable = true;
                    found.output = Output::Recoverable;
                }
                Ok(Opt::Short(b'F', value) | Opt::Long("file", value)) => {
                    if found.file.is_some() {
                        return Err(Early::Fail("only one device may be specified".to_string()));
                    }
                    found.file = value;
                }
                Ok(Opt::Long("-debug", _)) => found.dev_debug = true,
                Ok(Opt::Long("help", _)) => return Err(Early::Help),
                Ok(Opt::Long("version", _)) => return Err(Early::Version),
                // `default:` -- an operand, or any option getopt refused.
                _ => {
                    let at = start.saturating_add(opti);
                    let word = args.get(at).map(|w| coreutils::quote::os_bytes(w));
                    if !matches!(word.as_deref(), Some(b"-drain" | b"drain")) {
                        found.noargs = false;
                    }
                    start = at.saturating_add(1);
                    continue 'runs;
                }
            }
            // "Clear fully-parsed arguments, so they don't confuse the 2nd pass."
            let optind = parser.optind();
            while opti < optind {
                if let Some(slot) = found.settings.get_mut(start.saturating_add(opti)) {
                    *slot = None;
                }
                opti = opti.saturating_add(1);
            }
        }
    }
    if found.verbose && found.recoverable {
        return Err(Early::Fail(
            "the options for verbose and stty-readable output styles are\nmutually exclusive"
                .to_string(),
        ));
    }
    if !found.noargs && (found.verbose || found.recoverable) {
        return Err(Early::Fail(
            "when specifying an output style, modes may not be set".to_string(),
        ));
    }
    Ok(found)
}

/// How `apply_settings` refuses: a diagnostic, and whether upstream's `usage
/// (EXIT_FAILURE)` -- the `Try …` line -- follows it.
#[derive(Debug, PartialEq, Eq)]
struct Refusal {
    message: String,
    usage: bool,
}

impl Refusal {
    fn usage(message: String) -> Self {
        Refusal {
            message,
            usage: true,
        }
    }

    fn fatal(message: String) -> Self {
        Refusal {
            message,
            usage: false,
        }
    }
}

/// What the second pass needs of the terminal, so it can run against a fake
/// one in the tests: the window size, and the screen's width for wrapping.
trait Terminal {
    /// `ioctl (fd, TIOCGWINSZ)`: the whole `struct winsize`, or the `errno`.
    fn window_size(&mut self) -> Result<WinSize, i32>;
    /// `ioctl (fd, TIOCSWINSZ)`.
    fn set_window_size(&mut self, size: WinSize) -> Result<(), i32>;
    /// `screen_columns ()`.
    fn screen_columns(&mut self) -> i64;
}

/// The state `apply_settings` keeps between settings and leaves for `main`.
struct Applied {
    /// Whether a change needs `tcsetattr`.
    require_set_attr: bool,
    /// `drain`'s answer: `TCSADRAIN` unless `-drain` said `TCSANOW`.
    tcsetattr_options: i32,
    last_ibaud: Option<u32>,
    last_obaud: Option<u32>,
}

impl Applied {
    fn new() -> Self {
        Applied {
            require_set_attr: false,
            tcsetattr_options: termios::TCSADRAIN,
            last_ibaud: None,
            last_obaud: None,
        }
    }
}

const EINVAL: i32 = 22;
/// `EBADF`, for the terminal the trial run does not have.
const EBADF: i32 = 9;

/// The trial run's terminal: there is none yet, and nothing in the trial run
/// asks it anything -- `size`, `speed`, `rows` and `cols` all stop short of the
/// terminal when only checking.
struct NoTerminal;

impl Terminal for NoTerminal {
    fn window_size(&mut self) -> Result<WinSize, i32> {
        Err(EBADF)
    }
    fn set_window_size(&mut self, _size: WinSize) -> Result<(), i32> {
        Err(EBADF)
    }
    fn screen_columns(&mut self) -> i64 {
        80
    }
}

/// `apply_settings`: the second pass, `checking` for the trial run against a
/// blank record that refuses a bad command line before anything is changed.
///
/// Output it prints (`size`, `speed`) goes to `w`; on the trial run there is
/// none.
#[allow(clippy::too_many_lines)]
fn apply_settings(
    checking: bool,
    device: &[u8],
    settings: &[Option<OsString>],
    mode: &mut Termios,
    state: &mut Applied,
    term: &mut dyn Terminal,
    w: &mut Wrap,
) -> Result<(), Refusal> {
    let quote = |b: &[u8]| coreutils::quote::quote(b);
    let mut k = 0usize;
    while k < settings.len() {
        let Some(word) = settings.get(k).cloned().flatten() else {
            k = k.saturating_add(1);
            continue;
        };
        let whole = coreutils::quote::os_bytes(&word).into_owned();
        let reversed = whole.first() == Some(&b'-');
        let arg: &[u8] = if reversed {
            whole.get(1..).unwrap_or_default()
        } else {
            &whole
        };
        // `check_argument`: the next word must be there and not erased.
        let next = settings.get(k.saturating_add(1)).cloned().flatten();
        let missing = || Refusal::usage(format!("missing argument to {}", quote(arg)));

        if arg == b"drain" {
            state.tcsetattr_options = if reversed {
                termios::TCSANOW
            } else {
                termios::TCSADRAIN
            };
            k = k.saturating_add(1);
            continue;
        }
        let mut match_found = false;
        let mut not_set_attr = false;
        if let Some(info) = MODE_INFO.iter().find(|i| i.name.as_bytes() == arg) {
            if info.flags & NO_SETATTR == 0 {
                match_found = set_mode(info, reversed, mode);
                state.require_set_attr = true;
            } else {
                match_found = true;
                not_set_attr = true;
            }
        }
        if !match_found && reversed {
            return Err(Refusal::usage(format!(
                "invalid argument {}",
                quote(&whole)
            )));
        }
        if !match_found {
            if let Some(info) = CONTROL_INFO.iter().find(|i| i.name.as_bytes() == arg) {
                let Some(value) = next.as_ref() else {
                    return Err(missing());
                };
                match_found = true;
                k = k.saturating_add(1);
                set_control_char(info, &coreutils::quote::os_bytes(value), mode)
                    .map_err(Refusal::fatal)?;
                state.require_set_attr = true;
            }
        }
        if !match_found || not_set_attr {
            match arg {
                b"ispeed" | b"ospeed" => {
                    let Some(value) = next.as_ref() else {
                        return Err(missing());
                    };
                    k = k.saturating_add(1);
                    let value = coreutils::quote::os_bytes(value).into_owned();
                    let input = arg == b"ispeed";
                    let Some(baud) = string_to_baud(&value) else {
                        let which = if input { "ispeed" } else { "ospeed" };
                        return Err(Refusal::usage(format!("invalid {which} {}", quote(&value))));
                    };
                    set_speed(input, !input, baud, &value, mode, state)?;
                    if !checking {
                        state.require_set_attr = true;
                    }
                }
                b"rows" | b"cols" | b"columns" => {
                    let Some(value) = next.as_ref() else {
                        return Err(missing());
                    };
                    k = k.saturating_add(1);
                    if !checking {
                        let n = integer_arg(&coreutils::quote::os_bytes(value), 2_147_483_647)
                            .map_err(Refusal::fatal)?;
                        let n = i64::try_from(n).unwrap_or(i64::MAX);
                        let (rows, cols) = if arg == b"rows" { (n, -1) } else { (-1, n) };
                        set_window_size(term, rows, cols, device)?;
                    }
                }
                b"size" => {
                    if !checking {
                        w.max_col = term.screen_columns();
                        w.current_col = 0;
                        display_window_size(w, term, false, device)?;
                    }
                }
                b"line" => {
                    let Some(value) = next.as_ref() else {
                        return Err(missing());
                    };
                    k = k.saturating_add(1);
                    let text = coreutils::quote::os_bytes(value).into_owned();
                    let n = integer_arg(&text, u64::MAX).map_err(Refusal::fatal)?;
                    // `c_line` is a `cc_t`: C keeps the low byte.
                    mode.c_line = u8::try_from(n & 0xff).unwrap_or(0);
                    if u64::from(mode.c_line) != n {
                        w.warn(format!("invalid line discipline {}", quote(&text)));
                    }
                    state.require_set_attr = true;
                }
                b"speed" => {
                    if !checking {
                        w.max_col = term.screen_columns();
                        display_speed(w, mode, false);
                    }
                }
                _ => {
                    if let Some(baud) = string_to_baud(arg) {
                        set_speed(true, true, baud, arg, mode, state)?;
                        if !checking {
                            state.require_set_attr = true;
                        }
                    } else if recover_mode(arg, mode) {
                        state.require_set_attr = true;
                    } else {
                        return Err(Refusal::usage(format!("invalid argument {}", quote(arg))));
                    }
                }
            }
        }
        k = k.saturating_add(1);
    }
    if checking {
        check_speed(mode, state)?;
    }
    Ok(())
}

/// `set_speed`: the input speed, the output speed, or both.
fn set_speed(
    input: bool,
    output: bool,
    baud: u32,
    arg: &[u8],
    mode: &mut Termios,
    state: &mut Applied,
) -> Result<(), Refusal> {
    if input {
        state.last_ibaud = Some(baud);
        termios::set_input_speed(mode, baud).map_err(|_| {
            Refusal::fatal(format!(
                "unsupported ispeed {}",
                coreutils::quote::quoteaf(arg)
            ))
        })?;
    }
    if output {
        state.last_obaud = Some(baud);
        termios::set_output_speed(mode, baud).map_err(|_| {
            Refusal::fatal(format!(
                "unsupported ospeed {}",
                coreutils::quote::quoteaf(arg)
            ))
        })?;
    }
    Ok(())
}

/// `check_speed`: after the trial run, refuse speeds the library could not
/// keep apart -- on glibc, any two different ones.
fn check_speed(mode: &Termios, state: &Applied) -> Result<(), Refusal> {
    if let (Some(ibaud), Some(obaud)) = (state.last_ibaud, state.last_obaud) {
        if termios::input_speed(mode) != ibaud || termios::output_speed(mode) != obaud {
            return Err(Refusal::fatal(format!(
                "asymmetric input ({}), output ({}) speeds not supported",
                baud_to_value(ibaud),
                baud_to_value(obaud)
            )));
        }
    }
    Ok(())
}

/// `quotef (device_name)`, for the diagnostics that name the terminal.
fn quotef(device: &[u8]) -> String {
    coreutils::quote::quotef(device)
}

/// `set_window_size`: change what the terminal says its rows or columns are
/// (`-1` leaves one alone).
fn set_window_size(
    term: &mut dyn Terminal,
    rows: i64,
    cols: i64,
    device: &[u8],
) -> Result<(), Refusal> {
    // `memset (&win, 0, sizeof (win))` when the terminal has no size to read.
    let mut win = match term.window_size() {
        Ok(size) => size,
        Err(e) if e == EINVAL => WinSize::default(),
        Err(e) => {
            return Err(Refusal::fatal(format!(
                "{}: {}",
                quotef(device),
                strerror(e)
            )));
        }
    };
    // `win.ws_row = rows`: an `unsigned short`, so C keeps the low 16 bits.
    if rows >= 0 {
        win.rows = low_u16(rows);
    }
    if cols >= 0 {
        win.cols = low_u16(cols);
    }
    term.set_window_size(win)
        .map_err(|e| Refusal::fatal(format!("{}: {}", quotef(device), strerror(e))))
}

/// C's conversion to `unsigned short`: the low 16 bits.
fn low_u16(n: i64) -> u16 {
    let b = n.to_le_bytes();
    u16::from_le_bytes([b[0], b[1]])
}

/// `display_window_size`.
fn display_window_size(
    w: &mut Wrap,
    term: &mut dyn Terminal,
    fancy: bool,
    device: &[u8],
) -> Result<(), Refusal> {
    match term.window_size() {
        Ok(WinSize { rows, cols, .. }) => {
            if fancy {
                w.item(&format!("rows {rows}; columns {cols};"));
            } else {
                w.item(&format!("{rows} {cols}\n"));
                w.current_col = 0;
            }
            Ok(())
        }
        Err(e) if e != EINVAL => Err(Refusal::fatal(format!(
            "{}: {}",
            quotef(device),
            strerror(e)
        ))),
        Err(_) if !fancy => Err(Refusal::fatal(format!(
            "{}: no size information for this device",
            quotef(device)
        ))),
        Err(_) => Ok(()),
    }
}

/// `strerror`, from an `errno`.
fn strerror(errno: i32) -> String {
    coreutils::errmsg::strerror(&std::io::Error::from_raw_os_error(errno))
}

/// Upstream's `usage (EXIT_SUCCESS)`, without GNU's closing block of links.
const HELP: &str = r#"Usage: stty [-F DEVICE | --file=DEVICE] [SETTING]...
  or:  stty [-F DEVICE | --file=DEVICE] [-a|--all]
  or:  stty [-F DEVICE | --file=DEVICE] [-g|--save]
Print or change terminal characteristics.

Mandatory arguments to long options are mandatory for short options too.
  -a, --all          print all current settings in human-readable form
  -g, --save         print all current settings in a stty-readable form
  -F, --file=DEVICE  open and use the specified DEVICE instead of stdin
      --help        display this help and exit
      --version     output version information and exit

Optional - before SETTING indicates negation.  An * marks non-POSIX
settings.  The underlying system defines which settings are available.

Special characters:
 * discard CHAR  CHAR will toggle discarding of output
   eof CHAR      CHAR will send an end of file (terminate the input)
   eol CHAR      CHAR will end the line
 * eol2 CHAR     alternate CHAR for ending the line
   erase CHAR    CHAR will erase the last character typed
   intr CHAR     CHAR will send an interrupt signal
   kill CHAR     CHAR will erase the current line
 * lnext CHAR    CHAR will enter the next character quoted
   quit CHAR     CHAR will send a quit signal
 * rprnt CHAR    CHAR will redraw the current line
   start CHAR    CHAR will restart the output after stopping it
   stop CHAR     CHAR will stop the output
   susp CHAR     CHAR will send a terminal stop signal
 * swtch CHAR    CHAR will switch to a different shell layer
 * werase CHAR   CHAR will erase the last word typed

Special settings:
   N             set the input and output speeds to N bauds
 * cols N        tell the kernel that the terminal has N columns
 * columns N     same as cols N
 * [-]drain      wait for transmission before applying settings (on by default)
   ispeed N      set the input speed to N
 * line N        use line discipline N
   min N         with -icanon, set N characters minimum for a completed read
   ospeed N      set the output speed to N
 * rows N        tell the kernel that the terminal has N rows
 * size          print the number of rows and columns according to the kernel
   speed         print the terminal speed
   time N        with -icanon, set read timeout of N tenths of a second

Control settings:
   [-]clocal     disable modem control signals
   [-]cread      allow input to be received
 * [-]crtscts    enable RTS/CTS handshaking
   csN           set character size to N bits, N in [5..8]
   [-]cstopb     use two stop bits per character (one with '-')
   [-]hup        send a hangup signal when the last process closes the tty
   [-]hupcl      same as [-]hup
   [-]parenb     generate parity bit in output and expect parity bit in input
   [-]parodd     set odd parity (or even parity with '-')
 * [-]cmspar     use "stick" (mark/space) parity

Input settings:
   [-]brkint     breaks cause an interrupt signal
   [-]icrnl      translate carriage return to newline
   [-]ignbrk     ignore break characters
   [-]igncr      ignore carriage return
   [-]ignpar     ignore characters with parity errors
 * [-]imaxbel    beep and do not flush a full input buffer on a character
   [-]inlcr      translate newline to carriage return
   [-]inpck      enable input parity checking
   [-]istrip     clear high (8th) bit of input characters
 * [-]iutf8      assume input characters are UTF-8 encoded
 * [-]iuclc      translate uppercase characters to lowercase
 * [-]ixany      let any character restart output, not only start character
   [-]ixoff      enable sending of start/stop characters
   [-]ixon       enable XON/XOFF flow control
   [-]parmrk     mark parity errors (with a 255-0-character sequence)
   [-]tandem     same as [-]ixoff

Output settings:
 * bsN           backspace delay style, N in [0..1]
 * crN           carriage return delay style, N in [0..3]
 * ffN           form feed delay style, N in [0..1]
 * nlN           newline delay style, N in [0..1]
 * [-]ocrnl      translate carriage return to newline
 * [-]ofdel      use delete characters for fill instead of NUL characters
 * [-]ofill      use fill (padding) characters instead of timing for delays
 * [-]olcuc      translate lowercase characters to uppercase
 * [-]onlcr      translate newline to carriage return-newline
 * [-]onlret     newline performs a carriage return
 * [-]onocr      do not print carriage returns in the first column
   [-]opost      postprocess output
 * tabN          horizontal tab delay style, N in [0..3]
 * tabs          same as tab0
 * -tabs         same as tab3
 * vtN           vertical tab delay style, N in [0..1]

Local settings:
   [-]crterase   echo erase characters as backspace-space-backspace
 * crtkill       kill all line by obeying the echoprt and echoe settings
 * -crtkill      kill all line by obeying the echoctl and echok settings
 * [-]ctlecho    echo control characters in hat notation ('^c')
   [-]echo       echo input characters
 * [-]echoctl    same as [-]ctlecho
   [-]echoe      same as [-]crterase
   [-]echok      echo a newline after a kill character
 * [-]echoke     same as [-]crtkill
   [-]echonl     echo newline even if not echoing other characters
 * [-]echoprt    echo erased characters backward, between '\' and '/'
 * [-]extproc    enable "LINEMODE"; useful with high latency links
 * [-]flusho     discard output
   [-]icanon     enable special characters: erase, kill, werase, rprnt
   [-]iexten     enable non-POSIX special characters
   [-]isig       enable interrupt, quit, and suspend special characters
   [-]noflsh     disable flushing after interrupt and quit special characters
 * [-]prterase   same as [-]echoprt
 * [-]tostop     stop background jobs that try to write to the terminal
 * [-]xcase      with icanon, escape with '\' for uppercase characters

Combination settings:
 * [-]LCASE      same as [-]lcase
   cbreak        same as -icanon
   -cbreak       same as icanon
   cooked        same as brkint ignpar istrip icrnl ixon opost isig
                 icanon, eof and eol characters to their default values
   -cooked       same as raw
   crt           same as echoe echoctl echoke
   dec           same as echoe echoctl echoke -ixany intr ^c erase 0177
                 kill ^u
 * [-]decctlq    same as [-]ixany
   ek            erase and kill characters to their default values
   evenp         same as parenb -parodd cs7
   -evenp        same as -parenb cs8
 * [-]lcase      same as xcase iuclc olcuc
   litout        same as -parenb -istrip -opost cs8
   -litout       same as parenb istrip opost cs7
   nl            same as -icrnl -onlcr
   -nl           same as icrnl -inlcr -igncr onlcr -ocrnl -onlret
   oddp          same as parenb parodd cs7
   -oddp         same as -parenb cs8
   [-]parity     same as [-]evenp
   pass8         same as -parenb -istrip cs8
   -pass8        same as parenb istrip cs7
   raw           same as -ignbrk -brkint -ignpar -parmrk -inpck -istrip
                 -inlcr -igncr -icrnl -ixon -ixoff -icanon -opost
                 -isig -iuclc -ixany -imaxbel -xcase min 1 time 0
   -raw          same as cooked
   sane          same as cread -ignbrk brkint -inlcr -igncr icrnl
                 icanon iexten echo echoe echok -echonl -noflsh
                 -ixoff -iutf8 -iuclc -ixany imaxbel -xcase -olcuc -ocrnl
                 opost -ofill onlcr -onocr -onlret nl0 cr0 tab0 bs0 vt0 ff0
                 isig -tostop -ofdel -echoprt echoctl echoke -extproc -flusho,
                 all special characters to their default values

Handle the tty line connected to standard input.  Without arguments,
prints baud rate, line discipline, and deviations from stty sane.  In
settings, CHAR is taken literally, or coded as in ^c, 0x37, 0177 or
127; special values ^- or undef used to disable special characters.
"#;

#[cfg(unix)]
mod imp {
    use super::{
        Applied, Early, HELP, NoTerminal, Output, Piece, Refusal, Terminal, WinSize, Wrap,
        apply_settings, display_all, display_changed, display_recoverable, eq_mode, quotef, scan,
        strerror,
    };
    use coreutils::diag;
    use coreutils::quote::{os_bytes, os_from_bytes};
    use coreutils::stdfd::{self, Stream};
    use libcall::termios::{self, Termios};
    use std::ffi::OsString;
    use std::io::Write;
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::OpenOptionsExt;
    use std::process::ExitCode;

    /// The terminal `stty` works on, and standard output for the width.
    struct Real {
        fd: i32,
    }

    impl Terminal for Real {
        fn window_size(&mut self) -> Result<WinSize, i32> {
            libcall::pty::window_size(self.fd)
        }

        fn set_window_size(&mut self, size: WinSize) -> Result<(), i32> {
            libcall::pty::set_window_size(self.fd, size)
        }

        fn screen_columns(&mut self) -> i64 {
            if let Ok(w) = libcall::pty::window_size(1) {
                if w.cols > 0 {
                    return i64::from(w.cols);
                }
            }
            let Some(text) = std::env::var_os("COLUMNS") else {
                return 80;
            };
            let (n, status) = coreutils::xnum::xstrtoimax_base(&os_bytes(&text), 0, Some(b""));
            if matches!(status, coreutils::xnum::Status::Ok) && n > 0 && n <= i64::from(i32::MAX) {
                n
            } else {
                80
            }
        }
    }

    /// Write a run's output and warnings in the order they happened.
    fn emit(out: &mut Stream, pieces: Vec<Piece>) {
        for piece in pieces {
            match piece {
                Piece::Out(bytes) => {
                    // Deliberately unread: a failed write is `Stream`'s to
                    // remember and `close_stdout`'s to report, once.
                    let _ = out.write_all(&bytes);
                }
                // `diag!` flushes standard output first, as `error` does.
                Piece::Warn(message) => diag!("stty: {message}"),
            }
        }
    }

    /// Print a refusal as upstream does, and give its status.
    fn refuse(r: &Refusal) -> u8 {
        diag!("stty: {}", r.message);
        if r.usage {
            diag!("Try 'stty --help' for more information.");
        }
        1
    }

    fn run(args: &[OsString], out: &mut Stream) -> u8 {
        let found = match scan(args) {
            Ok(found) => found,
            Err(Early::Help) => {
                let _ = out.write_all(HELP.as_bytes());
                return 0;
            }
            Err(Early::Version) => {
                let _ = out.write_all(b"stty (SlateOS coreutils) 0.1.0\n");
                return 0;
            }
            Err(Early::Fail(message)) => {
                diag!("stty: {message}");
                return 1;
            }
        };
        let device: Vec<u8> = found
            .file
            .as_ref()
            .map_or_else(|| b"standard input".to_vec(), |f| os_bytes(f).into_owned());

        // The trial run, against a blank record: a bad command line is refused
        // before the terminal is opened.
        if !found.noargs && !found.verbose && !found.recoverable {
            let mut check_mode = Termios::default();
            let mut state = Applied::new();
            let mut w = Wrap::new(80);
            let checked = apply_settings(
                true,
                &device,
                &found.settings,
                &mut check_mode,
                &mut state,
                &mut NoTerminal,
                &mut w,
            );
            // The trial run's warnings are printed too: upstream prints an
            // invalid line discipline once in each pass.
            emit(out, w.finish());
            if let Err(r) = checked {
                return refuse(&r);
            }
        }

        // `-F`: open the device, without waiting for a carrier, then make it
        // blocking again.
        let opened;
        let fd = match &found.file {
            Some(name) => {
                match std::fs::OpenOptions::new()
                    .read(true)
                    .custom_flags(termios::O_NONBLOCK)
                    .open(os_from_bytes(&os_bytes(name)))
                {
                    Ok(f) => {
                        if let Err(e) = termios::clear_nonblocking(f.as_raw_fd()) {
                            diag!(
                                "stty: {}: couldn't reset non-blocking mode: {}",
                                quotef(&device),
                                strerror(e)
                            );
                            return 1;
                        }
                        opened = f;
                        opened.as_raw_fd()
                    }
                    Err(e) => {
                        diag!(
                            "stty: {}: {}",
                            quotef(&device),
                            coreutils::errmsg::strerror(&e)
                        );
                        return 1;
                    }
                }
            }
            None => 0,
        };
        let mut term = Real { fd };

        let mut mode = match termios::get_attr(fd) {
            Ok(m) => m,
            Err(e) => {
                diag!("stty: {}: {}", quotef(&device), strerror(e));
                return 1;
            }
        };

        if found.verbose || found.recoverable || found.noargs {
            let mut w = Wrap::new(term.screen_columns());
            let shown = match found.output {
                Output::Changed => {
                    display_changed(&mut w, &mode);
                    Ok(())
                }
                Output::All => display_all(&mut w, &mode, &mut term, &device),
                Output::Recoverable => {
                    w.out
                        .extend_from_slice(display_recoverable(&mode).as_bytes());
                    Ok(())
                }
            };
            emit(out, w.finish());
            return match shown {
                Ok(()) => 0,
                Err(r) => refuse(&r),
            };
        }

        let mut state = Applied::new();
        let mut w = Wrap::new(80);
        let applied = apply_settings(
            false,
            &device,
            &found.settings,
            &mut mode,
            &mut state,
            &mut term,
            &mut w,
        );
        emit(out, w.finish());
        if let Err(r) = applied {
            return refuse(&r);
        }

        if state.require_set_attr {
            if let Err(e) = termios::set_attr(fd, state.tcsetattr_options, &mode) {
                diag!("stty: {}: {}", quotef(&device), strerror(e));
                return 1;
            }
            let new_mode = match termios::get_attr(fd) {
                Ok(m) => m,
                Err(e) => {
                    diag!("stty: {}: {}", quotef(&device), strerror(e));
                    return 1;
                }
            };
            if !eq_mode(&mode, &new_mode) {
                if found.dev_debug {
                    diag!("stty: indx: mode: actual mode");
                    for (i, (old, new)) in super::record_bytes(&mode)
                        .iter()
                        .zip(super::record_bytes(&new_mode))
                        .enumerate()
                    {
                        let mark = if *old == new { "" } else { " *" };
                        diag!("stty: 0x{i:02x}, 0x{old:02x}: 0x{new:02x}{mark}");
                    }
                }
                diag!(
                    "stty: {}: unable to perform all requested operations",
                    quotef(&device)
                );
                return 1;
            }
        }
        0
    }

    pub fn main() -> ExitCode {
        stdfd::restore();
        let args: Vec<OsString> = std::env::args_os().skip(1).collect();
        let mut out = Stream::stdout();
        let status = run(&args, &mut out);
        stdfd::close_stdout("stty", out, ExitCode::from(status))
    }
}

/// A record's bytes, in C's layout, for `---debug`'s comparison.
fn record_bytes(t: &Termios) -> Vec<u8> {
    let mut b = Vec::with_capacity(60);
    for word in [t.c_iflag, t.c_oflag, t.c_cflag, t.c_lflag] {
        b.extend_from_slice(&word.to_le_bytes());
    }
    b.push(t.c_line);
    b.extend_from_slice(&t.c_cc);
    // The three bytes of padding before `c_ispeed`, which `memcmp` sees too.
    b.extend_from_slice(&[0, 0, 0]);
    b.extend_from_slice(&t.c_ispeed.to_le_bytes());
    b.extend_from_slice(&t.c_ospeed.to_le_bytes());
    b
}

#[cfg(unix)]
fn main() -> std::process::ExitCode {
    coreutils::stdfd::close_stderr(imp::main(), 1)
}

/// The host build exists only so `cargo test` runs on the developer machine,
/// which has no terminals of ours.
#[cfg(not(unix))]
fn main() -> std::process::ExitCode {
    coreutils::diag!("stty: unix-only utility; not supported on this platform");
    std::process::ExitCode::from(1)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn argv(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    fn settings(found: &Scan) -> Vec<String> {
        found
            .settings
            .iter()
            .flatten()
            .map(|s| s.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn options_and_settings_mix() {
        let f = scan(&argv(&["-F", "/dev/tty", "-echo", "raw"])).unwrap();
        assert_eq!(f.file.as_deref(), Some(std::ffi::OsStr::new("/dev/tty")));
        assert!(!f.noargs);
        assert_eq!(settings(&f), ["-echo", "raw"]);
        let f = scan(&argv(&["-echo", "-F", "/dev/tty", "raw"])).unwrap();
        assert_eq!(f.file.as_deref(), Some(std::ffi::OsStr::new("/dev/tty")));
        assert_eq!(settings(&f), ["-echo", "raw"]);
    }

    #[test]
    fn drain_alone_is_no_setting() {
        let f = scan(&argv(&["drain"])).unwrap();
        assert!(f.noargs);
        let f = scan(&argv(&["-drain", "-a"])).unwrap();
        assert!(f.noargs && f.verbose);
    }

    #[test]
    fn output_styles_refuse_settings_and_each_other() {
        assert_eq!(
            scan(&argv(&["-a", "-g"])).unwrap_err(),
            Early::Fail(
                "the options for verbose and stty-readable output styles are\nmutually exclusive"
                    .to_string()
            )
        );
        assert_eq!(
            scan(&argv(&["-a", "echo"])).unwrap_err(),
            Early::Fail("when specifying an output style, modes may not be set".to_string())
        );
        // A bundle with an unknown letter is a setting, its `a` taken first.
        assert_eq!(
            scan(&argv(&["-aecho"])).unwrap_err(),
            Early::Fail("when specifying an output style, modes may not be set".to_string())
        );
    }

    #[test]
    fn one_device_only() {
        assert_eq!(
            scan(&argv(&["-F", "a", "--file=b"])).unwrap_err(),
            Early::Fail("only one device may be specified".to_string())
        );
    }

    #[test]
    fn help_and_version_act_where_they_are_met() {
        assert_eq!(
            scan(&argv(&["--help", "-a", "-g"])).unwrap_err(),
            Early::Help
        );
        assert_eq!(
            scan(&argv(&["echo", "--version"])).unwrap_err(),
            Early::Version
        );
        assert_eq!(scan(&argv(&["--he"])).unwrap_err(), Early::Help);
    }

    #[test]
    fn a_double_dash_ends_the_first_pass_before_any_setting() {
        // `getopt` stops at the `--`, so `echo` is never seen and nothing
        // made `noargs` false: the settings are printed, not changed.
        let f = scan(&argv(&["--", "echo"])).unwrap();
        assert!(f.noargs);
        // After a setting, the `--` stays for the second pass.
        let f = scan(&argv(&["echo", "--", "-echo"])).unwrap();
        assert!(!f.noargs);
        assert_eq!(settings(&f), ["echo", "--", "-echo"]);
    }

    #[test]
    fn a_missing_device_is_a_setting() {
        let f = scan(&argv(&["-F"])).unwrap();
        assert!(!f.noargs);
        assert_eq!(settings(&f), ["-F"]);
        assert!(f.file.is_none());
    }

    #[test]
    fn debug_is_three_dashes() {
        assert!(scan(&argv(&["---debug"])).unwrap().dev_debug);
    }

    #[test]
    fn visible_characters() {
        assert_eq!(visible(0), "<undef>");
        assert_eq!(visible(3), "^C");
        assert_eq!(visible(b'a'), "a");
        assert_eq!(visible(127), "^?");
        assert_eq!(visible(128), "M-^@");
        assert_eq!(visible(128 + 65), "M-A");
        assert_eq!(visible(255), "M-^?");
    }

    #[test]
    fn control_characters_in_every_spelling() {
        let mut t = Termios::default();
        let intr = &CONTROL_INFO[0];
        for (arg, want) in [
            (&b"^c"[..], 3u8),
            (b"^C", 3),
            (b"^?", 127),
            (b"^-", 0),
            (b"undef", 0),
            (b"x", b'x'),
            (b"", 0),
            (b"0x7f", 127),
            (b"0177", 127),
            (b"127", 127),
            (b"^abc", 1),
        ] {
            set_control_char(intr, arg, &mut t).unwrap();
            assert_eq!(t.c_cc[0], want, "{}", String::from_utf8_lossy(arg));
        }
        assert!(set_control_char(intr, b"256", &mut t).is_err());
        let min = CONTROL_INFO.iter().find(|i| i.name == "min").unwrap();
        set_control_char(min, b"5", &mut t).unwrap();
        assert_eq!(t.c_cc[VMIN], 5);
        assert!(set_control_char(min, b"x", &mut t).is_err());
    }

    #[test]
    fn sane_resets_flags_and_characters() {
        let mut t = Termios {
            c_iflag: 0xffff_ffff,
            c_oflag: 0xffff_ffff,
            c_lflag: 0,
            ..Termios::default()
        };
        sane_mode(&mut t);
        assert_eq!(t.c_cc[VERASE], 127);
        assert_eq!(t.c_cc[VMIN], 1);
        assert_ne!(t.c_lflag & ICANON, 0);
        assert_eq!(t.c_iflag & IXANY, 0);
    }

    #[test]
    fn combinations() {
        let mut t = Termios::default();
        assert!(set_mode(
            MODE_INFO.iter().find(|i| i.name == "evenp").unwrap(),
            false,
            &mut t
        ));
        assert_eq!(t.c_cflag & (PARENB | CSIZE), PARENB | CS7);
        assert!(set_mode(
            MODE_INFO.iter().find(|i| i.name == "evenp").unwrap(),
            true,
            &mut t
        ));
        assert_eq!(t.c_cflag & (PARENB | CSIZE), CS8);
        // `ek` and `sane` have no reverse.
        assert!(!set_mode(
            MODE_INFO.iter().find(|i| i.name == "ek").unwrap(),
            true,
            &mut t
        ));
        assert!(set_mode(
            MODE_INFO.iter().find(|i| i.name == "raw").unwrap(),
            false,
            &mut t
        ));
        assert_eq!(t.c_iflag, 0);
        assert_eq!((t.c_cc[VMIN], t.c_cc[VTIME]), (1, 0));
        // `-raw` is `cooked`.
        assert!(set_mode(
            MODE_INFO.iter().find(|i| i.name == "raw").unwrap(),
            true,
            &mut t
        ));
        assert_ne!(t.c_lflag & ICANON, 0);
    }

    #[test]
    fn recoverable_round_trips() {
        let mut t = Termios::default();
        sane_mode(&mut t);
        t.c_cflag = 0xbf;
        let line = display_recoverable(&t);
        let mut back = Termios::default();
        assert!(recover_mode(line.trim_end().as_bytes(), &mut back));
        assert_eq!(back.c_cc, t.c_cc);
        assert_eq!(back.c_cflag, 0xbf);
        // Thirty-one characters, a flag past 32 bits, a character past 255.
        let (short, _) = line.trim_end().rsplit_once(':').unwrap();
        assert!(!recover_mode(short.as_bytes(), &mut back));
        assert!(!recover_mode(b"100000000:0:0:0", &mut back));
        let wide = format!("{short}:100");
        assert!(!recover_mode(wide.as_bytes(), &mut back));
    }

    #[test]
    fn strtoul16_is_glibcs() {
        assert_eq!(strtoul16(b"500:"), (0x500, 3, false));
        assert_eq!(strtoul16(b" 0x1F:"), (0x1f, 5, false));
        assert_eq!(strtoul16(b"0x:"), (0, 1, false));
        assert_eq!(strtoul16(b"-1:"), (u64::MAX, 2, false));
        assert_eq!(strtoul16(b":"), (0, 0, false));
        assert_eq!(strtoul16(b"fffffffffffffffff:"), (u64::MAX, 17, true));
    }

    #[test]
    fn wrapping() {
        let mut w = Wrap::new(10);
        w.item("abc");
        w.item("defg");
        w.item("hi");
        assert_eq!(w.out, b"abc defg\nhi");
    }

    struct Fake {
        size: Result<WinSize, i32>,
        set: Option<WinSize>,
        columns: i64,
    }

    impl Terminal for Fake {
        fn window_size(&mut self) -> Result<WinSize, i32> {
            self.size
        }
        fn set_window_size(&mut self, size: WinSize) -> Result<(), i32> {
            self.set = Some(size);
            Ok(())
        }
        fn screen_columns(&mut self) -> i64 {
            self.columns
        }
    }

    fn apply(
        args: &[&str],
        mode: &mut Termios,
        checking: bool,
    ) -> (Result<(), Refusal>, Applied, Vec<Piece>, Option<WinSize>) {
        let found = scan(&argv(args)).unwrap();
        let mut state = Applied::new();
        let mut fake = Fake {
            size: Ok(WinSize {
                rows: 24,
                cols: 80,
                xpixel: 640,
                ypixel: 480,
            }),
            set: None,
            columns: 80,
        };
        let mut w = Wrap::new(80);
        let r = apply_settings(
            checking,
            b"standard input",
            &found.settings,
            mode,
            &mut state,
            &mut fake,
            &mut w,
        );
        (r, state, w.finish(), fake.set)
    }

    #[test]
    fn the_second_pass_refuses_what_upstream_refuses() {
        let mut t = Termios::default();
        assert_eq!(
            apply(&["frobnicate"], &mut t, true).0,
            Err(Refusal::usage("invalid argument ‘frobnicate’".to_string()))
        );
        assert_eq!(
            apply(&["-sane"], &mut t, true).0,
            Err(Refusal::usage("invalid argument ‘-sane’".to_string()))
        );
        assert_eq!(
            apply(&["intr"], &mut t, true).0,
            Err(Refusal::usage("missing argument to ‘intr’".to_string()))
        );
        assert_eq!(
            apply(&["ispeed", "123"], &mut t, true).0,
            Err(Refusal::usage("invalid ispeed ‘123’".to_string()))
        );
    }

    #[test]
    fn size_and_rows_reach_the_terminal_only_for_real() {
        let mut t = Termios::default();
        let (r, _, out, set) = apply(&["size"], &mut t, true);
        assert!(r.is_ok() && out.is_empty() && set.is_none());
        let (r, _, out, _) = apply(&["size"], &mut t, false);
        assert!(r.is_ok());
        assert_eq!(out, [Piece::Out(b"24 80\n".to_vec())]);
        // The pixel fields go back as they came.
        let (r, _, _, set) = apply(&["rows", "30"], &mut t, false);
        assert!(r.is_ok());
        let size = |rows, cols| WinSize {
            rows,
            cols,
            xpixel: 640,
            ypixel: 480,
        };
        assert_eq!(set, Some(size(30, 80)));
        let (r, _, _, set) = apply(&["cols", "100"], &mut t, false);
        assert!(r.is_ok());
        assert_eq!(set, Some(size(24, 100)));
    }

    /// The warning falls between the output before it and after it.
    #[test]
    fn a_line_discipline_too_large_warns_in_its_place() {
        let mut t = Termios::default();
        let (r, _, pieces, _) = apply(&["size", "line", "300", "size"], &mut t, false);
        assert!(r.is_ok());
        assert_eq!(
            pieces,
            [
                Piece::Out(b"24 80\n".to_vec()),
                Piece::Warn("invalid line discipline ‘300’".to_string()),
                Piece::Out(b"24 80\n".to_vec()),
            ]
        );
        assert_eq!(t.c_line, 44);
    }

    #[test]
    fn drain_sets_the_moment_of_the_change() {
        let mut t = Termios::default();
        assert_eq!(
            apply(&["echo"], &mut t, false).1.tcsetattr_options,
            termios::TCSADRAIN
        );
        assert_eq!(
            apply(&["-drain", "echo"], &mut t, false)
                .1
                .tcsetattr_options,
            termios::TCSANOW
        );
    }

    #[test]
    fn the_help_is_upstreams_body() {
        assert!(HELP.starts_with("Usage: stty [-F DEVICE | --file=DEVICE] [SETTING]...\n"));
        assert!(HELP.ends_with("special values ^- or undef used to disable special characters.\n"));
        assert_eq!(HELP.len(), 7803);
    }

    #[test]
    fn the_display_of_a_sane_record() {
        let mut t = Termios::default();
        sane_mode(&mut t);
        t.c_cflag |= CS8 | 0o000017; // B38400
        t.c_ispeed = 0o17;
        t.c_ospeed = 0o17;
        let mut w = Wrap::new(80);
        display_changed(&mut w, &t);
        let text = String::from_utf8(w.out).unwrap();
        assert!(text.starts_with("speed 38400 baud; line = 0;\n"), "{text}");
    }
}
