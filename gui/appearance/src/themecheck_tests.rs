//! Tests for the theme checker: a fixture theme for each kind of finding, and
//! the theme the desktop ships, which it must pass.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use std::fs;
use std::path::{Path, PathBuf};

use scratchdir::ScratchDir;

use super::*;
use crate::cursors::tests::{img, xcursor};

/// A theme's folder under construction, in a scratch directory of its own.
struct Theme {
    _scratch: ScratchDir,
    dir: PathBuf,
}

impl Theme {
    /// An empty folder called `name`.
    fn empty(name: &str) -> Self {
        let scratch = ScratchDir::new("themecheck");
        let dir = scratch.dir().join(name);
        fs::create_dir_all(&dir).unwrap();
        Self {
            _scratch: scratch,
            dir,
        }
    }

    /// A theme with nothing wrong with it: a file saying all a shared theme
    /// should, an icon and a screenshot.
    fn tidy() -> Self {
        let theme = Self::empty("tidy");
        theme
            .write("theme.yaml", TIDY)
            .write("icons/folder.svg", SQUARE)
            .write("shot.png", png());
        theme
    }

    fn write(&self, rel: &str, bytes: impl AsRef<[u8]>) -> &Self {
        let path = self.dir.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
        self
    }

    fn mkdir(&self, rel: &str) -> &Self {
        fs::create_dir_all(self.dir.join(rel)).unwrap();
        self
    }

    /// A link at `rel` whose text is `target`; `false` where this machine
    /// will not make one (Windows without the right to).
    fn link(&self, rel: &str, target: &str) -> bool {
        let path = self.dir.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        #[cfg(unix)]
        let made = std::os::unix::fs::symlink(target, &path);
        #[cfg(windows)]
        let made = std::os::windows::fs::symlink_file(target, &path);
        made.is_ok()
    }

    fn check(&self) -> Report {
        check(&self.dir)
    }
}

const TIDY: &str = "\
meta:
  name: Tidy
  author: Someone
  version: \"1.0\"
  license: MIT
  tags: [test]
  screenshots: [shot.png]
  supports: [colors, icons]
colors:
  base: \"#1e1e2e\"
  text: \"#cdd6f4\"
";

/// An icon: a square in `currentColor`. Its `xmlns` names an `http` address,
/// as every SVG's does, which is not a reference to fetch.
const SQUARE: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"16\" height=\"16\" \
     viewBox=\"0 0 16 16\"><rect x=\"2\" y=\"2\" width=\"12\" height=\"12\" fill=\"currentColor\"/></svg>";

/// A small PNG.
fn png() -> Vec<u8> {
    imagecodec::encode_png(4, 4, &[0xFF33_6699; 16]).unwrap()
}

/// Every finding, a line each, for a failed assertion to show.
fn listing(report: &Report) -> String {
    report
        .findings
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

/// Whether `report` says something of `severity` at `place` with `words` in it.
fn said(report: &Report, severity: Severity, place: &str, words: &str) -> bool {
    report.findings.iter().any(|finding| {
        finding.severity == severity && finding.place == place && finding.message.contains(words)
    })
}

/// Whether the filesystem holding this crate records which files may be
/// run. One that does not -- a Windows drive as WSL mounts it -- shows every
/// file as runnable, so the shipped theme's files are "marked as a program"
/// there and nowhere else. Asked of `Cargo.toml`, which is never runnable.
fn modes_are_recorded() -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        fs::metadata(manifest).is_ok_and(|meta| meta.permissions().mode() & 0o111 == 0)
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// [`said`], or fail showing everything that was.
#[track_caller]
fn assert_said(report: &Report, severity: Severity, place: &str, words: &str) {
    assert!(
        said(report, severity, place, words),
        "no {} at `{place}` saying {words:?}; found:\n{}",
        severity.label(),
        listing(report)
    );
}

// ─── The whole ──────────────────────────────────────────────────────────────

/// **A tidy theme passes with nothing to say but notes**, and the report
/// says what it covers and what it holds.
#[test]
fn a_tidy_theme_passes_with_nothing_but_notes() {
    let report = Theme::tidy().check();
    assert!(report.passes(), "{}", listing(&report));
    assert_eq!(report.count(Severity::Warning), 0, "{}", listing(&report));
    assert_eq!(report.covers, ["colors", "icons"]);
    assert_eq!(report.files, 3);
    assert!(report.bytes > 0);
}

/// **The theme the desktop ships passes**, and of the warnings it has, each is
/// one of two things true of it: it names no licence -- the project has not
/// chosen one -- and the built-in palette's paler inks are darkened on the
/// raised surfaces a user's card style draws text on, which the palette does
/// on purpose. Anything else said against it is a disagreement between the
/// checker and the desktop, and one of the two is wrong.
#[test]
fn the_theme_the_desktop_ships_passes() {
    let aero = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("themes")
        .join(themes::BUILT_IN);
    let report = check(&aero);
    assert!(report.passes(), "{}", listing(&report));
    for finding in &report.findings {
        if finding.severity == Severity::Warning {
            let allowed = finding.message.contains("`meta.license`")
                || finding.message.contains("text needs to be read")
                || (!modes_are_recorded() && finding.message.contains("marked as a program"));
            assert!(allowed, "{finding}");
        }
    }
    assert_eq!(
        report.covers,
        [
            "colors",
            "terminal",
            "syntax",
            "widget-style",
            "animation",
            "window-decorations",
            "taskbar-panel",
            "icons"
        ]
    );
    assert_said(&report, Severity::Note, "", "the built-in theme's name");
}

/// **What is not a theme is said to be none**: a folder with nothing a theme
/// has, and a file where a folder should be.
#[test]
fn what_is_not_a_theme_is_said_to_be_none() {
    let theme = Theme::empty("bare");
    theme.write("notes.txt", "hello");
    let report = theme.check();
    assert!(!report.passes());
    assert_said(&report, Severity::Error, "", "it is not a theme");

    let report = check(&theme.dir.join("notes.txt"));
    assert_said(&report, Severity::Error, "", "is not a folder");
}

/// **A theme that sets nothing usable is refused**: a file of `meta` alone.
#[test]
fn a_theme_that_sets_nothing_is_refused() {
    let theme = Theme::empty("hollow");
    theme.write("theme.yaml", "meta:\n  name: Hollow\n");
    let report = theme.check();
    assert_said(
        &report,
        Severity::Error,
        "",
        "sets nothing this desktop can use",
    );
    assert!(report.covers.is_empty());
}

/// **Errors come first, then warnings, then notes**, each in the order found.
#[test]
fn errors_come_first() {
    let theme = Theme::tidy();
    theme.write("extra.txt", "unread").write("tool.sh", "echo");
    let report = theme.check();
    let order: Vec<Severity> = report.findings.iter().map(|f| f.severity).collect();
    let mut sorted = order.clone();
    sorted.sort_by(|a, b| b.cmp(a));
    assert_eq!(order, sorted, "{}", listing(&report));
    assert_eq!(report.findings[0].severity, Severity::Error);
}

// ─── The theme file ─────────────────────────────────────────────────────────

/// **What the desktop's own reader ignores is reported**, as it says it.
#[test]
fn the_readers_own_warnings_are_reported() {
    let theme = Theme::tidy();
    theme.write("theme.yaml", format!("{TIDY}  link: \"#zzz\"\n"));
    let report = theme.check();
    assert_said(
        &report,
        Severity::Warning,
        "theme.yaml",
        "`colors.link` is ignored",
    );
}

/// **A section or a `meta` key nothing reads is reported** -- `colours` for
/// `colors` is the likely one -- and so is a section that sets nothing.
#[test]
fn what_nothing_in_the_file_reads_is_reported() {
    let theme = Theme::tidy();
    theme.write(
        "theme.yaml",
        format!("{TIDY}colours:\n  base: \"#000000\"\nwidget-style:\nanimation:\n  speed: fast\n")
            .replace(
                "  license: MIT\n",
                "  license: MIT\n  homepage: example.org\n",
            ),
    );
    let report = theme.check();
    assert_said(
        &report,
        Severity::Warning,
        "theme.yaml",
        "`colours` is not a section",
    );
    assert_said(
        &report,
        Severity::Warning,
        "theme.yaml",
        "`meta.homepage` is not read",
    );
    assert_said(
        &report,
        Severity::Warning,
        "theme.yaml",
        "`widget-style` sets nothing",
    );
    assert_said(
        &report,
        Severity::Warning,
        "theme.yaml",
        "`animation` sets nothing",
    );
}

/// **A file that cannot be read is an error**: too large, or not text.
#[test]
fn a_theme_file_that_cannot_be_read_is_an_error() {
    let theme = Theme::empty("unread");
    let limit = usize::try_from(themes::MAX_FILE_BYTES).unwrap();
    theme.write("theme.yaml", "#".repeat(limit + 1));
    assert_said(&theme.check(), Severity::Error, "theme.yaml", "over the");
    theme.write("theme.yaml", b"colors:\n  base: \"\xff\"\n");
    assert_said(&theme.check(), Severity::Error, "theme.yaml", "is not text");
}

/// **What a theme offered to others should say about itself**: who made it,
/// its version and its licence are warnings; its name, pictures and tags
/// notes; and what it covers, unlisted, a warning.
#[test]
fn what_a_shared_theme_should_say_about_itself() {
    let theme = Theme::empty("plain");
    theme.write("theme.yaml", "colors:\n  base: \"#1e1e2e\"\n");
    let report = theme.check();
    assert!(report.passes(), "{}", listing(&report));
    for key in ["author", "version", "license", "supports"] {
        assert_said(
            &report,
            Severity::Warning,
            "theme.yaml",
            &format!("`meta.{key}`"),
        );
    }
    for key in ["name", "screenshots", "tags"] {
        assert_said(
            &report,
            Severity::Note,
            "theme.yaml",
            &format!("`meta.{key}`"),
        );
    }
    assert_said(
        &report,
        Severity::Note,
        "theme.yaml",
        "folder's name, `plain`",
    );
}

/// **`meta.supports` is held to what the theme covers**: an axis listed and
/// not set -- `sounds` as much as `cursors`, now that it is one -- one set
/// and not listed, and a word that is no axis.
#[test]
fn meta_supports_is_held_to_what_the_theme_covers() {
    let theme = Theme::tidy();
    theme.write(
        "theme.yaml",
        TIDY.replace(
            "supports: [colors, icons]",
            "supports: [colors, icons, cursors, sounds, glitter]",
        ) + "terminal:\n  foreground: \"#ffffff\"\n",
    );
    let report = theme.check();
    assert_said(
        &report,
        Severity::Warning,
        "theme.yaml",
        "lists `cursors`, but",
    );
    assert_said(
        &report,
        Severity::Warning,
        "theme.yaml",
        "sets `terminal`, which",
    );
    assert_said(
        &report,
        Severity::Warning,
        "theme.yaml",
        "lists `sounds`, but",
    );
    assert_said(
        &report,
        Severity::Warning,
        "theme.yaml",
        "lists `glitter`, which is not an axis",
    );
    assert!(!said(
        &report,
        Severity::Warning,
        "theme.yaml",
        "lists `icons`"
    ));
}

/// **The meta keys are the fields of `ThemeMeta`**: a field added there
/// stops this compiling until it is listed in `META_KEYS`, and every key
/// listed is read.
#[test]
fn the_meta_keys_are_the_fields_of_theme_meta() {
    let ThemeMeta {
        name,
        author,
        version,
        license,
        tags,
        screenshots,
        supports,
    } = themes::parse(
        "meta:\n  name: A\n  author: B\n  version: C\n  license: D\n  tags: [e]\n  screenshots: [f.png]\n  supports: [colors]\n",
    )
    .meta;
    assert!(name.is_some() && author.is_some() && version.is_some() && license.is_some());
    assert!(!tags.is_empty() && !screenshots.is_empty() && !supports.is_empty());
    assert_eq!(
        themes::META_KEYS,
        [
            "name",
            "author",
            "version",
            "license",
            "tags",
            "screenshots",
            "supports"
        ]
    );
}

// ─── Screenshots ────────────────────────────────────────────────────────────

/// **A screenshot is a picture inside the folder, within its size**: one
/// outside is an error, a missing one or a folder a warning, one that is no
/// picture or too large an error.
#[test]
fn screenshots_are_pictures_inside_the_folder() {
    let theme = Theme::tidy();
    let limit = usize::try_from(MAX_SCREENSHOT_BYTES).unwrap();
    theme
        .write(
            "theme.yaml",
            TIDY.replace(
                "screenshots: [shot.png]",
                "screenshots: [shot.png, ../outside.png, missing.png, words.png, huge.png, shots, ok.svg]",
            ),
        )
        .write("words.png", "not a picture")
        .write("huge.png", vec![0_u8; limit + 1])
        .write("ok.svg", SQUARE)
        .mkdir("shots");
    let report = theme.check();
    assert_said(
        &report,
        Severity::Error,
        "theme.yaml",
        "`../outside.png` is outside",
    );
    assert_said(
        &report,
        Severity::Warning,
        "theme.yaml",
        "`missing.png` is not in",
    );
    assert_said(&report, Severity::Error, "words.png", "is not a picture");
    assert_said(&report, Severity::Error, "huge.png", "larger than");
    assert_said(
        &report,
        Severity::Warning,
        "theme.yaml",
        "`shots` is a folder",
    );
    assert!(
        !report
            .findings
            .iter()
            .any(|f| f.place == "ok.svg" || f.place == "shot.png")
    );
}

// ─── Programs and scripts ───────────────────────────────────────────────────

/// **Nothing in a theme is a program or a script**: by its name, or by how
/// it begins.
#[test]
fn programs_and_scripts_are_refused() {
    let theme = Theme::tidy();
    theme
        .write("tool.sh", "echo hi")
        .write("Setup.EXE", "anything")
        .write("payload", b"\x7fELF\x02\x01\x01")
        .write("run.me", "#!/bin/sh\n")
        .write("module.dat", b"\0asm\x01\0\0\0");
    let report = theme.check();
    assert_said(&report, Severity::Error, "tool.sh", "`.sh`");
    assert_said(&report, Severity::Error, "Setup.EXE", "`.exe`");
    assert_said(&report, Severity::Error, "payload", "an ELF program");
    assert_said(&report, Severity::Error, "run.me", "a script");
    assert_said(&report, Severity::Error, "module.dat", "WebAssembly");
}

/// **An SVG that runs something, or reaches outside itself, is refused** --
/// wherever it is -- and one that only names an `http` namespace, or quotes
/// an attribute's name in a value, is not.
#[test]
fn an_svg_that_runs_or_reaches_out_is_refused() {
    let wrap = |inner: &str| {
        format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 16 16\">{inner}<rect width=\"16\" height=\"16\"/></svg>"
        )
    };
    let theme = Theme::tidy();
    theme
        .write("icons/a.svg", wrap("<script>alert(1)</script>"))
        .write("icons/b.svg", "<svg xmlns=\"http://www.w3.org/2000/svg\" onload=\"go()\" viewBox=\"0 0 16 16\"><rect width=\"16\" height=\"16\"/></svg>")
        .write("icons/c.svg", wrap("<image xlink:href=\"https://example.org/x.png\" width=\"4\" height=\"4\"/>"))
        .write("icons/d.svg", wrap("<foreignObject width=\"4\" height=\"4\"></foreignObject>"))
        .write("icons/e.svg", wrap("<style>@import 'x.css';</style>"))
        .write("art/f.svg", wrap("<a href=\"javascript:go()\"></a>"))
        .write(
            "icons/g.svg",
            wrap("<desc>an onload= in text</desc><rect id=\"x onclick=y\" width=\"1\" height=\"1\"/>"),
        );
    let report = theme.check();
    assert_said(&report, Severity::Error, "icons/a.svg", "<script>");
    assert_said(
        &report,
        Severity::Error,
        "icons/b.svg",
        "`onload` attribute",
    );
    assert_said(&report, Severity::Error, "icons/c.svg", "outside itself");
    assert_said(&report, Severity::Error, "icons/d.svg", "foreignObject");
    assert_said(&report, Severity::Error, "icons/e.svg", "@import");
    assert_said(&report, Severity::Error, "art/f.svg", "javascript:");
    assert!(
        !report
            .findings
            .iter()
            .any(|f| f.severity == Severity::Error && f.place == "icons/g.svg"),
        "{}",
        listing(&report)
    );
}

// ─── Icons and cursors ──────────────────────────────────────────────────────

/// **An icon is drawn as the desktop draws it**: one that draws nothing,
/// one that is not an SVG it can read, one past its size; and what the
/// desktop never asks for -- a misnamed icon, a PNG, a folder -- is said.
#[test]
fn icons_are_drawn_as_the_desktop_draws_them() {
    let theme = Theme::tidy();
    let limit = usize::try_from(icons::MAX_ICON_BYTES).unwrap();
    let huge = format!("{SQUARE}{}", " ".repeat(limit));
    theme
        .write(
            "icons/blank.svg",
            "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 16 16\"></svg>",
        )
        .write("icons/broken.svg", "words, and no markup at all")
        .write("icons/huge.svg", huge)
        .write("icons/User-Home.svg", SQUARE)
        .write("icons/folder.png", png())
        .write("icons/more/inner.svg", SQUARE);
    let report = theme.check();
    assert_said(&report, Severity::Error, "icons/blank.svg", "draws nothing");
    assert_said(
        &report,
        Severity::Error,
        "icons/broken.svg",
        "not an SVG this desktop can draw",
    );
    assert_said(&report, Severity::Error, "icons/huge.svg", "larger than");
    assert_said(
        &report,
        Severity::Warning,
        "icons/User-Home.svg",
        "not named as an icon",
    );
    assert_said(&report, Severity::Warning, "icons/folder.png", "not read");
    assert_said(&report, Severity::Warning, "icons/more", "does not look in");
    // Reported once, as its folder: not again as a file nothing reads.
    assert!(
        !report
            .findings
            .iter()
            .any(|f| f.place == "icons/more/inner.svg")
    );
    assert_eq!(
        report.covers,
        ["colors", "icons"],
        "the tidy icon still draws"
    );
}

/// **A cursor is read as the desktop reads it**: an XCursor file is, a file
/// that is not one is an error, a name nothing asks for a warning, and a
/// cursor theme's `index.theme` is not a file nothing reads.
#[test]
fn cursors_are_read_as_the_desktop_reads_them() {
    let theme = Theme::empty("pointers");
    theme
        .write("cursors/default", xcursor(&[img(24, 0xFF00_0000)]))
        .write("cursors/broken", "not a cursor")
        .write("cursors/bad.name", xcursor(&[img(24, 0xFF00_0000)]))
        .write("index.theme", "[Icon Theme]\nName=Pointers\n");
    let report = theme.check();
    assert!(!report.passes(), "{}", listing(&report));
    assert_said(
        &report,
        Severity::Error,
        "cursors/broken",
        "not an XCursor file",
    );
    assert_said(
        &report,
        Severity::Warning,
        "cursors/bad.name",
        "not named as a cursor",
    );
    assert!(!report.findings.iter().any(|f| f.place == "index.theme"));
    assert!(!report.findings.iter().any(|f| f.place == "cursors/default"));
    assert_eq!(report.covers, ["cursors"]);
}

// ─── Sounds ─────────────────────────────────────────────────────────────────

/// The start of an Ogg file, as far as the checker looks.
const OGG: &[u8] = b"OggS\0\x02\0\0\0\0\0\0\0\0";

/// The start of a WAV file, as far as the checker looks.
const WAV: &[u8] = b"RIFF\x24\0\0\0WAVEfmt ";

/// **A folder of sounds alone is a theme, and its sounds are its `stereo`
/// files**: Ogg and WAV that are what their names say, and a `.disabled`
/// that silences an event; its `index.theme` is no stray file.
#[test]
fn a_folder_of_sounds_is_a_theme() {
    let theme = Theme::empty("chimes");
    theme
        .write("stereo/bell.oga", OGG)
        .write("stereo/message-new-instant.wav", WAV)
        .write("stereo/trash-empty.disabled", "")
        .write(
            "index.theme",
            "[Sound Theme]\nName=Chimes\nDirectories=stereo\n",
        );
    let report = theme.check();
    assert!(report.passes(), "{}", listing(&report));
    assert_eq!(report.covers, ["sounds"]);
    for place in [
        "stereo/bell.oga",
        "stereo/message-new-instant.wav",
        "stereo/trash-empty.disabled",
        "index.theme",
    ] {
        assert!(!report.findings.iter().any(|f| f.place == place), "{place}");
    }
}

/// **What the player would refuse is said**: a name no event has, an
/// extension it does not try, a file that is not what its name says, a
/// folder it does not look in, a file past the size it reads.
#[test]
fn what_the_player_would_refuse_is_said() {
    let theme = Theme::tidy();
    theme
        .write("stereo/Bell.oga", OGG)
        .write("stereo/complete.mp3", "ID3")
        .write("stereo/dialog-error.oga", "not ogg at all")
        .write("stereo/dialog-warning.wav", OGG)
        .write("stereo/more/inner.oga", OGG)
        .write("stereo/noextension", OGG);
    let huge = vec![b'O'; usize::try_from(sounds::MAX_SOUND_BYTES).unwrap() + 1];
    theme.write("stereo/screen-capture.oga", huge);
    let report = theme.check();
    assert!(!report.passes());
    assert_said(
        &report,
        Severity::Warning,
        "stereo/Bell.oga",
        "not named as an event",
    );
    assert_said(
        &report,
        Severity::Warning,
        "stereo/complete.mp3",
        "the desktop tries `.disabled`",
    );
    assert_said(
        &report,
        Severity::Error,
        "stereo/dialog-error.oga",
        "not the Ogg file",
    );
    assert_said(
        &report,
        Severity::Error,
        "stereo/dialog-warning.wav",
        "not the WAV file",
    );
    assert_said(
        &report,
        Severity::Warning,
        "stereo/more",
        "does not look in",
    );
    assert_said(
        &report,
        Severity::Warning,
        "stereo/noextension",
        "not named `<event>.<extension>`",
    );
    assert_said(
        &report,
        Severity::Error,
        "stereo/screen-capture.oga",
        "larger than",
    );
    assert!(!report.covers.contains(&"sounds"), "nothing there plays");
}

/// **The checker's sound checks are the player's own**: the same size
/// limit, the same test of a WAV file, and an Ogg file the checker passes
/// is one the player decodes -- lane F's fixture -- where bytes it fails
/// are refused by the player too.
#[test]
fn the_sound_checks_are_the_players() {
    assert_eq!(sounds::MAX_SOUND_BYTES, sound::MAX_FILE_BYTES);
    for bytes in [
        WAV,
        OGG,
        b"RIFF\0\0\0\0AVI ".as_slice(),
        b"RIF".as_slice(),
        b"",
    ] {
        assert_eq!(super::is_wav(bytes), sound::wav::is_wav(bytes), "{bytes:?}");
    }
    let fixture: &[u8] = include_bytes!("../../video/vorbis/tests/data/mono_q3.ogg");
    assert!(fixture.starts_with(b"OggS"));
    assert!(sound::decode::decode(fixture).is_ok());
    assert!(matches!(
        sound::decode::decode(b"not ogg at all"),
        Err(sound::DecodeError::Unrecognised)
    ));
}

// ─── Links ──────────────────────────────────────────────────────────────────

/// **A link that ends inside the folder is followed; one that ends outside
/// it is refused and not followed** -- whether it says so in its own text,
/// leads to a link that does, or passes through a linked folder that does.
/// Cursor themes are made of links, so the first is no fault.
#[test]
fn links_inside_the_folder_are_followed_and_links_out_are_refused() {
    let theme = Theme::empty("linked");
    // An icon that would draw, beside the theme's folder rather than in it.
    fs::write(theme.dir.parent().unwrap().join("secret.svg"), SQUARE).unwrap();
    // The theme's one cursor is reached only through links: `left_ptr` to
    // `default`, and `default` to a file outside `cursors/`.
    theme
        .write("shared/arrow", xcursor(&[img(24, 0xFF00_0000)]))
        .write("theme.yaml", "colors:\n  base: \"#1e1e2e\"\n");
    if !theme.link("cursors/default", "../shared/arrow") {
        eprintln!("skipped: this machine will not make a link (Windows without the right to)");
        return;
    }
    assert!(theme.link("cursors/left_ptr", "default"));
    assert!(theme.link("icons/out.svg", "../../secret.svg"));
    assert!(theme.link("icons/abs.svg", "/etc/passwd"));
    assert!(theme.link("icons/gone.svg", "nowhere.svg"));
    assert!(theme.link("icons/via.svg", "out.svg"));
    // Read as text alone, `through.svg` stays inside: `icons/sub/secret.svg`.
    // But `sub` is a link to the folder above the theme's.
    assert!(theme.link("icons/sub", "../.."));
    assert!(theme.link("icons/through.svg", "sub/secret.svg"));
    assert!(theme.link("icons/ring-a.svg", "ring-b.svg"));
    assert!(theme.link("icons/ring-b.svg", "ring-a.svg"));
    let report = theme.check();
    // Followed to the cursor, which is read -- and so is no file nothing reads.
    for quiet in ["cursors/left_ptr", "cursors/default", "shared/arrow"] {
        assert!(
            !report.findings.iter().any(|f| f.place == quiet),
            "{quiet}: {}",
            listing(&report)
        );
    }
    for out in [
        "icons/out.svg",
        "icons/via.svg",
        "icons/sub",
        "icons/through.svg",
    ] {
        assert_said(&report, Severity::Error, out, "outside the theme's folder");
    }
    assert_said(
        &report,
        Severity::Error,
        "icons/abs.svg",
        "a path from the root",
    );
    assert_said(
        &report,
        Severity::Warning,
        "icons/gone.svg",
        "leads nowhere",
    );
    assert_said(
        &report,
        Severity::Warning,
        "icons/ring-a.svg",
        "leads nowhere",
    );
    // None of them was followed, so the icon outside -- which would draw --
    // was never read.
    assert!(!report.covers.contains(&"icons"), "{}", listing(&report));
    assert!(report.covers.contains(&"cursors"), "{}", listing(&report));
}

// ─── Contrast ───────────────────────────────────────────────────────────────

/// **Text the desktop must darken to read is reported with what it draws**:
/// the worst ground, the ratio and the colour drawn instead.
#[test]
fn text_the_desktop_must_darken_is_reported() {
    let theme = Theme::tidy();
    theme.write(
        "theme.yaml",
        TIDY.replace(
            "colors:",
            "colors-light:\n  base: \"#ffffff\"\n  text: \"#999999\"\ncolors:",
        ),
    );
    let report = theme.check();
    assert_said(
        &report,
        Severity::Warning,
        "theme.yaml",
        "`colors-light.text`, #999999, is ",
    );
    assert_said(
        &report,
        Severity::Warning,
        "theme.yaml",
        "under the 4.5:1 text needs",
    );
    let finding = report
        .findings
        .iter()
        .find(|f| f.message.contains("`colors-light.text`"))
        .unwrap();
    let drawn = finding.message.rsplit("draws it as ").next().unwrap();
    let drawn = Color::from_hex_text(drawn.trim_end_matches(" instead")).unwrap();
    assert!(contrast_ratio(drawn, Color::rgb(255, 255, 255)) >= TEXT_CONTRAST_FLOOR);
    assert!(
        !said(&report, Severity::Warning, "theme.yaml", "`colors.text`"),
        "the dark text reads"
    );
}

/// **Text that reads on the page but not on a toolbar is reported**, naming
/// the toolbar's ground: a colour is judged by the worst ground it is drawn
/// on, not the page alone.
#[test]
fn text_is_judged_by_its_worst_ground() {
    let theme = Theme::tidy();
    theme.write(
        "theme.yaml",
        TIDY.replace(
            "  base: \"#1e1e2e\"\n  text: \"#cdd6f4\"\n",
            "  base: \"#000000\"\n  text: \"#ffffff\"\n  mantle: \"#d0d0d0\"\n",
        ),
    );
    let report = theme.check();
    assert_said(
        &report,
        Severity::Warning,
        "theme.yaml",
        "`colors.text`, #ffffff, is ",
    );
    assert_said(
        &report,
        Severity::Warning,
        "theme.yaml",
        "on `mantle` (#d0d0d0)",
    );
}

/// **Text that reads everywhere but on the card style's raised surfaces is
/// a note, not a warning** -- the card style is a user's choice, and the
/// palette darkens the text there on purpose -- gathered into one for the
/// mode.
#[test]
fn text_darkened_only_on_cards_is_a_note() {
    let theme = Theme::tidy();
    theme.write(
        "theme.yaml",
        TIDY.replace(
            "  base: \"#1e1e2e\"\n  text: \"#cdd6f4\"\n",
            "  base: \"#000000\"\n  text: \"#ffffff\"\n  link: \"#f0f0f0\"\n  surface1: \"#d0d0d0\"\n",
        ),
    );
    let report = theme.check();
    assert!(
        !said(&report, Severity::Warning, "theme.yaml", "`colors.text`"),
        "{}",
        listing(&report)
    );
    assert_said(
        &report,
        Severity::Note,
        "theme.yaml",
        "darkens 2 of this mode's colours",
    );
    assert_said(
        &report,
        Severity::Note,
        "theme.yaml",
        "`colors.text` (1.54:1 on `surface1`)",
    );
    assert_said(&report, Severity::Note, "theme.yaml", "`colors.link` (");
}

/// **A terminal's text and the colours of code are held to the floor too.**
#[test]
fn a_terminals_text_and_codes_colours_are_held_to_the_floor() {
    let theme = Theme::tidy();
    theme.write(
        "theme.yaml",
        format!(
            "{TIDY}terminal:\n  foreground: \"#333333\"\n  background: \"#000000\"\nsyntax:\n  comment: \"#222233\"\n"
        )
        .replace("supports: [colors, icons]", "supports: [colors, icons, terminal, syntax]"),
    );
    let report = theme.check();
    assert_said(
        &report,
        Severity::Warning,
        "theme.yaml",
        "the terminal's text, #333333 on #000000",
    );
    assert_said(
        &report,
        Severity::Warning,
        "theme.yaml",
        "`syntax.comment`, #222233",
    );
}

/// **A mode drawn in the other's colours never uses its own terminal or code
/// colours**, and saying so is the theme's only clue.
#[test]
fn a_mode_drawn_in_the_others_colours_never_uses_its_own_terminal() {
    let theme = Theme::tidy();
    theme.write(
        "theme.yaml",
        format!("{TIDY}terminal-light:\n  foreground: \"#000000\"\n").replace(
            "supports: [colors, icons]",
            "supports: [colors, icons, terminal]",
        ),
    );
    let report = theme.check();
    assert_said(
        &report,
        Severity::Warning,
        "theme.yaml",
        "`terminal-light` is never used",
    );
}

/// **The grounds named are the palette's text grounds**, in its order, under
/// every style -- a list here could otherwise drift from the one the floor
/// is applied against.
#[test]
fn the_grounds_named_are_the_palettes() {
    for cards in [false, true] {
        for filled in [false, true] {
            let mut palette = Palette::for_mode(false);
            palette.set_surface_style(if cards {
                SurfaceStyle::Cards
            } else {
                SurfaceStyle::Borders
            });
            palette.set_strip_style(if filled {
                StripStyle::Filled
            } else {
                StripStyle::Separator
            });
            let named: Vec<Color> = text_grounds(&palette).iter().map(|(_, c)| *c).collect();
            let theirs: Vec<Color> = palette.text_grounds().iter().flatten().copied().collect();
            assert_eq!(named, theirs, "cards {cards}, filled {filled}");
        }
    }
}

// ─── The folder ─────────────────────────────────────────────────────────────

/// **A file nothing reads is reported, a document for people is not, and
/// what is hidden is reported once** -- a hidden folder not looked inside.
#[test]
fn files_nothing_reads_are_reported_but_documents_are_not() {
    let theme = Theme::tidy();
    theme
        .write("extra.txt", "unread")
        .write("README.md", "about")
        .write("LICENSE", "terms")
        .write("docs/README.md", "not at the top")
        .write(".hidden", "x")
        .write(".git/config", "[core]");
    let report = theme.check();
    assert_said(&report, Severity::Warning, "extra.txt", "nothing reads");
    assert_said(
        &report,
        Severity::Warning,
        "docs/README.md",
        "nothing reads",
    );
    assert_said(&report, Severity::Warning, ".hidden", "hidden file");
    assert_said(&report, Severity::Warning, ".git", "hidden folder");
    for quiet in ["README.md", "LICENSE", ".git/config"] {
        assert!(
            !report.findings.iter().any(|f| f.place == quiet),
            "{quiet}: {}",
            listing(&report)
        );
    }
    assert_eq!(report.files, 8, "the hidden folder's file is not counted");
}

/// **A folder too deep, or too full, is said so** rather than walked for
/// ever.
#[test]
fn a_folder_too_deep_or_too_full_is_said_so() {
    let theme = Theme::tidy();
    // The folder `MAX_DEPTH` down is reported, and nothing in it read.
    let deep = (0..MAX_DEPTH)
        .map(|i| format!("d{i}"))
        .collect::<Vec<_>>()
        .join("/");
    theme.write(&format!("{deep}/x.txt"), "deep");
    let report = theme.check();
    assert_said(&report, Severity::Error, &deep, "folders down");
    assert!(!report.findings.iter().any(|f| f.place.ends_with("x.txt")));

    // The bound on entries, lowered from `MAX_ENTRIES` so the test makes
    // eight files rather than ten thousand. The tidy theme's four entries at
    // the top, its icon, and eight in `many` are thirteen: a bound of
    // thirteen says nothing, one of twelve says so.
    let full = Theme::tidy();
    full.mkdir("many");
    for i in 0..8 {
        fs::write(full.dir.join("many").join(i.to_string()), b"").unwrap();
    }
    let bounded = |max_entries: usize| {
        let mut checker = Checker::new(&full.dir);
        checker.max_entries = max_entries;
        checker.run();
        checker.finish()
    };
    assert!(!said(&bounded(13), Severity::Error, "", "holds more than"));
    assert_said(&bounded(12), Severity::Error, "", "holds more than 12");
    assert_eq!(Checker::new(&full.dir).max_entries, MAX_ENTRIES);
}

// ─── Wallpapers ─────────────────────────────────────────────────────────────

/// **A theme's wallpapers are pictures the desktop can show**: those its
/// folder bundles are read -- one that is no picture is an error -- and a
/// recommended one, in the folder or elsewhere in the theme, covers the
/// axis.
#[test]
fn wallpapers_are_pictures_the_desktop_can_show() {
    let theme = Theme::tidy();
    theme
        .write(
            "theme.yaml",
            TIDY.replace(
                "supports: [colors, icons]",
                "supports: [colors, icons, wallpapers]",
            ) + "wallpapers:\n  dark: wallpapers/night.png\n  light: day.png\n",
        )
        .write("wallpapers/night.png", png())
        .write("wallpapers/spare.png", png())
        .write("wallpapers/broken.png", "not a picture")
        .write("day.png", png());
    let report = theme.check();
    assert!(
        report.covers.contains(&"wallpapers"),
        "{}",
        listing(&report)
    );
    assert_said(
        &report,
        Severity::Error,
        "wallpapers/broken.png",
        "is not a picture",
    );
    for quiet in ["wallpapers/night.png", "wallpapers/spare.png", "day.png"] {
        assert!(
            !report.findings.iter().any(|f| f.place == quiet),
            "{quiet}: {}",
            listing(&report)
        );
    }
}

/// **A recommendation the theme cannot show is said**: outside its folder an
/// error, missing a warning -- and with neither shown, the axis is not
/// covered.
#[test]
fn a_wallpaper_the_theme_cannot_show_is_said() {
    let theme = Theme::tidy();
    theme.write(
        "theme.yaml",
        format!("{TIDY}wallpapers:\n  dark: ../outside.png\n  light: gone.png\n"),
    );
    let report = theme.check();
    assert_said(
        &report,
        Severity::Error,
        "theme.yaml",
        "dark wallpaper `../outside.png` is outside",
    );
    assert_said(
        &report,
        Severity::Warning,
        "theme.yaml",
        "light wallpaper `gone.png` is not in",
    );
    assert!(
        !report.covers.contains(&"wallpapers"),
        "{}",
        listing(&report)
    );

    // A recommendation that is no picture covers nothing either.
    let theme = Theme::tidy();
    theme
        .write(
            "theme.yaml",
            format!("{TIDY}wallpapers:\n  dark: wallpapers/x.png\n"),
        )
        .write("wallpapers/x.png", "words");
    let report = theme.check();
    assert_said(
        &report,
        Severity::Error,
        "wallpapers/x.png",
        "is not a picture",
    );
    assert!(!report.covers.contains(&"wallpapers"));

    // And a section that names no wallpaper for either mode is as good as
    // not there, and is said to be, as any other such section is.
    let theme = Theme::tidy();
    theme.write(
        "theme.yaml",
        format!("{TIDY}wallpapers:\n  dusk: dusk.png\n"),
    );
    assert_said(
        &theme.check(),
        Severity::Warning,
        "theme.yaml",
        "`wallpapers` sets nothing this desktop can use",
    );
}

/// **A theme named for the built-in one is told what that means.**
#[test]
fn a_theme_named_for_the_built_in_one_is_told_so() {
    let theme = Theme::empty(themes::BUILT_IN);
    theme
        .write("theme.yaml", TIDY)
        .write("icons/folder.svg", SQUARE)
        .write("shot.png", png());
    assert_said(
        &theme.check(),
        Severity::Note,
        "",
        "the built-in theme's name",
    );
    assert!(!said(
        &Theme::tidy().check(),
        Severity::Note,
        "",
        "built-in theme's name"
    ));
}

/// **A file marked to run is reported**, on a system with such a mark.
#[cfg(unix)]
#[test]
fn a_file_marked_to_run_is_reported() {
    use std::os::unix::fs::PermissionsExt;
    let theme = Theme::tidy();
    let shot = theme.dir.join("shot.png");
    fs::set_permissions(&shot, fs::Permissions::from_mode(0o755)).unwrap();
    assert_said(
        &theme.check(),
        Severity::Warning,
        "shot.png",
        "marked as a program",
    );
}

// ─── Fonts ──────────────────────────────────────────────────────────────────

/// The checker, asking about fonts as `installed` and `fixed_pitch` answer
/// rather than as this machine would: what a machine has installed is no
/// fixture.
fn checked_with_fonts(
    theme: &Theme,
    installed: fn(&str) -> bool,
    fixed_pitch: fn(&str) -> bool,
) -> Report {
    let mut checker = Checker::new(&theme.dir);
    checker.fonts = FontQuestions {
        installed,
        fixed_pitch,
    };
    checker.run();
    checker.finish()
}

/// **A theme's fonts are said against this machine's**: a family it lacks is
/// a note -- the theme is used where it may be installed -- a fixed-pitch
/// recommendation that is installed and is not fixed-pitch is a warning, and
/// a section naming any family covers the axis, installed or not.
#[test]
fn fonts_are_said_against_the_machines() {
    let theme = Theme::tidy();
    theme.write(
        "theme.yaml",
        format!("{TIDY}fonts:\n  ui: [Inter, Cantarell]\n  mono: [Fira Code, Cantarell]\n"),
    );
    let report = checked_with_fonts(
        &theme,
        |family| family == "Cantarell" || family == "Fira Code",
        |family| family == "Fira Code",
    );
    assert_said(
        &report,
        Severity::Note,
        "theme.yaml",
        "`Inter` (`fonts.ui`) is not installed here",
    );
    assert!(
        !said(&report, Severity::Note, "theme.yaml", "`Cantarell`"),
        "{}",
        listing(&report)
    );
    assert_said(
        &report,
        Severity::Warning,
        "theme.yaml",
        "`Cantarell` (`fonts.mono`) is not fixed-pitch",
    );
    assert!(
        !said(&report, Severity::Warning, "theme.yaml", "`Fira Code`"),
        "{}",
        listing(&report)
    );
    assert!(
        report.covers.contains(&themes::FONTS_SECTION),
        "{:?}",
        report.covers
    );

    // None of them here: notes, no warning -- a family not installed is not
    // known to be proportional -- and the axis still covered.
    let none = checked_with_fonts(&theme, |_| false, |_| false);
    assert!(
        !said(&none, Severity::Warning, "theme.yaml", "fixed-pitch"),
        "{}",
        listing(&none)
    );
    assert_said(
        &none,
        Severity::Note,
        "theme.yaml",
        "`Fira Code` (`fonts.mono`) is not installed here",
    );
    assert!(none.covers.contains(&themes::FONTS_SECTION));
}

/// **Fonts are an axis now, not a planned one**: a theme claiming them and
/// recommending none is told so, and a section that names no family sets
/// nothing.
#[test]
fn fonts_are_an_axis_a_theme_can_claim() {
    let claims = Theme::tidy();
    claims.write(
        "theme.yaml",
        TIDY.replace(
            "supports: [colors, icons]",
            "supports: [colors, icons, fonts]",
        ),
    );
    let report = claims.check();
    assert_said(
        &report,
        Severity::Warning,
        "theme.yaml",
        "lists `fonts`, but the theme sets no fonts",
    );
    assert!(
        !said(&report, Severity::Note, "theme.yaml", "no axis for yet"),
        "{}",
        listing(&report)
    );

    let empty = Theme::tidy();
    empty.write("theme.yaml", format!("{TIDY}fonts:\n  title: Georgia\n"));
    assert_said(
        &empty.check(),
        Severity::Warning,
        "theme.yaml",
        "`fonts` sets nothing this desktop can use",
    );
}
