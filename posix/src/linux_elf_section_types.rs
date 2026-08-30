//! `<elf.h>` — ELF section header type and flag constants.
//!
//! ELF sections hold the actual content of the file: code, data,
//! symbol tables, string tables, relocation entries, etc. Each
//! section has a type and flags describing its semantics and
//! permissions.

// ---------------------------------------------------------------------------
// Section types (sh_type)
// ---------------------------------------------------------------------------

/// Inactive section.
pub const SHT_NULL: u32 = 0;
/// Program data (code/initialized data).
pub const SHT_PROGBITS: u32 = 1;
/// Symbol table.
pub const SHT_SYMTAB: u32 = 2;
/// String table.
pub const SHT_STRTAB: u32 = 3;
/// Relocation entries with addends.
pub const SHT_RELA: u32 = 4;
/// Symbol hash table.
pub const SHT_HASH: u32 = 5;
/// Dynamic linking info.
pub const SHT_DYNAMIC: u32 = 6;
/// Notes (vendor-specific info).
pub const SHT_NOTE: u32 = 7;
/// Uninitialized data (BSS).
pub const SHT_NOBITS: u32 = 8;
/// Relocation entries without addends.
pub const SHT_REL: u32 = 9;
/// Dynamic linker symbol table.
pub const SHT_DYNSYM: u32 = 11;
/// Initialization function pointers.
pub const SHT_INIT_ARRAY: u32 = 14;
/// Termination function pointers.
pub const SHT_FINI_ARRAY: u32 = 15;
/// Pre-initialization function pointers.
pub const SHT_PREINIT_ARRAY: u32 = 16;
/// Section group.
pub const SHT_GROUP: u32 = 17;
/// GNU hash table.
pub const SHT_GNU_HASH: u32 = 0x6FFF_FFF6;
/// GNU version needs.
pub const SHT_GNU_VERNEED: u32 = 0x6FFF_FFFE;
/// GNU version symbol table.
pub const SHT_GNU_VERSYM: u32 = 0x6FFF_FFFF;

// ---------------------------------------------------------------------------
// Section flags (sh_flags)
// ---------------------------------------------------------------------------

/// Section contains writable data.
pub const SHF_WRITE: u64 = 0x01;
/// Section occupies memory at runtime.
pub const SHF_ALLOC: u64 = 0x02;
/// Section contains executable instructions.
pub const SHF_EXECINSTR: u64 = 0x04;
/// Section may be merged.
pub const SHF_MERGE: u64 = 0x10;
/// Section contains null-terminated strings.
pub const SHF_STRINGS: u64 = 0x20;
/// Section's sh_info holds a section index.
pub const SHF_INFO_LINK: u64 = 0x40;
/// Section requires specific link order.
pub const SHF_LINK_ORDER: u64 = 0x80;
/// Section is thread-local storage.
pub const SHF_TLS: u64 = 0x400;
/// Section is compressed.
pub const SHF_COMPRESSED: u64 = 0x800;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_section_types_distinct() {
        let types = [
            SHT_NULL,
            SHT_PROGBITS,
            SHT_SYMTAB,
            SHT_STRTAB,
            SHT_RELA,
            SHT_HASH,
            SHT_DYNAMIC,
            SHT_NOTE,
            SHT_NOBITS,
            SHT_REL,
            SHT_DYNSYM,
            SHT_INIT_ARRAY,
            SHT_FINI_ARRAY,
            SHT_PREINIT_ARRAY,
            SHT_GROUP,
            SHT_GNU_HASH,
            SHT_GNU_VERNEED,
            SHT_GNU_VERSYM,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_section_flags_no_overlap() {
        let flags = [
            SHF_WRITE,
            SHF_ALLOC,
            SHF_EXECINSTR,
            SHF_MERGE,
            SHF_STRINGS,
            SHF_INFO_LINK,
            SHF_LINK_ORDER,
            SHF_TLS,
            SHF_COMPRESSED,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_null_is_zero() {
        assert_eq!(SHT_NULL, 0);
    }
}
