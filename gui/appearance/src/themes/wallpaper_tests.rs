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
use std::path::PathBuf;

use scratchdir::ScratchDir;

use super::super::{ThemeError, available_in, parse};
use super::*;

/// Themes directories in a scratch directory: a user's and a system's.
struct Dirs {
    _scratch: ScratchDir,
    dirs: ThemeDirs,
}

impl Dirs {
    fn new() -> Self {
        let scratch = ScratchDir::new("wallpaper-themes");
        let dirs = ThemeDirs {
            user: Some(scratch.dir().join("user")),
            system: scratch.dir().join("system"),
        };
        Self {
            _scratch: scratch,
            dirs,
        }
    }

    /// Install the theme `name` in the user's directory: its `theme.yaml`
    /// saying `text`, and an empty file for each of `pictures`.
    fn install(&self, name: &str, text: &str, pictures: &[&str]) -> PathBuf {
        let dir = self.dirs.user.as_ref().unwrap().join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(super::super::FILE_NAME), text).unwrap();
        for picture in pictures {
            let path = dir.join(picture);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, b"a picture").unwrap();
        }
        dir
    }
}

/// **A theme's `wallpapers` section names a picture for each mode**, as
/// written; one naming none, or no section, is no recommendation; and a key
/// that is no mode is said.
#[test]
fn the_section_names_a_picture_for_each_mode() {
    let file = parse("wallpapers:\n  dark: wallpapers/night.jpg\n  light: day.png\n");
    assert_eq!(
        file.wallpapers,
        Some(WallpaperNames {
            dark: Some("wallpapers/night.jpg".to_owned()),
            light: Some("day.png".to_owned()),
        })
    );
    assert!(file.warnings.is_empty(), "{:?}", file.warnings);

    assert_eq!(parse("colors:\n  base: \"#000000\"\n").wallpapers, None);
    assert_eq!(parse("wallpapers:\n  dark: \"\"\n").wallpapers, None);
    let odd = parse("wallpapers:\n  dark: a.png\n  evening: b.png\n");
    assert_eq!(odd.wallpapers.unwrap().dark.as_deref(), Some("a.png"));
    assert!(
        odd.warnings
            .iter()
            .any(|w| w.contains("`wallpapers.evening` is ignored")),
        "{:?}",
        odd.warnings
    );
}

/// **The chosen theme's picture for the mode is shown** -- its other one
/// when it recommends only one, since a picture is not made for one mode as
/// colours are -- and the built-in theme recommends none.
#[test]
fn a_theme_gives_its_picture_for_the_mode() {
    let dirs = Dirs::new();
    let dir = dirs.install(
        "aurora",
        "wallpapers:\n  dark: wallpapers/night.png\n  light: wallpapers/day.png\n",
        &["wallpapers/night.png", "wallpapers/day.png"],
    );
    let theme = WallpaperTheme::load_from(&dirs.dirs, OsStr::new("aurora"));
    assert_eq!(theme.problem(), None);
    assert_eq!(
        theme.picture(false),
        Some(dir.join("wallpapers").join("night.png").as_path())
    );
    assert_eq!(
        theme.picture(true),
        Some(dir.join("wallpapers").join("day.png").as_path())
    );

    let one = dirs.install("dusk", "wallpapers:\n  dark: dusk.png\n", &["dusk.png"]);
    let theme = WallpaperTheme::load_from(&dirs.dirs, OsStr::new("dusk"));
    assert_eq!(theme.picture(true), Some(one.join("dusk.png").as_path()));
    assert_eq!(theme.picture(false), Some(one.join("dusk.png").as_path()));

    let built_in = WallpaperTheme::load_from(&dirs.dirs, OsStr::new(super::super::BUILT_IN));
    assert_eq!(built_in, WallpaperTheme::built_in());
    assert!(built_in.picture(true).is_none() && built_in.picture(false).is_none());
}

/// **A recommendation the theme's folder does not hold is dropped and
/// said** -- a missing picture, one outside the folder -- and a theme left
/// with none recommends nothing, saying why; as does one not installed.
#[test]
fn what_cannot_be_shown_is_dropped_and_said() {
    let dirs = Dirs::new();
    dirs.install(
        "patchy",
        "wallpapers:\n  dark: gone.png\n  light: ../outside.png\n",
        &[],
    );
    let theme = WallpaperTheme::load_from(&dirs.dirs, OsStr::new("patchy"));
    assert!(theme.picture(false).is_none());
    let problem = theme.problem().expect("no picture is a problem");
    assert!(problem.contains("recommends no wallpaper"), "{problem}");
    assert!(problem.contains("your own wallpaper"), "{problem}");

    dirs.install(
        "half",
        "wallpapers:\n  dark: here.png\n  light: gone.png\n",
        &["here.png"],
    );
    let theme = WallpaperTheme::load_from(&dirs.dirs, OsStr::new("half"));
    assert_eq!(theme.problem(), None);
    assert!(theme.picture(true).is_some_and(|p| p.ends_with("here.png")));
    assert!(
        theme
            .warnings()
            .iter()
            .any(|w| w.contains("`gone.png` is ignored")),
        "{:?}",
        theme.warnings()
    );

    let absent = WallpaperTheme::load_from(&dirs.dirs, OsStr::new("nowhere"));
    assert!(absent.picture(false).is_none());
    assert!(
        absent
            .problem()
            .unwrap()
            .contains(&ThemeError::NotInstalled.to_string())
    );
    assert_eq!(absent.id(), "nowhere", "the choice is kept");
}

/// **A theme list says which themes recommend a wallpaper, and offers the
/// pictures each bundles** -- recommended or not, by name.
#[test]
fn a_list_offers_a_themes_pictures() {
    let dirs = Dirs::new();
    dirs.install(
        "aurora",
        "wallpapers:\n  dark: wallpapers/night.png\n",
        &["wallpapers/night.png", "wallpapers/a-spare.jpg"],
    );
    dirs.install("plain", "colors:\n  base: \"#000000\"\n", &[]);
    dirs.install(
        "broken",
        "wallpapers:\n  dark: gone.png\n",
        &["wallpapers/x.png"],
    );
    let list = available_in(&dirs.dirs);
    let find = |id: &str| list.iter().find(|t| t.id == OsStr::new(id)).unwrap();

    let aurora = find("aurora");
    assert!(aurora.provides_wallpapers());
    let names: Vec<_> = aurora
        .wallpapers
        .iter()
        .map(|p| p.file_name().unwrap().to_owned())
        .collect();
    assert_eq!(names, ["a-spare.jpg", "night.png"]);
    assert!(!find("plain").provides_wallpapers());
    assert!(find("plain").wallpapers.is_empty());
    // Recommends a picture that is not there: offers what it bundles, and
    // cannot be chosen for the axis.
    assert!(!find("broken").provides_wallpapers());
    assert_eq!(find("broken").wallpapers.len(), 1);
    // The built-in theme recommends none.
    assert!(!list[0].provides_wallpapers());

    // Not even when the system's copy of its file recommends one: choosing
    // the built-in theme for the axis is choosing your own picture
    // (`WallpaperTheme::built_in`), so a list saying otherwise would offer
    // what choosing it does not give.
    let system = dirs.dirs.system.join(super::super::BUILT_IN);
    fs::create_dir_all(system.join(WALLPAPERS_DIR)).unwrap();
    fs::write(
        system.join(super::super::FILE_NAME),
        "wallpapers:\n  dark: wallpapers/x.png\n",
    )
    .unwrap();
    fs::write(system.join(WALLPAPERS_DIR).join("x.png"), b"a picture").unwrap();
    let list = available_in(&dirs.dirs);
    assert_eq!(list[0].id, OsStr::new(super::super::BUILT_IN));
    assert!(!list[0].provides_wallpapers());
    assert_eq!(list[0].wallpapers.len(), 1, "what it bundles is offered");
}

/// **The pictures a folder bundles are its `wallpapers` folder's files**,
/// by name, and none where it has no such folder.
#[test]
fn bundled_pictures_are_the_folders_files() {
    let dirs = Dirs::new();
    let dir = dirs.install(
        "aurora",
        "",
        &["wallpapers/b.png", "wallpapers/a.png", "elsewhere.png"],
    );
    fs::create_dir_all(dir.join(WALLPAPERS_DIR).join("sub")).unwrap();
    let names: Vec<_> = bundled(&dir)
        .into_iter()
        .map(|p| p.file_name().unwrap().to_owned())
        .collect();
    assert_eq!(names, ["a.png", "b.png"], "files only, by name");
    assert!(bundled(&dir.join("nothing")).is_empty());
}
