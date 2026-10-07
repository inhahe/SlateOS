//! What ffprobe cannot answer: files built here page by page, where the crate
//! differs from FFmpeg by design (a lost page, an empty Opus packet, a
//! stream without its tags) or where FFmpeg's answer is no answer (a file
//! cut short, junk between pages); and the facts a packet list does not
//! show (a stream's duration, the probe, headers given again in band).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    reason = "a test: a failure should be loud, and its numbers are small"
)]

use std::fs::File;
use std::io::Cursor;

use ogg::{Codec, Demuxer, Packet};

const CONTINUED: u8 = 1;
const FIRST: u8 = 2;
const LAST: u8 = 4;

fn data(name: &str) -> String {
    format!("{}/tests/data/{name}", env!("CARGO_MANIFEST_DIR"))
}

fn crc(page: &[u8]) -> u32 {
    let mut c = 0u32;
    for (i, &b) in page.iter().enumerate() {
        c ^= u32::from(if (22..26).contains(&i) { 0 } else { b }) << 24;
        for _ in 0..8 {
            c = if c & 0x8000_0000 != 0 {
                (c << 1) ^ 0x04c1_1db7
            } else {
                c << 1
            };
        }
    }
    c
}

/// A page of the given lacing values and body.
fn page(
    flags: u8,
    granule: i64,
    serial: u32,
    sequence: u32,
    lacing: &[u8],
    body: &[u8],
) -> Vec<u8> {
    assert_eq!(
        lacing.iter().map(|&l| usize::from(l)).sum::<usize>(),
        body.len()
    );
    let mut p = b"OggS\0".to_vec();
    p.push(flags);
    p.extend_from_slice(&granule.to_le_bytes());
    p.extend_from_slice(&serial.to_le_bytes());
    p.extend_from_slice(&sequence.to_le_bytes());
    p.extend_from_slice(&[0; 4]);
    p.push(lacing.len() as u8);
    p.extend_from_slice(lacing);
    p.extend_from_slice(body);
    let c = crc(&p);
    p[22..26].copy_from_slice(&c.to_le_bytes());
    p
}

/// A page of whole packets.
fn packets_page(flags: u8, granule: i64, serial: u32, sequence: u32, packets: &[&[u8]]) -> Vec<u8> {
    let mut lacing = Vec::new();
    for p in packets {
        lacing.extend(std::iter::repeat_n(255u8, p.len() / 255));
        lacing.push((p.len() % 255) as u8);
    }
    page(flags, granule, serial, sequence, &lacing, &packets.concat())
}

/// An `OpusHead` of two channels and the given pre-skip.
fn opus_head(pre_skip: u16) -> Vec<u8> {
    let mut h = b"OpusHead\x01\x02".to_vec();
    h.extend_from_slice(&pre_skip.to_le_bytes());
    h.extend_from_slice(&48_000u32.to_le_bytes());
    h.extend_from_slice(&[0, 0, 0]);
    h
}

/// A 20 ms CELT packet (TOC 0xf8: config 31, one frame), `n` bytes long.
fn celt(n: usize, fill: u8) -> Vec<u8> {
    let mut p = vec![fill; n];
    p[0] = 0xf8;
    p
}

fn read(file: Vec<u8>) -> Vec<Packet> {
    let mut d = Demuxer::open(Cursor::new(file)).unwrap();
    let mut out = Vec::new();
    while let Some(p) = d.next_packet().unwrap() {
        out.push(p);
    }
    out
}

/// An Opus stream's two header pages.
fn opus_headers(serial: u32) -> Vec<u8> {
    let mut f = packets_page(FIRST, 0, serial, 0, &[&opus_head(312)]);
    f.extend(packets_page(
        0,
        0,
        serial,
        1,
        &[b"OpusTags\0\0\0\0\0\0\0\0"],
    ));
    f
}

/// Pages 2 to 5 of a stream: A whole, B begun; B ended and C; D; E, last.
fn four_pages(serial: u32) -> [Vec<u8>; 4] {
    let a = celt(40, 1);
    let b = celt(300, 2);
    let c = celt(50, 3);
    let (d, e) = (celt(60, 4), celt(70, 5));
    // A, then B's first 255 bytes, running on.
    let p2 = page(
        0,
        312 + 960,
        serial,
        2,
        &[40, 255],
        &[a.as_slice(), &b[..255]].concat(),
    );
    let p3 = page(
        CONTINUED,
        312 + 3 * 960,
        serial,
        3,
        &[45, 50],
        &[&b[255..], c.as_slice()].concat(),
    );
    let p4 = packets_page(0, 312 + 4 * 960, serial, 4, &[&d]);
    let p5 = packets_page(LAST, 312 + 5 * 960 - 100, serial, 5, &[&e]);
    [p2, p3, p4, p5]
}

fn sizes(packets: &[Packet]) -> Vec<usize> {
    packets.iter().map(|p| p.data.len()).collect()
}

#[test]
fn packets_run_across_pages_and_are_timed_from_their_pages() {
    let mut f = opus_headers(7);
    for p in four_pages(7) {
        f.extend(p);
    }
    let got = read(f);
    assert_eq!(sizes(&got), vec![40, 300, 50, 60, 70]);
    let pts: Vec<Option<i64>> = got.iter().map(|p| p.pts).collect();
    // The first page's: its granule less its one packet; then each page's.
    assert_eq!(
        pts,
        vec![Some(0), Some(960), Some(1920), Some(2880), Some(3840)]
    );
    assert_eq!(got[0].skip_samples, 312);
    // The last page ends 100 samples into E: the rest is its padding.
    assert_eq!((got[4].duration, got[4].discard_padding), (100 + 760, 100));
}

#[test]
fn a_lost_page_loses_the_packet_it_cut() {
    // Without page 3, B's end and C are gone: B, begun on page 2, is lost
    // with them (FFmpeg, reading no page numbers, would join B's start to
    // page 4's D, and time it from page 2). D is timed back from its own
    // page's granule: page 2's no longer says where it starts.
    let mut f = opus_headers(7);
    let [p2, _p3, p4, p5] = four_pages(7);
    f.extend(p2);
    f.extend(p4);
    f.extend(p5);
    let got = read(f);
    assert_eq!(sizes(&got), vec![40, 60, 70]);
    assert_eq!(got[1].pts, Some(312 + 4 * 960 - 312 - 960));
}

#[test]
fn a_damaged_page_is_a_lost_one() {
    let mut f = opus_headers(7);
    let [p2, mut p3, p4, p5] = four_pages(7);
    p3[40] ^= 0x20;
    for p in [p2, p3, p4, p5] {
        f.extend(p);
    }
    assert_eq!(sizes(&read(f)), vec![40, 60, 70]);
}

#[test]
fn junk_before_and_between_pages_is_passed_over() {
    let mut f = b"not a page OggS either".to_vec();
    let start = f.len();
    f.extend(opus_headers(7));
    for p in four_pages(7) {
        f.extend(p);
        f.extend_from_slice(b"OggSjunk");
    }
    let got = read(f);
    assert_eq!(sizes(&got), vec![40, 300, 50, 60, 70]);
    assert!(got[0].page_position > start as u64);
}

#[test]
fn a_file_cut_short_gives_what_it_holds() {
    let mut f = opus_headers(7);
    for p in four_pages(7) {
        f.extend(p);
    }
    let cut = f.len() - 30;
    f.truncate(cut);
    // The last page is gone, and with it E.
    assert_eq!(sizes(&read(f)), vec![40, 300, 50, 60]);
}

#[test]
fn an_empty_opus_packet_is_a_packet_and_a_damaged_one_is_flagged() {
    // An empty packet says one was lost (RFC 7845); a code-3 packet without
    // its count byte has no length. FFmpeg stops reading at either.
    let mut f = opus_headers(7);
    f.extend(packets_page(
        0,
        312 + 2 * 960,
        7,
        2,
        &[&celt(30, 1), &[], &[0xfb], &celt(30, 2)],
    ));
    f.extend(packets_page(LAST, 312 + 3 * 960, 7, 3, &[&celt(30, 3)]));
    let got = read(f);
    assert_eq!(sizes(&got), vec![30, 0, 1, 30, 30]);
    let facts: Vec<(Option<i64>, u64, bool)> =
        got.iter().map(|p| (p.pts, p.duration, p.corrupt)).collect();
    assert_eq!(
        facts,
        vec![
            (Some(0), 960, false),
            (Some(960), 0, false),
            (Some(960), 0, true),
            (Some(960), 960, false),
            (Some(1920), 960, false),
        ]
    );
}

#[test]
fn a_stream_without_its_tags_begins_its_data_at_once() {
    let mut f = packets_page(FIRST, 0, 7, 0, &[&opus_head(312)]);
    f.extend(packets_page(
        LAST,
        312 + 2 * 960,
        7,
        1,
        &[&celt(30, 1), &celt(30, 2)],
    ));
    let mut d = Demuxer::open(Cursor::new(f)).unwrap();
    assert_eq!(d.streams()[0].headers.len(), 1);
    let mut n = 0;
    while d.next_packet().unwrap().is_some() {
        n += 1;
    }
    assert_eq!(n, 2);
}

#[test]
fn a_file_with_no_stream_is_refused() {
    for f in [
        Vec::new(),
        b"OggS but nothing else at all".to_vec(),
        packets_page(0, 0, 1, 3, &[b"stray"]),
    ] {
        assert!(matches!(
            Demuxer::open(Cursor::new(f)),
            Err(ogg::Error::Invalid(_))
        ));
    }
}

#[test]
fn a_stream_lasts_from_its_first_sound_to_its_last_pages_end() {
    // 2.51 s at 48 kHz and 1.73 s at 44.1 kHz, as the fixtures were made;
    // the chain's two links, 0.81 s and 0.67 s; its unlike second link not
    // counted.
    for (name, ticks) in [
        ("opus_stereo.opus", 120_480),
        ("vorbis_stereo.ogg", 76_293),
        ("opus_chained.opus", 38_880 + 32_160),
        ("opus_chained_unlike.opus", 20_640),
    ] {
        let d = Demuxer::open(File::open(data(name)).unwrap()).unwrap();
        assert_eq!(d.duration(0), Some(ticks), "{name}");
    }
}

#[test]
fn the_probe_knows_an_ogg_file() {
    let head = std::fs::read(data("opus_stereo.opus")).unwrap();
    assert!(ogg::probe(&head));
    assert!(!ogg::probe(b"OggS\x01\x00"));
    assert!(!ogg::probe(b"OggS\0\x08"));
    assert!(!ogg::probe(b"\x1aE\xdf\xa3"));
}

#[test]
fn a_chain_of_one_serial_gives_its_headers_again_in_band() {
    // Two copies of one file, one after the other: the second's first page
    // is not a new stream's (its serial is the first's), so it is one link,
    // whose headers come again among its data -- given with the next packet,
    // with Opus's pre-skip, as FFmpeg gives them.
    for (name, headers) in [
        ("opus_chained_link0.opus", 2),
        ("vorbis_chained_link0.ogg", 3),
    ] {
        let one = std::fs::read(data(name)).unwrap();
        let alone = read(one.clone());
        let twice = read([one.as_slice(), &one].concat());
        assert_eq!(twice.len(), 2 * alone.len(), "{name}");
        let second = &twice[alone.len()];
        assert_eq!(
            second.new_headers.as_ref().map(Vec::len),
            Some(headers),
            "{name}"
        );
        assert_eq!(second.data, alone[0].data, "{name}");
        if headers == 2 {
            assert_eq!(second.skip_samples, 312);
        }
        let d = Demuxer::open(Cursor::new([one.as_slice(), &one].concat())).unwrap();
        assert_eq!(d.links(), 1);
        assert_eq!(
            d.streams()[0].codec,
            if headers == 2 {
                Codec::Opus
            } else {
                Codec::Vorbis
            }
        );
    }
}
