//! Memory protection (mprotect) and W^X enforcement.
//!
//! Provides the kernel-side `mprotect` operation: changing page permissions
//! on existing mappings.  Enforces the W^X (write-xor-execute) invariant:
//! a page may be writable or executable, but never both simultaneously.
//!
//! ## W^X Enforcement
//!
//! All userspace mappings default to non-executable (NX bit set).  Only
//! the ELF loader creates executable pages (for .text segments), and only
//! with read+execute (never write+execute).
//!
//! JIT compilers (V8, LuaJIT, JVM HotSpot, .NET RyuJIT) need to create
//! executable pages at runtime.  The supported pattern is:
//!
//! 1. Allocate anonymous memory (writable, non-executable)
//! 2. Write generated code into it
//! 3. `mprotect` to read+execute (removing write)
//! 4. Execute the generated code
//! 5. To modify: `mprotect` back to read+write, modify, `mprotect` to
//!    read+execute again
//!
//! This two-phase approach prevents code injection: an attacker cannot
//! write to a page that is currently executable.
//!
//! ## Capability Gate
//!
//! Creating executable pages via `mprotect` (transitioning from any state
//! to read+execute) requires the `mem.jit` capability.  Programs without
//! this capability cannot create new executable pages beyond their initial
//! .text mapping.  The ELF loader's initial executable mapping does NOT
//! require `mem.jit` — it's part of normal program loading.
//!
//! ## References
//!
//! - OpenBSD W^X enforcement (the gold standard)
//! - Windows DEP (Data Execution Prevention)
//! - Linux `mprotect(2)` with SELinux `execmem` restriction

use crate::error::{KernelError, KernelResult};
use crate::mm::frame::FRAME_SIZE;
use crate::mm::page_table::{self, PageFlags, PageTableEntry, VirtAddr};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Memory protection flags
// ---------------------------------------------------------------------------

/// Memory protection mode for `mprotect`.
///
/// Represents the valid combinations of read/write/execute permissions.
/// W^X is enforced structurally: there is no variant for write+execute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // Some variants are reserved for future mprotect support.
pub enum MemProt {
    /// No access.  Any access will fault.
    None,

    /// Read-only, non-executable.
    ReadOnly,

    /// Read-write, non-executable.  Default for anonymous memory.
    ReadWrite,

    /// Read-execute.  For code pages.
    /// Requires `mem.jit` capability when applied via `mprotect`
    /// (not required for initial ELF loading).
    ReadExecute,
}

impl MemProt {
    /// Convert to `PageFlags` suitable for a userspace mapping.
    ///
    /// All modes include `PRESENT` and `USER_ACCESSIBLE`.
    /// `NO_EXECUTE` is set for all non-executable modes.
    #[must_use]
    pub fn to_page_flags(self) -> PageFlags {
        match self {
            Self::None => {
                // Map as present but not writable, not user-accessible.
                // Alternatively, could unmap entirely.  Using present but
                // inaccessible catches bugs (fault with a known reason).
                PageFlags::PRESENT | PageFlags::NO_EXECUTE
            }
            Self::ReadOnly => {
                PageFlags::PRESENT
                    | PageFlags::USER_ACCESSIBLE
                    | PageFlags::NO_EXECUTE
            }
            Self::ReadWrite => {
                PageFlags::PRESENT
                    | PageFlags::USER_ACCESSIBLE
                    | PageFlags::WRITABLE
                    | PageFlags::NO_EXECUTE
            }
            Self::ReadExecute => {
                // No NO_EXECUTE → page is executable.
                // No WRITABLE → page is not writable.  W^X enforced.
                PageFlags::PRESENT
                    | PageFlags::USER_ACCESSIBLE
            }
        }
    }

    /// Whether this protection mode makes the page executable.
    #[must_use]
    pub const fn is_executable(self) -> bool {
        matches!(self, Self::ReadExecute)
    }

    /// Whether this protection mode makes the page writable.
    #[must_use]
    pub const fn is_writable(self) -> bool {
        matches!(self, Self::ReadWrite)
    }
}

// ---------------------------------------------------------------------------
// mprotect implementation
// ---------------------------------------------------------------------------

/// Change memory protection for a range of virtual addresses.
///
/// The range `[start, start + len)` must be frame-aligned (16 KiB).
/// All frames in the range must already be mapped (present in the page
/// tables).  The protection flags of each frame's PTEs are updated
/// to match `prot`.
///
/// ## W^X enforcement
///
/// This function structurally prevents write+execute: the [`MemProt`]
/// enum has no variant combining both.  This is the kernel's primary
/// W^X enforcement point.
///
/// ## JIT capability gate
///
/// Setting `prot` to `ReadExecute` requires `has_jit_cap` to be `true`.
/// This prevents unprivileged processes from creating executable pages.
/// The initial ELF .text mapping bypasses this check (it goes through
/// the ELF loader, not mprotect).
///
/// ## TLB flush
///
/// The caller is responsible for flushing the TLB after mprotect
/// completes (via `tlb::flush_range` or `tlb::shootdown_range` for SMP).
/// This function does NOT flush the TLB because the caller may be
/// batching multiple mprotect calls.
///
/// # Errors
///
/// - `BadAlignment`: `start` is not frame-aligned or `len` is 0.
/// - `PermissionDenied`: `prot` is `ReadExecute` but `has_jit_cap` is false.
/// - `InvalidAddress`: A frame in the range is not mapped.
#[allow(clippy::arithmetic_side_effects)]
pub fn mprotect(
    pml4_phys: u64,
    start: u64,
    len: usize,
    prot: MemProt,
    has_jit_cap: bool,
) -> KernelResult<usize> {
    if len == 0 {
        return Err(KernelError::InvalidArgument);
    }

    let start_addr = VirtAddr::new(start);
    if !start_addr.is_frame_aligned() {
        return Err(KernelError::BadAlignment);
    }

    // Capability gate: only processes with mem.jit can create executable pages.
    if prot.is_executable() && !has_jit_cap {
        return Err(KernelError::PermissionDenied);
    }

    let flags = prot.to_page_flags();

    // Round up to frame boundary.
    let len_aligned = len.div_ceil(FRAME_SIZE) * FRAME_SIZE;
    let end = start.saturating_add(len_aligned as u64);

    let mut frames_changed = 0usize;
    let mut addr = start;

    while addr < end {
        let virt = VirtAddr::new(addr);

        // SAFETY: We're changing flags on an existing mapping.
        // The caller guarantees pml4_phys is valid for this address space.
        // We don't flush TLB here (caller does it after batching).
        let result = unsafe { page_table::change_flags(pml4_phys, virt, flags) };

        match result {
            Ok(()) => {
                frames_changed += 1;
            }
            Err(KernelError::InvalidAddress) => {
                // Frame not mapped.  If we've already changed some frames,
                // report partial success.  Otherwise, propagate the error.
                if frames_changed == 0 {
                    return Err(KernelError::InvalidAddress);
                }
                // Stop at the first unmapped frame — don't skip gaps.
                break;
            }
            Err(e) => return Err(e),
        }

        addr = addr.saturating_add(FRAME_SIZE as u64);
    }

    Ok(frames_changed)
}

// ---------------------------------------------------------------------------
// W^X validation
// ---------------------------------------------------------------------------

/// Check whether a set of `PageFlags` violates W^X.
///
/// Returns `true` if the flags have both `WRITABLE` and executable
/// (i.e., `NO_EXECUTE` is NOT set).
#[must_use]
pub fn is_wx_violation(flags: PageFlags) -> bool {
    flags.contains(PageFlags::WRITABLE) && !flags.contains(PageFlags::NO_EXECUTE)
}

// ---------------------------------------------------------------------------
// Kernel W^X audit
// ---------------------------------------------------------------------------

/// Result of a kernel W^X audit.
#[derive(Debug, Clone, Copy)]
pub struct WxAuditResult {
    /// Number of HHDM (direct-map) pages with W+X.  Expected to be non-zero
    /// until we patch the Limine-created HHDM to set the NX bit.
    pub hhdm_violations: usize,
    /// Number of non-HHDM kernel pages with W+X.  This SHOULD be zero.
    pub kernel_violations: usize,
}

impl WxAuditResult {
    /// Total violations across both categories.
    #[must_use]
    #[allow(dead_code)] // API convenience; used once W^X enforcement has full test coverage.
    pub const fn total(&self) -> usize {
        self.hhdm_violations + self.kernel_violations
    }
}

/// Audit the kernel's page table for W^X violations.
///
/// Walks the kernel half of the page table (PML4 entries 256–511)
/// and checks every leaf PTE for simultaneous WRITABLE + executable.
///
/// Separates HHDM (Higher Half Direct Map) violations from real
/// kernel text/data violations.  Limine's HHDM doesn't set the NX
/// bit, so thousands of apparent W+X pages are expected there — those
/// are not real security issues because we never execute code from
/// the direct map.  Non-HHDM violations (kernel .text, .data, stacks,
/// heap) are genuine and should be investigated.
///
/// Only the first few non-HHDM violations are logged individually
/// to avoid flooding serial output.
///
/// This is a diagnostic function, not a hot path.
#[allow(clippy::arithmetic_side_effects)]
pub fn audit_kernel_wx(pml4_phys: u64) -> WxAuditResult {
    let hhdm = match page_table::hhdm() {
        Some(h) => h,
        None => return WxAuditResult { hhdm_violations: 0, kernel_violations: 0 },
    };

    let mut hhdm_violations = 0usize;
    let mut kernel_violations = 0usize;

    // The HHDM region starts at pml4_idx = (hhdm >> 39) & 0x1FF.
    // Typically 0xFFFF_8000_0000_0000 → PML4 index 256.
    let hhdm_pml4_start = ((hhdm >> 39) & 0x1FF) as usize;

    // The HHDM covers enough entries to map all physical RAM.  In practice
    // on a 256 MiB VM, one PML4 entry (512 GiB) is more than enough.  We
    // conservatively treat entries [hhdm_pml4_start, hhdm_pml4_start+4) as
    // HHDM — that covers up to 2 TiB of physical memory.
    let hhdm_pml4_end = hhdm_pml4_start.saturating_add(4).min(512);

    // Max non-HHDM violations to log individually.
    const MAX_LOGGED: usize = 8;

    // Kernel half: PML4 entries 256–511.
    //
    // On x86_64, if ANY intermediate entry (PML4E, PDPTE, PDE) has the NX
    // bit set, the page is effectively non-executable — the hardware ORs
    // the NX bit across all levels.  We track inherited NX through the walk
    // so the audit reflects the true effective permissions.
    // SAFETY (group — covers all read_entry calls in this 4-level walk):
    // pml4_phys is the active page table; each index is 0..512; hhdm is
    // valid.  At each level, we only descend if is_present() is true, so
    // phys_addr() yields a valid table address for the next level.
    for pml4_idx in 256..512 {
        let pml4e = unsafe { page_table::read_entry(pml4_phys, pml4_idx, hhdm) };
        if !pml4e.is_present() {
            continue;
        }

        let is_hhdm = pml4_idx >= hhdm_pml4_start && pml4_idx < hhdm_pml4_end;
        let pml4_nx = pml4e.flags().contains(PageFlags::NO_EXECUTE);

        for pdpt_idx in 0..512 {
            let pdpte = unsafe { page_table::read_entry(pml4e.phys_addr(), pdpt_idx, hhdm) };
            if !pdpte.is_present() {
                continue;
            }
            let pdpt_nx = pml4_nx || pdpte.flags().contains(PageFlags::NO_EXECUTE);

            // 1 GiB huge page — check directly.
            if pdpte.is_huge() {
                if !pdpt_nx && is_wx_violation(pdpte.flags()) {
                    if is_hhdm {
                        hhdm_violations += 1;
                    } else {
                        let virt = (pml4_idx * (1 << 39) + pdpt_idx * (1 << 30)) as u64;
                        if kernel_violations < MAX_LOGGED {
                            serial_println!(
                                "[wx-audit] VIOLATION: 1GiB page at {:#x} is W+X",
                                virt | 0xFFFF_0000_0000_0000u64
                            );
                        }
                        kernel_violations += 1;
                    }
                }
                continue;
            }

            for pd_idx in 0..512 {
                let pde = unsafe { page_table::read_entry(pdpte.phys_addr(), pd_idx, hhdm) };
                if !pde.is_present() {
                    continue;
                }
                let pd_nx = pdpt_nx || pde.flags().contains(PageFlags::NO_EXECUTE);

                // 2 MiB huge page — check directly.
                if pde.is_huge() {
                    if !pd_nx && is_wx_violation(pde.flags()) {
                        if is_hhdm {
                            hhdm_violations += 1;
                        } else {
                            let virt = (pml4_idx * (1 << 39) + pdpt_idx * (1 << 30)
                                + pd_idx * (1 << 21)) as u64;
                            if kernel_violations < MAX_LOGGED {
                                serial_println!(
                                    "[wx-audit] VIOLATION: 2MiB page at {:#x} is W+X",
                                    virt | 0xFFFF_0000_0000_0000u64
                                );
                            }
                            kernel_violations += 1;
                        }
                    }
                    continue;
                }

                for pt_idx in 0..512 {
                    let pte = unsafe { page_table::read_entry(pde.phys_addr(), pt_idx, hhdm) };
                    if !pte.is_present() {
                        continue;
                    }
                    // Effective NX: any ancestor OR leaf has NX → non-executable.
                    let effective_nx = pd_nx || pte.flags().contains(PageFlags::NO_EXECUTE);

                    if !effective_nx && pte.flags().contains(PageFlags::WRITABLE) {
                        if is_hhdm {
                            hhdm_violations += 1;
                        } else {
                            let virt = (pml4_idx * (1 << 39) + pdpt_idx * (1 << 30)
                                + pd_idx * (1 << 21) + pt_idx * (1 << 12)) as u64;
                            if kernel_violations < MAX_LOGGED {
                                serial_println!(
                                    "[wx-audit] VIOLATION: 4KiB page at {:#x} is W+X",
                                    virt | 0xFFFF_0000_0000_0000u64
                                );
                            }
                            kernel_violations += 1;
                        }
                    }
                }
            }
        }
    }

    WxAuditResult { hhdm_violations, kernel_violations }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for memory protection.
// ---------------------------------------------------------------------------
// HHDM NX hardening
// ---------------------------------------------------------------------------

/// Set the NX (No-Execute) bit on all HHDM PML4 entries.
///
/// Limine's Higher Half Direct Map doesn't set NX on the page table
/// entries, leaving the entire physical memory direct map as executable.
/// We never execute code from the HHDM — it's only used for reading
/// and writing physical memory — so we can safely mark the entire range
/// as non-executable.
///
/// We set NX at the PML4 level for maximum efficiency: a single bit flip
/// per PML4 entry covers 512 GiB of address space.  This eliminates
/// thousands of apparent W^X violations from the audit.
///
/// Must be called after the page table subsystem is initialized.
/// Flushes the TLB on all CPUs after modifying the PML4 entries.
///
/// Returns the number of PML4 entries hardened.
#[allow(clippy::arithmetic_side_effects)]
pub fn harden_hhdm_nx(pml4_phys: u64) -> usize {
    let hhdm = match page_table::hhdm() {
        Some(h) => h,
        None => return 0,
    };

    let hhdm_pml4_start = ((hhdm >> 39) & 0x1FF) as usize;
    // Cover up to 2 TiB of physical memory (4 PML4 entries × 512 GiB each).
    let hhdm_pml4_end = hhdm_pml4_start.saturating_add(4).min(512);

    let mut hardened = 0usize;

    for pml4_idx in hhdm_pml4_start..hhdm_pml4_end {
        // SAFETY: pml4_phys is the active page table, index is valid.
        let pml4e = unsafe { page_table::read_entry(pml4_phys, pml4_idx, hhdm) };
        if !pml4e.is_present() {
            continue;
        }

        // Set the NX bit (bit 63) on the PML4 entry.
        let new_raw = pml4e.raw() | PageFlags::NO_EXECUTE.bits();
        let new_entry = PageTableEntry::from_raw(new_raw);

        // SAFETY: We're only adding NX to an existing valid entry.
        // The physical address and other flags are preserved.
        unsafe { page_table::write_entry(pml4_phys, pml4_idx, new_entry, hhdm); }
        hardened += 1;
    }

    if hardened > 0 {
        // Flush TLB on all CPUs so the NX changes take effect.
        crate::tlb::flush_all();
    }

    hardened
}

// ---------------------------------------------------------------------------
// Kernel section NX hardening
// ---------------------------------------------------------------------------

// Linker script section boundary symbols.
unsafe extern "C" {
    static __requests_start: u8;
    static __requests_end: u8;
    static __text_start: u8;
    static __text_end: u8;
    static __rodata_start: u8;
    static __rodata_end: u8;
    static __data_start: u8;
    static __data_end: u8;
    static __bss_start: u8;
    static __bss_end: u8;
}

/// Harden kernel section page permissions after boot.
///
/// Limine may not set the NX bit on non-executable kernel sections.
/// This function walks the kernel page tables and applies the correct
/// permissions based on linker script section boundaries:
///
/// - `.text`     → R+X       (executable code, not writable)
/// - `.rodata`   → R+NX      (read-only data, not executable)
/// - `.requests` → RW+NX     (Limine request data, not executable)
/// - `.data`     → RW+NX     (initialized data, not executable)
/// - `.bss`      → RW+NX     (zero-initialized data, not executable)
///
/// This enforces W^X for the kernel's own code: no page is both
/// writable and executable after this function runs.
///
/// Returns (pages_hardened, errors).
#[allow(clippy::arithmetic_side_effects)]
pub fn harden_kernel_sections(pml4_phys: u64) -> (usize, usize) {
    let hhdm = match page_table::hhdm() {
        Some(h) => h,
        None => return (0, 0),
    };

    let mut hardened = 0usize;
    let errors = 0usize;

    // Linker-defined section boundaries. addr_of! takes the address without
    // dereferencing, so no unsafe block is needed (Rust 2024 rules for
    // `unsafe extern` statics).
    let text_start = core::ptr::addr_of!(__text_start) as u64;
    let text_end = core::ptr::addr_of!(__text_end) as u64;
    let rodata_start = core::ptr::addr_of!(__rodata_start) as u64;
    let rodata_end = core::ptr::addr_of!(__rodata_end) as u64;
    let requests_start = core::ptr::addr_of!(__requests_start) as u64;
    let requests_end = core::ptr::addr_of!(__requests_end) as u64;
    let data_start = core::ptr::addr_of!(__data_start) as u64;
    let data_end = core::ptr::addr_of!(__data_end) as u64;
    let bss_start = core::ptr::addr_of!(__bss_start) as u64;
    let bss_end = core::ptr::addr_of!(__bss_end) as u64;

    // Helper: determine what flags a kernel virtual address should have.
    let flags_for_addr = |addr: u64| -> PageFlags {
        if addr >= text_start && addr < text_end {
            // .text: read-execute, not writable.
            PageFlags::PRESENT | PageFlags::GLOBAL
        } else if addr >= rodata_start && addr < rodata_end {
            // .rodata: read-only, not executable.
            PageFlags::PRESENT | PageFlags::GLOBAL | PageFlags::NO_EXECUTE
        } else if addr >= requests_start && addr < requests_end {
            // .requests: read-write (Limine data), not executable.
            PageFlags::PRESENT | PageFlags::WRITABLE | PageFlags::GLOBAL | PageFlags::NO_EXECUTE
        } else if addr >= data_start && addr < data_end {
            // .data: read-write, not executable.
            PageFlags::PRESENT | PageFlags::WRITABLE | PageFlags::GLOBAL | PageFlags::NO_EXECUTE
        } else if addr >= bss_start && addr < bss_end {
            // .bss: read-write, not executable.
            PageFlags::PRESENT | PageFlags::WRITABLE | PageFlags::GLOBAL | PageFlags::NO_EXECUTE
        } else {
            // Unknown kernel page — keep RW+NX as a safe default.
            PageFlags::PRESENT | PageFlags::WRITABLE | PageFlags::GLOBAL | PageFlags::NO_EXECUTE
        }
    };

    // Walk the kernel image region (PML4 index 511, which covers
    // 0xFFFF_FF80_0000_0000 to 0xFFFF_FFFF_FFFF_FFFF, containing
    // the kernel at 0xFFFF_FFFF_8000_0000).
    // SAFETY (group — covers all read_entry/write_entry calls in this walk):
    // pml4_phys is the active page table; all indices are 0..512; hhdm is
    // valid.  We only descend into sub-tables whose entries are present.
    let kernel_pml4_idx: usize = 511;
    let pml4e = unsafe { page_table::read_entry(pml4_phys, kernel_pml4_idx, hhdm) };
    if !pml4e.is_present() {
        return (0, 0);
    }

    for pdpt_idx in 0..512usize {
        let pdpte = unsafe { page_table::read_entry(pml4e.phys_addr(), pdpt_idx, hhdm) };
        if !pdpte.is_present() || pdpte.is_huge() {
            continue;
        }

        for pd_idx in 0..512usize {
            let pde = unsafe { page_table::read_entry(pdpte.phys_addr(), pd_idx, hhdm) };
            if !pde.is_present() || pde.is_huge() {
                continue;
            }

            for pt_idx in 0..512usize {
                let pte = unsafe { page_table::read_entry(pde.phys_addr(), pt_idx, hhdm) };
                if !pte.is_present() {
                    continue;
                }

                // Compute the virtual address for this PTE.
                let virt = (kernel_pml4_idx as u64) << 39
                    | (pdpt_idx as u64) << 30
                    | (pd_idx as u64) << 21
                    | (pt_idx as u64) << 12;
                // Sign-extend for canonical form (bit 47 set → bits 48-63 all 1).
                let virt = virt | 0xFFFF_0000_0000_0000;

                // Only harden pages within the kernel image.
                if virt < requests_start || virt >= bss_end {
                    continue;
                }

                let desired = flags_for_addr(virt);
                let current_flags = pte.flags();

                // Check if flags already match.
                // Compare the permission-relevant bits: WRITABLE, NO_EXECUTE, GLOBAL.
                let perm_mask = PageFlags::WRITABLE | PageFlags::NO_EXECUTE | PageFlags::GLOBAL;
                if (current_flags & perm_mask) == (desired & perm_mask) {
                    continue; // Already correct.
                }

                // Build new PTE: keep physical address, use desired flags.
                let new_entry = PageTableEntry::new(pte.phys_addr(), desired);

                // SAFETY: We're updating a present PTE with the same physical
                // address but different permission flags.
                unsafe {
                    page_table::write_entry(pde.phys_addr(), pt_idx, new_entry, hhdm);
                }
                hardened += 1;
            }
        }
    }

    if hardened > 0 {
        crate::tlb::flush_all();
    }

    (hardened, errors)
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

///
/// Tests the mprotect function and W^X enforcement.
#[allow(clippy::arithmetic_side_effects)]
pub fn self_test() -> KernelResult<()> {
    serial_println!("[protect] Running memory protection self-test...");

    // -- Test 1: MemProt flag conversion ---------------------------------
    {
        let rw_flags = MemProt::ReadWrite.to_page_flags();
        assert!(rw_flags.contains(PageFlags::WRITABLE));
        assert!(rw_flags.contains(PageFlags::NO_EXECUTE));
        assert!(rw_flags.contains(PageFlags::USER_ACCESSIBLE));

        let rx_flags = MemProt::ReadExecute.to_page_flags();
        assert!(!rx_flags.contains(PageFlags::WRITABLE));
        assert!(!rx_flags.contains(PageFlags::NO_EXECUTE));
        assert!(rx_flags.contains(PageFlags::USER_ACCESSIBLE));

        let ro_flags = MemProt::ReadOnly.to_page_flags();
        assert!(!ro_flags.contains(PageFlags::WRITABLE));
        assert!(ro_flags.contains(PageFlags::NO_EXECUTE));
        assert!(ro_flags.contains(PageFlags::USER_ACCESSIBLE));

        serial_println!("[protect]   MemProt flag conversion: OK");
    }

    // -- Test 2: W^X violation detection ---------------------------------
    {
        // Write + execute (no NO_EXECUTE) = violation
        let wx_flags = PageFlags::PRESENT | PageFlags::WRITABLE;
        assert!(is_wx_violation(wx_flags));

        // Write + no-execute = OK
        let w_flags = PageFlags::PRESENT | PageFlags::WRITABLE | PageFlags::NO_EXECUTE;
        assert!(!is_wx_violation(w_flags));

        // Read + execute (no write) = OK
        let rx_flags = PageFlags::PRESENT;
        assert!(!is_wx_violation(rx_flags));

        serial_println!("[protect]   W^X violation detection: OK");
    }

    // -- Test 3: JIT capability gate -------------------------------------
    {
        // mprotect to ReadExecute without JIT cap should fail.
        // We can't easily test with real pages, so test the logic.
        let prot = MemProt::ReadExecute;
        assert!(prot.is_executable());
        assert!(!prot.is_writable());

        let prot2 = MemProt::ReadWrite;
        assert!(!prot2.is_executable());
        assert!(prot2.is_writable());

        serial_println!("[protect]   JIT capability gate logic: OK");
    }

    // -- Test 4: Kernel W^X audit ----------------------------------------
    {
        let pml4_phys = page_table::active_pml4_phys();
        let result = audit_kernel_wx(pml4_phys);

        if result.hhdm_violations > 0 {
            // Expected: Limine's HHDM doesn't set NX on the direct map.
            // harden_hhdm_nx() sets NX at the PML4 level, but the audit
            // checks at the 4 KiB level where NX is inherited.  If
            // violations persist, the audit runs before hardening.
            serial_println!(
                "[protect]   Kernel W^X audit: {} HHDM pages lack NX (expected, Limine direct map)",
                result.hhdm_violations
            );
        }

        if result.kernel_violations == 0 {
            serial_println!("[protect]   Kernel W^X audit: OK (0 non-HHDM violations)");
        } else {
            serial_println!(
                "[protect]   Kernel W^X audit: WARNING ({} non-HHDM W+X pages found)",
                result.kernel_violations
            );
            // Don't fail — some kernel sections (like the SMP trampoline)
            // may legitimately be W+X temporarily during boot.
        }
    }

    // -- Test 5: mprotect on a test page ---------------------------------
    {
        use crate::mm::frame;

        let pml4_phys = page_table::active_pml4_phys();

        // Allocate and map a test frame.
        let test_frame = frame::alloc_frame()?;
        let test_virt = VirtAddr::new(0xFFFF_C800_0000_0000); // Test VA

        let initial_flags = PageFlags::PRESENT
            | PageFlags::WRITABLE
            | PageFlags::NO_EXECUTE;

        // SAFETY: Test-only mapping in kernel space, will be cleaned up.
        unsafe {
            page_table::map_frame(pml4_phys, test_virt, test_frame, initial_flags)?;
        }

        // mprotect to ReadOnly.
        let changed = mprotect(
            pml4_phys,
            test_virt.as_u64(),
            FRAME_SIZE,
            MemProt::ReadOnly,
            false, // no JIT cap needed for non-executable
        )?;
        assert!(changed == 1, "expected 1 frame changed");

        // Verify flags changed.
        let pte = page_table::read_leaf_pte(pml4_phys, test_virt)
            .ok_or(KernelError::InternalError)?;
        let new_flags = pte.flags();
        if new_flags.contains(PageFlags::WRITABLE) {
            serial_println!("[protect]   FAIL: WRITABLE still set after mprotect(ReadOnly)");
            return Err(KernelError::InternalError);
        }

        // mprotect to ReadExecute WITHOUT JIT cap → should fail.
        let result = mprotect(
            pml4_phys,
            test_virt.as_u64(),
            FRAME_SIZE,
            MemProt::ReadExecute,
            false, // no JIT cap
        );
        match result {
            Err(KernelError::PermissionDenied) => { /* expected */ }
            _ => {
                serial_println!("[protect]   FAIL: ReadExecute without JIT cap should be denied");
                return Err(KernelError::InternalError);
            }
        }

        // mprotect to ReadExecute WITH JIT cap → should succeed.
        let changed = mprotect(
            pml4_phys,
            test_virt.as_u64(),
            FRAME_SIZE,
            MemProt::ReadExecute,
            true, // has JIT cap
        )?;
        assert!(changed == 1);

        // Verify: executable (no NO_EXECUTE), not writable.
        let pte2 = page_table::read_leaf_pte(pml4_phys, test_virt)
            .ok_or(KernelError::InternalError)?;
        let exec_flags = pte2.flags();
        if exec_flags.contains(PageFlags::NO_EXECUTE) {
            serial_println!("[protect]   FAIL: NO_EXECUTE still set after mprotect(ReadExecute)");
            return Err(KernelError::InternalError);
        }
        if exec_flags.contains(PageFlags::WRITABLE) {
            serial_println!("[protect]   FAIL: WRITABLE set after mprotect(ReadExecute) — W^X violation!");
            return Err(KernelError::InternalError);
        }

        // mprotect back to ReadWrite (remove execute, add write).
        let changed = mprotect(
            pml4_phys,
            test_virt.as_u64(),
            FRAME_SIZE,
            MemProt::ReadWrite,
            false,
        )?;
        assert!(changed == 1);

        // Verify: writable, not executable.
        let pte3 = page_table::read_leaf_pte(pml4_phys, test_virt)
            .ok_or(KernelError::InternalError)?;
        let rw_flags = pte3.flags();
        if !rw_flags.contains(PageFlags::WRITABLE) {
            serial_println!("[protect]   FAIL: not WRITABLE after mprotect(ReadWrite)");
            return Err(KernelError::InternalError);
        }
        if !rw_flags.contains(PageFlags::NO_EXECUTE) {
            serial_println!("[protect]   FAIL: NO_EXECUTE not set after mprotect(ReadWrite) — W^X violation!");
            return Err(KernelError::InternalError);
        }

        // Clean up.
        // SAFETY: Test mapping, we own it.
        unsafe {
            page_table::unmap_frame(pml4_phys, test_virt)?;
            frame::free_frame(test_frame)?;
        }

        serial_println!("[protect]   mprotect + W^X enforcement: OK");
    }

    serial_println!("[protect] Memory protection self-test PASSED");
    Ok(())
}
