//! The quick settings' switches do what they show: Dark Mode turns the
//! desktop dark or light and saves it, following the mode however it was
//! chosen; Wi-Fi and Bluetooth, with no radio behind them, say so and take
//! no press (design-decisions §1485).

#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use appearance::{AppearanceSettings, ThemeMode};
use guitk::palette::Palette;
use guitk::render::RenderCommand;

use crate::notif_pane::{NotifPaneEvent, QuickSetting};
use crate::{DesktopShell, RADIOS_UNAVAILABLE};

fn shell() -> DesktopShell {
    DesktopShell::new(1920, 1080)
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

/// **Dark Mode turns the desktop light or dark, saves it, and shows it.**
#[test]
fn dark_mode_turns_the_desktop_and_saves_it() {
    appearance::config::testing::with_scratch_config("quick-dark-mode", |_root| {
        let mut s = shell();
        let dark = !s.appearance.is_light();
        assert_eq!(
            s.notifications.quick_setting_value(QuickSetting::DarkMode),
            dark,
            "the switch shows what is drawn"
        );
        s.toggle_dark_mode();
        assert_eq!(s.appearance.is_light(), dark, "the desktop turned");
        assert_eq!(
            s.notifications.quick_setting_value(QuickSetting::DarkMode),
            !dark
        );
        let saved = appearance::AppearanceFile::load().settings.theme_mode;
        assert_eq!(
            saved,
            if dark {
                ThemeMode::Light
            } else {
                ThemeMode::Dark
            }
        );
        assert!(s.take_appearance_change(), "the compositor is told");
        s.toggle_dark_mode();
        assert_eq!(s.appearance.is_light(), !dark, "and back");
    });
}

/// **In the automatic mode the switch shows the hour's mode, and flipping
/// it chooses the other outright** -- a switch that flipped and stayed
/// showing the same mode until the next edge of the day would look broken.
#[test]
fn in_the_automatic_mode_the_switch_shows_the_hour_and_chooses_the_other() {
    appearance::config::testing::with_scratch_config("quick-dark-auto", |_root| {
        let mut s = shell();
        let mut auto = AppearanceSettings::default();
        auto.theme_mode = ThemeMode::System;
        auto.auto_is_light = true;
        s.set_appearance(auto);
        assert!(!s.notifications.quick_setting_value(QuickSetting::DarkMode));
        s.toggle_dark_mode();
        assert_eq!(s.appearance.theme_mode, ThemeMode::Dark);
        assert!(s.notifications.quick_setting_value(QuickSetting::DarkMode));
    });
}

/// **A mode chosen elsewhere turns the switch with it.**
#[test]
fn a_mode_chosen_elsewhere_turns_the_switch() {
    let mut s = shell();
    let mut light = AppearanceSettings::default();
    light.theme_mode = ThemeMode::Light;
    s.set_appearance(light);
    assert!(!s.notifications.quick_setting_value(QuickSetting::DarkMode));
    let mut dark = AppearanceSettings::default();
    dark.theme_mode = ThemeMode::Dark;
    s.set_appearance(dark);
    assert!(s.notifications.quick_setting_value(QuickSetting::DarkMode));
}

/// **The radios' switches say they are not available, and take no press.**
#[test]
fn the_radio_switches_say_they_are_not_available() {
    let mut s = shell();
    for radio in [QuickSetting::WiFi, QuickSetting::Bluetooth] {
        assert_eq!(s.notifications.unavailable(radio), Some(RADIOS_UNAVAILABLE));
    }
    assert_eq!(s.notifications.unavailable(QuickSetting::DarkMode), None);
    s.toggle_notifications();
    let text = pane_text(&s);
    assert_eq!(
        text.iter().filter(|t| *t == RADIOS_UNAVAILABLE).count(),
        2,
        "{text:?}"
    );
    // A press where Wi-Fi's switch would be.
    let before = s.notifications.quick_setting_value(QuickSetting::WiFi);
    s.notifications.press_switch(QuickSetting::WiFi);
    assert_eq!(
        s.notifications.quick_setting_value(QuickSetting::WiFi),
        before
    );
    assert!(
        !s.notifications
            .drain_events()
            .contains(&NotifPaneEvent::QuickSettingToggled(QuickSetting::WiFi))
    );
    // The control: Dark Mode's switch, pressed the same way, is pressed.
    s.notifications.press_switch(QuickSetting::DarkMode);
    assert!(
        s.notifications
            .drain_events()
            .contains(&NotifPaneEvent::QuickSettingToggled(QuickSetting::DarkMode))
    );
}
