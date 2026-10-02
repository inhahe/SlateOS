//! `file` run as a program: what lives in `main` -- the options, the output
//! line, standard input, the version text -- and the database the program
//! carries, which `libmagic`'s own tests do not reach. `scripts/file-diff.sh`
//! holds all of it byte for byte to file 5.45; these are the parts worth
//! knowing about without WSL.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::ffi::OsStr;
use std::io::Write as _;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

fn file<S: AsRef<OsStr>>(args: &[S]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_file"))
        .args(args)
        .output()
        .expect("run file")
}

/// A scratch file holding `bytes`, removed when the guard is dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str, bytes: &[u8]) -> Scratch {
        let path = std::env::temp_dir().join(format!("file-cli-{tag}-{}", std::process::id()));
        std::fs::write(&path, bytes).unwrap();
        Scratch(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        // A leftover scratch file is the only consequence of a failed removal.
        let _ = std::fs::remove_file(&self.0);
    }
}

fn stdout(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).unwrap()
}

#[test]
fn text_is_identified_and_named() {
    let f = Scratch::new("text", b"hello world\n");
    let out = file(&[&f.0]);
    assert!(out.status.success());
    assert_eq!(stdout(&out), format!("{}: ASCII text\n", f.0.display()));
}

#[test]
fn brief_and_mime_shape_the_line() {
    let f = Scratch::new("mime", b"hello world\n");
    assert_eq!(
        stdout(&file(&[OsStr::new("-b"), f.0.as_os_str()])),
        "ASCII text\n"
    );
    assert_eq!(
        stdout(&file(&[OsStr::new("-bi"), f.0.as_os_str()])),
        "text/plain; charset=us-ascii\n"
    );
    assert_eq!(
        stdout(&file(&[
            OsStr::new("--mime-type"),
            OsStr::new("-b"),
            f.0.as_os_str()
        ])),
        "text/plain\n"
    );
}

/// The database the program carries is there to be used: its own executable
/// is identified as one, whatever the host's format.
#[test]
fn the_program_itself_is_an_executable() {
    let line = stdout(&file(&["-b", env!("CARGO_BIN_EXE_file")]));
    assert!(
        line.starts_with("ELF ") || line.starts_with("PE32"),
        "{line}"
    );
}

#[test]
fn an_empty_file_is_empty() {
    let f = Scratch::new("empty", b"");
    assert_eq!(
        stdout(&file(&[OsStr::new("-b"), f.0.as_os_str()])),
        "empty\n"
    );
}

/// A file that cannot be opened is reported in the output line, and is not a
/// failure of the run -- upstream's rule, which `-E` changes.
#[test]
fn a_missing_file_is_reported_in_its_line() {
    let out = file(&["no-such-file-here"]);
    assert!(out.status.success());
    assert_eq!(
        stdout(&out),
        "no-such-file-here: cannot open `no-such-file-here' (No such file or directory)\n"
    );
}

#[test]
fn a_dash_reads_standard_input() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_file"))
        .args(["-b", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"#!/bin/sh\necho hi\n")
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert_eq!(stdout(&out), "POSIX shell script, ASCII text executable\n");
}

/// `-v` is file 5.45's, and names where the database comes from.
#[test]
fn version_names_the_magic_file() {
    let out = file(&["-v"]);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(text.contains("-5.45\nmagic file from "), "{text}");
}
