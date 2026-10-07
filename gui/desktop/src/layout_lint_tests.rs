//! A check over the whole shell as tools see it: in each state a user can put
//! it in -- each surface open, with windows, notifications, widgets and icons
//! -- no two controls side by side in the tree are drawn over each other.
//!
//! Two controls a press can land on at once are a press that reaches the
//! wrong one, or neither, and a reader of the screen sees one drawn over the
//! other. The automation tree places every part where it is drawn and
//! pressed, so it is where such an overlap shows, for every surface at once:
//! writing the on-screen display's nodes found two overlays drawn over each
//! other (2026-10-07), and this asks the same of everything.
//!
//! Only siblings are compared -- a card and its own cross are meant to
//! overlap -- and only parts a press does something to: a heading drawn
//! over a list is not this check's business.

#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use guitk::widget::automation::{Accessible, Node, Role};

use crate::accessible::ShellPart;
use crate::{DesktopShell, WindowInfo, WindowList};

/// Whether a press on a part of `role` does something: what this check is
/// about.
const fn pressed(role: Role) -> bool {
    matches!(
        role,
        Role::Button
            | Role::CheckBox
            | Role::RadioButton
            | Role::TextField
            | Role::TextArea
            | Role::Slider
            | Role::ListItem
            | Role::MenuItem
            | Role::Tab
            | Role::TreeItem
            | Role::ComboBox
            | Role::Link
            | Role::GridCell
            // Read rather than pressed, and drawn as much: two overlays
            // of the on-screen display drawn over each other were the
            // overlap that started this.
            | Role::Label
            | Role::ProgressBar
            | Role::Image
    )
}

/// Every pair of siblings a press does something to whose boxes overlap by
/// more than half a pixel each way, said as "a over b".
fn overlaps(root: &Node<ShellPart>) -> Vec<String> {
    let mut found = Vec::new();
    for parent in root.walk() {
        let kids: Vec<&Node<ShellPart>> = parent
            .children
            .iter()
            .filter(|n| pressed(n.role) && n.shown && n.bounds.w > 0.0 && n.bounds.h > 0.0)
            .collect();
        for (i, a) in kids.iter().enumerate() {
            for b in kids.iter().skip(i + 1) {
                if let Some(both) = a.bounds.intersect(b.bounds)
                    && both.w > 0.5
                    && both.h > 0.5
                {
                    found.push(format!(
                        "{:?} \"{}\" {:?} over {:?} \"{}\" {:?}",
                        a.id, a.name, a.bounds, b.id, b.name, b.bounds
                    ));
                }
            }
        }
    }
    found
}

/// Assert no two controls of `shell`'s are drawn over each other, saying
/// which state it is in.
fn check(shell: &DesktopShell, what: &str, found: &mut Vec<String>) {
    for overlap in overlaps(&shell.automation(0.0, 0.0)) {
        found.push(format!("{what}: {overlap}"));
    }
}

/// A notification from `app`, titled `title`.
fn notif(app: &str, title: &str, at: u64) -> crate::notif_pane::Notification {
    crate::notif_pane::Notification {
        id: 0,
        app_name: app.to_owned(),
        title: title.to_owned(),
        body: "Something happened".to_owned(),
        timestamp: at,
        priority: crate::notif_pane::NotifPriority::Normal,
        read: false,
        action: None,
        silent: false,
    }
}

/// A desktop as a user has it: icons, windows open, notifications kept.
fn lived_in(width: u32, height: u32) -> DesktopShell {
    let mut shell = DesktopShell::new(width, height);
    shell.icons.populate_defaults();
    let _asked = shell.apply_window_list(&WindowList::new(
        0,
        vec![
            WindowInfo::new(1, 40, "notes.md - Editor"),
            WindowInfo::new(2, 41, "Inbox - Mail"),
            WindowInfo::new(3, 42, "Calendar"),
            WindowInfo::new(
                4,
                43,
                "A window with a title far too long to fit on any button",
            ),
        ],
    ));
    for i in 0..5 {
        let _ = shell.notify(notif("Mail", &format!("Message {i}"), 1_000 + i));
    }
    let _ = shell.notify(notif("Chat", "Lunch?", 2_000));
    // The pop-ups slid in and settled: arriving together, they start from
    // one place and spread over a fifth of a second, which is motion, not
    // layout.
    shell.advance_toasts(1_000);
    shell
}

/// **No two controls of the shell are drawn over each other**, in each
/// state a user can put it in, on a large display and a small one.
#[test]
fn no_two_controls_of_the_shell_are_drawn_over_each_other() {
    let mut found = Vec::new();
    for (width, height) in [(1920, 1080), (1024, 768), (800, 600)] {
        let size = format!("{width}x{height}");
        let mut shell = lived_in(width, height);
        check(&shell, &format!("{size} desktop"), &mut found);

        // The on-screen display with a tall overlay and short ones after it:
        // placed each by its own height, the short reached back over the tall.
        shell.osd.show(
            crate::osd::OsdKind::MediaTrack {
                title: "Song".to_owned(),
                artist: "Band".to_owned(),
                album: "Album".to_owned(),
            },
            0,
        );
        shell.osd.show(
            crate::osd::OsdKind::KeyboardLock {
                lock_type: crate::osd::LockType::CapsLock,
                active: true,
            },
            0,
        );
        shell.osd.show(
            crate::osd::OsdKind::Volume {
                level: 40,
                muted: false,
            },
            0,
        );
        check(&shell, &format!("{size} on-screen display"), &mut found);
        shell.osd.dismiss_all(0);
        shell.osd.tick(10_000);

        shell.toggle_start_menu();
        check(&shell, &format!("{size} start menu"), &mut found);
        shell.toggle_power_menu();
        check(&shell, &format!("{size} power choices"), &mut found);
        let _ = shell.dismiss_popups();

        shell.toggle_calendar();
        check(&shell, &format!("{size} calendar"), &mut found);
        let _ = shell.dismiss_popups();

        shell.toggle_volume_flyout();
        check(&shell, &format!("{size} volume"), &mut found);
        let _ = shell.dismiss_popups();

        shell.toggle_notifications();
        check(&shell, &format!("{size} notification pane"), &mut found);
        shell
            .notifications
            .set_grouping(notifsettings::Grouping::ByTime);
        check(
            &shell,
            &format!("{size} notification pane by time"),
            &mut found,
        );
        let _ = shell.dismiss_popups();

        #[allow(clippy::cast_precision_loss, reason = "a display's size")]
        let (mid_x, mid_y) = (width as f32 / 2.0, height as f32 / 2.0);
        shell.open_desktop_menu(mid_x, mid_y);
        check(&shell, &format!("{size} desktop menu"), &mut found);
        let _ = shell.dismiss_popups();

        shell.toggle_shortcut_card();
        check(&shell, &format!("{size} shortcut card"), &mut found);
        let _ = shell.dismiss_popups();

        shell.toggle_run_dialog();
        check(&shell, &format!("{size} run box"), &mut found);
        let _ = shell.dismiss_popups();

        shell.start_alt_tab();
        check(&shell, &format!("{size} window switcher"), &mut found);
        let _ = shell.finish_alt_tab();

        shell
            .overview
            .show(crate::overview::OverviewMode::AllWindows);
        check(&shell, &format!("{size} overview"), &mut found);
        let _ = shell.dismiss_popups();

        let _ = shell.toggle_zone_overlay();
        check(&shell, &format!("{size} tiling overlay"), &mut found);
        let _ = shell.dismiss_popups();

        // Widgets out on the desktop, added as the user adds them -- the
        // desktop menu's rows -- each where the grid has room.
        for row in [
            DesktopShell::MENU_ADD_CLOCK,
            DesktopShell::MENU_ADD_NOTE,
            DesktopShell::MENU_ADD_CALENDAR,
            DesktopShell::MENU_ADD_SYSTEM_MONITOR,
        ] {
            shell.open_desktop_menu(mid_x, mid_y);
            let _ = shell.activate_desktop_menu_item(row);
            let _ = shell.dismiss_popups();
        }
        assert!(
            shell.widgets.all_widgets().len() >= 3,
            "{size}: the widgets were not put out, so this checks nothing"
        );
        check(&shell, &format!("{size} widgets"), &mut found);

        // A crowded bar: a dozen windows, two with very long titles.
        let crowd: Vec<WindowInfo> = (1..=12)
            .map(|i| {
                let title = if i % 6 == 0 {
                    format!("Window {i} with a title long enough to fill the bar alone")
                } else {
                    format!("Window {i}")
                };
                WindowInfo::new(i, 100 + i, &title)
            })
            .collect();
        let _asked = shell.apply_window_list(&WindowList::new(0, crowd));
        let tiles = shell
            .automation(0.0, 0.0)
            .walk()
            .filter(|n| matches!(n.id, ShellPart::Control(crate::Hit::TaskbarButton(_))))
            .count();
        assert!(
            tiles >= 6,
            "{size}: {tiles} of a dozen windows on the bar, so this checks little"
        );
        check(&shell, &format!("{size} a dozen windows"), &mut found);
    }
    assert!(
        found.is_empty(),
        "{} overlaps:\n{}",
        found.len(),
        found.join("\n")
    );
}
