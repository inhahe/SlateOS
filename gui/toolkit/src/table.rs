//! Column geometry for hand-drawn tables.
//!
//! Several apps draw a table by hand — a header row of labels, then a body loop
//! that walks a cursor across the row pushing one [`RenderCommand::Text`] per
//! cell. Written directly, every column width ends up recorded two or three
//! times: once in the header array, once in each cell's `max_width`, and once
//! more as a literal in the cursor's increment. `apps/torrent` had all three
//! (`300.0` / `Some(300.0)` / `cx += 308.0`), agreeing only by hand.
//!
//! The drift is the obvious hazard, but the worse one is subtler: when a width
//! lives in three places, no single place is "the column", so nothing ever asks
//! whether a cell's text *fits* in it. That question has no home, and the cells
//! that overflow — nearly always the ones holding a filename, a path, or a
//! string that arrived over the network — clip silently against whatever is
//! drawn beside them. `RenderCommand::Text`'s `max_width` is a clip, not a
//! wrap and not an ellipsis: it stops mid-glyph with no marker that anything
//! was dropped.
//!
//! [`Table`] gives the width exactly one home. The header and the body both ask
//! it where a column starts and how wide it is, so they cannot disagree, and
//! [`Table::cell`] elides to that width with the cut marked. Because the
//! geometry is queryable ([`Table::left`], [`Table::right`]), a test can walk a
//! panel's render commands and assert that no cell escapes its column — which
//! is how the overflows in `apps/torrent` were found.
//!
//! ```no_run
//! use guitk::table::{Column, Fit, Table};
//! # use guitk::render::{FontWeightHint, RenderCommand};
//! # use guitk::color::Color;
//! # let (text_color, dim) = (Color::from_hex(0xCDD6F4), Color::from_hex(0x6C7086));
//! const COLUMNS: &[Column] = &[
//!     Column { label: "Name", width: 260.0 },
//!     Column { label: "Path", width: 200.0 },
//! ];
//!
//! let mut cmds = Vec::new();
//! let table = Table::new(COLUMNS, 16.0);
//! table.header(&mut cmds, 4.0, dim, 11.0);
//! table.cell(&mut cmds, 0, 24.0, "report.txt", text_color, 12.0, Fit::Start);
//! // A path's tail is what identifies it, so it is cut at the front.
//! table.cell(&mut cmds, 1, 24.0, "/home/u/very/deep", dim, 11.0, Fit::End);
//! ```

use crate::color::Color;
use crate::render::{FontWeightHint, RenderCommand};
use crate::text;

/// One column of a table: its heading and its content width in pixels.
///
/// The width is the space available to the cell *text*; the gap between
/// columns is added by [`Table`] and is not part of it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Column {
    /// Heading drawn by [`Table::header`].
    pub label: &'static str,
    /// Width available to this column's text, in pixels.
    pub width: f32,
}

/// Default horizontal gap between one column's text and the next column's left
/// edge.
pub const DEFAULT_GAP: f32 = 8.0;

/// The ellipsis [`Table::cell`] marks a truncated cell with.
const ELLIPSIS: &str = "…";

/// Which end of an over-long cell survives truncation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Fit {
    /// Keep the start, mark the cut at the end. The right choice for names,
    /// labels and anything read left-to-right.
    #[default]
    Start,
    /// Keep the *end*, mark the cut at the front.
    ///
    /// For paths and URLs, whose identifying part is the tail. A file path cut
    /// the usual way reads `Some Show/Season 1/Episode 01 - A Very…`, which
    /// renders every episode in a season identically; cut from the front it
    /// reads `…- The One With The Thing.mkv`, which names the file.
    End,
}

/// A table's column geometry, anchored at an x position.
///
/// Cheap to construct — hold one for the duration of a render and let both the
/// header and the body rows ask it for positions.
#[derive(Clone, Copy, Debug)]
pub struct Table<'a> {
    columns: &'a [Column],
    x: f32,
    gap: f32,
}

impl<'a> Table<'a> {
    /// A table whose first column's text begins one [`DEFAULT_GAP`] right of
    /// `x`, so that `x` can be the enclosing panel's left edge.
    #[must_use]
    pub fn new(columns: &'a [Column], x: f32) -> Self {
        Self {
            columns,
            x,
            gap: DEFAULT_GAP,
        }
    }

    /// As [`Table::new`], with a custom inter-column gap.
    #[must_use]
    pub fn with_gap(columns: &'a [Column], x: f32, gap: f32) -> Self {
        Self { columns, x, gap }
    }

    /// The columns this table was built from.
    #[must_use]
    pub fn columns(&self) -> &'a [Column] {
        self.columns
    }

    /// Number of columns.
    #[must_use]
    pub fn len(&self) -> usize {
        self.columns.len()
    }

    /// Whether the table has no columns.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.columns.is_empty()
    }

    /// Width of column `index`, or 0 if there is no such column.
    #[must_use]
    pub fn width(&self, index: usize) -> f32 {
        self.columns.get(index).map_or(0.0, |c| c.width)
    }

    /// x at which column `index`'s text starts.
    ///
    /// Out-of-range indices return the x just past the last column rather than
    /// panicking: a table is a layout, and a caller asking about a column that
    /// is not there has a drawing bug, not a reason to take the app down.
    #[must_use]
    pub fn left(&self, index: usize) -> f32 {
        let mut cx = self.x + self.gap;
        for col in self.columns.iter().take(index) {
            cx += col.width + self.gap;
        }
        cx
    }

    /// x past which column `index`'s text must not draw.
    #[must_use]
    pub fn right(&self, index: usize) -> f32 {
        self.left(index) + self.width(index)
    }

    /// Total width the table occupies, including the leading and trailing gaps.
    #[must_use]
    pub fn total_width(&self) -> f32 {
        if self.columns.is_empty() {
            return 0.0;
        }
        self.left(self.columns.len()) - self.x
    }

    /// `(left, right)` x-range of every column, in order.
    ///
    /// Provided for tests that walk a panel's render commands and check that
    /// each cell stayed inside its column.
    #[must_use]
    pub fn spans(&self) -> Vec<(f32, f32)> {
        let mut spans = Vec::with_capacity(self.columns.len());
        let mut cx = self.x + self.gap;
        for col in self.columns {
            spans.push((cx, cx + col.width));
            cx += col.width + self.gap;
        }
        spans
    }

    /// Draw the header row at `y`, in bold.
    ///
    /// Labels are bold, and bold glyphs are wider than regular ones at the same
    /// size, so they are measured at the weight they are drawn at — measuring a
    /// bold heading as regular under-measures it and lets a real overflow
    /// through.
    pub fn header(&self, cmds: &mut Vec<RenderCommand>, y: f32, color: Color, font_size: f32) {
        self.header_weighted(cmds, y, color, font_size, FontWeightHint::Bold);
    }

    /// As [`Table::header`], at an explicit weight.
    ///
    /// Bold is the usual choice but not the only one: an app whose other tables
    /// mark their headings by colour alone gains a stray bold row from
    /// [`Table::header`], and "my headings are the wrong weight" is a bad reason
    /// to keep a table hand-drawn and lose the fitting that comes with this one.
    /// As with [`Table::cell_weighted`], the weight is not cosmetic — it decides
    /// how wide the glyphs are, so it decides where a long heading is cut.
    pub fn header_weighted(
        &self,
        cmds: &mut Vec<RenderCommand>,
        y: f32,
        color: Color,
        font_size: f32,
        font_weight: FontWeightHint,
    ) {
        for (i, col) in self.columns.iter().enumerate() {
            cmds.push(RenderCommand::Text {
                x: self.left(i),
                y,
                text: text::elide(col.label, col.width, ELLIPSIS, font_size, font_weight),
                font_size,
                color,
                font_weight,
                max_width: Some(col.width),
            });
        }
    }

    /// Draw one body cell in column `index` at `y`, fitted to that column.
    ///
    /// The text is elided to the column width with the cut marked, so a value
    /// too long to show is visibly shortened rather than silently clipped.
    /// That distinction matters most for the cells that hold data the app did
    /// not author — a filename, a path, a string off the network: a value
    /// chosen so its clipped form reads as some *other* value is a
    /// misrepresentation the user has no way to notice.
    #[allow(clippy::too_many_arguments)]
    pub fn cell(
        &self,
        cmds: &mut Vec<RenderCommand>,
        index: usize,
        y: f32,
        text: &str,
        color: Color,
        font_size: f32,
        fit: Fit,
    ) {
        self.cell_weighted(
            cmds,
            index,
            y,
            text,
            color,
            font_size,
            fit,
            FontWeightHint::Regular,
        );
    }

    /// As [`Table::cell`], at an explicit font weight.
    ///
    /// Separate because the weight is not cosmetic: it changes how wide the
    /// glyphs are, so it changes where the text has to be cut.
    #[allow(clippy::too_many_arguments)]
    pub fn cell_weighted(
        &self,
        cmds: &mut Vec<RenderCommand>,
        index: usize,
        y: f32,
        text: &str,
        color: Color,
        font_size: f32,
        fit: Fit,
        font_weight: FontWeightHint,
    ) {
        let width = self.width(index);
        let fitted = match fit {
            Fit::Start => text::elide(text, width, ELLIPSIS, font_size, font_weight),
            Fit::End => text::elide_start(text, width, ELLIPSIS, font_size, font_weight),
        };
        cmds.push(RenderCommand::Text {
            x: self.left(index),
            y,
            text: fitted,
            font_size,
            color,
            font_weight,
            max_width: Some(width),
        });
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    const COLUMNS: &[Column] = &[
        Column {
            label: "Name",
            width: 100.0,
        },
        Column {
            label: "Path",
            width: 60.0,
        },
        Column {
            label: "Size",
            width: 40.0,
        },
    ];

    const INK: Color = Color::from_hex(0xCDD6F4);

    fn texts(cmds: &[RenderCommand]) -> Vec<(f32, String, f32, FontWeightHint)> {
        cmds.iter()
            .filter_map(|cmd| match cmd {
                RenderCommand::Text {
                    x,
                    text,
                    font_size,
                    font_weight,
                    ..
                } => Some((*x, text.clone(), *font_size, *font_weight)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn columns_are_laid_out_left_to_right_with_gaps() {
        let table = Table::new(COLUMNS, 16.0);
        assert!((table.left(0) - 24.0).abs() < 0.01);
        assert!((table.right(0) - 124.0).abs() < 0.01);
        assert!((table.left(1) - 132.0).abs() < 0.01);
        assert!((table.right(1) - 192.0).abs() < 0.01);
        assert!((table.left(2) - 200.0).abs() < 0.01);
        assert!((table.right(2) - 240.0).abs() < 0.01);
    }

    #[test]
    fn a_column_never_overlaps_the_next() {
        let table = Table::new(COLUMNS, 0.0);
        let spans = table.spans();
        for pair in spans.windows(2) {
            assert!(
                pair[0].1 <= pair[1].0,
                "column ending at {} overlaps the next starting at {}",
                pair[0].1,
                pair[1].0
            );
        }
        assert_eq!(spans.len(), COLUMNS.len());
    }

    #[test]
    fn the_header_and_the_body_agree_on_where_a_column_starts() {
        // The whole point of the type: one source for the width, so these two
        // cannot drift the way three hand-written copies did.
        let table = Table::new(COLUMNS, 12.0);
        let mut header = Vec::new();
        table.header(&mut header, 0.0, INK, 11.0);
        let mut body = Vec::new();
        for i in 0..table.len() {
            table.cell(&mut body, i, 20.0, "x", INK, 11.0, Fit::Start);
        }
        let header_x: Vec<f32> = texts(&header).into_iter().map(|t| t.0).collect();
        let body_x: Vec<f32> = texts(&body).into_iter().map(|t| t.0).collect();
        assert_eq!(header_x.len(), COLUMNS.len());
        assert_eq!(header_x, body_x);
    }

    #[test]
    fn no_cell_draws_past_its_column() {
        let table = Table::new(COLUMNS, 8.0);
        let shouty = "a string very much longer than any of these columns is wide";
        let mut cmds = Vec::new();
        let mut checked = 0usize;
        for i in 0..table.len() {
            table.cell(&mut cmds, i, 0.0, shouty, INK, 11.0, Fit::Start);
            table.cell(&mut cmds, i, 20.0, shouty, INK, 11.0, Fit::End);
        }
        for (x, t, size, weight) in texts(&cmds) {
            let (_, right) = table
                .spans()
                .into_iter()
                .find(|(l, _)| (l - x).abs() < 0.01)
                .expect("every cell should sit at a column's left edge");
            let drawn = x + text::measure(&t, size, weight);
            assert!(
                drawn <= right + 0.01,
                "cell {t:?} at {x} draws to {drawn}, past its column edge {right}"
            );
            checked += 1;
        }
        assert!(checked >= COLUMNS.len() * 2, "only {checked} cells checked");
    }

    #[test]
    fn a_cut_cell_is_marked_at_the_end_by_default() {
        let table = Table::new(COLUMNS, 0.0);
        let mut cmds = Vec::new();
        table.cell(
            &mut cmds,
            1,
            0.0,
            "far too long for sixty pixels",
            INK,
            11.0,
            Fit::Start,
        );
        let drawn = &texts(&cmds)[0].1;
        assert!(drawn.ends_with(ELLIPSIS), "got {drawn:?}");
        assert!(drawn.starts_with("far"), "got {drawn:?}");
    }

    #[test]
    fn a_path_cell_keeps_its_tail() {
        let table = Table::new(COLUMNS, 0.0);
        let mut cmds = Vec::new();
        table.cell(
            &mut cmds,
            1,
            0.0,
            "/home/user/projects/deep/notes.txt",
            INK,
            11.0,
            Fit::End,
        );
        let drawn = &texts(&cmds)[0].1;
        assert!(drawn.starts_with(ELLIPSIS), "got {drawn:?}");
        assert!(drawn.ends_with("notes.txt"), "got {drawn:?}");
    }

    #[test]
    fn a_cell_that_fits_is_left_verbatim() {
        let table = Table::new(COLUMNS, 0.0);
        let mut cmds = Vec::new();
        table.cell(&mut cmds, 0, 0.0, "short", INK, 11.0, Fit::Start);
        table.cell(&mut cmds, 0, 20.0, "short", INK, 11.0, Fit::End);
        for (_, drawn, _, _) in texts(&cmds) {
            assert_eq!(drawn, "short");
        }
    }

    #[test]
    fn a_bold_cell_is_fitted_at_its_own_weight() {
        // Bold glyphs are wider, so fitting bold text as if it were regular
        // under-measures it and lets a real overflow through.
        let table = Table::new(COLUMNS, 0.0);
        let long = "a moderately long value here";
        let mut cmds = Vec::new();
        table.cell_weighted(
            &mut cmds,
            1,
            0.0,
            long,
            INK,
            11.0,
            Fit::Start,
            FontWeightHint::Bold,
        );
        let (x, drawn, size, weight) = texts(&cmds)[0].clone();
        assert_eq!(weight, FontWeightHint::Bold);
        let width = x + text::measure(&drawn, size, weight);
        assert!(
            width <= table.right(1) + 0.01,
            "bold cell {drawn:?} draws to {width}, past {}",
            table.right(1)
        );
    }

    #[test]
    fn a_column_too_narrow_for_the_ellipsis_draws_nothing() {
        // Better an empty cell than one glyph of a value the reader would
        // then misread as the whole of it.
        const NARROW: &[Column] = &[Column {
            label: "n",
            width: 1.0,
        }];
        let table = Table::new(NARROW, 0.0);
        let mut cmds = Vec::new();
        table.cell(&mut cmds, 0, 0.0, "something", INK, 11.0, Fit::Start);
        assert_eq!(texts(&cmds)[0].1, "");
    }

    #[test]
    fn an_out_of_range_column_does_not_panic() {
        let table = Table::new(COLUMNS, 0.0);
        assert!((table.width(9) - 0.0).abs() < f32::EPSILON);
        // Past the end of the table, not somewhere inside it.
        assert!(table.left(9) >= table.right(COLUMNS.len() - 1));
        let mut cmds = Vec::new();
        table.cell(&mut cmds, 9, 0.0, "lost", INK, 11.0, Fit::Start);
        assert_eq!(texts(&cmds)[0].1, "");
    }

    #[test]
    fn a_regular_weight_header_is_fitted_at_regular_weight() {
        // The point of the parameter is that the *fitting* follows the weight,
        // not just the drawing: a heading measured at the wrong weight is cut
        // in the wrong place.
        const NARROW: &[Column] = &[Column {
            label: "Remote Address",
            width: 46.0,
        }];
        let table = Table::new(NARROW, 0.0);
        let mut bold = Vec::new();
        table.header(&mut bold, 0.0, INK, 11.0);
        let mut regular = Vec::new();
        table.header_weighted(&mut regular, 0.0, INK, 11.0, FontWeightHint::Regular);

        let (bx, btext, bsize, bweight) = texts(&bold)[0].clone();
        let (rx, rtext, rsize, rweight) = texts(&regular)[0].clone();
        assert_eq!(bweight, FontWeightHint::Bold);
        assert_eq!(rweight, FontWeightHint::Regular);
        for (x, t, size, weight) in [
            (bx, btext.clone(), bsize, bweight),
            (rx, rtext.clone(), rsize, rweight),
        ] {
            let drawn = x + text::measure(&t, size, weight);
            assert!(
                drawn <= table.right(0) + 0.01,
                "heading {t:?} at {weight:?} draws to {drawn}, past {}",
                table.right(0)
            );
        }
        // Regular glyphs are narrower, so more of the label survives.
        assert!(
            rtext.chars().count() >= btext.chars().count(),
            "regular {rtext:?} kept less than bold {btext:?}"
        );
    }

    #[test]
    fn an_empty_table_has_no_width_and_draws_nothing() {
        let table = Table::new(&[], 30.0);
        assert!(table.is_empty());
        assert!((table.total_width() - 0.0).abs() < f32::EPSILON);
        let mut cmds = Vec::new();
        table.header(&mut cmds, 0.0, INK, 11.0);
        assert!(cmds.is_empty());
    }

    #[test]
    fn total_width_spans_from_the_anchor_past_the_last_column() {
        let table = Table::new(COLUMNS, 5.0);
        // gap + 100 + gap + 60 + gap + 40 + gap
        assert!((table.total_width() - 232.0).abs() < 0.01);
    }
}
