//! Tests for an icon grid as tools see it: its items, every one -- one
//! scrolled out of sight below the grid -- chosen as clicked and opened as
//! double-clicked, scrolled into view first.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use super::super::GridItem;
use super::*;

/// Twenty items, ids 100 on, in a grid 400 by 300: more rows than it shows.
fn grid() -> GridView {
    let mut grid = GridView::new();
    grid.set_container_size(400.0, 300.0);
    grid.set_items(
        (0..20)
            .map(|n| GridItem {
                id: 100 + n,
                label: format!("item {n}"),
                icon_id: None,
                badge: None,
                user_data: 0,
            })
            .collect(),
    );
    grid
}

fn act(
    grid: &mut GridView,
    part: GridPart,
    action: Action,
) -> Result<Option<Vec<GridEvent>>, Refusal> {
    grid.invoke(&part, action, 0.0, 0.0)
}

/// **An icon grid shows tools every item, named by its label -- one
/// scrolled out of sight with its box below the grid -- and an item is
/// chosen as clicked**, scrolled into view first, the grid saying its
/// selection changed.
#[test]
fn an_item_is_chosen_as_clicked() {
    let mut grid = grid();
    let root = grid.automation(0.0, 0.0);
    assert_eq!(root.role, Role::List);
    assert_eq!(root.children.len(), 20, "every item");
    assert_eq!(root.children[3].name, "item 3");
    let last = root.children.last().unwrap();
    assert!(
        last.bounds.y >= 300.0,
        "the last below the grid: {:?}",
        last.bounds
    );
    assert!(
        root.children.iter().all(|item| !item.focused),
        "the keyboard on none before a click"
    );

    let said = act(&mut grid, GridPart::Item(119), Action::Choose)
        .unwrap()
        .expect("the selection changed");
    assert!(
        said.contains(&GridEvent::SelectionChanged(vec![19])),
        "{said:?}"
    );
    let after = grid.automation(0.0, 0.0);
    let chosen = after.children[19].clone();
    assert_eq!(chosen.value, Some(Value::Chosen(true)));
    assert!(chosen.focused, "the keyboard on the item clicked");
    assert!(!after.children[3].focused);
    assert!(
        chosen.bounds.y >= 0.0 && chosen.bounds.bottom() <= 300.0,
        "scrolled into view: {:?}",
        chosen.bounds
    );
    assert!(grid.drain_events().is_empty(), "the host hears it once");
}

/// **An item pressed is opened, as a double click opens it**; what is not
/// an item's to do is refused.
#[test]
fn an_item_is_opened_as_double_clicked() {
    let mut grid = grid();
    let said = act(&mut grid, GridPart::Item(102), Action::Press)
        .unwrap()
        .expect("it opens");
    assert!(said.contains(&GridEvent::Activate(2)), "{said:?}");
    assert_eq!(
        act(&mut grid, GridPart::Item(999), Action::Press),
        Err(Refusal::NoSuchWidget)
    );
    assert_eq!(
        act(&mut grid, GridPart::Item(102), Action::Toggle),
        Err(Refusal::NotApplicable {
            role: Role::ListItem,
            action: "toggle"
        })
    );
    assert_eq!(
        act(&mut grid, GridPart::Grid, Action::Press),
        Err(Refusal::NotApplicable {
            role: Role::List,
            action: "press"
        })
    );
}
