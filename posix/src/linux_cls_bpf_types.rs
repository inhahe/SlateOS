//! `<linux/pkt_cls.h>` — TC BPF classifier constants.
//!
//! Traffic control BPF classifier constants covering attribute types
//! and classifier flags.

// ---------------------------------------------------------------------------
// TC BPF classifier attribute types
// ---------------------------------------------------------------------------

/// Unspec.
pub const TCA_BPF_UNSPEC: u32 = 0;
/// Action.
pub const TCA_BPF_ACT: u32 = 1;
/// Police.
pub const TCA_BPF_POLICE: u32 = 2;
/// Class ID.
pub const TCA_BPF_CLASSID: u32 = 3;
/// Ops length.
pub const TCA_BPF_OPS_LEN: u32 = 4;
/// Ops.
pub const TCA_BPF_OPS: u32 = 5;
/// File descriptor.
pub const TCA_BPF_FD: u32 = 6;
/// Name.
pub const TCA_BPF_NAME: u32 = 7;
/// Flags.
pub const TCA_BPF_FLAGS: u32 = 8;
/// Flags gen.
pub const TCA_BPF_FLAGS_GEN: u32 = 9;
/// Tag.
pub const TCA_BPF_TAG: u32 = 10;
/// ID.
pub const TCA_BPF_ID: u32 = 11;

// ---------------------------------------------------------------------------
// TC BPF flags
// ---------------------------------------------------------------------------

/// Direct action mode.
pub const TCA_BPF_FLAG_ACT_DIRECT: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_attrs_distinct() {
        let attrs = [
            TCA_BPF_UNSPEC, TCA_BPF_ACT, TCA_BPF_POLICE,
            TCA_BPF_CLASSID, TCA_BPF_OPS_LEN, TCA_BPF_OPS,
            TCA_BPF_FD, TCA_BPF_NAME, TCA_BPF_FLAGS,
            TCA_BPF_FLAGS_GEN, TCA_BPF_TAG, TCA_BPF_ID,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_flag_power_of_two() {
        assert!(TCA_BPF_FLAG_ACT_DIRECT.is_power_of_two());
    }
}
