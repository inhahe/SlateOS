// Kernel build script: linker script, and the SPARK/Ada components.

//
// This used to live in the workspace-root `.cargo/config.toml` as
// `link-arg=-Tkernel/linker.ld`, but that flag is merged into every
// crate targeting `x86_64-unknown-none` — including the bare-metal
// services in `services/`, which then failed to link because
// `kernel/linker.ld` doesn't exist relative to their build cwd. Keeping
// it in a build.rs scopes it to the kernel crate only.

use std::path::{Path, PathBuf};
use std::process::Command;

// ---------------------------------------------------------------------------
// Ada/SPARK integration
//
// `kernel/ada/` holds the safety-critical driver logic that design.txt (lines
// 84-95) puts in SPARK rather than Rust. It is compiled by a *cross* GNAT
// (x86_64-elf) against a one-file ZFP runtime and linked in as a plain ELF
// object; see design-decisions.md §205.
//
// THE PROBLEM THIS CODE SOLVES
//
// The Ada toolchain is a ~1 GB install, and the boot test builds the whole
// workspace — so requiring it outright would block lanes B and C from building
// the kernel at all, over a component neither of them touches. But the
// alternative, a committed prebuilt object, has a failure mode that is much
// worse than an inconvenient install: the object silently stops matching the
// source it was proved from, and we ship a "proved" component whose proof is
// about different code.
//
// So both, with a gate. The object is committed, *and* a stamp file records
// the SHA-256 of every input that determines it (the two Ada sources, the
// runtime's system.ads, the target parameters, and the exact compiler flags).
//
//   * Every build, with or without a toolchain, re-hashes those inputs and
//     compares. A source edited without regenerating the object fails the
//     build, everywhere, immediately — which is the case that actually matters,
//     because it is the one that would otherwise be invisible.
//   * Builds that *do* have the toolchain recompile and compare the result
//     byte-for-byte against the committed object (GNAT's output is
//     deterministic here — verified), so the committed artifact cannot drift
//     from what the compiler actually produces either.
//
// Regenerate with: python kernel/ada/regen-prebuilt.py
// ---------------------------------------------------------------------------

/// Flags that determine the object. Any change here changes the stamp, which
/// is the point: `-mno-red-zone` and `-mcmodel=kernel` are load-bearing (an
/// interrupt can arrive at any instruction and would clobber the red zone; the
/// kernel is mapped in the top 2 GiB), and a build that quietly dropped one
/// would produce an object that links fine and misbehaves under interrupt.
/// Kept identical to `kernel/ada/virtqueue.gpr`, which is what gnatprove reads
/// — if these two disagree, we prove one thing and ship another.
const ADA_FLAGS: &[&str] = &[
    "-mno-red-zone",
    "-mcmodel=kernel",
    "-O2",
    "-gnatwa",
    "-gnatw.X",
];

/// Ada unit names to compile. GNAT requires the object file to be named after
/// the unit, so this doubles as the object basename.
const ADA_UNITS: &[&str] = &["virtqueue_descriptors"];

fn main() {
    // CARGO_MANIFEST_DIR is always set by cargo when invoking a build
    // script; if it isn't, fall back to a relative path so we still
    // produce a usable -T arg rather than panicking the whole build.
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    // Linker script anchored to this crate's directory. Lives here
    // rather than in `.cargo/config.toml` because cargo merges rustflags
    // into every crate sharing the target triple — a workspace-level
    // `link-arg=-T<path>` would also be passed when building bare-metal
    // services, which need their own linker scripts.
    println!("cargo:rustc-link-arg=-T{manifest}/linker.ld");
    println!("cargo:rerun-if-changed=linker.ld");

    // --- Embedded service binaries: declare the edge, and refuse without it ---
    //
    // The kernel `include_bytes!`s compiled service binaries -- six of them, at
    // fifteen sites across container.rs, main.rs and proc/spawn.rs. Nothing in
    // this build script used to mention them, which is two bugs:
    //
    //   Loud:  on a tree without the artifacts the kernel cannot build, and it
    //          fails as fifteen `include_bytes!` errors blaming the kernel for a
    //          file the kernel is not missing. `boot-test.sh`'s own comment has
    //          described that symptom for months without anyone connecting it
    //          to a missing build-graph edge.
    //   Quiet: with no `rerun-if-changed`, rebuilding a service does NOT
    //          invalidate the kernel. It keeps embedding the previous copy and
    //          says nothing, so a change to an embedded program appears to have
    //          no effect -- indistinguishable from one that genuinely has none,
    //          which sends the debugging to the program rather than the build.
    //
    // The list is DERIVED by scanning for the `include_bytes!` calls, not
    // written out here. `scripts/bootstrap-worktree.sh` derives the same list
    // the same way, and that is why it is the one artefact in this story that
    // never drifted while four sessions counted by hand and got four different
    // answers. A hardcoded list would be a fifth hand-count that runs on every
    // build.
    //
    // ORDER IS LOAD-BEARING: check existence first, and only then emit the
    // directives. Measured on cargo 1.95.0 -- a `rerun-if-changed` naming a
    // path that does not exist makes this script re-run on EVERY build (a
    // counter gave 1,2,3 across three no-op builds, against 1,1,1 for a path
    // that exists). Declaring before checking would therefore trade the quiet
    // bug above for a second one whose only symptom is that the kernel never
    // caches -- which reads as "the kernel is slow to build" and would survive
    // indefinitely.
    //
    // NOT built here. `services` is in the root manifest's `exclude` list
    // deliberately -- its comment says that keeps those crates outside the
    // `userspace/.cargo/config.toml` build-std blast radius -- so driving their
    // builds from this script would re-couple what the manifest decouples, and
    // hide the coupling in a build script rather than the manifest that states
    // it. Provisioning stays in `bootstrap-worktree.sh`.
    let embedded = embedded_service_artifacts(&manifest);
    if embedded.is_empty() {
        // Fail closed, mirroring bootstrap-worktree.sh: finding none means the
        // scan broke, not that the kernel stopped embedding services.
        panic!(
            "kernel/build.rs: found no include_bytes! service artifacts under \
             kernel/src. The kernel embeds several; finding none means this scan \
             is broken, not that they are gone."
        );
    }
    let missing: Vec<&String> = embedded.iter().filter(|p| !Path::new(p).exists()).collect();
    if !missing.is_empty() {
        let mut msg = String::from(
            "kernel/build.rs: the kernel embeds service binaries that are not \
             built in this checkout.\n\nMissing:\n",
        );
        for p in &missing {
            msg.push_str("    ");
            msg.push_str(p);
            msg.push('\n');
        }
        msg.push_str(
            "\nBuild them with:\n    ./scripts/bootstrap-worktree.sh\n\
             or run the boot test with --bootstrap to provision and continue.\n\n\
             If this tree was working a minute ago, the usual cause is a `cargo \
             clean` in a service crate: these live under services/*/target/, \
             which is gitignored, so cleaning one silently removes an input to \
             the kernel build.\n",
        );
        panic!("{msg}");
    }
    for p in &embedded {
        println!("cargo:rerun-if-changed={p}");
    }

    // `kernel/src/layout_pad.rs` reads this with `option_env!`, so cargo has to
    // be told that changing it invalidates the build. Without this line a
    // layout sweep would set a new pad value, get a cache hit, and measure the
    // *previous* layout under the new label — a calibration that silently
    // measures nothing, which is precisely the failure the pad exists to
    // detect. (The kernel also prints the value it was actually built with in
    // its boot banner, so a stale build is visible in the record too; belt and
    // braces, because this one is invisible at the point where it matters.)
    println!("cargo:rerun-if-env-changed=SLATEOS_TEXT_PAD");

    ada_build(Path::new(&manifest));
}

fn ada_build(manifest: &Path) {
    let ada = manifest.join("ada");
    let prebuilt = ada.join("prebuilt");

    // Only the kernel target needs these objects. A host-target build (`cargo
    // test` on the dev machine, which compiles the kernel crate for
    // x86_64-pc-windows-gnu to run unit tests) must not try to link x86-64 ELF
    // objects into a PE image — it would fail at link time with an error that
    // says nothing about why. The Rust side's `#[cfg]` gates the externs to
    // match; see kernel/src/ada.rs.
    let target = std::env::var("TARGET").unwrap_or_default();
    let for_kernel = target == "x86_64-unknown-none" || target.contains("slateos");

    for f in ["src", "rts/adainclude/system.ads", "x86_64-elf.atp"] {
        println!("cargo:rerun-if-changed=ada/{f}");
    }
    println!("cargo:rerun-if-changed=ada/prebuilt/stamp.txt");

    let stamp_path = prebuilt.join("stamp.txt");
    let recorded = match std::fs::read_to_string(&stamp_path) {
        Ok(s) => s,
        Err(e) => {
            // Missing entirely is only tolerable if we are not building the
            // kernel image; otherwise there is nothing to link.
            //
            // `assert!` rather than `if … { panic!() }`: a build script's `main`
            // returns `()`, so there is no error channel to propagate into and
            // nothing downstream that could act on a failure — failing the build
            // loudly is the correct behaviour here, not a lint to suppress. The
            // assert spelling is what `clippy::manual_assert` asks for, matches
            // the one at the bottom of this function, and keeps the crate's
            // `panic = "warn"` lint free to catch a panic that is *not*
            // deliberate. Always-on, not `debug_assert!`: a release build must
            // refuse just as loudly.
            assert!(
                !for_kernel,
                "kernel/ada/prebuilt/stamp.txt is missing or unreadable ({e}).\n\
                 Regenerate it with: python kernel/ada/regen-prebuilt.py"
            );
            return;
        }
    };

    let expected = ada_stamp(&ada);
    // `assert!` for the reason given above the previous one. Not `assert_eq!`:
    // that would print the two digests a second time, as `left`/`right`, under
    // labels that say nothing about which is the source and which is the object
    // — the message below already names them, and naming them is the point.
    assert!(
        recorded.trim() == expected.trim(),
        "\n\
             The Ada/SPARK sources in kernel/ada/ have changed, but the compiled\n\
             object in kernel/ada/prebuilt/ was not regenerated.\n\
             \n\
             This is a hard error rather than a warning on purpose: the object is\n\
             what ships, and gnatprove's result is about the *source*. Linking a\n\
             stale object would mean shipping a component whose proof describes\n\
             different code -- the exact failure the stamp exists to make loud.\n\
             \n\
             Regenerate (needs the Ada toolchain -- see design-decisions.md §205):\n\
                 python kernel/ada/regen-prebuilt.py\n\
             \n\
             expected: {expected}\n\
             recorded: {recorded}\n",
        expected = expected.trim(),
        recorded = recorded.trim(),
    );

    if !for_kernel {
        return;
    }

    // If a cross GNAT is available, rebuild and require the result to match the
    // committed object byte-for-byte. The stamp above catches an edited source;
    // this catches a *tampered or mis-generated object*, which the stamp cannot
    // see because the stamp records what the object should be built from, not
    // what it contains.
    if let Some(gcc) = find_ada_gcc() {
        verify_against_toolchain(&gcc, &ada, &prebuilt);
    }

    for unit in ADA_UNITS {
        let obj = prebuilt.join(format!("{unit}.o"));
        assert!(
            obj.is_file(),
            "kernel/ada/prebuilt/{unit}.o is missing. \
             Regenerate with: python kernel/ada/regen-prebuilt.py"
        );
        println!("cargo:rustc-link-arg={}", obj.display());
    }
}

/// SHA-256 over every input that determines the object, in a fixed order.
///
/// This duplicates `stamp()` in kernel/ada/regen-prebuilt.py, and the
/// duplication is safe because it is self-policing rather than merely
/// documented: `stamp.txt` is written by the Python side and checked by this
/// one, so if the two ever computed different digests -- a reordered input, a
/// different newline rule, a typo in the SHA-256 constants -- *every* build
/// would fail on the very next run, on every machine, with the mismatch
/// printed. There is no state in which they silently disagree, which is the
/// only property that would have made sharing the code worth a build
/// dependency.
fn ada_stamp(ada: &Path) -> String {
    let mut h = sha2::Sha256::new();

    // Flags first: a flag change with unchanged sources must still invalidate.
    h.update(ADA_FLAGS.join(" ").as_bytes());
    h.update(b"\n");

    let mut inputs: Vec<PathBuf> = vec![
        ada.join("rts").join("adainclude").join("system.ads"),
        ada.join("x86_64-elf.atp"),
    ];
    for unit in ADA_UNITS {
        inputs.push(ada.join("src").join(format!("{unit}.ads")));
        inputs.push(ada.join("src").join(format!("{unit}.adb")));
    }

    for p in &inputs {
        // `panic!` and not `?`: this is reached only from a build script, whose
        // `main` returns `()`, so there is no error channel to propagate into.
        // It cannot become an `assert!` like the two above — the value is needed
        // on the success path, and the failure carries both the path and the io
        // error, neither of which an assert condition can name. A silent skip
        // would be the one genuinely wrong answer: the digest would then be
        // computed over fewer inputs than it claims to cover, and a changed Ada
        // source would hash to the recorded stamp and link a stale object.
        #[allow(clippy::panic)]
        let bytes = std::fs::read(p)
            .unwrap_or_else(|e| panic!("cannot read Ada input {}: {e}", p.display()));
        // Normalise line endings before hashing. The repo is checked out with
        // core.autocrlf on Windows, so the same commit yields CRLF here and LF
        // in CI -- hashing the raw bytes would make the stamp fail on one of
        // them for a difference the compiler does not care about.
        let norm: Vec<u8> = bytes.into_iter().filter(|&b| b != b'\r').collect();
        h.update(&norm);
        h.update(b"\n");
    }
    sha2::hex(&h.finalize()).to_string()
}

fn verify_against_toolchain(gcc: &Path, ada: &Path, prebuilt: &Path) {
    // GNAT rejects a `--RTS=` path that has no `adalib/`, before it looks at a
    // single line of Ada: `gnat1: RTS path not valid: missing adalib directory`.
    // Our RTS is a stub whose `adalib/` is legitimately *empty* -- we link no
    // Ada runtime -- and git cannot track an empty directory, so for as long as
    // this check has existed it deleted itself on every fresh checkout and the
    // failure was absorbed by the warning arm below. `adalib/.gitkeep` is what
    // keeps the directory in the tree; this assert is what makes its removal
    // say so, instead of quietly reverting to a build that verifies nothing.
    //
    // A hard error and not a warning, because the two failures below are not
    // the same kind of thing: this one is a defect in the *repository*, is
    // identical on every machine, and is fixed by a commit. The compile failure
    // below is a property of the developer's own toolchain install.
    assert!(
        ada.join("rts").join("adalib").is_dir(),
        "kernel/ada/rts/adalib/ is missing, so GNAT will reject --RTS and the \
         committed object cannot be verified against the compiler. It is meant \
         to be an empty directory held in git by adalib/.gitkeep -- restore it \
         with: git checkout -- kernel/ada/rts"
    );

    let out =
        PathBuf::from(std::env::var("OUT_DIR").unwrap_or_else(|_| ".".into())).join("adaverify");
    if std::fs::create_dir_all(&out).is_err() {
        return;
    }

    for unit in ADA_UNITS {
        let produced = out.join(format!("{unit}.o"));
        let status = Command::new(gcc)
            .arg("-c")
            .arg(ada.join("src").join(format!("{unit}.adb")))
            .arg("-o")
            .arg(&produced)
            .arg(format!("--RTS={}", ada.join("rts").display()))
            .args(ADA_FLAGS)
            .arg(format!("-gnateT={}", ada.join("x86_64-elf.atp").display()))
            .status();

        match status {
            Ok(s) if s.success() => {}
            // A toolchain that is present but broken is a property of this
            // machine, not of the tree, so it must not fail a build that every
            // other machine completes. Warn instead -- but do not claim the
            // check was made. What survives is only the stamp, and the stamp
            // answers a *different* question: it records what the object should
            // have been built from, never what it contains (see the comment at
            // the call site). A tampered or mis-generated object is invisible
            // to it, which is the entire reason this function exists. So the
            // warning has to say the verification was SKIPPED; a reader who
            // takes "the stamp still passed" as reassurance has been told the
            // opposite of the truth.
            _ => {
                println!(
                    "cargo:warning=Ada toolchain found but failed to compile {unit}. \
                     The committed object was NOT verified against the compiler this build; \
                     only the source stamp was checked, and the stamp cannot detect a \
                     modified object. Fix the toolchain, or run: python kernel/ada/regen-prebuilt.py --check"
                );
                continue;
            }
        }

        let (a, b) = (
            std::fs::read(&produced),
            std::fs::read(prebuilt.join(format!("{unit}.o"))),
        );
        if let (Ok(a), Ok(b)) = (a, b) {
            assert!(
                a == b,
                "kernel/ada/prebuilt/{unit}.o does not match what the Ada compiler \
                 produces from kernel/ada/src/{unit}.adb, even though the source stamp \
                 matches. The committed object has been modified independently of its \
                 source. Regenerate with: python kernel/ada/regen-prebuilt.py"
            );
        }
    }
}

/// Locate the cross GNAT. Explicit env var wins so a CI image can point at its
/// own install without matching our layout.
fn find_ada_gcc() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("SLATEOS_ADA_GCC") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    let exe = if cfg!(windows) {
        "x86_64-elf-gcc.exe"
    } else {
        "x86_64-elf-gcc"
    };

    // PATH.
    if let Ok(path) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path) {
            let c = dir.join(exe);
            if c.is_file() {
                return Some(c);
            }
        }
    }

    // Alire's toolchain cache, whose directory name carries a version and a
    // hash we deliberately do not hardcode -- glob for the crate prefix so a
    // toolchain upgrade does not silently stop being found.
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .ok()?;
    let cache = Path::new(&home).join("AppData/Local/alire/cache/toolchains");
    let cache = if cache.is_dir() {
        cache
    } else {
        Path::new(&home).join(".local/share/alire/toolchains")
    };
    let entries = std::fs::read_dir(cache).ok()?;
    for e in entries.flatten() {
        let name = e.file_name();
        if name.to_string_lossy().starts_with("gnat_x86_64_elf") {
            let c = e.path().join("bin").join(exe);
            if c.is_file() {
                return Some(c);
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// SHA-256 comes from the shared `sha2` crate.
//
// This file used to write one out, on the grounds that "the workspace has no
// `sha2` in its dependency graph today, and adding a crate (plus its
// `cfg-if`/`typenum`/`generic-array` tail) to every kernel build in order to
// hash four small files is a poor trade". Both halves of that have stopped
// being true: `sha2/` is now a workspace member, and it is a local path crate
// with *zero* dependencies -- the tail being avoided was RustCrypto's crates.io
// package, not this one. What is left is a build dependency on one no_std file.
//
// Adopting it also upgrades a check rather than merely removing code. The
// stamp this file computes is computed a second time by
// kernel/ada/regen-prebuilt.py using Python's `hashlib`, and the two are
// compared on every build: `stamp.txt` is written by the Python side and
// verified by this one. So from here on, every kernel build on every machine
// cross-checks `sha2` against a vetted reference implementation over real
// input. That is a stronger and more continuous guarantee than the
// four-vector unit test that used to live at the bottom of this file, which
// is why that test is not carried forward -- `sha2`'s own suite already covers
// all four FIPS 180-4 vectors plus every streaming split, and the stamp
// comparison covers the integration.
// ---------------------------------------------------------------------------

/// Paths of the service binaries the kernel embeds with `include_bytes!`.
///
/// Derived by scanning `kernel/src` rather than listed, so this cannot fall out
/// of step with the kernel the way a written-out list would. Mirrors the
/// derivation in `scripts/bootstrap-worktree.sh`, which has stayed correct
/// unattended while hand-counts of the same set disagreed four times.
///
/// Returns absolute paths, deduplicated and sorted so the emitted directives
/// are stable across builds.
fn embedded_service_artifacts(manifest: &str) -> Vec<String> {
    let src = Path::new(manifest).join("src");
    let mut found: Vec<String> = Vec::new();
    let mut stack = vec![src];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            for rest in text.split("include_bytes!(\"").skip(1) {
                let Some(rel) = rest.split('"').next() else {
                    continue;
                };
                if !rel.contains("/services/") {
                    continue;
                }
                // `include_bytes!` paths are relative to the including FILE.
                let Some(base) = path.parent() else { continue };
                let joined = base.join(rel);
                // Normalise `..` without touching the filesystem: the artifact
                // may not exist yet, which is the case being reported on.
                let mut parts: Vec<std::ffi::OsString> = Vec::new();
                for comp in joined.components() {
                    match comp {
                        std::path::Component::ParentDir => {
                            parts.pop();
                        }
                        std::path::Component::CurDir => {}
                        other => parts.push(other.as_os_str().to_os_string()),
                    }
                }
                let mut out = std::path::PathBuf::new();
                for p in parts {
                    out.push(p);
                }
                let s = out.to_string_lossy().replace('\\', "/");
                if !found.contains(&s) {
                    found.push(s);
                }
            }
        }
    }
    found.sort();
    found
}
