//! A theme's widget-style axis: its `widget-style` section, read, and the
//! widget style in use ([`WidgetTheme`]).
//!
//! The section chooses the shapes of the toolkit's controls
//! ([`guitk::widget_style`]); the user picks which theme's shapes to use as
//! `theme.widget_style` in `appearance.yaml`, independently of `theme.colors`
//! -- one theme's colours with another's controls, the mix-and-match every
//! axis allows. `design-decisions.md` §1435.
//!
//! ```yaml
//! widget-style:
//!   button:
//!     radius: 4          # 0 to 14 pixels; 14 is a pill
//!     padding: 14        # room either side of the label: 4 to 24 pixels
//!     gloss: true        # the upper half of the face a shade brighter
//!     shadow: false      # a soft shadow under the button
//!   field:
//!     radius: 3          # 0 to 13
//!     border: box        # box | underline
//!     focus: glow        # ring | glow | underline
//!   check:
//!     radius: 2          # 0 to 7; 7 is a circle
//!   toggle: pill         # pill | checkbox
//!   scrollbar:
//!     width: normal      # thin | normal
//!     visibility: always # always | overlay
//! ```
//!
//! As with colours, a value the section leaves out keeps the built-in
//! theme's, and a value that cannot be understood costs that value and is
//! listed for the theme's author; the rest of the section is used. A radius
//! past the roundest a control can be is drawn as the roundest, since that is
//! plainly what was meant.

use super::values::{at, read_choice, read_flag, value_of};
use super::{
    BUILT_IN, FILE_NAME, ThemeDirs, ThemeError, ThemeFile, WIDGET_SECTION as SECTION, Warnings,
    is_valid_id, quoted, read_theme_file,
};
use guitk::widget_style::{
    ButtonStyle, CheckStyle, FieldBorder, FieldStyle, FocusMark, ScrollbarVisibility,
    ScrollbarWidth, ToggleStyle, WidgetStyle,
};
use std::ffi::{OsStr, OsString};
use yamldoc::Document;

// ============================================================================
// Reading the section
// ============================================================================

/// The widget style `doc`'s section sets, over the built-in one; `None` when
/// the section is absent or sets nothing usable.
pub(super) fn read(doc: &Document, warnings: &mut Warnings) -> Option<WidgetStyle> {
    let mut style = WidgetStyle::AERO;
    let mut any = false;
    for part in doc.keys(&[SECTION]) {
        let set = match part.as_str() {
            "button" => read_button(doc, &mut style.button, warnings),
            "field" => read_field(doc, &mut style.field, warnings),
            "check" => read_check(doc, &mut style.check, warnings),
            "scrollbar" => read_scrollbar(doc, &mut style.scrollbar, warnings),
            "toggle" => read_choice(
                doc,
                &[SECTION, "toggle"],
                &[
                    ("pill", ToggleStyle::Pill),
                    ("checkbox", ToggleStyle::Checkbox),
                ],
                warnings,
            )
            .map(|toggle| style.toggle = toggle)
            .is_some(),
            other => {
                warnings.push(format!(
                    "`{SECTION}.{}` is ignored: a widget style has no part called `{}` \
                     (it has button, field, check, toggle and scrollbar)",
                    quoted(other),
                    quoted(other)
                ));
                false
            }
        };
        any |= set;
    }
    any.then_some(style)
}

/// The settings under `widget-style.<part>`, each handed to `apply` by name;
/// a name `apply` does not know is listed. Answers whether any was set.
///
/// `known` is what the warning offers instead, so a misspelt `radious` is met
/// with the word that was meant.
fn read_part(
    doc: &Document,
    part: &str,
    known: &str,
    warnings: &mut Warnings,
    mut apply: impl FnMut(&str, &mut Warnings) -> Option<bool>,
) -> bool {
    let keys = doc.keys(&[SECTION, part]);
    if keys.is_empty() {
        // `button: flat` -- a value where settings belong.
        if let Some(value) = doc.get_str(&[SECTION, part]) {
            warnings.push(format!(
                "`{SECTION}.{part}` is ignored: it holds settings ({known}), not a value like `{}`",
                quoted(value.trim())
            ));
        }
        return false;
    }
    let mut any = false;
    for key in keys {
        match apply(&key, warnings) {
            Some(set) => any |= set,
            None => warnings.push(format!(
                "`{SECTION}.{part}.{}` is ignored: a {part} has no setting called `{}` (it has {known})",
                quoted(&key),
                quoted(&key)
            )),
        }
    }
    any
}

fn read_button(doc: &Document, button: &mut ButtonStyle, warnings: &mut Warnings) -> bool {
    read_part(
        doc,
        "button",
        "radius, padding, gloss and shadow",
        warnings,
        |key, w| {
            let path = [SECTION, "button", key];
            Some(match key {
                "radius" => read_radius(doc, &path, ButtonStyle::MAX_RADIUS, "a pill", w)
                    .map(|r| button.radius = r)
                    .is_some(),
                "padding" => read_padding(doc, &path, w)
                    .map(|p| button.padding = p)
                    .is_some(),
                "gloss" => read_flag(doc, &path, w).map(|g| button.gloss = g).is_some(),
                "shadow" => read_flag(doc, &path, w)
                    .map(|s| button.shadow = s)
                    .is_some(),
                _ => return None,
            })
        },
    )
}

fn read_field(doc: &Document, field: &mut FieldStyle, warnings: &mut Warnings) -> bool {
    read_part(
        doc,
        "field",
        "radius, border and focus",
        warnings,
        |key, w| {
            let path = [SECTION, "field", key];
            Some(match key {
                "radius" => read_radius(doc, &path, FieldStyle::MAX_RADIUS, "a pill", w)
                    .map(|r| field.radius = r)
                    .is_some(),
                "border" => read_choice(
                    doc,
                    &path,
                    &[
                        ("box", FieldBorder::Box),
                        ("underline", FieldBorder::Underline),
                    ],
                    w,
                )
                .map(|b| field.border = b)
                .is_some(),
                "focus" => read_choice(
                    doc,
                    &path,
                    &[
                        ("ring", FocusMark::Ring),
                        ("glow", FocusMark::Glow),
                        ("underline", FocusMark::Underline),
                    ],
                    w,
                )
                .map(|f| field.focus = f)
                .is_some(),
                _ => return None,
            })
        },
    )
}

fn read_check(doc: &Document, check: &mut CheckStyle, warnings: &mut Warnings) -> bool {
    read_part(doc, "check", "radius", warnings, |key, w| {
        let path = [SECTION, "check", key];
        Some(match key {
            "radius" => read_radius(doc, &path, CheckStyle::MAX_RADIUS, "a circle", w)
                .map(|r| check.radius = r)
                .is_some(),
            _ => return None,
        })
    })
}

fn read_scrollbar(
    doc: &Document,
    bar: &mut guitk::widget_style::ScrollbarStyle,
    warnings: &mut Warnings,
) -> bool {
    read_part(
        doc,
        "scrollbar",
        "width and visibility",
        warnings,
        |key, w| {
            let path = [SECTION, "scrollbar", key];
            Some(match key {
                "width" => read_choice(
                    doc,
                    &path,
                    &[
                        ("thin", ScrollbarWidth::Thin),
                        ("normal", ScrollbarWidth::Normal),
                    ],
                    w,
                )
                .map(|width| bar.width = width)
                .is_some(),
                "visibility" => read_choice(
                    doc,
                    &path,
                    &[
                        ("always", ScrollbarVisibility::Always),
                        ("overlay", ScrollbarVisibility::Overlay),
                    ],
                    w,
                )
                .map(|v| bar.visibility = v)
                .is_some(),
                _ => return None,
            })
        },
    )
}

/// A radius in whole pixels, from 0 to `max`. Past `max` is `max` -- which is
/// `roundest`, a pill or a circle -- with a note saying so; anything else that
/// is not a whole number of pixels is ignored.
fn read_radius(
    doc: &Document,
    path: &[&str],
    max: u8,
    roundest: &str,
    warnings: &mut Warnings,
) -> Option<u8> {
    let raw = value_of(doc, path, "a number of pixels, like 4", warnings)?;
    let Ok(pixels) = raw.parse::<i64>() else {
        warnings.push(format!(
            "`{}` is ignored: `{}` is not a whole number of pixels",
            at(path),
            quoted(&raw)
        ));
        return None;
    };
    if pixels < 0 {
        warnings.push(format!(
            "`{}` is ignored: a radius cannot be less than 0",
            at(path)
        ));
        return None;
    }
    match u8::try_from(pixels) {
        Ok(fits) if fits <= max => Some(fits),
        _ => {
            warnings.push(format!(
                "`{}` is drawn as {max}: that is already {roundest}, the roundest it can be",
                at(path)
            ));
            Some(max)
        }
    }
}

/// A button's padding in whole pixels, held to
/// `ButtonStyle::MIN_PADDING..=MAX_PADDING` with a note when it is not in it.
fn read_padding(doc: &Document, path: &[&str], warnings: &mut Warnings) -> Option<u8> {
    let raw = value_of(doc, path, "a number of pixels, like 14", warnings)?;
    let Ok(pixels) = raw.parse::<i64>() else {
        warnings.push(format!(
            "`{}` is ignored: `{}` is not a whole number of pixels",
            at(path),
            quoted(&raw)
        ));
        return None;
    };
    let (min, max) = (
        i64::from(ButtonStyle::MIN_PADDING),
        i64::from(ButtonStyle::MAX_PADDING),
    );
    let held = pixels.clamp(min, max);
    if held != pixels {
        warnings.push(format!(
            "`{}` is drawn as {held}: a button's padding is {min} to {max} pixels",
            at(path)
        ));
    }
    u8::try_from(held).ok()
}

// ============================================================================
// The widget style in use
// ============================================================================

/// The widget style in use: which theme's was chosen, and what reading it
/// gave. [`super::ColorTheme`]'s counterpart for this axis, and one value for
/// the same reason: the name and the shapes cannot disagree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WidgetTheme {
    id: OsString,
    style: WidgetStyle,
    warnings: Vec<String>,
    problem: Option<String>,
}

impl Default for WidgetTheme {
    fn default() -> Self {
        Self::built_in()
    }
}

impl WidgetTheme {
    /// The built-in theme's controls, [`WidgetStyle::AERO`].
    #[must_use]
    pub fn built_in() -> Self {
        Self {
            id: OsString::from(BUILT_IN),
            style: WidgetStyle::AERO,
            warnings: Vec::new(),
            problem: None,
        }
    }

    /// The widget style of the theme named `id`, read from the standard
    /// directories.
    ///
    /// Never fails. A theme that cannot be used -- not installed, unreadable,
    /// with no `widget-style` section -- keeps its name, so saving the
    /// settings does not quietly forget the choice, and is drawn in the
    /// built-in style with a [`problem`](Self::problem) saying why.
    #[must_use]
    pub fn load(id: &OsStr) -> Self {
        Self::load_from(&ThemeDirs::standard(), id)
    }

    /// [`load`](Self::load), from the given directories.
    #[must_use]
    pub fn load_from(dirs: &ThemeDirs, id: &OsStr) -> Self {
        if id == OsStr::new(BUILT_IN) {
            return Self::built_in();
        }
        match read_for_widgets(dirs, id) {
            Ok((style, warnings)) => Self {
                id: id.to_owned(),
                style,
                warnings,
                problem: None,
            },
            Err(err) => Self {
                id: id.to_owned(),
                style: WidgetStyle::AERO,
                warnings: Vec::new(),
                problem: Some(format!(
                    "\"{}\" {err}, so the built-in controls are shown.",
                    pathcodec::display_os(id)
                )),
            },
        }
    }

    /// A widget style already in hand -- a preview of one being edited, or a
    /// test's.
    #[must_use]
    pub fn from_style(id: impl Into<OsString>, style: WidgetStyle) -> Self {
        Self {
            id: id.into(),
            style,
            warnings: Vec::new(),
            problem: None,
        }
    }

    /// The theme's name, as `theme.widget_style` holds it.
    #[must_use]
    pub fn id(&self) -> &OsStr {
        &self.id
    }

    /// Whether this is the built-in theme.
    #[must_use]
    pub fn is_built_in(&self) -> bool {
        self.id == OsStr::new(BUILT_IN)
    }

    /// The shapes to draw the controls in: the theme's, or the built-in ones
    /// when it could not be used.
    #[must_use]
    pub fn style(&self) -> WidgetStyle {
        self.style
    }

    /// What in the theme's file was ignored; see [`ThemeFile::warnings`].
    #[must_use]
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    /// Why the chosen theme's controls are not in use, as a sentence for the
    /// user; `None` when they are.
    #[must_use]
    pub fn problem(&self) -> Option<&str> {
        self.problem.as_deref()
    }
}

/// Find and read the installed theme `id` for its widget style, with what in
/// its file was ignored.
fn read_for_widgets(
    dirs: &ThemeDirs,
    id: &OsStr,
) -> Result<(WidgetStyle, Vec<String>), ThemeError> {
    if !is_valid_id(id) {
        return Err(ThemeError::InvalidName);
    }
    let (dir, _) = dirs.find(id).ok_or(ThemeError::NotInstalled)?;
    let ThemeFile {
        widget_style,
        warnings,
        ..
    } = read_theme_file(&dir.join(FILE_NAME))?;
    let style = widget_style.ok_or(ThemeError::NoWidgetStyle)?;
    Ok((style, warnings))
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::super::{Origin, available_in, parse};
    use super::*;
    use guitk::widget_style::ScrollbarStyle;
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
                scratch: ScratchDir::new(&format!("slateos-widget-themes-{tag}")),
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
    const FULL: &str = "\
widget-style:
  button:
    radius: 9
    padding: 20
    gloss: false
    shadow: true
  field:
    radius: 6
    border: underline
    focus: underline
  check:
    radius: 7
  toggle: checkbox
  scrollbar:
    width: thin
    visibility: overlay
";

    fn full() -> WidgetStyle {
        WidgetStyle {
            button: ButtonStyle {
                radius: 9,
                padding: 20,
                gloss: false,
                shadow: true,
            },
            field: FieldStyle {
                radius: 6,
                border: FieldBorder::Underline,
                focus: FocusMark::Underline,
            },
            check: CheckStyle { radius: 7 },
            toggle: ToggleStyle::Checkbox,
            scrollbar: ScrollbarStyle {
                width: ScrollbarWidth::Thin,
                visibility: ScrollbarVisibility::Overlay,
            },
        }
    }

    /// **Every setting is read**, and a section that writes them all gives
    /// exactly what it wrote, with nothing to report.
    #[test]
    fn every_setting_is_read() {
        let file = parse(FULL);
        assert_eq!(file.warnings, Vec::<String>::new());
        assert_eq!(file.widget_style, Some(full()));
        assert_ne!(full(), WidgetStyle::AERO, "the fixture proves nothing");
    }

    /// **A section sets only what it names**: the rest is the built-in
    /// theme's, as a colour a theme leaves out is.
    #[test]
    fn a_section_sets_only_what_it_names() {
        let file = parse("widget-style:\n  field:\n    focus: ring\n");
        let mut expected = WidgetStyle::AERO;
        expected.field.focus = FocusMark::Ring;
        assert_eq!(file.widget_style, Some(expected));
    }

    /// **No section, or one that sets nothing usable, is no widget style** --
    /// which is what keeps a colours-only theme from being offered for the
    /// controls.
    #[test]
    fn a_file_without_a_usable_section_sets_no_widget_style() {
        assert_eq!(parse("colors:\n  base: \"#2e3440\"\n").widget_style, None);
        assert_eq!(parse("widget-style:\n").widget_style, None);
        let bad = parse("widget-style:\n  button:\n    radius: big\n");
        assert_eq!(bad.widget_style, None);
        assert_eq!(bad.warnings.len(), 1, "{:?}", bad.warnings);
    }

    /// **What is not understood costs that value and is listed**: each
    /// mistake below is named for the theme's author, the values around it
    /// are used, and a radius past the roundest is the roundest.
    #[test]
    fn what_is_not_understood_costs_that_value_and_is_listed() {
        let file = parse(
            "\
widget-style:
  button:
    radius: big
    padding: 99
    gloss: maybe
    shadow:
    colour: red
  field:
    radius: -2
    border: dotted
    focus: ring
  check:
    radius: 30
  toggle: switch
  scrollbar: flat
  menu:
    radius: 2
",
        );
        let style = file.widget_style.expect("the good values are used");
        let mut expected = WidgetStyle::AERO;
        expected.button.padding = ButtonStyle::MAX_PADDING;
        expected.field.focus = FocusMark::Ring;
        expected.check.radius = CheckStyle::MAX_RADIUS;
        assert_eq!(style, expected);

        let said = |needle: &str| {
            assert!(
                file.warnings.iter().any(|w| w.contains(needle)),
                "no warning says {needle:?}: {:#?}",
                file.warnings
            );
        };
        said("`widget-style.button.radius` is ignored: `big` is not a whole number of pixels");
        said("`widget-style.button.padding` is drawn as 24: a button's padding is 4 to 24 pixels");
        said("`widget-style.button.gloss` is ignored: `maybe` is not true or false");
        said("`widget-style.button.shadow` has no value: write true or false");
        said(
            "`widget-style.button.colour` is ignored: a button has no setting called `colour` \
             (it has radius, padding, gloss and shadow)",
        );
        said("`widget-style.field.radius` is ignored: a radius cannot be less than 0");
        said("`widget-style.field.border` is ignored: `dotted` is not box or underline");
        said("`widget-style.check.radius` is drawn as 7: that is already a circle");
        said("`widget-style.toggle` is ignored: `switch` is not pill or checkbox");
        said(
            "`widget-style.scrollbar` is ignored: it holds settings (width and visibility), \
             not a value like `flat`",
        );
        said("`widget-style.menu` is ignored: a widget style has no part called `menu`");
        assert_eq!(file.warnings.len(), 11, "{:#?}", file.warnings);
    }

    /// **The shipped built-in theme writes out the built-in controls**, every
    /// setting present, as the template to copy -- and the destructure below
    /// stops compiling when a setting is added to `WidgetStyle`, until someone
    /// has written it into the template and the reader.
    #[test]
    fn the_shipped_built_in_theme_writes_out_the_built_in_controls() {
        const AERO_FILE: &str = include_str!("../../themes/aero/theme.yaml");
        let file = parse(AERO_FILE);
        assert_eq!(file.warnings, Vec::<String>::new());
        assert_eq!(file.widget_style, Some(WidgetStyle::AERO));

        let WidgetStyle {
            button:
                ButtonStyle {
                    radius: _,
                    padding: _,
                    gloss: _,
                    shadow: _,
                },
            field:
                FieldStyle {
                    radius: _,
                    border: _,
                    focus: _,
                },
            check: CheckStyle { radius: _ },
            toggle: _,
            scrollbar:
                ScrollbarStyle {
                    width: _,
                    visibility: _,
                },
        } = WidgetStyle::AERO;
        let doc = Document::parse(AERO_FILE);
        assert_eq!(
            doc.keys(&[SECTION]),
            ["button", "field", "check", "toggle", "scrollbar"]
        );
        for (part, keys) in [
            ("button", &["radius", "padding", "gloss", "shadow"][..]),
            ("field", &["radius", "border", "focus"]),
            ("check", &["radius"]),
            ("scrollbar", &["width", "visibility"]),
        ] {
            assert_eq!(doc.keys(&[SECTION, part]), keys, "{part}");
        }
        assert!(doc.get_str(&[SECTION, "toggle"]).is_some());
    }

    /// **An installed theme is loaded for its controls.**
    #[test]
    fn an_installed_theme_is_loaded_for_its_controls() {
        let f = Fixture::new("load");
        f.install("soft", FULL.as_bytes());
        let theme = WidgetTheme::load_from(&f.dirs(), OsStr::new("soft"));
        assert_eq!(theme.id(), "soft");
        assert!(!theme.is_built_in());
        assert_eq!(theme.problem(), None);
        assert_eq!(theme.style(), full());
    }

    /// **The built-in theme reads no file**: its controls are compiled in, so
    /// an `aero` folder with a section of its own changes nothing.
    #[test]
    fn the_built_in_theme_reads_no_file() {
        let f = Fixture::new("built-in");
        f.install(BUILT_IN, FULL.as_bytes());
        let theme = WidgetTheme::load_from(&f.dirs(), OsStr::new(BUILT_IN));
        assert_eq!(theme, WidgetTheme::built_in());
        assert_eq!(theme.style(), WidgetStyle::AERO);
        assert!(theme.is_built_in());
        assert_eq!(WidgetTheme::default(), WidgetTheme::built_in());
    }

    /// **A theme that cannot give the controls says why and keeps its
    /// name**, and the built-in controls are drawn: not installed, not a
    /// folder's name, no section, not text.
    #[test]
    fn a_theme_that_cannot_be_used_says_why_and_keeps_its_name() {
        let f = Fixture::new("unusable");
        f.install("nord", b"colors:\n  base: \"#2e3440\"\n");
        f.install("binary", b"widget-style:\n  toggle: \xff\n");
        for (id, why) in [
            ("gone", "\"gone\" is not installed"),
            ("../up", "is not the name of a folder in a themes directory"),
            ("nord", "\"nord\" sets no widget style"),
            ("binary", "\"binary\" has a file that is not text"),
        ] {
            let theme = WidgetTheme::load_from(&f.dirs(), OsStr::new(id));
            assert_eq!(theme.id(), id);
            assert_eq!(theme.style(), WidgetStyle::AERO, "{id}");
            let problem = theme.problem().unwrap_or_default();
            assert!(problem.contains(why), "{id}: {problem:?}");
            assert!(
                problem.ends_with("so the built-in controls are shown."),
                "{problem}"
            );
        }
    }

    /// **The list says which themes can give the controls**: the built-in
    /// one always, a theme with a usable section, and not a colours-only
    /// theme or one that cannot be read.
    #[test]
    fn the_list_says_which_themes_give_the_controls() {
        let f = Fixture::new("list");
        f.install("nord", b"colors:\n  base: \"#2e3440\"\n");
        f.install("soft", FULL.as_bytes());
        f.install(
            "whole",
            format!("colors:\n  base: \"#101010\"\n{FULL}").as_bytes(),
        );
        f.install("broken", b"widget-style:\n  toggle: \xff\n");
        let listed = available_in(&f.dirs());
        let gives = |id: &str| {
            listed
                .iter()
                .find(|info| info.id == OsStr::new(id))
                .unwrap_or_else(|| panic!("{id} is not listed"))
                .provides_widget_style()
        };
        assert_eq!(listed[0].origin, Origin::BuiltIn);
        assert!(gives(BUILT_IN));
        assert!(!gives("nord"));
        assert!(gives("soft"));
        assert!(gives("whole"));
        assert!(!gives("broken"));

        // A description that says both -- a section, and a reason it cannot be
        // used -- is not offered: the reason wins. The listing never builds
        // one, but the fields are public and a theme browser may.
        let mut contradictory = listed
            .iter()
            .find(|info| info.id == OsStr::new("soft"))
            .cloned()
            .expect("soft is listed");
        contradictory.problem = Some(ThemeError::NotText);
        assert!(!contradictory.provides_widget_style());
    }
}
