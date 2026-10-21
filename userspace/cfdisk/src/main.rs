//! cfdisk -- display or manipulate a disk partition table, full screen.
//!
//! A port of util-linux 2.39.3's `disk-utils/cfdisk.c` as Ubuntu builds it
//! -- with the one patch of its own that touches cfdisk, upstream 2.40's
//! "fix menu behavior after writing changes" -- function by function and
//! with upstream's names, on the libfdisk port, the curses port (Ubuntu
//! builds cfdisk with ncursesw) and the smartcols port; measured against
//! Ubuntu's `cfdisk` by `scripts/cfdisk-diff.sh`.
//!
//! # What is upstream's, and easy to get wrong
//!
//! * **Sizes and positions are `size_t`**, and upstream's arithmetic on
//!   them wraps where it goes below zero (`cl - 1` at column 0, a menu wider
//!   than the screen); the wrapped value is then cut to an `int` for curses,
//!   which refuses it. The same arithmetic wraps the same way here
//!   ([`c_int`]).
//! * **An empty size is taken as 0**: `ui_get_size`'s "Please, specify
//!   size." `continue`s a `do ... while (rc != 0)` loop whose condition then
//!   ends it, so the partition is made with what was typed -- nothing.
//! * **A key the line editor does not know is inserted as a character**:
//!   `get_wch`'s key code, `F1` among them, is a `wint_t` like any other.
//! * **`mvcur` writes through `putchar`**, so `ui_end`'s last move reaches
//!   standard output after curses has ended, with the newline after it.
//!
//! # What is not upstream's
//!
//! * **Table headers are not coloured** in the extra-information window's
//!   tables -- which upstream does not colour either -- and nowhere else,
//!   since `smartcols` draws no colour (known-issues
//!   `TD-B-SMARTCOLS-DRAWS-NO-COLOUR`); cfdisk's own colours, which are
//!   curses', are all there.
//! * **No debugging output**: `CFDISK_DEBUG` and `LIBFDISK_DEBUG` are not
//!   read.

stdfdguard::guard_std_fds!();

mod mbsedit;
#[cfg(test)]
mod tests;

use std::cell::RefCell;
use std::ffi::{OsStr, OsString};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};

use curses::{A_BOLD, A_REVERSE, Attr, KEY_BTAB, KEY_DC, KEY_DOWN, KEY_END, KEY_ENTER, KEY_HOME};
use curses::{KEY_BACKSPACE, KEY_LEFT, KEY_NPAGE, KEY_PPAGE, KEY_RIGHT, KEY_UP, STDSCR, Win};
use fdisk::Term;
use getoptlong::{LongVals, Opt, Program, Takes};
use libfdisk::fieldtype;
use libfdisk::{Ask, AskType, Context, LabelType, PartType, Partition, Table};
use mbsedit::{Editor, MBS_EDIT_END, MBS_EDIT_HOME, MBS_EDIT_LEFT, MBS_EDIT_RIGHT};
use quoting::{escape_unprintable, os_bytes};
use smartcols::mbs::{Align, mbs_safe_width, mbsalign};
use smartcols::{FL_RIGHT, FL_TREE, FL_TRUNC};
use ulclosestream::stderr_write;

/// `PACKAGE_STRING`.
const PACKAGE_STRING: &str = "util-linux 2.39.3";

/// `default_disks[]`: what is opened when no disk is named.
const DEFAULT_DISKS: [&[u8]; 3] = [b"/dev/sda", b"/dev/vda", b"/dev/hda"];

/// `ARROW_CURSOR_STRING`, `ARROW_CURSOR_DUMMY`, `ARROW_CURSOR_WIDTH`.
const ARROW_CURSOR_STRING: &[u8] = b">>  ";
const ARROW_CURSOR_DUMMY: &[u8] = b"    ";
const ARROW_CURSOR_WIDTH: usize = 4;

/// `MENU_V_SPADDING`: space around a vertical menu's item.
const MENU_V_SPADDING: usize = 1;
/// `MENU_H_SPADDING`: around a horizontal one's.
const MENU_H_SPADDING: usize = 0;
/// `MENU_H_BETWEEN`: between a horizontal menu's items.
const MENU_H_BETWEEN: usize = 2;
/// `MENU_H_PRESTR`, `MENU_H_POSTSTR`, and their sizes.
const MENU_H_PRESTR: &[u8] = b"[";
const MENU_H_POSTSTR: &[u8] = b"]";
const MENU_H_PRESTR_SZ: usize = 1;
const MENU_H_POSTSTR_SZ: usize = 1;
/// `MENU_TITLE_PADDING`.
const MENU_TITLE_PADDING: usize = 3;

/// `TABLE_START_LINE`.
const TABLE_START_LINE: usize = 4;

/// `ERR_ESC`: a dialog left with escape.
const ERR_ESC: isize = -5000;

/// `PATH_MAX`: the size of `ui_script_read`'s and `ui_script_write`'s
/// buffers.
const PATH_MAX: usize = 4096;

/// `-errno`, as the functions here return it.
fn neg(errno: i32) -> isize {
    isize::try_from(errno).unwrap_or(0).wrapping_neg()
}

/// `KEY_ESC`, `KEY_DELETE`: curses names neither.
const KEY_ESC: i32 = 0o33;
const KEY_DELETE: i32 = 0o177;

/// `CFDISK_CL_*`: the colour pairs.
const CFDISK_CL_WARNING: i16 = 1;
const CFDISK_CL_FREESPACE: i16 = 2;
const CFDISK_CL_INFO: i16 = 3;
/// `color_pairs[]`: each pair's foreground and background, from 1.
const COLOR_PAIRS: [(i16, i16); 4] = [
    (0, 0),
    (curses::COLOR_RED, -1),
    (curses::COLOR_GREEN, -1),
    (curses::COLOR_BLUE, -1),
];

/// `sig_resize`, `sig_die`: what the signal handlers note.
static SIG_RESIZE: AtomicBool = AtomicBool::new(false);
static SIG_DIE: AtomicBool = AtomicBool::new(false);

/// `(int) v`: a `size_t` as C converts it for an `int` parameter -- the low
/// 32 bits, as two's complement.
fn c_int(v: usize) -> i32 {
    let [a, b, c, d, ..] = v.to_le_bytes();
    i32::from_le_bytes([a, b, c, d])
}

/// `(size_t) v`: an `int` as C converts it for a `size_t`.
fn c_size(v: i32) -> usize {
    isize::try_from(v).unwrap_or(-1).cast_unsigned()
}

/// `main_menuitems[]`.
fn main_menuitems() -> Vec<MenuItem> {
    let item = |key: u8, name: &[u8], desc: &[u8]| MenuItem {
        key: i32::from(key),
        name: name.to_vec(),
        desc: Some(desc.to_vec()),
        userdata: None,
    };
    vec![
        item(
            b'b',
            b"Bootable",
            b"Toggle bootable flag of the current partition",
        ),
        item(b'd', b"Delete", b"Delete the current partition"),
        item(b'r', b"Resize", b"Reduce or enlarge the current partition"),
        item(b'n', b"New", b"Create new partition from free space"),
        item(b'q', b"Quit", b"Quit program without writing changes"),
        item(b't', b"Type", b"Change the partition type"),
        item(b'h', b"Help", b"Print help screen"),
        item(b's', b"Sort", b"Fix partitions order"),
        item(
            b'W',
            b"Write",
            b"Write partition table to disk (this might destroy data)",
        ),
        item(
            b'u',
            b"Dump",
            b"Dump partition table to sfdisk compatible script file",
        ),
    ]
}

/// `struct cfdisk_menuitem`.
#[derive(Clone, Debug)]
struct MenuItem {
    /// `key`: the keyboard shortcut.
    key: i32,
    /// `name`.
    name: Vec<u8>,
    /// `desc`: the hint.
    desc: Option<Vec<u8>>,
    /// `userdata`: the partition type, by its place in the label's list.
    userdata: Option<usize>,
}

/// `struct cfdisk_menu`.
#[derive(Clone, Debug, Default)]
struct Menu {
    /// `title`.
    title: Option<Vec<u8>>,
    /// `items`.
    items: Vec<MenuItem>,
    /// `ignore`: the keys whose items are left out, if any.
    ignore: Option<Vec<u8>>,
    /// `width`: the widest item.
    width: usize,
    /// `nitems`: the items not left out.
    nitems: usize,
    /// `page_sz`: lines per page, 0 for a menu that fits.
    page_sz: usize,
    /// `idx`: the current item.
    idx: usize,
    /// `prefkey`: the item to go to when the items change.
    prefkey: i32,
    /// `ignore_cb` set: the main menu, whose ignored keys
    /// `main_menu_ignore_keys` works out.
    main: bool,
    /// `vertical`.
    vertical: bool,
}

/// What `main_menu_ignore_keys` reads of the program: the current
/// partition (whether it is free space, `None` for none), the label, and
/// the table.
#[derive(Clone, Copy, Debug)]
struct MainKeys {
    freespace: Option<bool>,
    dos_or_sgi: bool,
    wrong_order: bool,
    readonly: bool,
}

/// The screen and the menus: what the program draws with, and what
/// libfdisk's dialogs (`ask_callback`) draw with too.
struct Ui {
    /// `ui_enabled`.
    enabled: bool,
    /// `ui_lines`, `ui_cols`.
    lines: usize,
    cols: usize,
    /// `cf->menu` and the menus under it, the current one last.
    menus: Vec<Menu>,
    /// `colors_wanted ()`.
    colors: bool,
    /// The locale's codeset is UTF-8.
    utf8: bool,
    /// Standard output, and the program's name.
    term: Rc<Term>,
}

/// `struct cfdisk_line`.
struct CfLine {
    /// `data`: the line as printed.
    data: Option<Vec<u8>>,
    /// `extra`: the partition's extra information, name and value.
    extra: Option<smartcols::Table>,
    /// `w`: the window showing it.
    w: Option<Win>,
}

/// What `get_mountpoint` keeps between calls.
#[derive(Default)]
struct Mounts {
    mtab: Option<ulmount::tab::Table>,
    fstab: Option<ulmount::tab::Table>,
    cache: Option<ulmount::cache::Cache>,
}

/// `struct cfdisk`.
struct Cfdisk {
    /// `cxt`.
    cxt: Context,
    /// `table`: the partitions and free spaces, in the order shown.
    table: Table,
    /// `original_layout`: the partitions on the disk when it was opened.
    original_layout: Option<Table>,
    /// The screen and the menus.
    ui: Rc<RefCell<Ui>>,
    /// `fields`: the columns.
    fields: Vec<i32>,
    /// `lines`: the header, then one per entry of `table`.
    lines: Vec<CfLine>,
    /// `lines_idx`: the current partition.
    lines_idx: usize,
    /// `page_sz`.
    page_sz: usize,
    /// `nwrites`.
    nwrites: u32,
    /// `act_win`: the extra-information window on the screen.
    act_win: Option<Win>,
    /// libmount's tables and cache, for mount points.
    mounts: Mounts,
    /// `wrong_order`, `zero_start`, `device_is_used`, `show_extra`.
    wrong_order: bool,
    zero_start: bool,
    device_is_used: bool,
    show_extra: bool,
}

// ---- the signal handlers

/// `sig_handler_die`.
extern "C" fn sig_handler_die(_sig: i32) {
    SIG_DIE.store(true, Ordering::SeqCst);
}

/// `sig_handler_resize`.
extern "C" fn sig_handler_resize(_sig: i32) {
    SIG_RESIZE.store(true, Ordering::SeqCst);
}

/// `sig_die`.
fn sig_die() -> bool {
    SIG_DIE.load(Ordering::SeqCst)
}

/// `sig_resize`.
fn sig_resize() -> bool {
    SIG_RESIZE.load(Ordering::SeqCst)
}

// ---- the screen

impl Ui {
    /// `MENU_START_LINE`: `ui_lines - 4`.
    fn menu_start_line(&self) -> usize {
        self.lines.wrapping_sub(4)
    }

    /// `INFO_LINE` (and `WARN_LINE`): `ui_lines - 2`.
    fn info_line(&self) -> usize {
        self.lines.wrapping_sub(2)
    }

    /// `HINT_LINE`: `ui_lines - 1`.
    fn hint_line(&self) -> usize {
        self.lines.wrapping_sub(1)
    }

    /// The current menu.
    fn menu(&self) -> Option<&Menu> {
        self.menus.last()
    }

    /// The current menu, to change.
    fn menu_mut(&mut self) -> Option<&mut Menu> {
        self.menus.last_mut()
    }

    /// `ui_end ()`: the cursor to the bottom left, shown, newline mapping
    /// back, curses ended, and a newline; `-EINVAL` when the screen was not
    /// set up.
    fn end(&mut self) -> i32 {
        if !self.enabled {
            return libcall::EINVAL.wrapping_neg();
        }
        let term = Rc::clone(&self.term);
        curses::mvcur(
            0,
            c_int(self.cols.wrapping_sub(1)),
            c_int(self.lines.wrapping_sub(1)),
            0,
            &mut |b| term.write(&[b]),
        );
        curses::curs_set(1);
        curses::nl();
        curses::endwin();
        self.term.write(b"\n");
        self.enabled = false;
        0
    }

    /// `ui_vprint_center (line, attrs, fmt, ap)`: `text` centred on `line`
    /// in `attrs` -- or, too wide for the screen, broken at its last space
    /// before the edge and shown on two lines from the left.
    fn print_center(&self, line: usize, attrs: Attr, text: &[u8]) {
        let mut line = line;
        curses::mv(c_int(line), 0);
        curses::clrtoeol();
        let width = mbs_safe_width(text, self.utf8);
        if width > self.cols {
            // `strrchr (buf + ui_cols, ' ')`: the last space from the edge
            // on, else the edge.
            let tail = text.get(self.cols..).unwrap_or_default();
            let cut = tail
                .iter()
                .rposition(|&b| b == b' ')
                .map_or(self.cols, |p| self.cols.saturating_add(p));
            let first = text.get(..cut).unwrap_or(text);
            let rest = text.get(cut.saturating_add(1)..).unwrap_or_default();
            if line.wrapping_add(1) >= self.lines {
                line = line.wrapping_sub(1);
            }
            curses::attron(attrs);
            curses::mvaddstr(c_int(line), 0, first);
            curses::mvaddstr(c_int(line.wrapping_add(1)), 0, rest);
            curses::attroff(attrs);
        } else {
            curses::attron(attrs);
            curses::mvaddstr(c_int(line), c_int(self.cols.wrapping_sub(width) / 2), text);
            curses::attroff(attrs);
        }
    }

    /// `ui_center (line, fmt, ...)`.
    fn center(&self, line: usize, text: &[u8]) {
        self.print_center(line, 0, text);
    }

    /// `COLOR_PAIR (pair)` when colours are wanted, else nothing.
    fn color(&self, pair: i16) -> Attr {
        if self.colors {
            curses::color_pair(i32::from(pair))
        } else {
            0
        }
    }

    /// `ui_warnx (fmt, ...)`: on the warning line in the warning colour, or
    /// before the screen is up, on standard error.
    fn warnx(&self, text: &[u8]) {
        if self.enabled {
            self.print_center(self.info_line(), self.color(CFDISK_CL_WARNING), text);
        } else {
            let mut m = text.to_vec();
            m.push(b'\n');
            stderr_write(&m);
        }
    }

    /// `ui_warn (fmt, ...)`: [`Ui::warnx`] with `: ` and `errnum`'s
    /// message after it (`%m`).
    fn warn(&self, text: &[u8], errnum: i32) {
        let mut m = text.to_vec();
        m.extend_from_slice(b": ");
        m.extend_from_slice(
            errmsg::strerror(&std::io::Error::from_raw_os_error(errnum)).as_bytes(),
        );
        self.warnx(&m);
    }

    /// `ui_clean_warn ()`.
    fn clean_warn(&self) {
        curses::mv(c_int(self.info_line()), 0);
        curses::clrtoeol();
    }

    /// `ui_info (fmt, ...)`: on the information line in its colour, or
    /// before the screen is up, on standard output.
    fn info(&self, text: &[u8]) {
        if self.enabled {
            self.print_center(self.info_line(), self.color(CFDISK_CL_INFO), text);
        } else {
            self.term.write(text);
            self.term.write(b"\n");
        }
    }

    /// `ui_clean_info ()`.
    fn clean_info(&self) {
        curses::mv(c_int(self.info_line()), 0);
        curses::clrtoeol();
    }

    /// `ui_hint (fmt, ...)`: on the hint line in bold.
    fn hint(&self, text: &[u8]) {
        if self.enabled {
            self.print_center(self.hint_line(), A_BOLD, text);
        } else {
            self.term.write(text);
            self.term.write(b"\n");
        }
    }

    /// `ui_clean_hint ()`.
    fn clean_hint(&self) {
        curses::mv(c_int(self.hint_line()), 0);
        curses::clrtoeol();
    }

    /// `ui_errx (rc, fmt, ...)`: the screen ended, and the program with
    /// the message.
    fn errx(&mut self, status: u8, text: &[u8]) -> ! {
        self.end();
        let mut m = self.term.short.clone();
        m.extend_from_slice(b": ");
        m.extend_from_slice(text);
        m.push(b'\n');
        stderr_write(&m);
        self.term.exit(status)
    }

    /// `ui_err (rc, fmt, ...)`: [`Ui::errx`] with `errnum`'s message.
    fn err(&mut self, status: u8, text: &[u8], errnum: i32) -> ! {
        let mut m = text.to_vec();
        m.extend_from_slice(b": ");
        m.extend_from_slice(
            errmsg::strerror(&std::io::Error::from_raw_os_error(errnum)).as_bytes(),
        );
        self.errx(status, &m)
    }

    // ---- menus

    /// `menu_refresh_size (cf)`.
    fn menu_refresh_size(&mut self) {
        let avail = self.lines.wrapping_sub(4);
        if let Some(m) = self.menu_mut()
            && m.nitems != 0
        {
            m.page_sz = if m.nitems.checked_div(avail).unwrap_or(0) != 0 {
                avail
            } else {
                0
            };
        }
    }

    /// `menu_push (cf, items)`: a new current menu.
    fn menu_push(&mut self, items: Vec<MenuItem>) {
        let mut m = Menu {
            items,
            ..Menu::default()
        };
        for d in &m.items {
            let len = mbs_safe_width(&d.name, self.utf8);
            if len > m.width {
                m.width = len;
            }
            m.nitems = m.nitems.saturating_add(1);
        }
        self.menus.push(m);
        self.menu_refresh_size();
    }

    /// `menu_pop (cf)`.
    fn menu_pop(&mut self) {
        self.menus.pop();
    }

    /// `menu_set_title (m, title)`.
    fn menu_set_title(&mut self, title: &[u8]) {
        let len = mbs_safe_width(title, self.utf8);
        if let Some(m) = self.menu_mut() {
            if len.saturating_add(MENU_TITLE_PADDING) > m.width {
                m.width = len.saturating_add(MENU_TITLE_PADDING);
            }
            m.title = Some(title.to_vec());
        }
    }

    /// `MENU_H_ITEMWIDTH (m)`.
    fn menu_h_itemwidth(m: &Menu) -> usize {
        MENU_H_PRESTR_SZ
            .wrapping_add(MENU_H_SPADDING)
            .wrapping_add(m.width)
            .wrapping_add(MENU_H_SPADDING)
            .wrapping_add(MENU_H_POSTSTR_SZ)
    }

    /// `MENU_V_ITEMWIDTH (m)`.
    fn menu_v_itemwidth(m: &Menu) -> usize {
        MENU_V_SPADDING
            .wrapping_add(m.width)
            .wrapping_add(MENU_V_SPADDING)
    }

    /// `menuitem_get_line (cf, idx)`.
    fn menuitem_get_line(&self, idx: usize) -> usize {
        let Some(m) = self.menu() else {
            return 0;
        };
        if m.vertical {
            if m.page_sz == 0 {
                // small menu
                return (self.lines.wrapping_sub(m.nitems.wrapping_add(1)) / 2).wrapping_add(idx);
            }
            return idx.checked_rem(m.page_sz).unwrap_or(0).wrapping_add(1);
        }
        let len = Self::menu_h_itemwidth(m).wrapping_add(MENU_H_BETWEEN);
        let items = self.cols.checked_div(len).unwrap_or(0);
        if items == 0 {
            return 0;
        }
        self.menu_start_line()
            .wrapping_add(idx.checked_div(items).unwrap_or(0))
    }

    /// `menuitem_get_column (cf, idx)`.
    fn menuitem_get_column(&self, idx: usize) -> i32 {
        let Some(m) = self.menu() else {
            return 0;
        };
        if m.vertical {
            let nc = Self::menu_v_itemwidth(m);
            if self.cols <= nc {
                return 0;
            }
            return c_int(self.cols.wrapping_sub(nc) / 2);
        }
        let len = Self::menu_h_itemwidth(m).wrapping_add(MENU_H_BETWEEN);
        let items = self.cols.checked_div(len).unwrap_or(0);
        let mut extra = if items < m.nitems {
            self.cols.checked_rem(len).unwrap_or(0)
        } else {
            self.cols.wrapping_sub(m.nitems.wrapping_mul(len))
        };
        if items == 0 {
            return 0;
        }
        extra = extra.wrapping_add(MENU_H_BETWEEN);
        if idx < items {
            return c_int(idx.wrapping_mul(len).wrapping_add(extra / 2));
        }
        c_int(
            idx.checked_rem(items)
                .unwrap_or(0)
                .wrapping_mul(len)
                .wrapping_add(extra / 2),
        )
    }

    /// `menuitem_on_page (cf, idx)`.
    fn menuitem_on_page(&self, idx: usize) -> bool {
        self.menu().is_none_or(|m| {
            m.page_sz == 0 || m.idx.checked_div(m.page_sz) == idx.checked_div(m.page_sz)
        })
    }

    /// `menu_get_menuitem (cf, idx)`: the `idx`th item not left out, by
    /// its place among all the items.
    fn menu_get_menuitem(&self, idx: usize) -> Option<usize> {
        let m = self.menu()?;
        let mut i = 0usize;
        for (n, d) in m.items.iter().enumerate() {
            if Self::ignored(m, d.key) {
                continue;
            }
            if i == idx {
                return Some(n);
            }
            i = i.saturating_add(1);
        }
        None
    }

    /// `menu_get_menuitem_by_key (cf, key, &idx)`: the item with `key`, by
    /// its place among all the items, and its index among those not left
    /// out.
    fn menu_get_menuitem_by_key(&self, key: i32) -> Option<(usize, usize)> {
        let m = self.menu()?;
        let mut idx = 0usize;
        for (n, d) in m.items.iter().enumerate() {
            if Self::ignored(m, d.key) {
                continue;
            }
            if key == d.key {
                return Some((n, idx));
            }
            idx = idx.saturating_add(1);
        }
        None
    }

    /// `m->ignore && strchr (m->ignore, key)`: `key` is left out. As for
    /// `strchr`, the key is a `char` and 0 finds the string's end.
    fn ignored(m: &Menu, key: i32) -> bool {
        m.ignore.as_ref().is_some_and(|ig| {
            let c = u8::try_from(key & 0xff).unwrap_or(0);
            c == 0 || ig.contains(&c)
        })
    }

    /// `menu_update_ignore (cf)`: the main menu's ignored keys worked out
    /// again from `keys`, and when they changed, its items counted again
    /// and its current item kept, or the preferred one, or the first.
    fn menu_update_ignore(&mut self, keys: &MainKeys) {
        let Some(m) = self.menu_mut() else {
            return;
        };
        let ignore = main_menu_ignore_keys(keys, m);
        // "return if no change"
        let unchanged = match &m.ignore {
            None => ignore.is_empty(),
            Some(old) => !ignore.is_empty() && *old == ignore,
        };
        if unchanged {
            return;
        }
        let org = if m.prefkey == 0 {
            self.menu_get_menuitem(self.menu().map_or(0, |m| m.idx))
                .and_then(|n| self.menu().and_then(|m| m.items.get(n)).map(|d| d.key))
        } else {
            None
        };
        let Some(m) = self.menu_mut() else {
            return;
        };
        m.ignore = Some(ignore);
        m.nitems = m.items.iter().filter(|d| !Self::ignored(m, d.key)).count();
        let prefkey = m.prefkey;
        // "refresh menu index to be at the same menuitem or go to the first"
        let idx = if let Some((_, idx)) = org.and_then(|k| self.menu_get_menuitem_by_key(k)) {
            idx
        } else if let Some((_, idx)) = (prefkey != 0)
            .then(|| self.menu_get_menuitem_by_key(prefkey))
            .flatten()
        {
            idx
        } else {
            0
        };
        if let Some(m) = self.menu_mut() {
            m.idx = idx;
        }
        self.menu_refresh_size();
    }

    /// `ui_draw_menuitem (cf, d, idx)`: item `n` (by its place among all),
    /// the `idx`th shown.
    fn draw_menuitem(&self, n: usize, idx: usize) {
        let Some(m) = self.menu() else {
            return;
        };
        let Some(d) = m.items.get(n) else {
            return;
        };
        if !self.menuitem_on_page(idx) {
            // no visible item
            return;
        }
        let ln = c_int(self.menuitem_get_line(idx));
        let cl = self.menuitem_get_column(idx);
        let vert = m.vertical;
        let mut width = if vert {
            m.width.wrapping_add(MENU_V_SPADDING)
        } else {
            MENU_H_SPADDING
                .wrapping_add(m.width)
                .wrapping_add(MENU_H_SPADDING)
        };
        let align = if vert { Align::Left } else { Align::Center };
        let mut buf = Vec::new();
        if vert {
            buf.extend_from_slice(&[b' '; MENU_V_SPADDING]);
        }
        buf.extend_from_slice(&mbsalign(&d.name, &mut width, align, b' ', self.utf8));
        if vert {
            curses::mvaddch(ln, cl.wrapping_sub(1), curses::acs(curses::ACS_VLINE));
            curses::mvaddch(
                ln,
                cl.wrapping_add(c_int(Self::menu_v_itemwidth(m))),
                curses::acs(curses::ACS_VLINE),
            );
        }
        let current = m.idx == idx;
        if current {
            curses::standout();
        }
        if vert {
            curses::mvaddstr(ln, cl, &buf);
        } else {
            let mut s = MENU_H_PRESTR.to_vec();
            s.extend_from_slice(&buf);
            s.extend_from_slice(MENU_H_POSTSTR);
            curses::mvaddstr(ln, cl, &s);
        }
        if current {
            curses::standend();
            if let Some(desc) = &d.desc {
                self.hint(desc);
            }
        }
    }

    /// `ui_clean_menu (cf)`.
    fn clean_menu(&self) {
        let Some(m) = self.menu() else {
            return;
        };
        let ln = self.menuitem_get_line(0);
        let lastline = if m.vertical {
            ln.wrapping_add(if m.page_sz != 0 { m.page_sz } else { m.nitems })
        } else {
            self.menuitem_get_line(m.nitems)
        };
        let mut i = ln;
        while i <= lastline {
            curses::mv(c_int(i), 0);
            curses::clrtoeol();
            i = i.wrapping_add(1);
            if i == 0 {
                break;
            }
        }
        if m.vertical {
            curses::mv(c_int(ln.wrapping_sub(1)), 0);
            curses::clrtoeol();
        }
        self.clean_hint();
    }

    /// `ui_draw_menu (cf)`: the current menu drawn -- the main menu's
    /// ignored keys worked out first, from `keys`.
    fn draw_menu(&mut self, keys: Option<&MainKeys>) {
        let ln = self.menuitem_get_line(0);
        self.clean_menu();
        let Some(m) = self.menu() else {
            return;
        };
        let nlines = if m.vertical {
            if m.page_sz != 0 { m.page_sz } else { m.nitems }
        } else {
            self.menuitem_get_line(m.nitems)
        };
        if m.main
            && let Some(keys) = keys
        {
            self.menu_update_ignore(keys);
        }
        let mut i = 0usize;
        while let Some(n) = self.menu_get_menuitem(i) {
            self.draw_menuitem(n, i);
            i = i.saturating_add(1);
        }
        let Some(m) = self.menu() else {
            return;
        };
        if m.vertical {
            let cl = c_size(self.menuitem_get_column(0));
            let curpg = m.idx.checked_div(m.page_sz).unwrap_or(0);
            let vw = Self::menu_v_itemwidth(m);
            let top = c_int(ln.wrapping_sub(1));
            let bottom = c_int(ln.wrapping_add(nlines));
            // corners and horizontal lines
            curses::mvaddch(
                top,
                c_int(cl.wrapping_sub(1)),
                curses::acs(curses::ACS_ULCORNER),
            );
            curses::mvaddch(
                bottom,
                c_int(cl.wrapping_sub(1)),
                curses::acs(curses::ACS_LLCORNER),
            );
            let mut i = 0usize;
            while i < vw {
                curses::mvaddch(
                    top,
                    c_int(cl.wrapping_add(i)),
                    curses::acs(curses::ACS_HLINE),
                );
                curses::mvaddch(
                    bottom,
                    c_int(cl.wrapping_add(i)),
                    curses::acs(curses::ACS_HLINE),
                );
                i = i.wrapping_add(1);
            }
            curses::mvaddch(
                top,
                c_int(cl.wrapping_add(i)),
                curses::acs(curses::ACS_URCORNER),
            );
            curses::mvaddch(
                bottom,
                c_int(cl.wrapping_add(i)),
                curses::acs(curses::ACS_LRCORNER),
            );
            // "draw also lines around empty lines on last page"
            if m.page_sz != 0 && m.nitems.checked_div(m.page_sz) == m.idx.checked_div(m.page_sz) {
                let mut i = m.nitems.checked_rem(m.page_sz).unwrap_or(0).wrapping_add(1);
                while i <= m.page_sz {
                    curses::mvaddch(
                        c_int(i),
                        c_int(cl.wrapping_sub(1)),
                        curses::acs(curses::ACS_VLINE),
                    );
                    curses::mvaddch(
                        c_int(i),
                        c_int(cl.wrapping_add(vw)),
                        curses::acs(curses::ACS_VLINE),
                    );
                    i = i.wrapping_add(1);
                }
            }
            if let Some(title) = &m.title {
                curses::attron(A_BOLD);
                let mut s = b" ".to_vec();
                s.extend_from_slice(title);
                s.push(b' ');
                curses::mvaddstr(top, c_int(cl), &s);
                curses::attroff(A_BOLD);
            }
            if curpg != 0 {
                curses::mvaddch(
                    top,
                    c_int(cl.wrapping_add(vw).wrapping_sub(2)),
                    curses::acs(curses::ACS_UARROW),
                );
            }
            if m.page_sz != 0 && curpg < m.nitems.checked_div(m.page_sz).unwrap_or(0) {
                curses::mvaddch(
                    bottom,
                    c_int(cl.wrapping_add(vw).wrapping_sub(2)),
                    curses::acs(curses::ACS_DARROW),
                );
            }
        }
    }

    /// `ui_menu_goto (cf, where)`: the current item moved -- held at either
    /// end of a vertical menu, round to the other end of a horizontal one.
    fn menu_goto(&mut self, where_: i32, keys: Option<&MainKeys>) {
        let Some(m) = self.menu() else {
            return;
        };
        let nitems = m.nitems;
        let mut where_ = where_;
        if m.vertical {
            if where_ < 0 {
                where_ = 0;
            } else if where_ > c_int(nitems).wrapping_sub(1) {
                where_ = c_int(nitems.wrapping_sub(1));
            }
        } else if where_ < 0 {
            where_ = c_int(nitems.wrapping_sub(1));
        } else if c_size(where_) > nitems.wrapping_sub(1) {
            where_ = 0;
        }
        if c_size(where_) == m.idx {
            return;
        }
        self.clean_info();
        let old = m.idx;
        if let Some(m) = self.menu_mut() {
            m.idx = c_size(where_);
        }
        if !self.menuitem_on_page(old) {
            self.draw_menu(keys);
            return;
        }
        if let Some(n) = self.menu_get_menuitem(old) {
            self.draw_menuitem(n, old);
        }
        let new = c_size(where_);
        if let Some(n) = self.menu_get_menuitem(new) {
            self.draw_menuitem(n, new);
        }
    }

    /// `ui_menu_move (cf, key)`: 0 when `key` moved in the menu (or was
    /// `ERR`, or ^L, which redraws it), 1 when it is not a menu key.
    fn menu_move(&mut self, key: i32, keys: Option<&MainKeys>) -> i32 {
        if key == curses::ERR {
            // "ignore errors"
            return 0;
        }
        let Some(m) = self.menu() else {
            return 1;
        };
        let (idx, page_sz, nitems) = (c_int(m.idx), c_int(m.page_sz), c_int(m.nitems));
        if m.vertical {
            match key {
                KEY_DOWN | 0o16 | 0x6a => {
                    self.menu_goto(idx.wrapping_add(1), keys);
                    return 0;
                }
                KEY_UP | 0o20 | 0x6b => {
                    self.menu_goto(idx.wrapping_sub(1), keys);
                    return 0;
                }
                KEY_PPAGE | KEY_HOME => {
                    let to = if key == KEY_PPAGE && page_sz != 0 {
                        idx.wrapping_sub(page_sz)
                    } else {
                        0
                    };
                    self.menu_goto(to, keys);
                    return 0;
                }
                KEY_NPAGE | KEY_END => {
                    let to = if key == KEY_NPAGE && page_sz != 0 {
                        idx.wrapping_add(page_sz)
                    } else {
                        nitems
                    };
                    self.menu_goto(to, keys);
                    return 0;
                }
                _ => {}
            }
        } else {
            match key {
                KEY_RIGHT | 0x09 => {
                    self.menu_goto(idx.wrapping_add(1), keys);
                    return 0;
                }
                KEY_LEFT | KEY_BTAB => {
                    self.menu_goto(idx.wrapping_sub(1), keys);
                    return 0;
                }
                _ => {}
            }
        }
        if key == 0o14 {
            // ^L refresh
            self.menu_resize(keys);
            return 0;
        }
        1
    }

    /// `ui_menu_resize (cf)`: for the dialogs' menus.
    fn menu_resize(&mut self, keys: Option<&MainKeys>) {
        self.resize();
        self.clean_menu();
        self.menu_refresh_size();
        self.draw_menu(keys);
        curses::refresh();
    }

    /// `resize ()`: the screen made the terminal's size, if it has one, and
    /// all of it drawn again at the next refresh.
    fn resize(&mut self) {
        if let Ok(ws) = libcall::pty::window_size(1)
            && ws.rows != 0
            && ws.cols != 0
        {
            self.lines = usize::from(ws.rows);
            self.cols = usize::from(ws.cols);
            curses::resizeterm(i32::from(ws.rows), i32::from(ws.cols));
            curses::clearok(STDSCR, true);
        }
        curses::touchwin(STDSCR);
        SIG_RESIZE.store(false, Ordering::SeqCst);
    }
}

/// `main_menu_ignore_keys (cf, ignore, sz)`: the main menu's keys that do
/// nothing for the current partition, and its preferred item set.
fn main_menu_ignore_keys(keys: &MainKeys, m: &mut Menu) -> Vec<u8> {
    let mut ignore = Vec::new();
    let Some(freespace) = keys.freespace else {
        return ignore;
    };
    if freespace {
        ignore.extend_from_slice(b"dtbr");
        m.prefkey = i32::from(b'n');
    } else {
        m.prefkey = i32::from(b'q');
        ignore.push(b'n');
        if !keys.dos_or_sgi {
            ignore.push(b'b');
        }
    }
    if !keys.wrong_order {
        ignore.push(b's');
    }
    if keys.readonly {
        ignore.push(b'W');
    }
    ignore
}

// ---- the dialogs libfdisk asks for

/// `ask_menu (ask, cf)`: libfdisk's menu as a cfdisk menu, and the item
/// chosen given back; -1 when it was left.
fn ask_menu(ui: &RefCell<Ui>, ask: &mut Ask) -> i32 {
    let nitems = ask.menu_get_nitems();
    let mut cm = Vec::with_capacity(nitems);
    for i in 0..nitems {
        let Some(item) = ask.menu_get_item(i) else {
            break;
        };
        cm.push(MenuItem {
            key: item.key,
            name: item.name.as_bytes().to_vec(),
            desc: (!item.desc.is_empty()).then(|| item.desc.as_bytes().to_vec()),
            userdata: None,
        });
    }
    {
        let mut u = ui.borrow_mut();
        // "make the new menu active"
        u.menu_push(cm);
        u.draw_menu(None);
    }
    curses::refresh();
    // "wait for keys"
    while !sig_die() {
        let key = curses::getch();
        if sig_die() {
            break;
        }
        let mut u = ui.borrow_mut();
        if sig_resize() {
            u.menu_resize(None);
        }
        if u.menu_move(key, None) == 0 {
            continue;
        }
        if key == KEY_ENTER || key == 0x0a || key == 0x0d {
            let idx = u.menu().map_or(0, |m| m.idx);
            let d = u
                .menu_get_menuitem(idx)
                .and_then(|n| u.menu().and_then(|m| m.items.get(n)).map(|d| d.key));
            if let Some(k) = d {
                ask.menu_set_result(k);
            }
            u.menu_pop();
            return 0;
        }
    }
    if sig_die() {
        die_on_signal(ui);
    }
    ui.borrow_mut().menu_pop();
    -1
}

/// `ask_callback (cxt, ask, data)`: libfdisk's messages on the screen's
/// lines, and its menus as cfdisk menus.
fn ask_callback(ui: &RefCell<Ui>, ask: &mut Ask) -> i32 {
    let mesg = || ask.print_get_mesg().unwrap_or_default().to_vec();
    match ask.get_type() {
        AskType::Info => ui.borrow().info(&mesg()),
        AskType::Warnx => ui.borrow().warnx(&mesg()),
        AskType::Warn => ui.borrow().warn(&mesg(), ask.print_get_errno()),
        AskType::Menu => {
            ask_menu(ui, ask);
        }
        other => {
            let code = match other {
                AskType::None => 0,
                AskType::Number => 1,
                AskType::Offset => 2,
                AskType::Warn => 3,
                AskType::Warnx => 4,
                AskType::Info => 5,
                AskType::YesNo => 6,
                AskType::String => 7,
                AskType::Menu => 8,
            };
            ui.borrow()
                .warnx(format!("internal error: unsupported dialog type {code}").as_bytes());
            return libcall::EINVAL.wrapping_neg();
        }
    }
    0
}

/// `die_on_signal ()`: the screen ended, and the program with status 1.
fn die_on_signal(ui: &RefCell<Ui>) -> ! {
    let mut u = ui.borrow_mut();
    u.end();
    u.term.exit(1)
}

// ---- the program

/// `iszero (str)`: nothing but zeros.
fn iszero(s: &[u8]) -> bool {
    s.iter().all(|&b| b == b'0')
}

/// The same partition type: what upstream's pointer comparison finds, a
/// partition's type being the label's own entry for it.
fn same_type(a: &PartType, b: &PartType) -> bool {
    !a.is_unknown()
        && !b.is_unknown()
        && a.get_code() == b.get_code()
        && a.get_string() == b.get_string()
        && a.get_name() == b.get_name()
}

impl Cfdisk {
    /// What `main_menu_ignore_keys` reads, read now.
    fn main_keys(&self) -> MainKeys {
        MainKeys {
            freespace: self.get_current_partition().map(Partition::is_freespace),
            dos_or_sgi: self.cxt.is_labeltype(LabelType::Dos)
                || self.cxt.is_labeltype(LabelType::Sgi),
            wrong_order: self.wrong_order,
            readonly: self.cxt.is_readonly(),
        }
    }

    /// `ui_draw_menu (cf)`, from the program.
    fn draw_menu(&self) {
        let keys = self.main_keys();
        self.ui.borrow_mut().draw_menu(Some(&keys));
    }

    /// `ui_menu_move (cf, key)`, from the program.
    fn menu_move(&self, key: i32) -> i32 {
        let keys = self.main_keys();
        self.ui.borrow_mut().menu_move(key, Some(&keys))
    }

    /// `cols_init (cf)`: the label's columns.
    fn cols_init(&mut self) -> i32 {
        match self.cxt.label_get_fields_ids() {
            Ok(ids) => {
                self.fields = ids;
                0
            }
            Err(rc) => {
                self.fields.clear();
                rc
            }
        }
    }

    /// `table_to_string (cf, tb)`: the table as the lines to show -- in a
    /// tree when anything is nested -- and `tb` put in the order they show
    /// it. `None` when there is nothing to show.
    fn table_to_string(&mut self) -> Option<Vec<u8>> {
        let tree = self.table.iter().any(Partition::is_nested);
        let lb_fields: Vec<(Vec<u8>, f64, u32)> = {
            let lb = self.cxt.label_ref()?;
            self.fields
                .iter()
                .filter_map(|&id| {
                    let field = lb.get_field(id)?;
                    let mut fl = 0;
                    if field.is_number() {
                        fl |= FL_RIGHT;
                    }
                    if field.get_id() == fieldtype::TYPE {
                        fl |= FL_TRUNC;
                    }
                    if tree && field.get_id() == fieldtype::DEVICE {
                        fl |= FL_TREE;
                    }
                    Some((field.get_name().as_bytes().to_vec(), field.get_width(), fl))
                })
                .collect()
        };
        let mut table = smartcols::Table::new();
        // Neither can fail on a table with no lines yet.
        let _ = table.enable_maxout(true);
        table.enable_nowrap(true);
        for (name, width, fl) in &lb_fields {
            let _ = table.new_column(name, *width, *fl);
        }

        // data
        let mut ln_cont: Option<smartcols::LineId> = None;
        let mut owners: Vec<(smartcols::LineId, Partition)> = Vec::new();
        let parts: Vec<Partition> = self.table.iter().cloned().collect();
        for mut pa in parts {
            let parent = if pa.is_nested() { ln_cont } else { None };
            let ln = table.new_line(parent).ok()?;
            for (i, &id) in self.fields.iter().enumerate() {
                let Ok(Some(cdata)) = self.cxt.partition_to_string(&mut pa, id) else {
                    continue;
                };
                // A line and a column this function made.
                let _ = table.line_refer_data(ln, i, &cdata);
            }
            if tree && pa.is_container() {
                ln_cont = Some(ln);
            }
            owners.push((ln, pa));
        }
        if table.nlines() == 0 {
            return None;
        }
        table.reduce_termwidth(ARROW_CURSOR_WIDTH);
        let res = table.print_to_string().ok();

        // "scols_* code might reorder lines, let's reorder @tb according to
        // the final output"
        self.table.reset();
        for ln in table.line_ids() {
            if table.line_parent(ln).is_some() {
                continue;
            }
            partition_from_scols(&mut self.table, &table, &mut owners, ln);
        }
        res
    }

    /// `cfdisk_free_lines (cf)`: the lines gone, and their windows.
    fn free_lines(&mut self) {
        for l in &mut self.lines {
            if let Some(w) = l.w.take() {
                curses::delwin(w);
            }
        }
        self.act_win = None;
        self.lines.clear();
    }

    /// `lines_refresh (cf)`: the partitions and free spaces read again, and
    /// the lines to show made from them.
    fn lines_refresh(&mut self) -> i32 {
        self.free_lines();
        self.table = Table::new();
        // "read partitions and free spaces into cf->table"
        let mut tb = Table::new();
        let mut rc = self.cxt.get_partitions(&mut tb);
        if rc == 0 {
            rc = self.cxt.get_freespaces(&mut tb);
        }
        if rc != 0 {
            return rc;
        }
        self.table = tb;
        let Some(linesbuf) = self.table_to_string() else {
            return libfdisk::errno::ENOMEM.wrapping_neg();
        };
        let nlines = self.table.get_nents().saturating_add(1);
        self.page_sz = 0;
        self.wrong_order = self.table.wrong_order();
        let (start, menu) = {
            let u = self.ui.borrow();
            (TABLE_START_LINE, u.menu_start_line())
        };
        if menu.wrapping_sub(start) < nlines {
            self.page_sz = menu.wrapping_sub(start).wrapping_sub(1);
        }
        let mut rest: Option<&[u8]> = Some(&linesbuf);
        for _ in 0..nlines {
            let Some(p) = rest else {
                self.lines.push(CfLine {
                    data: None,
                    extra: None,
                    w: None,
                });
                continue;
            };
            let (data, next) = match p.iter().position(|&b| b == b'\n') {
                Some(at) => (
                    p.get(..at).unwrap_or_default(),
                    p.get(at.saturating_add(1)..),
                ),
                None => (p, None),
            };
            rest = next;
            let mut extra = smartcols::Table::new();
            extra.enable_noheadings(true);
            let _ = extra.new_unnamed_column(0.0, FL_RIGHT);
            let _ = extra.new_unnamed_column(0.0, FL_TRUNC);
            self.lines.push(CfLine {
                data: Some(data.to_vec()),
                extra: Some(extra),
                w: None,
            });
        }
        0
    }

    /// `get_current_partition (cf)`.
    fn get_current_partition(&self) -> Option<&Partition> {
        self.table.get_partition(self.lines_idx)
    }

    /// `is_freespace (cf, i)`.
    fn is_freespace(&self, i: usize) -> bool {
        self.table
            .get_partition(i)
            .is_some_and(Partition::is_freespace)
    }

    /// `get_mountpoint (cf, tagname, tagdata)`: where the filesystem the tag
    /// names is mounted -- "(mounted)" after it -- or else where fstab would
    /// mount it.
    fn get_mountpoint(&mut self, tagname: &[u8], tagdata: &[u8]) -> Option<Vec<u8>> {
        use ulmount::tab::{Direction, Table as MntTable};
        let m = &mut self.mounts;
        if m.cache.is_none() {
            m.cache = Some(ulmount::cache::Cache::new());
        }
        // "1st try between mounted filesystems"
        if m.mtab.is_none() {
            let mut tb = MntTable::new();
            // Upstream does not look at the result: an unreadable mtab is an
            // empty one.
            let _ = ulmount::tab_parse::parse_mtab(&mut tb, None, None);
            m.mtab = Some(tb);
        }
        let mut target = None;
        let mut mounted = false;
        if let Some(tb) = &m.mtab
            && let Some(i) = tb.find_tag(tagname, tagdata, Direction::Forward, m.cache.as_mut())
        {
            target = tb
                .ents
                .get(i)
                .map(|fs| fs.target.clone().unwrap_or_default());
            mounted = true;
        }
        // "2nd try fstab"
        if target.is_none() {
            if m.fstab.is_none() {
                let mut tb = MntTable::new();
                if ulmount::tab_parse::parse_fstab(&mut tb, None, None).is_ok() {
                    m.fstab = Some(tb);
                }
            }
            if let Some(tb) = &m.fstab
                && let Some(i) = tb.find_tag(tagname, tagdata, Direction::Forward, m.cache.as_mut())
            {
                target = tb
                    .ents
                    .get(i)
                    .map(|fs| fs.target.clone().unwrap_or_default());
            }
        }
        target.map(|t| {
            if mounted {
                let mut s = t;
                s.extend_from_slice(b" (mounted)");
                s
            } else {
                t
            }
        })
    }

    /// `has_uuid (tb, uuid)`: a partition of the disk as it was opened has
    /// `uuid`.
    fn has_uuid(&self, uuid: &[u8]) -> bool {
        self.original_layout
            .as_ref()
            .is_some_and(|tb| tb.iter().any(|pa| pa.get_uuid() == Some(uuid)))
    }

    /// The text of field `id` of the current partition, if it has any.
    fn current_field(&mut self, id: i32) -> Option<Vec<u8>> {
        let mut pa = self.get_current_partition()?.clone();
        // A field that cannot be read is one without text: the extra window
        // leaves it out, as upstream's `if (!fdisk_partition_to_string (...)
        // && data)` does.
        self.cxt.partition_to_string(&mut pa, id).ok().flatten()
    }

    /// `extra_insert_pair (l, name, data)`.
    fn extra_insert_pair(&mut self, name: &[u8], data: &[u8]) {
        if data.is_empty() {
            return;
        }
        let Some(extra) = self
            .lines
            .get_mut(self.lines_idx)
            .and_then(|l| l.extra.as_mut())
        else {
            return;
        };
        let Ok(lsl) = extra.new_line(None) else {
            return;
        };
        // The table's own new line, and its two columns.
        let _ = extra.line_refer_data(lsl, 0, name);
        let _ = extra.line_refer_data(lsl, 1, data);
    }

    /// `extra_prepare_data (cf)`: the current partition's extra
    /// information, name and value.
    fn extra_prepare_data(&mut self) {
        let mut mountpoint: Option<Vec<u8>> = None;
        // "string data should not equal an empty string"
        if let Some(data) = self.current_field(fieldtype::NAME) {
            self.extra_insert_pair(b"Partition name:", &data);
            if mountpoint.is_none() {
                mountpoint = self.get_mountpoint(b"PARTLABEL", &data);
            }
        }
        if let Some(data) = self.current_field(fieldtype::UUID) {
            self.extra_insert_pair(b"Partition UUID:", &data);
            // "Search for mountpoint by PARTUUID= ... Let's check if the
            // UUID already exist in the old layout, otherwise ignore it."
            if mountpoint.is_none() && self.has_uuid(&data) {
                mountpoint = self.get_mountpoint(b"PARTUUID", &data);
            }
        }
        if let Some(data) = self.current_field(fieldtype::TYPE) {
            let code = self.current_field(fieldtype::TYPEID);
            let mut ty = data;
            ty.extend_from_slice(b" (");
            ty.extend_from_slice(code.as_deref().unwrap_or(b"(null)"));
            ty.push(b')');
            self.extra_insert_pair(b"Partition type:", &ty);
        }
        if let Some(data) = self.current_field(fieldtype::ATTR) {
            self.extra_insert_pair(b"Attributes:", &data);
        }
        // "for numeric data, only show non-zero rows"
        for (id, name) in [
            (fieldtype::BSIZE, &b"BSIZE:"[..]),
            (fieldtype::CPG, b"CPG:"),
            (fieldtype::FSIZE, b"FSIZE:"),
        ] {
            if let Some(data) = self.current_field(id)
                && !iszero(&data)
            {
                self.extra_insert_pair(name, &data);
            }
        }
        if let Some(data) = self.current_field(fieldtype::FSUUID) {
            self.extra_insert_pair(b"Filesystem UUID:", &data);
            if mountpoint.is_none() {
                mountpoint = self.get_mountpoint(b"UUID", &data);
            }
        }
        if let Some(data) = self.current_field(fieldtype::FSLABEL) {
            self.extra_insert_pair(b"Filesystem LABEL:", &data);
            if mountpoint.is_none() {
                mountpoint = self.get_mountpoint(b"LABEL", &data);
            }
        }
        if let Some(data) = self.current_field(fieldtype::FSTYPE) {
            self.extra_insert_pair(b"Filesystem:", &data);
        }
        if let Some(mp) = mountpoint {
            self.extra_insert_pair(b"Mountpoint:", &mp);
        }
    }

    /// `ui_draw_extra (cf)`: the current partition's extra information in
    /// a box under the table, as far as there is room; 1 when there is
    /// none.
    fn draw_extra(&mut self) -> i32 {
        if !self.show_extra {
            return 0;
        }
        if let Some(w) = self.act_win {
            curses::wclear(w);
            curses::touchwin(STDSCR);
        }
        let empty = |cf: &Self| {
            cf.lines
                .get(cf.lines_idx)
                .and_then(|l| l.extra.as_ref())
                .is_none_or(|t| t.nlines() == 0)
        };
        if empty(self) {
            self.extra_prepare_data();
            if empty(self) {
                return 0;
            }
        }
        let (menu_start, cols) = {
            let u = self.ui.borrow();
            (u.menu_start_line(), u.cols)
        };
        let ndatalines = c_int(self.table.get_nents()).wrapping_add(1);
        // "nents + header + one free line"
        let mut win_ex_start_line = c_int(TABLE_START_LINE).wrapping_add(ndatalines);
        let mut win_height = c_int(menu_start).wrapping_sub(win_ex_start_line);
        let Some(extra) = self
            .lines
            .get_mut(self.lines_idx)
            .and_then(|l| l.extra.as_mut())
        else {
            return 0;
        };
        let tblen = c_int(extra.nlines());
        // "we can't get a single line of data under the partlist"
        if win_height < 3 {
            return 1;
        }
        // "number of data lines + 2 for top/bottom lines"
        if win_height >= tblen.wrapping_add(2) {
            win_height = tblen.wrapping_add(2);
        }
        if c_size(win_ex_start_line)
            .wrapping_add(c_size(win_height))
            .wrapping_add(1)
            < menu_start
        {
            win_ex_start_line = c_int(menu_start).wrapping_sub(win_height);
        }
        let win_ex = curses::subwin(
            STDSCR,
            win_height,
            c_int(cols.wrapping_sub(2)),
            win_ex_start_line,
            1,
        );
        extra.reduce_termwidth(4);
        // Upstream ignores this call's result too; a table that cannot be
        // printed draws an empty window rather than ending the program.
        let tbstr = extra.print_to_string().unwrap_or_default();
        if let Some(w) = win_ex {
            curses::box_(w, 0, 0);
        }
        let mut rows = tbstr.split(|&b| b == b'\n');
        let mut wline = 1;
        loop {
            win_height = win_height.wrapping_sub(1);
            if win_height <= 1 {
                break;
            }
            let row = rows.next().unwrap_or_default();
            if let Some(w) = win_ex {
                curses::mvwaddstr(w, wline, 1, row);
            }
            wline = wline.wrapping_add(1);
        }
        if let Some(old) = self.lines.get_mut(self.lines_idx).and_then(|l| l.w.take()) {
            curses::delwin(old);
        }
        curses::touchwin(STDSCR);
        if let Some(w) = win_ex {
            curses::wrefresh(w);
        }
        self.act_win = win_ex;
        if let Some(l) = self.lines.get_mut(self.lines_idx) {
            l.w = win_ex;
        }
        0
    }

    /// `partition_on_page (cf, i)`.
    fn partition_on_page(&self, i: usize) -> bool {
        self.page_sz == 0 || self.lines_idx.checked_div(self.page_sz) == i.checked_div(self.page_sz)
    }

    /// `ui_draw_partition (cf, i)`.
    fn draw_partition(&self, i: usize) {
        let mut ln = c_int(TABLE_START_LINE.wrapping_add(1).wrapping_add(i));
        let cl = c_int(ARROW_CURSOR_WIDTH);
        let cur = self.lines_idx == i;
        let mut curpg = 0usize;
        if self.page_sz != 0 {
            if !self.partition_on_page(i) {
                return;
            }
            ln = c_int(
                TABLE_START_LINE
                    .wrapping_add(i.checked_rem(self.page_sz).unwrap_or(0))
                    .wrapping_add(1),
            );
            curpg = self.lines_idx.checked_div(self.page_sz).unwrap_or(0);
        }
        let data = self
            .lines
            .get(i.wrapping_add(1))
            .and_then(|l| l.data.clone())
            .unwrap_or_default();
        let (colors, menu_start, cols) = {
            let u = self.ui.borrow();
            (u.colors, u.menu_start_line(), u.cols)
        };
        if cur {
            curses::attron(A_REVERSE);
            curses::mvaddstr(ln, 0, ARROW_CURSOR_STRING);
            curses::mvaddstr(ln, cl, &data);
            curses::attroff(A_REVERSE);
        } else {
            let at = colors && self.is_freespace(i);
            if at {
                curses::attron(curses::color_pair(i32::from(CFDISK_CL_FREESPACE)));
            }
            curses::mvaddstr(ln, 0, ARROW_CURSOR_DUMMY);
            curses::mvaddstr(ln, cl, &data);
            if at {
                curses::attroff(curses::color_pair(i32::from(CFDISK_CL_FREESPACE)));
            }
        }
        if c_size(ln) == menu_start.wrapping_sub(1)
            && self.page_sz != 0
            && curpg < self.lines.len().checked_div(self.page_sz).unwrap_or(0)
        {
            if cur {
                curses::attron(A_REVERSE);
            }
            curses::mvaddch(
                ln,
                c_int(cols.wrapping_sub(1)),
                curses::acs(curses::ACS_DARROW),
            );
            curses::mvaddch(ln, 0, curses::acs(curses::ACS_DARROW));
            if cur {
                curses::attroff(A_REVERSE);
            }
        }
    }

    /// `ui_draw_table (cf)`.
    fn draw_table(&mut self) -> i32 {
        let cl = c_int(ARROW_CURSOR_WIDTH);
        let nparts = self.table.get_nents();
        let curpg = self.lines_idx.checked_div(self.page_sz).unwrap_or(0);
        let mut i = TABLE_START_LINE;
        while i <= TABLE_START_LINE.wrapping_add(self.page_sz) {
            curses::mv(c_int(i), 0);
            curses::clrtoeol();
            i = i.wrapping_add(1);
        }
        if nparts == 0 || self.lines_idx > nparts.wrapping_sub(1) {
            self.lines_idx = nparts.saturating_sub(1);
        }
        // print header
        curses::attron(A_BOLD);
        let header = self
            .lines
            .first()
            .and_then(|l| l.data.clone())
            .unwrap_or_default();
        curses::mvaddstr(c_int(TABLE_START_LINE), cl, &header);
        curses::attroff(A_BOLD);
        // print partitions
        for i in 0..nparts {
            self.draw_partition(i);
        }
        let (menu_start, cols) = {
            let u = self.ui.borrow();
            (u.menu_start_line(), u.cols)
        };
        if curpg != 0 {
            curses::mvaddch(
                c_int(TABLE_START_LINE),
                c_int(cols.wrapping_sub(1)),
                curses::acs(curses::ACS_UARROW),
            );
            curses::mvaddch(c_int(TABLE_START_LINE), 0, curses::acs(curses::ACS_UARROW));
        }
        if self.page_sz != 0 && curpg < self.lines.len().checked_div(self.page_sz).unwrap_or(0) {
            let ln = c_int(menu_start.wrapping_sub(1));
            curses::mvaddch(
                ln,
                c_int(cols.wrapping_sub(1)),
                curses::acs(curses::ACS_DARROW),
            );
            curses::mvaddch(ln, 0, curses::acs(curses::ACS_DARROW));
        }
        0
    }

    /// `ui_table_goto (cf, where)`.
    fn table_goto(&mut self, where_: i32) -> i32 {
        let nparts = self.table.get_nents();
        let mut where_ = where_;
        if where_ < 0 {
            where_ = 0;
        } else if c_size(where_) > nparts.wrapping_sub(1) {
            where_ = c_int(nparts.wrapping_sub(1));
        }
        if c_size(where_) == self.lines_idx {
            return 0;
        }
        let old = self.lines_idx;
        self.lines_idx = c_size(where_);
        if !self.partition_on_page(old) || !self.partition_on_page(self.lines_idx) {
            self.draw_table();
        } else {
            // cleanup old, draw new
            self.draw_partition(old);
            self.draw_partition(self.lines_idx);
        }
        self.ui.borrow().clean_info();
        self.draw_menu();
        self.draw_extra();
        curses::refresh();
        0
    }

    /// `ui_refresh (cf)`: the whole screen drawn again.
    fn refresh(&mut self) -> i32 {
        if !self.ui.borrow().enabled {
            return libcall::EINVAL.wrapping_neg();
        }
        let bytes = self
            .cxt
            .get_nsectors()
            .wrapping_mul(self.cxt.get_sector_size());
        let strsz = ulstrutils::size_to_human_string(
            ulstrutils::SIZE_DECIMAL_2DIGITS
                | ulstrutils::SIZE_SUFFIX_SPACE
                | ulstrutils::SIZE_SUFFIX_3LETTER,
            bytes,
        );
        let label_name = self.cxt.label_ref().map_or("", libfdisk::Label::get_name);
        curses::clear();
        let devname = self.cxt.get_devname().unwrap_or_default().to_vec();
        // An identifier that cannot be read is shown as none: "Label: X"
        // alone, upstream's branch for a nonzero return or a NULL id.
        let id = self.cxt.get_disklabel_id().ok().flatten();
        let nsectors = self.cxt.get_nsectors();
        {
            let u = self.ui.borrow();
            // header
            curses::attron(A_BOLD);
            let mut l0 = b"Disk: ".to_vec();
            l0.extend_from_slice(&devname);
            u.center(0, &l0);
            curses::attroff(A_BOLD);
            u.center(
                1,
                format!("Size: {strsz}, {bytes} bytes, {nsectors} sectors").as_bytes(),
            );
            match id {
                Some(id) => {
                    let mut l2 = format!("Label: {label_name}, identifier: ").into_bytes();
                    l2.extend_from_slice(&id);
                    u.center(2, &l2);
                }
                None => u.center(2, format!("Label: {label_name}").as_bytes()),
            }
        }
        self.draw_table();
        self.draw_menu();
        curses::refresh();
        0
    }

    /// `ui_get_size (cf, prompt, &res, low, up, &expsize)`: a size asked
    /// for, in bytes, between `low` and `up`; `ERR_ESC` when the
    /// dialog was left.
    fn get_size(
        &mut self,
        prompt: &[u8],
        res: &mut u64,
        low: u64,
        up: u64,
        expsize: Option<&mut bool>,
    ) -> isize {
        let mut user = 0u64;
        let dflt = ulstrutils::size_to_human_string(0, *res).into_bytes();
        self.ui.borrow().clean_info();
        let mut buf = dflt.clone();
        let mut expsize = expsize;
        let mut rc: isize;
        loop {
            let mut pwr = 0i32;
            let mut insec = false;
            rc = ui_get_string(
                &self.ui,
                Some(prompt),
                Some(b"May be followed by M for MiB, G for GiB, T for TiB, or S for sectors."),
                &mut buf,
                128,
            );
            self.ui.borrow().clean_warn();
            if rc == 0 {
                self.ui.borrow().warnx(b"Please, specify size.");
                // "nothing specified": the `continue` reaches the loop's
                // condition, which ends it.
                break;
            }
            if rc == ERR_ESC {
                // "cancel dialog"
                break;
            }
            if buf == dflt {
                // "no change, use default"
                user = *res;
                rc = 0;
            } else {
                // Off `buf` itself, as upstream's `buf[len - 1] = '\0'` takes
                // it: a size refused below is offered again without its `S`.
                if matches!(buf.last(), Some(b'S' | b's')) {
                    insec = true;
                    buf.pop();
                }
                let (r, x, p) = ulstrutils::parse_size_power(&buf);
                user = x;
                pwr = p;
                rc = match r {
                    Ok(_) => 0,
                    Err(ulstrutils::NumErr::Invalid) => neg(libcall::EINVAL),
                    Err(ulstrutils::NumErr::Range) => neg(libcall::ERANGE),
                };
            }
            if rc == 0 {
                if insec {
                    user = user.wrapping_mul(self.cxt.get_sector_size());
                }
                if user < low {
                    self.ui
                        .borrow()
                        .warnx(format!("Minimum size is {low} bytes.").as_bytes());
                    rc = neg(libcall::ERANGE);
                }
                // `1ULL << pwr * 10`, the shift taken modulo 64 as x86 takes
                // it for the suffixes past `E`.
                let span = 1u64.wrapping_shl(pwr.cast_unsigned().wrapping_mul(10));
                if user > up && pwr != 0 && user < up.wrapping_add(span) {
                    // "ignore when the user specified size overflow with in
                    // range specified by suffix (e.g. MiB)"
                    user = up;
                }
                if user > up {
                    self.ui
                        .borrow()
                        .warnx(format!("Maximum size is {up} bytes.").as_bytes());
                    rc = neg(libcall::ERANGE);
                }
                if rc == 0
                    && insec
                    && let Some(e) = expsize.as_deref_mut()
                {
                    *e = true;
                }
            } else {
                self.ui.borrow().warnx(b"Failed to parse size.");
            }
            if rc == 0 {
                break;
            }
        }
        if rc == 0 {
            *res = user;
        }
        rc
    }

    /// `ui_get_parttype (cf, cur)`: a partition type chosen from the
    /// label's, the current one `cur` first in view; `None` when the menu
    /// was left.
    fn get_parttype(&mut self, cur: Option<&PartType>) -> Option<PartType> {
        let lb = self.cxt.label_ref()?;
        // "create cfdisk menu according to label types"
        let nitems = lb.get_nparttypes();
        if nitems == 0 {
            return None;
        }
        let codetypes = lb.has_code_parttypes();
        let mut cm = Vec::with_capacity(nitems);
        // Beside the menu, in its order: an item's userdata is its type's
        // index here.
        let mut types: Vec<PartType> = Vec::with_capacity(nitems);
        let mut idx = 0usize;
        for i in 0..nitems {
            let Some(x) = lb.get_parttype(i) else {
                continue;
            };
            let name = x.get_name().unwrap_or_default();
            let (name, desc) = if codetypes {
                let mut n = format!("{:2x} ", x.get_code()).into_bytes();
                n.extend_from_slice(name);
                (n, None)
            } else {
                (name.to_vec(), x.get_string().map(<[u8]>::to_vec))
            };
            if cur.is_some_and(|c| same_type(x, c)) {
                idx = types.len();
            }
            cm.push(MenuItem {
                key: 0,
                name,
                desc,
                userdata: Some(types.len()),
            });
            types.push(x.clone());
        }
        {
            let mut u = self.ui.borrow_mut();
            // "make the new menu active"
            u.menu_push(cm);
            if let Some(m) = u.menu_mut() {
                m.vertical = true;
                m.idx = idx;
            }
            u.menu_set_title(b"Select partition type");
            u.draw_menu(None);
        }
        curses::refresh();
        let mut t = None;
        while !sig_die() {
            let key = curses::getch();
            if sig_die() {
                break;
            }
            let mut u = self.ui.borrow_mut();
            if sig_resize() {
                u.menu_resize(None);
            }
            if u.menu_move(key, None) == 0 {
                continue;
            }
            if key == KEY_ENTER || key == 0x0a || key == 0x0d {
                let i = u.menu().map_or(0, |m| m.idx);
                t = u
                    .menu_get_menuitem(i)
                    .and_then(|n| u.menu().and_then(|m| m.items.get(n)))
                    .and_then(|d| d.userdata)
                    .and_then(|i| types.get(i).cloned());
                break;
            }
            if key == KEY_ESC || key == i32::from(b'q') || key == i32::from(b'Q') {
                break;
            }
        }
        if sig_die() {
            die_on_signal(&self.ui);
        }
        self.ui.borrow_mut().menu_pop();
        t
    }

    /// `ui_script_read (cf)`: a script file applied to the table in memory;
    /// 0 when it was, `<= 0` from the dialog when no name was given.
    fn script_read(&mut self) -> isize {
        curses::erase();
        let mut buf = Vec::new();
        let rc = ui_get_string(
            &self.ui,
            Some(b"Enter script file name: "),
            Some(b"The script file will be applied to in-memory partition table."),
            &mut buf,
            PATH_MAX,
        );
        if rc <= 0 {
            return rc;
        }
        let mut rc = -1;
        match libfdisk::script::new_script_from_file(&mut self.cxt, &buf) {
            Err(errno) => {
                let mut m = b"Cannot open ".to_vec();
                m.extend_from_slice(&buf);
                self.ui.borrow().warn(&m, errno);
            }
            Ok(sc) => {
                let sc = Rc::new(RefCell::new(sc));
                if self.cxt.apply_script(&sc) != 0 {
                    let mut m = b"Failed to apply script ".to_vec();
                    m.extend_from_slice(&buf);
                    self.ui.borrow().warnx(&m);
                } else {
                    rc = 0;
                }
            }
        }
        self.ui.borrow().clean_hint();
        rc
    }

    /// `ui_script_write (cf)`: the table in memory dumped to a script file.
    fn script_write(&mut self) -> isize {
        let mut buf = Vec::new();
        let rc = ui_get_string(
            &self.ui,
            Some(b"Enter script file name: "),
            Some(b"The current in-memory partition table will be dumped to the file."),
            &mut buf,
            PATH_MAX,
        );
        if rc <= 0 {
            return rc;
        }
        let mut sc = libfdisk::script::Script::new();
        let mut rc = sc.read_context(&mut self.cxt);
        let mut errnum = 0;
        if rc != 0 {
            self.ui
                .borrow()
                .warnx(b"Failed to read disk layout into script.");
        } else {
            match std::fs::File::create(ulsysfs::path_of(&buf)) {
                Err(e) => {
                    errnum = e.raw_os_error().unwrap_or(0);
                    let mut m = b"Cannot open ".to_vec();
                    m.extend_from_slice(&buf);
                    self.ui.borrow().warn(&m, errnum);
                    rc = errnum.wrapping_neg();
                }
                Ok(mut f) => {
                    let mut out = Vec::new();
                    rc = sc.write_file(&self.cxt, &mut out);
                    if rc == 0 && std::io::Write::write_all(&mut f, &out).is_err() {
                        rc = libfdisk::errno::EIO.wrapping_neg();
                    }
                    if rc == 0 {
                        self.ui.borrow().info(b"Disk layout successfully dumped.");
                    }
                }
            }
        }
        if rc != 0 {
            let mut m = b"Failed to write script ".to_vec();
            m.extend_from_slice(&buf);
            // `%m` is what `errno` holds then.
            self.ui.borrow().warn(&m, errnum);
        }
        isize::try_from(rc).unwrap_or(-1)
    }

    /// `ui_create_label (cf)`: a label type chosen and a new label of it
    /// made, or a script loaded; 1 when the menu was left.
    fn create_label(&mut self) -> i32 {
        let mut rc = 1;
        let mut refresh_menu = true;
        // "create cfdisk menu according to libfdisk labels"
        let cm: Vec<MenuItem> = self
            .cxt
            .labels()
            .filter(|lb| !lb.is_disabled() && lb.get_type() != LabelType::Bsd)
            .map(|lb| MenuItem {
                key: 0,
                name: lb.get_name().as_bytes().to_vec(),
                desc: None,
                userdata: None,
            })
            .collect();
        curses::erase();
        {
            let mut u = self.ui.borrow_mut();
            // "make the new menu active"
            u.menu_push(cm);
            if let Some(m) = u.menu_mut() {
                m.vertical = true;
            }
            u.menu_set_title(b"Select label type");
            if !self.zero_start {
                u.info(b"Device does not contain a recognized partition table.");
            }
        }
        while !sig_die() {
            if refresh_menu {
                let mut u = self.ui.borrow_mut();
                u.draw_menu(None);
                u.hint(b"Select a type to create a new label, press 'L' to load script file, 'Q' quits.");
                curses::refresh();
                refresh_menu = false;
            }
            let key = curses::getch();
            if sig_die() {
                break;
            }
            {
                let mut u = self.ui.borrow_mut();
                if sig_resize() {
                    u.menu_resize(None);
                }
                if u.menu_move(key, None) == 0 {
                    continue;
                }
            }
            if key == KEY_ENTER || key == 0x0a || key == 0x0d {
                let name = {
                    let u = self.ui.borrow();
                    let i = u.menu().map_or(0, |m| m.idx);
                    u.menu_get_menuitem(i)
                        .and_then(|n| u.menu().and_then(|m| m.items.get(n)))
                        .map(|d| d.name.clone())
                };
                if let Some(name) = name {
                    rc = self.cxt.create_disklabel(Some(&name));
                }
                break;
            }
            if key == KEY_ESC || key == i32::from(b'q') || key == i32::from(b'Q') {
                break;
            }
            if key == i32::from(b'l') || key == i32::from(b'L') {
                let r = self.script_read();
                rc = i32::try_from(r).unwrap_or(-1);
                if r == 0 {
                    break;
                }
                refresh_menu = true;
            }
        }
        if sig_die() {
            die_on_signal(&self.ui);
        }
        self.ui.borrow_mut().menu_pop();
        rc
    }

    /// `main_menu_action (cf, key)`: what a key of the main menu does --
    /// 0 for the current item's own; 1 to quit, `< 0` for an error.
    #[allow(
        clippy::too_many_lines,
        reason = "upstream's main_menu_action, one switch, kept whole"
    )]
    fn main_menu_action(&mut self, key: i32) -> i32 {
        let org_order = self.wrong_order;
        let mut key = key;
        if key == 0 {
            let u = self.ui.borrow();
            let idx = u.menu().map_or(0, |m| m.idx);
            let Some(k) = u
                .menu_get_menuitem(idx)
                .and_then(|n| u.menu().and_then(|m| m.items.get(n)))
                .map(|d| d.key)
            else {
                return 0;
            };
            key = k;
        } else if key != i32::from(b'w') && key != i32::from(b'W') {
            // "case insensitive except 'W'rite"
            key = libcall::locale::tolower(key);
        }
        if self.ui.borrow().menu().is_some_and(|m| Ui::ignored(m, key)) {
            return 0;
        }
        let Some(pa) = self.get_current_partition().cloned() else {
            return libcall::EINVAL.wrapping_neg();
        };
        let n = pa.get_partno();
        {
            let u = self.ui.borrow();
            u.clean_hint();
            u.clean_info();
        }
        let mut refresh = false;
        let mut info: Option<String> = None;
        let mut warn: Option<String> = None;
        match key {
            k if k == i32::from(b'b') => {
                // Bootable flag
                let fl = if self.cxt.is_labeltype(LabelType::Dos) {
                    libfdisk::DOS_FLAG_ACTIVE
                } else if self.cxt.is_labeltype(LabelType::Sgi) {
                    libfdisk::SGI_FLAG_BOOT
                } else {
                    0
                };
                if fl != 0 && self.cxt.toggle_partition_flag(n, fl) != 0 {
                    warn = Some("Could not toggle the flag.".to_owned());
                } else if fl != 0 {
                    refresh = true;
                }
            }
            k if k == KEY_DC || k == i32::from(b'd') => {
                // Delete
                if self.cxt.delete_partition(n) != 0 {
                    warn = Some(format!("Could not delete partition {}.", n.wrapping_add(1)));
                } else {
                    info = Some(format!("Partition {} has been deleted.", n.wrapping_add(1)));
                }
                refresh = true;
            }
            k if k == i32::from(b'h') || k == i32::from(b'?') => {
                // Help
                self.help();
                refresh = true;
            }
            k if k == i32::from(b'n') => {
                // New
                if !pa.is_freespace() || !pa.has_start() {
                    return libcall::EINVAL.wrapping_neg();
                }
                // "free space range"
                let start = pa.get_start();
                let ss = self.cxt.get_sector_size();
                let mut size = pa.get_size().wrapping_mul(ss);
                let (max_size, dflt_size) = (size, size);
                let mut expsize = false;
                if self.get_size(
                    b"Partition size: ",
                    &mut size,
                    ss,
                    max_size,
                    Some(&mut expsize),
                ) != ERR_ESC
                {
                    let secs = size.checked_div(ss).unwrap_or(0);
                    let mut npa = Partition::new();
                    if dflt_size == size {
                        // "default is to fillin all free space"
                        npa.set_end_follow_default(true);
                    } else {
                        npa.set_size(secs);
                    }
                    // Upstream marks the free space, not the new partition,
                    // as sized explicitly: `fdisk_partition_size_explicit
                    // (pa, 1)` -- which nothing reads afterwards.
                    npa.set_start(start);
                    npa.set_partno_follow_default(true);
                    // "add to disk label -- libfdisk will ask for missing
                    // details"
                    if self.cxt.add_partition(Some(&mut npa), None) == 0 {
                        refresh = true;
                    }
                }
            }
            k if k == i32::from(b'q') => {
                // Quit
                return 1;
            }
            k if k == i32::from(b't') => {
                // Type
                if pa.is_freespace() {
                    return libcall::EINVAL.wrapping_neg();
                }
                let t = self.get_parttype(pa.get_type());
                refresh = true;
                info = Some(
                    if t.is_some_and(|t| self.cxt.set_partition_type(n, &t) == 0) {
                        format!("Changed type of partition {}.", n.wrapping_add(1))
                    } else {
                        format!("The type of partition {} is unchanged.", n.wrapping_add(1))
                    },
                );
            }
            k if k == i32::from(b'r') => {
                // resize
                if pa.is_freespace() || !pa.has_start() {
                    return libcall::EINVAL.wrapping_neg();
                }
                let mut size = pa.get_size();
                // "is the next freespace?"
                if let Some(next) = self.table.get_partition(self.lines_idx.wrapping_add(1))
                    && next.is_freespace()
                {
                    size = size.wrapping_add(next.get_size());
                }
                let ss = self.cxt.get_sector_size();
                size = size.wrapping_mul(ss);
                let max_size = size;
                if self.get_size(b"New size: ", &mut size, ss, max_size, None) != ERR_ESC {
                    let secs = size.checked_div(ss).unwrap_or(0);
                    let mut npa = Partition::new();
                    npa.set_size(secs);
                    if self.cxt.set_partition(n, &mut npa) == 0 {
                        refresh = true;
                        info = Some(format!("Partition {} resized.", n.wrapping_add(1)));
                    }
                }
            }
            k if k == i32::from(b's') => {
                // Sort
                if self.wrong_order {
                    self.cxt.reorder_partitions();
                    refresh = true;
                }
            }
            k if k == i32::from(b'u') => {
                // dUmp
                self.script_write();
            }
            k if k == i32::from(b'W') => {
                // Write
                if self.cxt.is_readonly() {
                    warn = Some("Device is open in read-only mode.".to_owned());
                } else {
                    let mut buf = Vec::new();
                    let rc = ui_get_string(
                        &self.ui,
                        Some(b"Are you sure you want to write the partition table to disk? "),
                        Some(b"Type \"yes\" or \"no\", or press ESC to leave this dialog."),
                        &mut buf,
                        64,
                    );
                    refresh = true;
                    if rc <= 0 || !buf.eq_ignore_ascii_case(b"yes") {
                        info = Some("Did not write partition table to disk.".to_owned());
                    } else {
                        if self.cxt.write_disklabel() != 0 {
                            warn = Some("Failed to write disklabel.".to_owned());
                        } else {
                            if self.device_is_used {
                                if let Some(org) = &self.original_layout {
                                    self.cxt.reread_changes(org);
                                }
                            } else {
                                self.cxt.reread_partition_table();
                            }
                            info = Some("The partition table has been altered.".to_owned());
                            // Ubuntu's 2.39.3 carries upstream's "fix menu
                            // behavior after writing changes" (2.40): once
                            // written, the next thing to do is quit.
                            let keys = self.main_keys();
                            let mut u = self.ui.borrow_mut();
                            if let Some((_, q_idx)) = u.menu_get_menuitem_by_key(i32::from(b'q')) {
                                u.menu_goto(c_int(q_idx), Some(&keys));
                            }
                        }
                        self.nwrites = self.nwrites.wrapping_add(1);
                    }
                }
            }
            _ => {}
        }
        if refresh {
            self.lines_refresh();
            self.refresh();
            self.draw_extra();
        } else {
            self.draw_menu();
        }
        let u = self.ui.borrow();
        u.clean_hint();
        if let Some(w) = warn {
            u.warnx(w.as_bytes());
        } else if let Some(i) = info {
            u.info(i.as_bytes());
        } else if key == i32::from(b'n') && self.wrong_order && !org_order {
            u.info(b"Note that partition table entries are not in disk order now.");
        }
        0
    }

    /// `ui_help ()`.
    fn help(&self) {
        const HELP: [&[u8]; 30] = [
            b"This is cfdisk, a curses-based disk partitioning program.",
            b"It lets you create, delete, and modify partitions on a block device.",
            b"  ",
            b"Command      Meaning",
            b"-------      -------",
            b"  b          Toggle bootable flag of the current partition",
            b"  d          Delete the current partition",
            b"  h          Print this screen",
            b"  n          Create new partition from free space",
            b"  q          Quit program without writing partition table",
            b"  r          Reduce or enlarge the current partition",
            b"  s          Fix partitions order (only when in disarray)",
            b"  t          Change the partition type",
            b"  u          Dump disk layout to sfdisk compatible script file",
            b"  W          Write partition table to disk (you must enter uppercase W);",
            b"               since this might destroy data on the disk, you must either",
            b"               confirm or deny the write by entering 'yes' or 'no'",
            b"  x          Display/hide extra information about a partition",
            b"Up Arrow     Move cursor to the previous partition",
            b"Down Arrow   Move cursor to the next partition",
            b"Left Arrow   Move cursor to the previous menu item",
            b"Right Arrow  Move cursor to the next menu item",
            b"  ",
            b"Note: All of the commands can be entered with either upper or lower",
            b"case letters (except for Write).",
            b"  ",
            b"Use lsblk(8) or partx(8) to see more details about the device.",
            b"  ",
            b"  ",
            b"Copyright (C) 2014-2017 Karel Zak <kzak@redhat.com>",
        ];
        curses::erase();
        for (i, line) in HELP.iter().enumerate() {
            curses::mvaddstr(c_int(i), 1, line);
        }
        self.ui.borrow().info(b"Press a key to continue.");
        curses::getch();
        if sig_die() {
            die_on_signal(&self.ui);
        }
    }

    /// `ui_resize_refresh (cf)`.
    fn resize_refresh(&mut self) {
        {
            let mut u = self.ui.borrow_mut();
            u.resize();
            u.menu_refresh_size();
        }
        self.lines_refresh();
        self.refresh();
        self.draw_extra();
    }

    /// `toggle_show_extra (cf)`.
    fn toggle_show_extra(&mut self) {
        if self.show_extra
            && let Some(w) = self.act_win
        {
            curses::wclear(w);
            curses::touchwin(STDSCR);
        }
        self.show_extra = !self.show_extra;
        if self.show_extra {
            self.draw_extra();
        }
    }

    /// `ui_run (cf)`: the program on the screen until it quits.
    fn run(&mut self) -> i32 {
        {
            let mut u = self.ui.borrow_mut();
            u.lines = usize::try_from(curses::lines()).unwrap_or(0);
            u.cols = usize::try_from(curses::cols()).unwrap_or(0);
        }
        if let Some(collision) = self.cxt.get_collision().map(<[u8]>::to_vec) {
            let mut m = b"Device already contains a ".to_vec();
            m.extend_from_slice(&collision);
            m.extend_from_slice(b" signature; it will be removed by a write command.");
            self.ui.borrow().warnx(&m);
            self.cxt.enable_wipe(true);
            self.ui.borrow().hint(b"Press a key to continue.");
            curses::getch();
        }
        if !self.cxt.has_label() || self.zero_start {
            let rc = self.create_label();
            if rc < 0 {
                self.ui
                    .borrow_mut()
                    .err(1, b"failed to create a new disklabel", rc.wrapping_neg());
            }
            if rc != 0 {
                return rc;
            }
        }
        self.cols_init();
        if self.lines_refresh() != 0 {
            self.ui.borrow_mut().errx(1, b"failed to read partitions");
        }
        {
            let mut u = self.ui.borrow_mut();
            u.menu_push(main_menuitems());
            if let Some(m) = u.menu_mut() {
                m.main = true;
            }
        }
        let rc = self.refresh();
        if rc != 0 {
            return rc;
        }
        self.show_extra = true;
        self.draw_extra();
        if self.cxt.is_readonly() {
            self.ui
                .borrow()
                .warnx(b"Device is open in read-only mode. Changes will remain in memory only.");
        } else if self.device_is_used {
            self.ui
                .borrow()
                .warnx(b"Device is currently in use, repartitioning is probably a bad idea.");
        } else if self.wrong_order {
            self.ui
                .borrow()
                .info(b"Note that partition table entries are not in disk order now.");
        }
        while !sig_die() {
            let key = curses::getch();
            if sig_die() {
                break;
            }
            if sig_resize() {
                // "Note that ncurses getch() returns ERR when interrupted by
                // signal, but SLang does not interrupt at all."
                self.resize_refresh();
            }
            if key == curses::ERR {
                continue;
            }
            if key == 0o14 {
                // ^L refresh
                self.resize_refresh();
                continue;
            }
            if self.menu_move(key) == 0 {
                continue;
            }
            let idx = c_int(self.lines_idx);
            let page = c_int(self.page_sz);
            let mut rc = 0;
            match key {
                KEY_DOWN | 0o16 | 0x6a => {
                    self.table_goto(idx.wrapping_add(1));
                }
                KEY_UP | 0o20 | 0x6b => {
                    self.table_goto(idx.wrapping_sub(1));
                }
                KEY_PPAGE | KEY_HOME => {
                    let to = if key == KEY_PPAGE && self.page_sz != 0 {
                        idx.wrapping_sub(page)
                    } else {
                        0
                    };
                    self.table_goto(to);
                }
                KEY_NPAGE | KEY_END => {
                    let to = if key == KEY_NPAGE && self.page_sz != 0 {
                        idx.wrapping_add(page)
                    } else {
                        c_int(self.lines.len()).wrapping_sub(1)
                    };
                    self.table_goto(to);
                }
                KEY_ENTER | 0x0a | 0x0d => {
                    rc = self.main_menu_action(0);
                }
                0x58 | 0x78 => {
                    // Extra
                    self.toggle_show_extra();
                }
                _ => {
                    rc = self.main_menu_action(key);
                    if rc < 0 {
                        curses::beep();
                    }
                }
            }
            if rc == 1 {
                // quit
                break;
            }
        }
        self.ui.borrow_mut().menu_pop();
        0
    }
}

/// `partition_from_scols (tb, ln)`: the partition shown on `ln` back in
/// `tb`, then those of its children, in their order.
fn partition_from_scols(
    tb: &mut Table,
    table: &smartcols::Table,
    owners: &mut Vec<(smartcols::LineId, Partition)>,
    ln: smartcols::LineId,
) {
    if let Some(at) = owners.iter().position(|(l, _)| *l == ln) {
        let (_, pa) = owners.swap_remove(at);
        tb.add_partition(pa);
    }
    for child in table.line_children(ln) {
        partition_from_scols(tb, table, owners, child);
    }
}

/// `ui_get_string (prompt, hint, buf, len)`: a line edited on the menu
/// lines, starting from what `buf` holds; its length, `ERR_ESC` for
/// escape, -1 when the input ended on a terminal.
fn ui_get_string(
    ui: &RefCell<Ui>,
    prompt: Option<&[u8]>,
    hint: Option<&[u8]>,
    buf: &mut Vec<u8>,
    len: usize,
) -> isize {
    let (ln, utf8) = {
        let u = ui.borrow();
        (c_int(u.menu_start_line()), u.utf8)
    };
    let mut cl = 1i32;
    let mut rc: isize = -1;
    curses::mv(ln, 0);
    curses::clrtoeol();
    curses::mv(ln.wrapping_add(1), 0);
    curses::clrtoeol();
    if let Some(p) = prompt {
        curses::mvaddstr(ln, cl, p);
        cl = cl.wrapping_add(c_int(mbs_safe_width(p, utf8)));
    }
    // Upstream also gives `mbs_new_edit` the cells left (`ui_cols - cl`),
    // which it keeps and never reads.
    let mut edit = Editor::new(buf, len, utf8);
    edit.goto(MBS_EDIT_END);
    match hint {
        Some(h) => ui.borrow().hint(h),
        None => ui.borrow().clean_hint(),
    }
    curses::curs_set(1);
    let mut done_ok = false;
    while !sig_die() {
        curses::mv(ln, cl);
        curses::clrtoeol();
        curses::mvaddstr(ln, cl, edit.text());
        curses::mv(ln, cl.wrapping_add(c_int(edit.cursor_cells)));
        curses::refresh();
        let (code, c) = curses::get_wch();
        if code == curses::ERR {
            if sig_die() {
                break;
            }
            if sig_resize() {
                ui.borrow_mut().resize();
                continue;
            }
            if !libcall::fd::is_terminal(0) {
                let term = Rc::clone(&ui.borrow().term);
                term.exit(2);
            }
            break;
        }
        if c == 0x0d || c == 0x0a || c == KEY_ENTER.cast_unsigned() {
            done_ok = true;
            break;
        }
        rc = 1;
        let key = c.cast_signed();
        match key {
            KEY_ESC => {
                rc = ERR_ESC;
                break;
            }
            KEY_LEFT => rc = isize::try_from(edit.goto(MBS_EDIT_LEFT)).unwrap_or(1),
            KEY_RIGHT => rc = isize::try_from(edit.goto(MBS_EDIT_RIGHT)).unwrap_or(1),
            KEY_END => rc = isize::try_from(edit.goto(MBS_EDIT_END)).unwrap_or(1),
            KEY_HOME => rc = isize::try_from(edit.goto(MBS_EDIT_HOME)).unwrap_or(1),
            KEY_UP | KEY_DOWN => {}
            KEY_DC => rc = isize::try_from(edit.delete()).unwrap_or(1),
            0x08 | KEY_DELETE | KEY_BACKSPACE => {
                rc = isize::try_from(edit.backspace()).unwrap_or(1);
            }
            _ => rc = isize::try_from(edit.insert(c)).unwrap_or(1),
        }
        if rc == 1 {
            curses::beep();
        }
    }
    if sig_die() {
        die_on_signal(ui);
    }
    if done_ok {
        // "success"
        rc = isize::try_from(edit.text().len()).unwrap_or(isize::MAX);
    }
    *buf = edit.text().to_vec();
    curses::mv(ln, 0);
    curses::clrtoeol();
    curses::curs_set(0);
    curses::refresh();
    rc
}

/// `ui_init (cf)`: the signal handlers, the screen, its colours and modes.
fn ui_init(ui: &RefCell<Ui>) {
    // Upstream does not look at whether these could be installed.
    let _ = libcall::signal::set_handler(libcall::signal::SIGINT, sig_handler_die, false);
    let _ = libcall::signal::set_handler(libcall::signal::SIGTERM, sig_handler_die, false);
    let _ = libcall::signal::set_handler(libcall::signal::SIGWINCH, sig_handler_resize, false);
    ui.borrow_mut().enabled = true;
    curses::initscr();
    if ui.borrow().colors && curses::has_colors() {
        curses::start_color();
        curses::use_default_colors();
        // "yeah, start from 1!"
        for (i, &(fg, bg)) in COLOR_PAIRS.iter().enumerate().skip(1) {
            curses::init_pair(i16::try_from(i).unwrap_or(0), fg, bg);
        }
    }
    curses::cbreak();
    curses::noecho();
    curses::nonl();
    curses::curs_set(0);
    curses::keypad(STDSCR, true);
}

/// `usage ()`.
fn usage(short: &[u8]) -> Vec<u8> {
    let mut out = b"\nUsage:\n ".to_vec();
    out.extend_from_slice(short);
    out.extend_from_slice(
        b" [options] <disk>\n\nDisplay or manipulate a disk partition table.\n\nOptions:\n",
    );
    let colors = format!(
        "                            {}",
        ulcolors::usage_colors_default()
    );
    for line in [
        &b" -L, --color[=<when>]     colorize output (auto, always or never)"[..],
        colors.as_bytes(),
        b" -z, --zero               start with zeroed partition table",
        b"     --lock[=<mode>]      use exclusive device lock (yes, no or nonblock)",
        b" -r, --read-only          forced open cfdisk in read-only mode",
        b"",
        b" -h, --help               display this help",
        b" -V, --version            display version",
        b"",
        b"For more details see cfdisk(8).",
    ] {
        out.extend_from_slice(line);
        out.push(b'\n');
    }
    out
}

/// The program, as invoked by `cfdisk`.
const CFDISK: Program = Program::new("cfdisk", 1);

/// Upstream's option string.
const SHORTS: &str = "L::hVzr";

/// `OPT_LOCK`: `CHAR_MAX + 1`.
const OPT_LOCK: i32 = 128;

/// Upstream's `longopts[]`, in its order.
const LONGS: &[(&str, Takes)] = &[
    ("color", Takes::Optional),
    ("lock", Takes::Optional),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
    ("zero", Takes::Nothing),
    ("read-only", Takes::Nothing),
];
/// Each one's `val`.
const LONG_VALS: [i32; 6] = [
    b'L' as i32,
    OPT_LOCK,
    b'h' as i32,
    b'V' as i32,
    b'z' as i32,
    b'r' as i32,
];
/// [`LONGS`] and [`LONG_VALS`] as one table.
const OPTIONS: LongVals<'static, i32> = LongVals::new(LONGS, &LONG_VALS);

/// An argument's bytes.
fn arg_bytes(s: &OsStr) -> Vec<u8> {
    os_bytes(s).into_owned()
}

/// `main ()`: the exit status.
#[allow(
    clippy::too_many_lines,
    reason = "upstream's main, kept in one piece so it can be read against it"
)]
fn run(argv: &[OsString], term: &Rc<Term>) -> u8 {
    let arg0 = argv
        .first()
        .map_or(OsStr::new("cfdisk"), OsString::as_os_str);
    let short = term.short.clone();
    let mut colormode = ulcolors::ColorMode::Undef;
    let mut lockmode: Option<Vec<u8>> = None;
    let mut read_only = false;
    let mut zero_start = false;
    let mut operands: Vec<Vec<u8>> = Vec::new();

    let mut buf = [0u8; 256];
    // Upstream does not look at whether the locale could be set.
    let _ = libcall::locale::select_from_env(libcall::locale::LC_ALL, &mut buf);

    let own = argv.get(1..).unwrap_or_default();
    for item in CFDISK.parse(own, SHORTS, LONGS) {
        let opt = match item {
            Ok(opt) => opt,
            Err(e) => {
                // glibc names the program by argv[0] as given.
                stderr_write(
                    format!("{}: {}\n", escape_unprintable(&os_bytes(arg0)), e.sentence).as_bytes(),
                );
                stderr_write(
                    format!(
                        "Try '{} --help' for more information.\n",
                        escape_unprintable(&short)
                    )
                    .as_bytes(),
                );
                return 1;
            }
        };
        let Some((c, value)) = OPTIONS.code::<i32>(&opt) else {
            if let Opt::Operand(w) = opt {
                operands.push(arg_bytes(w));
            }
            continue;
        };
        let argb = value.as_deref().map(arg_bytes).unwrap_or_default();
        let ch = |b: u8| i32::from(b);
        match c {
            c if c == ch(b'h') => {
                term.write(&usage(&short));
                return 0;
            }
            c if c == ch(b'L') => {
                colormode = ulcolors::ColorMode::Auto;
                if value.is_some() {
                    match ulcolors::colormode_or_err(&argb) {
                        Ok(m) => colormode = m,
                        Err(bad) => {
                            let mut m = short.clone();
                            m.extend_from_slice(b": unsupported color mode: ");
                            m.extend_from_slice(quoting::escaped_in_quotes(&bad).as_bytes());
                            m.push(b'\n');
                            stderr_write(&m);
                            return 1;
                        }
                    }
                }
            }
            c if c == ch(b'r') => read_only = true,
            c if c == ch(b'V') => {
                term.write(&short);
                term.write(format!(" from {PACKAGE_STRING}\n").as_bytes());
                return 0;
            }
            c if c == ch(b'z') => zero_start = true,
            OPT_LOCK => {
                lockmode = Some(match value {
                    Some(_) => argb.strip_prefix(b"=").unwrap_or(&argb).to_vec(),
                    None => b"1".to_vec(),
                });
            }
            _ => {
                stderr_write(
                    format!(
                        "Try '{} --help' for more information.\n",
                        escape_unprintable(&short)
                    )
                    .as_bytes(),
                );
                return 1;
            }
        }
    }

    let colors = ulcolors::Colors::init(colormode, b"cfdisk", smartcols::tty::stdout_is_tty());
    let ui = Rc::new(RefCell::new(Ui {
        enabled: false,
        lines: 0,
        cols: 0,
        menus: Vec::new(),
        colors: colors.wanted(),
        utf8: smartcols::tty::codeset_is_utf8(),
        term: Rc::clone(term),
    }));
    *term.colors.borrow_mut() = Some(colors);

    let mut cf = Cfdisk {
        cxt: Context::new(),
        table: Table::new(),
        original_layout: None,
        ui: Rc::clone(&ui),
        fields: Vec::new(),
        lines: Vec::new(),
        lines_idx: 0,
        page_sz: 0,
        nwrites: 0,
        act_win: None,
        mounts: Mounts::default(),
        wrong_order: false,
        zero_start,
        device_is_used: false,
        show_extra: false,
    };
    let cb_ui = Rc::clone(&ui);
    cf.cxt
        .set_ask(Some(Rc::new(move |_: &mut Context, ask: &mut Ask| {
            ask_callback(&cb_ui, ask)
        })));

    let diskpath = match operands.first() {
        Some(p) => p.clone(),
        None => DEFAULT_DISKS
            .iter()
            .find(|d| ulsysfs::path_of(d).exists())
            .copied()
            .unwrap_or(b"/dev/sda")
            .to_vec(),
    };

    let mut rc = cf.cxt.assign_device(&diskpath, read_only);
    if rc == libcall::EACCES.wrapping_neg() && !read_only {
        rc = cf.cxt.assign_device(&diskpath, true);
    }
    if rc != 0 {
        let mut m = short.clone();
        m.extend_from_slice(b": cannot open ");
        m.extend_from_slice(&diskpath);
        m.extend_from_slice(b": ");
        m.extend_from_slice(
            errmsg::strerror(&std::io::Error::from_raw_os_error(rc.wrapping_neg())).as_bytes(),
        );
        m.push(b'\n');
        stderr_write(&m);
        return 1;
    }

    if !cf.cxt.is_readonly() {
        if let Some(dev) = cf.cxt.get_dev().map(Rc::clone)
            && ulblkid::blkdev::lock(&dev, &diskpath, lockmode.as_deref(), &short) != 0
        {
            return 1;
        }
        cf.device_is_used = cf.cxt.device_is_used();
        let mut org = Table::new();
        // Upstream does not look at the result: a table it could not read
        // is an empty one.
        let _ = cf.cxt.get_partitions(&mut org);
        cf.original_layout = Some(org);
    }

    // "Don't use err(), warn() from this point"
    ui_init(&ui);
    cf.run();
    ui.borrow_mut().end();

    cf.free_lines();
    let rc = cf.cxt.deassign_device(cf.nwrites == 0);
    u8::from(rc != 0)
}

fn main() {
    // Before anything touches standard I/O: a descriptor the process was
    // started without stays closed, as upstream would find it.
    stdfdguard::restore();
    let argv: Vec<OsString> = std::env::args_os().collect();
    let arg0 = os_bytes(
        argv.first()
            .map_or(OsStr::new("cfdisk"), OsString::as_os_str),
    );
    // `program_invocation_short_name`: what follows the last slash.
    let short = arg0
        .rsplit(|&b| b == b'/')
        .next()
        .unwrap_or_default()
        .to_vec();
    let term = Rc::new(Term::new(short));
    let status = run(&argv, &term);
    term.exit(status)
}
