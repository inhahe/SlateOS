//! A file's video, frame by frame: [`Video`].
//!
//! It reads the file's packets in order, keeps those of its video track,
//! decodes them, and gives back each picture with its time -- converted
//! ([`Video::next_frame`]), or not yet ([`Video::next_picture`], for a player
//! that has fallen behind and drops pictures without converting them).
//!
//! **Which track.** Of the file's video tracks, the first that is enabled,
//! marked default and in a codec decoded here, else the first decodable one;
//! [`Video::open_track`] names another.
//!
//! **Alpha.** WebM's transparency is a second VP9 stream in each block's
//! `BlockAdditional` 1, used only where the track says it is (`AlphaMode`
//! 1), as RFC 9559 defines it and Chrome reads it.
//!
//! **Crop.** A track's `PixelCrop` is applied to each converted frame -- the
//! picture's pixels outside the crop cut away, those inside exactly as the
//! whole picture converts them -- where it fits the frame; a frame it does
//! not fit is shown whole, as FFmpeg shows it.
//!
//! **Seeking** ([`Video::seek`]) goes to the latest key frame at or before
//! the time (`gui/video/matroska`'s seek, held to FFmpeg's), then either
//! gives back frames from it ([`SeekMode::KeyFrame`], for scrubbing) or
//! decodes quietly up to the frame showing at that time and gives back
//! that one first ([`SeekMode::Exact`]).

use std::io::{Read, Seek};

use matroska::{Demuxer, TrackKind};

use crate::decoder::{Decoder, Packet};
use crate::picture::Picture;
use crate::{Codec, ColourHint, Error, Frame, Limits, time};

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
    /// The track's number in the file.
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
    demuxer: Demuxer<R>,
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
    /// The video of the Matroska or WebM file `source`: its best track
    /// (see the module documentation).
    ///
    /// # Errors
    ///
    /// [`Error::Container`] when the file cannot be read; [`Error::NoVideo`]
    /// when it has no video track; [`Error::Codec`] when none of its video
    /// is in a codec decoded here (the codec of the one it would have
    /// played).
    pub fn open(source: R) -> Result<Self, Error> {
        Self::open_with(source, None, Limits::default())
    }

    /// The video track numbered `track` of `source`.
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
        let demuxer = Demuxer::open(source)?;
        let chosen = {
            let videos: Vec<&matroska::Track> = demuxer
                .tracks()
                .iter()
                .filter(|t| t.kind == TrackKind::Video && t.video.is_some() && t.readable())
                .collect();
            match track {
                Some(n) => videos.iter().find(|t| t.number == n).copied(),
                // Decodable first, then enabled, then marked default; the
                // file's order among equals (`min_by_key` keeps the first).
                None => videos.iter().copied().min_by_key(|t| {
                    let decodable = matches!(codec_of(t), Codec::Vp9 | Codec::Av1);
                    (!decodable, !t.enabled, !t.default)
                }),
            }
            .ok_or(Error::NoVideo)?
            .clone()
        };
        let time_base = demuxer.time_base(chosen.number).ok_or(Error::NoVideo)?;
        let picture = chosen.video.ok_or(Error::NoVideo)?;
        let codec = codec_of(&chosen);
        let hint = ColourHint::matroska(picture.colour.as_ref());
        let decoder = Decoder::new(codec, &chosen.codec_private, hint, limits)?;
        let info = VideoInfo {
            track: chosen.number,
            codec,
            width: cropped(picture.pixel_width, picture.crop[0], picture.crop[2]),
            height: cropped(picture.pixel_height, picture.crop[1], picture.crop[3]),
            display_width: 0,
            display_height: 0,
            frame_duration: chosen.default_duration,
            duration: segment_duration(&demuxer),
            alpha: picture.alpha_mode == 1,
        };
        let (display_width, display_height) = display_size(&picture, info.width, info.height);
        Ok(Self {
            demuxer,
            info: VideoInfo {
                display_width,
                display_height,
                ..info
            },
            time_base,
            crop: picture.crop,
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
                Some(packet) if packet.track == self.info.track => self.send(&packet),
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
        self.demuxer.seek(self.info.track, ticks)?;
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
    fn send(&mut self, packet: &matroska::Packet) {
        let time = packet
            .timestamp
            .map_or(self.next_time, |t| time::to_ns(t, self.time_base));
        let duration = time::duration_to_ns(packet.duration, self.time_base);
        self.next_time = time.saturating_add_unsigned(duration);
        let alpha = if self.info.alpha {
            packet
                .additions
                .iter()
                .find(|(id, _)| *id == 1)
                .map(|(_, bytes)| bytes.as_slice())
        } else {
            None
        };
        let sent = self.decoder.send(&Packet {
            data: &packet.data,
            alpha,
            time,
            duration,
            keyframe: packet.keyframe,
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

/// A track's codec, as this crate names it.
fn codec_of(track: &matroska::Track) -> Codec {
    match track.codec {
        matroska::Codec::Vp8 => Codec::Vp8,
        matroska::Codec::Vp9 => Codec::Vp9,
        matroska::Codec::Av1 => Codec::Av1,
        _ => Codec::Other,
    }
}

/// `size` less the two crops, as a `u32` (0 for a crop that leaves
/// nothing, which the demuxer refuses anyway).
fn cropped(size: u64, a: u64, b: u64) -> u32 {
    u32::try_from(size.saturating_sub(a.saturating_add(b))).unwrap_or(u32::MAX)
}

/// The size to show a `width` x `height` frame at: its height, and the width
/// the file's `DisplayWidth` : `DisplayHeight` gives it -- FFmpeg's reading,
/// which turns the two into a pixel's shape (in every unit but "unknown",
/// 4) -- rounded to the nearest pixel.
fn display_size(video: &matroska::Video, width: u32, height: u32) -> (u32, u32) {
    match (
        video.display_unit,
        video.display_width,
        video.display_height,
    ) {
        (0..=3, Some(dw), Some(dh)) if dw > 0 && dh > 0 && height > 0 => {
            // height * dw / dh, rounded half up: (2 * height * dw + dh) /
            // (2 * dh), which a u128 holds for any u32 height and u64 ratio.
            let shown = u128::from(height)
                .checked_mul(u128::from(dw))
                .and_then(|v| v.checked_mul(2))
                .and_then(|v| v.checked_add(u128::from(dh)))
                .and_then(|v| v.checked_div(u128::from(dh).checked_mul(2)?));
            match shown.and_then(|s| u32::try_from(s).ok()) {
                Some(shown) => (shown.max(1), height),
                None => (width, height),
            }
        }
        _ => (width, height),
    }
}

/// The segment's duration in nanoseconds, if the file gives one that is a
/// finite, non-negative number of ticks.
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a timestamp scale is far inside f64's integer-exact range, and the product is checked finite, non-negative and in range first"
)]
fn segment_duration<R: Read + Seek>(demuxer: &Demuxer<R>) -> Option<u64> {
    let info = demuxer.info();
    let ns = info.duration? * info.timestamp_scale as f64;
    (ns.is_finite() && ns >= 0.0 && ns < u64::MAX as f64).then_some(ns as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn video(unit: u64, dw: Option<u64>, dh: Option<u64>) -> matroska::Video {
        matroska::Video {
            pixel_width: 720,
            pixel_height: 576,
            crop: [0; 4],
            display_width: dw,
            display_height: dh,
            display_unit: unit,
            alpha_mode: 0,
            colour: None,
        }
    }

    #[test]
    fn the_display_size_keeps_the_height_and_takes_the_files_shape() {
        // 720x576 PAL shown 16:9: 1024x576.
        assert_eq!(
            display_size(&video(0, Some(1024), Some(576)), 720, 576),
            (1024, 576)
        );
        assert_eq!(
            display_size(&video(3, Some(16), Some(9)), 720, 576),
            (1024, 576)
        );
        // 4:3, rounded to the nearest pixel: 576 * 4 / 3 = 768.
        assert_eq!(
            display_size(&video(3, Some(4), Some(3)), 720, 576),
            (768, 576)
        );
        // Nothing said, a zero, or an unknown unit: the frame's own size.
        assert_eq!(display_size(&video(0, None, None), 720, 576), (720, 576));
        assert_eq!(
            display_size(&video(0, Some(0), Some(9)), 720, 576),
            (720, 576)
        );
        assert_eq!(
            display_size(&video(4, Some(16), Some(9)), 720, 576),
            (720, 576)
        );
    }

    #[test]
    fn a_crop_leaves_the_rest() {
        assert_eq!(cropped(100, 10, 20), 70);
        assert_eq!(cropped(100, 60, 60), 0);
    }
}
