//! column -- columnate lists, or lay input out as a table.
//!
//! A port of util-linux 2.39.3's `text-utils/column.c`, function by function
//! and with upstream's names, handing its tables to `smartcols` (the
//! libsmartcols port) as upstream hands them to libsmartcols; measured
//! against `column from util-linux 2.39.3` by `scripts/column-diff.sh`.
//!
//! This replaces a hand-written program that laid its tables out itself,
//! decoded its input lossily, and had its own reading of the options.
//!
//! # What is upstream's, and easy to get wrong
//!
//! * **A line is what `getline` read, as a C string**: cut at its first NUL;
//!   blank if nothing but C-locale white space is in it; and otherwise kept
//!   whole, leading and trailing blanks included -- which the default
//!   separators then skip in `-t`, but `-s` does not, and a list keeps.
//! * **A line that does not decode** has each broken byte written `\xNN`,
//!   and in it, and only in it, a literal `\x` is written `\x5cx`, so the
//!   escape cannot be forged. In the C locale every byte above 0x7f is
//!   broken. The list modes are glibc wide-stream output (`fputws`), whose
//!   buffer counts characters (`ulclosestream::Stdout::orient_wide`).
//! * **The default separators are greedy** (`wcstok`: runs of blanks are
//!   one); `-s` makes every separator split, empty fields and all.
//! * **The exit status**: a file that cannot be opened is warned about and
//!   counted, and with no input left to print the count is the status --
//!   two unreadable files exit 2. A printed table's status replaces it
//!   (`eval = scols_print_table(...)`), so `column -t missing present`
//!   exits 0.
//! * **`--table-column width=` and `errno`.** libsmartcols refuses `width=`
//!   whenever `errno` is set when it is read, and nothing clears it first;
//!   the port therefore carries upstream's `errno` in [`Ctl::errno`]
//!   through every call that changes it: loading a locale other than C and
//!   POSIX (measured: glibc 2.39 leaves it set), `-c` and `-l` clearing it
//!   as `ul_strtou64` does, the terminal probes' `ENOTTY` and their
//!   `COLUMNS`/`LINES` reads clearing it again, a file that will not open,
//!   and a first line that will not decode. What it does not model --
//!   glibc's `isatty` on a standard input that is a character device but
//!   no terminal, and the debug variable `LIBSMARTCOLS_DEBUG` -- no
//!   harness case reaches.
//!
//! # What is not upstream's
//!
//! * **A name in a diagnostic** has its unprintable bytes escaped
//!   (design-decisions §370).
//! * **`--table-order` naming more columns than there are**: upstream
//!   writes past the array it collects them in; here each is moved in turn.
//! * **A range reaching `INT_MAX`** (`-R 1-2147483647`): upstream's `int`
//!   loop overflows and, compiled as it is, never ends; here it stops once
//!   no column can be left to find.

stdfdguard::guard_std_fds!();

use getoptlong::{Opt, Program, Takes};
use quoting::{escape_unprintable, os_bytes};
use smartcols::{
    ColumnId, FL_HIDDEN, FL_NOEXTREMES, FL_RIGHT, FL_TREE, FL_TRUNC, FL_WRAP, LineId, Table,
    TermForce,
};
use std::ffi::{OsStr, OsString};
use std::io::{BufRead, BufReader, Read};
use std::process::ExitCode;
use ulclosestream::{Stdout, stderr_write, warn, warnx};
use ulstrutils::{
    NumErr, c_isspace, err_exclusive_options, isdigit_string, num_error_message, parse_range,
    ul_strtou64,
};

/// `TABCHAR_CELLS`: the tab stops the list modes align to.
const TABCHAR_CELLS: usize = 8;

/// `ENOENT`, for a failed open with no number of its own, and `EILSEQ`,
/// what upstream's `errno` holds after a line did not decode. Only whether
/// `errno` is zero is ever read.
const ENOENT: i32 = 2;
const EILSEQ: i32 = 84;

/// Getopt's errors are only sentences here; the referral follows them.
const COLUMN: Program = Program::new("column", 1);

/// Upstream's option string: GNU order, operands anywhere.
const SHORTS: &str = "C:c:dE:eH:hi:Jl:LN:n:mO:o:p:R:r:s:T:tVW:x";

/// Upstream's `longopts[]`, in its order (the order an ambiguity lists),
/// with the deprecated `columns` and `table-empty-lines` where it has them.
const LONGS: &[(&str, Takes)] = &[
    ("columns", Takes::Required),
    ("fillrows", Takes::Nothing),
    ("help", Takes::Nothing),
    ("json", Takes::Nothing),
    ("keep-empty-lines", Takes::Nothing),
    ("output-separator", Takes::Required),
    ("output-width", Takes::Required),
    ("separator", Takes::Required),
    ("table", Takes::Nothing),
    ("table-columns", Takes::Required),
    ("table-column", Takes::Required),
    ("table-columns-limit", Takes::Required),
    ("table-hide", Takes::Required),
    ("table-name", Takes::Required),
    ("table-maxout", Takes::Nothing),
    ("table-noextreme", Takes::Required),
    ("table-noheadings", Takes::Nothing),
    ("table-order", Takes::Required),
    ("table-right", Takes::Required),
    ("table-truncate", Takes::Required),
    ("table-wrap", Takes::Required),
    ("table-empty-lines", Takes::Nothing),
    ("table-header-repeat", Takes::Nothing),
    ("tree", Takes::Required),
    ("tree-id", Takes::Required),
    ("tree-parent", Takes::Required),
    ("version", Takes::Nothing),
];
/// Each long option's `val`, in [`LONGS`]' order.
const LONG_VALS: [u8; 27] = [
    b'c', b'x', b'h', b'J', b'L', b'o', b'c', b's', b't', b'N', b'C', b'l', b'H', b'n', b'm', b'E',
    b'd', b'O', b'R', b'T', b'W', b'L', b'e', b'r', b'i', b'p', b'V',
];

/// `excl[]`: rows and members in ASCII order, as `err_exclusive_options`
/// requires.
const EXCL: [&[i32]; 3] = [
    &[b'C' as i32, b'N' as i32],
    &[b'J' as i32, b'x' as i32],
    &[b't' as i32, b'x' as i32],
];

/// `COLUMN_MODE_*`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    FillCols,
    FillRows,
    Table,
    Simple,
}

/// `struct column_control`.
struct Ctl {
    mode: Mode,
    /// `termwidth`: `None` until known, `Some(0)` unlimited.
    termwidth: Option<usize>,
    tab: Option<Table>,
    /// `--table-columns`, split; `None` when not given.
    tab_colnames: Option<Vec<Vec<u8>>>,
    tab_name: Option<Vec<u8>>,
    tab_order: Option<Vec<u8>>,
    /// Every `--table-column`, in order; empty when none was given.
    tab_columns: Vec<Vec<u8>>,
    tab_colright: Option<Vec<u8>>,
    tab_coltrunc: Option<Vec<u8>>,
    tab_colnoextrem: Option<Vec<u8>>,
    tab_colwrap: Option<Vec<u8>>,
    tab_colhide: Option<Vec<u8>>,
    tree: Option<Vec<u8>>,
    tree_id: Option<Vec<u8>>,
    tree_parent: Option<Vec<u8>>,
    /// `input_separator`, as the wide characters `mbstowcs` made of it.
    input_separator: Vec<char>,
    output_separator: Vec<u8>,
    /// `ents`: the list modes' entries, as the wide strings upstream holds.
    ents: Vec<Vec<char>>,
    /// The widest entry, in cells.
    maxlength: usize,
    /// `--table-columns-limit`, 0 for none.
    maxncols: usize,
    greedy: bool,
    json: bool,
    header_repeat: bool,
    hide_unnamed: bool,
    maxout: bool,
    keep_empty_lines: bool,
    tab_noheadings: bool,
    /// Whether the locale is UTF-8; otherwise it is C, and ASCII.
    utf8: bool,
    /// Upstream's `errno`, as far as `--table-column width=` reads it.
    errno: i32,
}

fn main() -> ExitCode {
    // Before anything touches standard I/O: a descriptor the process was
    // started without stays closed, as upstream would find it.
    stdfdguard::restore();
    let argv: Vec<OsString> = std::env::args_os().collect();
    let short = short_name(
        argv.first()
            .map_or(OsStr::new("column"), OsString::as_os_str),
    );
    let mut out = Stdout::new(1);
    let status = run(&argv, &short, &mut out);
    // `close_stdout`, which upstream registers with `atexit`.
    ExitCode::from(out.close(status, &short))
}

/// `program_invocation_short_name`: argv[0] past its last `/`.
fn short_name(arg0: &OsStr) -> Vec<u8> {
    let bytes = os_bytes(arg0);
    let start = bytes
        .iter()
        .rposition(|&b| b == b'/')
        .map_or(0, |i| i.saturating_add(1));
    bytes.get(start..).unwrap_or_default().to_vec()
}

/// Bytes shown in a diagnostic: upstream's text, unprintable bytes escaped.
fn shown(text: &[u8]) -> String {
    escape_unprintable(text)
}

/// `errtryhelp(EXIT_FAILURE)`.
fn errtryhelp(short: &[u8]) -> u8 {
    stderr_write(format!("Try '{} --help' for more information.\n", shown(short)).as_bytes());
    1
}

/// `usage()`.
fn usage(short: &[u8]) -> Vec<u8> {
    let mut text = b"\nUsage:\n ".to_vec();
    text.extend_from_slice(short);
    text.extend_from_slice(
        b" [options] [<file>...]\n\
\nColumnate lists.\n\
\nOptions:\n\
\x20-t, --table                      create a table\n\
\x20-n, --table-name <name>          table name for JSON output\n\
\x20-O, --table-order <columns>      specify order of output columns\n\
\x20-C, --table-column <properties>  define column\n\
\x20-N, --table-columns <names>      comma separated columns names\n\
\x20-l, --table-columns-limit <num>  maximal number of input columns\n\
\x20-E, --table-noextreme <columns>  don't count long text from the columns to column width\n\
\x20-d, --table-noheadings           don't print header\n\
\x20-m, --table-maxout               fill all available space\n\
\x20-e, --table-header-repeat        repeat header for each page\n\
\x20-H, --table-hide <columns>       don't print the columns\n\
\x20-R, --table-right <columns>      right align text in these columns\n\
\x20-T, --table-truncate <columns>   truncate text in the columns when necessary\n\
\x20-W, --table-wrap <columns>       wrap text in the columns when necessary\n\
\x20-L, --keep-empty-lines           don't ignore empty lines\n\
\x20-J, --json                       use JSON output format for table\n\
\n\
\x20-r, --tree <column>              column to use tree-like output for the table\n\
\x20-i, --tree-id <column>           line ID to specify child-parent relation\n\
\x20-p, --tree-parent <column>       parent to specify child-parent relation\n\
\n\
\x20-c, --output-width <width>       width of output in number of characters\n\
\x20-o, --output-separator <string>  columns separator for table output (default is two spaces)\n\
\x20-s, --separator <string>         possible table delimiters\n\
\x20-x, --fillrows                   fill rows before columns\n\
\n",
    );
    text.extend_from_slice(format!("{:<34}{}\n", " -h, --help", "display this help").as_bytes());
    text.extend_from_slice(format!("{:<34}{}\n", " -V, --version", "display version").as_bytes());
    text.extend_from_slice(b"\nFor more details see column(1).\n");
    text
}

/// `mbs_to_wcs(s)`: the wide characters of `s`, or `None` when it does not
/// decode -- in a UTF-8 locale, invalid UTF-8; in the C locale, anything
/// but ASCII.
fn mbs_to_wcs(s: &[u8], utf8: bool) -> Option<Vec<char>> {
    if !utf8 && !s.is_ascii() {
        return None;
    }
    std::str::from_utf8(s).ok().map(|t| t.chars().collect())
}

/// `wcs_to_mbs(wcs)`: back to bytes.
fn wcs_to_mbs(wcs: &[char]) -> Vec<u8> {
    wcs.iter().collect::<String>().into_bytes()
}

/// `mbs_invalid_encode(s)`: each byte that begins no valid character
/// written `\xNN`, and the backslash of a literal `\x` too. Each broken byte
/// is judged alone, as `mbrtowc` judges it with the string's NUL after it:
/// an incomplete sequence at the end is broken, byte by byte.
fn mbs_invalid_encode(s: &[u8], utf8: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len().saturating_mul(4));
    let mut i = 0usize;
    while let Some(&b) = s.get(i) {
        let rest = s.get(i..).unwrap_or_default();
        let len = if utf8 {
            match quoting::next_mb(rest) {
                Some(quoting::Mb::Char(_, n)) => Some(n),
                _ => None,
            }
        } else {
            b.is_ascii().then_some(1)
        };
        match len {
            Some(_) if b == b'\\' && rest.get(1) == Some(&b'x') => {
                out.extend_from_slice(b"\\x5c");
                i = i.saturating_add(1);
            }
            Some(n) => {
                out.extend_from_slice(rest.get(..n).unwrap_or(rest));
                i = i.saturating_add(n);
            }
            None => {
                out.extend_from_slice(format!("\\x{b:02x}").as_bytes());
                i = i.saturating_add(1);
            }
        }
    }
    out
}

/// `width(wcs)`: the cells the characters take, those `wcwidth` calls
/// unprintable counting nothing.
fn width(wcs: &[char]) -> usize {
    wcs.iter()
        .map(|&c| charwidth::char_width(c).unwrap_or(0))
        .fold(0usize, usize::saturating_add)
}

/// Where a line's tokenizing is: `wcstok`'s or `local_wcstok`'s saved
/// pointer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tok {
    /// Nothing read yet.
    Start,
    /// The next token starts looking here.
    At(usize),
    /// `local_wcstok` found the last token, or `wcstok` the end.
    Done,
}

/// `local_wcstok(ctl, p, &state)`: the next field of `line`, as a range.
///
/// Greedy -- the default separators -- is `wcstok`: separators before a
/// token are skipped, so a run of them is one, and none is at either end.
/// Otherwise every separator ends a field, empty ones included.
fn local_wcstok(
    greedy: bool,
    seps: &[char],
    line: &[char],
    state: &mut Tok,
) -> Option<(usize, usize)> {
    let is_sep = |i: usize| line.get(i).is_some_and(|c| seps.contains(c));
    let len = line.len();
    let mut i = match *state {
        Tok::Start => 0,
        Tok::At(i) => i,
        Tok::Done => return None,
    };
    if greedy {
        while i < len && is_sep(i) {
            i = i.saturating_add(1);
        }
        if i >= len {
            *state = Tok::Done;
            return None;
        }
        let start = i;
        while i < len && !is_sep(i) {
            i = i.saturating_add(1);
        }
        *state = Tok::At(if i < len { i.saturating_add(1) } else { len });
        return Some((start, i));
    }
    match (i..len).find(|&j| is_sep(j)) {
        Some(j) => {
            *state = Tok::At(j.saturating_add(1));
            Some((i, j))
        }
        None => {
            *state = Tok::Done;
            Some((i, len))
        }
    }
}

/// `strv_split(str, ",")`: the words between commas, empty ones dropped.
fn strv_split(s: &[u8]) -> Vec<Vec<u8>> {
    s.split(|&b| b == b',')
        .filter(|w| !w.is_empty())
        .map(<[u8]>::to_vec)
        .collect()
}

/// `strtou32_or_err(str, errmesg)`: the refusal is the caller's to report.
fn strtou32(s: &[u8]) -> Result<u32, NumErr> {
    let v = ul_strtou64(s, 10)?;
    u32::try_from(v).map_err(|_| NumErr::Range)
}

/// `strtou32_or_err`'s refusal: `errmesg: 'arg'`, and the reason for a
/// number out of range.
fn strtou32_or_err(s: &[u8], errmesg: &str, short: &[u8]) -> Result<u32, u8> {
    strtou32(s).map_err(|e| {
        warnx(
            short,
            &num_error_message(errmesg, &quoting::os_from_bytes(s), e),
        );
        1
    })
}

impl Ctl {
    fn new(utf8: bool) -> Self {
        Ctl {
            mode: Mode::FillCols,
            termwidth: None,
            tab: None,
            tab_colnames: None,
            tab_name: None,
            tab_order: None,
            tab_columns: Vec::new(),
            tab_colright: None,
            tab_coltrunc: None,
            tab_colnoextrem: None,
            tab_colwrap: None,
            tab_colhide: None,
            tree: None,
            tree_id: None,
            tree_parent: None,
            input_separator: vec!['\t', ' '],
            output_separator: b"  ".to_vec(),
            ents: Vec::new(),
            maxlength: 0,
            maxncols: 0,
            greedy: true,
            json: false,
            header_repeat: false,
            hide_unnamed: false,
            maxout: false,
            keep_empty_lines: false,
            tab_noheadings: false,
            utf8,
            errno: 0,
        }
    }

    /// `init_table`.
    fn init_table(&mut self) {
        let mut tab = Table::new();
        // `scols_new_table`'s terminal probe.
        self.errno = smartcols::tty::dimension_errno(self.errno, true, true);
        tab.set_utf8(self.utf8);
        tab.set_column_separator(&self.output_separator);
        if self.json {
            tab.enable_json(true);
            tab.set_name(self.tab_name.as_deref().unwrap_or(b"table"));
        } else {
            tab.enable_noencoding(true);
        }
        // Refused only with minout on, which nothing here sets; upstream
        // does not look either.
        let _ = tab.enable_maxout(self.maxout);
        if !self.tab_columns.is_empty() {
            for opts in &self.tab_columns {
                let cl = tab.new_unnamed_column(0.0, 0);
                // Upstream does not look at the result: a refused `width=`
                // leaves the column as far as it got.
                let _ = tab.column_set_properties(cl, opts, &mut self.errno);
            }
        } else if let Some(names) = &self.tab_colnames {
            for name in names {
                tab.new_column(name, 0.0, 0);
            }
        } else {
            tab.enable_noheadings(true);
        }
        if self.tab_colnames.is_some() || !self.tab_columns.is_empty() {
            if self.header_repeat {
                tab.enable_header_repeat(true);
            }
            tab.enable_noheadings(self.tab_noheadings);
        }
        self.tab = Some(tab);
    }

    /// `add_line_to_table`: the line's fields as a new line of cells, a
    /// column made for each field that has none.
    fn add_line_to_table(&mut self, wcs: &[char], short: &[u8]) -> Result<(), u8> {
        if self.tab.is_none() {
            self.init_table();
        }
        let (greedy, maxncols, hide_unnamed) = (self.greedy, self.maxncols, self.hide_unnamed);
        let seps = self.input_separator.clone();
        let Some(tab) = self.tab.as_mut() else {
            return Err(1);
        };
        let mut state = Tok::Start;
        let mut n = 0usize;
        let mut ln: Option<LineId> = None;
        while let Some((start, end)) = local_wcstok(greedy, &seps, wcs, &mut state) {
            // At the limit, the rest of the line is the last field.
            let data = if maxncols != 0 && n.saturating_add(1) == maxncols {
                wcs.get(start..).unwrap_or_default()
            } else {
                wcs.get(start..end).unwrap_or_default()
            };
            if tab.ncols() < n.saturating_add(1) {
                if tab.is_json() && !hide_unnamed {
                    warnx(
                        short,
                        &format!(
                            "line {}: for JSON the name of the column {} is required",
                            tab.nlines().saturating_add(1),
                            n.saturating_add(1)
                        ),
                    );
                    return Err(1);
                }
                tab.new_unnamed_column(0.0, if hide_unnamed { FL_HIDDEN } else { 0 });
            }
            let line = match ln {
                Some(l) => l,
                None => {
                    let l = tab.new_line(None).map_err(|_| 1u8)?;
                    ln = Some(l);
                    l
                }
            };
            if tab.line_refer_data(line, n, &wcs_to_mbs(data)).is_err() {
                warnx(short, "failed to add output data");
                return Err(1);
            }
            n = n.saturating_add(1);
            if maxncols != 0 && n == maxncols {
                break;
            }
        }
        Ok(())
    }

    /// `add_emptyline_to_table`.
    fn add_emptyline_to_table(&mut self) -> Result<(), u8> {
        if self.tab.is_none() {
            self.init_table();
        }
        let tab = self.tab.as_mut().ok_or(1u8)?;
        tab.new_line(None).map_err(|_| 1u8)?;
        Ok(())
    }

    /// One line of `read_input`, as `getline` read it.
    fn read_line(&mut self, raw: &[u8], short: &[u8]) -> Result<(), u8> {
        // The C string: to the first NUL, and without the newline.
        let line = smartcols::mbs::c_str(raw);
        let line = line.strip_suffix(b"\n").unwrap_or(line);
        if line.iter().all(|&b| c_isspace(b)) {
            if self.keep_empty_lines {
                if self.mode == Mode::Table {
                    self.add_emptyline_to_table()?;
                } else {
                    self.ents.push(Vec::new());
                }
            }
            return Ok(());
        }
        let wcs = match mbs_to_wcs(line, self.utf8) {
            Some(w) => w,
            None => {
                self.errno = EILSEQ;
                // What the encoding leaves decodes, by construction.
                let encoded = mbs_invalid_encode(line, self.utf8);
                mbs_to_wcs(&encoded, self.utf8).unwrap_or_default()
            }
        };
        match self.mode {
            Mode::Table => self.add_line_to_table(&wcs, short)?,
            Mode::FillCols | Mode::FillRows => {
                let len = width(&wcs);
                self.ents.push(wcs);
                if self.maxlength < len {
                    self.maxlength = len;
                }
            }
            Mode::Simple => {}
        }
        Ok(())
    }

    /// `read_input(ctl, fp)`: every line, each dealt with before the next
    /// is read, so a table's refusal comes before a later read error.
    fn read_input(&mut self, input: impl Read, short: &[u8]) -> Result<(), u8> {
        let mut reader = BufReader::new(input);
        let mut raw = Vec::new();
        loop {
            raw.clear();
            match reader.read_until(b'\n', &mut raw) {
                Ok(0) => return Ok(()),
                Ok(_) => self.read_line(&raw, short)?,
                Err(e) => {
                    warn(short, "read failed", &e);
                    return Err(1);
                }
            }
        }
    }
}

/// `column_set_flag(cl, fl)`.
fn column_set_flag(tab: &mut Table, cl: ColumnId, fl: u32) {
    let cur = tab.column_flags(cl).unwrap_or(0);
    // The column is the table's own.
    let _ = tab.column_set_flags(cl, cur | fl);
}

/// `get_last_visible_column(ctl, n)`: the `n`th visible column from the
/// end.
fn get_last_visible_column(tab: &Table, n: usize) -> Option<ColumnId> {
    tab.column_ids()
        .into_iter()
        .rev()
        .filter(|&cl| tab.column_flags(cl).is_some_and(|f| f & FL_HIDDEN == 0))
        .nth(n)
}

/// The number of visible columns.
fn visible_columns(tab: &Table) -> usize {
    tab.column_ids()
        .into_iter()
        .filter(|&cl| tab.column_flags(cl).is_some_and(|f| f & FL_HIDDEN == 0))
        .count()
}

/// `string_to_column(ctl, str)`: a column by number (from 1), `-1` for the
/// last visible, or name. None is `undefined column name`.
fn string_to_column(tab: &Table, s: &[u8], short: &[u8]) -> Result<ColumnId, u8> {
    let cl = if isdigit_string(s) {
        let n = strtou32_or_err(s, "failed to parse column", short)?;
        // `uint32_t` arithmetic: column 0 is number 4294967295.
        usize::try_from(n.wrapping_sub(1))
            .ok()
            .and_then(|n| tab.column(n))
    } else if s == b"-1" {
        get_last_visible_column(tab, 0)
    } else {
        tab.column_by_name(s)
    };
    cl.ok_or_else(|| {
        warnx(short, &format!("undefined column name '{}'", shown(s)));
        1
    })
}

/// `has_unnamed(list)`: whether `-` is one of the list's words.
fn has_unnamed(list: &[u8]) -> bool {
    if list == b"-" {
        return true;
    }
    if !list.contains(&b',') {
        return false;
    }
    strv_split(list).iter().any(|w| w == b"-")
}

/// `apply_columnflag_from_list(ctl, list, flag, errmsg)`: `0` is every
/// column; otherwise each word is `-` (the unnamed columns), a range `N-M`
/// (negative numbers counting visible columns from the end), or one column.
fn apply_columnflag_from_list(
    tab: &mut Table,
    list: &[u8],
    flag: u32,
    short: &[u8],
) -> Result<(), u8> {
    if list == b"0" {
        for cl in tab.column_ids() {
            column_set_flag(tab, cl, flag);
        }
        return Ok(());
    }
    let mut unnamed = false;
    for one in strv_split(list) {
        if one == b"-" {
            unnamed = true;
            continue;
        }
        if one.contains(&b'-')
            && let Ok((low, up)) = parse_range(&one, 0)
        {
            let (mut low, up) = (i64::from(low), i64::from(up));
            while low <= up {
                if low < 0 {
                    let visible = i64::try_from(visible_columns(tab)).unwrap_or(i64::MAX);
                    // `-low - 1` past the visible columns finds none, and
                    // flags only ever hide more: skip to where one can be.
                    if low.saturating_neg().saturating_sub(1) >= visible {
                        low = visible.saturating_neg().max(low.saturating_add(1));
                        continue;
                    }
                    let n = usize::try_from(low.saturating_neg().saturating_sub(1)).unwrap_or(0);
                    if let Some(cl) = get_last_visible_column(tab, n) {
                        column_set_flag(tab, cl, flag);
                    }
                } else {
                    // `scols_table_get_column(tab, low - 1)`: 0 finds none.
                    let n = usize::try_from(low.saturating_sub(1)).ok();
                    if n.is_some_and(|n| n >= tab.ncols()) {
                        break;
                    }
                    if let Some(cl) = n.and_then(|n| tab.column(n)) {
                        column_set_flag(tab, cl, flag);
                    }
                }
                low = low.saturating_add(1);
            }
            continue;
        }
        let cl = string_to_column(tab, &one, short)?;
        column_set_flag(tab, cl, flag);
    }
    if unnamed {
        for cl in tab.column_ids() {
            if tab.column_name(cl).is_none() {
                column_set_flag(tab, cl, flag);
            }
        }
    }
    Ok(())
}

/// `reorder_table`: each column named, in turn, after the one before it.
fn reorder_table(tab: &mut Table, order: &[u8], short: &[u8]) -> Result<(), u8> {
    let mut wanted = Vec::new();
    for one in strv_split(order) {
        wanted.push(string_to_column(tab, &one, short)?);
    }
    let mut last: Option<ColumnId> = None;
    for cl in wanted {
        // Both are the table's own.
        let _ = tab.move_column(last, cl);
        last = Some(cl);
    }
    Ok(())
}

/// `create_tree`: each line made a child of the (last) line whose ID is
/// its parent, unless that would close a loop.
fn create_tree(
    tab: &mut Table,
    ctl_tree: &[u8],
    parent: &[u8],
    id: &[u8],
    short: &[u8],
) -> Result<(), u8> {
    let cl_tree = string_to_column(tab, ctl_tree, short)?;
    let cl_p = string_to_column(tab, parent, short)?;
    let cl_i = string_to_column(tab, id, short)?;
    column_set_flag(tab, cl_tree, FL_TREE);
    let lines: Vec<LineId> = tab.line_ids().collect();
    for &ln_i in &lines {
        let Some(id) = tab.line_column_data(ln_i, cl_i).map(<[u8]>::to_vec) else {
            continue;
        };
        for &ln in &lines {
            if tab.line_column_data(ln, cl_p) != Some(id.as_slice()) {
                continue;
            }
            if tab.line_is_ancestor(ln, ln_i) {
                continue;
            }
            // Both lines are the table's own.
            let _ = tab.line_add_child(ln_i, ln);
        }
    }
    Ok(())
}

impl Ctl {
    /// `modify_table`: the terminal, the columns' flags, the tree, and --
    /// last -- the order.
    fn modify_table(&mut self, short: &[u8]) -> Result<(), u8> {
        let Some(tab) = self.tab.as_mut() else {
            return Ok(());
        };
        if let Some(w) = self.termwidth
            && w > 0
        {
            tab.set_termwidth(w);
            tab.set_termforce(TermForce::Always);
        }
        for (list, flag) in [
            (&self.tab_colhide, FL_HIDDEN),
            (&self.tab_colright, FL_RIGHT),
            (&self.tab_coltrunc, FL_TRUNC),
            (&self.tab_colnoextrem, FL_NOEXTREMES),
            (&self.tab_colwrap, FL_WRAP),
        ] {
            if let Some(list) = list {
                apply_columnflag_from_list(tab, list, flag, short)?;
            }
        }
        if self.tab_colnoextrem.is_none()
            && let Some(cl) = get_last_visible_column(tab, 0)
        {
            column_set_flag(tab, cl, FL_NOEXTREMES);
        }
        if let (Some(tree), Some(parent), Some(id)) = (&self.tree, &self.tree_parent, &self.tree_id)
        {
            create_tree(tab, tree, parent, id, short)?;
        }
        if let Some(order) = &self.tab_order {
            reorder_table(tab, order, short)?;
        }
        Ok(())
    }

    /// `columnate_fillrows`: entries across, then down.
    fn columnate_fillrows(&mut self, out: &mut Vec<u8>) {
        self.maxlength = self.maxlength.saturating_add(TABCHAR_CELLS) & !(TABCHAR_CELLS - 1);
        let numcols = self
            .termwidth
            .unwrap_or(0)
            .checked_div(self.maxlength)
            .unwrap_or(0);
        let mut endcol = self.maxlength;
        let (mut chcnt, mut col) = (0usize, 0usize);
        let n = self.ents.len();
        for (i, entry) in self.ents.iter().enumerate() {
            out.extend_from_slice(&wcs_to_mbs(entry));
            chcnt = chcnt.saturating_add(width(entry));
            if i.saturating_add(1) == n {
                break;
            }
            col = col.saturating_add(1);
            if col == numcols {
                chcnt = 0;
                col = 0;
                endcol = self.maxlength;
                out.extend_from_slice(b"\n");
            } else {
                tab_to(out, &mut chcnt, endcol);
                endcol = endcol.saturating_add(self.maxlength);
            }
        }
        if chcnt != 0 {
            out.extend_from_slice(b"\n");
        }
    }

    /// `columnate_fillcols`: entries down, then across.
    fn columnate_fillcols(&mut self, out: &mut Vec<u8>) {
        self.maxlength = self.maxlength.saturating_add(TABCHAR_CELLS) & !(TABCHAR_CELLS - 1);
        let numcols = self
            .termwidth
            .unwrap_or(0)
            .checked_div(self.maxlength)
            .unwrap_or(0)
            .max(1);
        let nents = self.ents.len();
        let numrows = nents.div_ceil(numcols);
        for row in 0..numrows {
            let mut endcol = self.maxlength;
            let mut chcnt = 0usize;
            let mut base = row;
            for _ in 0..numcols {
                let Some(entry) = self.ents.get(base) else {
                    break;
                };
                out.extend_from_slice(&wcs_to_mbs(entry));
                chcnt = chcnt.saturating_add(width(entry));
                base = base.saturating_add(numrows);
                if base >= nents {
                    break;
                }
                tab_to(out, &mut chcnt, endcol);
                endcol = endcol.saturating_add(self.maxlength);
            }
            out.extend_from_slice(b"\n");
        }
    }

    /// `simple_print`: one entry a line.
    fn simple_print(&self, out: &mut Vec<u8>) {
        for entry in &self.ents {
            out.extend_from_slice(&wcs_to_mbs(entry));
            out.extend_from_slice(b"\n");
        }
    }
}

/// Tabs to the last tab stop at or before `endcol`.
fn tab_to(out: &mut Vec<u8>, chcnt: &mut usize, endcol: usize) {
    loop {
        let cnt = chcnt.saturating_add(TABCHAR_CELLS) & !(TABCHAR_CELLS - 1);
        if cnt > endcol {
            break;
        }
        out.extend_from_slice(b"\t");
        *chcnt = cnt;
    }
}

/// The option each parsed item stands for, as upstream's switch sees it.
fn option_code(opt: &Opt<'_>) -> Option<(u8, Option<OsString>)> {
    match opt {
        Opt::Short(c, value) => Some((*c, value.clone())),
        Opt::Long(name, value) => {
            let i = LONGS.iter().position(|&(n, _)| n == *name)?;
            Some((*LONG_VALS.get(i)?, value.clone()))
        }
        Opt::Operand(_) => None,
    }
}

/// `option_to_longopt(c, longopts)`: the first long option for `c`.
fn option_to_longopt(c: i32) -> Option<&'static str> {
    LONG_VALS
        .iter()
        .position(|&v| i32::from(v) == c)
        .and_then(|i| LONGS.get(i))
        .map(|&(name, _)| name)
}

/// `main()`.
#[allow(
    clippy::too_many_lines,
    reason = "upstream's main, kept in one piece so it can be read against it"
)]
fn run(argv: &[OsString], short: &[u8], out: &mut Stdout) -> u8 {
    let arg0 = argv
        .first()
        .map_or(OsStr::new("column"), OsString::as_os_str);
    let mut ctl = Ctl::new(smartcols::tty::codeset_is_utf8());
    // `setlocale(LC_ALL, "")`.
    ctl.errno = smartcols::tty::setlocale_errno();
    let mut excl_st = [0i32; 3];
    let mut files: Vec<&OsString> = Vec::new();

    let own = argv.get(1..).unwrap_or_default();
    for item in COLUMN.parse(own, SHORTS, LONGS) {
        let opt = match item {
            Ok(opt) => opt,
            Err(e) => {
                // glibc names the program by argv[0] as given.
                stderr_write(format!("{}: {}\n", shown(&os_bytes(arg0)), e.sentence).as_bytes());
                return errtryhelp(short);
            }
        };
        if let Opt::Operand(file) = opt {
            files.push(file);
            continue;
        }
        let Some((c, value)) = option_code(&opt) else {
            continue;
        };
        if let Some(msg) =
            err_exclusive_options(i32::from(c), &EXCL, &mut excl_st, option_to_longopt, short)
        {
            stderr_write(msg.as_bytes());
            return 1;
        }
        let arg = value
            .as_deref()
            .map(os_bytes)
            .unwrap_or_default()
            .into_owned();
        match c {
            b'C' => ctl.tab_columns.push(arg),
            b'c' => {
                if arg == b"unlimited" {
                    ctl.termwidth = Some(0);
                } else {
                    match strtou32_or_err(&arg, "invalid columns argument", short) {
                        Ok(w) => ctl.termwidth = usize::try_from(w).ok(),
                        Err(status) => return status,
                    }
                    // `ul_strtou64` cleared it.
                    ctl.errno = 0;
                }
            }
            b'd' => ctl.tab_noheadings = true,
            b'E' => ctl.tab_colnoextrem = Some(arg),
            b'e' => ctl.header_repeat = true,
            b'H' => {
                ctl.hide_unnamed = has_unnamed(&arg);
                ctl.tab_colhide = Some(arg);
            }
            b'i' => ctl.tree_id = Some(arg),
            b'J' => {
                ctl.json = true;
                ctl.mode = Mode::Table;
            }
            b'L' => ctl.keep_empty_lines = true,
            b'l' => {
                match strtou32_or_err(&arg, "invalid columns limit argument", short) {
                    Ok(n) => ctl.maxncols = usize::try_from(n).unwrap_or(usize::MAX),
                    Err(status) => return status,
                }
                ctl.errno = 0;
                if ctl.maxncols == 0 {
                    warnx(short, "columns limit must be greater than zero");
                    return 1;
                }
            }
            b'N' => ctl.tab_colnames = Some(strv_split(&arg)),
            b'n' => ctl.tab_name = Some(arg),
            b'm' => ctl.maxout = true,
            b'O' => ctl.tab_order = Some(arg),
            b'o' => ctl.output_separator = arg,
            b'p' => ctl.tree_parent = Some(arg),
            b'R' => ctl.tab_colright = Some(arg),
            b'r' => ctl.tree = Some(arg),
            b's' => match mbs_to_wcs(smartcols::mbs::c_str(&arg), ctl.utf8) {
                Some(seps) => {
                    ctl.input_separator = seps;
                    ctl.greedy = false;
                }
                None => {
                    warn(
                        short,
                        "failed to use input separator",
                        &std::io::Error::from_raw_os_error(EILSEQ),
                    );
                    return 1;
                }
            },
            b'T' => ctl.tab_coltrunc = Some(arg),
            b't' => ctl.mode = Mode::Table,
            b'W' => ctl.tab_colwrap = Some(arg),
            b'x' => ctl.mode = Mode::FillRows,
            b'h' => {
                out.write(&usage(short));
                return 0;
            }
            b'V' => {
                let mut line = short.to_vec();
                line.extend_from_slice(b" from util-linux 2.39.3\n");
                out.write(&line);
                return 0;
            }
            _ => return errtryhelp(short),
        }
    }

    if ctl.termwidth.is_none() {
        // `get_terminal_width(80)`.
        ctl.errno = smartcols::tty::dimension_errno(ctl.errno, true, false);
        ctl.termwidth = Some(smartcols::tty::terminal_dimension().0.unwrap_or(80));
    }
    if ctl.tree.is_some() {
        ctl.mode = Mode::Table;
        if ctl.tree_parent.is_none() || ctl.tree_id.is_none() {
            warnx(
                short,
                "options --tree-id and --tree-parent are required for tree formatting",
            );
            return 1;
        }
    }
    if ctl.mode != Mode::Table
        && (ctl.tab_order.is_some()
            || ctl.tab_name.is_some()
            || ctl.tab_colwrap.is_some()
            || ctl.tab_colhide.is_some()
            || ctl.tab_coltrunc.is_some()
            || ctl.tab_colnoextrem.is_some()
            || ctl.tab_colright.is_some()
            || ctl.tab_colnames.is_some()
            || !ctl.tab_columns.is_empty())
    {
        warnx(short, "option --table required for all --table-*");
        return 1;
    }
    if ctl.tab_colnames.is_none() && ctl.tab_columns.is_empty() && ctl.json {
        warnx(
            short,
            "option --table-columns or --table-column required for --json",
        );
        return 1;
    }

    // `eval`: an unsigned int, one for each file that would not open.
    let mut eval: u32 = 0;
    if files.is_empty() {
        if let Err(status) = ctl.read_input(sys::stdin(), short) {
            return status;
        }
    } else {
        for file in files {
            match std::fs::File::open(file) {
                Ok(f) => {
                    if let Err(status) = ctl.read_input(f, short) {
                        return status;
                    }
                }
                Err(e) => {
                    warn(short, &shown(&os_bytes(file)), &e);
                    ctl.errno = e.raw_os_error().unwrap_or(ENOENT);
                    eval = eval.wrapping_add(1);
                }
            }
        }
    }

    if ctl.mode != Mode::Table {
        if ctl.ents.is_empty() {
            // `exit(eval)`: the count, as the low byte of the status.
            return eval.to_le_bytes()[0];
        }
        if ctl.maxlength >= ctl.termwidth.unwrap_or(0) {
            ctl.mode = Mode::Simple;
        }
    }

    match ctl.mode {
        Mode::Table => {
            if ctl.tab.as_ref().is_some_and(|t| t.nlines() > 0) {
                if let Err(status) = ctl.modify_table(short) {
                    return status;
                }
                if let Some(tab) = ctl.tab.as_mut() {
                    let mut text = Vec::new();
                    let printed = tab.print_into(&mut text);
                    out.write(&text);
                    eval = u32::from(printed.is_err());
                }
            }
        }
        Mode::FillCols | Mode::FillRows | Mode::Simple => {
            // Written at once: what glibc's buffer does with it is the same
            // as for the pieces, since it only asks how much it holds.
            let mut text = Vec::new();
            match ctl.mode {
                Mode::FillCols => ctl.columnate_fillcols(&mut text),
                Mode::FillRows => ctl.columnate_fillrows(&mut text),
                _ => ctl.simple_print(&mut text),
            }
            out.orient_wide();
            out.write(&text);
        }
    }
    u8::from(eval != 0)
}

/// Standard input, read through descriptor 0 itself: Rust's own `Stdin`
/// reads a closed descriptor as an empty one, where upstream's `getline`
/// fails with `EBADF` -- `read failed`.
mod sys {
    use std::io::Read;

    #[cfg(unix)]
    pub fn stdin() -> impl Read {
        use std::os::fd::FromRawFd;
        // SAFETY: descriptor 0 belongs to the process for its whole life;
        // the `File` built on it is never dropped (`ManuallyDrop`), so it is
        // never closed here, and reading a descriptor that is closed only
        // fails with `EBADF`.
        let file = unsafe { std::fs::File::from_raw_fd(0) };
        Stdin(std::mem::ManuallyDrop::new(file))
    }

    /// Descriptor 0, borrowed.
    #[cfg(unix)]
    struct Stdin(std::mem::ManuallyDrop<std::fs::File>);

    #[cfg(unix)]
    impl Read for Stdin {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            self.0.read(buf)
        }
    }

    /// The Windows host the unit tests run on.
    #[cfg(not(unix))]
    pub fn stdin() -> impl Read {
        std::io::stdin()
    }
}

#[cfg(test)]
mod tests;
