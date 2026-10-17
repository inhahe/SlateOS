//! util-linux 2.39.3's `lib/mbsedit.c`: a line of text being edited in
//! place -- the cursor kept both as a byte offset and as screen cells, and
//! characters put in and taken out where it is -- for cfdisk's prompts.
//!
//! The characters are the locale's: read with `mbrtowc` from at most
//! `MB_CUR_MAX` bytes, written with `wctomb`, measured with `wcwidth`, and
//! the whole line with `mbs_safe_width`. A failed `mbrtowc` gives upstream
//! a length of `(size_t) -1` or `-2`, which its arithmetic then wraps; the
//! same lengths wrap the same way here, with every byte reached through
//! bounds-checked access. Only where upstream would move memory by such a
//! length -- removing a character the locale cannot read, which upstream's
//! own callers never ask for -- does this refuse instead (status 1).

use libcall::locale::{Mb, MbState, mb_cur_max, mbrtowc, wcrtomb, wcwidth};
use smartcols::mbs::mbs_safe_width;

/// `MBS_EDIT_LEFT`: one character back.
pub const MBS_EDIT_LEFT: i32 = 0;
/// `MBS_EDIT_RIGHT`: one character on.
pub const MBS_EDIT_RIGHT: i32 = 1;
/// `MBS_EDIT_END`: after the last character.
pub const MBS_EDIT_END: i32 = 2;
/// `MBS_EDIT_HOME`: before the first.
pub const MBS_EDIT_HOME: i32 = 3;

/// `(size_t) -1`: what `mbrtowc` and `wctomb` return for no character.
const NONE: usize = usize::MAX;
/// `(size_t) -2`: what `mbrtowc` returns for the start of one.
const INCOMPLETE: usize = usize::MAX - 1;

/// `(size_t) wcwidth (wc)`: -1 for a character it cannot show wraps to
/// `SIZE_MAX`, as C's conversion makes it.
fn cells_of(wc: i32) -> usize {
    isize::try_from(wcwidth(wc)).unwrap_or(-1).cast_unsigned()
}

/// `struct mbs_editor`.
#[derive(Clone, Debug)]
pub struct Editor {
    /// `buf`: the text, and the NUL that ends it.
    buf: Vec<u8>,
    /// `max_bytes`: the size of the caller's buffer, NUL included.
    max_bytes: usize,
    /// `cur_cells`: the cells the text takes.
    pub cur_cells: usize,
    /// `cur_bytes`: its length.
    pub cur_bytes: usize,
    /// `cursor`: where the cursor is, in bytes.
    pub cursor: usize,
    /// `cursor_cells`: and in cells.
    pub cursor_cells: usize,
    /// The locale's codeset is UTF-8, for `mbs_safe_width`.
    utf8: bool,
}

impl Editor {
    /// `mbs_new_edit (buf, bufsz, ncells)`: an editor on the C string in
    /// `buf`, whose buffer holds `bufsz` bytes. Upstream keeps `ncells`
    /// too, and reads it nowhere; so it is not kept here.
    #[must_use]
    pub fn new(buf: &[u8], bufsz: usize, utf8: bool) -> Self {
        let text = smartcols::mbs::c_str(buf);
        let mut owned = text.to_vec();
        owned.push(0);
        Self {
            cur_cells: mbs_safe_width(text, utf8),
            cur_bytes: text.len(),
            buf: owned,
            max_bytes: bufsz,
            cursor: 0,
            cursor_cells: 0,
            utf8,
        }
    }

    /// The text: the C string, up to its NUL.
    #[must_use]
    pub fn text(&self) -> &[u8] {
        smartcols::mbs::c_str(&self.buf)
    }

    /// The bytes from `at` on, NUL included, as upstream's pointer into the
    /// buffer sees them; none past the end.
    fn from(&self, at: usize) -> &[u8] {
        self.buf.get(at..).unwrap_or_default()
    }

    /// `mbs_next (str, &ncells)`: the length of the character at `at` and
    /// its cells; 0 at the end of the text.
    fn mbs_next(&self, at: usize) -> (usize, usize) {
        let s = self.from(at);
        if s.first().is_none_or(|&b| b == 0) {
            return (0, 0);
        }
        let s = s.get(..mb_cur_max().min(s.len())).unwrap_or_default();
        match mbrtowc(s, &mut MbState::new()) {
            Mb::Char(wc, n) => (n, cells_of(wc)),
            // `wc` is left as it was: 0 here.
            Mb::Invalid => (NONE, cells_of(0)),
            Mb::Incomplete => (INCOMPLETE, cells_of(0)),
        }
    }

    /// `mbs_prev (start, end, &ncells)`: the length of the character
    /// before byte `end` and its cells; 0 at the start of the text.
    fn mbs_prev(&self, end: usize) -> (usize, usize) {
        if end == 0 || self.buf.first().is_none_or(|&b| b == 0) {
            return (0, 0);
        }
        let (mut p, mut prev) = (0usize, 0usize);
        let (mut n, mut wc) = (0usize, 0i32);
        while p < end {
            let s = self.from(p);
            let s = s.get(..mb_cur_max().min(s.len())).unwrap_or_default();
            n = match mbrtowc(s, &mut MbState::new()) {
                Mb::Char(c, len) => {
                    wc = c;
                    len
                }
                Mb::Invalid => NONE,
                Mb::Incomplete => INCOMPLETE,
            };
            prev = p;
            // A NUL before `end` would stop upstream's walk for good; the
            // text has none before its end, which `end` never passes.
            if n == NONE || n == INCOMPLETE || n == 0 {
                p = p.wrapping_add(1);
            } else {
                p = p.wrapping_add(n);
            }
        }
        if prev == end {
            return (0, 0);
        }
        (n, cells_of(wc))
    }

    /// `mbs_edit_goto (edit, where)`: the cursor moved; 1 where it cannot
    /// go, `-EINVAL` for an unknown move.
    pub fn goto(&mut self, where_: i32) -> i32 {
        match where_ {
            MBS_EDIT_LEFT => {
                if self.cursor == 0 {
                    return 1;
                }
                let (n, cells) = self.mbs_prev(self.cursor);
                if n != 0 {
                    self.cursor = self.cursor.wrapping_sub(n);
                    self.cursor_cells = self.cursor_cells.wrapping_sub(cells);
                }
            }
            MBS_EDIT_RIGHT => {
                if self.cursor_cells >= self.cur_cells {
                    return 1;
                }
                let (n, cells) = self.mbs_next(self.cursor);
                if n != 0 {
                    self.cursor = self.cursor.wrapping_add(n);
                    self.cursor_cells = self.cursor_cells.wrapping_add(cells);
                }
            }
            MBS_EDIT_HOME => {
                self.cursor = 0;
                self.cursor_cells = 0;
            }
            MBS_EDIT_END => {
                self.cursor = self.cur_bytes;
                self.cursor_cells = self.cur_cells;
            }
            _ => return libcall::EINVAL.wrapping_neg(),
        }
        0
    }

    /// `mbs_edit_remove (edit)`: the character at the cursor taken out; 1
    /// when there is none.
    fn remove(&mut self) -> i32 {
        if self.cur_cells == 0 || self.cursor >= self.cur_bytes {
            return 1;
        }
        // `remove_next`.
        let (n, _cells) = self.mbs_next(self.cursor);
        if n == NONE || n == INCOMPLETE {
            return 1;
        }
        let end = self.cursor.saturating_add(n).min(self.buf.len());
        self.buf.drain(self.cursor.min(end)..end);
        self.cur_bytes = self.cur_bytes.wrapping_sub(n);
        self.cur_cells = mbs_safe_width(self.text(), self.utf8);
        0
    }

    /// `mbs_edit_delete (edit)`: the character at the cursor taken out --
    /// or, at the end of the text, the one before it.
    pub fn delete(&mut self) -> i32 {
        if self.cursor >= self.cur_bytes && self.goto(MBS_EDIT_LEFT) == 1 {
            return 1;
        }
        self.remove()
    }

    /// `mbs_edit_backspace (edit)`: the character before the cursor taken
    /// out.
    pub fn backspace(&mut self) -> i32 {
        if self.goto(MBS_EDIT_LEFT) == 0 {
            return self.remove();
        }
        1
    }

    /// `mbs_edit_insert (edit, c)`: the character `c` put in at the cursor,
    /// the cursor after it; 1 when the buffer could not take a character of
    /// the most bytes the locale has, or the locale cannot write `c`.
    pub fn insert(&mut self, c: u32) -> i32 {
        if self.cur_bytes.saturating_add(mb_cur_max()) > self.max_bytes {
            return 1;
        }
        // `mbs_insert`: `wctomb` of `(wchar_t) c`.
        let wc = c.cast_signed();
        let mut bytes = [0u8; libcall::locale::MB_LEN_MAX];
        let Some(n) = wcrtomb(wc, &mut MbState::new(), &mut bytes) else {
            return 1;
        };
        let at = self.cursor.min(self.buf.len());
        let encoded = bytes.get(..n).unwrap_or_default();
        self.buf.splice(at..at, encoded.iter().copied());
        self.cursor = self.cursor.wrapping_add(n);
        self.cursor_cells = self.cursor_cells.wrapping_add(cells_of(wc));
        self.cur_bytes = self.cur_bytes.wrapping_add(n);
        self.cur_cells = mbs_safe_width(self.text(), self.utf8);
        0
    }
}
