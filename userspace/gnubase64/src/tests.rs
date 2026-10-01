//! What the two GNU packages depend on: the decoder's handling of the cases a
//! clean-room decoder would get "right" and upstream does not. The packages'
//! own harnesses (`scripts/basenc-diff.sh`, `scripts/uu-diff.sh`) compare the
//! end results against the real programs.

#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use super::{Ctx, Out, decode_ctx, encode, is_base64, value};

/// Decode `lines`, one call each, through one context.
fn with_ctx(lines: &[&[u8]]) -> Vec<(bool, Vec<u8>)> {
    let mut ctx = Ctx::default();
    lines
        .iter()
        .map(|l| {
            let mut buf = Vec::new();
            let ok = decode_ctx(
                Some(&mut ctx),
                l,
                &mut Out {
                    buf: &mut buf,
                    left: 1 << 16,
                },
            );
            (ok, buf)
        })
        .collect()
}

/// Decode `input` with no context.
fn bare(input: &[u8]) -> (bool, Vec<u8>) {
    let mut buf = Vec::new();
    let ok = decode_ctx(
        None,
        input,
        &mut Out {
            buf: &mut buf,
            left: input.len(),
        },
    );
    (ok, buf)
}

#[test]
fn encoding_pads_with_equals() {
    assert_eq!(encode(b""), b"");
    assert_eq!(encode(b"f"), b"Zg==");
    assert_eq!(encode(b"fo"), b"Zm8=");
    assert_eq!(encode(b"foo"), b"Zm9v");
    assert_eq!(encode(b"foobar"), b"Zm9vYmFy");
    assert_eq!(encode(&[0xff, 0xfe]), b"//4=");
}

#[test]
fn the_alphabet() {
    assert_eq!(value(b'A'), Some(0));
    assert_eq!(value(b'/'), Some(63));
    assert!(!is_base64(b'='));
    assert!(!is_base64(b'\n'));
}

#[test]
fn lines_decode_and_newlines_are_skipped() {
    let r = with_ctx(&[b"Zm9v\n", b"YmFy\n", b"Zg==\n"]);
    assert_eq!(
        r,
        vec![
            (true, b"foo".to_vec()),
            (true, b"bar".to_vec()),
            (true, b"f".to_vec())
        ]
    );
}

#[test]
fn a_blank_line_decodes_to_nothing() {
    assert_eq!(with_ctx(&[b"\n"]), vec![(true, Vec::new())]);
}

#[test]
fn a_group_split_across_calls_is_carried() {
    let r = with_ctx(&[b"Zm\n", b"9v\n"]);
    assert_eq!(r, vec![(true, Vec::new()), (true, b"foo".to_vec())]);
}

#[test]
fn a_carriage_return_fails_the_next_call() {
    let r = with_ctx(&[b"Zm9v\r\n", b"YmFy\r\n"]);
    assert_eq!(r[0], (true, b"foo".to_vec()));
    assert!(!r[1].0);
}

#[test]
fn mid_line_padding_passes_through_the_context_but_not_without_one() {
    assert_eq!(with_ctx(&[b"Zg==Zg==\n"])[0], (true, b"ff".to_vec()));
    assert!(!bare(b"Zg==Zg==").0);
}

#[test]
fn a_bad_byte_after_the_last_whole_group_fails_the_next_call() {
    let r = with_ctx(&[b"Zm9v!\n", b"YmFy\n"]);
    assert_eq!(r[0], (true, b"foo".to_vec()));
    assert!(!r[1].0);
    assert!(!with_ctx(&[b"!Zm9v\n"])[0].0);
}

#[test]
fn a_failed_group_keeps_its_partial_bytes() {
    // "YWJj" is "abc"; "ZG" then '*' fails in the slow path after writing 'd'.
    let (ok, out) = bare(b"YWJjZG*");
    assert!(!ok);
    assert_eq!(out, b"abcd");
}

#[test]
fn without_a_context_the_input_must_be_whole_groups() {
    assert_eq!(bare(b"Zm9v"), (true, b"foo".to_vec()));
    assert_eq!(bare(b"Zg=="), (true, b"f".to_vec()));
    assert!(!bare(b"Zg").0);
    assert!(!bare(b"Zm9v\n").0);
}

#[test]
fn an_empty_call_flushes_what_is_carried() {
    let mut ctx = Ctx::default();
    let mut buf = Vec::new();
    let mut out = Out {
        buf: &mut buf,
        left: 16,
    };
    assert!(decode_ctx(Some(&mut ctx), b"Zg=", &mut out));
    assert_eq!(ctx.pending(), 3);
    // Three characters cannot be a group: the flush refuses them.
    assert!(!decode_ctx(Some(&mut ctx), b"", &mut out));
}

#[test]
fn output_past_the_room_is_dropped() {
    let mut buf = Vec::new();
    let ok = decode_ctx(
        None,
        b"Zm9vYmFy",
        &mut Out {
            buf: &mut buf,
            left: 4,
        },
    );
    assert!(ok);
    assert_eq!(buf, b"foob");
}
