//! Tests for a widget tree styled with CSS: a widget's own, a style sheet's,
//! inherited, in each state, sized and drawn.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_cmp
)]

use super::{Widget, WidgetTree};
use crate::color::Color;
use crate::event::{Event, MouseEvent, MouseEventKind};
use crate::layout::FlexDirection;
use crate::palette::Palette;
use crate::render::RenderCommand;
use crate::style::Edges;

const RED: Color = Color::rgba(255, 0, 0, 255);
const BLUE: Color = Color::rgba(0, 0, 255, 255);
const GREEN: Color = Color::rgba(0, 128, 0, 255);

/// A column of `children` in a window 400 by 300, laid out.
fn tree_of(children: Vec<Widget>) -> WidgetTree {
    let root = Widget::container()
        .with_flex_direction(FlexDirection::Column)
        .with_children(children);
    let mut tree = WidgetTree::new(root, 400.0, 300.0);
    tree.set_palette(Palette::for_mode(false));
    tree
}

fn moved_to(tree: &mut WidgetTree, x: f32, y: f32) {
    tree.handle_event(&Event::Mouse(MouseEvent {
        x,
        y,
        kind: MouseEventKind::Move,
    }));
}

/// **A widget's own CSS sets its look, and leaves its program's style as it
/// was**: what CSS starts from each time.
#[test]
fn a_widgets_own_css_sets_its_look() {
    let tree = tree_of(vec![
        Widget::label("hi").css("color: red; padding: 4px 6px"),
    ]);
    let label = &tree.root.children[0];
    assert_eq!(label.look().foreground, Some(RED));
    assert_eq!(label.look().padding, Edges::symmetric(4.0, 6.0));
    assert_eq!(label.style.foreground, None, "the program's is untouched");
    assert_eq!(
        label.layout.padding,
        Edges::symmetric(4.0, 6.0),
        "and laid out so"
    );
    assert!(tree.css_warnings().is_empty(), "{:?}", tree.css_warnings());
}

/// **A tree no CSS styles is drawn in its program's styles**: nothing is
/// computed, so nothing can differ.
#[test]
fn a_tree_without_css_is_its_programs() {
    let tree = tree_of(vec![Widget::label("hi")]);
    assert!(!tree.styled);
    assert!(tree.root.children[0].computed.is_none());
}

/// **A state's block comes and goes with the state**: the pointer over a
/// widget gives it its `:hover` style, and moving off takes it away.
#[test]
fn a_states_block_comes_and_goes_with_the_state() {
    let mut tree =
        tree_of(vec![Widget::button("Go").css(
            "background-color: red; &:hover { background-color: blue }",
        )]);
    let (x, y) = middle_of_first(&tree);
    assert_eq!(tree.root.children[0].look().background, RED);
    moved_to(&mut tree, x, y);
    assert_eq!(tree.root.children[0].look().background, BLUE, "hovered");
    moved_to(&mut tree, 399.0, 299.0);
    assert_eq!(
        tree.root.children[0].look().background,
        RED,
        "and off again"
    );
}

/// The middle of the first widget in `tree`.
fn middle_of_first(tree: &WidgetTree) -> (f32, f32) {
    let b = &tree.root.children[0].layout;
    (
        b.x + b.border_box_width() / 2.0,
        b.y + b.border_box_height() / 2.0,
    )
}

/// **A style for a state alone is taken up when the state comes** -- a
/// widget's own `&:hover` block, and a style sheet's `Button:hover`, with
/// nothing styling the widget until the pointer is over it.
#[test]
fn a_style_for_a_state_alone_is_taken_up() {
    let mut own = tree_of(vec![
        Widget::button("Go").css("&:hover { background-color: blue }"),
    ]);
    let program = own.root.children[0].style.background;
    assert_ne!(program, BLUE);
    let (x, y) = middle_of_first(&own);
    moved_to(&mut own, x, y);
    assert_eq!(own.root.children[0].look().background, BLUE, "its own");
    moved_to(&mut own, 399.0, 299.0);
    assert_eq!(own.root.children[0].look().background, program, "and off");

    let mut sheet = tree_of(vec![Widget::button("Go")]);
    assert!(
        sheet
            .set_style_sheet("Button:hover { padding: 9px }")
            .is_empty()
    );
    let (x, y) = middle_of_first(&sheet);
    moved_to(&mut sheet, x, y);
    assert_eq!(
        sheet.root.children[0].look().padding,
        Edges::all(9.0),
        "the sheet's"
    );
}

/// **Only a change of state styles a tree again**: a pointer moving within
/// one widget changes nothing a style hangs on, so the tree is not laid out
/// for it -- a change the program made to a style waits for the next
/// layout, which the pointer leaving the widget brings.
#[test]
fn only_a_change_of_state_styles_the_tree_again() {
    let mut tree = tree_of(vec![Widget::button("Go").css("&:hover { color: blue }")]);
    let (x, y) = middle_of_first(&tree);
    moved_to(&mut tree, x, y);
    assert_eq!(tree.root.children[0].look().foreground, Some(BLUE));
    let laid_out = tree.root.children[0].look().padding;
    assert_ne!(laid_out, Edges::all(9.0));
    tree.root.children[0].style.padding = Edges::all(9.0);
    moved_to(&mut tree, x + 1.0, y);
    assert_eq!(
        tree.root.children[0].look().padding,
        laid_out,
        "a move within it is not a change"
    );
    moved_to(&mut tree, 399.0, 299.0);
    assert_eq!(
        tree.root.children[0].look().padding,
        Edges::all(9.0),
        "leaving it is"
    );
}

/// **A style sheet chooses widgets by kind, class and name**, its rules in
/// the order written, then each widget's own.
#[test]
fn a_style_sheet_chooses_by_kind_class_and_name() {
    let mut tree = tree_of(vec![
        Widget::button("A"),
        Widget::button("B").class("big"),
        Widget::label("C").named("note").css("opacity: 0.25"),
    ]);
    let warnings = tree
        .set_style_sheet(
            "Button { padding: 10px } .big { padding: 20px } #note { opacity: 0.5; color: blue }",
        )
        .to_vec();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(tree.root.children[0].look().padding, Edges::all(10.0));
    assert_eq!(
        tree.root.children[1].look().padding,
        Edges::all(20.0),
        "the later rule"
    );
    let note = tree.root.children[2].look();
    assert_eq!(note.foreground, Some(BLUE));
    assert_eq!(note.opacity, 0.25, "its own, after the sheet's");
}

/// **A child inherits what a style set on its parent**, through the tree.
#[test]
fn a_child_inherits_through_the_tree() {
    let root = Widget::container()
        .css("color: rgb(0, 128, 0); font-size: 20px")
        .with_child(Widget::label("a"));
    let mut tree = WidgetTree::new(root, 400.0, 300.0);
    tree.layout();
    let label = &tree.root.children[0];
    assert_eq!(label.look().foreground, Some(GREEN));
    assert_eq!(label.look().font_size, 20.0);
}

/// **A percentage is of the container's content, and a fixed size is the
/// border box's**, held between its limits.
#[test]
fn sizes_and_percentages_are_laid_out() {
    let tree = tree_of(vec![
        Widget::label("half").css("width: 50%"),
        Widget::label("fixed").css("width: 120px; height: 40px; padding: 5px"),
        Widget::label("held").css("width: 500px; max-width: 200px"),
    ]);
    let half = &tree.root.children[0].layout;
    assert_eq!(half.border_box_width(), 200.0);
    let fixed = &tree.root.children[1].layout;
    assert_eq!(fixed.border_box_width(), 120.0, "padding inside it");
    assert_eq!(fixed.border_box_height(), 40.0);
    let held = &tree.root.children[2].layout;
    assert_eq!(held.border_box_width(), 200.0);
}

/// **A box's shadow is drawn under it, and a label's text shadow under its
/// text** -- offset and in the shadow's colour.
#[test]
fn shadows_are_drawn() {
    let tree = tree_of(vec![Widget::label("t").css(
        "box-shadow: 2px 3px 4px red; text-shadow: 1px 1px blue; color: green",
    )]);
    let commands = tree.render().commands;
    let shadow = commands
        .iter()
        .find_map(|c| match c {
            RenderCommand::BoxShadow {
                offset_x,
                offset_y,
                blur,
                color,
                ..
            } => Some((*offset_x, *offset_y, *blur, *color)),
            _ => None,
        })
        .expect("a box shadow");
    assert_eq!(shadow, (2.0, 3.0, 4.0, RED));
    let texts: Vec<(f32, f32, Color)> = commands
        .iter()
        .filter_map(|c| match c {
            RenderCommand::Text { x, y, color, .. } => Some((*x, *y, *color)),
            _ => None,
        })
        .collect();
    assert_eq!(texts.len(), 2);
    assert_eq!(texts[0].2, BLUE, "the shadow first");
    assert_eq!(texts[1].2, GREEN);
    assert_eq!(
        (texts[0].0 - texts[1].0, texts[0].1 - texts[1].1),
        (1.0, 1.0)
    );
}

/// **A label's text sits where its alignment says.**
#[test]
fn a_labels_text_is_aligned() {
    let x_of = |css: &str| {
        let tree = tree_of(vec![Widget::label("mid").css(css)]);
        tree.render()
            .commands
            .iter()
            .find_map(|c| match c {
                RenderCommand::Text { x, .. } => Some(*x),
                _ => None,
            })
            .unwrap()
    };
    let left = x_of("width: 300px");
    let centre = x_of("width: 300px; text-align: center");
    let right = x_of("width: 300px; text-align: right");
    assert!(left < centre && centre < right, "{left} {centre} {right}");
}

/// **`:disabled`, `:focus` and `:checked` follow the widget's state.**
#[test]
fn the_states_follow_the_widget() {
    let mut tree = tree_of(vec![
        Widget::button("off")
            .css("&:disabled { opacity: 0.4 }")
            .disabled(),
        Widget::checkbox("box", true).css(":checked { color: red }"),
        Widget::text_input("", "").css(":focus { border: 2px solid var(--accent) }"),
    ]);
    assert_eq!(tree.root.children[0].look().opacity, 0.4);
    assert_eq!(tree.root.children[1].look().foreground, Some(RED));
    assert_eq!(tree.root.children[2].look().border.top.width, 0.0);
    let id = tree.root.children[2].id;
    assert!(tree.focus(Some(id)));
    let field = tree.root.children[2].look();
    assert_eq!(field.border.top.width, 2.0, "focused");
    assert_eq!(field.border.top.color, tree.palette().accent);
}

/// **What is not read is said**: the style sheet's and each widget's own.
#[test]
fn what_is_not_read_is_said() {
    let mut tree = tree_of(vec![
        Widget::label("a").css("colour: red; color: var(--nothing)"),
    ]);
    let sheet = tree.set_style_sheet("Label Button { color: red }").to_vec();
    assert_eq!(sheet.len(), 1);
    let all = tree.css_warnings();
    assert_eq!(all.len(), 3, "{all:?}");
    assert!(all[0].contains("style sheet"));
    assert!(all.iter().any(|w| w.contains("colour")));
    assert!(all.iter().any(|w| w.contains("--nothing")));
}
