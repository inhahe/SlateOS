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
    pub(crate) fn append(&mut self, s: &[u8]) {
        self.data.extend_from_slice(mbs::c_str(s));
    }

    /// `ul_buffer_append_ntimes`: `s`, `n` times.
    pub(crate) fn append_ntimes(&mut self, n: usize, s: &[u8]) {
        for _ in 0..n {
            self.append(s);
        }
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

/// The widest column printed: 2^32 cells. Only a width hint no program
/// sets -- `column --table-column width=`, negative or near 2^64, reaching
/// C's undefined conversion (see `calc::to_size`) -- makes one wider, and
/// upstream then pads it until it is killed.
const MAX_PRINTED_WIDTH: usize = 1 << 32;

// table.c's box-drawing characters, byte for byte as its octal escapes
// spell them. (Its comments name U+2504 for `UTF_H3`; the bytes are U+2508.)
/// `UTF_V`: U+2502 `│`.
const UTF_V: &[u8] = b"\xe2\x94\x82";
/// `UTF_VR`: U+251C `├`.
const UTF_VR: &[u8] = b"\xe2\x94\x9c";
/// `UTF_H`: U+2500 `─`.
const UTF_H: &[u8] = b"\xe2\x94\x80";
/// `UTF_UR`: U+2514 `└`.
const UTF_UR: &[u8] = b"\xe2\x94\x94";
/// `UTF_V3`: U+2506 `┆`.
const UTF_V3: &[u8] = b"\xe2\x94\x86";
/// `UTF_H3`: U+2508 `┈`.
const UTF_H3: &[u8] = b"\xe2\x94\x88";
/// `UTF_DR`: U+250C `┌`.
const UTF_DR: &[u8] = b"\xe2\x94\x8c";
/// `UTF_DH`: U+252C `┬`.
const UTF_DH: &[u8] = b"\xe2\x94\xac";
/// `UTF_TR`: U+25B6 `▶`.
const UTF_TR: &[u8] = b"\xe2\x96\xb6";

/// `scols_table_set_default_symbols`: the drawing symbols of a table printed
/// with none set -- Unicode box lines in a UTF-8 locale unless ASCII was
/// asked for.
fn default_symbols(ascii: bool, utf8: bool) -> Symbols {
    let s = |parts: &[&[u8]]| Some(parts.concat());
    if !ascii && utf8 {
        Symbols {
            tree_branch: [UTF_VR, UTF_H].concat(),
            tree_vert: [UTF_V, b" "].concat(),
            tree_right: [UTF_UR, UTF_H].concat(),
            group_horz: s(&[UTF_H3]),
            group_vert: s(&[UTF_V3]),
            group_first_member: s(&[UTF_DR, UTF_H3, UTF_TR]),
            group_last_member: s(&[UTF_UR, UTF_DH, UTF_TR]),
            group_middle_member: s(&[UTF_VR, UTF_H3, UTF_TR]),
            group_last_child: s(&[UTF_UR, UTF_H3]),
            group_middle_child: s(&[UTF_VR, UTF_H3]),
            title_padding: b" ".to_vec(),
            cell_padding: b" ".to_vec(),
        }
    } else {
        Symbols {
            tree_branch: b"|-".to_vec(),
            tree_vert: b"| ".to_vec(),
            tree_right: b"`-".to_vec(),
            group_horz: s(&[b"-"]),
            group_vert: s(&[b"|"]),
            group_first_member: s(&[b",->"]),
            group_last_member: s(&[b"'->"]),
            group_middle_member: s(&[b"|->"]),
            group_last_child: s(&[b"`-"]),
            group_middle_child: s(&[b"|-"]),
            title_padding: b" ".to_vec(),
            cell_padding: b" ".to_vec(),
        }
    }
}

impl Table {
    pub(crate) fn sym(&self) -> Symbols {
        self.symbols
            .clone()
            .unwrap_or_else(|| default_symbols(self.ascii, self.utf8))
    }

    fn cell_padding(&self) -> Vec<u8> {
        self.sym().cell_padding
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

    /// `__cell_to_buffer`: the cell's data, after the groups' chart and the
    /// tree's drawing in the tree column.
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
        let is_groups = self.columns.get(cl).is_some_and(|c| c.is_groups);
        if !json && is_groups {
            self.groups_art(buf, false);
        }
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

    /// `print_empty_cell`: padding, or in the tree column the groups' and
    /// the tree's continuing lines.
    fn print_empty_cell(&self, out: &mut Vec<u8>, cl: usize, ln: Option<LineId>) {
        let mut len_pad = 0usize;
        if let Some(ln) = ln
            && let Some(col) = self.columns.get(cl)
            && col.is_tree()
        {
            let mut art = Buf::default();
            if col.is_groups {
                self.groups_art(&mut art, true);
            }
            self.tree_art(ln, &mut art);
            if self.has_children(ln) && self.has_pending_data() {
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
        let customwrap = self
            .columns
            .get(cl)
            .is_some_and(crate::Column::is_customwrap);
        let nl = if customwrap {
            data.iter().position(|&b| b == b'\n')
        } else {
            None
        };
        let bytes = if let Some(nl) = nl {
            // `scols_wrapnl_nextchunk`: this piece, whatever its width; the
            // newline goes with it.
            data.truncate(nl);
            len = self.chunk_width(&data);
            nl.saturating_add(1)
        } else {
            mbs::mbs_truncate(&mut data, &mut len, self.utf8)
        };
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

    /// The width of one piece of an (already encoded) custom-wrapped cell:
    /// `mbs_safe_nwidth` over the encoded bytes, as upstream measures it --
    /// so an encoded `\x..` counts its backslash as four cells.
    fn chunk_width(&self, data: &[u8]) -> usize {
        if self.no_encode {
            mbs::mbs_width(data, self.utf8)
        } else {
            mbs::mbs_safe_width(data, self.utf8)
        }
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
                if self
                    .columns
                    .get(cl)
                    .is_some_and(crate::Column::is_customwrap)
                {
                    // One element per newline-separated piece; a trailing
                    // newline ends the last piece rather than starting an
                    // empty one.
                    let mut rest = Some(mbs::c_str(data));
                    while let Some(d) = rest {
                        let (piece, next) = match d.iter().position(|&b| b == b'\n') {
                            Some(i) => {
                                (d.get(..i).unwrap_or_default(), d.get(i.saturating_add(1)..))
                            }
                            None => (d, None),
                        };
                        if ty == JsonType::ArrayString {
                            self.json.value_s(out, None, piece);
                        } else {
                            self.json.value_raw(out, None, piece);
                        }
                        rest = next.filter(|n| !n.is_empty());
                    }
                } else {
                    self.json.value_s(out, None, data);
                }
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
        if is_last && self.is_json() && self.is_tree() && ln.is_some_and(|l| self.has_children(l)) {
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
        let (is_right, is_trunc, is_customwrap) =
            (col.is_right(), col.is_trunc(), col.is_customwrap());
        let is_wrap = col.is_wrap() && !is_customwrap;

        // A custom multi-line cell: this piece now, the rest on the extra
        // lines (nothing left over when the data ends with the newline).
        if is_customwrap && let Some(nl) = data.iter().position(|&b| b == b'\n') {
            let rest = data
                .get(nl.saturating_add(1)..)
                .unwrap_or_default()
                .to_vec();
            if let Some(col) = self.columns.get_mut(cl) {
                col.pending = (!rest.is_empty()).then_some(rest);
            }
            data.truncate(nl);
            len = self.chunk_width(&data);
        }

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
            // Above the groups' chart, as many blanks as it is wide.
            if col.is_groups && self.is_tree() && col.is_tree() {
                buf.data
                    .extend(std::iter::repeat_n(b' ', self.grpset.len().saturating_add(1)));
            }
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
        let order = self.order.clone();
        let n = order.len();
        for (i, ln) in order.into_iter().enumerate() {
            let last = i.saturating_add(1) == n;
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
        if self.has_children(ln) {
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
                let is_child = self.is_child(line);
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

    /// `__scols_print_tree`: `scols_walk_tree` with `print_tree_line`.
    fn print_tree(&mut self, out: &mut Vec<u8>, buf: &mut Buf) -> Result<(), crate::Error> {
        self.walk_tree(&mut |tb: &mut Table, ln| tb.print_tree_line(out, ln, buf))
    }

    /// `__scols_initialize_printing`.
    ///
    /// # Errors
    ///
    /// The width calculation's walk failed (see [`Table::walk_tree`]); the
    /// printing is cleaned up again and nothing is printed.
    fn initialize_printing(&mut self, buf: &mut Buf) -> Result<(), crate::Error> {
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
        // The groups' members in the order the tree will walk them.
        if self.has_groups() && self.is_tree() {
            self.groups_fix_members_order();
        }
        if self.format == Format::Human
            && let Err(e) = self.calculate(buf)
        {
            self.cleanup_printing();
            return Err(e);
        }
        Ok(())
    }

    /// `__scols_cleanup_printing`.
    fn cleanup_printing(&mut self) {
        if self.priv_symbols {
            self.symbols = None;
            self.priv_symbols = false;
        }
    }

    /// `scols_table_print_range(tb, NULL, NULL)` for a table that is not a
    /// tree: the header (once, ever), then every line; no JSON brackets
    /// around them and no newline after the last.
    ///
    /// # Errors
    ///
    /// No columns, or a line that could not be printed.
    pub(crate) fn do_print_range(&mut self, out: &mut Vec<u8>) -> Result<(), crate::Error> {
        if self.columns.is_empty() {
            return Err(crate::Error::Invalid);
        }
        let mut buf = Buf::default();
        self.initialize_printing(&mut buf)?;
        if self
            .columns
            .iter()
            .any(|c| !c.is_hidden() && c.width > MAX_PRINTED_WIDTH)
        {
            self.cleanup_printing();
            return Err(crate::Error::Invalid);
        }
        self.print_header(out, &mut buf);
        let printed = self.print_lines(out, &mut buf);
        self.cleanup_printing();
        printed
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
        self.initialize_printing(&mut buf)?;
        if self
            .columns
            .iter()
            .any(|c| !c.is_hidden() && c.width > MAX_PRINTED_WIDTH)
        {
            // Upstream starts printing and pads the column for as long as it
            // is let; this refuses before the first byte.
            self.cleanup_printing();
            return Err(crate::Error::Invalid);
        }
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
