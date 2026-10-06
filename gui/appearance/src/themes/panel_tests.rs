//! Tests for the taskbar-panel axis: the section read setting by setting,
//! what is refused and listed, the shipped theme against the built-in panel,
//! and a theme chosen for its panel.

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
            scratch: ScratchDir::new(&format!("slateos-panel-themes-{tag}")),
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

/// Every setting, none of them the built-in theme's: a flat bar, spaced out.
const FULL: &str = "taskbar-panel:
  gloss: 0
  spacing:
    tiles: 6
    after-start: 12
    between-sections: 31
";

/// [`FULL`]'s panel.
const FULL_STYLE: PanelStyle = PanelStyle {
    gloss: 0,
    tile_gap: 6,
    start_gap: 12,
    section_gap: 31,
};

/// **Every setting is read**, and a section that writes them all gives
/// exactly what it wrote, with nothing to report.
#[test]
fn every_setting_is_read() {
    let file = parse(FULL);
    assert_eq!(file.warnings, Vec::<String>::new());
    assert_eq!(file.panel, Some(FULL_STYLE));
}

/// **A section sets only what it names**: the rest is the built-in panel's.
#[test]
fn a_section_sets_only_what_it_names() {
    let file = parse("taskbar-panel:\n  spacing:\n    tiles: 3\n");
    assert_eq!(file.warnings, Vec::<String>::new());
    assert_eq!(
        file.panel,
        Some(PanelStyle {
            tile_gap: 3,
            ..PanelStyle::AERO
        })
    );
}

/// **Gloss is a share from 0 to 1**, written as a fraction or a percentage,
/// in hundredths; outside the range it is the nearer end, with a note, and
/// what is not a share is ignored and said.
#[test]
fn gloss_is_a_share() {
    let gloss = |value: &str| {
        let file = parse(&format!("taskbar-panel:\n  gloss: {value}\n"));
        (file.panel.map(|p| p.gloss), file.warnings)
    };
    assert_eq!(gloss("0.6"), (Some(60), Vec::new()));
    assert_eq!(gloss("1"), (Some(100), Vec::new()));
    assert_eq!(gloss("35%"), (Some(35), Vec::new()));
    assert_eq!(gloss("0.333"), (Some(33), Vec::new()));
    // Rounded to the nearest hundredth, not cut down to the one below.
    assert_eq!(gloss("0.996"), (Some(100), Vec::new()));
    assert_eq!(
        gloss("1.5"),
        (
            Some(100),
            vec!["`taskbar-panel.gloss` is taken as 1: the gloss is 0 to 1".to_owned()]
        )
    );
    assert_eq!(
        gloss("-2"),
        (
            Some(0),
            vec!["`taskbar-panel.gloss` is taken as 0: the gloss is 0 to 1".to_owned()]
        )
    );
    for bad in ["shiny", "NaN", "inf", "1/2"] {
        let (set, warnings) = gloss(bad);
        assert_eq!(set, None, "{bad}");
        assert_eq!(
            warnings,
            [format!(
                "`taskbar-panel.gloss` is ignored: `{bad}` is not a share from 0 to 1"
            )],
            "{bad}"
        );
    }
}

/// **A gap outside its range is the nearer end, with a note**; one that is
/// not a whole number is ignored and said.
#[test]
fn gaps_outside_their_range_are_the_nearer_end() {
    let file = parse(
        "taskbar-panel:\n  spacing:\n    tiles: 99\n    after-start: 2.5\n    between-sections: 1\n",
    );
    let style = file.panel.expect("set");
    assert_eq!(style.tile_gap, PanelStyle::MAX_TILE_GAP);
    assert_eq!(style.start_gap, PanelStyle::AERO.start_gap);
    assert_eq!(style.section_gap, PanelStyle::MIN_SECTION_GAP);
    assert_eq!(
        file.warnings,
        [
            "`taskbar-panel.spacing.tiles` is taken as 24: the gap between two tiles is 0 to 24 pixels",
            "`taskbar-panel.spacing.after-start` is ignored: `2.5` is not a whole number of pixels",
            "`taskbar-panel.spacing.between-sections` is taken as 3: the gap between the sections is 3 to 64 pixels",
        ]
    );
    let wide =
        parse("taskbar-panel:\n  spacing:\n    after-start: 100\n    between-sections: 100\n");
    let style = wide.panel.expect("set");
    assert_eq!(style.start_gap, PanelStyle::MAX_START_GAP);
    assert_eq!(style.section_gap, PanelStyle::MAX_SECTION_GAP);
}

/// **What is not understood costs that value and is listed**, and the rest
/// of the section is used.
#[test]
fn what_is_not_understood_costs_that_value_and_is_listed() {
    let file = parse(
        "taskbar-panel:
  blur: 4
  gloss: 0.5
  spacing:
    clock: 3
    tiles: 2
",
    );
    assert_eq!(
        file.panel,
        Some(PanelStyle {
            gloss: 50,
            tile_gap: 2,
            ..PanelStyle::AERO
        })
    );
    assert_eq!(
        file.warnings,
        [
            "`taskbar-panel.blur` is ignored: the taskbar panel has no part called `blur` (it has gloss and spacing)",
            "`taskbar-panel.spacing.clock` is ignored: the spacing has no gap called `clock` (it has tiles, after-start and between-sections)",
        ]
    );
}

/// **A section, or its spacing, written as a value is refused**, and a file
/// without a usable section sets no panel.
#[test]
fn a_file_without_a_usable_section_sets_no_panel() {
    let scalar = parse("taskbar-panel: flat\n");
    assert_eq!(scalar.panel, None);
    assert_eq!(
        scalar.warnings,
        [
            "`taskbar-panel` is ignored: it holds settings (gloss and spacing), not a value like `flat`"
        ]
    );
    let spacing = parse("taskbar-panel:\n  spacing: wide\n");
    assert_eq!(spacing.panel, None);
    assert_eq!(
        spacing.warnings,
        [
            "`taskbar-panel.spacing` is ignored: it holds gaps (tiles, after-start and between-sections), not a value like `wide`"
        ]
    );
    assert_eq!(parse("meta:\n  name: Plain\n").panel, None);
    let useless = parse("taskbar-panel:\n  gloss: shiny\n");
    assert_eq!(useless.panel, None);
    assert_eq!(useless.warnings.len(), 1);
}

/// **The shipped built-in theme writes out the built-in panel**, every
/// setting present, as the template to copy.
#[test]
fn the_shipped_built_in_theme_writes_out_the_built_in_panel() {
    const AERO_FILE: &str = include_str!("../../themes/aero/theme.yaml");
    let file = parse(AERO_FILE);
    assert_eq!(file.warnings, Vec::<String>::new());
    assert_eq!(file.panel, Some(PanelStyle::AERO));
    let doc = Document::parse(AERO_FILE);
    assert_eq!(doc.keys(&[SECTION]), ["gloss", "spacing"]);
    assert_eq!(
        doc.keys(&[SECTION, "spacing"]),
        ["tiles", "after-start", "between-sections"]
    );
    assert!(
        file.meta.supports.iter().any(|axis| axis == SECTION),
        "the template does not say it covers the axis"
    );
}

/// **An installed theme is loaded for its panel.**
#[test]
fn an_installed_theme_is_loaded_for_its_panel() {
    let f = Fixture::new("load");
    f.install("flat", FULL.as_bytes());
    let theme = PanelTheme::load_from(&f.dirs(), OsStr::new("flat"));
    assert_eq!(theme.id(), "flat");
    assert!(!theme.is_built_in());
    assert_eq!(theme.problem(), None);
    assert_eq!(theme.warnings(), &[] as &[String]);
    assert_eq!(theme.style(), FULL_STYLE);

    f.install("noisy", b"taskbar-panel:\n  gloss: 0.5\n  glow: 2\n");
    let theme = PanelTheme::load_from(&f.dirs(), OsStr::new("noisy"));
    assert_eq!(theme.style().gloss, 50);
    assert_eq!(theme.warnings().len(), 1, "{:?}", theme.warnings());

    let chosen = PanelTheme::from_style("preview", FULL_STYLE);
    assert_eq!(chosen.id(), "preview");
    assert_eq!(chosen.style(), FULL_STYLE);
}

/// **The built-in theme reads no file**: its panel is compiled in, so an
/// `aero` folder with a section of its own changes nothing.
#[test]
fn the_built_in_theme_reads_no_file() {
    let f = Fixture::new("built-in");
    f.install(BUILT_IN, FULL.as_bytes());
    let theme = PanelTheme::load_from(&f.dirs(), OsStr::new(BUILT_IN));
    assert_eq!(theme, PanelTheme::built_in());
    assert_eq!(theme.style(), PanelStyle::AERO);
    assert!(theme.is_built_in());
    assert_eq!(PanelTheme::default(), PanelTheme::built_in());
}

/// **A theme that cannot give the panel says why and keeps its name**, and
/// the built-in panel is used.
#[test]
fn a_theme_that_cannot_be_used_says_why_and_keeps_its_name() {
    let f = Fixture::new("unusable");
    f.install("nord", b"colors:\n  base: \"#2e3440\"\n");
    f.install("binary", b"taskbar-panel:\n  gloss: \xff\n");
    for (id, why) in [
        ("gone", "\"gone\" is not installed"),
        ("../up", "is not the name of a folder in a themes directory"),
        ("nord", "\"nord\" sets no taskbar panel"),
        ("binary", "\"binary\" has a file that is not text"),
    ] {
        let theme = PanelTheme::load_from(&f.dirs(), OsStr::new(id));
        assert_eq!(theme.id(), id);
        assert_eq!(theme.style(), PanelStyle::AERO, "{id}");
        let problem = theme.problem().unwrap_or_default();
        assert!(problem.contains(why), "{id}: {problem:?}");
        assert!(
            problem.ends_with("so the built-in taskbar panel is used."),
            "{problem}"
        );
    }
}

/// **The list says which themes can give the panel**: the built-in one
/// always, a theme with a usable section, and not a colours-only theme or
/// one that cannot be read.
#[test]
fn the_list_says_which_themes_give_the_panel() {
    let f = Fixture::new("list");
    f.install("nord", b"colors:\n  base: \"#2e3440\"\n");
    f.install("flat", FULL.as_bytes());
    f.install("broken", b"taskbar-panel:\n  gloss: \xff\n");
    let listed = available_in(&f.dirs());
    let gives = |id: &str| {
        listed
            .iter()
            .find(|info| info.id == id)
            .is_some_and(super::super::ThemeInfo::provides_panel)
    };
    assert!(gives(BUILT_IN));
    assert!(gives("flat"));
    assert!(!gives("nord"));
    assert!(!gives("broken"));
    let built_in = listed.iter().find(|info| info.id == BUILT_IN).unwrap();
    assert_eq!(built_in.origin, Origin::BuiltIn);
}
