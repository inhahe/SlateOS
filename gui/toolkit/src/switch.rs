//! The on/off switch: a pill with a knob at the end that says which.
//!
//! # Why the toolkit has it
//!
//! It began in the desktop shell (`gui/desktop`), which had seventeen
//! hand-drawn switches and one correct answer between them, and moved here on
//! 2026-09-27 with its reasoning and its tests, as the slider did
//! ([`crate::slider`]): three applications draw switches of their own, and a
//! control a user learns once should be drawn once. [`switch`] is the shapes,
//! unchanged; [`draw`] is the control -- the light under the pointer, the
//! keyboard ring, the disabled look; [`hit`] and [`toggles`] are its input.
//!
//! # The knob is derived from the track
//!
//! All seventeen of the desktop's hand-drawn switches made the same mistake:
//! the knob -- the little circle that tells you *which side the switch is
//! on* -- was filled with the palette's `text`. On the "off" track
//! (`surface2`) that is fine. On the "on" track it is the user's accent, and
//! the ordinary text colour on a pale accent is a light grey on a light blue:
//! **1.35:1** on the stock dark theme, against the 4.5:1 ordinary text is
//! expected to reach. The one part of the control that carries the state was
//! the one part you could not see.
//!
//! The knob is [`readable_on`] of whatever the track is. It is *derived* from
//! the track rather than chosen beside it, so a track colour that changes -- a
//! new accent, a panel that uses green for "on" -- drags the knob with it. A
//! fill and the ink on top of it are one decision. (A slider's thumb takes the
//! opposite rule, and for a reason: it overhangs its track, where a switch's
//! knob is contained by it. See [`crate::slider`].)
//!
//! # The geometry is not a new opinion
//!
//! Every one of the seventeen hand-written switches obeyed the same rule
//! without writing it down: the knob is a circle inset [`INSET`] from every
//! edge of the track, so its diameter is `height - 2 * INSET`, and the
//! track's corner radius is `height / 2`. That held at 40x20, 40x22, 36x20 and
//! 36x18 alike, and [`switch`] takes the track's size as arguments so every
//! existing caller draws the pixels it did before.

use crate::color::Color;
use crate::disabled::DISABLED_OPACITY;
use crate::event::{Key, KeyEvent};
use crate::frame::Rect;
use crate::grab;
use crate::palette::{Palette, emphasized, readable_on};
use crate::render::RenderCommand;
use crate::style::CornerRadii;
use crate::surface::CommandSink;

/// The gap between the knob and each edge of the track.
///
/// Not a preference: it is the value all seventeen hand-written switches
/// already used, recovered by measuring them.
pub const INSET: f32 = 2.0;

/// A switch's width where the caller has no layout of its own to fit.
pub const WIDTH: f32 = 40.0;

/// A switch's height where the caller has no layout of its own to fit.
pub const HEIGHT: f32 = 20.0;

/// Draw an on/off switch's shapes: the track, then the knob that sits on it.
///
/// `track` is the colour of the pill, which the caller chooses because panels
/// disagree about it on purpose -- most use the accent for "on", but a panel
/// where "on" means *safe* rather than *selected* uses green. The knob's
/// colour is not a choice: it is [`readable_on`] the track, so it stays
/// visible whatever the caller picked.
///
/// The two commands are returned in painting order, track first. A `height`
/// smaller than `2 * INSET` yields a knob of no size, which the renderer draws
/// as nothing.
#[must_use]
pub fn switch(
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    on: bool,
    track: Color,
) -> [RenderCommand; 2] {
    let knob = height - INSET * 2.0;
    let knob_x = if on {
        x + width - knob - INSET
    } else {
        x + INSET
    };
    [
        RenderCommand::FillRect {
            x,
            y,
            width,
            height,
            color: track,
            corner_radii: CornerRadii::all(height / 2.0),
        },
        RenderCommand::FillRect {
            x: knob_x,
            y: y + INSET,
            width: knob,
            height: knob,
            color: readable_on(track),
            corner_radii: CornerRadii::all(knob / 2.0),
        },
    ]
}

/// The two track colours of a switch: what it is when on, and when off.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Look {
    /// The track when the switch is on.
    pub on: Color,
    /// The track when it is off -- a surface role.
    pub off: Color,
}

impl Look {
    /// On is the accent, off is `surface2`: a switch that *selects* something.
    #[must_use]
    pub const fn accent(p: &Palette) -> Self {
        Self {
            on: p.accent,
            off: p.surface2,
        }
    }

    /// On is green, off is `surface1`: a switch where on means *safe*, as the
    /// desktop's power, sound and storage pages use.
    #[must_use]
    pub const fn safe(p: &Palette) -> Self {
        Self {
            on: p.green,
            off: p.surface1,
        }
    }
}

/// What is happening to a switch now, which the host knows and the switch
/// does not.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct State {
    /// The pointer is over it.
    pub hovered: bool,
    /// The keyboard is on it: a ring round it in the accent.
    pub focused: bool,
    /// It cannot be changed now: drawn at the toolkit's disabled opacity,
    /// with no light and no ring.
    pub disabled: bool,
}

/// Draw a switch as a control at `rect`.
///
/// The track is `look.on` or `look.off`, lit a step ([`emphasized`]) while the
/// pointer is over it -- and the knob, being derived from the track, follows
/// the lit colour. A focused switch has a ring outside its pill, `focus_ring`
/// wide, in the accent; a disabled one is drawn at the toolkit's disabled
/// opacity and nothing else.
pub fn draw(
    sink: &mut impl CommandSink,
    p: &Palette,
    rect: Rect,
    on: bool,
    look: Look,
    state: State,
    focus_ring: f32,
) {
    let base = if on { look.on } else { look.off };
    let track = if state.hovered && !state.disabled {
        emphasized(base)
    } else {
        base
    };
    for cmd in switch(rect.x, rect.y, rect.w, rect.h, on, track) {
        if state.disabled {
            sink.emit(dimmed(cmd));
        } else {
            sink.emit(cmd);
        }
    }
    if state.focused && !state.disabled && focus_ring > 0.0 && focus_ring.is_finite() {
        // Outside the pill, so a thicker ring is still a ring round it rather
        // than a band across it.
        sink.emit(RenderCommand::StrokeRect {
            x: rect.x - focus_ring,
            y: rect.y - focus_ring,
            width: rect.w + focus_ring * 2.0,
            height: rect.h + focus_ring * 2.0,
            color: p.accent,
            line_width: focus_ring,
            corner_radii: CornerRadii::all(rect.h / 2.0 + focus_ring),
        });
    }
}

/// `cmd` at the toolkit's disabled opacity.
fn dimmed(cmd: RenderCommand) -> RenderCommand {
    match cmd {
        RenderCommand::FillRect {
            x,
            y,
            width,
            height,
            color,
            corner_radii,
        } => {
            let a = (f32::from(color.a) * DISABLED_OPACITY).round();
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "rounded, and between 0 and 255 because both factors are"
            )]
            let a = a as u8;
            RenderCommand::FillRect {
                x,
                y,
                width,
                height,
                color: Color::rgba(color.r, color.g, color.b, a),
                corner_radii,
            }
        }
        other => other,
    }
}

/// The region a press takes hold of a switch drawn at `rect` in: the pill
/// grown as [`grab::handle`] grows a handle, so a 40-by-20 switch answers over
/// 48 by 28.
#[must_use]
pub fn hit(rect: Rect) -> Rect {
    grab::handle(rect)
}

/// Whether `key` flips a switch that has the keyboard: Space or Enter,
/// pressed, with no Ctrl, Alt or Super (those are the program's shortcuts).
#[must_use]
pub fn toggles(key: &KeyEvent) -> bool {
    let m = key.modifiers;
    key.pressed && !(m.ctrl || m.alt || m.super_key) && matches!(key.key, Key::Space | Key::Enter)
}

#[cfg(test)]
mod tests {
    // A helper handed the wrong command shape has nothing useful to return,
    // and a search over a render this test just built cannot legitimately
    // come up empty: in both cases the panic *is* the failure report.
    #![allow(clippy::panic, clippy::expect_used, clippy::indexing_slicing)]

    use super::*;
    use crate::event::Modifiers;
    use crate::palette::{DARK_EXTREME, LIGHT_EXTREME};
    use crate::theme::contrast_ratio;

    fn rect(c: &RenderCommand) -> (f32, f32, f32, f32, Color, f32) {
        match c {
            RenderCommand::FillRect {
                x,
                y,
                width,
                height,
                color,
                corner_radii,
            } => (*x, *y, *width, *height, *color, corner_radii.top_left),
            other => panic!("expected a FillRect, got {other:?}"),
        }
    }

    /// Every hue the palette can be asked to make its accent.
    fn accents(p: &Palette) -> [(&'static str, Color); 14] {
        [
            ("blue", p.blue),
            ("green", p.green),
            ("red", p.red),
            ("yellow", p.yellow),
            ("peach", p.peach),
            ("lavender", p.lavender),
            ("mauve", p.mauve),
            ("sapphire", p.sapphire),
            ("teal", p.teal),
            ("sky", p.sky),
            ("pink", p.pink),
            ("rosewater", p.rosewater),
            ("flamingo", p.flamingo),
            ("maroon", p.maroon),
        ]
    }

    fn key(k: Key) -> KeyEvent {
        KeyEvent {
            key: k,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        }
    }

    /// The pixels the seventeen hand-written switches drew, recovered from
    /// them before they were replaced. If this module's geometry rule were
    /// wrong, the conversion would have moved something on screen.
    #[test]
    fn the_geometry_is_the_one_every_hand_written_switch_already_used() {
        for (w, h, side) in [
            (40.0, 20.0, 16.0),
            (40.0, 22.0, 18.0),
            (36.0, 20.0, 16.0),
            (36.0, 18.0, 14.0),
        ] {
            for on in [false, true] {
                let cmds = switch(100.0, 50.0, w, h, on, Color::from_hex(0x0080_8080));
                let (tx, ty, tw, th, _, tr) = rect(&cmds[0]);
                assert_eq!((tx, ty, tw, th), (100.0, 50.0, w, h));
                assert!(
                    (tr - h / 2.0).abs() < f32::EPSILON,
                    "a {w}x{h} track's radius is {tr}"
                );
                let (kx, ky, kw, kh, _, kr) = rect(&cmds[1]);
                assert_eq!((kw, kh), (side, side), "a {w}x{h} switch's knob");
                assert!((kr - side / 2.0).abs() < f32::EPSILON, "the knob is round");
                assert!((ky - (50.0 + INSET)).abs() < f32::EPSILON);
                let expected_x = if on {
                    100.0 + w - side - INSET
                } else {
                    100.0 + INSET
                };
                assert!(
                    (kx - expected_x).abs() < f32::EPSILON,
                    "knob at {kx} with on={on}"
                );
            }
        }
    }

    /// The knob moves, and it moves to the side that says what the state is.
    #[test]
    fn the_knob_is_at_the_right_end_when_on_and_the_left_end_when_off() {
        let bg = Color::from_hex(0x0080_8080);
        let (off_x, ..) = rect(&switch(0.0, 0.0, 40.0, 20.0, false, bg)[1]);
        let (on_x, ..) = rect(&switch(0.0, 0.0, 40.0, 20.0, true, bg)[1]);
        assert!(
            on_x > off_x,
            "the on knob ({on_x}) is right of the off knob ({off_x})"
        );
        assert!((off_x - INSET).abs() < f32::EPSILON, "off knob at {off_x}");
        assert!(
            (on_x - (40.0 - 16.0 - INSET)).abs() < f32::EPSILON,
            "on knob at {on_x}"
        );
    }

    /// On every palette role, the knob is the more legible of the two inks
    /// [`readable_on`] chooses between. (4.5:1 cannot be asked on every role:
    /// on a mid grey neither extreme reaches it. What can be asked is that the
    /// choice between the two is the right way round.)
    #[test]
    fn the_knob_is_the_more_legible_of_the_two_inks_on_offer() {
        for light in [false, true] {
            let p = Palette::for_mode(light);
            for (name, track) in p.roles() {
                let (.., ink, _) = rect(&switch(0.0, 0.0, 40.0, 20.0, true, track)[1]);
                let other = if ink == DARK_EXTREME {
                    LIGHT_EXTREME
                } else {
                    DARK_EXTREME
                };
                let (chosen, rejected) = (contrast_ratio(track, ink), contrast_ratio(track, other));
                assert!(
                    chosen >= rejected,
                    "on `{name}` in {} mode the knob took {chosen:.2}:1 when the other ink was \
                     worth {rejected:.2}:1",
                    if light { "light" } else { "dark" }
                );
            }
        }
    }

    /// The defect this module exists to remove, as a floor over the tracks a
    /// panel can actually choose: every accent the palette offers, green, and
    /// the three surfaces. `text` on the stock dark accent reached 1.35:1.
    #[test]
    fn the_knob_is_legible_on_every_track_a_panel_can_choose() {
        let mut worst: Option<(String, f32)> = None;
        for light in [false, true] {
            let p = Palette::for_mode(light);
            let mode = if light { "light" } else { "dark" };
            let mut tracks = vec![
                ("green", p.green),
                ("surface0", p.surface0),
                ("surface1", p.surface1),
                ("surface2", p.surface2),
            ];
            tracks.extend(accents(&p));
            for (name, track) in tracks {
                let c = contrast_ratio(track, readable_on(track));
                if worst.as_ref().is_none_or(|(_, w)| c < *w) {
                    worst = Some((format!("`{name}` in {mode} mode"), c));
                }
                let (.., ink, _) = rect(&switch(0.0, 0.0, 40.0, 20.0, true, track)[1]);
                assert_eq!(
                    ink,
                    readable_on(track),
                    "the knob on `{name}` in {mode} mode"
                );
            }
        }
        let (where_, c) = worst.expect("there are tracks");
        assert!(c >= 4.5, "the tightest knob is on {where_}, at {c:.2}:1");
    }

    /// The knob follows the track: change the track and the knob changes
    /// without anyone editing a second line.
    #[test]
    fn the_knob_follows_the_track_rather_than_the_theme() {
        let pale = Color::from_hex(0x00F5_E0DC);
        let deep = Color::from_hex(0x0011_1B2B);
        let (.., pale_ink, _) = rect(&switch(0.0, 0.0, 40.0, 20.0, true, pale)[1]);
        let (.., deep_ink, _) = rect(&switch(0.0, 0.0, 40.0, 20.0, true, deep)[1]);
        assert_ne!(
            pale_ink, deep_ink,
            "the same ink on a pale and a deep track"
        );
        assert!(contrast_ratio(pale, pale_ink) >= 4.5);
        assert!(contrast_ratio(deep, deep_ink) >= 4.5);
    }

    /// As a control: the track is the look's colour for the state, lit under
    /// the pointer (with the knob following the lit track), ringed when
    /// focused, and dimmed with nothing else when disabled.
    #[test]
    fn the_control_lights_rings_and_dims() {
        let p = Palette::for_mode(false);
        let r = Rect::new(10.0, 10.0, WIDTH, HEIGHT);
        let look = Look::accent(&p);

        let mut plain = Vec::new();
        draw(&mut plain, &p, r, true, look, State::default(), 2.0);
        assert_eq!(plain.len(), 2);
        assert_eq!(rect(&plain[0]).4, p.accent);
        let mut off = Vec::new();
        draw(&mut off, &p, r, false, look, State::default(), 2.0);
        assert_eq!(rect(&off[0]).4, p.surface2);

        let mut lit = Vec::new();
        let hovered = State {
            hovered: true,
            ..State::default()
        };
        draw(&mut lit, &p, r, true, look, hovered, 2.0);
        let track = rect(&lit[0]).4;
        assert_eq!(track, emphasized(p.accent), "the pointer lights the track");
        assert_eq!(
            rect(&lit[1]).4,
            readable_on(track),
            "and the knob follows it"
        );

        let mut ringed = Vec::new();
        let focused = State {
            focused: true,
            ..State::default()
        };
        draw(&mut ringed, &p, r, true, look, focused, 2.0);
        assert!(
            matches!(ringed.last(), Some(RenderCommand::StrokeRect { color, x, .. })
                if *color == p.accent && (*x - 8.0).abs() < f32::EPSILON),
            "a focused switch is ringed outside its pill: {ringed:?}"
        );

        let mut dim = Vec::new();
        let disabled = State {
            disabled: true,
            focused: true,
            hovered: true,
        };
        draw(&mut dim, &p, r, true, look, disabled, 2.0);
        assert_eq!(dim.len(), 2, "no ring on a disabled switch: {dim:?}");
        assert_eq!(rect(&dim[0]).4.a, 128, "dimmed to the disabled opacity");
        assert_eq!(
            (rect(&dim[0]).4.r, rect(&dim[0]).4.g),
            (p.accent.r, p.accent.g),
            "and not lit"
        );
    }

    /// A 40-by-20 switch is taken hold of over 48 by 28, centred on it.
    #[test]
    fn the_switch_is_a_generous_target() {
        let h = hit(Rect::new(100.0, 50.0, WIDTH, HEIGHT));
        assert!(
            h.w >= WIDTH + grab::HANDLE_MARGIN * 2.0 && h.h >= grab::MIN_TARGET,
            "{h:?}"
        );
        assert!((h.x + h.w / 2.0 - 120.0).abs() < 1e-4 && (h.y + h.h / 2.0 - 60.0).abs() < 1e-4);
    }

    /// Space and Enter flip a focused switch; shortcuts and releases do not.
    #[test]
    fn space_and_enter_flip_it_and_nothing_else_does() {
        assert!(toggles(&key(Key::Space)));
        assert!(toggles(&key(Key::Enter)));
        assert!(!toggles(&key(Key::Tab)));
        let mut released = key(Key::Space);
        released.pressed = false;
        assert!(!toggles(&released));
        let mut ctrl = key(Key::Enter);
        ctrl.modifiers.ctrl = true;
        assert!(!toggles(&ctrl));
    }
}
