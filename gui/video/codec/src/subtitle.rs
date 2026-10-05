//! A video file's text subtitles, cue by cue: SubRip, ASS and SSA, and
//! WebVTT, in Matroska and WebM.
//!
//! ```no_run
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let file = std::fs::File::open("film.mkv")?;
//! let mut subtitles = videocodec::Subtitles::open(file)?;
//! while let Some(cue) = subtitles.next_cue()? {
//!     // Show `cue.text` from `cue.start` until `cue.end` nanoseconds.
//!     # let _ = cue;
//! }
//! # Ok(())
//! # }
//! ```
//!
//! **A cue's text is SRT markup**, whatever the track's format: `<i>`, `<b>`,
//! `<u>`, `<s>`, `<font>` with `color="#rrggbb"`, `face="…"` and
//! `size="N"`, `{\anN}` for where on the picture it goes (a numeric keypad's
//! layout: 8 is top centre; none is bottom centre), and line breaks as `\n`.
//! A size is in 1/288 of the picture's height, 16 being SRT's usual size. It is written as `ffmpeg -c:s srt` writes it, and is ffmpeg's text
//! for the track wherever ffmpeg's says what the format's own renderer shows
//! (`srt.rs`, and each format's module for where this departs):
//!
//! - SubRip (`S_TEXT/UTF8`) is SRT already; its markup is read as ffmpeg's
//!   SubRip decoder reads it, and written back (`subrip.rs`).
//! - ASS and SSA (`S_TEXT/ASS`, `S_TEXT/SSA`) are read as libass reads them,
//!   styles from the track's header, and said in SRT as far as SRT can say
//!   them: positions, rotation, karaoke and the like are dropped (`ass.rs`).
//! - WebVTT (WebM's `D_WEBVTT/SUBTITLES`, Matroska's `S_TEXT/WEBVTT`) is read
//!   as its specification reads it (`webvtt.rs`).
//!
//! A track of pictures of text -- Blu-ray's PGS, DVD's VobSub, DVB's -- is
//! refused by its format's name ([`crate::Error::SubtitleFormat`]). MP4's
//! text tracks are not read yet.
//!
//! **Time** is in nanoseconds on the file's clock, the clock [`crate::Video`]
//! and [`crate::Sound`] give theirs on. A cue whose packet the file gives no
//! time, whose text is not UTF-8 (as Matroska requires it to be), or which is
//! not an ASS event, is passed over and counted ([`Subtitles::damaged`]).

mod ass;
mod colours;
mod srt;
mod subrip;
mod webvtt;

use std::io::{Read, Seek};

use crate::container::{Container, SubtitleTrack};
use crate::{Error, SubtitleFormat, time};

/// A file's subtitle track, as [`Subtitles::info`] describes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubtitleInfo {
    /// The track's number in the file (Matroska's `TrackNumber`): what
    /// [`Subtitles::open_track`] takes.
    pub track: u64,
    pub format: SubtitleFormat,
    /// Matroska's `FlagDefault`: the track to show when nobody has chosen.
    pub default: bool,
    /// Matroska's `FlagForced`: shown even with subtitles off, for what the
    /// film itself leaves untranslated -- a sign, a line in another language.
    pub forced: bool,
}

/// One cue: text to show from `start` until `end`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cue {
    /// When it appears, in nanoseconds on the file's clock.
    pub start: i64,
    /// When it goes, in nanoseconds: `start` plus its block's duration, or
    /// `start` itself where the file gives none.
    pub end: i64,
    /// What it shows, as SRT markup (see the module documentation).
    pub text: String,
}

/// How a track's packets are read.
enum Reader {
    SubRip,
    Ass(ass::Script),
    /// WebVTT: in WebM, each block the cue's identifier, its settings and
    /// its text, a line each; in Matroska (`S_TEXT/WEBVTT`), the text alone,
    /// the rest in the block's additions.
    WebVtt {
        webm: bool,
    },
}

/// A file's subtitle track, read cue by cue.
pub struct Subtitles<R> {
    demuxer: Container<R>,
    /// What the container's packets name the track by.
    key: u64,
    info: SubtitleInfo,
    /// The track's tick: `num / den` seconds.
    time_base: (u64, u64),
    reader: Reader,
    /// A seek's time, while cues that have gone by it are passed over.
    target: Option<i64>,
    damaged: u64,
}

impl<R: Read + Seek> Subtitles<R> {
    /// The subtitles of `source` -- a Matroska or WebM file -- from its best
    /// track: one read here before one that is not, then an enabled one,
    /// then one marked default, the file's order among equals.
    ///
    /// # Errors
    ///
    /// [`Error::Container`] when the file cannot be read, or is not one of
    /// those; [`Error::NoSubtitles`] when it has no subtitle track;
    /// [`Error::SubtitleFormat`] when none is in a format read here (the
    /// format of the one it would have shown).
    pub fn open(source: R) -> Result<Self, Error> {
        Self::open_with(source, None)
    }

    /// The subtitle track numbered `track` of `source` (as
    /// [`SubtitleInfo::track`] numbers it).
    ///
    /// # Errors
    ///
    /// As [`Self::open`]; [`Error::NoSubtitles`] when `track` is not a
    /// subtitle track of the file.
    pub fn open_track(source: R, track: u64) -> Result<Self, Error> {
        Self::open_with(source, Some(track))
    }

    fn open_with(source: R, track: Option<u64>) -> Result<Self, Error> {
        let demuxer = Container::open(source)?;
        let tracks = demuxer.subtitles();
        let chosen: &SubtitleTrack = match track {
            Some(n) => tracks.iter().find(|t| t.number == n),
            None => tracks
                .iter()
                .min_by_key(|t| (!t.format.is_text(), !t.enabled, !t.default)),
        }
        .ok_or(Error::NoSubtitles)?;
        let reader = match chosen.format {
            SubtitleFormat::SubRip => Reader::SubRip,
            SubtitleFormat::Ass | SubtitleFormat::Ssa => {
                Reader::Ass(ass::Script::parse(&chosen.config))
            }
            SubtitleFormat::WebVtt => Reader::WebVtt {
                webm: chosen.codec_id.starts_with(b"D_WEBVTT/"),
            },
            format => return Err(Error::SubtitleFormat(format)),
        };
        Ok(Self {
            key: chosen.key,
            info: SubtitleInfo {
                track: chosen.number,
                format: chosen.format,
                default: chosen.default,
                forced: chosen.forced,
            },
            time_base: chosen.time_base,
            reader,
            target: None,
            damaged: 0,
            demuxer,
        })
    }

    /// The track being read.
    pub fn info(&self) -> &SubtitleInfo {
        &self.info
    }

    /// The next cue, in the file's order -- the order they start in; `None`
    /// at the end.
    ///
    /// # Errors
    ///
    /// [`Error::Container`] when the source fails, or the file is damaged
    /// past reading on.
    pub fn next_cue(&mut self) -> Result<Option<Cue>, Error> {
        while let Some(sample) = self.demuxer.next_packet()? {
            if sample.track != self.key {
                continue;
            }
            let Some(ticks) = sample.time else {
                self.damaged = self.damaged.saturating_add(1);
                continue;
            };
            let start = time::to_ns(ticks, self.time_base);
            let length = time::duration_to_ns(sample.duration, self.time_base);
            let end = start.saturating_add(i64::try_from(length).unwrap_or(i64::MAX));
            if let Some(target) = self.target {
                if start >= target {
                    self.target = None;
                } else if end <= target {
                    continue;
                }
            }
            // Matroska's text is UTF-8; a cue that is not cannot be read
            // without guessing at what it was.
            let Ok(text) = core::str::from_utf8(&sample.data) else {
                self.damaged = self.damaged.saturating_add(1);
                continue;
            };
            // A muxer that ends a cue as C ends a string.
            let text = text.trim_end_matches('\0');
            let text = match &self.reader {
                Reader::SubRip => write(subrip::ops(text)),
                Reader::WebVtt { webm } => {
                    let cue = if *webm { webm_cue(text) } else { Some(text) };
                    let Some(cue) = cue else {
                        self.damaged = self.damaged.saturating_add(1);
                        continue;
                    };
                    write(webvtt::ops(cue.trim_end_matches(['\r', '\n'])))
                }
                Reader::Ass(script) => {
                    let Some(text) = ass::cue(script, text) else {
                        self.damaged = self.damaged.saturating_add(1);
                        continue;
                    };
                    text
                }
            };
            return Ok(Some(Cue { start, end, text }));
        }
        Ok(None)
    }

    /// Go to `time` (nanoseconds on the file's clock): the next cue is the
    /// first still showing then, or the first after it.
    ///
    /// # Errors
    ///
    /// [`Error::Container`] when the source fails, or the track has no cue
    /// to go to. Reading then goes on where it was.
    pub fn seek(&mut self, time: i64) -> Result<(), Error> {
        self.demuxer
            .seek(self.key, time::to_ticks(time, self.time_base))?;
        self.target = Some(time);
        Ok(())
    }

    /// How many cues were passed over: given no time, not UTF-8, or not an
    /// ASS event.
    pub fn damaged(&self) -> u64 {
        self.damaged
    }
}

/// The text of a WebM WebVTT block -- the cue's identifier, its settings,
/// then its text, the first two each ending in `\n` or `\r\n` -- as ffmpeg's
/// demuxer takes it apart (`matroska_parse_webvtt`); `None` for a block
/// without both lines.
fn webm_cue(block: &str) -> Option<&str> {
    let line = |s: &str| -> Option<usize> {
        let end = s.find(['\r', '\n'])?;
        let rest = s.get(end..)?;
        let rest = rest.strip_prefix('\r').unwrap_or(rest);
        rest.strip_prefix('\n')?;
        Some(s.len().saturating_sub(rest.len()).saturating_add(1))
    };
    let after_identifier = block.get(line(block)?..)?;
    after_identifier.get(line(after_identifier)?..)
}

/// A cue's steps as SRT markup.
fn write(ops: Vec<srt::Op>) -> String {
    let mut w = srt::Writer::new();
    for op in ops {
        w.op(op);
    }
    w.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_webm_block_is_its_identifier_its_settings_then_its_text() {
        assert_eq!(webm_cue("\n\ntext"), Some("text"));
        assert_eq!(
            webm_cue("id\r\nline:0\nfirst\nsecond"),
            Some("first\nsecond")
        );
        assert_eq!(webm_cue("id\nno settings line"), None);
        assert_eq!(webm_cue("id\rsettings\ntext"), None);
    }
}
