//! `<linux/memfd.h>` — `memfd_create()` flags.
//!
//! Re-exports `memfd_create()` from `mman` and defines the `MFD_*`
//! flag constants used with it.

// ---------------------------------------------------------------------------
// Re-export memfd_create
// ---------------------------------------------------------------------------

pub use crate::mman::memfd_create;

// ---------------------------------------------------------------------------
// MFD_* flags
// ---------------------------------------------------------------------------

/// Set close-on-exec on the new fd.
pub const MFD_CLOEXEC: u32 = 0x0001;
/// Allow sealing operations (fcntl F_ADD_SEALS).
pub const MFD_ALLOW_SEALING: u32 = 0x0002;
/// Create memfd in hugetlb filesystem.
pub const MFD_HUGETLB: u32 = 0x0004;
/// Don't allow exec (W^X enforcement).
pub const MFD_NOEXEC_SEAL: u32 = 0x0008;
/// Allow exec mapping.
pub const MFD_EXEC: u32 = 0x0010;

// ---------------------------------------------------------------------------
// Huge page size encoding (used with MFD_HUGETLB)
// ---------------------------------------------------------------------------

/// Shift for encoding huge page size in flags.
pub const MFD_HUGE_SHIFT: u32 = 26;
/// Mask for huge page size bits.
pub const MFD_HUGE_MASK: u32 = 0x3F;

/// 64 KiB huge pages.
pub const MFD_HUGE_64KB: u32 = 16 << MFD_HUGE_SHIFT;
/// 512 KiB huge pages.
pub const MFD_HUGE_512KB: u32 = 19 << MFD_HUGE_SHIFT;
/// 1 MiB huge pages.
pub const MFD_HUGE_1MB: u32 = 20 << MFD_HUGE_SHIFT;
/// 2 MiB huge pages.
pub const MFD_HUGE_2MB: u32 = 21 << MFD_HUGE_SHIFT;
/// 8 MiB huge pages.
pub const MFD_HUGE_8MB: u32 = 23 << MFD_HUGE_SHIFT;
/// 16 MiB huge pages.
pub const MFD_HUGE_16MB: u32 = 24 << MFD_HUGE_SHIFT;
/// 32 MiB huge pages.
pub const MFD_HUGE_32MB: u32 = 25 << MFD_HUGE_SHIFT;
/// 256 MiB huge pages.
pub const MFD_HUGE_256MB: u32 = 28 << MFD_HUGE_SHIFT;
/// 512 MiB huge pages.
pub const MFD_HUGE_512MB: u32 = 29 << MFD_HUGE_SHIFT;
/// 1 GiB huge pages.
pub const MFD_HUGE_1GB: u32 = 30 << MFD_HUGE_SHIFT;
/// 2 GiB huge pages.
pub const MFD_HUGE_2GB: u32 = 31 << MFD_HUGE_SHIFT;
/// 16 GiB huge pages.
pub const MFD_HUGE_16GB: u32 = 34 << MFD_HUGE_SHIFT;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mfd_flags_values() {
        assert_eq!(MFD_CLOEXEC, 0x0001);
        assert_eq!(MFD_ALLOW_SEALING, 0x0002);
        assert_eq!(MFD_HUGETLB, 0x0004);
        assert_eq!(MFD_NOEXEC_SEAL, 0x0008);
        assert_eq!(MFD_EXEC, 0x0010);
    }

    #[test]
    fn test_mfd_flags_distinct() {
        let flags = [
            MFD_CLOEXEC,
            MFD_ALLOW_SEALING,
            MFD_HUGETLB,
            MFD_NOEXEC_SEAL,
            MFD_EXEC,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_mfd_flags_are_powers_of_two() {
        let flags = [
            MFD_CLOEXEC,
            MFD_ALLOW_SEALING,
            MFD_HUGETLB,
            MFD_NOEXEC_SEAL,
            MFD_EXEC,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "MFD flag {f:#x} is not power of 2");
        }
    }

    #[test]
    fn test_huge_page_encoding() {
        assert_eq!(MFD_HUGE_SHIFT, 26);
        // 2 MiB = order 21
        assert_eq!(MFD_HUGE_2MB >> MFD_HUGE_SHIFT, 21);
        // 1 GiB = order 30
        assert_eq!(MFD_HUGE_1GB >> MFD_HUGE_SHIFT, 30);
    }

    #[test]
    fn test_memfd_create_basic() {
        // memfd_create with no flags should succeed and yield a positive fd.
        // (Underlying open() may fail in host-target test runs without a
        // real /dev/shm — accept either success or a real errno that is
        // *not* ENOSYS.)
        let ret = memfd_create(b"test\0".as_ptr(), 0);
        if ret < 0 {
            let e = crate::errno::get_errno();
            assert_ne!(
                e,
                crate::errno::ENOSYS,
                "memfd_create must not return ENOSYS — it is implemented"
            );
        }
        if ret >= 0 {
            let _ = crate::file::close(ret);
        }
    }

    #[test]
    fn test_memfd_create_null_name_efault() {
        let ret = memfd_create(core::ptr::null(), 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_memfd_create_unknown_flag_einval() {
        let ret = memfd_create(b"x\0".as_ptr(), 0x8000_0000);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_memfd_create_hugetlb_einval() {
        let ret = memfd_create(b"x\0".as_ptr(), MFD_HUGETLB);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_memfd_create_slash_in_name_einval() {
        let ret = memfd_create(b"a/b\0".as_ptr(), 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_memfd_create_name_too_long_einval() {
        let long_name = [b'a'; 250];
        // Build a NUL-terminated copy.
        let mut buf = [0u8; 256];
        for (i, c) in long_name.iter().enumerate() {
            if let Some(slot) = buf.get_mut(i) {
                *slot = *c;
            }
        }
        let ret = memfd_create(buf.as_ptr(), 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_memfd_create_accepts_known_flags() {
        // CLOEXEC, ALLOW_SEALING, NOEXEC_SEAL, and EXEC must be accepted
        // (the call may still fail at open() in a host-target test run,
        // but it must not fail with EINVAL on flag validation).
        for &fl in &[MFD_CLOEXEC, MFD_ALLOW_SEALING, MFD_NOEXEC_SEAL, MFD_EXEC] {
            let _ = crate::errno::set_errno(0);
            let ret = memfd_create(b"x\0".as_ptr(), fl);
            if ret < 0 {
                let e = crate::errno::get_errno();
                assert_ne!(e, crate::errno::EINVAL, "flag {fl:#x} should not be EINVAL");
            } else {
                let _ = crate::file::close(ret);
            }
        }
    }

    #[test]
    fn test_huge_sizes_distinct() {
        let sizes = [
            MFD_HUGE_64KB,
            MFD_HUGE_512KB,
            MFD_HUGE_1MB,
            MFD_HUGE_2MB,
            MFD_HUGE_8MB,
            MFD_HUGE_16MB,
            MFD_HUGE_32MB,
            MFD_HUGE_256MB,
            MFD_HUGE_512MB,
            MFD_HUGE_1GB,
            MFD_HUGE_2GB,
            MFD_HUGE_16GB,
        ];
        for i in 0..sizes.len() {
            for j in (i + 1)..sizes.len() {
                assert_ne!(sizes[i], sizes[j]);
            }
        }
    }
}
