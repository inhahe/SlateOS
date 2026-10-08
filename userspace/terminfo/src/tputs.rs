//! `tputs` (`lib_tputs.c`) and `delay_output`: a capability on its way to
//! the terminal, its `$<n>` padding turned into pad characters.
//!
//! A program that has set a terminal up with `setupterm` or `tgetent` but
//! has no curses screen calls `tputs` through a stand-in screen of
//! upstream's making, one with no terminal of its own -- and for that
//! screen upstream applies every delay (`normal_delay`), mandatory or not.
//! A delay of `n` milliseconds is then `n * baud / 9000` copies of `PC`
//! (`pad_char`, else NUL) at the speed of the terminal `setupterm` found,
//! which is none -- so nothing -- when neither standard output nor standard
//! error is a terminal; or, for a terminal with `npc`, a pause instead.
//! With no terminal set up at all, `delay_output` does nothing.

use std::time::Duration;

/// `MAX_DELAY_MSECS`.
const MAX_DELAY_MSECS: i32 = 30000;
/// `BAUDBYTE`: 7 bits, a parity bit and a stop bit.
const BAUDBYTE: i32 = 9;

/// What `delay_output` needs to know of the terminal.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Padding {
    /// Whether a terminal is set up at all.
    pub terminal: bool,
    /// `_nc_baudrate (ospeed)`: the terminal's output speed in bits per
    /// second, 0 when it was found on no terminal.
    pub baud: i32,
    /// `PC`: `pad_char`'s first byte, set by `tgetent`; NUL otherwise.
    pub pad: u8,
    /// `no_pad_char` (`npc`): pause rather than pad.
    pub no_pad_char: bool,
}

/// The byte at `i`, or the NUL that ends a C string.
fn at(s: &[u8], i: usize) -> u8 {
    s.get(i).copied().unwrap_or(0)
}

/// `delay_output (ms)`, its pad characters onto `out`.
fn delay_output(out: &mut Vec<u8>, ms: i32, padding: &Padding) {
    let ms = ms.min(MAX_DELAY_MSECS);
    if !padding.terminal {
        return;
    }
    if padding.no_pad_char {
        // `napms (ms)`.
        std::thread::sleep(Duration::from_millis(u64::try_from(ms).unwrap_or(0)));
    } else {
        // `(ms * _nc_baudrate (ospeed)) / (BAUDBYTE * 1000)`, in `int`s.
        let nullcount = ms
            .wrapping_mul(padding.baud)
            .checked_div(BAUDBYTE * 1000)
            .unwrap_or(0);
        out.extend(std::iter::repeat_n(
            padding.pad,
            usize::try_from(nullcount).unwrap_or(0),
        ));
    }
}

/// `tputs (string, 1, outc)`, called with no curses screen: the bytes
/// `outc` is given.
#[must_use]
pub fn tputs(string: &[u8], padding: &Padding) -> Vec<u8> {
    let mut out = Vec::with_capacity(string.len());
    let mut s = 0usize;
    while at(string, s) != 0 {
        if at(string, s) == b'$' {
            s = s.saturating_add(1);
            if at(string, s) == b'<' {
                s = s.saturating_add(1);
                let rest = string.get(s..).unwrap_or_default();
                let rest = rest
                    .get(..rest.iter().position(|&b| b == 0).unwrap_or(rest.len()))
                    .unwrap_or_default();
                if (!at(string, s).is_ascii_digit() && at(string, s) != b'.')
                    || !rest.contains(&b'>')
                {
                    // Not padding after all: `$<` as it is, and on from
                    // the byte after it.
                    out.extend_from_slice(b"$<");
                    continue;
                }
                // Tenths of a millisecond: the digits, and one after a dot.
                let mut number: i32 = 0;
                while at(string, s).is_ascii_digit() {
                    number = number
                        .wrapping_mul(10)
                        .wrapping_add(i32::from(at(string, s).wrapping_sub(b'0')));
                    s = s.saturating_add(1);
                }
                number = number.wrapping_mul(10);
                if at(string, s) == b'.' {
                    s = s.saturating_add(1);
                    if at(string, s).is_ascii_digit() {
                        number = number.wrapping_add(i32::from(at(string, s).wrapping_sub(b'0')));
                        s = s.saturating_add(1);
                    }
                    while at(string, s).is_ascii_digit() {
                        s = s.saturating_add(1);
                    }
                }
                // `*` multiplies by the lines affected, which is 1 here;
                // `/` makes it mandatory, which with no screen it is anyway.
                while at(string, s) == b'*' || at(string, s) == b'/' {
                    s = s.saturating_add(1);
                }
                if number > 0 {
                    delay_output(&mut out, number / 10, padding);
                }
            } else {
                out.push(b'$');
                if at(string, s) != 0 {
                    out.push(at(string, s));
                }
            }
        } else {
            out.push(at(string, s));
        }
        if at(string, s) == 0 {
            break;
        }
        s = s.saturating_add(1);
    }
    out
}

/// `_nc_baudrate (ospeed)`: the bits per second of a `B*` speed, -1 for a
/// code that is none of them.
#[must_use]
pub fn baudrate(ospeed: u32) -> i32 {
    const SPEEDS: [(u32, i32); 31] = [
        (0, 0),
        (1, 50),
        (2, 75),
        (3, 110),
        (4, 134),
        (5, 150),
        (6, 200),
        (7, 300),
        (8, 600),
        (9, 1200),
        (10, 1800),
        (11, 2400),
        (12, 4800),
        (13, 9600),
        (14, 19200),
        (15, 38400),
        (0o10001, 57600),
        (0o10002, 115_200),
        (0o10003, 230_400),
        (0o10004, 460_800),
        (0o10005, 500_000),
        (0o10006, 576_000),
        (0o10007, 921_600),
        (0o10010, 1_000_000),
        (0o10011, 1_152_000),
        (0o10012, 1_500_000),
        (0o10013, 2_000_000),
        (0o10014, 2_500_000),
        (0o10015, 3_000_000),
        (0o10016, 3_500_000),
        (0o10017, 4_000_000),
    ];
    SPEEDS
        .iter()
        .find(|&&(code, _)| code == ospeed)
        .map_or(-1, |&(_, baud)| baud)
}

#[cfg(test)]
#[allow(clippy::arithmetic_side_effects)]
mod tests {
    use super::*;

    const NONE: Padding = Padding {
        terminal: false,
        baud: 0,
        pad: 0,
        no_pad_char: false,
    };

    #[test]
    fn with_no_terminal_padding_goes_and_the_rest_stays() {
        assert_eq!(tputs(b"\x1b[1m$<2>", &NONE), b"\x1b[1m");
        assert_eq!(tputs(b"\x1b[m\x0f$<2>", &NONE), b"\x1b[m\x0f");
        assert_eq!(tputs(b"a$<5*/>b", &NONE), b"ab");
        assert_eq!(tputs(b"a$<1.5>b", &NONE), b"ab");
        // Not padding: no digit, or no `>`.
        assert_eq!(tputs(b"a$<x>b", &NONE), b"a$<x>b");
        assert_eq!(tputs(b"a$<5b", &NONE), b"a$<5b");
        // A lone `$`.
        assert_eq!(tputs(b"a$b$", &NONE), b"a$b$");
        // The byte that ends a malformed delay is skipped, as upstream
        // skips it: `$<5x>` loses its `x`.
        assert_eq!(tputs(b"$<5x>!", &NONE), b">!");
    }

    #[test]
    fn on_a_terminal_a_delay_is_pad_characters_at_its_speed() {
        // Measured: vt100's `bold`, `\E[1m$<2>`, on a 38400-baud pty is
        // followed by eight NULs; a `$<5>` by twenty-one.
        let pty = Padding {
            terminal: true,
            baud: 38400,
            pad: 0,
            no_pad_char: false,
        };
        assert_eq!(tputs(b"\x1b[1m$<2>", &pty), b"\x1b[1m\0\0\0\0\0\0\0\0");
        assert_eq!(tputs(b"$<5>", &pty).len(), 21);
        // A tenth is read, but `delay_output` is given whole milliseconds:
        // 2.5 is 2, and 0.9 is a delay of nothing.
        assert_eq!(tputs(b"$<2.5>", &pty).len(), 8);
        assert_eq!(tputs(b"$<0.9>", &pty).len(), 0);
        // Another pad character, and no speed at all.
        let pc = Padding { pad: b'*', ..pty };
        assert_eq!(tputs(b"x$<2>", &pc), b"x********");
        let pipe = Padding { baud: 0, ..pty };
        assert_eq!(tputs(b"x$<2>", &pipe), b"x");
    }

    #[test]
    fn speeds_are_ncurses_table() {
        assert_eq!(baudrate(0), 0);
        assert_eq!(baudrate(13), 9600);
        assert_eq!(baudrate(15), 38400);
        assert_eq!(baudrate(0o10002), 115_200);
        assert_eq!(baudrate(16), -1);
    }
}
