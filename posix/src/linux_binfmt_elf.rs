//! `<linux/elf.h>` — ELF binary format constants.
//!
//! The Executable and Linkable Format (ELF) is the standard binary
//! format for Linux executables, shared libraries, and core dumps.
//! These constants define ELF header fields, section types,
//! segment types, and related values used by the kernel's ELF loader.

// ---------------------------------------------------------------------------
// ELF magic bytes
// ---------------------------------------------------------------------------

/// ELF magic byte 0.
pub const ELFMAG0: u8 = 0x7F;
/// ELF magic byte 1.
pub const ELFMAG1: u8 = b'E';
/// ELF magic byte 2.
pub const ELFMAG2: u8 = b'L';
/// ELF magic byte 3.
pub const ELFMAG3: u8 = b'F';

/// Complete ELF magic number (first 4 bytes).
pub const ELFMAG: [u8; 4] = [ELFMAG0, ELFMAG1, ELFMAG2, ELFMAG3];

/// Size of the ELF identification array.
pub const EI_NIDENT: usize = 16;

// ---------------------------------------------------------------------------
// ELF class (32-bit vs 64-bit)
// ---------------------------------------------------------------------------

/// Invalid class.
pub const ELFCLASSNONE: u8 = 0;
/// 32-bit ELF.
pub const ELFCLASS32: u8 = 1;
/// 64-bit ELF.
pub const ELFCLASS64: u8 = 2;

// ---------------------------------------------------------------------------
// ELF data encoding (endianness)
// ---------------------------------------------------------------------------

/// Invalid data encoding.
pub const ELFDATANONE: u8 = 0;
/// Little-endian (2's complement, LSB first).
pub const ELFDATA2LSB: u8 = 1;
/// Big-endian (2's complement, MSB first).
pub const ELFDATA2MSB: u8 = 2;

// ---------------------------------------------------------------------------
// ELF identification indices
// ---------------------------------------------------------------------------

/// File identification (magic number).
pub const EI_MAG0: usize = 0;
/// File class.
pub const EI_CLASS: usize = 4;
/// Data encoding.
pub const EI_DATA: usize = 5;
/// File version.
pub const EI_VERSION: usize = 6;
/// OS/ABI identification.
pub const EI_OSABI: usize = 7;
/// ABI version.
pub const EI_ABIVERSION: usize = 8;
/// Start of padding bytes.
pub const EI_PAD: usize = 9;

// ---------------------------------------------------------------------------
// ELF file types (e_type)
// ---------------------------------------------------------------------------

/// No file type.
pub const ET_NONE: u16 = 0;
/// Relocatable file.
pub const ET_REL: u16 = 1;
/// Executable file.
pub const ET_EXEC: u16 = 2;
/// Shared object file.
pub const ET_DYN: u16 = 3;
/// Core file.
pub const ET_CORE: u16 = 4;

// ---------------------------------------------------------------------------
// ELF machine types (e_machine) — common subset
// ---------------------------------------------------------------------------

/// x86 (32-bit).
pub const EM_386: u16 = 3;
/// ARM (32-bit).
pub const EM_ARM: u16 = 40;
/// x86_64.
pub const EM_X86_64: u16 = 62;
/// AArch64.
pub const EM_AARCH64: u16 = 183;
/// RISC-V.
pub const EM_RISCV: u16 = 243;

// ---------------------------------------------------------------------------
// ELF version
// ---------------------------------------------------------------------------

/// Invalid version.
pub const EV_NONE_ELF: u32 = 0;
/// Current version.
pub const EV_CURRENT: u32 = 1;

// ---------------------------------------------------------------------------
// Program header types (p_type)
// ---------------------------------------------------------------------------

/// Unused entry.
pub const PT_NULL: u32 = 0;
/// Loadable segment.
pub const PT_LOAD: u32 = 1;
/// Dynamic linking info.
pub const PT_DYNAMIC: u32 = 2;
/// Interpreter path.
pub const PT_INTERP: u32 = 3;
/// Auxiliary information.
pub const PT_NOTE: u32 = 4;
/// Reserved (unused).
pub const PT_SHLIB: u32 = 5;
/// Program header table.
pub const PT_PHDR: u32 = 6;
/// Thread-local storage.
pub const PT_TLS: u32 = 7;
/// GNU EH frame.
pub const PT_GNU_EH_FRAME: u32 = 0x6474_E550;
/// GNU stack permissions.
pub const PT_GNU_STACK: u32 = 0x6474_E551;
/// GNU read-only after relocation.
pub const PT_GNU_RELRO: u32 = 0x6474_E552;
/// GNU property note.
pub const PT_GNU_PROPERTY: u32 = 0x6474_E553;

// ---------------------------------------------------------------------------
// Segment flags (p_flags)
// ---------------------------------------------------------------------------

/// Segment is executable.
pub const PF_X: u32 = 1 << 0;
/// Segment is writable.
pub const PF_W: u32 = 1 << 1;
/// Segment is readable.
pub const PF_R: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Section header types (sh_type)
// ---------------------------------------------------------------------------

/// Inactive section.
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
/// Dynamic linking info.
pub const SHT_DYNAMIC: u32 = 6;
/// Note section.
pub const SHT_NOTE: u32 = 7;
/// BSS (no file data).
pub const SHT_NOBITS: u32 = 8;
/// Relocation entries without addends.
pub const SHT_REL: u32 = 9;
/// Reserved.
pub const SHT_SHLIB: u32 = 10;
/// Dynamic symbol table.
pub const SHT_DYNSYM: u32 = 11;
/// Init array.
pub const SHT_INIT_ARRAY: u32 = 14;
/// Fini array.
pub const SHT_FINI_ARRAY: u32 = 15;

// ---------------------------------------------------------------------------
// Section flags (sh_flags)
// ---------------------------------------------------------------------------

/// Writable section.
pub const SHF_WRITE: u64 = 1 << 0;
/// Section occupies memory at runtime.
pub const SHF_ALLOC: u64 = 1 << 1;
/// Executable section.
pub const SHF_EXECINSTR: u64 = 1 << 2;
/// Section may be merged.
pub const SHF_MERGE: u64 = 1 << 4;
/// Contains null-terminated strings.
pub const SHF_STRINGS: u64 = 1 << 5;
/// Section holds thread-local data.
pub const SHF_TLS: u64 = 1 << 10;

// ---------------------------------------------------------------------------
// Auxiliary vector types (AT_*)
// ---------------------------------------------------------------------------

/// End of auxiliary vector.
pub const AT_NULL: u64 = 0;
/// Program headers start.
pub const AT_PHDR: u64 = 3;
/// Size of one program header entry.
pub const AT_PHENT: u64 = 4;
/// Number of program headers.
pub const AT_PHNUM: u64 = 5;
/// System page size.
pub const AT_PAGESZ: u64 = 6;
/// Interpreter base address.
pub const AT_BASE: u64 = 7;
/// Flags.
pub const AT_FLAGS: u64 = 8;
/// Entry point of program.
pub const AT_ENTRY: u64 = 9;
/// Real UID.
pub const AT_UID: u64 = 11;
/// Effective UID.
pub const AT_EUID: u64 = 12;
/// Real GID.
pub const AT_GID: u64 = 13;
/// Effective GID.
pub const AT_EGID: u64 = 14;
/// Platform string.
pub const AT_PLATFORM: u64 = 15;
/// Hardware capability bits.
pub const AT_HWCAP: u64 = 16;
/// Clock ticks per second.
pub const AT_CLKTCK: u64 = 17;
/// Secure mode flag.
pub const AT_SECURE: u64 = 23;
/// Random bytes (16 bytes of entropy).
pub const AT_RANDOM: u64 = 25;
/// Extended hardware capabilities.
pub const AT_HWCAP2: u64 = 26;
/// Filename of executed program.
pub const AT_EXECFN: u64 = 31;
/// Pointer to vDSO.
pub const AT_SYSINFO_EHDR: u64 = 33;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_elf_magic() {
        assert_eq!(ELFMAG, [0x7F, b'E', b'L', b'F']);
    }

    #[test]
    fn test_ei_nident() {
        assert_eq!(EI_NIDENT, 16);
    }

    #[test]
    fn test_elf_classes_distinct() {
        let classes = [ELFCLASSNONE, ELFCLASS32, ELFCLASS64];
        for i in 0..classes.len() {
            for j in (i + 1)..classes.len() {
                assert_ne!(classes[i], classes[j]);
            }
        }
    }

    #[test]
    fn test_elf_data_distinct() {
        let data = [ELFDATANONE, ELFDATA2LSB, ELFDATA2MSB];
        for i in 0..data.len() {
            for j in (i + 1)..data.len() {
                assert_ne!(data[i], data[j]);
            }
        }
    }

    #[test]
    fn test_file_types_distinct() {
        let types = [ET_NONE, ET_REL, ET_EXEC, ET_DYN, ET_CORE];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_machine_types_distinct() {
        let machines = [EM_386, EM_ARM, EM_X86_64, EM_AARCH64, EM_RISCV];
        for i in 0..machines.len() {
            for j in (i + 1)..machines.len() {
                assert_ne!(machines[i], machines[j]);
            }
        }
    }

    #[test]
    fn test_phdr_types_distinct() {
        let types = [
            PT_NULL,
            PT_LOAD,
            PT_DYNAMIC,
            PT_INTERP,
            PT_NOTE,
            PT_SHLIB,
            PT_PHDR,
            PT_TLS,
            PT_GNU_EH_FRAME,
            PT_GNU_STACK,
            PT_GNU_RELRO,
            PT_GNU_PROPERTY,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_segment_flags_powers_of_two() {
        let flags = [PF_X, PF_W, PF_R];
        for flag in &flags {
            assert!(flag.is_power_of_two(), "0x{:x}", flag);
        }
    }

    #[test]
    fn test_segment_flags_no_overlap() {
        let flags = [PF_X, PF_W, PF_R];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

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
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_section_flags_powers_of_two() {
        let flags = [
            SHF_WRITE,
            SHF_ALLOC,
            SHF_EXECINSTR,
            SHF_MERGE,
            SHF_STRINGS,
            SHF_TLS,
        ];
        for flag in &flags {
            assert!(flag.is_power_of_two(), "0x{:x}", flag);
        }
    }

    #[test]
    fn test_auxv_types_distinct() {
        let types = [
            AT_NULL,
            AT_PHDR,
            AT_PHENT,
            AT_PHNUM,
            AT_PAGESZ,
            AT_BASE,
            AT_FLAGS,
            AT_ENTRY,
            AT_UID,
            AT_EUID,
            AT_GID,
            AT_EGID,
            AT_PLATFORM,
            AT_HWCAP,
            AT_CLKTCK,
            AT_SECURE,
            AT_RANDOM,
            AT_HWCAP2,
            AT_EXECFN,
            AT_SYSINFO_EHDR,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_ei_indices_distinct() {
        let indices = [
            EI_MAG0,
            EI_CLASS,
            EI_DATA,
            EI_VERSION,
            EI_OSABI,
            EI_ABIVERSION,
            EI_PAD,
        ];
        for i in 0..indices.len() {
            for j in (i + 1)..indices.len() {
                assert_ne!(indices[i], indices[j]);
            }
        }
    }
}
