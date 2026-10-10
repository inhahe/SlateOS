//! The decoder: a packet's header bits (Tremor's `synthesis.c`), its
//! floors, residues, coupling, transform and window (`mapping0_inverse`),
//! and the overlap-add that turns blocks into samples (`block.c`).
//!
//! Translated into Rust from Tremor's `synthesis.c`, `mapping0.c` and
//! `block.c`, copyright Xiph.Org, used under its BSD licence
//! (`licenses/tremor-COPYING`). Tremor's granule-position bookkeeping is
//! left out: trimming a stream's ends is its container's business, as it
//! is for FFmpeg's decoder.

#![allow(
    clippy::arithmetic_side_effects,
    reason = "offsets within blocks of 64 to 8192 samples; the overlap's sums wrap, as C's"
)]
#![allow(
    clippy::indexing_slicing,
    reason = "channel numbers below the channel count the mapping was checked against; offsets within the two-block buffers this module sizes"
)]

use crate::Error;
use crate::bitpack::BitReader;
use crate::floor0::Floor0Look;
use crate::info::{Floor, Info, Setup};
use crate::mapping::{decouple, ilog_below};
use crate::misc::clip_to_15;
use crate::{mdct, window};

/// A Vorbis stream's decoder: its headers, and the half block it has not
/// finished overlapping.
#[derive(Clone, Debug)]
pub struct Decoder {
    info: Info,
    setup: Setup,
    modebits: u32,
    /// Each floor 0's lookups, by block size (made for the sizes a mode
    /// uses it at).
    floor0_looks: Vec<[Option<Floor0Look>; 2]>,
    slopes: [&'static [i32]; 2],
    /// Each channel's overlap buffer, a long block (`vorbis_dsp_state`'s
    /// `pcm`): two halves used in turn.
    pcm: Vec<Vec<i32>>,
    /// Which half is next (`centerW`): 0 or half a long block.
    center_w: usize,
    pcm_current: usize,
    /// None before the first block (`pcm_returned == -1`).
    pcm_returned: Option<usize>,
    /// The previous and the current block's sizes, long or not.
    lw: bool,
    w: bool,
    /// The block being decoded, a channel each (`vorbis_block`'s `pcm`).
    block: Vec<Vec<i32>>,
    /// Each channel's floor, as its first stage leaves it.
    memos: Vec<Vec<i32>>,
    nonzero: Vec<bool>,
    classes: Vec<u8>,
}

impl Decoder {
    /// A decoder for the stream whose identification and setup headers
    /// these are (Tremor's `vorbis_synthesis_headerin` for both, then
    /// `vorbis_synthesis_init`). The comment header is not needed; read it
    /// with [`crate::Comments::parse`].
    pub fn new(id: &[u8], setup: &[u8]) -> Result<Self, Error> {
        let info = Info::parse(id)?;
        let setup = Setup::parse(setup, info.channels)?;
        let bad = Error::BadHeader;
        let modebits = ilog_below(setup.modes.len() as u32);
        let slopes = [
            window::slope(info.blocksizes[0] / 2).ok_or(bad)?,
            window::slope(info.blocksizes[1] / 2).ok_or(bad)?,
        ];
        // Floor 0's lookups depend on the block size: made, as Tremor makes
        // them, for each mode's mapping's floors at that mode's size.
        let mut floor0_looks: Vec<[Option<Floor0Look>; 2]> = vec![[None, None]; setup.floors.len()];
        for mode in &setup.modes {
            let size = usize::from(mode.blockflag);
            for &f in &setup.maps[mode.mapping].floorsubmap {
                if let Floor::Zero(floor) = &setup.floors[f] {
                    if floor0_looks[f][size].is_none() {
                        floor0_looks[f][size] =
                            Some(floor.look(info.blocksizes[size] / 2).ok_or(bad)?);
                    }
                }
            }
        }
        let channels = info.channels;
        let long = info.blocksizes[1];
        Ok(Self {
            info,
            setup,
            modebits,
            floor0_looks,
            slopes,
            pcm: vec![vec![0; long]; channels],
            center_w: long / 2,
            pcm_current: long / 2,
            pcm_returned: None,
            lw: false,
            w: false,
            block: vec![vec![0; long]; channels],
            memos: vec![Vec::new(); channels],
            nonzero: vec![false; channels],
            classes: Vec::new(),
        })
    }

    /// The identification header's facts.
    pub fn info(&self) -> &Info {
        &self.info
    }

    /// The most samples a channel one packet can complete: half a long
    /// block.
    pub fn max_samples(&self) -> usize {
        self.info.blocksizes[1] / 2
    }

    /// Forgets the half block in hand, as after a seek
    /// (`vorbis_synthesis_restart`): the next packet completes no samples,
    /// and only primes the overlap.
    pub fn reset(&mut self) {
        let long = self.info.blocksizes[1];
        self.center_w = long / 2;
        self.pcm_current = long / 2;
        self.pcm_returned = None;
    }

    /// The samples a channel this packet will complete, without decoding
    /// it: 0 for the first after [`Self::new`] or [`Self::reset`], else a
    /// quarter of the previous block and a quarter of this one.
    pub fn samples_in(&self, packet: &[u8]) -> Result<usize, Error> {
        let w = self.blockflag(packet)?;
        Ok(match self.pcm_returned {
            None => 0,
            Some(_) => {
                self.info.blocksizes[usize::from(self.w)] / 4
                    + self.info.blocksizes[usize::from(w)] / 4
            }
        })
    }

    /// Decodes an audio packet into `out`, interleaved, as Tremor's
    /// `ov_read` gives samples: 16-bit, clipped. Returns the samples a
    /// channel. A packet that is not audio, or whose header bits are
    /// damaged, is an error and changes nothing; a packet cut short
    /// decodes as far as it goes, as Tremor's does.
    pub fn decode(&mut self, packet: &[u8], out: &mut [i16]) -> Result<usize, Error> {
        self.decode_into(packet, out, |v| clip_to_15(v >> 9) as i16)
    }

    /// As [`Self::decode`], at Tremor's own precision: 1.0 is `1 << 24`,
    /// and nothing is clipped.
    pub fn decode_i32(&mut self, packet: &[u8], out: &mut [i32]) -> Result<usize, Error> {
        self.decode_into(packet, out, |v| v)
    }

    /// As [`Self::decode_i32`], as floating point: 1.0 is full scale.
    pub fn decode_float(&mut self, packet: &[u8], out: &mut [f32]) -> Result<usize, Error> {
        self.decode_into(packet, out, |v| v as f32 * (1.0 / 16_777_216.0))
    }

    /// The packet's mode's block size, long or not (the start of
    /// `_vorbis_synthesis1`, and `vorbis_packet_blocksize`).
    fn blockflag(&self, packet: &[u8]) -> Result<bool, Error> {
        let mut opb = BitReader::new(packet);
        if opb.read(1) != 0 {
            return Err(Error::NotAudio);
        }
        let mode = opb.read(self.modebits);
        let mode = usize::try_from(mode).map_err(|_| Error::BadPacket)?;
        Ok(self
            .setup
            .modes
            .get(mode)
            .ok_or(Error::BadPacket)?
            .blockflag)
    }

    fn decode_into<T: Copy>(
        &mut self,
        packet: &[u8],
        out: &mut [T],
        convert: impl Fn(i32) -> T,
    ) -> Result<usize, Error> {
        let channels = self.info.channels;
        if out.len() < self.samples_in(packet)? * channels {
            return Err(Error::BufferTooSmall);
        }
        let w = self.synthesis(packet)?;
        self.blockin(w);
        // `vorbis_synthesis_pcmout`, then `vorbis_synthesis_read` of all.
        let Some(from) = self.pcm_returned else {
            return Ok(0);
        };
        let to = self.pcm_current;
        let count = to.saturating_sub(from);
        for (c, chan) in self.pcm.iter().enumerate() {
            for (i, &v) in chan[from..to].iter().enumerate() {
                out[i * channels + c] = convert(v);
            }
        }
        self.pcm_returned = Some(to);
        Ok(count)
    }

    /// `_vorbis_synthesis1` with `mapping0_inverse`: the packet's block,
    /// windowed, into `self.block`; whether it is long.
    fn synthesis(&mut self, packet: &[u8]) -> Result<bool, Error> {
        let mut opb = BitReader::new(packet);
        if opb.read(1) != 0 {
            return Err(Error::NotAudio);
        }
        let mode = opb.read(self.modebits);
        let mode = *usize::try_from(mode)
            .ok()
            .and_then(|m| self.setup.modes.get(m))
            .ok_or(Error::BadPacket)?;
        let w = mode.blockflag;
        let (lw, nw) = if w {
            let lw = opb.read(1);
            let nw = opb.read(1);
            if nw == -1 {
                return Err(Error::BadPacket);
            }
            (lw != 0, nw != 0)
        } else {
            (false, false)
        };

        let n = self.info.blocksizes[usize::from(w)];
        let half = n / 2;
        let setup = &self.setup;
        let map = &setup.maps[mode.mapping];
        let channels = self.info.channels;

        // The spectral envelope, each channel's floor's first stage.
        for ch in 0..channels {
            let submap = map.chmuxlist[ch];
            let memo = &mut self.memos[ch];
            self.nonzero[ch] = match &setup.floors[map.floorsubmap[submap]] {
                Floor::Zero(f) => f.inverse1(&mut opb, &setup.books, memo),
                Floor::One(f) => f.inverse1(&mut opb, &setup.books, memo),
            };
            self.block[ch][..half].fill(0);
        }
        // A coupled pair is coded if either channel is.
        let floored: Vec<bool> = self.nonzero.clone();
        for &(mag, ang) in &map.coupling {
            if self.nonzero[mag] || self.nonzero[ang] {
                self.nonzero[mag] = true;
                self.nonzero[ang] = true;
            }
        }
        // The residue, submap by submap.
        for (i, &r) in map.residuesubmap.iter().enumerate() {
            let mut bundle: Vec<&mut [i32]> = Vec::new();
            let mut zero = Vec::new();
            for (ch, chan) in self.block.iter_mut().enumerate() {
                if map.chmuxlist[ch] == i {
                    bundle.push(&mut chan[..half]);
                    zero.push(self.nonzero[ch]);
                }
            }
            setup.residues[r].inverse(
                &mut opb,
                &setup.books,
                &mut bundle,
                &zero,
                half,
                &mut self.classes,
            );
        }
        // Undo the coupling, last step first.
        for &(mag, ang) in map.coupling.iter().rev() {
            let (m, a) = two_mut(&mut self.block, mag, ang);
            decouple(&mut m[..half], &mut a[..half]);
        }
        // The envelope applied: each floor's second stage.
        for ch in 0..channels {
            let submap = map.chmuxlist[ch];
            let f = map.floorsubmap[submap];
            let memo = floored[ch].then_some(self.memos[ch].as_slice());
            let out = &mut self.block[ch][..half];
            match &setup.floors[f] {
                Floor::Zero(floor) => match &self.floor0_looks[f][usize::from(w)] {
                    Some(look) => floor.inverse2(look, memo, out),
                    None => out.fill(0),
                },
                Floor::One(floor) => floor.inverse2(memo, out),
            }
        }
        // To the time domain, and windowed.
        let size = |flag: bool| self.info.blocksizes[usize::from(flag)];
        for ch in 0..channels {
            let chan = &mut self.block[ch];
            mdct::backward(n, chan);
            if self.nonzero[ch] {
                window::apply(
                    chan,
                    n,
                    (size(lw), self.slopes[usize::from(lw)]),
                    (size(nw), self.slopes[usize::from(nw)]),
                );
            } else {
                chan[..n].fill(0);
            }
        }
        Ok(w)
    }

    /// `vorbis_synthesis_blockin`: the new block's first half overlapped
    /// with the last block's second, and its own second half kept for the
    /// next.
    #[inline(never)]
    fn blockin(&mut self, w: bool) {
        self.lw = self.w;
        self.w = w;
        let bs = self.info.blocksizes;
        let n = bs[usize::from(w)] / 2;
        let n0 = bs[0] / 2;
        let n1 = bs[1] / 2;
        let (this_center, prev_center) = if self.center_w != 0 { (n1, 0) } else { (0, n1) };
        for (pcm, p) in self.pcm.iter_mut().zip(&self.block) {
            let add = |dst: &mut [i32], src: &[i32]| {
                for (d, &s) in dst.iter_mut().zip(src) {
                    *d = d.wrapping_add(s);
                }
            };
            match (self.lw, self.w) {
                (true, true) => add(&mut pcm[prev_center..prev_center + n1], &p[..n1]),
                (true, false) => {
                    let at = prev_center + n1 / 2 - n0 / 2;
                    add(&mut pcm[at..at + n0], &p[..n0]);
                }
                (false, true) => {
                    let off = n1 / 2 - n0 / 2;
                    add(&mut pcm[prev_center..prev_center + n0], &p[off..off + n0]);
                    let rest = n1 / 2 + n0 / 2;
                    pcm[prev_center + n0..prev_center + rest]
                        .copy_from_slice(&p[off + n0..off + rest]);
                }
                (false, false) => add(&mut pcm[prev_center..prev_center + n0], &p[..n0]),
            }
            pcm[this_center..this_center + n].copy_from_slice(&p[n..2 * n]);
        }
        self.center_w = if self.center_w != 0 { 0 } else { n1 };
        if self.pcm_returned.is_none() {
            self.pcm_returned = Some(this_center);
            self.pcm_current = this_center;
        } else {
            self.pcm_returned = Some(prev_center);
            self.pcm_current =
                prev_center + bs[usize::from(self.lw)] / 4 + bs[usize::from(self.w)] / 4;
        }
    }
}

/// Two different channels of `v`, both mutable.
fn two_mut(v: &mut [Vec<i32>], a: usize, b: usize) -> (&mut [i32], &mut [i32]) {
    if a < b {
        let (lo, hi) = v.split_at_mut(b);
        (&mut lo[a], &mut hi[0])
    } else {
        let (lo, hi) = v.split_at_mut(a);
        (&mut hi[0], &mut lo[b])
    }
}
