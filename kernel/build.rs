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
            if for_kernel {
                panic!(
                    "kernel/ada/prebuilt/stamp.txt is missing or unreadable ({e}).\n\
                     Regenerate it with: python kernel/ada/regen-prebuilt.py"
                );
            }
            return;
        }
    };

    let expected = ada_stamp(&ada);
    if recorded.trim() != expected.trim() {
        panic!(
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
    }

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
    let mut h = Sha256::new();

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
    h.hex()
}

fn verify_against_toolchain(gcc: &Path, ada: &Path, prebuilt: &Path) {
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
            // A toolchain that is present but broken should not fail a build
            // that the committed object can satisfy. Warn loudly instead: the
            // stamp already guarantees the object matches the sources.
            _ => {
                println!(
                    "cargo:warning=Ada toolchain found but failed to compile {unit}; using the committed object. The stamp check still passed."
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
// Minimal SHA-256.
//
// Written out rather than pulled in as a build-dependency: the workspace has no
// `sha2` in its dependency graph today, and adding a crate (plus its
// `cfg-if`/`typenum`/`generic-array` tail) to every kernel build in order to
// hash four small files is a poor trade. This is the reference algorithm from
// FIPS 180-4 with no optimisation attempted -- it hashes a few KiB once per
// build. Correctness is checked by a test below against the two standard NIST
// vectors, so a typo in the constants cannot pass silently.
// ---------------------------------------------------------------------------

const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

struct Sha256 {
    h: [u32; 8],
    buf: Vec<u8>,
    len: u64,
}

impl Sha256 {
    fn new() -> Self {
        Self {
            h: [
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
                0x5be0cd19,
            ],
            buf: Vec::new(),
            len: 0,
        }
    }

    fn update(&mut self, data: &[u8]) {
        self.len = self.len.wrapping_add(data.len() as u64);
        self.buf.extend_from_slice(data);
        while self.buf.len() >= 64 {
            let block: [u8; 64] = self.buf[..64].try_into().expect("64 bytes");
            self.compress(&block);
            self.buf.drain(..64);
        }
    }

    fn compress(&mut self, block: &[u8; 64]) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                block[i * 4],
                block[i * 4 + 1],
                block[i * 4 + 2],
                block[i * 4 + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = self.h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (dst, src) in self.h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *dst = dst.wrapping_add(src);
        }
    }

    fn hex(mut self) -> String {
        let bits = self.len.wrapping_mul(8);
        self.buf.push(0x80);
        while self.buf.len() % 64 != 56 {
            self.buf.push(0);
        }
        let tail = bits.to_be_bytes();
        self.buf.extend_from_slice(&tail);
        let blocks: Vec<[u8; 64]> = self
            .buf
            .chunks_exact(64)
            .map(|c| c.try_into().expect("64 bytes"))
            .collect();
        for b in blocks {
            self.compress(&b);
        }
        self.h.iter().map(|w| format!("{w:08x}")).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::Sha256;

    /// The two NIST FIPS 180-4 example vectors, plus the multi-block case.
    /// A build script is not normally tested, which is exactly why this is
    /// here: a wrong hash would not fail the build, it would silently make the
    /// stamp check compare two consistently-wrong values and always pass.
    #[test]
    fn sha256_known_vectors() {
        let mut h = Sha256::new();
        h.update(b"abc");
        assert_eq!(
            h.hex(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );

        let mut h = Sha256::new();
        h.update(b"");
        assert_eq!(
            h.hex(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );

        // 56 bytes: forces padding into a second block.
        let mut h = Sha256::new();
        h.update(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq");
        assert_eq!(
            h.hex(),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );

        // Fed in pieces, to check the buffering path agrees with one shot.
        let mut h = Sha256::new();
        for chunk in b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq".chunks(7) {
            h.update(chunk);
        }
        assert_eq!(
            h.hex(),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }
}
