//! The element IDs this reads, as RFC 8794 (EBML) and RFC 9559 (Matroska)
//! number them, marker bits kept.

use crate::ebml::Id;

// EBML.
pub const EBML: Id = 0x1A45_DFA3;
/// Void and CRC-32: EBML's own, which may stand anywhere.
pub const VOID: Id = 0xEC;
pub const CRC_32: Id = 0xBF;
pub const EBML_READ_VERSION: Id = 0x42F7;
pub const EBML_MAX_ID_LENGTH: Id = 0x42F2;
pub const EBML_MAX_SIZE_LENGTH: Id = 0x42F3;
pub const DOC_TYPE: Id = 0x4282;
pub const DOC_TYPE_READ_VERSION: Id = 0x4285;

// The segment and its top-level elements.
pub const SEGMENT: Id = 0x1853_8067;
pub const SEEK_HEAD: Id = 0x114D_9B74;
pub const INFO: Id = 0x1549_A966;
pub const TRACKS: Id = 0x1654_AE6B;
pub const CUES: Id = 0x1C53_BB6B;
pub const CLUSTER: Id = 0x1F43_B675;
pub const CHAPTERS: Id = 0x1043_A770;
pub const TAGS: Id = 0x1254_C367;
pub const ATTACHMENTS: Id = 0x1941_A469;

// SeekHead.
pub const SEEK: Id = 0x4DBB;
pub const SEEK_ID: Id = 0x53AB;
pub const SEEK_POSITION: Id = 0x53AC;

// Info.
pub const TIMESTAMP_SCALE: Id = 0x2A_D7B1;
pub const DURATION: Id = 0x4489;
pub const TITLE: Id = 0x7BA9;
pub const MUXING_APP: Id = 0x4D80;
pub const WRITING_APP: Id = 0x5741;

// Tracks.
pub const TRACK_ENTRY: Id = 0xAE;
pub const TRACK_NUMBER: Id = 0xD7;
pub const TRACK_UID: Id = 0x73C5;
pub const TRACK_TYPE: Id = 0x83;
pub const FLAG_ENABLED: Id = 0xB9;
pub const FLAG_DEFAULT: Id = 0x88;
pub const FLAG_FORCED: Id = 0x55AA;
pub const DEFAULT_DURATION: Id = 0x23_E383;
pub const TRACK_TIMESTAMP_SCALE: Id = 0x23_314F;
pub const NAME: Id = 0x536E;
pub const LANGUAGE: Id = 0x22_B59C;
pub const CODEC_ID: Id = 0x86;
pub const CODEC_PRIVATE: Id = 0x63A2;
pub const CODEC_DELAY: Id = 0x56AA;
pub const SEEK_PRE_ROLL: Id = 0x56BB;
pub const VIDEO: Id = 0xE0;
pub const AUDIO: Id = 0xE1;
pub const CONTENT_ENCODINGS: Id = 0x6D80;

// Video.
pub const FLAG_INTERLACED: Id = 0x9A;
pub const STEREO_MODE: Id = 0x53B8;
pub const ALPHA_MODE: Id = 0x53C0;
pub const PIXEL_WIDTH: Id = 0xB0;
pub const PIXEL_HEIGHT: Id = 0xBA;
pub const PIXEL_CROP_BOTTOM: Id = 0x54AA;
pub const PIXEL_CROP_TOP: Id = 0x54BB;
pub const PIXEL_CROP_LEFT: Id = 0x54CC;
pub const PIXEL_CROP_RIGHT: Id = 0x54DD;
pub const DISPLAY_WIDTH: Id = 0x54B0;
pub const DISPLAY_HEIGHT: Id = 0x54BA;
pub const DISPLAY_UNIT: Id = 0x54B2;
pub const COLOUR: Id = 0x55B0;
pub const MATRIX_COEFFICIENTS: Id = 0x55B1;
pub const BITS_PER_CHANNEL: Id = 0x55B2;
pub const CHROMA_SUBSAMPLING_HORZ: Id = 0x55B3;
pub const CHROMA_SUBSAMPLING_VERT: Id = 0x55B4;
pub const CHROMA_SITING_HORZ: Id = 0x55B7;
pub const CHROMA_SITING_VERT: Id = 0x55B8;
pub const RANGE: Id = 0x55B9;
pub const TRANSFER_CHARACTERISTICS: Id = 0x55BA;
pub const PRIMARIES: Id = 0x55BB;
pub const PROJECTION: Id = 0x7670;
pub const PROJECTION_TYPE: Id = 0x7671;
pub const PROJECTION_PRIVATE: Id = 0x7672;
pub const PROJECTION_POSE_YAW: Id = 0x7673;
pub const PROJECTION_POSE_PITCH: Id = 0x7674;
pub const PROJECTION_POSE_ROLL: Id = 0x7675;

// Audio.
pub const SAMPLING_FREQUENCY: Id = 0xB5;
pub const OUTPUT_SAMPLING_FREQUENCY: Id = 0x78B5;
pub const CHANNELS: Id = 0x9F;
pub const BIT_DEPTH: Id = 0x6264;

// ContentEncodings.
pub const CONTENT_ENCODING: Id = 0x6240;
pub const CONTENT_ENCODING_SCOPE: Id = 0x5032;
pub const CONTENT_ENCODING_TYPE: Id = 0x5033;
pub const CONTENT_COMPRESSION: Id = 0x5034;
pub const CONTENT_COMP_ALGO: Id = 0x4254;
pub const CONTENT_COMP_SETTINGS: Id = 0x4255;

// Cues.
pub const CUE_POINT: Id = 0xBB;
pub const CUE_TIME: Id = 0xB3;
pub const CUE_TRACK_POSITIONS: Id = 0xB7;
pub const CUE_TRACK: Id = 0xF7;
pub const CUE_CLUSTER_POSITION: Id = 0xF1;

// Cluster.
pub const TIMESTAMP: Id = 0xE7;
pub const CLUSTER_POSITION: Id = 0xA7;
pub const CLUSTER_PREV_SIZE: Id = 0xAB;
pub const SIMPLE_BLOCK: Id = 0xA3;
pub const BLOCK_GROUP: Id = 0xA0;
pub const BLOCK: Id = 0xA1;
pub const BLOCK_DURATION: Id = 0x9B;
pub const REFERENCE_BLOCK: Id = 0xFB;
pub const CODEC_STATE: Id = 0xA4;
pub const DISCARD_PADDING: Id = 0x75A2;
pub const BLOCK_ADDITIONS: Id = 0x75A1;
pub const BLOCK_MORE: Id = 0xA6;
pub const BLOCK_ADD_ID: Id = 0xEE;
pub const BLOCK_ADDITIONAL: Id = 0xA5;

/// Whether `id` is one of the Segment's own children: the elements that end
/// a Cluster of unknown size when one begins.
pub const fn is_top_level(id: Id) -> bool {
    matches!(
        id,
        SEEK_HEAD | INFO | TRACKS | CUES | CLUSTER | CHAPTERS | TAGS | ATTACHMENTS
    )
}
