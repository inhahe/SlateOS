//! Grid layout: items in the cells of rows and columns -- CSS Grid's model,
//! as native code over sizes, for forms, dashboards and anything whose
//! parts line up both ways.
//!
//! A grid ([`GridLayout`]) has its columns and rows ([`Track`]: so many
//! pixels, as big as what is in it, or a share of the room left), the gaps
//! between them, and how items sit in their cells. Each item
//! ([`GridItem`]) says which cell it starts in -- or nothing, to be placed
//! in the next free one -- how many columns and rows it spans, how it sits
//! in its cell, and the least and most it may be. [`grid_layout`] answers a
//! box per item, and where every column and row ended up.
//!
//! What it does follows the specification's order (CSS Grid §8 and §11),
//! for what a desktop UI uses:
//!
//! 1. **Placement.** Items with both a row and a column go there; items
//!    with a row alone take the first columns free in it; the rest are
//!    placed row by row in the first cells free -- an item with a column
//!    alone going to that column, a row further down if it must. Rows past
//!    those given are added as the items need them, `auto_rows` high, and
//!    columns likewise (`auto_columns`).
//! 2. **Track sizes.** A [`Track::Px`] is its size. A [`Track::Auto`] is as
//!    big as the biggest item in it alone, and an item spanning several
//!    content-sized tracks shares what they lack among them. A
//!    [`Track::Fr`] takes its share of the room the others leave, never
//!    less than the largest `min` of the items in it: a share too small for
//!    that is fixed at it, and the rest re-shared. With no room to share --
//!    an unbounded container -- a share is as big as its content.
//! 3. **The grid in the container.** Room the tracks leave goes where
//!    `justify_content` and `align_content` say, `Stretch` giving it to the
//!    content-sized tracks.
//! 4. **Items in their cells**, `Stretch` filling the cell within the item's
//!    `max`, or `Start`, `Center` and `End` at the item's own size.
//!
//! What it leaves out: named lines and areas, dense packing, `minmax` with
//! a share as its minimum, and items placed by negative line numbers.

use super::{AlignContent, LayoutBox, Size};
use crate::style::Edges;

/// How big a column or a row is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Track {
    /// This many pixels, whatever is in it.
    Px(f32),
    /// As big as the biggest item in it (CSS's `auto`, at the content's
    /// size).
    Auto,
    /// A share of the room the other tracks leave: `Fr(2.0)` has twice
    /// `Fr(1.0)`'s (CSS's `fr`). Never smaller than the largest `min` of the
    /// items in it.
    Fr(f32),
    /// As big as what is in it, held between `min` and `max` pixels (CSS's
    /// `minmax(min, max)` with lengths).
    MinMax {
        /// The least it is.
        min: f32,
        /// The most it is.
        max: f32,
    },
}

impl Track {
    /// Whether its size comes from what is in it.
    fn is_content_sized(self) -> bool {
        matches!(self, Self::Auto | Self::MinMax { .. })
    }

    /// The share it takes, if it is one.
    fn share(self) -> Option<f32> {
        match self {
            Self::Fr(n) => Some(n.max(0.0)),
            _ => None,
        }
    }

    /// The most a content-sized track may be.
    fn limit(self) -> f32 {
        match self {
            Self::MinMax { max, .. } => max.max(0.0),
            Self::Px(px) => px.max(0.0),
            Self::Auto | Self::Fr(_) => f32::INFINITY,
        }
    }

    /// The size it starts at, before anything in it is measured.
    fn initial(self) -> f32 {
        match self {
            Self::Px(px) => px.max(0.0),
            Self::MinMax { min, .. } => min.max(0.0),
            Self::Auto | Self::Fr(_) => 0.0,
        }
    }
}

/// Where an item sits in its cell, along one axis.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GridAlign {
    /// At the cell's start, its own size.
    Start,
    /// At the cell's end.
    End,
    /// In the cell's middle.
    Center,
    /// Filling the cell, within its `max`.
    #[default]
    Stretch,
}

/// A grid's columns and rows, and how its items sit.
#[derive(Clone, Debug)]
pub struct GridLayout {
    /// The columns, left to right.
    pub columns: Vec<Track>,
    /// The rows, top to bottom.
    pub rows: Vec<Track>,
    /// Columns added past `columns` when an item needs them.
    pub auto_columns: Track,
    /// Rows added past `rows` when items need them -- which they do in a
    /// grid that gives no rows at all and lets its items fill it.
    pub auto_rows: Track,
    /// The space between columns.
    pub column_gap: f32,
    /// The space between rows.
    pub row_gap: f32,
    /// Where items sit across their cells, unless they say.
    pub justify_items: GridAlign,
    /// Where items sit down their cells, unless they say.
    pub align_items: GridAlign,
    /// Where the columns sit when they are narrower than the container.
    pub justify_content: AlignContent,
    /// Where the rows sit when they are shorter than it.
    pub align_content: AlignContent,
}

impl Default for GridLayout {
    fn default() -> Self {
        Self {
            columns: Vec::new(),
            rows: Vec::new(),
            auto_columns: Track::Auto,
            auto_rows: Track::Auto,
            column_gap: 0.0,
            row_gap: 0.0,
            justify_items: GridAlign::Stretch,
            align_items: GridAlign::Stretch,
            justify_content: AlignContent::Start,
            align_content: AlignContent::Start,
        }
    }
}

/// One item of a grid: where it goes, what it spans, how it sits.
#[derive(Clone, Debug)]
pub struct GridItem {
    /// The column it starts in, from 0; `None` lets the grid place it.
    pub column: Option<usize>,
    /// The row it starts in, from 0; `None` lets the grid place it.
    pub row: Option<usize>,
    /// How many columns it covers; 0 is taken as one.
    pub column_span: usize,
    /// How many rows it covers; 0 is taken as one.
    pub row_span: usize,
    /// Where it sits across its cell; `None` is the grid's `justify_items`.
    pub justify_self: Option<GridAlign>,
    /// Where it sits down its cell; `None` is the grid's `align_items`.
    pub align_self: Option<GridAlign>,
    /// The least it may be.
    pub min: Size,
    /// The most it may be; `f32::INFINITY` is no limit.
    pub max: Size,
    /// Room kept clear around it, inside its cell.
    pub margin: Edges,
}

impl Default for GridItem {
    fn default() -> Self {
        Self {
            column: None,
            row: None,
            column_span: 1,
            row_span: 1,
            justify_self: None,
            align_self: None,
            min: Size::ZERO,
            max: Size::new(f32::INFINITY, f32::INFINITY),
            margin: Edges::ZERO,
        }
    }
}

impl GridItem {
    /// An item in the cell at `row`, `column`.
    #[must_use]
    pub fn at(row: usize, column: usize) -> Self {
        Self {
            row: Some(row),
            column: Some(column),
            ..Self::default()
        }
    }

    /// The same item, covering `rows` rows and `columns` columns.
    #[must_use]
    pub fn spanning(self, rows: usize, columns: usize) -> Self {
        Self {
            row_span: rows,
            column_span: columns,
            ..self
        }
    }
}

/// Where a column or a row ended up: its start in the container's space
/// (inside the padding), and its size.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TrackSpan {
    /// Where it starts.
    pub start: f32,
    /// How big it is.
    pub size: f32,
}

/// The cells an item was given.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GridArea {
    /// The row it starts in.
    pub row: usize,
    /// The column it starts in.
    pub column: usize,
    /// How many rows it covers.
    pub row_span: usize,
    /// How many columns.
    pub column_span: usize,
}

/// What [`grid_layout`] answers.
#[derive(Clone, Debug, Default)]
pub struct GridPlacement {
    /// A box per item, in the order given: its margin box's corner and its
    /// size, its margin the item's (as [`super::flex_layout`]'s are).
    pub boxes: Vec<LayoutBox>,
    /// The cells each item was given, in the same order.
    pub areas: Vec<GridArea>,
    /// Where each column ended up, the ones added for items included.
    pub columns: Vec<TrackSpan>,
    /// Where each row ended up.
    pub rows: Vec<TrackSpan>,
}

/// Lay `children` out in a grid `container_size` big, inside its `padding`.
/// Each child is its content size and its [`GridItem`]; see the module
/// documentation for what is done with them, in order.
#[must_use]
pub fn grid_layout(
    container_size: Size,
    grid: &GridLayout,
    children: &[(Size, GridItem)],
    padding: &Edges,
) -> GridPlacement {
    let avail_w = (container_size.width - padding.horizontal()).max(0.0);
    let avail_h = (container_size.height - padding.vertical()).max(0.0);
    let column_gap = grid.column_gap.max(0.0);
    let row_gap = grid.row_gap.max(0.0);

    // 1. Placement, and the tracks it needs.
    let areas = place(grid, children);
    let column_count = areas
        .iter()
        .map(|a| a.column.saturating_add(a.column_span))
        .max()
        .unwrap_or(0)
        .max(grid.columns.len());
    let row_count = areas
        .iter()
        .map(|a| a.row.saturating_add(a.row_span))
        .max()
        .unwrap_or(0)
        .max(grid.rows.len());
    let column_tracks: Vec<Track> = (0..column_count)
        .map(|i| grid.columns.get(i).copied().unwrap_or(grid.auto_columns))
        .collect();
    let row_tracks: Vec<Track> = (0..row_count)
        .map(|i| grid.rows.get(i).copied().unwrap_or(grid.auto_rows))
        .collect();

    // 2. Track sizes, columns first, then rows.
    let wanted = |(size, item): &(Size, GridItem), across: bool| {
        if across {
            hold(size.width, item.min.width, item.max.width) + item.margin.horizontal()
        } else {
            hold(size.height, item.min.height, item.max.height) + item.margin.vertical()
        }
    };
    let floor = |(_, item): &(Size, GridItem), across: bool| {
        if across {
            item.min.width + item.margin.horizontal()
        } else {
            item.min.height + item.margin.vertical()
        }
    };
    let column_items: Vec<Span> = areas
        .iter()
        .zip(children)
        .map(|(a, child)| Span {
            start: a.column,
            count: a.column_span,
            wants: wanted(child, true),
            floor: floor(child, true),
        })
        .collect();
    let row_items: Vec<Span> = areas
        .iter()
        .zip(children)
        .map(|(a, child)| Span {
            start: a.row,
            count: a.row_span,
            wants: wanted(child, false),
            floor: floor(child, false),
        })
        .collect();
    let mut columns = size_tracks(&column_tracks, &column_items, avail_w, column_gap);
    let mut rows = size_tracks(&row_tracks, &row_items, avail_h, row_gap);

    // 3. The tracks in the container.
    let column_starts = distribute(
        &mut columns,
        &column_tracks,
        avail_w,
        column_gap,
        grid.justify_content,
    );
    let row_starts = distribute(&mut rows, &row_tracks, avail_h, row_gap, grid.align_content);
    let column_spans: Vec<TrackSpan> = column_starts
        .iter()
        .zip(&columns)
        .map(|(&start, &size)| TrackSpan {
            start: start + padding.left,
            size,
        })
        .collect();
    let row_spans: Vec<TrackSpan> = row_starts
        .iter()
        .zip(&rows)
        .map(|(&start, &size)| TrackSpan {
            start: start + padding.top,
            size,
        })
        .collect();

    // 4. Each item in its cells.
    let boxes = areas
        .iter()
        .zip(children)
        .map(|(a, (size, item))| {
            let (cx, cw) = cell(&column_spans, a.column, a.column_span);
            let (cy, ch) = cell(&row_spans, a.row, a.row_span);
            let m = &item.margin;
            let (x, w) = sit(
                cx + m.left,
                cw - m.horizontal(),
                size.width,
                item.min.width,
                item.max.width,
                item.justify_self.unwrap_or(grid.justify_items),
            );
            let (y, h) = sit(
                cy + m.top,
                ch - m.vertical(),
                size.height,
                item.min.height,
                item.max.height,
                item.align_self.unwrap_or(grid.align_items),
            );
            LayoutBox {
                x: x - m.left,
                y: y - m.top,
                width: w,
                height: h,
                padding: Edges::ZERO,
                border_widths: Edges::ZERO,
                margin: *m,
            }
        })
        .collect();

    GridPlacement {
        boxes,
        areas,
        columns: column_spans,
        rows: row_spans,
    }
}

/// `value` held between `min` and `max`, the minimum winning where they
/// cross -- as [`super::flex_layout`] holds them.
fn hold(value: f32, min: f32, max: f32) -> f32 {
    value.min(max).max(min).max(0.0)
}

/// One item seen along one axis: the tracks it covers, the room it wants
/// with its margins, and the least it may have.
struct Span {
    start: usize,
    count: usize,
    wants: f32,
    floor: f32,
}

impl Span {
    fn tracks(&self) -> core::ops::Range<usize> {
        self.start..self.start.saturating_add(self.count)
    }
}

/// Give each item its cells (§8.5, sparse): fixed items first, then those
/// with only a row, then the rest in order behind a cursor moving row by
/// row. Spans of 0 count as 1. An item placed by the grid never lands on a
/// taken cell; one placed by its row and column goes there, over another
/// if told to, as in CSS.
fn place(grid: &GridLayout, children: &[(Size, GridItem)]) -> Vec<GridArea> {
    let mut taken = Occupancy::default();
    let mut areas: Vec<Option<GridArea>> = vec![None; children.len()];
    // The columns auto-placement fills across: those given, or as many as
    // the widest item or furthest-right fixed item needs.
    let width = children
        .iter()
        .map(|(_, it)| it.column.unwrap_or(0).saturating_add(it.column_span.max(1)))
        .max()
        .unwrap_or(0)
        .max(grid.columns.len())
        .max(1);
    let spans = |it: &GridItem| (it.row_span.max(1), it.column_span.max(1));

    // Both given: where they say, over whatever is there -- CSS lets fixed
    // items overlap, and so does this.
    for ((_, it), slot) in children.iter().zip(areas.iter_mut()) {
        if let (Some(row), Some(column)) = (it.row, it.column) {
            let (row_span, column_span) = spans(it);
            let area = GridArea {
                row,
                column,
                row_span,
                column_span,
            };
            taken.mark(area);
            *slot = Some(area);
        }
    }
    // A row alone: the first columns free in it.
    for ((_, it), slot) in children.iter().zip(areas.iter_mut()) {
        if let (Some(row), None) = (it.row, it.column) {
            let (row_span, column_span) = spans(it);
            // Past the furthest column taken every column is free, so the
            // search ends there at the latest.
            let end = taken.end_column();
            let column = (0..=end)
                .find(|&c| taken.free(row, c, row_span, column_span))
                .unwrap_or(end);
            let area = GridArea {
                row,
                column,
                row_span,
                column_span,
            };
            taken.mark(area);
            *slot = Some(area);
        }
    }
    // The rest, behind the cursor.
    let (mut at_row, mut at_column) = (0_usize, 0_usize);
    for ((_, it), slot) in children.iter().zip(areas.iter_mut()) {
        if slot.is_some() {
            continue;
        }
        let (row_span, column_span) = spans(it);
        let area = if let Some(column) = it.column {
            // A column alone: that column, a row down if the cursor has
            // passed it, and further down until it is free there.
            if column < at_column {
                at_row = at_row.saturating_add(1);
            }
            let end = taken.end_row().max(at_row);
            let row = (at_row..=end)
                .find(|&r| taken.free(r, column, row_span, column_span))
                .unwrap_or(end);
            GridArea {
                row,
                column,
                row_span,
                column_span,
            }
        } else {
            // Nothing given: the first cells free at or after the cursor,
            // row by row, where its columns fit across the grid.
            let fits = width.saturating_sub(column_span);
            let mut found = None;
            let mut row = at_row;
            let mut from = at_column;
            while found.is_none() {
                found = (from..=fits).find(|&c| taken.free(row, c, row_span, column_span));
                if found.is_none() {
                    row = row.saturating_add(1);
                    from = 0;
                }
            }
            GridArea {
                row,
                column: found.unwrap_or(0),
                row_span,
                column_span,
            }
        };
        taken.mark(area);
        at_row = area.row;
        at_column = area.column.saturating_add(area.column_span);
        *slot = Some(area);
    }
    areas.into_iter().map(Option::unwrap_or_default).collect()
}

/// Which cells are taken.
#[derive(Default)]
struct Occupancy {
    cells: std::collections::BTreeSet<(usize, usize)>,
}

impl Occupancy {
    fn mark(&mut self, a: GridArea) {
        for r in a.row..a.row.saturating_add(a.row_span) {
            for c in a.column..a.column.saturating_add(a.column_span) {
                self.cells.insert((r, c));
            }
        }
    }

    /// One past the furthest column taken: every column from it on is free.
    fn end_column(&self) -> usize {
        self.cells
            .iter()
            .map(|&(_, c)| c.saturating_add(1))
            .max()
            .unwrap_or(0)
    }

    /// One past the furthest row taken: every row from it on is free.
    fn end_row(&self) -> usize {
        self.cells
            .iter()
            .map(|&(r, _)| r.saturating_add(1))
            .max()
            .unwrap_or(0)
    }

    fn free(&self, row: usize, column: usize, rows: usize, columns: usize) -> bool {
        (row..row.saturating_add(rows)).all(|r| {
            (column..column.saturating_add(columns)).all(|c| !self.cells.contains(&(r, c)))
        })
    }
}

/// The sizes of one axis's tracks (§11, for the track kinds there are):
/// fixed ones as given; content-sized ones as big as their single-track
/// items, then whatever spanning items still lack shared among them; shares
/// of the room left, each at least its items' floors.
fn size_tracks(tracks: &[Track], items: &[Span], avail: f32, gap: f32) -> Vec<f32> {
    let mut sizes: Vec<f32> = tracks.iter().map(|t| t.initial()).collect();
    let unbounded = !avail.is_finite();
    // A share in an unbounded container has nothing to share: it is sized
    // by its content, as a content-sized track is.
    let content_sized = |t: Track| t.is_content_sized() || (unbounded && t.share().is_some());

    // Items in one track: the track is at least as big as each wants.
    for item in items.iter().filter(|i| i.count == 1) {
        if let (Some(&track), Some(size)) = (tracks.get(item.start), sizes.get_mut(item.start))
            && content_sized(track)
        {
            *size = size.max(item.wants.min(track.limit()));
        }
    }
    // Items across several, smallest spans first: what the tracks lack,
    // shared equally among the content-sized ones that can still grow.
    let mut spanning: Vec<&Span> = items.iter().filter(|i| i.count > 1).collect();
    spanning.sort_by_key(|i| i.count);
    for item in spanning {
        let range = item.tracks();
        #[allow(clippy::cast_precision_loss, reason = "a span of a few tracks")]
        let gaps = gap * item.count.saturating_sub(1) as f32;
        let have: f32 = sizes.get(range.clone()).map_or(0.0, |s| s.iter().sum()) + gaps;
        let mut lack = item.wants - have;
        // Shared out in rounds: a track that reaches its limit drops out.
        while lack > 0.001 {
            let growable: Vec<usize> = range
                .clone()
                .filter(|&i| {
                    tracks.get(i).is_some_and(|&t| content_sized(t))
                        && sizes
                            .get(i)
                            .zip(tracks.get(i))
                            .is_some_and(|(&s, t)| s < t.limit())
                })
                .collect();
            if growable.is_empty() {
                break;
            }
            #[allow(clippy::cast_precision_loss, reason = "a span of a few tracks")]
            let each = lack / growable.len() as f32;
            for i in growable {
                if let (Some(size), Some(track)) = (sizes.get_mut(i), tracks.get(i)) {
                    let grown = (*size + each).min(track.limit());
                    lack -= grown - *size;
                    *size = grown;
                }
            }
        }
    }

    // Shares of the room the others leave.
    if !unbounded {
        let floors: Vec<f32> = (0..tracks.len())
            .map(|t| {
                items
                    .iter()
                    .filter(|i| i.count == 1 && i.start == t)
                    .map(|i| i.floor)
                    .fold(0.0_f32, f32::max)
            })
            .collect();
        #[allow(clippy::cast_precision_loss, reason = "a grid's track count")]
        let gaps = gap * tracks.len().saturating_sub(1) as f32;
        let others: f32 = tracks
            .iter()
            .zip(&sizes)
            .filter(|(t, _)| t.share().is_none())
            .map(|(_, s)| s)
            .sum();
        let room = (avail - others - gaps).max(0.0);
        // A share whose floor is bigger than its share is fixed at its
        // floor, and the rest shared again (§11.7.1).
        let mut fixed: Vec<bool> = tracks.iter().map(|t| t.share().is_none()).collect();
        loop {
            let left = room
                - tracks
                    .iter()
                    .zip(&fixed)
                    .zip(&floors)
                    .filter(|((t, f), _)| **f && t.share().is_some())
                    .map(|(_, floor)| floor)
                    .sum::<f32>();
            let total: f32 = tracks
                .iter()
                .zip(&fixed)
                .filter(|(_, f)| !**f)
                .filter_map(|(t, _)| t.share())
                .sum();
            let unit = if total > 0.0 {
                left.max(0.0) / total.max(1.0)
            } else {
                0.0
            };
            let mut changed = false;
            for (i, track) in tracks.iter().enumerate() {
                let (Some(share), Some(&floor), Some(f)) =
                    (track.share(), floors.get(i), fixed.get_mut(i))
                else {
                    continue;
                };
                if !*f && share * unit < floor {
                    *f = true;
                    changed = true;
                }
            }
            if !changed {
                for (i, track) in tracks.iter().enumerate() {
                    if let (Some(share), Some(&floor), Some(&f), Some(size)) =
                        (track.share(), floors.get(i), fixed.get(i), sizes.get_mut(i))
                    {
                        *size = if f { floor } else { share * unit };
                    }
                }
                break;
            }
        }
    }
    sizes
}

/// Where each track starts, with the room the tracks leave placed as
/// `align` says -- `Stretch` giving it to the content-sized tracks, or
/// placing the grid at the start when there are none.
fn distribute(
    sizes: &mut [f32],
    tracks: &[Track],
    avail: f32,
    gap: f32,
    align: AlignContent,
) -> Vec<f32> {
    #[allow(clippy::cast_precision_loss, reason = "a grid's track count")]
    let count = sizes.len() as f32;
    let used = sizes.iter().sum::<f32>() + gap * (count - 1.0).max(0.0);
    let free = if avail.is_finite() { avail - used } else { 0.0 };
    let (mut at, between) = match align {
        AlignContent::Start => (0.0, 0.0),
        AlignContent::End => (free, 0.0),
        AlignContent::Center => (free / 2.0, 0.0),
        AlignContent::Stretch => {
            let stretchy: Vec<usize> = (0..sizes.len())
                .filter(|&i| tracks.get(i).is_some_and(|t| t.is_content_sized()))
                .collect();
            if free > 0.0 && !stretchy.is_empty() {
                #[allow(clippy::cast_precision_loss, reason = "a grid's track count")]
                let each = free / stretchy.len() as f32;
                for i in stretchy {
                    if let Some(s) = sizes.get_mut(i) {
                        *s += each;
                    }
                }
            }
            (0.0, 0.0)
        }
        AlignContent::SpaceBetween if free > 0.0 && count > 1.0 => (0.0, free / (count - 1.0)),
        AlignContent::SpaceBetween => (0.0, 0.0),
        AlignContent::SpaceAround if free > 0.0 => (free / count / 2.0, free / count),
        AlignContent::SpaceEvenly if free > 0.0 => (free / (count + 1.0), free / (count + 1.0)),
        AlignContent::SpaceAround | AlignContent::SpaceEvenly => (free / 2.0, 0.0),
    };
    let mut starts = Vec::with_capacity(sizes.len());
    for s in sizes.iter() {
        starts.push(at);
        at += s + gap + between;
    }
    starts
}

/// The start and size of the cells `start..start + count` of one axis:
/// the first's start to the last's end, the gaps between included.
fn cell(spans: &[TrackSpan], start: usize, count: usize) -> (f32, f32) {
    let first = spans.get(start).copied().unwrap_or_default();
    let last = spans
        .get(start.saturating_add(count.max(1)).saturating_sub(1))
        .copied()
        .unwrap_or(first);
    (first.start, (last.start + last.size - first.start).max(0.0))
}

/// Where an item `content` big sits in a cell starting at `start`, `room`
/// long once its margins are kept, and how big it is: filling it when
/// stretched (within `max`), else its own size at the cell's start, middle
/// or end.
fn sit(start: f32, room: f32, content: f32, min: f32, max: f32, align: GridAlign) -> (f32, f32) {
    let room = room.max(0.0);
    match align {
        GridAlign::Stretch => (start, hold(room, min, max)),
        GridAlign::Start => (start, hold(content, min, max)),
        GridAlign::End => {
            let size = hold(content, min, max);
            (start + room - size, size)
        }
        GridAlign::Center => {
            let size = hold(content, min, max);
            (start + (room - size) / 2.0, size)
        }
    }
}

#[cfg(test)]
#[path = "grid_tests.rs"]
mod tests;
