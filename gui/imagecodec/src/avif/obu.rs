//! The AV1 sequence header, read as libavif reads it before any decoding:
//! `avifSequenceHeaderParse` (`src/obu.c`, itself from dav1d).
//!
//! libavif looks for the sequence header at the start of a picture's first
//! frame to learn its colour -- the primaries, transfer, matrix and, above
//! all, whether its values use the full range or the "video" range -- when
//! the container has no `nclx` colour box to say. It reads the frame in
//! growing pieces until the header parses (`setup.rs`), and what it learns
//! stands: the range it finds here is the range the picture is converted
//! with, whatever the decoder later reports. So the reader must succeed and
//! fail exactly where libavif's does, bit reader included -- which reads
//! zeros past the end of its data and flags an error only on the byte after
//! the last.
//!
//! Portions of this file are copyright 2018 VideoLAN and dav1d authors and
//! 2018 Two Orioles, LLC, by way of libavif's `src/obu.c`, and used under
//! their BSD-2-Clause licence: `licenses/libavif-LICENSE.txt`.

/// What libavif takes from a sequence header: `avifSequenceHeader`'s colour
/// fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct SequenceColour {
    pub(super) primaries: u16,
    pub(super) transfer: u16,
    pub(super) matrix: u16,
    pub(super) full_range: bool,
}

/// `AVIF_COLOR_PRIMARIES_UNSPECIFIED` and its two siblings: 2 in H.273.
const UNSPECIFIED: u16 = 2;
const PRIMARIES_BT709: u16 = 1;
const TRANSFER_SRGB: u16 = 13;
const MATRIX_IDENTITY: u16 = 0;

/// `avifBits`: dav1d's `GetBits`.
struct Bits<'a> {
    data: &'a [u8],
    /// Bytes consumed: `ptr - start`.
    pos: usize,
    error: bool,
    eof: bool,
    state: u64,
    bits_left: u32,
}

impl<'a> Bits<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            pos: 0,
            error: false,
            eof: data.is_empty(),
            state: 0,
            bits_left: 0,
        }
    }

    /// `avifBitsReadPos`: bits consumed.
    fn read_pos(&self) -> u64 {
        u64::try_from(self.pos)
            .unwrap_or(u64::MAX)
            .saturating_mul(8)
            .saturating_sub(u64::from(self.bits_left))
    }

    /// `avifBitsRefill`: whole bytes until `n` bits are held, zeros past the
    /// end, and an error once a byte is wanted after the end was reached.
    fn refill(&mut self, n: u32) {
        let mut state = 0u64;
        loop {
            state = state.wrapping_shl(8);
            self.bits_left = self.bits_left.saturating_add(8);
            if !self.eof {
                state |= u64::from(self.data.get(self.pos).copied().unwrap_or(0));
                self.pos = self.pos.saturating_add(1);
            }
            if self.pos >= self.data.len() {
                self.error = self.eof;
                self.eof = true;
            }
            if n <= self.bits_left {
                break;
            }
        }
        // `bits_left` is at most 39 here: it was below `n` (at most 32)
        // before the last byte.
        self.state |= state.wrapping_shl(64u32.saturating_sub(self.bits_left));
    }

    /// `avifBitsRead`: `n` bits, 1 to 32.
    fn read(&mut self, n: u32) -> u32 {
        if n > self.bits_left {
            self.refill(n);
        }
        let state = self.state;
        self.bits_left = self.bits_left.saturating_sub(n);
        self.state = self.state.wrapping_shl(n);
        u32::try_from(state.wrapping_shr(64u32.saturating_sub(n))).unwrap_or(u32::MAX)
    }

    fn flag(&mut self) -> bool {
        self.read(1) == 1
    }

    /// `avifBitsReadUleb128`.
    fn uleb128(&mut self) -> u32 {
        let mut value = 0u64;
        let mut shift = 0u32;
        let mut more;
        loop {
            let byte = self.read(8);
            more = byte & 0x80 != 0;
            value |= u64::from(byte & 0x7F).wrapping_shl(shift);
            shift = shift.saturating_add(7);
            if !(more && shift < 56) {
                break;
            }
        }
        match u32::try_from(value) {
            Ok(value) if !more => value,
            _ => {
                self.error = true;
                0
            }
        }
    }

    /// `avifBitsReadVLC`: `uvlc()`, or all ones for 32 leading zeros.
    fn vlc(&mut self) -> u32 {
        let mut leading = 0u32;
        while self.read(1) == 0 {
            leading = leading.saturating_add(1);
            if leading == 32 {
                return u32::MAX;
            }
        }
        if leading == 0 {
            return 0;
        }
        1u32.wrapping_shl(leading)
            .wrapping_sub(1)
            .wrapping_add(self.read(leading))
    }
}

/// `avifSequenceHeaderParse` for AV1: the colour of the first sequence header
/// among the OBUs in `sample`, or `None` where libavif's parse fails.
pub(super) fn sequence_colour(sample: &[u8]) -> Option<SequenceColour> {
    let mut obus = sample;
    while !obus.is_empty() {
        let mut bits = Bits::new(obus);
        if bits.read(1) != 0 {
            return None; // obu_forbidden_bit
        }
        let obu_type = bits.read(4);
        let extension = bits.flag();
        let has_size = bits.flag();
        bits.read(1); // obu_reserved_1bit
        if extension {
            bits.read(8); // temporal_id, spatial_id, reserved
        }
        let obu_size = if has_size {
            bits.uleb128()
        } else {
            // libavif computes this in `int` and keeps it as `uint32_t`.
            let len = i64::try_from(obus.len()).unwrap_or(i64::MAX);
            let size = len.saturating_sub(1).saturating_sub(i64::from(extension));
            u32::try_from(size.rem_euclid(1 << 32)).unwrap_or(0)
        };
        if bits.error {
            return None;
        }
        let header_bytes = usize::try_from(bits.read_pos() >> 3).ok()?;
        let obu_size = usize::try_from(obu_size).ok()?;
        if obu_size > obus.len().checked_sub(header_bytes)? {
            return None;
        }
        let body_end = header_bytes.checked_add(obu_size)?;
        if obu_type == 1 {
            let body = obus.get(header_bytes..body_end)?;
            return parse_sequence_header(&mut Bits::new(body));
        }
        obus = obus.get(body_end..)?;
    }
    None
}

/// `parseAV1SequenceHeader`, as far as the colour: every field before it is
/// read for its length, and a field that rules the header out fails it.
fn parse_sequence_header(bits: &mut Bits<'_>) -> Option<SequenceColour> {
    // parseSequenceHeaderProfile
    let seq_profile = bits.read(3);
    if seq_profile > 2 || bits.error {
        return None;
    }
    // parseSequenceHeaderLevelIdxAndTier
    let still_picture = bits.flag();
    let reduced_still_picture_header = bits.flag();
    if reduced_still_picture_header && !still_picture {
        return None;
    }
    if reduced_still_picture_header {
        bits.read(5); // seq_level_idx[0]
    } else {
        let timing_info_present = bits.flag();
        let mut decoder_model_info_present = false;
        let mut buffer_delay_length = 0;
        if timing_info_present {
            bits.read(32); // num_units_in_display_tick
            bits.read(32); // time_scale
            if bits.flag() && bits.vlc() == u32::MAX {
                return None; // equal_picture_interval, num_ticks_per_picture_minus_1
            }
            decoder_model_info_present = bits.flag();
            if decoder_model_info_present {
                buffer_delay_length = bits.read(5).saturating_add(1);
                bits.read(32); // num_units_in_decoding_tick
                bits.read(10); // buffer_removal_time_length_minus_1, frame_presentation_time_length_minus_1
            }
        }
        let initial_display_delay_present = bits.flag();
        let operating_points = bits.read(5).saturating_add(1);
        for _ in 0..operating_points {
            bits.read(12); // operating_point_idc
            let seq_level_idx = bits.read(5);
            if seq_level_idx > 7 {
                bits.read(1); // seq_tier
            }
            if decoder_model_info_present && bits.flag() {
                bits.read(buffer_delay_length); // decoder_buffer_delay
                bits.read(buffer_delay_length); // encoder_buffer_delay
                bits.read(1); // low_delay_mode_flag
            }
            if initial_display_delay_present && bits.flag() {
                bits.read(4); // initial_display_delay_minus_1
            }
        }
    }
    if bits.error {
        return None;
    }
    // parseSequenceHeaderFrameMaxDimensions
    let frame_width_bits = bits.read(4).saturating_add(1);
    let frame_height_bits = bits.read(4).saturating_add(1);
    bits.read(frame_width_bits); // max_frame_width_minus_1
    bits.read(frame_height_bits); // max_frame_height_minus_1
    if !reduced_still_picture_header && bits.flag() {
        bits.read(7); // delta_frame_id_length_minus_2, additional_frame_id_length_minus_1
    }
    if bits.error {
        return None;
    }
    bits.read(1); // use_128x128_superblock
    // parseSequenceHeaderEnabledFeatures
    bits.read(2); // enable_filter_intra, enable_intra_edge_filter
    if !reduced_still_picture_header {
        bits.read(4); // enable_interintra_compound .. enable_dual_filter
        let enable_order_hint = bits.flag();
        if enable_order_hint {
            bits.read(2); // enable_jnt_comp, enable_ref_frame_mvs
        }
        let force_screen_content_tools = if bits.flag() { 2 } else { bits.read(1) };
        if force_screen_content_tools > 0 && !bits.flag() {
            bits.read(1); // seq_force_integer_mv
        }
        if enable_order_hint {
            bits.read(3); // order_hint_bits_minus_1
        }
    }
    if bits.error {
        return None;
    }
    bits.read(3); // enable_superres, enable_cdef, enable_restoration
    // parseSequenceHeaderColorConfig
    let high_bitdepth = bits.flag();
    let twelve_bit = seq_profile == 2 && high_bitdepth && bits.flag();
    let monochrome = seq_profile != 1 && bits.flag();
    let (primaries, transfer, matrix) = if bits.flag() {
        let primaries = narrow(bits.read(8));
        let transfer = narrow(bits.read(8));
        let matrix = narrow(bits.read(8));
        (primaries, transfer, matrix)
    } else {
        (UNSPECIFIED, UNSPECIFIED, UNSPECIFIED)
    };
    let full_range = if monochrome {
        bits.flag()
    } else if primaries == PRIMARIES_BT709 && transfer == TRANSFER_SRGB && matrix == MATRIX_IDENTITY
    {
        true
    } else {
        let full_range = bits.flag();
        let (subsampling_x, subsampling_y) = match seq_profile {
            0 => (true, true),
            1 => (false, false),
            _ => {
                if twelve_bit {
                    let x = bits.flag();
                    let y = x && bits.flag();
                    (x, y)
                } else {
                    (true, false)
                }
            }
        };
        if subsampling_x && subsampling_y {
            bits.read(2); // chroma_sample_position
        }
        full_range
    };
    if bits.error {
        return None;
    }
    if !monochrome {
        bits.read(1); // separate_uv_delta_q
    }
    bits.read(1); // film_grain_params_present
    if bits.error {
        return None;
    }
    Some(SequenceColour {
        primaries,
        transfer,
        matrix,
        full_range,
    })
}

/// An eight-bit field.
fn narrow(bits: u32) -> u16 {
    u16::try_from(bits).unwrap_or(u16::MAX)
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        clippy::cast_possible_truncation,
        reason = "tests assemble bitstreams by hand"
    )]

    use super::*;
    use alloc::vec::Vec;

    /// Bits, most significant first, into bytes (zero-padded).
    #[derive(Default)]
    struct Writer {
        bits: Vec<bool>,
    }

    impl Writer {
        fn put(&mut self, value: u32, count: u32) -> &mut Self {
            for i in (0..count).rev() {
                self.bits.push((value >> i) & 1 == 1);
            }
            self
        }

        fn bytes(&self) -> Vec<u8> {
            self.bits
                .chunks(8)
                .map(|chunk| {
                    chunk
                        .iter()
                        .enumerate()
                        .fold(0u8, |byte, (i, &bit)| byte | (u8::from(bit) << (7 - i)))
                })
                .collect()
        }
    }

    /// A reduced still-picture sequence header of profile 0 with the given
    /// colour description and range.
    fn still_header(colour: Option<(u32, u32, u32)>, full_range: bool) -> Vec<u8> {
        let mut w = Writer::default();
        w.put(0, 3); // seq_profile
        w.put(1, 1).put(1, 1); // still_picture, reduced_still_picture_header
        w.put(8, 5); // seq_level_idx
        w.put(15, 4).put(15, 4); // frame_width_bits_minus_1, frame_height_bits_minus_1
        w.put(63, 16).put(47, 16); // max frame size
        w.put(0, 1); // use_128x128_superblock
        w.put(0, 2); // enable_filter_intra, enable_intra_edge_filter
        w.put(0, 3); // enable_superres, enable_cdef, enable_restoration
        w.put(0, 1); // high_bitdepth
        w.put(0, 1); // mono_chrome
        match colour {
            Some((p, t, m)) => {
                w.put(1, 1).put(p, 8).put(t, 8).put(m, 8);
            }
            None => {
                w.put(0, 1);
            }
        }
        w.put(u32::from(full_range), 1); // color_range
        w.put(0, 2); // chroma_sample_position
        w.put(0, 1); // separate_uv_delta_q
        w.put(0, 1); // film_grain_params_present
        w.bytes()
    }

    /// An OBU of type `kind` with a size field.
    fn obu(kind: u8, body: &[u8]) -> Vec<u8> {
        let mut out = alloc::vec![(kind << 3) | 0b010];
        out.push(u8::try_from(body.len()).unwrap());
        out.extend_from_slice(body);
        out
    }

    #[test]
    fn the_colour_of_a_still_picture_header_is_found_after_other_obus() {
        let header = still_header(Some((9, 16, 9)), false);
        let mut sample = obu(2, &[]); // a temporal delimiter
        sample.extend(obu(1, &header));
        sample.extend(obu(6, &[0; 20])); // a frame
        assert_eq!(
            sequence_colour(&sample),
            Some(SequenceColour {
                primaries: 9,
                transfer: 16,
                matrix: 9,
                full_range: false
            })
        );
        // No colour description: unspecified, and the range as coded.
        let sample = obu(1, &still_header(None, true));
        assert_eq!(
            sequence_colour(&sample),
            Some(SequenceColour {
                primaries: 2,
                transfer: 2,
                matrix: 2,
                full_range: true
            })
        );
        // sRGB identity: full range without reading the flag.
        let sample = obu(1, &still_header(Some((1, 13, 0)), false));
        assert!(sequence_colour(&sample).unwrap().full_range);
    }

    #[test]
    fn a_header_cut_short_or_absent_is_not_found() {
        let header = still_header(Some((1, 1, 1)), true);
        let sample = obu(1, &header);
        // The OBU's size says more than there is.
        assert_eq!(sequence_colour(&sample[..sample.len() - 1]), None);
        // The header itself cut short, inside a well-formed OBU.
        assert_eq!(sequence_colour(&obu(1, &header[..3])), None);
        // No sequence header; the forbidden bit; nothing at all.
        assert_eq!(sequence_colour(&obu(6, &[0; 4])), None);
        assert_eq!(sequence_colour(&[0x80]), None);
        assert_eq!(sequence_colour(&[]), None);
    }

    #[test]
    fn an_obu_without_a_size_runs_to_the_end() {
        let header = still_header(Some((1, 1, 1)), true);
        let mut sample = alloc::vec![1 << 3]; // type 1, no size field
        sample.extend_from_slice(&header);
        assert!(sequence_colour(&sample).is_some());
    }

    #[test]
    fn the_bit_reader_flags_only_reads_past_the_end() {
        let mut bits = Bits::new(&[0xA5]);
        assert_eq!(bits.read(8), 0xA5);
        assert!(!bits.error);
        assert_eq!(bits.read(1), 0);
        assert!(bits.error);
        let mut bits = Bits::new(&[0x81, 0x01]);
        assert_eq!(bits.uleb128(), 129);
        assert!(!bits.error);
        let mut bits = Bits::new(&[0xFF; 9]);
        assert_eq!(bits.uleb128(), 0);
        assert!(bits.error);
    }
}
