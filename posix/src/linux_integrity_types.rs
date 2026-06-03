//! `<linux/integrity.h>` — Integrity Measurement Architecture constants.
//!
//! The Linux integrity subsystem ensures that files haven't been
//! tampered with. IMA (Integrity Measurement Architecture) measures
//! file hashes at access time and can enforce that only files with
//! known-good hashes are executed or opened. EVM (Extended Verification
//! Module) protects file metadata (extended attributes) using HMAC or
//! digital signatures. Together they form a chain of trust from boot
//! to application execution.

// ---------------------------------------------------------------------------
// IMA policy actions
// ---------------------------------------------------------------------------

/// Don't measure (skip this file).
pub const IMA_ACTION_DONT_MEASURE: u32 = 0;
/// Measure (record hash in measurement list).
pub const IMA_ACTION_MEASURE: u32 = 1;
/// Appraise (verify hash matches stored reference).
pub const IMA_ACTION_APPRAISE: u32 = 2;
/// Audit (log measurement event).
pub const IMA_ACTION_AUDIT: u32 = 3;
/// Hash (calculate but don't add to log).
pub const IMA_ACTION_HASH: u32 = 4;

// ---------------------------------------------------------------------------
// IMA policy conditions
// ---------------------------------------------------------------------------

/// Match by uid.
pub const IMA_COND_UID: u32 = 0x01;
/// Match by file owner (fowner).
pub const IMA_COND_FOWNER: u32 = 0x02;
/// Match by filesystem magic number.
pub const IMA_COND_FSMAGIC: u32 = 0x04;
/// Match by filesystem UUID.
pub const IMA_COND_FSUUID: u32 = 0x08;
/// Match by filename.
pub const IMA_COND_FNAME: u32 = 0x10;
/// Match by LSM label.
pub const IMA_COND_LSM_LABEL: u32 = 0x20;

// ---------------------------------------------------------------------------
// IMA hash algorithms
// ---------------------------------------------------------------------------

/// SHA-1 (legacy, 160-bit).
pub const IMA_HASH_SHA1: u32 = 0;
/// SHA-256 (default, 256-bit).
pub const IMA_HASH_SHA256: u32 = 1;
/// SHA-384 (384-bit).
pub const IMA_HASH_SHA384: u32 = 2;
/// SHA-512 (512-bit).
pub const IMA_HASH_SHA512: u32 = 3;
/// SM3 (Chinese national standard, 256-bit).
pub const IMA_HASH_SM3: u32 = 4;

// ---------------------------------------------------------------------------
// IMA template types
// ---------------------------------------------------------------------------

/// ima template (hash + filename).
pub const IMA_TEMPLATE_IMA: u32 = 0;
/// ima-ng template (hash algorithm + hash + filename).
pub const IMA_TEMPLATE_IMA_NG: u32 = 1;
/// ima-sig template (ima-ng + digital signature).
pub const IMA_TEMPLATE_IMA_SIG: u32 = 2;
/// ima-buf template (buffer data measurement).
pub const IMA_TEMPLATE_IMA_BUF: u32 = 3;
/// ima-modsig template (module appended signature).
pub const IMA_TEMPLATE_IMA_MODSIG: u32 = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_actions_distinct() {
        let actions = [
            IMA_ACTION_DONT_MEASURE,
            IMA_ACTION_MEASURE,
            IMA_ACTION_APPRAISE,
            IMA_ACTION_AUDIT,
            IMA_ACTION_HASH,
        ];
        for i in 0..actions.len() {
            for j in (i + 1)..actions.len() {
                assert_ne!(actions[i], actions[j]);
            }
        }
    }

    #[test]
    fn test_conditions_no_overlap() {
        let conds = [
            IMA_COND_UID,
            IMA_COND_FOWNER,
            IMA_COND_FSMAGIC,
            IMA_COND_FSUUID,
            IMA_COND_FNAME,
            IMA_COND_LSM_LABEL,
        ];
        for i in 0..conds.len() {
            assert!(conds[i].is_power_of_two());
            for j in (i + 1)..conds.len() {
                assert_eq!(conds[i] & conds[j], 0);
            }
        }
    }

    #[test]
    fn test_hash_algorithms_distinct() {
        let hashes = [
            IMA_HASH_SHA1,
            IMA_HASH_SHA256,
            IMA_HASH_SHA384,
            IMA_HASH_SHA512,
            IMA_HASH_SM3,
        ];
        for i in 0..hashes.len() {
            for j in (i + 1)..hashes.len() {
                assert_ne!(hashes[i], hashes[j]);
            }
        }
    }

    #[test]
    fn test_templates_distinct() {
        let templates = [
            IMA_TEMPLATE_IMA,
            IMA_TEMPLATE_IMA_NG,
            IMA_TEMPLATE_IMA_SIG,
            IMA_TEMPLATE_IMA_BUF,
            IMA_TEMPLATE_IMA_MODSIG,
        ];
        for i in 0..templates.len() {
            for j in (i + 1)..templates.len() {
                assert_ne!(templates[i], templates[j]);
            }
        }
    }
}
