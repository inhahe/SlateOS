//! Tests for the overview as tools see it: its search, its cards and their
//! close buttons, its lanes, and that each press point presses its part.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use super::*;
use crate::overview::{DesktopLane, OverviewAction, WindowThumbnail, on_mouse_click};
use crate::{ShellControlAction, ShellRequest, WindowId};

const SW: f32 = 1920.0;
const SH: f32 = 1080.0;

/// A window's card, of an 800 x 600 window.
fn thumb(id: u64, desktop: u32, title: &str) -> WindowThumbnail {
    WindowThumbnail {
        window_id: id,
        desktop_id: desktop,
        title: title.to_owned(),
        x: 100.0,
        y: 100.0,
        width: 800.0,
        height: 600.0,
        is_focused: id == 2,
        is_minimized: false,
    }
}

/// An open overview of two desktops' windows: Terminal and Editor (which
/// has the keyboard) on the first, the current one, and Browser on the
/// second.
fn overview(mode: OverviewMode) -> OverviewState {
    let mut state = OverviewState::new();
    state.lanes = vec![
        DesktopLane {
            desktop_id: 0,
            name: "Desktop 1".to_owned(),
            thumbnails: vec![thumb(1, 0, "Terminal"), thumb(2, 0, "Editor")],
            is_current: true,
        },
        DesktopLane {
            desktop_id: 1,
            name: "Desktop 2".to_owned(),
            thumbnails: vec![thumb(3, 1, "Browser")],
            is_current: false,
        },
    ];
    state.show(mode);
    state
}

fn tree(state: &OverviewState) -> Node<OverviewPart> {
    automation(state, &OverviewConfig::default(), SW, SH)
}

fn node(state: &OverviewState, part: OverviewPart) -> Node<OverviewPart> {
    tree(state)
        .walk()
        .find(|node| node.id == part)
        .unwrap_or_else(|| panic!("no {part:?}"))
        .clone()
}

/// A press at `part`'s press point, and what the overview made of it.
fn press(state: &mut OverviewState, part: OverviewPart) -> OverviewAction {
    let (x, y) = press_point(state, &OverviewConfig::default(), SW, SH, part)
        .unwrap_or_else(|| panic!("{part:?} has no press point"));
    let layouts = overview_layout(state, &OverviewConfig::default(), SW, SH);
    let lanes = lane_layout(state, &OverviewConfig::default(), SW, SH);
    on_mouse_click(state, x, y, &layouts, &lanes)
}

/// **The overview shows tools its search and its windows**: each card named
/// by its window's title and said to be focused, each holding its close
/// button; the search holding what is typed, and the cards it leaves out
/// said to be.
#[test]
fn the_overview_shows_tools_its_search_and_its_windows() {
    let mut state = overview(OverviewMode::AllWindows);
    let root = tree(&state);
    assert_eq!((root.role, root.name.as_str()), (Role::Dialog, "Overview"));
    let names: Vec<(Role, &str)> = root
        .children
        .iter()
        .map(|n| (n.role, n.name.as_str()))
        .collect();
    assert_eq!(
        names,
        [(Role::TextField, "Search windows"), (Role::List, "Windows")]
    );
    let cards: Vec<&str> = root.children[1]
        .children
        .iter()
        .map(|n| n.name.as_str())
        .collect();
    assert_eq!(cards, ["Terminal", "Editor"], "this desktop's windows");
    let editor = node(&state, OverviewPart::Card(2));
    assert_eq!(editor.description.as_deref(), Some("focused"));
    assert_eq!(
        editor.children.iter().map(|n| n.id).collect::<Vec<_>>(),
        [OverviewPart::Close(2)]
    );

    state.type_search_text("term");
    assert_eq!(
        node(&state, OverviewPart::Search).value,
        Some(Value::Text("term".to_owned()))
    );
    assert_eq!(
        node(&state, OverviewPart::Card(2)).description.as_deref(),
        Some("focused, not found by the search")
    );
    assert_eq!(node(&state, OverviewPart::Card(1)).description, None);
}

/// **Every desktop shows as its lane, holding its windows**: the current
/// one said to be, the one the wheel chose chosen.
#[test]
fn every_desktop_is_a_lane_holding_its_windows() {
    let mut state = overview(OverviewMode::AllDesktops);
    state.selected_desktop = Some(1);
    let desktops = node(&state, OverviewPart::Windows);
    assert_eq!(
        (desktops.role, desktops.name.as_str()),
        (Role::List, "Desktops")
    );
    let lanes: Vec<(&str, Option<&str>, Option<Value>, usize)> = desktops
        .children
        .iter()
        .map(|lane| {
            (
                lane.name.as_str(),
                lane.description.as_deref(),
                lane.value.clone(),
                lane.children.len(),
            )
        })
        .collect();
    assert_eq!(
        lanes,
        [
            (
                "Desktop 1",
                Some("the current desktop"),
                Some(Value::Chosen(false)),
                2
            ),
            ("Desktop 2", None, Some(Value::Chosen(true)), 1),
        ]
    );
}

/// **Each part's press point presses it**: a card switches to its window,
/// a lit card's close button closes its window, and a lane switches to its
/// desktop; a close button takes no press while its card is not lit.
#[test]
fn a_press_at_a_parts_press_point_presses_it() {
    let config = OverviewConfig::default();
    let mut state = overview(OverviewMode::AllWindows);
    assert_eq!(
        press_point(&state, &config, SW, SH, OverviewPart::Close(1)),
        None,
        "not drawn until its card is lit"
    );
    state.hovered_window = Some(1);
    assert_eq!(
        press(&mut state, OverviewPart::Close(1)),
        OverviewAction::Request(ShellRequest::window(WindowId(1), ShellControlAction::Close))
    );
    assert_eq!(
        press(&mut state, OverviewPart::Card(2)),
        OverviewAction::Request(ShellRequest::window(
            WindowId(2),
            ShellControlAction::Activate
        ))
    );

    let mut state = overview(OverviewMode::AllDesktops);
    assert_eq!(
        press(&mut state, OverviewPart::Lane(1)),
        OverviewAction::Request(ShellRequest::SwitchDesktop { desktop: 1 })
    );
    assert_eq!(
        press_point(&state, &config, SW, SH, OverviewPart::Card(1)),
        None,
        "closed"
    );
    let state = overview(OverviewMode::AllWindows);
    for part in [
        OverviewPart::Lane(0),
        OverviewPart::Card(3),
        OverviewPart::Search,
        OverviewPart::Overview,
    ] {
        assert_eq!(press_point(&state, &config, SW, SH, part), None, "{part:?}");
    }
}
