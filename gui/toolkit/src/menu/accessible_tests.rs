//! Tests for a context menu as tools see it: its rows, what each says, and
//! a row pressed as a click presses it -- greyed rows refused, submenus
//! opened, rows out of the panel scrolled in first.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use super::*;

/// An action row.
fn action(id: MenuItemId, label: &str, shortcut: Option<&str>, checked: Option<bool>) -> MenuItem {
    MenuItem::Action {
        id,
        label: label.to_owned(),
        shortcut: shortcut.map(str::to_owned),
        icon: None,
        enabled: true,
        checked,
    }
}

/// A shown menu: Cut (with its keys), Paste (greyed, and why), a line, Word
/// wrap (ticked), and Sort by, which opens Name (a check) and Date.
fn menu() -> ContextMenu {
    let mut paste = action(2, "Paste", None, None);
    if let MenuItem::Action { enabled, .. } = &mut paste {
        *enabled = false;
    }
    let mut menu = ContextMenu::new(vec![
        action(1, "Cut", Some("Ctrl+X"), None),
        paste,
        MenuItem::Separator,
        action(3, "Word wrap", None, Some(true)),
        MenuItem::Submenu {
            id: 4,
            label: "Sort by".to_owned(),
            icon: None,
            enabled: true,
            children: vec![
                action(5, "Name", None, Some(false)),
                action(6, "Date", None, None),
            ],
        },
    ]);
    menu.explain(2, "Nothing to paste");
    menu.show(100.0, 100.0, (1920.0, 1080.0));
    menu
}

/// The menu as tools see it.
fn tree(menu: &ContextMenu) -> Node<MenuPart> {
    menu.automation(0.0, 0.0)
}

/// The node of `part`.
fn node(menu: &ContextMenu, part: MenuPart) -> Node<MenuPart> {
    tree(menu)
        .walk()
        .find(|node| node.id == part)
        .unwrap_or_else(|| panic!("no {part:?}"))
        .clone()
}

/// **A menu shows tools its rows, the line between them left out**: each
/// named by its label and boxed where it is drawn; its keys as what it
/// says, or, greyed, why; a check ticked or not.
#[test]
fn a_menu_shows_tools_its_rows() {
    let menu = menu();
    let root = tree(&menu);
    assert_eq!(root.role, Role::Menu);
    assert_eq!(Some(root.bounds), menu.panel_rect());
    let names: Vec<&str> = root.children.iter().map(|n| n.name.as_str()).collect();
    assert_eq!(names, ["Cut", "Paste", "Word wrap", "Sort by"]);
    for (row, index) in root.children.iter().zip([0, 1, 3, 4]) {
        assert_eq!(row.role, Role::MenuItem);
        assert_eq!(Some(row.bounds), menu.item_rect(index), "{}", row.name);
    }
    let cut = node(&menu, MenuPart::Item(1));
    assert_eq!(cut.description.as_deref(), Some("Ctrl+X"));
    assert!(cut.enabled && cut.value.is_none());
    let paste = node(&menu, MenuPart::Item(2));
    assert!(!paste.enabled);
    assert_eq!(paste.description.as_deref(), Some("Nothing to paste"));
    assert_eq!(
        node(&menu, MenuPart::Item(3)).value,
        Some(Value::Check(CheckState::Checked))
    );
    assert!(
        node(&menu, MenuPart::Item(4)).children.is_empty(),
        "its submenu is not open"
    );
}

/// **A row pressed is chosen, and the menu closes, as a click does; a
/// greyed one is refused and the menu stays.**
#[test]
fn a_row_pressed_is_chosen_and_a_greyed_one_refused() {
    let mut menu = menu();
    assert_eq!(
        menu.invoke(&MenuPart::Item(2), Action::Press, 0.0, 0.0),
        Err(Refusal::Disabled)
    );
    assert!(menu.is_visible());
    assert_eq!(
        menu.invoke(&MenuPart::Item(1), Action::Press, 0.0, 0.0),
        Ok(Some(MenuAction::Selected(1)))
    );
    assert!(!menu.is_visible());
    assert_eq!(menu.press_point(1), Err(Refusal::NoSuchWidget), "closed");
}

/// **A submenu's row pressed opens it, held by the row; its rows are then
/// pressed as the menu's own are, and a check is toggled by choosing it.**
#[test]
fn a_submenu_opens_and_its_rows_are_pressed() {
    let mut menu = menu();
    assert_eq!(
        menu.invoke(&MenuPart::Item(6), Action::Press, 0.0, 0.0),
        Err(Refusal::NoSuchWidget),
        "not before its submenu is open"
    );
    assert_eq!(
        menu.invoke(&MenuPart::Item(4), Action::Press, 0.0, 0.0),
        Ok(None)
    );
    let sort = node(&menu, MenuPart::Item(4));
    let names: Vec<&str> = sort.children[0]
        .children
        .iter()
        .map(|n| n.name.as_str())
        .collect();
    assert_eq!(
        (sort.children[0].id, sort.children[0].name.as_str()),
        (MenuPart::Submenu(4), "Sort by")
    );
    assert_eq!(names, ["Name", "Date"]);
    assert_eq!(
        node(&menu, MenuPart::Item(5)).value,
        Some(Value::Check(CheckState::Unchecked))
    );
    assert_eq!(
        menu.invoke(&MenuPart::Item(5), Action::Toggle, 0.0, 0.0),
        Ok(Some(MenuAction::Selected(5)))
    );
    assert!(!menu.is_visible());
}

/// **What a part is not for is refused**: a row that is no check is not
/// toggled, and the menu itself is not pressed.
#[test]
fn what_a_part_is_not_for_is_refused() {
    let mut menu = menu();
    assert_eq!(
        menu.invoke(&MenuPart::Item(1), Action::Toggle, 0.0, 0.0),
        Err(Refusal::NotApplicable {
            role: Role::MenuItem,
            action: "toggle"
        })
    );
    assert_eq!(
        menu.invoke(&MenuPart::Menu, Action::Press, 0.0, 0.0),
        Err(Refusal::NotApplicable {
            role: Role::Menu,
            action: "press"
        })
    );
    assert!(menu.is_visible(), "a refusal changes nothing");
}

/// **A row out of a panel too short for the menu is scrolled into it
/// first**, as the arrow keys bring it, and then chosen.
#[test]
fn a_row_out_of_the_panel_is_scrolled_in_first() {
    let rows: Vec<MenuItem> = (0..100)
        .map(|i| action(i, &format!("Row {i}"), None, None))
        .collect();
    let mut menu = ContextMenu::new(rows);
    menu.show(10.0, 0.0, (800.0, 300.0));
    let panel = menu.panel_rect().unwrap();
    let far = node(&menu, MenuPart::Item(90));
    assert!(far.bounds.y > panel.bottom(), "its box is below the panel");
    assert!(menu.item_rect(90).is_none());

    let (x, y) = menu.press_point(90).unwrap();
    assert!(panel.contains(x, y), "({x}, {y}) is in {panel:?}");
    assert_eq!(menu.item_rect(90).map(Rect::centre), Some((x, y)));
    assert_eq!(menu.handle_click(x, y), Some(90));
}
