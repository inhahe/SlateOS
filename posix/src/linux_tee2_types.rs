//! `<linux/tee.h>` — Additional TEE constants.
//!
//! Supplementary Trusted Execution Environment constants covering
//! TEE origin values, login types, and parameter types.

// ---------------------------------------------------------------------------
// TEE origin values (TEEC_ORIGIN_*)
// ---------------------------------------------------------------------------

/// Origin: API.
pub const TEEC_ORIGIN_API: u32 = 0x00000001;
/// Origin: communication.
pub const TEEC_ORIGIN_COMMS: u32 = 0x00000002;
/// Origin: TEE.
pub const TEEC_ORIGIN_TEE: u32 = 0x00000003;
/// Origin: trusted application.
pub const TEEC_ORIGIN_TRUSTED_APP: u32 = 0x00000004;

// ---------------------------------------------------------------------------
// TEE login types (TEEC_LOGIN_*)
// ---------------------------------------------------------------------------

/// Public login.
pub const TEEC_LOGIN_PUBLIC: u32 = 0x00000000;
/// User login.
pub const TEEC_LOGIN_USER: u32 = 0x00000001;
/// Group login.
pub const TEEC_LOGIN_GROUP: u32 = 0x00000002;
/// Application login.
pub const TEEC_LOGIN_APPLICATION: u32 = 0x00000004;
/// User-application login.
pub const TEEC_LOGIN_USER_APPLICATION: u32 = 0x00000005;
/// Group-application login.
pub const TEEC_LOGIN_GROUP_APPLICATION: u32 = 0x00000006;

// ---------------------------------------------------------------------------
// TEE parameter types (TEEC_PARAM_*)
// ---------------------------------------------------------------------------

/// None (unused parameter).
pub const TEEC_NONE: u32 = 0x00000000;
/// Value input.
pub const TEEC_VALUE_INPUT: u32 = 0x00000001;
/// Value output.
pub const TEEC_VALUE_OUTPUT: u32 = 0x00000002;
/// Value in/out.
pub const TEEC_VALUE_INOUT: u32 = 0x00000003;
/// Temp memory input.
pub const TEEC_MEMREF_TEMP_INPUT: u32 = 0x00000005;
/// Temp memory output.
pub const TEEC_MEMREF_TEMP_OUTPUT: u32 = 0x00000006;
/// Temp memory in/out.
pub const TEEC_MEMREF_TEMP_INOUT: u32 = 0x00000007;
/// Whole memory reference.
pub const TEEC_MEMREF_WHOLE: u32 = 0x0000000C;
/// Partial memory input.
pub const TEEC_MEMREF_PARTIAL_INPUT: u32 = 0x0000000D;
/// Partial memory output.
pub const TEEC_MEMREF_PARTIAL_OUTPUT: u32 = 0x0000000E;
/// Partial memory in/out.
pub const TEEC_MEMREF_PARTIAL_INOUT: u32 = 0x0000000F;

// ---------------------------------------------------------------------------
// TEE shared memory flags
// ---------------------------------------------------------------------------

/// Input shared memory.
pub const TEEC_MEM_INPUT: u32 = 0x00000001;
/// Output shared memory.
pub const TEEC_MEM_OUTPUT: u32 = 0x00000002;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_origins_distinct() {
        let origins = [
            TEEC_ORIGIN_API,
            TEEC_ORIGIN_COMMS,
            TEEC_ORIGIN_TEE,
            TEEC_ORIGIN_TRUSTED_APP,
        ];
        for i in 0..origins.len() {
            for j in (i + 1)..origins.len() {
                assert_ne!(origins[i], origins[j]);
            }
        }
    }

    #[test]
    fn test_login_types_distinct() {
        let logins = [
            TEEC_LOGIN_PUBLIC,
            TEEC_LOGIN_USER,
            TEEC_LOGIN_GROUP,
            TEEC_LOGIN_APPLICATION,
            TEEC_LOGIN_USER_APPLICATION,
            TEEC_LOGIN_GROUP_APPLICATION,
        ];
        for i in 0..logins.len() {
            for j in (i + 1)..logins.len() {
                assert_ne!(logins[i], logins[j]);
            }
        }
    }

    #[test]
    fn test_param_types_distinct() {
        let params = [
            TEEC_NONE,
            TEEC_VALUE_INPUT,
            TEEC_VALUE_OUTPUT,
            TEEC_VALUE_INOUT,
            TEEC_MEMREF_TEMP_INPUT,
            TEEC_MEMREF_TEMP_OUTPUT,
            TEEC_MEMREF_TEMP_INOUT,
            TEEC_MEMREF_WHOLE,
            TEEC_MEMREF_PARTIAL_INPUT,
            TEEC_MEMREF_PARTIAL_OUTPUT,
            TEEC_MEMREF_PARTIAL_INOUT,
        ];
        for i in 0..params.len() {
            for j in (i + 1)..params.len() {
                assert_ne!(params[i], params[j]);
            }
        }
    }

    #[test]
    fn test_mem_flags_no_overlap() {
        assert_eq!(TEEC_MEM_INPUT & TEEC_MEM_OUTPUT, 0);
    }

    #[test]
    fn test_none_is_zero() {
        assert_eq!(TEEC_NONE, 0);
    }

    #[test]
    fn test_public_login_is_zero() {
        assert_eq!(TEEC_LOGIN_PUBLIC, 0);
    }
}
