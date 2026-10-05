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
use crate::metadata::{Attachment, Chapter, Lists, Metadata, describe};
use crate::nest::{self, SeekEntry};
use crate::track::{Track, TrackKind, read_track};
use crate::{Error, ids};

/// The segment's description: `Info`.
///
/// A file with several Info elements before its first Cluster has each read
/// in turn, as FFmpeg reads them: each starts the timestamp scale and the
/// duration afresh, and the strings and the date keep their last value.
#[derive(Clone, Debug, PartialEq)]
pub struct SegmentInfo {
    /// Nanoseconds in a tick, the unit of every timestamp: 1,000,000 (a
    /// millisecond) unless the file says otherwise, and 0 is read as that.
    pub timestamp_scale: u64,
    /// The duration, in ticks, if the file gives it.
    pub duration: Option<f64>,
    /// The file's title (`Title`), as written (UTF-8 by the specification,
    /// not checked), where it gives one -- the last, if it gives several, as
    /// FFmpeg keeps the last; empty for an empty one.
    pub title: Option<Vec<u8>>,
    /// The program that wrote the file (`MuxingApp`), as written: FFmpeg's
    /// `encoder`.
    pub muxing_app: Option<Vec<u8>>,
    /// When the file was written (`DateUTC`), in nanoseconds from
    /// 2001-01-01 UTC, where its last `DateUTC` is the eight bytes it should
    /// be: FFmpeg's `creation_time`.
    pub date_utc: Option<i64>,
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

/// Where reading is, whole: what a walk puts aside, and restores when it is
/// done.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Place {
    state: State,
    pos: u64,
    last_good: u64,
    segment_end: u64,
}

/// A walk through the Clusters for their key frames: how a track without
/// Cues is sought in.
///
/// FFmpeg's seek in such a file reads on from the key frames it already
/// knows until it meets one of the seek's track past the time sought
/// (`matroska_read_seek`'s loop, or `seek_frame_generic`), then goes to the
/// latest at or before it. So does this. The walk begins at the first
/// Cluster with the first seek that needs it, goes each time only as far as
/// that seek needs, and is left where it stopped for the next: a seek near
/// the start of a long file without Cues -- a live recording's, a browser's
/// `MediaRecorder`'s -- reads its first Clusters, not all of them, and a seek
/// back reads nothing.
#[derive(Clone, Debug)]
struct Walk {
    /// Every read track's key frames found so far, in file order.
    found: Vec<Cue>,
    /// Per subtitle track, where its last frame walked ends: the walk reads
    /// the file afresh, so it keeps these apart from playing's.
    subtitle_ends: Vec<(u64, i64)>,
    /// Where it goes on from; `None` once it has reached the end.
    next: Option<Place>,
    /// Where it begins: the first Cluster, as the file was opened.
    start: Option<Place>,
}

/// A Matroska or WebM file being read.
pub struct Demuxer<R> {
    r: Reader<R>,
    doc_type: Option<Vec<u8>>,
    info: SegmentInfo,
    tracks: Vec<Track>,
    /// The file's metadata, its chapters and its attachments, as FFmpeg
    /// makes them (`metadata.rs`).
    metadata: Metadata,
    chapters: Vec<Chapter>,
    attachments: Vec<Attachment>,
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
    /// Where the Cues are, as FFmpeg finds them: every Cues element read
    /// before the first Cluster (or where a SeekHead entry naming something
    /// else points), all of them one index; else the one the SeekHead names,
    /// unless following it failed. And the index once read.
    cues_at: Vec<u64>,
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
    /// While walking for key frames: those found, in place of packets.
    indexing: Option<Vec<Cue>>,
    walk: Walk,
}

/// FFmpeg's count of master elements open around a top-level element read
/// before the first Cluster -- the Segment -- and around one read where a
/// SeekHead points, where FFmpeg opens a stand-in level of its own too.
/// FFmpeg nests masters 16 deep at most, so the second has a level less to
/// nest tags in.
const IN_SEGMENT: u32 = 1;
const FROM_SEEK_HEAD: u32 = 2;

/// FFmpeg's record of the Segment's top-level elements (`level1_elems`):
/// where each was found and whether it has been read. One per ID, but one
/// per position for SeekHeads and Tags, of which a file may have several; at
/// most 64, past which a SeekHead's pointers to new ones are not followed.
#[derive(Default)]
struct Level1 {
    entries: Vec<Level1Entry>,
}

struct Level1Entry {
    id: Id,
    pos: u64,
    parsed: bool,
}

impl Level1 {
    /// FFmpeg's `matroska_find_level1_elem`: the record of `id` (at `pos`,
    /// for a SeekHead or Tags), made if there is none and room for one; none
    /// for a Cluster, or an ID whose marker bit is not where its length puts
    /// it.
    fn find(&mut self, id: Id, pos: u64) -> Option<usize> {
        let valid = matches!(id.checked_ilog2(), Some(7 | 14 | 21 | 28));
        if !valid || id == ids::CLUSTER {
            return None;
        }
        let by_position = id == ids::SEEK_HEAD || id == ids::TAGS;
        if let Some(i) = self
            .entries
            .iter()
            .position(|e| e.id == id && (e.pos == pos || !by_position))
        {
            return Some(i);
        }
        if self.entries.len() >= 64 {
            return None;
        }
        self.entries.push(Level1Entry {
            id,
            pos: 0,
            parsed: false,
        });
        Some(self.entries.len().saturating_sub(1))
    }

    /// A top-level element about to be read where it stands: its record
    /// marked read, before it is -- as FFmpeg marks it.
    fn met(&mut self, id: Id, pos: u64) {
        if let Some(i) = self.find(id, pos)
            && let Some(e) = self.entries.get_mut(i)
        {
            if e.pos == 0 {
                e.pos = pos;
            }
            e.parsed = true;
        }
    }

    /// Where the Cues are, if a SeekHead pointed at them and they have not
    /// been read: the last place it pointed.
    fn cues_unread(&self) -> Option<u64> {
        self.entries
            .iter()
            .find(|e| e.id == ids::CUES && !e.parsed)
            .map(|e| e.pos)
    }
}

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
                title: None,
                muxing_app: None,
                date_utc: None,
            },
            tracks: Vec::new(),
            metadata: Metadata::default(),
            chapters: Vec::new(),
            attachments: Vec::new(),
            declared: Vec::new(),
            segment_data: segment.data,
            segment_end,
            first_cluster: None,
            cues_at: Vec::new(),
            cues: None,
            state: State::Done,
            queue: VecDeque::new(),
            last_good: segment.start,
            skip: Skip::default(),
            subtitle_ends: Vec::new(),
            indexing: None,
            walk: Walk {
                found: Vec::new(),
                subtitle_ends: Vec::new(),
                next: None,
                start: None,
            },
        };
        d.read_description(segment.end())?;
        if let Some(at) = d.first_cluster {
            d.r.seek_to(at)?;
            d.state = State::Segment;
            let start = Place {
                state: State::Segment,
                pos: at,
                last_good: at,
                segment_end: d.segment_end,
            };
            d.walk.start = Some(start);
            d.walk.next = Some(start);
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

    /// The file's metadata, as FFmpeg gives it: `title`, `encoder` and
    /// `creation_time` from its Info, then its tags that name no track,
    /// chapter or attachment.
    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    /// The file's chapters, as FFmpeg makes them: the first edition's and
    /// every other's top-level chapters, each starting after the last
    /// (see [`crate::metadata`]'s rules).
    pub fn chapters(&self) -> &[Chapter] {
        &self.chapters
    }

    /// Each chapter's end, in nanoseconds: its own where the file gives one;
    /// where not, as FFmpeg fills it in -- the next chapter's start, or the
    /// end of the file (its Info's duration after `start`, the
    /// presentation's first timestamp in nanoseconds, which FFmpeg takes
    /// from the first packets: 0 in nearly every file, and taken as 0 when
    /// `None`), or with neither, the chapter's own start.
    ///
    /// FFmpeg estimates a duration from the bit rate where the Info gives
    /// none; this does not, and ends such a file's last chapter where it
    /// starts.
    pub fn chapter_ends(&self, start: Option<i64>) -> Vec<i64> {
        crate::metadata::chapter_ends(
            &self.chapters,
            self.duration_in_micros(),
            start.map(nanos_to_micros),
        )
    }

    /// FFmpeg's duration of the file in microseconds
    /// (`AVFormatContext.duration`): the Info's duration in ticks times the
    /// tick, in C's doubles, truncated; none if the Info gives none, or 0.
    fn duration_in_micros(&self) -> Option<i64> {
        let ticks = self.info.duration?;
        #[allow(
            clippy::float_cmp,
            reason = "FFmpeg's test is for exactly 0, a duration it takes for none"
        )]
        let none = ticks == 0.0;
        if none {
            return None;
        }
        #[allow(
            clippy::cast_precision_loss,
            reason = "C's conversion of the uint64_t scale to double"
        )]
        let micros = ticks * self.info.timestamp_scale as f64 * 1000.0 / 1_000_000.0;
        // C's conversion to int64_t: a value out of its range is the
        // processor's INT64_MIN.
        let limit = 2f64.powi(63);
        #[allow(
            clippy::cast_possible_truncation,
            reason = "the value is checked in range first"
        )]
        let micros = if micros.is_nan() || micros < -limit || micros >= limit {
            i64::MIN
        } else {
            micros as i64
        };
        Some(micros)
    }

    /// The file's attachments FFmpeg keeps: those with a name, a media type
    /// and data, in the file's order, up to FFmpeg's 1000 streams with the
    /// tracks.
    pub fn attachments(&self) -> &[Attachment] {
        &self.attachments
    }

    /// The bytes of attachment `index` (in [`Self::attachments`]), read from
    /// the file now. Reading packets goes on where it was.
    ///
    /// # Errors
    ///
    /// When there is no such attachment, or the source fails.
    pub fn attachment_data(&mut self, index: usize) -> Result<Vec<u8>, Error> {
        let a = self
            .attachments
            .get(index)
            .ok_or(Error::Invalid("an attachment past the last"))?;
        let (at, size) = (a.position, a.size);
        let reading = self.r.pos();
        let data = self
            .r
            .seek_to(at)
            .and_then(|()| self.r.binary(Size::Known(size), MAX_BINARY));
        self.r.seek_to(reading)?;
        data
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
    /// the SeekHead points to that was not met -- by FFmpeg's rules
    /// (`matroska_read_header`, `matroska_execute_seekhead`):
    ///
    /// - Before the first Cluster, every top-level element met is read, a
    ///   second Info or Tracks too (FFmpeg's "Duplicate element" is a
    ///   warning, not a refusal): a second Info starts its numbers afresh, a
    ///   second Tracks adds its tracks, and so do further Chapters, Tags and
    ///   Attachments.
    /// - Then each SeekHead entry in turn, chained SeekHeads' too: one is
    ///   followed if FFmpeg's record of that element ([`Level1`]) is not
    ///   yet read -- an Info, Tracks, Chapters or Attachments read anywhere
    ///   already is not read again, a SeekHead or Tags at a new position is
    ///   -- and whatever top-level element stands where it points is read,
    ///   be it the one named or not. The Cues are left for the first seek,
    ///   from where the last entry naming them points.
    /// - A damaged SeekHead, Chapters, Tags or Attachments keeps what was
    ///   read of it. Met before the first Cluster, reading goes on from the
    ///   next top-level element found after its ID (where FFmpeg comes to on
    ///   its second pass; FFmpeg first goes back to the start of the
    ///   Segment and reads it all again, which repeats every track before
    ///   the damage, and that is not followed here). Met through a SeekHead,
    ///   the SeekHead is followed no further, and its Cues are not used --
    ///   FFmpeg marks its index broken.
    /// - A damaged Info or Tracks refuses the file, as it always has here.
    fn read_description(&mut self, segment_ends: Option<u64>) -> Result<(), Error> {
        // Where the Segment says it ends, which bounds its top-level elements
        // until a resynchronisation: after one, FFmpeg takes the Segment for
        // one of unknown size, read to the end of the file.
        let mut segment_ends = segment_ends;
        let mut lists = Lists::default();
        let mut level1 = Level1::default();
        // Cues read where they stand, which FFmpeg reads at once.
        let mut cues_read: Vec<u64> = Vec::new();
        self.r.seek_to(self.segment_data)?;
        while self.r.pos() < self.segment_end {
            let Some(h) = self.r.header()? else { break };
            if h.id == ids::CLUSTER {
                self.first_cluster = Some(h.start);
                break;
            }
            let end = h
                .end()
                .ok_or(Error::Invalid("a top-level element of unknown size"))?;
            let mut next = end.min(self.segment_end);
            if ids::is_top_level(h.id) {
                level1.met(h.id, h.start);
                match h.id {
                    ids::INFO => self.read_info(&h)?,
                    ids::TRACKS => self.read_tracks(&h)?,
                    ids::CUES => cues_read.push(h.start),
                    _ => {
                        let read = if segment_ends.is_some_and(|e| end > e) {
                            Err(Error::Invalid("an element running past its Segment"))
                        } else {
                            self.read_listed(&h, IN_SEGMENT, &mut lists)
                        };
                        match read {
                            Ok(()) => {}
                            Err(Error::Io(kind)) => return Err(Error::Io(kind)),
                            Err(_) => {
                                // Past its four-byte ID, and one more.
                                match self.find_top_level(h.start.saturating_add(5))? {
                                    Some(at) => {
                                        next = at;
                                        // As FFmpeg, a Segment resynchronised
                                        // in is read to the end of the file,
                                        // its stated end no longer a bound.
                                        self.segment_end = self.r.len();
                                        segment_ends = None;
                                    }
                                    None => break,
                                }
                            }
                        }
                    }
                }
            }
            self.r.seek_to(next)?;
        }

        let mut cues_broken = false;
        let mut i = 0;
        while let Some(SeekEntry { id, pos }) = lists.seek.get(i).copied() {
            i = i.saturating_add(1);
            // FFmpeg's checks: an ID of up to four bytes, and a position that
            // stays within int64_t when the Segment's start is added.
            let Ok(id) = Id::try_from(id) else { continue };
            let Some(pos) = pos
                .checked_add(self.segment_data)
                .filter(|&p| i64::try_from(p).is_ok())
            else {
                continue;
            };
            let Some(k) = level1.find(id, pos) else {
                continue;
            };
            let Some(entry) = level1.entries.get_mut(k) else {
                continue;
            };
            if entry.parsed {
                continue;
            }
            entry.pos = pos;
            if id == ids::CUES {
                continue;
            }
            match self.read_at(pos, &mut level1, &mut lists, &mut cues_read) {
                Ok(()) => {}
                Err(Error::Io(kind)) => return Err(Error::Io(kind)),
                Err(_) => {
                    cues_broken = true;
                    break;
                }
            }
            if let Some(entry) = level1.entries.get_mut(k) {
                entry.parsed = true;
            }
        }
        self.cues_at = if !cues_read.is_empty() {
            cues_read
        } else if cues_broken {
            Vec::new()
        } else {
            level1.cues_unread().into_iter().collect()
        };
        if self.info.timestamp_scale == 0 {
            self.info.timestamp_scale = 1_000_000;
        }
        self.time_tracks();
        let described = describe(&self.info, &mut self.tracks, &lists);
        self.metadata = described.metadata;
        self.chapters = described.chapters;
        self.attachments = described.attachments;
        Ok(())
    }

    /// What FFmpeg reads where a SeekHead entry points: the top-level
    /// element there, whichever it is -- a Cluster, or an element it does
    /// not know, read as nothing.
    fn read_at(
        &mut self,
        pos: u64,
        level1: &mut Level1,
        lists: &mut Lists,
        cues_read: &mut Vec<u64>,
    ) -> Result<(), Error> {
        self.r.seek_to(pos)?;
        let id = nest::read_id(&mut self.r)?;
        if id == ids::CLUSTER {
            // FFmpeg stops at a Cluster's ID, its size unread.
            return Ok(());
        }
        let size = nest::read_size(&mut self.r)?;
        let h = Header {
            id,
            size,
            start: pos,
            data: self.r.pos(),
        };
        if !ids::is_top_level(id) {
            return nest::skip_any(&mut self.r, &h);
        }
        level1.met(id, pos);
        if h.end().is_none() {
            // FFmpeg tries to read one of unknown size, which the
            // specification forbids; this refuses it.
            return Err(Error::Invalid("a top-level element of unknown size"));
        }
        match id {
            ids::INFO => self.read_info(&h),
            ids::TRACKS => self.read_tracks(&h),
            ids::CUES => {
                // Read at once by FFmpeg: these are its index then.
                cues_read.push(pos);
                Ok(())
            }
            _ => self.read_listed(&h, FROM_SEEK_HEAD, lists),
        }
    }

    /// A SeekHead, Chapters, Tags or Attachments element into `lists`.
    fn read_listed(&mut self, h: &Header, levels: u32, lists: &mut Lists) -> Result<(), Error> {
        match h.id {
            ids::SEEK_HEAD => nest::read_seek_head(&mut self.r, h, levels, &mut lists.seek),
            ids::CHAPTERS => nest::read_chapters(&mut self.r, h, levels, &mut lists.chapters),
            ids::TAGS => nest::read_tags(&mut self.r, h, levels, &mut lists.tags),
            ids::ATTACHMENTS => {
                nest::read_attachments(&mut self.r, h, levels, &mut lists.attachments)
            }
            _ => Ok(()),
        }
    }

    /// An Info element, as FFmpeg reads each one it meets: the timestamp
    /// scale and duration start afresh; the strings and the date keep their
    /// last value.
    fn read_info(&mut self, h: &Header) -> Result<(), Error> {
        let info = &mut self.info;
        info.timestamp_scale = 1_000_000;
        info.duration = None;
        self.r.children(h, |r, c| {
            match c.id {
                ids::TIMESTAMP_SCALE => info.timestamp_scale = r.uint(c.size, 1_000_000)?,
                ids::DURATION => info.duration = Some(r.float(c.size, 0.0)?),
                // FFmpeg reads an empty string as one, not as none.
                ids::TITLE => info.title = Some(r.string(c.size)?.unwrap_or_default()),
                ids::MUXING_APP => info.muxing_app = Some(r.string(c.size)?.unwrap_or_default()),
                ids::DATE_UTC => {
                    let bytes = r.binary(c.size, MAX_BINARY)?;
                    info.date_utc = <[u8; 8]>::try_from(bytes.as_slice())
                        .ok()
                        .map(i64::from_be_bytes);
                }
                _ => {}
            }
            Ok(())
        })?;
        // FFmpeg reads a duration that is not a number as none.
        if self.info.duration.is_some_and(f64::is_nan) {
            self.info.duration = None;
        }
        Ok(())
    }

    /// A Tracks element: its tracks added to those of any before it, as
    /// FFmpeg adds them.
    fn read_tracks(&mut self, h: &Header) -> Result<(), Error> {
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
        self.tracks.extend(tracks);
        // Timing comes once the TimestampScale is certain.
        self.declared.extend(
            declared
                .into_iter()
                .map(|(n, read)| (n, read.then_some(placeholder_timing()))),
        );
        Ok(())
    }

    /// Where the next of the Segment's top-level elements begins, at or
    /// after `from`, found by its ID as FFmpeg's `matroska_resync` finds one.
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "positions are within the file (`at` stays under its length, and an ID is found only after four bytes of the scan, so its start is at or past where the scan began)"
    )]
    fn find_top_level(&mut self, from: u64) -> Result<Option<u64>, Error> {
        let mut at = from;
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
                    return Ok(Some(at + u64::try_from(i).unwrap_or(0) - 3));
                }
            }
            at += u64::try_from(n).unwrap_or(1);
        }
        Ok(None)
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
        for (_, timing) in &mut self.declared {
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
            next += 1;
        }
        // Only the first entry of a number counts, read or ignored: a block
        // goes to the track FFmpeg's `matroska_find_track_by_num` finds.
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
    ///
    /// Each element FFmpeg knows where it stands -- one of the Segment's own,
    /// a Cluster's, a BlockGroup's, or EBML's Void and CRC-32 anywhere --
    /// is noted as the last good one when it begins (`last_good`), so that
    /// a resync starts where FFmpeg's does: one byte past it.
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
                // Anything else is passed over -- Cues too: FFmpeg reads Cues
                // only before the first Cluster or where the SeekHead points.
                let end = h
                    .end()
                    .ok_or(Error::Invalid("a top-level element of unknown size"))?;
                if counts(Level::Segment, &h) {
                    self.last_good = h.start;
                }
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
                if counts(Level::Cluster, &h) {
                    self.last_good = h.start;
                }
                match h.id {
                    ids::TIMESTAMP => {
                        let t = self.r.uint(h.size, 0)?;
                        self.state = State::Cluster {
                            start,
                            end,
                            timestamp: t,
                        };
                    }
                    ids::SIMPLE_BLOCK => {
                        let data = self.r.binary(h.size, MAX_BINARY)?;
                        if !data.is_empty() {
                            self.block(&data, h.data, timestamp, start, None)?;
                        }
                    }
                    ids::BLOCK_GROUP => {
                        let (data, pos, group) = self.read_group(&h)?;
                        if !data.is_empty() {
                            self.block(&data, pos, timestamp, start, Some(group))?;
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
    /// Each element inside it FFmpeg knows is the last good one in turn.
    fn read_group(&mut self, h: &Header) -> Result<(Vec<u8>, u64, Group), Error> {
        let mut data = Vec::new();
        let mut pos = h.data;
        let mut g = Group {
            duration: 0,
            references: 0,
            discard_padding: 0,
            additions: Vec::new(),
        };
        let last_good = &mut self.last_good;
        self.r.children(h, |r, c| {
            if counts(Level::Group, c) {
                *last_good = c.start;
            }
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
                    if counts(Level::Additions, more) {
                        *last_good = more.start;
                    }
                    if more.id != ids::BLOCK_MORE {
                        return Ok(());
                    }
                    let (mut id, mut bytes) = (1, Vec::new());
                    r.children(more, |r, f| {
                        if counts(Level::More, f) {
                            *last_good = f.start;
                        }
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

    /// A block's frames, as packets queued in order -- or, while walking for
    /// key frames, its key frame noted and its frames only checked. FFmpeg's
    /// `matroska_parse_block` does both, in this order: the track, the key
    /// frame noted, then the laces, then each frame's encoding undone --
    /// either of the last two can fail after the key frame is noted.
    fn block(
        &mut self,
        data: &[u8],
        position: u64,
        cluster_time: u64,
        cluster: u64,
        group: Option<Group>,
    ) -> Result<(), Error> {
        let (number, relative, flags) = block::header(data)?;
        let Some(&(_, timing)) = self.declared.iter().find(|(n, _)| *n == number) else {
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
            None => (flags & 0x80 != 0, 0, 0, Vec::new()),
            Some(g) => (
                g.references == 0,
                g.duration,
                g.discard_padding,
                g.additions,
            ),
        };
        let mut timestamp = block_timestamp(track, timing, cluster_time, relative);
        // A walk reads the file afresh, so it keeps subtitles' ends of its own.
        let walking = self.indexing.is_some();
        let subtitle_ends = if walking {
            &mut self.walk.subtitle_ends
        } else {
            &mut self.subtitle_ends
        };
        if let Some(t) = timestamp
            && track.kind == TrackKind::Subtitle
        {
            let ended = subtitle_ends
                .iter()
                .find(|(n, _)| *n == number)
                .map_or(0, |(_, e)| *e);
            if t < ended {
                keyframe = false;
            }
        }

        if let Some(found) = self.indexing.as_mut() {
            if keyframe && let Some(time) = timestamp {
                found.push(Cue {
                    time,
                    track: number,
                    cluster,
                });
            }
        } else {
            // After a seek: the demuxer's dropping, then the seek's track's.
            if let Some(until) = self.skip.until
                && track.kind != TrackKind::Subtitle
            {
                if timestamp.is_none_or(|t| t < until) {
                    return Ok(());
                }
                // A key frame ends it -- and so does another track's frame
                // that is not one, which FFmpeg reports as "keyframes not
                // correctly marked" and gives out.
                if keyframe || self.skip.key_for != Some(number) {
                    self.skip.until = None;
                }
            }
            if self.skip.key_for == Some(number) {
                if !keyframe {
                    return Ok(());
                }
                self.skip.key_for = None;
            }
        }

        let b: Block = block::parse(data)?;
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
            match subtitle_ends.iter_mut().find(|(n, _)| *n == number) {
                Some((_, e)) => *e = (*e).max(end),
                None => subtitle_ends.push((number, end)),
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
            // Undone while walking too, as FFmpeg's walk does: a frame that
            // does not inflate ends the Cluster there as well.
            let frame = track.encoding.undo(stored)?.into_owned();
            if walking {
                continue;
            }
            let frame_additions: Vec<(u64, Vec<u8>)> = additions
                .iter()
                .filter(|(_, bytes)| !bytes.is_empty())
                .cloned()
                .collect();
            // An empty frame with no additions is no packet.
            if !(frame.is_empty() && additions.is_empty()) {
                self.queue.push_back(Packet {
                    track: number,
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
    ///
    /// The frames a damaged block gave before its damage stay queued, as
    /// FFmpeg delivers the laces it queued before one failed.
    fn resync(&mut self) -> Result<bool, Error> {
        match self.find_top_level(self.last_good.saturating_add(1))? {
            Some(start) => {
                self.r.seek_to(start)?;
                self.last_good = start;
                // As FFmpeg, a segment resynchronised into is read to the
                // end of the file, its stated end aside.
                self.segment_end = self.r.len();
                self.state = State::Segment;
                Ok(true)
            }
            None => {
                self.state = State::Done;
                Ok(false)
            }
        }
    }

    /// Go to the latest key frame of `track` at or before `timestamp` (in
    /// the track's ticks), so that the next packet is the first to decode
    /// from: through the Cues if the file has them for the track, by walking
    /// the Clusters for its key frames if not -- only as far as the first
    /// key frame past the time ([`Walk`]).
    ///
    /// Packets before the key frame's time are then dropped, of every track
    /// but subtitles, and the track's own until its key frame -- as after
    /// FFmpeg's seek.
    ///
    /// The Cues are read alone, as a freshly opened FFmpeg reads them: its
    /// index also holds every key frame it has read since, so that once it
    /// has read past a key frame the Cues leave out, it may seek to that one
    /// instead (design-decisions §1348). And the seek is the demuxer's: a
    /// sound decoder that must decode [`crate::Track::seek_pre_roll`] before
    /// its output is right is given it by a caller who seeks that much
    /// earlier, as FFmpeg's callers do.
    ///
    /// # Errors
    ///
    /// When `track` is not one of [`Self::tracks`], has no key frame to go
    /// to (FFmpeg's seek fails too), or the source fails. Reading then goes
    /// on where it was.
    pub fn seek(&mut self, track: u64, timestamp: i64) -> Result<(), Error> {
        if self.timing(track).is_none() {
            return Err(Error::Invalid("a seek in a track that is not read"));
        }
        let mut entries: Vec<Cue> = self
            .cues()?
            .iter()
            .filter(|c| c.track == track)
            .copied()
            .collect();
        if entries.is_empty() {
            // No Cues for the track -- none at all, too few for FFmpeg to
            // use, or only other tracks' -- so its key frames, walked to.
            self.walk_until(track, timestamp)?;
            entries = self
                .walk
                .found
                .iter()
                .filter(|c| c.track == track)
                .copied()
                .collect();
            // FFmpeg's index is in time order, and of two key frames at one
            // time it keeps the later: a stable sort keeps the file's order
            // among equal times, and the search below takes the last.
            entries.sort_by_key(|c| c.time);
        }
        let target = match entries.first() {
            Some(first) => timestamp.max(first.time),
            None => timestamp,
        };
        let Some(entry) = entries
            .iter()
            .rev()
            .find(|c| c.time <= target)
            .or(entries.first())
            .copied()
        else {
            if self.first_cluster.is_none() {
                // Nothing to play, so nothing to go to.
                self.state = State::Done;
                return Ok(());
            }
            return Err(Error::Invalid("a seek in a track with no key frame"));
        };
        self.queue.clear();
        self.r.seek_to(entry.cluster)?;
        self.last_good = entry.cluster;
        self.state = State::Segment;
        self.subtitle_ends.clear();
        self.skip = Skip {
            until: Some(entry.time),
            key_for: Some(track),
        };
        Ok(())
    }

    /// The file's Cues, read the first time a seek wants them; empty if it
    /// has none FFmpeg would use. Reading stays where it was.
    fn cues(&mut self) -> Result<&[Cue], Error> {
        if self.cues.is_none() {
            let reading = self.r.pos();
            let mut points = Vec::new();
            for at in self.cues_at.clone() {
                // Damage keeps the points read before it, as FFmpeg keeps
                // them: what the error leaves in `points` is the index, so
                // the error itself is passed over.
                let _ = self.read_cue_points(at, &mut points);
            }
            let index = cues::index(points, self.segment_data, self.info.timestamp_scale);
            self.cues = Some(index);
            self.r.seek_to(reading)?;
        }
        Ok(self.cues.as_deref().unwrap_or_default())
    }

    /// The cue points of the Cues element at `at`, into `points`: none if
    /// another element stands there, as FFmpeg reads none from it.
    fn read_cue_points(&mut self, at: u64, points: &mut Vec<cues::Point>) -> Result<(), Error> {
        self.r.seek_to(at)?;
        let id = nest::read_id(&mut self.r)?;
        if id != ids::CUES {
            return Ok(());
        }
        let size = nest::read_size(&mut self.r)?;
        let h = Header {
            id,
            size,
            start: at,
            data: self.r.pos(),
        };
        cues::read_points(&mut self.r, &h, FROM_SEEK_HEAD, points)
    }

    fn place(&self) -> Place {
        Place {
            state: self.state,
            pos: self.r.pos(),
            last_good: self.last_good,
            segment_end: self.segment_end,
        }
    }

    fn go_to(&mut self, p: Place) -> Result<(), Error> {
        self.state = p.state;
        self.last_good = p.last_good;
        self.segment_end = p.segment_end;
        self.r.seek_to(p.pos)
    }

    /// Walk on through the Clusters, noting every read track's key frames
    /// from the blocks' headers alone, until one of `track` later than
    /// `time` is found or the file ends ([`Walk`]). Reading then goes on
    /// where it was.
    ///
    /// FFmpeg's test is the same, on the frames it reads: a key frame of
    /// the seek's track past the time ends its walk.
    fn walk_until(&mut self, track: u64, time: i64) -> Result<(), Error> {
        let past = |c: &Cue| c.track == track && c.time > time;
        if self.walk.found.iter().any(past) {
            return Ok(());
        }
        let Some(from) = self.walk.next else {
            return Ok(());
        };
        let reading = self.place();
        self.indexing = Some(core::mem::take(&mut self.walk.found));
        let mut result = self.go_to(from);
        let mut ended = false;
        while result.is_ok() {
            let found = self.indexing.as_ref().and_then(|f| f.last());
            if found.is_some_and(past) {
                break;
            }
            match self.step() {
                Ok(true) => {}
                Ok(false) => {
                    ended = true;
                    break;
                }
                Err(Error::Io(kind)) => result = Err(Error::Io(kind)),
                Err(_) => match self.resync() {
                    Ok(true) => {}
                    Ok(false) => {
                        ended = true;
                        break;
                    }
                    Err(e) => result = Err(e),
                },
            }
        }
        self.walk.found = self.indexing.take().unwrap_or_default();
        self.walk.next = match result {
            // The source failed partway: where the walk stood is not to be
            // trusted, so the next seek walks again from the start.
            Err(_) => {
                self.walk.found.clear();
                self.walk.subtitle_ends.clear();
                self.walk.start
            }
            Ok(()) if ended => None,
            Ok(()) => Some(self.place()),
        };
        self.go_to(reading)?;
        result
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

/// Where an element stands, for [`counts`].
#[derive(Clone, Copy, Debug)]
enum Level {
    /// A child of the Segment.
    Segment,
    /// A child of a Cluster.
    Cluster,
    /// A child of a `BlockGroup`.
    Group,
    /// A child of a `BlockAdditions`.
    Additions,
    /// A child of a `BlockMore`.
    More,
}

/// Whether FFmpeg counts `h` good where it stands -- an element its syntax
/// tables know there, or EBML's Void or CRC-32 anywhere, of a size its type
/// allows (`ebml_parse`'s `update_pos` and `max_lengths`) -- so that a resync
/// after damage starts one byte past it, as FFmpeg's does. An element it does
/// not know it passes over without counting: damage after one resyncs from
/// before it.
fn counts(level: Level, h: &Header) -> bool {
    let Size::Known(size) = h.size else {
        return false;
    };
    // FFmpeg's limits by type: 8 bytes for a number, 256 MiB for bytes, none
    // for a master element or one it does not read.
    let limit = match (level, h.id) {
        (_, ids::VOID | ids::CRC_32) => u64::MAX,
        (Level::Segment, id) if ids::is_top_level(id) => u64::MAX,
        (Level::Cluster, ids::TIMESTAMP) => 8,
        (Level::Cluster, ids::SIMPLE_BLOCK) => MAX_BINARY,
        (Level::Cluster, ids::BLOCK_GROUP | ids::CLUSTER_POSITION | ids::CLUSTER_PREV_SIZE) => {
            u64::MAX
        }
        (Level::Group, ids::BLOCK) => MAX_BINARY,
        (Level::Group, ids::BLOCK_DURATION | ids::DISCARD_PADDING | ids::REFERENCE_BLOCK) => 8,
        (Level::Group, ids::BLOCK_ADDITIONS | ids::CODEC_STATE) => u64::MAX,
        (Level::Additions, ids::BLOCK_MORE) => u64::MAX,
        (Level::More, ids::BLOCK_ADD_ID) => 8,
        (Level::More, ids::BLOCK_ADDITIONAL) => MAX_BINARY,
        _ => return false,
    };
    size <= limit
}

/// Nanoseconds as FFmpeg's microseconds: `av_rescale_q`, a half rounded
/// away from zero.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "an i64 and 500 add within an i128, and the quotient is a thousandth of an i64"
)]
fn nanos_to_micros(ns: i64) -> i64 {
    let n = i128::from(ns);
    let micros = if n >= 0 {
        (n + 500) / 1000
    } else {
        -((500 - n) / 1000)
    };
    i64::try_from(micros).unwrap_or(i64::MIN)
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
