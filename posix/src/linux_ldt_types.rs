//! `<asm/ldt.h>` — x86 Local Descriptor Table entry constants.
//!
//! Constants for the `modify_ldt(2)` syscall on x86. Userspace
//! threading libraries that manage per-thread segment selectors
//! (legacy TLS on 32-bit, Wine, DOSEMU) consume these.

// ---------------------------------------------------------------------------
// modify_ldt() function codes (first argument)
// ---------------------------------------------------------------------------

/// Read LDT.
pub const LDT_FUNC_READ: u32 = 0;
/// Write LDT entry.
pub const LDT_FUNC_WRITE: u32 = 1;
/// Read default LDT.
pub const LDT_FUNC_READ_DEFAULT: u32 = 2;
/// Write LDT entry (legacy alias accepting old struct layout).
pub const LDT_FUNC_WRITE_LEGACY: u32 = 0x11;

// ---------------------------------------------------------------------------
// LDT layout limits
// ---------------------------------------------------------------------------

/// Number of LDT entries on x86 (also matches `LDT_ENTRIES`).
pub const LDT_ENTRIES: u32 = 8192;
/// Size of a single descriptor (bytes).
pub const LDT_ENTRY_SIZE: u32 = 8;

// ---------------------------------------------------------------------------
// user_desc.flags bit field positions
// ---------------------------------------------------------------------------

/// Segment is read/write (else read-only data / execute-only code).
pub const LDT_FLAG_READ_EXEC_ONLY: u32 = 1 << 0;
/// Limit is in pages (else bytes).
pub const LDT_FLAG_LIMIT_IN_PAGES: u32 = 1 << 1;
/// Segment is empty (no descriptor present).
pub const LDT_FLAG_SEG_NOT_PRESENT: u32 = 1 << 2;
/// Segment is 32-bit (else 16-bit).
pub const LDT_FLAG_USEABLE: u32 = 1 << 3;
/// Long mode bit (64-bit code segment).
pub const LDT_FLAG_LM: u32 = 1 << 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_func_codes_distinct() {
        let funcs = [
            LDT_FUNC_READ,
            LDT_FUNC_WRITE,
            LDT_FUNC_READ_DEFAULT,
            LDT_FUNC_WRITE_LEGACY,
        ];
        for i in 0..funcs.len() {
            for j in (i + 1)..funcs.len() {
                assert_ne!(funcs[i], funcs[j]);
            }
        }
    }

    #[test]
    fn test_ldt_layout_consistent() {
        // The full LDT is one page on classic x86: 8192 entries * 8 bytes
        // = 65536 bytes. Verify the constants compose to that.
        assert_eq!(LDT_ENTRIES * LDT_ENTRY_SIZE, 65536);
    }

    #[test]
    fn test_flag_bits_distinct_powers_of_two() {
        let flags = [
            LDT_FLAG_READ_EXEC_ONLY,
            LDT_FLAG_LIMIT_IN_PAGES,
            LDT_FLAG_SEG_NOT_PRESENT,
            LDT_FLAG_USEABLE,
            LDT_FLAG_LM,
        ];
        for &f in &flags {
            assert!(f.is_power_of_two());
        }
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }
}
