//! `<linux/crash_core.h>` — Crash dump core constants.
//!
//! The crash dump subsystem captures kernel memory after a panic
//! for post-mortem analysis. A pre-loaded crash kernel boots via
//! kexec, reads the old kernel's memory, and writes it to a dump
//! file. makedumpfile and crash utility parse these dumps.

// ---------------------------------------------------------------------------
// Crash dump types
// ---------------------------------------------------------------------------

/// ELF core dump format.
pub const KDUMP_FORMAT_ELF: u32 = 0;
/// Compressed kdump format.
pub const KDUMP_FORMAT_COMPRESSED: u32 = 1;

// ---------------------------------------------------------------------------
// Vmcoreinfo note type
// ---------------------------------------------------------------------------

/// Vmcoreinfo ELF note name.
pub const VMCOREINFO_NOTE_NAME: &str = "VMCOREINFO";
/// Vmcoreinfo note type.
pub const VMCOREINFO_NOTE_TYPE: u32 = 0;
/// Maximum vmcoreinfo size.
pub const VMCOREINFO_MAX_SIZE: usize = 4096;

// ---------------------------------------------------------------------------
// PHDR flags for crash segments
// ---------------------------------------------------------------------------

/// Readable segment.
pub const CRASH_SEG_READ: u32 = 1 << 0;
/// Writable segment.
pub const CRASH_SEG_WRITE: u32 = 1 << 1;
/// Executable segment.
pub const CRASH_SEG_EXEC: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Crash memory range types
// ---------------------------------------------------------------------------

/// RAM (normal memory).
pub const CRASH_MEM_RAM: u32 = 0;
/// Reserved memory.
pub const CRASH_MEM_RESERVED: u32 = 1;
/// ACPI tables.
pub const CRASH_MEM_ACPI: u32 = 2;
/// ACPI NVS.
pub const CRASH_MEM_ACPI_NVS: u32 = 3;
/// Unusable memory.
pub const CRASH_MEM_UNUSABLE: u32 = 4;
/// Persistent memory.
pub const CRASH_MEM_PERSISTENT: u32 = 5;

// ---------------------------------------------------------------------------
// Makedumpfile compression types
// ---------------------------------------------------------------------------

/// No compression.
pub const DUMP_COMPRESS_NONE: u32 = 0;
/// Zlib compression.
pub const DUMP_COMPRESS_ZLIB: u32 = 1;
/// LZO compression.
pub const DUMP_COMPRESS_LZO: u32 = 2;
/// Snappy compression.
pub const DUMP_COMPRESS_SNAPPY: u32 = 3;
/// Zstd compression.
pub const DUMP_COMPRESS_ZSTD: u32 = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dump_formats_distinct() {
        assert_ne!(KDUMP_FORMAT_ELF, KDUMP_FORMAT_COMPRESSED);
    }

    #[test]
    fn test_vmcoreinfo() {
        assert_eq!(VMCOREINFO_NOTE_NAME, "VMCOREINFO");
        assert_eq!(VMCOREINFO_MAX_SIZE, 4096);
    }

    #[test]
    fn test_seg_flags_powers_of_two() {
        let flags = [CRASH_SEG_READ, CRASH_SEG_WRITE, CRASH_SEG_EXEC];
        for flag in &flags {
            assert!(flag.is_power_of_two(), "0x{:x}", flag);
        }
    }

    #[test]
    fn test_seg_flags_no_overlap() {
        let flags = [CRASH_SEG_READ, CRASH_SEG_WRITE, CRASH_SEG_EXEC];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_mem_types_distinct() {
        let types = [
            CRASH_MEM_RAM, CRASH_MEM_RESERVED, CRASH_MEM_ACPI,
            CRASH_MEM_ACPI_NVS, CRASH_MEM_UNUSABLE,
            CRASH_MEM_PERSISTENT,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_compress_types_distinct() {
        let types = [
            DUMP_COMPRESS_NONE, DUMP_COMPRESS_ZLIB,
            DUMP_COMPRESS_LZO, DUMP_COMPRESS_SNAPPY,
            DUMP_COMPRESS_ZSTD,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }
}
