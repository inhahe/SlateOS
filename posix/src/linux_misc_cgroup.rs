//! `<linux/cgroup/misc.h>` — Misc cgroup controller constants.
//!
//! The misc cgroup controller provides a generic mechanism to
//! limit and track usage of miscellaneous scalar resources that
//! don't warrant their own dedicated controller. Examples include
//! SEV (Secure Encrypted Virtualization) ASIDs and SGX EPC pages.

// ---------------------------------------------------------------------------
// Cgroup v2 interface files
// ---------------------------------------------------------------------------

/// Maximum allowed for each resource type.
pub const MISC_MAX: &str = "misc.max";
/// Current usage of each resource type.
pub const MISC_CURRENT: &str = "misc.current";
/// Peak usage of each resource type.
pub const MISC_PEAK: &str = "misc.peak";
/// Events (resource exhaustion).
pub const MISC_EVENTS: &str = "misc.events";
/// Capacity (system-wide total per resource).
pub const MISC_CAPACITY: &str = "misc.capacity";

// ---------------------------------------------------------------------------
// Known resource type names
// ---------------------------------------------------------------------------

/// AMD SEV ASIDs.
pub const MISC_RES_SEV: &str = "sev";
/// AMD SEV-ES ASIDs.
pub const MISC_RES_SEV_ES: &str = "sev_es";
/// Intel SGX EPC bytes.
pub const MISC_RES_SGX_EPC: &str = "sgx_epc";

// ---------------------------------------------------------------------------
// Special values
// ---------------------------------------------------------------------------

/// Unlimited (written as "max").
pub const MISC_MAX_STR: &str = "max";

// ---------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------

/// Maximum number of misc resource types supported.
pub const MISC_RES_TYPES_MAX: u32 = 64;

// ---------------------------------------------------------------------------
// Event names
// ---------------------------------------------------------------------------

/// Event: resource allocation was rejected.
pub const MISC_EVENT_MAX: &str = "max";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_interface_files_distinct() {
        let files = [
            MISC_MAX, MISC_CURRENT, MISC_PEAK,
            MISC_EVENTS, MISC_CAPACITY,
        ];
        for i in 0..files.len() {
            for j in (i + 1)..files.len() {
                assert_ne!(files[i], files[j]);
            }
        }
    }

    #[test]
    fn test_interface_files_have_prefix() {
        let files = [
            MISC_MAX, MISC_CURRENT, MISC_PEAK,
            MISC_EVENTS, MISC_CAPACITY,
        ];
        for file in &files {
            assert!(file.starts_with("misc."), "{}", file);
        }
    }

    #[test]
    fn test_resource_names_distinct() {
        let res = [MISC_RES_SEV, MISC_RES_SEV_ES, MISC_RES_SGX_EPC];
        for i in 0..res.len() {
            for j in (i + 1)..res.len() {
                assert_ne!(res[i], res[j]);
            }
        }
    }

    #[test]
    fn test_res_types_max() {
        assert!(MISC_RES_TYPES_MAX > 0);
    }
}
