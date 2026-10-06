//! The shell's volume controls on a sound card: the volume keys, the mute
//! key and the pane's slider turn the card's master volume, from its level
//! as it is now -- and say why they cannot when the card is out of reach,
//! moving nothing (design-decisions §1485).

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
