#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_cmp
)]

use guitk::widget::automation::Query;

use super::*;
use crate::notif_pane::QuickSetting;
use crate::volume::Output;
use crate::volume::volume_tests::card;
use crate::widgets::WidgetKind;
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
    assert_eq!(root.children[0].id, ShellPart::Icons);
    let bar = &root.children[1];
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
        2,
        "nothing open over the taskbar: nothing else shown"
    );
}

/// **The desktop's icons are a list**, each named by its label and said to
/// be what it is; one chosen is chosen as a click chooses it, one pressed
/// is opened as a double click opens it.
#[test]
fn the_desktops_icons_are_a_list() {
    let mut shell = shell();
    // The defaults alone: This PC and the bin whoever runs the test, and no
    // layout read from anyone's settings.
    shell.icons.populate_defaults();
    let icons = node(&shell, ShellPart::Icons);
    let bin = icons
        .children
        .iter()
        .find(|icon| icon.name == "Recycle Bin")
        .expect("the recycle bin")
        .clone();
    assert_eq!(bin.description.as_deref(), Some("recycle bin"));
    assert_eq!(bin.value, Some(Value::Chosen(false)));
    let (x, y) = bin.bounds.centre();
    assert_eq!(shell.hit_test(x, y), Hit::Desktop);
    assert_eq!(shell.invoke(&bin.id, Action::Choose, 0.0, 0.0), Ok(None));
    assert_eq!(node(&shell, bin.id).value, Some(Value::Chosen(true)));
    let opened = shell.invoke(&bin.id, Action::Press, 0.0, 0.0);
    assert!(
        matches!(opened, Ok(Some(ShellAction::Launch(_)))),
        "{opened:?}"
    );
    assert_eq!(
        shell.invoke(&bin.id, Action::SetText("x".to_owned()), 0.0, 0.0),
        Err(Refusal::NotApplicable {
            role: Role::ListItem,
            action: "set the text of"
        })
    );
    assert_eq!(
        shell.invoke(
            &ShellPart::Icon(crate::icons::IconId(u64::MAX)),
            Action::Press,
            0.0,
            0.0
        ),
        Err(Refusal::NoSuchWidget)
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

/// A notification from `app`, titled `title`.
fn notif(app: &str, title: &str) -> crate::notif_pane::Notification {
    crate::notif_pane::Notification {
        id: 0,
        app_name: app.to_owned(),
        title: title.to_owned(),
        body: String::new(),
        timestamp: 0,
        priority: crate::notif_pane::NotifPriority::Normal,
        read: false,
        action: None,
        silent: false,
    }
}

/// **The notification pane is held among the shell's parts while it is
/// open**, stopping at the taskbar as it is drawn; closed, it is not.
#[test]
fn the_notification_pane_is_among_the_shells_parts_while_open() {
    let mut shell = shell();
    let in_tree = |shell: &DesktopShell| {
        tree(shell)
            .walk()
            .any(|node| node.id == ShellPart::Pane(PanePart::Pane))
    };
    assert!(!in_tree(&shell));
    press(&mut shell, ShellPart::Control(Hit::NotificationBell)).unwrap();
    let pane = node(&shell, ShellPart::Pane(PanePart::Pane));
    assert_eq!(
        (pane.role, pane.name.as_str()),
        (Role::Dialog, "Notifications")
    );
    assert_eq!(pane.bounds.bottom(), shell.notification_pane_height());
    let dark = node(
        &shell,
        ShellPart::Pane(PanePart::Switch(QuickSetting::DarkMode)),
    );
    assert_eq!(dark.role, Role::CheckBox);
    press(&mut shell, ShellPart::Control(Hit::NotificationBell)).unwrap();
    assert!(!in_tree(&shell), "closed by its bell");
}

/// **A notification pressed through the shell is opened as a click opens
/// it** -- what it names is started, and the pane closes over it -- and
/// its cross dismisses it, pointed at first as the pointer would be.
#[test]
fn a_notification_is_opened_and_dismissed_as_the_pointer_would() {
    appearance::config::testing::with_scratch_config("acc-pane-cards", |_root| {
        let mut shell = shell();
        let mut mail = notif("Mail", "Invoice");
        mail.action = Some("/apps/mail".to_owned());
        let mail = shell.notify(mail);
        let chat = shell.notify(notif("Chat", "Lunch?"));
        shell.toggle_notifications();

        assert_eq!(
            press(&mut shell, ShellPart::Pane(PanePart::Dismiss(chat))),
            Ok(None)
        );
        assert!(
            shell
                .notifications
                .notifications()
                .iter()
                .all(|n| n.id != chat),
            "the card stayed"
        );
        assert_eq!(
            press(&mut shell, ShellPart::Pane(PanePart::Card(mail))),
            Ok(Some(ShellAction::Launch(crate::hotkeys::Launch::program(
                "/apps/mail"
            ))))
        );
        assert!(!shell.notifications.pane_state().is_visible());
        assert_eq!(
            press(&mut shell, ShellPart::Pane(PanePart::Card(mail))),
            Err(Refusal::NoSuchWidget),
            "the pane is closed"
        );
    });
}

/// **The pane's levels are set as their sliders set them, its switches
/// flip as a click flips them, and one with nothing behind it is out of
/// use.**
#[test]
fn the_panes_levels_and_switches_are_set_as_the_user_sets_them() {
    appearance::config::testing::with_scratch_config("acc-pane-levels", |_root| {
        let mut shell = shell();
        shell.toggle_notifications();
        let set = |shell: &mut DesktopShell, part: PanePart, value: f64| {
            shell.invoke(&ShellPart::Pane(part), Action::SetValue(value), 0.0, 0.0)
        };
        assert_eq!(set(&mut shell, PanePart::Volume, 42.4), Ok(None));
        assert_eq!(shell.notifications.volume(), 42, "on a whole step");
        assert_eq!(
            set(&mut shell, PanePart::Volume, f64::NAN),
            Err(Refusal::NotANumber)
        );
        assert_eq!(set(&mut shell, PanePart::Brightness, 155.0), Ok(None));
        assert_eq!(shell.notifications.brightness(), 100, "within its bounds");

        let toggle = |shell: &mut DesktopShell, setting: QuickSetting| {
            shell.invoke(
                &ShellPart::Pane(PanePart::Switch(setting)),
                Action::Toggle,
                0.0,
                0.0,
            )
        };
        assert_eq!(
            toggle(&mut shell, QuickSetting::WiFi),
            Err(Refusal::Disabled),
            "no radio is behind it"
        );
        let dark = shell
            .notifications
            .quick_setting_value(QuickSetting::DarkMode);
        assert_eq!(toggle(&mut shell, QuickSetting::DarkMode), Ok(None));
        assert_eq!(
            shell
                .notifications
                .quick_setting_value(QuickSetting::DarkMode),
            !dark
        );
        assert_eq!(shell.appearance.is_light(), dark, "the desktop turned");
        assert_eq!(
            press(&mut shell, ShellPart::Pane(PanePart::Volume)),
            Err(Refusal::NotApplicable {
                role: Role::Slider,
                action: "press"
            })
        );
    });
}

/// **A press that would not reach its part is refused, and changes
/// nothing**: a notification's menu open over the pane takes every press;
/// the pane's scrim covers the desktop's icons; an open flyout spends a
/// press anywhere else closing itself.
#[test]
fn a_press_that_would_not_reach_its_part_is_refused() {
    appearance::config::testing::with_scratch_config("acc-pane-covered", |_root| {
        let mut shell = shell();
        shell.icons.populate_defaults();
        let icon = shell.icons.icon_ids()[0];
        let chat = shell.notify(notif("Chat", "Lunch?"));
        shell.toggle_notifications();
        assert_eq!(
            press(&mut shell, ShellPart::Icon(icon)),
            Err(Refusal::Hidden),
            "under the pane's scrim"
        );
        assert!(shell.notifications.pane_state().is_visible());

        let (x, y) = node(&shell, ShellPart::Pane(PanePart::Card(chat)))
            .bounds
            .centre();
        shell.handle_mouse(&MouseEvent {
            x,
            y,
            kind: MouseEventKind::Press(MouseButton::Right),
        });
        assert!(shell.notification_menu.is_some(), "its menu opened");
        assert_eq!(
            press(&mut shell, ShellPart::Pane(PanePart::ClearAll)),
            Err(Refusal::Hidden)
        );
        let volume = shell.notifications.volume();
        assert_eq!(
            shell.invoke(
                &ShellPart::Pane(PanePart::Volume),
                Action::SetValue(f64::from(volume) / 2.0),
                0.0,
                0.0
            ),
            Err(Refusal::Hidden),
            "set under the menu"
        );
        assert_eq!(shell.notifications.volume(), volume);
        assert_eq!(shell.notifications.notifications().len(), 1);
        assert!(shell.notification_menu.is_some(), "the menu was closed");

        let mut flyout = DesktopShell::new(1920, 1080);
        press(&mut flyout, ShellPart::Control(Hit::VolumeIcon)).unwrap();
        assert_eq!(
            press(&mut flyout, ShellPart::Control(Hit::Clock)),
            Err(Refusal::Hidden),
            "a press on the clock would only close the flyout"
        );
        assert!(flyout.volume_flyout.is_visible());
        assert!(!flyout.calendar.visible);
        // The speaker is the flyout's own, and closes it; then the clock is
        // reached.
        press(&mut flyout, ShellPart::Control(Hit::VolumeIcon)).unwrap();
        assert!(!flyout.volume_flyout.is_visible());
        press(&mut flyout, ShellPart::Control(Hit::Clock)).unwrap();
        assert!(flyout.calendar.visible);
    });
}

/// **Whatever is open over everything takes every press, and nothing under
/// it is pressed**: the desktop's menu, the Run box, the overview.
#[test]
fn nothing_is_pressed_under_what_is_open_over_everything() {
    /// What opens something over everything, and what it is.
    type Opener = (&'static str, fn(&mut DesktopShell));
    let opened: [Opener; 3] = [
        ("the desktop's menu", |shell| {
            shell.open_desktop_menu(400.0, 300.0);
        }),
        ("the Run box", DesktopShell::toggle_run_dialog),
        ("the overview", |shell| {
            shell
                .overview
                .show(crate::overview::OverviewMode::AllWindows);
        }),
    ];
    for (what, open) in opened {
        let mut shell = shell();
        open(&mut shell);
        assert_eq!(
            press(&mut shell, ShellPart::Control(Hit::StartButton)),
            Err(Refusal::Hidden),
            "Start under {what}"
        );
        assert!(!shell.start_menu_open, "{what}: Start opened under it");
    }
}

/// The desktop's menu, opened with a right-click at `(x, y)`.
fn desktop_menu_at(shell: &mut DesktopShell, x: f32, y: f32) {
    shell.handle_mouse(&MouseEvent {
        x,
        y,
        kind: MouseEventKind::Press(MouseButton::Right),
    });
    assert!(shell.desktop_menu.is_visible(), "the desktop's menu opened");
}

/// The node in the shell's tree named `name`.
fn named(shell: &DesktopShell, name: &str) -> Node<ShellPart> {
    tree(shell)
        .walk()
        .find(|node| node.name == name)
        .unwrap_or_else(|| panic!("nothing named {name:?}"))
        .clone()
}

/// **The desktop's menu shows tools its rows, and a row is chosen as a
/// click chooses it**: "View" opens its submenu, held by the row, with the
/// menu staying open -- a click on it closed the whole menu -- and a check
/// in it is toggled by choosing it, which closes the menu.
#[test]
fn the_desktops_menu_is_chosen_from_as_a_click_chooses() {
    appearance::config::testing::with_scratch_config("acc-desktop-menu", |_root| {
        let mut shell = shell();
        desktop_menu_at(&mut shell, 400.0, 300.0);
        let menu = node(&shell, ShellPart::Menu(ShellMenu::Desktop, MenuPart::Menu));
        assert_eq!((menu.role, menu.name.as_str()), (Role::Menu, "Desktop"));
        let view = named(&shell, "View");
        assert_eq!(view.role, Role::MenuItem);
        assert_eq!(press(&mut shell, view.id), Ok(None));
        assert!(shell.desktop_menu.is_visible(), "the menu closed on View");
        assert_eq!(named(&shell, "View").children.len(), 1, "its submenu");

        let auto = named(&shell, "Auto arrange icons");
        let was = auto.value.clone();
        assert!(matches!(was, Some(Value::Check(_))), "{was:?}");
        assert_eq!(
            press(
                &mut shell,
                ShellPart::Menu(ShellMenu::Desktop, MenuPart::Menu)
            ),
            Err(Refusal::NotApplicable {
                role: Role::Menu,
                action: "press"
            })
        );
        assert_eq!(shell.invoke(&auto.id, Action::Toggle, 0.0, 0.0), Ok(None));
        assert!(!shell.desktop_menu.is_visible(), "chosen, the menu closes");
        assert_eq!(
            shell.invoke(&auto.id, Action::Toggle, 0.0, 0.0),
            Err(Refusal::NoSuchWidget),
            "the menu is closed"
        );

        desktop_menu_at(&mut shell, 400.0, 300.0);
        let view = named(&shell, "View").id;
        press(&mut shell, view).unwrap();
        assert_ne!(
            named(&shell, "Auto arrange icons").value,
            was,
            "the switch did not turn"
        );
        assert_eq!(
            shell.invoke(&named(&shell, "View").id, Action::Toggle, 0.0, 0.0),
            Err(Refusal::NotApplicable {
                role: Role::MenuItem,
                action: "toggle"
            }),
            "View is no check"
        );
    });
}

/// **A notification's menu, open over the pane, is the one a press
/// reaches**, and its rows are chosen through it; the pane's parts under it
/// are not.
#[test]
fn a_notifications_menu_is_chosen_from_over_the_pane() {
    appearance::config::testing::with_scratch_config("acc-notif-menu", |_root| {
        let mut shell = shell();
        let chat = shell.notify(notif("Chat", "Lunch?"));
        shell.toggle_notifications();
        let (x, y) = node(&shell, ShellPart::Pane(PanePart::Card(chat)))
            .bounds
            .centre();
        shell.handle_mouse(&MouseEvent {
            x,
            y,
            kind: MouseEventKind::Press(MouseButton::Right),
        });
        let menu = node(
            &shell,
            ShellPart::Menu(ShellMenu::Notification, MenuPart::Menu),
        );
        let rows: Vec<&str> = menu.children.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(
            rows,
            ["Turn off notifications from Chat", "Notification settings"]
        );
        assert_eq!(
            press(&mut shell, menu.children[1].id),
            Ok(Some(ShellAction::Launch(crate::launcher::settings_page(
                crate::launcher::NOTIFICATIONS_PAGE
            ))))
        );
        assert!(shell.notification_menu.is_none());
    });
}

/// **Of two menus open, only the one a press reaches is pressed**: with the
/// desktop's open and a notification's beside it, the desktop's takes every
/// press, so a row of the other is refused, and nothing changes.
#[test]
fn a_menu_under_another_is_not_pressed() {
    appearance::config::testing::with_scratch_config("acc-menu-under", |_root| {
        let mut shell = shell();
        shell.open_desktop_menu(400.0, 300.0);
        shell.open_notification_menu("Chat".to_owned(), 900.0, 500.0, false);
        assert!(
            shell.desktop_menu.is_visible() && shell.notification_menu.is_some(),
            "the test's premise: both open"
        );
        let row = node(
            &shell,
            ShellPart::Menu(ShellMenu::Notification, MenuPart::Menu),
        )
        .children[1]
            .id;
        assert_eq!(press(&mut shell, row), Err(Refusal::Hidden));
        assert!(
            shell.notification_menu.is_some(),
            "a refusal changes nothing"
        );
        assert!(shell.desktop_menu.is_visible());
    });
}

/// **The window switcher shows tools its windows, the one the switch goes
/// to chosen**: one chosen is stepped to as Tab steps, and one pressed ends
/// the switch on it, asking for it to be raised, as letting go does.
#[test]
fn the_window_switcher_is_stepped_and_let_go_of_as_the_user_does() {
    let mut shell = shell();
    let _asked = shell.apply_window_list(&WindowList::new(
        0,
        vec![
            WindowInfo::new(1, 40, "notes.md"),
            WindowInfo::new(2, 41, "Inbox"),
            WindowInfo::new(3, 42, "Calendar"),
        ],
    ));
    let switcher = |shell: &DesktopShell| {
        tree(shell)
            .walk()
            .any(|node| node.id == ShellPart::Switcher)
    };
    assert!(!switcher(&shell), "no switch under way");
    assert_eq!(
        press(&mut shell, ShellPart::SwitchTo(WindowId(1))),
        Err(Refusal::NoSuchWidget)
    );

    shell.start_alt_tab();
    let list = node(&shell, ShellPart::Switcher);
    assert_eq!(
        (list.role, list.name.as_str()),
        (Role::List, "Switch windows")
    );
    assert_eq!(list.children.len(), shell.switcher_windows().len());
    let chosen: Vec<ShellPart> = list
        .children
        .iter()
        .filter(|item| item.value == Some(Value::Chosen(true)))
        .map(|item| item.id)
        .collect();
    let goes_to = shell.switcher_windows()[shell.alt_tab_index].id;
    assert_eq!(
        chosen,
        [ShellPart::SwitchTo(goes_to)],
        "one chosen: where it goes"
    );
    for item in &list.children {
        assert!(
            list.bounds
                .contains(item.bounds.centre().0, item.bounds.centre().1),
            "{} is boxed in the strip",
            item.name
        );
    }

    let other = list
        .children
        .iter()
        .find(|item| item.id != ShellPart::SwitchTo(goes_to))
        .expect("another window")
        .clone();
    assert_eq!(shell.invoke(&other.id, Action::Choose, 0.0, 0.0), Ok(None));
    assert_eq!(node(&shell, other.id).value, Some(Value::Chosen(true)));
    let ShellPart::SwitchTo(id) = other.id else {
        panic!("{:?} is no window", other.id);
    };
    assert_eq!(
        press(&mut shell, other.id),
        Ok(Some(ShellAction::Control(crate::ShellRequest::window(
            id,
            crate::ShellControlAction::Activate
        ))))
    );
    assert!(!switcher(&shell), "let go of, the switch ends");
    assert_eq!(
        shell.invoke(&ShellPart::Switcher, Action::Press, 0.0, 0.0),
        Err(Refusal::NotApplicable {
            role: Role::List,
            action: "press"
        })
    );
}

/// **The clock opens the calendar, and its controls are pressed as they
/// are clicked**: the next month shown, a day chosen -- and chosen again,
/// left chosen, where a click would have cleared it -- and "Today" back.
#[test]
fn the_calendar_is_used_as_the_user_uses_it() {
    appearance::config::testing::with_scratch_config("acc-calendar", |_root| {
        let mut shell = shell();
        let control = |hit: CalendarHit| ShellPart::Control(Hit::CalendarControl(hit));
        press(&mut shell, ShellPart::Control(Hit::Clock)).unwrap();
        assert!(shell.calendar.visible);
        let popup = node(&shell, ShellPart::Calendar(CalendarPart::Popup));
        assert_eq!(
            (popup.role, popup.name.as_str()),
            (Role::Dialog, "Calendar")
        );
        let month = shell.calendar.view_month;
        assert_eq!(press(&mut shell, control(CalendarHit::NextPage)), Ok(None));
        assert_ne!(shell.calendar.view_month, month, "the next month");

        let index = shell
            .calendar
            .generate_grid()
            .iter()
            .position(|cell| cell.current_month && cell.day == 15)
            .unwrap();
        let day = control(CalendarHit::Day(index));
        assert_eq!(node(&shell, day).role, Role::GridCell);
        for _ in 0..2 {
            assert_eq!(shell.invoke(&day, Action::Choose, 0.0, 0.0), Ok(None));
            assert_eq!(node(&shell, day).value, Some(Value::Chosen(true)));
        }
        assert_eq!(
            shell.invoke(&day, Action::SetText("x".to_owned()), 0.0, 0.0),
            Err(Refusal::NotApplicable {
                role: Role::GridCell,
                action: "set the text of"
            })
        );
        assert_eq!(
            press(&mut shell, ShellPart::Calendar(CalendarPart::Grid)),
            Err(Refusal::NotApplicable {
                role: Role::Grid,
                action: "press"
            })
        );
        assert_eq!(press(&mut shell, control(CalendarHit::Today)), Ok(None));
        assert_eq!(shell.calendar.view_month, month, "back to today's month");
        assert_eq!(
            press(&mut shell, control(CalendarHit::Today)),
            Err(Refusal::NoSuchWidget),
            "no Today on today's month"
        );
    });
}

/// **The overview shows tools its windows, and is used as the pointer and
/// the keys use it**: the search's text set as typing sets it, a close
/// button pressed with its card lit first, a card pressed as clicked --
/// which switches to its window and closes the overview.
#[test]
fn the_overview_is_used_as_the_user_uses_it() {
    let mut shell = shell();
    let _asked = shell.apply_window_list(&WindowList::new(
        0,
        vec![
            WindowInfo::new(1, 40, "notes.md"),
            WindowInfo::new(2, 41, "Inbox"),
        ],
    ));
    assert_eq!(
        press(&mut shell, ShellPart::Overview(OverviewPart::Card(1))),
        Err(Refusal::NoSuchWidget),
        "not while it is closed"
    );
    shell
        .overview
        .show(crate::overview::OverviewMode::AllWindows);
    let root = node(&shell, ShellPart::Overview(OverviewPart::Overview));
    assert_eq!((root.role, root.name.as_str()), (Role::Dialog, "Overview"));
    let mut cards: Vec<String> = tree(&shell)
        .walk()
        .filter(|n| matches!(n.id, ShellPart::Overview(OverviewPart::Card(_))))
        .map(|n| n.name.clone())
        .collect();
    cards.sort();
    assert_eq!(cards, ["Inbox", "notes.md"]);

    let search = ShellPart::Overview(OverviewPart::Search);
    assert_eq!(
        shell.invoke(&search, Action::SetText("inb".to_owned()), 0.0, 0.0),
        Ok(None)
    );
    assert_eq!(
        node(&shell, search).value,
        Some(Value::Text("inb".to_owned()))
    );
    assert_eq!(shell.overview.search_results, [2]);

    assert_eq!(
        press(&mut shell, ShellPart::Overview(OverviewPart::Close(1))),
        Ok(Some(ShellAction::Control(crate::ShellRequest::window(
            WindowId(1),
            crate::ShellControlAction::Close
        ))))
    );
    assert_eq!(shell.overview.hovered_window, Some(1), "lit to be closed");
    assert_eq!(
        shell.invoke(
            &ShellPart::Overview(OverviewPart::Card(2)),
            Action::Choose,
            0.0,
            0.0
        ),
        Ok(None)
    );
    assert_eq!(
        shell.overview.hovered_window,
        Some(2),
        "lit as the pointer lights it"
    );
    assert_eq!(
        press(&mut shell, ShellPart::Overview(OverviewPart::Card(2))),
        Ok(Some(ShellAction::Control(crate::ShellRequest::window(
            WindowId(2),
            crate::ShellControlAction::Activate
        ))))
    );
    assert!(
        !shell.overview.visible,
        "a window taken closes the overview"
    );
    assert_eq!(
        press(&mut shell, search),
        Err(Refusal::NoSuchWidget),
        "closed"
    );
}

/// The Run box up, its Browse's chooser over it, showing one file, `hello`.
fn browsing() -> DesktopShell {
    let mut shell = shell();
    shell.toggle_run_dialog();
    shell.open_run_box_chooser();
    shell.set_chooser_entries(vec![guitk::dialog::DirEntry {
        name: "hello".into(),
        is_dir: false,
        size: 1,
        modified_timestamp: 0,
        extension: std::ffi::OsString::new(),
    }]);
    shell
}

/// **The file chooser is the file dialog's own parts, where the shell draws
/// it, and what it chooses goes where it was put up for**: a file the Run
/// box's Browse finds is put on the box's line, and the chooser comes down.
/// Under it nothing is pressed.
#[test]
fn the_run_boxs_chooser_is_used_as_the_user_uses_it() {
    let mut shell = browsing();
    let (x, y, w, h) = shell.chooser_rect();
    let entry = node(&shell, ShellPart::Chooser(DialogTarget::Entry(0)));
    assert_eq!(entry.name, "hello");
    let (cx, cy) = entry.bounds.centre();
    assert!(
        Rect::new(x, y, w, h).contains(cx, cy),
        "where the shell draws it: {:?} in {:?}",
        entry.bounds,
        (x, y, w, h)
    );
    assert_eq!(
        press(&mut shell, ShellPart::Control(Hit::StartButton)),
        Err(Refusal::Hidden),
        "nothing under it"
    );
    assert_eq!(
        shell.invoke(
            &ShellPart::Chooser(DialogTarget::Entry(0)),
            Action::Choose,
            0.0,
            0.0
        ),
        Ok(None)
    );
    assert_eq!(
        press(&mut shell, ShellPart::Chooser(DialogTarget::Confirm)),
        Ok(None)
    );
    assert!(!shell.chooser_open(), "chosen, it comes down");
    assert_eq!(shell.run_dialog.line(), "/hello");
    assert_eq!(
        press(&mut shell, ShellPart::Chooser(DialogTarget::Cancel)),
        Err(Refusal::NoSuchWidget),
        "down"
    );

    let mut cancelled = browsing();
    press(&mut cancelled, ShellPart::Chooser(DialogTarget::Cancel)).unwrap();
    assert!(!cancelled.chooser_open());
    assert_eq!(cancelled.run_dialog.line(), "", "the box as it was");
}

/// **The character picker over a field is its own parts, where the shell
/// draws it, and a character pressed is typed into the field**, the picker
/// taken down; while it is up, the chooser under it is not pressed.
#[test]
fn the_character_picker_types_into_its_field() {
    let mut shell = browsing();
    shell.open_char_picker(crate::MenuField::RunBox);
    let root = tree(&shell);
    assert!(
        matches!(
            root.children.last().map(|n| n.id),
            Some(ShellPart::CharPicker(_))
        ),
        "drawn over everything"
    );
    assert_eq!(
        press(&mut shell, ShellPart::Chooser(DialogTarget::Cancel)),
        Err(Refusal::Hidden),
        "the picker is over the chooser"
    );
    assert!(shell.chooser_open());

    let picker = root.children.last().unwrap().clone();
    let field = shell.run_dialog.field_rect();
    assert_eq!(
        (picker.bounds.x, picker.bounds.y),
        (field.x, field.bottom()),
        "under its field, where the shell draws it"
    );
    let cell = picker
        .walk()
        .find(|n| matches!(n.id, ShellPart::CharPicker(charpicker::Target::Cell(_))))
        .unwrap()
        .clone();
    let (cx, cy) = cell.bounds.centre();
    assert!(picker.bounds.contains(cx, cy), "{:?}", cell.bounds);
    // A cell is named by its character's name, and holds the character.
    let Some(Value::Text(character)) = cell.value.clone() else {
        panic!("{:?} holds no character", cell.name);
    };
    press(&mut shell, cell.id).unwrap();
    assert!(!shell.char_picker_open(), "one pick, then gone");
    press(&mut shell, ShellPart::Chooser(DialogTarget::Cancel)).unwrap();
    assert!(
        shell.run_dialog.line().ends_with(character.as_str()),
        "{character:?} typed into {:?}",
        shell.run_dialog.line()
    );
}

/// **The Run box is among the shell's parts while it is up, and a line run
/// from it is started as after a click**; Browse puts the chooser up over
/// it, and under the chooser the box is not pressed.
#[test]
fn the_run_box_runs_a_line_and_browses() {
    let mut running = shell();
    running.toggle_run_dialog();
    let field = ShellPart::RunBox(RunPart::Field);
    assert_eq!(node(&running, field).name, "Open:");
    running
        .invoke(&field, Action::SetText("calculator".to_owned()), 0.0, 0.0)
        .unwrap();
    let launched = running.invoke(&field, Action::Press, 0.0, 0.0).unwrap();
    assert!(
        matches!(launched, Some(ShellAction::Launch(_))),
        "{launched:?}"
    );
    assert!(!running.run_dialog.is_visible(), "run, the box goes");
    assert_eq!(press(&mut running, field), Err(Refusal::NoSuchWidget));

    let mut browsing = shell();
    browsing.toggle_run_dialog();
    assert_eq!(
        press(&mut browsing, ShellPart::RunBox(RunPart::Browse)),
        Ok(None)
    );
    assert!(browsing.chooser_open(), "Browse puts the chooser up");
    assert_eq!(
        press(&mut browsing, ShellPart::RunBox(RunPart::Cancel)),
        Err(Refusal::Hidden),
        "the chooser is over the box"
    );
    assert!(browsing.run_dialog.is_visible());
}

/// A shell with one focused window, "notes.md", and a second with no title.
fn two_windows() -> DesktopShell {
    let mut shell = shell();
    let _asked = shell.apply_window_list(&WindowList::new(
        0,
        vec![
            WindowInfo::new(1, 40, "notes.md"),
            WindowInfo::new(2, 41, ""),
        ],
    ));
    shell.focused_window = Some(WindowId(1));
    shell
}

/// **The tiling overlay shows tools its zones, each named by where it is,
/// and its picker's layouts**; a layout is chosen as the user chooses it --
/// the pointer to the top edge, onto the layout, pressed -- and a zone
/// pressed tiles the focused window into it, as a click does.
#[test]
fn the_tiling_overlay_tiles_and_changes_its_layout() {
    let mut shell = two_windows();
    assert!(shell.toggle_zone_overlay(), "the premise: it opens");
    let snap = node(&shell, ShellPart::Snap);
    assert_eq!(
        snap.description.as_deref(),
        Some(shell.snap.active_preset().label())
    );
    let zones: Vec<&str> = snap
        .children
        .iter()
        .filter(|n| matches!(n.id, ShellPart::Control(Hit::SnapZone(_))))
        .map(|n| n.name.as_str())
        .collect();
    assert_eq!(zones.len(), shell.snap.layout().zones.len(), "{zones:?}");
    let picker = node(&shell, ShellPart::SnapLayouts);
    assert!(!picker.shown, "the picker waits for the pointer");
    assert_eq!(picker.children.len(), SnapLayoutPreset::all().len());
    assert_eq!(
        press(&mut shell, ShellPart::Control(Hit::StartButton)),
        Err(Refusal::Hidden),
        "under the overlay"
    );

    let other = SnapLayoutPreset::all()
        .iter()
        .copied()
        .find(|&preset| preset != shell.snap.active_preset())
        .expect("a second layout");
    assert_eq!(
        shell.invoke(&ShellPart::SnapLayout(other), Action::Choose, 0.0, 0.0),
        Ok(None)
    );
    assert_eq!(shell.snap.active_preset(), other);
    assert_eq!(
        node(&shell, ShellPart::SnapLayout(other)).value,
        Some(Value::Chosen(true))
    );

    let zone = node(&shell, ShellPart::Snap)
        .children
        .iter()
        .find(|n| matches!(n.id, ShellPart::Control(Hit::SnapZone(_))))
        .expect("a zone")
        .id;
    let tiled = press(&mut shell, zone);
    assert!(
        matches!(tiled, Ok(Some(ShellAction::Control(_)))),
        "{tiled:?}"
    );
    assert!(!shell.snap.is_overlay_visible(), "chosen, it goes");
    assert_eq!(press(&mut shell, zone), Err(Refusal::NoSuchWidget));
}

/// **The list a shut down waits on says what it says, names the programs
/// still open, and is answered by its buttons** as a click answers it:
/// Cancel leaves the session as it is, and going ahead anyway carries the
/// shut down out. Nothing under it is pressed.
#[test]
fn the_list_a_shut_down_waits_on_is_answered_by_its_buttons() {
    let listing = || {
        let mut shell = two_windows();
        let asked = shell.choose_power(crate::power::PowerChoice::ShutDown);
        assert!(matches!(asked, ShellAction::ControlAll(_)), "{asked:?}");
        shell.osd_clock_ms = shell.osd_clock_ms.saturating_add(crate::ENDING_GRACE_MS);
        assert!(shell.tick_ending(), "the premise: the list goes up");
        shell
    };
    let mut shell = listing();
    let ending = node(&shell, ShellPart::Ending);
    assert_eq!(ending.name, "2 programs are still open");
    assert!(
        ending
            .description
            .as_deref()
            .is_some_and(|said| said.contains("Cancel, and close it yourself")),
        "{:?}",
        ending.description
    );
    let programs = node(&shell, ShellPart::EndingPrograms);
    let names: Vec<(ShellPart, &str)> = programs
        .children
        .iter()
        .map(|n| (n.id, n.name.as_str()))
        .collect();
    assert_eq!(
        names[0],
        (ShellPart::EndingProgram(WindowId(1)), "notes.md")
    );
    assert_eq!(names.len(), 2);
    assert!(
        !names[1].1.is_empty(),
        "a window with no title is still named"
    );
    assert_eq!(
        node(&shell, ShellPart::Control(Hit::EndingAnyway)).name,
        "Shut down anyway"
    );
    assert_eq!(
        press(&mut shell, ShellPart::Control(Hit::StartButton)),
        Err(Refusal::Hidden),
        "under the list"
    );
    assert_eq!(
        press(&mut shell, ShellPart::EndingProgram(WindowId(1))),
        Err(Refusal::NotApplicable {
            role: Role::ListItem,
            action: "press"
        })
    );

    assert_eq!(
        press(&mut shell, ShellPart::Control(Hit::EndingCancel)),
        Ok(None)
    );
    assert!(shell.ending.is_none(), "cancelled");
    assert_eq!(
        press(&mut shell, ShellPart::Control(Hit::EndingCancel)),
        Err(Refusal::NoSuchWidget)
    );

    let mut anyway = listing();
    let went = press(&mut anyway, ShellPart::Control(Hit::EndingAnyway));
    assert!(matches!(went, Ok(Some(ShellAction::Launch(_)))), "{went:?}");
    assert!(anyway.ending.is_none());
}

/// **A zone whose middle lies under the open layout picker is refused**: a
/// click there would choose a layout, not tile the window. Several layouts
/// put a zone's middle under the picker; the first is enough.
#[test]
fn a_zone_under_the_open_picker_is_not_pressed() {
    let mut shell = two_windows();
    assert!(shell.toggle_zone_overlay(), "the premise: it opens");
    let covered = SnapLayoutPreset::all().iter().find_map(|&preset| {
        shell.snap.set_layout(preset);
        shell.snap.show_picker();
        shell.snap.layout().zones.iter().find_map(|zone| {
            let (x, y) = (zone.x + zone.width / 2.0, zone.y + zone.height / 2.0);
            shell.snap.picker_hit(x, y).then_some(zone.id)
        })
    });
    let zone = covered.expect("the premise: a zone's middle under the picker");
    assert_eq!(
        press(&mut shell, ShellPart::Control(Hit::SnapZone(zone))),
        Err(Refusal::Hidden)
    );
    assert!(shell.snap.is_overlay_visible(), "nothing chosen");
}

/// **The list a shut down waits on is not pressed under what the shell asks
/// for a press first**: the overview, and the notification pane's scrim.
#[test]
fn the_shut_down_list_is_not_pressed_under_the_overview_or_the_pane() {
    let listing = || {
        let mut shell = two_windows();
        let _asked = shell.choose_power(crate::power::PowerChoice::ShutDown);
        shell.osd_clock_ms = shell.osd_clock_ms.saturating_add(crate::ENDING_GRACE_MS);
        assert!(shell.tick_ending(), "the premise: the list goes up");
        shell
    };
    let mut overview = listing();
    overview
        .overview
        .show(crate::overview::OverviewMode::AllWindows);
    assert_eq!(
        press(&mut overview, ShellPart::Control(Hit::EndingCancel)),
        Err(Refusal::Hidden),
        "under the overview"
    );
    let mut pane = listing();
    pane.toggle_notifications();
    assert_eq!(
        press(&mut pane, ShellPart::Control(Hit::EndingCancel)),
        Err(Refusal::Hidden),
        "under the pane's scrim"
    );
    assert!(pane.ending_listing(), "still waiting");
}

/// **The notifications popped up in the corner are seen -- each named and
/// described as its card in the pane is, with its close button -- and one is
/// opened as a click on it opens it**: read in the pane, its program asked
/// for, and gone.
#[test]
fn a_popped_up_notification_is_opened_as_clicked() {
    appearance::config::testing::with_scratch_config("acc-toast-open", |_root| {
        let mut s = shell();
        let mut lunch = notif("Chat", "Lunch?");
        lunch.body = "At noon".to_owned();
        lunch.action = Some("/bin/chat".to_owned());
        let id = s.notify(lunch);
        s.notify(notif("Mail", ""));
        s.advance_toasts(1_000);

        let toasts = node(&s, ShellPart::Toasts);
        assert_eq!(
            (toasts.role, toasts.name.as_str()),
            (Role::List, "New notifications")
        );
        let items: Vec<(&str, Option<&str>)> = toasts
            .children
            .iter()
            .map(|n| (n.name.as_str(), n.description.as_deref()))
            .collect();
        assert_eq!(
            items,
            [("Lunch?", Some("Chat. At noon")), ("Mail", Some("Mail"))],
            "top to bottom, the oldest first"
        );
        let close = &toasts.children[0].children[0];
        assert_eq!(
            (close.id, close.role, close.name.as_str()),
            (ShellPart::ToastClose(id), Role::Button, "Close")
        );

        let said = press(&mut s, ShellPart::Toast(id)).unwrap();
        assert!(matches!(said, Some(ShellAction::Launch(_))), "{said:?}");
        let filed = s.notifications.notifications();
        assert!(
            filed.iter().any(|n| n.id == id && n.read),
            "read in the pane"
        );
        s.advance_toasts(1_000);
        assert!(!s.toasts.ids().contains(&id), "and gone");
    });
}

/// **A pop-up's close button pressed closes it, as clicked**: it goes, and
/// its notification stays in the pane, unread. With none popped up there is
/// no list, and no pop-up to press.
#[test]
fn a_popped_up_notification_is_closed_as_clicked() {
    appearance::config::testing::with_scratch_config("acc-toast-close", |_root| {
        let mut s = shell();
        let id = s.notify(notif("Chat", "Lunch?"));
        s.advance_toasts(1_000);
        assert_eq!(press(&mut s, ShellPart::ToastClose(id)), Ok(None));
        s.advance_toasts(1_000);
        assert!(s.toasts.ids().is_empty());
        let filed = s.notifications.notifications();
        assert!(
            filed.iter().any(|n| n.id == id && !n.read),
            "unread in the pane"
        );
        assert!(tree(&s).walk().all(|n| n.id != ShellPart::Toasts));
        assert_eq!(
            press(&mut s, ShellPart::Toast(id)),
            Err(Refusal::NoSuchWidget)
        );
    });
}

/// **A press a click could not make on a pop-up is refused, and nothing
/// changes**: while a menu opened from one is up -- a click there only
/// closes the menu -- and while it is still sliding in from beyond the
/// screen's edge.
#[test]
fn a_popped_up_notification_a_click_cannot_reach_is_refused() {
    appearance::config::testing::with_scratch_config("acc-toast-refused", |_root| {
        let mut s = shell();
        let id = s.notify(notif("Chat", "Lunch?"));
        assert_eq!(
            press(&mut s, ShellPart::Toast(id)),
            Err(Refusal::Hidden),
            "beyond the screen's edge, arriving"
        );

        s.advance_toasts(1_000);
        let toast = s.toasts.placed()[0];
        let _menu = s.handle_toast_mouse(&MouseEvent {
            x: toast.rect.x + 40.0,
            y: toast.rect.y + toast.rect.h / 2.0,
            kind: MouseEventKind::Press(MouseButton::Right),
        });
        assert!(s.notification_menu.is_some(), "its menu is up");
        assert_eq!(
            press(&mut s, ShellPart::Toast(id)),
            Err(Refusal::Hidden),
            "under its menu"
        );
        assert!(s.notification_menu.is_some(), "and stays up");
        assert!(s.notifications.notifications().iter().all(|n| !n.read));
        assert_eq!(
            s.invoke(&ShellPart::Toast(id), Action::Toggle, 0.0, 0.0),
            Err(Refusal::NotApplicable {
                role: Role::ListItem,
                action: "toggle"
            })
        );
    });
}

/// **The on-screen display is seen while it is up, to be read and not
/// used** -- a level by what it measures -- and so is the card saying how
/// to move the wallpaper, while it is being moved.
#[test]
fn the_on_screen_display_and_the_wallpaper_card_are_read() {
    let mut s = shell();
    assert!(
        tree(&s)
            .walk()
            .all(|n| !matches!(n.id, ShellPart::Osd(_) | ShellPart::WallpaperMove)),
        "read with nothing up"
    );
    s.osd.show(crate::osd::OsdKind::Brightness { level: 70 }, 0);
    let display = node(&s, ShellPart::Osd(OsdPart::Display));
    let level = display.children[0].clone();
    assert_eq!(
        (level.role, level.name.as_str()),
        (Role::ProgressBar, "Brightness")
    );
    assert_eq!(
        level.value,
        Some(Value::Progress {
            value: 70.0,
            max: 100.0
        })
    );
    assert_eq!(
        press(&mut s, level.id),
        Err(Refusal::NotApplicable {
            role: Role::ProgressBar,
            action: "press"
        })
    );
    assert_eq!(
        press(&mut s, ShellPart::Osd(OsdPart::Overlay(u64::MAX))),
        Err(Refusal::NoSuchWidget)
    );

    assert_eq!(
        press(&mut s, ShellPart::WallpaperMove),
        Err(Refusal::NoSuchWidget),
        "the card refused as there with the wallpaper not being moved"
    );
    s.wallpaper_room = Some((-1000.0, 0.0));
    assert_eq!(
        s.activate_desktop_menu_item(DesktopShell::MENU_MOVE_WALLPAPER),
        ShellAction::Consumed
    );
    let card = node(&s, ShellPart::WallpaperMove);
    assert_eq!(
        (card.role, card.name.as_str()),
        (Role::Dialog, "Move the wallpaper")
    );
    assert_eq!(
        card.description.as_deref(),
        Some(
            "Drag the picture, or use the arrow keys, to choose the part that shows. \
             Enter keeps it here -- Esc puts it back."
        )
    );
    // Where it is drawn: the card's panel, centred across the screen.
    let drawn = s
        .render_wallpaper_move()
        .expect("the card is drawn")
        .commands
        .into_iter()
        .find_map(|c| match c {
            guitk::render::RenderCommand::FillRect {
                x,
                y,
                width,
                height,
                ..
            } => Some(Rect::new(x, y, width, height)),
            _ => None,
        })
        .expect("the card's panel");
    assert_eq!(card.bounds, drawn);
    assert!(
        (drawn.x + drawn.w / 2.0 - 960.0).abs() < 0.5,
        "the card is not centred: {drawn:?}"
    );
    assert_eq!(
        press(&mut s, ShellPart::WallpaperMove),
        Err(Refusal::NotApplicable {
            role: Role::Dialog,
            action: "press"
        })
    );
}

/// A shell with a clock and a note out on its desktop, added as the user
/// adds them -- the desktop menu's rows -- and their ids, its layout saved.
fn with_widgets() -> (DesktopShell, WidgetInstanceId, WidgetInstanceId) {
    let mut s = shell();
    for row in [DesktopShell::MENU_ADD_CLOCK, DesktopShell::MENU_ADD_NOTE] {
        s.open_desktop_menu(900.0, 500.0);
        assert!(s.activate_desktop_menu_item(row).changed(), "not added");
    }
    s.dismiss_popups();
    let of = |kind: WidgetKind| {
        s.widgets
            .all_widgets()
            .iter()
            .find(|w| w.kind == kind)
            .map(|w| w.id)
            .expect("added")
    };
    let (clock, note) = (of(WidgetKind::Clock), of(WidgetKind::Notes));
    let _saved = s.take_widgets_dirty();
    (s, clock, note)
}

/// A key typing `text`.
fn typing(text: &str) -> crate::KeyEvent {
    crate::KeyEvent {
        key: crate::Key::A,
        pressed: true,
        modifiers: crate::Modifiers::NONE,
        text: text.to_owned(),
    }
}

/// **The widgets out on the desktop are seen, over its icons** -- between
/// the icons and the taskbar, as they are drawn -- each named by its title,
/// a clock reading what the taskbar's clock reads.
#[test]
fn the_desktops_widgets_are_seen_over_its_icons() {
    let (s, clock, note) = with_widgets();
    let root = tree(&s);
    let order: Vec<ShellPart> = root.children.iter().map(|n| n.id).collect();
    assert_eq!(
        order[..3],
        [
            ShellPart::Icons,
            ShellPart::Widget(WidgetPart::Layer),
            ShellPart::Taskbar
        ]
    );
    // Either side of a minute turning over between the two.
    let before = s.live_readings().clock_time;
    let time = node(&s, ShellPart::Widget(WidgetPart::Time(clock)));
    let after = s.live_readings().clock_time;
    assert!(time.name == before || time.name == after, "{:?}", time.name);
    let written = node(&s, ShellPart::Widget(WidgetPart::Note(note)));
    assert_eq!(
        (written.role, written.name.as_str()),
        (Role::TextArea, "Quick Notes")
    );
    assert!(!written.focused);
}

/// **A calendar widget shows the month today is in**, made by the shell's
/// own calendar, the popup's -- and the month is made only while a calendar
/// is out.
#[test]
fn a_calendar_widget_shows_this_month() {
    let mut s = shell();
    assert!(
        s.live_readings().month.days.is_empty(),
        "a month made with no calendar out"
    );
    s.open_desktop_menu(900.0, 500.0);
    assert!(
        s.activate_desktop_menu_item(DesktopShell::MENU_ADD_CALENDAR)
            .changed()
    );
    s.dismiss_popups();
    let id = s
        .widgets
        .all_widgets()
        .iter()
        .find(|w| w.kind == WidgetKind::Calendar)
        .map(|w| w.id)
        .expect("added");
    // Either side of a month turning over between the two.
    let before = s.live_readings().month.title;
    let month = node(&s, ShellPart::Widget(WidgetPart::Month(id)));
    let after = s.live_readings().month.title;
    assert!(
        month.name == before || month.name == after,
        "{:?}",
        month.name
    );
    assert_eq!(month.children.len(), 42);
    let today = month
        .children
        .iter()
        .filter(|day| {
            day.description
                .as_deref()
                .is_some_and(|said| said.starts_with("today"))
        })
        .count();
    assert_eq!(today, 1, "today marked once");
}

/// **A note's text is set as the user would set it**: the note opened with
/// a click on it, which gives it the keyboard, and the text pasted over all
/// of it -- its layout to be saved, as after typing -- and what is typed next
/// follows it. Set again while open, it is not clicked again, which would
/// put the caret where the click landed.
#[test]
fn a_notes_text_is_set_as_the_user_would() {
    let (mut s, _, note) = with_widgets();
    let part = ShellPart::Widget(WidgetPart::Note(note));
    let words = |s: &DesktopShell| s.widgets.get(note).expect("placed").state_text.clone();
    assert_eq!(
        s.invoke(&part, Action::SetText("milk".to_owned()), 0.0, 0.0),
        Ok(None)
    );
    assert_eq!(words(&s), "milk");
    assert_eq!(s.widgets.writing_note(), Some(note), "not opened");
    assert!(node(&s, part).focused);
    assert!(s.take_widgets_dirty(), "the words are not saved");
    let _typed = s.handle_desktop_key(&typing("!"));
    assert_eq!(words(&s), "milk!");

    // Lines enough that a click on the note's middle lands among them.
    let long = (1..=40)
        .map(|n| format!("line {n}\n"))
        .collect::<Vec<_>>()
        .concat();
    assert_eq!(
        s.invoke(&part, Action::SetText(long.clone()), 0.0, 0.0),
        Ok(None)
    );
    assert_eq!(s.invoke(&part, Action::Focus, 0.0, 0.0), Ok(None));
    let _typed = s.handle_desktop_key(&typing("END"));
    assert_eq!(words(&s), format!("{long}END"), "clicked again");
}

/// **A note given the keyboard is opened as clicked**, and opening it
/// changes no words, so nothing is saved.
#[test]
fn a_note_given_the_keyboard_is_opened() {
    let (mut s, _, note) = with_widgets();
    let part = ShellPart::Widget(WidgetPart::Note(note));
    assert_eq!(s.invoke(&part, Action::Focus, 0.0, 0.0), Ok(None));
    assert_eq!(s.widgets.writing_note(), Some(note));
    assert!(!s.take_widgets_dirty(), "opening a note saved the layout");
}

/// **A note a click could not reach is refused, and nothing changes**:
/// under the start menu -- a click there only closes it -- and under the
/// pop-ups in the corner, which take every press in their stack. And only a
/// note is written in: a clock's time is not, nor is a note pressed.
#[test]
fn a_note_a_click_cannot_reach_is_refused() {
    appearance::config::testing::with_scratch_config("acc-note-refused", |_root| {
        let (mut s, clock, note) = with_widgets();
        let part = ShellPart::Widget(WidgetPart::Note(note));
        let set = |s: &mut DesktopShell| s.invoke(&part, Action::SetText("x".to_owned()), 0.0, 0.0);
        s.toggle_start_menu();
        assert_eq!(set(&mut s), Err(Refusal::Hidden), "under the start menu");
        assert!(s.start_menu_open, "the refusal closed the menu");
        assert_eq!(s.widgets.writing_note(), None);
        s.toggle_start_menu();

        // The grid moved so that the note's middle is under the stack.
        s.notify(notif("Chat", "Lunch?"));
        s.advance_toasts(1_000);
        let stack = s.toasts.extent().expect("one is up");
        let (sx, sy) = stack.centre();
        let (x, y, w, h) = s.widgets.content_rect(note).expect("placed");
        s.widgets.grid.origin_x += sx - (x + w / 2.0);
        s.widgets.grid.origin_y += sy - (y + h / 2.0);
        let (x, y, w, h) = s.widgets.content_rect(note).expect("placed");
        assert!(stack.contains(x + w / 2.0, y + h / 2.0));
        assert_eq!(set(&mut s), Err(Refusal::Hidden), "under the pop-ups");
        assert_eq!(s.widgets.writing_note(), None);
        assert_eq!(s.widgets.get(note).expect("placed").state_text, "");

        assert_eq!(
            s.invoke(
                &ShellPart::Widget(WidgetPart::Time(clock)),
                Action::SetText("12:00".to_owned()),
                0.0,
                0.0
            ),
            Err(Refusal::NotApplicable {
                role: Role::Label,
                action: "set the text of"
            })
        );
        assert_eq!(
            press(&mut s, part),
            Err(Refusal::NotApplicable {
                role: Role::TextArea,
                action: "press"
            })
        );
        assert_eq!(
            s.invoke(
                &ShellPart::Widget(WidgetPart::Note(u64::MAX)),
                Action::Focus,
                0.0,
                0.0
            ),
            Err(Refusal::NoSuchWidget)
        );
    });
}

/// **An icon under a widget is refused**: the widget is drawn over it and
/// takes the click, so the icon is neither chosen nor opened -- and the
/// refusal presses nothing.
#[test]
fn an_icon_under_a_widget_is_refused() {
    let mut s = shell();
    s.icons.populate_defaults();
    let pc = s
        .icons
        .icon_ids()
        .into_iter()
        .find(|&id| s.icons.get_icon(id).is_some_and(|i| i.label == "This PC"))
        .expect("This PC");
    let (x, y) = s.icons.icon_rect(pc).expect("placed").centre();
    let cell = s.widgets.pixel_to_grid(x, y).expect("on the grid");
    s.widgets
        .add_widget(WidgetKind::Clock, cell)
        .expect("the grid is empty");
    assert!(
        s.widgets.hit_test(x, y).is_some(),
        "the clock is not over it"
    );
    let icon = ShellPart::Icon(pc);
    assert_eq!(
        s.invoke(&icon, Action::Choose, 0.0, 0.0),
        Err(Refusal::Hidden)
    );
    assert_eq!(press(&mut s, icon), Err(Refusal::Hidden));
    assert!(s.icons.selected_ids().is_empty());
    assert!(
        s.widget_drag.is_none(),
        "the refusal took hold of the clock"
    );
}
