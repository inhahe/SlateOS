//! The decompression object: libjpeg-turbo's `jdapimin.c`, `jdapistd.c`,
//! `jdinput.c` and `jdmaster.c`, with its main and post-processing
//! controllers folded into handing out one output row at a time.
//!
//! The shape of the API is libjpeg's, because TIFF needs it: [`Decompress`]
//! reads a header ([`Decompress::read_header`]), lets its caller choose the
//! colour spaces and scale, starts ([`Decompress::start`]), hands out rows
//! ([`Decompress::read_row`]) and finishes ([`Decompress::finish`]), each step
//! failing exactly where libjpeg's would -- and libtiff reacts differently to a
//! failure in each. The tables it reads go into a [`Tables`] that outlives it,
//! as libjpeg's permanent pool does, so that TIFF's abbreviated strips find the
//! tables its `JPEGTables` field defined.
//!
//! The input side follows libjpeg's state machine. A single-scan image is
//! decoded one iMCU row -- one row of MCUs -- at a time as output rows are asked
//! for, and a row's worth of context ahead for the upsampler, just as
//! libjpeg's context controller reads ahead; a multi-scan image (progressive,
//! or sequential with the components in separate scans) is read whole into
//! [`Store`]s when the decode starts, as `jpeg_start_decompress` does, and
//! reconstructed a row at a time from there, with block smoothing where
//! libjpeg would smooth.

use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;

use super::arith::Arith;
use super::coef::{self, SAVED, Smoothing, Store};
use super::color::{self, ColorSpace, Ycc};
use super::error::{Error, jerr};
use super::huffman::{Pass, Progression, Progressive, Sequential};
use super::idct::{self, Target};
use super::marker::{self, Header, Input, Reached};
use super::tables::Tables;
use super::upsample::{Rows, Samples, Shape};
use crate::Limits;

/// What a header turned out to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Headed {
    /// An abbreviated datastream of tables alone (`JPEG_HEADER_TABLES_ONLY`).
    TablesOnly,
    /// An image, ready to start (`JPEG_HEADER_OK`).
    Image,
}

/// A scan's entropy decoder.
enum Entropy {
    Sequential(Box<Sequential>),
    Progressive(Box<Progressive>),
    Arith(Box<Arith>),
}

impl Entropy {
    fn insufficient(&self) -> bool {
        match self {
            Self::Sequential(d) => d.bits.insufficient,
            Self::Progressive(d) => d.bits.insufficient,
            Self::Arith(_) => false,
        }
    }

    fn decode_mcu(
        &mut self,
        input: &mut Input<'_>,
        header: &Header,
        blocks: &mut [[i16; 64]],
    ) -> Result<(), Error> {
        match self {
            Self::Sequential(d) => {
                d.decode_mcu(input, header, blocks);
                Ok(())
            }
            Self::Progressive(d) => d.decode_mcu(input, header, blocks),
            Self::Arith(d) => {
                d.decode_mcu(input, header, blocks);
                Ok(())
            }
        }
    }
}

/// `ceil(a / b)` for `b > 0`.
fn div_up(a: usize, b: usize) -> usize {
    a.div_ceil(b.max(1))
}

/// One JPEG datastream being decoded (`jpeg_decompress_struct`).
pub(crate) struct Decompress<'d, 't> {
    input: Input<'d>,
    tables: &'t mut Tables,
    header: Header,
    // The input controller.
    inheaders: bool,
    has_multiple_scans: bool,
    eoi_reached: bool,
    /// Whether the next input step reads markers (else scan data).
    reading_markers: bool,
    // Set at the first scan.
    max_h: usize,
    max_v: usize,
    total_imcu_rows: usize,
    // Set per scan.
    mcus_per_row: usize,
    /// For each block of an MCU, its component's position in the scan.
    membership: Vec<usize>,
    input_imcu_row: usize,
    mcu_rows_per_imcu_row: usize,
    last_good_imcu_row: usize,
    // Decompression parameters, defaulted by the header.
    jpeg_color_space: ColorSpace,
    out_color_space: ColorSpace,
    /// The reduced size each block becomes: 8 for full size, 4, 2 or 1.
    block_size: usize,
    max_scans: Option<u32>,
    // Decoding state.
    entropy: Option<Entropy>,
    progression: Option<Progression>,
    stores: Vec<Store>,
    min_dct: usize,
    output_width: usize,
    output_height: usize,
    out_components: usize,
    planes: Vec<Samples>,
    rows: Vec<Rows>,
    output_imcu_row: usize,
    output_scanline: usize,
    /// Per component, the progression status latched for smoothing: as it is,
    /// and as it was before the last scan.
    smoothing: Option<Vec<([i32; SAVED], [i32; SAVED])>>,
    ycc: Option<Ycc>,
    out_row: Vec<u8>,
}

impl<'d, 't> Decompress<'d, 't> {
    /// A decompressor for `data`, reading and keeping tables in `tables`.
    pub(crate) fn new(data: &'d [u8], tables: &'t mut Tables) -> Self {
        Self {
            input: Input::new(data),
            tables,
            header: Header::default(),
            inheaders: true,
            has_multiple_scans: false,
            eoi_reached: false,
            reading_markers: true,
            max_h: 1,
            max_v: 1,
            total_imcu_rows: 0,
            mcus_per_row: 0,
            membership: Vec::new(),
            input_imcu_row: 0,
            mcu_rows_per_imcu_row: 1,
            last_good_imcu_row: 0,
            jpeg_color_space: ColorSpace::Unknown,
            out_color_space: ColorSpace::Unknown,
            block_size: 8,
            max_scans: None,
            entropy: None,
            progression: None,
            stores: Vec::new(),
            min_dct: 8,
            output_width: 0,
            output_height: 0,
            out_components: 0,
            planes: Vec::new(),
            rows: Vec::new(),
            output_imcu_row: 0,
            output_scanline: 0,
            smoothing: None,
            ycc: None,
            out_row: Vec::new(),
        }
    }

    /// `jpeg_read_header`: read to the first scan, or to the end of a
    /// tables-only datastream (an error if `require_image`).
    pub(crate) fn read_header(&mut self, require_image: bool) -> Result<Headed, Error> {
        match self.consume_markers()? {
            Reached::Sos => {
                self.default_parameters();
                Ok(Headed::Image)
            }
            Reached::Eoi if require_image => Err(jerr::NO_IMAGE),
            Reached::Eoi => Ok(Headed::TablesOnly),
        }
    }

    // What the header said, for the caller to check before starting.

    pub(crate) const fn image_width(&self) -> usize {
        self.header.width
    }

    pub(crate) const fn image_height(&self) -> usize {
        self.header.height
    }

    pub(crate) fn num_components(&self) -> usize {
        self.header.components.len()
    }

    pub(crate) const fn data_precision(&self) -> u8 {
        self.header.precision
    }

    /// Component `ci`'s sampling factors.
    pub(crate) fn sampling(&self, ci: usize) -> Option<(usize, usize)> {
        self.header.components.get(ci).map(|c| (c.h, c.v))
    }

    /// The colour space the header implies (`default_decompress_parms`).
    pub(crate) const fn jpeg_color_space(&self) -> ColorSpace {
        self.jpeg_color_space
    }

    /// Override the colour spaces, as libtiff does.
    pub(crate) const fn set_color_spaces(&mut self, jpeg: ColorSpace, out: ColorSpace) {
        self.jpeg_color_space = jpeg;
        self.out_color_space = out;
    }

    /// Decode each 8x8 block to `block` samples a side: 8, 4, 2 or 1
    /// (`scale_num / scale_denom` of `block / 8`).
    pub(crate) fn set_block_size(&mut self, block: usize) {
        self.block_size = match block {
            1 => 1,
            2 => 2,
            4 => 4,
            _ => 8,
        };
    }

    /// Fail a decode whose datastream has this many scans or more, as the
    /// progress monitors of libtiff and Chrome do (both at 100).
    pub(crate) const fn set_max_scans(&mut self, max: u32) {
        self.max_scans = Some(max);
    }

    pub(crate) const fn output_width(&self) -> usize {
        self.output_width
    }

    pub(crate) const fn output_height(&self) -> usize {
        self.output_height
    }

    /// `consume_markers`.
    fn consume_markers(&mut self) -> Result<Reached, Error> {
        if self.eoi_reached {
            return Ok(Reached::Eoi);
        }
        let reached = marker::read_markers(&mut self.input, &mut self.header, self.tables)?;
        match reached {
            Reached::Sos => {
                if self.inheaders {
                    self.initial_setup()?;
                    self.inheaders = false;
                } else {
                    if !self.has_multiple_scans {
                        return Err(jerr::EOI_EXPECTED);
                    }
                    self.start_input_pass()?;
                }
            }
            Reached::Eoi => {
                self.eoi_reached = true;
                if self.inheaders && self.header.saw_sof {
                    return Err(jerr::SOF_NO_SOS);
                }
            }
        }
        Ok(reached)
    }

    /// `initial_setup`, at the first scan.
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "frame geometry: dimensions are at most 65500, sampling factors at most 4 and block sizes at most 8, so every product here is far below usize::MAX"
    )]
    fn initial_setup(&mut self) -> Result<(), Error> {
        let header = &mut self.header;
        if header.height > 65500 || header.width > 65500 {
            return Err(jerr::IMAGE_TOO_BIG);
        }
        let precision_ok = if header.lossless {
            (2..=16).contains(&header.precision)
        } else {
            header.precision == 8 || header.precision == 12
        };
        if !precision_ok {
            return Err(jerr::BAD_PRECISION);
        }
        if header.components.len() > 10 {
            return Err(jerr::COMPONENT_COUNT);
        }
        let (mut max_h, mut max_v) = (1usize, 1usize);
        for component in &header.components {
            if !(1..=4).contains(&component.h) || !(1..=4).contains(&component.v) {
                return Err(jerr::BAD_SAMPLING);
            }
            max_h = max_h.max(component.h);
            max_v = max_v.max(component.v);
        }
        let unit = if header.lossless { 1 } else { 8 };
        let (width, height) = (header.width, header.height);
        for component in &mut header.components {
            component.dct_scaled_size = unit;
            component.width_in_blocks =
                div_up(width.saturating_mul(component.h), max_h.saturating_mul(unit));
            component.height_in_blocks =
                div_up(height.saturating_mul(component.v), max_v.saturating_mul(unit));
            component.downsampled_width = div_up(width.saturating_mul(component.h), max_h);
            component.downsampled_height = div_up(height.saturating_mul(component.v), max_v);
            component.quant_table = None;
        }
        self.max_h = max_h;
        self.max_v = max_v;
        self.min_dct = unit;
        self.total_imcu_rows = div_up(height, max_v.saturating_mul(unit));
        self.has_multiple_scans =
            header.scan.count < header.components.len() || header.progressive;
        Ok(())
    }

    /// `default_decompress_parms`: guess the colour space from the markers
    /// and component ids.
    fn default_parameters(&mut self) {
        let header = &self.header;
        let ids: Vec<u8> = header.components.iter().map(|c| c.id).collect();
        let (jpeg, out) = match header.components.len() {
            1 => (ColorSpace::Grayscale, ColorSpace::Grayscale),
            3 => {
                let jpeg = if header.saw_jfif {
                    ColorSpace::YCbCr
                } else if header.saw_adobe {
                    match header.adobe_transform {
                        0 => ColorSpace::Rgb,
                        1 => ColorSpace::YCbCr,
                        _ => {
                            // JWRN_ADOBE_XFORM.
                            self.input.warn();
                            ColorSpace::YCbCr
                        }
                    }
                } else if ids == [1, 2, 3] {
                    if header.lossless {
                        ColorSpace::Rgb
                    } else {
                        ColorSpace::YCbCr
                    }
                } else if ids == [b'R', b'G', b'B'] || header.lossless {
                    ColorSpace::Rgb
                } else {
                    ColorSpace::YCbCr
                };
                (jpeg, ColorSpace::Rgb)
            }
            4 => {
                let jpeg = if header.saw_adobe {
                    match header.adobe_transform {
                        0 => ColorSpace::Cmyk,
                        2 => ColorSpace::Ycck,
                        _ => {
                            self.input.warn();
                            ColorSpace::Ycck
                        }
                    }
                } else {
                    ColorSpace::Cmyk
                };
                (jpeg, ColorSpace::Cmyk)
            }
            _ => (ColorSpace::Unknown, ColorSpace::Unknown),
        };
        self.jpeg_color_space = jpeg;
        self.out_color_space = out;
        self.block_size = 8;
    }

    /// `jpeg_start_decompress`: select and check every module as
    /// `master_selection` does, start the first scan, and read a multi-scan
    /// image whole. `rows_wanted`, if the caller will read fewer rows than the
    /// image has, bounds the samples accounted against `limits`.
    pub(crate) fn start(&mut self, limits: &Limits, rows_wanted: Option<usize>) -> Result<(), Error> {
        if self.header.lossless {
            return Err(Error::Unsupported("JPEG: lossless (SOF3, SOF11)"));
        }
        self.calc_output_dimensions();
        self.select_color()?;
        let fancy = self.min_dct > 1;
        self.select_upsampling(fancy)?;
        if self.header.precision != 8 {
            // A 12-bit image passes libjpeg's start and fails at the first
            // 8-bit scanline read; either way it does not decode.
            return Err(jerr::BAD_PRECISION);
        }
        if self.header.progressive {
            self.progression = Some(Progression::new(self.header.components.len()));
        }
        if !self.header.arith && !self.header.progressive {
            self.tables.default_huffman();
        }
        self.allocate(limits, rows_wanted)?;
        self.start_input_pass()?;
        self.last_good_imcu_row = 0;
        if self.has_multiple_scans {
            loop {
                if let Some(max) = self.max_scans {
                    if self.header.input_scan_number >= max {
                        return Err(jerr::TOO_MANY_SCANS);
                    }
                }
                if self.reading_markers {
                    if self.consume_markers()? == Reached::Eoi {
                        break;
                    }
                } else {
                    self.consume_data()?;
                }
            }
        }
        self.smoothing = self.smoothing_latches();
        self.output_scanline = 0;
        self.output_imcu_row = 0;
        Ok(())
    }

    /// `jpeg_calc_output_dimensions`, for the scales this decoder offers.
    #[allow(
    clippy::arithmetic_side_effects,
    reason = "frame geometry: dimensions are at most 65500 (initial_setup refuses more), sampling factors at most 4 and block sizes at most 8, so every product here is far below usize::MAX"
    )]
    fn calc_output_dimensions(&mut self) {
        let min = self.block_size;
        self.min_dct = min;
        let (width, height) = (self.header.width, self.header.height);
        self.output_width = div_up(width.saturating_mul(min), 8);
        self.output_height = div_up(height.saturating_mul(min), 8);
        let (max_h, max_v) = (self.max_h, self.max_v);
        for component in &mut self.header.components {
            // Scale the chroma up in the transform rather than the upsampler
            // wherever the ratio allows.
            let mut size = min;
            while size < 8
                && (max_h * min).is_multiple_of(component.h * size * 2)
                && (max_v * min).is_multiple_of(component.v * size * 2)
            {
                size *= 2;
            }
            component.dct_scaled_size = size;
            component.downsampled_width =
                div_up(width.saturating_mul(component.h * size), max_h * 8);
            component.downsampled_height =
                div_up(height.saturating_mul(component.v * size), max_v * 8);
        }
    }

    /// `jinit_color_deconverter`'s checks, and the output's component count.
    fn select_color(&mut self) -> Result<(), Error> {
        let n = self.header.components.len();
        let count_ok = match self.jpeg_color_space {
            ColorSpace::Grayscale => n == 1,
            ColorSpace::Rgb | ColorSpace::YCbCr => n == 3,
            ColorSpace::Cmyk | ColorSpace::Ycck => n == 4,
            ColorSpace::Unknown => n >= 1,
        };
        if !count_ok {
            return Err(jerr::BAD_J_COLORSPACE);
        }
        use ColorSpace as C;
        let supported = match self.out_color_space {
            C::Grayscale => matches!(self.jpeg_color_space, C::Grayscale | C::YCbCr),
            C::Rgb => matches!(self.jpeg_color_space, C::YCbCr | C::Grayscale | C::Rgb),
            C::Cmyk => matches!(self.jpeg_color_space, C::Ycck | C::Cmyk),
            out => out == self.jpeg_color_space,
        };
        if !supported {
            return Err(jerr::CONVERSION_NOTIMPL);
        }
        self.out_components = self.out_color_space.components().unwrap_or(n);
        if matches!(
            (self.jpeg_color_space, self.out_color_space),
            (C::YCbCr, C::Rgb) | (C::Ycck, C::Cmyk)
        ) {
            self.ycc = Some(Ycc::new());
        }
        Ok(())
    }

    /// `jinit_upsampler`'s checks, and each plane's shape and filter.
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "frame geometry: dimensions are at most 65500, sampling factors at most 4 and block sizes at most 8, so every product here is far below usize::MAX"
    )]
    fn select_upsampling(&mut self, fancy: bool) -> Result<(), Error> {
        let (max_h, max_v, min) = (self.max_h, self.max_v, self.min_dct);
        self.planes.clear();
        self.rows.clear();
        for component in &self.header.components {
            let size = component.dct_scaled_size;
            let h_in = component.h * size / min;
            let v_in = component.v * size / min;
            if h_in == 0 || v_in == 0 || max_h % h_in != 0 || max_v % v_in != 0 {
                return Err(jerr::FRACT_SAMPLE_NOTIMPL);
            }
            let shape = Shape {
                stride: component.width_in_blocks * size,
                rows: self.total_imcu_rows * component.v * size,
                width: component.downsampled_width,
                height: component.downsampled_height,
                across: (component.h * size, max_h * min),
                down: (component.v * size, max_v * min),
            };
            self.rows.push(Rows::new(&shape, self.output_width, fancy));
            self.planes.push(Samples::new(shape));
        }
        self.out_row = vec![0u8; self.output_width * self.out_components];
        Ok(())
    }

    /// The coefficient stores of a multi-scan image, after checking them and
    /// the planes against `limits`.
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "frame geometry: dimensions are at most 65500, sampling factors at most 4 and block sizes at most 8, so every product here is far below usize::MAX"
    )]
    fn allocate(&mut self, limits: &Limits, rows_wanted: Option<usize>) -> Result<(), Error> {
        let imcu_rows = match rows_wanted {
            Some(rows) => div_up(rows, self.max_v * self.min_dct)
                .saturating_add(1)
                .min(self.total_imcu_rows),
            None => self.total_imcu_rows,
        };
        let mut bytes = 0u64;
        for (component, plane) in self.header.components.iter().zip(&self.planes) {
            let rows = (imcu_rows * component.v * component.dct_scaled_size).min(plane.shape.rows);
            bytes = bytes.saturating_add((rows as u64).saturating_mul(plane.shape.stride as u64));
            if self.has_multiple_scans {
                let blocks = component.width_in_blocks.next_multiple_of(component.h)
                    * component.height_in_blocks.next_multiple_of(component.v);
                bytes = bytes.saturating_add(Store::bytes(blocks, component.dct_scaled_size));
            }
        }
        let limit = limits.max_decompressed_bytes as u64;
        if bytes > limit {
            return Err(Error::TooLarge {
                amount: bytes,
                limit,
            });
        }
        self.stores.clear();
        if self.has_multiple_scans {
            for component in &self.header.components {
                self.stores.push(Store::new(
                    component.width_in_blocks.next_multiple_of(component.h),
                    component.height_in_blocks.next_multiple_of(component.v),
                    component.dct_scaled_size,
                ));
            }
        }
        Ok(())
    }

    /// `start_input_pass`: set up for the scan whose header was just read.
    fn start_input_pass(&mut self) -> Result<(), Error> {
        self.per_scan_setup()?;
        self.latch_quant_tables()?;
        let entropy = if self.header.arith {
            let pass = if self.header.progressive {
                let pass = Pass::of(&self.header)?;
                if let Some(progression) = self.progression.as_mut() {
                    progression.update(&mut self.input, &self.header);
                }
                Some(pass)
            } else {
                None
            };
            Entropy::Arith(Box::new(Arith::start(
                &mut self.input,
                &self.header,
                pass,
                &self.membership,
            )?))
        } else if self.header.progressive {
            let pass = Pass::of(&self.header)?;
            if let Some(progression) = self.progression.as_mut() {
                progression.update(&mut self.input, &self.header);
            }
            Entropy::Progressive(Box::new(Progressive::start(
                pass,
                &self.header,
                self.tables,
                &self.membership,
            )?))
        } else {
            Entropy::Sequential(Box::new(Sequential::start(
                &mut self.input,
                &self.header,
                self.tables,
                &self.membership,
            )?))
        };
        self.entropy = Some(entropy);
        self.input_imcu_row = 0;
        self.start_imcu_row();
        self.reading_markers = false;
        Ok(())
    }

    /// `per_scan_setup`.
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "frame geometry: dimensions are at most 65500, sampling factors at most 4 and block sizes at most 8, so every product here is far below usize::MAX"
    )]
    fn per_scan_setup(&mut self) -> Result<(), Error> {
        let scan = self.header.scan;
        self.membership.clear();
        if scan.count == 1 {
            let ci = scan.comps[0];
            let component = self
                .header
                .components
                .get_mut(ci)
                .ok_or(jerr::BAD_COMPONENT_ID)?;
            self.mcus_per_row = component.width_in_blocks;
            component.mcu_width = 1;
            component.mcu_height = 1;
            component.mcu_blocks = 1;
            component.mcu_sample_width = component.dct_scaled_size;
            component.last_col_width = 1;
            let rest = component.height_in_blocks % component.v.max(1);
            component.last_row_height = if rest == 0 { component.v } else { rest };
            self.membership.push(0);
        } else {
            if !(1..=4).contains(&scan.count) {
                return Err(jerr::COMPONENT_COUNT);
            }
            self.mcus_per_row = div_up(self.header.width, self.max_h * 8);
            for (position, &ci) in scan.components().iter().enumerate() {
                let component = self
                    .header
                    .components
                    .get_mut(ci)
                    .ok_or(jerr::BAD_COMPONENT_ID)?;
                component.mcu_width = component.h;
                component.mcu_height = component.v;
                component.mcu_blocks = component.h * component.v;
                component.mcu_sample_width = component.h * component.dct_scaled_size;
                let rest = component.width_in_blocks % component.h;
                component.last_col_width = if rest == 0 { component.h } else { rest };
                let rest = component.height_in_blocks % component.v;
                component.last_row_height = if rest == 0 { component.v } else { rest };
                if self.membership.len() + component.mcu_blocks > 10 {
                    return Err(jerr::BAD_MCU_SIZE);
                }
                self.membership
                    .extend(core::iter::repeat_n(position, component.mcu_blocks));
            }
        }
        Ok(())
    }

    /// `latch_quant_tables`: each component keeps the quantisation table in
    /// force at its first scan.
    fn latch_quant_tables(&mut self) -> Result<(), Error> {
        let scan = self.header.scan;
        for &ci in scan.components() {
            let Some(component) = self.header.components.get_mut(ci) else {
                continue;
            };
            if component.quant_table.is_some() {
                continue;
            }
            let table = self
                .tables
                .quant
                .get(usize::from(component.quant_tbl_no))
                .copied()
                .flatten()
                .ok_or(jerr::NO_QUANT_TABLE)?;
            component.quant_table = Some(table);
        }
        Ok(())
    }

    /// `start_iMCU_row`.
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "frame geometry: dimensions are at most 65500, sampling factors at most 4 and block sizes at most 8, so every product here is far below usize::MAX"
    )]
    fn start_imcu_row(&mut self) {
        let scan = &self.header.scan;
        self.mcu_rows_per_imcu_row = if scan.count > 1 {
            1
        } else {
            let component = scan
                .comps
                .first()
                .and_then(|&ci| self.header.components.get(ci));
            match component {
                Some(c) if self.input_imcu_row + 1 < self.total_imcu_rows => c.v,
                Some(c) => c.last_row_height,
                None => 1,
            }
        };
    }

    /// `consume_data`: one iMCU row of a multi-scan image's current scan into
    /// the stores.
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "frame geometry: dimensions are at most 65500, sampling factors at most 4 and block sizes at most 8, so every product here is far below usize::MAX"
    )]
    fn consume_data(&mut self) -> Result<(), Error> {
        let scan = self.header.scan;
        let mut blocks = [[0i16; 64]; 10];
        let mut places = [(0usize, 0usize, 0usize); 10];
        for yoffset in 0..self.mcu_rows_per_imcu_row {
            for mcu_col in 0..self.mcus_per_row {
                let mut count = 0usize;
                for &ci in scan.components() {
                    let Some(component) = self.header.components.get(ci) else {
                        continue;
                    };
                    let Some(store) = self.stores.get(ci) else {
                        continue;
                    };
                    let start_col = mcu_col * component.mcu_width;
                    for yindex in 0..component.mcu_height {
                        let by = self.input_imcu_row * component.v + yindex + yoffset;
                        for xindex in 0..component.mcu_width {
                            let bx = start_col + xindex;
                            if let (Some(block), Some(place)) =
                                (blocks.get_mut(count), places.get_mut(count))
                            {
                                *block = store.load(bx, by);
                                *place = (ci, bx, by);
                            }
                            count += 1;
                        }
                    }
                }
                let Some(entropy) = self.entropy.as_mut() else {
                    return Ok(());
                };
                if !entropy.insufficient() {
                    self.last_good_imcu_row = self.input_imcu_row;
                }
                let count = count.min(10);
                let mcu = blocks.get_mut(..count).unwrap_or_default();
                entropy.decode_mcu(&mut self.input, &self.header, mcu)?;
                for (block, &(ci, bx, by)) in blocks.iter().zip(&places).take(count) {
                    if let Some(store) = self.stores.get_mut(ci) {
                        store.save(bx, by, block);
                    }
                }
            }
        }
        self.input_imcu_row += 1;
        if self.input_imcu_row < self.total_imcu_rows {
            self.start_imcu_row();
        } else {
            self.reading_markers = true;
        }
        Ok(())
    }

    /// `decompress_onepass`: decode one iMCU row of a single-scan image and
    /// reconstruct it into the planes.
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "frame geometry: dimensions are at most 65500, sampling factors at most 4 and block sizes at most 8, so every product here is far below usize::MAX"
    )]
    fn decompress_onepass(&mut self) -> Result<(), Error> {
        let scan = self.header.scan;
        let last_imcu_row = self.total_imcu_rows.saturating_sub(1);
        let last_mcu_col = self.mcus_per_row.saturating_sub(1);
        let row = self.input_imcu_row;
        for (component, plane) in self.header.components.iter().zip(self.planes.iter_mut()) {
            plane.grow_to((row + 1) * component.v * component.dct_scaled_size);
        }
        let mut blocks = [[0i16; 64]; 10];
        let blocks_in_mcu = self.membership.len().min(10);
        for yoffset in 0..self.mcu_rows_per_imcu_row {
            for mcu_col in 0..=last_mcu_col {
                for block in blocks.iter_mut().take(blocks_in_mcu) {
                    *block = [0; 64];
                }
                let Some(entropy) = self.entropy.as_mut() else {
                    return Ok(());
                };
                if !entropy.insufficient() {
                    self.last_good_imcu_row = row;
                }
                let mcu = blocks.get_mut(..blocks_in_mcu).unwrap_or_default();
                entropy.decode_mcu(&mut self.input, &self.header, mcu)?;
                let mut blkn = 0usize;
                for &ci in scan.components() {
                    let (Some(component), Some(plane)) =
                        (self.header.components.get(ci), self.planes.get_mut(ci))
                    else {
                        continue;
                    };
                    let size = component.dct_scaled_size;
                    let quant = component.quant_table.unwrap_or([0; 64]);
                    let useful_width = if mcu_col < last_mcu_col {
                        component.mcu_width
                    } else {
                        component.last_col_width
                    };
                    let row0 = row * component.v * size + yoffset * size;
                    let start_col = mcu_col * component.mcu_sample_width;
                    let stride = plane.shape.stride;
                    for yindex in 0..component.mcu_height {
                        if row < last_imcu_row || yoffset + yindex < component.last_row_height {
                            for xindex in 0..useful_width {
                                let Some(block) = blocks.get(blkn + xindex) else {
                                    continue;
                                };
                                let at = (row0 + yindex * size) * stride + start_col + xindex * size;
                                idct::inverse(
                                    size,
                                    block,
                                    &quant,
                                    &mut Target {
                                        out: &mut plane.data,
                                        at,
                                        stride,
                                    },
                                );
                            }
                        }
                        blkn += component.mcu_width;
                    }
                }
            }
        }
        self.output_imcu_row += 1;
        self.input_imcu_row += 1;
        if self.input_imcu_row < self.total_imcu_rows {
            self.start_imcu_row();
        } else {
            self.reading_markers = true;
        }
        Ok(())
    }

    /// `decompress_data`, or `decompress_smooth_data` where smoothing
    /// applies: reconstruct one iMCU row of a multi-scan image from the
    /// stores.
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "frame geometry: dimensions are at most 65500, sampling factors at most 4 and block sizes at most 8, so every product here is far below usize::MAX"
    )]
    fn decompress_data(&mut self) {
        let row = self.output_imcu_row;
        let last_imcu_row = self.total_imcu_rows.saturating_sub(1);
        for (ci, (component, plane)) in self
            .header
            .components
            .iter()
            .zip(self.planes.iter_mut())
            .enumerate()
        {
            let Some(store) = self.stores.get(ci) else {
                continue;
            };
            let size = component.dct_scaled_size;
            let v = component.v;
            plane.grow_to((row + 1) * v * size);
            let block_rows = if row < last_imcu_row {
                v
            } else {
                let rest = component.height_in_blocks % v;
                if rest == 0 { v } else { rest }
            };
            let quant = component.quant_table.unwrap_or([0; 64]);
            let stride = plane.shape.stride;
            if let Some(latches) = self.smoothing.as_ref().and_then(|l| l.get(ci)) {
                let bits = if row > self.last_good_imcu_row {
                    &latches.1
                } else {
                    &latches.0
                };
                coef::smooth_row(
                    &Smoothing {
                        store,
                        quant: &quant,
                        bits,
                        width_in_blocks: component.width_in_blocks,
                        v,
                        block_rows,
                        imcu_row: row,
                        total_imcu_rows: self.total_imcu_rows,
                        size,
                    },
                    &mut plane.data,
                    stride,
                );
                continue;
            }
            for block_row in 0..block_rows {
                let by = row * v + block_row;
                for bx in 0..component.width_in_blocks {
                    let block = store.load(bx, by);
                    idct::inverse(
                        size,
                        &block,
                        &quant,
                        &mut Target {
                            out: &mut plane.data,
                            at: by * size * stride + bx * size,
                            stride,
                        },
                    );
                }
            }
        }
        self.output_imcu_row += 1;
    }

    /// `smoothing_ok`, with the latches it takes.
    fn smoothing_latches(&self) -> Option<Vec<([i32; SAVED], [i32; SAVED])>> {
        if !self.header.progressive {
            return None;
        }
        let progression = self.progression.as_ref()?;
        let mut useful = false;
        let mut latches = Vec::with_capacity(self.header.components.len());
        for (ci, component) in self.header.components.iter().enumerate() {
            let quant = component.quant_table?;
            if !coef::quantisers_usable(&quant) {
                return None;
            }
            let bits = progression.bits.get(ci)?;
            let previous = progression.previous.get(ci)?;
            if bits[0] < 0 {
                return None;
            }
            let mut latch = [0i32; SAVED];
            let mut prev_latch = [0i32; SAVED];
            for (coefficient, ((slot, prev_slot), (&now, &before))) in latch
                .iter_mut()
                .zip(prev_latch.iter_mut())
                .zip(bits.iter().zip(previous))
                .enumerate()
            {
                *slot = now;
                if coefficient == 0 {
                    continue;
                }
                *prev_slot = if self.header.input_scan_number > 1 {
                    before
                } else {
                    -1
                };
                if now != 0 {
                    useful = true;
                }
            }
            latches.push((latch, prev_latch));
        }
        useful.then_some(latches)
    }

    /// `jpeg_read_scanlines` for one row: the next output row, with
    /// `output_components` samples a pixel. Empty once every row has been
    /// read (`JWRN_TOO_MUCH_DATA`).
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "frame geometry: dimensions are at most 65500, sampling factors at most 4 and block sizes at most 8, so every product here is far below usize::MAX"
    )]
    pub(crate) fn read_row(&mut self) -> Result<&[u8], Error> {
        let y = self.output_scanline;
        if y >= self.output_height {
            self.input.warn();
            return Ok(&[]);
        }
        let per_imcu = (self.max_v * self.min_dct).max(1);
        let needed = (y / per_imcu + 1).min(self.total_imcu_rows.saturating_sub(1));
        while self.output_imcu_row <= needed && self.output_imcu_row < self.total_imcu_rows {
            if self.has_multiple_scans {
                self.decompress_data();
            } else {
                self.decompress_onepass()?;
            }
        }
        let mut lines: [&[u8]; 10] = [&[]; 10];
        for ((line, rows), plane) in lines.iter_mut().zip(self.rows.iter_mut()).zip(&self.planes) {
            *line = rows.row(plane, y);
        }
        let n = self.planes.len().min(10);
        let lines = lines.get(..n).unwrap_or_default();
        let out = &mut self.out_row;
        use ColorSpace as C;
        match (self.jpeg_color_space, self.out_color_space, lines) {
            (C::YCbCr, C::Rgb, [y, cb, cr]) => {
                if let Some(ycc) = self.ycc.as_ref() {
                    color::ycc_rgb(ycc, y, cb, cr, out);
                }
            }
            (C::Ycck, C::Cmyk, [y, cb, cr, k]) => {
                if let Some(ycc) = self.ycc.as_ref() {
                    color::ycck_cmyk(ycc, y, cb, cr, k, out);
                }
            }
            (C::Grayscale, C::Rgb, [y]) => color::gray_rgb(y, out),
            (C::YCbCr | C::Grayscale, C::Grayscale, [y, ..]) => color::interleave(&[y], out),
            (_, _, lines) => color::interleave(lines, out),
        }
        self.output_scanline += 1;
        Ok(&self.out_row)
    }

    /// Rows read so far.
    pub(crate) const fn output_scanline(&self) -> usize {
        self.output_scanline
    }

    /// `jpeg_finish_decompress`: every row must have been read; then read to
    /// the end of the datastream, which can still fail -- on a second scan in
    /// a single-scan image, a second frame, a marker libjpeg does not know.
    pub(crate) fn finish(&mut self) -> Result<(), Error> {
        if self.output_scanline < self.output_height {
            return Err(Error::Malformed("JPEG: finished before every row was read"));
        }
        while !self.eoi_reached {
            if self.reading_markers {
                self.consume_markers()?;
            } else if self.has_multiple_scans {
                self.consume_data()?;
            } else {
                self.decompress_onepass()?;
            }
        }
        Ok(())
    }
}
