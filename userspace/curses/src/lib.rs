//! ncursesw 6.4's screen library -- the `libncursesw.so.6` of Ubuntu's
//! ncurses 6.4+20240113-1ubuntu2.2 -- as far as procps' `watch` and
//! `slabtop` use it: a screen and its window, characters added to it,
//! attributes and colours, the refresh that brings the terminal into line
//! with them, `endwin`, resizing, and the signals curses handles.
//!
//! A full-screen program's output *is* what ncurses writes: which cursor
//! movements it chose, which attribute strings, which lines it scrolled
//! rather than redrew. So this is a port, file by file, of the library's
//! code, not a library with the same interface; each module names the C
//! files it carries.
//!
//! # Built as Ubuntu builds it
//!
//! The library's behaviour depends on how it was configured, and this
//! follows Debian's flags for the wide-character library (`ncurses_cfg.h`
//! regenerated from them): wide characters, extended colours and functions,
//! `NCURSES_NO_PADDING`, `SIGWINCH` handling, assumed colours, and the
//! hash-map scrolling optimiser on; hard tabs, magic-cookie support, scroll
//! hints, reentrancy and the term driver off; `NDEBUG` defined, so a
//! position the library does not check is not checked here either.
//! `NCURSES_SIZE_T` is a `short`.
//!
//! # The interface
//!
//! The free functions below are the C interface's, on `stdscr` and the one
//! screen [`initscr`] makes, under C's names -- but for `move`, a Rust
//! keyword, which is [`mv`]. Each returns what C's returns, `OK` and `ERR`
//! being `true` and `false`; before `initscr` (or after it failed) every one
//! fails, as C's do on a null screen. They may be called from any thread,
//! and a screen in use by one call is refused to another, so a program's
//! own signal handler must not draw: it may only ask for [`end_and_exit`].
//! A screen of one's own, kept as a value, is [`Screen::newterm`]'s.
//!
//! # The locale
//!
//! What is printable, how wide a character is and how it is written are the
//! C library's answers for the selected locale (`libcall::locale`), as they
//! are ncurses' -- design-decisions §768's one C library. A program sets the
//! locale (`setlocale (LC_ALL, "")`) before [`initscr`], as a C one does.
//!
//! # Modules
//!
//! - [`cell`]: `cchar_t` and the attribute bits.
//! - [`window`]: `WINDOW` and what is done to one without the screen.
//! - [`addch`]: adding characters, by bytes and by wide characters.
//! - [`term`]: the terminal side -- output, padding, attributes, colours.
//! - [`mvcur`]: moving the cursor the cheapest way.
//! - [`update`]: `doupdate`'s work -- each changed line rewritten.
//! - [`hashmap`]: finding the lines that moved, and scrolling them.
//! - [`screen`]: `SCREEN` -- setup, modes, refresh, `endwin`, resizing.
//! - [`signals`]: the process's screen, and `SIGTSTP`, `SIGINT`,
//!   `SIGTERM` and `SIGWINCH`.
//! - [`libc`]: the locale, from the C library.
//! - [`caps`]: every capability's index.

use core::sync::atomic::{AtomicBool, Ordering};

pub mod addch;
pub mod caps;
pub mod cell;
pub mod hashmap;
pub mod libc;
pub mod mvcur;
pub mod screen;
pub mod signals;
pub mod term;
#[cfg(test)]
mod testing;
pub mod update;
pub mod window;

pub use cell::{
    A_ALTCHARSET, A_ATTRIBUTES, A_BLINK, A_BOLD, A_CHARTEXT, A_COLOR, A_DIM, A_INVIS, A_ITALIC,
    A_NORMAL, A_PROTECT, A_REVERSE, A_STANDOUT, A_UNDERLINE, Attr, Cell, WChar, color_pair,
    pair_number,
};
pub use libc::Libc;
pub use screen::{NoTerminal, Options, Screen};
pub use signals::end_and_exit;
pub use term::Sink;

use addch::AddCtx;
use window::Window;

/// `COLOR_BLACK` .. `COLOR_WHITE`: the eight colours every colour terminal
/// numbers alike.
pub const COLOR_BLACK: i16 = 0;
pub const COLOR_RED: i16 = 1;
pub const COLOR_GREEN: i16 = 2;
pub const COLOR_YELLOW: i16 = 3;
pub const COLOR_BLUE: i16 = 4;
pub const COLOR_MAGENTA: i16 = 5;
pub const COLOR_CYAN: i16 = 6;
pub const COLOR_WHITE: i16 = 7;

/// `_nc_prescreen.use_env`.
static USE_ENV: AtomicBool = AtomicBool::new(true);
/// `_nc_globals.init_screen`: `initscr` has run.
static INIT_SCREEN: AtomicBool = AtomicBool::new(false);

/// `use_env (f)`: whether the screen [`initscr`] makes asks the window's
/// size and reads `LINES` and `COLUMNS` (the default), or believes the
/// terminal description. Only before `initscr`.
pub fn use_env(f: bool) {
    USE_ENV.store(f, Ordering::Release);
}

/// `initscr ()`: the screen, on standard output, for the terminal `TERM`
/// names -- `unknown` when it names none -- with curses' signal handlers
/// installed where the program left the defaults. A second call does
/// nothing. When there is no such terminal, "Error opening terminal:
/// NAME." and an exit with status 1, as C's.
pub fn initscr() {
    // "Portable applications must not call initscr() more than once"
    if INIT_SCREEN.swap(true, Ordering::AcqRel) {
        return;
    }
    let name = match std::env::var_os("TERM") {
        Some(v) if !v.is_empty() => screen::os_bytes(&v),
        _ => b"unknown".to_vec(),
    };
    let opts = Options {
        output: Sink::Fd(1),
        input: 0,
        use_env: USE_ENV.load(Ordering::Acquire),
    };
    if let Ok(sp) = Screen::newterm(&name, &opts, Box::new(Libc)) {
        signals::with_slot(|slot| {
            let sp = slot.insert(sp);
            signals::install(&mut sp.tstp);
        });
        // "def_shell_mode - done in newterm/_nc_setupscreen"
        signals::with(Screen::def_prog_mode);
    } else {
        let mut msg = b"Error opening terminal: ".to_vec();
        msg.extend_from_slice(&name);
        msg.extend_from_slice(b".\n");
        // Nothing is left to report a failure to write to standard error
        // to; the exit status says it.
        let _ = std::io::Write::write_all(&mut std::io::stderr(), &msg);
        std::process::exit(1);
    }
}

/// The screen's answer, or `ERR` without one.
fn ok(f: impl FnOnce(&mut Screen) -> bool) -> bool {
    signals::with(f).unwrap_or(false)
}

/// `f` on `stdscr`, with what adding to it needs; `ERR` without a screen.
fn on_stdscr(f: impl FnOnce(&mut Window, &AddCtx<'_>) -> bool) -> bool {
    ok(|sp| {
        let (win, ctx) = sp.stdscr_ctx();
        f(win, &ctx)
    })
}

/// `endwin ()`.
pub fn endwin() -> bool {
    ok(Screen::endwin)
}

/// `isendwin ()`: `endwin` was called and no refresh has come back since.
#[must_use]
pub fn isendwin() -> bool {
    signals::with(|sp| sp.endwin == screen::EndWin::Suspend).unwrap_or(false)
}

/// `refresh ()`.
pub fn refresh() -> bool {
    ok(|sp| {
        sp.refresh();
        true
    })
}

/// `wnoutrefresh (stdscr)`.
pub fn wnoutrefresh() -> bool {
    ok(|sp| {
        sp.wnoutrefresh();
        true
    })
}

/// `doupdate ()`.
pub fn doupdate() -> bool {
    ok(|sp| {
        sp.doupdate();
        true
    })
}

/// `move (y, x)`.
pub fn mv(y: i32, x: i32) -> bool {
    on_stdscr(|w, _| w.wmove(y, x))
}

/// `addch (ch)`: a byte and its attributes, as a `chtype`.
pub fn addch(ch: Attr) -> bool {
    on_stdscr(|w, ctx| addch::waddch(w, ctx, ch))
}

/// `mvaddch (y, x, ch)`.
pub fn mvaddch(y: i32, x: i32, ch: Attr) -> bool {
    on_stdscr(|w, ctx| w.wmove(y, x) && addch::waddch(w, ctx, ch))
}

/// `addstr (str)`: the bytes up to a NUL.
pub fn addstr(s: &[u8]) -> bool {
    addnstr(s, -1)
}

/// `addnstr (str, n)`: at most `n` of the bytes up to a NUL; all of them for
/// a negative `n`, and `ERR` for 0.
pub fn addnstr(s: &[u8], n: i32) -> bool {
    on_stdscr(|w, ctx| addch::waddnstr(w, ctx, s, n))
}

/// `mvaddstr (y, x, str)`.
pub fn mvaddstr(y: i32, x: i32, s: &[u8]) -> bool {
    mvaddnstr(y, x, s, -1)
}

/// `mvaddnstr (y, x, str, n)`.
pub fn mvaddnstr(y: i32, x: i32, s: &[u8], n: i32) -> bool {
    on_stdscr(|w, ctx| w.wmove(y, x) && addch::waddnstr(w, ctx, s, n))
}

/// `printw (fmt, ...)`, the formatting done: the formatted bytes added as
/// `addstr` adds them -- which with `vsnprintf` to grow its buffer is all of
/// them, up to a NUL.
pub fn printw(formatted: &[u8]) -> bool {
    addstr(formatted)
}

/// `addwstr (wstr)`: wide characters, up to a NUL.
pub fn addwstr(s: &[WChar]) -> bool {
    addnwstr(s, -1)
}

/// `addnwstr (wstr, n)`.
pub fn addnwstr(s: &[WChar], n: i32) -> bool {
    on_stdscr(|w, ctx| addch::waddnwstr(w, ctx, s, n))
}

/// `mvaddwstr (y, x, wstr)`.
pub fn mvaddwstr(y: i32, x: i32, s: &[WChar]) -> bool {
    mvaddnwstr(y, x, s, -1)
}

/// `mvaddnwstr (y, x, wstr, n)`.
pub fn mvaddnwstr(y: i32, x: i32, s: &[WChar], n: i32) -> bool {
    on_stdscr(|w, ctx| w.wmove(y, x) && addch::waddnwstr(w, ctx, s, n))
}

/// `add_wch (wch)`.
pub fn add_wch(wch: &Cell) -> bool {
    on_stdscr(|w, ctx| addch::wadd_wch(w, ctx, wch))
}

/// `clear ()`: `erase`, and the whole screen repainted at the next refresh.
pub fn clear() -> bool {
    on_stdscr(|w, _| w.wclear())
}

/// `erase ()`.
pub fn erase() -> bool {
    on_stdscr(|w, _| w.werase())
}

/// `clrtobot ()`.
pub fn clrtobot() -> bool {
    on_stdscr(|w, _| w.wclrtobot())
}

/// `clrtoeol ()`.
pub fn clrtoeol() -> bool {
    on_stdscr(|w, _| w.wclrtoeol())
}

/// `attron (attrs)`.
pub fn attron(attrs: Attr) -> bool {
    on_stdscr(|w, _| {
        w.wattr_on(attrs);
        true
    })
}

/// `attroff (attrs)`.
pub fn attroff(attrs: Attr) -> bool {
    on_stdscr(|w, _| {
        w.wattr_off(attrs);
        true
    })
}

/// `attrset (attrs)`.
pub fn attrset(attrs: Attr) -> bool {
    on_stdscr(|w, _| {
        w.wattrset(attrs);
        true
    })
}

/// `attr_set (attrs, pair, NULL)`: the attributes but for their colour, and
/// the pair, set whole.
pub fn attr_set(attrs: Attr, pair: i16) -> bool {
    on_stdscr(|w, _| {
        w.wattr_set(attrs, pair);
        true
    })
}

/// `standout ()`.
pub fn standout() -> bool {
    on_stdscr(|w, _| {
        w.wstandout();
        true
    })
}

/// `standend ()`.
pub fn standend() -> bool {
    on_stdscr(|w, _| {
        w.wstandend();
        true
    })
}

/// `inch ()`: the character under the cursor and its attributes, as a
/// `chtype`; `ERR`'s bits (all ones) without a screen.
#[must_use]
pub fn inch() -> Attr {
    signals::with(|sp| sp.stdscr.winch()).unwrap_or(Attr::MAX)
}

/// `in_wch (&wch)`: the cell under the cursor; `None` for `ERR`.
#[must_use]
pub fn in_wch() -> Option<Cell> {
    signals::with(|sp| sp.stdscr.win_wch())
}

/// `getyx (stdscr, y, x)`: the cursor; -1, -1 without a screen.
#[must_use]
pub fn getyx() -> (i32, i32) {
    signals::with(|sp| (i32::from(sp.stdscr.cury), i32::from(sp.stdscr.curx))).unwrap_or((-1, -1))
}

/// `getmaxyx (stdscr, y, x)`: the window's size; -1, -1 without a screen.
#[must_use]
pub fn getmaxyx() -> (i32, i32) {
    signals::with(|sp| {
        (
            i32::from(sp.stdscr.maxy).wrapping_add(1),
            i32::from(sp.stdscr.maxx).wrapping_add(1),
        )
    })
    .unwrap_or((-1, -1))
}

/// `LINES`: the lines of `stdscr`; 0 before `initscr`.
#[must_use]
pub fn lines() -> i32 {
    signals::with(|sp| sp.lines).unwrap_or(0)
}

/// `COLS`; 0 before `initscr`.
#[must_use]
pub fn cols() -> i32 {
    signals::with(|sp| sp.cols).unwrap_or(0)
}

/// `TABSIZE`; 8 before `initscr`.
#[must_use]
pub fn tabsize() -> i32 {
    signals::with(|sp| sp.tabsize).unwrap_or(8)
}

/// `curs_set (visibility)`: 0 invisible, 1 normal, 2 very visible; the
/// visibility before, or -1 (`ERR`) when the terminal cannot.
pub fn curs_set(visibility: i32) -> i32 {
    signals::with(|sp| sp.t.curs_set(visibility)).unwrap_or(-1)
}

/// `nl ()`: curses maps newline to carriage return and newline itself.
pub fn nl() -> bool {
    ok(|sp| {
        sp.nl = true;
        true
    })
}

/// `nonl ()`.
pub fn nonl() -> bool {
    ok(|sp| {
        sp.nl = false;
        true
    })
}

/// `echo ()`: curses echoes what it reads (the terminal never does).
pub fn echo() -> bool {
    ok(|sp| {
        sp.echo = true;
        true
    })
}

/// `noecho ()`.
pub fn noecho() -> bool {
    ok(|sp| {
        sp.echo = false;
        true
    })
}

/// `cbreak ()`.
pub fn cbreak() -> bool {
    ok(Screen::cbreak)
}

/// `nocbreak ()`.
pub fn nocbreak() -> bool {
    ok(Screen::nocbreak)
}

/// `beep ()`.
pub fn beep() -> bool {
    ok(Screen::beep)
}

/// `flushinp ()`.
pub fn flushinp() -> bool {
    ok(|sp| {
        sp.flushinp();
        true
    })
}

/// `def_prog_mode ()`.
pub fn def_prog_mode() -> bool {
    ok(Screen::def_prog_mode)
}

/// `def_shell_mode ()`.
pub fn def_shell_mode() -> bool {
    ok(Screen::def_shell_mode)
}

/// `reset_prog_mode ()`.
pub fn reset_prog_mode() -> bool {
    ok(Screen::reset_prog_mode)
}

/// `reset_shell_mode ()`.
pub fn reset_shell_mode() -> bool {
    ok(Screen::reset_shell_mode)
}

/// `has_colors ()`.
#[must_use]
pub fn has_colors() -> bool {
    signals::with(|sp| sp.t.has_colors()).unwrap_or(false)
}

/// `can_change_color ()`.
#[must_use]
pub fn can_change_color() -> bool {
    signals::with(|sp| sp.t.can_change_color()).unwrap_or(false)
}

/// `start_color ()`.
pub fn start_color() -> bool {
    ok(|sp| sp.t.start_color())
}

/// `use_default_colors ()`: colour pair 0 the terminal's own colours.
pub fn use_default_colors() -> bool {
    assume_default_colors(-1, -1)
}

/// `assume_default_colors (fg, bg)`.
pub fn assume_default_colors(fg: i32, bg: i32) -> bool {
    ok(|sp| sp.assume_default_colors(fg, bg))
}

/// `init_pair (pair, f, b)`.
pub fn init_pair(pair: i16, f: i16, b: i16) -> bool {
    ok(|sp| sp.init_pair(i32::from(pair), i32::from(f), i32::from(b)))
}

/// `init_color (color, r, g, b)`: a colour redefined, each component out of
/// 1000.
pub fn init_color(color: i16, r: i16, g: i16, b: i16) -> bool {
    ok(|sp| {
        sp.t.init_color(i32::from(color), i32::from(r), i32::from(g), i32::from(b))
    })
}

/// `COLORS`: 0 before `start_color`.
#[must_use]
pub fn colors() -> i32 {
    signals::with(|sp| sp.t.color_count).unwrap_or(0)
}

/// `COLOR_PAIRS`: 0 before `start_color`.
#[must_use]
pub fn color_pairs() -> i32 {
    signals::with(|sp| sp.t.pair_count).unwrap_or(0)
}

/// `is_term_resized (lines, cols)`.
#[must_use]
pub fn is_term_resized(lines: i32, cols: i32) -> bool {
    signals::with(|sp| sp.is_term_resized(lines, cols)).unwrap_or(false)
}

/// `resize_term (lines, cols)`.
pub fn resize_term(lines: i32, cols: i32) -> bool {
    ok(|sp| sp.resize_term(lines, cols))
}

/// `resizeterm (lines, cols)`.
pub fn resizeterm(lines: i32, cols: i32) -> bool {
    ok(|sp| sp.resizeterm(lines, cols))
}
