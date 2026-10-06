//! Tests for the notification pane as tools see it: what each part is
//! called and holds, that its box is where the pane draws it and takes a
//! press on it, and that a part scrolled out of the list is brought into it.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_cmp,
    clippy::cast_precision_loss
)]

use guitk::event::{MouseButton, MouseEvent, MouseEventKind};
use guitk::palette::Palette;

use super::*;
use crate::notif_pane::{
    AppSettingKind, NotifPaneEvent, NotifPriority, Notification, PaneState, SettingValue,
};

const SCREEN_W: f32 = 1920.0;
const SCREEN_H: f32 = 800.0;

/// A notification from `app`, titled `title`, saying `body`.
fn notif(app: &str, title: &str, body: &str) -> Notification {
    Notification {
        id: 0,
        app_name: app.to_owned(),
        title: title.to_owned(),
        body: body.to_owned(),
        timestamp: 1000,
        priority: NotifPriority::Normal,
        read: false,
        action: None,
        silent: false,
    }
}

/// An open pane holding `n` notifications, from `apps` programs in turn.
fn pane(n: usize, apps: usize) -> NotificationPane {
    let mut pane = NotificationPane::new();
    pane.show();
    pane.state = PaneState::Visible;
    for i in 0..n {
        pane.push_notification(notif(
            &format!("App{}", i % apps.max(1)),
            &format!("N{i}"),
            "Hello",
        ));
    }
    pane.current_time = 1000;
    pane.set_screen_height(SCREEN_H);
    pane
}

/// The pane as tools see it.
fn tree(pane: &NotificationPane) -> Node<PanePart> {
    pane.automation(SCREEN_W, SCREEN_H)
}

/// The node of `part`.
fn node(pane: &NotificationPane, part: PanePart) -> Node<PanePart> {
    tree(pane)
        .walk()
        .find(|node| node.id == part)
        .unwrap_or_else(|| panic!("no {part:?}"))
        .clone()
}

/// A left press at `(x, y)` on the screen, and what the pane said of it.
fn press_at(pane: &mut NotificationPane, (x, y): (f32, f32)) -> Vec<NotifPaneEvent> {
    pane.drain_events();
    pane.handle_mouse_event(
        &MouseEvent {
            x,
            y,
            kind: MouseEventKind::Press(MouseButton::Left),
        },
        SCREEN_W,
        SCREEN_H,
    );
    pane.drain_events()
}

/// Press `part` at the middle of its box, as a tool's press lands.
fn press(pane: &mut NotificationPane, part: PanePart) -> Vec<NotifPaneEvent> {
    let at = pane
        .press_point(part, SCREEN_W, SCREEN_H)
        .unwrap_or_else(|| panic!("{part:?} is not on screen"));
    press_at(pane, at)
}

/// The rectangles the pane paints, on the screen.
fn painted(pane: &NotificationPane) -> Vec<Rect> {
    let left = SCREEN_W - PANE_WIDTH;
    pane.render(&Palette::for_mode(false), SCREEN_W, SCREEN_H)
        .iter()
        .filter_map(appearance::painted_rect)
        .map(|(x, y, w, h, _)| Rect::new(x + left, y, w, h))
        .collect()
}

/// **The pane shows tools its parts**: a dialog named for what it shows,
/// its header's links, the quick settings -- a switch each and the two
/// levels -- and the notifications, newest first, each with its cross.
#[test]
fn the_pane_shows_tools_its_parts() {
    let pane = pane(3, 3);
    let root = tree(&pane);
    assert_eq!(
        (root.role, root.name.as_str()),
        (Role::Dialog, "Notifications")
    );
    assert_eq!(
        root.bounds,
        Rect::new(SCREEN_W - PANE_WIDTH, 0.0, PANE_WIDTH, SCREEN_H)
    );
    let names: Vec<(Role, &str)> = root
        .children
        .iter()
        .map(|n| (n.role, n.name.as_str()))
        .collect();
    assert_eq!(
        names,
        [
            (Role::Button, "Settings"),
            (Role::Button, "Clear all"),
            (Role::Group, "Quick Settings"),
            (Role::List, "Notifications"),
        ]
    );
    let block = &root.children[2];
    let switches: Vec<&str> = block
        .children
        .iter()
        .filter(|n| n.role == Role::CheckBox)
        .map(|n| n.name.as_str())
        .collect();
    let labels: Vec<&str> = QuickSetting::all().iter().map(|s| s.label()).collect();
    assert_eq!(switches, labels);
    let levels: Vec<&str> = block
        .children
        .iter()
        .filter(|n| n.role == Role::Slider)
        .map(|n| n.name.as_str())
        .collect();
    assert_eq!(levels, ["Volume", "Brightness"]);
    let list = &root.children[3];
    let titles: Vec<&str> = list.children.iter().map(|n| n.name.as_str()).collect();
    assert_eq!(titles, ["N2", "N1", "N0"], "newest first");
    for card in &list.children {
        assert_eq!(card.role, Role::ListItem);
        let PanePart::Card(id) = card.id else {
            panic!("{:?} is no card", card.id);
        };
        assert_eq!(
            card.children.iter().map(|n| n.id).collect::<Vec<_>>(),
            [PanePart::Dismiss(id)]
        );
    }
}

/// **A card says who it is from, when, whether it is unread and what it
/// says** -- and, with no title, is named by its program.
#[test]
fn a_card_says_who_when_and_what() {
    let mut pane = pane(0, 1);
    let id = pane.push_notification(notif("Mail", "Invoice", "Your invoice is ready"));
    let quiet = pane.push_notification(notif("Chat", "", ""));
    pane.mark_read(quiet);
    let card = node(&pane, PanePart::Card(id));
    assert_eq!(card.name, "Invoice");
    let said = card.description.unwrap();
    assert!(said.starts_with("Mail, "), "{said}");
    assert!(said.contains("unread"), "{said}");
    assert!(said.ends_with(". Your invoice is ready"), "{said}");
    let card = node(&pane, PanePart::Card(quiet));
    assert_eq!(card.name, "Chat", "no title: its program");
    let said = card.description.unwrap();
    assert!(!said.contains("unread") && !said.contains(". "), "{said}");
}

/// **Each card's box, and each switch's, is where the pane paints it**:
/// a card's background and a switch's pill are among what it draws, at
/// exactly the node's box.
#[test]
fn a_cards_and_a_switchs_box_is_where_it_is_painted() {
    let pane = pane(3, 3);
    let drawn = painted(&pane);
    let root = tree(&pane);
    for node in root.walk() {
        if matches!(node.id, PanePart::Card(_) | PanePart::Switch(_)) {
            assert!(
                drawn.contains(&node.bounds),
                "{:?} at {:?} is not painted there",
                node.id,
                node.bounds
            );
        }
    }
}

/// **A press at the middle of a part's box presses that part**: each
/// switch flips itself, a card is opened, its cross dismisses it, "Clear
/// all" empties the list and "Settings" shows the programs.
#[test]
fn a_press_at_the_middle_of_a_parts_box_presses_it() {
    let mut pane = pane(3, 3);
    for &setting in QuickSetting::all() {
        if pane.unavailable(setting).is_some() {
            continue;
        }
        assert_eq!(
            press(&mut pane, PanePart::Switch(setting)),
            [NotifPaneEvent::QuickSettingToggled(setting)]
        );
    }
    let ids: Vec<u64> = pane.notifications().iter().map(|n| n.id).collect();
    assert_eq!(
        press(&mut pane, PanePart::Card(ids[1])),
        [NotifPaneEvent::NotificationClicked(ids[1])]
    );
    assert_eq!(
        press(&mut pane, PanePart::Dismiss(ids[0])),
        [NotifPaneEvent::NotificationDismissed(ids[0])]
    );
    assert_eq!(
        press(&mut pane, PanePart::ClearAll),
        [NotifPaneEvent::ClearAll]
    );
    assert!(pane.notifications().is_empty());
    assert!(press(&mut pane, PanePart::Settings).is_empty());
    assert!(pane.show_settings);
}

/// **A switch says whether it is on, and one that cannot be used says why
/// and is out of use**; a level is a slider from 0 to 100, and one the
/// pane cannot change says why -- with its level where the pane shows one.
#[test]
fn switches_and_levels_say_what_they_hold() {
    let mut pane = pane(0, 1);
    pane.set_quick_setting(QuickSetting::DoNotDisturb, true);
    pane.set_unavailable(QuickSetting::WiFi, Some("No radio"));
    let dnd = node(&pane, PanePart::Switch(QuickSetting::DoNotDisturb));
    assert_eq!(dnd.value, Some(Value::Check(CheckState::Checked)));
    assert!(dnd.enabled);
    let wifi = node(&pane, PanePart::Switch(QuickSetting::WiFi));
    assert!(!wifi.enabled);
    assert_eq!(wifi.description.as_deref(), Some("No radio"));

    let range = |value: f64| {
        Some(Value::Range {
            value,
            min: 0.0,
            max: 100.0,
        })
    };
    pane.set_volume(40);
    assert_eq!(node(&pane, PanePart::Volume).value, range(40.0));
    pane.set_volume_out_of_reach(Some("No sound card"));
    let volume = node(&pane, PanePart::Volume);
    assert_eq!(volume.value, None, "no level is shown");
    assert!(!volume.enabled);
    assert_eq!(volume.description.as_deref(), Some("No sound card"));

    pane.show_fixed_brightness(Some(30), "Can't be changed yet");
    let brightness = node(&pane, PanePart::Brightness);
    assert_eq!(brightness.value, range(30.0), "the level shown beside it");
    assert!(!brightness.enabled);
    pane.set_brightness(70);
    assert_eq!(pane.brightness(), 30, "a fixed level is not set");
    pane.show_screen_brightness(30);
    pane.set_brightness(170);
    assert_eq!(pane.brightness(), 100, "held to its range");
}

/// **A card scrolled out of the list has no press point until it is
/// revealed**, which scrolls no further than it takes; then a press opens
/// it, and its cross dismisses it.
#[test]
fn a_card_out_of_sight_is_scrolled_into_the_list_first() {
    let mut pane = pane(40, 1);
    let last = pane.notifications().last().unwrap().id;
    assert_eq!(
        pane.press_point(PanePart::Card(last), SCREEN_W, SCREEN_H),
        None
    );
    let card = node(&pane, PanePart::Card(last));
    assert!(card.bounds.y > SCREEN_H, "its box is below the list");

    pane.reveal(PanePart::Card(last), SCREEN_H);
    assert_eq!(pane.scroll_offset, pane.max_scroll(), "scrolled to the end");
    let card = node(&pane, PanePart::Card(last));
    assert!(card.bounds.bottom() <= SCREEN_H, "{:?}", card.bounds);
    assert_eq!(
        press(&mut pane, PanePart::Card(last)),
        [NotifPaneEvent::NotificationClicked(last)]
    );
    // One already in the list stays where it is.
    let before = pane.scroll_offset;
    pane.reveal(PanePart::Card(last), SCREEN_H);
    assert_eq!(pane.scroll_offset, before);

    // Back up to the first, its top at the list's top -- below its group's
    // heading, which it need not show.
    let first = pane.notifications()[0].id;
    pane.reveal(PanePart::Dismiss(first), SCREEN_H);
    assert_eq!(pane.scroll_offset, pane.card_tops()[0]);
    assert_eq!(
        press(&mut pane, PanePart::Dismiss(first)),
        [NotifPaneEvent::NotificationDismissed(first)]
    );
}

/// **The programs' view shows each program with its switch, and the way to
/// Settings**; a switch pressed turns its program off, and says so after.
#[test]
fn the_programs_show_their_switches_and_the_way_to_settings() {
    let mut pane = pane(3, 3);
    pane.show_programs(true);
    let root = tree(&pane);
    assert_eq!(root.name, "Notification Settings");
    let list = &root.children[2];
    assert_eq!((list.id, list.role), (PanePart::Programs, Role::List));
    let names: Vec<&str> = list.children.iter().map(|n| n.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "App0",
            "App1",
            "App2",
            "Open full notification settings\u{2026}"
        ]
    );
    let switch = node(&pane, PanePart::ProgramSwitch(0));
    assert_eq!(
        (switch.role, switch.name.as_str()),
        (Role::CheckBox, "Notifications from App0")
    );
    assert_eq!(switch.value, Some(Value::Check(CheckState::Checked)));
    assert_eq!(
        press(&mut pane, PanePart::ProgramSwitch(0)),
        [NotifPaneEvent::SettingChanged {
            app: "App0".to_owned(),
            setting: AppSettingKind::Enabled,
            value: SettingValue::Bool(false),
        }]
    );
    assert_eq!(
        node(&pane, PanePart::ProgramSwitch(0)).value,
        Some(Value::Check(CheckState::Unchecked))
    );
    assert_eq!(
        press(&mut pane, PanePart::FullSettings),
        [NotifPaneEvent::SettingsAsked]
    );
    assert!(press(&mut pane, PanePart::Back).is_empty());
    assert!(!pane.show_settings);
}

/// **A program past the pane's foot is revealed as a card is.**
#[test]
fn a_program_out_of_sight_is_scrolled_into_the_list_first() {
    let mut pane = pane(20, 20);
    pane.show_programs(true);
    let part = PanePart::ProgramSwitch(19);
    assert_eq!(pane.press_point(part, SCREEN_W, SCREEN_H), None);
    pane.reveal(part, SCREEN_H);
    let card = node(&pane, PanePart::Program(19));
    assert!(card.bounds.bottom() <= SCREEN_H, "{:?}", card.bounds);
    assert_eq!(card.bounds.h, APP_CARD_HEIGHT);
    assert!(matches!(
        press(&mut pane, part).as_slice(),
        [NotifPaneEvent::SettingChanged { app, .. }] if app == "App19"
    ));
}

/// **A card already in the list is not scrolled for**: revealing it leaves
/// the list where it is, wherever that is.
#[test]
fn a_card_in_sight_is_left_where_it_is() {
    let mut pane = pane(40, 1);
    pane.handle_mouse_event(
        &MouseEvent {
            x: SCREEN_W - 100.0,
            y: 500.0,
            kind: MouseEventKind::Scroll { dx: 0.0, dy: -3.0 },
        },
        SCREEN_W,
        SCREEN_H,
    );
    let scrolled = pane.scroll_offset;
    assert!(scrolled > 0.0, "the fixture scrolls");
    let list_top = NotificationPane::list_start_y();
    let shown = pane
        .notifications()
        .iter()
        .map(|n| n.id)
        .find(|&id| {
            let card = node(&pane, PanePart::Card(id)).bounds;
            card.y > list_top + 10.0 && card.bottom() < SCREEN_H - 10.0
        })
        .expect("a card wholly in the list");
    pane.reveal(PanePart::Card(shown), SCREEN_H);
    assert_eq!(pane.scroll_offset, scrolled);
}

/// **A sliding pane's parts are where it is drawn**, part of the way in:
/// a press at a part's middle still presses it.
#[test]
fn a_sliding_panes_parts_are_where_it_is_drawn() {
    let mut pane = pane(2, 2);
    pane.state = PaneState::SlideIn(0.5);
    let shown = pane.shown();
    assert!(shown > 0.0 && shown < 1.0, "half way: {shown}");
    let root = tree(&pane);
    assert_eq!(root.bounds.x, SCREEN_W - PANE_WIDTH * shown);
    assert!(root.bounds.x > SCREEN_W - PANE_WIDTH);
    assert_eq!(
        press(&mut pane, PanePart::ClearAll),
        [NotifPaneEvent::ClearAll]
    );
}

/// **What the pane does not show has no press point**: a link of the
/// other view, a notification that is not there, a program past the end,
/// any part of a closed pane.
#[test]
fn what_the_pane_does_not_show_has_no_press_point() {
    let mut pane = pane(2, 2);
    for part in [
        PanePart::Back,
        PanePart::Card(999),
        PanePart::Dismiss(999),
        PanePart::Program(0),
        PanePart::FullSettings,
    ] {
        assert_eq!(pane.press_point(part, SCREEN_W, SCREEN_H), None, "{part:?}");
    }
    pane.show_programs(true);
    for part in [PanePart::Settings, PanePart::ClearAll, PanePart::Program(2)] {
        assert_eq!(pane.press_point(part, SCREEN_W, SCREEN_H), None, "{part:?}");
    }
    pane.hide();
    assert_eq!(pane.press_point(PanePart::Back, SCREEN_W, SCREEN_H), None);
}
