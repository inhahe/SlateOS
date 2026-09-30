//! Tests for a notification's own menu: a right-click on its card in the
//! pane, or on its toast, offers turning its program's notifications off --
//! `design.txt`'s "option for any notification to not show notifications
//! from that application again" -- and the Settings app's Notifications page.
//!
//! `notif_pane`'s and `toasts`' tests hold that a right-click *asks* for the
//! menu, where each is drawn; these hold the shell's part: where the menu
//! opens, what its rows do, what they leave open, and that every way it
//! closes lets the toasts go on.

#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_precision_loss
)]

use crate::{DesktopShell, ShellAction, launcher, notif_pane};
use guitk::event::{Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind};
use guitk::menu::MenuItem;
use guitk::render::RenderCommand;
use notifsettings::Importance;

/// A notification from `app`, titled `title` so its card can be found.
fn notif(id: u64, app: &str, title: &str) -> notif_pane::Notification {
    notif_pane::Notification {
        id,
        app_name: app.to_owned(),
        title: title.to_owned(),
        body: String::new(),
        timestamp: 0,
        priority: notif_pane::NotifPriority::Normal,
        read: false,
        action: None,
        silent: false,
    }
}

fn at(x: f32, y: f32, kind: MouseEventKind) -> MouseEvent {
    MouseEvent { x, y, kind }
}

fn key(k: Key) -> KeyEvent {
    KeyEvent {
        key: k,
        pressed: true,
        modifiers: Modifiers::NONE,
        text: String::new(),
    }
}

/// The labels of the notification menu's rows, `None` when it is closed.
fn rows(shell: &DesktopShell) -> Option<Vec<String>> {
    let (menu, _) = shell.notification_menu.as_ref()?;
    Some(
        menu.items()
            .iter()
            .filter_map(|item| match item {
                MenuItem::Action { label, .. } => Some(label.clone()),
                _ => None,
            })
            .collect(),
    )
}

/// The middle of the notification menu's row `index`, on the screen.
fn row_centre(shell: &DesktopShell, index: usize) -> (f32, f32) {
    let (menu, _) = shell.notification_menu.as_ref().expect("the menu is open");
    let r = menu.item_rect(index).expect("the row is shown");
    (r.x + r.w / 2.0, r.y + r.h / 2.0)
}

/// Where the card titled `title` is drawn in the open pane: a point on it.
///
/// Read from what the pane draws, so the press lands where the user sees
/// the card. The pane is drawn under a translate in x only, so the title's
/// `y` is on the screen; any `x` in the pane's column is on the card.
fn card_point(shell: &DesktopShell, title: &str) -> (f32, f32) {
    let tree = shell.render_notifications().expect("the pane is open");
    let y = tree
        .commands
        .iter()
        .find_map(|c| match c {
            RenderCommand::Text { text, y, .. } if text == title => Some(*y),
            _ => None,
        })
        .expect("the card's title is drawn");
    (shell.screen_width as f32 - 60.0, y + 2.0)
}

/// A shell with `notifs` filed and their toasts in place.
fn with_toasts(notifs: &[notif_pane::Notification]) -> DesktopShell {
    let mut s = DesktopShell::new(1920, 1080);
    for n in notifs {
        s.notify(n.clone());
    }
    s.advance_toasts(1_000);
    s
}

/// Right-click the toast of notification `id`, answering the shell's
/// action.
fn right_click_toast(s: &mut DesktopShell, id: u64) -> ShellAction {
    let placed = s.toasts.placed();
    let toast = placed
        .iter()
        .find(|p| p.id == id)
        .expect("the toast is shown");
    let (x, y) = (toast.rect.x + 40.0, toast.rect.y + toast.rect.h / 2.0);
    s.handle_toast_mouse(&at(x, y, MouseEventKind::Press(MouseButton::Right)))
}

/// **A right-click on a card offers its program's menu, over the pane,
/// which stays open**; the card is not marked read.
#[test]
fn a_right_click_on_a_card_offers_its_programs_menu_over_the_pane() {
    appearance::config::testing::with_scratch_config("notif-menu-card", |_root| {
        let mut s = DesktopShell::new(1920, 1080);
        s.notify(notif(1, "Chat", "Lunch?"));
        s.notify(notif(2, "Mail", "Invoice"));
        s.toggle_notifications();
        let (x, y) = card_point(&s, "Invoice");

        let action = s.handle_mouse(&at(x, y, MouseEventKind::Press(MouseButton::Right)));
        assert_eq!(action, ShellAction::Consumed);
        assert_eq!(
            rows(&s),
            Some(vec![
                "Turn off notifications from Mail".to_owned(),
                "Notification settings".to_owned()
            ])
        );
        assert!(s.notifications.pane_state().is_visible(), "the pane closed");
        assert!(
            s.notifications.notifications().iter().all(|n| !n.read),
            "a right-click read a card"
        );
        // Drawn with the rest of the popups, and at the press.
        let panel = s
            .notification_menu
            .as_ref()
            .unwrap()
            .0
            .panel_rect()
            .unwrap();
        assert!(s.render_notification_menu().is_some());
        assert!(s.any_popup_open());
        assert!(
            (panel.y - y).abs() < 0.5 || (panel.y + panel.h - y).abs() < 0.5,
            "{panel:?} did not open at the press"
        );
    });
}

/// **Turning a program off from its card's menu silences it, saves it, and
/// leaves the pane open with the card still there** -- the place it is
/// turned back on from.
#[test]
fn turning_a_program_off_from_a_card_silences_it_and_leaves_the_pane() {
    appearance::config::testing::with_scratch_config("notif-menu-off", |_root| {
        let mut s = DesktopShell::new(1920, 1080);
        s.notify(notif(1, "Chat", "Lunch?"));
        s.toggle_notifications();
        let (x, y) = card_point(&s, "Lunch?");
        s.handle_mouse(&at(x, y, MouseEventKind::Press(MouseButton::Right)));
        let (rx, ry) = row_centre(&s, 0);

        assert_eq!(
            s.handle_mouse(&at(rx, ry, MouseEventKind::Press(MouseButton::Left))),
            ShellAction::Consumed
        );
        assert!(rows(&s).is_none(), "the menu stayed open");
        assert!(!s.focus.should_show_notification("Chat"));
        assert_eq!(
            notifsettings::NotifFile::load()
                .settings
                .rule_for("Chat")
                .importance,
            Importance::Silent,
            "turned off in memory and never written"
        );
        assert!(s.notifications.pane_state().is_visible(), "the pane closed");
        assert_eq!(s.notifications.notifications().len(), 1, "the card went");

        // Asked again, the same card offers turning it back on -- and does.
        let (x, y) = card_point(&s, "Lunch?");
        s.handle_mouse(&at(x, y, MouseEventKind::Press(MouseButton::Right)));
        assert_eq!(
            rows(&s).unwrap()[0],
            "Turn on notifications from Chat",
            "a silenced program is offered being silenced"
        );
        let (rx, ry) = row_centre(&s, 0);
        s.handle_mouse(&at(rx, ry, MouseEventKind::Press(MouseButton::Left)));
        assert!(s.focus.should_show_notification("Chat"));
        assert_eq!(
            notifsettings::NotifFile::load()
                .settings
                .rule_for("Chat")
                .importance,
            Importance::Normal
        );
    });
}

/// **"Notification settings" starts Settings on its Notifications page and
/// closes the pane**, whose scrim would dim the window it opens.
#[test]
fn notification_settings_opens_the_page_and_closes_the_pane() {
    appearance::config::testing::with_scratch_config("notif-menu-settings", |_root| {
        let mut s = DesktopShell::new(1920, 1080);
        s.notify(notif(1, "Chat", "Lunch?"));
        s.toggle_notifications();
        let (x, y) = card_point(&s, "Lunch?");
        s.handle_mouse(&at(x, y, MouseEventKind::Press(MouseButton::Right)));
        let (rx, ry) = row_centre(&s, 1);

        let action = s.handle_mouse(&at(rx, ry, MouseEventKind::Press(MouseButton::Left)));
        assert_eq!(
            action,
            ShellAction::Launch(launcher::settings_page(launcher::NOTIFICATIONS_PAGE))
        );
        assert!(
            !s.notifications.pane_state().is_visible(),
            "the pane stayed"
        );
        assert!(rows(&s).is_none());
        assert!(
            s.focus.should_show_notification("Chat"),
            "opening Settings changed a rule"
        );
    });
}

/// **Escape closes the menu and leaves the pane**, as a press outside the
/// menu does -- and that press is spent closing it, not on the card under
/// it.
#[test]
fn escape_or_a_press_elsewhere_closes_only_the_menu() {
    appearance::config::testing::with_scratch_config("notif-menu-escape", |_root| {
        let mut s = DesktopShell::new(1920, 1080);
        s.notify(notif(1, "Chat", "Lunch?"));
        s.toggle_notifications();
        let (x, y) = card_point(&s, "Lunch?");

        s.handle_mouse(&at(x, y, MouseEventKind::Press(MouseButton::Right)));
        assert!(rows(&s).is_some());
        drop(s.handle_hotkey(&key(Key::Escape)));
        assert!(rows(&s).is_none(), "Escape left the menu");
        assert!(
            s.notifications.pane_state().is_visible(),
            "Escape closed the pane too"
        );

        s.handle_mouse(&at(x, y, MouseEventKind::Press(MouseButton::Right)));
        let panel = s
            .notification_menu
            .as_ref()
            .unwrap()
            .0
            .panel_rect()
            .unwrap();
        // On the card, beside the menu: opened near the screen's right edge,
        // it flips to end at the press, leaving the card to its right.
        let (ox, oy) = (x + 20.0, y);
        assert!(
            !panel.contains(ox, oy),
            "the probe is on the menu: {panel:?}"
        );
        s.handle_mouse(&at(ox, oy, MouseEventKind::Press(MouseButton::Left)));
        assert!(rows(&s).is_none(), "a press elsewhere left the menu");
        assert!(s.notifications.pane_state().is_visible());
        assert!(
            s.notifications.notifications().iter().all(|n| !n.read),
            "the press that closed the menu also opened a card"
        );
    });
}

/// **The rows can be chosen from the keyboard**: Down to the first, Enter.
#[test]
fn the_rows_are_chosen_from_the_keyboard_too() {
    appearance::config::testing::with_scratch_config("notif-menu-keys", |_root| {
        let mut s = DesktopShell::new(1920, 1080);
        s.notify(notif(1, "Chat", "Lunch?"));
        s.toggle_notifications();
        let (x, y) = card_point(&s, "Lunch?");
        s.handle_mouse(&at(x, y, MouseEventKind::Press(MouseButton::Right)));
        drop(s.handle_hotkey(&key(Key::Down)));
        drop(s.handle_hotkey(&key(Key::Enter)));
        assert!(rows(&s).is_none());
        assert!(!s.focus.should_show_notification("Chat"));

        s.handle_mouse(&at(x, y, MouseEventKind::Press(MouseButton::Right)));
        drop(s.handle_hotkey(&key(Key::Down)));
        drop(s.handle_hotkey(&key(Key::Down)));
        let outcome = s.handle_hotkey(&key(Key::Enter));
        assert_eq!(
            outcome.launches,
            vec![launcher::settings_page(launcher::NOTIFICATIONS_PAGE)]
        );
    });
}

/// **A right-click on a toast opens its program's menu beside the stack,
/// never under it**, and holds the toasts while it is up.
#[test]
fn a_toasts_menu_opens_beside_the_stack_and_holds_it() {
    appearance::config::testing::with_scratch_config("notif-menu-toast", |_root| {
        let mut s = with_toasts(&[notif(1, "Chat", "Lunch?"), notif(2, "Mail", "Invoice")]);
        assert!(s.toasts.next_due_in().is_some());
        // Something else open: a toast's menu closes it, as every menu
        // opening does.
        s.toggle_start_menu();
        assert!(s.start_menu_open);

        assert_eq!(right_click_toast(&mut s, 2), ShellAction::Consumed);
        assert_eq!(rows(&s).unwrap()[0], "Turn off notifications from Mail");
        assert!(!s.start_menu_open, "the start menu stayed open under it");
        let stack = s.toast_extent().expect("the toasts are shown");
        let panel = s
            .notification_menu
            .as_ref()
            .unwrap()
            .0
            .panel_rect()
            .unwrap();
        assert!(
            panel.x + panel.w <= stack.x + 0.01,
            "{panel:?} is drawn under the toasts' surface {stack:?}"
        );
        assert!(
            (panel.x + panel.w - stack.x).abs() < 0.01,
            "not against the stack"
        );
        assert_eq!(s.toasts.next_due_in(), None, "the toasts were not held");
        s.advance_toasts(60_000);
        assert_eq!(s.toasts.ids(), [1, 2], "a toast left under its menu");
    });
}

/// **Turning a program off from its toast takes away its toasts and only
/// its**, and files them still: the pane keeps every card.
#[test]
fn turning_a_program_off_from_a_toast_takes_its_toasts() {
    appearance::config::testing::with_scratch_config("notif-menu-toast-off", |_root| {
        let mut s = with_toasts(&[
            notif(1, "Chat", "Lunch?"),
            notif(2, "Mail", "Invoice"),
            notif(3, "Chat", "Coffee?"),
        ]);
        right_click_toast(&mut s, 3);
        let (x, y) = row_centre(&s, 0);
        assert_eq!(
            s.handle_mouse(&at(x, y, MouseEventKind::Press(MouseButton::Left))),
            ShellAction::Consumed
        );
        s.advance_toasts(1_000);
        assert_eq!(s.toasts.ids(), [2], "Chat's toasts stayed, or Mail's went");
        assert_eq!(s.notifications.notifications().len(), 3, "a card went");
        assert!(!s.focus.should_show_notification("Chat"));

        // And the next one from Chat is filed without popping up.
        s.notify(notif(4, "Chat", "Tea?"));
        s.advance_toasts(1_000);
        assert_eq!(s.toasts.ids(), [2]);
        assert_eq!(s.notifications.notifications().len(), 4);
    });
}

/// **A press on a toast while its menu is up closes the menu and does
/// nothing else** -- the toast is neither opened nor closed by the press
/// that dismissed its menu.
#[test]
fn a_press_on_a_toast_while_its_menu_is_up_only_closes_the_menu() {
    appearance::config::testing::with_scratch_config("notif-menu-toast-press", |_root| {
        let mut s = with_toasts(&[notif(1, "Chat", "Lunch?")]);
        right_click_toast(&mut s, 1);
        let toast = s.toasts.placed()[0];
        let (x, y) = (toast.rect.x + 40.0, toast.rect.y + toast.rect.h / 2.0);
        assert_eq!(
            s.handle_toast_mouse(&at(x, y, MouseEventKind::Press(MouseButton::Left))),
            ShellAction::Consumed
        );
        assert!(rows(&s).is_none(), "the menu stayed open");
        assert!(!s.toasts.is_moving(), "the press also closed the toast");
        assert!(
            s.notifications.notifications().iter().all(|n| !n.read),
            "the press also opened the notification"
        );
        assert!(
            s.toasts.next_due_in().is_some(),
            "the toasts are still held"
        );
    });
}

/// **Every way the menu closes lets the toasts go on**: a row chosen, a
/// press elsewhere, Escape, and another popup opening. A way that forgot
/// would keep the toasts on screen until the next menu.
#[test]
fn every_way_a_toasts_menu_closes_lets_the_toasts_go() {
    appearance::config::testing::with_scratch_config("notif-menu-release", |_root| {
        type Close = fn(&mut DesktopShell);
        let ways: [(&str, Close); 5] = [
            ("a row chosen", |s| {
                let (x, y) = row_centre(s, 1);
                s.handle_mouse(&at(x, y, MouseEventKind::Press(MouseButton::Left)));
            }),
            ("a press elsewhere", |s| {
                s.handle_mouse(&at(20.0, 20.0, MouseEventKind::Press(MouseButton::Left)));
            }),
            ("Escape", |s| {
                drop(s.handle_hotkey(&key(Key::Escape)));
            }),
            ("the popups dismissed", |s| {
                s.dismiss_popups();
            }),
            ("another menu opened", |s| {
                s.open_desktop_menu(300.0, 300.0);
            }),
        ];
        for (way, close) in ways {
            let mut s = with_toasts(&[notif(1, "Chat", "Lunch?")]);
            right_click_toast(&mut s, 1);
            assert_eq!(
                s.toasts.next_due_in(),
                None,
                "{way}: not held to begin with"
            );
            close(&mut s);
            assert!(rows(&s).is_none(), "{way}: the menu stayed open");
            assert!(
                s.toasts.next_due_in().is_some(),
                "{way}: the toasts stayed held"
            );
        }
    });
}

/// **A right-click beside the toasts, or with none showing, opens nothing.**
#[test]
fn a_right_click_off_the_toasts_opens_nothing() {
    appearance::config::testing::with_scratch_config("notif-menu-miss", |_root| {
        let mut s = with_toasts(&[notif(1, "Chat", "Lunch?")]);
        let toast = s.toasts.placed()[0];
        s.handle_toast_mouse(&at(
            toast.rect.x - 5.0,
            toast.rect.y + 10.0,
            MouseEventKind::Press(MouseButton::Right),
        ));
        assert!(rows(&s).is_none());
        assert!(s.toasts.take_events().is_empty());
        assert!(s.toasts.next_due_in().is_some(), "a miss held the toasts");
    });
}
