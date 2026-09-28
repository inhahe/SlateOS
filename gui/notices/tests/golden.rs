//! The bundle `scripts/gather-notices.py` writes, read back.
//!
//! `tests/fixtures/bundle` is the gatherer's output for the fixture tree its
//! self-test builds, committed. `scripts/test-gather-notices.py` fails if the
//! gatherer stops writing exactly these bytes, and these tests fail if this
//! reader stops reading them -- so a change to the format on either side fails
//! a test on that side, instead of reaching an image whose notices screen is
//! empty.

// A test that cannot read its fixture has nothing else useful to do than stop,
// and names the entry it indexes in its own assertions.
#![allow(clippy::expect_used, clippy::indexing_slicing)]

use std::path::{Path, PathBuf};

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("bundle")
}

fn golden() -> Vec<notices::Notice> {
    notices::load(&golden_dir()).expect("the golden bundle loads")
}

#[test]
fn the_golden_bundle_reads_back_as_the_gatherer_wrote_it() {
    let found: Vec<(String, String, String, String)> = golden()
        .iter()
        .map(|n| {
            (
                n.key.clone(),
                n.title(),
                n.licence.clone(),
                n.origin.clone(),
            )
        })
        .collect();
    let want = [
        (
            "cfg-if-1.0.5",
            "cfg-if 1.0.5",
            "MIT OR Apache-2.0",
            "vendor/cfg-if",
        ),
        (
            "libjpeg-turbo-3.1.1",
            "libjpeg-turbo 3.1.1",
            "IJG AND BSD-3-Clause AND Zlib",
            "gui/codec",
        ),
        ("spin-0.9.8", "spin 0.9.8", "MIT", "crates.io"),
    ]
    .map(|(a, b, c, d)| (a.to_owned(), b.to_owned(), c.to_owned(), d.to_owned()));
    assert_eq!(found, want);
}

/// The IJG licence asks for this sentence in so many words; a notices screen
/// that dropped it would be the one failure the field exists to prevent.
#[test]
fn a_required_sentence_survives_the_round_trip_word_for_word() {
    let notices = golden();
    assert_eq!(
        notices[1].attribution.as_deref(),
        Some("This software is based in part on the work of the Independent JPEG Group.")
    );
    assert_eq!(notices[0].attribution, None);
}

#[test]
fn every_text_reads_back_byte_for_byte() {
    let notices = golden();
    let text = |i: usize| {
        let t = &notices[i].texts[0];
        (t.name.clone(), t.read().expect("the golden text is there"))
    };
    assert_eq!(
        text(1),
        (
            "LICENSE.md".to_owned(),
            b"IJG text \xc2\xa9 1991\nno final newline".to_vec()
        )
    );
    assert_eq!(text(0), ("LICENSE-MIT".to_owned(), b"MIT text\n".to_vec()));
    assert_eq!(text(2), ("LICENSE".to_owned(), b"spin MIT text\n".to_vec()));
}

/// Everything in one file, for a person with no screen: each notice's
/// heading, its licence and its required sentence, then its texts.
#[test]
fn the_readable_file_carries_every_notice() {
    let readable = std::fs::read(golden_dir().join("NOTICES.txt")).expect("NOTICES.txt");
    let contains = |needle: &[u8]| readable.windows(needle.len()).any(|w| w == needle);
    for needle in [
        &b"cfg-if 1.0.5\nLicence: MIT OR Apache-2.0\n"[..],
        b"libjpeg-turbo 3.1.1\nLicence: IJG AND BSD-3-Clause AND Zlib\nThis software is based in part",
        b"spin 0.9.8\nLicence: MIT\n",
        b"IJG text \xc2\xa9 1991\nno final newline\n",
    ] {
        assert!(contains(needle), "NOTICES.txt lacks {}", needle.escape_ascii());
    }
}
