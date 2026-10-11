#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code, on values the tests choose"
)]

use super::*;

/// Configuration `config`'s mask, as FFmpeg's output names its channels.
fn mask_of(config: i32, strict: bool) -> u64 {
    let mut map: LayoutMap = [[0; 3]; MAX_ELEM_ID * 4];
    let tags = default_channel_config(&mut map, config, strict).unwrap();
    sniff_channel_order(&mut map, tags)
}

const FL: u64 = 1 << 0;
const FR: u64 = 1 << 1;
const FC: u64 = 1 << 2;
const LFE: u64 = 1 << 3;
const BL: u64 = 1 << 4;
const BR: u64 = 1 << 5;
const FLC: u64 = 1 << 6;
const FRC: u64 = 1 << 7;
const BC: u64 = 1 << 8;
const SL: u64 = 1 << 9;
const SR: u64 = 1 << 10;
const TFL: u64 = 1 << 12;
const TFR: u64 = 1 << 14;

#[test]
fn each_channel_configuration_has_ffmpegs_layout() {
    // FFmpeg's names for them (`ff_aac_ch_layout`): mono, stereo,
    // surround, 4.0, 5.0 (back), 5.1 (back), 7.1, 6.1 (back), 7.1, 22.2,
    // 7.1 (top back).
    assert_eq!(mask_of(1, false), FC);
    assert_eq!(mask_of(2, false), FL | FR);
    assert_eq!(mask_of(3, false), FL | FR | FC);
    assert_eq!(mask_of(4, false), FL | FR | FC | BC);
    assert_eq!(mask_of(5, false), FL | FR | FC | BL | BR);
    assert_eq!(mask_of(6, false), FL | FR | FC | LFE | BL | BR);
    // 7 as encoders mean it, its second front pair behind; and as the
    // standard says, 7.1 (wide).
    assert_eq!(mask_of(7, false), FL | FR | FC | LFE | BL | BR | SL | SR);
    assert_eq!(mask_of(7, true), FL | FR | FC | LFE | BL | BR | FLC | FRC);
    assert_eq!(mask_of(11, false), FL | FR | FC | LFE | BL | BR | BC);
    assert_eq!(mask_of(12, false), FL | FR | FC | LFE | BL | BR | SL | SR);
    assert_eq!(mask_of(13, false), chan::LAYOUT_22POINT2);
    assert_eq!(mask_of(14, false), FL | FR | FC | LFE | BL | BR | TFL | TFR);
    for config in [0, 8, 9, 10, 15] {
        let mut map: LayoutMap = [[0; 3]; MAX_ELEM_ID * 4];
        assert_eq!(
            default_channel_config(&mut map, config, false),
            Err(InvalidConfig)
        );
    }
}

#[test]
fn elements_are_sorted_into_the_order_of_their_channels() {
    let mut map: LayoutMap = [[0; 3]; MAX_ELEM_ID * 4];
    let tags = default_channel_config(&mut map, 6, false).unwrap();
    assert_eq!(count_channels(&map, tags), 6);
    sniff_channel_order(&mut map, tags);
    // FL and FR, FC, LFE, BL and BR.
    assert_eq!(
        map[..tags],
        [
            [TYPE_CPE, 0, CHANNEL_FRONT],
            [TYPE_SCE, 0, CHANNEL_FRONT],
            [TYPE_LFE, 0, CHANNEL_LFE],
            [TYPE_CPE, 1, CHANNEL_BACK]
        ]
    );
}

#[test]
fn a_program_config_elements_layout() {
    // al_sbr_cm_48_5.1.mp4's: a single and two pairs at the front, and an
    // LFE -- the centre, the inner pair, the outer pair. FFmpeg's output
    // for it has the mask 207.
    let mut map: LayoutMap = [[0; 3]; MAX_ELEM_ID * 4];
    map[..4].copy_from_slice(&[
        [TYPE_SCE, 0, CHANNEL_FRONT],
        [TYPE_CPE, 0, CHANNEL_FRONT],
        [TYPE_CPE, 1, CHANNEL_FRONT],
        [TYPE_LFE, 0, CHANNEL_LFE],
    ]);
    assert_eq!(sniff_channel_order(&mut map, 4), 207);
    assert_eq!(FL | FR | FC | LFE | FLC | FRC, 207);
}

#[test]
fn a_layout_with_no_order_is_left_as_it_is() {
    // A single between two front pairs, not at the centre.
    let mut map: LayoutMap = [[0; 3]; MAX_ELEM_ID * 4];
    let given = [
        [TYPE_CPE, 0, CHANNEL_FRONT],
        [TYPE_SCE, 0, CHANNEL_FRONT],
        [TYPE_CPE, 1, CHANNEL_FRONT],
    ];
    map[..3].copy_from_slice(&given);
    assert_eq!(sniff_channel_order(&mut map, 3), 0);
    assert_eq!(map[..3], given);
    // A single after a front pair, at the end of the front.
    let given = [[TYPE_CPE, 0, CHANNEL_FRONT], [TYPE_SCE, 0, CHANNEL_FRONT]];
    map[..2].copy_from_slice(&given);
    assert_eq!(sniff_channel_order(&mut map, 2), 0);
    assert_eq!(map[..2], given);
    // A coupling channel is no output channel.
    let cc = [[TYPE_SCE, 0, CHANNEL_FRONT], [TYPE_CCE, 0, CHANNEL_CC]];
    assert_eq!(count_channels(&cc, 2), 1);
}
