//! `xxd` -- make a hex dump, or turn one back into binary: vim 9.1.0016's,
//! ported.
//!
//! ```text
//! xxd [options] [infile [outfile]]
//! xxd -r [-s [-]offset] [-c cols] [-ps] [infile [outfile]]
//! ```
//!
//! A transcription of vim's `src/xxd/xxd.c` at v9.1.0016 ("xxd
//! 2023-10-25"), which Ubuntu's `xxd` 2:9.1.0016-1ubuntu7.20 -- the
//! reference `scripts/xxd-diff.sh` measures this against -- ships unpatched.
//!
//! What upstream does and this keeps:
//!
//! - **Its own option parser.** An option is known by its first two
//!   characters, so `-a`, `-autoskip` and `-abc` all toggle autoskip, and a
//!   `--` in front of a word is dropped to one: `--cols` is `-cols`. Options
//!   end at the first operand; a value is attached (`-c8`) or the next word,
//!   except that a word continuing the option's long name (`-cols`) always
//!   takes the next word. `-h` is no option at all, and like any unknown one
//!   prints the usage and exits 1.
//! - **Numbers are `strtol (arg, NULL, 0)`**, with nothing checked: `-c 0x10`
//!   is sixteen, `-c 1x` one, `-c x` zero -- and zero columns is the default.
//! - **A line is built in one buffer**, as upstream's static `l[]` is: the
//!   offset, spaces to the end, then each byte's digits and character at
//!   computed positions. A short last line therefore keeps its alignment, and
//!   with colour the same arithmetic places the escape sequences -- including
//!   where it pads a short last line with red spaces.
//! - **`-a` squeezes** three or more all-zero lines to `*` with upstream's
//!   state machine (`xxdline`), which keeps the last line of a file whole.
//! - **`-r` is upstream's `huntype`**: it reads the offset before each line's
//!   first non-hex character, takes digits in pairs, and gives up on a line
//!   at two adjacent non-hex characters. It seeks the output where it can and
//!   writes zeros where it cannot, refuses to go backwards on a pipe
//!   (`Sorry, cannot seek backwards.`, status 5), and rewinds its input
//!   first -- standard input included, which moves the position the shell's
//!   next command reads from. A named output is patched, not truncated.
//! - **`-s` seeks the input**, from the start, the end (`-s -N`) or where it
//!   is (`-s +N`), and on a pipe reads its way forward instead.
//! - **stdio's buffering on both sides**: input a block at a time, which a
//!   shared standard input loses to the next reader, since `fclose` gives
//!   nothing back; output buffered by the descriptor's block size, a line at
//!   a time on a terminal.
//! - **Colour** (`-R`, default `auto`) when standard output is a terminal and
//!   `NO_COLOR` is unset or empty -- decided by standard output even when the
//!   dump goes to a named file, as upstream decides it.
//! - **Upstream's diagnostics and statuses**: 1 for usage and bad columns, 2
//!   for the input, 3 for the output, 4 for a seek, 5 for a backward one, and
//!   255 (`exit (-1)`) for `-r` of a type it cannot revert. stderr is not
//!   checked: upstream never looks at what its own messages came to.
//!
//! # Deliberate differences
//!
//! - `-v`, and the usage line that quotes it, name SlateOS coreutils, as
//!   every program here does.
//! - Upstream's line buffer is a fixed 2582 bytes, which colour overruns past
//!   about a hundred columns, and a negative decimal offset (`-d -o -1`)
//!   past 253 -- undefined behaviour in C. Here such a line is as long as it
//!   needs to be.

use std::io::{self, SeekFrom};
use std::process::ExitCode;

use coreutils::quote::{os_bytes, os_from_bytes};
use coreutils::stdfd;
use coreutils::stdio::{StdioFile, StdioReader};
use cstrtol::{low_i32, strtol, strtoul};

coreutils::guard_std_fds!();

/// What `-v` prints, and the usage text quotes. Upstream's is
/// `xxd 2023-10-25 by Juergen Weigert et al.`
const VERSION: &str = "xxd from SlateOS coreutils 0.1.0";

/// `COLS`: the most columns a hex, bits or little-endian dump may have.
const COLS: i64 = 256;

/// `LLEN`: upstream's line buffer, less its terminator --
/// `2 * sizeof (unsigned long) + 4 + (9 * COLS - 1) + COLS + 2`.
const LLEN: usize = 2 * 8 + 4 + (9 * 256 - 1) + 256 + 2;

/// `hexxa`: the lower-case digits, then the upper-case ones `-u` selects.
const HEXXA: &[u8; 32] = b"0123456789abcdef0123456789ABCDEF";

/// The kinds of dump.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum HexType {
    /// `HEX_NORMAL`: offset, grouped hex, characters.
    Normal,
    /// `HEX_POSTSCRIPT`, `-p`: the hex digits alone.
    Postscript,
    /// `HEX_CINCLUDE`, `-i`: a C array.
    CInclude,
    /// `HEX_BITS`, `-b`: binary digits.
    Bits,
    /// `HEX_LITTLEENDIAN`, `-e`: each group's bytes reversed.
    LittleEndian,
}

/// The colour digits of `ESC [ 1 ; 3 <digit> m`.
const COLOR_RED: u8 = b'1';
const COLOR_GREEN: u8 = b'2';
const COLOR_YELLOW: u8 = b'3';
const COLOR_BLUE: u8 = b'4';
const COLOR_WHITE: u8 = b'7';

/// `etoa64`: EBCDIC 0x40 to 0xff in ASCII ("a proposed BTL standard April
/// 16, 1979"). Below 0x40 is shown as `.`.
const ETOA64: [u8; 192] = [
    0o040, 0o240, 0o241, 0o242, 0o243, 0o244, 0o245, 0o246, 0o247, 0o250, 0o325, 0o056, 0o074,
    0o050, 0o053, 0o174, 0o046, 0o251, 0o252, 0o253, 0o254, 0o255, 0o256, 0o257, 0o260, 0o261,
    0o041, 0o044, 0o052, 0o051, 0o073, 0o176, 0o055, 0o057, 0o262, 0o263, 0o264, 0o265, 0o266,
    0o267, 0o270, 0o271, 0o313, 0o054, 0o045, 0o137, 0o076, 0o077, 0o272, 0o273, 0o274, 0o275,
    0o276, 0o277, 0o300, 0o301, 0o302, 0o140, 0o072, 0o043, 0o100, 0o047, 0o075, 0o042, 0o303,
    0o141, 0o142, 0o143, 0o144, 0o145, 0o146, 0o147, 0o150, 0o151, 0o304, 0o305, 0o306, 0o307,
    0o310, 0o311, 0o312, 0o152, 0o153, 0o154, 0o155, 0o156, 0o157, 0o160, 0o161, 0o162, 0o136,
    0o314, 0o315, 0o316, 0o317, 0o320, 0o321, 0o345, 0o163, 0o164, 0o165, 0o166, 0o167, 0o170,
    0o171, 0o172, 0o322, 0o323, 0o324, 0o133, 0o326, 0o327, 0o330, 0o331, 0o332, 0o333, 0o334,
    0o335, 0o336, 0o337, 0o340, 0o341, 0o342, 0o343, 0o344, 0o135, 0o346, 0o347, 0o173, 0o101,
    0o102, 0o103, 0o104, 0o105, 0o106, 0o107, 0o110, 0o111, 0o350, 0o351, 0o352, 0o353, 0o354,
    0o355, 0o175, 0o112, 0o113, 0o114, 0o115, 0o116, 0o117, 0o120, 0o121, 0o122, 0o356, 0o357,
    0o360, 0o361, 0o362, 0o363, 0o134, 0o237, 0o123, 0o124, 0o125, 0o126, 0o127, 0o130, 0o131,
    0o132, 0o364, 0o365, 0o366, 0o367, 0o370, 0o371, 0o060, 0o061, 0o062, 0o063, 0o064, 0o065,
    0o066, 0o067, 0o070, 0o071, 0o372, 0o373, 0o374, 0o375, 0o376, 0o377,
];

/// The usage text's option lines, but the last, which quotes [`VERSION`].
const USAGE_OPTIONS: &str = "Options:
    -a          toggle autoskip: A single '*' replaces nul-lines. Default off.
    -b          binary digit dump (incompatible with -ps,-i). Default hex.
    -C          capitalize variable names in C include file style (-i).
    -c cols     format <cols> octets per line. Default 16 (-i: 12, -ps: 30).
    -E          show characters in EBCDIC. Default ASCII.
    -e          little-endian dump (incompatible with -ps,-i,-r).
    -g bytes    number of octets per group in normal output. Default 2 (-e: 4).
    -h          print this summary.
    -i          output in C include file style.
    -l len      stop after <len> octets.
    -n name     set the variable name used in C include output (-i).
    -o off      add <off> to the displayed file position.
    -ps         output in postscript plain hexdump style.
    -r          reverse operation: convert (or patch) hexdump into binary.
    -r -s off   revert with <off> added to file positions found in hexdump.
    -d          show offset in decimal instead of hex.
    -s [+][-]seek  start at <seek> bytes abs. (or +: rel.) infile offset.
    -u          use upper case hex letters.
    -R when     colorize the output; <when> can be 'always', 'auto' or 'never'. Default: 'auto'.
";

/// A run that has ended: its diagnostic, if any, is already out, and this is
/// the status `exit` was called with.
#[derive(Debug)]
struct Die(u8);

/// The options, once parsed: upstream's locals of `main`.
#[derive(Debug)]
struct Opts {
    hextype: HexType,
    cols: i64,
    octspergrp: i64,
    autoskip: bool,
    upper: bool,
    capitalize: bool,
    decimal_offset: bool,
    revert: bool,
    ebcdic: bool,
    color: bool,
    /// `-l`: negative for all.
    length: i64,
    seekoff: i64,
    relseek: bool,
    negseek: bool,
    displayoff: u64,
    varname: Option<Vec<u8>>,
}

/// The two streams and the name the diagnostics use.
struct Xxd {
    pname: Vec<u8>,
    /// `fp`; `None` once closed.
    input: Option<StdioReader>,
    /// `fpo`; `None` once closed.
    out: Option<StdioFile>,
}

/// glibc's `strerror` wording.
fn reason(e: &io::Error) -> String {
    coreutils::errmsg::strerror(e)
}

/// A diagnostic, to descriptor 2 as `fprintf (stderr, ...)` puts it.
fn say(bytes: &[u8]) {
    // Upstream checks none of its writes to stderr, and its status does not
    // depend on them: there is nothing to do with a failure here either.
    drop(stdfd::write_all(2, bytes));
}

/// The text of the C string in `s`: up to its first NUL.
fn c_str(s: &[u8]) -> &[u8] {
    let end = s.iter().position(|&b| b == 0).unwrap_or(s.len());
    s.get(..end).unwrap_or_default()
}

impl Xxd {
    /// `exit_with_usage`: the summary on stderr, and status 1.
    fn usage(&self) -> Die {
        let p = self.pname.as_slice();
        let mut m: Vec<u8> = Vec::new();
        m.extend_from_slice(b"Usage:\n       ");
        m.extend_from_slice(p);
        m.extend_from_slice(b" [options] [infile [outfile]]\n    or\n       ");
        m.extend_from_slice(p);
        m.extend_from_slice(b" -r [-s [-]offset] [-c cols] [-ps] [infile [outfile]]\n");
        m.extend_from_slice(USAGE_OPTIONS.as_bytes());
        m.extend_from_slice(format!("    -v          show version: \"{VERSION}\".\n").as_bytes());
        say(&m);
        Die(1)
    }

    /// `perror_exit`: `xxd: REASON`, and `ret`.
    fn perror_exit(&self, ret: u8, e: &io::Error) -> Die {
        let mut m = self.pname.clone();
        m.extend_from_slice(b": ");
        m.extend_from_slice(reason(e).as_bytes());
        m.push(b'\n');
        say(&m);
        Die(ret)
    }

    /// `error_exit`: `xxd: MESSAGE`, and `ret`.
    fn error_exit(&self, ret: u8, msg: &str) -> Die {
        let mut m = self.pname.clone();
        m.extend_from_slice(b": ");
        m.extend_from_slice(msg.as_bytes());
        m.push(b'\n');
        say(&m);
        Die(ret)
    }

    /// `fprintf (stderr, "%s: ", pname); perror (name)`.
    fn perror_named(&self, name: &[u8], e: &io::Error) {
        let mut m = self.pname.clone();
        m.extend_from_slice(b": ");
        m.extend_from_slice(name);
        m.extend_from_slice(b": ");
        m.extend_from_slice(reason(e).as_bytes());
        m.push(b'\n');
        say(&m);
    }

    /// `putc_or_die`, `fputs_or_die` and `FPRINTF_OR_DIE`: bytes onto the
    /// output, a failure being status 3.
    fn put(&mut self, bytes: &[u8]) -> Result<(), Die> {
        let result = match self.out.as_mut() {
            Some(out) => out.write(bytes),
            None => Err(io::Error::from_raw_os_error(9)),
        };
        result.map_err(|e| self.perror_exit(3, &e))
    }

    /// `getc_or_die`: the next input byte, `None` at the end, a read error
    /// being status 2.
    fn getc_or_die(&mut self) -> Result<Option<u8>, Die> {
        let result = match self.input.as_mut() {
            Some(input) => input.getc(),
            None => Err(io::Error::from_raw_os_error(9)),
        };
        result.map_err(|e| self.perror_exit(2, &e))
    }

    /// Plain `getc`, as `huntype`'s loop calls it: a read error ends the
    /// input as its end does.
    fn getc(&mut self) -> Option<u8> {
        self.input.as_mut().and_then(|i| i.getc().ok().flatten())
    }

    /// `fflush (fpo)`, its failure being status 3.
    fn flush_or_die(&mut self) -> Result<(), Die> {
        let result = match self.out.as_mut() {
            Some(out) => out.flush(),
            None => Ok(()),
        };
        result.map_err(|e| self.perror_exit(3, &e))
    }

    /// `fclose_or_die (fpi, fpo)`: the output closed, then the input.
    fn fclose_or_die(&mut self) -> Result<(), Die> {
        if let Some(mut out) = self.out.take() {
            if let Err(e) = out.close() {
                return Err(self.perror_exit(3, &e));
            }
        }
        if let Some(input) = self.input.take() {
            if let Err(e) = input.fclose() {
                return Err(self.perror_exit(2, &e));
            }
        }
        Ok(())
    }

    /// What `exit` does to the streams still open: write out what the
    /// output holds, and give a shared input back what was read ahead.
    fn exit_cleanup(&mut self) {
        if let Some(out) = self.out.as_mut() {
            // `exit` checks nothing: a failure here has nowhere to go.
            drop(out.flush());
        }
        if let Some(input) = self.input.as_mut() {
            input.exit_sync();
        }
    }

    /// `skip_to_eol`: discard input through the end of the line, returning
    /// the newline or `None` at the end of the input.
    fn skip_to_eol(&mut self, mut c: Option<u8>) -> Result<Option<u8>, Die> {
        while c.is_some() && c != Some(b'\n') {
            c = self.getc_or_die()?;
        }
        Ok(c)
    }

    /// `huntype`: turn a dump back into binary.
    fn huntype(&mut self, cols: i64, hextype: HexType, base_off: i64) -> Result<u8, Die> {
        let mut ign_garb = true;
        let (mut n1, mut n2, mut n3): (i32, i32, i32) = (-1, 0, 0);
        let mut p = cols;
        let mut b: i32 = 0;
        let mut bcnt: i32 = 0;
        let mut have_off: i64 = 0;
        let mut want_off: i64 = 0;

        if let Some(input) = self.input.as_mut() {
            input.rewind();
        }

        while let Some(byte) = self.getc() {
            let mut c = Some(byte);
            if byte == b'\r' {
                // Doze style input file?
                continue;
            }
            // Multiple spaces are allowed only in the plain format, where no
            // text follows the digits that could look like them.
            if hextype == HexType::Postscript && matches!(byte, b' ' | b'\n' | b'\t') {
                continue;
            }
            if hextype == HexType::Normal || hextype == HexType::Postscript {
                n3 = n2;
                n2 = n1;
                n1 = parse_hex_digit(byte);
                if n1 == -1 && ign_garb {
                    continue;
                }
            } else {
                n1 = parse_hex_digit(byte);
                if n1 == -1 && ign_garb {
                    continue;
                }
                // `b = ((b << 1) | bt)`: only the low eight bits are ever
                // written, so what is shifted out is never missed.
                if let Some(bt) = parse_bin_digit(byte) {
                    b = b.wrapping_shl(1) | bt;
                    bcnt = bcnt.wrapping_add(1);
                }
            }

            ign_garb = false;

            if hextype != HexType::Postscript && p >= cols {
                // Still in the offset, which ends at the first non-digit.
                if n1 < 0 {
                    p = 0;
                    if hextype == HexType::Bits {
                        bcnt = 0;
                    }
                    continue;
                }
                want_off = want_off.wrapping_shl(4) | i64::from(n1);
                continue;
            }

            let target = base_off.wrapping_add(want_off);
            if target != have_off {
                self.flush_or_die()?;
                let delta = target.wrapping_sub(have_off);
                if self
                    .out
                    .as_mut()
                    .is_some_and(|o| o.seek(SeekFrom::Current(delta)).is_ok())
                {
                    have_off = target;
                }
                if target < have_off {
                    return Err(self.error_exit(5, "Sorry, cannot seek backwards."));
                }
                while have_off < target {
                    self.put(&[0])?;
                    have_off = have_off.wrapping_add(1);
                }
            }

            if hextype == HexType::Normal || hextype == HexType::Postscript {
                if n2 >= 0 && n1 >= 0 {
                    self.put(&[nibbles(n2, n1)])?;
                    have_off = have_off.wrapping_add(1);
                    want_off = want_off.wrapping_add(1);
                    n1 = -1;
                    if hextype == HexType::Normal {
                        p = p.wrapping_add(1);
                        if p >= cols {
                            // The rest of the line is the characters.
                            c = self.skip_to_eol(c)?;
                        }
                    }
                } else if n1 < 0 && n2 < 0 && n3 < 0 {
                    // Already stumbled into garbage: skip the line.
                    c = self.skip_to_eol(c)?;
                }
            } else if bcnt == 8 {
                self.put(&[b.to_le_bytes()[0]])?;
                have_off = have_off.wrapping_add(1);
                want_off = want_off.wrapping_add(1);
                b = 0;
                bcnt = 0;
                p = p.wrapping_add(1);
                if p >= cols {
                    c = self.skip_to_eol(c)?;
                }
            }

            if c == Some(b'\n') {
                if hextype == HexType::Normal || hextype == HexType::Bits {
                    want_off = 0;
                }
                p = cols;
                ign_garb = true;
            }
        }
        self.flush_or_die()?;
        if let Some(out) = self.out.as_mut() {
            // `fseek (fpo, 0L, SEEK_END)`, unchecked: it only leaves a shared
            // output's position at its end.
            drop(out.seek(SeekFrom::End(0)));
        }
        self.fclose_or_die()?;
        Ok(0)
    }

    /// `xxdline`: print line `l` -- or, with `nz` zero, perhaps not. Three or
    /// more zero lines in a row become one `*`; `nz` negative is the end of
    /// the input, which shows the last line even when it is all zeros.
    fn xxdline(&mut self, sq: &mut Squeeze, l: &[u8], nz: i64) -> Result<(), Die> {
        let l = c_str(l);
        if nz == 0 && sq.zero_seen == 1 {
            sq.z = l.to_vec();
        }
        let shown = if nz != 0 {
            true
        } else {
            let before = sq.zero_seen;
            sq.zero_seen = sq.zero_seen.wrapping_add(1);
            before == 0
        };
        if shown {
            if nz != 0 {
                if nz < 0 {
                    sq.zero_seen = sq.zero_seen.wrapping_sub(1);
                }
                if sq.zero_seen == 2 {
                    let z = sq.z.clone();
                    self.put(&z)?;
                }
                if sq.zero_seen > 2 {
                    self.put(b"*\n")?;
                }
            }
            if nz >= 0 || sq.zero_seen > 0 {
                self.put(l)?;
            }
            if nz != 0 {
                sq.zero_seen = 0;
            }
        }
        Ok(())
    }
}

/// `xxdline`'s statics: the second of a run of zero lines, and how many have
/// been seen.
#[derive(Default)]
struct Squeeze {
    z: Vec<u8>,
    zero_seen: i32,
}

/// C's `a / b` on the line arithmetic. `b` is a group size or a column
/// count, never zero by the time a line is built; 0 stands in if it were.
fn div(a: i64, b: i64) -> i64 {
    a.checked_div(b).unwrap_or(0)
}

/// `parse_hex_digit`: the digit's value, or -1.
fn parse_hex_digit(c: u8) -> i32 {
    char::from(c)
        .to_digit(16)
        .and_then(|d| i32::try_from(d).ok())
        .unwrap_or(-1)
}

/// `parse_bin_digit`: 0 or 1, or `None` for anything else.
fn parse_bin_digit(c: u8) -> Option<i32> {
    match c {
        b'0' => Some(0),
        b'1' => Some(1),
        _ => None,
    }
}

/// The byte two hex digits make, `(hi << 4) | lo`.
fn nibbles(hi: i32, lo: i32) -> u8 {
    (hi.wrapping_shl(4) | lo).to_le_bytes()[0]
}

/// `l[c] = b` on the line buffer, growing it where upstream's fixed one
/// would have been overrun.
fn set(l: &mut Vec<u8>, at: i64, b: u8) {
    let Ok(i) = usize::try_from(at) else {
        return;
    };
    if i >= l.len() {
        l.resize(i.saturating_add(1), 0);
    }
    if let Some(slot) = l.get_mut(i) {
        *slot = b;
    }
}

/// `l[c++] = b`.
fn push(l: &mut Vec<u8>, c: &mut i64, b: u8) {
    set(l, *c, b);
    *c = c.wrapping_add(1);
}

/// `COLOR_PROLOGUE`: `ESC [ 1 ; 3`, before the colour digit.
fn prologue(l: &mut Vec<u8>, c: &mut i64) {
    for &b in b"\x1b[1;3" {
        push(l, c, b);
    }
}

/// `COLOR_EPILOGUE`: `ESC [ 0 m`.
fn epilogue(l: &mut Vec<u8>, c: &mut i64) {
    for &b in b"\x1b[0m" {
        push(l, c, b);
    }
}

/// `begin_coloring_char`: the colour of byte `e`, then `m`. In ASCII,
/// printable is green, tab, newline and return yellow, NUL white, 0xff blue
/// and the rest red; in EBCDIC the same classes by EBCDIC's code points.
fn begin_coloring_char(l: &mut Vec<u8>, c: &mut i64, e: u8, ebcdic: bool) {
    let colour = if ebcdic {
        if matches!(
            e,
            75..=80 | 90..=97 | 107..=111 | 121..=127 | 129..=137 | 145..=154 | 162..=169 | 192..=201
                | 208..=217 | 226..=233 | 240..=249 | 189 | 64 | 173 | 224
        ) {
            COLOR_GREEN
        } else if matches!(e, 37 | 13 | 5) {
            COLOR_YELLOW
        } else if e == 0 {
            COLOR_WHITE
        } else if e == 255 {
            COLOR_BLUE
        } else {
            COLOR_RED
        }
    } else if (32..127).contains(&e) {
        COLOR_GREEN
    } else if matches!(e, 9 | 10 | 13) {
        COLOR_YELLOW
    } else if e == 0 {
        COLOR_WHITE
    } else if e == 255 {
        COLOR_BLUE
    } else {
        COLOR_RED
    };
    push(l, c, colour);
    push(l, c, b'm');
}

/// The character column's byte: EBCDIC translated if asked, and anything
/// unprintable a dot.
fn shown_char(e: u8, ebcdic: bool) -> u8 {
    let e = if ebcdic {
        if e < 64 {
            b'.'
        } else {
            ETOA64
                .get(usize::from(e).saturating_sub(64))
                .copied()
                .unwrap_or(b'.')
        }
    } else {
        e
    };
    if (32..127).contains(&e) { e } else { b'.' }
}

/// `isatty (STDOUT_FILENO)`: `enable_color` on a Unix system.
fn enable_color() -> bool {
    stdfd::is_tty(1)
}

/// `(int) strtol (s, NULL, 0)`.
fn int_arg(s: &[u8]) -> i64 {
    i64::from(low_i32(strtol(s, 0).0))
}

/// `strtol (s, NULL, 0)`.
fn long_arg(s: &[u8]) -> i64 {
    strtol(s, 0).0
}

fn main() -> ExitCode {
    stdfd::restore();
    let args: Vec<Vec<u8>> = std::env::args_os()
        .map(|a| os_bytes(&a).into_owned())
        .collect();
    // `pname`: the program name after its last `/`.
    let pname = args.first().map_or_else(Vec::new, |a0| {
        let start = a0
            .iter()
            .rposition(|&c| c == b'/')
            .map_or(0, |i| i.saturating_add(1));
        a0.get(start..).unwrap_or_default().to_vec()
    });
    let mut x = Xxd {
        pname,
        input: None,
        out: None,
    };
    let status = match run(&mut x, &args) {
        Ok(code) | Err(Die(code)) => code,
    };
    x.exit_cleanup();
    // Whatever is still open stays open, as `exit` leaves it for the kernel.
    // Closing it here is not merely redundant: with standard output closed
    // at the start, the input file opens *as* descriptor 1, and
    // `fclose (stdout)` has then closed it under the input stream -- which
    // glibc shrugs off, and Rust's drop of the input's `File` answers by
    // aborting the process (an I/O-safety violation) instead of exiting 3.
    std::mem::forget(x);
    ExitCode::from(status)
}

/// Upstream's `main`, from its option loop on.
fn run(x: &mut Xxd, args: &[Vec<u8>]) -> Result<u8, Die> {
    let mut o = Opts {
        hextype: HexType::Normal,
        cols: 0,
        octspergrp: -1,
        autoskip: false,
        upper: false,
        capitalize: false,
        decimal_offset: false,
        revert: false,
        ebcdic: false,
        color: false,
        length: -1,
        seekoff: 0,
        relseek: true,
        negseek: false,
        displayoff: 0,
        varname: None,
    };
    let mut colsgiven = false;

    // `NO_COLOR` set and not empty turns off the default; `-R` still decides.
    let no_color = std::env::var_os("NO_COLOR");
    if no_color.as_ref().is_none_or(|v| v.is_empty()) {
        o.color = enable_color();
    }

    // `argv[1]` is `args[a]`, and `argc` is `args.len () - a + 1`.
    let mut a = 1usize;
    while let Some(arg1) = args.get(a) {
        let pp: &[u8] = if arg1.starts_with(b"--") && arg1.len() > 2 {
            arg1.get(1..).unwrap_or_default()
        } else {
            arg1
        };
        let rest = pp.get(2..).unwrap_or_default();
        let argv2 = args.get(a.saturating_add(1));
        if pp.starts_with(b"-a") {
            o.autoskip = !o.autoskip;
        } else if pp.starts_with(b"-b") {
            o.hextype = HexType::Bits;
        } else if pp.starts_with(b"-e") {
            o.hextype = HexType::LittleEndian;
        } else if pp.starts_with(b"-u") {
            o.upper = true;
        } else if pp.starts_with(b"-p") {
            o.hextype = HexType::Postscript;
        } else if pp.starts_with(b"-i") {
            o.hextype = HexType::CInclude;
        } else if pp.starts_with(b"-C") {
            o.capitalize = true;
        } else if pp.starts_with(b"-d") {
            o.decimal_offset = true;
        } else if pp.starts_with(b"-r") {
            o.revert = true;
        } else if pp.starts_with(b"-E") {
            o.ebcdic = true;
        } else if pp.starts_with(b"-v") {
            say(format!("{VERSION}\n").as_bytes());
            return Err(Die(0));
        } else if pp.starts_with(b"-c") {
            if !rest.is_empty() && rest.starts_with(b"apitalize") {
                o.capitalize = true;
            } else if !rest.is_empty() && !rest.starts_with(b"ols") {
                colsgiven = true;
                o.cols = int_arg(rest);
            } else {
                let Some(v) = argv2 else {
                    return Err(x.usage());
                };
                colsgiven = true;
                o.cols = int_arg(v);
                a = a.saturating_add(1);
            }
        } else if pp.starts_with(b"-g") {
            if !rest.is_empty() && !rest.starts_with(b"roup") {
                o.octspergrp = int_arg(rest);
            } else {
                let Some(v) = argv2 else {
                    return Err(x.usage());
                };
                o.octspergrp = int_arg(v);
                a = a.saturating_add(1);
            }
        } else if pp.starts_with(b"-o") {
            if !rest.is_empty() && !rest.starts_with(b"ffset") {
                o.displayoff = strtoul(rest, 0).0;
            } else {
                let Some(v) = argv2 else {
                    return Err(x.usage());
                };
                let rel = usize::from(v.first() == Some(&b'+'));
                let neg = usize::from(v.get(rel) == Some(&b'-'));
                let n = strtoul(v.get(rel.saturating_add(neg)..).unwrap_or_default(), 0).0;
                // `ULONG_MAX - n + 1`.
                o.displayoff = if neg == 1 { n.wrapping_neg() } else { n };
                a = a.saturating_add(1);
            }
        } else if pp.starts_with(b"-s") {
            o.relseek = false;
            o.negseek = false;
            let attached =
                !rest.is_empty() && !rest.starts_with(b"kip") && !rest.starts_with(b"eek");
            let v = if attached {
                rest
            } else {
                let Some(v) = argv2 else {
                    return Err(x.usage());
                };
                a = a.saturating_add(1);
                v.as_slice()
            };
            if v.first() == Some(&b'+') {
                o.relseek = true;
            }
            let rel = usize::from(o.relseek);
            if v.get(rel) == Some(&b'-') {
                o.negseek = true;
            }
            let skip = rel.saturating_add(usize::from(o.negseek));
            o.seekoff = long_arg(v.get(skip..).unwrap_or_default());
        } else if pp.starts_with(b"-l") {
            if !rest.is_empty() && !rest.starts_with(b"en") {
                o.length = long_arg(rest);
            } else {
                let Some(v) = argv2 else {
                    return Err(x.usage());
                };
                o.length = long_arg(v);
                a = a.saturating_add(1);
            }
        } else if pp.starts_with(b"-n") {
            if !rest.is_empty() && !rest.starts_with(b"ame") {
                o.varname = Some(rest.to_vec());
            } else {
                let Some(v) = argv2 else {
                    return Err(x.usage());
                };
                o.varname = Some(v.clone());
                a = a.saturating_add(1);
            }
        } else if pp.starts_with(b"-R") {
            let pw = if rest.is_empty() {
                a = a.saturating_add(1);
                match argv2 {
                    Some(v) => v.as_slice(),
                    None => return Err(x.usage()),
                }
            } else {
                rest
            };
            if pw.starts_with(b"always") {
                o.color = true;
            } else if pw.starts_with(b"never") {
                o.color = false;
            } else if pw.starts_with(b"auto") {
                o.color = enable_color();
            } else {
                return Err(x.usage());
            }
        } else if pp == b"--" {
            // The end of the options.
            a = a.saturating_add(1);
            break;
        } else if pp.first() == Some(&b'-') && pp.len() > 1 {
            // An unknown option.
            return Err(x.usage());
        } else {
            // Not an option.
            break;
        }
        a = a.saturating_add(1);
    }
    let operands = args.get(a..).unwrap_or_default();

    if !colsgiven || (o.cols == 0 && o.hextype != HexType::Postscript) {
        o.cols = match o.hextype {
            HexType::Postscript => 30,
            HexType::CInclude => 12,
            HexType::Bits => 6,
            HexType::Normal | HexType::LittleEndian => 16,
        };
    }
    if o.octspergrp < 0 {
        o.octspergrp = match o.hextype {
            HexType::Bits => 1,
            HexType::Normal => 2,
            HexType::LittleEndian => 4,
            HexType::Postscript | HexType::CInclude => 0,
        };
    }
    if (o.hextype == HexType::Postscript && o.cols < 0)
        || (o.hextype != HexType::Postscript && o.cols < 1)
        || (matches!(
            o.hextype,
            HexType::Normal | HexType::Bits | HexType::LittleEndian
        ) && o.cols > COLS)
    {
        let mut m = x.pname.clone();
        m.extend_from_slice(format!(": invalid number of columns (max. {COLS}).\n").as_bytes());
        say(&m);
        return Err(Die(1));
    }
    if o.octspergrp < 1 || o.octspergrp > o.cols {
        o.octspergrp = o.cols;
    } else if o.hextype == HexType::LittleEndian
        && (o.octspergrp & o.octspergrp.wrapping_sub(1)) != 0
    {
        return Err(x.error_exit(
            1,
            "number of octets per group must be a power of 2 with -e.",
        ));
    }

    if operands.len() > 2 {
        return Err(x.usage());
    }

    let is_dash = |s: &[u8]| s == b"-";
    let infile = operands.first().filter(|f| !is_dash(f));
    match infile {
        None => x.input = Some(StdioReader::stdin()),
        Some(name) => match std::fs::File::open(os_from_bytes(name)) {
            Ok(f) => x.input = Some(StdioReader::from_file(f)),
            Err(e) => {
                x.perror_named(name, &e);
                return Err(Die(2));
            }
        },
    }
    match operands.get(1).filter(|f| !is_dash(f)) {
        None => x.out = Some(StdioFile::stdout()),
        Some(name) => {
            let opened = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(!o.revert)
                .open(os_from_bytes(name));
            match opened {
                Ok(f) => {
                    let mut out = StdioFile::from_file(f);
                    // `rewind (fpo)`: a file just opened is at its start, so
                    // nothing can come of this.
                    drop(out.seek(SeekFrom::Start(0)));
                    x.out = Some(out);
                }
                Err(e) => {
                    x.perror_named(name, &e);
                    return Err(Die(3));
                }
            }
        }
    }

    if o.revert {
        return match o.hextype {
            HexType::Normal | HexType::Postscript | HexType::Bits => {
                let base = if o.negseek {
                    o.seekoff.wrapping_neg()
                } else {
                    o.seekoff
                };
                x.huntype(o.cols, o.hextype, base)
            }
            HexType::CInclude | HexType::LittleEndian => {
                // `error_exit (-1, ...)`: `exit (-1)` is status 255.
                Err(x.error_exit(255, "Sorry, cannot revert this type of hexdump"))
            }
        };
    }

    if o.seekoff != 0 || o.negseek || !o.relseek {
        seek_input(x, &mut o)?;
    }

    match o.hextype {
        HexType::CInclude => {
            if o.varname.is_none() {
                // A name the user set overrides standard input's having none.
                o.varname = infile.cloned();
            }
            c_include(x, &o)
        }
        HexType::Postscript => postscript(x, &o),
        HexType::Normal | HexType::Bits | HexType::LittleEndian => dump(x, &o),
    }
}

/// `-s`: `fseek` the input, and failing that read forward to the offset --
/// which a seek from the end cannot do.
fn seek_input(x: &mut Xxd, o: &mut Opts) -> Result<(), Die> {
    let off = if o.negseek {
        o.seekoff.wrapping_neg()
    } else {
        o.seekoff
    };
    let to = if o.relseek {
        Some(SeekFrom::Current(off))
    } else if o.negseek {
        Some(SeekFrom::End(off))
    } else {
        // A position before the start is `EINVAL` from `lseek`.
        u64::try_from(off).ok().map(SeekFrom::Start)
    };
    let sought = match (to, x.input.as_mut()) {
        (Some(to), Some(input)) => input.seek(to).ok(),
        _ => None,
    };
    match sought {
        Some(at) => {
            // `seekoff = ftell (fp)`.
            o.seekoff = i64::try_from(at).unwrap_or(i64::MAX);
        }
        None if o.negseek => return Err(x.error_exit(4, "Sorry, cannot seek.")),
        None => {
            let mut s = o.seekoff;
            while s != 0 {
                s = s.wrapping_sub(1);
                if x.getc_or_die()?.is_none() {
                    return Err(x.error_exit(4, "Sorry, cannot seek."));
                }
            }
        }
    }
    Ok(())
}

/// The bytes still to read under `-l`: `length < 0 || n < length`.
fn more(length: i64, n: i64) -> bool {
    length < 0 || n < length
}

/// `-i`: the input as a C array, and its length.
fn c_include(x: &mut Xxd, o: &Opts) -> Result<u8, Die> {
    let ident = |name: &[u8]| -> Vec<u8> {
        let mut v = Vec::with_capacity(name.len().saturating_add(2));
        if name.first().is_some_and(u8::is_ascii_digit) {
            v.extend_from_slice(b"__");
        }
        for &c in name {
            v.push(if c.is_ascii_alphanumeric() {
                if o.capitalize {
                    c.to_ascii_uppercase()
                } else {
                    c
                }
            } else {
                b'_'
            });
        }
        v
    };
    if let Some(name) = &o.varname {
        let mut head = b"unsigned char ".to_vec();
        head.extend_from_slice(&ident(name));
        x.put(&head)?;
        x.put(b"[] = {\n")?;
    }
    let mut p: i32 = 0;
    while more(o.length, i64::from(p)) {
        let Some(c) = x.getc_or_die()? else {
            break;
        };
        let lead: &str = if p.checked_rem(low_i32(o.cols)).unwrap_or(0) != 0 {
            ", "
        } else if p == 0 {
            "  "
        } else {
            ",\n  "
        };
        let item = if o.upper {
            format!("{lead}0X{c:02X}")
        } else {
            format!("{lead}0x{c:02x}")
        };
        x.put(item.as_bytes())?;
        p = p.wrapping_add(1);
    }
    if p != 0 {
        x.put(b"\n")?;
    }
    if let Some(name) = &o.varname {
        x.put(b"};\n")?;
        let mut tail = b"unsigned int ".to_vec();
        tail.extend_from_slice(&ident(name));
        x.put(&tail)?;
        x.put(format!("_{} = {p};\n", if o.capitalize { "LEN" } else { "len" }).as_bytes())?;
    }
    x.fclose_or_die()?;
    Ok(0)
}

/// `-p`: the hex digits alone, `cols` bytes to a line -- or one line, for 0.
fn postscript(x: &mut Xxd, o: &Opts) -> Result<u8, Die> {
    let hexx = digits(o.upper);
    let mut p = o.cols;
    let mut n: i64 = 0;
    while more(o.length, n) {
        let Some(e) = x.getc_or_die()? else {
            break;
        };
        x.put(&[hex_hi(hexx, e)])?;
        x.put(&[hex_lo(hexx, e)])?;
        n = n.wrapping_add(1);
        if o.cols > 0 {
            p = p.wrapping_sub(1);
            if p == 0 {
                x.put(b"\n")?;
                p = o.cols;
            }
        }
    }
    if o.cols == 0 || p < o.cols {
        x.put(b"\n")?;
    }
    x.fclose_or_die()?;
    Ok(0)
}

/// `hexx`: the sixteen digits in use.
fn digits(upper: bool) -> &'static [u8] {
    if upper {
        HEXXA.get(16..).unwrap_or_default()
    } else {
        HEXXA.get(..16).unwrap_or_default()
    }
}

/// `hexx[(e >> 4) & 0xf]`.
fn hex_hi(hexx: &[u8], e: u8) -> u8 {
    hexx.get(usize::from(e >> 4)).copied().unwrap_or(b'0')
}

/// `hexx[e & 0xf]`.
fn hex_lo(hexx: &[u8], e: u8) -> u8 {
    hexx.get(usize::from(e & 0xf)).copied().unwrap_or(b'0')
}

/// The hex, bits and little-endian dumps.
fn dump(x: &mut Xxd, o: &Opts) -> Result<u8, Die> {
    let hexx = digits(o.upper);
    let cols = o.cols;
    let g = o.octspergrp;
    let le = o.hextype == HexType::LittleEndian;
    let bits = o.hextype == HexType::Bits;
    // Characters per group of octets.
    let grplen: i64 = if bits {
        8_i64.saturating_mul(g).saturating_add(1)
    } else if o.color {
        // Each byte's colour costs eleven more characters.
        g.saturating_mul(2)
            .saturating_add(1)
            .saturating_add(g.saturating_mul(11))
    } else {
        g.saturating_mul(2).saturating_add(1)
    };
    // How far the line can reach: upstream's `LLEN`, unless colour or a long
    // offset takes it further (where upstream's buffer would overrun).
    let reach = |addrlen: i64| -> usize {
        let hex = div(grplen.saturating_mul(cols.saturating_add(g)), g);
        let chars = cols.saturating_mul(if o.color { 12 } else { 1 });
        let need = addrlen
            .saturating_add(hex)
            .saturating_add(chars)
            .saturating_add(32);
        usize::try_from(need).unwrap_or(0).max(LLEN)
    };
    let mut l: Vec<u8> = vec![0; LLEN.saturating_add(1)];
    let mut sq = Squeeze::default();
    let mut p: i64 = 0;
    let mut n: i64 = 0;
    let mut c: i64 = 0;
    let mut addrlen: i64 = 9;
    let mut nonzero: i64 = 0;

    while more(o.length, n) {
        let Some(e) = x.getc_or_die()? else {
            break;
        };
        if p == 0 {
            let addr = u64::from_le_bytes(n.wrapping_add(o.seekoff).to_le_bytes())
                .wrapping_add(o.displayoff);
            let text = if o.decimal_offset {
                format!("{:08}:", i64::from_le_bytes(addr.to_le_bytes()))
            } else {
                format!("{addr:08x}:")
            };
            addrlen = i64::try_from(text.len()).unwrap_or(0);
            for (i, &b) in text.as_bytes().iter().enumerate() {
                set(&mut l, i64::try_from(i).unwrap_or(0), b);
            }
            let end = reach(addrlen);
            let start = text.len();
            if l.len() <= end {
                l.resize(end.saturating_add(1), 0);
            }
            if let Some(span) = l.get_mut(start..end) {
                span.fill(b' ');
            }
        }
        let xpos = if le { p ^ g.wrapping_sub(1) } else { p };
        c = addrlen
            .saturating_add(1)
            .saturating_add(div(grplen.saturating_mul(xpos), g));
        if bits {
            for mask in [0x80u8, 0x40, 0x20, 0x10, 0x08, 0x04, 0x02, 0x01] {
                push(&mut l, &mut c, if e & mask != 0 { b'1' } else { b'0' });
            }
        } else if o.color {
            prologue(&mut l, &mut c);
            begin_coloring_char(&mut l, &mut c, e, o.ebcdic);
            push(&mut l, &mut c, hex_hi(hexx, e));
            push(&mut l, &mut c, hex_lo(hexx, e));
            epilogue(&mut l, &mut c);
        } else {
            set(&mut l, c, hex_hi(hexx, e));
            set(&mut l, c.saturating_add(1), hex_lo(hexx, e));
        }
        if e != 0 {
            nonzero = nonzero.wrapping_add(1);
        }
        // Where the characters start. (When changing this, upstream says,
        // update the definition of LLEN.)
        c = if le {
            // The last group is used in full: round up.
            grplen.saturating_mul(div(cols.saturating_add(g).saturating_sub(1), g))
        } else {
            div(grplen.saturating_mul(cols).saturating_sub(1), g)
        };
        if o.color {
            if bits {
                c = c
                    .saturating_add(addrlen)
                    .saturating_add(3)
                    .saturating_add(p.saturating_mul(12));
            } else {
                c = addrlen
                    .saturating_add(3)
                    .saturating_add(div(grplen.saturating_mul(cols).saturating_sub(1), g))
                    .saturating_add(p.saturating_mul(12));
            }
            if le {
                c = c.saturating_add(1);
            }
            prologue(&mut l, &mut c);
            begin_coloring_char(&mut l, &mut c, e, o.ebcdic);
            push(&mut l, &mut c, shown_char(e, o.ebcdic));
            epilogue(&mut l, &mut c);
            n = n.wrapping_add(1);
            p = p.wrapping_add(1);
            if p == cols {
                push(&mut l, &mut c, b'\n');
                push(&mut l, &mut c, 0);
                x.xxdline(&mut sq, &l, if o.autoskip { nonzero } else { 1 })?;
                nonzero = 0;
                p = 0;
            }
        } else {
            c = c
                .saturating_add(addrlen)
                .saturating_add(3)
                .saturating_add(p);
            push(&mut l, &mut c, shown_char(e, o.ebcdic));
            n = n.wrapping_add(1);
            p = p.wrapping_add(1);
            if p == cols {
                push(&mut l, &mut c, b'\n');
                set(&mut l, c, 0);
                x.xxdline(&mut sq, &l, if o.autoskip { nonzero } else { 1 })?;
                nonzero = 0;
                p = 0;
            }
        }
    }
    if p != 0 {
        push(&mut l, &mut c, b'\n');
        set(&mut l, c, 0);
        if o.color {
            pad_last_line(&mut l, p, o, grplen, addrlen);
        }
        x.xxdline(&mut sq, &l, 1)?;
    } else if o.autoskip {
        // The last chance to flush out suppressed lines.
        x.xxdline(&mut sq, &l, -1)?;
    }
    x.fclose_or_die()?;
    Ok(0)
}

/// With colour, the short last line's missing bytes are red spaces -- for a
/// little-endian dump, first the ones that fill its last group. `p` is the
/// number of bytes the line has.
///
/// Upstream steps its position past the line's NUL first, then sets it
/// afresh in each branch before writing, so the position the line ended at
/// plays no part.
fn pad_last_line(l: &mut Vec<u8>, p: i64, o: &Opts, grplen: i64, addrlen: i64) {
    let cols = o.cols;
    let g = o.octspergrp;
    let red_space = |l: &mut Vec<u8>, c: &mut i64| {
        prologue(l, c);
        push(l, c, COLOR_RED);
        push(l, c, b'm');
        push(l, c, b' ');
        epilogue(l, c);
    };
    let mut x = p;
    let mut p = p;
    if o.hextype == HexType::LittleEndian {
        let mut fill = g.saturating_sub(p.checked_rem(g).unwrap_or(0));
        if fill == g {
            fill = 0;
        }
        let mut c = addrlen.saturating_add(1).saturating_add(div(
            grplen.saturating_mul(x.saturating_sub(g.saturating_sub(fill))),
            g,
        ));
        for _ in 0..fill {
            red_space(l, &mut c);
            x = x.saturating_add(1);
            p = p.saturating_add(1);
        }
    }
    if o.hextype != HexType::Bits {
        let mut c = addrlen
            .saturating_add(1)
            .saturating_add(div(grplen.saturating_mul(x), g));
        c = c.saturating_add(cols.saturating_sub(p));
        c = c.saturating_add(div(cols.saturating_sub(p), g));
        let mut i = cols.saturating_sub(p);
        while i > 0 {
            red_space(l, &mut c);
            i = i.saturating_sub(1);
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn the_line_buffer_is_upstreams() {
        assert_eq!(LLEN, 2581);
    }

    #[test]
    fn hex_and_bin_digits() {
        assert_eq!(parse_hex_digit(b'0'), 0);
        assert_eq!(parse_hex_digit(b'f'), 15);
        assert_eq!(parse_hex_digit(b'F'), 15);
        assert_eq!(parse_hex_digit(b'g'), -1);
        assert_eq!(parse_hex_digit(b' '), -1);
        assert_eq!(parse_bin_digit(b'1'), Some(1));
        assert_eq!(parse_bin_digit(b'2'), None);
        assert_eq!(nibbles(4, 1), b'A');
    }

    #[test]
    fn the_character_column() {
        assert_eq!(shown_char(b'A', false), b'A');
        assert_eq!(shown_char(0, false), b'.');
        assert_eq!(shown_char(0x7f, false), b'.');
        assert_eq!(shown_char(0xc1, true), b'A');
        assert_eq!(shown_char(0x40, true), b' ');
        assert_eq!(shown_char(0x3f, true), b'.');
        assert_eq!(shown_char(0xf0, true), b'0');
        // 0x4a is `0o325` in the table, which is not printable.
        assert_eq!(shown_char(0x4a, true), b'.');
    }

    #[test]
    fn colours_by_class() {
        let colour = |e: u8, ebcdic: bool| {
            let mut l = Vec::new();
            let mut c = 0;
            begin_coloring_char(&mut l, &mut c, e, ebcdic);
            (l[0], c)
        };
        assert_eq!(colour(b'a', false), (COLOR_GREEN, 2));
        assert_eq!(colour(b'\n', false), (COLOR_YELLOW, 2));
        assert_eq!(colour(0, false), (COLOR_WHITE, 2));
        assert_eq!(colour(0xff, false), (COLOR_BLUE, 2));
        assert_eq!(colour(0x80, false), (COLOR_RED, 2));
        assert_eq!(colour(0xc1, true), (COLOR_GREEN, 2));
        assert_eq!(colour(37, true), (COLOR_YELLOW, 2));
        // 97 is in EBCDIC's printable 90..=97 (it is `/`); 1 is a control.
        assert_eq!(colour(97, true), (COLOR_GREEN, 2));
        assert_eq!(colour(1, true), (COLOR_RED, 2));
    }

    #[test]
    fn the_last_zero_lines_are_squeezed_as_upstream_does() {
        // Driven without streams: the state machine alone, recording what
        // would have been printed.
        fn run(lines: &[(&[u8], i64)]) -> Vec<u8> {
            let mut sq = Squeeze::default();
            let mut out = Vec::new();
            for &(l, nz) in lines {
                if nz == 0 && sq.zero_seen == 1 {
                    sq.z = l.to_vec();
                }
                let shown = if nz != 0 {
                    true
                } else {
                    let before = sq.zero_seen;
                    sq.zero_seen += 1;
                    before == 0
                };
                if shown {
                    if nz != 0 {
                        if nz < 0 {
                            sq.zero_seen -= 1;
                        }
                        if sq.zero_seen == 2 {
                            out.extend_from_slice(&sq.z);
                        }
                        if sq.zero_seen > 2 {
                            out.extend_from_slice(b"*\n");
                        }
                    }
                    if nz >= 0 || sq.zero_seen > 0 {
                        out.extend_from_slice(l);
                    }
                    if nz != 0 {
                        sq.zero_seen = 0;
                    }
                }
            }
            out
        }
        // Three zero lines at the end: all three shown.
        assert_eq!(
            run(&[(b"a\n", 0), (b"b\n", 0), (b"c\n", 0), (b"c\n", -1)]),
            b"a\nb\nc\n"
        );
        // Four: the first, a star, the last.
        assert_eq!(
            run(&[
                (b"a\n", 0),
                (b"b\n", 0),
                (b"c\n", 0),
                (b"d\n", 0),
                (b"d\n", -1)
            ]),
            b"a\n*\nd\n"
        );
        // Two zero lines then data: both.
        assert_eq!(run(&[(b"a\n", 0), (b"b\n", 0), (b"X\n", 1)]), b"a\nb\nX\n");
    }
}
