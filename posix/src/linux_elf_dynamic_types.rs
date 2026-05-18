//! `<elf.h>` — ELF dynamic section tag constants.
//!
//! The .dynamic section contains an array of tag-value pairs that
//! the dynamic linker uses to locate symbol tables, relocation
//! entries, shared library dependencies, and initialization
//! functions at runtime.

// ---------------------------------------------------------------------------
// Dynamic section tags (d_tag)
// ---------------------------------------------------------------------------

/// End of dynamic array (sentinel).
pub const DT_NULL: i64 = 0;
/// Name of a needed shared library.
pub const DT_NEEDED: i64 = 1;
/// Total size of PLT relocation entries.
pub const DT_PLTRELSZ: i64 = 2;
/// Address of PLT/GOT.
pub const DT_PLTGOT: i64 = 3;
/// Address of symbol hash table.
pub const DT_HASH: i64 = 4;
/// Address of string table (.dynstr).
pub const DT_STRTAB: i64 = 5;
/// Address of symbol table (.dynsym).
pub const DT_SYMTAB: i64 = 6;
/// Address of RELA relocation table.
pub const DT_RELA: i64 = 7;
/// Total size of RELA table.
pub const DT_RELASZ: i64 = 8;
/// Size of one RELA entry.
pub const DT_RELAENT: i64 = 9;
/// Size of string table.
pub const DT_STRSZ: i64 = 10;
/// Size of one symbol table entry.
pub const DT_SYMENT: i64 = 11;
/// Address of init function.
pub const DT_INIT: i64 = 12;
/// Address of fini function.
pub const DT_FINI: i64 = 13;
/// Name of shared object (soname).
pub const DT_SONAME: i64 = 14;
/// Library search path.
pub const DT_RPATH: i64 = 15;
/// Type of PLT relocations (DT_REL or DT_RELA).
pub const DT_PLTREL: i64 = 20;
/// Address of PLT relocations.
pub const DT_JMPREL: i64 = 23;
/// Bind now (no lazy binding).
pub const DT_BIND_NOW: i64 = 24;
/// Address of init_array.
pub const DT_INIT_ARRAY: i64 = 25;
/// Address of fini_array.
pub const DT_FINI_ARRAY: i64 = 26;
/// Size of init_array.
pub const DT_INIT_ARRAYSZ: i64 = 27;
/// Size of fini_array.
pub const DT_FINI_ARRAYSZ: i64 = 28;
/// Library search path (replaces RPATH).
pub const DT_RUNPATH: i64 = 29;
/// Flags (DT_FLAGS values).
pub const DT_FLAGS: i64 = 30;
/// Extended flags (DT_FLAGS_1 values).
pub const DT_FLAGS_1: i64 = 0x6FFF_FFFB;
/// GNU hash table address.
pub const DT_GNU_HASH: i64 = 0x6FFF_FEF5;

// ---------------------------------------------------------------------------
// DT_FLAGS values
// ---------------------------------------------------------------------------

/// Object uses $ORIGIN substitution.
pub const DF_ORIGIN: u64 = 0x01;
/// Symbol resolution must include this object.
pub const DF_SYMBOLIC: u64 = 0x02;
/// Object contains text relocations.
pub const DF_TEXTREL: u64 = 0x04;
/// No lazy binding for this object.
pub const DF_BIND_NOW: u64 = 0x08;
/// Object uses static TLS.
pub const DF_STATIC_TLS: u64 = 0x10;

// ---------------------------------------------------------------------------
// DT_FLAGS_1 values
// ---------------------------------------------------------------------------

/// Set RTLD_NOW for this object.
pub const DF_1_NOW: u64 = 0x0000_0001;
/// Set RTLD_GLOBAL for this object.
pub const DF_1_GLOBAL: u64 = 0x0000_0002;
/// Object is a filter.
pub const DF_1_GROUP: u64 = 0x0000_0004;
/// Object may not be deleted.
pub const DF_1_NODELETE: u64 = 0x0000_0008;
/// Position-independent executable.
pub const DF_1_PIE: u64 = 0x0800_0000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tags_distinct() {
        let tags = [
            DT_NULL, DT_NEEDED, DT_PLTRELSZ, DT_PLTGOT,
            DT_HASH, DT_STRTAB, DT_SYMTAB, DT_RELA,
            DT_RELASZ, DT_RELAENT, DT_STRSZ, DT_SYMENT,
            DT_INIT, DT_FINI, DT_SONAME, DT_RPATH,
            DT_PLTREL, DT_JMPREL, DT_BIND_NOW,
            DT_INIT_ARRAY, DT_FINI_ARRAY,
            DT_INIT_ARRAYSZ, DT_FINI_ARRAYSZ,
            DT_RUNPATH, DT_FLAGS,
        ];
        for i in 0..tags.len() {
            for j in (i + 1)..tags.len() {
                assert_ne!(tags[i], tags[j]);
            }
        }
    }

    #[test]
    fn test_df_flags_no_overlap() {
        let flags = [
            DF_ORIGIN, DF_SYMBOLIC, DF_TEXTREL,
            DF_BIND_NOW, DF_STATIC_TLS,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_null_is_zero() {
        assert_eq!(DT_NULL, 0);
    }
}
