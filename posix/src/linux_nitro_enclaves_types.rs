//! `<linux/nitro_enclaves.h>` — AWS Nitro Enclaves constants.
//!
//! Nitro Enclaves are isolated compute environments on AWS EC2
//! instances. An enclave runs in its own VM carved from the parent
//! instance's resources (CPUs and memory), with no network access,
//! no persistent storage, and no interactive login. Communication
//! with the parent happens only through a vsock channel. Used for
//! processing sensitive data (cryptographic operations, PII) with
//! hardware-enforced isolation and attestation.

// ---------------------------------------------------------------------------
// Enclave IOCTL commands (on /dev/nitro_enclaves)
// ---------------------------------------------------------------------------

/// Create a new enclave VM.
pub const NE_CREATE_VM: u32 = 0x20;
/// Add vCPU to the enclave.
pub const NE_ADD_VCPU: u32 = 0x21;
/// Get enclave image load info (memory regions).
pub const NE_GET_IMAGE_LOAD_INFO: u32 = 0x22;
/// Set user memory region for enclave.
pub const NE_SET_USER_MEMORY_REGION: u32 = 0x23;
/// Start the enclave.
pub const NE_START_ENCLAVE: u32 = 0x24;

// ---------------------------------------------------------------------------
// Enclave flags
// ---------------------------------------------------------------------------

/// Run enclave in debug mode (console access, attestation doc has debug flag).
pub const NE_ENCLAVE_DEBUG_MODE: u32 = 1 << 0;
/// Production enclave (no debug access, clean attestation).
pub const NE_ENCLAVE_PRODUCTION_MODE: u32 = 0;

// ---------------------------------------------------------------------------
// Image load flags
// ---------------------------------------------------------------------------

/// Load EIF (Enclave Image Format) file.
pub const NE_EIF_IMAGE: u32 = 0;

// ---------------------------------------------------------------------------
// Enclave start flags
// ---------------------------------------------------------------------------

/// Wait for enclave to reach ready state after start.
pub const NE_START_WAIT_READY: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// Memory region flags
// ---------------------------------------------------------------------------

/// Memory region is read-only for enclave.
pub const NE_MEM_REGION_RO: u32 = 0;
/// Memory region is read-write for enclave.
pub const NE_MEM_REGION_RW: u32 = 1;

// ---------------------------------------------------------------------------
// Enclave state
// ---------------------------------------------------------------------------

/// Enclave not yet started.
pub const NE_STATE_INIT: u32 = 0;
/// Enclave is running.
pub const NE_STATE_RUNNING: u32 = 1;
/// Enclave is stopped/terminated.
pub const NE_STATE_STOPPED: u32 = 2;

// ---------------------------------------------------------------------------
// vsock port ranges for enclave communication
// ---------------------------------------------------------------------------

/// CID for the parent instance (from enclave's perspective).
pub const NE_PARENT_CID: u32 = 3;
/// Minimum user-assignable vsock port for enclave.
pub const NE_VSOCK_PORT_MIN: u32 = 8000;
/// Maximum user-assignable vsock port for enclave.
pub const NE_VSOCK_PORT_MAX: u32 = 65535;

// ---------------------------------------------------------------------------
// Attestation document constants
// ---------------------------------------------------------------------------

/// Maximum attestation document size (16 KiB).
pub const NE_ATTESTATION_MAX_SIZE: u32 = 16384;
/// NSM (Nitro Security Module) device path minor.
pub const NE_NSM_MINOR: u32 = 147;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctl_commands_distinct() {
        let ioctls = [
            NE_CREATE_VM,
            NE_ADD_VCPU,
            NE_GET_IMAGE_LOAD_INFO,
            NE_SET_USER_MEMORY_REGION,
            NE_START_ENCLAVE,
        ];
        for i in 0..ioctls.len() {
            for j in (i + 1)..ioctls.len() {
                assert_ne!(ioctls[i], ioctls[j]);
            }
        }
    }

    #[test]
    fn test_enclave_modes_distinct() {
        assert_ne!(NE_ENCLAVE_DEBUG_MODE, NE_ENCLAVE_PRODUCTION_MODE);
    }

    #[test]
    fn test_mem_region_flags_distinct() {
        assert_ne!(NE_MEM_REGION_RO, NE_MEM_REGION_RW);
    }

    #[test]
    fn test_states_distinct() {
        let states = [NE_STATE_INIT, NE_STATE_RUNNING, NE_STATE_STOPPED];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_vsock_port_range() {
        assert!(NE_VSOCK_PORT_MIN < NE_VSOCK_PORT_MAX);
        assert!(NE_VSOCK_PORT_MIN > 0);
    }

    #[test]
    fn test_attestation_max_size() {
        assert_eq!(NE_ATTESTATION_MAX_SIZE, 16384);
        assert!(NE_ATTESTATION_MAX_SIZE.is_power_of_two());
    }

    #[test]
    fn test_parent_cid() {
        // CID 3 is the well-known parent CID
        assert_eq!(NE_PARENT_CID, 3);
    }

    #[test]
    fn test_debug_mode_is_flag() {
        assert!(NE_ENCLAVE_DEBUG_MODE.is_power_of_two());
    }
}
