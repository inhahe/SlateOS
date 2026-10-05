//! Mapping type 0 (Vorbis I §4.2.4): which floor and residue each channel
//! uses (by submap), and which channel pairs are coded as magnitude and
//! angle.
//!
//! Translated into Rust from Tremor's `mapping0.c`, copyright Xiph.Org,
//! used under its BSD licence (`licenses/tremor-COPYING`); the decode
//! itself, `mapping0_inverse`, is [`crate::decoder`]'s, since it drives
//! every other part.

#![allow(
    clippy::arithmetic_side_effects,
    reason = "counts from 4- and 8-bit fields"
)]

use crate::bitpack::BitReader;

/// A mapping (`vorbis_info_mapping0`).
#[derive(Clone, Debug)]
pub(crate) struct Mapping {
    /// Each channel's submap.
    pub chmuxlist: Vec<usize>,
    /// Each submap's floor and residue.
    pub floorsubmap: Vec<usize>,
    pub residuesubmap: Vec<usize>,
    /// Magnitude and angle channels, in the order they were coupled.
    pub coupling: Vec<(usize, usize)>,
}

/// The bits a number below `v` needs (Tremor's `ilog` in `mapping0.c` and
/// `block.c`, which counts `v - 1`).
pub(crate) const fn ilog_below(v: u32) -> u32 {
    32 - v.saturating_sub(1).leading_zeros()
}

impl Mapping {
    /// `mapping0_unpack`: a mapping for `channels` channels from the setup
    /// header, its floors, residues and times checked against the counts
    /// there are.
    pub(crate) fn unpack(
        opb: &mut BitReader<'_>,
        channels: usize,
        times: i64,
        floors: usize,
        residues: usize,
    ) -> Option<Self> {
        let b = opb.read(1);
        if b < 0 {
            return None;
        }
        let submaps = if b != 0 { opb.read(4) + 1 } else { 1 };
        if submaps <= 0 {
            return None;
        }
        let b = opb.read(1);
        if b < 0 {
            return None;
        }
        let mut coupling = Vec::new();
        if b != 0 {
            let steps = opb.read(8) + 1;
            if steps <= 0 {
                return None;
            }
            let bits = ilog_below(channels as u32);
            for _ in 0..steps {
                let mag = opb.read(bits);
                let ang = opb.read(bits);
                if mag < 0
                    || ang < 0
                    || mag == ang
                    || mag >= channels as i64
                    || ang >= channels as i64
                {
                    return None;
                }
                coupling.push((mag as usize, ang as usize));
            }
        }
        // Reserved.
        if opb.read(2) != 0 {
            return None;
        }
        let mut chmuxlist = vec![0; channels];
        if submaps > 1 {
            for mux in &mut chmuxlist {
                let v = opb.read(4);
                if v < 0 || v >= submaps {
                    return None;
                }
                *mux = v as usize;
            }
        }
        let mut floorsubmap = Vec::with_capacity(submaps as usize);
        let mut residuesubmap = Vec::with_capacity(submaps as usize);
        for _ in 0..submaps {
            // The time submap: Vorbis I has no time backend, and Tremor
            // checks only the upper bound (the read below catches an end).
            if opb.read(8) >= times {
                return None;
            }
            let floor = opb.read(8);
            if floor < 0 || floor >= floors as i64 {
                return None;
            }
            let residue = opb.read(8);
            if residue < 0 || residue >= residues as i64 {
                return None;
            }
            floorsubmap.push(floor as usize);
            residuesubmap.push(residue as usize);
        }
        Some(Self {
            chmuxlist,
            floorsubmap,
            residuesubmap,
            coupling,
        })
    }
}

/// The inverse of square-polar coupling, for one magnitude and angle pair
/// of spectra (`mapping0_inverse`'s coupling loop).
#[inline(never)]
pub(crate) fn decouple(mag: &mut [i32], ang: &mut [i32]) {
    for (m, a) in mag.iter_mut().zip(ang.iter_mut()) {
        let (mv, av) = (*m, *a);
        if mv > 0 {
            if av > 0 {
                *a = mv.wrapping_sub(av);
            } else {
                *a = mv;
                *m = mv.wrapping_add(av);
            }
        } else if av > 0 {
            *a = mv.wrapping_add(av);
        } else {
            *a = mv;
            *m = mv.wrapping_sub(av);
        }
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
    fn ilog_below_counts_the_bits_of_the_largest_number_below() {
        assert_eq!(ilog_below(0), 0);
        assert_eq!(ilog_below(1), 0);
        assert_eq!(ilog_below(2), 1);
        assert_eq!(ilog_below(3), 2);
        assert_eq!(ilog_below(4), 2);
        assert_eq!(ilog_below(5), 3);
        assert_eq!(ilog_below(64), 6);
    }

    #[test]
    fn decoupling_restores_left_and_right() {
        // (M, A) as Vorbis's square-polar coupling makes them, back to
        // (left, right).
        let cases = [
            ((5, 2), (5, 3)),
            ((5, -2), (3, 5)),
            ((-5, 2), (-5, -3)),
            ((-5, -2), (-3, -5)),
        ];
        for ((m, a), want) in cases {
            let (mut mm, mut aa) = ([m], [a]);
            decouple(&mut mm, &mut aa);
            assert_eq!((mm[0], aa[0]), want, "M {m}, A {a}");
        }
    }
}
