//! `<linux/openat2.h>` — `openat2(2)` with path-resolution restrictions.
//!
//! `openat2` (Linux 5.6+) extends `openat` with the `struct open_how`
//! argument and a set of `RESOLVE_*` flags that constrain symlink and
//! mount traversal. Container runtimes and sandboxed file servers
//! lean on these flags to refuse symlink-escape attacks at the
//! syscall boundary instead of trying to re-validate paths in
//! userspace.

// ---------------------------------------------------------------------------
// `open_how.resolve` bitmask (`RESOLVE_*`)
// ---------------------------------------------------------------------------

/// Don't follow any symlinks during path resolution.
pub const RESOLVE_NO_SYMLINKS: u64 = 0x04;
/// Refuse to cross mount points.
pub const RESOLVE_NO_XDEV: u64 = 0x01;
/// Refuse magic-link traversal (e.g. `/proc/<pid>/fd/N`).
pub const RESOLVE_NO_MAGICLINKS: u64 = 0x02;
/// All path components must resolve under `dirfd` — no `..`/absolute.
pub const RESOLVE_BENEATH: u64 = 0x08;
/// Treat `dirfd` as the root for absolute paths and `..`.
pub const RESOLVE_IN_ROOT: u64 = 0x10;
/// Force-cache mode: fail rather than block on I/O for path lookup.
pub const RESOLVE_CACHED: u64 = 0x20;

/// Union of every public `RESOLVE_*` flag — anything outside this is
/// reserved and userspace passing such a bit gets `EINVAL`.
pub const RESOLVE_ALL: u64 = RESOLVE_NO_XDEV
    | RESOLVE_NO_MAGICLINKS
    | RESOLVE_NO_SYMLINKS
    | RESOLVE_BENEATH
    | RESOLVE_IN_ROOT
    | RESOLVE_CACHED;

// ---------------------------------------------------------------------------
// `struct open_how` size — versioning anchor
// ---------------------------------------------------------------------------
//
// The kernel reads `min(usize, sizeof(open_how))` so userspace can
// extend the struct over time. These are the documented sizes for
// the two versions shipped so far.

/// Initial layout: `flags`, `mode`, `resolve` — three u64 fields.
pub const OPEN_HOW_SIZE_VER0: usize = 24;
/// Latest stable layout shipped to date matches VER0 — kept as a
/// forward-looking alias.
pub const OPEN_HOW_SIZE_LATEST: usize = OPEN_HOW_SIZE_VER0;

// ---------------------------------------------------------------------------
// Syscall numbers (x86_64)
// ---------------------------------------------------------------------------

pub const NR_OPENAT2: u32 = 437;
/// AT_FDCWD — the magic dirfd that means "current working directory".
pub const AT_FDCWD: i32 = -100;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_flags_single_bit() {
        let r = [
            RESOLVE_NO_XDEV,
            RESOLVE_NO_MAGICLINKS,
            RESOLVE_NO_SYMLINKS,
            RESOLVE_BENEATH,
            RESOLVE_IN_ROOT,
            RESOLVE_CACHED,
        ];
        for v in r {
            assert!(v.is_power_of_two());
        }
        // They fill the low six bits 0x01..0x20 exactly.
        let mut or = 0u64;
        for v in r {
            or |= v;
        }
        assert_eq!(or, 0x3F);
    }

    #[test]
    fn test_resolve_flags_distinct() {
        let r = [
            RESOLVE_NO_XDEV,
            RESOLVE_NO_MAGICLINKS,
            RESOLVE_NO_SYMLINKS,
            RESOLVE_BENEATH,
            RESOLVE_IN_ROOT,
            RESOLVE_CACHED,
        ];
        for i in 0..r.len() {
            for j in (i + 1)..r.len() {
                assert_ne!(r[i], r[j]);
            }
        }
    }

    #[test]
    fn test_resolve_all_is_or_of_all_flags() {
        assert_eq!(RESOLVE_ALL, 0x3F);
        assert_eq!(
            RESOLVE_ALL,
            RESOLVE_NO_XDEV
                | RESOLVE_NO_MAGICLINKS
                | RESOLVE_NO_SYMLINKS
                | RESOLVE_BENEATH
                | RESOLVE_IN_ROOT
                | RESOLVE_CACHED
        );
    }

    #[test]
    fn test_open_how_layout_sizes() {
        // 3 × u64 = 24 bytes. If we ever grow the struct, VER0 stays
        // pinned at 24 for backward compatibility.
        assert_eq!(OPEN_HOW_SIZE_VER0, 24);
        assert_eq!(OPEN_HOW_SIZE_LATEST, OPEN_HOW_SIZE_VER0);
    }

    #[test]
    fn test_syscall_and_atfdcwd() {
        // openat2 was added in 5.6 as syscall 437.
        assert_eq!(NR_OPENAT2, 437);
        assert_eq!(AT_FDCWD, -100);
    }
}
