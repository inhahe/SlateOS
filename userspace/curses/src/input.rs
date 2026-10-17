//! The keyboard: `lib_getch.c`'s `wgetch` and `kgetch` over the queue of
//! `fifo_defs.h`, `lib_ungetch.c`, `lib_get_wch.c`'s wide characters,
//! `lib_getstr.c`'s line of cooked input, `lib_twait.c`'s wait for input,
//! the key tries (`add_tries.c`, `init_keytry.c`, `key_defined.c`), and
//! the input options of `lib_options.c` and `lib_raw.c` (`keypad`,
//! `nodelay`, `timeout`, `notimeout`, `halfdelay`, `meta`), with
//! `ESCDELAY`.
//!
//! # As Ubuntu builds it
//!
//! Without threads, `wgetch_events`, GPM or any other mouse driver: the
//! mouse hooks stay `lib_set_term.c`'s `no_mouse_*`, since nothing here
//! calls `mousemask`. A terminal's mouse sequence (`kmous`) is still a key
//! like any other, and `KEY_MOUSE` still makes `wgetch` wait
//! `_maxclick` milliseconds for more of a gesture, as upstream's does.
//! `ESCDELAY` is one variable for the whole process, as it is in the
//! library that is not built reentrant.
//!
//! # The queue
//!
//! What is read is queued before it is interpreted: `head` is the next byte
//! or key to return, `peek` the next one the key tries have not looked at,
//! `tail` where the next read goes; -1 for `head` is an empty queue and for
//! `tail` a full one. A key pushed back (`ungetch`, `KEY_RESIZE`) goes in
//! front of `head`, as already interpreted.

use core::sync::atomic::{AtomicI32, Ordering};

use terminfo::Entry;

use crate::addch;
use crate::cell::Attr;
use crate::keys::{
    KEY_BACKSPACE, KEY_CODE_YES, KEY_DOWN, KEY_ENTER, KEY_LEFT, KEY_MAX, KEY_MIN, KEY_MOUSE,
    KEY_RESIZE, TINFO_FKEYS,
};
use crate::screen::Screen;
use crate::window::{HASMOVED, ISPAD, WRAPPED};
use crate::windows::Win;

/// `ERR`.
pub const ERR: i32 = -1;
/// `OK`.
pub const OK: i32 = 0;

/// `MAXCOLUMNS`: the longest line `wgetch` reads in cooked mode.
pub const MAXCOLUMNS: i32 = 135;
/// `FIFO_SIZE`: `MAXCOLUMNS + 2`, "for nocbreak mode input".
const FIFO_SIZE: i32 = MAXCOLUMNS + 2;
/// `FIFO_SIZE - 1`: the last index.
const FIFO_LAST: i32 = FIFO_SIZE - 1;
/// `FIFO_SIZE`, as the queue's length.
const FIFO_LEN: usize = 137;
const _: () = assert!(FIFO_SIZE == 137, "FIFO_LEN is FIFO_SIZE");
/// `DEFAULT_MAXCLICK`: `_maxclick`'s first value, in milliseconds.
pub const DEFAULT_MAXCLICK: i32 = 166;
/// `(MB_LEN_MAX * 9) + 1`, `MB_LEN_MAX` being glibc's 16: `wget_wch`'s
/// buffer, which "allow[s] some redundant shifts".
const WCH_BUFFER: usize = 16 * 9 + 1;
/// `_nc_globals.getstr_limit - 1` on glibc: `LINE_MAX` (2048) less the NUL.
const GETSTR_LIMIT: i32 = 2047;

/// `ESCDELAY`: how long, in milliseconds, `wgetch` waits for the rest of a
/// key sequence once it has the start of one.
static ESCDELAY: AtomicI32 = AtomicI32::new(1000);

/// `set_escdelay (value)`: `false` (`ERR`) for a negative value.
pub fn set_escdelay(value: i32) -> bool {
    if value < 0 {
        return false;
    }
    ESCDELAY.store(value, Ordering::Release);
    true
}

/// `get_escdelay ()`.
#[must_use]
pub fn get_escdelay() -> i32 {
    ESCDELAY.load(Ordering::Acquire)
}

/// The queue of what was read and not yet returned (`_fifo` and its three
/// indices).
#[derive(Clone, Debug)]
pub struct Fifo {
    buf: [i32; FIFO_LEN],
    head: i32,
    tail: i32,
    peek: i32,
}

impl Default for Fifo {
    /// `SP_PRE_INIT`'s and `flushinp`'s: empty.
    fn default() -> Self {
        Self {
            buf: [0; FIFO_LEN],
            head: -1,
            tail: 0,
            peek: 0,
        }
    }
}

impl Fifo {
    /// `_fifo[i]`; `ERR` outside it.
    fn at(&self, i: i32) -> i32 {
        usize::try_from(i)
            .ok()
            .and_then(|i| self.buf.get(i))
            .copied()
            .unwrap_or(ERR)
    }

    /// `_fifo[i] = v`.
    fn set(&mut self, i: i32, v: i32) {
        if let Some(slot) = usize::try_from(i).ok().and_then(|i| self.buf.get_mut(i)) {
            *slot = v;
        }
    }

    /// `h_inc ()`.
    fn h_inc(&mut self) {
        self.head = if self.head >= FIFO_LAST {
            0
        } else {
            self.head.wrapping_add(1)
        };
        if self.head == self.tail {
            self.head = -1;
            self.tail = 0;
        }
    }

    /// `h_dec ()`.
    fn h_dec(&mut self) {
        self.head = if self.head <= 0 {
            FIFO_LAST
        } else {
            self.head.wrapping_sub(1)
        };
        if self.head == self.tail {
            self.tail = -1;
        }
    }

    /// `t_inc ()`.
    fn t_inc(&mut self) {
        self.tail = if self.tail >= FIFO_LAST {
            0
        } else {
            self.tail.wrapping_add(1)
        };
        if self.tail == self.head {
            self.tail = -1;
        }
    }

    /// `t_dec ()`.
    fn t_dec(&mut self) {
        self.tail = if self.tail <= 0 {
            FIFO_LAST
        } else {
            self.tail.wrapping_sub(1)
        };
        if self.head == self.tail {
            *self = Self::default();
        }
    }

    /// `p_inc ()`.
    fn p_inc(&mut self) {
        self.peek = if self.peek >= FIFO_LAST {
            0
        } else {
            self.peek.wrapping_add(1)
        };
    }

    /// `cooked_key_in_fifo ()`: something the tries have been through.
    #[must_use]
    pub fn cooked_key_in_fifo(&self) -> bool {
        self.head >= 0 && self.peek != self.head
    }

    /// `raw_key_in_fifo ()`: something they have not.
    fn raw_key_in_fifo(&self) -> bool {
        self.head >= 0 && self.peek != self.tail
    }

    /// `fifo_peek (sp)`: the next uninterpreted entry, `peek` moved past it.
    fn peek_next(&mut self) -> i32 {
        let ch = if self.peek >= 0 {
            self.at(self.peek)
        } else {
            ERR
        };
        self.p_inc();
        ch
    }

    /// `fifo_pull (sp)`: the entry at `head`, taken off.
    fn pull(&mut self) -> i32 {
        let ch = if self.head >= 0 {
            self.at(self.head)
        } else {
            ERR
        };
        if self.peek == self.head {
            self.h_inc();
            self.peek = self.head;
        } else {
            self.h_inc();
        }
        ch
    }

    /// `safe_ungetch (sp, ch)`: `ch` put in front of everything, as already
    /// interpreted; `false` (`ERR`) when the queue is full.
    pub fn unget(&mut self, ch: i32) -> bool {
        if self.tail < 0 {
            return false;
        }
        if self.head < 0 {
            self.head = 0;
            self.t_inc();
            self.peek = self.tail;
        } else {
            self.h_dec();
        }
        self.set(self.head, ch);
        true
    }
}

/// `TRIES`: one byte of one or more key sequences.
#[derive(Clone, Copy, Debug)]
struct TryNode {
    /// The next byte of the sequences this one starts.
    child: Option<usize>,
    /// Another byte at the same place.
    sibling: Option<usize>,
    /// The byte; 0 stands for a NUL, which terminfo writes as `\200`.
    ch: u8,
    /// The code of the sequence that ends here; 0 for none.
    value: u16,
}

/// The key sequences of a terminal (`sp->_keytry`), as a tree of bytes kept
/// in one vector.
#[derive(Clone, Debug, Default)]
pub struct Tries {
    nodes: Vec<TryNode>,
    root: Option<usize>,
}

/// `SET_TRY`'s byte: `\200` stored as the NUL it stands for.
const fn try_byte(b: u8) -> u8 {
    if b == 128 { 0 } else { b }
}

/// `CMP_TRY (a, b)`: whether the stored byte `a` is the string's byte `b`.
const fn cmp_try(a: u8, b: u8) -> bool {
    if a == 0 { b == 128 } else { a == b }
}

impl Tries {
    /// The node `i`.
    fn node(&self, i: usize) -> Option<&TryNode> {
        self.nodes.get(i)
    }

    /// The node `i`, to change.
    fn node_mut(&mut self, i: usize) -> Option<&mut TryNode> {
        self.nodes.get_mut(i)
    }

    /// A new node for the byte `b`, unattached.
    fn push(&mut self, b: u8) -> usize {
        self.nodes.push(TryNode {
            child: None,
            sibling: None,
            ch: try_byte(b),
            value: 0,
        });
        self.nodes.len().saturating_sub(1)
    }

    /// `_nc_add_to_try (tree, str, code)`: the sequence `s` -- up to a NUL --
    /// made to mean `code`, a later sequence the same as an earlier one
    /// replacing its code. `false` (`ERR`) for an empty sequence or code 0.
    pub fn add(&mut self, s: &[u8], code: u32) -> bool {
        let s = s.split(|&b| b == 0).next().unwrap_or_default();
        let Some(&first) = s.first() else {
            return false;
        };
        if code == 0 {
            return false;
        }
        // `(unsigned short) code`.
        let value = u16::try_from(code & 0xffff).unwrap_or(0);
        let mut txt = 0usize;
        let mut ptr;
        if let Some(root) = self.root {
            ptr = root;
            loop {
                let cmp = s.get(txt).copied().unwrap_or(0);
                while let Some(n) = self.node(ptr)
                    && !cmp_try(n.ch, cmp)
                    && let Some(sib) = n.sibling
                {
                    ptr = sib;
                }
                if self.node(ptr).is_some_and(|n| cmp_try(n.ch, cmp)) {
                    txt = txt.saturating_add(1);
                    if txt >= s.len() {
                        if let Some(n) = self.node_mut(ptr) {
                            n.value = value;
                        }
                        return true;
                    }
                    match self.node(ptr).and_then(|n| n.child) {
                        Some(c) => ptr = c,
                        None => break,
                    }
                } else {
                    let new = self.push(cmp);
                    txt = txt.saturating_add(1);
                    if let Some(n) = self.node_mut(ptr) {
                        n.sibling = Some(new);
                    }
                    ptr = new;
                    break;
                }
            }
        } else {
            ptr = self.push(first);
            txt = 1;
            self.root = Some(ptr);
        }
        // "at this point, we are adding to the try. ptr->child == 0"
        while let Some(&b) = s.get(txt) {
            let new = self.push(b);
            txt = txt.saturating_add(1);
            if let Some(n) = self.node_mut(ptr) {
                n.child = Some(new);
            }
            ptr = new;
        }
        if let Some(n) = self.node_mut(ptr) {
            n.value = value;
        }
        true
    }

    /// `find_definition (tree, str)`: `OK` when `s` is no sequence's start,
    /// `ERR` when it is only the start of longer ones, else the code of the
    /// sequence it is.
    fn find_definition(&self, tree: Option<usize>, s: &[u8]) -> i32 {
        let mut result = OK;
        let Some(&b) = s.first() else {
            return result;
        };
        let rest = s.get(1..).unwrap_or_default();
        let mut ptr = tree;
        while let Some(p) = ptr {
            let Some(n) = self.node(p) else {
                break;
            };
            if b == n.ch {
                if rest.is_empty() && n.child.is_some() {
                    result = ERR;
                } else {
                    result = self.find_definition(n.child, rest);
                    if result == OK {
                        result = i32::from(n.value);
                    } else if rest.is_empty() {
                        result = ERR;
                    }
                }
            }
            if result != OK {
                break;
            }
            ptr = n.sibling;
        }
        result
    }

    /// `key_defined (str)`: what [`Tries::find_definition`] says of `s` --
    /// up to a NUL -- from the top of the tree.
    #[must_use]
    pub fn key_defined(&self, s: &[u8]) -> i32 {
        let s = s.split(|&b| b == 0).next().unwrap_or_default();
        self.find_definition(self.root, s)
    }

    /// `_nc_init_keytry (sp)`: the tree of the terminal `entry`'s keys --
    /// each key capability of `_nc_tinfo_fkeys` in its order, then each of
    /// the entry's own string capabilities whose name starts with `k` and
    /// whose sequence is no key yet, as `KEY_MAX` and up.
    #[must_use]
    pub fn for_terminal(entry: &Entry) -> Self {
        let mut tries = Self::default();
        for &(index, code) in &TINFO_FKEYS {
            if let Some(s) = entry.string(index) {
                tries.add(s, code.cast_unsigned());
            }
        }
        for (n, name, value) in entry.ext_strings() {
            if name.first() == Some(&b'k')
                && let Some(value) = value
                && tries.key_defined(value) == OK
            {
                let code = i32::try_from(n).unwrap_or(0).wrapping_add(KEY_MAX);
                tries.add(value, code.cast_unsigned());
            }
        }
        tries
    }
}

/// `_nc_getstr_limit (n)`: the longest line `wgetnstr` takes, a negative
/// `n` asking for the most.
const fn getstr_limit(n: i32) -> i32 {
    if n < 0 || n > GETSTR_LIMIT {
        GETSTR_LIMIT
    } else {
        n
    }
}

/// `(char) c`, as C compares a `char` with an `int`: bytes from 128 up
/// negative.
fn signed_char(c: u8) -> i32 {
    i32::from(i8::from_ne_bytes([c]))
}

/// What `_nc_wgetch` returned: `ERR`, `OK` for a byte, `KEY_CODE_YES` for a
/// key, and the byte or key.
type Got = (i32, i32);

/// `_nc_wgetch`'s code for `ch`.
const fn code_of(ch: i32) -> i32 {
    if ch >= KEY_MIN { KEY_CODE_YES } else { OK }
}

/// What reading a key must wait for before it can go on: done with the
/// screen let go, so that curses' own signal handlers can suspend or end
/// the program meanwhile, as upstream's run inside its `read` and `poll`
/// ([`Need::satisfy`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Need {
    /// A byte from the input descriptor -- `read`, as long as it takes.
    Byte,
    /// Input on the input descriptor within this many milliseconds (-1 for
    /// ever) -- `poll`.
    Ready(i32),
}

/// What was got for a [`Need`], for the call that needed it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Fed {
    /// Nothing waits to be used.
    #[default]
    Nothing,
    /// The byte read, or `ERR` for the end of the input or an error (an
    /// interruption among them).
    Byte(i32),
    /// Whether input came: 1 or 0, as `_nc_timed_wait` answers.
    Ready(i32),
}

/// `_nc_timed_wait (sp, TWAIT_MASK, delay, NULL)` with no mouse: 1 when
/// input came on `fd` within `delay` milliseconds (-1 for ever), else 0 --
/// an interruption, an error and a hang-up without data among them.
fn timed_wait(fd: i32, delay: i32) -> i32 {
    let mut fds = [libcall::fd::PollFd {
        fd,
        events: libcall::fd::POLLIN,
        revents: 0,
    }];
    match libcall::fd::poll(&mut fds, delay) {
        Ok(n) if n > 0 && fds.iter().any(|f| f.revents & libcall::fd::POLLIN != 0) => 1,
        _ => 0,
    }
}

/// Whether a `read` of `fd` would return at once: something to read, the
/// end of it, or an error to report.
fn read_would_return(fd: i32) -> bool {
    let mut fds = [libcall::fd::PollFd {
        fd,
        events: libcall::fd::POLLIN,
        revents: 0,
    }];
    matches!(libcall::fd::poll(&mut fds, 0), Ok(n) if n > 0)
}

/// `read (fd, &c2, 1)` as `fifo_push` makes of it: the byte, or `ERR` for
/// the end of the input or an error.
fn read_byte(fd: i32) -> i32 {
    let mut c2 = [0u8; 1];
    match libcall::fd::read(fd, &mut c2) {
        Ok(1..) => i32::from(c2[0]),
        _ => ERR,
    }
}

impl Need {
    /// The wait itself, on the input descriptor `fd`: a real `read` or
    /// `poll`, so that whether a signal ends it is the kernel's to say, by
    /// the handler's `SA_RESTART`, as for upstream's.
    #[must_use]
    pub fn satisfy(self, fd: i32) -> Fed {
        match self {
            Self::Byte => Fed::Byte(read_byte(fd)),
            Self::Ready(ms) => Fed::Ready(timed_wait(fd, ms)),
        }
    }
}

/// A line `wgetnstr` is part way through reading for cooked mode, kept when
/// it must wait ([`Need`]) and taken up again where it stopped.
#[derive(Clone, Debug)]
pub struct CookedLine {
    /// The modes as they were read before it began, to be put back.
    buf: libcall::termios::Termios,
    /// `save_flags`: `_nl`, `_raw`, `_cbreak`, `_echo` before it began.
    save: (bool, bool, i32, bool),
    /// The erase and line-kill characters, as `char`s.
    erasec: i32,
    killc: i32,
    /// Where the line began on the window.
    y: i32,
    x: i32,
    /// The longest line it takes.
    maxlen: i32,
    /// The line so far.
    s: Vec<u8>,
}

impl Screen {
    /// The start of a read: the answer to a wait left by a read that went
    /// another way dropped -- but never a byte, which was typed.
    pub fn begin_read(&mut self) {
        if let Fed::Ready(_) = self.fed {
            self.fed = Fed::Nothing;
        }
    }

    /// `check_mouse_activity (sp, delay)` with no mouse, the screen held:
    /// for waits that are short, or that a key sequence is part way
    /// through.
    fn check_mouse_activity(&self, delay: i32) -> i32 {
        timed_wait(self.ifd, delay)
    }

    /// The same at a point where `wgetch` can be taken up again from its
    /// start: what was fed for it, or a [`Need`] -- unless the answer is
    /// there without waiting.
    fn check_fresh(&mut self, delay: i32) -> Result<i32, Need> {
        match self.fed {
            Fed::Ready(rc) => {
                self.fed = Fed::Nothing;
                Ok(rc)
            }
            // A byte was read already: there is input.
            Fed::Byte(_) => Ok(1),
            Fed::Nothing if delay == 0 => Ok(self.check_mouse_activity(0)),
            Fed::Nothing => Err(Need::Ready(delay)),
        }
    }

    /// `ch` queued at the tail, as `fifo_push` queues what it read.
    fn fifo_queue(&mut self, ch: i32) -> i32 {
        self.fifo.set(self.fifo.tail, ch);
        if self.fifo.head == -1 {
            self.fifo.head = self.fifo.tail;
            self.fifo.peek = self.fifo.tail;
        }
        self.fifo.t_inc();
        ch
    }

    /// `fifo_push (sp)`, the screen held: one byte read and queued -- or
    /// `ERR`, queued too, at the end of the input or on an error. Only where
    /// input is known to be there, or the read cannot wait.
    fn fifo_push(&mut self) -> i32 {
        if self.fifo.tail < 0 {
            return ERR;
        }
        let ch = match core::mem::take(&mut self.fed) {
            Fed::Byte(b) => b,
            _ => read_byte(self.ifd),
        };
        self.fifo_queue(ch)
    }

    /// `fifo_push (sp)` where `wgetch` can be taken up again from its start:
    /// the byte fed for it, or one there to read at once, or [`Need::Byte`].
    fn fifo_push_fresh(&mut self) -> Result<i32, Need> {
        if self.fifo.tail < 0 {
            return Ok(ERR);
        }
        if self.fed == Fed::Nothing && !read_would_return(self.ifd) {
            return Err(Need::Byte);
        }
        Ok(self.fifo_push())
    }

    /// `kgetch (sp, forever)`: a byte, or a key whose whole sequence came --
    /// each byte after the first within `ESCDELAY` milliseconds, unless
    /// `forever` -- else the bytes one at a time as they came. `fresh`: the
    /// first read may be left to the caller ([`Need`]).
    fn kgetch(&mut self, forever: bool, fresh: bool) -> Result<i32, Need> {
        let timeleft = if forever { 9_999_999 } else { get_escdelay() };
        let mut ptr = self.keytry.root;
        let mut first = fresh;
        let mut ch;
        loop {
            if self.fifo.cooked_key_in_fifo() && self.fifo.at(self.fifo.head) >= KEY_MIN {
                break;
            } else if !self.fifo.raw_key_in_fifo() {
                ch = if first {
                    self.fifo_push_fresh()?
                } else {
                    self.fifo_push()
                };
                if ch == ERR {
                    // "the keys stay uninterpreted"
                    self.fifo.peek = self.fifo.head;
                    return Ok(ERR);
                }
            }
            first = false;
            ch = self.fifo.peek_next();
            if ch >= KEY_MIN {
                // "If not first in queue, somebody put this key there on
                // purpose in emergency. Consider it higher priority than the
                // unfinished keysequence we are parsing."
                self.fifo.peek = self.fifo.head;
                // "assume the key is the last in fifo"
                self.fifo.t_dec();
                return Ok(ch);
            }
            // `(unsigned char) ch`.
            let byte = u8::try_from(ch & 0xff).unwrap_or(0);
            while let Some(p) = ptr
                && let Some(n) = self.keytry.node(p)
                && n.ch != byte
            {
                ptr = n.sibling;
            }
            let Some(n) = ptr.and_then(|p| self.keytry.node(p)).copied() else {
                break;
            };
            if n.value != 0 {
                // "sequence terminated"
                if self.fifo.peek == self.fifo.tail {
                    self.fifo = Fifo::default();
                } else {
                    self.fifo.head = self.fifo.peek;
                }
                return Ok(i32::from(n.value));
            }
            ptr = n.child;
            if !self.fifo.raw_key_in_fifo() && self.check_mouse_activity(timeleft) == 0 {
                // "ran out of time"
                break;
            }
        }
        ch = self.fifo.pull();
        self.fifo.peek = self.fifo.head;
        Ok(ch)
    }

    /// `recur_wrefresh (win)`: `w` refreshed if anything of it changed or
    /// its cursor moved.
    fn recur_wrefresh(&mut self, w: Win) {
        let moved = self.window(w).is_some_and(|win| win.flags & HASMOVED != 0);
        let pad = self.window(w).is_some_and(|win| win.flags & ISPAD != 0);
        if (self.is_wintouched(w) || moved) && !pad {
            self.wrefresh(w);
        }
    }

    /// `wechochar (win, ch)`: `ch` added to `w`, and `w` refreshed.
    fn wechochar(&mut self, w: Win, ch: Attr) -> bool {
        let added = self.with_win(w, |win, ctx| addch::waddch(win, ctx, ch));
        if added == Some(true) {
            self.wrefresh(w);
            true
        } else {
            false
        }
    }

    /// `_nc_wgetch (win, &result, use_meta)`: the next byte or key for `w`,
    /// read as the screen's modes and `w`'s options say -- or the
    /// [`Need`] it must wait for first, after which it is called again and
    /// goes on where it stopped.
    #[allow(
        clippy::too_many_lines,
        reason = "`_nc_wgetch`, step for step, in its order"
    )]
    fn nc_wgetch(&mut self, w: Win, use_meta: bool) -> Result<Got, Need> {
        if let Some(line) = self.cooked.take() {
            return self.cooked_line_done(w, line);
        }
        let Some((use_keypad, notimeout, delay)) = self
            .window(w)
            .map(|win| (win.use_keypad, win.notimeout, win.delay))
        else {
            return Ok((ERR, 0));
        };
        if self.fifo.cooked_key_in_fifo() {
            self.recur_wrefresh(w);
            let r = self.fifo.pull();
            return Ok((code_of(r), r));
        }
        // "Handle cooked mode. Grab a string from the screen, stuff its
        // contents in the FIFO queue, and pop off the first character to
        // return it."
        if self.fifo.head == -1
            && !self.tty.notty
            && !self.raw
            && self.cbreak == 0
            && !self.called_wgetch
        {
            let line = self.cooked_line_start(w, MAXCOLUMNS);
            return self.cooked_line_done(w, line);
        }
        if use_keypad != self.keypad_on {
            self.keypad(use_keypad);
        }
        self.recur_wrefresh(w);
        if (notimeout || delay >= 0 || self.cbreak > 1) && self.fifo.head == -1 {
            let delay = if self.cbreak > 1 {
                self.cbreak.wrapping_sub(1).wrapping_mul(100)
            } else {
                delay
            };
            if self.check_fresh(delay)? == 0 {
                return Ok(self.check_sigwinch());
            }
        }
        let mut ch;
        if use_keypad {
            // "We only want to get special-key events one at a time. But we
            // want to accumulate mouse events until either (a) the mouse
            // logic tells us it has picked up a complete gesture, or (b)
            // there's a detectable time lapse after one." With no mouse
            // driver, a gesture is never complete inline and always complete
            // when parsed.
            let mut runcount = 0i32;
            let mut fresh = true;
            loop {
                ch = self.kgetch(notimeout, fresh)?;
                fresh = false;
                if ch == KEY_MOUSE {
                    runcount = runcount.wrapping_add(1);
                }
                if self.maxclick < 0 || ch != KEY_MOUSE {
                    break;
                }
                if self.check_mouse_activity(self.maxclick) == 0 {
                    break;
                }
            }
            if runcount > 0 && ch != KEY_MOUSE {
                // "mouse event sequence ended by keystroke, store keystroke"
                self.fifo.unget(ch);
                ch = KEY_MOUSE;
            }
        } else {
            if self.fifo.head == -1 {
                self.fifo_push_fresh()?;
            }
            ch = self.fifo.pull();
        }
        if ch == ERR {
            return Ok(self.check_sigwinch());
        }
        // "If echo() is in effect, display the printable version of the key
        // on the screen."
        let pad = self.window(w).is_some_and(|win| win.flags & ISPAD != 0);
        if self.echo && !pad {
            let backup = if ch == KEY_BACKSPACE { 8 } else { ch };
            if backup < KEY_MIN {
                self.wechochar(w, backup.cast_unsigned());
            }
        }
        // "Simulate ICRNL mode"
        if ch == i32::from(b'\r') && self.nl {
            ch = i32::from(b'\n');
        }
        // "Strip 8th-bit if so desired."
        if !use_meta && ch < KEY_MIN && ch & 0x80 != 0 {
            ch &= 0x7f;
        }
        Ok((code_of(ch), ch))
    }

    /// `_nc_wgetch`'s `check_sigwinch`: after nothing came, a `SIGWINCH`
    /// seen to -- the screen resized, and the `KEY_RESIZE` that queues
    /// returned -- or else `ERR`.
    fn check_sigwinch(&mut self) -> Got {
        if self.handle_sigwinch() {
            self.update_screensize();
            // "resizeterm can push KEY_RESIZE"
            if self.fifo.cooked_key_in_fifo() {
                let r = self.fifo.pull();
                // "Get the ERR from queue -- it is from WINCH, so we should
                // take it out, the "error" is handled."
                if self.fifo.peek_next() == -1 {
                    self.fifo.pull();
                }
                return (code_of(r), r);
            }
        }
        (ERR, 0)
    }

    /// `wgetch (win)`, as far as it gets without waiting: the next byte or
    /// key for `w`, `ERR`, or the [`Need`] to satisfy before calling again.
    ///
    /// # Errors
    ///
    /// The [`Need`]: its answer goes in [`Screen::fed`], and the call is
    /// made again.
    pub fn wgetch_step(&mut self, w: Win) -> Result<i32, Need> {
        let (code, value) = self.nc_wgetch(w, self.use_meta)?;
        Ok(if code == ERR { ERR } else { value })
    }

    /// `wgetch (win)`: the next byte or key for `w`, or `ERR`, waiting with
    /// the screen in hand -- for a screen kept as a value, which no signal
    /// handler reaches.
    pub fn wgetch(&mut self, w: Win) -> i32 {
        self.begin_read();
        loop {
            match self.wgetch_step(w) {
                Ok(v) => return v,
                Err(need) => self.fed = need.satisfy(self.ifd),
            }
        }
    }

    /// `wget_wch (win, &wch)` as far as it gets without waiting: the next
    /// character -- its bytes put together as the locale says -- or key for
    /// `w`: `OK` and the character, `KEY_CODE_YES` and the key, or `ERR`
    /// and 0. A key in the middle of a character's bytes is pushed back,
    /// and the character is `ERR`; so is a run of bytes that never makes
    /// one. The bytes so far are kept while it waits.
    ///
    /// # Errors
    ///
    /// The [`Need`], as for [`Screen::wgetch_step`].
    pub fn wget_wch_step(&mut self, w: Win) -> Result<(i32, u32), Need> {
        loop {
            let (code, value) = self.nc_wgetch(w, true)?;
            let buffer = &mut self.wch_bytes;
            if code == ERR {
                buffer.clear();
                return Ok((ERR, value.cast_unsigned()));
            } else if code == KEY_CODE_YES {
                // "If we were processing an incomplete multibyte character,
                // return an error since we have a KEY_xxx code which
                // interrupts it."
                if buffer.is_empty() {
                    return Ok((code, value.cast_unsigned()));
                }
                buffer.clear();
                self.fifo.unget(value);
                return Ok((ERR, value.cast_unsigned()));
            } else if buffer.len().saturating_add(1) >= WCH_BUFFER {
                buffer.clear();
                self.fifo.unget(value);
                return Ok((ERR, value.cast_unsigned()));
            }
            buffer.push(u8::try_from(value & 0xff).unwrap_or(0));
            // `mblen` -- and `mbtowc`, which says the same of the same bytes:
            // the length, 0 for a NUL, -1 for no character yet or none at all.
            let (n, wc) = if buffer.first() == Some(&0) {
                (0, 0)
            } else {
                self.ctype.mbrtowc(&self.wch_bytes)
            };
            if n >= 0 {
                self.wch_bytes.clear();
                return Ok((code, wc.cast_unsigned()));
            }
        }
    }

    /// `wget_wch (win, &wch)`, waiting with the screen in hand.
    pub fn wget_wch(&mut self, w: Win) -> (i32, u32) {
        self.begin_read();
        loop {
            match self.wget_wch_step(w) {
                Ok(v) => return v,
                Err(need) => self.fed = need.satisfy(self.ifd),
            }
        }
    }

    /// `ungetch (ch)`: `ch` returned by the next `wgetch`; `false` (`ERR`)
    /// when the queue is full.
    pub fn ungetch(&mut self, ch: i32) -> bool {
        self.fifo.unget(ch)
    }

    /// `erasechar ()`: the shell's erase character, as a `char`; `ERR` when
    /// it is disabled.
    fn erasechar(&self) -> i32 {
        Self::cc_char(self.tty.ottyb.c_cc.get(libcall::termios::VERASE).copied())
    }

    /// `killchar ()`: the shell's line-kill character, likewise.
    fn killchar(&self) -> i32 {
        Self::cc_char(self.tty.ottyb.c_cc.get(libcall::termios::VKILL).copied())
    }

    /// A control character of the modes as `erasechar` returns it: `(char)`
    /// of it, or of `ERR` when it is `_POSIX_VDISABLE` (0).
    fn cc_char(c: Option<u8>) -> i32 {
        match c {
            Some(0) | None => ERR,
            Some(c) => signed_char(c),
        }
    }

    /// `WipeOut (win, y, x, first, last, echoed)`: the last byte of `s`
    /// taken off, and if it was echoed, the line drawn again from `(y, x)`
    /// and what is left of it blanked.
    fn wipe_out(&mut self, w: Win, at: (i32, i32), s: &mut Vec<u8>, echoed: bool) {
        if s.pop().is_none() || !echoed {
            return;
        }
        let shown = s.split(|&b| b == 0).next().unwrap_or_default().to_vec();
        self.with_win(w, |win, ctx| {
            let (y1, x1) = (win.cury, win.curx);
            win.wmove(at.0, at.1);
            addch::waddnstr(win, ctx, &shown, -1);
            let (y, x) = (win.cury, win.curx);
            while win.cury < y1 || (win.cury == y1 && win.curx < x1) {
                // Upstream would go on adding blanks for ever where one
                // fails without moving the cursor; this stops.
                if !addch::waddch(win, ctx, Attr::from(b' ')) {
                    break;
                }
            }
            win.wmove(i32::from(y), i32::from(x));
        });
    }

    /// The start of `wgetnstr (win, str, maxlen)`: the modes read, cbreak
    /// mode with curses' own echo off and newline mapping on, and `w`
    /// refreshed if it needs it.
    fn cooked_line_start(&mut self, w: Win, maxlen: i32) -> CookedLine {
        let buf = self.tty.get_mode().unwrap_or_default();
        let save = (self.nl, self.raw, self.cbreak, self.echo);
        self.nl = true;
        self.echo = false;
        if !save.1 {
            self.cbreak();
        }
        let (y, x) = self
            .window(w)
            .map_or((0, 0), |win| (i32::from(win.cury), i32::from(win.curx)));
        let moved = self.window(w).is_some_and(|win| win.flags & HASMOVED != 0);
        if self.is_wintouched(w) || moved {
            self.wrefresh(w);
        }
        CookedLine {
            buf,
            save,
            erasec: self.erasechar(),
            killc: self.killchar(),
            y,
            x,
            maxlen: getstr_limit(maxlen),
            s: Vec::new(),
        }
    }

    /// `wgetnstr`'s loop and end, taken up with `line`, and then
    /// `_nc_wgetch`'s cooked mode with what it read: the line's bytes queued
    /// for `wgetch` to return one at a time, a newline after them unless it
    /// ended in `ERR`.
    fn cooked_line_done(&mut self, w: Win, mut line: CookedLine) -> Result<Got, Need> {
        self.called_wgetch = true;
        let read = self.cooked_line_loop(w, &mut line);
        self.called_wgetch = false;
        let ch = match read {
            Ok(ch) => ch,
            Err(need) => {
                self.cooked = Some(line);
                return Err(need);
            }
        };
        let rc = self.cooked_line_end(w, &line, ch);
        // "ungetch in reverse order"
        if rc != ERR {
            self.fifo.unget(i32::from(b'\n'));
        }
        for &b in line.s.iter().rev() {
            self.fifo.unget(signed_char(b));
        }
        let r = self.fifo.pull();
        Ok((code_of(r), r))
    }

    /// `wgetnstr`'s loop: keys read on `w` with curses' own echo and editing
    /// -- erase and line-kill as the shell's modes have them, the left arrow
    /// and backspace keys as erase -- until a newline, a return, the
    /// down-arrow or enter key, `KEY_RESIZE` or `ERR`, which it returns; at
    /// most `maxlen` bytes kept.
    fn cooked_line_loop(&mut self, w: Win, line: &mut CookedLine) -> Result<i32, Need> {
        let echoed = line.save.3;
        loop {
            let ch = self.wgetch_step(w)?;
            if ch == ERR {
                return Ok(ch);
            }
            // "Some terminals (the Wyse-50 is the most common) generate a \n
            // from the down-arrow key."
            if ch == i32::from(b'\n') || ch == i32::from(b'\r') || ch == KEY_DOWN || ch == KEY_ENTER
            {
                let at_end = self
                    .window(w)
                    .is_some_and(|win| win.cury == win.maxy && win.scroll);
                if echoed && at_end {
                    self.wechochar(w, Attr::from(b'\n'));
                }
                return Ok(ch);
            }
            if ch == KEY_RESIZE {
                return Ok(ch);
            }
            let at = (line.y, line.x);
            if ch == line.erasec || ch == KEY_LEFT || ch == KEY_BACKSPACE {
                self.wipe_out(w, at, &mut line.s, echoed);
            } else if ch == line.killc {
                while !line.s.is_empty() {
                    self.wipe_out(w, at, &mut line.s, echoed);
                }
            } else if ch >= KEY_MIN
                || i32::try_from(line.s.len()).unwrap_or(i32::MAX) >= line.maxlen
            {
                self.beep();
            } else {
                line.s.push(u8::try_from(ch & 0xff).unwrap_or(0));
                if echoed {
                    let mut y = line.y;
                    let added = self.with_win(w, |win, ctx| {
                        let oldy = win.cury;
                        if !addch::waddch(win, ctx, ch.cast_unsigned()) {
                            // "We can't really use the lower-right corner for
                            // input, since it'll mess up bookkeeping for
                            // erases."
                            win.flags &= !WRAPPED;
                            addch::waddch(win, ctx, Attr::from(b' '));
                            return false;
                        }
                        if win.is_wrapped() {
                            // "If the last waddch forced a wrap & scroll,
                            // adjust our reference point for erasures."
                            if win.scroll && oldy == win.maxy && win.cury == win.maxy {
                                y = y.wrapping_sub(1).max(0);
                            }
                            win.flags &= !WRAPPED;
                        }
                        true
                    });
                    line.y = y;
                    if added == Some(false) {
                        self.wipe_out(w, (line.y, line.x), &mut line.s, echoed);
                        continue;
                    }
                    self.wrefresh(w);
                }
            }
        }
    }

    /// `wgetnstr`'s end, after the key `ch` ended the line: the cursor to
    /// the next line, `w` refreshed, the flags put back and the modes as
    /// they were read -- leaving the program's own modes as `cbreak` made
    /// them, as upstream does. `ERR`, `KEY_RESIZE` or `OK`.
    fn cooked_line_end(&mut self, w: Win, line: &CookedLine, ch: i32) -> i32 {
        if let Some(win) = self.window_mut(w) {
            win.curx = 0;
            win.flags &= !WRAPPED;
            if win.cury < win.maxy {
                win.cury = win.cury.wrapping_add(1);
            }
        }
        self.wrefresh(w);
        // "Restore with a single I/O call, to fix minor asymmetry between
        // raw/noraw, etc."
        (self.nl, self.raw, self.cbreak, self.echo) = line.save;
        self.tty.set_mode(&line.buf);
        if ch == ERR || ch == KEY_RESIZE {
            ch
        } else {
            OK
        }
    }

    /// `halfdelay (tenths)`: cbreak mode, and `wgetch` waiting at most
    /// `tenths` tenths of a second; `false` (`ERR`) outside 1 to 255.
    pub fn halfdelay(&mut self, tenths: i32) -> bool {
        if !(1..=255).contains(&tenths) {
            return false;
        }
        self.cbreak();
        self.cbreak = tenths.wrapping_add(1);
        true
    }

    /// `meta (win, flag)`: whether `wgetch` keeps the eighth bit, and the
    /// terminal's `smm` or `rmm` sent.
    pub fn meta(&mut self, flag: bool) {
        self.use_meta = flag;
        let cap = if flag {
            crate::caps::string::META_ON
        } else {
            crate::caps::string::META_OFF
        };
        self.t.putp_cap(cap);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::addch::Ctype;
    use crate::screen::Options;
    use crate::term::Sink;
    use crate::testing::{CLocale, Utf8};

    /// A screen on xterm-256color kept in memory, reading nothing.
    fn xterm(ctype: Box<dyn Ctype + Send>) -> Screen {
        let opts = Options {
            output: Sink::Memory(Vec::new()),
            input: -1,
            use_env: false,
        };
        Screen::newterm(b"xterm-256color", &opts, ctype).unwrap()
    }

    /// `bytes` queued as if read, not yet interpreted.
    fn typed(sp: &mut Screen, bytes: &[u8]) {
        for &b in bytes {
            sp.fifo_queue(i32::from(b));
        }
    }

    #[test]
    fn keys_pushed_back_come_out_last_in_first_out_until_the_queue_is_full() {
        let mut f = Fifo::default();
        assert!(f.unget(65));
        assert!(f.unget(66));
        assert_eq!(f.pull(), 66);
        assert_eq!(f.pull(), 65);
        assert_eq!(f.pull(), ERR);
        let mut f = Fifo::default();
        let mut n = 0;
        while f.unget(n) {
            n += 1;
        }
        assert_eq!(n, FIFO_SIZE);
    }

    #[test]
    fn a_sequence_is_a_key_and_its_prefix_is_not_one_yet() {
        let mut t = Tries::default();
        assert!(t.add(b"\x1bOB", 0o402));
        assert!(t.add(b"\x1bOA", 0o403));
        assert!(!t.add(b"", 0o404));
        assert!(!t.add(b"x", 0));
        assert_eq!(t.key_defined(b"\x1bOB"), 0o402);
        assert_eq!(t.key_defined(b"\x1bO"), ERR);
        assert_eq!(t.key_defined(b"zz"), OK);
        // A later sequence the same as an earlier one takes it over.
        assert!(t.add(b"\x1bOB", 0o405));
        assert_eq!(t.key_defined(b"\x1bOB"), 0o405);
        // terminfo's \200 is a NUL, stored as 0 and matched by 128.
        assert!(t.add(b"\x1b\x80", 0o406));
        assert_eq!(t.nodes.iter().filter(|n| n.ch == 0).count(), 1);
    }

    #[test]
    fn the_terminals_keys_are_learned_when_keypad_mode_first_goes_on() {
        let mut sp = xterm(Box::new(CLocale));
        assert!(!sp.tried);
        sp.keypad(true);
        assert!(sp.tried);
        assert_eq!(sp.keytry.key_defined(b"\x1bOB"), KEY_DOWN);
        assert_eq!(sp.keytry.key_defined(b"\x1bOP"), crate::keys::key_f(1));
    }

    #[test]
    fn wgetch_makes_a_key_of_its_sequence_and_returns_other_bytes_as_they_are() {
        let mut sp = xterm(Box::new(CLocale));
        sp.echo = false;
        sp.stdscr.use_keypad = true;
        sp.keypad(true);
        typed(&mut sp, b"\x1bOBx\x1bZ");
        assert_eq!(sp.wgetch_step(Win::Stdscr), Ok(KEY_DOWN));
        assert_eq!(sp.wgetch_step(Win::Stdscr), Ok(i32::from(b'x')));
        // An escape that starts no sequence comes back alone, then what
        // followed it.
        assert_eq!(sp.wgetch_step(Win::Stdscr), Ok(0x1b));
        assert_eq!(sp.wgetch_step(Win::Stdscr), Ok(i32::from(b'Z')));
        // Outside keypad mode the bytes are the bytes.
        sp.stdscr.use_keypad = false;
        typed(&mut sp, b"\x1bOB");
        assert_eq!(sp.wgetch_step(Win::Stdscr), Ok(0x1b));
        assert!(!sp.keypad_on, "keypad mode follows the window's");
    }

    #[test]
    fn a_return_reads_as_a_newline_in_nl_mode_and_meta_keeps_the_eighth_bit() {
        let mut sp = xterm(Box::new(CLocale));
        sp.echo = false;
        typed(&mut sp, b"\r\xe9");
        assert_eq!(sp.wgetch_step(Win::Stdscr), Ok(i32::from(b'\n')));
        sp.use_meta = false;
        assert_eq!(sp.wgetch_step(Win::Stdscr), Ok(0x69));
        sp.nl = false;
        sp.use_meta = true;
        typed(&mut sp, b"\r\xe9");
        assert_eq!(sp.wgetch_step(Win::Stdscr), Ok(i32::from(b'\r')));
        assert_eq!(sp.wgetch_step(Win::Stdscr), Ok(0xe9));
    }

    #[test]
    fn resizeterm_queues_key_resize() {
        let mut sp = xterm(Box::new(CLocale));
        sp.echo = false;
        assert!(sp.resizeterm(20, 60));
        assert_eq!(sp.wgetch_step(Win::Stdscr), Ok(KEY_RESIZE));
    }

    #[test]
    fn get_wch_puts_a_characters_bytes_together_and_a_key_interrupts_them() {
        let mut sp = xterm(Box::new(Utf8));
        sp.echo = false;
        typed(&mut sp, "é".as_bytes());
        assert_eq!(sp.wget_wch_step(Win::Stdscr), Ok((OK, 0xe9)));
        sp.stdscr.use_keypad = true;
        sp.keypad(true);
        typed(&mut sp, b"\xc3\x1bOAz");
        // The key is pushed back, and the half character is an error.
        assert_eq!(sp.wget_wch_step(Win::Stdscr), Ok((ERR, 0o403)));
        assert_eq!(sp.wget_wch_step(Win::Stdscr), Ok((KEY_CODE_YES, 0o403)));
        assert_eq!(sp.wget_wch_step(Win::Stdscr), Ok((OK, u32::from(b'z'))));
    }

    #[test]
    fn with_nothing_to_read_a_wait_is_what_wgetch_needs() {
        let mut sp = xterm(Box::new(CLocale));
        sp.echo = false;
        sp.cbreak = 1;
        // In cbreak mode with nothing queued, the read is left to the caller.
        assert_eq!(sp.wgetch_step(Win::Stdscr), Err(Need::Byte));
        sp.fed = Fed::Byte(i32::from(b'q'));
        assert_eq!(sp.wgetch_step(Win::Stdscr), Ok(i32::from(b'q')));
        // A delay is a wait too, and its answer goes back in.
        sp.stdscr.delay = 250;
        assert_eq!(sp.wgetch_step(Win::Stdscr), Err(Need::Ready(250)));
        sp.fed = Fed::Ready(0);
        assert_eq!(sp.wgetch_step(Win::Stdscr), Ok(ERR));
        // A halfdelay is counted in tenths.
        sp.stdscr.delay = -1;
        sp.cbreak = 3;
        assert_eq!(sp.wgetch_step(Win::Stdscr), Err(Need::Ready(200)));
    }

    #[test]
    fn escdelay_refuses_a_negative_value() {
        assert!(!set_escdelay(-1));
        let before = get_escdelay();
        assert!(set_escdelay(before));
        assert_eq!(get_escdelay(), before);
    }
}
