#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use std::io::Cursor;

use super::*;
use crate::Codec;
use crate::mkv::{CODEC_ID, SEEK, SEEK_ID, SEEK_POSITION};

// --- An EBML writer, for laying out files block by block -----------------

/// An id's bytes: the id with its marker, without leading zeros.
fn id(id: u64) -> Vec<u8> {
    let b = id.to_be_bytes();
    let first = b.iter().position(|&x| x != 0).unwrap_or(7);
    b[first..].to_vec()
}

/// A length as an EBML integer of eight bytes.
fn size(n: usize) -> Vec<u8> {
    let mut b = (n as u64).to_be_bytes();
    b[0] = 0x01;
    b.to_vec()
}

/// The length "not known", in eight bytes.
const UNKNOWN: [u8; 8] = [0x01, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF];

fn el(i: u64, body: &[u8]) -> Vec<u8> {
    let mut out = id(i);
    out.extend(size(body.len()));
    out.extend_from_slice(body);
    out
}

fn el_unknown(i: u64, body: &[u8]) -> Vec<u8> {
    let mut out = id(i);
    out.extend(UNKNOWN);
    out.extend_from_slice(body);
    out
}

fn uint_el(i: u64, v: u64) -> Vec<u8> {
    el(i, &v.to_be_bytes())
}

fn cat(parts: &[Vec<u8>]) -> Vec<u8> {
    parts.concat()
}

const VIDEO: u64 = 1;
const AUDIO: u64 = 2;

/// A track entry: `number`, `kind` (1 video, 2 audio), `codec`, then
/// `extra` elements.
fn track(number: u64, kind: u64, codec: &str, extra: &[Vec<u8>]) -> Vec<u8> {
    let mut body = cat(&[
        uint_el(TRACK_NUMBER, number),
        uint_el(0x83, kind),
        el(CODEC_ID, codec.as_bytes()),
    ]);
    for e in extra {
        body.extend_from_slice(e);
    }
    el(TRACK_ENTRY, &body)
}

/// The usual two tracks: VP9 video (1) at 25 frames a second, Opus (2).
fn two_tracks() -> Vec<u8> {
    el(
        TRACKS,
        &cat(&[
            track(VIDEO, 1, "V_VP9", &[uint_el(DEFAULT_DURATION, 40_000_000)]),
            track(AUDIO, 2, "A_OPUS", &[el(CODEC_PRIVATE, b"OpusHead")]),
        ]),
    )
}

/// A block's body: the track, its time from its cluster's, its flags, and
/// `rest` (one frame, or a lace).
fn block_body(track: u64, relative: i16, flags: u8, rest: &[u8]) -> Vec<u8> {
    let mut out = vec![0x80 | track as u8];
    out.extend(relative.to_be_bytes());
    out.push(flags);
    out.extend_from_slice(rest);
    out
}

fn simple(track: u64, relative: i16, key: bool, frame: &[u8]) -> Vec<u8> {
    el(
        SIMPLE_BLOCK,
        &block_body(track, relative, if key { 0x80 } else { 0 }, frame),
    )
}

fn cluster(time: u64, blocks: &[Vec<u8>]) -> Vec<u8> {
    let mut body = uint_el(CLUSTER_TIMESTAMP, time);
    for b in blocks {
        body.extend_from_slice(b);
    }
    el(CLUSTER, &body)
}

/// A file: the EBML header, then a Segment of Info (a millisecond a tick,
/// `duration` ticks long), `tracks` and `rest`.
fn file(tracks: &[u8], rest: &[Vec<u8>]) -> Vec<u8> {
    let header = el(EBML_HEADER, &el(0x4282, b"webm"));
    let info = el(
        INFO,
        &cat(&[
            uint_el(TIMESTAMP_SCALE, 1_000_000),
            el(DURATION, &3000.0_f64.to_be_bytes()),
        ]),
    );
    let mut body = cat(&[info, tracks.to_vec()]);
    for r in rest {
        body.extend_from_slice(r);
    }
    cat(&[header, el(SEGMENT, &body)])
}

fn open(bytes: &[u8]) -> Demuxer<Cursor<&[u8]>> {
    Demuxer::open(Cursor::new(bytes), bytes.len() as u64)
        .unwrap()
        .expect("a Matroska file")
}

fn all(d: &mut Demuxer<Cursor<&[u8]>>) -> Vec<Packet> {
    let mut out = Vec::new();
    while let Some(p) = d.next_packet().unwrap() {
        out.push(p);
        assert!(out.len() < 100_000, "the demuxer does not stop");
    }
    out
}

const MS: i64 = 1_000_000;

/// `CodecPrivate`'s id: written into the test files, which carry one as a
/// muxer's would, though the demuxer has nothing yet that reads it.
const CODEC_PRIVATE: u64 = 0x63A2;

// --- The tests -------------------------------------------------------------

#[test]
fn simple_blocks_come_out_in_order_with_their_times() {
    let bytes = file(
        &two_tracks(),
        &[
            cluster(
                100,
                &[
                    simple(VIDEO, 0, true, b"key"),
                    simple(AUDIO, 5, true, b"sound"),
                    simple(VIDEO, 40, false, b"inter"),
                ],
            ),
            cluster(180, &[simple(VIDEO, -1, false, b"late")]),
        ],
    );
    let mut d = open(&bytes);
    assert_eq!(d.duration_ns(), Some(3_000_000_000));
    let streams = d.streams();
    assert_eq!(streams.len(), 2);
    assert_eq!(
        (streams[0].number, streams[0].info.codec.clone()),
        (1, Codec::Vp9)
    );
    assert_eq!(streams[0].default_duration_ns, Some(40_000_000));
    let got: Vec<(u64, i64, bool, Vec<u8>)> = all(&mut d)
        .into_iter()
        .map(|p| (p.track, p.timestamp_ns, p.keyframe, p.data))
        .collect();
    let want: Vec<(u64, i64, bool, Vec<u8>)> = vec![
        (VIDEO, 100 * MS, true, b"key".to_vec()),
        (AUDIO, 105 * MS, true, b"sound".to_vec()),
        (VIDEO, 140 * MS, false, b"inter".to_vec()),
        (VIDEO, 179 * MS, false, b"late".to_vec()),
    ];
    assert_eq!(got, want);
}

#[test]
fn each_lacing_gives_its_frames() {
    let a = vec![1u8; 300];
    // 254 bytes: one Xiph size byte short of the 255 that says "more".
    let b = vec![2u8; 254];
    let c = vec![3u8; 7];
    // Xiph: 3 frames; sizes 300 (255 + 45) and 254; the last is the rest.
    let mut xiph = vec![2, 255, 45, 254];
    xiph.extend(&a);
    xiph.extend(&b);
    xiph.extend(&c);
    // EBML: 3 frames; 300, then 254 (a difference of -46), the last the
    // rest. -46 in a two-byte signed integer: 0x4000 | (-46 + 8191).
    let diff = (8191 - 46) as u16 | 0x4000;
    let mut ebml = vec![2, 0x41, 0x2C];
    ebml.extend(diff.to_be_bytes());
    ebml.extend(&a);
    ebml.extend(&b);
    ebml.extend(&c);
    // Fixed: 3 frames of 4.
    let mut fixed = vec![2];
    fixed.extend([4u8; 4]);
    fixed.extend([5u8; 4]);
    fixed.extend([6u8; 4]);
    let bytes = file(
        &two_tracks(),
        &[cluster(
            0,
            &[
                el(SIMPLE_BLOCK, &block_body(VIDEO, 0, 0x80 | 0x02, &xiph)),
                el(SIMPLE_BLOCK, &block_body(VIDEO, 200, 0x06, &ebml)),
                el(SIMPLE_BLOCK, &block_body(VIDEO, 400, 0x04, &fixed)),
            ],
        )],
    );
    let packets = all(&mut open(&bytes));
    let frames: Vec<&[u8]> = packets.iter().map(|p| p.data.as_slice()).collect();
    assert_eq!(
        frames,
        vec![&a[..], &b, &c, &a, &b, &c, &[4; 4][..], &[5; 4], &[6; 4]]
    );
    // The frames after a lace's first follow it by the track's 40 ms.
    let times: Vec<i64> = packets.iter().map(|p| p.timestamp_ns / MS).collect();
    assert_eq!(times, [0, 40, 80, 200, 240, 280, 400, 440, 480]);
    // Only a lace's first frame is where decoding may start.
    let keys: Vec<bool> = packets.iter().map(|p| p.keyframe).collect();
    assert_eq!(
        keys,
        [true, false, false, false, false, false, false, false, false]
    );
}

#[test]
fn a_lace_that_does_not_add_up_is_dropped() {
    // Fixed: 10 bytes do not divide into 3; Xiph: sizes past the data.
    let bytes = file(
        &two_tracks(),
        &[cluster(
            0,
            &[
                el(
                    SIMPLE_BLOCK,
                    &block_body(VIDEO, 0, 0x04, &[2, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10]),
                ),
                el(
                    SIMPLE_BLOCK,
                    &block_body(VIDEO, 1, 0x02, &[1, 255, 255, 9, 1]),
                ),
                simple(VIDEO, 2, true, b"after"),
            ],
        )],
    );
    let packets = all(&mut open(&bytes));
    assert_eq!(packets.len(), 1);
    assert_eq!(packets[0].data, b"after");
}

#[test]
fn a_block_group_is_a_key_frame_without_a_reference() {
    let group = |relative: i16, reference: bool, frame: &[u8]| {
        let mut body = cat(&[
            el(BLOCK, &block_body(VIDEO, relative, 0, frame)),
            uint_el(BLOCK_DURATION, 33),
        ]);
        if reference {
            body.extend(el(REFERENCE_BLOCK, &[0xFF]));
        }
        el(BLOCK_GROUP, &body)
    };
    let bytes = file(
        &two_tracks(),
        &[cluster(0, &[group(0, false, b"i"), group(33, true, b"p")])],
    );
    let packets = all(&mut open(&bytes));
    assert_eq!(packets.len(), 2);
    assert!(packets[0].keyframe);
    assert!(!packets[1].keyframe);
    assert_eq!(packets[0].duration_ns, Some(33 * 1_000_000));
}

#[test]
fn a_cluster_of_unknown_length_ends_where_the_next_begins() {
    let unknown = |time: u64, blocks: &[Vec<u8>]| {
        let mut body = uint_el(CLUSTER_TIMESTAMP, time);
        for b in blocks {
            body.extend_from_slice(b);
        }
        el_unknown(CLUSTER, &body)
    };
    let bytes = file(
        &two_tracks(),
        &[
            unknown(
                0,
                &[
                    simple(VIDEO, 0, true, b"one"),
                    simple(VIDEO, 40, false, b"two"),
                ],
            ),
            unknown(80, &[simple(VIDEO, 0, false, b"three")]),
            cluster(120, &[simple(VIDEO, 0, true, b"four")]),
        ],
    );
    let packets = all(&mut open(&bytes));
    let got: Vec<(&[u8], i64)> = packets
        .iter()
        .map(|p| (p.data.as_slice(), p.timestamp_ns / MS))
        .collect();
    assert_eq!(
        got,
        vec![
            (&b"one"[..], 0),
            (b"two", 40),
            (b"three", 80),
            (b"four", 120)
        ]
    );
}

/// A seek head of one entry: the cues, at `position` from the Segment's
/// body. The same length whatever the position.
fn seek_head(position: u64) -> Vec<u8> {
    el(
        SEEK_HEAD,
        &el(
            SEEK,
            &cat(&[el(SEEK_ID, &id(CUES)), uint_el(SEEK_POSITION, position)]),
        ),
    )
}

/// Three clusters a second apart, a key frame first in each; with cues
/// after them when `cued`, found as a muxer has them found -- through a
/// seek head before `Info` -- and positions from the Segment's body.
fn three_seconds(cued: bool) -> Vec<u8> {
    let clusters: Vec<Vec<u8>> = (0..3)
        .map(|s| {
            cluster(
                s * 1000,
                &[
                    simple(VIDEO, 0, true, format!("key{s}").as_bytes()),
                    simple(VIDEO, 40, false, format!("inter{s}").as_bytes()),
                ],
            )
        })
        .collect();
    let info = el(
        INFO,
        &cat(&[
            uint_el(TIMESTAMP_SCALE, 1_000_000),
            el(DURATION, &3000.0_f64.to_be_bytes()),
        ]),
    );
    let tracks = two_tracks();
    let mut body = Vec::new();
    let head = if cued { seek_head(0).len() } else { 0 };
    // Each cluster's offset from the Segment's body: the seek head, Info,
    // Tracks, then the clusters in turn.
    let mut at = (head + info.len() + tracks.len()) as u64;
    let mut points = Vec::new();
    for (s, c) in clusters.iter().enumerate() {
        points.push(el(
            CUE_POINT,
            &cat(&[
                uint_el(CUE_TIME, s as u64 * 1000),
                el(
                    CUE_TRACK_POSITIONS,
                    &cat(&[uint_el(CUE_TRACK, VIDEO), uint_el(CUE_CLUSTER_POSITION, at)]),
                ),
            ]),
        ));
        at += c.len() as u64;
    }
    if cued {
        body.extend(seek_head(at));
    }
    body.extend(info);
    body.extend(tracks);
    for c in &clusters {
        body.extend_from_slice(c);
    }
    if cued {
        body.extend(el(CUES, &points.concat()));
    }
    let header = el(EBML_HEADER, &el(0x4282, b"webm"));
    cat(&[header, el(SEGMENT, &body)])
}

/// RFC 9559's `CodecDelay` "MUST be subtracted from each frame timestamp in
/// order to get the timestamp that will be actually played": of its track's
/// frames alone, and to before zero at the start.
#[test]
fn a_codec_delay_is_taken_off_its_tracks_timestamps() {
    let tracks = el(
        TRACKS,
        &cat(&[
            track(VIDEO, 1, "V_VP9", &[]),
            track(AUDIO, 2, "A_OPUS", &[uint_el(CODEC_DELAY, 6_500_000)]),
        ]),
    );
    let bytes = file(
        &tracks,
        &[cluster(
            0,
            &[
                simple(AUDIO, 0, true, b"first"),
                simple(VIDEO, 0, true, b"picture"),
                simple(AUDIO, 20, true, b"second"),
            ],
        )],
    );
    let mut d = open(&bytes);
    assert_eq!(d.streams()[1].codec_delay_ns, 6_500_000);
    let got: Vec<(u64, i64)> = all(&mut d)
        .into_iter()
        .map(|p| (p.track, p.timestamp_ns))
        .collect();
    assert_eq!(
        got,
        [
            (AUDIO, -6_500_000),
            (VIDEO, 0),
            (AUDIO, 20 * MS - 6_500_000),
        ]
    );
}

/// Clusters a second apart of one Opus track whose decoder needs `pre_roll`
/// nanoseconds before what it gives back is right, and whose frames are
/// played `delay` nanoseconds before their blocks' times.
fn rolled(pre_roll: u64, delay: u64) -> Vec<u8> {
    let tracks = el(
        TRACKS,
        &track(
            AUDIO,
            2,
            "A_OPUS",
            &[
                uint_el(SEEK_PRE_ROLL, pre_roll),
                uint_el(CODEC_DELAY, delay),
            ],
        ),
    );
    let clusters: Vec<Vec<u8>> = (0..3)
        .map(|s| {
            cluster(
                s * 1000,
                &[simple(AUDIO, 0, true, format!("a{s}").as_bytes())],
            )
        })
        .collect();
    file(&tracks, &clusters)
}

/// A seek starts decoding a track's `SeekPreRoll` before the time sought,
/// so the decoder has had what it needs by then -- and finds the time in
/// block time, which is the played time plus the codec's delay.
#[test]
fn a_seek_starts_a_pre_roll_before_the_time_sought() {
    let first = |bytes: &[u8], at_ms: u64| {
        let mut d = open(bytes);
        d.seek(at_ms * 1_000_000, AUDIO).unwrap();
        let p = d.next_packet().unwrap().unwrap();
        String::from_utf8(p.data).unwrap()
    };
    let none = rolled(0, 0);
    assert_eq!(first(&none, 1050), "a1", "control: no pre-roll");
    let opus = rolled(80_000_000, 0);
    assert_eq!(
        first(&opus, 1050),
        "a0",
        "the pre-roll was not decoded first"
    );
    assert_eq!(first(&opus, 1100), "a1");
    // With a 960 ms delay, 30 ms played is 990 ms of block time -- still the
    // first cluster -- but 100 ms played is 1060: the second, where reading
    // the played time as block time would stay in the first.
    let delayed = rolled(0, 960_000_000);
    assert_eq!(first(&delayed, 30), "a0");
    assert_eq!(
        first(&delayed, 100),
        "a1",
        "the codec delay was not added back"
    );
}

#[test]
fn a_seek_goes_to_the_cluster_of_the_time_sought_with_cues_or_without() {
    for cued in [true, false] {
        let bytes = three_seconds(cued);
        let mut d = open(&bytes);
        assert_eq!(d.has_cues(), cued);
        for (seek_ms, first) in [
            (1500, "key1"),
            (1000, "key1"),
            (999, "key0"),
            (0, "key0"),
            (9000, "key2"),
        ] {
            d.seek(seek_ms * 1_000_000, VIDEO).unwrap();
            let p = d.next_packet().unwrap().unwrap();
            assert_eq!(
                std::str::from_utf8(&p.data).unwrap(),
                first,
                "cued {cued}, seek to {seek_ms} ms"
            );
            assert!(p.keyframe);
        }
        // And on from there to the end.
        d.seek(2_000_000_000, VIDEO).unwrap();
        assert_eq!(all(&mut d).len(), 2);
    }
}

/// A reader that remembers the furthest byte read.
struct Watched<'a> {
    inner: Cursor<&'a [u8]>,
    furthest: std::rc::Rc<std::cell::Cell<u64>>,
}

impl std::io::Read for Watched<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        let end = self.inner.position();
        if end > self.furthest.get() {
            self.furthest.set(end);
        }
        Ok(n)
    }
}

impl std::io::Seek for Watched<'_> {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        self.inner.seek(pos)
    }
}

/// Without cues, a seek walks the clusters' timestamps from the first and
/// stops at the first one past the time: a seek near the start of a long
/// film reads its first clusters, not all of them.
#[test]
fn a_walk_stops_at_the_first_cluster_past_the_time() {
    let bytes = three_seconds(false);
    let furthest = std::rc::Rc::new(std::cell::Cell::new(0));
    let r = Watched {
        inner: Cursor::new(&bytes),
        furthest: furthest.clone(),
    };
    let mut d = Demuxer::open(r, bytes.len() as u64).unwrap().unwrap();
    // Where the last cluster begins: its timestamp is read only by a walk
    // that did not stop at the one before it.
    let last = bytes.len()
        - cluster(
            2000,
            &[
                simple(VIDEO, 0, true, b"key2"),
                simple(VIDEO, 40, false, b"inter2"),
            ],
        )
        .len();
    furthest.set(0);
    d.seek(500 * 1_000_000, VIDEO).unwrap();
    assert!(
        furthest.get() <= last as u64,
        "the walk read on to {} past the cluster at {last}",
        furthest.get()
    );
    assert_eq!(d.next_packet().unwrap().unwrap().data, b"key0");
}

/// Cues need not name every cluster -- a muxer cues key frames, every few
/// seconds -- and a seek goes to the last cue's cluster, not the nearest
/// cluster a walk would find.
#[test]
fn a_seek_goes_to_a_cue_not_past_it() {
    let bytes = three_seconds(true);
    let mut d = open(&bytes);
    // The cue of the middle cluster dropped.
    d.cues.retain(|c| c.time != 1000);
    d.seek(1_500_000_000, VIDEO).unwrap();
    assert_eq!(d.next_packet().unwrap().unwrap().data, b"key0");
    // Cues of another track only: those are used.
    let mut d = open(&bytes);
    for c in &mut d.cues {
        c.track = AUDIO;
    }
    d.seek(1_500_000_000, VIDEO).unwrap();
    assert_eq!(d.next_packet().unwrap().unwrap().data, b"key1");
}

/// A tick of 100 microseconds, not the usual millisecond: cluster 100 and a
/// block 50 after it are 15 ms in.
#[test]
fn the_timestamp_scale_is_the_files() {
    let header = el(EBML_HEADER, &el(0x4282, b"webm"));
    let info = el(INFO, &uint_el(TIMESTAMP_SCALE, 100_000));
    let body = cat(&[
        info,
        two_tracks(),
        cluster(100, &[simple(VIDEO, 50, true, b"x")]),
    ]);
    let bytes = cat(&[header, el(SEGMENT, &body)]);
    let packets = all(&mut open(&bytes));
    assert_eq!(packets[0].timestamp_ns, 15 * MS);
    // Without a Duration, no length.
    assert_eq!(open(&bytes).duration_ns(), None);
}

#[test]
fn header_stripping_is_undone_and_other_compression_passed_over() {
    let encoding = |algo: u64, settings: &[u8]| {
        el(
            CONTENT_ENCODINGS,
            &el(
                CONTENT_ENCODING,
                &el(
                    CONTENT_COMPRESSION,
                    &cat(&[
                        uint_el(CONTENT_COMP_ALGO, algo),
                        el(CONTENT_COMP_SETTINGS, settings),
                    ]),
                ),
            ),
        )
    };
    let tracks = el(
        TRACKS,
        &cat(&[
            track(VIDEO, 1, "V_MPEG4/ISO/AVC", &[encoding(3, &[0, 0, 1])]),
            track(AUDIO, 2, "A_AAC", &[encoding(0, &[])]),
        ]),
    );
    let bytes = file(
        &tracks,
        &[cluster(
            0,
            &[
                simple(VIDEO, 0, true, b"rest"),
                simple(AUDIO, 0, true, b"zlib"),
            ],
        )],
    );
    let mut d = open(&bytes);
    assert!(d.streams()[0].readable);
    assert!(!d.streams()[1].readable);
    let packets = all(&mut d);
    assert_eq!(packets.len(), 1);
    assert_eq!(packets[0].data, b"\0\0\x01rest");
}

#[test]
fn bad_bytes_are_searched_past_for_the_next_cluster() {
    // A child whose id is no EBML integer (a zero byte), mid-cluster: the
    // rest of the cluster is lost, and the next one is found by its id past
    // two bytes of rubbish between them.
    let mut bad = vec![0x00, 0x00, 0x00];
    bad.extend(simple(VIDEO, 40, false, b"lost"));
    // The cluster's own length covers the bad bytes.
    let mut body = uint_el(CLUSTER_TIMESTAMP, 0);
    body.extend(simple(VIDEO, 0, true, b"before"));
    body.extend(&bad);
    let first = el(CLUSTER, &body);
    let bytes = file(
        &two_tracks(),
        &[
            first,
            vec![0xDE, 0xAD],
            cluster(1000, &[simple(VIDEO, 0, true, b"after")]),
        ],
    );
    let packets = all(&mut open(&bytes));
    let got: Vec<&[u8]> = packets.iter().map(|p| p.data.as_slice()).collect();
    assert_eq!(got, vec![&b"before"[..], b"after"]);
}

#[test]
fn what_is_not_matroska_is_none() {
    for bytes in [&b""[..], b"RIFF....AVI ", &[0x1A, 0x45, 0xDF, 0xA3, 0x80]] {
        assert!(
            Demuxer::open(Cursor::new(bytes), bytes.len() as u64)
                .unwrap()
                .is_none(),
            "{bytes:?}"
        );
    }
}

/// Every byte of a small file XORed, and every cut of it: the demuxer
/// comes back each time, with no panic and no endless loop. (A damaged
/// lace count can make one block 256 empty frames, so the bound is loose.)
#[test]
fn no_damage_makes_it_panic_or_loop() {
    let bytes = three_seconds(true);
    let check = |b: &[u8]| {
        if let Some(mut d) = Demuxer::open(Cursor::new(b), b.len() as u64).unwrap() {
            let mut n = 0;
            while let Some(_p) = d.next_packet().unwrap() {
                n += 1;
                assert!(n <= 4096, "the demuxer does not stop");
            }
            d.seek(1_500_000_000, VIDEO).unwrap();
            while d.next_packet().unwrap().is_some() {
                n += 1;
                assert!(n <= 8192, "the demuxer does not stop after a seek");
            }
        }
    };
    for i in 0..bytes.len() {
        for x in [0x01, 0x80, 0xFF] {
            let mut b = bytes.clone();
            b[i] ^= x;
            check(&b);
        }
        check(&bytes[..i]);
    }
}
