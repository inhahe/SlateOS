//! Tests for the pictures of a theme: the scene's parts, and the composed
//! picture following the theme it is drawn in -- its colours, its mode, its
//! wallpaper.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_cmp
)]

use super::*;
use appearance::ThemeMode;
use appearance::themes::{ColorTheme, ThemeDirs, WallpaperTheme};
use guitk::color::Color;
use guitk::palette::ThemeColors;
use guitk::render::RenderCommand;
use scratchdir::ScratchDir;
use std::collections::BTreeMap;

/// The smallest picture, drawn quickly.
const SIZE: (u32, u32) = MIN_SIZE;

/// A point on the desktop nothing in the scene covers: its top right
/// corner, above and beside the window, far from the start menu, the
/// taskbar and the notification.
fn open_desktop() -> (u32, u32) {
    (SIZE.0 - 8, 8)
}

fn dark() -> AppearanceSettings {
    AppearanceSettings {
        theme_mode: ThemeMode::Dark,
        ..AppearanceSettings::default()
    }
}

/// Settings wearing a colour theme whose grounds are all `ground`.
fn grounds_of(ground: Color) -> AppearanceSettings {
    let mut dark_colors = BTreeMap::new();
    for role in ["base", "mantle", "crust"] {
        dark_colors.insert(role.to_owned(), ground);
    }
    AppearanceSettings {
        color_theme: ColorTheme::from_colors(
            "grounds",
            ThemeColors {
                dark: dark_colors,
                ..ThemeColors::default()
            },
        ),
        ..dark()
    }
}

/// The colour `argb` as the compositor wrote it, alpha aside.
fn rgb(argb: u32) -> u32 {
    argb & 0x00FF_FFFF
}

#[test]
fn a_size_outside_the_bounds_is_refused() {
    for (w, h) in [
        (MIN_SIZE.0 - 1, MIN_SIZE.1),
        (MIN_SIZE.0, MIN_SIZE.1 - 1),
        (MAX_SIZE.0 + 1, MAX_SIZE.1),
        (0, 0),
    ] {
        assert_eq!(
            Scene::new(&dark(), w, h).map(|_| ()),
            Err(PreviewError::Size(w, h))
        );
    }
    assert!(PreviewError::Size(1, 2).to_string().contains("1x2 is not"));
}

/// **Every part of the scene is drawn**: the background with its icons,
/// the window of controls, the taskbar along the bottom, the start menu
/// open, and a notification.
#[test]
fn every_part_of_the_scene_is_drawn() {
    let scene = Scene::new(&dark(), SIZE.0, SIZE.1).unwrap();
    assert!(!scene.background.commands.is_empty());
    assert!(
        scene
            .background
            .commands
            .iter()
            .any(|c| matches!(c, RenderCommand::Image { .. })),
        "the icons are pictures"
    );
    let (window, at) = &scene.window;
    assert!(!window.commands.is_empty());
    assert!(at.x >= 0.0 && at.x + at.w <= f32::from(u16::try_from(SIZE.0).unwrap()));
    let menu = scene.menus.1;
    assert!(at.x >= menu.x + menu.w, "the window is over the start menu");
    let (bar, bar_at) = &scene.taskbar;
    assert!(!bar.commands.is_empty());
    assert_eq!(
        bar_at.y + bar_at.h,
        f32::from(u16::try_from(SIZE.1).unwrap())
    );
    assert!(!scene.menus.0.commands.is_empty(), "the start menu is open");
    let menu_at = scene.menus.1;
    assert!(
        menu_at.w < f32::from(u16::try_from(SIZE.0).unwrap()),
        "not the whole screen"
    );
    assert!(scene.toasts.is_some(), "a notification is up");
    assert!(scene.wallpaper.is_none(), "no theme recommends one here");
}

/// **The picture is the size asked, and something is drawn in it**: not one
/// colour from corner to corner.
#[test]
fn the_picture_is_drawn_at_its_size() {
    let picture = render(&dark(), SIZE.0, SIZE.1).unwrap();
    assert_eq!((picture.width, picture.height), SIZE);
    assert_eq!(
        picture.argb.len(),
        usize::try_from(SIZE.0 * SIZE.1).unwrap()
    );
    let first = picture.argb[0];
    assert!(picture.argb.iter().any(|&p| p != first));
    assert_eq!(picture.pixel(SIZE.0, 0), None);
    assert_eq!(picture.pixel(0, SIZE.1), None);
    // As a PNG, it reads back as itself.
    let png = picture.png().unwrap();
    let back = imagecodec::decode(&png, imagecodec::Limits::default()).unwrap();
    assert_eq!((back.width, back.height), SIZE);
}

/// **The picture is drawn in the theme**: the open desktop is the theme's
/// ground, and a theme with another ground draws another picture.
#[test]
fn the_picture_follows_the_themes_colours() {
    let magenta = Color::rgb(0xC0, 0x10, 0xA0);
    let themed = render(&grounds_of(magenta), SIZE.0, SIZE.1).unwrap();
    let (x, y) = open_desktop();
    assert_eq!(
        rgb(themed.pixel(x, y).unwrap()),
        0x00C0_10A0,
        "the desktop is not the theme's ground"
    );
    let plain = render(&dark(), SIZE.0, SIZE.1).unwrap();
    assert_ne!(themed.pixel(x, y), plain.pixel(x, y));
}

/// **Light and dark are two pictures.**
#[test]
fn light_and_dark_draw_differently() {
    let light = AppearanceSettings {
        theme_mode: ThemeMode::Light,
        ..AppearanceSettings::default()
    };
    let (x, y) = open_desktop();
    let in_light = render(&light, SIZE.0, SIZE.1).unwrap();
    let in_dark = render(&dark(), SIZE.0, SIZE.1).unwrap();
    assert_ne!(in_light.pixel(x, y), in_dark.pixel(x, y));
    assert_ne!(in_light, in_dark);
}

/// **A theme's own wallpaper is the desktop**: the picture it recommends,
/// decoded and drawn under everything.
#[test]
fn a_themes_wallpaper_is_drawn() {
    let scratch = ScratchDir::new("themepreview-wallpaper");
    let dirs = ThemeDirs {
        user: Some(scratch.dir().join("user")),
        system: scratch.dir().join("system"),
    };
    let dir = scratch.dir().join("user").join("pics");
    std::fs::create_dir_all(dir.join("wallpapers")).unwrap();
    let green = 0xFF20_A040_u32;
    let pixels = vec![green; 64 * 40];
    std::fs::write(
        dir.join("wallpapers/night.png"),
        imagecodec::encode_png(64, 40, &pixels).unwrap(),
    )
    .unwrap();
    std::fs::write(
        dir.join("theme.yaml"),
        "wallpapers:\n  dark: wallpapers/night.png\n",
    )
    .unwrap();
    let settings = AppearanceSettings {
        wallpaper_theme: WallpaperTheme::load_from(&dirs, std::ffi::OsStr::new("pics")),
        ..dark()
    };
    let scene = Scene::new(&settings, SIZE.0, SIZE.1).unwrap();
    assert!(scene.wallpaper.is_some(), "the picture was not decoded");
    let picture = scene.compose(&settings).unwrap();
    let (x, y) = open_desktop();
    assert_eq!(rgb(picture.pixel(x, y).unwrap()), 0x0020_A040);
}
