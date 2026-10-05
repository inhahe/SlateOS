//! A logical stream: what it holds, its headers, its packets put together
//! from the pages' segments, and each packet's time.
//!
//! **Codecs.** A stream's first packet says what it is (RFC 7845 for Opus,
//! the Vorbis I specification's appendix A, Xiph's mappings for FLAC,
//! Speex, Theora and Skeleton), and how its header packets are told from
//! its data. Opus and Vorbis are timed; the others' packets are given
//! untimed.
//!
//! **Times** are FFmpeg's (its Ogg demuxer, `libavformat/oggdec.c` with
//! `oggparseopus.c` and `oggparsevorbis.c`, as of git `9b7439c31b`): a
//! page's granule position gives the end of the last packet completed on
//! it. The packet after a page's last starts at that page's granule; any
//! other packet -- the stream's first, or one in the middle of a page --
//! starts at its page's granule less its own length and the lengths of the
//! packets after it on the page, unless the page is the stream's last, where
//! the packets after the first are left untimed (and the stream's first
//! page, when it is also its last, is taken to start at granule 0). An Opus
//! stream's times have its pre-skip taken off. On the last page, what runs
//! past its granule comes off the end of the packet it runs past in: the
//! packet's discard padding.
//!
//! **Where this differs from FFmpeg.**
//!
//! - *A Vorbis packet's length* is a quarter of the block before it and a
//!   quarter of its own. FFmpeg forgets the block before at every packet
//!   that is not the first after a page's end, so a short block in the
//!   middle of a page that follows a long one comes out
//!   `(long - short) / 4` samples late and that much short (448 samples at
//!   44.1 kHz's usual sizes, 10 ms), its end right: two of a stream's
//!   blocks then overlap in time. Here every packet's length is the one the
//!   decoder gives it.
//! - *A packet whose length cannot be read* (a damaged first byte) lasts
//!   nothing here and is flagged; FFmpeg stops reading an Opus stream at
//!   one, and may time a Vorbis page's earlier packets at 0.
//! - *A lost page* -- a gap in a stream's page numbers, a page whose CRC is
//!   wrong -- loses the packet it cut, as libogg loses it, and the packet
//!   after the gap is timed from its own page; FFmpeg, which reads no page
//!   numbers, joins the cut packet's two ends and times on from the page
//!   before the gap.
//! - *An empty Opus packet* is a packet (RFC 7845's way of saying one was
//!   lost); FFmpeg stops reading the stream at it.
//! - *A Vorbis stream of one page* -- its first page its last, as a stream
//!   of a second or less is -- starts with its first sound at granule 0, as
//!   the Vorbis I specification (A.2) and libvorbis start it, its first
//!   packet (which only primes the decoder) its own length before that.
//!   FFmpeg starts that first packet at 0, a packet's length late, and so
//!   cuts as much too much off the end -- or, where the cut then comes to more
//!   than the last packet holds, nothing, playing the encoder's padding.

#![allow(
    clippy::arithmetic_side_effects,
    reason = "times are i64 sample counts from 64-bit granules: the sums are saturating where a hostile granule could reach the edge, and offsets within a page are bounded by its 65 307 bytes"
)]

use crate::Packet;
use crate::page::{CONTINUED, LAST, Page};

/// What a logical stream holds, by its first packet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Codec {
    /// `OpusHead` (RFC 7845).
    Opus,
    /// `\x01vorbis` (Vorbis I, appendix A).
    Vorbis,
    /// `\x7fFLAC` (Xiph's Ogg FLAC mapping).
    Flac,
    /// `Speex   `.
    Speex,
    /// `\x80theora`.
    Theora,
    /// `fishead\0`: Ogg Skeleton, an index of the other streams.
    Skeleton,
    /// Anything else.
    Other,
}

impl Codec {
    /// The codec whose first packet this is.
    fn identify(first: &[u8]) -> Self {
        if first.starts_with(b"OpusHead") {
            Self::Opus
        } else if first.starts_with(b"\x01vorbis") {
            Self::Vorbis
        } else if first.starts_with(b"\x7fFLAC") {
            Self::Flac
        } else if first.starts_with(b"Speex   ") {
            Self::Speex
        } else if first.starts_with(b"\x80theora") {
            Self::Theora
        } else if first.starts_with(b"fishead\0") {
            Self::Skeleton
        } else {
            Self::Other
        }
    }

    /// Whether this reads the stream's packets' lengths, and so times them.
    pub fn is_timed(self) -> bool {
        matches!(self, Self::Opus | Self::Vorbis)
    }
}

/// A logical stream, as its headers describe it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stream {
    pub serial: u32,
    pub codec: Codec,
    /// The codec's header packets, in order, as the file has them: Opus's
    /// `OpusHead` and `OpusTags`; Vorbis's identification, comment and
    /// setup headers. A codec's headers can be damaged or missing: they are
    /// what the file holds, for the decoder to accept or refuse.
    pub headers: Vec<Vec<u8>>,
    /// The packets' ticks a second: 48 000 for Opus, the sampling rate for
    /// Vorbis; 0 for a stream that is not timed.
    pub rate: u32,
    /// Channels, as the first header says; 0 where it does not.
    pub channels: u32,
    /// Opus's pre-skip, in samples at 48 kHz; 0 for the others.
    pub pre_skip: u32,
}

/// A packet put together from segments: its bytes, where they begin, and
/// the page they begin on.
pub(crate) struct Assembled {
    pub data: Vec<u8>,
    pub position: u64,
    pub page_position: u64,
}

/// What the stream is reading.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    /// Its headers; the number read so far.
    Headers,
    /// Its data.
    Data,
}

/// A stream being read: its description, its headers, the packet being put
/// together, and what its next packet's time depends on.
#[derive(Clone)]
pub(crate) struct State {
    pub info: Stream,
    phase: Phase,
    /// Vorbis's block sizes and modes, from its headers.
    blocks: Option<vorbis::Blocks>,
    /// Vorbis headers read in band (a type-1, -3 or -5 packet among the
    /// data), until there are all three.
    in_band: [Option<Vec<u8>>; 3],
    /// Headers to give with the next data packet: the stream's when they
    /// change in band, or its link's after a jump into a chained file.
    pending_headers: Option<Vec<Vec<u8>>>,
    /// The packet being put together, and where it began.
    partial: Vec<u8>,
    partial_at: Option<(u64, u64)>,
    /// The last page's sequence number.
    sequence: Option<u32>,
    /// FFmpeg's `lastpts`: the time the next packet starts at, where it is
    /// known -- the last page's granule after its last packet, 0 at the
    /// stream's start.
    anchor: Option<i64>,
    /// The block size of the last Vorbis packet, where it is known.
    previous_block: Option<usize>,
    /// Whether no packet has been read since the stream started: FFmpeg's
    /// Vorbis parser then still holds the size its headers gave it.
    untouched: bool,
    /// Opus: where the last packet ended (FFmpeg's `cur_dts`).
    opus_end: i64,
    /// Vorbis on its last page: the first packet's time there, and the
    /// lengths since (FFmpeg's `final_pts`, `final_duration`).
    final_pts: Option<i64>,
    final_duration: i64,
    /// Samples to drop from the start of the next packet: Opus's pre-skip.
    pending_skip: u32,
    /// Added to every time: where this link's stream begins on the file's
    /// clock (0 in a file of one link).
    pub offset: i64,
    /// Whether its last page has been read.
    pub ended: bool,
}

impl State {
    /// A stream from its first page's serial.
    pub(crate) fn new(serial: u32) -> Self {
        Self {
            info: Stream {
                serial,
                codec: Codec::Other,
                headers: Vec::new(),
                rate: 0,
                channels: 0,
                pre_skip: 0,
            },
            phase: Phase::Headers,
            blocks: None,
            in_band: [None, None, None],
            pending_headers: None,
            partial: Vec::new(),
            partial_at: None,
            sequence: None,
            anchor: Some(0),
            previous_block: None,
            untouched: true,
            opus_end: 0,
            final_pts: None,
            final_duration: 0,
            pending_skip: 0,
            offset: 0,
            ended: false,
        }
    }

    /// The packets that end on `page`, put together: a page missing before
    /// it loses the packet it cut, as does a page continuing a packet whose
    /// start was not read.
    pub(crate) fn assemble(&mut self, page: &Page) -> Vec<Assembled> {
        let gap = self
            .sequence
            .is_some_and(|s| s.wrapping_add(1) != page.sequence);
        self.sequence = Some(page.sequence);
        if gap {
            // What was lost had lengths: the last page's granule no longer
            // says where the next packet starts, nor the last block what the
            // next overlaps. The next is timed back from its own page.
            self.partial.clear();
            self.partial_at = None;
            self.anchor = None;
            self.previous_block = None;
        }
        let mut skipping = page.has(CONTINUED) && self.partial_at.is_none();
        let mut out = Vec::new();
        let mut at = 0usize;
        let base = page.body_position();
        for &len in &page.lacing {
            let len = usize::from(len);
            let segment = page.body.get(at..at + len).unwrap_or_default();
            let position = base + at as u64;
            at += len;
            if skipping {
                skipping = len == 255;
                continue;
            }
            if self.partial_at.is_none() {
                self.partial_at = Some((position, page.position));
            }
            self.partial.extend_from_slice(segment);
            if len < 255 {
                let (position, page_position) =
                    self.partial_at.take().unwrap_or((position, page.position));
                out.push(Assembled {
                    data: std::mem::take(&mut self.partial),
                    position,
                    page_position,
                });
            }
        }
        out
    }

    /// Takes a packet as a header if it is one; `false` once the stream's
    /// headers are done, for its data.
    pub(crate) fn header(&mut self, packet: &[u8]) -> bool {
        if self.phase == Phase::Data {
            return false;
        }
        if self.info.headers.is_empty() {
            self.info.codec = Codec::identify(packet);
            self.first_header(packet);
            self.info.headers.push(packet.to_vec());
            if self.info.codec == Codec::Other {
                self.phase = Phase::Data;
            }
            return true;
        }
        let is_header = match self.info.codec {
            // `OpusTags`, which an Opus stream must have; a stream without
            // it begins its data at once.
            Codec::Opus => self.info.headers.len() == 1 && packet.starts_with(b"OpusTags"),
            // Vorbis's headers are its packets with the first bit set, until
            // the setup header (type 5).
            Codec::Vorbis => packet.first().is_some_and(|b| b & 1 == 1),
            // A FLAC frame begins with its sync code's 0xff; a header with a
            // metadata block's type.
            Codec::Flac => packet.first().is_some_and(|&b| b != 0xff),
            Codec::Speex => self.info.headers.len() < 2 + speex_extra_headers(&self.info.headers),
            Codec::Theora => packet.first().is_some_and(|b| b & 0x80 != 0),
            Codec::Skeleton => true,
            Codec::Other => false,
        };
        if !is_header {
            self.phase = Phase::Data;
            return false;
        }
        self.info.headers.push(packet.to_vec());
        let done = match self.info.codec {
            Codec::Opus => true,
            Codec::Vorbis => packet.first() == Some(&5),
            _ => false,
        };
        if done {
            self.headers_complete();
        }
        true
    }

    /// The first packet's facts.
    fn first_header(&mut self, packet: &[u8]) {
        match self.info.codec {
            Codec::Opus => {
                // RFC 7845 §5.1: the channel count at 9, the pre-skip at 10.
                // A version whose upper nibble is set is one this does not
                // read (as FFmpeg refuses it): the stream is left untimed.
                if packet.len() >= 19 && packet.get(8).is_some_and(|v| v & 0xf0 == 0) {
                    self.info.rate = 48_000;
                    self.info.channels = u32::from(packet.get(9).copied().unwrap_or(0));
                    self.info.pre_skip = u32::from(u16::from_le_bytes([
                        packet.get(10).copied().unwrap_or(0),
                        packet.get(11).copied().unwrap_or(0),
                    ]));
                    self.pending_skip = self.info.pre_skip;
                }
            }
            Codec::Vorbis => {
                if let Ok(info) = vorbis::Info::parse(packet) {
                    self.info.rate = info.rate;
                    self.info.channels = u32::try_from(info.channels).unwrap_or(0);
                }
            }
            _ => {}
        }
    }

    /// Every header read: Vorbis's blocks made from them.
    fn headers_complete(&mut self) {
        self.phase = Phase::Data;
        if self.info.codec == Codec::Vorbis {
            self.blocks = self.vorbis_blocks(&self.info.headers);
        }
    }

    /// Vorbis's blocks from its headers -- the identification header and
    /// the setup header, wherever they are among them.
    fn vorbis_blocks(&self, headers: &[Vec<u8>]) -> Option<vorbis::Blocks> {
        let find = |kind: u8| headers.iter().find(|h| h.first() == Some(&kind));
        vorbis::Blocks::new(find(1)?, find(5)?).ok()
    }

    /// Forgets what reading had in hand, for a jump to `position`: the
    /// stream's start (`at_start`), else the middle; with the headers to
    /// give with the next packet, in a chained file.
    pub(crate) fn jump(&mut self, at_start: bool, headers: Option<Vec<Vec<u8>>>) {
        self.partial.clear();
        self.partial_at = None;
        self.sequence = None;
        self.anchor = at_start.then_some(0);
        self.previous_block = None;
        self.untouched = at_start;
        self.opus_end = 0;
        self.final_pts = None;
        self.final_duration = 0;
        self.in_band = [None, None, None];
        self.pending_skip = if at_start { self.info.pre_skip } else { 0 };
        self.pending_headers = headers;
        self.ended = false;
    }

    /// Resumes after a jump to `page`, the last of the stream to end at or
    /// before the time sought: the packets completed on it are passed over,
    /// the next starts at its granule, and the one it leaves unfinished is
    /// kept.
    pub(crate) fn resume_after(&mut self, page: &Page) {
        let done = self.assemble(page);
        // The last block's size, where its packet is whole on the page.
        if let (Some(blocks), Some(last)) = (&self.blocks, done.last()) {
            if last.page_position == page.position {
                self.previous_block = blocks.block(&last.data).ok().map(|b| b.size);
            }
        }
        if page.granule != -1 {
            self.anchor = Some(page.granule);
        }
        self.ended = page.has(LAST);
    }

    /// A packet's length in ticks where it can be read, and its Vorbis
    /// block's size: a quarter of the block before -- `previous` where it
    /// is known, else what a long block's window bits say it was, else
    /// `unknown_short` -- and a quarter of its own.
    fn length(
        &self,
        packet: &[u8],
        previous: Option<usize>,
        unknown_short: usize,
    ) -> Option<(u64, Option<usize>)> {
        match self.info.codec {
            Codec::Opus => opus_samples(packet).map(|n| (u64::from(n), None)),
            Codec::Vorbis => {
                let block = self.blocks.as_ref()?.block(packet).ok()?;
                let before = previous.or(block.previous).unwrap_or(unknown_short);
                Some(((before / 4 + block.size / 4) as u64, Some(block.size)))
            }
            _ => None,
        }
    }

    /// Whether a packet among the data is a header in band: Opus's head or
    /// tags again, a Vorbis header.
    fn is_in_band_header(&self, packet: &[u8]) -> bool {
        match self.info.codec {
            Codec::Opus => {
                packet.len() > 8
                    && (packet.starts_with(b"OpusHead") || packet.starts_with(b"OpusTags"))
            }
            Codec::Vorbis => matches!(packet.first(), Some(1 | 3 | 5)),
            _ => false,
        }
    }

    /// The packets that ended on `page`, its data timed (see the module
    /// documentation) and its headers in band taken, as the stream at
    /// `index` gives them.
    pub(crate) fn time(
        &mut self,
        page: &Page,
        packets: Vec<Assembled>,
        index: usize,
    ) -> Vec<Packet> {
        let codec = self.info.codec;
        let timed = codec.is_timed() && self.info.rate > 0;
        let eos = page.has(LAST);
        // The page's granule, where it can be used: an Opus granule past
        // 2^62 is one FFmpeg refuses, and a negative one is none.
        let granule = (page.granule >= 0 && (codec != Codec::Opus || page.granule <= 1 << 62))
            .then_some(page.granule);
        let pre_skip = i64::from(self.info.pre_skip);
        // Each data packet's length, each reckoned from the one before.
        let unknown_short = self.unknown_short(eos, granule);
        let mut previous = self.previous_block;
        let lengths: Vec<Option<u64>> = packets
            .iter()
            .filter(|p| !self.is_in_band_header(&p.data))
            .map(|p| {
                let length = self.length(&p.data, previous, unknown_short);
                if let Some((_, Some(block))) = length {
                    previous = Some(block);
                }
                length.map(|(n, _)| n)
            })
            .collect();
        self.previous_block = previous;
        // Whether this page's first data packet is the stream's first.
        let at_start = self.untouched;
        self.untouched = false;
        // Whether the page's last packet ends on it, rather than running on.
        let whole_last = page.lacing.last().is_none_or(|&l| l < 255);
        let total = packets.len();
        let mut k = 0;
        let mut out = Vec::with_capacity(total);
        for (i, packet) in packets.into_iter().enumerate() {
            if self.in_band_header(&packet.data) {
                continue;
            }
            let length = lengths.get(k).copied().flatten();
            let rest: i64 = lengths
                .get(k..)
                .unwrap_or_default()
                .iter()
                .map(|l| i64::try_from(l.unwrap_or(0)).unwrap_or(i64::MAX))
                .fold(0, i64::saturating_add);
            k += 1;
            if !timed {
                out.push(self.packet(index, None, 0, false, packet, 0));
                continue;
            }
            let first_of_stream = at_start && k == 1;
            // Where nothing gives this packet a start, it is reckoned back
            // from the page's granule: its length and those after it.
            let mut reckoned = false;
            if (self.anchor.is_none() || self.anchor == Some(0)) && !eos {
                if let Some(g) = granule {
                    self.anchor = Some(g.saturating_sub(rest));
                    reckoned = true;
                    // FFmpeg's guard against a broken file (its ticket
                    // 3710): a page at granule 0 holding sound is untimed.
                    if codec == Codec::Vorbis && g == 0 && rest != 0 {
                        self.anchor = None;
                    }
                    self.final_pts = None;
                }
            }
            // A Vorbis stream whose first page gives nothing to reckon back
            // from -- it is also the last, as a stream short enough for one
            // page is -- starts where the Vorbis I specification starts it
            // (A.2): its first sound, the second packet's, at granule 0; the
            // first packet, which only primes the decoder, its own length
            // before that. FFmpeg starts the first packet at 0, a packet's
            // length late, and so cuts that much too much off the end -- or,
            // where the cut is then more than the last packet holds, nothing.
            if codec == Codec::Vorbis && first_of_stream && !reckoned && self.anchor == Some(0) {
                self.anchor = Some(0i64.saturating_sub(as_signed(length.unwrap_or(0))));
            }
            let mut duration = length.unwrap_or(0);
            let corrupt = length.is_none() && !packet.data.is_empty();
            let as_i64 = as_signed;
            let pts = self.anchor.take().map(|a| a.saturating_sub(pre_skip));
            let mut discard = 0u64;
            match codec {
                Codec::Opus => {
                    if let Some(p) = pts {
                        self.opus_end = p;
                    }
                    self.opus_end = self.opus_end.saturating_add(as_i64(duration));
                    if let (true, Some(g)) = (eos, granule) {
                        let past = self
                            .opus_end
                            .saturating_sub(g)
                            .saturating_add(pre_skip)
                            .min(as_i64(duration));
                        if past > 0 {
                            discard = past.unsigned_abs();
                            duration = if discard < duration {
                                duration - discard
                            } else {
                                1
                            };
                        }
                    }
                }
                Codec::Vorbis if eos => {
                    if pts.is_some() {
                        self.final_pts = pts;
                        self.final_duration = 0;
                    }
                    if i + 1 == total && whole_last {
                        if let (Some(fp), Some(g)) = (self.final_pts, granule) {
                            let start = fp.saturating_add(self.final_duration);
                            let past = start.saturating_add(as_i64(duration)).saturating_sub(g);
                            if past > 0 {
                                discard = past.unsigned_abs();
                            }
                            duration = u64::try_from(g.saturating_sub(start)).unwrap_or(0);
                        }
                    }
                    self.final_duration = self.final_duration.saturating_add(as_i64(duration));
                }
                _ => {}
            }
            let pts = pts.map(|p| p.saturating_add(self.offset));
            out.push(self.packet(index, pts, duration, corrupt, packet, discard));
        }
        // The next packet starts where this page's last ended.
        if page.ends_a_packet() && page.granule != -1 {
            self.anchor = Some(page.granule);
        }
        if eos {
            self.ended = true;
        }
        out
    }

    /// The size a short block's unknown predecessor is taken to have: the
    /// short size, as FFmpeg's parser has it once reset -- or, where it has
    /// not been (the stream's first page is its last, or has no granule),
    /// mode 0's, which it starts with.
    fn unknown_short(&self, eos: bool, granule: Option<i64>) -> usize {
        let Some(blocks) = &self.blocks else { return 0 };
        if self.untouched && (eos || granule.is_none()) {
            blocks.mode_size(0).unwrap_or(blocks.sizes()[0])
        } else {
            blocks.sizes()[0]
        }
    }

    /// A packet among the data that is a header in band: Opus's head or tags
    /// again, a Vorbis header. Taken, and given with the next data packet
    /// once whole.
    fn in_band_header(&mut self, packet: &[u8]) -> bool {
        match self.info.codec {
            Codec::Opus if packet.len() > 8 && packet.starts_with(b"OpusHead") => {
                self.info.headers = vec![packet.to_vec()];
                self.first_header(packet);
                self.pending_headers = Some(self.info.headers.clone());
                true
            }
            Codec::Opus if packet.len() > 8 && packet.starts_with(b"OpusTags") => {
                if self.info.headers.len() == 1 {
                    self.info.headers.push(packet.to_vec());
                    self.pending_headers = Some(self.info.headers.clone());
                }
                true
            }
            Codec::Vorbis => {
                let kind = match packet.first() {
                    Some(1) => 0,
                    Some(3) => 1,
                    Some(5) => 2,
                    _ => return false,
                };
                if let Some(slot) = self.in_band.get_mut(kind) {
                    *slot = Some(packet.to_vec());
                }
                if self.in_band.iter().all(Option::is_some) {
                    let headers: Vec<Vec<u8>> = std::mem::take(&mut self.in_band)
                        .into_iter()
                        .flatten()
                        .collect();
                    if let Some(blocks) = self.vorbis_blocks(&headers) {
                        self.blocks = Some(blocks);
                        if let Some(id) = headers.first() {
                            let id = id.clone();
                            self.first_header(&id);
                        }
                    }
                    self.info.headers = headers;
                    self.pending_headers = Some(self.info.headers.clone());
                }
                true
            }
            _ => false,
        }
    }

    fn packet(
        &mut self,
        stream: usize,
        pts: Option<i64>,
        duration: u64,
        corrupt: bool,
        a: Assembled,
        discard: u64,
    ) -> Packet {
        Packet {
            stream,
            pts,
            duration,
            data: a.data,
            skip_samples: std::mem::take(&mut self.pending_skip),
            discard_padding: u32::try_from(discard).unwrap_or(u32::MAX),
            corrupt,
            position: a.position,
            page_position: a.page_position,
            new_headers: self.pending_headers.take(),
        }
    }
}

/// A length as a signed count of ticks.
fn as_signed(n: u64) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

/// An Opus packet's length at 48 kHz, from its TOC byte (RFC 6716 §3.1),
/// as FFmpeg's Ogg demuxer reckons it: `None` for an empty packet, or a
/// code-3 packet without its count byte.
fn opus_samples(packet: &[u8]) -> Option<u32> {
    let &toc = packet.first()?;
    let config = u32::from(toc >> 3);
    let frame = if config < 12 {
        // SILK: 10, 20, 40, 60 ms.
        match config & 3 {
            0 => 480,
            1 => 960,
            2 => 1920,
            _ => 2880,
        }
    } else if config < 16 {
        // Hybrid: 10, 20 ms.
        480 << (config & 1)
    } else {
        // CELT: 2.5, 5, 10, 20 ms.
        120 << (config & 3)
    };
    let frames = match toc & 3 {
        0 => 1,
        3 => u32::from(packet.get(1)? & 0x3f),
        _ => 2,
    };
    Some(frame * frames)
}

/// How many extra headers a Speex stream's first header announces.
fn speex_extra_headers(headers: &[Vec<u8>]) -> usize {
    headers
        .first()
        .and_then(|h| h.get(76..80))
        .and_then(|b| b.try_into().ok())
        .map_or(0, |b: [u8; 4]| u32::from_le_bytes(b).min(64) as usize)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "a test: a failure should be loud"
)]
mod tests {
    use super::*;

    #[test]
    fn an_opus_packets_length_is_its_tocs() {
        // SILK 10/20/40/60 ms, hybrid 10/20, CELT 2.5/5/10/20.
        for (config, samples) in [
            (0, 480),
            (1, 960),
            (2, 1920),
            (3, 2880),
            (12, 480),
            (13, 960),
            (16, 120),
            (17, 240),
            (18, 480),
            (19, 960),
        ] {
            assert_eq!(
                opus_samples(&[config << 3]),
                Some(samples),
                "config {config}"
            );
            // Two frames, either way.
            assert_eq!(opus_samples(&[config << 3 | 1]), Some(samples * 2));
            assert_eq!(opus_samples(&[config << 3 | 2]), Some(samples * 2));
            // A count byte.
            assert_eq!(opus_samples(&[config << 3 | 3, 0x85]), Some(samples * 5));
        }
        assert_eq!(opus_samples(&[]), None);
        assert_eq!(opus_samples(&[3]), None);
        // FFmpeg counts what the byte says, past 120 ms or none at all.
        assert_eq!(opus_samples(&[19 << 3 | 3, 0x3f]), Some(960 * 63));
        assert_eq!(opus_samples(&[19 << 3 | 3, 0x40]), Some(0));
    }

    #[test]
    fn a_stream_is_told_by_its_first_packet() {
        for (first, codec) in [
            (&b"OpusHead\x01\x02"[..], Codec::Opus),
            (b"\x01vorbis\0\0", Codec::Vorbis),
            (b"\x7fFLAC\x01\0", Codec::Flac),
            (b"Speex   1.2", Codec::Speex),
            (b"\x80theora", Codec::Theora),
            (b"fishead\0", Codec::Skeleton),
            (b"OpusHea", Codec::Other),
            (b"", Codec::Other),
        ] {
            assert_eq!(Codec::identify(first), codec);
        }
    }

    #[test]
    fn packets_are_put_together_across_pages_and_lost_with_a_page() {
        use crate::page::tests::page;
        let parse = |bytes: Vec<u8>, at: u64| Page::parse(&bytes, at).unwrap();
        let long = vec![7u8; 255];
        let mut s = State::new(1);
        // "a", then a packet of 255 + 3 bytes split across two pages.
        let p0 = parse(page(0, 1, 1, 0, &[b"a", &long]), 0);
        let got = s.assemble(&p0);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].data, b"a");
        assert_eq!(got[0].position, 27 + 2);
        let p1 = parse(page(CONTINUED, 2, 1, 1, &[b"bcd", b"e"]), 1000);
        let got = s.assemble(&p1);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].data.len(), 258);
        assert_eq!((got[0].position, got[0].page_position), (27 + 2 + 1, 0));
        assert_eq!(
            (got[1].data.as_slice(), got[1].page_position),
            (&b"e"[..], 1000)
        );
        // A page lost (sequence 3 missing): the packet it began is lost, and
        // so is the continued start of the page after.
        let p2 = parse(page(0, 3, 1, 2, &[b"f", &long]), 2000);
        assert_eq!(s.assemble(&p2).len(), 1);
        let p4 = parse(page(CONTINUED, 5, 1, 4, &[b"gh", b"i"]), 3000);
        let got = s.assemble(&p4);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].data, b"i");
    }
}
