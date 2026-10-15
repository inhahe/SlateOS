//! The line's modes: what agetty sets before it reads a name -- raw, eight
//! bits, at the first speed (`termio_init`), or a virtual console's sane
//! defaults (`reset_virtual_console`) -- and what it sets after, from what
//! the name's characters showed (`termio_final`).
//!
//! Each works on a `struct termios` in memory; setting it on the line is
//! the caller's (`run.rs`).

use libcall::termios::{self as tc, Termios};

use crate::options::{
    Clocal, F_EIGHTBITS, F_KEEPCFLAGS, F_KEEPSPEED, F_RTSCTS, F_UTF8, F_VCONSOLE, Options,
};

/// `CTL (x)`: the control character of `x`.
#[must_use]
pub const fn ctl(x: u8) -> u8 {
    x ^ 0o100
}

/// Carriage return.
pub const CR: u8 = ctl(b'M');
/// Line feed.
pub const NL: u8 = ctl(b'J');
/// Back space.
pub const BS: u8 = ctl(b'H');
/// Delete.
pub const DEL: u8 = ctl(b'?');

/// `TTYDEF_SPEED`: a line's speed when it says it has none.
const TTYDEF_SPEED: u32 = 0o15;

/// `struct chardata`: what the characters of the name showed of the
/// terminal -- its erase, kill and end-of-line characters, the parity bits
/// seen, and whether it can type lower case.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Chardata {
    /// The erase character.
    pub erase: u8,
    /// The kill character.
    pub kill: u8,
    /// The end-of-line character, 0 until one was read.
    pub eol: u8,
    /// The parities seen: 1 odd, 2 even, both 3, none 0.
    pub parity: u8,
    /// Upper case and no lower case.
    pub capslock: bool,
}

impl Default for Chardata {
    /// `INIT_CHARDATA`.
    fn default() -> Self {
        Self {
            erase: DEL,
            kill: ctl(b'U'),
            eol: CR,
            parity: 0,
            capslock: false,
        }
    }
}

/// `reset_virtual_console (tp, flags)`: a virtual console's sane modes, as
/// `stty sane cr0 pass8` sets them -- its control flags too, unless
/// `keep_cflags`; UTF-8 input as `utf8` says.
pub fn reset_virtual_console(tp: &mut Termios, keep_cflags: bool, utf8: bool) {
    tp.c_iflag |= tc::BRKINT | tc::ICRNL | tc::IMAXBEL | tc::IXON | tc::IXANY;
    tp.c_oflag |= tc::OPOST | tc::ONLCR;
    tp.c_lflag |=
        tc::ECHO | tc::ICANON | tc::ISIG | tc::IEXTEN | tc::ECHOE | tc::ECHOKE | tc::ECHOCTL;
    if !keep_cflags {
        // Upstream clears the speed bits from the *local* flags here, and
        // so ORs B38400 into whatever speed the line had.
        tp.c_lflag &= !tc::CBAUD;
        tp.c_cflag |= tc::B38400 | tc::CREAD | tc::CS8 | tc::HUPCL;
    }
    tp.c_iflag |= tc::BRKINT | tc::ICRNL | tc::IMAXBEL;
    tp.c_iflag &=
        !(tc::IGNBRK | tc::INLCR | tc::IGNCR | tc::IXOFF | tc::IUCLC | tc::IXANY | tc::ISTRIP);
    tp.c_oflag |= tc::OPOST | tc::ONLCR;
    tp.c_oflag &= !(tc::OLCUC
        | tc::OCRNL
        | tc::ONOCR
        | tc::ONLRET
        | tc::OFILL
        | tc::NLDLY
        | tc::CRDLY
        | tc::TABDLY
        | tc::BSDLY
        | tc::VTDLY
        | tc::FFDLY);
    tp.c_lflag |= tc::ISIG
        | tc::ICANON
        | tc::IEXTEN
        | tc::ECHO
        | tc::ECHOE
        | tc::ECHOK
        | tc::ECHOKE
        | tc::ECHOCTL;
    tp.c_lflag &= !(tc::ECHONL | tc::ECHOPRT | tc::NOFLSH | tc::TOSTOP);
    if !keep_cflags {
        tp.c_cflag |= tc::CREAD | tc::CS8 | tc::HUPCL;
        tp.c_cflag &= !(tc::PARODD | tc::PARENB);
    }
    tp.c_oflag &= !tc::OFDEL;
    tp.c_lflag &= !tc::XCASE;
    if utf8 {
        tp.c_iflag |= tc::IUTF8;
    } else {
        tp.c_iflag &= !tc::IUTF8;
    }
    let cc = &mut tp.c_cc;
    for (slot, value) in [
        (tc::VTIME, 0),
        (tc::VMIN, 1),
        (tc::VINTR, ctl(b'C')),
        (tc::VQUIT, 0o34),
        (tc::VERASE, DEL),
        (tc::VKILL, ctl(b'U')),
        (tc::VEOF, ctl(b'D')),
        (tc::VSWTC, 0),
        (tc::VSTART, ctl(b'Q')),
        (tc::VSTOP, ctl(b'S')),
        (tc::VSUSP, ctl(b'Z')),
        (tc::VEOL, 0),
        (tc::VREPRINT, ctl(b'R')),
        (tc::VDISCARD, ctl(b'O')),
        (tc::VWERASE, ctl(b'W')),
        (tc::VLNEXT, ctl(b'V')),
        (tc::VEOL2, 0),
    ] {
        if let Some(c) = cc.get_mut(slot) {
            *c = value;
        }
    }
}

/// `termio_init`'s serial half, in memory: raw, eight-bit, blocking, at the
/// first speed -- or at the line's own, kept as the last to try, under
/// `--keep-baud` or with no speed given.
pub fn init_serial(op: &mut Options, tp: &mut Termios) {
    let (ispeed, ospeed);
    if op.flags & F_KEEPSPEED != 0 || op.speeds.is_empty() {
        let i = tc::input_speed(tp);
        let o = tc::output_speed(tp);
        // "Save also the original speed to array of the speeds to make it
        // possible to return the original after unexpected BREAKs."
        if !op.speeds.is_empty() {
            op.speeds.push(if i != 0 {
                i
            } else if o != 0 {
                o
            } else {
                TTYDEF_SPEED
            });
        }
        ispeed = if i == 0 { TTYDEF_SPEED } else { i };
        ospeed = if o == 0 { TTYDEF_SPEED } else { o };
    } else {
        let first = op.speeds.first().copied().unwrap_or(TTYDEF_SPEED);
        (ispeed, ospeed) = (first, first);
    }
    // With `--autologin` nothing is read from the line, and the input modes
    // a name's characters would set are not known: they are kept.
    if op.autolog.is_none() {
        tp.c_iflag &= tc::IUTF8;
        if tp.c_iflag & tc::IUTF8 != 0 {
            op.flags |= F_UTF8;
        }
    }
    tp.c_lflag = 0;
    tp.c_oflag &= tc::OPOST | tc::ONLCR;
    if op.flags & F_KEEPCFLAGS == 0 {
        tp.c_cflag = tc::CS8 | tc::HUPCL | tc::CREAD | (tp.c_cflag & tc::CLOCAL);
    }
    // A speed the library refuses leaves the record as it was, as upstream
    // ignores the refusal.
    let _ = tc::set_input_speed(tp, ispeed);
    let _ = tc::set_output_speed(tp, ospeed);
    match op.clocal {
        Clocal::Always => tp.c_cflag |= tc::CLOCAL,
        Clocal::Never => tp.c_cflag &= !tc::CLOCAL,
        Clocal::Auto => {}
    }
    tp.c_line = 0;
    if let Some(c) = tp.c_cc.get_mut(tc::VMIN) {
        *c = 1;
    }
    if let Some(c) = tp.c_cc.get_mut(tc::VTIME) {
        *c = 0;
    }
    if op.flags & F_RTSCTS != 0 {
        tp.c_cflag |= tc::CRTSCTS;
    }
}

/// Whether a virtual console's modes allow eight-bit characters: eight-bit
/// characters, no parity (`F_EIGHTBITS` after `reset_vc`).
#[must_use]
pub fn eight_bit_clean(tp: &Termios) -> bool {
    tp.c_cflag & (tc::CS8 | tc::PARODD | tc::PARENB) == tc::CS8
}

/// `termio_final (op, tp, cp)` in memory: a serial line's modes for the
/// login, from what the name's characters showed.
pub fn final_modes(op: &Options, tp: &mut Termios, cp: &Chardata) {
    tp.c_iflag |= tc::IXON | tc::IXOFF;
    tp.c_lflag |= tc::ICANON | tc::ISIG | tc::ECHO | tc::ECHOE | tc::ECHOK | tc::ECHOKE;
    tp.c_oflag |= tc::OPOST;
    for (slot, value) in [
        (tc::VINTR, ctl(b'C')),
        (tc::VQUIT, ctl(b'\\')),
        (tc::VEOF, ctl(b'D')),
        (tc::VEOL, 0),
        (tc::VSWTC, 0),
    ] {
        if let Some(c) = tp.c_cc.get_mut(slot) {
            *c = value;
        }
    }
    if cp.eol == CR {
        tp.c_iflag |= tc::ICRNL;
        tp.c_oflag |= tc::ONLCR;
    }
    if let Some(c) = tp.c_cc.get_mut(tc::VERASE) {
        *c = cp.erase;
    }
    if let Some(c) = tp.c_cc.get_mut(tc::VKILL) {
        *c = cp.kill;
    }
    // Odd parity falls through to even's, and both to seven bits.
    if cp.parity == 1 {
        tp.c_cflag |= tc::PARODD;
    }
    if cp.parity == 1 || cp.parity == 2 {
        tp.c_cflag |= tc::PARENB;
        tp.c_iflag |= tc::INPCK | tc::ISTRIP;
    }
    if cp.parity != 0 {
        tp.c_cflag &= !tc::CSIZE;
        tp.c_cflag |= tc::CS7;
    }
    if cp.capslock {
        tp.c_iflag |= tc::IUCLC;
        tp.c_lflag |= tc::XCASE;
        tp.c_oflag |= tc::OLCUC;
    }
    if op.flags & F_RTSCTS != 0 {
        tp.c_cflag |= tc::CRTSCTS;
    }
}

/// `auto_baud`'s reading of a modem's status message: the speed of the
/// first number in it, if the table knows it. Only the first digit's run is
/// looked at.
#[must_use]
pub fn status_message_speed(said: &[u8]) -> Option<u32> {
    let at = said.iter().position(u8::is_ascii_digit)?;
    let code = crate::speeds::bcode(said.get(at..)?);
    (code != 0).then_some(code)
}

/// `caps_lock (s)`: upper case in the name and no lower case.
#[must_use]
pub fn caps_lock(name: &[u8]) -> bool {
    !name.iter().any(u8::is_ascii_lowercase) && name.iter().any(u8::is_ascii_uppercase)
}

/// Whether the name is read eight bits clean: on an eight-bit line, or a
/// UTF-8 one.
#[must_use]
pub fn eight_bit(op: &Options) -> bool {
    op.flags & (F_EIGHTBITS | F_UTF8) != 0
}

/// Whether `op` describes a serial line that has `flag` set
/// (`serial_tty_option`).
#[must_use]
pub fn serial_option(op: &Options, flag: u32) -> bool {
    op.flags & (F_VCONSOLE | flag) == flag
}

#[cfg(test)]
#[allow(clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn line(cflag: u32) -> Termios {
        Termios {
            c_iflag: tc::ICRNL | tc::IXON | tc::IUTF8 | tc::ISTRIP,
            c_oflag: tc::OPOST | tc::ONLCR | tc::OLCUC,
            c_cflag: cflag,
            c_lflag: tc::ECHO | tc::ICANON,
            c_line: 3,
            c_cc: [7; tc::NCCS],
            c_ispeed: 0,
            c_ospeed: 0,
        }
    }

    #[test]
    fn the_control_characters() {
        assert_eq!((CR, NL, BS, DEL), (b'\r', b'\n', 8, 0x7f));
        assert_eq!(Chardata::default().kill, 0x15);
    }

    #[test]
    fn a_serial_line_is_made_raw_at_the_first_speed() {
        let mut op = Options {
            speeds: vec![0o15, 0o17],
            ..Options::default()
        };
        let mut tp = line(tc::CLOCAL | tc::PARENB | 0o13);
        init_serial(&mut op, &mut tp);
        assert_eq!(tp.c_iflag, tc::IUTF8);
        assert_ne!(op.flags & F_UTF8, 0);
        assert_eq!(tp.c_lflag, 0);
        assert_eq!(tp.c_oflag, tc::OPOST | tc::ONLCR);
        assert_eq!(
            tp.c_cflag,
            tc::CS8 | tc::HUPCL | tc::CREAD | tc::CLOCAL | 0o15
        );
        assert_eq!(tc::input_speed(&tp), 0o15);
        assert_eq!(
            (tp.c_line, tp.c_cc[tc::VMIN], tp.c_cc[tc::VTIME]),
            (0, 1, 0)
        );
        assert_eq!(op.speeds, [0o15, 0o17]);
    }

    #[test]
    fn keep_baud_starts_at_the_line_s_speed_and_tries_it_last() {
        let mut op = Options {
            speeds: vec![0o15],
            flags: F_KEEPSPEED,
            clocal: Clocal::Never,
            ..Options::default()
        };
        let mut tp = line(tc::CLOCAL | 0o13);
        init_serial(&mut op, &mut tp);
        assert_eq!(op.speeds, [0o15, 0o13]);
        assert_eq!(tp.c_cflag & tc::CBAUD, 0o13);
        assert_eq!(tp.c_cflag & tc::CLOCAL, 0);
        // A line with no speed is given 9600, and so is the list.
        let mut op = Options {
            speeds: vec![0o17],
            flags: F_KEEPSPEED | F_KEEPCFLAGS | F_RTSCTS,
            clocal: Clocal::Always,
            ..Options::default()
        };
        let mut tp = line(tc::PARENB);
        init_serial(&mut op, &mut tp);
        assert_eq!(op.speeds, [0o17, 0o15]);
        assert_eq!(tp.c_cflag, tc::PARENB | tc::CLOCAL | tc::CRTSCTS | 0o15);
    }

    #[test]
    fn autologin_keeps_the_input_modes() {
        let mut op = Options {
            autolog: Some(b"root".to_vec()),
            ..Options::default()
        };
        let mut tp = line(0o15);
        init_serial(&mut op, &mut tp);
        assert_eq!(tp.c_iflag, tc::ICRNL | tc::IXON | tc::IUTF8 | tc::ISTRIP);
        assert_eq!(op.flags & F_UTF8, 0);
    }

    #[test]
    fn a_virtual_console_is_made_sane() {
        let mut tp = Termios {
            c_iflag: tc::IGNBRK | tc::ISTRIP | tc::IUTF8,
            c_oflag: tc::OLCUC | tc::OFDEL,
            c_cflag: tc::PARENB | tc::PARODD | 0o15,
            c_lflag: tc::XCASE | tc::TOSTOP,
            ..Termios::default()
        };
        reset_virtual_console(&mut tp, false, false);
        assert_eq!(tp.c_iflag, tc::BRKINT | tc::ICRNL | tc::IMAXBEL | tc::IXON);
        assert_eq!(tp.c_oflag, tc::OPOST | tc::ONLCR);
        // Upstream's slip: 38400 is ORed into the speed already there.
        assert_eq!(tp.c_cflag, tc::CREAD | tc::CS8 | tc::HUPCL | 0o17);
        assert_eq!(
            tp.c_lflag,
            tc::ISIG
                | tc::ICANON
                | tc::IEXTEN
                | tc::ECHO
                | tc::ECHOE
                | tc::ECHOK
                | tc::ECHOKE
                | tc::ECHOCTL
        );
        assert_eq!(tp.c_cc[tc::VERASE], 0x7f);
        assert_eq!(tp.c_cc[tc::VQUIT], 0x1c);
        assert_eq!(tp.c_cc[tc::VMIN], 1);
        let mut kept = Termios {
            c_cflag: tc::PARENB | 0o15,
            ..Termios::default()
        };
        reset_virtual_console(&mut kept, true, true);
        assert_eq!(kept.c_cflag, tc::PARENB | 0o15);
        assert_ne!(kept.c_iflag & tc::IUTF8, 0);
        assert!(eight_bit_clean(&tp));
        assert!(!eight_bit_clean(&kept));
    }

    #[test]
    fn the_final_modes_follow_what_the_name_showed() {
        let op = Options::default();
        let mut tp = Termios {
            c_cflag: tc::CS8 | 0o15,
            ..Termios::default()
        };
        let cp = Chardata {
            erase: BS,
            kill: 0x18,
            eol: CR,
            parity: 1,
            capslock: true,
        };
        final_modes(&op, &mut tp, &cp);
        assert_eq!(
            tp.c_iflag,
            tc::IXON | tc::IXOFF | tc::ICRNL | tc::INPCK | tc::ISTRIP | tc::IUCLC
        );
        assert_eq!(tp.c_oflag, tc::OPOST | tc::ONLCR | tc::OLCUC);
        assert_eq!(tp.c_cflag, tc::CS7 | tc::PARENB | tc::PARODD | 0o15);
        assert_eq!(
            tp.c_lflag,
            tc::ICANON | tc::ISIG | tc::ECHO | tc::ECHOE | tc::ECHOK | tc::ECHOKE | tc::XCASE
        );
        assert_eq!((tp.c_cc[tc::VERASE], tp.c_cc[tc::VKILL]), (BS, 0x18));
        assert_eq!(tp.c_cc[tc::VQUIT], 0x1c);
        // Both parities seen: seven bits, no parity bit.
        let mut tp = Termios {
            c_cflag: tc::CS8,
            ..Termios::default()
        };
        let both = Chardata {
            eol: NL,
            parity: 3,
            ..Chardata::default()
        };
        final_modes(
            &Options {
                flags: F_RTSCTS,
                ..Options::default()
            },
            &mut tp,
            &both,
        );
        assert_eq!(tp.c_cflag, tc::CS7 | tc::CRTSCTS);
        assert_eq!(tp.c_iflag, tc::IXON | tc::IXOFF);
        assert_eq!(tp.c_oflag, tc::OPOST);
        // Even parity: a parity bit, not odd.
        let mut tp = Termios::default();
        final_modes(
            &op,
            &mut tp,
            &Chardata {
                parity: 2,
                ..Chardata::default()
            },
        );
        assert_eq!(tp.c_cflag, tc::CS7 | tc::PARENB);
    }

    #[test]
    fn a_modem_s_message_gives_its_first_number_s_speed() {
        assert_eq!(
            status_message_speed(b"\r\nCONNECT 9600/ARQ\r\n"),
            Some(0o15)
        );
        assert_eq!(status_message_speed(b"CONNECT 1234 9600"), None);
        assert_eq!(status_message_speed(b"RING"), None);
    }

    #[test]
    fn caps_lock_is_upper_case_and_no_lower() {
        assert!(caps_lock(b"ROOT"));
        assert!(caps_lock(b"R00T"));
        assert!(!caps_lock(b"Root"));
        assert!(!caps_lock(b"1234"));
        assert!(!caps_lock(b""));
    }

    #[test]
    fn serial_options_are_not_a_console_s() {
        let serial = Options {
            flags: crate::options::F_PARSE,
            ..Options::default()
        };
        assert!(serial_option(&serial, crate::options::F_PARSE));
        let console = Options {
            flags: crate::options::F_PARSE | F_VCONSOLE,
            ..Options::default()
        };
        assert!(!serial_option(&console, crate::options::F_PARSE));
        assert!(!eight_bit(&Options::default()));
        assert!(eight_bit(&Options {
            flags: F_UTF8,
            ..Options::default()
        }));
    }
}
