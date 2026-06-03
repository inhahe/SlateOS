//! `<asm/mce.h>` — Machine Check Exception (MCE) constants.
//!
//! Machine Check Exceptions are CPU-generated exceptions for hardware
//! errors detected by the CPU itself (cache ECC errors, bus errors,
//! TLB errors, internal parity errors). The MCE handler logs the
//! error via MSRs (IA32_MCi_STATUS, IA32_MCi_ADDR, IA32_MCi_MISC),
//! optionally recovers (if the error is correctable or in userspace
//! data), or panics (if the error is fatal/unrecoverable). MCEs are
//! exposed to userspace via /dev/mcelog and ras:mc_event tracepoints.

// ---------------------------------------------------------------------------
// MCE bank status register flags (IA32_MCi_STATUS)
// ---------------------------------------------------------------------------

/// Error is valid (VAL bit).
pub const MCE_STATUS_VAL: u64 = 1 << 63;
/// Overflow — additional errors not logged.
pub const MCE_STATUS_OVER: u64 = 1 << 62;
/// Uncorrected error.
pub const MCE_STATUS_UC: u64 = 1 << 61;
/// Error enabled (reporting is enabled for this error type).
pub const MCE_STATUS_EN: u64 = 1 << 60;
/// Miscellaneous register valid (MCi_MISC has useful info).
pub const MCE_STATUS_MISCV: u64 = 1 << 59;
/// Address register valid (MCi_ADDR has useful info).
pub const MCE_STATUS_ADDRV: u64 = 1 << 58;
/// Processor context corrupted (cannot continue).
pub const MCE_STATUS_PCC: u64 = 1 << 57;
/// Signaled (error was signaled via MCE or CMCI).
pub const MCE_STATUS_S: u64 = 1 << 56;
/// Correctable error recoverable (action required).
pub const MCE_STATUS_AR: u64 = 1 << 55;

// ---------------------------------------------------------------------------
// MCE error code types (MCA error codes in IA32_MCi_STATUS[15:0])
// ---------------------------------------------------------------------------

/// No error.
pub const MCE_ERROR_NONE: u16 = 0x0000;
/// Unclassified error.
pub const MCE_ERROR_UNCLASSIFIED: u16 = 0x0001;
/// Microcode ROM parity error.
pub const MCE_ERROR_UCODE_PARITY: u16 = 0x0002;
/// External error (bus/interconnect).
pub const MCE_ERROR_EXTERNAL: u16 = 0x0003;
/// FRC (Functional Redundancy Check) error.
pub const MCE_ERROR_FRC: u16 = 0x0004;
/// Internal parity error.
pub const MCE_ERROR_INTERNAL_PARITY: u16 = 0x0005;

// ---------------------------------------------------------------------------
// MCE severity levels (kernel's assessment)
// ---------------------------------------------------------------------------

/// No action needed (informational).
pub const MCE_SEVERITY_NO_ACTION: u32 = 0;
/// Some action required (e.g., offline page).
pub const MCE_SEVERITY_SOME: u32 = 1;
/// Action required (must handle to continue).
pub const MCE_SEVERITY_AR: u32 = 2;
/// Uncorrected no action required (log and continue).
pub const MCE_SEVERITY_UC_NAR: u32 = 3;
/// Fatal (panic).
pub const MCE_SEVERITY_PANIC: u32 = 4;

// ---------------------------------------------------------------------------
// MCE action types
// ---------------------------------------------------------------------------

/// Kill the affected process.
pub const MCE_ACTION_KILL: u32 = 0;
/// Offline the affected memory page (soft offline).
pub const MCE_ACTION_SOFT_OFFLINE: u32 = 1;
/// Offline the affected memory page (hard offline).
pub const MCE_ACTION_HARD_OFFLINE: u32 = 2;
/// Panic the system.
pub const MCE_ACTION_PANIC: u32 = 3;
/// Continue (error was corrected or non-critical).
pub const MCE_ACTION_CONTINUE: u32 = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_flags_no_overlap() {
        let flags = [
            MCE_STATUS_VAL,
            MCE_STATUS_OVER,
            MCE_STATUS_UC,
            MCE_STATUS_EN,
            MCE_STATUS_MISCV,
            MCE_STATUS_ADDRV,
            MCE_STATUS_PCC,
            MCE_STATUS_S,
            MCE_STATUS_AR,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_error_codes_distinct() {
        let codes = [
            MCE_ERROR_NONE,
            MCE_ERROR_UNCLASSIFIED,
            MCE_ERROR_UCODE_PARITY,
            MCE_ERROR_EXTERNAL,
            MCE_ERROR_FRC,
            MCE_ERROR_INTERNAL_PARITY,
        ];
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                assert_ne!(codes[i], codes[j]);
            }
        }
    }

    #[test]
    fn test_severity_distinct() {
        let sevs = [
            MCE_SEVERITY_NO_ACTION,
            MCE_SEVERITY_SOME,
            MCE_SEVERITY_AR,
            MCE_SEVERITY_UC_NAR,
            MCE_SEVERITY_PANIC,
        ];
        for i in 0..sevs.len() {
            for j in (i + 1)..sevs.len() {
                assert_ne!(sevs[i], sevs[j]);
            }
        }
    }

    #[test]
    fn test_actions_distinct() {
        let actions = [
            MCE_ACTION_KILL,
            MCE_ACTION_SOFT_OFFLINE,
            MCE_ACTION_HARD_OFFLINE,
            MCE_ACTION_PANIC,
            MCE_ACTION_CONTINUE,
        ];
        for i in 0..actions.len() {
            for j in (i + 1)..actions.len() {
                assert_ne!(actions[i], actions[j]);
            }
        }
    }
}
