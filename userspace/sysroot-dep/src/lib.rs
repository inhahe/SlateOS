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
    let start = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").ok()?);
    let mut dir = start.as_path();
    loop {
        if dir.join(LIBC_A).exists() {
            return Some(dir.to_path_buf());
        }
        dir = dir.parent()?;
    }
}
