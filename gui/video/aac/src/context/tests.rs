#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code, on values the tests choose"
)]

use super::*;
use crate::layout::{CHANNEL_BACK, CHANNEL_LFE, CHANNEL_OFF, CHANNEL_SIDE};

const FL: u64 = 1 << 0;
const FR: u64 = 1 << 1;
const FC: u64 = 1 << 2;
const LFE: u64 = 1 << 3;
const BL: u64 = 1 << 4;
const BR: u64 = 1 << 5;

fn elem(ty: u8, index: u8) -> ElemRef {
    ElemRef { ty, index }
}

/// An output channel as `output_element` holds one.
#[allow(
    clippy::unnecessary_wraps,
    reason = "compared with output_element's entries, which are options"
)]
fn ch(ty: u8, index: u8, ch: u8) -> Option<ChRef> {
    Some(ChRef {
        elem: elem(ty, index),
        ch,
    })
}

/// A decoder configured with channel configuration `config`, as a global
/// header configures one.
fn configured(config: i32) -> Context {
    let mut ac = Context::new(CodecContext::default());
    let mut map: LayoutMap = [[0; 3]; MAX_ELEM_ID * 4];
    let tags = ac.default_channel_config(&mut map, config).unwrap();
    ac.output_configure(Some(&map), tags, OcStatus::GlobalHdr, false)
        .unwrap();
    ac.oc[1].m4ac.chan_config = config;
    ac
}

#[test]
fn a_configuration_allocates_its_elements_and_names_its_channels() {
    let ac = configured(6);
    for (ty, index) in [(TYPE_SCE, 0), (TYPE_CPE, 0), (TYPE_CPE, 1), (TYPE_LFE, 0)] {
        assert!(ac.elem(elem(ty, index)).is_some(), "{ty}.{index}");
    }
    assert!(ac.elem(elem(TYPE_SCE, 1)).is_none());
    assert_eq!(
        ac.avctx.ch_layout,
        ChannelLayout::from_mask(FL | FR | FC | LFE | BL | BR)
    );
    assert_eq!(ac.oc[1].ch_layout, ac.avctx.ch_layout);
    assert_eq!(ac.oc[1].status, OcStatus::GlobalHdr);
    // In libavutil's order: the front pair, the centre, the LFE, the back
    // pair.
    assert_eq!(
        ac.output_element[..7],
        [
            ch(TYPE_CPE, 0, 0),
            ch(TYPE_CPE, 0, 1),
            ch(TYPE_SCE, 0, 0),
            ch(TYPE_LFE, 0, 0),
            ch(TYPE_CPE, 1, 0),
            ch(TYPE_CPE, 1, 1),
            None,
        ]
    );
    assert_eq!(
        ac.tag_che_map[usize::from(TYPE_CPE)][1],
        Some(elem(TYPE_CPE, 1))
    );
    assert_eq!(ac.tag_che_map[usize::from(TYPE_SCE)][1], None);
}

#[test]
fn the_configuration_keeps_the_map_as_given_until_it_falls_back_to_it() {
    let mut ac = configured(6);
    let given = CHANNEL_LAYOUT_MAP_6;
    // The map given is copied before it is sorted.
    assert_eq!(ac.oc[1].layout_map[..4], given);
    ac.oc[1].status = OcStatus::Locked;
    assert!(ac.push_output_configuration());
    assert_eq!(ac.oc[1].status, OcStatus::None);
    ac.pop_output_configuration();
    // Falling back configures with the configuration's own map, which is
    // sorted where it is.
    assert_eq!(
        ac.oc[1].layout_map[..4],
        [
            [TYPE_CPE, 0, CHANNEL_FRONT],
            [TYPE_SCE, 0, CHANNEL_FRONT],
            [TYPE_LFE, 0, CHANNEL_LFE],
            [TYPE_CPE, 1, CHANNEL_BACK],
        ]
    );
    assert_eq!(ac.oc[0].layout_map[..4], given);
    assert_eq!(ac.oc[1].status, OcStatus::Locked);
}

/// Channel configuration 6's map, as the standard lists it.
const CHANNEL_LAYOUT_MAP_6: [[u8; 3]; 4] = [
    [TYPE_SCE, 0, CHANNEL_FRONT],
    [TYPE_CPE, 0, CHANNEL_FRONT],
    [TYPE_CPE, 1, CHANNEL_BACK],
    [TYPE_LFE, 0, CHANNEL_LFE],
];

#[test]
fn a_kept_configuration_is_only_replaced_by_a_locked_one() {
    let mut ac = configured(2);
    // None kept yet: the one in force is kept, whatever its status.
    assert!(ac.push_output_configuration());
    assert_eq!(ac.oc[0].status, OcStatus::GlobalHdr);
    // One kept, and the one in force not locked: nothing changes but the
    // status.
    ac.oc[1].status = OcStatus::TrialFrame;
    ac.oc[1].m4ac.sample_rate = 1;
    assert!(!ac.push_output_configuration());
    assert_eq!(ac.oc[0].m4ac.sample_rate, 0);
    assert_eq!(ac.oc[1].status, OcStatus::None);
    // Not locked: falling back restores the one kept.
    ac.pop_output_configuration();
    assert_eq!(ac.oc[1].m4ac.sample_rate, 0);
    assert_eq!(ac.oc[1].status, OcStatus::GlobalHdr);
    // Locked: no falling back.
    ac.oc[1].status = OcStatus::Locked;
    ac.oc[1].m4ac.sample_rate = 2;
    ac.pop_output_configuration();
    assert_eq!(ac.oc[1].m4ac.sample_rate, 2);
}

#[test]
fn an_element_switched_off_is_freed_and_forgotten() {
    let mut ac = Context::new(CodecContext::default());
    let on = [[TYPE_SCE, 0, CHANNEL_FRONT], [TYPE_CPE, 0, CHANNEL_FRONT]];
    ac.output_configure(Some(&on), 2, OcStatus::GlobalHdr, false)
        .unwrap();
    assert_eq!(
        ac.tag_che_map[usize::from(TYPE_CPE)][0],
        Some(elem(TYPE_CPE, 0))
    );
    let off = [[TYPE_SCE, 0, CHANNEL_FRONT], [TYPE_CPE, 0, CHANNEL_OFF]];
    ac.output_configure(Some(&off), 2, OcStatus::GlobalHdr, false)
        .unwrap();
    assert!(ac.elem(elem(TYPE_CPE, 0)).is_none());
    assert_eq!(ac.tag_che_map[usize::from(TYPE_CPE)][0], None);
    assert_eq!(
        ac.tag_che_map[usize::from(TYPE_SCE)][0],
        Some(elem(TYPE_SCE, 0))
    );
    // FFmpeg forgets every output channel as it frees an element -- the
    // single's too, named just before.
    assert_eq!(ac.output_element, [None; MAX_CHANNELS]);
    assert_eq!(ac.avctx.ch_layout, ChannelLayout::from_mask(FC));
}

#[test]
fn parametric_stereo_makes_a_single_channel_two() {
    let mut ac = Context::new(CodecContext::default());
    ac.oc[1].m4ac.ps = 1;
    let map = [[TYPE_SCE, 0, CHANNEL_FRONT]];
    ac.output_configure(Some(&map), 1, OcStatus::GlobalHdr, false)
        .unwrap();
    assert_eq!(ac.avctx.ch_layout, ChannelLayout::from_mask(FL | FR));
    assert_eq!(
        ac.output_element[..3],
        [ch(TYPE_SCE, 0, 0), ch(TYPE_SCE, 0, 1), None]
    );
}

#[test]
fn a_layout_with_no_order_has_none() {
    let mut ac = Context::new(CodecContext::default());
    // A single between two front pairs: no order, so the channels come as
    // the map lists them.
    let map = [
        [TYPE_CPE, 0, CHANNEL_FRONT],
        [TYPE_SCE, 0, CHANNEL_FRONT],
        [TYPE_CPE, 1, CHANNEL_FRONT],
    ];
    ac.output_configure(Some(&map), 3, OcStatus::TrialPce, false)
        .unwrap();
    assert_eq!(
        ac.avctx.ch_layout,
        ChannelLayout {
            order: ORDER_UNSPEC,
            nb_channels: 5,
            mask: 0
        }
    );
    assert_eq!(ac.output_element[2], ch(TYPE_SCE, 0, 0));
}

#[test]
fn more_than_64_channels_are_refused() {
    let mut ac = Context::new(CodecContext::default());
    let mut map: LayoutMap = [[0; 3]; MAX_ELEM_ID * 4];
    for (i, entry) in map.iter_mut().take(45).enumerate() {
        let pos = [CHANNEL_FRONT, CHANNEL_SIDE, CHANNEL_BACK][i / 15];
        *entry = [TYPE_CPE, (i % 15) as u8, pos];
    }
    assert_eq!(
        ac.output_configure(Some(&map), 45, OcStatus::TrialPce, false),
        Err(Error::InvalidData)
    );
    // As FFmpeg's, the configuration is left part done: its map taken, its
    // layout and status not.
    assert_eq!(ac.oc[1].layout_map_tags, 45);
    assert_eq!(ac.oc[1].status, OcStatus::None);
    assert_eq!(ac.avctx.ch_layout, ChannelLayout::default());
}

#[test]
fn a_frame_gets_a_plane_for_each_channel() {
    let mut ac = configured(6);
    ac.frame_configure_elements();
    assert!(ac.frame.allocated);
    assert_eq!(ac.frame.nb_samples, 2048);
    assert_eq!(ac.frame.ch_layout, ac.avctx.ch_layout);
    assert_eq!(ac.frame.planes.len(), 6);
    let out = |ty, index: u8, c: usize| ac.elem(elem(ty, index)).unwrap().ch[c].output;
    assert_eq!(out(TYPE_CPE, 0, 0), Output::Plane(0));
    assert_eq!(out(TYPE_CPE, 0, 1), Output::Plane(1));
    assert_eq!(out(TYPE_SCE, 0, 0), Output::Plane(2));
    assert_eq!(out(TYPE_SCE, 0, 1), Output::RetBuf);
    assert_eq!(out(TYPE_LFE, 0, 0), Output::Plane(3));
    assert_eq!(out(TYPE_CPE, 1, 1), Output::Plane(5));
}

#[test]
fn a_frame_with_no_channels_gets_no_planes() {
    let mut ac = Context::new(CodecContext::default());
    ac.frame_configure_elements();
    assert!(!ac.frame.allocated);
    assert!(ac.frame.planes.is_empty());
}

#[test]
fn a_pair_in_a_mono_stream_makes_it_stereo() {
    let mut ac = configured(1);
    assert_eq!(ac.get_che(TYPE_CPE, 0), Some(elem(TYPE_CPE, 0)));
    assert_eq!(ac.oc[1].m4ac.chan_config, 2);
    assert_eq!(ac.oc[1].m4ac.ps, 0);
    assert_eq!(ac.oc[1].status, OcStatus::TrialFrame);
    assert_eq!(ac.avctx.ch_layout, ChannelLayout::from_mask(FL | FR));
    // The frame was got anew for two channels; the mono configuration is
    // kept to fall back to.
    assert_eq!(ac.frame.planes.len(), 2);
    assert_eq!(ac.oc[0].status, OcStatus::GlobalHdr);
    assert_eq!(ac.tags_mapped, 1);
}

#[test]
fn two_singles_in_a_stereo_stream_are_its_two_channels() {
    let mut ac = configured(2);
    assert_eq!(ac.get_che(TYPE_SCE, 0), Some(elem(TYPE_SCE, 0)));
    assert_eq!(ac.get_che(TYPE_SCE, 1), Some(elem(TYPE_SCE, 1)));
    assert_eq!(ac.avctx.ch_layout, ChannelLayout::from_mask(FL | FR));
    assert_eq!(
        ac.output_element[..2],
        [ch(TYPE_SCE, 0, 0), ch(TYPE_SCE, 1, 0)]
    );
}

#[test]
fn a_single_where_5_1_has_its_lfe_is_the_lfe() {
    let mut ac = configured(6);
    assert_eq!(ac.get_che(TYPE_SCE, 0), Some(elem(TYPE_SCE, 0)));
    assert_eq!(ac.get_che(TYPE_CPE, 0), Some(elem(TYPE_CPE, 0)));
    assert_eq!(ac.get_che(TYPE_CPE, 1), Some(elem(TYPE_CPE, 1)));
    assert_eq!(ac.get_che(TYPE_SCE, 1), Some(elem(TYPE_LFE, 0)));
    assert_eq!(
        ac.tag_che_map[usize::from(TYPE_SCE)][1],
        Some(elem(TYPE_LFE, 0))
    );
    assert_eq!(ac.tags_mapped, 4);
}

#[test]
fn an_element_a_configuration_has_no_place_for_has_none() {
    let mut ac = configured(1);
    assert_eq!(ac.get_che(TYPE_LFE, 0), None);
    assert_eq!(ac.tags_mapped, 0);
    // A place whose element is not allocated maps to none, and still
    // counts.
    let mut ac = configured(2);
    ac.tags_mapped = 1;
    assert_eq!(ac.get_che(TYPE_SCE, 3), None);
    assert_eq!(ac.tags_mapped, 2);
    assert_eq!(ac.tag_che_map[usize::from(TYPE_SCE)][3], None);
}

#[test]
fn a_program_config_element_maps_by_tag() {
    let mut ac = Context::new(CodecContext::default());
    let map = [[TYPE_CPE, 5, CHANNEL_FRONT], [TYPE_CPE, 2, CHANNEL_BACK]];
    ac.output_configure(Some(&map), 2, OcStatus::TrialPce, false)
        .unwrap();
    // Tags 5 and 2 take the first and second pairs allocated.
    assert_eq!(ac.get_che(TYPE_CPE, 5), Some(elem(TYPE_CPE, 0)));
    assert_eq!(ac.get_che(TYPE_CPE, 2), Some(elem(TYPE_CPE, 1)));
    assert_eq!(ac.get_che(TYPE_CPE, 0), None);
}
