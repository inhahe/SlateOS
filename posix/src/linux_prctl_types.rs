//! `<linux/prctl.h>` — prctl() process control constants.
//!
//! prctl() is a catch-all syscall for per-process/per-thread settings
//! that don't warrant their own syscall. It controls security features,
//! signal behavior, memory management hints, naming, and hardware
//! feature access.

// ---------------------------------------------------------------------------
// prctl option codes
// ---------------------------------------------------------------------------

/// Set process name (comm field, max 16 bytes).
pub const PR_SET_NAME: u32 = 15;
/// Get process name.
pub const PR_GET_NAME: u32 = 16;
/// Set dumpable flag.
pub const PR_SET_DUMPABLE: u32 = 4;
/// Get dumpable flag.
pub const PR_GET_DUMPABLE: u32 = 3;
/// Set securebits.
pub const PR_SET_SECUREBITS: u32 = 28;
/// Get securebits.
pub const PR_GET_SECUREBITS: u32 = 27;
/// Set no-new-privileges flag.
pub const PR_SET_NO_NEW_PRIVS: u32 = 38;
/// Get no-new-privileges flag.
pub const PR_GET_NO_NEW_PRIVS: u32 = 39;
/// Set parent-death signal.
pub const PR_SET_PDEATHSIG: u32 = 1;
/// Get parent-death signal.
pub const PR_GET_PDEATHSIG: u32 = 2;
/// Set timing method (statistical or timestamp).
pub const PR_SET_TIMING: u32 = 14;
/// Get timing method.
pub const PR_GET_TIMING: u32 = 13;
/// Set endianness (PowerPC).
pub const PR_SET_ENDIAN: u32 = 20;
/// Get endianness.
pub const PR_GET_ENDIAN: u32 = 19;
/// Set child subreaper.
pub const PR_SET_CHILD_SUBREAPER: u32 = 36;
/// Get child subreaper.
pub const PR_GET_CHILD_SUBREAPER: u32 = 37;
/// Set thread-local storage area.
pub const PR_SET_THP_DISABLE: u32 = 41;
/// Get THP disable.
pub const PR_GET_THP_DISABLE: u32 = 42;
/// Set speculation control.
pub const PR_SET_SPECULATION_CTRL: u32 = 53;
/// Get speculation control.
pub const PR_GET_SPECULATION_CTRL: u32 = 52;
/// Set memory-merge (KSM).
pub const PR_SET_MEMORY_MERGE: u32 = 67;
/// Get memory-merge.
pub const PR_GET_MEMORY_MERGE: u32 = 68;

// ---------------------------------------------------------------------------
// Seccomp-related prctl
// ---------------------------------------------------------------------------

/// Set seccomp mode.
pub const PR_SET_SECCOMP: u32 = 22;
/// Get seccomp mode.
pub const PR_GET_SECCOMP: u32 = 21;

// ---------------------------------------------------------------------------
// Speculation control values
// ---------------------------------------------------------------------------

/// Speculation: not affected.
pub const PR_SPEC_NOT_AFFECTED: u32 = 0;
/// Speculation: prctl control available.
pub const PR_SPEC_PRCTL: u32 = 1 << 0;
/// Speculation: mitigation enabled.
pub const PR_SPEC_ENABLE: u32 = 1 << 1;
/// Speculation: mitigation disabled.
pub const PR_SPEC_DISABLE: u32 = 1 << 2;
/// Speculation: forced mitigation.
pub const PR_SPEC_FORCE_DISABLE: u32 = 1 << 3;
/// Speculation: disable SSBD (Speculative Store Bypass).
pub const PR_SPEC_DISABLE_NOEXEC: u32 = 1 << 4;

// ---------------------------------------------------------------------------
// Speculation control commands
// ---------------------------------------------------------------------------

/// Store bypass speculation.
pub const PR_SPEC_STORE_BYPASS: u32 = 0;
/// Indirect branch speculation.
pub const PR_SPEC_INDIRECT_BRANCH: u32 = 1;
/// L1D flush.
pub const PR_SPEC_L1D_FLUSH: u32 = 2;

// ---------------------------------------------------------------------------
// Dumpable values
// ---------------------------------------------------------------------------

/// Not dumpable.
pub const SUID_DUMP_DISABLE: u32 = 0;
/// Dumpable (normal user processes).
pub const SUID_DUMP_USER: u32 = 1;
/// Root-only dump (setuid processes).
pub const SUID_DUMP_ROOT: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_option_codes_distinct() {
        let opts = [
            PR_SET_PDEATHSIG,
            PR_GET_PDEATHSIG,
            PR_GET_DUMPABLE,
            PR_SET_DUMPABLE,
            PR_GET_TIMING,
            PR_SET_TIMING,
            PR_SET_NAME,
            PR_GET_NAME,
            PR_GET_ENDIAN,
            PR_SET_ENDIAN,
            PR_GET_SECCOMP,
            PR_SET_SECCOMP,
            PR_GET_SECUREBITS,
            PR_SET_SECUREBITS,
            PR_SET_CHILD_SUBREAPER,
            PR_GET_CHILD_SUBREAPER,
            PR_SET_NO_NEW_PRIVS,
            PR_GET_NO_NEW_PRIVS,
            PR_SET_THP_DISABLE,
            PR_GET_THP_DISABLE,
            PR_GET_SPECULATION_CTRL,
            PR_SET_SPECULATION_CTRL,
            PR_SET_MEMORY_MERGE,
            PR_GET_MEMORY_MERGE,
        ];
        for i in 0..opts.len() {
            for j in (i + 1)..opts.len() {
                assert_ne!(opts[i], opts[j]);
            }
        }
    }

    #[test]
    fn test_spec_values_no_overlap() {
        let vals = [
            PR_SPEC_PRCTL,
            PR_SPEC_ENABLE,
            PR_SPEC_DISABLE,
            PR_SPEC_FORCE_DISABLE,
            PR_SPEC_DISABLE_NOEXEC,
        ];
        for i in 0..vals.len() {
            assert!(vals[i].is_power_of_two());
            for j in (i + 1)..vals.len() {
                assert_eq!(vals[i] & vals[j], 0);
            }
        }
    }

    #[test]
    fn test_spec_commands_distinct() {
        let cmds = [
            PR_SPEC_STORE_BYPASS,
            PR_SPEC_INDIRECT_BRANCH,
            PR_SPEC_L1D_FLUSH,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_dumpable_values_distinct() {
        let vals = [SUID_DUMP_DISABLE, SUID_DUMP_USER, SUID_DUMP_ROOT];
        for i in 0..vals.len() {
            for j in (i + 1)..vals.len() {
                assert_ne!(vals[i], vals[j]);
            }
        }
    }
}
