//! Tests for a dock as tools see it: its groups' tabs, the front one chosen,
//! brought to the front and closed as clicked; and its dividers, said where
//! they stand and dragged to where they are set.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_cmp
)]

use super::*;

fn id(name: &str) -> PanelId {
    PanelId::new(name).expect("a panel name")
}

fn kinds() -> Vec<PanelKind> {
    vec![
        PanelKind::new("files", "Files"),
        PanelKind::new("editor", "Editor"),
        PanelKind::new("outline", "Outline"),
    ]
}

/// Files (in front) and Outline in the left 30%, Editor in the rest.
fn dock() -> Dock {
    Dock::from_text("h(0.3:[*files,outline],0.7:[editor])", |_| true).expect("an arrangement")
}

const AREA: Rect = Rect {
    x: 0.0,
    y: 0.0,
    w: 1000.0,
    h: 600.0,
};

fn act(
    tool: &mut DockAccess<'_>,
    part: DockPart,
    action: Action,
) -> Result<Option<DockEvent>, Refusal> {
    tool.invoke(&part, action, 0.0, 0.0)
}

fn node(tool: &DockAccess<'_>, part: &DockPart) -> Node<DockPart> {
    tool.automation(0.0, 0.0)
        .walk()
        .find(|n| n.id == *part)
        .unwrap_or_else(|| panic!("no {part:?}"))
        .clone()
}

/// **A dock shows tools each group's tabs, named as they say, the front one
/// chosen, each with its close button; the front panel's contents; and each
/// divider, where it stands across its split** -- each where the dock's own
/// hit test finds it.
#[test]
fn a_dock_shows_tools_its_groups_tabs_and_dividers() {
    let (mut dock, mut input, kinds) = (dock(), DockInput::new(), kinds());
    let tool = DockAccess {
        dock: &mut dock,
        input: &mut input,
        area: AREA,
        kinds: &kinds,
    };
    let root = tool.automation(0.0, 0.0);
    let groups: Vec<&Node<DockPart>> = root
        .children
        .iter()
        .filter(|n| n.role == Role::TabList)
        .collect();
    assert_eq!(groups.len(), 2);
    let tabs: Vec<(&str, Option<Value>)> = groups[0]
        .children
        .iter()
        .filter(|n| n.role == Role::Tab)
        .map(|n| (n.name.as_str(), n.value.clone()))
        .collect();
    assert_eq!(
        tabs,
        [
            ("Files", Some(Value::Chosen(true))),
            ("Outline", Some(Value::Chosen(false)))
        ]
    );
    let files = node(&tool, &DockPart::Tab(id("files")));
    assert_eq!(files.children[0].id, DockPart::Close(id("files")));
    assert_eq!(files.children[0].name, "Close Files");
    assert_eq!(
        node(&tool, &DockPart::Content(id("files"))).name,
        "Files",
        "the front panel's contents"
    );
    let divider = root
        .children
        .iter()
        .find(|n| n.role == Role::Separator)
        .expect("a divider");
    let Some(Value::Range { value, min, max }) = divider.value else {
        panic!("{:?}", divider.value);
    };
    assert_eq!((min, max), (0.0, 1.0));
    assert!((value - 0.3).abs() < 0.02, "{value}");

    let layout = tool.layout();
    for part in [
        DockPart::Tab(id("outline")),
        DockPart::Close(id("files")),
        divider.id.clone(),
    ] {
        let (x, y) = node(&tool, &part).bounds.centre();
        assert_eq!(
            tool.place(&layout, &part).map(|(_, hit)| hit),
            tool.dock.hit(&layout, x, y),
            "{part:?}"
        );
    }
}

/// **A tab is brought to the front as clicked, and closed as its close
/// button is clicked**; a panel the dock does not hold is no part.
#[test]
fn a_tab_is_brought_to_the_front_and_closed() {
    let (mut dock, mut input, kinds) = (dock(), DockInput::new(), kinds());
    let mut tool = DockAccess {
        dock: &mut dock,
        input: &mut input,
        area: AREA,
        kinds: &kinds,
    };
    assert_eq!(
        act(&mut tool, DockPart::Tab(id("outline")), Action::Choose),
        Ok(Some(DockEvent::Activated(id("outline"))))
    );
    assert_eq!(
        node(&tool, &DockPart::Tab(id("outline"))).value,
        Some(Value::Chosen(true))
    );
    assert_eq!(
        act(&mut tool, DockPart::Close(id("outline")), Action::Press),
        Ok(Some(DockEvent::Closed(id("outline"))))
    );
    assert!(!tool.dock.contains(&id("outline")));
    assert_eq!(
        act(&mut tool, DockPart::Tab(id("outline")), Action::Press),
        Err(Refusal::NoSuchWidget)
    );
    assert_eq!(
        act(&mut tool, DockPart::Tab(id("files")), Action::Toggle),
        Err(Refusal::NotApplicable {
            role: Role::Tab,
            action: "toggle"
        })
    );
}

/// **A tab past the end of its bar is not shown, and is not pressed**: the
/// dock lays its tabs out end to end whatever the bar's width, draws none
/// past its end and finds none there -- a press at the tab's middle would
/// land beside the dock -- so a tool's is refused, and nothing changes.
#[test]
fn a_tab_past_the_end_of_its_bar_is_refused() {
    let (mut dock, mut input, kinds) = (
        Dock::from_text("[*files,editor,outline]", |_| true).expect("an arrangement"),
        DockInput::new(),
        kinds(),
    );
    let mut tool = DockAccess {
        dock: &mut dock,
        input: &mut input,
        area: Rect::new(0.0, 0.0, 120.0, 400.0),
        kinds: &kinds,
    };
    let root = tool.automation(0.0, 0.0);
    let group = &root.children[0];
    let past = group
        .children
        .iter()
        .find(|n| n.role == Role::Tab && n.bounds.x >= group.bounds.right())
        .expect("a tab laid out past the bar's end")
        .clone();
    assert!(!past.shown, "{past:?}");
    assert!(past.children.iter().all(|close| !close.shown));
    assert!(group.children[0].shown, "the first, at the bar's start");

    assert_eq!(
        act(&mut tool, past.id.clone(), Action::Choose),
        Err(Refusal::Hidden)
    );
    let DockPart::Tab(panel) = past.id else {
        panic!("a tab: {:?}", past.id);
    };
    assert_eq!(
        act(&mut tool, DockPart::Close(panel.clone()), Action::Press),
        Err(Refusal::Hidden)
    );
    assert!(tool.dock.contains(&panel), "not closed");
    assert_eq!(
        node(&tool, &DockPart::Tab(id("files"))).value,
        Some(Value::Chosen(true)),
        "the front one still in front"
    );
}

/// **A divider set to a place is dragged there**, as the pointer drags it,
/// and says where it stands now; a value that is no number is refused.
#[test]
fn a_divider_is_dragged_to_where_it_is_set() {
    let (mut dock, mut input, kinds) = (dock(), DockInput::new(), kinds());
    let mut tool = DockAccess {
        dock: &mut dock,
        input: &mut input,
        area: AREA,
        kinds: &kinds,
    };
    let divider = DockPart::Divider(Vec::new(), 0);
    assert_eq!(
        act(&mut tool, divider.clone(), Action::SetValue(0.6)),
        Ok(Some(DockEvent::Resized))
    );
    let Some(Value::Range { value, .. }) = node(&tool, &divider).value else {
        panic!("no place");
    };
    assert!((value - 0.6).abs() < 0.02, "{value}");
    assert_eq!(
        act(&mut tool, divider.clone(), Action::SetValue(f64::NAN)),
        Err(Refusal::NotANumber)
    );
    assert_eq!(
        act(&mut tool, divider, Action::Press),
        Err(Refusal::NotApplicable {
            role: Role::Separator,
            action: "press"
        })
    );
}
