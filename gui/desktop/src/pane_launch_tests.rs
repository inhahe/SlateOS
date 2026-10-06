//! What the notification pane starts, the shell starts -- and closes the
//! pane for: a notification that names a program, and "Open full
//! notification settings…" below the programs' switches. The window either
//! opens is what the user asked to see, and the pane's scrim would dim it.
//! A notification that names nothing is a message, read where it is, and
//! the pane stays.

#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::cast_precision_loss
)]

use std::path::PathBuf;

use guitk::event::{MouseButton, MouseEvent, MouseEventKind};
use guitk::render::RenderCommand;

use crate::{DesktopShell, ShellAction, hotkeys, launcher, notif_pane};

/// A notification from `app`, titled `title`, starting `action` if any.
fn notif(id: u64, app: &str, title: &str, action: Option<&str>) -> notif_pane::Notification {
    notif_pane::Notification {
        id,
        app_name: app.to_owned(),
        title: title.to_owned(),
        body: String::new(),
        timestamp: 0,
        priority: notif_pane::NotifPriority::Normal,
        read: false,
        action: action.map(str::to_owned),
        silent: false,
    }
}

/// Where `text` is drawn in the open pane, on the screen: the pane is drawn
/// under a translate in x only, so the text's `y` is the screen's, and its
/// `x` is the pane's own, from the pane's left edge.
fn drawn(shell: &DesktopShell, text: &str) -> (f32, f32) {
    let tree = shell.render_notifications().expect("the pane is open");
    let (x, y) = tree
        .commands
        .iter()
        .find_map(|c| match c {
            RenderCommand::Text { text: t, x, y, .. } if t == text => Some((*x, *y)),
            _ => None,
        })
        .unwrap_or_else(|| panic!("{text:?} is drawn"));
    let pane_left = shell.screen_width as f32 - 380.0;
    (pane_left + x, y)
}

/// A press and a release at `(x, y)`, answering what the press did.
fn click(shell: &mut DesktopShell, (x, y): (f32, f32)) -> ShellAction {
    let action = shell.handle_mouse(&MouseEvent {
        x: x + 2.0,
        y: y + 2.0,
        kind: MouseEventKind::Press(MouseButton::Left),
    });
    let _released = shell.handle_mouse(&MouseEvent {
        x: x + 2.0,
        y: y + 2.0,
        kind: MouseEventKind::Release(MouseButton::Left),
    });
    action
}

/// **A notification that names a program starts it and closes the pane.**
#[test]
fn a_notification_that_starts_a_program_closes_the_pane() {
    appearance::config::testing::with_scratch_config("pane-launch-card", |_root| {
        let mut s = DesktopShell::new(1920, 1080);
        s.notify(notif(1, "Mail", "Invoice", Some("/apps/mail")));
        s.toggle_notifications();
        let at = drawn(&s, "Invoice");

        assert_eq!(
            click(&mut s, at),
            ShellAction::Launch(hotkeys::Launch::program(PathBuf::from("/apps/mail")))
        );
        assert!(
            !s.notifications.pane_state().is_visible(),
            "the pane stayed over the program it started"
        );
        assert!(s.notifications.notifications()[0].read, "the card was read");
    });
}

/// **A notification that names nothing is read where it is, and the pane
/// stays** -- reading it is the whole of it.
#[test]
fn a_notification_that_starts_nothing_leaves_the_pane_open() {
    appearance::config::testing::with_scratch_config("pane-launch-message", |_root| {
        let mut s = DesktopShell::new(1920, 1080);
        s.notify(notif(1, "Mail", "Invoice", None));
        s.toggle_notifications();
        let at = drawn(&s, "Invoice");

        assert_eq!(click(&mut s, at), ShellAction::Consumed);
        assert!(s.notifications.pane_state().is_visible(), "the pane closed");
        assert!(
            s.notifications.notifications()[0].read,
            "the card was not read"
        );
    });
}

/// **"Open full notification settings…" starts Settings on its
/// notifications page and closes the pane** -- it was drawn as a link and
/// took no press at all.
#[test]
fn the_way_to_settings_opens_the_notifications_page_and_closes_the_pane() {
    appearance::config::testing::with_scratch_config("pane-launch-settings", |_root| {
        let mut s = DesktopShell::new(1920, 1080);
        s.notify(notif(1, "Mail", "Invoice", None));
        s.toggle_notifications();
        let settings = drawn(&s, "Settings");
        assert_eq!(click(&mut s, settings), ShellAction::Consumed);
        let link = drawn(&s, "Open full notification settings\u{2026}");

        assert_eq!(
            click(&mut s, link),
            ShellAction::Launch(launcher::settings_page(launcher::NOTIFICATIONS_PAGE))
        );
        assert!(
            !s.notifications.pane_state().is_visible(),
            "the pane stayed over Settings"
        );
    });
}
