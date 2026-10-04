//! Reading a file: every box when opened, then the samples as packets in
//! the order FFmpeg gives them (`mov_read_packet`), and seeking
//! (`mov_read_seek`).

use std::io::{Read, Seek};

use crate::index::{DISCARD, KEYFRAME, Stream, rescale, search_timestamp};
use crate::parse::{Description, Parser};
use crate::reader::Reader;
use crate::track::{Kind, Track};
use crate::{Codec, Error};

/// One sample of one track.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Packet {
    /// The track's place in [`Demuxer::tracks`].
    pub track: usize,
    /// When it is shown, in the track's timescale ([`Track::timescale`]).
    pub pts: i64,
    /// When it is decoded.
    pub dts: i64,
    /// How long it lasts; 0 if unknown.
    pub duration: i64,
    /// Whether decoding can start here.
    pub keyframe: bool,
    /// Outside the edit list: to be decoded, for the packets after it, and
    /// its picture or sound dropped.
    pub discard: bool,
    /// No other sample depends on it (`sdtp`).
    pub disposable: bool,
    /// Cut short by the end of the file: `data` holds what there is.
    pub corrupt: bool,
    pub data: Vec<u8>,
    /// Where its bytes begin in the file.
    pub position: u64,
    /// Sound to drop from the start of what this packet decodes to, in
    /// samples: the encoder's priming, or what a seek landed before.
    pub skip_samples: u32,
    /// The codec setup to decode this packet and those after it with, where
    /// it differs from the one before (a track of several sample entries).
    pub new_config: Option<Vec<u8>>,
}

/// An MP4 file being read.
pub struct Demuxer<R> {
    r: Reader<R>,
    tracks: Vec<Track>,
    streams: Vec<Stream>,
    /// FFmpeg reads nothing of a data track whose samples all last nothing.
    discarded: Vec<bool>,
}

/// FFmpeg's `AV_TIME_BASE`: the microseconds tracks are compared in.
const TIME_BASE: i64 = 1_000_000;

impl<R: Read + Seek> Demuxer<R> {
    /// Read `source`'s boxes: its tracks and every sample's place.
    ///
    /// # Errors
    ///
    /// When the source is not an MP4 file, has no `moov`, or its tables are
    /// damaged where FFmpeg refuses them.
    pub fn open(source: R) -> Result<Self, Error> {
        let mut r = Reader::new(source)?;
        let (streams, descriptions) = {
            let mut p = Parser::new(&mut r);
            p.read_header()?;
            (
                core::mem::take(&mut p.streams),
                core::mem::take(&mut p.descriptions),
            )
        };
        let tracks = streams
            .iter()
            .zip(&descriptions)
            .map(|(s, d)| track(s, d))
            .collect();
        let discarded = streams
            .iter()
            .map(|s| {
                s.kind == Kind::Data
                    && !s.stts.is_empty()
                    && s.stts.iter().all(|&(count, d)| count == 0 || d == 0)
            })
            .collect();
        let mut d = Self {
            r,
            tracks,
            streams,
            discarded,
        };
        for s in &mut d.streams {
            s.current_sample = 0;
            s.tts_index = 0;
            s.tts_sample = 0;
            s.stsc_index = 0;
            s.stsc_sample = 0;
            // A track's first sample entry other than the first: FFmpeg
            // starts from its setup.
            s.last_stsd_index = s
                .stsc
                .first()
                .filter(|e| e.id > 1 && e.id <= s.stsd_count)
                .map_or(0, |e| i64::from(e.id).saturating_sub(1));
        }
        Ok(d)
    }

    /// The tracks, in the file's order.
    pub fn tracks(&self) -> &[Track] {
        &self.tracks
    }

    /// The next packet, in FFmpeg's order: by position among tracks' samples
    /// within a second of each other, by time otherwise
    /// (`mov_find_next_sample`). `None` at the end.
    ///
    /// # Errors
    ///
    /// When the source fails.
    pub fn next_packet(&mut self) -> Result<Option<Packet>, Error> {
        loop {
            let Some(i) = self.next_stream() else {
                return Ok(None);
            };
            let Some(sc) = self.streams.get_mut(i) else {
                return Ok(None);
            };
            let Some(sample) = sc.index.get(sc.current_sample).copied() else {
                return Ok(None);
            };
            sc.current_sample = sc.current_sample.saturating_add(1);
            let discarded = self.discarded.get(i).copied().unwrap_or(false);
            let mut data = Vec::new();
            if !discarded {
                let pos = u64::try_from(sample.pos).unwrap_or(u64::MAX);
                if pos >= self.r.len() {
                    // A sample past the end of the file: FFmpeg's read
                    // fails there, and so does its reading.
                    return Ok(None);
                }
                self.r.seek_to(pos)?;
                // A sample cut off by the end of the file comes out short,
                // marked as FFmpeg marks it.
                let n = u64::from(sample.size).min(self.r.remaining());
                data = self.r.bytes(n)?;
            }
            let short =
                !discarded && data.len() < usize::try_from(sample.size).unwrap_or(usize::MAX);
            let p = self.finalize(i, sample, data).map(|mut p| {
                p.corrupt = short;
                p
            });
            if discarded {
                continue;
            }
            return Ok(p);
        }
    }

    /// The track whose next sample FFmpeg reads next.
    fn next_stream(&self) -> Option<usize> {
        let mut best: Option<(usize, i64, i64)> = None;
        for (i, s) in self.streams.iter().enumerate() {
            let Some(e) = s.index.get(s.current_sample) else {
                continue;
            };
            let dts = rescale(e.timestamp, TIME_BASE, i64::from(s.time_scale));
            let take = match best {
                None => true,
                Some((_, best_dts, best_pos)) => {
                    let diff = best_dts.abs_diff(dts);
                    (diff <= TIME_BASE.unsigned_abs() && e.pos < best_pos)
                        || (diff > TIME_BASE.unsigned_abs() && dts < best_dts)
                }
            };
            if take {
                best = Some((i, dts, e.pos));
            }
        }
        best.map(|(i, _, _)| i)
    }

    /// A sample as a packet (`mov_finalize_packet`), and the reading of its
    /// track moved on.
    fn finalize(&mut self, i: usize, sample: crate::index::Entry, data: Vec<u8>) -> Option<Packet> {
        let sc = self.streams.get_mut(i)?;
        let dts = sample.timestamp;
        let mut duration = 0i64;
        let tts = sc.tts.get(sc.tts_index).copied();
        if sc.has_stts
            && let Some(t) = tts
        {
            duration = i64::from(t.duration);
        }
        let pts = if sc.has_ctts
            && let Some(t) = tts
        {
            dts.saturating_add(i64::from(sc.dts_shift).saturating_add(i64::from(t.offset)))
        } else {
            if duration == 0 {
                let next = sc
                    .index
                    .get(sc.current_sample)
                    .map_or(sc.duration, |e| e.timestamp);
                if next >= dts {
                    duration = next.wrapping_sub(dts);
                }
            }
            dts
        };
        if let Some(t) = tts {
            sc.tts_sample = sc.tts_sample.saturating_add(1);
            if t.count == sc.tts_sample {
                sc.tts_index = sc.tts_index.saturating_add(1);
                sc.tts_sample = 0;
            }
        }
        let disposable = sc.current_sample <= sc.sdtp.len()
            && sc
                .sdtp
                .get(sc.current_sample.wrapping_sub(1))
                .is_some_and(|f| (f >> 2) & 3 == 2);
        let mut new_config = None;
        if let Some(run) = sc.stsc.get(sc.stsc_index).copied() {
            let id = i64::from(run.id);
            let entry = id.saturating_sub(1);
            if id > 0 && entry < i64::from(sc.stsd_count) && entry != sc.last_stsd_index {
                sc.last_stsd_index = entry;
                new_config = sc
                    .extradata
                    .get(usize::try_from(entry).unwrap_or(usize::MAX))
                    .filter(|c| !c.is_empty())
                    .cloned();
            }
            sc.stsc_sample = sc.stsc_sample.saturating_add(1);
            if sc.stsc_index.saturating_add(1) < sc.stsc.len()
                && stsc_samples(sc, sc.stsc_index) == sc.stsc_sample
            {
                sc.stsc_index = sc.stsc_index.saturating_add(1);
                sc.stsc_sample = 0;
            }
        }
        let skip = u32::try_from(sc.skip_samples.max(0)).unwrap_or(0);
        sc.skip_samples = 0;
        Some(Packet {
            track: i,
            pts,
            dts,
            duration,
            keyframe: sample.flags & KEYFRAME != 0,
            discard: sample.flags & DISCARD != 0,
            disposable,
            corrupt: false,
            data,
            position: u64::try_from(sample.pos).unwrap_or(0),
            skip_samples: skip,
            new_config,
        })
    }

    /// Go to the latest key frame of track `track` (its place in
    /// [`Self::tracks`]) whose presentation is at or before `timestamp` (in
    /// the track's timescale), and every other track to the sample at or
    /// before that key frame's time -- each track sought on its own, as
    /// FFmpeg seeks an MP4 file (`mov_read_seek`).
    ///
    /// # Errors
    ///
    /// When the track does not exist or has no sample to go to.
    pub fn seek(&mut self, track: usize, timestamp: i64) -> Result<(), Error> {
        self.seek_toward(track, timestamp, true)
    }

    /// As [`Self::seek`], but to the earliest key frame at or *after*
    /// `timestamp` -- where ffprobe's seek to a time of 0 goes, the
    /// direction FFmpeg's `avformat_seek_file` picks for it.
    ///
    /// # Errors
    ///
    /// As [`Self::seek`].
    pub fn seek_forward(&mut self, track: usize, timestamp: i64) -> Result<(), Error> {
        self.seek_toward(track, timestamp, false)
    }

    fn seek_toward(&mut self, track: usize, timestamp: i64, backward: bool) -> Result<(), Error> {
        let sample = self.seek_stream(track, timestamp, backward)?;
        let (seek_ts, scale) = {
            let sc = self
                .streams
                .get_mut(track)
                .ok_or(Error::Invalid("a seek in no track"))?;
            sc.skip_samples = skip_samples(sc, sample, self.tracks.get(track));
            let ts = sc.index.get(sample).map_or(0, |e| e.timestamp);
            (ts, sc.time_scale)
        };
        for i in 0..self.streams.len() {
            if i == track {
                continue;
            }
            let other_scale = self.streams.get(i).map_or(1, |s| s.time_scale);
            let ts = rescale_q(seek_ts, scale, other_scale);
            if let Ok(sample) = self.seek_stream(i, ts, backward)
                && let Some(sc) = self.streams.get_mut(i)
            {
                sc.skip_samples = skip_samples(sc, sample, self.tracks.get(i));
            }
        }
        Ok(())
    }

    /// One track to its sample for `timestamp` (`mov_seek_stream`).
    fn seek_stream(&mut self, i: usize, timestamp: i64, backward: bool) -> Result<usize, Error> {
        let sc = self
            .streams
            .get_mut(i)
            .ok_or(Error::Invalid("a seek in no track"))?;
        // The time is a presentation's: moved onto the decoding timeline.
        let wanted =
            timestamp.wrapping_sub(sc.min_corrected_pts.wrapping_add(i64::from(sc.dts_shift)));
        let sample = match search_timestamp(&sc.index, wanted, backward, false) {
            Some(s) => s,
            None if !sc.index.is_empty()
                && sc.index.first().is_some_and(|e| wanted < e.timestamp) =>
            {
                0
            }
            None => return Err(Error::Invalid("no sample to seek to")),
        };
        sc.current_sample = sample;
        // Where the sample is among the durations and offsets.
        let mut first = 0usize;
        sc.tts_index = 0;
        sc.tts_sample = 0;
        for (k, t) in sc.tts.iter().enumerate() {
            let next = first.saturating_add(usize::try_from(t.count).unwrap_or(usize::MAX));
            if next > sample {
                sc.tts_index = k;
                sc.tts_sample = u32::try_from(sample.saturating_sub(first)).unwrap_or(0);
                break;
            }
            first = next;
        }
        // And among the sample-to-chunk runs.
        if !sc.chunk_offsets.is_empty() {
            let mut first = 0i64;
            for k in 0..sc.stsc.len() {
                let next = first.saturating_add(stsc_samples(sc, k));
                if next > i64::try_from(sample).unwrap_or(i64::MAX) {
                    sc.stsc_index = k;
                    sc.stsc_sample = i64::try_from(sample).unwrap_or(0).saturating_sub(first);
                    break;
                }
                first = next;
            }
        }
        Ok(sample)
    }
}

/// The samples of `stsc` run `k`: its samples a chunk times its chunks
/// (`mov_get_stsc_samples`).
fn stsc_samples(sc: &Stream, k: usize) -> i64 {
    let Some(run) = sc.stsc.get(k) else {
        return 0;
    };
    let chunks = match sc.stsc.get(k.saturating_add(1)) {
        Some(next) => i64::from(next.first).saturating_sub(i64::from(run.first)),
        None => i64::try_from(sc.chunk_offsets.len())
            .unwrap_or(0)
            .saturating_sub(i64::from(run.first).saturating_sub(1)),
    };
    i64::from(run.count).saturating_mul(chunks)
}

/// FFmpeg's `mov_get_skip_samples`: of a sound track sought to `sample`, the
/// priming still to drop.
fn skip_samples(sc: &Stream, sample: usize, track: Option<&Track>) -> i32 {
    if sc.kind != Kind::Audio {
        return 0;
    }
    let rate = track.and_then(|t| t.audio).map_or(0, |a| a.sample_rate);
    let first = sc.index.first().map_or(0, |e| e.timestamp);
    let ts = sc.index.get(sample).map_or(0, |e| e.timestamp);
    let off = rescale_q(
        ts.wrapping_sub(first),
        sc.time_scale,
        i32::try_from(rate).unwrap_or(0),
    );
    let pad = i64::from(sc.start_pad).saturating_sub(off).max(0);
    i32::try_from(pad).unwrap_or(i32::MAX)
}

/// FFmpeg's `av_rescale_q` between two timescales (ticks a second).
fn rescale_q(ts: i64, from: i32, to: i32) -> i64 {
    // ts * (1/from) / (1/to) = ts * to / from.
    rescale(ts, i64::from(to), i64::from(from))
}

/// The public description of a track.
fn track(s: &Stream, d: &Description) -> Track {
    Track {
        id: u32::try_from(s.id).unwrap_or(0),
        kind: s.kind,
        codec: if s.kind == Kind::Audio || s.kind == Kind::Video {
            d.codec
        } else {
            Codec::Other
        },
        codec_tag: d.codec_tag,
        config: s.extradata.first().cloned().unwrap_or_default(),
        timescale: u32::try_from(s.time_scale).unwrap_or(1),
        duration: s.duration,
        language: d.language,
        default: d.default,
        video: (s.kind == Kind::Video).then(|| d.video()),
        audio: (s.kind == Kind::Audio).then(|| d.audio()),
    }
}
