//! `INVENTORY.md`, held to the code: every program, keyword, category, role
//! and default the fourteen old lists held is where the inventory says it now
//! lives. A row of the inventory that stops being true fails here, rather than
//! being found missing after the old lists are deleted.
//!
//! Each test names the inventory section it checks.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use desktopentry::{App, DesktopEntry};
use programs::{BUILT_IN, Role, built_in, default_for, defaults};

/// The workspace root, from this crate's manifest directory.
fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the workspace root")
}

fn program(id: &str) -> App {
    built_in(None)
        .into_iter()
        .find(|app| app.id == id)
        .unwrap_or_else(|| panic!("{id} is not a built-in program"))
}

/// Every binary a crate under `apps/` builds: its package name, and any
/// `[[bin]]` name.
fn app_binaries() -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for dir in std::fs::read_dir(workspace().join("apps")).expect("apps/") {
        let manifest = dir.expect("an entry").path().join("Cargo.toml");
        let Ok(text) = std::fs::read_to_string(&manifest) else {
            continue;
        };
        let mut section = String::new();
        for line in text.lines().map(str::trim) {
            if line.starts_with('[') {
                section = line.to_owned();
                continue;
            }
            let wanted = section == "[package]" || section == "[[bin]]";
            if let Some(value) = line.strip_prefix("name").map(str::trim_start)
                && wanted
                && let Some(value) = value.strip_prefix('=')
            {
                names.insert(value.trim().trim_matches('"').to_owned());
            }
        }
    }
    names
}

// ---- the entries themselves ----

/// Every file in `applications/` is built in, every built-in entry parses and
/// is a valid application entry, and its id is its file name -- the
/// reader drops one that fails, so a failure must show here instead.
#[test]
fn every_built_in_entry_is_listed_parses_and_is_named_by_its_file() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("applications");
    let mut files: Vec<String> = std::fs::read_dir(&dir)
        .expect("applications/")
        .map(|e| {
            e.expect("an entry")
                .file_name()
                .into_string()
                .expect("utf-8")
        })
        .filter(|name| name.ends_with(".desktop"))
        .collect();
    files.sort();
    let listed: Vec<String> = BUILT_IN.iter().map(|(id, _)| (*id).to_owned()).collect();
    assert_eq!(
        files, listed,
        "applications/ and BUILT_IN list different entries"
    );

    for (id, text) in BUILT_IN {
        let on_disk = std::fs::read_to_string(dir.join(id)).expect("the file");
        assert_eq!(
            on_disk.replace("\r\n", "\n"),
            text.replace("\r\n", "\n"),
            "{id}"
        );
        let entry = DesktopEntry::parse(text.as_bytes())
            .unwrap_or_else(|e| panic!("{id} does not parse: {e:?}"));
        let app = App::from_entry(&entry, id, None)
            .unwrap_or_else(|e| panic!("{id} is not a valid entry: {e:?}"));
        assert_eq!(app.id, *id);
    }
    assert_eq!(built_in(None).len(), BUILT_IN.len(), "an entry was dropped");
}

// ---- section 1: programs ----

/// The fifteen programs, each at the path of a binary the tree builds. The
/// old lists named nine paths no crate builds; this is what keeps a tenth from
/// arriving.
#[test]
fn section_1_every_program_is_a_binary_the_tree_builds() {
    let expected = [
        ("org.slateos.Explorer.desktop", "/usr/bin/explorer"),
        ("org.slateos.Terminal.desktop", "/usr/bin/terminal"),
        ("org.slateos.Editor.desktop", "/usr/bin/editor"),
        ("org.slateos.Settings.desktop", "/usr/bin/settings"),
        ("org.slateos.Calculator.desktop", "/usr/bin/calculator"),
        // Not `/usr/bin/sysinfo`, which the shell's old table named: that is
        // `userspace/sysinfo`, the command-line tool. The window is
        // `apps/sysinfo`, whose package -- and so its binary -- is
        // `sysinfo-app`.
        (
            "org.slateos.SystemInformation.desktop",
            "/usr/bin/sysinfo-app",
        ),
        (
            "org.slateos.ProcessExplorer.desktop",
            "/usr/bin/procexplorer",
        ),
        ("org.slateos.ImageViewer.desktop", "/usr/bin/imageviewer"),
        ("org.slateos.MusicPlayer.desktop", "/usr/bin/musicplayer"),
        ("org.slateos.VideoPlayer.desktop", "/usr/bin/videoplayer"),
        ("org.slateos.Screenshot.desktop", "/usr/bin/screenshot"),
        ("org.slateos.PdfViewer.desktop", "/usr/bin/pdfviewer"),
        (
            "org.slateos.ArchiveManager.desktop",
            "/usr/bin/archivemanager",
        ),
        ("org.slateos.HexEditor.desktop", "/usr/bin/hexeditor"),
        ("org.slateos.Calendar.desktop", "/usr/bin/calendar"),
    ];
    let binaries = app_binaries();
    for (id, path) in expected {
        let app = program(id);
        let exec = app
            .exec
            .as_ref()
            .unwrap_or_else(|| panic!("{id} has no Exec"));
        assert_eq!(exec.program(), path, "{id}");
        let name = Path::new(path).file_name().unwrap().to_str().unwrap();
        assert!(
            binaries.contains(name),
            "{id} starts {path}, and no crate under apps/ builds `{name}`"
        );
    }
    assert_eq!(
        built_in(None).len(),
        expected.len(),
        "a program is not in the inventory"
    );
}

// ---- section 2: what each program is found by ----

/// Every keyword any old list gave a program is one of its keywords now, and
/// its categories put it in the start-menu folder the shell's old table did.
#[test]
fn section_2_every_old_keyword_and_folder_survives() {
    let rows: [(&str, &[&str], &str, &str); 15] = [
        (
            "org.slateos.Explorer.desktop",
            &[
                "explorer",
                "finder",
                "nautilus",
                "files",
                "browse",
                "folder",
                "directory",
            ],
            "Utility",
            "system-file-manager",
        ),
        (
            "org.slateos.Terminal.desktop",
            &["shell", "console", "command", "bash", "cli"],
            "System",
            "utilities-terminal",
        ),
        (
            "org.slateos.Editor.desktop",
            &["notepad", "edit", "vim", "nano", "code", "write", "text"],
            "Utility",
            "accessories-text-editor",
        ),
        (
            "org.slateos.Settings.desktop",
            &[
                "preferences",
                "config",
                "control",
                "options",
                "display",
                "monitor",
                "resolution",
                "dpi",
                "network",
                "wifi",
                "ethernet",
                "vpn",
                "internet",
                "sound",
                "audio",
                "volume",
                "speaker",
                "microphone",
            ],
            "Settings",
            "preferences-system",
        ),
        (
            "org.slateos.Calculator.desktop",
            &["calc", "math", "compute"],
            "Utility",
            "accessories-calculator",
        ),
        (
            "org.slateos.SystemInformation.desktop",
            &["about", "hardware", "specs", "info"],
            "System",
            "computer",
        ),
        (
            "org.slateos.ProcessExplorer.desktop",
            &["task", "manager", "top", "htop", "processes", "kill"],
            "System",
            "utilities-system-monitor",
        ),
        (
            "org.slateos.ImageViewer.desktop",
            &["photo", "picture", "gallery", "png", "jpg"],
            "Graphics",
            "image-x-generic",
        ),
        (
            "org.slateos.MusicPlayer.desktop",
            &["music", "vlc", "mpv", "audio", "song", "mp3", "media"],
            "AudioVideo",
            "audio-x-generic",
        ),
        (
            "org.slateos.VideoPlayer.desktop",
            &["video", "vlc", "mpv", "media"],
            "AudioVideo",
            "video-x-generic",
        ),
        (
            "org.slateos.Screenshot.desktop",
            &["capture", "snip", "screen", "grab"],
            "Utility",
            "applets-screenshooter",
        ),
        (
            "org.slateos.PdfViewer.desktop",
            &["document", "reader", "pdf"],
            "Office",
            "x-office-document",
        ),
        (
            "org.slateos.ArchiveManager.desktop",
            &["zip", "tar", "compress", "extract"],
            "Utility",
            "package-x-generic",
        ),
        (
            "org.slateos.HexEditor.desktop",
            &["hex", "binary", "bytes"],
            "Development",
            "accessories-text-editor",
        ),
        (
            "org.slateos.Calendar.desktop",
            &["events", "reminders", "schedule"],
            "Office",
            "x-office-calendar",
        ),
    ];
    for (id, keywords, first_category, icon) in rows {
        let app = program(id);
        for word in keywords {
            assert!(
                app.keywords.iter().any(|k| k == word),
                "{id} lost the keyword {word:?}"
            );
        }
        // The first main category is the start-menu folder
        // (`desktopentry::menu::Folder::of`), which is why its position, not
        // just its presence, is checked.
        assert_eq!(
            app.categories.first().map(String::as_str),
            Some(first_category),
            "{id} is in another start-menu folder than the old table put it"
        );
        assert_eq!(app.icon.as_deref(), Some(icon), "{id}");
    }
}

// ---- section 3: roles ----

#[test]
fn section_3_every_role_and_who_fills_it() {
    let programs = built_in(None);
    let expected = [
        (Role::WebBrowser, None),
        (Role::Email, None),
        (Role::FileManager, Some("org.slateos.Explorer.desktop")),
        (Role::TextEditor, Some("org.slateos.Editor.desktop")),
        (Role::Terminal, Some("org.slateos.Terminal.desktop")),
        (Role::ImageViewer, Some("org.slateos.ImageViewer.desktop")),
        (Role::VideoPlayer, Some("org.slateos.VideoPlayer.desktop")),
        (Role::MusicPlayer, Some("org.slateos.MusicPlayer.desktop")),
        (Role::DocumentReader, Some("org.slateos.PdfViewer.desktop")),
        (
            Role::ArchiveManager,
            Some("org.slateos.ArchiveManager.desktop"),
        ),
        (Role::Calculator, Some("org.slateos.Calculator.desktop")),
        (Role::Calendar, Some("org.slateos.Calendar.desktop")),
        (Role::Maps, None),
        (
            Role::SystemMonitor,
            Some("org.slateos.ProcessExplorer.desktop"),
        ),
    ];
    assert_eq!(
        expected.len(),
        Role::ALL.len(),
        "a role is not in the inventory"
    );
    for (role, program) in expected {
        assert!(Role::ALL.contains(&role));
        assert_eq!(
            role.filled_by(&programs).map(|app| app.id.as_str()),
            program,
            "{role:?}"
        );
    }
}

// ---- section 4: what opens what ----

/// Every type the old lists gave a default, and the program it opens with now.
#[test]
fn section_4_every_type_the_old_lists_defaulted_opens_with_its_program() {
    let editor = [
        "text/plain",
        "text/x-rust",
        "text/x-c",
        "text/x-c++",
        "text/x-python",
        "text/javascript",
        "text/typescript",
        "text/css",
        "text/markdown",
        "application/x-shellscript",
        "text/csv",
        "application/json",
        "application/toml",
        "application/x-yaml",
        "application/xml",
        "application/sql",
        "text/html",
        "application/rtf",
    ];
    let images = [
        "image/png",
        "image/jpeg",
        "image/gif",
        "image/bmp",
        "image/webp",
        "image/tiff",
        "image/svg+xml",
        "image/x-icon",
    ];
    let audio = [
        "audio/mpeg",
        "audio/wav",
        "audio/ogg",
        "audio/flac",
        "audio/mp4",
        "audio/aac",
        "audio/opus",
    ];
    let video = [
        "video/mp4",
        "video/x-matroska",
        "video/webm",
        "video/x-msvideo",
        "video/quicktime",
        "video/x-ms-wmv",
        "video/x-flv",
    ];
    let archives = [
        "application/zip",
        "application/gzip",
        "application/x-bzip2",
        "application/x-xz",
        "application/zstd",
        "application/x-7z-compressed",
        "application/vnd.rar",
        "application/x-tar",
    ];
    let groups: [(&[&str], &str); 7] = [
        (&editor, "org.slateos.Editor.desktop"),
        (&images, "org.slateos.ImageViewer.desktop"),
        (&audio, "org.slateos.MusicPlayer.desktop"),
        (&video, "org.slateos.VideoPlayer.desktop"),
        (&archives, "org.slateos.ArchiveManager.desktop"),
        (&["application/pdf"], "org.slateos.PdfViewer.desktop"),
        (&["inode/directory"], "org.slateos.Explorer.desktop"),
    ];
    let mut count = 0;
    for (types, id) in groups {
        for mime in types {
            assert_eq!(default_for(mime), Some(id), "{mime}");
            // And the program it names says it opens the type: a default the
            // program does not claim would start it without the file.
            assert!(
                program(id).mime_types.iter().any(|m| m == mime),
                "{id} is the default for {mime} but does not list it"
            );
            count += 1;
        }
    }
    assert_eq!(
        defaults().count(),
        count,
        "a default is not in the inventory"
    );
}

/// The types section 4 names with no default, and why each has none.
#[test]
fn section_4_the_types_left_without_a_default_have_none() {
    for mime in [
        // Every unrecognised file's type: a default would open all of them in
        // the hex editor. The hex editor is offered for it instead.
        "application/octet-stream",
        // The file manager opens the folder holding an image, not the image.
        "application/x-iso9660-image",
        // Neither the PDF viewer nor the plain-text e-book reader reads EPUB.
        "application/epub+zip",
        // The calendar does not open a file named on its command line.
        "text/calendar",
        // Office documents: the text editor would show their bytes.
        "application/msword",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "application/vnd.oasis.opendocument.text",
        // No browser, no mail program: the roles stay, the defaults do not.
        "x-scheme-handler/http",
        "x-scheme-handler/mailto",
    ] {
        assert_eq!(default_for(mime), None, "{mime}");
    }
    assert!(
        program("org.slateos.HexEditor.desktop")
            .mime_types
            .iter()
            .any(|m| m == "application/octet-stream"),
        "the hex editor is to be offered for any file"
    );
}

// ---- section 5: file types ----

/// The extensions the kernel knew and the toolkit did not are in the toolkit's
/// table now, and the nine that wait on a decision are still out of it. The
/// toolkit's own tests check each row's type; this checks the inventory's
/// promise that they survived.
#[test]
fn section_5_the_kernels_extensions_reached_the_toolkit() {
    use guitk::filetypes::{FileCategory, category_from_extension};
    for ext in [
        "a", "bat", "cc", "cmd", "cpio", "cxx", "diff", "epub", "gzip", "htm", "hxx", "jar", "lib",
        "markdown", "mjs", "o", "oga", "patch", "psm1", "pyw", "text", "xsd", "xsl", "zstd",
    ] {
        assert_ne!(
            category_from_extension(ext),
            FileCategory::Unknown,
            ".{ext} was carried into the toolkit and is not there"
        );
    }
    for ext in [
        "exe", "dll", "class", "wasm", "deb", "rpm", "db", "sqlite", "sqlite3",
    ] {
        assert_eq!(
            category_from_extension(ext),
            FileCategory::Unknown,
            ".{ext} waits on a decision"
        );
    }
}

/// The kinds of file the kernel recognised by content, and the toolkit now
/// does.
#[test]
fn section_5_the_kernels_signatures_reached_the_toolkit() {
    use guitk::filetypes::detect_from_magic;
    let mut tar = vec![0u8; 512];
    tar[257..262].copy_from_slice(b"ustar");
    let mut avi = b"RIFF\x24\x00\x00\x00AVI ".to_vec();
    avi.extend_from_slice(&[0; 4]);
    let cases: [(&[u8], &str); 9] = [
        (b"\x7fELF\x02\x01\x01\x00", ".elf"),
        (b"II*\x00\x08\x00\x00\x00", ".tiff"),
        (b"\x28\xb5\x2f\xfd\x00\x00", ".zst"),
        (b"\x04\x22\x4d\x18\x00\x00", ".lz4"),
        (&tar, ".tar"),
        (b"070701000000", ".cpio"),
        (b"!<arch>\nfile.o", ".a"),
        (b"MThd\x00\x00\x00\x06", ".mid"),
        (&avi, ".avi"),
    ];
    for (header, want) in cases {
        assert_eq!(
            detect_from_magic(header).map(|info| info.extension),
            Some(want),
            "{want}"
        );
    }
}
