//! Lossless JPEG (`SOF3`): libjpeg-turbo's lossless decompressor --
//! `jdlhuff.c` (its Huffman decoder), `jddiffct.c` (the difference buffer
//! controller) and `jdlossls.c` (prediction, undifferencing and the point
//! transform).
//!
//! A lossless JPEG codes each sample as its difference from a prediction made
//! from its neighbours already decoded -- the sample to its left (`Ra`), the
//! one above (`Rb`) and the one above-left (`Rc`) -- by one of seven
//! predictors the scan names (`Ss`, the predictor selection value), and the
//! decoder adds the prediction back. A scan's first row, and the first row of
//! each restart interval, has no row above: it is predicted from the left, its
//! first sample from the middle of the range. The samples may have been
//! shifted right before coding (the point transform, `Al`), and are shifted
//! back.
//!
//! libjpeg-turbo decodes an iMCU row's differences whole and only then
//! undifferences its rows, choosing the predictor for each row by a state it
//! resets -- at the start of a scan, at a restart marker, and whenever it has
//! run out of data -- for every component at once. So where a restart falls
//! inside an iMCU row (a component taller than one sample per MCU, in a scan
//! of its own), or the data runs out partway through one, the reset lands on
//! the iMCU row's first row rather than the row it happened at, and the rows
//! after it are predicted from the row above after all. That is kept, like
//! everything else in this port: what is wanted is the pixels libjpeg-turbo
//! gives, which for a file its own encoder wrote are the ones that went in.
//!
//! Past the end of its data a scan decodes as zero differences from a reset
//! predictor: the middle of the range -- grey -- for the rest of the scan, as
//! the lossy decoders give.
//!
//! Only the 8-bit interface is here, as for lossy JPEG: samples of 2 to 8
//! bits, handed out as they are, so a 6-bit image's samples run from 0 to 63,
//! as in every program built on libjpeg.

use alloc::vec;
use alloc::vec::Vec;

use super::error::{Error, jerr};
use super::huffman::{Bits, Derived, derive, extend};
use super::marker::{Component, Header, Input, read_restart_marker};
use super::tables::Tables;
use super::upsample::Samples;

/// The most samples an MCU holds (`D_MAX_BLOCKS_IN_MCU`).
const MAX_SAMPLES: usize = 10;

/// One scan's Huffman decoder (`lhuff_entropy_decoder`).
pub(super) struct Entropy {
    /// The scan's derived tables, by table number.
    tables: [Option<Derived>; 4],
    /// For each sample of an MCU: which of [`Self::pointers`] it is written
    /// through, and its table.
    samples: [(usize, u8); MAX_SAMPLES],
    blocks_in_mcu: usize,
    /// The MCU's rows of samples, one per row of each component in it: the
    /// component's frame index, the row within the MCU, and how many samples
    /// of the component an MCU is across (`output_ptr_info`).
    pointers: [(usize, usize, usize); MAX_SAMPLES],
    num_pointers: usize,
    pub(super) bits: Bits,
}

impl Entropy {
    /// `start_pass_lhuff_decoder`: derive the scan's tables and lay out where
    /// each sample of an MCU goes. `membership` is `MCU_membership`: for each
    /// sample of an MCU, its component's position in the scan.
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "counts bounded by MAX_SAMPLES: per_scan_setup refuses an MCU of more than ten samples"
    )]
    pub(super) fn start(
        header: &Header,
        tables: &Tables,
        membership: &[usize],
    ) -> Result<Self, Error> {
        let scan = &header.scan;
        let mut derived: [Option<Derived>; 4] = Default::default();
        for &ci in scan.components() {
            let component = header.components.get(ci).ok_or(jerr::BAD_COMPONENT_ID)?;
            let table = derive(tables, true, component.dc_tbl_no, true)?;
            if let Some(slot) = derived.get_mut(usize::from(component.dc_tbl_no)) {
                *slot = Some(table);
            }
        }
        let mut samples = [(0usize, 0u8); MAX_SAMPLES];
        let mut pointers = [(0usize, 0usize, 0usize); MAX_SAMPLES];
        let (mut sampn, mut ptrn) = (0usize, 0usize);
        let blocks_in_mcu = membership.len().min(MAX_SAMPLES);
        while sampn < blocks_in_mcu {
            let Some(component) = membership
                .get(sampn)
                .and_then(|&position| scan.comps.get(position))
                .and_then(|&ci| header.components.get(ci).map(|c| (ci, c)))
            else {
                break;
            };
            let (ci, component) = component;
            for yoffset in 0..component.mcu_height {
                if let Some(slot) = pointers.get_mut(ptrn) {
                    *slot = (ci, yoffset, component.mcu_width);
                }
                for _ in 0..component.mcu_width {
                    if let Some(slot) = samples.get_mut(sampn) {
                        *slot = (ptrn, component.dc_tbl_no);
                    }
                    sampn += 1;
                }
                ptrn += 1;
            }
        }
        Ok(Self {
            tables: derived,
            samples,
            blocks_in_mcu,
            pointers,
            num_pointers: ptrn.min(MAX_SAMPLES),
            bits: Bits::default(),
        })
    }

    /// `process_restart`: drop what is buffered, read the restart marker, and
    /// clear the out-of-data flag unless the marker read was not one.
    fn restart(&mut self, input: &mut Input<'_>) {
        self.bits.reset();
        read_restart_marker(input);
        if input.unread_marker == 0 {
            self.bits.insufficient = false;
        }
    }

    /// `decode_mcus`: a whole row of `count` MCUs, row `mcu_row` of the iMCU
    /// row, into the difference rows of `ctl`. Once the data has run out it
    /// decodes nothing: the row's differences are zeroed and every component's
    /// predictor reset, so what follows is the middle of the range.
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "positions within a row of at most 65500 samples rounded up to whole MCUs"
    )]
    fn decode_mcus(
        &mut self,
        input: &mut Input<'_>,
        ctl: &mut Controller,
        mcu_row: usize,
        count: usize,
    ) {
        // Where each pointer starts: its row, from the first MCU of the row.
        let mut at = [0usize; MAX_SAMPLES];
        for (slot, &(ci, yoffset, _)) in at.iter_mut().zip(&self.pointers).take(self.num_pointers) {
            *slot = (mcu_row + yoffset) * ctl.stride.get(ci).copied().unwrap_or(0);
        }
        if self.bits.insufficient {
            for (&start, &(ci, _, mcu_width)) in
                at.iter().zip(&self.pointers).take(self.num_pointers)
            {
                if let Some(row) = ctl.diff.get_mut(ci) {
                    let end = start
                        .saturating_add(count.saturating_mul(mcu_width))
                        .min(row.len());
                    if let Some(cells) = row.get_mut(start..end) {
                        cells.fill(0);
                    }
                }
            }
            // `start_pass_lossless`, whose check the scan passed when it began.
            ctl.first_row.fill(true);
            return;
        }
        for _ in 0..count {
            for &(pointer, table) in self.samples.iter().take(self.blocks_in_mcu) {
                let Some(Some(table)) = self.tables.get(usize::from(table)) else {
                    continue;
                };
                // H.2.2: the difference's size in bits, then its bits; a size
                // of 16 has none and is always 32768.
                let mut s = self.bits.decode(input, table);
                if s != 0 {
                    s = if s == 16 {
                        32768
                    } else {
                        let r = self.bits.get(input, s);
                        extend(r, s)
                    };
                }
                let (Some(&(ci, _, _)), Some(position)) =
                    (self.pointers.get(pointer), at.get_mut(pointer))
                else {
                    continue;
                };
                if let Some(cell) = ctl.diff.get_mut(ci).and_then(|row| row.get_mut(*position)) {
                    *cell = s;
                }
                *position += 1;
            }
        }
    }
}

/// The difference buffers and restart count of `jddiffct.c`, and the choice
/// of undifferencer for each component `jdlossls.c` keeps.
pub(super) struct Controller {
    /// For each component of the frame: `v` rows of differences, each
    /// `stride` long.
    diff: Vec<Vec<i32>>,
    /// The same rows undifferenced: samples before the point transform is
    /// undone, modulo 2^16.
    undiff: Vec<Vec<i32>>,
    /// For each component: the length of its rows, its width in samples
    /// rounded up to whole MCUs.
    stride: Vec<usize>,
    /// For each component: whether its next row is undifferenced as a first
    /// row (`jpeg_undifference_first_row`) rather than by the scan's
    /// predictor.
    first_row: Vec<bool>,
    /// MCU rows left before the next restart marker.
    restart_rows_to_go: usize,
}

impl Controller {
    /// `jinit_d_diff_controller`'s buffers, for the frame's components.
    pub(super) fn new(components: &[Component]) -> Self {
        let stride: Vec<usize> = components.iter().map(row_length).collect();
        let rows = |c: &Component, stride: usize| vec![0i32; stride.saturating_mul(c.v)];
        Self {
            diff: components
                .iter()
                .zip(&stride)
                .map(|(c, &s)| rows(c, s))
                .collect(),
            undiff: components
                .iter()
                .zip(&stride)
                .map(|(c, &s)| rows(c, s))
                .collect(),
            stride,
            first_row: vec![true; components.len()],
            restart_rows_to_go: 0,
        }
    }

    /// What [`Self::new`] allocates, in bytes.
    pub(super) fn bytes(components: &[Component]) -> u64 {
        components
            .iter()
            .map(|c| {
                // Two buffers of four-byte samples.
                (row_length(c) as u64)
                    .saturating_mul(c.v as u64)
                    .saturating_mul(8)
            })
            .fold(0u64, u64::saturating_add)
    }

    /// `start_pass_lossless`: check the scan's parameters -- a predictor from
    /// 1 to 7, `Se` and `Ah` zero, a point transform less than the precision
    /// -- and make every component's next row a first row.
    fn start_pass(&mut self, header: &Header) -> Result<(), Error> {
        let scan = &header.scan;
        if !(1..=7).contains(&scan.ss)
            || scan.se != 0
            || scan.ah != 0
            || scan.al >= header.precision
        {
            return Err(jerr::BAD_PROGRESSION);
        }
        self.first_row.fill(true);
        Ok(())
    }

    /// The difference controller's `start_input_pass`: [`Self::start_pass`],
    /// then the restart interval, which must be a whole number of MCU rows.
    pub(super) fn start_input_pass(
        &mut self,
        header: &Header,
        mcus_per_row: usize,
    ) -> Result<(), Error> {
        self.start_pass(header)?;
        let interval = usize::from(header.restart_interval);
        let per_row = mcus_per_row.max(1);
        if !interval.is_multiple_of(per_row) {
            return Err(jerr::BAD_RESTART);
        }
        self.restart_rows_to_go = interval.checked_div(per_row).unwrap_or(0);
        Ok(())
    }

    /// `jpeg_undifference_first_row` or the scan's `jpeg_undifferenceN`: row
    /// `row` of component `ci` from its differences and the samples of row
    /// `prev` -- which is `row` itself for a component one row tall, so each
    /// sample above is read before the sample in its place is written, as the
    /// C does.
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "positions within a component's rows; samples below 2^16 and differences within 2^15 in i32, predictors in i64"
    )]
    fn undifference(
        &mut self,
        ci: usize,
        row: usize,
        prev: usize,
        width: usize,
        psv: u8,
        initial: i32,
    ) {
        let stride = self.stride.get(ci).copied().unwrap_or(0);
        let (Some(diff), Some(undiff), Some(first)) = (
            self.diff.get(ci),
            self.undiff.get_mut(ci),
            self.first_row.get_mut(ci),
        ) else {
            return;
        };
        let width = width.min(stride);
        let (here, above) = (row * stride, prev * stride);
        let Some(diff) = diff.get(here..here + width) else {
            return;
        };
        if undiff.len() < here.max(above) + width {
            return;
        }
        let at = |buffer: &[i32], i: usize| buffer.get(i).copied().unwrap_or(0);
        if *first || psv == 1 {
            // The 1-D horizontal predictor: the first sample from the middle
            // of the range in a first row, from the one above in any other.
            let mut ra = if *first { initial } else { at(undiff, above) };
            for (x, &d) in diff.iter().enumerate() {
                ra = (d + ra) & 0xFFFF;
                if let Some(slot) = undiff.get_mut(here + x) {
                    *slot = ra;
                }
            }
            *first = false;
            return;
        }
        // The 2-D predictors: the first sample from the one above.
        let mut rb = at(undiff, above);
        let mut ra = (diff.first().copied().unwrap_or(0) + rb) & 0xFFFF;
        if let Some(slot) = undiff.get_mut(here) {
            *slot = ra;
        }
        for (x, &d) in diff.iter().enumerate().skip(1) {
            let rc = rb;
            rb = at(undiff, above + x);
            ra = (d + predict(psv, ra, rb, rc)) & 0xFFFF;
            if let Some(slot) = undiff.get_mut(here + x) {
                *slot = ra;
            }
        }
    }
}

/// `PREDICTOR2` to `PREDICTOR7` (1 is `Ra`, and handled as the 1-D case):
/// the arithmetic in C's `JLONG`, the halving an arithmetic right shift, the
/// result cast back to `int`.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "operands below 2^16, so no sum or difference comes near i64's range"
)]
fn predict(psv: u8, ra: i32, rb: i32, rc: i32) -> i32 {
    let (a, b, c) = (i64::from(ra), i64::from(rb), i64::from(rc));
    let p = match psv {
        2 => b,
        3 => c,
        4 => a + b - c,
        5 => a + ((b - c) >> 1),
        6 => b + ((a - c) >> 1),
        7 => (a + b) >> 1,
        _ => a,
    };
    // Between -65535 and 131070: always an i32.
    i32::try_from(p).unwrap_or(0)
}

/// A component's row length in the difference buffers: its width in samples
/// rounded up to whole MCUs (`jround_up(width_in_blocks, h_samp_factor)`).
fn row_length(component: &Component) -> usize {
    component
        .width_in_blocks
        .next_multiple_of(component.h.max(1))
}

/// Where the input side stands in the scan: the iMCU row being decoded,
/// whether it is the image's last, and how many MCUs make it.
pub(super) struct Position {
    pub(super) imcu_row: usize,
    pub(super) last: bool,
    pub(super) mcus_per_row: usize,
    /// MCU rows in this iMCU row (`MCU_rows_per_iMCU_row`).
    pub(super) mcu_rows: usize,
}

/// The difference controller's `decompress_data`: decode one iMCU row of the
/// scan -- restart markers included -- then undifference each row of each of
/// its components and undo the point transform into the component's plane.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "row arithmetic within planes of at most 65500 rows; the precision check leaves a shift below 16"
)]
pub(super) fn decompress_row(
    input: &mut Input<'_>,
    header: &Header,
    entropy: &mut Entropy,
    ctl: &mut Controller,
    at: &Position,
    planes: &mut [Samples],
) {
    let interval = usize::from(header.restart_interval);
    let per_row = at.mcus_per_row.max(1);
    for yoffset in 0..at.mcu_rows {
        if interval != 0 && ctl.restart_rows_to_go == 0 {
            entropy.restart(input);
            // `start_pass_lossless`, whose check the scan passed when it began.
            ctl.first_row.fill(true);
            ctl.restart_rows_to_go = interval / per_row;
        }
        entropy.decode_mcus(input, ctl, yoffset, at.mcus_per_row);
        if interval != 0 {
            ctl.restart_rows_to_go = ctl.restart_rows_to_go.saturating_sub(1);
        }
    }
    let scan = &header.scan;
    let al = u32::from(scan.al);
    // `INITIAL_PREDICTORx`: the middle of the range the scan codes. The
    // precision is at least 2 and the point transform below it.
    let initial = 1i32
        << u32::from(header.precision)
            .saturating_sub(al)
            .saturating_sub(1)
            .min(15);
    for &ci in scan.components() {
        let (Some(component), Some(plane)) = (header.components.get(ci), planes.get_mut(ci)) else {
            continue;
        };
        let v = component.v.max(1);
        let rows = if at.last {
            component.last_row_height
        } else {
            v
        };
        plane.grow_to((at.imcu_row + 1) * v);
        let stride = ctl.stride.get(ci).copied().unwrap_or(0);
        let width = component.width_in_blocks;
        let mut prev = v - 1;
        for row in 0..rows.min(v) {
            ctl.undifference(ci, row, prev, width, scan.ss, initial);
            // `simple_upscale` or `noscale`: back up by the point transform,
            // kept to the sample's byte.
            let out_row = at.imcu_row * v + row;
            let (Some(samples), Some(out)) = (
                ctl.undiff
                    .get(ci)
                    .and_then(|u| u.get(row * stride..row * stride + width)),
                plane.rows_mut(out_row, 1),
            ) else {
                prev = row;
                continue;
            };
            for (slot, &sample) in out.iter_mut().zip(samples) {
                // The C's cast to `JSAMPLE`: the low byte, whatever a corrupt
                // file made the sample.
                *slot = u8::try_from((sample << al) & 0xFF).unwrap_or(0);
            }
            prev = row;
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
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss
)]
mod tests {
    use alloc::vec::Vec;

    use super::*;

    /// A component `width` samples across and `v` rows tall per iMCU row.
    fn component(width: usize, h: usize, v: usize) -> Component {
        Component {
            h,
            v,
            width_in_blocks: width,
            ..Component::default()
        }
    }

    /// A small xorshift, for differences that are not a pattern.
    fn noise(seed: &mut u32) -> u32 {
        *seed ^= *seed << 13;
        *seed ^= *seed >> 17;
        *seed ^= *seed << 5;
        *seed
    }

    #[test]
    fn the_predictors_are_the_standard_s_with_c_s_rounding() {
        // Table H.1, with the halvings an arithmetic shift: -3 >> 1 is -2.
        let (ra, rb, rc) = (10, 3, 6);
        assert_eq!(predict(2, ra, rb, rc), 3);
        assert_eq!(predict(3, ra, rb, rc), 6);
        assert_eq!(predict(4, ra, rb, rc), 7);
        assert_eq!(predict(5, ra, rb, rc), 8);
        assert_eq!(predict(6, ra, rb, rc), 5);
        assert_eq!(predict(7, ra, rb, rc), 6);
        // And out of range both ways, as a corrupt file makes them.
        assert_eq!(predict(4, 0, 0, 65535), -65535);
        assert_eq!(predict(4, 65535, 65535, 0), 131_070);
    }

    #[test]
    fn a_first_row_of_zero_differences_is_the_middle_of_the_range() {
        let mut ctl = Controller::new(&[component(5, 1, 1)]);
        ctl.undifference(0, 0, 0, 5, 4, 1 << 7);
        assert_eq!(&ctl.undiff[0][..5], &[128; 5]);
        assert!(
            !ctl.first_row[0],
            "the next row is predicted by the scan's predictor"
        );
    }

    #[test]
    fn a_row_undifferenced_in_place_is_the_row_undifferenced_from_a_copy() {
        // A component one row tall reads the row above from the very buffer it
        // writes; each sample above must be read before it is overwritten.
        let width = 37;
        let mut seed = 0x1234_5678;
        for psv in 1..=7u8 {
            let mut single = Controller::new(&[component(width, 1, 1)]);
            let mut double = Controller::new(&[component(width, 1, 2)]);
            for row in 0..6 {
                let diffs: Vec<i32> = (0..width)
                    .map(|_| (noise(&mut seed) % 511) as i32 - 255)
                    .collect();
                single.diff[0][..width].copy_from_slice(&diffs);
                single.undifference(0, 0, 0, width, psv, 128);
                // The same, with the rows in two separate halves of a buffer.
                let (here, above) = (row % 2, (row + 1) % 2);
                double.diff[0][here * width..(here + 1) * width].copy_from_slice(&diffs);
                double.undifference(0, here, above, width, psv, 128);
                assert_eq!(
                    &single.undiff[0][..width],
                    &double.undiff[0][here * width..(here + 1) * width],
                    "predictor {psv}, row {row}"
                );
            }
        }
    }

    #[test]
    fn samples_wrap_at_sixteen_bits() {
        let mut ctl = Controller::new(&[component(3, 1, 1)]);
        ctl.diff[0][..3].copy_from_slice(&[32768, 32768, -1]);
        ctl.undifference(0, 0, 0, 3, 1, 128);
        assert_eq!(&ctl.undiff[0][..3], &[32896, 128, 127]);
    }

    #[test]
    fn a_row_is_as_long_as_its_mcus() {
        assert_eq!(row_length(&component(5, 2, 1)), 6);
        assert_eq!(row_length(&component(6, 2, 1)), 6);
        assert_eq!(row_length(&component(5, 1, 1)), 5);
        assert_eq!(Controller::bytes(&[component(5, 2, 2)]), 6 * 2 * 8);
    }

    #[test]
    fn a_scan_is_checked_as_lossless_scans_are() {
        let mut header = Header {
            precision: 8,
            ..Header::default()
        };
        header.scan.ss = 1;
        let mut ctl = Controller::new(&[component(4, 1, 1)]);
        assert!(ctl.start_input_pass(&header, 4).is_ok());
        for (ss, se, ah, al) in [
            (0, 0, 0, 0),
            (8, 0, 0, 0),
            (1, 1, 0, 0),
            (1, 0, 1, 0),
            (1, 0, 0, 8),
        ] {
            header.scan.ss = ss;
            header.scan.se = se;
            header.scan.ah = ah;
            header.scan.al = al;
            assert_eq!(
                ctl.start_input_pass(&header, 4),
                Err(jerr::BAD_PROGRESSION),
                "Ss {ss} Se {se} Ah {ah} Al {al}"
            );
        }
        header.scan.ss = 7;
        header.scan.se = 0;
        header.scan.ah = 0;
        header.scan.al = 7;
        assert!(ctl.start_input_pass(&header, 4).is_ok());
        // A restart interval must be a whole number of MCU rows.
        header.restart_interval = 6;
        assert_eq!(ctl.start_input_pass(&header, 4), Err(jerr::BAD_RESTART));
        header.restart_interval = 8;
        assert!(ctl.start_input_pass(&header, 4).is_ok());
        assert_eq!(ctl.restart_rows_to_go, 2);
    }
}
