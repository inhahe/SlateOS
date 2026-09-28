// A test that panics on bad data is a test reporting a fault.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_cmp
)]

use super::*;
use appearance::{AccentColor, AppearanceSettings, ThemeMode};

/// The palette for `accent` in the light or the dark mode.
fn palette(accent: AccentColor, light: bool) -> Palette {
    Palette::from_settings(&AppearanceSettings {
        accent_color: accent,
        theme_mode: if light {
            ThemeMode::Light
        } else {
            ThemeMode::Dark
        },
        ..AppearanceSettings::default()
    })
}

/// Every role is the palette's -- an entry, its ink, or an entry made
/// translucent -- in a light theme as in a dark one.
#[test]
fn the_chrome_is_the_palettes_in_either_mode() {
    for light in [false, true] {
        let p = Palette::for_mode(light);
        let c = Chrome::of(&p);
        appearance::palette_check::assert_colours_from(
            &p,
            &[
                ("page", c.page),
                ("band", c.band),
                ("well", c.well),
                ("raised", c.raised),
                ("lit", c.lit),
                ("high", c.high),
                ("text", c.text),
                ("dim", c.dim),
                ("off", c.off),
                ("good", c.good),
                ("bad", c.bad),
                ("even", c.even),
                ("title", c.title),
                ("ring", c.ring),
                ("key", c.key),
                ("scrim", c.scrim),
                ("veil", c.veil),
            ],
            &[],
            "gamechrome",
        );
    }
    assert_ne!(
        Chrome::of(&Palette::for_mode(false)).page,
        Chrome::of(&Palette::for_mode(true)).page,
        "the page did not follow the mode"
    );
}

/// **The second side is never mistaken for the first**, whatever accent the
/// user picks, in either mode.
#[test]
fn the_second_side_is_never_mistaken_for_the_accent() {
    for &accent in AccentColor::presets() {
        for light in [false, true] {
            let p = palette(accent, light);
            let second = apart_from_accent(&p);
            assert!(
                !hard_to_tell_apart(second, p.ink(p.accent)),
                "{accent:?} (light: {light}): the second side looks like the first"
            );
        }
    }
    // A red accent does not get a red second side.
    let p = palette(AccentColor::Red, false);
    assert_ne!(apart_from_accent(&p), p.ink(p.red));
}

/// A button is the toolkit's colours for its kind and state, with the game's
/// label size -- and a label too long for it is cut, not run over the edge.
#[test]
fn a_button_is_the_toolkits_paint_at_the_games_size() {
    let p = Palette::for_mode(false);
    let ground = p.crust;
    for state in [
        State::default(),
        State {
            disabled: true,
            ..State::default()
        },
        State {
            hovered: true,
            ..State::default()
        },
    ] {
        let mut cmds: Vec<RenderCommand> = Vec::new();
        button(
            &mut cmds,
            &p,
            (10.0, 20.0, 120.0, 30.0),
            "New game",
            18.0,
            Kind::Plain,
            state,
            ground,
        );
        let paint = tk_button::paint(&p, Kind::Plain, state, ground);
        assert!(
            cmds.iter().any(|c| matches!(
                c,
                RenderCommand::FillRect { color, .. } if *color == paint.lower
            )),
            "{state:?}: the face is not the toolkit's"
        );
        let label = cmds
            .iter()
            .find_map(|c| match c {
                RenderCommand::Text {
                    text,
                    color,
                    font_size,
                    max_width,
                    ..
                } => Some((text.clone(), *color, *font_size, *max_width)),
                _ => None,
            })
            .expect("a label");
        assert_eq!(label.0, "New game");
        assert_eq!(label.1, paint.ink, "{state:?}: the label's ink");
        assert_eq!(label.2, 18.0, "the game's size, not the toolkit's 13");
        assert!(
            label.3.is_some_and(|w| w <= 120.0),
            "the label may run over the edge"
        );
    }
}

/// The keyboard's ring is drawn round a focused button, in the accent, and
/// not round a disabled one.
#[test]
fn a_focused_button_has_the_accents_ring() {
    let p = Palette::for_mode(true);
    let ring = |state: State| {
        let mut cmds: Vec<RenderCommand> = Vec::new();
        button(
            &mut cmds,
            &p,
            (0.0, 0.0, 90.0, 28.0),
            "Help",
            13.0,
            Kind::Plain,
            state,
            p.base,
        );
        cmds.iter().any(|c| {
            matches!(
                c,
                RenderCommand::StrokeRect { color, .. } if *color == p.accent
            )
        })
    };
    let focused = State {
        focused: true,
        ..State::default()
    };
    assert!(ring(focused));
    assert!(!ring(State::default()));
    assert!(!ring(State {
        disabled: true,
        ..focused
    }));
}

/// A colour a player reads is drawn in the shade that reads on its tile: the
/// dark theme's on a dark tile, the light theme's on a light one.
#[test]
fn a_read_colour_takes_the_shade_that_reads_on_its_tile() {
    let yellow = (Color::from_hex(0xF9E2AF), Color::from_hex(0xDF8E1D));
    let dark_tile = Palette::for_mode(false).mantle;
    let light_tile = Palette::for_mode(true).mantle;
    assert_eq!(legible_on(yellow, dark_tile), yellow.0);
    assert_eq!(legible_on(yellow, light_tile), yellow.1);
    assert!(
        contrast_ratio(legible_on(yellow, light_tile), light_tile)
            > contrast_ratio(yellow.0, light_tile)
    );
}

/// A button as wide as [`button_width`] says shows its label whole: the
/// label's room is at least the label.
#[test]
fn a_button_as_wide_as_it_asks_shows_its_label_whole() {
    let p = Palette::for_mode(false);
    for (label, size, h) in [
        ("New game", 18.0, 30.0),
        ("Undo", 11.0, 16.0),
        ("W", 30.0, 44.0),
    ] {
        let w = button_width(label, size, h);
        let mut cmds: Vec<RenderCommand> = Vec::new();
        button(
            &mut cmds,
            &p,
            (0.0, 0.0, w, h),
            label,
            size,
            Kind::Plain,
            State::default(),
            p.base,
        );
        let room = cmds
            .iter()
            .find_map(|c| match c {
                RenderCommand::Text { max_width, .. } => *max_width,
                _ => None,
            })
            .unwrap_or_else(|| panic!("{label:?} is not drawn"));
        let needs = text::measure(label, size, FontWeightHint::Bold);
        assert!(
            room >= needs - 0.01,
            "{label:?} needs {needs} and has {room}"
        );
        // A pixel narrower and it is cut: the width is what it needs, not more.
        let mut narrow: Vec<RenderCommand> = Vec::new();
        button(
            &mut narrow,
            &p,
            (0.0, 0.0, w - 1.0, h),
            label,
            size,
            Kind::Plain,
            State::default(),
            p.base,
        );
        let cut = narrow
            .iter()
            .find_map(|c| match c {
                RenderCommand::Text { max_width, .. } => *max_width,
                _ => None,
            })
            .unwrap_or(0.0);
        assert!(
            cut < needs,
            "{label:?}: a narrower button still has room for it all"
        );
    }
}

/// A button's label is centred in it, across and down -- down on the height
/// of its line, not on its font size, which is the smaller number and leaves
/// a label a sixth of a line low.
#[test]
fn a_buttons_label_is_centred_on_its_line() {
    let p = Palette::for_mode(false);
    let (x, y, w, h) = (10.0, 20.0, 160.0, 40.0);
    let mut cmds: Vec<RenderCommand> = Vec::new();
    button(
        &mut cmds,
        &p,
        (x, y, w, h),
        "Check",
        15.0,
        Kind::Plain,
        State::default(),
        p.base,
    );
    let (tx, ty) = cmds
        .iter()
        .find_map(|c| match c {
            RenderCommand::Text { x, y, .. } => Some((*x, *y)),
            _ => None,
        })
        .expect("a label");
    let wide = text::measure("Check", 15.0, FontWeightHint::Bold);
    let line = text::line_height(15.0, FontWeightHint::Bold);
    assert!(
        (tx + wide / 2.0 - (x + w / 2.0)).abs() < 0.01,
        "the label is off centre across"
    );
    assert!(
        (ty + line / 2.0 - (y + h / 2.0)).abs() < 0.01,
        "the label is off centre down"
    );
}

/// A label taller than its button is left out, not drawn over the button's
/// edges; the face is still drawn, because it is still the control.
#[test]
fn a_label_taller_than_its_button_is_left_out() {
    let p = Palette::for_mode(false);
    let mut cmds: Vec<RenderCommand> = Vec::new();
    button(
        &mut cmds,
        &p,
        (0.0, 0.0, 120.0, 8.0),
        "Check",
        13.0,
        Kind::Plain,
        State::default(),
        p.base,
    );
    assert!(
        !cmds.iter().any(|c| matches!(c, RenderCommand::Text { .. })),
        "a 13px label was drawn in an 8px button"
    );
    assert!(
        cmds.iter()
            .any(|c| matches!(c, RenderCommand::FillRect { .. })),
        "the face went with the label"
    );
}

/// An ink is moved only when it does not read, and then only as far as the
/// floor for its strength: a colour that reads is kept exactly, one between
/// the two floors is kept for large text and moved for small, and every ink
/// reads on every ground at its floor afterwards, whichever way the grounds
/// lie.
#[test]
fn an_ink_is_moved_only_as_far_as_it_must_be_to_read() {
    let (dark, grey) = (Color::from_hex(0x303030), Color::from_hex(0x606060));
    let white = Color::from_hex(0xFFFFFF);
    assert_eq!(
        Ink::on(white, &[dark, grey]),
        Ink {
            large: white,
            small: white
        },
        "an ink that reads was moved"
    );
    // Between the floors on the dark ground.
    let between = Color::from_hex(0x858585);
    let ratio = contrast_ratio(between, dark);
    assert!((3.0..4.5).contains(&ratio), "the fixture is {ratio:.2}:1");
    let ink = Ink::on(between, &[dark]);
    assert_eq!(ink.large, between, "large text that reads was moved");
    assert_ne!(ink.small, between, "small text that does not read was kept");
    // Moved only as far as it must: just over the floor, not far past it.
    let dim = Color::from_hex(0x505050);
    let ink = Ink::on(dim, &[dark, grey]);
    for ground in [dark, grey] {
        let large = contrast_ratio(ink.large, ground);
        let small = contrast_ratio(ink.small, ground);
        assert!(large >= 3.0, "large text is {large:.2}:1");
        assert!(small >= 4.5, "small text is {small:.2}:1");
    }
    assert!(
        contrast_ratio(ink.large, grey) < 3.2,
        "large text was moved past its floor: {:.2}:1",
        contrast_ratio(ink.large, grey)
    );
    // On light grounds it goes the other way.
    let (pale, paler) = (Color::from_hex(0xC0C0C0), Color::from_hex(0xE0E0E0));
    let ink = Ink::on(Color::from_hex(0xA0A0A0), &[pale, paler]);
    for ground in [pale, paler] {
        assert!(contrast_ratio(ink.small, ground) >= 4.5);
    }
}

/// A text is large at 24px, or at 18.66px bold, and takes that strength.
#[test]
fn an_ink_is_picked_by_the_size_it_is_drawn_at() {
    let ink = Ink {
        large: Color::from_hex(0x111111),
        small: Color::from_hex(0x222222),
    };
    assert_eq!(ink.at(24.0, false), ink.large);
    assert_eq!(ink.at(23.9, false), ink.small);
    assert_eq!(ink.at(18.66, true), ink.large);
    assert_eq!(ink.at(18.6, true), ink.small);
}

/// Nothing is drawn for a button with no room.
#[test]
fn a_button_with_no_room_draws_nothing() {
    let p = Palette::for_mode(false);
    let mut cmds: Vec<RenderCommand> = Vec::new();
    button(
        &mut cmds,
        &p,
        (0.0, 0.0, 0.0, 28.0),
        "X",
        13.0,
        Kind::Plain,
        State::default(),
        p.base,
    );
    assert!(cmds.is_empty());
}

/// A board's two squares follow the theme -- a dark theme's board is dark --
/// stay clearly apart in either, and are named the right way round.
#[test]
fn a_boards_squares_follow_the_theme_and_stay_apart() {
    for light in [false, true] {
        let p = Palette::for_mode(light);
        let (pale, deep) = squares(&p);
        assert!(
            relative_luminance(pale) > relative_luminance(deep),
            "the light square is the darker (light: {light})"
        );
        let apart = contrast_ratio(pale, deep);
        assert!(
            apart >= 1.5,
            "the squares are {apart:.2}:1 apart (light: {light})"
        );
        appearance::palette_check::assert_colours_from(
            &p,
            &[("light square", pale), ("dark square", deep)],
            &[],
            "gamechrome squares",
        );
    }
    let dark = squares(&Palette::for_mode(false));
    let light = squares(&Palette::for_mode(true));
    assert!(
        relative_luminance(dark.0) < relative_luminance(light.1),
        "a dark theme's board is not darker than a light theme's"
    );
}

/// **A piece is seen on every square in either theme**: a black disc on a
/// dark theme's squares and a white one on a light theme's are ringed in a
/// rim that stands off the square, and a piece that stands off by itself
/// keeps its own edge.
#[test]
fn a_piece_is_seen_on_every_square_in_either_theme() {
    let black = (Color::from_hex(0x1A1A2E), Color::from_hex(0x000000));
    let white = (Color::from_hex(0xE8E8E8), Color::from_hex(0xBBBBBB));
    for light in [false, true] {
        let (pale, deep) = squares(&Palette::for_mode(light));
        for ground in [pale, deep] {
            for (piece, own) in [black, white] {
                let edge = edge_on(own, piece, ground);
                let seen = contrast_ratio(piece, ground).max(contrast_ratio(edge, ground));
                assert!(
                    seen >= 3.0,
                    "{piece:?} on {ground:?} is seen at {seen:.2}:1 (light: {light})"
                );
                if contrast_ratio(piece, ground) >= 3.0 {
                    assert_eq!(edge, own, "a piece that stands off lost its own edge");
                }
            }
        }
    }
    // Both ways round: the dark theme rings the black disc, the light theme
    // the white one.
    let (_, dark_square) = squares(&Palette::for_mode(false));
    assert_eq!(edge_on(black.1, black.0, dark_square), RIMS.0);
    let (light_square, _) = squares(&Palette::for_mode(true));
    assert_eq!(edge_on(white.1, white.0, light_square), RIMS.1);
}

/// A card's suits read on its face: the pale pink the red suits were drawn in
/// was 1.6:1 on a lavender face.
#[test]
fn a_cards_suits_read_on_its_face() {
    for red in [false, true] {
        let ratio = contrast_ratio(cards::suit_ink(red), cards::FACE);
        assert!(
            ratio >= 4.5,
            "red: {red}: the suit is {ratio:.2}:1 on the face"
        );
    }
}

/// **A card table is the palette's in either theme**, and a card is seen on
/// its felt face up or face down, and ringed so it can be seen where it is not;
/// the keyboard's ring stands off the felt and is not the back's colour.
#[test]
fn a_card_table_follows_the_theme_and_every_card_is_seen_on_it() {
    for light in [false, true] {
        for accent in [AccentColor::Blue, AccentColor::Yellow, AccentColor::Red] {
            let p = palette(accent, light);
            let t = cards::Table::of(&p);
            appearance::palette_check::assert_colours_from(
                &p,
                &[
                    ("felt", t.felt),
                    ("back", t.back),
                    ("pattern", t.pattern),
                    ("focus", t.focus),
                    ("picked", t.picked),
                    ("empty", t.empty),
                    ("empty edge", t.empty_edge),
                ],
                &[],
                "gamechrome cards",
            );
            for (what, body, edge) in [
                ("face", cards::FACE, t.face_edge()),
                ("back", t.back, t.back_edge()),
            ] {
                let seen = contrast_ratio(body, t.felt).max(contrast_ratio(edge, t.felt));
                assert!(
                    seen >= 3.0,
                    "a card's {what} is seen at {seen:.2}:1 ({accent:?}, light: {light})"
                );
            }
            for (what, ring) in [("focus", t.focus), ("picked", t.picked)] {
                let ratio = contrast_ratio(ring, t.felt);
                assert!(
                    ratio >= 3.0,
                    "the {what} ring is {ratio:.2}:1 on the felt ({accent:?}, light: {light})"
                );
            }
            // The keyboard's ring is told apart from a back, and from a card's
            // rim: neither the accent nor a grey.
            assert!(
                !hard_to_tell_apart(t.focus, t.back),
                "the focus ring is the back's colour ({accent:?}, light: {light})"
            );
            assert!(
                !hard_to_tell_apart(t.focus, t.face_edge()),
                "the focus ring is the rim's colour ({accent:?}, light: {light})"
            );
        }
    }
}
