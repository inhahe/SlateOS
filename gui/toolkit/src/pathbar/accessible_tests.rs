//! Tests for a path bar as tools see it: its crumbs and its address, each
//! used as the user uses it, and the suggestions offered while typing.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::PathBuf;

use super::*;
use crate::pathbar::CompletionItem;

const WIDTH: f32 = 600.0;
const HEIGHT: f32 = 28.0;

/// A bar showing `/home/user/docs`, laid out 600 x 28.
fn bar() -> PathBar {
    let mut bar = PathBar::new("/home/user/docs");
    bar.lay_out(WIDTH, HEIGHT);
    bar
}

fn act(
    bar: &mut PathBar,
    part: PathBarPart,
    action: Action,
) -> Result<Option<Vec<PathBarEvent>>, Refusal> {
    bar.invoke(&part, action, WIDTH, HEIGHT)
}

fn tree(bar: &PathBar) -> Node<PathBarPart> {
    bar.automation(WIDTH, HEIGHT)
}

/// **Showing a path, the bar shows tools its address and its crumbs**, each
/// crumb named by its folder; a crumb pressed asks to go there.
#[test]
fn a_crumb_pressed_goes_to_its_folder() {
    let mut bar = bar();
    let root = tree(&bar);
    assert_eq!(root.role, Role::Group);
    let field = &root.children[0];
    assert_eq!(
        (field.role, field.name.as_str()),
        (Role::TextField, "Address")
    );
    assert_eq!(field.value, Some(Value::Text("/home/user/docs".to_owned())));
    assert!(!field.focused);
    let crumbs: Vec<(PathBarPart, &str)> = root.children[1..]
        .iter()
        .map(|n| (n.id, n.name.as_str()))
        .collect();
    assert_eq!(
        crumbs,
        [
            (PathBarPart::Crumb(0), "/"),
            (PathBarPart::Crumb(1), "home"),
            (PathBarPart::Crumb(2), "user"),
            (PathBarPart::Crumb(3), "docs"),
        ]
    );
    assert_eq!(
        act(&mut bar, PathBarPart::Crumb(1), Action::Press),
        Ok(Some(vec![PathBarEvent::Navigate(PathBuf::from("/home"))]))
    );
    assert_eq!(
        act(&mut bar, PathBarPart::Crumb(9), Action::Press),
        Err(Refusal::NoSuchWidget)
    );
}

/// **The address's text is set as typing sets it, and pressed it is
/// confirmed as Enter confirms it**: the bar asks for suggestions for what
/// is typed, offers them, one is chosen as a click chooses it, and the
/// confirmed path is one to go to.
#[test]
fn the_address_is_typed_over_and_confirmed() {
    let mut bar = bar();
    assert_eq!(
        act(&mut bar, PathBarPart::Field, Action::Press),
        Err(Refusal::NotApplicable {
            role: Role::TextField,
            action: "press"
        }),
        "nothing typed to confirm"
    );
    let typed = act(
        &mut bar,
        PathBarPart::Field,
        Action::SetText("/usr/sh".to_owned()),
    )
    .unwrap()
    .unwrap();
    assert_eq!(typed[0], PathBarEvent::EditModeEntered);
    assert!(
        typed
            .iter()
            .any(|event| matches!(event, PathBarEvent::RequestAutoComplete { .. })),
        "{typed:?}"
    );
    assert!(bar.drain_events().is_empty(), "the host hears them once");
    let field = tree(&bar).children[0].clone();
    assert_eq!(field.value, Some(Value::Text("/usr/sh".to_owned())));
    assert!(field.focused);

    bar.set_completions(vec![
        CompletionItem {
            name: "share".to_owned(),
            is_directory: true,
        },
        CompletionItem {
            name: "shells".to_owned(),
            is_directory: false,
        },
    ]);
    bar.lay_out(WIDTH, HEIGHT);
    let list = tree(&bar)
        .walk()
        .find(|n| n.id == PathBarPart::Suggestions)
        .unwrap()
        .clone();
    let offered: Vec<(&str, Option<&str>)> = list
        .children
        .iter()
        .map(|n| (n.name.as_str(), n.description.as_deref()))
        .collect();
    assert_eq!(offered, [("share", Some("folder")), ("shells", None)]);
    let highlighted: Vec<Option<Value>> = list.children.iter().map(|n| n.value.clone()).collect();
    assert_eq!(
        highlighted,
        [Some(Value::Chosen(true)), Some(Value::Chosen(false))],
        "the first offered is the one Tab takes"
    );
    act(&mut bar, PathBarPart::Completion(0), Action::Choose).unwrap();
    let field = tree(&bar).children[0].clone();
    let Some(Value::Text(text)) = field.value else {
        panic!("{:?}", field.value);
    };
    assert!(text.starts_with("/usr/share"), "{text}");

    let confirmed = act(&mut bar, PathBarPart::Field, Action::Press)
        .unwrap()
        .unwrap();
    assert!(
        confirmed.iter().any(
            |event| matches!(event, PathBarEvent::Navigate(path) if path.starts_with("/usr/share"))
        ),
        "{confirmed:?}"
    );
}

/// The bar's text set to nothing is the whole of it deleted, as a user
/// clears a field.
#[test]
fn the_address_set_to_nothing_is_cleared() {
    let mut bar = bar();
    act(&mut bar, PathBarPart::Field, Action::SetText(String::new())).unwrap();
    let field = tree(&bar).children[0].clone();
    assert_eq!(field.value, Some(Value::Text(String::new())));
    assert!(field.focused);
}

/// **A suggestion the list has scrolled out of sight is listed where it would
/// be drawn, and chosen it is scrolled into the list first**, as the arrow
/// keys bring it, and then clicked.
#[test]
fn a_suggestion_out_of_sight_is_scrolled_in_and_chosen() {
    let mut bar = bar();
    act(
        &mut bar,
        PathBarPart::Field,
        Action::SetText("/usr/".to_owned()),
    )
    .unwrap();
    bar.set_completions(
        (0..12)
            .map(|n| CompletionItem {
                name: format!("dir{n:02}"),
                is_directory: true,
            })
            .collect(),
    );
    let root = tree(&bar);
    let list = root
        .walk()
        .find(|n| n.id == PathBarPart::Suggestions)
        .unwrap();
    assert_eq!(list.children.len(), 12, "every suggestion, shown or not");
    let tenth = list
        .children
        .iter()
        .find(|n| n.id == PathBarPart::Completion(10))
        .unwrap();
    assert_eq!(tenth.name, "dir10");
    assert!(
        tenth.bounds.y >= list.bounds.bottom(),
        "below the list: {:?} under {:?}",
        tenth.bounds,
        list.bounds
    );

    assert_eq!(
        act(&mut bar, PathBarPart::Completion(12), Action::Choose),
        Err(Refusal::NoSuchWidget),
        "past the last"
    );
    let first = tree(&bar)
        .walk()
        .find(|n| n.id == PathBarPart::Completion(0))
        .unwrap()
        .bounds;
    assert!(
        first.y < list.bounds.bottom() && first.y >= list.bounds.y,
        "a refusal leaves the list where it was: {first:?} in {:?}",
        list.bounds
    );

    act(&mut bar, PathBarPart::Completion(10), Action::Choose).unwrap();
    assert_eq!(bar.typed_text(), Some("/usr/dir10/"));
}

/// A part is refused what the user could not do to it.
#[test]
fn what_a_part_cannot_do_is_refused() {
    let mut bar = bar();
    assert_eq!(
        act(&mut bar, PathBarPart::Bar, Action::Press),
        Err(Refusal::NotApplicable {
            role: Role::Group,
            action: "press"
        })
    );
    assert_eq!(
        act(&mut bar, PathBarPart::Crumb(1), Action::Toggle),
        Err(Refusal::NotApplicable {
            role: Role::Button,
            action: "toggle"
        })
    );
    assert_eq!(
        act(&mut bar, PathBarPart::Completion(0), Action::Choose),
        Err(Refusal::NoSuchWidget),
        "no suggestions while the path is shown"
    );
    act(&mut bar, PathBarPart::Field, Action::Focus).unwrap();
    assert!(bar.is_editing());
    assert_eq!(
        act(&mut bar, PathBarPart::Crumb(1), Action::Press),
        Err(Refusal::Hidden),
        "typing, the bar shows text, not the trail"
    );
    assert!(
        !tree(&bar)
            .walk()
            .any(|n| matches!(n.id, PathBarPart::Crumb(_))),
        "nor lists it"
    );
    assert_eq!(
        act(&mut bar, PathBarPart::Suggestions, Action::Press),
        Err(Refusal::NotApplicable {
            role: Role::List,
            action: "press"
        })
    );
}
