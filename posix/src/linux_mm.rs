//! `<linux/mm.h>` — Memory management constants.
//!
//! The mm subsystem manages virtual memory, page tables, VMAs
//! (virtual memory areas), and page cache. This module defines
//! VMA flags, page flags, and memory protection constants used
//! throughout the kernel and exposed to userspace via mmap/mprotect.

// ---------------------------------------------------------------------------
// VM flags (vm_flags in vm_area_struct)
// ---------------------------------------------------------------------------

/// Readable.
pub const VM_READ: u64 = 1 << 0;
/// Writable.
pub const VM_WRITE: u64 = 1 << 1;
/// Executable.
pub const VM_EXEC: u64 = 1 << 2;
/// Shared mapping.
pub const VM_SHARED: u64 = 1 << 3;
/// May read.
pub const VM_MAYREAD: u64 = 1 << 4;
/// May write.
pub const VM_MAYWRITE: u64 = 1 << 5;
/// May execute.
pub const VM_MAYEXEC: u64 = 1 << 6;
/// May share.
pub const VM_MAYSHARE: u64 = 1 << 7;
/// Grows down (stack).
pub const VM_GROWSDOWN: u64 = 1 << 8;
/// Grows up.
pub const VM_GROWSUP: u64 = 1 << 9;
/// Page frame number (direct physical mapping).
pub const VM_PFNMAP: u64 = 1 << 10;
/// Locked (mlock).
pub const VM_LOCKED: u64 = 1 << 11;
/// I/O mapping.
pub const VM_IO: u64 = 1 << 12;
/// Sequential read (readahead hint).
pub const VM_SEQ_READ: u64 = 1 << 13;
/// Random read (disable readahead).
pub const VM_RAND_READ: u64 = 1 << 14;
/// Don't copy on fork.
pub const VM_DONTCOPY: u64 = 1 << 15;
/// Don't expand (mremap).
pub const VM_DONTEXPAND: u64 = 1 << 16;
/// Don't dump in core file.
pub const VM_DONTDUMP: u64 = 1 << 17;
/// Account for memory usage.
pub const VM_ACCOUNT: u64 = 1 << 18;
/// Non-farmable.
pub const VM_NORESERVE: u64 = 1 << 19;
/// Huge page TLB.
pub const VM_HUGETLB: u64 = 1 << 20;
/// Synchronized page fault.
pub const VM_SYNC: u64 = 1 << 21;
/// Architecture-specific bit 1.
pub const VM_ARCH_1: u64 = 1 << 22;
/// Wipe on fork.
pub const VM_WIPEONFORK: u64 = 1 << 23;
/// Don't fork.
pub const VM_DONTFORK: u64 = VM_DONTCOPY;

// ---------------------------------------------------------------------------
// Page fault return codes
// ---------------------------------------------------------------------------

/// Fault handled, page installed.
pub const VM_FAULT_NOPAGE: u32 = 0x0001;
/// Minor fault (no I/O needed).
pub const VM_FAULT_MINOR: u32 = 0x0002;
/// Major fault (I/O required).
pub const VM_FAULT_MAJOR: u32 = 0x0004;
/// OOM during fault.
pub const VM_FAULT_OOM: u32 = 0x0008;
/// Signal delivered.
pub const VM_FAULT_SIGBUS: u32 = 0x0010;
/// Retry with mmap_lock.
pub const VM_FAULT_RETRY: u32 = 0x0020;
/// Fallback to huge page.
pub const VM_FAULT_FALLBACK: u32 = 0x0040;
/// Done flag.
pub const VM_FAULT_DONE_COW: u32 = 0x0080;
/// Needdsync.
pub const VM_FAULT_NEEDDSYNC: u32 = 0x0100;

// ---------------------------------------------------------------------------
// GFP (Get Free Pages) flags — common subset
// ---------------------------------------------------------------------------

/// Kernel allocation.
pub const GFP_KERNEL: u32 = 0x0CC0;
/// Atomic allocation (can't sleep).
pub const GFP_ATOMIC: u32 = 0x0500;
/// User allocation.
pub const GFP_USER: u32 = 0x0DC0;
/// High memory allocation.
pub const GFP_HIGHUSER: u32 = 0x0DD0;
/// No wait (don't sleep).
pub const GFP_NOWAIT: u32 = 0x0400;
/// DMA-capable memory.
pub const GFP_DMA: u32 = 0x01;
/// Zero the allocated page.
pub const __GFP_ZERO: u32 = 0x0100;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vm_flags_powers_of_two() {
        let flags: [u64; 20] = [
            VM_READ, VM_WRITE, VM_EXEC, VM_SHARED,
            VM_MAYREAD, VM_MAYWRITE, VM_MAYEXEC, VM_MAYSHARE,
            VM_GROWSDOWN, VM_GROWSUP, VM_PFNMAP, VM_LOCKED,
            VM_IO, VM_SEQ_READ, VM_RAND_READ, VM_DONTCOPY,
            VM_DONTEXPAND, VM_DONTDUMP, VM_ACCOUNT, VM_NORESERVE,
        ];
        for flag in &flags {
            assert!(flag.is_power_of_two(), "0x{:x}", flag);
        }
    }

    #[test]
    fn test_vm_flags_no_overlap() {
        let flags: [u64; 20] = [
            VM_READ, VM_WRITE, VM_EXEC, VM_SHARED,
            VM_MAYREAD, VM_MAYWRITE, VM_MAYEXEC, VM_MAYSHARE,
            VM_GROWSDOWN, VM_GROWSUP, VM_PFNMAP, VM_LOCKED,
            VM_IO, VM_SEQ_READ, VM_RAND_READ, VM_DONTCOPY,
            VM_DONTEXPAND, VM_DONTDUMP, VM_ACCOUNT, VM_NORESERVE,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_fault_codes_distinct() {
        let codes = [
            VM_FAULT_NOPAGE, VM_FAULT_MINOR, VM_FAULT_MAJOR,
            VM_FAULT_OOM, VM_FAULT_SIGBUS, VM_FAULT_RETRY,
            VM_FAULT_FALLBACK, VM_FAULT_DONE_COW, VM_FAULT_NEEDDSYNC,
        ];
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                assert_ne!(codes[i], codes[j]);
            }
        }
    }

    #[test]
    fn test_dontfork_alias() {
        assert_eq!(VM_DONTFORK, VM_DONTCOPY);
    }
}
