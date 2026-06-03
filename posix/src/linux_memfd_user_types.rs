//! `<linux/memfd.h>` — `memfd_create(2)` flags.
//!
//! `memfd_create` returns an anonymous file descriptor backed by tmpfs.
//! Wayland compositors use it for shared surfaces, glibc and musl use
//! it as the storage for `shm_open` (replacing `/dev/shm` files), and
//! dynamic linkers use sealed memfds to load anonymous code.

// ---------------------------------------------------------------------------
// `memfd_create(2)` flags
// ---------------------------------------------------------------------------

/// Close-on-exec.
pub const MFD_CLOEXEC: u32 = 0x0001;
/// Allow `F_ADD_SEALS` / `F_GET_SEALS` via `fcntl`.
pub const MFD_ALLOW_SEALING: u32 = 0x0002;
/// Back the memfd with hugetlbfs.
pub const MFD_HUGETLB: u32 = 0x0004;
/// Don't allow execute permission.
pub const MFD_NOEXEC_SEAL: u32 = 0x0008;
/// Allow execute permission (default before 6.3).
pub const MFD_EXEC: u32 = 0x0010;

// ---------------------------------------------------------------------------
// Hugetlb size encoding (shared with `mmap(2)` MAP_HUGE_*).
// ---------------------------------------------------------------------------

/// Bit position of the log2(page-size) field in `flags`.
pub const MFD_HUGE_SHIFT: u32 = 26;
/// Mask for the hugetlb log2 page-size field.
pub const MFD_HUGE_MASK: u32 = 0x3F;

pub const MFD_HUGE_64KB: u32 = 16 << MFD_HUGE_SHIFT;
pub const MFD_HUGE_512KB: u32 = 19 << MFD_HUGE_SHIFT;
pub const MFD_HUGE_1MB: u32 = 20 << MFD_HUGE_SHIFT;
pub const MFD_HUGE_2MB: u32 = 21 << MFD_HUGE_SHIFT;
pub const MFD_HUGE_8MB: u32 = 23 << MFD_HUGE_SHIFT;
pub const MFD_HUGE_16MB: u32 = 24 << MFD_HUGE_SHIFT;
pub const MFD_HUGE_32MB: u32 = 25 << MFD_HUGE_SHIFT;
pub const MFD_HUGE_256MB: u32 = 28 << MFD_HUGE_SHIFT;
pub const MFD_HUGE_512MB: u32 = 29 << MFD_HUGE_SHIFT;
pub const MFD_HUGE_1GB: u32 = 30 << MFD_HUGE_SHIFT;
pub const MFD_HUGE_2GB: u32 = 31 << MFD_HUGE_SHIFT;
pub const MFD_HUGE_16GB: u32 = 34 << MFD_HUGE_SHIFT;

// ---------------------------------------------------------------------------
// File seals (from `<linux/fcntl.h>` but conventionally listed with memfd).
// ---------------------------------------------------------------------------

pub const F_SEAL_SEAL: u32 = 0x0001;
pub const F_SEAL_SHRINK: u32 = 0x0002;
pub const F_SEAL_GROW: u32 = 0x0004;
pub const F_SEAL_WRITE: u32 = 0x0008;
pub const F_SEAL_FUTURE_WRITE: u32 = 0x0010;
pub const F_SEAL_EXEC: u32 = 0x0020;

// ---------------------------------------------------------------------------
// Syscall numbers
// ---------------------------------------------------------------------------

/// `__NR_memfd_create` on x86_64.
pub const NR_MEMFD_CREATE: u32 = 319;
/// `__NR_memfd_secret` on x86_64 (Linux 5.14+).
pub const NR_MEMFD_SECRET: u32 = 447;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flags_are_single_bits() {
        for f in [
            MFD_CLOEXEC,
            MFD_ALLOW_SEALING,
            MFD_HUGETLB,
            MFD_NOEXEC_SEAL,
            MFD_EXEC,
        ] {
            assert!(f.is_power_of_two());
        }
    }

    #[test]
    fn test_flags_dense_low_nibble() {
        // Low five bits (0x01..0x10) are densely packed for the create flags.
        assert_eq!(
            MFD_CLOEXEC | MFD_ALLOW_SEALING | MFD_HUGETLB | MFD_NOEXEC_SEAL | MFD_EXEC,
            0x1F
        );
    }

    #[test]
    fn test_hugetlb_field_layout() {
        // Hugetlb size sits in bits 26..32 — far above the flag bits.
        assert_eq!(MFD_HUGE_SHIFT, 26);
        assert_eq!(MFD_HUGE_MASK, 0x3F);
        // 2 MB = log2(2^21) -> field value 21.
        assert_eq!(MFD_HUGE_2MB >> MFD_HUGE_SHIFT, 21);
        assert_eq!(MFD_HUGE_1GB >> MFD_HUGE_SHIFT, 30);
        // Flag bits must not collide with the size field.
        let all_flags =
            MFD_CLOEXEC | MFD_ALLOW_SEALING | MFD_HUGETLB | MFD_NOEXEC_SEAL | MFD_EXEC;
        assert_eq!(all_flags & (MFD_HUGE_MASK << MFD_HUGE_SHIFT), 0);
    }

    #[test]
    fn test_seal_bits_dense_and_single_bit() {
        let s = [
            F_SEAL_SEAL,
            F_SEAL_SHRINK,
            F_SEAL_GROW,
            F_SEAL_WRITE,
            F_SEAL_FUTURE_WRITE,
            F_SEAL_EXEC,
        ];
        for v in s {
            assert!(v.is_power_of_two());
        }
        // OR of all seal bits == 0x3F (six dense bits).
        let or = s.iter().fold(0, |a, b| a | b);
        assert_eq!(or, 0x3F);
    }

    #[test]
    fn test_syscall_numbers() {
        assert_eq!(NR_MEMFD_CREATE, 319);
        assert_eq!(NR_MEMFD_SECRET, 447);
        assert!(NR_MEMFD_SECRET > NR_MEMFD_CREATE);
    }
}
