//! Tests for a drop-down as tools see it: a combo box holding its choice,
//! opened and folded away as its field is clicked, an option chosen as its
//! row is clicked -- scrolled into a list too long for the screen first --
//! and one its host has disabled refusing.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use super::*;

/// Where the host draws the field.
const FIELD: Rect = Rect {
    x: 100.0,
    y: 100.0,
    w: 160.0,
    h: 26.0,
};

/// Red, Green (chosen), Blue.
fn colours() -> Dropdown {
    Dropdown::new(
        vec!["Red".to_owned(), "Green".to_owned(), "Blue".to_owned()],
        Some(1),
    )
}

fn access(dropdown: &mut Dropdown, viewport: (f32, f32), enabled: bool) -> DropdownAccess<'_> {
    DropdownAccess {
        dropdown,
        name: "Colour",
        field: FIELD,
        viewport,
        enabled,
    }
}

fn act(
    tool: &mut DropdownAccess<'_>,
    part: DropdownPart,
    action: Action,
) -> Result<Option<DropdownEvent>, Refusal> {
    tool.invoke(&part, action, 0.0, 0.0)
}

/// **A drop-down is a combo box named by its label and holding its choice;
/// pressed, its list opens -- each option by its label, the chosen one
/// chosen and with the keyboard -- and an option chosen is clicked and
/// becomes the choice.** Pressed again, the list folds away.
#[test]
fn a_dropdown_is_opened_and_an_option_chosen() {
    let mut dropdown = colours();
    let mut tool = access(&mut dropdown, (800.0, 600.0), true);
    let root = tool.automation(0.0, 0.0);
    assert_eq!((root.role, root.name.as_str()), (Role::ComboBox, "Colour"));
    assert_eq!(root.value, Some(Value::Text("Green".to_owned())));
    assert_eq!(root.bounds, FIELD);
    assert!(root.children.is_empty(), "closed, it shows no list");
    assert_eq!(
        act(&mut tool, DropdownPart::Option(2), Action::Choose),
        Err(Refusal::Hidden),
        "the list is opened first"
    );

    assert_eq!(
        act(&mut tool, DropdownPart::Field, Action::Press),
        Ok(Some(DropdownEvent::Opened))
    );
    let list = tool.automation(0.0, 0.0).children[0].clone();
    assert_eq!((list.role, list.id), (Role::List, DropdownPart::List));
    let options: Vec<(&str, Option<Value>, bool)> = list
        .children
        .iter()
        .map(|n| (n.name.as_str(), n.value.clone(), n.focused))
        .collect();
    assert_eq!(
        options,
        [
            ("Red", Some(Value::Chosen(false)), false),
            ("Green", Some(Value::Chosen(true)), true),
            ("Blue", Some(Value::Chosen(false)), false),
        ]
    );
    for (index, option) in list.children.iter().enumerate() {
        assert_eq!(
            Some(option.bounds),
            tool.dropdown.list_item_rect(index),
            "{}",
            option.name
        );
    }
    assert_eq!(
        act(&mut tool, DropdownPart::Option(2), Action::Choose),
        Ok(Some(DropdownEvent::Selected(2)))
    );
    assert!(!tool.dropdown.is_open());
    assert_eq!(tool.dropdown.selected(), Some(2));
    assert_eq!(
        act(&mut tool, DropdownPart::Option(3), Action::Choose),
        Err(Refusal::NoSuchWidget)
    );

    act(&mut tool, DropdownPart::Field, Action::Press).unwrap();
    assert_eq!(
        act(&mut tool, DropdownPart::Field, Action::Press),
        Ok(Some(DropdownEvent::Closed)),
        "pressed again, it folds away"
    );
    assert_eq!(tool.dropdown.selected(), Some(2));
}

/// **An option out of a list too long for the screen is listed where it
/// would be drawn, and chosen it is scrolled into the list first.**
#[test]
fn an_option_out_of_a_long_list_is_scrolled_in_and_chosen() {
    let mut dropdown = Dropdown::new((0..40).map(|n| format!("Option {n}")).collect(), None)
        .with_placeholder("Pick one");
    let mut tool = access(&mut dropdown, (800.0, 300.0), true);
    let root = tool.automation(0.0, 0.0);
    assert_eq!(root.value, None, "nothing chosen");
    assert_eq!(root.description.as_deref(), Some("Pick one"));

    act(&mut tool, DropdownPart::Field, Action::Press).unwrap();
    let list = tool.automation(0.0, 0.0).children[0].clone();
    assert_eq!(list.children.len(), 40, "every option, shown or not");
    let last = &list.children[39];
    assert!(
        last.bounds.y >= list.bounds.bottom(),
        "below the list: {:?} under {:?}",
        last.bounds,
        list.bounds
    );
    assert_eq!(
        act(&mut tool, DropdownPart::Option(39), Action::Choose),
        Ok(Some(DropdownEvent::Selected(39)))
    );
}

/// **A drop-down its host has disabled says so and refuses**; and what is
/// not done to a part is refused as such.
#[test]
fn a_disabled_dropdown_refuses() {
    let mut dropdown = colours();
    let mut tool = access(&mut dropdown, (800.0, 600.0), false);
    assert!(!tool.automation(0.0, 0.0).enabled);
    assert_eq!(
        act(&mut tool, DropdownPart::Field, Action::Press),
        Err(Refusal::Disabled)
    );
    assert!(!tool.dropdown.is_open());

    tool.enabled = true;
    assert_eq!(
        act(&mut tool, DropdownPart::Field, Action::Toggle),
        Err(Refusal::NotApplicable {
            role: Role::ComboBox,
            action: "toggle"
        })
    );
    act(&mut tool, DropdownPart::Field, Action::Press).unwrap();
    assert_eq!(
        act(&mut tool, DropdownPart::List, Action::Press),
        Err(Refusal::NotApplicable {
            role: Role::List,
            action: "press"
        })
    );
    assert_eq!(
        act(&mut tool, DropdownPart::Option(0), Action::Toggle),
        Err(Refusal::NotApplicable {
            role: Role::ListItem,
            action: "toggle"
        })
    );
}
