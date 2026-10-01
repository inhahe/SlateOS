//! A theme's animation axis: its `animation` section, read, and the motion
//! chosen from it ([`AnimationTheme`]).
//!
//! The section says how the desktop's transitions move
//! ([`guitk::motion`]): how long the standard one takes, along which curve,
//! or that nothing moves at all. The user picks which theme's to use as
//! `theme.animation` in `appearance.yaml`, independently of the colours, the
//! icons and the controls -- the mix-and-match every axis allows -- and their
//! own animation speed then scales it. `design-decisions.md` §1446.
//!
//! ```yaml
//! animation:
//!   enabled: true       # false: nothing on the desktop moves
//!   duration-ms: 200    # the standard transition: 50 to 1000 milliseconds
//!   easing: ease-out    # ease-out | linear | spring
//! ```
//!
//! As with every section, a setting left out keeps the built-in theme's, and
//! a value that cannot be understood costs that value and is listed for the
//! theme's author; the rest of the section is used. A duration outside 50 to
//! 1000 is taken as the nearer end, with a note, since the direction is
//! plainly what was meant.

use super::values::{at, read_choice, read_flag, value_of};
use super::{
    ANIMATION_SECTION as SECTION, BUILT_IN, FILE_NAME, ThemeDirs, ThemeError, ThemeFile, Warnings,
    is_valid_id, quoted, read_theme_file,
};
use guitk::motion::{Curve, Motion};
use std::ffi::{OsStr, OsString};
use yamldoc::Document;

/// The settings a section may hold, as a warning offers them.
const SETTINGS: &str = "enabled, duration-ms and easing";

// ============================================================================
// Reading the section
// ============================================================================

/// The motion `doc`'s section sets, over the built-in one; `None` when the
/// section is absent or sets nothing usable.
pub(super) fn read(doc: &Document, warnings: &mut Warnings) -> Option<Motion> {
    let keys = doc.keys(&[SECTION]);
    if keys.is_empty() {
        // `animation: false` -- a value where settings belong. Refused, as
        // every section refuses one, but with the spelling that was meant.
        if let Some(value) = doc.get_str(&[SECTION]).filter(|v| !v.trim().is_empty()) {
            warnings.push(format!(
                "`{SECTION}` is ignored: it holds settings ({SETTINGS}), not a value like `{}` \
                 -- for no animation, write `enabled: false` under it",
                quoted(value.trim())
            ));
        }
        return None;
    }
    let mut enabled = true;
    let mut standard_ms = Motion::STANDARD.standard_ms();
    let mut curve = Motion::STANDARD.curve();
    let mut any = false;
    let curves = Curve::ALL.map(|curve| (curve.name(), curve));
    for key in keys {
        let path = [SECTION, key.as_str()];
        let set = match key.as_str() {
            "enabled" => read_flag(doc, &path, warnings)
                .map(|on| enabled = on)
                .is_some(),
            "duration-ms" => read_duration(doc, &path, warnings)
                .map(|ms| standard_ms = ms)
                .is_some(),
            "easing" => read_choice(doc, &path, &curves, warnings)
                .map(|chosen| curve = chosen)
                .is_some(),
            other => {
                warnings.push(format!(
                    "`{SECTION}.{}` is ignored: an animation has no setting called `{}` \
                     (it has {SETTINGS})",
                    quoted(other),
                    quoted(other)
                ));
                false
            }
        };
        any |= set;
    }
    any.then(|| {
        if enabled {
            Motion::new(standard_ms, curve)
        } else {
            Motion::STILL
        }
    })
}

/// The standard transition's length in whole milliseconds, held to
/// [`Motion::MIN_MS`]..=[`Motion::MAX_MS`] with a note when it is not in it.
fn read_duration(doc: &Document, path: &[&str], warnings: &mut Warnings) -> Option<u16> {
    let raw = value_of(doc, path, "a number of milliseconds, like 200", warnings)?;
    let Ok(ms) = raw.parse::<i64>() else {
        warnings.push(format!(
            "`{}` is ignored: `{}` is not a whole number of milliseconds",
            at(path),
            quoted(&raw)
        ));
        return None;
    };
    let (min, max) = (i64::from(Motion::MIN_MS), i64::from(Motion::MAX_MS));
    let held = ms.clamp(min, max);
    if held != ms {
        // A zero is most likely "no animation", which has its own spelling.
        let instead = if ms <= 0 {
            " -- for no animation, write `enabled: false`"
        } else {
            ""
        };
        warnings.push(format!(
            "`{}` is taken as {held}: a standard transition is {min} to {max} milliseconds{instead}",
            at(path)
        ));
    }
    u16::try_from(held).ok()
}

// ============================================================================
// The motion in use
// ============================================================================

/// The animation theme in use: which theme's was chosen, and what reading it
/// gave. [`super::WidgetTheme`]'s counterpart for this axis, and one value for
/// the same reason: the name and the motion cannot disagree.
///
/// The motion here is the *theme's*. The user's speed is applied where the
/// two meet, `AppearanceSettings`' palette source, so that a theme browser
/// can show a theme's own motion whatever the user's speed is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnimationTheme {
    id: OsString,
    motion: Motion,
    warnings: Vec<String>,
    problem: Option<String>,
}

impl Default for AnimationTheme {
    fn default() -> Self {
        Self::built_in()
    }
}

impl AnimationTheme {
    /// The built-in theme's motion, [`Motion::STANDARD`].
    #[must_use]
    pub fn built_in() -> Self {
        Self {
            id: OsString::from(BUILT_IN),
            motion: Motion::STANDARD,
            warnings: Vec::new(),
            problem: None,
        }
    }

    /// The motion of the theme named `id`, read from the standard
    /// directories.
    ///
    /// Never fails. A theme that cannot be used -- not installed, unreadable,
    /// with no `animation` section -- keeps its name, so saving the settings
    /// does not quietly forget the choice, and the built-in motion is used
    /// with a [`problem`](Self::problem) saying why.
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
        match read_for_animation(dirs, id) {
            Ok((motion, warnings)) => Self {
                id: id.to_owned(),
                motion,
                warnings,
                problem: None,
            },
            Err(err) => Self {
                id: id.to_owned(),
                motion: Motion::STANDARD,
                warnings: Vec::new(),
                problem: Some(format!(
                    "\"{}\" {err}, so the built-in animation is used.",
                    pathcodec::display_os(id)
                )),
            },
        }
    }

    /// A motion already in hand -- a preview of one being edited, or a
    /// test's.
    #[must_use]
    pub fn from_motion(id: impl Into<OsString>, motion: Motion) -> Self {
        Self {
            id: id.into(),
            motion,
            warnings: Vec::new(),
            problem: None,
        }
    }

    /// The theme's name, as `theme.animation` holds it.
    #[must_use]
    pub fn id(&self) -> &OsStr {
        &self.id
    }

    /// Whether this is the built-in theme.
    #[must_use]
    pub fn is_built_in(&self) -> bool {
        self.id == OsStr::new(BUILT_IN)
    }

    /// The theme's motion -- or the built-in one when it could not be used --
    /// before the user's speed is applied.
    #[must_use]
    pub fn motion(&self) -> Motion {
        self.motion
    }

    /// What in the theme's file was ignored; see [`ThemeFile::warnings`].
    #[must_use]
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    /// Why the chosen theme's motion is not in use, as a sentence for the
    /// user; `None` when it is.
    #[must_use]
    pub fn problem(&self) -> Option<&str> {
        self.problem.as_deref()
    }
}

/// Find and read the installed theme `id` for its motion, with what in its
/// file was ignored.
fn read_for_animation(dirs: &ThemeDirs, id: &OsStr) -> Result<(Motion, Vec<String>), ThemeError> {
    if !is_valid_id(id) {
        return Err(ThemeError::InvalidName);
    }
    let (dir, _) = dirs.find(id).ok_or(ThemeError::NotInstalled)?;
    let ThemeFile {
        motion, warnings, ..
    } = read_theme_file(&dir.join(FILE_NAME))?;
    let motion = motion.ok_or(ThemeError::NoAnimation)?;
    Ok((motion, warnings))
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
                scratch: ScratchDir::new(&format!("slateos-animation-themes-{tag}")),
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
    const FULL: &str = "animation:\n  enabled: true\n  duration-ms: 350\n  easing: spring\n";

    /// **Every setting is read**, and a section that writes them all gives
    /// exactly what it wrote, with nothing to report.
    #[test]
    fn every_setting_is_read() {
        let file = parse(FULL);
        assert_eq!(file.warnings, Vec::<String>::new());
        assert_eq!(file.motion, Some(Motion::new(350, Curve::Spring)));
        for curve in Curve::ALL {
            let file = parse(&format!("animation:\n  easing: {}\n", curve.name()));
            assert_eq!(file.motion, Some(Motion::new(200, curve)));
        }
    }

    /// **A section sets only what it names**; the rest is the built-in
    /// theme's.
    #[test]
    fn a_section_sets_only_what_it_names() {
        let file = parse("animation:\n  duration-ms: 120\n");
        assert_eq!(file.motion, Some(Motion::new(120, Curve::EaseOut)));
        let file = parse("animation:\n  easing: linear\n");
        assert_eq!(file.motion, Some(Motion::new(200, Curve::Linear)));
        assert_eq!(
            parse("animation:\n  enabled: true\n").motion,
            Some(Motion::STANDARD)
        );
    }

    /// **`enabled: false` is still**, whatever else the section says.
    #[test]
    fn disabled_is_still() {
        let file = parse("animation:\n  enabled: false\n  duration-ms: 400\n  easing: spring\n");
        assert_eq!(file.warnings, Vec::<String>::new());
        assert_eq!(file.motion, Some(Motion::STILL));
        assert_eq!(
            parse("animation:\n  enabled: False\n").motion,
            Some(Motion::STILL)
        );
    }

    /// **A file without a usable section sets no motion**: no section, an
    /// empty one, or one whose every value was refused.
    #[test]
    fn a_file_without_a_usable_section_sets_no_motion() {
        for text in [
            "colors:\n  base: \"#101010\"\n",
            "animation:\n",
            "animation:\n  easing: bouncy\n",
        ] {
            assert_eq!(parse(text).motion, None, "{text:?}");
        }
    }

    /// **What is not understood costs that value and is listed**: each
    /// mistake below is named for the theme's author, the values around it
    /// are used, and a duration past either end is that end.
    #[test]
    fn what_is_not_understood_costs_that_value_and_is_listed() {
        let file = parse(
            "\
animation:
  enabled: sometimes
  duration-ms: 5000
  easing: bounce
  delay: 20
",
        );
        assert_eq!(file.motion, Some(Motion::new(1000, Curve::EaseOut)));
        let said = |warnings: &[String], needle: &str| {
            assert!(
                warnings.iter().any(|w| w.contains(needle)),
                "no warning says {needle:?}: {warnings:#?}"
            );
        };
        said(
            &file.warnings,
            "`animation.enabled` is ignored: `sometimes` is not true or false",
        );
        said(
            &file.warnings,
            "`animation.duration-ms` is taken as 1000: a standard transition is 50 to 1000 \
             milliseconds",
        );
        said(
            &file.warnings,
            "`animation.easing` is ignored: `bounce` is not ease-out, linear or spring",
        );
        said(
            &file.warnings,
            "`animation.delay` is ignored: an animation has no setting called `delay` \
             (it has enabled, duration-ms and easing)",
        );
        assert_eq!(file.warnings.len(), 4, "{:#?}", file.warnings);

        // Too short, and nothing at all -- which is told how to say "none".
        let file = parse("animation:\n  duration-ms: 10\n");
        assert_eq!(file.motion, Some(Motion::new(50, Curve::EaseOut)));
        assert!(
            !file.warnings[0].contains("enabled: false"),
            "{:?}",
            file.warnings
        );
        let file = parse("animation:\n  duration-ms: 0\n");
        assert_eq!(file.motion, Some(Motion::new(50, Curve::EaseOut)));
        said(
            &file.warnings,
            "is taken as 50: a standard transition is 50 to 1000 milliseconds -- for no \
             animation, write `enabled: false`",
        );

        // Not a number, and nothing written.
        let file = parse("animation:\n  duration-ms: 200ms\n  easing:\n");
        assert_eq!(file.motion, None);
        said(
            &file.warnings,
            "`animation.duration-ms` is ignored: `200ms` is not a whole number of milliseconds",
        );
        said(
            &file.warnings,
            "`animation.easing` has no value: write ease-out, linear or spring",
        );

        // A value where the settings belong.
        let file = parse("animation: false\n");
        assert_eq!(file.motion, None);
        said(
            &file.warnings,
            "`animation` is ignored: it holds settings (enabled, duration-ms and easing), not a \
             value like `false` -- for no animation, write `enabled: false` under it",
        );
    }

    /// **The shipped built-in theme writes out the built-in motion**, every
    /// setting present, as the template to copy.
    #[test]
    fn the_shipped_built_in_theme_writes_out_the_built_in_motion() {
        const AERO_FILE: &str = include_str!("../../themes/aero/theme.yaml");
        let file = parse(AERO_FILE);
        assert_eq!(file.warnings, Vec::<String>::new());
        assert_eq!(file.motion, Some(Motion::STANDARD));
        let doc = Document::parse(AERO_FILE);
        assert_eq!(doc.keys(&[SECTION]), ["enabled", "duration-ms", "easing"]);
    }

    /// **An installed theme is loaded for its motion.**
    #[test]
    fn an_installed_theme_is_loaded_for_its_motion() {
        let f = Fixture::new("load");
        f.install("springy", FULL.as_bytes());
        let theme = AnimationTheme::load_from(&f.dirs(), OsStr::new("springy"));
        assert_eq!(theme.id(), "springy");
        assert!(!theme.is_built_in());
        assert_eq!(theme.problem(), None);
        assert_eq!(theme.warnings(), &[] as &[String]);
        assert_eq!(theme.motion(), Motion::new(350, Curve::Spring));

        f.install("noisy", b"animation:\n  easing: linear\n  pace: 3\n");
        let theme = AnimationTheme::load_from(&f.dirs(), OsStr::new("noisy"));
        assert_eq!(theme.motion(), Motion::new(200, Curve::Linear));
        assert_eq!(theme.warnings().len(), 1, "{:?}", theme.warnings());

        let chosen = AnimationTheme::from_motion("preview", Motion::STILL);
        assert_eq!(chosen.id(), "preview");
        assert_eq!(chosen.motion(), Motion::STILL);
    }

    /// **The built-in theme reads no file**: its motion is compiled in, so
    /// an `aero` folder with a section of its own changes nothing.
    #[test]
    fn the_built_in_theme_reads_no_file() {
        let f = Fixture::new("built-in");
        f.install(BUILT_IN, FULL.as_bytes());
        let theme = AnimationTheme::load_from(&f.dirs(), OsStr::new(BUILT_IN));
        assert_eq!(theme, AnimationTheme::built_in());
        assert_eq!(theme.motion(), Motion::STANDARD);
        assert!(theme.is_built_in());
        assert_eq!(AnimationTheme::default(), AnimationTheme::built_in());
    }

    /// **A theme that cannot give the motion says why and keeps its name**,
    /// and the built-in motion is used: not installed, not a folder's name,
    /// no section, not text.
    #[test]
    fn a_theme_that_cannot_be_used_says_why_and_keeps_its_name() {
        let f = Fixture::new("unusable");
        f.install("nord", b"colors:\n  base: \"#2e3440\"\n");
        f.install("binary", b"animation:\n  easing: \xff\n");
        for (id, why) in [
            ("gone", "\"gone\" is not installed"),
            ("../up", "is not the name of a folder in a themes directory"),
            ("nord", "\"nord\" sets no animation"),
            ("binary", "\"binary\" has a file that is not text"),
        ] {
            let theme = AnimationTheme::load_from(&f.dirs(), OsStr::new(id));
            assert_eq!(theme.id(), id);
            assert_eq!(theme.motion(), Motion::STANDARD, "{id}");
            let problem = theme.problem().unwrap_or_default();
            assert!(problem.contains(why), "{id}: {problem:?}");
            assert!(
                problem.ends_with("so the built-in animation is used."),
                "{problem}"
            );
        }
    }

    /// **The list says which themes can give the motion**: the built-in one
    /// always, a theme with a usable section -- a still one included -- and
    /// not a colours-only theme or one that cannot be read.
    #[test]
    fn the_list_says_which_themes_give_the_motion() {
        let f = Fixture::new("list");
        f.install("nord", b"colors:\n  base: \"#2e3440\"\n");
        f.install("springy", FULL.as_bytes());
        f.install("calm", b"animation:\n  enabled: false\n");
        f.install("broken", b"animation:\n  easing: \xff\n");
        let listed = available_in(&f.dirs());
        let gives = |id: &str| {
            listed
                .iter()
                .find(|info| info.id == OsStr::new(id))
                .unwrap_or_else(|| panic!("{id} is not listed"))
                .provides_animation()
        };
        assert_eq!(listed[0].origin, Origin::BuiltIn);
        assert!(gives(BUILT_IN));
        assert!(!gives("nord"));
        assert!(gives("springy"));
        assert!(gives("calm"));
        assert!(!gives("broken"));

        let mut contradictory = listed
            .iter()
            .find(|info| info.id == OsStr::new("springy"))
            .cloned()
            .expect("springy is listed");
        contradictory.problem = Some(ThemeError::NotText);
        assert!(!contradictory.provides_animation());
    }
}
