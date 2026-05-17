//! `<asm/mce.h>` (additional) — Machine Check Exception extended constants.
//!
//! Machine Check Exceptions (MCEs) report hardware errors detected
//! by the CPU: memory ECC errors, cache parity errors, bus errors,
//! thermal overload. The MCE handler logs the error, determines if
//! it's recoverable, and either isolates the affected page (if
//! recoverable) or panics the system (if fatal). These constants
//! cover the extended MCE status fields and recovery actions.

// ---------------------------------------------------------------------------
// MCE severity levels
// ---------------------------------------------------------------------------

/// No error (informational).
pub const MCE_SEVERITY_NO: u32 = 0;
/// Keep going (corrected error, no action needed).
pub const MCE_SEVERITY_KEEP: u32 = 1;
/// Some error (log but continue).
pub const MCE_SEVERITY_SOME: u32 = 2;
/// Action required (recoverable, kill affected process/page).
pub const MCE_SEVERITY_AR: u32 = 3;
/// Urgent action required (recoverable, immediate action).
pub const MCE_SEVERITY_UC: u32 = 4;
/// Panic (unrecoverable, system must halt).
pub const MCE_SEVERITY_PANIC: u32 = 5;

// ---------------------------------------------------------------------------
// MCE action types (what to do about the error)
// ---------------------------------------------------------------------------

/// No action (error was corrected by hardware).
pub const MCE_ACTION_NONE: u32 = 0;
/// Offline the affected memory page (poison it).
pub const MCE_ACTION_PAGE_OFFLINE: u32 = 1;
/// Kill the process that consumed corrupt data.
pub const MCE_ACTION_KILL_PROCESS: u32 = 2;
/// Soft-offline (test the page, migrate if possible).
pub const MCE_ACTION_SOFT_OFFLINE: u32 = 3;
/// Reset the system (last resort).
pub const MCE_ACTION_RESET: u32 = 4;

// ---------------------------------------------------------------------------
// MCE error source types
// ---------------------------------------------------------------------------

/// CPU internal cache error.
pub const MCE_SOURCE_CACHE: u32 = 0;
/// Memory controller error (ECC).
pub const MCE_SOURCE_MEMORY: u32 = 1;
/// Bus/interconnect error (QPI/UPI).
pub const MCE_SOURCE_BUS: u32 = 2;
/// TLB (Translation Lookaside Buffer) error.
pub const MCE_SOURCE_TLB: u32 = 3;
/// Internal unclassified error.
pub const MCE_SOURCE_INTERNAL: u32 = 4;

// ---------------------------------------------------------------------------
// Memory poison flags
// ---------------------------------------------------------------------------

/// Page has been poisoned (HWPoison).
pub const PAGE_POISONED: u32 = 0x01;
/// Page poison is being resolved (migration attempt).
pub const PAGE_POISON_RESOLVING: u32 = 0x02;
/// Page was successfully migrated away from bad memory.
pub const PAGE_POISON_MIGRATED: u32 = 0x04;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_severity_ordered() {
        assert!(MCE_SEVERITY_NO < MCE_SEVERITY_KEEP);
        assert!(MCE_SEVERITY_KEEP < MCE_SEVERITY_SOME);
        assert!(MCE_SEVERITY_SOME < MCE_SEVERITY_AR);
        assert!(MCE_SEVERITY_AR < MCE_SEVERITY_UC);
        assert!(MCE_SEVERITY_UC < MCE_SEVERITY_PANIC);
    }

    #[test]
    fn test_actions_distinct() {
        let actions = [
            MCE_ACTION_NONE, MCE_ACTION_PAGE_OFFLINE,
            MCE_ACTION_KILL_PROCESS, MCE_ACTION_SOFT_OFFLINE,
            MCE_ACTION_RESET,
        ];
        for i in 0..actions.len() {
            for j in (i + 1)..actions.len() {
                assert_ne!(actions[i], actions[j]);
            }
        }
    }

    #[test]
    fn test_sources_distinct() {
        let sources = [
            MCE_SOURCE_CACHE, MCE_SOURCE_MEMORY, MCE_SOURCE_BUS,
            MCE_SOURCE_TLB, MCE_SOURCE_INTERNAL,
        ];
        for i in 0..sources.len() {
            for j in (i + 1)..sources.len() {
                assert_ne!(sources[i], sources[j]);
            }
        }
    }

    #[test]
    fn test_poison_flags_no_overlap() {
        let flags = [PAGE_POISONED, PAGE_POISON_RESOLVING, PAGE_POISON_MIGRATED];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
