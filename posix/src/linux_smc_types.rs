//! `<linux/smc.h>` — SMC (Shared Memory Communication) constants.
//!
//! SMC is IBM's networking protocol that transparently replaces TCP
//! connections with RDMA-based shared memory when both endpoints are
//! on the same fabric (typically IBM Z mainframes or POWER systems).
//! It appears as a regular socket to applications but uses direct
//! memory access for data transfer, achieving much lower latency.

// ---------------------------------------------------------------------------
// SMC socket options
// ---------------------------------------------------------------------------

/// SMC protocol family number.
pub const PF_SMC: u32 = 43;
/// SMC address family.
pub const AF_SMC: u32 = 43;

// ---------------------------------------------------------------------------
// SMC protocol types
// ---------------------------------------------------------------------------

/// SMC over RDMA (SMC-R, InfiniBand).
pub const SMC_PROT_SMC: u32 = 0;
/// SMC over ISM (SMC-D, direct memory, IBM Z).
pub const SMC_PROT_SMCD: u32 = 1;

// ---------------------------------------------------------------------------
// SMC socket options (SOL_SMC level)
// ---------------------------------------------------------------------------

/// SMC-specific socket option level.
pub const SOL_SMC: u32 = 286;
/// Cork (batch) outgoing data.
pub const SMC_CORK: u32 = 1;
/// Enable/disable nodelay (like TCP_NODELAY).
pub const SMC_NODELAY: u32 = 2;
/// Limit send buffer size.
pub const SMC_SNDBUF: u32 = 3;
/// Limit receive buffer size.
pub const SMC_RCVBUF: u32 = 4;

// ---------------------------------------------------------------------------
// SMC CLC (Connection Layer Control) types
// ---------------------------------------------------------------------------

/// CLC proposal message.
pub const SMC_CLC_PROPOSAL: u32 = 1;
/// CLC accept message.
pub const SMC_CLC_ACCEPT: u32 = 2;
/// CLC confirm message.
pub const SMC_CLC_CONFIRM: u32 = 3;
/// CLC decline message.
pub const SMC_CLC_DECLINE: u32 = 4;

// ---------------------------------------------------------------------------
// SMC decline reasons
// ---------------------------------------------------------------------------

/// Peer does not support SMC.
pub const SMC_CLC_DECL_NOSMC: u32 = 0x0100;
/// No ISM device available.
pub const SMC_CLC_DECL_NOISM: u32 = 0x0200;
/// No RDMA device available.
pub const SMC_CLC_DECL_NOLNK: u32 = 0x0300;
/// Peer uses different IP subnet.
pub const SMC_CLC_DECL_DIFFPREFIX: u32 = 0x0400;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_family() {
        assert_eq!(PF_SMC, AF_SMC);
        assert_eq!(AF_SMC, 43);
    }

    #[test]
    fn test_protocol_types_distinct() {
        assert_ne!(SMC_PROT_SMC, SMC_PROT_SMCD);
    }

    #[test]
    fn test_socket_options_distinct() {
        let opts = [SMC_CORK, SMC_NODELAY, SMC_SNDBUF, SMC_RCVBUF];
        for i in 0..opts.len() {
            for j in (i + 1)..opts.len() {
                assert_ne!(opts[i], opts[j]);
            }
        }
    }

    #[test]
    fn test_clc_types_distinct() {
        let types = [
            SMC_CLC_PROPOSAL,
            SMC_CLC_ACCEPT,
            SMC_CLC_CONFIRM,
            SMC_CLC_DECLINE,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_decline_reasons_distinct() {
        let reasons = [
            SMC_CLC_DECL_NOSMC,
            SMC_CLC_DECL_NOISM,
            SMC_CLC_DECL_NOLNK,
            SMC_CLC_DECL_DIFFPREFIX,
        ];
        for i in 0..reasons.len() {
            for j in (i + 1)..reasons.len() {
                assert_ne!(reasons[i], reasons[j]);
            }
        }
    }

    #[test]
    fn test_sol_smc() {
        assert_eq!(SOL_SMC, 286);
    }
}
