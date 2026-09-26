//! `print.c`, `print-api.c` and `walk.c`: the table written out, in each of
//! the four formats, with a tree's drawing, wrapped cells continued on extra
//! lines, and the header and title.

use crate::json::Kind;
use crate::{Cell, Format, JsonType, LineId, Symbols, Table, careful, mbs};

/// `struct ul_buffer`: one cell's text as it is built, and where the tree's
/// drawing in it ends (`SCOLS_BUFPTR_TREEEND`).
#[derive(Debug, Default)]
pub(crate) struct Buf {
    pub(crate) data: Vec<u8>,
    pub(crate) treeend: Option<usize>,
}

impl Buf {
    /// `ul_buffer_reset_data`.
    fn reset(&mut self) {
        self.data.clear();
        self.treeend = None;
    }

    /// `ul_buffer_append_string`: nothing for an empty string.
    fn append(&mut self, s: &[u8]) {
        self.data.extend_from_slice(mbs::c_str(s));
    }

    /// `ul_buffer_save_pointer(buf, SCOLS_BUFPTR_TREEEND)`.
    fn save_treeend(&mut self) {
        self.treeend = Some(self.data.len());
    }

    /// `ul_buffer_get_safe_pointer_width(buf, SCOLS_BUFPTR_TREEEND)`: the
    /// encoded width of the drawing.
    pub(crate) fn safe_pointer_width(&self, utf8: bool) -> usize {
        match self.treeend {
            Some(len) if len > 0 => {
                mbs::mbs_safe_nwidth(self.data.get(..len).unwrap_or_default(), utf8).0
            }
            _ => 0,
        }
    }
}

/// The drawing symbols of a table printed with none set: Unicode box lines
/// in a UTF-8 locale unless ASCII was asked for.
fn default_symbols(ascii: bool, utf8: bool) -> Symbols {
    if !ascii && utf8 {
        Symbols {
            tree_branch: "\u{251c}\u{2500}".as_bytes().to_vec(),
            tree_vert: "\u{2502} ".as_bytes().to_vec(),
            tree_right: "\u{2514}\u{2500}".as_bytes().to_vec(),
            title_padding: b" ".to_vec(),
            cell_padding: b" ".to_vec(),
        }
    } else {
        Symbols {
            tree_branch: b"|-".to_vec(),
            tree_vert: b"| ".to_vec(),
            tree_right: b"`-".to_vec(),
            title_padding: b" ".to_vec(),
            cell_padding: b" ".to_vec(),
        }
    }
}

impl Table {
    fn sym(&self) -> Symbols {
        self.symbols
            .clone()
            .unwrap_or_else(|| default_symbols(self.ascii, self.utf8))
    }

    fn cell_padding(&self) -> Vec<u8> {
        self.sym().cell_padding
    }

    fn has_children(&self, ln: Option<LineId>) -> bool {
        ln.and_then(|l| self.line(l))
            .is_some_and(|l| !l.children.is_empty())
    }

    /// `tree_ascii_art_to_buffer(tb, ln, buf)`: for `ln` and each of its
    /// ancestors below the root, a gap under a last child and a vertical
    /// line under any other.
    fn tree_art(&self, ln: LineId, buf: &mut Buf) {
        let Some(parent) = self.line(ln).and_then(|l| l.parent) else {
            return;
        };
        self.tree_art(parent, buf);
        if self.is_last_child(ln) {
            buf.append(b"  ");
        } else {
            buf.append(&self.sym().tree_vert);
        }
    }

    /// `__cell_to_buffer`: the cell's data, after the tree's drawing in the
    /// tree column.
    pub(crate) fn cell_to_buffer(&self, ln: LineId, cl: usize, buf: &mut Buf) {
        buf.reset();
        let data = self.cell(ln, cl).and_then(Cell::data).map(<[u8]>::to_vec);
        let is_tree = self.columns.get(cl).is_some_and(crate::Column::is_tree);
        if !is_tree {
            if let Some(d) = data {
                buf.append(&d);
            }
            return;
        }
        let parent = self.line(ln).and_then(|l| l.parent);
        let json = self.is_json();
        if let Some(parent) = parent
            && !json
        {
            self.tree_art(parent, buf);
            if self.is_last_child(ln) {
                buf.append(&self.sym().tree_right);
            } else {
                buf.append(&self.sym().tree_branch);
            }
        }
        let is_groups = self.columns.get(cl).is_some_and(|c| c.is_groups);
        if (parent.is_some() || is_groups) && !json {
            buf.save_treeend();
        }
        if let Some(d) = data {
            buf.append(&d);
        }
    }

    /// `is_next_columns_empty`: nothing but hidden or empty cells after `cl`
    /// on this line (a tree column counts as not empty).
    fn is_next_columns_empty(&self, cl: usize, ln: Option<LineId>) -> bool {
        if self.is_last_column(cl) {
            return true;
        }
        let Some(ln) = ln else {
            return false;
        };
        for next in cl.saturating_add(1)..self.columns.len() {
            let Some(col) = self.columns.get(next) else {
                continue;
            };
            if col.is_hidden() {
                continue;
            }
            if col.is_tree() {
                return false;
            }
            if self
                .cell(ln, next)
                .and_then(Cell::data)
                .is_some_and(|d| !d.is_empty())
            {
                return false;
            }
        }
        true
    }

    /// `has_pending_data`.
    fn has_pending_data(&self) -> bool {
        self.columns
            .iter()
            .any(|c| !c.is_hidden() && c.pending.is_some())
    }

    /// Fill to `width` from `len`, then the separator unless last -- or
    /// nothing at all for minout's empty tail or a default last column.
    fn finish_cell(
        &self,
        out: &mut Vec<u8>,
        cl: usize,
        ln: Option<LineId>,
        len: usize,
        width: usize,
        is_last: bool,
    ) -> bool {
        if self.minout && self.is_next_columns_empty(cl, ln) {
            return false;
        }
        if !self.maxout && is_last {
            return false;
        }
        let pad = self.cell_padding();
        for _ in len..width {
            out.extend_from_slice(&pad);
        }
        true
    }

    /// `print_empty_cell`: padding, or in the tree column the tree's
    /// continuing lines.
    fn print_empty_cell(&self, out: &mut Vec<u8>, cl: usize, ln: Option<LineId>) {
        let mut len_pad = 0usize;
        if let Some(ln) = ln
            && self.columns.get(cl).is_some_and(crate::Column::is_tree)
        {
            let mut art = Buf::default();
            self.tree_art(ln, &mut art);
            if self.has_children(Some(ln)) && self.has_pending_data() {
                art.append(&self.sym().tree_vert);
            }
            let data = if self.no_encode {
                len_pad = mbs::mbs_width(&art.data, self.utf8);
                Some(art.data.clone())
            } else {
                mbs::mbs_safe_encode(&art.data, None, self.utf8).map(|(d, w)| {
                    len_pad = w;
                    d
                })
            };
            if let Some(d) = data
                && len_pad > 0
            {
                out.extend_from_slice(&d);
            }
        }
        let width = self.columns.get(cl).map_or(0, |c| c.width);
        let is_last = self.is_last_column(cl);
        if self.finish_cell(out, cl, ln, len_pad, width, is_last) && !is_last {
            out.extend_from_slice(&self.colsep());
        }
    }

    /// `print_newline_padding`: a line break, then every column up to and
    /// including `cl` as empty cells -- hidden ones too, as upstream does.
    fn print_newline_padding(&mut self, out: &mut Vec<u8>, cl: usize, ln: Option<LineId>) {
        out.extend_from_slice(&self.linesep());
        self.termlines_used = self.termlines_used.saturating_add(1);
        for i in 0..=cl {
            self.print_empty_cell(out, i, ln);
        }
    }

    /// `step_pending_data`.
    fn step_pending(&mut self, cl: usize, bytes: usize) {
        if let Some(col) = self.columns.get_mut(cl)
            && let Some(p) = col.pending.as_mut()
        {
            if bytes >= p.len() {
                col.pending = None;
            } else {
                p.drain(..bytes);
            }
        }
    }

    /// `print_pending_data`: the next piece of a wrapped cell.
    ///
    /// # Errors
    ///
    /// The column has no width left to print it in: upstream's `-EINVAL`,
    /// which ends the print. (Returning without it would leave the data
    /// pending, and the caller's loop printing empty lines for ever.)
    fn print_pending_data(
        &mut self,
        out: &mut Vec<u8>,
        cl: usize,
        ln: Option<LineId>,
    ) -> Result<(), crate::Error> {
        let Some(mut data) = self.columns.get(cl).and_then(|c| c.pending.clone()) else {
            return Ok(());
        };
        let width = self.columns.get(cl).map_or(0, |c| c.width);
        if width == 0 {
            return Err(crate::Error::Invalid);
        }
        let mut len = width;
        let bytes = mbs::mbs_truncate(&mut data, &mut len, self.utf8);
        if bytes > 0 {
            self.step_pending(cl, bytes);
        } else if let Some(col) = self.columns.get_mut(cl) {
            // Nothing fits -- a character wider than the column. Upstream
            // keeps the data and prints empty lines for ever; here the rest
            // of the cell is dropped instead.
            col.pending = None;
        }
        out.extend_from_slice(&data);
        let is_last = self.is_last_column(cl);
        if self.finish_cell(out, cl, ln, len, width, is_last) && !is_last {
            out.extend_from_slice(&self.colsep());
        }
        Ok(())
    }

    /// `print_json_data`.
    fn print_json_data(&mut self, out: &mut Vec<u8>, cl: usize, name: Option<&[u8]>, data: &[u8]) {
        let ty = self
            .columns
            .get(cl)
            .map_or(JsonType::String, |c| c.json_type);
        match ty {
            JsonType::String => self.json.value_s(out, name, data),
            JsonType::Number => self.json.value_raw(out, name, data),
            JsonType::Boolean | JsonType::BooleanOptional => {
                if ty == JsonType::BooleanOptional && (data.is_empty() || data == b"-") {
                    self.json.value_null(out, name);
                } else {
                    let v = !matches!(data.first(), None | Some(b'0' | b'N' | b'n'));
                    self.json.value_boolean(out, name, v);
                }
            }
            JsonType::ArrayString | JsonType::ArrayNumber => {
                self.json.open(out, name, Kind::Array);
                self.json.value_s(out, None, data);
                self.json.close(out, Kind::Array);
            }
        }
    }

    /// `print_data`: one cell in the table's format.
    #[allow(
        clippy::too_many_lines,
        reason = "upstream's one function, kept whole to be read against it"
    )]
    fn print_data(&mut self, out: &mut Vec<u8>, cl: usize, ln: Option<LineId>, buf: &Buf) {
        let raw = buf.data.clone();
        let name: Option<Vec<u8>> = if self.format == Format::Human {
            None
        } else {
            self.columns.get(cl).and_then(|c| {
                if self.is_shellvar {
                    c.name_as_shellvar()
                } else {
                    c.header.data().map(<[u8]>::to_vec)
                }
            })
        };
        let mut is_last = self.is_last_column(cl);
        if is_last && self.is_json() && self.is_tree() && self.has_children(ln) {
            // "children": [] is the real last value.
            is_last = false;
        }
        match self.format {
            Format::Raw => {
                careful::fputs_nonblank(out, &raw);
                if !is_last {
                    out.extend_from_slice(&self.colsep());
                }
                return;
            }
            Format::Export => {
                out.extend_from_slice(name.as_deref().unwrap_or_default());
                out.push(b'=');
                careful::fputs_quoted(out, &raw);
                if !is_last {
                    out.extend_from_slice(&self.colsep());
                }
                return;
            }
            Format::Json => {
                self.print_json_data(out, cl, name.as_deref(), &raw);
                return;
            }
            Format::Human => {}
        }

        // Encode: `len` and `width` are cells, `bytes` bytes.
        let (mut data, mut len) = if self.no_encode {
            let w = if raw.is_empty() {
                0
            } else {
                mbs::mbs_width(&raw, self.utf8)
            };
            (raw, w)
        } else {
            let safe = self.columns.get(cl).and_then(|c| c.safechars.clone());
            mbs::mbs_safe_encode(&raw, safe.as_deref(), self.utf8).unwrap_or_default()
        };
        let Some(col) = self.columns.get(cl) else {
            return;
        };
        let mut width = col.width;
        let (is_right, is_trunc, is_wrap) = (col.is_right(), col.is_trunc(), col.is_wrap());

        if is_last && len < width && !self.maxout && !is_right {
            width = len;
        }
        // Truncate.
        if len > width && is_trunc {
            len = width;
            mbs::mbs_truncate(&mut data, &mut len, self.utf8);
        }
        // A standard multi-line cell: the rest waits for the extra lines.
        if len > width && is_wrap {
            if let Some(col) = self.columns.get_mut(cl) {
                col.pending = (!data.is_empty()).then(|| data.clone());
            }
            len = width;
            let bytes = mbs::mbs_truncate(&mut data, &mut len, self.utf8);
            if bytes > 0 {
                self.step_pending(cl, bytes);
            }
        }

        if !data.is_empty() {
            if is_right {
                let pad = self.cell_padding();
                for _ in len..width {
                    out.extend_from_slice(&pad);
                }
                len = width;
            }
            out.extend_from_slice(&data);
        }
        if !self.finish_cell(out, cl, ln, len, width, is_last) {
            return;
        }
        if len > width && !is_trunc {
            // The next column starts on the next line.
            self.print_newline_padding(out, cl, ln);
        } else if !is_last {
            out.extend_from_slice(&self.colsep());
        }
    }

    /// `print_line`, with the extra lines of wrapped cells.
    ///
    /// # Errors
    ///
    /// A wrapped cell's column has no width (`print_pending_data`).
    fn print_line(
        &mut self,
        out: &mut Vec<u8>,
        ln: LineId,
        buf: &mut Buf,
    ) -> Result<(), crate::Error> {
        let mut pending = false;
        for cl in 0..self.columns.len() {
            if self.columns.get(cl).is_some_and(crate::Column::is_hidden) {
                continue;
            }
            self.cell_to_buffer(ln, cl, buf);
            self.print_data(out, cl, Some(ln), buf);
            if self.columns.get(cl).is_some_and(|c| c.pending.is_some()) {
                pending = true;
            }
        }
        while pending {
            pending = false;
            out.extend_from_slice(&self.linesep());
            self.termlines_used = self.termlines_used.saturating_add(1);
            for cl in 0..self.columns.len() {
                let Some(col) = self.columns.get(cl) else {
                    continue;
                };
                if col.is_hidden() {
                    continue;
                }
                if col.pending.is_some() {
                    self.print_pending_data(out, cl, Some(ln))?;
                    if self.columns.get(cl).is_some_and(|c| c.pending.is_some()) {
                        pending = true;
                    }
                } else {
                    self.print_empty_cell(out, cl, Some(ln));
                }
            }
        }
        Ok(())
    }

    /// `__scols_print_title`.
    fn print_title(&self, out: &mut Vec<u8>) {
        let Some(title) = self.title.data() else {
            return;
        };
        let (text, len) = if self.no_encode {
            // `len = bufsz = strlen(title) + 1`: the terminating NUL is
            // counted, so a title on the left keeps one cell of padding.
            (title.to_vec(), title.len().saturating_add(1))
        } else {
            match mbs::mbs_safe_encode(title, None, self.utf8) {
                Some((t, w)) if w > 0 => (t, w),
                // "title is empty string -- ignore", or not encodable.
                _ => return,
            }
        };
        let mut width = if self.is_term { self.termwidth } else { 80 };
        let padding = self.sym().title_padding;
        let padchar = padding.first().copied().unwrap_or(b' ');
        let align = if self.title.flags & crate::CELL_FL_RIGHT != 0 {
            mbs::Align::Right
        } else if self.title.flags & crate::CELL_FL_CENTER != 0 {
            mbs::Align::Center
        } else {
            // No blank padding after a title on the left, as for the last
            // column.
            if len < width && !self.maxout && (padchar == b' ' || padchar == b'\t') {
                width = len;
            }
            mbs::Align::Left
        };
        out.extend_from_slice(&mbs::mbsalign(&text, &mut width, align, padchar, self.utf8));
        out.push(b'\n');
    }

    /// `__scols_print_header`.
    fn print_header(&mut self, out: &mut Vec<u8>, buf: &mut Buf) {
        if (self.header_printed && !self.header_repeat)
            || self.no_headings
            || self.format == Format::Export
            || self.is_json()
            || self.lines.is_empty()
        {
            return;
        }
        for cl in 0..self.columns.len() {
            let Some(col) = self.columns.get(cl) else {
                continue;
            };
            if col.is_hidden() {
                continue;
            }
            buf.reset();
            let name = if self.is_shellvar {
                col.name_as_shellvar()
            } else {
                col.header.data().map(<[u8]>::to_vec)
            };
            if let Some(name) = name {
                buf.append(&name);
            }
            self.print_data(out, cl, None, buf);
        }
        out.extend_from_slice(&self.linesep());
        self.termlines_used = self.termlines_used.saturating_add(1);
        self.header_printed = true;
        self.header_next = self.termlines_used.saturating_add(self.termheight);
    }

    /// `want_repeat_header`.
    fn want_repeat_header(&self) -> bool {
        !self.header_repeat || self.header_next <= self.termlines_used
    }

    /// `__scols_print_table` / `__scols_print_range` over every line; a line
    /// that fails still gets its closing (`fput_line_close`), and ends the
    /// range.
    fn print_lines(&mut self, out: &mut Vec<u8>, buf: &mut Buf) -> Result<(), crate::Error> {
        let n = self.lines.len();
        for i in 0..n {
            let last = i.saturating_add(1) == n;
            let ln = LineId(i);
            if self.is_json() {
                self.json.open(out, None, Kind::Object);
            }
            let rc = self.print_line(out, ln, buf);
            if self.is_json() {
                self.json.close(out, Kind::Object);
            } else if !last && !self.no_linesep {
                out.extend_from_slice(&self.linesep());
                self.termlines_used = self.termlines_used.saturating_add(1);
            }
            rc?;
            if !last && self.want_repeat_header() {
                self.print_header(out, buf);
            }
        }
        Ok(())
    }

    fn is_tree_root(&self, ln: LineId) -> bool {
        self.line(ln).is_some_and(|l| l.parent.is_none())
    }

    fn is_last_tree_root(&self, ln: LineId) -> bool {
        self.walk_last_tree_root == Some(ln)
    }

    /// `scols_walk_is_last`: the last line the walk will print.
    fn walk_is_last(&self, ln: LineId) -> bool {
        if !self.walk_last_done || self.has_children(Some(ln)) {
            return false;
        }
        if self.is_tree_root(ln) && !self.is_last_tree_root(ln) {
            return false;
        }
        if self.line(ln).and_then(|l| l.parent).is_some() {
            if !self.is_last_child(ln) {
                return false;
            }
            let mut parent = self.line(ln).and_then(|l| l.parent);
            while let Some(p) = parent {
                let pp = self.line(p).and_then(|l| l.parent);
                if pp.is_some() && !self.is_last_child(p) {
                    return false;
                }
                if pp.is_none() {
                    if !self.is_last_tree_root(p) {
                        return false;
                    }
                    break;
                }
                parent = pp;
            }
        }
        true
    }

    /// `print_tree_line`, the walk's callback. A line that fails ends the
    /// walk there, before its closing, as upstream's returns at once.
    fn print_tree_line(
        &mut self,
        out: &mut Vec<u8>,
        ln: LineId,
        buf: &mut Buf,
    ) -> Result<(), crate::Error> {
        if self.is_json() {
            self.json.open(out, None, Kind::Object);
        }
        self.print_line(out, ln, buf)?;
        if self.has_children(Some(ln)) {
            if self.is_json() {
                self.json.open(out, Some(b"children"), Kind::Array);
            } else {
                // Between a parent and its first child.
                out.extend_from_slice(&self.linesep());
                self.termlines_used = self.termlines_used.saturating_add(1);
            }
        } else if self.is_json() {
            // Close every object and "children" array this leaf ends.
            let mut cur = Some(ln);
            while let Some(line) = cur {
                let is_child = self.line(line).and_then(|l| l.parent).is_some();
                let last = (is_child && self.is_last_child(line))
                    || (self.is_tree_root(line) && self.is_last_tree_root(line));
                self.json.close(out, Kind::Object);
                if last && is_child {
                    self.json.close(out, Kind::Array);
                }
                cur = self.line(line).and_then(|l| l.parent);
                if !last {
                    break;
                }
            }
        } else if !self.no_linesep && !self.walk_is_last(ln) {
            out.extend_from_slice(&self.linesep());
            self.termlines_used = self.termlines_used.saturating_add(1);
        }
        Ok(())
    }

    /// `walk_line`: a line, then its children in order; the first failure
    /// ends the walk.
    fn walk_line(
        &mut self,
        out: &mut Vec<u8>,
        ln: LineId,
        buf: &mut Buf,
    ) -> Result<(), crate::Error> {
        self.print_tree_line(out, ln, buf)?;
        let children = self
            .line(ln)
            .map(|l| l.children.clone())
            .unwrap_or_default();
        for child in children {
            self.walk_line(out, child, buf)?;
        }
        Ok(())
    }

    /// `__scols_print_tree`: `scols_walk_tree` with `print_tree_line`.
    fn print_tree(&mut self, out: &mut Vec<u8>, buf: &mut Buf) -> Result<(), crate::Error> {
        self.walk_last_tree_root = None;
        self.walk_last_done = false;
        // The last root, in table order.
        for i in 0..self.lines.len() {
            let ln = LineId(i);
            if self.walk_last_tree_root.is_none() {
                self.walk_last_tree_root = Some(ln);
            }
            if !self.is_tree_root(ln) {
                continue;
            }
            self.walk_last_tree_root = Some(ln);
        }
        for i in 0..self.lines.len() {
            let ln = LineId(i);
            if !self.is_tree_root(ln) {
                continue;
            }
            if self.walk_last_tree_root == Some(ln) {
                self.walk_last_done = true;
            }
            if let Err(e) = self.walk_line(out, ln, buf) {
                self.walk_last_done = false;
                return Err(e);
            }
        }
        self.walk_last_done = false;
        Ok(())
    }

    /// `__scols_initialize_printing`.
    fn initialize_printing(&mut self, buf: &mut Buf) {
        if self.symbols.is_none() {
            self.symbols = Some(default_symbols(self.ascii, self.utf8));
            self.priv_symbols = true;
        } else {
            self.priv_symbols = false;
        }
        if self.format == Format::Human {
            self.is_term = match self.termforce {
                crate::TermForce::Never => false,
                crate::TermForce::Always => true,
                crate::TermForce::Auto => crate::tty::stdout_is_tty(),
            };
        }
        if self.is_term && self.termreduce > 0 && self.termreduce < self.termwidth {
            self.termwidth = self.termwidth.saturating_sub(self.termreduce);
        }
        if !self.is_term || self.format != Format::Human || self.is_tree() {
            self.header_repeat = false;
        }
        if self.is_json() {
            self.json = crate::json::JsonWriter::new(0);
        }
        if self.format == Format::Human {
            self.calculate(buf);
        }
    }

    /// `__scols_cleanup_printing`.
    fn cleanup_printing(&mut self) {
        if self.priv_symbols {
            self.symbols = None;
            self.priv_symbols = false;
        }
    }

    /// `do_print_table`: whether there was nothing to print.
    ///
    /// # Errors
    ///
    /// No columns; or a line that could not be printed, which ends the table
    /// -- the JSON brackets are still closed, and what came before it stays
    /// in `out`, as upstream's stays on its stream.
    pub(crate) fn do_print(&mut self, out: &mut Vec<u8>) -> Result<bool, crate::Error> {
        if self.columns.is_empty() {
            return Err(crate::Error::Invalid);
        }
        let name = self.name.clone().unwrap_or_default();
        if self.lines.is_empty() {
            if self.is_json() {
                self.json = crate::json::JsonWriter::new(0);
                self.json.open(out, None, Kind::Object);
                self.json.open(out, Some(&name), Kind::Array);
                self.json.close(out, Kind::Array);
                self.json.close(out, Kind::Object);
                return Ok(false);
            }
            return Ok(true);
        }
        self.header_printed = false;
        let mut buf = Buf::default();
        self.initialize_printing(&mut buf);
        if self.is_json() {
            self.json.open(out, None, Kind::Object);
            self.json.open(out, Some(&name), Kind::Array);
        }
        if self.format == Format::Human {
            self.print_title(out);
        }
        self.print_header(out, &mut buf);
        let printed = if self.is_tree() {
            self.print_tree(out, &mut buf)
        } else {
            self.print_lines(out, &mut buf)
        };
        if self.is_json() {
            self.json.close(out, Kind::Array);
            self.json.close(out, Kind::Object);
        }
        self.cleanup_printing();
        printed.map(|()| false)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use crate::{FL_WRAP, Table, TermForce};

    /// Upstream's `print_pending_data` answers `-EINVAL` for a column with
    /// no width, and the print stops; carrying on would leave the data
    /// pending and the caller printing empty lines for ever. The width
    /// calculation never leaves a wrapped column at zero (it hides a column
    /// it reduces to nothing), so this is set up by hand.
    #[test]
    fn a_wrapped_cell_with_no_width_to_go_into_is_an_error() {
        let mut tb = Table::new();
        tb.set_termforce(TermForce::Never);
        tb.new_column(b"A", 0.0, FL_WRAP);
        tb.columns[0].width = 0;
        tb.columns[0].pending = Some(b"abc".to_vec());
        let mut out = Vec::new();
        assert_eq!(
            tb.print_pending_data(&mut out, 0, None),
            Err(crate::Error::Invalid)
        );
        assert!(out.is_empty());
    }
}
