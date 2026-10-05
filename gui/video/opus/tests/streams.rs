//! The decoder held to libopus, to the bit: every stream in `tests/data`
//! decoded at every output rate and channel count -- intact, with packets
//! lost, and damaged -- and with each decoder setting, and each output's
//! digest compared with that of libopus 1.5.2's fixed-point decoders', as
//! are the decode calls that failed and how
//! (`tests/data/references.txt`, which `tools/references.py` writes). So
//! is a stream of made-up packets, every TOC with random payloads.
//!
//! The streams are libopus's encoder's work on signals `tools/make_streams.c`
//! makes up: SILK, hybrid and CELT in each bandwidth and frame size, mono and
//! stereo, switching between them; with DTX, in-band FEC, CBR padding and
//! several frames a packet; surround and ambisonics.
//!
//! The RFC 8251 test vectors are held to `tests/data/rfc8251.txt` the same
//! way when `OPUS_VECTORS` names their directory -- they are the IETF's, so
//! not in this repository: <https://opus-codec.org/testvectors/>.
//!
//! `OPUS_REFERENCE` names a directory of libopus's outputs themselves
//! (`tools/references.py --keep`), for a decode that differs to say where.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    reason = "a test: a failure should be loud, and its sizes are small"
)]

mod common;

use common::{check_all, data_dir, references};
use std::path::PathBuf;

#[test]
fn every_stream_decodes_as_libopus_decodes_it() {
    let refs = references(&data_dir().join("references.txt"));
    let failures = check_all(&refs, &data_dir());
    for f in &failures {
        eprintln!("{f}");
    }
    assert!(
        failures.is_empty(),
        "{} of {} decodes differ from libopus",
        failures.len(),
        refs.len()
    );
}

#[test]
#[ignore = "needs OPUS_VECTORS, the directory of the RFC 8251 test vectors (testvectorNN.bit)"]
fn the_rfc_8251_vectors_decode_as_libopus_decodes_them() {
    let dir = PathBuf::from(std::env::var_os("OPUS_VECTORS").expect("OPUS_VECTORS"));
    let refs = references(&data_dir().join("rfc8251.txt"));
    let failures = check_all(&refs, &dir);
    for f in &failures {
        eprintln!("{f}");
    }
    assert!(
        failures.is_empty(),
        "{} of {} decodes differ from libopus",
        failures.len(),
        refs.len()
    );
}

/// Every stream in `tests/data` has references, and every reference a
/// stream: a stream added without them would be tested by nothing.
#[test]
fn every_stream_has_references() {
    let refs = references(&data_dir().join("references.txt"));
    let mut streams: Vec<String> = std::fs::read_dir(data_dir())
        .expect("tests/data")
        .filter_map(|e| {
            let name = e
                .expect("an entry")
                .file_name()
                .into_string()
                .expect("a UTF-8 name");
            name.strip_suffix(".bit").map(str::to_owned)
        })
        .collect();
    streams.sort();
    // `@random` is made, not read.
    let mut named: Vec<String> = refs
        .iter()
        .filter(|r| r.name != "@random")
        .map(|r| r.name.clone())
        .collect();
    named.sort();
    named.dedup();
    assert_eq!(streams, named);
    // And each single stream at all ten rate and channel counts, intact,
    // lossy and damaged; each multistream one at all five rates.
    for name in &named {
        let multistream = data_dir().join(format!("{name}.head")).exists();
        let mut have: Vec<(u32, usize, &str)> = refs
            .iter()
            .filter(|r| {
                &r.name == name && ["plain", "lossy", "damaged"].contains(&r.variant.as_str())
            })
            .map(|r| (r.rate, r.channels, r.variant.as_str()))
            .collect();
        have.sort_unstable();
        have.dedup();
        let expected = if multistream { 15 } else { 30 };
        assert_eq!(have.len(), expected, "{name}: {have:?}");
    }
    assert_eq!(
        refs.iter().filter(|r| r.name == "@random").count(),
        10,
        "@random at every rate and channel count"
    );
}
