//! `<linux/elf.h>` — Additional ELF format constants.
//!
//! Supplementary ELF constants covering section types,
//! dynamic tags, note types, and relocation types.

// ---------------------------------------------------------------------------
// ELF section types (SHT_*)
// ---------------------------------------------------------------------------

/// Null section.
pub const SHT_NULL: u32 = 0;
/// Program data.
pub const SHT_PROGBITS: u32 = 1;
/// Symbol table.
pub const SHT_SYMTAB: u32 = 2;
/// String table.
pub const SHT_STRTAB: u32 = 3;
/// Relocation entries with addends.
pub const SHT_RELA: u32 = 4;
/// Symbol hash table.
pub const SHT_HASH: u32 = 5;
/// Dynamic linking information.
pub const SHT_DYNAMIC: u32 = 6;
/// Notes.
pub const SHT_NOTE: u32 = 7;
/// BSS (no data in file).
pub const SHT_NOBITS: u32 = 8;
/// Relocation entries without addends.
pub const SHT_REL: u32 = 9;
/// Reserved.
pub const SHT_SHLIB: u32 = 10;
/// Dynamic linker symbol table.
pub const SHT_DYNSYM: u32 = 11;
/// Init array.
pub const SHT_INIT_ARRAY: u32 = 14;
/// Fini array.
pub const SHT_FINI_ARRAY: u32 = 15;
/// Pre-init array.
pub const SHT_PREINIT_ARRAY: u32 = 16;
/// Section group.
pub const SHT_GROUP: u32 = 17;
/// Extended section index.
pub const SHT_SYMTAB_SHNDX: u32 = 18;
/// GNU hash.
pub const SHT_GNU_HASH: u32 = 0x6FFFFFF6;
/// GNU version definitions.
pub const SHT_GNU_VERDEF: u32 = 0x6FFFFFFD;
/// GNU version needs.
pub const SHT_GNU_VERNEED: u32 = 0x6FFFFFFE;
/// GNU version symbol table.
pub const SHT_GNU_VERSYM: u32 = 0x6FFFFFFF;

// ---------------------------------------------------------------------------
// ELF section flags (SHF_*)
// ---------------------------------------------------------------------------

/// Writable.
pub const SHF_WRITE: u64 = 1 << 0;
/// Allocate memory.
pub const SHF_ALLOC: u64 = 1 << 1;
/// Executable.
pub const SHF_EXECINSTR: u64 = 1 << 2;
/// Merge.
pub const SHF_MERGE: u64 = 1 << 4;
/// Strings.
pub const SHF_STRINGS: u64 = 1 << 5;
/// Info link.
pub const SHF_INFO_LINK: u64 = 1 << 6;
/// Link order.
pub const SHF_LINK_ORDER: u64 = 1 << 7;
/// Non-conforming OS.
pub const SHF_OS_NONCONFORMING: u64 = 1 << 8;
/// Group member.
pub const SHF_GROUP: u64 = 1 << 9;
/// Thread-local storage.
pub const SHF_TLS: u64 = 1 << 10;
/// Compressed.
pub const SHF_COMPRESSED: u64 = 1 << 11;

// ---------------------------------------------------------------------------
// ELF dynamic tags (DT_*)
// ---------------------------------------------------------------------------

/// NULL tag (marks end).
pub const DT_NULL: i64 = 0;
/// Needed shared library.
pub const DT_NEEDED: i64 = 1;
/// PLT relocation size.
pub const DT_PLTRELSZ: i64 = 2;
/// PLT/GOT address.
pub const DT_PLTGOT: i64 = 3;
/// Hash table address.
pub const DT_HASH: i64 = 4;
/// String table address.
pub const DT_STRTAB: i64 = 5;
/// Symbol table address.
pub const DT_SYMTAB: i64 = 6;
/// RELA table address.
pub const DT_RELA: i64 = 7;
/// RELA table size.
pub const DT_RELASZ: i64 = 8;
/// RELA entry size.
pub const DT_RELAENT: i64 = 9;
/// String table size.
pub const DT_STRSZ: i64 = 10;
/// Symbol entry size.
pub const DT_SYMENT: i64 = 11;
/// Init function address.
pub const DT_INIT: i64 = 12;
/// Fini function address.
pub const DT_FINI: i64 = 13;
/// Shared object name.
pub const DT_SONAME: i64 = 14;
/// Library search path.
pub const DT_RPATH: i64 = 15;

// ---------------------------------------------------------------------------
// ELF note types (NT_*)
// ---------------------------------------------------------------------------

/// Process status.
pub const NT_PRSTATUS: u32 = 1;
/// Floating point registers.
pub const NT_PRFPREG: u32 = 2;
/// Process info.
pub const NT_PRPSINFO: u32 = 3;
/// Task structure.
pub const NT_TASKSTRUCT: u32 = 4;
/// Auxiliary vector.
pub const NT_AUXV: u32 = 6;
/// x86 XSTATE.
pub const NT_X86_XSTATE: u32 = 0x202;
/// ARM VFP registers.
pub const NT_ARM_VFP: u32 = 0x400;
/// GNU ABI tag.
pub const NT_GNU_ABI_TAG: u32 = 1;
/// GNU build ID.
pub const NT_GNU_BUILD_ID: u32 = 3;

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
            SHT_SHLIB,
            SHT_DYNSYM,
            SHT_INIT_ARRAY,
            SHT_FINI_ARRAY,
            SHT_PREINIT_ARRAY,
            SHT_GROUP,
            SHT_SYMTAB_SHNDX,
            SHT_GNU_HASH,
            SHT_GNU_VERDEF,
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
    fn test_section_flags_power_of_two() {
        let flags = [
            SHF_WRITE,
            SHF_ALLOC,
            SHF_EXECINSTR,
            SHF_MERGE,
            SHF_STRINGS,
            SHF_INFO_LINK,
            SHF_LINK_ORDER,
            SHF_OS_NONCONFORMING,
            SHF_GROUP,
            SHF_TLS,
            SHF_COMPRESSED,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "0x{:04x} not power of two", f);
        }
    }

    #[test]
    fn test_dynamic_tags_distinct() {
        let tags = [
            DT_NULL,
            DT_NEEDED,
            DT_PLTRELSZ,
            DT_PLTGOT,
            DT_HASH,
            DT_STRTAB,
            DT_SYMTAB,
            DT_RELA,
            DT_RELASZ,
            DT_RELAENT,
            DT_STRSZ,
            DT_SYMENT,
            DT_INIT,
            DT_FINI,
            DT_SONAME,
            DT_RPATH,
        ];
        for i in 0..tags.len() {
            for j in (i + 1)..tags.len() {
                assert_ne!(tags[i], tags[j]);
            }
        }
    }

    #[test]
    fn test_note_types_distinct() {
        let notes = [
            NT_PRSTATUS,
            NT_PRFPREG,
            NT_PRPSINFO,
            NT_TASKSTRUCT,
            NT_AUXV,
            NT_X86_XSTATE,
            NT_ARM_VFP,
        ];
        for i in 0..notes.len() {
            for j in (i + 1)..notes.len() {
                assert_ne!(notes[i], notes[j]);
            }
        }
    }
}
