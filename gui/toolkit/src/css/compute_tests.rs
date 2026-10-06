#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_cmp,
    clippy::arithmetic_side_effects
)]

use super::super::decl::declare;
use super::super::token::tokenize;
use super::*;
use crate::style::Edges;

/// `text`'s declarations: `name: value` pairs split on `;` -- a test's own
/// reading, ahead of the style sheet's.
fn decls(text: &str) -> Vec<Declared> {
    text.split(';')
        .filter(|d| !d.trim().is_empty())
        .flat_map(|d| {
            let (name, value) = d.split_once(':').expect("a declaration");
            declare(name.trim(), &tokenize(value)).unwrap_or_else(|e| panic!("{d}: {e}"))
        })
        .collect()
}

fn env(palette: &Palette) -> Env<'_> {
    Env {
        root_font_size: 16.0,
        viewport: (800.0, 600.0),
        px_per_mm: 4.0,
        palette,
        // A `0` is half the size, and in a fixed-pitch family the whole of it.
        zero_width: &|size, _, family| {
            if family.is_some_and(|f| f.first() == Some(&Family::Mono)) {
                size
            } else {
                size / 2.0
            }
        },
    }
}

/// `text` computed over `base`, under a parent styled `parent_style` that
/// left `parent`.
fn computed(text: &str, base: &Style, parent_style: &Style, parent: &Inherited) -> Computed {
    let palette = Palette::for_mode(false);
    let all = decls(text);
    let refs: Vec<&Declared> = all.iter().collect();
    compute(base, &refs, parent_style, parent, &env(&palette))
}

fn top(text: &str) -> Computed {
    computed(
        text,
        &Style::default(),
        &Style::default(),
        &Inherited::default(),
    )
}

/// **A declaration sets what it names over the program's style; what it
/// does not name stays the program's.**
#[test]
fn a_declaration_sets_only_what_it_names() {
    let base = Style {
        padding: Edges::all(9.0),
        font_size: 20.0,
        ..Style::default()
    };
    let c = computed(
        "background-color: #102030",
        &base,
        &Style::default(),
        &Inherited::default(),
    );
    assert_eq!(c.style.background, Color::rgba(0x10, 0x20, 0x30, 255));
    assert_eq!(c.style.padding, Edges::all(9.0));
    assert_eq!(c.style.font_size, 20.0);
    assert!(c.warnings.is_empty());
}

/// **A later declaration wins** -- the order is the whole rule.
#[test]
fn a_later_declaration_wins() {
    let c = top("color: red; color: blue; padding: 1px; padding-left: 7px");
    assert_eq!(c.style.foreground, Some(Color::rgba(0, 0, 255, 255)));
    assert_eq!(
        c.style.padding,
        Edges {
            top: 1.0,
            right: 1.0,
            bottom: 1.0,
            left: 7.0
        }
    );
}

/// **A child inherits what a style set on its parent** -- colour, size,
/// weight, alignment -- **and not what code set.**
#[test]
fn a_child_inherits_what_a_style_set() {
    let parent = top("color: #ff0000; font-size: 20px; font-weight: bold; text-align: center");
    let child = computed("", &Style::default(), &parent.style, &parent.inherited);
    assert_eq!(child.style.foreground, Some(Color::rgba(255, 0, 0, 255)));
    assert_eq!(child.style.font_size, 20.0);
    assert_eq!(child.style.font_weight, FontWeight::Bold);
    assert_eq!(child.style.text_align, TextAlign::Center);

    // The parent's size set in code: the child keeps its own.
    let coded = Style {
        font_size: 30.0,
        ..Style::default()
    };
    let child = computed("", &Style::default(), &coded, &Inherited::default());
    assert_eq!(child.style.font_size, Style::default().font_size);
    // A background is not inherited, by a style or otherwise.
    let parent = top("background-color: red");
    let child = computed("", &Style::default(), &parent.style, &parent.inherited);
    assert_eq!(child.style.background, Style::default().background);
}

/// **An em is the widget's font size -- its parent's, for `font-size`
/// itself -- and a rem the base size.**
#[test]
fn ems_and_rems_are_measured_as_css_measures_them() {
    let parent = top("font-size: 10px");
    let c = computed(
        "font-size: 2em; padding: 1em; margin-top: 1rem",
        &Style::default(),
        &parent.style,
        &parent.inherited,
    );
    assert_eq!(c.style.font_size, 20.0, "twice the parent's");
    assert_eq!(c.style.padding.left, 20.0, "one of its own");
    assert_eq!(c.style.margin.top, 16.0, "the base size");
    assert_eq!(
        top("font-size: 150%").style.font_size,
        21.0,
        "of the parent's 14"
    );
    assert_eq!(
        top("width: 3ch").style.width,
        Some(21.0),
        "half the size, a `0`"
    );
    assert_eq!(top("width: 10vw; height: 10vh").style.height, Some(60.0));
    assert_eq!(top("width: 2mm").style.width, Some(8.0));
}

/// **The theme's colours are variables**, and a custom property stands for
/// what it was given -- declared on the widget or inherited -- with a
/// fallback for one that stands for nothing.
#[test]
fn variables_stand_for_the_theme_and_for_custom_properties() {
    let palette = Palette::for_mode(false);
    let c = top("color: var(--accent); background-color: var(--surface0)");
    assert_eq!(c.style.foreground, Some(palette.accent));
    assert_eq!(c.style.background, palette.surface0);

    let parent = top("--gap: 6px");
    let child = computed(
        "padding: var(--gap)",
        &Style::default(),
        &parent.style,
        &parent.inherited,
    );
    assert_eq!(child.style.padding, Edges::all(6.0), "inherited");
    let own = top("padding: var(--gap); --gap: 3px");
    assert_eq!(
        own.style.padding,
        Edges::all(3.0),
        "declared after it is used"
    );
    let fallback = top("padding: var(--nothing, 2px)");
    assert_eq!(fallback.style.padding, Edges::all(2.0));
    assert_eq!(
        top("width: calc(var(--font-size) * 2)").style.width,
        Some(32.0)
    );
}

/// **A variable that stands for nothing, or for itself, drops its
/// declaration with a warning** -- and the rest stand.
#[test]
fn a_variable_that_stands_for_nothing_is_warned_of() {
    let c = top("color: var(--nothing); padding: 1px");
    assert_eq!(c.style.foreground, None);
    assert_eq!(c.style.padding, Edges::all(1.0));
    assert_eq!(c.warnings.len(), 1);
    assert!(c.warnings[0].contains("--nothing"), "{:?}", c.warnings);
    let circle = top("--a: var(--b); --b: var(--a); color: var(--a)");
    assert_eq!(circle.warnings.len(), 1);
    assert!(
        circle.warnings[0].contains("circle"),
        "{:?}",
        circle.warnings
    );
}

/// **`currentcolor` is the text's colour**: a border's, unless it says.
#[test]
fn currentcolor_is_the_texts() {
    let c = top("color: #00ff00; border: 1px solid");
    assert_eq!(c.style.border.left.color, Color::rgba(0, 255, 0, 255));
    assert_eq!(c.style.border.left.width, 1.0);
}

/// **A side with no border style has no border**, whatever its width says.
#[test]
fn a_border_with_no_style_is_none() {
    let c = top("border: 4px red");
    assert_eq!(c.style.border.top.width, 0.0);
    let c = top("border: 4px solid red; border-left-style: none");
    assert_eq!(c.style.border.top.width, 4.0);
    assert_eq!(c.style.border.left.width, 0.0);
}

/// **A percentage of the container waits for it**: auto while it is being
/// measured, settled when it is laid out.
#[test]
fn a_percentage_waits_for_its_container() {
    let mut c = top("width: 50%; padding: 10%; min-height: 20px");
    assert!(c.lengths.waits());
    assert_eq!(c.style.width, None, "auto until the container is known");
    assert_eq!(c.style.padding.left, 0.0);
    assert_eq!(
        c.style.min_height,
        Some(20.0),
        "no percentage: settled at once"
    );
    c.lengths.apply(&mut c.style, 400.0, 100.0);
    assert_eq!(c.style.width, Some(200.0));
    assert_eq!(
        c.style.padding.top, 40.0,
        "of the width, top and bottom too"
    );
    let c = top("width: calc(100% - 20px)");
    let mut style = c.style.clone();
    c.lengths.apply(&mut style, 300.0, 0.0);
    assert_eq!(style.width, Some(280.0));
    assert!(!top("width: 20px").lengths.waits());
}

/// **`bolder` and `lighter` step from the parent's weight**, as CSS Fonts
/// steps them.
#[test]
fn bolder_and_lighter_step_from_the_parent() {
    let parent = top("font-weight: 300");
    let child = computed(
        "font-weight: bolder",
        &Style::default(),
        &parent.style,
        &parent.inherited,
    );
    assert_eq!(child.style.font_weight, FontWeight::Regular);
    let parent = top("font-weight: bold");
    let child = computed(
        "font-weight: bolder",
        &Style::default(),
        &parent.style,
        &parent.inherited,
    );
    assert_eq!(child.style.font_weight, FontWeight::ExtraBold);
    let child = computed(
        "font-weight: lighter",
        &Style::default(),
        &parent.style,
        &parent.inherited,
    );
    assert_eq!(child.style.font_weight, FontWeight::Regular);
}

/// **A line height is a multiple of the font size**: a length is taken as
/// one, and a child inherits a multiple of its own size.
#[test]
fn a_line_height_is_a_multiple() {
    let c = top("font-size: 20px; line-height: 30px");
    assert_eq!(c.style.line_height, 1.5);
    let parent = top("line-height: 2");
    let child = computed(
        "font-size: 10px",
        &Style::default(),
        &parent.style,
        &parent.inherited,
    );
    assert_eq!(child.style.line_height, 2.0);
}

/// **`inherit` takes the parent's value and `initial` the toolkit's.**
#[test]
fn inherit_and_initial() {
    let coded = Style {
        background: Color::rgba(1, 2, 3, 255),
        padding: Edges::all(5.0),
        ..Style::default()
    };
    let c = computed(
        "background-color: inherit; padding: inherit",
        &Style::default(),
        &coded,
        &Inherited::default(),
    );
    assert_eq!(c.style.background, Color::rgba(1, 2, 3, 255));
    assert_eq!(c.style.padding, Edges::all(5.0));
    let base = Style {
        opacity: 0.5,
        ..Style::default()
    };
    let c = computed(
        "opacity: initial",
        &base,
        &Style::default(),
        &Inherited::default(),
    );
    assert_eq!(c.style.opacity, 1.0);
}

/// **Shadows are measured where the widget is**, a box's and its text's.
#[test]
fn shadows_are_measured() {
    let c = top("font-size: 10px; box-shadow: 1em 2px 3px 1px red; text-shadow: 1px 1px blue");
    let s = c.style.shadow.unwrap();
    assert_eq!(
        (s.offset_x, s.offset_y, s.blur, s.spread),
        (10.0, 2.0, 3.0, 1.0)
    );
    assert_eq!(s.color, Color::rgba(255, 0, 0, 255));
    let t = c.style.text_shadow.unwrap();
    assert_eq!(t.color, Color::rgba(0, 0, 255, 255));
    assert_eq!(top("box-shadow: none").style.shadow, None);
}

/// **The families a style asks for are kept, and inherited.**
#[test]
fn families_are_kept_and_inherited() {
    let parent = top("font-family: \"Noto Serif\", serif");
    assert_eq!(
        parent.font_family,
        Some(vec![Family::Named("Noto Serif".into()), Family::Ui])
    );
    let child = computed("", &Style::default(), &parent.style, &parent.inherited);
    assert_eq!(child.font_family, parent.font_family);
    assert_eq!(top("").font_family, None);
}

/// **A `ch` is a `0` in the widget's own family** -- the one its style gives
/// it or its parent's style left it -- and for `font-size`, in its parent's.
#[test]
fn a_ch_is_a_zero_in_the_widgets_family() {
    assert_eq!(
        top("font-family: monospace; width: 2ch").style.width,
        Some(28.0),
        "a fixed-pitch 0, the whole of the size 14"
    );
    let parent = top("font-family: monospace; font-size: 10px");
    let c = computed(
        "font-size: 2ch; padding: 1ch",
        &Style::default(),
        &parent.style,
        &parent.inherited,
    );
    assert_eq!(c.style.font_size, 20.0, "two of the parent's 0s");
    assert_eq!(c.style.padding.left, 20.0, "one of its own, inherited");
}
