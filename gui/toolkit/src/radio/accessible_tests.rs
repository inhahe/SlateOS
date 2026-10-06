//! Tests for a radio group as tools see it: its options, the chosen one
//! chosen; one chosen or pressed as clicked; going back to no choice only
//! where the group allows it; a disabled group refusing.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use super::*;

const OPTIONS: [(&str, Rect); 3] = [
    (
        "Small",
        Rect {
            x: 10.0,
            y: 10.0,
            w: 80.0,
            h: 20.0,
        },
    ),
    (
        "Medium",
        Rect {
            x: 10.0,
            y: 34.0,
            w: 80.0,
            h: 20.0,
        },
    ),
    (
        "Large",
        Rect {
            x: 10.0,
            y: 58.0,
            w: 80.0,
            h: 20.0,
        },
    ),
];

fn access(group: &mut RadioGroup, enabled: bool) -> RadioAccess<'_> {
    RadioAccess {
        group,
        name: "Size",
        options: &OPTIONS,
        enabled,
    }
}

fn act(
    tool: &mut RadioAccess<'_>,
    part: RadioPart,
    action: Action,
) -> Result<Option<RadioEvent>, Refusal> {
    tool.invoke(&part, action, 0.0, 0.0)
}

/// **A radio group shows tools its options as radio buttons, the chosen one
/// chosen and the keyboard's focused; an option chosen is chosen as a click
/// chooses it**, and choosing the chosen one changes nothing.
#[test]
fn an_option_is_chosen_as_clicked() {
    let mut group = RadioGroup::new(3, Some(0));
    let mut tool = access(&mut group, true);
    let root = tool.automation(0.0, 0.0);
    assert_eq!((root.role, root.name.as_str()), (Role::Group, "Size"));
    assert_eq!(
        root.bounds,
        Rect::new(10.0, 10.0, 80.0, 68.0),
        "round its options"
    );
    let options: Vec<(&str, Option<Value>, bool)> = root
        .children
        .iter()
        .map(|n| (n.name.as_str(), n.value.clone(), n.focused))
        .collect();
    assert_eq!(
        options,
        [
            ("Small", Some(Value::Chosen(true)), true),
            ("Medium", Some(Value::Chosen(false)), false),
            ("Large", Some(Value::Chosen(false)), false),
        ]
    );
    assert_eq!(root.children[1].role, Role::RadioButton);

    assert_eq!(
        act(&mut tool, RadioPart::Option(2), Action::Choose),
        Ok(Some(RadioEvent::Selected(2)))
    );
    assert_eq!(tool.group.selected(), Some(2));
    assert_eq!(tool.group.focus(), 2, "the keyboard follows the click");
    assert_eq!(
        act(&mut tool, RadioPart::Option(2), Action::Choose),
        Ok(None)
    );
    assert_eq!(
        act(&mut tool, RadioPart::Option(2), Action::Press),
        Ok(None),
        "a plain group's click on the chosen does nothing"
    );
    assert_eq!(
        act(&mut tool, RadioPart::Option(3), Action::Choose),
        Err(Refusal::NoSuchWidget)
    );
    assert_eq!(
        act(&mut tool, RadioPart::Option(0), Action::Toggle),
        Err(Refusal::NotApplicable {
            role: Role::RadioButton,
            action: "toggle"
        })
    );
}

/// **In a group that allows no choice, the chosen option pressed is cleared,
/// as a click on it clears it** -- and chosen, it stays chosen.
#[test]
fn a_deselectable_groups_choice_is_cleared_by_a_press() {
    let mut group = RadioGroup::new(3, Some(1)).deselectable(true);
    let mut tool = access(&mut group, true);
    assert_eq!(
        act(&mut tool, RadioPart::Option(1), Action::Choose),
        Ok(None)
    );
    assert_eq!(tool.group.selected(), Some(1), "choosing is not clearing");
    assert_eq!(
        act(&mut tool, RadioPart::Option(1), Action::Press),
        Ok(Some(RadioEvent::Cleared))
    );
    assert_eq!(tool.group.selected(), None);
}

/// **A group its host has disabled says so and refuses.**
#[test]
fn a_disabled_group_refuses() {
    let mut group = RadioGroup::new(3, None);
    let mut tool = access(&mut group, false);
    let root = tool.automation(0.0, 0.0);
    assert!(!root.enabled && root.children.iter().all(|n| !n.enabled));
    assert_eq!(
        act(&mut tool, RadioPart::Option(0), Action::Choose),
        Err(Refusal::Disabled)
    );
    assert_eq!(tool.group.selected(), None);
}
