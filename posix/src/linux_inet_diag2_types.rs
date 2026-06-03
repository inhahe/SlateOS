//! `<linux/inet_diag.h>` — Additional inet diagnostics constants.
//!
//! Supplementary inet socket diagnostics constants covering
//! attribute types, bytecode operations, and extension flags.

// ---------------------------------------------------------------------------
// Inet diagnostics attribute types
// ---------------------------------------------------------------------------

/// Unspec.
pub const INET_DIAG_NONE: u32 = 0;
/// Memory info.
pub const INET_DIAG_MEMINFO: u32 = 1;
/// Info.
pub const INET_DIAG_INFO: u32 = 2;
/// Vegas info.
pub const INET_DIAG_VEGASINFO: u32 = 3;
/// Congestion algorithm.
pub const INET_DIAG_CONG: u32 = 4;
/// TOS.
pub const INET_DIAG_TOS: u32 = 5;
/// Traffic class.
pub const INET_DIAG_TCLASS: u32 = 6;
/// Socket memory info.
pub const INET_DIAG_SKMEMINFO: u32 = 7;
/// Shutdown.
pub const INET_DIAG_SHUTDOWN: u32 = 8;
/// DCTCP info.
pub const INET_DIAG_DCTCPINFO: u32 = 9;
/// Protocol.
pub const INET_DIAG_PROTOCOL: u32 = 10;
/// Socket opt stats.
pub const INET_DIAG_SKV6ONLY: u32 = 11;
/// Locals.
pub const INET_DIAG_LOCALS: u32 = 12;
/// Peers.
pub const INET_DIAG_PEERS: u32 = 13;
/// Pad.
pub const INET_DIAG_PAD: u32 = 14;
/// Mark.
pub const INET_DIAG_MARK: u32 = 15;
/// BBR info.
pub const INET_DIAG_BBRINFO: u32 = 16;
/// Class ID.
pub const INET_DIAG_CLASS_ID: u32 = 17;
/// Timestamp.
pub const INET_DIAG_MD5SIG: u32 = 18;
/// ULP info.
pub const INET_DIAG_ULP_INFO: u32 = 19;
/// Socket opt stats.
pub const INET_DIAG_SK_BPF_STORAGES: u32 = 20;
/// CGroup ID.
pub const INET_DIAG_CGROUP_ID: u32 = 21;
/// Socket cookie.
pub const INET_DIAG_SOCKOPT: u32 = 22;

// ---------------------------------------------------------------------------
// Inet diagnostics bytecode operations
// ---------------------------------------------------------------------------

/// No-op.
pub const INET_DIAG_BC_NOP: u32 = 0;
/// Jump if address.
pub const INET_DIAG_BC_JMP: u32 = 1;
/// Source address in range.
pub const INET_DIAG_BC_S_COND: u32 = 2;
/// Dest address in range.
pub const INET_DIAG_BC_D_COND: u32 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_attrs_distinct() {
        let attrs = [
            INET_DIAG_NONE,
            INET_DIAG_MEMINFO,
            INET_DIAG_INFO,
            INET_DIAG_VEGASINFO,
            INET_DIAG_CONG,
            INET_DIAG_TOS,
            INET_DIAG_TCLASS,
            INET_DIAG_SKMEMINFO,
            INET_DIAG_SHUTDOWN,
            INET_DIAG_DCTCPINFO,
            INET_DIAG_PROTOCOL,
            INET_DIAG_SKV6ONLY,
            INET_DIAG_LOCALS,
            INET_DIAG_PEERS,
            INET_DIAG_PAD,
            INET_DIAG_MARK,
            INET_DIAG_BBRINFO,
            INET_DIAG_CLASS_ID,
            INET_DIAG_MD5SIG,
            INET_DIAG_ULP_INFO,
            INET_DIAG_SK_BPF_STORAGES,
            INET_DIAG_CGROUP_ID,
            INET_DIAG_SOCKOPT,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_bytecode_ops_distinct() {
        let ops = [
            INET_DIAG_BC_NOP,
            INET_DIAG_BC_JMP,
            INET_DIAG_BC_S_COND,
            INET_DIAG_BC_D_COND,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }
}
