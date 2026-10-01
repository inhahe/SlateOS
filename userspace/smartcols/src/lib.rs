//! libsmartcols -- util-linux's table printer, ported.
//!
//! Every util-linux program that prints a table -- `lsblk`, `findmnt`,
//! `lsmem`, `lscpu`, `lslocks`, `swapon --show` and a dozen more -- builds it
//! with libsmartcols and lets the library decide the rest: how wide each
//! column is, what is cut when the terminal is too narrow, how a tree is
//! drawn, and what `--raw`, `--pairs` and `--json` look like. A faithful port
//! of any of them therefore needs a faithful port of this, and this is the
//! one copy.
//!
//! It is util-linux 2.39.3's `libsmartcols/src/` -- `table.c`, `column.c`,
//! `line.c`, `cell.c`, `grouping.c`, `calculate.c`, `print.c`,
//! `print-api.c`, `walk.c` -- and the `lib/` code they stand on
//! (`mbsalign.c` as [`mbs`], `jsonwrt.c` as [`json`], `carefulputc.h` as
//! [`careful`], `buffer.c` inline, `list.h`'s `list_sort`, and
//! `ttyutils.c`'s terminal size), function by function and with upstream's
//! names in the documentation.
//!
//! # The shape of the port
//!
//! Upstream links lines, columns and cells through reference-counted
//! pointers and intrusive lists. Here a [`Table`] owns everything: columns
//! live in a vector in upstream's list order; lines live in a vector in the
//! order they were made, and a second vector holds the table's order of
//! them (`tb_lines`), which sorting changes. A [`LineId`] is a line's
//! identity, which never changes (the removal API is not ported; no caller
//! here uses it); a [`ColumnId`] is a column's identity, which
//! [`Table::move_column`] does not change though it moves the column.
//! Each line's cells are indexed as upstream's are, by the column's
//! `seqnum` -- its place in the list, except where upstream's own list
//! surgery goes wrong (see `move_column`), which is kept. A group of lines
//! (`lsblk --merge`'s chart) is an entry in the table's list of them,
//! named by its place there, which never changes either.
//! Output is written into a byte buffer the caller then writes out, so a
//! failed write is the caller's to report, as `close_stdout` reports it.
//!
//! # Where it is not upstream's
//!
//! A width is never reduced past zero. Upstream's widths are `size_t`, and
//! on a terminal narrow enough a reduction can pass zero and wrap to a width
//! in the quintillions -- `prlimit` on a 10-column terminal prints spaces
//! until it is killed. Here the reduction stops at zero: the column
//! vanishes, and the table fits. (Where upstream's unsigned arithmetic wraps
//! and comes back -- enlarging a column past its widest cell and back -- it
//! is kept, and the result is the same.)
//! And a character wider than its whole column is dropped from a wrapped
//! cell, where upstream keeps it pending and prints empty lines for ever.
//! And when every column is at its minimum and the table still does not
//! fit, the reduction stops after its last stage and the table prints as
//! wide as it is, where upstream's reduction loop never ends (see
//! `reduce_column`). And a column more than 2^32 cells wide -- which only a
//! width hint no program sets can ask for, through C's undefined
//! double-to-`size_t` conversion -- is refused before anything is printed,
//! where upstream pads it until it is killed. All four are cases in which
//! upstream never finishes; nothing that finishes differs.
//!
//! Where upstream's own lists would be corrupted or walked without end,
//! the port does what the list surgery means instead, and finishes: a
//! group's child given a parent line (see [`Table::line_add_child`]); and a
//! loop -- a line made its own ancestor, or a group member made its own
//! group's child -- which upstream's sort recurses through until the stack
//! runs out (see [`Table::sort`] and [`Table::sort_by_tree`]). No program
//! builds any of them; `scripts/smartcols-diff.sh` meets them, and counts
//! them apart.
//!
//! # What is not ported yet
//!
//! Each is refused or absent rather than approximated, and each is what a
//! program that needs it will have to add: custom wrapping functions other
//! than upstream's own newline one (`scols_column_set_wrapfunc` with
//! anything but `scols_wrapnl_chunksize`/`scols_wrapnl_nextchunk`, which is
//! [`Table::column_set_wrapnl`]), colours (every colour is ignored, as
//! upstream ignores them when colours are not wanted), printing a range that
//! starts or ends mid-table, removing single lines, and the debug output.

mod calc;
pub mod careful;
mod grouping;
pub mod json;
pub mod mbs;
mod print;
mod props;
mod sort;
pub mod tty;
mod walk;

pub use sort::{CellView, CmpFunc, cmpstr_cells};

/// `SCOLS_FL_TRUNC`: cut the data when the column is too narrow.
pub const FL_TRUNC: u32 = 1 << 0;
/// `SCOLS_FL_TREE`: draw the tree in this column.
pub const FL_TREE: u32 = 1 << 1;
/// `SCOLS_FL_RIGHT`: align to the right.
pub const FL_RIGHT: u32 = 1 << 2;
/// `SCOLS_FL_STRICTWIDTH`: never widen an empty column to its minimum.
pub const FL_STRICTWIDTH: u32 = 1 << 3;
/// `SCOLS_FL_NOEXTREMES`: ignore unusually wide cells when sizing.
pub const FL_NOEXTREMES: u32 = 1 << 4;
/// `SCOLS_FL_HIDDEN`: keep the data, print nothing.
pub const FL_HIDDEN: u32 = 1 << 5;
/// `SCOLS_FL_WRAP`: continue long data on extra lines.
pub const FL_WRAP: u32 = 1 << 6;

/// `SCOLS_CELL_FL_*`: how the title is aligned.
pub const CELL_FL_LEFT: u32 = 0;
pub const CELL_FL_CENTER: u32 = 1 << 0;
pub const CELL_FL_RIGHT: u32 = 1 << 1;

/// `SCOLS_JSON_*`: how a column's value is written in `--json`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum JsonType {
    /// `"value"`, or `null` when empty.
    #[default]
    String,
    /// `value` as it is, or `null`.
    Number,
    /// `true`/`false` from the first byte: empty, `0`, `N` or `n` is false.
    Boolean,
    /// `[ "value" ]`.
    ArrayString,
    /// `[ "value" ]` too: without a custom wrap function upstream writes the
    /// one element as a string.
    ArrayNumber,
    /// As [`JsonType::Boolean`], but empty or `-` is `null`.
    BooleanOptional,
}

/// `SCOLS_TERMFORCE_*`: whether the output is laid out for a terminal.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TermForce {
    /// When stdout is a terminal.
    #[default]
    Auto,
    Never,
    Always,
}

/// `SCOLS_FMT_*`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Format {
    #[default]
    Human,
    Raw,
    Export,
    Json,
}

/// A column, by identity: the order it was made in, which moving it does
/// not change.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ColumnId(pub(crate) usize);

/// A line, by identity: the order it was made in, which sorting the table
/// does not change.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LineId(pub(crate) usize);

/// A group of lines, by its place in the table's list of groups.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GroupId(pub(crate) usize);

/// Why a table could not be printed or changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// `-EINVAL`: a table with no columns, or an id from another table.
    Invalid,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Invalid argument")
    }
}

impl std::error::Error for Error {}

/// `struct libscols_cell`.
#[derive(Clone, Debug, Default)]
pub(crate) struct Cell {
    pub(crate) data: Option<Vec<u8>>,
    pub(crate) flags: u32,
    /// The data's width, as the last calculation measured it.
    pub(crate) width: usize,
    /// `userdata`: what the program keeps with the cell, never printed --
    /// `lsblk` keeps a number to sort by. Upstream's is a `void *`; the one
    /// program that sets it points it at a `uint64_t`.
    pub(crate) userdata: Option<u64>,
}

impl Cell {
    /// `scols_cell_get_data`: the data, or nothing.
    pub(crate) fn data(&self) -> Option<&[u8]> {
        self.data.as_deref()
    }
}

/// `struct libscols_wstat`, `__scols_calculate`'s statistics.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct WStat {
    pub(crate) width_min: usize,
    pub(crate) width_max: usize,
    pub(crate) width_avg: f64,
    pub(crate) width_sqr_sum: f64,
    pub(crate) width_deviation: f64,
}

/// `struct libscols_column`.
#[derive(Clone, Debug, Default)]
pub(crate) struct Column {
    /// Which column this is: its [`ColumnId`].
    pub(crate) id: usize,
    /// `seqnum`: which of each line's cells is this column's.
    pub(crate) seqnum: usize,
    pub(crate) width: usize,
    /// Kept across printings, as upstream keeps it: only ever grows.
    pub(crate) width_treeart: usize,
    pub(crate) width_hint: f64,
    pub(crate) wstat: WStat,
    pub(crate) json_type: JsonType,
    pub(crate) flags: u32,
    pub(crate) safechars: Option<Vec<u8>>,
    /// The rest of a wrapped cell, for the extra lines.
    pub(crate) pending: Option<Vec<u8>>,
    /// `scols_column_set_wrapfunc(cl, scols_wrapnl_chunksize,
    /// scols_wrapnl_nextchunk, NULL)`: a wrapped cell breaks at its
    /// newlines rather than at the column's width.
    pub(crate) wrapnl: bool,
    /// The name, as a cell.
    pub(crate) header: Cell,
    /// Whether this column draws the groups' chart: the first tree column
    /// a calculation finds, once the table has groups. Never unset, as
    /// upstream never unsets it.
    pub(crate) is_groups: bool,
    /// `cmpfunc`: how [`Table::sort`] orders lines by this column.
    pub(crate) cmpfunc: Option<CmpFunc>,
}

impl Column {
    pub(crate) fn is_hidden(&self) -> bool {
        self.flags & FL_HIDDEN != 0
    }
    pub(crate) fn is_trunc(&self) -> bool {
        self.flags & FL_TRUNC != 0
    }
    pub(crate) fn is_tree(&self) -> bool {
        self.flags & FL_TREE != 0
    }
    pub(crate) fn is_right(&self) -> bool {
        self.flags & FL_RIGHT != 0
    }
    pub(crate) fn is_strict_width(&self) -> bool {
        self.flags & FL_STRICTWIDTH != 0
    }
    pub(crate) fn is_noextremes(&self) -> bool {
        self.flags & FL_NOEXTREMES != 0
    }
    pub(crate) fn is_wrap(&self) -> bool {
        self.flags & FL_WRAP != 0
    }
    /// `scols_column_is_customwrap`: wrapped, by the newline functions.
    pub(crate) fn is_customwrap(&self) -> bool {
        self.is_wrap() && self.wrapnl
    }

    /// `scols_column_get_name_as_shellvar`: `1FOO%` is `_1FOO_PCT`.
    pub(crate) fn name_as_shellvar(&self) -> Option<Vec<u8>> {
        let name = self.header.data().filter(|n| !n.is_empty())?;
        let mut out = Vec::with_capacity(name.len().saturating_add(4));
        if !name.first().is_some_and(u8::is_ascii_alphabetic) {
            out.push(b'_');
        }
        out.extend(
            name.iter()
                .map(|&c| if c.is_ascii_alphanumeric() { c } else { b'_' }),
        );
        if name.last() == Some(&b'%') {
            out.extend_from_slice(b"PCT");
        }
        Some(out)
    }
}

/// `struct libscols_line`.
#[derive(Clone, Debug, Default)]
pub(crate) struct Line {
    pub(crate) cells: Vec<Cell>,
    pub(crate) parent: Option<LineId>,
    /// `ln_branch`: the children, in their order.
    pub(crate) children: Vec<LineId>,
    /// `parent_group`: the group this line is a child of -- drawn after
    /// the group's last member, as a child of all of them.
    pub(crate) parent_group: Option<GroupId>,
    /// `group`: the group this line is a member of.
    pub(crate) group: Option<GroupId>,
}

/// `SCOLS_GSTATE_*`: where the walk is in a group, as its chart shows it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum GState {
    /// Not drawn: not reached yet, or finished.
    #[default]
    None,
    FirstMember,
    MiddleMember,
    LastMember,
    MiddleChild,
    LastChild,
    /// Another line, between two members.
    ContMembers,
    /// Another line, between the last member and the last child.
    ContChildren,
}

/// `struct libscols_group`: lines drawn as one, with children in common.
#[derive(Clone, Debug, Default)]
pub(crate) struct Group {
    /// `nmembers`: how many lines were made members -- which the members
    /// list, rebuilt in the tree's order before each print, is checked
    /// against.
    pub(crate) nmembers: usize,
    /// `gr_members`.
    pub(crate) members: Vec<LineId>,
    /// `gr_children`.
    pub(crate) children: Vec<LineId>,
    pub(crate) state: GState,
}

/// `struct libscols_symbols`: the tree's and the groups' drawing. A group
/// symbol a program's own symbols leave unset (`None`) is drawn with
/// print.c's fallback for it.
#[derive(Clone, Debug)]
pub(crate) struct Symbols {
    pub(crate) tree_branch: Vec<u8>,
    pub(crate) tree_vert: Vec<u8>,
    pub(crate) tree_right: Vec<u8>,
    pub(crate) group_vert: Option<Vec<u8>>,
    pub(crate) group_horz: Option<Vec<u8>>,
    pub(crate) group_first_member: Option<Vec<u8>>,
    pub(crate) group_last_member: Option<Vec<u8>>,
    pub(crate) group_middle_member: Option<Vec<u8>>,
    pub(crate) group_last_child: Option<Vec<u8>>,
    pub(crate) group_middle_child: Option<Vec<u8>>,
    pub(crate) title_padding: Vec<u8>,
    pub(crate) cell_padding: Vec<u8>,
}

/// `struct libscols_table`.
#[derive(Debug)]
pub struct Table {
    pub(crate) name: Option<Vec<u8>>,
    /// The columns, in their order: upstream's `tb_columns`.
    pub(crate) columns: Vec<Column>,
    /// Columns `move_column` took out of the list without putting them back
    /// in it -- one moved behind itself, or behind one of these. Still the
    /// table's, and still holding their cells; never printed.
    pub(crate) detached: Vec<Column>,
    /// `tb->ncols`: every column ever added, detached ones too, and so the
    /// number of cells in each line.
    pub(crate) ncols: usize,
    /// Every line, indexed by its [`LineId`].
    pub(crate) lines: Vec<Line>,
    /// `tb_lines`: the lines in the table's order -- the order they were
    /// made in, until [`Table::sort`] or [`Table::sort_by_tree`].
    pub(crate) order: Vec<LineId>,
    /// `tb_groups`: every group, indexed by its [`GroupId`].
    pub(crate) groups: Vec<Group>,
    /// `grpset`: the groups' chart as the walk has it at the current line,
    /// [`grouping::GRPSET_CHUNKSIZ`] slots per group drawn. Grows as the
    /// walk needs room, and is kept (emptied, not shrunk) between walks.
    pub(crate) grpset: Vec<Option<GroupId>>,
    /// `ngrpchlds_pending`: groups whose last member the walk has reached
    /// and whose children it has not walked yet.
    pub(crate) ngrpchlds_pending: usize,
    /// `dflt_sort_column`: the column [`Table::sort`] last sorted by.
    pub(crate) dflt_sort_column: Option<ColumnId>,
    pub(crate) ntreecols: usize,
    pub(crate) termwidth: usize,
    pub(crate) termheight: usize,
    pub(crate) termreduce: usize,
    pub(crate) termforce: TermForce,
    pub(crate) colsep: Option<Vec<u8>>,
    pub(crate) linesep: Option<Vec<u8>>,
    pub(crate) symbols: Option<Symbols>,
    pub(crate) priv_symbols: bool,
    pub(crate) title: Cell,
    pub(crate) json: json::JsonWriter,
    pub(crate) format: Format,
    pub(crate) termlines_used: usize,
    pub(crate) header_next: usize,
    pub(crate) walk_last_tree_root: Option<LineId>,
    pub(crate) walk_last_done: bool,
    /// Whether the locale's codeset is UTF-8: how bytes are measured and
    /// which tree symbols are drawn.
    pub(crate) utf8: bool,
    pub(crate) ascii: bool,
    pub(crate) is_term: bool,
    pub(crate) is_dummy_print: bool,
    pub(crate) is_shellvar: bool,
    pub(crate) maxout: bool,
    pub(crate) minout: bool,
    pub(crate) header_repeat: bool,
    pub(crate) header_printed: bool,
    pub(crate) no_headings: bool,
    pub(crate) no_encode: bool,
    pub(crate) no_linesep: bool,
    pub(crate) no_wrap: bool,
}

impl Default for Table {
    fn default() -> Self {
        Self::new()
    }
}

impl Table {
    /// `scols_new_table()`: output to be printed on stdout, sized to its
    /// terminal -- or `COLUMNS`/`LINES`, or 80x24.
    #[must_use]
    pub fn new() -> Self {
        let (cols, rows) = tty::terminal_dimension();
        Table {
            name: None,
            columns: Vec::new(),
            detached: Vec::new(),
            ncols: 0,
            lines: Vec::new(),
            order: Vec::new(),
            groups: Vec::new(),
            grpset: Vec::new(),
            ngrpchlds_pending: 0,
            dflt_sort_column: None,
            ntreecols: 0,
            termwidth: cols.filter(|&c| c > 0).unwrap_or(80),
            termheight: rows.filter(|&r| r > 0).unwrap_or(24),
            termreduce: 0,
            termforce: TermForce::Auto,
            colsep: None,
            linesep: None,
            symbols: None,
            priv_symbols: false,
            title: Cell::default(),
            json: json::JsonWriter::new(0),
            format: Format::Human,
            termlines_used: 0,
            header_next: 0,
            walk_last_tree_root: None,
            walk_last_done: false,
            utf8: tty::codeset_is_utf8(),
            ascii: false,
            is_term: false,
            is_dummy_print: false,
            is_shellvar: false,
            maxout: false,
            minout: false,
            header_repeat: false,
            header_printed: false,
            no_headings: false,
            no_encode: false,
            no_linesep: false,
            no_wrap: false,
        }
    }

    /// Whether the locale's codeset is UTF-8, which [`Table::new`] reads from
    /// the environment as glibc's `setlocale(LC_ALL, "")` would. For a test,
    /// or a caller that knows better.
    pub fn set_utf8(&mut self, utf8: bool) {
        self.utf8 = utf8;
    }

    /// `scols_table_set_name`: the JSON array's name.
    pub fn set_name(&mut self, name: &[u8]) {
        self.name = Some(mbs::c_str(name).to_vec());
    }

    /// `scols_table_set_column_separator` (default `" "`).
    pub fn set_column_separator(&mut self, sep: &[u8]) {
        self.colsep = Some(mbs::c_str(sep).to_vec());
    }

    /// `scols_table_set_line_separator` (default `"\n"`).
    pub fn set_line_separator(&mut self, sep: &[u8]) {
        self.linesep = Some(mbs::c_str(sep).to_vec());
    }

    /// `scols_table_enable_raw`.
    pub fn enable_raw(&mut self, enable: bool) {
        self.set_format(Format::Raw, enable);
    }

    /// `scols_table_enable_json`.
    pub fn enable_json(&mut self, enable: bool) {
        self.set_format(Format::Json, enable);
    }

    /// `scols_table_enable_export`: `NAME="value"` pairs.
    pub fn enable_export(&mut self, enable: bool) {
        self.set_format(Format::Export, enable);
    }

    /// The formats are one setting: enabling one replaces the other, and
    /// disabling one that is not in force changes nothing.
    fn set_format(&mut self, format: Format, enable: bool) {
        if enable {
            self.format = format;
        } else if self.format == format {
            self.format = Format::Human;
        }
    }

    /// `scols_table_enable_shellvar`: column names usable as shell
    /// variables.
    pub fn enable_shellvar(&mut self, enable: bool) {
        self.is_shellvar = enable;
    }

    /// `scols_table_enable_ascii`: ASCII tree symbols.
    pub fn enable_ascii(&mut self, enable: bool) {
        self.ascii = enable;
    }

    /// `scols_table_enable_noheadings`.
    pub fn enable_noheadings(&mut self, enable: bool) {
        self.no_headings = enable;
    }

    /// `scols_table_enable_header_repeat`.
    pub fn enable_header_repeat(&mut self, enable: bool) {
        self.header_repeat = enable;
    }

    /// `scols_table_enable_maxout`: fill the terminal's width.
    ///
    /// # Errors
    ///
    /// Minout is on: the two exclude each other, and upstream changes
    /// nothing.
    pub fn enable_maxout(&mut self, enable: bool) -> Result<(), Error> {
        if self.minout {
            return Err(Error::Invalid);
        }
        self.maxout = enable;
        Ok(())
    }

    /// `scols_table_enable_minout`: no padding after the last data on a
    /// line.
    ///
    /// # Errors
    ///
    /// Maxout is on.
    pub fn enable_minout(&mut self, enable: bool) -> Result<(), Error> {
        if self.maxout {
            return Err(Error::Invalid);
        }
        self.minout = enable;
        Ok(())
    }

    /// `scols_table_enable_nowrap`: never continue a line.
    pub fn enable_nowrap(&mut self, enable: bool) {
        self.no_wrap = enable;
    }

    /// `scols_table_enable_noencoding`: print control characters as they
    /// are.
    pub fn enable_noencoding(&mut self, enable: bool) {
        self.no_encode = enable;
    }

    /// `scols_table_enable_nolinesep`.
    pub fn enable_nolinesep(&mut self, enable: bool) {
        self.no_linesep = enable;
    }

    /// `scols_table_set_termforce`.
    pub fn set_termforce(&mut self, force: TermForce) {
        self.termforce = force;
    }

    /// `scols_table_set_termwidth`.
    pub fn set_termwidth(&mut self, width: usize) {
        self.termwidth = width;
    }

    /// `scols_table_get_termwidth`.
    #[must_use]
    pub fn termwidth(&self) -> usize {
        self.termwidth
    }

    /// `scols_table_set_termheight`.
    pub fn set_termheight(&mut self, height: usize) {
        self.termheight = height;
    }

    /// `scols_table_reduce_termwidth`: leave `reduce` cells unused.
    pub fn reduce_termwidth(&mut self, reduce: usize) {
        self.termreduce = reduce;
    }

    /// `scols_table_is_json`.
    #[must_use]
    pub fn is_json(&self) -> bool {
        self.format == Format::Json
    }

    /// `scols_table_is_raw`.
    #[must_use]
    pub fn is_raw(&self) -> bool {
        self.format == Format::Raw
    }

    /// `scols_table_is_export`.
    #[must_use]
    pub fn is_export(&self) -> bool {
        self.format == Format::Export
    }

    /// `scols_table_is_tree`: some column draws the tree.
    #[must_use]
    pub fn is_tree(&self) -> bool {
        self.ntreecols > 0
    }

    /// `scols_new_symbols()`, `scols_symbols_set_branch`, `_vertical` and
    /// `_right`, then `scols_table_set_symbols`: the tree drawn with these
    /// pieces. The paddings, which a program setting its own symbols leaves
    /// `NULL`, are what upstream falls back to for `NULL` -- one space each.
    ///
    /// The groups' symbols are left unset too, and so drawn with print.c's
    /// fallbacks.
    pub fn set_tree_symbols(&mut self, branch: &[u8], vertical: &[u8], right: &[u8]) {
        self.symbols = Some(Symbols {
            tree_branch: branch.to_vec(),
            tree_vert: vertical.to_vec(),
            tree_right: right.to_vec(),
            group_vert: None,
            group_horz: None,
            group_first_member: None,
            group_last_member: None,
            group_middle_member: None,
            group_last_child: None,
            group_middle_child: None,
            title_padding: b" ".to_vec(),
            cell_padding: b" ".to_vec(),
        });
    }

    /// `scols_table_get_title` + `scols_cell_set_data` + the alignment:
    /// a line printed above a human-readable table.
    pub fn set_title(&mut self, title: &[u8], align: u32) {
        self.title.data = Some(mbs::c_str(title).to_vec());
        self.title.flags = align;
    }

    /// `scols_table_new_column(tb, name, whint, flags)`: a whint below 1 is a
    /// fraction of the terminal, 1 or more a number of cells.
    pub fn new_column(&mut self, name: &[u8], whint: f64, flags: u32) -> ColumnId {
        self.add_column(Some(name), whint, flags)
    }

    /// `scols_table_new_column(tb, NULL, whint, flags)`: a column with no
    /// name -- an empty header, and no key in `--json`.
    pub fn new_unnamed_column(&mut self, whint: f64, flags: u32) -> ColumnId {
        self.add_column(None, whint, flags)
    }

    /// `scols_table_add_column`: the column at the end of the list, and its
    /// cell -- the `ncols`th, detached columns counted -- in every line.
    fn add_column(&mut self, name: Option<&[u8]>, whint: f64, flags: u32) -> ColumnId {
        let seqnum = self.ncols;
        if flags & FL_TREE != 0 {
            self.ntreecols = self.ntreecols.saturating_add(1);
        }
        self.columns.push(Column {
            id: seqnum,
            seqnum,
            width_hint: whint,
            flags,
            header: Cell {
                data: name.map(|n| mbs::c_str(n).to_vec()),
                ..Cell::default()
            },
            ..Column::default()
        });
        self.ncols = seqnum.saturating_add(1);
        // `scols_line_alloc_cells`: every line has a cell for every column.
        for line in &mut self.lines {
            line.cells.resize_with(self.ncols, Cell::default);
        }
        ColumnId(seqnum)
    }

    /// Where `cl` is in the list, if it is in it.
    pub(crate) fn position(&self, cl: ColumnId) -> Option<usize> {
        // A column never moved is where its id says.
        if self.columns.get(cl.0).is_some_and(|c| c.id == cl.0) {
            return Some(cl.0);
        }
        self.columns.iter().position(|c| c.id == cl.0)
    }

    /// The column `cl`, in the list or detached.
    fn column_ref(&self, cl: ColumnId) -> Option<&Column> {
        match self.position(cl) {
            Some(i) => self.columns.get(i),
            None => self.detached.iter().find(|c| c.id == cl.0),
        }
    }

    fn column_mut(&mut self, cl: ColumnId) -> Option<&mut Column> {
        match self.position(cl) {
            Some(i) => self.columns.get_mut(i),
            None => self.detached.iter_mut().find(|c| c.id == cl.0),
        }
    }

    /// `scols_column_set_flags`, keeping the tree count right -- for a
    /// detached column too, as upstream's is still the table's.
    ///
    /// # Errors
    ///
    /// The column is not this table's.
    pub fn column_set_flags(&mut self, cl: ColumnId, flags: u32) -> Result<(), Error> {
        let column = self.column_mut(cl).ok_or(Error::Invalid)?;
        let was = column.flags & FL_TREE != 0;
        let is = flags & FL_TREE != 0;
        column.flags = flags;
        if was && !is {
            self.ntreecols = self.ntreecols.saturating_sub(1);
        } else if !was && is {
            self.ntreecols = self.ntreecols.saturating_add(1);
        }
        Ok(())
    }

    /// `scols_column_get_flags`.
    #[must_use]
    pub fn column_flags(&self, cl: ColumnId) -> Option<u32> {
        self.column_ref(cl).map(|c| c.flags)
    }

    /// `scols_column_get_name`: `None` for a column made without one.
    #[must_use]
    pub fn column_name(&self, cl: ColumnId) -> Option<&[u8]> {
        self.column_ref(cl).and_then(|c| c.header.data())
    }

    /// `scols_column_set_name`: the header, or none.
    ///
    /// # Errors
    ///
    /// The column is not this table's.
    pub fn column_set_name(&mut self, cl: ColumnId, name: Option<&[u8]>) -> Result<(), Error> {
        self.column_mut(cl).ok_or(Error::Invalid)?.header.data =
            name.map(|n| mbs::c_str(n).to_vec());
        Ok(())
    }

    /// `scols_column_set_whint`.
    ///
    /// # Errors
    ///
    /// The column is not this table's.
    pub fn column_set_whint(&mut self, cl: ColumnId, whint: f64) -> Result<(), Error> {
        self.column_mut(cl).ok_or(Error::Invalid)?.width_hint = whint;
        Ok(())
    }

    /// `scols_column_set_json_type`.
    ///
    /// # Errors
    ///
    /// The column is not this table's.
    pub fn column_set_json_type(&mut self, cl: ColumnId, ty: JsonType) -> Result<(), Error> {
        self.column_mut(cl).ok_or(Error::Invalid)?.json_type = ty;
        Ok(())
    }

    /// `scols_column_set_safechars`: bytes printed as they are, however
    /// they look.
    ///
    /// # Errors
    ///
    /// The column is not this table's.
    pub fn column_set_safechars(&mut self, cl: ColumnId, safe: &[u8]) -> Result<(), Error> {
        self.column_mut(cl).ok_or(Error::Invalid)?.safechars = Some(mbs::c_str(safe).to_vec());
        Ok(())
    }

    /// `scols_column_set_wrapfunc(cl, scols_wrapnl_chunksize,
    /// scols_wrapnl_nextchunk, NULL)`: when the column also has
    /// [`FL_WRAP`], a cell breaks at each newline, one piece per line; the
    /// column is as wide as the widest piece; and in JSON an array column
    /// is written one element per piece. (The last piece, if still too
    /// wide, is wrapped at the column's width, as upstream wraps it.)
    ///
    /// # Errors
    ///
    /// The column is not this table's.
    pub fn column_set_wrapnl(&mut self, cl: ColumnId) -> Result<(), Error> {
        self.column_mut(cl).ok_or(Error::Invalid)?.wrapnl = true;
        Ok(())
    }

    /// `scols_table_remove_lines`: every line gone, the columns kept, so
    /// the table can be filled and printed again.
    ///
    /// The groups go with them. Upstream's stay, their members kept alive
    /// by the groups' references though gone from the table, and would draw
    /// a chart of lines no longer printed; no program groups lines and then
    /// removes them.
    pub fn remove_lines(&mut self) {
        self.lines.clear();
        self.order.clear();
        self.groups.clear();
        self.grpset.clear();
        self.ngrpchlds_pending = 0;
    }

    /// The columns in their order (`scols_table_next_column`); detached
    /// ones are not in it.
    #[must_use]
    pub fn column_ids(&self) -> Vec<ColumnId> {
        self.columns.iter().map(|c| ColumnId(c.id)).collect()
    }

    /// `scols_table_get_column(tb, n)`: the column whose cells are each
    /// line's `n`th -- in a table whose columns were never detached, the
    /// `n`th column.
    #[must_use]
    pub fn column(&self, n: usize) -> Option<ColumnId> {
        if n >= self.ncols {
            return None;
        }
        self.columns
            .iter()
            .find(|c| c.seqnum == n)
            .map(|c| ColumnId(c.id))
    }

    /// `scols_table_get_column_by_name`: the first column so named.
    #[must_use]
    pub fn column_by_name(&self, name: &[u8]) -> Option<ColumnId> {
        self.columns
            .iter()
            .find(|c| c.header.data() == Some(name))
            .map(|c| ColumnId(c.id))
    }

    /// `scols_table_move_column(tb, pre, cl)`: `cl` after `pre`, or first
    /// with none, and each line's cells moved with it.
    ///
    /// Upstream's list surgery, kept exactly because `column --table-order`
    /// shows it: when `pre` is `cl` itself (`-O 1,1`), unlinking `cl` and
    /// linking it after itself leaves it in a list of its own, outside the
    /// table's. It is then never printed; the columns after it are
    /// renumbered while each line's cells stay where they were, so each of
    /// them shows the cell of the column before it. A column moved after
    /// such a detached one is detached with it, and a detached column moved
    /// after one in the list rejoins it, taking its cells from where its old
    /// number says they are.
    ///
    /// # Errors
    ///
    /// Either column is not this table's.
    pub fn move_column(&mut self, pre: Option<ColumnId>, cl: ColumnId) -> Result<(), Error> {
        let oldseq = self.column_ref(cl).ok_or(Error::Invalid)?.seqnum;
        if let Some(p) = pre {
            let pre_seq = self.column_ref(p).ok_or(Error::Invalid)?.seqnum;
            if pre_seq.checked_add(1) == Some(oldseq) {
                return Ok(());
            }
        } else if oldseq == 0 {
            return Ok(());
        }
        // `list_del_init`: out of whichever list holds it.
        let column = match self.position(cl) {
            Some(i) => self.columns.remove(i),
            None => {
                let i = self
                    .detached
                    .iter()
                    .position(|c| c.id == cl.0)
                    .ok_or(Error::Invalid)?;
                self.detached.remove(i)
            }
        };
        // `list_add`: after `pre` in whichever list holds it.
        match pre.map(|p| (p, self.position(p))) {
            None => self.columns.insert(0, column),
            Some((p, Some(i))) if p != cl => self.columns.insert(i.saturating_add(1), column),
            Some(_) => self.detached.push(column),
        }
        for (n, c) in self.columns.iter_mut().enumerate() {
            c.seqnum = n;
        }
        let newseq = self.column_ref(cl).map_or(oldseq, |c| c.seqnum);
        for line in &mut self.lines {
            line_move_cells(&mut line.cells, newseq, oldseq);
        }
        Ok(())
    }

    /// `scols_table_new_line(tb, parent)`: a line at the end of the table,
    /// and, with a parent, at the end of the parent's children.
    ///
    /// # Errors
    ///
    /// The parent is not this table's.
    pub fn new_line(&mut self, parent: Option<LineId>) -> Result<LineId, Error> {
        if parent.is_some_and(|p| p.0 >= self.lines.len()) {
            return Err(Error::Invalid);
        }
        let id = LineId(self.lines.len());
        self.lines.push(Line {
            cells: vec![Cell::default(); self.ncols],
            parent,
            ..Line::default()
        });
        self.order.push(id);
        if let Some(p) = parent
            && let Some(line) = self.lines.get_mut(p.0)
        {
            line.children.push(id);
        }
        Ok(id)
    }

    /// The lines in table order (`scols_table_next_line`), which moving a
    /// line in the tree does not change and sorting does.
    pub fn line_ids(&self) -> impl Iterator<Item = LineId> + use<> {
        self.order.clone().into_iter()
    }

    /// `scols_line_get_cell(ln, n)` and `scols_cell_set_userdata`: a number
    /// kept with the line's `n`th cell for a comparison function to read
    /// ([`CellView::userdata`]); never printed.
    ///
    /// # Errors
    ///
    /// The line is not this table's, or has no `n`th cell.
    pub fn cell_set_userdata(&mut self, ln: LineId, n: usize, data: u64) -> Result<(), Error> {
        self.lines
            .get_mut(ln.0)
            .and_then(|l| l.cells.get_mut(n))
            .ok_or(Error::Invalid)?
            .userdata = Some(data);
        Ok(())
    }

    /// `scols_column_set_cmpfunc(cl, cmp, NULL)`: how [`Table::sort`]
    /// orders lines by this column -- [`cmpstr_cells`], or the program's own.
    ///
    /// # Errors
    ///
    /// The column is not this table's.
    pub fn column_set_cmpfunc(&mut self, cl: ColumnId, cmp: CmpFunc) -> Result<(), Error> {
        self.column_mut(cl).ok_or(Error::Invalid)?.cmpfunc = Some(cmp);
        Ok(())
    }

    /// `scols_line_set_column_data(ln, cl, data)`: the cell's data, copied
    /// up to its first NUL, as a C string is.
    ///
    /// # Errors
    ///
    /// The line or the column is not this table's.
    pub fn line_set_data(&mut self, ln: LineId, cl: ColumnId, data: &[u8]) -> Result<(), Error> {
        let n = self.column_ref(cl).ok_or(Error::Invalid)?.seqnum;
        self.line_refer_data(ln, n, data)
    }

    /// `scols_line_refer_data(ln, n, data)`: the line's `n`th cell --
    /// whichever column's that is.
    ///
    /// # Errors
    ///
    /// The line is not this table's, or has no `n`th cell.
    pub fn line_refer_data(&mut self, ln: LineId, n: usize, data: &[u8]) -> Result<(), Error> {
        let cell = self
            .lines
            .get_mut(ln.0)
            .and_then(|l| l.cells.get_mut(n))
            .ok_or(Error::Invalid)?;
        cell.data = Some(mbs::c_str(data).to_vec());
        Ok(())
    }

    /// `scols_line_get_column_cell` and `scols_cell_get_data`: the data in
    /// `cl`'s cell of `ln`, if any was set.
    #[must_use]
    pub fn line_column_data(&self, ln: LineId, cl: ColumnId) -> Option<&[u8]> {
        let n = self.column_ref(cl)?.seqnum;
        self.line(ln)?.cells.get(n)?.data()
    }

    /// `scols_line_is_ancestor(ln, parent)`: whether `ln` is `parent` or
    /// one of its ancestors -- so whether making `ln` a child of `parent`
    /// would close a loop.
    ///
    /// Upstream follows the parents without end on a line whose ancestry
    /// already loops (which only `line_add_child` without this check
    /// makes); here the walk stops after as many steps as there are lines.
    #[must_use]
    pub fn line_is_ancestor(&self, ln: LineId, parent: LineId) -> bool {
        let mut cur = Some(parent);
        for _ in 0..=self.lines.len() {
            let Some(p) = cur else {
                return false;
            };
            if p == ln {
                return true;
            }
            cur = self.line(p).and_then(|l| l.parent);
        }
        false
    }

    /// `scols_line_add_child(ln, child)`: `child` taken from its parent's
    /// children, if it has a parent, and put last among `ln`'s.
    ///
    /// A group's child is taken from the group's children too. Upstream
    /// links it into `ln`'s children while the group's list still holds it,
    /// and so corrupts both lists; no program gives a group's child a
    /// parent.
    ///
    /// # Errors
    ///
    /// Either line is not this table's.
    pub fn line_add_child(&mut self, ln: LineId, child: LineId) -> Result<(), Error> {
        if ln.0 >= self.lines.len() || child.0 >= self.lines.len() {
            return Err(Error::Invalid);
        }
        self.detach_line(child);
        if let Some(line) = self.lines.get_mut(ln.0) {
            line.children.push(child);
        }
        if let Some(line) = self.lines.get_mut(child.0) {
            line.parent = Some(ln);
        }
        Ok(())
    }

    /// `scols_line_remove_child(ln, child)`: `child` taken from its parent's
    /// children, a root again. As upstream's, from whichever line is its
    /// parent, `ln` or not -- or from its group's children, since the one
    /// link is in either list; a group's child so taken still names its
    /// group, and so is no root, and is not printed.
    ///
    /// # Errors
    ///
    /// Either line is not this table's.
    pub fn line_remove_child(&mut self, ln: LineId, child: LineId) -> Result<(), Error> {
        if ln.0 >= self.lines.len() || child.0 >= self.lines.len() {
            return Err(Error::Invalid);
        }
        self.detach_line(child);
        Ok(())
    }

    /// `list_del_init(&child->ln_children)` -- out of its parent's children,
    /// or its group's -- and `child->parent = NULL`.
    fn detach_line(&mut self, child: LineId) {
        let (old, group) = match self.lines.get_mut(child.0) {
            Some(l) => (l.parent.take(), l.parent_group),
            None => return,
        };
        let list = match (old, group) {
            (Some(old), _) => self.lines.get_mut(old.0).map(|p| &mut p.children),
            (None, Some(gr)) => self.groups.get_mut(gr.0).map(|g| &mut g.children),
            (None, None) => None,
        };
        if let Some(list) = list
            && let Some(i) = list.iter().position(|&c| c == child)
        {
            list.remove(i);
        }
    }

    /// `scols_table_get_nlines`.
    #[must_use]
    pub fn nlines(&self) -> usize {
        self.lines.len()
    }

    /// `scols_table_get_ncols`: every column added, detached ones too.
    #[must_use]
    pub fn ncols(&self) -> usize {
        self.ncols
    }

    /// `scols_print_table`, into `out`: the table, and a final newline unless
    /// it is JSON, had nothing to print, or failed.
    ///
    /// # Errors
    ///
    /// The table has no columns (nothing is printed); or a wrapped cell's
    /// column was left no width, which ends the print where it is -- what
    /// was printed before stays in `out`, as upstream's stays on its stream,
    /// and a program that ignores the error (as `lsmem` and `prlimit`
    /// upstream do) writes it all the same.
    pub fn print_into(&mut self, out: &mut Vec<u8>) -> Result<(), Error> {
        let empty = self.do_print(out)?;
        if !empty && !self.is_json() {
            out.push(b'\n');
        }
        Ok(())
    }

    /// [`Table::print_into`] a new buffer. On an error the partial output
    /// is lost; a program that must write it uses `print_into`.
    ///
    /// # Errors
    ///
    /// As [`Table::print_into`].
    pub fn print(&mut self) -> Result<Vec<u8>, Error> {
        let mut out = Vec::new();
        self.print_into(&mut out)?;
        Ok(out)
    }

    /// `scols_table_print_range(tb, NULL, NULL)`: every line, as a list,
    /// without the JSON brackets around the table (each line is an object
    /// of its own) and without a final newline -- the header only the first
    /// time, since a range continues what an earlier one printed.
    ///
    /// # Errors
    ///
    /// The table is a tree (upstream's `-EINVAL`), or a wrapped cell's
    /// column was left no width; what was printed stays in `out`.
    pub fn print_range_into(&mut self, out: &mut Vec<u8>) -> Result<(), Error> {
        if self.is_tree() {
            return Err(Error::Invalid);
        }
        self.do_print_range(out)
    }

    /// `scols_print_table_to_string`: as [`Table::print`], without the final
    /// newline.
    ///
    /// # Errors
    ///
    /// The table has no columns.
    pub fn print_to_string(&mut self) -> Result<Vec<u8>, Error> {
        let mut out = Vec::new();
        self.do_print(&mut out)?;
        Ok(out)
    }

    /// `colsep(tb)`.
    pub(crate) fn colsep(&self) -> Vec<u8> {
        self.colsep.clone().unwrap_or_else(|| b" ".to_vec())
    }

    /// `linesep(tb)`.
    pub(crate) fn linesep(&self) -> Vec<u8> {
        self.linesep.clone().unwrap_or_else(|| b"\n".to_vec())
    }

    /// `is_last_column(cl)`: the last, or followed only by hidden ones.
    pub(crate) fn is_last_column(&self, cl: usize) -> bool {
        let next = cl.saturating_add(1);
        match self.columns.get(next) {
            None => true,
            Some(c) => c.is_hidden() && self.is_last_column(next),
        }
    }

    /// `is_last_child(ln)`.
    pub(crate) fn is_last_child(&self, ln: LineId) -> bool {
        let Some(parent) = self.line(ln).and_then(|l| l.parent) else {
            return false;
        };
        self.line(parent)
            .and_then(|p| p.children.last())
            .is_some_and(|&last| last == ln)
    }

    pub(crate) fn line(&self, ln: LineId) -> Option<&Line> {
        self.lines.get(ln.0)
    }

    /// The cell of `ln` for the column at position `cl`: the one its
    /// `seqnum` names.
    pub(crate) fn cell(&self, ln: LineId, cl: usize) -> Option<&Cell> {
        let n = self.columns.get(cl)?.seqnum;
        self.line(ln).and_then(|l| l.cells.get(n))
    }

    pub(crate) fn cell_mut(&mut self, ln: LineId, cl: usize) -> Option<&mut Cell> {
        let n = self.columns.get(cl)?.seqnum;
        self.lines.get_mut(ln.0).and_then(|l| l.cells.get_mut(n))
    }
}

/// `scols_line_move_cells(ln, newn, oldn)`: the cell at `oldn` moved to
/// `newn`, those between shifted to make room. Nothing for a position past
/// the cells, as upstream refuses it.
fn line_move_cells(cells: &mut Vec<Cell>, newn: usize, oldn: usize) {
    if newn >= cells.len() || oldn >= cells.len() || newn == oldn {
        return;
    }
    let cell = cells.remove(oldn);
    cells.insert(newn, cell);
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::float_cmp
)]
mod tests;
