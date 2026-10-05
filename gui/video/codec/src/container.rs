//! The file's container -- Matroska (WebM among them), MP4, Ogg, a native
//! FLAC file, or an MPEG audio one (`.mp3`, `.mp2`, `.mp1`) -- told apart by
//! its first bytes as FFmpeg tells them, and read through one face:
//! its video tracks described alike ([`Track`]), its sound tracks
//! ([`SoundTrack`]), its packets given alike ([`Sample`]), and a seek.
//!
//! **Telling them apart.** A Matroska file begins with EBML's magic, and
//! FFmpeg's Matroska probe asks for it at the very start; an Ogg file with a
//! page, as FFmpeg's Ogg probe asks (`ogg::probe`); a FLAC file with its
//! `fLaC` marker, past an ID3v2 tag as libavformat skips one before probing;
//! anything else is MP4 when FFmpeg's MP4 probe would take it for one
//! (`mp4::probe`, over as much of the file as FFmpeg reads to decide, a
//! mebibyte), else MPEG audio when FFmpeg's MP3 probe would take it, past
//! its ID3v2 tags (`mp3::is_mpeg_audio`), and otherwise not a file this
//! plays.
//!
//! **An MPEG audio file** is taken apart by `mp3::Reader`, as FFmpeg's MP3
//! demuxer and parser take it apart: a packet a frame (any junk before a
//! frame dropped, as is what is left at the end holding none), timed on
//! FFmpeg's clock, the LAME tag's delay and padding given as each packet's
//! skip and discard.
//!
//! **A FLAC file** is read by libFLAC's reader (`flac::Reader`), which finds
//! its frames by decoding them: it gives no packets, and `Sound` takes its
//! frames from it directly.
//!
//! **Ogg's times** are its demuxer's, which leaves a packet untimed where
//! the file does (after the first on a stream's last page); those are
//! filled in here as libavformat fills them in for every player built on
//! it -- the last packet's time and length -- so that what plays is timed
//! as FFmpeg times it.

use std::io::{Read, Seek, SeekFrom};

use crate::{Codec, ColourHint, ContainerError, Orientation, SoundCodec, SubtitleFormat, time};

/// How much of a file is read to tell what it is: the most FFmpeg reads to
/// decide (`PROBE_BUF_MAX`), or the whole of a smaller file.
const PROBE: u64 = 1 << 20;

/// EBML's magic, with which every Matroska and WebM file begins.
const EBML: [u8; 4] = [0x1A, 0x45, 0xDF, 0xA3];

/// How far ahead a reader of one small track -- sound, subtitles -- reads a
/// Matroska or MP4 file, where it passes over most of the bytes: each run of
/// the others' passed over costs a read of this much past it. Measured on a
/// minute of 1080p film at 5 Mbit/s (`film_read_track_by_track` in
/// `gui/video/matroska/tests/beyond_ffprobe.rs` and
/// `gui/video/mp4/tests/fixtures.rs`): its sound or its subtitles alone read
/// 82% of the file 64 KiB ahead, 15% 4 KiB ahead and 4% 1 KiB ahead, at
/// about a read a video frame; 512 bytes ahead read no less, in twice the
/// reads.
pub(crate) const PASSING_READ_AHEAD: usize = 1024;

/// A file being read.
// Each boxed: the two differ in size by hundreds of bytes, and a `Video`
// holds one for as long as it plays.
pub(crate) enum Container<R> {
    Matroska(Box<matroska::Demuxer<R>>),
    Mp4(Box<mp4::Demuxer<R>>),
    Ogg(Box<OggFile<R>>),
    Flac(Box<flac::Reader<R>>),
    Mp3(Box<mp3::Reader<R>>),
}

/// An Ogg file, and the times libavformat would fill in for its packets.
pub(crate) struct OggFile<R> {
    demuxer: ogg::Demuxer<R>,
    /// Each stream's next packet's time, in its ticks, where the last
    /// packet's time and length give it (libavformat's `cur_dts`).
    next: Vec<Option<i64>>,
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
    /// Matroska's `BlockAdditional` 1: for a WebM video frame, its alpha
    /// channel; for an `S_TEXT/WEBVTT` cue, its settings and identifier.
    pub addition: Option<Vec<u8>>,
    /// The codec setup this packet and those after it decode with, where it
    /// changes (an MP4 track of several sample entries).
    pub new_config: Option<Vec<u8>>,
    /// Sound to discard, in nanoseconds: from the end of the packet's if
    /// positive, its start if negative (Matroska's `DiscardPadding`; 0 in
    /// MP4 and Ogg).
    pub discard_padding: i64,
    /// Sound to discard from the end of what the packet decodes to, in
    /// samples (Ogg's: what its stream's last page says runs past the end;
    /// 0 in Matroska and MP4).
    pub discard_samples: u64,
    /// Sound to drop from the start of what this packet decodes to, in
    /// samples (MP4's: the priming its edit list leaves out, given with the
    /// first packet and with the first after a seek into it; Ogg's: an Opus
    /// stream's pre-skip, with its first packet and each chained link's; 0
    /// in Matroska, whose codec delay is the track's).
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

/// A subtitle track.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SubtitleTrack {
    /// Matroska's track number: what a caller names it by.
    pub number: u64,
    /// What the container's packets and seeks name it by.
    pub key: u64,
    pub format: SubtitleFormat,
    /// The codec ID as written: WebM's WebVTT (`D_WEBVTT/…`) and Matroska's
    /// (`S_TEXT/WEBVTT`) frame a cue differently.
    pub codec_id: Vec<u8>,
    /// Its setup: an ASS or SSA track's script header (`CodecPrivate`).
    pub config: Vec<u8>,
    pub enabled: bool,
    pub default: bool,
    pub forced: bool,
    /// The track's tick: `num / den` seconds.
    pub time_base: (u64, u64),
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
        } else if ogg::probe(&head) {
            let demuxer = ogg::Demuxer::open(source)?;
            let next = vec![None; demuxer.streams().len()];
            Ok(Self::Ogg(Box::new(OggFile { demuxer, next })))
        } else if is_flac(&head) {
            Ok(Self::Flac(Box::new(flac::Reader::open(source)?)))
        } else if mp4::probe(&head) {
            Ok(Self::Mp4(Box::new(mp4::Demuxer::open(source)?)))
        } else if mp3::is_mpeg_audio(&mut source).map_err(|e| ContainerError::Io(e.kind()))? {
            let reader = mp3::Reader::open(source).map_err(|e| ContainerError::Io(e.kind()))?;
            Ok(Self::Mp3(Box::new(reader)))
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
            Self::Ogg(f) => f
                .demuxer
                .streams()
                .iter()
                .enumerate()
                .filter(|(_, s)| s.codec == ogg::Codec::Theora)
                .filter_map(|(i, s)| theora_track(i, s))
                .collect(),
            Self::Flac(_) | Self::Mp3(_) => Vec::new(),
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
                            _ if t.codec_id == b"A_FLAC" => SoundCodec::Flac,
                            _ if [&b"A_MPEG/L3"[..], b"A_MPEG/L2", b"A_MPEG/L1"]
                                .contains(&t.codec_id.as_slice()) =>
                            {
                                SoundCodec::Mp3
                            }
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
                        mp4::Codec::Flac => (SoundCodec::Flac, t.config.clone(), 0),
                        mp4::Codec::Mp3 => (SoundCodec::Mp3, t.config.clone(), 0),
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
            Self::Ogg(f) => f
                .demuxer
                .streams()
                .iter()
                .enumerate()
                .filter_map(|(i, s)| ogg_sound(&f.demuxer, i, s))
                .collect(),
            Self::Flac(r) => r
                .metadata()
                .stream_info
                .map(|info| SoundTrack {
                    number: 1,
                    key: 0,
                    codec: SoundCodec::Flac,
                    config: Vec::new(),
                    enabled: true,
                    default: true,
                    time_base: (1, u64::from(info.sample_rate.max(1))),
                    codec_delay: 0,
                    seek_pre_roll: 0,
                })
                .into_iter()
                .collect(),
            // One stream: its packets' times are FFmpeg's ticks, and its
            // reader goes back itself, far enough for the bit reservoir.
            Self::Mp3(r) => r
                .info()
                .first
                .map(|first| SoundTrack {
                    number: 1,
                    key: 0,
                    codec: SoundCodec::Mp3,
                    config: first.raw.to_be_bytes().to_vec(),
                    enabled: true,
                    default: true,
                    time_base: (1, mp3::TICKS_PER_SECOND),
                    codec_delay: 0,
                    seek_pre_roll: 0,
                })
                .into_iter()
                .collect(),
        }
    }

    /// From here on, give out only the packets of the track `key` names, and
    /// read ahead `read_ahead` bytes at a time if given. The other tracks'
    /// packets are passed over unread, as FFmpeg passes over a discarded
    /// stream's: Matroska's once a block's header names its track
    /// (`matroska::Demuxer::select_tracks`), MP4's samples without a read
    /// (`mp4::Demuxer::select_tracks`). An Ogg file's pages carry its
    /// streams together, and are read as before, for the caller to drop
    /// the other streams' packets.
    ///
    /// # Errors
    ///
    /// The container's own, when the source cannot be put back where reading
    /// is.
    pub(crate) fn read_only(
        &mut self,
        key: u64,
        read_ahead: Option<usize>,
    ) -> Result<(), ContainerError> {
        match self {
            Self::Matroska(d) => {
                d.select_tracks(Some(&[key]));
                if let Some(bytes) = read_ahead {
                    d.set_read_ahead(bytes)?;
                }
            }
            Self::Mp4(d) => {
                if let Ok(index) = usize::try_from(key) {
                    d.select_tracks(Some(&[index]));
                }
                if let Some(bytes) = read_ahead {
                    d.set_read_ahead(bytes)?;
                }
            }
            Self::Ogg(_) | Self::Flac(_) | Self::Mp3(_) => {}
        }
        Ok(())
    }

    /// The file's subtitle tracks, in the file's order: Matroska's, its
    /// encrypted ones aside, and MP4's. Ogg's (Kate) are not read, and the
    /// other files have none.
    pub(crate) fn subtitles(&self) -> Vec<SubtitleTrack> {
        match self {
            Self::Matroska(d) => d
                .tracks()
                .iter()
                .filter(|t| t.kind == matroska::TrackKind::Subtitle && t.readable())
                .filter_map(|t| {
                    Some(SubtitleTrack {
                        number: t.number,
                        key: t.number,
                        format: subtitle_format(&t.codec_id),
                        codec_id: t.codec_id.clone(),
                        config: t.codec_private.clone(),
                        enabled: t.enabled,
                        default: t.default,
                        forced: t.forced,
                        time_base: d.time_base(t.number)?,
                    })
                })
                .collect(),
            Self::Mp4(d) => d
                .tracks()
                .iter()
                .enumerate()
                // WebVTT (`wvtt`), which FFmpeg's demuxer and so `mp4` make
                // a data track -- FFmpeg reads none of it -- is subtitles
                // here.
                .filter(|(_, t)| {
                    t.kind == mp4::TrackKind::Subtitle
                        || (t.kind == mp4::TrackKind::Data && t.codec_tag == *b"wvtt")
                })
                .filter_map(|(i, t)| {
                    Some(SubtitleTrack {
                        number: u64::from(t.id),
                        key: u64::try_from(i).ok()?,
                        format: if t.codec == mp4::Codec::MovText {
                            SubtitleFormat::MovText
                        } else if t.codec_tag == *b"wvtt" {
                            SubtitleFormat::WebVtt
                        } else {
                            SubtitleFormat::Other
                        },
                        codec_id: t.codec_tag.to_vec(),
                        config: t.config.clone(),
                        enabled: true,
                        default: t.default,
                        forced: false,
                        time_base: (1, u64::from(t.timescale.max(1))),
                    })
                })
                .collect(),
            Self::Ogg(_) | Self::Flac(_) | Self::Mp3(_) => Vec::new(),
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
            // Each stream's sound, from its first to its last page's end.
            Self::Ogg(f) => (0..f.demuxer.streams().len())
                .filter_map(|i| {
                    let ticks = f.demuxer.duration(i).filter(|&n| n > 0)?;
                    Some(time::duration_to_ns(ticks, f.demuxer.time_base(i)?))
                })
                .max(),
            // STREAMINFO's count of samples, where it gives one.
            Self::Flac(r) => {
                let info = r.metadata().stream_info?;
                (info.total_samples > 0 && info.sample_rate > 0).then(|| {
                    time::duration_to_ns(info.total_samples, (1, u64::from(info.sample_rate)))
                })
            }
            // The Xing, Info or VBRI frame's count of frames; else estimated
            // from the file's size at the first frame's bit rate, as FFmpeg
            // estimates it.
            Self::Mp3(r) => r
                .info()
                .duration
                .map(|ticks| time::duration_to_ns(ticks, (1, mp3::TICKS_PER_SECOND))),
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
                let addition = p
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
                    addition,
                    new_config: None,
                    discard_padding: p.discard_padding,
                    discard_samples: 0,
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
                    addition: None,
                    new_config: p.new_config,
                    discard_padding: 0,
                    discard_samples: 0,
                    skip_samples: u64::from(p.skip_samples),
                    position: p.position,
                }))
            }
            Self::Ogg(f) => Ok(f.next_sample()?),
            // Its frames are `Sound`'s to read (`flac::Reader`).
            Self::Flac(_) => Ok(None),
            Self::Mp3(r) => loop {
                let Some(p) = r.next_packet().map_err(|e| ContainerError::Io(e.kind()))? else {
                    return Ok(None);
                };
                // What holds no whole frame -- junk, the end of a file cut
                // short -- FFmpeg's decoder refuses whole ("Header missing",
                // an incomplete frame): it plays nothing, and is not damage.
                // Junk before a frame is dropped and the frame decoded, where
                // FFmpeg's decoder, given both, refuses both.
                let Some(frame) = p.frame_bytes() else {
                    continue;
                };
                return Ok(Some(Sample {
                    track: 0,
                    time: Some(p.pts),
                    duration: u64::try_from(p.duration).unwrap_or(0),
                    keyframe: true,
                    discard: false,
                    data: frame.to_vec(),
                    addition: None,
                    new_config: None,
                    discard_padding: 0,
                    discard_samples: p.discard_padding,
                    skip_samples: p.skip_samples,
                    position: p.position,
                }));
            },
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
            Self::Ogg(f) => {
                let index = usize::try_from(key).map_err(|_| {
                    ContainerError::Ogg(ogg::Error::Invalid(
                        "a seek in a stream the file does not have",
                    ))
                })?;
                f.demuxer.seek(index, ticks)?;
                // What libavformat knew of the times before is forgotten.
                f.next.iter_mut().for_each(|n| *n = None);
                Ok(())
            }
            // `Sound` seeks its reader itself, to the sample.
            Self::Flac(_) => Ok(()),
            Self::Mp3(r) => r.seek(ticks).map_err(|e| ContainerError::Io(e.kind())),
        }
    }
}

/// A Matroska subtitle track's format, by its codec ID as FFmpeg's demuxer
/// knows them. WebVTT's descriptions and metadata are not for showing, and
/// are no format read here.
fn subtitle_format(codec_id: &[u8]) -> SubtitleFormat {
    match codec_id {
        b"S_TEXT/UTF8" => SubtitleFormat::SubRip,
        b"S_TEXT/ASS" | b"S_ASS" => SubtitleFormat::Ass,
        b"S_TEXT/SSA" | b"S_SSA" => SubtitleFormat::Ssa,
        b"D_WEBVTT/SUBTITLES" | b"D_WEBVTT/CAPTIONS" | b"S_TEXT/WEBVTT" => SubtitleFormat::WebVtt,
        b"S_HDMV/PGS" => SubtitleFormat::Pgs,
        b"S_VOBSUB" => SubtitleFormat::VobSub,
        b"S_DVBSUB" => SubtitleFormat::Dvb,
        _ => SubtitleFormat::Other,
    }
}

/// Whether `head` -- a file's first bytes -- begins a FLAC file: its `fLaC`
/// marker, at the start or past an ID3v2 tag (ten bytes, then its size in
/// four bytes of seven bits).
fn is_flac(head: &[u8]) -> bool {
    if head.starts_with(b"fLaC") {
        return true;
    }
    if !head.starts_with(b"ID3") {
        return false;
    }
    let Some(size) = head.get(6..10) else {
        return false;
    };
    let size = size
        .iter()
        .fold(0usize, |s, &b| s << 7 | usize::from(b & 0x7f));
    head.get(10usize.saturating_add(size)..)
        .is_some_and(|rest| rest.starts_with(b"fLaC"))
}

impl<R: Read + Seek> OggFile<R> {
    /// The next packet, its time filled in as libavformat fills it in: the
    /// last packet's time and length, where the file gives none and the
    /// packet has a length.
    fn next_sample(&mut self) -> Result<Option<Sample>, ContainerError> {
        let Some(p) = self.demuxer.next_packet()? else {
            return Ok(None);
        };
        let next = self.next.get_mut(p.stream);
        let duration = i64::try_from(p.duration).unwrap_or(i64::MAX);
        let time = match (p.pts, &next) {
            (Some(t), _) => Some(t),
            (None, Some(Some(n))) if p.duration > 0 => Some(*n),
            _ => None,
        };
        if let (Some(slot), Some(t)) = (next, time) {
            *slot = Some(t.saturating_add(duration));
        }
        let stream = self.demuxer.streams().get(p.stream);
        let new_config = match (p.new_headers, stream.map(|s| s.codec)) {
            (Some(h), Some(codec)) => Some(sound_config(codec, &h)),
            _ => None,
        };
        Ok(Some(Sample {
            track: u64::try_from(p.stream).unwrap_or(u64::MAX),
            time,
            duration: p.duration,
            keyframe: true,
            discard: false,
            data: p.data,
            addition: None,
            new_config,
            discard_padding: 0,
            discard_samples: u64::from(p.discard_padding),
            skip_samples: u64::from(p.skip_samples),
            position: p.position,
        }))
    }
}

/// An Ogg stream's sound track, where it is one: numbered as ffprobe numbers
/// streams, from 1.
fn ogg_sound<R: Read + Seek>(
    demuxer: &ogg::Demuxer<R>,
    index: usize,
    s: &ogg::Stream,
) -> Option<SoundTrack> {
    let codec = match s.codec {
        ogg::Codec::Opus => SoundCodec::Opus,
        ogg::Codec::Vorbis => SoundCodec::Vorbis,
        ogg::Codec::Flac => SoundCodec::Flac,
        ogg::Codec::Speex => SoundCodec::Other,
        _ => return None,
    };
    // FLAC's packets are untimed here: their frames carry their own sample
    // numbers, at STREAMINFO's rate.
    let flac_rate = (codec == SoundCodec::Flac)
        .then(|| s.headers.first().and_then(|h| crate::sound::flac_info(h)))
        .flatten()
        .map(|info| (1, u64::from(info.sample_rate.max(1))));
    Some(SoundTrack {
        number: u64::try_from(index).ok()?.checked_add(1)?,
        key: u64::try_from(index).ok()?,
        codec,
        config: sound_config(s.codec, &s.headers),
        enabled: true,
        default: true,
        time_base: flac_rate
            .or_else(|| demuxer.time_base(index))
            .unwrap_or((1, 1)),
        // Opus's pre-skip comes with the first packet (`Sample::skip_samples`),
        // as FFmpeg's demuxer gives it.
        codec_delay: 0,
        // FFmpeg's Ogg demuxer gives Opus 80 ms of pre-roll.
        seek_pre_roll: if codec == SoundCodec::Opus {
            80_000_000
        } else {
            0
        },
    })
}

/// A stream's headers as the codec setup `Sound` takes: Opus's `OpusHead`;
/// Vorbis's three headers, Xiph-laced as a Matroska track's private data
/// holds them (empty where there are not three).
fn sound_config(codec: ogg::Codec, headers: &[Vec<u8>]) -> Vec<u8> {
    match (codec, headers) {
        (ogg::Codec::Vorbis, [id, comments, setup]) => {
            let mut laced = vec![2u8];
            for h in [id, comments] {
                let mut n = h.len();
                while n >= 255 {
                    laced.push(255);
                    n = n.saturating_sub(255);
                }
                laced.push(u8::try_from(n).unwrap_or(0));
            }
            for h in [id, comments, setup] {
                laced.extend_from_slice(h);
            }
            laced
        }
        (ogg::Codec::Vorbis, _) => Vec::new(),
        _ => headers.first().cloned().unwrap_or_default(),
    }
}

/// An Ogg Theora stream, as a video track: refused by its codec, which is not
/// decoded here, so that a film's pictures are refused by name.
fn theora_track(index: usize, s: &ogg::Stream) -> Option<Track> {
    // The identification header's picture size: 24 bits each, at 14 and 17.
    let header = s.headers.first()?;
    let field = |at: usize| -> u64 {
        header
            .get(at..at.saturating_add(3))
            .map_or(0, |b| b.iter().fold(0u64, |v, &x| v << 8 | u64::from(x)))
    };
    Some(Track {
        number: u64::try_from(index).ok()?.checked_add(1)?,
        key: u64::try_from(index).ok()?,
        codec: Codec::Theora,
        config: header.clone(),
        enabled: true,
        default: true,
        width: field(14),
        height: field(17),
        crop: [0; 4],
        aspect: Aspect::Square,
        orientation: Orientation::Upright,
        alpha: false,
        hint: ColourHint::default(),
        frame_duration: None,
        time_base: (1, 1000),
    })
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
    fn vorbis_headers_are_laced_as_matroska_holds_them() {
        let headers = [vec![1u8; 30], vec![3u8; 300], vec![5u8; 7]];
        let laced = sound_config(ogg::Codec::Vorbis, &headers);
        assert_eq!(laced[..4], [2, 30, 255, 45]);
        assert_eq!(laced.len(), 4 + 30 + 300 + 7);
        assert!(sound_config(ogg::Codec::Vorbis, &headers[..2]).is_empty());
        assert_eq!(
            sound_config(ogg::Codec::Opus, &[b"OpusHead".to_vec()]),
            b"OpusHead"
        );
    }

    #[test]
    fn a_file_is_told_by_its_first_bytes() {
        use std::io::Cursor;
        let refused = Container::open(Cursor::new(b"not a video at all".to_vec()));
        assert!(matches!(refused, Err(ContainerError::Unknown)));
        let refused = Container::open(Cursor::new(Vec::new()));
        assert!(matches!(refused, Err(ContainerError::Unknown)));
        // EBML's magic is Matroska, whatever follows; an Ogg page's start is
        // Ogg; an ftyp box is MP4.
        let mkv = Container::open(Cursor::new(vec![0x1A, 0x45, 0xDF, 0xA3, 0x80]));
        assert!(matches!(mkv, Err(ContainerError::Matroska(_))));
        let ogg = Container::open(Cursor::new(b"OggS\0\x02 and no more".to_vec()));
        assert!(matches!(ogg, Err(ContainerError::Ogg(_))));
        let mut mp4 = 16u32.to_be_bytes().to_vec();
        mp4.extend_from_slice(b"ftypisom\0\0\0\0");
        assert!(matches!(
            Container::open(Cursor::new(mp4)),
            Err(ContainerError::Mp4(_))
        ));
    }
}
