//! [`PiecewiseInflater`]: one zlib stream in flushed pieces, as VNC's ZRLE
//! sends it -- each piece decoded as it comes, reaching back into the ones
//! before it.
//!
//! The vectors are real zlib 1.3 output (Python's `zlib`, level 9): a
//! compressor fed two texts, `Z_SYNC_FLUSH` after each, then `Z_FINISH`.
//! The second piece is eleven bytes for twenty-four of text because it
//! refers back into the first -- which is the whole point of the type.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]

use deflate::{Error, PiecewiseInflater};

/// `b"hello hello hello world "`, with the zlib header and a sync flush.
const P1: [u8; 23] = [
    0x78, 0xda, 0xca, 0x48, 0xcd, 0xc9, 0xc9, 0x57, 0xc8, 0x40, 0x22, 0xcb, 0xf3, 0x8b, 0x72, 0x52,
    0x14, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff,
];
/// `b"hello world, hello world"`, back-referencing P1, and a sync flush.
const P2: [u8; 11] = [
    0x42, 0x62, 0xeb, 0x20, 0x73, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff,
];
/// The final (empty) block and the Adler-32 of everything.
const P3: [u8; 6] = [0x03, 0x00, 0xb7, 0x51, 0x11, 0xe9];

#[test]
fn each_piece_decodes_as_it_arrives_reaching_into_the_last() {
    let mut z = PiecewiseInflater::zlib();
    assert_eq!(
        z.inflate_piece(&P1, 1 << 20).unwrap(),
        b"hello hello hello world "
    );
    assert!(!z.finished());
    assert_eq!(
        z.inflate_piece(&P2, 1 << 20).unwrap(),
        b"hello world, hello world",
        "the second piece's back-references did not reach into the first"
    );
    assert_eq!(z.inflate_piece(&P3, 1 << 20).unwrap(), b"");
    assert!(z.finished(), "the final block was not seen");
}

#[test]
fn a_piece_alone_cannot_be_decoded_without_its_history() {
    // P2 on its own is a raw stream whose distances reach before its start.
    let mut raw = PiecewiseInflater::raw();
    assert!(
        raw.inflate_piece(&P2, 1 << 20).is_err(),
        "decoded without history"
    );
}

#[test]
fn a_wrong_trailer_is_a_checksum_mismatch() {
    let mut z = PiecewiseInflater::zlib();
    z.inflate_piece(&P1, 1 << 20).unwrap();
    z.inflate_piece(&P2, 1 << 20).unwrap();
    let mut bad = P3;
    bad[5] ^= 1;
    assert!(matches!(
        z.inflate_piece(&bad, 1 << 20),
        Err(Error::ChecksumMismatch { .. })
    ));
}

#[test]
fn a_piece_cut_inside_a_block_breaks_the_stream_for_good() {
    let mut z = PiecewiseInflater::zlib();
    assert_eq!(
        z.inflate_piece(&P1[..10], 1 << 20),
        Err(Error::UnexpectedEnd)
    );
    assert_eq!(
        z.inflate_piece(&P2, 1 << 20),
        Err(Error::UnexpectedEnd),
        "a stream that lost its place was carried on"
    );
}

#[test]
fn nothing_follows_the_final_block() {
    let mut z = PiecewiseInflater::zlib();
    for piece in [&P1[..], &P2[..], &P3[..]] {
        z.inflate_piece(piece, 1 << 20).unwrap();
    }
    assert_eq!(z.inflate_piece(&P2, 1 << 20), Err(Error::UnexpectedEnd));
}

#[test]
fn a_bad_header_is_refused() {
    let mut z = PiecewiseInflater::zlib();
    let mut bad = P1;
    bad[1] ^= 1; // the header check no longer divides by 31
    assert_eq!(z.inflate_piece(&bad, 1 << 20), Err(Error::BadWrapperHeader));
}

#[test]
fn the_limit_is_per_piece() {
    let mut z = PiecewiseInflater::zlib();
    assert_eq!(z.inflate_piece(&P1, 5), Err(Error::OutputTooLarge));
    let mut z = PiecewiseInflater::zlib();
    z.inflate_piece(&P1, 24).unwrap();
    z.inflate_piece(&P2, 24)
        .expect("a second piece counted the first against its limit");
}
