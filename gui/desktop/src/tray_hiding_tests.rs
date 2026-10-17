//! Tests for moving a tray icon into and out of the overflow -- the
//! reference's "hidden icons" -- by dragging: `design.txt`'s "can drag and
//! drop icons into and out of the system tray".
//!
//! `tray_dnd`'s tests hold the arrangement's part (a hidden icon is in the
//! overflow, in its place); these hold the shell's: the chevron that comes
//! up to be dropped on, where an icon carried back is shown, that a click on
//! a row still clicks its program, the label that says what letting go
//! will do, and that where the user put an icon, and whether they hid it,
//! is still so when its program -- or the desktop -- starts again.

#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_precision_loss
)]

use crate::{DesktopShell, Rect, ShellAction, ShellRequest, tray_dnd};
use appearance::config::testing::with_scratch_config;
use guitk::event::{Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind};
use guitk::render::RenderCommand;

/// A shell whose tray holds icons `1..=n`, each its own program's. The
/// programs give no app id, so nothing about them is remembered and these
/// shells write no settings.
fn shell_with(n: u32) -> DesktopShell {
    let mut s = DesktopShell::new(1920, 1080);
    s.apply_tray_icons(
        (1..=n)
            .map(|i| guiremote::tray::TrayIcon::new(u64::from(i), i, "X", format!("program {i}")))
            .collect(),
    );
    s
}

/// Icons `1..=n` from programs that name themselves -- `org.example.p<i>`,
/// with a `#` in one of the names, which the settings file must keep apart
/// from the icon's id -- each running as process `pid_base + i`.
fn named_icons(n: u32, pid_base: u64) -> Vec<guiremote::tray::TrayIcon> {
    (1..=n)
        .map(|i| {
            let app = if i == 2 {
                "org.example.p#2".to_owned()
            } else {
                format!("org.example.p{i}")
            };
            guiremote::tray::TrayIcon::new(pid_base + u64::from(i), i, "X", format!("program {i}"))
                .with_app_id(app)
        })
        .collect()
}

/// A desktop as a start leaves it: its settings read, then the programs'
/// icons arriving.
fn started_with(icons: Vec<guiremote::tray::TrayIcon>) -> DesktopShell {
    let mut s = DesktopShell::new(1920, 1080);
    s.load_tray();
    s.apply_tray_icons(icons);
    s
}

/// Carry the drawn icon at `from` onto the chevron and let go.
fn drop_on_chevron(s: &mut DesktopShell, from: usize) {
    let at = centre(s.tray_icon_rects()[from]);
    press(s, at);
    move_to(s, (at.0, at.1 - 20.0));
    let chevron = s.tray_overflow_rect().expect("the chevron comes up");
    move_to(s, centre(chevron));
    release(s, centre(chevron));
}

/// Carry the drawn icon at `from` along the bar to the far side of the
/// drawn icon at `past`, and let go.
fn carry_past(s: &mut DesktopShell, from: usize, past: usize) {
    let rects = s.tray_icon_rects();
    let at = centre(rects[from]);
    let to = (rects[past].x + rects[past].w - 1.0, at.1);
    press(s, at);
    move_to(s, (at.0 + 6.0, at.1));
    move_to(s, to);
    release(s, to);
}

fn key(i: u32) -> tray_dnd::TrayIconKey {
    tray_dnd::TrayIconKey {
        owner: u64::from(i),
        id: i,
    }
}

/// The icons drawn on the bar, left to right, by number.
fn drawn(s: &DesktopShell) -> Vec<u32> {
    s.ordered_tray_icons().iter().map(|icon| icon.id).collect()
}

/// The icons the overflow lists, in order, by number.
fn listed(s: &DesktopShell) -> Vec<u32> {
    s.overflowed_tray_icons()
        .iter()
        .map(|icon| icon.id)
        .collect()
}

fn centre(r: Rect) -> (f32, f32) {
    (r.x + r.w / 2.0, r.y + r.h / 2.0)
}

fn mouse(s: &mut DesktopShell, (x, y): (f32, f32), kind: MouseEventKind) -> ShellAction {
    s.handle_mouse(&MouseEvent { x, y, kind })
}

fn press(s: &mut DesktopShell, at: (f32, f32)) -> ShellAction {
    mouse(s, at, MouseEventKind::Press(MouseButton::Left))
}

fn move_to(s: &mut DesktopShell, at: (f32, f32)) -> ShellAction {
    mouse(s, at, MouseEventKind::Move)
}

fn release(s: &mut DesktopShell, at: (f32, f32)) -> ShellAction {
    mouse(s, at, MouseEventKind::Release(MouseButton::Left))
}

/// Open the overflow list from the chevron, and answer where its row
/// `index` is.
fn open_list_at_row(s: &mut DesktopShell, index: usize) -> (f32, f32) {
    let chevron = s.tray_overflow_rect().expect("the chevron is up");
    press(s, centre(chevron));
    let (menu, _) = s.tray_overflow_menu.as_ref().expect("the list opened");
    centre(menu.item_rect(index).expect("the row is shown"))
}

/// The texts of the label that follows a carried thing, if one is up.
fn carry_texts(s: &DesktopShell) -> Vec<String> {
    s.render_carry().map_or_else(Vec::new, |tree| {
        tree.commands
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    })
}

/// **An icon let go on the chevron goes into hiding**: off the bar, into
/// the overflow, which keeps the chevron up for it though every shown icon
/// fits.
#[test]
fn an_icon_let_go_on_the_chevron_goes_into_hiding() {
    let mut s = shell_with(3);
    let from = centre(s.tray_icon_rects()[1]);
    press(&mut s, from);
    move_to(&mut s, (from.0, from.1 - 20.0));
    let chevron = s
        .tray_overflow_rect()
        .expect("the chevron comes up to be dropped on");
    move_to(&mut s, centre(chevron));
    assert_eq!(
        carry_texts(&s),
        ["program 2", "Move to hidden icons"],
        "over the chevron the icon says where it is going"
    );
    assert_eq!(release(&mut s, centre(chevron)), ShellAction::Consumed);

    assert_eq!(drawn(&s), [1, 3]);
    assert_eq!(listed(&s), [2]);
    assert!(s.tray_overflows());
    assert!(
        s.tray_overflow_rect().is_some(),
        "the chevron went, leaving the hidden icon unreachable"
    );
    assert!(s.render_carry().is_none());
}

/// **The chevron comes up for a drag and goes when it ends**, and the icons
/// already on the bar keep their places while it is up -- it grows the run
/// from its left end.
#[test]
fn the_chevron_comes_up_for_a_drag_and_goes_when_it_ends() {
    let mut s = shell_with(3);
    assert!(
        s.tray_overflow_rect().is_none(),
        "nothing is in the overflow"
    );
    let before = s.tray_icon_rects();
    let from = centre(before[0]);
    press(&mut s, from);
    assert!(
        s.tray_overflow_rect().is_none(),
        "a press is not yet a drag"
    );
    move_to(&mut s, (from.0, from.1 - 20.0));
    assert!(s.tray_overflow_rect().is_some());
    assert_eq!(s.tray_icon_rects(), before, "the run moved under the drag");
    assert!(
        carry_texts(&s).is_empty(),
        "off the chevron the row's own movement is the feedback"
    );
    release(&mut s, (from.0, from.1 - 20.0));
    assert!(s.tray_overflow_rect().is_none());
    assert_eq!(drawn(&s), [1, 2, 3]);
}

/// **A hidden icon carried out of the list onto the bar is shown where it
/// is let go** -- between the two icons it was dropped between -- and the
/// list closes with nothing clicked.
#[test]
fn a_hidden_icon_carried_onto_the_bar_is_shown_where_it_is_let_go() {
    let mut s = shell_with(3);
    s.tray_arrangement.hide_icon(key(2));
    assert_eq!(drawn(&s), [1, 3]);
    let row = open_list_at_row(&mut s, 0);
    let icons = s.tray_icon_rects();
    let gap = (icons[1].x, icons[1].y + icons[1].h / 2.0);

    assert_eq!(press(&mut s, row), ShellAction::Consumed);
    assert!(
        s.tray_overflow_menu.is_some(),
        "a press on a row chose it before it was known to be a click"
    );
    move_to(&mut s, gap);
    assert_eq!(carry_texts(&s), ["program 2", "Show on the taskbar"]);
    assert_eq!(release(&mut s, gap), ShellAction::Consumed);

    assert_eq!(drawn(&s), [1, 2, 3]);
    assert!(listed(&s).is_empty());
    assert!(s.tray_overflow_menu.is_none(), "the list stayed open");
    assert!(
        s.tray_overflow_rect().is_none(),
        "nothing is hidden any more"
    );
}

/// **A press and release on a row still clicks its program**, as choosing
/// the row always has -- on the release now, since the press no longer
/// knows which it is.
#[test]
fn a_click_on_a_row_of_the_list_clicks_its_program() {
    let mut s = shell_with(3);
    s.tray_arrangement.hide_icon(key(2));
    let row = open_list_at_row(&mut s, 0);
    assert_eq!(press(&mut s, row), ShellAction::Consumed);
    assert_eq!(
        release(&mut s, row),
        ShellAction::Control(ShellRequest::ClickTrayIcon {
            owner: 2,
            id: 2,
            button: MouseButton::Left,
        })
    );
    assert!(s.tray_overflow_menu.is_none());
    assert_eq!(listed(&s), [2], "a click moved the icon");
}

/// **Let go off the bar, a carried icon stays where it was**, the list
/// closes, and no label promised otherwise.
#[test]
fn an_icon_let_go_off_the_bar_stays_in_the_overflow() {
    let mut s = shell_with(3);
    s.tray_arrangement.hide_icon(key(2));
    let row = open_list_at_row(&mut s, 0);
    press(&mut s, row);
    let desktop = (600.0, 300.0);
    move_to(&mut s, desktop);
    assert_eq!(
        carry_texts(&s),
        ["program 2"],
        "a label promised the desktop something"
    );
    assert_eq!(release(&mut s, desktop), ShellAction::Consumed);
    assert_eq!(listed(&s), [2]);
    assert_eq!(drawn(&s), [1, 3]);
    assert!(s.tray_overflow_menu.is_none());
}

/// **Let go past the end of a full bar, an icon takes the last place** --
/// the icon it displaces goes to the overflow -- rather than landing just
/// past the end, where it would still be out of sight.
#[test]
fn an_icon_let_go_past_the_end_of_a_full_bar_takes_its_last_place() {
    let mut s = shell_with(80);
    let before = drawn(&s);
    let outside = listed(&s);
    assert!(!outside.is_empty(), "80 icons must not all fit");
    let carried = outside[0];
    let row = open_list_at_row(&mut s, 0);
    let last = *s.tray_icon_rects().last().unwrap();
    let end = (last.x + last.w - 1.0, last.y + last.h / 2.0);

    press(&mut s, row);
    move_to(&mut s, end);
    release(&mut s, end);

    let after = drawn(&s);
    assert_eq!(after.len(), before.len(), "the bar's room changed");
    assert_eq!(*after.last().unwrap(), carried, "{after:?}");
    assert!(
        listed(&s).contains(before.last().unwrap()),
        "the displaced icon was lost rather than moved to the overflow"
    );
}

/// **A press held on a row does nothing once the list has gone** -- closed
/// by Escape while the button was down -- neither a click nor a move.
#[test]
fn a_press_held_on_a_row_does_nothing_once_the_list_has_gone() {
    let mut s = shell_with(3);
    s.tray_arrangement.hide_icon(key(2));
    let row = open_list_at_row(&mut s, 0);
    press(&mut s, row);
    drop(s.handle_hotkey(&KeyEvent {
        key: Key::Escape,
        pressed: true,
        modifiers: Modifiers::NONE,
        text: String::new(),
    }));
    assert!(s.tray_overflow_menu.is_none());
    let icons = s.tray_icon_rects();
    let gap = (icons[1].x, icons[1].y + icons[1].h / 2.0);
    move_to(&mut s, gap);
    assert!(
        carry_texts(&s).is_empty(),
        "a label for a list that is gone"
    );
    assert_eq!(release(&mut s, gap), ShellAction::Consumed);
    assert_eq!(listed(&s), [2]);
}

/// **A hidden icon is still hidden when its program and the desktop start
/// again**: the choice is kept by the program's name, not by the process,
/// which is new each time.
#[test]
fn a_hidden_icon_stays_hidden_across_a_restart() {
    with_scratch_config("tray-hidden-restart", |_| {
        let mut s = started_with(named_icons(3, 100));
        drop_on_chevron(&mut s, 1);
        assert_eq!(drawn(&s), [1, 3]);

        let again = started_with(named_icons(3, 200));
        assert_eq!(
            drawn(&again),
            [1, 3],
            "the hidden icon came back on the bar"
        );
        assert_eq!(listed(&again), [2]);
    });
}

/// **Where an icon was moved to is where it comes back**, its program
/// started again after the others.
#[test]
fn an_icons_place_is_kept_across_a_restart() {
    with_scratch_config("tray-order-restart", |_| {
        let mut s = started_with(named_icons(3, 100));
        carry_past(&mut s, 0, 2);
        assert_eq!(drawn(&s), [2, 3, 1]);

        // The programs register in their own order, and one late.
        let icons = named_icons(3, 200);
        let mut again = DesktopShell::new(1920, 1080);
        again.load_tray();
        again.apply_tray_icons(vec![icons[2].clone(), icons[0].clone()]);
        assert_eq!(drawn(&again), [3, 1]);
        again.apply_tray_icons(icons);
        assert_eq!(
            drawn(&again),
            [2, 3, 1],
            "the late icon did not take its place"
        );
    });
}

/// **Shown again, an icon stays shown**: carrying it out of the list onto
/// the bar is saved as surely as hiding it.
#[test]
fn an_icon_shown_again_stays_shown_across_a_restart() {
    with_scratch_config("tray-shown-restart", |_| {
        let mut s = started_with(named_icons(3, 100));
        drop_on_chevron(&mut s, 1);
        let row = open_list_at_row(&mut s, 0);
        // Into the gap between the two drawn icons, as the test above.
        let icons = s.tray_icon_rects();
        let gap = (icons[1].x, icons[1].y + icons[1].h / 2.0);
        press(&mut s, row);
        move_to(&mut s, gap);
        release(&mut s, gap);
        assert_eq!(drawn(&s), [1, 2, 3]);

        let again = started_with(named_icons(3, 200));
        assert_eq!(drawn(&again), [1, 2, 3]);
        assert!(listed(&again).is_empty());
    });
}

/// **A click is not a choice**: pressing and letting go of an icon writes
/// nothing, and neither does starting -- the file appears only once the
/// user has moved something.
#[test]
fn a_click_writes_no_settings() {
    with_scratch_config("tray-click-no-write", |root| {
        let mut s = started_with(named_icons(2, 100));
        let at = centre(s.tray_icon_rects()[0]);
        press(&mut s, at);
        release(&mut s, at);
        assert_eq!(
            files_under(root),
            Vec::<std::path::PathBuf>::new(),
            "a click saved the tray"
        );

        // The control: a move does write, so the check above can fail.
        carry_past(&mut s, 0, 1);
        assert_ne!(
            files_under(root),
            Vec::<std::path::PathBuf>::new(),
            "a move saved nothing"
        );
    });
}

/// Every file under `dir`, at any depth.
fn files_under(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(files_under(&path));
        } else {
            out.push(path);
        }
    }
    out
}
