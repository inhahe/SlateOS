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
//! `line.c`, `cell.c`, `calculate.c`, `print.c`, `print-api.c`, `walk.c` --
//! and the `lib/` code they stand on (`mbsalign.c` as [`mbs`], `jsonwrt.c`
//! as [`json`], `carefulputc.h` as [`careful`], `buffer.c` inline, and
//! `ttyutils.c`'s terminal size), function by function and with upstream's
//! names in the documentation.
//!
//! # The shape of the port
//!
//! Upstream links lines, columns and cells through reference-counted
//! pointers and intrusive lists. Here a [`Table`] owns everything: columns
//! and lines live in vectors in upstream's list order. A [`LineId`] is a
//! line's place in the table, which never changes (the removal API is not
//! ported; no caller here uses it); a [`ColumnId`] is a column's identity,
//! which [`Table::move_column`] does not change though it moves the column.
//! Each line's cells are indexed as upstream's are, by the column's
//! `seqnum` -- its place in the list, except where upstream's own list
//! surgery goes wrong (see `move_column`), which is kept.
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
//! # What is not ported yet
//!
//! Each is refused or absent rather than approximated, and each is what a
//! program that needs it will have to add: groups (`scols_line_link_group`,
//! the group chart), custom wrapping functions (`scols_column_set_wrapfunc`),
//! sorting (`scols_sort_table`), colours (every colour is ignored, as upstream
//! ignores them when colours are not wanted), printing a range, and the
//! debug output.

mod calc;
pub mod careful;
pub mod json;
pub mod mbs;
mod print;
mod props;
pub mod tty;

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

/// A line, by its position among the table's lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LineId(pub(crate) usize);

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
    /// The name, as a cell.
    pub(crate) header: Cell,
    pub(crate) is_groups: bool,
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
    pub(crate) children: Vec<LineId>,
}

/// `struct libscols_symbols`: the tree's drawing.
#[derive(Clone, Debug)]
pub(crate) struct Symbols {
    pub(crate) tree_branch: Vec<u8>,
    pub(crate) tree_vert: Vec<u8>,
    pub(crate) tree_right: Vec<u8>,
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
    pub(crate) lines: Vec<Line>,
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

    /// `scols_table_is_tree`: some column draws the tree.
    #[must_use]
    pub fn is_tree(&self) -> bool {
        self.ntreecols > 0
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
            children: Vec::new(),
        });
        if let Some(p) = parent
            && let Some(line) = self.lines.get_mut(p.0)
        {
            line.children.push(id);
        }
        Ok(id)
    }

    /// The lines in table order (`scols_table_next_line`), which moving a
    /// line in the tree does not change.
    pub fn line_ids(&self) -> impl Iterator<Item = LineId> + use<> {
        (0..self.lines.len()).map(LineId)
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
    /// parent, `ln` or not.
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

    /// `list_del_init(&child->ln_children)` and `child->parent = NULL`.
    fn detach_line(&mut self, child: LineId) {
        let old = self.lines.get_mut(child.0).and_then(|l| l.parent.take());
        if let Some(old) = old
            && let Some(parent) = self.lines.get_mut(old.0)
            && let Some(i) = parent.children.iter().position(|&c| c == child)
        {
            parent.children.remove(i);
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
