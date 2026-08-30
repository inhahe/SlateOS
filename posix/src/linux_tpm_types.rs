//! `<linux/tpm.h>` — Trusted Platform Module constants.
//!
//! TPM provides hardware-based security functions: secure key storage,
//! platform integrity measurement (PCR extend), attestation, and
//! random number generation. TPM 2.0 is the current standard;
//! TPM 1.2 is legacy.

// ---------------------------------------------------------------------------
// TPM versions
// ---------------------------------------------------------------------------

/// TPM 1.2.
pub const TPM_VERSION_1_2: u8 = 1;
/// TPM 2.0.
pub const TPM_VERSION_2_0: u8 = 2;

// ---------------------------------------------------------------------------
// TPM 2.0 command codes (TPM_CC)
// ---------------------------------------------------------------------------

/// Startup command.
pub const TPM2_CC_STARTUP: u32 = 0x0144;
/// Shutdown command.
pub const TPM2_CC_SHUTDOWN: u32 = 0x0145;
/// Self test.
pub const TPM2_CC_SELF_TEST: u32 = 0x0143;
/// PCR extend.
pub const TPM2_CC_PCR_EXTEND: u32 = 0x0182;
/// PCR read.
pub const TPM2_CC_PCR_READ: u32 = 0x017E;
/// Create primary key.
pub const TPM2_CC_CREATE_PRIMARY: u32 = 0x0131;
/// Create key.
pub const TPM2_CC_CREATE: u32 = 0x0153;
/// Load key.
pub const TPM2_CC_LOAD: u32 = 0x0157;
/// Unseal data.
pub const TPM2_CC_UNSEAL: u32 = 0x015E;
/// Get random bytes.
pub const TPM2_CC_GET_RANDOM: u32 = 0x017B;
/// Get capability.
pub const TPM2_CC_GET_CAPABILITY: u32 = 0x017A;
/// Flush context.
pub const TPM2_CC_FLUSH_CONTEXT: u32 = 0x0165;

// ---------------------------------------------------------------------------
// TPM 2.0 algorithms (TPM_ALG)
// ---------------------------------------------------------------------------

/// SHA-1.
pub const TPM2_ALG_SHA1: u16 = 0x0004;
/// SHA-256.
pub const TPM2_ALG_SHA256: u16 = 0x000B;
/// SHA-384.
pub const TPM2_ALG_SHA384: u16 = 0x000C;
/// SHA-512.
pub const TPM2_ALG_SHA512: u16 = 0x000D;
/// SM3-256 (Chinese standard).
pub const TPM2_ALG_SM3_256: u16 = 0x0012;
/// RSA.
pub const TPM2_ALG_RSA: u16 = 0x0001;
/// ECC.
pub const TPM2_ALG_ECC: u16 = 0x0023;
/// AES.
pub const TPM2_ALG_AES: u16 = 0x0006;
/// NULL algorithm.
pub const TPM2_ALG_NULL: u16 = 0x0010;

// ---------------------------------------------------------------------------
// PCR banks
// ---------------------------------------------------------------------------

/// Number of PCR registers.
pub const TPM2_PCR_COUNT: u8 = 24;
/// PCR 0: BIOS/firmware code.
pub const TPM2_PCR_BIOS: u8 = 0;
/// PCR 7: Secure Boot policy.
pub const TPM2_PCR_SECURE_BOOT: u8 = 7;
/// PCR 8-15: OS-defined.
pub const TPM2_PCR_OS_START: u8 = 8;

// ---------------------------------------------------------------------------
// TPM startup types
// ---------------------------------------------------------------------------

/// Clear startup.
pub const TPM2_SU_CLEAR: u16 = 0x0000;
/// State startup (resume from saved state).
pub const TPM2_SU_STATE: u16 = 0x0001;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_versions_distinct() {
        assert_ne!(TPM_VERSION_1_2, TPM_VERSION_2_0);
    }

    #[test]
    fn test_command_codes_distinct() {
        let cmds = [
            TPM2_CC_STARTUP,
            TPM2_CC_SHUTDOWN,
            TPM2_CC_SELF_TEST,
            TPM2_CC_PCR_EXTEND,
            TPM2_CC_PCR_READ,
            TPM2_CC_CREATE_PRIMARY,
            TPM2_CC_CREATE,
            TPM2_CC_LOAD,
            TPM2_CC_UNSEAL,
            TPM2_CC_GET_RANDOM,
            TPM2_CC_GET_CAPABILITY,
            TPM2_CC_FLUSH_CONTEXT,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_algorithms_distinct() {
        let algs = [
            TPM2_ALG_SHA1,
            TPM2_ALG_SHA256,
            TPM2_ALG_SHA384,
            TPM2_ALG_SHA512,
            TPM2_ALG_SM3_256,
            TPM2_ALG_RSA,
            TPM2_ALG_ECC,
            TPM2_ALG_AES,
            TPM2_ALG_NULL,
        ];
        for i in 0..algs.len() {
            for j in (i + 1)..algs.len() {
                assert_ne!(algs[i], algs[j]);
            }
        }
    }

    #[test]
    fn test_startup_types_distinct() {
        assert_ne!(TPM2_SU_CLEAR, TPM2_SU_STATE);
    }

    #[test]
    fn test_pcr_count() {
        assert_eq!(TPM2_PCR_COUNT, 24);
    }
}
