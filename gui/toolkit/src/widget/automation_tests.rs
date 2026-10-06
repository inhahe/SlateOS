//! Tests for the tree as tools see it ([`super::automation`]): what a node
//! says, what a search finds, and what an action does -- through the widget,
//! as its user would.

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

use super::automation::{Action, Node, Query, Refusal, Role, Value};
use super::{CheckState, Signal, SignalKind, Widget, WidgetId, WidgetKind, WidgetTree};
use crate::event::{Event, MouseButton, MouseEvent, MouseEventKind};
use crate::layout::FlexDirection;
use crate::palette::Palette;

/// A form, and the ids of its parts.
struct Form {
    tree: WidgetTree,
    label: WidgetId,
    field: WidgetId,
    boxed: WidgetId,
    middle: WidgetId,
    small: WidgetId,
    large: WidgetId,
    slider: WidgetId,
    area: WidgetId,
    progress: WidgetId,
    picture: WidgetId,
    pane: WidgetId,
    inside: WidgetId,
    save: WidgetId,
    delete: WidgetId,
    held_off: WidgetId,
    hidden: WidgetId,
    kept: WidgetId,
    kept_button: WidgetId,
}

/// A form in a window 400 by 900: a label, a field, two boxes (one in its
/// middle state), two choices, a slider, a text area, a progress bar, a
/// picture, a scroll pane holding a tall button, a button and a disabled
/// one, a button in a disabled panel, a button in a hidden one, and a panel
/// its program keeps from tools.
fn form() -> Form {
    let label = Widget::label("Name:");
    let field = Widget::text_input("", "Your name").named("name");
    let boxed = Widget::checkbox("Subscribe", false);
    let mut middle = Widget::checkbox("Some", false);
    if let WidgetKind::Checkbox { checked, .. } = &mut middle.kind {
        *checked = CheckState::Indeterminate;
    }
    let small = Widget::radio("Small", true);
    let large = Widget::radio("Large", false);
    let choices = Widget::container().with_children(vec![small, large]);
    let (small, large) = (choices.children[0].id, choices.children[1].id);
    let slider = Widget::slider(0.0, 10.0, 5.0).labelled("Volume");
    let area = Widget::text_area("", "Notes");
    let progress = Widget::progress_bar(3.0, 10.0).with_tooltip("Copying");
    let picture = Widget::image(7, 16.0, 16.0).labelled("Logo");
    let inside = Widget::button("Inside").css("height: 200px");
    let inside_id = inside.id;
    let pane = Widget::scroll_view().css("height: 40px").with_child(inside);
    let save = Widget::button("Save").named("save").with_tooltip("Keep it");
    let delete = Widget::button("Delete").disabled();
    let held_off = Widget::button("Held off");
    let held_off_id = held_off.id;
    let off_panel = Widget::container().disabled().with_child(held_off);
    let hidden = Widget::button("Unseen");
    let hidden_id = hidden.id;
    let hidden_panel = Widget::container().hidden().with_child(hidden);
    let kept_button = Widget::button("Secret");
    let kept_button_id = kept_button.id;
    let kept = Widget::container()
        .hidden_from_automation()
        .with_child(kept_button);
    let ids = (
        label.id,
        field.id,
        boxed.id,
        middle.id,
        slider.id,
        area.id,
        progress.id,
        picture.id,
        pane.id,
        save.id,
        delete.id,
        kept.id,
    );
    let root = Widget::container()
        .with_flex_direction(FlexDirection::Column)
        .with_children(vec![
            label,
            field,
            boxed,
            middle,
            choices,
            slider,
            area,
            progress,
            picture,
            pane,
            save,
            delete,
            off_panel,
            hidden_panel,
            kept,
        ]);
    let mut tree = WidgetTree::new(root, 400.0, 900.0);
    tree.set_palette(Palette::for_mode(false));
    tree.layout();
    Form {
        tree,
        label: ids.0,
        field: ids.1,
        boxed: ids.2,
        middle: ids.3,
        small,
        large,
        slider: ids.4,
        area: ids.5,
        progress: ids.6,
        picture: ids.7,
        pane: ids.8,
        inside: inside_id,
        save: ids.9,
        delete: ids.10,
        held_off: held_off_id,
        hidden: hidden_id,
        kept: ids.11,
        kept_button: kept_button_id,
    }
}

fn node(tree: &WidgetTree, id: WidgetId) -> Node {
    tree.automation_node(id)
        .unwrap_or_else(|| panic!("{id:?} is not in the tree tools see"))
}

fn signals(tree: &mut WidgetTree) -> Vec<(WidgetId, SignalKind)> {
    tree.take_signals()
        .into_iter()
        .map(|s| (s.from, s.kind))
        .collect()
}

fn press_at(tree: &mut WidgetTree, x: f32, y: f32) {
    tree.handle_event(&Event::Mouse(MouseEvent {
        x,
        y,
        kind: MouseEventKind::Press(MouseButton::Left),
    }));
}

/// **A node says what its widget is, what it is called, and what it holds.**
#[test]
fn a_node_says_what_its_widget_is() {
    let f = form();
    let t = &f.tree;
    let root = t.automation().expect("the root is exposed");
    assert_eq!(root.role, Role::Group);

    let label = node(t, f.label);
    assert_eq!((label.role, label.name.as_str()), (Role::Label, "Name:"));
    assert_eq!(label.value, None);

    let field = node(t, f.field);
    assert_eq!(field.role, Role::TextField);
    assert_eq!(
        field.name, "Your name",
        "a field is called by its placeholder"
    );
    assert_eq!(field.key.as_deref(), Some("name"));
    assert_eq!(field.value, Some(Value::Text(String::new())));

    let boxed = node(t, f.boxed);
    assert_eq!(
        (boxed.role, boxed.name.as_str()),
        (Role::CheckBox, "Subscribe")
    );
    assert_eq!(boxed.value, Some(Value::Check(CheckState::Unchecked)));
    assert_eq!(
        node(t, f.middle).value,
        Some(Value::Check(CheckState::Indeterminate))
    );

    assert_eq!(node(t, f.small).value, Some(Value::Chosen(true)));
    assert_eq!(node(t, f.large).value, Some(Value::Chosen(false)));
    assert_eq!(node(t, f.large).role, Role::RadioButton);

    let slider = node(t, f.slider);
    assert_eq!(
        (slider.role, slider.name.as_str()),
        (Role::Slider, "Volume")
    );
    assert_eq!(
        slider.value,
        Some(Value::Range {
            value: 5.0,
            min: 0.0,
            max: 10.0
        })
    );

    let area = node(t, f.area);
    assert_eq!((area.role, area.name.as_str()), (Role::TextArea, "Notes"));

    let progress = node(t, f.progress);
    assert_eq!(progress.role, Role::ProgressBar);
    assert_eq!(
        progress.name, "Copying",
        "named by its tooltip, having no text"
    );
    assert_eq!(progress.description.as_deref(), Some("Copying"));
    assert_eq!(
        progress.value,
        Some(Value::Progress {
            value: 3.0,
            max: 10.0
        })
    );

    let picture = node(t, f.picture);
    assert_eq!((picture.role, picture.name.as_str()), (Role::Image, "Logo"));
    assert_eq!(node(t, f.pane).role, Role::ScrollPane);

    let save = node(t, f.save);
    assert_eq!(save.name, "Save", "its own text before its tooltip");
    assert_eq!(save.description.as_deref(), Some("Keep it"));
    assert_eq!(save.key.as_deref(), Some("save"));
    assert_eq!(Role::CheckBox.name(), "check box");
}

/// **A node is usable and shown only where all that holds it is**, as its
/// events are delivered; and it says whether it has the keyboard, and could.
#[test]
fn a_nodes_states_follow_what_holds_it() {
    let mut f = form();
    let t = &f.tree;
    assert!(node(t, f.save).enabled && node(t, f.save).shown);
    assert!(!node(t, f.delete).enabled);
    assert!(!node(t, f.held_off).enabled, "in a disabled panel");
    assert!(node(t, f.held_off).shown);
    assert!(!node(t, f.hidden).shown, "in a hidden panel");
    assert!(node(t, f.save).focusable);
    assert!(!node(t, f.label).focusable);
    assert!(!node(t, f.delete).focusable);
    assert!(!node(t, f.held_off).focusable);
    assert!(!node(t, f.field).focused);
    assert!(f.tree.focus(Some(f.field)));
    assert!(node(&f.tree, f.field).focused);
}

/// **A widget its program keeps from tools is not in the tree, nor what it
/// holds, and no action reaches it.**
#[test]
fn a_widget_kept_from_tools_is_out_of_the_tree() {
    let mut f = form();
    let tree = f.tree.automation().unwrap();
    assert!(tree.walk().all(|n| n.id != f.kept && n.id != f.kept_button));
    assert!(f.tree.automation_node(f.kept_button).is_none());
    let query = Query {
        name: Some("Secret".into()),
        ..Query::default()
    };
    assert!(f.tree.find(&query).is_empty());
    assert_eq!(
        f.tree.invoke(f.kept_button, Action::Press),
        Err(Refusal::NoSuchWidget)
    );
    assert!(signals(&mut f.tree).is_empty());
    // A root kept from tools keeps the whole tree.
    let tree = WidgetTree::new(Widget::container().hidden_from_automation(), 10.0, 10.0);
    assert_eq!(tree.automation(), None);
}

/// **A node's box is where its widget is drawn and clicked**: a press at
/// the middle of each box that could take the keyboard gives it the
/// keyboard -- the tree's own hit-testing agreeing with the boxes tools are
/// told -- and a scroll pane's contents move as it scrolls.
#[test]
fn a_nodes_box_is_where_its_widget_is() {
    let mut f = form();
    let tree = f.tree.automation().unwrap();
    // What the pane holds is cut to the pane's box, and its middle is below
    // it: it is pressed where it shows, below.
    let targets: Vec<(WidgetId, crate::frame::Rect)> = tree
        .walk()
        .filter(|n| n.focusable && n.id != f.inside)
        .map(|n| (n.id, n.bounds))
        .collect();
    assert!(targets.len() >= 8, "{}", targets.len());
    for (id, b) in targets {
        press_at(&mut f.tree, b.x + b.w / 2.0, b.y + b.h / 2.0);
        assert_eq!(f.tree.focused_id(), Some(id), "{b:?}");
    }
    // Scrolled, what the pane holds is drawn higher, and hit there.
    let pane = node(&f.tree, f.pane);
    let before = node(&f.tree, f.inside).bounds;
    assert!(before.y >= pane.bounds.y);
    f.tree
        .invoke(f.pane, Action::ScrollTo { x: 0.0, y: 30.0 })
        .unwrap();
    let after = node(&f.tree, f.inside).bounds;
    assert_eq!(after.y, before.y - 30.0);
    press_at(&mut f.tree, after.x + after.w / 2.0, pane.bounds.y + 10.0);
    assert_eq!(f.tree.focused_id(), Some(f.inside));
}

/// **A search finds by role, name, the program's name and text**, every
/// part given having to fit.
#[test]
fn a_search_finds_by_role_name_key_and_text() {
    let mut f = form();
    let buttons = f.tree.find(&Query {
        role: Some(Role::Button),
        ..Query::default()
    });
    assert!(buttons.contains(&f.save) && buttons.contains(&f.delete));
    assert!(!buttons.contains(&f.boxed));
    let by_name = Query {
        name: Some("SAVE".into()),
        ..Query::default()
    };
    assert_eq!(f.tree.find(&by_name), [f.save], "whatever the case");
    let by_key = Query {
        key: Some("name".into()),
        ..Query::default()
    };
    assert_eq!(f.tree.find(&by_key), [f.field]);
    f.tree
        .invoke(f.field, Action::SetText("Ada Lovelace".into()))
        .unwrap();
    let by_text = Query {
        text: Some("lovelace".into()),
        ..Query::default()
    };
    assert_eq!(f.tree.find(&by_text), [f.field]);
    let both = Query {
        role: Some(Role::Label),
        name: Some("Save".into()),
        ..Query::default()
    };
    assert!(f.tree.find(&both).is_empty(), "every part must fit");
}

/// **A button pressed is clicked, as its user clicks it -- and one its user
/// could not reach or use refuses, as do widgets a press is not for.**
#[test]
fn a_press_clicks_and_what_cannot_be_used_refuses() {
    let mut f = form();
    f.tree.invoke(f.save, Action::Press).unwrap();
    assert_eq!(signals(&mut f.tree), [(f.save, SignalKind::Clicked)]);
    assert_eq!(
        f.tree.invoke(f.delete, Action::Press),
        Err(Refusal::Disabled)
    );
    assert_eq!(
        f.tree.invoke(f.held_off, Action::Press),
        Err(Refusal::Disabled)
    );
    assert_eq!(f.tree.invoke(f.hidden, Action::Press), Err(Refusal::Hidden));
    let refused = f.tree.invoke(f.label, Action::Press).unwrap_err();
    assert_eq!(
        refused,
        Refusal::NotApplicable {
            role: Role::Label,
            action: "press"
        }
    );
    assert_eq!(refused.to_string(), "cannot press a label");
    assert_eq!(
        f.tree.invoke(WidgetId(u64::MAX), Action::Press),
        Err(Refusal::NoSuchWidget)
    );
    assert!(signals(&mut f.tree).is_empty(), "a refusal changed nothing");
}

/// **A box toggled and a choice chosen go as a click does**: a box in its
/// middle state is ticked, and choosing one of a group clears the rest.
#[test]
fn toggling_and_choosing_go_as_a_click_does() {
    let mut f = form();
    f.tree.invoke(f.boxed, Action::Toggle).unwrap();
    assert_eq!(
        node(&f.tree, f.boxed).value,
        Some(Value::Check(CheckState::Checked))
    );
    f.tree.invoke(f.middle, Action::Toggle).unwrap();
    assert_eq!(
        node(&f.tree, f.middle).value,
        Some(Value::Check(CheckState::Checked))
    );
    assert_eq!(
        signals(&mut f.tree),
        [
            (f.boxed, SignalKind::Toggled(CheckState::Checked)),
            (f.middle, SignalKind::Toggled(CheckState::Checked)),
        ]
    );
    f.tree.invoke(f.large, Action::Choose).unwrap();
    assert_eq!(node(&f.tree, f.large).value, Some(Value::Chosen(true)));
    assert_eq!(node(&f.tree, f.small).value, Some(Value::Chosen(false)));
    assert_eq!(signals(&mut f.tree), [(f.large, SignalKind::Chosen)]);
    f.tree.invoke(f.large, Action::Choose).unwrap();
    assert!(signals(&mut f.tree).is_empty(), "chosen already");
    assert!(matches!(
        f.tree.invoke(f.boxed, Action::Choose),
        Err(Refusal::NotApplicable { .. })
    ));
}

/// **Text set is an edit**: the field holds it with the caret after it, and
/// signals; the same text again changes nothing; a text area's is one edit
/// its user can undo.
#[test]
fn text_set_is_an_edit() {
    let mut f = form();
    f.tree
        .invoke(f.field, Action::SetText("Ada".into()))
        .unwrap();
    assert_eq!(
        node(&f.tree, f.field).value,
        Some(Value::Text("Ada".into()))
    );
    assert_eq!(signals(&mut f.tree), [(f.field, SignalKind::Edited)]);
    f.tree
        .invoke(f.field, Action::SetText("Ada".into()))
        .unwrap();
    assert!(signals(&mut f.tree).is_empty());
    let WidgetKind::TextInput { cursor, .. } = &f.tree.root.children[1].kind else {
        panic!("the field moved");
    };
    assert_eq!(cursor.byte(), 3, "the caret after what was set");

    f.tree
        .invoke(f.area, Action::SetText("line one\nline two".into()))
        .unwrap();
    assert_eq!(signals(&mut f.tree), [(f.area, SignalKind::Edited)]);
    let WidgetKind::TextArea { area, .. } = &mut f.tree.root.children[6].kind else {
        panic!("the area moved");
    };
    assert_eq!(area.text(), "line one\nline two");
    assert!(area.undo(), "an edit its user can take back");
    assert_eq!(area.text(), "");
    assert!(matches!(
        f.tree.invoke(f.save, Action::SetText("x".into())),
        Err(Refusal::NotApplicable { .. })
    ));
}

/// **A slider is set within its bounds, on its steps; a value that is no
/// number is refused.**
#[test]
fn a_slider_is_set_within_its_bounds() {
    let mut f = form();
    f.tree.invoke(f.slider, Action::SetValue(7.0)).unwrap();
    assert_eq!(signals(&mut f.tree), [(f.slider, SignalKind::Moved(7.0))]);
    f.tree.invoke(f.slider, Action::SetValue(99.0)).unwrap();
    assert_eq!(
        node(&f.tree, f.slider).value,
        Some(Value::Range {
            value: 10.0,
            min: 0.0,
            max: 10.0
        })
    );
    assert_eq!(
        f.tree.invoke(f.slider, Action::SetValue(f64::NAN)),
        Err(Refusal::NotANumber)
    );
    f.tree.invoke(f.slider, Action::SetValue(10.0)).unwrap();
    let moved: Vec<_> = signals(&mut f.tree);
    assert_eq!(
        moved,
        [(f.slider, SignalKind::Moved(10.0))],
        "not again at 10"
    );
}

/// **The keyboard is given only to what takes it; a pane scrolls only as far
/// as it goes.**
#[test]
fn focus_and_scroll() {
    let mut f = form();
    f.tree.invoke(f.save, Action::Focus).unwrap();
    assert_eq!(f.tree.focused_id(), Some(f.save));
    assert!(node(&f.tree, f.save).focused);
    assert!(matches!(
        f.tree.invoke(f.label, Action::Focus),
        Err(Refusal::NotApplicable {
            role: Role::Label,
            ..
        })
    ));
    assert_eq!(
        f.tree.focused_id(),
        Some(f.save),
        "a refusal kept the focus"
    );
    f.tree
        .invoke(f.pane, Action::ScrollTo { x: 0.0, y: 1.0e6 })
        .unwrap();
    let WidgetKind::ScrollView {
        scroll_y,
        content_height,
        ..
    } = &f.tree.root.children[9].kind
    else {
        panic!("the pane moved");
    };
    let shown = f.tree.root.children[9].layout.height;
    assert_eq!(*scroll_y, content_height - shown, "as far as it goes");
    assert_eq!(
        f.tree.invoke(
            f.pane,
            Action::ScrollTo {
                x: 0.0,
                y: f32::NAN
            }
        ),
        Err(Refusal::NotANumber)
    );
}

/// **An action is followed as an event is**: the tree is styled again for
/// the state a style hangs on, and every listener hears the signal.
#[test]
fn an_action_restyles_the_tree_and_reaches_its_listeners() {
    let mut f = form();
    let _ = f
        .tree
        .set_style_sheet("Checkbox:checked { padding-top: 40px; }");
    f.tree.layout();
    let heard: Rc<RefCell<Vec<Signal>>> = Rc::default();
    let into = Rc::clone(&heard);
    let _slot = f
        .tree
        .connect(Some(f.boxed), move |s| into.borrow_mut().push(s.clone()));
    let before = node(&f.tree, f.boxed).bounds;
    f.tree.invoke(f.boxed, Action::Toggle).unwrap();
    let after = node(&f.tree, f.boxed).bounds;
    // Taller by the padding: across, the column stretches it whatever its
    // padding.
    assert_eq!(after.h, before.h + 40.0, "styled again for :checked");
    assert_eq!(
        heard.borrow().as_slice(),
        [Signal {
            from: f.boxed,
            kind: SignalKind::Toggled(CheckState::Checked)
        }]
    );
}

/// **A walk visits every node once, in order.**
#[test]
fn a_walk_visits_every_node_in_order() {
    let f = form();
    let tree = f.tree.automation().unwrap();
    let ids: Vec<WidgetId> = tree.walk().map(|n| n.id).collect();
    let mut unique = ids.clone();
    unique.sort_by_key(|id| id.0);
    unique.dedup();
    assert_eq!(unique.len(), ids.len());
    assert_eq!(ids[0], tree.id);
    let label = ids.iter().position(|&id| id == f.label).unwrap();
    let save = ids.iter().position(|&id| id == f.save).unwrap();
    assert!(label < save, "the tree's order");
}
