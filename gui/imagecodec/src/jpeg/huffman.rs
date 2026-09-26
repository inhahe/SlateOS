//! Huffman entropy decoding: libjpeg-turbo's `jdhuff.c` (sequential) and
//! `jdphuff.c` (progressive), and the bit reader they share.
//!
//! The reader is `jpeg_fill_bit_buffer`: it loads bytes until it holds 57
//! bits, turns `FF 00` into `FF`, and stops at any other marker, leaving its
//! code for the marker reader. From then on it supplies zero bits, and the
//! first time it has to -- a request the bits it holds cannot meet -- the scan
//! is marked short of data (`insufficient_data`), after which the sequential
//! and first-pass decoders stop decoding and leave their blocks as they are:
//! zero, which is mid-grey, for the rest of the scan or until a restart marker
//! resynchronises it. That is how a cut-off JPEG looks in every program built
//! on libjpeg, and it is reproduced by reproducing the reader, not by
//! detecting the cut.
//!
//! Codes are read with libjpeg's eight-bit lookahead table and its
//! bit-at-a-time fallback (`jpeg_huff_decode`), so a code the table does not
//! have reads seventeen bits and yields zero, as there. libjpeg-turbo also
//! has a "fast" path used when plenty of input remains; it hands any MCU that
//! meets a marker back to the path transcribed here and redoes it, so it
//! computes the same thing and is not reproduced.
//!
//! What is done here for speed leaves that path's every step where it was:
//! a fill takes the bytes the loop would take one at a time at once when
//! none of them is `0xFF` (`bulk_fill`), and the sequential decoder keeps the
//! buffer in locals for an MCU (`Window`), as libjpeg's `BITREAD_STATE_VARS`
//! do. Each fill still happens when libjpeg's does and ends where it ends, so
//! the input position -- which a source that fails when its data runs out,
//! old-style JPEG in TIFF, makes visible -- is libjpeg's at every step.

use super::coef::Coefficients;
use super::error::{Error, jerr};
use super::marker::{Header, Input, read_restart_marker};
use super::tables::{HuffSpec, Tables, natural};

/// `MIN_GET_BITS` for a 64-bit bit buffer.
const MIN_GET_BITS: i32 = 57;

/// A Huffman table ready for decoding (`d_derived_tbl`).
#[derive(Clone)]
pub(super) struct Derived {
    /// Largest code of each length, -1 for none; `[17]` is a sentinel.
    maxcode: [i64; 18],
    /// For a code of length `l`, `values[code + valoffset[l]]` is its symbol.
    valoffset: [i64; 18],
    /// By the next eight bits: `(length << 8) | symbol`, or `9 << 8` when the
    /// code is longer than eight bits.
    lookup: [u16; 256],
    values: [u8; 256],
}

/// `jpeg_make_d_derived_tbl`, with its validation.
pub(super) fn derive(
    tables: &Tables,
    is_dc: bool,
    number: u8,
    lossless: bool,
) -> Result<Derived, Error> {
    let slots = if is_dc { &tables.dc } else { &tables.ac };
    let spec: &HuffSpec = slots
        .get(usize::from(number))
        .and_then(Option::as_ref)
        .ok_or(jerr::NO_HUFF_TABLE)?;
    // Figure C.1: the length of each symbol's code, in symbol order.
    let mut sizes = [0u8; 257];
    let mut count = 0usize;
    for (length, &n) in (1u8..).zip(spec.bits.iter().skip(1)) {
        if count.saturating_add(usize::from(n)) > 256 {
            return Err(jerr::BAD_HUFF_TABLE);
        }
        for _ in 0..n {
            if let Some(slot) = sizes.get_mut(count) {
                *slot = length;
            }
            count = count.saturating_add(1);
        }
    }
    // Figure C.2: the codes themselves, checking they form a real code.
    let mut codes = [0u32; 257];
    let mut code = 0u32;
    let mut size = sizes.first().copied().unwrap_or(0);
    let mut p = 0usize;
    while sizes.get(p).copied().unwrap_or(0) != 0 {
        while sizes.get(p).copied().unwrap_or(0) == size {
            if let Some(slot) = codes.get_mut(p) {
                *slot = code;
            }
            p = p.saturating_add(1);
            code = code.wrapping_add(1);
        }
        if u64::from(code) >= 1u64 << size {
            return Err(jerr::BAD_HUFF_TABLE);
        }
        code <<= 1;
        size = size.saturating_add(1);
    }
    // Figure F.15: the tables for decoding a bit at a time.
    let mut maxcode = [-1i64; 18];
    let mut valoffset = [0i64; 18];
    let mut p = 0usize;
    for (l, &n) in spec.bits.iter().enumerate().skip(1) {
        if n != 0 {
            let first = i64::from(codes.get(p).copied().unwrap_or(0));
            if let Some(slot) = valoffset.get_mut(l) {
                *slot = (p as i64).wrapping_sub(first);
            }
            p = p.saturating_add(usize::from(n));
            if let Some(slot) = maxcode.get_mut(l) {
                *slot = i64::from(codes.get(p.wrapping_sub(1)).copied().unwrap_or(0));
            }
        }
    }
    maxcode[17] = 0xFFFFF;
    valoffset[17] = 0;
    // The lookahead table.
    let mut lookup = [9u16 << 8; 256];
    let mut p = 0usize;
    for (l, &n) in spec.bits.iter().enumerate().skip(1).take(8) {
        for _ in 0..n {
            let code = codes.get(p).copied().unwrap_or(0);
            let symbol = u16::from(spec.values.get(p).copied().unwrap_or(0));
            let shift = 8u32.saturating_sub(l as u32);
            let first = (code << shift) as usize;
            for slot in lookup.iter_mut().skip(first).take(1usize << shift) {
                *slot = ((l as u16) << 8) | symbol;
            }
            p = p.saturating_add(1);
        }
    }
    // A DC table's symbols are bit counts, at most 15 (16 lossless).
    if is_dc {
        let limit = if lossless { 16 } else { 15 };
        if spec.values.iter().take(count).any(|&symbol| symbol > limit) {
            return Err(jerr::BAD_HUFF_TABLE);
        }
    }
    Ok(Derived {
        maxcode,
        valoffset,
        lookup,
        values: spec.values,
    })
}

/// The bit buffer and the out-of-data flag: libjpeg's `bitread_perm_state`
/// and `insufficient_data`.
#[derive(Debug, Clone, Default)]
pub(super) struct Bits {
    buffer: u64,
    left: i32,
    /// Set the first time zero bits had to be made up after a marker.
    pub(super) insufficient: bool,
}

impl Bits {
    /// Discard what is buffered, as a restart or a new scan does.
    pub(super) const fn reset(&mut self) {
        self.buffer = 0;
        self.left = 0;
    }

    /// `jpeg_fill_bit_buffer`: load at least `nbits`, stuffing zeros past a
    /// marker.
    ///
    /// The bytes libjpeg's loop takes one at a time -- as many as bring the
    /// buffer to 57 bits or more, which from `left` bits is exactly
    /// `(64 - left) / 8` of them -- are taken at once when they are all
    /// data: none of them `0xFF`, the one byte that can begin a marker or a
    /// stuffed `FF 00`. The buffer and the input position come out as the
    /// loop leaves them, so nothing downstream can tell; anything else --
    /// an `FF`, the last eight bytes of the input, a marker already met --
    /// goes through the loop.
    #[inline(never)]
    fn fill(&mut self, input: &mut Input<'_>, nbits: i32) {
        if let Some(window) = bulk_fill(self.window(), input) {
            self.keep(window);
            return;
        }
        if input.unread_marker == 0 {
            while self.left < MIN_GET_BITS {
                let mut c = input.src.byte();
                if c == 0xFF {
                    loop {
                        c = input.src.byte();
                        if c != 0xFF {
                            break;
                        }
                    }
                    if c == 0 {
                        c = 0xFF;
                    } else {
                        input.unread_marker = c;
                        self.stuff(input, nbits);
                        return;
                    }
                }
                self.buffer = (self.buffer << 8) | u64::from(c);
                self.left = self.left.wrapping_add(8);
            }
        } else {
            self.stuff(input, nbits);
        }
    }

    /// The buffer and its count, to be worked on in locals.
    #[inline(always)]
    const fn window(&self) -> Window {
        Window {
            buffer: self.buffer,
            left: self.left,
        }
    }

    /// Take back a window worked on in locals.
    #[inline(always)]
    const fn keep(&mut self, window: Window) {
        self.buffer = window.buffer;
        self.left = window.left;
    }

    /// [`Self::fill`] for a window held in locals, out of line: the part a
    /// decoder's inner loop reaches only at a `0xFF`, near the end of the
    /// input, or past a marker.
    #[inline(never)]
    fn refill(&mut self, window: Window, input: &mut Input<'_>, nbits: i32) -> Window {
        self.keep(window);
        self.fill(input, nbits);
        self.window()
    }

    /// [`Self::slow_decode`] for a window held in locals.
    #[cold]
    #[inline(never)]
    fn slow_decode_window(
        &mut self,
        window: Window,
        input: &mut Input<'_>,
        table: &Derived,
        min_bits: i32,
    ) -> (Window, i32) {
        self.keep(window);
        let symbol = self.slow_decode(input, table, min_bits);
        (self.window(), symbol)
    }

    /// The `no_more_bytes` half of `jpeg_fill_bit_buffer`.
    fn stuff(&mut self, input: &mut Input<'_>, nbits: i32) {
        if nbits > self.left {
            if !self.insufficient {
                // JWRN_HIT_MARKER.
                input.warn();
                self.insufficient = true;
            }
            let shift = MIN_GET_BITS.wrapping_sub(self.left).clamp(0, 63) as u32;
            self.buffer <<= shift;
            self.left = MIN_GET_BITS;
        }
    }

    /// `CHECK_BIT_BUFFER` then `GET_BITS`: the next `n` bits (at most 16).
    ///
    /// After the check `left` is at least `n` -- a fill brings it to 57 or
    /// more, a marker's zero bits to 57 -- so the shifts below are by 0 to
    /// 63, and `wrapping_shr` is the plain shift.
    #[inline(always)]
    pub(super) fn get(&mut self, input: &mut Input<'_>, n: i32) -> i32 {
        if self.left < n {
            self.fill(input, n);
        }
        self.left = self.left.wrapping_sub(n);
        let mask = 1u64.wrapping_shl(n as u32).wrapping_sub(1);
        (self.buffer.wrapping_shr(self.left as u32) & mask) as i32
    }

    /// `HUFF_DECODE`: one symbol.
    #[inline(always)]
    pub(super) fn decode(&mut self, input: &mut Input<'_>, table: &Derived) -> i32 {
        let mut nb = 1;
        if self.left < 8 {
            self.fill(input, 0);
        }
        if self.left >= 8 {
            let look = (self.buffer.wrapping_shr(self.left.wrapping_sub(8) as u32) & 0xFF) as usize;
            let entry = table.lookup.get(look).copied().unwrap_or(9 << 8);
            nb = i32::from(entry >> 8);
            if nb <= 8 {
                self.left = self.left.wrapping_sub(nb);
                return i32::from(entry & 0xFF);
            }
        }
        self.slow_decode(input, table, nb)
    }

    /// `jpeg_huff_decode`: a code at least `min_bits` long, a bit at a time.
    #[cold]
    #[inline(never)]
    fn slow_decode(&mut self, input: &mut Input<'_>, table: &Derived, min_bits: i32) -> i32 {
        let mut l = min_bits;
        let mut code = i64::from(self.get(input, l));
        while code > table.maxcode.get(l as usize).copied().unwrap_or(0xFFFFF) {
            code = (code << 1) | i64::from(self.get(input, 1));
            l = l.wrapping_add(1);
        }
        if l > 16 {
            // JWRN_HUFF_BAD_CODE: the safest thing is a zero.
            input.warn();
            return 0;
        }
        let at = code.wrapping_add(table.valoffset.get(l as usize).copied().unwrap_or(0));
        usize::try_from(at)
            .ok()
            .and_then(|at| table.values.get(at))
            .map_or(0, |&v| i32::from(v))
    }
}

/// The bit buffer as a decoder's inner loop holds it -- in locals, so in
/// registers: libjpeg's `BITREAD_STATE_VARS`, which exist for the same
/// reason. [`Bits`] is where it lives between MCUs, and what the rare work
/// done out of line (a fill that meets an `0xFF`, a code longer than eight
/// bits) is handed. The methods are [`Bits`]'s, with the same fills at the
/// same moments, so the two read the same bytes when.
#[derive(Debug, Clone, Copy)]
pub(super) struct Window {
    buffer: u64,
    left: i32,
}

impl Window {
    /// A fill, for `nbits`: in place if the bytes are all data, otherwise
    /// by way of [`Bits::refill`].
    #[inline(always)]
    fn fill(&mut self, bits: &mut Bits, input: &mut Input<'_>, nbits: i32) {
        if let Some(window) = bulk_fill(*self, input) {
            *self = window;
        } else {
            *self = bits.refill(*self, input, nbits);
        }
    }

    /// [`Bits::get`].
    #[inline(always)]
    pub(super) fn get(&mut self, bits: &mut Bits, input: &mut Input<'_>, n: i32) -> i32 {
        if self.left < n {
            self.fill(bits, input, n);
        }
        self.left = self.left.wrapping_sub(n);
        let mask = 1u64.wrapping_shl(n as u32).wrapping_sub(1);
        (self.buffer.wrapping_shr(self.left as u32) & mask) as i32
    }

    /// [`Bits::decode`].
    #[inline(always)]
    pub(super) fn decode(
        &mut self,
        bits: &mut Bits,
        input: &mut Input<'_>,
        table: &Derived,
    ) -> i32 {
        let mut nb = 1;
        if self.left < 8 {
            self.fill(bits, input, 0);
        }
        if self.left >= 8 {
            let look = (self.buffer.wrapping_shr(self.left.wrapping_sub(8) as u32) & 0xFF) as usize;
            let entry = table.lookup.get(look).copied().unwrap_or(9 << 8);
            nb = i32::from(entry >> 8);
            if nb <= 8 {
                self.left = self.left.wrapping_sub(nb);
                return i32::from(entry & 0xFF);
            }
        }
        let (window, symbol) = bits.slow_decode_window(*self, input, table, nb);
        *self = window;
        symbol
    }
}

/// The part of `jpeg_fill_bit_buffer` that can be done at once: the bytes
/// its loop would take one at a time -- as many as bring the buffer to 57
/// bits or more, which from `left` bits is exactly `(64 - left) / 8` of them
/// -- taken together when they are all data, none of them `0xFF`, the one
/// byte that can begin a marker or a stuffed `FF 00`. `None`, having taken
/// nothing, where the loop is needed: an `FF`, the last eight bytes of the
/// input, or a marker already met.
#[inline(always)]
fn bulk_fill(window: Window, input: &mut Input<'_>) -> Option<Window> {
    if input.unread_marker != 0 {
        return None;
    }
    let word = input.src.peek8()?;
    // `left` is below 57 here (a fill is only asked for when a read needs
    // more than it holds, at most 16 bits), so this is 6, 7 or 8.
    let take = (64u32.saturating_sub(window.left.clamp(0, 64) as u32)) / 8;
    let bytes = if take >= 8 {
        word
    } else {
        word >> 64u32.wrapping_sub(take.wrapping_mul(8))
    };
    if has_ff_byte(bytes, take) {
        return None;
    }
    input.src.advance(take as usize);
    Some(Window {
        buffer: if take >= 8 {
            bytes
        } else {
            (window.buffer << take.wrapping_mul(8)) | bytes
        },
        left: window.left.wrapping_add(take.wrapping_mul(8) as i32),
    })
}

/// Whether any of the low `count` bytes of `word` is `0xFF`: the classic
/// test for a zero byte, on the complement. The bytes above `count` are zero
/// in `word`, so `0xFF` in its complement, and never count.
#[inline]
const fn has_ff_byte(word: u64, count: u32) -> bool {
    let flipped = !word;
    let found = flipped.wrapping_sub(0x0101_0101_0101_0101) & !flipped & 0x8080_8080_8080_8080;
    // Only the low `count` bytes are the ones being taken.
    let keep = if count >= 8 {
        u64::MAX
    } else {
        (1u64 << count.wrapping_mul(8)).wrapping_sub(1)
    };
    found & keep != 0
}

/// `HUFF_EXTEND`: the `s`-bit value `x` as a signed difference (1 <= s <= 16).
///
/// libjpeg-turbo's branch-free form: the sign of `x - 2^(s-1)` selects
/// whether `1 - 2^s` is added. Which way it goes is as unpredictable as the
/// picture, so a branch here would be mispredicted about half the time.
#[inline]
pub(super) const fn extend(x: i32, s: i32) -> i32 {
    if s <= 0 || s > 16 {
        return x;
    }
    let below = x.wrapping_sub(1 << (s.wrapping_sub(1))) >> 31;
    x.wrapping_add(below & (-1i32 << s).wrapping_add(1))
}

/// The derived tables a scan's blocks use, and the per-scan state libjpeg
/// keeps in its entropy decoder.
#[derive(Clone)]
pub(super) struct Sequential {
    dc: [Option<Derived>; 4],
    ac: [Option<Derived>; 4],
    /// For each block of the MCU: its component's position in the scan,
    /// whether its AC coefficients are wanted (not at an eighth scale), and
    /// its component's DC and AC table numbers.
    blocks: [Block; 10],
    blocks_in_mcu: usize,
    last_dc: [i32; 4],
    restarts_to_go: u32,
    pub(super) bits: Bits,
}

impl Sequential {
    /// `start_pass_huff_decoder`.
    pub(super) fn start(
        input: &mut Input<'_>,
        header: &Header,
        tables: &Tables,
        membership: &[usize],
    ) -> Result<Self, Error> {
        let scan = &header.scan;
        if scan.ss != 0 || scan.se != 63 || scan.ah != 0 || scan.al != 0 {
            // JWRN_NOT_SEQUENTIAL.
            input.warn();
        }
        let mut dc: [Option<Derived>; 4] = Default::default();
        let mut ac: [Option<Derived>; 4] = Default::default();
        for &ci in scan.components() {
            let component = header.components.get(ci).ok_or(jerr::BAD_COMPONENT_ID)?;
            let derived = derive(tables, true, component.dc_tbl_no, false)?;
            if let Some(slot) = dc.get_mut(usize::from(component.dc_tbl_no)) {
                *slot = Some(derived);
            }
            let derived = derive(tables, false, component.ac_tbl_no, false)?;
            if let Some(slot) = ac.get_mut(usize::from(component.ac_tbl_no)) {
                *slot = Some(derived);
            }
        }
        let mut blocks = [Block::default(); 10];
        for (slot, &position) in blocks.iter_mut().zip(membership) {
            let ci = scan.comps.get(position).copied().unwrap_or(0);
            let component = header.components.get(ci);
            let scaled = component.map_or(8, |c| c.dct_scaled_size);
            let (dc_no, ac_no) = component.map_or((0, 0), |c| (c.dc_tbl_no, c.ac_tbl_no));
            *slot = Block {
                position,
                ac_wanted: scaled > 1,
                dc: usize::from(dc_no),
                ac: usize::from(ac_no),
            };
        }
        Ok(Self {
            dc,
            ac,
            blocks,
            blocks_in_mcu: membership.len(),
            last_dc: [0; 4],
            restarts_to_go: u32::from(header.restart_interval),
            bits: Bits::default(),
        })
    }

    /// `process_restart`.
    fn restart(&mut self, input: &mut Input<'_>, interval: u16) {
        self.bits.reset();
        read_restart_marker(input);
        self.last_dc = [0; 4];
        self.restarts_to_go = u32::from(interval);
        if input.unread_marker == 0 {
            self.bits.insufficient = false;
        }
    }

    /// `decode_mcu`: one MCU's coefficients into `blocks`, which the caller
    /// has zeroed, in natural order and not dequantised.
    pub(super) fn decode_mcu(
        &mut self,
        input: &mut Input<'_>,
        header: &Header,
        blocks: &mut [[i16; 64]],
    ) {
        let interval = header.restart_interval;
        if interval != 0 && self.restarts_to_go == 0 {
            self.restart(input, interval);
        }
        if !self.bits.insufficient {
            let bits = &mut self.bits;
            let mut w = bits.window();
            for (member, block) in self
                .blocks
                .iter()
                .zip(blocks.iter_mut())
                .take(self.blocks_in_mcu)
            {
                let (Some(Some(dc)), Some(Some(ac))) =
                    (self.dc.get(member.dc), self.ac.get(member.ac))
                else {
                    continue;
                };
                // F.2.2.1: the DC difference.
                let mut s = w.decode(bits, input, dc);
                if s != 0 {
                    let r = w.get(bits, input, s);
                    s = extend(r, s);
                }
                if let Some(last) = self.last_dc.get_mut(member.position) {
                    s = s.wrapping_add(*last);
                    *last = s;
                }
                block[0] = s as i16;
                // F.2.2.2: the AC coefficients.
                let mut k = 1usize;
                if member.ac_wanted {
                    while k < 64 {
                        let symbol = w.decode(bits, input, ac);
                        let run = (symbol >> 4) as usize;
                        let size = symbol & 15;
                        if size != 0 {
                            k = k.wrapping_add(run);
                            let r = w.get(bits, input, size);
                            // `natural` is below 64 for any `k`; the mask
                            // says so where the compiler can see it.
                            if let Some(cell) = block.get_mut(natural(k) & 63) {
                                *cell = extend(r, size) as i16;
                            }
                        } else {
                            if run != 15 {
                                break;
                            }
                            k = k.wrapping_add(15);
                        }
                        k = k.wrapping_add(1);
                    }
                } else {
                    while k < 64 {
                        let symbol = w.decode(bits, input, ac);
                        let run = (symbol >> 4) as usize;
                        let size = symbol & 15;
                        if size != 0 {
                            k = k.wrapping_add(run);
                            w.get(bits, input, size);
                        } else {
                            if run != 15 {
                                break;
                            }
                            k = k.wrapping_add(15);
                        }
                        k = k.wrapping_add(1);
                    }
                }
            }
            bits.keep(w);
        }
        if interval != 0 {
            self.restarts_to_go = self.restarts_to_go.wrapping_sub(1);
        }
    }
}

/// One block of a sequential scan's MCU, as [`Sequential::decode_mcu`] needs
/// it: worked out when the scan starts rather than looked up per block.
#[derive(Debug, Clone, Copy, Default)]
struct Block {
    /// Its component's position in the scan, which keys the DC predictor.
    position: usize,
    /// Whether its AC coefficients are kept (not at an eighth scale).
    ac_wanted: bool,
    /// Its component's DC and AC table numbers.
    dc: usize,
    ac: usize,
}

/// Which of the four progressive decoders a scan uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Pass {
    DcFirst,
    AcFirst,
    DcRefine,
    AcRefine,
}

impl Pass {
    /// `start_pass_phuff_decoder`'s validation (shared with the arithmetic
    /// decoder), and which decoder the scan gets.
    pub(super) fn of(header: &Header) -> Result<Self, Error> {
        let scan = &header.scan;
        let dc_band = scan.ss == 0;
        let mut bad = if dc_band {
            scan.se != 0
        } else {
            scan.ss > scan.se || scan.se >= 64 || scan.count != 1
        };
        if scan.ah != 0 && scan.al != scan.ah.wrapping_sub(1) {
            bad = true;
        }
        if scan.al > 13 {
            bad = true;
        }
        if bad {
            return Err(jerr::BAD_PROGRESSION);
        }
        Ok(match (dc_band, scan.ah == 0) {
            (true, true) => Self::DcFirst,
            (false, true) => Self::AcFirst,
            (true, false) => Self::DcRefine,
            (false, false) => Self::AcRefine,
        })
    }
}

/// libjpeg's progression status (`coef_bits`): for each component, the `Al`
/// each coefficient is known to (-1 before any scan), and the same as it was
/// before the latest scan that touched it.
#[derive(Debug, Clone)]
pub(super) struct Progression {
    pub(super) bits: alloc::vec::Vec<[i32; 64]>,
    pub(super) previous: alloc::vec::Vec<[i32; 64]>,
}

impl Progression {
    pub(super) fn new(components: usize) -> Self {
        Self {
            bits: alloc::vec![[-1; 64]; components],
            previous: alloc::vec![[0; 64]; components],
        }
    }

    /// The progression-status update at the start of a progressive scan,
    /// with its warnings for inconsistent scans.
    #[allow(
        clippy::indexing_slicing,
        reason = "coefficient indices are clamped to 63 before they index a block's 64"
    )]
    pub(super) fn update(&mut self, input: &mut Input<'_>, header: &Header) {
        let scan = &header.scan;
        let dc_band = scan.ss == 0;
        for &ci in scan.components() {
            let (Some(bits), Some(previous)) = (self.bits.get_mut(ci), self.previous.get_mut(ci))
            else {
                continue;
            };
            if !dc_band && bits[0] < 0 {
                // JWRN_BOGUS_PROGRESSION: AC without a prior DC scan.
                input.warn();
            }
            let from = usize::from(scan.ss.min(1));
            let to = usize::from(scan.se.max(9));
            for coefficient in from..=to.min(63) {
                previous[coefficient] = if header.input_scan_number > 1 {
                    bits[coefficient]
                } else {
                    0
                };
            }
            for coefficient in usize::from(scan.ss)..=usize::from(scan.se).min(63) {
                let expected = bits[coefficient].max(0);
                if i32::from(scan.ah) != expected {
                    input.warn();
                }
                bits[coefficient] = i32::from(scan.al);
            }
        }
    }
}

/// A progressive Huffman scan (`phuff_entropy_decoder`).
#[derive(Clone)]
pub(super) struct Progressive {
    pass: Pass,
    /// By table number: DC tables for a DC scan, the AC table for an AC scan.
    tables: [Option<Derived>; 4],
    /// Each block's component's position in the scan.
    positions: [usize; 10],
    blocks_in_mcu: usize,
    last_dc: [i32; 4],
    eobrun: u32,
    restarts_to_go: u32,
    pub(super) bits: Bits,
}

impl Progressive {
    /// `start_pass_phuff_decoder`, after the validation and the progression
    /// update ([`Pass::of`], [`Progression::update`]).
    pub(super) fn start(
        pass: Pass,
        header: &Header,
        tables: &Tables,
        membership: &[usize],
    ) -> Result<Self, Error> {
        let scan = &header.scan;
        let mut derived: [Option<Derived>; 4] = Default::default();
        for &ci in scan.components() {
            let component = header.components.get(ci).ok_or(jerr::BAD_COMPONENT_ID)?;
            match pass {
                Pass::DcFirst => {
                    let table = derive(tables, true, component.dc_tbl_no, false)?;
                    if let Some(slot) = derived.get_mut(usize::from(component.dc_tbl_no)) {
                        *slot = Some(table);
                    }
                }
                Pass::AcFirst | Pass::AcRefine => {
                    let table = derive(tables, false, component.ac_tbl_no, false)?;
                    if let Some(slot) = derived.get_mut(usize::from(component.ac_tbl_no)) {
                        *slot = Some(table);
                    }
                }
                Pass::DcRefine => {}
            }
        }
        let mut positions = [0usize; 10];
        for (slot, &position) in positions.iter_mut().zip(membership) {
            *slot = position;
        }
        Ok(Self {
            pass,
            tables: derived,
            positions,
            blocks_in_mcu: membership.len(),
            last_dc: [0; 4],
            eobrun: 0,
            restarts_to_go: u32::from(header.restart_interval),
            bits: Bits::default(),
        })
    }

    /// `process_restart`.
    fn restart(&mut self, input: &mut Input<'_>, interval: u16) {
        self.bits.reset();
        read_restart_marker(input);
        self.last_dc = [0; 4];
        self.eobrun = 0;
        self.restarts_to_go = u32::from(interval);
        if input.unread_marker == 0 {
            self.bits.insufficient = false;
        }
    }

    /// One MCU into `blocks`, which hold the coefficients so far.
    pub(super) fn decode_mcu(
        &mut self,
        input: &mut Input<'_>,
        header: &Header,
        blocks: &mut [[i16; 64]],
    ) -> Result<(), Error> {
        let interval = header.restart_interval;
        if interval != 0 && self.restarts_to_go == 0 {
            self.restart(input, interval);
        }
        match self.pass {
            Pass::DcFirst => self.dc_first(input, header, blocks)?,
            Pass::DcRefine => self.dc_refine(input, header, blocks),
            Pass::AcFirst | Pass::AcRefine => {
                if let Some(block) = blocks.first_mut() {
                    self.ac(input, header, block);
                }
            }
        }
        if interval != 0 {
            self.restarts_to_go = self.restarts_to_go.wrapping_sub(1);
        }
        Ok(())
    }

    /// One MCU of an AC scan -- one block, of one component -- decoded into
    /// `block` wherever it lies: [`Self::decode_mcu`] for the scans the
    /// coefficient store decodes in place.
    pub(super) fn decode_ac<B: Coefficients>(
        &mut self,
        input: &mut Input<'_>,
        header: &Header,
        block: &mut B,
    ) {
        let interval = header.restart_interval;
        if interval != 0 && self.restarts_to_go == 0 {
            self.restart(input, interval);
        }
        self.ac(input, header, block);
        if interval != 0 {
            self.restarts_to_go = self.restarts_to_go.wrapping_sub(1);
        }
    }

    /// The AC pass this scan is.
    fn ac<B: Coefficients>(&mut self, input: &mut Input<'_>, header: &Header, block: &mut B) {
        match self.pass {
            Pass::AcFirst => self.ac_first(input, header, block),
            Pass::AcRefine => self.ac_refine(input, header, block),
            Pass::DcFirst | Pass::DcRefine => {}
        }
    }

    /// `decode_mcu_DC_first`.
    fn dc_first(
        &mut self,
        input: &mut Input<'_>,
        header: &Header,
        blocks: &mut [[i16; 64]],
    ) -> Result<(), Error> {
        if self.bits.insufficient {
            return Ok(());
        }
        let scan = &header.scan;
        for (&position, block) in self
            .positions
            .iter()
            .zip(blocks.iter_mut())
            .take(self.blocks_in_mcu)
        {
            let ci = scan.comps.get(position).copied().unwrap_or(0);
            let number = header.components.get(ci).map_or(0, |c| c.dc_tbl_no);
            let Some(table) = self
                .tables
                .get(usize::from(number))
                .and_then(Option::as_ref)
            else {
                continue;
            };
            let mut s = self.bits.decode(input, table);
            if s != 0 {
                let r = self.bits.get(input, s);
                s = extend(r, s);
            }
            let Some(last) = self.last_dc.get_mut(position) else {
                continue;
            };
            s = last.checked_add(s).ok_or(jerr::BAD_DCT_COEF)?;
            *last = s;
            block[0] = left_shift(s, scan.al);
        }
        Ok(())
    }

    /// `decode_mcu_AC_first`.
    fn ac_first<B: Coefficients>(&mut self, input: &mut Input<'_>, header: &Header, block: &mut B) {
        if self.bits.insufficient {
            return;
        }
        if self.eobrun > 0 {
            self.eobrun = self.eobrun.wrapping_sub(1);
            return;
        }
        let scan = &header.scan;
        let ci = scan.comps.first().copied().unwrap_or(0);
        let number = header.components.get(ci).map_or(0, |c| c.ac_tbl_no);
        // Borrowed from its field rather than through a method on `self`, so
        // the bit reader can be borrowed beside it: a table is a kilobyte,
        // and copying it for every block of every AC scan cost a large
        // progressive photograph gigabytes of copying.
        let Some(table) = self
            .tables
            .get(usize::from(number))
            .and_then(Option::as_ref)
        else {
            return;
        };
        let mut k = usize::from(scan.ss);
        let se = usize::from(scan.se);
        while k <= se {
            let symbol = self.bits.decode(input, table);
            let run = symbol >> 4;
            let size = symbol & 15;
            if size != 0 {
                k = k.saturating_add(run as usize);
                let r = self.bits.get(input, size);
                block.set(natural(k), left_shift(extend(r, size), scan.al));
            } else if run == 15 {
                k = k.saturating_add(15);
            } else {
                let mut eobrun = 1u32 << run;
                if run != 0 {
                    eobrun = eobrun.wrapping_add(self.bits.get(input, run) as u32);
                }
                self.eobrun = eobrun.wrapping_sub(1);
                break;
            }
            k = k.saturating_add(1);
        }
    }

    /// `decode_mcu_DC_refine`: one more bit of each DC. It reads on past the
    /// end of the data as libjpeg does -- the bits are then zeros and change
    /// nothing.
    fn dc_refine(&mut self, input: &mut Input<'_>, header: &Header, blocks: &mut [[i16; 64]]) {
        let p1 = 1i32 << header.scan.al;
        for block in blocks.iter_mut().take(self.blocks_in_mcu) {
            if self.bits.get(input, 1) != 0 {
                block[0] = (i32::from(block[0]) | p1) as i16;
            }
        }
    }

    /// `decode_mcu_AC_refine`.
    fn ac_refine<B: Coefficients>(
        &mut self,
        input: &mut Input<'_>,
        header: &Header,
        block: &mut B,
    ) {
        if self.bits.insufficient {
            return;
        }
        let scan = &header.scan;
        let p1 = 1i32 << scan.al;
        let m1 = -1i32 << scan.al;
        let ci = scan.comps.first().copied().unwrap_or(0);
        let number = header.components.get(ci).map_or(0, |c| c.ac_tbl_no);
        // Borrowed from its field, beside the bit reader, as in `ac_first`.
        let Some(table) = self
            .tables
            .get(usize::from(number))
            .and_then(Option::as_ref)
        else {
            return;
        };
        let se = usize::from(scan.se);
        let mut k = usize::from(scan.ss);
        let mut eobrun = self.eobrun;
        if eobrun == 0 {
            while k <= se {
                let symbol = self.bits.decode(input, table);
                let mut run = symbol >> 4;
                let size = symbol & 15;
                let mut value = 0i32;
                if size != 0 {
                    if size != 1 {
                        // JWRN_HUFF_BAD_CODE.
                        input.warn();
                    }
                    value = if self.bits.get(input, 1) != 0 { p1 } else { m1 };
                } else if run != 15 {
                    eobrun = 1u32 << run;
                    if run != 0 {
                        eobrun = eobrun.wrapping_add(self.bits.get(input, run) as u32);
                    }
                    break;
                }
                // Advance over already-nonzero coefficients and `run` zero
                // ones, appending a correction bit to each nonzero one.
                loop {
                    let at = natural(k);
                    let coefficient = block.at(at);
                    if coefficient != 0 {
                        if self.bits.get(input, 1) != 0 {
                            refine(block, at, p1, m1);
                        }
                    } else {
                        run = run.wrapping_sub(1);
                        if run < 0 {
                            break;
                        }
                    }
                    k = k.saturating_add(1);
                    if k > se {
                        break;
                    }
                }
                if value != 0 {
                    block.set(natural(k), value as i16);
                }
                k = k.saturating_add(1);
            }
        }
        if eobrun > 0 {
            while k <= se {
                let at = natural(k);
                if block.at(at) != 0 && self.bits.get(input, 1) != 0 {
                    refine(block, at, p1, m1);
                }
                k = k.saturating_add(1);
            }
            eobrun = eobrun.wrapping_sub(1);
        }
        self.eobrun = eobrun;
    }
}

/// A correction bit for an already-nonzero coefficient: increase its
/// magnitude by `p1`, unless that bit is already set.
fn refine<B: Coefficients>(block: &mut B, at: usize, p1: i32, m1: i32) {
    let cell = block.at(at);
    let value = i32::from(cell);
    if value & p1 == 0 {
        let refined = if value >= 0 {
            cell.wrapping_add(p1 as i16)
        } else {
            cell.wrapping_add(m1 as i16)
        };
        block.set(at, refined);
    }
}

/// `(JCOEF)LEFT_SHIFT(value, al)`: shifted as an unsigned long, then cut to
/// 16 bits.
pub(super) const fn left_shift(value: i32, al: u8) -> i16 {
    ((value as i64 as u64) << (al & 63)) as i16
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn extend_makes_small_values_negative() {
        assert_eq!(extend(0, 1), -1);
        assert_eq!(extend(1, 1), 1);
        assert_eq!(extend(3, 3), -4);
        assert_eq!(extend(4, 3), 4);
        assert_eq!(extend(0, 15), -32767);
    }

    #[test]
    fn the_standard_tables_derive_and_decode() {
        let mut tables = Tables::new();
        tables.default_huffman();
        let dc = derive(&tables, true, 0, false).unwrap();
        // The luma DC code for category 0 is `00`; for 5, `110`.
        let mut input = Input::new(&[0b0011_0000]);
        let mut bits = Bits::default();
        assert_eq!(bits.decode(&mut input, &dc), 0);
        assert_eq!(bits.decode(&mut input, &dc), 5);
    }

    #[test]
    fn an_oversubscribed_table_is_refused() {
        let mut tables = Tables::new();
        let mut bits = [0u8; 17];
        bits[1] = 3; // three one-bit codes cannot exist
        tables.dc[0] = Some(HuffSpec {
            bits,
            values: [0; 256],
        });
        assert!(derive(&tables, true, 0, false).is_err());
        assert_eq!(
            derive(&tables, true, 1, false).err(),
            Some(jerr::NO_HUFF_TABLE)
        );
    }

    #[test]
    fn a_marker_makes_zero_bits_and_marks_the_scan_short() {
        let mut input = Input::new(&[0xAB, 0xFF, 0xD9]);
        let mut bits = Bits::default();
        assert_eq!(bits.get(&mut input, 8), 0xAB);
        assert!(!bits.insufficient);
        assert_eq!(bits.get(&mut input, 8), 0);
        assert!(bits.insufficient);
        assert_eq!(input.unread_marker, 0xD9);
    }

    #[test]
    fn stuffed_ff_is_a_data_byte() {
        let mut input = Input::new(&[0xFF, 0x00, 0x12]);
        let mut bits = Bits::default();
        assert_eq!(bits.get(&mut input, 16), 0xFF12);
    }

    #[test]
    fn left_shift_wraps_to_sixteen_bits() {
        assert_eq!(left_shift(3, 2), 12);
        assert_eq!(left_shift(16, 12), 0);
        assert_eq!(left_shift(-1, 4), -16);
    }
}
