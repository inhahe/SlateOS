//! Every binary that expands `guard_std_fds!` also calls `stdfd::restore`, and
//! the other way round; and the binaries that do neither are exactly the ones
//! listed here.
//!
//! The two halves only work together. The macro installs a constructor that
//! records, before Rust's runtime gets to them, which standard descriptors were
//! closed and how `SIGPIPE` was set; `restore` puts that back. With the macro
//! and no call, the record is made and never used, and `ps` with its standard
//! output closed wrote into the `/dev/null` the runtime put on descriptor 1
//! and reported success. With the call and no macro there is no record, the
//! call does nothing, and `chown -v` did the same. Both compiled, passed
//! clippy and passed their own tests, and both were found on 2026-10-07 by
//! accident -- which is why this reads the source instead.
//!
//! A binary may restore through a library entry point that calls `restore` for
//! it (`coreutils::digest::main`, say). Those modules are found here, not
//! listed: any `src/*.rs` that calls `stdfd::restore()` is one, and a binary
//! that names it counts as restoring.

// A test fails by panicking, so the lints that keep panics out of production
// code are allowed here, where a panic is the point.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The binaries that have neither half yet. They answer a closed standard
/// descriptor as if it were `/dev/null`, and a broken pipe quietly rather than
/// by dying of `SIGPIPE` -- see
/// `known-issues/TD-COREUTILS-AN-UNWRITABLE-STDERR-ABORTS-THE-PROCESS.md`,
/// which records each one's conversion as it happens.
///
/// Exact both ways: a new binary without the guard fails here until it has
/// one or is added, and a converted binary fails here until it is taken off.
const NOT_YET_GUARDED: &[&str] = &[
    "bc", "chmod", "df", "dir", "ed", "find", "hostname", "install", "kill", "ls", "more", "patch",
    "stat", "vdir",
];

fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The whole source of every binary, by name: `src/bin/NAME.rs`, or every
/// file under `src/bin/NAME/`.
fn binaries() -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let bin = crate_dir().join("src/bin");
    for entry in std::fs::read_dir(&bin).expect("src/bin reads") {
        let path = entry.expect("an entry").path();
        if path.is_dir() {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            out.insert(name, all_rust_under(&path));
        } else if path.extension().is_some_and(|e| e == "rs") {
            let name = path.file_stem().unwrap().to_string_lossy().into_owned();
            out.insert(name, std::fs::read_to_string(&path).expect("utf-8 source"));
        }
    }
    assert!(out.len() > 100, "only {} binaries found", out.len());
    out
}

fn all_rust_under(dir: &Path) -> String {
    let mut text = String::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in std::fs::read_dir(&d).expect("readable") {
            let path = entry.expect("an entry").path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                text.push_str(&std::fs::read_to_string(&path).expect("utf-8 source"));
            }
        }
    }
    text
}

/// The library modules that call `restore` themselves, as `coreutils::NAME::`.
fn restoring_modules() -> Vec<String> {
    let src = crate_dir().join("src");
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&src).expect("src reads") {
        let path = entry.expect("an entry").path();
        if path.file_name().is_some_and(|n| n == "stdfd.rs") {
            continue;
        }
        let text = if path.is_dir() {
            all_rust_under(&path)
        } else if path.extension().is_some_and(|e| e == "rs") {
            std::fs::read_to_string(&path).expect("utf-8 source")
        } else {
            continue;
        };
        if text.contains("stdfd::restore()") {
            let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
            out.push(format!("coreutils::{stem}::"));
        }
    }
    out
}

#[test]
fn a_binary_with_the_guard_restores_and_one_that_restores_has_the_guard() {
    let modules = restoring_modules();
    // The three that exist today, so a change to how they are found shows.
    for known in [
        "coreutils::digest::",
        "coreutils::basenc::",
        "coreutils::pgrep::",
    ] {
        assert!(modules.iter().any(|m| m == known), "{known} in {modules:?}");
    }
    let mut mismatched = Vec::new();
    let mut neither = Vec::new();
    for (name, text) in binaries() {
        let guarded = text.contains("guard_std_fds!");
        let restores =
            text.contains("stdfd::restore()") || modules.iter().any(|m| text.contains(m));
        match (guarded, restores) {
            (true, true) => {}
            (false, false) => neither.push(name),
            (true, false) => mismatched.push(format!(
                "{name}: expands guard_std_fds! and never calls stdfd::restore"
            )),
            (false, true) => mismatched.push(format!(
                "{name}: calls stdfd::restore without guard_std_fds!, so it restores nothing"
            )),
        }
    }
    assert!(mismatched.is_empty(), "{mismatched:#?}");
    assert_eq!(
        neither, NOT_YET_GUARDED,
        "the binaries with neither half; update NOT_YET_GUARDED, and the \
         known-issues entry, to match"
    );
}
