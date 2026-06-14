//! `x86_64` page table management.
//!
//! Provides abstractions over `x86_64` 4-level paging:
//! PML4 → PDPT → PD → PT → 4 KiB hardware page.
//!
//! ## 16 KiB Frames on 4 KiB Hardware Pages
//!
//! `x86_64` hardware supports only 4 KiB, 2 MiB, and 1 GiB page sizes.
//! Our OS uses 16 KiB frames as the logical allocation unit.  To map
//! a 16 KiB frame, we set 4 consecutive page table entries, each
//! pointing to one of the 4 contiguous 4 KiB hardware pages within
//! the frame.
//!
//! ## Page Table Page Allocation
//!
//! Each level of the page table hierarchy is a 4 KiB page containing
//! 512 entries (512 × 8 bytes = 4096 bytes).  Since the physical frame
//! allocator hands out 16 KiB frames, we split each frame into 4 page
//! table pages using an intrusive free list to avoid 75% waste.
//!
//! ## HHDM Access
//!
//! Page table entries contain physical addresses.  To read/write them
//! from kernel code, we convert to virtual addresses using the Higher
//! Half Direct Map (HHDM): `virt = phys + hhdm_offset`.
//!
//! ## Address Space Layout (48-bit virtual, 4-level paging)
//!
//! ```text
//! 0x0000_0000_0000_0000 ┬──────────────────────────────────────┐
//!                       │ User space (128 TiB)                 │
//!                       │   Text, data, heap, mmap, stack      │
//! 0x0000_7FFF_FFFF_FFFF ┴──────────────────────────────────────┘
//!                       │ Non-canonical hole (16M TiB)         │
//! 0xFFFF_8000_0000_0000 ┬──────────────────────────────────────┐
//!                       │ Kernel space (128 TiB)               │
//!                       │   HHDM (all physical memory)         │
//!                       │   Kernel text, data, BSS             │
//!                       │   Kernel stacks, page tables         │
//! 0xFFFF_FFFF_FFFF_FFFF ┴──────────────────────────────────────┘
//! ```
//!
//! Based on `x86_64`'s standard split between PML4 entries 0–255 (user)
//! and 256–511 (kernel).  Kernel page table entries are shared across
//! all address spaces to ensure consistent kernel mappings.

use crate::error::{KernelError, KernelResult};
use crate::mm::frame::{self, PhysFrame, FRAME_SIZE};
use crate::serial_println;
use core::ptr;
use spin::{Mutex, Once};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Number of entries in each page table level (PML4, PDPT, PD, PT).
const ENTRIES_PER_TABLE: usize = 512;

/// Size of a single hardware page (`x86_64` base page size).
const HW_PAGE_SIZE: usize = 4096;

/// Number of 4 KiB hardware pages per 16 KiB frame.
const HW_PAGES_PER_FRAME: usize = FRAME_SIZE / HW_PAGE_SIZE;

/// Mask for extracting the physical address from a page table entry.
/// Bits 12–51 (40 bits) contain the 4 KiB-aligned physical address.
const PHYS_ADDR_MASK: u64 = 0x000F_FFFF_FFFF_F000;

// ---------------------------------------------------------------------------
// Address space layout
// ---------------------------------------------------------------------------

/// Start of kernel virtual address space (upper canonical half).
///
/// PML4 entries 256–511 cover this range.  Kernel mappings in this
/// region are shared across all process address spaces.
#[allow(dead_code)] // Public API for address space layout queries.
pub const KERNEL_BASE: u64 = 0xFFFF_8000_0000_0000;

/// End of user virtual address space (exclusive).
///
/// User space spans PML4 entries 0–255: `[0, USER_SPACE_END)`.
pub const USER_SPACE_END: u64 = 0x0000_8000_0000_0000;

/// Virtual address used for kernel test mappings during self-test.
///
/// Well above the HHDM region (which starts near `KERNEL_BASE` and
/// only extends for the physical memory size) but below the kernel
/// text/data region.
const TEST_MAP_BASE: u64 = 0xFFFF_C900_0000_0000;

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

/// HHDM offset, set once during [`init`].
static HHDM_OFFSET: Once<u64> = Once::new();

/// Convenience accessor for the stored HHDM offset.
///
/// Returns `None` before [`init`] is called.
pub fn hhdm() -> Option<u64> {
    HHDM_OFFSET.get().copied()
}

// ---------------------------------------------------------------------------
// Page table entry flags
// ---------------------------------------------------------------------------

/// Flags for `x86_64` page table entries.
///
/// These map directly to the hardware-defined bits in each 64-bit
/// page table entry.  They apply at all four levels of the hierarchy,
/// though some flags have different semantics at different levels
/// (documented per flag).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageFlags(u64);

impl PageFlags {
    /// Entry is present / valid.  Hardware ignores all other bits if
    /// this is not set.  A page fault occurs on access to a non-present
    /// page.
    pub const PRESENT: Self = Self(1 << 0);

    /// Page is writable.  At intermediate levels (`PML4E`, `PDPTE`, `PDE`),
    /// clearing this makes ALL pages below read-only regardless of
    /// their own flags.
    pub const WRITABLE: Self = Self(1 << 1);

    /// Page is accessible from ring 3 (user mode).  Like `WRITABLE`,
    /// this is AND'd across all levels — clearing it at any level
    /// blocks user access to all pages below.
    pub const USER_ACCESSIBLE: Self = Self(1 << 2);

    /// Write-through caching policy.
    #[allow(dead_code)] // Used by DMA and MMIO mapping.
    pub const WRITE_THROUGH: Self = Self(1 << 3);

    /// Disable caching entirely.  Used for memory-mapped I/O regions
    /// where reads must hit the device, not a stale cache line.
    pub const NO_CACHE: Self = Self(1 << 4);

    /// Set by the CPU on any read or write.  Not cleared automatically.
    /// Used for aging/LRU algorithms in the page replacement policy.
    pub const ACCESSED: Self = Self(1 << 5);

    /// Set by the CPU on write.  Only meaningful at the leaf (PT)
    /// level.  Not cleared automatically.  Used to detect modified
    /// pages for writeback.
    #[allow(dead_code)] // Used by swap writeback detection.
    pub const DIRTY: Self = Self(1 << 6);

    /// Page size bit: at PD level creates a 2 MiB page, at PDPT level
    /// creates a 1 GiB page.  Not valid at PT or PML4 level.
    pub const HUGE_PAGE: Self = Self(1 << 7);

    /// Global page: not flushed from TLB when CR3 is changed.  Used
    /// for kernel pages that are identical across all address spaces.
    /// Requires `CR4.PGE` to be set.
    pub const GLOBAL: Self = Self(1 << 8);

    /// Copy-on-Write marker (software-defined, bit 9).
    ///
    /// When set on a present, non-writable PTE, indicates that this page
    /// is shared via CoW.  A write fault on a COW page triggers the CoW
    /// handler to either:
    /// - Copy the page (if refcount > 1) and make the copy writable, or
    /// - Just make the page writable (if refcount == 1, last reference).
    ///
    /// Bit 9 is one of three "available for OS use" bits (9, 10, 11) in
    /// x86_64 PTEs.  The hardware ignores it.
    ///
    /// Invariant: COW is only meaningful when PRESENT is set and WRITABLE
    /// is cleared.  Setting both COW and WRITABLE is invalid.
    pub const COW: Self = Self(1 << 9);

    /// No-execute: instruction fetches cause a page fault.  Requires
    /// `IA32_EFER.NXE` to be enabled (Limine does this).
    pub const NO_EXECUTE: Self = Self(1 << 63);

    /// No flags set.
    #[must_use]
    #[allow(dead_code)] // Constructor for page table operations.
    pub const fn empty() -> Self {
        Self(0)
    }

    /// Construct flags from a raw 64-bit value.
    ///
    /// Used internally by the CoW handler and other subsystems that
    /// need to manipulate flag bits directly (e.g., clearing COW while
    /// setting WRITABLE).
    #[must_use]
    pub const fn from_bits(bits: u64) -> Self {
        Self(bits)
    }

    /// The raw 64-bit value of the flags.
    #[must_use]
    pub const fn bits(self) -> u64 {
        self.0
    }

    /// Check whether all flags in `other` are set in `self`.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

impl core::ops::BitOr for PageFlags {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl core::ops::BitOrAssign for PageFlags {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

impl core::ops::BitAnd for PageFlags {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self {
        Self(self.0 & rhs.0)
    }
}

impl core::ops::Not for PageFlags {
    type Output = Self;
    fn not(self) -> Self {
        Self(!self.0)
    }
}

// ---------------------------------------------------------------------------
// Page table entry
// ---------------------------------------------------------------------------

/// A single 64-bit entry in any level of the `x86_64` page table.
///
/// The entry format is:
/// ```text
/// 63  62:52    51:12          11:0
/// ┌────┬─────┬───────────────┬────────────┐
/// │ NX │ avl │ physical addr │ flags      │
/// └────┴─────┴───────────────┴────────────┘
/// ```
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct PageTableEntry(u64);

impl PageTableEntry {
    /// An empty (non-present) page table entry.
    pub const EMPTY: Self = Self(0);

    /// Create an entry pointing to `phys_addr` with the given flags.
    ///
    /// `phys_addr` must be 4 KiB aligned (low 12 bits zero).
    ///
    /// See [`PageFlags`] for the available flag bits.
    #[must_use]
    pub const fn new(phys_addr: u64, flags: PageFlags) -> Self {
        Self((phys_addr & PHYS_ADDR_MASK) | flags.bits())
    }

    /// Is this entry present (valid)?
    #[must_use]
    pub const fn is_present(self) -> bool {
        self.0 & PageFlags::PRESENT.bits() != 0
    }

    /// Is this a huge-page entry (PS bit set)?
    #[must_use]
    pub const fn is_huge(self) -> bool {
        self.0 & PageFlags::HUGE_PAGE.bits() != 0
    }

    /// Is this entry marked as Copy-on-Write?
    ///
    /// COW entries are present but non-writable, with the COW bit (bit 9)
    /// set.  A write to a COW page triggers the CoW fault handler.
    #[must_use]
    pub const fn is_cow(self) -> bool {
        self.0 & PageFlags::COW.bits() != 0
    }

    /// Extract the 4 KiB-aligned physical address from this entry.
    #[must_use]
    pub const fn phys_addr(self) -> u64 {
        self.0 & PHYS_ADDR_MASK
    }

    /// Extract the flags (everything except the physical address bits).
    #[must_use]
    pub const fn flags(self) -> PageFlags {
        PageFlags(self.0 & !PHYS_ADDR_MASK)
    }

    /// Raw 64-bit value of the entry.
    #[must_use]
    pub const fn raw(self) -> u64 {
        self.0
    }

    /// Create a page table entry from a raw 64-bit value.
    ///
    /// Used by the swap subsystem to construct non-present entries
    /// that encode swap slot information.
    #[must_use]
    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    /// Check if this is a swap entry (non-present, with swap marker).
    ///
    /// Swap entries use the format:
    /// - Bit 0: 0 (not present)
    /// - Bit 1: 1 (swap marker — distinguishes from truly-unmapped pages)
    /// - Bits 2–31: swap slot index
    ///
    /// A PTE of 0 is NOT a swap entry (it's an unmapped page).
    #[must_use]
    pub const fn is_swap(self) -> bool {
        // Not present (bit 0 = 0) but swap marker set (bit 1 = 1).
        (self.0 & 0b11) == 0b10
    }
}

// ---------------------------------------------------------------------------
// Virtual address
// ---------------------------------------------------------------------------

/// A 64-bit virtual address with methods to extract page table indices.
///
/// On `x86_64` with 4-level paging, a 48-bit virtual address is split:
///
/// ```text
/// 63    48 47    39 38    30 29    21 20    12 11     0
/// ┌───────┬────────┬────────┬────────┬────────┬────────┐
/// │ sign  │ PML4   │ PDPT   │   PD   │   PT   │ offset │
/// │ ext.  │ [8:0]  │ [8:0]  │  [8:0] │  [8:0] │        │
/// └───────┴────────┴────────┴────────┴────────┴────────┘
///   16 bit   9 bit   9 bit    9 bit    9 bit    12 bit
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct VirtAddr(u64);

// Truncation: the & 0x1FF masks guarantee results fit in 9 bits,
// and page_offset masks to 12 bits.  Both are well within usize on
// any platform.  On x86_64 (our only target), usize = u64 anyway.
#[allow(clippy::cast_possible_truncation)]
impl VirtAddr {
    /// Create a `VirtAddr` from a raw 64-bit address.
    #[must_use]
    pub const fn new(addr: u64) -> Self {
        Self(addr)
    }

    /// The raw 64-bit value.
    #[must_use]
    pub const fn as_u64(self) -> u64 {
        self.0
    }

    /// Check if this is a canonical `x86_64` address.
    ///
    /// Canonical addresses have bits 48–63 equal to bit 47 (sign
    /// extension of the 48-bit virtual address).
    #[must_use]
    pub const fn is_canonical(self) -> bool {
        let top = self.0 >> 47;
        top == 0 || top == 0x1_FFFF
    }

    /// Is this address in user space (lower canonical half)?
    #[must_use]
    pub const fn is_user(self) -> bool {
        self.0 < USER_SPACE_END
    }

    /// Is this address in kernel space (upper canonical half)?
    #[must_use]
    #[allow(dead_code)] // Public API for address classification.
    pub const fn is_kernel(self) -> bool {
        self.0 >= KERNEL_BASE
    }

    /// PML4 index (bits 47–39).
    #[must_use]
    pub const fn pml4_index(self) -> usize {
        ((self.0 >> 39) & 0x1FF) as usize
    }

    /// PDPT index (bits 38–30).
    #[must_use]
    pub const fn pdpt_index(self) -> usize {
        ((self.0 >> 30) & 0x1FF) as usize
    }

    /// PD index (bits 29–21).
    #[must_use]
    pub const fn pd_index(self) -> usize {
        ((self.0 >> 21) & 0x1FF) as usize
    }

    /// PT index (bits 20–12).
    #[must_use]
    pub const fn pt_index(self) -> usize {
        ((self.0 >> 12) & 0x1FF) as usize
    }

    /// Page offset within a 4 KiB page (bits 11–0).
    #[must_use]
    pub const fn page_offset(self) -> usize {
        (self.0 & 0xFFF) as usize
    }

    /// Is this address aligned to a 16 KiB frame boundary?
    #[must_use]
    #[allow(clippy::arithmetic_side_effects)]
    pub const fn is_frame_aligned(self) -> bool {
        self.0 & (FRAME_SIZE as u64 - 1) == 0
    }
}

impl core::fmt::Display for VirtAddr {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{:#x}", self.0)
    }
}

// ---------------------------------------------------------------------------
// Page table page pool (4 KiB pages from 16 KiB frames)
// ---------------------------------------------------------------------------

/// Pool of 4 KiB page-table pages, backed by 16 KiB physical frames.
///
/// When a new page table is needed (e.g., during [`map_frame`]), we
/// allocate from this pool.  If the pool is empty, a 16 KiB frame is
/// allocated from the buddy allocator and split into 4 pages.
///
/// Free pages form an intrusive singly-linked list: the first 8 bytes
/// of each free page store the physical address of the next free page
/// (or 0 for end-of-list).
struct PtPagePool {
    /// Physical address of the first free 4 KiB page (0 = empty).
    head: u64,
    /// HHDM offset for physical → virtual conversion.
    hhdm_offset: u64,
}

// SAFETY: PtPagePool is only accessed through a spin::Mutex.
// Raw pointers (created transiently from hhdm_offset + physical) are
// valid HHDM addresses exclusively owned by this allocator.
unsafe impl Send for PtPagePool {}

/// Global page table page pool, protected by a spinlock.
///
/// Lock ordering: `PT_PAGE_POOL` is always acquired before the frame
/// allocator lock (inside `refill → alloc_frame`).  No code acquires
/// them in reverse order, so there is no deadlock risk.
static PT_PAGE_POOL: Mutex<PtPagePool> = Mutex::new(PtPagePool {
    head: 0,
    hhdm_offset: 0,
});

// cast_ptr_alignment: page addresses are 4 KiB aligned, exceeding
// the 8-byte alignment requirement for reading/writing u64 values.
#[allow(clippy::cast_ptr_alignment)]
impl PtPagePool {
    /// Allocate a zeroed 4 KiB page for use as a page table.
    ///
    /// Returns the physical address of the page.
    #[allow(clippy::arithmetic_side_effects)]
    fn alloc(&mut self) -> KernelResult<u64> {
        if self.head == 0 {
            self.refill()?;
        }

        let page_phys = self.head;

        // Pop the head page from the free list.
        // SAFETY: page_phys is non-zero (we just refilled if needed).
        // It points to a valid 4 KiB page in HHDM-mapped memory.
        // The first 8 bytes contain the next-page physical address,
        // written by `refill` or a previous `free`.
        let page_virt = (page_phys + self.hhdm_offset) as *const u64;
        self.head = unsafe { ptr::read(page_virt) };

        // Zero the page.  Page tables require all non-present entries
        // to be zero (hardware reads non-present entries' reserved bits).
        // SAFETY: page_virt points to a valid, exclusively-owned 4 KiB page.
        unsafe {
            ptr::write_bytes(page_virt as *mut u8, 0, HW_PAGE_SIZE);
        }

        Ok(page_phys)
    }

    /// Return a 4 KiB page to the pool.
    ///
    /// # Safety
    ///
    /// `page_phys` must be a 4 KiB-aligned physical address previously
    /// obtained from [`alloc`](PtPagePool::alloc) and no longer
    /// referenced by any page table entry.
    #[allow(clippy::arithmetic_side_effects)]
    unsafe fn _free(&mut self, page_phys: u64) {
        let page_virt = (page_phys + self.hhdm_offset) as *mut u64;
        // SAFETY: page_virt is a valid 4 KiB page we exclusively own.
        unsafe {
            ptr::write(page_virt, self.head);
        }
        self.head = page_phys;
    }

    /// Allocate a 16 KiB frame and split it into 4 page table pages.
    ///
    /// All 4 pages are pushed onto the free list.
    #[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
    fn refill(&mut self) -> KernelResult<()> {
        let frame = frame::alloc_frame()?;
        let base = frame.addr();
        super::memtype::charge(super::memtype::MemType::PageTable, 1);

        // Split into 4 × 4 KiB pages.  Push in reverse order so the
        // lowest address ends up at the head (first to be allocated).
        for i in (0..HW_PAGES_PER_FRAME).rev() {
            let page_phys = base + (i as u64) * (HW_PAGE_SIZE as u64);
            let page_virt = (page_phys + self.hhdm_offset) as *mut u64;
            // SAFETY: frame is freshly allocated, all 4 pages are ours.
            unsafe {
                ptr::write(page_virt, self.head);
            }
            self.head = page_phys;
        }

        Ok(())
    }
}

/// Allocate a zeroed 4 KiB page for a page table.
///
/// The returned physical address is 4 KiB-aligned.  Used by both CPU
/// page tables and IOMMU second-level page tables.
pub fn alloc_pt_page() -> KernelResult<u64> {
    PT_PAGE_POOL.lock().alloc()
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Read a page table entry.
///
/// # Safety
///
/// - `table_phys` must be the physical address of a valid 4 KiB page
///   table (512 entries).
/// - `index` must be < [`ENTRIES_PER_TABLE`] (512).
/// - `hhdm` must be the correct HHDM offset.
#[inline]
#[allow(clippy::arithmetic_side_effects)]
pub unsafe fn read_entry(table_phys: u64, index: usize, hhdm: u64) -> PageTableEntry {
    let table_virt = (table_phys + hhdm) as *const PageTableEntry;
    // SAFETY: Caller guarantees table_phys is valid, index < 512,
    // and the HHDM maps this physical page.  PageTableEntry is 8 bytes
    // and the table is 4 KiB aligned, so alignment is satisfied.
    unsafe { ptr::read(table_virt.add(index)) }
}

/// Write a page table entry.
///
/// # Safety
///
/// Same as [`read_entry`], plus the caller must have exclusive access
/// to this entry (either via a lock or single-threaded boot context).
#[inline]
#[allow(clippy::arithmetic_side_effects)]
pub(crate) unsafe fn write_entry(
    table_phys: u64,
    index: usize,
    entry: PageTableEntry,
    hhdm: u64,
) {
    let table_virt = (table_phys + hhdm) as *mut PageTableEntry;
    // SAFETY: Caller guarantees validity and exclusive access.
    unsafe { ptr::write(table_virt.add(index), entry); }
}

/// Walk one level of the page table hierarchy.
///
/// If the entry at `index` in `table_phys` is present, returns the
/// physical address of the next-level table.  If not present and
/// `create` is true, allocates a new zeroed page table page and
/// installs it with `PRESENT | WRITABLE` (plus `USER_ACCESSIBLE` if
/// `user` is true).
///
/// # Safety
///
/// - `table_phys` must be a valid page table.
/// - `index` must be < 512.
#[allow(clippy::arithmetic_side_effects)]
pub(crate) unsafe fn walk_or_create(
    table_phys: u64,
    index: usize,
    create: bool,
    user: bool,
    hhdm: u64,
) -> KernelResult<u64> {
    // SAFETY: Caller guarantees table_phys and index are valid.
    let entry = unsafe { read_entry(table_phys, index, hhdm) };

    if entry.is_present() {
        if entry.is_huge() {
            // Can't walk into a huge page — the caller tried to create
            // a 4 KiB mapping within a region that's already covered by
            // a 2 MiB or 1 GiB huge page.
            return Err(KernelError::InvalidAddress);
        }
        Ok(entry.phys_addr())
    } else if create {
        // Allocate a new zeroed page table page.
        let new_page = alloc_pt_page()?;

        // Intermediate entries need PRESENT + WRITABLE so that leaf
        // entries below can freely set or clear their own WRITABLE bit.
        // USER_ACCESSIBLE is set for user-space ranges so that leaf
        // entries below can grant user access.
        let mut flags = PageFlags::PRESENT | PageFlags::WRITABLE;
        if user {
            flags |= PageFlags::USER_ACCESSIBLE;
        }

        let new_entry = PageTableEntry::new(new_page, flags);
        // SAFETY: table_phys is valid, index < 512.
        unsafe { write_entry(table_phys, index, new_entry, hhdm); }

        Ok(new_page)
    } else {
        Err(KernelError::InvalidAddress)
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Initialize the page table subsystem.
///
/// Must be called after the frame allocator is initialized and before
/// any page table operations (except [`read_cr3`] and [`cr3_to_pml4`]
/// which are pure hardware reads).
pub fn init(hhdm_offset: u64) {
    HHDM_OFFSET.call_once(|| hhdm_offset);
    PT_PAGE_POOL.lock().hhdm_offset = hhdm_offset;

    // Record the kernel PML4 in the accounting module so that
    // kernel-space mappings are excluded from per-process RSS tracking.
    let kernel_pml4 = cr3_to_pml4(read_cr3());
    super::accounting::set_kernel_pml4(kernel_pml4);

    serial_println!("[mm] Page table subsystem initialized");
    serial_println!("[mm]   Active PML4: {:#x}", kernel_pml4);
}

/// Translate a virtual address to a physical address.
///
/// Walks the page table hierarchy starting from `pml4_phys`, handling
/// 4 KiB pages, 2 MiB huge pages, and 1 GiB huge pages.
///
/// Returns `None` if:
/// - The page table subsystem is not initialized.
/// - The address is non-canonical.
/// - Any level of the walk encounters a non-present entry.
// Arithmetic: masked offset additions for huge pages and page offset.
// These cannot overflow because offsets are bounded by the page size.
#[allow(clippy::arithmetic_side_effects)]
pub fn translate(pml4_phys: u64, virt: VirtAddr) -> Option<u64> {
    let hhdm = hhdm()?;

    if !virt.is_canonical() {
        return None;
    }

    // PML4 → PDPT.
    // SAFETY: pml4_phys comes from CR3 or our own allocation.  Index
    // is from VirtAddr which masks to 0..511.
    let pml4e = unsafe { read_entry(pml4_phys, virt.pml4_index(), hhdm) };
    if !pml4e.is_present() {
        return None;
    }

    // SAFETY: pml4e is present, so phys_addr() is a valid PDPT table.
    let pdpte = unsafe { read_entry(pml4e.phys_addr(), virt.pdpt_index(), hhdm) };
    if !pdpte.is_present() {
        return None;
    }
    if pdpte.is_huge() {
        // 1 GiB huge page: offset is the low 30 bits.
        return Some(pdpte.phys_addr() + (virt.as_u64() & 0x3FFF_FFFF));
    }

    // SAFETY: pdpte is present and not huge, so phys_addr() is a valid PD table.
    let pde = unsafe { read_entry(pdpte.phys_addr(), virt.pd_index(), hhdm) };
    if !pde.is_present() {
        return None;
    }
    if pde.is_huge() {
        // 2 MiB huge page: offset is the low 21 bits.
        return Some(pde.phys_addr() + (virt.as_u64() & 0x1F_FFFF));
    }

    // SAFETY: pde is present and not huge, so phys_addr() is a valid PT table.
    let pte = unsafe { read_entry(pde.phys_addr(), virt.pt_index(), hhdm) };
    if !pte.is_present() {
        return None;
    }

    Some(pte.phys_addr() + virt.page_offset() as u64)
}

/// Translate a virtual address and return the page flags.
///
/// Walks the page table hierarchy and returns the flags of the PTE that
/// maps `virt`.  Returns `None` if the address is not mapped.
///
/// Used by memory compaction to preserve flags when migrating pages.
pub fn translate_flags(pml4_phys: u64, virt: VirtAddr) -> Option<PageFlags> {
    let hhdm = hhdm()?;

    if !virt.is_canonical() {
        return None;
    }

    // SAFETY for all read_entry calls below: each table physical address
    // is either pml4_phys (from CR3 or our allocation) or extracted from
    // a present parent entry via phys_addr().  The index is computed from
    // the canonical virtual address.  hhdm converts phys→virt correctly.
    let pml4e = unsafe { read_entry(pml4_phys, virt.pml4_index(), hhdm) };
    if !pml4e.is_present() {
        return None;
    }
    let pdpte = unsafe { read_entry(pml4e.phys_addr(), virt.pdpt_index(), hhdm) };
    if !pdpte.is_present() {
        return None;
    }
    if pdpte.is_huge() {
        // Extract flag bits (low 12 bits + NX bit 63).
        let raw = pdpte.raw();
        return Some(PageFlags::from_bits((raw & 0xFFF) | (raw & (1 << 63))));
    }
    let pde = unsafe { read_entry(pdpte.phys_addr(), virt.pd_index(), hhdm) };
    if !pde.is_present() {
        return None;
    }
    if pde.is_huge() {
        let raw = pde.raw();
        return Some(PageFlags::from_bits((raw & 0xFFF) | (raw & (1 << 63))));
    }
    let pte = unsafe { read_entry(pde.phys_addr(), virt.pt_index(), hhdm) };
    if !pte.is_present() {
        return None;
    }
    let raw = pte.raw();
    Some(PageFlags::from_bits((raw & 0xFFF) | (raw & (1 << 63))))
}

/// Map a single 4 KiB hardware page if not already mapped.
///
/// Unlike [`map_frame`], this operates on individual 4 KiB pages and
/// does not require 16 KiB alignment.  If the page is already present,
/// returns `Ok(false)`.  If it was newly mapped, returns `Ok(true)`.
///
/// This is used for filling HHDM gaps where the bootloader may have
/// mapped some but not all 4 KiB pages within a 16 KiB frame boundary
/// (e.g., at the edge between a reserved and ACPI reclaimable region).
///
/// # Errors
///
/// - [`KernelError::InvalidAddress`] — `virt` is non-canonical, or a
///   huge page was found at an intermediate level.
/// - [`KernelError::NotSupported`] — subsystem not initialized.
///
/// # Safety
///
/// - `pml4_phys` must be a valid PML4 table.
/// - `virt` must be 4 KiB aligned and canonical.
/// - `phys_4k` must be a valid 4 KiB-aligned physical address.
pub unsafe fn map_4k_if_absent(
    pml4_phys: u64,
    virt: VirtAddr,
    phys_4k: u64,
    flags: PageFlags,
) -> KernelResult<bool> {
    let hhdm = hhdm().ok_or(KernelError::NotSupported)?;

    if !virt.is_canonical() {
        return Err(KernelError::InvalidAddress);
    }

    let user = virt.is_user();

    // Walk PML4 → PDPT → PD → PT, creating intermediate tables as
    // needed.  This may fail if a huge page is encountered (which would
    // mean the target is already mapped via a large page).
    // SAFETY: pml4_phys is valid (caller guarantee).  Each subsequent
    // call uses a table returned by walk_or_create, which guarantees a
    // valid, present page table at the returned physical address.
    let pdpt = unsafe {
        walk_or_create(pml4_phys, virt.pml4_index(), true, user, hhdm)?
    };
    // SAFETY: pdpt returned by walk_or_create above.
    let pd = unsafe {
        walk_or_create(pdpt, virt.pdpt_index(), true, user, hhdm)?
    };
    // SAFETY: pd returned by walk_or_create above.
    let pt = unsafe {
        walk_or_create(pd, virt.pd_index(), true, user, hhdm)?
    };

    let pt_idx = virt.pt_index();

    // SAFETY: pt is a valid page table, pt_idx < 512.
    let existing = unsafe { read_entry(pt, pt_idx, hhdm) };
    if existing.is_present() {
        return Ok(false); // Already mapped, nothing to do.
    }

    let entry = PageTableEntry::new(phys_4k, flags);
    // SAFETY: pt valid, pt_idx < 512, existing is not-present so no
    // conflict.
    unsafe { write_entry(pt, pt_idx, entry, hhdm); }

    Ok(true)
}

/// Map a 16 KiB physical frame at a virtual address.
///
/// Sets 4 consecutive page table entries (one per 4 KiB hardware page
/// within the frame).  Intermediate page table levels (PDPT, PD, PT)
/// are allocated automatically if they don't exist.
///
/// `flags` must include [`PageFlags::PRESENT`] for the mapping to be
/// usable.  Additional flags (`WRITABLE`, `USER_ACCESSIBLE`, `NO_EXECUTE`,
/// etc.) control access permissions at the leaf level.
///
/// # Errors
///
/// - [`KernelError::BadAlignment`] — `virt` is not 16 KiB aligned.
/// - [`KernelError::InvalidAddress`] — `virt` is non-canonical, or an
///   intermediate entry is a huge page.
/// - [`KernelError::AlreadyExists`] — one of the 4 PTEs is already
///   present.
/// - [`KernelError::OutOfMemory`] — page table page allocation failed.
/// - [`KernelError::NotSupported`] — subsystem not initialized.
///
/// # Safety
///
/// - `pml4_phys` must be a valid PML4 table that the caller owns.
/// - `phys` must be a valid physical frame.
/// - If mapping in the active address space, the caller must flush
///   the TLB for the new addresses (see [`flush_frame`]).
// Arithmetic: address calculations for 4 consecutive hardware pages.
// base_pt_index + i is bounded by ENTRIES_PER_TABLE (see proof below).
#[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
pub unsafe fn map_frame(
    pml4_phys: u64,
    virt: VirtAddr,
    phys: PhysFrame,
    flags: PageFlags,
) -> KernelResult<()> {
    let hhdm = hhdm().ok_or(KernelError::NotSupported)?;

    if !virt.is_frame_aligned() {
        return Err(KernelError::BadAlignment);
    }
    if !virt.is_canonical() {
        return Err(KernelError::InvalidAddress);
    }

    let user = virt.is_user();

    // Walk PML4 → PDPT → PD → PT, creating intermediate tables as needed.
    // SAFETY: pml4_phys is valid (caller guarantee).  Each subsequent
    // level uses a table returned by walk_or_create, guaranteed valid.
    let pdpt = unsafe {
        walk_or_create(pml4_phys, virt.pml4_index(), true, user, hhdm)?
    };
    // SAFETY: pdpt returned by walk_or_create above.
    let pd = unsafe {
        walk_or_create(pdpt, virt.pdpt_index(), true, user, hhdm)?
    };
    // SAFETY: pd returned by walk_or_create above.
    let pt = unsafe {
        walk_or_create(pd, virt.pd_index(), true, user, hhdm)?
    };

    let base_pt_index = virt.pt_index();

    // Proof that all 4 PT indices are in bounds:
    //
    // virt is 16 KiB (0x4000) aligned, so bits 13:12 of the address
    // are 00.  The PT index is bits 20:12, so the low 2 bits of the
    // PT index are 0 — meaning base_pt_index is a multiple of 4.
    //
    // The maximum PT index that is a multiple of 4 is 508 (0x1FC).
    // 508 + 3 = 511 < 512 = ENTRIES_PER_TABLE.  ✓
    debug_assert!(base_pt_index.is_multiple_of(HW_PAGES_PER_FRAME));
    debug_assert!(base_pt_index + HW_PAGES_PER_FRAME <= ENTRIES_PER_TABLE);

    // OPT: Single-pass check-and-write.  Read each existing entry,
    // verify it's non-present, then write the new entry immediately.
    // This does 4 reads + 4 writes = 8 memory accesses, versus the
    // previous two-loop approach that did 4 reads (pre-check) + 4 writes
    // = 8 reads + 4 writes = 12 memory accesses.  On the page fault
    // hot path, each saved read shaves ~100-400ns on real hardware.
    //
    // If any entry is already present, undo the partial mapping by
    // clearing entries we've already written, preserving atomicity.
    for i in 0..HW_PAGES_PER_FRAME {
        // SAFETY: pt is a valid page table, index < 512 (proven above).
        let existing = unsafe { read_entry(pt, base_pt_index + i, hhdm) };
        if existing.is_present() {
            // Roll back entries written so far.
            for j in 0..i {
                // SAFETY: entries 0..i were just written by us.
                unsafe {
                    write_entry(pt, base_pt_index + j, PageTableEntry::EMPTY, hhdm);
                }
            }
            return Err(KernelError::AlreadyExists);
        }

        let hw_phys = phys.addr() + (i as u64) * (HW_PAGE_SIZE as u64);
        let entry = PageTableEntry::new(hw_phys, flags);
        // SAFETY: pt valid, index < 512, exclusive access guaranteed
        // by caller (single-threaded boot or holding a lock).
        unsafe { write_entry(pt, base_pt_index + i, entry, hhdm); }
    }

    // Track per-address-space RSS for OOM killer and diagnostics.
    super::accounting::charge(pml4_phys, 1);

    Ok(())
}

/// Unmap a 16 KiB frame, clearing 4 consecutive page table entries.
///
/// Returns the physical frame that was mapped at `virt`.  Does NOT
/// free the physical frame — the caller decides whether to return it
/// to the frame allocator.  Also does NOT free intermediate page
/// table pages (they may be needed for adjacent mappings).
///
/// # Errors
///
/// - [`KernelError::BadAlignment`] — `virt` is not 16 KiB aligned.
/// - [`KernelError::InvalidAddress`] — `virt` is non-canonical, not
///   mapped, or passes through a huge page entry.
/// - [`KernelError::NotSupported`] — subsystem not initialized.
///
/// # Safety
///
/// - `pml4_phys` must be a valid PML4 table.
/// - The caller must flush the TLB afterward ([`flush_frame`]).
/// - In SMP, the caller must ensure no other CPU holds a TLB entry
///   for these addresses (TLB shootdown).
#[allow(clippy::arithmetic_side_effects)]
pub unsafe fn unmap_frame(
    pml4_phys: u64,
    virt: VirtAddr,
) -> KernelResult<PhysFrame> {
    let hhdm = hhdm().ok_or(KernelError::NotSupported)?;

    if !virt.is_frame_aligned() {
        return Err(KernelError::BadAlignment);
    }
    if !virt.is_canonical() {
        return Err(KernelError::InvalidAddress);
    }

    // Walk PML4 → PT (no creation — the mapping must already exist).
    // SAFETY: pml4_phys is valid (caller guarantee).
    let pml4e = unsafe { read_entry(pml4_phys, virt.pml4_index(), hhdm) };
    if !pml4e.is_present() {
        return Err(KernelError::InvalidAddress);
    }

    // SAFETY: pml4e is present, so phys_addr() is a valid PDPT table.
    let pdpte = unsafe { read_entry(pml4e.phys_addr(), virt.pdpt_index(), hhdm) };
    if !pdpte.is_present() || pdpte.is_huge() {
        return Err(KernelError::InvalidAddress);
    }

    // SAFETY: pdpte is present and not huge, so phys_addr() is a valid PD.
    let pde = unsafe { read_entry(pdpte.phys_addr(), virt.pd_index(), hhdm) };
    if !pde.is_present() || pde.is_huge() {
        return Err(KernelError::InvalidAddress);
    }

    let pt = pde.phys_addr();
    let base_pt_index = virt.pt_index();

    // SAFETY: pt from pde which is present, index < 512.
    let first_pte = unsafe { read_entry(pt, base_pt_index, hhdm) };
    if !first_pte.is_present() {
        return Err(KernelError::InvalidAddress);
    }

    // Clear all 4 PTEs.
    for i in 0..HW_PAGES_PER_FRAME {
        // SAFETY: pt valid, index < 512.
        unsafe {
            write_entry(pt, base_pt_index + i, PageTableEntry::EMPTY, hhdm);
        }
    }

    // Track per-address-space RSS for OOM killer and diagnostics.
    super::accounting::uncharge(pml4_phys, 1);

    // The first PTE's physical address is the base of the 16 KiB frame.
    PhysFrame::from_addr(first_pte.phys_addr()).ok_or(KernelError::InternalError)
}

/// Unmap a single 4 KiB hardware page.
///
/// Clears the page table entry at the given virtual address and returns
/// the physical address that was mapped.  Unlike [`unmap_frame`], this
/// operates on individual 4 KiB pages and does NOT require 16 KiB frame
/// alignment.
///
/// Primarily used by the DMA subsystem, which maps individual 4 KiB
/// hardware pages via [`map_4k_if_absent`] rather than full 16 KiB frames.
///
/// # Errors
///
/// - [`KernelError::InvalidAddress`] — `virt` is non-canonical, not
///   mapped, or passes through a huge page entry.
/// - [`KernelError::NotSupported`] — subsystem not initialized.
///
/// # Safety
///
/// - `pml4_phys` must be a valid PML4 table.
/// - The caller must flush the TLB afterward.
/// - In SMP, the caller must ensure TLB shootdown is performed.
pub unsafe fn unmap_4k(
    pml4_phys: u64,
    virt: VirtAddr,
) -> KernelResult<u64> {
    let hhdm = hhdm().ok_or(KernelError::NotSupported)?;

    if !virt.is_canonical() {
        return Err(KernelError::InvalidAddress);
    }

    // Walk PML4 → PT (no creation — the mapping must already exist).
    // SAFETY: pml4_phys is valid (caller guarantee).
    let pml4e = unsafe { read_entry(pml4_phys, virt.pml4_index(), hhdm) };
    if !pml4e.is_present() {
        return Err(KernelError::InvalidAddress);
    }

    // SAFETY: pml4e present → valid PDPT table.
    let pdpte = unsafe { read_entry(pml4e.phys_addr(), virt.pdpt_index(), hhdm) };
    if !pdpte.is_present() || pdpte.is_huge() {
        return Err(KernelError::InvalidAddress);
    }

    // SAFETY: pdpte present, not huge → valid PD table.
    let pde = unsafe { read_entry(pdpte.phys_addr(), virt.pd_index(), hhdm) };
    if !pde.is_present() || pde.is_huge() {
        return Err(KernelError::InvalidAddress);
    }

    let pt = pde.phys_addr();
    let pt_idx = virt.pt_index();

    // SAFETY: pt from present pde, pt_idx < 512 (from VirtAddr mask).
    let pte = unsafe { read_entry(pt, pt_idx, hhdm) };
    if !pte.is_present() {
        return Err(KernelError::InvalidAddress);
    }

    let phys = pte.phys_addr();

    // SAFETY: pt valid, pt_idx < 512, exclusive access (caller guarantee).
    unsafe {
        write_entry(pt, pt_idx, PageTableEntry::EMPTY, hhdm);
    }

    Ok(phys)
}

/// Change the protection flags on an existing 16 KiB frame mapping.
///
/// All 4 page table entries are updated with the new `flags`.  The
/// physical addresses are preserved.  The frame must currently be
/// fully mapped (all 4 PTEs present).
///
/// # Safety
///
/// Same requirements as [`map_frame`].  The caller must flush the TLB.
#[allow(clippy::arithmetic_side_effects)]
pub unsafe fn change_flags(
    pml4_phys: u64,
    virt: VirtAddr,
    flags: PageFlags,
) -> KernelResult<()> {
    let hhdm = hhdm().ok_or(KernelError::NotSupported)?;

    if !virt.is_frame_aligned() {
        return Err(KernelError::BadAlignment);
    }
    if !virt.is_canonical() {
        return Err(KernelError::InvalidAddress);
    }

    // Walk to the PT (no creation).
    // SAFETY for all read_entry calls: each table address is either
    // pml4_phys (caller-provided, valid per fn safety contract) or from
    // a present parent entry.  Indices from canonical virt address.
    let pml4e = unsafe { read_entry(pml4_phys, virt.pml4_index(), hhdm) };
    if !pml4e.is_present() {
        return Err(KernelError::InvalidAddress);
    }

    let pdpte = unsafe { read_entry(pml4e.phys_addr(), virt.pdpt_index(), hhdm) };
    if !pdpte.is_present() || pdpte.is_huge() {
        return Err(KernelError::InvalidAddress);
    }

    let pde = unsafe { read_entry(pdpte.phys_addr(), virt.pd_index(), hhdm) };
    if !pde.is_present() || pde.is_huge() {
        return Err(KernelError::InvalidAddress);
    }

    let pt = pde.phys_addr();
    let base_pt_index = virt.pt_index();

    // Update all 4 PTEs: keep the physical address, replace the flags.
    for i in 0..HW_PAGES_PER_FRAME {
        // SAFETY: pt valid from present pde, index < 512.
        let pte = unsafe { read_entry(pt, base_pt_index + i, hhdm) };
        if !pte.is_present() {
            return Err(KernelError::InvalidAddress);
        }
        let updated = PageTableEntry::new(pte.phys_addr(), flags);
        // SAFETY: pt valid, index < 512, exclusive access.
        unsafe { write_entry(pt, base_pt_index + i, updated, hhdm); }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Swap PTE helpers
// ---------------------------------------------------------------------------

/// Read the raw PTE for the first 4 KiB page of a 16 KiB frame.
/// Read the leaf page table entry for a virtual address.
///
/// Walks the page table hierarchy and returns the raw PTE value for
/// the leaf entry at `virt`.  Unlike [`translate`], this function
/// does NOT require the entry to be present — it returns the raw
/// value even for non-present (swap) entries.
///
/// Handles 4 KiB, 2 MiB (huge), and 1 GiB (huge) page sizes:
/// if a huge page is encountered, its PTE is returned directly.
///
/// Returns `None` if:
/// - The subsystem is not initialized.
/// - `virt` is not canonical.
/// - An intermediate level (PML4/PDPT/PD) is not present and is
///   not a huge page (the walk can't reach the leaf level).
/// - The final leaf PTE is not present.
///
/// The caller must guarantee `pml4_phys` is a valid PML4 table.
/// This function is safe because reading PTEs through the HHDM is
/// a read-only operation that cannot cause UB when the subsystem
/// is initialized (checked by `hhdm()`).
#[must_use]
pub fn read_leaf_pte(pml4_phys: u64, virt: VirtAddr) -> Option<PageTableEntry> {
    let hhdm = hhdm()?;
    if !virt.is_canonical() {
        return None;
    }

    // SAFETY for all read_entry calls: pml4_phys is valid (caller
    // guarantee), each subsequent table address is from a present parent
    // entry via phys_addr().  Indices from VirtAddr are always 0..511.
    // hhdm is valid (checked above).
    let pml4e = unsafe { read_entry(pml4_phys, virt.pml4_index(), hhdm) };
    if !pml4e.is_present() {
        return None;
    }

    let pdpte = unsafe { read_entry(pml4e.phys_addr(), virt.pdpt_index(), hhdm) };
    if !pdpte.is_present() {
        return None;
    }
    if pdpte.is_huge() {
        return Some(pdpte); // 1 GiB huge page.
    }

    let pde = unsafe { read_entry(pdpte.phys_addr(), virt.pd_index(), hhdm) };
    if !pde.is_present() {
        return None;
    }
    if pde.is_huge() {
        return Some(pde); // 2 MiB huge page.
    }

    let pte = unsafe { read_entry(pde.phys_addr(), virt.pt_index(), hhdm) };
    if pte.is_present() { Some(pte) } else { None }
}

/// Write a swap entry into all 4 leaf PTEs for a 16 KiB frame.
///
/// Used by the swap subsystem to mark a frame as swapped-out.  All
/// 4 hardware page PTEs are set to the same swap entry so that a
/// page fault on any of the 4 KiB pages within the frame can find
/// the swap slot index.
///
/// Intermediate page table levels must already exist (the frame was
/// previously mapped, so they do).
///
/// # Safety
///
/// - `pml4_phys` must be a valid PML4 table.
/// - The existing PTEs must be NOT present (the physical frame should
///   have already been unmapped).
/// - The caller must flush the TLB afterward.
#[allow(clippy::arithmetic_side_effects)]
pub unsafe fn write_swap_entries(
    pml4_phys: u64,
    virt: VirtAddr,
    entry: PageTableEntry,
) -> KernelResult<()> {
    let hhdm = hhdm().ok_or(KernelError::NotSupported)?;

    if !virt.is_frame_aligned() {
        return Err(KernelError::BadAlignment);
    }
    if !virt.is_canonical() {
        return Err(KernelError::InvalidAddress);
    }

    // Walk to PT (no creation — must already exist).
    // SAFETY for all read_entry calls: pml4_phys valid per fn safety
    // contract; each subsequent table address is from a present parent
    // entry.  Indices from canonical VirtAddr are always 0..511.
    let pml4e = unsafe { read_entry(pml4_phys, virt.pml4_index(), hhdm) };
    if !pml4e.is_present() {
        return Err(KernelError::InvalidAddress);
    }

    let pdpte = unsafe { read_entry(pml4e.phys_addr(), virt.pdpt_index(), hhdm) };
    if !pdpte.is_present() || pdpte.is_huge() {
        return Err(KernelError::InvalidAddress);
    }

    let pde = unsafe { read_entry(pdpte.phys_addr(), virt.pd_index(), hhdm) };
    if !pde.is_present() || pde.is_huge() {
        return Err(KernelError::InvalidAddress);
    }

    let pt = pde.phys_addr();
    let base_pt_index = virt.pt_index();

    // Write the swap entry into all 4 PTEs.
    for i in 0..HW_PAGES_PER_FRAME {
        // SAFETY: pt valid, index < 512 (base_pt_index is aligned to
        // a 4-entry group within the 512-entry table).
        unsafe { write_entry(pt, base_pt_index + i, entry, hhdm); }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// TLB and CR3 operations
// ---------------------------------------------------------------------------

/// Invalidate the TLB entry for a single 4 KiB virtual page.
///
/// Must be called after modifying a page table entry that affects the
/// active address space, to ensure the CPU uses the updated mapping.
///
/// # Safety
///
/// No memory-safety issue — `invlpg` is always valid in ring 0.
/// Calling it unnecessarily is a performance cost (the TLB will
/// reload the entry on the next access to that page).
#[inline]
#[allow(dead_code)] // Low-level TLB primitive; used by tlb module.
pub unsafe fn invlpg(addr: u64) {
    // SAFETY: invlpg is always safe in ring 0.
    unsafe {
        core::arch::asm!(
            "invlpg [{}]",
            in(reg) addr,
            options(nostack, preserves_flags),
        );
    }
}

/// Flush TLB entries for an entire 16 KiB frame (4 × `invlpg`) on **all
/// online CPUs**.
///
/// On single-CPU systems this is equivalent to 4 `invlpg` instructions.
/// On SMP systems this sends a TLB shootdown IPI so all CPUs flush the
/// range.
///
/// # Safety
///
/// Same as [`invlpg`] — always safe in ring 0.
#[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
pub unsafe fn flush_frame(virt: VirtAddr) {
    // Delegate to the TLB module which handles both local and cross-CPU
    // invalidation.  4 hardware pages per 16 KiB frame.
    crate::tlb::flush_range(virt.as_u64(), HW_PAGES_PER_FRAME as u32);
}

/// Flush TLB entries for an entire 16 KiB frame on **only the local
/// CPU** (no IPI).
///
/// Use this when cross-CPU consistency is guaranteed by other means
/// (e.g., the address space is not yet active on other CPUs, or the
/// caller is modifying identity mappings during single-threaded boot).
///
/// # Safety
///
/// Same as [`invlpg`].  The caller must ensure no other CPU relies
/// on the TLB entries being flushed.
#[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
pub unsafe fn flush_frame_local(virt: VirtAddr) {
    let base = virt.as_u64();
    for i in 0..HW_PAGES_PER_FRAME {
        // SAFETY: invlpg is always safe.
        unsafe { invlpg(base + (i as u64) * (HW_PAGE_SIZE as u64)); }
    }
}

/// Read the CR3 register (raw value including PCID/flags in low bits).
///
/// The PML4 physical address is in bits 12–63.  Use [`cr3_to_pml4`]
/// to extract just the address.
#[inline]
#[must_use]
pub fn read_cr3() -> u64 {
    let cr3: u64;
    // SAFETY: Reading CR3 is always safe in ring 0.
    unsafe {
        core::arch::asm!(
            "mov {}, cr3",
            out(reg) cr3,
            options(nomem, nostack, preserves_flags),
        );
    }
    cr3
}

/// Extract the PML4 physical address from a raw CR3 value.
///
/// Strips the PCID field (bits 0–11) and any reserved/flag bits.
#[inline]
#[must_use]
pub const fn cr3_to_pml4(cr3: u64) -> u64 {
    cr3 & PHYS_ADDR_MASK
}

/// Get the currently active PML4 physical address.
///
/// Convenience wrapper around `cr3_to_pml4(read_cr3())`.
#[inline]
#[must_use]
pub fn active_pml4_phys() -> u64 {
    cr3_to_pml4(read_cr3())
}

/// Write a new value to CR3, switching the active page table.
///
/// This flushes the entire TLB (except entries marked [`GLOBAL`]).
///
/// # Safety
///
/// - `pml4_phys` must be the physical address of a valid, 4 KiB-
///   aligned PML4 table.
/// - The new page table must map the currently executing code and the
///   current stack, or the CPU will immediately triple-fault.
///
/// [`GLOBAL`]: PageFlags::GLOBAL
#[inline]
pub unsafe fn write_cr3(pml4_phys: u64) {
    // SAFETY: Caller guarantees the new PML4 is valid and maps the
    // running code + stack.
    unsafe {
        core::arch::asm!(
            "mov cr3, {}",
            in(reg) pml4_phys,
            options(nostack, preserves_flags),
        );
    }
}

// ---------------------------------------------------------------------------
// Per-process address space
// ---------------------------------------------------------------------------

/// Allocate a new PML4 table for a userspace process.
///
/// The new PML4 is initialized as follows:
/// - Entries 0–255 (userspace half): zeroed — the process starts with
///   no userspace mappings.
/// - Entries 256–511 (kernel half): copied from the current (kernel)
///   PML4 — the kernel is mapped identically in every address space.
///
/// Returns the physical address of the new PML4 (4 KiB aligned).
///
/// # Errors
///
/// - [`KernelError::OutOfMemory`] if page allocation fails.
pub fn alloc_pml4() -> KernelResult<u64> {
    let hhdm = hhdm().ok_or(KernelError::NotSupported)?;

    // Allocate a fresh 4 KiB page for the PML4.
    let new_pml4_phys = PT_PAGE_POOL.lock().alloc()?;

    // Copy kernel-half PML4 entries (256–511) from the current page table.
    let kernel_pml4 = cr3_to_pml4(read_cr3());
    let src_virt = (kernel_pml4 + hhdm) as *const u64;
    let dst_virt = (new_pml4_phys + hhdm) as *mut u64;

    // SAFETY:
    // - kernel_pml4 is the active PML4 (valid, mapped via HHDM).
    // - new_pml4_phys is freshly allocated and zeroed (pool zeroes on alloc).
    // - Entries 256–511 are the kernel half; copying them shares the
    //   kernel's PDPT/PD/PT structures (read-only sharing at the PML4
    //   level — we never modify kernel page table entries through the
    //   process PML4).
    // - Each entry is 8 bytes, 256 entries = 2048 bytes.
    unsafe {
        // Userspace entries 0–255 are already zeroed by alloc().
        // Copy kernel entries 256–511.
        core::ptr::copy_nonoverlapping(
            src_virt.add(256), // Entry 256 in source
            dst_virt.add(256), // Entry 256 in dest
            256,               // 256 entries
        );
    }

    // Register the new address space for per-process memory accounting.
    super::accounting::init_address_space(new_pml4_phys);

    Ok(new_pml4_phys)
}

/// Free a PML4 table previously allocated by [`alloc_pml4`].
///
/// This only frees the PML4 page itself.  The caller must first unmap
/// and free all userspace page table pages and their mapped frames.
/// Kernel entries (256–511) are shared and must NOT be freed.
///
/// # Safety
///
/// - `pml4_phys` must have been returned by [`alloc_pml4`].
/// - No CPU may be using this PML4 (i.e., it must not be in any CR3).
/// - All userspace mappings must have been torn down first.
pub unsafe fn free_pml4(pml4_phys: u64) {
    // Remove from per-process memory accounting before freeing.
    super::accounting::destroy_address_space(pml4_phys);

    let mut pool = PT_PAGE_POOL.lock();
    // SAFETY: Caller guarantees the PML4 is no longer in use.
    unsafe {
        pool._free(pml4_phys);
    }
}

/// Tear down the user half of a process address space.
///
/// Walks PML4 entries 0–255 (the user half — entries 256–511 are the
/// shared kernel mappings and must NOT be freed).  For every mapped
/// leaf page, frees the physical frame back to the frame allocator.
/// For every intermediate page table page (PDPT, PD, PT), returns it
/// to the page table page pool.  Finally, frees the PML4 page itself.
///
/// This is the proper cleanup path for process destruction.  After
/// this call, all physical memory used by the process's address space
/// is reclaimed.
///
/// # Safety
///
/// - `pml4_phys` must have been returned by [`alloc_pml4`].
/// - No CPU may be using this PML4 (must not be in any CR3).
/// - No thread may still be accessing the user address space.
#[allow(clippy::arithmetic_side_effects)]
pub unsafe fn destroy_user_address_space(pml4_phys: u64) {
    // SAFETY: Caller guarantees no CPU is using this address space.
    unsafe { clear_user_address_space(pml4_phys); }

    // Free the PML4 page itself.
    // SAFETY: pml4_phys was allocated by alloc_pml4 (caller contract)
    // and clear_user_address_space just released all child pages.
    unsafe { free_pml4(pml4_phys); }
}

/// Clear all user-space mappings from a PML4, freeing all mapped frames
/// and intermediate page table pages (PDPT/PD/PT).  The PML4 page
/// itself is NOT freed — its user entries (0–255) are simply zeroed.
///
/// This is used by `exec` to replace a process's address space without
/// destroying the PML4 (which is still referenced by the PCB and by
/// any kernel stack/thread that will use the same PML4 for the new
/// image).
///
/// After this call, the PML4's user half is completely empty — ready
/// to have new ELF segments and a stack mapped into it.
///
/// # Safety
///
/// - `pml4_phys` must be a valid PML4 physical address.
/// - No CPU may have this PML4 loaded in CR3 while user entries are
///   being walked (the kernel half 256–511 is untouched so kernel
///   code can still execute, but user TLB entries may go stale).
/// - All frames and page table pages in the user half must have been
///   allocated by this module (they are returned to the frame allocator
///   and PT page pool respectively).
pub unsafe fn clear_user_address_space(pml4_phys: u64) {
    let Some(hhdm) = hhdm() else {
        return;
    };

    // Reset the RSS counter to 0.  We're about to free all user pages
    // directly (not through unmap_frame), so the per-frame uncharge
    // calls won't fire.  The peak RSS is preserved for diagnostics.
    super::accounting::reset_rss(pml4_phys);

    // Walk PML4 entries 0–255 (user half only).
    // Entries 256–511 point to shared kernel PDPT/PD/PT pages that
    // must not be freed (they belong to every address space).
    for pml4_idx in 0..256_usize {
        // SAFETY: pml4_phys is valid (caller guarantee), index < 512.
        let pml4e = unsafe { read_entry(pml4_phys, pml4_idx, hhdm) };
        if !pml4e.is_present() {
            continue;
        }

        let pdpt_phys = pml4e.phys_addr();

        for pdpt_idx in 0..ENTRIES_PER_TABLE {
            // SAFETY: pdpt_phys is from present pml4e, index < 512.
            let pdpte = unsafe { read_entry(pdpt_phys, pdpt_idx, hhdm) };
            if !pdpte.is_present() {
                continue;
            }
            // Skip 1 GiB huge pages (shouldn't exist in user space,
            // but be defensive).
            if pdpte.is_huge() {
                continue;
            }

            let pd_phys = pdpte.phys_addr();

            for pd_idx in 0..ENTRIES_PER_TABLE {
                // SAFETY: pd_phys is from present pdpte, index < 512.
                let pde = unsafe { read_entry(pd_phys, pd_idx, hhdm) };
                if !pde.is_present() {
                    continue;
                }
                // Skip 2 MiB huge pages.
                if pde.is_huge() {
                    continue;
                }

                let pt_phys = pde.phys_addr();

                // Walk PT entries in groups of HW_PAGES_PER_FRAME (4).
                // Our 16 KiB frames are always mapped as 4 consecutive
                // 4 KiB PTEs with the first aligned to a multiple of 4.
                for base_pt_idx in (0..ENTRIES_PER_TABLE).step_by(HW_PAGES_PER_FRAME) {
                    // SAFETY: pt_phys is from present pde, index < 512.
                    let pte = unsafe { read_entry(pt_phys, base_pt_idx, hhdm) };
                    if !pte.is_present() {
                        continue;
                    }

                    // The PTE points to a 4 KiB hardware page; the
                    // 16 KiB frame base is the address aligned down to
                    // FRAME_SIZE.
                    let frame_base = pte.phys_addr() & !(FRAME_SIZE as u64 - 1);
                    if let Some(frame) = PhysFrame::from_addr(frame_base) {
                        // Drop this frame's reverse-mapping (if any) before
                        // freeing it.  A demand-faulted user page is registered
                        // in rmap (proc::pcb fault handler, cow, swap); without
                        // removing it here the entry would dangle, pointing into
                        // a physical frame that is about to be freed and handed
                        // to another address space — compaction/swap could then
                        // migrate or evict a frame that no longer belongs to
                        // this process.  rmap::remove is a no-op for frames that
                        // were never tracked (eagerly-mapped / kernel frames).
                        //
                        // The mapping key is (frame_phys, pml4_phys, virt_frame
                        // _base); reconstruct the 16 KiB-aligned user virtual
                        // address from the walk indices (the user half has bit
                        // 47 == 0, so no sign extension is needed).
                        let virt_frame_base = ((pml4_idx as u64) << 39)
                            | ((pdpt_idx as u64) << 30)
                            | ((pd_idx as u64) << 21)
                            | ((base_pt_idx as u64) << 12);
                        super::rmap::remove(frame_base, pml4_phys, virt_frame_base);
                        // Same reasoning for the swap reclaimable set: a
                        // demand-faulted page is registered there too
                        // (proc::pcb fault handler), and the bulk teardown here
                        // bypasses unmap_frame (which is what normally
                        // unregisters).  Drop it so the Clock reclaimer can
                        // never select a freed/reused frame.  No-op if absent.
                        super::swap::unregister_reclaimable(pml4_phys, virt_frame_base);
                        // SAFETY: This frame was mapped exclusively
                        // into this process's address space and the
                        // process is being destroyed / exec'd.
                        let _ = unsafe { frame::free_frame(frame) };
                    }
                }

                // Free the PT page back to the pool.
                let mut pool = PT_PAGE_POOL.lock();
                // SAFETY: pt_phys was allocated by walk_or_create for
                // this process and is no longer referenced.
                unsafe { pool._free(pt_phys); }
            }

            // Free the PD page.
            let mut pool = PT_PAGE_POOL.lock();
            // SAFETY: pd_phys was allocated for this process's page
            // tables and all child PT pages have been freed above.
            unsafe { pool._free(pd_phys); }
        }

        // Free the PDPT page.
        let mut pool = PT_PAGE_POOL.lock();
        // SAFETY: pdpt_phys was allocated for this process and all
        // child PD pages have been freed above.
        unsafe { pool._free(pdpt_phys); }

        // Zero the PML4 entry so the user half is clean.
        // SAFETY: pml4_phys valid, pml4_idx < 256 (loop bound), hhdm valid.
        unsafe { write_entry(pml4_phys, pml4_idx, PageTableEntry::EMPTY, hhdm); }
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Committed range mapping (atomically alloc + map + rollback)
// ---------------------------------------------------------------------------

/// Allocate and map a contiguous range of zeroed frames into a virtual
/// address range.  Provides all-or-nothing semantics: if any frame
/// allocation or mapping fails partway through, all previously mapped
/// frames are unmapped and freed before returning the error.
///
/// This is the "committed allocation" path for `mmap` — physical frames
/// are reserved immediately (no demand paging).  All frames are zeroed
/// before mapping.
///
/// # Arguments
///
/// - `pml4_phys`: The root page table (physical address).
/// - `base_virt`: Starting virtual address (must be 16 KiB aligned).
/// - `num_frames`: Number of 16 KiB frames to map.
/// - `flags`: Page flags (e.g., USER | WRITABLE | PRESENT).
///
/// # Returns
///
/// `Ok(())` if all frames were allocated and mapped successfully.
/// `Err(...)` if allocation or mapping failed (all partial work undone).
///
/// # Safety
///
/// - `pml4_phys` must be a valid PML4 table.
/// - The virtual address range `[base_virt, base_virt + num_frames * 16K)`
///   must not already be mapped.
/// - The caller must flush the TLB for mapped addresses on success.
#[allow(clippy::arithmetic_side_effects)]
#[allow(dead_code)] // API for kernel-ipc zone; used when mmap partial-failure fix is wired in.
pub unsafe fn map_committed_range(
    pml4_phys: u64,
    base_virt: VirtAddr,
    num_frames: usize,
    flags: PageFlags,
) -> KernelResult<()> {
    let hhdm = hhdm().ok_or(KernelError::NotSupported)?;

    for i in 0..num_frames {
        let va = VirtAddr::new(base_virt.as_u64() + (i as u64) * (FRAME_SIZE as u64));

        // Allocate a physical frame.
        let phys = match frame::alloc_frame() {
            Ok(f) => f,
            Err(e) => {
                // Rollback: unmap and free all frames mapped so far.
                // SAFETY: we just mapped these frames successfully.
                unsafe { rollback_range(pml4_phys, base_virt, i); }
                return Err(e);
            }
        };

        // Zero the frame via HHDM.
        let frame_virt = phys.to_virt(hhdm);
        // SAFETY: frame_virt is the HHDM mapping of a freshly allocated
        // frame that we exclusively own.
        unsafe {
            core::ptr::write_bytes(frame_virt as *mut u8, 0, FRAME_SIZE);
        }

        // Map the frame at the target virtual address.
        // SAFETY: pml4_phys is valid, phys is freshly allocated.
        if let Err(e) = unsafe { map_frame(pml4_phys, va, phys, flags) } {
            // Free the frame we just allocated but couldn't map.
            // SAFETY: phys is freshly allocated and not yet referenced.
            let _ = unsafe { frame::free_frame(phys) };
            // Rollback all previously mapped frames.
            // SAFETY: frames 0..i were mapped successfully.
            unsafe { rollback_range(pml4_phys, base_virt, i); }
            return Err(e);
        }
    }

    Ok(())
}

/// Unmap and free a contiguous range of frames.
///
/// The counterpart to [`map_committed_range`].  Unmaps `num_frames`
/// frames starting at `base_virt` and returns each physical frame to
/// the allocator.  Silently skips frames that are not mapped (allows
/// partial cleanup).
///
/// # Safety
///
/// - `pml4_phys` must be a valid PML4 table.
/// - The caller must flush the TLB for the unmapped range afterward.
/// - Any user-visible data in the frames is lost.
#[allow(clippy::arithmetic_side_effects)]
#[allow(dead_code)] // API for kernel-ipc zone; used when mmap cleanup is wired in.
pub unsafe fn unmap_committed_range(
    pml4_phys: u64,
    base_virt: VirtAddr,
    num_frames: usize,
) {
    for i in 0..num_frames {
        let va = VirtAddr::new(base_virt.as_u64() + (i as u64) * (FRAME_SIZE as u64));
        // SAFETY: pml4_phys is valid.  If not mapped, unmap_frame returns Err.
        if let Ok(phys) = unsafe { unmap_frame(pml4_phys, va) } {
            // SAFETY: frame was exclusively mapped and is now unreferenced.
            let _ = unsafe { frame::free_frame(phys) };
        }
    }
}

/// Rollback helper: unmap and free `count` frames starting at `base_virt`.
///
/// Used internally by [`map_committed_range`] to undo partial mappings.
///
/// # Safety
///
/// All frames at `base_virt + 0..count * FRAME_SIZE` must be validly
/// mapped in `pml4_phys`.
#[allow(clippy::arithmetic_side_effects, dead_code)]
unsafe fn rollback_range(pml4_phys: u64, base_virt: VirtAddr, count: usize) {
    for j in 0..count {
        let va = VirtAddr::new(base_virt.as_u64() + (j as u64) * (FRAME_SIZE as u64));
        // SAFETY: we mapped this frame successfully in a prior iteration.
        if let Ok(phys) = unsafe { unmap_frame(pml4_phys, va) } {
            // SAFETY: frame was allocated by us and is no longer mapped.
            let _ = unsafe { frame::free_frame(phys) };
        }
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Run a boot-time self-test of the page table subsystem.
///
/// Tests:
/// 1. `VirtAddr` index extraction on known values.
/// 2. `translate` on existing HHDM mappings.
/// 3. `map_frame` + verify all 4 hardware pages + write/read.
/// 4. `change_flags` verification.
/// 5. `unmap_frame` + verify.
#[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
pub fn self_test() -> KernelResult<()> {
    serial_println!("[pt] Running page table self-test...");

    let hhdm = hhdm().ok_or(KernelError::NotSupported)?;
    let pml4_phys = cr3_to_pml4(read_cr3());

    // -- Test 1: VirtAddr decomposition ----------------------------------------
    test_virt_addr_decomposition()?;

    // -- Test 2: Translate HHDM mapping ----------------------------------------
    let test_frame = frame::alloc_frame()?;
    let hhdm_virt = VirtAddr::new(test_frame.to_virt(hhdm));

    match translate(pml4_phys, hhdm_virt) {
        Some(phys) if phys == test_frame.addr() => {}
        Some(phys) => {
            serial_println!(
                "[pt]   FAIL: HHDM translate: expected {:#x}, got {:#x}",
                test_frame.addr(),
                phys
            );
            // SAFETY: test_frame was just allocated and is being freed
            // on the error path before returning.
            unsafe { frame::free_frame(test_frame)?; }
            return Err(KernelError::InternalError);
        }
        None => {
            serial_println!("[pt]   FAIL: HHDM address {} not mapped", hhdm_virt);
            // SAFETY: test_frame was just allocated; freed on error.
            unsafe { frame::free_frame(test_frame)?; }
            return Err(KernelError::InternalError);
        }
    }
    serial_println!("[pt]   HHDM translate: OK");

    // -- Tests 3-5: Map, change flags, unmap -----------------------------------
    test_map_unmap(pml4_phys, test_frame, hhdm)?;

    serial_println!("[pt] Page table self-test PASSED");
    Ok(())
}

/// Test 1: Verify `VirtAddr` index decomposition on a known value.
#[allow(clippy::arithmetic_side_effects)]
fn test_virt_addr_decomposition() -> KernelResult<()> {
    // PML4=1, PDPT=2, PD=3, PT=4, offset=0x123
    let constructed =
        (1_u64 << 39) | (2_u64 << 30) | (3_u64 << 21) | (4_u64 << 12) | 0x123;
    let va = VirtAddr::new(constructed);

    if va.pml4_index() != 1
        || va.pdpt_index() != 2
        || va.pd_index() != 3
        || va.pt_index() != 4
        || va.page_offset() != 0x123
    {
        serial_println!("[pt]   FAIL: VirtAddr decomposition");
        return Err(KernelError::InternalError);
    }
    serial_println!("[pt]   VirtAddr decomposition: OK");
    Ok(())
}

/// Tests 3–5: Map a frame, verify translations and write/read, change
/// flags, then unmap and verify cleanup.
#[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
fn test_map_unmap(pml4_phys: u64, test_frame: PhysFrame, hhdm: u64) -> KernelResult<()> {
    let test_virt = VirtAddr::new(TEST_MAP_BASE);

    // Ensure the test address is not already mapped.
    if translate(pml4_phys, test_virt).is_some() {
        serial_println!(
            "[pt]   SKIP: test address {:#x} already mapped",
            TEST_MAP_BASE
        );
        // SAFETY: test_frame was just allocated; freed because the
        // test address is already in use.
        unsafe { frame::free_frame(test_frame)?; }
        return Ok(());
    }

    let map_flags = PageFlags::PRESENT
        | PageFlags::WRITABLE
        | PageFlags::GLOBAL
        | PageFlags::NO_EXECUTE;

    // SAFETY: pml4_phys is the active PML4, test_frame is valid,
    // test_virt is in kernel space and currently unmapped.
    unsafe {
        map_frame(pml4_phys, test_virt, test_frame, map_flags)?;
        flush_frame(test_virt);
    }

    // Verify all 4 hardware pages translate correctly.
    for i in 0..HW_PAGES_PER_FRAME {
        let check_virt =
            VirtAddr::new(test_virt.as_u64() + (i as u64) * (HW_PAGE_SIZE as u64));
        let expected_phys =
            test_frame.addr() + (i as u64) * (HW_PAGE_SIZE as u64);

        match translate(pml4_phys, check_virt) {
            Some(phys) if phys == expected_phys => {}
            other => {
                serial_println!(
                    "[pt]   FAIL: page {} translate: expected {:#x}, got {:?}",
                    i, expected_phys, other
                );
                return Err(KernelError::InternalError);
            }
        }
    }

    // Write through the new mapping and verify via the HHDM mapping.
    // SAFETY: test_virt is mapped to test_frame (valid and writable).
    unsafe {
        let mapped_ptr = test_virt.as_u64() as *mut u8;
        ptr::write_bytes(mapped_ptr, 0xBB, 16);

        let hhdm_ptr = test_frame.to_virt(hhdm) as *const u8;
        let byte = ptr::read(hhdm_ptr);
        if byte != 0xBB {
            serial_println!(
                "[pt]   FAIL: write through new mapping not visible via HHDM"
            );
            return Err(KernelError::InternalError);
        }
    }
    serial_println!("[pt]   Map + translate (4 pages) + write/verify: OK");

    // -- Change flags: remove WRITABLE, verify in PTE --------------------------
    let ro_flags = PageFlags::PRESENT | PageFlags::GLOBAL | PageFlags::NO_EXECUTE;

    // SAFETY: pml4_phys is the active PML4, test_virt is mapped (we just
    // mapped it above).  change_flags only modifies leaf PTE flags.
    unsafe {
        change_flags(pml4_phys, test_virt, ro_flags)?;
        flush_frame(test_virt);
    }

    // SAFETY for read_entry calls: pml4_phys is the active PML4; each
    // subsequent table address is from a present parent entry.
    let pml4e = unsafe { read_entry(pml4_phys, test_virt.pml4_index(), hhdm) };
    let pdpte = unsafe {
        read_entry(pml4e.phys_addr(), test_virt.pdpt_index(), hhdm)
    };
    let pde = unsafe {
        read_entry(pdpte.phys_addr(), test_virt.pd_index(), hhdm)
    };
    let pte = unsafe {
        read_entry(pde.phys_addr(), test_virt.pt_index(), hhdm)
    };

    if pte.flags().contains(PageFlags::WRITABLE) {
        serial_println!("[pt]   FAIL: WRITABLE still set after change_flags");
        return Err(KernelError::InternalError);
    }
    if !pte.flags().contains(PageFlags::NO_EXECUTE) {
        serial_println!("[pt]   FAIL: NO_EXECUTE cleared unexpectedly");
        return Err(KernelError::InternalError);
    }
    serial_println!("[pt]   Change flags (remove WRITABLE): OK");

    // -- Unmap and verify ------------------------------------------------------
    // SAFETY: pml4_phys is the active PML4, test_virt was mapped above.
    let unmapped = unsafe { unmap_frame(pml4_phys, test_virt)? };
    // SAFETY: TLB flush after unmap to ensure stale translations are gone.
    unsafe { flush_frame(test_virt); }

    if unmapped != test_frame {
        serial_println!("[pt]   FAIL: unmap returned wrong frame");
        return Err(KernelError::InternalError);
    }

    if translate(pml4_phys, test_virt).is_some() {
        serial_println!("[pt]   FAIL: address still mapped after unmap");
        return Err(KernelError::InternalError);
    }
    serial_println!("[pt]   Unmap + verify: OK");

    // Free the test frame.
    // SAFETY: test_frame was allocated by us, unmapped, no references remain.
    unsafe { frame::free_frame(test_frame)?; }

    // -- Test: Double-map returns AlreadyExists, rollback cleans up ------
    test_double_map_rollback(pml4_phys)?;

    Ok(())
}

/// Test that mapping a frame over an already-mapped address returns
/// `AlreadyExists` and that the single-pass rollback logic correctly
/// cleans up any partially-written PTEs.
#[allow(clippy::arithmetic_side_effects)]
fn test_double_map_rollback(pml4_phys: u64) -> KernelResult<()> {
    // Use a fresh virtual address for this test (offset from TEST_MAP_BASE).
    let base1: u64 = TEST_MAP_BASE + 0x4000; // +1 frame
    let base2: u64 = TEST_MAP_BASE + 0x8000; // +2 frames

    let frame1 = frame::alloc_frame()?;
    let frame2 = frame::alloc_frame()?;

    let flags = PageFlags::PRESENT | PageFlags::WRITABLE | PageFlags::NO_EXECUTE;

    // Map frame1 at base1.
    // SAFETY: test addresses in kernel space, valid frames.
    unsafe {
        map_frame(pml4_phys, VirtAddr::new(base1), frame1, flags)?;
        flush_frame(VirtAddr::new(base1));
    }

    // Verify it's mapped.
    assert!(
        translate(pml4_phys, VirtAddr::new(base1)).is_some(),
        "frame1 should be mapped at base1"
    );

    // Try to map frame2 at the same address (base1) — should fail.
    // SAFETY: testing double-map error path; frames and PML4 are valid.
    let result = unsafe {
        map_frame(pml4_phys, VirtAddr::new(base1), frame2, flags)
    };
    assert!(
        matches!(result, Err(KernelError::AlreadyExists)),
        "double-map should return AlreadyExists"
    );

    // The original mapping should be intact (rollback must not corrupt it).
    let translated = translate(pml4_phys, VirtAddr::new(base1));
    assert!(
        translated == Some(frame1.addr()),
        "original mapping should be intact after failed double-map"
    );

    // Map frame2 at base2 (different address) — should succeed.
    // SAFETY: base2 is unmapped, frame2 and PML4 are valid.
    unsafe {
        map_frame(pml4_phys, VirtAddr::new(base2), frame2, flags)?;
        flush_frame(VirtAddr::new(base2));
    }
    assert!(
        translate(pml4_phys, VirtAddr::new(base2)) == Some(frame2.addr()),
        "frame2 should be mapped at base2"
    );

    // Cleanup: unmap both and free.
    // SAFETY: both addresses were mapped above.
    let r1 = unsafe { unmap_frame(pml4_phys, VirtAddr::new(base1))? };
    let r2 = unsafe { unmap_frame(pml4_phys, VirtAddr::new(base2))? };
    // SAFETY: TLB flush after unmap.
    unsafe {
        flush_frame(VirtAddr::new(base1));
        flush_frame(VirtAddr::new(base2));
    }
    assert!(r1 == frame1, "unmap base1 should return frame1");
    assert!(r2 == frame2, "unmap base2 should return frame2");
    // SAFETY: frames were just unmapped and no longer referenced.
    unsafe {
        frame::free_frame(frame1)?;
        frame::free_frame(frame2)?;
    }

    serial_println!("[pt]   Double-map rollback: OK");
    Ok(())
}
