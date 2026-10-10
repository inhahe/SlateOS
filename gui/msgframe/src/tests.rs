//! Tests for the frame and its fields: what is written is what is read, and
//! nothing malformed is read in part.

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use super::{Decoded, Protocol, TooLarge, Writer, decode};

const TEST: Protocol = Protocol {
    mark: b"TST",
    version: 3,
    max_frame: 64,
};

/// A message of kind 7: a byte, a u16, a u64, some bytes and some text.
fn written(text: &str) -> Vec<u8> {
    let mut w = Writer::new(TEST, 7);
    w.u8(9);
    w.u16(0xBEEF);
    w.u64(u64::MAX - 1);
    w.bytes(&[0, 255, 1], 8).unwrap();
    w.text(text, 8).unwrap();
    w.finish().unwrap()
}

/// Reads `written`'s fields back.
fn read(bytes: &[u8]) -> Decoded<(u8, u16, u64, Vec<u8>, String)> {
    decode(bytes, TEST, 7, |r| {
        Some((
            r.u8()?,
            r.u16()?,
            r.u64()?,
            r.bytes(8)?.to_vec(),
            r.text(8)?,
        ))
    })
}

/// **What is written is what is read**, and the frame is read off the front
/// of what follows it.
#[test]
fn what_is_written_is_what_is_read() {
    let frame = written("héllo");
    let fields = (9, 0xBEEF, u64::MAX - 1, vec![0, 255, 1], "héllo".to_owned());
    assert_eq!(read(&frame), Decoded::Complete(fields.clone(), frame.len()));
    let mut two = frame.clone();
    two.extend_from_slice(&written("x"));
    assert_eq!(read(&two), Decoded::Complete(fields, frame.len()));
}

/// **Part of a frame is not yet a message**, at every cut.
#[test]
fn part_of_a_frame_is_not_yet_a_message() {
    let frame = written("abc");
    for cut in 0..frame.len() {
        assert_eq!(read(&frame[..cut]), Decoded::Partial, "{cut}");
    }
}

/// **What is not the protocol's message is malformed**: a wrong mark,
/// version or kind; a field past its bound or text that is not UTF-8;
/// fields left unread; a frame longer than the protocol reads, refused on
/// its length alone.
#[test]
fn what_is_not_the_message_is_malformed() {
    let frame = written("abc");
    let with = |at: usize, value: u8| {
        let mut f = frame.clone();
        f[4 + at] = value;
        f
    };
    assert_eq!(read(&with(0, b'X')), Decoded::Malformed);
    assert_eq!(read(&with(3, 4)), Decoded::Malformed);
    assert_eq!(read(&with(4, 8)), Decoded::Malformed);
    // The text's first byte, made a lone continuation byte.
    let text_at = 3 + 1 + 1 + 1 + 2 + 8 + 2 + 3 + 2;
    assert_eq!(read(&with(text_at, 0x80)), Decoded::Malformed);
    // Read with a tighter bound than it was written to.
    let tight = decode(&frame, TEST, 7, |r| {
        r.u8()?;
        r.u16()?;
        r.u64()?;
        r.bytes(2).map(<[u8]>::to_vec)
    });
    assert_eq!(tight, Decoded::Malformed);
    // Fields left unread.
    assert_eq!(decode(&frame, TEST, 7, |r| r.u8()), Decoded::Malformed);
    let too_long = u32::try_from(TEST.max_frame + 1).unwrap().to_le_bytes();
    assert_eq!(read(&too_long), Decoded::Malformed);
}

/// **What would break a bound is not written**: a field past its bound, a
/// message past the protocol's frame.
#[test]
fn what_would_break_a_bound_is_not_written() {
    let mut w = Writer::new(TEST, 7);
    assert_eq!(w.bytes(&[0; 9], 8), Err(TooLarge));
    assert_eq!(w.text("123456789", 8), Err(TooLarge));
    let mut big = Writer::new(TEST, 7);
    big.bytes(&[0; 60], 60).unwrap();
    assert_eq!(big.finish(), Err(TooLarge));
}

/// **A writer given room writes its message without growing**: the frame
/// comes back in the buffer reserved for it -- header, length and all -- and
/// reads as any other.
#[test]
fn a_writer_given_room_does_not_grow() {
    let mut w = Writer::with_capacity(TEST, 7, 40);
    w.bytes(&[7; 30], 30).unwrap();
    let frame = w.finish().unwrap();
    // Header (length, mark, version, kind) and the room asked for.
    assert!(frame.capacity() >= 4 + 3 + 2 + 40, "{}", frame.capacity());
    assert_eq!(
        decode(&frame, TEST, 7, |r| r.bytes(30).map(<[u8]>::to_vec)),
        Decoded::Complete(vec![7; 30], frame.len())
    );
}
