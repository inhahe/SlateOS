//! `<uuid/uuid.h>` (libuuid) — 128-bit Universally Unique Identifiers.
//!
//! UUIDs are 16-byte big-endian identifiers used by `/etc/fstab`
//! (`UUID=…`), GPT partition tables, GlusterFS, the kernel keyring,
//! and D-Bus. Linux exposes a random UUID at `/proc/sys/kernel/random/uuid`
//! and uses RFC 4122 variant/version encoding.

// ---------------------------------------------------------------------------
// Sizes
// ---------------------------------------------------------------------------

/// Raw UUID is 128 bits — always 16 bytes.
pub const UUID_BIN_LEN: usize = 16;

/// Canonical "8-4-4-4-12" string is 36 chars (`xxxxxxxx-xxxx-...`).
pub const UUID_STR_LEN: usize = 36;

/// String + trailing NUL.
pub const UUID_STR_CSTR_LEN: usize = 37;

// ---------------------------------------------------------------------------
// Common sentinels
// ---------------------------------------------------------------------------

/// `00000000-0000-0000-0000-000000000000` — the "nil" UUID.
pub const UUID_NIL: [u8; 16] = [0; 16];

/// `FFFFFFFF-FFFF-FFFF-FFFF-FFFFFFFFFFFF` — the "max" UUID (RFC 9562).
pub const UUID_MAX: [u8; 16] = [0xFF; 16];

// ---------------------------------------------------------------------------
// Byte offsets inside the canonical layout
// ---------------------------------------------------------------------------

/// `time_low` — bytes 0..4 (big-endian u32).
pub const UUID_OFF_TIME_LOW: usize = 0;
/// `time_mid` — bytes 4..6 (big-endian u16).
pub const UUID_OFF_TIME_MID: usize = 4;
/// `time_hi_and_version` — bytes 6..8 (high 4 bits = version).
pub const UUID_OFF_TIME_HI_VER: usize = 6;
/// `clock_seq_hi_and_reserved` — byte 8 (top bits = variant).
pub const UUID_OFF_CLOCK_SEQ_HI: usize = 8;
/// `clock_seq_low` — byte 9.
pub const UUID_OFF_CLOCK_SEQ_LOW: usize = 9;
/// `node` — bytes 10..16 (MAC or random).
pub const UUID_OFF_NODE: usize = 10;

// ---------------------------------------------------------------------------
// String dash positions (after every 8, 4, 4, 4 nibbles)
// ---------------------------------------------------------------------------

pub const UUID_DASH_POS_1: usize = 8;
pub const UUID_DASH_POS_2: usize = 13;
pub const UUID_DASH_POS_3: usize = 18;
pub const UUID_DASH_POS_4: usize = 23;

// ---------------------------------------------------------------------------
// RFC 4122 versions (top 4 bits of byte 6)
// ---------------------------------------------------------------------------

pub const UUID_VERSION_TIME: u8 = 1;
pub const UUID_VERSION_DCE: u8 = 2;
pub const UUID_VERSION_MD5: u8 = 3;
pub const UUID_VERSION_RANDOM: u8 = 4;
pub const UUID_VERSION_SHA1: u8 = 5;
pub const UUID_VERSION_TIME_REORDERED: u8 = 6;
pub const UUID_VERSION_UNIX_EPOCH: u8 = 7;
pub const UUID_VERSION_CUSTOM: u8 = 8;

// ---------------------------------------------------------------------------
// Variants — top bits of byte 8
// ---------------------------------------------------------------------------

/// NCS backwards-compat (top bit 0).
pub const UUID_VARIANT_NCS: u8 = 0x00;
/// RFC 4122 (top bits 10).
pub const UUID_VARIANT_RFC4122: u8 = 0x80;
/// Microsoft (top bits 110).
pub const UUID_VARIANT_MICROSOFT: u8 = 0xC0;
/// Reserved (top bits 111).
pub const UUID_VARIANT_FUTURE: u8 = 0xE0;

// ---------------------------------------------------------------------------
// Kernel UUID sources
// ---------------------------------------------------------------------------

pub const PROC_RANDOM_UUID: &str = "/proc/sys/kernel/random/uuid";
pub const PROC_BOOT_ID: &str = "/proc/sys/kernel/random/boot_id";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_size_constants() {
        assert_eq!(UUID_BIN_LEN, 16);
        // 8 + 1 + 4 + 1 + 4 + 1 + 4 + 1 + 12 = 36.
        assert_eq!(UUID_STR_LEN, 8 + 1 + 4 + 1 + 4 + 1 + 4 + 1 + 12);
        assert_eq!(UUID_STR_CSTR_LEN, UUID_STR_LEN + 1);
    }

    #[test]
    fn test_nil_and_max_distinct() {
        assert_eq!(UUID_NIL.len(), UUID_BIN_LEN);
        assert_eq!(UUID_MAX.len(), UUID_BIN_LEN);
        assert_ne!(UUID_NIL, UUID_MAX);
        for &b in &UUID_NIL {
            assert_eq!(b, 0);
        }
        for &b in &UUID_MAX {
            assert_eq!(b, 0xFF);
        }
    }

    #[test]
    fn test_byte_offsets_dense_and_cover_all_16() {
        // The field offsets walk through the buffer with no gap.
        assert_eq!(UUID_OFF_TIME_LOW, 0);
        assert_eq!(UUID_OFF_TIME_MID, 4);
        assert_eq!(UUID_OFF_TIME_HI_VER, 6);
        assert_eq!(UUID_OFF_CLOCK_SEQ_HI, 8);
        assert_eq!(UUID_OFF_CLOCK_SEQ_LOW, 9);
        assert_eq!(UUID_OFF_NODE, 10);
        // node is 6 bytes — totals to UUID_BIN_LEN.
        assert_eq!(UUID_OFF_NODE + 6, UUID_BIN_LEN);
    }

    #[test]
    fn test_dash_positions_inside_36_char_string() {
        for p in [UUID_DASH_POS_1, UUID_DASH_POS_2, UUID_DASH_POS_3, UUID_DASH_POS_4] {
            assert!(p < UUID_STR_LEN);
        }
        // Each successive dash is 5 chars apart (4 hex + 1 dash).
        assert_eq!(UUID_DASH_POS_2 - UUID_DASH_POS_1, 5);
        assert_eq!(UUID_DASH_POS_3 - UUID_DASH_POS_2, 5);
        assert_eq!(UUID_DASH_POS_4 - UUID_DASH_POS_3, 5);
    }

    #[test]
    fn test_versions_dense_1_to_8() {
        let v = [
            UUID_VERSION_TIME,
            UUID_VERSION_DCE,
            UUID_VERSION_MD5,
            UUID_VERSION_RANDOM,
            UUID_VERSION_SHA1,
            UUID_VERSION_TIME_REORDERED,
            UUID_VERSION_UNIX_EPOCH,
            UUID_VERSION_CUSTOM,
        ];
        for (i, &v) in v.iter().enumerate() {
            assert_eq!(v as usize, i + 1);
        }
        // Version is 4 bits, so values must fit in 0..16.
        for v in v {
            assert!(v < 16);
        }
    }

    #[test]
    fn test_variant_top_bits_match_rfc4122_table() {
        // NCS: 0xxxxxxx, RFC4122: 10xxxxxx, MS: 110xxxxx, Future: 111xxxxx.
        assert_eq!(UUID_VARIANT_NCS & 0x80, 0x00);
        assert_eq!(UUID_VARIANT_RFC4122 & 0xC0, 0x80);
        assert_eq!(UUID_VARIANT_MICROSOFT & 0xE0, 0xC0);
        assert_eq!(UUID_VARIANT_FUTURE & 0xE0, 0xE0);
    }

    #[test]
    fn test_kernel_uuid_paths() {
        // /proc/sys/kernel/random exposes both a fresh-each-read UUID
        // and a system-life-time boot ID.
        assert!(PROC_RANDOM_UUID.ends_with("/uuid"));
        assert!(PROC_BOOT_ID.ends_with("/boot_id"));
    }
}
