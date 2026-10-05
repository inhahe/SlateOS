//! MP4 files -- the ISO base media file format, as `.mp4`, `.m4a`, `.m4v`
//! and their fragmented kin for streaming are written -- taken apart into
//! their tracks' packets.
//!
//! An MP4 file is a sequence of *boxes*: a size, a four-letter type, and the
//! body. The media itself sits in `mdat` boxes, undivided; what divides it
//! is the `moov` box, before or after the media, which holds each track's
//! description (`trak`) and its *sample tables*: where each chunk of samples
//! begins, how many samples each chunk holds, each sample's size, how long
//! each lasts, which may start decoding, and how far each is shown after it
//! is decoded. An *edit list* then says which stretch of each track plays,
//! and from when. A fragmented file carries the tables in pieces instead,
//! one `moof` box before each run of media.
//!
//! [`Demuxer`] reads one: the tracks ([`Track`]) when opened -- every
//! table, and every fragment's -- then each packet in the order FFmpeg
//! gives them ([`Demuxer::next_packet`]), and from a time on
//! ([`Demuxer::seek`]).
//!
//! # Whose rules
//!
//! ISO/IEC 14496-12 says what a file means, and FFmpeg's demuxer
//! (`libavformat/mov.c` as of 2026-03, git `9b7439c31b`, the demuxer behind
//! Chrome, VLC and mpv) decides everything it leaves open: how an edit list
//! trims and shifts a track, which packets are kept only so that others can
//! be decoded (marked [`Packet::discard`]), what a negative composition
//! offset does to the decoding times, how the tracks' packets interleave,
//! and how damage is read. Every fixture's packets and seeks are held to
//! `ffprobe`'s.
//!
//! So is what a track says of its picture ([`Video`]): its colour (`colr`,
//! `vpcC`, a code point FFmpeg has no name for read as unspecified), its
//! pixel's shape (`pasp`, else the display matrix's stretch, else `tkhd`'s
//! size against the picture's), its display matrix (rotation and mirroring,
//! the track's after the movie's), its clean aperture (`clap`, worked out
//! in FFmpeg's rationals and its C conversions, wraps and all), and its
//! frame rate where FFmpeg's demuxer finds one. Each is held to ffprobe over
//! files written to exercise it, in a code no decoder reads so that what
//! ffprobe prints is the demuxer's word alone.
//!
//! A text track is subtitles where FFmpeg's `mov_codec_id` makes it so --
//! its handler (`subp`, `clcp`), or a data track's sample entry (`tx3g`,
//! `text`: 3GPP timed text, [`Codec::MovText`]) -- and the rest of its sample
//! entry is its setup ([`Track::config`]), as `mov_parse_stsd_subtitle`
//! keeps it: the default style, the justification and the font table.
//!
//! [`Demuxer::select_tracks`] reads one track alone, the others' samples
//! passed over unread as FFmpeg passes over a discarded stream's.
//!
//! [`probe`] tells an MP4 file from others by its first boxes, as FFmpeg's
//! probe does.
//!
//! `mutate.py` breaks the code one rule at a time -- an edit list's rule
//! skipped, a table misread, a box's size rule dropped, a picture's
//! description read another way -- and checks that the tests named for each
//! rule are the ones that notice.
//!
//! # Whose code
//!
//! FFmpeg's, translated: the index building, the edit lists, the fragments,
//! the reading order and the picture's description follow `mov.c`'s
//! functions closely, each named where it is ported, with `libavutil`'s
//! rationals beneath them. FFmpeg is LGPL version 2.1 or later, and so is
//! this crate, its licence in `licenses/`. Whether SlateOS keeps an LGPL
//! demuxer or has one written again from the specification and these tests
//! is `open-questions/F-Q7.md`.
//!
//! # A hostile file
//!
//! Errors, never a panic. Every table is bounded by the file's length
//! before anything is allocated for it, and every box by its parent. What a
//! table claims is bounded too: a file's tracks index at most one sample a
//! byte of it between them, as every sample of a real file is at least a
//! byte of it (design-decisions §1364). A track whose tables claim more --
//! every sample one size and billions of them, a fragment's run of samples
//! that take none of its bytes, an edit list giving the same samples again
//! -- is held to that, and what is read of it is FFmpeg's still: the
//! samples the file holds, then its end. FFmpeg's own ceilings -- the
//! entries its allocator lets it index -- are kept exactly.
//!
//! What a file costs to read is bounded the same way. FFmpeg's lookups in
//! an index walk it an entry at a time -- in its search, and once an edit in
//! its edit lists -- and a file made for it makes each walk the index's
//! length: quadratic, to seek in or to open. Here each walk is one step,
//! from a table built the first time a walk is long, with FFmpeg's answers
//! (design-decisions §1366).

mod demux;
mod index;
mod parse;
mod rational;
mod reader;
mod track;

pub use demux::{Demuxer, Packet};
pub use track::{Audio, Codec, Colour, Track, TrackKind, Video};

/// Whether a file whose first bytes are `head` is one FFmpeg would take for
/// MP4 (or QuickTime, its parent): its probe (`mov_probe`) walks the boxes
/// at the top of the file and gives the file a score for any box type an
/// MP4 file begins with. Give it as much of the file as a caller can spare
/// to read; FFmpeg reads up to a mebibyte to decide.
///
/// FFmpeg weighs that score against every other format's, so the few files
/// it scores low -- JPEG 2000 and JPEG XL in their ISO boxes, MPEG program
/// streams packed in QuickTime -- go to another demuxer there and are taken
/// here: [`Demuxer::open`] then finds no film in them.
pub fn probe(head: &[u8]) -> bool {
    let len = u64::try_from(head.len()).unwrap_or(u64::MAX);
    let word = |at: u64, n: usize| -> Option<&[u8]> {
        let at = usize::try_from(at).ok()?;
        head.get(at..at.checked_add(n)?)
    };
    let mut offset: u64 = 0;
    loop {
        if offset.saturating_add(8) > len {
            return false;
        }
        let Some(header) = word(offset, 8) else {
            return false;
        };
        let size32 = header
            .get(..4)
            .map_or(0, |b| u32::from_be_bytes(b.try_into().unwrap_or([0; 4])));
        let mut size = i64::from(size32);
        let mut least = 8;
        if size == 1 && offset.saturating_add(16) <= len {
            let big = word(offset.saturating_add(8), 8)
                .and_then(|b| b.try_into().ok())
                .map_or(0, u64::from_be_bytes);
            size = big.cast_signed();
            least = 16;
        } else if size == 0 {
            size = i64::try_from(len.saturating_sub(offset)).unwrap_or(i64::MAX);
        }
        if size < least {
            // Not a box: FFmpeg looks again four bytes on.
            offset = offset.saturating_add(4);
            continue;
        }
        if matches!(
            header.get(4..8),
            Some(
                b"moov"
                    | b"mdat"
                    | b"pnot"
                    | b"udta"
                    | b"ftyp"
                    | b"ediw"
                    | b"wide"
                    | b"free"
                    | b"junk"
                    | b"pict"
                    | [0x82, 0x82, 0x7f, 0x7d]
                    | b"skip"
                    | b"uuid"
                    | b"prfl"
            )
        ) {
            return true;
        }
        match u64::try_from(size)
            .ok()
            .and_then(|s| offset.checked_add(s))
            .filter(|&o| o <= i64::MAX.cast_unsigned())
        {
            Some(next) => offset = next,
            None => return false,
        }
    }
}

/// Why a file could not be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// The source failed.
    Io(std::io::ErrorKind),
    /// The file ends inside something it promised.
    Truncated,
    /// The file breaks the format's rules: the words say which.
    Invalid(&'static str),
    /// The file uses something this does not read.
    Unsupported(&'static str),
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        match e.kind() {
            std::io::ErrorKind::UnexpectedEof => Self::Truncated,
            kind => Self::Io(kind),
        }
    }
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Io(kind) => write!(f, "cannot read the MP4 file: {kind}"),
            Self::Truncated => f.write_str("the MP4 file ends too soon"),
            Self::Invalid(why) => write!(f, "damaged MP4 file: {why}"),
            Self::Unsupported(why) => write!(f, "unsupported MP4 file: {why}"),
        }
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::probe;

    fn header(size: u32, kind: &[u8; 4]) -> Vec<u8> {
        let mut b = size.to_be_bytes().to_vec();
        b.extend_from_slice(kind);
        b
    }

    #[test]
    fn mp4s_first_boxes_are_known_by_their_types() {
        for kind in [
            b"ftyp", b"moov", b"mdat", b"free", b"wide", b"skip", b"uuid",
        ] {
            assert!(probe(&header(8, kind)), "{kind:?}");
        }
        // A box of a type FFmpeg does not know, then one it does.
        let mut two = header(16, b"abcd");
        two.extend_from_slice(&[0; 8]);
        two.extend_from_slice(&header(8, b"moov"));
        assert!(probe(&two));
        // A 64-bit size, and a size of 0 (to the end).
        let mut big = header(1, b"ftyp");
        big.extend_from_slice(&16u64.to_be_bytes());
        assert!(probe(&big));
        assert!(probe(&header(0, b"mdat")));
    }

    #[test]
    fn other_files_are_not_mp4() {
        assert!(!probe(b""));
        assert!(!probe(b"not a video at all"));
        assert!(!probe(&[0x1A, 0x45, 0xDF, 0xA3, 0x9F, 0x42, 0x86, 0x81]));
        // A known type past what is given is not seen.
        let mut far = header(64, b"abcd");
        far.extend_from_slice(&header(8, b"moov"));
        assert!(!probe(&far));
        // A size too small to be a box's is stepped over, four bytes at a
        // time: here onto a box header.
        let mut small = 4u32.to_be_bytes().to_vec();
        small.extend_from_slice(&header(8, b"ftyp"));
        assert!(probe(&small));
        // From a box of a type it does not know, the walk jumps by its size,
        // past a header inside it.
        let mut inside = header(4, b"xxxx");
        inside.extend_from_slice(&header(8, b"ftyp"));
        assert!(!probe(&inside));
    }
}
