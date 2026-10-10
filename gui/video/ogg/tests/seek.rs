//! Seeking, held to reading through: after `seek(stream, t)`, the stream's
//! packets are those a read from the start gives, from the first to end
//! after the last page whose granule position is at or before `t` (found
//! here by a page walk of the test's own) -- each with the same time,
//! length, bytes and trimming.
//!
//! What a seek may give differently, by design: the packet it lands on,
//! for Vorbis, lasts what the block before it says only where that block's
//! packet begins on the page the seek found (else its window's word, or the
//! short size, is taken for it); after a seek to a link's start, its first
//! packet carries the stream's pre-skip again; and in a chained file the
//! packet a seek lands on carries its link's headers.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test: a failure should be loud, and its numbers are small"
)]

use std::fs::File;

use ogg::{Codec, Demuxer, Packet};

fn data(name: &str) -> String {
    format!("{}/tests/data/{name}", env!("CARGO_MANIFEST_DIR"))
}

fn all(d: &mut Demuxer<File>) -> Vec<Packet> {
    let mut out = Vec::new();
    while let Some(p) = d.next_packet().unwrap() {
        out.push(p);
    }
    out
}

/// A page's facts, from a walk of the file's bytes: position, serial,
/// granule position, and whether a packet begun on it ends on a later one.
struct PageFacts {
    position: u64,
    serial: u32,
    granule: i64,
}

fn walk(file: &[u8]) -> Vec<PageFacts> {
    let mut out = Vec::new();
    let mut at = 0;
    while at + 27 <= file.len() {
        assert_eq!(&file[at..at + 4], b"OggS", "a page at {at}");
        let granule = i64::from_le_bytes(file[at + 6..at + 14].try_into().unwrap());
        let serial = u32::from_le_bytes(file[at + 14..at + 18].try_into().unwrap());
        let n = usize::from(file[at + 26]);
        let size: usize = file[at + 27..at + 27 + n]
            .iter()
            .map(|&l| usize::from(l))
            .sum();
        out.push(PageFacts {
            position: at as u64,
            serial,
            granule,
        });
        at += 27 + n + size;
    }
    out
}

/// Seeks `name`'s stream `stream` to each of `times` and checks what follows.
fn seeks_as_read_through(name: &str, stream: usize) {
    let file = std::fs::read(data(name)).unwrap();
    let mut d = Demuxer::open(File::open(data(name)).unwrap()).unwrap();
    let whole: Vec<Packet> = all(&mut d)
        .into_iter()
        .filter(|p| p.stream == stream)
        .collect();
    let info = d.streams()[stream].clone();
    let chained = d.links() > 1;
    let first_time = whole.iter().find_map(|p| p.pts).unwrap();
    let last_time = whole.iter().rev().find_map(|p| p.pts).unwrap();
    let span = last_time - first_time;
    // The stream's data pages: from the one its first packet begins on (its
    // header pages, at granule 0, are not where a seek can land).
    let data_start = whole[0].page_position;
    let pages: Vec<PageFacts> = walk(&file)
        .into_iter()
        .filter(|p| p.serial == info.serial && p.granule != -1 && p.position >= data_start)
        .collect();
    let pre_skip = i64::from(info.pre_skip);
    let mut times = vec![first_time - 1000, first_time, 0, 1];
    for k in 1..=9 {
        times.push(first_time + span * k / 10);
    }
    times.push(last_time);
    times.push(last_time + 100_000);
    for t in times {
        d.seek(stream, t).unwrap();
        let after: Vec<Packet> = all(&mut d)
            .into_iter()
            .filter(|p| p.stream == stream)
            .collect();
        // At or past the stream's last page, nothing is left. (A chained
        // file's last page is its last link's, on that link's own clock:
        // there, past the last packet's time.)
        let past_end = if chained {
            t > last_time && after.is_empty()
        } else {
            pages.last().is_some_and(|p| p.granule - pre_skip <= t)
        };
        if past_end {
            assert!(after.is_empty(), "{name} at {t}: packets past the end");
            continue;
        }
        let first = after
            .first()
            .unwrap_or_else(|| panic!("{name} at {t}: nothing after the seek"));
        // Where it landed, in the read-through: the packet that begins where
        // this one does.
        let j = whole
            .iter()
            .position(|p| p.position == first.position)
            .unwrap_or_else(|| {
                panic!(
                    "{name} at {t}: a packet at {} the read-through has not",
                    first.position
                )
            });
        assert_eq!(
            after.len(),
            whole.len() - j,
            "{name} at {t}: the packets after the seek"
        );
        // The page the seek should have found (in an unchained file, whose
        // times are its granules'): the last at or before the time.
        if !chained {
            let found = pages.iter().rev().find(|p| p.granule - pre_skip <= t);
            match found {
                Some(page) => {
                    assert_eq!(
                        first.pts,
                        Some(page.granule - pre_skip),
                        "{name} at {t}: the packet after page {}",
                        page.position
                    );
                    // And no later page's packet would do.
                    assert!(
                        whole[j..]
                            .iter()
                            .filter_map(|p| p.pts)
                            .all(|pts| pts >= page.granule - pre_skip),
                        "{name} at {t}"
                    );
                }
                None => assert_eq!(j, 0, "{name} at {t}: before every page, the stream's start"),
            }
        }
        for (k, (ours, theirs)) in after.iter().zip(&whole[j..]).enumerate() {
            let mut want = theirs.clone();
            if k == 0 {
                if info.codec == Codec::Vorbis {
                    // Its length depends on a block the seek may not have read.
                    want.duration = ours.duration;
                }
                if j > 0 || !chained {
                    want.new_headers = ours.new_headers.clone();
                }
                if chained {
                    assert!(
                        ours.new_headers.is_some(),
                        "{name} at {t}: a link's headers after a seek"
                    );
                    want.new_headers = ours.new_headers.clone();
                }
            }
            assert_eq!(ours, &want, "{name} at {t}: packet {k} after the seek");
        }
    }
}

#[test]
fn opus_seeks() {
    for name in [
        "opus_stereo.opus",
        "opus_mono_silk.opus",
        "opus_short_frames.ogg",
        "opus_51.opus",
        "opus_spanning.opus",
        "opus_one_page.opus",
    ] {
        seeks_as_read_through(name, 0);
    }
}

#[test]
fn vorbis_seeks() {
    for name in [
        "vorbis_stereo.ogg",
        "vorbis_mono_22k.ogg",
        "vorbis_51.ogg",
        "vorbis_spanning.ogg",
        "vorbis_native.ogg",
        "vorbis_one_page.ogg",
        "vorbis_headers_with_data.ogg",
    ] {
        seeks_as_read_through(name, 0);
    }
}

#[test]
fn a_films_sound_seeks() {
    seeks_as_read_through("theora_vorbis.ogv", 1);
}

#[test]
fn chained_files_seek_across_their_links() {
    for name in ["opus_chained.opus", "vorbis_chained.ogg"] {
        seeks_as_read_through(name, 0);
    }
}

#[test]
fn a_seek_in_a_stream_the_file_has_not_is_refused() {
    let mut d = Demuxer::open(File::open(data("opus_stereo.opus")).unwrap()).unwrap();
    assert_eq!(
        d.seek(1, 0),
        Err(ogg::Error::Invalid(
            "a seek in a stream the file does not have"
        ))
    );
}
