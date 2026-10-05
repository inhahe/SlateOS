//! Every fixture's packets, held to `ffprobe`'s: what FFmpeg's demuxer makes
//! of each (`tests/data/generate_fixtures.py`, which says how each fixture
//! was made and what it is for). A test per fixture, so that a failure names
//! the file, and so that `mutate.py`'s table can say which file catches what.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test: a failure should be loud, and its numbers are small"
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

fn data(name: &str) -> String {
    format!("{}/tests/data/{name}", env!("CARGO_MANIFEST_DIR"))
}

/// A fixture's answers: each stream's time base, and the packets.
fn answers(name: &str) -> (Vec<(u64, u64)>, Vec<Line>) {
    let base = name.rsplit_once('.').unwrap().0;
    let path = data(&format!("{base}.txt"));
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

/// The fixture's streams and every packet -- the stream, timestamp,
/// duration, size, position, key-frame flag, bytes and additions -- as
/// FFmpeg's demuxer gives them.
fn demuxes_as_ffmpeg_does(name: &str) {
    let mut d =
        Demuxer::open(File::open(data(name)).unwrap()).unwrap_or_else(|e| panic!("{name}: {e}"));
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

// Written by ffmpeg.

#[test]
fn packets_of_vp9_opus() {
    demuxes_as_ffmpeg_does("vp9_opus.webm");
}

#[test]
fn packets_of_av1() {
    demuxes_as_ffmpeg_does("av1.webm");
}

#[test]
fn packets_of_vp8_vorbis() {
    demuxes_as_ffmpeg_does("vp8_vorbis.webm");
}

#[test]
fn packets_of_vp9_alpha() {
    demuxes_as_ffmpeg_does("vp9_alpha.webm");
}

#[test]
fn packets_of_live() {
    demuxes_as_ffmpeg_does("live.webm");
}

#[test]
fn packets_of_subtitles() {
    demuxes_as_ffmpeg_does("subtitles.mkv");
}

// Written byte by byte.

#[test]
fn packets_of_laced() {
    demuxes_as_ffmpeg_does("laced.mkv");
}

#[test]
fn packets_of_laced_untimed() {
    demuxes_as_ffmpeg_does("laced_untimed.mkv");
}

#[test]
fn packets_of_compressed() {
    demuxes_as_ffmpeg_does("compressed.mkv");
}

#[test]
fn packets_of_unknown_sizes() {
    demuxes_as_ffmpeg_does("unknown_sizes.mkv");
}

#[test]
fn packets_of_damaged() {
    demuxes_as_ffmpeg_does("damaged.mkv");
}

#[test]
fn packets_of_empty_frames() {
    demuxes_as_ffmpeg_does("empty_frames.mkv");
}

#[test]
fn packets_of_zlib_laces() {
    demuxes_as_ffmpeg_does("zlib_laces.mkv");
}

#[test]
fn packets_of_overrun() {
    demuxes_as_ffmpeg_does("overrun.mkv");
}

#[test]
fn packets_of_ignored_tracks() {
    demuxes_as_ffmpeg_does("ignored_tracks.mkv");
}

#[test]
fn packets_of_tracks_at_the_end() {
    demuxes_as_ffmpeg_does("tracks_at_the_end.mkv");
}

#[test]
fn packets_of_reordered() {
    demuxes_as_ffmpeg_does("reordered.mkv");
}

#[test]
fn packets_of_resync_point() {
    demuxes_as_ffmpeg_does("resync_point.mkv");
}

#[test]
fn packets_of_resync_in_group() {
    demuxes_as_ffmpeg_does("resync_in_group.mkv");
}

#[test]
fn packets_of_walk_damage() {
    demuxes_as_ffmpeg_does("walk_damage.mkv");
}

// mediaprobe's layouts.

#[test]
fn packets_of_order() {
    demuxes_as_ffmpeg_does("order.mkv");
}

#[test]
fn packets_of_lacings() {
    demuxes_as_ffmpeg_does("lacings.mkv");
}

#[test]
fn packets_of_bad_laces() {
    demuxes_as_ffmpeg_does("bad_laces.mkv");
}

#[test]
fn packets_of_groups() {
    demuxes_as_ffmpeg_does("groups.mkv");
}

#[test]
fn packets_of_unknown_clusters() {
    demuxes_as_ffmpeg_does("unknown_clusters.mkv");
}

#[test]
fn packets_of_cued() {
    demuxes_as_ffmpeg_does("cued.mkv");
}

#[test]
fn packets_of_uncued() {
    demuxes_as_ffmpeg_does("uncued.mkv");
}

#[test]
fn packets_of_sparse_cues() {
    demuxes_as_ffmpeg_does("sparse_cues.mkv");
}

#[test]
fn packets_of_cues_of_another_track() {
    demuxes_as_ffmpeg_does("cues_of_another_track.mkv");
}

#[test]
fn packets_of_one_cue_point() {
    demuxes_as_ffmpeg_does("one_cue_point.mkv");
}

#[test]
fn packets_of_unlisted_cues() {
    demuxes_as_ffmpeg_does("unlisted_cues.mkv");
}

#[test]
fn packets_of_codec_delay() {
    demuxes_as_ffmpeg_does("codec_delay.mkv");
}

#[test]
fn packets_of_pre_roll() {
    demuxes_as_ffmpeg_does("pre_roll.mkv");
}

#[test]
fn packets_of_delayed() {
    demuxes_as_ffmpeg_does("delayed.mkv");
}

#[test]
fn packets_of_tick_100us() {
    demuxes_as_ffmpeg_does("tick_100us.mkv");
}

#[test]
fn packets_of_encodings() {
    demuxes_as_ffmpeg_does("encodings.mkv");
}

#[test]
fn packets_of_resync() {
    demuxes_as_ffmpeg_does("resync.mkv");
}

// The metadata fixtures (`tests/metadata.rs` holds their metadata), whose
// packets show the tracks and their ticks read as FFmpeg reads them: a
// second Info starting the timestamp scale afresh, a second Tracks adding
// its track.

#[test]
fn packets_of_meta_info() {
    demuxes_as_ffmpeg_does("meta_info.mkv");
}

#[test]
fn packets_of_meta_two_tracks() {
    demuxes_as_ffmpeg_does("meta_two_tracks.mkv");
}

#[test]
fn packets_of_meta_tags() {
    demuxes_as_ffmpeg_does("meta_tags.mkv");
}

#[test]
fn packets_of_meta_chapters() {
    demuxes_as_ffmpeg_does("meta_chapters.mkv");
}

#[test]
fn packets_of_meta_date() {
    demuxes_as_ffmpeg_does("meta_date.mkv");
}

// Cues the SeekHead points at twice -- the last entry's are FFmpeg's -- and
// Cues FFmpeg leaves unused because following the SeekHead failed after
// them.

// Elements of reserved IDs, which FFmpeg passes over as ones it does not
// know: before the first Cluster and inside it.

#[test]
fn packets_of_reserved_ids() {
    demuxes_as_ffmpeg_does("reserved_ids.mkv");
}

#[test]
fn packets_of_cues_last_entry() {
    demuxes_as_ffmpeg_does("cues_last_entry.mkv");
}

#[test]
fn packets_of_cues_broken() {
    demuxes_as_ffmpeg_does("cues_broken.mkv");
}

/// Every byte of a file with Cues, a laced one, one of unknown sizes and a
/// compressed one changed in turn -- its low bit, its high bit, and all of it
/// -- then each cut short at every length: each opens and reads to its end,
/// or is refused -- never a panic, never more packets or bytes than the file
/// could hold -- and a seek in each works or is refused.
#[test]
fn a_damaged_file_is_read_or_refused_but_never_panics() {
    for name in [
        "live.webm",
        "laced.mkv",
        "unknown_sizes.mkv",
        "compressed.mkv",
        "vp9_alpha.webm",
        "cued.mkv",
        "lacings.mkv",
        // Chapters, tags and attachments: before the Clusters, and after them
        // through the SeekHead.
        "meta_tags.mkv",
        "meta_trailing.mkv",
    ] {
        let bytes = std::fs::read(data(name)).unwrap();
        let read_all = |data: &[u8], what: &dyn Fn() -> String| {
            let Ok(mut d) = Demuxer::open(std::io::Cursor::new(data.to_vec())) else {
                return;
            };
            let (mut packets, mut total) = (0usize, 0usize);
            let mut bounded = |d: &mut Demuxer<_>| {
                while let Ok(Some(p)) = d.next_packet() {
                    packets += 1;
                    total += p.data.len();
                    // A packet is at least the byte of its block it came
                    // from, but zlib can grow one: bounded by the inflate
                    // limit.
                    assert!(packets <= data.len() * 256, "{name}: {}: packets", what());
                    assert!(
                        total <= data.len() * 1024 + (256 << 20),
                        "{name}: {}: bytes",
                        what()
                    );
                }
            };
            // The metadata, made when the file was opened, and the
            // attachments' bytes, read now: whatever they hold, no panic.
            // (A failed read is as good an outcome as any here.)
            let _ = d.chapter_ends(Some(0));
            for i in 0..d.attachments().len() {
                let _ = d.attachment_data(i);
            }
            bounded(&mut d);
            if let Some(t) = d.tracks().first().map(|t| t.number) {
                if d.seek(t, 50).is_ok() {
                    bounded(&mut d);
                }
                if d.seek(t, 1500).is_ok() {
                    bounded(&mut d);
                }
            }
        };
        let mut damaged = bytes.clone();
        for at in 0..bytes.len() {
            for mask in [0x01, 0x80, 0xff] {
                damaged[at] ^= mask;
                read_all(&damaged, &|| format!("byte {at} ^ {mask:#x}"));
                damaged[at] ^= mask;
            }
        }
        for len in 0..bytes.len() {
            read_all(&bytes[..len], &|| format!("cut at {len}"));
        }
    }
}

/// One seek's answer: where to, in milliseconds, and each packet after it
/// (its stream, timestamp and key-frame flag).
type SeekAnswer = (i64, Vec<(usize, Option<i64>, bool)>);

/// After a seek to each of the times `generate_fixtures.py` seeks to, the
/// first packets are FFmpeg's: the latest key frame at or before the time,
/// and what follows it, the earlier packets of every track dropped.
fn seeks_as_ffmpeg_does(name: &str) {
    let base = name.rsplit_once('.').unwrap().0;
    let path = data(&format!("{base}.seek.txt"));
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
    for (ms, expected) in seeks {
        // A fresh demuxer, as each ffprobe run is one.
        let mut d = Demuxer::open(File::open(data(name)).unwrap()).unwrap();
        let numbers: Vec<u64> = d.tracks().iter().map(|t| t.number).collect();
        // FFmpeg's default stream: in these files, the first video track,
        // or the first track when there is none.
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

#[test]
fn seeks_in_vp9_opus() {
    seeks_as_ffmpeg_does("vp9_opus.webm");
}

#[test]
fn seeks_in_av1() {
    seeks_as_ffmpeg_does("av1.webm");
}

#[test]
fn seeks_in_vp8_vorbis() {
    seeks_as_ffmpeg_does("vp8_vorbis.webm");
}

#[test]
fn seeks_in_live() {
    seeks_as_ffmpeg_does("live.webm");
}

#[test]
fn seeks_in_unknown_sizes() {
    seeks_as_ffmpeg_does("unknown_sizes.mkv");
}

#[test]
fn seeks_in_laced() {
    seeks_as_ffmpeg_does("laced.mkv");
}

#[test]
fn seeks_in_reordered() {
    seeks_as_ffmpeg_does("reordered.mkv");
}

#[test]
fn seeks_in_walk_damage() {
    seeks_as_ffmpeg_does("walk_damage.mkv");
}

#[test]
fn seeks_in_zlib_laces() {
    seeks_as_ffmpeg_does("zlib_laces.mkv");
}

#[test]
fn seeks_in_cued() {
    seeks_as_ffmpeg_does("cued.mkv");
}

#[test]
fn seeks_in_uncued() {
    seeks_as_ffmpeg_does("uncued.mkv");
}

#[test]
fn seeks_in_cues_of_another_track() {
    seeks_as_ffmpeg_does("cues_of_another_track.mkv");
}

#[test]
fn seeks_in_one_cue_point() {
    seeks_as_ffmpeg_does("one_cue_point.mkv");
}

#[test]
fn seeks_in_unlisted_cues() {
    seeks_as_ffmpeg_does("unlisted_cues.mkv");
}

#[test]
fn seeks_in_pre_roll() {
    seeks_as_ffmpeg_does("pre_roll.mkv");
}

#[test]
fn seeks_in_delayed() {
    seeks_as_ffmpeg_does("delayed.mkv");
}

#[test]
fn seeks_in_cues_last_entry() {
    seeks_as_ffmpeg_does("cues_last_entry.mkv");
}

#[test]
fn seeks_in_cues_broken() {
    seeks_as_ffmpeg_does("cues_broken.mkv");
}
