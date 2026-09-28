//! An AVIF image sequence's `moov` box -- its tracks and their sample tables
//! -- read as libavif 1.3.0 reads it: `avifParseMovieBox` and the parsers
//! under it (`src/read.c`).
//!
//! A sequence is a track of AV1 frames, and perhaps a second track of their
//! alpha. Each frame is a *sample*; samples are gathered into *chunks*, and
//! the sample table says where each chunk starts (`stco`/`co64`), how many
//! samples each holds (`stsc`), and how long each sample is (`stsz`). Frame
//! *n*'s bytes are found by walking those three together
//! ([`SampleTable::samples`]).
//!
//! libavif keeps every box's entries as it reads them, and so does this, with
//! one difference that changes nothing it answers: the list of samples those
//! entries describe -- up to 2,592,000 of them from a few dozen bytes of
//! table -- is walked, not stored.
//!
//! Portions of this file are copyright 2019 Joe Drago, from libavif, and
//! used under its BSD-2-Clause licence: `licenses/libavif-LICENSE.txt`.

use alloc::vec::Vec;

use super::Error;
use super::container::{self, FourCc, Meta, Property};
use super::stream::{Stream, Truncated};

/// `AVIF_RESULT_BMFF_PARSE_FAILED` for a box named `what`.
const fn bad(what: &'static str) -> impl Fn(Truncated) -> Error + Copy {
    move |_| Error::Parse(what)
}

/// The size of a `VisualSampleEntry`'s own fields, before its child boxes.
const VISUAL_SAMPLE_ENTRY_SIZE: usize = 78;

/// A track duration of all ones: "indefinite".
const INDEFINITE_DURATION32: u32 = u32::MAX;

/// How many times an animation plays after the first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Repetition {
    /// No edit list: the file does not say (`AVIF_REPETITION_COUNT_UNKNOWN`).
    Unknown,
    /// Forever (`AVIF_REPETITION_COUNT_INFINITE`).
    Infinite,
    /// This many more times.
    Count(u32),
}

/// One `trak` box: `avifTrack`.
#[derive(Debug)]
pub(super) struct Track<'a> {
    /// `tkhd`'s track ID; 0 for a track without one.
    pub(super) id: u32,
    /// `tref`: the track this one is the auxiliary (alpha) track of, and the
    /// track whose alpha this one's colour is premultiplied by.
    pub(super) aux_for: u32,
    pub(super) prem_by: u32,
    pub(super) media_timescale: u32,
    pub(super) media_duration: u64,
    track_duration: u64,
    segment_duration: u64,
    is_repeating: bool,
    pub(super) repetition: Repetition,
    /// `tkhd`'s width and height, whole pixels.
    pub(super) width: u32,
    pub(super) height: u32,
    /// `mdia`'s `hdlr`: `pict`, `vide` or `auxv` for a picture track.
    handler: FourCc,
    pub(super) sample_table: Option<SampleTable<'a>>,
    /// The track's own `meta` box, which holds its Exif and XMP.
    pub(super) meta: Meta<'a>,
}

impl Track<'_> {
    fn new() -> Self {
        Self {
            id: 0,
            aux_for: 0,
            prem_by: 0,
            media_timescale: 0,
            media_duration: 0,
            track_duration: 0,
            segment_duration: 0,
            is_repeating: false,
            repetition: Repetition::Unknown,
            width: 0,
            height: 0,
            handler: [0; 4],
            sample_table: None,
            meta: Meta::default(),
        }
    }
}

/// A sample entry of `stsd`: `avifSampleDescription`.
#[derive(Debug)]
pub(super) struct SampleDescription<'a> {
    pub(super) format: FourCc,
    /// The entry's child boxes, read as item properties -- for an `av01`
    /// entry; any other format's are not read.
    pub(super) properties: Vec<Property<'a>>,
}

/// One `stsc` entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SampleToChunk {
    first_chunk: u32,
    samples_per_chunk: u32,
}

/// One `stts` entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct TimeToSample {
    pub(super) sample_count: u32,
    pub(super) sample_delta: u32,
}

/// A track's `stbl` box: `avifSampleTable`. Each of its boxes may appear more
/// than once, and libavif appends what each holds.
#[derive(Debug, Default)]
pub(super) struct SampleTable<'a> {
    chunks: Vec<u64>,
    sample_to_chunks: Vec<SampleToChunk>,
    sample_sizes: Vec<u32>,
    /// `stsz`'s one size for every sample, when it gives one.
    all_samples_size: u32,
    pub(super) sync_samples: Vec<u32>,
    pub(super) time_to_samples: Vec<TimeToSample>,
    pub(super) descriptions: Vec<SampleDescription<'a>>,
}

/// Where one sample's bytes are in the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Sample {
    pub(super) offset: u64,
    pub(super) size: u32,
}

impl<'a> SampleTable<'a> {
    /// Whether the table has any chunk: a track without one is passed over.
    pub(super) fn has_chunks(&self) -> bool {
        !self.chunks.is_empty()
    }

    /// `avifSampleTableGetCodecType`: whether any sample entry is AV1.
    pub(super) fn is_av1(&self) -> bool {
        self.descriptions
            .iter()
            .any(|d| container::is_av1(&d.format))
    }

    /// `avifSampleTableGetProperties`: the first AV1 sample entry's
    /// properties.
    pub(super) fn av1_properties(&self) -> Option<&[Property<'a>]> {
        self.descriptions
            .iter()
            .find(|d| container::is_av1(&d.format))
            .map(|d| d.properties.as_slice())
    }

    /// `avifGetSampleCountOfChunk` for every chunk in turn (see [`ChunkCounts`]).
    fn chunk_sample_counts(&self) -> ChunkCounts {
        ChunkCounts::new(self)
    }

    /// `avifCodecDecodeInputFillFromSampleTable`'s checks and walk: every
    /// sample, in order, or libavif's reason for refusing the table. `visit`
    /// sees each sample and may stop the walk by returning `false` -- after
    /// which the rest of the table has still been *checked* up to the frame
    /// limit, as libavif checks it before building anything.
    pub(super) fn samples(
        &self,
        image_count_limit: u32,
        size_hint: u64,
        mut visit: impl FnMut(Sample) -> bool,
    ) -> Result<u32, Error> {
        if image_count_limit > 0 {
            let mut left = image_count_limit;
            let mut counts = self.chunk_sample_counts();
            while let Some(count) = counts.next(self) {
                if count == 0 {
                    return Err(Error::Parse("AVIF chunk of no samples"));
                }
                if count > left {
                    return Err(Error::Parse("AVIF sequence of too many frames"));
                }
                left = left.saturating_sub(count);
            }
        }
        let mut cursor = SampleCursor::new(self);
        let mut total = 0u32;
        let mut visiting = true;
        while let Some(sample) = cursor.next(self, size_hint)? {
            if visiting {
                visiting = visit(sample);
            }
            total = total.saturating_add(1);
        }
        Ok(total)
    }

    /// The samples libavif marks as sync -- decodable without any frame before
    /// them -- among the first `count`, as indices from 0, ascending: each
    /// `stss` entry that names one of them (it numbers from 1), and frame 0,
    /// which libavif takes to be one whatever `stss` says. A track without
    /// `stss` therefore has frame 0 alone, although the format would call
    /// every sample of such a track a sync sample: libavif reads it so, and
    /// seeks from frame 0, which decodes the same frames only more slowly.
    #[cfg(any(feature = "avif", test))] // the animation, and tests
    pub(super) fn sync_frames(&self, count: u32) -> Vec<u32> {
        let mut frames: Vec<u32> = self
            .sync_samples
            .iter()
            .map(|&number| number.wrapping_sub(1))
            .filter(|&frame| frame < count)
            .collect();
        if count > 0 {
            frames.push(0);
        }
        frames.sort_unstable();
        frames.dedup();
        frames
    }
}

/// The samples in each chunk in turn: `avifGetSampleCountOfChunk` for chunk 1,
/// 2, ... -- by the last `stsc` entry (in the table's order) whose first chunk
/// is at or before it. libavif searches the entries afresh for each chunk; with
/// the entries sorted once, a table of a million of each is a million steps
/// rather than a trillion, and the answers are the same.
///
/// It holds no borrow of its table, so that a cursor built on it can live
/// beside the table in one owner; every call must be given the table it was
/// made from.
#[derive(Clone, Debug)]
struct ChunkCounts {
    /// The `stsc` entries' first chunks and positions, sorted.
    order: Vec<(u32, usize)>,
    /// The next entry of `order` not yet reached.
    next_entry: usize,
    /// The latest-in-table-order entry reached so far.
    last: Option<usize>,
    /// The next chunk's index.
    chunk: usize,
}

impl ChunkCounts {
    fn new(table: &SampleTable<'_>) -> Self {
        let mut order: Vec<(u32, usize)> = table
            .sample_to_chunks
            .iter()
            .enumerate()
            .map(|(index, entry)| (entry.first_chunk, index))
            .collect();
        order.sort_unstable();
        Self {
            order,
            next_entry: 0,
            last: None,
            chunk: 0,
        }
    }

    /// The next chunk's sample count, or `None` after the last chunk.
    fn next(&mut self, table: &SampleTable<'_>) -> Option<u32> {
        if self.chunk >= table.chunks.len() {
            return None;
        }
        let number = u64::try_from(self.chunk)
            .unwrap_or(u64::MAX)
            .saturating_add(1);
        while let Some(&(first, index)) = self.order.get(self.next_entry) {
            if u64::from(first) > number {
                break;
            }
            self.last = Some(self.last.map_or(index, |last| last.max(index)));
            self.next_entry = self.next_entry.saturating_add(1);
        }
        self.chunk = self.chunk.saturating_add(1);
        Some(
            self.last
                .and_then(|index| table.sample_to_chunks.get(index))
                .map_or(0, |entry| entry.samples_per_chunk),
        )
    }
}

/// A walk through a table's samples in order, a sample at a time: the walk
/// [`SampleTable::samples`] makes, for a caller that takes each frame's bytes
/// as it comes to it -- a sequence played from its start costs one step a
/// frame, where finding frame *n* afresh costs *n*.
///
/// Like [`ChunkCounts`] it holds no borrow of its table, and must be given the
/// same table at every step.
#[derive(Clone, Debug)]
pub(super) struct SampleCursor {
    counts: ChunkCounts,
    /// Samples left in the chunk being walked.
    left: u32,
    /// Where the next sample starts.
    offset: u64,
    /// The next sample's entry in `stsz`.
    size_index: usize,
    /// How many samples have been walked.
    walked: u32,
}

impl SampleCursor {
    pub(super) fn new(table: &SampleTable<'_>) -> Self {
        Self {
            counts: ChunkCounts::new(table),
            left: 0,
            offset: 0,
            size_index: 0,
            walked: 0,
        }
    }

    /// How many samples this has walked: the index of the next.
    #[cfg(any(feature = "avif", test))] // the animation, and tests
    pub(super) const fn position(&self) -> u32 {
        self.walked
    }

    /// The next sample, or `None` after the last -- or the reason libavif
    /// refuses the table there, with the file `size_hint` bytes long (0: not
    /// known).
    pub(super) fn next(
        &mut self,
        table: &SampleTable<'_>,
        size_hint: u64,
    ) -> Result<Option<Sample>, Error> {
        while self.left == 0 {
            let chunk = self.counts.chunk;
            let Some(count) = self.counts.next(table) else {
                return Ok(None);
            };
            if count == 0 {
                return Err(Error::Parse("AVIF chunk of no samples"));
            }
            self.left = count;
            self.offset = *table
                .chunks
                .get(chunk)
                .ok_or(Error::Parse("AVIF sample table cut short"))?;
        }
        let mut size = table.all_samples_size;
        if size == 0 {
            size = *table
                .sample_sizes
                .get(self.size_index)
                .ok_or(Error::Parse("AVIF sample table cut short"))?;
        }
        let offset = self.offset;
        let end = offset
            .checked_add(u64::from(size))
            .ok_or(Error::Parse("AVIF sample offset"))?;
        if size_hint > 0 && end > size_hint {
            return Err(Error::Parse("AVIF sample past the end of the file"));
        }
        self.left = self.left.saturating_sub(1);
        self.offset = end;
        self.size_index = self.size_index.saturating_add(1);
        self.walked = self.walked.saturating_add(1);
        Ok(Some(Sample { offset, size }))
    }
}

/// `avifSampleTableGetImageDelta` -- how long a frame lasts, in the track's
/// timescale: the `stts` entry holding it, or the last entry for a frame past
/// them all, or 1 for a table without entries, with libavif's running count
/// kept in 32 bits and wrapping as it wraps.
///
/// libavif walks the entries from the first for every frame. For frames asked
/// for in increasing order this resumes where the last frame left off, and the
/// answers are the same: the first entry that holds a frame never comes before
/// the one that held the frame before it (libavif's test is `index < running
/// count`, and a larger index fails it wherever a smaller one did), wrapping
/// included.
#[cfg(any(feature = "avif", test))] // the animation, and tests
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Timeline {
    /// The `stts` entry the last frame was found in.
    entry: usize,
    /// libavif's running count through the entries before `entry`.
    before: u32,
    /// The last frame asked about, which the next must not precede.
    last: u32,
}

#[cfg(any(feature = "avif", test))] // the animation, and tests
impl Timeline {
    /// How long frame `index` lasts. A frame before the last one asked about
    /// restarts the walk from the first entry.
    pub(super) fn delta(&mut self, table: &SampleTable<'_>, index: u32) -> u32 {
        if index < self.last {
            *self = Self::default();
        }
        self.last = index;
        let entries = &table.time_to_samples;
        let count = entries.len();
        while let Some(entry) = entries.get(self.entry) {
            let through = self.before.wrapping_add(entry.sample_count);
            if index < through || self.entry.saturating_add(1) == count {
                return entry.sample_delta;
            }
            self.before = through;
            self.entry = self.entry.saturating_add(1);
        }
        1
    }
}

/// `avifParseMovieBox`: the tracks, each checked for a sensible size if it is
/// a picture track.
pub(super) fn parse_moov(payload: &[u8]) -> Result<Vec<Track<'_>>, Error> {
    let mut s = Stream::new(payload);
    let mut tracks = Vec::new();
    while s.has_bytes_left(1) {
        let (kind, size) = s.box_header().map_err(bad("AVIF moov box"))?;
        let body = s.bytes(size).map_err(bad("AVIF moov box"))?;
        if &kind == b"trak" {
            let track = parse_trak(body)?;
            if matches!(&track.handler, b"pict" | b"vide" | b"auxv") {
                if track.width == 0 || track.height == 0 {
                    return Err(Error::Parse("AVIF track of no size"));
                }
                if super::too_large(track.width, track.height) {
                    return Err(Error::Parse("AVIF track too large"));
                }
            }
            tracks.push(track);
        }
    }
    if tracks.is_empty() {
        return Err(Error::Parse("AVIF moov box with no track"));
    }
    Ok(tracks)
}

/// `avifParseTrackBox`.
fn parse_trak(payload: &[u8]) -> Result<Track<'_>, Error> {
    const WHAT: &str = "AVIF trak box";
    let mut s = Stream::new(payload);
    let mut track = Track::new();
    let (mut tkhd_seen, mut edts_seen) = (false, false);
    while s.has_bytes_left(1) {
        let (kind, size) = s.box_header().map_err(bad(WHAT))?;
        let body = s.bytes(size).map_err(bad(WHAT))?;
        match &kind {
            b"tkhd" => {
                if tkhd_seen {
                    return Err(Error::Parse("AVIF trak box with two tkhd boxes"));
                }
                parse_tkhd(&mut track, body).map_err(bad("AVIF tkhd box"))?;
                tkhd_seen = true;
            }
            b"meta" => container::parse_meta(&mut track.meta, body)?,
            b"mdia" => parse_mdia(&mut track, body)?,
            b"tref" => parse_tref(&mut track, body).map_err(bad("AVIF tref box"))?,
            b"edts" => {
                if edts_seen {
                    return Err(Error::Parse("AVIF trak box with two edts boxes"));
                }
                parse_edts(&mut track, body).map_err(bad("AVIF edts box"))?;
                edts_seen = true;
            }
            _ => {}
        }
    }
    if !tkhd_seen {
        return Err(Error::Parse("AVIF trak box without tkhd"));
    }
    track.repetition = if !edts_seen {
        Repetition::Unknown
    } else if track.is_repeating {
        if track.track_duration == u64::MAX {
            Repetition::Infinite
        } else {
            if track.track_duration == 0 {
                return Err(Error::Parse("AVIF track of duration 0"));
            }
            // `parse_elst` refuses a segment duration of 0.
            let segments = track
                .track_duration
                .checked_div(track.segment_duration)
                .ok_or(Error::Parse(WHAT))?;
            let partial = track.track_duration.checked_rem(track.segment_duration) != Some(0);
            let count = segments
                .saturating_add(u64::from(partial))
                .saturating_sub(1);
            u32::try_from(count)
                .ok()
                .filter(|&count| i32::try_from(count).is_ok())
                .map_or(Repetition::Infinite, Repetition::Count)
        }
    } else {
        Repetition::Count(0)
    };
    Ok(track)
}

/// `avifParseTrackHeaderBox`.
fn parse_tkhd(track: &mut Track<'_>, payload: &[u8]) -> Result<(), Truncated> {
    let mut s = Stream::new(payload);
    let (version, _) = s.version_and_flags()?;
    let id = match version {
        1 => {
            s.u64()?; // creation_time
            s.u64()?; // modification_time
            let id = s.u32()?;
            s.u32()?; // reserved
            track.track_duration = s.u64()?;
            id
        }
        0 => {
            s.u32()?; // creation_time
            s.u32()?; // modification_time
            let id = s.u32()?;
            s.u32()?; // reserved
            let duration = s.u32()?;
            track.track_duration = if duration == INDEFINITE_DURATION32 {
                u64::MAX
            } else {
                u64::from(duration)
            };
            id
        }
        _ => return Err(Truncated),
    };
    track.id = id;
    // reserved, layer, alternate_group, volume, reserved, matrix.
    s.skip(52)?;
    track.width = s.u32()? >> 16;
    track.height = s.u32()? >> 16;
    Ok(())
}

/// `avifParseMediaHeaderBox`.
fn parse_mdhd(track: &mut Track<'_>, payload: &[u8]) -> Result<(), Truncated> {
    let mut s = Stream::new(payload);
    let (version, _) = s.version_and_flags()?;
    match version {
        1 => {
            s.u64()?; // creation_time
            s.u64()?; // modification_time
            track.media_timescale = s.u32()?;
            track.media_duration = s.u64()?;
        }
        0 => {
            s.u32()?; // creation_time
            s.u32()?; // modification_time
            track.media_timescale = s.u32()?;
            track.media_duration = u64::from(s.u32()?);
        }
        _ => return Err(Truncated),
    }
    Ok(())
}

/// `avifParseMediaBox`.
fn parse_mdia<'a>(track: &mut Track<'a>, payload: &'a [u8]) -> Result<(), Error> {
    let mut s = Stream::new(payload);
    while s.has_bytes_left(1) {
        let (kind, size) = s.box_header().map_err(bad("AVIF mdia box"))?;
        let body = s.bytes(size).map_err(bad("AVIF mdia box"))?;
        match &kind {
            b"mdhd" => parse_mdhd(track, body).map_err(bad("AVIF mdhd box"))?,
            b"minf" => parse_minf(track, body)?,
            b"hdlr" => track.handler = container::parse_hdlr(body).map_err(bad("AVIF hdlr box"))?,
            _ => {}
        }
    }
    Ok(())
}

/// `avifParseMediaInformationBox`.
fn parse_minf<'a>(track: &mut Track<'a>, payload: &'a [u8]) -> Result<(), Error> {
    let mut s = Stream::new(payload);
    while s.has_bytes_left(1) {
        let (kind, size) = s.box_header().map_err(bad("AVIF minf box"))?;
        let body = s.bytes(size).map_err(bad("AVIF minf box"))?;
        if &kind == b"stbl" {
            if track.sample_table.is_some() {
                return Err(Error::Parse("AVIF track with two stbl boxes"));
            }
            track.sample_table = Some(parse_stbl(body)?);
        }
    }
    Ok(())
}

/// `avifParseSampleTableBox`.
fn parse_stbl(payload: &[u8]) -> Result<SampleTable<'_>, Error> {
    let mut s = Stream::new(payload);
    let mut table = SampleTable::default();
    while s.has_bytes_left(1) {
        let (kind, size) = s.box_header().map_err(bad("AVIF stbl box"))?;
        let body = s.bytes(size).map_err(bad("AVIF stbl box"))?;
        match &kind {
            b"stco" => {
                parse_chunk_offsets(&mut table, body, false).map_err(bad("AVIF stco box"))?;
            }
            b"co64" => parse_chunk_offsets(&mut table, body, true).map_err(bad("AVIF co64 box"))?,
            b"stsc" => parse_stsc(&mut table, body)?,
            b"stsz" => parse_stsz(&mut table, body).map_err(bad("AVIF stsz box"))?,
            b"stss" => parse_stss(&mut table, body).map_err(bad("AVIF stss box"))?,
            b"stts" => parse_stts(&mut table, body).map_err(bad("AVIF stts box"))?,
            b"stsd" => parse_stsd(&mut table, body)?,
            _ => {}
        }
    }
    Ok(table)
}

/// Room for `count` entries of `size` bytes in what is left of `s`: reserved
/// only once the box is seen to hold them, so a count in a header never sizes
/// a list by itself.
fn reserve<T>(list: &mut Vec<T>, s: &Stream<'_>, count: u32, size: usize) {
    let fits = usize::try_from(count)
        .ok()
        .and_then(|count| count.checked_mul(size))
        .is_some_and(|bytes| s.has_bytes_left(bytes));
    if fits {
        list.reserve(usize::try_from(count).unwrap_or(0));
    }
}

/// `avifParseChunkOffsetBox`.
fn parse_chunk_offsets(
    table: &mut SampleTable<'_>,
    payload: &[u8],
    large: bool,
) -> Result<(), Truncated> {
    let mut s = Stream::new(payload);
    s.enforce_version(0)?;
    let count = s.u32()?;
    reserve(&mut table.chunks, &s, count, if large { 8 } else { 4 });
    for _ in 0..count {
        let offset = if large { s.u64()? } else { u64::from(s.u32()?) };
        table.chunks.push(offset);
    }
    Ok(())
}

/// `avifParseSampleToChunkBox`.
fn parse_stsc(table: &mut SampleTable<'_>, payload: &[u8]) -> Result<(), Error> {
    let bad = bad("AVIF stsc box");
    let mut s = Stream::new(payload);
    s.enforce_version(0).map_err(bad)?;
    let count = s.u32().map_err(bad)?;
    reserve(&mut table.sample_to_chunks, &s, count, 12);
    let mut previous = 0u32;
    for i in 0..count {
        let first_chunk = s.u32().map_err(bad)?;
        let samples_per_chunk = s.u32().map_err(bad)?;
        s.u32().map_err(bad)?; // sample_description_index
        table.sample_to_chunks.push(SampleToChunk {
            first_chunk,
            samples_per_chunk,
        });
        // The first chunks start at 1 and strictly increase.
        let in_order = if i == 0 {
            first_chunk == 1
        } else {
            first_chunk > previous
        };
        if !in_order {
            return Err(Error::Parse("AVIF stsc chunks out of order"));
        }
        previous = first_chunk;
    }
    Ok(())
}

/// `avifParseSampleSizeBox`.
fn parse_stsz(table: &mut SampleTable<'_>, payload: &[u8]) -> Result<(), Truncated> {
    let mut s = Stream::new(payload);
    s.enforce_version(0)?;
    let all = s.u32()?;
    let count = s.u32()?;
    if all > 0 {
        table.all_samples_size = all;
    } else {
        reserve(&mut table.sample_sizes, &s, count, 4);
        for _ in 0..count {
            table.sample_sizes.push(s.u32()?);
        }
    }
    Ok(())
}

/// `avifParseSyncSampleBox`.
fn parse_stss(table: &mut SampleTable<'_>, payload: &[u8]) -> Result<(), Truncated> {
    let mut s = Stream::new(payload);
    s.enforce_version(0)?;
    let count = s.u32()?;
    reserve(&mut table.sync_samples, &s, count, 4);
    for _ in 0..count {
        table.sync_samples.push(s.u32()?);
    }
    Ok(())
}

/// `avifParseTimeToSampleBox`.
fn parse_stts(table: &mut SampleTable<'_>, payload: &[u8]) -> Result<(), Truncated> {
    let mut s = Stream::new(payload);
    s.enforce_version(0)?;
    let count = s.u32()?;
    reserve(&mut table.time_to_samples, &s, count, 8);
    for _ in 0..count {
        table.time_to_samples.push(TimeToSample {
            sample_count: s.u32()?,
            sample_delta: s.u32()?,
        });
    }
    Ok(())
}

/// `avifParseSampleDescriptionBox`.
fn parse_stsd<'a>(table: &mut SampleTable<'a>, payload: &'a [u8]) -> Result<(), Error> {
    const WHAT: &str = "AVIF stsd box";
    let bad = bad(WHAT);
    let mut s = Stream::new(payload);
    let (version, _) = s.version_and_flags().map_err(bad)?;
    // ISO/IEC 14496-12 section 8.5.2.3: version 1 is read as version 0.
    if version > 1 {
        return Err(Error::Parse(WHAT));
    }
    let count = s.u32().map_err(bad)?;
    for _ in 0..count {
        let (format, size) = s.box_header().map_err(bad)?;
        let body = s.bytes(size).map_err(bad)?;
        let mut properties = Vec::new();
        if container::is_av1(&format) {
            let children = body
                .get(VISUAL_SAMPLE_ENTRY_SIZE..)
                .ok_or(Error::Parse("AVIF sample entry too short"))?;
            container::parse_ipco(&mut properties, children, true)?;
        }
        table
            .descriptions
            .push(SampleDescription { format, properties });
    }
    Ok(())
}

/// `avifTrackReferenceBox`: the first track each `auxl` or `prem` reference
/// names. A reference box shorter than one track ID fails when libavif skips
/// "the rest" of it, which is then less than nothing.
fn parse_tref(track: &mut Track<'_>, payload: &[u8]) -> Result<(), Truncated> {
    let mut s = Stream::new(payload);
    while s.has_bytes_left(1) {
        let (kind, size) = s.box_header()?;
        match &kind {
            b"auxl" | b"prem" => {
                let id = s.u32()?;
                s.skip(size.checked_sub(4).ok_or(Truncated)?)?;
                if &kind == b"auxl" {
                    track.aux_for = id;
                } else {
                    track.prem_by = id;
                }
            }
            _ => s.skip(size)?,
        }
    }
    Ok(())
}

/// `avifParseEditBox`: exactly one `elst`.
fn parse_edts(track: &mut Track<'_>, payload: &[u8]) -> Result<(), Truncated> {
    let mut s = Stream::new(payload);
    let mut seen = false;
    while s.has_bytes_left(1) {
        let (kind, size) = s.box_header()?;
        let body = s.bytes(size)?;
        if &kind == b"elst" {
            if seen {
                return Err(Truncated);
            }
            parse_elst(track, body)?;
            seen = true;
        }
    }
    if seen { Ok(()) } else { Err(Truncated) }
}

/// `avifParseEditListBox`: whether the track repeats, and if so the one
/// segment's duration.
fn parse_elst(track: &mut Track<'_>, payload: &[u8]) -> Result<(), Truncated> {
    let mut s = Stream::new(payload);
    let (version, flags) = s.version_and_flags()?;
    if flags & 1 == 0 {
        track.is_repeating = false;
        return Ok(());
    }
    track.is_repeating = true;
    if s.u32()? != 1 {
        return Err(Truncated); // entry_count
    }
    track.segment_duration = match version {
        1 => s.u64()?,
        0 => u64::from(s.u32()?),
        _ => return Err(Truncated),
    };
    if track.segment_duration == 0 {
        return Err(Truncated);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::arithmetic_side_effects,
        clippy::cast_possible_truncation,
        reason = "tests build boxes by hand and fail loudly"
    )]

    use super::*;
    use alloc::vec;

    fn table(chunks: &[u64], stsc: &[(u32, u32)], sizes: &[u32], all: u32) -> SampleTable<'static> {
        SampleTable {
            chunks: chunks.to_vec(),
            sample_to_chunks: stsc
                .iter()
                .map(|&(first_chunk, samples_per_chunk)| SampleToChunk {
                    first_chunk,
                    samples_per_chunk,
                })
                .collect(),
            sample_sizes: sizes.to_vec(),
            all_samples_size: all,
            ..SampleTable::default()
        }
    }

    fn walk(table: &SampleTable<'_>, limit: u32, hint: u64) -> Result<Vec<Sample>, Error> {
        let mut seen = Vec::new();
        table.samples(limit, hint, |sample| {
            seen.push(sample);
            true
        })?;
        Ok(seen)
    }

    #[test]
    fn samples_are_laid_end_to_end_within_each_chunk() {
        // Two chunks: the first of two samples, the second (by the same
        // entry) also of two.
        let t = table(&[100, 500], &[(1, 2)], &[10, 20, 30, 40], 0);
        let at = |offset, size| Sample { offset, size };
        assert_eq!(
            walk(&t, 0, 0).unwrap(),
            [at(100, 10), at(110, 20), at(500, 30), at(530, 40)]
        );
        // One size for all; a later entry for chunks from 2 on.
        let t = table(&[0, 64], &[(1, 1), (2, 3)], &[], 8);
        assert_eq!(
            walk(&t, 0, 0).unwrap(),
            [at(0, 8), at(64, 8), at(72, 8), at(80, 8)]
        );
    }

    #[test]
    fn a_table_is_refused_as_libavif_refuses_it() {
        // A chunk of no samples.
        assert!(walk(&table(&[0], &[(1, 0)], &[], 1), 0, 0).is_err());
        // More sizes needed than given.
        assert!(walk(&table(&[0], &[(1, 3)], &[1, 1], 0), 0, 0).is_err());
        // Past the end of the file.
        assert!(walk(&table(&[10], &[(1, 1)], &[], 5), 0, 14).is_err());
        assert!(walk(&table(&[10], &[(1, 1)], &[], 5), 0, 15).is_ok());
        // Over the frame limit, counted before anything is walked.
        assert!(walk(&table(&[0, 1], &[(1, 2)], &[], 1), 3, 0).is_err());
        assert!(walk(&table(&[0, 1], &[(1, 2)], &[], 1), 4, 0).is_ok());
    }

    #[test]
    fn the_walk_stops_being_visited_but_is_still_checked() {
        let t = table(&[0], &[(1, 3)], &[1, 2], 0);
        let mut seen = 0;
        let result = t.samples(0, 0, |_| {
            seen += 1;
            false
        });
        assert_eq!(seen, 1);
        assert!(result.is_err());
    }

    fn boxed(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&(8 + payload.len() as u32).to_be_bytes());
        out.extend_from_slice(kind);
        out.extend_from_slice(payload);
        out
    }

    fn tkhd(id: u32, width: u32, height: u32, duration: u32) -> Vec<u8> {
        let mut body = vec![0; 4];
        body.extend_from_slice(&[0; 8]);
        body.extend_from_slice(&id.to_be_bytes());
        body.extend_from_slice(&[0; 4]);
        body.extend_from_slice(&duration.to_be_bytes());
        body.extend_from_slice(&[0; 52]);
        body.extend_from_slice(&(width << 16).to_be_bytes());
        body.extend_from_slice(&(height << 16).to_be_bytes());
        boxed(b"tkhd", &body)
    }

    #[test]
    fn a_track_needs_a_header_and_repeats_as_its_edit_list_says() {
        assert!(parse_trak(&[]).is_err());
        let header = tkhd(7, 64, 48, 300);
        let track = parse_trak(&header).unwrap();
        assert_eq!((track.id, track.width, track.height), (7, 64, 48));
        assert_eq!(track.repetition, Repetition::Unknown);
        // Repeating, 300 ticks of a 100-tick segment: two more times.
        let elst = |flags: u8, segment: u32| {
            let mut body = vec![0, 0, 0, flags, 0, 0, 0, 1];
            body.extend_from_slice(&segment.to_be_bytes());
            boxed(b"edts", &boxed(b"elst", &body))
        };
        let mut trak = tkhd(1, 8, 8, 300);
        trak.extend(elst(1, 100));
        assert_eq!(parse_trak(&trak).unwrap().repetition, Repetition::Count(2));
        let mut trak = tkhd(1, 8, 8, 301);
        trak.extend(elst(1, 100));
        assert_eq!(parse_trak(&trak).unwrap().repetition, Repetition::Count(3));
        let mut trak = tkhd(1, 8, 8, u32::MAX);
        trak.extend(elst(1, 100));
        assert_eq!(parse_trak(&trak).unwrap().repetition, Repetition::Infinite);
        let mut trak = tkhd(1, 8, 8, 300);
        trak.extend(elst(0, 100));
        assert_eq!(parse_trak(&trak).unwrap().repetition, Repetition::Count(0));
        // A segment of 0 is refused; so is a track of duration 0 that repeats.
        let mut trak = tkhd(1, 8, 8, 300);
        trak.extend(elst(1, 0));
        assert!(parse_trak(&trak).is_err());
        let mut trak = tkhd(1, 8, 8, 0);
        trak.extend(elst(1, 10));
        assert!(parse_trak(&trak).is_err());
    }

    #[test]
    fn a_track_reference_takes_the_first_id_and_needs_one() {
        let mut track = Track::new();
        let mut tref = boxed(b"auxl", &[0, 0, 0, 3, 0, 0, 0, 4]);
        tref.extend(boxed(b"prem", &[0, 0, 0, 9]));
        parse_tref(&mut track, &tref).unwrap();
        assert_eq!((track.aux_for, track.prem_by), (3, 9));
        assert!(parse_tref(&mut Track::new(), &boxed(b"auxl", &[0, 0])).is_err());
    }

    fn timed(entries: &[(u32, u32)]) -> SampleTable<'static> {
        SampleTable {
            time_to_samples: entries
                .iter()
                .map(|&(sample_count, sample_delta)| TimeToSample {
                    sample_count,
                    sample_delta,
                })
                .collect(),
            ..SampleTable::default()
        }
    }

    /// libavif's `avifSampleTableGetImageDelta`, transcribed: the reference
    /// the resuming walk is held to.
    fn libavif_delta(table: &SampleTable<'_>, index: u32) -> u32 {
        let mut max_sample_index = 0u32;
        let count = table.time_to_samples.len();
        for (i, entry) in table.time_to_samples.iter().enumerate() {
            max_sample_index = max_sample_index.wrapping_add(entry.sample_count);
            if index < max_sample_index || i == count - 1 {
                return entry.sample_delta;
            }
        }
        1
    }

    #[test]
    fn a_frame_lasts_its_stts_entry_s_delta_as_libavif_finds_it() {
        let fresh = |t: &SampleTable<'_>, i| Timeline::default().delta(t, i);
        // No entries: one tick.
        assert_eq!(fresh(&timed(&[]), 0), 1);
        // Two frames of 10, three of 20, and past them the last entry's.
        let t = timed(&[(2, 10), (3, 20)]);
        assert_eq!(
            [0, 1, 2, 4, 5, 99].map(|i| fresh(&t, i)),
            [10, 10, 20, 20, 20, 20]
        );
        // libavif's running count is 32 bits and wraps: frame 3 is not in the
        // entry whose count carries it past 2^32, but in the next.
        let t = timed(&[(1, 10), (u32::MAX, 20), (5, 30), (1, 40)]);
        assert_eq!(fresh(&t, 0), 10);
        assert_eq!(fresh(&t, 3), 30);
        assert_eq!(libavif_delta(&t, 3), 30);
    }

    #[test]
    fn resuming_the_walk_answers_as_walking_afresh_in_any_order() {
        let tables = [
            timed(&[(2, 10), (3, 20), (1, 5)]),
            timed(&[(1, 10), (u32::MAX, 20), (5, 30), (1, 40)]),
            timed(&[(0, 7), (0, 8), (4, 9)]),
            timed(&[(3, 1)]),
        ];
        let orders: [&[u32]; 3] = [
            &[0, 1, 2, 3, 4, 5, 6, 7, 8],
            &[8, 0, 4, 4, 2, 7, 1, 1, 0, 6],
            &[3, 3, 3, 5, 0, 9, 2],
        ];
        for table in &tables {
            for order in orders {
                let mut timeline = Timeline::default();
                for &index in order {
                    assert_eq!(
                        timeline.delta(table, index),
                        libavif_delta(table, index),
                        "frame {index} of {:?}",
                        table.time_to_samples
                    );
                }
            }
        }
    }

    #[test]
    fn the_sync_frames_are_stss_s_from_one_and_frame_0() {
        let table = |numbers: &[u32]| SampleTable {
            sync_samples: numbers.to_vec(),
            ..SampleTable::default()
        };
        // 3 and 1 name frames 2 and 0; 0 wraps to a frame past the end, and 9
        // is past it too, so both are passed over as libavif passes them.
        assert_eq!(table(&[3, 1, 3, 0, 9]).sync_frames(5), [0, 2]);
        // Without stss, frame 0 alone.
        assert_eq!(table(&[]).sync_frames(5), [0]);
        assert!(table(&[1]).sync_frames(0).is_empty());
    }

    #[test]
    fn a_cursor_walks_the_samples_the_whole_walk_visits() {
        let t = table(
            &[100, 500, 900],
            &[(1, 2), (3, 1)],
            &[10, 20, 30, 40, 50],
            0,
        );
        let mut cursor = SampleCursor::new(&t);
        let mut stepped = Vec::new();
        while let Some(sample) = cursor.next(&t, 0).unwrap() {
            stepped.push(sample);
            assert_eq!(cursor.position() as usize, stepped.len());
        }
        assert_eq!(stepped, walk(&t, 0, 0).unwrap());
        // Once done, it stays done.
        assert_eq!(cursor.next(&t, 0).unwrap(), None);
        // A table libavif refuses is refused where the walk reaches the fault.
        let t = table(&[0], &[(1, 3)], &[1, 2], 0);
        let mut cursor = SampleCursor::new(&t);
        assert!(cursor.next(&t, 0).unwrap().is_some());
        assert!(cursor.next(&t, 0).unwrap().is_some());
        assert!(cursor.next(&t, 0).is_err());
    }

    #[test]
    fn stsc_entries_must_start_at_one_and_increase() {
        let stsc = |entries: &[(u32, u32)]| {
            let mut body = vec![0; 4];
            body.extend_from_slice(&(entries.len() as u32).to_be_bytes());
            for &(first, per) in entries {
                body.extend_from_slice(&first.to_be_bytes());
                body.extend_from_slice(&per.to_be_bytes());
                body.extend_from_slice(&1u32.to_be_bytes());
            }
            body
        };
        let mut t = SampleTable::default();
        assert!(parse_stsc(&mut t, &stsc(&[(1, 1), (3, 2)])).is_ok());
        assert!(parse_stsc(&mut SampleTable::default(), &stsc(&[(2, 1)])).is_err());
        assert!(parse_stsc(&mut SampleTable::default(), &stsc(&[(1, 1), (1, 2)])).is_err());
        // A second box appends, and starts its own order again.
        assert!(parse_stsc(&mut t, &stsc(&[(1, 5)])).is_ok());
        assert_eq!(t.sample_to_chunks.len(), 3);
    }
}
