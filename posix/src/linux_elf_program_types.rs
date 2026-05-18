//! `<elf.h>` — ELF program header (segment) type and flag constants.
//!
//! Program headers describe segments for runtime loading. The kernel
//! and dynamic linker use these to map the executable into memory,
//! locate the dynamic section, find the interpreter path, and set
//! up the stack and TLS.

// ---------------------------------------------------------------------------
// Program header types (p_type)
// ---------------------------------------------------------------------------

/// Unused entry.
pub const PT_NULL: u32 = 0;
/// Loadable segment (mmap into memory).
pub const PT_LOAD: u32 = 1;
/// Dynamic linking information.
pub const PT_DYNAMIC: u32 = 2;
/// Interpreter pathname (.interp section).
pub const PT_INTERP: u32 = 3;
/// Auxiliary information (notes).
pub const PT_NOTE: u32 = 4;
/// Reserved (unused).
pub const PT_SHLIB: u32 = 5;
/// Program header table itself.
pub const PT_PHDR: u32 = 6;
/// Thread-local storage template.
pub const PT_TLS: u32 = 7;
/// GNU EH frame (exception handling).
pub const PT_GNU_EH_FRAME: u32 = 0x6474_E550;
/// GNU stack permissions.
pub const PT_GNU_STACK: u32 = 0x6474_E551;
/// GNU read-only after relocation.
pub const PT_GNU_RELRO: u32 = 0x6474_E552;
/// GNU property note.
pub const PT_GNU_PROPERTY: u32 = 0x6474_E553;

// ---------------------------------------------------------------------------
// Program header flags (p_flags)
// ---------------------------------------------------------------------------

/// Segment is executable.
pub const PF_X: u32 = 0x01;
/// Segment is writable.
pub const PF_W: u32 = 0x02;
/// Segment is readable.
pub const PF_R: u32 = 0x04;

// ---------------------------------------------------------------------------
// Common segment permission combinations
// ---------------------------------------------------------------------------

/// Read + Execute (code).
pub const PF_RX: u32 = PF_R | PF_X;
/// Read + Write (data, BSS).
pub const PF_RW: u32 = PF_R | PF_W;
/// Read + Write + Execute (rare, insecure).
pub const PF_RWX: u32 = PF_R | PF_W | PF_X;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_segment_types_distinct() {
        let types = [
            PT_NULL, PT_LOAD, PT_DYNAMIC, PT_INTERP,
            PT_NOTE, PT_SHLIB, PT_PHDR, PT_TLS,
            PT_GNU_EH_FRAME, PT_GNU_STACK, PT_GNU_RELRO,
            PT_GNU_PROPERTY,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        assert!(PF_X.is_power_of_two());
        assert!(PF_W.is_power_of_two());
        assert!(PF_R.is_power_of_two());
        assert_eq!(PF_X & PF_W, 0);
        assert_eq!(PF_W & PF_R, 0);
        assert_eq!(PF_X & PF_R, 0);
    }

    #[test]
    fn test_permission_combos() {
        assert_eq!(PF_RX, PF_R | PF_X);
        assert_eq!(PF_RW, PF_R | PF_W);
        assert_eq!(PF_RWX, PF_R | PF_W | PF_X);
    }

    #[test]
    fn test_load_is_one() {
        assert_eq!(PT_LOAD, 1);
    }
}
