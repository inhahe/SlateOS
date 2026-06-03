//! `<asm/prctl.h>` — Thread-Local Storage (TLS) constants.
//!
//! TLS provides per-thread private data accessed through segment
//! registers (FS/GS on x86_64). Each thread has its own TLS area
//! pointed to by FS_BASE (for userspace, via arch_prctl) or GS_BASE
//! (for kernel, set by swapgs on syscall entry). The C runtime stores
//! errno, stack canary, and thread-local variables in TLS. On x86_64,
//! ARCH_SET_FS/ARCH_GET_FS control the FS base for userspace TLS.

// ---------------------------------------------------------------------------
// arch_prctl operations (x86_64 TLS)
// ---------------------------------------------------------------------------

/// Set the FS base address (user TLS pointer).
pub const ARCH_SET_FS: u32 = 0x1002;
/// Get the FS base address.
pub const ARCH_GET_FS: u32 = 0x1003;
/// Set the GS base address (usually kernel-reserved).
pub const ARCH_SET_GS: u32 = 0x1001;
/// Get the GS base address.
pub const ARCH_GET_GS: u32 = 0x1004;

// ---------------------------------------------------------------------------
// GDT/LDT TLS entry indices (32-bit compat)
// ---------------------------------------------------------------------------

/// First TLS entry in GDT.
pub const GDT_ENTRY_TLS_MIN: u32 = 6;
/// Last TLS entry in GDT.
pub const GDT_ENTRY_TLS_MAX: u32 = 8;
/// Number of TLS entries available.
pub const GDT_ENTRY_TLS_ENTRIES: u32 = 3;

// ---------------------------------------------------------------------------
// set_thread_area flags (32-bit ABI)
// ---------------------------------------------------------------------------

/// Entry number is read-write (kernel assigns if -1).
pub const TLS_FLAG_WRITABLE: u32 = 0x01;
/// Limit is in pages (not bytes).
pub const TLS_FLAG_LIMIT_PAGES: u32 = 0x02;
/// Segment is 32-bit.
pub const TLS_FLAG_32BIT: u32 = 0x04;
/// Contents are data (not code).
pub const TLS_FLAG_CONTENTS_DATA: u32 = 0x00;
/// Contents are stack (expand-down segment).
pub const TLS_FLAG_CONTENTS_STACK: u32 = 0x08;
/// Contents are code (execute-read).
pub const TLS_FLAG_CONTENTS_CODE: u32 = 0x10;

// ---------------------------------------------------------------------------
// TLS alignment
// ---------------------------------------------------------------------------

/// Required alignment for TLS area (bytes).
pub const TLS_ALIGN: u32 = 16;
/// Minimum TLS size (for static TLS block).
pub const TLS_MIN_SIZE: u32 = 64;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_arch_prctl_ops_distinct() {
        let ops = [ARCH_SET_FS, ARCH_GET_FS, ARCH_SET_GS, ARCH_GET_GS];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_gdt_entries() {
        assert!(GDT_ENTRY_TLS_MIN < GDT_ENTRY_TLS_MAX);
        assert_eq!(
            GDT_ENTRY_TLS_MAX - GDT_ENTRY_TLS_MIN + 1,
            GDT_ENTRY_TLS_ENTRIES
        );
    }

    #[test]
    fn test_tls_alignment() {
        assert!(TLS_ALIGN.is_power_of_two());
        assert!(TLS_MIN_SIZE >= TLS_ALIGN);
    }
}
