//! Tests for a ribbon as tools see it: its tabs, groups and controls, each
//! used as clicked; a menu opened from an arrow and a row of it chosen; a
//! drop-down's and a gallery's choices made; a folded group opened; and what
//! a press would not reach refused.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use super::super::{ButtonSize, Choice, Command, Group, RibbonTab};
use super::*;
use crate::menu::{MenuItem, MenuItemId};

const PASTE: CommandId = 1;
const PASTE_SPECIAL: MenuItemId = 101;
const PASTE_TEXT: MenuItemId = 102;
const CUT: CommandId = 2;
const FONT: CommandId = 10;
const BOLD: CommandId = 11;
const UNDERLINE: CommandId = 13;
const STYLES: CommandId = 20;
const ZOOM: CommandId = 30;

const VIEWPORT: (f32, f32) = (2400.0, 1000.0);

fn row(id: MenuItemId, label: &str) -> MenuItem {
    MenuItem::Action {
        id,
        label: label.to_owned(),
        shortcut: None,
        icon: None,
        enabled: true,
        checked: None,
    }
}

/// Home (Clipboard: Paste with its menu, Cut; Font: a drop-down, Bold,
/// Underline that cannot be used; Styles: a gallery showing three of five)
/// and View (Zoom).
fn ribbon() -> Ribbon {
    Ribbon::new(vec![
        RibbonTab::new("home", "Home")
            .with(
                Group::new("clipboard", "Clipboard")
                    .with_priority(3)
                    .with(Control::split(
                        Command::new(PASTE, "Paste"),
                        ButtonSize::Large,
                        vec![
                            row(PASTE_SPECIAL, "Paste special"),
                            row(PASTE_TEXT, "Paste as text"),
                        ],
                    ))
                    .with(Control::button(
                        Command::new(CUT, "Cut"),
                        ButtonSize::Medium,
                    )),
            )
            .with(
                Group::new("font", "Font")
                    .with_priority(2)
                    .with(Control::dropdown(
                        Command::new(FONT, "Font"),
                        vec![
                            Choice::new("Sans"),
                            Choice::new("Serif"),
                            Choice::new("Mono"),
                        ],
                        110.0,
                    ))
                    .with(Control::toggle(
                        Command::new(BOLD, "Bold"),
                        ButtonSize::Small,
                    ))
                    .with(Control::button(
                        Command::new(UNDERLINE, "Underline")
                            .disabled_because("Select some text first"),
                        ButtonSize::Medium,
                    )),
            )
            .with(
                Group::new("styles", "Styles")
                    .with_priority(1)
                    .with(Control::gallery(
                        Command::new(STYLES, "Styles"),
                        ["Normal", "Title", "Heading 1", "Heading 2", "Quote"]
                            .into_iter()
                            .map(Choice::new)
                            .collect(),
                        3,
                    )),
            ),
        RibbonTab::new("view", "View").with(Group::new("zoom", "Zoom").with(Control::button(
            Command::new(ZOOM, "Zoom"),
            ButtonSize::Large,
        ))),
    ])
}

fn access(ribbon: &mut Ribbon, width: f32) -> RibbonAccess<'_> {
    RibbonAccess {
        ribbon,
        x: 0.0,
        y: 0.0,
        width,
        viewport: VIEWPORT,
    }
}

fn act(
    tool: &mut RibbonAccess<'_>,
    part: RibbonPart,
    action: Action,
) -> Result<Option<RibbonEvent>, Refusal> {
    tool.invoke(&part, action, 0.0, 0.0)
}

fn node(tool: &RibbonAccess<'_>, part: RibbonPart) -> Node<RibbonPart> {
    tool.automation(0.0, 0.0)
        .walk()
        .find(|n| n.id == part)
        .unwrap_or_else(|| panic!("no {part:?}"))
        .clone()
}

fn named(tool: &RibbonAccess<'_>, name: &str) -> Node<RibbonPart> {
    tool.automation(0.0, 0.0)
        .walk()
        .find(|n| n.name == name)
        .unwrap_or_else(|| panic!("nothing named {name:?}"))
        .clone()
}

/// **A ribbon shows tools its tabs, the front one chosen, and the front
/// tab's groups and controls** -- each control by its command, its kind
/// said by its role, a toggle's state and a disabled one's reason given --
/// each where the ribbon's own hit test finds it.
#[test]
fn a_ribbon_shows_tools_its_tabs_groups_and_controls() {
    let mut ribbon = ribbon();
    let tool = access(&mut ribbon, 2000.0);
    let root = tool.automation(0.0, 0.0);
    let tabs: Vec<(&str, Option<Value>)> = root.children[0]
        .children
        .iter()
        .map(|n| (n.name.as_str(), n.value.clone()))
        .collect();
    assert_eq!(
        tabs,
        [
            ("Home", Some(Value::Chosen(true))),
            ("View", Some(Value::Chosen(false)))
        ]
    );
    let groups: Vec<&str> = root.children[1..]
        .iter()
        .filter(|n| matches!(n.id, RibbonPart::Group(_)))
        .map(|n| n.name.as_str())
        .collect();
    assert_eq!(groups, ["Clipboard", "Font", "Styles"]);

    let paste = node(&tool, RibbonPart::Command(PASTE));
    assert_eq!(paste.role, Role::Button);
    assert_eq!(paste.children[0].id, RibbonPart::Arrow(PASTE));
    let bold = node(&tool, RibbonPart::Command(BOLD));
    assert_eq!(
        (bold.role, bold.value),
        (Role::CheckBox, Some(Value::Check(CheckState::Unchecked)))
    );
    assert_eq!(node(&tool, RibbonPart::Command(FONT)).role, Role::ComboBox);
    let underline = node(&tool, RibbonPart::Command(UNDERLINE));
    assert!(!underline.enabled);
    assert_eq!(
        underline.description.as_deref(),
        Some("Select some text first")
    );
    let styles = node(&tool, RibbonPart::Command(STYLES));
    let shown: Vec<&str> = styles
        .children
        .iter()
        .filter(|n| matches!(n.id, RibbonPart::Choice(..)))
        .map(|n| n.name.as_str())
        .collect();
    assert_eq!(
        shown,
        ["Normal", "Title", "Heading 1"],
        "three of five shown"
    );

    let layout = tool.layout();
    for part in [
        RibbonPart::Tab(1),
        RibbonPart::Command(CUT),
        RibbonPart::Arrow(PASTE),
        RibbonPart::Choice(STYLES, 2),
    ] {
        let (x, y) = node(&tool, part).bounds.centre();
        assert_eq!(
            tool.place(&layout, part).map(|(_, hit)| hit),
            tool.ribbon.hit(&layout, x, y),
            "{part:?}"
        );
    }
}

/// **A control is used as it is clicked**: a button does its command, a
/// toggle flips and says what it is now, a disabled one is refused; an
/// arrow opens its menu, shown to tools, and a row of it is chosen as
/// clicked -- while the menu is open, nothing under it is pressed.
#[test]
fn a_control_is_used_as_clicked() {
    let mut ribbon = ribbon();
    let mut tool = access(&mut ribbon, 2000.0);
    assert_eq!(
        act(&mut tool, RibbonPart::Command(CUT), Action::Press),
        Ok(Some(RibbonEvent::Command(CUT)))
    );
    assert_eq!(
        act(&mut tool, RibbonPart::Command(BOLD), Action::Toggle),
        Ok(Some(RibbonEvent::Toggled { id: BOLD, on: true }))
    );
    assert_eq!(
        act(&mut tool, RibbonPart::Command(UNDERLINE), Action::Press),
        Err(Refusal::Disabled)
    );
    assert_eq!(
        act(&mut tool, RibbonPart::Command(CUT), Action::Toggle),
        Err(Refusal::NotApplicable {
            role: Role::Button,
            action: "toggle"
        })
    );

    assert_eq!(
        act(&mut tool, RibbonPart::Arrow(PASTE), Action::Press),
        Ok(None)
    );
    assert!(tool.ribbon.menu_open());
    let text = named(&tool, "Paste as text");
    assert_eq!(
        act(&mut tool, RibbonPart::Command(CUT), Action::Press),
        Err(Refusal::Hidden),
        "under the menu"
    );
    assert_eq!(
        act(&mut tool, text.id, Action::Press),
        Ok(Some(RibbonEvent::MenuItem {
            id: PASTE,
            item: PASTE_TEXT
        }))
    );
    assert!(!tool.ribbon.menu_open());
}

/// **A drop-down's list opens as its box is pressed and a choice is made
/// from it; a gallery's choice is made as clicked** -- and each says what is
/// chosen now.
#[test]
fn a_choice_is_made_as_the_user_makes_it() {
    let mut ribbon = ribbon();
    let mut tool = access(&mut ribbon, 2000.0);
    assert_eq!(
        act(&mut tool, RibbonPart::Command(FONT), Action::Press),
        Ok(None)
    );
    let serif = named(&tool, "Serif");
    assert_eq!(
        act(&mut tool, serif.id, Action::Press),
        Ok(Some(RibbonEvent::Chose { id: FONT, index: 1 }))
    );
    tool.ribbon.set_selected(FONT, Some(1));
    assert_eq!(
        node(&tool, RibbonPart::Command(FONT)).value,
        Some(Value::Text("Serif".to_owned()))
    );

    assert_eq!(
        act(&mut tool, RibbonPart::Choice(STYLES, 2), Action::Choose),
        Ok(Some(RibbonEvent::Chose {
            id: STYLES,
            index: 2
        }))
    );
    assert_eq!(
        act(&mut tool, RibbonPart::Choice(STYLES, 4), Action::Choose),
        Err(Refusal::NoSuchWidget),
        "not shown in the group: its arrow lists it"
    );
}

/// **A tab is brought to the front as clicked**, and the controls shown are
/// its: another tab's are no parts.
#[test]
fn a_tab_is_brought_to_the_front() {
    let mut ribbon = ribbon();
    let mut tool = access(&mut ribbon, 2000.0);
    assert_eq!(
        act(&mut tool, RibbonPart::Tab(1), Action::Choose),
        Ok(Some(RibbonEvent::TabSelected("view".to_owned())))
    );
    assert_eq!(
        node(&tool, RibbonPart::Tab(1)).value,
        Some(Value::Chosen(true))
    );
    assert_eq!(
        act(&mut tool, RibbonPart::Command(ZOOM), Action::Press),
        Ok(Some(RibbonEvent::Command(ZOOM)))
    );
    assert_eq!(
        act(&mut tool, RibbonPart::Command(CUT), Action::Press),
        Err(Refusal::NoSuchWidget),
        "Home's, not in front"
    );
}

/// **In a ribbon too narrow for its groups, a group folds into one button,
/// which pressed opens it as a panel**, its controls used there as clicked;
/// while it is open, a control outside it is not pressed -- a press there
/// only closes the panel.
#[test]
fn a_folded_group_opens_as_a_panel() {
    let mut ribbon = ribbon();
    // The widest ribbon in which Clipboard -- the last to fold -- is folded.
    let mut width = 900.0;
    while width > 100.0
        && !access(&mut ribbon, width)
            .automation(0.0, 0.0)
            .walk()
            .any(|n| n.id == RibbonPart::Folded(0))
    {
        width -= 10.0;
    }
    let mut tool = access(&mut ribbon, width);
    let folded = node(&tool, RibbonPart::Folded(0));
    assert_eq!(
        (folded.name.as_str(), folded.description.as_deref()),
        ("Clipboard", Some("folded"))
    );
    assert_eq!(act(&mut tool, folded.id, Action::Press), Ok(None));
    assert!(tool.ribbon.panel_open());
    let panel = node(&tool, RibbonPart::Panel);
    assert!(
        panel.walk().any(|n| n.id == RibbonPart::Command(CUT)),
        "Cut, in the panel"
    );
    let inside = node(&tool, RibbonPart::Command(CUT));
    let outside = tool
        .automation(0.0, 0.0)
        .children
        .iter()
        .filter(|n| matches!(n.id, RibbonPart::Group(_)))
        .flat_map(|g| g.walk())
        .find(|n| matches!(n.id, RibbonPart::Command(_)) && n.enabled)
        .map(|n| n.id);
    if let Some(outside) = outside {
        assert_eq!(
            act(&mut tool, outside, Action::Press),
            Err(Refusal::Hidden),
            "outside the open panel"
        );
    }
    assert_eq!(
        act(&mut tool, inside.id, Action::Press),
        Ok(Some(RibbonEvent::Command(CUT)))
    );
}
