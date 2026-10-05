//! minimp3's own test vectors -- the ISO/IEC 11172-4 and 13818-4
//! conformance streams among them, which reach what no encoder here writes:
//! Layer III intensity stereo, mixed blocks, every Layer I and II
//! allocation -- and its fuzzing finds, each decoded as the reference
//! decodes it.
//!
//! The vectors are fetched, not carried (their terms are the conformance
//! suites' own). Ignored unless asked for, with `MP3_VECTORS` naming a
//! directory that holds minimp3's `vectors/*.bit` (and `vectors/fuzz/*`)
//! and, beside each, `NAME.answer` from `tools/reference.c`:
//!
//! ```text
//! git clone https://github.com/lieff/minimp3   # ea99364f6, 2022-06-25
//! for f in minimp3/vectors/*.bit minimp3/vectors/fuzz/*; do
//!   b=$(basename "$f"); ./reference "$f" > "$DIR/${b%.*}.answer"; cp "$f" "$DIR/"
//! done
//! MP3_VECTORS=$DIR cargo test -p mp3 --test vectors -- --ignored
//! ```

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "a test: a panic is a failed test"
)]

mod common;

use std::path::PathBuf;

#[test]
#[ignore = "needs minimp3's vectors and their answers (MP3_VECTORS)"]
fn minimp3_vectors() {
    let dir = PathBuf::from(
        std::env::var_os("MP3_VECTORS").expect("MP3_VECTORS names the vectors' directory"),
    );
    let mut checked = 0usize;
    for entry in std::fs::read_dir(&dir).expect("MP3_VECTORS") {
        let answer = entry.expect("an entry").path();
        if answer.extension().is_none_or(|e| e != "answer") {
            continue;
        }
        let stem = answer.file_stem().expect("a name").to_owned();
        let stream = ["bit", "mp3"]
            .iter()
            .map(|ext| dir.join(&stem).with_extension(ext))
            .find(|p| p.exists())
            .unwrap_or_else(|| panic!("no stream for {}", answer.display()));
        common::check(&stream, &answer);
        checked += 1;
    }
    // minimp3 ea99364f6 has 83 vectors and one fuzzing find.
    assert!(checked >= 84, "only {checked} vectors in MP3_VECTORS");
}
