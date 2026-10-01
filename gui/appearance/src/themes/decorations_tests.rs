//! Tests for the window-decorations axis: the section read setting by
//! setting, what is refused and listed, the shipped theme against the
//! built-in frame, and a theme chosen for its frames.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use super::super::{Origin, available_in, parse};
use super::*;
use scratchdir::ScratchDir;
use std::fs;

/// A system themes directory in a scratch directory, and no user one, so
/// nothing on the machine running the test is read.
struct Fixture {
    scratch: ScratchDir,
}

impl Fixture {
    fn new(tag: &str) -> Self {
        Self {
            scratch: ScratchDir::new(&format!("slateos-decoration-themes-{tag}")),
        }
    }

    fn dirs(&self) -> ThemeDirs {
        ThemeDirs {
            user: None,
            system: self.scratch.dir().join("system"),
        }
    }

    fn install(&self, id: &str, text: &[u8]) {
        let dir = self.scratch.dir().join("system").join(id);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(FILE_NAME), text).unwrap();
    }
}

/// Every setting, none of them the built-in theme's.
const FULL: &str = "window-decorations:
  title-bar:
    height: 40
    align: center
    bold: true
  buttons:
    side: left
    order: [close, minimize, maximize]
    shape: circle
    size: 14
    gap: 8
  border: 2
  shadow: 20
";

/// [`FULL`]'s frame.
const FULL_STYLE: DecorationStyle = DecorationStyle {
    title_height: 40,
    title_align: TitleAlign::Center,
    title_bold: true,
    button_side: ButtonSide::Left,
    buttons: [
        TitleButton::Close,
        TitleButton::Minimize,
        TitleButton::Maximize,
    ],
    button_shape: ButtonShape::Circle,
    button_size: 14,
    button_gap: 8,
    border: 2,
    shadow: 20,
};

/// **Every setting is read**, and a section that writes them all gives
/// exactly what it wrote, with nothing to report.
#[test]
fn every_setting_is_read() {
    let file = parse(FULL);
    assert_eq!(file.warnings, Vec::<String>::new());
    assert_eq!(file.decorations, Some(FULL_STYLE));
}

/// **A section sets only what it names**: the rest is the built-in frame's.
#[test]
fn a_section_sets_only_what_it_names() {
    let file = parse("window-decorations:\n  border: 0\n  buttons:\n    shape: glyph\n");
    assert_eq!(file.warnings, Vec::<String>::new());
    assert_eq!(
        file.decorations,
        Some(DecorationStyle {
            border: 0,
            button_shape: ButtonShape::Glyph,
            ..DecorationStyle::AERO
        })
    );
}

/// **An order is the three buttons, each once**, in flow or block style;
/// one that names a button twice, leaves one out or names something else
/// is refused whole, and says which.
#[test]
fn an_order_names_each_button_once() {
    let block = parse(
        "window-decorations:\n  buttons:\n    order:\n      - close\n      - maximize\n      - minimize\n",
    );
    assert_eq!(block.warnings, Vec::<String>::new());
    assert_eq!(
        block.decorations.map(|d| d.buttons),
        Some([
            TitleButton::Close,
            TitleButton::Maximize,
            TitleButton::Minimize
        ])
    );

    for (order, why) in [
        ("[close, close, minimize]", "it names `close` twice"),
        ("[close, minimize]", "it leaves out `maximize`"),
        ("[close, shrink, minimize]", "`shrink` is not a button"),
    ] {
        let file = parse(&format!(
            "window-decorations:\n  border: 2\n  buttons:\n    order: {order}\n"
        ));
        // The rest of the section is used; the order is not.
        let style = file.decorations.expect("the border is still set");
        assert_eq!(style.buttons, DecorationStyle::AERO.buttons, "{order}");
        assert_eq!(style.border, 2);
        assert_eq!(file.warnings.len(), 1, "{order}: {:?}", file.warnings);
        assert!(
            file.warnings[0].contains(why),
            "{order}: {:?}",
            file.warnings
        );
    }

    let empty = parse("window-decorations:\n  border: 2\n  buttons:\n    order:\n");
    assert!(
        empty.warnings[0].contains("has no value"),
        "{:?}",
        empty.warnings
    );
}

/// **A size outside its range is the nearer end, with a note**; one that is
/// not a whole number is ignored and said.
#[test]
fn sizes_outside_their_range_are_the_nearer_end() {
    let file = parse(
        "window-decorations:\n  title-bar:\n    height: 100\n  buttons:\n    size: 2\n    gap: 2.5\n  shadow: -3\n",
    );
    let style = file.decorations.expect("set");
    assert_eq!(style.title_height, DecorationStyle::MAX_TITLE_HEIGHT);
    assert_eq!(style.button_size, DecorationStyle::MIN_BUTTON_SIZE);
    assert_eq!(style.button_gap, DecorationStyle::AERO.button_gap);
    assert_eq!(style.shadow, 0);
    assert_eq!(
        file.warnings,
        [
            "`window-decorations.title-bar.height` is taken as 56: a title bar is 20 to 56 pixels",
            "`window-decorations.buttons.size` is taken as 12: a button is 12 to 40 pixels",
            "`window-decorations.buttons.gap` is ignored: `2.5` is not a whole number of pixels",
            "`window-decorations.shadow` is taken as 0: a shadow is 0 to 48 pixels",
        ]
    );
}

/// **What is not understood costs that value and is listed**: an unknown
/// part or setting, a choice that is none of the choices, a flag that is
/// neither -- and the rest of the section is used.
#[test]
fn what_is_not_understood_costs_that_value_and_is_listed() {
    let file = parse(
        "window-decorations:
  corners: 4
  title-bar:
    align: middle
    font: serif
    bold: maybe
    height: 24
  buttons:
    shape: hexagon
    side: right
",
    );
    assert_eq!(
        file.decorations,
        Some(DecorationStyle {
            title_height: 24,
            ..DecorationStyle::AERO
        })
    );
    assert_eq!(file.warnings.len(), 5, "{:?}", file.warnings);
    for expected in [
        "`window-decorations.corners` is ignored: window decorations have no part called `corners`",
        "`window-decorations.title-bar.align` is ignored: `middle` is not left or center",
        "`window-decorations.title-bar.font` is ignored: a title bar has no setting called `font`",
        "`window-decorations.title-bar.bold` is ignored: `maybe` is not true or false",
        "`window-decorations.buttons.shape` is ignored: `hexagon` is not rounded, circle, square or glyph",
    ] {
        assert!(
            file.warnings.iter().any(|w| w.starts_with(expected)),
            "missing {expected:?} in {:?}",
            file.warnings
        );
    }
}

/// **A section written as a value is refused, and a file without a usable
/// section sets no frames.**
#[test]
fn a_file_without_a_usable_section_sets_no_frames() {
    let scalar = parse("window-decorations: none\n");
    assert_eq!(scalar.decorations, None);
    assert_eq!(
        scalar.warnings,
        [
            "`window-decorations` is ignored: it holds settings (title-bar, buttons, border and shadow), not a value like `none`"
        ]
    );
    assert_eq!(parse("meta:\n  name: Plain\n").decorations, None);
    let useless = parse("window-decorations:\n  title-bar:\n    align: middle\n");
    assert_eq!(useless.decorations, None);
    assert_eq!(useless.warnings.len(), 1);
}

/// **The shipped built-in theme writes out the built-in frame**, every
/// setting present, as the template to copy.
#[test]
fn the_shipped_built_in_theme_writes_out_the_built_in_frames() {
    const AERO_FILE: &str = include_str!("../../themes/aero/theme.yaml");
    let file = parse(AERO_FILE);
    assert_eq!(file.warnings, Vec::<String>::new());
    assert_eq!(file.decorations, Some(DecorationStyle::AERO));
    let doc = Document::parse(AERO_FILE);
    assert_eq!(
        doc.keys(&[SECTION]),
        ["title-bar", "buttons", "border", "shadow"]
    );
    assert_eq!(
        doc.keys(&[SECTION, "title-bar"]),
        ["height", "align", "bold"]
    );
    assert_eq!(
        doc.keys(&[SECTION, "buttons"]),
        ["side", "order", "shape", "size", "gap"]
    );
    assert!(
        file.meta.supports.iter().any(|axis| axis == SECTION),
        "the template does not say it covers the axis"
    );
}

/// **An installed theme is loaded for its frames.**
#[test]
fn an_installed_theme_is_loaded_for_its_frames() {
    let f = Fixture::new("load");
    f.install("roomy", FULL.as_bytes());
    let theme = DecorationTheme::load_from(&f.dirs(), OsStr::new("roomy"));
    assert_eq!(theme.id(), "roomy");
    assert!(!theme.is_built_in());
    assert_eq!(theme.problem(), None);
    assert_eq!(theme.warnings(), &[] as &[String]);
    assert_eq!(theme.style(), FULL_STYLE);

    f.install("noisy", b"window-decorations:\n  border: 3\n  glow: 2\n");
    let theme = DecorationTheme::load_from(&f.dirs(), OsStr::new("noisy"));
    assert_eq!(theme.style().border, 3);
    assert_eq!(theme.warnings().len(), 1, "{:?}", theme.warnings());

    let chosen = DecorationTheme::from_style("preview", FULL_STYLE);
    assert_eq!(chosen.id(), "preview");
    assert_eq!(chosen.style(), FULL_STYLE);
}

/// **The built-in theme reads no file**: its frames are compiled in, so an
/// `aero` folder with a section of its own changes nothing.
#[test]
fn the_built_in_theme_reads_no_file() {
    let f = Fixture::new("built-in");
    f.install(BUILT_IN, FULL.as_bytes());
    let theme = DecorationTheme::load_from(&f.dirs(), OsStr::new(BUILT_IN));
    assert_eq!(theme, DecorationTheme::built_in());
    assert_eq!(theme.style(), DecorationStyle::AERO);
    assert!(theme.is_built_in());
    assert_eq!(DecorationTheme::default(), DecorationTheme::built_in());
}

/// **A theme that cannot give the frames says why and keeps its name**, and
/// the built-in frames are used.
#[test]
fn a_theme_that_cannot_be_used_says_why_and_keeps_its_name() {
    let f = Fixture::new("unusable");
    f.install("nord", b"colors:\n  base: \"#2e3440\"\n");
    f.install("binary", b"window-decorations:\n  border: \xff\n");
    for (id, why) in [
        ("gone", "\"gone\" is not installed"),
        ("../up", "is not the name of a folder in a themes directory"),
        ("nord", "\"nord\" sets no window frames"),
        ("binary", "\"binary\" has a file that is not text"),
    ] {
        let theme = DecorationTheme::load_from(&f.dirs(), OsStr::new(id));
        assert_eq!(theme.id(), id);
        assert_eq!(theme.style(), DecorationStyle::AERO, "{id}");
        let problem = theme.problem().unwrap_or_default();
        assert!(problem.contains(why), "{id}: {problem:?}");
        assert!(
            problem.ends_with("so the built-in window frames are used."),
            "{problem}"
        );
    }
}

/// **The list says which themes can give the frames**: the built-in one
/// always, a theme with a usable section, and not a colours-only theme or
/// one that cannot be read.
#[test]
fn the_list_says_which_themes_give_the_frames() {
    let f = Fixture::new("list");
    f.install("nord", b"colors:\n  base: \"#2e3440\"\n");
    f.install("roomy", FULL.as_bytes());
    f.install("broken", b"window-decorations:\n  border: \xff\n");
    let listed = available_in(&f.dirs());
    let gives = |id: &str| {
        listed
            .iter()
            .find(|info| info.id == OsStr::new(id))
            .unwrap_or_else(|| panic!("{id} is not listed"))
            .provides_decorations()
    };
    assert_eq!(listed[0].origin, Origin::BuiltIn);
    assert!(gives(BUILT_IN));
    assert!(!gives("nord"));
    assert!(gives("roomy"));
    assert!(!gives("broken"));

    let mut contradictory = listed
        .iter()
        .find(|info| info.id == OsStr::new("roomy"))
        .cloned()
        .expect("roomy is listed");
    contradictory.problem = Some(ThemeError::NotText);
    assert!(!contradictory.provides_decorations());
}
