//! CPUID-gated code patching ("alternatives").
//!
//! Some instructions are only legal on CPUs that advertise the corresponding
//! feature — `clac`/`stac` raise #UD without CPUID.SMAP, for instance. When such
//! an instruction belongs on a hot path (every interrupt, every syscall), the
//! usual workarounds are all bad:
//!
//! - a runtime `if` costs a load, a test and a predictable-but-real branch on
//!   the hottest path in the kernel, forever;
//! - an unconditional instruction faults on older CPUs;
//! - doing without it gives up the protection entirely.
//!
//! The standard answer, which this module implements, is to *rewrite the code
//! once at boot*. Each site reserves a few bytes holding a safe default (almost
//! always a NOP). If CPUID says the feature is present, the patcher overwrites
//! those bytes with the real instruction. Afterwards the fast path is exactly
//! the instruction we wanted, with no branch and no test.
//!
//! Modelled on Linux's `arch/x86/kernel/alternative.c` and the `ALTERNATIVE`
//! macro in `arch/x86/include/asm/alternative.h`, reduced to what we need: a
//! single unconditional replacement per site, no nested or alternative-2 forms,
//! and no runtime re-patching.
//!
//! # Declaring a site
//!
//! From `global_asm!` (see the ISR stubs in [`crate::idt`]), wrap the default
//! bytes in [`alternative_site!`]:
//!
//! ```ignore
//! global_asm!(
//!     alternative_site!("nop3", "clac", Feature::Smap),
//! );
//! ```
//!
//! # Getting write access to `.text`
//!
//! `.text` is *never* writable through its own mapping. `linker.ld` gives it a
//! `PT_LOAD` with `FLAGS(R|X)` and Limine honours that, so the kernel's code is
//! read-only from the first instruction — well before
//! [`crate::mm::protect::harden_kernel_sections`] gets involved. Writing to
//! `entry.site` directly therefore takes a #PF with `error=0x3` (present +
//! write), which is exactly what an earlier revision of this module did.
//!
//! There are two ways out, and we take the second:
//!
//! - **Clear `CR0.WP`** for the duration. Cheap, but it disables write
//!   protection *globally* for this CPU, so any concurrent bug anywhere gets a
//!   free pass to write read-only kernel memory, and the window is unwinding-
//!   and interrupt-hostile.
//! - **Write through a different, writable mapping of the same physical
//!   pages.** Limine's HHDM already maps all of physical memory read-write, so
//!   the alias exists for free. This is Linux's `text_poke` strategy
//!   (`arch/x86/mm/init.c` / `alternative.c`), minus the temporary-mm dance we
//!   do not need with one CPU running.
//!
//! Translating a `.text` virtual address to its HHDM alias needs the kernel's
//! physical load address, which the bootloader supplies directly via
//! [`crate::boot::executable_address`] — no page-table walk, so this works at a
//! point in boot before `mm::page_table` is even initialized.
//!
//! Note that the executable mapping is never made writable, not even
//! transiently: W^X holds throughout.
//!
//! # Why the SMP rules are satisfied
//!
//! The SDM's cross-modifying-code rules require the other CPUs to be stopped.
//! We sidestep that by *when* we run: [`apply`] is called early on the BSP, long
//! before `smp::init()` starts any AP, so at patch time exactly one CPU is
//! executing. [`apply`] asserts this rather than trusting the call site.
//!
//! # References
//!
//! - Linux: `arch/x86/kernel/alternative.c` (`apply_alternatives`, `text_poke`)
//! - Intel SDM Vol. 3A §8.1.3 "Handling Self- and Cross-Modifying Code"

use crate::serial_println;
use core::sync::atomic::{AtomicBool, Ordering};

// ---------------------------------------------------------------------------
// Feature identifiers
// ---------------------------------------------------------------------------

/// A CPU feature a patch site can be gated on.
///
/// The discriminants are baked into `.altinstructions` records by the assembler,
/// so they are part of an on-disk format in all but name: **never renumber an
/// existing variant**, only append.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum Feature {
    /// CPUID.(EAX=7,ECX=0):EBX.SMAP\[20] — enables `clac`/`stac`.
    Smap = 1,
}

impl Feature {
    /// Decode a discriminant read back out of an `.altinstructions` record.
    const fn from_u16(v: u16) -> Option<Self> {
        match v {
            1 => Some(Self::Smap),
            _ => None,
        }
    }

    /// Whether this CPU advertises the feature.
    fn is_present(self) -> bool {
        let Some(f) = crate::cpu::features() else {
            // Feature detection has not run yet.  Patching nothing is always
            // safe (every site keeps its default), so report absent rather than
            // guessing — `apply()` separately warns about this case.
            return false;
        };
        match self {
            Self::Smap => f.smap,
        }
    }
}

// ---------------------------------------------------------------------------
// The on-disk record
// ---------------------------------------------------------------------------

/// One patch record, as emitted into `.altinstructions` by [`alternative_site!`].
///
/// `repr(C)` and the field order are load-bearing: the assembler writes these
/// bytes directly (`.quad`/`.word`/`.byte`), so the layout here must match the
/// macro exactly.  Size is asserted below.
#[repr(C)]
struct AltEntry {
    /// Virtual address of the patch site in `.text`.
    site: u64,
    /// Virtual address of the replacement bytes in `.altinstr_replacement`.
    repl: u64,
    /// Which [`Feature`] gates this site.
    feature: u16,
    /// Bytes reserved at the site.  The replacement must fit within this.
    site_len: u8,
    /// Bytes of replacement to copy.
    repl_len: u8,
    _pad: [u8; 4],
}

const _: () = assert!(core::mem::size_of::<AltEntry>() == 24);
const _: () = assert!(core::mem::align_of::<AltEntry>() == 8);

/// Size of one `.altinstructions` record.
const ENTRY_SIZE: usize = core::mem::size_of::<AltEntry>();

/// How many whole records fit in `bytes`, or `None` if `bytes` is not an exact
/// multiple of [`ENTRY_SIZE`] — i.e. if the section is malformed.
///
/// The division is written in checked form purely because clippy cannot see
/// that `size_of::<AltEntry>()` is non-zero; the `None` arm it forces is folded
/// into the malformed-section case, which is where a caller would report it
/// anyway.
fn record_count(bytes: usize) -> Option<usize> {
    let count = bytes.checked_div(ENTRY_SIZE)?;
    match bytes.checked_rem(ENTRY_SIZE)? {
        0 => Some(count),
        _ => None,
    }
}

/// Ties the numeric discriminants written into `.altinstructions` by
/// `alternative_site!` back to the [`Feature`] enum.  Renumbering a variant
/// without updating every call site would otherwise leave records naming a
/// feature nobody recognises — or worse, the *wrong* one, gating `clac` on
/// something unrelated.
mod feature_discriminants_match_asm {
    use super::Feature;
    const _: () = assert!(Feature::Smap as u16 == 1);
}

unsafe extern "C" {
    static __altinstructions_start: u8;
    static __altinstructions_end: u8;
    static __text_start: u8;
    static __text_end: u8;
}

/// Set on entry to [`apply`], purely as the idempotency guard.
static STARTED: AtomicBool = AtomicBool::new(false);

/// Set once [`apply`] has run, so the self-test can tell "not patched because
/// the CPU lacks the feature" from "the patcher never ran at all".
static APPLIED: AtomicBool = AtomicBool::new(false);

/// Set only if [`apply`] reached the end having patched *every* site whose
/// feature this CPU has — no malformed table, no unreachable HHDM alias, no
/// per-site error.  See [`all_supported_sites_patched`].
static CLEAN: AtomicBool = AtomicBool::new(false);

/// Whether [`apply`] has run.
#[must_use]
pub fn has_run() -> bool {
    APPLIED.load(Ordering::Acquire)
}

/// Whether every patch site gated on a feature this CPU *has* was successfully
/// rewritten.
///
/// This, not [`has_run`], is what a caller must consult before relying on a
/// patched-in instruction being there.  The two differ precisely in the cases
/// that matter: [`apply`] can run to completion and still leave sites at their
/// defaults (a bad linker script, a missing HHDM, a corrupt record), and a
/// caller that only checked "did the patcher run?" would then enable a
/// protection whose enforcement never got installed.  `crate::smep_smap` gates
/// SMAP on this for exactly that reason — without the `clac` in the ISR stubs,
/// SMAP is disabled by any ring-3 process that sets `EFLAGS.AC`.
#[must_use]
pub fn all_supported_sites_patched() -> bool {
    CLEAN.load(Ordering::Acquire)
}

// ---------------------------------------------------------------------------
// Declaring a site
// ---------------------------------------------------------------------------

/// Emit a patch site: `$default` normally, `$replacement` if `$feature` is present.
///
/// Expands to a string suitable for `global_asm!`.  Both operands must be
/// assembler source; the replacement must assemble to no more bytes than the
/// default, which the macro checks at assembly time (see `.if` below) so an
/// oversized replacement is a build error rather than a corrupted `.text`.
///
/// Numeric local labels (`661:`…`664:`) are used rather than named ones so the
/// macro can be expanded many times in one translation unit without collisions —
/// the same trick Linux's `ALTERNATIVE` uses.
///
/// `$feature` is the *numeric* discriminant of a [`Feature`], not the Rust path:
/// the assembler cannot see Rust items, so the value is stringified into a
/// `.word`.  The `FEATURE_DISCRIMINANTS_MATCH_ASM` assertions below tie those
/// numbers back to the enum so the two cannot drift.
#[macro_export]
macro_rules! alternative_site {
    ($default:expr, $replacement:expr, $feature:expr) => {
        concat!(
            // --- the patch site itself, in .text ---
            "661:\n",
            $default,
            "\n662:\n",
            // --- the replacement bytes, parked in .rodata ---
            ".pushsection .altinstr_replacement, \"a\"\n",
            "663:\n",
            $replacement,
            "\n664:\n",
            ".popsection\n",
            // --- the record tying them together ---
            ".pushsection .altinstructions, \"a\"\n",
            ".balign 8\n",
            ".quad 661b\n",              // site
            ".quad 663b\n",              // replacement
            ".word ", stringify!($feature), "\n", // feature discriminant
            ".byte 662b-661b\n",         // site_len
            ".byte 664b-663b\n",         // repl_len
            ".zero 4\n",                 // _pad
            ".popsection\n",
            // A replacement longer than the site would run off the end of the
            // reserved bytes and shred the following instruction.  Catch it at
            // assembly time; there is no sane way to recover at runtime.
            ".if (664b-663b) > (662b-661b)\n",
            ".error \"alternative_site!: replacement is longer than the site\"\n",
            ".endif\n",
        )
    };
}

// ---------------------------------------------------------------------------
// Applying the patches
// ---------------------------------------------------------------------------

/// Single-byte NOP (`0x90`), used to pad a site when the replacement is shorter
/// than the space reserved for it.
const NOP: u8 = 0x90;

/// Where a `.text` address can be written: the kernel image's HHDM alias.
///
/// `.text` is mapped read-execute, so patching goes through the read-write
/// mapping of the same physical pages that the HHDM already provides.  Limine
/// loads the kernel image contiguously, so one offset covers the whole image:
/// `phys = virt - virtual_base + physical_base`, and `alias = phys + hhdm`.
#[derive(Clone, Copy)]
struct TextAlias {
    /// Added to a kernel virtual address to get its writable alias.
    delta: u64,
}

impl TextAlias {
    /// Derive the alias offset from the bootloader's own answers.
    ///
    /// Returns `None` (with a logged reason) if either Limine response is
    /// missing, in which case no patching can be done safely.
    fn derive() -> Option<Self> {
        // SAFETY: called from `apply()` at or after kernel entry, so Limine has
        // long since populated the request statics.
        let Some(hhdm) = (unsafe { crate::boot::hhdm_offset_early() }) else {
            serial_println!(
                "[alt] ERROR: no HHDM offset from the bootloader — .text has no writable \
                 alias, so no site can be patched"
            );
            return None;
        };
        let Some((phys_base, virt_base)) = crate::boot::executable_address() else {
            serial_println!(
                "[alt] ERROR: bootloader did not answer the executable-address request — \
                 cannot locate .text's physical pages, so no site can be patched"
            );
            return None;
        };

        // virt -> alias in one step: (virt - virt_base) + phys_base + hhdm.
        // Wrapping is correct rather than merely tolerated: virt_base is a
        // higher-half address (~0xFFFF_FFFF_8000_0000) and phys_base is small,
        // so `delta` is a large negative number in two's complement and every
        // use of it wraps back down into the HHDM.
        let delta = phys_base
            .wrapping_add(hhdm)
            .wrapping_sub(virt_base);
        Some(Self { delta })
    }

    /// Writable alias of a kernel virtual address.
    fn writable(self, virt: u64) -> *mut u8 {
        virt.wrapping_add(self.delta) as *mut u8
    }
}

/// Rewrite every `.altinstructions` site whose feature this CPU has.
///
/// Must be called on the BSP, after `cpu::detect_features()` (the gate is
/// CPUID-derived) and before `smp::init()`, so that no other CPU can be
/// executing the bytes being rewritten.  The CPU count is checked.
///
/// Patching writes through the kernel image's HHDM alias, so it does *not*
/// require `.text` to be writable — it never is — and imposes no ordering
/// constraint relative to `mm::protect::harden_kernel_sections()`.
///
/// Idempotent: a second call is a no-op.
pub fn apply() {
    if STARTED.swap(true, Ordering::AcqRel) {
        return;
    }
    APPLIED.store(true, Ordering::Release);

    // Cross-modifying code needs the other CPUs stopped (SDM Vol. 3A §8.1.3).
    // We rely on being early enough that none are running yet; assert it rather
    // than assume, because a future reordering of boot would otherwise turn this
    // into an extremely rare and undebuggable corruption.
    assert!(
        crate::smp::cpu_count() <= 1,
        "alternatives::apply() ran with {} CPUs online — patching .text while \
         another CPU may be executing it is cross-modifying code and requires \
         stopping them first",
        crate::smp::cpu_count()
    );

    if crate::cpu::features().is_none() {
        serial_println!(
            "[alt] WARNING: CPU features not detected yet — every site keeps its \
             default and no feature-gated instruction will be enabled"
        );
    }

    let Some(alias) = TextAlias::derive() else {
        return;
    };

    let start = core::ptr::addr_of!(__altinstructions_start) as usize;
    let end = core::ptr::addr_of!(__altinstructions_end) as usize;
    let text_start = core::ptr::addr_of!(__text_start) as u64;
    let text_end = core::ptr::addr_of!(__text_end) as u64;

    let Some(bytes) = end.checked_sub(start) else {
        serial_println!("[alt] ERROR: __altinstructions_end < start — bad linker script");
        return;
    };
    let Some(count) = record_count(bytes) else {
        serial_println!(
            "[alt] ERROR: .altinstructions is {bytes} bytes, not a multiple of {ENTRY_SIZE} — \
             refusing to patch from a malformed table"
        );
        return;
    };

    let mut patched = 0usize;
    let mut skipped = 0usize;
    let mut errors = 0usize;

    for i in 0..count {
        // SAFETY: `start` is the linker-provided base of `.altinstructions` and
        // `i < count` where count is derived from the section's own length, so
        // the computed pointer is inside the section.  The section is 8-byte
        // aligned by the linker script and every record is 8-byte aligned by
        // `.balign 8` in the macro, matching AltEntry's alignment.  The bytes
        // were written by the assembler in exactly this layout.
        let entry: &AltEntry = unsafe {
            let p = (start as *const AltEntry).add(i);
            &*p
        };

        let Some(feature) = Feature::from_u16(entry.feature) else {
            serial_println!(
                "[alt] ERROR: site {:#x} names unknown feature {} — skipping",
                entry.site,
                entry.feature
            );
            errors = errors.saturating_add(1);
            continue;
        };

        // Validate the site lies within .text before writing to it.  A record
        // pointing anywhere else means the table is corrupt, and a stray write
        // guided by it could land in arbitrary kernel memory.
        let site_end = entry.site.saturating_add(u64::from(entry.site_len));
        if entry.site < text_start || site_end > text_end {
            serial_println!(
                "[alt] ERROR: site {:#x}..{:#x} is outside .text ({text_start:#x}..{text_end:#x}) \
                 — refusing to patch",
                entry.site,
                site_end
            );
            errors = errors.saturating_add(1);
            continue;
        }

        if entry.repl_len > entry.site_len {
            // The assembler-time `.if` should have caught this already; treat a
            // survivor as table corruption rather than clobbering past the site.
            serial_println!(
                "[alt] ERROR: site {:#x} replacement {} > reserved {} — refusing to patch",
                entry.site,
                entry.repl_len,
                entry.site_len
            );
            errors = errors.saturating_add(1);
            continue;
        }

        if !feature.is_present() {
            skipped = skipped.saturating_add(1);
            continue;
        }

        // SAFETY: `dst` is the HHDM alias of `entry.site`, which was checked
        // above to lie wholly within .text; the HHDM maps all of physical
        // memory read-write, so `site_len` bytes there are writable and belong
        // to no one else.  No other CPU is running (asserted above).  `repl`
        // points into .altinstr_replacement with `repl_len` valid bytes,
        // emitted by the assembler alongside this record.  Source and
        // destination cannot overlap: the source is in .rodata and the
        // destination is in the HHDM window, which is disjoint from the kernel
        // image's own higher-half mapping.  Any tail beyond `repl_len` is
        // filled with NOPs so the site remains a valid instruction stream of
        // exactly `site_len` bytes.
        unsafe {
            let dst = alias.writable(entry.site);
            let src = entry.repl as *const u8;
            core::ptr::copy_nonoverlapping(src, dst, entry.repl_len as usize);
            let tail = (entry.site_len as usize).saturating_sub(entry.repl_len as usize);
            if tail > 0 {
                core::ptr::write_bytes(dst.add(entry.repl_len as usize), NOP, tail);
            }
        }
        patched = patched.saturating_add(1);
    }

    if patched > 0 {
        // Serialize so this CPU cannot execute a stale prefetch of any byte we
        // just rewrote (SDM Vol. 3A §8.1.3: a serializing instruction after the
        // store is required even for self-modifying code on one CPU).  CPUID is
        // the canonical choice and is unconditionally available.
        //
        // Writing through the HHDM alias rather than the executable mapping
        // needs no extra care on x86: the caches and the instruction fetch unit
        // are coherent over *physical* addresses, so the store is visible to a
        // fetch through the other mapping without any explicit flush.
        crate::cpu::serialize();
    }

    // Only "clean" if every site that should have been patched was.  `errors`
    // counts malformed or out-of-bounds records; sites skipped merely because
    // this CPU lacks the feature are the correct outcome and do not count.
    if errors == 0 {
        CLEAN.store(true, Ordering::Release);
    }

    serial_println!(
        "[alt] {count} site(s): {patched} patched, {skipped} left at default, {errors} error(s)"
    );
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Boot self-test: verify the table is well-formed and was consistently applied.
///
/// This cannot simply assert "everything got patched" — on a CPU without the
/// feature, leaving the default in place *is* the correct outcome.  What it
/// checks is that each site's state agrees with CPUID, which is the property
/// that actually matters and is wrong in both failure directions.
pub fn self_test() {
    serial_println!("[alt] Running alternatives self-test...");

    assert!(
        has_run(),
        "alt: self-test ran before apply() — every feature-gated instruction is \
         still its default, so any protection relying on one is silently off"
    );

    let start = core::ptr::addr_of!(__altinstructions_start) as usize;
    let end = core::ptr::addr_of!(__altinstructions_end) as usize;
    let bytes = end.saturating_sub(start);
    let records = record_count(bytes);
    assert!(
        records.is_some(),
        "alt: .altinstructions is {bytes} bytes, not a whole number of records"
    );
    let count = records.unwrap_or(0);
    serial_println!("[alt]   {count} patch site(s) in .altinstructions");

    let mut mismatches = 0usize;
    for i in 0..count {
        // SAFETY: as in `apply()` — bounded by the section's own length, and the
        // records are assembler-emitted with matching layout and alignment.
        let entry: &AltEntry = unsafe { &*(start as *const AltEntry).add(i) };

        let Some(feature) = Feature::from_u16(entry.feature) else {
            mismatches = mismatches.saturating_add(1);
            serial_println!("[alt]   site {:#x}: unknown feature id", entry.site);
            continue;
        };

        // SAFETY: `site`/`repl` were validated by `apply()` to lie in .text and
        // .altinstr_replacement respectively, with `repl_len` bytes readable at
        // each.  We only read.
        let (site_bytes, repl_bytes) = unsafe {
            (
                core::slice::from_raw_parts(entry.site as *const u8, entry.repl_len as usize),
                core::slice::from_raw_parts(entry.repl as *const u8, entry.repl_len as usize),
            )
        };

        let is_patched = site_bytes == repl_bytes;
        if is_patched != feature.is_present() {
            mismatches = mismatches.saturating_add(1);
            serial_println!(
                "[alt]   site {:#x}: feature {feature:?} present={} but patched={}",
                entry.site,
                feature.is_present(),
                is_patched
            );
        }
    }

    assert_eq!(
        mismatches, 0,
        "alt: {mismatches} patch site(s) disagree with CPUID — a feature-gated \
         instruction is either missing on a CPU that supports it, or present on \
         one that does not (which would #UD)"
    );

    serial_println!("[alt] Alternatives self-test PASSED");
}
