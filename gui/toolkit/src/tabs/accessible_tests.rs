//! Tests for a tab bar as tools see it: its tabs and their close buttons,
//! each where it is clicked, and a tab out of the bar scrolled in first.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_cmp
)]

use super::*;
use crate::tabs::{Tab, TabPosition, TabWidth};

/// A bar of `n` tabs, 100 pixels each, the last unsaved and the first not
/// closeable.
fn bar(n: u64) -> TabView {
    let mut view = TabView::new(TabPosition::Top);
    view.set_tab_width(TabWidth::Fixed(100.0));
    for id in 1..=n {
        let mut tab = Tab::new(id, format!("Doc {id}"));
        tab.closeable = id != 1;
        tab.dirty = id == n;
        view.add_tab(tab);
    }
    view
}

/// The node of `part`, for a bar `width` wide.
fn node(view: &TabView, part: TabPart, width: f32) -> Node<TabPart> {
    view.automation(width, 0.0)
        .walk()
        .find(|node| node.id == part)
        .unwrap_or_else(|| panic!("no {part:?}"))
        .clone()
}

/// **A tab bar shows tools its tabs**: each named by its label, the one
/// showing chosen, an unsaved one said to be, a closeable one holding its
/// close button -- each box where the bar takes its click.
#[test]
fn a_tab_bar_shows_tools_its_tabs() {
    let mut view = bar(3);
    let root = view.automation(1000.0, 0.0);
    assert_eq!((root.role, root.name.as_str()), (Role::TabList, "Tabs"));
    let tabs: Vec<(&str, Option<Value>)> = root
        .children
        .iter()
        .map(|n| (n.name.as_str(), n.value.clone()))
        .collect();
    assert_eq!(
        tabs,
        [
            ("Doc 1", Some(Value::Chosen(true))),
            ("Doc 2", Some(Value::Chosen(false))),
            ("Doc 3", Some(Value::Chosen(false))),
        ]
    );
    assert!(root.children[0].children.is_empty(), "not closeable");
    assert_eq!(
        root.children[2].description.as_deref(),
        Some("unsaved changes")
    );
    for tab in &root.children {
        assert_eq!(tab.role, Role::Tab);
        let (x, y) = tab.bounds.centre();
        let TabPart::Tab(id) = tab.id else {
            panic!("{:?}", tab.id);
        };
        let mut clicked = bar(3);
        let answer = clicked.handle_click(x, y);
        assert!(
            answer.is_none() || answer == Some(TabEvent::Selected(id)),
            "{}: {answer:?}",
            tab.name
        );
        for close in &tab.children {
            let (x, y) = close.bounds.centre();
            assert_eq!(view.handle_click(x, y), Some(TabEvent::CloseRequested(id)));
        }
    }
}

/// **A tab chosen is clicked**: its page shows, and choosing the one
/// showing says nothing; a close button pressed asks for its tab to close.
#[test]
fn a_tab_is_chosen_and_closed_as_clicked() {
    let mut view = bar(3);
    assert_eq!(
        view.invoke(&TabPart::Tab(2), Action::Choose, 1000.0, 0.0),
        Ok(Some(TabEvent::Selected(2)))
    );
    assert_eq!(view.active_id(), Some(2));
    assert_eq!(
        view.invoke(&TabPart::Tab(2), Action::Press, 1000.0, 0.0),
        Ok(None),
        "already showing"
    );
    assert_eq!(
        view.invoke(&TabPart::Close(3), Action::Press, 1000.0, 0.0),
        Ok(Some(TabEvent::CloseRequested(3)))
    );
    assert_eq!(
        view.invoke(&TabPart::Close(1), Action::Press, 1000.0, 0.0),
        Err(Refusal::NoSuchWidget),
        "the first cannot close"
    );
    assert_eq!(
        view.invoke(&TabPart::Tab(9), Action::Press, 1000.0, 0.0),
        Err(Refusal::NoSuchWidget)
    );
    assert_eq!(
        view.invoke(&TabPart::Bar, Action::Press, 1000.0, 0.0),
        Err(Refusal::NotApplicable {
            role: Role::TabList,
            action: "press"
        })
    );
}

/// **A tab out of a bar too narrow for every tab is scrolled into it
/// first**, as the user would scroll to it, and then chosen; a narrow tab
/// is chosen beside its close button, not on it.
#[test]
fn a_tab_out_of_the_bar_is_scrolled_in_first() {
    let mut view = bar(10);
    let width = 350.0;
    let far = node(&view, TabPart::Tab(9), width);
    assert!(far.bounds.x >= width, "its box is past the bar");
    assert_eq!(
        view.invoke(&TabPart::Tab(9), Action::Choose, width, 0.0),
        Ok(Some(TabEvent::Selected(9)))
    );
    let shown = node(&view, TabPart::Tab(9), width);
    assert!(
        shown.bounds.x >= 0.0 && shown.bounds.right() <= width,
        "{:?} is in the bar",
        shown.bounds
    );
    assert_eq!(
        view.invoke(&TabPart::Tab(1), Action::Choose, width, 0.0),
        Ok(Some(TabEvent::Selected(1)))
    );
    assert_eq!(view.scroll_offset(), 0.0, "back to the start");

    let mut narrow = TabView::new(TabPosition::Top);
    narrow.set_tab_width(TabWidth::Fixed(30.0));
    narrow.add_tab(Tab::new(1, "A"));
    narrow.add_tab(Tab::new(2, "B"));
    assert_eq!(
        narrow.invoke(&TabPart::Tab(2), Action::Choose, 500.0, 0.0),
        Ok(Some(TabEvent::Selected(2))),
        "chosen, not closed"
    );
}
