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

use guitk::menu::MenuPart;
use guitk::widget::automation::{Accessible, Action};

use crate::accessible::{ShellMenu, ShellPart};
use crate::notif_pane::PanePart;
use crate::overview::OverviewMode;
use crate::widgets::{WidgetKind, WidgetSize};
use crate::{DesktopShell, Hit, MouseButton, MouseEvent, MouseEventKind, WindowInfo, WindowList};

/// Whether `shell` has `menu` open -- and, asked for `submenu`, one of its
/// submenus too: so a state the check is asked about is one it reached,
/// rather than the desktop it was meant to be over.
fn has_menu(shell: &DesktopShell, menu: ShellMenu, submenu: bool) -> bool {
    shell.automation(0.0, 0.0).walk().any(|n| match n.id {
        ShellPart::Menu(open, part) => {
            open == menu && (!submenu || matches!(part, MenuPart::Submenu(_)))
        }
        _ => false,
    })
}

/// Note every two parts of `shell`'s drawn over each other -- the toolkit's
/// rule (`Node::overlapping_siblings`): siblings a person presses or reads
/// -- saying which state it is in.
fn check(shell: &DesktopShell, what: &str, found: &mut Vec<String>) {
    let tree = shell.automation(0.0, 0.0);
    for (a, b) in tree.overlapping_siblings() {
        found.push(format!(
            "{what}: {:?} \"{}\" {:?} over {:?} \"{}\" {:?}",
            a.id, a.name, a.bounds, b.id, b.name, b.bounds
        ));
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
    let mut roomy = 0;
    let mut taskbar_menus = 0;
    for (width, height) in [(1920, 1080), (1024, 768), (800, 600)] {
        let size = format!("{width}x{height}");
        let mut shell = lived_in(width, height);
        check(&shell, &format!("{size} desktop"), &mut found);

        // A notification's menu, its pop-up right-clicked.
        let toast = shell
            .toasts
            .placed()
            .first()
            .map(|placed| placed.rect)
            .expect("a pop-up shown");
        let _ = shell.handle_toast_mouse(&MouseEvent {
            x: toast.x + 40.0,
            y: toast.y + toast.h / 2.0,
            kind: MouseEventKind::Press(MouseButton::Right),
        });
        assert!(
            has_menu(&shell, ShellMenu::Notification, false),
            "{size}: no notification's menu"
        );
        check(&shell, &format!("{size} a notification's menu"), &mut found);
        let _ = shell.dismiss_popups();

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

        // A window's menu, its tile on the bar right-clicked; and the bar's
        // own, the bar between its tiles and its tray right-clicked.
        let pins = shell.taskbar.pinned_apps().len();
        let tiles = shell.taskbar_layout();
        let tile = tiles[pins];
        let _ = shell.handle_press(
            tile.x + tile.w / 2.0,
            tile.y + tile.h / 2.0,
            MouseButton::Right,
        );
        assert!(
            shell.render_pin_menu().is_some(),
            "{size}: no window's menu, so this checks nothing"
        );
        check(&shell, &format!("{size} a window's menu"), &mut found);
        let _ = shell.dismiss_popups();
        let bar = shell.taskbar_rect();
        let (bx, by) = (
            tiles.last().map_or(bar.x, |t| t.right()) + 24.0,
            bar.y + bar.h / 2.0,
        );
        if shell.hit_test(bx, by) == Hit::TaskbarPanel {
            let _ = shell.handle_press(bx, by, MouseButton::Right);
            assert!(
                has_menu(&shell, ShellMenu::Taskbar, false),
                "{size}: no taskbar menu"
            );
            taskbar_menus += 1;
            check(&shell, &format!("{size} the taskbar's menu"), &mut found);
            let _ = shell.dismiss_popups();
        }

        // The start menu searching, and a program's menu in it.
        shell.toggle_start_menu();
        shell
            .invoke(
                &ShellPart::StartSearch,
                Action::SetText("e".to_owned()),
                0.0,
                0.0,
            )
            .expect("the search typed into");
        check(&shell, &format!("{size} start menu searching"), &mut found);
        let search = shell.start_search_rect();
        let _ = shell.handle_press(
            search.x + 10.0,
            search.y + search.h / 2.0,
            MouseButton::Right,
        );
        assert!(
            has_menu(&shell, ShellMenu::Field, false),
            "{size}: no search field's menu"
        );
        check(
            &shell,
            &format!("{size} the search field's menu"),
            &mut found,
        );
        let _ = shell.dismiss_popups();
        shell.toggle_start_menu();
        let row = shell
            .start_row_of_program(0)
            .expect("a program in the start menu");
        let r = shell.start_menu_row_rect(row);
        let _ = shell.handle_press(r.x + r.w / 2.0, r.y + r.h / 2.0, MouseButton::Right);
        assert!(
            shell.render_pin_menu().is_some(),
            "{size}: no program's menu"
        );
        check(&shell, &format!("{size} a program's menu"), &mut found);
        let _ = shell.dismiss_popups();

        // An icon's menu.
        let icon = shell.icons.icon_ids().first().copied().expect("icons out");
        let placed = shell.icons.get_icon(icon).expect("the icon is out");
        #[allow(clippy::cast_precision_loss, reason = "a place on the screen")]
        let (ix, iy) = (placed.x as f32 + 10.0, placed.y as f32 + 10.0);
        shell.open_desktop_menu(ix, iy);
        assert!(
            has_menu(&shell, ShellMenu::Desktop, false),
            "{size}: no icon's menu"
        );
        check(&shell, &format!("{size} an icon's menu"), &mut found);
        let _ = shell.dismiss_popups();

        // A program's notifications all shown: Mail's, past its three --
        // grouped by program again, as the pane above was left by time.
        shell
            .notifications
            .set_grouping(notifsettings::Grouping::ByProgram);
        shell.toggle_notifications();
        let more = shell
            .automation(0.0, 0.0)
            .walk()
            .find(|n| matches!(n.id, ShellPart::Pane(PanePart::GroupMore(_))))
            .map(|n| n.id)
            .expect("Mail's rest behind \"Show N more\"");
        shell
            .invoke(&more, Action::Press, 0.0, 0.0)
            .expect("shown as pressed");
        check(
            &shell,
            &format!("{size} a program's notifications all shown"),
            &mut found,
        );
        let _ = shell.dismiss_popups();

        // Every desktop's windows, in the overview.
        shell.overview.show(OverviewMode::AllDesktops);
        check(&shell, &format!("{size} every desktop"), &mut found);
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

        // A widget's menu, and the sizes it offers.
        let clock = shell
            .widgets
            .all_widgets()
            .iter()
            .find(|w| w.kind == WidgetKind::Clock)
            .map(|w| w.id)
            .expect("a clock out");
        let (cx, cy, _, _) = shell.widgets.content_rect(clock).expect("placed");
        shell.open_desktop_menu(cx + 4.0, cy + 4.0);
        assert!(
            has_menu(&shell, ShellMenu::Desktop, false),
            "{size}: no widget's menu"
        );
        check(&shell, &format!("{size} a widget's menu"), &mut found);
        // "Size" pressed, as a click on its row opens it.
        shell
            .invoke(
                &ShellPart::Menu(
                    ShellMenu::Desktop,
                    MenuPart::Item(DesktopShell::MENU_WIDGET_SIZE_SUBMENU),
                ),
                Action::Press,
                0.0,
                0.0,
            )
            .expect("the sizes opened");
        assert!(
            has_menu(&shell, ShellMenu::Desktop, true),
            "{size}: no widget's sizes"
        );
        check(&shell, &format!("{size} a widget's sizes"), &mut found);
        let _ = shell.dismiss_popups();

        // The system monitor given two rows, where the grid has them: its
        // graphs of the last minute, and the disk's meter at their foot.
        let monitor = shell
            .widgets
            .all_widgets()
            .iter()
            .find(|w| w.kind == WidgetKind::SystemMonitor)
            .map(|w| w.id);
        if let Some(id) = monitor
            && shell.widgets.resize_widget(id, WidgetSize::LARGE)
        {
            roomy += 1;
            check(&shell, &format!("{size} roomy monitor"), &mut found);
        }

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
        roomy > 0,
        "no display had room for a monitor's graphs, so they were never checked"
    );
    assert!(
        taskbar_menus > 0,
        "no display had a free stretch of taskbar to right-click, so its menu was never checked"
    );
    assert!(
        found.is_empty(),
        "{} overlaps:\n{}",
        found.len(),
        found.join("\n")
    );
}
