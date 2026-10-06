#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_cmp,
    clippy::arithmetic_side_effects
)]

use super::*;
use crate::css::value::{Length, Units};
use crate::motion::Curve;
use crate::style::Edges;

const RED: Color = Color::rgba(255, 0, 0, 255);
const BLUE: Color = Color::rgba(0, 0, 255, 255);
const GREEN: Color = Color::rgba(0, 255, 0, 255);

/// The desktop's motion at the standard speed, moving at a constant one --
/// so a test's numbers are its own and not a curve's.
fn linear() -> Motion {
    Motion::new(Motion::STANDARD_MS, Curve::Linear)
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-3
}

// ---------------------------------------------------------------------------
// Timing
// ---------------------------------------------------------------------------

/// **Each timing function runs from 0 to 1**, and `ease` is CSS's: about
/// 0.8 of the way at half the time.
#[test]
fn every_timing_runs_from_nought_to_one() {
    let m = linear();
    for t in [
        Timing::Linear,
        Timing::EASE,
        Timing::EASE_IN,
        Timing::EASE_OUT,
        Timing::EASE_IN_OUT,
        Timing::Desktop,
        Timing::Steps(3, StepPosition::JumpEnd),
    ] {
        assert!(close(t.ease(0.0, m), 0.0), "{t:?} at 0");
        assert!(close(t.ease(1.0, m), 1.0), "{t:?} at 1");
    }
    assert!(close(Timing::Linear.ease(0.3, m), 0.3));
    assert!(close(Timing::EASE.ease(0.5, m), 0.802_4), "ease at half");
    assert!(close(Timing::EASE_IN_OUT.ease(0.5, m), 0.5), "symmetric");
    assert!(Timing::EASE_IN.ease(0.5, m) < 0.5 && Timing::EASE_OUT.ease(0.5, m) > 0.5);
    // A bezier whose y passes 1 passes it.
    assert!(Timing::CubicBezier(0.3, 1.6, 0.7, 1.6).ease(0.5, m) > 1.0);
    // Held to 0..=1, and a NaN is the end.
    assert!(close(Timing::Linear.ease(2.0, m), 1.0));
    assert!(close(Timing::Linear.ease(f32::NAN, m), 1.0));
}

/// **The desktop's curve is the user's**: ease-out at the standard, and at
/// its end at once with animations off.
#[test]
fn the_desktops_curve_is_the_users() {
    let ease_out = Motion::new(Motion::STANDARD_MS, Curve::EaseOut);
    assert!(close(Timing::Desktop.ease(0.5, ease_out), 0.875));
    assert!(close(Timing::Desktop.ease(0.5, linear()), 0.5));
    assert!(close(Timing::Desktop.ease(0.1, Motion::STILL), 1.0));
}

/// **Steps jump where their position says.**
#[test]
fn steps_jump_where_they_say() {
    let m = linear();
    let at = |n, p, t| Timing::Steps(n, p).ease(t, m);
    // Four steps, jumping at each step's end.
    assert!(close(at(4, StepPosition::JumpEnd, 0.0), 0.0));
    assert!(close(at(4, StepPosition::JumpEnd, 0.49), 0.25));
    assert!(close(at(4, StepPosition::JumpEnd, 0.99), 0.75));
    // At each step's start.
    assert!(close(at(4, StepPosition::JumpStart, 0.0), 0.25));
    assert!(close(at(4, StepPosition::JumpStart, 0.99), 1.0));
    // At neither end: three jumps between four values.
    assert!(close(at(4, StepPosition::JumpNone, 0.0), 0.0));
    assert!(close(at(4, StepPosition::JumpNone, 0.3), 1.0 / 3.0));
    assert!(close(at(4, StepPosition::JumpNone, 0.99), 1.0));
    // At both: five jumps.
    assert!(close(at(4, StepPosition::JumpBoth, 0.0), 0.2));
    assert!(close(at(4, StepPosition::JumpBoth, 0.99), 0.8));
    assert!(close(at(4, StepPosition::JumpBoth, 1.0), 1.0));
}

// ---------------------------------------------------------------------------
// What a style says moves
// ---------------------------------------------------------------------------

fn spec_of(properties: Vec<TransitionTarget>, durations: Vec<f32>) -> TransitionSpec {
    TransitionSpec {
        properties,
        durations,
        timings: vec![Timing::Linear],
        delays: vec![0.0],
    }
}

fn named(p: Property) -> TransitionTarget {
    TransitionTarget::Properties(vec![p])
}

/// **The last entry naming a value is its**, `all` names every one, a name
/// this does not read names none, and the other lists repeat to the
/// properties' length.
#[test]
fn a_value_moves_as_the_last_entry_naming_it_says() {
    let spec = spec_of(
        vec![
            TransitionTarget::All,
            named(Property::Color),
            TransitionTarget::Unknown("colour".into()),
            named(Property::Opacity),
        ],
        vec![100.0, 300.0],
    );
    let ms = |a| spec.how(a).map(|h| h.duration_ms);
    assert_eq!(ms(Animated::Background), Some(100.0), "all's");
    assert_eq!(ms(Animated::Color), Some(300.0), "its own, after all");
    assert_eq!(ms(Animated::Opacity), Some(300.0), "the durations repeat");
    assert!(spec.moves_anything());
    let none = spec_of(Vec::new(), vec![100.0]);
    assert_eq!(none.how(Animated::Color), None);
    assert!(!none.moves_anything());
    assert!(!TransitionSpec::default().moves_anything(), "all 0s");
    assert_eq!(
        TransitionSpec::default()
            .how(Animated::Color)
            .map(|h| h.timing),
        Some(Timing::Desktop)
    );
}

// ---------------------------------------------------------------------------
// Values part-way
// ---------------------------------------------------------------------------

/// **Colours mix weighted by their alpha**: red fading out stays red, not
/// darkening toward the transparent one's black; and a mix past the end is
/// held to the channels' range.
#[test]
fn colours_mix_as_css_mixes_them() {
    let clear = Color::rgba(0, 0, 0, 0);
    assert_eq!(mix(RED, clear, 0.5), Color::rgba(255, 0, 0, 128));
    assert_eq!(
        mix(
            Color::rgba(0, 0, 0, 255),
            Color::rgba(255, 255, 255, 255),
            0.5
        ),
        Color::rgba(128, 128, 128, 255)
    );
    assert_eq!(mix(RED, BLUE, 0.0), RED);
    assert_eq!(mix(RED, BLUE, 1.0), BLUE);
    assert_eq!(mix(RED, BLUE, 1.5), Color::rgba(0, 0, 255, 255), "held");
    assert_eq!(mix(clear, clear, 0.5), clear);
}

/// **Only values with a number either side move**: a size that was `auto`
/// or a text colour the theme chose has no way part-way, and changes at
/// once; a shadow from none fades in from a transparent one.
#[test]
fn only_values_with_a_way_between_them_move() {
    assert_eq!(
        Part::MaybeNumber(None).toward(Part::MaybeNumber(Some(10.0)), 0.5),
        None
    );
    assert_eq!(
        Part::MaybeColor(None).toward(Part::MaybeColor(Some(RED)), 0.5),
        None
    );
    assert_eq!(
        Part::Number(10.0).toward(Part::Number(20.0), 0.25),
        Some(Part::Number(12.5))
    );
    let shadow = BoxShadow {
        offset_x: 2.0,
        offset_y: 4.0,
        blur: 6.0,
        spread: 0.0,
        color: RED,
    };
    let Some(Part::Shadow(Some(half))) = Part::Shadow(None).toward(Part::Shadow(Some(shadow)), 0.5)
    else {
        panic!("a shadow from none moves");
    };
    assert_eq!((half.offset_x, half.offset_y, half.blur), (2.0, 4.0, 6.0));
    assert_eq!(half.color, Color::rgba(255, 0, 0, 128));
    assert_eq!(Part::Shadow(None).toward(Part::Shadow(None), 0.5), None);
}

/// **A value is written as far as it can go**: no padding below nought, no
/// opacity past one -- a spring passes its end, a value may not.
#[test]
fn a_value_is_held_where_it_is_written() {
    let mut style = Style::default();
    Animated::Padding(Side::Left).write(&mut style, Part::Number(-3.0));
    assert_eq!(style.padding.left, 0.0);
    Animated::Margin(Side::Left).write(&mut style, Part::Number(-3.0));
    assert_eq!(style.margin.left, -3.0, "a margin may be negative");
    Animated::Opacity.write(&mut style, Part::Number(1.2));
    assert_eq!(style.opacity, 1.0);
    // A part of another kind changes nothing.
    Animated::Opacity.write(&mut style, Part::Color(RED));
    assert_eq!(style.opacity, 1.0);
}

/// **Every value that can move is its own**: each written with a value of
/// its own, all of them read back as written -- so no two write one field.
#[test]
fn every_value_is_written_and_read_as_itself() {
    let mut style = Style::default();
    let all: Vec<Animated> = Animated::all().collect();
    assert_eq!(all.len(), 37);
    let written: Vec<Part> = all
        .iter()
        .enumerate()
        .map(|(i, what)| {
            let part = what.read(&style).numbered(u8::try_from(i + 1).unwrap());
            what.write(&mut style, part);
            part
        })
        .collect();
    for (what, part) in all.iter().zip(&written) {
        assert_eq!(what.read(&style), *part, "{what:?}");
    }
}

impl Part {
    /// A value of its kind numbered `n`: a number `n` sixty-fourths (under 1,
    /// so an opacity takes it as it is), a colour whose red is `n`.
    fn numbered(self, n: u8) -> Self {
        let v = f32::from(n) / 64.0;
        let color = Color::rgba(n, 1, 2, 255);
        match self {
            Self::Color(_) => Self::Color(color),
            Self::MaybeColor(_) => Self::MaybeColor(Some(color)),
            Self::Number(_) => Self::Number(v),
            Self::MaybeNumber(_) => Self::MaybeNumber(Some(v)),
            Self::Shadow(_) => Self::Shadow(Some(BoxShadow {
                offset_x: v,
                offset_y: v,
                blur: v,
                spread: v,
                color,
            })),
        }
    }
}

// ---------------------------------------------------------------------------
// A widget's transitions
// ---------------------------------------------------------------------------

fn background(c: Color) -> Style {
    Style {
        background: c,
        ..Style::default()
    }
}

fn moves_background(ms: f32, delay: f32) -> TransitionSpec {
    TransitionSpec {
        properties: vec![named(Property::BackgroundColor)],
        durations: vec![ms],
        timings: vec![Timing::Linear],
        delays: vec![delay],
    }
}

/// What `t` shows of `target` at `now`.
fn shown(t: &mut Transitions, target: &Style, spec: &TransitionSpec, now: f64, m: Motion) -> Color {
    let mut style = target.clone();
    t.update(&mut style, spec, &BoxLengths::default(), now, m);
    style.background
}

/// **A changed value moves from where it was to where it is going over its
/// time**, and is there, and still, at the end.
#[test]
fn a_change_moves_over_its_time() {
    let spec = moves_background(100.0, 0.0);
    let mut t = Transitions::from_shown(&background(RED));
    let blue = background(BLUE);
    assert_eq!(shown(&mut t, &blue, &spec, 0.0, linear()), RED, "from red");
    assert!(t.is_moving() && t.moves(Animated::Background));
    assert_eq!(
        shown(&mut t, &blue, &spec, 50.0, linear()),
        mix(RED, BLUE, 0.5)
    );
    assert_eq!(shown(&mut t, &blue, &spec, 100.0, linear()), BLUE);
    assert!(!t.is_moving(), "over");
}

/// **The first style changes nothing** -- there is nothing it changed from.
#[test]
fn the_first_style_changes_nothing() {
    let mut t = Transitions::default();
    let spec = moves_background(100.0, 0.0);
    assert_eq!(shown(&mut t, &background(BLUE), &spec, 0.0, linear()), BLUE);
    assert!(!t.is_moving());
}

/// **A delay holds the value where it was, and a negative one starts
/// part-way.**
#[test]
fn a_delay_waits_and_a_negative_one_starts_part_way() {
    let blue = background(BLUE);
    let mut t = Transitions::from_shown(&background(RED));
    let spec = moves_background(100.0, 50.0);
    assert_eq!(shown(&mut t, &blue, &spec, 0.0, linear()), RED);
    assert_eq!(shown(&mut t, &blue, &spec, 40.0, linear()), RED, "waiting");
    assert_eq!(
        shown(&mut t, &blue, &spec, 100.0, linear()),
        mix(RED, BLUE, 0.5)
    );
    let mut t = Transitions::from_shown(&background(RED));
    let spec = moves_background(100.0, -50.0);
    assert_eq!(
        shown(&mut t, &blue, &spec, 0.0, linear()),
        mix(RED, BLUE, 0.5),
        "half-way at once"
    );
}

/// **The user's motion sets the pace**: a slower standard takes longer, and
/// with animations off a change is shown at once -- and what was moving
/// stops.
#[test]
fn the_users_motion_sets_the_pace() {
    let blue = background(BLUE);
    let spec = moves_background(100.0, 0.0);
    let slow = Motion::new(Motion::STANDARD_MS * 2, Curve::Linear);
    let mut t = Transitions::from_shown(&background(RED));
    let _ = shown(&mut t, &blue, &spec, 0.0, slow);
    assert_eq!(
        shown(&mut t, &blue, &spec, 100.0, slow),
        mix(RED, BLUE, 0.5),
        "twice as long"
    );
    assert_eq!(shown(&mut t, &blue, &spec, 100.0, Motion::STILL), BLUE);
    assert!(!t.is_moving(), "stopped");
    let mut t = Transitions::from_shown(&background(RED));
    assert_eq!(shown(&mut t, &blue, &spec, 0.0, Motion::STILL), BLUE);
    assert!(!t.is_moving());
}

/// **It is the new style's `transition` that counts**, as in CSS: one that
/// names nothing stops what was moving; one whose time comes to nothing --
/// the initial `all 0s` -- lets what runs toward the same value run on, and
/// shows a change at once.
#[test]
fn it_is_the_new_styles_transition_that_counts() {
    let moving = moves_background(100.0, 0.0);
    let none = spec_of(Vec::new(), vec![100.0]);
    let mut t = Transitions::from_shown(&background(RED));
    let _ = shown(&mut t, &background(BLUE), &moving, 0.0, linear());
    assert!(t.is_moving());
    assert_eq!(
        shown(&mut t, &background(BLUE), &none, 50.0, linear()),
        BLUE
    );
    assert!(!t.is_moving(), "stopped");

    let instant = TransitionSpec::default();
    let mut t = Transitions::from_shown(&background(RED));
    let _ = shown(&mut t, &background(BLUE), &moving, 0.0, linear());
    assert_eq!(
        shown(&mut t, &background(BLUE), &instant, 50.0, linear()),
        mix(RED, BLUE, 0.5),
        "running on toward the same"
    );
    assert_eq!(
        shown(&mut t, &background(GREEN), &instant, 60.0, linear()),
        GREEN,
        "a change, at once"
    );
    assert!(!t.is_moving());
}

/// **Turning back part-way is CSS's reversal**: back toward where it
/// started, from where it is, taking as much of its time as the way back.
#[test]
fn turning_back_takes_as_long_as_the_way_back() {
    let spec = moves_background(100.0, 0.0);
    let (red, blue) = (background(RED), background(BLUE));
    let mut t = Transitions::from_shown(&red);
    let _ = shown(&mut t, &blue, &spec, 0.0, linear());
    let half = mix(RED, BLUE, 0.5);
    assert_eq!(shown(&mut t, &blue, &spec, 50.0, linear()), half);
    // Back to red at half-way: from half-way, over half the time.
    assert_eq!(shown(&mut t, &red, &spec, 50.0, linear()), half);
    assert_eq!(
        shown(&mut t, &red, &spec, 75.0, linear()),
        mix(half, RED, 0.5)
    );
    assert_eq!(shown(&mut t, &red, &spec, 100.0, linear()), RED);
    assert!(!t.is_moving(), "over in half the time");
}

/// **A change elsewhere part-way starts afresh** from where it is, over the
/// whole of its time.
#[test]
fn a_new_destination_part_way_starts_from_where_it_is() {
    let spec = moves_background(100.0, 0.0);
    let mut t = Transitions::from_shown(&background(RED));
    let _ = shown(&mut t, &background(BLUE), &spec, 0.0, linear());
    let half = mix(RED, BLUE, 0.5);
    assert_eq!(
        shown(&mut t, &background(GREEN), &spec, 50.0, linear()),
        half
    );
    assert_eq!(
        shown(&mut t, &background(GREEN), &spec, 100.0, linear()),
        mix(half, GREEN, 0.5)
    );
    assert_eq!(
        shown(&mut t, &background(GREEN), &spec, 150.0, linear()),
        GREEN
    );
}

/// **A length that is a percentage of its container moves when the
/// container settles it**; one that is not moves when computed, and the
/// settling does not move it twice.
#[test]
fn a_percentage_moves_when_its_container_settles_it() {
    let units = Units {
        em: 13.0,
        rem: 13.0,
        ch: 6.5,
        viewport: (800.0, 600.0),
        px_per_mm: 3.78,
    };
    let percent = |p: f32| Length {
        percent: p,
        ..Length::default()
    };
    let lengths = |left: Length| BoxLengths {
        padding: [None, None, None, Some(left)],
        margin: [Some(Length::px(4.0)), None, None, None],
        units: Some(units),
        ..BoxLengths::default()
    };
    let spec = TransitionSpec {
        properties: vec![TransitionTarget::All],
        durations: vec![100.0],
        timings: vec![Timing::Linear],
        delays: vec![0.0],
    };
    let settle = |t: &mut Transitions, lengths: &BoxLengths, width: f32| {
        let mut style = Style {
            margin: Edges::all(4.0),
            ..Style::default()
        };
        t.update(&mut style, &spec, lengths, t.now_ms, linear());
        lengths.apply(&mut style, width, 100.0);
        t.settle(&mut style, &spec, lengths);
        style
    };
    let mut t = Transitions::from_shown(&Style {
        margin: Edges::all(4.0),
        ..Style::default()
    });
    t.now_ms = 0.0;
    let ten = lengths(percent(10.0));
    let s = settle(&mut t, &ten, 200.0);
    assert_eq!(s.padding.left, 0.0, "from nought, the shown style's");
    assert!(t.moves(Animated::Padding(Side::Left)));
    assert!(!t.moves(Animated::Margin(Side::Top)), "unchanged");
    t.now_ms = 50.0;
    let s = settle(&mut t, &ten, 200.0);
    assert_eq!(s.padding.left, 10.0, "half-way to 10% of 200");
    assert_eq!(s.margin.top, 4.0);
    t.now_ms = 100.0;
    assert_eq!(settle(&mut t, &ten, 200.0).padding.left, 20.0);
    assert!(!t.is_moving());
}
