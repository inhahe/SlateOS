//! `<scsi/scsi_transport.h>` — SCSI transport protocol constants.
//!
//! SCSI commands travel over different physical transports: SAS
//! (Serial Attached SCSI), FC (Fibre Channel), iSCSI (IP-based),
//! SBP (FireWire), SRP (InfiniBand), and USB. Each transport has
//! its own speed, topology, and error recovery characteristics.
//! The Linux SCSI transport classes expose transport-specific
//! attributes via sysfs.

// ---------------------------------------------------------------------------
// SCSI transport protocols
// ---------------------------------------------------------------------------

/// FCP (Fibre Channel Protocol).
pub const SCSI_TRANSPORT_FCP: u32 = 0;
/// SPI (Parallel SCSI, legacy).
pub const SCSI_TRANSPORT_SPI: u32 = 1;
/// SAS (Serial Attached SCSI).
pub const SCSI_TRANSPORT_SAS: u32 = 2;
/// iSCSI (SCSI over IP).
pub const SCSI_TRANSPORT_ISCSI: u32 = 3;
/// SBP (Serial Bus Protocol, FireWire).
pub const SCSI_TRANSPORT_SBP: u32 = 4;
/// SRP (SCSI RDMA Protocol, InfiniBand).
pub const SCSI_TRANSPORT_SRP: u32 = 5;
/// USB Attached SCSI (UAS/BOT).
pub const SCSI_TRANSPORT_USB: u32 = 6;
/// ATA over SCSI (libata translation).
pub const SCSI_TRANSPORT_ATA: u32 = 7;

// ---------------------------------------------------------------------------
// SAS link rates
// ---------------------------------------------------------------------------

/// 1.5 Gbps (SAS-1 / SATA I).
pub const SAS_LINK_RATE_1_5_GBPS: u32 = 0x08;
/// 3.0 Gbps (SAS-1 / SATA II).
pub const SAS_LINK_RATE_3_0_GBPS: u32 = 0x09;
/// 6.0 Gbps (SAS-2 / SATA III).
pub const SAS_LINK_RATE_6_0_GBPS: u32 = 0x0A;
/// 12.0 Gbps (SAS-3).
pub const SAS_LINK_RATE_12_0_GBPS: u32 = 0x0B;
/// 22.5 Gbps (SAS-4).
pub const SAS_LINK_RATE_22_5_GBPS: u32 = 0x0C;

// ---------------------------------------------------------------------------
// iSCSI session states
// ---------------------------------------------------------------------------

/// Session is logged in and active.
pub const ISCSI_SESSION_LOGGED_IN: u32 = 0;
/// Session recovery in progress.
pub const ISCSI_SESSION_RECOVERY: u32 = 1;
/// Session is being freed.
pub const ISCSI_SESSION_FREE: u32 = 2;
/// Session login failed.
pub const ISCSI_SESSION_FAILED: u32 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transports_distinct() {
        let transports = [
            SCSI_TRANSPORT_FCP,
            SCSI_TRANSPORT_SPI,
            SCSI_TRANSPORT_SAS,
            SCSI_TRANSPORT_ISCSI,
            SCSI_TRANSPORT_SBP,
            SCSI_TRANSPORT_SRP,
            SCSI_TRANSPORT_USB,
            SCSI_TRANSPORT_ATA,
        ];
        for i in 0..transports.len() {
            for j in (i + 1)..transports.len() {
                assert_ne!(transports[i], transports[j]);
            }
        }
    }

    #[test]
    fn test_sas_link_rates_ordered() {
        assert!(SAS_LINK_RATE_1_5_GBPS < SAS_LINK_RATE_3_0_GBPS);
        assert!(SAS_LINK_RATE_3_0_GBPS < SAS_LINK_RATE_6_0_GBPS);
        assert!(SAS_LINK_RATE_6_0_GBPS < SAS_LINK_RATE_12_0_GBPS);
        assert!(SAS_LINK_RATE_12_0_GBPS < SAS_LINK_RATE_22_5_GBPS);
    }

    #[test]
    fn test_iscsi_states_distinct() {
        let states = [
            ISCSI_SESSION_LOGGED_IN,
            ISCSI_SESSION_RECOVERY,
            ISCSI_SESSION_FREE,
            ISCSI_SESSION_FAILED,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_transport_values() {
        assert_eq!(SCSI_TRANSPORT_FCP, 0);
        assert_eq!(SCSI_TRANSPORT_SAS, 2);
        assert_eq!(SCSI_TRANSPORT_ISCSI, 3);
    }
}
