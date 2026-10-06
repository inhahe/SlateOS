//! Tests for a widget tree's signals: what each control signals, and how a
//! program hears them -- the queue, a callback, a channel.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_cmp
)]

use std::cell::RefCell;
use std::rc::Rc;

use super::{CheckState, MAX_QUEUED_SIGNALS, Signal, SignalKind, Widget, WidgetKind, WidgetTree};
use crate::event::{Event, Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind};
use crate::layout::FlexDirection;
use crate::palette::Palette;

/// A column of `children` in a window 400 by 300, laid out.
fn tree_of(children: Vec<Widget>) -> WidgetTree {
    let root = Widget::container()
        .with_flex_direction(FlexDirection::Column)
        .with_children(children);
    let mut tree = WidgetTree::new(root, 400.0, 300.0);
    tree.set_palette(Palette::for_mode(false));
    tree
}

fn mouse(tree: &mut WidgetTree, x: f32, y: f32, kind: MouseEventKind) {
    tree.handle_event(&Event::Mouse(MouseEvent { x, y, kind }));
}

/// The middle of the root's `index`th child.
fn centre(tree: &WidgetTree, index: usize) -> (f32, f32) {
    let b = &tree.root.children[index].layout;
    (
        b.x + b.border_box_width() / 2.0,
        b.y + b.border_box_height() / 2.0,
    )
}

/// A press and a release of the left button over the `index`th child.
fn click(tree: &mut WidgetTree, index: usize) {
    let (x, y) = centre(tree, index);
    mouse(tree, x, y, MouseEventKind::Press(MouseButton::Left));
    mouse(tree, x, y, MouseEventKind::Release(MouseButton::Left));
}

fn key(tree: &mut WidgetTree, k: Key) {
    tree.handle_event(&Event::Key(KeyEvent {
        key: k,
        pressed: true,
        modifiers: Modifiers::NONE,
        text: String::new(),
    }));
}

fn typed(tree: &mut WidgetTree, text: &str) {
    for ch in text.chars() {
        tree.handle_event(&Event::Key(KeyEvent {
            key: Key::Unknown(0),
            pressed: true,
            modifiers: Modifiers::NONE,
            text: ch.to_string(),
        }));
    }
}

/// What the tree signalled, as the kinds alone.
fn kinds(tree: &mut WidgetTree) -> Vec<SignalKind> {
    tree.take_signals().into_iter().map(|s| s.kind).collect()
}

/// **A click is a press and a release over a button**, signalled once, from
/// the button; a press on it released off it is none, and lets it go.
#[test]
fn a_click_is_a_press_and_release_over_a_button() {
    let mut tree = tree_of(vec![Widget::button("Go")]);
    let id = tree.root.children[0].id;
    click(&mut tree, 0);
    assert_eq!(
        tree.take_signals(),
        [Signal {
            from: id,
            kind: SignalKind::Clicked
        }]
    );
    let (x, y) = centre(&tree, 0);
    mouse(&mut tree, x, y, MouseEventKind::Press(MouseButton::Left));
    mouse(
        &mut tree,
        399.0,
        299.0,
        MouseEventKind::Release(MouseButton::Left),
    );
    assert!(kinds(&mut tree).is_empty(), "released off it");
    assert!(
        matches!(
            tree.root.children[0].kind,
            WidgetKind::Button { pressed: false, .. }
        ),
        "and let go of"
    );
    // A release over it after a press elsewhere is no click either.
    mouse(&mut tree, x, y, MouseEventKind::Release(MouseButton::Left));
    assert!(kinds(&mut tree).is_empty());
}

/// **Space or Enter clicks a focused button**, and leaves it drawn as it
/// was -- not stuck pressed.
#[test]
fn space_or_enter_clicks_a_focused_button() {
    let mut tree = tree_of(vec![Widget::button("Go")]);
    assert!(tree.focus_first());
    key(&mut tree, Key::Space);
    key(&mut tree, Key::Enter);
    assert_eq!(kinds(&mut tree), [SignalKind::Clicked, SignalKind::Clicked]);
    assert!(matches!(
        tree.root.children[0].kind,
        WidgetKind::Button { pressed: false, .. }
    ));
}

/// **A checkbox says what it is now; a radio button says it was chosen**,
/// once, and not again while it stays chosen.
#[test]
fn a_box_says_its_state_and_a_radio_its_choice() {
    let mut tree = tree_of(vec![
        Widget::checkbox("Wrap", false),
        Widget::radio("One", false),
    ]);
    click(&mut tree, 0);
    click(&mut tree, 0);
    assert_eq!(
        kinds(&mut tree),
        [
            SignalKind::Toggled(CheckState::Checked),
            SignalKind::Toggled(CheckState::Unchecked)
        ]
    );
    click(&mut tree, 1);
    click(&mut tree, 1);
    assert_eq!(kinds(&mut tree), [SignalKind::Chosen], "chosen once");
}

/// **A text field says each edit, and Enter as "done"**; a key that changes
/// nothing -- Backspace in an empty field, an arrow -- says nothing.
#[test]
fn a_field_says_each_edit_and_enter() {
    let mut tree = tree_of(vec![Widget::text_input("", "Name")]);
    assert!(tree.focus_first());
    key(&mut tree, Key::Backspace);
    key(&mut tree, Key::Left);
    assert!(kinds(&mut tree).is_empty(), "nothing changed");
    typed(&mut tree, "ab");
    key(&mut tree, Key::Backspace);
    key(&mut tree, Key::Enter);
    assert_eq!(
        kinds(&mut tree),
        [
            SignalKind::Edited,
            SignalKind::Edited,
            SignalKind::Edited,
            SignalKind::Submitted
        ]
    );
}

/// **A slider says where it moved to**, and a key that moves it nowhere --
/// Home at its start -- says nothing.
#[test]
fn a_slider_says_where_it_moved() {
    let mut tree = tree_of(vec![Widget::slider(0.0, 10.0, 0.0)]);
    assert!(tree.focus_first());
    key(&mut tree, Key::Home);
    assert!(kinds(&mut tree).is_empty());
    key(&mut tree, Key::End);
    assert_eq!(kinds(&mut tree), [SignalKind::Moved(10.0)]);
}

/// **A text area says each edit.**
#[test]
fn a_text_area_says_each_edit() {
    let mut tree = tree_of(vec![Widget::text_area("", "Notes")]);
    assert!(tree.focus_first());
    typed(&mut tree, "x");
    key(&mut tree, Key::Enter);
    assert_eq!(kinds(&mut tree), [SignalKind::Edited, SignalKind::Edited]);
}

/// **A callback hears its widget's signals, or every widget's, until it is
/// disconnected** -- and the queue keeps them as well.
#[test]
fn a_callback_hears_its_widgets_signals_until_disconnected() {
    let mut tree = tree_of(vec![Widget::button("A"), Widget::button("B")]);
    let (a, b) = (tree.root.children[0].id, tree.root.children[1].id);
    let heard_a = Rc::new(RefCell::new(Vec::new()));
    let heard_all = Rc::new(RefCell::new(0usize));
    let slot = {
        let heard_a = Rc::clone(&heard_a);
        tree.connect(Some(a), move |s| heard_a.borrow_mut().push(s.from))
    };
    {
        let heard_all = Rc::clone(&heard_all);
        tree.connect(None, move |_| *heard_all.borrow_mut() += 1);
    }
    click(&mut tree, 0);
    click(&mut tree, 1);
    assert_eq!(*heard_a.borrow(), [a], "only its own");
    assert_eq!(*heard_all.borrow(), 2);
    assert!(tree.disconnect(slot));
    assert!(!tree.disconnect(slot), "already gone");
    click(&mut tree, 0);
    assert_eq!(heard_a.borrow().len(), 1, "no longer");
    let queued: Vec<_> = tree.take_signals().into_iter().map(|s| s.from).collect();
    assert_eq!(queued, [a, b, a]);
    assert!(tree.take_signals().is_empty(), "taken");
}

/// **A channel is sent each signal, and let go of once its receiver has
/// gone.**
#[test]
fn a_channel_is_sent_each_signal() {
    let mut tree = tree_of(vec![Widget::button("Go")]);
    let id = tree.root.children[0].id;
    let (send, receive) = std::sync::mpsc::channel();
    tree.connect_channel(send);
    click(&mut tree, 0);
    assert_eq!(
        receive.try_recv(),
        Ok(Signal {
            from: id,
            kind: SignalKind::Clicked
        })
    );
    drop(receive);
    click(&mut tree, 0);
    assert!(tree.channels.is_empty(), "let go of");
}

/// **The queue keeps the newest signals, no more than its bound**, for a
/// program that hears them another way and never takes them.
#[test]
fn the_queue_is_bounded() {
    let mut tree = tree_of(vec![Widget::checkbox("x", false)]);
    for _ in 0..MAX_QUEUED_SIGNALS + 2 {
        click(&mut tree, 0);
    }
    let kept = tree.take_signals();
    assert_eq!(kept.len(), MAX_QUEUED_SIGNALS);
    // The oldest two went: the first kept is the third, a tick (odd clicks
    // tick, even ones untick).
    assert_eq!(kept[0].kind, SignalKind::Toggled(CheckState::Checked));
}
