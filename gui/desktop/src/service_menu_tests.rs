//! Tests for what programs add to a file's right-click menu
//! (design-decisions §1448), as a user meets it: a right-click on an icon
//! offers the service menus on for what is selected, after Open; choosing an
//! item starts its command on those files, in their folder.
//!
//! `servicemenus`' own tests hold the reading of the files and the running of
//! a command; these hold the shell's part -- which icons are files, where the
//! items go, and what a choice becomes.

#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_precision_loss
)]

use crate::{DesktopShell, ShellAction, icons};
use guitk::menu::MenuItem;
use scratchdir::ScratchDir;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// A menu for PNG pictures: one item, one file at a time.
const ROTATE: &str = "[Desktop Entry]\nType=Service\nMimeType=image/png;\nActions=rotate;\n\n\
                      [Desktop Action rotate]\nName=Rotate right\nExec=mogrify -rotate 90 %f\n";

/// A menu for anything, with one item named `name`.
fn for_anything(name: &str) -> String {
    format!(
        "[Desktop Entry]\nType=Service\nMimeType=all/all;\nActions=go;\n\n\
         [Desktop Action go]\nName={name}\nExec=tool %F\n"
    )
}

/// A shell, a scratch data directory for its menus, and a folder of files.
struct Setup {
    scratch: ScratchDir,
    shell: DesktopShell,
}

impl Setup {
    fn new(tag: &str) -> Self {
        Self {
            scratch: ScratchDir::new(&format!("slateos-desktop-servicemenus-{tag}")),
            shell: DesktopShell::new(1920, 1080),
        }
    }

    fn dirs(&self) -> servicemenus::Dirs {
        servicemenus::Dirs {
            user: Some(self.scratch.dir().join("user")),
            system: vec![self.scratch.dir().join("system")],
        }
    }

    /// Install a menu -- the user's own, or the system's -- and give the
    /// shell the menus as they now are, as the session does.
    fn install(&mut self, user: bool, name: &str, text: &str) {
        let base = if user { "user" } else { "system" };
        let dir = self.scratch.dir().join(base).join("kio/servicemenus");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(name), text).unwrap();
        let scan = servicemenus::scan(&self.dirs(), None);
        self.shell.set_service_menus(scan);
    }

    /// A file on disk, and an icon for it in the first column's `row`.
    fn file_icon(&mut self, name: &str, row: i32) -> (icons::IconId, PathBuf) {
        let files = self.scratch.dir().join("files");
        std::fs::create_dir_all(&files).unwrap();
        let path = files.join(name);
        std::fs::write(&path, b"x").unwrap();
        let (x, y) = self.shell.icons.cell_origin(0, row);
        let id = self.shell.icons.add_icon(
            name,
            icons::IconType::File,
            icons::IconAction::OpenPath(path.clone()),
            x,
            y,
        );
        (id, path)
    }

    /// Right-click `icon`, answering the rows of the menu that opens.
    fn right_click(&mut self, icon: icons::IconId) -> Vec<MenuItem> {
        let item = self.shell.icons.get_icon(icon).unwrap();
        let grid = self.shell.icons.grid();
        let x = item.x as f32 + grid.cell_width() as f32 / 2.0;
        let y = item.y as f32 + grid.cell_height() as f32 / 2.0;
        self.shell.open_desktop_menu(x, y);
        self.shell.desktop_menu.items().to_vec()
    }
}

/// The id of the item labelled `label`, looking into submenus.
fn id_of(items: &[MenuItem], label: &str) -> Option<u64> {
    items.iter().find_map(|item| match item {
        MenuItem::Action { id, label: l, .. } if l == label => Some(*id),
        MenuItem::Submenu { children, .. } => id_of(children, label),
        _ => None,
    })
}

/// The rows' labels, a line as `-`.
fn labels(items: &[MenuItem]) -> Vec<String> {
    items
        .iter()
        .map(|item| match item {
            MenuItem::Action { label, .. } | MenuItem::Submenu { label, .. } => label.clone(),
            MenuItem::Separator => String::from("-"),
        })
        .collect()
}

/// **A right-click on a file offers what its menus add, after Open**, and
/// choosing the item starts its command on the file, in the file's folder.
#[test]
fn a_files_menu_offers_and_runs_an_installed_item() {
    let mut s = Setup::new("offer");
    s.install(false, "rotate.desktop", ROTATE);
    let (icon, path) = s.file_icon("photo.png", 0);
    let items = s.right_click(icon);
    assert_eq!(
        labels(&items)[..5],
        ["Open", "Open with", "-", "Rotate right", "-"],
        "{:?}",
        labels(&items)
    );
    let id = id_of(&items, "Rotate right").unwrap();
    match s.shell.activate_desktop_menu_item(id) {
        ShellAction::Launch(launch) => {
            assert_eq!(launch.program, Path::new("mogrify"));
            assert_eq!(
                launch.args,
                [
                    OsString::from("-rotate"),
                    OsString::from("90"),
                    path.clone().into_os_string()
                ]
            );
            assert_eq!(
                launch.dir,
                Some(std::fs::canonicalize(path.parent().unwrap()).unwrap()),
                "it starts in the file's folder"
            );
        }
        other => panic!("{other:?}"),
    }
    // The menu's business is done: its icon is no longer the one an Open
    // would act on.
    assert_eq!(
        s.shell
            .activate_desktop_menu_item(DesktopShell::MENU_ICON_OPEN),
        ShellAction::Pass
    );
}

/// **A kind of text is offered what plain text is**: a menu for
/// `text/plain` is on a JSON file's menu, whose type is not `text/*`.
#[test]
fn a_kind_of_text_is_offered_what_plain_text_is() {
    let mut s = Setup::new("text");
    s.install(
        false,
        "edit.desktop",
        "[Desktop Entry]\nMimeType=text/plain;\nActions=edit;\n\n\
         [Desktop Action edit]\nName=Edit as text\nExec=editor %f\n",
    );
    let (json, _) = s.file_icon("data.json", 0);
    assert!(id_of(&s.right_click(json), "Edit as text").is_some());
    let (png, _) = s.file_icon("photo.png", 1);
    assert_eq!(id_of(&s.right_click(png), "Edit as text"), None);
}

/// **An item shows its picture**, as its menu names it, found in the icon
/// theme like every other picture the shell draws.
#[test]
fn an_item_shows_its_picture() {
    let mut s = Setup::new("picture");
    s.install(
        false,
        "rotate.desktop",
        "[Desktop Entry]\nMimeType=image/png;\nActions=rotate;\n\n\
         [Desktop Action rotate]\nName=Rotate right\nIcon=object-rotate-right\n\
         Exec=mogrify -rotate 90 %f\n",
    );
    let (icon, _) = s.file_icon("photo.png", 0);
    drop(s.right_click(icon));
    let tree = s.shell.render_desktop_menu().expect("the menu is open");
    let pictured: Vec<String> = tree
        .commands
        .iter()
        .filter_map(|c| match c {
            guitk::render::RenderCommand::Image { image_id, .. } => s
                .shell
                .icon_request(*image_id)
                .map(|request| request.name.into_owned()),
            _ => None,
        })
        .collect();
    assert_eq!(pictured, ["object-rotate-right"]);

    // A submenu's row shows its picture too: its first item's.
    let mut s = Setup::new("submenu-picture");
    s.install(
        false,
        "tools.desktop",
        "[Desktop Entry]\nMimeType=all/all;\nX-KDE-Submenu=Tools\nActions=fix;\n\n\
         [Desktop Action fix]\nName=Fix\nIcon=wrench\nExec=fix %f\n",
    );
    let (icon, _) = s.file_icon("notes.txt", 0);
    drop(s.right_click(icon));
    let tree = s.shell.render_desktop_menu().expect("the menu is open");
    let pictured: Vec<String> = tree
        .commands
        .iter()
        .filter_map(|c| match c {
            guitk::render::RenderCommand::Image { image_id, .. } => s
                .shell
                .icon_request(*image_id)
                .map(|request| request.name.into_owned()),
            _ => None,
        })
        .collect();
    assert_eq!(pictured, ["wrench"]);
}

/// **Two files, a command that takes one at a time: a program each.**
#[test]
fn two_files_start_one_program_each() {
    let mut s = Setup::new("two");
    s.install(false, "rotate.desktop", ROTATE);
    let (a, first) = s.file_icon("a.png", 0);
    let (b, second) = s.file_icon("b.png", 1);
    s.shell.icons.select_single(a);
    s.shell.icons.toggle_selection(b);
    let items = s.right_click(a);
    match s
        .shell
        .activate_desktop_menu_item(id_of(&items, "Rotate right").unwrap())
    {
        ShellAction::LaunchAll(launches) => {
            let files: Vec<OsString> = launches
                .iter()
                .map(|launch| launch.args.last().unwrap().clone())
                .collect();
            assert_eq!(files.len(), 2);
            assert!(files.contains(&first.into_os_string()));
            assert!(files.contains(&second.into_os_string()));
        }
        other => panic!("{other:?}"),
    }
}

/// **Only what a menu is for is offered**: a picture's item is not on a text
/// file's menu, nor on This PC's -- which is not a file at all.
#[test]
fn only_what_a_menu_is_for_is_offered() {
    let mut s = Setup::new("kinds");
    s.install(false, "rotate.desktop", ROTATE);
    let (text, _) = s.file_icon("notes.txt", 0);
    let items = s.right_click(text);
    assert_eq!(id_of(&items, "Rotate right"), None);
    // Nothing offered leaves no stray line behind.
    let rows = labels(&items);
    assert!(
        !rows.windows(2).any(|pair| pair[0] == "-" && pair[1] == "-"),
        "{rows:?}"
    );
    s.install(false, "any.desktop", &for_anything("Anything"));
    let (x, y) = s.shell.icons.cell_origin(0, 2);
    let pc = s.shell.icons.add_icon(
        "This PC",
        icons::IconType::Computer,
        icons::IconAction::LaunchSystem(icons::THIS_PC.to_string()),
        x,
        y,
    );
    assert_eq!(id_of(&s.right_click(pc), "Anything"), None);
    // The text file is offered what is for anything.
    assert!(id_of(&s.right_click(text), "Anything").is_some());
}

/// **A menu the user put in their own folder waits to be turned on**; once
/// `context-menus.yaml` says so, it is offered.
#[test]
fn a_users_own_menu_waits_to_be_turned_on() {
    settingsfile::testing::with_scratch_config("desktop-servicemenus-choices", |_root| {
        let mut s = Setup::new("choices");
        s.install(true, "mine.desktop", &for_anything("Mine"));
        let (icon, _) = s.file_icon("p.png", 0);
        let _ = s.shell.poll_service_choices();
        assert_eq!(id_of(&s.right_click(icon), "Mine"), None);
        let found = servicemenus::scan(&s.dirs(), None);
        let mine = found.menus.iter().find(|m| m.id == "mine.desktop").unwrap();
        let mut file = servicemenus::ChoicesFile::load();
        file.choices.set(mine, true);
        file.save().unwrap();
        assert!(s.shell.poll_service_choices(), "the choice was not read");
        assert!(!s.shell.poll_service_choices(), "read twice");
        // A comment added changes the file and not the choices.
        let text =
            std::fs::read_to_string(settingsfile::path_for(servicemenus::CONFIG_NAME).unwrap())
                .unwrap();
        std::fs::write(
            settingsfile::path_for(servicemenus::CONFIG_NAME).unwrap(),
            format!("# my menus\n{text}"),
        )
        .unwrap();
        assert!(
            !s.shell.poll_service_choices(),
            "a comment is not a change of choice"
        );
        assert!(id_of(&s.right_click(icon), "Mine").is_some());
    });
}

/// **A menu gone between the opening and the choice runs nothing.**
#[test]
fn a_menu_gone_since_the_opening_runs_nothing() {
    let mut s = Setup::new("gone");
    s.install(false, "rotate.desktop", ROTATE);
    let (icon, _) = s.file_icon("p.png", 0);
    let items = s.right_click(icon);
    s.shell.set_service_menus(servicemenus::Scan::default());
    assert_eq!(
        s.shell
            .activate_desktop_menu_item(id_of(&items, "Rotate right").unwrap()),
        ShellAction::Pass
    );
}

/// **An item of a menu no longer open runs nothing**: opening another --
/// here This PC's, which offers no service menus -- lets go of the last
/// one's items.
#[test]
fn a_closed_menus_item_runs_nothing() {
    let mut s = Setup::new("stale");
    s.install(false, "rotate.desktop", ROTATE);
    let (icon, _) = s.file_icon("p.png", 0);
    let id = id_of(&s.right_click(icon), "Rotate right").unwrap();
    let (x, y) = s.shell.icons.cell_origin(0, 2);
    let pc = s.shell.icons.add_icon(
        "This PC",
        icons::IconType::Computer,
        icons::IconAction::LaunchSystem(icons::THIS_PC.to_string()),
        x,
        y,
    );
    drop(s.right_click(pc));
    assert_eq!(s.shell.activate_desktop_menu_item(id), ShellAction::Pass);
}

/// **Many items go under Actions, as KDE's file manager puts them.**
#[test]
fn many_items_go_under_actions() {
    let mut s = Setup::new("many");
    for n in 0..5 {
        s.install(
            false,
            &format!("m{n}.desktop"),
            &for_anything(&format!("Item {n}")),
        );
    }
    let (icon, path) = s.file_icon("p.png", 0);
    let items = s.right_click(icon);
    let actions = items
        .iter()
        .find_map(|item| match item {
            MenuItem::Submenu {
                label, children, ..
            } if label == servicemenus::ACTIONS_LABEL => Some(children),
            _ => None,
        })
        .expect("an Actions submenu");
    assert_eq!(actions.len(), 5);
    // An item inside it runs like any other.
    match s
        .shell
        .activate_desktop_menu_item(id_of(&items, "Item 3").unwrap())
    {
        ShellAction::Launch(launch) => {
            assert_eq!(launch.program, Path::new("tool"));
            assert_eq!(launch.args, [path.into_os_string()]);
        }
        other => panic!("{other:?}"),
    }
}
