#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_cmp
)]

use super::*;
use crate::richinput::RichDoc;

fn palette() -> Palette {
    Palette::for_mode(false)
}

/// **The buttons stand in a row, in order, apart**, and a press finds the
/// one it lands on.
#[test]
fn the_buttons_stand_in_a_row_and_are_found() {
    let p = palette();
    let laid = layout(&p, (10.0, 20.0));
    assert_eq!(laid.len(), TOOLS.len());
    for pair in laid.windows(2) {
        let (_, (x0, y0, w0, _)) = pair[0];
        let (_, (x1, y1, _, _)) = pair[1];
        assert!(x1 >= x0 + w0, "apart");
        assert_eq!(y0, y1, "in a row");
    }
    for (tool, (x, y, w, h)) in &laid {
        assert_eq!(
            hit(&p, (10.0, 20.0), (x + w / 2.0, y + h / 2.0)),
            Some(*tool)
        );
    }
    assert_eq!(hit(&p, (10.0, 20.0), (0.0, 0.0)), None);
}

/// **A switch's button sets it over the selection, and shows it pressed
/// where all of the selection has it.**
#[test]
fn a_switch_sets_and_shows_its_format() {
    let mut input = RichInput::with_doc(RichDoc::plain("hello", Format::default()));
    input.select_all();
    assert!(!input.shown_has(Toggle::Bold));
    apply(&mut input, Tool::Switch(Toggle::Bold), 13.0);
    assert!(input.doc().format_at(0).bold);
    assert!(input.shown_has(Toggle::Bold), "pressed now");
    apply(&mut input, Tool::Switch(Toggle::Bold), 13.0);
    assert!(!input.doc().format_at(0).bold, "and off again");
}

/// **The size buttons step through the sizes**, from the field's own where
/// the text gives none, and stop at either end.
#[test]
fn the_size_buttons_step_through_the_sizes() {
    let mut input = RichInput::with_doc(RichDoc::plain("x", Format::default()));
    input.select_all();
    apply(&mut input, Tool::Larger, 13.0);
    assert_eq!(input.doc().format_at(0).size, Some(15.0));
    apply(&mut input, Tool::Smaller, 13.0);
    apply(&mut input, Tool::Smaller, 13.0);
    assert_eq!(input.doc().format_at(0).size, Some(12.0));
    for _ in 0..20 {
        apply(&mut input, Tool::Smaller, 13.0);
    }
    assert_eq!(
        input.doc().format_at(0).size,
        Some(SIZES[0]),
        "the smallest"
    );
    apply(&mut input, Tool::Clear, 13.0);
    assert_eq!(input.doc().format_at(0), Format::default());
}

/// **A pressed switch is drawn otherwise than one not pressed.**
#[test]
fn a_pressed_switch_is_drawn_pressed() {
    let p = palette();
    let mut input = RichInput::new();
    let drawn = |input: &RichInput| {
        let mut tree = RenderTree::new();
        draw(&mut tree, &p, input, (0.0, 0.0), p.base, None);
        tree.commands
    };
    let plain = drawn(&input);
    input.toggle(Toggle::Bold);
    assert_ne!(drawn(&input), plain, "bold's button pressed");
}
