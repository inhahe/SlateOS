//! Reading a file: the header and the segment's description when opened,
//! then the Clusters' blocks as packets, and seeking.
//!
//! The order of work follows FFmpeg's (`matroska_read_header`,
//! `matroska_read_packet`): the Segment's top-level elements are read up to
//! the first Cluster; anything the SeekHead points to that was not met on
//! the way (an Info or Tracks written after the Clusters) is read from where
//! it says; the Cues are read only when a seek needs them. Then Clusters are
//! read one element at a time, and a block's frames become packets.

use std::collections::VecDeque;
use std::io::{Read, Seek};

use crate::block::{self, Block};
use crate::cues::{self, Cue};
use crate::ebml::{Header, Id, MAX_BINARY, Reader, Size};
use crate::track::{Track, TrackKind, read_track};
use crate::{Error, ids};

/// The segment's description: `Info`.
#[derive(Clone, Debug, PartialEq)]
pub struct SegmentInfo {
    /// Nanoseconds in a tick, the unit of every timestamp: 1,000,000 (a
    /// millisecond) unless the file says otherwise, and 0 is read as that.
    pub timestamp_scale: u64,
    /// The duration, in ticks, if the file gives it.
    pub duration: Option<f64>,
}

/// One frame of one track.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Packet {
    /// The track's number ([`Track::number`]).
    pub track: u64,
    /// When the frame is presented, in the track's ticks
    /// ([`Demuxer::time_base`]): the Cluster's timestamp plus the block's,
    /// less the codec delay -- for a laced frame, the block's plus the
    /// frames' before it. `None` where FFmpeg has no timestamp: before the
    /// file's first tick, or after a laced frame of unknown duration.
    pub timestamp: Option<i64>,
    /// How long it lasts, in the track's ticks: the block's duration, or
    /// the track's default, shared among a block's frames; 0 if unknown.
    pub duration: u64,
    /// Whether decoding can start here.
    pub keyframe: bool,
    /// The frame, with the track's encoding undone.
    pub data: Vec<u8>,
    /// The block's additions, in order: `(BlockAddID, bytes)`, the empty
    /// ones left out. WebM's alpha channel for VP8 and VP9 is ID 1, a second
    /// stream of the same codec.
    pub additions: Vec<(u64, Vec<u8>)>,
    /// Sound to discard, in nanoseconds: from the end of the frame if
    /// positive, its start if negative (Opus's end trimming).
    pub discard_padding: i64,
    /// Where the block's bytes begin in the file.
    pub position: u64,
}

/// Where reading is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    /// Between the Segment's top-level elements.
    Segment,
    /// Inside a Cluster.
    Cluster {
        /// Where it begins.
        start: u64,
        /// Where it ends; `None` for a Cluster of unknown size, which ends
        /// where a top-level element begins.
        end: Option<u64>,
        /// Its timestamp, in ticks: 0 until its `Timestamp` element, as
        /// FFmpeg defaults it.
        timestamp: u64,
    },
    /// Past the end.
    Done,
}

/// What a `BlockGroup` adds to its `Block`.
struct Group {
    duration: u64,
    references: usize,
    discard_padding: i64,
    additions: Vec<(u64, Vec<u8>)>,
}

/// A track as the reading needs it: where it is in [`Demuxer::tracks`], and
/// its codec delay and time base worked out once.
#[derive(Clone, Copy, Debug)]
struct Timing {
    index: usize,
    /// The track's tick: `num / den` seconds, reduced.
    num: u64,
    den: u64,
    /// The codec delay, in the track's ticks.
    delay: u64,
}

/// After a seek, what is dropped, at FFmpeg's two levels: the demuxer drops
/// every packet before the key frame's time until a key frame comes
/// (`skip_to_keyframe`), and the generic layer above it drops the seek's
/// own track's packets until that track's key frame.
#[derive(Clone, Copy, Debug, Default)]
struct Skip {
    /// Drop non-subtitle packets before this time, until a key frame.
    until: Option<i64>,
    /// Drop this track's packets until its key frame.
    key_for: Option<u64>,
}

/// A Matroska or WebM file being read.
pub struct Demuxer<R> {
    r: Reader<R>,
    doc_type: Option<Vec<u8>>,
    info: SegmentInfo,
    tracks: Vec<Track>,
    /// Every `TrackEntry`'s number, the first of each, with its track's
    /// timing if FFmpeg reads the track: a block naming a number not here
    /// is damage, one naming an ignored track is ignored.
    declared: Vec<(u64, Option<Timing>)>,
    /// Where the Segment's data begins: SeekHead and Cues positions count
    /// from here.
    segment_data: u64,
    /// Where the Segment ends: the file's end for one of unknown size.
    segment_end: u64,
    /// Where the first Cluster begins, if there is one.
    first_cluster: Option<u64>,
    /// Where the Cues are, if the file says, and them once read.
    cues_at: Option<u64>,
    cues: Option<Vec<Cue>>,
    state: State,
    /// The frames of the last block not yet given out.
    queue: VecDeque<Packet>,
    /// The start of the last element read whole: resynchronising after
    /// damage starts one byte past it, as FFmpeg's does.
    last_good: u64,
    skip: Skip,
    /// Per subtitle track, where its last frame ends: FFmpeg marks a
    /// subtitle starting before that not a key frame.
    subtitle_ends: Vec<(u64, i64)>,
    /// While indexing a file without Cues: the key frames found, in place
    /// of packets.
    indexing: Option<Vec<Cue>>,
}

/// The most SeekHead entries followed, chained SeekHeads included.
const MAX_SEEK_ENTRIES: usize = 4096;

impl<R: Read + Seek> Demuxer<R> {
    /// Read `source`'s header, its segment's description and its tracks.
    ///
    /// # Errors
    ///
    /// When the source is not an EBML file, asks for a version of EBML this
    /// does not read, has no Segment, or its description is damaged.
    pub fn open(source: R) -> Result<Self, Error> {
        let mut r = Reader::new(source)?;
        let doc_type = read_ebml_header(&mut r)?;

        // The Segment: anything else at the top level is passed over.
        let segment = loop {
            let h = r.header()?.ok_or(Error::Invalid("no Segment"))?;
            if h.id == ids::SEGMENT {
                break h;
            }
            let end = h
                .end()
                .ok_or(Error::Invalid("a top-level element of unknown size"))?;
            r.seek_to(end)?;
        };
        let segment_end = segment.end().map_or(r.len(), |e| e.min(r.len()));

        let mut d = Self {
            r,
            doc_type,
            info: SegmentInfo {
                timestamp_scale: 1_000_000,
                duration: None,
            },
            tracks: Vec::new(),
            declared: Vec::new(),
            segment_data: segment.data,
            segment_end,
            first_cluster: None,
            cues_at: None,
            cues: None,
            state: State::Done,
            queue: VecDeque::new(),
            last_good: segment.start,
            skip: Skip::default(),
            subtitle_ends: Vec::new(),
            indexing: None,
        };
        d.read_description()?;
        if let Some(at) = d.first_cluster {
            d.r.seek_to(at)?;
            d.state = State::Segment;
        }
        Ok(d)
    }

    /// The EBML header's `DocType`: `webm` or `matroska`, or anything else
    /// a writer put (read regardless, as FFmpeg does).
    pub fn doc_type(&self) -> Option<&[u8]> {
        self.doc_type.as_deref()
    }

    pub fn info(&self) -> &SegmentInfo {
        &self.info
    }

    /// The tracks FFmpeg reads, in the file's order.
    pub fn tracks(&self) -> &[Track] {
        &self.tracks
    }

    /// A track's tick, `(num, den)`: `num / den` seconds, reduced. The
    /// segment's `TimestampScale` in nanoseconds, times the track's
    /// deprecated `TrackTimestampScale`.
    pub fn time_base(&self, track: u64) -> Option<(u64, u64)> {
        self.timing(track).map(|t| (t.num, t.den))
    }

    fn timing(&self, track: u64) -> Option<Timing> {
        self.declared
            .iter()
            .find(|(n, _)| *n == track)
            .and_then(|(_, t)| *t)
    }

    /// The Segment's top-level elements up to the first Cluster, then what
    /// the SeekHead points to that was not met.
    fn read_description(&mut self) -> Result<(), Error> {
        let mut seek_entries: Vec<(Id, u64)> = Vec::new();
        let (mut info_read, mut tracks_read) = (false, false);
        self.r.seek_to(self.segment_data)?;
        while self.r.pos() < self.segment_end {
            let Some(h) = self.r.header()? else { break };
            if h.id == ids::CLUSTER {
                self.first_cluster = Some(h.start);
                break;
            }
            self.read_top_level(&h, &mut seek_entries, &mut info_read, &mut tracks_read)?;
            let end = h
                .end()
                .ok_or(Error::Invalid("a top-level element of unknown size"))?;
            self.r.seek_to(end.min(self.segment_end))?;
        }
        // What the SeekHead points to, chained SeekHeads included; Cues are
        // left until a seek wants them.
        let mut i = 0;
        while let Some(&(id, pos)) = seek_entries.get(i) {
            i = i.saturating_add(1);
            if i > MAX_SEEK_ENTRIES {
                break;
            }
            let wanted = match id {
                ids::INFO => !info_read,
                ids::TRACKS => !tracks_read,
                ids::SEEK_HEAD => true,
                ids::CUES => {
                    self.cues_at.get_or_insert(pos);
                    false
                }
                _ => false,
            };
            if !wanted || pos >= self.segment_end {
                continue;
            }
            // A damaged pointer is passed over, as FFmpeg stops following
            // its SeekHead but keeps reading.
            let seen_before = seek_entries.len();
            let read = self.r.seek_to(pos).and_then(|()| {
                let h = self.r.header()?.ok_or(Error::Truncated)?;
                if h.id != id {
                    return Ok(());
                }
                // A SeekHead pointing at itself, or one already read, adds
                // nothing new; the cap above bounds a loop of them.
                self.read_top_level(&h, &mut seek_entries, &mut info_read, &mut tracks_read)
            });
            if read.is_err() {
                seek_entries.truncate(seen_before);
                break;
            }
        }
        if self.info.timestamp_scale == 0 {
            self.info.timestamp_scale = 1_000_000;
        }
        self.time_tracks();
        Ok(())
    }

    /// Read one of the Segment's top-level elements, if it is one this
    /// reads; the caller moves past it.
    fn read_top_level(
        &mut self,
        h: &Header,
        seek_entries: &mut Vec<(Id, u64)>,
        info_read: &mut bool,
        tracks_read: &mut bool,
    ) -> Result<(), Error> {
        match h.id {
            ids::SEEK_HEAD => {
                let base = self.segment_data;
                self.r.children(h, |r, seek| {
                    if seek.id != ids::SEEK {
                        return Ok(());
                    }
                    let (mut id, mut pos) = (None, None);
                    r.children(seek, |r, f| {
                        match f.id {
                            ids::SEEK_ID => {
                                let bytes = r.binary(f.size, 4)?;
                                id = Some(bytes.iter().fold(0u32, |a, &b| (a << 8) | u32::from(b)));
                            }
                            ids::SEEK_POSITION => pos = Some(r.uint(f.size, 0)?),
                            _ => {}
                        }
                        Ok(())
                    })?;
                    if let (Some(id), Some(pos)) = (id, pos)
                        && seek_entries.len() < MAX_SEEK_ENTRIES
                        && let Some(at) = pos.checked_add(base)
                    {
                        seek_entries.push((id, at));
                    }
                    Ok(())
                })?;
            }
            ids::INFO if !*info_read => {
                *info_read = true;
                let info = &mut self.info;
                self.r.children(h, |r, c| {
                    match c.id {
                        ids::TIMESTAMP_SCALE => info.timestamp_scale = r.uint(c.size, 1_000_000)?,
                        ids::DURATION => info.duration = Some(r.float(c.size, 0.0)?),
                        // Read as FFmpeg reads them, and kept by nothing:
                        // nothing here shows a file's title or who wrote it
                        // (design-decisions §856).
                        ids::TITLE | ids::MUXING_APP | ids::WRITING_APP => {
                            r.string(c.size)?;
                        }
                        _ => {}
                    }
                    Ok(())
                })?;
                // FFmpeg reads a duration that is not a number as none.
                if self.info.duration.is_some_and(f64::is_nan) {
                    self.info.duration = None;
                }
            }
            ids::TRACKS if !*tracks_read => {
                *tracks_read = true;
                let (mut tracks, mut declared) = (Vec::new(), Vec::new());
                self.r.children(h, |r, entry| {
                    if entry.id != ids::TRACK_ENTRY {
                        return Ok(());
                    }
                    let (number, track) = read_track(r, entry)?;
                    declared.push((number, track.is_some()));
                    if let Some(t) = track {
                        tracks.push(t);
                    }
                    Ok(())
                })?;
                self.tracks = tracks;
                // Timing comes once the TimestampScale is certain.
                self.declared = declared
                    .into_iter()
                    .map(|(n, read)| (n, read.then_some(placeholder_timing())))
                    .collect();
            }
            ids::CUES => {
                self.cues_at.get_or_insert(h.start);
            }
            _ => {}
        }
        Ok(())
    }

    /// Each read track's time base and codec delay in its ticks, as
    /// FFmpeg's `avpriv_set_pts_info` and `av_rescale_q` make them.
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "the tick is at most 2^32 ns, so the 128-bit products are far from overflow and the divisor at least 10^9; the gcd is at least 1; `next` counts tracks"
    )]
    fn time_tracks(&mut self) {
        let scale = self.info.timestamp_scale;
        let mut next = 0usize;
        let mut first_of: Vec<u64> = Vec::new();
        for (number, timing) in &mut self.declared {
            let Some(_) = timing else { continue };
            let Some(track) = self.tracks.get(next) else {
                *timing = None;
                continue;
            };
            // C's conversion of the double to the unsigned int FFmpeg
            // takes: truncation, held in range here.
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                clippy::cast_precision_loss,
                reason = "C's conversion of a double to unsigned int, clamped to its range"
            )]
            let ticks_ns =
                ((scale as f64) * track.time_scale).clamp(1.0, f64::from(u32::MAX)) as u64;
            let g = gcd(ticks_ns, 1_000_000_000);
            let (num, den) = (ticks_ns / g, 1_000_000_000 / g);
            // av_rescale_q(delay, 1/1e9, num/den), rounding to nearest.
            let c = u128::from(num) * 1_000_000_000;
            let delay =
                u64::try_from((u128::from(track.codec_delay) * u128::from(den) + c / 2) / c)
                    .unwrap_or(u64::MAX);
            *timing = Some(Timing {
                index: next,
                num,
                den,
                delay,
            });
            // Only the first entry of a number counts (FFmpeg's
            // `matroska_find_track_by_num`).
            if first_of.contains(number) {
                *timing = None;
            }
            first_of.push(*number);
            next += 1;
        }
        let mut seen = Vec::new();
        self.declared.retain(|(n, _)| {
            let first = !seen.contains(n);
            seen.push(*n);
            first
        });
    }

    /// The next packet, in file order; `None` at the end.
    ///
    /// A damaged element ends its Cluster: reading goes on from the next
    /// top-level element found after it, as FFmpeg resynchronises.
    ///
    /// # Errors
    ///
    /// Only when the source fails; damage is read past.
    pub fn next_packet(&mut self) -> Result<Option<Packet>, Error> {
        loop {
            if let Some(p) = self.queue.pop_front() {
                return Ok(Some(p));
            }
            match self.step() {
                Ok(true) => {}
                Ok(false) => return Ok(None),
                Err(Error::Io(kind)) => return Err(Error::Io(kind)),
                Err(_) => {
                    if !self.resync()? {
                        return Ok(None);
                    }
                }
            }
        }
    }

    /// Read one element: `false` at the end.
    fn step(&mut self) -> Result<bool, Error> {
        match self.state {
            State::Done => Ok(false),
            State::Segment => {
                if self.r.pos() >= self.segment_end {
                    self.state = State::Done;
                    return Ok(false);
                }
                let Some(h) = self.r.header()? else {
                    self.state = State::Done;
                    return Ok(false);
                };
                if h.id == ids::CLUSTER {
                    let end = h.end();
                    if end.is_some_and(|e| e > self.segment_end) {
                        return Err(Error::Invalid("a Cluster running past its Segment"));
                    }
                    self.last_good = h.start;
                    self.state = State::Cluster {
                        start: h.start,
                        end,
                        timestamp: 0,
                    };
                    return Ok(true);
                }
                if h.id == ids::CUES {
                    self.cues_at.get_or_insert(h.start);
                }
                let end = h
                    .end()
                    .ok_or(Error::Invalid("a top-level element of unknown size"))?;
                self.last_good = h.start;
                self.r.seek_to(end.min(self.segment_end))?;
                Ok(true)
            }
            State::Cluster {
                start,
                end,
                timestamp,
            } => {
                if end.is_some_and(|e| self.r.pos() >= e) {
                    self.state = State::Segment;
                    return Ok(true);
                }
                let Some(h) = self.r.header()? else {
                    self.state = State::Done;
                    return Ok(false);
                };
                if end.is_none()
                    && (ids::is_top_level(h.id) || h.id == ids::SEGMENT || h.id == ids::EBML)
                {
                    // A Cluster of unknown size ends where something that
                    // cannot be inside it begins.
                    self.r.seek_to(h.start)?;
                    self.state = State::Segment;
                    return Ok(true);
                }
                let child_end = h
                    .end()
                    .ok_or(Error::Invalid("an element of unknown size in a Cluster"))?;
                if end.is_some_and(|e| child_end > e) || child_end > self.r.len() {
                    return Err(Error::Invalid("an element running past its Cluster"));
                }
                self.last_good = h.start;
                match h.id {
                    ids::TIMESTAMP => {
                        let t = self.r.uint(h.size, 0)?;
                        self.state = State::Cluster {
                            start,
                            end,
                            timestamp: t,
                        };
                    }
                    ids::SIMPLE_BLOCK if self.indexing.is_some() => {
                        let head = self
                            .r
                            .binary(Size::Known(head_len(&h)), block::HEADER_MAX)?;
                        if let Ok((track, relative, flags)) = block::header(&head) {
                            self.index_key(track, relative, flags & 0x80 != 0, timestamp, start);
                        }
                    }
                    ids::SIMPLE_BLOCK => {
                        let data = self.r.binary(h.size, MAX_BINARY)?;
                        if !data.is_empty() {
                            self.block(&data, h.data, timestamp, None)?;
                        }
                    }
                    ids::BLOCK_GROUP if self.indexing.is_some() => {
                        let (head, references) = self.read_group_head(&h)?;
                        if let Ok((track, relative, _)) = block::header(&head) {
                            self.index_key(track, relative, references == 0, timestamp, start);
                        }
                    }
                    ids::BLOCK_GROUP => {
                        let (data, pos, group) = self.read_group(&h)?;
                        if !data.is_empty() {
                            self.block(&data, pos, timestamp, Some(group))?;
                        }
                    }
                    _ => {}
                }
                self.r.seek_to(child_end)?;
                Ok(true)
            }
        }
    }

    /// A `BlockGroup`: its `Block`'s bytes and where they are, and the rest.
    fn read_group(&mut self, h: &Header) -> Result<(Vec<u8>, u64, Group), Error> {
        let mut data = Vec::new();
        let mut pos = h.data;
        let mut g = Group {
            duration: 0,
            references: 0,
            discard_padding: 0,
            additions: Vec::new(),
        };
        self.r.children(h, |r, c| {
            match c.id {
                ids::BLOCK => {
                    pos = c.data;
                    data = r.binary(c.size, MAX_BINARY)?;
                }
                ids::BLOCK_DURATION => g.duration = r.uint(c.size, 0)?,
                ids::REFERENCE_BLOCK => {
                    // The reference itself does not matter, only that there
                    // is one; its value is read to check it.
                    let _ = r.sint(c.size, 0)?;
                    g.references = g.references.saturating_add(1);
                }
                ids::DISCARD_PADDING => g.discard_padding = r.sint(c.size, 0)?,
                ids::BLOCK_ADDITIONS => r.children(c, |r, more| {
                    if more.id != ids::BLOCK_MORE {
                        return Ok(());
                    }
                    let (mut id, mut bytes) = (1, Vec::new());
                    r.children(more, |r, f| {
                        match f.id {
                            ids::BLOCK_ADD_ID => id = r.uint(f.size, 1)?,
                            ids::BLOCK_ADDITIONAL => bytes = r.binary(f.size, MAX_BINARY)?,
                            _ => {}
                        }
                        Ok(())
                    })?;
                    g.additions.push((id, bytes));
                    Ok(())
                })?,
                _ => {}
            }
            Ok(())
        })?;
        Ok((data, pos, g))
    }

    /// A `BlockGroup`'s `Block` header and how many references it has, for
    /// indexing: the frames are not read.
    fn read_group_head(&mut self, h: &Header) -> Result<(Vec<u8>, usize), Error> {
        let (mut head, mut references) = (Vec::new(), 0usize);
        self.r.children(h, |r, c| {
            match c.id {
                ids::BLOCK => head = r.binary(Size::Known(head_len(c)), block::HEADER_MAX)?,
                ids::REFERENCE_BLOCK => references = references.saturating_add(1),
                _ => {}
            }
            Ok(())
        })?;
        Ok((head, references))
    }

    /// While indexing: note a key frame of a read track.
    fn index_key(&mut self, track: u64, relative: i16, key: bool, cluster_time: u64, cluster: u64) {
        if !key {
            return;
        }
        let Some(timing) = self.timing(track) else {
            return;
        };
        let Some(t) = self.tracks.get(timing.index) else {
            return;
        };
        if let Some(time) = block_timestamp(t, timing, cluster_time, relative)
            && let Some(index) = self.indexing.as_mut()
        {
            index.push(Cue {
                time,
                track,
                cluster,
            });
        }
    }

    /// A block's frames, as packets queued in order: FFmpeg's
    /// `matroska_parse_block`.
    fn block(
        &mut self,
        data: &[u8],
        position: u64,
        cluster_time: u64,
        group: Option<Group>,
    ) -> Result<(), Error> {
        let b: Block = block::parse(data)?;
        let Some(&(_, timing)) = self.declared.iter().find(|(n, _)| *n == b.track) else {
            return Err(Error::Invalid("a block for a track that is not declared"));
        };
        let Some(timing) = timing else {
            // A track FFmpeg ignores: so are its blocks.
            return Ok(());
        };
        let Some(track) = self.tracks.get(timing.index) else {
            return Ok(());
        };
        if !track.readable() {
            return Ok(());
        }
        let (mut keyframe, block_duration, discard_padding, additions) = match group {
            None => (b.flags & 0x80 != 0, 0, 0, Vec::new()),
            Some(g) => (
                g.references == 0,
                g.duration,
                g.discard_padding,
                g.additions,
            ),
        };
        let mut timestamp = block_timestamp(track, timing, cluster_time, b.relative);
        if let Some(t) = timestamp
            && track.kind == TrackKind::Subtitle
        {
            let ended = self
                .subtitle_ends
                .iter()
                .find(|(n, _)| *n == b.track)
                .map_or(0, |(_, e)| *e);
            if t < ended {
                keyframe = false;
            }
        }

        // After a seek: the demuxer's dropping, then the seek's track's.
        if let Some(until) = self.skip.until
            && track.kind != TrackKind::Subtitle
        {
            if timestamp.is_none_or(|t| t < until) {
                return Ok(());
            }
            // A key frame ends it -- and so does another track's frame that
            // is not one, which FFmpeg reports as "keyframes not correctly
            // marked" and gives out.
            if keyframe || self.skip.key_for != Some(b.track) {
                self.skip.until = None;
            }
        }
        if self.skip.key_for == Some(b.track) {
            if !keyframe {
                return Ok(());
            }
            self.skip.key_for = None;
        }

        let laces = b.frames.len();
        let laces_u64 = u64::try_from(laces).unwrap_or(1).max(1);
        let mut duration = block_duration;
        if duration == 0
            && let Some(d) = track.default_duration
        {
            // FFmpeg: the default duration (ns) times the frames, over the
            // segment's TimestampScale.
            duration = d
                .wrapping_mul(laces_u64)
                .checked_div(self.info.timestamp_scale)
                .unwrap_or(0);
        }
        if track.kind == TrackKind::Subtitle
            && let Some(t) = timestamp
        {
            let end = t.saturating_add(i64::try_from(duration).unwrap_or(i64::MAX));
            match self.subtitle_ends.iter_mut().find(|(n, _)| *n == b.track) {
                Some((_, e)) => *e = (*e).max(end),
                None => self.subtitle_ends.push((b.track, end)),
            }
        }

        let duration = duration.min(i64::MAX.unsigned_abs());
        for (n, range) in b.frames.iter().enumerate() {
            let n_u64 = u64::try_from(n).unwrap_or(0);
            // Each frame's share of the block's duration, rounded as FFmpeg
            // shares it.
            let lace_duration = mul_div(duration, n_u64.saturating_add(1), laces_u64)
                .wrapping_sub(mul_div(duration, n_u64, laces_u64));
            let stored = data.get(range.clone()).unwrap_or_default();
            let frame = track.encoding.undo(stored)?.into_owned();
            let frame_additions: Vec<(u64, Vec<u8>)> = additions
                .iter()
                .filter(|(_, bytes)| !bytes.is_empty())
                .cloned()
                .collect();
            // An empty frame with no additions is no packet.
            if !(frame.is_empty() && additions.is_empty()) {
                self.queue.push_back(Packet {
                    track: b.track,
                    timestamp,
                    duration: lace_duration,
                    // Every frame of a key block is a key frame, as FFmpeg has
                    // marked them since 7.1 (which marked only the first).
                    keyframe,
                    data: frame,
                    additions: frame_additions,
                    discard_padding,
                    position,
                });
            }
            timestamp = match timestamp {
                Some(t) if lace_duration != 0 => Some(t.wrapping_add(lace_duration.cast_signed())),
                _ => None,
            };
        }
        Ok(())
    }

    /// After damage: look past it for the next top-level element and go
    /// on from there; `false` if none is left. FFmpeg's `matroska_resync`.
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "positions are within the file (`at` stays under its length, and an ID is found only after four bytes of the scan, so its start is at or past where the scan began); `filled` counts bytes read"
    )]
    fn resync(&mut self) -> Result<bool, Error> {
        self.queue.clear();
        let mut at = self.last_good.saturating_add(1);
        let len = self.r.len();
        let mut window = 0u32;
        let mut filled = 0;
        let mut buf = vec![0u8; 64 * 1024];
        while at < len {
            self.r.seek_to(at)?;
            let n = usize::try_from((len - at).min(64 * 1024)).unwrap_or(64 * 1024);
            let chunk = buf.get_mut(..n).unwrap_or_default();
            self.r.read_into(chunk)?;
            for (i, &b) in chunk.iter().enumerate() {
                window = (window << 8) | u32::from(b);
                filled += 1;
                if filled >= 4 && ids::is_top_level(window) {
                    let start = at + u64::try_from(i).unwrap_or(0) - 3;
                    self.r.seek_to(start)?;
                    self.last_good = start;
                    // As FFmpeg, a segment resynchronised into is read to
                    // the end of the file, its stated end aside.
                    self.segment_end = len;
                    self.state = State::Segment;
                    return Ok(true);
                }
            }
            at += u64::try_from(n).unwrap_or(1);
        }
        self.state = State::Done;
        Ok(false)
    }

    /// Go to the latest key frame of `track` at or before `timestamp` (in
    /// the track's ticks), so that the next packet is the first to decode
    /// from: through the Cues if the file has them, by walking the Clusters'
    /// timestamps if not.
    ///
    /// Packets before the key frame's time are then dropped, of every track
    /// but subtitles, and the track's own until its key frame -- as after
    /// FFmpeg's seek.
    ///
    /// # Errors
    ///
    /// When `track` is not one of [`Self::tracks`], or the source fails.
    pub fn seek(&mut self, track: u64, timestamp: i64) -> Result<(), Error> {
        if self.timing(track).is_none() {
            return Err(Error::Invalid("a seek in a track that is not read"));
        }
        let index = self.index()?;
        let entries: Vec<&Cue> = index.iter().filter(|c| c.track == track).collect();
        let target = match entries.first() {
            Some(first) => timestamp.max(first.time),
            None => timestamp,
        };
        let entry = entries
            .iter()
            .rev()
            .find(|c| c.time <= target)
            .or(entries.first())
            .copied()
            .copied();
        let (pos, time) = match entry {
            Some(c) => (c.cluster, c.time),
            None => match self.first_cluster {
                Some(first) => (first, i64::MIN),
                None => {
                    self.state = State::Done;
                    return Ok(());
                }
            },
        };
        self.queue.clear();
        self.r.seek_to(pos)?;
        self.last_good = pos;
        self.state = State::Segment;
        self.subtitle_ends.clear();
        self.skip = Skip {
            until: Some(time),
            key_for: Some(track),
        };
        Ok(())
    }

    /// The index to seek with: the Cues if the file has a usable set, or
    /// else the key frames found by reading the blocks' headers.
    fn index(&mut self) -> Result<Vec<Cue>, Error> {
        if self.cues.is_none() {
            let cues = match self.cues_at {
                Some(at) => self.read_cues(at).unwrap_or_default(),
                None => Vec::new(),
            };
            self.cues = Some(cues);
        }
        let cues = self.cues.clone().unwrap_or_default();
        if !cues.is_empty() {
            return Ok(cues);
        }
        self.scan_keyframes()
    }

    fn read_cues(&mut self, at: u64) -> Result<Vec<Cue>, Error> {
        self.r.seek_to(at)?;
        let h = self.r.header()?.ok_or(Error::Truncated)?;
        if h.id != ids::CUES {
            return Ok(Vec::new());
        }
        cues::read(
            &mut self.r,
            &h,
            self.segment_data,
            self.info.timestamp_scale,
        )
    }

    /// Without Cues: every read track's key frames, from the blocks' headers
    /// alone -- the latest at or before a time is where FFmpeg's seek lands
    /// in such a file. Reading resumes where it was.
    fn scan_keyframes(&mut self) -> Result<Vec<Cue>, Error> {
        let Some(first) = self.first_cluster else {
            return Ok(Vec::new());
        };
        let saved = (self.state, self.r.pos(), self.last_good, self.segment_end);
        self.indexing = Some(Vec::new());
        self.state = State::Segment;
        let mut result = self.r.seek_to(first);
        while result.is_ok() {
            match self.step() {
                Ok(true) => {}
                Ok(false) => break,
                Err(Error::Io(kind)) => result = Err(Error::Io(kind)),
                Err(_) => match self.resync() {
                    Ok(true) => {}
                    Ok(false) => break,
                    Err(e) => result = Err(e),
                },
            }
        }
        let mut index = self.indexing.take().unwrap_or_default();
        (self.state, _, self.last_good, self.segment_end) = saved;
        self.r.seek_to(saved.1)?;
        result?;
        index.sort_by_key(|c| c.time);
        Ok(index)
    }
}

/// A block's timestamp in its track's ticks, as FFmpeg's unsigned C
/// arithmetic computes it: the Cluster's (divided by the track's deprecated
/// scale), plus the block's, less the codec delay; or none, for a Cluster
/// timestamp of all ones or a block before the file's first tick.
fn block_timestamp(track: &Track, timing: Timing, cluster_time: u64, relative: i16) -> Option<i64> {
    let block_time = i64::from(relative);
    if cluster_time == u64::MAX || (block_time < 0 && cluster_time < block_time.unsigned_abs()) {
        return None;
    }
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss,
        reason = "FFmpeg's (double) cluster_time / track->time_scale, converted to uint64_t"
    )]
    let in_track = (cluster_time as f64 / track.time_scale) as u64;
    Some(
        in_track
            .wrapping_add(block_time.cast_unsigned())
            .wrapping_sub(timing.delay)
            .cast_signed(),
    )
}

/// How much of a block element to read for its header.
fn head_len(h: &Header) -> u64 {
    match h.size {
        Size::Known(n) => n.min(block::HEADER_MAX),
        Size::Unknown => 0,
    }
}

/// The FFmpeg-shaped stand-in for a read track's timing until the
/// TimestampScale is known.
const fn placeholder_timing() -> Timing {
    Timing {
        index: 0,
        num: 1,
        den: 1,
        delay: 0,
    }
}

/// `a * b / c` in 128 bits, as C's 64-bit `block_duration*(n+1) / laces`
/// computes it for the values it meets (`c` is at least 1).
#[allow(
    clippy::arithmetic_side_effects,
    reason = "two u64s multiply within a u128, and the divisor is at least 1"
)]
fn mul_div(a: u64, b: u64, c: u64) -> u64 {
    u64::try_from(u128::from(a) * u128::from(b) / u128::from(c.max(1))).unwrap_or(u64::MAX)
}

#[allow(
    clippy::arithmetic_side_effects,
    reason = "the remainder is taken only while the divisor is nonzero"
)]
const fn gcd(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        let t = a % b;
        a = b;
        b = t;
    }
    if a == 0 { 1 } else { a }
}

/// The EBML header: its checks, as FFmpeg makes them, and the `DocType`.
fn read_ebml_header<R: Read + Seek>(r: &mut Reader<R>) -> Result<Option<Vec<u8>>, Error> {
    let h = r.header()?.ok_or(Error::Truncated)?;
    if h.id != ids::EBML {
        return Err(Error::Invalid("not an EBML file"));
    }
    let (mut read_version, mut max_id, mut max_size, mut doc_read_version) = (1, 4, 8, 1);
    let mut doc_type = None;
    r.children(&h, |r, c| {
        match c.id {
            ids::EBML_READ_VERSION => read_version = r.uint(c.size, 1)?,
            ids::EBML_MAX_ID_LENGTH => max_id = r.uint(c.size, 4)?,
            ids::EBML_MAX_SIZE_LENGTH => max_size = r.uint(c.size, 8)?,
            ids::DOC_TYPE => doc_type = r.string(c.size)?,
            ids::DOC_TYPE_READ_VERSION => doc_read_version = r.uint(c.size, 1)?,
            _ => {}
        }
        Ok(())
    })?;
    if read_version > 1 || max_size > 8 || max_id > 4 || doc_read_version > 3 {
        return Err(Error::Unsupported(
            "an EBML or Matroska version this does not read",
        ));
    }
    r.seek_to(h.end().ok_or(Error::Truncated)?)?;
    Ok(doc_type)
}
