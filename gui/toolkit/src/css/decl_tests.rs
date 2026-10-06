#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_cmp
)]

use super::super::token::tokenize;
use super::*;
use crate::color::Color;

fn get(name: &str, value: &str) -> Result<Vec<(Property, Value)>, ValueError> {
    read(name, &tokenize(value))
}

fn one(name: &str, value: &str) -> Value {
    let mut all = get(name, value).unwrap_or_else(|e| panic!("{name}: {value}: {e}"));
    assert_eq!(all.len(), 1, "{name}: {value}");
    all.remove(0).1
}

fn px(v: f32) -> Value {
    Value::Length(Length::px(v))
}

fn color(r: u8, g: u8, b: u8) -> Value {
    Value::Color(ColorValue::Color(Color::rgba(r, g, b, 255)))
}

/// **A longhand is read into its property.**
#[test]
fn a_longhand_is_read_into_its_property() {
    assert_eq!(
        get("color", "red"),
        Ok(vec![(Property::Color, color(255, 0, 0))])
    );
    assert_eq!(one("width", "120px"), px(120.0));
    assert_eq!(one("width", "auto"), Value::Auto);
    assert_eq!(one("max-height", "none"), Value::None);
    assert_eq!(one("min-width", "auto"), Value::Length(Length::default()));
    assert_eq!(one("opacity", "0.5"), Value::Number(0.5));
    assert_eq!(one("opacity", "40%"), Value::Number(0.4));
    assert_eq!(one("opacity", "3"), Value::Number(1.0), "held to 0..1");
    assert_eq!(
        one("text-align", "center"),
        Value::TextAlign(TextAlign::Center)
    );
    assert_eq!(
        one("cursor", "pointer"),
        Value::Cursor(PointerCursor::Pointer)
    );
    assert_eq!(one("border-top-left-radius", "4px"), px(4.0));
    assert_eq!(one("margin-left", "-2px"), px(-2.0));
}

/// **Margins and padding take one to four values, as CSS gives them to the
/// sides.**
#[test]
fn sides_take_one_to_four_values() {
    let sides = |v: &str| -> Vec<f32> {
        get("padding", v)
            .unwrap()
            .into_iter()
            .map(|(_, v)| match v {
                Value::Length(l) => l.px,
                other => panic!("{other:?}"),
            })
            .collect()
    };
    assert_eq!(sides("1px"), [1.0, 1.0, 1.0, 1.0]);
    assert_eq!(sides("1px 2px"), [1.0, 2.0, 1.0, 2.0]);
    assert_eq!(sides("1px 2px 3px"), [1.0, 2.0, 3.0, 2.0]);
    assert_eq!(sides("1px 2px 3px 4px"), [1.0, 2.0, 3.0, 4.0]);
    assert!(get("padding", "1px 2px 3px 4px 5px").is_err());
    assert!(
        get("padding", "-1px").is_err(),
        "padding may not be negative"
    );
    assert!(
        get("margin", "auto").is_err(),
        "auto margins are not laid out"
    );
}

/// **`border` sets each side's width, style and colour, in any order**; what
/// it leaves out is CSS's initial value -- so a border with no style is none.
#[test]
fn border_sets_width_style_and_colour() {
    let all = get("border", "solid 2px #00f").unwrap();
    assert_eq!(all.len(), 12);
    assert!(all.contains(&(Property::BorderWidth(Side::Left), px(2.0))));
    assert!(all.contains(&(
        Property::BorderStyle(Side::Top),
        Value::BorderStyle(BorderStyle::Solid)
    )));
    assert!(all.contains(&(Property::BorderColor(Side::Bottom), color(0, 0, 255))));
    let top = get("border-top", "1px red").unwrap();
    assert_eq!(top.len(), 3);
    assert!(top.contains(&(
        Property::BorderStyle(Side::Top),
        Value::BorderStyle(BorderStyle::None)
    )));
    assert!(get("border", "1px 2px solid").is_err(), "two widths");
    assert!(get("border", "solid wobbly").is_err());
    assert_eq!(one("border-left-width", "thick"), px(5.0));
    assert_eq!(
        one("border-right-style", "dashed"),
        Value::BorderStyle(BorderStyle::Solid),
        "drawn solid"
    );
}

/// **`border-radius` gives its radii from the top left, clockwise**; the
/// elliptical form is refused.
#[test]
fn border_radius_goes_clockwise() {
    let all = get("border-radius", "1px 2px 3px").unwrap();
    assert_eq!(
        all,
        [
            (Property::BorderRadius(Corner::TopLeft), px(1.0)),
            (Property::BorderRadius(Corner::TopRight), px(2.0)),
            (Property::BorderRadius(Corner::BottomRight), px(3.0)),
            (Property::BorderRadius(Corner::BottomLeft), px(2.0)),
        ]
    );
    assert!(get("border-radius", "4px / 2px").is_err());
}

/// **Font families are read as CSS lists them**: quoted, unquoted names of
/// several words, the generic families.
#[test]
fn font_families_are_read_as_listed() {
    assert_eq!(
        one(
            "font-family",
            "\"Noto Sans\", Open Sans, monospace, system-ui"
        ),
        Value::FontFamily(vec![
            Family::Named("Noto Sans".into()),
            Family::Named("Open Sans".into()),
            Family::Mono,
            Family::Ui,
        ])
    );
    assert!(get("font-family", "12px").is_err());
}

/// **A font size is a length, a share of the parent's, or a keyword**; a
/// percentage is a share of an em.
#[test]
fn a_font_size_is_read() {
    assert_eq!(one("font-size", "14px"), px(14.0));
    assert_eq!(
        one("font-size", "150%"),
        Value::Length(Length {
            em: 1.5,
            ..Length::default()
        })
    );
    assert_eq!(
        one("font-size", "large"),
        Value::Length(Length {
            rem: 1.2,
            ..Length::default()
        })
    );
    assert_eq!(
        one("font-size", "larger"),
        Value::Length(Length {
            em: 1.2,
            ..Length::default()
        })
    );
    assert!(get("font-size", "-1px").is_err());
}

/// **A weight is 1 to 1000, or a keyword.**
#[test]
fn a_weight_is_read() {
    assert_eq!(one("font-weight", "bold"), Value::FontWeight(700));
    assert_eq!(one("font-weight", "normal"), Value::FontWeight(400));
    assert_eq!(one("font-weight", "650"), Value::FontWeight(650));
    assert_eq!(one("font-weight", "bolder"), Value::Bolder);
    assert!(get("font-weight", "0").is_err());
    assert!(get("font-weight", "1001").is_err());
}

/// **A line height is `normal`, a multiple, or a length**, a percentage a
/// multiple.
#[test]
fn a_line_height_is_read() {
    assert_eq!(
        one("line-height", "normal"),
        Value::LineHeight(LineHeight::Normal)
    );
    assert_eq!(
        one("line-height", "1.5"),
        Value::LineHeight(LineHeight::Multiple(1.5))
    );
    assert_eq!(
        one("line-height", "120%"),
        Value::LineHeight(LineHeight::Multiple(1.2))
    );
    assert_eq!(
        one("line-height", "20px"),
        Value::LineHeight(LineHeight::Length(Length::px(20.0)))
    );
}

/// **`font` sets the weight, size, line height and family together.**
#[test]
fn font_sets_four_properties() {
    let all = get("font", "bold 16px/1.25 \"Noto Serif\", serif").unwrap();
    assert_eq!(
        all,
        [
            (Property::FontWeight, Value::FontWeight(700)),
            (Property::FontSize, px(16.0)),
            (
                Property::LineHeight,
                Value::LineHeight(LineHeight::Multiple(1.25))
            ),
            (
                Property::FontFamily,
                Value::FontFamily(vec![Family::Named("Noto Serif".into()), Family::Ui])
            ),
        ]
    );
    assert!(
        get("font", "italic 12px serif").is_err(),
        "italic is not drawn yet"
    );
    assert!(
        get("font", "bold").is_err(),
        "a size and a family are needed"
    );
}

/// **A shadow is two to four lengths and a colour, in either order**; a
/// list, an inset and a negative blur are refused.
#[test]
fn a_shadow_is_read() {
    let Value::Shadow(s) = one("box-shadow", "1px 2px 3px 4px rgba(0, 0, 0, 0.5)") else {
        panic!("not a shadow");
    };
    assert_eq!(
        (s.x.px, s.y.px, s.blur.px, s.spread.px),
        (1.0, 2.0, 3.0, 4.0)
    );
    assert_eq!(s.color, ColorValue::Color(Color::rgba(0, 0, 0, 128)));
    let Value::Shadow(t) = one("text-shadow", "red 1px 1px") else {
        panic!("not a shadow");
    };
    assert_eq!(t.color, ColorValue::Color(Color::rgba(255, 0, 0, 255)));
    assert_eq!(one("box-shadow", "none"), Value::None);
    assert!(
        get("text-shadow", "1px 1px 1px 1px").is_err(),
        "text has no spread"
    );
    assert!(get("box-shadow", "1px 1px, 2px 2px").is_err());
    assert!(get("box-shadow", "inset 1px 1px").is_err());
    assert!(get("box-shadow", "1px 1px -2px").is_err());
    assert!(get("box-shadow", "1px").is_err());
}

/// **`inherit` and `initial` are taken by every property**, and by every
/// property a shorthand sets.
#[test]
fn every_property_takes_inherit_and_initial() {
    assert_eq!(one("color", "inherit"), Value::Inherit);
    let all = get("margin", "initial").unwrap();
    assert_eq!(all.len(), 4);
    assert!(all.iter().all(|(_, v)| *v == Value::Initial));
}

/// **What is not a property, or more than its value, is refused** -- with a
/// word for what is not read yet.
#[test]
fn what_is_not_read_is_refused() {
    assert!(get("colour", "red").unwrap_err().contains("not a property"));
    assert!(
        get("position", "absolute")
            .unwrap_err()
            .contains("not read yet")
    );
    assert!(
        get("color", "red blue")
            .unwrap_err()
            .contains("more than its value")
    );
    assert!(get("color", "").is_err());
}

/// **A declaration that names a variable waits for it**, kept as written; a
/// custom property is kept as written for `var()` to substitute.
#[test]
fn a_variable_waits_and_a_custom_property_is_kept() {
    let pending = declare("Color", &tokenize(" var(--accent) ")).unwrap();
    assert!(matches!(
        &pending[..],
        [Declared::Pending { name, tokens }] if name == "color" && tokens.len() == 3
    ));
    let custom = declare("--gap", &tokenize(" 4px ")).unwrap();
    assert!(matches!(
        &custom[..],
        [Declared::Custom { name, tokens }] if name == "--gap" && tokens.len() == 1
    ));
    assert!(
        declare("colour", &tokenize("var(--a)")).is_err(),
        "known even before it is read"
    );
    assert_eq!(
        declare("color", &tokenize("blue")).unwrap(),
        [Declared::Value(Property::Color, color(0, 0, 255))]
    );
}

/// **The inherited properties are CSS's**: text and font, not boxes.
#[test]
fn the_inherited_properties_are_css_s() {
    assert!(Property::Color.inherited());
    assert!(Property::FontSize.inherited());
    assert!(!Property::BackgroundColor.inherited());
    assert!(!Property::Padding(Side::Top).inherited());
    assert!(!Property::Width.inherited());
    assert!(!Property::TransitionDuration.inherited());
}

fn transitions(value: &str) -> Vec<(Property, Value)> {
    get("transition", value).unwrap_or_else(|e| panic!("{value}: {e}"))
}

/// **The `transition` shorthand is read into its four lists**, a time first
/// the duration and second the delay, in any order with the property and the
/// timing function; what one leaves out is the initial value.
#[test]
fn transition_is_read_into_four_lists() {
    let padding: Vec<Property> = Side::ALL.into_iter().map(Property::Padding).collect();
    assert_eq!(
        transitions("background-color 200ms, ease-in 1s padding -50ms"),
        vec![
            (
                Property::TransitionProperty,
                Value::Transitions(vec![
                    TransitionTarget::Properties(vec![Property::BackgroundColor]),
                    TransitionTarget::Properties(padding),
                ])
            ),
            (
                Property::TransitionDuration,
                Value::Times(vec![200.0, 1000.0])
            ),
            (
                Property::TransitionTimingFunction,
                Value::Timings(vec![Timing::Desktop, Timing::EASE_IN])
            ),
            (Property::TransitionDelay, Value::Times(vec![0.0, -50.0])),
        ]
    );
    let all = transitions("0.5s");
    assert_eq!(
        all[0].1,
        Value::Transitions(vec![TransitionTarget::All]),
        "all, unsaid"
    );
    assert_eq!(all[1].1, Value::Times(vec![500.0]));
    let none = transitions("none");
    assert_eq!(none[0].1, Value::Transitions(Vec::new()));
    assert!(
        get("transition", "none, color 1s").is_err(),
        "none in a list"
    );
    assert!(get("transition", "color 1s 2s 3s").is_err(), "three times");
    assert!(
        get("transition", "color -1s").is_err(),
        "a negative duration"
    );
    assert!(get("transition", "color 1s,").is_err(), "an empty one");
    assert!(get("transition", "color 1").is_err(), "a time with no unit");
}

/// **The longhands take lists**, and a name this does not read is kept so the
/// lists still line up.
#[test]
fn the_transition_longhands_take_lists() {
    assert_eq!(
        one("transition-property", "opacity, colour, all"),
        Value::Transitions(vec![
            TransitionTarget::Properties(vec![Property::Opacity]),
            TransitionTarget::Unknown("colour".into()),
            TransitionTarget::All,
        ])
    );
    assert_eq!(
        one("transition-property", "none"),
        Value::Transitions(Vec::new())
    );
    assert!(get("transition-property", "color, none").is_err());
    assert_eq!(
        one("transition-duration", "1s, 250ms"),
        Value::Times(vec![1000.0, 250.0])
    );
    assert!(get("transition-duration", "-1s").is_err());
    assert_eq!(
        one("transition-delay", "-250ms"),
        Value::Times(vec![-250.0])
    );
}

/// **Every timing function is read**: the keywords, `cubic-bezier()` with
/// its x held to 0 to 1, and `steps()` with its positions.
#[test]
fn timing_functions_are_read() {
    let timing = |text: &str| match one("transition-timing-function", text) {
        Value::Timings(t) => t,
        other => panic!("{text}: {other:?}"),
    };
    assert_eq!(
        timing("linear, ease, ease-in-out, step-start, step-end"),
        vec![
            Timing::Linear,
            Timing::EASE,
            Timing::EASE_IN_OUT,
            Timing::Steps(1, StepPosition::JumpStart),
            Timing::Steps(1, StepPosition::JumpEnd),
        ]
    );
    assert_eq!(
        timing("cubic-bezier(0.1, -0.5, 0.9, 1.5)"),
        vec![Timing::CubicBezier(0.1, -0.5, 0.9, 1.5)]
    );
    assert_eq!(
        timing("steps(4), steps(3, jump-both), steps(2, jump-none), steps(5, start)"),
        vec![
            Timing::Steps(4, StepPosition::JumpEnd),
            Timing::Steps(3, StepPosition::JumpBoth),
            Timing::Steps(2, StepPosition::JumpNone),
            Timing::Steps(5, StepPosition::JumpStart),
        ]
    );
    for bad in [
        "cubic-bezier(1.5, 0, 0, 1)",
        "cubic-bezier(0, 0, 1)",
        "steps(0)",
        "steps(1.5)",
        "steps(1, jump-none)",
        "steps(2, sideways)",
        "bounce",
    ] {
        assert!(
            get("transition-timing-function", bad).is_err(),
            "{bad} was read"
        );
    }
}
