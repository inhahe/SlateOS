#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code, on values the tests choose"
)]

use super::*;
use crate::context::{ChannelLayout, CodecContext, ORDER_UNSPEC};
use crate::layout::{TYPE_CPE, TYPE_SCE};

/// Bits written as strings of 0s and 1s, then zero bytes for the reader to
/// look ahead into.
fn bytes(bits: &[&str]) -> (Vec<u8>, usize) {
    let all: String = bits.concat().chars().filter(|c| *c != ' ').collect();
    let mut out = vec![0u8; all.len().div_ceil(8) + 8];
    for (i, c) in all.bytes().enumerate() {
        if c == b'1' {
            out[i / 8] |= 0x80 >> (i % 8);
        }
    }
    (out, all.len())
}

fn decoder() -> Context {
    Context::new(CodecContext::default())
}

#[test]
fn an_adts_header_reads_as_ffmpeg_reads_it() {
    // LC, 44.1 kHz, stereo, no CRC, a frame of 256 bytes, one block.
    let header = [0xFF, 0xF1, 0x50, 0x80, 0x20, 0x1F, 0xFC];
    let mut gb = Bits::whole(&header).unwrap();
    let h = adts_header_parse(&mut gb).unwrap();
    assert_eq!(
        h,
        AdtsHeader {
            sample_rate: 44100,
            samples: 1024,
            bit_rate: 256 * 8 * 44100 / 1024,
            crc_absent: 1,
            object_type: 2,
            sampling_index: 4,
            chan_config: 2,
            num_aac_frames: 1,
            frame_length: 256,
        }
    );
    assert_eq!(gb.count(), 56);
    let refused = |bytes: [u8; 7]| adts_header_parse(&mut Bits::whole(&bytes).unwrap());
    assert_eq!(
        refused([0xFF, 0xE1, 0x50, 0x80, 0x20, 0x1F, 0xFC]),
        Err(AdtsError::Sync)
    );
    // Rate index 13, which names no rate.
    assert_eq!(
        refused([0xFF, 0xF1, 0x74, 0x80, 0x20, 0x1F, 0xFC]),
        Err(AdtsError::SampleRate)
    );
    // A frame of 6 bytes, shorter than its header.
    assert_eq!(
        refused([0xFF, 0xF1, 0x50, 0x80, 0x00, 0xDF, 0xFC]),
        Err(AdtsError::FrameSize)
    );
}

#[test]
fn rates_without_an_index_take_the_nearest() {
    assert_eq!(sample_rate_idx(96000), 0);
    assert_eq!(sample_rate_idx(44100), 4);
    assert_eq!(sample_rate_idx(46008), 4);
    assert_eq!(sample_rate_idx(46009), 3);
    assert_eq!(sample_rate_idx(8000), 11);
    assert_eq!(sample_rate_idx(0), 11);
}

/// A program config element's body, 54 bits: a single and a pair at the
/// front, a pair at the back, an LFE, no data, a coupling channel.
const PCE: &str = concat!(
    "00",    // object_type
    "0100",  // sampling_frequency_index
    "0010",  // num_front_channel_elements
    "0000",  // num_side_channel_elements
    "0001",  // num_back_channel_elements
    "01",    // num_lfe_channel_elements
    "000",   // num_assoc_data_elements
    "0001",  // num_valid_cc_elements
    "000",   // no mixdowns
    "00000", // front: a single, id 0
    "10001", // front: a pair, id 1
    "10010", // back: a pair, id 2
    "0000",  // LFE id 0
    "00011", // a coupling channel, id 3
);

/// The element, then `tail`: its alignment and comment.
fn pce(tail: &str) -> (Vec<u8>, usize) {
    bytes(&[PCE, tail])
}

#[test]
fn a_program_config_element_lists_its_elements() {
    // Two bits to the byte boundary, then a comment of one byte.
    let (data, bits) = pce("00 00000001 01101000");
    let mut gb = Bits::new(&data, bits).unwrap();
    let mut map: LayoutMap = [[0; 3]; MAX_ELEM_ID * 4];
    assert_eq!(decode_pce(&mut map, &mut gb, 0), Ok(5));
    assert_eq!(
        map[..5],
        [
            [TYPE_SCE, 0, CHANNEL_FRONT],
            [TYPE_CPE, 1, CHANNEL_FRONT],
            [TYPE_CPE, 2, CHANNEL_BACK],
            [TYPE_LFE, 0, CHANNEL_LFE],
            [TYPE_CCE, 3, CHANNEL_CC],
        ]
    );
    assert_eq!(gb.count() as usize, bits);
    // Aligned from bit 2 instead: four bits to the boundary.
    let (data, bits) = pce("0000 00000000");
    let mut gb = Bits::new(&data, bits).unwrap();
    assert_eq!(decode_pce(&mut map, &mut gb, 2), Ok(5));
    assert_eq!(gb.count() as usize, bits);
    // A comment longer than what is left.
    let (data, bits) = pce("00 00000010 01101000");
    let mut gb = Bits::new(&data, bits).unwrap();
    assert_eq!(decode_pce(&mut map, &mut gb, 0), Err(Error::InvalidData));
    // Fewer bits than its elements take.
    let (data, _) = pce("");
    let mut gb = Bits::new(&data, 40).unwrap();
    assert_eq!(decode_pce(&mut map, &mut gb, 0), Err(Error::InvalidData));
}

#[test]
fn an_lc_config_configures_its_channels() {
    let mut ac = decoder();
    // LC, 48 kHz, stereo; no 960 frames, no core coder, no extension.
    let (data, bits) = bytes(&["00010 0011 0010 000"]);
    assert_eq!(ac.decode_audio_specific_config(&data, bits, true), Ok(16));
    let m = ac.oc[1].m4ac;
    assert_eq!(
        (
            m.object_type,
            m.sampling_index,
            m.sample_rate,
            m.chan_config
        ),
        (2, 3, 48000, 2)
    );
    // Two channels: no parametric stereo; SBR left for the stream to say.
    assert_eq!((m.sbr, m.ps), (-1, 0));
    assert_eq!(ac.avctx.ch_layout, ChannelLayout::from_mask(0b11));
    assert_eq!(ac.oc[1].status, OcStatus::GlobalHdr);
}

#[test]
fn a_mono_he_aac_config_expects_parametric_stereo() {
    let mut ac = decoder();
    // SBR (5) at 24 kHz core, 48 kHz extension, mono, LC core.
    let (data, bits) = bytes(&["00101 0110 0001 0011 00010 000"]);
    ac.decode_audio_specific_config(&data, bits, true).unwrap();
    let m = ac.oc[1].m4ac;
    assert_eq!((m.object_type, m.sbr, m.ext_sample_rate), (2, 1, 48000));
    // One channel and SBR: PS signalled implicitly, two channels out.
    assert_eq!(m.ps, 1);
    assert_eq!(ac.avctx.ch_layout, ChannelLayout::from_mask(0b11));
}

#[test]
fn a_config_with_a_program_config_element_configures_by_it() {
    let mut ac = decoder();
    // LC, 44.1 kHz, channel configuration 0; GA flags; the element's tag;
    // the element from bit 20, so six bits to the boundary counted from the
    // config's first bit; no comment.
    let (data, bits) = bytes(&["00010 0100 0000 000 0000", PCE, "000000 00000000"]);
    ac.decode_audio_specific_config(&data, bits, true).unwrap();
    // FL FR FC LFE BL BR: 5.1, sorted, the coupling channel no output.
    assert_eq!(ac.avctx.ch_layout, ChannelLayout::from_mask(0x3f));
    assert!(
        ac.elem(crate::context::ElemRef {
            ty: TYPE_CCE,
            index: 0
        })
        .is_some()
    );
}

#[test]
fn side_elements_are_read_with_their_type() {
    // A pair at the front (id 1), a single and a pair at the side (ids 4 and
    // 5), no LFE, data or coupling; no comment.
    let (data, bits) = bytes(&[
        "00 0100 0001 0010 0000 00 000 0000 000",
        "1 0001",
        "0 0100 1 0101",
        "000 00000000",
    ]);
    let mut gb = Bits::new(&data, bits).unwrap();
    let mut map: LayoutMap = [[0; 3]; MAX_ELEM_ID * 4];
    assert_eq!(decode_pce(&mut map, &mut gb, 0), Ok(3));
    assert_eq!(
        map[..3],
        [
            [TYPE_CPE, 1, CHANNEL_FRONT],
            [TYPE_SCE, 4, CHANNEL_SIDE],
            [TYPE_CPE, 5, CHANNEL_SIDE],
        ]
    );
    assert_eq!(gb.count() as usize, bits);
}

#[test]
fn two_channels_from_a_program_config_element_rule_out_parametric_stereo() {
    let mut ac = decoder();
    // LC, 44.1 kHz, channel configuration 0 -- which counts no channels,
    // leaving parametric stereo for the element to rule on -- and an
    // element of one front pair.
    let (data, bits) = bytes(&[
        "00010 0100 0000 000 0000",
        "00 0100 0001 0000 0000 00 000 0000 000",
        "1 0000",
        "000 00000000",
    ]);
    ac.decode_audio_specific_config(&data, bits, true).unwrap();
    assert_eq!(ac.oc[1].m4ac.ps, 0);
    assert_eq!(ac.avctx.ch_layout, ChannelLayout::from_mask(0b11));
}

#[test]
fn a_backward_compatible_sbr_signal_is_found_after_the_config() {
    let mut ac = decoder();
    // LC at 24 kHz, stereo; the GA part; then the sync extension 0x2b7,
    // SBR, present, at 48 kHz.
    let (data, bits) = bytes(&[
        "00010 0110 0010",
        "000",
        "01010110111 00101 1 0011",
        "00000000000 0000",
    ]);
    // The bits read are the config's own, up to the end of its GA part:
    // the extension is looked for in a copy of the reader.
    assert_eq!(ac.decode_audio_specific_config(&data, bits, true), Ok(16));
    let m = ac.oc[1].m4ac;
    assert_eq!((m.sbr, m.ext_sample_rate, m.ps), (1, 48000, 0));
    // Not looked for, it is not found.
    let mut ac = decoder();
    assert_eq!(ac.decode_audio_specific_config(&data, bits, false), Ok(16));
    assert_eq!(ac.oc[1].m4ac.sbr, -1);
}

#[test]
fn configs_ffmpeg_refuses_leave_the_common_part_as_it_was() {
    let mut ac = decoder();
    ac.oc[1].m4ac.sample_rate = 12345;
    // LC with sampling index 13, which names no rate.
    let (data, bits) = bytes(&["00010 1101 0010 000"]);
    assert_eq!(
        ac.decode_audio_specific_config(&data, bits, true),
        Err(Error::InvalidData)
    );
    assert_eq!(ac.oc[1].m4ac.sample_rate, 12345);
    assert_eq!(ac.oc[1].m4ac.object_type, 0);
    // ER AAC LD at 96 kHz: low delay takes 48 to 22.05 kHz only.
    let (data, bits) = bytes(&["10111 0000 0010 000"]);
    assert_eq!(
        ac.decode_audio_specific_config(&data, bits, true),
        Err(Error::InvalidData)
    );
    assert_eq!(ac.oc[1].m4ac.object_type, 0);
    // USAC (31, then 42 - 32), which the fixed-point decoder refuses.
    let (data, bits) = bytes(&["11111 001010 0011 0010 000"]);
    assert_eq!(
        ac.decode_audio_specific_config(&data, bits, true),
        Err(Error::Unsupported)
    );
}

#[test]
fn a_decoder_opened_without_a_config_takes_its_channels_count() {
    let opened = |channels: i32, rate: i32| {
        let mut ac = Context::new(CodecContext {
            ch_layout: ChannelLayout {
                order: ORDER_UNSPEC,
                nb_channels: channels,
                mask: 0,
            },
            sample_rate: rate,
            ..CodecContext::default()
        });
        ac.init_config(&[]).map(|()| ac)
    };
    let ac = opened(2, 44100).unwrap();
    assert_eq!(
        (ac.oc[1].m4ac.chan_config, ac.oc[1].m4ac.sampling_index),
        (2, 4)
    );
    assert_eq!(ac.avctx.ch_layout, ChannelLayout::from_mask(0b11));
    assert_eq!(ac.random_state, 0x1f2e_3d4c);
    // Seven channels are configuration 11, 6.1.
    assert_eq!(opened(7, 48000).unwrap().oc[1].m4ac.chan_config, 11);
    // None, or a count no configuration has: nothing configured.
    let ac = opened(0, 48000).unwrap();
    assert_eq!(ac.oc[1].m4ac.chan_config, 0);
    assert_eq!(ac.oc[1].status, OcStatus::None);
    assert_eq!(opened(9, 48000).unwrap().oc[1].m4ac.chan_config, 0);
    assert_eq!(opened(2, 96001).err(), Some(Error::InvalidData));
    assert_eq!(opened(65, 48000).err(), Some(Error::InvalidData));
}
