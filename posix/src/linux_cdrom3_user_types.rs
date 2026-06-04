//! `<linux/cdrom.h>` (part 3) — CD/DVD/BD media constants and DVD-CSS.
//!
//! Beyond the original CD-ROM ioctls, the same driver also exposes
//! generic packet commands and DVD/Blu-ray authentication primitives
//! (CSS, region masks, AACS). This module captures those user-visible
//! constants.

// ---------------------------------------------------------------------------
// Sector layout
// ---------------------------------------------------------------------------

/// CD raw frame size (2352 bytes/sector).
pub const CD_FRAMESIZE_RAW: usize = 2_352;

/// CD-DA frame size (yellow-book mode-1 user data).
pub const CD_FRAMESIZE: usize = 2_048;

/// CD audio sample rate (Hz).
pub const CD_AUDIO_FRAMERATE: u32 = 75;

/// Frames per second of audio CD playback (75 sectors/s).
pub const CD_FRAMES_PER_SECOND: u32 = 75;

/// Minutes/seconds/frames maximum MSF time.
pub const CD_MSF_OFFSET: u32 = 150;

// ---------------------------------------------------------------------------
// DVD authentication / CSS
// ---------------------------------------------------------------------------

/// CSS region mask — one bit per region (8 regions).
pub const DVD_REGION_MASK_ALL: u8 = 0xFF;

/// Maximum number of region changes allowed before the drive locks.
pub const DVD_REGION_CHANGE_COUNT_MAX: u32 = 5;

/// CSS key size (40-bit key stored in 5 bytes).
pub const DVD_CSS_KEY_LEN: usize = 5;

/// Disc-key size (2048 entries × 5 bytes).
pub const DVD_DISC_KEY_LEN: usize = 2_048 * 5;

// ---------------------------------------------------------------------------
// DVD struct types (`dvd_struct.type`)
// ---------------------------------------------------------------------------

pub const DVD_STRUCT_PHYSICAL: u32 = 0x00;
pub const DVD_STRUCT_COPYRIGHT: u32 = 0x01;
pub const DVD_STRUCT_DISCKEY: u32 = 0x02;
pub const DVD_STRUCT_BCA: u32 = 0x03;
pub const DVD_STRUCT_MANUFACT: u32 = 0x04;

// ---------------------------------------------------------------------------
// DVD authentication phases (`dvd_authinfo.type`)
// ---------------------------------------------------------------------------

pub const DVD_LU_SEND_AGID: u32 = 0;
pub const DVD_HOST_SEND_CHALLENGE: u32 = 1;
pub const DVD_LU_SEND_KEY1: u32 = 2;
pub const DVD_LU_SEND_CHALLENGE: u32 = 3;
pub const DVD_HOST_SEND_KEY2: u32 = 4;
pub const DVD_AUTH_ESTABLISHED: u32 = 5;
pub const DVD_AUTH_FAILURE: u32 = 6;
pub const DVD_LU_SEND_TITLE_KEY: u32 = 7;
pub const DVD_LU_SEND_ASF: u32 = 8;
pub const DVD_INVALIDATE_AGID: u32 = 9;
pub const DVD_LU_SEND_RPC_STATE: u32 = 10;
pub const DVD_HOST_SEND_RPC_STATE: u32 = 11;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frame_sizes() {
        assert_eq!(CD_FRAMESIZE_RAW, 2_352);
        assert_eq!(CD_FRAMESIZE, 2_048);
        // Raw frame leaves overhead for header + ECC: 2352 - 2048 = 304.
        assert_eq!(CD_FRAMESIZE_RAW - CD_FRAMESIZE, 304);
        // Mode-1 user data is one logical block.
        assert!(CD_FRAMESIZE.is_power_of_two());
    }

    #[test]
    fn test_audio_timing() {
        assert_eq!(CD_AUDIO_FRAMERATE, 75);
        assert_eq!(CD_FRAMES_PER_SECOND, CD_AUDIO_FRAMERATE);
        // Yellow-book: 2 second pre-gap = 150 sectors.
        assert_eq!(CD_MSF_OFFSET, 2 * CD_FRAMES_PER_SECOND);
    }

    #[test]
    fn test_dvd_region_mask_and_change_limit() {
        // All 8 region bits set.
        assert_eq!(DVD_REGION_MASK_ALL, 0xFF);
        assert_eq!(DVD_REGION_MASK_ALL.count_ones(), 8);
        // 5 region changes is the consumer-drive lock threshold.
        assert_eq!(DVD_REGION_CHANGE_COUNT_MAX, 5);
    }

    #[test]
    fn test_css_key_sizes() {
        // 40-bit key in 5 bytes.
        assert_eq!(DVD_CSS_KEY_LEN, 5);
        // 2048 5-byte entries.
        assert_eq!(DVD_DISC_KEY_LEN, 2_048 * 5);
        assert_eq!(DVD_DISC_KEY_LEN / DVD_CSS_KEY_LEN, 2_048);
    }

    #[test]
    fn test_dvd_struct_types_dense_0_to_4() {
        let t = [
            DVD_STRUCT_PHYSICAL,
            DVD_STRUCT_COPYRIGHT,
            DVD_STRUCT_DISCKEY,
            DVD_STRUCT_BCA,
            DVD_STRUCT_MANUFACT,
        ];
        for (i, &v) in t.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_dvd_auth_states_dense_0_to_11() {
        let a = [
            DVD_LU_SEND_AGID,
            DVD_HOST_SEND_CHALLENGE,
            DVD_LU_SEND_KEY1,
            DVD_LU_SEND_CHALLENGE,
            DVD_HOST_SEND_KEY2,
            DVD_AUTH_ESTABLISHED,
            DVD_AUTH_FAILURE,
            DVD_LU_SEND_TITLE_KEY,
            DVD_LU_SEND_ASF,
            DVD_INVALIDATE_AGID,
            DVD_LU_SEND_RPC_STATE,
            DVD_HOST_SEND_RPC_STATE,
        ];
        for (i, &v) in a.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        // ESTABLISHED and FAILURE are adjacent terminal states.
        assert_eq!(DVD_AUTH_FAILURE - DVD_AUTH_ESTABLISHED, 1);
    }
}
