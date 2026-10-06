//! Tests for a menu bar as tools see it: its titles, the open menu and the
//! submenus open below it, each row pressed as clicked -- and a submenu row
//! pressed one level up from the bottom of an open chain opening its own
//! submenu there.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use super::*;
use crate::menubar::MenuBarItem;

fn action(id: MenuItemId, label: &str, shortcut: Option<&str>, enabled: bool) -> MenuBarEntry {
    MenuBarEntry::Action {
        label: label.to_owned(),
        shortcut: shortcut.map(str::to_owned),
        enabled,
        id,
    }
}

fn submenu(label: &str, children: Vec<MenuBarEntry>) -> MenuBarEntry {
    MenuBarEntry::SubMenu {
        label: label.to_owned(),
        children,
    }
}

/// File: New (Ctrl+N), Open Recent > {a.txt, More > {b.txt}, Older >
/// {c.txt}}, a line, Word wrap (ticked), Print (greyed). Edit: Undo.
fn file_and_edit() -> MenuBar {
    MenuBar::new(vec![
        MenuBarItem {
            label: "&File".to_owned(),
            children: vec![
                action(1, "New", Some("Ctrl+N"), true),
                submenu(
                    "Open Recent",
                    vec![
                        action(10, "a.txt", None, true),
                        submenu("More", vec![action(20, "b.txt", None, true)]),
                        submenu("Older", vec![action(30, "c.txt", None, true)]),
                    ],
                ),
                MenuBarEntry::Separator,
                MenuBarEntry::Check {
                    label: "Word wrap".to_owned(),
                    checked: true,
                    id: 2,
                },
                action(3, "Print", None, false),
            ],
        },
        MenuBarItem {
            label: "&Edit".to_owned(),
            children: vec![action(4, "Undo", None, true)],
        },
    ])
}

fn press(bar: &mut MenuBar, part: MenuBarPart) -> Result<Option<MenuBarEvent>, Refusal> {
    bar.invoke(&part, Action::Press, 800.0, 0.0)
}

fn node(bar: &MenuBar, part: &MenuBarPart) -> Node<MenuBarPart> {
    bar.automation(800.0, 0.0)
        .walk()
        .find(|node| node.id == *part)
        .unwrap_or_else(|| panic!("no {part:?}"))
        .clone()
}

fn names(node: &Node<MenuBarPart>) -> Vec<&str> {
    node.children.iter().map(|n| n.name.as_str()).collect()
}

/// **A menu bar shows tools its menus' titles, and the open menu's rows
/// under its title**: each named by its label, the keys as what a row
/// says, a check ticked, a greyed row out of use, the line left out.
#[test]
fn a_menu_bar_shows_tools_its_menus() {
    let mut bar = file_and_edit();
    let root = bar.automation(800.0, 0.0);
    assert_eq!((root.role, root.name.as_str()), (Role::MenuBar, "Menu bar"));
    assert_eq!(names(&root), ["File", "Edit"]);
    assert!(root.children[0].children.is_empty(), "no menu open");

    assert_eq!(press(&mut bar, MenuBarPart::Title(0)), Ok(None));
    let file = node(&bar, &MenuBarPart::Title(0));
    assert_eq!(file.value, Some(Value::Chosen(true)));
    let menu = &file.children[0];
    assert_eq!(
        (menu.id.clone(), menu.role),
        (MenuBarPart::Menu(vec![0]), Role::Menu)
    );
    assert_eq!(names(menu), ["New", "Open Recent", "Word wrap", "Print"]);
    assert_eq!(menu.children[0].description.as_deref(), Some("Ctrl+N"));
    assert_eq!(
        menu.children[2].value,
        Some(Value::Check(CheckState::Checked))
    );
    assert!(!menu.children[3].enabled);
    // Each row where the bar takes its click.
    for row in &menu.children {
        let (x, y) = row.bounds.centre();
        let mut clicked = file_and_edit();
        clicked
            .invoke(&MenuBarPart::Title(0), Action::Press, 800.0, 0.0)
            .unwrap();
        let _handled = clicked.handle_mouse_event(
            &MouseEvent {
                x,
                y,
                kind: MouseEventKind::Press(MouseButton::Left),
            },
            clicked.viewport,
        );
        if let MenuBarPart::Item(id) = row.id
            && row.enabled
        {
            let events = clicked.drain_events();
            assert!(
                events.iter().any(|event| matches!(
                    event,
                    MenuBarEvent::ItemClicked(got) | MenuBarEvent::CheckToggled(got, _) if *got == id
                )),
                "{}: {events:?}",
                row.name
            );
        }
    }
}

/// **A row pressed raises its event, taken off the bar's queue, and the
/// menu closes; a check toggled says what it is now; a greyed row and a
/// row of a menu not open are refused.**
#[test]
fn a_row_pressed_raises_its_event() {
    let mut bar = file_and_edit();
    assert_eq!(
        press(&mut bar, MenuBarPart::Item(1)),
        Err(Refusal::NoSuchWidget),
        "File is not open"
    );
    press(&mut bar, MenuBarPart::Title(0)).unwrap();
    assert_eq!(
        press(&mut bar, MenuBarPart::Item(3)),
        Err(Refusal::Disabled)
    );
    assert_eq!(
        press(&mut bar, MenuBarPart::Item(1)),
        Ok(Some(MenuBarEvent::ItemClicked(1)))
    );
    assert!(!bar.is_open());
    assert!(bar.drain_events().is_empty(), "the host hears it once");

    press(&mut bar, MenuBarPart::Title(0)).unwrap();
    assert_eq!(
        bar.invoke(&MenuBarPart::Item(2), Action::Toggle, 800.0, 0.0),
        Ok(Some(MenuBarEvent::CheckToggled(2, false)))
    );
    press(&mut bar, MenuBarPart::Title(0)).unwrap();
    assert_eq!(
        bar.invoke(&MenuBarPart::Item(1), Action::Toggle, 800.0, 0.0),
        Err(Refusal::NotApplicable {
            role: Role::MenuItem,
            action: "toggle"
        }),
        "New is no check"
    );
}

/// **A submenu row pressed opens its submenu, held by the row, and its rows
/// are pressed as the menu's are** -- and a submenu row one level up from
/// the bottom of an open chain opens its submenu at its own level, in place
/// of the one open there. It used to hang it off the bottom panel, where
/// its rows were looked up in a list that does not hold them and it showed
/// nothing.
#[test]
fn submenus_open_where_their_rows_are() {
    let mut bar = file_and_edit();
    press(&mut bar, MenuBarPart::Title(0)).unwrap();
    assert_eq!(press(&mut bar, MenuBarPart::SubMenu(vec![0, 1])), Ok(None));
    let recent = node(&bar, &MenuBarPart::Menu(vec![0, 1]));
    assert_eq!(names(&recent), ["a.txt", "More", "Older"]);
    assert_eq!(
        press(&mut bar, MenuBarPart::SubMenu(vec![0, 1, 1])),
        Ok(None)
    );
    assert_eq!(
        names(&node(&bar, &MenuBarPart::Menu(vec![0, 1, 1]))),
        ["b.txt"]
    );

    // Older is in Open Recent, a level above the bottom.
    assert_eq!(
        press(&mut bar, MenuBarPart::SubMenu(vec![0, 1, 2])),
        Ok(None)
    );
    let older = node(&bar, &MenuBarPart::Menu(vec![0, 1, 2]));
    assert_eq!(names(&older), ["c.txt"]);
    assert!(
        !bar.automation(800.0, 0.0)
            .walk()
            .any(|node| node.id == MenuBarPart::Menu(vec![0, 1, 1])),
        "More gave way to Older"
    );
    assert_eq!(
        press(&mut bar, MenuBarPart::Item(30)),
        Ok(Some(MenuBarEvent::ItemClicked(30)))
    );
}
