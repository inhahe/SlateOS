//! Arithmetic entropy decoding: libjpeg-turbo's `jdarith.c` and the
//! probability estimation table of `jaricom.c`.
//!
//! Arithmetic coding was patent-encumbered for most of JPEG's life, so almost
//! nothing writes it, but libjpeg-turbo decodes it by default and so does every
//! program built on it; a file that uses it is a file those programs show.
//!
//! Unlike Huffman data, reaching a marker inside arithmetic-coded data is
//! legal: the decoder supplies zero bytes from then on. Corrupt data surfaces
//! as a decision sequence that cannot happen -- a magnitude past 15 bits, a
//! run past the end of the band -- after which libjpeg sets the decoder's
//! counter to -1 and decodes nothing more in the scan (until a restart), with
//! a warning rather than an error. Both are reproduced.

use super::coef::Coefficients;
use super::error::{Error, jerr};
use super::huffman::{Pass, left_shift};
use super::marker::{Header, Input, read_restart_marker};
use super::tables::natural;

/// `jpeg_aritab`: `(Qe << 16) | (Next_Index_MPS << 8) | (Switch_MPS << 7) |
/// Next_Index_LPS`, for each of the 113 states and the fixed 114th.
const ARITAB: [u32; 114] = {
    const fn v(qe: u32, lps: u32, mps: u32, switch: u32) -> u32 {
        (qe << 16) | (mps << 8) | (switch << 7) | lps
    }
    [
        v(0x5a1d, 1, 1, 1),
        v(0x2586, 14, 2, 0),
        v(0x1114, 16, 3, 0),
        v(0x080b, 18, 4, 0),
        v(0x03d8, 20, 5, 0),
        v(0x01da, 23, 6, 0),
        v(0x00e5, 25, 7, 0),
        v(0x006f, 28, 8, 0),
        v(0x0036, 30, 9, 0),
        v(0x001a, 33, 10, 0),
        v(0x000d, 35, 11, 0),
        v(0x0006, 9, 12, 0),
        v(0x0003, 10, 13, 0),
        v(0x0001, 12, 13, 0),
        v(0x5a7f, 15, 15, 1),
        v(0x3f25, 36, 16, 0),
        v(0x2cf2, 38, 17, 0),
        v(0x207c, 39, 18, 0),
        v(0x17b9, 40, 19, 0),
        v(0x1182, 42, 20, 0),
        v(0x0cef, 43, 21, 0),
        v(0x09a1, 45, 22, 0),
        v(0x072f, 46, 23, 0),
        v(0x055c, 48, 24, 0),
        v(0x0406, 49, 25, 0),
        v(0x0303, 51, 26, 0),
        v(0x0240, 52, 27, 0),
        v(0x01b1, 54, 28, 0),
        v(0x0144, 56, 29, 0),
        v(0x00f5, 57, 30, 0),
        v(0x00b7, 59, 31, 0),
        v(0x008a, 60, 32, 0),
        v(0x0068, 62, 33, 0),
        v(0x004e, 63, 34, 0),
        v(0x003b, 32, 35, 0),
        v(0x002c, 33, 9, 0),
        v(0x5ae1, 37, 37, 1),
        v(0x484c, 64, 38, 0),
        v(0x3a0d, 65, 39, 0),
        v(0x2ef1, 67, 40, 0),
        v(0x261f, 68, 41, 0),
        v(0x1f33, 69, 42, 0),
        v(0x19a8, 70, 43, 0),
        v(0x1518, 72, 44, 0),
        v(0x1177, 73, 45, 0),
        v(0x0e74, 74, 46, 0),
        v(0x0bfb, 75, 47, 0),
        v(0x09f8, 77, 48, 0),
        v(0x0861, 78, 49, 0),
        v(0x0706, 79, 50, 0),
        v(0x05cd, 48, 51, 0),
        v(0x04de, 50, 52, 0),
        v(0x040f, 50, 53, 0),
        v(0x0363, 51, 54, 0),
        v(0x02d4, 52, 55, 0),
        v(0x025c, 53, 56, 0),
        v(0x01f8, 54, 57, 0),
        v(0x01a4, 55, 58, 0),
        v(0x0160, 56, 59, 0),
        v(0x0125, 57, 60, 0),
        v(0x00f6, 58, 61, 0),
        v(0x00cb, 59, 62, 0),
        v(0x00ab, 61, 63, 0),
        v(0x008f, 61, 32, 0),
        v(0x5b12, 65, 65, 1),
        v(0x4d04, 80, 66, 0),
        v(0x412c, 81, 67, 0),
        v(0x37d8, 82, 68, 0),
        v(0x2fe8, 83, 69, 0),
        v(0x293c, 84, 70, 0),
        v(0x2379, 86, 71, 0),
        v(0x1edf, 87, 72, 0),
        v(0x1aa9, 87, 73, 0),
        v(0x174e, 72, 74, 0),
        v(0x1424, 72, 75, 0),
        v(0x119c, 74, 76, 0),
        v(0x0f6b, 74, 77, 0),
        v(0x0d51, 75, 78, 0),
        v(0x0bb6, 77, 79, 0),
        v(0x0a40, 77, 48, 0),
        v(0x5832, 80, 81, 1),
        v(0x4d1c, 88, 82, 0),
        v(0x438e, 89, 83, 0),
        v(0x3bdd, 90, 84, 0),
        v(0x34ee, 91, 85, 0),
        v(0x2eae, 92, 86, 0),
        v(0x299a, 93, 87, 0),
        v(0x2516, 86, 71, 0),
        v(0x5570, 88, 89, 1),
        v(0x4ca9, 95, 90, 0),
        v(0x44d9, 96, 91, 0),
        v(0x3e22, 97, 92, 0),
        v(0x3824, 99, 93, 0),
        v(0x32b4, 99, 94, 0),
        v(0x2e17, 93, 86, 0),
        v(0x56a8, 95, 96, 1),
        v(0x4f46, 101, 97, 0),
        v(0x47e5, 102, 98, 0),
        v(0x41cf, 103, 99, 0),
        v(0x3c3d, 104, 100, 0),
        v(0x375e, 99, 93, 0),
        v(0x5231, 105, 102, 0),
        v(0x4c0f, 106, 103, 0),
        v(0x4639, 107, 104, 0),
        v(0x415e, 103, 99, 0),
        v(0x5627, 105, 106, 1),
        v(0x50e7, 108, 107, 0),
        v(0x4b85, 109, 103, 0),
        v(0x5597, 110, 109, 0),
        v(0x504f, 111, 107, 0),
        v(0x5a10, 110, 111, 1),
        v(0x5522, 112, 109, 0),
        v(0x59eb, 112, 111, 1),
        v(0x5a1d, 113, 113, 0),
    ]
};

/// `DC_STAT_BINS` and `AC_STAT_BINS`.
const DC_BINS: usize = 64;
const AC_BINS: usize = 256;

/// Which statistics bin a decision uses.
#[derive(Clone, Copy)]
enum Bin {
    Dc(usize, usize),
    Ac(usize, usize),
    Fixed,
}

/// An arithmetic-coded scan (`arith_entropy_decoder`).
#[derive(Clone)]
pub(super) struct Arith {
    /// `None` for sequential; the progressive pass otherwise.
    pass: Option<Pass>,
    c: i64,
    a: i64,
    /// Bits of `c` not yet shifted into the decision; -16 at the start of a
    /// segment, -1 once the data has been found corrupt.
    ct: i32,
    last_dc: [i32; 4],
    dc_context: [usize; 4],
    restarts_to_go: u32,
    dc_stats: [[u8; DC_BINS]; 16],
    ac_stats: [[u8; AC_BINS]; 16],
    fixed_bin: u8,
    /// Each block's component's position in the scan.
    positions: [usize; 10],
    blocks_in_mcu: usize,
}

impl Arith {
    /// `start_pass`, after [`Pass::of`] and the progression update for a
    /// progressive scan.
    pub(super) fn start(
        input: &mut Input<'_>,
        header: &Header,
        pass: Option<Pass>,
        membership: &[usize],
    ) -> Result<Self, Error> {
        let scan = &header.scan;
        if pass.is_none() && (scan.ss != 0 || scan.se != 63 || scan.ah != 0 || scan.al != 0) {
            // JWRN_NOT_SEQUENTIAL.
            input.warn();
        }
        // libjpeg allocates the statistics once per image and zeroes, per
        // scan, those the scan uses; the others it does not read, so starting
        // every scan from zeros is the same thing.
        let mut dc_stats = [[0u8; DC_BINS]; 16];
        let mut ac_stats = [[0u8; AC_BINS]; 16];
        let uses_dc = pass.is_none_or(|p| p == Pass::DcFirst);
        let uses_ac = pass.is_none_or(|p| matches!(p, Pass::AcFirst | Pass::AcRefine));
        for &ci in scan.components() {
            let component = header.components.get(ci).ok_or(jerr::BAD_COMPONENT_ID)?;
            if uses_dc {
                let stats = dc_stats
                    .get_mut(usize::from(component.dc_tbl_no))
                    .ok_or(jerr::NO_ARITH_TABLE)?;
                *stats = [0; DC_BINS];
            }
            if uses_ac {
                let stats = ac_stats
                    .get_mut(usize::from(component.ac_tbl_no))
                    .ok_or(jerr::NO_ARITH_TABLE)?;
                *stats = [0; AC_BINS];
            }
        }
        let mut positions = [0usize; 10];
        for (slot, &position) in positions.iter_mut().zip(membership) {
            *slot = position;
        }
        Ok(Self {
            pass,
            c: 0,
            a: 0,
            ct: -16,
            last_dc: [0; 4],
            dc_context: [0; 4],
            restarts_to_go: u32::from(header.restart_interval),
            dc_stats,
            ac_stats,
            fixed_bin: 113,
            positions,
            blocks_in_mcu: membership.len(),
        })
    }

    /// `get_byte`.
    fn byte(input: &mut Input<'_>) -> i64 {
        i64::from(input.src.byte())
    }

    fn bin(&mut self, bin: Bin) -> &mut u8 {
        let fallback = &mut self.fixed_bin;
        match bin {
            Bin::Dc(table, at) => self
                .dc_stats
                .get_mut(table)
                .and_then(|t| t.get_mut(at))
                .unwrap_or(fallback),
            Bin::Ac(table, at) => self
                .ac_stats
                .get_mut(table)
                .and_then(|t| t.get_mut(at))
                .unwrap_or(fallback),
            Bin::Fixed => fallback,
        }
    }

    /// `arith_decode`: one binary decision.
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "the coder's registers: A stays below 2^17, C holds at most 16 bits of interval and 16 of buffered data, and the bin, magnitude and position counters are all small: nothing here approaches the i64 or usize range"
    )]
    fn decode(&mut self, input: &mut Input<'_>, bin: Bin) -> i32 {
        // Renormalisation and data input (D.2.6).
        while self.a < 0x8000 {
            self.ct = self.ct.wrapping_sub(1);
            if self.ct < 0 {
                let data = if input.unread_marker != 0 {
                    0
                } else {
                    let mut data = Self::byte(input);
                    if data == 0xFF {
                        loop {
                            data = Self::byte(input);
                            if data != 0xFF {
                                break;
                            }
                        }
                        if data == 0 {
                            data = 0xFF;
                        } else {
                            input.unread_marker = data as u8;
                            data = 0;
                        }
                    }
                    data
                };
                self.c = (self.c << 8) | data;
                self.ct = self.ct.wrapping_add(8);
                if self.ct < 0 {
                    self.ct = self.ct.wrapping_add(1);
                    if self.ct == 0 {
                        // Two initial bytes read.
                        self.a = 0x8000;
                    }
                }
            }
            self.a <<= 1;
        }
        let state = *self.bin(bin);
        let sv = i32::from(state);
        let entry = ARITAB.get(usize::from(state & 0x7F)).copied().unwrap_or(0);
        let nl = (entry & 0xFF) as u8;
        let nm = ((entry >> 8) & 0xFF) as u8;
        let qe = i64::from(entry >> 16);
        let mut sv_out = sv;
        let temp = self.a - qe;
        self.a = temp;
        let shifted = temp << self.ct.clamp(0, 62);
        let new_state;
        if self.c >= shifted {
            self.c -= shifted;
            if self.a < qe {
                self.a = qe;
                new_state = (state & 0x80) ^ nm;
            } else {
                self.a = qe;
                new_state = (state & 0x80) ^ nl;
                sv_out ^= 0x80;
            }
            *self.bin(bin) = new_state;
        } else if self.a < 0x8000 {
            if self.a < qe {
                new_state = (state & 0x80) ^ nl;
                sv_out ^= 0x80;
            } else {
                new_state = (state & 0x80) ^ nm;
            }
            *self.bin(bin) = new_state;
        }
        sv_out >> 7
    }

    /// `process_restart`.
    fn restart(&mut self, input: &mut Input<'_>, header: &Header) {
        read_restart_marker(input);
        let scan = &header.scan;
        for (position, &ci) in scan.components().iter().enumerate() {
            let Some(component) = header.components.get(ci) else {
                continue;
            };
            let dc = self.pass.is_none_or(|p| p == Pass::DcFirst);
            let ac = self
                .pass
                .is_none_or(|p| matches!(p, Pass::AcFirst | Pass::AcRefine));
            if dc {
                if let Some(stats) = self.dc_stats.get_mut(usize::from(component.dc_tbl_no)) {
                    *stats = [0; DC_BINS];
                }
                if let Some(last) = self.last_dc.get_mut(position) {
                    *last = 0;
                }
                if let Some(context) = self.dc_context.get_mut(position) {
                    *context = 0;
                }
            }
            if ac {
                if let Some(stats) = self.ac_stats.get_mut(usize::from(component.ac_tbl_no)) {
                    *stats = [0; AC_BINS];
                }
            }
        }
        self.c = 0;
        self.a = 0;
        self.ct = -16;
        self.restarts_to_go = u32::from(header.restart_interval);
    }

    /// One MCU.
    pub(super) fn decode_mcu(
        &mut self,
        input: &mut Input<'_>,
        header: &Header,
        blocks: &mut [[i16; 64]],
    ) {
        if header.restart_interval != 0 {
            if self.restarts_to_go == 0 {
                self.restart(input, header);
            }
            self.restarts_to_go = self.restarts_to_go.wrapping_sub(1);
        }
        match self.pass {
            None => self.sequential(input, header, blocks),
            Some(Pass::DcFirst) => self.dc_first(input, header, blocks),
            Some(Pass::DcRefine) => self.dc_refine(input, header, blocks),
            Some(Pass::AcFirst | Pass::AcRefine) => {
                if let Some(block) = blocks.first_mut() {
                    self.ac(input, header, block);
                }
            }
        }
    }

    /// One MCU of an AC scan decoded into `block` wherever it lies:
    /// [`Self::decode_mcu`] for the scans the coefficient store decodes in
    /// place.
    pub(super) fn decode_ac<B: Coefficients>(
        &mut self,
        input: &mut Input<'_>,
        header: &Header,
        block: &mut B,
    ) {
        if header.restart_interval != 0 {
            if self.restarts_to_go == 0 {
                self.restart(input, header);
            }
            self.restarts_to_go = self.restarts_to_go.wrapping_sub(1);
        }
        self.ac(input, header, block);
    }

    /// The AC pass this scan is.
    fn ac<B: Coefficients>(&mut self, input: &mut Input<'_>, header: &Header, block: &mut B) {
        match self.pass {
            Some(Pass::AcFirst) => self.ac_first(input, header, block),
            Some(Pass::AcRefine) => self.ac_refine(input, header, block),
            _ => {}
        }
    }

    /// The DC difference of one block (F.19-F.24), updating the component's
    /// conditioning; `None` once the data is found corrupt.
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "the coder's registers: A stays below 2^17, C holds at most 16 bits of interval and 16 of buffered data, and the bin, magnitude and position counters are all small: nothing here approaches the i64 or usize range"
    )]
    fn dc_difference(
        &mut self,
        input: &mut Input<'_>,
        header: &Header,
        position: usize,
        table: usize,
    ) -> Option<i32> {
        let context = self.dc_context.get(position).copied().unwrap_or(0);
        if self.decode(input, Bin::Dc(table, context)) == 0 {
            if let Some(slot) = self.dc_context.get_mut(position) {
                *slot = 0;
            }
            return Some(0);
        }
        let sign = self.decode(input, Bin::Dc(table, context.saturating_add(1)));
        let mut st = context.saturating_add(2).saturating_add(sign as usize);
        let mut m = self.decode(input, Bin::Dc(table, st));
        if m != 0 {
            st = 20;
            while self.decode(input, Bin::Dc(table, st)) != 0 {
                m <<= 1;
                if m == 0x8000 {
                    // JWRN_ARITH_BAD_CODE: magnitude overflow.
                    input.warn();
                    self.ct = -1;
                    return None;
                }
                st = st.saturating_add(1);
            }
        }
        let low = header.arith_dc_l.get(table).copied().unwrap_or(0);
        let high = header.arith_dc_u.get(table).copied().unwrap_or(1);
        let new_context = if m < ((1i32 << low) >> 1) {
            0
        } else if m > ((1i32 << high) >> 1) {
            12 + (sign as usize) * 4
        } else {
            4 + (sign as usize) * 4
        };
        if let Some(slot) = self.dc_context.get_mut(position) {
            *slot = new_context;
        }
        let mut v = m;
        st = st.saturating_add(14);
        loop {
            m >>= 1;
            if m == 0 {
                break;
            }
            if self.decode(input, Bin::Dc(table, st)) != 0 {
                v |= m;
            }
        }
        v += 1;
        Some(if sign != 0 { -v } else { v })
    }

    /// The magnitude and sign of one nonzero AC coefficient at `k`, whose
    /// statistics start at `st`; `None` on corrupt data.
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "the coder's registers: A stays below 2^17, C holds at most 16 bits of interval and 16 of buffered data, and the bin, magnitude and position counters are all small: nothing here approaches the i64 or usize range"
    )]
    fn ac_value(
        &mut self,
        input: &mut Input<'_>,
        header: &Header,
        table: usize,
        k: usize,
        mut st: usize,
    ) -> Option<i32> {
        let sign = self.decode(input, Bin::Fixed);
        st = st.saturating_add(2);
        let mut m = self.decode(input, Bin::Ac(table, st));
        if m != 0 && self.decode(input, Bin::Ac(table, st)) != 0 {
            m <<= 1;
            let limit = usize::from(header.arith_ac_k.get(table).copied().unwrap_or(5));
            st = if k <= limit { 189 } else { 217 };
            while self.decode(input, Bin::Ac(table, st)) != 0 {
                m <<= 1;
                if m == 0x8000 {
                    input.warn();
                    self.ct = -1;
                    return None;
                }
                st = st.saturating_add(1);
            }
        }
        let mut v = m;
        st = st.saturating_add(14);
        loop {
            m >>= 1;
            if m == 0 {
                break;
            }
            if self.decode(input, Bin::Ac(table, st)) != 0 {
                v |= m;
            }
        }
        v += 1;
        Some(if sign != 0 { -v } else { v })
    }

    /// `decode_mcu`, the sequential one.
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "the coder's registers: A stays below 2^17, C holds at most 16 bits of interval and 16 of buffered data, and the bin, magnitude and position counters are all small: nothing here approaches the i64 or usize range"
    )]
    fn sequential(&mut self, input: &mut Input<'_>, header: &Header, blocks: &mut [[i16; 64]]) {
        if self.ct == -1 {
            return;
        }
        let scan = &header.scan;
        for blkn in 0..self.blocks_in_mcu {
            let position = self.positions.get(blkn).copied().unwrap_or(0);
            let ci = scan.comps.get(position).copied().unwrap_or(0);
            let Some(component) = header.components.get(ci) else {
                continue;
            };
            let (dc_table, ac_table) = (
                usize::from(component.dc_tbl_no),
                usize::from(component.ac_tbl_no),
            );
            let Some(diff) = self.dc_difference(input, header, position, dc_table) else {
                return;
            };
            if diff != 0 {
                if let Some(last) = self.last_dc.get_mut(position) {
                    *last = last.wrapping_add(diff) & 0xFFFF;
                }
            }
            let dc = self.last_dc.get(position).copied().unwrap_or(0);
            let Some(block) = blocks.get_mut(blkn) else {
                continue;
            };
            block[0] = dc as i16;
            // F.20: the AC coefficients.
            let mut k = 1usize;
            while k <= 63 {
                let mut st = 3 * (k - 1);
                if self.decode(input, Bin::Ac(ac_table, st)) != 0 {
                    break;
                }
                while self.decode(input, Bin::Ac(ac_table, st.saturating_add(1))) == 0 {
                    st = st.saturating_add(3);
                    k += 1;
                    if k > 63 {
                        input.warn();
                        self.ct = -1;
                        return;
                    }
                }
                let Some(v) = self.ac_value(input, header, ac_table, k, st) else {
                    return;
                };
                if let Some(cell) = block.get_mut(natural(k)) {
                    *cell = v as i16;
                }
                k += 1;
            }
        }
    }

    /// `decode_mcu_DC_first`.
    fn dc_first(&mut self, input: &mut Input<'_>, header: &Header, blocks: &mut [[i16; 64]]) {
        if self.ct == -1 {
            return;
        }
        let scan = &header.scan;
        for blkn in 0..self.blocks_in_mcu {
            let position = self.positions.get(blkn).copied().unwrap_or(0);
            let ci = scan.comps.get(position).copied().unwrap_or(0);
            let table = header
                .components
                .get(ci)
                .map_or(0, |c| usize::from(c.dc_tbl_no));
            let Some(diff) = self.dc_difference(input, header, position, table) else {
                return;
            };
            if diff != 0 {
                if let Some(last) = self.last_dc.get_mut(position) {
                    *last = last.wrapping_add(diff) & 0xFFFF;
                }
            }
            let dc = self.last_dc.get(position).copied().unwrap_or(0);
            if let Some(block) = blocks.get_mut(blkn) {
                block[0] = left_shift(dc, scan.al);
            }
        }
    }

    /// `decode_mcu_AC_first`.
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "the coder's registers: A stays below 2^17, C holds at most 16 bits of interval and 16 of buffered data, and the bin, magnitude and position counters are all small: nothing here approaches the i64 or usize range"
    )]
    fn ac_first<B: Coefficients>(&mut self, input: &mut Input<'_>, header: &Header, block: &mut B) {
        if self.ct == -1 {
            return;
        }
        let scan = &header.scan;
        let ci = scan.comps.first().copied().unwrap_or(0);
        let table = header
            .components
            .get(ci)
            .map_or(0, |c| usize::from(c.ac_tbl_no));
        let se = usize::from(scan.se);
        let mut k = usize::from(scan.ss);
        while k <= se {
            let mut st = 3usize.wrapping_mul(k.wrapping_sub(1));
            if self.decode(input, Bin::Ac(table, st)) != 0 {
                break;
            }
            while self.decode(input, Bin::Ac(table, st.wrapping_add(1))) == 0 {
                st = st.wrapping_add(3);
                k += 1;
                if k > se {
                    input.warn();
                    self.ct = -1;
                    return;
                }
            }
            let Some(v) = self.ac_value(input, header, table, k, st) else {
                return;
            };
            block.set(natural(k), ((v as u32) << scan.al) as i16);
            k += 1;
        }
    }

    /// `decode_mcu_DC_refine`.
    fn dc_refine(&mut self, input: &mut Input<'_>, header: &Header, blocks: &mut [[i16; 64]]) {
        let p1 = 1i32 << header.scan.al;
        for block in blocks.iter_mut().take(self.blocks_in_mcu) {
            if self.decode(input, Bin::Fixed) != 0 {
                block[0] = (i32::from(block[0]) | p1) as i16;
            }
        }
    }

    /// `decode_mcu_AC_refine`.
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "the coder's registers: A stays below 2^17, C holds at most 16 bits of interval and 16 of buffered data, and the bin, magnitude and position counters are all small: nothing here approaches the i64 or usize range"
    )]
    fn ac_refine<B: Coefficients>(
        &mut self,
        input: &mut Input<'_>,
        header: &Header,
        block: &mut B,
    ) {
        if self.ct == -1 {
            return;
        }
        let scan = &header.scan;
        let ci = scan.comps.first().copied().unwrap_or(0);
        let table = header
            .components
            .get(ci)
            .map_or(0, |c| usize::from(c.ac_tbl_no));
        let p1 = 1i32 << scan.al;
        let m1 = -1i32 << scan.al;
        let se = usize::from(scan.se);
        // The previous stage's end of block.
        let mut kex = se;
        while kex > 0 && block.at(natural(kex)) == 0 {
            kex -= 1;
        }
        let mut k = usize::from(scan.ss);
        while k <= se {
            let mut st = 3usize.wrapping_mul(k.wrapping_sub(1));
            if k > kex && self.decode(input, Bin::Ac(table, st)) != 0 {
                break;
            }
            loop {
                let at = natural(k);
                let value = block.at(at);
                if value != 0 {
                    if self.decode(input, Bin::Ac(table, st.wrapping_add(2))) != 0 {
                        let refined = if value < 0 {
                            value.wrapping_add(m1 as i16)
                        } else {
                            value.wrapping_add(p1 as i16)
                        };
                        block.set(at, refined);
                    }
                    break;
                }
                if self.decode(input, Bin::Ac(table, st.wrapping_add(1))) != 0 {
                    let negative = self.decode(input, Bin::Fixed) != 0;
                    block.set(at, if negative { m1 as i16 } else { p1 as i16 });
                    break;
                }
                st = st.wrapping_add(3);
                k += 1;
                if k > se {
                    input.warn();
                    self.ct = -1;
                    return;
                }
            }
            k += 1;
        }
    }
}
