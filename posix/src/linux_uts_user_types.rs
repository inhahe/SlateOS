//! `<sys/utsname.h>` — `uname(2)` ABI and UTS namespaces.
//!
//! `struct utsname` is the fixed-layout block `uname(2)` fills in.
//! Each field is a 65-byte (`UTSNAME_LEN`) NUL-terminated string.
//! UTS namespaces (Linux 2.6.19+) virtualise the hostname and domain
//! per-container so each can have its own `/proc/sys/kernel/hostname`.

// ---------------------------------------------------------------------------
// `struct utsname` field length
// ---------------------------------------------------------------------------

/// `_UTSNAME_LENGTH` — Linux uses 65 (64 chars + NUL) for every field.
pub const UTSNAME_LEN: usize = 65;

/// `HOST_NAME_MAX` per POSIX — payload only, no NUL.
pub const HOST_NAME_MAX: usize = 64;

/// Field offsets inside `struct utsname` (5 contiguous 65-byte blocks).
pub const UTSNAME_OFF_SYSNAME: usize = 0;
pub const UTSNAME_OFF_NODENAME: usize = UTSNAME_LEN;
pub const UTSNAME_OFF_RELEASE: usize = 2 * UTSNAME_LEN;
pub const UTSNAME_OFF_VERSION: usize = 3 * UTSNAME_LEN;
pub const UTSNAME_OFF_MACHINE: usize = 4 * UTSNAME_LEN;
/// `domainname` (sixth field, Linux-extension).
pub const UTSNAME_OFF_DOMAINNAME: usize = 5 * UTSNAME_LEN;

/// Total size of `struct utsname` on Linux.
pub const UTSNAME_SIZE: usize = 6 * UTSNAME_LEN;

// ---------------------------------------------------------------------------
// `clone(2)` / `unshare(2)` flag for UTS namespace
// ---------------------------------------------------------------------------

pub const CLONE_NEWUTS: u32 = 0x0400_0000;

// ---------------------------------------------------------------------------
// /proc paths exposing the UTS fields
// ---------------------------------------------------------------------------

pub const PROC_SYS_KERNEL_HOSTNAME: &str = "/proc/sys/kernel/hostname";
pub const PROC_SYS_KERNEL_DOMAINNAME: &str = "/proc/sys/kernel/domainname";
pub const PROC_SYS_KERNEL_OSTYPE: &str = "/proc/sys/kernel/ostype";
pub const PROC_SYS_KERNEL_OSRELEASE: &str = "/proc/sys/kernel/osrelease";
pub const PROC_SYS_KERNEL_VERSION: &str = "/proc/sys/kernel/version";

pub const ETC_HOSTNAME: &str = "/etc/hostname";

// ---------------------------------------------------------------------------
// Per-process UTS namespace file
// ---------------------------------------------------------------------------

pub const PROC_SELF_NS_UTS: &str = "/proc/self/ns/uts";

// ---------------------------------------------------------------------------
// Syscalls
// ---------------------------------------------------------------------------

pub const NR_UNAME: u32 = 63;
pub const NR_SETHOSTNAME: u32 = 170;
pub const NR_SETDOMAINNAME: u32 = 171;

// ---------------------------------------------------------------------------
// Conventional `machine` field values
// ---------------------------------------------------------------------------

pub const MACHINE_X86_64: &str = "x86_64";
pub const MACHINE_I686: &str = "i686";
pub const MACHINE_AARCH64: &str = "aarch64";
pub const MACHINE_ARMV7L: &str = "armv7l";
pub const MACHINE_RISCV64: &str = "riscv64";
pub const MACHINE_PPC64LE: &str = "ppc64le";

pub const SYSNAME_LINUX: &str = "Linux";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_utsname_layout() {
        // Six contiguous 65-byte fields.
        assert_eq!(UTSNAME_LEN, 65);
        // POSIX HOST_NAME_MAX = 64 (one less than the C buffer slot
        // because the trailing NUL is in the 65th byte).
        assert_eq!(HOST_NAME_MAX, UTSNAME_LEN - 1);
        assert_eq!(UTSNAME_SIZE, 6 * 65);
    }

    #[test]
    fn test_field_offsets_dense() {
        let o = [
            UTSNAME_OFF_SYSNAME,
            UTSNAME_OFF_NODENAME,
            UTSNAME_OFF_RELEASE,
            UTSNAME_OFF_VERSION,
            UTSNAME_OFF_MACHINE,
            UTSNAME_OFF_DOMAINNAME,
        ];
        for (i, &v) in o.iter().enumerate() {
            assert_eq!(v, i * UTSNAME_LEN);
        }
        // The last offset + one field fills the struct exactly.
        assert_eq!(UTSNAME_OFF_DOMAINNAME + UTSNAME_LEN, UTSNAME_SIZE);
    }

    #[test]
    fn test_clone_newuts_value() {
        // CLONE_NEWUTS is bit 26 (0x04000000) — shared with all the
        // other namespace flags in the top byte.
        assert_eq!(CLONE_NEWUTS, 1 << 26);
    }

    #[test]
    fn test_proc_paths_under_sys_kernel() {
        let p = [
            PROC_SYS_KERNEL_HOSTNAME,
            PROC_SYS_KERNEL_DOMAINNAME,
            PROC_SYS_KERNEL_OSTYPE,
            PROC_SYS_KERNEL_OSRELEASE,
            PROC_SYS_KERNEL_VERSION,
        ];
        for path in p {
            assert!(path.starts_with("/proc/sys/kernel/"));
        }
        assert_eq!(ETC_HOSTNAME, "/etc/hostname");
        assert_eq!(PROC_SELF_NS_UTS, "/proc/self/ns/uts");
    }

    #[test]
    fn test_syscall_numbers_x86_64() {
        assert_eq!(NR_UNAME, 63);
        // sethostname / setdomainname adjacent.
        assert_eq!(NR_SETHOSTNAME, 170);
        assert_eq!(NR_SETDOMAINNAME, NR_SETHOSTNAME + 1);
    }

    #[test]
    fn test_machine_strings_distinct() {
        let m = [
            MACHINE_X86_64,
            MACHINE_I686,
            MACHINE_AARCH64,
            MACHINE_ARMV7L,
            MACHINE_RISCV64,
            MACHINE_PPC64LE,
        ];
        for a in 0..m.len() {
            for b in (a + 1)..m.len() {
                assert_ne!(m[a], m[b]);
            }
        }
        assert_eq!(SYSNAME_LINUX, "Linux");
    }
}
