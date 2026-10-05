//! What ffprobe's output cannot show: how much of a file a seek reads, what
//! a seek does where FFmpeg's answer depends on what it read before, what the
//! caller of a seek owes a sound decoder, and the files this refuses or
//! passes over that FFmpeg reads. Most of the cases came from lane E's
//! `apps/mediaprobe`, whose own Matroska demuxer this crate replaced
//! (`tests/data/generate_fixtures.py` has the rest, as fixtures).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    reason = "a test: a failure should be loud, and the files it lays out are small"
)]

use std::cell::RefCell;
use std::fs::File;
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::rc::Rc;

use matroska::{Demuxer, Error};

fn data(name: &str) -> String {
    format!("{}/tests/data/{name}", env!("CARGO_MANIFEST_DIR"))
}

fn open(name: &str) -> Demuxer<File> {
    Demuxer::open(File::open(data(name)).unwrap()).unwrap()
}

/// The next packet's bytes, as text.
fn next(d: &mut Demuxer<impl Read + Seek>) -> String {
    String::from_utf8(d.next_packet().unwrap().expect("a packet").data).unwrap()
}

// --- A file laid out here, element by element ----------------------------

/// An element: its ID's bytes, its size in eight bytes, and its body.
fn el(id: &[u8], body: &[u8]) -> Vec<u8> {
    let mut size = (body.len() as u64).to_be_bytes();
    size[0] = 0x01;
    [id, &size, body].concat()
}

fn uint(id: &[u8], v: u64) -> Vec<u8> {
    el(id, &v.to_be_bytes())
}

const SEGMENT: &[u8] = &[0x18, 0x53, 0x80, 0x67];
const CLUSTER: &[u8] = &[0x1F, 0x43, 0xB6, 0x75];

/// A SimpleBlock of `track`, `relative` ticks into its Cluster.
fn block(track: u8, relative: i16, key: bool, frame: &[u8]) -> Vec<u8> {
    let mut body = vec![0x80 | track];
    body.extend(relative.to_be_bytes());
    body.push(if key { 0x80 } else { 0 });
    body.extend_from_slice(frame);
    el(&[0xA3], &body)
}

/// A SimpleBlock of track 1.
fn simple(relative: i16, key: bool, frame: &[u8]) -> Vec<u8> {
    block(1, relative, key, frame)
}

/// A BlockGroup of `track`, `relative` ticks into its Cluster: a key frame
/// (no ReferenceBlock) and `addition` as its BlockAdditional 1.
fn group(track: u8, relative: i16, frame: &[u8], addition: &[u8]) -> Vec<u8> {
    let mut body = vec![0x80 | track];
    body.extend(relative.to_be_bytes());
    body.push(0);
    body.extend_from_slice(frame);
    let more = el(&[0xA6], &[uint(&[0xEE], 1), el(&[0xA5], addition)].concat());
    el(
        &[0xA0],
        &[el(&[0xA1], &body), el(&[0x75, 0xA1], &more)].concat(),
    )
}

fn cluster(time: u64, blocks: &[Vec<u8>]) -> Vec<u8> {
    el(CLUSTER, &[uint(&[0xE7], time), blocks.concat()].concat())
}

/// A file of the tracks `entries` -- each a `TrackEntry`'s body after its
/// number, numbered from 1 -- a millisecond a tick, and `clusters`.
fn file_of(entries: &[Vec<u8>], clusters: &[Vec<u8>]) -> Vec<u8> {
    let header = el(&[0x1A, 0x45, 0xDF, 0xA3], &el(&[0x42, 0x82], b"webm"));
    let info = el(
        &[0x15, 0x49, 0xA9, 0x66],
        &uint(&[0x2A, 0xD7, 0xB1], 1_000_000),
    );
    let tracks: Vec<u8> = (1u64..)
        .zip(entries)
        .flat_map(|(n, entry)| el(&[0xAE], &[uint(&[0xD7], n), entry.clone()].concat()))
        .collect();
    let tracks = el(&[0x16, 0x54, 0xAE, 0x6B], &tracks);
    [
        header,
        el(SEGMENT, &[info, tracks, clusters.concat()].concat()),
    ]
    .concat()
}

/// A file of one track.
fn file(entry: &[u8], clusters: &[Vec<u8>]) -> Vec<u8> {
    file_of(&[entry.to_vec()], clusters)
}

/// A VP9 video track's description after its number.
fn video() -> Vec<u8> {
    [
        uint(&[0x83], 1),
        el(&[0x86], b"V_VP9"),
        el(&[0xE0], &[uint(&[0xB0], 64), uint(&[0xBA], 48)].concat()),
    ]
    .concat()
}

/// An Opus track's description after its number.
fn sound() -> Vec<u8> {
    [uint(&[0x83], 2), el(&[0x86], b"A_OPUS")].concat()
}

/// A source that writes down where each read from it began and ended.
struct Watched {
    inner: Cursor<Vec<u8>>,
    reads: Rc<RefCell<Vec<(u64, u64)>>>,
}

impl Read for Watched {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let start = self.inner.position();
        let n = self.inner.read(buf)?;
        if n > 0 {
            self.reads.borrow_mut().push((start, start + n as u64));
        }
        Ok(n)
    }
}

impl Seek for Watched {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        self.inner.seek(pos)
    }
}

// --- The cases -------------------------------------------------------------

/// Without Cues, a seek walks the Clusters from the first and stops at the
/// first key frame of its track past the time: a seek near the start of a
/// long file reads its first Clusters, not all of them. A seek to a time the
/// walk has already passed for its track reads nothing, and one that needs
/// more goes on from where the walk stopped.
#[test]
fn a_walk_reads_only_as_far_as_the_seek_needs() {
    // Video frames larger than the reader's 64 KiB buffer, so that what is
    // read is what was asked for; a frame of sound after each key frame.
    let frame = |s: u8| vec![s; 128 * 1024];
    let clusters: Vec<Vec<u8>> = (0..3u8)
        .map(|s| {
            cluster(
                u64::from(s) * 1000,
                &[
                    block(1, 0, true, &frame(s)),
                    block(2, 5, true, b"sound"),
                    block(1, 40, false, &frame(s)),
                ],
            )
        })
        .collect();
    let bytes = file_of(&[video(), sound()], &clusters);
    let last = (bytes.len() - clusters[2].len()) as u64;
    let second = last - clusters[1].len() as u64;
    let reads = Rc::new(RefCell::new(Vec::new()));
    let source = Watched {
        inner: Cursor::new(bytes),
        reads: reads.clone(),
    };
    let mut d = Demuxer::open(source).unwrap();
    let seek = |d: &mut Demuxer<Watched>, track: u64, time: i64| {
        reads.borrow_mut().clear();
        d.seek(track, time).unwrap();
        let read = reads.borrow().clone();
        let p = d.next_packet().unwrap().unwrap();
        (read, p.track, p.timestamp)
    };

    // The video at 500 ms: the walk stops at the second Cluster's key frame.
    let (read, track, time) = seek(&mut d, 1, 500);
    let furthest = read.iter().map(|r| r.1).max().unwrap();
    assert!(
        furthest <= last,
        "the walk read on to {furthest}, past the last Cluster's start at {last}"
    );
    assert_eq!((track, time), (1, Some(0)));

    // The video at 300 ms: walked past already.
    let (read, _, time) = seek(&mut d, 1, 300);
    assert_eq!(read, [], "a seek the walk had passed read again");
    assert_eq!(time, Some(0));

    // The sound at 500 ms: the walk goes on, from where it stopped, to the
    // second Cluster's sound.
    let (read, track, time) = seek(&mut d, 2, 500);
    let nearest = read.iter().map(|r| r.0).min().unwrap();
    assert!(
        nearest > second,
        "the walk began again at {nearest}, not where it stopped in the second Cluster"
    );
    assert_eq!((track, time), (2, Some(5)));

    // The video at 300 ms again: the walk's last find is the sound's, but it
    // has passed the time for the video.
    let (read, _, time) = seek(&mut d, 1, 300);
    assert_eq!(read, [], "a seek the walk had passed read again");
    assert_eq!(time, Some(0));

    // The video at 2500 ms, past the last key frame: to the end.
    let (_, track, time) = seek(&mut d, 1, 2500);
    assert_eq!((track, time), (1, Some(2000)));
}

/// Cues need not name every key frame -- a muxer may cue one every few
/// seconds -- and a seek goes to the last cued one at or before the time,
/// not to a nearer key frame the Cues leave out. FFmpeg's index holds the
/// Cues and every key frame it has read since it opened the file, so it
/// does the same until it has read past the uncued one, and goes there
/// after: ffprobe, which reads this small file whole before it seeks, lands
/// on the second Cluster (design-decisions §1348).
#[test]
fn a_seek_goes_to_a_cue_not_past_it() {
    let mut d = open("sparse_cues.mkv");
    d.seek(1, 1500).unwrap();
    assert_eq!(next(&mut d), "key0");
    d.seek(1, 2000).unwrap();
    assert_eq!(next(&mut d), "key2");
}

/// Cues the file does not point to -- after the Clusters, with no SeekHead
/// to say where -- are not used, even once reading has passed them: FFmpeg
/// reads Cues only before the first Cluster or where the SeekHead points. So
/// where a seek lands does not depend on how far the file was read first
/// (`unlisted_cues.seek.txt` has the same seek made on a fresh file).
#[test]
fn cues_met_while_reading_are_passed_over() {
    let mut d = open("unlisted_cues.mkv");
    while d.next_packet().unwrap().is_some() {}
    d.seek(1, 1500).unwrap();
    assert_eq!(next(&mut d), "key1");
}

/// A sound decoder that needs `SeekPreRoll` of sound before its output is
/// right (Opus: 80 ms) is given it by its caller, who seeks that much before
/// the time wanted and drops what it decodes before the time -- as FFmpeg's
/// callers do, its seek taking no account of it (`pre_roll.seek.txt`).
#[test]
fn a_caller_pre_rolls_by_seeking_that_much_earlier() {
    let mut d = open("pre_roll.mkv");
    let track = d.tracks()[0].clone();
    assert_eq!(track.seek_pre_roll, 80_000_000);
    let (num, den) = d.time_base(track.number).unwrap();
    let pre_roll = i64::try_from(track.seek_pre_roll * den / (num * 1_000_000_000)).unwrap();
    d.seek(track.number, 1050).unwrap();
    assert_eq!(next(&mut d), "a1", "the demuxer's own seek");
    d.seek(track.number, 1050 - pre_roll).unwrap();
    assert_eq!(
        next(&mut d),
        "a0",
        "1050 ms less the pre-roll is the first Cluster"
    );
}

/// The segment's tick and length are the file's: a tick of 100
/// microseconds, and no length where the file gives none.
#[test]
fn the_segment_says_its_tick_and_its_length() {
    let d = open("tick_100us.mkv");
    assert_eq!(d.info().timestamp_scale, 100_000);
    assert_eq!(d.info().duration, None);
    assert_eq!(d.time_base(1), Some((1, 10_000)));
    let d = open("order.mkv");
    assert_eq!(d.info().duration, Some(3000.0));
    assert_eq!(d.doc_type(), Some(&b"webm"[..]));
}

/// What a file says for a person to read -- its title, and a track's
/// language as a BCP 47 tag beside its ISO 639-2 code -- is kept as written,
/// whatever its bytes, less EBML's zero padding; the last of two titles
/// stands; and who wrote the file is not kept. (ffprobe shows the title, but
/// FFmpeg's demuxer reads no BCP 47 tag, so neither is a fixture's.)
#[test]
fn the_title_and_a_bcp47_language_are_kept_as_written() {
    let header = el(&[0x1A, 0x45, 0xDF, 0xA3], &el(&[0x42, 0x82], b"webm"));
    let info = el(
        &[0x15, 0x49, 0xA9, 0x66],
        &[
            uint(&[0x2A, 0xD7, 0xB1], 1_000_000),
            el(&[0x7B, 0xA9], b"first"),
            el(&[0x4D, 0x80], b"a muxer"),
            el(&[0x7B, 0xA9], b"A \xffTitle\0\0"),
        ]
        .concat(),
    );
    let entry = [
        uint(&[0xD7], 1),
        video(),
        el(&[0x22, 0xB5, 0x9C], b"ger"),
        el(&[0x22, 0xB5, 0x9D], b"de-CH"),
    ]
    .concat();
    let tracks = el(&[0x16, 0x54, 0xAE, 0x6B], &el(&[0xAE], &entry));
    let clusters = cluster(0, &[simple(0, true, b"f")]);
    let bytes = [header, el(SEGMENT, &[info, tracks, clusters].concat())].concat();
    let d = Demuxer::open(Cursor::new(bytes)).unwrap();
    assert_eq!(d.info().title.as_deref(), Some(&b"A \xffTitle"[..]));
    let track = &d.tracks()[0];
    assert_eq!(track.language, b"ger");
    assert_eq!(track.language_bcp47.as_deref(), Some(&b"de-CH"[..]));
    // A file that says neither.
    let d = open("order.mkv");
    assert_eq!(d.info().title, None);
    assert!(d.tracks().iter().all(|t| t.language_bcp47.is_none()));
}

/// What is not Matroska is refused: no bytes, another format, and an EBML
/// header with no Segment after it.
#[test]
fn what_is_not_matroska_is_refused() {
    for bytes in [&b""[..], b"RIFF....AVI ", &[0x1A, 0x45, 0xDF, 0xA3, 0x80]] {
        assert!(
            Demuxer::open(Cursor::new(bytes)).is_err(),
            "{bytes:?} was read as Matroska"
        );
    }
}

/// A track stored with an encoding this does not undo -- bzip2, LZO,
/// encryption -- is not readable, and its packets are passed over while the
/// other tracks' come out. (FFmpeg undoes bzip2 and LZO, and hands encrypted
/// frames on for a decrypter: those files' packets are not ffprobe's.)
#[test]
fn a_track_stored_as_this_cannot_undo_is_passed_over() {
    let encoding = |kind: u64, algo: u64| {
        el(
            &[0x6D, 0x80],
            &el(
                &[0x62, 0x40],
                &[
                    uint(&[0x50, 0x33], kind),
                    el(&[0x50, 0x34], &uint(&[0x42, 0x54], algo)),
                ]
                .concat(),
            ),
        )
    };
    for (what, kind, algo) in [("bzip2", 0, 1), ("LZO", 0, 2), ("encryption", 1, 0)] {
        let header = el(&[0x1A, 0x45, 0xDF, 0xA3], &el(&[0x42, 0x82], b"matroska"));
        let info = el(
            &[0x15, 0x49, 0xA9, 0x66],
            &uint(&[0x2A, 0xD7, 0xB1], 1_000_000),
        );
        let entry = |n: u64, extra: &[u8]| {
            el(
                &[0xAE],
                &[uint(&[0xD7], n), video(), extra.to_vec()].concat(),
            )
        };
        let tracks = el(
            &[0x16, 0x54, 0xAE, 0x6B],
            &[entry(1, &encoding(kind, algo)), entry(2, &[])].concat(),
        );
        let block = |track: u8, frame: &[u8]| {
            el(&[0xA3], &[&[0x80 | track, 0, 0, 0x80][..], frame].concat())
        };
        let body = [
            info,
            tracks,
            cluster(0, &[block(1, b"stored"), block(2, b"plain")]),
        ]
        .concat();
        let bytes = [header, el(SEGMENT, &body)].concat();
        let mut d = Demuxer::open(Cursor::new(bytes)).unwrap();
        assert!(!d.tracks()[0].readable(), "{what}");
        assert!(d.tracks()[1].readable(), "{what}");
        assert_eq!(next(&mut d), "plain", "{what}");
        assert!(d.next_packet().unwrap().is_none(), "{what}");
    }
}

/// A track with no key frame has nowhere to seek to: the seek fails, as
/// FFmpeg's does, and reading goes on where it was.
#[test]
fn a_seek_in_a_track_without_key_frames_fails_and_reading_goes_on() {
    let bytes = file(
        &video(),
        &[
            cluster(0, &[simple(0, false, b"one"), simple(40, false, b"two")]),
            cluster(1000, &[simple(0, false, b"three")]),
        ],
    );
    let mut d = Demuxer::open(Cursor::new(bytes)).unwrap();
    assert_eq!(next(&mut d), "one");
    assert!(matches!(d.seek(1, 1500), Err(Error::Invalid(_))));
    assert_eq!(next(&mut d), "two");
    assert_eq!(next(&mut d), "three");
}

// --- One track read alone (`Demuxer::select_tracks`) -----------------------

fn every_packet(d: &mut Demuxer<impl Read + Seek>) -> Vec<matroska::Packet> {
    let mut all = Vec::new();
    while let Some(p) = d.next_packet().unwrap() {
        all.push(p);
    }
    all
}

/// A track read alone gives exactly the packets it gives among the others,
/// in every fixture but those whose damage is in another track's block --
/// which, passed over unread, no longer costs this track the packets near
/// it that a resynchronisation passes by (FFmpeg's too, discarding them).
#[test]
fn a_track_selected_alone_gives_the_packets_it_gives_among_the_others() {
    let mut differ = Vec::new();
    let mut checked = 0;
    for entry in std::fs::read_dir(data("")).unwrap() {
        let name = entry.unwrap().file_name().into_string().unwrap();
        if !(name.ends_with(".mkv") || name.ends_with(".webm")) {
            continue;
        }
        // The fixtures of files refused whole have nothing to select from.
        let Ok(mut d) = Demuxer::open(File::open(data(&name)).unwrap()) else {
            continue;
        };
        let all = every_packet(&mut d);
        // Read ahead a few bytes at a time, the file reads the same.
        let mut ahead = open(&name);
        ahead.set_read_ahead(7).unwrap();
        if every_packet(&mut ahead) != all {
            differ.push(format!("{name}: read 7 bytes ahead at a time"));
        }
        for n in d.tracks().iter().map(|t| t.number) {
            let mut alone = open(&name);
            alone.select_tracks(Some(&[n]));
            alone.set_read_ahead(1024).unwrap();
            let got = every_packet(&mut alone);
            let want: Vec<_> = all.iter().filter(|p| p.track == n).cloned().collect();
            if got != want {
                differ.push(format!(
                    "{name} track {n}: {} packets, {} wanted",
                    got.len(),
                    want.len()
                ));
            }
            checked += 1;
        }
    }
    assert!(
        differ.is_empty(),
        "{} of {checked} differ:\n{}",
        differ.len(),
        differ.join("\n")
    );
    assert!(checked > 40, "only {checked} tracks checked");
}

/// A source that counts the bytes read from it, and the reads.
struct Counted<R> {
    inner: R,
    read: Rc<RefCell<(u64, u64)>>,
}

impl<R: Read> Read for Counted<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        let mut read = self.read.borrow_mut();
        read.0 += n as u64;
        read.1 += 1;
        Ok(n)
    }
}

impl<R: Seek> Seek for Counted<R> {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        self.inner.seek(pos)
    }
}

/// A track read alone reads its own blocks and only the headers of the
/// others: a film's sound, or its subtitles, read through a handle of their
/// own, do not read the film's pictures over again -- a BlockGroup's
/// additions (a picture's alpha) no more than its Block.
#[test]
fn a_track_selected_alone_reads_little_of_the_others() {
    // A second of video at 24 frames, 30 KiB a frame -- a film's size -- a
    // frame of sound after each; every other frame a BlockGroup, its alpha
    // as big as its picture.
    let clusters: Vec<Vec<u8>> = (0..4u8)
        .map(|c| {
            let blocks: Vec<Vec<u8>> = (0..6i16)
                .flat_map(|f| {
                    let frame = vec![c; 30 * 1024];
                    let picture = if f % 2 == 0 {
                        block(1, f * 42, f == 0, &frame)
                    } else {
                        group(1, f * 42, &frame, &frame)
                    };
                    [picture, block(2, f * 42, true, b"sound")]
                })
                .collect();
            cluster(u64::from(c) * 250, &blocks)
        })
        .collect();
    let bytes = file_of(&[video(), sound()], &clusters);
    let size = bytes.len() as u64;
    let read = Rc::new(RefCell::new((0, 0)));
    let source = Counted {
        inner: Cursor::new(bytes),
        read: read.clone(),
    };
    let mut d = Demuxer::open(source).unwrap();
    d.select_tracks(Some(&[2]));
    // Read ahead little: each block passed over costs a read of this much.
    // (Opening read the file's start 64 KiB ahead: counted from here.)
    d.set_read_ahead(1024).unwrap();
    *read.borrow_mut() = (0, 0);
    let sound = every_packet(&mut d);
    assert_eq!(sound.len(), 24);
    assert!(sound.iter().all(|p| p.track == 2 && p.data == b"sound"));
    let read = read.borrow().0;
    assert!(
        read * 10 < size,
        "{read} of the file's {size} bytes read for its sound alone"
    );
}

/// A block naming a track the file does not declare is damage, to a track
/// read alone as to all of them: not passed over as another track's, it is
/// read whole and the reading resynchronises past it. ffprobe gives `a` and
/// `c` here -- `b`, after the damage in its Cluster, is lost -- checked with
/// a text track in place of the sound (FFmpeg's Opus parser drops these
/// made-up packets; this demuxer has no parser).
#[test]
fn a_block_naming_no_track_is_damage_to_a_track_read_alone_too() {
    let clusters = [
        cluster(
            0,
            &[
                block(2, 0, true, b"a"),
                block(9, 10, true, b"x"),
                block(2, 20, true, b"b"),
            ],
        ),
        cluster(1000, &[block(2, 0, true, b"c")]),
    ];
    let bytes = file_of(&[video(), sound()], &clusters);
    let all = every_packet(&mut Demuxer::open(Cursor::new(bytes.clone())).unwrap());
    let data: Vec<&[u8]> = all.iter().map(|p| p.data.as_slice()).collect();
    assert_eq!(data, [b"a".as_slice(), b"c"]);
    let mut alone = Demuxer::open(Cursor::new(bytes)).unwrap();
    alone.select_tracks(Some(&[2]));
    assert_eq!(every_packet(&mut alone), all);
}

/// How much of a real film each track read alone reads, and how long it
/// takes: `MATROSKA_FILM=film.mkv cargo test --release -p matroska --test
/// beyond_ffprobe -- --ignored --nocapture film`.
#[test]
#[ignore = "a measurement, over a film of the caller's"]
fn film_read_track_by_track() {
    let path = std::env::var("MATROSKA_FILM").expect("MATROSKA_FILM names a film");
    let size = std::fs::metadata(&path).unwrap().len();
    let tracks: Vec<u64> = Demuxer::open(File::open(&path).unwrap())
        .unwrap()
        .tracks()
        .iter()
        .map(|t| t.number)
        .collect();
    let selections = std::iter::once(None).chain(tracks.into_iter().map(Some));
    for selection in selections {
        for ahead in [64 * 1024, 4096, 2048, 1024, 512, 256] {
            let read = Rc::new(RefCell::new((0, 0)));
            let source = Counted {
                inner: File::open(&path).unwrap(),
                read: read.clone(),
            };
            let start = std::time::Instant::now();
            let mut d = Demuxer::open(source).unwrap();
            d.set_read_ahead(ahead).unwrap();
            if let Some(n) = selection {
                d.select_tracks(Some(&[n]));
            }
            let packets = every_packet(&mut d).len();
            let (bytes, reads) = *read.borrow();
            println!(
                "track {selection:?}, {ahead} ahead: {packets} packets, {bytes} of {size} bytes read ({:.1}%) in {reads} reads, {:?}",
                bytes as f64 * 100.0 / size as f64,
                start.elapsed()
            );
        }
    }
}
