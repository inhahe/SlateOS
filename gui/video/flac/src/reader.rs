//! A native FLAC file, read as libFLAC 1.5.0's stream decoder reads one --
//! `find_metadata_`, `read_metadata_`, `frame_sync_`, the missing-frame
//! silence of `read_frame_`, and the MD5 check of `write_audio_frame_to_client_`
//! and `FLAC__stream_decoder_finish`, translated into Rust (copyright Josh
//! Coalson and Xiph.Org, BSD: `licenses/flac-COPYING`).
//!
//! **Finding the stream.** The file's `fLaC` marker, past any ID3v2 tags
//! before it; or, where a frame's sync code comes first, that frame -- a
//! stream with no metadata at all, as a file cut from the middle of another
//! is.
//!
//! **Damage** is met as libFLAC meets it, so that what plays is libFLAC's to
//! the sample: a frame that does not decode is passed over and the search
//! for the next resumes where libFLAC's does (where a damaged header's
//! reading stopped, or the frame's fourth byte); frames missing between two
//! that decoded are filled with silence, up to five seconds or fifty
//! blocks; a metadata block that breaks its rules ends the metadata. Each
//! is counted ([`Reader::errors`]).
//!
//! **Seeking** finds the frame holding the sample by bisection on the frame
//! headers' sample numbers and decodes from it, its samples before the one
//! sought dropped: the samples are those a read from the start gives, from
//! that one on.

#![allow(
    clippy::arithmetic_side_effects,
    reason = "file positions bounded by the file's length; sample counts by 36-bit sample numbers"
)]

use std::collections::VecDeque;
use std::io::{self, Read, Seek, SeekFrom};

use crate::Error;
use crate::frame::{Decoded, FrameDecoder, Header, Status};
use crate::metadata::{Metadata, parse_block};

/// How much is read from the source at a time.
const CHUNK: usize = 1 << 16;

/// What the reader met, in the order libFLAC's callbacks would have been
/// told it: each metadata block it read, and each error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    /// A metadata block read: its type and its stated length.
    Metadata {
        kind: u8,
        length: u32,
    },
    Error(Status),
}

/// One frame's samples, as the reader gives them.
#[derive(Debug)]
pub struct Frame<'a> {
    pub header: Header,
    /// Each channel's samples, `header.block_size` of them.
    pub channels: Vec<&'a [i32]>,
}

/// What the reader is doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    /// Looking for a frame's sync code.
    Search,
    /// At a sync code.
    Frame,
    End,
}

/// A native FLAC file being read.
pub struct Reader<R> {
    source: R,
    /// The file's bytes from `buf_at`.
    buf: Vec<u8>,
    buf_at: u64,
    /// Whether the source has nothing past `buf`.
    eof: bool,
    /// The next byte to read.
    pos: u64,
    state: State,
    metadata: Metadata,
    /// Where the first frame starts.
    first_frame: u64,
    decoder: FrameDecoder,
    /// The last frame given, real or silence (`last_frame`).
    last: Option<Header>,
    /// Whether libFLAC would have sent an error since the last frame
    /// attempt ended (`error_has_been_sent`).
    error_sent: bool,
    log: Vec<Event>,
    /// Silent frames to give before the frame decoded.
    silence: VecDeque<Header>,
    zeros: Vec<i32>,
    /// A frame decoded, to give after the silence.
    pending: Option<Header>,
    /// After a seek: the sample the next frame given starts at.
    target: Option<u64>,
    md5: Option<md5::Md5>,
    md5_bytes: Vec<u8>,
    /// Whether every frame since the start was given whole and in order.
    md5_whole: bool,
}

impl<R: Read + Seek> Reader<R> {
    /// Opens `source`: its metadata, read up to the first frame.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] when the source fails; [`Error::NotFlac`] when it ends
    /// before a `fLaC` marker or a frame.
    pub fn open(mut source: R) -> Result<Self, Error> {
        source.seek(SeekFrom::Start(0))?;
        let mut r = Self {
            source,
            buf: Vec::new(),
            buf_at: 0,
            eof: false,
            pos: 0,
            state: State::Search,
            metadata: Metadata::default(),
            first_frame: 0,
            decoder: FrameDecoder::default(),
            last: None,
            error_sent: false,
            log: Vec::new(),
            silence: VecDeque::new(),
            zeros: Vec::new(),
            pending: None,
            target: None,
            md5: None,
            md5_bytes: Vec::new(),
            md5_whole: true,
        };
        if r.find_metadata()? {
            r.read_metadata()?;
        }
        r.first_frame = r.pos;
        r.decoder.defaults = r.metadata.stream_info.map(|s| s.defaults());
        // libFLAC checks the MD5 only where STREAMINFO gives one.
        if r.metadata.stream_info.is_some_and(|s| s.md5 != [0; 16]) {
            r.md5 = Some(md5::Md5::new());
        }
        Ok(r)
    }

    /// The stream's metadata.
    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    /// Every metadata block and error met so far, in order.
    pub fn log(&self) -> &[Event] {
        &self.log
    }

    /// Every error met so far, in order, as libFLAC's error callback would
    /// have been told them.
    pub fn errors(&self) -> impl Iterator<Item = Status> + '_ {
        self.log.iter().filter_map(|e| match e {
            Event::Error(s) => Some(*s),
            Event::Metadata { .. } => None,
        })
    }

    /// Whether the samples given match `STREAMINFO`'s MD5: `None` before
    /// the end, where the stream gives no MD5, or after a seek (the sum is
    /// of the whole stream, from its start).
    pub fn md5_matches(&self) -> Option<bool> {
        if self.state != State::End || !self.md5_whole {
            return None;
        }
        let sum = self.md5.clone()?.finalize();
        Some(Some(sum) == self.metadata.stream_info.map(|s| s.md5))
    }

    /// The next frame's samples; `None` at the end.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] when the source fails.
    pub fn next_frame(&mut self) -> Result<Option<Frame<'_>>, Error> {
        loop {
            if let Some(h) = self.silence.pop_front() {
                return Ok(Some(self.give_silence(h)));
            }
            if let Some(h) = self.pending.take() {
                if let Some((h, skip)) = self.trim(h) {
                    return Ok(Some(self.frame(h, skip)));
                }
                continue;
            }
            match self.state {
                State::End => return Ok(None),
                State::Search => self.frame_sync()?,
                State::Frame => self.read_frame()?,
            }
        }
    }

    /// The frame `h` describes, trimmed to a seek's target: its header and
    /// the samples to drop from its start; `None` where the whole of it is
    /// before the target.
    fn trim(&mut self, mut h: Header) -> Option<(Header, usize)> {
        let mut skip = 0usize;
        if let Some(target) = self.target {
            let end = h.sample_number + u64::from(h.block_size);
            if end <= target {
                return None;
            }
            skip = usize::try_from(target.saturating_sub(h.sample_number)).unwrap_or(0);
            h.block_size -= u32::try_from(skip).unwrap_or(0);
            h.sample_number += skip as u64;
            self.target = None;
        }
        Some((h, skip))
    }

    /// The frame `h` describes, from the decoder's output past `skip`.
    fn frame(&mut self, h: Header, skip: usize) -> Frame<'_> {
        let block = h.block_size as usize;
        let channels: Vec<&[i32]> = self
            .decoder
            .output
            .iter()
            .take(h.channels as usize)
            .map(|c| c.get(skip..skip + block).unwrap_or_default())
            .collect();
        if let Some(md5) = &mut self.md5 {
            accumulate(md5, &mut self.md5_bytes, &channels, h.bits_per_sample);
        }
        Frame {
            header: h,
            channels,
        }
    }

    fn give_silence(&mut self, h: Header) -> Frame<'_> {
        let block = h.block_size as usize;
        if self.zeros.len() < block {
            self.zeros.resize(block, 0);
        }
        let zeros = self.zeros.get(..block).unwrap_or_default();
        let channels = vec![zeros; h.channels as usize];
        if let Some(md5) = &mut self.md5 {
            accumulate(md5, &mut self.md5_bytes, &channels, h.bits_per_sample);
        }
        Frame {
            header: h,
            channels,
        }
    }

    /// Goes to sample `sample` (counted from the stream's start, a channel):
    /// the next frame given starts there. `false` -- and the reader where it
    /// was -- for a sample at or past the stream's end as `STREAMINFO` gives
    /// it, as libFLAC refuses one; where the end is not known, a seek past
    /// it is made, and the next frame is `None`.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] when the source fails.
    pub fn seek(&mut self, sample: u64) -> Result<bool, Error> {
        let total = self.metadata.stream_info.map_or(0, |s| s.total_samples);
        if total > 0 && sample >= total {
            return Ok(false);
        }
        self.silence.clear();
        self.pending = None;
        self.last = None;
        self.md5_whole = false;
        // The last frame starting at or before the sample, by bisection on
        // positions: a frame's header says where in the stream it starts.
        let (mut lo, mut hi) = (self.first_frame, self.file_len()?);
        let mut best = self.first_frame;
        while hi.saturating_sub(lo) > CHUNK as u64 {
            let mid = lo + (hi - lo) / 2;
            match self.probe(mid, hi)? {
                Some((at, start, _)) if start <= sample => {
                    best = at;
                    lo = at + 1;
                }
                _ => hi = mid,
            }
        }
        // Frame by frame from the best found.
        let mut at = best;
        while let Some((next, start, len)) = self.probe(at, u64::MAX)? {
            if start > sample {
                break;
            }
            best = next;
            at = next + len.max(1);
        }
        self.pos = best;
        self.state = State::Search;
        self.target = Some(sample);
        Ok(true)
    }

    /// The first frame that decodes at or after `from` and starts before
    /// `before`: where it is, its first sample and its length.
    fn probe(&mut self, from: u64, before: u64) -> Result<Option<(u64, u64, u64)>, Error> {
        let mut at = from;
        let mut decoder = self.decoder.clone();
        loop {
            let Some(sync) = self.find_sync(at)? else {
                return Ok(None);
            };
            if sync >= before {
                return Ok(None);
            }
            match self.decode_at(&mut decoder, sync)? {
                Decoded::Frame { header, len } => {
                    return Ok(Some((sync, header.sample_number, len as u64)));
                }
                _ => at = sync + 1,
            }
        }
    }

    // --- Metadata.

    /// `find_metadata_`: the `fLaC` marker (`true`), past ID3v2 tags; or a
    /// frame's sync code first (`false`, the reader then at it).
    fn find_metadata(&mut self) -> Result<bool, Error> {
        const MARKER: &[u8; 4] = b"fLaC";
        const ID3: &[u8; 3] = b"ID3";
        let (mut i, mut id) = (0usize, 0usize);
        let mut first = true;
        while i < 4 {
            let Some(x) = self.byte()? else {
                return Err(Error::NotFlac);
            };
            if MARKER.get(i) == Some(&x) {
                first = true;
                i += 1;
                id = 0;
                continue;
            }
            if id >= 3 {
                return Err(Error::NotFlac);
            }
            if ID3.get(id) == Some(&x) {
                id += 1;
                i = 0;
                if id == 3 {
                    self.skip_id3v2()?;
                }
                continue;
            }
            id = 0;
            if x == 0xff {
                match self.byte()? {
                    None => return Err(Error::NotFlac),
                    // Another 0xff: it may begin the code.
                    Some(0xff) => self.pos -= 1,
                    Some(y) if y >> 1 == 0x7c => {
                        self.pos -= 2;
                        self.state = State::Frame;
                        return Ok(false);
                    }
                    Some(_) => {}
                }
            }
            i = 0;
            if first {
                self.error(Status::LostSync);
                first = false;
            }
        }
        Ok(true)
    }

    /// `skip_id3v2_tag_`: past its version and flags, its size in four
    /// seven-bit bytes, and that many bytes.
    fn skip_id3v2(&mut self) -> Result<(), Error> {
        for _ in 0..3 {
            self.byte()?.ok_or(Error::NotFlac)?;
        }
        let mut skip = 0u64;
        for _ in 0..4 {
            let x = self.byte()?.ok_or(Error::NotFlac)?;
            skip = (skip << 7) | u64::from(x & 0x7f);
        }
        self.pos += skip;
        Ok(())
    }

    /// `read_metadata_`, block by block to the last.
    fn read_metadata(&mut self) -> Result<(), Error> {
        loop {
            let mut head = [0u8; 4];
            for h in &mut head {
                *h = self.byte()?.ok_or(Error::NotFlac)?;
            }
            let last = head[0] & 0x80 != 0;
            let kind = head[0] & 0x7f;
            let len = usize::from(head[1]) << 16 | usize::from(head[2]) << 8 | usize::from(head[3]);
            let start = self.pos;
            // A STREAMINFO is read whole, its 34 bytes, whatever its length
            // says.
            let want = if kind == 0 { len.max(34) } else { len };
            let body = self.bytes(start, want)?;
            if body.len() < want {
                // The file ends inside the block.
                self.pos += body.len() as u64;
                self.state = State::Search;
                return Ok(());
            }
            let body = body.to_vec();
            if kind == 0 && len < 34 {
                // libFLAC stops here; its client, reading on, searches for
                // a frame past the 34 bytes.
                self.pos = start + 34;
                self.error(Status::BadMetadata);
                self.state = State::Search;
                return Ok(());
            }
            match parse_block(kind, body.get(..len).unwrap_or_default()) {
                Ok(block) => {
                    self.metadata.add(block);
                    self.log.push(Event::Metadata {
                        kind,
                        length: u32::try_from(len).unwrap_or(u32::MAX),
                    });
                    self.pos = start + len as u64;
                }
                Err(stopped) => {
                    self.error(Status::BadMetadata);
                    self.pos = start + stopped as u64;
                    self.state = State::Search;
                    return Ok(());
                }
            }
            if last {
                self.state = State::Search;
                return Ok(());
            }
        }
    }

    // --- Frames.

    /// `frame_sync_`: the next sync code from the reader's position; an
    /// error where bytes are passed over to reach it.
    fn frame_sync(&mut self) -> Result<(), Error> {
        let from = self.pos;
        match self.find_sync(from)? {
            Some(at) => {
                if at > from {
                    self.error(Status::LostSync);
                }
                self.pos = at;
                self.state = State::Frame;
            }
            None => {
                // libFLAC says so of bytes it looks at and passes over before
                // the end -- all but a last 0xff, whose partner it never reads.
                let rest = self.bytes(from, 2)?;
                if !rest.is_empty() && rest != [0xff] {
                    self.error(Status::LostSync);
                }
                self.state = State::End;
            }
        }
        Ok(())
    }

    /// Where the next sync code is, from `from`.
    fn find_sync(&mut self, mut from: u64) -> Result<Option<u64>, Error> {
        loop {
            let window = self.bytes(from, 2)?;
            if window.len() < 2 {
                return Ok(None);
            }
            match window
                .windows(2)
                .position(|w| matches!(w, [0xff, b] if b >> 1 == 0x7c))
            {
                Some(i) => return Ok(Some(from + i as u64)),
                None => from += window.len() as u64 - 1,
            }
        }
    }

    /// The frame at `at`, decoded by `decoder`, reading on as it needs:
    /// `NeedMore` only where the file ends first.
    fn decode_at(&mut self, decoder: &mut FrameDecoder, at: u64) -> Result<Decoded, Error> {
        let mut want = CHUNK;
        loop {
            let bytes = self.bytes(at, want)?;
            let short = bytes.len() < want;
            match decoder.decode(bytes) {
                Decoded::NeedMore { .. } if !short => want *= 2,
                done => return Ok(done),
            }
        }
    }

    /// `read_frame_`: the frame at the reader's position, and the silence
    /// for frames missing before it.
    fn read_frame(&mut self) -> Result<(), Error> {
        let at = self.pos;
        let mut decoder = std::mem::take(&mut self.decoder);
        let decoded = self.decode_at(&mut decoder, at);
        self.decoder = decoder;
        self.state = State::Search;
        match decoded? {
            Decoded::Frame { header, len } => {
                self.pos = at + len as u64;
                self.missing_frames(&header);
                self.error_sent = false;
                self.last = Some(header);
                self.pending = Some(header);
            }
            Decoded::Error {
                status,
                resume,
                past_header,
            } => {
                self.error(status);
                self.pos = at + resume as u64;
                // Past the header, libFLAC's flag starts again for the
                // next frame.
                if past_header {
                    self.error_sent = false;
                }
            }
            // The file ends inside the header: the stream's end.
            Decoded::NeedMore { in_header: true } => self.state = State::End,
            // Inside the body: the frame is given up, with no error said
            // (libFLAC's end of stream is no error), and the search goes on
            // from its fourth byte through what is left.
            Decoded::NeedMore { in_header: false } => {
                self.pos = at + 3;
                self.error_sent = false;
            }
        }
        Ok(())
    }

    /// Frames missing between the last and `h`: silence, as libFLAC adds it.
    fn missing_frames(&mut self, h: &Header) {
        let Some(last) = self.last else { return };
        let expected = last.sample_number + u64::from(last.block_size);
        if expected >= h.sample_number {
            return;
        }
        let mut needed = h.sample_number - expected;
        if !self.error_sent {
            self.error(Status::MissingFrame);
        }
        if last.sample_rate != h.sample_rate
            || last.channels != h.channels
            || last.bits_per_sample != h.bits_per_sample
            || last.block_size < 16
        {
            return;
        }
        // At most five seconds, or fifty blocks.
        needed = needed
            .min(5 * u64::from(last.sample_rate))
            .min(50 * u64::from(last.block_size));
        let mut silent = last;
        while needed > 0 {
            silent.sample_number += u64::from(silent.block_size);
            if needed < u64::from(silent.block_size) {
                silent.block_size = u32::try_from(needed).unwrap_or(u32::MAX);
            }
            needed -= u64::from(silent.block_size);
            self.silence.push_back(silent);
        }
    }

    fn error(&mut self, status: Status) {
        self.error_sent = true;
        self.log.push(Event::Error(status));
    }

    // --- Bytes.

    fn file_len(&mut self) -> io::Result<u64> {
        let here = self.source.stream_position()?;
        let len = self.source.seek(SeekFrom::End(0))?;
        self.source.seek(SeekFrom::Start(here))?;
        Ok(len)
    }

    /// The next byte, or `None` at the file's end.
    fn byte(&mut self) -> Result<Option<u8>, Error> {
        let at = self.pos;
        let b = self.bytes(at, 1)?.first().copied();
        if b.is_some() {
            self.pos += 1;
        }
        Ok(b)
    }

    /// At least `want` of the file's bytes from `at` (fewer at its end).
    fn bytes(&mut self, at: u64, want: usize) -> Result<&[u8], Error> {
        let end = self.buf_at + self.buf.len() as u64;
        if at < self.buf_at || at > end {
            self.buf.clear();
            self.buf_at = at;
            self.eof = false;
            self.source.seek(SeekFrom::Start(at))?;
        }
        while self.buf_at + self.buf.len() as u64 - at < want as u64 && !self.eof {
            // Drop what is behind `at` before growing.
            let behind = usize::try_from(at - self.buf_at).unwrap_or(0);
            if behind > 4 * CHUNK {
                self.buf.drain(..behind);
                self.buf_at = at;
            }
            let old = self.buf.len();
            let more = want.max(CHUNK);
            self.buf.resize(old + more, 0);
            let mut got = 0;
            while got < more {
                let n = match self.buf.get_mut(old + got..) {
                    Some(room) => self.source.read(room)?,
                    None => 0,
                };
                if n == 0 {
                    self.eof = true;
                    break;
                }
                got += n;
            }
            self.buf.truncate(old + got);
        }
        let from = usize::try_from(at - self.buf_at).unwrap_or(usize::MAX);
        Ok(self.buf.get(from..).unwrap_or_default())
    }
}

/// libFLAC's `FLAC__MD5Accumulate`: each sample little-endian in as many
/// bytes as the bit depth needs, channels interleaved.
fn accumulate(md5: &mut md5::Md5, scratch: &mut Vec<u8>, channels: &[&[i32]], bits: u32) {
    let width = (bits as usize).div_ceil(8);
    let n = channels.first().map_or(0, |c| c.len());
    scratch.clear();
    scratch.reserve(n * channels.len() * width);
    for i in 0..n {
        for c in channels {
            let s = c.get(i).copied().unwrap_or(0);
            scratch.extend(s.to_le_bytes().into_iter().take(width));
        }
    }
    md5.update(scratch);
}
