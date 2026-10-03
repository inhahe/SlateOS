//! PPMd var.H as 7-Zip decodes it in a 7z archive (method `030401`):
//! `C/Ppmd7.c`, `C/Ppmd7Dec.c` and `CPP/7zip/Compress/PpmdDecoder.cpp` from
//! the LZMA SDK 26.00 (Igor Pavlov, public domain; PPMd var.H by Dmitry
//! Shkarin, public domain), ported.
//!
//! PPMd predicts each byte from the bytes before it, with a model of
//! contexts it builds as it goes, all of it in one block of memory whose
//! size the archive gives and which it carves up with an allocator of its
//! own. When the block is full the model is thrown away and started again
//! -- so *when* that happens, and so every byte decoded after it, depends on
//! the exact allocations. The port therefore keeps the block itself: a
//! `Vec<u8>` with every record at the offset `Ppmd7.h` lays it out at
//! (little-endian, as on the x86 7-Zip runs on), and references as 32-bit
//! offsets into it -- the layout 7-Zip uses on a 64-bit machine.
//!
//! Every access to the block is checked. The model is built only from
//! decoded symbols, so a damaged stream cannot make it inconsistent; but a
//! read or write out of range, or a search that runs off its record, ends
//! the decoding as damage rather than reaching past the block.

// The arithmetic is `Ppmd7.c`'s: frequencies below 256, sums below 2^16,
// offsets inside the block (and checked where used); the range coder's on
// `u32`, wrapping where the C code relies on it (`wrapping_*`).
#![allow(clippy::arithmetic_side_effects)]

use alloc::vec;
use alloc::vec::Vec;

/// `PPMD_INT_BITS`
const INT_BITS: u32 = 7;
/// `PPMD_PERIOD_BITS`
const PERIOD_BITS: u32 = 7;
/// `PPMD_BIN_SCALE`
const BIN_SCALE: u32 = 1 << (INT_BITS + PERIOD_BITS);
/// `PPMD_NUM_INDEXES`: the sizes of free-list block, 1 to 128 units.
const NUM_INDEXES: usize = 4 + 4 + 4 + (128 + 3 - 4 - 2 * 4 - 3 * 4) / 4;
/// `PPMD7_MAX_ORDER`
const MAX_ORDER: usize = 64;
/// `PPMD7_MIN_ORDER`
const MIN_ORDER: u32 = 2;
/// `PPMD7_MIN_MEM_SIZE`
const MIN_MEM_SIZE: u32 = 1 << 11;
/// `PPMD7_MAX_MEM_SIZE`
const MAX_MEM_SIZE: u32 = 0xFFFF_FFFF - 12 * 3;
/// `MAX_FREQ`
const MAX_FREQ: u32 = 124;
/// `UNIT_SIZE`: the allocator's unit, a context record's size.
const UNIT_SIZE: u32 = 12;
/// A `CPpmd_State`'s size.
const STATE_SIZE: u32 = 6;
/// `PPMD7_kExpEscape`
const EXP_ESCAPE: [u8; 16] = [25, 14, 9, 7, 5, 5, 4, 4, 4, 3, 3, 3, 2, 2, 2, 2];
/// `PPMD7_kInitBinEsc`
const INIT_BIN_ESC: [u16; 8] = [
    0x3CDD, 0x1F3F, 0x59BF, 0x48F3, 0x64A1, 0x5ABC, 0x6632, 0x6051,
];
/// `kTopValue`
const TOP: u32 = 1 << 24;

const _: () = assert!(NUM_INDEXES == 38);

/// `PPMD_GET_MEAN`
const fn get_mean(summ: u32) -> u32 {
    (summ + (1 << (PERIOD_BITS - 2))) >> PERIOD_BITS
}

/// `PPMD7_HiBitsFlag_3`
const fn hi_bits_flag_3(sym: u32) -> u32 {
    ((sym + 0xC0) >> (8 - 3)) & (1 << 3)
}

/// `PPMD7_HiBitsFlag_4`
const fn hi_bits_flag_4(sym: u32) -> u32 {
    ((sym + 0xC0) >> (8 - 4)) & (1 << 4)
}

/// A byte of a value known to fit one: frequencies are below 256 by the
/// model's rules (`MAX_FREQ`, and the halving in `rescale`).
#[allow(clippy::cast_possible_truncation)]
const fn byte(v: u32) -> u8 {
    v as u8
}

/// The low 16 bits, as the C code's `(UInt16)` casts take them.
#[allow(clippy::cast_possible_truncation)]
const fn low16(v: u32) -> u16 {
    v as u16
}

/// `CPpmd_See`: an escape estimator.
#[derive(Clone, Copy, Debug, Default)]
struct See {
    summ: u16,
    shift: u8,
    count: u8,
}

/// What [`Ppmd7::decode_symbol`] decoded: `PPMD7_SYM_END` and
/// `PPMD7_SYM_ERROR` as variants.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Symbol {
    Byte(u8),
    End,
    Error,
}

/// Which escape estimator a masked context uses.
#[derive(Clone, Copy, Debug)]
enum SeeRef {
    Table(usize, usize),
    Dummy,
}

/// `CPpmd7`, with its range decoder (`CPpmd7_RangeDec`) and input.
struct Ppmd7<'a> {
    /// `Base`: `align_offset` bytes, then the model's `size`.
    mem: Vec<u8>,
    size: u32,
    align_offset: u32,
    min_context: u32,
    max_context: u32,
    found_state: u32,
    order_fall: u32,
    init_esc: u32,
    prev_success: u32,
    max_order: u32,
    hi_bits_flag: u32,
    run_length: i32,
    init_rl: i32,
    glue_count: u32,
    lo_unit: u32,
    hi_unit: u32,
    text: u32,
    units_start: u32,
    indx2units: [u8; NUM_INDEXES],
    units2indx: [u8; 128],
    free_list: [u32; NUM_INDEXES],
    ns2bs_indx: [u8; 256],
    ns2indx: [u8; 256],
    dummy_see: See,
    see: [[See; 16]; 25],
    bin_summ: [[u16; 64]; 128],
    range: u32,
    code: u32,
    input: &'a [u8],
    pos: usize,
    /// `CByteInBufWrap::Extra`: a byte was read past the input's end (and
    /// read as 0).
    extra: bool,
    /// An access outside the block, or a search that ran off its record:
    /// "cannot happen", and the stream is called damaged.
    broken: bool,
}

impl<'a> Ppmd7<'a> {
    /// `Ppmd7_Construct` and `Ppmd7_Alloc(size)`.
    fn new(size: u32, input: &'a [u8]) -> Self {
        let align_offset = (4u32.wrapping_sub(size)) & 3;
        let mut indx2units = [0u8; NUM_INDEXES];
        let mut units2indx = [0u8; 128];
        let mut k = 0usize;
        for (i, slot) in indx2units.iter_mut().enumerate() {
            let step = if i >= 12 { 4 } else { (i >> 2) + 1 };
            for _ in 0..step {
                if let Some(u) = units2indx.get_mut(k) {
                    *u = byte(i as u32);
                }
                k += 1;
            }
            *slot = byte(k as u32);
        }
        let mut ns2bs_indx = [0u8; 256];
        ns2bs_indx[1] = 2;
        for (i, v) in ns2bs_indx.iter_mut().enumerate().skip(2) {
            *v = if i < 11 { 4 } else { 6 };
        }
        let mut ns2indx = [0u8; 256];
        ns2indx[1] = 1;
        ns2indx[2] = 2;
        let (mut m, mut k) = (3u32, 1u32);
        for v in ns2indx.iter_mut().skip(3) {
            *v = byte(m);
            k -= 1;
            if k == 0 {
                m += 1;
                k = m - 2;
            }
        }
        Self {
            mem: vec![0; (align_offset as usize).saturating_add(size as usize)],
            size,
            align_offset,
            min_context: 0,
            max_context: 0,
            found_state: 0,
            order_fall: 0,
            init_esc: 0,
            prev_success: 0,
            max_order: 0,
            hi_bits_flag: 0,
            run_length: 0,
            init_rl: 0,
            glue_count: 0,
            lo_unit: 0,
            hi_unit: 0,
            text: 0,
            units_start: 0,
            indx2units,
            units2indx,
            free_list: [0; NUM_INDEXES],
            ns2bs_indx,
            ns2indx,
            dummy_see: See::default(),
            see: [[See::default(); 16]; 25],
            bin_summ: [[0; 64]; 128],
            range: 0,
            code: 0,
            input,
            pos: 0,
            extra: false,
            broken: false,
        }
    }

    // ---------- the block ----------

    fn rd8(&mut self, off: u32) -> u8 {
        match self.mem.get(off as usize) {
            Some(&b) => b,
            None => {
                self.broken = true;
                0
            }
        }
    }

    fn wr8(&mut self, off: u32, v: u8) {
        match self.mem.get_mut(off as usize) {
            Some(b) => *b = v,
            None => self.broken = true,
        }
    }

    fn rd16(&mut self, off: u32) -> u16 {
        let o = off as usize;
        match self.mem.get(o..o.saturating_add(2)) {
            Some(&[a, b]) => u16::from_le_bytes([a, b]),
            _ => {
                self.broken = true;
                0
            }
        }
    }

    fn wr16(&mut self, off: u32, v: u16) {
        let o = off as usize;
        match self.mem.get_mut(o..o.saturating_add(2)) {
            Some(d) => d.copy_from_slice(&v.to_le_bytes()),
            None => self.broken = true,
        }
    }

    fn rd32(&mut self, off: u32) -> u32 {
        let o = off as usize;
        match self.mem.get(o..o.saturating_add(4)) {
            Some(&[a, b, c, d]) => u32::from_le_bytes([a, b, c, d]),
            _ => {
                self.broken = true;
                0
            }
        }
    }

    fn wr32(&mut self, off: u32, v: u32) {
        let o = off as usize;
        match self.mem.get_mut(o..o.saturating_add(4)) {
            Some(d) => d.copy_from_slice(&v.to_le_bytes()),
            None => self.broken = true,
        }
    }

    /// Copies `len` bytes within the block (the `CPpmd_State` and unit
    /// copies).
    fn copy(&mut self, dst: u32, src: u32, len: u32) {
        let (d, s, n) = (dst as usize, src as usize, len as usize);
        if s.saturating_add(n) <= self.mem.len() && d.saturating_add(n) <= self.mem.len() {
            self.mem.copy_within(s..s + n, d);
        } else {
            self.broken = true;
        }
    }

    // Records: a context (`CPpmd7_Context`, 12 bytes) is NumStats at 0,
    // SummFreq at 2 -- or, with one symbol, that symbol's state at 2 --
    // Stats at 4, Suffix at 8. A state (`CPpmd_State`, 6 bytes) is Symbol
    // at 0, Freq at 1, Successor at 2.

    fn num_stats(&mut self, c: u32) -> u32 {
        u32::from(self.rd16(c))
    }
    fn set_num_stats(&mut self, c: u32, v: u32) {
        self.wr16(c, low16(v));
    }
    fn summ_freq(&mut self, c: u32) -> u32 {
        u32::from(self.rd16(c + 2))
    }
    fn set_summ_freq(&mut self, c: u32, v: u32) {
        self.wr16(c + 2, low16(v));
    }
    fn stats(&mut self, c: u32) -> u32 {
        self.rd32(c + 4)
    }
    fn set_stats(&mut self, c: u32, v: u32) {
        self.wr32(c + 4, v);
    }
    fn suffix(&mut self, c: u32) -> u32 {
        self.rd32(c + 8)
    }
    fn set_suffix(&mut self, c: u32, v: u32) {
        self.wr32(c + 8, v);
    }
    /// `Ppmd7Context_OneState`
    const fn one_state(c: u32) -> u32 {
        c + 2
    }
    fn symbol(&mut self, s: u32) -> u32 {
        u32::from(self.rd8(s))
    }
    fn set_symbol(&mut self, s: u32, v: u32) {
        self.wr8(s, byte(v));
    }
    fn freq(&mut self, s: u32) -> u32 {
        u32::from(self.rd8(s + 1))
    }
    fn set_freq(&mut self, s: u32, v: u32) {
        self.wr8(s + 1, byte(v));
    }
    /// `Ppmd_GET_SUCCESSOR`, little-endian.
    fn successor(&mut self, s: u32) -> u32 {
        self.rd32(s + 2)
    }
    fn set_successor(&mut self, s: u32, v: u32) {
        self.wr32(s + 2, v);
    }
    /// `SWAP_STATES(s)`: the state at `s` and the one before it.
    fn swap_states(&mut self, s: u32) {
        let o = s as usize;
        let p = o.wrapping_sub(STATE_SIZE as usize);
        if p <= o && o.saturating_add(STATE_SIZE as usize) <= self.mem.len() {
            let (lo, hi) = self.mem.split_at_mut(o);
            if let (Some(a), Some(b)) = (lo.get_mut(p..), hi.get_mut(..STATE_SIZE as usize)) {
                a.swap_with_slice(b);
                return;
            }
        }
        self.broken = true;
    }

    fn i2u(&self, indx: usize) -> u32 {
        u32::from(self.indx2units.get(indx).copied().unwrap_or(0))
    }

    fn u2i(&self, nu: u32) -> usize {
        usize::from(
            self.units2indx
                .get((nu as usize).wrapping_sub(1))
                .copied()
                .unwrap_or(0),
        )
    }

    // ---------- the allocator ----------

    /// `Ppmd7_InsertNode`
    fn insert_node(&mut self, node: u32, indx: usize) {
        let head = self.free_list.get(indx).copied().unwrap_or(0);
        self.wr32(node, head);
        match self.free_list.get_mut(indx) {
            Some(f) => *f = node,
            None => self.broken = true,
        }
    }

    /// `Ppmd7_RemoveNode`
    fn remove_node(&mut self, indx: usize) -> u32 {
        let node = self.free_list.get(indx).copied().unwrap_or(0);
        let next = self.rd32(node);
        if let Some(f) = self.free_list.get_mut(indx) {
            *f = next;
        }
        node
    }

    /// `Ppmd7_SplitBlock`
    fn split_block(&mut self, ptr: u32, old_indx: usize, new_indx: usize) {
        let nu = self.i2u(old_indx) - self.i2u(new_indx);
        let ptr = ptr + self.i2u(new_indx) * UNIT_SIZE;
        let mut i = self.u2i(nu);
        if self.i2u(i) != nu {
            i -= 1;
            let k = self.i2u(i);
            self.insert_node(ptr + k * UNIT_SIZE, (nu - k - 1) as usize);
        }
        self.insert_node(ptr, i);
    }

    /// `Ppmd7_GlueFreeBlocks`: joins neighbouring free blocks. A node is
    /// Stamp at 0 (0 for free), NU at 2, Next at 4.
    fn glue_free_blocks(&mut self) {
        self.glue_count = 255;
        if self.lo_unit != self.hi_unit {
            self.wr16(self.lo_unit, 1);
        }

        let mut n = 0u32;
        for i in 0..NUM_INDEXES {
            let nu = self.i2u(i);
            let mut next = self.free_list.get(i).copied().unwrap_or(0);
            if let Some(f) = self.free_list.get_mut(i) {
                *f = 0;
            }
            while next != 0 && !self.broken {
                let node = next;
                next = self.rd32(node);
                self.wr16(node, 0);
                self.wr16(node + 2, low16(nu));
                self.wr32(node + 4, n);
                n = node;
            }
        }

        let mut head = n;
        // `prev`: None is `head`, else the node whose Next to set.
        let mut prev: Option<u32> = None;
        while n != 0 && !self.broken {
            let node = n;
            let mut nu = u32::from(self.rd16(node + 2));
            n = self.rd32(node + 4);
            if nu == 0 {
                match prev {
                    None => head = n,
                    Some(p) => self.wr32(p + 4, n),
                }
                continue;
            }
            prev = Some(node);
            loop {
                let node2 = node + nu * UNIT_SIZE;
                nu += u32::from(self.rd16(node2 + 2));
                if self.rd16(node2) != 0 || nu >= 0x10000 || self.broken {
                    break;
                }
                self.wr16(node + 2, low16(nu));
                self.wr16(node2 + 2, 0);
            }
        }

        n = head;
        while n != 0 && !self.broken {
            let mut node = n;
            let mut nu = u32::from(self.rd16(node + 2));
            n = self.rd32(node + 4);
            if nu == 0 {
                continue;
            }
            while nu > 128 {
                self.insert_node(node, NUM_INDEXES - 1);
                nu -= 128;
                node += 128 * UNIT_SIZE;
            }
            let mut i = self.u2i(nu);
            if self.i2u(i) != nu {
                i -= 1;
                let k = self.i2u(i);
                self.insert_node(node + k * UNIT_SIZE, (nu - k - 1) as usize);
            }
            self.insert_node(node, i);
        }
    }

    /// `Ppmd7_AllocUnitsRare`: `None` when the block is full.
    fn alloc_units_rare(&mut self, indx: usize) -> Option<u32> {
        if self.glue_count == 0 {
            self.glue_free_blocks();
            if self.free_list.get(indx).copied().unwrap_or(0) != 0 {
                return Some(self.remove_node(indx));
            }
        }
        let mut i = indx;
        loop {
            i += 1;
            if i == NUM_INDEXES {
                let num_bytes = self.i2u(indx) * UNIT_SIZE;
                let us = self.units_start;
                self.glue_count = self.glue_count.wrapping_sub(1);
                return if us.wrapping_sub(self.text) > num_bytes {
                    self.units_start = us - num_bytes;
                    Some(self.units_start)
                } else {
                    None
                };
            }
            if self.free_list.get(i).copied().unwrap_or(0) != 0 {
                break;
            }
        }
        let block = self.remove_node(i);
        self.split_block(block, i, indx);
        Some(block)
    }

    /// `Ppmd7_AllocUnits`
    fn alloc_units(&mut self, indx: usize) -> Option<u32> {
        if self.free_list.get(indx).copied().unwrap_or(0) != 0 {
            return Some(self.remove_node(indx));
        }
        let num_bytes = self.i2u(indx) * UNIT_SIZE;
        let lo = self.lo_unit;
        if self.hi_unit.wrapping_sub(lo) >= num_bytes {
            self.lo_unit = lo + num_bytes;
            return Some(lo);
        }
        self.alloc_units_rare(indx)
    }

    // ---------- the model ----------

    /// `Ppmd7_RestartModel`
    fn restart_model(&mut self) {
        #[cfg(test)]
        tests::RESTARTS.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        self.free_list = [0; NUM_INDEXES];
        self.text = self.align_offset;
        self.hi_unit = self.text + self.size;
        self.lo_unit = self.hi_unit - self.size / 8 / UNIT_SIZE * 7 * UNIT_SIZE;
        self.units_start = self.lo_unit;
        self.glue_count = 0;

        self.order_fall = self.max_order;
        let rl = -i32::try_from(self.max_order.min(12)).unwrap_or(12) - 1;
        self.run_length = rl;
        self.init_rl = rl;
        self.prev_success = 0;

        self.hi_unit -= UNIT_SIZE;
        let mc = self.hi_unit;
        let s = self.lo_unit;
        self.lo_unit += (256 / 2) * UNIT_SIZE;
        self.max_context = mc;
        self.min_context = mc;
        self.found_state = s;
        self.set_num_stats(mc, 256);
        self.set_summ_freq(mc, 256 + 1);
        self.set_stats(mc, s);
        self.set_suffix(mc, 0);
        for i in 0..256u32 {
            let st = s + i * STATE_SIZE;
            self.set_symbol(st, i);
            self.set_freq(st, 1);
            self.set_successor(st, 0);
        }

        for (i, row) in self.bin_summ.iter_mut().enumerate() {
            for (k, &esc) in INIT_BIN_ESC.iter().enumerate() {
                let val = low16(BIN_SCALE - u32::from(esc) / (i as u32 + 2));
                for m in (0..64).step_by(8) {
                    if let Some(v) = row.get_mut(k + m) {
                        *v = val;
                    }
                }
            }
        }
        for (i, row) in self.see.iter_mut().enumerate() {
            let summ = low16((5 * i as u32 + 10) << (PERIOD_BITS - 4));
            for s in row.iter_mut() {
                *s = See {
                    summ,
                    shift: byte(PERIOD_BITS - 4),
                    count: 4,
                };
            }
        }
        self.dummy_see = See {
            summ: 0,
            shift: byte(PERIOD_BITS),
            count: 64,
        };
    }

    /// `Ppmd7_Init`
    fn init(&mut self, max_order: u32) {
        self.max_order = max_order;
        self.restart_model();
    }

    /// The state for `sym` in context `c`'s list, searched for as the C
    /// code does -- but no further than the list.
    fn find_state(&mut self, c: u32, sym: u32) -> u32 {
        let n = self.num_stats(c);
        let mut s = self.stats(c);
        for _ in 0..n {
            if self.symbol(s) == sym {
                return s;
            }
            s += STATE_SIZE;
        }
        self.broken = true;
        self.stats(c)
    }

    /// `Ppmd7_CreateSuccessors`: `None` when the block is full.
    fn create_successors(&mut self) -> Option<u32> {
        let mut c = self.min_context;
        let mut up_branch = self.successor(self.found_state);
        let mut ps = [0u32; MAX_ORDER];
        let mut num_ps = 0usize;
        if self.order_fall != 0 {
            ps[0] = self.found_state;
            num_ps = 1;
        }
        while self.suffix(c) != 0 && !self.broken {
            c = self.suffix(c);
            let s = if self.num_stats(c) != 1 {
                let sym = self.symbol(self.found_state);
                self.find_state(c, sym)
            } else {
                Self::one_state(c)
            };
            let successor = self.successor(s);
            if successor != up_branch {
                c = successor;
                if num_ps == 0 {
                    return Some(c);
                }
                break;
            }
            match ps.get_mut(num_ps) {
                Some(p) => *p = s,
                None => {
                    self.broken = true;
                    return Some(c);
                }
            }
            num_ps += 1;
        }

        let new_sym = u32::from(self.rd8(up_branch));
        up_branch += 1;
        let new_freq = if self.num_stats(c) == 1 {
            self.freq(Self::one_state(c))
        } else {
            let s = self.find_state(c, new_sym);
            let cf = self.freq(s).wrapping_sub(1);
            let s0 = self
                .summ_freq(c)
                .wrapping_sub(self.num_stats(c))
                .wrapping_sub(cf);
            1 + if 2 * cf <= s0 {
                u32::from(5 * cf > s0)
            } else {
                match (2 * cf + s0 - 1).checked_div(2 * s0) {
                    Some(q) => q + 1,
                    None => {
                        self.broken = true;
                        1
                    }
                }
            }
        };

        loop {
            let c1 = if self.hi_unit != self.lo_unit {
                self.hi_unit -= UNIT_SIZE;
                self.hi_unit
            } else if self.free_list[0] != 0 {
                self.remove_node(0)
            } else {
                self.alloc_units_rare(0)?
            };
            self.set_num_stats(c1, 1);
            let one = Self::one_state(c1);
            self.set_symbol(one, new_sym);
            self.set_freq(one, new_freq);
            self.set_successor(one, up_branch);
            self.set_suffix(c1, c);
            // `ps[--numPs]`: the model never comes here with none.
            let Some(n) = num_ps.checked_sub(1) else {
                self.broken = true;
                return Some(c1);
            };
            num_ps = n;
            let p = ps.get(num_ps).copied().unwrap_or(0);
            self.set_successor(p, c1);
            c = c1;
            if num_ps == 0 || self.broken {
                break;
            }
        }
        Some(c)
    }

    /// `Ppmd7_UpdateModel`
    #[allow(clippy::too_many_lines)]
    fn update_model(&mut self) {
        let fs = self.found_state;
        let fs_freq = self.freq(fs);
        let fs_sym = self.symbol(fs);
        if fs_freq < MAX_FREQ / 4 && self.suffix(self.min_context) != 0 {
            let c = self.suffix(self.min_context);
            if self.num_stats(c) == 1 {
                let s = Self::one_state(c);
                let f = self.freq(s);
                if f < 32 {
                    self.set_freq(s, f + 1);
                }
            } else {
                let mut s = self.stats(c);
                if self.symbol(s) != fs_sym {
                    s = self.find_state(c, fs_sym);
                    if self.freq(s) >= self.freq(s - STATE_SIZE) {
                        self.swap_states(s);
                        s -= STATE_SIZE;
                    }
                }
                let f = self.freq(s);
                if f < MAX_FREQ - 9 {
                    self.set_freq(s, f + 2);
                    let sf = self.summ_freq(c);
                    self.set_summ_freq(c, sf + 2);
                }
            }
        }

        if self.order_fall == 0 {
            match self.create_successors() {
                Some(c) => {
                    self.min_context = c;
                    self.max_context = c;
                    self.set_successor(self.found_state, c);
                }
                None => self.restart_model(),
            }
            return;
        }

        self.wr8(self.text, byte(fs_sym));
        self.text += 1;
        if self.text >= self.units_start {
            self.restart_model();
            return;
        }
        let mut max_successor = self.text;
        let mut min_successor = self.successor(self.found_state);

        if min_successor != 0 {
            if min_successor <= max_successor {
                match self.create_successors() {
                    Some(cs) => min_successor = cs,
                    None => {
                        self.restart_model();
                        return;
                    }
                }
            }
            self.order_fall -= 1;
            if self.order_fall == 0 {
                max_successor = min_successor;
                self.text -= u32::from(self.max_context != self.min_context);
            }
        } else {
            self.set_successor(self.found_state, max_successor);
            min_successor = self.min_context;
        }

        let mc = self.min_context;
        let mut c = self.max_context;
        self.max_context = min_successor;
        self.min_context = min_successor;
        if c == mc {
            return;
        }

        let ns = self.num_stats(mc);
        let s0 = self
            .summ_freq(mc)
            .wrapping_sub(ns)
            .wrapping_sub(self.freq(self.found_state).wrapping_sub(1));

        loop {
            let ns1 = self.num_stats(c);
            let mut sum;
            if ns1 != 1 {
                if ns1 & 1 == 0 {
                    let old_nu = ns1 >> 1;
                    let i = self.u2i(old_nu);
                    if i != self.u2i(old_nu + 1) {
                        let Some(ptr) = self.alloc_units(i + 1) else {
                            self.restart_model();
                            return;
                        };
                        let old_ptr = self.stats(c);
                        self.copy(ptr, old_ptr, old_nu * UNIT_SIZE);
                        self.insert_node(old_ptr, i);
                        self.set_stats(c, ptr);
                    }
                }
                sum = self.summ_freq(c);
                sum += u32::from(2 * ns1 < ns)
                    + 2 * (u32::from(4 * ns1 <= ns) & u32::from(sum <= 8 * ns1));
            } else {
                let Some(s) = self.alloc_units(0) else {
                    self.restart_model();
                    return;
                };
                let one = Self::one_state(c);
                let mut freq = self.freq(one);
                // The one state, Successor and all, to the new list.
                self.copy(s, one, STATE_SIZE);
                self.set_stats(c, s);
                if freq < MAX_FREQ / 4 - 1 {
                    freq <<= 1;
                } else {
                    freq = MAX_FREQ - 4;
                }
                self.set_freq(s, freq);
                sum = freq + self.init_esc + u32::from(ns > 3);
            }

            let s = self.stats(c) + ns1 * STATE_SIZE;
            let mut cf = 2 * (sum + 6) * self.freq(self.found_state);
            let sf = s0.wrapping_add(sum);
            self.set_symbol(s, fs_sym);
            self.set_num_stats(c, ns1 + 1);
            self.set_successor(s, max_successor);
            if cf < 6 * sf {
                cf = 1 + u32::from(cf > sf) + u32::from(cf >= 4 * sf);
                sum += 3;
            } else {
                cf = 4
                    + u32::from(cf >= 9 * sf)
                    + u32::from(cf >= 12 * sf)
                    + u32::from(cf >= 15 * sf);
                sum += cf;
            }
            self.set_summ_freq(c, sum);
            self.set_freq(s, cf);
            c = self.suffix(c);
            if c == mc || self.broken {
                break;
            }
        }
    }

    /// `Ppmd7_Rescale`: halves the frequencies of `min_context`'s symbols,
    /// keeping them sorted, and drops those that reach 0 (only in a
    /// max-order context).
    #[allow(clippy::too_many_lines)]
    fn rescale(&mut self) {
        let mc = self.min_context;
        let stats = self.stats(mc);
        let mut s = self.found_state;

        // The found state to the front.
        if s != stats {
            let mut tmp = [0u8; STATE_SIZE as usize];
            self.read_state(s, &mut tmp);
            while s != stats && !self.broken {
                self.copy(s, s - STATE_SIZE, STATE_SIZE);
                s -= STATE_SIZE;
            }
            self.write_state(s, &tmp);
        }

        let mut sum_freq = self.freq(s);
        let mut esc_freq = self.summ_freq(mc).wrapping_sub(sum_freq);
        let adder = u32::from(self.order_fall != 0);
        sum_freq = (sum_freq + 4 + adder) >> 1;
        let mut i = self.num_stats(mc).wrapping_sub(1);
        self.set_freq(s, sum_freq);

        while i != 0 && !self.broken {
            s += STATE_SIZE;
            let mut freq = self.freq(s);
            esc_freq = esc_freq.wrapping_sub(freq);
            freq = (freq + adder) >> 1;
            sum_freq += freq;
            self.set_freq(s, freq);
            if freq > self.freq(s - STATE_SIZE) {
                let mut tmp = [0u8; STATE_SIZE as usize];
                self.read_state(s, &mut tmp);
                let mut s1 = s;
                loop {
                    self.copy(s1, s1 - STATE_SIZE, STATE_SIZE);
                    s1 -= STATE_SIZE;
                    if s1 == stats || freq <= self.freq(s1 - STATE_SIZE) || self.broken {
                        break;
                    }
                }
                self.write_state(s1, &tmp);
            }
            i -= 1;
        }

        if self.freq(s) == 0 {
            let mut i = 0u32;
            loop {
                i += 1;
                s -= STATE_SIZE;
                if self.freq(s) != 0 || self.broken {
                    break;
                }
            }
            esc_freq = esc_freq.wrapping_add(i);
            let num_stats = self.num_stats(mc);
            let num_stats_new = num_stats - i;
            self.set_num_stats(mc, num_stats_new);
            let n0 = (num_stats + 1) >> 1;

            if num_stats_new == 1 {
                let mut freq = self.freq(stats);
                loop {
                    esc_freq >>= 1;
                    freq = (freq + 1) >> 1;
                    if esc_freq <= 1 {
                        break;
                    }
                }
                let one = Self::one_state(mc);
                self.copy(one, stats, STATE_SIZE);
                self.set_freq(one, freq);
                self.found_state = one;
                let indx = self.u2i(n0);
                self.insert_node(stats, indx);
                return;
            }

            let n1 = (num_stats_new + 1) >> 1;
            if n0 != n1 {
                let i0 = self.u2i(n0);
                let i1 = self.u2i(n1);
                if i0 != i1 {
                    if self.free_list.get(i1).copied().unwrap_or(0) != 0 {
                        let ptr = self.remove_node(i1);
                        self.set_stats(mc, ptr);
                        self.copy(ptr, stats, n1 * UNIT_SIZE);
                        self.insert_node(stats, i0);
                    } else {
                        self.split_block(stats, i0, i1);
                    }
                }
            }
        }
        self.set_summ_freq(
            mc,
            sum_freq.wrapping_add(esc_freq).wrapping_sub(esc_freq >> 1),
        );
        self.found_state = self.stats(mc);
    }

    fn read_state(&mut self, s: u32, out: &mut [u8; STATE_SIZE as usize]) {
        let o = s as usize;
        match self.mem.get(o..o.saturating_add(STATE_SIZE as usize)) {
            Some(b) => out.copy_from_slice(b),
            None => self.broken = true,
        }
    }

    fn write_state(&mut self, s: u32, v: &[u8; STATE_SIZE as usize]) {
        let o = s as usize;
        match self.mem.get_mut(o..o.saturating_add(STATE_SIZE as usize)) {
            Some(b) => b.copy_from_slice(v),
            None => self.broken = true,
        }
    }

    /// `Ppmd7_MakeEscFreq`: the escape estimator for `min_context` with
    /// `num_masked` symbols masked, and the escape frequency.
    fn make_esc_freq(&mut self, num_masked: u32) -> (SeeRef, u32) {
        let mc = self.min_context;
        let num_stats = self.num_stats(mc);
        if num_stats == 256 {
            return (SeeRef::Dummy, 1);
        }
        let non_masked = num_stats.wrapping_sub(num_masked);
        let row = usize::from(
            self.ns2indx
                .get((non_masked as usize).wrapping_sub(1))
                .copied()
                .unwrap_or(0),
        );
        let suffix = self.suffix(mc);
        let suffix_stats = self.num_stats(suffix);
        let col = u32::from(non_masked < suffix_stats.wrapping_sub(num_stats))
            + 2 * u32::from(self.summ_freq(mc) < 11 * num_stats)
            + 4 * u32::from(num_masked > non_masked)
            + self.hi_bits_flag;
        let col = col as usize;
        let Some(see) = self.see.get_mut(row).and_then(|r| r.get_mut(col)) else {
            self.broken = true;
            return (SeeRef::Dummy, 1);
        };
        let summ = u32::from(see.summ);
        let r = summ >> see.shift;
        see.summ = low16(summ - r);
        (SeeRef::Table(row, col), r + u32::from(r == 0))
    }

    fn see_mut(&mut self, see: SeeRef) -> &mut See {
        match see {
            SeeRef::Table(r, c) => match self.see.get_mut(r).and_then(|row| row.get_mut(c)) {
                Some(s) => s,
                None => &mut self.dummy_see,
            },
            SeeRef::Dummy => &mut self.dummy_see,
        }
    }

    /// `Ppmd7_NextContext`
    fn next_context(&mut self) {
        let c = self.successor(self.found_state);
        if self.order_fall == 0 && c > self.text {
            self.max_context = c;
            self.min_context = c;
        } else {
            self.update_model();
        }
    }

    /// `Ppmd7_Update1`
    fn update1(&mut self) {
        let mut s = self.found_state;
        let freq = self.freq(s) + 4;
        let mc = self.min_context;
        let sf = self.summ_freq(mc);
        self.set_summ_freq(mc, sf + 4);
        self.set_freq(s, freq);
        if freq > self.freq(s - STATE_SIZE) {
            self.swap_states(s);
            s -= STATE_SIZE;
            self.found_state = s;
            if freq > MAX_FREQ {
                self.rescale();
            }
        }
        self.next_context();
    }

    /// `Ppmd7_Update1_0`
    fn update1_0(&mut self) {
        let s = self.found_state;
        let mc = self.min_context;
        let freq = self.freq(s);
        let summ_freq = self.summ_freq(mc);
        self.prev_success = u32::from(2 * freq > summ_freq);
        self.run_length += i32::from(self.prev_success != 0);
        self.set_summ_freq(mc, summ_freq + 4);
        self.set_freq(s, freq + 4);
        if freq + 4 > MAX_FREQ {
            self.rescale();
        }
        self.next_context();
    }

    /// `Ppmd7_Update2`
    fn update2(&mut self) {
        let s = self.found_state;
        let freq = self.freq(s) + 4;
        self.run_length = self.init_rl;
        let mc = self.min_context;
        let sf = self.summ_freq(mc);
        self.set_summ_freq(mc, sf + 4);
        self.set_freq(s, freq);
        if freq > MAX_FREQ {
            self.rescale();
        }
        self.update_model();
    }

    // ---------- the range decoder (`Ppmd7z_RangeDec`) ----------

    /// `IByteIn_Read` on `CByteInBufWrap`: past the end, 0 and `extra`.
    fn read_byte(&mut self) -> u32 {
        match self.input.get(self.pos) {
            Some(&b) => {
                self.pos += 1;
                u32::from(b)
            }
            None => {
                self.extra = true;
                0
            }
        }
    }

    /// `Ppmd7z_RangeDec_Init`
    fn range_dec_init(&mut self) -> bool {
        self.code = 0;
        self.range = 0xFFFF_FFFF;
        if self.read_byte() != 0 {
            return false;
        }
        for _ in 0..4 {
            self.code = (self.code << 8) | self.read_byte();
        }
        self.code < 0xFFFF_FFFF
    }

    /// `RC_NORM`: up to two bytes.
    fn rc_norm(&mut self) {
        if self.range < TOP {
            self.code = (self.code << 8) | self.read_byte();
            self.range <<= 8;
            if self.range < TOP {
                self.code = (self.code << 8) | self.read_byte();
                self.range <<= 8;
            }
        }
    }

    /// `RC_Decode`
    fn rc_decode(&mut self, start: u32, size: u32) {
        self.code = self.code.wrapping_sub(start.wrapping_mul(self.range));
        self.range = self.range.wrapping_mul(size);
    }

    /// `RC_DecodeFinal`
    fn rc_decode_final(&mut self, start: u32, size: u32) {
        self.rc_decode(start, size);
        self.rc_norm();
    }

    /// `RC_GetThreshold`
    fn rc_get_threshold(&mut self, total: u32) -> u32 {
        self.range = self.range.checked_div(total).unwrap_or(0);
        match self.code.checked_div(self.range) {
            Some(q) => q,
            None => {
                self.broken = true;
                0
            }
        }
    }

    /// `Ppmd7z_DecodeSymbol`
    #[allow(clippy::too_many_lines)]
    fn decode_symbol(&mut self) -> Symbol {
        // `charMask`: 0xFF for a symbol not yet masked.
        let mut char_mask = [0xFFu8; 256];

        let mc = self.min_context;
        if self.num_stats(mc) != 1 {
            let mut s = self.stats(mc);
            let summ_freq = self.summ_freq(mc);
            let mut count = self.rc_get_threshold(summ_freq);
            let hi_cnt = count;
            count = count.wrapping_sub(self.freq(s));
            if (count as i32) < 0 {
                let f = self.freq(s);
                self.rc_decode_final(0, f);
                self.found_state = s;
                let sym = self.symbol(s);
                self.update1_0();
                return self.done(sym);
            }
            self.prev_success = 0;
            let mut i = self.num_stats(mc).wrapping_sub(1);
            while i != 0 && !self.broken {
                s += STATE_SIZE;
                let f = self.freq(s);
                count = count.wrapping_sub(f);
                if (count as i32) < 0 {
                    self.rc_decode_final(hi_cnt.wrapping_sub(count).wrapping_sub(f), f);
                    self.found_state = s;
                    let sym = self.symbol(s);
                    self.update1();
                    return self.done(sym);
                }
                i -= 1;
            }
            if hi_cnt >= summ_freq {
                return Symbol::Error;
            }
            let hi = hi_cnt.wrapping_sub(count);
            self.rc_decode(hi, summ_freq.wrapping_sub(hi));
            let fs_sym = self.symbol(self.found_state);
            self.hi_bits_flag = hi_bits_flag_3(fs_sym);
            // Every symbol of the context masked.
            let n = self.num_stats(mc);
            let mut s2 = self.stats(mc);
            for _ in 0..n {
                let sym = self.symbol(s2) as usize;
                if let Some(m) = char_mask.get_mut(sym) {
                    *m = 0;
                }
                s2 += STATE_SIZE;
            }
        } else {
            let s = Self::one_state(mc);
            let freq = self.freq(s);
            let sym_s = self.symbol(s);
            let suffix = self.suffix(mc);
            let suffix_stats = self.num_stats(suffix);
            let fs_sym = self.symbol(self.found_state);
            self.hi_bits_flag = hi_bits_flag_3(fs_sym);
            let row = freq.wrapping_sub(1) as usize;
            let col = self.prev_success
                + ((self.run_length >> 26) as u32 & 0x20)
                + u32::from(
                    self.ns2bs_indx
                        .get((suffix_stats as usize).wrapping_sub(1))
                        .copied()
                        .unwrap_or(0),
                )
                + hi_bits_flag_4(sym_s)
                + self.hi_bits_flag;
            let Some(prob) = self
                .bin_summ
                .get_mut(row)
                .and_then(|r| r.get_mut(col as usize))
            else {
                self.broken = true;
                return Symbol::Error;
            };
            let mut pr = u32::from(*prob);
            let size0 = (self.range >> 14) * pr;
            pr -= get_mean(pr);
            if self.code < size0 {
                *prob = low16(pr + (1 << INT_BITS));
                self.range = size0;
                // RC_NORM_1
                if self.range < TOP {
                    self.code = (self.code << 8) | self.read_byte();
                    self.range <<= 8;
                }
                let c = self.successor(s);
                self.found_state = s;
                self.prev_success = 1;
                self.run_length += 1;
                self.set_freq(s, freq + u32::from(freq < 128));
                if self.order_fall == 0 && c > self.text {
                    self.max_context = c;
                    self.min_context = c;
                } else {
                    self.update_model();
                }
                return self.done(sym_s);
            }
            *prob = low16(pr);
            self.init_esc = u32::from(EXP_ESCAPE.get((pr >> 10) as usize).copied().unwrap_or(2));
            self.code = self.code.wrapping_sub(size0);
            self.range = self.range.wrapping_sub(size0);
            if let Some(m) = char_mask.get_mut(sym_s as usize) {
                *m = 0;
            }
            self.prev_success = 0;
        }

        loop {
            self.rc_norm();
            let mut mc = self.min_context;
            let num_masked = self.num_stats(mc);
            loop {
                self.order_fall += 1;
                let suffix = self.suffix(mc);
                if suffix == 0 {
                    return Symbol::End;
                }
                mc = suffix;
                if self.num_stats(mc) != num_masked || self.broken {
                    break;
                }
            }
            if self.broken {
                return Symbol::Error;
            }

            let stats = self.stats(mc);
            let num = self.num_stats(mc);
            self.min_context = mc;
            let mut hi_cnt = 0u32;
            let mut s = stats;
            for _ in 0..num {
                let sym = self.symbol(s) as usize;
                let mask = u32::from(char_mask.get(sym).copied().unwrap_or(0));
                hi_cnt += self.freq(s) & mask;
                s += STATE_SIZE;
            }

            let (see, esc) = self.make_esc_freq(num_masked);
            let freq_sum = esc + hi_cnt;
            let mut count = self.rc_get_threshold(freq_sum);

            if count < hi_cnt {
                let mut s = stats;
                let hi = count;
                loop {
                    let sym = self.symbol(s) as usize;
                    let mask = u32::from(char_mask.get(sym).copied().unwrap_or(0));
                    count = count.wrapping_sub(self.freq(s) & mask);
                    s += STATE_SIZE;
                    if (count as i32) < 0 || self.broken {
                        break;
                    }
                }
                s -= STATE_SIZE;
                let f = self.freq(s);
                self.rc_decode_final(hi.wrapping_sub(count).wrapping_sub(f), f);
                // Ppmd_See_UPDATE
                let see = self.see_mut(see);
                if u32::from(see.shift) < PERIOD_BITS {
                    see.count = see.count.wrapping_sub(1);
                    if see.count == 0 {
                        see.summ = see.summ.wrapping_shl(1);
                        see.count = byte(3u32 << see.shift);
                        see.shift += 1;
                    }
                }
                self.found_state = s;
                let sym = self.symbol(s);
                self.update2();
                return self.done(sym);
            }

            if count >= freq_sum {
                return Symbol::Error;
            }
            self.rc_decode(hi_cnt, freq_sum - hi_cnt);
            let see = self.see_mut(see);
            see.summ = low16(u32::from(see.summ) + freq_sum);

            let mut s = stats;
            for _ in 0..num {
                let sym = self.symbol(s) as usize;
                if let Some(m) = char_mask.get_mut(sym) {
                    *m = 0;
                }
                s += STATE_SIZE;
            }
        }
    }

    /// A decoded symbol, unless the model broke on the way.
    fn done(&self, sym: u32) -> Symbol {
        if self.broken {
            Symbol::Error
        } else {
            Symbol::Byte(byte(sym))
        }
    }
}

/// Why a PPMd coder could not start: 7-Zip's `SetDecoderProperties2`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Refused {
    /// `E_NOTIMPL`: an order or a memory size outside PPMd's -- or fewer
    /// than five property bytes, `E_INVALIDARG`, which `7zDecode.cpp`
    /// reports as `E_NOTIMPL` too.
    Unsupported,
    /// The model's memory passes the caller's limit (7-Zip would try to
    /// allocate it, and fail as out of memory where it could not).
    TooLarge,
}

/// `NCompress::NPpmd::CDecoder::Code` in finish mode: `input` is the
/// packed stream, `out_size` the coder's unpacked size, `mem_limit` the
/// most memory the model may have.
pub(crate) fn decode(
    props: &[u8],
    input: &[u8],
    out_size: usize,
    mem_limit: usize,
) -> Result<crate::lzma_coder::Coded, Refused> {
    let &[order, m0, m1, m2, m3, ..] = props else {
        return Err(Refused::Unsupported);
    };
    let order = u32::from(order);
    let mem_size = u32::from_le_bytes([m0, m1, m2, m3]);
    if !(MIN_ORDER..=MAX_ORDER as u32).contains(&order)
        || !(MIN_MEM_SIZE..=MAX_MEM_SIZE).contains(&mem_size)
    {
        return Err(Refused::Unsupported);
    }
    if mem_size as usize > mem_limit {
        return Err(Refused::TooLarge);
    }

    let mut ppmd = Ppmd7::new(mem_size, input);
    let mut out = Vec::new();
    // `CodeSpec`: kStatus_NeedInit.
    if !ppmd.range_dec_init() || ppmd.extra {
        return Ok(crate::lzma_coder::Coded { out, ok: false });
    }
    ppmd.init(order);

    let mut sym = Symbol::Byte(0);
    while out.len() < out_size {
        sym = ppmd.decode_symbol();
        match sym {
            Symbol::Byte(b) if !ppmd.extra => out.push(b),
            _ => break,
        }
    }
    if ppmd.extra {
        // CHECK_EXTRA_ERROR: the stream ran out.
        return Ok(crate::lzma_coder::Coded { out, ok: false });
    }
    let ok = match sym {
        // All the output decoded: the range coder must be finished.
        Symbol::Byte(_) => out.len() == out_size && ppmd.code == 0,
        // An end marker: the range coder finished, and in finish mode the
        // output its size.
        Symbol::End => ppmd.code == 0 && out.len() == out_size,
        Symbol::Error => false,
    };
    // `Code`: the packed stream must be used up.
    let ok = ok && ppmd.pos == input.len();
    Ok(crate::lzma_coder::Coded { out, ok })
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )]

    extern crate std;

    use core::sync::atomic::{AtomicU32, Ordering};

    use super::*;

    /// How many times a model was started, the first time included.
    pub(super) static RESTARTS: AtomicU32 = AtomicU32::new(0);

    /// 7-Zip's archive of 105 KB with 64 KiB of model decodes byte for
    /// byte (`archives_7zip_made_are_read`) -- and it is the restarts that
    /// make that worth anything: the model fills and starts again many
    /// times, each at the byte the allocator decides.
    #[test]
    fn the_small_model_restarts_and_still_decodes() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/data/made/ppmd-small-mem.7z");
        let data = std::fs::read(path).unwrap();
        let archive = crate::Archive::open(&data).unwrap();
        let before = RESTARTS.load(Ordering::Relaxed);
        for entry in archive.entries() {
            if !entry.is_dir() {
                entry.read(1 << 24).unwrap();
            }
        }
        let restarts = RESTARTS.load(Ordering::Relaxed) - before;
        // Each read decodes the whole solid folder: count one folder's.
        let files = archive.entries().filter(|e| e.has_stream()).count() as u32;
        assert!(restarts / files > 3, "{restarts} starts over {files} reads");
    }

    #[test]
    fn properties_7zip_refuses_are_refused() {
        // Order 2 to 64, memory 2 KiB to 4 GiB - 36, five bytes at least.
        let mem = (1u32 << 16).to_le_bytes();
        let ok = [6, mem[0], mem[1], mem[2], mem[3]];
        assert!(decode(&ok, &[0; 5], 0, 1 << 20).is_ok());
        assert_eq!(
            decode(&ok[..4], &[0; 5], 0, 1 << 20).err(),
            Some(Refused::Unsupported)
        );
        for order in [0u8, 1, 65] {
            let p = [order, mem[0], mem[1], mem[2], mem[3]];
            assert_eq!(
                decode(&p, &[0; 5], 0, 1 << 20).err(),
                Some(Refused::Unsupported)
            );
        }
        let small = (MIN_MEM_SIZE - 1).to_le_bytes();
        let p = [6, small[0], small[1], small[2], small[3]];
        assert_eq!(
            decode(&p, &[0; 5], 0, 1 << 20).err(),
            Some(Refused::Unsupported)
        );
        // Memory past the caller's limit is refused before it is taken.
        assert_eq!(
            decode(&ok, &[0; 5], 0, 1 << 15).err(),
            Some(Refused::TooLarge)
        );
    }

    #[test]
    fn the_range_coder_must_start_with_a_zero_and_end_finished() {
        let mem = (1u32 << 16).to_le_bytes();
        let props = [6, mem[0], mem[1], mem[2], mem[3]];
        // A first byte that is not zero.
        let c = decode(&props, &[1, 0, 0, 0, 0], 0, 1 << 20).unwrap();
        assert!(!c.ok);
        // Nothing to decode, the range coder finished, the input used up.
        let c = decode(&props, &[0; 5], 0, 1 << 20).unwrap();
        assert!(c.ok && c.out.is_empty());
        // ... but not with input left over, or a coder not finished.
        assert!(!decode(&props, &[0; 6], 0, 1 << 20).unwrap().ok);
        assert!(!decode(&props, &[0, 0, 0, 0, 1], 0, 1 << 20).unwrap().ok);
        // Too short to start the range coder.
        assert!(!decode(&props, &[0; 4], 0, 1 << 20).unwrap().ok);
    }
}
