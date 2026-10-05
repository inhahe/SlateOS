//! The subtitle fixtures (`tests/data/subrip*`, `ass*`, `ssa*`, `webvtt*`,
//! `overlap*`, `movtext*`) read through [`videocodec::Subtitles`]: every
//! cue's start, end and text held to the fixture's answer, `NAME.srt` --
//! ffmpeg's SRT of the track (`NAME.ffmpeg.srt`) but for the cues where this
//! follows the format's own renderer instead, which
//! `tests/data/generate_subtitle_fixtures.py` lists and says why of, one by
//! one. The pictures of Blu-ray's PGS (`pgs*`) and DVD's VobSub (`vobsub*`)
//! are held to `NAME.states`: the picture FFmpeg's sub2video shows at each
//! change, every subtitle shown and the forced alone, but where the disc's
//! player shows otherwise (the generator's drawing there).

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
fn text_is_opened_before_pictures_and_pictures_read_before_those_not() {
    // Blu-ray pictures marked default, text beside them: the text.
    let file = File::open(data("pgs_and_text.mkv")).unwrap();
    let info = Subtitles::open(file).unwrap().info().clone();
    assert_eq!((info.format, info.default), (SubtitleFormat::SubRip, false));
    // DVB pictures, not read here, marked default, Blu-ray's beside them:
    // Blu-ray's.
    let file = File::open(data("dvb_and_pgs.mkv")).unwrap();
    let info = Subtitles::open(file).unwrap().info().clone();
    assert_eq!((info.format, info.default), (SubtitleFormat::Pgs, false));
    // The DVB track asked for by its number: refused by its format's name.
    let file = File::open(data("dvb_and_pgs.mkv")).unwrap();
    assert!(matches!(
        Subtitles::open_track(file, 1),
        Err(Error::SubtitleFormat(SubtitleFormat::Dvb))
    ));
}
