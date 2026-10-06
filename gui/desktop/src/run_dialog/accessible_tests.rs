//! Tests for the Run box as tools see it: its line typed over and run, a
//! suggestion chosen, its buttons pressed, a line that did not run saying
//! why, and a box that is not up refusing.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use super::*;

/// The box up in the middle of a 1920 x 1080 screen.
fn shown() -> RunDialog {
    let mut dialog = RunDialog::new();
    dialog.show();
    dialog.centre_on(1920.0, 1080.0);
    dialog
}

fn act(
    dialog: &mut RunDialog,
    part: RunPart,
    action: Action,
) -> Result<Option<Vec<RunDialogEvent>>, Refusal> {
    dialog.invoke(&part, action, 0.0, 0.0)
}

fn tree(dialog: &RunDialog) -> Node<RunPart> {
    dialog.automation(0.0, 0.0)
}

/// **The Run box shows tools its line, named by its label, and its buttons,
/// each where the box takes a press; the line's text is set as typing sets
/// it, bringing the suggestions typing brings, and pressed it runs.**
#[test]
fn the_line_is_typed_over_and_run() {
    let mut dialog = shown();
    let root = tree(&dialog);
    assert_eq!((root.role, root.name.as_str()), (Role::Dialog, "Run"));
    assert_eq!(root.description.as_deref(), Some(INSTRUCTION));
    assert!(root.shown);
    let names: Vec<(RunPart, &str)> = root
        .children
        .iter()
        .map(|n| (n.id, n.name.as_str()))
        .collect();
    assert_eq!(
        names[..4],
        [
            (RunPart::Field, "Open:"),
            (RunPart::Ok, "OK"),
            (RunPart::Cancel, "Cancel"),
            (RunPart::Browse, "Browse..."),
        ]
    );
    assert!(root.children[0].focused, "the line has the keyboard");

    assert_eq!(
        act(
            &mut dialog,
            RunPart::Field,
            Action::SetText("calc".to_owned())
        ),
        Ok(None)
    );
    assert_eq!(dialog.input.text(), "calc");
    let list = tree(&dialog)
        .walk()
        .find(|n| n.id == RunPart::Suggestions)
        .cloned()
        .expect("typing brings suggestions");
    assert!(
        list.children.iter().any(|n| n.name == "calculator"),
        "{:?}",
        list.children.iter().map(|n| &n.name).collect::<Vec<_>>()
    );
    act(
        &mut dialog,
        RunPart::Field,
        Action::SetText("calculator".to_owned()),
    )
    .unwrap();
    assert_eq!(dialog.input.text(), "calculator", "typed over, not after");

    let said = act(&mut dialog, RunPart::Field, Action::Press)
        .unwrap()
        .expect("it runs");
    assert!(
        matches!(said.first(), Some(RunDialogEvent::Execute(request)) if request.whole == "calculator"),
        "{said:?}"
    );
    assert!(dialog.drain_events().is_empty(), "the host hears it once");
    assert!(!dialog.is_visible(), "run, the box goes");
    assert_eq!(
        act(&mut dialog, RunPart::Ok, Action::Press),
        Err(Refusal::Hidden)
    );
}

/// **A suggestion is chosen as a click chooses it**, wherever its row lies;
/// **a line that cannot run says why**; and the buttons press as clicked.
#[test]
fn a_suggestion_is_chosen_and_the_buttons_pressed() {
    let mut dialog = shown();
    act(&mut dialog, RunPart::Field, Action::SetText("c".to_owned())).unwrap();
    let rows: Vec<(RunPart, String)> = tree(&dialog)
        .walk()
        .filter(|n| matches!(n.id, RunPart::Suggestion(_)))
        .map(|n| (n.id, n.name.clone()))
        .collect();
    let (last, name) = rows.last().cloned().expect("suggestions for c");
    assert_eq!(act(&mut dialog, last, Action::Choose), Ok(None));
    assert_eq!(dialog.input.text(), name);
    assert_eq!(
        act(&mut dialog, RunPart::Suggestion(99), Action::Choose),
        Err(Refusal::NoSuchWidget)
    );

    act(
        &mut dialog,
        RunPart::Field,
        Action::SetText("nonexistent!@#".to_owned()),
    )
    .unwrap();
    assert_eq!(act(&mut dialog, RunPart::Ok, Action::Press), Ok(None));
    let field = tree(&dialog).children[0].clone();
    assert!(
        field
            .description
            .as_deref()
            .is_some_and(|why| why.contains("not recognized")),
        "{:?}",
        field.description
    );
    assert_eq!(
        act(&mut dialog, RunPart::Browse, Action::Press),
        Ok(Some(vec![RunDialogEvent::Browse]))
    );
    assert!(dialog.is_visible(), "Browse leaves the box up");
    assert_eq!(
        act(&mut dialog, RunPart::Ok, Action::Toggle),
        Err(Refusal::NotApplicable {
            role: Role::Button,
            action: "toggle"
        })
    );
    let said = act(&mut dialog, RunPart::Cancel, Action::Press)
        .unwrap()
        .expect("cancelled");
    assert_eq!(said.first(), Some(&RunDialogEvent::Cancel));
    assert!(!dialog.is_visible());
}
