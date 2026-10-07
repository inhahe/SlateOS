//! The metadata FFmpeg makes of a file -- the file's, each track's, chapter's
//! and attachment's -- and its chapters and attachments, held to `ffprobe`'s
//! (`tests/data/generate_fixtures.py`, `metadata_fixtures`, which says what
//! each file puts to the test); and what `ffprobe` cannot show.
//!
//! An answer (`NAME.meta.txt`) is a line per thing ffprobe shows: the file's
//! start (which FFmpeg ends chapters by), its tags, each stream -- a track,
//! or an attachment or a picture with its size and MD5 -- with its tags, and
//! each chapter (its ID, start and end) with its tags; bytes outside
//! printable ASCII written `\xHH`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    reason = "a test: a failure should be loud, and its numbers are small"
)]

use std::io::Cursor;

use matroska::{AttachmentKind, Demuxer, Metadata};

fn data(name: &str) -> String {
    format!("{}/tests/data/{name}", env!("CARGO_MANIFEST_DIR"))
}

/// Bytes as the answers write them.
fn esc(b: &[u8]) -> String {
    b.iter()
        .map(|&c| {
            if (0x20..0x7f).contains(&c) && c != b'\\' {
                char::from(c).to_string()
            } else {
                format!("\\x{c:02x}")
            }
        })
        .collect()
}

fn tags(lines: &mut Vec<String>, m: &Metadata) {
    for (k, v) in m.iter() {
        lines.push(format!("tag {}={}", esc(k), esc(v)));
    }
}

/// FFmpeg's name for what an attachment is.
fn codec(kind: AttachmentKind) -> &'static str {
    match kind {
        AttachmentKind::Gif => "gif",
        AttachmentKind::Jpeg => "mjpeg",
        AttachmentKind::Png => "png",
        AttachmentKind::Tiff => "tiff",
        AttachmentKind::TrueType => "ttf",
        AttachmentKind::OpenType => "otf",
        AttachmentKind::Binary => "bin_data",
        AttachmentKind::Other => "none",
    }
}

/// What the crate makes of `bytes`, as the answers write what ffprobe shows
/// -- all but the `start` line, which is ffprobe's to give: the file's start
/// in microseconds, by which the chapters without ends are ended.
fn dump(bytes: &[u8], start: Option<i64>) -> Vec<String> {
    let Ok(mut d) = Demuxer::open(Cursor::new(bytes.to_vec())) else {
        return vec!["refused".to_owned()];
    };
    let mut lines = vec!["format".to_owned()];
    tags(&mut lines, d.metadata());
    for (i, t) in d.tracks().iter().enumerate() {
        lines.push(format!("stream {i} track"));
        tags(&mut lines, &t.metadata);
    }
    let first = d.tracks().len();
    let attachments = d.attachments().to_vec();
    for (j, a) in attachments.iter().enumerate() {
        let bytes = d.attachment_data(j).unwrap();
        assert_eq!(bytes.len() as u64, a.size);
        let what = if a.kind.is_picture() {
            "picture"
        } else {
            "attachment"
        };
        lines.push(format!(
            "stream {} {what} {} {} {}",
            first + j,
            codec(a.kind),
            bytes.len(),
            md5::md5_hex(&bytes)
        ));
        tags(&mut lines, &a.metadata);
    }
    let ends = d.chapter_ends(start.map(|us| us * 1000));
    assert_eq!(ends.len(), d.chapters().len());
    for (c, end) in d.chapters().iter().zip(ends) {
        lines.push(format!("chapter {} {} {end}", c.uid.cast_signed(), c.start));
        tags(&mut lines, &c.metadata);
    }
    lines
}

/// `start`'s value in an answer.
fn start_of(word: &str) -> Option<i64> {
    (word != "none").then(|| word.parse().unwrap())
}

/// The file's metadata, streams and chapters, as ffprobe shows them.
fn holds_to_ffprobe(name: &str) {
    let base = name.rsplit_once('.').unwrap().0;
    let path = data(&format!("{base}.meta.txt"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let mut lines = text.lines().filter(|l| !l.starts_with('#'));
    let start = lines.next().unwrap().strip_prefix("start ").unwrap();
    let expected: Vec<&str> = lines.collect();
    let bytes = std::fs::read(data(name)).unwrap();
    let got = dump(&bytes, start_of(start));
    for (i, (g, e)) in got.iter().zip(&expected).enumerate() {
        assert_eq!(g, e, "{name}: line {i} of what ffprobe shows");
    }
    assert_eq!(got.len(), expected.len(), "{name}: the number of lines");
}

#[test]
fn metadata_of_ffmpeg_s_file() {
    holds_to_ffprobe("meta_ffmpeg.mkv");
}

#[test]
fn metadata_of_tags() {
    holds_to_ffprobe("meta_tags.mkv");
}

#[test]
fn metadata_of_chapters() {
    holds_to_ffprobe("meta_chapters.mkv");
}

#[test]
fn metadata_of_attachments() {
    holds_to_ffprobe("meta_attachments.mkv");
}

#[test]
fn metadata_of_two_infos() {
    holds_to_ffprobe("meta_info.mkv");
}

#[test]
fn metadata_of_a_date() {
    holds_to_ffprobe("meta_date.mkv");
}

#[test]
fn metadata_of_a_damaged_info() {
    holds_to_ffprobe("meta_damaged_info.mkv");
}

#[test]
fn metadata_of_two_tracks_elements() {
    holds_to_ffprobe("meta_two_tracks.mkv");
}

#[test]
fn metadata_through_the_seek_head() {
    holds_to_ffprobe("meta_seek_head.mkv");
}

#[test]
fn metadata_where_the_seek_head_names_the_wrong_element() {
    holds_to_ffprobe("meta_seek_mismatch.mkv");
}

#[test]
fn metadata_after_the_clusters() {
    holds_to_ffprobe("meta_trailing.mkv");
}

/// A file whose metadata comes after its Clusters, cut short at every
/// length through it: what was read before each cut is kept as FFmpeg keeps
/// it -- a number cut short as far as it was read, a string cut short not
/// at all, an attachment whose data is cut short left out -- and the
/// SeekHead followed no further.
#[test]
fn metadata_cut_short_anywhere_is_ffmpeg_s() {
    let bytes = std::fs::read(data("meta_trailing.mkv")).unwrap();
    let text = std::fs::read_to_string(data("meta_trailing.cut.txt")).unwrap();
    let mut cuts = 0;
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        let w: Vec<&str> = line.split(' ').collect();
        let n: usize = w[1].parse().unwrap();
        let start = if w[2] == "refused" {
            None
        } else {
            start_of(w[2])
        };
        let got = dump(&bytes[..n], start);
        if w[2] == "refused" {
            assert_eq!(got, ["refused"], "cut at {n}");
        } else {
            let text = got.join("\n") + "\n";
            assert_eq!(
                md5::md5_hex(text.as_bytes()).to_string(),
                w[3],
                "cut at {n}: {got:#?}"
            );
        }
        cuts += 1;
    }
    assert!(cuts > 600, "{cuts} cuts");
}

// --- Beyond ffprobe -----------------------------------------------------------

/// An element: its ID's bytes, a one-byte size, and its body (under 127
/// bytes).
fn el(id: &[u8], body: &[u8]) -> Vec<u8> {
    [id, &[0x80 | u8::try_from(body.len()).unwrap()], body].concat()
}

/// An element with an eight-byte size.
fn big(id: &[u8], body: &[u8]) -> Vec<u8> {
    let mut size = (body.len() as u64).to_be_bytes();
    size[0] = 0x01;
    [id, &size, body].concat()
}

fn simple_tag(name: &[u8], value: &[u8]) -> Vec<u8> {
    el(
        &[0x67, 0xC8],
        &[el(&[0x45, 0xA3], name), el(&[0x44, 0x87], value)].concat(),
    )
}

const SEGMENT: &[u8] = &[0x18, 0x53, 0x80, 0x67];

/// The EBML header; and an Info and a Tracks of one Snow track.
fn head() -> (Vec<u8>, Vec<u8>) {
    let header = el(&[0x1A, 0x45, 0xDF, 0xA3], &el(&[0x42, 0x82], b"matroska"));
    let info = el(
        &[0x15, 0x49, 0xA9, 0x66],
        &el(&[0x2A, 0xD7, 0xB1], &[0x0F, 0x42, 0x40]),
    );
    let entry = [
        el(&[0xD7], &[1]),
        el(&[0x73, 0xC5], &[1]),
        el(&[0x83], &[1]),
        el(&[0x86], b"V_SNOW"),
    ]
    .concat();
    let tracks = el(&[0x16, 0x54, 0xAE, 0x6B], &el(&[0xAE], &entry));
    (header, [info, tracks].concat())
}

/// A Cluster of one key frame, `k`.
fn key_cluster() -> Vec<u8> {
    el(
        &[0x1F, 0x43, 0xB6, 0x75],
        &[el(&[0xE7], &[0]), el(&[0xA3], &[0x81, 0, 0, 0x80, b'k'])].concat(),
    )
}

/// Chapters of one chapter, titled `title`.
fn one_chapter(title: &[u8]) -> Vec<u8> {
    let atom = el(
        &[0xB6],
        &[
            el(&[0x73, 0xC4], &[9]),
            el(&[0x91], &[0]),
            el(&[0x80], &el(&[0x85], title)),
        ]
        .concat(),
    );
    el(&[0x10, 0x43, 0xA7, 0x70], &el(&[0x45, 0xB9], &atom))
}

/// Tags whose one tag's string runs past its SimpleTag.
fn damaged_tags() -> Vec<u8> {
    let bad_simple = el(
        &[0x67, 0xC8],
        &[
            el(&[0x45, 0xA3], b"AFTER"),
            vec![0x44, 0x87, 0xC8],
            b"lost".to_vec(),
        ]
        .concat(),
    );
    el(&[0x12, 0x54, 0xC3, 0x67], &el(&[0x73, 0x73], &bad_simple))
}

/// A file of one Snow track and a Cluster of a key frame, with `before`
/// between the Tracks and the Cluster.
fn file_with(before: &[u8]) -> Vec<u8> {
    let (header, description) = head();
    [
        header,
        big(
            SEGMENT,
            &[description, before.to_vec(), key_cluster()].concat(),
        ),
    ]
    .concat()
}

/// Tags damaged before the first Cluster -- the second tag's string runs
/// past it -- keep the first tag, and reading goes on: to Chapters after
/// them, and to the packets. (FFmpeg reads the Segment again from its start
/// after such damage, which also gives it the track twice; that is not
/// followed: design-decisions §1358.)
#[test]
fn damage_before_the_first_cluster_keeps_what_was_read() {
    let good = el(&[0x73, 0x73], &simple_tag(b"BEFORE", b"damage"));
    let bad = damaged_tags();
    // The good tag and the damaged one, in one Tags element.
    let tags = el(
        &[0x12, 0x54, 0xC3, 0x67],
        &[good, bad[5..].to_vec()].concat(),
    );
    let bytes = file_with(&[tags, one_chapter(b"After")].concat());
    let mut d = Demuxer::open(Cursor::new(bytes)).unwrap();
    assert_eq!(d.metadata().get(b"before"), Some(&b"damage"[..]));
    assert_eq!(d.metadata().get(b"AFTER"), None, "its string never read");
    assert_eq!(d.tracks().len(), 1);
    assert_eq!(d.chapters().len(), 1);
    assert_eq!(d.chapters()[0].metadata.get(b"title"), Some(&b"After"[..]));
    assert_eq!(d.next_packet().unwrap().unwrap().data, b"k");
}

/// After damage before the first Cluster, the Segment's stated end bounds
/// nothing more: as FFmpeg, reading goes on to the end of the file. Here a
/// Segment says it ends with its damaged Tags, and Chapters and a Cluster
/// follow it.
#[test]
fn after_damage_the_segment_runs_to_the_end_of_the_file() {
    let (header, description) = head();
    let segment = big(SEGMENT, &[description, damaged_tags()].concat());
    let bytes = [header, segment, one_chapter(b"Past the end"), key_cluster()].concat();
    let mut d = Demuxer::open(Cursor::new(bytes)).unwrap();
    assert_eq!(d.chapters().len(), 1);
    assert_eq!(
        d.chapters()[0].metadata.get(b"title"),
        Some(&b"Past the end"[..])
    );
    assert_eq!(d.next_packet().unwrap().unwrap().data, b"k");
}

/// A tag nested deeper than FFmpeg reads -- 13 SimpleTags deep is the most
/// before the first Cluster -- ends the Tags there, the tags before it kept.
#[test]
fn tags_nest_no_deeper_than_ffmpeg_reads() {
    let nested = |depth: usize| {
        let mut t = big(
            &[0x67, 0xC8],
            &[el(&[0x45, 0xA3], b"N"), el(&[0x44, 0x87], b"v")].concat(),
        );
        for _ in 1..depth {
            t = big(
                &[0x67, 0xC8],
                &[el(&[0x45, 0xA3], b"N"), el(&[0x44, 0x87], b"v"), t].concat(),
            );
        }
        let first = el(&[0x73, 0x73], &simple_tag(b"FIRST", b"kept"));
        big(
            &[0x12, 0x54, 0xC3, 0x67],
            &[first, big(&[0x73, 0x73], &t)].concat(),
        )
    };
    let d = Demuxer::open(Cursor::new(file_with(&nested(13)))).unwrap();
    let key = vec!["N"; 13].join("/");
    assert_eq!(d.metadata().get(key.as_bytes()), Some(&b"v"[..]));
    let d = Demuxer::open(Cursor::new(file_with(&nested(14)))).unwrap();
    assert_eq!(d.metadata().get(b"FIRST"), Some(&b"kept"[..]));
    assert_eq!(
        d.metadata().get(key.as_bytes()),
        Some(&b"v"[..]),
        "the 13 before the 14th"
    );
    let deeper = vec!["N"; 14].join("/");
    assert_eq!(d.metadata().get(deeper.as_bytes()), None, "the 14th itself");
}

/// An attachment's bytes are read when asked for, and reading packets goes
/// on where it was.
#[test]
fn reading_an_attachment_leaves_the_packets_where_they_were() {
    let open =
        || Demuxer::open(std::fs::File::open(data("meta_attachments.mkv")).unwrap()).unwrap();
    let mut plain = open();
    let mut with = open();
    let first = plain.next_packet().unwrap().unwrap();
    assert_eq!(with.next_packet().unwrap().unwrap(), first);
    let font = with.attachment_data(1).unwrap();
    assert_eq!(font, b"font bytes");
    assert_eq!(with.next_packet().unwrap(), plain.next_packet().unwrap());
    assert!(with.attachment_data(99).is_err(), "no such attachment");
}

/// Kept in the file's order, the pictures among them, with what they are.
#[test]
fn attachments_say_what_they_are() {
    let mut d = Demuxer::open(std::fs::File::open(data("meta_attachments.mkv")).unwrap()).unwrap();
    let kinds: Vec<(Vec<u8>, AttachmentKind, bool)> = d
        .attachments()
        .iter()
        .map(|a| (a.name.clone(), a.kind, a.kind.is_picture()))
        .collect();
    assert_eq!(
        kinds,
        [
            (b"cover.png".to_vec(), AttachmentKind::Png, true),
            (b"font.ttf".to_vec(), AttachmentKind::TrueType, false),
            (b"".to_vec(), AttachmentKind::Binary, false),
            (b"photo.jpg".to_vec(), AttachmentKind::Jpeg, true),
            (b"open.otf".to_vec(), AttachmentKind::OpenType, false),
            (b"font2.ttf".to_vec(), AttachmentKind::TrueType, false),
            (b"notes.txt".to_vec(), AttachmentKind::Other, false),
        ]
    );
    let photo = &d.attachments()[3];
    assert_eq!(photo.description.as_deref(), Some(&b"A photo"[..]));
    assert_eq!(photo.media_type, b"image/jpeg; q=1");
    let png = d.attachment_data(0).unwrap();
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
}

/// A chapter's end where the file gives none comes from the file's start
/// the caller gives: FFmpeg ends the last chapter at the start plus the
/// duration.
#[test]
fn the_last_chapter_ends_at_the_start_plus_the_duration() {
    let d = Demuxer::open(std::fs::File::open(data("meta_seek_head.mkv")).unwrap()).unwrap();
    assert_eq!(d.chapters()[0].end, None);
    assert_eq!(d.chapter_ends(None), [1_000_000_000]);
    assert_eq!(d.chapter_ends(Some(0)), [1_000_000_000]);
    assert_eq!(d.chapter_ends(Some(250_000_000)), [1_250_000_000]);
    // To FFmpeg's microsecond.
    assert_eq!(d.chapter_ends(Some(1_499)), [1_000_001_000]);
}
