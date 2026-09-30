#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use super::*;
use scratchdir::ScratchDir;
use yamldoc::Document;

/// A user directory and one system directory in a scratch directory.
struct Fixture {
    scratch: ScratchDir,
}

impl Fixture {
    fn new(tag: &str) -> Self {
        Self {
            scratch: ScratchDir::new(&format!("slateos-servicemenus-{tag}")),
        }
    }

    fn dirs(&self) -> Dirs {
        Dirs {
            user: Some(self.scratch.dir().join("user")),
            system: vec![self.scratch.dir().join("system")],
        }
    }

    /// Install `text` as `name` under `origin`'s `sub` directory.
    fn install_in(&self, origin: Origin, sub: &str, name: &str, text: &[u8]) {
        let base = match origin {
            Origin::User => "user",
            Origin::System => "system",
        };
        let dir = self.scratch.dir().join(base).join(sub);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(name), text).unwrap();
    }

    fn install(&self, origin: Origin, name: &str, text: &str) {
        self.install_in(origin, "kio/servicemenus", name, text.as_bytes());
    }
}

/// A menu with every key this reads.
const FULL: &str = "\
[Desktop Entry]
Type=Service
MimeType=image/png;image/jpeg;
Actions=rotate;flip;
X-KDE-Submenu=Image
X-KDE-Submenu[de]=Bild
X-KDE-Priority=TopLevel
X-KDE-MinNumberOfUrls=1
X-KDE-MaxNumberOfUrls=5
X-KDE-Require=Write;

[Desktop Action rotate]
Name=Rotate right
Name[de]=Rechts drehen
Icon=object-rotate-right
Exec=mogrify -rotate 90 %f

[Desktop Action flip]
Name=Flip
Exec=convert %F -flip
";

/// A one-item menu for everything, with `extra` lines in its entry group.
fn with_keys(extra: &str) -> String {
    format!(
        "[Desktop Entry]\nType=Service\nMimeType=all/all;\nActions=go;\n{extra}\n\n[Desktop Action go]\nName=Go\nExec=x\n"
    )
}

fn simple(mime: &str, exec: &str) -> String {
    format!(
        "[Desktop Entry]\nType=Service\nMimeType={mime}\nActions=go;\n\n[Desktop Action go]\nName=Go\nExec={exec}\n"
    )
}

fn file(mime: &str) -> Target {
    Target {
        path: PathBuf::from(format!("/home/u/file.{}", mime.replace('/', "-"))),
        mime: mime.to_owned(),
        inherits: Vec::new(),
        is_dir: false,
        writable: true,
    }
}

fn folder() -> Target {
    Target {
        path: PathBuf::from("/home/u/folder"),
        mime: String::from("inode/directory"),
        inherits: Vec::new(),
        is_dir: true,
        writable: true,
    }
}

/// **The directories are the XDG specification's**, the user's apart from
/// the system's, a relative path in either ignored.
#[test]
fn the_directories_are_the_xdg_specifications() {
    let env = |pairs: &'static [(&'static str, &'static str)]| {
        move |name: &str| {
            pairs
                .iter()
                .find(|(k, _)| *k == name)
                .map(|(_, v)| OsString::from(*v))
        }
    };
    let d = Dirs::from_env(env(&[("HOME", "/home/u")]));
    assert_eq!(d.user, Some(PathBuf::from("/home/u/.local/share")));
    assert_eq!(
        d.system,
        [
            PathBuf::from("/usr/local/share"),
            PathBuf::from("/usr/share")
        ]
    );
    let d = Dirs::from_env(env(&[("XDG_DATA_HOME", "rel"), ("HOME", "/home/u")]));
    assert_eq!(d.user, None, "a relative XDG_DATA_HOME is ignored");
    // An empty variable is an unset one: the specification's defaults.
    let d = Dirs::from_env(env(&[
        ("XDG_DATA_HOME", ""),
        ("XDG_DATA_DIRS", ""),
        ("HOME", "/home/u"),
    ]));
    assert_eq!(d.user, Some(PathBuf::from("/home/u/.local/share")));
    assert_eq!(d.system.len(), 2, "{:?}", d.system);
    // Absolute on this machine, whichever it is, and a list in its own
    // separator.
    let abs = |name: &str| std::env::temp_dir().join(name);
    let list = std::env::join_paths([abs("a"), PathBuf::from("relative"), abs("b")]).unwrap();
    let get = |name: &str| match name {
        "XDG_DATA_HOME" => Some(abs("data").into_os_string()),
        "XDG_DATA_DIRS" => Some(list.clone()),
        _ => None,
    };
    let d = Dirs::from_env(get);
    assert_eq!(d.user, Some(abs("data")));
    assert_eq!(d.system, [abs("a"), abs("b")]);
}

/// **A service menu is read whole**: its kinds, its items with their
/// names, icons and commands, its submenu, its priority, the counts it
/// allows and what it requires -- in the reader's language.
#[test]
fn a_service_menu_is_read_whole() {
    let f = Fixture::new("full");
    f.install(Origin::System, "image.desktop", FULL);
    let found = scan(&f.dirs(), None);
    assert_eq!(found.skipped, []);
    let menu = &found.menus[0];
    assert_eq!(menu.id, "image.desktop");
    assert_eq!(menu.origin, Origin::System);
    assert_eq!(menu.mime_types, ["image/png", "image/jpeg"]);
    assert_eq!(menu.submenu.as_deref(), Some("Image"));
    assert!(menu.top_level);
    assert_eq!(
        menu.counts,
        Counts {
            min: Some(1),
            max: Some(5),
            exactly: vec![]
        }
    );
    assert!(menu.requires_write);
    assert_eq!(
        menu.actions
            .iter()
            .map(|a| a.name.as_str())
            .collect::<Vec<_>>(),
        ["Rotate right", "Flip"]
    );
    assert_eq!(menu.actions[0].icon.as_deref(), Some("object-rotate-right"));
    let german = Locale::parse("de_DE").unwrap();
    let found = scan(&f.dirs(), Some(&german));
    assert_eq!(found.menus[0].submenu.as_deref(), Some("Bild"));
    assert_eq!(found.menus[0].actions[0].name, "Rechts drehen");
}

/// **What is not a menu this desktop offers says why**, and costs only
/// itself.
#[test]
fn what_is_not_offered_says_why() {
    let f = Fixture::new("skipped");
    let cases: [(&str, String, &str); 9] = [
        (
            "no-type.desktop",
            String::from("[Desktop Entry]\nMimeType=all/all;\n"),
            "has no Type",
        ),
        (
            "app.desktop",
            String::from("[Desktop Entry]\nType=Application\nExec=x\n"),
            "is of type Application",
        ),
        (
            "no-mime.desktop",
            String::from(
                "[Desktop Entry]\nType=Service\nActions=a;\n[Desktop Action a]\nName=A\nExec=a\n",
            ),
            "names no kind of file",
        ),
        (
            "no-actions.desktop",
            String::from("[Desktop Entry]\nType=Service\nMimeType=all/all;\n"),
            "lists no actions",
        ),
        (
            "bad-action.desktop",
            String::from(
                "[Desktop Entry]\nType=Service\nMimeType=all/all;\nActions=a;\n[Desktop Action a]\nName=A\n",
            ),
            "a: it has no Exec",
        ),
        (
            "no-name.desktop",
            String::from(
                "[Desktop Entry]\nType=Service\nMimeType=all/all;\nActions=a;\n[Desktop Action a]\nExec=a\n",
            ),
            "a: it has no Name",
        ),
        (
            "running.desktop",
            with_keys("X-KDE-ShowIfRunning=org.kde.x"),
            "X-KDE-ShowIfRunning",
        ),
        (
            "remote.desktop",
            with_keys("X-KDE-Protocols=smb,ftp"),
            "X-KDE-Protocols",
        ),
        (
            "garbage.desktop",
            String::from("not an ini file at all ["),
            "is not a desktop entry",
        ),
    ];
    for (name, text, _) in &cases {
        f.install(Origin::System, name, text);
    }
    // One good menu among them is still offered.
    f.install(Origin::System, "good.desktop", &simple("all/all;", "x %f"));
    // A file not named as a desktop entry is not a menu, and not reported.
    f.install(Origin::System, "notes.txt", &simple("all/all;", "x"));
    f.install(Origin::System, "desktop", &simple("all/all;", "x"));
    let found = scan(&f.dirs(), None);
    assert_eq!(found.skipped.len(), cases.len(), "{:?}", found.skipped);
    assert_eq!(found.menus.len(), 1, "{:?}", found.menus);
    assert_eq!(found.menus[0].id, "good.desktop");
    for (name, _, why) in &cases {
        let skipped = found
            .skipped
            .iter()
            .find(|s| s.path.file_name() == Some(OsStr::new(name)))
            .unwrap_or_else(|| panic!("{name} was not reported"));
        assert!(skipped.why.contains(why), "{name}: {:?}", skipped.why);
    }
    // A protocol list naming this machine's files is fine.
    let f = Fixture::new("protocols");
    f.install(
        Origin::System,
        "local.desktop",
        &with_keys("X-KDE-Protocols=file,smb"),
    );
    assert_eq!(scan(&f.dirs(), None).menus.len(), 1);
    // An empty list is no restriction: an accident of authoring, not a wish
    // to hide the menu.
    let f = Fixture::new("no-protocols");
    f.install(
        Origin::System,
        "empty.desktop",
        &with_keys("X-KDE-Protocols="),
    );
    assert_eq!(scan(&f.dirs(), None).menus.len(), 1);
}

/// **Optional keys at their edges**: a blank submenu is none, a priority
/// other than `TopLevel` is the ordinary one, a count may have blanks about
/// it, and one that is not a number is no bound.
#[test]
fn optional_keys_at_their_edges() {
    let f = Fixture::new("edges");
    f.install(
        Origin::System,
        "m.desktop",
        &with_keys(
            "X-KDE-Submenu=  \nX-KDE-Priority=Low\nX-KDE-MaxNumberOfUrls=2 \nX-KDE-MinNumberOfUrls=one",
        ),
    );
    let found = scan(&f.dirs(), None);
    let menu = &found.menus[0];
    assert_eq!(menu.submenu, None);
    assert!(!menu.top_level);
    assert_eq!(
        menu.counts,
        Counts {
            min: None,
            max: Some(2),
            exactly: vec![]
        }
    );
}

/// **An item whose command the specification refuses is said, with why**
/// -- `%f` twice in one line, which the desktop entry specification allows
/// once -- and the menu's other items are offered.
#[test]
fn an_item_the_specification_refuses_is_said() {
    let f = Fixture::new("twice");
    f.install(
        Origin::System,
        "twice.desktop",
        "[Desktop Entry]\nType=Service\nMimeType=all/all;\nActions=bad;good;\n\n\
         [Desktop Action bad]\nName=Bad\nExec=convert %f %f\n\n\
         [Desktop Action good]\nName=Good\nExec=mogrify %f\n",
    );
    let found = scan(&f.dirs(), None);
    let menu = &found.menus[0];
    assert_eq!(menu.actions.len(), 1);
    assert_eq!(menu.actions[0].name, "Good");
    assert_eq!(menu.unusable.len(), 1, "{:?}", menu.unusable);
    assert!(
        menu.unusable[0].starts_with("bad: its Exec cannot be used: more than one"),
        "{:?}",
        menu.unusable
    );
}

/// **A service menu larger than any real one is not read.**
#[test]
fn a_file_past_the_limit_is_not_read() {
    // A menu padded with a comment to `len` bytes exactly.
    let padded = |len: usize| {
        let mut text = simple("all/all;", "x");
        text.push('#');
        text.push_str(&"p".repeat(len - text.len() - 1));
        text.push('\n');
        assert_eq!(text.len(), len);
        text
    };
    let limit = usize::try_from(MAX_FILE_BYTES).unwrap();
    let f = Fixture::new("large");
    f.install(Origin::System, "at-limit.desktop", &padded(limit));
    f.install(Origin::System, "past-limit.desktop", &padded(limit + 1));
    let found = scan(&f.dirs(), None);
    assert_eq!(found.menus.len(), 1, "{:?}", found.skipped);
    assert_eq!(found.menus[0].id, "at-limit.desktop");
    assert_eq!(found.skipped.len(), 1);
    assert!(
        found.skipped[0].path.ends_with("past-limit.desktop")
            && found.skipped[0].why.contains("over the"),
        "{:?}",
        found.skipped
    );
}

/// **The user's copy of a name shadows the system's** -- `Hidden=true`
/// removes it, and a copy gone wrong is said rather than replaced by the
/// system's. Both of KDE's directories are read, the first name found
/// winning.
#[test]
fn the_users_copy_shadows_the_systems() {
    let f = Fixture::new("shadow");
    f.install(Origin::System, "a.desktop", &simple("all/all;", "system-a"));
    f.install(Origin::User, "a.desktop", &simple("all/all;", "user-a"));
    f.install(Origin::System, "b.desktop", &simple("all/all;", "system-b"));
    f.install(
        Origin::User,
        "b.desktop",
        "[Desktop Entry]\nType=Service\nHidden=true\n",
    );
    f.install(Origin::System, "c.desktop", &simple("all/all;", "system-c"));
    f.install(Origin::User, "c.desktop", "[Desktop Entry]\nType=Service\n");
    f.install_in(
        Origin::System,
        "kservices5/ServiceMenus",
        "d.desktop",
        simple("all/all;", "old-d").as_bytes(),
    );
    f.install_in(
        Origin::System,
        "kservices5/ServiceMenus",
        "a.desktop",
        simple("all/all;", "old-a").as_bytes(),
    );
    // In one data directory, KDE's current place before its older one.
    f.install(Origin::System, "e.desktop", &simple("all/all;", "new-e"));
    f.install_in(
        Origin::System,
        "kservices5/ServiceMenus",
        "e.desktop",
        simple("all/all;", "old-e").as_bytes(),
    );
    let found = scan(&f.dirs(), None);
    let ids: Vec<&OsStr> = found.menus.iter().map(|m| m.id.as_os_str()).collect();
    assert_eq!(ids, ["a.desktop", "e.desktop", "d.desktop"]);
    assert_eq!(found.menus[1].actions[0].exec.program(), "new-e");
    assert_eq!(found.menus[0].origin, Origin::User);
    assert_eq!(found.menus[0].actions[0].exec.program(), "user-a");
    assert_eq!(found.menus[2].actions[0].exec.program(), "old-d");
    assert_eq!(found.skipped.len(), 1, "{:?}", found.skipped);
    assert!(
        found.skipped[0]
            .path
            .ends_with("user/kio/servicemenus/c.desktop")
    );
}

/// **A menu is for the kinds it names**: exactly, by `type/*`, `all/all`
/// for anything, `all/allfiles` for files but not folders,
/// `inode/directory` for folders -- every target one of them.
#[test]
fn a_menu_is_for_the_kinds_it_names() {
    let menu = |mimes: &[&str]| ServiceMenu {
        id: OsString::from("m.desktop"),
        origin: Origin::System,
        path: PathBuf::from("m.desktop"),
        mime_types: mimes.iter().map(|m| (*m).to_owned()).collect(),
        submenu: None,
        top_level: false,
        counts: Counts::default(),
        requires_write: false,
        actions: Vec::new(),
        unusable: Vec::new(),
    };
    let png = file("image/png");
    let text = file("text/plain");
    let dir = folder();
    assert!(menu(&["image/png"]).applies_to(std::slice::from_ref(&png)));
    assert!(menu(&["IMAGE/PNG"]).applies_to(std::slice::from_ref(&png)));
    assert!(!menu(&["image/png"]).applies_to(std::slice::from_ref(&text)));
    assert!(menu(&["image/*"]).applies_to(std::slice::from_ref(&png)));
    assert!(!menu(&["image/*"]).applies_to(std::slice::from_ref(&text)));
    assert!(menu(&["all/all"]).applies_to(&[png.clone(), dir.clone()]));
    assert!(menu(&["all/allfiles"]).applies_to(&[png.clone(), text.clone()]));
    assert!(!menu(&["all/allfiles"]).applies_to(&[png.clone(), dir.clone()]));
    assert!(menu(&["inode/directory"]).applies_to(std::slice::from_ref(&dir)));
    assert!(!menu(&["inode/directory"]).applies_to(std::slice::from_ref(&png)));
    // Every target must be one of its kinds.
    assert!(!menu(&["image/*"]).applies_to(&[png.clone(), text.clone()]));
    assert!(menu(&["image/*", "text/plain"]).applies_to(&[png.clone(), text]));
    // Nothing is for nothing.
    assert!(!menu(&["all/all"]).applies_to(&[]));
}

/// **A menu is for as many things as it allows, and writable ones where it
/// requires them.**
#[test]
fn counts_and_writability_are_held_to() {
    let mut m = ServiceMenu {
        id: OsString::from("m.desktop"),
        origin: Origin::System,
        path: PathBuf::from("m.desktop"),
        mime_types: vec![String::from("all/all")],
        submenu: None,
        top_level: false,
        counts: Counts {
            min: Some(2),
            max: Some(3),
            exactly: vec![],
        },
        requires_write: false,
        actions: Vec::new(),
        unusable: Vec::new(),
    };
    let one = [file("a/b")];
    let two = [file("a/b"), file("a/c")];
    let four = [file("a/b"), file("a/c"), file("a/d"), file("a/e")];
    let three = [file("a/b"), file("a/c"), file("a/d")];
    assert!(!m.applies_to(&one));
    assert!(m.applies_to(&two), "the least allowed is allowed");
    assert!(m.applies_to(&three), "the most allowed is allowed");
    assert!(!m.applies_to(&four));
    m.counts = Counts {
        min: None,
        max: None,
        exactly: vec![1, 4],
    };
    assert!(m.applies_to(&one) && !m.applies_to(&two) && m.applies_to(&four));
    m.counts = Counts::default();
    m.requires_write = true;
    let mut locked = file("a/b");
    locked.writable = false;
    assert!(m.applies_to(&one));
    assert!(!m.applies_to(&[file("a/c"), locked]));
}

/// **An item's command is its `Exec`, the targets put in**: `%f` once per
/// file, `%F` all at once -- `desktopentry`'s reading of the
/// specification.
#[test]
fn an_items_command_takes_its_targets() {
    let f = Fixture::new("exec");
    f.install(Origin::System, "image.desktop", FULL);
    let found = scan(&f.dirs(), None);
    let menu = &found.menus[0];
    let targets = [file("image/png"), file("image/jpeg")];
    let rotate = menu.actions[0].command_lines(&targets, menu);
    assert_eq!(rotate.len(), 2, "%f: one line per file");
    assert_eq!(
        rotate[0],
        [
            OsString::from("mogrify"),
            OsString::from("-rotate"),
            OsString::from("90"),
            targets[0].path.clone().into_os_string(),
        ]
    );
    let flip = menu.actions[1].command_lines(&targets, menu);
    assert_eq!(flip.len(), 1, "%F: one line for all");
    assert_eq!(flip[0].len(), 4);
}

/// **A system menu is on until turned off; a user's own is off until
/// turned on** -- and what is offered is what is on and applies.
#[test]
fn choices_decide_what_is_offered() {
    let f = Fixture::new("choices");
    f.install(Origin::System, "sys.desktop", &simple("all/all;", "sys"));
    f.install(Origin::User, "mine.desktop", &simple("all/all;", "mine"));
    f.install(
        Origin::System,
        "pictures.desktop",
        &simple("image/*;", "pic"),
    );
    let found = scan(&f.dirs(), None);
    let names = |offered: Vec<&ServiceMenu>| -> Vec<OsString> {
        offered.iter().map(|m| m.id.clone()).collect()
    };
    let mut choices = Choices::default();
    let targets = [file("text/plain")];
    assert_eq!(names(found.offered(&choices, &targets)), ["sys.desktop"]);
    let mine = found.menus.iter().find(|m| m.id == "mine.desktop").unwrap();
    let sys = found.menus.iter().find(|m| m.id == "sys.desktop").unwrap();
    choices.set(mine, true);
    choices.set(sys, false);
    assert_eq!(names(found.offered(&choices, &targets)), ["mine.desktop"]);
    choices.set(mine, false);
    choices.set(sys, true);
    assert_eq!(names(found.offered(&choices, &targets)), ["sys.desktop"]);
    assert_eq!(
        names(found.offered(&choices, &[file("image/png")])),
        ["pictures.desktop", "sys.desktop"]
    );
}

/// **The choices survive a save**, whatever bytes a name holds, and a file
/// with neither key is the defaults.
#[test]
fn the_choices_survive_a_save() {
    let menu = |id: &str, origin| ServiceMenu {
        id: OsString::from(id),
        origin,
        path: PathBuf::from(id),
        mime_types: vec![String::from("all/all")],
        submenu: None,
        top_level: false,
        counts: Counts::default(),
        requires_write: false,
        actions: Vec::new(),
        unusable: Vec::new(),
    };
    let mut file = ChoicesFile::from_document(Document::parse("# my menus\n"));
    assert_eq!(file.choices, Choices::default());
    let odd = menu("été 100%41.desktop", Origin::User);
    let padded = menu(" padded .desktop", Origin::User);
    let off = menu("compress.desktop", Origin::System);
    file.choices.set(&odd, true);
    file.choices.set(&padded, true);
    file.choices.set(&off, false);
    let text = file.to_text();
    assert!(text.contains("# my menus"), "the comment was lost: {text}");
    // Written in the one escape for names in settings: a `%` is `%25`, so a
    // name that holds `%41` is not read back as `A`.
    assert!(text.contains("100%2541"), "{text}");
    let back = ChoicesFile::from_document(Document::parse(&text));
    assert!(back.choices.is_on(&odd));
    assert!(
        back.choices.is_on(&padded),
        "blanks are part of a name: {text}"
    );
    assert!(!back.choices.is_on(&off));
    assert_eq!(back.choices, file.choices);

    settingsfile::testing::with_scratch_config("servicemenus-choices", |_root| {
        let mut saved = ChoicesFile::load();
        saved.choices.set(&odd, true);
        saved.save().unwrap();
        assert!(ChoicesFile::load().choices.is_on(&odd));
    });
}

/// **A target is what the file system says it is**: a folder is
/// `inode/directory` whatever it was called, a file its given kind.
#[test]
fn a_target_is_what_the_file_system_says() {
    let scratch = ScratchDir::new("slateos-servicemenus-target");
    let dir = scratch.dir().join("folder");
    fs::create_dir_all(&dir).unwrap();
    let doc = scratch.dir().join("doc.txt");
    fs::write(&doc, b"x").unwrap();
    let t = Target::at(&dir, "text/plain");
    assert!(t.is_dir);
    assert_eq!(t.mime, "inode/directory");
    let t = Target::at(&doc, "text/plain");
    assert!(!t.is_dir);
    assert!(t.writable);
    assert_eq!(t.mime, "text/plain");
    assert_eq!(t.inherits, Vec::<String>::new());
    // A file the system will not let be written is not writable.
    let locked = scratch.dir().join("locked.txt");
    fs::write(&locked, b"x").unwrap();
    let mut permissions = fs::metadata(&locked).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&locked, permissions.clone()).unwrap();
    let t = Target::at(&locked, "text/plain");
    // Writable again, so the scratch directory can be removed.
    #[allow(clippy::permissions_set_readonly_false)]
    permissions.set_readonly(false);
    fs::set_permissions(&locked, permissions).unwrap();
    assert!(!t.writable);
    assert!(!t.is_dir);
    // A path that is not there is neither a folder nor writable.
    let t = Target::at(scratch.dir().join("absent"), "text/plain");
    assert!(!t.is_dir && !t.writable);
    assert_eq!(t.mime, "text/plain");
}

/// **KDE's lists are read with commas or semicolons**: `X-KDE-Require`,
/// `X-KDE-RequiredNumberOfUrls` and `X-KDE-Protocols` are comma lists in
/// KDE's own files and semicolon lists in some others.
#[test]
fn kdes_lists_take_commas_or_semicolons() {
    let f = Fixture::new("lists");
    f.install(
        Origin::System,
        "commas.desktop",
        &with_keys("X-KDE-Require=Write\nX-KDE-RequiredNumberOfUrls=1, 3"),
    );
    f.install(
        Origin::System,
        "semicolons.desktop",
        &with_keys("X-KDE-Require=Write;\nX-KDE-RequiredNumberOfUrls=2;x;4;"),
    );
    f.install(
        Origin::System,
        "unwritten.desktop",
        &with_keys("X-KDE-Require=Writable"),
    );
    let found = scan(&f.dirs(), None);
    assert_eq!(found.skipped, []);
    let menu = |id: &str| found.menus.iter().find(|m| m.id == id).unwrap();
    assert!(menu("commas.desktop").requires_write);
    assert_eq!(menu("commas.desktop").counts.exactly, [1, 3]);
    assert!(menu("semicolons.desktop").requires_write);
    assert_eq!(
        menu("semicolons.desktop").counts.exactly,
        [2, 4],
        "a count that is not a number is dropped, not the list"
    );
    assert!(!menu("unwritten.desktop").requires_write);
}

/// **`X-KDE-Protocol` names the one place a menu is for, or with `!` the one
/// it is not** -- and is read in preference to `X-KDE-Protocols`.
#[test]
fn one_protocol_is_for_or_against_this_machines_files() {
    let cases = [
        ("X-KDE-Protocol=file", true),
        ("X-KDE-Protocol= file ", true),
        ("X-KDE-Protocol=smb", false),
        ("X-KDE-Protocol=!sftp", true),
        ("X-KDE-Protocol=!file", false),
        ("X-KDE-Protocol=", true),
        ("X-KDE-Protocol=file\nX-KDE-Protocols=smb", true),
        ("X-KDE-Protocol=smb\nX-KDE-Protocols=file", false),
        ("X-KDE-Protocols=smb;file;", true),
        ("X-KDE-Protocols=smb, sftp", false),
    ];
    for (keys, offered) in cases {
        let f = Fixture::new("protocol");
        f.install(Origin::System, "p.desktop", &with_keys(keys));
        let found = scan(&f.dirs(), None);
        assert_eq!(found.menus.len(), usize::from(offered), "{keys:?}");
        if !offered {
            let key = keys.lines().next().unwrap().split('=').next().unwrap();
            assert!(
                found.skipped[0].why.contains(&format!("({key})")),
                "{keys:?}: {:?}",
                found.skipped
            );
        }
    }
}

/// **`_SEPARATOR_` puts a line between two items** -- one line however many
/// are written, none before the first item or after the last.
#[test]
fn a_separator_is_a_line_between_items() {
    let f = Fixture::new("separator");
    let item = |id: &str| format!("[Desktop Action {id}]\nName={id}\nExec={id}\n\n");
    f.install(
        Origin::System,
        "lines.desktop",
        &format!(
            "[Desktop Entry]\nType=Service\nMimeType=all/all;\n\
             Actions=_SEPARATOR_;a;_SEPARATOR_;_SEPARATOR_;b;c;_SEPARATOR_;\n\n{}{}{}",
            item("a"),
            item("b"),
            item("c")
        ),
    );
    let found = scan(&f.dirs(), None);
    let menu = &found.menus[0];
    assert_eq!(
        menu.unusable,
        Vec::<String>::new(),
        "a separator is not an item"
    );
    assert_eq!(
        menu.actions
            .iter()
            .map(|a| (a.id.as_str(), a.separator_before))
            .collect::<Vec<_>>(),
        [("a", false), ("b", true), ("c", false)]
    );
}

/// **A kind of file is also what it inherits**: every `text/*` is
/// `text/plain`, every file (not a folder) is `application/octet-stream`,
/// and whatever else the caller says it is.
#[test]
fn a_kind_is_also_what_it_inherits() {
    let menu = |mime: &str| ServiceMenu {
        id: OsString::from("m.desktop"),
        origin: Origin::System,
        path: PathBuf::from("m.desktop"),
        mime_types: vec![mime.to_owned()],
        submenu: None,
        top_level: false,
        counts: Counts::default(),
        requires_write: false,
        actions: Vec::new(),
        unusable: Vec::new(),
    };
    let python = file("text/x-python");
    let png = file("image/png");
    let mut script = file("application/x-shellscript");
    assert!(menu("text/plain").applies_to(std::slice::from_ref(&python)));
    assert!(menu("TEXT/PLAIN").applies_to(std::slice::from_ref(&python)));
    assert!(!menu("text/plain").applies_to(std::slice::from_ref(&png)));
    assert!(!menu("text/plain").applies_to(std::slice::from_ref(&script)));
    assert!(menu("application/octet-stream").applies_to(&[png.clone(), python]));
    assert!(!menu("application/octet-stream").applies_to(&[folder()]));
    let mut device = file("inode/blockdevice");
    device.is_dir = false;
    assert!(!menu("application/octet-stream").applies_to(&[device]));
    script.inherits = vec![String::from("text/plain")];
    assert!(menu("text/plain").applies_to(std::slice::from_ref(&script)));
    assert!(menu("text/*").applies_to(std::slice::from_ref(&script)));
    assert!(!menu("image/*").applies_to(&[script]));
    assert!(!menu("image/jpeg").applies_to(&[png]));
}

/// **A `type/*` is matched without regard to case**, on either side.
#[test]
fn a_major_type_is_matched_in_any_case() {
    let menu = |mime: &str| ServiceMenu {
        id: OsString::from("m.desktop"),
        origin: Origin::System,
        path: PathBuf::from("m.desktop"),
        mime_types: vec![mime.to_owned()],
        submenu: None,
        top_level: false,
        counts: Counts::default(),
        requires_write: false,
        actions: Vec::new(),
        unusable: Vec::new(),
    };
    assert!(menu("IMAGE/*").applies_to(&[file("image/png")]));
    assert!(menu("image/*").applies_to(&[file("Image/PNG")]));
    assert!(!menu("image/*").applies_to(&[file("imagex/png")]));
}

/// **Menus are read in the order of their file names' bytes** -- the same on
/// every file system, whatever order it lists a directory in.
#[test]
fn menus_are_read_in_the_order_of_their_names() {
    let f = Fixture::new("order");
    for name in ["b.desktop", "a.desktop", "C.desktop"] {
        f.install(Origin::System, name, &simple("all/all;", "x"));
    }
    let found = scan(&f.dirs(), None);
    let ids: Vec<&OsStr> = found.menus.iter().map(|m| m.id.as_os_str()).collect();
    assert_eq!(ids, ["C.desktop", "a.desktop", "b.desktop"]);
}
