//! `<linux/ccix.h>` — Cache Coherent Interconnect for Accelerators (CCIX) constants.
//!
//! CCIX extends PCIe with cache coherency for accelerators (GPUs,
//! FPGAs, SmartNICs). Unlike standard PCIe where the CPU manages
//! coherency, CCIX allows accelerators to participate in the cache
//! coherence protocol, enabling shared virtual memory between CPU
//! and accelerator without explicit data movement. CCIX uses PCIe
//! PHY with an additional coherency protocol layer.

// ---------------------------------------------------------------------------
// CCIX port types
// ---------------------------------------------------------------------------

/// Requesting Agent (RA) — initiates coherent requests.
pub const CCIX_PORT_RA: u32 = 0;
/// Home Agent (HA) — manages coherency for a memory range.
pub const CCIX_PORT_HA: u32 = 1;
/// Slave Agent (SA) — responds to coherent requests (memory/device).
pub const CCIX_PORT_SA: u32 = 2;

// ---------------------------------------------------------------------------
// CCIX coherency states (MOESI-like)
// ---------------------------------------------------------------------------

/// Invalid (not in cache).
pub const CCIX_STATE_INVALID: u32 = 0;
/// Shared Clean (in cache, unmodified, shared).
pub const CCIX_STATE_SHARED_CLEAN: u32 = 1;
/// Shared Dirty (in cache, modified, shared with owner).
pub const CCIX_STATE_SHARED_DIRTY: u32 = 2;
/// Unique Clean (in cache, unmodified, exclusive).
pub const CCIX_STATE_UNIQUE_CLEAN: u32 = 3;
/// Unique Dirty (in cache, modified, exclusive).
pub const CCIX_STATE_UNIQUE_DIRTY: u32 = 4;

// ---------------------------------------------------------------------------
// CCIX message types
// ---------------------------------------------------------------------------

/// Read request (coherent read).
pub const CCIX_MSG_READ: u32 = 0;
/// Read response with data.
pub const CCIX_MSG_DATA: u32 = 1;
/// Snoop request (check other caches).
pub const CCIX_MSG_SNOOP: u32 = 2;
/// Snoop response.
pub const CCIX_MSG_SNOOP_RESP: u32 = 3;
/// Writeback (dirty data to home).
pub const CCIX_MSG_WRITEBACK: u32 = 4;
/// Evict (clean eviction notification).
pub const CCIX_MSG_EVICT: u32 = 5;

// ---------------------------------------------------------------------------
// CCIX link speeds
// ---------------------------------------------------------------------------

/// CCIX over PCIe Gen4 (16 GT/s per lane).
pub const CCIX_SPEED_GEN4: u32 = 16;
/// CCIX over PCIe Gen5 (32 GT/s per lane).
pub const CCIX_SPEED_GEN5: u32 = 32;
/// CCIX over PCIe Gen6 (64 GT/s per lane).
pub const CCIX_SPEED_GEN6: u32 = 64;

// ---------------------------------------------------------------------------
// CCIX error types
// ---------------------------------------------------------------------------

/// Link error (physical layer).
pub const CCIX_ERROR_LINK: u32 = 0;
/// Protocol error (coherency protocol violation).
pub const CCIX_ERROR_PROTOCOL: u32 = 1;
/// Poisoned data (data marked as corrupt).
pub const CCIX_ERROR_POISON: u32 = 2;
/// Address error (invalid address in coherent request).
pub const CCIX_ERROR_ADDRESS: u32 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_port_types_distinct() {
        let ports = [CCIX_PORT_RA, CCIX_PORT_HA, CCIX_PORT_SA];
        for i in 0..ports.len() {
            for j in (i + 1)..ports.len() {
                assert_ne!(ports[i], ports[j]);
            }
        }
    }

    #[test]
    fn test_coherency_states_distinct() {
        let states = [
            CCIX_STATE_INVALID,
            CCIX_STATE_SHARED_CLEAN,
            CCIX_STATE_SHARED_DIRTY,
            CCIX_STATE_UNIQUE_CLEAN,
            CCIX_STATE_UNIQUE_DIRTY,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_message_types_distinct() {
        let msgs = [
            CCIX_MSG_READ,
            CCIX_MSG_DATA,
            CCIX_MSG_SNOOP,
            CCIX_MSG_SNOOP_RESP,
            CCIX_MSG_WRITEBACK,
            CCIX_MSG_EVICT,
        ];
        for i in 0..msgs.len() {
            for j in (i + 1)..msgs.len() {
                assert_ne!(msgs[i], msgs[j]);
            }
        }
    }

    #[test]
    fn test_link_speeds_ordered() {
        assert!(CCIX_SPEED_GEN4 < CCIX_SPEED_GEN5);
        assert!(CCIX_SPEED_GEN5 < CCIX_SPEED_GEN6);
    }

    #[test]
    fn test_error_types_distinct() {
        let errors = [
            CCIX_ERROR_LINK,
            CCIX_ERROR_PROTOCOL,
            CCIX_ERROR_POISON,
            CCIX_ERROR_ADDRESS,
        ];
        for i in 0..errors.len() {
            for j in (i + 1)..errors.len() {
                assert_ne!(errors[i], errors[j]);
            }
        }
    }
}
