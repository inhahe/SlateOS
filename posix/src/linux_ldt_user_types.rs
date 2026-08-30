//! `<asm/ldt.h>` — x86 Local Descriptor Table for `modify_ldt(2)`.
//!
//! Wine, dosemu, and 32-bit threading libraries that need their own
//! per-thread segments call `modify_ldt` with the bit layout below.
//! Modern code uses GDT slots via `set_thread_area`, but the LDT
//! interface is still the right way to express custom code segments
//! for 16/32-bit guests.

// ---------------------------------------------------------------------------
// `modify_ldt(2)` function codes
// ---------------------------------------------------------------------------

/// Read the LDT into the caller's buffer.
pub const MODIFY_LDT_READ: u32 = 0;
/// Replace one LDT entry.
pub const MODIFY_LDT_WRITE: u32 = 1;
/// Read the default LDT layout.
pub const MODIFY_LDT_READ_DEFAULT: u32 = 2;
/// Replace one LDT entry only if it's currently invalid.
pub const MODIFY_LDT_WRITE_IF_EMPTY: u32 = 0x11;

/// Syscall number `__NR_modify_ldt` on x86_64.
pub const NR_MODIFY_LDT: u32 = 154;

// ---------------------------------------------------------------------------
// Sizes
// ---------------------------------------------------------------------------

/// Maximum number of LDT entries on x86 (8192 × 8 bytes = 64 KiB).
pub const LDT_ENTRIES: usize = 8192;
/// Each LDT entry is 8 bytes (one descriptor).
pub const LDT_ENTRY_SIZE: usize = 8;

// ---------------------------------------------------------------------------
// `user_desc` bitfield flag values (see `<asm/ldt.h>`)
// ---------------------------------------------------------------------------

pub const MODIFY_LDT_CONTENTS_DATA: u32 = 0;
pub const MODIFY_LDT_CONTENTS_STACK: u32 = 1;
pub const MODIFY_LDT_CONTENTS_CODE: u32 = 2;

/// Bit clear ⇒ segment present; bit set ⇒ "not present" (kernel quirk).
pub const MODIFY_LDT_SEG_NOT_PRESENT: u32 = 1 << 0;
pub const MODIFY_LDT_READ_EXEC_ONLY: u32 = 1 << 1;
pub const MODIFY_LDT_LIMIT_IN_PAGES: u32 = 1 << 2;
pub const MODIFY_LDT_SEG_32BIT: u32 = 1 << 3;
pub const MODIFY_LDT_USEABLE: u32 = 1 << 4;
pub const MODIFY_LDT_LM: u32 = 1 << 5;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_function_codes_distinct() {
        let f = [
            MODIFY_LDT_READ,
            MODIFY_LDT_WRITE,
            MODIFY_LDT_READ_DEFAULT,
            MODIFY_LDT_WRITE_IF_EMPTY,
        ];
        for i in 0..f.len() {
            for j in (i + 1)..f.len() {
                assert_ne!(f[i], f[j]);
            }
        }
        assert_eq!(MODIFY_LDT_WRITE_IF_EMPTY, 0x11);
    }

    #[test]
    fn test_syscall_number() {
        assert_eq!(NR_MODIFY_LDT, 154);
    }

    #[test]
    fn test_size_constants() {
        // 8192 × 8 = 64 KiB — the full LDT footprint.
        assert_eq!(LDT_ENTRIES, 8192);
        assert_eq!(LDT_ENTRY_SIZE, 8);
        assert_eq!(LDT_ENTRIES * LDT_ENTRY_SIZE, 0x10000);
    }

    #[test]
    fn test_contents_codes_dense() {
        let c = [
            MODIFY_LDT_CONTENTS_DATA,
            MODIFY_LDT_CONTENTS_STACK,
            MODIFY_LDT_CONTENTS_CODE,
        ];
        for (i, &v) in c.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_seg_flags_pow2() {
        for &b in &[
            MODIFY_LDT_SEG_NOT_PRESENT,
            MODIFY_LDT_READ_EXEC_ONLY,
            MODIFY_LDT_LIMIT_IN_PAGES,
            MODIFY_LDT_SEG_32BIT,
            MODIFY_LDT_USEABLE,
            MODIFY_LDT_LM,
        ] {
            assert!(b.is_power_of_two());
        }
    }
}
