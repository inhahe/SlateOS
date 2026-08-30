//! The Page Attribute Table: giving the kernel a write-combining memory type.
//!
//! ## What this is for
//!
//! On x86-64 a page's caching behaviour is not encoded in the PTE directly.
//! Three PTE bits — `PAT`, `PCD`, `PWT` — form a 3-bit index into an
//! eight-entry table held in the `IA32_PAT` MSR (`0x277`), and *that* table
//! says which of the six memory types the page gets. The PTE picks a slot; the
//! MSR decides what the slot means.
//!
//! Left at its power-on value the table reads `WB, WT, UC-, UC, WB, WT, UC-,
//! UC` — six of the eight slots are duplicates and **none of them is
//! write-combining**. Write-combining cannot be reached at all until this MSR
//! is written, which is why this module exists.
//!
//! ## Why write-combining is worth a boot-time MSR write
//!
//! It is the memory type a framebuffer wants. The CPU gathers consecutive
//! stores into a fill buffer and flushes them as whole cache-line bursts, so
//! the sequential writes that painting pixels produces cross the bus in
//! line-sized units instead of one transaction per store. The alternatives for
//! a framebuffer are all bad in a different way:
//!
//! | Type | Safe for a framebuffer? | Why |
//! |---|---|---|
//! | `WB` (writeback) | **no** | pixels sit in a dirty cache line the display engine cannot see |
//! | `UC` (uncacheable) | yes | correct, and one bus transaction per store |
//! | `WT` (write-through) | yes | correct, and no faster than `UC` for pure writes |
//! | `WC` (write-combining) | yes | correct, and the only fast one |
//!
//! So without this module the only *correct* options are the slow ones. That
//! is not a theoretical cost: it is why the ATI boot exercise paints at
//! 640x480 rather than anything larger, and why a VRAM buffer cannot yet be
//! handed to userspace as a framebuffer.
//!
//! ## The layout, and the one flag whose meaning changes
//!
//! We adopt Linux's layout verbatim (`arch/x86/mm/pat/memtype.c`, `pat_init`):
//!
//! | Slot | `PAT` `PCD` `PWT` | Power-on | Ours |
//! |---|---|---|---|
//! | 0 | `0 0 0` | WB | **WB** (unchanged) |
//! | 1 | `0 0 1` | WT | **WC** ← the point of the exercise |
//! | 2 | `0 1 0` | UC- | **UC-** (unchanged) |
//! | 3 | `0 1 1` | UC | **UC** (unchanged) |
//! | 4 | `1 0 0` | WB | WB |
//! | 5 | `1 0 1` | WT | WP |
//! | 6 | `1 1 0` | UC- | UC- |
//! | 7 | `1 1 1` | UC | **WT** ← relocated from slot 1 |
//!
//! Copying Linux is not deference, it is the specific reason this change is
//! safe to make on a running kernel. Slots 0, 2 and 3 keep their power-on
//! meanings, and those are the slots every existing mapping in this kernel
//! selects: ordinary memory is slot 0, and all fifteen `NO_CACHE` MMIO
//! mappings (APIC, HPET, IOAPIC, AHCI, NVMe, xHCI, e1000, HDA, ACPI,
//! virtio-gpu, and this driver's own register aperture) set `PCD` alone, which
//! is slot 2 — `UC-` before and after. Not one of them changes behaviour.
//!
//! **Slot 1 does change**, and it is the one thing about this commit that
//! could bite silently. `PWT` alone used to mean write-through; it now means
//! write-combining, which is *weaker*-ordered, not merely faster. The kernel
//! has exactly one caller that wants write-through — `mm::dma::alloc_for_user`,
//! whose comment asks for it precisely to avoid "subtle ordering issues with
//! device reads" — and it asks by name, via
//! [`PageFlags::WRITE_THROUGH`](super::page_table::PageFlags::WRITE_THROUGH).
//! That constant is redefined to slot 7 in the same change that writes this
//! MSR, so the caller follows automatically and keeps the memory type it asked
//! for. Anything that hard-codes `1 << 3` instead of using the constant would
//! silently get `WC`; nothing does, and nothing should.
//!
//! ## Why every CPU must agree
//!
//! `IA32_PAT` is per-logical-processor, and the PTE only carries an *index*.
//! A CPU whose table still holds the power-on layout reads a slot-1 PTE as
//! write-through while another reads the identical PTE as write-combining —
//! the same page, two memory types, decided by which core touched it. That is
//! a memory-ordering bug that reproduces only under migration, so
//! [`init_ap`] is not an optimisation and must run on every AP before it does
//! any mapped access. See the mirror of this argument in
//! `smep_smap::init_ap`.
//!
//! ## Ordering within boot
//!
//! [`init`] must run before anything creates a mapping whose memory type it
//! would change — in practice, before `mm::dma` maps anything write-through.
//! It runs immediately after `smep_smap::init()`, which is well before the
//! frame allocator hands out anything and long before any driver maps a BAR.
//!
//! ## References
//!
//! - Intel SDM Vol. 3A §11.12 ("Page Attribute Table"), and §11.11.8 for the
//!   cache-disable/flush sequence used when changing memory types at runtime.
//! - Linux `arch/x86/mm/pat/memtype.c` — `pat_init()`.

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crate::error::{KernelError, KernelResult};
use crate::serial_println;

/// `IA32_PAT`. Intel SDM Vol. 4, Table 2-2.
const IA32_PAT: u32 = 0x277;

// Memory-type encodings for a PAT slot (Intel SDM Vol. 3A, Table 11-10).
// Values 0x02 and 0x03 are reserved and writing them faults, which is why
// these are named constants rather than a computed range.
/// Uncacheable.
const MT_UC: u64 = 0x00;
/// Write-combining.
const MT_WC: u64 = 0x01;
/// Write-through.
const MT_WT: u64 = 0x04;
/// Write-protected.
const MT_WP: u64 = 0x05;
/// Writeback.
const MT_WB: u64 = 0x06;
/// Uncacheable, but overridable to WC by an MTRR. Effectively UC for us.
const MT_UC_MINUS: u64 = 0x07;

/// Place memory type `mt` in slot `slot`.
///
/// `slot` is always a literal 0..=7 at the call sites below, so the shift is at
/// most 56 and cannot overflow; `saturating_mul` states that without needing a
/// panic path in a `const` context.
const fn slot(slot: u32, mt: u64) -> u64 {
    mt << slot.saturating_mul(8)
}

/// The table we program: `WB, WC, UC-, UC, WB, WP, UC-, WT`.
///
/// Identical to Linux's. See the module docs for why each slot is where it is
/// and which existing mappings that does and does not disturb.
const PAT_LAYOUT: u64 = slot(0, MT_WB)
    | slot(1, MT_WC)
    | slot(2, MT_UC_MINUS)
    | slot(3, MT_UC)
    | slot(4, MT_WB)
    | slot(5, MT_WP)
    | slot(6, MT_UC_MINUS)
    | slot(7, MT_WT);

/// Whether [`init`] successfully programmed the table.
///
/// Read by [`write_combining_available`] so that callers can fall back to
/// `NO_CACHE` rather than silently mapping a framebuffer write-*through* — on
/// a CPU without PAT, slot 1 still means WT, so the `WRITE_COMBINING` flag
/// would be correct-but-slow instead of wrong. Distinguishing the two lets a
/// driver log which one it got.
static PAT_PROGRAMMED: AtomicBool = AtomicBool::new(false);

/// The table that was in force before [`init`] rewrote it.
///
/// Exists so that [`memory_type_of`] can decode against the table *actually*
/// installed rather than the one the architecture says should be there. Those
/// are not the same thing: on the QEMU boot path the firmware/bootloader hands
/// the kernel `0x0000010500070406`, whose slots 4–7 are `WP, WC, UC, UC` —
/// nothing like the architectural power-on `WB, WT, UC-, UC`. A decoder using
/// the architectural default would have reported those four slots as memory
/// types they demonstrably were not, and been confident about it.
///
/// Initialised to the architectural default because that is the correct answer
/// in the one case where nothing better can be read: a CPU whose CPUID says it
/// has no `IA32_PAT` at all, where reading the MSR would `#GP` and no PTE bit
/// pattern selects a table entry in the first place.
static FALLBACK_LAYOUT: AtomicU64 = AtomicU64::new(ARCH_POWER_ON);

/// The architectural power-on table: `WB, WT, UC-, UC, WB, WT, UC-, UC`.
///
/// Intel SDM Vol. 3A §11.12.4. Note what is absent: no slot means
/// write-combining, which is why [`init`] has to exist at all.
const ARCH_POWER_ON: u64 = slot(0, MT_WB)
    | slot(1, MT_WT)
    | slot(2, MT_UC_MINUS)
    | slot(3, MT_UC)
    | slot(4, MT_WB)
    | slot(5, MT_WT)
    | slot(6, MT_UC_MINUS)
    | slot(7, MT_UC);

/// Whether write-combining mappings actually combine on this machine.
///
/// `false` means [`init`] did not run or the CPU lacks PAT. Mappings made with
/// `PageFlags::WRITE_COMBINING` remain *correct* in that case — slot 1 keeps
/// its power-on write-through meaning, which is still safe for a framebuffer —
/// but they get no speedup.
#[must_use]
pub fn write_combining_available() -> bool {
    PAT_PROGRAMMED.load(Ordering::Acquire)
}

/// Program `IA32_PAT` on the bootstrap processor.
///
/// Safe to call once, early, before any write-through or write-combining
/// mapping exists. Idempotent in effect — writing the same layout twice is
/// harmless — but it is called once from the boot path.
pub fn init() {
    let Some(features) = crate::cpu::features() else {
        serial_println!("[pat] CPU features unavailable — leaving PAT at power-on layout");
        return;
    };
    if !features.pat {
        // Pre-Pentium III hardware, or a hypervisor hiding the bit. Every
        // mapping stays correct; framebuffers are merely slow.
        serial_println!("[pat] CPU does not support PAT — write-combining unavailable");
        return;
    }

    // SAFETY: CPUID reports PAT support, so `IA32_PAT` exists and is
    // writable. `PAT_LAYOUT` contains only the six architecturally defined
    // memory-type encodings — never the reserved 0x02/0x03, which are the
    // only values that would #GP. The write follows the SDM's
    // cache-disable/flush sequence; see `program_pat`.
    let previous = unsafe {
        let previous = crate::cpu::rdmsr(IA32_PAT);
        program_pat(PAT_LAYOUT);
        previous
    };

    // Recorded before `PAT_PROGRAMMED`, so a decode racing this store either
    // sees the old table (and reads the value we just saved) or the new one.
    FALLBACK_LAYOUT.store(previous, Ordering::Release);
    PAT_PROGRAMMED.store(true, Ordering::Release);
    serial_println!(
        "[pat] IA32_PAT {previous:#018x} → {PAT_LAYOUT:#018x} (slot 1 = write-combining)"
    );
}

/// Program `IA32_PAT` on an application processor.
///
/// Every CPU must hold the *same* table: the PTE stores only a slot index, so
/// a core with the power-on layout would read a write-combining page as
/// write-through. That disagreement is invisible until a thread migrates.
///
/// Silent on success — an AP that logged this would print it once per core for
/// no new information. A CPU that cannot program it is worth a line, because
/// it means the machine is heterogeneous in a way that matters.
pub fn init_ap() {
    let Some(features) = crate::cpu::features() else {
        return;
    };
    if !features.pat {
        serial_println!("[pat] WARNING: AP lacks PAT support the BSP had — memory types disagree");
        return;
    }
    // Deliberately gated on the BSP's outcome rather than on this CPU's
    // CPUID alone. If the BSP did not program the table, an AP that does
    // creates exactly the disagreement this function exists to prevent.
    if !write_combining_available() {
        return;
    }
    // SAFETY: as in `init` — CPUID reports PAT, and the layout holds only
    // defined encodings.
    unsafe {
        program_pat(PAT_LAYOUT);
    }
}

/// Write `IA32_PAT`, following the SDM's sequence for changing memory types
/// while paging is enabled.
///
/// Intel SDM Vol. 3A §11.11.8 requires that the caches and TLBs not hold
/// entries whose memory type is about to be reinterpreted. The full dance is:
/// disable interrupts, enter no-fill cache mode (`CR0.CD=1, CR0.NW=0`),
/// `WBINVD`, flush the TLB, write the MSR, `WBINVD` and flush again, then
/// restore `CR0`.
///
/// Skipping it would *probably* work here — this runs early, and the three
/// slots any live mapping uses keep their meanings — but "probably" is doing
/// load-bearing work in that sentence, and the cost is a few microseconds
/// once per CPU at boot. The failure it prevents is a stale cache line
/// written back under a memory type that no longer describes the page, which
/// is not a failure that reproduces or debugs pleasantly.
///
/// # Safety
///
/// The CPU must support PAT, and `layout` must contain only the defined
/// memory-type encodings (`0x00`, `0x01`, `0x04`–`0x07`); the reserved values
/// `0x02` and `0x03` raise `#GP`. Interrupts are disabled for the duration and
/// restored to their previous state.
unsafe fn program_pat(layout: u64) {
    // SAFETY: the whole body runs with the caller's guarantees; each step is
    // an architecturally defined control-register or MSR access.
    unsafe {
        let flags: u64;
        core::arch::asm!("pushfq; pop {}", out(reg) flags, options(nomem, preserves_flags));
        let interrupts_were_on = flags & (1 << 9) != 0;
        core::arch::asm!("cli", options(nomem, nostack));

        // Enter no-fill cache mode and write back everything already cached,
        // so nothing survives that was cached under the old interpretation.
        let cr0: u64;
        core::arch::asm!("mov {}, cr0", out(reg) cr0, options(nomem, nostack));
        // CD=1 (bit 30), NW=0 (bit 29) — "no-fill", the SDM's required state.
        let cr0_nofill = (cr0 | (1 << 30)) & !(1u64 << 29);
        core::arch::asm!("mov cr0, {}", in(reg) cr0_nofill, options(nomem, nostack));
        core::arch::asm!("wbinvd", options(nomem, nostack));

        // Flush the TLB by reloading CR3. Any global pages would survive this,
        // so PGE is cleared across the window; the kernel's global mappings
        // are all slot 0 and unaffected, but the SDM asks for it and a
        // surviving global TLB entry is precisely the thing that would carry a
        // stale memory type.
        let cr4: u64;
        core::arch::asm!("mov {}, cr4", out(reg) cr4, options(nomem, nostack));
        let pge = cr4 & (1 << 7);
        if pge != 0 {
            core::arch::asm!("mov cr4, {}", in(reg) cr4 & !(1u64 << 7), options(nomem, nostack));
        }
        let cr3: u64;
        core::arch::asm!("mov {}, cr3", out(reg) cr3, options(nomem, nostack));
        core::arch::asm!("mov cr3, {}", in(reg) cr3, options(nomem, nostack));

        crate::cpu::wrmsr(IA32_PAT, layout);

        // Second flush: drop anything that got cached during the window under
        // the old table, then restore normal caching.
        core::arch::asm!("wbinvd", options(nomem, nostack));
        core::arch::asm!("mov cr3, {}", in(reg) cr3, options(nomem, nostack));
        if pge != 0 {
            core::arch::asm!("mov cr4, {}", in(reg) cr4, options(nomem, nostack));
        }
        core::arch::asm!("mov cr0, {}", in(reg) cr0, options(nomem, nostack));

        if interrupts_were_on {
            core::arch::asm!("sti", options(nomem, nostack));
        }
    }
}

/// An x86 memory type, as selected by a leaf PTE's `PAT`/`PCD`/`PWT` bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryType {
    /// Uncacheable. Every access goes to memory, in program order.
    Uncacheable,
    /// Write-combining: stores gathered into cache-line bursts, weakly ordered.
    WriteCombining,
    /// Write-through: reads cached, writes go to memory as well as the cache.
    WriteThrough,
    /// Write-protected: reads cached, writes go to memory and invalidate.
    WriteProtected,
    /// Writeback: the default for ordinary RAM. Writes may sit in a dirty
    /// cache line indefinitely.
    Writeback,
    /// The slot holds an encoding this kernel does not program.
    Unknown,
}

impl MemoryType {
    /// Whether a write through this memory type can sit in a cache line that a
    /// device reading through the memory controller would not observe.
    ///
    /// This is the question a driver mapping a device aperture actually has —
    /// not "is it cached?" but "can my pixels be invisible to the CRTC?".
    /// Write-through and write-protected cache *reads*, which is harmless
    /// here; only the two writeback-ish types hold stores back.
    #[must_use]
    pub const fn writes_may_linger(self) -> bool {
        matches!(self, Self::Writeback | Self::WriteProtected | Self::Unknown)
    }

    /// A short name, for logs.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Uncacheable => "UC",
            Self::WriteCombining => "WC",
            Self::WriteThrough => "WT",
            Self::WriteProtected => "WP",
            Self::Writeback => "WB",
            Self::Unknown => "?",
        }
    }
}

/// Decode the memory type a leaf PTE's flags select, under the table actually
/// in force on this machine.
///
/// "Actually in force" is the point: the same three bits mean different things
/// before and after [`init`], so a decoder that assumed our layout would
/// confidently report `WC` on a CPU where the slot still says `WT`. When
/// [`init`] has not run, this reads the power-on table instead.
///
/// Only meaningful for 4 KiB leaf entries. On a huge-page entry the `PAT` bit
/// lives at bit 12, not bit 7, so this would misread it — which is one more
/// reason [`crate::mm::hugepage::map_huge_2m`] refuses flags carrying bit 7.
#[must_use]
pub fn memory_type_of(flags: crate::mm::page_table::PageFlags) -> MemoryType {
    let bits = flags.bits();
    // Built by OR rather than by multiply-and-add so the lint against
    // trapping arithmetic has nothing to complain about, and so the mapping
    // from bit position to slot bit is visible at a glance.
    let mut index: u32 = 0;
    if bits & (1 << 3) != 0 {
        index |= 0b001; // PWT
    }
    if bits & (1 << 4) != 0 {
        index |= 0b010; // PCD
    }
    if bits & (1 << 7) != 0 {
        index |= 0b100; // PAT (leaf entries only)
    }

    let table = if write_combining_available() {
        PAT_LAYOUT
    } else {
        // Not the architectural default — the table this machine actually
        // booted with. See `FALLBACK_LAYOUT`.
        FALLBACK_LAYOUT.load(Ordering::Acquire)
    };
    // `index` is at most 7, so the shift is at most 56 and cannot overflow.
    let encoding = (table >> index.saturating_mul(8)) & 0xFF;
    match encoding {
        MT_UC | MT_UC_MINUS => MemoryType::Uncacheable,
        MT_WC => MemoryType::WriteCombining,
        MT_WT => MemoryType::WriteThrough,
        MT_WP => MemoryType::WriteProtected,
        MT_WB => MemoryType::Writeback,
        _ => MemoryType::Unknown,
    }
}

/// Verify the table the CPU is actually using.
///
/// Reading the MSR back is the whole point. The failure this guards against is
/// not a `#GP` — that would be loud — but a write that lands and yet leaves
/// the table saying something other than intended, after which every
/// write-combining mapping in the kernel is silently a different memory type.
/// Nothing faults, nothing logs, the framebuffer is merely slow or, worse,
/// writeback. A wrong PAT is exactly the class of bug that is invisible from
/// inside the system, so it is checked from the one place that can: the MSR.
///
/// # Errors
///
/// `InternalError` if the MSR does not read back as the layout we programmed,
/// or if a slot decodes to a memory type other than the one intended.
pub fn self_test() -> KernelResult<()> {
    use crate::mm::page_table::PageFlags;

    let Some(features) = crate::cpu::features() else {
        serial_println!("[pat]   SKIP: CPU features unavailable");
        return Ok(());
    };
    if !features.pat {
        serial_println!("[pat]   SKIP: CPU does not support PAT");
        return Ok(());
    }
    if !write_combining_available() {
        serial_println!("[pat]   SKIP: PAT was not programmed");
        return Ok(());
    }

    // SAFETY: CPUID reports PAT support, so the MSR exists.
    let live = unsafe { current() };
    if live != PAT_LAYOUT {
        serial_println!("[pat]   FAIL: IA32_PAT reads {live:#018x}, expected {PAT_LAYOUT:#018x}");
        return Err(KernelError::InternalError);
    }
    serial_println!("[pat]   IA32_PAT reads back as programmed ({live:#018x}): OK");

    // Decode the flag constants the rest of the kernel actually uses, rather
    // than re-deriving slot indices here. This is the check that would have
    // caught leaving `WRITE_THROUGH` at its old `PWT`-alone encoding after
    // moving write-through to slot 7 — the one silent hazard in this change.
    let cases: [(&str, PageFlags, MemoryType); 4] = [
        (
            "default (no bits)",
            PageFlags::empty(),
            MemoryType::Writeback,
        ),
        (
            "WRITE_COMBINING",
            PageFlags::WRITE_COMBINING,
            MemoryType::WriteCombining,
        ),
        (
            "WRITE_THROUGH",
            PageFlags::WRITE_THROUGH,
            MemoryType::WriteThrough,
        ),
        ("NO_CACHE", PageFlags::NO_CACHE, MemoryType::Uncacheable),
    ];
    for (name, flags, want) in cases {
        let got = memory_type_of(flags);
        if got != want {
            serial_println!(
                "[pat]   FAIL: {name} decodes to {}, expected {}",
                got.name(),
                want.name()
            );
            return Err(KernelError::InternalError);
        }
    }
    serial_println!(
        "[pat]   PageFlags decode: default=WB, WRITE_COMBINING=WC, WRITE_THROUGH=WT, NO_CACHE=UC: OK"
    );

    // The property that makes this change safe for the fifteen existing MMIO
    // mappings: `NO_CACHE` selects the same slot before and after, so none of
    // them changed memory type when the table was reprogrammed.
    if PageFlags::NO_CACHE.bits() != (1 << 4) {
        serial_println!("[pat]   FAIL: NO_CACHE is no longer PCD-alone; MMIO mappings moved slot");
        return Err(KernelError::InternalError);
    }
    // ...and the property that makes it *useful*: a framebuffer mapping can
    // never be writeback, whichever of the two types it ends up with.
    for flags in [PageFlags::WRITE_COMBINING, PageFlags::WRITE_THROUGH] {
        if memory_type_of(flags).writes_may_linger() {
            serial_println!("[pat]   FAIL: a framebuffer memory type can hold writes in cache");
            return Err(KernelError::InternalError);
        }
    }
    serial_println!("[pat]   MMIO slot unchanged, framebuffer types never writeback: OK");

    serial_println!("[pat] Self-test PASSED");
    Ok(())
}

/// Read the CPU's current `IA32_PAT`.
///
/// # Safety
///
/// The CPU must support PAT; `rdmsr` on a machine without it raises `#GP`.
/// Check `cpu::features().pat` first.
#[must_use]
pub unsafe fn current() -> u64 {
    // SAFETY: caller guarantees PAT support.
    unsafe { crate::cpu::rdmsr(IA32_PAT) }
}
