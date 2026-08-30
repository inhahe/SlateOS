//! `<linux/smc.h>` — Shared Memory Communication socket family.
//!
//! SMC-R and SMC-D are zero-copy, RDMA-accelerated socket protocols
//! used heavily on IBM Z. Linux exposes them via `AF_SMC` with a
//! `SOCK_STREAM` interface that transparently falls back to TCP if
//! the peer doesn't support SMC. The constants here are the
//! getsockopt/setsockopt and netlink ABI.

// ---------------------------------------------------------------------------
// Address family / proto
// ---------------------------------------------------------------------------

pub const AF_SMC: u32 = 43;
pub const PF_SMC: u32 = AF_SMC;

pub const SMCPROTO_SMC: u32 = 0;
pub const SMCPROTO_SMC6: u32 = 1;

pub const SOL_SMC: u32 = 286;

// ---------------------------------------------------------------------------
// `SOL_SMC` socket options (`SMC_*`)
// ---------------------------------------------------------------------------

pub const SMC_FALLBACK: u32 = 1;
pub const SMC_LIMIT_HS: u32 = 2;
pub const SMC_AUTOCORKING: u32 = 3;

// ---------------------------------------------------------------------------
// SMC link-group state / role
// ---------------------------------------------------------------------------

pub const SMC_LGR_NONE: u8 = 0;
pub const SMC_LGR_FREE: u8 = 1;
pub const SMC_LGR_ASYMMETRIC_LOCAL: u8 = 2;
pub const SMC_LGR_ASYMMETRIC_PEER: u8 = 3;
pub const SMC_LGR_SYMMETRIC: u8 = 4;

pub const SMC_NETLINK_GENL_NAME: &str = "SMC_GEN_NETLINK";
pub const SMC_GENL_FAMILY_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// SMC fallback reason codes (subset)
// ---------------------------------------------------------------------------

pub const SMC_CLC_DECLINE: u32 = 0x03_03_00_00;
pub const SMC_CLC_DECL_IPSEC: u32 = 0x03_03_00_03;
pub const SMC_CLC_DECL_NOSMCDEV: u32 = 0x03_03_00_04;
pub const SMC_CLC_DECL_MEM: u32 = 0x03_03_00_06;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_af_smc_value() {
        // AF_SMC is 43 — added in Linux 4.11.
        assert_eq!(AF_SMC, 43);
        assert_eq!(PF_SMC, AF_SMC);
    }

    #[test]
    fn test_proto_dense_0_to_1() {
        assert_eq!(SMCPROTO_SMC, 0);
        assert_eq!(SMCPROTO_SMC6, 1);
    }

    #[test]
    fn test_sol_smc_above_legacy_levels() {
        // SOL_SMC=286 sits above all legacy SOL_* numbers (which top
        // out around 280) but below the next protocol family's bucket.
        assert_eq!(SOL_SMC, 286);
    }

    #[test]
    fn test_smc_sockopts_dense_1_to_3() {
        assert_eq!(SMC_FALLBACK, 1);
        assert_eq!(SMC_LIMIT_HS, 2);
        assert_eq!(SMC_AUTOCORKING, 3);
    }

    #[test]
    fn test_lgr_states_dense_0_to_4() {
        let l = [
            SMC_LGR_NONE,
            SMC_LGR_FREE,
            SMC_LGR_ASYMMETRIC_LOCAL,
            SMC_LGR_ASYMMETRIC_PEER,
            SMC_LGR_SYMMETRIC,
        ];
        for (i, &v) in l.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_clc_decline_codes_share_high_word() {
        // All CLC decline codes share the high 16 bits 0x0303.
        let c = [
            SMC_CLC_DECLINE,
            SMC_CLC_DECL_IPSEC,
            SMC_CLC_DECL_NOSMCDEV,
            SMC_CLC_DECL_MEM,
        ];
        for v in c {
            assert_eq!(v >> 16, 0x0303);
        }
    }
}
