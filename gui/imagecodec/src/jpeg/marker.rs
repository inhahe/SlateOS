//! Reading a datastream's markers: libjpeg-turbo's `jdmarker.c`.
//!
//! Everything a JPEG says about itself outside the entropy-coded data is a
//! marker, and which datastreams a decoder accepts is decided almost entirely
//! here: the exact length arithmetic of every segment, which table numbers are
//! legal, what happens to garbage between markers, what an unexpected marker
//! is. So this is a transcription rather than a reading of the standard, down
//! to libjpeg's quirks -- a scan component may only be matched to one of the
//! frame's first four, a `DQT` whose precision nibble is anything but zero is
//! 16-bit, a segment is read on past its declared length and only the final
//! count is checked.
//!
//! libjpeg writes each field into the decompression object as it reads it, and
//! reads fake end-of-image bytes once the data runs out ([`Source`]), so a
//! segment cut short is parsed to its end all the same and fails -- or, now
//! and then, does not -- on the values the fake bytes make. That is kept too.

use alloc::vec::Vec;

use super::error::{Error, jerr};
use super::source::Source;
use super::tables::{HuffSpec, NATURAL, Tables};

/// The input side shared by the marker reader and the entropy decoders.
#[derive(Debug, Clone)]
pub(super) struct Input<'a> {
    pub(super) src: Source<'a>,
    /// `cinfo->unread_marker`: a marker code read but not yet processed, or 0.
    pub(super) unread_marker: u8,
    /// `marker->next_restart_num`: the restart marker expected next.
    pub(super) next_restart_num: u8,
    /// libjpeg's warning count, for diagnostics.
    pub(super) warnings: u32,
}

impl<'a> Input<'a> {
    pub(super) const fn new(data: &'a [u8]) -> Self {
        Self {
            src: Source::new(data),
            unread_marker: 0,
            next_restart_num: 0,
            warnings: 0,
        }
    }

    pub(super) const fn warn(&mut self) {
        self.warnings = self.warnings.saturating_add(1);
    }
}

/// One component of the frame (`jpeg_component_info`).
#[derive(Debug, Clone, Default)]
pub(super) struct Component {
    pub(super) id: u8,
    /// Sampling factors, as the frame gives them (any nibble; `initial_setup`
    /// refuses those outside 1-4).
    pub(super) h: usize,
    pub(super) v: usize,
    pub(super) quant_tbl_no: u8,
    /// Set by each scan that includes the component.
    pub(super) dc_tbl_no: u8,
    pub(super) ac_tbl_no: u8,
    // Set at the first scan (`initial_setup`) and by `jpeg_calc_output_dimensions`.
    pub(super) width_in_blocks: usize,
    pub(super) height_in_blocks: usize,
    pub(super) downsampled_width: usize,
    pub(super) downsampled_height: usize,
    /// `DCT_scaled_size`: how many samples a side each block becomes.
    pub(super) dct_scaled_size: usize,
    /// The quantisation table latched at the component's first scan.
    pub(super) quant_table: Option<[u16; 64]>,
    // Set per scan (`per_scan_setup`).
    pub(super) mcu_width: usize,
    pub(super) mcu_height: usize,
    pub(super) mcu_blocks: usize,
    pub(super) mcu_sample_width: usize,
    pub(super) last_col_width: usize,
    pub(super) last_row_height: usize,
}

/// A scan's header (`SOS`).
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct Scan {
    /// The frame indices of its components, in scan order.
    pub(super) comps: [usize; 4],
    /// `comps_in_scan`.
    pub(super) count: usize,
    pub(super) ss: u8,
    pub(super) se: u8,
    pub(super) ah: u8,
    pub(super) al: u8,
}

impl Scan {
    /// The frame indices of the scan's components.
    pub(super) fn components(&self) -> &[usize] {
        self.comps.get(..self.count).unwrap_or(&[])
    }
}

/// Everything the markers of one datastream set: libjpeg's `cinfo` fields
/// that `jpeg_abort` and `reset_marker_reader` start afresh.
#[derive(Debug, Clone, Default)]
pub(super) struct Header {
    pub(super) saw_soi: bool,
    pub(super) saw_sof: bool,
    /// Reset by `SOI`, set by `DRI`.
    pub(super) restart_interval: u16,
    /// Arithmetic-coding conditioning, reset by `SOI`, set by `DAC`.
    pub(super) arith_dc_l: [u8; 16],
    pub(super) arith_dc_u: [u8; 16],
    pub(super) arith_ac_k: [u8; 16],
    pub(super) saw_jfif: bool,
    pub(super) saw_adobe: bool,
    pub(super) adobe_transform: u8,
    // The frame.
    pub(super) precision: u8,
    pub(super) width: usize,
    pub(super) height: usize,
    pub(super) components: Vec<Component>,
    pub(super) progressive: bool,
    pub(super) lossless: bool,
    pub(super) arith: bool,
    // The current scan.
    pub(super) scan: Scan,
    pub(super) input_scan_number: u32,
}

/// What `read_markers` stopped at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Reached {
    Sos,
    Eoi,
}

/// `next_marker`: skip to the next marker, over garbage and fill bytes, and
/// leave its code in `unread_marker`. The result is never 0 or `FF`.
pub(super) fn next_marker(input: &mut Input<'_>) {
    let mut discarded = false;
    loop {
        let mut c = input.src.byte();
        while c != 0xFF {
            discarded = true;
            c = input.src.byte();
        }
        loop {
            c = input.src.byte();
            if c != 0xFF {
                break;
            }
        }
        if c != 0 {
            if discarded {
                // JWRN_EXTRANEOUS_DATA.
                input.warn();
            }
            input.unread_marker = c;
            return;
        }
        // A stuffed zero outside a scan: garbage.
        discarded = true;
    }
}

/// `first_marker`: the datastream must begin `FF D8`, with nothing before it.
fn first_marker(input: &mut Input<'_>) -> Result<(), Error> {
    let c = input.src.byte();
    let c2 = input.src.byte();
    if c != 0xFF || c2 != 0xD8 {
        return Err(jerr::NO_SOI);
    }
    input.unread_marker = c2;
    Ok(())
}

/// `read_markers`: process markers until a scan or the end.
pub(super) fn read_markers(
    input: &mut Input<'_>,
    header: &mut Header,
    tables: &mut Tables,
) -> Result<Reached, Error> {
    loop {
        if input.unread_marker == 0 {
            if header.saw_soi {
                next_marker(input);
            } else {
                first_marker(input)?;
            }
        }
        match input.unread_marker {
            0xD8 => get_soi(header)?,
            // Baseline and extended sequential, Huffman.
            0xC0 | 0xC1 => get_sof(input, header, false, false, false)?,
            0xC2 => get_sof(input, header, true, false, false)?,
            0xC3 => get_sof(input, header, false, true, false)?,
            0xC9 => get_sof(input, header, false, false, true)?,
            0xCA => get_sof(input, header, true, false, true)?,
            0xCB => get_sof(input, header, false, true, true)?,
            0xC5..=0xC8 | 0xCD..=0xCF => return Err(jerr::SOF_UNSUPPORTED),
            0xDA => {
                get_sos(input, header)?;
                input.unread_marker = 0;
                return Ok(Reached::Sos);
            }
            0xD9 => {
                input.unread_marker = 0;
                return Ok(Reached::Eoi);
            }
            0xCC => get_dac(input, header)?,
            0xC4 => get_dht(input, tables)?,
            0xDB => get_dqt(input, tables)?,
            0xDD => get_dri(input, header)?,
            0xE0 | 0xEE => get_interesting_appn(input, header),
            0xE1..=0xEF | 0xFE | 0xDC => skip_variable(input),
            // Parameterless.
            0xD0..=0xD7 | 0x01 => {}
            _ => return Err(jerr::UNKNOWN_MARKER),
        }
        input.unread_marker = 0;
    }
}

/// `get_soi`: reset what an `SOI` resets.
fn get_soi(header: &mut Header) -> Result<(), Error> {
    if header.saw_soi {
        return Err(jerr::SOI_DUPLICATE);
    }
    header.arith_dc_l = [0; 16];
    header.arith_dc_u = [1; 16];
    header.arith_ac_k = [5; 16];
    header.restart_interval = 0;
    header.saw_jfif = false;
    header.saw_adobe = false;
    header.adobe_transform = 0;
    header.saw_soi = true;
    Ok(())
}

/// `get_sof`.
fn get_sof(
    input: &mut Input<'_>,
    header: &mut Header,
    progressive: bool,
    lossless: bool,
    arith: bool,
) -> Result<(), Error> {
    if header.saw_sof {
        return Err(jerr::SOF_DUPLICATE);
    }
    header.progressive = progressive;
    header.lossless = lossless;
    header.arith = arith;
    let length = i64::from(input.src.word());
    header.precision = input.src.byte();
    header.height = usize::from(input.src.word());
    header.width = usize::from(input.src.word());
    let count = input.src.byte();
    if header.height == 0 || header.width == 0 || count == 0 {
        return Err(jerr::EMPTY_IMAGE);
    }
    if length.wrapping_sub(8) != i64::from(count).wrapping_mul(3) {
        return Err(jerr::BAD_LENGTH);
    }
    header.components = (0..usize::from(count))
        .map(|_| {
            let id = input.src.byte();
            let sampling = input.src.byte();
            let quant_tbl_no = input.src.byte();
            Component {
                id,
                h: usize::from(sampling >> 4),
                v: usize::from(sampling & 0x0F),
                quant_tbl_no,
                ..Component::default()
            }
        })
        .collect();
    header.saw_sof = true;
    Ok(())
}

/// `get_sos`.
fn get_sos(input: &mut Input<'_>, header: &mut Header) -> Result<(), Error> {
    if !header.saw_sof {
        return Err(jerr::SOS_NO_SOF);
    }
    let length = input.src.word();
    let n = input.src.byte();
    if u32::from(length) != u32::from(n).wrapping_mul(2).wrapping_add(6) || !(1..=4).contains(&n)
    {
        return Err(jerr::BAD_LENGTH);
    }
    let count = usize::from(n);
    // Indexed by scan position, as libjpeg's `cur_comp_info`; the search below
    // tests it at the *frame* index, which is libjpeg's own rule.
    let mut cur: [Option<usize>; 4] = [None; 4];
    for i in 0..count {
        let id = input.src.byte();
        let tables = input.src.byte();
        let searchable = header.components.len().min(4);
        let found = (0..searchable).find(|&ci| {
            header.components.get(ci).is_some_and(|c| c.id == id)
                && cur.get(ci).copied().flatten().is_none()
        });
        let Some(ci) = found else {
            return Err(jerr::BAD_COMPONENT_ID);
        };
        if let Some(slot) = cur.get_mut(i) {
            *slot = Some(ci);
        }
        if let Some(component) = header.components.get_mut(ci) {
            component.dc_tbl_no = tables >> 4;
            component.ac_tbl_no = tables & 0x0F;
        }
        if cur.iter().take(i).any(|&previous| previous == Some(ci)) {
            return Err(jerr::BAD_COMPONENT_ID);
        }
    }
    let ss = input.src.byte();
    let se = input.src.byte();
    let a = input.src.byte();
    let mut comps = [0usize; 4];
    for (slot, found) in comps.iter_mut().zip(cur) {
        *slot = found.unwrap_or(0);
    }
    header.scan = Scan {
        comps,
        count,
        ss,
        se,
        ah: a >> 4,
        al: a & 0x0F,
    };
    input.next_restart_num = 0;
    header.input_scan_number = header.input_scan_number.saturating_add(1);
    Ok(())
}

/// `get_dac`: arithmetic-coding conditioning.
fn get_dac(input: &mut Input<'_>, header: &mut Header) -> Result<(), Error> {
    let mut length = i64::from(input.src.word()).wrapping_sub(2);
    while length > 0 {
        let index = input.src.byte();
        let value = input.src.byte();
        length = length.wrapping_sub(2);
        if index >= 32 {
            return Err(jerr::DAC_INDEX);
        }
        let at = usize::from(index & 0x0F);
        if index >= 16 {
            if let Some(k) = header.arith_ac_k.get_mut(at) {
                *k = value;
            }
        } else {
            let (low, high) = (value & 0x0F, value >> 4);
            if let Some(l) = header.arith_dc_l.get_mut(at) {
                *l = low;
            }
            if let Some(u) = header.arith_dc_u.get_mut(at) {
                *u = high;
            }
            if low > high {
                return Err(jerr::DAC_VALUE);
            }
        }
    }
    if length != 0 {
        return Err(jerr::BAD_LENGTH);
    }
    Ok(())
}

/// `get_dht`.
fn get_dht(input: &mut Input<'_>, tables: &mut Tables) -> Result<(), Error> {
    let mut length = i64::from(input.src.word()).wrapping_sub(2);
    while length > 16 {
        let index = input.src.byte();
        let mut bits = [0u8; 17];
        let mut count = 0i64;
        for slot in bits.iter_mut().skip(1) {
            *slot = input.src.byte();
            count = count.wrapping_add(i64::from(*slot));
        }
        length = length.wrapping_sub(17);
        if count > 256 || count > length {
            return Err(jerr::BAD_HUFF_TABLE);
        }
        let mut values = [0u8; 256];
        for slot in values.iter_mut().take(usize::try_from(count).unwrap_or(0)) {
            *slot = input.src.byte();
        }
        length = length.wrapping_sub(count);
        let (slot, number) = if index & 0x10 != 0 {
            (&mut tables.ac, index.wrapping_sub(0x10))
        } else {
            (&mut tables.dc, index)
        };
        let Some(table) = slot.get_mut(usize::from(number)) else {
            return Err(jerr::DHT_INDEX);
        };
        *table = Some(HuffSpec { bits, values });
    }
    if length != 0 {
        return Err(jerr::BAD_LENGTH);
    }
    Ok(())
}

/// `get_dqt`. The table is written into as it is read, as libjpeg does, so a
/// segment that then fails its length check has already changed it.
fn get_dqt(input: &mut Input<'_>, tables: &mut Tables) -> Result<(), Error> {
    let mut length = i64::from(input.src.word()).wrapping_sub(2);
    while length > 0 {
        let spec = input.src.byte();
        let wide = spec >> 4 != 0;
        let Some(slot) = tables.quant.get_mut(usize::from(spec & 0x0F)) else {
            return Err(jerr::DQT_INDEX);
        };
        let table = slot.get_or_insert([0u16; 64]);
        for &position in NATURAL.iter().take(64) {
            let value = if wide {
                input.src.word()
            } else {
                u16::from(input.src.byte())
            };
            if let Some(cell) = table.get_mut(position) {
                *cell = value;
            }
        }
        length = length.wrapping_sub(if wide { 129 } else { 65 });
    }
    if length != 0 {
        return Err(jerr::BAD_LENGTH);
    }
    Ok(())
}

/// `get_dri`.
fn get_dri(input: &mut Input<'_>, header: &mut Header) -> Result<(), Error> {
    if input.src.word() != 4 {
        return Err(jerr::BAD_LENGTH);
    }
    header.restart_interval = input.src.word();
    Ok(())
}

/// `get_interesting_appn`: `APP0` and `APP14`, examined for the JFIF and
/// Adobe markers that say what colour space the components are in.
fn get_interesting_appn(input: &mut Input<'_>, header: &mut Header) {
    let marker = input.unread_marker;
    let mut length = i64::from(input.src.word()).wrapping_sub(2);
    let wanted = length.clamp(0, 14);
    let mut data = [0u8; 14];
    for slot in data.iter_mut().take(usize::try_from(wanted).unwrap_or(0)) {
        *slot = input.src.byte();
    }
    length = length.wrapping_sub(wanted);
    let read = usize::try_from(wanted).unwrap_or(0);
    if marker == 0xE0 {
        if read >= 14 && data.starts_with(b"JFIF\0") {
            header.saw_jfif = true;
            if data.get(5) != Some(&1) {
                // JWRN_JFIF_MAJOR.
                input.warn();
            }
        }
    } else if read >= 12 && data.starts_with(b"Adobe") {
        header.saw_adobe = true;
        header.adobe_transform = data.get(11).copied().unwrap_or(0);
    }
    if length > 0 {
        input.src.skip(length.unsigned_abs());
    }
}

/// `skip_variable`: a segment nothing here reads.
fn skip_variable(input: &mut Input<'_>) {
    let length = i64::from(input.src.word()).wrapping_sub(2);
    if length > 0 {
        input.src.skip(length.unsigned_abs());
    }
}

/// `read_restart_marker`, with `jpeg_resync_to_restart` for when the marker
/// found is not the one expected.
pub(super) fn read_restart_marker(input: &mut Input<'_>) {
    if input.unread_marker == 0 {
        next_marker(input);
    }
    let expected = 0xD0u8.wrapping_add(input.next_restart_num);
    if input.unread_marker == expected {
        input.unread_marker = 0;
    } else {
        resync_to_restart(input, input.next_restart_num);
    }
    input.next_restart_num = input.next_restart_num.wrapping_add(1) & 7;
}

/// `jpeg_resync_to_restart`: discard the marker, scan on to the next one, or
/// leave it for the entropy decoder to stop at, depending on how far it is
/// from the restart marker wanted.
fn resync_to_restart(input: &mut Input<'_>, desired: u8) {
    // JWRN_MUST_RESYNC.
    input.warn();
    let rst = |offset: u8| 0xD0u8.wrapping_add(desired.wrapping_add(offset) & 7);
    loop {
        let marker = input.unread_marker;
        // A valid marker that is not a restart, or one of the next two
        // restarts, is left for the entropy decoder to stop at.
        let action = if marker < 0xC0 {
            2
        } else if !(0xD0..=0xD7).contains(&marker) || marker == rst(1) || marker == rst(2) {
            3
        } else if marker == rst(7) || marker == rst(6) {
            2
        } else {
            1
        };
        match action {
            1 => {
                input.unread_marker = 0;
                return;
            }
            2 => next_marker(input),
            _ => return,
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn header_of(bytes: &[u8]) -> (Result<Reached, Error>, Header, Tables) {
        let mut input = Input::new(bytes);
        let mut header = Header::default();
        let mut tables = Tables::new();
        let reached = read_markers(&mut input, &mut header, &mut tables);
        (reached, header, tables)
    }

    #[test]
    fn a_datastream_must_start_with_soi() {
        assert_eq!(header_of(&[0x00, 0xFF, 0xD8]).0, Err(jerr::NO_SOI));
        assert_eq!(header_of(&[]).0, Err(jerr::NO_SOI));
    }

    #[test]
    fn tables_only_ends_at_eoi_and_keeps_the_tables() {
        let mut bytes = alloc::vec![0xFF, 0xD8, 0xFF, 0xDB, 0x00, 0x43, 0x01];
        bytes.extend((1..=64).map(|v: u8| v));
        bytes.extend([0xFF, 0xD9]);
        let (reached, _, tables) = header_of(&bytes);
        assert_eq!(reached, Ok(Reached::Eoi));
        let table = tables.quant[1].unwrap();
        // Zig-zag order into natural order: the third value is (1, 0).
        assert_eq!((table[0], table[1], table[8]), (1, 2, 3));
    }

    #[test]
    fn a_cut_header_ends_at_the_fake_end_of_image() {
        // SOI, then a COM segment claiming more than there is.
        let (reached, ..) = header_of(&[0xFF, 0xD8, 0xFF, 0xFE, 0x00, 0x40, 1, 2]);
        assert_eq!(reached, Ok(Reached::Eoi));
    }

    #[test]
    fn a_frame_is_read_and_a_scan_matched_to_it() {
        let bytes = [
            0xFF, 0xD8, 0xFF, 0xC0, 0x00, 0x11, 8, 0x00, 0x10, 0x00, 0x20, 3, 1, 0x22, 0, 2, 0x11,
            1, 3, 0x11, 1, 0xFF, 0xDA, 0x00, 0x0C, 3, 1, 0x00, 2, 0x11, 3, 0x11, 0, 63, 0,
        ];
        let (reached, header, _) = header_of(&bytes);
        assert_eq!(reached, Ok(Reached::Sos));
        assert_eq!((header.width, header.height), (32, 16));
        assert_eq!(header.scan.components(), &[0, 1, 2]);
        assert_eq!((header.components[0].h, header.components[0].v), (2, 2));
        assert_eq!(header.components[2].ac_tbl_no, 1);
        assert_eq!(header.input_scan_number, 1);
    }

    #[test]
    fn a_scan_may_not_name_a_component_before_its_own_position() {
        // Frame components 1, 2, 3; the scan lists 2 then 1. libjpeg only
        // looks at frame index >= scan position once earlier slots are taken.
        let bytes = [
            0xFF, 0xD8, 0xFF, 0xC0, 0x00, 0x11, 8, 0x00, 0x08, 0x00, 0x08, 3, 1, 0x11, 0, 2, 0x11,
            0, 3, 0x11, 0, 0xFF, 0xDA, 0x00, 0x0A, 2, 2, 0x00, 1, 0x00, 0, 63, 0,
        ];
        assert_eq!(header_of(&bytes).0, Err(jerr::BAD_COMPONENT_ID));
    }

    #[test]
    fn resync_discards_or_waits_as_libjpeg_decides() {
        // Expecting RST0, finding RST5: too far either way, discarded.
        let mut input = Input::new(&[]);
        input.unread_marker = 0xD5;
        read_restart_marker(&mut input);
        assert_eq!(input.unread_marker, 0);
        assert_eq!(input.next_restart_num, 1);
        // Expecting RST1, finding RST2: a later restart, left for the decoder.
        input.unread_marker = 0xD2;
        read_restart_marker(&mut input);
        assert_eq!(input.unread_marker, 0xD2);
        // Expecting RST2, finding RST1: an earlier one, so read on -- to the
        // fake end of image, which is then left in place.
        input.unread_marker = 0xD1;
        read_restart_marker(&mut input);
        assert_eq!(input.unread_marker, 0xD9);
    }
}
