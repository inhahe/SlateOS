// A test module's job is to fail loudly the instant the code under test is
// wrong, so the defensive lints that forbid exactly that in production code
// are off here -- as `CLAUDE.md` prescribes.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use std::fs;

use scratchdir::ScratchDir;

use super::super::{ThemeError, available_in, parse};
use super::*;
use crate::FontSettings;

/// Themes directories in a scratch directory: a user's and a system's.
struct Dirs {
    _scratch: ScratchDir,
    dirs: ThemeDirs,
}

impl Dirs {
    fn new() -> Self {
        let scratch = ScratchDir::new("font-themes");
        let dirs = ThemeDirs {
            user: Some(scratch.dir().join("user")),
            system: scratch.dir().join("system"),
        };
        Self {
            _scratch: scratch,
            dirs,
        }
    }

    /// Install the theme `name` in the user's directory, its `theme.yaml`
    /// saying `text`.
    fn install(&self, name: &str, text: &str) {
        let dir = self.dirs.user.as_ref().unwrap().join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(super::super::FILE_NAME), text).unwrap();
    }
}

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|name| (*name).to_owned()).collect()
}

/// **A theme's `fonts` section names families for each role**, in the order
/// they are tried -- a flow list, a block list or one name -- and one naming
/// none, or no section, is no recommendation.
#[test]
fn the_section_names_families_for_each_role() {
    let file = parse("fonts:\n  ui: [Inter, \"Noto Sans\"]\n  mono: Fira Code\n");
    assert_eq!(
        file.fonts,
        Some(FontNames {
            ui: names(&["Inter", "Noto Sans"]),
            mono: names(&["Fira Code"]),
        })
    );
    assert!(file.warnings.is_empty(), "{:?}", file.warnings);

    let block = parse("fonts:\n  mono:\n    - Iosevka\n    - \"Fira Code\"\n");
    assert_eq!(
        block.fonts,
        Some(FontNames {
            ui: Vec::new(),
            mono: names(&["Iosevka", "Fira Code"]),
        })
    );

    assert_eq!(parse("colors:\n  base: \"#000000\"\n").fonts, None);
    assert_eq!(parse("fonts:\n  ui: \"\"\n").fonts, None);
}

/// **What the section holds that is no role, or no family's name, is said
/// and dropped** -- and a family named twice, in any case, is tried once.
///
/// The tab is written in a block list: there a double-quoted item's escapes
/// are YAML's, so `\t` is a tab. A flow list (`[a, b]`) is read by the theme
/// file's own small reader, which takes a backslash's next character as it
/// is -- `\t` there is a `t`.
#[test]
fn what_is_not_a_family_is_dropped_and_said() {
    let file = parse(
        "fonts:\n  ui:\n    - Inter\n    - inter\n    - \"Tab\\tName\"\n    - INTER\n    - Cantarell\n  \
         title: Georgia\n",
    );
    assert_eq!(file.fonts.unwrap().ui, names(&["Inter", "Cantarell"]));
    for wanted in ["`fonts.title` is ignored", "has no control characters"] {
        assert!(
            file.warnings.iter().any(|w| w.contains(wanted)),
            "{wanted}: {:?}",
            file.warnings
        );
    }
}

/// **The chosen theme's first family this machine has is drawn for each
/// role**, in place of the user's own; a role with none installed keeps the
/// user's; and the built-in theme recommends nothing, so the user's own are
/// drawn whole.
#[test]
fn a_theme_gives_its_first_installed_family_for_each_role() {
    let dirs = Dirs::new();
    dirs.install(
        "nord",
        "fonts:\n  ui: [Inter, Noto Sans, DejaVu Sans]\n  mono: [Fira Code]\n",
    );
    let theme = FontTheme::load_from(&dirs.dirs, OsStr::new("nord"));
    assert_eq!(theme.problem(), None);
    assert_eq!(theme.ui(), names(&["Inter", "Noto Sans", "DejaVu Sans"]));
    assert_eq!(theme.mono(), names(&["Fira Code"]));

    let own = FontSettings {
        ui_font: "Open Sans".to_owned(),
        mono_font: "JetBrains Mono".to_owned(),
        ui_size: 15.0,
        ..FontSettings::default()
    };
    let installed = |family: &str| family == "Noto Sans" || family == "DejaVu Sans";
    let fonts = theme.families_in_use(&own, installed);
    assert_eq!(fonts.ui_font, "Noto Sans", "the first of its installed");
    assert_eq!(
        fonts.mono_font, "JetBrains Mono",
        "none installed: the user's"
    );
    assert_eq!(
        FontSettings {
            ui_font: own.ui_font.clone(),
            ..fonts
        },
        own,
        "only the families are the theme's"
    );

    let everything = theme.families_in_use(&own, |_| true);
    assert_eq!(everything.ui_font, "Inter");
    assert_eq!(everything.mono_font, "Fira Code");

    let built_in = FontTheme::load_from(&dirs.dirs, OsStr::new(super::super::BUILT_IN));
    assert_eq!(built_in, FontTheme::built_in());
    assert_eq!(built_in.families_in_use(&own, |_| true), own);
}

/// **A theme that cannot give fonts says why**, keeps its name, and
/// recommends nothing -- one with no `fonts` section, one not installed.
#[test]
fn what_cannot_be_used_says_why() {
    let dirs = Dirs::new();
    dirs.install("plain", "colors:\n  base: \"#000000\"\n");
    let plain = FontTheme::load_from(&dirs.dirs, OsStr::new("plain"));
    let problem = plain.problem().expect("no fonts is a problem");
    assert!(problem.contains("recommends no fonts"), "{problem}");
    assert!(problem.contains("your own fonts"), "{problem}");
    assert!(plain.ui().is_empty() && plain.mono().is_empty());

    let absent = FontTheme::load_from(&dirs.dirs, OsStr::new("nowhere"));
    assert!(
        absent
            .problem()
            .unwrap()
            .contains(&ThemeError::NotInstalled.to_string())
    );
    assert_eq!(absent.id(), "nowhere", "the choice is kept");
    let own = FontSettings::default();
    assert_eq!(absent.families_in_use(&own, |_| true), own);
}

/// **A theme list says which themes recommend fonts, and which** -- the
/// built-in theme none, whatever the system's copy of its file says.
#[test]
fn a_list_says_which_themes_recommend_fonts() {
    let dirs = Dirs::new();
    dirs.install("nord", "fonts:\n  ui: Inter\n");
    dirs.install("plain", "colors:\n  base: \"#000000\"\n");
    let system = dirs.dirs.system.join(super::super::BUILT_IN);
    fs::create_dir_all(&system).unwrap();
    fs::write(
        system.join(super::super::FILE_NAME),
        "fonts:\n  ui: Georgia\n",
    )
    .unwrap();

    let list = available_in(&dirs.dirs);
    let find = |id: &str| list.iter().find(|t| t.id == OsStr::new(id)).unwrap();
    assert!(find("nord").provides_fonts());
    assert_eq!(find("nord").fonts.ui, names(&["Inter"]));
    assert!(!find("plain").provides_fonts());
    assert!(find("plain").fonts.is_empty());
    assert_eq!(list[0].id, OsStr::new(super::super::BUILT_IN));
    assert!(!list[0].provides_fonts());
    assert!(list[0].fonts.is_empty());
}
