#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use super::*;

const ANNEX_B: Flags = Flags {
    nalff: false,
    small_padding: false,
};

/// A unit's unescaped bytes, as many as it has.
fn bytes<'s>(s: &'s Splitter, packet: &'s [u8], nal: &Nal) -> &'s [u8] {
    &s.data(packet, nal)[..nal.size]
}

#[test]
fn start_codes_of_three_and_four_bytes() {
    // An SPS-type unit (0x67), then a slice (0x65) after a four-byte start
    // code, with a trailing zero byte before it that belongs to the first.
    let packet = [
        0, 0, 1, 0x67, 0x42, 0x80, //
        0, 0, 0, 1, 0x65, 0x88, 0x84, 0x80,
    ];
    let mut s = Splitter::default();
    s.split(&packet, 4, ANNEX_B).unwrap();
    let nals = s.nals().to_vec();
    assert_eq!(nals.len(), 2);
    assert_eq!((nals[0].kind, nals[0].ref_idc), (NAL_SPS, 3));
    // The zero before the next start code is the first unit's, and dropped
    // from its bit length with the stop bit: 0x80 is all stop.
    assert_eq!(bytes(&s, &packet, &nals[0]), [0x67, 0x42, 0x80, 0]);
    assert_eq!(nals[0].size_bits, 16);
    assert_eq!((nals[0].raw_start, nals[0].raw_size), (3, 4));
    assert_eq!((nals[1].kind, nals[1].ref_idc), (NAL_IDR_SLICE, 3));
    assert_eq!(bytes(&s, &packet, &nals[1]), [0x65, 0x88, 0x84, 0x80]);
    assert_eq!(nals[1].size_bits, 24);
    // The reader starts past the header byte.
    let mut gb = s.reader(&packet, &nals[1]).unwrap();
    assert_eq!(gb.count(), 8);
    assert_eq!(gb.bits(8), 0x88);
}

#[test]
fn the_stop_bit_and_what_follows_it_are_not_syntax() {
    for (last, bits) in [(0x80u8, 8), (0x01, 15), (0b1010_1000, 12), (0xFF, 15)] {
        let packet = [0, 0, 1, 0x06, last];
        let mut s = Splitter::default();
        s.split(&packet, 4, ANNEX_B).unwrap();
        assert_eq!(s.nals()[0].size_bits, bits, "last byte {last:#04x}");
    }
    // A unit of its header alone is eight bits: there is no stop bit to
    // take off one byte.
    let packet = [0, 0, 1, 0x09, 0, 0, 1, 0x09, 0x10];
    let mut s = Splitter::default();
    s.split(&packet, 4, ANNEX_B).unwrap();
    assert_eq!(s.nals()[0].size_bits, 8);
}

#[test]
fn escapes_are_removed_and_a_start_code_ends_a_unit() {
    let packet = [
        0, 0, 1, 0x01, 0xAA, 0, 0, 3, 0, 0, 0, 3, 1, 0xBB, 0, 0, 3, //
        0, 0, 1, 0x06, 0x80,
    ];
    let mut s = Splitter::default();
    s.split(&packet, 4, ANNEX_B).unwrap();
    let nals = s.nals().to_vec();
    assert_eq!(nals.len(), 2);
    assert_eq!(
        bytes(&s, &packet, &nals[0]),
        [0x01, 0xAA, 0, 0, 0, 0, 0, 1, 0xBB, 0, 0],
        "three escapes gone, the last at the very end"
    );
    assert_eq!(nals[0].raw_size, 14);
    // Its data in the buffer is followed by the next unit's, as FFmpeg's
    // layout leaves it: each written at the running sum of the bytes the
    // ones before took from the packet.
    let after = &s.data(&packet, &nals[0])[nals[0].size..];
    assert_eq!(&after[..3], [0, 0, 0], "three bytes the escapes freed");
    assert_eq!(&after[3..5], [0x06, 0x80], "then the next unit");

    // 00 00 02 ends a unit once an escape has been met, and what follows
    // it up to the next start code is no unit at all...
    let packet = [
        0, 0, 1, 0x01, 0, 0, 3, 0xAA, 0, 0, 2, 0xCC, 0, 0, 1, 0x06, 0x80,
    ];
    let mut s = Splitter::default();
    s.split(&packet, 4, ANNEX_B).unwrap();
    let nals = s.nals().to_vec();
    assert_eq!(nals.len(), 2);
    assert_eq!(bytes(&s, &packet, &nals[0]), [0x01, 0, 0, 0xAA]);
    assert_eq!(nals[0].raw_size, 5);
    assert_eq!(nals[1].kind, NAL_SEI);

    // ...but FFmpeg's first scan looks for 00 00 01 and 00 00 03 alone, so
    // before any escape it is copied like any other bytes.
    let packet = [0, 0, 1, 0x01, 0xAA, 0, 0, 2, 0xCC, 0, 0, 1, 0x06, 0x80];
    let mut s = Splitter::default();
    s.split(&packet, 4, ANNEX_B).unwrap();
    let nals = s.nals().to_vec();
    assert_eq!(nals.len(), 2);
    assert_eq!(bytes(&s, &packet, &nals[0]), [0x01, 0xAA, 0, 0, 2, 0xCC]);
    assert_eq!(nals[0].raw_size, 6);
}

#[test]
fn units_after_their_lengths() {
    for size in [1usize, 2, 4] {
        let mut packet = Vec::new();
        for unit in [&[0x67u8, 0x42, 0x80][..], &[0x68, 0xCE, 0x80]] {
            let len = unit.len() as u32;
            packet.extend_from_slice(&len.to_be_bytes()[4 - size..]);
            packet.extend_from_slice(unit);
        }
        let mut s = Splitter::default();
        s.split(
            &packet,
            size,
            Flags {
                nalff: true,
                small_padding: false,
            },
        )
        .unwrap();
        let kinds: Vec<u32> = s.nals().iter().map(|n| n.kind).collect();
        assert_eq!(kinds, [NAL_SPS, NAL_PPS], "length size {size}");
        assert_eq!(s.nals()[1].raw_start, 2 * size + 3);
    }
}

#[test]
fn a_length_past_the_packet_is_an_error_for_the_whole_packet() {
    let nalff = Flags {
        nalff: true,
        small_padding: false,
    };
    let mut s = Splitter::default();
    assert_eq!(
        s.split(&[0, 0, 0, 9, 0x67, 0x42, 0x80], 4, nalff),
        Err(Error::InvalidData)
    );
    assert_eq!(
        s.split(&[0, 0, 0, 0, 0x67, 0x42, 0x80], 4, nalff),
        Err(Error::InvalidData),
        "a zero length"
    );
    assert_eq!(
        s.split(&[0x80, 0, 0, 3, 0x67, 0x42, 0x80], 4, nalff),
        Err(Error::InvalidData),
        "a length FFmpeg's int reads as negative"
    );
}

#[test]
fn no_start_code_is_an_error_and_bytes_after_the_last_unit_are_not() {
    let mut s = Splitter::default();
    assert_eq!(
        s.split(&[1, 2, 3, 4, 5, 6], 4, ANNEX_B),
        Err(Error::InvalidData)
    );
    s.split(&[0, 0, 1, 0x09, 0x10, 0, 0, 1], 4, ANNEX_B)
        .unwrap();
    assert_eq!(s.nals().len(), 1);
    // Fewer than four bytes is nothing at all, and no error.
    s.split(&[0, 0, 1], 4, ANNEX_B).unwrap();
    assert!(s.nals().is_empty());
}

#[test]
fn damaged_and_empty_units_are_left_out() {
    // A unit whose forbidden bit is set, and one of zeros alone.
    let packet = [
        0, 0, 1, 0xE7, 0x42, 0x80, 0, 0, 1, 0, 0, 0, 0, 1, 0x09, 0x10,
    ];
    let mut s = Splitter::default();
    s.split(&packet, 4, ANNEX_B).unwrap();
    let kinds: Vec<u32> = s.nals().iter().map(|n| n.kind).collect();
    assert_eq!(kinds, [NAL_AUD]);
}

/// Past the end of a packet's last unit and its padding, FFmpeg's buffer
/// holds what an earlier, longer packet left there; a read that far finds
/// it.
#[test]
fn the_buffer_is_kept_from_packet_to_packet() {
    let mut long = vec![0, 0, 1, 0x01];
    long.extend(std::iter::repeat_n(0xAA, 300));
    let mut s = Splitter::default();
    s.split(&long, 4, ANNEX_B).unwrap();
    let short = [0, 0, 1, 0x01, 0x80];
    s.split(&short, 4, ANNEX_B).unwrap();
    let nal = s.nals()[0];
    let data = s.data(&short, &nal);
    assert_eq!(&data[..2], [0x01, 0x80]);
    assert!(
        data[2..2 + PADDING].iter().all(|&b| b == 0),
        "the unit's own padding"
    );
    assert_eq!(data[2 + PADDING], 0xAA, "the first packet's, past it");
}

#[test]
fn small_padding_reads_a_unit_with_nothing_to_unescape_in_place() {
    let small = Flags {
        nalff: false,
        small_padding: true,
    };
    let packet = [0, 0, 1, 0x67, 0x42, 0x80, 0, 0, 1, 0x68, 0, 0, 3, 1, 0x80];
    let mut s = Splitter::default();
    s.split(&packet, 4, small).unwrap();
    let nals = s.nals().to_vec();
    assert_eq!(nals[0].data, Place::Packet(3));
    assert_eq!(bytes(&s, &packet, &nals[0]), [0x67, 0x42, 0x80]);
    assert!(matches!(nals[1].data, Place::Rbsp(_)), "an escape: copied");
    assert_eq!(bytes(&s, &packet, &nals[1]), [0x68, 0, 0, 1, 0x80]);
}

/// A unit followed by 00 00 01 E0 keeps its trailing zero bytes as syntax,
/// as FFmpeg's commit 3566042a0 has it.
#[test]
fn a_unit_before_00_00_01_e0_keeps_its_trailing_zeros() {
    let packet = [0, 0, 1, 0x01, 0x80, 0, 0, 0, 1, 0xE0, 0, 0];
    let mut s = Splitter::default();
    s.split(&packet, 4, ANNEX_B).unwrap();
    let nal = s.nals()[0];
    assert_eq!(nal.size, 3, "the unit runs to the next start code's zero");
    assert_eq!(
        nal.size_bits, 24,
        "and keeps it: no stop bit in a zero byte"
    );
}
