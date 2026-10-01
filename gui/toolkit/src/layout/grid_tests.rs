//! Tests for the grid layout: placement, track sizes, the grid in its
//! container and items in their cells -- each against numbers worked out by
//! hand from CSS Grid's rules.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_cmp
)]

use super::*;

/// An item `w` by `h`, placed by the grid.
fn auto(w: f32, h: f32) -> (Size, GridItem) {
    (Size::new(w, h), GridItem::default())
}

/// An item `w` by `h` at `row`, `column`.
fn at(row: usize, column: usize, w: f32, h: f32) -> (Size, GridItem) {
    (Size::new(w, h), GridItem::at(row, column))
}

fn lay(container: Size, grid: &GridLayout, items: &[(Size, GridItem)]) -> GridPlacement {
    grid_layout(container, grid, items, &Edges::ZERO)
}

/// `(row, column)` of every item.
fn cells(p: &GridPlacement) -> Vec<(usize, usize)> {
    p.areas.iter().map(|a| (a.row, a.column)).collect()
}

fn sizes(spans: &[TrackSpan]) -> Vec<f32> {
    spans.iter().map(|s| s.size).collect()
}

fn starts(spans: &[TrackSpan]) -> Vec<f32> {
    spans.iter().map(|s| s.start).collect()
}

fn close(a: &[f32], b: &[f32]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() < 0.01)
}

/// **Items fill the cells row by row, rows as tall as their tallest item,
/// and each item fills its cell** -- the gaps between, never outside.
#[test]
fn items_fill_the_cells_row_by_row() {
    let grid = GridLayout {
        columns: vec![Track::Px(100.0), Track::Px(50.0)],
        column_gap: 5.0,
        row_gap: 2.0,
        ..GridLayout::default()
    };
    let p = lay(
        Size::new(400.0, 300.0),
        &grid,
        &[
            auto(30.0, 20.0),
            auto(40.0, 10.0),
            auto(20.0, 30.0),
            auto(10.0, 10.0),
        ],
    );
    assert_eq!(cells(&p), [(0, 0), (0, 1), (1, 0), (1, 1)]);
    assert_eq!(sizes(&p.rows), [20.0, 30.0]);
    assert_eq!(starts(&p.columns), [0.0, 105.0]);
    assert_eq!(starts(&p.rows), [0.0, 22.0]);
    let b = &p.boxes;
    assert_eq!(
        (b[0].x, b[0].y, b[0].width, b[0].height),
        (0.0, 0.0, 100.0, 20.0)
    );
    assert_eq!(
        (b[1].x, b[1].y, b[1].width, b[1].height),
        (105.0, 0.0, 50.0, 20.0)
    );
    assert_eq!(
        (b[2].x, b[2].y, b[2].width, b[2].height),
        (0.0, 22.0, 100.0, 30.0)
    );
    assert_eq!(
        (b[3].x, b[3].y, b[3].width, b[3].height),
        (105.0, 22.0, 50.0, 30.0)
    );
}

/// **Shares divide the room the other tracks leave**, in proportion:
/// 400 less a 100 column and two 10 gaps leaves 280, a third and two
/// thirds.
#[test]
fn shares_divide_the_room_left() {
    let grid = GridLayout {
        columns: vec![Track::Px(100.0), Track::Fr(1.0), Track::Fr(2.0)],
        column_gap: 10.0,
        ..GridLayout::default()
    };
    let p = lay(Size::new(400.0, 100.0), &grid, &[auto(10.0, 10.0)]);
    assert!(close(
        &sizes(&p.columns),
        &[100.0, 280.0 / 3.0, 560.0 / 3.0]
    ));
    assert!(close(
        &starts(&p.columns),
        &[0.0, 110.0, 120.0 + 280.0 / 3.0]
    ));
}

/// **A share is never smaller than the largest `min` of its items**: one
/// too small is fixed at it, and the others share the rest.
#[test]
fn a_share_is_never_below_its_items_minimum() {
    let grid = GridLayout {
        columns: vec![Track::Fr(1.0), Track::Fr(1.0)],
        ..GridLayout::default()
    };
    let mut wide = auto(10.0, 10.0);
    wide.1.min = Size::new(200.0, 0.0);
    let p = lay(Size::new(300.0, 100.0), &grid, &[wide, auto(10.0, 10.0)]);
    assert_eq!(sizes(&p.columns), [200.0, 100.0]);
}

/// **Shares summing to less than one take only that part of the room**:
/// a lone half share is half.
#[test]
fn shares_under_one_take_part_of_the_room() {
    let grid = GridLayout {
        columns: vec![Track::Fr(0.5)],
        ..GridLayout::default()
    };
    let p = lay(Size::new(300.0, 100.0), &grid, &[auto(10.0, 10.0)]);
    assert_eq!(sizes(&p.columns), [150.0]);
}

/// **Content-sized columns are as wide as their widest item alone, and an
/// item spanning them shares what they lack equally.**
#[test]
fn auto_columns_fit_their_items_and_spanning_items_share() {
    let grid = GridLayout {
        columns: vec![Track::Auto, Track::Auto],
        ..GridLayout::default()
    };
    let p = lay(
        Size::new(400.0, 300.0),
        &grid,
        &[
            at(0, 0, 50.0, 10.0),
            at(1, 1, 30.0, 10.0),
            (Size::new(120.0, 10.0), GridItem::at(2, 0).spanning(1, 2)),
        ],
    );
    // 120 wanted across 80: 40 short, 20 each.
    assert_eq!(sizes(&p.columns), [70.0, 50.0]);
    // The spanning item covers both.
    assert_eq!(p.boxes[2].width, 120.0);
}

/// **A `MinMax` track is its content's size held between its limits.**
#[test]
fn minmax_holds_the_content_between_its_limits() {
    let grid = GridLayout {
        columns: vec![
            Track::MinMax {
                min: 40.0,
                max: 60.0,
            },
            Track::MinMax {
                min: 40.0,
                max: 60.0,
            },
        ],
        ..GridLayout::default()
    };
    let p = lay(
        Size::new(400.0, 100.0),
        &grid,
        &[auto(100.0, 10.0), auto(10.0, 10.0)],
    );
    assert_eq!(sizes(&p.columns), [60.0, 40.0]);
}

/// **An item placed by its row and column goes there, and the grid fills
/// the other cells around it.**
#[test]
fn placed_items_are_filled_around() {
    let grid = GridLayout {
        columns: vec![Track::Px(10.0), Track::Px(10.0)],
        ..GridLayout::default()
    };
    let p = lay(
        Size::new(100.0, 100.0),
        &grid,
        &[
            auto(1.0, 1.0),
            at(0, 1, 1.0, 1.0),
            auto(1.0, 1.0),
            auto(1.0, 1.0),
        ],
    );
    assert_eq!(cells(&p), [(0, 0), (0, 1), (1, 0), (1, 1)]);

    // An item with a row alone takes the first column free in it.
    let mut row_only = auto(1.0, 1.0);
    row_only.1.row = Some(1);
    let p = lay(
        Size::new(100.0, 100.0),
        &grid,
        &[at(1, 0, 1.0, 1.0), row_only],
    );
    assert_eq!(cells(&p), [(1, 0), (1, 1)]);

    // An item with a column alone behind the cursor goes a row down.
    let mut column_only = auto(1.0, 1.0);
    column_only.1.column = Some(0);
    let p = lay(
        Size::new(100.0, 100.0),
        &grid,
        &[auto(1.0, 1.0), auto(1.0, 1.0), column_only],
    );
    assert_eq!(cells(&p), [(0, 0), (0, 1), (1, 0)]);
}

/// **An item too wide for the rest of a row starts the next one**, leaving
/// the cells it skipped empty (sparse placement, as CSS's default).
#[test]
fn a_span_that_does_not_fit_starts_the_next_row() {
    let grid = GridLayout {
        columns: vec![Track::Px(10.0); 3],
        ..GridLayout::default()
    };
    let p = lay(
        Size::new(100.0, 100.0),
        &grid,
        &[
            auto(1.0, 1.0),
            auto(1.0, 1.0),
            (Size::new(1.0, 1.0), GridItem::default().spanning(1, 2)),
            auto(1.0, 1.0),
        ],
    );
    assert_eq!(cells(&p), [(0, 0), (0, 1), (1, 0), (1, 2)]);
}

/// **Rows and columns past those given are added as items need them**:
/// rows `auto_rows` high, and a column for an item placed past the last.
#[test]
fn tracks_are_added_as_items_need_them() {
    let grid = GridLayout {
        columns: vec![Track::Px(10.0)],
        rows: vec![Track::Px(5.0)],
        auto_rows: Track::Px(30.0),
        auto_columns: Track::Px(20.0),
        ..GridLayout::default()
    };
    // The item in column 2 makes the grid three wide, so the two the grid
    // places fill row 0 beside it and the fourth starts row 1.
    let p = lay(
        Size::new(100.0, 100.0),
        &grid,
        &[
            auto(1.0, 1.0),
            auto(1.0, 1.0),
            at(0, 2, 1.0, 1.0),
            auto(1.0, 1.0),
        ],
    );
    assert_eq!(cells(&p), [(0, 0), (0, 1), (0, 2), (1, 0)]);
    assert_eq!(sizes(&p.rows), [5.0, 30.0]);
    assert_eq!(sizes(&p.columns), [10.0, 20.0, 20.0]);

    // A span wider than the grid widens it.
    let p = lay(
        Size::new(100.0, 100.0),
        &grid,
        &[(Size::new(1.0, 1.0), GridItem::default().spanning(1, 3))],
    );
    assert_eq!(p.columns.len(), 3);
}

/// **An item sits in its cell as told**: at its own size at the start,
/// middle or end, or filling it -- its own alignment over the grid's, and
/// stretching stopping at its `max`.
#[test]
fn items_sit_in_their_cells_as_told() {
    let grid = GridLayout {
        columns: vec![Track::Px(100.0)],
        rows: vec![Track::Px(50.0)],
        justify_items: GridAlign::Center,
        align_items: GridAlign::End,
        ..GridLayout::default()
    };
    let p = lay(Size::new(400.0, 300.0), &grid, &[auto(20.0, 10.0)]);
    let b = &p.boxes[0];
    assert_eq!((b.x, b.y, b.width, b.height), (40.0, 40.0, 20.0, 10.0));

    let mut own = auto(20.0, 10.0);
    own.1.justify_self = Some(GridAlign::End);
    own.1.align_self = Some(GridAlign::Start);
    let p = lay(Size::new(400.0, 300.0), &grid, &[own]);
    let b = &p.boxes[0];
    assert_eq!((b.x, b.y), (80.0, 0.0));

    let mut capped = auto(20.0, 10.0);
    capped.1.justify_self = Some(GridAlign::Stretch);
    capped.1.max = Size::new(70.0, f32::INFINITY);
    let p = lay(Size::new(400.0, 300.0), &grid, &[capped]);
    assert_eq!(p.boxes[0].width, 70.0);
}

/// **Columns narrower than the container sit where `justify_content` says,
/// and `Stretch` gives the room to the content-sized ones.**
#[test]
fn the_grid_sits_in_its_container_as_told() {
    let lay_with = |justify_content, columns: Vec<Track>| {
        let grid = GridLayout {
            columns,
            justify_content,
            ..GridLayout::default()
        };
        lay(
            Size::new(300.0, 100.0),
            &grid,
            &[auto(50.0, 10.0), auto(50.0, 10.0)],
        )
    };
    let px = || vec![Track::Px(50.0), Track::Px(50.0)];
    assert_eq!(
        starts(&lay_with(AlignContent::Center, px()).columns),
        [100.0, 150.0]
    );
    assert_eq!(
        starts(&lay_with(AlignContent::End, px()).columns),
        [200.0, 250.0]
    );
    assert_eq!(
        starts(&lay_with(AlignContent::SpaceBetween, px()).columns),
        [0.0, 250.0]
    );
    // Stretch: the 200 spare goes to the content-sized columns, 100 each.
    let p = lay_with(AlignContent::Stretch, vec![Track::Auto, Track::Auto]);
    assert_eq!(sizes(&p.columns), [150.0, 150.0]);
    // ...and a fixed column takes none of it.
    let p = lay_with(AlignContent::Stretch, vec![Track::Px(50.0), Track::Auto]);
    assert_eq!(sizes(&p.columns), [50.0, 250.0]);
}

/// **In a container with no width a share is as big as its content**, as
/// there is no room to share.
#[test]
fn an_unbounded_grid_sizes_shares_by_content() {
    let grid = GridLayout {
        columns: vec![Track::Fr(1.0), Track::Fr(1.0)],
        ..GridLayout::default()
    };
    let p = lay(
        Size::new(f32::INFINITY, f32::INFINITY),
        &grid,
        &[auto(40.0, 10.0), auto(70.0, 10.0)],
    );
    assert_eq!(sizes(&p.columns), [40.0, 70.0]);
    assert!(
        p.boxes
            .iter()
            .all(|b| b.width.is_finite() && b.height.is_finite())
    );
}

/// **Padding moves the grid in, and an item's margins are kept inside its
/// cell**: its box is its margin box, its border box where `content_x`
/// says.
#[test]
fn padding_and_margins() {
    let grid = GridLayout {
        columns: vec![Track::Px(100.0)],
        rows: vec![Track::Px(40.0)],
        ..GridLayout::default()
    };
    let mut item = auto(10.0, 10.0);
    item.1.margin = Edges {
        top: 2.0,
        right: 4.0,
        bottom: 6.0,
        left: 8.0,
    };
    let padding = Edges {
        top: 5.0,
        right: 5.0,
        bottom: 5.0,
        left: 10.0,
    };
    let p = grid_layout(Size::new(300.0, 200.0), &grid, &[item], &padding);
    let b = &p.boxes[0];
    assert_eq!((b.x, b.y), (10.0, 5.0));
    assert_eq!((b.content_x(), b.content_y()), (18.0, 7.0));
    assert_eq!((b.width, b.height), (88.0, 32.0));
    assert_eq!(starts(&p.columns), [10.0]);
}

/// **Two items told the same cell both go there**, as CSS lets them --
/// and an item the grid places takes the next cell free, not theirs.
#[test]
fn items_told_the_same_cell_share_it() {
    let grid = GridLayout {
        columns: vec![Track::Px(10.0), Track::Px(10.0)],
        ..GridLayout::default()
    };
    let p = lay(
        Size::new(100.0, 100.0),
        &grid,
        &[at(0, 0, 1.0, 1.0), at(0, 0, 1.0, 1.0), auto(1.0, 1.0)],
    );
    assert_eq!(cells(&p), [(0, 0), (0, 0), (0, 1)]);
}

/// **A span of nought is a span of one**, rather than an item with no
/// cells.
#[test]
fn a_span_of_nought_is_one() {
    let grid = GridLayout {
        columns: vec![Track::Px(10.0), Track::Px(10.0)],
        ..GridLayout::default()
    };
    let p = lay(
        Size::new(100.0, 100.0),
        &grid,
        &[(Size::new(1.0, 1.0), GridItem::default().spanning(0, 0))],
    );
    assert_eq!(
        p.areas[0],
        GridArea {
            row: 0,
            column: 0,
            row_span: 1,
            column_span: 1
        }
    );
    assert_eq!(p.boxes[0].width, 10.0);
}
