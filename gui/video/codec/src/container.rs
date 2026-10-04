//! The file's container -- Matroska (WebM among them) or MP4 -- told apart by
//! its first bytes as FFmpeg tells them, and read through one face: its video
//! tracks described alike ([`Track`]), its sound tracks ([`SoundTrack`]), its
//! packets given alike ([`Sample`]), and a seek.
//!
//! **Telling them apart.** A Matroska file begins with EBML's magic, and
//! FFmpeg's Matroska probe asks for it at the very start; anything else is
//! MP4 when FFmpeg's MP4 probe would take it for one (`mp4::probe`, over as
//! much of the file as FFmpeg reads to decide, a mebibyte), and otherwise
//! not a file this plays.

use std::io::{Read, Seek, SeekFrom};

use crate::{Codec, ColourHint, ContainerError, Orientation, SoundCodec, time};

/// How much of a file is read to tell what it is: the most FFmpeg reads to
/// decide (`PROBE_BUF_MAX`), or the whole of a smaller file.
const PROBE: u64 = 1 << 20;

/// EBML's magic, with which every Matroska and WebM file begins.
const EBML: [u8; 4] = [0x1A, 0x45, 0xDF, 0xA3];

/// A file being read.
// Each boxed: the two differ in size by hundreds of bytes, and a `Video`
// holds one for as long as it plays.
pub(crate) enum Container<R> {
    Matroska(Box<matroska::Demuxer<R>>),
    Mp4(Box<mp4::Demuxer<R>>),
}

/// A video track, as either container describes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Track {
    /// Matroska's track number; MP4's track ID: what a caller names it by.
    pub number: u64,
    /// What the container's packets and seeks name it by: Matroska's track
    /// number, MP4's place among the tracks (an ID a damaged file may give
    /// two tracks).
    pub key: u64,
    pub codec: Codec,
    /// The codec's setup as the file stores it: Matroska's `CodecPrivate`,
    /// MP4's configuration box (AV1's `av1C` among them).
    pub config: Vec<u8>,
    /// Matroska's `FlagEnabled`; every MP4 track.
    pub enabled: bool,
    /// Matroska's `FlagDefault`; MP4's `tkhd` "enabled" flag, which FFmpeg
    /// reads as the default track.
    pub default: bool,
    /// The picture's size as the file declares it, before the crop.
    pub width: u64,
    pub height: u64,
    /// The crop: left, top, right, bottom.
    pub crop: [u64; 4],
    /// The shape the file asks a frame shown at.
    pub aspect: Aspect,
    /// Which way up the file asks a frame shown.
    pub orientation: Orientation,
    /// WebM's alpha channel, in each block's `BlockAdditional` 1.
    pub alpha: bool,
    /// What the file says of the track's colour.
    pub hint: ColourHint,
    /// How long each frame lasts, in nanoseconds, where they all last the
    /// same.
    pub frame_duration: Option<u64>,
    /// The track's tick: `num / den` seconds.
    pub time_base: (u64, u64),
}

/// The shape a file asks its frames shown at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Aspect {
    /// None asked: as the frame is.
    Square,
    /// Matroska's `DisplayWidth` : `DisplayHeight`, in `DisplayUnit`.
    Display { unit: u64, width: u64, height: u64 },
    /// MP4's pixel shape, width to height, as FFmpeg settles it.
    Pixel { num: i32, den: i32 },
}

/// One packet of a track, as either container gives it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Sample {
    /// The track, as [`Track::key`] names it.
    pub track: u64,
    /// When its picture is shown, in the track's ticks; `None` where the
    /// file does not say (a Matroska lace after the first).
    pub time: Option<i64>,
    /// How long, in ticks; 0 if the file does not say.
    pub duration: u64,
    pub keyframe: bool,
    /// Decoded for the frames after it, its own picture not shown: a frame
    /// MP4's edit list leaves out.
    pub discard: bool,
    pub data: Vec<u8>,
    /// WebM's alpha channel for the frame: its `BlockAdditional` 1.
    pub alpha: Option<Vec<u8>>,
    /// The codec setup this packet and those after it decode with, where it
    /// changes (an MP4 track of several sample entries).
    pub new_config: Option<Vec<u8>>,
    /// Sound to discard, in nanoseconds: from the end of the packet's if
    /// positive, its start if negative (Matroska's `DiscardPadding`; 0 in
    /// MP4).
    pub discard_padding: i64,
    /// Sound to drop from the start of what this packet decodes to, in
    /// samples (MP4's: the priming its edit list leaves out, given with the
    /// first packet and with the first after a seek into it; 0 in Matroska,
    /// whose codec delay is the track's).
    pub skip_samples: u64,
    /// Where the packet's bytes begin in the file: what tells one packet
    /// from another.
    pub position: u64,
}

/// A sound track.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SoundTrack {
    /// Matroska's track number, MP4's track ID: what a caller names it by.
    pub number: u64,
    /// What the container's packets and seeks name it by.
    pub key: u64,
    pub codec: SoundCodec,
    /// The codec's setup: Opus's `OpusHead` (made from MP4's `dOps` as
    /// FFmpeg makes it), Vorbis's three headers laced.
    pub config: Vec<u8>,
    pub enabled: bool,
    pub default: bool,
    /// The track's tick: `num / den` seconds.
    pub time_base: (u64, u64),
    /// The codec's delay, in nanoseconds (Opus's pre-skip): the packets'
    /// times have it taken off already, as FFmpeg takes it.
    pub codec_delay: u64,
    /// How much sound a decoder needs after a jump before its output is
    /// right, in nanoseconds (Opus: 80 ms).
    pub seek_pre_roll: u64,
}

impl<R: Read + Seek> Container<R> {
    /// Tell what `source` is, and open it as that.
    ///
    /// # Errors
    ///
    /// [`ContainerError::Unknown`] for a file neither container's probe
    /// takes; the container's own error for one it takes and cannot read.
    pub(crate) fn open(mut source: R) -> Result<Self, ContainerError> {
        let mut head = Vec::new();
        (&mut source)
            .take(PROBE)
            .read_to_end(&mut head)
            .map_err(|e| ContainerError::Io(e.kind()))?;
        source
            .seek(SeekFrom::Start(0))
            .map_err(|e| ContainerError::Io(e.kind()))?;
        if head.starts_with(&EBML) {
            Ok(Self::Matroska(Box::new(matroska::Demuxer::open(source)?)))
        } else if mp4::probe(&head) {
            Ok(Self::Mp4(Box::new(mp4::Demuxer::open(source)?)))
        } else {
            Err(ContainerError::Unknown)
        }
    }

    /// The file's video tracks that can be read, in the file's order.
    pub(crate) fn videos(&self) -> Vec<Track> {
        match self {
            Self::Matroska(d) => d
                .tracks()
                .iter()
                .filter(|t| t.kind == matroska::TrackKind::Video && t.readable())
                .filter_map(|t| matroska_track(d, t))
                .collect(),
            Self::Mp4(d) => d
                .tracks()
                .iter()
                .enumerate()
                .filter_map(|(i, t)| mp4_track(i, t))
                .collect(),
        }
    }

    /// The file's sound tracks, in the file's order.
    pub(crate) fn sounds(&self) -> Vec<SoundTrack> {
        match self {
            Self::Matroska(d) => d
                .tracks()
                .iter()
                .filter(|t| t.kind == matroska::TrackKind::Audio && t.readable())
                .filter_map(|t| {
                    Some(SoundTrack {
                        number: t.number,
                        key: t.number,
                        codec: match t.codec {
                            matroska::Codec::Opus => SoundCodec::Opus,
                            matroska::Codec::Vorbis => SoundCodec::Vorbis,
                            _ if t.codec_id.starts_with(b"A_AAC") => SoundCodec::Aac,
                            _ => SoundCodec::Other,
                        },
                        config: t.codec_private.clone(),
                        enabled: t.enabled,
                        default: t.default,
                        time_base: d.time_base(t.number)?,
                        codec_delay: t.codec_delay,
                        seek_pre_roll: t.seek_pre_roll,
                    })
                })
                .collect(),
            Self::Mp4(d) => d
                .tracks()
                .iter()
                .enumerate()
                .filter(|(_, t)| t.kind == mp4::TrackKind::Audio)
                .filter_map(|(i, t)| {
                    let (codec, config, seek_pre_roll) = match t.codec {
                        // FFmpeg's `mov_read_dops`: the OpusHead, and Opus's
                        // 80 ms of pre-roll.
                        mp4::Codec::Opus => (SoundCodec::Opus, opus_head(&t.config), 80_000_000),
                        mp4::Codec::Aac => (SoundCodec::Aac, t.config.clone(), 0),
                        _ => (SoundCodec::Other, t.config.clone(), 0),
                    };
                    Some(SoundTrack {
                        number: u64::from(t.id),
                        key: u64::try_from(i).ok()?,
                        codec,
                        config,
                        enabled: true,
                        default: t.default,
                        time_base: (1, u64::from(t.timescale)),
                        // The priming comes with the packets
                        // (`Sample::skip_samples`), as FFmpeg's demuxer gives
                        // it; else the decoder's own pre-skip drops it.
                        codec_delay: 0,
                        seek_pre_roll,
                    })
                })
                .collect(),
        }
    }

    /// How long the file plays, in nanoseconds, if it says: Matroska's
    /// segment duration; for MP4, its longest track's (FFmpeg's duration of
    /// each, from `mdhd`, cut by its edit list or grown by its fragments).
    pub(crate) fn duration(&self) -> Option<u64> {
        match self {
            Self::Matroska(d) => segment_duration(d),
            Self::Mp4(d) => d
                .tracks()
                .iter()
                .filter_map(|t| {
                    let ticks = u64::try_from(t.duration).ok().filter(|&n| n > 0)?;
                    Some(time::duration_to_ns(ticks, (1, u64::from(t.timescale))))
                })
                .max(),
        }
    }

    /// The next packet of any track; `None` at the end.
    ///
    /// # Errors
    ///
    /// The container's own, when the source fails.
    pub(crate) fn next_packet(&mut self) -> Result<Option<Sample>, ContainerError> {
        match self {
            Self::Matroska(d) => Ok(d.next_packet()?.map(|p| {
                let alpha = p
                    .additions
                    .into_iter()
                    .find(|(id, _)| *id == 1)
                    .map(|(_, bytes)| bytes);
                Sample {
                    track: p.track,
                    time: p.timestamp,
                    duration: p.duration,
                    keyframe: p.keyframe,
                    discard: false,
                    data: p.data,
                    alpha,
                    new_config: None,
                    discard_padding: p.discard_padding,
                    skip_samples: 0,
                    position: p.position,
                }
            })),
            Self::Mp4(d) => {
                let Some(p) = d.next_packet()? else {
                    return Ok(None);
                };
                Ok(Some(Sample {
                    track: u64::try_from(p.track).unwrap_or(u64::MAX),
                    time: Some(p.pts),
                    duration: u64::try_from(p.duration).unwrap_or(0),
                    keyframe: p.keyframe,
                    discard: p.discard,
                    data: p.data,
                    alpha: None,
                    new_config: p.new_config,
                    discard_padding: 0,
                    skip_samples: u64::from(p.skip_samples),
                    position: p.position,
                }))
            }
        }
    }

    /// Go to the latest key frame at or before `ticks` of the track `key`
    /// names ([`Track::key`]).
    ///
    /// # Errors
    ///
    /// The container's own: the source failing, or a track that cannot be
    /// sought in.
    pub(crate) fn seek(&mut self, key: u64, ticks: i64) -> Result<(), ContainerError> {
        match self {
            Self::Matroska(d) => Ok(d.seek(key, ticks)?),
            Self::Mp4(d) => {
                let index = usize::try_from(key).map_err(|_| {
                    ContainerError::Mp4(mp4::Error::Invalid(
                        "a seek in a track the file does not have",
                    ))
                })?;
                Ok(d.seek(index, ticks)?)
            }
        }
    }
}

impl Track {
    /// The size to show a frame of size `width` x `height` at -- cropped,
    /// and turned the track's way: its height, and the width that gives it
    /// the shape the file asks -- turned with it, as ffmpeg's `transpose`
    /// turns a pixel's shape -- rounded to the nearest pixel, a half up. The
    /// frame's own size where the file asks nothing, or something that
    /// cannot be shown.
    pub(crate) fn display_size(&self, width: u32, height: u32) -> (u32, u32) {
        let turned = self.orientation.swaps_sides();
        // `height * a / b`, or `width * a / b`, rounded half up:
        // (2 * n * a + b) / (2 * b), which a u128 holds for any u32 and u64s.
        let scaled = |n: u32, a: u64, b: u64| {
            u128::from(n)
                .checked_mul(u128::from(a))
                .and_then(|v| v.checked_mul(2))
                .and_then(|v| v.checked_add(u128::from(b)))
                .and_then(|v| v.checked_div(u128::from(b).checked_mul(2)?))
                .and_then(|s| u32::try_from(s).ok())
        };
        let shown = match self.aspect {
            // FFmpeg's reading of Matroska's display size: a pixel's shape
            // in every unit but "unknown" (4).
            Aspect::Display {
                unit: 0..=3,
                width: dw,
                height: dh,
            } if dw > 0 && dh > 0 && height > 0 => {
                if turned {
                    scaled(height, dh, dw)
                } else {
                    scaled(height, dw, dh)
                }
            }
            Aspect::Pixel { num, den } if num > 0 && den > 0 && width > 0 => {
                let (num, den) = (u64::from(num.unsigned_abs()), u64::from(den.unsigned_abs()));
                if turned {
                    scaled(width, den, num)
                } else {
                    scaled(width, num, den)
                }
            }
            _ => None,
        };
        match shown {
            Some(w) => (w.max(1), height),
            None => (width, height),
        }
    }
}

/// A Matroska video track, if it has its `Video` element and a time base.
fn matroska_track<R: Read + Seek>(
    demuxer: &matroska::Demuxer<R>,
    t: &matroska::Track,
) -> Option<Track> {
    let picture = t.video?;
    let aspect = match (picture.display_width, picture.display_height) {
        (Some(width), Some(height)) => Aspect::Display {
            unit: picture.display_unit,
            width,
            height,
        },
        _ => Aspect::Square,
    };
    Some(Track {
        number: t.number,
        key: t.number,
        codec: matroska_codec(t),
        config: t.codec_private.clone(),
        enabled: t.enabled,
        default: t.default,
        width: picture.pixel_width,
        height: picture.pixel_height,
        crop: picture.crop,
        aspect,
        orientation: picture
            .display_matrix()
            .map_or(Orientation::Upright, |m| Orientation::from_matrix(&m)),
        alpha: picture.alpha_mode == 1,
        hint: ColourHint::matroska(picture.colour.as_ref()),
        frame_duration: t.default_duration,
        time_base: demuxer.time_base(t.number)?,
    })
}

/// A Matroska track's codec, by its codec ID: those WebM allows, and the
/// commonest of the rest by FFmpeg's names for them.
fn matroska_codec(t: &matroska::Track) -> Codec {
    match t.codec {
        matroska::Codec::Vp8 => Codec::Vp8,
        matroska::Codec::Vp9 => Codec::Vp9,
        matroska::Codec::Av1 => Codec::Av1,
        _ => match t.codec_id.as_slice() {
            b"V_MPEG4/ISO/AVC" => Codec::H264,
            b"V_MPEGH/ISO/HEVC" => Codec::Hevc,
            b"V_MPEG4/ISO/SP" | b"V_MPEG4/ISO/ASP" | b"V_MPEG4/ISO/AP" => Codec::Mpeg4,
            _ => Codec::Other,
        },
    }
}

/// An MP4 video track, the `index`th of the file's.
/// MP4's Opus setup (`dOps`) as the `OpusHead` an Ogg or Matroska file
/// carries, as FFmpeg's `mov_read_dops` makes it: the magic and version 1,
/// then the box's fields after its version, the pre-skip, the input rate and
/// the gain turned little-endian (the mapping after them is bytes either
/// way). Empty where the box is too short to be one.
fn opus_head(dops: &[u8]) -> Vec<u8> {
    let Some(rest) = dops.get(1..).filter(|r| r.len() >= 10) else {
        return Vec::new();
    };
    let mut head = Vec::with_capacity(rest.len().saturating_add(9));
    head.extend_from_slice(b"OpusHead");
    head.push(1);
    head.extend_from_slice(rest);
    // OpusHead's pre-skip, input rate and gain.
    for field in [10..12, 12..16, 16..18] {
        if let Some(field) = head.get_mut(field) {
            field.reverse();
        }
    }
    head
}

fn mp4_track(index: usize, t: &mp4::Track) -> Option<Track> {
    if t.kind != mp4::TrackKind::Video {
        return None;
    }
    let picture = t.video?;
    let [left, top, right, bottom] = picture.crop;
    // A crop that leaves nothing of the picture -- a clean aperture FFmpeg's
    // arithmetic wraps -- is no crop, as FFmpeg's decoders drop it.
    let fits = left.checked_add(right).is_some_and(|w| w < picture.width)
        && top.checked_add(bottom).is_some_and(|h| h < picture.height);
    let crop = if fits {
        [left, top, right, bottom].map(u64::from)
    } else {
        [0; 4]
    };
    let aspect = match picture.pixel_aspect {
        Some((num, den)) => Aspect::Pixel { num, den },
        None => Aspect::Square,
    };
    Some(Track {
        number: u64::from(t.id),
        key: u64::try_from(index).ok()?,
        codec: match t.codec {
            mp4::Codec::Vp8 => Codec::Vp8,
            mp4::Codec::Vp9 => Codec::Vp9,
            mp4::Codec::Av1 => Codec::Av1,
            mp4::Codec::H264 => Codec::H264,
            mp4::Codec::Hevc => Codec::Hevc,
            mp4::Codec::Mpeg4 => Codec::Mpeg4,
            _ => Codec::Other,
        },
        config: t.config.clone(),
        enabled: true,
        default: t.default,
        width: u64::from(picture.width),
        height: u64::from(picture.height),
        crop,
        aspect,
        orientation: picture
            .matrix
            .map_or(Orientation::Upright, |m| Orientation::from_matrix(&m)),
        alpha: false,
        hint: ColourHint::mp4(picture.colour.as_ref()),
        frame_duration: picture
            .frame_duration
            .filter(|&d| d > 0)
            .map(|d| time::duration_to_ns(u64::from(d), (1, u64::from(t.timescale)))),
        time_base: (1, u64::from(t.timescale.max(1))),
    })
}

/// The segment's duration in nanoseconds, if the file gives one that is a
/// finite, non-negative number of ticks.
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a timestamp scale is far inside f64's integer-exact range, and the product is checked finite, non-negative and in range first"
)]
fn segment_duration<R: Read + Seek>(demuxer: &matroska::Demuxer<R>) -> Option<u64> {
    let info = demuxer.info();
    let ns = info.duration? * info.timestamp_scale as f64;
    (ns.is_finite() && ns >= 0.0 && ns < u64::MAX as f64).then_some(ns as u64)
}

#[cfg(test)]
#[allow(clippy::indexing_slicing, reason = "a test: a failure should be loud")]
mod tests {
    use super::*;

    #[test]
    fn an_mp4_opus_setup_is_the_opushead_it_stands_for() {
        // dOps: version 0, 2 channels, pre-skip 312, 48 kHz, gain -3, family 0
        // -- big-endian.
        let dops = [0, 2, 0x01, 0x38, 0, 0, 0xbb, 0x80, 0xff, 0xfd, 0];
        let mut want = b"OpusHead".to_vec();
        want.extend_from_slice(&[1, 2, 0x38, 0x01, 0x80, 0xbb, 0, 0, 0xfd, 0xff, 0]);
        assert_eq!(opus_head(&dops), want);
        assert_eq!(opus::Head::parse(&want).map(|h| h.pre_skip), Some(312));
        // A mapping table after the family is copied as it is.
        let mut five_one = dops.to_vec();
        five_one[1] = 6;
        five_one[10] = 1;
        five_one.extend_from_slice(&[4, 2, 0, 4, 1, 2, 3, 5]);
        // The family at 18, then the stream counts and the mapping.
        assert_eq!(opus_head(&five_one)[18..], [1, 4, 2, 0, 4, 1, 2, 3, 5]);
        // Too short to be one: nothing.
        assert!(opus_head(&dops[..10]).is_empty());
    }

    fn track(aspect: Aspect) -> Track {
        Track {
            number: 1,
            key: 1,
            codec: Codec::Vp9,
            config: Vec::new(),
            enabled: true,
            default: true,
            width: 720,
            height: 576,
            crop: [0; 4],
            aspect,
            orientation: Orientation::Upright,
            alpha: false,
            hint: ColourHint::default(),
            frame_duration: None,
            time_base: (1, 1000),
        }
    }

    fn display(unit: u64, width: u64, height: u64) -> Aspect {
        Aspect::Display {
            unit,
            width,
            height,
        }
    }

    #[test]
    fn matroskas_display_size_keeps_the_height_and_takes_the_files_shape() {
        // 720x576 PAL shown 16:9: 1024x576.
        assert_eq!(
            track(display(0, 1024, 576)).display_size(720, 576),
            (1024, 576)
        );
        assert_eq!(track(display(3, 16, 9)).display_size(720, 576), (1024, 576));
        // 4:3, rounded to the nearest pixel: 576 * 4 / 3 = 768.
        assert_eq!(track(display(3, 4, 3)).display_size(720, 576), (768, 576));
        // Nothing said, a zero, or an unknown unit: the frame's own size.
        assert_eq!(track(Aspect::Square).display_size(720, 576), (720, 576));
        assert_eq!(track(display(0, 0, 9)).display_size(720, 576), (720, 576));
        assert_eq!(track(display(4, 16, 9)).display_size(720, 576), (720, 576));
    }

    #[test]
    fn mp4s_pixel_shape_widens_the_frame() {
        // PAL's 16:9 pixel, 64:45: 720 * 64 / 45 = 1024.
        let wide = Aspect::Pixel { num: 64, den: 45 };
        assert_eq!(track(wide).display_size(720, 576), (1024, 576));
        // A square pixel, and shapes that cannot be shown.
        for aspect in [
            Aspect::Pixel { num: 1, den: 1 },
            Aspect::Pixel { num: -4, den: 3 },
            Aspect::Pixel { num: 4, den: 0 },
        ] {
            assert_eq!(track(aspect).display_size(720, 576), (720, 576));
        }
        // 3:2 on 175 wide: 262.5, a half up to 263.
        let odd = Aspect::Pixel { num: 3, den: 2 };
        assert_eq!(track(odd).display_size(175, 144), (263, 144));
    }

    #[test]
    fn a_turned_frame_turns_its_shape_with_it() {
        let turned = |aspect| Track {
            orientation: Orientation::Clockwise,
            ..track(aspect)
        };
        // A 4:3 pixel on a 176 x 144 picture, turned a quarter: the frame is
        // 144 x 176, its pixel 3:4, shown 108 wide.
        let wide = Aspect::Pixel { num: 4, den: 3 };
        assert_eq!(turned(wide).display_size(144, 176), (108, 176));
        // Matroska's 16:9 on 720 x 576, turned: 9:16 at a height of 720.
        assert_eq!(turned(display(3, 16, 9)).display_size(576, 720), (405, 720));
        // Mirrored or turned half round, nothing changes shape.
        let half = Track {
            orientation: Orientation::HalfTurn,
            ..track(wide)
        };
        assert_eq!(half.display_size(176, 144), (235, 144));
    }

    #[test]
    fn a_file_is_told_by_its_first_bytes() {
        use std::io::Cursor;
        let refused = Container::open(Cursor::new(b"not a video at all".to_vec()));
        assert!(matches!(refused, Err(ContainerError::Unknown)));
        let refused = Container::open(Cursor::new(Vec::new()));
        assert!(matches!(refused, Err(ContainerError::Unknown)));
        // EBML's magic is Matroska, whatever follows; an ftyp box is MP4.
        let mkv = Container::open(Cursor::new(vec![0x1A, 0x45, 0xDF, 0xA3, 0x80]));
        assert!(matches!(mkv, Err(ContainerError::Matroska(_))));
        let mut mp4 = 16u32.to_be_bytes().to_vec();
        mp4.extend_from_slice(b"ftypisom\0\0\0\0");
        assert!(matches!(
            Container::open(Cursor::new(mp4)),
            Err(ContainerError::Mp4(_))
        ));
    }
}
