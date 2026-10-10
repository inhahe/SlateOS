//! Every fixture read through the crate's `Reader` and held, line for
//! line, to what libFLAC 1.5.0's stream decoder made of it
//! (`tests/data/NAME.txt`, from `tests/data/generate_fixtures.py` and
//! `tools/reference.c`): each metadata block, each frame -- its first sample,
//! size, channels, bit depth, rate and a hash of its samples --, each error,
//! the MD5 verdict, and the frames after each seek.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test: a failure should be loud, and its numbers are small"
)]

mod common;

fn data(name: &str) -> String {
    format!("{}/tests/data/{name}", env!("CARGO_MANIFEST_DIR"))
}

fn decodes_as_libflac_does(name: &str) {
    common::decodes_as_libflac_does(&data(name));
}

macro_rules! fixtures {
    ($($test:ident => $name:literal,)*) => {
        $(
            #[test]
            fn $test() {
                decodes_as_libflac_does($name);
            }
        )*

        /// Every fixture in the directory has a test.
        #[test]
        fn every_fixture_is_tested() {
            let tested = [$($name),*];
            for entry in std::fs::read_dir(data("")).unwrap() {
                let name = entry.unwrap().file_name().into_string().unwrap();
                if name.ends_with(".flac") {
                    assert!(tested.contains(&name.as_str()), "{name} has no test");
                }
            }
        }
    };
}

fixtures! {
    // libFLAC's encoder at its levels and settings.
    stereo_level_5 => "s16_stereo_l5.flac",
    stereo_level_0 => "s16_stereo_l0.flac",
    stereo_level_8 => "s16_stereo_l8.flac",
    stereo_exhaustive => "s16_stereo_exhaustive.flac",
    mono_lpc_order_32 => "s16_mono_order32.flac",
    fixed_predictors_only => "s16_order_fixed_only.flac",
    blocks_of_192 => "s16_block_192.flac",
    blocks_of_576 => "s16_block_576.flac",
    blocks_of_100 => "s16_block_100.flac",
    blocks_of_1000 => "s16_block_1000.flac",
    blocks_of_16384 => "s16_block_16384.flac",
    blocks_of_65535 => "s16_block_65535.flac",
    rice_partition_order_15 => "s16_rice_order15.flac",
    blocks_of_1152 => "s16_stereo_b1152.flac",
    // Bit depths.
    four_bits => "s4_mono.flac",
    seven_bits_at_11025 => "s7_rate_11025.flac",
    eight_bits => "s8_mono.flac",
    twelve_bits => "s12_stereo.flac",
    twenty_bits => "s20_stereo.flac",
    twenty_four_bits => "s24_stereo.flac",
    twenty_four_bits_noise => "s24_noise_rice2.flac",
    thirty_two_bits => "s32_stereo.flac",
    thirty_two_bits_with_a_33_bit_side => "s32_side_33bit.flac",
    // Sample rates the header codes four ways, or leaves to STREAMINFO.
    rate_22060 => "s16_rate_22060.flac",
    rate_12k => "s16_rate_12k.flac",
    rate_768k => "s16_rate_768k.flac",
    rate_655350 => "s16_rate_655350.flac",
    // Channels.
    three_channels => "s16_3ch.flac",
    four_channels => "s16_4ch.flac",
    five_one => "s16_51.flac",
    eight_channels => "s16_8ch.flac",
    // Subframe kinds.
    wasted_bits => "s16_wasted_bits.flac",
    silence_constant => "s16_silence_constant.flac",
    noise_verbatim => "s16_noise_verbatim.flac",
    // Metadata.
    tags_picture_seek_table => "s16_tags_picture_seektable.flac",
    no_padding_no_md5 => "s16_no_padding_no_md5.flac",
    id3v2_in_front => "id3v2_in_front.flac",
    no_metadata => "no_metadata.flac",
    // Damage.
    bits_flipped => "damaged_bitflips.flac",
    a_frame_removed => "damaged_frame_removed.flac",
    a_range_cut => "damaged_range_cut.flac",
    garbage_between_frames => "damaged_garbage.flac",
    cut_short => "damaged_truncated.flac",
    a_header_crc_broken => "damaged_header_crc.flac",
    a_sync_code_inside_a_header => "damaged_header_sync_inside.flac",
    a_tag_too_long => "damaged_tags_length.flac",
}
