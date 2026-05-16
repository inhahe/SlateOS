//! `<linux/tee.h>` — Trusted Execution Environment constants.
//!
//! TEE provides a secure execution environment (ARM TrustZone,
//! Intel SGX, etc.) for running trusted applications. The Linux
//! TEE subsystem exposes /dev/tee0 and /dev/teepriv0 for
//! communication with OP-TEE and other TEE implementations.

// ---------------------------------------------------------------------------
// TEE ioctl commands
// ---------------------------------------------------------------------------

/// Get TEE version.
pub const TEE_IOC_VERSION: u32 = 0x8008_A400;
/// Open session.
pub const TEE_IOC_OPEN_SESSION: u32 = 0xC010_A402;
/// Invoke command.
pub const TEE_IOC_INVOKE: u32 = 0xC010_A403;
/// Cancel command.
pub const TEE_IOC_CANCEL: u32 = 0xC010_A404;
/// Close session.
pub const TEE_IOC_CLOSE_SESSION: u32 = 0x4004_A405;
/// Register shared memory.
pub const TEE_IOC_SHM_ALLOC: u32 = 0xC010_A406;
/// Register shared memory FD.
pub const TEE_IOC_SHM_REGISTER_FD: u32 = 0xC018_A408;
/// Supplicant receive.
pub const TEE_IOC_SUPPL_RECV: u32 = 0xC018_A406;
/// Supplicant send.
pub const TEE_IOC_SUPPL_SEND: u32 = 0xC018_A407;

// ---------------------------------------------------------------------------
// TEE implementation IDs
// ---------------------------------------------------------------------------

/// OP-TEE.
pub const TEE_IMPL_ID_OPTEE: u32 = 1;
/// AMD-TEE.
pub const TEE_IMPL_ID_AMDTEE: u32 = 2;
/// TEE implementation start of vendor range.
pub const TEE_IMPL_ID_VENDOR_START: u32 = 0x8000_0000;

// ---------------------------------------------------------------------------
// TEE origin codes (where error occurred)
// ---------------------------------------------------------------------------

/// API origin.
pub const TEE_ORIGIN_API: u32 = 0x00000001;
/// Communication origin.
pub const TEE_ORIGIN_COMMS: u32 = 0x00000002;
/// TEE origin.
pub const TEE_ORIGIN_TEE: u32 = 0x00000003;
/// Trusted application origin.
pub const TEE_ORIGIN_TRUSTED_APP: u32 = 0x00000004;

// ---------------------------------------------------------------------------
// Parameter types
// ---------------------------------------------------------------------------

/// No parameter.
pub const TEE_PARAM_TYPE_NONE: u32 = 0;
/// Input value parameter.
pub const TEE_PARAM_TYPE_VALUE_INPUT: u32 = 1;
/// Output value parameter.
pub const TEE_PARAM_TYPE_VALUE_OUTPUT: u32 = 2;
/// Input/output value parameter.
pub const TEE_PARAM_TYPE_VALUE_INOUT: u32 = 3;
/// Input memory reference.
pub const TEE_PARAM_TYPE_MEMREF_INPUT: u32 = 5;
/// Output memory reference.
pub const TEE_PARAM_TYPE_MEMREF_OUTPUT: u32 = 6;
/// Input/output memory reference.
pub const TEE_PARAM_TYPE_MEMREF_INOUT: u32 = 7;

// ---------------------------------------------------------------------------
// Login types
// ---------------------------------------------------------------------------

/// Public login.
pub const TEE_LOGIN_PUBLIC: u32 = 0x00000000;
/// User login.
pub const TEE_LOGIN_USER: u32 = 0x00000001;
/// Group login.
pub const TEE_LOGIN_GROUP: u32 = 0x00000002;
/// Application login.
pub const TEE_LOGIN_APPLICATION: u32 = 0x00000004;
/// Application user login.
pub const TEE_LOGIN_APPLICATION_USER: u32 = 0x00000005;
/// Application group login.
pub const TEE_LOGIN_APPLICATION_GROUP: u32 = 0x00000006;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctl_cmds_distinct() {
        let cmds = [
            TEE_IOC_VERSION, TEE_IOC_OPEN_SESSION,
            TEE_IOC_INVOKE, TEE_IOC_CANCEL,
            TEE_IOC_CLOSE_SESSION, TEE_IOC_SHM_ALLOC,
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
        assert!(TEE_IMPL_ID_VENDOR_START > TEE_IMPL_ID_AMDTEE);
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
    fn test_param_types_distinct() {
        let types = [
            TEE_PARAM_TYPE_NONE, TEE_PARAM_TYPE_VALUE_INPUT,
            TEE_PARAM_TYPE_VALUE_OUTPUT, TEE_PARAM_TYPE_VALUE_INOUT,
            TEE_PARAM_TYPE_MEMREF_INPUT, TEE_PARAM_TYPE_MEMREF_OUTPUT,
            TEE_PARAM_TYPE_MEMREF_INOUT,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_login_types_distinct() {
        let logins = [
            TEE_LOGIN_PUBLIC, TEE_LOGIN_USER, TEE_LOGIN_GROUP,
            TEE_LOGIN_APPLICATION, TEE_LOGIN_APPLICATION_USER,
            TEE_LOGIN_APPLICATION_GROUP,
        ];
        for i in 0..logins.len() {
            for j in (i + 1)..logins.len() {
                assert_ne!(logins[i], logins[j]);
            }
        }
    }
}
