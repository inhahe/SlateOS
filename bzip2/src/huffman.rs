//! Huffman coding: libbzip2's `huffman.c`.
//!
//! Three routines, each kept to the reference's exact arithmetic, because the
//! code lengths the compressor chooses are written into the stream: a
//! different but equally good tree would be a different stream.

/// `BZ_MAX_ALPHA_SIZE`: the 256 byte values' move-to-front positions less
/// position zero, plus `RUNA`, `RUNB` and end-of-block.
pub(crate) const MAX_ALPHA_SIZE: usize = 258;

/// `BZ_MAX_CODE_LEN`: the decode tables' size, room for code lengths to 20
/// and the `length + 1` the base computation indexes with.
pub(crate) const MAX_CODE_LEN: usize = 23;

/// `heap` and `parent`/`weight` sizes in `BZ2_hbMakeCodeLengths`.
const HEAP_SIZE: usize = MAX_ALPHA_SIZE + 2;
const NODE_SIZE: usize = MAX_ALPHA_SIZE * 2;

fn at(v: &[i32], i: i32) -> i32 {
    usize::try_from(i)
        .ok()
        .and_then(|i| v.get(i))
        .copied()
        .unwrap_or(0)
}

fn put(v: &mut [i32], i: i32, x: i32) {
    if let Some(slot) = usize::try_from(i).ok().and_then(|i| v.get_mut(i)) {
        *slot = x;
    }
}

/// `ADDWEIGHTS`: a node's weight is its frequency shifted left 8, with the
/// subtree's depth in the low byte, so that of two equal frequencies the
/// shallower subtree sorts first and the tree stays as flat as it can.
const fn add_weights(w1: i32, w2: i32) -> i32 {
    let weights = (w1 & !0xff).wrapping_add(w2 & !0xff);
    let d1 = w1 & 0xff;
    let d2 = w2 & 0xff;
    let depth = if d1 > d2 { d1 } else { d2 };
    weights | depth.wrapping_add(1)
}

/// `UPHEAP`. Entry 0 of `heap` holds node 0, whose weight 0 no real node is
/// below, so the climb stops at the root without a bounds test.
fn upheap(heap: &mut [i32], weight: &[i32], z: i32) {
    let mut zz = z;
    let tmp = at(heap, zz);
    while at(weight, tmp) < at(weight, at(heap, zz >> 1)) {
        put(heap, zz, at(heap, zz >> 1));
        zz >>= 1;
    }
    put(heap, zz, tmp);
}

/// `DOWNHEAP`.
fn downheap(heap: &mut [i32], weight: &[i32], n_heap: i32, z: i32) {
    let mut zz = z;
    let tmp = at(heap, zz);
    loop {
        let mut yy = zz << 1;
        if yy > n_heap {
            break;
        }
        // `yy` is at most `n_heap`, below `HEAP_SIZE`: the step cannot wrap.
        if yy < n_heap && at(weight, at(heap, yy.wrapping_add(1))) < at(weight, at(heap, yy)) {
            yy = yy.wrapping_add(1);
        }
        if at(weight, tmp) < at(weight, at(heap, yy)) {
            break;
        }
        put(heap, zz, at(heap, yy));
        zz = yy;
    }
    put(heap, zz, tmp);
}

/// `BZ2_hbMakeCodeLengths`: the code length of each of the first
/// `alpha_size` symbols, from their frequencies, none longer than `max_len`.
///
/// A plain Huffman tree, built with a heap; when it comes out deeper than
/// `max_len`, every frequency is halved (plus one) and the tree rebuilt,
/// which flattens it -- libbzip2's way of limiting lengths, chosen over an
/// optimal length-limited algorithm, and kept because the lengths are part of
/// the output.
// Every count here is at most the block's symbol count (under 2^20) shifted
// left 8, and every index below `NODE_SIZE`: no `i32` arithmetic can overflow.
#[allow(clippy::arithmetic_side_effects)]
pub(crate) fn make_code_lengths(len: &mut [u8], freq: &[i32], alpha_size: usize, max_len: i32) {
    let mut heap = [0i32; HEAP_SIZE];
    let mut weight = [0i32; NODE_SIZE];
    let mut parent = [0i32; NODE_SIZE];
    let alpha = i32::try_from(alpha_size).unwrap_or(0);

    for i in 0..alpha {
        let f = at(freq, i);
        put(&mut weight, i + 1, (if f == 0 { 1 } else { f }) << 8);
    }

    loop {
        let mut n_nodes = alpha;
        let mut n_heap = 0i32;

        put(&mut heap, 0, 0);
        put(&mut weight, 0, 0);
        put(&mut parent, 0, -2);

        for i in 1..=alpha {
            put(&mut parent, i, -1);
            n_heap += 1;
            put(&mut heap, n_heap, i);
            upheap(&mut heap, &weight, n_heap);
        }

        while n_heap > 1 {
            let n1 = at(&heap, 1);
            let last = at(&heap, n_heap);
            put(&mut heap, 1, last);
            n_heap -= 1;
            downheap(&mut heap, &weight, n_heap, 1);
            let n2 = at(&heap, 1);
            let last = at(&heap, n_heap);
            put(&mut heap, 1, last);
            n_heap -= 1;
            downheap(&mut heap, &weight, n_heap, 1);
            n_nodes += 1;
            put(&mut parent, n1, n_nodes);
            put(&mut parent, n2, n_nodes);
            let joined = add_weights(at(&weight, n1), at(&weight, n2));
            put(&mut weight, n_nodes, joined);
            put(&mut parent, n_nodes, -1);
            n_heap += 1;
            put(&mut heap, n_heap, n_nodes);
            upheap(&mut heap, &weight, n_heap);
        }

        let mut too_long = false;
        for (i, slot) in (1..=alpha).zip(len.iter_mut()) {
            let mut j = 0i32;
            let mut k = i;
            // Each step climbs to a parent with a higher node number, and the
            // root's parent is -1: the walk ends within `n_nodes` steps.
            while at(&parent, k) >= 0 {
                k = at(&parent, k);
                j += 1;
            }
            *slot = u8::try_from(j).unwrap_or(u8::MAX);
            if j > max_len {
                too_long = true;
            }
        }

        if !too_long {
            break;
        }

        for i in 1..=alpha {
            let j = at(&weight, i) >> 8;
            let j = 1 + (j / 2);
            put(&mut weight, i, j << 8);
        }
    }
}

/// `BZ2_hbAssignCodes`: canonical codes -- by length, then by symbol -- for
/// lengths already known to lie between `min_len` and `max_len`.
pub(crate) fn assign_codes(code: &mut [i32], length: &[u8], min_len: u8, max_len: u8) {
    let mut vec = 0i32;
    for n in min_len..=max_len {
        for (slot, &l) in code.iter_mut().zip(length) {
            if l == n {
                *slot = vec;
                vec = vec.wrapping_add(1);
            }
        }
        vec = vec.wrapping_shl(1);
    }
}

/// One coding table as `BZ2_decompress` decodes with it: the output of
/// `BZ2_hbCreateDecodeTables`, and the table's shortest code length.
pub(crate) struct DecodeTable {
    /// `limit[n]`: the largest code of length `n`, as an integer of `n` bits.
    pub(crate) limit: [i32; MAX_CODE_LEN],
    /// `base[n]`: what to subtract from a code of length `n` to index `perm`.
    pub(crate) base: [i32; MAX_CODE_LEN],
    /// The symbols, in canonical code order.
    pub(crate) perm: [u16; MAX_ALPHA_SIZE],
    /// The shortest code length, where decoding starts.
    pub(crate) min_len: u32,
}

impl DecodeTable {
    /// `BZ2_hbCreateDecodeTables`, with `minLen` and `maxLen` found as
    /// `BZ2_decompress` finds them. Every length must already have been
    /// checked to lie from 1 to 20, as the decoder does as it reads them.
    pub(crate) fn new(length: &[u8]) -> Self {
        let min_len = length.iter().copied().min().unwrap_or(1);
        let max_len = length.iter().copied().max().unwrap_or(0);

        let mut perm = [0u16; MAX_ALPHA_SIZE];
        let mut pp = 0usize;
        for i in min_len..=max_len {
            for (j, &l) in (0u16..).zip(length) {
                if l == i {
                    if let Some(slot) = perm.get_mut(pp) {
                        *slot = j;
                    }
                    pp = pp.wrapping_add(1);
                }
            }
        }

        let mut base = [0i32; MAX_CODE_LEN];
        for &l in length {
            if let Some(slot) = base.get_mut(usize::from(l).wrapping_add(1)) {
                *slot = slot.wrapping_add(1);
            }
        }
        for i in 1..MAX_CODE_LEN {
            let prev = base.get(i.wrapping_sub(1)).copied().unwrap_or(0);
            if let Some(slot) = base.get_mut(i) {
                *slot = slot.wrapping_add(prev);
            }
        }

        // With lengths of 1 to 20 and at most 258 symbols, `vec` stays below
        // 258 << 20: the wrapping operations below never wrap.
        let mut limit = [0i32; MAX_CODE_LEN];
        let mut vec = 0i32;
        for i in usize::from(min_len)..=usize::from(max_len) {
            let here = base.get(i).copied().unwrap_or(0);
            let next = base.get(i.wrapping_add(1)).copied().unwrap_or(0);
            vec = vec.wrapping_add(next.wrapping_sub(here));
            if let Some(slot) = limit.get_mut(i) {
                *slot = vec.wrapping_sub(1);
            }
            vec = vec.wrapping_shl(1);
        }
        for i in usize::from(min_len).wrapping_add(1)..=usize::from(max_len) {
            let below = limit.get(i.wrapping_sub(1)).copied().unwrap_or(0);
            if let Some(slot) = base.get_mut(i) {
                *slot = below.wrapping_add(1).wrapping_shl(1).wrapping_sub(*slot);
            }
        }

        Self {
            limit,
            base,
            perm,
            min_len: u32::from(min_len),
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

    /// Kraft's sum of a set of lengths, scaled so that a complete code is 1 << 20.
    fn kraft(len: &[u8]) -> u64 {
        len.iter().map(|&l| 1u64 << (20 - u32::from(l))).sum()
    }

    #[test]
    fn lengths_form_a_complete_code() {
        let freq = [10, 1, 1, 5, 0, 7, 3, 3, 100, 2];
        let mut len = [0u8; 10];
        make_code_lengths(&mut len, &freq, 10, 17);
        assert_eq!(kraft(&len), 1 << 20, "{len:?}");
        // The most frequent symbol has the shortest code.
        let shortest = *len.iter().min().unwrap();
        assert_eq!(len[8], shortest);
    }

    #[test]
    fn a_tree_too_deep_is_flattened_to_the_limit() {
        // Fibonacci frequencies make the deepest possible Huffman tree.
        let mut freq = [0i32; 30];
        let (mut a, mut b) = (1i32, 1i32);
        for f in &mut freq {
            *f = a;
            (a, b) = (b, a + b);
        }
        let mut len = [0u8; 30];
        make_code_lengths(&mut len, &freq, 30, 17);
        assert!(len.iter().all(|&l| (1..=17).contains(&l)), "{len:?}");
        assert!(kraft(&len) <= 1 << 20);
        let mut unlimited = [0u8; 30];
        make_code_lengths(&mut unlimited, &freq, 30, 32);
        assert!(
            unlimited.iter().any(|&l| l > 17),
            "the test needs a deep tree"
        );
    }

    #[test]
    fn codes_are_canonical() {
        let len = [2u8, 1, 3, 3];
        let mut code = [0i32; 4];
        assign_codes(&mut code, &len, 1, 3);
        assert_eq!(code, [0b10, 0b0, 0b110, 0b111]);
    }

    #[test]
    fn the_decode_table_inverts_the_codes() {
        let len = [2u8, 1, 3, 3];
        let t = DecodeTable::new(&len);
        assert_eq!(t.min_len, 1);
        // Symbols in code order: 1 (len 1), 0 (len 2), 2 and 3 (len 3).
        assert_eq!(&t.perm[..4], &[1, 0, 2, 3]);
        assert_eq!(t.limit[1], 0);
        assert_eq!(t.limit[2], 0b10);
        assert_eq!(t.limit[3], 0b111);
        // perm index = code - base[length].
        assert_eq!(0b10 - t.base[2], 1);
        assert_eq!(0b110 - t.base[3], 2);
    }
}
