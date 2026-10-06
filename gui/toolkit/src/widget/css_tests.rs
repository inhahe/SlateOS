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

use super::{Widget, WidgetKind, WidgetTree, weight_to_hint};
use crate::color::Color;
use crate::event::{Event, EventResult, MouseButton, MouseEvent, MouseEventKind};
use crate::layout::FlexDirection;
use crate::motion::{Curve, Motion};
use crate::palette::Palette;
use crate::render::{FontFamily, FontWeightHint, RenderCommand};
use crate::style::Edges;
use crate::text::{in_family, measure_in};

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

/// **A coloured margin is drawn as four bands round the border box** -- the
/// top and bottom whole across, the sides between them -- and a clear one
/// draws nothing.
#[test]
fn a_coloured_margin_is_drawn_round_the_box() {
    let tree = tree_of(vec![
        Widget::label("hi").css("margin: 10px 20px; margin-color: red; margin-left-color: blue"),
        Widget::label("plain").css("margin: 10px"),
    ]);
    let label = &tree.root.children[0];
    let (x, y) = (label.layout.x, label.layout.y);
    let (w, h) = (label.layout.outer_width(), label.layout.outer_height());
    let fills: Vec<(f32, f32, f32, f32, Color)> = tree
        .render()
        .commands
        .into_iter()
        .filter_map(|c| match c {
            RenderCommand::FillRect {
                x,
                y,
                width,
                height,
                color,
                ..
            } if color == RED || color == BLUE => Some((x, y, width, height, color)),
            _ => None,
        })
        .collect();
    let between = label.layout.border_box_height();
    assert_eq!(
        fills,
        [
            (x, y, w, 10.0, RED),
            (x, y + h - 10.0, w, 10.0, RED),
            (x, y + 10.0, 20.0, between, BLUE),
            (x + w - 20.0, y + 10.0, 20.0, between, RED),
        ]
    );
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

/// The fonts `commands` push and pop, in order: each push's family, and a
/// pop as `None`.
fn fonts_of(commands: &[RenderCommand]) -> Vec<Option<FontFamily>> {
    commands
        .iter()
        .filter_map(|c| match c {
            RenderCommand::PushFont { family } => Some(Some(*family)),
            RenderCommand::PopFont => Some(None),
            _ => None,
        })
        .collect()
}

/// The width of `w`'s text as it is measured to lay it out, padding off.
fn text_width(w: &Widget, p: &Palette) -> f32 {
    w.intrinsic_size(p).width - w.look().padding.horizontal()
}

/// `text` at `w`'s size and weight, in `family`.
fn measured(w: &Widget, text: &str, family: FontFamily) -> f32 {
    let look = w.look();
    measure_in(
        text,
        look.font_size,
        weight_to_hint(look.font_weight),
        family,
    )
}

/// **A widget's family is pushed round its drawing, and its text is
/// measured in it**: a label in `monospace` is laid out as wide as its text
/// in the fixed-pitch face, and drawn in that face.
#[test]
fn a_family_draws_and_measures_a_widgets_text() {
    let text = "Hamburgefonstiv WMQ";
    let tree = tree_of(vec![Widget::label(text).css("font-family: monospace")]);
    let label = &tree.root.children[0];
    assert_eq!(
        text_width(label, tree.palette()),
        measured(label, text, FontFamily::Mono)
    );
    let commands = tree.render().commands;
    assert_eq!(
        fonts_of(&commands),
        vec![Some(FontFamily::Mono), None],
        "pushed once"
    );
    let at = |want: fn(&RenderCommand) -> bool| commands.iter().position(want).unwrap();
    let push = at(|c| matches!(c, RenderCommand::PushFont { .. }));
    let drawn = at(|c| matches!(c, RenderCommand::Text { .. }));
    let pop = at(|c| matches!(c, RenderCommand::PopFont));
    assert!(push < drawn && drawn < pop, "{push} {drawn} {pop}");
}

/// **Children inherit their parent's family and are not pushed again**; a
/// child with a family of its own pushes it, and one set back to `initial`
/// pushes the UI face rather than staying in its parent's.
#[test]
fn children_inherit_a_family_and_push_only_their_own() {
    let root = Widget::container()
        .with_flex_direction(FlexDirection::Column)
        .css("font-family: monospace")
        .with_children(vec![
            Widget::label("inherits"),
            Widget::label("its own").css("font-family: system-ui"),
            Widget::label("set back").css("font-family: initial"),
        ]);
    let mut tree = WidgetTree::new(root, 400.0, 300.0);
    tree.layout();
    let (mono, ui) = (Some(FontFamily::Mono), Some(FontFamily::Ui));
    assert_eq!(
        fonts_of(&tree.render().commands),
        vec![mono, ui, None, ui, None, None]
    );
    let p = tree.palette();
    let [inherits, own, back] = [0, 1, 2].map(|i| &tree.root.children[i]);
    assert_eq!(
        text_width(inherits, p),
        measured(inherits, "inherits", FontFamily::Mono)
    );
    assert_eq!(text_width(own, p), measured(own, "its own", FontFamily::Ui));
    assert_eq!(
        text_width(back, p),
        measured(back, "set back", FontFamily::Ui)
    );
}

/// **A list's first family drawn here is the one**: a name this machine has
/// no font for is passed over, and with none drawn here it is the UI face.
#[test]
fn a_list_takes_its_first_family_drawn_here() {
    let font_of = |css: &str| tree_of(vec![Widget::label("x").css(css)]).root.children[0].font;
    assert_eq!(
        font_of("font-family: \"Slate No Such Family 3c9\", monospace"),
        Some(FontFamily::Mono)
    );
    assert_eq!(
        font_of("font-family: \"Slate No Such Family 3c9\""),
        Some(FontFamily::Ui),
        "none drawn here"
    );
    assert_eq!(font_of("color: red"), None, "no family: the drawing's");
}

/// **A click in a text field finds the caret by its family's advances** --
/// the event handled measuring in the face the field is drawn in.
#[test]
fn a_click_finds_the_caret_in_the_fields_family() {
    let value = "WWWWiiiiWWii";
    let mut tree = tree_of(vec![
        Widget::text_input(value, "").css("font-family: monospace"),
    ]);
    let field = &tree.root.children[0];
    let WidgetKind::TextInput { cursor, .. } = field.kind else {
        panic!("a text input");
    };
    let (content_x, _) = field.content_origin();
    let (width, size) = (field.layout.width, field.look().font_size);
    let y = field.layout.y + field.layout.border_box_height() / 2.0;
    let at = |dx: f32| {
        crate::textedit::cursor_at_click(value, cursor, width, size, FontWeightHint::Regular, dx)
    };
    // Where the two faces put a click differently, if anywhere: what only
    // measuring in the field's own face gets right.
    let dx = (1..80u8)
        .map(|i| f32::from(i) * 2.0)
        .find(|&dx| in_family(FontFamily::Mono, || at(dx)) != at(dx))
        .unwrap_or(30.0);
    let want = in_family(FontFamily::Mono, || at(dx));
    tree.handle_event(&Event::Mouse(MouseEvent {
        x: content_x + dx,
        y,
        kind: MouseEventKind::Press(MouseButton::Left),
    }));
    let WidgetKind::TextInput { cursor, .. } = tree.root.children[0].kind else {
        panic!("a text input");
    };
    assert_eq!(cursor, want);
}

/// `tree`, timed by its ticks, in a palette that moves at a constant speed
/// at the standard pace -- or not at all, with `motion` still.
fn ticking(mut tree: WidgetTree, motion: Motion) -> WidgetTree {
    tree.time_by_ticks();
    let mut palette = Palette::for_mode(false);
    palette.motion = motion;
    tree.set_palette(palette);
    tree
}

fn tick(tree: &mut WidgetTree, ms: u64) -> EventResult {
    tree.handle_event(&Event::Tick { elapsed_ms: ms })
}

/// The colour `p` of the way from `a` to `b`, as a transition mixes it.
fn part_way(a: Color, b: Color, p: f32) -> Color {
    let lerp = |x: u8, y: u8| {
        let v = f32::from(x) + (f32::from(y) - f32::from(x)) * p;
        // Opaque either side: a plain mix, rounded.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let v = v.round() as u8;
        v
    };
    Color::rgba(lerp(a.r, b.r), lerp(a.g, b.g), lerp(a.b, b.b), 255)
}

const LINEAR: Motion = Motion::new(Motion::STANDARD_MS, Curve::Linear);

/// **A style's transition moves a change of state over its time**: the
/// pointer over a button moves its background from red to blue as the
/// ticks come, and the tree says it is animating until it is there.
#[test]
fn a_change_of_state_moves_over_the_ticks() {
    let mut tree = ticking(
        tree_of(vec![Widget::button("Go").css(
            "background-color: red; transition: background-color 100ms linear; \
             &:hover { background-color: blue }",
        )]),
        LINEAR,
    );
    assert!(!tree.animating());
    assert_eq!(tick(&mut tree, 16), EventResult::Ignored, "nothing moving");
    let (x, y) = middle_of_first(&tree);
    moved_to(&mut tree, x, y);
    assert!(tree.animating());
    assert_eq!(tree.root.children[0].look().background, RED, "from red");
    assert_eq!(tick(&mut tree, 50), EventResult::Consumed, "draw again");
    assert_eq!(
        tree.root.children[0].look().background,
        part_way(RED, BLUE, 0.5)
    );
    tick(&mut tree, 50);
    assert_eq!(tree.root.children[0].look().background, BLUE);
    assert!(!tree.animating(), "there");
}

/// **A child inherits its parent's colour part-way**, as it moves.
#[test]
fn a_child_inherits_a_moving_colour() {
    let panel = Widget::container()
        .with_flex_direction(FlexDirection::Column)
        .css("color: red; transition: color 100ms linear; &:hover { color: blue }")
        .with_child(Widget::label("text"));
    let mut tree = ticking(tree_of(vec![panel]), LINEAR);
    let (x, y) = middle_of_first(&tree);
    moved_to(&mut tree, x, y);
    tick(&mut tree, 50);
    let label = &tree.root.children[0].children[0];
    assert_eq!(label.look().foreground, Some(part_way(RED, BLUE, 0.5)));
}

/// **With animations off nothing moves**: the change is shown at once, and
/// the tree never says it is animating.
#[test]
fn with_animations_off_a_change_is_shown_at_once() {
    let mut tree = ticking(
        tree_of(vec![Widget::button("Go").css(
            "background-color: red; transition: background-color 100ms; \
             &:hover { background-color: blue }",
        )]),
        Motion::STILL,
    );
    let (x, y) = middle_of_first(&tree);
    moved_to(&mut tree, x, y);
    assert_eq!(tree.root.children[0].look().background, BLUE);
    assert!(!tree.animating());
}

/// **A state whose style moves moves into it, and is left at once** -- CSS's
/// rule, that the transition is the new style's: the hover's own
/// `transition` moves the button into its hover, and the style it leaves
/// for has none.
#[test]
fn a_states_own_transition_moves_into_it_only() {
    let mut tree = ticking(
        tree_of(vec![Widget::button("Go").css(
            "&:hover { background-color: blue; transition: background-color 100ms linear }",
        )]),
        LINEAR,
    );
    let program = tree.root.children[0].style.background;
    let (x, y) = middle_of_first(&tree);
    moved_to(&mut tree, x, y);
    assert_eq!(
        tree.root.children[0].look().background,
        program,
        "from its own"
    );
    tick(&mut tree, 50);
    // A button's own background is the theme's to draw -- none of its own --
    // so the blue fades in over it, rather than darkening through black.
    assert_eq!(program.a, 0);
    assert_eq!(
        tree.root.children[0].look().background,
        Color::rgba(0, 0, 255, 128)
    );
    moved_to(&mut tree, 399.0, 299.0);
    assert_eq!(
        tree.root.children[0].look().background,
        program,
        "back at once"
    );
    assert!(!tree.animating());
}

/// **A size moves too, and the tree is laid out as it does**: padding that
/// grows on hover grows the button frame by frame.
#[test]
fn a_size_moves_and_is_laid_out_as_it_does() {
    let mut tree = ticking(
        tree_of(vec![Widget::label("x").css(
            "padding: 0; transition: padding 100ms linear; &:hover { padding: 10px }",
        )]),
        LINEAR,
    );
    let width = |t: &WidgetTree| t.root.children[0].layout.border_box_height();
    let before = width(&tree);
    let (x, y) = middle_of_first(&tree);
    moved_to(&mut tree, x, y);
    tick(&mut tree, 50);
    assert_eq!(width(&tree), before + 10.0, "half of 10 each side");
    tick(&mut tree, 50);
    assert_eq!(width(&tree), before + 20.0);
}

/// **A transition moves on with every event, not with ticks alone**: on the
/// machine's clock, a program that sends none sees it over at the first
/// event after its end -- here a move within the widget, which changes no
/// state -- rather than left where it started.
#[test]
fn a_transition_ends_at_an_event_without_ticks() {
    let mut tree = tree_of(vec![Widget::button("Go").css(
        "background-color: red; transition: background-color 1ms linear; \
         &:hover { background-color: blue }",
    )]);
    let mut palette = Palette::for_mode(false);
    palette.motion = LINEAR;
    tree.set_palette(palette);
    let (x, y) = middle_of_first(&tree);
    moved_to(&mut tree, x, y);
    assert!(tree.animating(), "under way");
    std::thread::sleep(std::time::Duration::from_millis(30));
    moved_to(&mut tree, x + 1.0, y);
    assert_eq!(tree.root.children[0].look().background, BLUE, "over");
    assert!(!tree.animating());
}

/// A press of the left button at a point.
fn pressed_at(tree: &mut WidgetTree, x: f32, y: f32) -> EventResult {
    tree.handle_event(&Event::Mouse(MouseEvent {
        x,
        y,
        kind: MouseEventKind::Press(MouseButton::Left),
    }))
}

/// Whether the `index`th child of the root is a pressed button.
fn is_pressed(tree: &WidgetTree, index: usize) -> bool {
    matches!(
        tree.root.children[index].kind,
        WidgetKind::Button { pressed: true, .. }
    )
}

/// **A relative widget moves from where the flow put it, and nothing
/// around it moves.**
#[test]
fn a_relative_widget_moves_and_nothing_around_it_does() {
    let still = tree_of(vec![Widget::label("a"), Widget::label("b")]);
    let moved = tree_of(vec![
        Widget::label("a").css("position: relative; left: 10px; top: 5px"),
        Widget::label("b"),
    ]);
    let (a, b) = (&still.root.children, &moved.root.children);
    assert_eq!(b[0].layout.x, a[0].layout.x + 10.0);
    assert_eq!(b[0].layout.y, a[0].layout.y + 5.0);
    assert_eq!(
        (b[1].layout.x, b[1].layout.y),
        (a[1].layout.x, a[1].layout.y),
        "its neighbour stays"
    );
    let back = tree_of(vec![
        Widget::label("a").css("position: relative; right: 4px; bottom: 2px"),
    ]);
    assert_eq!(back.root.children[0].layout.x, a[0].layout.x - 4.0);
    assert_eq!(back.root.children[0].layout.y, a[0].layout.y - 2.0);
}

/// A panel 220 by 120 with 10 pixels of padding -- its content 200 by 100
/// -- holding `children`, at the window's corner.
fn panel_of(children: Vec<Widget>) -> WidgetTree {
    tree_of(vec![
        Widget::container()
            .with_flex_direction(FlexDirection::Column)
            .css("width: 220px; height: 120px; padding: 10px")
            .with_children(children),
    ])
}

/// **An absolute widget is placed in its parent's padding box by its
/// insets, and takes no room in the flow**: against the far corner by
/// `right` and `bottom`, stretched between `left` and `right`, a
/// percentage of the box.
#[test]
fn an_absolute_widget_is_placed_by_its_insets() {
    let tree = panel_of(vec![
        Widget::label("flow"),
        Widget::label("corner")
            .css("position: absolute; right: 0; bottom: 0; width: 50px; height: 20px"),
        Widget::label("across").css("position: absolute; left: 10px; right: 10px; top: 0"),
        Widget::label("half").css("position: absolute; left: 50%; top: 50%"),
    ]);
    let panel = &tree.root.children[0];
    let [flow, corner, across, half] = [0, 1, 2, 3].map(|i| &panel.children[i]);
    assert_eq!(
        (flow.layout.x, flow.layout.y),
        (0.0, 0.0),
        "first in the flow"
    );
    // The padding box is 220 by 120, starting 10 before the content.
    assert_eq!(corner.layout.x, 220.0 - 50.0 - 10.0);
    assert_eq!(corner.layout.y, 120.0 - 20.0 - 10.0);
    assert_eq!(corner.layout.border_box_width(), 50.0);
    assert_eq!(across.layout.x, 10.0 - 10.0);
    assert_eq!(
        across.layout.border_box_width(),
        200.0,
        "220 less 10 each side"
    );
    assert_eq!(half.layout.x, 110.0 - 10.0);
    assert_eq!(half.layout.y, 60.0 - 10.0);
}

/// **`z-index` orders siblings, drawn and hit**: the higher is drawn later,
/// and where two overlap the pointer is the one on top's -- its hover, its
/// press.
#[test]
fn z_index_orders_siblings_drawn_and_hit() {
    let over = "position: absolute; left: 0; top: 0; width: 100px; height: 40px";
    let mut tree = tree_of(vec![
        Widget::button("high").css(&format!("{over}; z-index: 2")),
        Widget::button("low").css(over),
    ]);
    let texts: Vec<String> = tree
        .render()
        .commands
        .iter()
        .filter_map(|c| match c {
            RenderCommand::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(texts, ["low", "high"], "the higher drawn last");
    moved_to(&mut tree, 20.0, 20.0);
    assert!(tree.root.children[0].is_hovered(), "the one on top");
    assert!(!tree.root.children[1].is_hovered(), "not what lies beneath");
    pressed_at(&mut tree, 20.0, 20.0);
    assert!(is_pressed(&tree, 0) && !is_pressed(&tree, 1));
}

/// **A fixed widget is placed in the window, drawn over everything, and
/// takes the pointer first** -- wherever it is in the tree, and uncut by
/// the panel it is in.
#[test]
fn a_fixed_widget_is_placed_in_the_window_over_everything() {
    let mut tree = tree_of(vec![
        Widget::button("beneath").css("width: 200px; height: 60px"),
        Widget::container()
            .with_flex_direction(FlexDirection::Column)
            .css("width: 50px; height: 50px; padding: 5px")
            .with_child(Widget::button("toast").css(
                "position: fixed; left: 10px; top: 10px; width: 120px; height: 30px; z-index: 1",
            )),
    ]);
    let toast = &tree.root.children[1].children[0];
    assert_eq!(
        (toast.layout.x, toast.layout.y),
        (10.0, 10.0),
        "in the window"
    );
    assert_eq!(
        toast.layout.border_box_width(),
        120.0,
        "wider than its panel"
    );
    let commands = tree.render().commands;
    let last_text = commands.iter().rev().find_map(|c| match c {
        RenderCommand::Text { text, .. } => Some(text.as_str()),
        _ => None,
    });
    assert_eq!(last_text, Some("toast"), "drawn last");
    // Over the button beneath it, the press is the toast's.
    moved_to(&mut tree, 20.0, 20.0);
    assert!(tree.root.children[1].children[0].is_hovered());
    assert!(!tree.root.children[0].is_hovered(), "nothing beneath it");
    pressed_at(&mut tree, 20.0, 20.0);
    assert!(matches!(
        tree.root.children[1].children[0].kind,
        WidgetKind::Button { pressed: true, .. }
    ));
    assert!(!is_pressed(&tree, 0));
    // Clear of it, the button beneath takes the press.
    pressed_at(&mut tree, 150.0, 50.0);
    assert!(is_pressed(&tree, 0));
}

/// **`border-radius: 50%` draws a circle**: the radius is half the box's
/// side, once it is laid out -- and drawn so.
#[test]
fn a_half_radius_draws_a_circle() {
    let tree = tree_of(vec![Widget::container().css(
        "width: 40px; height: 40px; background-color: red; border-radius: 50%",
    )]);
    assert_eq!(tree.root.children[0].look().border_radius.top_left, 20.0);
    let drawn = tree.render().commands.iter().find_map(|c| match c {
        RenderCommand::FillRect {
            color,
            corner_radii,
            ..
        } if *color == RED => Some(corner_radii.bottom_right),
        _ => None,
    });
    assert_eq!(drawn, Some(20.0));
}
