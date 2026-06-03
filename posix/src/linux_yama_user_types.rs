//! `<linux/yama.h>` — Yama LSM (ptrace scope restrictions).
//!
//! Yama is a minor LSM that locks down `ptrace(2)`. Distros (Ubuntu,
//! Debian, Arch) enable it by default with `ptrace_scope=1` so a
//! process can only trace its descendants. Userspace exposes the knob
//! via `/proc/sys/kernel/yama/ptrace_scope` and via `prctl(2)` with
//! `PR_SET_PTRACER`.

// ---------------------------------------------------------------------------
// Sysctl path
// ---------------------------------------------------------------------------

pub const SYSCTL_PTRACE_SCOPE: &str = "/proc/sys/kernel/yama/ptrace_scope";

// ---------------------------------------------------------------------------
// `ptrace_scope` levels (kernel/yama/yama.c)
// ---------------------------------------------------------------------------

/// Classic permissive behavior — any process with the same UID can attach.
pub const YAMA_SCOPE_CLASSIC: u32 = 0;

/// Default on most distros — only descendants (or processes that called
/// `prctl(PR_SET_PTRACER, pid)` to grant access) can attach.
pub const YAMA_SCOPE_RESTRICTED: u32 = 1;

/// Admin-only — `CAP_SYS_PTRACE` required to ptrace anything.
pub const YAMA_SCOPE_ADMIN_ONLY: u32 = 2;

/// `ptrace(2)` is completely disabled. This is a one-way switch — once
/// written, the kernel refuses to lower it back without reboot.
pub const YAMA_SCOPE_NO_ATTACH: u32 = 3;

pub const YAMA_SCOPE_MIN: u32 = YAMA_SCOPE_CLASSIC;
pub const YAMA_SCOPE_MAX: u32 = YAMA_SCOPE_NO_ATTACH;

// ---------------------------------------------------------------------------
// `prctl(2)` options used by Yama (also in `<sys/prctl.h>`)
// ---------------------------------------------------------------------------

pub const PR_SET_PTRACER: u32 = 0x5961_7461;

/// Sentinel passed as the pid argument to allow *any* process to attach.
pub const PR_SET_PTRACER_ANY: i64 = -1;

// ---------------------------------------------------------------------------
// LSM name as exposed by `/sys/kernel/security/lsm`
// ---------------------------------------------------------------------------

pub const YAMA_LSM_NAME: &str = "yama";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sysctl_path_under_kernel_yama() {
        assert_eq!(
            SYSCTL_PTRACE_SCOPE,
            "/proc/sys/kernel/yama/ptrace_scope"
        );
        assert!(SYSCTL_PTRACE_SCOPE.starts_with("/proc/sys/kernel/yama/"));
    }

    #[test]
    fn test_scope_levels_dense_0_to_3() {
        let s = [
            YAMA_SCOPE_CLASSIC,
            YAMA_SCOPE_RESTRICTED,
            YAMA_SCOPE_ADMIN_ONLY,
            YAMA_SCOPE_NO_ATTACH,
        ];
        for (i, &v) in s.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        assert_eq!(YAMA_SCOPE_MIN, 0);
        assert_eq!(YAMA_SCOPE_MAX, 3);
        assert!(YAMA_SCOPE_MAX > YAMA_SCOPE_MIN);
    }

    #[test]
    fn test_pr_set_ptracer_constant_and_any() {
        // PR_SET_PTRACER = 0x59617461 = ascii 'Y','a','t','a' rearranged —
        // documented in include/uapi/linux/prctl.h.
        assert_eq!(PR_SET_PTRACER, 0x5961_7461);
        // The "any" sentinel is -1 (kernel checks for negative pid).
        assert_eq!(PR_SET_PTRACER_ANY, -1);
    }

    #[test]
    fn test_lsm_name_matches_kernel() {
        assert_eq!(YAMA_LSM_NAME, "yama");
    }
}
