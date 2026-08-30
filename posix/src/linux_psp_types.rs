//! `<uapi/linux/psp-sev.h>` — AMD PSP/SEV ioctl constants.
//!
//! Constants for AMD's Platform Security Processor exposed via
//! `/dev/sev`. Used by SEV (Secure Encrypted Virtualization) host
//! tools (`sevtool`, libvirt) to provision platform keys and launch
//! encrypted VMs.

// ---------------------------------------------------------------------------
// PSP/SEV ioctl command codes (cmd argument to SEV_ISSUE_CMD)
// ---------------------------------------------------------------------------

/// Reset platform state.
pub const SEV_FACTORY_RESET: u32 = 0;
/// Query platform status.
pub const SEV_PLATFORM_STATUS: u32 = 1;
/// Generate PEK (Platform Endorsement Key).
pub const SEV_PEK_GEN: u32 = 2;
/// Sign PEK certificate.
pub const SEV_PEK_CSR: u32 = 3;
/// Generate PDH (Platform Diffie-Hellman) key.
pub const SEV_PDH_GEN: u32 = 4;
/// Export PDH certificate chain.
pub const SEV_PDH_CERT_EXPORT: u32 = 5;
/// Import a signed PEK certificate.
pub const SEV_PEK_CERT_IMPORT: u32 = 6;
/// Retrieve a unique chip identifier.
pub const SEV_GET_ID: u32 = 7;
/// SNP-specific platform status.
pub const SEV_SNP_PLATFORM_STATUS: u32 = 9;
/// Commit pending firmware version.
pub const SEV_SNP_COMMIT: u32 = 10;
/// Set SNP configuration.
pub const SEV_SNP_SET_CONFIG: u32 = 11;

// ---------------------------------------------------------------------------
// SEV firmware error codes (returned in ioctl arg)
// ---------------------------------------------------------------------------

/// Successful completion.
pub const SEV_RET_SUCCESS: u32 = 0;
/// Invalid platform state for command.
pub const SEV_RET_INVALID_PLATFORM_STATE: u32 = 1;
/// Invalid guest state.
pub const SEV_RET_INVALID_GUEST_STATE: u32 = 2;
/// Configuration block is invalid.
pub const SEV_RET_INAVLID_CONFIG: u32 = 3;
/// Length-field invalid.
pub const SEV_RET_INVALID_LEN: u32 = 4;
/// Already owned.
pub const SEV_RET_ALREADY_OWNED: u32 = 5;
/// Certificate is invalid.
pub const SEV_RET_INVALID_CERTIFICATE: u32 = 6;
/// Policy violation.
pub const SEV_RET_POLICY_FAILURE: u32 = 7;
/// Inactive guest.
pub const SEV_RET_INACTIVE: u32 = 8;
/// Invalid address.
pub const SEV_RET_INVALID_ADDRESS: u32 = 9;
/// Bad signature.
pub const SEV_RET_BAD_SIGNATURE: u32 = 10;
/// Bad measurement.
pub const SEV_RET_BAD_MEASUREMENT: u32 = 11;
/// ASID already in use.
pub const SEV_RET_ASID_OWNED: u32 = 12;
/// Hardware platform error.
pub const SEV_RET_HWSEV_RET_PLATFORM: u32 = 13;
/// Hardware unsafe.
pub const SEV_RET_HWSEV_RET_UNSAFE: u32 = 14;
/// Feature unsupported.
pub const SEV_RET_UNSUPPORTED: u32 = 15;
/// Invalid parameter.
pub const SEV_RET_INVALID_PARAM: u32 = 16;
/// Resource limit reached.
pub const SEV_RET_RESOURCE_LIMIT: u32 = 17;
/// Secure data integrity check failed.
pub const SEV_RET_SECURE_DATA_INVALID: u32 = 18;

// ---------------------------------------------------------------------------
// Platform-state codes (sev_user_data_status.state)
// ---------------------------------------------------------------------------

/// Uninitialised.
pub const SEV_STATE_UNINIT: u32 = 0;
/// Initialised, no owner.
pub const SEV_STATE_INIT: u32 = 1;
/// Working (owned and provisioned).
pub const SEV_STATE_WORKING: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cmds_distinct() {
        let cmds = [
            SEV_FACTORY_RESET,
            SEV_PLATFORM_STATUS,
            SEV_PEK_GEN,
            SEV_PEK_CSR,
            SEV_PDH_GEN,
            SEV_PDH_CERT_EXPORT,
            SEV_PEK_CERT_IMPORT,
            SEV_GET_ID,
            SEV_SNP_PLATFORM_STATUS,
            SEV_SNP_COMMIT,
            SEV_SNP_SET_CONFIG,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_success_is_zero() {
        // POSIX-style — caller checks for non-zero to detect error.
        assert_eq!(SEV_RET_SUCCESS, 0);
    }

    #[test]
    fn test_ret_codes_distinct() {
        let codes = [
            SEV_RET_SUCCESS,
            SEV_RET_INVALID_PLATFORM_STATE,
            SEV_RET_INVALID_GUEST_STATE,
            SEV_RET_INAVLID_CONFIG,
            SEV_RET_INVALID_LEN,
            SEV_RET_ALREADY_OWNED,
            SEV_RET_INVALID_CERTIFICATE,
            SEV_RET_POLICY_FAILURE,
            SEV_RET_INACTIVE,
            SEV_RET_INVALID_ADDRESS,
            SEV_RET_BAD_SIGNATURE,
            SEV_RET_BAD_MEASUREMENT,
            SEV_RET_ASID_OWNED,
            SEV_RET_HWSEV_RET_PLATFORM,
            SEV_RET_HWSEV_RET_UNSAFE,
            SEV_RET_UNSUPPORTED,
            SEV_RET_INVALID_PARAM,
            SEV_RET_RESOURCE_LIMIT,
            SEV_RET_SECURE_DATA_INVALID,
        ];
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                assert_ne!(codes[i], codes[j]);
            }
        }
    }

    #[test]
    fn test_states_ordered() {
        // State machine progresses UNINIT -> INIT -> WORKING; the
        // ordering matters for state-transition validation.
        assert!(SEV_STATE_UNINIT < SEV_STATE_INIT);
        assert!(SEV_STATE_INIT < SEV_STATE_WORKING);
    }
}
