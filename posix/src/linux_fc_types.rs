//! `<scsi/fc/fc_fs.h>` — Fibre Channel constants.
//!
//! Fibre Channel constants covering frame types,
//! service types, port types, and port states.

// ---------------------------------------------------------------------------
// FC frame routing (R_CTL)
// ---------------------------------------------------------------------------

/// Data frames.
pub const FC_RCTL_DD_UNCAT: u8 = 0x00;
/// Solicited data.
pub const FC_RCTL_DD_SOL_DATA: u8 = 0x01;
/// Unsolicited control.
pub const FC_RCTL_DD_UNSOL_CTL: u8 = 0x02;
/// Solicited control.
pub const FC_RCTL_DD_SOL_CTL: u8 = 0x03;
/// Unsolicited data.
pub const FC_RCTL_DD_UNSOL_DATA: u8 = 0x04;
/// Data descriptor.
pub const FC_RCTL_DD_DATA_DESC: u8 = 0x05;
/// Unsolicited command.
pub const FC_RCTL_DD_UNSOL_CMD: u8 = 0x06;
/// Command status.
pub const FC_RCTL_DD_CMD_STATUS: u8 = 0x07;
/// Extended link services.
pub const FC_RCTL_ELS_REQ: u8 = 0x22;
/// ELS response.
pub const FC_RCTL_ELS_REP: u8 = 0x23;

// ---------------------------------------------------------------------------
// FC type codes
// ---------------------------------------------------------------------------

/// Basic Link Service.
pub const FC_TYPE_BLS: u8 = 0x00;
/// Extended Link Service.
pub const FC_TYPE_ELS: u8 = 0x01;
/// FCP (SCSI).
pub const FC_TYPE_FCP: u8 = 0x08;
/// Common Transport.
pub const FC_TYPE_CT: u8 = 0xFC;
/// IP over FC.
pub const FC_TYPE_IP: u8 = 0x05;

// ---------------------------------------------------------------------------
// FC port types
// ---------------------------------------------------------------------------

/// Unknown port type.
pub const FC_PORTTYPE_UNKNOWN: u32 = 0;
/// Other port.
pub const FC_PORTTYPE_OTHER: u32 = 1;
/// Fabric port (F_Port).
pub const FC_PORTTYPE_NPORT: u32 = 2;
/// Node port (NL_Port).
pub const FC_PORTTYPE_NLPORT: u32 = 3;
/// Loop port (FL_Port).
pub const FC_PORTTYPE_LPORT: u32 = 4;
/// Point-to-point.
pub const FC_PORTTYPE_PTP: u32 = 5;
/// N-port ID virtualization.
pub const FC_PORTTYPE_NPIV: u32 = 6;

// ---------------------------------------------------------------------------
// FC port states
// ---------------------------------------------------------------------------

/// Unknown state.
pub const FC_PORTSTATE_UNKNOWN: u32 = 0;
/// Not present.
pub const FC_PORTSTATE_NOTPRESENT: u32 = 1;
/// Online.
pub const FC_PORTSTATE_ONLINE: u32 = 2;
/// Offline.
pub const FC_PORTSTATE_OFFLINE: u32 = 3;
/// Blocked.
pub const FC_PORTSTATE_BLOCKED: u32 = 4;
/// Bypassed.
pub const FC_PORTSTATE_BYPASSED: u32 = 5;
/// Diagnostics.
pub const FC_PORTSTATE_DIAGNOSTICS: u32 = 6;
/// Linkdown.
pub const FC_PORTSTATE_LINKDOWN: u32 = 7;
/// Error.
pub const FC_PORTSTATE_ERROR: u32 = 8;
/// Loopback.
pub const FC_PORTSTATE_LOOPBACK: u32 = 9;
/// Deleted.
pub const FC_PORTSTATE_DELETED: u32 = 10;
/// Marginal.
pub const FC_PORTSTATE_MARGINAL: u32 = 11;

// ---------------------------------------------------------------------------
// FC well-known addresses
// ---------------------------------------------------------------------------

/// Fabric controller.
pub const FC_FID_FLOGI: u32 = 0xFFFFFE;
/// Name server.
pub const FC_FID_FCTRL: u32 = 0xFFFFFD;
/// Broadcast.
pub const FC_FID_BCAST: u32 = 0xFFFFFF;
/// Directory server.
pub const FC_FID_DIR_SERV: u32 = 0xFFFFFC;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rctl_distinct() {
        let rctls: [u8; 10] = [
            FC_RCTL_DD_UNCAT,
            FC_RCTL_DD_SOL_DATA,
            FC_RCTL_DD_UNSOL_CTL,
            FC_RCTL_DD_SOL_CTL,
            FC_RCTL_DD_UNSOL_DATA,
            FC_RCTL_DD_DATA_DESC,
            FC_RCTL_DD_UNSOL_CMD,
            FC_RCTL_DD_CMD_STATUS,
            FC_RCTL_ELS_REQ,
            FC_RCTL_ELS_REP,
        ];
        for i in 0..rctls.len() {
            for j in (i + 1)..rctls.len() {
                assert_ne!(rctls[i], rctls[j]);
            }
        }
    }

    #[test]
    fn test_type_codes_distinct() {
        let types: [u8; 5] = [
            FC_TYPE_BLS,
            FC_TYPE_ELS,
            FC_TYPE_FCP,
            FC_TYPE_CT,
            FC_TYPE_IP,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_port_types_distinct() {
        let types = [
            FC_PORTTYPE_UNKNOWN,
            FC_PORTTYPE_OTHER,
            FC_PORTTYPE_NPORT,
            FC_PORTTYPE_NLPORT,
            FC_PORTTYPE_LPORT,
            FC_PORTTYPE_PTP,
            FC_PORTTYPE_NPIV,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_port_states_distinct() {
        let states = [
            FC_PORTSTATE_UNKNOWN,
            FC_PORTSTATE_NOTPRESENT,
            FC_PORTSTATE_ONLINE,
            FC_PORTSTATE_OFFLINE,
            FC_PORTSTATE_BLOCKED,
            FC_PORTSTATE_BYPASSED,
            FC_PORTSTATE_DIAGNOSTICS,
            FC_PORTSTATE_LINKDOWN,
            FC_PORTSTATE_ERROR,
            FC_PORTSTATE_LOOPBACK,
            FC_PORTSTATE_DELETED,
            FC_PORTSTATE_MARGINAL,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_well_known_addrs_distinct() {
        let addrs = [FC_FID_FLOGI, FC_FID_FCTRL, FC_FID_BCAST, FC_FID_DIR_SERV];
        for i in 0..addrs.len() {
            for j in (i + 1)..addrs.len() {
                assert_ne!(addrs[i], addrs[j]);
            }
        }
    }
}
