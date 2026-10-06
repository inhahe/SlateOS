//! Tests for a rich field as tools see it: a text area under its toolbar,
//! its text set as a paste over all of it, each button pressed as the
//! toolbar's click; and a disabled field refusing.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_cmp
)]

use super::*;
use crate::richinput::{Format, RichDoc};

const BOLD: RichInputPart = RichInputPart::Tool(Tool::Switch(Toggle::Bold));
const ITALIC: RichInputPart = RichInputPart::Tool(Tool::Switch(Toggle::Italic));

fn field(text: &str) -> RichInput {
    RichInput::with_doc(RichDoc::plain(text, Format::default()))
}

/// The field 300 by 120 at (10, 50), its toolbar -- where `bar` -- above it
/// at (10, 10).
fn access<'a>(
    input: &'a mut RichInput,
    palette: &'a Palette,
    bar: bool,
    enabled: bool,
) -> RichInputAccess<'a> {
    RichInputAccess {
        input,
        name: "Message",
        look: Look {
            rect: (10.0, 50.0, 300.0, 120.0),
            focused: true,
            placeholder: "Write here",
        },
        toolbar: bar.then_some(((10.0, 10.0), palette)),
        base: 13.0,
        enabled,
    }
}

fn act(
    tool: &mut RichInputAccess<'_>,
    part: RichInputPart,
    action: Action,
) -> Result<Option<KeyEdit>, Refusal> {
    tool.invoke(&part, action, 0.0, 0.0)
}

fn node(tool: &RichInputAccess<'_>, part: RichInputPart) -> Option<Node<RichInputPart>> {
    tool.automation(0.0, 0.0)
        .walk()
        .find(|n| n.id == part)
        .cloned()
}

/// **A rich field is a text area holding its text, named by its label,
/// under its toolbar** -- each switch a check box, ticked where the field
/// shows its format, and its buttons -- all where its host draws them.
#[test]
fn a_rich_field_is_a_text_area_under_its_toolbar() {
    let palette = Palette::for_mode(false);
    let mut input = field("hello");
    let tool = access(&mut input, &palette, true, true);
    let root = tool.automation(0.0, 0.0);
    assert_eq!((root.role, root.name.as_str()), (Role::Group, "Message"));
    let bar = &root.children[0];
    assert_eq!((bar.id, bar.role), (RichInputPart::Toolbar, Role::Group));
    let buttons: Vec<(&str, Role, Option<Value>)> = bar
        .children
        .iter()
        .map(|n| (n.name.as_str(), n.role, n.value.clone()))
        .collect();
    let off = Some(Value::Check(CheckState::Unchecked));
    assert_eq!(
        buttons,
        [
            ("Bold", Role::CheckBox, off.clone()),
            ("Italic", Role::CheckBox, off.clone()),
            ("Underline", Role::CheckBox, off.clone()),
            ("Strikethrough", Role::CheckBox, off),
            ("Smaller text", Role::Button, None),
            ("Larger text", Role::Button, None),
            ("Clear formatting", Role::Button, None),
        ]
    );
    let first = toolbar::layout(&palette, (10.0, 10.0))[0].1;
    assert_eq!(
        bar.children[0].bounds,
        Rect::new(first.0, first.1, first.2, first.3),
        "where the toolbar draws it"
    );

    let text = &root.children[1];
    assert_eq!(
        (text.id, text.role, text.name.as_str()),
        (RichInputPart::Field, Role::TextArea, "Message")
    );
    assert_eq!(text.value, Some(Value::Text("hello".to_owned())));
    assert_eq!(text.bounds, Rect::new(10.0, 50.0, 300.0, 120.0));
    assert!(text.focused && text.focusable && text.enabled);
    let right = toolbar::layout(&palette, (10.0, 10.0))
        .iter()
        .map(|(_, (x, _, w, _))| x + w)
        .fold(310.0_f32, f32::max);
    assert_eq!(
        root.bounds,
        Rect::new(10.0, 10.0, right - 10.0, 160.0),
        "round both"
    );
}

/// **A field its host draws without a toolbar is the field alone**, called
/// by what it shows while empty where it has no label; the toolbar's parts
/// are none.
#[test]
fn a_field_without_a_toolbar_is_the_field_alone() {
    let palette = Palette::for_mode(false);
    let mut input = field("");
    let mut tool = access(&mut input, &palette, false, true);
    tool.name = "";
    let root = tool.automation(0.0, 0.0);
    assert_eq!(root.children.len(), 1);
    assert_eq!(root.children[0].name, "Write here");
    assert_eq!(root.bounds, root.children[0].bounds);
    for part in [RichInputPart::Toolbar, BOLD] {
        assert!(node(&tool, part).is_none());
        assert_eq!(
            act(&mut tool, part, Action::Press),
            Err(Refusal::NoSuchWidget)
        );
    }
}

/// **Its text set is a paste of plain text over all of it: one step, which
/// one undo takes back** -- none where it already says so; set to nothing,
/// it is emptied.
#[test]
fn its_text_set_is_one_step_undone_by_one_undo() {
    let palette = Palette::for_mode(false);
    let mut input = field("old");
    {
        let mut tool = access(&mut input, &palette, false, true);
        assert_eq!(
            act(
                &mut tool,
                RichInputPart::Field,
                Action::SetText("new words".to_owned())
            ),
            Ok(Some(KeyEdit::Changed))
        );
        assert_eq!(tool.input.text(), "new words");
        assert_eq!(
            act(
                &mut tool,
                RichInputPart::Field,
                Action::SetText("new words".to_owned())
            ),
            Ok(None),
            "already so"
        );
    }
    assert!(input.undo());
    assert_eq!(input.text(), "old", "taken back whole");

    let mut tool = access(&mut input, &palette, false, true);
    assert_eq!(
        act(
            &mut tool,
            RichInputPart::Field,
            Action::SetText(String::new())
        ),
        Ok(Some(KeyEdit::Changed))
    );
    assert_eq!(tool.input.text(), "");
}

/// **A switch pressed is the toolbar's click**: over the selection it
/// changes the text's format, and is ticked; with nothing selected it sets
/// what typing takes next, the text unchanged.
#[test]
fn a_switch_is_the_toolbars_click() {
    let palette = Palette::for_mode(false);
    let mut input = field("bold me");
    input.select_all();
    let mut tool = access(&mut input, &palette, true, true);
    assert_eq!(
        act(&mut tool, BOLD, Action::Toggle),
        Ok(Some(KeyEdit::Changed))
    );
    assert!(tool.input.doc().format_at(0).bold);
    assert_eq!(
        node(&tool, BOLD).unwrap().value,
        Some(Value::Check(CheckState::Checked))
    );

    tool.input.set_cursor(0);
    assert_eq!(
        act(&mut tool, ITALIC, Action::Press),
        Ok(Some(KeyEdit::Handled))
    );
    assert!(!tool.input.doc().format_at(0).italic, "the text as it was");
    assert!(
        tool.input.shown_has(Toggle::Italic),
        "for what is typed next"
    );
}

/// **A size button steps the selection's size, and clearing takes every
/// format off**, as their clicks do.
#[test]
fn the_size_and_clearing_buttons_are_clicked() {
    let palette = Palette::for_mode(false);
    let mut input = field("words");
    input.select_all();
    let mut tool = access(&mut input, &palette, true, true);
    let larger = RichInputPart::Tool(Tool::Larger);
    assert_eq!(
        act(&mut tool, larger, Action::Press),
        Ok(Some(KeyEdit::Changed))
    );
    assert_eq!(
        tool.input.doc().format_at(0).size,
        Some(15.0),
        "the next size up"
    );
    act(&mut tool, BOLD, Action::Press).unwrap();
    assert_eq!(
        act(&mut tool, RichInputPart::Tool(Tool::Clear), Action::Press),
        Ok(Some(KeyEdit::Changed))
    );
    let format = tool.input.doc().format_at(0);
    assert!(!format.bold && format.size.is_none(), "{format:?}");
    assert_eq!(
        act(&mut tool, larger, Action::Toggle),
        Err(Refusal::NotApplicable {
            role: Role::Button,
            action: "toggle"
        })
    );
}

/// **A field its host has disabled says so, and refuses**; what is not a
/// part's to do is refused whatever -- its keyboard among it, which is its
/// host's to give.
#[test]
fn a_disabled_field_refuses() {
    let palette = Palette::for_mode(false);
    let mut input = field("fixed");
    let mut tool = access(&mut input, &palette, true, false);
    let root = tool.automation(0.0, 0.0);
    assert!(root.walk().all(|n| !n.enabled), "{root:?}");
    assert!(!node(&tool, RichInputPart::Field).unwrap().focusable);
    assert_eq!(
        act(
            &mut tool,
            RichInputPart::Field,
            Action::SetText("x".to_owned())
        ),
        Err(Refusal::Disabled)
    );
    assert_eq!(act(&mut tool, BOLD, Action::Toggle), Err(Refusal::Disabled));
    assert_eq!(tool.input.text(), "fixed");
    assert_eq!(
        act(&mut tool, RichInputPart::Field, Action::Focus),
        Err(Refusal::NotApplicable {
            role: Role::TextArea,
            action: "focus"
        })
    );
    assert_eq!(
        act(&mut tool, RichInputPart::Whole, Action::Press),
        Err(Refusal::NotApplicable {
            role: Role::Group,
            action: "press"
        })
    );
}
