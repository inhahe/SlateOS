//! Supplemental enhancement information, read as FFmpeg reads it
//! (`h264_sei.c`, and `h2645_sei.c` for the messages H.264 shares with
//! HEVC).
//!
//! Most of it is about a picture rather than in it -- its timing, how a
//! stereo pair is packed, which way up it is, its mastering display -- but
//! some of it changes what is decoded: a recovery point says from where a
//! stream that starts without an IDR picture is shown, and the x264 version
//! an encoder writes in its user data turns on FFmpeg's workarounds for
//! that encoder's old bugs. Film grain parameters are kept to be laid over
//! the picture as it is shown.
//!
//! What is read for one access unit is cleared before the next
//! ([`Sei::uninit`]), except the x264 version and the preferred transfer
//! characteristics, which last, as FFmpeg's do.

#![allow(
    clippy::indexing_slicing,
    reason = "indices are under the fixed sizes they index: time codes by the clock-timestamp \
              count (at most 3), film grain intervals and values by their read counts (at \
              most 256 and 6), checked first"
)]
#![allow(
    clippy::arithmetic_side_effects,
    reason = "byte positions within one unit, under i32::MAX; message types and sizes summed \
              from bytes, a byte at a time, within the unit"
)]

use crate::ps::{ParamSets, Sps};
use ffbits::Bits;

/// `SEI_TYPE_BUFFERING_PERIOD`.
const BUFFERING_PERIOD: u32 = 0;
/// `SEI_TYPE_PIC_TIMING`.
const PIC_TIMING: u32 = 1;
/// `SEI_TYPE_USER_DATA_REGISTERED_ITU_T_T35`.
const USER_DATA_REGISTERED: u32 = 4;
/// `SEI_TYPE_USER_DATA_UNREGISTERED`.
const USER_DATA_UNREGISTERED: u32 = 5;
/// `SEI_TYPE_RECOVERY_POINT`.
const RECOVERY_POINT: u32 = 6;
/// `SEI_TYPE_FILM_GRAIN_CHARACTERISTICS`.
const FILM_GRAIN_CHARACTERISTICS: u32 = 19;
/// `SEI_TYPE_FRAME_PACKING_ARRANGEMENT`.
const FRAME_PACKING_ARRANGEMENT: u32 = 45;
/// `SEI_TYPE_DISPLAY_ORIENTATION`.
const DISPLAY_ORIENTATION: u32 = 47;
/// `SEI_TYPE_GREEN_METADATA`.
const GREEN_METADATA: u32 = 56;
/// `SEI_TYPE_MASTERING_DISPLAY_COLOUR_VOLUME`.
const MASTERING_DISPLAY: u32 = 137;
/// `SEI_TYPE_CONTENT_LIGHT_LEVEL_INFO`.
const CONTENT_LIGHT_LEVEL: u32 = 144;
/// `SEI_TYPE_ALTERNATIVE_TRANSFER_CHARACTERISTICS`.
const ALTERNATIVE_TRANSFER: u32 = 147;
/// `SEI_TYPE_AMBIENT_VIEWING_ENVIRONMENT`.
const AMBIENT_VIEWING_ENVIRONMENT: u32 = 148;

/// `H264_SEI_PIC_STRUCT_*`: how a picture is to be shown -- a frame, one
/// field, two fields in either order, three (telecine), or a frame shown
/// two or three times.
#[allow(dead_code, reason = "the set whole: a frame is 0, which nothing names")]
pub(crate) const PIC_STRUCT_FRAME: u32 = 0;
pub(crate) const PIC_STRUCT_TOP_FIELD: u32 = 1;
pub(crate) const PIC_STRUCT_BOTTOM_FIELD: u32 = 2;
pub(crate) const PIC_STRUCT_TOP_BOTTOM: u32 = 3;
pub(crate) const PIC_STRUCT_BOTTOM_TOP: u32 = 4;
pub(crate) const PIC_STRUCT_TOP_BOTTOM_TOP: u32 = 5;
pub(crate) const PIC_STRUCT_BOTTOM_TOP_BOTTOM: u32 = 6;
pub(crate) const PIC_STRUCT_FRAME_DOUBLING: u32 = 7;
/// The last picture structure.
pub(crate) const PIC_STRUCT_FRAME_TRIPLING: u32 = 8;
/// How many clock timestamps each picture structure has
/// (`sei_num_clock_ts_table`).
const NUM_CLOCK_TS: [usize; 9] = [1, 1, 1, 2, 2, 3, 3, 2, 3];

/// Why the messages of a unit were not all read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SeiError {
    /// Damaged: the messages after it are not read
    /// (`AVERROR_INVALIDDATA`).
    Invalid,
    /// A buffering period naming a sequence parameter set not yet seen;
    /// the messages after it are still read (`AVERROR_PS_NOT_FOUND`).
    PsNotFound,
}

/// One clock timestamp of a picture timing message (`H264SEITimeCode`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct TimeCode {
    pub(crate) full: u32,
    pub(crate) frame: u32,
    pub(crate) seconds: u32,
    pub(crate) minutes: u32,
    pub(crate) hours: u32,
    pub(crate) dropframe: u32,
}

/// A picture timing message (`H264SEIPictureTiming`): kept as it came until
/// the slice says which sequence parameter set it is read against.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct PictureTiming {
    pub(crate) payload: Vec<u8>,
    pub(crate) present: bool,
    /// How the picture is shown: a frame, a field, fields in an order,
    /// repeated (`H264_SEI_PicStructType`).
    pub(crate) pic_struct: u32,
    /// The clock types the timestamps name, a bit each.
    pub(crate) ct_type: u32,
    pub(crate) timecode: [TimeCode; 3],
    pub(crate) timecode_cnt: usize,
}

/// A film grain characteristics message (`H2645SEIFilmGrainCharacteristics`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FilmGrain {
    pub(crate) present: bool,
    pub(crate) model_id: u32,
    pub(crate) log2_scale_factor: u32,
    pub(crate) comp_model_present_flag: [u32; 3],
    /// The rest in FFmpeg's types (`uint16_t`, `uint8_t`, `int16_t`): a
    /// model value past 16 bits is cut to them where it is stored, as
    /// FFmpeg's is, and film grain synthesis reads it so.
    pub(crate) num_intensity_intervals: [u16; 3],
    pub(crate) num_model_values: [u8; 3],
    /// A row for each of the three components.
    pub(crate) intensity_interval_lower_bound: Box<[[u8; 256]]>,
    pub(crate) intensity_interval_upper_bound: Box<[[u8; 256]]>,
    pub(crate) comp_model_value: Box<[[[i16; 6]; 256]]>,
    pub(crate) repetition_period: u32,
}

impl Default for FilmGrain {
    fn default() -> Self {
        Self {
            present: false,
            model_id: 0,
            log2_scale_factor: 0,
            comp_model_present_flag: [0; 3],
            num_intensity_intervals: [0; 3],
            num_model_values: [0; 3],
            intensity_interval_lower_bound: vec![[0; 256]; 3].into_boxed_slice(),
            intensity_interval_upper_bound: vec![[0; 256]; 3].into_boxed_slice(),
            comp_model_value: vec![[[0; 6]; 256]; 3].into_boxed_slice(),
            repetition_period: 0,
        }
    }
}

/// A frame packing arrangement message: how a stereo pair is packed into
/// one picture (`H2645SEIFramePacking`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FramePacking {
    pub(crate) present: bool,
    /// -1 until a message says, as the decoder starts it
    /// (`h264_init_context`).
    pub(crate) arrangement_cancel_flag: i32,
    pub(crate) arrangement_type: u32,
    pub(crate) content_interpretation_type: u32,
    pub(crate) quincunx_sampling_flag: u32,
    pub(crate) current_frame_is_frame0_flag: u32,
}

impl Default for FramePacking {
    fn default() -> Self {
        Self {
            present: false,
            arrangement_cancel_flag: -1,
            arrangement_type: 0,
            content_interpretation_type: 0,
            quincunx_sampling_flag: 0,
            current_frame_is_frame0_flag: 0,
        }
    }
}

/// A display orientation message: which way up the picture is shown
/// (`H2645SEIDisplayOrientation`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct DisplayOrientation {
    pub(crate) present: bool,
    pub(crate) anticlockwise_rotation: u32,
    pub(crate) hflip: u32,
    pub(crate) vflip: u32,
}

/// The display a picture was mastered on (`H2645SEIMasteringDisplay`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct MasteringDisplay {
    pub(crate) present: u32,
    pub(crate) display_primaries: [[u16; 2]; 3],
    pub(crate) white_point: [u16; 2],
    pub(crate) max_luminance: u32,
    pub(crate) min_luminance: u32,
}

/// The brightest light the content holds (`H2645SEIContentLight`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ContentLight {
    pub(crate) present: u32,
    pub(crate) max_content_light_level: u16,
    pub(crate) max_pic_average_light_level: u16,
}

/// The light a picture is meant to be watched in
/// (`H2645SEIAmbientViewingEnvironment`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct AmbientViewing {
    pub(crate) present: bool,
    pub(crate) ambient_illuminance: u32,
    pub(crate) ambient_light_x: u16,
    pub(crate) ambient_light_y: u16,
}

/// Everything an access unit's SEI said (`H264SEIContext` with its
/// `H2645SEI`).
#[derive(Clone, Debug)]
pub(crate) struct Sei {
    pub(crate) picture_timing: PictureTiming,
    /// Frames until the picture is right, from a recovery point; -1 for
    /// none.
    pub(crate) recovery_frame_cnt: i32,
    /// The x264 build that wrote the stream, from its user data; -1 for
    /// none. Kept from one access unit to the next.
    pub(crate) x264_build: i32,
    /// Each unregistered user data message's bytes, its UUID first.
    pub(crate) unregistered: Vec<Vec<u8>>,
    /// A/53 closed captions, three bytes a caption, fields merged.
    pub(crate) a53_caption: Vec<u8>,
    /// The active format description, when one came.
    pub(crate) afd: Option<u32>,
    /// LCEVC enhancement data, as it came.
    pub(crate) lcevc: Option<Vec<u8>>,
    pub(crate) film_grain: Option<Box<FilmGrain>>,
    pub(crate) frame_packing: FramePacking,
    pub(crate) display_orientation: DisplayOrientation,
    /// The preferred transfer characteristics, when an alternative was
    /// sent. Kept from one access unit to the next.
    pub(crate) alternative_transfer: Option<u32>,
    pub(crate) mastering_display: MasteringDisplay,
    pub(crate) content_light: ContentLight,
    pub(crate) ambient_viewing: AmbientViewing,
}

impl Default for Sei {
    fn default() -> Self {
        Self {
            picture_timing: PictureTiming::default(),
            recovery_frame_cnt: -1,
            x264_build: -1,
            unregistered: Vec::new(),
            a53_caption: Vec::new(),
            afd: None,
            lcevc: None,
            film_grain: None,
            frame_packing: FramePacking::default(),
            display_orientation: DisplayOrientation::default(),
            alternative_transfer: None,
            mastering_display: MasteringDisplay::default(),
            content_light: ContentLight::default(),
            ambient_viewing: AmbientViewing::default(),
        }
    }
}

/// A reader of bytes that answers zero past its end, as FFmpeg's
/// unchecked-by-default `bytestream2_get_byte` does.
struct Bytes<'a> {
    buf: &'a [u8],
    at: usize,
}

impl<'a> Bytes<'a> {
    fn left(&self) -> usize {
        self.buf.len().saturating_sub(self.at)
    }

    fn u8(&mut self) -> u8 {
        let v = self.buf.get(self.at).copied().unwrap_or(0);
        if self.at < self.buf.len() {
            self.at += 1;
        }
        v
    }

    fn be16(&mut self) -> u16 {
        if self.left() < 2 {
            self.at = self.buf.len();
            return 0;
        }
        u16::from_be_bytes([self.u8(), self.u8()])
    }

    fn be32(&mut self) -> u32 {
        if self.left() < 4 {
            self.at = self.buf.len();
            return 0;
        }
        u32::from_be_bytes([self.u8(), self.u8(), self.u8(), self.u8()])
    }

    fn rest(&self) -> &'a [u8] {
        self.buf.get(self.at..).unwrap_or(&[])
    }
}

impl Sei {
    /// Forget what the last access unit said (`ff_h264_sei_uninit`, with
    /// `ff_h2645_sei_reset`): exactly the fields those clear. The rest --
    /// a picture timing message's structure and time codes, the frame
    /// packing and orientation values, the mastering
    /// display's, the x264 version, the alternative transfer -- stay until
    /// a message overwrites them, as FFmpeg's do.
    pub(crate) fn uninit(&mut self) {
        self.recovery_frame_cnt = -1;
        self.picture_timing.present = false;
        self.frame_packing.present = false;
        self.display_orientation.present = false;
        self.afd = None;
        // ff_h2645_sei_reset.
        self.a53_caption.clear();
        self.unregistered.clear();
        self.lcevc = None;
        self.ambient_viewing.present = false;
        self.mastering_display.present = 0;
        self.content_light.present = 0;
        self.film_grain = None;
    }

    /// Read a unit's messages from where `gb` stands, after the unit's
    /// header (`ff_h264_sei_decode`). A damaged message stops the reading;
    /// what came before it is kept.
    pub(crate) fn decode(&mut self, gb: &Bits<'_>, ps: &ParamSets) -> Result<(), SeiError> {
        let start = (gb.count() / 8) as usize;
        let len = usize::try_from(gb.left() / 8).unwrap_or(0);
        let data = gb.buffer();
        let unit = data.get(start..start + len).unwrap_or(&[]);
        // What a message's bits run on into past its end: the rest of the
        // unit's buffer, as FFmpeg's reader reads it.
        let tail = data.get(start..).unwrap_or(&[]);
        let mut at = 0usize;
        let mut master: Result<(), SeiError> = Ok(());
        while unit.len() - at > 2 && (unit[at] != 0 || unit[at + 1] != 0) {
            let mut kind = 0u32;
            loop {
                let Some(&b) = unit.get(at) else {
                    return Err(SeiError::Invalid);
                };
                kind += u32::from(b);
                at += 1;
                if b != 255 {
                    break;
                }
            }
            let mut size = 0usize;
            loop {
                let Some(&b) = unit.get(at) else {
                    return Err(SeiError::Invalid);
                };
                size += usize::from(b);
                at += 1;
                if b != 255 {
                    break;
                }
            }
            if size > unit.len() - at {
                return Err(SeiError::Invalid);
            }
            let payload = &unit[at..at + size];
            let Some(mut pb) = Bits::new(&tail[at..], size * 8) else {
                return Err(SeiError::Invalid);
            };
            let mut bytes = Bytes {
                buf: payload,
                at: 0,
            };
            let r = match kind {
                PIC_TIMING => self.decode_picture_timing(payload),
                RECOVERY_POINT => self.decode_recovery_point(&mut pb),
                BUFFERING_PERIOD => Self::decode_buffering_period(&mut pb, ps),
                GREEN_METADATA => Ok(()), // read by FFmpeg for its log alone
                USER_DATA_REGISTERED => self.decode_registered(&mut bytes),
                USER_DATA_UNREGISTERED => self.decode_unregistered(payload),
                DISPLAY_ORIENTATION => {
                    self.decode_display_orientation(&mut pb);
                    Ok(())
                }
                FILM_GRAIN_CHARACTERISTICS => {
                    let mut fg = Box::<FilmGrain>::default();
                    let r = decode_film_grain(&mut fg, &mut pb);
                    self.film_grain = Some(fg);
                    r
                }
                FRAME_PACKING_ARRANGEMENT => {
                    self.decode_frame_packing(&mut pb);
                    Ok(())
                }
                ALTERNATIVE_TRANSFER => {
                    if payload.is_empty() {
                        Err(SeiError::Invalid)
                    } else {
                        self.alternative_transfer = Some(u32::from(payload[0]));
                        Ok(())
                    }
                }
                AMBIENT_VIEWING_ENVIRONMENT => self.decode_ambient_viewing(&mut bytes),
                MASTERING_DISPLAY => self.decode_mastering_display(&mut bytes),
                CONTENT_LIGHT_LEVEL => self.decode_content_light(&mut bytes),
                _ => Ok(()), // not handled
            };
            match r {
                Err(SeiError::Invalid) => return r,
                Err(SeiError::PsNotFound) => master = r,
                Ok(()) => {}
            }
            at += size;
        }
        master
    }

    /// `decode_picture_timing`: the payload kept as it is, at most 40
    /// bytes.
    fn decode_picture_timing(&mut self, payload: &[u8]) -> Result<(), SeiError> {
        if payload.len() > 40 {
            return Err(SeiError::Invalid);
        }
        self.picture_timing.payload = payload.to_vec();
        self.picture_timing.present = true;
        Ok(())
    }

    /// `decode_recovery_point`.
    fn decode_recovery_point(&mut self, gb: &mut Bits<'_>) -> Result<(), SeiError> {
        let cnt = gb.ue_long();
        if cnt >= 1 << 16 {
            return Err(SeiError::Invalid);
        }
        #[allow(clippy::cast_possible_wrap, reason = "under 2^16")]
        let cnt = cnt as i32;
        self.recovery_frame_cnt = cnt;
        // exact_match_flag, broken_link_flag, changing_slice_group_idc
        gb.skip(4);
        Ok(())
    }

    /// `decode_buffering_period`: read for the errors FFmpeg's gives -- a
    /// sequence parameter set out of range, or not yet seen. Its initial
    /// delays, which FFmpeg reads next into fields only its parser reads,
    /// are the message's last syntax, so they are not read here.
    fn decode_buffering_period(gb: &mut Bits<'_>, ps: &ParamSets) -> Result<(), SeiError> {
        let sps_id = gb.ue_31();
        if sps_id > 31 {
            return Err(SeiError::Invalid);
        }
        if ps
            .sps_list
            .get(sps_id as usize)
            .and_then(Option::as_deref)
            .is_none()
        {
            return Err(SeiError::PsNotFound);
        }
        Ok(())
    }

    /// `decode_registered_user_data` for H.264: ATSC's active format
    /// description and A/53 captions, and V-Nova's LCEVC; anything else is
    /// passed over.
    fn decode_registered(&mut self, gb: &mut Bytes<'_>) -> Result<(), SeiError> {
        if gb.left() < 3 {
            return Err(SeiError::Invalid);
        }
        let country_code = gb.u8();
        if country_code == 0xFF {
            if gb.left() < 3 {
                return Err(SeiError::Invalid);
            }
            gb.u8(); // itu_t_t35_country_code_extension_byte
        }
        match country_code {
            // ITU_T_T35_COUNTRY_CODE_US
            0xB5 => {
                let provider_code = gb.be16();
                // ITU_T_T35_PROVIDER_CODE_ATSC
                if provider_code == 0x0031 {
                    if gb.left() < 4 {
                        return Err(SeiError::Invalid);
                    }
                    match &gb.be32().to_be_bytes() {
                        b"DTG1" => return self.decode_afd(gb),
                        b"GA94" => return self.decode_a53(gb.rest()),
                        _ => {}
                    }
                }
            }
            // ITU_T_T35_COUNTRY_CODE_UK
            0xB4 => {
                gb.u8(); // t35_uk_country_code_second_octet
                if gb.left() < 2 {
                    return Err(SeiError::Invalid);
                }
                // ITU_T_T35_PROVIDER_CODE_VNOVA
                if gb.be16() == 0x5000 {
                    if gb.left() < 2 {
                        return Err(SeiError::Invalid);
                    }
                    self.lcevc = Some(gb.rest().to_vec());
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// `decode_registered_user_data_afd`.
    fn decode_afd(&mut self, gb: &mut Bytes<'_>) -> Result<(), SeiError> {
        if gb.left() == 0 {
            return Err(SeiError::Invalid);
        }
        if gb.u8() & 0x40 != 0 {
            if gb.left() == 0 {
                return Err(SeiError::Invalid);
            }
            self.afd = Some(u32::from(gb.u8() & 0xF));
        }
        Ok(())
    }

    /// `ff_parse_a53_cc`: three bytes a caption, appended to what the
    /// access unit's other fields brought.
    fn decode_a53(&mut self, data: &[u8]) -> Result<(), SeiError> {
        if data.len() < 3 {
            return Err(SeiError::Invalid);
        }
        if data[0] != 0x3 || data[1] & 0x40 == 0 {
            return Ok(());
        }
        let cc_count = usize::from(data[1] & 0x1F);
        if cc_count == 0 {
            return Ok(());
        }
        // Three bytes a caption, and a marker byte after them.
        if cc_count * 3 >= data.len() - 3 {
            return Err(SeiError::Invalid);
        }
        self.a53_caption
            .extend_from_slice(&data[3..3 + cc_count * 3]);
        Ok(())
    }

    /// `decode_unregistered_user_data`, and the x264 version in it.
    fn decode_unregistered(&mut self, payload: &[u8]) -> Result<(), SeiError> {
        if payload.len() < 16 {
            return Err(SeiError::Invalid);
        }
        self.unregistered.push(payload.to_vec());
        // The text after the 16-byte UUID, as the C string it is read as.
        let text = &payload[16..];
        let text = text
            .iter()
            .position(|&b| b == 0)
            .map_or(text, |end| &text[..end]);
        if let Some(build) = x264_core(text) {
            if build > 0 {
                self.x264_build = build;
            }
            if build == 1 && text.starts_with(b"x264 - core 0000") {
                self.x264_build = 67;
            }
        }
        Ok(())
    }

    /// `decode_display_orientation`.
    fn decode_display_orientation(&mut self, gb: &mut Bits<'_>) {
        let d = &mut self.display_orientation;
        d.present = gb.bit() == 0; // display_orientation_cancel_flag
        if d.present {
            d.hflip = gb.bit();
            d.vflip = gb.bit();
            d.anticlockwise_rotation = gb.bits(16);
        }
    }

    /// `decode_frame_packing_arrangement`.
    fn decode_frame_packing(&mut self, gb: &mut Bits<'_>) {
        let f = &mut self.frame_packing;
        gb.ue_long(); // frame_packing_arrangement_id
        f.arrangement_cancel_flag = i32::from(gb.flag());
        f.present = f.arrangement_cancel_flag == 0;
        if f.present {
            f.arrangement_type = gb.bits(7);
            f.quincunx_sampling_flag = gb.bit();
            f.content_interpretation_type = gb.bits(6);
            // spatial_flipping_flag, frame0_flipped_flag, field_views_flag
            gb.skip(3);
            f.current_frame_is_frame0_flag = gb.bit();
            // What follows -- the self-contained flags, the grid positions,
            // the repetition period -- FFmpeg reads into nothing it keeps
            // or into fields nothing reads, so it is not read here.
        }
    }

    /// `decode_ambient_viewing_environment`.
    fn decode_ambient_viewing(&mut self, gb: &mut Bytes<'_>) -> Result<(), SeiError> {
        const MAX_AMBIENT_LIGHT: u16 = 50000;
        if gb.left() < 8 {
            return Err(SeiError::Invalid);
        }
        let a = &mut self.ambient_viewing;
        a.ambient_illuminance = gb.be32();
        if a.ambient_illuminance == 0 {
            return Err(SeiError::Invalid);
        }
        a.ambient_light_x = gb.be16();
        if a.ambient_light_x > MAX_AMBIENT_LIGHT {
            return Err(SeiError::Invalid);
        }
        a.ambient_light_y = gb.be16();
        if a.ambient_light_y > MAX_AMBIENT_LIGHT {
            return Err(SeiError::Invalid);
        }
        a.present = true;
        Ok(())
    }

    /// `decode_nal_sei_mastering_display_info`.
    fn decode_mastering_display(&mut self, gb: &mut Bytes<'_>) -> Result<(), SeiError> {
        if gb.left() < 24 {
            return Err(SeiError::Invalid);
        }
        let m = &mut self.mastering_display;
        for p in &mut m.display_primaries {
            p[0] = gb.be16();
            p[1] = gb.be16();
        }
        m.white_point = [gb.be16(), gb.be16()];
        m.max_luminance = gb.be32();
        m.min_luminance = gb.be32();
        m.present = 2;
        Ok(())
    }

    /// `decode_nal_sei_content_light_info`.
    fn decode_content_light(&mut self, gb: &mut Bytes<'_>) -> Result<(), SeiError> {
        if gb.left() < 4 {
            return Err(SeiError::Invalid);
        }
        let c = &mut self.content_light;
        c.max_content_light_level = gb.be16();
        c.max_pic_average_light_level = gb.be16();
        c.present = 2;
        Ok(())
    }
}

/// `decode_film_grain_characteristics` for H.264.
fn decode_film_grain(h: &mut FilmGrain, gb: &mut Bits<'_>) -> Result<(), SeiError> {
    h.present = gb.bit() == 0; // film_grain_characteristics_cancel_flag
    if !h.present {
        return Ok(());
    }
    *h = FilmGrain::default();
    h.model_id = gb.bits(2);
    // The colour description and blending_mode_id FFmpeg passes on only to
    // a caller that asks for the grain's parameters rather than the grain.
    if gb.bit() != 0 {
        // separate_colour_description_present_flag
        gb.skip(3); // film_grain_bit_depth_luma_minus8
        gb.skip(3); // film_grain_bit_depth_chroma_minus8
        gb.skip(1); // film_grain_full_range_flag
        gb.skip(8); // film_grain_colour_primaries
        gb.skip(8); // film_grain_transfer_characteristics
        gb.skip(8); // film_grain_matrix_coefficients
    }
    gb.skip(2); // blending_mode_id
    h.log2_scale_factor = gb.bits(4);
    for c in 0..3 {
        h.comp_model_present_flag[c] = gb.bit();
    }
    for c in 0..3 {
        if h.comp_model_present_flag[c] == 0 {
            continue;
        }
        #[allow(clippy::cast_possible_truncation, reason = "8 and 3 bits, plus 1")]
        {
            h.num_intensity_intervals[c] = (gb.bits(8) + 1) as u16;
            h.num_model_values[c] = (gb.bits(3) + 1) as u8;
        }
        if h.num_model_values[c] > 6 {
            return Err(SeiError::Invalid);
        }
        for i in 0..usize::from(h.num_intensity_intervals[c]) {
            #[allow(
                clippy::cast_possible_truncation,
                reason = "8-bit bounds; the values cut to FFmpeg's int16_t"
            )]
            {
                h.intensity_interval_lower_bound[c][i] = gb.bits(8) as u8;
                h.intensity_interval_upper_bound[c][i] = gb.bits(8) as u8;
                for j in 0..usize::from(h.num_model_values[c]) {
                    h.comp_model_value[c][i][j] = gb.se_long() as i16;
                }
            }
        }
    }
    h.repetition_period = gb.ue_long();
    h.present = true;
    Ok(())
}

/// Read a picture timing message's payload against the sequence parameter
/// set it belongs to (`ff_h264_sei_process_picture_timing`).
pub(crate) fn process_picture_timing(pt: &mut PictureTiming, sps: &Sps) -> Result<(), SeiError> {
    let payload = pt.payload.clone();
    let Some(mut gb) = Bits::whole(&payload) else {
        return Err(SeiError::Invalid);
    };
    if sps.nal_hrd_parameters_present_flag != 0 || sps.vcl_hrd_parameters_present_flag != 0 {
        #[allow(clippy::cast_sign_loss, reason = "lengths of 1 to 32 bits")]
        let (cpb_len, dpb_len) = (
            sps.cpb_removal_delay_length as u32,
            sps.dpb_output_delay_length as u32,
        );
        gb.skip(cpb_len); // cpb_removal_delay
        gb.skip(dpb_len); // dpb_output_delay
    }
    if sps.pic_struct_present_flag != 0 {
        pt.pic_struct = gb.bits(4);
        pt.ct_type = 0;
        if pt.pic_struct > PIC_STRUCT_FRAME_TRIPLING {
            return Err(SeiError::Invalid);
        }
        pt.timecode_cnt = 0;
        for _ in 0..NUM_CLOCK_TS[pt.pic_struct as usize] {
            if gb.bit() == 0 {
                continue; // clock_timestamp_flag
            }
            let tc = &mut pt.timecode[pt.timecode_cnt];
            pt.timecode_cnt += 1;
            pt.ct_type |= 1 << gb.bits(2);
            gb.skip(1); // nuit_field_based_flag
            let counting_type = gb.bits(5);
            let full_timestamp_flag = gb.bit();
            gb.skip(1); // discontinuity_flag
            let cnt_dropped_flag = gb.bit();
            if cnt_dropped_flag != 0 && counting_type > 1 && counting_type < 7 {
                tc.dropframe = 1;
            }
            tc.frame = gb.bits(8);
            if full_timestamp_flag != 0 {
                tc.full = 1;
                tc.seconds = gb.bits(6);
                tc.minutes = gb.bits(6);
                tc.hours = gb.bits(5);
            } else {
                tc.seconds = 0;
                tc.minutes = 0;
                tc.hours = 0;
                tc.full = 0;
                if gb.bit() != 0 {
                    tc.seconds = gb.bits(6);
                    if gb.bit() != 0 {
                        tc.minutes = gb.bits(6);
                        if gb.bit() != 0 {
                            tc.hours = gb.bits(5);
                        }
                    }
                }
            }
            if sps.time_offset_length > 0 {
                #[allow(clippy::cast_sign_loss, reason = "a 5-bit length")]
                gb.skip(sps.time_offset_length as u32); // time_offset
            }
        }
    }
    Ok(())
}

/// `sscanf(text, "x264 - core %d")`: the build number when the text reads
/// so -- a space in the pattern standing for any run of white space, none
/// included, as `sscanf` takes it.
fn x264_core(text: &[u8]) -> Option<i32> {
    fn space(text: &[u8], at: &mut usize) {
        while text
            .get(*at)
            .is_some_and(|b| b" \t\n\x0B\x0C\r".contains(b))
        {
            *at += 1;
        }
    }
    // The pattern begins with a letter, not a space: the text must too.
    if !text.starts_with(b"x264") {
        return None;
    }
    let mut at = 4;
    for part in [&b"-"[..], b"core"] {
        space(text, &mut at);
        if !text.get(at..)?.starts_with(part) {
            return None;
        }
        at += part.len();
    }
    // %d: white space, a sign, digits.
    space(text, &mut at);
    let negative = match text.get(at) {
        Some(b'-') => {
            at += 1;
            true
        }
        Some(b'+') => {
            at += 1;
            false
        }
        _ => false,
    };
    let digits = text
        .get(at..)?
        .iter()
        .take_while(|b| b.is_ascii_digit())
        .count();
    if digits == 0 {
        return None;
    }
    let value = text[at..at + digits].iter().fold(0i64, |v, &d| {
        v.saturating_mul(10).saturating_add(i64::from(d - b'0'))
    });
    let value = if negative { -value } else { value };
    #[allow(
        clippy::cast_possible_truncation,
        reason = "C's conversion of an overlong %d, wrapped"
    )]
    let value = value as i32;
    Some(value)
}

#[cfg(test)]
mod tests;
