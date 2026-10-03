//! Compression: libbzip2's input stage (`bzlib.c`: `ADD_CHAR_TO_BLOCK`,
//! `add_pair_to_block`, `flush_RL`, and the block cutting of
//! `handle_compress`) and its back end (`compress.c`).
//!
//! Every choice the reference makes that shows in the output -- where a block
//! ends, how runs are coded, how many coding tables, which symbols start in
//! which table, four refinement passes, which table each group of 50 takes on
//! a tie -- is made the same way here, so that the stream is libbzip2's own.

use alloc::vec::Vec;

use crate::Level;
use crate::blocksort::Sorter;
use crate::huffman::{self, MAX_ALPHA_SIZE};
use crate::tables::CRC_TABLE;

/// `BZ_N_GROUPS`
const N_GROUPS: usize = 6;
/// `BZ_G_SIZE`
const G_SIZE: usize = 50;
/// `BZ_N_ITERS`: refinement passes over the table assignment.
const N_ITERS: usize = 4;
/// `BZ_LESSER_ICOST`, `BZ_GREATER_ICOST`: the starting code lengths, cheap
/// for the symbols a table starts out owning and dear for the rest.
const LESSER_ICOST: u8 = 0;
const GREATER_ICOST: u8 = 15;
/// The longest code the compressor makes, since bzip2 1.0.3 (it was 20; the
/// decoder still reads 20).
const MAX_CODE_LEN_OUT: i32 = 17;
const RUNA: u16 = 0;
const RUNB: u16 = 1;

/// `BZ_UPDATE_CRC`; see `decompress::crc_update`.
fn crc_update(crc: u32, byte: u8) -> u32 {
    let i = usize::from((crc >> 24) as u8 ^ byte);
    #[allow(clippy::indexing_slicing)]
    let entry = CRC_TABLE[i];
    (crc << 8) ^ entry
}

/// libbzip2's `bsW` and friends: bits written most significant first into a
/// 32-bit buffer, flushed a byte at a time.
struct BitWriter {
    out: Vec<u8>,
    buff: u32,
    /// Bits in `buff`: below 8 after each flush.
    live: u32,
}

impl BitWriter {
    /// `bsW`: writes the low `n` bits of `v`, `n` from 1 to 24.
    fn put(&mut self, n: u32, v: u32) {
        while self.live >= 8 {
            self.out.push((self.buff >> 24) as u8);
            self.buff <<= 8;
            self.live = self.live.wrapping_sub(8);
        }
        // `live` is below 8 and `n` at most 24, so the shift is from 1 to 32
        // less 7 less 24 and the sum stays below 32.
        let shift = 32u32.wrapping_sub(self.live).wrapping_sub(n);
        self.buff |= v.wrapping_shl(shift);
        self.live = self.live.wrapping_add(n);
    }

    /// `bsPutUChar`.
    fn put_byte(&mut self, c: u8) {
        self.put(8, u32::from(c));
    }

    /// `bsPutUInt32`.
    fn put_u32(&mut self, u: u32) {
        for b in u.to_be_bytes() {
            self.put_byte(b);
        }
    }

    /// `bsFinishWrite`: the last bits, padded with zeros to a byte.
    fn finish(mut self) -> Vec<u8> {
        while self.live > 0 {
            self.out.push((self.buff >> 24) as u8);
            self.buff <<= 8;
            self.live = self.live.saturating_sub(8);
        }
        self.out
    }
}

/// The state of one compression: libbzip2's `EState`, less what a
/// whole-buffer call does not need.
struct Encoder {
    /// `nblockMAX`: a block is full once it holds this many bytes. Set 19
    /// short of the level's size, because the run-length step can add up to
    /// five bytes past the check.
    nblock_max: usize,
    /// The block being filled, after run-length coding.
    block: Vec<u8>,
    /// Which byte values occur in `block`.
    in_use: [bool; 256],
    /// CRC of the bytes the block stands for, before run-length coding.
    block_crc: u32,
    combined_crc: u32,
    /// The run being accumulated: its byte (256 for none) and length.
    state_in_ch: u32,
    state_in_len: u32,
    w: BitWriter,
    sorter: Sorter,
    mtfv: Vec<u16>,
    /// Always the default here; the tests bend it to make the streams other
    /// encoders, or damage, make.
    shape: Shape,
}

/// Ways a block can be written other than as `bzip2` writes it.
#[derive(Clone, Copy, Debug, Default)]
struct Shape {
    /// Selectors written beyond those the block needs, as some encoders
    /// round the count up -- which libbzip2 1.0.8 reads, and the kernel's
    /// old copy refused.
    pad_selectors: usize,
    /// Lengths to write in place of the ones the encoder chose. To write a
    /// table that is not a prefix code (which only an unused table can be
    /// and still decode), or one with codes to spare.
    table_lengths: Option<LengthFor>,
}

/// Given a table's index, whether any selector names it, and a symbol: the
/// symbol's length in that table, or `None` to leave it.
type LengthFor = fn(usize, bool, usize) -> Option<u8>;

/// Compresses `data` as `bzip2 -<level>` does.
pub(crate) fn compress(data: &[u8], level: Level) -> Vec<u8> {
    let mut enc = Encoder::new(level, data.len());
    for &c in data {
        enc.add_char(c);
    }
    enc.finish()
}

impl Encoder {
    fn new(level: Level, input_len: usize) -> Self {
        let level = level.get();
        let size = usize::from(level).wrapping_mul(100_000);
        let mut w = BitWriter {
            out: Vec::with_capacity((input_len / 4).saturating_add(64)),
            buff: 0,
            live: 0,
        };
        // The stream header, which libbzip2 writes with the first block.
        for c in [b'B', b'Z', b'h', b'0'.wrapping_add(level)] {
            w.put_byte(c);
        }
        Self {
            nblock_max: size.wrapping_sub(19),
            block: Vec::with_capacity(input_len.min(size)),
            in_use: [false; 256],
            block_crc: 0xffff_ffff,
            combined_crc: 0,
            state_in_ch: 256,
            state_in_len: 0,
            w,
            sorter: Sorter::default(),
            mtfv: Vec::new(),
            shape: Shape::default(),
        }
    }

    /// `prepare_new_block`.
    fn prepare_new_block(&mut self) {
        self.block.clear();
        self.block_crc = 0xffff_ffff;
        self.in_use = [false; 256];
    }

    /// The input loop of `copy_input_until_stop` and `handle_compress`: a
    /// full block is compressed before the next byte goes in, and the run in
    /// progress carries over into the next block.
    fn add_char(&mut self, c: u8) {
        if self.block.len() >= self.nblock_max {
            self.compress_block(false);
            self.prepare_new_block();
        }
        self.add_char_to_block(u32::from(c));
    }

    /// `ADD_CHAR_TO_BLOCK`: runs of up to 255 equal bytes are gathered
    /// before they reach the block.
    fn add_char_to_block(&mut self, zchh: u32) {
        if zchh != self.state_in_ch && self.state_in_len == 1 {
            // The common case: a run of one ends.
            let ch = self.state_in_ch.to_le_bytes()[0];
            self.block_crc = crc_update(self.block_crc, ch);
            self.mark_in_use(ch);
            self.block.push(ch);
            self.state_in_ch = zchh;
        } else if zchh != self.state_in_ch || self.state_in_len == 255 {
            if self.state_in_ch < 256 {
                self.add_pair_to_block();
            }
            self.state_in_ch = zchh;
            self.state_in_len = 1;
        } else {
            self.state_in_len = self.state_in_len.wrapping_add(1);
        }
    }

    fn mark_in_use(&mut self, b: u8) {
        if let Some(slot) = self.in_use.get_mut(usize::from(b)) {
            *slot = true;
        }
    }

    /// `add_pair_to_block`: a run of one to three is written out; a longer one
    /// as four copies and a count of the rest.
    fn add_pair_to_block(&mut self) {
        let ch = self.state_in_ch.to_le_bytes()[0];
        for _ in 0..self.state_in_len {
            self.block_crc = crc_update(self.block_crc, ch);
        }
        self.mark_in_use(ch);
        match self.state_in_len {
            1..=3 => {
                for _ in 0..self.state_in_len {
                    self.block.push(ch);
                }
            }
            len => {
                // `len` is from 4 to 255.
                let extra = u8::try_from(len.wrapping_sub(4)).unwrap_or(0);
                self.mark_in_use(extra);
                self.block.extend_from_slice(&[ch, ch, ch, ch, extra]);
            }
        }
    }

    /// `flush_RL`.
    fn flush_rl(&mut self) {
        if self.state_in_ch < 256 {
            self.add_pair_to_block();
        }
        self.state_in_ch = 256;
        self.state_in_len = 0;
    }

    /// The end of input: what `BZ_FINISH` does after the last `BZ_RUN`.
    fn finish(mut self) -> Vec<u8> {
        if self.block.len() >= self.nblock_max {
            self.compress_block(false);
            self.prepare_new_block();
        }
        self.flush_rl();
        self.compress_block(true);
        self.w.finish()
    }

    /// `BZ2_compressBlock`.
    fn compress_block(&mut self, is_last: bool) {
        if !self.block.is_empty() {
            let block_crc = !self.block_crc;
            self.combined_crc = self.combined_crc.rotate_left(1) ^ block_crc;
            let orig_ptr = self.sorter.sort(&mut self.block);

            for c in [0x31, 0x41, 0x59, 0x26, 0x53, 0x59] {
                self.w.put_byte(c);
            }
            self.w.put_u32(block_crc);
            // Never randomised: since 0.9.5 the sort does not need it.
            self.w.put(1, 0);
            self.w.put(24, orig_ptr);

            let mut mtf_freq = [0i32; MAX_ALPHA_SIZE];
            let n_in_use = generate_mtf_values(
                &self.sorter.ptr,
                &self.block,
                &self.in_use,
                &mut self.mtfv,
                &mut mtf_freq,
            );
            send_mtf_values(
                &mut self.w,
                &self.mtfv,
                &mtf_freq,
                &self.in_use,
                n_in_use,
                self.shape,
            );
        }

        if is_last {
            for c in [0x17, 0x72, 0x45, 0x38, 0x50, 0x90] {
                self.w.put_byte(c);
            }
            self.w.put_u32(self.combined_crc);
        }
    }
}

/// `generateMTFValues`: the sorted block's last column as move-to-front
/// positions, runs of position 0 coded with RUNA and RUNB, other positions
/// shifted up one, and an end-of-block symbol. Returns the number of byte
/// values in use.
fn generate_mtf_values(
    ptr: &[u32],
    block: &[u8],
    in_use: &[bool; 256],
    mtfv: &mut Vec<u16>,
    mtf_freq: &mut [i32; MAX_ALPHA_SIZE],
) -> usize {
    // `makeMaps_e`
    let mut unseq_to_seq = [0u8; 256];
    let mut n_in_use = 0usize;
    for (slot, &used) in unseq_to_seq.iter_mut().zip(in_use) {
        if used {
            *slot = u8::try_from(n_in_use).unwrap_or(u8::MAX);
            n_in_use = n_in_use.wrapping_add(1);
        }
    }
    let eob = n_in_use.wrapping_add(1);

    mtf_freq.fill(0);
    mtfv.clear();

    let bump = |mtf_freq: &mut [i32; MAX_ALPHA_SIZE], sym: usize| {
        if let Some(f) = mtf_freq.get_mut(sym) {
            *f = f.wrapping_add(1);
        }
    };
    // Writes the run of `z_pend` zeros in bijective base 2.
    let flush_zeros = |mtfv: &mut Vec<u16>, mtf_freq: &mut [i32; MAX_ALPHA_SIZE], z_pend: u32| {
        if z_pend == 0 {
            return;
        }
        let mut z = z_pend.wrapping_sub(1);
        loop {
            let sym = if z & 1 == 1 { RUNB } else { RUNA };
            mtfv.push(sym);
            bump(mtf_freq, usize::from(sym));
            if z < 2 {
                break;
            }
            z = (z.wrapping_sub(2)) / 2;
        }
    };

    let mut yy = [0u8; 256];
    for (slot, v) in yy.iter_mut().zip(0..=255u8) {
        *slot = v;
    }
    let nblock = block.len();
    let mut z_pend = 0u32;
    for &p in ptr {
        // The byte before rotation `p` starts: the last column.
        let p = usize::try_from(p).unwrap_or(0);
        let j = if p == 0 {
            nblock.wrapping_sub(1)
        } else {
            p.wrapping_sub(1)
        };
        let ll_i = unseq_to_seq
            .get(usize::from(block.get(j).copied().unwrap_or(0)))
            .copied()
            .unwrap_or(0);

        if yy[0] == ll_i {
            z_pend = z_pend.wrapping_add(1);
        } else {
            flush_zeros(mtfv, mtf_freq, z_pend);
            z_pend = 0;
            // Move `ll_i` to the front; its old position is the value.
            let j = yy.iter().position(|&v| v == ll_i).unwrap_or(0);
            if let Some(front) = yy.get_mut(..=j) {
                front.rotate_right(1);
            }
            let sym = j.wrapping_add(1);
            mtfv.push(u16::try_from(sym).unwrap_or(u16::MAX));
            bump(mtf_freq, sym);
        }
    }
    flush_zeros(mtfv, mtf_freq, z_pend);

    mtfv.push(u16::try_from(eob).unwrap_or(u16::MAX));
    bump(mtf_freq, eob);
    n_in_use
}

/// `sendMTFValues`: chooses the coding tables and writes the block's
/// mapping table, selectors, tables and coded symbols.
// One function in libbzip2, kept as one; every count in it is bounded by the
// block's symbol count (below 2^20) and every index by the alphabet size.
#[allow(clippy::too_many_lines, clippy::arithmetic_side_effects)]
fn send_mtf_values(
    w: &mut BitWriter,
    mtfv: &[u16],
    mtf_freq: &[i32; MAX_ALPHA_SIZE],
    in_use: &[bool; 256],
    n_in_use: usize,
    shape: Shape,
) {
    let alpha_size = n_in_use + 2;
    let n_mtf = mtfv.len();
    let mut len = [[GREATER_ICOST; MAX_ALPHA_SIZE]; N_GROUPS];

    // How many coding tables: more for longer blocks.
    let n_groups: usize = match n_mtf {
        0..200 => 2,
        200..600 => 3,
        600..1200 => 4,
        1200..2400 => 5,
        _ => 6,
    };

    // The initial tables: the alphabet cut into `n_groups` ranges of roughly
    // equal frequency, each table cheap on its range.
    {
        let freq = |v: i32| -> i32 {
            usize::try_from(v)
                .ok()
                .and_then(|v| mtf_freq.get(v))
                .copied()
                .unwrap_or(0)
        };
        let n_groups_i = n_groups as i32;
        let alpha_i = alpha_size as i32;
        let mut n_part = n_groups_i;
        let mut rem_f = n_mtf as i32;
        let mut gs = 0i32;
        while n_part > 0 {
            let t_freq = rem_f / n_part;
            let mut ge = gs - 1;
            let mut a_freq = 0i32;
            while a_freq < t_freq && ge < alpha_i - 1 {
                ge += 1;
                a_freq += freq(ge);
            }
            if ge > gs && n_part != n_groups_i && n_part != 1 && (n_groups_i - n_part) % 2 == 1 {
                a_freq -= freq(ge);
                ge -= 1;
            }
            if let Some(table) = usize::try_from(n_part - 1)
                .ok()
                .and_then(|t| len.get_mut(t))
            {
                for (v, slot) in (0i32..).zip(table.iter_mut().take(alpha_size)) {
                    *slot = if v >= gs && v <= ge {
                        LESSER_ICOST
                    } else {
                        GREATER_ICOST
                    };
                }
            }
            n_part -= 1;
            gs = ge + 1;
            rem_f -= a_freq;
        }
    }

    // Refine: give each group of 50 symbols the table that codes it in the
    // fewest bits (the first on a tie), then rebuild each table from the
    // symbols it was given.
    let mut rfreq = [[0i32; MAX_ALPHA_SIZE]; N_GROUPS];
    let mut selectors: Vec<u8> = Vec::with_capacity(n_mtf.div_ceil(G_SIZE));
    for _ in 0..N_ITERS {
        for t in rfreq.iter_mut().take(n_groups) {
            t.fill(0);
        }
        selectors.clear();

        for group in mtfv.chunks(G_SIZE) {
            let mut cost = [0u16; N_GROUPS];
            for &icv in group {
                for (c, table) in cost.iter_mut().zip(&len).take(n_groups) {
                    *c = c
                        .wrapping_add(u16::from(table.get(usize::from(icv)).copied().unwrap_or(0)));
                }
            }
            let mut bc = 999_999_999i32;
            let mut bt = 0usize;
            for (t, &c) in cost.iter().enumerate().take(n_groups) {
                if i32::from(c) < bc {
                    bc = i32::from(c);
                    bt = t;
                }
            }
            selectors.push(u8::try_from(bt).unwrap_or(0));
            if let Some(freqs) = rfreq.get_mut(bt) {
                for &icv in group {
                    if let Some(f) = freqs.get_mut(usize::from(icv)) {
                        *f += 1;
                    }
                }
            }
        }

        for (table, freqs) in len.iter_mut().zip(&rfreq).take(n_groups) {
            huffman::make_code_lengths(table, freqs, alpha_size, MAX_CODE_LEN_OUT);
        }
    }
    if let Some(length_for) = shape.table_lengths {
        for (t, table) in len.iter_mut().enumerate().take(n_groups) {
            let named = selectors.iter().any(|&s| usize::from(s) == t);
            for (v, slot) in table.iter_mut().enumerate().take(alpha_size) {
                if let Some(l) = length_for(t, named, v) {
                    *slot = l;
                }
            }
        }
    }

    // The selectors, move-to-front coded.
    let mut pos = [0u8, 1, 2, 3, 4, 5];
    let selector_mtf: Vec<u32> = selectors
        .iter()
        .map(|&sel| {
            let j = pos.iter().position(|&p| p == sel).unwrap_or(0);
            if let Some(front) = pos.get_mut(..=j) {
                front.rotate_right(1);
            }
            j as u32
        })
        .collect();

    // The codes themselves.
    let mut code = [[0i32; MAX_ALPHA_SIZE]; N_GROUPS];
    for (codes, lengths) in code.iter_mut().zip(&len).take(n_groups) {
        let used = lengths.get(..alpha_size).unwrap_or_default();
        let min_len = used.iter().copied().min().unwrap_or(1);
        let max_len = used.iter().copied().max().unwrap_or(1);
        huffman::assign_codes(codes, used, min_len, max_len);
    }

    // The mapping table: which sixteenths of the byte range occur, then
    // which bytes in each.
    let mut in_use16 = [false; 16];
    for (slot, chunk) in in_use16.iter_mut().zip(in_use.chunks(16)) {
        *slot = chunk.iter().any(|&u| u);
    }
    for &used in &in_use16 {
        w.put(1, u32::from(used));
    }
    for (&used, chunk) in in_use16.iter().zip(in_use.chunks(16)) {
        if used {
            for &u in chunk {
                w.put(1, u32::from(u));
            }
        }
    }

    // The selectors, in unary.
    w.put(3, n_groups as u32);
    // Fifteen bits hold at most 32 767.
    let pad = shape.pad_selectors.min(0x7fff - selectors.len());
    w.put(15, (selectors.len() + pad) as u32);
    for &j in &selector_mtf {
        for _ in 0..j {
            w.put(1, 1);
        }
        w.put(1, 0);
    }
    for _ in 0..pad {
        w.put(1, 0);
    }

    // The code lengths: a start, then +1 (`10`) and -1 (`11`) steps and a
    // `0` per symbol.
    for lengths in len.iter().take(n_groups) {
        let used = lengths.get(..alpha_size).unwrap_or_default();
        let mut curr = used.first().copied().unwrap_or(0);
        w.put(5, u32::from(curr));
        for &l in used {
            while curr < l {
                w.put(2, 2);
                curr += 1;
            }
            while curr > l {
                w.put(2, 3);
                curr -= 1;
            }
            w.put(1, 0);
        }
    }

    // The block data proper.
    for (group, &sel) in mtfv.chunks(G_SIZE).zip(&selectors) {
        let sel = usize::from(sel);
        let (Some(lengths), Some(codes)) = (len.get(sel), code.get(sel)) else {
            continue;
        };
        for &v in group {
            let v = usize::from(v);
            let l = lengths.get(v).copied().unwrap_or(0);
            let c = codes.get(v).copied().unwrap_or(0);
            w.put(u32::from(l), u32::try_from(c).unwrap_or(0));
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
mod tests {
    use super::*;

    #[test]
    fn the_writer_packs_most_significant_first() {
        let mut w = BitWriter {
            out: Vec::new(),
            buff: 0,
            live: 0,
        };
        w.put(1, 1);
        w.put(3, 0b010);
        w.put(8, 0xff);
        w.put(24, 0x00_0102);
        assert_eq!(w.finish(), [0b1010_1111, 0b1111_0000, 0x00, 0x10, 0x20]);
    }

    /// Runs of four or more become four copies and a count; a run of 300 is
    /// a run of 255 and one of 45; the CRC covers the bytes before coding.
    #[test]
    fn runs_are_coded_as_libbzip2_codes_them() {
        let mut e = Encoder::new(Level::BEST, 0);
        let mut input = Vec::new();
        input.extend_from_slice(b"abbcccdddd");
        input.extend(core::iter::repeat_n(b'e', 300));
        input.push(b'f');
        for &c in &input {
            e.add_char(c);
        }
        e.flush_rl();
        let mut want = b"abbcccdddd\x00".to_vec();
        want.extend_from_slice(b"eeee");
        want.push(251);
        want.extend_from_slice(b"eeee");
        want.push(41);
        want.push(b'f');
        assert_eq!(e.block, want);
        let crc = !input.iter().fold(!0u32, |c, &b| crc_update(c, b));
        assert_eq!(!e.block_crc, crc);
        assert!(e.in_use[0] && e.in_use[251] && e.in_use[41]);
    }

    fn crc_of(data: &[u8]) -> u32 {
        !data.iter().fold(!0u32, |c, &b| crc_update(c, b))
    }

    /// A one-block stream of `block` exactly as given -- not run-length coded
    /// on the way in -- claiming `crc`, with `pad` selectors more than it
    /// needs.
    fn raw_stream(block: &[u8], crc: u32, pad: usize) -> Vec<u8> {
        shaped_stream(
            block,
            crc,
            Shape {
                pad_selectors: pad,
                ..Shape::default()
            },
        )
    }

    /// As [`raw_stream`], written in `shape`.
    fn shaped_stream(block: &[u8], crc: u32, shape: Shape) -> Vec<u8> {
        let mut e = Encoder::new(Level::BEST, 0);
        e.shape = shape;
        e.block = block.to_vec();
        for &b in block {
            e.mark_in_use(b);
        }
        e.block_crc = !crc;
        e.compress_block(true);
        e.w.finish()
    }

    /// The stream as 7-Zip reads it: its bytes, or its error and the bytes
    /// before it.
    fn as_7zip(packed: &[u8]) -> (Result<usize, crate::Error>, Vec<u8>) {
        let mut out = Vec::new();
        let r = crate::decompress_as_7zip(packed, &mut out, 1000);
        (r, out)
    }

    /// libbzip2's output stage calls a block corrupt when four equal bytes
    /// end it with no count after them, whatever its CRC says; 7-Zip writes
    /// the four, and the CRC decides.
    #[test]
    fn a_run_without_its_count_byte_is_refused_by_libbzip2_alone() {
        let whole = raw_stream(b"xyzAAAA\x00", crc_of(b"xyzAAAA"), 0);
        assert_eq!(crate::decompress(&whole).unwrap(), b"xyzAAAA");
        assert_eq!(as_7zip(&whole), (Ok(whole.len()), b"xyzAAAA".to_vec()));
        let cut = raw_stream(b"xyzAAAA", crc_of(b"xyzAAAA"), 0);
        assert_eq!(crate::decompress(&cut), Err(crate::Error::InvalidRun));
        assert_eq!(as_7zip(&cut), (Ok(cut.len()), b"xyzAAAA".to_vec()));
        // Four bytes alone, and four after a run that had its count.
        for (block, want) in [(&b"AAAA"[..], &b"AAAA"[..]), (b"AAAA\x02BBBB", b"AAAAAABBBB")] {
            let cut = raw_stream(block, crc_of(want), 0);
            assert_eq!(crate::decompress(&cut), Err(crate::Error::InvalidRun));
            assert_eq!(as_7zip(&cut), (Ok(cut.len()), want.to_vec()));
        }
        // The CRC is still checked, after the bytes are out.
        let wrong = raw_stream(b"xyzAAAA", crc_of(b"xyzAAAB"), 0);
        let (r, out) = as_7zip(&wrong);
        assert!(matches!(r, Err(crate::Error::BlockCrcMismatch { .. })));
        assert_eq!(out, b"xyzAAAA");
    }

    /// A table no selector names whose lengths -- 1, 1, 2, 2: a Kraft sum
    /// of 3/2 -- are no prefix code: libbzip2 never uses it and decodes the
    /// block; 7-Zip builds every table and refuses it.
    #[test]
    fn an_unused_table_that_is_no_prefix_code_is_refused_by_7zip_alone() {
        let data = b"abababababab";
        let shape = Shape {
            table_lengths: Some(|_, named, v| (!named).then_some([1, 1, 2, 2][v])),
            ..Shape::default()
        };
        let packed = shaped_stream(data, crc_of(data), shape);
        assert_eq!(crate::decompress(&packed).unwrap(), data);
        assert_eq!(as_7zip(&packed), (Err(crate::Error::InvalidTables), Vec::new()));
        // The same table as a prefix code is read by both.
        let shape = Shape {
            table_lengths: Some(|_, named, v| (!named).then_some([1, 2, 3, 3][v])),
            ..Shape::default()
        };
        let packed = shaped_stream(data, crc_of(data), shape);
        assert_eq!(as_7zip(&packed), (Ok(packed.len()), data.to_vec()));
    }

    /// A table with codes to spare -- four symbols of three bits, a Kraft sum
    /// of 1/2 -- in use: lbzip2 writes such tables, and both read them.
    #[test]
    fn a_table_with_codes_to_spare_is_read_by_both() {
        let data = b"abababababab";
        let shape = Shape {
            table_lengths: Some(|_, named, _| named.then_some(3)),
            ..Shape::default()
        };
        let packed = shaped_stream(data, crc_of(data), shape);
        assert_ne!(packed, raw_stream(data, crc_of(data), 0));
        assert_eq!(crate::decompress(&packed).unwrap(), data);
        assert_eq!(as_7zip(&packed), (Ok(packed.len()), data.to_vec()));
    }

    /// libbzip2 1.0.8 reads up to 32 767 selectors and uses the first 18 002
    /// ("some implementations might round up"); a stream that declares more
    /// than its block needs is valid.
    #[test]
    fn more_selectors_than_a_block_needs_are_read_and_ignored() {
        let data = b"selectors rounded up";
        for pad in [1, 18_001, 32_766] {
            let packed = raw_stream(data, crc_of(data), pad);
            assert_eq!(
                crate::decompress(&packed).unwrap(),
                data,
                "{pad} extra selectors"
            );
        }
    }

    #[test]
    fn zero_runs_are_bijective_base_two() {
        // A block whose sorted last column starts with a run of the front
        // byte: one byte value only, so every position is 0.
        for (zeros, want) in [
            (1usize, &[RUNA][..]),
            (2, &[RUNB]),
            (3, &[RUNA, RUNA]),
            (4, &[RUNB, RUNA]),
            (5, &[RUNA, RUNB]),
            (6, &[RUNB, RUNB]),
            (7, &[RUNA, RUNA, RUNA]),
        ] {
            let block = alloc::vec![9u8; zeros];
            let ptr: Vec<u32> = (0..zeros as u32).collect();
            let mut in_use = [false; 256];
            in_use[9] = true;
            let mut mtfv = Vec::new();
            let mut freq = [0i32; MAX_ALPHA_SIZE];
            let n = generate_mtf_values(&ptr, &block, &in_use, &mut mtfv, &mut freq);
            assert_eq!(n, 1);
            assert_eq!(&mtfv[..mtfv.len() - 1], want, "{zeros} zeros");
            assert_eq!(*mtfv.last().unwrap(), 2, "end of block is n_in_use + 1");
        }
    }
}
