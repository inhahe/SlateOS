//! `curses-probe SCRIPT`: this crate driven by a script of curses calls, one
//! per line -- the subject side of `scripts/curses-diff.sh`, whose reference
//! side is `scripts/curses-probe.c` running the same script against Ubuntu's
//! libncursesw. That file describes the script; the two read it alike.
//!
//! What the screen writes goes to standard output, as the library writes
//! it; what each call returns goes to standard error, one line per call, as
//! "NAME VALUE" with C's `OK` and `ERR` as 0 and -1.
//!
//! A test instrument, not a program: it is built as an example so that it
//! is never installed (`scripts/diff-wsl.sh`, `DIFF_EXAMPLES`).

use std::io::Write as _;

/// The escapes `\n`, `\t`, `\e`, `\\` and `\xHH` replaced, as the C probe's
/// `unescape` does.
fn unescape(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0usize;
    while let Some(&c) = s.get(i) {
        let next = s.get(i.saturating_add(1)).copied();
        if c != b'\\' || next.is_none() {
            out.push(c);
            i = i.saturating_add(1);
            continue;
        }
        i = i.saturating_add(1);
        match next {
            Some(b'n') => {
                out.push(b'\n');
                i = i.saturating_add(1);
            }
            Some(b't') => {
                out.push(b'\t');
                i = i.saturating_add(1);
            }
            Some(b'e') => {
                out.push(0x1b);
                i = i.saturating_add(1);
            }
            Some(b'\\') => {
                out.push(b'\\');
                i = i.saturating_add(1);
            }
            Some(b'x') => {
                let hex = s.get(i.saturating_add(1)..i.saturating_add(3));
                match hex {
                    Some(h) if h.len() == 2 => {
                        let v = std::str::from_utf8(h)
                            .ok()
                            .and_then(|h| u8::from_str_radix(h, 16).ok())
                            .unwrap_or(0);
                        out.push(v);
                        i = i.saturating_add(3);
                    }
                    _ => {
                        out.push(b'x');
                        i = i.saturating_add(1);
                    }
                }
            }
            _ => out.push(b'\\'),
        }
    }
    out
}

/// UTF-8 to wide characters, as the C probe's `utf8_to_wide` decodes it: a
/// lead byte and as many continuation bytes as it promises make one
/// character; any other byte stands for itself.
fn utf8_to_wide(s: &[u8]) -> Vec<curses::WChar> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0usize;
    while let Some(&c) = s.get(i) {
        let (need, mut wc): (usize, u32) = if c < 0x80 {
            (0, u32::from(c))
        } else if c & 0xe0 == 0xc0 {
            (1, u32::from(c & 0x1f))
        } else if c & 0xf0 == 0xe0 {
            (2, u32::from(c & 0x0f))
        } else if c & 0xf8 == 0xf0 {
            (3, u32::from(c & 0x07))
        } else {
            (4, u32::from(c))
        };
        let mut whole = need < 4;
        for k in 1..=need.min(3) {
            match s.get(i.saturating_add(k)) {
                Some(&d) if d & 0xc0 == 0x80 => wc = (wc << 6) | u32::from(d & 0x3f),
                _ => {
                    whole = false;
                    break;
                }
            }
        }
        if whole {
            out.push(wc.cast_signed());
            i = i.saturating_add(need).saturating_add(1);
        } else {
            out.push(i32::from(c));
            i = i.saturating_add(1);
        }
    }
    out
}

/// What a call returned, as the C probe prints it. (Standard error is
/// where the comparison reads it; a failure to write it shows there.)
fn say(name: &[u8], value: i32) {
    let mut line = name.to_vec();
    line.extend_from_slice(format!(" {value}\n").as_bytes());
    let _ = std::io::stderr().write_all(&line);
}

/// `(short) v`: the low sixteen bits, as C's cast keeps them.
fn short(v: i32) -> i16 {
    let [a, b, ..] = v.to_le_bytes();
    i16::from_le_bytes([a, b])
}

/// C's `OK` or `ERR`.
const fn rc(ok: bool) -> i32 {
    if ok { 0 } else { -1 }
}

/// A line's words, the rest of it kept whole for text.
struct Line<'a> {
    rest: &'a [u8],
}

impl<'a> Line<'a> {
    /// The next space-separated word.
    fn word(&mut self) -> &'a [u8] {
        match self.rest.iter().position(|&b| b == b' ') {
            Some(at) => {
                let (w, rest) = self.rest.split_at(at);
                self.rest = rest.get(1..).unwrap_or_default();
                w
            }
            None => std::mem::take(&mut self.rest),
        }
    }

    /// The next word as `atoi` reads it.
    fn n(&mut self) -> i32 {
        let w = self.word();
        let (v, _) = cstrtol::strtol(w, 10);
        i32::try_from(v).unwrap_or(0)
    }

    /// The next word as hexadecimal, `strtoul (w, NULL, 16)` as a `chtype`.
    fn h(&mut self) -> curses::Attr {
        let w = self.word();
        let (v, _) = cstrtol::strtoul(w, 16);
        let [a, b, c, d, ..] = v.to_le_bytes();
        u32::from_le_bytes([a, b, c, d])
    }

    /// The rest of the line, unescaped.
    fn text(&mut self) -> Vec<u8> {
        unescape(std::mem::take(&mut self.rest))
    }
}

fn main() {
    let args: Vec<_> = std::env::args_os().collect();
    let script = match args.as_slice() {
        [_, path] => std::fs::read(path).ok(),
        _ => None,
    };
    let Some(script) = script else {
        let _ = writeln!(std::io::stderr(), "usage: curses-probe SCRIPT");
        std::process::exit(2);
    };
    let mut buf = [0u8; 256];
    // As the C probe: whatever the environment selects, or the C locale.
    let _ = libcall::locale::select_from_env(libcall::locale::LC_ALL, &mut buf);
    for raw in script.split(|&b| b == b'\n') {
        if raw.is_empty() || raw.first() == Some(&b'#') {
            continue;
        }
        let mut line = Line { rest: raw };
        let cmd = line.word();
        let name = cmd;
        match cmd {
            b"use_env" => {
                curses::use_env(line.n() != 0);
                say(name, 0);
            }
            b"initscr" => {
                curses::initscr();
                say(name, 0);
            }
            b"endwin" => say(name, rc(curses::endwin())),
            b"isendwin" => say(name, i32::from(curses::isendwin())),
            b"refresh" => say(name, rc(curses::refresh())),
            b"wnoutrefresh" => say(name, rc(curses::wnoutrefresh())),
            b"doupdate" => say(name, rc(curses::doupdate())),
            b"move" => {
                let (y, x) = (line.n(), line.n());
                say(name, rc(curses::mv(y, x)));
            }
            b"addch" => say(name, rc(curses::addch(line.h()))),
            b"mvaddch" => {
                let (y, x) = (line.n(), line.n());
                say(name, rc(curses::mvaddch(y, x, line.h())));
            }
            b"addstr" => say(name, rc(curses::addstr(&line.text()))),
            b"addnstr" => {
                let n = line.n();
                say(name, rc(curses::addnstr(&line.text(), n)));
            }
            b"mvaddstr" => {
                let (y, x) = (line.n(), line.n());
                say(name, rc(curses::mvaddstr(y, x, &line.text())));
            }
            b"mvaddnstr" => {
                let (y, x, n) = (line.n(), line.n(), line.n());
                say(name, rc(curses::mvaddnstr(y, x, &line.text(), n)));
            }
            b"printw" => say(name, rc(curses::printw(&line.text()))),
            b"addwstr" => {
                let wide = utf8_to_wide(&line.text());
                say(name, rc(curses::addwstr(&wide)));
            }
            b"addnwstr" => {
                let k = line.n();
                let wide = utf8_to_wide(&line.text());
                say(name, rc(curses::addnwstr(&wide, k)));
            }
            b"mvaddnwstr" => {
                let (y, x, k) = (line.n(), line.n(), line.n());
                let wide = utf8_to_wide(&line.text());
                say(name, rc(curses::mvaddnwstr(y, x, &wide, k)));
            }
            b"clear" => say(name, rc(curses::clear())),
            b"erase" => say(name, rc(curses::erase())),
            b"clrtobot" => say(name, rc(curses::clrtobot())),
            b"clrtoeol" => say(name, rc(curses::clrtoeol())),
            b"attron" => say(name, rc(curses::attron(line.h()))),
            b"attroff" => say(name, rc(curses::attroff(line.h()))),
            b"attrset" => say(name, rc(curses::attrset(line.h()))),
            b"standout" => say(name, rc(curses::standout())),
            b"standend" => say(name, rc(curses::standend())),
            b"inch" => {
                let _ = writeln!(std::io::stderr(), "inch {:x}", curses::inch());
            }
            b"in_wch" => {
                let (code, c) = match curses::in_wch() {
                    Some(c) => (0, c),
                    None => (-1, curses::Cell::default()),
                };
                let _ = writeln!(
                    std::io::stderr(),
                    "in_wch {code} {:x} {:x} {:x} {}",
                    c.attr,
                    c.chars[0].cast_unsigned(),
                    c.chars[1].cast_unsigned(),
                    c.ext_color
                );
            }
            b"getyx" => {
                let (y, x) = curses::getyx();
                let _ = writeln!(std::io::stderr(), "getyx {y} {x}");
            }
            b"getmaxyx" => {
                let (y, x) = curses::getmaxyx();
                let _ = writeln!(std::io::stderr(), "getmaxyx {y} {x}");
            }
            b"lines" => {
                let _ = writeln!(
                    std::io::stderr(),
                    "lines {} {} {}",
                    curses::lines(),
                    curses::cols(),
                    curses::tabsize()
                );
            }
            b"curs_set" => say(name, curses::curs_set(line.n())),
            b"nl" => say(name, rc(curses::nl())),
            b"nonl" => say(name, rc(curses::nonl())),
            b"echo" => say(name, rc(curses::echo())),
            b"noecho" => say(name, rc(curses::noecho())),
            b"cbreak" => say(name, rc(curses::cbreak())),
            b"nocbreak" => say(name, rc(curses::nocbreak())),
            b"beep" => say(name, rc(curses::beep())),
            b"has_colors" => say(name, i32::from(curses::has_colors())),
            b"can_change_color" => say(name, i32::from(curses::can_change_color())),
            b"start_color" => say(name, rc(curses::start_color())),
            b"use_default_colors" => say(name, rc(curses::use_default_colors())),
            b"assume_default_colors" => {
                let (f, b) = (line.n(), line.n());
                say(name, rc(curses::assume_default_colors(f, b)));
            }
            b"init_pair" => {
                let (p, f, b) = (line.n(), line.n(), line.n());
                say(name, rc(curses::init_pair(short(p), short(f), short(b))));
            }
            b"init_color" => {
                let (c, r, g, b) = (line.n(), line.n(), line.n(), line.n());
                say(
                    name,
                    rc(curses::init_color(short(c), short(r), short(g), short(b))),
                );
            }
            b"colors" => {
                let _ = writeln!(
                    std::io::stderr(),
                    "colors {} {}",
                    curses::colors(),
                    curses::color_pairs()
                );
            }
            b"resizeterm" => {
                let (l, c) = (line.n(), line.n());
                say(name, rc(curses::resizeterm(l, c)));
            }
            b"resize_term" => {
                let (l, c) = (line.n(), line.n());
                say(name, rc(curses::resize_term(l, c)));
            }
            b"is_term_resized" => {
                let (l, c) = (line.n(), line.n());
                say(name, i32::from(curses::is_term_resized(l, c)));
            }
            // The window changed size: the terminal on standard output told
            // so, and the signal raised (the kernel sends one too, on a
            // terminal). Neither can fail in a way the C probe would see.
            b"winch" => {
                let (rows, cols) = (line.n(), line.n());
                let size = libcall::pty::WinSize {
                    rows: short(rows).cast_unsigned(),
                    cols: short(cols).cast_unsigned(),
                    xpixel: 0,
                    ypixel: 0,
                };
                let _ = libcall::pty::set_window_size(1, size);
                let _ = libcall::signal::raise(libcall::signal::SIGWINCH);
                say(name, 0);
            }
            b"suspend" | b"interrupt" | b"terminate" => {
                let signal = match cmd {
                    b"suspend" => libcall::signal::SIGTSTP,
                    b"interrupt" => libcall::signal::SIGINT,
                    _ => libcall::signal::SIGTERM,
                };
                let _ = libcall::signal::raise(signal);
                say(name, 0);
            }
            _ => {
                let mut msg = b"curses-probe: unknown call '".to_vec();
                msg.extend_from_slice(name);
                msg.extend_from_slice(b"'\n");
                let _ = std::io::stderr().write_all(&msg);
                std::process::exit(2);
            }
        }
    }
}
