#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_cmp
)]

use super::super::token::tokenize;
use super::*;

fn len(text: &str, percent: bool) -> Result<Length, ValueError> {
    let tokens = tokenize(text);
    let mut c = Cursor::new(&tokens);
    let v = length(&mut c, percent)?;
    assert!(c.at_end(), "{text}: left over");
    Ok(v)
}

fn col(text: &str) -> Result<ColorValue, ValueError> {
    let tokens = tokenize(text);
    let mut c = Cursor::new(&tokens);
    let v = color(&mut c)?;
    assert!(c.at_end(), "{text}: left over");
    Ok(v)
}

fn rgba(r: u8, g: u8, b: u8, a: u8) -> ColorValue {
    ColorValue::Color(Color::rgba(r, g, b, a))
}

const UNITS: Units = Units {
    em: 10.0,
    rem: 16.0,
    ch: 8.0,
    viewport: (1000.0, 500.0),
    px_per_mm: 4.0,
};

/// **Each unit is worth what CSS says where it is used**: an em the font,
/// a rem the base size, a ch a `0`, vw and vh the window, mm the display,
/// a percentage what it is of.
#[test]
fn each_unit_is_worth_what_css_says() {
    let px = |t: &str| len(t, true).unwrap().resolve(&UNITS, 200.0);
    assert_eq!(px("3px"), 3.0);
    assert_eq!(px("2em"), 20.0);
    assert_eq!(px("1.5rem"), 24.0);
    assert_eq!(px("2ch"), 16.0);
    assert_eq!(px("10vw"), 100.0);
    assert_eq!(px("10vh"), 50.0);
    assert_eq!(px("2mm"), 8.0);
    assert_eq!(px("1cm"), 40.0);
    assert_eq!(px("25%"), 50.0);
    assert_eq!(px("0"), 0.0);
    assert_eq!(px("2PX"), 2.0, "units ignore case");
}

/// **A length needs a unit unless it is nought, and one this does not read
/// is refused** -- print's `pt` and `in` among them.
#[test]
fn a_length_needs_a_unit_it_knows() {
    assert!(len("12", true).is_err());
    assert!(len("3pt", true).is_err());
    assert!(len("1in", true).is_err());
    assert!(
        len("50%", false).is_err(),
        "a percentage where none is taken"
    );
    assert!(len("red", true).is_err());
}

/// **`calc()` keeps a sum of units until it is drawn** -- and multiplies and
/// divides by numbers, nested and parenthesised.
#[test]
fn calc_keeps_a_sum_of_units() {
    let v = len("calc(100% - 2em)", true).unwrap();
    assert_eq!(v.percent, 100.0);
    assert_eq!(v.em, -2.0);
    assert_eq!(v.resolve(&UNITS, 300.0), 280.0);
    assert_eq!(
        len("calc(2 * 3px)", true).unwrap().resolve(&UNITS, 0.0),
        6.0
    );
    assert_eq!(
        len("calc(3px * 2)", true).unwrap().resolve(&UNITS, 0.0),
        6.0
    );
    assert_eq!(
        len("calc(10px / 4)", true).unwrap().resolve(&UNITS, 0.0),
        2.5
    );
    assert_eq!(
        len("calc((1px + 2px) * 2 - calc(1em / 2))", true)
            .unwrap()
            .resolve(&UNITS, 0.0),
        1.0
    );
}

/// **A calculation CSS would refuse is refused**: a length times a length,
/// a division by a length or by nought, a number added to a length, a
/// minus with no spaces round it.
#[test]
fn a_calculation_css_refuses_is_refused() {
    for bad in [
        "calc(2px * 3px)",
        "calc(4px / 2px)",
        "calc(4px / 0)",
        "calc(1px + 2)",
        "calc(10px -2px)",
        "calc(1px",
        "calc()",
    ] {
        assert!(len(bad, true).is_err(), "{bad}");
    }
}

/// **A colour may be written every way CSS writes one.**
#[test]
fn a_colour_is_read_every_way_css_writes_one() {
    assert_eq!(col("#f80"), Ok(rgba(255, 136, 0, 255)));
    assert_eq!(col("#f808"), Ok(rgba(255, 136, 0, 136)));
    assert_eq!(col("#1a2B3c"), Ok(rgba(0x1a, 0x2b, 0x3c, 255)));
    assert_eq!(col("#1a2b3c80"), Ok(rgba(0x1a, 0x2b, 0x3c, 0x80)));
    assert_eq!(col("rgb(1, 2, 3)"), Ok(rgba(1, 2, 3, 255)));
    assert_eq!(col("rgba(1, 2, 3, 0.5)"), Ok(rgba(1, 2, 3, 128)));
    assert_eq!(col("rgb(1 2 3 / 50%)"), Ok(rgba(1, 2, 3, 128)));
    assert_eq!(col("rgb(100% 0% 50%)"), Ok(rgba(255, 0, 128, 255)));
    assert_eq!(col("hsl(0, 100%, 50%)"), Ok(rgba(255, 0, 0, 255)));
    assert_eq!(col("hsl(120deg 100% 25%)"), Ok(rgba(0, 128, 0, 255)));
    assert_eq!(col("hsla(240, 100%, 50%, 0.25)"), Ok(rgba(0, 0, 255, 64)));
    assert_eq!(col("hsl(0.5turn 100% 50%)"), Ok(rgba(0, 255, 255, 255)));
    assert_eq!(col("RebeccaPurple"), Ok(rgba(0x66, 0x33, 0x99, 255)));
    assert_eq!(col("transparent"), Ok(rgba(0, 0, 0, 0)));
    assert_eq!(col("currentColor"), Ok(ColorValue::CurrentColor));
}

/// **Channels past their range are held to it**, as CSS clamps them.
#[test]
fn channels_past_their_range_are_held_to_it() {
    assert_eq!(col("rgb(300, -5, 128)"), Ok(rgba(255, 0, 128, 255)));
    assert_eq!(col("rgba(0, 0, 0, 2)"), Ok(rgba(0, 0, 0, 255)));
}

/// **What is not a colour is said to be not one.**
#[test]
fn what_is_not_a_colour_is_refused() {
    for bad in [
        "#ff",
        "#fffff",
        "#ggg",
        "notacolour",
        "rgb(1, 2)",
        "rgb(1, 2, 3, 4, 5)",
        "rgb(1px, 2, 3)",
        "hsl(10px, 50%, 50%)",
        "rgb(1 2 3 / )",
        "lab(50 0 0)",
        "rgb(1, 2, 3",
    ] {
        assert!(col(bad).is_err(), "{bad}");
    }
}

/// **The named colours are sorted**, which the lookup's binary search needs,
/// and each is found by its name.
#[test]
fn the_named_colours_are_sorted_and_found() {
    for pair in NAMED.windows(2) {
        assert!(pair[0].0 < pair[1].0, "{} before {}", pair[0].0, pair[1].0);
    }
    for (name, _) in NAMED {
        assert!(named_color(name).is_some(), "{name}");
    }
}
