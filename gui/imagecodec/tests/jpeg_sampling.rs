//! Every way a JPEG lays out its colour, against libjpeg-turbo.
//!
//! Most JPEGs store colour at a lower resolution than brightness, and how it
//! is brought back up is left to the decoder: the format does not say. What
//! "right" means in practice is what libjpeg-turbo does -- it is what nearly
//! every browser, image library and desktop decodes with, so it is how anyone
//! has ever seen the picture -- and each fixture here is compared against
//! Pillow's decode of it (libjpeg-turbo underneath), to within the rounding
//! two decoders' inverse DCTs and colour conversions may differ by.
//!
//! | fixture | colour, against brightness | what libjpeg does with it |
//! |---|---|---|
//! | `jpeg444` | the same | nothing |
//! | `jpeg422` | half across | `h2v1_fancy_upsample` |
//! | `jpeg440` | half down | `h1v2_fancy_upsample` |
//! | `jpeg420`, `jpeg420r`, `jpegbig420` | half both ways | `h2v2_fancy_upsample` |
//! | `jpeg411` | a quarter across | repeats each sample |
//! | `jpegnarrow420`, `jpegnarrow422` | two samples across | repeats them: too narrow to filter |
//! | `jpegslim420` | three samples across | the narrowest plane it filters |
//! | `jpeggrey` | none | nothing |
//!
//! Written by `tests/data/generate_jpeg.py`, which says where each came from.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss
)]

mod common;

use common::{answer, assert_agrees, read};
use imagecodec::{Limits, decode};

/// Each fixture, and the answer file holding Pillow's decode of it.
const FIXTURES: &[(&str, &str)] = &[
    // The baseline twins of the progressive pairs. libjpeg decodes each twin
    // to the same pixels -- the generator checks it -- so one answer serves
    // both.
    ("jpeg444_baseline", "jpeg444_progressive"),
    ("jpeg422_baseline", "jpeg422_progressive"),
    ("jpeg420_baseline", "jpeg420_progressive"),
    ("jpeg420r_baseline", "jpeg420r_progressive"),
    ("jpegbig420_baseline", "jpegbig420_progressive"),
    ("jpeggrey_baseline", "jpeggrey_progressive"),
    // Layouts Pillow cannot write, written by TurboJPEG.
    ("jpeg440", "jpeg440"),
    ("jpeg411", "jpeg411"),
    // The edge of the filter.
    ("jpegnarrow420", "jpegnarrow420"),
    ("jpegnarrow422", "jpegnarrow422"),
    ("jpegslim420", "jpegslim420"),
];

#[test]
fn every_colour_layout_decodes_as_libjpeg_decodes_it() {
    for &(file, reference) in FIXTURES {
        let image =
            decode(&read(file), Limits::default()).unwrap_or_else(|e| panic!("{file}: {e}"));
        assert_agrees(file, &image, &answer(reference));
    }
}
