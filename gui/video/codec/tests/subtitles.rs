//! The subtitle fixtures (`tests/data/subrip*`, `ass*`, `ssa*`, `webvtt*`,
//! `overlap*`, `movtext*`) read through [`videocodec::Subtitles`]: every
//! cue's start, end and text held to the fixture's answer, `NAME.srt` --
//! ffmpeg's SRT of the track (`NAME.ffmpeg.srt`) but for the cues where this
//! follows the format's own renderer instead, which
//! `tests/data/generate_subtitle_fixtures.py` lists and says why of, one by
//! one. The pictures of Blu-ray's PGS (`pgs*`), DVD's VobSub (`vobsub*`) and
//! DVB's (`dvb*`) are held to `NAME.states`: the picture FFmpeg's sub2video
//! shows at each change, every subtitle shown and the forced alone, but
//! where the format's player shows otherwise (the generator's drawing
//! there).

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

use videocodec::{Cue, CueImage, Error, SubtitleFormat, Subtitles};

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
                images: Vec::new(),
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

/// WebVTT in MP4 (`wvtt`), as MP4Box writes it: the cues of `webvtt`'s
/// `.vtt`, each a sample of its own, read as WebM's are.
#[test]
fn webvtt_in_mp4() {
    held_to_its_answer("webvtt_mp4", "webvtt_mp4.mp4", SubtitleFormat::WebVtt);
}

#[test]
fn webvtt_overlapping_cues() {
    held_to_its_answer(
        "webvtt_overlap",
        "webvtt_overlap.webm",
        SubtitleFormat::WebVtt,
    );
}

/// The same cues in MP4, where WebVTT cuts them into a sample for every
/// stretch between one cue's start or end and the next, every cue in each
/// sample it shows through: joined again, they are WebM's cues, in its
/// order.
#[test]
fn webvtt_in_mp4_cut_into_samples_is_joined_again() {
    held_to_its_answer(
        "webvtt_overlap_mp4",
        "webvtt_overlap_mp4.mp4",
        SubtitleFormat::WebVtt,
    );
}

/// And in fragments, as DASH and HLS segments carry it.
#[test]
fn webvtt_in_mp4_fragments() {
    held_to_its_answer(
        "webvtt_overlap_fragments",
        "webvtt_overlap_fragments.mp4",
        SubtitleFormat::WebVtt,
    );
}

/// A run of a cue's text: its text and style -- bold, italic, underline and
/// strike as four `0`/`1`, the colour as `rrggbb` or `-` for SRT's white --
/// or a line break.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Run {
    Text {
        text: String,
        flags: String,
        colour: String,
    },
    Break,
}

/// A TTML cue: its times in nanoseconds, its placement (1 to 9, a numeric
/// keypad's) and its runs.
type TtmlCue = (i64, i64, u8, Vec<Run>);

/// A TTML fixture's answer, `NAME.cues`: its cues, and how many of its
/// samples are no document (`damaged N`; none where the line is missing).
fn cue_answers(name: &str) -> (Vec<TtmlCue>, u64) {
    let path = data(name);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let path = path.display();
    let mut cues: Vec<TtmlCue> = Vec::new();
    let mut damaged = 0;
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        let (kind, rest) = line.split_once(' ').unwrap_or((line, ""));
        match kind {
            "damaged" => damaged = rest.parse().unwrap(),
            "cue" => {
                let w: Vec<&str> = rest.split(' ').collect();
                cues.push((
                    w[0].parse().unwrap(),
                    w[1].parse().unwrap(),
                    w[2].parse().unwrap(),
                    Vec::new(),
                ));
            }
            "break" => cues.last_mut().unwrap().3.push(Run::Break),
            "text" => {
                let (flags, rest) = rest.split_once(' ').unwrap();
                let (colour, quoted) = rest.split_once(' ').unwrap();
                cues.last_mut().unwrap().3.push(Run::Text {
                    text: json_string(quoted),
                    flags: flags.to_owned(),
                    colour: colour.to_owned(),
                });
            }
            other => panic!("{path}: a line this test does not know: {other}"),
        }
    }
    (cues, damaged)
}

/// A JSON string's text: what the generator writes with `json.dumps`.
fn json_string(quoted: &str) -> String {
    let inner = quoted
        .strip_prefix('"')
        .and_then(|q| q.strip_suffix('"'))
        .unwrap();
    let mut out = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next().unwrap() {
            'n' => out.push('\n'),
            't' => out.push('\t'),
            '"' => out.push('"'),
            '\\' => out.push('\\'),
            '/' => out.push('/'),
            'u' => {
                let hex: String = chars.by_ref().take(4).collect();
                out.push(char::from_u32(u32::from_str_radix(&hex, 16).unwrap()).unwrap());
            }
            other => panic!("an escape this test does not know: {other}"),
        }
    }
    out
}

/// A cue's SRT markup read back into its placement and runs: `{\anN}`
/// first, then `<b>`, `<i>`, `<u>`, `<s>` and `<font color>` and their ends,
/// and line breaks; the word joiners the writer puts after what SRT would
/// read as markup dropped; neighbouring runs of one style one run.
fn runs_of(markup: &str) -> (u8, Vec<Run>) {
    let (an, mut rest) = match markup.strip_prefix("{\\an") {
        Some(r) if r.chars().nth(1) == Some('}') => (r[..1].parse().unwrap(), &r[2..]),
        _ => (2, markup),
    };
    let (mut b, mut i, mut u, mut s) = (0, 0, 0, 0);
    let mut colours: Vec<String> = Vec::new();
    let mut runs: Vec<Run> = Vec::new();
    while let Some(c) = rest.chars().next() {
        if c == '<' && !rest[1..].starts_with('\u{2060}') {
            let end = rest.find('>').unwrap();
            let tag = &rest[1..end];
            rest = &rest[end + 1..];
            match tag {
                "b" => b += 1,
                "/b" => b -= 1,
                "i" => i += 1,
                "/i" => i -= 1,
                "u" => u += 1,
                "/u" => u -= 1,
                "s" => s += 1,
                "/s" => s -= 1,
                "/font" => {
                    colours.pop();
                }
                _ => {
                    let colour = tag
                        .strip_prefix("font color=\"#")
                        .and_then(|t| t.strip_suffix('"'))
                        .unwrap_or_else(|| panic!("a tag this test does not know: {tag}"));
                    colours.push(colour.to_owned());
                }
            }
            continue;
        }
        rest = &rest[c.len_utf8()..];
        match c {
            '\u{2060}' => {}
            '\n' => runs.push(Run::Break),
            c => {
                let flag = |n: i32| if n > 0 { '1' } else { '0' };
                let flags: String = [flag(b), flag(i), flag(u), flag(s)].iter().collect();
                let colour = colours.last().cloned().unwrap_or_else(|| "-".to_owned());
                match runs.last_mut() {
                    Some(Run::Text {
                        text,
                        flags: f,
                        colour: k,
                    }) if *f == flags && *k == colour => text.push(c),
                    _ => runs.push(Run::Text {
                        text: c.to_string(),
                        flags,
                        colour,
                    }),
                }
            }
        }
    }
    (an, runs)
}

/// Every cue of `NAME.mp4`, a TTML track, against `NAME.cues`: ttconv's
/// reading of its samples, said as this crate says it -- and the samples
/// that are no document counted, as ttconv cannot read them either.
fn ttml_held_to_its_answer(name: &str) {
    let file = format!("{name}.mp4");
    let mut subtitles = Subtitles::open(File::open(data(&file)).unwrap()).unwrap();
    assert_eq!(subtitles.info().format, SubtitleFormat::Ttml, "{file}");
    let got: Vec<TtmlCue> = read_all(&mut subtitles)
        .into_iter()
        .map(|c| {
            let (an, runs) = runs_of(&c.text);
            (c.start, c.end, an, runs)
        })
        .collect();
    let (want, damaged) = cue_answers(&format!("{name}.cues"));
    let mut wrong = Vec::new();
    for (k, (w, g)) in want.iter().zip(&got).enumerate() {
        if w != g {
            wrong.push(format!("cue {}:\n  want {w:?}\n  got  {g:?}", k + 1));
        }
    }
    assert!(
        wrong.is_empty() && want.len() == got.len(),
        "{file}: {} cues read, {} wanted; these differ:\n{}",
        got.len(),
        want.len(),
        wrong.join("\n")
    );
    assert_eq!(subtitles.damaged(), damaged, "{file}");
}

/// TTML in MP4 (`stpp`), as MP4Box writes it: styles inline and
/// referenced, three regions, breaks, white space, colours. Its sample of
/// `a &lt; b &amp; c` is no XML -- MP4Box writes the two bare -- and shows
/// nothing, as in GPAC itself and ttconv.
#[test]
fn ttml_in_mp4() {
    ttml_held_to_its_answer("ttml");
}

/// The same document whole in every sample of two seconds, each showing
/// only its own stretch, as ISO/IEC 14496-30 has it: the paragraphs cut at
/// every sample's edge are joined again into the same cues -- the escapes
/// MP4Box loses among them.
#[test]
fn ttml_in_samples_of_two_seconds_is_joined_again() {
    ttml_held_to_its_answer("ttml_split");
}

/// Time containers, offsets, frames and ticks, a sequence, spans of their
/// own times: the document in one sample, read through.
#[test]
fn ttml_timing() {
    ttml_held_to_its_answer("ttml_timing_one");
}

#[test]
fn ttml_timing_cut_into_samples() {
    ttml_held_to_its_answer("ttml_timing_split");
}

/// The same document as MP4Box splits it -- each paragraph in the samples
/// its own begin and end alone would put it in, so most show in none of
/// theirs -- shows each sample's document only in that sample's stretch:
/// what the samples hold, not what the document they came from would show.
#[test]
fn ttml_shows_each_sample_only_in_its_own_stretch() {
    ttml_held_to_its_answer("ttml_timing");
}

/// The TTML cues of `file` from a seek to `time` on.
fn ttml_after_seek(file: &str, time: i64) -> Vec<TtmlCue> {
    let mut subtitles = Subtitles::open(File::open(data(file)).unwrap()).unwrap();
    subtitles.seek(time).unwrap();
    read_all(&mut subtitles)
        .into_iter()
        .map(|c| {
            let (an, runs) = runs_of(&c.text);
            (c.start, c.end, an, runs)
        })
        .collect()
}

/// A seek into TTML in MP4 gives the cues showing at the time, then the
/// rest: from the document in one sample, each as the document has it; from
/// samples of a second and a half, each still showing from the sample the
/// seek landed in -- as far as the samples after the seek say -- as WebVTT's
/// in MP4.
#[test]
fn a_seek_in_ttml_gives_the_cues_showing_then() {
    const SAMPLE: i64 = 1_500_000_000;
    let (one, _) = cue_answers("ttml_timing_one.cues");
    let (split, _) = cue_answers("ttml_timing_split.cues");
    assert_eq!(one, split, "the two files carry the same document");
    // Inside a cue, at a cue's start and end, inside a sequence's turn,
    // between cues, past the last.
    for time in [
        0,
        1_000_000_000,
        2_500_000_000,
        3_000_000_000,
        5_500_000_000,
        7_400_000_000,
        11_200_000_000,
        15_500_000_000,
        17_000_000_000,
        18_300_000_000,
        30_000_000_000,
    ] {
        let showing: Vec<TtmlCue> = one.iter().filter(|c| c.1 > time).cloned().collect();
        assert_eq!(
            ttml_after_seek("ttml_timing_one.mp4", time),
            showing,
            "the one sample, from {time} ns"
        );
        let landed = time - time % SAMPLE;
        let from_sample: Vec<TtmlCue> = showing
            .iter()
            .map(|c| (c.0.max(landed), c.1, c.2, c.3.clone()))
            .collect();
        assert_eq!(
            ttml_after_seek("ttml_timing_split.mp4", time),
            from_sample,
            "samples of 1.5 s, from {time} ns"
        );
    }
}

/// Which region a paragraph shows in, if any, and where that is.
#[test]
fn ttml_regions() {
    ttml_held_to_its_answer("ttml_regions");
}

/// A document of no regions: the root container is the one, its text at
/// the top left by TTML's initial alignments.
#[test]
fn ttml_without_regions() {
    ttml_held_to_its_answer("ttml_default");
}

/// A seek back after reading forgets what was read: the cues read again are
/// the same cues, none twice -- from part way through, and from the end,
/// where a cue reaching the last sample's end was still open.
#[test]
fn a_seek_in_ttml_forgets_what_was_read() {
    let said = |cues: Vec<Cue>| -> Vec<(i64, i64, String)> {
        cues.into_iter().map(|c| (c.start, c.end, c.text)).collect()
    };
    for file in [
        "ttml_timing_one.mp4",
        "ttml_timing_split.mp4",
        "ttml_split.mp4",
    ] {
        let mut subtitles = Subtitles::open(File::open(data(file)).unwrap()).unwrap();
        let all = said(read_all(&mut subtitles));
        subtitles.seek(0).unwrap();
        assert_eq!(said(read_all(&mut subtitles)), all, "{file}, from the end");
        subtitles.seek(0).unwrap();
        for _ in 0..3 {
            subtitles.next_cue().unwrap();
        }
        subtitles.seek(0).unwrap();
        assert_eq!(said(read_all(&mut subtitles)), all, "{file}, from part way");
    }
}

/// A seek into WebVTT in MP4 gives the cues showing at the time -- each
/// beginning, as far as the samples after the seek say, at the sample the
/// seek landed in -- then the rest.
#[test]
fn a_seek_in_webvtt_in_mp4_gives_the_cues_showing_then() {
    let file = "webvtt_overlap_mp4.mp4";
    assert_eq!(
        after_seek(file, 5.5),
        [
            "{\\an8}long, under all but the last",
            "the same text",
            "the same text",
            "after them all, past a stretch of none",
            "first of a pair",
            "second of a pair"
        ]
    );
    assert_eq!(
        after_seek(file, 11.0),
        [
            "after them all, past a stretch of none",
            "first of a pair",
            "second of a pair"
        ]
    );
    assert_eq!(after_seek(file, 0.0).len(), 8);
    assert!(after_seek(file, 60.0).is_empty());
    let mut subtitles = Subtitles::open(File::open(data(file)).unwrap()).unwrap();
    subtitles.seek(5_500_000_000).unwrap();
    let first = subtitles.next_cue().unwrap().unwrap();
    assert_eq!(
        (first.start, first.end),
        (5_000_000_000, 10_000_000_000),
        "the long cue, from the sample the seek landed in"
    );
}

/// A seek while a cue still shows forgets it: the samples read after the
/// seek are joined to nothing read before it.
#[test]
fn a_seek_in_webvtt_in_mp4_forgets_the_cue_showing_before_it() {
    let file = File::open(data("webvtt_overlap_mp4.mp4")).unwrap();
    let mut subtitles = Subtitles::open(file).unwrap();
    subtitles.seek(14_500_000_000).unwrap();
    // The first of the pair is given while the second still shows.
    let first = subtitles.next_cue().unwrap().unwrap();
    assert_eq!(first.text, "first of a pair");
    subtitles.seek(14_500_000_000).unwrap();
    let texts: Vec<String> = read_all(&mut subtitles)
        .into_iter()
        .map(|c| c.text)
        .collect();
    assert_eq!(texts, ["first of a pair", "second of a pair"]);
}

/// A sample of WebVTT in MP4 that cannot be read shows nothing, and is
/// counted: the cues showing end where it begins, and one in the samples
/// on both sides of it is two cues.
#[test]
fn a_damaged_webvtt_sample_in_mp4_shows_nothing_and_is_counted() {
    let mut bytes = std::fs::read(data("webvtt_overlap_mp4.mp4")).unwrap();
    // The short cue is in one sample alone, 2 s to 3 s: its text's box made
    // to run past its cue.
    let text = b"short, inside the long";
    let at = bytes
        .windows(4 + text.len())
        .position(|w| &w[..4] == b"payl" && &w[4..] == text)
        .expect("the cue's text box");
    bytes[at - 4..at].copy_from_slice(&0xFFFFu32.to_be_bytes());
    let mut subtitles = Subtitles::open(std::io::Cursor::new(bytes)).unwrap();
    let mut got = Vec::new();
    while let Some(cue) = subtitles.next_cue().unwrap() {
        got.push((cue.start / 1_000_000, cue.end / 1_000_000, cue.text));
    }
    let long = "{\\an8}long, under all but the last".to_owned();
    assert_eq!(
        got[..3],
        [
            (1000, 2000, long.clone()),
            (3000, 10_000, long),
            (
                3000,
                4000,
                "{\\an1}begun with the short, ended after it".to_owned()
            ),
        ]
    );
    assert_eq!(got.len(), 8, "{got:?}");
    assert_eq!(subtitles.damaged(), 1);
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

// --- Pictures of text: Blu-ray's PGS ---------------------------------------

/// A pictures fixture's answer, `NAME.states`: its canvas, and the picture
/// at each change -- the time in milliseconds, an MD5 of the canvas's RGBA
/// bytes -- with every subtitle shown, and with the forced alone.
struct Pictures {
    canvas: (u32, u32),
    all: Vec<(i64, String)>,
    forced: Vec<(i64, String)>,
}

fn pictures(name: &str) -> Pictures {
    let text = std::fs::read_to_string(data(&format!("{name}.states"))).unwrap();
    let mut p = Pictures {
        canvas: (0, 0),
        all: Vec::new(),
        forced: Vec::new(),
    };
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        let w: Vec<&str> = line.split(' ').collect();
        match w[0] {
            "canvas" => p.canvas = (w[1].parse().unwrap(), w[2].parse().unwrap()),
            "state" => p.all.push((w[1].parse().unwrap(), w[2].to_owned())),
            "forced" => p.forced.push((w[1].parse().unwrap(), w[2].to_owned())),
            other => panic!("{name}.states: a line this test does not know: {other}"),
        }
    }
    p
}

/// The canvas with `images` drawn on it as FFmpeg's sub2video draws them --
/// each copied over what is there, in order; one reaching past the canvas
/// left out -- as an MD5 of its RGBA bytes.
fn drawn(canvas: (u32, u32), images: &[&CueImage]) -> String {
    let (w, h) = (canvas.0 as usize, canvas.1 as usize);
    let mut frame = vec![0u8; w * h * 4];
    for i in images {
        let (x, y) = (i.x as usize, i.y as usize);
        let (iw, ih) = (i.width as usize, i.height as usize);
        assert_eq!(i.rgba.len(), iw * ih * 4, "an image's pixels fill it");
        if x + iw > w || y + ih > h {
            continue;
        }
        for row in 0..ih {
            let to = ((y + row) * w + x) * 4;
            frame[to..to + iw * 4].copy_from_slice(&i.rgba[row * iw * 4..(row + 1) * iw * 4]);
        }
    }
    md5::md5_hex(&frame).to_string()
}

/// What `cues` show, change by change -- with every image, and with the
/// forced alone: each cue's pictures at its start, the blank at its end
/// where no cue starts then, with a leading blank and repeats dropped, as
/// the answers drop them. (A canvas is drawn once a cue where it can be: a
/// test built without optimisation hashes a few megabytes a second.)
fn shown(cues: &[Cue], canvas: (u32, u32)) -> [Vec<(i64, String)>; 2] {
    let blank = drawn(canvas, &[]);
    let mut changes: [Vec<(i64, String)>; 2] = [Vec::new(), Vec::new()];
    for (k, cue) in cues.iter().enumerate() {
        let every: Vec<&CueImage> = cue.images.iter().collect();
        let forced: Vec<&CueImage> = cue.images.iter().filter(|i| i.forced).collect();
        let all = drawn(canvas, &every);
        let only_forced = match forced.len() {
            0 => blank.clone(),
            n if n == every.len() => all.clone(),
            _ => drawn(canvas, &forced),
        };
        let start = cue.start / 1_000_000;
        changes[0].push((start, all));
        changes[1].push((start, only_forced));
        if cue.end != i64::MAX && cues.get(k + 1).map(|c| c.start) != Some(cue.end) {
            for c in &mut changes {
                c.push((cue.end / 1_000_000, blank.clone()));
            }
        }
    }
    changes.map(|changes| {
        let mut out: Vec<(i64, String)> = Vec::new();
        for (t, md5) in changes {
            if out.last().map_or(md5 != blank, |(_, m)| *m != md5) {
                out.push((t, md5));
            }
        }
        out
    })
}

/// Every picture of `NAME.mkv` against `NAME.states`; the cues of pictures
/// alone, on the answer's canvas. The cues, for more to be asked of them.
fn pictures_held_to_their_answer(
    name: &str,
    format: SubtitleFormat,
) -> (Vec<Cue>, Subtitles<File>) {
    let answer = pictures(name);
    let file = File::open(data(&format!("{name}.mkv"))).unwrap();
    let mut subtitles = Subtitles::open(file).unwrap();
    assert_eq!(subtitles.info().format, format, "{name}");
    let cues = read_all(&mut subtitles);
    for cue in &cues {
        assert!(cue.text.is_empty(), "{name}: pictures have no text");
        assert!(!cue.images.is_empty(), "{name}: a cue shows something");
        for i in &cue.images {
            assert_eq!((i.canvas_width, i.canvas_height), answer.canvas, "{name}");
        }
    }
    let [all, forced] = shown(&cues, answer.canvas);
    assert_eq!(all, answer.all, "{name}: the pictures");
    assert_eq!(forced, answer.forced, "{name}: the forced pictures");
    (cues, subtitles)
}

#[test]
fn pgs() {
    let (cues, subtitles) = pictures_held_to_their_answer("pgs", SubtitleFormat::Pgs);
    assert_eq!(subtitles.damaged(), 0);
    // The last picture is never cleared: it shows until the film ends. The
    // one set at the same time before it was never seen, and is no cue:
    // the cue before the last is the one cleared at 20 s.
    let last = cues.last().unwrap();
    assert_eq!((last.start, last.end), (21_000_000_000, i64::MAX));
    assert_eq!(cues[cues.len() - 2].end, 20_000_000_000);
}

#[test]
fn pgs_on_a_standard_definition_canvas() {
    let (_, subtitles) = pictures_held_to_their_answer("pgs_sd", SubtitleFormat::Pgs);
    assert_eq!(subtitles.damaged(), 0);
}

#[test]
fn pgs_cropped_as_a_blu_ray_player_crops() {
    let (_, subtitles) = pictures_held_to_their_answer("pgs_cropped", SubtitleFormat::Pgs);
    assert_eq!(subtitles.damaged(), 0);
}

#[test]
fn pgs_damage_taken_as_ffmpeg_takes_it() {
    let (_, subtitles) = pictures_held_to_their_answer("pgs_damage", SubtitleFormat::Pgs);
    // Ten palettes, sixty-five objects, one wider than the canvas, codes a
    // line short, a composition cut short: five sets met damage.
    assert_eq!(subtitles.damaged(), 5);
}

/// The first cue after seeking `NAME.mkv` to `ms`: its start and end in
/// milliseconds, and the picture it shows.
fn first_after_seek(name: &str, ms: i64) -> (i64, i64, String) {
    let answer = pictures(name);
    let file = File::open(data(&format!("{name}.mkv"))).unwrap();
    let mut subtitles = Subtitles::open(file).unwrap();
    subtitles.seek(ms * 1_000_000).unwrap();
    let cue = subtitles.next_cue().unwrap().unwrap();
    let images: Vec<&CueImage> = cue.images.iter().collect();
    (
        cue.start / 1_000_000,
        cue.end / 1_000_000,
        drawn(answer.canvas, &images),
    )
}

#[test]
fn a_seek_in_pictures_lands_on_the_picture_showing_then() {
    let answer = pictures("pgs");
    let at = |t: i64| {
        answer
            .all
            .iter()
            .rev()
            .find(|(s, _)| *s <= t)
            .unwrap()
            .1
            .clone()
    };
    // A picture its own display set defines.
    assert_eq!(first_after_seek("pgs", 5500), (5000, 6000, at(5000)));
    // Pictures in the middle of an epoch, their object and palette defined
    // by the sets before: the seek goes back to the epoch's start.
    assert_eq!(first_after_seek("pgs", 6500), (6000, 7000, at(6000)));
    assert_eq!(first_after_seek("pgs", 7500), (7000, 8000, at(7000)));
    assert_eq!(first_after_seek("pgs", 8000), (8000, 9000, at(8000)));
    // Between pictures: the next one.
    assert_eq!(first_after_seek("pgs", 2500), (3000, 4000, at(3000)));
}

#[test]
fn a_seek_back_forgets_what_was_read_after_it() {
    // pgs_damage's first display set, at half a second, names an object and
    // a palette no set before it defined, and shows nothing. Read to the
    // end, the reader holds the last epoch's of those ids; sought back to
    // the first set, it must have forgotten them, and the first picture is
    // the one at a second.
    let file = File::open(data("pgs_damage.mkv")).unwrap();
    let mut subtitles = Subtitles::open(file).unwrap();
    read_all(&mut subtitles);
    subtitles.seek(600_000_000).unwrap();
    assert_eq!(subtitles.next_cue().unwrap().unwrap().start, 1_000_000_000);
}

// --- Pictures of text: DVD's VobSub ----------------------------------------

#[test]
fn vobsub() {
    let (cues, subtitles) = pictures_held_to_their_answer("vobsub", SubtitleFormat::VobSub);
    assert_eq!(subtitles.damaged(), 0);
    // The forced subtitle, and no other, is marked.
    let forced: Vec<i64> = cues
        .iter()
        .filter(|c| c.images.iter().any(|i| i.forced))
        .map(|c| c.start / 1_000_000)
        .collect();
    assert_eq!(forced, [3000]);
}

#[test]
fn vobsub_as_a_dvd_player_shows_it() {
    let (_, subtitles) = pictures_held_to_their_answer("vobsub_dvd", SubtitleFormat::VobSub);
    assert_eq!(subtitles.damaged(), 0);
}

#[test]
fn vobsub_without_a_palette() {
    let (_, subtitles) = pictures_held_to_their_answer("vobsub_grey", SubtitleFormat::VobSub);
    assert_eq!(subtitles.damaged(), 0);
}

#[test]
fn vobsub_without_a_size() {
    let (cues, _) = pictures_held_to_their_answer("vobsub_pal", SubtitleFormat::VobSub);
    assert_eq!(cues[0].images[0].canvas_height, 576);
}

#[test]
fn vobsub_damage_taken_as_ffmpeg_takes_it() {
    let (_, subtitles) = pictures_held_to_their_answer("vobsub_damage", SubtitleFormat::VobSub);
    // Fields past the data, an area taller than its codes, an SPU cut short.
    assert_eq!(subtitles.damaged(), 3);
}

#[test]
fn a_seek_in_dvd_pictures_finds_one_still_showing() {
    let answer = pictures("vobsub");
    let at = |t: i64| {
        answer
            .all
            .iter()
            .rev()
            .find(|(s, _)| *s <= t)
            .unwrap()
            .1
            .clone()
    };
    // At 22.1 s the SPU there has not started (it starts at 22.25 s), and
    // the one before still shows -- found by going back one.
    let (start, end, picture) = first_after_seek("vobsub", 22_100);
    assert_eq!((start, end), (20_000, 22_250));
    assert_eq!(picture, at(20_000));
    // A picture its own SPU shows; between pictures, the next.
    assert_eq!(first_after_seek("vobsub", 8_500), (8_000, 9_001, at(8_000)));
    assert_eq!(first_after_seek("vobsub", 2_500), (3_000, 5_002, at(3_000)));
}

#[test]
fn dvb() {
    let (cues, subtitles) = pictures_held_to_their_answer("dvb", SubtitleFormat::Dvb);
    assert_eq!(subtitles.damaged(), 0);
    // A page's ten seconds end it where nothing replaces it sooner: the
    // last, at 28 s, cleared at 29 s by the next; the one at 12 s gone after
    // its own two.
    let at = |s: i64| cues.iter().find(|c| c.start == s * 1_000_000_000).unwrap();
    assert_eq!(at(12).end, 14_000_000_000);
    assert_eq!(at(28).end, 29_000_000_000);
}

#[test]
fn dvb_as_a_receiver_shows_it() {
    let (_, subtitles) = pictures_held_to_their_answer("dvb_receiver", SubtitleFormat::Dvb);
    assert_eq!(subtitles.damaged(), 0);
}

#[test]
fn dvb_on_a_high_definition_display_with_a_window() {
    let (cues, _) = pictures_held_to_their_answer("dvb_hd", SubtitleFormat::Dvb);
    // The window's corner moves the regions.
    let places: Vec<(u32, u32)> = cues[0].images.iter().map(|i| (i.x, i.y)).collect();
    assert_eq!(places, [(1240, 835), (240, 135)]);
}

#[test]
fn dvb_reads_its_own_services_pages() {
    let (_, subtitles) = pictures_held_to_their_answer("dvb_pages", SubtitleFormat::Dvb);
    assert_eq!(subtitles.damaged(), 0);
}

#[test]
fn dvb_damage_taken_as_ffmpeg_takes_it() {
    let (_, subtitles) = pictures_held_to_their_answer("dvb_damage", SubtitleFormat::Dvb);
    // A segment past its block, data for an object no region places, an
    // object of characters, field lengths past their segment, a region of
    // no width, one placing an object outside it, and a block of six bytes:
    // seven blocks.
    assert_eq!(subtitles.damaged(), 7);
}

#[test]
fn a_seek_in_dvb_pictures_reads_from_where_they_begin_afresh() {
    let answer = pictures("dvb");
    let at = |t: i64| {
        answer
            .all
            .iter()
            .rev()
            .find(|(s, _)| *s <= t)
            .unwrap()
            .1
            .clone()
    };
    // The page at 20 s moves what the mode change at 18 s drew.
    assert_eq!(
        first_after_seek("dvb", 21_000),
        (20_000, 22_000, at(20_000))
    );
    // From the mode change at 10 s: regions drawn at 11 s, shown again at 12.
    assert_eq!(
        first_after_seek("dvb", 13_000),
        (12_000, 14_000, at(12_000))
    );
    // Between pictures: the next; one whose page lasts no time is none.
    assert_eq!(
        first_after_seek("dvb", 14_500),
        (15_000, 16_000, at(15_000))
    );
}

#[test]
fn a_seek_in_dvb_pictures_forgets_what_was_read() {
    // The track read through, its last page of the version of the one at
    // 18 s: a seek back to 19 s reads that page afresh, not as the page
    // held.
    let answer = pictures("dvb");
    let file = File::open(data("dvb.mkv")).unwrap();
    let mut subtitles = Subtitles::open(file).unwrap();
    read_all(&mut subtitles);
    subtitles.seek(19_000_000_000).unwrap();
    let cue = subtitles.next_cue().unwrap().unwrap();
    let images: Vec<&CueImage> = cue.images.iter().collect();
    let shown = answer
        .all
        .iter()
        .rev()
        .find(|(s, _)| *s <= 18_000)
        .unwrap()
        .1
        .clone();
    assert_eq!((cue.start, cue.end), (18_000_000_000, 20_000_000_000));
    assert_eq!(drawn(answer.canvas, &images), shown);
}

#[test]
fn text_is_opened_before_pictures_and_pictures_read_before_those_not() {
    // Blu-ray pictures marked default, text beside them: the text.
    let file = File::open(data("pgs_and_text.mkv")).unwrap();
    let info = Subtitles::open(file).unwrap().info().clone();
    assert_eq!((info.format, info.default), (SubtitleFormat::SubRip, false));
    // DVB pictures marked default, Blu-ray's beside them: both read, the
    // default.
    let file = File::open(data("dvb_and_pgs.mkv")).unwrap();
    let info = Subtitles::open(file).unwrap().info().clone();
    assert_eq!((info.format, info.default), (SubtitleFormat::Dvb, true));
    // Kate, not read here, marked default, DVB's beside it: DVB's.
    let file = File::open(data("kate_and_dvb.mkv")).unwrap();
    let info = Subtitles::open(file).unwrap().info().clone();
    assert_eq!((info.format, info.default), (SubtitleFormat::Dvb, false));
    // The Kate track asked for by its number: refused by its format.
    let file = File::open(data("kate_and_dvb.mkv")).unwrap();
    assert!(matches!(
        Subtitles::open_track(file, 1),
        Err(Error::SubtitleFormat(SubtitleFormat::Other))
    ));
}

#[test]
fn ffmpegs_own_dvb_subtitles_are_read() {
    // ffmpeg's dvbsub encoder's pictures of pgs_sd's: every cue read, none
    // damaged, each on the canvas its display definition gives.
    let file = File::open(data("dvb_and_pgs.mkv")).unwrap();
    let mut subtitles = Subtitles::open_track(file, 1).unwrap();
    let cues = read_all(&mut subtitles);
    assert!(!cues.is_empty());
    assert_eq!(subtitles.damaged(), 0);
    for cue in &cues {
        for i in &cue.images {
            assert_eq!((i.canvas_width, i.canvas_height), (720, 480));
        }
    }
}
