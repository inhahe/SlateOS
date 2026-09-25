//! Progressive JPEG (`SOF2`): the picture in several passes.
//!
//! A baseline file sends each 8x8 block once, whole. A progressive one sends
//! the whole picture several times over, each pass adding either **more of the
//! frequencies** — the first pass is often the DC alone, an eighth-scale
//! mosaic — or **more precision** to ones already sent, a bit at a time. That
//! is how a web browser can show a blurry photograph at once and sharpen it as
//! it arrives, and it is why a large share of the photographs on the web, and
//! many a camera's export, are progressive.
//!
//! So a progressive decode cannot turn a block into pixels as it goes. Every
//! coefficient of every block is accumulated across the scans in
//! [`Coefficients`], and the picture is reconstructed once they are all in —
//! through the same dequantise, inverse-DCT and upsampling path as baseline,
//! so the two cannot disagree about anything but the order the numbers came
//! in. A progressive file and a baseline file carrying the same coefficients
//! decode to the same pixels, bit for bit, and a test holds them to it.
//!
//! # The four kinds of scan (ITU T.81, Annex G)
//!
//! Each scan names a band of coefficients `Ss..=Se` in zig-zag order and a
//! bit position: `Ah` (the precision already sent; zero for a first pass) and
//! `Al` (the bit this pass sends down to).
//!
//! | scan | `Ss` | `Ah` | carries |
//! |---|---|---|---|
//! | DC first | 0 | 0 | each block's DC, Huffman-coded as a difference, shifted up by `Al` |
//! | DC refinement | 0 | >0 | one more bit of each DC, raw |
//! | AC first | ≥1 | 0 | the band shifted up by `Al`, with *end-of-band runs* covering many empty blocks at once |
//! | AC refinement | ≥1 | >0 | one more bit of every coefficient already non-zero, and the band's newly non-zero ones |
//!
//! The last is the only intricate one. It is transcribed from the standard's
//! procedure and cross-read against libjpeg's `decode_mcu_AC_refine` and
//! stb_image's, rather than reinvented.
//!
//! # Memory, and thumbnails
//!
//! The coefficients have to be held for the whole picture: for a 21-megapixel
//! 4:2:0 photograph that is some 64 MB at 64 coefficients a block. A scaled
//! decode — a thumbnail — needs fewer: its reduced transform gives no weight
//! at all to some frequencies (`jpeg::idct_scaled`, `jpeg::kept_frequency`),
//! so only the others are kept — 7 a side at half scale, 5 at a quarter, the
//! DC alone at an eighth ([`Kept`]). The rest are still parsed, since a bit
//! stream cannot be skipped, and remembered only as whether they are non-zero
//! — one bit each, which is all a refinement pass needs to read the stream
//! correctly. An eighth-scale thumbnail keeps ten bytes a luma block instead
//! of 136; a 4:2:0 file's chroma, which libjpeg reconstructs at twice the
//! picture's block size (`jpeg::component_block`), keeps the 25 coefficients
//! its 2x2 transform reads.
//!
//! # Hostile input
//!
//! As for baseline: every table and component index is checked, the
//! coefficient store is bounded by [`Limits`] before it is allocated, a scan's
//! parameters are checked against what the standard allows, and a file that
//! stops early — mid-scan, or before its last scan — is reconstructed from what
//! arrived: a softer picture rather than a failure.

use alloc::vec;
use alloc::vec::Vec;

use super::{
    Basis, BitReader, Component, Samples, Shape, Tables, ZIGZAG, component_block, flat_block,
    idct_8x8, idct_scaled, kept_frequency, scale_block, store_block, to_pixels,
};
use crate::{Image, ImageError, ImageResult, Limits};

/// One component's coefficients.
struct Plane {
    /// Blocks across and down, padded out to whole MCUs: the grid an
    /// interleaved scan visits and reconstruction writes.
    blocks_w: usize,
    blocks_h: usize,
    /// Blocks across and down that cover the component's own pixels: the grid
    /// a *non-interleaved* scan visits, which is not padded to MCUs.
    scan_w: usize,
    scan_h: usize,
    /// How many samples a side each block is reconstructed to: the picture's
    /// block size, or twice it for chroma a scaled decode reconstructs at the
    /// output's resolution.
    size: usize,
    /// Which coefficients that reconstruction reads, and so which are kept.
    kept: Kept,
    /// `kept.per_block()` coefficients per block, in natural order among the
    /// kept ones, already shifted into place by each scan's `Al`.
    values: Vec<i16>,
    /// Per block, one bit per zig-zag position: whether that coefficient is
    /// non-zero. Kept for all 64 whether or not the value is, because a
    /// refinement scan reads a correction bit for exactly the non-zero ones.
    nonzero: Vec<u64>,
    /// The quantisation table, latched when the component's first scan began:
    /// a `DQT` between scans may redefine the table *number*, and what was
    /// already sent was quantised with the old one.
    quant: Option<[u16; 64]>,
}

/// Every coefficient of a progressive picture, accumulated across its scans.
pub(super) struct Coefficients {
    width: usize,
    height: usize,
    /// The frame's components, in frame order.
    components: Vec<Component>,
    planes: Vec<Plane>,
    /// The largest sampling factors, which set the MCU.
    max_h: usize,
    max_v: usize,
    mcus_x: usize,
    mcus_y: usize,
    /// How many pixels each of the picture's blocks becomes: 8 for a full
    /// decode, 4, 2 or 1 for a scaled one.
    block: usize,
    /// Whether any scan has been read. A file cut off before its first scan
    /// has no picture to reconstruct.
    scanned: bool,
}

/// One scan's parameters, from its `SOS` header.
pub(super) struct Scan {
    /// The components it carries, as indices into the frame's list, with the
    /// Huffman tables the header chose for each and their DC predictors.
    components: Vec<(usize, Component)>,
    /// First and last zig-zag position of the band.
    ss: usize,
    se: usize,
    /// Successive approximation: the bit already sent, and the bit sent now.
    ah: u32,
    al: u32,
}

/// The restart interval's bookkeeping for one scan.
struct Restarts {
    interval: usize,
    since: usize,
}

impl Restarts {
    /// Before each MCU — or, in a scan of one component, each block: at the
    /// interval, step past the restart marker. `true` if one was consumed, in
    /// which case the DC predictors and the end-of-band run start again.
    fn before_unit(&mut self, bits: &mut BitReader<'_>) -> bool {
        let mut restarted = false;
        if self.interval > 0 && self.since == self.interval {
            restarted = bits.restart();
            self.since = 0;
        }
        self.since = self.since.saturating_add(1);
        restarted
    }
}

/// Which of a block's coefficients a plane keeps: those its reconstruction
/// gives a weight to, which are the frequencies [`kept_frequency`] names in
/// each direction.
#[derive(Clone, Copy)]
struct Kept {
    /// How many frequencies a side: 8, 7, 5 or 1 for blocks reconstructed to
    /// 8, 4, 2 or 1 samples.
    side: usize,
    /// Each frequency's place among the kept ones, or `None` if it is not.
    rank: [Option<u8>; 8],
}

impl Kept {
    /// What a block reconstructed to `size` samples a side reads.
    fn for_size(size: usize) -> Self {
        let mut rank = [None; 8];
        let mut side = 0usize;
        for (k, slot) in rank.iter_mut().enumerate() {
            if kept_frequency(k, size) {
                *slot = u8::try_from(side).ok();
                side = side.saturating_add(1);
            }
        }
        Self { side, rank }
    }

    /// Coefficients kept per block.
    const fn per_block(&self) -> usize {
        self.side.saturating_mul(self.side)
    }

    /// Where the coefficient at natural (row-major) position `natural` lives
    /// among a block's kept ones, or `None` if it is not kept.
    fn slot(&self, natural: usize) -> Option<usize> {
        let u = usize::from(self.rank.get(natural % 8).copied().flatten()?);
        let v = usize::from(self.rank.get(natural / 8).copied().flatten()?);
        Some(v.saturating_mul(self.side).saturating_add(u))
    }

    /// The same for zig-zag position `k`.
    fn zigzag_slot(&self, k: usize) -> Option<usize> {
        self.slot(*ZIGZAG.get(k)?)
    }
}

/// `1 << bit` as the `i32` the arithmetic below uses, saturated for a bit no
/// validated scan can name.
fn bit_value(bit: u32) -> i32 {
    1i32.checked_shl(bit).unwrap_or(i32::MAX)
}

impl Plane {
    /// The kept value at zig-zag position `k` of block `block`, or zero.
    fn value(&self, block: usize, k: usize) -> i32 {
        self.kept
            .zigzag_slot(k)
            .and_then(|slot| {
                self.values.get(
                    block
                        .saturating_mul(self.kept.per_block())
                        .saturating_add(slot),
                )
            })
            .map_or(0, |v| i32::from(*v))
    }

    /// Store `value` at zig-zag position `k` of block `block`, keeping the
    /// non-zero mask true whether or not the value itself is kept.
    fn set(&mut self, block: usize, k: usize, value: i32) {
        if let Some(mask) = self.nonzero.get_mut(block) {
            let bit = 1u64
                .checked_shl(u32::try_from(k).unwrap_or(64))
                .unwrap_or(0);
            if value == 0 {
                *mask &= !bit;
            } else {
                *mask |= bit;
            }
        }
        if let Some(slot) = self.kept.zigzag_slot(k) {
            let at = block
                .saturating_mul(self.kept.per_block())
                .saturating_add(slot);
            if let Some(cell) = self.values.get_mut(at) {
                // A coefficient of an 8-bit sample fits in twelve bits; a value
                // past `i16` is a corrupt stream, clamped rather than wrapped.
                *cell = i16::try_from(value.clamp(i32::from(i16::MIN), i32::from(i16::MAX)))
                    .unwrap_or(0);
            }
        }
    }

    /// Whether the coefficient at zig-zag position `k` of block `block` is
    /// non-zero — known for every position, kept or not.
    fn is_nonzero(&self, block: usize, k: usize) -> bool {
        let bit = 1u64
            .checked_shl(u32::try_from(k).unwrap_or(64))
            .unwrap_or(0);
        self.nonzero.get(block).is_some_and(|mask| mask & bit != 0)
    }
}

impl Coefficients {
    /// Room for every coefficient the frame will send, bounded by `limits`.
    ///
    /// `block` is the output size of each 8x8 block, as for a baseline decode:
    /// 8 for full size, 1, 2 or 4 for a scaled one.
    ///
    /// # Errors
    ///
    /// [`ImageError::TooLarge`] if the picture is past `limits.max_pixels`, or
    /// the coefficients and the samples reconstructed from them are past
    /// `limits.max_decompressed_bytes`.
    pub(super) fn new(
        width: usize,
        height: usize,
        components: Vec<Component>,
        limits: Limits,
        block: usize,
    ) -> ImageResult<Self> {
        let pixels_claimed = width.saturating_mul(height) as u64;
        if pixels_claimed > limits.max_pixels {
            return Err(ImageError::TooLarge {
                pixels: pixels_claimed,
                limit: limits.max_pixels,
            });
        }
        let block = scale_block(block);
        let max_h = components.iter().map(|c| c.h).max().unwrap_or(1);
        let max_v = components.iter().map(|c| c.v).max().unwrap_or(1);
        let mcus_x = width.div_ceil(max_h.saturating_mul(8));
        let mcus_y = height.div_ceil(max_v.saturating_mul(8));

        let mut total = 0usize;
        let mut planes = Vec::with_capacity(components.len());
        for component in &components {
            let size = component_block(block, (component.h, component.v), (max_h, max_v));
            let kept = Kept::for_size(size);
            // Per block: the kept coefficients at two bytes each, and the mask.
            let per_block = kept.per_block().saturating_mul(2).saturating_add(8);
            let blocks_w = mcus_x.saturating_mul(component.h);
            let blocks_h = mcus_y.saturating_mul(component.v);
            let blocks = blocks_w.saturating_mul(blocks_h);
            // Plus the samples reconstruction writes for this component, which
            // exist beside the coefficients for a while.
            let samples = blocks.saturating_mul(size).saturating_mul(size);
            total = total
                .saturating_add(blocks.saturating_mul(per_block))
                .saturating_add(samples);
            if total > limits.max_decompressed_bytes {
                return Err(ImageError::TooLarge {
                    pixels: total as u64,
                    limit: limits.max_decompressed_bytes as u64,
                });
            }
            // The component's own size, in pixels and then in blocks: what a
            // non-interleaved scan covers. `ceil(width * h / max_h)`.
            let comp_w = width.saturating_mul(component.h).div_ceil(max_h.max(1));
            let comp_h = height.saturating_mul(component.v).div_ceil(max_v.max(1));
            planes.push(Plane {
                blocks_w,
                blocks_h,
                scan_w: comp_w.div_ceil(8).min(blocks_w),
                scan_h: comp_h.div_ceil(8).min(blocks_h),
                size,
                kept,
                values: vec![0i16; blocks.saturating_mul(kept.per_block())],
                nonzero: vec![0u64; blocks],
                quant: None,
            });
        }
        Ok(Self {
            width,
            height,
            components,
            planes,
            max_h,
            max_v,
            mcus_x,
            mcus_y,
            block,
            scanned: false,
        })
    }

    /// Whether any scan has been read, so there is something to reconstruct.
    pub(super) const fn has_scans(&self) -> bool {
        self.scanned
    }

    /// Read one scan's header against this frame.
    ///
    /// # Errors
    ///
    /// [`ImageError::Malformed`] for a header the standard does not allow: an
    /// empty or unknown component list, a band out of order, a DC scan that
    /// also carries AC coefficients, an AC scan of more than one component, or
    /// successive approximation that is not one bit at a time.
    pub(super) fn read_scan(&self, payload: &[u8]) -> ImageResult<Scan> {
        let count = usize::from(*payload.first().ok_or(ImageError::Truncated)?);
        if count == 0 || count > self.components.len() {
            return Err(ImageError::Malformed("a scan naming no components"));
        }
        let mut components = Vec::with_capacity(count);
        for n in 0..count {
            let base = 1usize.saturating_add(n.saturating_mul(2));
            let id = *payload.get(base).ok_or(ImageError::Truncated)?;
            let spec = *payload
                .get(base.saturating_add(1))
                .ok_or(ImageError::Truncated)?;
            let index =
                self.components
                    .iter()
                    .position(|c| c.id == id)
                    .ok_or(ImageError::Malformed(
                        "a scan naming a component the frame lacks",
                    ))?;
            let mut component = *self.components.get(index).ok_or(ImageError::Malformed(
                "a scan naming a component the frame lacks",
            ))?;
            component.dc_table = usize::from(spec >> 4).min(3);
            component.ac_table = usize::from(spec & 0x0F).min(3);
            component.dc_prediction = 0;
            components.push((index, component));
        }
        let tail = 1usize.saturating_add(count.saturating_mul(2));
        let ss = usize::from(*payload.get(tail).ok_or(ImageError::Truncated)?);
        let se = usize::from(
            *payload
                .get(tail.saturating_add(1))
                .ok_or(ImageError::Truncated)?,
        );
        let approximation = *payload
            .get(tail.saturating_add(2))
            .ok_or(ImageError::Truncated)?;
        let (ah, al) = (
            u32::from(approximation >> 4),
            u32::from(approximation & 0x0F),
        );

        if ss > se || se > 63 {
            return Err(ImageError::Malformed("a scan whose band is out of order"));
        }
        if ss == 0 && se != 0 {
            return Err(ImageError::Malformed(
                "a progressive scan carrying DC and AC together",
            ));
        }
        if ss > 0 && count != 1 {
            return Err(ImageError::Malformed(
                "a progressive AC scan of more than one component",
            ));
        }
        // An 8-bit sample's coefficients fit in eleven bits and a sign, so no
        // pass can send a bit above the thirteenth; and a refinement pass sends
        // exactly the bit below the last one.
        if al > 13 || (ah != 0 && ah != al.saturating_add(1)) {
            return Err(ImageError::Malformed(
                "a scan's successive approximation is not one bit at a time",
            ));
        }
        Ok(Scan {
            components,
            ss,
            se,
            ah,
            al,
        })
    }

    /// Decode one scan's entropy-coded data into the coefficients, and return
    /// how many bytes of `data` it read: the next marker is at or after that.
    ///
    /// A scan that runs out of data stops where it is and keeps what it read:
    /// a truncated progressive file still has every earlier pass, and
    /// reconstructing from those is a softer picture rather than none.
    pub(super) fn decode_scan(&mut self, data: &[u8], scan: &mut Scan, tables: &Tables) -> usize {
        self.scanned = true;
        // Each component's table is latched by the first scan that carries it.
        for (index, _) in &scan.components {
            let number = self.components.get(*index).map_or(0, |c| c.quant);
            if let Some(plane) = self.planes.get_mut(*index)
                && plane.quant.is_none()
            {
                plane.quant = tables.quant.get(number).copied();
            }
        }
        let mut bits = BitReader::new(data);
        let mut restarts = Restarts {
            interval: tables.restart_interval,
            since: 0,
        };
        let mut eobrun = 0u32;
        let (ss, se, ah, al) = (scan.ss, scan.se, scan.ah, scan.al);

        if scan.components.len() > 1 {
            // Interleaved, which only a DC scan may be: whole MCUs, each
            // component's own blocks in turn.
            'mcus: for mcu_y in 0..self.mcus_y {
                for mcu_x in 0..self.mcus_x {
                    if restarts.before_unit(&mut bits) {
                        for (_, component) in &mut scan.components {
                            component.dc_prediction = 0;
                        }
                    }
                    for (index, component) in &mut scan.components {
                        let (h, v) = (component.h, component.v);
                        for by in 0..v {
                            for bx in 0..h {
                                let row = mcu_y.saturating_mul(v).saturating_add(by);
                                let col = mcu_x.saturating_mul(h).saturating_add(bx);
                                if !self.decode_dc(
                                    &mut bits,
                                    component,
                                    tables,
                                    (*index, row, col),
                                    (ah, al),
                                ) {
                                    break 'mcus;
                                }
                            }
                        }
                    }
                }
            }
        } else if let Some((index, component)) = scan.components.first_mut() {
            // One component: its own blocks in raster order, not padded out
            // to MCUs, each block an MCU for the restart interval.
            let index = *index;
            let (scan_w, scan_h) = self
                .planes
                .get(index)
                .map_or((0, 0), |p| (p.scan_w, p.scan_h));
            'blocks: for row in 0..scan_h {
                for col in 0..scan_w {
                    if restarts.before_unit(&mut bits) {
                        component.dc_prediction = 0;
                        eobrun = 0;
                    }
                    let ok = if ss == 0 {
                        self.decode_dc(&mut bits, component, tables, (index, row, col), (ah, al))
                    } else if ah == 0 {
                        self.decode_ac_first(
                            &mut bits,
                            component,
                            tables,
                            (index, row, col),
                            (ss, se, al),
                            &mut eobrun,
                        )
                    } else {
                        self.decode_ac_refine(
                            &mut bits,
                            component,
                            tables,
                            (index, row, col),
                            (ss, se, al),
                            &mut eobrun,
                        )
                    };
                    if !ok {
                        break 'blocks;
                    }
                }
            }
        }
        bits.position()
    }

    /// The block at `(row, col)` of component `index`, as an index into its
    /// plane, if the plane has one there.
    fn block_at(&self, index: usize, row: usize, col: usize) -> Option<usize> {
        let plane = self.planes.get(index)?;
        if row >= plane.blocks_h || col >= plane.blocks_w {
            return None;
        }
        Some(row.saturating_mul(plane.blocks_w).saturating_add(col))
    }

    /// A DC coefficient: a first pass (a Huffman-coded difference, shifted up
    /// by `al`) or a refinement (one raw bit). `false` when the data ran out.
    fn decode_dc(
        &mut self,
        bits: &mut BitReader<'_>,
        component: &mut Component,
        tables: &Tables,
        (index, row, col): (usize, usize, usize),
        (ah, al): (u32, u32),
    ) -> bool {
        let Some(block) = self.block_at(index, row, col) else {
            return true;
        };
        let Some(plane) = self.planes.get_mut(index) else {
            return true;
        };
        if ah == 0 {
            let Some(table) = tables.dc.get(component.dc_table) else {
                return false;
            };
            let Some(length) = table.decode(bits) else {
                return false;
            };
            let diff = bits.receive_extend(u32::from(length)).unwrap_or(0);
            component.dc_prediction = component.dc_prediction.saturating_add(diff);
            let value = component.dc_prediction.saturating_mul(bit_value(al));
            plane.set(block, 0, value);
        } else {
            let Some(bit) = bits.bit() else {
                return false;
            };
            if bit != 0 {
                let value = plane.value(block, 0).saturating_add(bit_value(al));
                plane.set(block, 0, value);
            }
        }
        true
    }

    /// A first pass over a band of AC coefficients. `false` when the data ran
    /// out.
    fn decode_ac_first(
        &mut self,
        bits: &mut BitReader<'_>,
        component: &Component,
        tables: &Tables,
        (index, row, col): (usize, usize, usize),
        (ss, se, al): (usize, usize, u32),
        eobrun: &mut u32,
    ) -> bool {
        // Inside a run of blocks with nothing in this band.
        if *eobrun > 0 {
            *eobrun = eobrun.saturating_sub(1);
            return true;
        }
        let Some(block) = self.block_at(index, row, col) else {
            return true;
        };
        let Some(table) = tables.ac.get(component.ac_table) else {
            return false;
        };
        let Some(plane) = self.planes.get_mut(index) else {
            return true;
        };
        let mut k = ss;
        while k <= se {
            let Some(symbol) = table.decode(bits) else {
                return false;
            };
            let run = u32::from(symbol >> 4);
            let size = u32::from(symbol & 0x0F);
            if size == 0 {
                if run < 15 {
                    // End of band here, and for `2^run - 1 + extra` blocks after
                    // this one.
                    let extra = if run > 0 {
                        u32::try_from(bits.bits(run).unwrap_or(0)).unwrap_or(0)
                    } else {
                        0
                    };
                    *eobrun = (1u32 << run).saturating_sub(1).saturating_add(extra);
                    break;
                }
                // Sixteen zeros, and the band goes on.
                k = k.saturating_add(16);
                continue;
            }
            k = k.saturating_add(usize::try_from(run).unwrap_or(64));
            if k > se {
                break;
            }
            let value = bits.receive_extend(size).unwrap_or(0);
            plane.set(block, k, value.saturating_mul(bit_value(al)));
            k = k.saturating_add(1);
        }
        true
    }

    /// A refinement pass over a band of AC coefficients: one more bit of every
    /// coefficient already non-zero, and the band's newly non-zero ones.
    /// `false` when the data ran out.
    ///
    /// The standard's procedure (T.81 G.1.2.3), in the shape libjpeg and
    /// stb_image both give it. The subtle part: a run of zeros counts only the
    /// coefficients that are *still* zero, and every non-zero coefficient the
    /// run passes over takes a correction bit on the way.
    fn decode_ac_refine(
        &mut self,
        bits: &mut BitReader<'_>,
        component: &Component,
        tables: &Tables,
        (index, row, col): (usize, usize, usize),
        (ss, se, al): (usize, usize, u32),
        eobrun: &mut u32,
    ) -> bool {
        let Some(block) = self.block_at(index, row, col) else {
            return true;
        };
        let Some(table) = tables.ac.get(component.ac_table) else {
            return false;
        };
        let Some(plane) = self.planes.get_mut(index) else {
            return true;
        };
        let p1 = bit_value(al);

        // One more bit of an already non-zero coefficient: set it, on the side
        // away from zero, unless it is set already.
        let correct = |plane: &mut Plane, bits: &mut BitReader<'_>, k: usize| -> bool {
            let Some(bit) = bits.bit() else {
                return false;
            };
            if bit != 0 {
                let value = plane.value(block, k);
                if value & p1 == 0 {
                    let moved = if value >= 0 {
                        value.saturating_add(p1)
                    } else {
                        value.saturating_sub(p1)
                    };
                    plane.set(block, k, moved);
                }
            }
            true
        };

        let mut k = ss;
        if *eobrun == 0 {
            while k <= se {
                let Some(symbol) = table.decode(bits) else {
                    return false;
                };
                let mut run = u32::from(symbol >> 4);
                let size = u32::from(symbol & 0x0F);
                let mut value = 0i32;
                if size == 0 {
                    if run < 15 {
                        // End of band: this block's remaining non-zero
                        // coefficients still take their correction bits below.
                        let extra = if run > 0 {
                            u32::try_from(bits.bits(run).unwrap_or(0)).unwrap_or(0)
                        } else {
                            0
                        };
                        *eobrun = (1u32 << run).saturating_add(extra);
                        break;
                    }
                    // run == 15: sixteen zeros, with corrections along the way.
                } else {
                    // A newly non-zero coefficient is always one bit big.
                    let Some(sign) = bits.bit() else {
                        return false;
                    };
                    value = if sign != 0 { p1 } else { p1.saturating_neg() };
                }
                // Advance past `run` still-zero coefficients, correcting each
                // non-zero one passed, then place `value` in the next zero.
                while k <= se {
                    if plane.is_nonzero(block, k) {
                        if !correct(plane, bits, k) {
                            return false;
                        }
                    } else {
                        if run == 0 {
                            if value != 0 {
                                plane.set(block, k, value);
                            }
                            k = k.saturating_add(1);
                            break;
                        }
                        run = run.saturating_sub(1);
                    }
                    k = k.saturating_add(1);
                }
            }
        }
        if *eobrun > 0 {
            // Inside an end-of-band run: only the corrections.
            while k <= se {
                if plane.is_nonzero(block, k) && !correct(plane, bits, k) {
                    return false;
                }
                k = k.saturating_add(1);
            }
            *eobrun = eobrun.saturating_sub(1);
        }
        true
    }

    /// Turn the coefficients into a picture: dequantise, inverse-DCT, upsample
    /// and convert, exactly as a baseline scan does block by block.
    ///
    /// # Errors
    ///
    /// [`ImageError::Malformed`] only for a size that cannot be represented.
    pub(super) fn finish(mut self) -> ImageResult<Image> {
        let block = self.block;
        let out_width = self.width.saturating_mul(block).div_ceil(8).max(1);
        let out_height = self.height.saturating_mul(block).div_ceil(8).max(1);
        let basis = Basis::new();
        let mut samples: Vec<Samples> = Vec::with_capacity(self.planes.len());
        for (plane, component) in self.planes.iter_mut().zip(&self.components) {
            let size = plane.size;
            // The same plane a baseline decode of this frame writes: padded to
            // whole MCUs, `blocks_w * size` across.
            let mut out = Samples::new(Shape::of(
                (self.width, self.height),
                (component.h, component.v),
                (self.max_h, self.max_v),
                (size, block),
            ));
            let quant = plane.quant.unwrap_or([1u16; 64]);
            let per_block = plane.kept.per_block();
            for row in 0..plane.blocks_h {
                for col in 0..plane.blocks_w {
                    let index = row.saturating_mul(plane.blocks_w).saturating_add(col);
                    // Decided over all 63, kept or not, exactly as a baseline
                    // scan decides it: a block whose only AC coefficients are
                    // ones a thumbnail's transform ignores is still transformed
                    // rather than filled, so the two decodes stay bit for bit
                    // alike.
                    let any_ac = plane.nonzero.get(index).is_some_and(|mask| mask & !1 != 0);
                    // Each kept coefficient dequantised into its natural place.
                    // The rest stay zero, which the transform weighs at zero
                    // anyway.
                    let mut coefficients = [0.0f32; 64];
                    let first = index.saturating_mul(per_block);
                    for (natural, cell) in coefficients.iter_mut().enumerate() {
                        let Some(slot) = plane.kept.slot(natural) else {
                            continue;
                        };
                        let value = i32::from(
                            plane
                                .values
                                .get(first.saturating_add(slot))
                                .copied()
                                .unwrap_or(0),
                        );
                        let q = i32::from(quant.get(natural).copied().unwrap_or(1));
                        #[allow(clippy::cast_precision_loss, reason = "coefficients are small")]
                        let dequantised = value.saturating_mul(q) as f32;
                        *cell = dequantised;
                    }
                    let pixels = if !any_ac || size == 1 {
                        // Every output of the transform is the same one
                        // number -- for a block of one sample, whatever else
                        // it carries; see `flat_block`.
                        [flat_block(coefficients[0], &basis); 64]
                    } else if size == 8 {
                        idct_8x8(&mut coefficients, &basis);
                        coefficients
                    } else {
                        let mut scaled = [0.0f32; 64];
                        idct_scaled(&coefficients, &basis, size, &mut scaled);
                        scaled
                    };
                    store_block(
                        &pixels,
                        size,
                        &mut out,
                        col.saturating_mul(size),
                        row.saturating_mul(size),
                    );
                }
            }
            // The coefficients are done with; free them before the next
            // component's samples are allocated.
            plane.values = Vec::new();
            plane.nonzero = Vec::new();
            samples.push(out);
        }
        Ok(Image {
            width: u32::try_from(out_width)
                .map_err(|_| ImageError::Malformed("an impossible width"))?,
            height: u32::try_from(out_height)
                .map_err(|_| ImageError::Malformed("an impossible height"))?,
            pixels: to_pixels(out_width, out_height, &samples, block > 1),
        })
    }
}
