//! Tests for making themes: deriving, editing and saving, installing,
//! exporting and removing -- each against scratch theme directories, with
//! what is left on disk afterwards checked as closely as what is returned.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use super::*;
use crate::cursors::CursorTheme;
use crate::icons::IconTheme;
use crate::themes::{
    ColorTheme, FontTheme, Origin, ThemeDirs, WallpaperTheme, WidgetTheme, available_in,
};
use scratchdir::ScratchDir;

/// A user's and a system's theme directories in a scratch directory, and
/// somewhere outside both to keep themes that are not installed.
struct Fixture {
    scratch: ScratchDir,
}

impl Fixture {
    fn new(tag: &str) -> Self {
        Self {
            scratch: ScratchDir::new(&format!("slateos-authoring-{tag}")),
        }
    }

    fn dirs(&self) -> ThemeDirs {
        ThemeDirs {
            user: Some(self.user()),
            system: self.system(),
        }
    }

    fn user(&self) -> PathBuf {
        self.scratch.dir().join("user")
    }

    fn system(&self) -> PathBuf {
        self.scratch.dir().join("system")
    }

    /// A folder outside both theme directories.
    fn outside(&self, rel: &str) -> PathBuf {
        self.scratch.dir().join("outside").join(rel)
    }

    /// Write `bytes` at `rel` under `root`, making the folders on the way.
    fn write(&self, root: &Path, rel: &str, bytes: impl AsRef<[u8]>) {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    /// Install a theme of `text` as `id` in the system's directory.
    fn system_theme(&self, id: &str, text: &str) -> PathBuf {
        let dir = self.system().join(id);
        self.write(&dir, FILE_NAME, text);
        dir
    }

    /// Install a theme of `text` as `id` in the user's directory.
    fn user_theme(&self, id: &str, text: &str) -> PathBuf {
        let dir = self.user().join(id);
        self.write(&dir, FILE_NAME, text);
        dir
    }

    /// The names in the user's directory, hidden ones included: what a
    /// failure must not leave behind. Kept as the names they are, bytes and
    /// all, so a name that is not text is not reported as another.
    fn user_entries(&self) -> Vec<OsString> {
        let Ok(listing) = fs::read_dir(self.user()) else {
            return Vec::new();
        };
        let mut names: Vec<OsString> = listing.map(|entry| entry.unwrap().file_name()).collect();
        names.sort();
        names
    }
}

/// A theme with comments, a screenshot, a wallpaper that is also a
/// screenshot, and an icon.
const NORD: &str = "\
# Nord, for a cold night.
meta:
  name: Nord
  author: Someone
  version: \"1.2\"
  license: MIT
  tags: [cool, blue]
  screenshots: [dark.png, wallpapers/night.png]
  supports: [colors, icons, wallpapers]
colors:
  base: \"#2e3440\"  # the page
  text: \"#eceff4\"
colors-light:
  base: \"#eceff4\"
wallpapers:
  dark: wallpapers/night.png
";

/// An icon: a square in `currentColor`.
const SQUARE: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"16\" height=\"16\" \
     viewBox=\"0 0 16 16\"><rect x=\"2\" y=\"2\" width=\"12\" height=\"12\" fill=\"currentColor\"/></svg>";

/// A small PNG.
fn png() -> Vec<u8> {
    imagecodec::encode_png(4, 4, &[0xFF33_6699; 16]).unwrap()
}

/// Install Nord in the system's directory with its icon and pictures.
fn system_nord(fx: &Fixture) -> PathBuf {
    let dir = fx.system_theme("nord", NORD);
    fx.write(&dir, "icons/folder.svg", SQUARE);
    fx.write(&dir, "dark.png", png());
    fx.write(&dir, "wallpapers/night.png", png());
    dir
}

fn os(id: &str) -> &OsStr {
    OsStr::new(id)
}

fn rgb(hex: u32) -> Color {
    Color::from_hex(hex)
}

/// Every finding, a line each, for a failed assertion to show.
fn listing(findings: &[Finding]) -> String {
    findings
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

// ---- sections and keys ----

#[test]
fn each_section_and_meta_key_is_found_by_its_key() {
    for section in ColorSection::ALL {
        assert_eq!(ColorSection::from_key(section.key()), Some(section));
    }
    assert_eq!(ColorSection::from_key("colours"), None);
    for field in MetaText::ALL {
        assert_eq!(MetaText::from_key(field.key()), Some(field));
    }
    assert_eq!(MetaText::from_key("tags"), None);
}

/// The roles are the reader's: the palette's without the accent, a
/// terminal's, and the highlighter's kinds.
#[test]
fn a_section_can_set_the_colours_the_desktop_reads_it_for() {
    let desktop = ColorSection::Dark.roles();
    assert_eq!(desktop, THEME_ROLES.to_vec());
    assert!(!desktop.contains(&"accent"));
    assert_eq!(ColorSection::Light.roles(), desktop);
    assert!(ColorSection::TerminalLight.roles().contains(&"bright-red"));
    assert!(ColorSection::SyntaxDark.roles().contains(&"keyword"));
    assert!(!ColorSection::SyntaxDark.roles().contains(&"base"));
}

#[test]
fn a_section_gives_its_own_colours_of_a_file() {
    let file = parse(BUILT_IN_TEMPLATE);
    assert_eq!(ColorSection::Dark.of(&file.colors), &file.colors.dark);
    assert_eq!(ColorSection::Light.of(&file.colors), &file.colors.light);
    assert_eq!(
        ColorSection::TerminalDark.of(&file.colors),
        &file.colors.terminal_dark
    );
    assert_eq!(
        ColorSection::TerminalLight.of(&file.colors),
        &file.colors.terminal_light
    );
    assert_eq!(
        ColorSection::SyntaxDark.of(&file.colors),
        &file.colors.syntax_dark
    );
    assert_eq!(
        ColorSection::SyntaxLight.of(&file.colors),
        &file.colors.syntax_light
    );
    // Six different maps, not one returned six times.
    assert_ne!(file.colors.dark, file.colors.light);
    assert_ne!(file.colors.terminal_dark, file.colors.syntax_dark);
}

// ---- a draft ----

#[test]
fn a_colour_set_is_one_line_changed_and_the_rest_of_the_text_kept() {
    let fx = Fixture::new("set");
    fx.user_theme("mine", NORD);
    let mut draft = ThemeDraft::open(&fx.dirs(), os("mine")).unwrap();
    assert!(!draft.is_changed());

    draft
        .set_color(ColorSection::Dark, "base", rgb(0x10_2030))
        .unwrap();
    assert!(draft.is_changed());
    let expected = NORD.replace(
        "base: \"#2e3440\"  # the page",
        "base: \"#102030\"  # the page",
    );
    assert_eq!(draft.doc.to_text(), expected);
    assert_eq!(draft.file().colors.dark["base"], rgb(0x10_2030));

    // A role the section does not have yet: a new line in it.
    draft
        .set_color(ColorSection::Light, "text", rgb(0x11_1111))
        .unwrap();
    assert_eq!(draft.file().colors.light["text"], rgb(0x11_1111));
    // A section the file does not have yet: made.
    draft
        .set_color(ColorSection::TerminalDark, "bright-red", rgb(0xFF_0000))
        .unwrap();
    draft
        .set_color(ColorSection::SyntaxLight, "keyword", rgb(0x88_39EF))
        .unwrap();
    let file = draft.file();
    assert_eq!(file.colors.terminal_dark["bright-red"], rgb(0xFF_0000));
    assert_eq!(file.colors.syntax_light["keyword"], rgb(0x88_39EF));
    assert_eq!(file.warnings, Vec::<String>::new());

    // Nothing reached the disk until the save.
    let on_disk = fx.user().join("mine").join(FILE_NAME);
    assert_eq!(fs::read_to_string(&on_disk).unwrap(), NORD);
    draft.save().unwrap();
    assert!(!draft.is_changed());
    assert_eq!(fs::read_to_string(&on_disk).unwrap(), draft.doc.to_text());
    let reopened = ThemeDraft::open(&fx.dirs(), os("mine")).unwrap();
    assert_eq!(reopened.file(), file);
    assert!(!reopened.is_changed());
    // The save left no temporary beside the file.
    let mut names: Vec<_> = fs::read_dir(fx.user().join("mine"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    names.sort();
    assert_eq!(names, [OsString::from(FILE_NAME)]);
}

#[test]
fn a_colour_cleared_goes_with_its_section_once_that_is_empty() {
    let fx = Fixture::new("clear");
    fx.user_theme("mine", NORD);
    let mut draft = ThemeDraft::open(&fx.dirs(), os("mine")).unwrap();

    assert!(draft.clear_color(ColorSection::Dark, "text"));
    assert!(!draft.file().colors.dark.contains_key("text"));
    assert!(draft.file().colors.dark.contains_key("base"));
    assert!(draft.doc.contains(&[DARK_SECTION]));
    // Nothing to clear: nothing changes.
    let before = draft.doc.to_text();
    assert!(!draft.clear_color(ColorSection::Dark, "text"));
    assert!(!draft.clear_color(ColorSection::SyntaxDark, "keyword"));
    assert_eq!(draft.doc.to_text(), before);

    assert!(draft.clear_color(ColorSection::Light, "base"));
    assert!(!draft.doc.contains(&[LIGHT_SECTION]));
    assert!(draft.file().colors.light.is_empty());
    // The rest of the file is as it was.
    assert!(
        draft
            .doc
            .to_text()
            .starts_with("# Nord, for a cold night.\n")
    );
    assert_eq!(draft.file().meta.name.as_deref(), Some("Nord"));
}

#[test]
fn a_colour_a_theme_cannot_hold_is_refused_and_changes_nothing() {
    let fx = Fixture::new("refuse");
    fx.user_theme("mine", NORD);
    let mut draft = ThemeDraft::open(&fx.dirs(), os("mine")).unwrap();
    let before = draft.doc.to_text();

    // The accent is the user's.
    assert_eq!(
        draft.set_color(ColorSection::Dark, "accent", rgb(0xFF_0000)),
        Err(AuthoringError::NoSuchRole(
            ColorSection::Dark,
            "accent".to_owned()
        ))
    );
    // A kind of code is not a desktop colour, nor the other way about.
    assert!(matches!(
        draft.set_color(ColorSection::Light, "keyword", rgb(0)),
        Err(AuthoringError::NoSuchRole(ColorSection::Light, _))
    ));
    assert!(matches!(
        draft.set_color(ColorSection::SyntaxDark, "base", rgb(0)),
        Err(AuthoringError::NoSuchRole(ColorSection::SyntaxDark, _))
    ));
    let see_through = Color::rgba(1, 2, 3, 254);
    assert_eq!(
        draft.set_color(ColorSection::Dark, "base", see_through),
        Err(AuthoringError::Translucent(see_through))
    );
    assert_eq!(draft.doc.to_text(), before);
    assert!(!draft.is_changed());
}

#[test]
fn what_describes_a_theme_is_set_and_removed() {
    let fx = Fixture::new("meta");
    fx.user_theme("mine", NORD);
    let mut draft = ThemeDraft::open(&fx.dirs(), os("mine")).unwrap();

    draft.set_meta(MetaText::Name, "  Nord, warmer  ");
    draft.set_meta(MetaText::Author, "");
    draft.set_meta(MetaText::Version, "2.0");
    draft.set_meta(MetaText::License, "CC-BY-4.0");
    draft.set_tags(&["warm", " ", " evening "]);
    let meta = draft.file().meta;
    assert_eq!(meta.name.as_deref(), Some("Nord, warmer"));
    assert_eq!(meta.author, None);
    // A version that looks like a number is kept as the text it is.
    assert_eq!(meta.version.as_deref(), Some("2.0"));
    assert_eq!(meta.license.as_deref(), Some("CC-BY-4.0"));
    assert_eq!(meta.tags, ["warm", "evening"]);
    assert_eq!(draft.file().warnings, Vec::<String>::new());

    draft.set_tags(&[]);
    assert_eq!(draft.file().meta.tags, Vec::<String>::new());
    assert!(!draft.doc.contains(&[META_SECTION, TAGS_KEY]));
}

#[test]
fn a_meta_block_emptied_goes() {
    let fx = Fixture::new("meta-empty");
    fx.user_theme("mine", "meta:\n  name: X\ncolors:\n  base: \"#000000\"\n");
    let mut draft = ThemeDraft::open(&fx.dirs(), os("mine")).unwrap();
    draft.set_meta(MetaText::Name, " ");
    assert_eq!(draft.doc.to_text(), "colors:\n  base: \"#000000\"\n");
    draft.set_tags(&["a"]);
    draft.set_tags(&[]);
    assert_eq!(draft.doc.to_text(), "colors:\n  base: \"#000000\"\n");
}

#[test]
fn a_file_too_large_to_be_read_is_not_saved() {
    let fx = Fixture::new("too-large");
    fx.user_theme("mine", NORD);
    let mut draft = ThemeDraft::open(&fx.dirs(), os("mine")).unwrap();
    let long = "x".repeat(usize::try_from(MAX_FILE_BYTES).unwrap());
    draft.set_meta(MetaText::Name, &long);
    assert!(matches!(draft.save(), Err(AuthoringError::TooLarge(size)) if size > MAX_FILE_BYTES));
    assert!(draft.is_changed());
    assert_eq!(
        fs::read_to_string(fx.user().join("mine").join(FILE_NAME)).unwrap(),
        NORD
    );
}

#[test]
fn only_the_users_own_themes_open() {
    let fx = Fixture::new("open");
    system_nord(&fx);
    let dirs = fx.dirs();
    assert_eq!(
        ThemeDraft::open(&dirs, os(BUILT_IN)),
        Err(AuthoringError::NotTheUsers)
    );
    assert_eq!(
        ThemeDraft::open(&dirs, os("nord")),
        Err(AuthoringError::NotTheUsers)
    );
    assert_eq!(
        ThemeDraft::open(&dirs, os("missing")),
        Err(AuthoringError::NotInstalled)
    );
    assert_eq!(
        ThemeDraft::open(&dirs, os("../nord")),
        Err(AuthoringError::NotInstalled)
    );
    let nobody = ThemeDirs {
        user: None,
        system: fx.system(),
    };
    assert_eq!(
        ThemeDraft::open(&nobody, os("mine")),
        Err(AuthoringError::NoUserDirectory)
    );
}

/// An icon pack is a theme with no file yet: it opens empty, and a save
/// gives it one.
#[test]
fn a_theme_with_no_file_opens_empty_and_gains_one() {
    let fx = Fixture::new("icon-pack");
    fx.write(&fx.user().join("pack"), "icons/folder.svg", SQUARE);
    let mut draft = ThemeDraft::open(&fx.dirs(), os("pack")).unwrap();
    assert_eq!(draft.file(), ThemeFile::default());
    draft
        .set_color(ColorSection::Dark, "base", rgb(0x22_2222))
        .unwrap();
    draft.save().unwrap();
    let text = fs::read_to_string(fx.user().join("pack").join(FILE_NAME)).unwrap();
    assert_eq!(text, "colors:\n  base: \"#222222\"\n");
}

#[test]
fn a_file_that_is_not_text_does_not_open() {
    let fx = Fixture::new("not-text");
    fx.write(&fx.user().join("mine"), FILE_NAME, [0xFF_u8, 0xFE, 0x00]);
    assert_eq!(
        ThemeDraft::open(&fx.dirs(), os("mine")),
        Err(AuthoringError::Unreadable(ThemeError::NotText))
    );
}

// ---- reading ----

#[test]
fn any_installed_theme_is_read_as_the_desktop_reads_it() {
    let fx = Fixture::new("read");
    system_nord(&fx);
    fx.user_theme("mine", "colors:\n  base: \"#000000\"\n");
    fx.write(&fx.user().join("pack"), "icons/folder.svg", SQUARE);
    let dirs = fx.dirs();
    assert_eq!(read_theme(&dirs, os("nord")).unwrap(), parse(NORD));
    assert_eq!(
        read_theme(&dirs, os("mine")).unwrap().colors.dark["base"],
        rgb(0)
    );
    assert_eq!(
        read_theme(&dirs, os(BUILT_IN)).unwrap(),
        parse(BUILT_IN_TEMPLATE)
    );
    assert_eq!(read_theme(&dirs, os("pack")).unwrap(), ThemeFile::default());
    assert_eq!(
        read_theme(&dirs, os("missing")),
        Err(AuthoringError::NotInstalled)
    );
    // The user's copy is the one read, as the desktop reads it.
    fx.user_theme("nord", "meta:\n  name: My Nord\n");
    assert_eq!(
        read_theme(&dirs, os("nord")).unwrap().meta.name.as_deref(),
        Some("My Nord")
    );
}

// ---- deriving ----

#[test]
fn a_derived_theme_is_a_copy_under_its_new_name() {
    let fx = Fixture::new("derive");
    let source = system_nord(&fx);
    let dirs = fx.dirs();

    let (draft, left_out) = derive(&dirs, os("nord"), os("nord-warm"), "Nord, warmer").unwrap();
    assert_eq!(left_out, Vec::new(), "{}", listing(&left_out));
    assert_eq!(draft.id(), os("nord-warm"));
    let dir = fx.user().join("nord-warm");
    assert_eq!(draft.dir(), dir);
    assert!(!draft.is_changed());

    let text = fs::read_to_string(dir.join(FILE_NAME)).unwrap();
    // The text is the source's, comments and all, with the new name and
    // without the screenshots.
    assert!(text.starts_with("# Nord, for a cold night.\n"), "{text}");
    assert!(text.contains("base: \"#2e3440\"  # the page"), "{text}");
    let file = parse(&text);
    assert_eq!(file.meta.name.as_deref(), Some("Nord, warmer"));
    assert_eq!(file.meta.author.as_deref(), Some("Someone"));
    assert_eq!(file.meta.license.as_deref(), Some("MIT"));
    assert_eq!(file.meta.screenshots, Vec::<String>::new());
    assert_eq!(file.colors, parse(NORD).colors);

    // The icon and the wallpaper came; the screenshot that is only a
    // screenshot did not.
    assert_eq!(
        fs::read_to_string(dir.join("icons/folder.svg")).unwrap(),
        SQUARE
    );
    assert_eq!(fs::read(dir.join("wallpapers/night.png")).unwrap(), png());
    assert!(!dir.join("dark.png").exists());

    // The source is as it was.
    assert_eq!(fs::read_to_string(source.join(FILE_NAME)).unwrap(), NORD);
    assert!(source.join("dark.png").is_file());

    // It is listed as the user's, under its new name, covering what Nord did.
    let listed = available_in(&dirs);
    let entry = listed.iter().find(|info| info.id == "nord-warm").unwrap();
    assert_eq!(entry.origin, Origin::User);
    assert_eq!(entry.name, "Nord, warmer");
    assert!(entry.provides_colors());
    assert!(entry.provides_icons());
    assert!(entry.provides_wallpapers());
    // Nothing else was left in the user's directory.
    assert_eq!(fx.user_entries(), ["nord-warm"]);
}

#[test]
fn a_derived_theme_named_blank_shows_its_folders_name() {
    let fx = Fixture::new("derive-blank");
    system_nord(&fx);
    let (draft, _) = derive(&fx.dirs(), os("nord"), os("cold"), "  ").unwrap();
    assert_eq!(draft.file().meta.name, None);
    let listed = available_in(&fx.dirs());
    assert_eq!(
        listed.iter().find(|info| info.id == "cold").unwrap().name,
        "cold"
    );
}

/// The built-in theme's copy is its template -- every colour, control,
/// frame and panel value the desktop has compiled in -- and its icons, and
/// passes the checker as it stands.
#[test]
fn a_theme_derived_from_the_built_in_one_is_its_template_and_its_icons() {
    let fx = Fixture::new("derive-built-in");
    let (draft, left_out) = derive(&fx.dirs(), os(BUILT_IN), os("mine"), "Mine").unwrap();
    assert_eq!(left_out, Vec::new());
    let dir = fx.user().join("mine");
    let text = fs::read_to_string(dir.join(FILE_NAME)).unwrap();

    // The opening comment is the copy's, not the built-in theme's.
    assert!(
        text.starts_with("# A theme made from Aero, SlateOS's built-in theme.\n"),
        "{text}"
    );
    assert!(!text.contains("compiled into it"), "{text}");
    assert!(!text.contains("Tests hold them equal"), "{text}");
    // The comments inside the sections stay: they are true of a copy.
    assert!(
        text.contains("# Backgrounds, darkest to lightest"),
        "{text}"
    );

    let copy = parse(&text);
    let template = parse(BUILT_IN_TEMPLATE);
    assert_eq!(copy.meta.name.as_deref(), Some("Mine"));
    assert_eq!(copy.colors, template.colors);
    assert_eq!(copy.widget_style, template.widget_style);
    assert_eq!(copy.motion, template.motion);
    assert_eq!(copy.decorations, template.decorations);
    assert_eq!(copy.panel, template.panel);
    assert_eq!(copy.warnings, Vec::<String>::new());
    assert_eq!(draft.file(), copy);

    // One file for each icon the built-in theme draws, holding its picture.
    for (name, svg) in icons::built_in_icons() {
        let icon = dir.join(ICONS_DIR).join(format!("{name}.svg"));
        assert_eq!(fs::read_to_string(&icon).unwrap(), *svg, "{name}");
    }
    let count = fs::read_dir(dir.join(ICONS_DIR)).unwrap().count();
    assert_eq!(count, icons::built_in_icons().len());

    let report = themecheck::check(&dir);
    assert!(report.passes(), "{}", listing(&report.findings));
    assert!(
        !report
            .findings
            .iter()
            .any(|finding| finding.place.starts_with(ICONS_DIR)),
        "{}",
        listing(&report.findings)
    );
}

/// A system theme copied under its own name stands in for it, as a user's
/// copy does.
#[test]
fn a_system_theme_copied_under_its_own_name_stands_in_for_it() {
    let fx = Fixture::new("stand-in");
    system_nord(&fx);
    let dirs = fx.dirs();
    derive(&dirs, os("nord"), os("nord"), "Nord").unwrap();
    let listed = available_in(&dirs);
    let nords: Vec<_> = listed.iter().filter(|info| info.id == "nord").collect();
    assert_eq!(nords.len(), 1);
    assert_eq!(nords[0].origin, Origin::User);
    // And now that it is the user's, a second copy under the name is refused.
    assert_eq!(
        derive(&dirs, os("nord"), os("nord"), "Nord").map(|_| ()),
        Err(AuthoringError::Exists)
    );
}

#[test]
fn a_derived_theme_needs_a_source_and_a_free_name() {
    let fx = Fixture::new("derive-names");
    system_nord(&fx);
    fx.system_theme("solar", "colors:\n  base: \"#002b36\"\n");
    fx.user_theme("mine", NORD);
    let dirs = fx.dirs();
    let derived = |from: &str, id: &str| derive(&dirs, os(from), os(id), "X").map(|_| ());

    assert_eq!(derived("missing", "new"), Err(AuthoringError::NotInstalled));
    assert_eq!(derived("../nord", "new"), Err(AuthoringError::NotInstalled));
    // Taken by the user, or -- under another theme's name -- by the system:
    // a copy of Nord must not hide Solarized.
    assert_eq!(derived("nord", "mine"), Err(AuthoringError::Exists));
    assert_eq!(derived("nord", "solar"), Err(AuthoringError::Exists));
    // Not a name a theme can have.
    assert_eq!(derived("nord", BUILT_IN), Err(AuthoringError::InvalidName));
    assert_eq!(derived("nord", ".hidden"), Err(AuthoringError::InvalidName));
    assert_eq!(derived("nord", "a/b"), Err(AuthoringError::InvalidName));
    assert_eq!(derived("nord", ""), Err(AuthoringError::InvalidName));
    // A user's theme can be derived from too.
    assert_eq!(derived("mine", "mine-2"), Ok(()));
    assert_eq!(fx.user_entries(), ["mine", "mine-2"]);
}

#[test]
fn a_failed_derive_leaves_nothing_behind() {
    let fx = Fixture::new("derive-fails");
    let big = "#".repeat(usize::try_from(MAX_FILE_BYTES).unwrap() + 1);
    fx.system_theme("big", &big);
    let result = derive(&fx.dirs(), os("big"), os("copy"), "Copy").map(|_| ());
    assert!(
        matches!(
            result,
            Err(AuthoringError::Unreadable(ThemeError::TooLarge(_)))
        ),
        "{result:?}"
    );
    assert_eq!(fx.user_entries(), Vec::<OsString>::new());
}

// ---- installing ----

#[test]
fn a_theme_folder_is_installed_once_its_copy_passes_the_check() {
    let fx = Fixture::new("install");
    let from = fx.outside("ocean");
    fx.write(&from, FILE_NAME, NORD);
    fx.write(&from, "icons/folder.svg", SQUARE);
    fx.write(&from, "dark.png", png());
    fx.write(&from, "wallpapers/night.png", png());
    fx.write(&from, "README.md", "An ocean.");

    let report = install(&fx.dirs(), &from, os("ocean")).unwrap();
    assert!(report.passes(), "{}", listing(&report.findings));
    let dir = fx.user().join("ocean");
    assert_eq!(fs::read_to_string(dir.join(FILE_NAME)).unwrap(), NORD);
    assert_eq!(
        fs::read_to_string(dir.join("icons/folder.svg")).unwrap(),
        SQUARE
    );
    // A screenshot stays on install: it pictures the theme installed.
    assert_eq!(fs::read(dir.join("dark.png")).unwrap(), png());
    assert!(dir.join("README.md").is_file());
    assert_eq!(fx.user_entries(), ["ocean"]);
    // The source is untouched.
    assert!(from.join(FILE_NAME).is_file());

    // Installed: a second install under the name is refused.
    assert_eq!(
        install(&fx.dirs(), &from, os("ocean")),
        Err(AuthoringError::Exists)
    );
}

#[test]
fn a_theme_that_fails_its_check_is_refused_and_nothing_is_installed() {
    let fx = Fixture::new("install-refused");
    let from = fx.outside("bad");
    fx.write(&from, FILE_NAME, NORD);
    fx.write(&from, "run.sh", "#!/bin/sh\necho hello\n");

    let Err(AuthoringError::Refused(report)) = install(&fx.dirs(), &from, os("bad")) else {
        panic!("a theme holding a script was installed");
    };
    assert!(!report.passes());
    assert!(
        report
            .findings
            .iter()
            .any(|finding| finding.severity == Severity::Error && finding.place == "run.sh"),
        "{}",
        listing(&report.findings)
    );
    assert_eq!(fx.user_entries(), Vec::<OsString>::new());
}

#[test]
fn a_lone_theme_file_is_installed_as_a_folders_file() {
    let fx = Fixture::new("install-file");
    let from = fx.outside("nord.yaml");
    fx.write(from.parent().unwrap(), "nord.yaml", NORD);
    // Its screenshots and wallpaper are not there, which the check says;
    // nothing in that is an error.
    let report = install(&fx.dirs(), &from, os("nord")).unwrap();
    assert!(report.passes(), "{}", listing(&report.findings));
    assert_eq!(
        fs::read_to_string(fx.user().join("nord").join(FILE_NAME)).unwrap(),
        NORD
    );
    assert_eq!(fx.user_entries(), ["nord"]);
}

#[test]
fn a_lone_file_too_large_to_be_a_theme_is_not_installed() {
    let fx = Fixture::new("install-large");
    let from = fx.outside("big.yaml");
    let big = "#".repeat(usize::try_from(MAX_FILE_BYTES).unwrap() + 1);
    fx.write(from.parent().unwrap(), "big.yaml", big);
    assert_eq!(
        install(&fx.dirs(), &from, os("big")),
        Err(AuthoringError::TooLarge(MAX_FILE_BYTES + 1))
    );
    assert_eq!(fx.user_entries(), Vec::<OsString>::new());
}

#[test]
fn an_install_needs_something_to_install_and_a_name() {
    let fx = Fixture::new("install-names");
    let from = fx.outside("ocean");
    fx.write(&from, FILE_NAME, NORD);
    let dirs = fx.dirs();
    assert!(matches!(
        install(&dirs, &fx.outside("missing"), os("x")),
        Err(AuthoringError::Io(_))
    ));
    assert_eq!(
        install(&dirs, &from, os(BUILT_IN)),
        Err(AuthoringError::InvalidName)
    );
    assert_eq!(
        install(&dirs, &from, os(".ocean")),
        Err(AuthoringError::InvalidName)
    );
    // Under a system theme's name it stands in for that theme.
    fx.system_theme("ocean", "colors:\n  base: \"#000000\"\n");
    install(&dirs, &from, os("ocean")).unwrap();
    let listed = available_in(&dirs);
    let entry = listed.iter().find(|info| info.id == "ocean").unwrap();
    assert_eq!(entry.origin, Origin::User);
    assert_eq!(entry.name, "Nord");
}

/// What is hidden is no part of a theme: left out of the copy, and said.
#[test]
fn a_copy_leaves_out_what_is_hidden_and_says_so() {
    let fx = Fixture::new("hidden");
    let from = fx.outside("ocean");
    fx.write(&from, FILE_NAME, NORD);
    fx.write(&from, ".DS_Store", "junk");
    fx.write(&from, ".git/HEAD", "ref: refs/heads/main\n");
    fx.write(&from, "icons/.cache", "junk");

    let report = install(&fx.dirs(), &from, os("ocean")).unwrap();
    for place in [".DS_Store", ".git", "icons/.cache"] {
        assert!(
            report.findings.iter().any(|finding| {
                finding.severity == Severity::Warning
                    && finding.place == place
                    && finding.message.contains("hidden")
            }),
            "{place}: {}",
            listing(&report.findings)
        );
    }
    let dir = fx.user().join("ocean");
    assert!(!dir.join(".DS_Store").exists());
    assert!(!dir.join(".git").exists());
    assert!(!dir.join("icons/.cache").exists());
    // The copy's findings come first among the warnings.
    let first = report
        .findings
        .iter()
        .find(|finding| finding.severity == Severity::Warning)
        .unwrap();
    assert!(
        first.message.contains("hidden"),
        "{}",
        listing(&report.findings)
    );
}

#[test]
fn a_copy_stops_at_the_checkers_depth() {
    let fx = Fixture::new("deep");
    let from = fx.outside("ocean");
    fx.write(&from, FILE_NAME, NORD);
    let deep = (0..MAX_DEPTH)
        .map(|n| format!("d{n}"))
        .collect::<Vec<_>>()
        .join("/");
    fx.write(&from, &format!("{deep}/x.txt"), "x");
    let Err(AuthoringError::Refused(report)) = install(&fx.dirs(), &from, os("ocean")) else {
        panic!("a theme deeper than any was installed");
    };
    assert!(
        report
            .findings
            .iter()
            .any(|finding| finding.severity == Severity::Error
                && finding.message.contains("folders down")
                && finding.message.contains("not copied")),
        "{}",
        listing(&report.findings)
    );
    assert_eq!(fx.user_entries(), Vec::<OsString>::new());
}

#[test]
fn a_file_larger_than_any_in_a_theme_is_not_copied() {
    let fx = Fixture::new("copy-large");
    let from = fx.outside("x");
    let to = fx.outside("copy");
    fs::create_dir_all(&to).unwrap();
    fx.write(&from, "big.bin", vec![0_u8; 64]);
    assert!(matches!(
        copy_file(&from.join("big.bin"), &to.join("big.bin"), 63),
        Ok(Copied::TooLarge(64))
    ));
    assert!(!to.join("big.bin").exists());
    assert!(matches!(
        copy_file(&from.join("big.bin"), &to.join("big.bin"), 64),
        Ok(Copied::Whole)
    ));
    assert_eq!(fs::read(to.join("big.bin")).unwrap(), vec![0_u8; 64]);
    // The copy is a new file: one already there is not written over.
    assert!(matches!(
        copy_file(&from.join("big.bin"), &to.join("big.bin"), 64),
        Err(CopyFailure::Write(_))
    ));
    assert!(matches!(
        copy_file(&from.join("missing"), &to.join("other"), 64),
        Err(CopyFailure::Read(_))
    ));
}

/// A link to a file inside the folder is kept as a second name for it; one
/// leading out is not followed, and a theme with one is not installed.
#[cfg(unix)]
#[test]
fn a_link_inside_is_kept_and_one_leading_out_is_not_followed() {
    use std::os::unix::fs::{MetadataExt, symlink};

    let fx = Fixture::new("links");
    let from = fx.outside("cursors-theme");
    fx.write(&from, FILE_NAME, NORD);
    fx.write(&from, "icons/folder.svg", SQUARE);
    symlink("folder.svg", from.join("icons/folder-open.svg")).unwrap();

    let report = install(&fx.dirs(), &from, os("linked")).unwrap();
    assert!(report.passes(), "{}", listing(&report.findings));
    let dir = fx.user().join("linked");
    let kept = dir.join("icons/folder-open.svg");
    assert_eq!(fs::read_to_string(&kept).unwrap(), SQUARE);
    // A second name for the one copy, not a link and not a second copy.
    assert!(
        !fs::symlink_metadata(&kept)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        fs::metadata(&kept).unwrap().ino(),
        fs::metadata(dir.join("icons/folder.svg")).unwrap().ino()
    );

    // A link out of the folder -- to a file of the user's -- is not followed.
    let secret = fx.outside("secret.txt");
    fx.write(secret.parent().unwrap(), "secret.txt", "not for sharing");
    let leaky = fx.outside("leaky");
    fx.write(&leaky, FILE_NAME, NORD);
    symlink(&secret, leaky.join("notes.txt")).unwrap();
    let Err(AuthoringError::Refused(report)) = install(&fx.dirs(), &leaky, os("leaky")) else {
        panic!("a theme with a link leading out was installed");
    };
    assert!(
        report
            .findings
            .iter()
            .any(|finding| finding.severity == Severity::Error
                && finding.place == "notes.txt"
                && finding.message.contains("out of the folder")),
        "{}",
        listing(&report.findings)
    );
    assert_eq!(fx.user_entries(), ["linked"]);

    // Exported, it is left out of the copy, and said.
    let out = fx.outside("shared");
    fs::create_dir_all(&out).unwrap();
    fx.write(&fx.user().join("mine"), FILE_NAME, NORD);
    symlink(&secret, fx.user().join("mine").join("notes.txt")).unwrap();
    let report = export(&fx.dirs(), os("mine"), &out.join("mine")).unwrap();
    assert!(!out.join("mine").join("notes.txt").exists());
    assert!(
        report
            .findings
            .iter()
            .any(|finding| finding.place == "notes.txt"
                && finding.message.contains("out of the folder")),
        "{}",
        listing(&report.findings)
    );
}

/// A file that may be run is copied as bytes: the mark does not come too.
#[cfg(unix)]
#[test]
fn a_copy_does_not_carry_a_mark_to_run() {
    use std::os::unix::fs::PermissionsExt;

    let fx = Fixture::new("modes");
    let from = fx.outside("ocean");
    fx.write(&from, FILE_NAME, NORD);
    fx.write(&from, "icons/folder.svg", SQUARE);
    let icon = from.join("icons/folder.svg");
    fs::set_permissions(&icon, fs::Permissions::from_mode(0o755)).unwrap();
    install(&fx.dirs(), &from, os("ocean")).unwrap();
    let copied = fx.user().join("ocean").join("icons/folder.svg");
    assert_eq!(
        fs::metadata(copied).unwrap().permissions().mode() & 0o111,
        0
    );
}

/// A junction -- Windows' link to a folder, which any user may make -- is
/// a link to a folder, and one leading out of the theme is not followed.
#[cfg(windows)]
#[test]
fn a_link_to_a_folder_is_not_followed() {
    let fx = Fixture::new("junction");
    let elsewhere = fx.outside("elsewhere");
    fx.write(&elsewhere, "secret.txt", "not for sharing");
    let from = fx.outside("ocean");
    fx.write(&from, FILE_NAME, NORD);
    let made = std::process::Command::new("cmd")
        .arg("/C")
        .arg("mklink")
        .arg("/J")
        .arg(from.join("extras"))
        .arg(&elsewhere)
        .output()
        .unwrap();
    assert!(made.status.success(), "{made:?}");
    let Err(AuthoringError::Refused(report)) = install(&fx.dirs(), &from, os("ocean")) else {
        panic!("a theme with a link leading out was installed");
    };
    assert!(
        report
            .findings
            .iter()
            .any(|finding| finding.severity == Severity::Error
                && finding.place == "extras"
                && finding.message.contains("out of the folder")),
        "{}",
        listing(&report.findings)
    );
    assert_eq!(fx.user_entries(), Vec::<OsString>::new());
    // Nothing was read through it into the copy -- and nothing outside was
    // touched.
    assert!(elsewhere.join("secret.txt").is_file());
}

// ---- exporting ----

#[test]
fn a_theme_is_copied_out_to_share_and_checked() {
    let fx = Fixture::new("export");
    let source = system_nord(&fx);
    let out = fx.outside("shared");
    fs::create_dir_all(&out).unwrap();
    let to = out.join("nord");

    let report = export(&fx.dirs(), os("nord"), &to).unwrap();
    assert!(report.passes(), "{}", listing(&report.findings));
    // As it is: screenshots and all.
    assert_eq!(fs::read_to_string(to.join(FILE_NAME)).unwrap(), NORD);
    assert_eq!(fs::read(to.join("dark.png")).unwrap(), png());
    assert_eq!(
        fs::read_to_string(to.join("icons/folder.svg")).unwrap(),
        SQUARE
    );
    assert_eq!(fs::read_to_string(source.join(FILE_NAME)).unwrap(), NORD);
    // Only the copy is in the folder: nothing it was built in is left.
    let names: Vec<_> = fs::read_dir(&out)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names, [OsString::from("nord")]);

    // Nothing at the destination is replaced or merged into.
    assert_eq!(
        export(&fx.dirs(), os("nord"), &to),
        Err(AuthoringError::Exists)
    );
    assert_eq!(
        export(&fx.dirs(), os("missing"), &out.join("x")),
        Err(AuthoringError::NotInstalled)
    );
    assert!(matches!(
        export(&fx.dirs(), os("nord"), &fx.outside("no/such/folder/nord")),
        Err(AuthoringError::Io(_))
    ));
}

#[test]
fn the_built_in_theme_is_copied_out_as_its_template_and_icons() {
    let fx = Fixture::new("export-built-in");
    let out = fx.outside("shared");
    fs::create_dir_all(&out).unwrap();
    let to = out.join("aero");
    let report = export(&fx.dirs(), os(BUILT_IN), &to).unwrap();
    assert!(report.passes(), "{}", listing(&report.findings));
    assert_eq!(
        fs::read_to_string(to.join(FILE_NAME)).unwrap(),
        BUILT_IN_TEMPLATE
    );
    assert_eq!(
        fs::read_dir(to.join(ICONS_DIR)).unwrap().count(),
        icons::built_in_icons().len()
    );
}

// ---- removing ----

#[test]
fn a_users_theme_is_removed_and_no_other_is() {
    let fx = Fixture::new("remove");
    system_nord(&fx);
    let mine = fx.user_theme("mine", NORD);
    fx.write(&mine, "icons/folder.svg", SQUARE);
    let dirs = fx.dirs();

    remove(&dirs, os("mine")).unwrap();
    assert!(!mine.exists());
    assert_eq!(fx.user_entries(), Vec::<OsString>::new());
    assert!(!available_in(&dirs).iter().any(|info| info.id == "mine"));

    assert_eq!(remove(&dirs, os("mine")), Err(AuthoringError::NotInstalled));
    assert_eq!(remove(&dirs, os("nord")), Err(AuthoringError::NotTheUsers));
    assert_eq!(
        remove(&dirs, os(BUILT_IN)),
        Err(AuthoringError::NotTheUsers)
    );
    assert_eq!(remove(&dirs, os("..")), Err(AuthoringError::NotInstalled));
    assert!(fx.system().join("nord").join(FILE_NAME).is_file());
}

/// A user's copy standing in for a system theme, removed, gives the system's
/// back.
#[test]
fn removing_a_stand_in_brings_back_the_system_theme() {
    let fx = Fixture::new("remove-stand-in");
    system_nord(&fx);
    fx.user_theme(
        "nord",
        "meta:\n  name: My Nord\ncolors:\n  base: \"#000000\"\n",
    );
    let dirs = fx.dirs();
    remove(&dirs, os("nord")).unwrap();
    let listed = available_in(&dirs);
    let entry = listed.iter().find(|info| info.id == "nord").unwrap();
    assert_eq!(entry.origin, Origin::System);
    assert_eq!(entry.name, "Nord");
}

// ---- names ----

#[test]
fn a_new_themes_name_is_its_own_where_it_can_be() {
    let fx = Fixture::new("names");
    fx.user_theme("mine", NORD);
    fx.user_theme("mine-2", NORD);
    fx.system_theme("solar", NORD);
    let dirs = fx.dirs();
    let id = |name: &str| id_for(&dirs, name).into_string().unwrap();

    assert_eq!(id("Nord, warmer"), "Nord, warmer");
    assert_eq!(id("  Ocean  "), "Ocean");
    assert_eq!(id("a/b\\c"), "a-b-c");
    assert_eq!(id("tab\there"), "tab-here");
    assert_eq!(id("...dotted"), "dotted");
    // Dots and spaces in any order: what is left must not be hidden either.
    assert_eq!(id(". . ."), "theme");
    assert_eq!(id(" . .x"), "x");
    assert_eq!(id(""), "theme");
    assert_eq!(id("..."), "theme");
    // Taken by the built-in theme, the user, or the system: the next free.
    assert_eq!(id(BUILT_IN), "aero-2");
    assert_eq!(id("mine"), "mine-3");
    assert_eq!(id("solar"), "solar-2");
    // Long: cut on a character's boundary.
    let long = "é".repeat(150);
    let cut = id(&long);
    assert!(cut.len() <= MAX_ID_BYTES, "{}", cut.len());
    assert_eq!(cut, "é".repeat(MAX_ID_BYTES / 2));
    // Whatever it makes is a name derive takes.
    for name in [
        "Nord, warmer",
        "a/b\\c",
        "",
        "solar",
        BUILT_IN,
        ". . .",
        " .\t.",
    ] {
        let made = id_for(&dirs, name);
        assert!(new_theme_place(&dirs, &made, false).is_ok(), "{made:?}");
    }
}

/// With nowhere to put a theme, no name is free; the search ends, and the
/// name it gives is refused for the reason that matters.
#[test]
fn a_new_themes_name_is_found_even_with_nowhere_to_put_it() {
    let fx = Fixture::new("names-nowhere");
    let nobody = ThemeDirs {
        user: None,
        system: fx.system(),
    };
    let made = id_for(&nobody, "Mine");
    assert_eq!(made, "Mine");
    assert_eq!(
        derive(&nobody, os(BUILT_IN), &made, "Mine").map(|_| ()),
        Err(AuthoringError::NoUserDirectory)
    );
}

// ---- errors ----

#[test]
fn every_error_says_what_went_wrong() {
    let report = Report {
        findings: vec![
            Finding {
                severity: Severity::Error,
                place: "run.sh".to_owned(),
                message: "is a script".to_owned(),
            },
            Finding {
                severity: Severity::Error,
                place: String::new(),
                message: "sets nothing".to_owned(),
            },
        ],
        ..Report::default()
    };
    let errors = [
        AuthoringError::InvalidName,
        AuthoringError::NoUserDirectory,
        AuthoringError::Exists,
        AuthoringError::NotInstalled,
        AuthoringError::NotTheUsers,
        AuthoringError::NoSuchRole(ColorSection::Dark, "accent".to_owned()),
        AuthoringError::Translucent(Color::rgba(1, 2, 3, 4)),
        AuthoringError::TooLarge(MAX_FILE_BYTES + 1),
        AuthoringError::Unreadable(ThemeError::NotText),
        AuthoringError::Io("could not write x (denied)".to_owned()),
        AuthoringError::Refused(Box::new(report)),
    ];
    let said: Vec<String> = errors.iter().map(ToString::to_string).collect();
    for (error, text) in errors.iter().zip(&said) {
        assert!(!text.is_empty(), "{error:?}");
    }
    assert!(
        said[5].contains("`colors`") && said[5].contains("`accent`"),
        "{}",
        said[5]
    );
    assert!(said[6].contains("#01020304"), "{}", said[6]);
    assert_eq!(said[8], "the theme has a file that is not text");
    assert_eq!(
        said[10],
        "the theme did not pass its check: 2 errors, the first: error: run.sh: is a script"
    );
}

// ---- one theme from the user's mix ----

/// A file with comments above and inside its sections, a blank line in one,
/// and sections one after another.
const MIXED: &str = "\
# Nord, for a cold night.
meta:
  name: Nord
# The page and the ink.
colors:
  base: \"#2e3440\"  # the page

  text: \"#eceff4\"

terminal:
  red: \"#bf616a\"
widget-style:
  button:
    radius: 9
";

/// **A section is taken as its theme writes it**: the comments directly
/// above it, every line inside it -- blank ones and comments included --
/// and nothing after it.
#[test]
fn a_sections_text_is_taken_as_written() {
    let section = |name: &str| section_text(MIXED, name);
    assert_eq!(
        section("colors").as_deref(),
        Some(
            "# The page and the ink.\ncolors:\n  base: \"#2e3440\"  # the page\n\n  text: \"#eceff4\"\n"
        )
    );
    assert_eq!(
        section("terminal").as_deref(),
        Some("terminal:\n  red: \"#bf616a\"\n")
    );
    assert_eq!(
        section("widget-style").as_deref(),
        Some("widget-style:\n  button:\n    radius: 9\n")
    );
    assert_eq!(
        section("meta").as_deref(),
        Some("# Nord, for a cold night.\nmeta:\n  name: Nord\n")
    );
    assert_eq!(section("syntax"), None);
    // Line ends are made `\n`; a quoted key is found by its name.
    assert_eq!(
        section_text("colors:\r\n  base: \"#000000\"\r\n", "colors").as_deref(),
        Some("colors:\n  base: \"#000000\"\n")
    );
    assert_eq!(
        section_text("\"colors\":\n  base: \"#000000\"\n", "colors").as_deref(),
        Some("\"colors\":\n  base: \"#000000\"\n")
    );
    // A key inside another section is not a section.
    assert_eq!(section_text("meta:\n  colors: x\n", "colors"), None);
}

/// **A theme is put together from the user's mix**: each axis as the theme
/// chosen for it writes it, the built-in theme's from its template, the
/// wallpaper theme's pictures and the icon theme's icons copied in -- and
/// what it covers claimed as the checker reads it.
#[test]
fn a_theme_is_put_together_from_the_users_mix() {
    let fx = Fixture::new("compose");
    let dirs = fx.dirs();
    fx.system_theme("nord", MIXED);
    fx.user_theme(
        "round",
        "meta:\n  name: Round\nwidget-style:\n  button:\n    radius: 14   # a pill\n",
    );
    fx.user_theme("fonty", "fonts:\n  ui: [Inter, Noto Sans]\n");
    let pics = fx.user_theme(
        "pics",
        "meta:\n  name: Pictures\nwallpapers:\n  dark: night.png\n  light: shots/day.png\n",
    );
    fx.write(&pics, "night.png", png());
    fx.write(&pics, "shots/day.png", png());
    fx.write(&fx.user().join("lines"), "icons/folder.svg", SQUARE);
    let settings = AppearanceSettings {
        color_theme: ColorTheme::load_from(&dirs, os("nord")),
        widget_theme: WidgetTheme::load_from(&dirs, os("round")),
        font_theme: FontTheme::load_from(&dirs, os("fonty")),
        wallpaper_theme: WallpaperTheme::load_from(&dirs, os("pics")),
        icon_theme: IconTheme::named(os("lines"), dirs.clone()),
        cursor_theme: CursorTheme::load(os("Adwaita")),
        ..AppearanceSettings::default()
    };

    let (draft, said) = compose(&dirs, &settings, os("my-look"), "My look").unwrap();
    assert_eq!(draft.id(), os("my-look"));
    let dir = fx.user().join("my-look");
    let text = fs::read_to_string(dir.join(FILE_NAME)).unwrap();

    // Each section as its theme writes it, under a line saying whose.
    for section in [DARK_SECTION, TERMINAL_DARK_SECTION] {
        assert!(
            text.contains(&section_text(MIXED, section).unwrap()),
            "{section}: {text}"
        );
    }
    assert!(text.contains("# The colours: Nord's.\n"), "{text}");
    assert!(text.contains("# The controls: Round's.\n"), "{text}");
    assert!(text.contains("    radius: 14   # a pill\n"), "{text}");
    assert!(
        !text.contains("radius: 9"),
        "Nord's controls were taken: {text}"
    );
    for section in [ANIMATION_SECTION, DECORATIONS_SECTION, PANEL_SECTION] {
        assert!(
            text.contains(&section_text(BUILT_IN_TEMPLATE, section).unwrap()),
            "{section}: {text}"
        );
    }
    assert!(
        text.contains("fonts:\n  ui: [Inter, Noto Sans]\n"),
        "{text}"
    );

    let file = parse(&text);
    assert_eq!(file.warnings, Vec::<String>::new());
    assert_eq!(file.meta.name.as_deref(), Some("My look"));
    assert_eq!(file.colors, parse(MIXED).colors);
    assert_eq!(
        file.widget_style,
        parse("widget-style:\n  button:\n    radius: 14\n").widget_style
    );
    assert_eq!(file.motion, parse(BUILT_IN_TEMPLATE).motion);
    assert_eq!(file.decorations, parse(BUILT_IN_TEMPLATE).decorations);
    assert_eq!(file.panel, parse(BUILT_IN_TEMPLATE).panel);

    // The pictures, named for their modes, and the icons.
    assert_eq!(fs::read(dir.join("wallpapers/dark.png")).unwrap(), png());
    assert_eq!(fs::read(dir.join("wallpapers/light.png")).unwrap(), png());
    let names = file.wallpapers.clone().unwrap();
    assert_eq!(names.dark.as_deref(), Some("wallpapers/dark.png"));
    assert_eq!(names.light.as_deref(), Some("wallpapers/light.png"));
    assert_eq!(
        fs::read_to_string(dir.join("icons/folder.svg")).unwrap(),
        SQUARE
    );

    // What it claims to cover is what the checker finds it covers.
    let report = themecheck::check(&dir);
    assert!(report.passes(), "{}", listing(&report.findings));
    assert_eq!(file.meta.supports, report.covers);
    for axis in [
        "colors",
        "widget-style",
        "animation",
        "wallpapers",
        "fonts",
        "icons",
    ] {
        assert!(report.covers.contains(&axis), "{axis}: {:?}", report.covers);
    }

    // The cursors stay a theme of their own, and say so; the sounds are
    // the built-in ones, and nothing is said of them.
    assert!(
        said.iter()
            .any(|f| f.message.contains("cursors") && f.message.contains("Adwaita")),
        "{}",
        listing(&said)
    );
    assert!(!said.iter().any(|f| f.message.contains("sounds")));
    assert_eq!(
        fx.user_entries(),
        ["fonty", "lines", "my-look", "pics", "round"]
    );
}

/// **The built-in theme's look, put together, is its template and its
/// icons**; and a theme chosen that is not installed is the built-in one,
/// as the desktop shows it, which is said.
#[test]
fn the_built_in_look_and_a_missing_theme_are_the_template() {
    let fx = Fixture::new("compose-built-in");
    let dirs = fx.dirs();
    let (_, said) = compose(&dirs, &AppearanceSettings::default(), os("plain"), "Plain").unwrap();
    assert_eq!(said, Vec::new(), "{}", listing(&said));
    let dir = fx.user().join("plain");
    let file = parse(&fs::read_to_string(dir.join(FILE_NAME)).unwrap());
    let template = parse(BUILT_IN_TEMPLATE);
    assert_eq!(file.colors, template.colors);
    assert_eq!(file.widget_style, template.widget_style);
    assert_eq!(file.motion, template.motion);
    assert_eq!(file.wallpapers, None);
    assert_eq!(file.fonts, None);
    assert_eq!(
        fs::read_dir(dir.join(ICONS_DIR)).unwrap().count(),
        icons::built_in_icons().len()
    );
    assert!(themecheck::check(&dir).passes());

    let gone = AppearanceSettings {
        color_theme: ColorTheme::load_from(&dirs, os("gone")),
        ..AppearanceSettings::default()
    };
    let (draft, said) = compose(&dirs, &gone, os("gone-look"), "").unwrap();
    assert_eq!(draft.file().colors, template.colors);
    assert_eq!(draft.file().meta.name, None, "a blank name: the folder's");
    assert!(
        said.iter().any(|f| f.severity == Severity::Note
            && f.message.contains("colours")
            && f.message.contains("gone")
            && f.message.contains("not installed")),
        "{}",
        listing(&said)
    );
}

/// **A theme put together needs a name of its own**, and leaves nothing
/// behind without one.
#[test]
fn a_theme_put_together_needs_a_free_name() {
    let fx = Fixture::new("compose-names");
    fx.user_theme("mine", NORD);
    fx.system_theme("solar", NORD);
    let dirs = fx.dirs();
    let made = |id: &str| compose(&dirs, &AppearanceSettings::default(), os(id), "X").map(|_| ());
    assert_eq!(made("mine"), Err(AuthoringError::Exists));
    // Never standing in for a system theme: it is a theme of its own.
    assert_eq!(made("solar"), Err(AuthoringError::Exists));
    assert_eq!(made(BUILT_IN), Err(AuthoringError::InvalidName));
    assert_eq!(fx.user_entries(), ["mine"]);
}
