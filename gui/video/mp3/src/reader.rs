//! `.mp3`, `.mp2` and `.mp1` files, taken apart into packets as FFmpeg 6.1
//! takes them apart for its decoder, and timed and trimmed as it times and
//! trims them: [`Reader`].
//!
//! - **The start.** ID3v2 tags in front are passed over, as every FFmpeg
//!   demuxer passes them (`libavformat/id3v2.c`), each by its size and, in
//!   ID3v2.4, its footer's. The first frame may be a Xing, Info or VBRI
//!   frame -- a Layer III frame of silence that says how many frames and
//!   bytes follow it -- and if it says either it is not played
//!   (`libavformat/mp3dec.c`, `mp3_parse_vbr_tags`); its LAME tag, where
//!   LAME, libavformat or libavcodec wrote it, gives the encoder's delay and
//!   padding. Then up to 64 KiB of junk is passed over, to the first frame
//!   whose next frame agrees with it.
//! - **Packets.** FFmpeg's MPEG audio parser (`libavcodec/mpegaudio_parser.c`)
//!   is given the file in the demuxer's reads of 1024 bytes, and ends a
//!   packet at the end of each frame it finds -- a frame being any four
//!   bytes that make a header, and as long as that header says -- so a
//!   packet is the junk before a frame, if any, and the frame. What is left
//!   at the end of the file is a packet too, unless it is an ID3v1 or APE
//!   tag.
//! - **Times**, on FFmpeg's clock ([`TICKS_PER_SECOND`]): from 0 at the
//!   first packet, each packet as long as the parser's count of a frame's
//!   samples, which keeps the last agreeing frame's through junk.
//! - **Trims**: the encoder's delay and the decoder's 529 samples (528 + 1)
//!   come off the start of the stream; its padding, less 529, off its end,
//!   where the frame count says it ends (FFmpeg's `start_skip_samples`,
//!   `first_discard_sample` and `last_discard_sample`, as
//!   `libavformat/demux.c` gives them each packet as side data).
//! - **Seeks** go to the last packet at or before a time, by the times the
//!   packets read so far had -- FFmpeg's generic index -- reading on as far
//!   as the time first.
//!
//! **Where it differs from FFmpeg:** a file whose only frame ends it opens
//! here, where FFmpeg's check for junk reads past the end and refuses the
//! file; a free-format stream (no bit rate in its headers) is read, its
//! frames sized as minimp3 sizes them, where FFmpeg reads none; and after a
//! seek, packets are timed as reading through times them (see
//! `Parser::reset`). A stream with no frame count has its duration
//! estimated at its first frame's bit rate, where FFmpeg averages over the
//! frames it read to find the stream's parameters: the same at a constant
//! bit rate.

#![allow(
    clippy::arithmetic_side_effects,
    reason = "positions within a file and counts of its bytes and samples, far from u64's and i64's ends"
)]

use std::collections::VecDeque;
use std::io::{self, Read, Seek, SeekFrom};

/// FFmpeg's clock for MPEG audio: ticks a second, the least common multiple
/// of its nine rates, so that every sample is a whole number of ticks.
pub const TICKS_PER_SECOND: u64 = 14_112_000;

/// The demuxer's read: what the parser is given at a time.
const READ_SIZE: usize = 1024;

/// How far past the start the demuxer looks for a frame its next agrees
/// with.
const JUNK_SEARCH: u64 = 64 * 1024;

/// The decoder's delay: what every MPEG audio decoder's filterbank holds back
/// (FFmpeg's and minimp3's alike), dropped with the encoder's own.
pub const DECODER_DELAY: u64 = 528 + 1;

/// `MP3_MASK`: the header bits two frames of one stream share for the
/// demuxer's junk check.
const MP3_MASK: u32 = 0xFFFE_0CCF;

/// `SAME_HEADER_MASK`: the bits the parser compares, frame to frame.
const SAME_HEADER_MASK: u32 = 0xffe0_0000 | (3 << 17) | (3 << 10) | (3 << 19);

/// The bits minimp3 compares to call two frames one stream's (`hdr_compare`:
/// sync, version, layer, rate) -- free format's sizing, here.
const FREE_FORMAT_MASK: u32 = 0xFFFE_0C00 | 0xF000;

/// minimp3's longest free-format frame.
const MAX_FREE_FORMAT_FRAME_SIZE: usize = 2304;

/// `av_rescale(a, b, c)`: `a * b / c`, to the nearest, halves up.
fn rescale(a: u64, b: u64, c: u64) -> u64 {
    let c = u128::from(c.max(1));
    u64::try_from((u128::from(a) * u128::from(b) + c / 2) / c).unwrap_or(u64::MAX)
}

/// A frame header, as `avpriv_mpegaudio_decode_header` reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    /// The four bytes, big-endian.
    pub raw: u32,
    /// 1, 2 or 3.
    pub layer: u32,
    /// MPEG-2 or -2.5: half the samples a Layer III frame.
    pub lsf: bool,
    pub sample_rate: u32,
    /// 1 or 2.
    pub channels: usize,
    /// The frame's size in bytes, header included; 0 in free format, where
    /// the header does not say.
    pub frame_size: usize,
    pub bit_rate: u32,
}

impl Header {
    /// `ff_mpa_check_header` and `avpriv_mpegaudio_decode_header`: the
    /// header in `raw`, or `None` where it is not one. A free-format header
    /// is one, with `frame_size` 0.
    #[must_use]
    pub fn parse(raw: u32) -> Option<Self> {
        if raw & 0xffe0_0000 != 0xffe0_0000
            || raw & (3 << 19) == 1 << 19
            || raw & (3 << 17) == 0
            || raw & (0xf << 12) == 0xf << 12
            || raw & (3 << 10) == 3 << 10
        {
            return None;
        }
        let (lsf, mpeg25) = if raw & (1 << 20) != 0 {
            (raw & (1 << 19) == 0, false)
        } else {
            (true, true)
        };
        let layer = 4 - ((raw >> 17) & 3);
        let shift = u32::from(lsf) + u32::from(mpeg25);
        let sample_rate = [44100u32, 48000, 32000]
            .get(((raw >> 10) & 3) as usize)
            .copied()
            .unwrap_or(44100)
            >> shift;
        let bitrate_index = ((raw >> 12) & 0xf) as usize;
        let padding = ((raw >> 9) & 1) as usize;
        let channels = if (raw >> 6) & 3 == 3 { 1 } else { 2 };
        // `ff_mpa_bitrate_tab[lsf][layer - 1]`, in kbit/s.
        const KBPS: [[[u32; 15]; 3]; 2] = [
            [
                [
                    0, 32, 64, 96, 128, 160, 192, 224, 256, 288, 320, 352, 384, 416, 448,
                ],
                [
                    0, 32, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384,
                ],
                [
                    0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320,
                ],
            ],
            [
                [
                    0, 32, 48, 56, 64, 80, 96, 112, 128, 144, 160, 176, 192, 224, 256,
                ],
                [0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160],
                [0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160],
            ],
        ];
        let kbps = KBPS
            .get(usize::from(lsf))
            .and_then(|t| t.get((layer - 1) as usize))
            .and_then(|t| t.get(bitrate_index))
            .copied()
            .unwrap_or(0);
        let rate = sample_rate as usize;
        let frame_size = if kbps == 0 {
            0
        } else {
            let k = kbps as usize;
            match layer {
                1 => (k * 12000 / rate + padding) * 4,
                2 => k * 144_000 / rate + padding,
                _ => k * 144_000 / (rate << usize::from(lsf)) + padding,
            }
        };
        Some(Self {
            raw,
            layer,
            lsf,
            sample_rate,
            channels,
            frame_size,
            bit_rate: kbps * 1000,
        })
    }

    /// Its padding, in bytes: a Layer I slot is four.
    #[must_use]
    pub fn padding(&self) -> usize {
        match ((self.raw >> 9) & 1, self.layer) {
            (0, _) => 0,
            (_, 1) => 4,
            _ => 1,
        }
    }

    /// Samples a channel the frame decodes to.
    #[must_use]
    pub fn samples(&self) -> u64 {
        match (self.layer, self.lsf) {
            (1, _) => 384,
            (3, true) => 576,
            _ => 1152,
        }
    }

    /// Ticks a sample, at this header's rate.
    fn ticks_per_sample(&self) -> u64 {
        TICKS_PER_SECOND / u64::from(self.sample_rate.max(1))
    }
}

/// What the start of the file says about the stream.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Info {
    /// Where the first packet starts: past the ID3v2 tags, the Xing, Info or
    /// VBRI frame and the junk the demuxer passes over.
    pub data_offset: u64,
    /// The first frame's header -- the stream's rate and channels, as
    /// FFmpeg's decoder first reports them.
    pub first: Option<Header>,
    /// The Xing, Info or VBRI frame's count of frames, if it gives one and
    /// the file is not longer than its count of bytes says by a sixteenth
    /// (FFmpeg takes such a file for two joined, and the count for the
    /// first's).
    pub frames: Option<u32>,
    /// Its count of bytes.
    pub bytes: Option<u32>,
    /// Its LAME tag's encoder delay and padding, in samples.
    pub encoder: Option<(u32, u32)>,
    /// Samples a channel the stream drops from its start: the encoder's
    /// delay and [`DECODER_DELAY`].
    pub start_skip: u64,
    /// The first sample past the stream's end, and the end of the frames
    /// the count says there are: FFmpeg's `first_discard_sample` and
    /// `last_discard_sample`.
    pub discard: Option<(u64, u64)>,
    /// How long the stream plays, in ticks: by the frame count, or else
    /// estimated, as FFmpeg estimates it, from the file's size and the
    /// first frame's bit rate.
    pub duration: Option<u64>,
    /// Whether `duration` is that estimate.
    pub duration_estimated: bool,
    /// A free-format stream's frame size, padding aside: not a format FFmpeg
    /// reads; found as minimp3 finds it.
    pub free_format: Option<usize>,
}

/// One packet, as FFmpeg's parser gives it to the decoder.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Packet {
    /// Its bytes: any junk the parser took with the frame, then the frame.
    pub data: Vec<u8>,
    /// Where in `data` the frame's header starts, if `data` ends with a
    /// whole frame -- `None` for what is left at the end of the file
    /// holding none.
    pub frame: Option<usize>,
    /// Where its first byte is in the file.
    pub position: u64,
    /// When it starts and how long it lasts, in ticks.
    pub pts: i64,
    pub duration: i64,
    /// Samples a channel to drop from the start of what it decodes to, and
    /// from the end: the side data FFmpeg gives it.
    pub skip_samples: u64,
    pub discard_padding: u64,
}

impl Packet {
    /// The frame's bytes.
    #[must_use]
    pub fn frame_bytes(&self) -> Option<&[u8]> {
        self.data.get(self.frame?..)
    }
}

/// A packet the parser finished: its bytes, where it starts in the file,
/// and where in it the frame's header is.
struct Parsed {
    data: Vec<u8>,
    position: u64,
    frame: Option<usize>,
}

/// FFmpeg's MPEG audio parser's state (`MpegAudioParseContext`, with the
/// `ParseContext` buffer and the codec context fields it writes).
#[derive(Clone, Debug, Default)]
struct Parser {
    /// The last four bytes seen, looking for a header.
    state: u32,
    /// Bytes of the current frame still to come.
    frame_size: usize,
    /// The last header found, and how many in a row agreed with it.
    header: u32,
    header_count: i32,
    /// The parser's count of a frame's samples: what a packet lasts.
    duration: u64,
    /// The codec context's rate and layer, as the parser last set them; the
    /// layer starts at the demuxer's, 3.
    sample_rate: u32,
    layer: u32,
    /// The packet being gathered, where it starts in the file, and where in
    /// it the current frame's header starts.
    buffer: Vec<u8>,
    buffer_position: u64,
    header_at: Option<usize>,
    /// A free-format stream's header bits and frame size: its frames are
    /// sized by it (FFmpeg's parser takes no free-format header for one).
    free_format: Option<(u32, usize)>,
}

impl Parser {
    fn new() -> Self {
        Self {
            layer: 3,
            ..Self::default()
        }
    }

    /// Forgets the packet in hand and what it was looking for, as a seek
    /// does (`ff_read_frame_flush` closes the parser), keeping what it had
    /// told the codec context -- and, **unlike FFmpeg**, its count of a
    /// frame's samples, which a new parser would not have for a packet whose
    /// frame does not agree with the stream (junk's look-alike): FFmpeg
    /// times such a packet as lasting nothing after a seek, and every packet
    /// after it a frame early, where reading through it gave it a frame's
    /// time. Here a seek's packets are timed as reading through times them.
    fn reset(&mut self) {
        *self = Self {
            sample_rate: self.sample_rate,
            layer: self.layer,
            duration: self.duration,
            free_format: self.free_format,
            ..Self::new()
        };
    }

    /// `state` as a header, a free-format one sized as the stream's are.
    fn header(&self, state: u32) -> Option<Header> {
        let mut h = Header::parse(state)?;
        if h.frame_size == 0
            && let Some((bits, size)) = self.free_format
            && state & FREE_FORMAT_MASK == bits
        {
            h.frame_size = size + h.padding();
        }
        Some(h).filter(|h| h.frame_size > 0)
    }

    /// `mpegaudio_parse` on `buf`, which starts at `position` in the file:
    /// the bytes it used, and the packet it finished, if any -- its bytes,
    /// position and frame header's place.
    fn parse(&mut self, buf: &[u8], position: u64) -> (usize, Option<Parsed>) {
        if self.buffer.is_empty() {
            self.buffer_position = position;
        }
        let mut state = self.state;
        let mut i = 0usize;
        let mut next = None;
        while i < buf.len() {
            if self.frame_size > 0 {
                let inc = (buf.len() - i).min(self.frame_size);
                i += inc;
                self.frame_size -= inc;
                state = 0;
                if self.frame_size == 0 {
                    next = Some(i);
                    break;
                }
            } else {
                while i < buf.len() {
                    state = (state << 8) | u32::from(buf.get(i).copied().unwrap_or(0));
                    i += 1;
                    // `ff_mpa_decode_header`: a header with its size; free
                    // format is not one to the parser.
                    match self.header(state) {
                        None => {
                            if i > 4 {
                                self.header_count = -2;
                            }
                        }
                        Some(h) => {
                            let threshold = i32::from(self.layer != h.layer);
                            if (state & SAME_HEADER_MASK) != (self.header & SAME_HEADER_MASK)
                                && self.header != 0
                            {
                                self.header_count = -3;
                            }
                            self.header = state;
                            self.header_count += 1;
                            self.frame_size = h.frame_size.saturating_sub(4);
                            // The header's first byte, in the packet: what
                            // came before this call, and this call's bytes
                            // up to it (it may have begun in an earlier one).
                            self.header_at = (self.buffer.len() + i).checked_sub(4);
                            if self.header_count > threshold {
                                self.sample_rate = h.sample_rate;
                                self.duration = h.samples();
                                self.layer = h.layer;
                            }
                            break;
                        }
                    }
                }
            }
        }
        self.state = state;
        match next {
            None => {
                self.buffer.extend_from_slice(buf);
                (buf.len(), None)
            }
            Some(n) => {
                let mut packet = std::mem::take(&mut self.buffer);
                packet.extend_from_slice(buf.get(..n).unwrap_or_default());
                let frame = self.header_at.take();
                (
                    n,
                    Some(Parsed {
                        data: packet,
                        position: self.buffer_position,
                        frame,
                    }),
                )
            }
        }
    }

    /// What is left at the end of the file, as the parser's last packet:
    /// `None` if nothing is, or if it is an ID3v1 or APE tag.
    fn flush(&mut self) -> Option<(Vec<u8>, u64)> {
        let rest = std::mem::take(&mut self.buffer);
        self.header_at = None;
        self.frame_size = 0;
        if rest.is_empty()
            || (rest.len() >= 128 && rest.starts_with(b"TAG"))
            || (rest.len() >= 32 && rest.starts_with(b"APETAGEX"))
        {
            return None;
        }
        Some((rest, self.buffer_position))
    }
}

/// `mp3_read_probe`: how sure FFmpeg's MP3 demuxer is that `buf`, the
/// start of a file, is MPEG audio -- 0 (not) to 51 (seven frames in a row at
/// its start). Every run of frames is followed from every byte; a run
/// counts while each frame's header, and at most two look-alikes inside it,
/// agree.
#[must_use]
pub fn probe(buf: &[u8]) -> u32 {
    let word = |i: usize| -> u32 {
        buf.get(i..i + 4)
            .and_then(|b| b.try_into().ok())
            .map_or(0, u32::from_be_bytes)
    };
    let end = buf.len().saturating_sub(4);
    let mut buf0 = 0usize;
    while buf0 < end && buf.get(buf0) == Some(&0) {
        buf0 += 1;
    }
    let (mut max_frames, mut max_framesizes, mut first_frames) = (0usize, 0usize, 0usize);
    let mut whole_used = false;
    let mut b = buf0;
    while b < end {
        let mut b2 = b;
        let (mut frames, mut framesizes) = (0usize, 0usize);
        while b2 < end {
            let header = word(b2);
            let Some(h) = Header::parse(header).filter(|h| h.frame_size > 0) else {
                break;
            };
            let available = h.frame_size.min(end - b2);
            let emulations = (b2 + 4..b2 + available)
                .filter(|&b3| word(b3) & MP3_MASK == header & MP3_MASK)
                .count();
            if emulations > 2 {
                break;
            }
            framesizes += h.frame_size;
            if available < h.frame_size {
                frames += 1;
                break;
            }
            b2 += h.frame_size;
            frames += 1;
        }
        max_frames = max_frames.max(frames);
        max_framesizes = max_framesizes.max(framesizes);
        if b == buf0 {
            first_frames = frames;
            whole_used = b2 == buf.len();
        }
        b = b2 + 1;
    }
    let tag = buf
        .get(buf0..buf0 + 10)
        .and_then(|h| <&[u8; 10]>::try_from(h).ok())
        .and_then(id3v2_len);
    if first_frames >= 7 {
        51
    } else if max_frames > 200 && buf.len() < 2 * max_framesizes {
        50
    } else if max_frames >= 4 && buf.len() < 2 * max_framesizes {
        25
    } else if tag.is_some_and(|len| 2 * len >= buf.len() as u64) {
        if buf.len() < PROBE_MAX { 12 } else { 48 }
    } else if first_frames > 1 && whole_used {
        5
    } else if max_frames >= 1 && buf.len() < 10 * max_framesizes {
        1
    } else {
        0
    }
}

/// The most FFmpeg reads to decide what a file is: 1 MiB.
const PROBE_MAX: usize = 1 << 20;

/// Whether `source` is MPEG audio as FFmpeg would decide it, had it no
/// other format to consider: past its ID3v2 tags, its first 2 KiB, then 4,
/// 8 and so on to 1 MiB (or the whole file), until the probe is more than
/// half sure (over 25) -- or, at the last, sure at all.
///
/// # Errors
///
/// When `source` fails.
pub fn is_mpeg_audio<R: Read + Seek>(source: &mut R) -> io::Result<bool> {
    let size = source.seek(SeekFrom::End(0))?;
    let mut off = 0u64;
    loop {
        let mut head = [0u8; 10];
        if off + 10 > size {
            break;
        }
        source.seek(SeekFrom::Start(off))?;
        source.read_exact(&mut head)?;
        match id3v2_len(&head) {
            Some(len) => off += len,
            None => break,
        }
    }
    let rest = size.saturating_sub(off);
    let last = usize::try_from(rest).unwrap_or(PROBE_MAX).min(PROBE_MAX);
    let mut buf = vec![0u8; last];
    source.seek(SeekFrom::Start(off))?;
    source.read_exact(&mut buf)?;
    let mut len = 2048usize.min(last);
    loop {
        let score = probe(buf.get(..len).unwrap_or_default());
        if len == last {
            return Ok(score > 0);
        }
        if score > 25 {
            return Ok(true);
        }
        len = (len * 2).min(last);
    }
}

/// An MPEG audio file, read packet by packet.
pub struct Reader<R> {
    source: R,
    size: u64,
    info: Info,
    parser: Parser,
    /// Where the next read starts, and whether the file has ended.
    read_position: u64,
    ended: bool,
    /// The read in hand, and how much of it the parser has used.
    chunk: Vec<u8>,
    chunk_used: usize,
    chunk_position: u64,
    /// Packets parsed and not yet given.
    queue: VecDeque<Packet>,
    /// The time the next packet starts at, in ticks.
    next_pts: i64,
    /// Whether the stream's first packets are still being gathered: those
    /// whose duration the parser had not yet counted take the first count
    /// it makes (`update_initial_durations`).
    at_start: bool,
    /// Every packet read, in order: where it starts and when.
    index: Vec<(u64, i64)>,
}

/// The ID3v2 tag at `position`, if one starts there: its length, header,
/// body and footer (`ff_id3v2_match`; `id3v2_parse`'s end).
fn id3v2_len(head: &[u8; 10]) -> Option<u64> {
    let synchsafe = head.get(6..10)?;
    if &head[..3] != b"ID3"
        || head[3] == 0xff
        || head[4] == 0xff
        || synchsafe.iter().any(|b| b & 0x80 != 0)
    {
        return None;
    }
    let len = synchsafe
        .iter()
        .fold(0u64, |acc, &b| (acc << 7) | u64::from(b & 0x7f));
    // A footer counts in ID3v2.4 only; a tag of a version FFmpeg cannot read,
    // or a compressed ID3v2.2 one, is passed over without it.
    let footer = if head[3] == 4 && head[5] & 0x10 != 0 {
        10
    } else {
        0
    };
    Some(10 + len + footer)
}

impl<R: Read + Seek> Reader<R> {
    /// The file in `source`, its start read as FFmpeg's demuxer reads it.
    ///
    /// # Errors
    ///
    /// When `source` fails.
    pub fn open(mut source: R) -> io::Result<Self> {
        let size = source.seek(SeekFrom::End(0))?;
        let mut reader = Self {
            source,
            size,
            info: Info::default(),
            parser: Parser::new(),
            read_position: 0,
            ended: false,
            chunk: Vec::new(),
            chunk_used: 0,
            chunk_position: 0,
            queue: VecDeque::new(),
            next_pts: 0,
            at_start: true,
            index: Vec::new(),
        };
        let mut off = 0u64;
        while let Some(head) = reader.read_array::<10>(off)? {
            match id3v2_len(&head) {
                Some(len) => off += len,
                None => break,
            }
        }
        off = reader.parse_vbr_tags(off)?;
        // A free-format stream is not one to FFmpeg, whose search for junk
        // would pass its frames over looking for sized ones: here it starts
        // where its first frame does -- found, where the search finds no
        // sized ones, as minimp3 finds it, at the first frame of three that
        // agree.
        let free = match reader.free_format_at(off)? {
            Some(size) => Some((off, size)),
            None => match reader.skip_junk(off)? {
                Some(start) => {
                    off = start;
                    None
                }
                None => reader.search_free_format(off)?,
            },
        };
        if let Some((start, size)) = free {
            off = start;
            reader.info.free_format = Some(size);
            let bits = reader
                .header_at(off)?
                .map_or(0, |h| h.raw & FREE_FORMAT_MASK);
            reader.parser.free_format = Some((bits, size));
        }
        reader.info.data_offset = off;
        reader.info.first = reader
            .read_array::<4>(off)?
            .and_then(|b| reader.parser.header(u32::from_be_bytes(b)));
        reader.read_position = off;
        if reader.info.duration.is_none()
            && let Some(h) = reader.info.first
        {
            // `estimate_timings_from_bit_rate`: the bytes from the first
            // packet on at the first frame's bit rate (FFmpeg's is the
            // parser's average over the frames it reads to find the stream's
            // parameters: the same for a constant bit rate) -- a free-format
            // frame's being its size at its rate.
            let bit_rate = if h.bit_rate > 0 {
                u64::from(h.bit_rate)
            } else {
                rescale(
                    h.frame_size as u64 * 8,
                    u64::from(h.sample_rate),
                    h.samples(),
                )
            };
            if bit_rate > 0 {
                let bits = size.saturating_sub(off) * 8;
                reader.info.duration = Some(rescale(bits, TICKS_PER_SECOND, bit_rate));
                reader.info.duration_estimated = true;
            }
        }
        Ok(reader)
    }

    /// The first free-format stream within 64 KiB of `off`, where it starts
    /// and its frame size.
    fn search_free_format(&mut self, off: u64) -> io::Result<Option<(u64, usize)>> {
        let window = self.read_bytes(off, usize::try_from(JUNK_SEARCH + 4).unwrap_or(0))?;
        for i in 0..window.len().saturating_sub(4) {
            let free = window
                .get(i..i + 4)
                .and_then(|b| b.try_into().ok())
                .and_then(|b| Header::parse(u32::from_be_bytes(b)))
                .is_some_and(|h| h.frame_size == 0);
            if free && let Some(size) = self.free_format_at(off + i as u64)? {
                return Ok(Some((off + i as u64, size)));
            }
        }
        Ok(None)
    }

    /// What the file's start says.
    pub fn info(&self) -> &Info {
        &self.info
    }

    /// The `N` bytes at `position`, or `None` past the end.
    fn read_array<const N: usize>(&mut self, position: u64) -> io::Result<Option<[u8; N]>> {
        if position.saturating_add(N as u64) > self.size {
            return Ok(None);
        }
        let mut buf = [0u8; N];
        self.source.seek(SeekFrom::Start(position))?;
        self.source.read_exact(&mut buf)?;
        Ok(Some(buf))
    }

    /// Up to `len` bytes at `position`, fewer at the end of the file.
    fn read_bytes(&mut self, position: u64, len: usize) -> io::Result<Vec<u8>> {
        let len = self.size.saturating_sub(position).min(len as u64);
        let mut buf = vec![0u8; usize::try_from(len).unwrap_or(0)];
        self.source.seek(SeekFrom::Start(position))?;
        self.source.read_exact(&mut buf)?;
        Ok(buf)
    }

    /// The header at `position`, if its four bytes make one.
    fn header_at(&mut self, position: u64) -> io::Result<Option<Header>> {
        Ok(self
            .read_array::<4>(position)?
            .and_then(|b| Header::parse(u32::from_be_bytes(b))))
    }

    /// `mp3_parse_vbr_tags`: a Xing, Info or VBRI frame at `off` read, and
    /// passed over if it gives a frame or byte count; where the stream goes
    /// on.
    fn parse_vbr_tags(&mut self, off: u64) -> io::Result<u64> {
        let Some(h) = self.header_at(off)? else {
            return Ok(off);
        };
        if h.layer != 3 {
            return Ok(off);
        }
        let spf = if h.lsf { 576u64 } else { 1152 };
        // The frame, and the bytes after it the tag's fields may run into
        // (FFmpeg reads on, whatever the frame's size): 4 for the header,
        // 32 for the side information at most, then the tag's 156.
        let frame = self.read_bytes(off, 4 + 32 + 156)?;
        let at = |i: usize| frame.get(i).copied().unwrap_or(0);
        let rb32 = |i: usize| u32::from_be_bytes([at(i), at(i + 1), at(i + 2), at(i + 3)]);
        let mut frames = 0u32;
        let mut bytes = 0u32;
        // Xing or Info, after the side information: 32 or 17 bytes in
        // MPEG-1, 17 or 9 in MPEG-2 and -2.5, stereo or mono.
        let mut p = 4 + match (h.lsf, h.channels == 1) {
            (false, false) => 32,
            (false, true) | (true, false) => 17,
            (true, true) => 9,
        };
        let magic = rb32(p);
        p += 4;
        if magic == u32::from_be_bytes(*b"Xing") || magic == u32::from_be_bytes(*b"Info") {
            let flags = rb32(p);
            p += 4;
            if flags & 1 != 0 {
                frames = rb32(p);
                p += 4;
            }
            if flags & 2 != 0 {
                bytes = rb32(p);
                p += 4;
            }
            let rest = self.size.saturating_sub(off + 4);
            if rest > 0 && bytes > 0 {
                let (lo, hi) = (rest.min(u64::from(bytes)), rest.max(u64::from(bytes)));
                if rest > u64::from(bytes) && hi - lo > lo >> 4 {
                    // "invalid concatenated file": the count is the first
                    // file's.
                    frames = 0;
                }
            }
            if flags & 4 != 0 {
                p += 100;
            }
            if flags & 8 != 0 {
                p += 4;
            }
            let version = [at(p), at(p + 1), at(p + 2), at(p + 3)];
            // The version (9), revision and lowpass (2), peak (4), two gains
            // (4), flags and bit rate (2): then the delays, 12 bits each.
            p += 9 + 2 + 4 + 4 + 2;
            let delays =
                (u32::from(at(p)) << 16) | (u32::from(at(p + 1)) << 8) | u32::from(at(p + 2));
            if [*b"LAME", *b"Lavf", *b"Lavc"].contains(&version) {
                let (start_pad, end_pad) = (delays >> 12, delays & 4095);
                self.info.encoder = Some((start_pad, end_pad));
                self.info.start_skip = u64::from(start_pad) + DECODER_DELAY;
                if frames > 0 {
                    let last = u64::from(frames) * spf;
                    let first = (last + DECODER_DELAY).saturating_sub(u64::from(end_pad));
                    self.info.discard = Some((first, last));
                }
            }
        }
        // VBRI: 32 bytes after the header, whatever the mode.
        if rb32(4 + 32) == u32::from_be_bytes(*b"VBRI") && u16::from_be_bytes([at(40), at(41)]) == 1
        {
            bytes = rb32(4 + 32 + 10);
            frames = rb32(4 + 32 + 14);
        }
        if frames == 0 && bytes == 0 {
            return Ok(off);
        }
        if frames > 0 {
            self.info.frames = Some(frames);
            self.info.duration = Some(u64::from(frames) * spf * h.ticks_per_sample());
        }
        if bytes > 0 {
            self.info.bytes = Some(bytes);
        }
        Ok(off + h.frame_size as u64)
    }

    /// A free-format stream starting at `off`: its frame size, padding
    /// aside, found as minimp3 finds it (`mp3d_find_frame`): the first
    /// distance at which a header of the stream follows, with another
    /// after that frame -- and here a third, so that a look-alike in the
    /// first frame's data is not taken for the second.
    fn free_format_at(&mut self, off: u64) -> io::Result<Option<usize>> {
        let Some(h) = self.header_at(off)? else {
            return Ok(None);
        };
        if h.frame_size != 0 {
            return Ok(None);
        }
        let data = self.read_bytes(off, 3 * MAX_FREE_FORMAT_FRAME_SIZE + 16)?;
        let same = |at: usize| -> Option<Header> {
            let w = u32::from_be_bytes(data.get(at..at + 4)?.try_into().ok()?);
            Header::parse(w)
                .filter(|o| o.frame_size == 0 && w & FREE_FORMAT_MASK == h.raw & FREE_FORMAT_MASK)
        };
        for k in 4..MAX_FREE_FORMAT_FRAME_SIZE {
            if 2 * k >= data.len().saturating_sub(4) {
                break;
            }
            let Some(second) = same(k) else {
                continue;
            };
            let Some(size) = k.checked_sub(h.padding()).filter(|&s| s > 0) else {
                continue;
            };
            let third = k + size + second.padding();
            if let Some(t) = same(third)
                && same(third + size + t.padding()).is_some()
            {
                return Ok(Some(size));
            }
        }
        Ok(None)
    }

    /// `mp3_read_header`'s search: the first position within 64 KiB of
    /// `off` with a frame whose next frame agrees with it (`MP3_MASK`) --
    /// `None` if there is none, where FFmpeg goes on from `off`.
    fn skip_junk(&mut self, off: u64) -> io::Result<Option<u64>> {
        let window = self.read_bytes(off, usize::try_from(JUNK_SEARCH + 4).unwrap_or(0))?;
        let word = |i: usize| -> Option<u32> {
            Some(u32::from_be_bytes(window.get(i..i + 4)?.try_into().ok()?))
        };
        for i in 0..JUNK_SEARCH as usize {
            let Some(h) = word(i).and_then(Header::parse).filter(|h| h.frame_size > 0) else {
                continue;
            };
            let next = i + h.frame_size;
            // The next header, from the window or past it.
            let second = match word(next) {
                Some(w) => Some(w),
                None => self
                    .read_array::<4>(off + next as u64)?
                    .map(u32::from_be_bytes),
            };
            match second.and_then(|w| Header::parse(w).filter(|h2| h2.frame_size > 0)) {
                Some(h2) if (h.raw & MP3_MASK) == (h2.raw & MP3_MASK) => {
                    return Ok(Some(off + i as u64));
                }
                // A frame that ends the file: FFmpeg's check reads past the
                // end and refuses the file; here, the frame is the stream.
                _ if second.is_none() && off + next as u64 == self.size => {
                    return Ok(Some(off + i as u64));
                }
                _ => {}
            }
        }
        Ok(None)
    }

    /// The next packet; `None` at the end of the file.
    ///
    /// # Errors
    ///
    /// When the source fails.
    pub fn next_packet(&mut self) -> io::Result<Option<Packet>> {
        loop {
            // The stream's first packets wait for the parser's first count of
            // a frame's samples, which they all take.
            if let Some(p) = self.queue.front()
                && (!self.at_start || p.duration > 0 || self.ended)
            {
                self.at_start = false;
                return Ok(self.queue.pop_front().map(|p| self.finish(p)));
            }
            if self.ended && self.queue.is_empty() {
                return Ok(None);
            }
            self.parse_more()?;
        }
    }

    /// The trims FFmpeg's demuxer gives a packet as side data, and its place
    /// in the index.
    fn finish(&mut self, mut p: Packet) -> Packet {
        let ticks = self.info.first.map_or(1, |h| h.ticks_per_sample().max(1)) as i64;
        // `ts_to_samples`: `av_rescale`, to the nearest.
        let samples = |t: i64| (t + ticks / 2).div_euclid(ticks);
        if let Some((first, last)) = self.info.discard {
            let sample = samples(p.pts);
            let duration = samples(p.duration);
            let end = sample + duration;
            if duration > 0 && end >= first as i64 && sample < last as i64 {
                p.discard_padding = u64::try_from((end - first as i64).min(duration)).unwrap_or(0);
            }
        }
        if self.info.start_skip > 0 && p.pts == 0 {
            p.skip_samples = self.info.start_skip;
        }
        if self.index.last().is_none_or(|&(at, _)| at < p.position) {
            self.index.push((p.position, p.pts));
        }
        p
    }

    /// One more read given to the parser, its packets timed and queued; at
    /// the end of the file, the parser's last packet.
    fn parse_more(&mut self) -> io::Result<()> {
        if self.chunk_used >= self.chunk.len() {
            if self.ended {
                return Ok(());
            }
            // `mp3_read_packet`: 1024 bytes, fewer at the end.
            let len = if self.size > 128 && self.read_position < self.size {
                READ_SIZE.min(usize::try_from(self.size - self.read_position).unwrap_or(READ_SIZE))
            } else {
                READ_SIZE
            };
            self.chunk = self.read_bytes(self.read_position, len)?;
            self.chunk_position = self.read_position;
            self.chunk_used = 0;
            self.read_position += self.chunk.len() as u64;
            if self.chunk.is_empty() {
                self.ended = true;
                if let Some((data, position)) = self.parser.flush() {
                    self.push(data, position, None);
                }
                return Ok(());
            }
        }
        while self.chunk_used < self.chunk.len() {
            let buf = self.chunk.get(self.chunk_used..).unwrap_or_default();
            let (used, packet) = self
                .parser
                .parse(buf, self.chunk_position + self.chunk_used as u64);
            self.chunk_used += used.max(1).min(buf.len());
            if let Some(Parsed {
                data,
                position,
                frame,
            }) = packet
            {
                self.push(data, position, frame);
                return Ok(());
            }
        }
        Ok(())
    }

    /// A packet the parser gave, timed: on from the last by the parser's
    /// count of samples at its rate (`compute_pkt_fields`).
    fn push(&mut self, data: Vec<u8>, position: u64, frame: Option<usize>) {
        let rate = u64::from(self.parser.sample_rate);
        let duration = if rate > 0 {
            i64::try_from(self.parser.duration * (TICKS_PER_SECOND / rate)).unwrap_or(0)
        } else {
            0
        };
        // The frame must fill the packet's end: what is left at the end of
        // the file may hold a frame's start without its end.
        let frame = frame.filter(|&at| {
            data.get(at..at + 4)
                .and_then(|b| self.parser.header(u32::from_be_bytes(b.try_into().ok()?)))
                .is_some_and(|h| at + h.frame_size == data.len())
        });
        if self.at_start && duration > 0 {
            // `update_initial_durations`: the stream's first packets, counted
            // before the parser counted a frame's samples, last as long as the
            // first it counts, and are timed again from 0.
            let mut pts = 0i64;
            for p in &mut self.queue {
                p.pts = pts;
                if p.duration == 0 {
                    p.duration = duration;
                }
                pts += p.duration;
            }
            self.next_pts = pts;
        }
        let pts = self.next_pts;
        self.next_pts += duration;
        self.queue.push_back(Packet {
            data,
            frame,
            position,
            pts,
            duration,
            ..Packet::default()
        });
    }

    /// Goes to the last packet at or before `ticks` -- reading on as far as
    /// that first --, and from there back far enough that the frames after
    /// it decode as they would have: past the 511 bytes of main data a
    /// Layer III frame may take from those before it, and two more for the
    /// filterbank's overlap. The next packet is that one.
    ///
    /// # Errors
    ///
    /// When the source fails.
    pub fn seek(&mut self, ticks: i64) -> io::Result<()> {
        while self.index.last().is_none_or(|&(_, pts)| pts <= ticks)
            && !(self.ended && self.queue.is_empty())
        {
            if self.next_packet()?.is_none() {
                break;
            }
        }
        let found = self
            .index
            .partition_point(|&(_, pts)| pts <= ticks)
            .saturating_sub(1);
        // The frame decodes as it would have when the one before it did too
        // (its spectrum gives the IMDCT's overlap and the filterbank's
        // history), and each of them when its main data is in the
        // reservoir: when the frames before the one before it hold the 511
        // bytes of main data the furthest reach back takes. Their main data
        // is counted short, past the most a header, CRC and side
        // information take (38 bytes), and one more frame is taken to spare.
        let mut at = found.saturating_sub(1);
        let mut main_data = 0u64;
        while at > 0 && main_data < 511 {
            let (start, _) = self.index.get(at - 1).copied().unwrap_or((0, 0));
            let (end, _) = self.index.get(at).copied().unwrap_or((start, 0));
            main_data += (end - start).saturating_sub(4 + 2 + 32);
            at -= 1;
        }
        at = at.saturating_sub(1);
        let (position, pts) = self
            .index
            .get(at)
            .copied()
            .unwrap_or((self.info.data_offset, 0));
        self.parser.reset();
        self.queue.clear();
        self.chunk.clear();
        self.chunk_used = 0;
        self.read_position = position;
        self.next_pts = pts;
        self.ended = false;
        self.at_start = position == self.info.data_offset;
        Ok(())
    }
}
