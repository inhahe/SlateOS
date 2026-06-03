//! `<sys/prctl.h>` — `prctl(2)` operation codes.
//!
//! `prctl` is a grab-bag syscall for per-process knobs: process name,
//! parent-death signal, dumpable flag, capability bounding set, seccomp
//! mode, no-new-privs, THP toggle, and the architecture-specific bits
//! used by JITs. The numbers here are the stable subset that libc,
//! systemd, and language runtimes all touch.

// ---------------------------------------------------------------------------
// Core ops (process-attribute manipulation)
// ---------------------------------------------------------------------------

pub const PR_SET_PDEATHSIG: u32 = 1;
pub const PR_GET_PDEATHSIG: u32 = 2;
pub const PR_GET_DUMPABLE: u32 = 3;
pub const PR_SET_DUMPABLE: u32 = 4;
pub const PR_GET_UNALIGN: u32 = 5;
pub const PR_SET_UNALIGN: u32 = 6;
pub const PR_GET_KEEPCAPS: u32 = 7;
pub const PR_SET_KEEPCAPS: u32 = 8;
pub const PR_GET_FPEMU: u32 = 9;
pub const PR_SET_FPEMU: u32 = 10;
pub const PR_GET_FPEXC: u32 = 11;
pub const PR_SET_FPEXC: u32 = 12;
pub const PR_GET_TIMING: u32 = 13;
pub const PR_SET_TIMING: u32 = 14;
pub const PR_SET_NAME: u32 = 15;
pub const PR_GET_NAME: u32 = 16;

// ---------------------------------------------------------------------------
// Capability bounding set ops
// ---------------------------------------------------------------------------

pub const PR_CAPBSET_READ: u32 = 23;
pub const PR_CAPBSET_DROP: u32 = 24;

// ---------------------------------------------------------------------------
// securebits / ambient caps / per-task secret
// ---------------------------------------------------------------------------

pub const PR_GET_SECUREBITS: u32 = 27;
pub const PR_SET_SECUREBITS: u32 = 28;
pub const PR_SET_TIMERSLACK: u32 = 29;
pub const PR_GET_TIMERSLACK: u32 = 30;

// ---------------------------------------------------------------------------
// Seccomp / no-new-privs
// ---------------------------------------------------------------------------

pub const PR_GET_SECCOMP: u32 = 21;
pub const PR_SET_SECCOMP: u32 = 22;
pub const PR_SET_NO_NEW_PRIVS: u32 = 38;
pub const PR_GET_NO_NEW_PRIVS: u32 = 39;

// ---------------------------------------------------------------------------
// THP, child subreaper, ptracer, mm tunables
// ---------------------------------------------------------------------------

pub const PR_SET_THP_DISABLE: u32 = 41;
pub const PR_GET_THP_DISABLE: u32 = 42;
pub const PR_MPX_ENABLE_MANAGEMENT: u32 = 43;
pub const PR_MPX_DISABLE_MANAGEMENT: u32 = 44;
pub const PR_SET_CHILD_SUBREAPER: u32 = 36;
pub const PR_GET_CHILD_SUBREAPER: u32 = 37;
pub const PR_SET_PTRACER: u32 = 0x59616D61;
pub const PR_SET_PTRACER_ANY: u32 = u32::MAX;

// ---------------------------------------------------------------------------
// Ambient capabilities (`PR_CAP_AMBIENT`)
// ---------------------------------------------------------------------------

pub const PR_CAP_AMBIENT: u32 = 47;
pub const PR_CAP_AMBIENT_IS_SET: u32 = 1;
pub const PR_CAP_AMBIENT_RAISE: u32 = 2;
pub const PR_CAP_AMBIENT_LOWER: u32 = 3;
pub const PR_CAP_AMBIENT_CLEAR_ALL: u32 = 4;

// ---------------------------------------------------------------------------
// `SET_DUMPABLE` values
// ---------------------------------------------------------------------------

pub const SUID_DUMP_DISABLE: u32 = 0;
pub const SUID_DUMP_USER: u32 = 1;
pub const SUID_DUMP_ROOT: u32 = 2;

// ---------------------------------------------------------------------------
// `TASK_COMM_LEN` — name buffer size for `PR_SET_NAME` / `PR_GET_NAME`
// ---------------------------------------------------------------------------

pub const TASK_COMM_LEN: usize = 16;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_set_pairs_consecutive() {
        // Most prctl ops come in GET/SET pairs at consecutive numbers.
        assert_eq!(PR_GET_PDEATHSIG, PR_SET_PDEATHSIG + 1);
        // Note: GET_DUMPABLE=3 / SET_DUMPABLE=4 (GET first).
        assert_eq!(PR_SET_DUMPABLE, PR_GET_DUMPABLE + 1);
        assert_eq!(PR_GET_KEEPCAPS, PR_SET_KEEPCAPS - 1);
        assert_eq!(PR_GET_NAME, PR_SET_NAME + 1);
        assert_eq!(PR_GET_SECCOMP, PR_SET_SECCOMP - 1);
        assert_eq!(PR_GET_SECUREBITS, PR_SET_SECUREBITS - 1);
        assert_eq!(PR_GET_THP_DISABLE, PR_SET_THP_DISABLE + 1);
        assert_eq!(PR_GET_NO_NEW_PRIVS, PR_SET_NO_NEW_PRIVS + 1);
        assert_eq!(PR_GET_CHILD_SUBREAPER, PR_SET_CHILD_SUBREAPER + 1);
    }

    #[test]
    fn test_pr_set_name_anchor_and_buffer() {
        // The PR_SET_NAME number (15) is one of the most-referenced
        // prctl op codes in libc — pin it.
        assert_eq!(PR_SET_NAME, 15);
        // The corresponding name buffer holds 15 bytes + NUL.
        assert_eq!(TASK_COMM_LEN, 16);
    }

    #[test]
    fn test_no_new_privs_op_number() {
        // PR_SET_NO_NEW_PRIVS is the foundation seccomp filter primitive.
        assert_eq!(PR_SET_NO_NEW_PRIVS, 38);
    }

    #[test]
    fn test_capbset_ops_distinct_known() {
        assert_eq!(PR_CAPBSET_READ, 23);
        assert_eq!(PR_CAPBSET_DROP, 24);
    }

    #[test]
    fn test_ambient_subops_dense_1_to_4() {
        let a = [
            PR_CAP_AMBIENT_IS_SET,
            PR_CAP_AMBIENT_RAISE,
            PR_CAP_AMBIENT_LOWER,
            PR_CAP_AMBIENT_CLEAR_ALL,
        ];
        for (i, &v) in a.iter().enumerate() {
            assert_eq!(v as usize, i + 1);
        }
        assert_eq!(PR_CAP_AMBIENT, 47);
    }

    #[test]
    fn test_suid_dump_dense_0_to_2() {
        assert_eq!(SUID_DUMP_DISABLE, 0);
        assert_eq!(SUID_DUMP_USER, 1);
        assert_eq!(SUID_DUMP_ROOT, 2);
    }

    #[test]
    fn test_ptracer_magic() {
        // PR_SET_PTRACER's op-number is the ASCII "Yama" magic — picked
        // so it doesn't collide with the dense op range.
        assert_eq!(PR_SET_PTRACER, 0x59616D61);
        assert_eq!(PR_SET_PTRACER_ANY, u32::MAX);
    }
}
