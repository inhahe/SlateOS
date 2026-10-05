//! The subtitle fixtures (`tests/data/subrip*`, `ass*`, `ssa*`, `webvtt*`,
//! `overlap*`) read through [`videocodec::Subtitles`]: every cue's start, end
//! and text held to the fixture's answer, `NAME.srt` -- ffmpeg's SRT of the
//! track (`NAME.ffmpeg.srt`) but for the cues where this follows the format's
//! own renderer instead, which `tests/data/generate_subtitle_fixtures.py`
//! lists and says why of, one by one.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test: a failure should be loud, and its sizes are small"
)]

use std::fs::File;
use std::io::Cursor;
use std::path::PathBuf;

use videocodec::{Cue, Error, SubtitleFormat, Subtitles};

fn data(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("data")
        .join(name)
}

/// `HH:MM:SS,mmm` in nanoseconds.
fn srt_time(t: &str) -> i64 {
    let (hms, ms) = t.split_once(',').unwrap();
    let mut parts = hms.split(':').map(|p| p.parse::<i64>().unwrap());
    let (h, m, s) = (
        parts.next().unwrap(),
        parts.next().unwrap(),
        parts.next().unwrap(),
    );
    (((h * 60 + m) * 60 + s) * 1000 + ms.parse::<i64>().unwrap()) * 1_000_000
}

/// An SRT file's cues. No answer's text holds a blank line, so a blank line
/// ends a cue.
fn answers(name: &str) -> Vec<Cue> {
    let srt = std::fs::read_to_string(data(name)).unwrap();
    srt.split("\n\n")
        .filter(|block| !block.trim().is_empty())
        .map(|block| {
            let mut lines = block.split('\n');
            let _number = lines.next().unwrap();
            let (start, end) = lines.next().unwrap().split_once(" --> ").unwrap();
            Cue {
                start: srt_time(start),
                end: srt_time(end),
                text: lines.collect::<Vec<_>>().join("\n"),
            }
        })
        .collect()
}

fn read_all(subtitles: &mut Subtitles<File>) -> Vec<Cue> {
    let mut cues = Vec::new();
    while let Some(cue) = subtitles.next_cue().unwrap() {
        cues.push(cue);
    }
    cues
}

/// Every cue of `file` against `NAME.srt`, every difference reported at once.
fn held_to_its_answer(name: &str, file: &str, format: SubtitleFormat) {
    let mut subtitles = Subtitles::open(File::open(data(file)).unwrap()).unwrap();
    assert_eq!(subtitles.info().format, format, "{file}");
    let got = read_all(&mut subtitles);
    let want = answers(&format!("{name}.srt"));
    let mut wrong = Vec::new();
    for (i, (w, g)) in want.iter().zip(&got).enumerate() {
        if w != g {
            wrong.push(format!(
                "cue {}:\n  want {} --> {} {:?}\n  got  {} --> {} {:?}",
                i + 1,
                w.start,
                w.end,
                w.text,
                g.start,
                g.end,
                g.text
            ));
        }
    }
    assert!(
        wrong.is_empty() && want.len() == got.len(),
        "{file}: {} cues read, {} wanted; these differ:\n{}",
        got.len(),
        want.len(),
        wrong.join("\n")
    );
    assert_eq!(subtitles.damaged(), 0, "{file}");
}

#[test]
fn subrip() {
    held_to_its_answer("subrip", "subrip.mkv", SubtitleFormat::SubRip);
}

#[test]
fn subrip_colour_names() {
    held_to_its_answer(
        "subrip_colours",
        "subrip_colours.mkv",
        SubtitleFormat::SubRip,
    );
}

#[test]
fn ass() {
    held_to_its_answer("ass", "ass.mkv", SubtitleFormat::Ass);
}

#[test]
fn ass_laid_out_for_720_lines() {
    held_to_its_answer("ass_720", "ass_720.mkv", SubtitleFormat::Ass);
}

#[test]
fn ssa() {
    held_to_its_answer("ssa", "ssa.mkv", SubtitleFormat::Ssa);
}

#[test]
fn webvtt_in_webm() {
    held_to_its_answer("webvtt", "webvtt.webm", SubtitleFormat::WebVtt);
}

#[test]
fn webvtt_in_matroska() {
    held_to_its_answer("webvtt_mkv", "webvtt_mkv.mkv", SubtitleFormat::WebVtt);
}

#[test]
fn overlapping_cues() {
    held_to_its_answer("overlap", "overlap.mkv", SubtitleFormat::SubRip);
}

#[test]
fn timed_text_in_mp4_as_ffmpeg_writes_it() {
    held_to_its_answer("movtext", "movtext.mp4", SubtitleFormat::MovText);
}

#[test]
fn timed_text_style_runs() {
    held_to_its_answer(
        "movtext_styles",
        "movtext_styles.mp4",
        SubtitleFormat::MovText,
    );
}

#[test]
fn timed_text_with_a_styled_default() {
    held_to_its_answer(
        "movtext_default",
        "movtext_default.mp4",
        SubtitleFormat::MovText,
    );
}

#[test]
fn timed_text_justified() {
    held_to_its_answer(
        "movtext_justified",
        "movtext_justified.mp4",
        SubtitleFormat::MovText,
    );
}

#[test]
fn a_seek_in_timed_text_lands_on_the_cue_showing_then() {
    let all = answers("movtext_styles.srt");
    // Cue 4 shows from 5.5 s to 6.5 s; between cues, the next one.
    let texts = |from: usize| {
        all[from..]
            .iter()
            .map(|c| c.text.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(after_seek("movtext_styles.mp4", 6.0), texts(3));
    assert_eq!(after_seek("movtext_styles.mp4", 6.7), texts(4));
}

/// The texts of the cues read after seeking `file` to `seconds`.
fn after_seek(file: &str, seconds: f64) -> Vec<String> {
    let mut subtitles = Subtitles::open(File::open(data(file)).unwrap()).unwrap();
    subtitles.seek((seconds * 1e9) as i64).unwrap();
    read_all(&mut subtitles)
        .into_iter()
        .map(|c| c.text)
        .collect()
}

#[test]
fn a_seek_gives_the_cues_still_showing_then_the_rest() {
    // A cue begun long before the time and still showing comes first; one
    // over by then does not.
    assert_eq!(
        after_seek("overlap.mkv", 5.5),
        [
            "long, under the next two",
            "another inside the long",
            "after them all"
        ]
    );
    assert_eq!(after_seek("overlap.mkv", 11.0), ["after them all"]);
    assert_eq!(after_seek("overlap.mkv", 0.0).len(), 4);
    assert!(after_seek("overlap.mkv", 60.0).is_empty());
}

#[test]
fn a_seek_into_a_long_track_lands_on_the_cue_showing_then() {
    let all = answers("ass.srt");
    // Cue 20 shows from 29.5 s to 30.5 s; 21 from 31 s to 32 s.
    let texts = |from: usize| {
        all[from..]
            .iter()
            .map(|c| c.text.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(after_seek("ass.mkv", 30.0), texts(19));
    assert_eq!(after_seek("ass.mkv", 32.2), texts(21));
    // And back again.
    let mut subtitles = Subtitles::open(File::open(data("ass.mkv")).unwrap()).unwrap();
    subtitles.seek(30_000_000_000).unwrap();
    subtitles.next_cue().unwrap();
    subtitles.seek(0).unwrap();
    assert_eq!(subtitles.next_cue().unwrap().unwrap().text, all[0].text);
}

#[test]
fn a_file_without_subtitles_is_refused() {
    let file = File::open(data("opus_stereo.webm")).unwrap();
    assert!(matches!(Subtitles::open(file), Err(Error::NoSubtitles)));
    let file = File::open(data("subrip.mkv")).unwrap();
    assert!(matches!(
        Subtitles::open_track(file, 99),
        Err(Error::NoSubtitles)
    ));
}

#[test]
fn what_is_not_a_video_file_is_refused() {
    let r = Subtitles::open(Cursor::new(b"not a video file at all".to_vec()));
    assert!(matches!(r, Err(Error::Container(_))));
}

#[test]
fn a_track_is_opened_by_its_number() {
    let file = File::open(data("ass.mkv")).unwrap();
    let number = Subtitles::open(file).unwrap().info().track;
    let file = File::open(data("ass.mkv")).unwrap();
    let subtitles = Subtitles::open_track(file, number).unwrap();
    assert_eq!(subtitles.info().format, SubtitleFormat::Ass);
}
