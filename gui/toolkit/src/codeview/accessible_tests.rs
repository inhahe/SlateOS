//! Tests for the code view as tools see it: the code, set as a paste over
//! all of it and scrolled as the scrollbar's thumb drags it; and while it is
//! open, the find bar's fields, typed into, its switches, clicked, and what
//! it says of the matches.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_cmp
)]

use super::*;
use crate::event::{Key, KeyEvent, Modifiers};

const BOUNDS: Rect = Rect {
    x: 10.0,
    y: 20.0,
    w: 600.0,
    h: 300.0,
};

fn view(text: &str) -> CodeView {
    let mut v = CodeView::new(CodeEditor::from_text(text));
    v.set_bounds(BOUNDS);
    v.set_focused(true);
    v
}

fn act(v: &mut CodeView, part: CodePart, action: Action) -> Result<Option<CodeViewEvent>, Refusal> {
    v.invoke(&part, action, 0.0, 0.0)
}

fn node(v: &CodeView, part: CodePart) -> Option<Node<CodePart>> {
    v.automation(0.0, 0.0)
        .walk()
        .find(|n| n.id == part)
        .cloned()
}

fn text_of(v: &CodeView, part: CodePart) -> Option<Value> {
    node(v, part).and_then(|n| n.value)
}

fn matches_said(v: &CodeView) -> Option<String> {
    node(v, CodePart::Matches).map(|n| n.name)
}

/// **The code is a text area holding all of it, with the keyboard where the
/// view has it** -- and while the find bar is closed, it is all there is:
/// the bar's parts are none.
#[test]
fn the_code_is_a_text_area_holding_all_of_it() {
    let mut v = view("fn main() {}\n");
    let root = v.automation(0.0, 0.0);
    assert_eq!(
        (root.role, root.name.as_str(), root.bounds),
        (Role::Group, "Code editor", BOUNDS)
    );
    assert_eq!(root.children.len(), 1, "the code alone: {root:?}");
    let code = &root.children[0];
    assert_eq!((code.role, code.name.as_str()), (Role::TextArea, "Code"));
    assert_eq!(code.value, Some(Value::Text("fn main() {}\n".to_owned())));
    assert_eq!(code.bounds, BOUNDS, "all of the view, with no bar over it");
    assert!(code.focused && code.focusable);

    v.set_focused(false);
    assert!(
        !node(&v, CodePart::Code).unwrap().focused,
        "the host's to say"
    );
    for part in [
        CodePart::FindBar,
        CodePart::Find,
        CodePart::Replace,
        CodePart::MatchCase,
        CodePart::Matches,
    ] {
        assert!(node(&v, part).is_none(), "{part:?}");
        assert_eq!(
            act(&mut v, part, Action::Focus),
            Err(Refusal::NoSuchWidget),
            "{part:?}"
        );
    }
}

/// **The code set is a paste over all of it: one edit, which one Ctrl+Z
/// takes back** -- none where it already says so; and set to nothing, it is
/// emptied.
#[test]
fn the_code_set_is_one_edit_undone_by_one_undo() {
    let mut v = view("old text");
    assert_eq!(
        act(
            &mut v,
            CodePart::Code,
            Action::SetText("new\ntext".to_owned())
        ),
        Ok(Some(CodeViewEvent::Changed))
    );
    assert_eq!(v.editor().buffer().text(), "new\ntext");
    assert_eq!(
        act(
            &mut v,
            CodePart::Code,
            Action::SetText("new\ntext".to_owned())
        ),
        Ok(None),
        "already so"
    );
    let undo = KeyEvent {
        key: Key::Z,
        pressed: true,
        modifiers: Modifiers {
            ctrl: true,
            ..Modifiers::NONE
        },
        text: String::new(),
    };
    v.handle_key(&undo);
    assert_eq!(v.editor().buffer().text(), "old text", "taken back whole");

    assert_eq!(
        act(&mut v, CodePart::Code, Action::SetText(String::new())),
        Ok(Some(CodeViewEvent::Changed))
    );
    assert!(v.editor().buffer().is_empty());
}

/// **The open find bar shows its fields, its switches and what it says of
/// the matches** -- the field with the keyboard focused, the code then not;
/// what to find set is typed, and the view goes to the first match from the
/// caret, as typing takes it there.
#[test]
fn the_find_bar_shows_its_fields_switches_and_matches() {
    let mut v = view("one two one");
    v.open_find(true);
    let bar = node(&v, CodePart::FindBar).expect("open");
    let parts: Vec<(CodePart, Role, &str)> = bar
        .children
        .iter()
        .map(|n| (n.id, n.role, n.name.as_str()))
        .collect();
    assert_eq!(
        parts,
        [
            (CodePart::Find, Role::TextField, "Find"),
            (CodePart::Replace, Role::TextField, "Replace with"),
            (CodePart::MatchCase, Role::CheckBox, "Match case"),
            (CodePart::WholeWord, Role::CheckBox, "Whole word"),
            (CodePart::Regex, Role::CheckBox, "Regular expression"),
        ],
        "nothing said of matches while there is nothing to find"
    );
    assert!(bar.children[0].focused && !bar.children[1].focused);
    assert!(!node(&v, CodePart::Code).unwrap().focused);
    v.set_focused(false);
    assert!(
        !node(&v, CodePart::Find).unwrap().focused,
        "not while the view itself is without the keyboard"
    );
    v.set_focused(true);
    assert!(
        node(&v, CodePart::Code).unwrap().bounds.y >= bar.bounds.bottom(),
        "the code below the bar"
    );
    assert_eq!(
        text_of(&v, CodePart::MatchCase),
        Some(Value::Check(CheckState::Unchecked))
    );

    assert_eq!(
        act(&mut v, CodePart::Find, Action::SetText("one".to_owned())),
        Ok(Some(CodeViewEvent::Moved))
    );
    assert_eq!(
        text_of(&v, CodePart::Find),
        Some(Value::Text("one".to_owned()))
    );
    assert_eq!(v.editor().primary().range(), 0..3, "on the first match");
    assert_eq!(matches_said(&v).as_deref(), Some("1 of 2"));
    assert_eq!(
        act(&mut v, CodePart::Find, Action::SetText("one".to_owned())),
        Ok(None),
        "already so"
    );
}

/// **A switch pressed is clicked, and the search runs again**: matching
/// case, one of two is left. What is not a switch's to do is refused.
#[test]
fn a_switch_is_clicked_and_the_search_runs_again() {
    let mut v = view("One one");
    v.open_find(false);
    act(&mut v, CodePart::Find, Action::SetText("one".to_owned())).unwrap();
    assert_eq!(matches_said(&v).as_deref(), Some("1 of 2"));

    assert_eq!(
        act(&mut v, CodePart::MatchCase, Action::Toggle),
        Ok(Some(CodeViewEvent::Moved))
    );
    assert_eq!(
        text_of(&v, CodePart::MatchCase),
        Some(Value::Check(CheckState::Checked))
    );
    assert_eq!(matches_said(&v).as_deref(), Some("1 of 1"));
    assert_eq!(v.editor().primary().range(), 4..7, "on the one left");

    assert_eq!(
        act(&mut v, CodePart::WholeWord, Action::Press),
        Ok(Some(CodeViewEvent::Moved))
    );
    assert_eq!(
        text_of(&v, CodePart::WholeWord),
        Some(Value::Check(CheckState::Checked))
    );
    assert_eq!(
        act(&mut v, CodePart::Regex, Action::Focus),
        Err(Refusal::NotApplicable {
            role: Role::CheckBox,
            action: "focus"
        })
    );
}

/// **What to replace a match with, set, is typed -- and moves nothing**, as
/// typing there is no new search; the field is none while its row is
/// hidden.
#[test]
fn what_to_replace_with_set_moves_nothing() {
    let mut v = view("a b a");
    v.open_find(true);
    act(&mut v, CodePart::Find, Action::SetText("a".to_owned())).unwrap();
    let on = v.editor().primary().range();
    assert_eq!(
        act(&mut v, CodePart::Replace, Action::SetText("z".to_owned())),
        Ok(Some(CodeViewEvent::Moved))
    );
    assert_eq!(
        text_of(&v, CodePart::Replace),
        Some(Value::Text("z".to_owned()))
    );
    assert_eq!(v.editor().primary().range(), on);
    assert_eq!(v.editor().buffer().text(), "a b a", "nothing replaced");

    v.close_find();
    v.open_find(false);
    assert!(node(&v, CodePart::Replace).is_none());
    assert_eq!(
        act(&mut v, CodePart::Replace, Action::SetText("y".to_owned())),
        Err(Refusal::NoSuchWidget)
    );
}

/// **A field given the keyboard is clicked into; the code given it is
/// clicked on**, and the bar lets it go.
#[test]
fn the_keyboard_goes_where_a_click_puts_it() {
    let mut v = view("x");
    v.open_find(true);
    let focused = |v: &CodeView, part| node(v, part).unwrap().focused;
    assert_eq!(
        act(&mut v, CodePart::Replace, Action::Focus),
        Ok(Some(CodeViewEvent::Moved))
    );
    assert!(focused(&v, CodePart::Replace) && !focused(&v, CodePart::Find));
    assert_eq!(
        act(&mut v, CodePart::Code, Action::Focus),
        Ok(Some(CodeViewEvent::Moved))
    );
    assert!(focused(&v, CodePart::Code) && !focused(&v, CodePart::Replace));
    act(&mut v, CodePart::Find, Action::Focus).unwrap();
    assert!(focused(&v, CodePart::Find) && !focused(&v, CodePart::Code));
}

/// **The code is scrolled as the scrollbar's thumb drags it**: down by
/// whole lines, as far as the last screenful; across, where long lines are
/// not wrapped, as far as the widest goes; a place that is no number is
/// refused.
#[test]
fn the_code_is_scrolled_as_its_thumb_drags_it() {
    let text = (0..100)
        .map(|n| format!("line {n}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut v = view(&text);
    let line = v.line_height();
    let to = |x, y| Action::ScrollTo { x, y };
    assert_eq!(
        act(&mut v, CodePart::Code, to(0.0, line * 10.5)),
        Ok(Some(CodeViewEvent::Moved))
    );
    assert_eq!(v.top_line(), 10);
    act(&mut v, CodePart::Code, to(0.0, 1.0e9)).unwrap();
    assert_eq!(v.top_line(), 100 - v.visible_rows(), "the last screenful");
    assert_eq!(
        act(&mut v, CodePart::Code, to(0.0, f32::NAN)),
        Err(Refusal::NotANumber)
    );
    assert_eq!(v.top_line(), 100 - v.visible_rows(), "nothing moved");

    let mut wide = view(&"x".repeat(500));
    act(&mut wide, CodePart::Code, to(1.0e6, 0.0)).unwrap();
    let rows = wide.rows_of(0);
    let widest = wide.x_in_row(&rows[0], rows[0].range.end);
    assert!(wide.scroll_x > 0.0);
    assert_eq!(
        wide.scroll_x,
        widest - wide.text_rect().w,
        "as far as it goes"
    );
    act(&mut wide, CodePart::Code, to(-5.0, 0.0)).unwrap();
    assert_eq!(wide.scroll_x, 0.0);
}

/// **What is not a part's to do is refused.**
#[test]
fn what_a_part_cannot_do_is_refused() {
    let mut v = view("abc abc");
    v.open_find(false);
    act(&mut v, CodePart::Find, Action::SetText("abc".to_owned())).unwrap();
    let refused = |role, action| Err(Refusal::NotApplicable { role, action });
    assert_eq!(
        act(&mut v, CodePart::Code, Action::Press),
        refused(Role::TextArea, "press")
    );
    assert_eq!(
        act(&mut v, CodePart::Find, Action::Toggle),
        refused(Role::TextField, "toggle")
    );
    assert_eq!(
        act(&mut v, CodePart::Matches, Action::Press),
        refused(Role::Label, "press")
    );
    assert_eq!(
        act(&mut v, CodePart::FindBar, Action::Press),
        refused(Role::Group, "press")
    );
    assert_eq!(
        act(&mut v, CodePart::View, Action::Focus),
        refused(Role::Group, "focus")
    );
}
