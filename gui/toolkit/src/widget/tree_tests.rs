//! Tests for the widget tree drawn through the toolkit's component modules,
//! in a palette: what each kind draws, and what its pointer and keys do.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_cmp
)]

use super::{CheckState, Widget, WidgetKind, WidgetTree};
use crate::event::{Event, Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind};
use crate::frame::Rect;
use crate::layout::{Size, SizeConstraint};
use crate::palette::Palette;
use crate::render::{RenderCommand, RenderTree};
use crate::style::FOCUS_RING_WIDTH;

/// A dark palette, so a colour that ignored it -- black text, say -- shows.
fn dark() -> Palette {
    Palette::for_mode(false)
}

/// `w` laid out alone, its border box exactly `width` by `height`.
fn alone(mut w: Widget, width: f32, height: f32, p: &Palette) -> Widget {
    w.do_layout(SizeConstraint::tight(Size::new(width, height)), p);
    w
}

/// What `w` draws, in `p`.
fn drawn(w: &Widget, p: &Palette) -> Vec<RenderCommand> {
    let mut tree = RenderTree::new();
    w.render(p, &mut tree);
    tree.commands
}

fn mouse(x: f32, y: f32, kind: MouseEventKind) -> Event {
    Event::Mouse(MouseEvent { x, y, kind })
}

fn key(k: Key) -> Event {
    Event::Key(KeyEvent {
        key: k,
        pressed: true,
        modifiers: Modifiers::NONE,
        text: String::new(),
    })
}

/// `text` typed into `tree`, a character a keystroke.
fn type_into(tree: &mut WidgetTree, text: &str) {
    for ch in text.chars() {
        tree.handle_event(&Event::Key(KeyEvent {
            // A character arrives in `text`; `Key` has no variant for it.
            key: Key::Unknown(0),
            pressed: true,
            modifiers: Modifiers::NONE,
            text: ch.to_string(),
        }));
    }
}

/// **Each control is drawn by its component module, exactly**: a button,
/// a check box, a radio button and a slider in the tree draw the very
/// commands their modules draw for the same box and state -- so the tree and
/// the modules cannot draw one control two ways.
#[test]
fn each_control_is_drawn_by_its_module() {
    let p = dark();
    let button = alone(Widget::button("OK"), 90.0, 28.0, &p);
    let mut expected = Vec::new();
    crate::button::draw(
        &mut expected,
        &p,
        (0.0, 0.0, 90.0, 28.0),
        "OK",
        crate::button::Kind::Plain,
        crate::button::State::default(),
        p.base,
        FOCUS_RING_WIDTH,
    );
    assert_eq!(drawn(&button, &p), expected);

    let check = alone(Widget::checkbox("Wrap", true), 120.0, 20.0, &p);
    let mut expected = Vec::new();
    crate::checkbox::draw(
        &mut expected,
        &p,
        (0.0, 0.0, 20.0),
        "Wrap",
        CheckState::Checked,
        crate::checkbox::State::default(),
        FOCUS_RING_WIDTH,
    );
    assert_eq!(drawn(&check, &p), expected);

    let radio = alone(Widget::radio("Large", true), 120.0, 20.0, &p);
    let mut expected = Vec::new();
    crate::radio::draw(
        &mut expected,
        &p,
        (0.0, 0.0, 20.0),
        "Large",
        true,
        crate::radio::State::default(),
        FOCUS_RING_WIDTH,
    );
    assert_eq!(drawn(&radio, &p), expected);

    let slider = alone(Widget::slider(0.0, 10.0, 5.0), 200.0, 20.0, &p);
    let mut expected = Vec::new();
    crate::slider::Slider::new(0.0, 10.0, 5.0).draw(
        &mut expected,
        &p,
        &crate::slider::Placement::horizontal(Rect::new(7.0, 8.0, 186.0, 4.0), 14.0),
        crate::slider::Look::accent(&p, p.surface1),
        false,
        FOCUS_RING_WIDTH,
    );
    assert_eq!(drawn(&slider, &p), expected);
}

/// **Text is in the palette's colours unless the program says otherwise**:
/// a label in the theme's text, a disabled one in its disabled grey, one
/// whose style sets a colour in that.
#[test]
fn text_is_in_the_palettes_colours() {
    let p = dark();
    let ink = |w: &Widget| {
        drawn(w, &p).iter().find_map(|c| match c {
            RenderCommand::Text { color, .. } => Some(*color),
            _ => None,
        })
    };
    let label = alone(Widget::label("Hello"), 100.0, 20.0, &p);
    assert_eq!(ink(&label), Some(p.text));
    let off = alone(Widget::label("Hello").disabled(), 100.0, 20.0, &p);
    assert_eq!(ink(&off), Some(p.overlay0));
    let mut own = Widget::label("Hello");
    own.style.foreground = Some(p.red);
    assert_eq!(ink(&alone(own, 100.0, 20.0, &p)), Some(p.red));
}

/// **The kinds that drew nothing draw**: a text area, a radio button, a
/// slider, an image -- and a scroll view what it holds.
#[test]
fn the_kinds_that_drew_nothing_draw() {
    let p = dark();
    let area = alone(Widget::text_area("Lines\nof text", ""), 200.0, 80.0, &p);
    let texts = |cmds: &[RenderCommand]| {
        cmds.iter()
            .filter(|c| {
                matches!(
                    c,
                    RenderCommand::Text { .. } | RenderCommand::RichText { .. }
                )
            })
            .count()
    };
    assert!(texts(&drawn(&area, &p)) >= 2, "{:?}", drawn(&area, &p));
    let image = alone(Widget::image(7, 32.0, 32.0), 32.0, 32.0, &p);
    assert_eq!(
        drawn(&image, &p),
        [RenderCommand::Image {
            x: 0.0,
            y: 0.0,
            width: 32.0,
            height: 32.0,
            image_id: 7
        }]
    );
    let view = alone(
        Widget::scroll_view().with_child(Widget::label("Inside")),
        100.0,
        50.0,
        &p,
    );
    assert!(texts(&drawn(&view, &p)) == 1);
}

/// **A progress bar is the palette's**: its track the surface, its fill the
/// accent, as far as it has got.
#[test]
fn a_progress_bar_is_the_palettes() {
    let p = dark();
    let bar = alone(Widget::progress_bar(1.0, 4.0), 200.0, 12.0, &p);
    let fills: Vec<(f32, crate::color::Color)> = drawn(&bar, &p)
        .iter()
        .filter_map(|c| match c {
            RenderCommand::FillRect { width, color, .. } => Some((*width, *color)),
            _ => None,
        })
        .collect();
    assert_eq!(fills, [(200.0, p.surface0), (50.0, p.accent)]);
    // Nothing done, or a share that is not one, fills nothing.
    for (value, max) in [(0.0, 4.0), (f32::NAN, 4.0), (1.0, 0.0)] {
        let empty = alone(Widget::progress_bar(value, max), 200.0, 12.0, &p);
        assert_eq!(drawn(&empty, &p).len(), 1, "{value} of {max}");
    }
}

/// A tree in the dark palette: a column of `children`, 300 wide.
fn tree_of(children: Vec<Widget>) -> WidgetTree {
    let root = Widget::container()
        .with_flex_direction(crate::layout::FlexDirection::Column)
        .with_children(children);
    let mut tree = WidgetTree::new(root, 300.0, 400.0);
    tree.set_palette(dark());
    tree
}

/// Whether the `index`th child of the tree's root is a chosen radio button.
fn chosen(tree: &WidgetTree, index: usize) -> bool {
    matches!(
        tree.root.children[index].kind,
        WidgetKind::RadioButton { selected: true, .. }
    )
}

/// The centre of the `index`th child of the root, where a click lands on it.
fn centre(tree: &WidgetTree, index: usize) -> (f32, f32) {
    let w = &tree.root.children[index];
    (
        w.layout.x + w.layout.border_box_width() / 2.0,
        w.layout.y + w.layout.border_box_height() / 2.0,
    )
}

/// **Choosing a radio button clears its group**, by a click or by Space;
/// radio buttons in another container are another group.
#[test]
fn choosing_a_radio_button_clears_its_group() {
    let mut tree = tree_of(vec![
        Widget::radio("Small", true),
        Widget::radio("Large", false),
        Widget::container().with_child(Widget::radio("Elsewhere", true)),
    ]);
    let (x, y) = centre(&tree, 1);
    tree.handle_event(&mouse(x, y, MouseEventKind::Press(MouseButton::Left)));
    tree.handle_event(&mouse(x, y, MouseEventKind::Release(MouseButton::Left)));
    assert!(!chosen(&tree, 0) && chosen(&tree, 1));
    assert!(matches!(
        tree.root.children[2].children[0].kind,
        WidgetKind::RadioButton { selected: true, .. }
    ));
    // Space on the focused first one takes the choice back.
    let first = tree.root.children[0].id;
    assert!(tree.focus(Some(first)));
    tree.handle_event(&key(Key::Space));
    assert!(chosen(&tree, 0) && !chosen(&tree, 1));
}

/// The value of the tree's `index`th child, a slider.
fn value(tree: &WidgetTree, index: usize) -> f64 {
    match &tree.root.children[index].kind {
        WidgetKind::Slider { slider } => slider.value(),
        other => panic!("not a slider: {other:?}"),
    }
}

/// **A slider in the tree is moved as its module moves it**: a press on its
/// track, the arrow keys once it has the keyboard.
#[test]
fn a_slider_moves_by_pointer_and_keys() {
    let mut tree = tree_of(vec![Widget::slider(0.0, 10.0, 0.0).with_flex_grow(0.0)]);
    let w = &tree.root.children[0];
    // Three quarters of the way along its track.
    let track_x = w.layout.x + 7.0;
    let track_w = w.layout.width - 14.0;
    let (_, y) = centre(&tree, 0);
    tree.handle_event(&mouse(
        track_x + track_w * 0.75,
        y,
        MouseEventKind::Press(MouseButton::Left),
    ));
    tree.handle_event(&mouse(
        track_x + track_w * 0.75,
        y,
        MouseEventKind::Release(MouseButton::Left),
    ));
    assert!((value(&tree, 0) - 7.5).abs() < 0.2, "{}", value(&tree, 0));
    // The press focused it: End takes it to the top.
    tree.handle_event(&key(Key::End));
    assert_eq!(value(&tree, 0), 10.0);
}

/// The text of the tree's `index`th child, a text area.
fn text(tree: &WidgetTree, index: usize) -> String {
    match &tree.root.children[index].kind {
        WidgetKind::TextArea { area, .. } => area.text().to_owned(),
        other => panic!("not a text area: {other:?}"),
    }
}

/// **A text area in the tree is edited as its module edits it**: a click
/// focuses it and places the caret, typing goes in, Enter breaks the line.
#[test]
fn a_text_area_is_typed_into() {
    let mut tree = tree_of(vec![Widget::text_area("", "Notes")]);
    let (x, y) = centre(&tree, 0);
    tree.handle_event(&mouse(x, y, MouseEventKind::Press(MouseButton::Left)));
    tree.handle_event(&mouse(x, y, MouseEventKind::Release(MouseButton::Left)));
    assert_eq!(tree.focused_id(), Some(tree.root.children[0].id));
    type_into(&mut tree, "hi");
    tree.handle_event(&key(Key::Enter));
    type_into(&mut tree, "there");
    assert_eq!(text(&tree, 0), "hi\nthere");
}

/// The scroll of the tree's `index`th child, a scroll view.
fn scrolled(tree: &WidgetTree, index: usize) -> f32 {
    match tree.root.children[index].kind {
        WidgetKind::ScrollView { scroll_y, .. } => scroll_y,
        ref other => panic!("not a scroll view: {other:?}"),
    }
}

/// A scroll view 100 high holding twenty buttons, 28 high each: 560 of
/// content.
fn scrolling_tree() -> WidgetTree {
    let mut view = Widget::scroll_view();
    view.style.min_height = Some(100.0);
    view.style.max_height = Some(100.0);
    for i in 0..20 {
        view = view.with_child(Widget::button(&format!("Item {i}")));
    }
    tree_of(vec![view])
}

/// **A scroll view scrolls under the wheel, as far as its content goes**,
/// and draws a bar showing where.
#[test]
fn a_scroll_view_scrolls_as_far_as_its_content() {
    let mut tree = scrolling_tree();
    let (x, y) = centre(&tree, 0);
    // Towards the end: a turn of the wheel towards the user.
    tree.handle_event(&mouse(x, y, MouseEventKind::Scroll { dx: 0.0, dy: -1.0 }));
    let once = scrolled(&tree, 0);
    assert!(once > 0.0);
    for _ in 0..100 {
        tree.handle_event(&mouse(x, y, MouseEventKind::Scroll { dx: 0.0, dy: -1.0 }));
    }
    assert_eq!(scrolled(&tree, 0), 560.0 - 100.0);
    // And back to the start, and no further.
    for _ in 0..100 {
        tree.handle_event(&mouse(x, y, MouseEventKind::Scroll { dx: 0.0, dy: 1.0 }));
    }
    assert_eq!(scrolled(&tree, 0), 0.0);
    // Its content overflows it: a bar is drawn.
    let bars = tree
        .render()
        .commands
        .iter()
        .filter(|c| matches!(c, RenderCommand::FillRect { color, .. } if *color == dark().surface2))
        .count();
    assert!(bars >= 1);
}

/// **What a scroll view holds is drawn and hit where it shows**: scrolled
/// up by the scroll, and not at all outside the view.
#[test]
fn a_scroll_views_content_is_hit_where_it_shows() {
    let mut tree = scrolling_tree();
    let (x, y) = centre(&tree, 0);
    for _ in 0..3 {
        tree.handle_event(&mouse(x, y, MouseEventKind::Scroll { dx: 0.0, dy: -1.0 }));
    }
    let scroll = scrolled(&tree, 0);
    let view = &tree.root.children[0];
    // The button now at the view's top edge is the one scroll / 28 down.
    let index = (scroll / 28.0).floor() as usize;
    let top = view.layout.y + 2.0;
    let below = view.layout.y + view.layout.border_box_height() + 10.0;
    let hidden = view.children[index + 5].id;
    tree.handle_event(&mouse(x, top, MouseEventKind::Press(MouseButton::Left)));
    let pressed: Vec<usize> = tree.root.children[0]
        .children
        .iter()
        .enumerate()
        .filter(|(_, b)| matches!(b.kind, WidgetKind::Button { pressed: true, .. }))
        .map(|(i, _)| i)
        .collect();
    assert_eq!(pressed, [index]);
    // Below the view, nothing in it is hit -- though a button lies there.
    assert_ne!(tree.root.focus_target_at(x, below), Some(hidden));
}

/// **The pointer over a button lights it**, and leaving puts it out.
#[test]
fn the_pointer_over_a_button_lights_it() {
    let mut tree = tree_of(vec![Widget::button("OK").with_flex_grow(0.0)]);
    let unlit = tree.render().commands;
    let (x, y) = centre(&tree, 0);
    tree.handle_event(&mouse(x, y, MouseEventKind::Move));
    assert!(tree.root.children[0].is_hovered());
    assert_ne!(tree.render().commands, unlit);
    tree.handle_event(&mouse(x, y, MouseEventKind::Leave));
    assert_eq!(tree.render().commands, unlit);
}

/// **A new palette is drawn in**: the program gives the tree the user's, and
/// changes it when the user changes theme.
#[test]
fn a_new_palette_is_drawn_in() {
    let mut tree = tree_of(vec![Widget::label("Hello")]);
    let dark_ink = tree.render().commands;
    tree.set_palette(Palette::for_mode(true));
    assert_ne!(tree.render().commands, dark_ink);
    assert!(tree.render().commands.iter().any(
        |c| matches!(c, RenderCommand::Text { color, .. } if *color == Palette::for_mode(true).text)
    ));
}

/// **A control is as big as its module makes it**, so what is laid out is
/// what is drawn: a button in the theme's padding, a box or an option its
/// mark and label, a slider its thumb's row, a picture its size.
#[test]
fn a_control_is_as_big_as_its_module_makes_it() {
    let p = dark();
    let size = |w: Widget| w.intrinsic_size(&p);
    assert_eq!(
        size(Widget::button("Apply")),
        Size::new(
            crate::button::width(&p.widget_style.button, "Apply"),
            crate::button::HEIGHT
        )
    );
    assert_eq!(
        size(Widget::checkbox("Wrap", false)),
        Size::new(crate::checkbox::width("Wrap"), crate::checkbox::HEIGHT)
    );
    assert_eq!(
        size(Widget::radio("Large", false)),
        Size::new(crate::radio::width("Large"), crate::checkbox::HEIGHT)
    );
    assert_eq!(size(Widget::slider(0.0, 1.0, 0.0)), Size::new(120.0, 20.0));
    assert_eq!(size(Widget::image(1, 40.0, 30.0)), Size::new(40.0, 30.0));
    assert_eq!(size(Widget::image(1, f32::NAN, -3.0)), Size::new(0.0, 0.0));
}

/// **A drag in a text area selects**, from where it was pressed to where it
/// is let go -- even past the area's edge.
#[test]
fn a_drag_in_a_text_area_selects() {
    let mut tree = tree_of(vec![Widget::text_area("hello world", "")]);
    let w = &tree.root.children[0];
    let (left, y) = (w.layout.x + 10.0, w.layout.y + 10.0);
    let past = w.layout.x + w.layout.border_box_width() + 50.0;
    tree.handle_event(&mouse(left, y, MouseEventKind::Press(MouseButton::Left)));
    tree.handle_event(&mouse(past, y, MouseEventKind::Move));
    tree.handle_event(&mouse(past, y, MouseEventKind::Release(MouseButton::Left)));
    let WidgetKind::TextArea { area, .. } = &tree.root.children[0].kind else {
        panic!("not a text area");
    };
    assert!(area.has_selection());
    assert!(
        area.selected_text().ends_with("world"),
        "{:?}",
        area.selected_text()
    );
}

/// **A press below a scroll view reaches nothing scrolled out of it**,
/// though one of its buttons lies there.
#[test]
fn a_press_below_a_scroll_view_reaches_nothing_in_it() {
    let mut tree = scrolling_tree();
    let view = &tree.root.children[0];
    let (x, below) = (
        view.layout.x + view.layout.border_box_width() / 2.0,
        view.layout.y + view.layout.border_box_height() + 10.0,
    );
    tree.handle_event(&mouse(x, below, MouseEventKind::Press(MouseButton::Left)));
    assert!(
        tree.root.children[0]
            .children
            .iter()
            .all(|b| matches!(b.kind, WidgetKind::Button { pressed: false, .. }))
    );
}
