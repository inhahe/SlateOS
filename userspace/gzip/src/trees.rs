//! `trees.c`: tallying the literals and matches deflate finds, and writing
//! each block with whichever of dynamic Huffman trees, the fixed trees, or
//! no compression at all makes it smallest.

// Table code over fixed-size arrays, indexed by values the tables'
// construction bounds: a symbol below `L_CODES`, a code length below
// `MAX_BITS + 1`, a heap index below `HEAP_SIZE`. Transcribed from C.

use crate::bits::bi_reverse;
use crate::{Gz, Io, MAX_MATCH, MIN_MATCH, Res};

/// All codes must not exceed this many bits.
const MAX_BITS: i32 = 15;
/// Bit-length codes must not exceed this many bits.
const MAX_BL_BITS: i32 = 7;
/// Length codes, not counting the end-of-block code.
const LENGTH_CODES: usize = 29;
/// Literal bytes.
const LITERALS: usize = 256;
/// The end-of-block literal.
const END_BLOCK: usize = 256;
/// Literal and length codes, end of block included.
pub const L_CODES: usize = LITERALS + 1 + LENGTH_CODES;
/// Distance codes.
const D_CODES: usize = 30;
/// Codes that send the bit lengths.
const BL_CODES: usize = 19;

/// Extra bits for each length code.
const EXTRA_LBITS: [i32; LENGTH_CODES] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
/// Extra bits for each distance code.
const EXTRA_DBITS: [i32; D_CODES] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];
/// Extra bits for each bit-length code.
const EXTRA_BLBITS: [i32; BL_CODES] = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 3, 7];

/// The three kinds of block.
const STORED_BLOCK: i32 = 0;
const STATIC_TREES: i32 = 1;
const DYN_TREES: i32 = 2;

/// The literal buffer's size (it is `inbuf`) and the distance buffer's.
const LIT_BUFSIZE: u32 = 0x8000;
const DIST_BUFSIZE: u32 = 0x8000;

/// Repeat the previous length 3-6 times; a zero length 3-10 times; 11-138.
const REP_3_6: usize = 16;
const REPZ_3_10: usize = 17;
const REPZ_11_138: usize = 18;

/// The heap's size.
const HEAP_SIZE: usize = 2 * L_CODES + 1;

/// The order the bit-length code lengths are sent in.
const BL_ORDER: [usize; BL_CODES] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

/// `ct_data`: two C unions. `fc` is a frequency while a tree is built and
/// the code's bits after; `dl` is the father's index while built and the
/// code's length after. Upstream reads each as either, and so does this.
#[derive(Debug, Default, Clone, Copy)]
pub struct CtData {
    /// `fc.freq` / `fc.code`.
    pub fc: u16,
    /// `dl.dad` / `dl.len`.
    pub dl: u16,
}

/// Which tree a descriptor describes: upstream's three `tree_desc`s.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Which {
    L,
    D,
    Bl,
}

/// The trees' state: upstream's statics in `trees.c`.
#[derive(Debug)]
pub struct Ct {
    dyn_ltree: [CtData; HEAP_SIZE],
    dyn_dtree: [CtData; 2 * D_CODES + 1],
    static_ltree: [CtData; L_CODES + 2],
    static_dtree: [CtData; D_CODES],
    bl_tree: [CtData; 2 * BL_CODES + 1],
    /// Each descriptor's `max_code`: literal, distance, bit length.
    max_code: [i32; 3],
    bl_count: [u16; MAX_BITS as usize + 1],
    heap: [i32; 2 * L_CODES + 1],
    heap_len: i32,
    heap_max: i32,
    depth: [u8; 2 * L_CODES + 1],
    length_code: [u8; (MAX_MATCH - MIN_MATCH + 1) as usize],
    dist_code: [u8; 512],
    base_length: [i32; LENGTH_CODES],
    base_dist: [i32; D_CODES],
    flag_buf: [u8; (LIT_BUFSIZE / 8) as usize],
    last_lit: u32,
    last_dist: u32,
    last_flags: u32,
    flags: u8,
    flag_bit: u8,
    opt_len: u64,
    static_len: u64,
    compressed_len: i64,
    input_len: i64,
    /// `*file_type`: binary, text, or not known yet.
    file_type: u16,
}

impl Default for Ct {
    fn default() -> Self {
        Ct {
            dyn_ltree: [CtData::default(); HEAP_SIZE],
            dyn_dtree: [CtData::default(); 2 * D_CODES + 1],
            static_ltree: [CtData::default(); L_CODES + 2],
            static_dtree: [CtData::default(); D_CODES],
            bl_tree: [CtData::default(); 2 * BL_CODES + 1],
            max_code: [0; 3],
            bl_count: [0; MAX_BITS as usize + 1],
            heap: [0; 2 * L_CODES + 1],
            heap_len: 0,
            heap_max: 0,
            depth: [0; 2 * L_CODES + 1],
            length_code: [0; (MAX_MATCH - MIN_MATCH + 1) as usize],
            dist_code: [0; 512],
            base_length: [0; LENGTH_CODES],
            base_dist: [0; D_CODES],
            flag_buf: [0; (LIT_BUFSIZE / 8) as usize],
            last_lit: 0,
            last_dist: 0,
            last_flags: 0,
            flags: 0,
            flag_bit: 0,
            opt_len: 0,
            static_len: 0,
            compressed_len: 0,
            input_len: 0,
            file_type: 0,
        }
    }
}

/// The file type's "not known yet", binary and text.
const UNKNOWN: u16 = 0xffff;
const BINARY: u16 = 0;
const ASCII: u16 = 1;

impl Ct {
    fn tree(&self, w: Which) -> &[CtData] {
        match w {
            Which::L => &self.dyn_ltree,
            Which::D => &self.dyn_dtree,
            Which::Bl => &self.bl_tree,
        }
    }

    fn tree_mut(&mut self, w: Which) -> &mut [CtData] {
        match w {
            Which::L => &mut self.dyn_ltree,
            Which::D => &mut self.dyn_dtree,
            Which::Bl => &mut self.bl_tree,
        }
    }

    /// `d_code (dist)`: the distance code for `dist`, the distance less 1.
    fn d_code(&self, dist: u32) -> usize {
        if dist < 256 {
            self.dist_code[dist as usize] as usize
        } else {
            self.dist_code[256 + (dist >> 7) as usize] as usize
        }
    }

    /// `smaller (tree, n, m)`: by frequency, then by depth.
    fn smaller(&self, w: Which, n: i32, m: i32) -> bool {
        let tree = self.tree(w);
        let (fnn, fm) = (tree[n as usize].fc, tree[m as usize].fc);
        fnn < fm || (fnn == fm && self.depth[n as usize] <= self.depth[m as usize])
    }

    /// `pqdownheap (tree, k)`.
    fn pqdownheap(&mut self, w: Which, mut k: i32) {
        let v = self.heap[k as usize];
        let mut j = k << 1;
        while j <= self.heap_len {
            if j < self.heap_len
                && self.smaller(w, self.heap[j as usize + 1], self.heap[j as usize])
            {
                j += 1;
            }
            if self.smaller(w, v, self.heap[j as usize]) {
                break;
            }
            self.heap[k as usize] = self.heap[j as usize];
            k = j;
            j <<= 1;
        }
        self.heap[k as usize] = v;
    }

    /// `pqremove (tree, top)`.
    fn pqremove(&mut self, w: Which) -> i32 {
        let top = self.heap[1];
        self.heap[1] = self.heap[self.heap_len as usize];
        self.heap_len -= 1;
        self.pqdownheap(w, 1);
        top
    }

    /// The descriptor's fixed parts: static tree, extra bits and their
    /// base, elements, maximum length.
    fn desc(w: Which) -> (Option<()>, &'static [i32], i32, i32, i32) {
        match w {
            Which::L => (
                Some(()),
                &EXTRA_LBITS,
                LITERALS as i32 + 1,
                L_CODES as i32,
                MAX_BITS,
            ),
            Which::D => (Some(()), &EXTRA_DBITS, 0, D_CODES as i32, MAX_BITS),
            Which::Bl => (None, &EXTRA_BLBITS, 0, BL_CODES as i32, MAX_BL_BITS),
        }
    }

    /// The static tree's length for `n`, for the descriptor `w`.
    fn static_len_of(&self, w: Which, n: usize) -> u16 {
        match w {
            Which::L => self.static_ltree[n].dl,
            Which::D => self.static_dtree[n].dl,
            Which::Bl => 0,
        }
    }

    fn widx(w: Which) -> usize {
        match w {
            Which::L => 0,
            Which::D => 1,
            Which::Bl => 2,
        }
    }

    /// `gen_bitlen (desc)`.
    fn gen_bitlen(&mut self, w: Which) {
        let (stree, extra, base, _, max_length) = Self::desc(w);
        let max_code = self.max_code[Self::widx(w)];
        let mut overflow = 0;

        self.bl_count = [0; MAX_BITS as usize + 1];

        let root = self.heap[self.heap_max as usize] as usize;
        self.tree_mut(w)[root].dl = 0;

        let mut h = self.heap_max as usize + 1;
        while h < HEAP_SIZE {
            let n = self.heap[h] as usize;
            let dad = self.tree(w)[n].dl as usize;
            let mut bits = i32::from(self.tree(w)[dad].dl) + 1;
            if bits > max_length {
                bits = max_length;
                overflow += 1;
            }
            self.tree_mut(w)[n].dl = bits as u16;
            h += 1;
            if n as i32 > max_code {
                continue;
            }
            self.bl_count[bits as usize] += 1;
            let mut xbits = 0;
            if n as i32 >= base {
                xbits = extra[n - base as usize];
            }
            let f = u64::from(self.tree(w)[n].fc);
            self.opt_len = self
                .opt_len
                .wrapping_add(f.wrapping_mul((bits + xbits) as u64));
            if stree.is_some() {
                let slen = i32::from(self.static_len_of(w, n));
                self.static_len = self
                    .static_len
                    .wrapping_add(f.wrapping_mul((slen + xbits) as u64));
            }
        }
        if overflow == 0 {
            return;
        }

        loop {
            let mut bits = max_length - 1;
            while self.bl_count[bits as usize] == 0 {
                bits -= 1;
            }
            self.bl_count[bits as usize] -= 1;
            self.bl_count[bits as usize + 1] += 2;
            self.bl_count[max_length as usize] = self.bl_count[max_length as usize].wrapping_sub(1);
            overflow -= 2;
            if overflow <= 0 {
                break;
            }
        }

        let mut h = HEAP_SIZE;
        let mut bits = max_length;
        while bits != 0 {
            let mut n = self.bl_count[bits as usize];
            while n != 0 {
                h -= 1;
                let m = self.heap[h];
                if m > max_code {
                    continue;
                }
                let m = m as usize;
                let len = self.tree(w)[m].dl;
                if u32::from(len) != bits as u32 {
                    let delta = (i64::from(bits) - i64::from(len)) * i64::from(self.tree(w)[m].fc);
                    self.opt_len = self.opt_len.wrapping_add(delta as u64);
                    self.tree_mut(w)[m].dl = bits as u16;
                }
                n -= 1;
            }
            bits -= 1;
        }
    }

    /// `gen_codes (tree, max_code)`, over `tree` itself.
    fn gen_codes(bl_count: &[u16; MAX_BITS as usize + 1], tree: &mut [CtData], max_code: i32) {
        let mut next_code = [0u16; MAX_BITS as usize + 1];
        let mut code: u16 = 0;
        for bits in 1..=MAX_BITS as usize {
            code = code.wrapping_add(bl_count[bits - 1]).wrapping_shl(1);
            next_code[bits] = code;
        }
        for n in 0..=max_code.max(-1) {
            let n = n as usize;
            let len = tree[n].dl as usize;
            if len == 0 {
                continue;
            }
            tree[n].fc = bi_reverse(u32::from(next_code[len]), len as i32) as u16;
            next_code[len] = next_code[len].wrapping_add(1);
        }
    }

    /// `build_tree (desc)`.
    fn build_tree(&mut self, w: Which) {
        let (stree, _, _, elems, _) = Self::desc(w);
        let mut max_code: i32 = -1;
        let mut node = elems;

        self.heap_len = 0;
        self.heap_max = HEAP_SIZE as i32;

        for n in 0..elems as usize {
            if self.tree(w)[n].fc == 0 {
                self.tree_mut(w)[n].dl = 0;
            } else {
                self.heap_len += 1;
                self.heap[self.heap_len as usize] = n as i32;
                max_code = n as i32;
                self.depth[n] = 0;
            }
        }

        while self.heap_len < 2 {
            let new = if max_code < 2 {
                max_code += 1;
                max_code
            } else {
                0
            };
            self.heap_len += 1;
            self.heap[self.heap_len as usize] = new;
            let new = new as usize;
            self.tree_mut(w)[new].fc = 1;
            self.depth[new] = 0;
            self.opt_len = self.opt_len.wrapping_sub(1);
            if stree.is_some() {
                let slen = u64::from(self.static_len_of(w, new));
                self.static_len = self.static_len.wrapping_sub(slen);
            }
        }
        self.max_code[Self::widx(w)] = max_code;

        let mut n = self.heap_len / 2;
        while n >= 1 {
            self.pqdownheap(w, n);
            n -= 1;
        }

        loop {
            let n = self.pqremove(w);
            let m = self.heap[1];

            self.heap_max -= 1;
            self.heap[self.heap_max as usize] = n;
            self.heap_max -= 1;
            self.heap[self.heap_max as usize] = m;

            let (nu, mu) = (n as usize, m as usize);
            let tree = self.tree_mut(w);
            tree[node as usize].fc = tree[nu].fc.wrapping_add(tree[mu].fc);
            tree[nu].dl = node as u16;
            tree[mu].dl = node as u16;
            let (dn, dm) = (self.depth[nu], self.depth[mu]);
            self.depth[node as usize] = (if dn >= dm { dn } else { dm }).wrapping_add(1);

            self.heap[1] = node;
            node += 1;
            self.pqdownheap(w, 1);

            if self.heap_len < 2 {
                break;
            }
        }

        self.heap_max -= 1;
        self.heap[self.heap_max as usize] = self.heap[1];

        self.gen_bitlen(w);
        let bl_count = self.bl_count;
        Self::gen_codes(&bl_count, self.tree_mut(w), max_code);
    }

    /// `scan_tree (tree, max_code)`.
    fn scan_tree(&mut self, w: Which, max_code: i32) {
        let mut prevlen: i32 = -1;
        let mut nextlen = i32::from(self.tree(w)[0].dl);
        let mut count = 0;
        let mut max_count = 7;
        let mut min_count = 4;

        if nextlen == 0 {
            max_count = 138;
            min_count = 3;
        }
        self.tree_mut(w)[(max_code + 1) as usize].dl = 0xffff;

        for n in 0..=max_code as usize {
            let curlen = nextlen;
            nextlen = i32::from(self.tree(w)[n + 1].dl);
            count += 1;
            if count < max_count && curlen == nextlen {
                continue;
            }
            if count < min_count {
                self.bl_tree[curlen as usize].fc =
                    self.bl_tree[curlen as usize].fc.wrapping_add(count as u16);
            } else if curlen != 0 {
                if curlen != prevlen {
                    self.bl_tree[curlen as usize].fc =
                        self.bl_tree[curlen as usize].fc.wrapping_add(1);
                }
                self.bl_tree[REP_3_6].fc = self.bl_tree[REP_3_6].fc.wrapping_add(1);
            } else if count <= 10 {
                self.bl_tree[REPZ_3_10].fc = self.bl_tree[REPZ_3_10].fc.wrapping_add(1);
            } else {
                self.bl_tree[REPZ_11_138].fc = self.bl_tree[REPZ_11_138].fc.wrapping_add(1);
            }
            count = 0;
            prevlen = curlen;
            if nextlen == 0 {
                max_count = 138;
                min_count = 3;
            } else if curlen == nextlen {
                max_count = 6;
                min_count = 3;
            } else {
                max_count = 7;
                min_count = 4;
            }
        }
    }

    /// `build_bl_tree`: the index in [`BL_ORDER`] of the last bit-length
    /// code to send.
    fn build_bl_tree(&mut self) -> i32 {
        let (lmax, dmax) = (self.max_code[0], self.max_code[1]);
        self.scan_tree(Which::L, lmax);
        self.scan_tree(Which::D, dmax);
        self.build_tree(Which::Bl);

        let mut max_blindex = BL_CODES as i32 - 1;
        while max_blindex >= 3 {
            if self.bl_tree[BL_ORDER[max_blindex as usize]].dl != 0 {
                break;
            }
            max_blindex -= 1;
        }
        self.opt_len = self
            .opt_len
            .wrapping_add((3 * (max_blindex + 1) + 5 + 5 + 4) as u64);
        max_blindex
    }

    /// `set_file_type`.
    fn set_file_type(&mut self) {
        let mut n = 0;
        let mut ascii_freq: u32 = 0;
        let mut bin_freq: u32 = 0;
        while n < 7 {
            bin_freq = bin_freq.wrapping_add(u32::from(self.dyn_ltree[n].fc));
            n += 1;
        }
        while n < 128 {
            ascii_freq = ascii_freq.wrapping_add(u32::from(self.dyn_ltree[n].fc));
            n += 1;
        }
        while n < LITERALS {
            bin_freq = bin_freq.wrapping_add(u32::from(self.dyn_ltree[n].fc));
            n += 1;
        }
        self.file_type = if bin_freq > (ascii_freq >> 2) {
            BINARY
        } else {
            ASCII
        };
    }

    /// `init_block`.
    fn init_block(&mut self) {
        for n in 0..L_CODES {
            self.dyn_ltree[n].fc = 0;
        }
        for n in 0..D_CODES {
            self.dyn_dtree[n].fc = 0;
        }
        for n in 0..BL_CODES {
            self.bl_tree[n].fc = 0;
        }
        self.dyn_ltree[END_BLOCK].fc = 1;
        self.opt_len = 0;
        self.static_len = 0;
        self.last_lit = 0;
        self.last_dist = 0;
        self.last_flags = 0;
        self.flags = 0;
        self.flag_bit = 1;
    }
}

impl<I: Io> Gz<I> {
    /// `ct_init (attr, methodp)`: start a file; the tables are built on the
    /// first call of the process.
    pub fn ct_init(&mut self) {
        let ct = &mut self.ct;
        ct.file_type = 0;
        ct.compressed_len = 0;
        ct.input_len = 0;

        if ct.static_dtree[0].dl != 0 {
            return;
        }

        let mut length = 0usize;
        let mut code = 0usize;
        while code < LENGTH_CODES - 1 {
            ct.base_length[code] = length as i32;
            for _ in 0..(1 << EXTRA_LBITS[code]) {
                ct.length_code[length] = code as u8;
                length += 1;
            }
            code += 1;
        }
        ct.length_code[length - 1] = code as u8;

        let mut dist = 0usize;
        code = 0;
        while code < 16 {
            ct.base_dist[code] = dist as i32;
            for _ in 0..(1 << EXTRA_DBITS[code]) {
                ct.dist_code[dist] = code as u8;
                dist += 1;
            }
            code += 1;
        }
        dist >>= 7;
        while code < D_CODES {
            ct.base_dist[code] = (dist << 7) as i32;
            for _ in 0..(1 << (EXTRA_DBITS[code] - 7)) {
                ct.dist_code[256 + dist] = code as u8;
                dist += 1;
            }
            code += 1;
        }

        ct.bl_count = [0; MAX_BITS as usize + 1];
        let mut n = 0;
        while n <= 143 {
            ct.static_ltree[n].dl = 8;
            ct.bl_count[8] += 1;
            n += 1;
        }
        while n <= 255 {
            ct.static_ltree[n].dl = 9;
            ct.bl_count[9] += 1;
            n += 1;
        }
        while n <= 279 {
            ct.static_ltree[n].dl = 7;
            ct.bl_count[7] += 1;
            n += 1;
        }
        while n <= 287 {
            ct.static_ltree[n].dl = 8;
            ct.bl_count[8] += 1;
            n += 1;
        }
        let bl_count = ct.bl_count;
        Ct::gen_codes(&bl_count, &mut ct.static_ltree, L_CODES as i32 + 1);

        for n in 0..D_CODES {
            ct.static_dtree[n].dl = 5;
            ct.static_dtree[n].fc = bi_reverse(n as u32, 5) as u16;
        }

        ct.init_block();
    }

    /// `ct_tally (dist, lc)`: record a literal (`dist` 0, `lc` the byte) or
    /// a match (`dist` back, `lc` its length less [`MIN_MATCH`]), and say
    /// whether the block should end here.
    pub fn ct_tally(&mut self, dist: u32, lc: u32) -> bool {
        let level = self.level;
        let strstart = self.lm.strstart;
        let block_start = self.lm.block_start;
        let ct = &mut self.ct;
        self.inbuf[ct.last_lit as usize] = lc as u8;
        ct.last_lit += 1;
        if dist == 0 {
            ct.dyn_ltree[lc as usize].fc = ct.dyn_ltree[lc as usize].fc.wrapping_add(1);
        } else {
            let dist = dist - 1;
            let lcode = ct.length_code[lc as usize] as usize + LITERALS + 1;
            ct.dyn_ltree[lcode].fc = ct.dyn_ltree[lcode].fc.wrapping_add(1);
            let dcode = ct.d_code(dist);
            ct.dyn_dtree[dcode].fc = ct.dyn_dtree[dcode].fc.wrapping_add(1);
            self.d_buf[ct.last_dist as usize] = dist as u16;
            ct.last_dist += 1;
            ct.flags |= ct.flag_bit;
        }
        ct.flag_bit = ct.flag_bit.wrapping_shl(1);

        if ct.last_lit & 7 == 0 {
            ct.flag_buf[ct.last_flags as usize] = ct.flags;
            ct.last_flags += 1;
            ct.flags = 0;
            ct.flag_bit = 1;
        }
        if level > 2 && ct.last_lit & 0xfff == 0 {
            let mut out_length = u64::from(ct.last_lit) * 8;
            let in_length = u64::from(strstart).wrapping_sub(block_start as u64);
            for dcode in 0..D_CODES {
                out_length += u64::from(ct.dyn_dtree[dcode].fc) * (5 + EXTRA_DBITS[dcode] as u64);
            }
            out_length >>= 3;
            if ct.last_dist < ct.last_lit / 2 && out_length < in_length / 2 {
                return true;
            }
        }
        ct.last_lit == LIT_BUFSIZE - 1 || ct.last_dist == DIST_BUFSIZE
    }

    /// `compress_block (ltree, dtree)`, with the dynamic trees or the static
    /// ones.
    fn compress_block(&mut self, dynamic: bool) -> Res<()> {
        let mut lx = 0u32;
        let mut dx = 0u32;
        let mut fx = 0usize;
        let mut flag: u8 = 0;

        let code_of = |ct: &Ct, c: usize, lit: bool| -> (i32, i32) {
            let t = match (dynamic, lit) {
                (true, true) => ct.dyn_ltree[c],
                (true, false) => ct.dyn_dtree[c],
                (false, true) => ct.static_ltree[c],
                (false, false) => ct.static_dtree[c],
            };
            (i32::from(t.fc), i32::from(t.dl))
        };

        if self.ct.last_lit != 0 {
            loop {
                if lx & 7 == 0 {
                    flag = self.ct.flag_buf[fx];
                    fx += 1;
                }
                let mut lc = i32::from(self.inbuf[lx as usize]);
                lx += 1;
                if flag & 1 == 0 {
                    let (c, l) = code_of(&self.ct, lc as usize, true);
                    self.send_bits(c, l)?;
                } else {
                    let code = self.ct.length_code[lc as usize] as usize;
                    let (c, l) = code_of(&self.ct, code + LITERALS + 1, true);
                    self.send_bits(c, l)?;
                    let extra = EXTRA_LBITS[code];
                    if extra != 0 {
                        lc -= self.ct.base_length[code];
                        self.send_bits(lc, extra)?;
                    }
                    let mut dist = u32::from(self.d_buf[dx as usize]);
                    dx += 1;
                    let code = self.ct.d_code(dist);
                    let (c, l) = code_of(&self.ct, code, false);
                    self.send_bits(c, l)?;
                    let extra = EXTRA_DBITS[code];
                    if extra != 0 {
                        dist = dist.wrapping_sub(self.ct.base_dist[code] as u32);
                        self.send_bits(dist as i32, extra)?;
                    }
                }
                flag >>= 1;
                if lx >= self.ct.last_lit {
                    break;
                }
            }
        }
        let (c, l) = code_of(&self.ct, END_BLOCK, true);
        self.send_bits(c, l)
    }

    /// `send_all_trees (lcodes, dcodes, blcodes)`.
    fn send_all_trees(&mut self, lcodes: i32, dcodes: i32, blcodes: i32) -> Res<()> {
        self.send_bits(lcodes - 257, 5)?;
        self.send_bits(dcodes - 1, 5)?;
        self.send_bits(blcodes - 4, 4)?;
        for rank in 0..blcodes as usize {
            let len = i32::from(self.ct.bl_tree[BL_ORDER[rank]].dl);
            self.send_bits(len, 3)?;
        }
        self.send_tree(Which::L, lcodes - 1)?;
        self.send_tree(Which::D, dcodes - 1)
    }

    /// `send_code (c, bl_tree)`.
    fn send_bl_code(&mut self, c: usize) -> Res<()> {
        let t = self.ct.bl_tree[c];
        self.send_bits(i32::from(t.fc), i32::from(t.dl))
    }

    /// `send_tree (tree, max_code)`.
    fn send_tree(&mut self, w: Which, max_code: i32) -> Res<()> {
        let mut prevlen: i32 = -1;
        let mut nextlen = i32::from(self.ct.tree(w)[0].dl);
        let mut count = 0;
        let mut max_count = 7;
        let mut min_count = 4;

        if nextlen == 0 {
            max_count = 138;
            min_count = 3;
        }

        for n in 0..=max_code as usize {
            let curlen = nextlen;
            nextlen = i32::from(self.ct.tree(w)[n + 1].dl);
            count += 1;
            if count < max_count && curlen == nextlen {
                continue;
            }
            if count < min_count {
                loop {
                    self.send_bl_code(curlen as usize)?;
                    count -= 1;
                    if count == 0 {
                        break;
                    }
                }
            } else if curlen != 0 {
                if curlen != prevlen {
                    self.send_bl_code(curlen as usize)?;
                    count -= 1;
                }
                self.send_bl_code(REP_3_6)?;
                self.send_bits(count - 3, 2)?;
            } else if count <= 10 {
                self.send_bl_code(REPZ_3_10)?;
                self.send_bits(count - 3, 3)?;
            } else {
                self.send_bl_code(REPZ_11_138)?;
                self.send_bits(count - 11, 7)?;
            }
            count = 0;
            prevlen = curlen;
            if nextlen == 0 {
                max_count = 138;
                min_count = 3;
            } else if curlen == nextlen {
                max_count = 6;
                min_count = 3;
            } else {
                max_count = 7;
                min_count = 4;
            }
        }
        Ok(())
    }

    /// `flush_block (buf, stored_len, pad, eof)`: write the block of
    /// `stored_len` input bytes, which start at `window[buf]` if they are
    /// still there; pad to a byte when `pad`; end the stream when `eof`.
    /// The compressed length so far, in bytes.
    ///
    /// # Errors
    ///
    /// As [`Gz::put_byte`].
    pub fn flush_block(
        &mut self,
        buf: Option<usize>,
        stored_len: u64,
        pad: bool,
        eof: bool,
    ) -> Res<i64> {
        let eof_bit = i32::from(eof);
        self.ct.flag_buf[self.ct.last_flags as usize] = self.ct.flags;

        if self.ct.file_type == UNKNOWN {
            self.ct.set_file_type();
        }

        self.ct.build_tree(Which::L);
        self.ct.build_tree(Which::D);
        let max_blindex = self.ct.build_bl_tree();

        let mut opt_lenb = self.ct.opt_len.wrapping_add(3 + 7) >> 3;
        let static_lenb = self.ct.static_len.wrapping_add(3 + 7) >> 3;
        self.ct.input_len = self.ct.input_len.wrapping_add(stored_len as i64);

        if static_lenb <= opt_lenb {
            opt_lenb = static_lenb;
        }

        // `seekable ()` is 0, so the first of upstream's four cases -- the
        // whole file stored -- never applies.
        if stored_len.wrapping_add(4) <= opt_lenb && buf.is_some() {
            self.send_bits((STORED_BLOCK << 1) + eof_bit, 3)?;
            self.ct.compressed_len = (self.ct.compressed_len + 3 + 7) & !7;
            self.ct.compressed_len += ((stored_len + 4) << 3) as i64;
            self.copy_block(buf.unwrap_or(0), stored_len as u32, true)?;
        } else if static_lenb == opt_lenb {
            self.send_bits((STATIC_TREES << 1) + eof_bit, 3)?;
            self.compress_block(false)?;
            self.ct.compressed_len = self
                .ct
                .compressed_len
                .wrapping_add(3 + self.ct.static_len as i64);
        } else {
            self.send_bits((DYN_TREES << 1) + eof_bit, 3)?;
            let (l, d) = (self.ct.max_code[0], self.ct.max_code[1]);
            self.send_all_trees(l + 1, d + 1, max_blindex + 1)?;
            self.compress_block(true)?;
            self.ct.compressed_len = self
                .ct
                .compressed_len
                .wrapping_add(3 + self.ct.opt_len as i64);
        }
        self.ct.init_block();

        if eof {
            self.bi_windup()?;
            self.ct.compressed_len += 7;
        } else if pad && self.ct.compressed_len % 8 != 0 {
            self.send_bits((STORED_BLOCK << 1) + eof_bit, 3)?;
            self.ct.compressed_len = (self.ct.compressed_len + 3 + 7) & !7;
            self.copy_block(buf.unwrap_or(0), 0, true)?;
        }

        Ok(self.ct.compressed_len >> 3)
    }
}
