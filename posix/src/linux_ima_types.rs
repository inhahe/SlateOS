//! `<linux/ima.h>` — Integrity Measurement Architecture (IMA) constants.
//!
//! IMA detects if files have been altered (accidentally or
//! maliciously) by maintaining runtime measurements (hashes) of
//! files. It can enforce appraisal policies requiring valid signatures
//! before files are executed or mmapped. Measurements are stored in
//! the TPM PCR for remote attestation. IMA policies control which
//! files are measured, appraised, and/or audited based on uid, fowner,
//! fsname, and other selectors.

// ---------------------------------------------------------------------------
// IMA action types (what to do when a file is accessed)
// ---------------------------------------------------------------------------

/// Don't measure this file.
pub const IMA_DO_NOT_MEASURE: u32 = 0;
/// Measure the file (add hash to measurement list).
pub const IMA_MEASURE: u32 = 1;
/// Measure and audit the file.
pub const IMA_MEASURED: u32 = 2;

// ---------------------------------------------------------------------------
// IMA policy actions (bit flags)
// ---------------------------------------------------------------------------

/// Measure the file (hash into measurement list).
pub const IMA_ACTION_MEASURE: u32 = 1 << 0;
/// Appraise the file (verify hash/signature before access).
pub const IMA_ACTION_APPRAISE: u32 = 1 << 1;
/// Audit the file access.
pub const IMA_ACTION_AUDIT: u32 = 1 << 2;
/// Hash the file (compute hash for xattr storage).
pub const IMA_ACTION_HASH: u32 = 1 << 3;

// ---------------------------------------------------------------------------
// IMA hooks (when files are checked)
// ---------------------------------------------------------------------------

/// File is being opened (read/write).
pub const IMA_HOOK_FILE_CHECK: u32 = 1;
/// File is being mmap'd with execute permission.
pub const IMA_HOOK_MMAP_CHECK: u32 = 2;
/// Binary is being executed (execve).
pub const IMA_HOOK_BPRM_CHECK: u32 = 3;
/// Kernel module is being loaded.
pub const IMA_HOOK_MODULE_CHECK: u32 = 4;
/// Firmware is being loaded.
pub const IMA_HOOK_FIRMWARE_CHECK: u32 = 5;
/// Kexec image is being loaded.
pub const IMA_HOOK_KEXEC_CHECK: u32 = 6;
/// Policy is being read.
pub const IMA_HOOK_POLICY_CHECK: u32 = 7;
/// Kernel kexec initramfs.
pub const IMA_HOOK_KEXEC_INITRAMFS_CHECK: u32 = 8;
/// Critical data (keyrings, etc.).
pub const IMA_HOOK_CRITICAL_DATA: u32 = 9;

// ---------------------------------------------------------------------------
// IMA appraisal status
// ---------------------------------------------------------------------------

/// File has not been appraised yet.
pub const IMA_APPRAISE_NOT_APPRAISED: u32 = 0;
/// File passed appraisal (hash/signature valid).
pub const IMA_APPRAISE_OK: u32 = 1;
/// File failed appraisal (hash mismatch or missing signature).
pub const IMA_APPRAISE_BAD: u32 = 2;
/// File appraisal skipped (policy exemption).
pub const IMA_APPRAISE_SKIP: u32 = 3;
/// File needs re-appraisal (content changed).
pub const IMA_APPRAISE_NEEDS_REAPPRAISE: u32 = 4;

// ---------------------------------------------------------------------------
// IMA hash algorithms (for measurement and appraisal)
// ---------------------------------------------------------------------------

/// SHA-1 (legacy, not recommended).
pub const IMA_HASH_SHA1: u32 = 0;
/// SHA-256 (default).
pub const IMA_HASH_SHA256: u32 = 1;
/// SHA-384.
pub const IMA_HASH_SHA384: u32 = 2;
/// SHA-512.
pub const IMA_HASH_SHA512: u32 = 3;
/// SM3 (Chinese national standard).
pub const IMA_HASH_SM3: u32 = 4;

// ---------------------------------------------------------------------------
// IMA xattr types (stored in security.ima extended attribute)
// ---------------------------------------------------------------------------

/// No xattr.
pub const IMA_XATTR_NONE: u32 = 0;
/// Digest only (hash of file content).
pub const IMA_XATTR_DIGEST: u32 = 1;
/// Signature (IMA/EVM signature over digest).
pub const IMA_XATTR_SIGNATURE: u32 = 2;
/// Digest with algorithm prefix (hash_algo + digest).
pub const IMA_XATTR_DIGEST_NG: u32 = 3;
/// Signature v2 (with additional metadata).
pub const IMA_XATTR_SIGNATURE_V2: u32 = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_policy_actions_no_overlap() {
        let actions = [
            IMA_ACTION_MEASURE,
            IMA_ACTION_APPRAISE,
            IMA_ACTION_AUDIT,
            IMA_ACTION_HASH,
        ];
        for i in 0..actions.len() {
            assert!(actions[i].is_power_of_two());
            for j in (i + 1)..actions.len() {
                assert_eq!(actions[i] & actions[j], 0);
            }
        }
    }

    #[test]
    fn test_hooks_distinct() {
        let hooks = [
            IMA_HOOK_FILE_CHECK,
            IMA_HOOK_MMAP_CHECK,
            IMA_HOOK_BPRM_CHECK,
            IMA_HOOK_MODULE_CHECK,
            IMA_HOOK_FIRMWARE_CHECK,
            IMA_HOOK_KEXEC_CHECK,
            IMA_HOOK_POLICY_CHECK,
            IMA_HOOK_KEXEC_INITRAMFS_CHECK,
            IMA_HOOK_CRITICAL_DATA,
        ];
        for i in 0..hooks.len() {
            for j in (i + 1)..hooks.len() {
                assert_ne!(hooks[i], hooks[j]);
            }
        }
    }

    #[test]
    fn test_appraisal_status_distinct() {
        let statuses = [
            IMA_APPRAISE_NOT_APPRAISED,
            IMA_APPRAISE_OK,
            IMA_APPRAISE_BAD,
            IMA_APPRAISE_SKIP,
            IMA_APPRAISE_NEEDS_REAPPRAISE,
        ];
        for i in 0..statuses.len() {
            for j in (i + 1)..statuses.len() {
                assert_ne!(statuses[i], statuses[j]);
            }
        }
    }

    #[test]
    fn test_hash_algos_distinct() {
        let algos = [
            IMA_HASH_SHA1,
            IMA_HASH_SHA256,
            IMA_HASH_SHA384,
            IMA_HASH_SHA512,
            IMA_HASH_SM3,
        ];
        for i in 0..algos.len() {
            for j in (i + 1)..algos.len() {
                assert_ne!(algos[i], algos[j]);
            }
        }
    }

    #[test]
    fn test_xattr_types_distinct() {
        let types = [
            IMA_XATTR_NONE,
            IMA_XATTR_DIGEST,
            IMA_XATTR_SIGNATURE,
            IMA_XATTR_DIGEST_NG,
            IMA_XATTR_SIGNATURE_V2,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }
}
