//! Ogg pages (RFC 3533 §6) and the reader that finds them in a file.
//!
//! A page is a 27-byte header -- the capture pattern `OggS`, version 0, its
//! flags, the granule position, the stream's serial number, the page's
//! sequence number, a CRC and the segment count -- then a lacing value a
//! segment, then the segments. A packet is a run of segments ending in one
//! shorter than 255 bytes, which may run on across pages.
//!
//! [`Pages`] reads them through a buffer, from any position: it looks for
//! the capture pattern and takes the first page whose CRC is right, passing
//! over whatever is not one -- damage, or a pattern that happens to occur in
//! a packet's bytes -- as FFmpeg's and libogg's readers pass over it.

#![allow(
    clippy::arithmetic_side_effects,
    reason = "offsets within a buffer of at most a few hundred KiB, and file positions bounded by the file's length"
)]

use std::io::{self, Read, Seek, SeekFrom};

/// The page's first packet continues one from the page before.
pub(crate) const CONTINUED: u8 = 1;
/// The stream's first page.
pub(crate) const FIRST: u8 = 2;
/// The stream's last page.
pub(crate) const LAST: u8 = 4;

/// The largest page there can be: the header, 255 lacing values, and 255
/// segments of 255 bytes.
pub(crate) const MAX_PAGE: usize = 27 + 255 + 255 * 255;

/// How much is read from the source at a time: two pages' worth at least,
/// so that a page found near a chunk's end is still whole in the buffer
/// after a refill from its start.
const CHUNK: usize = 2 * MAX_PAGE;

/// A page, as read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Page {
    /// Where it begins in the file.
    pub position: u64,
    pub flags: u8,
    /// -1 where no packet ends on the page.
    pub granule: i64,
    pub serial: u32,
    pub sequence: u32,
    /// Each segment's length, in order.
    pub lacing: Vec<u8>,
    /// The segments, one after another.
    pub body: Vec<u8>,
}

impl Page {
    /// Where the body begins, in the file.
    pub(crate) fn body_position(&self) -> u64 {
        self.position + 27 + self.lacing.len() as u64
    }

    /// The page's length, header and all.
    pub(crate) fn len(&self) -> u64 {
        27 + self.lacing.len() as u64 + self.body.len() as u64
    }

    /// Where the next page begins.
    pub(crate) fn end(&self) -> u64 {
        self.position + self.len()
    }

    pub(crate) fn has(&self, flag: u8) -> bool {
        self.flags & flag != 0
    }

    /// Whether a packet ends on it: a segment shorter than 255 bytes.
    pub(crate) fn ends_a_packet(&self) -> bool {
        self.lacing.iter().any(|&l| l < 255)
    }

    /// The page parsed from `bytes`, which begin with its capture pattern;
    /// `None` if it is not one (a version other than 0, a CRC that does not
    /// match), or if `bytes` end before it does. `position` is where `bytes`
    /// begin in the file.
    pub(crate) fn parse(bytes: &[u8], position: u64) -> Option<Self> {
        let header = bytes.get(..27)?;
        if header.get(..4)? != b"OggS" || *header.get(4)? != 0 {
            return None;
        }
        let segments = usize::from(*header.get(26)?);
        let lacing = bytes.get(27..27 + segments)?;
        let size: usize = lacing.iter().map(|&l| usize::from(l)).sum();
        let whole = bytes.get(..27 + segments + size)?;
        let field = |at: usize| -> Option<[u8; 4]> { header.get(at..at + 4)?.try_into().ok() };
        if crc(whole) != u32::from_le_bytes(field(22)?) {
            return None;
        }
        Some(Self {
            position,
            flags: *header.get(5)?,
            granule: i64::from_le_bytes(header.get(6..14)?.try_into().ok()?),
            serial: u32::from_le_bytes(field(14)?),
            sequence: u32::from_le_bytes(field(18)?),
            lacing: lacing.to_vec(),
            body: whole.get(27 + segments..)?.to_vec(),
        })
    }
}

/// The page CRC (RFC 3533 §6): CRC-32 with the polynomial 0x04c11db7, MSB
/// first, initial value 0 and no final xor, over the page with its CRC field
/// taken as zero.
#[allow(clippy::indexing_slicing, reason = "a byte indexes a table of 256")]
pub(crate) fn crc(page: &[u8]) -> u32 {
    let mut c = 0u32;
    for (i, &b) in page.iter().enumerate() {
        let b = if (22..26).contains(&i) { 0 } else { b };
        c = (c << 8) ^ TABLE[usize::from((c >> 24) as u8 ^ b)];
    }
    c
}

#[allow(
    clippy::indexing_slicing,
    reason = "i counts to 256, the table's length"
)]
static TABLE: [u32; 256] = {
    let mut t = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut r = (i as u32) << 24;
        let mut k = 0;
        while k < 8 {
            r = if r & 0x8000_0000 != 0 {
                (r << 1) ^ 0x04c1_1db7
            } else {
                r << 1
            };
            k += 1;
        }
        t[i] = r;
        i += 1;
    }
    t
};

/// A file's pages, read through a buffer.
pub(crate) struct Pages<R> {
    source: R,
    /// The file's length.
    len: u64,
    buf: Vec<u8>,
    /// Where `buf` begins in the file.
    buf_at: u64,
}

impl<R: Read + Seek> Pages<R> {
    pub(crate) fn new(mut source: R) -> io::Result<Self> {
        let len = source.seek(SeekFrom::End(0))?;
        Ok(Self {
            source,
            len,
            buf: Vec::new(),
            buf_at: 0,
        })
    }

    /// The file's length.
    pub(crate) fn len(&self) -> u64 {
        self.len
    }

    /// The file's bytes from `at`, as many as the buffer holds -- at least
    /// `want` of them where the file has that many, and fewer only at its
    /// end.
    fn bytes(&mut self, at: u64, want: usize) -> io::Result<&[u8]> {
        let end = self.buf_at + self.buf.len() as u64;
        let wanted_end = at.saturating_add(want as u64).min(self.len);
        if at < self.buf_at || wanted_end > end {
            let n = usize::try_from((self.len.saturating_sub(at)).min(CHUNK.max(want) as u64))
                .unwrap_or(CHUNK);
            self.buf.resize(n, 0);
            self.source.seek(SeekFrom::Start(at))?;
            self.source.read_exact(&mut self.buf)?;
            self.buf_at = at;
        }
        let from = usize::try_from(at - self.buf_at).unwrap_or(usize::MAX);
        Ok(self.buf.get(from..).unwrap_or_default())
    }

    /// The first page at or after `at`, read whole and its CRC checked;
    /// `None` when there is none before the file's end.
    pub(crate) fn next_page(&mut self, mut at: u64) -> io::Result<Option<Page>> {
        while at < self.len {
            let window = self.bytes(at, 4)?;
            let Some(start) = window.windows(4).position(|w| w == b"OggS") else {
                // On past what was looked at, but its last three bytes, which
                // may begin a pattern the next read completes.
                at += (window.len().saturating_sub(3)).max(1) as u64;
                continue;
            };
            let position = at + start as u64;
            let bytes = self.bytes(position, MAX_PAGE)?;
            match Page::parse(bytes, position) {
                Some(page) => return Ok(Some(page)),
                None => at = position + 1,
            }
        }
        Ok(None)
    }

    /// The last page that begins in `[from, before)`; `None` if there is
    /// none. Read backward, a chunk at a time, so that the end of a long
    /// file costs a read or two.
    pub(crate) fn last_page(&mut self, from: u64, before: u64) -> io::Result<Option<Page>> {
        let mut hi = before.min(self.len);
        while hi > from {
            let lo = hi.saturating_sub(CHUNK as u64).max(from);
            let mut at = lo;
            let mut last = None;
            while let Some(page) = self.next_page(at)? {
                if page.position >= hi {
                    break;
                }
                at = page.end();
                last = Some(page);
            }
            if last.is_some() {
                return Ok(last);
            }
            hi = lo;
        }
        Ok(None)
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    reason = "a test: a failure should be loud"
)]
pub(crate) mod tests {
    use super::*;

    /// A page of `segments`, its CRC filled in: each segment as one lacing
    /// value, so each shorter than 255 bytes ends a packet.
    pub(crate) fn page(
        flags: u8,
        granule: i64,
        serial: u32,
        sequence: u32,
        segments: &[&[u8]],
    ) -> Vec<u8> {
        let mut p = b"OggS".to_vec();
        p.push(0);
        p.push(flags);
        p.extend_from_slice(&granule.to_le_bytes());
        p.extend_from_slice(&serial.to_le_bytes());
        p.extend_from_slice(&sequence.to_le_bytes());
        p.extend_from_slice(&[0; 4]);
        p.push(segments.len() as u8);
        p.extend(segments.iter().map(|s| s.len() as u8));
        for s in segments {
            p.extend_from_slice(s);
        }
        let c = crc(&p);
        p[22..26].copy_from_slice(&c.to_le_bytes());
        p
    }

    #[test]
    fn a_page_reads_back_and_a_flipped_bit_fails_its_crc() {
        let bytes = page(FIRST, 7, 0x1234, 0, &[b"hello", b""]);
        let p = Page::parse(&bytes, 100).unwrap();
        assert_eq!(
            (p.flags, p.granule, p.serial, p.sequence),
            (FIRST, 7, 0x1234, 0)
        );
        assert_eq!(p.lacing, vec![5, 0]);
        assert_eq!(p.body, b"hello");
        assert_eq!(p.body_position(), 100 + 27 + 2);
        assert_eq!(p.len(), bytes.len() as u64);
        assert_eq!(p.end(), 100 + bytes.len() as u64);
        for i in 0..bytes.len() {
            let mut bad = bytes.clone();
            bad[i] ^= 0x10;
            assert!(
                Page::parse(&bad, 0).is_none_or(|q| q != p),
                "a flip at {i} unnoticed"
            );
        }
        assert!(Page::parse(&bytes[..bytes.len() - 1], 0).is_none());
    }

    #[test]
    fn the_crc_is_ogg_s() {
        assert_eq!(crc(&[]), 0);
        assert_eq!(crc(b"\x01"), 0x04c1_1db7);
        // libogg's CRC of a page it wrote (`ogg_page_checksum_set`): an
        // empty EOS page of stream 0, sequence 1.
        let p = page(LAST, 0, 0, 1, &[]);
        assert_eq!(&p[22..26], &crc(&p).to_le_bytes());
    }

    #[test]
    fn pages_are_found_past_damage_and_from_the_end() {
        let a = page(FIRST, 0, 1, 0, &[b"one"]);
        let b = page(0, 5, 1, 1, &[b"two", b"three"]);
        let c = page(LAST, 9, 1, 2, &[b"four"]);
        let mut bad = page(0, 7, 1, 9, &[b"lost"]);
        bad[30] ^= 1;
        let mut file = a.clone();
        file.extend_from_slice(b"junk OggS junk");
        file.extend_from_slice(&b);
        file.extend_from_slice(&bad);
        file.extend_from_slice(&c);
        let mut pages = Pages::new(std::io::Cursor::new(file.clone())).unwrap();
        let mut seen = Vec::new();
        let mut at = 0;
        while let Some(p) = pages.next_page(at).unwrap() {
            seen.push((p.position, p.sequence));
            at = p.end();
        }
        let b_at = (a.len() + 14) as u64;
        let c_at = b_at + (b.len() + bad.len()) as u64;
        assert_eq!(seen, vec![(0, 0), (b_at, 1), (c_at, 2)]);
        let last = pages.last_page(0, file.len() as u64).unwrap().unwrap();
        assert_eq!(last.position, c_at);
        let before = pages.last_page(0, c_at).unwrap().unwrap();
        assert_eq!(before.position, b_at);
        assert!(pages.last_page(1, b_at).unwrap().is_none());
    }
}
