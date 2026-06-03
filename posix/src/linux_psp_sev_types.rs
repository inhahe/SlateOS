//! `<linux/psp-sev.h>` — AMD PSP / SEV userspace ioctls.
//!
//! AMD's Platform Security Processor (PSP) exposes the SEV /
//! SEV-ES / SEV-SNP commands through `/dev/sev`. QEMU (sev-guest
//! launch flow), libvirt, snpguest, and the AMD SVSM bring-up
//! tools issue the constants below.

// ---------------------------------------------------------------------------
// ioctl group letter
// ---------------------------------------------------------------------------

/// Magic letter for `/dev/sev` ioctls ('S').
pub const SEV_IOC_TYPE: u8 = b'S';

// ---------------------------------------------------------------------------
// Platform-level commands (struct sev_issue_cmd.cmd)
// ---------------------------------------------------------------------------

/// Factory-reset the PSP.
pub const SEV_FACTORY_RESET: u32 = 0;
/// Query platform status.
pub const SEV_PLATFORM_STATUS: u32 = 1;
/// Begin PEK (platform endorsement key) certificate signing.
pub const SEV_PEK_GEN: u32 = 2;
/// Sign PEK with the platform key.
pub const SEV_PEK_CSR: u32 = 3;
/// Generate the platform-DH key.
pub const SEV_PDH_GEN: u32 = 4;
/// Certify PEK with the AMD root certificate.
pub const SEV_PDH_CERT_EXPORT: u32 = 5;
/// Import a PEK certificate.
pub const SEV_PEK_CERT_IMPORT: u32 = 6;
/// Query unique device id.
pub const SEV_GET_ID: u32 = 7;
/// Query unique device id (v2).
pub const SEV_GET_ID2: u32 = 8;
/// SEV-SNP platform status.
pub const SNP_PLATFORM_STATUS: u32 = 9;
/// SEV-SNP commit (lock down config).
pub const SNP_COMMIT: u32 = 10;
/// SEV-SNP set config.
pub const SNP_SET_CONFIG: u32 = 11;

// ---------------------------------------------------------------------------
// ioctl number (single SEV_ISSUE_CMD ioctl carrying a sub-command in struct)
// ---------------------------------------------------------------------------

/// `SEV_ISSUE_CMD` — generic command dispatch.
pub const SEV_ISSUE_CMD: u32 = 0xC010_5300;

// ---------------------------------------------------------------------------
// SEV firmware error codes (struct sev_issue_cmd.error)
// ---------------------------------------------------------------------------

/// Success.
pub const SEV_RET_SUCCESS: u32 = 0;
/// Invalid platform state.
pub const SEV_RET_INVALID_PLATFORM_STATE: u32 = 1;
/// Invalid guest state.
pub const SEV_RET_INVALID_GUEST: u32 = 2;
/// Invalid configuration.
pub const SEV_RET_INVALID_CONFIG: u32 = 3;
/// Buffer too small.
pub const SEV_RET_INVALID_LEN: u32 = 4;
/// Already owned by another caller.
pub const SEV_RET_ALREADY_OWNED: u32 = 5;
/// Invalid certificate.
pub const SEV_RET_INVALID_CERTIFICATE: u32 = 6;
/// Policy violation.
pub const SEV_RET_POLICY_FAILURE: u32 = 7;
/// Inactive.
pub const SEV_RET_INACTIVE: u32 = 8;
/// Invalid address.
pub const SEV_RET_INVALID_ADDRESS: u32 = 9;
/// Bad signature.
pub const SEV_RET_BAD_SIGNATURE: u32 = 10;
/// Bad measurement.
pub const SEV_RET_BAD_MEASUREMENT: u32 = 11;
/// Hardware error (platform).
pub const SEV_RET_HWSEV_RET_PLATFORM: u32 = 12;
/// Hardware error (unsafe).
pub const SEV_RET_HWSEV_RET_UNSAFE: u32 = 13;
/// Feature unsupported.
pub const SEV_RET_UNSUPPORTED: u32 = 14;

// ---------------------------------------------------------------------------
// SEV-SNP report sizes
// ---------------------------------------------------------------------------

/// Size of an attestation report in bytes.
pub const SEV_SNP_REPORT_SIZE: u32 = 1184;
/// Size of an extended report (with cert chain).
pub const SEV_SNP_REPORT_USER_DATA_LEN: u32 = 64;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctl_group_letter_s() {
        assert_eq!(SEV_IOC_TYPE, b'S');
        // SEV_ISSUE_CMD type byte must equal 'S' (0x53).
        assert_eq!((SEV_ISSUE_CMD >> 8) & 0xff, b'S' as u32);
    }

    #[test]
    fn test_platform_commands_dense() {
        let c = [
            SEV_FACTORY_RESET,
            SEV_PLATFORM_STATUS,
            SEV_PEK_GEN,
            SEV_PEK_CSR,
            SEV_PDH_GEN,
            SEV_PDH_CERT_EXPORT,
            SEV_PEK_CERT_IMPORT,
            SEV_GET_ID,
            SEV_GET_ID2,
            SNP_PLATFORM_STATUS,
            SNP_COMMIT,
            SNP_SET_CONFIG,
        ];
        for (i, &v) in c.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        // FACTORY_RESET must be 0 so zeroed cmd struct dispatches to
        // the obvious "reset" path rather than something destructive.
        assert_eq!(SEV_FACTORY_RESET, 0);
    }

    #[test]
    fn test_return_codes_dense_and_success_zero() {
        let r = [
            SEV_RET_SUCCESS,
            SEV_RET_INVALID_PLATFORM_STATE,
            SEV_RET_INVALID_GUEST,
            SEV_RET_INVALID_CONFIG,
            SEV_RET_INVALID_LEN,
            SEV_RET_ALREADY_OWNED,
            SEV_RET_INVALID_CERTIFICATE,
            SEV_RET_POLICY_FAILURE,
            SEV_RET_INACTIVE,
            SEV_RET_INVALID_ADDRESS,
            SEV_RET_BAD_SIGNATURE,
            SEV_RET_BAD_MEASUREMENT,
            SEV_RET_HWSEV_RET_PLATFORM,
            SEV_RET_HWSEV_RET_UNSAFE,
            SEV_RET_UNSUPPORTED,
        ];
        for (i, &v) in r.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        assert_eq!(SEV_RET_SUCCESS, 0);
    }

    #[test]
    fn test_snp_report_sizes() {
        // SNP attestation report is exactly 1184 bytes per AMD spec.
        assert_eq!(SEV_SNP_REPORT_SIZE, 1184);
        // User-data field is 64 bytes (SHA-512 of a nonce, typically).
        assert_eq!(SEV_SNP_REPORT_USER_DATA_LEN, 64);
    }
}
