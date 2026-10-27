//! The decoder's state, as FFmpeg's `AACDecContext` holds it with the
//! structures under it (`aacdec.h`), and the code that sets its channel
//! elements up for a stream's configuration (`aacdec.c`: `che_configure`,
//! `frame_configure_elements`, `push_output_configuration` and
//! `pop_output_configuration`, `ff_aac_output_configure`,
//! `ff_aac_get_che`).
//!
//! An *element* is one of a frame's syntax elements that carries sound -- a
//! single channel, a pair, a coupling channel, an LFE -- together with the
//! state its channels keep from frame to frame. The decoder owns them in
//! `che[type][index]`, allocated as a configuration names them. Where FFmpeg
//! points into that table -- the element each tag in a frame decodes into
//! (`tag_che_map`), the element channel each output channel comes from
//! (`output_element`), the buffer each channel's samples go to (`output`)
//! -- this holds the type and index instead, and clears them where FFmpeg
//! clears its pointers: as the element they name is freed. So a reference
//! here names the element FFmpeg's pointer would.
//!
//! The *output configuration* is what the decoder has decided about the
//! stream's channels: the AudioSpecificConfig's fields, the layout map and
//! the channel layout made from it, and how settled it is -- from a global
//! header, on trial from one frame's ADTS header or program config element,
//! or locked once a frame decodes with it. `oc[1]` is the one in force;
//! `oc[0]` the one before, which a frame that fails on a trial configuration
//! falls back to.

#![allow(
    clippy::indexing_slicing,
    reason = "element types are under 4 and element ids under MAX_ELEM_ID, checked where a \
              layout map is taken in; channels are under MAX_CHANNELS, checked as they are \
              counted; a layout map's tags are under its length, checked on entry"
)]
#![allow(
    clippy::arithmetic_side_effects,
    reason = "counts of channels, tags and mapped elements, each under 256"
)]

use crate::Error;
use crate::layout::{
    CHANNEL_FRONT, LayoutMap, MAX_CHANNELS, MAX_ELEM_ID, TAGS_PER_CONFIG, TYPE_CCE, TYPE_CPE,
    TYPE_END, TYPE_LFE, TYPE_SCE, default_channel_config, sniff_channel_order,
};
use crate::mpeg4audio::Config;
use crate::softfloat::SoftFloat;

/// The most coefficients a channel's temporal noise shaping filter has, and
/// the most bands a long window predicts from the past (`TNS_MAX_ORDER`,
/// `MAX_LTP_LONG_SFB`).
pub(crate) const TNS_MAX_ORDER: usize = 20;
pub(crate) const MAX_LTP_LONG_SFB: usize = 40;

/// The most spectral lines AAC Main's prediction runs over
/// (`MAX_PREDICTORS`).
pub(crate) const MAX_PREDICTORS: usize = 672;

/// A channel's window sequence (`enum WindowSequence`).
pub(crate) const ONLY_LONG_SEQUENCE: u8 = 0;
pub(crate) const LONG_START_SEQUENCE: u8 = 1;
pub(crate) const EIGHT_SHORT_SEQUENCE: u8 = 2;
pub(crate) const LONG_STOP_SEQUENCE: u8 = 3;

/// How a scalefactor band is coded (`enum BandType`): no spectrum, the
/// codebooks 1 to 11 (from 5 on, pairs of values rather than fours), noise,
/// or intensity stereo against the pair's other channel, in phase or out.
pub(crate) const ZERO_BT: u8 = 0;
pub(crate) const FIRST_PAIR_BT: u8 = 5;
pub(crate) const ESC_BT: u8 = 11;
pub(crate) const RESERVED_BT: u8 = 12;
pub(crate) const NOISE_BT: u8 = 13;
pub(crate) const INTENSITY_BT2: u8 = 14;
pub(crate) const INTENSITY_BT: u8 = 15;

/// Where in a channel's decoding a coupling channel is applied
/// (`enum CouplingPoint`).
pub(crate) const BEFORE_TNS: i32 = 0;
pub(crate) const BETWEEN_TNS_AND_IMDCT: i32 = 1;
pub(crate) const AFTER_IMDCT: i32 = 3;

/// libavutil's channel-layout orders (`enum AVChannelOrder`).
pub(crate) const ORDER_UNSPEC: i32 = 0;
pub(crate) const ORDER_NATIVE: i32 = 1;

/// libavutil's masks for the front centre channel, and for stereo
/// (`AV_CH_FRONT_CENTER`, `AV_CH_LAYOUT_STEREO`).
pub(crate) const AV_CH_FRONT_CENTER: u64 = 1 << 2;
pub(crate) const AV_CH_LAYOUT_STEREO: u64 = 0b11;

/// libavcodec's error-recognition flags the decoder reads
/// (`AV_EF_CRCCHECK`, `AV_EF_BITSTREAM`, `AV_EF_EXPLODE`); the first is its
/// default.
pub(crate) const EF_CRCCHECK: i32 = 1 << 0;
pub(crate) const EF_BITSTREAM: i32 = 1 << 1;
pub(crate) const EF_EXPLODE: i32 = 1 << 3;

/// How settled an output configuration is (`enum OCStatus`), in the order
/// FFmpeg compares them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum OcStatus {
    /// None yet.
    #[default]
    None,
    /// On trial, from a frame's program config element.
    TrialPce,
    /// On trial, from a frame's header or its elements.
    TrialFrame,
    /// From the global header, before a frame has decoded with it.
    GlobalHdr,
    /// Locked in by a frame that decoded with it.
    Locked,
}

/// The order the output's channels come in (`enum AACOutputChannelOrder`):
/// libavutil's, or the order the stream codes them in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ChannelOrder {
    #[default]
    Default,
    Coded,
}

/// A channel layout, as much of libavutil's `AVChannelLayout` as the
/// decoder makes: its order, its channels, and -- in the native order --
/// the mask naming them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ChannelLayout {
    pub(crate) order: i32,
    pub(crate) nb_channels: i32,
    pub(crate) mask: u64,
}

impl ChannelLayout {
    /// The layout `mask` names, in the native order
    /// (`av_channel_layout_from_mask`, called with a mask that is not 0).
    #[allow(
        clippy::cast_possible_wrap,
        reason = "a popcount of 64 bits is at most 64"
    )]
    pub(crate) fn from_mask(mask: u64) -> Self {
        Self {
            order: ORDER_NATIVE,
            nb_channels: mask.count_ones() as i32,
            mask,
        }
    }

    /// Whether it is a layout a frame may carry (`av_channel_layout_check`,
    /// for the orders the decoder makes and is given).
    pub(crate) fn is_valid(&self) -> bool {
        if self.nb_channels <= 0 {
            return false;
        }
        match self.order {
            ORDER_UNSPEC => true,
            ORDER_NATIVE => i64::from(self.mask.count_ones()) == i64::from(self.nb_channels),
            _ => false,
        }
    }
}

/// What the decoder reads and sets of its codec context (`AVCodecContext`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CodecContext {
    /// The output's channels: what the decoder was opened with, then what
    /// each configuration makes them.
    pub(crate) ch_layout: ChannelLayout,
    /// The output's sample rate: what the decoder was opened with, then
    /// what the first frame of each locked configuration makes it.
    pub(crate) sample_rate: i32,
    /// Samples a channel per frame, once a frame has decoded.
    pub(crate) frame_size: i32,
    /// The profile the stream decodes as: its object type less 1, or HE-AAC
    /// (4) or HE-AAC v2 (28) once its SBR data says so.
    pub(crate) profile: i32,
    /// `strict_std_compliance >= FF_COMPLIANCE_STRICT`: channel
    /// configuration 7 read as the standard has it.
    pub(crate) strict: bool,
    /// The `EF_*` flags.
    pub(crate) err_recognition: i32,
}

impl Default for CodecContext {
    fn default() -> Self {
        Self {
            ch_layout: ChannelLayout::default(),
            sample_rate: 0,
            frame_size: 0,
            profile: -99,
            strict: false,
            err_recognition: EF_CRCCHECK,
        }
    }
}

/// The frame being decoded into (`ac->frame`): what the decoder reads and
/// sets of FFmpeg's `AVFrame`.
#[derive(Debug, Default)]
pub(crate) struct Frame {
    /// Whether it has buffers (`data[0]` set).
    pub(crate) allocated: bool,
    /// Its channels: the codec context's as its buffers were got.
    pub(crate) ch_layout: ChannelLayout,
    pub(crate) nb_samples: i32,
    pub(crate) sample_rate: i32,
    /// A plane of 2048 samples for each channel. Planes are kept from frame
    /// to frame, as FFmpeg's buffer pool keeps them; a channel no element
    /// writes keeps what was there.
    pub(crate) planes: Vec<Box<[i32; 2048]>>,
}

impl Frame {
    /// Let go of the buffers (`av_frame_unref`).
    pub(crate) fn unref(&mut self) {
        self.allocated = false;
        self.ch_layout = ChannelLayout::default();
        self.nb_samples = 0;
        self.sample_rate = 0;
    }

    /// Buffers of `nb_samples` for each of the codec context's channels
    /// (`ff_get_buffer` on a frame with none).
    fn get_buffer(&mut self, avctx: &CodecContext, nb_samples: i32) {
        self.ch_layout = avctx.ch_layout;
        self.nb_samples = nb_samples;
        self.sample_rate = avctx.sample_rate;
        let channels = usize::try_from(avctx.ch_layout.nb_channels).unwrap_or(0);
        while self.planes.len() < channels {
            self.planes.push(Box::new([0; 2048]));
        }
        self.allocated = true;
    }
}

/// An element, by its type and its index among the elements of that type:
/// what FFmpeg's `ChannelElement *` into `che` is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ElemRef {
    pub(crate) ty: u8,
    pub(crate) index: u8,
}

/// One channel of an element: what FFmpeg's `SingleChannelElement *` into
/// an element's `ch` is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ChRef {
    pub(crate) elem: ElemRef,
    pub(crate) ch: u8,
}

/// Where a channel's samples go (`output`): nowhere yet, its element's own
/// buffer, or a plane of the frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Output {
    #[default]
    Unset,
    RetBuf,
    Plane(u8),
}

/// Long-term prediction's parameters for a channel (`LongTermPrediction`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LongTermPrediction {
    pub(crate) present: i8,
    pub(crate) lag: i16,
    pub(crate) coef: i32,
    pub(crate) used: [i8; MAX_LTP_LONG_SFB],
}

impl Default for LongTermPrediction {
    fn default() -> Self {
        Self {
            present: 0,
            lag: 0,
            coef: 0,
            used: [0; MAX_LTP_LONG_SFB],
        }
    }
}

/// A channel's stream information: its windows, their grouping, its bands
/// (`IndividualChannelStream`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct IndividualChannelStream {
    /// Scalefactor bands coded in each group.
    pub(crate) max_sfb: u8,
    /// This frame's window sequence and the last's.
    pub(crate) window_sequence: [u8; 2],
    /// Whether this frame's window shape, and the last's, is Kaiser-Bessel
    /// rather than sine.
    pub(crate) use_kb_window: [u8; 2],
    pub(crate) num_window_groups: i32,
    pub(crate) prev_num_window_groups: i32,
    pub(crate) group_len: [u8; 8],
    pub(crate) ltp: LongTermPrediction,
    /// Where each scalefactor band starts, for a window: one of the
    /// standard's tables, or none until a frame has set one.
    pub(crate) swb_offset: &'static [u16],
    pub(crate) num_swb: i32,
    pub(crate) num_windows: i32,
    pub(crate) tns_max_bands: i32,
    pub(crate) predictor_present: i32,
    pub(crate) predictor_initialized: i32,
    pub(crate) predictor_reset_group: i32,
    pub(crate) prediction_used: [u8; 41],
    /// Which windows came near clipping.
    pub(crate) window_clipping: [u8; 8],
}

impl Default for IndividualChannelStream {
    fn default() -> Self {
        Self {
            max_sfb: 0,
            window_sequence: [0; 2],
            use_kb_window: [0; 2],
            num_window_groups: 0,
            prev_num_window_groups: 0,
            group_len: [0; 8],
            ltp: LongTermPrediction::default(),
            swb_offset: &[],
            num_swb: 0,
            num_windows: 0,
            tns_max_bands: 0,
            predictor_present: 0,
            predictor_initialized: 0,
            predictor_reset_group: 0,
            prediction_used: [0; 41],
            window_clipping: [0; 8],
        }
    }
}

/// A channel's temporal noise shaping filters, up to 4 a window
/// (`TemporalNoiseShaping`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TemporalNoiseShaping {
    pub(crate) present: i32,
    pub(crate) n_filt: [i32; 8],
    pub(crate) length: [[i32; 4]; 8],
    pub(crate) direction: [[i32; 4]; 8],
    pub(crate) order: [[i32; 4]; 8],
    pub(crate) coef: [[[i32; TNS_MAX_ORDER]; 4]; 8],
}

impl Default for TemporalNoiseShaping {
    fn default() -> Self {
        Self {
            present: 0,
            n_filt: [0; 8],
            length: [[0; 4]; 8],
            direction: [[0; 4]; 8],
            order: [[0; 4]; 8],
            coef: [[[0; TNS_MAX_ORDER]; 4]; 8],
        }
    }
}

/// A coupling channel's targets and gains (`ChannelCoupling`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ChannelCoupling {
    /// When it is applied: [`BEFORE_TNS`], [`BETWEEN_TNS_AND_IMDCT`] or
    /// [`AFTER_IMDCT`].
    pub(crate) coupling_point: i32,
    /// How many targets, less one.
    pub(crate) num_coupled: i32,
    /// Each target's element type, SCE or CPE, and id.
    pub(crate) ty: [u8; 8],
    pub(crate) id_select: [i32; 8],
    /// Which of a target pair's channels take a gain list: 0 one list for
    /// both, 1 the right's, 2 the left's, 3 one each.
    pub(crate) ch_select: [i32; 8],
    pub(crate) gain: Box<[[i32; 120]; 16]>,
}

impl Default for ChannelCoupling {
    fn default() -> Self {
        Self {
            coupling_point: BEFORE_TNS,
            num_coupled: 0,
            ty: [0; 8],
            id_select: [0; 8],
            ch_select: [0; 8],
            gain: Box::new([[0; 120]; 16]),
        }
    }
}

/// AAC Main's predictor state for one spectral line, in libavutil's
/// soft-float (`PredictorStateFixed`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct PredictorState {
    pub(crate) cor0: SoftFloat,
    pub(crate) cor1: SoftFloat,
    pub(crate) var0: SoftFloat,
    pub(crate) var1: SoftFloat,
    pub(crate) r0: SoftFloat,
    pub(crate) r1: SoftFloat,
    pub(crate) k1: SoftFloat,
    pub(crate) x_est: SoftFloat,
}

/// One channel of an element, and what it keeps from frame to frame
/// (`SingleChannelElement`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SingleChannelElement {
    pub(crate) ics: IndividualChannelStream,
    pub(crate) tns: TemporalNoiseShaping,
    /// Each band's [`ZERO_BT`]-style coding.
    pub(crate) band_type: [u8; 128],
    /// Each band's scalefactor offset.
    pub(crate) sfo: [i32; 128],
    /// Each band's scalefactor (8 windows by 16 bands at most).
    pub(crate) sf: [i32; 128],
    /// The spectrum, dequantized, then processed toward the inverse MDCT.
    pub(crate) coeffs: Box<[i32; 1024]>,
    /// The last frame's second half, overlapped with this one's first.
    pub(crate) saved: Box<[i32; 1536]>,
    /// The time signal long-term prediction predicts from.
    pub(crate) ltp_state: Box<[i32; 3072]>,
    /// AAC Main's predictors, [`MAX_PREDICTORS`] of them (in FFmpeg,
    /// allocated beside the element).
    pub(crate) predictor_state: Box<[PredictorState]>,
    /// Where the channel's samples go.
    pub(crate) output: Output,
}

impl Default for SingleChannelElement {
    fn default() -> Self {
        Self {
            ics: IndividualChannelStream::default(),
            tns: TemporalNoiseShaping::default(),
            band_type: [0; 128],
            sfo: [0; 128],
            sf: [0; 128],
            coeffs: Box::new([0; 1024]),
            saved: Box::new([0; 1536]),
            ltp_state: Box::new([0; 3072]),
            // Built on the heap: at 43 KiB it is no array for the stack.
            predictor_state: vec![PredictorState::default(); MAX_PREDICTORS].into_boxed_slice(),
            output: Output::Unset,
        }
    }
}

/// An element: a single channel, a pair, a coupling channel or an LFE
/// (`ChannelElement`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ChannelElement {
    /// Whether this frame has coded it.
    pub(crate) present: i32,
    /// Whether each band of a pair is coded as mid and side.
    pub(crate) ms_mask: [u8; 128],
    pub(crate) ch: [SingleChannelElement; 2],
    pub(crate) coup: ChannelCoupling,
    /// Each channel's samples where the frame has no plane for it
    /// (`ret_buf`, kept here rather than in the channel so that a channel
    /// and the buffer its samples go to can be borrowed together).
    pub(crate) ret_buf: [Box<[i32; 2048]>; 2],
}

impl ChannelElement {
    /// A new element, everything zero, as FFmpeg allocates one
    /// (`ff_aac_sbr_ctx_alloc_init`).
    pub(crate) fn new() -> Box<Self> {
        Box::new(Self {
            present: 0,
            ms_mask: [0; 128],
            ch: [
                SingleChannelElement::default(),
                SingleChannelElement::default(),
            ],
            coup: ChannelCoupling::default(),
            ret_buf: [Box::new([0; 2048]), Box::new([0; 2048])],
        })
    }
}

/// Dynamic range control as the stream carries it, read and kept but not
/// applied (`DynamicRangeControl`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DynamicRangeControl {
    pub(crate) pce_instance_tag: i32,
    pub(crate) dyn_rng_sgn: [i32; 17],
    pub(crate) dyn_rng_ctl: [i32; 17],
    pub(crate) exclude_mask: [i32; MAX_CHANNELS],
    pub(crate) band_incr: i32,
    pub(crate) interpolation_scheme: i32,
    pub(crate) band_top: [i32; 17],
    pub(crate) prog_ref_level: i32,
}

impl Default for DynamicRangeControl {
    fn default() -> Self {
        Self {
            pce_instance_tag: 0,
            dyn_rng_sgn: [0; 17],
            dyn_rng_ctl: [0; 17],
            exclude_mask: [0; MAX_CHANNELS],
            band_incr: 0,
            interpolation_scheme: 0,
            band_top: [0; 17],
            prog_ref_level: 0,
        }
    }
}

/// An output configuration (`OutputConfiguration`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct OutputConfiguration {
    pub(crate) m4ac: Config,
    /// The layout map, as it was given -- in the order the configuration
    /// listed its elements, not the sorted one, except after a fall back to
    /// it (see [`Context::output_configure`]).
    pub(crate) layout_map: LayoutMap,
    pub(crate) layout_map_tags: usize,
    pub(crate) ch_layout: ChannelLayout,
    pub(crate) status: OcStatus,
}

impl Default for OutputConfiguration {
    fn default() -> Self {
        Self {
            m4ac: Config::default(),
            layout_map: [[0; 3]; MAX_ELEM_ID * 4],
            layout_map_tags: 0,
            ch_layout: ChannelLayout::default(),
            status: OcStatus::None,
        }
    }
}

/// The decoder (`AACDecContext`), less what is translated with the parts
/// that use it: the transforms, and the SBR state each element carries.
#[derive(Debug)]
pub(crate) struct Context {
    pub(crate) avctx: CodecContext,
    pub(crate) frame: Frame,
    pub(crate) che_drc: DynamicRangeControl,
    /// The elements, by type and index.
    pub(crate) che: [[Option<Box<ChannelElement>>; MAX_ELEM_ID]; 4],
    /// The element each type and tag in a frame decodes into.
    pub(crate) tag_che_map: [[Option<ElemRef>; MAX_ELEM_ID]; 4],
    /// How many of a frame's elements have been mapped to one of a channel
    /// configuration's (`ff_aac_get_che`).
    pub(crate) tags_mapped: i32,
    pub(crate) buf_mdct: Box<[i32; 1024]>,
    pub(crate) temp: [i32; 128],
    /// The noise generator's state (`random_state`).
    pub(crate) random_state: i32,
    /// The element channel each output channel comes from.
    pub(crate) output_element: [Option<ChRef>; MAX_CHANNELS],
    /// Japanese digital TV's dual mono: which channel to play, if forced
    /// (-1 not), and for this frame (0 both, 1 the first, 2 the second).
    pub(crate) force_dmono_mode: i32,
    pub(crate) dmono_mode: i32,
    pub(crate) output_channel_order: ChannelOrder,
    pub(crate) oc: [OutputConfiguration; 2],
}

impl Context {
    /// A decoder with nothing configured, its codec context as given.
    pub(crate) fn new(avctx: CodecContext) -> Self {
        Self {
            avctx,
            frame: Frame::default(),
            che_drc: DynamicRangeControl::default(),
            che: [const { [const { None }; MAX_ELEM_ID] }; 4],
            tag_che_map: [[None; MAX_ELEM_ID]; 4],
            tags_mapped: 0,
            buf_mdct: Box::new([0; 1024]),
            temp: [0; 128],
            random_state: 0,
            output_element: [None; MAX_CHANNELS],
            force_dmono_mode: -1,
            dmono_mode: 0,
            output_channel_order: ChannelOrder::Default,
            oc: [OutputConfiguration::default(); 2],
        }
    }

    /// The element `elem` names, if it is allocated.
    pub(crate) fn elem(&self, elem: ElemRef) -> Option<&ChannelElement> {
        self.che
            .get(usize::from(elem.ty))?
            .get(usize::from(elem.index))?
            .as_deref()
    }

    /// The element `elem` names, if it is allocated, to change.
    pub(crate) fn elem_mut(&mut self, elem: ElemRef) -> Option<&mut ChannelElement> {
        self.che
            .get_mut(usize::from(elem.ty))?
            .get_mut(usize::from(elem.index))?
            .as_deref_mut()
    }

    /// A reference to element `ty`, `index` if it is allocated, as FFmpeg's
    /// pointer to it is null if not.
    fn elem_ref(&self, ty: u8, index: usize) -> Option<ElemRef> {
        let r = ElemRef {
            ty,
            index: u8::try_from(index).ok()?,
        };
        self.elem(r).map(|_| r)
    }

    /// Allocate the element of type `ty` and index `index` if position
    /// `che_pos` is one, and give its channels the next output channels
    /// after `*channels` unless it is a coupling channel; free it if the
    /// position is off (`che_configure`).
    fn che_configure(
        &mut self,
        che_pos: u8,
        ty: u8,
        index: usize,
        channels: &mut usize,
    ) -> Result<(), Error> {
        if *channels >= MAX_CHANNELS {
            return Err(Error::InvalidData);
        }
        let slot = &mut self.che[usize::from(ty)][index];
        if che_pos != 0 {
            if slot.is_none() {
                *slot = Some(ChannelElement::new());
            }
            if ty != TYPE_CCE {
                let pair = ty == TYPE_CPE || (ty == TYPE_SCE && self.oc[1].m4ac.ps == 1);
                // "Too many channels".
                if *channels >= MAX_CHANNELS - usize::from(pair) {
                    return Err(Error::InvalidData);
                }
                let elem = ElemRef {
                    ty,
                    index: u8::try_from(index).map_err(|_| Error::Bug)?,
                };
                self.output_element[*channels] = Some(ChRef { elem, ch: 0 });
                *channels += 1;
                if pair {
                    self.output_element[*channels] = Some(ChRef { elem, ch: 1 });
                    *channels += 1;
                }
            }
        } else {
            if slot.is_some() {
                let freed = Some(ElemRef {
                    ty,
                    index: u8::try_from(index).map_err(|_| Error::Bug)?,
                });
                for tag in self.tag_che_map.iter_mut().flatten() {
                    if *tag == freed {
                        *tag = None;
                    }
                }
            }
            self.che[usize::from(ty)][index] = None;
            self.output_element = [None; MAX_CHANNELS];
        }
        Ok(())
    }

    /// Point every element's channels at their own buffers, then get the
    /// frame's buffers and point the output channels' element channels at
    /// theirs (`frame_configure_elements`). With no output channels the
    /// frame is left without buffers.
    pub(crate) fn frame_configure_elements(&mut self) {
        for che in self.che.iter_mut().flatten().flatten() {
            che.ch[0].output = Output::RetBuf;
            che.ch[1].output = Output::RetBuf;
        }
        self.frame.unref();
        let channels = usize::try_from(self.avctx.ch_layout.nb_channels).unwrap_or(0);
        if channels == 0 {
            return;
        }
        self.frame.get_buffer(&self.avctx, 2048);
        for ch in 0..channels.min(MAX_CHANNELS) {
            let Some(target) = self.output_element[ch] else {
                continue;
            };
            let Ok(plane) = u8::try_from(ch) else {
                continue;
            };
            if let Some(che) = self.elem_mut(target.elem) {
                che.ch[usize::from(target.ch & 1)].output = Output::Plane(plane);
            }
        }
    }

    /// Keep the configuration in force as the one to fall back to, if it is
    /// locked or there is none yet, and put it on no status
    /// (`push_output_configuration`). Answers whether it was kept.
    pub(crate) fn push_output_configuration(&mut self) -> bool {
        let mut pushed = false;
        if self.oc[1].status == OcStatus::Locked || self.oc[0].status == OcStatus::None {
            self.oc[0] = self.oc[1];
            pushed = true;
        }
        self.oc[1].status = OcStatus::None;
        pushed
    }

    /// Fall back to the configuration kept, if the one in force is not
    /// locked and one was kept (`pop_output_configuration`).
    pub(crate) fn pop_output_configuration(&mut self) {
        if self.oc[1].status != OcStatus::Locked && self.oc[0].status != OcStatus::None {
            self.oc[1] = self.oc[0];
            self.avctx.ch_layout = self.oc[1].ch_layout;
            let (tags, status) = (self.oc[1].layout_map_tags, self.oc[1].status);
            // FFmpeg ignores whether this fails: it is the configuration
            // the decoder was in, set up again.
            let _ = self.output_configure(None, tags, status, false);
        }
    }

    /// Configure the output for a layout map's first `tags` elements:
    /// allocate their elements, free those it switches off, name the
    /// output's channels, and with `get_new_frame` get the frame's buffers
    /// for them (`ff_aac_output_configure`).
    ///
    /// `layout_map` is the map given, copied as given into the
    /// configuration in force and sorted here into libavutil's order;
    /// `None` is the configuration's own, sorted where it is -- as FFmpeg's
    /// copies only a map that is not the configuration's own, and sorts the
    /// map it was given.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidData`] for more channels than [`MAX_CHANNELS`], and
    /// [`Error::Bug`] for a map naming an element no frame can, the
    /// configuration then partly done as FFmpeg's would be.
    pub(crate) fn output_configure(
        &mut self,
        layout_map: Option<&[[u8; 3]]>,
        tags: usize,
        oc_type: OcStatus,
        get_new_frame: bool,
    ) -> Result<(), Error> {
        let mut channels = 0usize;
        let mut id_map = [[0u8; MAX_ELEM_ID]; TYPE_END as usize];
        let mut type_counts = [0u8; TYPE_END as usize];

        let mut map: LayoutMap = self.oc[1].layout_map;
        if let Some(given) = layout_map {
            let given = given.get(..tags).ok_or(Error::Bug)?;
            map.get_mut(..tags)
                .ok_or(Error::Bug)?
                .copy_from_slice(given);
            self.oc[1].layout_map[..tags].copy_from_slice(given);
            self.oc[1].layout_map_tags = tags;
        } else if tags > map.len() {
            return Err(Error::Bug);
        }
        for &[ty, id, _] in &map[..tags] {
            // Every map is of the elements a frame can carry, by 4-bit id.
            if ty > TYPE_LFE || usize::from(id) >= MAX_ELEM_ID {
                return Err(Error::Bug);
            }
            let (ty, id) = (usize::from(ty), usize::from(id));
            id_map[ty][id] = type_counts[ty];
            type_counts[ty] = type_counts[ty].wrapping_add(1);
            if usize::from(id_map[ty][id]) >= MAX_ELEM_ID {
                // "Too large remapped id": no map has the tags for it.
                return Err(Error::Unsupported);
            }
        }
        // Sort the elements into libavutil's order of their channels where
        // an order can be found; otherwise the channels come in the order
        // the map lists them.
        let mut layout = 0u64;
        if self.output_channel_order == ChannelOrder::Default {
            layout = sniff_channel_order(&mut map, tags);
        }
        if layout_map.is_none() {
            self.oc[1].layout_map[..tags].copy_from_slice(&map[..tags]);
        }
        for &[ty, id, position] in &map[..tags] {
            let iid = usize::from(id_map[usize::from(ty)][usize::from(id)]);
            self.che_configure(position, ty, iid, &mut channels)?;
            self.tag_che_map[usize::from(ty)][usize::from(id)] = self.elem_ref(ty, iid);
        }
        if self.oc[1].m4ac.ps == 1 && channels == 2 {
            // HE-AAC v2's parametric stereo makes a single channel two.
            layout = if layout == AV_CH_FRONT_CENTER {
                AV_CH_LAYOUT_STEREO
            } else {
                0
            };
        }

        self.oc[1].ch_layout = if layout != 0 {
            ChannelLayout::from_mask(layout)
        } else {
            ChannelLayout {
                order: ORDER_UNSPEC,
                nb_channels: i32::try_from(channels).map_err(|_| Error::Bug)?,
                mask: 0,
            }
        };
        self.avctx.ch_layout = self.oc[1].ch_layout;
        self.oc[1].status = oc_type;

        if get_new_frame {
            self.frame_configure_elements();
        }
        Ok(())
    }

    /// Channel configuration `channel_config`'s layout map, and how many
    /// elements it has (`ff_aac_set_default_channel_config`).
    ///
    /// # Errors
    ///
    /// [`Error::InvalidData`] for a configuration the standard does not
    /// define.
    pub(crate) fn default_channel_config(
        &self,
        layout_map: &mut [[u8; 3]],
        channel_config: i32,
    ) -> Result<usize, Error> {
        default_channel_config(layout_map, channel_config, self.avctx.strict)
            .map_err(|_| Error::InvalidData)
    }

    /// The element a frame's element of type `ty` and tag `elem_id`
    /// decodes into (`ff_aac_get_che`): by tag where a program config
    /// element configured the stream; otherwise the next of the channel
    /// configuration's elements that one of its type may be, with FFmpeg's
    /// allowances for streams that code the configuration wrong -- a pair in
    /// a mono stream, two singles in a stereo one, a single for 5.1's LFE or
    /// an LFE for 4.0's back channel. None for an element the configuration
    /// has no place for.
    pub(crate) fn get_che(&mut self, ty: u8, elem_id: usize) -> Option<ElemRef> {
        if ty > TYPE_LFE || elem_id >= MAX_ELEM_ID {
            return None;
        }
        let t = usize::from(ty);
        if self.oc[1].m4ac.chan_config == 0 {
            return self.tag_che_map[t][elem_id];
        }
        // A pair, configured as mono: stereo.
        if self.tags_mapped == 0 && ty == TYPE_CPE && self.oc[1].m4ac.chan_config == 1 {
            self.push_output_configuration();
            let mut map: LayoutMap = [[0; 3]; MAX_ELEM_ID * 4];
            let tags = self.default_channel_config(&mut map, 2).ok()?;
            self.output_configure(Some(&map), tags, OcStatus::TrialFrame, true)
                .ok()?;
            self.oc[1].m4ac.chan_config = 2;
            self.oc[1].m4ac.ps = 0;
        }
        // A single, configured as stereo: two singles.
        if self.tags_mapped == 0 && ty == TYPE_SCE && self.oc[1].m4ac.chan_config == 2 {
            self.push_output_configuration();
            let map = [[TYPE_SCE, 0, CHANNEL_FRONT], [TYPE_SCE, 1, CHANNEL_FRONT]];
            self.output_configure(Some(&map), 2, OcStatus::TrialFrame, true)
                .ok()?;
            if self.oc[1].m4ac.sbr != 0 {
                self.oc[1].m4ac.ps = -1;
            }
        }

        // FFmpeg's switch on the configuration falls through from each case
        // to the next: a configuration enters at its own and tries every
        // one after.
        let config = self.oc[1].m4ac.chan_config;
        let entry = match config {
            14 => 0,
            13 => 1,
            12 | 7 => 2,
            11 => 3,
            6 => 4,
            5 => 5,
            4 => 6,
            3 | 2 => 7,
            1 => 8,
            _ => return None,
        };
        let mapped = self.tags_mapped;
        let last = TAGS_PER_CONFIG
            .get(usize::try_from(config).ok()?)
            .map_or(-1, |&n| i32::from(n) - 1);
        let elem_id_i = i32::try_from(elem_id).ok()?;
        let map_to = |s: &mut Self, tag_ty: u8, target_ty: u8, target: usize| {
            s.tags_mapped += 1;
            let r = s.elem_ref(target_ty, target);
            s.tag_che_map[usize::from(tag_ty)][elem_id] = r;
            r
        };
        if entry <= 0
            && mapped > 2
            && ((ty == TYPE_CPE && elem_id_i < 3) || (ty == TYPE_LFE && elem_id_i < 1))
        {
            return map_to(self, ty, ty, elem_id);
        }
        if entry <= 1
            && mapped > 3
            && ((ty == TYPE_CPE && elem_id_i < 8)
                || (ty == TYPE_SCE && elem_id_i < 6)
                || (ty == TYPE_LFE && elem_id_i < 2))
        {
            return map_to(self, ty, ty, elem_id);
        }
        if entry <= 2 && mapped == 3 && ty == TYPE_CPE {
            return map_to(self, TYPE_CPE, TYPE_CPE, 2);
        }
        if entry <= 3 && mapped == 3 && ty == TYPE_SCE {
            return map_to(self, TYPE_SCE, TYPE_SCE, 1);
        }
        // Some streams code 5.1's LFE as a second single.
        if entry <= 4 && mapped == last && (ty == TYPE_LFE || ty == TYPE_SCE) {
            return map_to(self, ty, TYPE_LFE, 0);
        }
        if entry <= 5 && mapped == 2 && ty == TYPE_CPE {
            return map_to(self, TYPE_CPE, TYPE_CPE, 1);
        }
        if entry <= 6 {
            // Some streams code 4.0's back channel as an LFE.
            if mapped == last && (ty == TYPE_LFE || ty == TYPE_SCE) {
                return map_to(self, ty, TYPE_SCE, 1);
            }
            if mapped == 2 && config == 4 && ty == TYPE_SCE {
                return map_to(self, TYPE_SCE, TYPE_SCE, 1);
            }
        }
        if entry <= 7 {
            if mapped == i32::from(config != 2) && ty == TYPE_CPE {
                return map_to(self, TYPE_CPE, TYPE_CPE, 0);
            } else if mapped == 1 && config == 2 && ty == TYPE_SCE {
                return map_to(self, TYPE_SCE, TYPE_SCE, 1);
            }
        }
        if mapped == 0 && ty == TYPE_SCE {
            return map_to(self, TYPE_SCE, TYPE_SCE, 0);
        }
        None
    }
}

#[cfg(test)]
mod tests;
