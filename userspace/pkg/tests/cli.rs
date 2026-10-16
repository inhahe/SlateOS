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
