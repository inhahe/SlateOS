//! Which channel each element of a stream is, and the order the channels
//! come out in, as FFmpeg's decoder decides them (`aacdec.c`:
//! `count_channels`, `sniff_channel_order` and what it calls, and
//! `ff_aac_set_default_channel_config`; `aacdec_tab.c`'s tables).
//!
//! A stream's channels come as elements -- a single channel (SCE), a pair
//! (CPE), a low-frequency channel (LFE) -- each with a 4-bit id and a
//! position (front, side, back, LFE), listed in a *layout map*: from one of
//! the standard's channel configurations, or from the stream's program
//! config element. FFmpeg then sorts the elements so its output's channels
//! come in libavutil's order, and names them with a channel mask; the
//! decoder's output has its channels in that order.

#![allow(
    clippy::indexing_slicing,
    reason = "tables indexed by a channel configuration under 16, a layer under 3 and a \
              position from 1 to 4, and layout maps by tags under 4 * MAX_ELEM_ID, each checked \
              where it comes from"
)]
#![allow(
    clippy::arithmetic_side_effects,
    reason = "counts of channels and elements, under 4 * MAX_ELEM_ID"
)]

/// The element types (`enum RawDataBlockType`).
pub(crate) const TYPE_SCE: u8 = 0;
pub(crate) const TYPE_CPE: u8 = 1;
pub(crate) const TYPE_CCE: u8 = 2;
pub(crate) const TYPE_LFE: u8 = 3;
pub(crate) const TYPE_DSE: u8 = 4;
pub(crate) const TYPE_PCE: u8 = 5;
pub(crate) const TYPE_FIL: u8 = 6;
pub(crate) const TYPE_END: u8 = 7;

/// An element's position (`enum ChannelPosition`).
pub(crate) const CHANNEL_OFF: u8 = 0;
pub(crate) const CHANNEL_FRONT: u8 = 1;
pub(crate) const CHANNEL_SIDE: u8 = 2;
pub(crate) const CHANNEL_BACK: u8 = 3;
pub(crate) const CHANNEL_LFE: u8 = 4;
pub(crate) const CHANNEL_CC: u8 = 5;

/// The most channels a stream may have, and the most element ids a type
/// may use (`MAX_CHANNELS`, `MAX_ELEM_ID`).
pub(crate) const MAX_CHANNELS: usize = 64;
pub(crate) const MAX_ELEM_ID: usize = 64;

/// A layout map: each element's type, id and position.
pub(crate) type LayoutMap = [[u8; 3]; MAX_ELEM_ID * 4];

/// libavutil's channel numbers, and the masks made of them
/// (`enum AVChannel`, `AV_CH_LAYOUT_22POINT2`).
pub(crate) mod chan {
    pub(crate) const NONE: i16 = -1;
    pub(crate) const FRONT_LEFT: i16 = 0;
    pub(crate) const FRONT_RIGHT: i16 = 1;
    pub(crate) const FRONT_CENTER: i16 = 2;
    pub(crate) const LOW_FREQUENCY: i16 = 3;
    pub(crate) const BACK_LEFT: i16 = 4;
    pub(crate) const BACK_RIGHT: i16 = 5;
    pub(crate) const FRONT_LEFT_OF_CENTER: i16 = 6;
    pub(crate) const FRONT_RIGHT_OF_CENTER: i16 = 7;
    pub(crate) const BACK_CENTER: i16 = 8;
    pub(crate) const SIDE_LEFT: i16 = 9;
    pub(crate) const SIDE_RIGHT: i16 = 10;
    pub(crate) const TOP_CENTER: i16 = 11;
    pub(crate) const TOP_FRONT_LEFT: i16 = 12;
    pub(crate) const TOP_FRONT_CENTER: i16 = 13;
    pub(crate) const TOP_FRONT_RIGHT: i16 = 14;
    pub(crate) const TOP_BACK_LEFT: i16 = 15;
    pub(crate) const TOP_BACK_CENTER: i16 = 16;
    pub(crate) const TOP_BACK_RIGHT: i16 = 17;
    pub(crate) const LOW_FREQUENCY_2: i16 = 35;
    pub(crate) const TOP_SIDE_LEFT: i16 = 36;
    pub(crate) const TOP_SIDE_RIGHT: i16 = 37;
    pub(crate) const BOTTOM_FRONT_CENTER: i16 = 38;
    pub(crate) const BOTTOM_FRONT_LEFT: i16 = 39;
    pub(crate) const BOTTOM_FRONT_RIGHT: i16 = 40;
    pub(crate) const UNUSED: i16 = 0x200;

    /// The 22.2 layout's mask, which the sort below makes an exception of.
    pub(crate) const LAYOUT_22POINT2: u64 = 0x1f8_0003_ffff;
}

/// How many elements each channel configuration has (`ff_tags_per_config`).
pub(crate) const TAGS_PER_CONFIG: [i8; 16] = [0, 1, 1, 2, 3, 3, 4, 5, 0, 0, 0, 5, 5, 16, 5, 0];

const fn e(ty: u8, id: u8, pos: u8) -> [u8; 3] {
    [ty, id, pos]
}

/// Each channel configuration's layout map, from configuration 1
/// (`ff_aac_channel_layout_map`).
pub(crate) const CHANNEL_LAYOUT_MAP: [[[u8; 3]; 16]; 16] = {
    use self::{
        CHANNEL_BACK as B, CHANNEL_FRONT as F, CHANNEL_LFE as L, CHANNEL_SIDE as S, TYPE_CPE as C,
        TYPE_LFE as LF, TYPE_SCE as SC,
    };
    let z = [0u8; 3];
    let mut t = [[z; 16]; 16];
    let rows: [&[[u8; 3]]; 16] = [
        &[e(SC, 0, F)],
        &[e(C, 0, F)],
        &[e(SC, 0, F), e(C, 0, F)],
        &[e(SC, 0, F), e(C, 0, F), e(SC, 1, B)],
        &[e(SC, 0, F), e(C, 0, F), e(C, 1, B)],
        &[e(SC, 0, F), e(C, 0, F), e(C, 1, B), e(LF, 0, L)],
        &[e(SC, 0, F), e(C, 0, F), e(C, 1, F), e(C, 2, B), e(LF, 0, L)],
        &[],
        &[],
        &[],
        &[
            e(SC, 0, F),
            e(C, 0, F),
            e(C, 1, B),
            e(SC, 1, B),
            e(LF, 0, L),
        ],
        &[e(SC, 0, F), e(C, 0, F), e(C, 1, B), e(C, 2, B), e(LF, 0, L)],
        &[
            e(SC, 0, F), // FC
            e(C, 0, F),  // FLc and FRc
            e(C, 1, F),  // FL and FR
            e(C, 2, B),  // SiL and SiR
            e(C, 3, B),  // BL and BR
            e(SC, 1, B), // BC
            e(LF, 0, L), // LFE1
            e(LF, 1, L), // LFE2
            e(SC, 2, F), // TpFC
            e(C, 4, F),  // TpFL and TpFR
            e(C, 5, S),  // TpSiL and TpSiR
            e(SC, 3, S), // TpC
            e(C, 6, B),  // TpBL and TpBR
            e(SC, 4, B), // TpBC
            e(SC, 5, F), // BtFC
            e(C, 7, F),  // BtFL and BtFR
        ],
        &[e(SC, 0, F), e(C, 0, F), e(C, 1, B), e(LF, 0, L), e(C, 2, F)],
        &[],
        &[],
    ];
    let mut i = 0;
    while i < 16 {
        let mut j = 0;
        while j < rows[i].len() {
            t[i][j] = rows[i][j];
            j += 1;
        }
        i += 1;
    }
    t
};

/// The channel each element position takes, by layer (normal, top,
/// bottom) and position (front, side, back, LFE): the first column for an
/// odd one out at the centre, the next four for pairs, the last for an odd
/// one out behind (`ff_aac_channel_map`).
pub(crate) const CHANNEL_MAP: [[[i16; 6]; 4]; 3] = {
    use self::chan::*;
    [
        [
            [
                FRONT_CENTER,
                FRONT_LEFT_OF_CENTER,
                FRONT_RIGHT_OF_CENTER,
                FRONT_LEFT,
                FRONT_RIGHT,
                NONE,
            ],
            [UNUSED, NONE, NONE, NONE, NONE, NONE],
            [
                UNUSED,
                SIDE_LEFT,
                SIDE_RIGHT,
                BACK_LEFT,
                BACK_RIGHT,
                BACK_CENTER,
            ],
            [LOW_FREQUENCY, LOW_FREQUENCY_2, NONE, NONE, NONE, NONE],
        ],
        [
            [
                TOP_FRONT_CENTER,
                NONE,
                NONE,
                TOP_FRONT_LEFT,
                TOP_FRONT_RIGHT,
                NONE,
            ],
            [
                UNUSED,
                TOP_SIDE_LEFT,
                TOP_SIDE_RIGHT,
                NONE,
                NONE,
                TOP_CENTER,
            ],
            [
                UNUSED,
                NONE,
                NONE,
                TOP_BACK_LEFT,
                TOP_BACK_RIGHT,
                TOP_BACK_CENTER,
            ],
            [NONE, NONE, NONE, NONE, NONE, NONE],
        ],
        [
            [
                BOTTOM_FRONT_CENTER,
                NONE,
                NONE,
                BOTTOM_FRONT_LEFT,
                BOTTOM_FRONT_RIGHT,
                NONE,
            ],
            [NONE, NONE, NONE, NONE, NONE, NONE],
            [NONE, NONE, NONE, NONE, NONE, NONE],
            [NONE, NONE, NONE, NONE, NONE, NONE],
        ],
    ]
};

/// How many channels `layout`'s first `tags` elements carry: a pair two,
/// the rest one, coupling channels and elements switched off none
/// (`count_channels`).
pub(crate) fn count_channels(layout: &[[u8; 3]], tags: usize) -> i32 {
    layout
        .iter()
        .take(tags)
        .map(|&[ty, _, pos]| {
            let carried = pos != CHANNEL_OFF && pos != CHANNEL_CC;
            i32::from(carried) * (1 + i32::from(ty == TYPE_CPE))
        })
        .sum()
}

/// One element's place in the output (`struct elem_to_channel`): its
/// channel mask, type, id and position.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct ElemToChannel {
    av_position: u64,
    syn_ele: u8,
    elem_id: u8,
    aac_position: u8,
}

/// The channel a map entry names, as a mask bit.
#[allow(
    clippy::cast_sign_loss,
    reason = "called on channel numbers from 0 to 63, NONE and UNUSED ruled out first"
)]
fn bit(ch: i16) -> u64 {
    1u64 << (ch as u32)
}

/// A pair's two channels to the element at `offset`, if it is a pair, or
/// to that element and the next, single ones; how many elements that took
/// (`assign_pair`).
fn assign_pair(
    e2c: &mut [ElemToChannel],
    layout_map: &[[u8; 3]],
    offset: usize,
    left: u64,
    right: u64,
    pos: u8,
    layout: &mut u64,
) -> usize {
    if layout_map[offset][0] == TYPE_CPE {
        e2c[offset] = ElemToChannel {
            av_position: left | right,
            syn_ele: TYPE_CPE,
            elem_id: layout_map[offset][1],
            aac_position: pos,
        };
        if e2c[offset].av_position != u64::MAX {
            *layout |= e2c[offset].av_position;
        }
        1
    } else {
        e2c[offset] = ElemToChannel {
            av_position: left,
            syn_ele: TYPE_SCE,
            elem_id: layout_map[offset][1],
            aac_position: pos,
        };
        e2c[offset + 1] = ElemToChannel {
            av_position: right,
            syn_ele: TYPE_SCE,
            elem_id: layout_map[offset + 1][1],
            aac_position: pos,
        };
        if left != u64::MAX {
            *layout |= left;
        }
        if right != u64::MAX {
            *layout |= right;
        }
        2
    }
}

/// How many channels the run of elements at position `pos` from `current`
/// carries, or -1 where its singles and pairs cannot be placed: a single
/// between pairs, except one at the front centre (`count_paired_channels`).
fn count_paired_channels(layout_map: &[[u8; 3]], tags: usize, pos: u8, current: usize) -> i32 {
    let mut num_pos_channels = 0;
    let mut first_cpe = false;
    let mut sce_parity = false;
    for entry in layout_map.iter().take(tags).skip(current) {
        if entry[2] != pos {
            break;
        }
        if entry[0] == TYPE_CPE {
            if sce_parity {
                if pos == CHANNEL_FRONT && !first_cpe {
                    sce_parity = false;
                } else {
                    return -1;
                }
            }
            num_pos_channels += 2;
            first_cpe = true;
        } else {
            num_pos_channels += 1;
            sce_parity ^= pos != CHANNEL_LFE;
        }
    }
    if sce_parity && pos == CHANNEL_FRONT && first_cpe {
        return -1;
    }
    num_pos_channels
}

/// The channels of the run of elements at position `pos` from `*current`,
/// in layer `layer`: an odd single at the centre first, then pairs, then an
/// odd single behind; `*current` moved past them. -1 for a run the layer
/// has no channels for; 0, the run left, for one of more than five
/// channels or none that can be placed (`assign_channels`).
fn assign_channels(
    e2c: &mut [ElemToChannel],
    layout_map: &[[u8; 3]],
    layout: &mut u64,
    tags: usize,
    layer: usize,
    pos: u8,
    current: &mut usize,
) -> i32 {
    let mut i = *current;
    let mut j = 0;
    let mut nb_channels = count_paired_channels(layout_map, tags, pos, i);
    if !(0..=5).contains(&nb_channels) {
        return 0;
    }
    let map = &CHANNEL_MAP[layer][usize::from(pos) - 1];
    if pos == CHANNEL_LFE {
        while nb_channels > 0 {
            if map[j] == chan::NONE {
                return -1;
            }
            e2c[i] = ElemToChannel {
                av_position: bit(map[j]),
                syn_ele: layout_map[i][0],
                elem_id: layout_map[i][1],
                aac_position: pos,
            };
            *layout |= e2c[i].av_position;
            i += 1;
            j += 1;
            nb_channels -= 1;
        }
        *current = i;
        return 0;
    }
    while nb_channels & 1 != 0 {
        if map[0] == chan::NONE {
            return -1;
        }
        if map[0] == chan::UNUSED {
            break;
        }
        e2c[i] = ElemToChannel {
            av_position: bit(map[0]),
            syn_ele: layout_map[i][0],
            elem_id: layout_map[i][1],
            aac_position: pos,
        };
        *layout |= e2c[i].av_position;
        i += 1;
        nb_channels -= 1;
    }
    j = if pos != CHANNEL_SIDE && nb_channels <= 3 {
        3
    } else {
        1
    };
    while nb_channels >= 2 {
        if map[j] == chan::NONE || map[j + 1] == chan::NONE {
            return -1;
        }
        i += assign_pair(
            e2c,
            layout_map,
            i,
            bit(map[j]),
            bit(map[j + 1]),
            pos,
            layout,
        );
        j += 2;
        nb_channels -= 2;
    }
    while nb_channels & 1 != 0 {
        if map[5] == chan::NONE {
            return -1;
        }
        e2c[i] = ElemToChannel {
            av_position: bit(map[5]),
            syn_ele: layout_map[i][0],
            elem_id: layout_map[i][1],
            aac_position: pos,
        };
        *layout |= e2c[i].av_position;
        i += 1;
        nb_channels -= 1;
    }
    if nb_channels != 0 {
        return -1;
    }
    *current = i;
    0
}

/// Sort `layout_map`'s first `tags` elements into the order of the channels
/// libavutil gives them, rewriting the map, and answer the output's channel
/// mask; 0, the map untouched, where no order can be found
/// (`sniff_channel_order`).
pub(crate) fn sniff_channel_order(layout_map: &mut [[u8; 3]], tags: usize) -> u64 {
    let mut e2c = [ElemToChannel::default(); 4 * MAX_ELEM_ID];
    let mut layout = 0u64;
    if e2c.len() < tags {
        return 0;
    }
    let mut i = 0;
    let mut n = 0;
    while n < 3 && i < tags {
        for pos in [CHANNEL_FRONT, CHANNEL_SIDE, CHANNEL_BACK, CHANNEL_LFE] {
            if assign_channels(&mut e2c, layout_map, &mut layout, tags, n, pos, &mut i) < 0 {
                return 0;
            }
        }
        n += 1;
    }
    let total_non_cc_elements = i;

    if layout == chan::LAYOUT_22POINT2 {
        // 22.2's elements into its own order.
        for (a, b) in [
            (2, 0),
            (2, 1),
            (6, 2),
            (4, 3),
            (6, 4),
            (7, 6),
            (9, 8),
            (11, 10),
            (12, 11),
        ] {
            e2c.swap(a, b);
        }
    } else {
        // Everything else by its channel's place in libavutil's order,
        // stably, as the bubble sort below is.
        let mut n = total_non_cc_elements;
        loop {
            let mut next_n = 0;
            for k in 1..n {
                if e2c[k - 1].av_position > e2c[k].av_position {
                    e2c.swap(k - 1, k);
                    next_n = k;
                }
            }
            n = next_n;
            if n == 0 {
                break;
            }
        }
    }

    for (entry, placed) in layout_map.iter_mut().zip(&e2c).take(total_non_cc_elements) {
        *entry = [placed.syn_ele, placed.elem_id, placed.aac_position];
    }
    layout
}

/// A channel configuration not in the standard's table (or 0, which a
/// program config element gives instead): FFmpeg's "invalid default
/// channel configuration".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct InvalidConfig;

/// Channel configuration `channel_config`'s layout map into `layout_map`,
/// and how many elements it has (`ff_aac_set_default_channel_config`).
/// Configuration 7 is read as 7.1 rather than the standard's 7.1 (wide)
/// unless `strict` -- what encoders that write it mean, as FFmpeg assumes
/// below `-strict strict`.
///
/// # Errors
///
/// [`InvalidConfig`] for 0, 8 to 10, and over 14.
pub(crate) fn default_channel_config(
    layout_map: &mut [[u8; 3]],
    channel_config: i32,
    strict: bool,
) -> Result<usize, InvalidConfig> {
    if channel_config < 1 || (channel_config > 7 && channel_config < 11) || channel_config > 14 {
        return Err(InvalidConfig);
    }
    let config = usize::try_from(channel_config).map_err(|_| InvalidConfig)?;
    let tags = usize::try_from(TAGS_PER_CONFIG[config]).map_err(|_| InvalidConfig)?;
    layout_map[..tags].copy_from_slice(&CHANNEL_LAYOUT_MAP[config - 1][..tags]);
    if channel_config == 7 && !strict {
        layout_map[2][2] = CHANNEL_BACK;
    }
    Ok(tags)
}

#[cfg(test)]
mod tests;
