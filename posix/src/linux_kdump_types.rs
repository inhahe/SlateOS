//! `<linux/crash_dump.h>` — Kdump/crash kernel constants.
//!
//! Kdump captures kernel crash dumps by booting a pre-loaded
//! "crash kernel" on panic. These constants define the crash
//! kernel's reserved memory layout, dump format markers, and
//! crash reason codes.

// ---------------------------------------------------------------------------
// Crash kernel memory reservation
// ---------------------------------------------------------------------------

/// Default crash kernel memory size (128 MiB).
pub const CRASHKERNEL_DEFAULT_SIZE: u64 = 128 * 1024 * 1024;
/// Minimum crash kernel memory (16 MiB).
pub const CRASHKERNEL_MIN_SIZE: u64 = 16 * 1024 * 1024;
/// Alignment for crash kernel memory (16 MiB).
pub const CRASHKERNEL_ALIGN: u64 = 16 * 1024 * 1024;

// ---------------------------------------------------------------------------
// Vmcore (ELF dump) markers
// ---------------------------------------------------------------------------

/// Vmcore ELF note type: kernel crash info.
pub const VMCOREINFO_NOTE_NAME_SIZE: u32 = 12;
/// Vmcore note type (NT_VMCOREINFO).
pub const VMCOREINFO_NOTE_TYPE: u32 = 0x0000_0900;

// ---------------------------------------------------------------------------
// Crash reason codes (passed to crash kernel)
// ---------------------------------------------------------------------------

/// Kernel panic.
pub const CRASH_REASON_PANIC: u32 = 0;
/// Hardware watchdog timeout.
pub const CRASH_REASON_WATCHDOG: u32 = 1;
/// Out-of-memory killer triggered.
pub const CRASH_REASON_OOM: u32 = 2;
/// Kernel oops (non-fatal error that escalated).
pub const CRASH_REASON_OOPS: u32 = 3;
/// Soft lockup detected.
pub const CRASH_REASON_SOFT_LOCKUP: u32 = 4;
/// Hard lockup detected (NMI watchdog).
pub const CRASH_REASON_HARD_LOCKUP: u32 = 5;
/// RCU stall detected.
pub const CRASH_REASON_RCU_STALL: u32 = 6;
/// User-triggered crash (sysrq-c or /proc/sysrq-trigger).
pub const CRASH_REASON_USER: u32 = 7;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_sizes() {
        assert_eq!(CRASHKERNEL_DEFAULT_SIZE, 128 * 1024 * 1024);
        assert_eq!(CRASHKERNEL_MIN_SIZE, 16 * 1024 * 1024);
        assert!(CRASHKERNEL_MIN_SIZE < CRASHKERNEL_DEFAULT_SIZE);
    }

    #[test]
    fn test_alignment_power_of_two() {
        assert!(CRASHKERNEL_ALIGN.is_power_of_two());
    }

    #[test]
    fn test_reasons_distinct() {
        let reasons = [
            CRASH_REASON_PANIC,
            CRASH_REASON_WATCHDOG,
            CRASH_REASON_OOM,
            CRASH_REASON_OOPS,
            CRASH_REASON_SOFT_LOCKUP,
            CRASH_REASON_HARD_LOCKUP,
            CRASH_REASON_RCU_STALL,
            CRASH_REASON_USER,
        ];
        for i in 0..reasons.len() {
            for j in (i + 1)..reasons.len() {
                assert_ne!(reasons[i], reasons[j]);
            }
        }
    }

    #[test]
    fn test_panic_is_zero() {
        assert_eq!(CRASH_REASON_PANIC, 0);
    }

    #[test]
    fn test_vmcoreinfo() {
        assert_eq!(VMCOREINFO_NOTE_TYPE, 0x900);
    }
}
