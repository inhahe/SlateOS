//! `/proc/<pid>/maps` flags and address-space layout constants.
//!
//! Linux exposes a process's address space through `/proc/<pid>/maps`,
//! `/proc/<pid>/smaps`, and `/proc/<pid>/pagemap`. Userspace tools
//! (`pmap`, `cat /proc/self/maps`, debuggers, AddressSanitizer) parse
//! the textual maps format whose constants are codified here.

// ---------------------------------------------------------------------------
// `/proc/<pid>/maps` line format — fixed column anchors
// ---------------------------------------------------------------------------

/// Permission string is 4 characters: `r/-`, `w/-`, `x/-`, `p/s`.
pub const MAPS_PERMS_LEN: usize = 4;

pub const MAPS_PERM_READ_CHAR: u8 = b'r';
pub const MAPS_PERM_WRITE_CHAR: u8 = b'w';
pub const MAPS_PERM_EXEC_CHAR: u8 = b'x';
pub const MAPS_PERM_PRIVATE_CHAR: u8 = b'p';
pub const MAPS_PERM_SHARED_CHAR: u8 = b's';
pub const MAPS_PERM_ABSENT_CHAR: u8 = b'-';

// ---------------------------------------------------------------------------
// Permission bitfield used internally (matches `VM_*`)
// ---------------------------------------------------------------------------

pub const MAPS_PERM_READ: u32 = 0x01;
pub const MAPS_PERM_WRITE: u32 = 0x02;
pub const MAPS_PERM_EXEC: u32 = 0x04;
pub const MAPS_PERM_SHARED: u32 = 0x08;

// ---------------------------------------------------------------------------
// Well-known pseudo-paths in the last column of `/proc/<pid>/maps`
// ---------------------------------------------------------------------------

pub const MAPS_PATH_HEAP: &str = "[heap]";
pub const MAPS_PATH_STACK: &str = "[stack]";
pub const MAPS_PATH_VDSO: &str = "[vdso]";
pub const MAPS_PATH_VVAR: &str = "[vvar]";
pub const MAPS_PATH_VSYSCALL: &str = "[vsyscall]";
pub const MAPS_PATH_ANON: &str = "[anon]";
pub const MAPS_PATH_UPROBES: &str = "[uprobes]";

// ---------------------------------------------------------------------------
// Standard layout constants on x86_64 Linux
// ---------------------------------------------------------------------------

/// PMD_SIZE on a 4 KiB-page x86_64 kernel — 2 MiB.
pub const PMD_SIZE_X86_64_4K: usize = 2 * 1024 * 1024;

/// PUD_SIZE on a 4 KiB-page x86_64 kernel — 1 GiB.
pub const PUD_SIZE_X86_64_4K: usize = 1024 * 1024 * 1024;

/// Classic 48-bit user-space ceiling: 0x0000_7FFF_FFFF_FFFF.
pub const X86_64_USER_VA_TOP_48BIT: u64 = 0x0000_7FFF_FFFF_FFFF;

/// 5-level paging user-space ceiling: 0x00FF_FFFF_FFFF_FFFF.
pub const X86_64_USER_VA_TOP_57BIT: u64 = 0x00FF_FFFF_FFFF_FFFF;

/// MMAP_RND_BITS_MIN on x86_64 — 28 bits of mmap ASLR.
pub const MMAP_RND_BITS_MIN_X86_64: u32 = 28;

// ---------------------------------------------------------------------------
// `/proc/<pid>/pagemap` entry layout
// ---------------------------------------------------------------------------

pub const PAGEMAP_ENTRY_SIZE: usize = 8;
pub const PAGEMAP_PFN_MASK: u64 = (1 << 55) - 1;
pub const PAGEMAP_SOFT_DIRTY: u64 = 1 << 55;
pub const PAGEMAP_FILE_PAGE: u64 = 1 << 61;
pub const PAGEMAP_SWAPPED: u64 = 1 << 62;
pub const PAGEMAP_PRESENT: u64 = 1 << 63;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_perms_len_and_chars_distinct() {
        assert_eq!(MAPS_PERMS_LEN, 4);
        let c = [
            MAPS_PERM_READ_CHAR,
            MAPS_PERM_WRITE_CHAR,
            MAPS_PERM_EXEC_CHAR,
            MAPS_PERM_PRIVATE_CHAR,
            MAPS_PERM_SHARED_CHAR,
            MAPS_PERM_ABSENT_CHAR,
        ];
        for i in 0..c.len() {
            for j in (i + 1)..c.len() {
                assert_ne!(c[i], c[j]);
            }
        }
    }

    #[test]
    fn test_perm_bits_low_4() {
        let p = [
            MAPS_PERM_READ,
            MAPS_PERM_WRITE,
            MAPS_PERM_EXEC,
            MAPS_PERM_SHARED,
        ];
        let mut or = 0u32;
        for v in p {
            assert!(v.is_power_of_two());
            or |= v;
        }
        assert_eq!(or, 0x0F);
    }

    #[test]
    fn test_pseudo_paths_bracket_form() {
        let p = [
            MAPS_PATH_HEAP,
            MAPS_PATH_STACK,
            MAPS_PATH_VDSO,
            MAPS_PATH_VVAR,
            MAPS_PATH_VSYSCALL,
            MAPS_PATH_ANON,
            MAPS_PATH_UPROBES,
        ];
        for s in p {
            assert!(s.starts_with('['));
            assert!(s.ends_with(']'));
        }
    }

    #[test]
    fn test_pmd_pud_size_ratios_512() {
        // PUD = 512 × PMD on x86_64 with 4 KiB pages.
        assert_eq!(PUD_SIZE_X86_64_4K / PMD_SIZE_X86_64_4K, 512);
        assert_eq!(PMD_SIZE_X86_64_4K, 2 * 1024 * 1024);
        assert_eq!(PUD_SIZE_X86_64_4K, 1 << 30);
    }

    #[test]
    fn test_user_va_tops_increase_with_levels() {
        assert!(X86_64_USER_VA_TOP_48BIT < X86_64_USER_VA_TOP_57BIT);
        // 48-bit canonical top: 47 bits of usable VA → 0x0000_7FFF_FFFF_FFFF.
        assert_eq!(X86_64_USER_VA_TOP_48BIT, (1 << 47) - 1);
        // 57-bit canonical top: 56 bits → 0x00FF_FFFF_FFFF_FFFF.
        assert_eq!(X86_64_USER_VA_TOP_57BIT, (1u64 << 56) - 1);
    }

    #[test]
    fn test_mmap_rnd_bits_min() {
        assert_eq!(MMAP_RND_BITS_MIN_X86_64, 28);
    }

    #[test]
    fn test_pagemap_entry_size_and_bits_disjoint() {
        // Each pagemap entry is a u64.
        assert_eq!(PAGEMAP_ENTRY_SIZE, 8);
        // The PFN occupies bits 0..54.
        assert_eq!(PAGEMAP_PFN_MASK.count_ones(), 55);
        // Status bits all lie above the PFN mask.
        assert_eq!(PAGEMAP_PFN_MASK & PAGEMAP_SOFT_DIRTY, 0);
        assert_eq!(PAGEMAP_PFN_MASK & PAGEMAP_FILE_PAGE, 0);
        assert_eq!(PAGEMAP_PFN_MASK & PAGEMAP_SWAPPED, 0);
        assert_eq!(PAGEMAP_PFN_MASK & PAGEMAP_PRESENT, 0);
        // Each status bit is single.
        for b in [
            PAGEMAP_SOFT_DIRTY,
            PAGEMAP_FILE_PAGE,
            PAGEMAP_SWAPPED,
            PAGEMAP_PRESENT,
        ] {
            assert!(b.is_power_of_two());
        }
    }
}
