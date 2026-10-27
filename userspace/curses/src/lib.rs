//! ncursesw 6.4's screen library -- the `libncursesw.so.6` of Ubuntu's
//! ncurses 6.4+20240113-1ubuntu2.2 -- as far as procps' `watch` and
//! `slabtop` and util-linux's `cfdisk` use it: a screen, its window and the
//! sub-windows made inside it, characters added to them, attributes,
//! colours and line drawing, the refresh that brings the terminal into line
//! with them, `endwin`, resizing, the signals curses handles, and the
//! keyboard -- bytes, key sequences, wide characters, and cooked lines.
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
//! The free functions below are the C interface's, on `stdscr` -- or on the
//! window a [`Win`] names -- and the one screen [`initscr`] makes, under C's
//! names, but for `move` and `box`, Rust keywords, which are [`mv`] and
//! [`box_`]. Each returns what C's returns, `OK` and `ERR` being `true` and
//! `false`; before `initscr` (or after it failed) every one fails, as C's do
//! on a null screen. They may be called from any thread, and a screen in use
//! by one call is refused to another, so a program's own signal handler must
//! not draw: it may only ask for [`end_and_exit`]. A screen of one's own,
//! kept as a value, is [`Screen::newterm`]'s.
//!
//! A read waits with the screen let go ([`input::Need`]), as upstream's
//! handlers run inside its `read`: curses' own `SIGTSTP` and `SIGINT`
//! handlers can then suspend or end the program while it waits for a key.
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
//! - [`windows`]: windows by handle -- sub-windows, `box`, refreshing one,
//!   resizing them all.
//! - [`input`]: the keyboard -- `getch`, `get_wch`, the key tries and the
//!   input queue.
//! - [`keys`]: the key codes.
//! - [`signals`]: the process's screen, and `SIGTSTP`, `SIGINT`,
//!   `SIGTERM` and `SIGWINCH`.
//! - [`libc`]: the locale, from the C library.
//! - [`caps`]: every capability's index.

use core::sync::atomic::{AtomicBool, Ordering};

pub mod addch;
pub mod caps;
pub mod cell;
pub mod hashmap;
pub mod input;
pub mod keys;
pub mod libc;
pub mod mvcur;
pub mod screen;
pub mod signals;
pub mod term;
#[cfg(test)]
mod testing;
pub mod update;
pub mod window;
pub mod windows;

/// The unit tests' allocator, which counts on request: what lets a test
/// show that the work a signal handler does allocates nothing.
#[cfg(test)]
#[global_allocator]
static ALLOCATOR: testing::Counting = testing::Counting;

pub use cell::{
    A_ALTCHARSET, A_ATTRIBUTES, A_BLINK, A_BOLD, A_CHARTEXT, A_COLOR, A_DIM, A_INVIS, A_ITALIC,
    A_NORMAL, A_PROTECT, A_REVERSE, A_STANDOUT, A_UNDERLINE, Attr, Cell, WChar, color_pair,
    pair_number,
};
pub use input::{ERR, OK, get_escdelay, set_escdelay};
pub use keys::*;
pub use libc::Libc;
pub use screen::{NoTerminal, Options, Screen};
pub use signals::end_and_exit;
pub use term::Sink;
pub use windows::{
    ACS_BLOCK, ACS_BTEE, ACS_BULLET, ACS_CKBOARD, ACS_DARROW, ACS_DIAMOND, ACS_HLINE, ACS_LARROW,
    ACS_LLCORNER, ACS_LRCORNER, ACS_LTEE, ACS_PLUS, ACS_RARROW, ACS_RTEE, ACS_TTEE, ACS_UARROW,
    ACS_ULCORNER, ACS_URCORNER, ACS_VLINE, Win, acs,
};

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

/// `wnoutrefresh (win)`.
pub fn wnoutrefresh(w: Win) -> bool {
    ok(|sp| sp.wnoutrefresh_win(w))
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

/// `idlok (stdscr, flag)`: [`Screen::idlok`]; `ERR` without a screen.
pub fn idlok(flag: bool) -> bool {
    ok(|sp| {
        sp.idlok(flag);
        true
    })
}

/// `idcok (stdscr, flag)`: [`Screen::idcok`]; nothing without a screen.
pub fn idcok(flag: bool) {
    ok(|sp| {
        sp.idcok(flag);
        true
    });
}

/// `use_legacy_coding (level)`: how bytes from 128 to 255 are shown -- 0, as
/// the locale has them; 1, those from 160 up as they are; 2, all of them as
/// they are. The level before, or `ERR` (-1) for any other level and
/// without a screen.
pub fn use_legacy_coding(level: i32) -> i32 {
    signals::with(|sp| sp.use_legacy_coding(level)).unwrap_or(-1)
}

/// What [`tigetstr`] answers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TermString {
    /// `(char *) -1`: the name is no string capability, or there is no
    /// screen.
    Cancelled,
    /// A null pointer: the terminal has no such string -- or curses took it
    /// away, as it takes `ht` and `cbt` from a terminal that expands tabs
    /// itself.
    Absent,
    /// The string.
    Value(Vec<u8>),
}

/// `tigetstr (name)`: the string capability `name` of the screen's
/// terminal, as curses holds it once set up.
pub fn tigetstr(name: &[u8]) -> TermString {
    signals::with(|sp| sp.tigetstr(name)).unwrap_or(TermString::Cancelled)
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

// ---- windows

/// `stdscr`.
pub const STDSCR: Win = Win::Stdscr;

/// `f` on the window `w`, with what adding to it needs; `ERR` without a
/// screen, or for a window since deleted.
fn on_win(w: Win, f: impl FnOnce(&mut Window, &AddCtx<'_>) -> bool) -> bool {
    signals::with(|sp| sp.with_win(w, f))
        .flatten()
        .unwrap_or(false)
}

/// `subwin (orig, lines, cols, begy, begx)`: a window inside `orig`,
/// sharing its cells, placed on the screen; `None` for one that would not
/// fit inside it.
pub fn subwin(orig: Win, lines: i32, cols: i32, begy: i32, begx: i32) -> Option<Win> {
    signals::with(|sp| sp.subwin(orig, lines, cols, begy, begx)).flatten()
}

/// `derwin (orig, lines, cols, begy, begx)`: likewise, placed within
/// `orig`.
pub fn derwin(orig: Win, lines: i32, cols: i32, begy: i32, begx: i32) -> Option<Win> {
    signals::with(|sp| sp.derwin(orig, lines, cols, begy, begx)).flatten()
}

/// `delwin (win)`: [`Screen::delwin`].
pub fn delwin(w: Win) -> bool {
    ok(|sp| sp.delwin(w))
}

/// `getyx (win, y, x)`: the cursor; -1, -1 without a screen or the window.
#[must_use]
pub fn wgetyx(w: Win) -> (i32, i32) {
    signals::with(|sp| {
        sp.window(w)
            .map(|win| (i32::from(win.cury), i32::from(win.curx)))
    })
    .flatten()
    .unwrap_or((-1, -1))
}

/// `getmaxyx (win, y, x)`: the window's size; -1, -1 without a screen or
/// the window.
#[must_use]
pub fn wgetmaxyx(w: Win) -> (i32, i32) {
    signals::with(|sp| {
        sp.window(w).map(|win| {
            (
                i32::from(win.maxy).wrapping_add(1),
                i32::from(win.maxx).wrapping_add(1),
            )
        })
    })
    .flatten()
    .unwrap_or((-1, -1))
}

/// `getbegyx (win, y, x)`: where the window is on the screen; -1, -1
/// without a screen or the window.
#[must_use]
pub fn wgetbegyx(w: Win) -> (i32, i32) {
    signals::with(|sp| {
        sp.window(w)
            .map(|win| (i32::from(win.begy), i32::from(win.begx)))
    })
    .flatten()
    .unwrap_or((-1, -1))
}

/// `clearok (win, flag)`: whether the next refresh of `w` repaints the
/// whole screen.
pub fn clearok(w: Win, flag: bool) -> bool {
    ok(|sp| {
        sp.window_mut(w).is_some_and(|win| {
            win.clear = flag;
            true
        })
    })
}

/// `mvcur (oldrow, oldcol, newrow, newcol)`: the terminal's cursor moved
/// the cheapest way that overwrites nothing -- the move written through
/// `putchar`, which for upstream is the C library's `putchar`: the
/// program's standard output, buffered as the program buffers it, not the
/// screen's own output.
pub fn mvcur(
    oldrow: i32,
    oldcol: i32,
    newrow: i32,
    newcol: i32,
    putchar: &mut dyn FnMut(u8),
) -> bool {
    ok(|sp| {
        let Screen {
            t, newscr, ctype, ..
        } = sp;
        t.mvcur_putchar(
            newscr,
            &**ctype,
            (oldrow, oldcol),
            (newrow, newcol),
            putchar,
        )
    })
}

/// `touchwin (win)`.
pub fn touchwin(w: Win) -> bool {
    ok(|sp| sp.touchwin(w))
}

/// `wrefresh (win)`.
pub fn wrefresh(w: Win) -> bool {
    ok(|sp| sp.wrefresh(w))
}

/// `wclear (win)`.
pub fn wclear(w: Win) -> bool {
    ok(|sp| sp.wclear(w))
}

/// `werase (win)`.
pub fn werase(w: Win) -> bool {
    ok(|sp| sp.werase(w))
}

/// `wmove (win, y, x)`.
pub fn wmove(w: Win, y: i32, x: i32) -> bool {
    on_win(w, |win, _| win.wmove(y, x))
}

/// `waddch (win, ch)`.
pub fn waddch(w: Win, ch: Attr) -> bool {
    on_win(w, |win, ctx| addch::waddch(win, ctx, ch))
}

/// `mvwaddch (win, y, x, ch)`.
pub fn mvwaddch(w: Win, y: i32, x: i32, ch: Attr) -> bool {
    on_win(w, |win, ctx| win.wmove(y, x) && addch::waddch(win, ctx, ch))
}

/// `waddnstr (win, str, n)`.
pub fn waddnstr(w: Win, s: &[u8], n: i32) -> bool {
    on_win(w, |win, ctx| addch::waddnstr(win, ctx, s, n))
}

/// `waddstr (win, str)`.
pub fn waddstr(w: Win, s: &[u8]) -> bool {
    waddnstr(w, s, -1)
}

/// `mvwaddnstr (win, y, x, str, n)`.
pub fn mvwaddnstr(w: Win, y: i32, x: i32, s: &[u8], n: i32) -> bool {
    on_win(w, |win, ctx| {
        win.wmove(y, x) && addch::waddnstr(win, ctx, s, n)
    })
}

/// `mvwaddstr (win, y, x, str)`.
pub fn mvwaddstr(w: Win, y: i32, x: i32, s: &[u8]) -> bool {
    mvwaddnstr(w, y, x, s, -1)
}

/// `wclrtoeol (win)`.
pub fn wclrtoeol(w: Win) -> bool {
    on_win(w, |win, _| win.wclrtoeol())
}

/// `wattron (win, attrs)`.
pub fn wattron(w: Win, attrs: Attr) -> bool {
    on_win(w, |win, _| {
        win.wattr_on(attrs);
        true
    })
}

/// `wattroff (win, attrs)`.
pub fn wattroff(w: Win, attrs: Attr) -> bool {
    on_win(w, |win, _| {
        win.wattr_off(attrs);
        true
    })
}

/// `wattrset (win, attrs)`.
pub fn wattrset(w: Win, attrs: Attr) -> bool {
    on_win(w, |win, _| {
        win.wattrset(attrs);
        true
    })
}

/// `wborder (win, ls, rs, ts, bs, tl, tr, bl, br)`: the window's edges,
/// each 0 drawn with the line-drawing default.
#[allow(
    clippy::too_many_arguments,
    reason = "upstream's eight edges, in its order"
)]
pub fn wborder(
    w: Win,
    ls: Attr,
    rs: Attr,
    ts: Attr,
    bs: Attr,
    tl: Attr,
    tr: Attr,
    bl: Attr,
    br: Attr,
) -> bool {
    ok(|sp| sp.wborder(w, [ls, rs, ts, bs, tl, tr, bl, br]))
}

/// `box (win, verch, horch)`.
pub fn box_(w: Win, verch: Attr, horch: Attr) -> bool {
    wborder(w, verch, verch, horch, horch, 0, 0, 0, 0)
}

// ---- the keyboard

/// `step` on the screen until it needs nothing more from the terminal --
/// each time it must wait, the wait done with the screen let go: a real
/// `read` or `poll`, as upstream's, during which curses' own signal
/// handlers find the screen free and suspend or end the program at once,
/// and after which a signal's `SA_RESTART` has decided, as for upstream,
/// whether the wait ended. `None` without a screen, or when another call
/// holds it as this one starts.
fn reading<R>(mut step: impl FnMut(&mut Screen) -> Result<R, input::Need>) -> Option<R> {
    let mut fed = input::Fed::Nothing;
    let mut first = true;
    loop {
        let mut attempt = |slot: &mut Option<Screen>| {
            slot.as_mut().map(|sp| {
                if first {
                    sp.begin_read();
                }
                if fed != input::Fed::Nothing {
                    sp.fed = fed;
                }
                step(sp).map_err(|need| (need, sp.ifd))
            })
        };
        // Starting, the call is refused a screen another holds, as every
        // call is. Under way, it let the screen go to wait, and refused now
        // it would lose what the wait read: so it waits its turn.
        let r = if first {
            signals::with_slot(&mut attempt).flatten()
        } else {
            signals::with_slot_when_free(&mut attempt)
        }?;
        first = false;
        match r {
            Ok(v) => return Some(v),
            Err((need, fd)) => fed = need.satisfy(fd),
        }
    }
}

/// `wgetch (win)`: the next byte or key, or `ERR`.
pub fn wgetch(w: Win) -> i32 {
    reading(|sp| sp.wgetch_step(w)).unwrap_or(ERR)
}

/// `getch ()`.
pub fn getch() -> i32 {
    wgetch(STDSCR)
}

/// `wget_wch (win, &wch)`: `OK` and a character, `KEY_CODE_YES` and a key,
/// or `ERR` and 0.
pub fn wget_wch(w: Win) -> (i32, u32) {
    reading(|sp| sp.wget_wch_step(w)).unwrap_or((ERR, 0))
}

/// `get_wch (&wch)`.
pub fn get_wch() -> (i32, u32) {
    wget_wch(STDSCR)
}

/// `ungetch (ch)`.
pub fn ungetch(ch: i32) -> bool {
    ok(|sp| sp.ungetch(ch))
}

/// `keypad (win, flag)`: whether `wgetch` on `w` turns key sequences into
/// key codes; keypad mode set on the terminal now.
pub fn keypad(w: Win, flag: bool) -> bool {
    ok(|sp| {
        let Some(win) = sp.window_mut(w) else {
            return false;
        };
        win.use_keypad = flag;
        sp.keypad(flag);
        true
    })
}

/// `nodelay (win, flag)`: whether `wgetch` on `w` returns `ERR` at once
/// when nothing was typed.
pub fn nodelay(w: Win, flag: bool) -> bool {
    ok(|sp| {
        sp.window_mut(w).is_some_and(|win| {
            win.delay = if flag { 0 } else { -1 };
            true
        })
    })
}

/// `wtimeout (win, delay)`: how long `wgetch` on `w` waits, in
/// milliseconds; -1 for as long as it takes.
pub fn wtimeout(w: Win, delay: i32) {
    ok(|sp| {
        sp.window_mut(w).is_some_and(|win| {
            win.delay = delay;
            true
        })
    });
}

/// `timeout (delay)`.
pub fn timeout(delay: i32) {
    wtimeout(STDSCR, delay);
}

/// `notimeout (win, flag)`: whether `wgetch` on `w` waits for the rest of a
/// key sequence as long as it takes.
pub fn notimeout(w: Win, flag: bool) -> bool {
    ok(|sp| {
        sp.window_mut(w).is_some_and(|win| {
            win.notimeout = flag;
            true
        })
    })
}

/// `halfdelay (tenths)`.
pub fn halfdelay(tenths: i32) -> bool {
    ok(|sp| sp.halfdelay(tenths))
}

/// `meta (win, flag)`.
pub fn meta(flag: bool) -> bool {
    ok(|sp| {
        sp.meta(flag);
        true
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// No unit test installs the process's screen, so each `stdscr` call
    /// gives its answer for none.
    #[test]
    fn without_a_screen_the_stdscr_calls_answer_err() {
        assert_eq!(use_legacy_coding(1), -1);
        assert_eq!(curs_set(1), -1);
        assert_eq!(getyx(), (-1, -1));
        assert!(!idlok(true));
        assert_eq!(tigetstr(b"cup"), TermString::Cancelled);
    }
}
