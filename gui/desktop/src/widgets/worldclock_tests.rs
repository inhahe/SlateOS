//! Tests for the world clock widget: where its clocks go, what it says of
//! those that do not fit or of none at all, and what it draws them in.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_cmp
)]

use super::*;
use appearance::palette_check::assert_drawn_from;

/// A veil no default gives a widget, so no wash is right by accident.
const ODD_ALPHA: u8 = 137;

/// `n` other clocks, each a reading with words in every field.
fn clocks(n: usize) -> Vec<WorldClockReading> {
    (0..n)
        .map(|i| WorldClockReading {
            label: format!("City {i}"),
            time: format!("{:02}:30", 9 + i),
            day: if i == 1 { Some("Tomorrow") } else { None },
            offset: format!("{} hours ahead", i + 1),
        })
        .collect()
}

/// A wide widget's content, one cell high.
fn wide() -> Rect {
    Rect::new(100.0, 200.0, 356.0, 96.0)
}

/// A large widget's content.
fn large() -> Rect {
    Rect::new(100.0, 200.0, 356.0, 258.0)
}

/// **The clocks that fit are listed top to bottom, inside the content**;
/// with every one in, nothing more is said.
#[test]
fn the_clocks_are_listed_in_the_content() {
    let shown = clocks(4);
    let laid = rows(&shown, large());
    let indices: Vec<usize> = laid.rows.iter().map(|(i, _)| *i).collect();
    assert_eq!(indices, [0, 1, 2, 3]);
    assert_eq!(laid.more, None);
    let content = large();
    let mut bottom = content.y;
    for (i, b) in &laid.rows {
        assert!(b.y >= bottom, "clock {i} overlaps the one above");
        assert!(
            b.y + b.h <= content.y + content.h,
            "clock {i} below the content"
        );
        bottom = b.y + b.h;
    }
}

/// **What does not fit is counted on a last line, inside the content.**
#[test]
fn clocks_that_do_not_fit_are_counted() {
    let shown = clocks(4);
    let content = wide();
    let laid = rows(&shown, content);
    assert!(!laid.rows.is_empty(), "no clock fitted a wide widget");
    let (more, at) = laid.more.expect("four clocks fitted one cell's height");
    assert_eq!(more, 4 - laid.rows.len());
    assert!(
        at.y + at.h <= content.y + content.h,
        "the count is below the content"
    );
    let (_, last) = laid.rows.last().unwrap();
    assert!(last.y + last.h <= at.y, "the count overlaps the last clock");
    assert_eq!(more_text(2), "2 more");
}

/// **With no other clock it says where they are added**, in words.
#[test]
fn no_clocks_says_where_to_add_them() {
    let p = Palette::for_mode(false);
    let mut commands = Vec::new();
    render(&[], large(), &p, 255, &mut commands);
    let said: Vec<&str> = commands
        .iter()
        .filter_map(|c| match c {
            RenderCommand::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(said, [NO_CLOCKS]);
}

/// **Each clock is drawn in its row**: its label, its time and its detail
/// line, inside the row's box.
#[test]
fn each_clock_is_drawn_in_its_row() {
    let p = Palette::for_mode(false);
    let shown = clocks(2);
    let mut commands = Vec::new();
    render(&shown, large(), &p, 255, &mut commands);
    let laid = rows(&shown, large());
    let texts: Vec<(&str, f32)> = commands
        .iter()
        .filter_map(|c| match c {
            RenderCommand::Text { text, y, .. } => Some((text.as_str(), *y)),
            _ => None,
        })
        .collect();
    for (index, bounds) in &laid.rows {
        let clock = &shown[*index];
        for said in [clock.label.clone(), clock.time.clone(), clock.detail()] {
            let (_, y) = texts
                .iter()
                .find(|(t, _)| *t == said)
                .unwrap_or_else(|| panic!("{said:?} is not drawn"));
            assert!(
                *y >= bounds.y && *y < bounds.y + bounds.h,
                "{said:?} at {y}, outside its row {bounds:?}"
            );
        }
    }
    assert!(texts.iter().any(|(t, _)| *t == "Tomorrow, 2 hours ahead"));
}

/// **Everything it draws is its palette's**, in both modes, at any veil.
#[test]
fn every_colour_it_draws_comes_from_its_palette() {
    for light in [false, true] {
        let mut p = Palette::for_mode(light);
        p.accent = appearance::PINK;
        for shown in [clocks(4), Vec::new()] {
            for content in [large(), wide()] {
                let mut commands = Vec::new();
                render(&shown, content, &p, ODD_ALPHA, &mut commands);
                assert_drawn_from(
                    &p,
                    &commands,
                    &[],
                    &format!("the world clock (light={light})"),
                );
            }
        }
    }
}
