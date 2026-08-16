//! The `$PATH` a test may search: the ambient one, minus the two directories
//! cargo injects for the duration of a test run.
//!
//! **This is what keeps the suite's result a property of the code rather than of
//! what else happens to have been built.** Before it runs a test binary, cargo
//! prepends `target/<triple>/<profile>/deps` and the profile directory above it
//! to `PATH` — the platform's dynamic-library search variable — so that the
//! binary can find the shared libraries its crate links against. On this
//! workspace that same directory holds ~200 SlateOS coreutils, `grep.exe`,
//! `sed.exe` and `cat.exe` among them. So a shell test that pipes through
//! `grep` runs *ours*, and only once `cargo test --workspace` (or any earlier
//! `cargo build`) has built it. Same commit, same test binary, two answers:
//! `cargo test -p oils` green and `cargo test --workspace` red, which is how
//! lane C found this
//! (`requests/c-b-workspace-test-red-slateos-coreutils-shadow-host.md`).
//!
//! The tools these tests reach for are *scaffolding*, and the scaffolding has to
//! be the reference implementation. They are differential tests against bash's
//! documented behaviour: `set -o | grep '^posix'` is a question about `set -o`,
//! and when it fails it must be because `set -o` is wrong. A `grep` of our own
//! in that position turns every gap in our coreutils into a shell-test failure
//! filed against the shell — the defect misattributed, and the real one hidden
//! behind it. Our coreutils are tested by their own suites, and the gaps this
//! uncovered are fixed there; see `known-issues.md` →
//! `B-THE-OILS-TESTS-RESOLVED-grep-sed-cat-FROM-THE-CARGO-BUILD-DIRECTORY`.
//!
//! ## Why this file is included rather than linked
//!
//! Both suites need it and they must not disagree. The in-process tests bind
//! [`host_path`] to a *shell variable* — `std::env::set_var` is unsound while
//! another thread may be reading the environment, which is why Rust 2024 made it
//! `unsafe`, and libtest runs every test on a thread. The end-to-end tests
//! spawn the real `osh` binary, which a shell variable cannot reach, so they
//! must hand the child a cleaned `PATH` through [`Command::env`]; [`scrub`] is
//! that call.
//!
//! A `tests/common/mod.rs` would serve the second group and not the first, and
//! a `pub` module of the library would put test scaffolding in the shipped
//! shell. So the file lives in `src/`, is declared `#[cfg(test)]` by `lib.rs`,
//! and is pulled into each integration test with `#[path = "../src/hostpath.rs"]`
//! — one source of truth, and not a byte of it in the binary that ships.

// This file is compiled into four crates — the library's own test build and the
// three integration tests — and each uses a different part of it: the in-process
// tests want `host_path`, the spawning ones want `scrub`. Whichever is unused in
// a given build is not dead code, it is the other suite's half.
#![allow(dead_code)]

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Whether two directory names denote the same directory, for the purpose of
/// striking one out of `$PATH`.
///
/// A textual comparison, not a `canonicalize`: the entries being compared are
/// both produced by the same process — cargo wrote one into the environment,
/// [`std::env::current_exe`] reported the other — so they differ at most in
/// separator style and, on Windows where the filesystem is case-insensitive, in
/// case. Canonicalising instead would touch the disk once per `$PATH` entry,
/// and would answer *no* for a directory that has since been removed, which is
/// the one case where striking it out matters least but costs the most to get
/// wrong.
fn same_dir(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| {
        let s = p.to_string_lossy().replace('\\', "/");
        let s = s.strip_suffix('/').map_or(s.clone(), str::to_string);
        if cfg!(windows) { s.to_ascii_lowercase() } else { s }
    };
    norm(a) == norm(b)
}

/// The directories cargo prepended to `PATH` before running this test binary.
///
/// `current_exe` is `…/target/<triple>/<profile>/deps/<test binary>`, so its
/// parent is the `deps` directory and its grandparent the profile directory —
/// exactly the two entries cargo adds. Deriving them rather than matching on
/// path shape means this keeps working under a custom `--target-dir`, a
/// different profile, or a cross-compilation triple.
pub fn injected_dirs() -> Vec<PathBuf> {
    let mut injected = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(deps) = exe.parent() {
            injected.push(deps.to_path_buf());
            if let Some(profile) = deps.parent() {
                injected.push(profile.to_path_buf());
            }
        }
    }
    injected
}

/// The ambient `PATH` with [`injected_dirs`] struck out.
pub fn host_path() -> OsString {
    let injected = injected_dirs();
    let ambient = std::env::var_os("PATH").unwrap_or_default();
    let kept: Vec<PathBuf> = std::env::split_paths(&ambient)
        .filter(|d| !injected.iter().any(|bad| same_dir(bad, d)))
        .collect();
    // `join_paths` refuses a component containing the separator, which no
    // directory that arrived *from* a split can hold; the fallback is
    // unreachable and is written out rather than unwrapped because being
    // test-only is not a licence to panic on a `Result` nothing proved.
    std::env::join_paths(kept).unwrap_or(ambient)
}

/// Give a child process the host's `PATH` instead of the one cargo staged.
///
/// The end-to-end tests spawn the real `osh`; a child inherits the *process*
/// environment, so the shell variable that fixes the in-process tests never
/// reaches it and the `PATH` has to be set on the `Command` itself.
pub fn scrub(cmd: &mut std::process::Command) -> &mut std::process::Command {
    cmd.env("PATH", host_path())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_build_directories_are_not_on_the_host_path() {
        let path = host_path();
        let kept: Vec<PathBuf> = std::env::split_paths(&path).collect();
        for bad in injected_dirs() {
            assert!(
                !kept.iter().any(|d| same_dir(&bad, d)),
                "cargo's {} survived the scrub",
                bad.display()
            );
        }
    }

    #[test]
    fn nothing_else_is_dropped() {
        // Striking out cargo's two entries must not disturb the rest: a test
        // that could not find the host's `sh` or `sed` would fail for a reason
        // of this module's own making.
        let ambient: Vec<PathBuf> = std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        )
        .collect();
        let injected = injected_dirs();
        let kept: Vec<PathBuf> = std::env::split_paths(&host_path()).collect();
        let expected = ambient
            .iter()
            .filter(|d| !injected.iter().any(|bad| same_dir(bad, d)))
            .count();
        assert_eq!(kept.len(), expected);
    }

    #[test]
    fn a_directory_is_matched_across_separator_style_and_case() {
        assert!(same_dir(Path::new("a/b"), Path::new("a\\b")));
        assert!(same_dir(Path::new("a/b/"), Path::new("a/b")));
        assert_eq!(
            same_dir(Path::new("A/B"), Path::new("a/b")),
            cfg!(windows),
            "case folding must follow the platform's filesystem"
        );
    }
}
