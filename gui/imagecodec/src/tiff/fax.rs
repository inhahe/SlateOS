//! CCITT fax compression -- Group 3 (1-D and 2-D), Group 4, and the
//! Modified Huffman run-length schemes -- as libtiff 4.7.1 decodes it
//! (`tif_fax3.c`, `tif_fax3.h`).
//!
//! A port, control flow and all: the row decoders are libtiff's macros
//! (`EXPAND1D`, `EXPAND2D`, `SYNC_EOL`, `CLEANUP_RUNS`) written out, and its
//! leniency is kept -- a bad code word ends its row, which is then padded
//! white or cut to width, and decoding carries on with the next; only data
//! that runs out stops a strip. Two of its habits are kept because files
//! decode differently without them: a Group 3 strip that turns out to have
//! no end-of-line codes is decoded again from where the call began, without
//! them, into the rows still to fill; and a Group 4 strip that ends early is
//! accepted if any row of it decoded, the rest of the buffer left as it was.
//!
//! The code tables are built as libtiff's `mkg3states` builds them.

use alloc::vec;
use alloc::vec::Vec;

use crate::{ImageError, ImageResult};

// Finite-state-machine codes (`S_*`).
const S_PASS: u8 = 1;
const S_HORIZ: u8 = 2;
const S_V0: u8 = 3;
const S_VR: u8 = 4;
const S_VL: u8 = 5;
const S_EXT: u8 = 6;
const S_TERM_W: u8 = 7;
const S_TERM_B: u8 = 8;
const S_MAKE_UP_W: u8 = 9;
const S_MAKE_UP_B: u8 = 10;
const S_MAKE_UP: u8 = 11;
const S_EOL: u8 = 12;

/// A table entry (`TIFFFaxTabEnt`).
#[derive(Clone, Copy, Default)]
struct Ent {
    state: u8,
    width: u8,
    param: u32,
}

/// `(code, (pixels << 4) + width)`, codes right-justified and
/// least-significant-bit first, as `mkg3states` lists them.
type Proto = &'static [(u16, u16)];

const PASS: Proto = &[(0x0008, 4)];
const HORIZ: Proto = &[(0x0004, 3)];
const V0: Proto = &[(0x0001, 1)];
const VR: Proto = &[
    (0x0006, (1 << 4) + 3),
    (0x0030, (2 << 4) + 6),
    (0x0060, (3 << 4) + 7),
];
const VL: Proto = &[
    (0x0002, (1 << 4) + 3),
    (0x0010, (2 << 4) + 6),
    (0x0020, (3 << 4) + 7),
];
const EXT: Proto = &[(0x0040, 7)];
const EOLV: Proto = &[(0x0000, 7)];
const MAKE_UP_W: Proto = &[
    (0x001b, 1029),
    (0x0009, 2053),
    (0x003a, 3078),
    (0x0076, 4103),
    (0x006c, 5128),
    (0x00ec, 6152),
    (0x0026, 7176),
    (0x00a6, 8200),
    (0x0016, 9224),
    (0x00e6, 10248),
    (0x0066, 11273),
    (0x0166, 12297),
    (0x0096, 13321),
    (0x0196, 14345),
    (0x0056, 15369),
    (0x0156, 16393),
    (0x00d6, 17417),
    (0x01d6, 18441),
    (0x0036, 19465),
    (0x0136, 20489),
    (0x00b6, 21513),
    (0x01b6, 22537),
    (0x0032, 23561),
    (0x0132, 24585),
    (0x00b2, 25609),
    (0x0006, 26630),
    (0x01b2, 27657),
];
const MAKE_UP_B: Proto = &[
    (0x03c0, 1034),
    (0x0130, 2060),
    (0x0930, 3084),
    (0x0da0, 4108),
    (0x0cc0, 5132),
    (0x02c0, 6156),
    (0x0ac0, 7180),
    (0x06c0, 8205),
    (0x16c0, 9229),
    (0x0a40, 10253),
    (0x1a40, 11277),
    (0x0640, 12301),
    (0x1640, 13325),
    (0x09c0, 14349),
    (0x19c0, 15373),
    (0x05c0, 16397),
    (0x15c0, 17421),
    (0x0dc0, 18445),
    (0x1dc0, 19469),
    (0x0940, 20493),
    (0x1940, 21517),
    (0x0540, 22541),
    (0x1540, 23565),
    (0x0b40, 24589),
    (0x1b40, 25613),
    (0x04c0, 26637),
    (0x14c0, 27661),
];
const MAKE_UP: Proto = &[
    (0x0080, 28683),
    (0x0180, 29707),
    (0x0580, 30731),
    (0x0480, 31756),
    (0x0c80, 32780),
    (0x0280, 33804),
    (0x0a80, 34828),
    (0x0680, 35852),
    (0x0e80, 36876),
    (0x0380, 37900),
    (0x0b80, 38924),
    (0x0780, 39948),
    (0x0f80, 40972),
];
const TERM_W: Proto = &[
    (0x00ac, 8),
    (0x0038, 22),
    (0x000e, 36),
    (0x0001, 52),
    (0x000d, 68),
    (0x0003, 84),
    (0x0007, 100),
    (0x000f, 116),
    (0x0019, 133),
    (0x0005, 149),
    (0x001c, 165),
    (0x0002, 181),
    (0x0004, 198),
    (0x0030, 214),
    (0x000b, 230),
    (0x002b, 246),
    (0x0015, 262),
    (0x0035, 278),
    (0x0072, 295),
    (0x0018, 311),
    (0x0008, 327),
    (0x0074, 343),
    (0x0060, 359),
    (0x0010, 375),
    (0x000a, 391),
    (0x006a, 407),
    (0x0064, 423),
    (0x0012, 439),
    (0x000c, 455),
    (0x0040, 472),
    (0x00c0, 488),
    (0x0058, 504),
    (0x00d8, 520),
    (0x0048, 536),
    (0x00c8, 552),
    (0x0028, 568),
    (0x00a8, 584),
    (0x0068, 600),
    (0x00e8, 616),
    (0x0014, 632),
    (0x0094, 648),
    (0x0054, 664),
    (0x00d4, 680),
    (0x0034, 696),
    (0x00b4, 712),
    (0x0020, 728),
    (0x00a0, 744),
    (0x0050, 760),
    (0x00d0, 776),
    (0x004a, 792),
    (0x00ca, 808),
    (0x002a, 824),
    (0x00aa, 840),
    (0x0024, 856),
    (0x00a4, 872),
    (0x001a, 888),
    (0x009a, 904),
    (0x005a, 920),
    (0x00da, 936),
    (0x0052, 952),
    (0x00d2, 968),
    (0x004c, 984),
    (0x00cc, 1000),
    (0x002c, 1016),
];
const TERM_B: Proto = &[
    (0x03b0, 10),
    (0x0002, 19),
    (0x0003, 34),
    (0x0001, 50),
    (0x0006, 67),
    (0x000c, 84),
    (0x0004, 100),
    (0x0018, 117),
    (0x0028, 134),
    (0x0008, 150),
    (0x0010, 167),
    (0x0050, 183),
    (0x0070, 199),
    (0x0020, 216),
    (0x00e0, 232),
    (0x0030, 249),
    (0x03a0, 266),
    (0x0060, 282),
    (0x0040, 298),
    (0x0730, 315),
    (0x00b0, 331),
    (0x01b0, 347),
    (0x0760, 363),
    (0x00a0, 379),
    (0x0740, 395),
    (0x00c0, 411),
    (0x0530, 428),
    (0x0d30, 444),
    (0x0330, 460),
    (0x0b30, 476),
    (0x0160, 492),
    (0x0960, 508),
    (0x0560, 524),
    (0x0d60, 540),
    (0x04b0, 556),
    (0x0cb0, 572),
    (0x02b0, 588),
    (0x0ab0, 604),
    (0x06b0, 620),
    (0x0eb0, 636),
    (0x0360, 652),
    (0x0b60, 668),
    (0x05b0, 684),
    (0x0db0, 700),
    (0x02a0, 716),
    (0x0aa0, 732),
    (0x06a0, 748),
    (0x0ea0, 764),
    (0x0260, 780),
    (0x0a60, 796),
    (0x04a0, 812),
    (0x0ca0, 828),
    (0x0240, 844),
    (0x0ec0, 860),
    (0x01c0, 876),
    (0x0e40, 892),
    (0x0140, 908),
    (0x01a0, 924),
    (0x09a0, 940),
    (0x0d40, 956),
    (0x0340, 972),
    (0x05a0, 988),
    (0x0660, 1004),
    (0x0e60, 1020),
];
const EOLH: Proto = &[(0x0000, 11)];

/// `FillTable`: every index whose low bits are the code gets its entry.
fn fill(table: &mut [Ent], size: u32, protos: Proto, state: u8) {
    let limit = 1usize << size;
    for &(code, val) in protos {
        let width = u8::try_from(val & 15).unwrap_or(0);
        let param = u32::from(val >> 4);
        let step = 1usize << width;
        let mut at = usize::from(code);
        while at < limit {
            if let Some(e) = table.get_mut(at) {
                *e = Ent {
                    state,
                    width,
                    param,
                };
            }
            at = at.saturating_add(step);
        }
    }
}

/// The three tables (`TIFFFaxMainTable`, `...WhiteTable`, `...BlackTable`).
pub(super) struct Tables {
    main: Vec<Ent>,
    white: Vec<Ent>,
    black: Vec<Ent>,
}

impl Tables {
    pub(super) fn new() -> Self {
        let mut main = vec![Ent::default(); 128];
        let mut white = vec![Ent::default(); 4096];
        let mut black = vec![Ent::default(); 8192];
        fill(&mut main, 7, PASS, S_PASS);
        fill(&mut main, 7, HORIZ, S_HORIZ);
        fill(&mut main, 7, V0, S_V0);
        fill(&mut main, 7, VR, S_VR);
        fill(&mut main, 7, VL, S_VL);
        fill(&mut main, 7, EXT, S_EXT);
        fill(&mut main, 7, EOLV, S_EOL);
        fill(&mut white, 12, MAKE_UP_W, S_MAKE_UP_W);
        fill(&mut white, 12, MAKE_UP, S_MAKE_UP);
        fill(&mut white, 12, TERM_W, S_TERM_W);
        fill(&mut white, 12, EOLH, S_EOL);
        fill(&mut black, 13, MAKE_UP_B, S_MAKE_UP_B);
        fill(&mut black, 13, MAKE_UP, S_MAKE_UP);
        fill(&mut black, 13, TERM_B, S_TERM_B);
        fill(&mut black, 13, EOLH, S_EOL);
        Self { main, white, black }
    }
}

/// The fax modes (`FAXMODE_*`).
const MODE_NOEOL: u32 = 0x0002;
const MODE_BYTEALIGN: u32 = 0x0004;
const MODE_WORDALIGN: u32 = 0x0008;

/// Which decoder a scheme uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    /// Compression 2 (byte-aligned rows) and 32771 (word-aligned).
    Rle { word_aligned: bool },
    /// Compression 3; `two_d` from `Group3Options` bit 0.
    Group3 { two_d: bool },
    /// Compression 4.
    Group4,
}

/// Why a strip's decode stopped: libtiff returns -1 for both.
#[derive(Debug)]
enum Stop {
    /// Out of data -- or a run array overflowed, which libtiff reports and
    /// treats the same way.
    Fail,
}

/// The decoder state that lives on the `TIFF` handle.
pub(super) struct Fax {
    tables: Tables,
    kind: Kind,
    mode: u32,
    row_pixels: u32,
    row_bytes: usize,
    nruns: usize,
    runs: Vec<u32>,
    reverse: bool,
    /// Whether rows may be coded against the one before (2-D Group 3, and
    /// Group 4), which needs the reference line's runs.
    two_d: bool,
}

/// The bit reader and row state of one decode call (`DECLARE_STATE`).
struct State<'a> {
    data: &'a [u8],
    cp: usize,
    acc: u32,
    avail: i32,
    eol_count: i32,
    reverse: bool,
    /// Where the call began, for the retry without end-of-line codes.
    start: (usize, u32, i32, i32),
}

impl State<'_> {
    fn next_byte(&mut self) -> u32 {
        let b = self.data.get(self.cp).copied().unwrap_or(0);
        self.cp = self.cp.saturating_add(1);
        u32::from(if self.reverse { b.reverse_bits() } else { b })
    }

    fn at_end(&self) -> bool {
        self.cp >= self.data.len()
    }

    /// `NeedBits8`: false for no data at all left.
    fn need8(&mut self, n: i32) -> bool {
        if self.avail < n {
            if self.at_end() {
                if self.avail == 0 {
                    return false;
                }
                self.avail = n;
            } else {
                let b = self.next_byte();
                self.acc |= b
                    .checked_shl(u32::try_from(self.avail).unwrap_or(0))
                    .unwrap_or(0);
                self.avail = self.avail.wrapping_add(8);
            }
        }
        true
    }

    /// `NeedBits16`.
    fn need16(&mut self, n: i32) -> bool {
        if self.avail < n {
            if self.at_end() {
                if self.avail == 0 {
                    return false;
                }
                self.avail = n;
            } else {
                let b = self.next_byte();
                self.acc |= b
                    .checked_shl(u32::try_from(self.avail).unwrap_or(0))
                    .unwrap_or(0);
                self.avail = self.avail.wrapping_add(8);
                if self.avail < n {
                    if self.at_end() {
                        self.avail = n;
                    } else {
                        let b = self.next_byte();
                        self.acc |= b
                            .checked_shl(u32::try_from(self.avail).unwrap_or(0))
                            .unwrap_or(0);
                        self.avail = self.avail.wrapping_add(8);
                    }
                }
            }
        }
        true
    }

    fn get(&self, n: u32) -> u32 {
        self.acc & (1u32 << n).wrapping_sub(1)
    }

    fn clr(&mut self, n: i32) {
        self.avail = self.avail.wrapping_sub(n);
        self.acc = self
            .acc
            .checked_shr(u32::try_from(n).unwrap_or(32))
            .unwrap_or(0);
    }

    /// `LOOKUP8`/`LOOKUP16`: `None` at the end of the data.
    fn lookup(&mut self, width: u32, table: &[Ent], sixteen: bool) -> Option<Ent> {
        let n = i32::try_from(width).unwrap_or(0);
        let ok = if sixteen {
            self.need16(n)
        } else {
            self.need8(n)
        };
        if !ok {
            return None;
        }
        let e = table
            .get(self.get(width) as usize)
            .copied()
            .unwrap_or_default();
        self.clr(i32::from(e.width));
        Some(e)
    }

    fn rewind(&mut self) {
        let (cp, acc, avail, eol) = self.start;
        self.cp = cp;
        self.acc = acc;
        self.avail = avail;
        self.eol_count = eol;
    }
}

/// One row's runs (`pa` into `thisrun`, and the reference line's `pb`).
struct Row {
    a0: i32,
    run_length: i32,
    lastx: i32,
    this: usize,
    pa: usize,
    end: usize,
}

impl Row {
    /// `SETVALUE`.
    fn set(&mut self, runs: &mut [u32], x: i32) -> Result<(), Stop> {
        if self.pa >= self.end {
            return Err(Stop::Fail);
        }
        let slot = runs.get_mut(self.pa).ok_or(Stop::Fail)?;
        *slot = self.run_length.wrapping_add(x).cast_unsigned();
        self.pa = self.pa.wrapping_add(1);
        self.a0 = self.a0.wrapping_add(x);
        self.run_length = 0;
        Ok(())
    }

    /// `*pa++ = v` without the bound check (the C checked it earlier).
    fn push(&mut self, runs: &mut [u32], v: i32) -> Result<(), Stop> {
        *runs.get_mut(self.pa).ok_or(Stop::Fail)? = v.cast_unsigned();
        self.pa = self.pa.wrapping_add(1);
        Ok(())
    }

    /// `CLEANUP_RUNS`: make the row exactly `lastx` long.
    fn cleanup(&mut self, runs: &mut [u32]) -> Result<(), Stop> {
        if self.run_length != 0 {
            self.set(runs, 0)?;
        }
        if self.a0 != self.lastx {
            while self.a0 > self.lastx && self.pa > self.this {
                self.pa = self.pa.wrapping_sub(1);
                let v = runs.get(self.pa).copied().ok_or(Stop::Fail)?;
                self.a0 = self.a0.wrapping_sub(v.cast_signed());
            }
            if self.a0 < self.lastx {
                if self.a0 < 0 {
                    self.a0 = 0;
                }
                if (self.pa.wrapping_sub(self.this)) & 1 != 0 {
                    self.set(runs, 0)?;
                }
                let x = self.lastx.wrapping_sub(self.a0);
                self.set(runs, x)?;
            } else if self.a0 > self.lastx {
                let x = self.lastx;
                self.set(runs, x)?;
                self.set(runs, 0)?;
            }
        }
        Ok(())
    }
}

/// How a row ended.
enum End {
    /// The row is done (`done1d` / `eol2d`).
    Done,
    /// The data ran out (`eof1d` / `eof2d`); the row was cleaned up.
    Eof,
}

impl Fax {
    /// `Fax3SetupState` and the codec's `TIFFSetField(TIFFTAG_FAXMODE)`:
    /// `None` for samples fax cannot carry.
    pub(super) fn new(
        kind: Kind,
        bits: u16,
        samples: u16,
        separate: bool,
        row_bytes: u64,
        row_pixels: u32,
        fill_order: u16,
    ) -> Option<Self> {
        if bits != 1 || (samples != 1 && !separate) {
            return None;
        }
        if row_bytes < u64::from(row_pixels).div_ceil(8) {
            return None;
        }
        let row_bytes = usize::try_from(row_bytes).ok()?;
        let two_d = matches!(kind, Kind::Group4 | Kind::Group3 { two_d: true });
        let mut nruns = u32::try_from(
            u64::from(row_pixels)
                .checked_add(1)?
                .div_ceil(32)
                .checked_mul(32)?,
        )
        .ok()?;
        if two_d {
            nruns = nruns.checked_mul(2)?;
        }
        let total = nruns.checked_mul(2)?;
        if nruns == 0 {
            return None;
        }
        let mode = match kind {
            Kind::Rle {
                word_aligned: false,
            } => MODE_NORTC | MODE_NOEOL | MODE_BYTEALIGN,
            Kind::Rle { word_aligned: true } => MODE_NORTC | MODE_NOEOL | MODE_WORDALIGN,
            _ => MODE_NORTC,
        };
        Some(Self {
            tables: Tables::new(),
            kind,
            mode,
            row_pixels,
            row_bytes,
            nruns: usize::try_from(nruns).ok()?,
            runs: vec![0; usize::try_from(total).ok()?],
            // The decoder reads least significant bit first, so fill order
            // 1 -- most significant first -- is reversed byte by byte.
            reverse: fill_order != 2,
            two_d,
        })
    }

    /// Decode one strip or tile (`Fax3PreDecode`, then the scheme's
    /// decoder) into `out`, whole rows. `offset` is where the strip lies in
    /// the file: the word-aligned scheme's alignment test reads it.
    ///
    /// # Errors
    ///
    /// libtiff's failures: data that runs out mid-row (except a Group 4
    /// strip with a row already done), a run array that overflows, or a
    /// buffer that is not whole rows.
    pub(super) fn decode(&mut self, data: &[u8], out: &mut [u8], offset: u64) -> ImageResult<()> {
        if self.row_bytes == 0 || !out.len().is_multiple_of(self.row_bytes) {
            return Err(ImageError::Malformed("TIFF fax strip not whole rows"));
        }
        // `Fax3PreDecode`: a white reference line, fresh bit state.
        let refl = if self.two_d { self.nruns } else { usize::MAX };
        if self.two_d {
            if let Some(r) = self.runs.get_mut(refl) {
                *r = self.row_pixels;
            }
            if let Some(r) = self.runs.get_mut(refl.wrapping_add(1)) {
                *r = 0;
            }
        }
        let mut st = State {
            data,
            cp: 0,
            acc: 0,
            avail: 0,
            eol_count: 0,
            reverse: self.reverse,
            start: (0, 0, 0, 0),
        };
        let ctx = Ctx {
            tables: &self.tables,
            nruns: self.nruns,
            lastx: i32::try_from(self.row_pixels).unwrap_or(i32::MAX),
            row_bytes: self.row_bytes,
        };
        let runs = &mut self.runs;
        let result = match self.kind {
            Kind::Rle { word_aligned } => {
                decode_rle(&ctx, self.mode, runs, &mut st, out, word_aligned, offset)
            }
            Kind::Group3 { two_d: false } => decode_1d(&ctx, &mut self.mode, runs, &mut st, out),
            Kind::Group3 { two_d: true } => {
                decode_2d(&ctx, &mut self.mode, runs, &mut st, out, refl)
            }
            Kind::Group4 => decode_g4(&ctx, runs, &mut st, out, refl),
        };
        result.map_err(|Stop::Fail| ImageError::Corrupt("fax data ends mid-row"))
    }
}

const MODE_NORTC: u32 = 0x0001;

/// What the row decoders share.
struct Ctx<'t> {
    tables: &'t Tables,
    nruns: usize,
    lastx: i32,
    row_bytes: usize,
}

impl Ctx<'_> {
    fn row(&self, this: usize) -> Row {
        Row {
            a0: 0,
            run_length: 0,
            lastx: self.lastx,
            this,
            pa: this,
            end: this.saturating_add(self.nruns),
        }
    }
}

/// A row of `out` starting at byte `at`.
fn row_of(out: &mut [u8], at: usize, len: usize) -> Result<&mut [u8], Stop> {
    out.get_mut(at..at.saturating_add(len)).ok_or(Stop::Fail)
}

/// `SYNC_EOL`: skip to just past the next end-of-line code. `Ok(false)` for
/// data that ends first; `*retry` set when there is none to find, so the
/// strip is to be decoded again without them.
fn sync_eol(mode: &mut u32, st: &mut State<'_>, retry: &mut bool) -> bool {
    if *mode & MODE_NOEOL != 0 {
        return true;
    }
    if st.eol_count == 0 {
        loop {
            if !st.need16(11) {
                return false;
            }
            if st.get(11) == 0 {
                break;
            }
            st.clr(1);
        }
    }
    loop {
        if !st.need8(8) {
            // `noEOLFound`: from now on this image is read without them.
            *mode |= MODE_NOEOL;
            *retry = true;
            return true;
        }
        if st.get(8) != 0 {
            break;
        }
        st.clr(8);
    }
    while st.get(1) == 0 {
        st.clr(1);
    }
    st.clr(1);
    st.eol_count = 0;
    true
}

fn param(e: Ent) -> i32 {
    e.param.cast_signed()
}

/// One colour's run: make-up codes then a terminating code
/// (`EXPAND1D`'s and `EXPAND2D`'s inner loops). `Ok(Some(true))` for a run
/// ended, `Ok(Some(false))` for an end-of-line code (1-D only),
/// `Ok(None)` for a bad code, `Err` at the end of the data.
fn run_of(
    st: &mut State<'_>,
    row: &mut Row,
    runs: &mut [u32],
    table: &[Ent],
    width: u32,
    term: u8,
    make_up: u8,
    eol_ok: bool,
) -> Result<Option<bool>, Eof> {
    loop {
        let e = st.lookup(width, table, true).ok_or(Eof::Data)?;
        match e.state {
            S_EOL if eol_ok => {
                st.eol_count = 1;
                return Ok(Some(false));
            }
            s if s == term => {
                row.set(runs, param(e))
                    .map_err(|Stop::Fail| Eof::Overflow)?;
                return Ok(Some(true));
            }
            s if s == make_up || s == S_MAKE_UP => {
                row.a0 = row.a0.wrapping_add(param(e));
                row.run_length = row.run_length.wrapping_add(param(e));
            }
            _ => return Ok(None),
        }
    }
}

/// The end of the data inside a row, or a run array that overflowed.
#[derive(Debug)]
enum Eof {
    Data,
    Overflow,
}

/// `EXPAND1D`.
fn expand_1d(
    ctx: &Ctx<'_>,
    st: &mut State<'_>,
    row: &mut Row,
    runs: &mut [u32],
) -> Result<End, Stop> {
    let eof = |row: &mut Row, runs: &mut [u32]| -> Result<End, Stop> {
        row.cleanup(runs)?;
        Ok(End::Eof)
    };
    loop {
        match run_of(
            st,
            row,
            runs,
            &ctx.tables.white,
            12,
            S_TERM_W,
            S_MAKE_UP_W,
            true,
        ) {
            Err(Eof::Data) => return eof(row, runs),
            Err(Eof::Overflow) => return Err(Stop::Fail),
            Ok(Some(true)) => {}
            // An end-of-line code, or a bad one: the row ends here.
            Ok(Some(false) | None) => {
                row.cleanup(runs)?;
                return Ok(End::Done);
            }
        }
        if row.a0 >= row.lastx {
            row.cleanup(runs)?;
            return Ok(End::Done);
        }
        match run_of(
            st,
            row,
            runs,
            &ctx.tables.black,
            13,
            S_TERM_B,
            S_MAKE_UP_B,
            true,
        ) {
            Err(Eof::Data) => return eof(row, runs),
            Err(Eof::Overflow) => return Err(Stop::Fail),
            Ok(Some(true)) => {}
            Ok(Some(false) | None) => {
                row.cleanup(runs)?;
                return Ok(End::Done);
            }
        }
        if row.a0 >= row.lastx {
            row.cleanup(runs)?;
            return Ok(End::Done);
        }
        // A white and black run both of nothing: drop them.
        let last = runs.get(row.pa.wrapping_sub(1)).copied();
        let before = runs.get(row.pa.wrapping_sub(2)).copied();
        if last == Some(0) && before == Some(0) {
            row.pa = row.pa.wrapping_sub(2);
        }
    }
}

/// The reference line's cursor (`pb`, `b1`).
struct Reference {
    pb: usize,
    b1: i32,
    end: usize,
}

impl Reference {
    fn next(&mut self, runs: &[u32]) -> Result<i32, Stop> {
        let v = runs.get(self.pb).copied().ok_or(Stop::Fail)?;
        self.pb = self.pb.wrapping_add(1);
        Ok(v.cast_signed())
    }

    /// `CHECK_b1`: move b1 to the first change on the reference line past
    /// a0 of the other colour.
    fn check(&mut self, row: &Row, runs: &[u32]) -> Result<(), Stop> {
        if row.pa != row.this {
            while self.b1 <= row.a0 && self.b1 < row.lastx {
                if self.pb.wrapping_add(1) >= self.end {
                    return Err(Stop::Fail);
                }
                let a = self.next(runs)?;
                let b = self.next(runs)?;
                self.b1 = self.b1.wrapping_add(a.wrapping_add(b));
            }
        }
        Ok(())
    }
}

/// `EXPAND2D`.
#[allow(clippy::too_many_lines)] // One `switch`, as in the C.
fn expand_2d(
    ctx: &Ctx<'_>,
    st: &mut State<'_>,
    row: &mut Row,
    runs: &mut [u32],
    rf: &mut Reference,
) -> Result<End, Stop> {
    let tables = ctx.tables;
    // The row's end, clean (`eol2d`), or at the end of the data (`eof2d`).
    macro_rules! eol {
        () => {{
            row.cleanup(runs)?;
            return Ok(End::Done);
        }};
    }
    macro_rules! eof {
        () => {{
            row.cleanup(runs)?;
            return Ok(End::Eof);
        }};
    }
    while row.a0 < row.lastx {
        if row.pa >= row.end {
            return Err(Stop::Fail);
        }
        let Some(e) = st.lookup(7, &tables.main, false) else {
            eof!()
        };
        match e.state {
            S_PASS => {
                rf.check(row, runs)?;
                if rf.pb.wrapping_add(1) >= rf.end {
                    return Err(Stop::Fail);
                }
                rf.b1 = rf.b1.wrapping_add(rf.next(runs)?);
                row.run_length = row.run_length.wrapping_add(rf.b1.wrapping_sub(row.a0));
                row.a0 = rf.b1;
                rf.b1 = rf.b1.wrapping_add(rf.next(runs)?);
            }
            S_HORIZ => {
                let black_first = row.pa.wrapping_sub(row.this) & 1 != 0;
                let order = if black_first {
                    [
                        (&tables.black, 13, S_TERM_B, S_MAKE_UP_B),
                        (&tables.white, 12, S_TERM_W, S_MAKE_UP_W),
                    ]
                } else {
                    [
                        (&tables.white, 12, S_TERM_W, S_MAKE_UP_W),
                        (&tables.black, 13, S_TERM_B, S_MAKE_UP_B),
                    ]
                };
                for (table, width, term, make_up) in order {
                    match run_of(st, row, runs, table, width, term, make_up, false) {
                        Err(Eof::Data) => eof!(),
                        Err(Eof::Overflow) => return Err(Stop::Fail),
                        Ok(Some(_)) => {}
                        Ok(None) => eol!(),
                    }
                }
                rf.check(row, runs)?;
            }
            S_V0 | S_VR => {
                rf.check(row, runs)?;
                let x = rf.b1.wrapping_sub(row.a0).wrapping_add(if e.state == S_VR {
                    param(e)
                } else {
                    0
                });
                row.set(runs, x)?;
                if rf.pb >= rf.end {
                    return Err(Stop::Fail);
                }
                rf.b1 = rf.b1.wrapping_add(rf.next(runs)?);
            }
            S_VL => {
                rf.check(row, runs)?;
                if rf.b1 < row.a0.wrapping_add(param(e)) {
                    eol!();
                }
                let x = rf.b1.wrapping_sub(row.a0).wrapping_sub(param(e));
                row.set(runs, x)?;
                rf.pb = rf.pb.wrapping_sub(1);
                let v = runs.get(rf.pb).copied().ok_or(Stop::Fail)?;
                rf.b1 = rf.b1.wrapping_sub(v.cast_signed());
            }
            S_EXT => {
                // Uncompressed mode, which libtiff does not read.
                let x = row.lastx.wrapping_sub(row.a0);
                row.push(runs, x)?;
                eol!();
            }
            S_EOL => {
                let x = row.lastx.wrapping_sub(row.a0);
                row.push(runs, x)?;
                if !st.need8(4) {
                    eof!();
                }
                st.clr(4);
                st.eol_count = 1;
                eol!();
            }
            _ => eol!(),
        }
    }
    if row.run_length != 0 {
        if row.run_length.wrapping_add(row.a0) < row.lastx {
            // A final V0 is expected.
            if !st.need8(1) {
                eof!();
            }
            if st.get(1) == 0 {
                eol!();
            }
            st.clr(1);
        }
        row.set(runs, 0)?;
    }
    eol!()
}

/// `Fax3Decode1D`.
fn decode_1d(
    ctx: &Ctx<'_>,
    mode: &mut u32,
    runs: &mut [u32],
    st: &mut State<'_>,
    out: &mut [u8],
) -> Result<(), Stop> {
    let mut at = 0usize;
    let this = 0usize;
    'retry: loop {
        st.rewind();
        while at < out.len() {
            let mut row = ctx.row(this);
            let mut retry = false;
            if !sync_eol(mode, st, &mut retry) {
                row.cleanup(runs)?;
                fill_row(
                    row_of(out, at, ctx.row_bytes)?,
                    runs,
                    this,
                    row.pa,
                    ctx.lastx,
                );
                return Err(Stop::Fail);
            }
            if retry {
                continue 'retry;
            }
            let end = expand_1d(ctx, st, &mut row, runs)?;
            fill_row(
                row_of(out, at, ctx.row_bytes)?,
                runs,
                this,
                row.pa,
                ctx.lastx,
            );
            if let End::Eof = end {
                return Err(Stop::Fail);
            }
            at = at.saturating_add(ctx.row_bytes);
        }
        return Ok(());
    }
}

/// `Fax3Decode2D`.
fn decode_2d(
    ctx: &Ctx<'_>,
    mode: &mut u32,
    runs: &mut [u32],
    st: &mut State<'_>,
    out: &mut [u8],
    refl: usize,
) -> Result<(), Stop> {
    let mut at = 0usize;
    let (mut cur, mut refl) = (0usize, refl);
    'retry: loop {
        st.rewind();
        while at < out.len() {
            let mut row = ctx.row(cur);
            let mut retry = false;
            let synced = sync_eol(mode, st, &mut retry);
            if retry {
                continue 'retry;
            }
            if !synced || !st.need8(1) {
                row.cleanup(runs)?;
                fill_row(
                    row_of(out, at, ctx.row_bytes)?,
                    runs,
                    cur,
                    row.pa,
                    ctx.lastx,
                );
                return Err(Stop::Fail);
            }
            let one_d = st.get(1) != 0;
            st.clr(1);
            let mut rf = Reference {
                pb: refl,
                b1: 0,
                end: refl.saturating_add(ctx.nruns),
            };
            rf.b1 = rf.next(runs)?;
            let end = if one_d {
                expand_1d(ctx, st, &mut row, runs)?
            } else {
                expand_2d(ctx, st, &mut row, runs, &mut rf)?
            };
            fill_row(
                row_of(out, at, ctx.row_bytes)?,
                runs,
                cur,
                row.pa,
                ctx.lastx,
            );
            if let End::Eof = end {
                return Err(Stop::Fail);
            }
            if row.pa < row.end {
                // An imaginary change, for the next row's reference.
                row.set(runs, 0)?;
            }
            core::mem::swap(&mut cur, &mut refl);
            at = at.saturating_add(ctx.row_bytes);
        }
        return Ok(());
    }
}

/// `Fax4Decode`.
fn decode_g4(
    ctx: &Ctx<'_>,
    runs: &mut [u32],
    st: &mut State<'_>,
    out: &mut [u8],
    refl: usize,
) -> Result<(), Stop> {
    let mut at = 0usize;
    let (mut cur, mut refl) = (0usize, refl);
    let mut rows = 0u32;
    let row_need = usize::try_from(ctx.lastx.wrapping_add(7) >> 3).unwrap_or(0);
    while at < out.len() {
        let mut row = ctx.row(cur);
        let mut rf = Reference {
            pb: refl,
            b1: 0,
            end: refl.saturating_add(ctx.nruns),
        };
        rf.b1 = rf.next(runs)?;
        let end = expand_2d(ctx, st, &mut row, runs, &mut rf)?;
        if matches!(end, End::Done) && st.eol_count == 0 {
            if row_need > out.len().saturating_sub(at) {
                return Err(Stop::Fail);
            }
            fill_row(
                row_of(out, at, ctx.row_bytes)?,
                runs,
                cur,
                row.pa,
                ctx.lastx,
            );
            row.set(runs, 0)?;
            core::mem::swap(&mut cur, &mut refl);
            at = at.saturating_add(ctx.row_bytes);
            rows = rows.saturating_add(1);
            continue;
        }
        // `EOFG4`: the end-of-block code, or the data's end. The row so
        // far is kept; a strip is badly ended, not bad, if a row came first.
        let _ = st.need16(13);
        st.clr(13);
        if row_need > out.len().saturating_sub(at) {
            return Err(Stop::Fail);
        }
        fill_row(
            row_of(out, at, ctx.row_bytes)?,
            runs,
            cur,
            row.pa,
            ctx.lastx,
        );
        return if rows > 0 { Ok(()) } else { Err(Stop::Fail) };
    }
    Ok(())
}

/// `Fax3DecodeRLE`: 1-D rows without end-of-line codes, each padded to a
/// byte or a 16-bit word.
fn decode_rle(
    ctx: &Ctx<'_>,
    mode: u32,
    runs: &mut [u32],
    st: &mut State<'_>,
    out: &mut [u8],
    word_aligned: bool,
    offset: u64,
) -> Result<(), Stop> {
    let mut at = 0usize;
    let this = 0usize;
    while at < out.len() {
        let mut row = ctx.row(this);
        let end = expand_1d(ctx, st, &mut row, runs)?;
        fill_row(
            row_of(out, at, ctx.row_bytes)?,
            runs,
            this,
            row.pa,
            ctx.lastx,
        );
        if let End::Eof = end {
            return Err(Stop::Fail);
        }
        let _ = word_aligned;
        if mode & MODE_BYTEALIGN != 0 {
            let n = st.avail.wrapping_sub(st.avail & !7);
            st.clr(n);
        } else if mode & MODE_WORDALIGN != 0 {
            let n = st.avail.wrapping_sub(st.avail & !15);
            st.clr(n);
            // The C tests the address of its next byte, which in the
            // memory-mapped file is the strip's offset plus the bytes read.
            if st.avail == 0 && (offset.wrapping_add(st.cp as u64)) & 1 != 0 {
                st.cp = st.cp.saturating_add(1);
            }
        }
        at = at.saturating_add(ctx.row_bytes);
    }
    Ok(())
}

/// `_TIFFFax3fillruns`: white runs clear bits, black runs set them, most
/// significant bit first; runs past the row are cut, in the array itself.
fn fill_row(buf: &mut [u8], runs: &mut [u32], this: usize, mut erun: usize, lastx: i32) {
    let lastx = lastx.cast_unsigned();
    if erun.wrapping_sub(this) & 1 != 0 {
        if let Some(r) = runs.get_mut(erun) {
            *r = 0;
        }
        erun = erun.wrapping_add(1);
    }
    let mut x = 0u32;
    let mut i = this;
    while i < erun {
        for (k, black) in [(i, false), (i.wrapping_add(1), true)] {
            let Some(slot) = runs.get_mut(k) else { return };
            let mut run = *slot;
            if x.wrapping_add(run) > lastx || run > lastx {
                run = lastx.wrapping_sub(x);
                *slot = run;
            }
            if run != 0 {
                paint(buf, x, run, black);
                x = x.wrapping_add(*slot);
            }
        }
        i = i.wrapping_add(2);
    }
}

/// Clear (white) or set (black) bits `x .. x + n` of a row.
fn paint(buf: &mut [u8], x: u32, n: u32, black: bool) {
    let mut bit = x as usize;
    let end = bit.saturating_add(n as usize);
    while bit < end {
        let byte = bit >> 3;
        let within = bit & 7;
        // At most 8, and at least 1 since bit < end.
        let take = 8usize.wrapping_sub(within).min(end.wrapping_sub(bit));
        // `take` bits from `within`, most significant first.
        let mask = u8::try_from((0xFF00u16 >> take) & 0xFF).unwrap_or(0) >> within;
        if let Some(b) = buf.get_mut(byte) {
            if black {
                *b |= mask;
            } else {
                *b &= !mask;
            }
        }
        bit = bit.wrapping_add(take);
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use super::*;

    #[test]
    fn the_tables_are_libtiffs() {
        // Spot checks against tif_fax3sm.c, which mkg3states wrote.
        let t = Tables::new();
        let e = |x: Ent| (x.state, x.width, x.param);
        assert_eq!(e(t.main[0]), (12, 7, 0));
        assert_eq!(e(t.main[1]), (3, 1, 0));
        assert_eq!(e(t.main[2]), (5, 3, 1));
        assert_eq!(e(t.main[8]), (1, 4, 0));
        assert_eq!(e(t.white[0]), (12, 11, 0));
        assert_eq!(e(t.white[1]), (7, 4, 3));
        assert_eq!(e(t.white[6]), (9, 6, 1664));
        assert_eq!(e(t.white[128]), (11, 11, 1792));
        assert_eq!(e(t.white[256]), (0, 0, 0));
    }

    #[test]
    fn painting_sets_and_clears_exact_bits() {
        let mut row = [0x00u8, 0xFF, 0x00];
        paint(&mut row, 3, 7, true);
        assert_eq!(row, [0b0001_1111, 0xFF, 0x00]);
        paint(&mut row, 6, 12, false);
        assert_eq!(row, [0b0001_1100, 0x00, 0x00]);
    }
}
