//! `<linux/auxvec.h>` — ELF auxiliary vector tag values (`AT_*`).
//!
//! At process exec the kernel pushes an auxiliary vector of
//! `Elf_auxv_t { a_type, a_un }` pairs above `argv`/`envp`. The C
//! library reads `AT_PHDR`, `AT_ENTRY`, `AT_RANDOM`, `AT_HWCAP`, etc.
//! out of it. Tag numbers are stable kernel ABI.

// ---------------------------------------------------------------------------
// Sentinel and ignored entries
// ---------------------------------------------------------------------------

pub const AT_NULL: u32 = 0;
pub const AT_IGNORE: u32 = 1;

// ---------------------------------------------------------------------------
// Loader-passing entries (program-image description)
// ---------------------------------------------------------------------------

pub const AT_EXECFD: u32 = 2;
pub const AT_PHDR: u32 = 3;
pub const AT_PHENT: u32 = 4;
pub const AT_PHNUM: u32 = 5;
pub const AT_PAGESZ: u32 = 6;
pub const AT_BASE: u32 = 7;
pub const AT_FLAGS: u32 = 8;
pub const AT_ENTRY: u32 = 9;
pub const AT_NOTELF: u32 = 10;
pub const AT_UID: u32 = 11;
pub const AT_EUID: u32 = 12;
pub const AT_GID: u32 = 13;
pub const AT_EGID: u32 = 14;
pub const AT_PLATFORM: u32 = 15;
pub const AT_HWCAP: u32 = 16;
pub const AT_CLKTCK: u32 = 17;

// ---------------------------------------------------------------------------
// Extended entries (non-contiguous on purpose — gap reserved)
// ---------------------------------------------------------------------------

pub const AT_SECURE: u32 = 23;
pub const AT_BASE_PLATFORM: u32 = 24;
pub const AT_RANDOM: u32 = 25;
pub const AT_HWCAP2: u32 = 26;
pub const AT_RSEQ_FEATURE_SIZE: u32 = 27;
pub const AT_RSEQ_ALIGN: u32 = 28;
pub const AT_HWCAP3: u32 = 29;
pub const AT_HWCAP4: u32 = 30;

pub const AT_EXECFN: u32 = 31;

pub const AT_SYSINFO: u32 = 32;
pub const AT_SYSINFO_EHDR: u32 = 33;

// ---------------------------------------------------------------------------
// AT_FLAGS bits (per-arch — generic bit zero is "preserve trampolines")
// ---------------------------------------------------------------------------

pub const AT_FLAGS_PRESERVE_ARGV0: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sentinel_and_ignore() {
        assert_eq!(AT_NULL, 0);
        assert_eq!(AT_IGNORE, 1);
        // AT_NULL terminates the vector.
        assert!(AT_NULL < AT_IGNORE);
    }

    #[test]
    fn test_loader_block_dense_2_to_17() {
        let l = [
            AT_EXECFD,
            AT_PHDR,
            AT_PHENT,
            AT_PHNUM,
            AT_PAGESZ,
            AT_BASE,
            AT_FLAGS,
            AT_ENTRY,
            AT_NOTELF,
            AT_UID,
            AT_EUID,
            AT_GID,
            AT_EGID,
            AT_PLATFORM,
            AT_HWCAP,
            AT_CLKTCK,
        ];
        for (i, &v) in l.iter().enumerate() {
            assert_eq!(v as usize, 2 + i);
        }
    }

    #[test]
    fn test_extended_block_after_gap() {
        // Tags 18..22 are reserved (historic FPU/cache info, now unused).
        assert!(AT_SECURE > 17);
        assert_eq!(AT_SECURE, 23);
        let e = [
            AT_SECURE,
            AT_BASE_PLATFORM,
            AT_RANDOM,
            AT_HWCAP2,
            AT_RSEQ_FEATURE_SIZE,
            AT_RSEQ_ALIGN,
            AT_HWCAP3,
            AT_HWCAP4,
            AT_EXECFN,
        ];
        // Densely numbered 23..=31.
        for (i, &v) in e.iter().enumerate() {
            assert_eq!(v as usize, 23 + i);
        }
        assert_eq!(AT_EXECFN, 31);
    }

    #[test]
    fn test_hwcap_family_spaced() {
        // HWCAP, HWCAP2, HWCAP3, HWCAP4 are *not* dense (they were
        // added at different kernel releases).
        assert_eq!(AT_HWCAP, 16);
        assert_eq!(AT_HWCAP2, 26);
        assert_eq!(AT_HWCAP3, 29);
        assert_eq!(AT_HWCAP4, 30);
        // Distinct.
        let h = [AT_HWCAP, AT_HWCAP2, AT_HWCAP3, AT_HWCAP4];
        for (i, &a) in h.iter().enumerate() {
            for &b in &h[i + 1..] {
                assert_ne!(a, b);
            }
        }
    }

    #[test]
    fn test_sysinfo_pair_after_execfn() {
        assert_eq!(AT_SYSINFO, 32);
        assert_eq!(AT_SYSINFO_EHDR, 33);
        assert_eq!(AT_SYSINFO_EHDR - AT_SYSINFO, 1);
        assert!(AT_SYSINFO > AT_EXECFN);
    }

    #[test]
    fn test_at_flags_bit_zero() {
        assert_eq!(AT_FLAGS_PRESERVE_ARGV0, 1);
        assert!(AT_FLAGS_PRESERVE_ARGV0.is_power_of_two());
    }
}
