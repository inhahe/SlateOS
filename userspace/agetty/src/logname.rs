//! Reading the login name off the line (`get_logname`'s loop): a byte at a
//! time, raw, echoing it, with erase, kill and end of line done by hand --
//! and, on the way, learning the terminal's erase and kill characters, its
//! end of line, its parity, and whether it types only upper case
//! ([`Chardata`]).

use crate::options::{F_EIGHTBITS, F_LCUC, F_UTF8, F_VCONSOLE, Options};
use crate::termio::{BS, CR, Chardata, DEL, NL, caps_lock, ctl, eight_bit};
use crate::{Diag, Exit};

/// `BUFSIZ`: the longest name, its terminating NUL included.
const LOGNAME_MAX: usize = 8192;

/// Backspace, space, backspace: one character rubbed out, in each parity.
const ERASE: [&[u8; 3]; 4] = [
    b"\x08\x20\x08",
    b"\x08\x20\x08",
    b"\x88\xa0\x88",
    b"\x88\xa0\x88",
];

/// The line, as the name is read from it.
pub trait Port {
    /// `read (STDIN_FILENO, &c, 1)`: a byte, `None` at end of file, or the
    /// `errno`.
    fn read_byte(&mut self) -> Result<Option<u8>, i32>;
    /// `write_all (STDOUT_FILENO, ...)`.
    fn write(&mut self, bytes: &[u8]);
    /// `xusleep (250000)`, before a read that would block is tried again.
    fn pause(&mut self);
}

/// How reading a name ended, short of an error.
#[derive(Debug, PartialEq, Eq)]
pub enum Read {
    /// The name, which the end of line ended -- possibly empty.
    Name(Vec<u8>),
    /// A BREAK (a NUL byte) on a line with speeds to try: the next one.
    NextSpeed,
}

/// One pass of `get_logname`'s reading, from a fresh prompt to an end of
/// line: the name, or [`Read::NextSpeed`].
///
/// `echo` is whether the line echoes by itself (`ECHO` in its local modes),
/// so that nothing is echoed here.
///
/// # Errors
///
/// The exit the line asked for: end of file or a hung-up line (status 0,
/// after the ten seconds' wait), `^D` (status 0 at once), a read that
/// failed otherwise or a name too long (logged, status 1).
pub fn read_name(
    port: &mut dyn Port,
    op: &Options,
    echo: bool,
    cp: &mut Chardata,
    diag: &mut dyn Diag,
) -> Result<Read, Exit> {
    let eightbit = eight_bit(op);
    let mut name: Vec<u8> = Vec::new();
    cp.eol = 0;
    while cp.eol == 0 {
        let byte = loop {
            match port.read_byte() {
                Ok(b) => break b,
                // "The terminal could be open with O_NONBLOCK when -L (force
                // CLOCAL) is specified..."
                Err(libcall::EINTR | libcall::EAGAIN) => port.pause(),
                Err(0 | libcall::pty::EIO | libcall::ESRCH | libcall::EINVAL | libcall::ENOENT) => {
                    return Err(diag.exit_slowly(0));
                }
                Err(e) => {
                    let mut msg = op.tty.clone();
                    msg.extend_from_slice(b": read: ");
                    msg.extend_from_slice(errmsg_of(e).as_bytes());
                    return Err(diag.log_err(&msg));
                }
            }
        };
        let at_end = byte.is_none();
        let c = byte.unwrap_or(0);
        let ascval = if eightbit { c } else { c & 0o177 };
        if !eightbit && c != ascval {
            // The bits set, the parity bit's among them.
            let bits = (ascval & 0o177).count_ones().saturating_add(1);
            cp.parity |= if bits & 1 == 1 { 1 } else { 2 };
        }
        // `strchr` finds a NUL in any string: the end that ends it. Not at
        // the end of the file, though, whose 0 upstream reads the same way
        // -- and then rubs out nothing, forever, at full speed, on a line
        // that hung up (lib.rs, deliberate differences).
        let key = if at_end {
            0
        } else if op
            .killchars
            .as_deref()
            .is_some_and(|k| ascval == 0 || k.contains(&ascval))
        {
            ctl(b'U')
        } else if op
            .erasechars
            .as_deref()
            .is_some_and(|k| ascval == 0 || k.contains(&ascval))
        {
            DEL
        } else {
            ascval
        };
        let erase = ERASE
            .get(usize::from(cp.parity))
            .copied()
            .unwrap_or(ERASE[0]);
        match key {
            0 => {
                if op.speeds.len() > 1 && op.flags & F_VCONSOLE == 0 {
                    return Ok(Read::NextSpeed);
                }
                if at_end {
                    return Err(diag.exit_slowly(0));
                }
            }
            CR | NL => cp.eol = ascval,
            BS | DEL => {
                cp.erase = ascval;
                if name.pop().is_some() && !echo {
                    port.write(erase);
                }
            }
            k if k == ctl(b'U') || k == ctl(b'C') => {
                if k == ctl(b'U') {
                    cp.kill = ascval;
                } else if op.flags & F_VCONSOLE == 0 {
                    // "Ignore CTRL+C on serial line".
                    continue;
                }
                while name.pop().is_some() {
                    if !echo {
                        port.write(erase);
                    }
                }
            }
            k if k == ctl(b'D') => return Err(Exit(0)),
            _ => {
                if name.len() >= LOGNAME_MAX - 1 {
                    let mut msg = op.tty.clone();
                    msg.extend_from_slice(b": input overrun");
                    return Err(diag.log_err(&msg));
                }
                if !echo {
                    // The byte as it came, parity bit and all.
                    port.write(&[c]);
                }
                name.push(ascval);
            }
        }
    }
    Ok(Read::Name(name))
}

/// What `get_logname` makes of a name read: on a UTF-8 line that is eight
/// bits clean, every character must be one and printable; otherwise, under
/// `--detect-case`, a name of upper case and no lower case is lowered and
/// the line noted as one that types only upper case.
///
/// # Errors
///
/// A name that is not characters, or holds one that cannot be printed:
/// logged, status 1.
pub fn finish_name(
    name: &mut [u8],
    op: &Options,
    cp: &mut Chardata,
    diag: &mut dyn Diag,
) -> Result<(), Exit> {
    if op.flags & (F_EIGHTBITS | F_UTF8) == F_EIGHTBITS | F_UTF8 {
        let mut state = libcall::locale::MbState::default();
        let mut rest: &[u8] = name;
        while !rest.is_empty() {
            match libcall::locale::mbrtowc(rest, &mut state) {
                libcall::locale::Mb::Char(wc, used) => {
                    if !libcall::locale::iswprint(wc) {
                        let mut msg = op.tty.clone();
                        msg.extend_from_slice(
                            format!(": invalid character 0x{wc:x} in login name").as_bytes(),
                        );
                        return Err(diag.log_err(&msg));
                    }
                    rest = rest.get(used.max(1)..).unwrap_or_default();
                }
                _ => {
                    let mut msg = op.tty.clone();
                    msg.extend_from_slice(b": invalid character conversion for login name");
                    return Err(diag.log_err(&msg));
                }
            }
        }
    } else if op.flags & F_LCUC != 0 && caps_lock(name) {
        cp.capslock = true;
        name.make_ascii_lowercase();
    }
    Ok(())
}

/// `%m`'s text for `errno`.
fn errmsg_of(errno: i32) -> String {
    errmsg::strerror(&std::io::Error::from_raw_os_error(errno))
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
mod tests {
    use super::*;
    use crate::tests::Said;

    /// A line whose input is scripted: bytes, then errors or end of file.
    #[derive(Default)]
    struct Script {
        input: Vec<Result<Option<u8>, i32>>,
        at: usize,
        out: Vec<u8>,
        pauses: usize,
    }

    impl Script {
        fn typing(bytes: &[u8]) -> Self {
            Self {
                input: bytes.iter().map(|&b| Ok(Some(b))).collect(),
                ..Self::default()
            }
        }
    }

    impl Port for Script {
        fn read_byte(&mut self) -> Result<Option<u8>, i32> {
            let r = self.input.get(self.at).copied().unwrap_or(Ok(None));
            self.at += 1;
            r
        }
        fn write(&mut self, bytes: &[u8]) {
            self.out.extend_from_slice(bytes);
        }
        fn pause(&mut self) {
            self.pauses += 1;
        }
    }

    fn read(input: &[u8], op: &Options) -> (Result<Read, Exit>, Chardata, Vec<u8>, Said) {
        let mut port = Script::typing(input);
        let mut cp = Chardata::default();
        let mut said = Said::default();
        let r = read_name(&mut port, op, false, &mut cp, &mut said);
        (r, cp, port.out, said)
    }

    #[test]
    fn a_name_is_echoed_and_ends_at_the_end_of_line() {
        let (r, cp, out, _) = read(b"root\r", &Options::default());
        assert_eq!(r, Ok(Read::Name(b"root".to_vec())));
        assert_eq!(cp.eol, CR);
        assert_eq!(out, b"root");
        let (r, cp, _, _) = read(b"x\n", &Options::default());
        assert_eq!((r, cp.eol), (Ok(Read::Name(b"x".to_vec())), NL));
        // An empty line is an empty name.
        assert_eq!(read(b"\r", &Options::default()).0, Ok(Read::Name(vec![])));
    }

    #[test]
    fn erase_and_kill_rub_out_and_are_remembered() {
        let (r, cp, out, _) = read(b"ab\x08c\x7fd\r", &Options::default());
        assert_eq!(r, Ok(Read::Name(b"ad".to_vec())));
        assert_eq!(cp.erase, DEL);
        assert_eq!(out, b"ab\x08 \x08c\x08 \x08d");
        let (r, cp, out, _) = read(b"abc\x15x\r", &Options::default());
        assert_eq!(r, Ok(Read::Name(b"x".to_vec())));
        assert_eq!(cp.kill, 0x15);
        assert_eq!(out, b"abc\x08 \x08\x08 \x08\x08 \x08x");
        // Nothing to rub out: nothing written, the character still learnt.
        let (_, cp, out, _) = read(b"\x08a\r", &Options::default());
        assert_eq!((cp.erase, out), (BS, b"a".to_vec()));
    }

    #[test]
    fn the_erase_and_kill_characters_may_be_named() {
        let op = Options {
            erasechars: Some(b"#".to_vec()),
            killchars: Some(b"@".to_vec()),
            ..Options::default()
        };
        let (r, cp, _, _) = read(b"ab#c@de#\r", &op);
        assert_eq!(r, Ok(Read::Name(b"d".to_vec())));
        assert_eq!((cp.erase, cp.kill), (b'#', b'@'));
    }

    #[test]
    fn control_c_rubs_out_only_on_a_console_and_control_d_ends() {
        let (r, ..) = read(b"ab\x03c\r", &Options::default());
        assert_eq!(r, Ok(Read::Name(b"abc".to_vec())));
        let console = Options {
            flags: F_VCONSOLE,
            ..Options::default()
        };
        let (r, ..) = read(b"ab\x03c\r", &console);
        assert_eq!(r, Ok(Read::Name(b"c".to_vec())));
        let (r, _, _, said) = read(b"ab\x04", &Options::default());
        assert_eq!(r, Err(Exit(0)));
        assert!(!said.slow_exit);
    }

    #[test]
    fn a_break_tries_the_next_speed_or_is_ignored() {
        let two = Options {
            speeds: vec![0o15, 0o17],
            ..Options::default()
        };
        assert_eq!(read(b"ab\0", &two).0, Ok(Read::NextSpeed));
        assert_eq!(
            read(b"ab\0c\r", &Options::default()).0,
            Ok(Read::Name(b"abc".to_vec()))
        );
        // Unless a kill character is named: a NUL is in every string.
        let killing = Options {
            killchars: Some(b"@".to_vec()),
            ..two
        };
        assert_eq!(read(b"ab\0c\r", &killing).0, Ok(Read::Name(b"c".to_vec())));
        // But the end of the file is the end, named characters or not.
        assert_eq!(read(b"ab", &killing).0, Ok(Read::NextSpeed));
        let one = Options {
            killchars: Some(b"@".to_vec()),
            erasechars: Some(b"#".to_vec()),
            ..Options::default()
        };
        let (r, _, _, said) = read(b"ab", &one);
        assert_eq!(r, Err(Exit(0)));
        assert!(said.slow_exit);
    }

    #[test]
    fn the_end_of_the_line_ends_slowly() {
        let (r, _, _, said) = read(b"ab", &Options::default());
        assert_eq!(r, Err(Exit(0)));
        assert!(said.slow_exit);
        let mut port = Script {
            input: vec![
                Err(libcall::EAGAIN),
                Err(libcall::EINTR),
                Ok(Some(b'a')),
                Err(libcall::pty::EIO),
            ],
            ..Script::default()
        };
        let mut said = Said::default();
        let r = read_name(
            &mut port,
            &Options::default(),
            false,
            &mut Chardata::default(),
            &mut said,
        );
        assert_eq!((r, port.pauses, said.slow_exit), (Err(Exit(0)), 2, true));
        let mut port = Script {
            input: vec![Err(libcall::pty::EBADF)],
            ..Script::default()
        };
        let mut said = Said::default();
        let r = read_name(
            &mut port,
            &Options::default(),
            false,
            &mut Chardata::default(),
            &mut said,
        );
        assert_eq!(r, Err(Exit(1)));
        let want = format!("tty1: read: {}", errmsg_of(libcall::pty::EBADF));
        assert_eq!(said.syslog, [want.into_bytes()]);
    }

    #[test]
    fn parity_is_learnt_from_the_eighth_bit() {
        // 'a' is 0x61, three bits: odd with the parity bit set is even
        // parity; 'c' 0x63, four bits, with the bit set is odd.
        let (r, cp, out, _) = read(b"\xe1\xe3\r", &Options::default());
        assert_eq!(r, Ok(Read::Name(b"ac".to_vec())));
        assert_eq!(cp.parity, 3);
        assert_eq!(out, b"\xe1\xe3");
        let (_, cp, out, _) = read(b"\xe1\x08\r", &Options::default());
        assert_eq!(cp.parity, 2);
        assert_eq!(out, b"\xe1\x88\xa0\x88");
        // An eight-bit line keeps the bit.
        let eight = Options {
            flags: F_EIGHTBITS,
            ..Options::default()
        };
        let (r, cp, ..) = read(b"\xe1\r", &eight);
        assert_eq!((r, cp.parity), (Ok(Read::Name(vec![0xe1])), 0));
    }

    #[test]
    fn an_echoing_line_is_not_echoed_to() {
        let mut port = Script::typing(b"ab\x7f\r");
        let r = read_name(
            &mut port,
            &Options::default(),
            true,
            &mut Chardata::default(),
            &mut Said::default(),
        );
        assert_eq!(r, Ok(Read::Name(b"a".to_vec())));
        assert!(port.out.is_empty());
    }

    #[test]
    fn a_name_too_long_is_an_overrun() {
        let mut long = vec![b'a'; LOGNAME_MAX];
        long.push(b'\r');
        let (r, _, _, said) = read(&long, &Options::default());
        assert_eq!(r, Err(Exit(1)));
        assert_eq!(said.syslog, [b"tty1: input overrun".to_vec()]);
    }

    #[test]
    fn upper_case_only_is_lowered_under_detect_case() {
        let op = Options {
            flags: F_LCUC,
            ..Options::default()
        };
        let mut cp = Chardata::default();
        let mut name = b"ROOT".to_vec();
        finish_name(&mut name, &op, &mut cp, &mut Said::default()).unwrap();
        assert_eq!((name.as_slice(), cp.capslock), (&b"root"[..], true));
        let mut name = b"Root".to_vec();
        let mut cp = Chardata::default();
        finish_name(&mut name, &op, &mut cp, &mut Said::default()).unwrap();
        assert_eq!((name.as_slice(), cp.capslock), (&b"Root"[..], false));
        let mut name = b"ROOT".to_vec();
        finish_name(
            &mut name,
            &Options::default(),
            &mut cp,
            &mut Said::default(),
        )
        .unwrap();
        assert_eq!(name, b"ROOT");
    }
}
