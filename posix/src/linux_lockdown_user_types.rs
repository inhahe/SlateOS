//! `<linux/lockdown.h>` — kernel lockdown LSM levels and reasons.
//!
//! "Lockdown" is the LSM that disables kernel interfaces capable of
//! letting root tamper with the running kernel (kexec without sig,
//! tracefs, /dev/mem, PCIe BARs, …). Secure-boot installers and the
//! kernel-signing pipeline read these constants from
//! `/sys/kernel/security/lockdown`.

// ---------------------------------------------------------------------------
// Securityfs path
// ---------------------------------------------------------------------------

/// File exposing the current lockdown level.
pub const LOCKDOWN_SECURITYFS_PATH: &str = "/sys/kernel/security/lockdown";

// ---------------------------------------------------------------------------
// Lockdown levels (`enum lockdown_reason`, only the level-changing values)
// ---------------------------------------------------------------------------

pub const LOCKDOWN_NONE: u32 = 0;
pub const LOCKDOWN_MODULE_SIGNATURE: u32 = 1;
pub const LOCKDOWN_DEV_MEM: u32 = 2;
pub const LOCKDOWN_EFI_TEST: u32 = 3;
pub const LOCKDOWN_KEXEC: u32 = 4;
pub const LOCKDOWN_HIBERNATION: u32 = 5;
pub const LOCKDOWN_PCI_ACCESS: u32 = 6;
pub const LOCKDOWN_IOPORT: u32 = 7;
pub const LOCKDOWN_MSR: u32 = 8;
pub const LOCKDOWN_ACPI_TABLES: u32 = 9;
pub const LOCKDOWN_PCMCIA_CIS: u32 = 10;
pub const LOCKDOWN_TIOCSSERIAL: u32 = 11;
pub const LOCKDOWN_MODULE_PARAMETERS: u32 = 12;
pub const LOCKDOWN_MMIOTRACE: u32 = 13;
pub const LOCKDOWN_DEBUGFS: u32 = 14;
pub const LOCKDOWN_XMON_WR: u32 = 15;
pub const LOCKDOWN_INTEGRITY_MAX: u32 = 16;

pub const LOCKDOWN_KCORE: u32 = 17;
pub const LOCKDOWN_KPROBES: u32 = 18;
pub const LOCKDOWN_BPF_READ_KERNEL: u32 = 19;
pub const LOCKDOWN_DBG_READ_KERNEL: u32 = 20;
pub const LOCKDOWN_PERF: u32 = 21;
pub const LOCKDOWN_TRACEFS: u32 = 22;
pub const LOCKDOWN_XMON_RW: u32 = 23;
pub const LOCKDOWN_XFRM_SECRET: u32 = 24;
pub const LOCKDOWN_CONFIDENTIALITY_MAX: u32 = 25;

// ---------------------------------------------------------------------------
// Userspace selection strings
// ---------------------------------------------------------------------------

pub const LOCKDOWN_STR_NONE: &str = "none";
pub const LOCKDOWN_STR_INTEGRITY: &str = "integrity";
pub const LOCKDOWN_STR_CONFIDENTIALITY: &str = "confidentiality";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_securityfs_path() {
        assert_eq!(LOCKDOWN_SECURITYFS_PATH, "/sys/kernel/security/lockdown");
        assert!(LOCKDOWN_SECURITYFS_PATH.starts_with("/sys/kernel/security/"));
    }

    #[test]
    fn test_lockdown_reasons_dense_0_to_25() {
        let r = [
            LOCKDOWN_NONE,
            LOCKDOWN_MODULE_SIGNATURE,
            LOCKDOWN_DEV_MEM,
            LOCKDOWN_EFI_TEST,
            LOCKDOWN_KEXEC,
            LOCKDOWN_HIBERNATION,
            LOCKDOWN_PCI_ACCESS,
            LOCKDOWN_IOPORT,
            LOCKDOWN_MSR,
            LOCKDOWN_ACPI_TABLES,
            LOCKDOWN_PCMCIA_CIS,
            LOCKDOWN_TIOCSSERIAL,
            LOCKDOWN_MODULE_PARAMETERS,
            LOCKDOWN_MMIOTRACE,
            LOCKDOWN_DEBUGFS,
            LOCKDOWN_XMON_WR,
            LOCKDOWN_INTEGRITY_MAX,
            LOCKDOWN_KCORE,
            LOCKDOWN_KPROBES,
            LOCKDOWN_BPF_READ_KERNEL,
            LOCKDOWN_DBG_READ_KERNEL,
            LOCKDOWN_PERF,
            LOCKDOWN_TRACEFS,
            LOCKDOWN_XMON_RW,
            LOCKDOWN_XFRM_SECRET,
            LOCKDOWN_CONFIDENTIALITY_MAX,
        ];
        for (i, &v) in r.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_max_boundaries_layered() {
        // INTEGRITY_MAX < CONFIDENTIALITY_MAX — confidentiality is a stricter
        // superset of integrity.
        assert!(LOCKDOWN_INTEGRITY_MAX < LOCKDOWN_CONFIDENTIALITY_MAX);
        // KCORE is the first reason that requires confidentiality lockdown.
        assert!(LOCKDOWN_KCORE > LOCKDOWN_INTEGRITY_MAX);
    }

    #[test]
    fn test_selection_strings_lowercase() {
        for s in [LOCKDOWN_STR_NONE, LOCKDOWN_STR_INTEGRITY, LOCKDOWN_STR_CONFIDENTIALITY] {
            for b in s.as_bytes() {
                assert!(b.is_ascii_lowercase());
            }
        }
    }
}
