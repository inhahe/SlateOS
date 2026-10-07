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

use super::*;

/// SlateOS theme roots and sound-theme roots inside a scratch directory.
struct Fixture {
    scratch: ScratchDir,
}

impl Fixture {
    fn new(tag: &str) -> Self {
        Self {
            scratch: ScratchDir::new(&format!("slateos-sounds-{tag}")),
        }
    }

    fn root(&self, root: &str) -> PathBuf {
        self.scratch.dir().join(root)
    }

    /// The theme `id`, read from this fixture's roots: `user` and `system`
    /// for SlateOS themes, then `share` and `usr` for other desktops'.
    fn theme(&self, id: &str) -> SoundTheme {
        SoundTheme::named(
            OsStr::new(id),
            ThemeDirs {
                user: Some(self.root("user")),
                system: self.root("system"),
            },
            vec![self.root("share"), self.root("usr")],
        )
    }

    /// Put the file `file` -- a name with its extension -- in `theme`'s
    /// directory `dir` under `root`, and give back its path.
    fn put(&self, root: &str, theme: &str, dir: &str, file: &str) -> PathBuf {
        let at = self.root(root).join(theme).join(dir);
        fs::create_dir_all(&at).unwrap();
        let path = at.join(file);
        fs::write(&path, b"a sound").unwrap();
        path
    }

    /// Give `theme` under `root` an `index.theme` of `body`.
    fn index(&self, root: &str, theme: &str, body: &str) {
        let dir = self.root(root).join(theme);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(INDEX_FILE), body).unwrap();
    }
}

fn file(path: &Path) -> SoundChoice {
    SoundChoice::File(path.to_path_buf())
}

/// **A theme's sound is its file of that name in `stereo`**, the user's copy
/// before the system's, SlateOS's roots before other desktops'.
#[test]
fn a_themes_sound_is_its_file_of_that_name() {
    let f = Fixture::new("own");
    let system = f.put("system", "chimes", "stereo", "bell.oga");
    assert_eq!(f.theme("chimes").sound("bell"), file(&system));
    let user = f.put("user", "chimes", "stereo", "bell.oga");
    assert_eq!(
        f.theme("chimes").sound("bell"),
        file(&user),
        "the user's copy first"
    );
    let f = Fixture::new("roots");
    let share = f.put("share", "chimes", "stereo", "bell.oga");
    f.put("usr", "chimes", "stereo", "bell.oga");
    assert_eq!(
        f.theme("chimes").sound("bell"),
        file(&share),
        "the first root wins"
    );
}

/// **The extensions are tried in the specification's order**: `.disabled`
/// silences the event, then `.oga`, `.ogg`, `.wav`.
#[test]
fn the_extensions_are_tried_in_order() {
    let f = Fixture::new("ext");
    let wav = f.put("share", "t", "stereo", "bell.wav");
    assert_eq!(f.theme("t").sound("bell"), file(&wav));
    let ogg = f.put("share", "t", "stereo", "bell.ogg");
    assert_eq!(f.theme("t").sound("bell"), file(&ogg));
    let oga = f.put("share", "t", "stereo", "bell.oga");
    assert_eq!(f.theme("t").sound("bell"), file(&oga));
    f.put("share", "t", "stereo", "bell.disabled");
    assert_eq!(f.theme("t").sound("bell"), SoundChoice::Silent);
    // A file of another kind is no sound.
    f.put("share", "t", "stereo", "trash-empty.mp3");
    assert_eq!(
        f.theme("t").sound("trash-empty"),
        SoundChoice::BuiltIn("trash-empty".to_string())
    );
}

/// **A theme inherits from the themes its `index.theme` names, then from
/// the freedesktop theme**, in that order, and its own sound comes first.
#[test]
fn a_theme_inherits_and_then_falls_back_to_freedesktop() {
    let f = Fixture::new("inherit");
    f.index(
        "share",
        "child",
        "[Sound Theme]\nName=Child\nInherits=parent\n",
    );
    let parent = f.put("share", "parent", "stereo", "bell.oga");
    let fallback = f.put("usr", "freedesktop", "stereo", "message.oga");
    let own = f.put("share", "child", "stereo", "complete.oga");
    let child = f.theme("child");
    assert_eq!(child.sound("bell"), file(&parent));
    assert_eq!(child.sound("message"), file(&fallback));
    assert_eq!(child.sound("complete"), file(&own));
    // Its own beats its parent's.
    let mine = f.put("share", "child", "stereo", "bell.oga");
    assert_eq!(child.sound("bell"), file(&mine));
}

/// **An event's name is cut at its last hyphen when no theme has it** --
/// only once every theme has been asked for the whole name -- and then
/// falls to the built-in sound of the name asked for.
#[test]
fn an_events_name_is_cut_after_every_theme_is_asked() {
    let f = Fixture::new("cut");
    let general = f.put("share", "t", "stereo", "dialog-error.oga");
    assert_eq!(f.theme("t").sound("dialog-error-serious"), file(&general));
    // The fallback theme's exact name beats the theme's shorter one.
    let exact = f.put("usr", "freedesktop", "stereo", "dialog-error-serious.oga");
    assert_eq!(f.theme("t").sound("dialog-error-serious"), file(&exact));
    assert_eq!(
        f.theme("t").sound("window-attention"),
        SoundChoice::BuiltIn("window-attention".to_string())
    );
}

/// **An `index.theme` may name other directories for stereo**, and those
/// are looked in instead of `stereo`; one for another output profile is
/// not; and none can point out of the theme.
#[test]
fn an_index_names_the_stereo_directories() {
    let f = Fixture::new("dirs");
    f.index(
        "share",
        "t",
        "[Sound Theme]\nDirectories=two,six,../escape\n\n[two]\nOutputProfile=stereo\n\n[six]\nOutputProfile=5.1\n",
    );
    let two = f.put("share", "t", "two", "bell.oga");
    f.put("share", "t", "six", "message.oga");
    f.put("share", "t", "stereo", "message.oga");
    f.put("share", "escape", "", "complete.oga");
    let theme = f.theme("t");
    assert_eq!(theme.sound("bell"), file(&two));
    assert_eq!(
        theme.sound("message"),
        SoundChoice::BuiltIn("message".to_string()),
        "neither the 5.1 directory nor the unnamed stereo one"
    );
    assert_eq!(
        theme.sound("complete"),
        SoundChoice::BuiltIn("complete".to_string())
    );
}

/// **A theme that inherits from itself, by any road, ends.**
#[test]
fn a_theme_that_inherits_from_itself_ends() {
    let f = Fixture::new("loop");
    f.index("share", "a", "[Sound Theme]\nInherits=b\n");
    f.index("share", "b", "[Sound Theme]\nInherits=a\n");
    assert_eq!(
        f.theme("a").sound("bell"),
        SoundChoice::BuiltIn("bell".to_string())
    );
}

/// **What is not an event's name finds nothing and makes no path**: upper
/// case, separators, dots, nothing at all. A name is trimmed first.
#[test]
fn what_is_not_an_events_name_finds_nothing() {
    let f = Fixture::new("names");
    // Found by `Bell` only on a file system that ignores case, as the
    // Windows host's does -- so `Bell` is refused before any is asked.
    f.put("share", "t", "stereo", "Bell.oga");
    for bad in [
        "",
        "Bell",
        "../bell",
        "a/b",
        "bell.oga",
        "bell\0",
        &"x".repeat(129),
    ] {
        assert_eq!(f.theme("t").sound(bad), SoundChoice::Silent, "{bad:?}");
    }
    assert!(is_valid_name("message-new-instant"));
    assert!(is_valid_name("bell_2"));
    assert!(is_valid_name(&"x".repeat(128)));
    let chime = f.put("share", "t", "stereo", "chime.oga");
    assert_eq!(f.theme("t").sound(" chime "), file(&chime), "trimmed");
    assert_eq!(
        f.theme("t").sound(" knock\n"),
        SoundChoice::BuiltIn("knock".to_string()),
        "trimmed"
    );
}

/// **The built-in theme is the built-in sounds** -- not the `freedesktop`
/// theme's, installed or not -- until its folder is given a sound.
#[test]
fn the_built_in_theme_is_the_built_in_sounds() {
    let f = Fixture::new("builtin");
    f.put("usr", FALLBACK_THEME, "stereo", "bell.oga");
    let theme = f.theme(themes::BUILT_IN);
    assert_eq!(
        theme.sound("bell"),
        SoundChoice::BuiltIn("bell".to_string())
    );
    assert_eq!(
        theme.sound("bell-terminal"),
        SoundChoice::BuiltIn("bell-terminal".to_string()),
        "nor freedesktop's for a cut of the name"
    );
    let edited = f.put("user", themes::BUILT_IN, "stereo", "bell.wav");
    assert_eq!(theme.sound("bell"), file(&edited));
    assert!(SoundTheme::built_in().is_built_in());
    assert_eq!(SoundTheme::default(), SoundTheme::built_in());
}

/// **The installed themes are listed for a picker**: the built-in one
/// first, then each folder with sounds, once, by its own name.
#[test]
fn the_installed_themes_are_listed() {
    let f = Fixture::new("list");
    f.put("share", "yaru", "stereo", "bell.oga");
    f.index("share", "yaru", "[Sound Theme]\nName=Yaru\n");
    f.put("usr", "yaru", "stereo", "bell.oga");
    f.index(
        "usr",
        "freedesktop",
        "[Sound Theme]\nName=Default\nDirectories=stereo\n",
    );
    fs::create_dir_all(f.root("usr").join("empty")).unwrap();
    let list = available_in(&[f.root("share"), f.root("usr")]);
    let names: Vec<&str> = list.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, [themes::BUILT_IN_NAME, "Default", "Yaru"]);
    assert!(list[0].built_in && !list[1].built_in);
}

/// **The other desktops' roots are the data directories' `sounds`**, the
/// user's first; with none named, the specification's defaults.
#[test]
fn the_other_roots_are_the_data_directories_sounds() {
    // An absolute path on the host the tests run on: a Windows host wants a
    // drive as well as the leading `/`.
    let abs = |tail: &str| {
        PathBuf::from(if cfg!(windows) {
            format!("C:/{tail}")
        } else {
            format!("/{tail}")
        })
    };
    let list = std::env::join_paths([abs("usr/local/share"), abs("opt/share")]).unwrap();
    let dirs = sound_dirs_from(|name| match name {
        "XDG_DATA_HOME" => Some(abs("home/u/.local/share").into_os_string()),
        "XDG_DATA_DIRS" => Some(list.clone()),
        _ => None,
    });
    assert_eq!(
        dirs,
        [
            abs("home/u/.local/share").join("sounds"),
            abs("usr/local/share").join("sounds"),
            abs("opt/share").join("sounds"),
        ]
    );
    let defaults = sound_dirs_from(|_| None);
    assert_eq!(
        defaults,
        [
            PathBuf::from("/usr/local/share").join("sounds"),
            PathBuf::from("/usr/share").join("sounds"),
        ]
    );
}

/// **The desktop's events are listed once each, by names that are events'**
/// -- each a key `sounds.events` can hold -- and each with a label.
#[test]
fn the_desktops_events_are_listed_once_each() {
    let mut names: Vec<&str> = SHELL_EVENTS.iter().map(|e| e.name).collect();
    for event in SHELL_EVENTS {
        assert!(is_valid_name(event.name), "{}", event.name);
        assert!(!event.label.trim().is_empty(), "{}", event.name);
    }
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), SHELL_EVENTS.len(), "a name listed twice");
}

/// **The fallback cuts a name at each hyphen, never to nothing.**
#[test]
fn a_name_is_cut_at_each_hyphen() {
    assert_eq!(
        cuts("dialog-error-serious").collect::<Vec<_>>(),
        ["dialog-error-serious", "dialog-error", "dialog"]
    );
    assert_eq!(cuts("bell").collect::<Vec<_>>(), ["bell"]);
    assert_eq!(cuts("-x").collect::<Vec<_>>(), ["-x"]);
}
