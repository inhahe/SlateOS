//! Tell cargo that `toolchain/sysroot/lib/libc.a` is an input.
//!
//! Every SlateOS userspace binary links a static archive that cargo cannot
//! see: `libc.a` is produced by `toolchain/build-sysroot.ps1`, by hand,
//! outside cargo entirely. So when the libc changes, nothing that links it is
//! out of date, the build prints `Finished`, and every binary still contains
//! the old library.
//!
//! Measured before this existed, on the real tree:
//!
//! ```text
//! cp mtime before        1789347710
//! libc.a touched to      1789371780
//! cargo build            Finished in 0.72s
//! cp mtime after         1789347710      <- unchanged
//! ```
//!
//! Deleting the output does not help, and that is the part that misleads:
//! `target/.../release/cp` is a HARDLINK into `deps/`, so removing it leaves
//! the inode, and cargo re-creates the name from its cache without running the
//! linker — restoring the file with its **original mtime**. The obvious way to
//! force a relink is indistinguishable from having done nothing.
//!
//! # Using it
//!
//! This is a `[build-dependencies]` entry, not a normal one. The crate that
//! links the sysroot needs a `build.rs` of its own:
//!
//! ```ignore
//! fn main() {
//!     sysroot_dep::emit();
//! }
//! ```
//!
//! The logic lives here so it is written once; the `cargo:rerun-if-changed`
//! has to be emitted by the dependent's own build script, because that is
//! whose rebuild it governs. A shared *library* crate would not do: cargo
//! would re-run ITS build script and, finding the output unchanged, leave the
//! dependents alone.

use std::path::{Path, PathBuf};

/// The target that actually links the sysroot. Every other target — the
/// Windows or Linux host used for `cargo test` — links the platform's own
/// libc, so making a host build depend on this archive would invalidate it for
/// a file it never reads.
const SLATEOS: &str = "x86_64-slateos";

/// Where the archive sits, relative to the repository root.
const LIBC_A: &str = "toolchain/sysroot/lib/libc.a";

/// Emit the cargo directives that make `libc.a` a build input.
///
/// Safe to call from any crate on any target: on a non-SlateOS target it does
/// nothing, so a host `cargo test` is unaffected.
pub fn emit() {
    // ABSENT AND "NOT SLATEOS" ARE DIFFERENT ANSWERS, and the first draft of
    // this used `unwrap_or_default()`, which made them the same one.
    // `check-read-defaults.py` refused the push for it, correctly: cargo sets
    // `TARGET` for every build script, so a missing one is not a host build,
    // it is a broken environment -- and silently treating it as "not SlateOS"
    // would mean the archive goes untracked while everything reports success.
    // That is precisely the no-op-that-looks-correct this crate exists to
    // stop, reintroduced one level up.
    let Ok(target) = std::env::var("TARGET") else {
        println!(
            "cargo:warning=sysroot-dep: TARGET is unset, so this build cannot              tell whether it links the sysroot; libc.a is NOT being tracked and              a libc rebuild will not relink this crate"
        );
        return;
    };
    // A host build links the platform's own libc and must not depend on ours.
    if !target.contains(SLATEOS) {
        return;
    }

    let Some(root) = repo_root() else {
        println!(
            "cargo:warning=sysroot-dep: no `{LIBC_A}` found above this crate; \
             a libc rebuild will not relink this binary"
        );
        return;
    };
    let archive = root.join(LIBC_A);

    // The dependency itself. This is what makes the build script re-run.
    println!("cargo:rerun-if-changed={}", archive.display());

    // ...and this is what makes the CRATE rebuild. `rerun-if-changed` alone
    // only re-runs the script; cargo compares the script's output and, if it
    // is the same, leaves the crate as it was. The fingerprint has to change
    // when the archive does, or the whole thing is a no-op that looks correct.
    println!(
        "cargo:rustc-env=SYSROOT_LIBC_FINGERPRINT={}",
        fingerprint(&archive)
    );
}

/// A value that changes when the archive does.
///
/// Modification time and length, not a hash: the archive is ~12 MB and this
/// runs on every build of every dependent. mtime alone would be enough almost
/// always, and the length is there for the case it is not — a rebuild that
/// produces a different archive within the filesystem's timestamp granularity.
fn fingerprint(archive: &Path) -> String {
    let Ok(meta) = std::fs::metadata(archive) else {
        // Absent is a state worth distinguishing from any present state, and
        // worth being stable: a changing value here would rebuild the world on
        // every single build until someone built the sysroot.
        return "absent".to_string();
    };
    let secs = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs());
    format!("{secs}.{}", meta.len())
}

/// Walk up from the crate being built until a tree holding `LIBC_A` is found.
///
/// Walked rather than hard-coded as `../../`: this is called from crates at
/// two different depths already (`userspace/coreutils`, `userspace/ar`), and a
/// relative path that is right for one is silently wrong for the other — it
/// would resolve to nothing, take the `absent` branch, and report success.
fn repo_root() -> Option<PathBuf> {
    repo_root_from(Path::new(&std::env::var("CARGO_MANIFEST_DIR").ok()?))
}

/// [`repo_root`] with the starting directory passed in rather than read from
/// the environment.
///
/// Split out so the walk can be tested without `set_var`. A test that mutates
/// a process-global is shared state between every other test in the same
/// binary, which is what `scripts/raced-globals.py` exists to find; removing
/// the sharing is better than serialising around it.
fn repo_root_from(start: &Path) -> Option<PathBuf> {
    let mut dir = start;
    loop {
        if dir.join(LIBC_A).exists() {
            return Some(dir.to_path_buf());
        }
        dir = dir.parent()?;
    }
}

#[cfg(test)]
mod tests {
    // The workspace's defensive lints are for production code; a test that
    // cannot create its own scratch directory should stop loudly rather than
    // carry on and report something about nothing. Same form as
    // userspace/randdist.
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::{LIBC_A, fingerprint, repo_root_from};
    use std::io::Write as _;
    use std::path::PathBuf;

    /// A scratch directory that removes itself, so a failing test does not
    /// leave the temp tree dirtier than it found it.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(tag: &str) -> Self {
            let mut p = std::env::temp_dir();
            p.push(format!("sysroot-dep-test-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).expect("scratch dir");
            Self(p)
        }
        fn path(&self) -> &std::path::Path {
            &self.0
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// An ABSENT archive is a distinct, STABLE answer.
    ///
    /// Stable matters as much as distinct: a value that varied for a missing
    /// file would rebuild every dependent on every build until someone built
    /// the sysroot, which is a worse failure than the one this crate fixes.
    #[test]
    fn a_missing_archive_fingerprints_as_absent_and_stays_put() {
        let s = Scratch::new("absent");
        let missing = s.path().join("nosuch.a");
        assert_eq!(fingerprint(&missing), "absent");
        assert_eq!(fingerprint(&missing), "absent", "and does not vary");
    }

    /// The fingerprint changes when the archive does, which is the half that
    /// makes the crate rebuild rather than merely re-running its script.
    #[test]
    fn the_fingerprint_tracks_the_archive() {
        let s = Scratch::new("track");
        let a = s.path().join("libc.a");
        std::fs::write(&a, b"one").expect("write");
        let first = fingerprint(&a);
        assert_ne!(first, "absent");

        // Same file, untouched: the same answer. Without this the fix would
        // "work" by rebuilding unconditionally, which is the other failure.
        assert_eq!(fingerprint(&a), first, "an untouched archive must not move");

        // Different LENGTH is caught even if the clock has not ticked, which
        // is exactly why the length is in there beside the mtime.
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&a)
            .expect("append");
        f.write_all(b"-longer").expect("append");
        drop(f);
        assert_ne!(fingerprint(&a), first, "a changed archive must move");
    }

    /// `repo_root` walks UP, so it works from any depth.
    ///
    /// Hard-coding `../../` would be right for `userspace/coreutils` and
    /// silently wrong for anything at another depth: it would resolve to
    /// nothing, take the `absent` branch, and report success.
    #[test]
    fn the_root_is_found_by_walking_up_not_by_a_fixed_depth() {
        let s = Scratch::new("walk");
        let root = s.path();
        std::fs::create_dir_all(root.join(LIBC_A).parent().expect("parent")).expect("dirs");
        std::fs::write(root.join(LIBC_A), b"x").expect("archive");

        // Deep enough that any fixed number of `..` would be wrong.
        let deep = root.join("userspace").join("a").join("b").join("c");
        std::fs::create_dir_all(&deep).expect("deep");
        let found = repo_root_from(&deep).expect("the root is above us");
        assert_eq!(
            std::fs::canonicalize(found).expect("canon"),
            std::fs::canonicalize(root).expect("canon"),
        );

        // ...and a tree with no archive yields None rather than a wrong guess.
        let bare = Scratch::new("bare");
        assert!(
            repo_root_from(bare.path()).is_none(),
            "no archive above means no root"
        );
    }
}
