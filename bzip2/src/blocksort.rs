//! Block sorting: libbzip2's `blocksort.c`.
//!
//! The Burrows-Wheeler transform needs the block's rotations in sorted order.
//! libbzip2 has two sorts and this is both of them, unchanged:
//!
//! - **`mainSort`**: a two-byte radix sort into 65 536 buckets, then a
//!   three-way string quicksort (Bentley and Sedgewick) and a Shell sort of
//!   the small buckets, with a trick that fills most buckets by scanning a
//!   finished one instead of sorting them, and a `quadrant` array that
//!   caches finished orderings so later comparisons stop early. Fast on
//!   ordinary data -- and quadratic on repetitive data, which is why it runs
//!   on a **budget**: every eight bytes compared cost one unit, and when the
//!   block has spent nine units per byte (the default work factor of 30) it
//!   gives up.
//! - **`fallbackSort`**: a doubling sort after Manber and Myers, sure to
//!   finish in O(n log n) whatever the data. Blocks under 10 000 bytes go
//!   straight to it, and so does any block `mainSort` gives up on.
//!
//! Ported exactly, budget and all, rather than replaced by a suffix-array
//! construction, because the two disagree where a block repeats itself:
//! rotations that are equal can come out in either order, and which one ends
//! up where decides the start pointer written into the stream. A different
//! correct sort would write a valid stream -- and not libbzip2's.
//!
//! **Indices.** The C works in `Int32`, and so does this: positions are
//! below the block length (at most 900 014 bytes) plus the 34-byte overshoot,
//! and the counts below the block length, so no `i32` here can overflow,
//! which is why the sort's functions allow `arithmetic_side_effects`. Reads
//! and writes go through `ld`/`st`, which turn an index that cannot be out of
//! range into a harmless no-op if it ever were, rather than a panic.

use alloc::vec;
use alloc::vec::Vec;

/// `BZ_N_RADIX`: the bytes the radix sort has already compared.
const N_RADIX: i32 = 2;
/// `BZ_N_QSORT`: the depth after which the quicksort hands over.
const N_QSORT: i32 = 12;
/// `BZ_N_SHELL`
const N_SHELL: i32 = 18;
/// `BZ_N_OVERSHOOT`: bytes of the block repeated after its end, so that a
/// comparison can run past the end without wrapping at every step.
pub(crate) const N_OVERSHOOT: usize = 34;
const _: () = assert!(N_OVERSHOOT == (N_RADIX + N_QSORT + N_SHELL + 2) as usize);

/// libbzip2's default work factor, what `BZ2_bzCompressInit` takes 0 to mean.
const WORK_FACTOR: i32 = 30;

/// Blocks shorter than this go straight to the fallback sort.
const FALLBACK_BELOW: usize = 10_000;

/// `FALLBACK_QSORT_SMALL_THRESH`
const FALLBACK_QSORT_SMALL_THRESH: i32 = 10;
/// `MAIN_QSORT_SMALL_THRESH`
const MAIN_QSORT_SMALL_THRESH: i32 = 20;
/// `MAIN_QSORT_DEPTH_THRESH`
const MAIN_QSORT_DEPTH_THRESH: i32 = N_RADIX + N_QSORT;

/// `SETMASK`: marks an `ftab` bucket as sorted. Bucket boundaries are below
/// 2^20, so bit 21 is free.
const SETMASK: u32 = 1 << 21;
const CLEARMASK: u32 = !SETMASK;

/// The Shell sort's increments (Knuth's, which libbzip2's comment says beat
/// Incerpi-Sedgewick's here).
const INCS: [i32; 14] = [
    1, 4, 13, 40, 121, 364, 1093, 3280, 9841, 29524, 88573, 265_720, 797_161, 2_391_484,
];

/// An `i32` index as a `usize`; a negative one becomes one no slice has.
fn ix(i: i32) -> usize {
    usize::try_from(i).unwrap_or(usize::MAX)
}

/// A `u32` index as a `usize`.
fn ux(i: u32) -> usize {
    usize::try_from(i).unwrap_or(usize::MAX)
}

/// `v[i]`, or the type's zero for an index past the end.
fn ld<T: Copy + Default>(v: &[T], i: usize) -> T {
    v.get(i).copied().unwrap_or_default()
}

/// `v[i] = x`, or nothing for an index past the end.
fn st<T>(v: &mut [T], i: usize, x: T) {
    if let Some(slot) = v.get_mut(i) {
        *slot = x;
    }
}

fn swap(v: &mut [u32], a: i32, b: i32) {
    let (a, b) = (ix(a), ix(b));
    if a < v.len() && b < v.len() {
        v.swap(a, b);
    }
}

/// `fvswap`/`mvswap`: swaps the `n` entries from `p1` with the `n` from `p2`.
// Both runs lie inside the partition being sorted.
#[allow(clippy::arithmetic_side_effects)]
fn vswap(v: &mut [u32], p1: i32, p2: i32, n: i32) {
    for k in 0..n {
        swap(v, p1 + k, p2 + k);
    }
}

/// A `u32` (a position below 2^20) as the `i32` the C computes with.
fn signed(v: u32) -> i32 {
    i32::try_from(v).unwrap_or(i32::MAX)
}

/// A position (below 2^20) as the `u32` the pointer array holds.
fn unsigned(v: i32) -> u32 {
    u32::try_from(v).unwrap_or(0)
}

// ---------------------------------------------------------------------------
// The fallback sort
// ---------------------------------------------------------------------------

/// `fallbackSimpleSort`: insertion sort of `fmap[lo..=hi]` by `eclass`, with
/// a stride-4 pass first.
#[allow(clippy::arithmetic_side_effects)]
fn fallback_simple_sort(fmap: &mut [u32], eclass: &[u32], lo: i32, hi: i32) {
    if lo == hi {
        return;
    }

    if hi - lo > 3 {
        let mut i = hi - 4;
        while i >= lo {
            let tmp = ld(fmap, ix(i));
            let ec_tmp = ld(eclass, ux(tmp));
            let mut j = i + 4;
            while j <= hi && ec_tmp > ld(eclass, ux(ld(fmap, ix(j)))) {
                st(fmap, ix(j - 4), ld(fmap, ix(j)));
                j += 4;
            }
            st(fmap, ix(j - 4), tmp);
            i -= 1;
        }
    }

    let mut i = hi - 1;
    while i >= lo {
        let tmp = ld(fmap, ix(i));
        let ec_tmp = ld(eclass, ux(tmp));
        let mut j = i + 1;
        while j <= hi && ec_tmp > ld(eclass, ux(ld(fmap, ix(j)))) {
            st(fmap, ix(j - 1), ld(fmap, ix(j)));
            j += 1;
        }
        st(fmap, ix(j - 1), tmp);
        i -= 1;
    }
}

/// `fallbackQSort3`: three-way quicksort of `fmap[lo_st..=hi_st]` by
/// `eclass`, with libbzip2's pseudo-random choice of pivot.
///
/// libbzip2 keeps its pending partitions on a 100-entry stack and asserts it
/// never fills; a `Vec` cannot fill, and is otherwise the same stack.
#[allow(clippy::arithmetic_side_effects)]
fn fallback_qsort3(fmap: &mut [u32], eclass: &[u32], lo_st: i32, hi_st: i32) {
    let mut r: u32 = 0;
    let mut stack: Vec<(i32, i32)> = vec![(lo_st, hi_st)];

    while let Some((lo, hi)) = stack.pop() {
        if hi - lo < FALLBACK_QSORT_SMALL_THRESH {
            fallback_simple_sort(fmap, eclass, lo, hi);
            continue;
        }

        // Random partitioning, after Sedgewick's chapter 35: median of three
        // was seen to fail on some inputs.
        r = (r * 7621 + 1) % 32768;
        let pick = match r % 3 {
            0 => lo,
            1 => (lo + hi) >> 1,
            _ => hi,
        };
        let med = ld(eclass, ux(ld(fmap, ix(pick))));

        let (mut un_lo, mut lt_lo) = (lo, lo);
        let (mut un_hi, mut gt_hi) = (hi, hi);

        loop {
            while un_lo <= un_hi {
                let e = ld(eclass, ux(ld(fmap, ix(un_lo))));
                if e == med {
                    swap(fmap, un_lo, lt_lo);
                    lt_lo += 1;
                    un_lo += 1;
                    continue;
                }
                if e > med {
                    break;
                }
                un_lo += 1;
            }
            while un_lo <= un_hi {
                let e = ld(eclass, ux(ld(fmap, ix(un_hi))));
                if e == med {
                    swap(fmap, un_hi, gt_hi);
                    gt_hi -= 1;
                    un_hi -= 1;
                    continue;
                }
                if e < med {
                    break;
                }
                un_hi -= 1;
            }
            if un_lo > un_hi {
                break;
            }
            swap(fmap, un_lo, un_hi);
            un_lo += 1;
            un_hi -= 1;
        }

        if gt_hi < lt_lo {
            continue;
        }

        let n = (lt_lo - lo).min(un_lo - lt_lo);
        vswap(fmap, lo, un_lo - n, n);
        let m = (hi - gt_hi).min(gt_hi - un_hi);
        vswap(fmap, un_lo, hi - m + 1, m);

        let n = lo + un_lo - lt_lo - 1;
        let m = hi - (gt_hi - un_hi) + 1;

        if n - lo > hi - m {
            stack.push((lo, n));
            stack.push((m, hi));
        } else {
            stack.push((m, hi));
            stack.push((lo, n));
        }
    }
}

/// The bucket-header bitmap of `fallbackSort` (`SET_BH` and its friends): a
/// set bit marks the first rotation of a bucket.
struct Bh(Vec<u32>);

impl Bh {
    fn set(&mut self, z: i32) {
        let z = ix(z);
        if let Some(w) = self.0.get_mut(z >> 5) {
            *w |= 1u32 << (z & 31);
        }
    }

    fn clear(&mut self, z: i32) {
        let z = ix(z);
        if let Some(w) = self.0.get_mut(z >> 5) {
            *w &= !(1u32 << (z & 31));
        }
    }

    fn is_set(&self, z: i32) -> bool {
        let z = ix(z);
        self.0
            .get(z >> 5)
            .is_some_and(|w| w & (1u32 << (z & 31)) != 0)
    }

    fn word(&self, z: i32) -> Option<u32> {
        self.0.get(ix(z) >> 5).copied()
    }
}

/// `UNALIGNED_BH`: whether `z` is not the first bit of a word.
fn unaligned(z: i32) -> bool {
    z & 0x1f != 0
}

/// `fallbackSort`: sorts the rotations of `block` into `fmap` by repeated
/// doubling of the compared length, each round refining the buckets the last
/// left unresolved.
///
/// libbzip2 keeps the block in the same storage as the equivalence classes
/// and rebuilds it afterwards; here the block is its own slice and needs no
/// rebuilding.
// Positions are below the block length (under 2^20), `h` below twice it.
#[allow(clippy::arithmetic_side_effects, clippy::too_many_lines)]
fn fallback_sort(fmap: &mut [u32], block: &[u8]) {
    let n = block.len();
    let nblock = i32::try_from(n).unwrap_or(0);
    let mut eclass = vec![0u32; n];

    // libbzip2 clears `2 + nblock / 32` words and sets sentinel bits up to
    // `nblock + 63`, which can be one word further; two spare words cover it.
    let mut bh = Bh(vec![0u32; 2 + n / 32 + 2]);

    // Initial one-byte radix sort, which gives the first buckets.
    let mut ftab = [0i32; 257];
    for &b in block {
        let count = ld(&ftab, usize::from(b)) + 1;
        st(&mut ftab, usize::from(b), count);
    }
    for i in 1..257 {
        let sum = ld(&ftab, i) + ld(&ftab, i - 1);
        st(&mut ftab, i, sum);
    }
    for (i, &b) in (0u32..).zip(block) {
        let k = ld(&ftab, usize::from(b)) - 1;
        st(&mut ftab, usize::from(b), k);
        st(fmap, ix(k), i);
    }
    for i in 0..256 {
        bh.set(ld(&ftab, i));
    }

    // Sentinel bits after the block, alternating, so that the bucket scans
    // below always stop.
    for i in 0..32 {
        bh.set(nblock + 2 * i);
        bh.clear(nblock + 2 * i + 1);
    }

    // The scans can never pass `nblock + 1` (bit `nblock` is set, the next
    // clear); `end` bounds them anyway, so that a broken invariant ends the
    // sort instead of looping.
    let end = nblock + 64;

    let mut h = 1i32;
    loop {
        let mut j = 0i32;
        for i in 0..nblock {
            if bh.is_set(i) {
                j = i;
            }
            let mut k = signed(ld(fmap, ix(i))) - h;
            if k < 0 {
                k += nblock;
            }
            st(&mut eclass, ix(k), unsigned(j));
        }

        let mut n_not_done = 0i32;
        let mut r = -1i32;
        loop {
            // Find the next non-singleton bucket.
            let mut k = r + 1;
            while k < end && bh.is_set(k) && unaligned(k) {
                k += 1;
            }
            if bh.is_set(k) {
                while k < end && bh.word(k) == Some(0xffff_ffff) {
                    k += 32;
                }
                while k < end && bh.is_set(k) {
                    k += 1;
                }
            }
            let l = k - 1;
            if l >= nblock {
                break;
            }
            while k < end && !bh.is_set(k) && unaligned(k) {
                k += 1;
            }
            if !bh.is_set(k) {
                while k < end && bh.word(k) == Some(0) {
                    k += 32;
                }
                while k < end && !bh.is_set(k) {
                    k += 1;
                }
            }
            r = k - 1;
            if r >= nblock {
                break;
            }

            // [l, r] brackets the bucket.
            if r > l {
                n_not_done += r - l + 1;
                fallback_qsort3(fmap, &eclass, l, r);

                // Mark where the class changes: those are the new buckets.
                let mut cc: Option<u32> = None;
                for i in l..=r {
                    let cc1 = ld(&eclass, ux(ld(fmap, ix(i))));
                    if cc != Some(cc1) {
                        bh.set(i);
                        cc = Some(cc1);
                    }
                }
            }
        }

        h *= 2;
        if h > nblock || n_not_done == 0 {
            break;
        }
    }
}

// ---------------------------------------------------------------------------
// The main sort
// ---------------------------------------------------------------------------

/// `mmed3`: the median of three bytes.
fn mmed3(a: u8, b: u8, c: u8) -> u8 {
    let (a, mut b) = if a > b { (b, a) } else { (a, b) };
    if b > c {
        b = c;
        if a > b {
            b = a;
        }
    }
    b
}

/// The state `mainSort` and its helpers share: libbzip2 passes it as six
/// arguments to each.
struct Main<'a> {
    /// The rotations, being sorted: `ptr[i]` is where rotation `i` starts.
    ptr: &'a mut [u32],
    /// The block, followed by its first `N_OVERSHOOT` bytes again.
    block: &'a [u8],
    /// Cached sort ranks, the same length as `block`.
    quadrant: &'a mut [u16],
    nblock: i32,
    /// Comparison work left; below zero, the sort gives up.
    budget: i32,
}

impl Main<'_> {
    /// `mainGtU`: whether the rotation at `i1` sorts after the one at `i2`,
    /// comparing at most `nblock + 8` bytes and spending a unit of budget per
    /// eight.
    ///
    /// Each comparison is a run of bytes from the block (and, past the first
    /// twelve, from `quadrant`), so it is done as slices: one bounds check a
    /// run instead of one a byte. The runs never reach past the overshoot --
    /// `i1` starts below `nblock + 15`, and is wrapped back below `nblock`
    /// after each run of eight -- so `get` cannot miss; if it did, the answer
    /// "not greater" would cost the sort order, not memory safety.
    #[allow(clippy::arithmetic_side_effects)]
    fn gt_u(&mut self, i1: u32, i2: u32) -> bool {
        let block = self.block;
        let quadrant = &*self.quadrant;
        let n = ix(self.nblock);
        let (mut i1, mut i2) = (ux(i1), ux(i2));

        let (Some(a), Some(b)) = (block.get(i1..i1 + 12), block.get(i2..i2 + 12)) else {
            return false;
        };
        for (c1, c2) in a.iter().zip(b) {
            if c1 != c2 {
                return c1 > c2;
            }
        }
        i1 += 12;
        i2 += 12;

        let mut k = self.nblock + 8;
        loop {
            let (Some(b1), Some(b2), Some(q1), Some(q2)) = (
                block.get(i1..i1 + 8),
                block.get(i2..i2 + 8),
                quadrant.get(i1..i1 + 8),
                quadrant.get(i2..i2 + 8),
            ) else {
                return false;
            };
            for (((c1, c2), s1), s2) in b1.iter().zip(b2).zip(q1).zip(q2) {
                if c1 != c2 {
                    return c1 > c2;
                }
                if s1 != s2 {
                    return s1 > s2;
                }
            }
            i1 += 8;
            i2 += 8;
            if i1 >= n {
                i1 -= n;
            }
            if i2 >= n {
                i2 -= n;
            }
            k -= 8;
            self.budget -= 1;
            if k < 0 {
                break;
            }
        }
        false
    }

    /// `mainSimpleSort`: Shell sort of `ptr[lo..=hi]` comparing from byte
    /// `d` on. The budget is looked at after every third insertion, where
    /// libbzip2 looks at it -- which decides whether a block falls back.
    #[allow(clippy::arithmetic_side_effects)]
    fn simple_sort(&mut self, lo: i32, hi: i32, d: i32) {
        let big_n = hi - lo + 1;
        if big_n < 2 {
            return;
        }

        let mut hp = 0usize;
        while hp < INCS.len() && ld(&INCS, hp) < big_n {
            hp += 1;
        }

        // `hp--; for (; hp >= 0; hp--)`
        while hp > 0 {
            hp -= 1;
            let h = ld(&INCS, hp);
            let mut i = lo + h;
            'rounds: loop {
                for _ in 0..3 {
                    if i > hi {
                        break 'rounds;
                    }
                    let v = ld(self.ptr, ix(i));
                    let mut j = i;
                    while self.gt_u(
                        ld(self.ptr, ix(j - h)).wrapping_add(unsigned(d)),
                        v.wrapping_add(unsigned(d)),
                    ) {
                        st(self.ptr, ix(j), ld(self.ptr, ix(j - h)));
                        j -= h;
                        if j < lo + h {
                            break;
                        }
                    }
                    st(self.ptr, ix(j), v);
                    i += 1;
                }
                if self.budget < 0 {
                    return;
                }
            }
        }
    }

    /// The byte `d` places into the rotation at `ptr[i]`.
    fn byte_at(&self, i: i32, d: i32) -> u8 {
        ld(
            self.block,
            ux(ld(self.ptr, ix(i)).wrapping_add(unsigned(d))),
        )
    }

    /// `mainQSort3`: three-way string quicksort of `ptr[lo_st..=hi_st]` from
    /// byte `d_st` on, handing small or deep partitions to the Shell sort.
    #[allow(clippy::arithmetic_side_effects)]
    fn qsort3(&mut self, lo_st: i32, hi_st: i32, d_st: i32) {
        let mut stack: Vec<(i32, i32, i32)> = vec![(lo_st, hi_st, d_st)];

        while let Some((lo, hi, d)) = stack.pop() {
            if hi - lo < MAIN_QSORT_SMALL_THRESH || d > MAIN_QSORT_DEPTH_THRESH {
                self.simple_sort(lo, hi, d);
                if self.budget < 0 {
                    return;
                }
                continue;
            }

            let med = i32::from(mmed3(
                self.byte_at(lo, d),
                self.byte_at(hi, d),
                self.byte_at((lo + hi) >> 1, d),
            ));

            let (mut un_lo, mut lt_lo) = (lo, lo);
            let (mut un_hi, mut gt_hi) = (hi, hi);

            loop {
                while un_lo <= un_hi {
                    let n = i32::from(self.byte_at(un_lo, d)) - med;
                    if n == 0 {
                        swap(self.ptr, un_lo, lt_lo);
                        lt_lo += 1;
                        un_lo += 1;
                        continue;
                    }
                    if n > 0 {
                        break;
                    }
                    un_lo += 1;
                }
                while un_lo <= un_hi {
                    let n = i32::from(self.byte_at(un_hi, d)) - med;
                    if n == 0 {
                        swap(self.ptr, un_hi, gt_hi);
                        gt_hi -= 1;
                        un_hi -= 1;
                        continue;
                    }
                    if n < 0 {
                        break;
                    }
                    un_hi -= 1;
                }
                if un_lo > un_hi {
                    break;
                }
                swap(self.ptr, un_lo, un_hi);
                un_lo += 1;
                un_hi -= 1;
            }

            if gt_hi < lt_lo {
                stack.push((lo, hi, d + 1));
                continue;
            }

            let n = (lt_lo - lo).min(un_lo - lt_lo);
            vswap(self.ptr, lo, un_lo - n, n);
            let m = (hi - gt_hi).min(gt_hi - un_hi);
            vswap(self.ptr, un_lo, hi - m + 1, m);

            let n = lo + un_lo - lt_lo - 1;
            let m = hi - (gt_hi - un_hi) + 1;

            let mut next = [(lo, n, d), (m, hi, d), (n + 1, m - 1, d + 1)];
            let size = |p: (i32, i32, i32)| p.1 - p.0;
            if size(next[0]) < size(next[1]) {
                next.swap(0, 1);
            }
            if size(next[1]) < size(next[2]) {
                next.swap(1, 2);
            }
            if size(next[0]) < size(next[1]) {
                next.swap(0, 1);
            }
            stack.extend(next);
        }
    }

    /// `mainSort`. Leaves `budget` below zero if it gave up, in which case
    /// `ptr` is only partly sorted and the caller sorts it again with the
    /// fallback.
    // A port keeps libbzip2's function boundaries, so that the two can be
    // read side by side; `mainSort` is one function there.
    #[allow(clippy::arithmetic_side_effects, clippy::too_many_lines)]
    fn sort(&mut self, ftab: &mut [u32]) {
        let n = ix(self.nblock);

        // The two-byte frequency table (the `quadrant` is already zero).
        ftab.fill(0);
        for i in 0..n {
            let j = usize::from(ld(self.block, i)) << 8 | usize::from(ld(self.block, i + 1));
            st(ftab, j, ld(ftab, j) + 1);
        }
        for i in 1..=65536 {
            st(ftab, i, ld(ftab, i) + ld(ftab, i - 1));
        }

        // The radix sort proper, from the last rotation back, as libbzip2
        // does: the order decides where equal two-byte prefixes land.
        for i in (0..n).rev() {
            let s = usize::from(ld(self.block, i)) << 8 | usize::from(ld(self.block, i + 1));
            let j = ld(ftab, s).wrapping_sub(1);
            st(ftab, s, j);
            st(self.ptr, ux(j), u32::try_from(i).unwrap_or(0));
        }

        // The big buckets (by first byte), smallest first.
        let big_freq = |ftab: &[u32], b: i32| {
            signed(ld(ftab, ix((b + 1) << 8))) - signed(ld(ftab, ix(b << 8)))
        };
        let mut running_order = [0i32; 256];
        for (slot, b) in running_order.iter_mut().zip(0..) {
            *slot = b;
        }
        {
            let mut h = 1i32;
            loop {
                h = 3 * h + 1;
                if h > 256 {
                    break;
                }
            }
            loop {
                h /= 3;
                for i in h..=255 {
                    let vv = ld(&running_order, ix(i));
                    let mut j = i;
                    while big_freq(ftab, ld(&running_order, ix(j - h))) > big_freq(ftab, vv) {
                        let moved = ld(&running_order, ix(j - h));
                        st(&mut running_order, ix(j), moved);
                        j -= h;
                        if j < h {
                            break;
                        }
                    }
                    st(&mut running_order, ix(j), vv);
                }
                if h == 1 {
                    break;
                }
            }
        }

        let mut big_done = [false; 256];
        let mut copy_start = [0i32; 256];
        let mut copy_end = [0i32; 256];

        for i in 0..=255i32 {
            let ss = ld(&running_order, ix(i));

            // Step 1: complete big bucket `ss` by quicksorting each of its
            // small buckets `[ss, j]` that an earlier step 2 did not fill.
            for j in 0..=255i32 {
                if j == ss {
                    continue;
                }
                let sb = ix((ss << 8) + j);
                if ld(ftab, sb) & SETMASK == 0 {
                    let lo = signed(ld(ftab, sb) & CLEARMASK);
                    let hi = signed(ld(ftab, sb + 1) & CLEARMASK) - 1;
                    if hi > lo {
                        self.qsort3(lo, hi, N_RADIX);
                        if self.budget < 0 {
                            return;
                        }
                    }
                }
                st(ftab, sb, ld(ftab, sb) | SETMASK);
            }

            // Step 2: scan big bucket `ss` to put every small bucket `[t, ss]`
            // in order without sorting it -- `[ss, ss]` included, which is
            // why the bounds are re-read as the loops extend them.
            for j in 0..256usize {
                let base = (j << 8) + ix(ss);
                st(&mut copy_start, j, signed(ld(ftab, base) & CLEARMASK));
                st(&mut copy_end, j, signed(ld(ftab, base + 1) & CLEARMASK) - 1);
            }
            let mut j = signed(ld(ftab, ix(ss << 8)) & CLEARMASK);
            while j < ld(&copy_start, ix(ss)) {
                let mut k = signed(ld(self.ptr, ix(j))) - 1;
                if k < 0 {
                    k += self.nblock;
                }
                let c1 = usize::from(ld(self.block, ix(k)));
                if !ld(&big_done, c1) {
                    let at = ld(&copy_start, c1);
                    st(self.ptr, ix(at), unsigned(k));
                    st(&mut copy_start, c1, at + 1);
                }
                j += 1;
            }
            let mut j = signed(ld(ftab, ix((ss + 1) << 8)) & CLEARMASK) - 1;
            while j > ld(&copy_end, ix(ss)) {
                let mut k = signed(ld(self.ptr, ix(j))) - 1;
                if k < 0 {
                    k += self.nblock;
                }
                let c1 = usize::from(ld(self.block, ix(k)));
                if !ld(&big_done, c1) {
                    let at = ld(&copy_end, c1);
                    st(self.ptr, ix(at), unsigned(k));
                    st(&mut copy_end, c1, at - 1);
                }
                j -= 1;
            }
            for j in 0..256usize {
                let at = (j << 8) + ix(ss);
                st(ftab, at, ld(ftab, at) | SETMASK);
            }

            // Step 3: big bucket `ss` is done. Record its order in `quadrant`
            // (overshoot included), scaled into 16 bits, so that comparisons
            // reaching it later stop there.
            st(&mut big_done, ix(ss), true);
            if i < 255 {
                let bb_start = signed(ld(ftab, ix(ss << 8)) & CLEARMASK);
                let bb_size = signed(ld(ftab, ix((ss + 1) << 8)) & CLEARMASK) - bb_start;
                let mut shifts = 0u32;
                while (bb_size >> shifts) > 65534 {
                    shifts += 1;
                }
                let mut j = bb_size - 1;
                while j >= 0 {
                    let a2update = ux(ld(self.ptr, ix(bb_start + j)));
                    let q_val = u16::try_from(j >> shifts).unwrap_or(u16::MAX);
                    st(self.quadrant, a2update, q_val);
                    if a2update < N_OVERSHOOT {
                        st(self.quadrant, a2update + n, q_val);
                    }
                    j -= 1;
                }
            }
        }
    }
}

/// Scratch the block sort needs, kept across blocks so that a stream of many
/// blocks allocates it once.
#[derive(Default)]
pub(crate) struct Sorter {
    /// The sorted rotations: `ptr[i]` is where the `i`th smallest starts.
    pub(crate) ptr: Vec<u32>,
    ftab: Vec<u32>,
    quadrant: Vec<u16>,
}

impl Sorter {
    /// `BZ2_blockSort`: sorts the rotations of `block` into `self.ptr` and
    /// returns `origPtr`, the sorted position of the rotation starting at 0.
    ///
    /// `block` is lent back unchanged: the main sort needs the overshoot
    /// after it, which is appended for the sort and removed again.
    pub(crate) fn sort(&mut self, block: &mut Vec<u8>) -> u32 {
        let n = block.len();
        self.ptr.clear();
        self.ptr.resize(n, 0);

        if n < FALLBACK_BELOW {
            fallback_sort(&mut self.ptr, block);
        } else {
            block.extend_from_within(..N_OVERSHOOT.min(n));
            self.quadrant.clear();
            self.quadrant.resize(n.saturating_add(N_OVERSHOOT), 0);
            self.ftab.clear();
            self.ftab.resize(65537, 0);

            let nblock = i32::try_from(n).unwrap_or(i32::MAX);
            let mut main = Main {
                ptr: &mut self.ptr,
                block,
                quadrant: &mut self.quadrant,
                nblock,
                budget: nblock.saturating_mul((WORK_FACTOR - 1) / 3),
            };
            main.sort(&mut self.ftab);
            let gave_up = main.budget < 0;
            block.truncate(n);
            if gave_up {
                fallback_sort(&mut self.ptr, block);
            }
        }

        // `ptr` is a permutation of 0..n, so 0 is in it.
        self.ptr
            .iter()
            .position(|&p| p == 0)
            .and_then(|i| u32::try_from(i).ok())
            .unwrap_or(0)
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

    /// The rotations sorted the obvious way, for comparison.
    fn naive(block: &[u8]) -> Vec<u32> {
        let n = block.len();
        let mut order: Vec<u32> = (0..n as u32).collect();
        order.sort_by(|&a, &b| {
            let ra = block[a as usize..].iter().chain(&block[..a as usize]);
            let rb = block[b as usize..].iter().chain(&block[..b as usize]);
            ra.cmp(rb)
        });
        order
    }

    /// Whether `ptr` lists the rotations in order: equal rotations may come
    /// in any order, so this compares the rotations, not the positions.
    fn sorted_rotations(block: &[u8], ptr: &[u32]) -> bool {
        let rot = |p: u32| {
            let p = p as usize;
            block[p..].iter().chain(&block[..p])
        };
        let mut seen = vec![false; block.len()];
        for &p in ptr {
            if seen[p as usize] {
                return false;
            }
            seen[p as usize] = true;
        }
        ptr.windows(2).all(|w| rot(w[0]).le(rot(w[1])))
    }

    fn lcg(seed: &mut u64) -> u8 {
        *seed = seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (*seed >> 33) as u8
    }

    #[test]
    fn the_fallback_sorts_small_blocks() {
        let mut seed = 1u64;
        for len in [1usize, 2, 3, 10, 100, 1000] {
            for alphabet in [1u8, 2, 4, 255] {
                let block: Vec<u8> = (0..len).map(|_| lcg(&mut seed) % alphabet).collect();
                let mut b = block.clone();
                let mut s = Sorter::default();
                let orig = s.sort(&mut b);
                assert_eq!(b, block, "the block comes back unchanged");
                assert!(
                    sorted_rotations(&block, &s.ptr),
                    "len {len} alphabet {alphabet}"
                );
                assert_eq!(s.ptr[orig as usize], 0);
                if alphabet == 255 {
                    // Distinct rotations: only one order is right.
                    assert_eq!(s.ptr, naive(&block));
                }
            }
        }
    }

    #[test]
    fn the_main_sort_sorts_large_blocks() {
        let mut seed = 7u64;
        let block: Vec<u8> = (0..30_000).map(|_| lcg(&mut seed) % 16).collect();
        let mut b = block.clone();
        let mut s = Sorter::default();
        let orig = s.sort(&mut b);
        assert_eq!(b, block);
        assert_eq!(s.ptr, naive(&block));
        assert_eq!(s.ptr[orig as usize], 0);
    }

    #[test]
    fn a_repetitive_block_exhausts_the_budget_and_falls_back() {
        // A period-3 block: the main sort compares equal rotations to the
        // end and runs out of budget.
        // Just over the size the main sort is tried at.
        let block: Vec<u8> = b"abc".iter().copied().cycle().take(10_002).collect();
        let nblock = block.len() as i32;
        let mut ext = block.clone();
        ext.extend_from_slice(&block[..N_OVERSHOOT]);
        let mut ptr = vec![0u32; block.len()];
        let mut quadrant = vec![0u16; ext.len()];
        let mut ftab = vec![0u32; 65537];
        let mut main = Main {
            ptr: &mut ptr,
            block: &ext,
            quadrant: &mut quadrant,
            nblock,
            budget: nblock * 9,
        };
        main.sort(&mut ftab);
        assert!(main.budget < 0, "the main sort should have given up");

        let mut b = block.clone();
        let mut s = Sorter::default();
        let orig = s.sort(&mut b);
        assert!(sorted_rotations(&block, &s.ptr));
        assert_eq!(s.ptr[orig as usize], 0);
    }

    #[test]
    fn mmed3_is_the_median() {
        for (a, b, c) in [
            (1, 2, 3),
            (3, 2, 1),
            (2, 3, 1),
            (1, 3, 2),
            (2, 1, 3),
            (3, 1, 2),
            (5, 5, 1),
        ] {
            let mut v = [a, b, c];
            v.sort_unstable();
            assert_eq!(mmed3(a, b, c), v[1], "{a} {b} {c}");
        }
    }
}
