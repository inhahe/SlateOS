// A test module's job is to fail loudly the instant the code under test is
// wrong, so the defensive lints that forbid exactly that in production code
// are off here -- as `CLAUDE.md` prescribes.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use super::*;
use crate::DesktopShell;
use crate::focus_assist::FocusMode;
use crate::notif_pane::{AppSettingKind, Notification, SettingValue};
use crate::osd::{LockType, OsdIcon};

/// A notification from `app` at `priority`.
fn notif(app: &str, priority: NotifPriority) -> Notification {
    Notification {
        id: 0,
        app_name: app.to_owned(),
        title: "Lunch?".to_owned(),
        body: String::new(),
        timestamp: 0,
        priority,
        read: false,
        action: None,
        silent: false,
    }
}

/// The events `shell` has asked to sound, oldest first.
fn heard(shell: &DesktopShell) -> Vec<&str> {
    events(&shell.event_sounds)
}

/// The events `sounds` was asked for, oldest first.
fn events(sounds: &EventSounds) -> Vec<&str> {
    sounds.recent().map(|asked| asked.event.as_str()).collect()
}

/// Every priority a notification has.
const PRIORITIES: [NotifPriority; 4] = [
    NotifPriority::Low,
    NotifPriority::Normal,
    NotifPriority::High,
    NotifPriority::Urgent,
];

/// One on-screen display of every kind, with the sound each makes.
fn every_osd() -> Vec<(OsdKind, Option<&'static str>)> {
    vec![
        (
            OsdKind::Volume {
                level: 40,
                muted: false,
            },
            Some("audio-volume-change"),
        ),
        (
            OsdKind::Volume {
                level: 40,
                muted: true,
            },
            None,
        ),
        (OsdKind::Brightness { level: 40 }, None),
        (
            OsdKind::MediaTrack {
                title: "Song".into(),
                artist: "Band".into(),
                album: "Record".into(),
            },
            None,
        ),
        (OsdKind::MediaPlayPause { playing: true }, None),
        (
            OsdKind::KeyboardLock {
                lock_type: LockType::CapsLock,
                active: true,
            },
            None,
        ),
        (
            OsdKind::DeviceEvent {
                device_name: "USB stick".into(),
                ejected: false,
            },
            Some("device-added"),
        ),
        (
            OsdKind::DeviceEvent {
                device_name: "USB stick".into(),
                ejected: true,
            },
            Some("device-removed"),
        ),
        (
            OsdKind::ScreenshotTaken {
                path: "/home/u/Pictures/shot.png".into(),
            },
            Some("screen-capture"),
        ),
        (OsdKind::Microphone { muted: true }, None),
        (
            OsdKind::NetworkStatus {
                connected: true,
                name: "home".into(),
            },
            Some("network-connectivity-established"),
        ),
        (
            OsdKind::NetworkStatus {
                connected: false,
                name: "home".into(),
            },
            Some("network-connectivity-lost"),
        ),
        (OsdKind::BatteryLow { percent: 5 }, Some("battery-low")),
        (
            OsdKind::Custom {
                icon: OsdIcon::Info,
                message: "Copied".into(),
            },
            None,
        ),
    ]
}

/// **A notification sounds by its priority**: a message's chime, a warning
/// for an urgent one, and nothing for a low one, which asks to be seen when
/// looked for.
#[test]
fn a_notification_sounds_by_its_priority() {
    assert_eq!(for_notification(NotifPriority::Low), None);
    assert_eq!(
        for_notification(NotifPriority::Normal),
        Some("message-new-instant")
    );
    assert_eq!(
        for_notification(NotifPriority::High),
        Some("message-new-instant")
    );
    assert_eq!(
        for_notification(NotifPriority::Urgent),
        Some("dialog-warning")
    );
    let mut s = DesktopShell::new(1920, 1080);
    for priority in PRIORITIES {
        s.notify(notif("Chat", priority));
    }
    assert_eq!(
        heard(&s),
        [
            "message-new-instant",
            "message-new-instant",
            "dialog-warning"
        ]
    );
}

/// **One focus assist silenced makes no sound** -- it is kept, for the
/// pane, and not announced.
#[test]
fn a_silenced_notification_makes_no_sound() {
    let mut s = DesktopShell::new(1920, 1080);
    s.focus.set_mode(FocusMode::TotalSilence);
    s.notify(notif("Alarms", NotifPriority::Urgent));
    assert!(heard(&s).is_empty(), "{:?}", heard(&s));
    assert_eq!(s.notifications.notifications().len(), 1, "kept");
    s.focus.set_mode(FocusMode::Off);
    s.notify(notif("Alarms", NotifPriority::Urgent));
    assert_eq!(heard(&s), ["dialog-warning"]);
}

/// **A program whose sound is off makes none**, while others still do; and
/// its banner is a separate switch -- one whose banner is off is heard.
#[test]
fn a_programs_rule_turns_its_sound_off() {
    settingsfile::testing::with_scratch_config("event-sounds-rule", |_root| {
        let mut s = DesktopShell::new(1920, 1080);
        s.apply_app_notification_setting("Chat", AppSettingKind::Sound, SettingValue::Bool(false));
        s.notify(notif("Chat", NotifPriority::Normal));
        assert!(heard(&s).is_empty(), "its sound is off");
        s.notify(notif("Mail", NotifPriority::Normal));
        assert_eq!(heard(&s), ["message-new-instant"], "another's is not");

        s.apply_app_notification_setting("Chat", AppSettingKind::Sound, SettingValue::Bool(true));
        s.apply_app_notification_setting("Chat", AppSettingKind::Banner, SettingValue::Bool(false));
        s.notify(notif("Chat", NotifPriority::Normal));
        assert_eq!(
            heard(&s),
            ["message-new-instant", "message-new-instant"],
            "no banner, still a sound"
        );
    });
}

/// **One that arrives while the pane is open still sounds**: it pops up no
/// toast, the pane showing it already, but a card added to a list being
/// read is easily missed.
#[test]
fn a_notification_into_the_open_pane_still_sounds() {
    settingsfile::testing::with_scratch_config("event-sounds-pane", |_root| {
        let mut s = DesktopShell::new(1920, 1080);
        s.toggle_notifications();
        assert!(s.notifications.pane_state().is_visible());
        s.notify(notif("Chat", NotifPriority::Normal));
        assert_eq!(heard(&s), ["message-new-instant"]);
    });
}

/// **Each on-screen display makes its sound or none**: a volume's new level,
/// a device, a screenshot's shutter, a low battery and the network are
/// heard; a mute, brightness, a track, a lock key, the microphone and the
/// shell's own words are not.
#[test]
fn each_on_screen_display_makes_its_sound_or_none() {
    let mut s = DesktopShell::new(1920, 1080);
    let mut expected = Vec::new();
    for (kind, sound) in every_osd() {
        assert_eq!(for_osd(&kind), sound, "{kind:?}");
        expected.extend(sound);
        s.show_osd(kind);
    }
    assert_eq!(heard(&s), expected);
}

/// Every event the shell sounds: its notifications', its on-screen displays',
/// and signing in and out (`session.rs`).
fn every_event() -> Vec<&'static str> {
    let mut events: Vec<&str> = PRIORITIES
        .into_iter()
        .filter_map(for_notification)
        .chain(every_osd().into_iter().filter_map(|(_, sound)| sound))
        .chain(["desktop-login", "desktop-logout"])
        .collect();
    events.sort_unstable();
    events.dedup();
    events
}

/// **The events a settings page offers are the shell's**: every one it
/// sounds is listed (`appearance::sounds::SHELL_EVENTS`), and nothing it
/// does not.
#[test]
fn the_events_a_settings_page_offers_are_the_shells() {
    let mut listed: Vec<&str> = appearance::sounds::SHELL_EVENTS
        .iter()
        .map(|event| event.name)
        .collect();
    listed.sort_unstable();
    assert_eq!(listed, every_event());
}

/// **Every event the shell sounds has a built-in sound of its own**, so a
/// desktop with no sound theme installed is not silent where a themed one
/// would sound, nor sounds a cut of the name's.
#[test]
fn every_event_the_shell_sounds_has_a_built_in_sound() {
    for event in every_event() {
        assert_eq!(
            sound::BuiltIn::for_event(event).map(sound::BuiltIn::name),
            Some(event),
            "{event}"
        );
    }
}

/// **What plays is the user's choice**: the sound theme's, here the
/// built-in sound -- and with sounds turned off, nothing, though the event
/// is still recorded, with what was chosen.
#[test]
fn what_plays_is_the_users_choice() {
    let mut sounds = EventSounds::new();
    assert!(!playback_allowed(), "a test hears nothing");
    let mut look = AppearanceSettings::default();
    let built_in = SoundChoice::BuiltIn("message-new-instant".to_string());
    assert_eq!(sounds.sound(&look, "message-new-instant"), built_in);
    look.sounds.enabled = false;
    assert_eq!(
        sounds.sound(&look, "message-new-instant"),
        SoundChoice::Silent
    );
    let asked = |choice| Asked {
        event: "message-new-instant".to_string(),
        choice,
    };
    assert_eq!(
        sounds.recent().cloned().collect::<Vec<_>>(),
        [asked(built_in), asked(SoundChoice::Silent)]
    );
}

/// **The record keeps the latest [`RECENT`] events**, dropping the oldest.
#[test]
fn the_record_keeps_the_latest_events() {
    let mut sounds = EventSounds::default();
    let look = AppearanceSettings::default();
    for i in 0..RECENT + 5 {
        sounds.sound(&look, &format!("event-{i}"));
    }
    let recent = events(&sounds);
    assert_eq!(recent.len(), RECENT);
    assert_eq!(recent.first().copied(), Some("event-5"));
    let last = format!("event-{}", RECENT + 4);
    assert_eq!(recent.last().copied(), Some(last.as_str()));
}
