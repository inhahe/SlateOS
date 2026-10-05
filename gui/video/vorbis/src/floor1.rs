//! Floor type 1 (Vorbis I §7): the spectral envelope as a piecewise-linear
//! curve in dB through up to 65 posts, each post's height coded as its
//! difference from the line through its neighbours.
//!
//! Translated into Rust from Tremor's `floor1.c`, copyright Xiph.Org, used
//! under its BSD licence (`licenses/tremor-COPYING`).

#![allow(
    clippy::arithmetic_side_effects,
    reason = "posts are 16-bit, heights 15-bit with a flag, products of the two well inside 32 bits; the curve's products wrap as C's"
)]
#![allow(
    clippy::indexing_slicing,
    reason = "classes are below 16 (4-bit), subclasses below 8 (2-bit), posts at most 65 with neighbours among them, curve heights clamped to the 256-entry table, and x below the block's half"
)]

use crate::bitpack::BitReader;
use crate::codebook::Book;
use crate::misc::{ilog, mult31_shift15};
use crate::tables::FLOOR_FROM_DB;

/// At most this many posts besides the two ends.
const VIF_POSIT: usize = 63;

/// A floor 1 and its lookups (`vorbis_info_floor1` with
/// `vorbis_look_floor1`).
#[derive(Clone, Debug)]
pub(crate) struct Floor1 {
    partitionclass: Vec<usize>,
    class_dim: [usize; 16],
    class_subs: [u32; 16],
    class_book: [usize; 16],
    /// -1 for none.
    class_subbook: [[i32; 8]; 16],
    mult: i32,
    /// The posts' x, the two ends first.
    postlist: Vec<i32>,
    /// The posts in order of x.
    forward_index: Vec<usize>,
    /// Each post's neighbours (from the third on) among the posts before
    /// it: the nearest below and above.
    loneighbor: Vec<usize>,
    hineighbor: Vec<usize>,
    quant_q: i32,
}

impl Floor1 {
    /// `floor1_unpack` and `floor1_look`: a floor from the setup header,
    /// its books checked against `books`, the number there are.
    pub(crate) fn unpack(opb: &mut BitReader<'_>, books: usize) -> Option<Self> {
        let books = books as i64;
        let partitions = opb.read(5);
        let mut partitionclass = Vec::new();
        let mut maxclass: i64 = -1;
        for _ in 0..partitions {
            let class = opb.read(4);
            if class < 0 {
                return None;
            }
            maxclass = maxclass.max(class);
            partitionclass.push(class as usize);
        }
        let mut class_dim = [0usize; 16];
        let mut class_subs = [0u32; 16];
        let mut class_book = [0usize; 16];
        let mut class_subbook = [[0i32; 8]; 16];
        for j in 0..(maxclass + 1) as usize {
            class_dim[j] = (opb.read(3) + 1) as usize;
            let subs = opb.read(2);
            if subs < 0 {
                return None;
            }
            class_subs[j] = subs as u32;
            if subs != 0 {
                let book = opb.read(8);
                if book < 0 || book >= books {
                    return None;
                }
                class_book[j] = book as usize;
            }
            for k in 0..1usize << subs {
                let book = opb.read(8) - 1;
                if book < -1 || book >= books {
                    return None;
                }
                class_subbook[j][k] = book as i32;
            }
        }
        let mult = (opb.read(2) + 1) as i32;
        let rangebits = opb.read(4);
        if rangebits < 0 {
            return None;
        }
        let mut postlist = vec![0i32, 1 << rangebits];
        let mut count = 0;
        for &class in &partitionclass {
            count += class_dim[class];
            if count > VIF_POSIT {
                return None;
            }
            while postlist.len() < count + 2 {
                let t = opb.read(rangebits as u32);
                if t < 0 || t >= 1 << rangebits {
                    return None;
                }
                postlist.push(t as i32);
            }
        }
        // Repeated posts would make segments of no length.
        let mut forward_index: Vec<usize> = (0..postlist.len()).collect();
        forward_index.sort_by_key(|&i| postlist[i]);
        if forward_index
            .windows(2)
            .any(|w| postlist[w[0]] == postlist[w[1]])
        {
            return None;
        }
        let quant_q = match mult {
            1 => 256,
            2 => 128,
            3 => 86,
            _ => 64,
        };
        // Each post's neighbours among those before it, for prediction.
        let n = postlist[1];
        let mut loneighbor = Vec::with_capacity(count);
        let mut hineighbor = Vec::with_capacity(count);
        for i in 0..count {
            let (mut lo, mut hi, mut lx, mut hx) = (0, 1, 0, n);
            let currentx = postlist[i + 2];
            for (j, &x) in postlist[..i + 2].iter().enumerate() {
                if x > lx && x < currentx {
                    lo = j;
                    lx = x;
                }
                if x < hx && x > currentx {
                    hi = j;
                    hx = x;
                }
            }
            loneighbor.push(lo);
            hineighbor.push(hi);
        }
        Some(Self {
            partitionclass,
            class_dim,
            class_subs,
            class_book,
            class_subbook,
            mult,
            postlist,
            forward_index,
            loneighbor,
            hineighbor,
            quant_q,
        })
    }

    /// The number of posts, both ends included.
    pub(crate) fn posts(&self) -> usize {
        self.postlist.len()
    }

    /// `floor1_inverse1`: this channel's posts from the packet into `fit`,
    /// unwrapped; false for a channel the packet leaves silent, or one cut
    /// short.
    #[inline(never)]
    pub(crate) fn inverse1(
        &self,
        opb: &mut BitReader<'_>,
        books: &[Book],
        fit: &mut Vec<i32>,
    ) -> bool {
        if opb.read(1) != 1 {
            return false;
        }
        let posts = self.posts();
        fit.clear();
        fit.resize(posts, 0);
        let bits = ilog((self.quant_q - 1) as u32) as u32;
        fit[0] = opb.read(bits) as i32;
        fit[1] = opb.read(bits) as i32;
        let mut j = 2;
        for &class in &self.partitionclass {
            let cdim = self.class_dim[class];
            let csubbits = self.class_subs[class];
            let csub = 1i64 << csubbits;
            let mut cval: i64 = 0;
            if csubbits != 0 {
                let Some(book) = books.get(self.class_book[class]) else {
                    return false;
                };
                cval = book.decode(opb);
                if cval == -1 {
                    return false;
                }
            }
            for k in 0..cdim {
                let book = self.class_subbook[class][(cval & (csub - 1)) as usize];
                cval >>= csubbits;
                fit[j + k] = if book >= 0 {
                    let Some(book) = books.get(book as usize) else {
                        return false;
                    };
                    let v = book.decode(opb);
                    if v == -1 {
                        return false;
                    }
                    v as i32
                } else {
                    0
                };
            }
            j += cdim;
        }
        // Unwrap each post, predicted from its neighbours, the flag 0x8000
        // marking one the encoder left on the line.
        for i in 2..posts {
            let (lo, hi) = (self.loneighbor[i - 2], self.hineighbor[i - 2]);
            let predicted = render_point(
                self.postlist[lo],
                self.postlist[hi],
                fit[lo],
                fit[hi],
                self.postlist[i],
            );
            let hiroom = self.quant_q - predicted;
            let loroom = predicted;
            let room = hiroom.min(loroom) * 2;
            let mut val = fit[i];
            if val != 0 {
                if val >= room {
                    if hiroom > loroom {
                        val -= loroom;
                    } else {
                        val = -1 - (val - hiroom);
                    }
                } else if val & 1 != 0 {
                    val = -((val + 1) >> 1);
                } else {
                    val >>= 1;
                }
                fit[i] = (val + predicted) & 0x7fff;
                fit[lo] &= 0x7fff;
                fit[hi] &= 0x7fff;
            } else {
                fit[i] = predicted | 0x8000;
            }
        }
        true
    }

    /// `floor1_inverse2`: `out`, this block's `n` spectral values, times the
    /// curve through the posts `fit` -- or zero for a channel with none.
    #[inline(never)]
    pub(crate) fn inverse2(&self, fit: Option<&[i32]>, out: &mut [i32]) {
        let Some(fit) = fit else {
            out.fill(0);
            return;
        };
        let n = out.len() as i32;
        let (mut hx, mut lx) = (0, 0);
        let mut ly = (fit[0] * self.mult).clamp(0, 255);
        for &current in &self.forward_index[1..] {
            let hy = fit[current] & 0x7fff;
            if hy == fit[current] {
                hx = self.postlist[current];
                let hy = (hy * self.mult).clamp(0, 255);
                render_line(n, lx, hx, ly, hy, out);
                lx = hx;
                ly = hy;
            }
        }
        // Past the last post: Tremor multiplies by the height itself.
        for v in out.iter_mut().skip(hx as usize) {
            *v = v.wrapping_mul(ly);
        }
    }
}

/// `render_point`: the line from `(x0, y0)` to `(x1, y1)` at `x`, the
/// heights' flags ignored.
fn render_point(x0: i32, x1: i32, y0: i32, y1: i32, x: i32) -> i32 {
    let y0 = y0 & 0x7fff;
    let y1 = y1 & 0x7fff;
    let dy = y1 - y0;
    let adx = x1 - x0;
    let off = dy.abs() * (x - x0) / adx;
    if dy < 0 { y0 - off } else { y0 + off }
}

/// `render_line`: `d[x0 .. min(x1, n)]` times the dB line from `y0` to
/// `y1`, stepped as Bresenham's.
fn render_line(n: i32, x0: i32, x1: i32, y0: i32, y1: i32, d: &mut [i32]) {
    let dy = y1 - y0;
    let adx = x1 - x0;
    let base = dy / adx;
    let sy = if dy < 0 { base - 1 } else { base + 1 };
    let ady = dy.abs() - (base * adx).abs();
    let n = n.min(x1);
    if x0 >= n {
        return;
    }
    let Some((first, rest)) = d
        .get_mut(x0 as usize..n as usize)
        .and_then(<[i32]>::split_first_mut)
    else {
        return;
    };
    // y moves from y0 toward y1 and stops a step short of it, so it stays
    // between the two, which inverse2 clamps to the table's 256: the mask
    // changes nothing, and spares the lookup its bounds check.
    let mut y = y0;
    let mut err = 0;
    *first = mult31_shift15(*first, FLOOR_FROM_DB[(y & 0xff) as usize]);
    for v in rest {
        err += ady;
        if err >= adx {
            err -= adx;
            y += sy;
        } else {
            y += base;
        }
        *v = mult31_shift15(*v, FLOOR_FROM_DB[(y & 0xff) as usize]);
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test: a failure should be loud"
)]
mod tests {
    use super::*;

    #[test]
    fn a_line_steps_evenly_from_end_to_end() {
        // Twelve steps up over 0 to 8: the heights Bresenham's gives, and
        // each value multiplied by its height's gain.
        let mut d = vec![1 << 20; 10];
        render_line(10, 0, 8, 100, 112, &mut d);
        let heights = [100, 101, 103, 104, 106, 107, 109, 110];
        for (x, &h) in heights.iter().enumerate() {
            assert_eq!(d[x], mult31_shift15(1 << 20, FLOOR_FROM_DB[h]), "x {x}");
        }
        // The line ends before its last post: x1 belongs to the next one.
        assert_eq!(d[8], 1 << 20);
        assert_eq!(render_point(0, 8, 100, 112, 4), 106);
        assert_eq!(render_point(0, 8, 112 | 0x8000, 100, 4), 106);
    }
}
