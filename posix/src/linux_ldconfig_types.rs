//! `<elf.h>` / ldconfig — Dynamic linker cache constants.
//!
//! `ldconfig` maintains the shared library cache used by the
//! dynamic linker (`ld.so`).  These constants define the cache
//! file format, library types, and search path conventions.

// ---------------------------------------------------------------------------
// ld.so.cache magic numbers and version
// ---------------------------------------------------------------------------

/// Old-format cache magic string length.
pub const LDCONFIG_MAGIC_OLD_LEN: u32 = 11;
/// New-format cache magic string: "glibc-ld.so.cache1.1".
pub const LDCONFIG_CACHE_VERSION: u32 = 0x00010001;
/// New-format cache header size (bytes).
pub const LDCONFIG_NEW_HEADER_SIZE: u32 = 48;

// ---------------------------------------------------------------------------
// Library type flags (in cache entries)
// ---------------------------------------------------------------------------

/// ELF library.
pub const LDCONFIG_FLAG_ELF: u32 = 0x0001;
/// ELF libc6 (glibc) library.
pub const LDCONFIG_FLAG_ELF_LIBC6: u32 = 0x0003;
/// x86_64 64-bit library.
pub const LDCONFIG_FLAG_X86_64: u32 = 0x0300;
/// i386 32-bit library.
pub const LDCONFIG_FLAG_I386: u32 = 0x0800;
/// ARM hard-float library.
pub const LDCONFIG_FLAG_ARM_HF: u32 = 0x0500;
/// AArch64 library.
pub const LDCONFIG_FLAG_AARCH64: u32 = 0x0a00;

// ---------------------------------------------------------------------------
// Standard library paths
// ---------------------------------------------------------------------------

/// Number of standard search directories.
pub const LDCONFIG_STD_DIRS_COUNT: u32 = 2;
/// Maximum library path length in cache entries.
pub const LDCONFIG_MAX_PATH: u32 = 4096;

// ---------------------------------------------------------------------------
// ld.so.conf parsing
// ---------------------------------------------------------------------------

/// Maximum include nesting depth.
pub const LDCONFIG_INCLUDE_MAX_DEPTH: u32 = 16;
/// Maximum number of directories in config.
pub const LDCONFIG_MAX_DIRS: u32 = 1024;

// ---------------------------------------------------------------------------
// Dynamic linker flags (DT_FLAGS / DT_FLAGS_1)
// ---------------------------------------------------------------------------

/// Library is a filter (DT_FLAGS).
pub const DF_ORIGIN: u32 = 0x00000001;
/// Symbol resolution is performed lazily.
pub const DF_SYMBOLIC: u32 = 0x00000002;
/// Object uses text relocations.
pub const DF_TEXTREL: u32 = 0x00000004;
/// Object uses binding now.
pub const DF_BIND_NOW: u32 = 0x00000008;
/// Object requires static TLS.
pub const DF_STATIC_TLS: u32 = 0x00000010;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_version() {
        assert_eq!(LDCONFIG_CACHE_VERSION, 0x00010001);
    }

    #[test]
    fn test_flags_distinct() {
        let flags = [
            LDCONFIG_FLAG_ELF,
            LDCONFIG_FLAG_ELF_LIBC6,
            LDCONFIG_FLAG_X86_64,
            LDCONFIG_FLAG_I386,
            LDCONFIG_FLAG_ARM_HF,
            LDCONFIG_FLAG_AARCH64,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_elf_flag_is_one() {
        assert_eq!(LDCONFIG_FLAG_ELF, 1);
    }

    #[test]
    fn test_max_path() {
        assert_eq!(LDCONFIG_MAX_PATH, 4096);
    }

    #[test]
    fn test_include_max_depth() {
        assert_eq!(LDCONFIG_INCLUDE_MAX_DEPTH, 16);
    }

    #[test]
    fn test_dt_flags_powers_of_two() {
        let flags = [
            DF_ORIGIN,
            DF_SYMBOLIC,
            DF_TEXTREL,
            DF_BIND_NOW,
            DF_STATIC_TLS,
        ];
        for f in flags {
            assert!(f.is_power_of_two());
        }
    }

    #[test]
    fn test_dt_flags_no_overlap() {
        let flags = [
            DF_ORIGIN,
            DF_SYMBOLIC,
            DF_TEXTREL,
            DF_BIND_NOW,
            DF_STATIC_TLS,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_origin_is_one() {
        assert_eq!(DF_ORIGIN, 1);
    }

    #[test]
    fn test_std_dirs_count() {
        assert_eq!(LDCONFIG_STD_DIRS_COUNT, 2);
    }
}
