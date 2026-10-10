//! What the reference tests share: the crate's decoding of a file in the
//! lines `tools/reference.c` prints of minimp3's, and the comparison.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::missing_panics_doc,
    reason = "test support: a panic is a failed test"
)]

use std::fmt::Write as _;
use std::path::Path;

use mp3::{Decoder, MAX_SAMPLES_PER_FRAME};

/// FNV-1a, 64-bit, over the samples' bytes, little-endian: the reference's
/// hash.
#[must_use]
pub fn fnv(samples: &[i16]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for v in samples {
        for b in v.to_le_bytes() {
            hash = (hash ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
        }
    }
    hash
}

/// The lines `tools/reference.c` prints for `data`, from this crate: one a
/// `decode_frame` call, each given the rest of the input.
#[must_use]
pub fn lines(data: &[u8]) -> Vec<String> {
    let mut dec = Decoder::new();
    let mut pcm = [0i16; MAX_SAMPLES_PER_FRAME];
    let mut out = Vec::new();
    let mut pos = 0usize;
    loop {
        let (samples, info) = dec.decode_frame(&data[pos..], Some(&mut pcm));
        if info.frame_bytes == 0 {
            out.push(format!("end {pos}"));
            return out;
        }
        let mut line = String::new();
        write!(
            line,
            "frame {pos} {} {samples} {} {} {} {} {:016x}",
            info.frame_bytes,
            info.channels,
            info.hz,
            info.layer,
            info.bitrate_kbps,
            fnv(&pcm[..samples * info.channels])
        )
        .unwrap();
        out.push(line);
        pos += info.frame_bytes;
    }
}

/// Holds the decoding of `stream` to the answer in `answer`, line for line;
/// the first difference fails the test, named.
#[allow(
    dead_code,
    reason = "fixtures.rs and vectors.rs check against answers; api.rs, which shares the module, does not"
)]
pub fn check(stream: &Path, answer: &Path) {
    let data = std::fs::read(stream).unwrap_or_else(|e| panic!("{}: {e}", stream.display()));
    let expected =
        std::fs::read_to_string(answer).unwrap_or_else(|e| panic!("{}: {e}", answer.display()));
    let expected: Vec<&str> = expected.lines().collect();
    let got = lines(&data);
    for (k, (g, e)) in got.iter().zip(&expected).enumerate() {
        assert_eq!(
            g,
            e,
            "{}: line {} differs from minimp3's",
            stream.display(),
            k + 1
        );
    }
    assert_eq!(
        got.len(),
        expected.len(),
        "{}: {} lines, minimp3 has {}",
        stream.display(),
        got.len(),
        expected.len()
    );
}
