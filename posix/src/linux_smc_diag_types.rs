//! `<linux/smc_diag.h>` — SMC socket-diagnostic constants.
//!
//! Constants for the sock-diag interface exposed by SMC (Shared
//! Memory Communications — IBM zEnterprise / RoCE). `ss` and `smc-tools`
//! consume these to enumerate SMC sockets and link groups.

// ---------------------------------------------------------------------------
// sock_diag family identifiers
// ---------------------------------------------------------------------------

/// AF_SMC family identifier (matches kernel's AF_SMC = 43).
pub const AF_SMC: u32 = 43;
/// SMC sock-diag protocol value used in inet_diag_req2.
pub const SMC_DIAG_GETSOCK_MAX: u32 = 17;

// ---------------------------------------------------------------------------
// SMC connection states (matches sk->sk_state for SMC)
// ---------------------------------------------------------------------------

/// Connection inactive.
pub const SMC_INACTIVE: u8 = 1;
/// Connection initialised.
pub const SMC_INIT: u8 = 2;
/// Connection closed (TCP fallback path closed).
pub const SMC_CLOSED: u8 = 7;
/// Active connection.
pub const SMC_ACTIVE: u8 = 8;
/// Peer closing.
pub const SMC_PEERCLOSEWAIT1: u8 = 9;
/// Peer closed (wait 2).
pub const SMC_PEERCLOSEWAIT2: u8 = 10;
/// Application sent close.
pub const SMC_APPLCLOSEWAIT1: u8 = 11;
/// Application sent close (wait 2).
pub const SMC_APPLCLOSEWAIT2: u8 = 12;
/// Aborted.
pub const SMC_APPLFINCLOSEWAIT: u8 = 13;
/// Peer aborted.
pub const SMC_PEERFINCLOSEWAIT: u8 = 14;
/// Aborted before init.
pub const SMC_PEERABORTWAIT: u8 = 15;
/// Processing close.
pub const SMC_PROCESSABORT: u8 = 16;

// ---------------------------------------------------------------------------
// SMC sock-diag extension attribute types (struct smc_diag_msg attrs)
// ---------------------------------------------------------------------------

/// No extension.
pub const SMC_DIAG_NONE: u16 = 0;
/// Connection info (struct smc_diag_conninfo).
pub const SMC_DIAG_CONNINFO: u16 = 1;
/// Link-group info.
pub const SMC_DIAG_LGRINFO: u16 = 2;
/// SHMEM info (struct smc_diag_shmem).
pub const SMC_DIAG_SHUTDOWN: u16 = 3;
/// Per-DMA info.
pub const SMC_DIAG_DMBINFO: u16 = 4;
/// Fallback reason.
pub const SMC_DIAG_FALLBACK: u16 = 5;

// ---------------------------------------------------------------------------
// SMC modes
// ---------------------------------------------------------------------------

/// SMC-R over RoCE.
pub const SMC_TYPE_R: u8 = 0;
/// SMC-D over IBM ISM.
pub const SMC_TYPE_D: u8 = 1;
/// Both SMC-R and SMC-D supported.
pub const SMC_TYPE_B: u8 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_af_smc_value() {
        // AF_SMC must be the well-known kernel value 43, otherwise
        // any sock-diag request mis-targets a different family.
        assert_eq!(AF_SMC, 43);
    }

    #[test]
    fn test_states_distinct() {
        let states = [
            SMC_INACTIVE,
            SMC_INIT,
            SMC_CLOSED,
            SMC_ACTIVE,
            SMC_PEERCLOSEWAIT1,
            SMC_PEERCLOSEWAIT2,
            SMC_APPLCLOSEWAIT1,
            SMC_APPLCLOSEWAIT2,
            SMC_APPLFINCLOSEWAIT,
            SMC_PEERFINCLOSEWAIT,
            SMC_PEERABORTWAIT,
            SMC_PROCESSABORT,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_diag_attrs_distinct() {
        let attrs = [
            SMC_DIAG_NONE,
            SMC_DIAG_CONNINFO,
            SMC_DIAG_LGRINFO,
            SMC_DIAG_SHUTDOWN,
            SMC_DIAG_DMBINFO,
            SMC_DIAG_FALLBACK,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_types_distinct() {
        let ts = [SMC_TYPE_R, SMC_TYPE_D, SMC_TYPE_B];
        for i in 0..ts.len() {
            for j in (i + 1)..ts.len() {
                assert_ne!(ts[i], ts[j]);
            }
        }
        // TYPE_B is the OR of R and D (R=0, D=1, both => 3).
        assert_eq!(SMC_TYPE_B & (1 << SMC_TYPE_D), 1 << SMC_TYPE_D);
    }
}
