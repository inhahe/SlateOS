//! `<linux/vdso_elf.h>` — vDSO ELF structure constants.
//!
//! The vDSO (virtual dynamic shared object) is mapped into every
//! process as a standard ELF shared library. It contains minimal
//! ELF headers, a symbol table, and code for accelerated syscalls.
//! These constants define the ELF structure layout of the vDSO image:
//! section indices, symbol hash parameters, and the function table
//! layout used by the dynamic linker to resolve vDSO symbols.

// ---------------------------------------------------------------------------
// vDSO ELF symbol versions
// ---------------------------------------------------------------------------

/// vDSO symbol version: Linux 2.6.
pub const VDSO_VERSION_LINUX_2_6: u32 = 0;
/// vDSO symbol version: Linux 2.6.39 (added clock_gettime).
pub const VDSO_VERSION_LINUX_2_6_39: u32 = 1;
/// vDSO symbol version: Linux 4.0 (added clock_getres).
pub const VDSO_VERSION_LINUX_4_0: u32 = 2;

// ---------------------------------------------------------------------------
// vDSO ELF section indices
// ---------------------------------------------------------------------------

/// .text section (code).
pub const VDSO_SEC_TEXT: u32 = 0;
/// .data section (vDSO data page pointer).
pub const VDSO_SEC_DATA: u32 = 1;
/// .dynamic section (dynamic linking info).
pub const VDSO_SEC_DYNAMIC: u32 = 2;
/// .dynsym section (dynamic symbol table).
pub const VDSO_SEC_DYNSYM: u32 = 3;
/// .dynstr section (dynamic string table).
pub const VDSO_SEC_DYNSTR: u32 = 4;
/// .note section (build ID, etc.).
pub const VDSO_SEC_NOTE: u32 = 5;

// ---------------------------------------------------------------------------
// vDSO symbol name constants
// ---------------------------------------------------------------------------

/// Maximum symbol name length in vDSO.
pub const VDSO_SYMNAME_MAX: u32 = 64;
/// Number of exported functions in typical vDSO.
pub const VDSO_NUM_FUNCS: u32 = 5;

// ---------------------------------------------------------------------------
// vDSO page layout
// ---------------------------------------------------------------------------

/// vDSO total size (one page for code, one for data).
pub const VDSO_PAGES: u32 = 2;
/// vDSO code page index.
pub const VDSO_PAGE_CODE: u32 = 0;
/// vDSO data page index.
pub const VDSO_PAGE_DATA: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_versions_distinct() {
        let versions = [
            VDSO_VERSION_LINUX_2_6,
            VDSO_VERSION_LINUX_2_6_39,
            VDSO_VERSION_LINUX_4_0,
        ];
        for i in 0..versions.len() {
            for j in (i + 1)..versions.len() {
                assert_ne!(versions[i], versions[j]);
            }
        }
    }

    #[test]
    fn test_sections_distinct() {
        let secs = [
            VDSO_SEC_TEXT, VDSO_SEC_DATA, VDSO_SEC_DYNAMIC,
            VDSO_SEC_DYNSYM, VDSO_SEC_DYNSTR, VDSO_SEC_NOTE,
        ];
        for i in 0..secs.len() {
            for j in (i + 1)..secs.len() {
                assert_ne!(secs[i], secs[j]);
            }
        }
    }

    #[test]
    fn test_pages() {
        assert_eq!(VDSO_PAGES, 2);
        assert_ne!(VDSO_PAGE_CODE, VDSO_PAGE_DATA);
    }
}
