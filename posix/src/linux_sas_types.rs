//! `<scsi/scsi_transport_sas.h>` — SAS (Serial Attached SCSI) constants.
//!
//! SAS transport constants covering device types,
//! protocol types, link rates, and phy events.

// ---------------------------------------------------------------------------
// SAS device types
// ---------------------------------------------------------------------------

/// No device.
pub const SAS_DEV_TYPE_NONE: u32 = 0;
/// End device (SAS or SATA).
pub const SAS_END_DEVICE: u32 = 1;
/// Edge expander.
pub const SAS_EDGE_EXPANDER_DEVICE: u32 = 2;
/// Fanout expander.
pub const SAS_FANOUT_EXPANDER_DEVICE: u32 = 3;

// ---------------------------------------------------------------------------
// SAS protocol types
// ---------------------------------------------------------------------------

/// SMP (Serial Management Protocol).
pub const SAS_PROTOCOL_SMP: u32 = 0x01;
/// SSP (Serial SCSI Protocol).
pub const SAS_PROTOCOL_SSP: u32 = 0x02;
/// STP (Serial ATA Tunneling Protocol).
pub const SAS_PROTOCOL_STP: u32 = 0x04;
/// SATA (native).
pub const SAS_PROTOCOL_SATA: u32 = 0x08;
/// All protocols.
pub const SAS_PROTOCOL_ALL: u32 = 0x0F;

// ---------------------------------------------------------------------------
// SAS link rates
// ---------------------------------------------------------------------------

/// Unknown rate.
pub const SAS_LINK_RATE_UNKNOWN: u32 = 0;
/// Disabled.
pub const SAS_PHY_DISABLED: u32 = 1;
/// Phy reset problem.
pub const SAS_PHY_RESET_PROBLEM: u32 = 2;
/// SATA spinup hold.
pub const SAS_SATA_SPINUP_HOLD: u32 = 3;
/// Port selector.
pub const SAS_SATA_PORT_SELECTOR: u32 = 4;
/// Phy reset in progress.
pub const SAS_PHY_RESET_IN_PROGRESS: u32 = 5;
/// 1.5 Gbps.
pub const SAS_LINK_RATE_1_5_GBPS: u32 = 8;
/// 3.0 Gbps.
pub const SAS_LINK_RATE_3_0_GBPS: u32 = 9;
/// 6.0 Gbps.
pub const SAS_LINK_RATE_6_0_GBPS: u32 = 10;
/// 12.0 Gbps.
pub const SAS_LINK_RATE_12_0_GBPS: u32 = 11;
/// 22.5 Gbps.
pub const SAS_LINK_RATE_22_5_GBPS: u32 = 12;

// ---------------------------------------------------------------------------
// SAS phy events
// ---------------------------------------------------------------------------

/// Invalid DWORD count.
pub const SAS_PHY_EVENT_INVALID_DWORD: u32 = 0;
/// Running disparity error.
pub const SAS_PHY_EVENT_RUN_DISP_ERR: u32 = 1;
/// Loss of DWORD sync.
pub const SAS_PHY_EVENT_LOSS_DWORD_SYNC: u32 = 2;
/// Phy reset problem (event).
pub const SAS_PHY_EVENT_PHY_RESET_PROBLEM: u32 = 3;

// ---------------------------------------------------------------------------
// SAS task management functions
// ---------------------------------------------------------------------------

/// Abort task.
pub const SAS_TMF_ABORT_TASK: u32 = 0x01;
/// Abort task set.
pub const SAS_TMF_ABORT_TASK_SET: u32 = 0x02;
/// Clear task set.
pub const SAS_TMF_CLEAR_TASK_SET: u32 = 0x04;
/// Logical unit reset.
pub const SAS_TMF_LU_RESET: u32 = 0x08;
/// IT nexus reset.
pub const SAS_TMF_IT_NEXUS_RESET: u32 = 0x10;
/// Clear ACA.
pub const SAS_TMF_CLEAR_ACA: u32 = 0x40;
/// Query task.
pub const SAS_TMF_QUERY_TASK: u32 = 0x80;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_types_distinct() {
        let types = [
            SAS_DEV_TYPE_NONE,
            SAS_END_DEVICE,
            SAS_EDGE_EXPANDER_DEVICE,
            SAS_FANOUT_EXPANDER_DEVICE,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_protocols_bitmask() {
        assert_eq!(
            SAS_PROTOCOL_ALL,
            SAS_PROTOCOL_SMP | SAS_PROTOCOL_SSP | SAS_PROTOCOL_STP | SAS_PROTOCOL_SATA
        );
    }

    #[test]
    fn test_link_rates_distinct() {
        let rates = [
            SAS_LINK_RATE_UNKNOWN,
            SAS_PHY_DISABLED,
            SAS_PHY_RESET_PROBLEM,
            SAS_SATA_SPINUP_HOLD,
            SAS_SATA_PORT_SELECTOR,
            SAS_PHY_RESET_IN_PROGRESS,
            SAS_LINK_RATE_1_5_GBPS,
            SAS_LINK_RATE_3_0_GBPS,
            SAS_LINK_RATE_6_0_GBPS,
            SAS_LINK_RATE_12_0_GBPS,
            SAS_LINK_RATE_22_5_GBPS,
        ];
        for i in 0..rates.len() {
            for j in (i + 1)..rates.len() {
                assert_ne!(rates[i], rates[j]);
            }
        }
    }

    #[test]
    fn test_phy_events_distinct() {
        let events = [
            SAS_PHY_EVENT_INVALID_DWORD,
            SAS_PHY_EVENT_RUN_DISP_ERR,
            SAS_PHY_EVENT_LOSS_DWORD_SYNC,
            SAS_PHY_EVENT_PHY_RESET_PROBLEM,
        ];
        for i in 0..events.len() {
            for j in (i + 1)..events.len() {
                assert_ne!(events[i], events[j]);
            }
        }
    }

    #[test]
    fn test_tmf_no_overlap() {
        let tmfs = [
            SAS_TMF_ABORT_TASK,
            SAS_TMF_ABORT_TASK_SET,
            SAS_TMF_CLEAR_TASK_SET,
            SAS_TMF_LU_RESET,
            SAS_TMF_IT_NEXUS_RESET,
            SAS_TMF_CLEAR_ACA,
            SAS_TMF_QUERY_TASK,
        ];
        for i in 0..tmfs.len() {
            for j in (i + 1)..tmfs.len() {
                assert_eq!(
                    tmfs[i] & tmfs[j],
                    0,
                    "0x{:02x} & 0x{:02x}",
                    tmfs[i],
                    tmfs[j]
                );
            }
        }
    }
}
