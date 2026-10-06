//! The shell's volume controls on a sound card: the volume keys, the mute
//! key and the pane's slider turn the card's master volume, from its level
//! as it is now -- and say why they cannot when the card is out of reach,
//! moving nothing (design-decisions §1485). And the pane's brightness row,
//! which shows the level the kernel reports and says it cannot be changed.

#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use guitk::palette::Palette;
use guitk::render::RenderCommand;
use sound::Errno;

use crate::backlight::Backlight;
use crate::backlight::tests::Recording;
use crate::volume::Output;
use crate::volume::tests::{Shared, card};
use crate::{DesktopShell, Key, KeyEvent, Modifiers, click};

fn shell() -> DesktopShell {
    DesktopShell::new(1920, 1080)
}

fn press(s: &mut DesktopShell, key: Key) {
    let outcome = s.handle_hotkey(&KeyEvent {
        key,
        pressed: true,
        modifiers: Modifiers::NONE,
        text: String::new(),
    });
    assert!(outcome.consumed, "{key:?} is the shell's");
}

/// The card's level and whether its switch is off.
fn on_card(card: &Shared) -> (i64, bool) {
    let state = card.0.borrow();
    (state.volume[0], state.switch.as_ref().unwrap()[0] == 0)
}

/// Every piece of text the overlay draws.
fn overlay_text(s: &DesktopShell) -> Vec<String> {
    s.osd
        .render(&Palette::for_mode(false))
        .into_iter()
        .filter_map(|c| match c {
            RenderCommand::Text { text, .. } => Some(text),
            _ => None,
        })
        .collect()
}

/// Every piece of text the open pane draws.
fn pane_text(s: &DesktopShell) -> Vec<String> {
    s.notifications
        .render(&Palette::for_mode(false), 1920.0, 1080.0)
        .into_iter()
        .filter_map(|c| match c {
            RenderCommand::Text { text, .. } => Some(text),
            _ => None,
        })
        .collect()
}

/// **The volume keys turn the card's volume, and the mute key its switch**,
/// and the overlay shows where they left it.
#[test]
fn the_volume_keys_turn_the_cards_volume() {
    let mut s = shell();
    let (card_state, output) = card(40, false);
    s.attach_volume(output);
    assert_eq!(s.notifications.volume(), 40, "the card's level, shown");
    press(&mut s, Key::VolumeUp);
    assert_eq!(on_card(&card_state), (45, false));
    assert_eq!(s.notifications.volume(), 45);
    assert!(
        overlay_text(&s).iter().any(|t| t.starts_with("Volume")),
        "{:?}",
        overlay_text(&s)
    );
    press(&mut s, Key::VolumeMute);
    assert_eq!(on_card(&card_state), (45, true), "muted, the level kept");
    assert!(overlay_text(&s).iter().any(|t| t.starts_with("Muted")));
    press(&mut s, Key::VolumeUp);
    assert_eq!(on_card(&card_state), (50, false), "turning it up unmutes");
}

/// **A volume key starts from the card's level as it is now** -- another
/// program may have moved it -- not from the shell's last idea of it.
#[test]
fn a_volume_key_starts_from_the_cards_level_now() {
    let mut s = shell();
    let (card_state, output) = card(40, false);
    s.attach_volume(output);
    card_state.0.borrow_mut().volume = vec![70];
    press(&mut s, Key::VolumeDown);
    assert_eq!(on_card(&card_state), (65, false));
    // And muted elsewhere: the mute key lets it sound.
    card_state.0.borrow_mut().switch = Some(vec![0]);
    press(&mut s, Key::VolumeMute);
    assert_eq!(on_card(&card_state), (65, false));
}

/// **Opening the pane shows the card's level as it is now.**
#[test]
fn opening_the_pane_shows_the_cards_level_now() {
    let mut s = shell();
    let (card_state, output) = card(40, false);
    s.attach_volume(output);
    card_state.0.borrow_mut().volume = vec![25];
    s.toggle_notifications();
    assert_eq!(s.notifications.volume(), 25);
    assert!(
        pane_text(&s).iter().any(|t| t == "Volume  25%"),
        "{:?}",
        pane_text(&s)
    );
}

/// The volume slider's track on the screen, as the open pane draws it: the
/// first fill of a slider's thickness and length, moved as the pane's
/// drawing is.
fn volume_track(s: &DesktopShell) -> (f32, f32, f32, f32) {
    let mut dx = 0.0;
    for c in s
        .notifications
        .render(&Palette::for_mode(false), 1920.0, 1080.0)
    {
        match c {
            RenderCommand::PushTranslate { dx: by, .. } => dx += by,
            RenderCommand::FillRect {
                x,
                y,
                width,
                height,
                ..
            } if (width - 140.0).abs() < 0.5 && height <= 8.0 => {
                return (x + dx, y, width, height);
            }
            _ => {}
        }
    }
    panic!("the volume slider's track is drawn")
}

/// **The pane's slider turns the card's volume** as it moves.
#[test]
fn the_panes_slider_turns_the_cards_volume() {
    let mut s = shell();
    let (card_state, output) = card(40, false);
    s.attach_volume(output);
    s.toggle_notifications();
    let (x, y, w, h) = volume_track(&s);
    // The far end of the track: wherever a press there takes the level, it
    // is not where it was.
    s.handle_mouse(&click(x + w - 1.0, y + h / 2.0));
    let level = s.notifications.volume();
    assert_ne!(level, 40, "the press moved the slider");
    assert_eq!(on_card(&card_state), (i64::from(level), false));
}

/// **A card out of reach is said to be, and nothing moves**: the pane shows
/// why where the slider would be, a press there does nothing, and the
/// volume keys show why in the overlay rather than a level that changes
/// nothing anyone hears.
#[test]
fn a_card_out_of_reach_is_said_to_be_and_nothing_moves() {
    let mut s = shell();
    s.attach_volume(Output::OutOfReach("No sound card reachable"));
    let level = s.notifications.volume();
    s.toggle_notifications();
    let text = pane_text(&s);
    assert!(
        text.iter().any(|t| t == "No sound card reachable"),
        "{text:?}"
    );
    assert!(
        text.iter().any(|t| t == "Volume"),
        "no level beside it: {text:?}"
    );
    assert!(!text.iter().any(|t| t.starts_with("Volume  ")), "{text:?}");
    s.toggle_notifications();
    press(&mut s, Key::VolumeUp);
    press(&mut s, Key::VolumeMute);
    assert_eq!(s.notifications.volume(), level);
    assert!(!s.notifications.is_muted());
    let overlay = overlay_text(&s);
    assert!(
        overlay.iter().any(|t| t == "No sound card reachable"),
        "{overlay:?}"
    );
    assert!(
        !overlay
            .iter()
            .any(|t| t.starts_with("Volume") || t.starts_with("Muted")),
        "{overlay:?}"
    );
}

/// **A card lost after it was found is said to be lost** -- the next key
/// press finds it out of reach -- and the level stays where the user left
/// it.
#[test]
fn a_card_lost_after_it_was_found_is_said_to_be() {
    let mut s = shell();
    let (card_state, output) = card(40, false);
    s.attach_volume(output);
    card_state.0.borrow_mut().refuse = Some(Errno::ENODEV);
    press(&mut s, Key::VolumeUp);
    assert_eq!(s.notifications.volume(), 40);
    assert_eq!(
        s.volume_output().out_of_reach(),
        Some("Sound card not answering")
    );
    assert!(
        overlay_text(&s)
            .iter()
            .any(|t| t == "Sound card not answering")
    );
    assert_eq!(
        s.notifications.volume_out_of_reach(),
        Some("Sound card not answering")
    );
}

/// **A shell that asked for no card keeps its own number**, as before: the
/// keys move it and the overlay shows it.
#[test]
fn a_shell_with_no_card_keeps_its_own_number() {
    let mut s = shell();
    let level = s.notifications.volume();
    press(&mut s, Key::VolumeUp);
    assert_eq!(s.notifications.volume(), level + 5);
    assert_eq!(s.volume_output().out_of_reach(), None);
    assert!(overlay_text(&s).iter().any(|t| t.starts_with("Volume")));
}

/// The brightness slider's track on the screen, as the open pane draws it:
/// the second fill of a slider's thickness and length.
fn brightness_track(s: &DesktopShell) -> Option<(f32, f32, f32, f32)> {
    let mut dx = 0.0;
    let mut seen = 0;
    for c in s
        .notifications
        .render(&Palette::for_mode(false), 1920.0, 1080.0)
    {
        match c {
            RenderCommand::PushTranslate { dx: by, .. } => dx += by,
            RenderCommand::FillRect {
                x,
                y,
                width,
                height,
                ..
            } if (width - 140.0).abs() < 0.5 && height <= 8.0 => {
                seen += 1;
                if seen == 2 {
                    return Some((x + dx, y, width, height));
                }
            }
            _ => {}
        }
    }
    None
}

/// **The pane's brightness is the screen's, as the kernel reports it, and
/// says it cannot be changed**: the level beside the label, the reason in
/// the slider's place, and a press there moves nothing.
#[test]
fn the_panes_brightness_is_the_screens_and_says_it_cannot_change() {
    let dir = scratchdir::ScratchDir::new("pane-brightness");
    let report = dir.path("brightness");
    std::fs::write(
        &report,
        "display_count: 1\nDisplays:\n  0   Built-in display      35%  min   5%  [manual]\n",
    )
    .unwrap();
    let mut s = shell();
    let (x, y, w, h) = {
        // Where the slider was before the report: the place a press is tried.
        s.toggle_notifications();
        let track = brightness_track(&s).expect("a slider before the report");
        s.toggle_notifications();
        track
    };
    // The kernel refuses the desktop: it holds no `SET_BRIGHTNESS`.
    let refused = Recording::default();
    *refused.refuse.borrow_mut() = Some(1);
    s.attach_backlight(Backlight::with(report, Box::new(refused)));
    s.toggle_notifications();
    let text = pane_text(&s);
    assert!(text.iter().any(|t| t == "Brightness  35%"), "{text:?}");
    assert!(text.iter().any(|t| t == "Can't be changed yet"), "{text:?}");
    assert_eq!(brightness_track(&s), None, "no slider where it cannot move");
    s.handle_mouse(&click(x + w - 1.0, y + h / 2.0));
    assert_eq!(s.notifications.brightness(), 35);
}

/// **Where the desktop may set the screen's brightness, the pane's slider
/// is the screen's**: it shows the level and sets it as it moves.
#[test]
fn where_the_desktop_may_the_brightness_slider_sets_the_screen() {
    let dir = scratchdir::ScratchDir::new("pane-brightness-live");
    let report = dir.path("brightness");
    std::fs::write(
        &report,
        "display_count: 1\nDisplays:\n  4   Panel                 35%  min   5%  [manual]\n",
    )
    .unwrap();
    let mut s = shell();
    let setter = Recording::default();
    s.attach_backlight(Backlight::with(report, Box::new(setter.clone())));
    s.toggle_notifications();
    let text = pane_text(&s);
    assert!(text.iter().any(|t| t == "Brightness  35%"), "{text:?}");
    let (x, y, w, h) = brightness_track(&s).expect("the slider is there");
    s.handle_mouse(&click(x + w - 1.0, y + h / 2.0));
    let level = s.notifications.brightness();
    assert_ne!(level, 35, "the press moved it");
    assert_eq!(
        setter.asked.borrow().last(),
        Some(&(4, level)),
        "onto the screen"
    );
}

/// **With no report, no level -- and why.**
#[test]
fn with_no_brightness_report_the_pane_says_so() {
    let dir = scratchdir::ScratchDir::new("pane-no-brightness");
    let mut s = shell();
    s.attach_backlight(Backlight::with(
        dir.path("missing"),
        Box::new(Recording::default()),
    ));
    s.toggle_notifications();
    let text = pane_text(&s);
    assert!(text.iter().any(|t| t == "Brightness"), "{text:?}");
    assert!(
        text.iter().any(|t| t == "No brightness control reachable"),
        "{text:?}"
    );
    assert!(
        !text.iter().any(|t| t.starts_with("Brightness  ")),
        "{text:?}"
    );
}

/// **A shell that asked for no screen keeps the pane's own slider.**
#[test]
fn a_shell_with_no_screen_keeps_its_brightness_slider() {
    let mut s = shell();
    s.toggle_notifications();
    assert!(brightness_track(&s).is_some());
    assert_eq!(s.notifications.brightness_fixed(), None);
}

// ---- The tray's speaker and its flyout ----

/// The middle of a rectangle.
fn middle(r: crate::Rect) -> (f32, f32) {
    (r.x + r.w / 2.0, r.y + r.h / 2.0)
}

/// **The speaker sits left of the bell, and a press on it opens the flyout
/// and a second closes it**; a press anywhere else closes it too.
#[test]
fn the_speaker_opens_and_closes_its_flyout() {
    let mut s = shell();
    let speaker = s.volume_icon_rect();
    let bell = s.bell_rect();
    assert!(speaker.x + speaker.w <= bell.x + 0.01, "left of the bell");
    let (x, y) = middle(speaker);
    assert_eq!(s.hit_test(x, y), crate::Hit::VolumeIcon);
    s.handle_mouse(&click(x, y));
    assert!(s.volume_flyout.is_visible(), "the press opened it");
    let panel = s.volume_flyout_layout().panel;
    assert!(panel.y + panel.h <= s.taskbar_rect().y, "above the bar");
    assert!(s.render_volume_flyout().is_some());
    s.handle_mouse(&click(x, y));
    assert!(!s.volume_flyout.is_visible(), "the second closed it");
    s.handle_mouse(&click(x, y));
    s.handle_mouse(&click(400.0, 300.0));
    assert!(!s.volume_flyout.is_visible(), "a press off it closed it");
}

/// **The flyout's slider and switch turn the card**, and a press in its own
/// margin keeps it open.
#[test]
fn the_flyouts_slider_and_switch_turn_the_card() {
    let mut s = shell();
    let (card_state, output) = card(40, false);
    s.attach_volume(output);
    s.toggle_volume_flyout();
    let layout = s.volume_flyout_layout();
    let track = layout.slider.track;
    s.handle_mouse(&click(track.x + track.w, track.y + track.h / 2.0));
    assert_eq!(on_card(&card_state), (100, false));
    s.handle_mouse(&crate::MouseEvent {
        x: track.x + track.w,
        y: track.y,
        kind: crate::MouseEventKind::Release(crate::MouseButton::Left),
    });
    s.handle_mouse(&click(layout.mute.x + 2.0, layout.mute.y + 2.0));
    assert_eq!(on_card(&card_state), (100, true), "muted");
    s.handle_mouse(&click(layout.panel.x + 4.0, layout.panel.y + 4.0));
    assert!(s.volume_flyout.is_visible(), "its own margin is its own");
}

/// **A drag of the flyout's slider turns the card as it goes.**
#[test]
fn a_drag_of_the_flyouts_slider_turns_the_card() {
    let mut s = shell();
    let (card_state, output) = card(0, false);
    s.attach_volume(output);
    s.toggle_volume_flyout();
    let track = s.volume_flyout_layout().slider.track;
    let y = track.y + track.h / 2.0;
    s.handle_mouse(&click(track.x, y));
    s.handle_mouse(&crate::MouseEvent {
        x: track.x + track.w / 2.0,
        y: y + 200.0,
        kind: crate::MouseEventKind::Move,
    });
    assert_eq!(
        on_card(&card_state),
        (50, false),
        "followed off the panel too"
    );
    s.handle_mouse(&crate::MouseEvent {
        x: track.x + track.w / 2.0,
        y,
        kind: crate::MouseEventKind::Release(crate::MouseButton::Left),
    });
    assert!(!s.volume_flyout.dragging());
}

/// **The wheel over the speaker turns the volume a step a notch, and the
/// keys move the open flyout's slider; Escape closes it.**
#[test]
fn the_wheel_and_the_keys_turn_it() {
    let mut s = shell();
    let (card_state, output) = card(40, false);
    s.attach_volume(output);
    let (x, y) = middle(s.volume_icon_rect());
    s.handle_mouse(&crate::scroll(x, y, 2.0));
    assert_eq!(on_card(&card_state), (50, false), "two notches up");
    s.handle_mouse(&crate::scroll(x, y, -1.0));
    assert_eq!(on_card(&card_state), (45, false));
    s.toggle_volume_flyout();
    press(&mut s, Key::Right);
    assert_eq!(on_card(&card_state), (46, false));
    press(&mut s, Key::Escape);
    assert!(!s.volume_flyout.is_visible());
}

/// **The speaker's tooltip says the level, or why there is none.**
#[test]
fn the_speakers_tooltip_says_the_level_or_why() {
    let mut s = shell();
    let (_card_state, output) = card(40, false);
    s.attach_volume(output);
    assert_eq!(s.volume_tooltip(), "Volume: 40%");
    press(&mut s, Key::VolumeMute);
    assert_eq!(s.volume_tooltip(), "Volume: muted (40%)");
    s.attach_volume(Output::OutOfReach("No sound card reachable"));
    assert_eq!(s.volume_tooltip(), "Volume: No sound card reachable");
}

/// **Out of reach, the flyout says why, and the wheel moves nothing but
/// says so on screen.**
#[test]
fn out_of_reach_the_flyout_says_why() {
    let mut s = shell();
    s.attach_volume(Output::OutOfReach("No sound card reachable"));
    let level = s.notifications.volume();
    s.toggle_volume_flyout();
    let drawn = s.render_volume_flyout().unwrap();
    assert!(drawn.commands.iter().any(|c| matches!(
        c,
        RenderCommand::Text { text, .. } if text == "No sound card reachable"
    )));
    s.toggle_volume_flyout();
    let (x, y) = middle(s.volume_icon_rect());
    s.handle_mouse(&crate::scroll(x, y, 1.0));
    assert_eq!(s.notifications.volume(), level);
    assert!(
        overlay_text(&s)
            .iter()
            .any(|t| t == "No sound card reachable")
    );
}

/// **The flyout and the other panels over the bar close one another.**
#[test]
fn the_flyout_and_the_other_panels_close_one_another() {
    let mut s = shell();
    s.toggle_volume_flyout();
    s.toggle_calendar();
    assert!(!s.volume_flyout.is_visible(), "the calendar closed it");
    s.toggle_volume_flyout();
    assert!(!s.calendar.visible, "and it the calendar");
    s.toggle_notifications();
    assert!(!s.volume_flyout.is_visible(), "the pane closed it");
    s.toggle_volume_flyout();
    assert!(
        !s.notifications.pane_state().is_visible(),
        "and it the pane"
    );
    s.toggle_start_menu();
    assert!(!s.volume_flyout.is_visible(), "the start menu closed it");
    s.toggle_volume_flyout();
    assert!(s.any_popup_open());
    assert!(s.dismiss_popups());
    assert!(!s.volume_flyout.is_visible());
}
