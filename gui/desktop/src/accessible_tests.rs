#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_cmp
)]

use guitk::widget::automation::Query;

use super::*;
use crate::volume::Output;
use crate::volume::volume_tests::card;
use crate::{WindowId, WindowInfo, WindowList};

fn shell() -> DesktopShell {
    DesktopShell::new(1920, 1080)
}

/// The shell as tools see it.
fn tree(shell: &DesktopShell) -> Node<ShellPart> {
    shell.automation(0.0, 0.0)
}

/// The node of `part`.
fn node(shell: &DesktopShell, part: ShellPart) -> Node<ShellPart> {
    tree(shell)
        .walk()
        .find(|node| node.id == part)
        .unwrap_or_else(|| panic!("no {part:?}"))
        .clone()
}

/// Press `part` as a tool does.
fn press(shell: &mut DesktopShell, part: ShellPart) -> Result<Option<ShellAction>, Refusal> {
    shell.invoke(&part, Action::Press, 0.0, 0.0)
}

/// **The taskbar shows tools its parts**: Start first, the speaker, the
/// bell, the clock -- named by the time, described by the whole date -- and
/// Show desktop; each part's box where the shell's own hit test finds it.
#[test]
fn the_taskbar_shows_tools_its_parts() {
    let shell = shell();
    let root = tree(&shell);
    assert_eq!((root.role, root.name.as_str()), (Role::Group, "Desktop"));
    let bar = &root.children[0];
    assert_eq!(bar.id, ShellPart::Taskbar);
    let names: Vec<&str> = bar.children.iter().map(|n| n.name.as_str()).collect();
    assert_eq!(names.first(), Some(&"Start"));
    for wanted in ["Volume", "Notifications", "Show desktop"] {
        assert!(names.contains(&wanted), "{wanted} in {names:?}");
    }
    let clock = node(&shell, ShellPart::Control(Hit::Clock));
    assert!(clock.name.contains(':'), "the time: {:?}", clock.name);
    assert!(clock.description.is_some_and(|date| !date.is_empty()));
    for part in &bar.children {
        if let ShellPart::Control(hit) = part.id {
            let (x, y) = part.bounds.centre();
            assert_eq!(shell.hit_test(x, y), hit, "{}", part.name);
        }
    }
    assert_eq!(
        root.children.len(),
        1,
        "nothing open over the taskbar: nothing else shown"
    );
}

/// **A window's tile is named by its title, the one with the keyboard
/// marked, and pressing it is the shell's click on it.**
#[test]
fn a_windows_tile_is_its_title() {
    let setup = || {
        let mut shell = shell();
        let _asked = shell.apply_window_list(&WindowList::new(
            0,
            vec![
                WindowInfo::new(1, 40, "notes.md"),
                WindowInfo::new(2, 41, "Inbox"),
            ],
        ));
        shell.focused_window = Some(WindowId(2));
        shell
    };
    let mut shell = setup();
    let inbox = Query {
        role: Some(Role::Button),
        name: Some("inbox".to_owned()),
        ..Query::default()
    }
    .find_in(&tree(&shell));
    assert_eq!(inbox, [ShellPart::Control(Hit::TaskbarButton(WindowId(2)))]);
    assert_eq!(
        node(&shell, inbox[0]).value,
        Some(Value::Chosen(true)),
        "the window with the keyboard"
    );
    assert_eq!(
        node(&shell, ShellPart::Control(Hit::TaskbarButton(WindowId(1)))).value,
        Some(Value::Chosen(false))
    );
    // Pressing it is a click on it: what the shell answers a click.
    let tile = node(&shell, ShellPart::Control(Hit::TaskbarButton(WindowId(1)))).bounds;
    let (x, y) = tile.centre();
    let mut clicked = setup();
    let by_hand = (
        clicked.handle_mouse(&click(x, y)),
        clicked.handle_mouse(&MouseEvent {
            x,
            y,
            kind: MouseEventKind::Release(MouseButton::Left),
        }),
    );
    let by_tool = press(
        &mut shell,
        ShellPart::Control(Hit::TaskbarButton(WindowId(1))),
    );
    assert_eq!(
        by_tool,
        Ok(answered(by_hand.1).or_else(|| answered(by_hand.0)))
    );
    assert_eq!(
        press(
            &mut shell,
            ShellPart::Control(Hit::TaskbarButton(WindowId(9)))
        ),
        Err(Refusal::NoSuchWidget)
    );
}

/// **Start pressed opens the start menu**: its search, its programs --
/// every row, a row scrolled out of the list below it -- its places, its
/// power button and caret. A search set filters the list, and a program
/// pressed is started, as a click starts it.
#[test]
fn start_opens_the_menu_and_its_search_filters_it() {
    let mut shell = shell();
    assert_eq!(
        press(&mut shell, ShellPart::Control(Hit::StartButton)),
        Ok(None)
    );
    assert!(shell.start_menu_open);
    let menu = node(&shell, ShellPart::StartMenu);
    let parts: Vec<(Role, &str)> = menu
        .children
        .iter()
        .map(|n| (n.role, n.name.as_str()))
        .collect();
    assert_eq!(parts[0], (Role::TextField, "Search"));
    assert_eq!(parts[1], (Role::List, "Programs"));
    assert!(parts.contains(&(Role::Button, "Home")), "{parts:?}");
    assert!(parts.contains(&(Role::Button, "Shut down")), "{parts:?}");
    assert!(
        parts.contains(&(Role::Button, "Power options")),
        "{parts:?}"
    );

    let list = node(&shell, ShellPart::StartList);
    assert_eq!(
        list.children.len(),
        shell.start_menu_rows().len(),
        "every row"
    );
    let shown = shell.start_menu_visible_rows();
    if list.children.len() > shown {
        let below = &list.children[shown];
        assert!(
            below.bounds.y >= list.bounds.y + list.bounds.h - 0.5,
            "a row scrolled out is below the list"
        );
    }

    let program = shell
        .start_menu_entries()
        .first()
        .expect("a program")
        .name
        .clone();
    assert!(
        list.children.iter().any(|row| row.name == program),
        "{program} listed"
    );
    assert_eq!(
        shell.invoke(
            &ShellPart::StartSearch,
            Action::SetText(program.clone()),
            0.0,
            0.0
        ),
        Ok(None)
    );
    assert_eq!(
        node(&shell, ShellPart::StartSearch).value,
        Some(Value::Text(program.clone()))
    );
    let found = node(&shell, ShellPart::StartList);
    assert!(
        found.children.len() < list.children.len(),
        "the search filtered"
    );
    assert_eq!(found.children[0].name, program, "the best match first");
    let started = press(&mut shell, found.children[0].id);
    assert!(
        matches!(started, Ok(Some(ShellAction::Launch(_)))),
        "{started:?}"
    );
}

/// **A row scrolled out of the start menu is scrolled into it, then
/// clicked** -- as the user would.
#[test]
fn a_row_out_of_sight_is_scrolled_to_first() {
    let mut shell = shell();
    press(&mut shell, ShellPart::Control(Hit::StartButton)).unwrap();
    let rows = shell.start_menu_rows().len();
    let shown = shell.start_menu_visible_rows();
    assert!(
        rows > shown,
        "a list longer than its window: {rows} rows, {shown} shown"
    );
    let last = rows - 1;
    let pressed = press(&mut shell, ShellPart::Control(Hit::StartMenuEntry(last)));
    assert!(pressed.is_ok(), "{pressed:?}");
    assert!(
        shell.start_menu_scroll + shown > last,
        "scrolled to it: from {} with {shown} shown",
        shell.start_menu_scroll
    );
}

/// **The power caret opens the power choices**, each named as the menu
/// names it.
#[test]
fn the_caret_opens_the_power_choices() {
    let mut shell = shell();
    press(&mut shell, ShellPart::Control(Hit::StartButton)).unwrap();
    press(&mut shell, ShellPart::Control(Hit::PowerCaret)).unwrap();
    assert!(shell.power_menu_open);
    let menu = node(&shell, ShellPart::PowerMenu);
    let names: Vec<&str> = menu.children.iter().map(|n| n.name.as_str()).collect();
    assert!(
        names.contains(&"Restart") && names.contains(&"Log out"),
        "{names:?}"
    );
}

/// **The volume flyout's level and mute are set as its controls set them**
/// -- held to the level's range; out of use while the card is out of reach;
/// and "Audio settings…" opens Settings' Sound page.
#[test]
fn the_volume_flyout_is_set_as_its_controls_set_it() {
    let mut shell = shell();
    let (_card_state, output) = card(40, false);
    shell.attach_volume(output);
    assert_eq!(
        shell.invoke(&ShellPart::VolumeLevel, Action::SetValue(10.0), 0.0, 0.0),
        Err(Refusal::NoSuchWidget),
        "not while it is closed"
    );
    press(&mut shell, ShellPart::Control(Hit::VolumeIcon)).unwrap();
    assert!(shell.volume_flyout.is_visible());
    assert_eq!(
        shell.invoke(&ShellPart::VolumeLevel, Action::SetValue(150.0), 0.0, 0.0),
        Ok(None)
    );
    assert_eq!(shell.notifications.volume(), 100, "held to its range");
    shell
        .invoke(&ShellPart::VolumeLevel, Action::SetValue(30.0), 0.0, 0.0)
        .unwrap();
    assert_eq!(
        node(&shell, ShellPart::VolumeLevel).value,
        Some(Value::Range {
            value: 30.0,
            min: 0.0,
            max: 100.0
        })
    );
    shell
        .invoke(&ShellPart::VolumeMute, Action::Toggle, 0.0, 0.0)
        .unwrap();
    assert!(shell.notifications.is_muted());
    assert_eq!(
        node(&shell, ShellPart::VolumeMute).value,
        Some(Value::Check(CheckState::Checked))
    );
    assert_eq!(
        press(&mut shell, ShellPart::AudioSettings),
        Ok(Some(ShellAction::Launch(crate::launcher::settings_page(
            crate::launcher::SOUND_PAGE
        ))))
    );

    let mut shell = DesktopShell::new(1920, 1080);
    shell.attach_volume(Output::OutOfReach("No sound card reachable"));
    press(&mut shell, ShellPart::Control(Hit::VolumeIcon)).unwrap();
    assert!(!node(&shell, ShellPart::VolumeLevel).enabled);
    assert_eq!(
        node(&shell, ShellPart::Volume).description.as_deref(),
        Some("No sound card reachable")
    );
    assert_eq!(
        shell.invoke(&ShellPart::VolumeLevel, Action::SetValue(10.0), 0.0, 0.0),
        Err(Refusal::Disabled)
    );
    assert_eq!(
        shell.invoke(
            &ShellPart::VolumeLevel,
            Action::SetValue(f64::NAN),
            0.0,
            0.0
        ),
        Err(Refusal::NotANumber)
    );
}

/// **What is not a part's to do is refused**, and a part a menu covers is
/// not clicked through it.
#[test]
fn what_a_part_is_not_for_is_refused() {
    let mut shell = shell();
    assert_eq!(
        shell.invoke(&ShellPart::Taskbar, Action::Press, 0.0, 0.0),
        Err(Refusal::NotApplicable {
            role: Role::Group,
            action: "press"
        })
    );
    assert_eq!(
        shell.invoke(
            &ShellPart::Control(Hit::Clock),
            Action::SetText("noon".to_owned()),
            0.0,
            0.0
        ),
        Err(Refusal::NotApplicable {
            role: Role::Button,
            action: "set the text of"
        })
    );
    assert_eq!(
        shell.invoke(
            &ShellPart::StartSearch,
            Action::SetText("x".to_owned()),
            0.0,
            0.0
        ),
        Err(Refusal::NoSuchWidget),
        "no search with the menu closed"
    );
    assert_eq!(
        press(&mut shell, ShellPart::Control(Hit::PowerButton)),
        Err(Refusal::NoSuchWidget),
        "no power button with the menu closed"
    );
}
