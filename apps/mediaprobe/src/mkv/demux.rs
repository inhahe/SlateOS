//! Matroska's and WebM's frames, in order: what a player reads packets
//! from.
//!
//! A Segment holds `Cluster`s, each a timestamp and blocks; a block is a
//! track number, a timestamp relative to its cluster's, flags, and one frame
//! or several "laced" together. [`Demuxer`] walks the clusters as they are
//! asked for, a block at a time, and gives back each frame as a [`Packet`]
//! with its track, its time in nanoseconds and whether it is a key frame.
//! Nothing is read ahead but the block in hand.
//!
//! What is read, from the Matroska specification (RFC 9559):
//!
//! - `SimpleBlock`, and `BlockGroup`'s `Block` -- a key frame when it has no
//!   `ReferenceBlock` -- with its `BlockDuration`;
//! - all three lacings: Xiph's, fixed-size and EBML's, the frames after the
//!   first timed by the track's `DefaultDuration` when it has one;
//! - `Cues`, through the `SeekHead`, for [`Demuxer::seek`]; a file without
//!   them is sought by walking its clusters' timestamps;
//! - a cluster of unknown length -- a live recording's -- ends where the
//!   next top-level element begins;
//! - a track's `ContentEncoding` of header stripping, the bytes it strips
//!   put back; a track compressed or encrypted otherwise is listed and its
//!   frames passed over.
//!
//! Damage is stepped over, not obeyed: a block that does not parse is
//! dropped, an element that runs past its cluster ends the cluster, and
//! bytes that are no element are searched for the next cluster's id, as a
//! player must to play the rest of a file with a bad patch in it. Every read
//! is bounded, and nothing panics on any input.

use std::collections::VecDeque;
use std::io::{self, Read, Seek};

use super::{
    DEFAULT_DURATION, DURATION, El, INFO, SEEK_HEAD, SEGMENT, TIMESTAMP_SCALE, TRACK_ENTRY, TRACKS,
    element_at, elements, float, read_body, read_seek_head, read_track, uint, vint,
};
use crate::{MAX_CHILDREN, read_at};

const EBML_HEADER: u64 = 0x1A45_DFA3;
const CLUSTER: u64 = 0x1F43_B675;
const CLUSTER_TIMESTAMP: u64 = 0xE7;
const SIMPLE_BLOCK: u64 = 0xA3;
const BLOCK_GROUP: u64 = 0xA0;
const BLOCK: u64 = 0xA1;
const BLOCK_DURATION: u64 = 0x9B;
const REFERENCE_BLOCK: u64 = 0xFB;
const CUES: u64 = 0x1C53_BB6B;
const CUE_POINT: u64 = 0xBB;
const CUE_TIME: u64 = 0xB3;
const CUE_TRACK_POSITIONS: u64 = 0xB7;
const CUE_TRACK: u64 = 0xF7;
const CUE_CLUSTER_POSITION: u64 = 0xF1;
const TRACK_NUMBER: u64 = 0xD7;
const CODEC_PRIVATE: u64 = 0x63A2;
const CODEC_DELAY: u64 = 0x56AA;
const SEEK_PRE_ROLL: u64 = 0x56BB;
const CONTENT_ENCODINGS: u64 = 0x6D80;
const CONTENT_ENCODING: u64 = 0x6240;
const CONTENT_COMPRESSION: u64 = 0x5034;
const CONTENT_COMP_ALGO: u64 = 0x4254;
const CONTENT_COMP_SETTINGS: u64 = 0x4255;
const CONTENT_ENCRYPTION: u64 = 0x5035;
const TAGS: u64 = 0x1254_C367;
const ATTACHMENTS: u64 = 0x1941_A469;
const CHAPTERS: u64 = 0x1043_A770;

/// The elements a Segment holds at its top level: one of them where a
/// cluster of unknown length has a child means that cluster is over.
const TOP_LEVEL: [u64; 8] = [
    CLUSTER,
    CUES,
    TAGS,
    ATTACHMENTS,
    CHAPTERS,
    SEEK_HEAD,
    INFO,
    TRACKS,
];

/// The most of one block that is read: a 4K key frame is a few megabytes,
/// and a block is one frame or a lace of a few.
const MAX_BLOCK: u64 = 64 * 1024 * 1024;
/// The most of `Cues` that is read: a cue a second for days.
const MAX_CUES: usize = 16 * 1024 * 1024;
/// The most of a `BlockGroup` around its block that is read.
const MAX_GROUP_EXTRA: u64 = 64 * 1024;
/// How far past bad bytes a cluster's id is looked for, a chunk at a time.
const RESYNC_CHUNK: usize = 64 * 1024;
/// The id of a `Cluster`, as it is written.
const CLUSTER_ID_BYTES: [u8; 4] = [0x1F, 0x43, 0xB6, 0x75];

/// One frame of one track.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Packet {
    /// The track's number, as its blocks name it ([`Stream::number`]).
    pub track: u64,
    /// When the frame is to be shown, in nanoseconds from the Segment's
    /// start. Negative for a frame before it (an encoder's priming).
    pub timestamp_ns: i64,
    /// How long it lasts, when the file says: a `BlockGroup`'s
    /// `BlockDuration`, or the track's `DefaultDuration`.
    pub duration_ns: Option<u64>,
    /// Whether decoding may start at this frame.
    pub keyframe: bool,
    /// The frame's bytes, with any stripped header put back.
    pub data: Vec<u8>,
}

/// One track, as the demuxer reads it.
#[derive(Clone, Debug, PartialEq)]
pub struct Stream {
    /// The number its blocks name it by.
    pub number: u64,
    /// What the probe says of it: kind, codec, picture size, rate, language.
    pub info: crate::Track,
    /// `CodecPrivate`: the setup a decoder needs before the first frame
    /// (an Opus head, a Vorbis codebook); empty for VP8 and VP9.
    pub codec_private: Vec<u8>,
    /// `DefaultDuration`, nanoseconds a frame.
    pub default_duration_ns: Option<u64>,
    /// `CodecDelay`: nanoseconds of decoded output to drop at the start.
    pub codec_delay_ns: u64,
    /// `SeekPreRoll`: how far before a seek target to start decoding.
    pub seek_pre_roll_ns: u64,
    /// Whether its frames can be given back: false for a track compressed
    /// other than by header stripping, or encrypted. Its frames are passed
    /// over.
    pub readable: bool,
    /// The bytes header stripping took off the front of every frame.
    strip: Vec<u8>,
}

/// Where a cue says a track's frames can be started from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Cue {
    time: u64,
    track: u64,
    /// The cluster's position from the Segment's body.
    cluster: u64,
}

/// The cluster being read.
#[derive(Clone, Copy, Debug)]
struct InCluster {
    /// Where its next child begins.
    at: u64,
    /// Where it ends: its own end, or the Segment's for one of unknown length.
    end: u64,
    /// Its timestamp, in ticks.
    time: u64,
}

/// The frames of a Matroska or WebM file, in order.
#[derive(Debug)]
pub struct Demuxer<R> {
    r: R,
    len: u64,
    /// The Segment's body and end.
    body: u64,
    end: u64,
    /// Nanoseconds a tick.
    scale: u64,
    duration_ns: Option<u64>,
    streams: Vec<Stream>,
    cues: Vec<Cue>,
    /// Where the first cluster is, for a seek to the start or a walk.
    first_cluster: Option<u64>,
    /// Where the next top-level element begins, between clusters.
    next: u64,
    cluster: Option<InCluster>,
    /// Frames of a laced block not yet given back.
    queue: VecDeque<Packet>,
}

impl<R: Read + Seek> Demuxer<R> {
    /// Reads the headers of the Matroska or WebM file `r`, `len` bytes long:
    /// its tracks, its timestamp scale and length, and its cues. `None` for
    /// a file that is neither -- no EBML header and Segment.
    ///
    /// # Errors
    ///
    /// What reading `r` fails with.
    pub fn open(mut r: R, len: u64) -> io::Result<Option<Self>> {
        let Some(header) = element_at(&mut r, 0, len, len)?.filter(|e| e.id == EBML_HEADER) else {
            return Ok(None);
        };
        let Some(segment) = element_at(&mut r, header.end, len, len)?.filter(|e| e.id == SEGMENT)
        else {
            return Ok(None);
        };
        let mut found: [Option<El>; 3] = [None; 3];
        let mut first_cluster = None;
        let mut seeks = Vec::new();
        let mut at = segment.body;
        let mut walked = 0_usize;
        // The headers before the first cluster, which is where muxers put
        // them; the seek head says where the rest are.
        while at < segment.end && walked < MAX_CHILDREN {
            let Some(el) = element_at(&mut r, at, segment.end, len)? else {
                break;
            };
            walked = walked.saturating_add(1);
            match el.id {
                INFO => found[0] = found[0].or(Some(el)),
                TRACKS => found[1] = found[1].or(Some(el)),
                CUES => found[2] = found[2].or(Some(el)),
                SEEK_HEAD => seeks.extend(read_seek_head(&mut r, el, len)?),
                CLUSTER => {
                    first_cluster = Some(at);
                    break;
                }
                _ => {}
            }
            at = el.end;
        }
        for (id, position) in seeks {
            let slot = match id {
                INFO => 0,
                TRACKS => 1,
                CUES => 2,
                _ => continue,
            };
            let Some(entry) = found.get_mut(slot).filter(|e| e.is_none()) else {
                continue;
            };
            let at = segment.body.saturating_add(position);
            *entry = element_at(&mut r, at, segment.end, len)?.filter(|e| e.id == id);
        }
        let [info, tracks, cues] = found;
        let mut demuxer = Self {
            len,
            body: segment.body,
            end: segment.end,
            scale: 1_000_000,
            duration_ns: None,
            streams: Vec::new(),
            cues: Vec::new(),
            first_cluster,
            next: first_cluster.unwrap_or(segment.end),
            cluster: None,
            queue: VecDeque::new(),
            r,
        };
        if let Some(info) = info {
            let b = read_body(&mut demuxer.r, info, super::MAX_INFO, len)?;
            demuxer.read_info(&b);
        }
        if let Some(tracks) = tracks {
            let b = read_body(&mut demuxer.r, tracks, super::MAX_TRACKS, len)?;
            demuxer.streams = elements(&b)
                .into_iter()
                .filter(|(id, _)| *id == TRACK_ENTRY)
                .filter_map(|(_, entry)| read_stream(entry))
                .collect();
        }
        if let Some(cues) = cues {
            let b = read_body(&mut demuxer.r, cues, MAX_CUES, len)?;
            demuxer.cues = read_cues(&b);
        }
        Ok(Some(demuxer))
    }

    /// The tracks, in the order the file lists them.
    #[must_use]
    pub fn streams(&self) -> &[Stream] {
        &self.streams
    }

    /// The Segment's length, in nanoseconds, when the file says.
    #[must_use]
    pub fn duration_ns(&self) -> Option<u64> {
        self.duration_ns
    }

    /// Whether the file has cues, so that a seek reads one cluster rather
    /// than walking every one before the time sought.
    #[must_use]
    pub fn has_cues(&self) -> bool {
        !self.cues.is_empty()
    }

    fn read_info(&mut self, b: &[u8]) {
        let mut duration = None;
        for (id, value) in elements(b) {
            match id {
                TIMESTAMP_SCALE => {
                    if let Some(s) = uint(value).filter(|&s| s > 0) {
                        self.scale = s;
                    }
                }
                DURATION => duration = float(value),
                _ => {}
            }
        }
        #[expect(
            clippy::cast_precision_loss,
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a timestamp scale is far inside f64's integer-exact range, and the product is checked finite, positive and in range first"
        )]
        {
            self.duration_ns = duration
                .map(|ticks| ticks * self.scale as f64)
                .filter(|ns| ns.is_finite() && *ns >= 0.0 && *ns < u64::MAX as f64)
                .map(|ns| ns as u64);
        }
    }

    /// The next frame of any track, in the order the file holds them --
    /// which is decode order -- or `None` at the end.
    ///
    /// # Errors
    ///
    /// What reading the file fails with. Damage is not an error: what does
    /// not parse is stepped over.
    pub fn next_packet(&mut self) -> io::Result<Option<Packet>> {
        loop {
            if let Some(p) = self.queue.pop_front() {
                return Ok(Some(p));
            }
            match self.cluster {
                Some(c) => self.step_in_cluster(c)?,
                None => {
                    if !self.enter_next_cluster()? {
                        return Ok(None);
                    }
                }
            }
        }
    }

    /// Moves `next` to the next cluster and into it; false at the end.
    fn enter_next_cluster(&mut self) -> io::Result<bool> {
        let mut walked = 0_usize;
        while self.next < self.end && walked < MAX_CHILDREN {
            walked = walked.saturating_add(1);
            let at = self.next;
            let Some(el) = element_at(&mut self.r, at, self.end, self.len)? else {
                // No element here: bad bytes. The next cluster's id, if any.
                match self.find_cluster(at.saturating_add(1))? {
                    Some(found) => {
                        self.next = found;
                        continue;
                    }
                    None => {
                        self.next = self.end;
                        return Ok(false);
                    }
                }
            };
            if el.id == CLUSTER {
                self.cluster = Some(InCluster {
                    at: el.body,
                    end: el.end,
                    time: 0,
                });
                // Where the walk resumes if the cluster's length is known;
                // one of unknown length moves it when it finds its end.
                self.next = el.end;
                return Ok(true);
            }
            self.next = el.end;
        }
        Ok(false)
    }

    /// Reads the cluster's next child, queueing the frames of a block.
    fn step_in_cluster(&mut self, c: InCluster) -> io::Result<()> {
        if c.at >= c.end {
            self.cluster = None;
            return Ok(());
        }
        let Some(el) = element_at(&mut self.r, c.at, c.end, self.len)? else {
            // The cluster is damaged from here: on to the next one.
            self.cluster = None;
            self.next = self
                .find_cluster(c.at.saturating_add(1))?
                .unwrap_or(self.end);
            return Ok(());
        };
        if TOP_LEVEL.contains(&el.id) {
            // A cluster of unknown length ends where the Segment's next
            // element begins.
            self.cluster = None;
            self.next = c.at;
            return Ok(());
        }
        let mut next = InCluster { at: el.end, ..c };
        match el.id {
            CLUSTER_TIMESTAMP => {
                let b = read_body(&mut self.r, el, 8, self.len)?;
                next.time = uint(&b).unwrap_or(0);
            }
            SIMPLE_BLOCK => {
                if el.end.saturating_sub(el.body) <= MAX_BLOCK {
                    let b = read_body(&mut self.r, el, usize::MAX, self.len)?;
                    self.queue_block(&b, c.time, None, None);
                }
            }
            BLOCK_GROUP => {
                if el.end.saturating_sub(el.body) <= MAX_BLOCK.saturating_add(MAX_GROUP_EXTRA) {
                    let b = read_body(&mut self.r, el, usize::MAX, self.len)?;
                    let mut block = None;
                    let mut duration = None;
                    let mut referenced = false;
                    for (id, value) in elements(&b) {
                        match id {
                            BLOCK => block = Some(value),
                            BLOCK_DURATION => duration = uint(value),
                            REFERENCE_BLOCK => referenced = true,
                            _ => {}
                        }
                    }
                    if let Some(block) = block {
                        self.queue_block(block, c.time, Some(!referenced), duration);
                    }
                }
            }
            _ => {}
        }
        self.cluster = Some(next);
        Ok(())
    }

    /// The frames of the block `b` in a cluster at `cluster_time` ticks,
    /// queued. `keyframe` is a `BlockGroup`'s answer (no `ReferenceBlock`);
    /// a `SimpleBlock` says in its flags. A block that does not parse, or
    /// of a track that is not there or cannot be read, queues nothing.
    fn queue_block(
        &mut self,
        b: &[u8],
        cluster_time: u64,
        keyframe: Option<bool>,
        duration: Option<u64>,
    ) {
        let Some(block) = parse_block(b) else {
            return;
        };
        let Some(stream) = self.streams.iter().find(|s| s.number == block.track) else {
            return;
        };
        if !stream.readable {
            return;
        }
        let keyframe = keyframe.unwrap_or(block.flags & 0x80 != 0);
        // In i128, where ticks of up to 2^64 at a scale of up to 2^64
        // nanoseconds cannot overflow; then clamped to an i64 of nanoseconds.
        let scale = i128::from(self.scale);
        let start = i128::from(cluster_time)
            .saturating_add(i128::from(block.relative))
            .saturating_mul(scale);
        let default_ns = stream.default_duration_ns;
        let duration_ns = duration
            .map(|ticks| u64::try_from(i128::from(ticks).saturating_mul(scale)).unwrap_or(u64::MAX))
            .or(default_ns);
        for (i, frame) in block.frames.iter().enumerate() {
            // Frames after a lace's first follow it by the track's frame
            // duration; without one, they share its time.
            let offset = default_ns.map_or(0, |d| {
                i128::from(d).saturating_mul(i128::try_from(i).unwrap_or(0))
            });
            let mut data = Vec::with_capacity(stream.strip.len().saturating_add(frame.len()));
            data.extend_from_slice(&stream.strip);
            data.extend_from_slice(frame);
            self.queue.push_back(Packet {
                track: block.track,
                timestamp_ns: i64::try_from(start.saturating_add(offset)).unwrap_or(i64::MAX),
                duration_ns: if block.frames.len() > 1 {
                    default_ns
                } else {
                    duration_ns
                },
                // Of a lace, the first frame is where decoding may start.
                keyframe: keyframe && i == 0,
                data,
            });
        }
    }

    /// The position of the next cluster's id at or after `from`, searched a
    /// chunk at a time to the Segment's end.
    fn find_cluster(&mut self, from: u64) -> io::Result<Option<u64>> {
        let mut at = from;
        while at < self.end {
            let want = usize::try_from(self.end.saturating_sub(at))
                .unwrap_or(usize::MAX)
                .min(RESYNC_CHUNK);
            let chunk = read_at(&mut self.r, at, want, self.len)?;
            if chunk.len() < CLUSTER_ID_BYTES.len() {
                return Ok(None);
            }
            if let Some(i) = chunk.windows(4).position(|w| w == CLUSTER_ID_BYTES) {
                return Ok(Some(at.saturating_add(i as u64)));
            }
            // Three bytes back, for an id across two chunks.
            let step = chunk.len().saturating_sub(3).max(1);
            at = at.saturating_add(step as u64);
        }
        Ok(None)
    }

    /// Goes to where decoding should start to show `time_ns`: the cluster of
    /// the last cue at or before it for `track` (any track's if it has none),
    /// or, without cues, the last cluster that starts at or before it. The
    /// frames given back from there may begin before `time_ns`: a player
    /// decodes them and shows from `time_ns` on, and skips frames until a key
    /// frame.
    ///
    /// # Errors
    ///
    /// What reading the file fails with.
    pub fn seek(&mut self, time_ns: u64, track: u64) -> io::Result<()> {
        self.queue.clear();
        self.cluster = None;
        let ticks = time_ns.checked_div(self.scale).unwrap_or(0);
        let target = if self.cues.is_empty() {
            self.walk_to(ticks)?
        } else {
            let of_track = self.cues.iter().any(|c| c.track == track);
            self.cues
                .iter()
                .filter(|c| !of_track || c.track == track)
                .filter(|c| c.time <= ticks)
                .max_by_key(|c| c.time)
                .map(|c| self.body.saturating_add(c.cluster))
                .or(self.first_cluster)
        };
        self.next = target.unwrap_or(self.end);
        Ok(())
    }

    /// The last cluster from the first that starts at or before `ticks`,
    /// found by reading each one's timestamp.
    fn walk_to(&mut self, ticks: u64) -> io::Result<Option<u64>> {
        let Some(mut at) = self.first_cluster else {
            return Ok(None);
        };
        let mut best = Some(at);
        // Each pass moves `at` on -- past an element, which is at least its
        // two header bytes, or to a cluster's id found after it -- so the
        // walk ends at the Segment's end at the latest.
        while at < self.end {
            let Some(el) = element_at(&mut self.r, at, self.end, self.len)? else {
                match self.find_cluster(at.saturating_add(1))? {
                    Some(found) => {
                        at = found;
                        continue;
                    }
                    None => break,
                }
            };
            if el.id != CLUSTER {
                at = el.end;
                continue;
            }
            // Its timestamp is its first child, or near it: the first bytes of
            // its body, and no further than its end.
            let head = read_body(&mut self.r, el, 64, self.len)?;
            let time = elements(&head)
                .into_iter()
                .find(|(id, _)| *id == CLUSTER_TIMESTAMP)
                .and_then(|(_, v)| uint(v));
            match time {
                Some(t) if t > ticks => break,
                Some(_) => best = Some(at),
                None => {}
            }
            if el.end >= self.end {
                // Of unknown length: the next cluster is wherever its id is.
                match self.find_cluster(el.body)? {
                    Some(found) => at = found,
                    None => break,
                }
            } else {
                at = el.end;
            }
        }
        Ok(best)
    }
}

/// A `TrackEntry` as the demuxer needs it; `None` without a track number.
fn read_stream(entry: &[u8]) -> Option<Stream> {
    let info = read_track(entry);
    let mut stream = Stream {
        number: 0,
        info,
        codec_private: Vec::new(),
        default_duration_ns: None,
        codec_delay_ns: 0,
        seek_pre_roll_ns: 0,
        readable: true,
        strip: Vec::new(),
    };
    for (id, value) in elements(entry) {
        match id {
            TRACK_NUMBER => stream.number = uint(value).unwrap_or(0),
            CODEC_PRIVATE => stream.codec_private = value.to_vec(),
            DEFAULT_DURATION => stream.default_duration_ns = uint(value).filter(|&n| n > 0),
            CODEC_DELAY => stream.codec_delay_ns = uint(value).unwrap_or(0),
            SEEK_PRE_ROLL => stream.seek_pre_roll_ns = uint(value).unwrap_or(0),
            CONTENT_ENCODINGS => read_encodings(value, &mut stream),
            _ => {}
        }
    }
    (stream.number != 0).then_some(stream)
}

/// A track's `ContentEncodings`: header stripping is undone; any other
/// compression, or encryption, makes the track unreadable here.
fn read_encodings(value: &[u8], stream: &mut Stream) {
    for (id, encoding) in elements(value) {
        if id != CONTENT_ENCODING {
            continue;
        }
        for (id, part) in elements(encoding) {
            match id {
                CONTENT_COMPRESSION => {
                    let fields = elements(part);
                    // Algorithm 0 (zlib) unless it says otherwise.
                    let algo = fields
                        .iter()
                        .find(|(i, _)| *i == CONTENT_COMP_ALGO)
                        .and_then(|(_, v)| uint(v))
                        .unwrap_or(0);
                    if algo == 3 {
                        if let Some((_, settings)) =
                            fields.iter().find(|(i, _)| *i == CONTENT_COMP_SETTINGS)
                        {
                            stream.strip = settings.to_vec();
                        }
                    } else {
                        stream.readable = false;
                    }
                }
                CONTENT_ENCRYPTION => stream.readable = false,
                _ => {}
            }
        }
    }
}

/// The `Cues`: each cue point's time and, for each track it names, where
/// its cluster is.
fn read_cues(b: &[u8]) -> Vec<Cue> {
    let mut out = Vec::new();
    for (id, point) in elements(b) {
        if id != CUE_POINT {
            continue;
        }
        let fields = elements(point);
        let Some(time) = fields
            .iter()
            .find(|(i, _)| *i == CUE_TIME)
            .and_then(|(_, v)| uint(v))
        else {
            continue;
        };
        for (id, positions) in &fields {
            if *id != CUE_TRACK_POSITIONS {
                continue;
            }
            let p = elements(positions);
            let track = p
                .iter()
                .find(|(i, _)| *i == CUE_TRACK)
                .and_then(|(_, v)| uint(v));
            let cluster = p
                .iter()
                .find(|(i, _)| *i == CUE_CLUSTER_POSITION)
                .and_then(|(_, v)| uint(v));
            if let (Some(track), Some(cluster)) = (track, cluster) {
                out.push(Cue {
                    time,
                    track,
                    cluster,
                });
            }
        }
    }
    out
}

/// A block's header and its frames.
#[derive(Debug, PartialEq, Eq)]
struct Block<'a> {
    track: u64,
    /// Ticks from its cluster's timestamp.
    relative: i16,
    flags: u8,
    frames: Vec<&'a [u8]>,
}

/// A `Block` or `SimpleBlock`'s body: a track number (an EBML integer), a
/// signed 16-bit time, flags, and the frames -- one, or a lace.
fn parse_block(b: &[u8]) -> Option<Block<'_>> {
    let (track, n) = vint(b, 0, false)?;
    let relative = i16::from_be_bytes(crate::bytes(b, n)?);
    let flags = *b.get(n.checked_add(2)?)?;
    let data = b.get(n.checked_add(3)?..)?;
    let frames = match flags & 0x06 {
        0x00 => vec![data],
        lacing => unlace(data, lacing)?,
    };
    Some(Block {
        track,
        relative,
        flags,
        frames,
    })
}

/// A lace's frames: a count less one, then the sizes of all frames but the
/// last -- Xiph's as runs of bytes (255 continues), EBML's as an integer and
/// then signed differences, fixed-size as none -- then the frames.
fn unlace(data: &[u8], lacing: u8) -> Option<Vec<&[u8]>> {
    let count = usize::from(*data.first()?).checked_add(1)?;
    let mut at = 1_usize;
    let mut sizes: Vec<usize> = Vec::with_capacity(count);
    match lacing {
        // Xiph.
        0x02 => {
            for _ in 1..count {
                let mut size = 0_usize;
                loop {
                    let byte = *data.get(at)?;
                    at = at.checked_add(1)?;
                    size = size.checked_add(usize::from(byte))?;
                    if byte != 255 {
                        break;
                    }
                }
                sizes.push(size);
            }
        }
        // Fixed-size: the rest in equal parts.
        0x04 => {
            let rest = data.len().checked_sub(at)?;
            if rest.checked_rem(count)? != 0 {
                return None;
            }
            sizes.resize(count.checked_sub(1)?, rest.checked_div(count)?);
        }
        // EBML: the first size, then each the last plus a signed difference
        // (the integer less the middle of its range).
        _ => {
            let (first, n) = vint(data, at, false)?;
            at = at.checked_add(n)?;
            let mut size = i128::from(first);
            if count > 1 {
                sizes.push(usize::try_from(size).ok()?);
            }
            for _ in 2..count {
                let (raw, n) = vint(data, at, false)?;
                at = at.checked_add(n)?;
                let bits = u32::try_from(n.checked_mul(7)?.checked_sub(1)?).ok()?;
                let bias = 1_i128.checked_shl(bits)?.checked_sub(1)?;
                size = size.checked_add(i128::from(raw).checked_sub(bias)?)?;
                sizes.push(usize::try_from(size).ok()?);
            }
        }
    }
    let mut frames = Vec::with_capacity(count);
    for size in sizes {
        let end = at.checked_add(size)?;
        frames.push(data.get(at..end)?);
        at = end;
    }
    // The last frame is the rest.
    frames.push(data.get(at..)?);
    Some(frames)
}

#[cfg(test)]
mod tests;
