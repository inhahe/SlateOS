//! Every fixture's packets, held to `ffprobe`'s: what FFmpeg's demuxer makes
//! of each (`tests/data/generate_fixtures.py`, which says how each fixture
//! was made and what it is for).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "a test: a failure should be loud"
)]

use std::fs::File;

use matroska::Demuxer;

/// One packet as `ffprobe` printed it.
#[derive(Debug, PartialEq, Eq)]
struct Line {
    stream: usize,
    pts: Option<i64>,
    /// 0 where ffprobe prints N/A.
    duration: u64,
    size: usize,
    pos: u64,
    key: bool,
    md5: String,
    additions: Vec<u64>,
}

/// A fixture's answers: each stream's time base, and the packets.
fn answers(name: &str) -> (Vec<(u64, u64)>, Vec<Line>) {
    let base = name.rsplit_once('.').unwrap().0;
    let path = format!("{}/tests/data/{base}.txt", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let (mut streams, mut packets) = (Vec::new(), Vec::new());
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        let w: Vec<&str> = line.split(' ').collect();
        let number = |s: &str| (s != "N/A").then(|| s.parse::<i64>().unwrap());
        match w[0] {
            "stream" => {
                let (num, den) = w[3].split_once('/').unwrap();
                streams.push((num.parse().unwrap(), den.parse().unwrap()));
            }
            "packet" => packets.push(Line {
                stream: w[1].parse().unwrap(),
                pts: number(w[2]),
                duration: number(w[3]).map_or(0, |d| u64::try_from(d).unwrap()),
                size: w[4].parse().unwrap(),
                pos: w[5].parse().unwrap(),
                key: w[6].starts_with('K'),
                md5: w[7].to_owned(),
                additions: if w[8] == "-" {
                    Vec::new()
                } else {
                    w[8].split(',').map(|a| a.parse().unwrap()).collect()
                },
            }),
            other => panic!("{path}: a line this test does not know: {other}"),
        }
    }
    (streams, packets)
}

const FIXTURES: [&str; 11] = [
    "vp9_opus.webm",
    "av1.webm",
    "vp8_vorbis.webm",
    "vp9_alpha.webm",
    "live.webm",
    "subtitles.mkv",
    "laced.mkv",
    "laced_untimed.mkv",
    "compressed.mkv",
    "unknown_sizes.mkv",
    "damaged.mkv",
];

/// Each fixture's streams and every packet -- the stream, timestamp,
/// duration, size, position, key-frame flag, bytes and additions -- as
/// FFmpeg's demuxer gives them.
#[test]
fn every_fixture_demuxes_as_ffmpeg_does() {
    for name in FIXTURES {
        let path = format!("{}/tests/data/{name}", env!("CARGO_MANIFEST_DIR"));
        let mut d =
            Demuxer::open(File::open(&path).unwrap()).unwrap_or_else(|e| panic!("{name}: {e}"));
        let (streams, expected) = answers(name);
        let numbers: Vec<u64> = d.tracks().iter().map(|t| t.number).collect();
        assert_eq!(numbers.len(), streams.len(), "{name}: the streams");
        for (n, tb) in numbers.iter().zip(&streams) {
            assert_eq!(d.time_base(*n), Some(*tb), "{name}: track {n}'s time base");
        }
        let mut got = Vec::new();
        while let Some(p) = d.next_packet().unwrap_or_else(|e| panic!("{name}: {e}")) {
            got.push(Line {
                stream: numbers.iter().position(|&n| n == p.track).unwrap(),
                pts: p.timestamp,
                duration: p.duration,
                size: p.data.len(),
                pos: p.position,
                key: p.keyframe,
                md5: md5::md5_hex(&p.data).to_string(),
                additions: p.additions.iter().map(|(id, _)| *id).collect(),
            });
        }
        for (i, (g, e)) in got.iter().zip(&expected).enumerate() {
            assert_eq!(g, e, "{name}: packet {i}");
        }
        assert_eq!(got.len(), expected.len(), "{name}: the packets");
    }
}

/// Every byte of a file with Cues, a laced one, one of unknown sizes and a
/// compressed one inverted in turn, then each cut short at every length: each
/// opens and reads to its end, or is refused -- never a panic, never more
/// packets or bytes than the file could hold -- and a seek in each works or
/// is refused.
#[test]
fn a_damaged_file_is_read_or_refused_but_never_panics() {
    for name in [
        "live.webm",
        "laced.mkv",
        "unknown_sizes.mkv",
        "compressed.mkv",
        "vp9_alpha.webm",
    ] {
        let path = format!("{}/tests/data/{name}", env!("CARGO_MANIFEST_DIR"));
        let bytes = std::fs::read(&path).unwrap();
        let read_all = |data: &[u8], what: &str| {
            let Ok(mut d) = Demuxer::open(std::io::Cursor::new(data.to_vec())) else {
                return;
            };
            let (mut packets, mut total) = (0usize, 0usize);
            while let Ok(Some(p)) = d.next_packet() {
                packets += 1;
                total += p.data.len();
                // A packet is at least the byte of its block it came from,
                // but zlib can grow one: bounded by the inflate limit.
                assert!(packets <= data.len() * 256, "{name}: {what}: packets");
                assert!(
                    total <= data.len() * 1024 + (256 << 20),
                    "{name}: {what}: bytes"
                );
            }
            if let Some(t) = d.tracks().first().map(|t| t.number) {
                let _ = d.seek(t, 50);
                while let Ok(Some(_)) = d.next_packet() {}
            }
        };
        let mut damaged = bytes.clone();
        for at in 0..bytes.len() {
            damaged[at] ^= 0xff;
            read_all(&damaged, &format!("byte {at} inverted"));
            damaged[at] ^= 0xff;
        }
        for len in 0..bytes.len() {
            read_all(&bytes[..len], &format!("cut at {len}"));
        }
    }
}

/// One seek's answer: where to, in milliseconds, and each packet after it
/// (its stream, timestamp and key-frame flag).
type SeekAnswer = (i64, Vec<(usize, Option<i64>, bool)>);

/// The fixtures `generate_fixtures.py` seeks in: with Cues, and without
/// (where the key frames are found by reading).
const SEEKABLE: [&str; 6] = [
    "vp9_opus.webm",
    "av1.webm",
    "vp8_vorbis.webm",
    "live.webm",
    "unknown_sizes.mkv",
    "laced.mkv",
];

/// After a seek to each of five times -- the start, between key frames, past
/// the end -- the first six packets are FFmpeg's: the latest key frame at or
/// before the time, and what follows it, the earlier packets of every track
/// dropped.
#[test]
fn every_seek_lands_where_ffmpeg_s_does() {
    for name in SEEKABLE {
        let base = name.rsplit_once('.').unwrap().0;
        let path = format!("{}/tests/data/{base}.seek.txt", env!("CARGO_MANIFEST_DIR"));
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
        let mut seeks: Vec<SeekAnswer> = Vec::new();
        for line in text.lines().filter(|l| !l.starts_with('#')) {
            let w: Vec<&str> = line.split(' ').collect();
            match w[0] {
                "seek" => seeks.push((w[1].parse().unwrap(), Vec::new())),
                "packet" => seeks.last_mut().unwrap().1.push((
                    w[1].parse().unwrap(),
                    (w[2] != "N/A").then(|| w[2].parse().unwrap()),
                    w[3].starts_with('K'),
                )),
                other => panic!("{path}: a line this test does not know: {other}"),
            }
        }
        let fixture = format!("{}/tests/data/{name}", env!("CARGO_MANIFEST_DIR"));
        for (ms, expected) in seeks {
            // A fresh demuxer, as each ffprobe run is one.
            let mut d = Demuxer::open(File::open(&fixture).unwrap()).unwrap();
            let numbers: Vec<u64> = d.tracks().iter().map(|t| t.number).collect();
            // FFmpeg's default stream: the first video track.
            let video = d
                .tracks()
                .iter()
                .find(|t| t.kind == matroska::TrackKind::Video)
                .map_or(numbers[0], |t| t.number);
            let (num, den) = d.time_base(video).unwrap();
            let ticks = ms * i64::try_from(den).unwrap() / (1000 * i64::try_from(num).unwrap());
            d.seek(video, ticks).unwrap();
            let mut got = Vec::new();
            while got.len() < expected.len() {
                let Some(p) = d.next_packet().unwrap() else {
                    break;
                };
                let stream = numbers.iter().position(|&n| n == p.track).unwrap();
                got.push((stream, p.timestamp, p.keyframe));
            }
            assert_eq!(got, expected, "{name}: after a seek to {ms} ms");
        }
    }
}
