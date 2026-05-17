//! `<linux/tee.h>` — Trusted Execution Environment (TEE) constants.
//!
//! The TEE subsystem provides a generic interface to hardware-backed
//! trusted execution environments (ARM TrustZone OP-TEE, Intel SGX
//! via intermediaries, AMD SEV, etc.). Applications communicate with
//! Trusted Applications (TAs) running in the secure world through
//! open/invoke/close sessions.

// ---------------------------------------------------------------------------
// TEE ioctl commands
// ---------------------------------------------------------------------------

/// Get TEE implementation version info.
pub const TEE_IOC_VERSION: u32 = 0x8008_A400;
/// Open a session to a Trusted Application.
pub const TEE_IOC_OPEN_SESSION: u32 = 0x8008_A402;
/// Invoke a command within a session.
pub const TEE_IOC_INVOKE: u32 = 0x8008_A403;
/// Cancel an in-progress operation.
pub const TEE_IOC_CANCEL: u32 = 0x8008_A404;
/// Close a session.
pub const TEE_IOC_CLOSE_SESSION: u32 = 0x8008_A405;
/// Allocate shared memory.
pub const TEE_IOC_SHM_ALLOC: u32 = 0x8008_A406;
/// Register existing shared memory.
pub const TEE_IOC_SHM_REGISTER: u32 = 0x8008_A407;
/// Notify supplicant (REE ↔ TEE communication).
pub const TEE_IOC_SUPPL_RECV: u32 = 0x8008_A408;
/// Send supplicant response.
pub const TEE_IOC_SUPPL_SEND: u32 = 0x8008_A409;

// ---------------------------------------------------------------------------
// TEE implementation IDs
// ---------------------------------------------------------------------------

/// OP-TEE (ARM TrustZone).
pub const TEE_IMPL_ID_OPTEE: u32 = 1;
/// AMD-TEE (AMD PSP).
pub const TEE_IMPL_ID_AMDTEE: u32 = 2;

// ---------------------------------------------------------------------------
// TEE origin codes (where an error originated)
// ---------------------------------------------------------------------------

/// Error from the TEE API layer.
pub const TEE_ORIGIN_API: u32 = 1;
/// Error from the TEE communication layer.
pub const TEE_ORIGIN_COMMS: u32 = 2;
/// Error from the Trusted Application.
pub const TEE_ORIGIN_TEE: u32 = 3;
/// Error from the Trusted OS.
pub const TEE_ORIGIN_TRUSTED_APP: u32 = 4;

// ---------------------------------------------------------------------------
// Shared memory flags
// ---------------------------------------------------------------------------

/// Shared memory is input (REE → TEE).
pub const TEE_SHM_INPUT: u32 = 0x0001;
/// Shared memory is output (TEE → REE).
pub const TEE_SHM_OUTPUT: u32 = 0x0002;

// ---------------------------------------------------------------------------
// Parameter types
// ---------------------------------------------------------------------------

/// No parameter (empty slot).
pub const TEE_PARAM_TYPE_NONE: u32 = 0;
/// Value input parameter.
pub const TEE_PARAM_TYPE_VALUE_INPUT: u32 = 1;
/// Value output parameter.
pub const TEE_PARAM_TYPE_VALUE_OUTPUT: u32 = 2;
/// Value input/output parameter.
pub const TEE_PARAM_TYPE_VALUE_INOUT: u32 = 3;
/// Memory reference input.
pub const TEE_PARAM_TYPE_MEMREF_INPUT: u32 = 5;
/// Memory reference output.
pub const TEE_PARAM_TYPE_MEMREF_OUTPUT: u32 = 6;
/// Memory reference input/output.
pub const TEE_PARAM_TYPE_MEMREF_INOUT: u32 = 7;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctl_commands_distinct() {
        let cmds = [
            TEE_IOC_VERSION, TEE_IOC_OPEN_SESSION,
            TEE_IOC_INVOKE, TEE_IOC_CANCEL,
            TEE_IOC_CLOSE_SESSION, TEE_IOC_SHM_ALLOC,
            TEE_IOC_SHM_REGISTER, TEE_IOC_SUPPL_RECV,
            TEE_IOC_SUPPL_SEND,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_impl_ids_distinct() {
        assert_ne!(TEE_IMPL_ID_OPTEE, TEE_IMPL_ID_AMDTEE);
    }

    #[test]
    fn test_origins_distinct() {
        let origins = [
            TEE_ORIGIN_API, TEE_ORIGIN_COMMS,
            TEE_ORIGIN_TEE, TEE_ORIGIN_TRUSTED_APP,
        ];
        for i in 0..origins.len() {
            for j in (i + 1)..origins.len() {
                assert_ne!(origins[i], origins[j]);
            }
        }
    }

    #[test]
    fn test_shm_flags_no_overlap() {
        assert_eq!(TEE_SHM_INPUT & TEE_SHM_OUTPUT, 0);
        assert!(TEE_SHM_INPUT.is_power_of_two());
        assert!(TEE_SHM_OUTPUT.is_power_of_two());
    }

    #[test]
    fn test_param_types_distinct() {
        let params = [
            TEE_PARAM_TYPE_NONE, TEE_PARAM_TYPE_VALUE_INPUT,
            TEE_PARAM_TYPE_VALUE_OUTPUT, TEE_PARAM_TYPE_VALUE_INOUT,
            TEE_PARAM_TYPE_MEMREF_INPUT, TEE_PARAM_TYPE_MEMREF_OUTPUT,
            TEE_PARAM_TYPE_MEMREF_INOUT,
        ];
        for i in 0..params.len() {
            for j in (i + 1)..params.len() {
                assert_ne!(params[i], params[j]);
            }
        }
    }
}
