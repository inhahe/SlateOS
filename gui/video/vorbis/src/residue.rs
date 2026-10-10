//! Residues (Vorbis I §8): the spectral fine structure that the floor
//! scales, coded partition by partition -- a classification word says
//! which books code which partitions, then up to eight passes add each
//! book's vectors in. Type 0 interleaves a vector's elements through its
//! partition, type 1 lays them out in order, type 2 interleaves the
//! channels first and codes them as one.
//!
//! Translated into Rust from Tremor's `res012.c`, copyright Xiph.Org, used
//! under its BSD licence (`licenses/tremor-COPYING`). Tremor tabulates
//! every classification word's digits in advance, a table a hostile setup
//! header can make hundreds of megabytes; this takes each word's digits
//! when it reads the word, which gives the same digits.

#![allow(
    clippy::arithmetic_side_effects,
    reason = "offsets bounded by the block's half (at most 4096 a channel, 255 channels); counts by the setup header's 6- and 8-bit fields"
)]
#![allow(
    clippy::indexing_slicing,
    reason = "classes are below the partition count, stages below 8, and every partition lies within the residue's end, which is clamped to the block's half"
)]

use crate::bitpack::BitReader;
use crate::codebook::Book;

/// A residue and its lookups (`vorbis_info_residue0` with
/// `vorbis_look_residue0`).
#[derive(Clone, Debug)]
pub(crate) struct Residue {
    /// 0, 1 or 2.
    kind: u8,
    begin: i64,
    end: i64,
    grouping: i64,
    partitions: usize,
    /// `partitions ^ dim`: the classification words there are.
    partvals: i64,
    groupbook: usize,
    /// Each class's passes, a bit a pass.
    secondstages: Vec<u32>,
    /// Each class's book for each pass it has.
    partbooks: Vec<[Option<usize>; 8]>,
    /// The most passes any class has.
    stages: u32,
}

impl Residue {
    /// `res0_unpack` and `res0_look`: a residue of `kind` from the setup
    /// header, its books checked against `books`.
    pub(crate) fn unpack(kind: u8, opb: &mut BitReader<'_>, books: &[Book]) -> Option<Self> {
        let begin = opb.read(24);
        let end = opb.read(24);
        let grouping = opb.read(24) + 1;
        let partitions = opb.read(6) + 1;
        let groupbook = opb.read(8);
        // A premature end of the packet.
        if groupbook < 0 {
            return None;
        }
        let mut secondstages = Vec::new();
        let mut acc = 0;
        for _ in 0..partitions {
            let mut cascade = opb.read(3);
            let cflag = opb.read(1);
            if cflag < 0 {
                return None;
            }
            if cflag != 0 {
                let c = opb.read(5);
                if c < 0 {
                    return None;
                }
                cascade |= c << 3;
            }
            secondstages.push(cascade as u32);
            acc += (cascade as u32).count_ones();
        }
        let mut booklist = Vec::with_capacity(acc as usize);
        for _ in 0..acc {
            let book = opb.read(8);
            if book < 0 {
                return None;
            }
            booklist.push(book as usize);
        }
        let group = books.get(groupbook as usize)?;
        for &b in &booklist {
            // A stage's book must have values to add.
            if books.get(b)?.maptype == 0 {
                return None;
            }
        }
        // The classification book must not promise more words than it has
        // entries for (an early beta encoder's books are oversized, and
        // still play).
        if group.dim < 1 {
            return None;
        }
        let mut partvals: i64 = 1;
        for _ in 0..group.dim {
            partvals *= partitions;
            if partvals > group.entries as i64 {
                return None;
            }
        }
        let mut partbooks = Vec::with_capacity(secondstages.len());
        let mut next = booklist.iter();
        let mut stages = 0;
        for &cascade in &secondstages {
            let mut list = [None; 8];
            let used = 32 - cascade.leading_zeros();
            stages = stages.max(used);
            for (k, slot) in list.iter_mut().enumerate().take(used as usize) {
                if cascade & (1 << k) != 0 {
                    *slot = next.next().copied();
                }
            }
            partbooks.push(list);
        }
        Some(Self {
            kind,
            begin,
            end,
            grouping,
            partitions: partitions as usize,
            partvals,
            groupbook: groupbook as usize,
            secondstages,
            partbooks,
            stages,
        })
    }

    /// `res0_inverse`, `res1_inverse` or `res2_inverse`: the residue added
    /// into `chans`, a submap's channels (each `half` long), of which
    /// those marked in `nonzero` are coded. A packet that ends early just
    /// ends the residue. `classes` is scratch.
    #[inline(never)]
    pub(crate) fn inverse(
        &self,
        opb: &mut BitReader<'_>,
        books: &[Book],
        chans: &mut [&mut [i32]],
        nonzero: &[bool],
        half: usize,
        classes: &mut Vec<u8>,
    ) {
        let Some(phrasebook) = books.get(self.groupbook) else {
            return;
        };
        if self.kind == 2 {
            self.inverse2(opb, books, phrasebook, chans, nonzero, half, classes);
            return;
        }
        // Types 0 and 1 code only the channels with a floor, in order.
        let mut used: Vec<&mut [i32]> = chans
            .iter_mut()
            .zip(nonzero)
            .filter(|(_, nz)| **nz)
            .map(|(c, _)| &mut **c)
            .collect();
        let ch = used.len();
        if ch == 0 {
            return;
        }
        let spp = self.grouping;
        let ppw = phrasebook.dim;
        let end = self.end.min(half as i64);
        let n = end - self.begin;
        if n <= 0 {
            return;
        }
        let partvals = (n / spp) as usize;
        let partwords = partvals.div_ceil(ppw);
        classes.clear();
        classes.resize(ch * partwords * ppw, 0);
        for s in 0..self.stages {
            let mut i = 0;
            let mut l = 0;
            while i < partvals {
                if s == 0 {
                    for j in 0..ch {
                        let at = (j * partwords + l) * ppw;
                        if !self.classify(opb, phrasebook, &mut classes[at..at + ppw]) {
                            return;
                        }
                    }
                }
                let mut k = 0;
                while k < ppw && i < partvals {
                    let offset = (self.begin + i as i64 * spp) as usize;
                    for (j, chan) in used.iter_mut().enumerate() {
                        let class = classes[(j * partwords + l) * ppw + k] as usize;
                        if self.secondstages[class] & (1 << s) == 0 {
                            continue;
                        }
                        let Some(book) =
                            self.partbooks[class][s as usize].and_then(|b| books.get(b))
                        else {
                            continue;
                        };
                        let a = &mut chan[offset..];
                        let r = if self.kind == 0 {
                            book.decodevs_add(a, opb, spp as usize, -8)
                        } else {
                            book.decodev_add(a, opb, spp as usize, -8)
                        };
                        if r == -1 {
                            return;
                        }
                    }
                    k += 1;
                    i += 1;
                }
                l += 1;
            }
        }
    }

    /// `res2_inverse`: the submap's channels interleaved and coded as one
    /// vector, if any of them is coded at all.
    #[allow(
        clippy::too_many_arguments,
        reason = "the residue's whole working set, as Tremor's"
    )]
    fn inverse2(
        &self,
        opb: &mut BitReader<'_>,
        books: &[Book],
        phrasebook: &Book,
        chans: &mut [&mut [i32]],
        nonzero: &[bool],
        half: usize,
        classes: &mut Vec<u8>,
    ) {
        let ch = chans.len();
        let spp = self.grouping;
        let ppw = phrasebook.dim;
        let end = self.end.min((half * ch) as i64);
        let n = end - self.begin;
        if n <= 0 {
            return;
        }
        let partvals = (n / spp) as usize;
        let partwords = partvals.div_ceil(ppw);
        let beginoff = (self.begin / ch as i64) as usize;
        if !nonzero.iter().any(|&nz| nz) {
            return;
        }
        let spp = (spp / ch as i64) as usize;
        classes.clear();
        classes.resize(partwords * ppw, 0);
        for s in 0..self.stages {
            let mut i = 0;
            let mut l = 0;
            while i < partvals {
                if s == 0 && !self.classify(opb, phrasebook, &mut classes[l * ppw..(l + 1) * ppw]) {
                    return;
                }
                let mut k = 0;
                while k < ppw && i < partvals {
                    let class = classes[l * ppw + k] as usize;
                    if self.secondstages[class] & (1 << s) != 0 {
                        if let Some(book) =
                            self.partbooks[class][s as usize].and_then(|b| books.get(b))
                        {
                            if book.decodevv_add(chans, i * spp + beginoff, ch, opb, spp, -8) == -1
                            {
                                return;
                            }
                        }
                    }
                    k += 1;
                    i += 1;
                }
                l += 1;
            }
        }
    }

    /// One classification word from the packet, its digits (base
    /// `partitions`, the first most significant) into `digits`; false at
    /// the packet's end or for a word past the last.
    fn classify(&self, opb: &mut BitReader<'_>, phrasebook: &Book, digits: &mut [u8]) -> bool {
        let temp = phrasebook.decode(opb);
        if temp == -1 || temp >= self.partvals {
            return false;
        }
        // Tremor's decodemap: mult starts at partvals / partitions.
        let parts = self.partitions as i64;
        let mut val = temp;
        let mut mult = self.partvals / parts;
        for d in digits.iter_mut() {
            let deco = if mult > 0 { val / mult } else { 0 };
            val -= deco * mult;
            mult /= parts;
            *d = deco as u8;
        }
        true
    }
}
