//! `pkg`'s commands, run as a user runs them -- the built program, on a
//! scratch system named by `--root` -- so what each does with the functions
//! the unit tests cover is checked too: which it calls, what it prints, and
//! the status it ends with.

use std::path::Path;
use std::process::{Command, Output};

/// `pkg --root ROOT ARGS...`.
fn pkg(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_pkg"))
        .arg("--root")
        .arg(root)
        .args(args)
        .output()
        .expect("pkg runs")
}

fn stdout(o: &Output) -> String {
    String::from_utf8(o.stdout.clone()).expect("UTF-8 output")
}

fn stderr(o: &Output) -> String {
    String::from_utf8(o.stderr.clone()).expect("UTF-8 output")
}

/// A package's sources: one file, and the manifest naming where it goes.
fn sources(dir: &Path) -> std::path::PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join("hello.txt"), b"hello\n").unwrap();
    let manifest = dir.join("manifest.txt");
    std::fs::write(
        &manifest,
        b"name: hello\nversion: 1.2.3\ndescription: Says hello\n\
          file: hello.txt -> /usr/share/hello/hello.txt 0644 -\n",
    )
    .unwrap();
    manifest
}

#[test]
fn with_no_command_the_usage_is_printed() {
    let o = Command::new(env!("CARGO_BIN_EXE_pkg")).output().unwrap();
    assert_eq!(o.status.code(), Some(0));
    assert!(
        stdout(&o).contains("Usage: pkg [--root <dir>] <command>"),
        "{}",
        stdout(&o)
    );
}

#[test]
fn a_root_needs_a_directory() {
    for args in [&["--root"][..], &["--root="][..]] {
        let o = Command::new(env!("CARGO_BIN_EXE_pkg"))
            .args(args)
            .output()
            .unwrap();
        assert_eq!(o.status.code(), Some(1));
        assert_eq!(stderr(&o), "pkg: --root requires a directory\n");
    }
}

#[test]
fn a_package_is_packed_installed_shown_and_removed_under_the_root() {
    let scratch = scratchdir::ScratchDir::new("pkg-cli-life");
    let root = scratch.path("root");
    let manifest = sources(&scratch.path("src"));
    let archive = scratch.path("hello-1.2.3.pkg");

    let o = pkg(
        &root,
        &[
            "pack",
            manifest.to_str().unwrap(),
            "-o",
            archive.to_str().unwrap(),
        ],
    );
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert!(archive.is_file());

    let o = pkg(&root, &["install", archive.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(0), "{}{}", stdout(&o), stderr(&o));
    let installed = root.join("usr/share/hello/hello.txt");
    assert_eq!(std::fs::read(&installed).unwrap(), b"hello\n");
    // The package's state is the root's, not the running system's.
    assert!(root.join("var/pkg/generations/current").is_file());

    let o = pkg(&root, &["list", "--installed"]);
    assert_eq!(o.status.code(), Some(0));
    let listed = stdout(&o);
    assert!(
        listed
            .lines()
            .any(|l| l.starts_with("hello") && l.contains("1.2.3")),
        "{listed}"
    );
    assert!(listed.contains("1 package(s) installed."), "{listed}");

    let o = pkg(&root, &["info", "hello"]);
    assert_eq!(o.status.code(), Some(0));
    let info = stdout(&o);
    for line in [
        "Name:      hello",
        "Version:   1.2.3",
        "Status:    installed",
        "Desc:      Says hello",
    ] {
        assert!(info.lines().any(|l| l == line), "{line:?} in {info}");
    }

    let o = pkg(&root, &["files", "hello"]);
    assert!(
        stdout(&o).contains("/usr/share/hello/hello.txt"),
        "{}",
        stdout(&o)
    );

    let o = pkg(&root, &["remove", "hello"]);
    assert_eq!(o.status.code(), Some(0), "{}{}", stdout(&o), stderr(&o));
    assert!(!installed.exists());
    let o = pkg(&root, &["list", "--installed"]);
    assert_eq!(stdout(&o), "No packages installed.\n");
}

/// An argument that is not UTF-8: a path is taken as the bytes it is, and a
/// word that must be text -- a package's name -- is refused, not a panic.
#[cfg(unix)]
#[test]
fn an_argument_need_not_be_text_where_a_path_is_wanted() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    let scratch = scratchdir::ScratchDir::new("pkg-cli-bytes");
    let root = scratch.path("root");
    let manifest = sources(&scratch.path("src-\u{ff}").join(OsStr::from_bytes(b"\xff")));
    let archive = scratch.path("x").join(OsStr::from_bytes(b"hello-\xfe.pkg"));
    std::fs::create_dir_all(archive.parent().unwrap()).unwrap();
    let run = |args: &[&OsStr]| {
        Command::new(env!("CARGO_BIN_EXE_pkg"))
            .arg("--root")
            .arg(&root)
            .args(args)
            .output()
            .unwrap()
    };

    let o = run(&[
        OsStr::new("pack"),
        manifest.as_os_str(),
        OsStr::new("-o"),
        archive.as_os_str(),
    ]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert!(archive.is_file());
    let o = run(&[OsStr::new("install"), archive.as_os_str()]);
    assert_eq!(o.status.code(), Some(0), "{}{}", stdout(&o), stderr(&o));
    assert!(root.join("usr/share/hello/hello.txt").is_file());

    let o = run(&[OsStr::new("info"), OsStr::from_bytes(b"he\xffllo")]);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(
        stderr(&o),
        "pkg: 'he'$'\\377''llo': not a package name, query or option (it is not UTF-8 text)\n"
    );

    let o = run(&[OsStr::new("which"), OsStr::from_bytes(b"/usr/\xff")]);
    assert_eq!(o.status.code(), Some(1));
    assert!(
        stderr(&o).ends_with(": not owned by any package\n"),
        "{}",
        stderr(&o)
    );
}

#[test]
fn a_font_family_is_searched_for_in_the_root_s_index() {
    let scratch = scratchdir::ScratchDir::new("pkg-cli-fonts");
    let root = scratch.path("root");

    // No index yet: the question cannot be answered.
    let o = pkg(&root, &["search", "--font", "Fira", "Code"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(stderr(&o).contains("no package index"), "{}", stderr(&o));

    let repos = root.join("var/pkg/repos");
    std::fs::create_dir_all(&repos).unwrap();
    std::fs::write(
        repos.join("stable.index"),
        b"name: fonts-fira\nversion: 6.2.0\ndescription: Fira fonts\nfonts: Fira Code, Fira Mono\n\n\
          name: tools\nversion: 1.0.0\ndescription: Tools\n",
    )
    .unwrap();

    let o = pkg(&root, &["search", "--font=fira", "code"]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert_eq!(stdout(&o), "fonts-fira 6.2.0 \n  Fira fonts\n");

    let o = pkg(&root, &["search", "Inter", "--font"]);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(stdout(&o), "");
    assert!(
        stderr(&o).contains("no package carries the font family"),
        "{}",
        stderr(&o)
    );

    // An ordinary search is still one.
    let o = pkg(&root, &["search", "tools"]);
    assert_eq!(o.status.code(), Some(0));
    assert!(stdout(&o).starts_with("tools 1.0.0"), "{}", stdout(&o));
}

/// A package's archive, packed by `pkg pack` from `files` -- each a source
/// name, its contents, and where it is installed.
fn package(
    scratch: &scratchdir::ScratchDir,
    name: &str,
    version: &str,
    files: &[(&str, &str, &str)],
) -> String {
    let dir = scratch.path(&format!("src-{name}-{version}"));
    std::fs::create_dir_all(&dir).unwrap();
    let mut manifest = format!("name: {name}\nversion: {version}\ndescription: {name}\n");
    for (src, contents, dst) in files {
        std::fs::write(dir.join(src), contents).unwrap();
        manifest.push_str(&format!("file: {src} -> {dst} 0644 -\n"));
    }
    let manifest_path = dir.join("manifest.txt");
    std::fs::write(&manifest_path, manifest).unwrap();
    let archive = scratch.path(&format!("{name}-{version}.pkg"));
    let o = Command::new(env!("CARGO_BIN_EXE_pkg"))
        .args([
            "pack",
            manifest_path.to_str().unwrap(),
            "-o",
            archive.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    archive.to_str().unwrap().to_string()
}

/// `pkg ARGS... --json`'s one document.
fn json(o: &Output) -> jsonvalue::JsonValue {
    jsonvalue::json_parse(&stdout(o)).unwrap_or_else(|e| panic!("{e}: {}", stdout(o)))
}

#[test]
fn a_rollback_puts_the_files_back_and_records_a_new_generation() {
    let scratch = scratchdir::ScratchDir::new("pkg-cli-rollback");
    let root = scratch.path("root");
    let hello1 = package(
        &scratch,
        "hello",
        "1.2.3",
        &[
            ("hello.txt", "hello\n", "/usr/share/hello/hello.txt"),
            ("old.txt", "old\n", "/usr/share/hello/old.txt"),
        ],
    );
    let hello2 = package(
        &scratch,
        "hello",
        "2.0.0",
        &[
            ("hello.txt", "hello 2\n", "/usr/share/hello/hello.txt"),
            ("new.txt", "new\n", "/usr/share/hello/new.txt"),
        ],
    );
    let world = package(
        &scratch,
        "world",
        "1.0.0",
        &[("world.txt", "world\n", "/usr/share/world/world.txt")],
    );
    for archive in [&hello1, &hello2, &world] {
        let o = pkg(&root, &["install", archive]);
        assert_eq!(o.status.code(), Some(0), "{}{}", stdout(&o), stderr(&o));
    }
    let file = |p: &str| std::fs::read_to_string(root.join(p)).ok();
    assert_eq!(
        file("usr/share/hello/hello.txt").as_deref(),
        Some("hello 2\n")
    );
    assert_eq!(
        file("usr/share/world/world.txt").as_deref(),
        Some("world\n")
    );

    // Nothing changes on a dry run, a generation that is not there, or a
    // word that is no generation.
    let o = pkg(&root, &["rollback", "1", "--dry-run"]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert!(
        stdout(&o).contains("  - world (will be removed)"),
        "{}",
        stdout(&o)
    );
    assert!(
        stdout(&o).contains("  ~ hello (2.0.0 → 1.2.3)"),
        "{}",
        stdout(&o)
    );
    let o = pkg(&root, &["rollback", "99"]);
    assert_eq!(
        (o.status.code(), stderr(&o).as_str()),
        (Some(1), "pkg: generation 99 not found\n")
    );
    let o = pkg(&root, &["rollback", "first"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(
        stderr(&o).contains("not a generation number"),
        "{}",
        stderr(&o)
    );
    let o = pkg(&root, &["rollback", "99", "--json"]);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(
        json(&o).get("outcome").and_then(|v| v.as_str()),
        Some("refused")
    );
    assert_eq!(
        file("usr/share/hello/hello.txt").as_deref(),
        Some("hello 2\n")
    );
    assert_eq!(
        file("usr/share/world/world.txt").as_deref(),
        Some("world\n")
    );

    // Back to the first generation: world goes, hello is 1.2.3 again.
    let o = pkg(&root, &["rollback", "1", "--json"]);
    assert_eq!(o.status.code(), Some(0), "{}{}", stdout(&o), stderr(&o));
    let report = json(&o);
    let field = |k: &str| report.get(k).cloned();
    assert_eq!(
        field("outcome")
            .and_then(|v| v.as_str().map(String::from))
            .as_deref(),
        Some("done")
    );
    assert_eq!(field("from").and_then(|v| v.as_u64()), Some(3));
    assert_eq!(field("to").and_then(|v| v.as_u64()), Some(1));
    assert_eq!(field("generation").and_then(|v| v.as_u64()), Some(4));
    assert_eq!(
        report
            .get("removed")
            .and_then(|v| v.as_array())
            .map(Vec::len),
        Some(1)
    );
    assert_eq!(
        file("usr/share/hello/hello.txt").as_deref(),
        Some("hello\n")
    );
    assert_eq!(file("usr/share/hello/old.txt").as_deref(), Some("old\n"));
    assert_eq!(
        file("usr/share/hello/new.txt"),
        None,
        "2.0.0's own file goes"
    );
    assert_eq!(file("usr/share/world/world.txt"), None);

    // And forward again, to the generation before the rollback: a new one.
    let o = pkg(&root, &["rollback"]);
    assert_eq!(o.status.code(), Some(0), "{}{}", stdout(&o), stderr(&o));
    assert!(
        stdout(&o).contains("Rolled back. Generation 5 created (based on 3)."),
        "{}",
        stdout(&o)
    );
    assert_eq!(
        file("usr/share/hello/hello.txt").as_deref(),
        Some("hello 2\n")
    );
    assert_eq!(file("usr/share/hello/new.txt").as_deref(), Some("new\n"));
    assert_eq!(file("usr/share/hello/old.txt"), None);
    assert_eq!(
        file("usr/share/world/world.txt").as_deref(),
        Some("world\n")
    );

    // The generations, as data: what made each, and which is current.
    let o = pkg(&root, &["generations", "--json"]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let all = json(&o);
    let all = all.as_array().unwrap();
    assert_eq!(all.len(), 5);
    let op = |i: usize| {
        all[i]
            .get("operation")
            .and_then(|v| v.as_str())
            .map(String::from)
    };
    assert_eq!(op(0).as_deref(), Some("install-local"));
    assert_eq!(op(3).as_deref(), Some("rollback"));
    let current: Vec<u64> = all
        .iter()
        .filter(|g| g.get("current").and_then(|v| v.as_bool()) == Some(true))
        .filter_map(|g| g.get("generation").and_then(|v| v.as_u64()))
        .collect();
    assert_eq!(current, [5]);
}

#[test]
fn an_installed_state_that_cannot_be_read_is_not_taken_for_an_empty_one() {
    let scratch = scratchdir::ScratchDir::new("pkg-cli-corrupt");
    let root = scratch.path("root");
    let hello = package(
        &scratch,
        "hello",
        "1.0.0",
        &[("hello.txt", "hello\n", "/usr/share/hello/hello.txt")],
    );
    let o = pkg(&root, &["install", &hello]);
    assert_eq!(o.status.code(), Some(0), "{}{}", stdout(&o), stderr(&o));
    let gens = root.join("var/pkg/generations");
    std::fs::write(gens.join("current"), "garbage").unwrap();
    for args in [
        &["list", "--installed"][..],
        &["remove", "hello"],
        &["install", &hello],
        &["rollback", "1"],
    ] {
        let o = pkg(&root, args);
        assert_eq!(
            o.status.code(),
            Some(1),
            "{args:?}: {}{}",
            stdout(&o),
            stderr(&o)
        );
        assert!(
            stderr(&o).contains("cannot read the installed state"),
            "{args:?}: {}",
            stderr(&o)
        );
    }
    // Generation 1's record is still the one install wrote.
    assert!(
        std::fs::read_to_string(gens.join("1.gen"))
            .unwrap()
            .contains("package: hello")
    );
    assert!(root.join("usr/share/hello/hello.txt").is_file());
}
