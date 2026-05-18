//! `<linux/tpm.h>` — Additional TPM (Trusted Platform Module) constants.
//!
//! Supplementary TPM constants covering command codes,
//! algorithm IDs, capability types, and session types.

// ---------------------------------------------------------------------------
// TPM2 command codes (TPM2_CC_*)
// ---------------------------------------------------------------------------

/// Startup.
pub const TPM2_CC_STARTUP: u32 = 0x0144;
/// Shutdown.
pub const TPM2_CC_SHUTDOWN: u32 = 0x0145;
/// Self test.
pub const TPM2_CC_SELF_TEST: u32 = 0x0143;
/// PCR extend.
pub const TPM2_CC_PCR_EXTEND: u32 = 0x0182;
/// PCR read.
pub const TPM2_CC_PCR_READ: u32 = 0x017E;
/// Create primary.
pub const TPM2_CC_CREATE_PRIMARY: u32 = 0x0131;
/// Create.
pub const TPM2_CC_CREATE: u32 = 0x0153;
/// Load.
pub const TPM2_CC_LOAD: u32 = 0x0157;
/// Unseal.
pub const TPM2_CC_UNSEAL: u32 = 0x015E;
/// Get capability.
pub const TPM2_CC_GET_CAPABILITY: u32 = 0x017A;
/// Get random.
pub const TPM2_CC_GET_RANDOM: u32 = 0x017B;
/// Hash.
pub const TPM2_CC_HASH: u32 = 0x017D;
/// Flush context.
pub const TPM2_CC_FLUSH_CONTEXT: u32 = 0x0165;

// ---------------------------------------------------------------------------
// TPM2 algorithm IDs (TPM2_ALG_*)
// ---------------------------------------------------------------------------

/// Error/null.
pub const TPM2_ALG_ERROR: u16 = 0x0000;
/// RSA.
pub const TPM2_ALG_RSA: u16 = 0x0001;
/// SHA-1.
pub const TPM2_ALG_SHA1: u16 = 0x0004;
/// HMAC.
pub const TPM2_ALG_HMAC: u16 = 0x0005;
/// AES.
pub const TPM2_ALG_AES: u16 = 0x0006;
/// SHA-256.
pub const TPM2_ALG_SHA256: u16 = 0x000B;
/// SHA-384.
pub const TPM2_ALG_SHA384: u16 = 0x000C;
/// SHA-512.
pub const TPM2_ALG_SHA512: u16 = 0x000D;
/// Null algorithm.
pub const TPM2_ALG_NULL: u16 = 0x0010;
/// SM3-256.
pub const TPM2_ALG_SM3_256: u16 = 0x0012;
/// SM4.
pub const TPM2_ALG_SM4: u16 = 0x0013;
/// ECC.
pub const TPM2_ALG_ECC: u16 = 0x0023;
/// CFB mode.
pub const TPM2_ALG_CFB: u16 = 0x0043;

// ---------------------------------------------------------------------------
// TPM2 startup types
// ---------------------------------------------------------------------------

/// Clear startup.
pub const TPM2_SU_CLEAR: u16 = 0x0000;
/// State startup.
pub const TPM2_SU_STATE: u16 = 0x0001;

// ---------------------------------------------------------------------------
// TPM2 capability types (TPM2_CAP_*)
// ---------------------------------------------------------------------------

/// Algorithms.
pub const TPM2_CAP_ALGS: u32 = 0x00000000;
/// Handles.
pub const TPM2_CAP_HANDLES: u32 = 0x00000001;
/// Commands.
pub const TPM2_CAP_COMMANDS: u32 = 0x00000002;
/// PCRs.
pub const TPM2_CAP_PCRS: u32 = 0x00000005;
/// TPM properties.
pub const TPM2_CAP_TPM_PROPERTIES: u32 = 0x00000006;

// ---------------------------------------------------------------------------
// TPM2 session types
// ---------------------------------------------------------------------------

/// HMAC session.
pub const TPM2_SE_HMAC: u8 = 0x00;
/// Policy session.
pub const TPM2_SE_POLICY: u8 = 0x01;
/// Trial session.
pub const TPM2_SE_TRIAL: u8 = 0x03;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cc_distinct() {
        let cmds = [
            TPM2_CC_STARTUP, TPM2_CC_SHUTDOWN, TPM2_CC_SELF_TEST,
            TPM2_CC_PCR_EXTEND, TPM2_CC_PCR_READ,
            TPM2_CC_CREATE_PRIMARY, TPM2_CC_CREATE,
            TPM2_CC_LOAD, TPM2_CC_UNSEAL,
            TPM2_CC_GET_CAPABILITY, TPM2_CC_GET_RANDOM,
            TPM2_CC_HASH, TPM2_CC_FLUSH_CONTEXT,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_alg_distinct() {
        let algs: [u16; 13] = [
            TPM2_ALG_ERROR, TPM2_ALG_RSA, TPM2_ALG_SHA1,
            TPM2_ALG_HMAC, TPM2_ALG_AES, TPM2_ALG_SHA256,
            TPM2_ALG_SHA384, TPM2_ALG_SHA512, TPM2_ALG_NULL,
            TPM2_ALG_SM3_256, TPM2_ALG_SM4, TPM2_ALG_ECC,
            TPM2_ALG_CFB,
        ];
        for i in 0..algs.len() {
            for j in (i + 1)..algs.len() {
                assert_ne!(algs[i], algs[j]);
            }
        }
    }

    #[test]
    fn test_startup_types() {
        assert_eq!(TPM2_SU_CLEAR, 0);
        assert_eq!(TPM2_SU_STATE, 1);
    }

    #[test]
    fn test_cap_types_distinct() {
        let caps = [
            TPM2_CAP_ALGS, TPM2_CAP_HANDLES,
            TPM2_CAP_COMMANDS, TPM2_CAP_PCRS,
            TPM2_CAP_TPM_PROPERTIES,
        ];
        for i in 0..caps.len() {
            for j in (i + 1)..caps.len() {
                assert_ne!(caps[i], caps[j]);
            }
        }
    }

    #[test]
    fn test_session_types() {
        let sessions: [u8; 3] = [TPM2_SE_HMAC, TPM2_SE_POLICY, TPM2_SE_TRIAL];
        for i in 0..sessions.len() {
            for j in (i + 1)..sessions.len() {
                assert_ne!(sessions[i], sessions[j]);
            }
        }
    }
}
