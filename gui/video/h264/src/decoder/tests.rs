//! The decoder's handling of pictures held to FFmpeg's on every conformance
//! stream: fed the packets FFmpeg's demuxer splits each into, every slice's
//! `-debug pict` line -- its picture's frame number and order counts, its
//! reference counts, QP, loop filter and weights -- every `-debug mmco`
//! line -- the reference lists and each marking operation -- and every
//! picture shown, in order, as ffprobe describes it, must be FFmpeg's.
//!
//! The streams are FFmpeg's FATE samples, too large to commit, and the
//! expected lines come from `tools/pict_expect.py`; with both on disk:
//!
//! ```text
//! H264_FATE=<fate-suite>/h264-conformance H264_PICT=<pict_expect.py's output> \
//!   cargo test -p h264 --target x86_64-pc-windows-gnu --lib -- --ignored pictures
//! ```

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use std::path::{Path, PathBuf};

use crate::Decoder;

/// Every `.pict` file under `dir`.
fn pict_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            pict_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "pict") {
            out.push(path);
        }
    }
}

/// FFmpeg's picture type letter (`av_get_picture_type_char`).
fn type_char(t: i32) -> char {
    use crate::slice::{TYPE_B, TYPE_I, TYPE_P, TYPE_SI, TYPE_SP};
    match t {
        TYPE_I => 'I',
        TYPE_P => 'P',
        TYPE_B => 'B',
        TYPE_SI => 'i',
        TYPE_SP => 'p',
        _ => '?',
    }
}

/// What this decoder makes of a stream fed as FFmpeg's demuxer splits it:
/// each slice's line, and each picture handed out as ffprobe describes one
/// -- the position of the packet that began it, its type, and whether it
/// is a key frame, interlaced, top field first, and how many fields it
/// repeats.
fn run(stream: &Path, packets: &Path) -> (Vec<String>, Vec<String>, Vec<String>) {
    let data = std::fs::read(stream).unwrap();
    let sizes: Vec<usize> = std::fs::read_to_string(packets)
        .unwrap()
        .lines()
        .map(|l| l.trim().parse().unwrap())
        .collect();
    let mut positions = Vec::with_capacity(sizes.len());
    let mut d = Decoder::new();
    // What FFmpeg's probe found the stream reorders by, which the decoder
    // that shows it starts from.
    let delay =
        std::fs::read_to_string(stream_delay(packets)).map_or(0, |t| t.trim().parse().unwrap());
    d.set_reorder_delay(delay);
    let mut frames = Vec::new();
    let mut at = 0;
    let row = |p: crate::Picture, positions: &[usize]| {
        let o = &p.out;
        let pos = positions[usize::try_from(o.props.time).unwrap()];
        format!(
            "{pos} {} {} {} {} {}",
            type_char(o.props.pict_type),
            i32::from(o.props.key),
            i32::from(o.props.interlaced),
            i32::from(o.props.top_field_first),
            o.props.repeat_pict
        )
    };
    for (i, size) in sizes.into_iter().enumerate() {
        positions.push(at);
        let packet = &data[at..at + size];
        at += size;
        // A packet with no picture in it is FFmpeg's "no frame!" error,
        // which the stream goes on past.
        if let Ok(Some(p)) = d.decode(packet, i64::try_from(i).unwrap(), 1) {
            frames.push(row(p, &positions));
        }
    }
    while let Some(p) = d.drain() {
        frames.push(row(p, &positions));
    }
    (
        std::mem::take(&mut d.h.debug),
        frames,
        std::mem::take(&mut d.h.debug_mmco),
    )
}

/// The `.delay` file beside a `.packets` one.
fn stream_delay(packets: &Path) -> PathBuf {
    packets.with_extension("delay")
}

/// The first line where `got` is not `want`, described.
fn first_difference(what: &str, want: &[String], got: &[String]) -> Option<String> {
    (0..want.len().max(got.len()))
        .find(|&k| want.get(k) != got.get(k))
        .map(|k| {
            format!(
                "{what} {k} of {}: FFmpeg {:?}, here {:?}",
                want.len(),
                want.get(k),
                got.get(k)
            )
        })
}

fn lines(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect()
}

#[test]
#[ignore = "needs FFmpeg's FATE samples and tools/pict_expect.py's lines: set H264_FATE and H264_PICT"]
fn pictures_are_set_up_and_shown_as_ffmpeg_does() {
    let fate = PathBuf::from(
        std::env::var("H264_FATE").expect("H264_FATE: the h264-conformance directory"),
    );
    let pict =
        PathBuf::from(std::env::var("H264_PICT").expect("H264_PICT: pict_expect.py's output"));
    let mut files = Vec::new();
    pict_files(&pict, &mut files);
    files.sort();
    assert!(!files.is_empty(), "no .pict files under {}", pict.display());

    let mut failures = Vec::new();
    let (mut slices, mut shown) = (0, 0);
    for file in &files {
        let rel = file.strip_prefix(&pict).unwrap();
        let name = rel.to_string_lossy().trim_end_matches(".pict").to_owned();
        let (got_slices, got_frames, got_mmco) =
            run(&fate.join(&name), &file.with_extension("packets"));
        let want_slices = lines(file);
        let want_frames = lines(&file.with_extension("frames"));
        let want_mmco = lines(&file.with_extension("mmco"));
        slices += want_slices.len();
        shown += want_frames.len();
        for d in [
            first_difference("slice", &want_slices, &got_slices),
            first_difference("picture shown", &want_frames, &got_frames),
            first_difference("marking line", &want_mmco, &got_mmco),
        ]
        .into_iter()
        .flatten()
        {
            failures.push(format!("{name}: {d}"));
        }
    }
    let report = failures.join("\n");
    assert!(
        failures.is_empty(),
        "{} differences over {} streams ({slices} slices, {shown} pictures shown):\n{report}",
        failures.len(),
        files.len()
    );
}
