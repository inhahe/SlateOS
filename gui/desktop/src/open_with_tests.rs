//! Tests for opening a file on the desktop with a program: Open's fallback to
//! the kind's built-in default, and the "Open with" submenu of a file's
//! right-click menu -- every program that opens it, the one Open would start
//! first.
//!
//! `launcher`'s tests hold what a program opens and how it is started on a
//! file; these hold the shell's part: which programs are offered, in what
//! order, and what a choice becomes.

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

/// A shell with SlateOS's own programs, and a folder of files.
struct Setup {
    scratch: ScratchDir,
    shell: DesktopShell,
}

impl Setup {
    fn new(tag: &str) -> Self {
        Self {
            scratch: ScratchDir::new(&format!("slateos-desktop-open-with-{tag}")),
            shell: DesktopShell::new(1920, 1080),
        }
    }

    /// A file on disk -- or a folder, for a name with no dot -- and an icon
    /// for it in the first column's `row`.
    fn icon(&mut self, name: &str, row: i32) -> (icons::IconId, PathBuf) {
        let path = self.scratch.dir().join(name);
        if name.contains('.') {
            std::fs::write(&path, b"x").unwrap();
        } else {
            std::fs::create_dir_all(&path).unwrap();
        }
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

/// The "Open with" submenu's rows, as (label, icon).
fn open_with(items: &[MenuItem]) -> Option<Vec<(String, Option<String>)>> {
    items.iter().find_map(|item| match item {
        MenuItem::Submenu {
            label, children, ..
        } if label == "Open with" => Some(
            children
                .iter()
                .filter_map(|row| match row {
                    MenuItem::Action { label, icon, .. } => Some((label.clone(), icon.clone())),
                    _ => None,
                })
                .collect(),
        ),
        _ => None,
    })
}

fn names(rows: &[(String, Option<String>)]) -> Vec<&str> {
    rows.iter().map(|(label, _)| label.as_str()).collect()
}

/// The id of the "Open with" row labelled `label`.
fn row_id(items: &[MenuItem], label: &str) -> u64 {
    items
        .iter()
        .find_map(|item| match item {
            MenuItem::Submenu {
                label: l, children, ..
            } if l == "Open with" => children.iter().find_map(|row| match row {
                MenuItem::Action { id, label: r, .. } if r == label => Some(*id),
                _ => None,
            }),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no Open with row {label:?}"))
}

/// **A file nobody chose a program for opens in its kind's default** --
/// SlateOS's own, through its command line -- where it used to say nothing
/// was set to open it.
#[test]
fn a_file_nobody_chose_for_opens_in_its_default() {
    settingsfile::testing::with_scratch_config("open-with-default", |_root| {
        let mut s = Setup::new("default");
        let (_, picture) = s.icon("photo.png", 0);
        match s.shell.open_path(&picture, "photo.png") {
            ShellAction::Launch(launch) => {
                assert_eq!(launch.program, Path::new("/usr/bin/imageviewer"));
                assert_eq!(launch.args, [picture.clone().into_os_string()]);
            }
            other => panic!("{other:?}"),
        }
        // A kind of text with no default of its own opens as plain text
        // does -- a `text/*` one, and one whose type is not `text/*`.
        for name in ["main.go", "setup.ps1"] {
            let (_, source) = s.icon(name, 1);
            match s.shell.open_path(&source, name) {
                ShellAction::Launch(launch) => {
                    assert_eq!(launch.program, Path::new("/usr/bin/editor"), "{name}");
                }
                other => panic!("{name}: {other:?}"),
            }
        }
    });
}

/// **The user's choice still comes first.**
#[test]
fn the_users_choice_comes_before_the_default() {
    settingsfile::testing::with_scratch_config("open-with-choice", |_root| {
        settingsfile::store(
            associations::CONFIG_NAME,
            &yamldoc::Document::parse("associations:\n  png: /opt/paint/bin/paint\n"),
        )
        .unwrap();
        let mut s = Setup::new("choice");
        let (icon, picture) = s.icon("photo.png", 0);
        match s.shell.open_path(&picture, "photo.png") {
            ShellAction::Launch(launch) => {
                assert_eq!(launch.program, Path::new("/opt/paint/bin/paint"));
            }
            other => panic!("{other:?}"),
        }
        // Not on the list of programs, so not a row to put first: the list
        // is in its own order.
        let rows = open_with(&s.right_click(icon)).expect("an Open with");
        assert_eq!(names(&rows), ["Hex Editor", "Image Viewer"]);
    });
}

/// **"Open with" offers every program that opens the file, Open's first**,
/// each with its picture: a picture's viewer, a text's editor, a folder's
/// file manager -- and the hex editor for any file.
#[test]
fn open_with_offers_what_opens_the_file_opens_first_first() {
    settingsfile::testing::with_scratch_config("open-with-rows", |_root| {
        let mut s = Setup::new("rows");
        let (picture, _) = s.icon("photo.png", 0);
        let items = s.right_click(picture);
        let rows = open_with(&items).expect("an Open with");
        assert_eq!(names(&rows), ["Image Viewer", "Hex Editor"]);
        assert!(rows.iter().all(|(_, icon)| icon.is_some()), "{rows:?}");
        // After Open, before the icon's own group.
        assert!(matches!(&items[0], MenuItem::Action { label, .. } if label == "Open"));
        assert!(matches!(&items[1], MenuItem::Submenu { label, .. } if label == "Open with"));
        assert!(matches!(items[2], MenuItem::Separator));

        let (text, _) = s.icon("notes.txt", 1);
        let rows = open_with(&s.right_click(text)).expect("an Open with");
        assert_eq!(names(&rows), ["Text Editor", "Hex Editor"]);

        // A folder's only program is the one Open starts: no list.
        let (folder, _) = s.icon("Projects", 2);
        assert_eq!(open_with(&s.right_click(folder)), None);
        // A file nothing opens by default still gets the one program that
        // can -- Open would say nothing is set.
        let (unknown, _) = s.icon("blob.qqq", 3);
        let rows = open_with(&s.right_click(unknown)).expect("an Open with");
        assert_eq!(names(&rows), ["Hex Editor"]);
    });
}

/// **Several files are offered what opens them all.**
#[test]
fn several_files_are_offered_what_opens_them_all() {
    settingsfile::testing::with_scratch_config("open-with-several", |_root| {
        let mut s = Setup::new("several");
        let (picture, _) = s.icon("photo.png", 0);
        let (text, _) = s.icon("notes.txt", 1);
        s.shell.icons.select_single(picture);
        s.shell.icons.toggle_selection(text);
        let rows = open_with(&s.right_click(picture)).expect("an Open with");
        assert_eq!(names(&rows), ["Hex Editor"]);
        // A file and a folder: nothing opens both.
        let (folder, _) = s.icon("Projects", 2);
        s.shell.icons.select_single(picture);
        s.shell.icons.toggle_selection(folder);
        assert_eq!(open_with(&s.right_click(picture)), None);
    });
}

/// **Choosing a program opens the files with it**, as its command line
/// says: one program for all where it takes them all (`%F`), one each where
/// it takes one (`%f`).
#[test]
fn choosing_a_program_opens_the_files_with_it() {
    settingsfile::testing::with_scratch_config("open-with-choose", |_root| {
        let mut s = Setup::new("choose");
        let (a, first) = s.icon("a.png", 0);
        let (b, second) = s.icon("b.png", 1);
        s.shell.icons.select_single(a);
        s.shell.icons.toggle_selection(b);
        let items = s.right_click(a);
        match s
            .shell
            .activate_desktop_menu_item(row_id(&items, "Hex Editor"))
        {
            ShellAction::Launch(launch) => {
                assert_eq!(launch.program, Path::new("/usr/bin/hexeditor"));
                let mut args = launch.args;
                args.sort();
                let mut want = vec![
                    OsString::from(first.clone()),
                    OsString::from(second.clone()),
                ];
                want.sort();
                assert_eq!(args, want);
            }
            other => panic!("{other:?}"),
        }
        let items = s.right_click(a);
        match s
            .shell
            .activate_desktop_menu_item(row_id(&items, "Image Viewer"))
        {
            ShellAction::LaunchAll(launches) => {
                assert_eq!(launches.len(), 2);
                assert!(
                    launches
                        .iter()
                        .all(|l| l.program == Path::new("/usr/bin/imageviewer"))
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
    });
}

/// **A row of a menu no longer open, or of a program gone since, starts
/// nothing.**
#[test]
fn a_stale_row_starts_nothing() {
    settingsfile::testing::with_scratch_config("open-with-stale", |_root| {
        let mut s = Setup::new("stale");
        let (picture, _) = s.icon("photo.png", 0);
        // Chosen once, the menu's rows are spent.
        let items = s.right_click(picture);
        let viewer = row_id(&items, "Image Viewer");
        assert!(matches!(
            s.shell.activate_desktop_menu_item(viewer),
            ShellAction::Launch(_)
        ));
        assert_eq!(
            s.shell.activate_desktop_menu_item(viewer),
            ShellAction::Pass
        );
        // A program gone from the list since the menu opened.
        let items = s.right_click(picture);
        let viewer = row_id(&items, "Image Viewer");
        s.shell.set_programs(Vec::new());
        assert_eq!(
            s.shell.activate_desktop_menu_item(viewer),
            ShellAction::Pass
        );
    });
}
