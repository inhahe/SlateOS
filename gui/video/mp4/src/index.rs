//! A track's samples as FFmpeg indexes them: one entry per sample -- where
//! it is, how big, its decoding time, whether decoding may start there --
//! built from the sample tables (`mov_build_index`), then cut and retimed by
//! the edit list (`mov_fix_index`), with each sample's duration and
//! composition offset beside it.

use crate::track::Kind;

/// A sample may start decoding.
pub(crate) const KEYFRAME: u8 = 1;
/// A sample outside the edit list, kept only so that others decode: FFmpeg
/// gives it out marked to be decoded and its picture or sound dropped.
pub(crate) const DISCARD: u8 = 2;

/// One sample of the index (FFmpeg's `AVIndexEntry`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Entry {
    pub pos: i64,
    /// The decoding time, in the track's timescale.
    pub timestamp: i64,
    pub size: u32,
    pub min_distance: u32,
    pub flags: u8,
}

/// A run of samples' durations and composition offsets (`MOVTimeToSample`);
/// after merging, one per sample.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Tts {
    pub count: u32,
    pub offset: i32,
    pub duration: u32,
}

/// A sample-to-chunk run (`stsc`): from chunk `first` (counting from 1),
/// `count` samples a chunk, described by sample entry `id`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Stsc {
    pub first: u32,
    pub count: u32,
    pub id: u32,
}

/// An edit (`elst`): `duration` in the movie's timescale, from `time` in the
/// track's (-1 for an empty edit).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Edit {
    pub duration: i64,
    pub time: i64,
}

/// Everything FFmpeg keeps of a track (its `MOVStreamContext`), as far as
/// the packets need it.
#[derive(Clone, Debug, Default)]
pub(crate) struct Stream {
    pub kind: Kind,
    /// The track's ID from `tkhd`; -1 until it is read.
    pub id: i64,
    pub time_scale: i32,
    /// The track's length in its timescale, as FFmpeg keeps it (`st->duration`).
    pub duration: i64,

    // The sample tables.
    pub chunk_offsets: Vec<i64>,
    pub stsc: Vec<Stsc>,
    pub sample_sizes: Vec<u32>,
    /// `stsz`'s one size for every sample, 0 if each has its own.
    pub stsz_sample_size: u32,
    /// The size of a sample, from `stsd` (uncompressed sound) or `stsz`.
    pub sample_size: u32,
    pub sample_count: u32,
    pub stts: Vec<(u32, u32)>,
    /// (count, offset) runs, entries of count 0 or less dropped.
    pub ctts: Vec<(u32, i32)>,
    pub keyframes: Vec<u32>,
    /// An `stss` with no entries: no sample is a key frame (but see
    /// `mov_build_index`).
    pub keyframe_absent: bool,
    pub stps: Vec<u32>,
    pub sdtp: Vec<u8>,
    /// `sbgp` 'rap ' runs: (count, group description index).
    pub rap_group: Vec<(u32, u32)>,
    pub edits: Vec<Edit>,
    /// Which sample entry each `stsd` slot is; `-1` once a second entry
    /// has been skipped as a repeat of the first codec.
    pub pseudo_stream_id: i32,
    pub stsd_count: u32,
    /// Each sample entry's codec configuration, for a packet that changes
    /// to another entry.
    pub extradata: Vec<Vec<u8>>,

    // The audio entry's numbers, as `mov_parse_stsd_audio` reads them.
    pub samples_per_frame: u32,
    pub bytes_per_frame: u32,

    // Times, as `mov_read_ctts`, `mov_build_index` and `mov_fix_index` make
    // them.
    pub dts_shift: i32,
    pub time_offset: i64,
    pub min_corrected_pts: i64,
    pub start_pad: i32,
    /// Samples to skip from the start of the next packet read: FFmpeg's
    /// `sti->skip_samples`, given with the first packet and then 0.
    pub skip_samples: i32,
    pub track_end: i64,

    // The index.
    pub index: Vec<Entry>,
    pub tts: Vec<Tts>,
    /// Whether the track had an `stts` / a `ctts` (FFmpeg tests their
    /// counts after merging).
    pub has_stts: bool,
    pub has_ctts: bool,

    // Where reading is.
    pub current_sample: usize,
    pub tts_index: usize,
    pub tts_sample: u32,
    pub stsc_index: usize,
    pub stsc_sample: i64,
    pub last_stsd_index: i64,
}

impl Stream {
    pub(crate) fn new() -> Self {
        Self {
            id: -1,
            min_corrected_pts: 0,
            ..Self::default()
        }
    }
}

/// FFmpeg's `av_rescale`: `a * b / c`, rounded to the nearest, halves away
/// from zero; `i64::MIN` for a divisor that is not positive.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "products of two i64 values fit an i128 with room to spare, and c is positive"
)]
pub(crate) fn rescale(a: i64, b: i64, c: i64) -> i64 {
    if c <= 0 || b < 0 {
        return i64::MIN;
    }
    let (a, b, c) = (i128::from(a), i128::from(b), i128::from(c));
    let r = c / 2;
    let v = if a < 0 {
        -((-a * b + r) / c)
    } else {
        (a * b + r) / c
    };
    i64::try_from(v).unwrap_or(if v < 0 { i64::MIN } else { i64::MAX })
}

impl Stream {
    /// The index from the sample tables: FFmpeg's `mov_build_index`, then
    /// its `mov_fix_index` when the edit lists are applied.
    pub(crate) fn build_index(&mut self, advanced: bool, movie_scale: i32, codec: crate::Codec) {
        let mut current_dts: i64 = 0;
        if !self.edits.is_empty() {
            let (mut edit_start, mut multiple, mut empty_duration, mut start_time) =
                (0usize, false, 0i64, 0i64);
            for (i, e) in self.edits.iter().enumerate() {
                if i == 0 && e.time == -1 {
                    // An empty first edit: the track starts that much late.
                    empty_duration = e.duration;
                    edit_start = 1;
                } else if i == edit_start && e.time >= 0 {
                    start_time = e.time;
                } else {
                    multiple = true;
                }
            }
            if (empty_duration != 0 || start_time != 0) && movie_scale > 0 {
                if empty_duration != 0 {
                    empty_duration = rescale(
                        empty_duration,
                        i64::from(self.time_scale),
                        i64::from(movie_scale),
                    );
                }
                self.time_offset = start_time.wrapping_sub(empty_duration);
                self.min_corrected_pts = start_time;
                if !advanced {
                    current_dts = self.time_offset.wrapping_neg();
                }
            }
            if !multiple && !advanced && codec == crate::Codec::Aac && start_time > 0 {
                self.start_pad = i32::try_from(start_time).unwrap_or(i32::MAX);
            }
        }

        let chunked = self.kind == Kind::Audio
            && self.stts.len() == 1
            && self.stts.first().is_some_and(|&(_, d)| d == 1);
        if chunked {
            if !self.build_chunked(current_dts) {
                return;
            }
        } else if !self.build_samples(current_dts) {
            return;
        }
        if advanced {
            self.fix_index(movie_scale, codec);
        }
    }

    /// FFmpeg's `mov_merge_tts_data`: `stts` and `ctts` spread out to one
    /// entry a sample, as far as there are samples. `false` where FFmpeg
    /// gives up.
    fn merge_tts(&mut self, merge_ctts: bool, merge_stts: bool) -> bool {
        if self.ctts.is_empty() && self.stts.is_empty() {
            return true;
        }
        let samples = usize::try_from(self.sample_count).unwrap_or(0);
        if samples == 0 {
            return false;
        }
        let mut tts = vec![Tts::default(); samples];
        let mut filled = 0usize;
        if merge_ctts && !self.ctts.is_empty() {
            for &(count, offset) in &self.ctts {
                for _ in 0..count {
                    let Some(t) = tts.get_mut(filled) else {
                        break;
                    };
                    t.offset = offset;
                    t.count = 1;
                    filled = filled.saturating_add(1);
                }
            }
        } else {
            self.has_ctts = false;
        }
        let mut stts_filled = 0usize;
        if merge_stts && !self.stts.is_empty() {
            for &(count, duration) in &self.stts {
                for _ in 0..count {
                    let Some(t) = tts.get_mut(stts_filled) else {
                        break;
                    };
                    t.duration = duration;
                    t.count = 1;
                    stts_filled = stts_filled.saturating_add(1);
                }
            }
        } else {
            self.has_stts = false;
        }
        tts.truncate(filled.max(stts_filled));
        self.tts = tts;
        true
    }

    /// The usual index: a sample at a time (FFmpeg's main loop in
    /// `mov_build_index`). `false` where FFmpeg returns from it early.
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "counters bounded by the tables' lengths; offsets checked against overflow, times wrapping as C's"
    )]
    fn build_samples(&mut self, start_dts: i64) -> bool {
        let mut current_dts = start_dts.wrapping_sub(i64::from(self.dts_shift));
        if self.sample_count == 0 || !self.index.is_empty() || !self.tts.is_empty() {
            return false;
        }
        if !self.merge_tts(true, true) {
            return false;
        }
        let key_off = u32::from(
            self.keyframes.first().is_some_and(|&k| k > 0)
                || self.stps.first().is_some_and(|&k| k > 0),
        );
        let rap_present = !self.rap_group.is_empty();
        let (mut stsc_index, mut stss_index, mut stps_index) = (0usize, 0usize, 0usize);
        let (mut stts_index, mut stts_sample, mut current_sample) = (0usize, 0u32, 0u32);
        let (mut rap_index, mut rap_sample) = (0usize, 0u32);
        let mut distance = 0u32;
        let chunks = core::mem::take(&mut self.chunk_offsets);
        let mut finished = true;
        'chunks: for (i, &chunk) in chunks.iter().enumerate() {
            let next_offset = chunks.get(i + 1).copied().unwrap_or(i64::MAX);
            let mut current_offset = chunk;
            while stsc_index + 1 < self.stsc.len()
                && self
                    .stsc
                    .get(stsc_index + 1)
                    .is_some_and(|s| u64::try_from(i + 1).is_ok_and(|n| n == u64::from(s.first)))
            {
                stsc_index += 1;
            }
            let Some(&run) = self.stsc.get(stsc_index) else {
                finished = false;
                break;
            };
            if next_offset > current_offset
                && self.sample_size > 0
                && self.sample_size < self.stsz_sample_size
                && i64::from(run.count) * i64::from(self.stsz_sample_size)
                    > next_offset - current_offset
            {
                // "STSZ sample size invalid (too large), ignoring".
                self.stsz_sample_size = self.sample_size;
            }
            if self.stsz_sample_size > 0 && self.stsz_sample_size < self.sample_size {
                self.stsz_sample_size = self.sample_size;
            }
            for j in 0..run.count {
                if current_sample >= self.sample_count {
                    // "wrong sample count": FFmpeg stops here.
                    finished = false;
                    break 'chunks;
                }
                let mut keyframe = false;
                if !self.keyframe_absent
                    && (self.keyframes.is_empty()
                        || self.keyframes.get(stss_index) == Some(&(current_sample + key_off)))
                {
                    keyframe = true;
                    if stss_index + 1 < self.keyframes.len() {
                        stss_index += 1;
                    }
                } else if !self.stps.is_empty()
                    && self.stps.get(stps_index) == Some(&(current_sample + key_off))
                {
                    keyframe = true;
                    if stps_index + 1 < self.stps.len() {
                        stps_index += 1;
                    }
                }
                if rap_present && let Some(&(count, group)) = self.rap_group.get(rap_index) {
                    if group > 0 {
                        keyframe = true;
                    }
                    rap_sample += 1;
                    if rap_sample == count {
                        rap_sample = 0;
                        rap_index += 1;
                    }
                }
                if self.keyframe_absent
                    && self.stps.is_empty()
                    && !rap_present
                    && (self.kind == Kind::Audio || (i == 0 && j == 0))
                {
                    keyframe = true;
                }
                if keyframe {
                    distance = 0;
                }
                let sample_size = if self.stsz_sample_size > 0 {
                    self.stsz_sample_size
                } else {
                    self.sample_sizes
                        .get(usize::try_from(current_sample).unwrap_or(usize::MAX))
                        .copied()
                        .unwrap_or(0)
                };
                if current_offset > i64::MAX - i64::from(sample_size) {
                    finished = false;
                    break 'chunks;
                }
                if self.pseudo_stream_id == -1
                    || i64::from(run.id) - 1 == i64::from(self.pseudo_stream_id)
                {
                    if sample_size > 0x3FFF_FFFF {
                        finished = false;
                        break 'chunks;
                    }
                    self.index.push(Entry {
                        pos: current_offset,
                        timestamp: current_dts,
                        size: sample_size,
                        min_distance: distance,
                        flags: if keyframe { KEYFRAME } else { 0 },
                    });
                }
                current_offset += i64::from(sample_size);
                let step = self.tts.get(stts_index).map_or(0, |t| t.duration);
                current_dts = current_dts.wrapping_add(i64::from(step));
                distance = distance.wrapping_add(1);
                stts_sample += 1;
                current_sample += 1;
                if stts_index + 1 < self.tts.len()
                    && self
                        .tts
                        .get(stts_index)
                        .is_some_and(|t| stts_sample == t.count)
                {
                    stts_sample = 0;
                    stts_index += 1;
                }
            }
        }
        self.chunk_offsets = chunks;
        finished
    }

    /// Uncompressed sound in chunks, a packet of up to 1024 samples (FFmpeg's
    /// second loop in `mov_build_index`). `false` where FFmpeg returns early.
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "counts bounded by the tables; sizes checked against FFmpeg's limit"
    )]
    fn build_chunked(&mut self, start_dts: i64) -> bool {
        let mut current_dts = start_dts;
        if self.chunk_offsets.is_empty() || !self.tts.is_empty() {
            return false;
        }
        let spf = self.samples_per_frame;
        let mut total: u64 = 0;
        for (i, run) in self.stsc.iter().enumerate() {
            let chunk_samples = run.count;
            if i + 1 != self.stsc.len() && spf != 0 && chunk_samples % spf != 0 {
                return false;
            }
            let count = if spf >= 160 {
                chunk_samples / spf
            } else if spf > 1 {
                let samples = (1024 / spf) * spf;
                chunk_samples.div_ceil(samples.max(1))
            } else {
                chunk_samples.div_ceil(1024)
            };
            let chunk_count = match self.stsc.get(i + 1) {
                Some(next) => u64::from(next.first.wrapping_sub(run.first)),
                None => u64::try_from(self.chunk_offsets.len())
                    .unwrap_or(0)
                    .wrapping_sub(u64::from(run.first).wrapping_sub(1)),
            };
            total = total.wrapping_add(chunk_count.wrapping_mul(u64::from(count)));
        }
        let mut stsc_index = 0usize;
        let chunks = core::mem::take(&mut self.chunk_offsets);
        let mut finished = true;
        'chunks: for (i, &chunk) in chunks.iter().enumerate() {
            let mut current_offset = chunk;
            if stsc_index + 1 < self.stsc.len()
                && self
                    .stsc
                    .get(stsc_index + 1)
                    .is_some_and(|s| u64::try_from(i + 1).is_ok_and(|n| n == u64::from(s.first)))
            {
                stsc_index += 1;
            }
            let mut chunk_samples = self.stsc.get(stsc_index).map_or(0, |s| s.count);
            while chunk_samples > 0 {
                if spf > 1 && self.bytes_per_frame == 0 {
                    finished = false;
                    break 'chunks;
                }
                let (size, samples) = if spf >= 160 {
                    (self.bytes_per_frame, spf)
                } else if spf > 1 {
                    let samples = ((1024 / spf) * spf).min(chunk_samples);
                    ((samples / spf).wrapping_mul(self.bytes_per_frame), samples)
                } else {
                    let samples = chunk_samples.min(1024);
                    (samples.wrapping_mul(self.sample_size), samples)
                };
                if u64::try_from(self.index.len()).unwrap_or(u64::MAX) >= total
                    || size > 0x3FFF_FFFF
                {
                    finished = false;
                    break 'chunks;
                }
                self.index.push(Entry {
                    pos: current_offset,
                    timestamp: current_dts,
                    size,
                    min_distance: 0,
                    flags: KEYFRAME,
                });
                current_offset = current_offset.wrapping_add(i64::from(size));
                current_dts = current_dts.wrapping_add(i64::from(samples));
                chunk_samples = chunk_samples.saturating_sub(samples);
            }
        }
        self.chunk_offsets = chunks;
        if !finished {
            return false;
        }
        // Only the composition offsets merge here: a packet's duration is the
        // time to the next one's.
        self.merge_tts(true, false)
    }
}

/// FFmpeg's `ff_index_search_timestamp`: the entry for `wanted`, of `entries`
/// in time order -- the last at or before it (`backward`) or the first at or
/// after it -- a key frame unless `any`, stepping over discarded entries in
/// the binary search as FFmpeg does.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "indices between -1 and the entries' count, as FFmpeg's ints"
)]
pub(crate) fn search_timestamp(
    entries: &[Entry],
    wanted: i64,
    backward: bool,
    any: bool,
) -> Option<usize> {
    let nb = i64::try_from(entries.len()).ok()?;
    let at = |m: i64| usize::try_from(m).ok().and_then(|m| entries.get(m));
    let (mut a, mut b) = (-1i64, nb);
    if b > 0 && at(b - 1).is_some_and(|e| e.timestamp < wanted) {
        a = b - 1;
    }
    while b - a > 1 {
        let mut m = (a + b) >> 1;
        while at(m).is_some_and(|e| e.flags & DISCARD != 0) && m < b && m < nb - 1 {
            m += 1;
            if m == b && at(m).is_some_and(|e| e.timestamp >= wanted) {
                m = b - 1;
                break;
            }
        }
        let t = at(m).map_or(i64::MAX, |e| e.timestamp);
        if t >= wanted {
            b = m;
        }
        if t <= wanted {
            a = m;
        }
    }
    let mut m = if backward { a } else { b };
    if !any {
        while m >= 0 && m < nb && at(m).is_some_and(|e| e.flags & KEYFRAME == 0) {
            m += if backward { -1 } else { 1 };
        }
    }
    if m == nb || m < 0 {
        return None;
    }
    usize::try_from(m).ok()
}

impl Stream {
    /// FFmpeg's `find_prev_closest_index`: the sample to start an edit from,
    /// at or before the presentation time `pts` -- with composition offsets,
    /// a key frame shown at or before it -- and, with them, its place among
    /// the offsets. `None` where FFmpeg finds none.
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "indices bounded by the old index and its offsets, as FFmpeg's"
    )]
    fn prev_closest(
        &self,
        old: &[Entry],
        tts: &[Tts],
        mut pts: i64,
        any: bool,
        tts_index: &mut i64,
        tts_sample: &mut i64,
    ) -> Option<i64> {
        if self.dts_shift > 0 {
            pts -= i64::from(self.dts_shift);
        }
        let mut index = i64::try_from(search_timestamp(old, pts, true, any)?).ok()?;
        let entry = |i: i64| usize::try_from(i).ok().and_then(|i| old.get(i));
        // Back over entries of the same time, to the earliest acceptable.
        let mut i = index;
        while i > 0 && entry(i).map(|e| e.timestamp) == entry(i - 1).map(|e| e.timestamp) {
            if any || entry(i - 1).is_some_and(|e| e.flags & KEYFRAME != 0) {
                index = i - 1;
            }
            i -= 1;
        }
        if self.has_ctts && index >= 0 {
            let count = i64::try_from(tts.len()).unwrap_or(0);
            let tts_at = |i: i64| usize::try_from(i).ok().and_then(|i| tts.get(i));
            *tts_index = 0;
            *tts_sample = 0;
            for _ in 0..index {
                if *tts_index < count {
                    *tts_sample += 1;
                    if tts_at(*tts_index).is_some_and(|t| i64::from(t.count) == *tts_sample) {
                        *tts_index += 1;
                        *tts_sample = 0;
                    }
                }
            }
            while index >= 0 && *tts_index >= 0 && *tts_index < count {
                let shown = entry(index).map_or(i64::MAX, |e| {
                    e.timestamp + i64::from(tts_at(*tts_index).map_or(0, |t| t.offset))
                });
                if shown <= pts && entry(index).is_some_and(|e| e.flags & KEYFRAME != 0) {
                    break;
                }
                index -= 1;
                if *tts_sample == 0 {
                    *tts_index -= 1;
                    if *tts_index >= 0 {
                        *tts_sample = tts_at(*tts_index).map_or(0, |t| i64::from(t.count)) - 1;
                    }
                } else {
                    *tts_sample -= 1;
                }
            }
        }
        (index >= 0).then_some(index)
    }

    /// FFmpeg's `mov_fix_index`: the index rebuilt to the edit list -- each
    /// edit's samples, from the key frame its first needs, retimed onto the
    /// presentation's clock; samples outside the edits kept but marked
    /// [`DISCARD`]; sound's samples cut at an edit's start counted as samples
    /// to skip.
    #[allow(
        clippy::arithmetic_side_effects,
        clippy::too_many_lines,
        reason = "FFmpeg's 64-bit time arithmetic on the track's own times; one function, as FFmpeg's, to be read beside it"
    )]
    pub(crate) fn fix_index(&mut self, movie_scale: i32, codec: crate::Codec) {
        if self.edits.is_empty() || self.index.is_empty() {
            return;
        }
        let audio = self.kind == Kind::Audio;
        let vorbis = false; // No Vorbis in MP4 that FFmpeg's tables name.
        let _ = codec;
        let old = core::mem::take(&mut self.index);
        let tts_old = core::mem::take(&mut self.tts);
        let tts_count_old = i64::try_from(tts_old.len()).unwrap_or(0);
        let old_at = |i: i64| usize::try_from(i).ok().and_then(|i| old.get(i)).copied();
        let tts_at = |i: i64| {
            usize::try_from(i)
                .ok()
                .and_then(|i| tts_old.get(i))
                .copied()
        };
        let (mut tts_index_old, mut tts_sample_old) = (0i64, 0i64);
        let mut edit_list_dts_entry_end = 0i64;
        let mut edit_list_dts_counter;
        let mut edit_list_start_tts_sample;
        let mut empty_edits_sum_duration = 0i64;
        let mut found_non_empty_edit = false;
        let mut first_non_zero_audio_edit = -1i32;
        let mut frame_duration_buffer: Option<Vec<i64>> = None;
        let mut num_discarded_begin;
        let shift = i64::from(self.dts_shift);
        self.min_corrected_pts = -1;
        if self.dts_shift > 0 {
            edit_list_dts_entry_end -= shift;
        }
        let start_dts = edit_list_dts_entry_end;
        let first_ts = old.first().map_or(0, |e| e.timestamp);

        for edit in self.edits.clone() {
            let media_time = edit.time;
            let mut duration = rescale(
                edit.duration,
                i64::from(self.time_scale),
                i64::from(movie_scale),
            );
            if movie_scale == 0 {
                // FFmpeg asks for a sample and stops reading the edits.
                break;
            }
            if duration.wrapping_add(media_time) < duration && media_time > 0 {
                duration = 0;
            }
            edit_list_dts_counter = edit_list_dts_entry_end;
            edit_list_dts_entry_end = edit_list_dts_entry_end.wrapping_add(duration);
            num_discarded_begin = 0usize;
            if !found_non_empty_edit && media_time == -1 {
                empty_edits_sum_duration += duration;
                continue;
            }
            found_non_empty_edit = true;
            if audio {
                first_non_zero_audio_edit = if first_non_zero_audio_edit < 0 { 1 } else { 0 };
                if first_non_zero_audio_edit > 0 {
                    self.skip_samples = 0;
                    self.start_pad = 0;
                }
            }
            let mut search = media_time;
            if audio {
                // A second of sound before the edit, for the decoder's delay.
                search = (search - i64::from(self.time_scale)).max(first_ts);
            }
            let mut index = match self.prev_closest(
                &old,
                &tts_old,
                search,
                false,
                &mut tts_index_old,
                &mut tts_sample_old,
            ) {
                Some(i) => i,
                None => match self.prev_closest(
                    &old,
                    &tts_old,
                    search,
                    true,
                    &mut tts_index_old,
                    &mut tts_sample_old,
                ) {
                    Some(i) => i,
                    None => {
                        tts_index_old = 0;
                        tts_sample_old = 0;
                        0
                    }
                },
            };
            edit_list_start_tts_sample = tts_sample_old;
            let mut edit_list_start_encountered = false;
            let mut found_keyframe_after_edit = false;
            while let Some(current) = old_at(index) {
                let frame_duration = match old_at(index + 1) {
                    Some(next) => next.timestamp - current.timestamp,
                    None => duration,
                };
                let mut flags = current.flags;
                let mut curr_cts = current.timestamp + shift;
                let mut curr_ctts = 0i64;
                if !tts_old.is_empty() && tts_index_old < tts_count_old {
                    let t = tts_at(tts_index_old).unwrap_or_default();
                    curr_ctts = i64::from(t.offset);
                    curr_cts += curr_ctts;
                    tts_sample_old += 1;
                    if tts_sample_old == i64::from(t.count) {
                        self.tts.push(Tts {
                            count: u32::try_from(i64::from(t.count) - edit_list_start_tts_sample)
                                .unwrap_or(0),
                            offset: t.offset,
                            duration: t.duration,
                        });
                        tts_index_old += 1;
                        tts_sample_old = 0;
                        edit_list_start_tts_sample = 0;
                    }
                }
                if curr_cts < media_time || curr_cts >= duration + media_time {
                    if audio
                        && !vorbis
                        && curr_cts < media_time
                        && curr_cts + frame_duration > media_time
                        && first_non_zero_audio_edit > 0
                    {
                        // A frame across the edit's start: kept, its sound
                        // before the start skipped.
                        let skip = media_time - curr_cts;
                        self.skip_samples = self
                            .skip_samples
                            .wrapping_add(i32::try_from(skip).unwrap_or(i32::MAX));
                        edit_list_dts_counter -= skip;
                        if !edit_list_start_encountered {
                            edit_list_start_encountered = true;
                            if let Some(buffer) = frame_duration_buffer.take() {
                                fix_timestamps(
                                    &mut self.index,
                                    edit_list_dts_counter,
                                    &buffer,
                                    num_discarded_begin,
                                );
                            }
                        }
                    } else {
                        flags |= DISCARD;
                        if !edit_list_start_encountered {
                            num_discarded_begin += 1;
                            frame_duration_buffer
                                .get_or_insert_with(Vec::new)
                                .push(frame_duration);
                            if audio && first_non_zero_audio_edit > 0 && !vorbis {
                                self.skip_samples = self.skip_samples.wrapping_add(
                                    i32::try_from(frame_duration).unwrap_or(i32::MAX),
                                );
                            }
                        }
                    }
                } else {
                    let pts = edit_list_dts_counter + curr_ctts + shift;
                    self.min_corrected_pts = if self.min_corrected_pts < 0 {
                        pts
                    } else {
                        self.min_corrected_pts.min(pts)
                    };
                    if !edit_list_start_encountered {
                        edit_list_start_encountered = true;
                        if let Some(buffer) = frame_duration_buffer.take() {
                            fix_timestamps(
                                &mut self.index,
                                edit_list_dts_counter,
                                &buffer,
                                num_discarded_begin,
                            );
                        }
                    }
                }
                self.index.push(Entry {
                    pos: current.pos,
                    timestamp: edit_list_dts_counter,
                    size: current.size,
                    min_distance: current.min_distance,
                    flags,
                });
                if edit_list_start_encountered {
                    edit_list_dts_counter += frame_duration;
                }
                if curr_cts + frame_duration >= duration + media_time
                    && (flags & KEYFRAME != 0 || audio)
                {
                    if self.has_ctts {
                        // A key frame after the edit: one more, for the
                        // B-frames after it that may belong to the edit.
                        if !audio && !found_keyframe_after_edit {
                            found_keyframe_after_edit = true;
                            index += 1;
                            continue;
                        }
                        if tts_sample_old != 0 {
                            let t = tts_at(tts_index_old).unwrap_or_default();
                            self.tts.push(Tts {
                                count: u32::try_from(tts_sample_old - edit_list_start_tts_sample)
                                    .unwrap_or(0),
                                offset: t.offset,
                                duration: t.duration,
                            });
                        }
                    }
                    break;
                }
                index += 1;
            }
        }
        self.min_corrected_pts -= empty_edits_sum_duration;
        if self.kind == Kind::Video && self.min_corrected_pts > 0 {
            for e in &mut self.index {
                e.timestamp -= self.min_corrected_pts;
            }
        }
        self.duration = self.duration.min(edit_list_dts_entry_end - start_dts);
        self.start_pad = self.skip_samples;
    }
}

/// FFmpeg's `fix_index_entry_timestamps`: the discarded entries before an
/// edit's start given times counting back from it, so that times rise.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "durations of the track's own samples"
)]
fn fix_timestamps(index: &mut [Entry], mut end_ts: i64, durations: &[i64], count: usize) {
    let end = index.len();
    for i in 0..count {
        let Some(&d) = durations.get(count - 1 - i) else {
            break;
        };
        end_ts -= d;
        if let Some(e) = end.checked_sub(1 + i).and_then(|k| index.get_mut(k)) {
            e.timestamp = end_ts;
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        reason = "a test: a failure should be loud"
    )]

    use super::*;

    fn entry(timestamp: i64, flags: u8) -> Entry {
        Entry {
            pos: 0,
            timestamp,
            size: 1,
            min_distance: 0,
            flags,
        }
    }

    #[test]
    fn rescale_rounds_to_the_nearest_halves_away_from_zero() {
        assert_eq!(rescale(3, 1, 2), 2, "1.5 up");
        assert_eq!(rescale(-3, 1, 2), -2, "-1.5 down");
        assert_eq!(rescale(10, 3, 4), 8, "7.5 up");
        assert_eq!(rescale(1, 1, 0), i64::MIN, "no divisor");
        assert_eq!(rescale(i64::MAX, 2, 2), i64::MAX, "no overflow on the way");
    }

    #[test]
    fn the_search_finds_key_frames_backward_and_forward() {
        let index = [
            entry(0, KEYFRAME),
            entry(10, 0),
            entry(20, KEYFRAME),
            entry(30, 0),
        ];
        assert_eq!(search_timestamp(&index, 25, true, false), Some(2));
        assert_eq!(
            search_timestamp(&index, 25, false, false),
            None,
            "no key frame after"
        );
        assert_eq!(search_timestamp(&index, 5, false, false), Some(2));
        assert_eq!(
            search_timestamp(&index, 15, true, true),
            Some(1),
            "any frame"
        );
        assert_eq!(
            search_timestamp(&index, -5, true, false),
            None,
            "before them all"
        );
        assert_eq!(search_timestamp(&[], 0, true, false), None);
    }

    #[test]
    fn the_search_steps_over_discarded_entries_as_ffmpeg_does() {
        // Entries kept only for decoding: the binary search passes them for
        // the next kept one, then walks back to a key frame.
        let index = [
            entry(-30, KEYFRAME | DISCARD),
            entry(-20, DISCARD),
            entry(-10, DISCARD),
            entry(0, 0),
            entry(10, KEYFRAME),
            entry(20, 0),
        ];
        assert_eq!(search_timestamp(&index, 0, true, false), Some(0));
        assert_eq!(search_timestamp(&index, 0, false, false), Some(4));
    }
}
