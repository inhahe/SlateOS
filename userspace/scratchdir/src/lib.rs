//! A private on-disk directory for one test, removed when its binding drops.
//!
//! # Why this is a crate and not four functions
//!
//! Six crates in this tree had each written the same test helper: a temp path
//! named after `SystemTime::now().as_nanos()`, sometimes with the pid mixed in,
//! and a `let _ = fs::remove_file(path)` at the end of each test body. Both
//! halves are wrong, and both were wrong in every copy.
//!
//! **The clock is not a unique id, at any resolution.** It is not merely
//! low-resolution — that would be fixable with more digits, a finer clock, or
//! `Instant` instead of `SystemTime`, and none of those help. The system clock
//! is refreshed on a timer interrupt rather than recomputed per read, so two
//! threads that read it inside one tick read the *same value*, however many
//! digits it has. And `cargo test` runs a binary's tests as threads of one
//! process, so "two threads in the same tick" is the normal case rather than a
//! rare race: lane C measured **2133 collisions in 16000 draws — 13%** with an
//! eight-thread probe on this machine.
//!
//! Mixing in `std::process::id()` does not fix it either, though it looks like
//! it should. The pid separates concurrent *runs* of a suite, which is a real
//! axis and worth keeping; it says nothing about the concurrent *threads*
//! inside one run, which is the axis that collides.
//!
//! So uniqueness here comes from the pid **and** a process-wide [`AtomicU64`],
//! which cover the two axes exactly, and the counter is unique by construction
//! rather than by hoping the clock advanced.
//!
//! **The cleanup line cannot run in the case that leaks.** A trailing
//! `let _ = fs::remove_file(path)` runs only when the test reaches the end of
//! its body — that is, only when the test *passed*, and a passing test is
//! precisely the one with nothing interesting to leave behind. An assertion
//! failure unwinds straight past it. [`Drop`] runs during unwind, so the guard
//! covers the failing case that the hand-written tail structurally could not.
//!
//! # What a collision actually costs
//!
//! In the crates that found this the hard way — `ftpd`, `sshd`, `authlib`,
//! `doas`, `logind` — the shared file was a stand-in for `/etc/shadow`. Two
//! tests naming the same file do not merely interfere: one test asserts against
//! the *other's* data, over the code that decides whether a password is
//! correct. That can fail spuriously, which is how it was found, but it can
//! equally **pass** spuriously, and a green run is exactly the evidence anyone
//! would cite for "authentication is covered".
//!
//! See known-issues.md `B-FTPD-SSHD-AUTH-TESTS-SHARE-TEMP-FILES-AND-FLAKE` for
//! the incident, and `design-decisions.md` §349 for why this became a crate
//! rather than a sixth corrected copy.
//!
//! # Use
//!
//! Add it as a dev-dependency — it has no business in a target build:
//!
//! ```toml
//! [dev-dependencies]
//! scratchdir = { path = "../scratchdir" }
//! ```
//!
//! ```
//! # use scratchdir::ScratchDir;
//! let dir = ScratchDir::new("mycrate_test");
//! let shadow = dir.path("shadow");
//! std::fs::write(&shadow, "alice:x:1:0:99999:7:::\n").unwrap();
//! assert!(shadow.exists());
//! // `dir` going out of scope removes the directory and everything in it,
//! // including on an unwind out of a failing assertion.
//! ```

#![deny(clippy::all, clippy::pedantic)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// A directory that exists for as long as this value does.
///
/// Created eagerly by [`ScratchDir::new`] and removed by [`Drop`], including
/// when the drop happens during a panic-unwind.
#[derive(Debug)]
pub struct ScratchDir(PathBuf);

impl ScratchDir {
    /// Create a fresh directory under the system temp directory, named after
    /// `prefix`, the current pid, and a process-wide counter.
    ///
    /// `prefix` exists to make a stray directory attributable to the suite that
    /// made it when someone goes looking in `%TEMP%`; it plays no part in
    /// uniqueness, so callers do not have to keep prefixes distinct from one
    /// another. That is deliberate — the scheme this replaces *did* depend on
    /// callers hand-maintaining distinct tags, which is a convention nothing
    /// checks and one copy-pasted test undoes.
    ///
    /// # Panics
    ///
    /// If the directory cannot be created. A fixture that cannot create the
    /// directory the test asked for has nothing to offer that test, and every
    /// fallback merely moves the failure into an assertion about the wrong
    /// subject. Failing loudly, here, is the behaviour we want.
    #[must_use]
    pub fn new(prefix: &str) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("{prefix}_{}_{n}", std::process::id()));
        #[allow(
            clippy::expect_used,
            reason = "see the Panics section: a fixture that cannot create its \
                      directory must fail as itself, not as the test's subject"
        )]
        std::fs::create_dir_all(&dir).expect("create scratch directory");
        Self(dir)
    }

    /// A path named `name` inside this directory.
    ///
    /// The file need not exist, and this does not create it — several callers
    /// specifically want a path that is *absent* (a `users.yaml` that is not
    /// there, so a shadow store is the only thing that can answer).
    #[must_use]
    pub fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    /// The directory itself, for callers that need to hand the whole thing to
    /// something that will populate it.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.0
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        // Best effort on purpose, and this is the one `let _ =` in the crate.
        // A leaked directory under the system temp directory is noise; a panic
        // raised *here*, during an already-panicking test, aborts the whole
        // test binary and loses every other test's result along with the
        // failure that was being reported.
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::ScratchDir;
    use std::collections::BTreeSet;
    use std::path::PathBuf;

    #[test]
    fn two_scratch_dirs_alive_at_once_never_share_a_path() {
        // Held alive at once on purpose: dropping each before making the next
        // would let a guard that reuses one directory name pass. They all share
        // one prefix, which is the case the tag-based scheme this replaces
        // could not survive.
        let dirs: Vec<ScratchDir> = (0..64).map(|_| ScratchDir::new("sd_test")).collect();
        let paths: Vec<PathBuf> = dirs.iter().map(|d| d.path("f")).collect();

        for (i, p) in paths.iter().enumerate() {
            std::fs::write(p, format!("{i}")).expect("write");
        }

        let distinct: BTreeSet<&PathBuf> = paths.iter().collect();
        assert_eq!(distinct.len(), paths.len(), "two guards produced one path");

        // Distinctness alone would not be enough: a later guard that cleared an
        // earlier one's directory gives the same read-someone-else's-data
        // symptom with all-distinct names.
        for (i, p) in paths.iter().enumerate() {
            assert_eq!(
                std::fs::read_to_string(p).expect("read back"),
                format!("{i}")
            );
        }
    }

    #[test]
    fn concurrent_threads_never_share_a_path() {
        // The actual failure mode, reproduced: many threads entering the
        // constructor at once. The helper this replaces failed here 13% of the
        // time; a counter cannot fail here at all.
        let handles: Vec<_> = (0..8)
            .map(|_| {
                std::thread::spawn(|| {
                    (0..200)
                        .map(|_| ScratchDir::new("sd_race"))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        let all: Vec<ScratchDir> = handles
            .into_iter()
            .flat_map(|h| h.join().expect("thread"))
            .collect();
        let distinct: BTreeSet<PathBuf> = all.iter().map(|d| d.dir().to_path_buf()).collect();
        assert_eq!(distinct.len(), all.len(), "concurrent guards collided");
    }

    #[test]
    fn the_directory_is_removed_even_when_the_test_panics() {
        // The case a trailing cleanup line structurally cannot reach, since the
        // unwind goes straight past it.
        let leaked = std::panic::catch_unwind(|| {
            let dir = ScratchDir::new("sd_panic");
            let path = dir.path("f");
            std::fs::write(&path, "x").expect("write");
            assert!(path.exists());
            // Unwinds out of this closure, dropping `dir` on the way.
            std::panic::panic_any(dir.dir().to_path_buf());
        })
        .expect_err("the closure must panic");
        let dir = leaked
            .downcast::<PathBuf>()
            .expect("the panic payload is the directory under test");
        assert!(
            !dir.exists(),
            "a panicking test leaked {}; the Drop guard did not run",
            dir.display()
        );
    }

    #[test]
    fn a_path_is_not_created_until_the_caller_writes_it() {
        // Callers rely on this: an absent `users.yaml` is the whole point of
        // several fixtures, so `path` must not conjure one.
        let dir = ScratchDir::new("sd_absent");
        assert!(!dir.path("users.yaml").exists());
        assert!(dir.dir().is_dir(), "the directory itself must exist");
    }

    #[test]
    fn the_directory_and_its_contents_go_together() {
        let path = {
            let dir = ScratchDir::new("sd_nested");
            std::fs::create_dir_all(dir.path("a/b")).expect("nested");
            std::fs::write(dir.path("a/b/c"), "x").expect("write");
            dir.dir().to_path_buf()
        };
        assert!(
            !path.exists(),
            "a populated directory must still be removed"
        );
    }
}
