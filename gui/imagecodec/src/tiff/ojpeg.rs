//! Old-style JPEG compression (6): libtiff 4.7.1's `tif_ojpeg.c`.
//!
//! The first JPEG-in-TIFF scheme (TIFF 6.0, 1992), superseded in 1995 and
//! written inconsistently by everything that wrote it: the tables in tags or
//! in a JPEG stream the `JPEGInterchangeFormat` tag points to, the entropy-
//! coded data in the strips or in that stream, a restart interval a strip or
//! none. libtiff reads them all by one scheme, which this transcribes:
//!
//! - the input is one stream -- the `JPEGInterchangeFormat` block, then every
//!   strip's bytes in order (`OJPEGReadBufferFill`);
//! - its headers are read up to the first scan, keeping each quantisation and
//!   Huffman table segment whole and the frame's and scan's fields, and where
//!   there are none the tables come from `JPEGQTables`, `JPEGDCTables` and
//!   `JPEGACTables` and the frame is made up (`OJPEGReadHeaderInfoSec`);
//! - a fresh JPEG is then written for libjpeg -- those tables, a restart
//!   interval if the strips are several, the frame, the scan -- followed by
//!   the rest of the input with restart markers put back between strips,
//!   then end-of-image markers for ever (`OJPEGWriteStream`);
//! - one libjpeg session reads a plane's strips in turn: `YCbCr` held
//!   together comes out raw -- the planes as the JPEG subsamples them, packed
//!   the way TIFF packs subsampled `YCbCr`, for the RGBA reader to convert as
//!   it converts any -- and anything else as libjpeg's scanlines.
//!
//! Before any of that, the subsampling is read from the JPEG's own frame
//! header where it has one (`OJPEGSubsamplingCorrect`): the tag is not to be
//! trusted, and what the directory reports is what the data says.
//!
//! Where the input runs out mid-scan the stream is cut there, and libjpeg is
//! told so: libtiff's source manager raises an error ("Premature end of JPEG
//! data"), as it does if libjpeg has to skip input or find a restart marker
//! out of step. So the session is strict ([`Decompress::set_strict_source`]),
//! and the decoder reads its input no earlier than libjpeg does.
//!
//! libtiff's own quirks come along: a big-endian file loses the codec's
//! post-decode step to the byte swap (`BitsPerSample` is set after the codec
//! is), so each strip after the first skips a strip's worth of the JPEG; the
//! raw rows' buffer outlives sessions, and rows the inverse transform does
//! not write -- past the JPEG's end, or a second column of tiles, which
//! libtiff's one-column frame never reaches -- show what it last held.
//!
//! Three corners of libtiff's reading are not modelled, each needing input
//! built to reach it (`known-issues.md`):
//!
//! - libtiff hands libjpeg a block 2048 bytes at a time, and libjpeg-turbo's
//!   Huffman decoder has a fast path that asks for no input while a piece
//!   still holds 512 bytes for each block of the MCU. Neither changes what
//!   is decoded, only when the source is asked for more: that shows only if
//!   the input ends hard -- no strip after the data -- with no restart
//!   interval, whose markers end the scan first, within a few bytes of the
//!   last bits the scan needs. Here the stream is one piece and the decoder
//!   libjpeg's careful path throughout.
//! - A new session starts from the plane's first data unless libtiff's input
//!   already stands at that file offset, in which case it carries on from
//!   there. The two differ only where one file range belongs to two blocks
//!   -- strips of different planes overlapping -- and the last session
//!   stopped at a piece boundary exactly there. Here a session always starts
//!   from the plane's first data.
//! - A table offset past 2^63, which only BigTIFF can hold, is a failed seek
//!   in libtiff, which then reads the table from wherever its file position
//!   was. Here there is no table and the image is refused.

use alloc::vec;
use alloc::vec::Vec;

use super::dir::Directory;
use crate::jpeg::{ColorSpace, Decompress, RawPlane};
use crate::{ImageError, ImageResult, Limits};

const SOF0: u8 = 0xC0;
const SOF1: u8 = 0xC1;
const SOF3: u8 = 0xC3;
const DHT: u8 = 0xC4;
const RST0: u8 = 0xD0;
const SOI: u8 = 0xD8;
const SOS: u8 = 0xDA;
const DQT: u8 = 0xDB;
const DRI: u8 = 0xDD;
const APP0: u8 = 0xE0;
const APP15: u8 = 0xEF;
const COM: u8 = 0xFE;

/// Where the input stream is reading (`OJPEGStateInBufferSource`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Block {
    NotSetYet,
    InterchangeFormat,
    Strile,
    Eof,
}

/// A position in the input stream: which block it is in, the next strip it
/// will move on to, and the unread part of the current block -- libtiff's
/// `in_buffer_*` fields, and what `sos_end` saves of them. (libtiff reads a
/// block in 2048-byte pieces; the whole file is to hand here, so a block is
/// a range of it.)
#[derive(Clone, Copy, Debug)]
struct Position {
    block: Block,
    next_strile: u32,
    pos: u64,
    togo: u64,
}

impl Position {
    const START: Self = Self {
        block: Block::NotSetYet,
        next_strile: 0,
        pos: 0,
        togo: 0,
    };
}

/// The input stream over a file: `JPEGInterchangeFormat`, then the strips.
struct Input<'a> {
    file: &'a [u8],
    offsets: &'a [u64],
    counts: &'a [u64],
    strile_count: u32,
    jif: u64,
    jif_len: u64,
    at: Position,
}

impl Input<'_> {
    fn file_size(&self) -> u64 {
        self.file.len() as u64
    }

    /// `OJPEGReadBufferFill`: move on to the next block with data in it;
    /// false once there is none.
    fn fill(&mut self) -> bool {
        loop {
            if self.at.togo != 0 {
                return true;
            }
            match self.at.block {
                Block::NotSetYet => {
                    if self.jif != 0 {
                        self.at.pos = self.jif;
                        self.at.togo = self.jif_len;
                    }
                    self.at.block = Block::InterchangeFormat;
                }
                Block::InterchangeFormat => self.at.block = Block::Strile,
                Block::Strile => {
                    if self.at.next_strile == self.strile_count {
                        self.at.block = Block::Eof;
                    } else {
                        // A strip array the directory lacks altogether is an
                        // error each time it is asked (`TIFFGetStrileOffsetWithErr`
                        // and `TIFFGetStrileByteCountWithErr`), and the fill
                        // fails where it stands. One the directory has is as
                        // long as there are strips.
                        let i = usize::try_from(self.at.next_strile).unwrap_or(usize::MAX);
                        let Some(&offset) = self.offsets.get(i) else {
                            return false;
                        };
                        self.at.pos = offset;
                        if offset != 0 {
                            let Some(&count) = self.counts.get(i) else {
                                return false;
                            };
                            let size = self.file_size();
                            if offset >= size {
                                self.at.pos = 0;
                            } else if count == 0 {
                                self.at.togo = size.saturating_sub(offset);
                            } else {
                                self.at.togo = count;
                                if offset.checked_add(count).is_none_or(|end| end > size) {
                                    self.at.togo = size.saturating_sub(offset);
                                }
                            }
                        }
                        self.at.next_strile = self.at.next_strile.saturating_add(1);
                    }
                }
                Block::Eof => return false,
            }
        }
    }

    /// `OJPEGReadBytePeek`.
    fn peek(&mut self) -> Option<u8> {
        if self.at.togo == 0 && !self.fill() {
            return None;
        }
        usize::try_from(self.at.pos)
            .ok()
            .and_then(|at| self.file.get(at))
            .copied()
    }

    /// `OJPEGReadByteAdvance`.
    fn advance(&mut self) {
        self.at.pos = self.at.pos.saturating_add(1);
        self.at.togo = self.at.togo.saturating_sub(1);
    }

    /// `OJPEGReadByte`.
    fn byte(&mut self) -> Option<u8> {
        let byte = self.peek()?;
        self.advance();
        Some(byte)
    }

    /// `OJPEGReadWord`.
    fn word(&mut self) -> Option<u16> {
        let high = self.byte()?;
        let low = self.byte()?;
        Some(u16::from_be_bytes([high, low]))
    }

    /// `OJPEGReadBlock`: `len` bytes, across blocks if need be.
    fn block(&mut self, len: usize) -> Option<Vec<u8>> {
        let mut out = Vec::with_capacity(len);
        while out.len() < len {
            if self.at.togo == 0 && !self.fill() {
                return None;
            }
            let want = len.saturating_sub(out.len()) as u64;
            let n = want.min(self.at.togo);
            let start = usize::try_from(self.at.pos).ok()?;
            let end = start.checked_add(usize::try_from(n).ok()?)?;
            out.extend_from_slice(self.file.get(start..end)?);
            self.at.pos = self.at.pos.saturating_add(n);
            self.at.togo = self.at.togo.saturating_sub(n);
        }
        Some(out)
    }

    /// `OJPEGReadSkip`: within the current block only; a skip past its end
    /// stops there.
    fn skip(&mut self, len: u16) {
        let n = u64::from(len).min(self.at.togo);
        self.at.pos = self.at.pos.saturating_add(n);
        self.at.togo = self.at.togo.saturating_sub(n);
    }
}

/// How the session hands a plane over (`libjpeg_jpeg_query_style`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Style {
    /// `YCbCr` held together: raw, packed as TIFF packs subsampled `YCbCr`.
    Raw,
    /// Anything else: libjpeg's scanlines, colour untouched.
    Scanlines,
}

/// The raw rows of the current iMCU row, and the packing geometry
/// (`subsampling_convert_*`). The first raw session makes it and every later
/// one reuses it, contents included, as libtiff reuses its buffer: rows the
/// inverse transform does not write keep what an earlier iMCU row -- or an
/// earlier session -- left there.
struct Convert {
    planes: Vec<RawPlane>,
    ylinelen: usize,
    clinelen: usize,
    clines: usize,
    clinelenout: usize,
    /// The chroma line of the iMCU row handed out next; 0 when a new iMCU
    /// row must be read first.
    state: usize,
}

/// A libjpeg session over one plane (`libjpeg_session_active`).
struct Session {
    jpeg: Decompress<'static, 'static>,
    style: Style,
}

/// libtiff's `OJPEGState`: the codec's tags, what its header reading found,
/// and the session.
pub(super) struct Ojpeg {
    // The tags.
    jif: u64,
    jif_len: u64,
    q_offsets: Vec<u64>,
    dc_offsets: Vec<u64>,
    ac_offsets: Vec<u64>,
    restart_interval: u16,
    // Subsampling.
    hor: u8,
    ver: u8,
    force_desubsampling: bool,
    correct_done: bool,
    // Geometry.
    image_width: u32,
    image_length: u32,
    strile_width: u32,
    strile_length: u32,
    strile_length_total: u32,
    samples_per_pixel: u8,
    plane_offset: u8,
    per_plane: u8,
    // What the headers held.
    qtable: [Option<Vec<u8>>; 4],
    dctable: [Option<Vec<u8>>; 4],
    actable: [Option<Vec<u8>>; 4],
    sof_log: bool,
    sof_marker: u8,
    sof_x: u32,
    sof_y: u32,
    sof_c: [u8; 3],
    sof_hv: [u8; 3],
    sof_tq: [u8; 3],
    sos_cs: [u8; 3],
    sos_tda: [u8; 3],
    sos_end: [Option<Position>; 3],
    header_done: bool,
    /// Header reading is in its subsampling-correction pass: note the
    /// first component's sampling and stop, saying nothing.
    correcting: bool,
    // The stream position between header reading and the session.
    at: Position,
    // The session.
    session: Option<Session>,
    /// A session was started and failed to set up: libtiff will not try
    /// again ("if a previous attempt failed, don't try again").
    session_failed: bool,
    convert: Option<Convert>,
    write_cursample: u16,
    write_curstrile: u32,
    bytes_per_line: usize,
    lines_per_strile: usize,
    decoder_ok: bool,
    error_in_raw: bool,
    limits: Limits,
}

/// The `YCbCrSubsampling` the rest of libtiff sees for an old-style JPEG
/// image (`OJPEGVGetField`): corrected from the JPEG's own frame header.
pub(super) fn subsampling_as_read(file: &[u8], dir: &mut Directory) {
    let (hor, ver) = Ojpeg::new(dir, Limits::default()).correct_subsampling(file, dir);
    dir.ycbcr_subsampling = [u16::from(hor), u16::from(ver)];
}

impl Ojpeg {
    /// `TIFFInitOJPEG`, with the directory's codec tags.
    pub(super) fn new(dir: &Directory, limits: Limits) -> Self {
        let o = &dir.ojpeg;
        Self {
            jif: o.jif,
            jif_len: o.jif_len,
            q_offsets: o.q_tables.clone(),
            dc_offsets: o.dc_tables.clone(),
            ac_offsets: o.ac_tables.clone(),
            restart_interval: o.restart_interval,
            hor: o.hor,
            ver: o.ver,
            force_desubsampling: false,
            correct_done: false,
            image_width: 0,
            image_length: 0,
            strile_width: 0,
            strile_length: 0,
            strile_length_total: 0,
            samples_per_pixel: 0,
            plane_offset: 0,
            per_plane: 0,
            qtable: Default::default(),
            dctable: Default::default(),
            actable: Default::default(),
            sof_log: false,
            sof_marker: 0,
            sof_x: 0,
            sof_y: 0,
            sof_c: [0; 3],
            sof_hv: [0; 3],
            sof_tq: [0; 3],
            sos_cs: [0; 3],
            sos_tda: [0; 3],
            sos_end: [None; 3],
            header_done: false,
            correcting: false,
            at: Position::START,
            session: None,
            session_failed: false,
            convert: None,
            write_cursample: 0,
            write_curstrile: 0,
            bytes_per_line: 0,
            lines_per_strile: 0,
            decoder_ok: false,
            error_in_raw: false,
            limits,
        }
    }

    fn input<'a>(&self, file: &'a [u8], dir: &'a Directory) -> Input<'a> {
        Input {
            file,
            offsets: &dir.strip_offsets,
            counts: &dir.strip_byte_counts,
            strile_count: dir.strips,
            jif: self.jif,
            jif_len: self.jif_len,
            at: self.at,
        }
    }

    /// `OJPEGSubsamplingCorrect`: the subsampling as the JPEG's frame
    /// header gives it, for `YCbCr` (and ITU L*a*b*) of three samples; 1x1
    /// for anything else. What the directory then reports (`OJPEGVGetField`).
    pub(super) fn correct_subsampling(&mut self, file: &[u8], dir: &Directory) -> (u8, u8) {
        if !self.correct_done {
            self.correct_done = true;
            let ycbcr = matches!(dir.photometric, Some(6 | 10));
            if dir.samples_per_pixel != 3 || !ycbcr {
                self.hor = 1;
                self.ver = 1;
                self.force_desubsampling = false;
            } else {
                self.correcting = true;
                // Whether it found anything is not asked.
                let _ = self.read_header_sec(file, dir);
                if self.force_desubsampling {
                    self.hor = 1;
                    self.ver = 1;
                }
                self.correcting = false;
            }
        }
        (self.hor, self.ver)
    }

    /// `OJPEGReadHeaderInfo`.
    fn read_header_info(&mut self, file: &[u8], dir: &Directory) -> bool {
        self.image_width = dir.width;
        self.image_length = dir.length;
        if dir.tiled {
            self.strile_width = dir.tile_width;
            self.strile_length = dir.tile_length;
            // In `uint32_t`, wrapping as it does there. (A tile length of 0
            // never gets this far: the directory has no tiles.)
            let tile = self.strile_length.max(1);
            self.strile_length_total = self
                .image_length
                .wrapping_add(tile)
                .wrapping_sub(1)
                .checked_div(tile)
                .unwrap_or(0)
                .wrapping_mul(tile);
        } else {
            self.strile_width = self.image_width;
            self.strile_length = dir.rows_per_strip;
            if self.strile_length == u32::MAX {
                self.strile_length = self.image_length;
            }
            self.strile_length_total = self.image_length;
        }
        if dir.samples_per_pixel == 1 {
            self.samples_per_pixel = 1;
            self.plane_offset = 0;
            self.per_plane = 1;
            self.hor = 1;
            self.ver = 1;
        } else {
            if dir.samples_per_pixel != 3 {
                return false;
            }
            self.samples_per_pixel = 3;
            self.plane_offset = 0;
            self.per_plane = if dir.planar_config == 1 { 3 } else { 1 };
        }
        if self.strile_length < self.image_length {
            if !matches!(self.hor, 1 | 2 | 4) || !matches!(self.ver, 1 | 2 | 4) {
                return false;
            }
            // Each factor is 1, 2 or 4 here, so neither product is 0.
            let mcu_rows = u32::from(self.ver).wrapping_mul(8);
            let mcu_cols = u32::from(self.hor).wrapping_mul(8);
            if !self.strile_length.is_multiple_of(mcu_rows) {
                return false;
            }
            // In `uint32_t`, wrapping, then cast to `uint16_t`, as there.
            let across = self
                .strile_width
                .wrapping_add(mcu_cols)
                .wrapping_sub(1)
                .checked_div(mcu_cols)
                .unwrap_or(0);
            let down = self.strile_length.checked_div(mcu_rows).unwrap_or(0);
            self.restart_interval = across.wrapping_mul(down) as u16;
        }
        if !self.read_header_sec(file, dir) {
            return false;
        }
        self.sos_end[0] = Some(self.at);
        self.header_done = true;
        true
    }

    /// `OJPEGReadHeaderInfoSec`: the headers from the start of the input to
    /// its first scan; the tables from the tags if there was no frame.
    #[allow(
        clippy::too_many_lines,
        reason = "one transcribed C function, kept whole so it can be read against libtiff's"
    )]
    fn read_header_sec(&mut self, file: &[u8], dir: &Directory) -> bool {
        let size = file.len() as u64;
        if self.jif != 0 {
            if self.jif >= size {
                self.jif = 0;
                self.jif_len = 0;
            } else if self.jif_len == 0
                || self
                    .jif
                    .checked_add(self.jif_len)
                    .is_none_or(|end| end > size)
            {
                self.jif_len = size.saturating_sub(self.jif);
            }
        }
        self.at = Position::START;
        let mut input = self.input(file, dir);
        let ok = self.read_header_markers(&mut input);
        self.at = input.at;
        if !ok {
            return false;
        }
        if self.correcting {
            return true;
        }
        if !self.sof_log {
            if !self.tables_q(file) {
                return false;
            }
            self.sof_marker = SOF0;
            for o in 0..self.samples_per_pixel {
                if let Some(slot) = self.sof_c.get_mut(usize::from(o)) {
                    *slot = o;
                }
            }
            self.sof_hv[0] = (self.hor << 4) | self.ver;
            for slot in self
                .sof_hv
                .iter_mut()
                .take(usize::from(self.samples_per_pixel))
                .skip(1)
            {
                *slot = 17;
            }
            self.sof_x = self.strile_width;
            self.sof_y = self.strile_length_total;
            self.sof_log = true;
            if !self.tables_dc(file) || !self.tables_ac(file) {
                return false;
            }
            for o in 1..self.samples_per_pixel {
                if let Some(slot) = self.sos_cs.get_mut(usize::from(o)) {
                    *slot = o;
                }
            }
        }
        true
    }

    /// The marker loop of `OJPEGReadHeaderInfoSec`.
    fn read_header_markers(&mut self, input: &mut Input<'_>) -> bool {
        loop {
            let Some(first) = input.peek() else {
                return false;
            };
            if first != 0xFF {
                // Entropy-coded data: the headers are over.
                return true;
            }
            input.advance();
            let marker = loop {
                let Some(m) = input.byte() else {
                    return false;
                };
                if m != 0xFF {
                    break m;
                }
            };
            match marker {
                SOI => {}
                COM | APP0..=APP15 => {
                    let Some(n) = input.word() else {
                        return false;
                    };
                    if n < 2 {
                        return false;
                    }
                    if n > 2 {
                        input.skip(n.saturating_sub(2));
                    }
                }
                DRI => {
                    if !self.stream_dri(input) {
                        return false;
                    }
                }
                DQT => {
                    if !self.stream_dqt(input) {
                        return false;
                    }
                }
                DHT => {
                    if !self.stream_dht(input) {
                        return false;
                    }
                }
                SOF0 | SOF1 | SOF3 => {
                    if !self.stream_sof(input, marker) {
                        return false;
                    }
                    if self.correcting {
                        return true;
                    }
                }
                SOS => {
                    if self.correcting {
                        return true;
                    }
                    return self.stream_sos(input);
                }
                _ => return false,
            }
        }
    }

    /// `OJPEGReadHeaderInfoSecStreamDri`.
    fn stream_dri(&mut self, input: &mut Input<'_>) -> bool {
        if input.word() != Some(4) {
            return false;
        }
        match input.word() {
            Some(interval) => {
                self.restart_interval = interval;
                true
            }
            None => false,
        }
    }

    /// `OJPEGReadHeaderInfoSecStreamDqt`: each table kept as a whole segment
    /// of its own, to be sent on as it is.
    fn stream_dqt(&mut self, input: &mut Input<'_>) -> bool {
        let Some(m) = input.word() else {
            return false;
        };
        if m <= 2 {
            return false;
        }
        if self.correcting {
            input.skip(m.saturating_sub(2));
            return true;
        }
        let mut m = m.saturating_sub(2);
        loop {
            if m < 65 {
                return false;
            }
            let Some(body) = input.block(65) else {
                return false;
            };
            let o = usize::from(body.first().copied().unwrap_or(0) & 15);
            if o > 3 {
                return false;
            }
            let mut segment = vec![0xFF, DQT, 0, 67];
            segment.extend_from_slice(&body);
            if let Some(slot) = self.qtable.get_mut(o) {
                *slot = Some(segment);
            }
            m = m.saturating_sub(65);
            if m == 0 {
                return true;
            }
        }
    }

    /// `OJPEGReadHeaderInfoSecStreamDht`: the segment kept whole, under the
    /// class and number of its first table.
    fn stream_dht(&mut self, input: &mut Input<'_>) -> bool {
        let Some(m) = input.word() else {
            return false;
        };
        if m <= 2 {
            return false;
        }
        if self.correcting {
            input.skip(m.saturating_sub(2));
            return true;
        }
        let Some(body) = input.block(usize::from(m.saturating_sub(2))) else {
            return false;
        };
        let o = body.first().copied().unwrap_or(0);
        let [high, low] = m.to_be_bytes();
        let mut segment = vec![0xFF, DHT, high, low];
        segment.extend_from_slice(&body);
        let slot = match o & 0xF0 {
            0 if o <= 3 => self.dctable.get_mut(usize::from(o)),
            16 if o & 15 <= 3 => self.actable.get_mut(usize::from(o & 15)),
            _ => return false,
        };
        if let Some(slot) = slot {
            *slot = Some(segment);
        }
        true
    }

    /// `OJPEGReadHeaderInfoSecStreamSof`.
    fn stream_sof(&mut self, input: &mut Input<'_>, marker: u8) -> bool {
        if self.sof_log {
            return false;
        }
        if !self.correcting {
            self.sof_marker = marker;
        }
        let Some(m) = input.word() else {
            return false;
        };
        if m < 11 {
            return false;
        }
        let m = m.saturating_sub(8);
        if m % 3 != 0 {
            return false;
        }
        let n = m / 3;
        if !self.correcting && n != u16::from(self.samples_per_pixel) {
            return false;
        }
        if input.byte() != Some(8) {
            return false;
        }
        if self.correcting {
            input.skip(4);
        } else {
            let Some(y) = input.word() else {
                return false;
            };
            let y = u32::from(y);
            if y < self.image_length && y < self.strile_length_total {
                return false;
            }
            self.sof_y = y;
            let Some(x) = input.word() else {
                return false;
            };
            let x = u32::from(x);
            if x < self.image_width && x < self.strile_width {
                return false;
            }
            if x > self.strile_width {
                return false;
            }
            self.sof_x = x;
        }
        if input.byte().map(u16::from) != Some(n) {
            return false;
        }
        for q in 0..usize::from(n) {
            let Some(c) = input.byte() else {
                return false;
            };
            if !self.correcting {
                if let Some(slot) = self.sof_c.get_mut(q) {
                    *slot = c;
                }
            }
            let Some(hv) = input.byte() else {
                return false;
            };
            if self.correcting {
                if q == 0 {
                    self.hor = hv >> 4;
                    self.ver = hv & 15;
                    if !matches!(self.hor, 1 | 2 | 4) || !matches!(self.ver, 1 | 2 | 4) {
                        self.force_desubsampling = true;
                    }
                } else if hv != 17 {
                    self.force_desubsampling = true;
                }
            } else {
                if let Some(slot) = self.sof_hv.get_mut(q) {
                    *slot = hv;
                }
                if !self.force_desubsampling {
                    let want = if q == 0 {
                        (self.hor << 4) | self.ver
                    } else {
                        17
                    };
                    if hv != want {
                        return false;
                    }
                }
            }
            let Some(tq) = input.byte() else {
                return false;
            };
            if !self.correcting {
                if let Some(slot) = self.sof_tq.get_mut(q) {
                    *slot = tq;
                }
            }
        }
        if !self.correcting {
            self.sof_log = true;
        }
        true
    }

    /// `OJPEGReadHeaderInfoSecStreamSos`.
    fn stream_sos(&mut self, input: &mut Input<'_>) -> bool {
        if !self.sof_log {
            return false;
        }
        let per = usize::from(self.per_plane);
        if input.word().map(usize::from) != Some(per.saturating_mul(2).saturating_add(6)) {
            return false;
        }
        if input.byte().map(usize::from) != Some(per) {
            return false;
        }
        for o in 0..per {
            let at = usize::from(self.plane_offset).saturating_add(o);
            let Some(cs) = input.byte() else {
                return false;
            };
            if let Some(slot) = self.sos_cs.get_mut(at) {
                *slot = cs;
            }
            let Some(tda) = input.byte() else {
                return false;
            };
            if let Some(slot) = self.sos_tda.get_mut(at) {
                *slot = tda;
            }
        }
        // Ss, Se, Ah and Al: not checked, as libjpeg does not check them.
        input.skip(3);
        true
    }

    /// A table the tags point to: `len` bytes at `offset`, or `None` if the
    /// file is too short (`TIFFReadFile` reading fewer).
    fn table_at(file: &[u8], offset: u64, len: usize) -> Option<&[u8]> {
        let start = usize::try_from(offset).ok()?;
        file.get(start..start.checked_add(len)?)
    }

    /// Whether table offset `m` is one already met, other than the one just
    /// before it -- which the caller has already found different.
    fn repeats_earlier(offsets: &[u64], m: usize) -> bool {
        let this = offsets.get(m).copied().unwrap_or(0);
        (0..m.saturating_sub(1)).any(|n| offsets.get(n).copied() == Some(this))
    }

    /// `OJPEGReadHeaderInfoSecTablesQTable`.
    fn tables_q(&mut self, file: &[u8]) -> bool {
        let offsets = self.q_offsets.clone();
        let offset = |m: usize| offsets.get(m).copied().unwrap_or(0);
        if offset(0) == 0 {
            return false;
        }
        for m in 0..usize::from(self.samples_per_pixel) {
            if offset(m) != 0 && (m == 0 || offset(m) != offset(m.wrapping_sub(1))) {
                if Self::repeats_earlier(&offsets, m) {
                    return false;
                }
                let Some(values) = Self::table_at(file, offset(m), 64) else {
                    return false;
                };
                let mut segment = vec![0xFF, DQT, 0, 67, m as u8];
                segment.extend_from_slice(values);
                if let Some(slot) = self.qtable.get_mut(m) {
                    *slot = Some(segment);
                }
                if let Some(slot) = self.sof_tq.get_mut(m) {
                    *slot = m as u8;
                }
            } else if m > 0 {
                let before = self.sof_tq.get(m.wrapping_sub(1)).copied().unwrap_or(0);
                if let Some(slot) = self.sof_tq.get_mut(m) {
                    *slot = before;
                }
            }
        }
        true
    }

    /// A Huffman table the tags point to, as a whole DHT segment of class
    /// `class`: its 16 counts, then as many values as they add up to.
    fn huffman_at(file: &[u8], offset: u64, class_number: u8) -> Option<Vec<u8>> {
        let counts = Self::table_at(file, offset, 16)?;
        let q: usize = counts.iter().map(|&c| usize::from(c)).sum();
        let values = Self::table_at(file, offset.checked_add(16)?, q)?;
        let length = 19usize.checked_add(q)?;
        let mut segment = vec![
            0xFF,
            DHT,
            (length >> 8) as u8,
            (length & 255) as u8,
            class_number,
        ];
        segment.extend_from_slice(counts);
        segment.extend_from_slice(values);
        Some(segment)
    }

    /// `OJPEGReadHeaderInfoSecTablesDcTable`.
    fn tables_dc(&mut self, file: &[u8]) -> bool {
        let offsets = self.dc_offsets.clone();
        let offset = |m: usize| offsets.get(m).copied().unwrap_or(0);
        if offset(0) == 0 {
            return false;
        }
        for m in 0..usize::from(self.samples_per_pixel) {
            if offset(m) != 0 && (m == 0 || offset(m) != offset(m.wrapping_sub(1))) {
                if Self::repeats_earlier(&offsets, m) {
                    return false;
                }
                let Some(segment) = Self::huffman_at(file, offset(m), m as u8) else {
                    return false;
                };
                if let Some(slot) = self.dctable.get_mut(m) {
                    *slot = Some(segment);
                }
                if let Some(slot) = self.sos_tda.get_mut(m) {
                    *slot = (m as u8) << 4;
                }
            } else if m > 0 {
                let before = self.sos_tda.get(m.wrapping_sub(1)).copied().unwrap_or(0);
                if let Some(slot) = self.sos_tda.get_mut(m) {
                    *slot = before;
                }
            }
        }
        true
    }

    /// `OJPEGReadHeaderInfoSecTablesAcTable`.
    fn tables_ac(&mut self, file: &[u8]) -> bool {
        let offsets = self.ac_offsets.clone();
        let offset = |m: usize| offsets.get(m).copied().unwrap_or(0);
        if offset(0) == 0 {
            return false;
        }
        for m in 0..usize::from(self.samples_per_pixel) {
            if offset(m) != 0 && (m == 0 || offset(m) != offset(m.wrapping_sub(1))) {
                if Self::repeats_earlier(&offsets, m) {
                    return false;
                }
                let Some(segment) = Self::huffman_at(file, offset(m), 16 | m as u8) else {
                    return false;
                };
                if let Some(slot) = self.actable.get_mut(m) {
                    *slot = Some(segment);
                }
                if let Some(slot) = self.sos_tda.get_mut(m) {
                    *slot |= m as u8;
                }
            } else if m > 0 {
                let before = self.sos_tda.get(m.wrapping_sub(1)).copied().unwrap_or(0) & 15;
                if let Some(slot) = self.sos_tda.get_mut(m) {
                    *slot |= before;
                }
            }
        }
        true
    }

    /// `OJPEGReadSecondarySos`: the scan header of plane `s` of a planar
    /// image, found by reading on from the plane before's.
    fn read_secondary_sos(&mut self, file: &[u8], dir: &Directory, s: u16) -> bool {
        let s = usize::from(s);
        let mut offset = s.saturating_sub(1);
        while offset > 0 && self.sos_end.get(offset).copied().flatten().is_none() {
            offset = offset.saturating_sub(1);
        }
        let Some(start) = self.sos_end.get(offset).copied().flatten() else {
            return false;
        };
        self.at = start;
        let mut input = self.input(file, dir);
        while offset < s {
            loop {
                let Some(m) = input.byte() else {
                    self.at = input.at;
                    return false;
                };
                if m == 0xFF {
                    let marker = loop {
                        let Some(m) = input.byte() else {
                            self.at = input.at;
                            return false;
                        };
                        if m != 0xFF {
                            break m;
                        }
                    };
                    if marker == SOS {
                        break;
                    }
                }
            }
            offset = offset.saturating_add(1);
            self.plane_offset = offset as u8;
            if !self.stream_sos(&mut input) {
                self.at = input.at;
                return false;
            }
            if let Some(slot) = self.sos_end.get_mut(offset) {
                *slot = Some(input.at);
            }
        }
        self.at = input.at;
        true
    }

    /// `OJPEGWriteStream`, run to its end: the JPEG libjpeg is given for the
    /// plane whose scan header ends at `self.at` -- tables, restart interval,
    /// frame and scan, then the input's remaining bytes with restart markers
    /// put back between strips. Whether it ends in end-of-image markers (as
    /// the input running out between strips does) or in the input failing.
    fn stream(&mut self, file: &[u8], dir: &Directory) -> (Vec<u8>, bool) {
        let mut out = vec![0xFF, SOI];
        for table in self
            .qtable
            .iter()
            .chain(&self.dctable)
            .chain(&self.actable)
            .flatten()
        {
            out.extend_from_slice(table);
        }
        if self.restart_interval != 0 {
            let [high, low] = self.restart_interval.to_be_bytes();
            out.extend_from_slice(&[0xFF, DRI, 0, 4, high, low]);
        }
        let per = usize::from(self.per_plane);
        let first = usize::from(self.plane_offset);
        let pick =
            |table: &[u8; 3], m: usize| table.get(first.saturating_add(m)).copied().unwrap_or(0);
        // The frame's height and width in two bytes each, cut to 16 bits as
        // libtiff cuts them.
        let [_, _, y_high, y_low] = self.sof_y.to_be_bytes();
        let [_, _, x_high, x_low] = self.sof_x.to_be_bytes();
        out.extend_from_slice(&[
            0xFF,
            self.sof_marker,
            0,
            8u8.wrapping_add(self.per_plane.wrapping_mul(3)),
            8,
            y_high,
            y_low,
            x_high,
            x_low,
            self.per_plane,
        ]);
        for m in 0..per {
            out.extend_from_slice(&[
                pick(&self.sof_c, m),
                pick(&self.sof_hv, m),
                pick(&self.sof_tq, m),
            ]);
        }
        out.extend_from_slice(&[
            0xFF,
            SOS,
            0,
            6u8.wrapping_add(self.per_plane.wrapping_mul(2)),
            self.per_plane,
        ]);
        for m in 0..per {
            out.extend_from_slice(&[pick(&self.sos_cs, m), pick(&self.sos_tda, m)]);
        }
        out.extend_from_slice(&[0, 63, 0]);
        let mut input = self.input(file, dir);
        let mut restart_index = 0u8;
        loop {
            // ososCompressed: the rest of the current block.
            if input.at.togo == 0 && !input.fill() {
                self.at = input.at;
                return (out, false);
            }
            let start = usize::try_from(input.at.pos).unwrap_or(usize::MAX);
            let len = usize::try_from(input.at.togo).unwrap_or(0);
            if let Some(bytes) = file.get(start..start.saturating_add(len)) {
                out.extend_from_slice(bytes);
            }
            input.at.pos = input.at.pos.saturating_add(input.at.togo);
            input.at.togo = 0;
            match input.at.block {
                Block::Strile if input.at.next_strile < input.strile_count => {
                    // ososRst.
                    out.extend_from_slice(&[0xFF, RST0.wrapping_add(restart_index)]);
                    restart_index = restart_index.wrapping_add(1) & 7;
                }
                Block::Strile | Block::Eof => {
                    self.at = input.at;
                    return (out, true);
                }
                Block::NotSetYet | Block::InterchangeFormat => {}
            }
        }
    }

    /// `OJPEGWriteHeaderInfo`: start a session for the plane whose scan
    /// header ends at `self.at`.
    fn start_session(&mut self, file: &[u8], dir: &Directory) -> bool {
        if self.session_failed {
            return false;
        }
        let (stream, ends_in_eoi) = self.stream(file, dir);
        let mut jpeg = Decompress::owned(stream);
        jpeg.set_strict_source(!ends_in_eoi);
        // From here on a failure is final: libtiff's session stays active,
        // and it starts no other.
        self.session_failed = true;
        if jpeg.read_header(true).is_err() {
            return false;
        }
        let style = if !self.force_desubsampling && self.per_plane > 1 {
            jpeg.set_raw_output();
            if self.convert.is_none() && !self.make_convert() {
                return false;
            }
            Style::Raw
        } else {
            jpeg.set_color_spaces(ColorSpace::Unknown, ColorSpace::Unknown);
            let width = usize::try_from(self.strile_width).unwrap_or(usize::MAX);
            self.bytes_per_line = usize::from(self.per_plane).saturating_mul(width);
            self.lines_per_strile = usize::try_from(self.strile_length).unwrap_or(usize::MAX);
            Style::Scanlines
        };
        if jpeg.start(&self.limits, None).is_err() {
            return false;
        }
        if u32::try_from(jpeg.image_width()).ok() != Some(self.strile_width) {
            return false;
        }
        if jpeg.max_sampling() != (usize::from(self.hor), usize::from(self.ver)) {
            return false;
        }
        self.session_failed = false;
        self.session = Some(Session { jpeg, style });
        true
    }

    /// The first raw session's buffers (`subsampling_convert_log` still 0),
    /// and the line sizes every raw session then works in, which libtiff
    /// sets here and nowhere else.
    fn make_convert(&mut self) -> bool {
        // "Check for division by zero."
        if self.hor == 0 || self.ver == 0 {
            return false;
        }
        let (hor, ver) = (usize::from(self.hor), usize::from(self.ver));
        let width = usize::try_from(self.strile_width).unwrap_or(usize::MAX);
        let length = usize::try_from(self.strile_length).unwrap_or(usize::MAX);
        let across = hor.saturating_mul(8);
        let ylinelen = width.div_ceil(across).saturating_mul(across);
        let ylines = ver.saturating_mul(8);
        let clinelen = ylinelen.checked_div(hor).unwrap_or(0);
        let clines = 8;
        let ybuflen = ylinelen.saturating_mul(ylines);
        let cbuflen = clinelen.saturating_mul(clines);
        // libtiff's `calloc` of the three, held to the decode's limit. (A
        // strip wider than a JPEG can be never gets past the width check
        // that follows, whatever is allocated here.)
        if ybuflen.saturating_add(cbuflen.saturating_mul(2)) > self.limits.max_decompressed_bytes {
            return false;
        }
        let clinelenout = width.div_ceil(hor);
        self.convert = Some(Convert {
            planes: vec![
                RawPlane {
                    data: vec![0; ybuflen],
                    stride: ylinelen,
                },
                RawPlane {
                    data: vec![0; cbuflen],
                    stride: clinelen,
                },
                RawPlane {
                    data: vec![0; cbuflen],
                    stride: clinelen,
                },
            ],
            ylinelen,
            clinelen,
            clines,
            clinelenout,
            state: 0,
        });
        self.error_in_raw = false;
        self.bytes_per_line = clinelenout.saturating_mul(ver.saturating_mul(hor).saturating_add(2));
        self.lines_per_strile = length.div_ceil(ver);
        true
    }

    /// `OJPEGPreDecode`, for strip (or tile) `m` of plane `s`.
    pub(super) fn pre_decode(
        &mut self,
        file: &[u8],
        dir: &Directory,
        s: u16,
        m: u32,
    ) -> ImageResult<()> {
        let fail = ImageError::Malformed("TIFF old-style JPEG");
        self.correct_subsampling(file, dir);
        if !self.header_done && !self.read_header_info(file, dir) {
            return Err(fail);
        }
        if self
            .sos_end
            .get(usize::from(s))
            .copied()
            .flatten()
            .is_none()
            && !self.read_secondary_sos(file, dir, s)
        {
            return Err(fail);
        }
        if self.session.is_some() && (self.write_cursample != s || self.write_curstrile > m) {
            self.session = None;
        }
        if self.session.is_none() {
            self.plane_offset = s as u8;
            self.write_cursample = s;
            self.write_curstrile = u32::from(s).wrapping_mul(dir.strips_per_image);
            // Back to the plane's first data. (libtiff stays put if its
            // input is already at that offset; see the module notes.)
            if let Some(start) = self.sos_end.get(usize::from(s)).copied().flatten() {
                self.at = start;
            }
            if !self.start_session(file, dir) {
                return Err(fail);
            }
        }
        if let Some(convert) = self.convert.as_mut() {
            convert.state = 0;
        }
        while self.write_curstrile < m {
            let skipped = match self.session.as_ref().map(|s| s.style) {
                Some(Style::Raw) => self.skip_raw(),
                Some(Style::Scanlines) => self.skip_scanlines(),
                None => false,
            };
            if !skipped {
                return Err(fail);
            }
            self.write_curstrile = self.write_curstrile.wrapping_add(1);
        }
        self.decoder_ok = true;
        Ok(())
    }

    /// `OJPEGPreDecodeSkipRaw`: a strip's worth of raw lines, read and
    /// dropped.
    fn skip_raw(&mut self) -> bool {
        let (Some(session), Some(convert)) = (self.session.as_mut(), self.convert.as_mut()) else {
            return false;
        };
        let mut m = self.lines_per_strile;
        if convert.state != 0 {
            let left = convert.clines.saturating_sub(convert.state);
            if left >= m {
                convert.state = convert.state.saturating_add(m);
                if convert.state == convert.clines {
                    convert.state = 0;
                }
                return true;
            }
            m = m.saturating_sub(left);
            convert.state = 0;
            self.error_in_raw = false;
        }
        while m >= convert.clines {
            if session.jpeg.read_raw(&mut convert.planes).is_err() {
                return false;
            }
            m = m.saturating_sub(convert.clines);
        }
        if m > 0 {
            if session.jpeg.read_raw(&mut convert.planes).is_err() {
                return false;
            }
            convert.state = m;
        }
        true
    }

    /// `OJPEGPreDecodeSkipScanlines`.
    fn skip_scanlines(&mut self) -> bool {
        let Some(session) = self.session.as_mut() else {
            return false;
        };
        for _ in 0..self.lines_per_strile {
            if session.jpeg.read_row().is_err() {
                return false;
            }
        }
        true
    }

    /// `OJPEGDecode` into `out`, then `OJPEGPostDecode` if the reader still
    /// runs it (`post_decode`): libtiff replaces it with its byte swap when
    /// a big-endian file's `BitsPerSample` is set, which follows the codec's
    /// setup.
    pub(super) fn decode(
        &mut self,
        out: &mut [u8],
        dir: &Directory,
        post_decode: bool,
    ) -> ImageResult<()> {
        if self.decode_lines(out).is_err() {
            out.fill(0);
            return Err(ImageError::Malformed("TIFF old-style JPEG strip"));
        }
        if post_decode {
            self.write_curstrile = self.write_curstrile.wrapping_add(1);
            if self
                .write_curstrile
                .is_multiple_of(dir.strips_per_image.max(1))
            {
                self.session = None;
            }
        }
        Ok(())
    }

    fn decode_lines(&mut self, out: &mut [u8]) -> Result<(), ()> {
        if !self.decoder_ok || self.error_in_raw {
            return Err(());
        }
        let Some(session) = self.session.as_mut() else {
            return Err(());
        };
        let per_line = self.bytes_per_line;
        if per_line == 0 || out.is_empty() || !out.len().is_multiple_of(per_line) {
            // "Fractional scanline not read".
            return Err(());
        }
        match session.style {
            Style::Scanlines => {
                for line in out.chunks_exact_mut(per_line) {
                    let row = session.jpeg.read_row().map_err(|_| ())?;
                    // Past the JPEG's last row libjpeg writes nothing, and the
                    // line keeps what the buffer held.
                    let n = row.len().min(line.len());
                    if let (Some(dst), Some(src)) = (line.get_mut(..n), row.get(..n)) {
                        dst.copy_from_slice(src);
                    }
                }
                Ok(())
            }
            Style::Raw => {
                let Some(convert) = self.convert.as_mut() else {
                    return Err(());
                };
                let (hor, ver) = (usize::from(self.hor), usize::from(self.ver));
                for line in out.chunks_exact_mut(per_line) {
                    if convert.state == 0 && session.jpeg.read_raw(&mut convert.planes).is_err() {
                        self.error_in_raw = true;
                        return Err(());
                    }
                    pack_line(convert, hor, ver, line);
                    convert.state = convert.state.saturating_add(1);
                    if convert.state == convert.clines {
                        convert.state = 0;
                    }
                }
                Ok(())
            }
        }
    }
}

/// One line of `OJPEGDecodeRaw`: for each chroma sample of the current chroma
/// row, its `hor` x `ver` luma samples and then its Cb and Cr -- the layout
/// TIFF gives subsampled `YCbCr`.
fn pack_line(convert: &Convert, hor: usize, ver: usize, line: &mut [u8]) {
    let [y, cb, cr] =
        [0, 1, 2].map(|i| convert.planes.get(i).map_or(&[][..], |p| p.data.as_slice()));
    let mut slots = line.iter_mut();
    let mut put = |value: u8| {
        if let Some(slot) = slots.next() {
            *slot = value;
        }
    };
    let ybase = convert
        .state
        .saturating_mul(ver)
        .saturating_mul(convert.ylinelen);
    let cbase = convert.state.saturating_mul(convert.clinelen);
    for q in 0..convert.clinelenout {
        let column = ybase.saturating_add(q.saturating_mul(hor));
        for sy in 0..ver {
            let row = column.saturating_add(sy.saturating_mul(convert.ylinelen));
            for sx in 0..hor {
                put(y.get(row.saturating_add(sx)).copied().unwrap_or(0));
            }
        }
        let c = cbase.saturating_add(q);
        put(cb.get(c).copied().unwrap_or(0));
        put(cr.get(c).copied().unwrap_or(0));
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]

    use super::*;

    /// An input over `file` with `JPEGInterchangeFormat` at `jif` and the
    /// strips `offsets` and `counts` (as many as there are offsets).
    fn over<'a>(
        file: &'a [u8],
        jif: (u64, u64),
        offsets: &'a [u64],
        counts: &'a [u64],
    ) -> Input<'a> {
        Input {
            file,
            offsets,
            counts,
            strile_count: u32::try_from(offsets.len().max(1)).unwrap(),
            jif: jif.0,
            jif_len: jif.1,
            at: Position::START,
        }
    }

    fn drain(input: &mut Input<'_>) -> Vec<u8> {
        let mut out = Vec::new();
        while let Some(byte) = input.byte() {
            out.push(byte);
        }
        out
    }

    fn file() -> Vec<u8> {
        (0..20).collect()
    }

    #[test]
    fn the_input_is_the_interchange_block_then_each_strip() {
        let file = file();
        let mut input = over(&file, (2, 4), &[10, 15], &[3, 2]);
        assert_eq!(drain(&mut input), [2, 3, 4, 5, 10, 11, 12, 15, 16]);
        assert_eq!(input.at.block, Block::Eof);
    }

    #[test]
    fn a_strip_at_0_or_past_the_end_has_no_data_and_one_of_no_count_runs_to_the_end() {
        let file = file();
        let mut input = over(&file, (0, 0), &[0, 30, 17], &[5, 5, 0]);
        assert_eq!(drain(&mut input), [17, 18, 19]);
    }

    #[test]
    fn a_count_past_the_end_of_the_file_stops_there() {
        let file = file();
        let mut input = over(&file, (0, 0), &[18], &[10]);
        assert_eq!(drain(&mut input), [18, 19]);
    }

    #[test]
    fn a_strip_array_the_directory_lacks_fails_where_it_stands() {
        let file = file();
        // No offsets at all: the first strip fails, and fails again.
        let mut input = over(&file, (2, 2), &[], &[]);
        assert_eq!(drain(&mut input), [2, 3]);
        assert_eq!(input.byte(), None);
        assert_eq!((input.at.block, input.at.next_strile), (Block::Strile, 0));
        // No counts: a strip at 0 needs none, the next one fails.
        let mut input = over(&file, (0, 0), &[0, 5], &[]);
        assert_eq!(drain(&mut input), []);
        assert_eq!((input.at.block, input.at.next_strile), (Block::Strile, 1));
    }

    #[test]
    fn a_skip_stops_at_its_block_end_and_a_block_read_crosses_it() {
        let file = file();
        let mut input = over(&file, (2, 4), &[10], &[3]);
        // Before the first read there is no block to skip in.
        input.skip(10);
        assert_eq!(input.byte(), Some(2));
        input.skip(10);
        assert_eq!(input.byte(), Some(10));
        let mut input = over(&file, (2, 4), &[10], &[3]);
        assert_eq!(input.block(6), Some(vec![2, 3, 4, 5, 10, 11]));
        assert_eq!(input.block(2), None);
    }

    #[test]
    fn a_raw_line_packs_each_chroma_sample_after_its_luma_block() {
        let plane = |rows: usize, stride: usize, base: usize| RawPlane {
            data: (0..rows * stride)
                .map(|i| u8::try_from((base + i) % 256).unwrap())
                .collect(),
            stride,
        };
        let convert = Convert {
            planes: vec![plane(16, 16, 0), plane(8, 8, 100), plane(8, 8, 200)],
            ylinelen: 16,
            clinelen: 8,
            clines: 8,
            clinelenout: 2,
            state: 1,
        };
        let mut line = [0u8; 12];
        pack_line(&convert, 2, 2, &mut line);
        // Chroma row 1 covers luma rows 2 and 3.
        let y = |r: usize, c: usize| u8::try_from(r * 16 + c).unwrap();
        assert_eq!(
            line,
            [
                y(2, 0),
                y(2, 1),
                y(3, 0),
                y(3, 1),
                108,
                208,
                y(2, 2),
                y(2, 3),
                y(3, 2),
                y(3, 3),
                109,
                209
            ]
        );
    }
}
