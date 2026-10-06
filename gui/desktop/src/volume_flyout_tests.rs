#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_cmp
)]

use guitk::event::{Key, Modifiers};

use super::*;

fn open() -> VolumeFlyout {
    let mut f = VolumeFlyout::new();
    f.set_visible(true);
    f
}

fn layout() -> Layout {
    Layout::new((100.0, 200.0), 1.0)
}

/// Every piece of text drawn.
fn texts(cmds: &[RenderCommand]) -> Vec<String> {
    cmds.iter()
        .filter_map(|c| match c {
            RenderCommand::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

/// **Everything is inside the panel**: the slider, the level right of it,
/// the switch at the right end of the row below -- at any scale.
#[test]
fn everything_is_inside_the_panel() {
    for scale in [1.0, 1.5, 2.0] {
        let l = Layout::new((10.0, 20.0), scale);
        assert_eq!(
            l.panel,
            Rect::new(10.0, 20.0, WIDTH * scale, HEIGHT * scale)
        );
        let inside = |r: Rect| {
            r.x >= l.panel.x
                && r.y >= l.panel.y
                && r.x + r.w <= l.panel.x + l.panel.w + 0.01
                && r.y + r.h <= l.panel.y + l.panel.h + 0.01
        };
        assert!(inside(l.slider.hit()), "{scale}: {:?}", l.slider.hit());
        assert!(inside(l.level), "{scale}");
        assert!(inside(l.mute), "{scale}");
        assert!(inside(l.settings), "{scale}: {:?}", l.settings);
        assert!(
            l.settings.y >= l.mute.y + l.mute.h,
            "the settings row below the switch"
        );
        assert!(
            l.level.x >= l.slider.track.x + l.slider.track.w,
            "the level right of it"
        );
        assert!(l.mute.y > l.slider.track.y, "the switch below");
    }
}

/// **A closed flyout draws nothing and takes nothing.**
#[test]
fn a_closed_flyout_draws_and_takes_nothing() {
    let mut f = VolumeFlyout::new();
    let p = Palette::for_mode(false);
    assert!(f.render(&p, &layout(), 40, false, None).is_empty());
    let mute = layout().mute;
    assert_eq!(
        f.press(&layout(), (mute.x + 2.0, mute.y + 2.0), 40, true),
        None
    );
}

/// **Open, it draws its panel, the caption, the level and the switch** --
/// "Muted" for the level while muted.
#[test]
fn open_it_draws_the_level_and_the_switch() {
    let f = open();
    let p = Palette::for_mode(false);
    let cmds = f.render(&p, &layout(), 45, false, None);
    assert!(
        matches!(cmds[1], RenderCommand::FillRect { .. }),
        "the panel first"
    );
    let t = texts(&cmds);
    assert_eq!(t, ["Volume", "45%", "Mute", SETTINGS_LABEL]);
    let muted = texts(&f.render(&p, &layout(), 45, true, None));
    assert_eq!(muted, ["Volume", "Muted", "Mute", SETTINGS_LABEL]);
}

/// **With the card out of reach it says why, and nothing in it moves.**
#[test]
fn out_of_reach_it_says_why_and_nothing_moves() {
    let mut f = open();
    let p = Palette::for_mode(false);
    let cmds = f.render(&p, &layout(), 45, false, Some("No sound card reachable"));
    assert_eq!(
        texts(&cmds),
        ["Volume", "No sound card reachable", SETTINGS_LABEL]
    );
    let fills = cmds
        .iter()
        .filter(|c| matches!(c, RenderCommand::FillRect { .. }))
        .count();
    assert_eq!(
        fills, 3,
        "the panel, the line and the link's underline: no slider, no switch"
    );
    let l = layout();
    let track = l.slider.track;
    assert_eq!(
        f.press(&l, (track.x + track.w, track.y + 1.0), 45, false),
        None
    );
    assert_eq!(
        f.press(&l, (l.mute.x + 2.0, l.mute.y + 2.0), 45, false),
        None
    );
}

/// **A press on the slider sets the level there; on the switch, mutes.**
#[test]
fn a_press_sets_the_level_or_mutes() {
    let mut f = open();
    let l = layout();
    let track = l.slider.track;
    let far = f.press(&l, (track.x + track.w, track.y + track.h / 2.0), 20, true);
    assert_eq!(far, Some(Action::Level(100)));
    let _ = f.release();
    let mid = f.press(
        &l,
        (track.x + track.w / 2.0, track.y + track.h / 2.0),
        100,
        true,
    );
    assert_eq!(mid, Some(Action::Level(50)));
    let _ = f.release();
    assert_eq!(
        f.press(&l, (l.mute.x + 2.0, l.mute.y + 2.0), 50, true),
        Some(Action::ToggleMute)
    );
    // The caption is no control.
    assert_eq!(
        f.press(&l, (l.panel.x + 20.0, l.panel.y + 18.0), 50, true),
        None
    );
}

/// **A drag moves the level as it goes, and a release settles it; closing
/// lets go.**
#[test]
fn a_drag_moves_the_level_and_closing_lets_go() {
    let mut f = open();
    let l = layout();
    let track = l.slider.track;
    let y = track.y + track.h / 2.0;
    // Onto the thumb, at 0: it is taken hold of without moving.
    assert_eq!(f.press(&l, (track.x, y), 0, true), None);
    assert!(f.dragging());
    assert_eq!(
        f.drag(&l, (track.x + track.w / 4.0, y)),
        Some(Action::Level(25))
    );
    assert_eq!(f.release(), Some(Action::Level(25)));
    assert!(!f.dragging());
    let _ = f.press(&l, (track.x + track.w, y), 25, true);
    assert!(f.dragging());
    f.set_visible(false);
    assert!(!f.dragging(), "closing let go of the drag");
}

/// **The keys move it as any slider's do.**
#[test]
fn the_keys_move_it() {
    let mut f = open();
    let key = |k| KeyEvent {
        key: k,
        pressed: true,
        modifiers: Modifiers::NONE,
        text: String::new(),
    };
    assert_eq!(f.key(&key(Key::Right), 40, true), Some(Action::Level(41)));
    assert_eq!(f.key(&key(Key::End), 41, true), Some(Action::Level(100)));
    assert_eq!(f.key(&key(Key::Home), 100, true), Some(Action::Level(0)));
    assert_eq!(f.key(&key(Key::Right), 40, false), None, "out of reach");
}

/// **"Audio settings…" is a link at the foot**: the link's colour,
/// underlined, below a line, inside its row -- and a press anywhere on the
/// row opens Settings, whether or not the card can be reached; closed, the
/// flyout takes no press there.
#[test]
fn audio_settings_is_a_link_at_the_foot() {
    let mut f = open();
    let p = Palette::for_mode(false);
    let l = layout();
    for out_of_reach in [None, Some("No sound card reachable")] {
        let cmds = f.render(&p, &l, 45, false, out_of_reach);
        let (label_y, color) = cmds
            .iter()
            .find_map(|c| match c {
                RenderCommand::Text { text, y, color, .. } if text == SETTINGS_LABEL => {
                    Some((*y, *color))
                }
                _ => None,
            })
            .expect("the link");
        assert_eq!(color, p.link);
        assert!(
            label_y >= l.settings.y && label_y < l.settings.y + l.settings.h,
            "{label_y} in {:?}",
            l.settings
        );
        let underlined = cmds.iter().any(|c| {
            matches!(c, RenderCommand::FillRect { y, width, color, .. }
                if *color == p.link && *y > label_y && *width > 0.0)
        });
        assert!(underlined, "underlined: {out_of_reach:?}");
        let line = cmds.iter().any(|c| {
            matches!(c, RenderCommand::FillRect { y, color, .. }
                if *color == p.surface1 && *y < l.settings.y && *y > l.mute.y + l.mute.h)
        });
        assert!(line, "a line above it: {out_of_reach:?}");
    }
    let (x, y) = (
        l.settings.x + l.settings.w - 2.0,
        l.settings.y + l.settings.h / 2.0,
    );
    assert_eq!(f.press(&l, (x, y), 45, true), Some(Action::OpenSettings));
    assert_eq!(f.press(&l, (x, y), 45, false), Some(Action::OpenSettings));
    f.set_visible(false);
    assert_eq!(f.press(&l, (x, y), 45, true), None);
}
