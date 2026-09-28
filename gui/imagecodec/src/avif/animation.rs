//! An AVIF sequence, played a frame at a time: libavif 1.3.0's
//! `avifDecoderNextImage` and `avifDecoderNthImage`, and the timing they
//! report (`src/read.c`), with Chrome's choices where libavif leaves one to
//! its caller.
//!
//! A sequence is a track of AV1 frames, and perhaps a second track of their
//! alpha (`movie.rs`). A frame after a key frame is predicted from the frames
//! before it, so each track keeps its decoder from one frame to the next
//! ([`Codecs`]), and each track's samples are walked in order
//! ([`SampleCursor`]) rather than found afresh -- playing a sequence from its
//! start costs one step a frame. Going back, or far forward, restarts from the
//! nearest key frame at or before the frame asked for, as libavif does: the
//! frames between are decoded and not shown.
//!
//! Each frame is the whole picture. Unlike a GIF's or a WebP's, an AVIF frame
//! is not drawn over the one before, so there is no canvas to composite onto:
//! a frame is cropped, converted and turned exactly as [`super::decode`] does
//! the first.
//!
//! A frame lasts its `stts` entry's delta, in the colour track's timescale,
//! given here in milliseconds rounded to the nearest (Pillow's
//! `round(1000 * delta / timescale)`, in exact arithmetic). A still picture is
//! a sequence of one frame lasting a second, which is libavif's answer too.
//! Whole milliseconds are what Firefox times frames in as well; Chrome keeps
//! microseconds, so a sequence at 30 frames a second -- 33.3 ms a frame --
//! plays about 1% faster here and in Firefox than in Chrome.
//!
//! Portions of this file are copyright 2019 Joe Drago, from libavif, and used
//! under its BSD-2-Clause licence: `licenses/libavif-LICENSE.txt`.

use alloc::vec::Vec;

use super::Error;
use super::decode::{self, Codecs};
use super::movie::{Repetition, SampleCursor, Timeline};
use super::setup::{self, Picture, TileInput};
use crate::{Image, ImageError, ImageResult, Limits};

/// How many times a sequence asks to be played.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Repeat {
    /// Without end -- as its edit list says, or, for a sequence without one,
    /// as Chrome and Firefox both play it.
    Forever,
    /// Once, then this many times more: libavif's repetition count. A still
    /// picture is `Count(0)`.
    Count(u32),
}

/// One frame of an [`Animation`].
#[derive(Clone, Copy, Debug)]
pub struct Frame<'a> {
    /// The whole picture at this frame, as it is shown.
    pub image: &'a Image,
    /// Which frame it is, counting from 0.
    pub index: usize,
    /// How long to show it, in milliseconds, as the file says.
    pub duration_ms: u32,
}

impl Frame<'_> {
    /// How long a browser shows this frame, in milliseconds: the file's
    /// duration, except that 10 ms or less is shown for 100 ms. Chrome applies
    /// that to every animated picture whatever its format
    /// (`DeferredImageDecoder::FrameDurationAtIndex`), as Firefox does, for
    /// the files that ask for "as fast as you can" and flicker when taken at
    /// their word.
    #[must_use]
    pub const fn display_duration_ms(&self) -> u32 {
        if self.duration_ms <= 10 {
            100
        } else {
            self.duration_ms
        }
    }
}

/// One track of the sequence being walked.
#[derive(Debug)]
struct Walk {
    /// Its index among the file's tracks.
    track: usize,
    cursor: SampleCursor,
    /// Its sync frames, ascending ([`super::movie::SampleTable::sync_frames`]).
    sync: Vec<u32>,
    /// How many frames it has.
    count: u32,
}

impl Walk {
    /// `avifDecoderIsKeyframe`, for this track: a frame it has, marked sync.
    fn is_sync(&self, frame: u32) -> bool {
        frame < self.count && self.sync.binary_search(&frame).is_ok()
    }

    /// The last frame at or before `frame` that this track could start from.
    fn sync_at_or_before(&self, frame: u32) -> u32 {
        let frame = frame.min(self.count.saturating_sub(1));
        match self.sync.binary_search(&frame) {
            Ok(_) => frame,
            Err(after) => after
                .checked_sub(1)
                .and_then(|at| self.sync.get(at))
                .copied()
                .unwrap_or(0),
        }
    }
}

/// An AVIF's frames, decoded one at a time: a sequence's in order, or a still
/// picture's one.
///
/// ```ignore
/// let mut animation = avif::Animation::new(&bytes, Limits::default())?;
/// while let Some(frame) = animation.next_frame()? {
///     show(frame.image, frame.display_duration_ms());
/// }
/// animation.rewind(); // and again, as `animation.repeat()` says
/// ```
///
/// A frame that fails to decode ends that play: the error is returned again
/// for every later frame, since the decoders' state is no longer to be
/// trusted, until [`rewind`](Self::rewind) or [`seek`](Self::seek) starts
/// again from a key frame.
pub struct Animation<'a> {
    picture: Picture<'a>,
    /// The sequence's tracks, colour then alpha, as the tiles name them;
    /// empty for a still picture.
    walks: Vec<Walk>,
    codecs: Codecs,
    timeline: Timeline,
    /// The next frame to decode: libavif's `imageIndex + 1`.
    next: u32,
    /// Frame `next - 1`, once decoded.
    shown: Option<Image>,
    /// Why the last frame failed, until decoding starts again.
    failed: Option<ImageError>,
}

impl<'a> Animation<'a> {
    /// Read the file's structure -- its picture or tracks, how many frames,
    /// how often it plays -- without decoding a frame.
    ///
    /// # Errors
    ///
    /// As [`super::dimensions`], and [`ImageError::TooLarge`] past `limits`.
    pub fn new(bytes: &'a [u8], limits: Limits) -> ImageResult<Self> {
        let picture = Picture::read(bytes)?;
        picture.check(limits)?;
        let size = setup::file_size(bytes);
        let mut walks = Vec::new();
        for (layer, _) in decode::layers(&picture) {
            for tile in &layer.tiles {
                if let TileInput::Track { track } = tile.input {
                    let table = setup::track_table(&picture.file, track)?;
                    let count = table.samples(super::IMAGE_COUNT_LIMIT, size, |_| true)?;
                    walks.push(Walk {
                        track,
                        cursor: SampleCursor::new(table),
                        sync: table.sync_frames(count),
                        count,
                    });
                }
            }
        }
        let codecs = Codecs::new(&picture);
        Ok(Self {
            picture,
            walks,
            codecs,
            timeline: Timeline::default(),
            next: 0,
            shown: None,
            failed: None,
        })
    }

    /// The width and height every frame is shown at: [`super::dimensions`].
    #[must_use]
    pub fn size(&self) -> (u32, u32) {
        self.picture.orientation().shown(self.picture.shown_size())
    }

    /// How many frames there are: the colour track's samples, or 1 for a still
    /// picture. More than one is an animation.
    #[must_use]
    pub fn frame_count(&self) -> usize {
        usize::try_from(self.picture.frame_count).unwrap_or(usize::MAX)
    }

    /// Whether the frames have alpha.
    #[must_use]
    pub const fn has_alpha(&self) -> bool {
        self.picture.alpha.is_some()
    }

    /// How many times the sequence asks to be played: libavif's repetition
    /// count from the colour track's edit list, read as Chrome reads it.
    #[must_use]
    pub const fn repeat(&self) -> Repeat {
        match self.picture.timing {
            None => Repeat::Count(0),
            Some(timing) => match timing.repetition {
                Repetition::Count(more) => Repeat::Count(more),
                // No edit list: Chrome loops, for the files it showed before
                // it read edit lists at all; Firefox loops too.
                Repetition::Infinite | Repetition::Unknown => Repeat::Forever,
            },
        }
    }

    /// The sequence's length as the colour track's media header gives it, in
    /// milliseconds -- libavif's `duration` -- or a second for a still
    /// picture. It is what the file says, not the frames' sum.
    #[must_use]
    pub fn duration_ms(&self) -> u64 {
        match self.picture.timing {
            None => 1000,
            Some(timing) => to_ms(timing.duration, timing.timescale),
        }
    }

    /// The next frame, or `None` after the last.
    ///
    /// # Errors
    ///
    /// [`ImageError::Malformed`] when a frame does not decode, or decodes to
    /// one the container does not describe; [`ImageError::Unsupported`] as
    /// [`super::decode`]. See the type's docs for what follows an error.
    pub fn next_frame(&mut self) -> ImageResult<Option<Frame<'_>>> {
        if let Some(error) = &self.failed {
            return Err(error.clone());
        }
        match self.decode_next() {
            Ok(true) => self.current().map(Some),
            Ok(false) => Ok(None),
            Err(error) => {
                self.shown = None;
                self.failed = Some(error.clone());
                Err(error)
            }
        }
    }

    /// Frame `index`, decoded as libavif's `avifDecoderNthImage` decodes it:
    /// the frame after the current one is simply the next; the current one is
    /// returned as it is; any other is reached from the last key frame at or
    /// before it -- restarting there when that is not ahead of where decoding
    /// is. `None` when there is no such frame. [`next_frame`](Self::next_frame)
    /// then continues from it.
    ///
    /// # Errors
    ///
    /// As [`next_frame`](Self::next_frame), for this frame or any decoded on
    /// the way to it.
    pub fn seek(&mut self, index: usize) -> ImageResult<Option<Frame<'_>>> {
        let Some(index) = u32::try_from(index)
            .ok()
            .filter(|&i| i < self.picture.frame_count)
        else {
            return Ok(None);
        };
        let healthy = self.failed.is_none();
        if healthy && index == self.next {
            return self.next_frame();
        }
        if healthy && index.checked_add(1) == Some(self.next) && self.shown.is_some() {
            return self.current().map(Some);
        }
        let key = self.nearest_key_frame(index);
        if !healthy || key > self.next || index < self.next {
            if let Err(error) = self.restart_at(key) {
                self.failed = Some(error.clone());
                return Err(error);
            }
        }
        while self.next <= index {
            if self.next_frame()?.is_none() {
                return Ok(None);
            }
        }
        self.current().map(Some)
    }

    /// Back to before the first frame: the decoders dropped (libavif's
    /// `avifDecoderFlush`) and every track walked again from its start.
    pub fn rewind(&mut self) {
        self.codecs = Codecs::new(&self.picture);
        self.timeline = Timeline::default();
        for walk in &mut self.walks {
            if let Ok(table) = setup::track_table(&self.picture.file, walk.track) {
                walk.cursor = SampleCursor::new(table);
            }
        }
        self.next = 0;
        self.shown = None;
        self.failed = None;
    }

    /// Frame `next - 1`, as last decoded, with its duration.
    fn current(&mut self) -> ImageResult<Frame<'_>> {
        let index = self
            .next
            .checked_sub(1)
            .ok_or(ImageError::Malformed("AVIF frame"))?;
        let duration_ms = self.frame_duration_ms(index);
        let image = self
            .shown
            .as_ref()
            .ok_or(ImageError::Malformed("AVIF frame"))?;
        Ok(Frame {
            image,
            index: usize::try_from(index).unwrap_or(usize::MAX),
            duration_ms,
        })
    }

    /// `avifDecoderNthImageTiming`'s duration of frame `index`, in
    /// milliseconds: its colour track's delta, or libavif's one second for a
    /// still picture.
    fn frame_duration_ms(&mut self, index: u32) -> u32 {
        let (Some(timing), Some(walk)) = (self.picture.timing, self.walks.first()) else {
            return 1000;
        };
        let Ok(table) = setup::track_table(&self.picture.file, walk.track) else {
            return 0;
        };
        let delta = self.timeline.delta(table, index);
        u32::try_from(to_ms(u64::from(delta), timing.timescale)).unwrap_or(u32::MAX)
    }

    /// Decode frame `next`: libavif's `avifDecoderNextImage`. `false` when
    /// there is none -- past the last frame, or past the end of a track
    /// shorter than the colour track (`AVIF_RESULT_NO_IMAGES_REMAINING`).
    fn decode_next(&mut self) -> ImageResult<bool> {
        let frame = self.next;
        if frame >= self.picture.frame_count || self.walks.iter().any(|w| frame >= w.count) {
            return Ok(false);
        }
        // avifDecoderPrepareTiles: every tile's sample, colour then alpha,
        // before anything is decoded.
        let bytes = self.picture.file.bytes;
        let size = setup::file_size(bytes);
        let mut samples = Vec::new();
        let mut walks = self.walks.iter_mut();
        for (layer, _) in decode::layers(&self.picture) {
            for tile in &layer.tiles {
                let sample = match tile.input {
                    TileInput::Item { .. } => self.picture.sample(tile, frame, 0)?,
                    TileInput::Track { track } => {
                        let walk = walks.next().ok_or(Error::MissingImage)?;
                        let table = setup::track_table(&self.picture.file, track)?;
                        let at = walk
                            .cursor
                            .next(table, size)?
                            .ok_or(Error::NoContent("AVIF frame past the last"))?;
                        let length = usize::try_from(at.size)
                            .map_err(|_| Error::Parse("AVIF sample size"))?;
                        setup::read_file(bytes, at.offset, length)?
                    }
                };
                samples.push(sample);
            }
        }
        let decoded = decode::decode_samples(&self.picture, &samples, &mut self.codecs)?;
        self.shown = Some(super::shown(&self.picture, decoded)?);
        self.next = frame.saturating_add(1);
        Ok(true)
    }

    /// `avifDecoderNearestKeyframe`: the last frame at or before `index` from
    /// which every track can start (`avifDecoderIsKeyframe`), or frame 0.
    ///
    /// libavif steps back a frame at a time. This steps back to the latest
    /// frame every track *could* start from -- the least of each track's last
    /// sync frame before -- which passes over only frames that some track
    /// cannot start from, so it lands where libavif does.
    fn nearest_key_frame(&self, index: u32) -> u32 {
        let mut at = index;
        while at > 0 {
            if self.walks.iter().all(|w| w.is_sync(at)) {
                return at;
            }
            let before = at.saturating_sub(1);
            at = self
                .walks
                .iter()
                .map(|w| w.sync_at_or_before(before))
                .fold(before, u32::min);
        }
        0
    }

    /// Start decoding again at frame `key`, a key frame: libavif's
    /// `avifDecoderFlush`, then each track's walk brought up to `key`.
    fn restart_at(&mut self, key: u32) -> ImageResult<()> {
        self.rewind();
        let size = setup::file_size(self.picture.file.bytes);
        for walk in &mut self.walks {
            let table = setup::track_table(&self.picture.file, walk.track)?;
            while walk.cursor.position() < key {
                walk.cursor
                    .next(table, size)?
                    .ok_or(Error::NoContent("AVIF frame past the last"))?;
            }
        }
        self.next = key;
        Ok(())
    }
}

/// `ticks` of a `timescale`-a-second clock in milliseconds, rounded to the
/// nearest and a half to the even: Pillow's `round(1000 * (ticks /
/// timescale))`, in exact arithmetic. A timescale of 0 gives 0, as libavif
/// gives no duration for one.
fn to_ms(ticks: u64, timescale: u32) -> u64 {
    let scale = u128::from(timescale);
    let milli = u128::from(ticks).saturating_mul(1000);
    let (Some(whole), Some(left)) = (milli.checked_div(scale), milli.checked_rem(scale)) else {
        return 0;
    };
    let twice = left.saturating_mul(2);
    let up = twice > scale || (twice == scale && whole & 1 == 1);
    u64::try_from(whole.saturating_add(u128::from(up))).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_duration_is_rounded_to_the_nearest_millisecond_and_a_half_to_the_even() {
        assert_eq!(to_ms(1, 30), 33);
        assert_eq!(to_ms(5, 30), 167);
        assert_eq!(to_ms(1, 1), 1000);
        assert_eq!(to_ms(1, 2000), 0); // 0.5
        assert_eq!(to_ms(3, 2000), 2); // 1.5
        assert_eq!(to_ms(5, 2000), 2); // 2.5
        assert_eq!(to_ms(7, 1000), 7);
        assert_eq!(to_ms(1, 0), 0);
        assert_eq!(to_ms(u64::MAX, 1), u64::MAX);
    }

    fn walk(sync: &[u32], count: u32) -> Walk {
        Walk {
            track: 0,
            cursor: SampleCursor::new(&super::super::movie::SampleTable::default()),
            sync: sync.to_vec(),
            count,
        }
    }

    #[test]
    fn a_track_starts_from_its_last_sync_frame_at_or_before_the_one_asked_for() {
        let w = walk(&[0, 2, 3], 5);
        assert_eq!(
            [0, 1, 2, 3, 4].map(|f| w.sync_at_or_before(f)),
            [0, 0, 2, 3, 3]
        );
        assert!(w.is_sync(3) && !w.is_sync(4) && !w.is_sync(5));
        // Past its last frame, a track answers for its last.
        let short = walk(&[0, 2], 3);
        assert_eq!(short.sync_at_or_before(9), 2);
        assert!(!short.is_sync(7));
    }
}
