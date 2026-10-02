//! Compile file 5.45's magic database into the program.
//!
//! Upstream installs its database compiled -- `magic.mgc`, 22,642 rules of
//! 376 bytes -- and maps it at every start. This makes the same file at build
//! time, with the library the program runs (`libmagic::apprentice`, which
//! `file -C` uses and `scripts/file-diff.sh` holds byte for byte to upstream's
//! compiler), packs its runs of zeros, and leaves it in
//! `OUT_DIR/magic.mgc.packed` for `src/main.rs` to carry. The program maps it
//! where upstream maps the file, and so starts without reading 1.6 MB of magic
//! text first.
//!
//! The text, `magic/`, is vendored from the release by
//! `scripts/file-magic-vendor.py`. A rule it cannot compile fails the build,
//! with the message `file -C` would print.

use std::io::Write;
use std::path::PathBuf;

fn main() {
    sysroot_dep::emit();
    println!("cargo:rerun-if-changed=magic");
    let Some(dir) = std::env::var_os("CARGO_MANIFEST_DIR").map(|d| PathBuf::from(d).join("magic")) else {
        fail(b"CARGO_MANIFEST_DIR is not set");
    };
    let Some(out) = std::env::var_os("OUT_DIR").map(|d| PathBuf::from(d).join("magic.mgc.packed")) else {
        fail(b"OUT_DIR is not set");
    };
    let path = libmagic::apprentice::os_bytes(dir.as_os_str());
    let packed = match libmagic::apprentice::compile_packed(&path) {
        Ok(p) => p,
        Err(why) => {
            let mut msg = b"cannot compile ".to_vec();
            msg.extend_from_slice(&path);
            msg.extend_from_slice(b": ");
            msg.extend_from_slice(&why);
            fail(&msg);
        }
    };
    if let Err(e) = std::fs::write(&out, packed) {
        fail(format!("cannot write {}: {e}", out.display()).as_bytes());
    }
}

/// Stop the build, saying why -- the message's bytes as they are, since a
/// magic file's name or a rule's text need not be UTF-8.
fn fail(why: &[u8]) -> ! {
    let mut w = b"file build: ".to_vec();
    w.extend_from_slice(why);
    w.push(b'\n');
    // Nothing more can be done if stderr cannot be written.
    let _written = std::io::stderr().write_all(&w);
    std::process::exit(1);
}
