//! A video file's subtitles, cue by cue: SubRip, ASS and SSA, and WebVTT, in
//! Matroska and WebM; 3GPP timed text in MP4 -- and Blu-ray's PGS, DVD's
//! VobSub and digital television's DVB subtitles, which are pictures of
//! text.
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
//! A size is in 1/288 of the picture's height, 16 being SRT's usual size.
//! It is written as `ffmpeg -c:s srt` writes it, and is ffmpeg's text for
//! the track wherever ffmpeg's says what the format's own renderer shows
//! (`srt.rs`, and each format's module for where this departs):
//!
//! - SubRip (`S_TEXT/UTF8`) is SRT already; its markup is read as ffmpeg's
//!   SubRip decoder reads it, and written back (`subrip.rs`).
//! - ASS and SSA (`S_TEXT/ASS`, `S_TEXT/SSA`) are read as libass reads them,
//!   styles from the track's header, and said in SRT as far as SRT can say
//!   them: positions, rotation, karaoke and the like are dropped (`ass.rs`).
//! - WebVTT (WebM's `D_WEBVTT/SUBTITLES`, Matroska's `S_TEXT/WEBVTT`) is read
//!   as its specification reads it (`webvtt.rs`).
//! - 3GPP timed text (MP4's `tx3g`) is read as ffmpeg's `mov_text` decoder
//!   reads it, its default style, place and style runs said in SRT
//!   (`movtext.rs`).
//!
//! **A cue of pictures** -- Blu-ray's PGS (`S_HDMV/PGS`), DVD's VobSub
//! (`S_VOBSUB`) and DVB's (`S_DVBSUB`), subtitles stored as pictures of
//! their text -- has no text and gives [`Cue::images`] instead: each
//! image's pixels, as RGBA, placed on the picture the subtitles were made
//! for (its *canvas*), which a player scales as it scales the film; each
//! marked when it is *forced*, shown even with subtitles off. They are what
//! FFmpeg's decoders show, to the bit, but where the format's player shows
//! otherwise: a PGS crop is cropped (`pgs.rs`); a DVD subpicture's later
//! control sequences -- colours changed, a fade, a second start -- take
//! effect at their dates, and one wholly transparent clears the screen
//! (`vobsub.rs`); DVB's pictures are a receiver's -- the service's own
//! pages, the standard's default colours, and four more (`dvb.rs`).
//! Pictures last until a later change replaces or clears them -- a DVB
//! page's timeout clears it -- the last of a track that nothing ends,
//! until the film ends (its end is `i64::MAX`).
//!
//! **Time** is in nanoseconds on the file's clock, the clock [`crate::Video`]
//! and [`crate::Sound`] give theirs on. A cue whose packet the file gives no
//! time, whose text is not UTF-8 (as Matroska requires it to be), which is
//! not an ASS event, or a timed-text sample too damaged to read, is passed
//! over and counted ([`Subtitles::damaged`]), as is a PGS display set or a
//! DVB block that met damage, or a DVD subpicture that cannot be read. A
//! timed-text sample with no text clears the screen, and is no cue.

mod ass;
mod colours;
mod dvb;
mod movtext;
mod pgs;
mod srt;
mod subrip;
mod vobsub;
mod webvtt;

use std::collections::VecDeque;
use std::io::{Read, Seek};

use crate::container::{Container, Sample, SubtitleTrack};
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

/// One cue: what to show from `start` until `end`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cue {
    /// When it appears, in nanoseconds on the file's clock.
    pub start: i64,
    /// When it goes, in nanoseconds: for text, `start` plus its block's
    /// duration, or `start` itself where the file gives none; for pictures,
    /// when a later change replaces or clears them -- the next display set,
    /// a subpicture's stop or the next subpicture, a DVB page's timeout --
    /// or `i64::MAX`, the end of the film, where none does.
    pub end: i64,
    /// What it shows, as SRT markup (see the module documentation); empty
    /// for a cue of pictures.
    pub text: String,
    /// What a cue of pictures shows, drawn in order, each over what is
    /// there; empty for text.
    pub images: Vec<CueImage>,
}

/// One image a cue of pictures shows: a picture of its text, placed on the
/// canvas -- the picture the subtitles were made for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CueImage {
    /// The canvas's size. A player scales the canvas to the film as it is
    /// shown, and the image with it.
    pub canvas_width: u32,
    pub canvas_height: u32,
    /// Where its top left pixel goes on the canvas.
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    /// `width * height` pixels, the top row first, each red, green, blue and
    /// alpha -- the colour not multiplied by the alpha.
    pub rgba: Vec<u8>,
    /// Shown even when only forced subtitles are: a sign, a line in another
    /// language, that the film itself leaves untranslated.
    pub forced: bool,
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
    MovText(movtext::Setup),
    /// Blu-ray's pictures, a display set a block.
    Pgs(pgs::Decoder),
    /// DVD's pictures, a subpicture unit a block.
    VobSub(vobsub::Setup),
    /// DVB's pictures, display sets in blocks.
    Dvb(dvb::Decoder),
}

/// A change to the pictures on screen: from a time on the file's clock,
/// these images (none, a clear).
type Change = (i64, Vec<CueImage>);

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
    /// The pictures on screen, given as a cue once what ends them is read.
    showing: Option<Cue>,
    /// What the last block of pictures changes, in order, not yet made: a
    /// later block's first change makes those before it and supersedes the
    /// rest.
    coming: VecDeque<Change>,
    /// Cues of pictures ended and not yet given, in order.
    ready: VecDeque<Cue>,
    damaged: u64,
}

impl<R: Read + Seek> Subtitles<R> {
    /// The subtitles of `source` -- a Matroska, WebM or MP4 file -- from its best
    /// track: text before pictures, pictures read here before those that
    /// are not, then an enabled one, then one marked default, the file's
    /// order among equals.
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
        let mut demuxer = Container::open(source)?;
        let tracks = demuxer.subtitles();
        let chosen: &SubtitleTrack = match track {
            Some(n) => tracks.iter().find(|t| t.number == n),
            None => tracks.iter().min_by_key(|t| {
                (
                    !t.format.is_text(),
                    !t.format.is_read(),
                    !t.enabled,
                    !t.default,
                )
            }),
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
            SubtitleFormat::MovText => Reader::MovText(movtext::Setup::parse(&chosen.config)),
            SubtitleFormat::Pgs => Reader::Pgs(pgs::Decoder::default()),
            SubtitleFormat::VobSub => Reader::VobSub(vobsub::Setup::parse(&chosen.config)),
            SubtitleFormat::Dvb => Reader::Dvb(dvb::Decoder::new(&chosen.config)),
            format @ SubtitleFormat::Other => return Err(Error::SubtitleFormat(format)),
        };
        // The track's own packets only, a film's pictures and sound passed
        // over unread: a few kilobytes of cues do not read the film again.
        demuxer.read_only(chosen.key, Some(crate::container::PASSING_READ_AHEAD))?;
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
            showing: None,
            coming: VecDeque::new(),
            ready: VecDeque::new(),
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
        loop {
            if let Some(cue) = self.ready.pop_front() {
                return Ok(Some(cue));
            }
            let Some(sample) = self.demuxer.next_packet()? else {
                break;
            };
            if sample.track != self.key {
                continue;
            }
            let Some(ticks) = sample.time else {
                self.damaged = self.damaged.saturating_add(1);
                continue;
            };
            let start = time::to_ns(ticks, self.time_base);
            let length = time::duration_to_ns(sample.duration, self.time_base);
            if let Some(changes) = self.pictures(&sample, start, length) {
                self.come(changes);
                continue;
            }
            let end = start.saturating_add(i64::try_from(length).unwrap_or(i64::MAX));
            if !self.kept(start, end) {
                continue;
            }
            let text = match said(&self.reader, &sample) {
                Said::Cue(text) => text,
                Said::Nothing => continue,
                Said::Damaged => {
                    self.damaged = self.damaged.saturating_add(1);
                    continue;
                }
            };
            return Ok(Some(Cue {
                start,
                end,
                text,
                images: Vec::new(),
            }));
        }
        // The end: the changes still to come are made, and pictures still on
        // screen then stay until the film ends.
        self.make_until(i64::MAX);
        if let Some(cue) = self.showing.take() {
            if self.kept(cue.start, cue.end) {
                self.ready.push_back(cue);
            }
        }
        Ok(self.ready.pop_front())
    }

    /// For a track of pictures, the changes `sample` (at `start`, lasting
    /// `length` nanoseconds) makes, in order; `None` for a track of text.
    /// A damaged SPU makes none, and is counted (a PGS display set's damage
    /// its decoder counts).
    fn pictures(&mut self, sample: &Sample, start: i64, length: u64) -> Option<Vec<Change>> {
        match &mut self.reader {
            Reader::Pgs(decoder) => Some(match decoder.display_set(&sample.data) {
                pgs::Shown::Images(images) => vec![(start, images)],
                pgs::Shown::Unchanged => Vec::new(),
            }),
            Reader::VobSub(setup) => {
                // The block's duration, where it has one, ends a picture its
                // SPU never stops.
                let duration = (length > 0).then(|| i64::try_from(length).unwrap_or(i64::MAX));
                let Some(changes) = vobsub::changes(setup, &sample.data, duration) else {
                    self.damaged = self.damaged.saturating_add(1);
                    return Some(Vec::new());
                };
                Some(
                    changes
                        .into_iter()
                        .map(|c| (start.saturating_add(c.at), c.images))
                        .collect(),
                )
            }
            Reader::Dvb(decoder) => Some(match decoder.block(&sample.data) {
                // Shown until its timeout clears it -- a page of no time on
                // screen for none, a cue lasting no time.
                dvb::Shown::Images { images, timeout } => {
                    let gone =
                        start.saturating_add(i64::from(timeout).saturating_mul(1_000_000_000));
                    vec![(start, images), (gone, Vec::new())]
                }
                dvb::Shown::Unchanged => Vec::new(),
            }),
            _ => None,
        }
    }

    /// A block's changes: those the last block had still to make before
    /// this one's first are made, the rest superseded, and these come.
    fn come(&mut self, changes: Vec<Change>) {
        let Some(&(first, _)) = changes.first() else {
            return;
        };
        self.make_until(first);
        self.coming.clear();
        self.coming.extend(changes);
    }

    /// The changes to come before `until`, made.
    fn make_until(&mut self, until: i64) {
        while self.coming.front().is_some_and(|(at, _)| *at < until) {
            let Some((at, images)) = self.coming.pop_front() else {
                break;
            };
            self.make(at, images);
        }
    }

    /// The pictures on screen become `images` at `at`: those showing end
    /// there -- given as a cue, unless they were replaced at the time they
    /// came, and so never seen -- and these begin.
    fn make(&mut self, at: i64, images: Vec<CueImage>) {
        if let Some(cue) = self.showing.take() {
            if at > cue.start && self.kept(cue.start, at) {
                self.ready.push_back(Cue { end: at, ..cue });
            }
        }
        if !images.is_empty() {
            self.showing = Some(Cue {
                start: at,
                end: i64::MAX,
                text: String::new(),
                images,
            });
        }
    }

    /// Whether a cue from `start` to `end` is given, after a seek: one gone
    /// by the seek's time is not. The first to start at the time or after
    /// ends the passing over; one still showing at it is given, and the
    /// passing over goes on, another cue may be showing too.
    fn kept(&mut self, start: i64, end: i64) -> bool {
        let Some(target) = self.target else {
            return true;
        };
        if start >= target {
            self.target = None;
            true
        } else {
            end > target
        }
    }

    /// Go to `time` (nanoseconds on the file's clock): the next cue is the
    /// first still showing then, or the first after it.
    ///
    /// Pictures are read from far enough back that what is on screen at the
    /// time is known: Blu-ray's and DVB's from the start of the epoch the
    /// time falls in -- the display set that defines afresh what the ones
    /// after it show -- up to [`EPOCH_STEPS`] display sets back; DVD's from
    /// the SPU before the one at the time, which may still show.
    ///
    /// # Errors
    ///
    /// [`Error::Container`] when the source fails, or the track has no cue
    /// to go to. Reading then goes on where it was.
    pub fn seek(&mut self, time: i64) -> Result<(), Error> {
        let ticks = time::to_ticks(time, self.time_base);
        self.demuxer.seek(self.key, ticks)?;
        match &self.reader {
            Reader::Pgs(_) => self.back_to_epoch_start(ticks, pgs::begins_epoch)?,
            Reader::Dvb(decoder) => {
                let pages = decoder.pages();
                self.back_to_epoch_start(ticks, |block| dvb::begins_epoch(block, pages))?;
            }
            Reader::VobSub(_) => self.back_one(ticks)?,
            _ => {}
        }
        match &mut self.reader {
            Reader::Pgs(decoder) => decoder.reset(),
            Reader::Dvb(decoder) => decoder.reset(),
            _ => {}
        }
        self.showing = None;
        self.coming.clear();
        self.ready.clear();
        self.target = Some(time);
        Ok(())
    }

    /// From the SPU a seek to `ticks` found, back one -- an SPU before it
    /// shows until the found one's first start -- or, the found one being
    /// the track's first, to it.
    fn back_one(&mut self, ticks: i64) -> Result<(), Error> {
        let at = match self.next_own()? {
            Some(Sample { time: Some(t), .. }) => t,
            _ => ticks,
        };
        if self.demuxer.seek(self.key, at.saturating_sub(1)).is_err() {
            self.demuxer.seek(self.key, at)?;
        }
        Ok(())
    }

    /// From the display set a seek to `ticks` found, back to the start of
    /// its epoch -- a set that `begins` one: Blu-ray's epoch start or
    /// acquisition point, DVB's acquisition point or mode change -- a set at
    /// a time found by seeking to it, the one before by seeking a tick
    /// earlier; no further than the track's first set, nor than
    /// [`EPOCH_STEPS`] sets.
    fn back_to_epoch_start(
        &mut self,
        ticks: i64,
        begins: impl Fn(&[u8]) -> bool,
    ) -> Result<(), Error> {
        // The time the demuxer was last sent to: it is at the set at or
        // before it.
        let mut at = ticks;
        // The earliest set read so far.
        let mut earliest: Option<i64> = None;
        for _ in 0..EPOCH_STEPS {
            let Some(sample) = self.next_own()? else {
                break;
            };
            let Some(t) = sample.time else {
                break;
            };
            if let Some(first) = earliest.filter(|&e| t >= e) {
                // A seek before the earliest set found it again, as a seek
                // before a track's first key frame does: it is the first.
                at = first;
                break;
            }
            if begins(&sample.data) {
                at = t;
                break;
            }
            earliest = Some(t);
            let before = t.saturating_sub(1);
            if self.demuxer.seek(self.key, before).is_err() {
                // No set before it: the track's first. Reading went on
                // where it was, past it.
                at = t;
                break;
            }
            at = before;
        }
        self.demuxer.seek(self.key, at)?;
        Ok(())
    }

    /// The track's next packet.
    fn next_own(&mut self) -> Result<Option<Sample>, Error> {
        while let Some(sample) = self.demuxer.next_packet()? {
            if sample.track == self.key {
                return Ok(Some(sample));
            }
        }
        Ok(None)
    }

    /// How many cues were passed over -- given no time, not UTF-8, or not
    /// an ASS event -- and pictures that met damage: PGS display sets, DVB
    /// blocks, and DVD SPUs that could not be read.
    pub fn damaged(&self) -> u64 {
        let pictures = match &self.reader {
            Reader::Pgs(decoder) => decoder.damaged(),
            Reader::Dvb(decoder) => decoder.damaged(),
            _ => 0,
        };
        self.damaged.saturating_add(pictures)
    }
}

/// The display sets [`Subtitles::seek`] goes back for the start of an
/// epoch: Blu-ray's usually begin one at every subtitle.
const EPOCH_STEPS: usize = 64;

/// What a packet says.
enum Said {
    /// A cue: its text as SRT markup.
    Cue(String),
    /// Nothing to show: a timed-text sample that clears the screen.
    Nothing,
    /// A packet that cannot be read.
    Damaged,
}

/// What `sample` says, read by `reader`.
fn said(reader: &Reader, sample: &Sample) -> Said {
    if let Reader::MovText(setup) = reader {
        return match movtext::cue(setup, &sample.data) {
            Some(text) if text.is_empty() => Said::Nothing,
            Some(text) => Said::Cue(text),
            None => Said::Damaged,
        };
    }
    // Matroska's text is UTF-8; a cue that is not cannot be read without
    // guessing at what it was.
    let Ok(text) = core::str::from_utf8(&sample.data) else {
        return Said::Damaged;
    };
    // A muxer that ends a cue as C ends a string.
    let text = text.trim_end_matches('\0');
    match reader {
        Reader::SubRip => Said::Cue(write(subrip::ops(text))),
        Reader::WebVtt { webm } => {
            let cue = if *webm {
                webm_cue(text)
            } else {
                // Matroska's settings are the first line of the block's
                // addition, where it has one; one that is not UTF-8 places
                // nothing.
                let addition = sample.addition.as_deref().unwrap_or_default();
                let settings = core::str::from_utf8(addition)
                    .map_or("", |a| a.split('\n').next().unwrap_or(""));
                Some((settings, text))
            };
            cue.map_or(Said::Damaged, |(settings, cue)| {
                Said::Cue(write(webvtt::ops(
                    cue.trim_end_matches(['\r', '\n']),
                    settings,
                )))
            })
        }
        Reader::Ass(script) => ass::cue(script, text).map_or(Said::Damaged, Said::Cue),
        // Read above, as bytes; pictures are not read here.
        Reader::MovText(_) | Reader::Pgs(_) | Reader::VobSub(_) | Reader::Dvb(_) => Said::Damaged,
    }
}

/// The settings and the text of a WebM WebVTT block -- the cue's identifier,
/// its settings, then its text, the first two each ending in `\n` or
/// `\r\n` -- as ffmpeg's demuxer takes it apart (`matroska_parse_webvtt`);
/// `None` for a block without both lines.
fn webm_cue(block: &str) -> Option<(&str, &str)> {
    // A line: where its text ends, and where the next line begins.
    let line = |s: &str| -> Option<(usize, usize)> {
        let end = s.find(['\r', '\n'])?;
        let rest = s.get(end..)?;
        let rest = rest.strip_prefix('\r').unwrap_or(rest);
        rest.strip_prefix('\n')?;
        Some((end, s.len().saturating_sub(rest.len()).saturating_add(1)))
    };
    let (_, after_identifier) = line(block)?;
    let rest = block.get(after_identifier..)?;
    let (settings_end, after_settings) = line(rest)?;
    Some((rest.get(..settings_end)?, rest.get(after_settings..)?))
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
        assert_eq!(webm_cue("\n\ntext"), Some(("", "text")));
        assert_eq!(
            webm_cue("id\r\nline:0\nfirst\nsecond"),
            Some(("line:0", "first\nsecond"))
        );
        assert_eq!(webm_cue("id\nno settings line"), None);
        assert_eq!(webm_cue("id\rsettings\ntext"), None);
    }
}
