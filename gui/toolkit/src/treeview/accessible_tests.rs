//! Tests for a tree as tools see it: its rows nested as the tree nests
//! them, each part pressed where the tree takes the press, a closed node's
//! children reached by opening it, a row out of the tree scrolled in first.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use super::*;
use crate::treeview::TreeItem;
use crate::widget::CheckState;

/// home/{docs/{a.txt, b.txt (locked)}}, readme.
struct Files;

impl TreeSource for Files {
    type Key = &'static str;
    fn children(&self, parent: &[&'static str]) -> Option<Vec<TreeItem<&'static str>>> {
        Some(match parent {
            [] => vec![
                TreeItem::branch("home", "home"),
                TreeItem::leaf("readme", "readme").with_detail("12 bytes"),
            ],
            ["home"] => vec![TreeItem::branch("docs", "docs")],
            ["home", "docs"] => vec![
                TreeItem::leaf("a.txt", "a.txt"),
                TreeItem::leaf("b.txt", "b.txt").disabled("locked"),
            ],
            _ => Vec::new(),
        })
    }
}

/// A tree of [`Files`] drawn at (10, 20), 300 wide and `rows` rows tall.
fn tree(checkable: bool, rows: f32) -> TreeView<&'static str> {
    let mut view = if checkable {
        TreeView::checkable(true)
    } else {
        TreeView::new()
    };
    let height = rows * view.metrics().row_height;
    view.set_bounds(Rect::new(10.0, 20.0, 300.0, height));
    let _opened = view.refresh(&Files);
    view
}

/// `action` on `part`, as a tool asks it.
fn act(
    view: &mut TreeView<&'static str>,
    part: TreePart<&'static str>,
    action: Action,
) -> Result<Option<Vec<TreeEvent<&'static str>>>, Refusal> {
    TreeAccess {
        view,
        source: &Files,
    }
    .invoke(&part, action, 0.0, 0.0)
}

/// The tree as tools see it.
fn seen(view: &mut TreeView<&'static str>) -> Node<TreePart<&'static str>> {
    TreeAccess {
        view,
        source: &Files,
    }
    .automation(0.0, 0.0)
}

/// The node of `part`.
fn node(
    view: &mut TreeView<&'static str>,
    part: &TreePart<&'static str>,
) -> Node<TreePart<&'static str>> {
    seen(view)
        .walk()
        .find(|node| node.id == *part)
        .unwrap_or_else(|| panic!("no {part:?}"))
        .clone()
}

fn row(path: &[&'static str]) -> TreePart<&'static str> {
    TreePart::Row(path.to_vec())
}

/// **A tree's rows are nested as the tree nests them**, a node's children
/// shown once it is opened -- by pressing its arrow, as the pointer does --
/// each named by its label and described by its detail.
#[test]
fn rows_are_nested_and_a_node_opens_from_its_arrow() {
    let mut view = tree(false, 10.0);
    let root = seen(&mut view);
    assert_eq!((root.role, root.bounds), (Role::Tree, view.bounds()));
    let names: Vec<&str> = root.children.iter().map(|n| n.name.as_str()).collect();
    assert_eq!(names, ["home", "readme"]);
    assert_eq!(root.children[1].description.as_deref(), Some("12 bytes"));
    let arrow = &root.children[0].children[0];
    assert_eq!((arrow.role, arrow.name.as_str()), (Role::Button, "Expand"));

    assert_eq!(
        act(&mut view, TreePart::Disclosure(vec!["home"]), Action::Press),
        Ok(Some(vec![TreeEvent::Expanded(vec!["home"])]))
    );
    let home = node(&mut view, &row(&["home"]));
    let kids: Vec<(Role, &str)> = home
        .children
        .iter()
        .map(|n| (n.role, n.name.as_str()))
        .collect();
    assert_eq!(
        kids,
        [(Role::Button, "Collapse"), (Role::TreeItem, "docs")],
        "its arrow, then its children"
    );
}

/// **A row chosen is selected as a click selects it; one pressed is opened
/// as a double click opens it**, a branch opened, its children nested; a
/// row inside a closed node is no part until it is opened; a greyed row is
/// refused and says why.
#[test]
fn a_row_is_chosen_as_clicked_and_opened_as_double_clicked() {
    let mut view = tree(false, 10.0);
    assert_eq!(
        act(&mut view, row(&["readme"]), Action::Choose),
        Ok(Some(vec![TreeEvent::Selected(vec!["readme"])]))
    );
    assert_eq!(
        node(&mut view, &row(&["readme"])).value,
        Some(Value::Chosen(true))
    );
    assert_eq!(
        act(&mut view, row(&["home", "docs"]), Action::Choose),
        Err(Refusal::NoSuchWidget),
        "inside a closed node"
    );
    let opened = act(&mut view, row(&["home"]), Action::Press)
        .unwrap()
        .unwrap();
    assert!(
        opened.contains(&TreeEvent::Expanded(vec!["home"])),
        "{opened:?}"
    );
    act(
        &mut view,
        TreePart::Disclosure(vec!["home", "docs"]),
        Action::Press,
    )
    .unwrap();
    let locked = node(&mut view, &row(&["home", "docs", "b.txt"]));
    assert!(!locked.enabled);
    assert_eq!(locked.description.as_deref(), Some("locked"));
    assert_eq!(
        act(&mut view, row(&["home", "docs", "b.txt"]), Action::Choose),
        Err(Refusal::Disabled)
    );
    assert_eq!(
        act(&mut view, TreePart::Tree, Action::Press),
        Err(Refusal::NotApplicable {
            role: Role::Tree,
            action: "press"
        })
    );
}

/// **In a checkbox tree, a node's box is ticked and unticked as a click
/// does**, and says which.
#[test]
fn a_checkbox_trees_box_is_toggled_as_clicked() {
    let mut view = tree(true, 10.0);
    let check = TreePart::Check(vec!["readme"]);
    assert_eq!(
        node(&mut view, &check).value,
        Some(Value::Check(CheckState::Checked))
    );
    assert_eq!(
        act(&mut view, check.clone(), Action::Toggle),
        Ok(Some(vec![TreeEvent::CheckChanged {
            path: vec!["readme"],
            included: false
        }]))
    );
    assert_eq!(
        node(&mut view, &check).value,
        Some(Value::Check(CheckState::Unchecked))
    );
    let mut plain = tree(false, 10.0);
    assert_eq!(
        act(&mut plain, check, Action::Toggle),
        Err(Refusal::NoSuchWidget),
        "no boxes in a plain tree"
    );
}

/// **A row out of a tree too short for its rows is scrolled into it
/// first**, as the user would scroll to it, then chosen.
#[test]
fn a_row_out_of_the_tree_is_scrolled_in_first() {
    let mut view = tree(false, 2.0);
    act(&mut view, TreePart::Disclosure(vec!["home"]), Action::Press).unwrap();
    act(
        &mut view,
        TreePart::Disclosure(vec!["home", "docs"]),
        Action::Press,
    )
    .unwrap();
    let last = row(&["readme"]);
    let before = node(&mut view, &last).bounds;
    assert!(
        before.y >= view.bounds().bottom(),
        "{before:?} is below the tree"
    );
    assert_eq!(
        act(&mut view, last.clone(), Action::Choose),
        Ok(Some(vec![TreeEvent::Selected(vec!["readme"])]))
    );
    let after = node(&mut view, &last).bounds;
    assert!(
        after.y >= view.bounds().y && after.bottom() <= view.bounds().bottom(),
        "{after:?} is in the tree"
    );
}
