//! `<linux/security.h>` (lockdown subset) — Kernel lockdown constants.
//!
//! Kernel lockdown restricts what userspace (even root) can do to the
//! running kernel. In "integrity" mode, operations that could modify
//! the kernel are blocked (writing to /dev/mem, loading unsigned modules,
//! using kexec with unsigned images). In "confidentiality" mode,
//! additionally operations that could extract kernel secrets are blocked
//! (reading /proc/kcore, using perf to read kernel memory). Lockdown
//! is typically enabled on UEFI Secure Boot systems.

// ---------------------------------------------------------------------------
// Lockdown levels
// ---------------------------------------------------------------------------

/// No lockdown (all operations permitted).
pub const LOCKDOWN_NONE: u32 = 0;
/// Integrity lockdown (prevent kernel modification).
pub const LOCKDOWN_INTEGRITY: u32 = 1;
/// Confidentiality lockdown (prevent kernel read + modification).
pub const LOCKDOWN_CONFIDENTIALITY: u32 = 2;

// ---------------------------------------------------------------------------
// Lockdown reasons (what triggered the lockdown check)
// ---------------------------------------------------------------------------

/// Module loading (unsigned module).
pub const LOCKDOWN_REASON_MODULE: u32 = 0;
/// /dev/mem or /dev/kmem access.
pub const LOCKDOWN_REASON_DEV_MEM: u32 = 1;
/// kexec (unsigned image).
pub const LOCKDOWN_REASON_KEXEC: u32 = 2;
/// Hibernation (image could be tampered).
pub const LOCKDOWN_REASON_HIBERNATION: u32 = 3;
/// PCI BAR access from userspace.
pub const LOCKDOWN_REASON_PCI_ACCESS: u32 = 4;
/// ACPI table override.
pub const LOCKDOWN_REASON_ACPI_TABLES: u32 = 5;
/// MSR write from userspace.
pub const LOCKDOWN_REASON_MSR: u32 = 6;
/// eBPF write to kernel memory.
pub const LOCKDOWN_REASON_BPF_WRITE: u32 = 7;
/// perf access to kernel addresses.
pub const LOCKDOWN_REASON_PERF: u32 = 8;
/// Tracefs/ftrace access.
pub const LOCKDOWN_REASON_TRACEFS: u32 = 9;
/// Debugfs access.
pub const LOCKDOWN_REASON_DEBUGFS: u32 = 10;
/// IOPL/IOPERM (I/O port access).
pub const LOCKDOWN_REASON_IOPORT: u32 = 11;

// ---------------------------------------------------------------------------
// Lockdown policy flags
// ---------------------------------------------------------------------------

/// Lockdown was set by kernel command line.
pub const LOCKDOWN_FLAG_CMDLINE: u32 = 0x01;
/// Lockdown was set by Secure Boot.
pub const LOCKDOWN_FLAG_SECUREBOOT: u32 = 0x02;
/// Lockdown was set by LSM policy.
pub const LOCKDOWN_FLAG_LSM: u32 = 0x04;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_levels_ordered() {
        assert!(LOCKDOWN_NONE < LOCKDOWN_INTEGRITY);
        assert!(LOCKDOWN_INTEGRITY < LOCKDOWN_CONFIDENTIALITY);
    }

    #[test]
    fn test_reasons_distinct() {
        let reasons = [
            LOCKDOWN_REASON_MODULE, LOCKDOWN_REASON_DEV_MEM,
            LOCKDOWN_REASON_KEXEC, LOCKDOWN_REASON_HIBERNATION,
            LOCKDOWN_REASON_PCI_ACCESS, LOCKDOWN_REASON_ACPI_TABLES,
            LOCKDOWN_REASON_MSR, LOCKDOWN_REASON_BPF_WRITE,
            LOCKDOWN_REASON_PERF, LOCKDOWN_REASON_TRACEFS,
            LOCKDOWN_REASON_DEBUGFS, LOCKDOWN_REASON_IOPORT,
        ];
        for i in 0..reasons.len() {
            for j in (i + 1)..reasons.len() {
                assert_ne!(reasons[i], reasons[j]);
            }
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            LOCKDOWN_FLAG_CMDLINE, LOCKDOWN_FLAG_SECUREBOOT,
            LOCKDOWN_FLAG_LSM,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
