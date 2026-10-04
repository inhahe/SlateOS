//! A file's video, frame by frame: [`Video`].
//!
//! It reads the file's packets in order -- from a Matroska (WebM) or an MP4
//! file alike (`container.rs`) -- keeps those of its video track, decodes
//! them, and gives back each picture with its time: converted
//! ([`Video::next_frame`]), or not yet ([`Video::next_picture`], for a player
//! that has fallen behind and drops pictures without converting them).
//!
//! **Which track.** Of the file's video tracks, the first that is decodable,
//! enabled and marked default, else the first decodable one;
//! [`Video::open_track`] names another. (An MP4 track is always enabled, and
//! marked default by `tkhd`'s "enabled" flag, as FFmpeg reads it.)
//!
//! **Alpha.** WebM's transparency is a second stream of the track's codec
//! (VP8 or VP9) in each block's `BlockAdditional` 1, used only where the
//! track says it is (`AlphaMode` 1), as RFC 9559 defines it and Chrome reads
//! it. MP4 has none.
//!
//! **Crop.** A track's crop -- Matroska's `PixelCrop`, MP4's clean aperture
//! (`clap`) -- is applied to each converted frame, the picture's pixels
//! outside it cut away and those inside exactly as the whole picture
//! converts them, where it fits the frame; a frame it does not fit is shown
//! whole, as FFmpeg shows it.
//!
//! **Edit lists.** An MP4 file's edit list says which stretch of a track
//! plays: the frames before it that later ones are coded from are decoded
//! and not shown (`gui/video/mp4` marks them, as FFmpeg's demuxer does), and
//! the times are the edit list's.
//!
//! **Seeking** ([`Video::seek`]) goes to the latest key frame at or before
//! the time (the container's seek, each held to FFmpeg's), then either
//! gives back frames from it ([`SeekMode::KeyFrame`], for scrubbing) or
//! decodes quietly up to the frame showing at that time and gives back
//! that one first ([`SeekMode::Exact`]).

use std::io::{Read, Seek};

use crate::container::{Container, Sample};
use crate::decoder::{Decoder, Packet};
use crate::picture::Picture;
use crate::{Codec, Error, Frame, Limits, time};

/// Where a seek lands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeekMode {
    /// On the frame showing at the time sought: the latest frame at or
    /// before it (or the first, for a time before every frame).
    Exact,
    /// On the key frame at or before the time, decoding from which costs
    /// nothing extra: for following a seek bar as it is dragged.
    KeyFrame,
}

/// What a file says of its video, before a frame is decoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VideoInfo {
    /// The track's number in the file: Matroska's track number, MP4's
    /// track ID.
    pub track: u64,
    pub codec: Codec,
    /// The size of its frames, as the file declares it (after any crop).
    /// A stream may change size as it plays; each [`Frame`] has its own.
    pub width: u32,
    pub height: u32,
    /// The size to show a frame at so it has the shape the file asks for:
    /// its height, and the width that gives the file's display aspect.
    /// The frame's own size where the file asks for none.
    pub display_width: u32,
    pub display_height: u32,
    /// How long each frame lasts, in nanoseconds, where every frame lasts
    /// the same: the frame rate's reciprocal.
    pub frame_duration: Option<u64>,
    /// How long the file plays, in nanoseconds, if it says.
    pub duration: Option<u64>,
    /// Whether the frames carry an alpha channel.
    pub alpha: bool,
}

/// A file's video track, decoded frame by frame.
pub struct Video<R> {
    demuxer: Container<R>,
    /// What the container's packets name the track by.
    key: u64,
    info: VideoInfo,
    /// The track's tick: `num / den` seconds.
    time_base: (u64, u64),
    /// The crop: left, top, right, bottom.
    crop: [u64; 4],
    decoder: Decoder,
    /// When the next packet without a time of its own is shown: the last
    /// packet's time plus its duration.
    next_time: i64,
    /// An exact seek's time, while the pictures before it are passed by.
    target: Option<i64>,
    /// During an exact seek, the latest picture at or before its time.
    held: Option<Picture>,
    /// A picture taken while passing that turned out to be past the time,
    /// to give back after the held one.
    after: Option<Picture>,
    /// The file's end has been read, and the decoder emptied.
    ended: bool,
    /// Packets that did not decode, and why the last did not.
    damaged: u64,
    last_damage: Option<Error>,
}

impl<R: Read + Seek> Video<R> {
    /// The video of `source` -- a Matroska, WebM or MP4 file -- from its
    /// best track (see the module documentation).
    ///
    /// # Errors
    ///
    /// [`Error::Container`] when the file cannot be read, or is none of
    /// those; [`Error::NoVideo`] when it has no video track; [`Error::Codec`]
    /// when none of its video is in a codec decoded here (the codec of the
    /// one it would have played).
    pub fn open(source: R) -> Result<Self, Error> {
        Self::open_with(source, None, Limits::default())
    }

    /// The video track numbered `track` of `source` (as
    /// [`VideoInfo::track`] numbers it).
    ///
    /// # Errors
    ///
    /// As [`Self::open`]; [`Error::NoVideo`] when `track` is not a video
    /// track of the file.
    pub fn open_track(source: R, track: u64) -> Result<Self, Error> {
        Self::open_with(source, Some(track), Limits::default())
    }

    /// [`Self::open`] or [`Self::open_track`], with `limits`.
    ///
    /// # Errors
    ///
    /// As [`Self::open_track`].
    pub fn open_with(source: R, track: Option<u64>, limits: Limits) -> Result<Self, Error> {
        let demuxer = Container::open(source)?;
        let videos = demuxer.videos();
        let chosen = match track {
            Some(n) => videos.iter().find(|t| t.number == n),
            // Decodable first, then enabled, then marked default; the
            // file's order among equals (`min_by_key` keeps the first).
            None => videos.iter().min_by_key(|t| {
                let decodable = matches!(t.codec, Codec::Vp8 | Codec::Vp9 | Codec::Av1);
                (!decodable, !t.enabled, !t.default)
            }),
        }
        .ok_or(Error::NoVideo)?;
        let decoder = Decoder::new(chosen.codec, &chosen.config, chosen.hint, limits)?;
        let width = cropped(chosen.width, chosen.crop[0], chosen.crop[2]);
        let height = cropped(chosen.height, chosen.crop[1], chosen.crop[3]);
        let (display_width, display_height) = chosen.display_size(width, height);
        let info = VideoInfo {
            track: chosen.number,
            codec: chosen.codec,
            width,
            height,
            display_width,
            display_height,
            frame_duration: chosen.frame_duration,
            duration: demuxer.duration(),
            alpha: chosen.alpha,
        };
        let (key, time_base, crop) = (chosen.key, chosen.time_base, chosen.crop);
        Ok(Self {
            demuxer,
            key,
            info,
            time_base,
            crop,
            decoder,
            next_time: 0,
            target: None,
            held: None,
            after: None,
            ended: false,
            damaged: 0,
            last_damage: None,
        })
    }

    /// What the file says of its video.
    pub fn info(&self) -> &VideoInfo {
        &self.info
    }

    /// The next frame, converted; `None` at the end.
    ///
    /// A packet that does not decode is passed over -- with the frames that
    /// depend on it, until the next key frame -- and counted
    /// ([`Self::damaged`]); the frames go on from there.
    ///
    /// # Errors
    ///
    /// [`Error::Container`] when the source fails; [`Error::Colour`] for a
    /// picture whose colour cannot be converted.
    pub fn next_frame(&mut self) -> Result<Option<Frame>, Error> {
        let Some(picture) = self.next_picture()? else {
            return Ok(None);
        };
        let frame = picture.to_frame()?;
        Ok(Some(self.cropped_frame(frame)))
    }

    /// The next picture, not yet converted; `None` at the end. For a player
    /// that has fallen behind: it takes pictures with this and converts
    /// ([`Self::convert`]) only those it shows.
    ///
    /// # Errors
    ///
    /// [`Error::Container`] when the source fails.
    pub fn next_picture(&mut self) -> Result<Option<Picture>, Error> {
        loop {
            if let Some(after) = self.after.take() {
                return Ok(Some(after));
            }
            if let Some(picture) = self.decoder.receive() {
                match self.target {
                    Some(target) if picture.time <= target => {
                        // At or before the time sought: the latest so far.
                        self.held = Some(picture);
                    }
                    Some(_) => {
                        // Past it: the held picture is the one showing at
                        // the time sought, and this comes after it.
                        self.target = None;
                        match self.held.take() {
                            Some(held) => {
                                self.after = Some(picture);
                                return Ok(Some(held));
                            }
                            None => return Ok(Some(picture)),
                        }
                    }
                    None => return Ok(Some(picture)),
                }
                continue;
            }
            if self.ended {
                // The end, perhaps before an exact seek's time was passed:
                // the last picture is the one showing then.
                self.target = None;
                return Ok(self.held.take());
            }
            match self.demuxer.next_packet()? {
                Some(sample) if sample.track == self.key => self.send(&sample),
                Some(_) => {}
                None => {
                    if let Err(e) = self.decoder.finish() {
                        self.damage(e);
                    }
                    self.ended = true;
                }
            }
        }
    }

    /// `picture` converted as [`Self::next_frame`] converts it: with the
    /// track's crop.
    ///
    /// # Errors
    ///
    /// [`Error::Colour`] for a picture whose colour cannot be converted.
    pub fn convert(&self, picture: &Picture) -> Result<Frame, Error> {
        Ok(self.cropped_frame(picture.to_frame()?))
    }

    /// Go to `time` (nanoseconds on the file's clock): the next frame is
    /// the one [`SeekMode`] says.
    ///
    /// # Errors
    ///
    /// [`Error::Container`] when the source fails.
    pub fn seek(&mut self, time: i64, mode: SeekMode) -> Result<(), Error> {
        let ticks = time::to_ticks(time, self.time_base);
        self.demuxer.seek(self.key, ticks)?;
        self.decoder.reset()?;
        self.held = None;
        self.after = None;
        self.ended = false;
        self.next_time = time;
        self.target = (mode == SeekMode::Exact).then_some(time);
        Ok(())
    }

    /// How many packets have not decoded so far.
    pub fn damaged(&self) -> u64 {
        self.damaged
    }

    /// Why the last packet that did not decode did not.
    pub fn last_damage(&self) -> Option<Error> {
        self.last_damage
    }

    /// Hand the decoder one of the track's packets.
    fn send(&mut self, sample: &Sample) {
        let time = sample
            .time
            .map_or(self.next_time, |t| time::to_ns(t, self.time_base));
        let duration = time::duration_to_ns(sample.duration, self.time_base);
        self.next_time = time.saturating_add_unsigned(duration);
        if let Some(config) = &sample.new_config {
            self.decoder.configure(config);
        }
        let alpha = if self.info.alpha {
            sample.alpha.as_deref()
        } else {
            None
        };
        let sent = self.decoder.send(&Packet {
            data: &sample.data,
            alpha,
            time,
            duration,
            keyframe: sample.keyframe,
            discard: sample.discard,
        });
        if let Err(e) = sent {
            self.damage(e);
        }
    }

    fn damage(&mut self, e: Error) {
        self.damaged = self.damaged.saturating_add(1);
        self.last_damage = Some(e);
    }

    /// `frame` with the track's crop, where it fits.
    fn cropped_frame(&self, frame: Frame) -> Frame {
        if self.crop == [0; 4] {
            return frame;
        }
        let [left, top, right, bottom] = self.crop;
        let keep = |size: u32, a: u64, b: u64| {
            u64::from(size)
                .checked_sub(a.saturating_add(b))
                .filter(|&k| k > 0)
                .and_then(|k| u32::try_from(k).ok())
        };
        // Within the frame's own size, so every value below fits a u32.
        let (Some(width), Some(height), Ok(left), Ok(top)) = (
            keep(frame.width, left, right),
            keep(frame.height, top, bottom),
            usize::try_from(left),
            usize::try_from(top),
        ) else {
            return frame;
        };
        let (Ok(stride), Ok(cols), Ok(rows)) = (
            usize::try_from(frame.width),
            usize::try_from(width),
            usize::try_from(height),
        ) else {
            return frame;
        };
        let mut pixels = Vec::with_capacity(cols.saturating_mul(rows));
        for row in frame
            .pixels
            .chunks_exact(stride.max(1))
            .skip(top)
            .take(rows)
        {
            pixels.extend(row.iter().skip(left).take(cols));
        }
        Frame {
            width,
            height,
            pixels,
            ..frame
        }
    }
}

/// `size` less the two crops, as a `u32` (0 for a crop that leaves
/// nothing, which Matroska's demuxer refuses anyway).
fn cropped(size: u64, a: u64, b: u64) -> u32 {
    u32::try_from(size.saturating_sub(a.saturating_add(b))).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_crop_leaves_the_rest() {
        assert_eq!(cropped(100, 10, 20), 70);
        assert_eq!(cropped(100, 60, 60), 0);
        // A crop past the picture leaves nothing, rather than wrapping.
        assert_eq!(cropped(64, u64::MAX, 48), 0);
    }
}
