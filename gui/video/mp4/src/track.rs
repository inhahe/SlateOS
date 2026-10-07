//! A track's description: what kind of media it holds, its codec and the
//! codec's setup, its picture or its sound -- from `tkhd`, `mdhd`, `hdlr`
//! and the sample entry in `stsd`, as FFmpeg reads them.

/// What a track carries, as FFmpeg decides it: from the handler (`hdlr`),
/// then the sample entry's codec.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TrackKind {
    Video,
    Audio,
    Subtitle,
    /// Anything else: timecodes, metadata, hints.
    #[default]
    Data,
}

/// The crate's own name for it, the kind FFmpeg's `codec_type` is.
pub(crate) type Kind = TrackKind;

/// The codecs a player here may meet in MP4, by sample entry; anything else
/// is [`Codec::Other`], its four-character code in [`Track::codec_tag`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Codec {
    Av1,
    Vp9,
    Vp8,
    H264,
    Hevc,
    /// MPEG-4 Part 2.
    Mpeg4,
    Aac,
    Mp3,
    Opus,
    Flac,
    Ac3,
    Eac3,
    /// 3GPP timed text (`tx3g`; QuickTime's `text`): MP4's subtitles, what
    /// `ffmpeg -c:s mov_text`, HandBrake and phones write.
    MovText,
    /// TTML (`stpp`, ISO/IEC 14496-30): a TTML document a sample, what
    /// broadcasters' DASH segments carry.
    Ttml,
    #[default]
    Other,
}

/// What FFmpeg's subtitle table (`ff_codec_movsubtitle_tags`) makes of a
/// sample entry's code, for a track no handler made video or sound.
pub(crate) fn subtitle_codec(tag: [u8; 4]) -> Option<Codec> {
    match &tag {
        b"tx3g" | b"text" => Some(Codec::MovText),
        b"stpp" => Some(Codec::Ttml),
        // CEA-608 closed captions, which no player here reads.
        b"c608" => Some(Codec::Other),
        _ => None,
    }
}

/// What FFmpeg's sound table (`ff_codec_movaudio_tags`) makes of a sample
/// entry's code: the codec, `Other` for one it knows that no player here
/// reads, `None` for one it does not know.
pub(crate) fn audio_codec(tag: [u8; 4]) -> Option<Codec> {
    match &tag {
        b"mp4a" => Some(Codec::Aac),
        b".mp3" | b"mp3 " => Some(Codec::Mp3),
        b"Opus" => Some(Codec::Opus),
        b"fLaC" => Some(Codec::Flac),
        b"ac-3" | b"sac3" => Some(Codec::Ac3),
        b"ec-3" => Some(Codec::Eac3),
        b"alac" | b"samr" | b"sawb" | b"sawp" | b"twos" | b"sowt" | b"raw " | b"lpcm" | b"in24"
        | b"in32" | b"fl32" | b"fl64" | b"ulaw" | b"alaw" | b"ima4" | b"MAC3" | b"MAC6"
        | b"agsm" | b"Qclp" | b"Qclq" | b"sqcp" | b"QDM2" | b"QDMC" | b"dtsc" | b"dtsh"
        | b"dtsl" | b"dtse" | b"mlpa" | b"spex" | b"ipcm" | b"fpcm" | b"mha1" | b"mhm1"
        | b"iamf" | b"ac-4" => Some(Codec::Other),
        _ => None,
    }
}

/// What FFmpeg's picture tables (`ff_codec_movvideo_tags`, then
/// `ff_codec_bmp_tags`) make of a sample entry's code.
pub(crate) fn video_codec(tag: [u8; 4]) -> Option<Codec> {
    match &tag {
        b"av01" => Some(Codec::Av1),
        b"vp09" => Some(Codec::Vp9),
        b"vp08" => Some(Codec::Vp8),
        b"avc1" | b"avc2" | b"avc3" | b"avc4" | b"ai5p" | b"ai5q" | b"ai52" | b"ai53" | b"ai55"
        | b"ai56" | b"ai1p" | b"ai1q" | b"ai12" | b"ai13" | b"ai15" | b"ai16" | b"AVin"
        | b"aivx" | b"rv64" | b"xalg" | b"avlg" | b"dva1" | b"dvav" | b"H264" | b"h264" => {
            Some(Codec::H264)
        }
        b"hvc1" | b"hev1" | b"dvh1" | b"dvhe" | b"HEVC" => Some(Codec::Hevc),
        b"mp4v" | b"DIVX" | b"XVID" | b"3IV2" | b"FMP4" | b"DX50" => Some(Codec::Mpeg4),
        b"s263" | b"h263" | b"H263" | b"jpeg" | b"mjpa" | b"mjpb" | b"png " | b"MPNG" | b"apcn"
        | b"apch" | b"apcs" | b"apco" | b"ap4h" | b"ap4x" | b"rpza" | b"SVQ1" | b"SVQ3"
        | b"cvid" | b"raw " | b"yuv2" | b"2vuy" | b"AVdn" | b"AVdh" | b"dvc " | b"dvcp"
        | b"dvpp" | b"dv5n" | b"dv5p" | b"mp2v" | b"mp1v" | b"hdv1" | b"hdv2" | b"hdv3"
        | b"mx5n" | b"mx5p" | b"vc-1" | b"VP6F" | b"smc " | b"rle " | b"tiff" | b"gif "
        | b"vvc1" | b"vvi1" | b"evc1" | b"apv1" | b"MJPG" | b"WMV3" | b"WVC1" => Some(Codec::Other),
        _ => None,
    }
}

/// A video track's picture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Video {
    /// The coded size, from the sample entry.
    pub width: u32,
    pub height: u32,
    /// The size `tkhd` asks it shown at, whole pixels.
    pub track_width: u32,
    pub track_height: u32,
    /// The pixel's width to its height, as FFmpeg settles it
    /// (`sample_aspect_ratio`): `pasp`'s, else the display matrix's stretch,
    /// else the shape that shows the picture at `tkhd`'s size. In lowest
    /// terms, the denominator positive; `None` where nothing says. A
    /// damaged file can make it negative.
    pub pixel_aspect: Option<(i32, i32)>,
    /// What `colr` (`nclx` or `nclc`) and `vpcC` say of the colour, a later
    /// box over an earlier; `None` where neither is there.
    pub colour: Option<Colour>,
    /// The display matrix -- the track's (`tkhd`) after the movie's
    /// (`mvhd`) -- where it is not the identity: rotation, mirroring and
    /// stretching for the picture to be shown with, as FFmpeg gives it
    /// (`AV_PKT_DATA_DISPLAYMATRIX`). Row by row: `a b u / c d v / x y w`,
    /// with `u`, `v` and `w` in 2.30 fixed point and the rest 16.16.
    pub matrix: Option<[i32; 9]>,
    /// `clap`'s crop: left, top, right, bottom, in pixels of the coded
    /// picture, as FFmpeg works it out (`AV_PKT_DATA_FRAME_CROPPING`); all
    /// 0 where there is none.
    pub crop: [u32; 4],
    /// How long each frame lasts, in the track's ticks, where FFmpeg takes
    /// the track to have one frame rate (its `r_frame_rate`): every sample
    /// in `stts` but the last lasts as long as the first. `None` where the
    /// `moov` lists no samples (a fragmented file).
    pub frame_duration: Option<u32>,
}

/// A picture's colour, numbered as ITU-T H.273 numbers it, as `colr` and
/// `vpcC` say it and FFmpeg keeps it: a code point FFmpeg has no name for
/// is 2, "unspecified", as is one never said.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Colour {
    pub primaries: u16,
    pub transfer: u16,
    pub matrix: u16,
    /// Whether samples span every code (`true`) or the studio range:
    /// `nclx`'s flag, or `vpcC`'s; `None` where only `nclc` spoke, which has
    /// none.
    pub full_range: Option<bool>,
}

/// An audio track's sound.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Audio {
    pub channels: u32,
    /// Samples a second: the sample entry's, or the track's timescale where
    /// the entry gives none.
    pub sample_rate: u32,
}

/// One track of the file, in the order of its `trak` boxes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Track {
    /// The ID `tkhd` gives it (a fragment names its track by it).
    pub id: u32,
    pub kind: TrackKind,
    pub codec: Codec,
    /// The first sample entry's four-character code: `av01`, `vp09`, `mp4a`.
    pub codec_tag: [u8; 4],
    /// The codec's setup as the file stores it: the body of the sample
    /// entry's `av1C`, `vpcC`, `avcC`, `hvcC`, `dOps` or `dfLa`, or AAC's
    /// decoder-specific info from `esds`. Empty where there is none.
    pub config: Vec<u8>,
    /// Ticks a second: every time of the track's is in these.
    pub timescale: u32,
    /// How long the track lasts, in its ticks, as FFmpeg reckons it
    /// (`st->duration`): `mdhd`'s, cut by the edit list, or as far as the
    /// fragments reach.
    pub duration: i64,
    /// ISO 639-2/T, from `mdhd`; `und` when the file does not say.
    pub language: [u8; 3],
    /// `tkhd`'s "enabled" flag, which FFmpeg reads as the default track.
    pub default: bool,
    pub video: Option<Video>,
    pub audio: Option<Audio>,
}

/// What an `esds` says, as FFmpeg's `ff_mp4_read_dec_config_descr` reads it:
/// the object type (0x40 AAC, 0x6B MP3, ...) and the decoder-specific info.
pub(crate) fn read_esds(body: &[u8]) -> (Option<u8>, Vec<u8>) {
    // The ES descriptor: tag 3, a length, ES_ID, flags and what they add.
    let mut at = 4usize; // version and flags
    let mut object_type = None;
    let mut dsi = Vec::new();
    let Some((tag, _, next)) = descriptor(body, at) else {
        return (None, Vec::new());
    };
    at = next;
    if tag == 3 {
        let Some(&flags) = body.get(at.saturating_add(2)) else {
            return (None, Vec::new());
        };
        at = at.saturating_add(3);
        if flags & 0x80 != 0 {
            at = at.saturating_add(2);
        }
        if flags & 0x40 != 0 {
            let url = usize::from(body.get(at).copied().unwrap_or(0));
            at = at.saturating_add(1).saturating_add(url);
        }
        if flags & 0x20 != 0 {
            at = at.saturating_add(2);
        }
    } else {
        // An ES_ID alone, in some writers' descriptors.
        at = at.saturating_add(2);
    }
    // The decoder config descriptor: tag 4.
    if let Some((4, _, next)) = descriptor(body, at) {
        object_type = body.get(next).copied();
        // object type, stream type, buffer size (3), max and average rate.
        at = next.saturating_add(13);
        // The setup's own length, not bounded by the descriptors around
        // it, as FFmpeg reads it.
        if let Some((5, len, next)) = descriptor(body, at) {
            let stop = next.saturating_add(len).min(body.len());
            dsi = body.get(next..stop).unwrap_or_default().to_vec();
        }
    }
    (object_type, dsi)
}

/// An MPEG-4 descriptor's tag and length at `at`, and where its body begins.
fn descriptor(body: &[u8], at: usize) -> Option<(u8, usize, usize)> {
    let tag = *body.get(at)?;
    let mut len = 0usize;
    let mut i = at.checked_add(1)?;
    for _ in 0..4 {
        let b = *body.get(i)?;
        len = (len << 7) | usize::from(b & 0x7f);
        i = i.checked_add(1)?;
        if b & 0x80 == 0 {
            break;
        }
    }
    Some((tag, len, i))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_esds_gives_its_object_type_and_setup() {
        // version/flags; ES descriptor (3): ES_ID, flags 0; decoder config
        // (4): AAC (0x40), stream type, buffer, rates; specific info (5):
        // two bytes of AudioSpecificConfig.
        let body = [
            0, 0, 0, 0, 3, 25, 0, 1, 0, 4, 17, 0x40, 0x15, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 5, 2,
            0x11, 0x90, 6, 1, 2,
        ];
        assert_eq!(read_esds(&body), (Some(0x40), vec![0x11, 0x90]));
        // Lengths in the long form (0x80 continuation bytes).
        let long = [
            0, 0, 0, 0, 3, 0x80, 0x80, 0x80, 25, 0, 1, 0, 4, 0x80, 0x80, 0x80, 17, 0x6B, 0x15, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 5, 0x80, 0x80, 0x80, 1, 7,
        ];
        assert_eq!(read_esds(&long), (Some(0x6B), vec![7]));
        assert_eq!(read_esds(&[0, 0, 0]), (None, vec![]), "cut short");
    }

    #[test]
    fn a_code_is_sound_or_picture_by_ffmpegs_tables() {
        assert_eq!(audio_codec(*b"mp4a"), Some(Codec::Aac));
        assert_eq!(video_codec(*b"mp4a"), None);
        assert_eq!(video_codec(*b"av01"), Some(Codec::Av1));
        // Both tables know "raw ": the track's handler decides.
        assert_eq!(audio_codec(*b"raw "), Some(Codec::Other));
        assert_eq!(video_codec(*b"raw "), Some(Codec::Other));
        assert_eq!(audio_codec(*b"zzzz"), None);
    }

    #[test]
    fn a_code_is_subtitles_by_ffmpegs_table() {
        // 3GPP timed text, as MP4 and QuickTime name it.
        assert_eq!(subtitle_codec(*b"tx3g"), Some(Codec::MovText));
        assert_eq!(subtitle_codec(*b"text"), Some(Codec::MovText));
        // TTML, which FFmpeg's table names though it has no decoder.
        assert_eq!(subtitle_codec(*b"stpp"), Some(Codec::Ttml));
        // Subtitles, but none read here.
        assert_eq!(subtitle_codec(*b"c608"), Some(Codec::Other));
        // Not in FFmpeg's table: a data track with these stays one.
        assert_eq!(subtitle_codec(*b"wvtt"), None);
        assert_eq!(subtitle_codec(*b"avc1"), None);
    }
}
