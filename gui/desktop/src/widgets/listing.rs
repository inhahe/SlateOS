//! A widget's list: rows of one height stacked down its content, as many as
//! fit, and -- when some do not -- a last line saying how many more.
//!
//! The world clock's and the disk usage widget's. The upcoming events, whose
//! rows are of two heights (a day's heading, an event), place their own and
//! count what did not fit in the same words ([`more_text`]). One copy of the
//! arithmetic and of the words, where there were two of each, so a list cut
//! short says so the same way on every widget.

use crate::Rect;

/// A list as laid out in a widget's content.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Rows {
    /// Each item that fits, by its place in the list, and its box.
    pub rows: Vec<(usize, Rect)>,
    /// How many did not fit, and the box of the line that says so; `None`
    /// when every one did.
    pub more: Option<(usize, Rect)>,
}

/// `count` items, each `row_height` tall and `gap` apart, placed down
/// `content` from its top: as many as fit, and -- when some do not -- a last
/// line, `more_height` tall, saying how many more, the items above it giving
/// up the room it takes.
#[must_use]
pub fn stack(count: usize, row_height: f32, gap: f32, more_height: f32, content: Rect) -> Rows {
    let place = |bottom: f32| -> Vec<(usize, Rect)> {
        let mut placed = Vec::new();
        let mut top = content.y;
        for index in 0..count {
            let gap = if index == 0 { 0.0 } else { gap };
            if top + gap + row_height > bottom {
                break;
            }
            top += gap;
            placed.push((index, Rect::new(content.x, top, content.w, row_height)));
            top += row_height;
        }
        placed
    };
    let all = place(content.y + content.h);
    if all.len() == count {
        return Rows {
            rows: all,
            more: None,
        };
    }
    let rows = place(content.y + content.h - more_height);
    let top = rows.last().map_or(content.y, |(_, b)| b.y + b.h);
    Rows {
        more: Some((
            count.saturating_sub(rows.len()),
            Rect::new(content.x, top, content.w, more_height),
        )),
        rows,
    }
}

/// What the line saying how many more did not fit says: "2 more".
#[must_use]
pub fn more_text(more: usize) -> String {
    format!("{more} more")
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        clippy::float_cmp
    )]

    use super::*;

    /// A content box 100 high.
    fn content() -> Rect {
        Rect::new(10.0, 20.0, 200.0, 100.0)
    }

    /// **Every item that fits is placed, top to bottom, a gap apart**, and
    /// with every one in nothing more is said.
    #[test]
    fn items_that_fit_are_stacked_a_gap_apart() {
        let laid = stack(3, 20.0, 5.0, 12.0, content());
        let tops: Vec<f32> = laid.rows.iter().map(|(_, b)| b.y).collect();
        assert_eq!(tops, [20.0, 45.0, 70.0]);
        assert!(laid.rows.iter().all(|(_, b)| b.x == 10.0 && b.w == 200.0));
        assert_eq!(laid.more, None);
    }

    /// **What does not fit is counted on a last line, inside the content**,
    /// the items above giving up its room: five rows of 20 fill 100 exactly,
    /// and a sixth does not fit -- nor do five with a 12-high line under
    /// them, so four are shown and two counted.
    #[test]
    fn what_does_not_fit_is_counted_under_what_does() {
        assert_eq!(stack(5, 20.0, 0.0, 12.0, content()).more, None);
        let laid = stack(6, 20.0, 0.0, 12.0, content());
        let indices: Vec<usize> = laid.rows.iter().map(|(i, _)| *i).collect();
        assert_eq!(indices, [0, 1, 2, 3]);
        let (more, at) = laid.more.unwrap();
        assert_eq!(more, 2);
        assert_eq!(at.y, 100.0);
        assert!(at.y + at.h <= 120.0, "the count is below the content");
        let seven = stack(7, 20.0, 0.0, 12.0, content());
        assert_eq!(seven.rows.len(), 4);
        assert_eq!(seven.more.unwrap().0, 3);
    }

    /// **Nothing fitting is all counted**, on a line at the content's top;
    /// and an empty list is placed as nothing, with nothing to say.
    #[test]
    fn none_fitting_is_all_counted_and_none_is_nothing() {
        let laid = stack(2, 200.0, 0.0, 12.0, content());
        assert!(laid.rows.is_empty());
        assert_eq!(laid.more, Some((2, Rect::new(10.0, 20.0, 200.0, 12.0))));
        assert_eq!(stack(0, 20.0, 5.0, 12.0, content()), Rows::default());
    }

    /// **The count is said in words.**
    #[test]
    fn the_count_is_said_in_words() {
        assert_eq!(more_text(1), "1 more");
        assert_eq!(more_text(12), "12 more");
    }
}
