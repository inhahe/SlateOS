//! Every vector in `tests/data/fixed_buffer/` through [`zlib_inflate_into`] and
//! [`zlib_decompress_into`], against what zlib 1.3 and libdeflate 1.24
//! themselves answered for it (`generate.py` beside the data builds both from
//! pinned sources and records their answers).
//!
//! The vectors are the places where the two libraries disagree with each
//! other and with a plain decoder: hand-built streams for each rule in the
//! table in `src/fixed_buffer.rs`, mutations of real zlib output, and dynamic
//! blocks with long codewords that reach libdeflate's subtables and its
//! fastloop, at buffer sizes around where its match-copy overrun shows.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;

use deflate::{Error, Filled, ZlibStop, zlib_decompress_into, zlib_inflate_into};

fn fnv(p: &[u8]) -> u64 {
    let mut h: u64 = 1_469_598_103_934_665_603;
    for &b in p {
        h ^= u64::from(b);
        h = h.wrapping_mul(1_099_511_628_211);
    }
    h
}

/// This crate's answer, in `generate.py`'s oracle format.
fn zlib_line(input: &[u8], size: usize) -> String {
    let mut out = vec![0u8; size];
    match zlib_inflate_into(input, &mut out) {
        Ok(ZlibStop::Full) => format!("Z FULL {size} {:016x}", fnv(&out)),
        Ok(ZlibStop::Ended(n)) => format!("Z ENDED {n} {:016x}", fnv(&out[..n])),
        Err(_) => "Z ERR".to_owned(),
    }
}

fn libdeflate_line(input: &[u8], size: usize) -> String {
    let mut out = vec![0x00u8; size];
    let mut out_ff = vec![0xFFu8; size];
    let second = zlib_decompress_into(input, &mut out_ff);
    match zlib_decompress_into(input, &mut out) {
        Ok(Filled::Complete) => format!("D COMPLETE {size} {:016x}", fnv(&out)),
        Ok(Filled::Full(_)) => {
            assert!(
                matches!(second, Ok(Filled::Full(_))),
                "same input, same verdict"
            );
            format!("D FULL {:016x} {:016x}", fnv(&out), fnv(&out_ff))
        }
        Err(Error::ShortOutput { .. }) => "D SHORT".to_owned(),
        Err(_) => "D BAD".to_owned(),
    }
}

#[test]
fn every_vector_is_answered_as_zlib_and_libdeflate_answered_it() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/fixed_buffer");
    let blob = std::fs::read(dir.join("vectors.bin")).expect("vectors.bin");
    let expected = std::fs::read_to_string(dir.join("expected.txt")).expect("expected.txt");
    let names = std::fs::read_to_string(dir.join("vectors.txt")).expect("vectors.txt");
    let expected: Vec<&str> = expected.lines().filter(|l| !l.starts_with('#')).collect();
    let names: Vec<&str> = names.lines().collect();

    let mut vectors = Vec::new();
    let mut at = 0;
    while at < blob.len() {
        let size = u32::from_le_bytes(blob[at..at + 4].try_into().unwrap()) as usize;
        let len = u32::from_le_bytes(blob[at + 4..at + 8].try_into().unwrap()) as usize;
        vectors.push((size, &blob[at + 8..at + 8 + len]));
        at += 8 + len;
    }
    // A corpus that lost its vectors, or an answer file cut short, would
    // otherwise pass by comparing nothing.
    assert!(vectors.len() >= 1000, "only {} vectors", vectors.len());
    assert_eq!(expected.len(), 2 * vectors.len(), "two answers per vector");
    assert_eq!(names.len(), vectors.len(), "one name per vector");

    let mut failures = String::new();
    let mut disagree = 0;
    let mut outcomes: BTreeMap<String, usize> = BTreeMap::new();
    for (i, &(size, input)) in vectors.iter().enumerate() {
        for (got, want) in [
            (zlib_line(input, size), expected[2 * i]),
            (libdeflate_line(input, size), expected[2 * i + 1]),
        ] {
            let class: String = want.split(' ').take(2).collect::<Vec<_>>().join(" ");
            *outcomes.entry(class).or_default() += 1;
            if got != want {
                disagree += 1;
                if disagree <= 20 {
                    let _ = writeln!(
                        failures,
                        "#{i} {} (out {size}): want `{want}`, got `{got}`",
                        names[i]
                    );
                }
            }
        }
    }
    assert!(
        disagree == 0,
        "{disagree} answer(s) disagree with the libraries:\n{failures}"
    );
    // Every outcome each library can give is represented, so the corpus
    // cannot quietly stop testing one of them.
    for class in [
        "Z FULL",
        "Z ENDED",
        "Z ERR",
        "D COMPLETE",
        "D FULL",
        "D SHORT",
        "D BAD",
    ] {
        let n = outcomes.get(class).copied().unwrap_or(0);
        assert!(n >= 20, "only {n} vector(s) answered `{class}`");
    }
}
