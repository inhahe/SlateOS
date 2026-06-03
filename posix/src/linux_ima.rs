//! `<linux/ima.h>` — Integrity Measurement Architecture constants.
//!
//! IMA measures (hashes) files before they are accessed, storing
//! measurements in a runtime log and extending TPM PCRs. This enables
//! remote attestation — a remote party can verify that only known-good
//! code has been executed on the system.

// ---------------------------------------------------------------------------
// IMA actions
// ---------------------------------------------------------------------------

/// Measure the file (hash and log).
pub const IMA_MEASURE: u32 = 1 << 0;
/// Appraise the file (verify signature/hash).
pub const IMA_APPRAISE: u32 = 1 << 1;
/// Audit the measurement.
pub const IMA_AUDIT: u32 = 1 << 2;
/// Hash the file (for digest lists).
pub const IMA_HASH: u32 = 1 << 3;

// ---------------------------------------------------------------------------
// IMA hooks (when measurement occurs)
// ---------------------------------------------------------------------------

/// File opened for read.
pub const IMA_FILE_CHECK: u8 = 0;
/// mmap with PROT_EXEC.
pub const IMA_MMAP_CHECK: u8 = 1;
/// bprm (execve) check.
pub const IMA_BPRM_CHECK: u8 = 2;
/// Module loading.
pub const IMA_MODULE_CHECK: u8 = 3;
/// Firmware loading.
pub const IMA_FIRMWARE_CHECK: u8 = 4;
/// Policy change.
pub const IMA_POLICY_CHECK: u8 = 5;
/// Kexec image.
pub const IMA_KEXEC_CHECK: u8 = 6;
/// Certificate validation.
pub const IMA_CERT_CHECK: u8 = 7;

// ---------------------------------------------------------------------------
// IMA policy conditions
// ---------------------------------------------------------------------------

/// Match by uid.
pub const IMA_COND_UID: u16 = 1;
/// Match by euid.
pub const IMA_COND_EUID: u16 = 2;
/// Match by fowner.
pub const IMA_COND_FOWNER: u16 = 3;
/// Match by fsmagic.
pub const IMA_COND_FSMAGIC: u16 = 4;
/// Match by fsuuid.
pub const IMA_COND_FSUUID: u16 = 5;
/// Match by file mask.
pub const IMA_COND_MASK: u16 = 6;

// ---------------------------------------------------------------------------
// IMA template types
// ---------------------------------------------------------------------------

/// ima (default: d|n).
pub const IMA_TEMPLATE_IMA: u8 = 0;
/// ima-ng (d-ng|n-ng).
pub const IMA_TEMPLATE_IMA_NG: u8 = 1;
/// ima-sig (d-ng|n-ng|sig).
pub const IMA_TEMPLATE_IMA_SIG: u8 = 2;
/// ima-buf (d-ng|n-ng|buf).
pub const IMA_TEMPLATE_IMA_BUF: u8 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_actions_no_overlap() {
        let actions = [IMA_MEASURE, IMA_APPRAISE, IMA_AUDIT, IMA_HASH];
        for i in 0..actions.len() {
            for j in (i + 1)..actions.len() {
                assert_eq!(actions[i] & actions[j], 0);
            }
        }
    }

    #[test]
    fn test_hooks_distinct() {
        let hooks = [
            IMA_FILE_CHECK,
            IMA_MMAP_CHECK,
            IMA_BPRM_CHECK,
            IMA_MODULE_CHECK,
            IMA_FIRMWARE_CHECK,
            IMA_POLICY_CHECK,
            IMA_KEXEC_CHECK,
            IMA_CERT_CHECK,
        ];
        for i in 0..hooks.len() {
            for j in (i + 1)..hooks.len() {
                assert_ne!(hooks[i], hooks[j]);
            }
        }
    }

    #[test]
    fn test_conditions_distinct() {
        let conds = [
            IMA_COND_UID,
            IMA_COND_EUID,
            IMA_COND_FOWNER,
            IMA_COND_FSMAGIC,
            IMA_COND_FSUUID,
            IMA_COND_MASK,
        ];
        for i in 0..conds.len() {
            for j in (i + 1)..conds.len() {
                assert_ne!(conds[i], conds[j]);
            }
        }
    }

    #[test]
    fn test_templates_distinct() {
        let tpls = [
            IMA_TEMPLATE_IMA,
            IMA_TEMPLATE_IMA_NG,
            IMA_TEMPLATE_IMA_SIG,
            IMA_TEMPLATE_IMA_BUF,
        ];
        for i in 0..tpls.len() {
            for j in (i + 1)..tpls.len() {
                assert_ne!(tpls[i], tpls[j]);
            }
        }
    }
}
