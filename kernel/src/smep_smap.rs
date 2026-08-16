//! SMEP/SMAP — Supervisor Mode Execution/Access Prevention.
//!
//! Hardware protection against the kernel accidentally (or maliciously via
//! exploit) accessing user-space memory:
//!
//! ## SMEP (Supervisor Mode Execution Prevention)
//!
//! When CR4.SMEP is set, any attempt by the kernel (CPL < 3) to fetch
//! and execute instructions from a user-mode page (U/S bit set in PTE)
//! triggers a #PF with RSVD bit set.  This defeats "ret2usr" exploits
//! where an attacker gets the kernel to jump to shellcode mapped in the
//! process's user address space.
//!
//! ## SMAP (Supervisor Mode Access Prevention)
//!
//! When CR4.SMAP is set, any attempt by the kernel to read or write a
//! user-mode page triggers a #PF — UNLESS the EFLAGS.AC (Alignment Check)
//! flag is set.  The kernel uses `STAC` (Set AC flag) before intentionally
//! accessing user memory and `CLAC` (Clear AC flag) immediately after.
//! This prevents:
//! - Confused-deputy bugs where kernel code accidentally dereferences a
//!   user-supplied pointer without validation
//! - Exploits that redirect kernel reads/writes to user-mapped pages
//!
//! ## Usage Pattern
//!
//! Syscall handlers do **not** call `stac`/`clac`/[`with_user_access`]
//! themselves.  Every kernel→user access goes through `mm::user`, which already
//! brackets each copy:
//!
//! ```ignore
//! let mut buf = alloc::vec![0u8; len];
//! crate::mm::user::copy_from_user(user_ptr, &mut buf)?;   // STAC … CLAC inside
//! pipe::write(&buf)?;                                      // kernel memory only
//! ```
//!
//! The rule that makes this work is that the AC window must contain exactly one
//! non-blocking copy.  A handler that opened a window around a *blocking* callee
//! would leave AC = 1 in the task's saved RFLAGS across the reschedule, silently
//! disabling SMAP for that task and for the scheduler itself — which is why
//! there is no "wrap the existing raw-pointer code" shortcut and why
//! [`with_user_access`] has no callers outside this module's own self-test.
//!
//! The single exception is a futex word, where the atomicity of the RMW against
//! a concurrent userspace CAS *is* the primitive and a copy-in/copy-out bounce
//! would reintroduce the lost update.  `mm::user::user_atomic_*` brackets one
//! atomic instruction per call, with retry loops outside the window — the same
//! shape as Linux's `arch/x86/include/asm/futex.h`.
//!
//! ## Performance
//!
//! - SMEP: Zero overhead once enabled (purely hardware-enforced)
//! - SMAP: ~2 cycles for STAC, ~2 cycles for CLAC. Negligible.
//! - Both features have been in Intel CPUs since Haswell (2013) and
//!   AMD since Zen (2017).  Virtually all x86_64 systems support them.
//!
//! ## References
//!
//! - Intel SDM Vol. 3A §4.6 "Access Rights"
//! - Linux: arch/x86/include/asm/smap.h (stac/clac)
//! - Linux: arch/x86/mm/fault.c (SMAP violation detection)

use crate::serial_println;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

// ---------------------------------------------------------------------------
// CR4 bits
// ---------------------------------------------------------------------------

/// CR4.SMEP — Supervisor Mode Execution Prevention (bit 20).
const CR4_SMEP: u64 = 1 << 20;
/// CR4.SMAP — Supervisor Mode Access Prevention (bit 21).
const CR4_SMAP: u64 = 1 << 21;
/// CR4.UMIP — User-Mode Instruction Prevention (bit 11).
const CR4_UMIP: u64 = 1 << 11;

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

/// Whether SMEP is currently enabled.
static SMEP_ENABLED: AtomicBool = AtomicBool::new(false);
/// Whether SMAP is currently enabled.
static SMAP_ENABLED: AtomicBool = AtomicBool::new(false);
/// Whether UMIP is currently enabled.
static UMIP_ENABLED: AtomicBool = AtomicBool::new(false);
/// Whether SMAP hardware is present (CPUID support for STAC/CLAC instructions).
/// STAC/CLAC #UD if the CPU doesn't support SMAP, so we must guard them.
static HW_SMAP: AtomicBool = AtomicBool::new(false);
/// Count of intentional user-access windows opened (STAC/CLAC pairs).
static USER_ACCESS_COUNT: AtomicU64 = AtomicU64::new(0);

/// Whether every kernel entry path clears `EFLAGS.AC`.
///
/// **CR4.SMAP must not be set while this is `false`** — `smap_enable_blocker()`
/// enforces that.
///
/// AC is the SMAP override: when AC = 1, supervisor accesses to user pages are
/// permitted and SMAP checks nothing.  A ring-3 process can set AC freely with
/// `popfq` (unlike IF or IOPL, AC is not privileged), so AC is *attacker-
/// controlled* on entry unless the entry path clears it:
///
/// - **SYSCALL** — covered.  `syscall::entry::init()` masks AC (bit 18) in
///   `IA32_FMASK`, so the CPU clears it as part of the instruction.
/// - **IDT gates (every interrupt, every exception)** — covered as of the
///   alternatives framework.  An interrupt gate loads the new RFLAGS clearing
///   only TF, NT, RF and VM (Intel SDM Vol. 3A §6.12.1); AC is inherited
///   verbatim, exactly as DF is.  So a process that set AC and then waited for a
///   timer tick used to enter every kernel handler with SMAP disabled.  Each ISR
///   stub now begins with a patch site that [`crate::alternatives`] rewrites
///   from a 3-byte NOP to `clac` at boot.
///
/// This is the same bug class as the missing `cld` fixed in `idt.rs` — inherited
/// ring-3 flag state — but it fails *open* and *silently*: nothing crashes, no
/// test goes red, SMAP is simply not enforced.  Hence the gate, and hence
/// `idt::ac_on_entry_self_test()` checking this answer against what a real IDT
/// gate does rather than trusting it.
///
/// Note the conjunction: the `clac` is patched in **only when CPUID reports
/// SMAP**, since `clac` #UDs otherwise.  On a CPU without SMAP the stubs keep
/// their NOP and AC is still inherited — harmless, because SMAP cannot be
/// enabled there either, but it means this must not report `true` merely because
/// the patcher ran.
///
/// It asks [`crate::alternatives::all_supported_sites_patched`] rather than
/// `has_run()` for the same reason: the patcher can run to completion and still
/// have installed nothing (a malformed table, no HHDM alias to write through),
/// and that outcome must read as "AC is not cleared", not as "the patcher ran,
/// so we're fine".
fn entry_paths_clear_ac_impl() -> bool {
    crate::alternatives::all_supported_sites_patched()
        && crate::cpu::features().is_some_and(|f| f.smap)
}

/// Whether every kernel path that touches user memory is wrapped in STAC/CLAC.
///
/// **CR4.SMAP must not be set while this is `false`** — `smap_enable_blocker()`
/// enforces that.  Unlike [`ENTRY_PATHS_CLEAR_AC`], this prerequisite fails
/// loudly (a #PF on the first unannotated user access), but gating it keeps
/// both preconditions in one place.
///
/// Every kernel→user access now goes through `mm::user`, which brackets each
/// copy in `stac()`/`clac()`.  Getting here was a refactor, not an audit: ~100
/// syscall handlers used to validate a user pointer and then build a slice
/// *over the user virtual address* and hand it to kernel code
/// (`core::slice::from_raw_parts(ptr, len)` straight into `pipe::read`, and so
/// on).  Each of those is now a bounce through a kernel buffer —
/// `copy_from_user` / `copy_to_user` / `read_user_value` / `write_user_value` /
/// `read_user_items` / `write_user_items`.
///
/// Wrapping the old shape in `stac()`/`clac()` would *not* have been the fix:
/// several of the callees block, and an open STAC window across a reschedule
/// leaves AC = 1 in the task's saved RFLAGS, disabling SMAP for that task and
/// for the scheduler itself.  The window has to stay inside a single
/// non-blocking copy.  (The raw-slice shape was also a use-after-free in its own
/// right, quite apart from SMAP: a peer thread can `munmap` the range while the
/// caller sleeps.)  See `D-SYSCALL-HANDLERS-HAND-RAW-USER-SLICES-TO-KERNEL-CODE`
/// in `known-issues.md`.
///
/// The one place a bounce is *wrong* is a futex word, where the primitive is the
/// atomicity of the RMW against a concurrent userspace CAS — copy-in / modify /
/// copy-out reintroduces the lost update.  Those go through
/// `mm::user::user_atomic_*`, which brackets exactly one atomic instruction, and
/// run their retry loops outside the window (as Linux does in
/// `arch/x86/include/asm/futex.h`).
///
/// Keeping this a `const` rather than deleting it: it is the single documented
/// place to flip SMAP back off if a missed access path turns up, and
/// `smap_enable_blocker()` still reads it so the reason lands on the serial log.
const USER_ACCESSES_ANNOTATED: bool = true;

/// Whether the kernel believes every entry path clears `EFLAGS.AC`.
///
/// Exposed so `idt::ac_on_entry_self_test()` can hold this claim against what an
/// actual IDT gate does — see [`ENTRY_PATHS_CLEAR_AC`] for why a wrong answer
/// here would otherwise fail silently.
#[must_use]
pub fn entry_paths_clear_ac() -> bool {
    entry_paths_clear_ac_impl()
}

/// Return the reason CR4.SMAP cannot be enabled yet, or `None` if it can.
///
/// Enabling SMAP with either prerequisite unmet is worse than leaving it off:
/// an unmet [`ENTRY_PATHS_CLEAR_AC`] yields a protection that silently does
/// nothing while reporting itself as ACTIVE, which is precisely the "a defence
/// that looks sufficient is not" failure mode recorded in design-decisions §118.
fn smap_enable_blocker() -> Option<&'static str> {
    if !entry_paths_clear_ac_impl() {
        // Keep this string specific: it is what a future reader sees on the
        // serial log when they wonder why SMAP is off.
        return Some(
            "IDT entry stubs do not clear EFLAGS.AC — alternatives::apply() has not patched in `clac`",
        );
    }
    if !USER_ACCESSES_ANNOTATED {
        return Some("user-access paths not fully STAC/CLAC-annotated");
    }
    None
}

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Detect and enable SMEP/SMAP on the current CPU.
///
/// Called during early boot after `cpu::detect_features()`.  Each AP also
/// calls this during SMP init (CR4 is per-CPU, so each CPU must set its own).
///
/// It is safe to call multiple times (idempotent).
pub fn init() {
    let Some(features) = crate::cpu::features() else {
        serial_println!("[smep_smap] CPU features not detected — skipping");
        return;
    };

    let cr4 = read_cr4();
    let mut new_cr4 = cr4;

    // Enable SMEP if supported.
    if features.smep {
        if cr4 & CR4_SMEP == 0 {
            new_cr4 |= CR4_SMEP;
            serial_println!("[smep_smap] Enabling SMEP (kernel exec of user pages blocked)");
        }
        SMEP_ENABLED.store(true, Ordering::Release);
    } else {
        serial_println!("[smep_smap] SMEP not supported by CPU");
    }

    // SMAP has TWO independent prerequisites, and both must be met before
    // setting CR4.SMAP.  Both are satisfied now, but the gate stays so that
    // regressing either one turns SMAP off with a reason rather than turning
    // the boot into a #PF storm:
    //
    // 1. Every kernel→user memory access path must use STAC/CLAC — see
    //    `USER_ACCESSES_ANNOTATED` below.  *This one fails loudly*: a #PF the
    //    first time a syscall touches a user buffer through a raw pointer.
    //
    // 2. Every kernel entry path must clear EFLAGS.AC — see
    //    `ENTRY_PATHS_CLEAR_AC` below.  *This one fails silently*, which is
    //    why it is asserted rather than left to a comment.
    if features.smap {
        // Record that the hardware supports STAC/CLAC instructions (they
        // #UD without CPUID SMAP support).  This gates their execution in
        // stac()/clac()/with_user_access().
        HW_SMAP.store(true, Ordering::Release);
        if let Some(blocker) = smap_enable_blocker() {
            serial_println!("[smep_smap] SMAP supported (enablement deferred — {blocker})");
            // Do NOT enable — leave SMAP_ENABLED = false.
        } else {
            new_cr4 |= CR4_SMAP;
            SMAP_ENABLED.store(true, Ordering::Release);
            serial_println!("[smep_smap] Enabling SMAP (kernel read/write of user pages blocked)");
        }
    } else {
        serial_println!("[smep_smap] SMAP not supported by CPU");
    }

    // Enable UMIP if supported.
    // UMIP blocks user-mode execution of SGDT, SIDT, SLDT, SMSW, STR.
    // These instructions leak kernel addresses (GDT/IDT base, LDT selector)
    // which could be used to bypass KASLR.  Safe to enable unconditionally
    // since user-mode code should never need these instructions.
    if features.umip {
        if cr4 & CR4_UMIP == 0 {
            new_cr4 |= CR4_UMIP;
            serial_println!("[smep_smap] Enabling UMIP (user SGDT/SIDT/SLDT/SMSW/STR blocked)");
        }
        UMIP_ENABLED.store(true, Ordering::Release);
    } else {
        serial_println!("[smep_smap] UMIP not supported by CPU");
    }

    // Apply CR4 changes if any bits were added.
    if new_cr4 != cr4 {
        // SAFETY: We've verified the CPU supports these features via CPUID.
        // Adding SMEP/SMAP bits to CR4 is safe as long as the kernel doesn't
        // intentionally execute user pages (it shouldn't!) and uses STAC/CLAC
        // when accessing user memory.
        unsafe {
            write_cr4(new_cr4);
        }
        serial_println!("[smep_smap] CR4 updated: {:#x} → {:#x}", cr4, new_cr4);
    }
}

/// Enable SMEP/SMAP on an Application Processor during SMP bootstrap.
///
/// Each CPU has its own CR4, so each AP must independently enable these bits.
pub fn init_ap() {
    let Some(features) = crate::cpu::features() else {
        return;
    };

    let cr4 = read_cr4();
    let mut new_cr4 = cr4;

    if features.smep && (cr4 & CR4_SMEP == 0) {
        new_cr4 |= CR4_SMEP;
    }
    // Use the same gate as `init()` rather than an independent decision here:
    // an AP running with a different CR4.SMAP than the BSP would make user
    // access succeed or fault depending on which CPU the syscall landed on.
    if features.smap && smap_enable_blocker().is_none() && (cr4 & CR4_SMAP == 0) {
        new_cr4 |= CR4_SMAP;
    }
    if features.umip && (cr4 & CR4_UMIP == 0) {
        new_cr4 |= CR4_UMIP;
    }

    if new_cr4 != cr4 {
        // SAFETY: Same as init() — CPU supports these features.
        unsafe {
            write_cr4(new_cr4);
        }
    }
}

// ---------------------------------------------------------------------------
// User memory access primitives (STAC/CLAC)
// ---------------------------------------------------------------------------

/// Temporarily allow kernel access to user-mode pages (clear SMAP enforcement).
///
/// Sets the AC flag in EFLAGS, which tells the CPU to permit supervisor-mode
/// accesses to user pages.  MUST be paired with [`clac()`] as soon as the
/// access is complete.
///
/// If SMAP is not enabled, this is a no-op (STAC is still safe to execute;
/// it just sets a flag that nothing checks).
///
/// # Safety
///
/// Caller must ensure that the user memory being accessed has been validated
/// (address range is in user space, pages are mapped with appropriate permissions).
/// The AC flag window must be as short as possible to minimize the attack surface.
#[inline(always)]
pub unsafe fn stac() {
    // STAC requires CPUID SMAP support — it #UDs on CPUs without SMAP.
    // When SMAP hardware is absent, the instruction is unnecessary (the
    // CPU already allows kernel access to user pages unconditionally).
    if !HW_SMAP.load(Ordering::Relaxed) {
        return;
    }
    // SAFETY: SMAP hardware is present (checked above).  STAC sets
    // EFLAGS.AC to suppress SMAP enforcement.
    unsafe {
        core::arch::asm!("stac", options(nomem, nostack, preserves_flags));
    }
    USER_ACCESS_COUNT.fetch_add(1, Ordering::Relaxed);
}

/// Re-enable SMAP enforcement after user memory access.
///
/// Clears the AC flag in EFLAGS.  After this, any kernel access to user
/// pages will fault.
///
/// # Safety
///
/// Must be called after a corresponding [`stac()`] call.  Forgetting CLAC
/// leaves the kernel vulnerable until the next context switch or IRET.
#[inline(always)]
pub unsafe fn clac() {
    // CLAC requires CPUID SMAP support — it #UDs on CPUs without SMAP.
    if !HW_SMAP.load(Ordering::Relaxed) {
        return;
    }
    // SAFETY: SMAP hardware is present (checked above).  CLAC clears
    // EFLAGS.AC to re-enable SMAP enforcement.
    unsafe {
        core::arch::asm!("clac", options(nomem, nostack, preserves_flags));
    }
}

/// Execute a closure with user memory access temporarily enabled.
///
/// This is the preferred way to access user memory from the kernel.
/// It ensures STAC/CLAC are properly paired and the window is minimal.
///
/// # Safety
///
/// - The closure must only access validated user memory
/// - The user pointer must have been range-checked before calling this
/// - The closure should be as short as possible (just the memory access)
///
/// # Example
///
/// ```ignore
/// let value = unsafe {
///     smep_smap::with_user_access(|| {
///         core::ptr::read(validated_user_ptr)
///     })
/// };
/// ```
#[inline(always)]
pub unsafe fn with_user_access<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    // SAFETY: Caller guarantees the closure only accesses validated user memory.
    unsafe {
        stac();
    }
    let result = f();
    // SAFETY: Paired with stac() above.
    unsafe {
        clac();
    }
    result
}

// ---------------------------------------------------------------------------
// Status and diagnostics
// ---------------------------------------------------------------------------

/// SMEP/SMAP/UMIP status for diagnostics.
#[derive(Debug, Clone)]
pub struct SmepSmapStatus {
    /// CPU supports SMEP.
    pub hw_smep: bool,
    /// CPU supports SMAP.
    pub hw_smap: bool,
    /// CPU supports UMIP.
    pub hw_umip: bool,
    /// SMEP currently enabled (CR4.SMEP set on BSP).
    pub smep_active: bool,
    /// SMAP currently enabled (CR4.SMAP set on BSP).
    pub smap_active: bool,
    /// UMIP currently enabled (CR4.UMIP set on BSP).
    pub umip_active: bool,
    /// Total user-access windows opened (STAC calls).
    pub user_access_count: u64,
    /// Current CR4 value (for diagnostics).
    pub cr4: u64,
}

/// Query current SMEP/SMAP/UMIP status.
pub fn status() -> SmepSmapStatus {
    let features = crate::cpu::features();
    let (hw_smep, hw_smap, hw_umip) = features
        .map(|f| (f.smep, f.smap, f.umip))
        .unwrap_or((false, false, false));

    SmepSmapStatus {
        hw_smep,
        hw_smap,
        hw_umip,
        smep_active: SMEP_ENABLED.load(Ordering::Relaxed),
        smap_active: SMAP_ENABLED.load(Ordering::Relaxed),
        umip_active: UMIP_ENABLED.load(Ordering::Relaxed),
        user_access_count: USER_ACCESS_COUNT.load(Ordering::Relaxed),
        cr4: read_cr4(),
    }
}

/// Whether SMEP is currently active.
#[allow(dead_code)]
pub fn smep_active() -> bool {
    SMEP_ENABLED.load(Ordering::Relaxed)
}

/// Whether SMAP is currently active.
#[allow(dead_code)]
pub fn smap_active() -> bool {
    SMAP_ENABLED.load(Ordering::Relaxed)
}

// ---------------------------------------------------------------------------
// Low-level helpers
// ---------------------------------------------------------------------------

/// Read CR4 register.
fn read_cr4() -> u64 {
    let val: u64;
    // SAFETY: Reading CR4 is always safe in ring 0.
    unsafe {
        core::arch::asm!("mov {}, cr4", out(reg) val, options(nomem, nostack));
    }
    val
}

/// Write CR4 register.
///
/// # Safety
///
/// Caller must ensure the new CR4 value is valid and doesn't disable
/// critical features without proper preparation.
unsafe fn write_cr4(val: u64) {
    // SAFETY: Caller guarantees the value is valid.
    unsafe {
        core::arch::asm!("mov cr4, {}", in(reg) val, options(nomem, nostack));
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for SMEP/SMAP detection and status.
pub fn self_test() {
    serial_println!("[smep_smap] Running self-test...");

    // Test 1: Status query works without panic.
    let s = status();
    serial_println!(
        "[smep_smap]   Status: SMEP hw={}, SMAP hw={}, UMIP hw={}",
        s.hw_smep,
        s.hw_smap,
        s.hw_umip
    );
    serial_println!(
        "[smep_smap]   Active: SMEP={}, SMAP={}, UMIP={}",
        s.smep_active,
        s.smap_active,
        s.umip_active
    );
    serial_println!("[smep_smap]   CR4={:#x}", s.cr4);

    // Test 2: If SMEP is supported, verify CR4.SMEP is set.
    if s.hw_smep {
        assert!(
            s.cr4 & CR4_SMEP != 0,
            "CR4.SMEP should be set when SMEP is supported"
        );
        assert!(s.smep_active, "SMEP should be marked active");
        serial_println!("[smep_smap]   SMEP enforcement: VERIFIED (CR4 bit set)");
    } else {
        assert_eq!(
            s.cr4 & CR4_SMEP,
            0,
            "CR4.SMEP should be clear without support"
        );
        serial_println!("[smep_smap]   SMEP: not available on this CPU");
    }

    // Test 3: SMAP.  Assert against `smap_enable_blocker()` rather than against
    // a flat "should be on" expectation, because the blocker is a real runtime
    // condition (`alternatives::apply()` may have patched nothing).  What must
    // hold either way is that CR4 and `SMAP_ENABLED` agree with the gate — a
    // CR4 bit set without the prerequisites, or the status flag claiming an
    // enforcement CR4 does not have, is the silent-failure mode this whole
    // module exists to prevent.
    if s.hw_smap {
        match smap_enable_blocker() {
            None => {
                assert!(
                    s.cr4 & CR4_SMAP != 0,
                    "CR4.SMAP should be set once both prerequisites are met"
                );
                assert!(s.smap_active, "SMAP should be marked active");
                serial_println!("[smep_smap]   SMAP enforcement: VERIFIED (CR4 bit set)");
            }
            Some(blocker) => {
                assert_eq!(
                    s.cr4 & CR4_SMAP,
                    0,
                    "CR4.SMAP should be clear while a prerequisite is unmet"
                );
                assert!(
                    !s.smap_active,
                    "SMAP should not be marked active while a prerequisite is unmet"
                );
                serial_println!("[smep_smap]   SMAP: supported but deferred — {blocker}");
            }
        }
    } else {
        assert_eq!(
            s.cr4 & CR4_SMAP,
            0,
            "CR4.SMAP should be clear without support"
        );
        assert!(
            !s.smap_active,
            "SMAP should not be marked active without support"
        );
        serial_println!("[smep_smap]   SMAP: not available on this CPU");
    }

    // Test 3b: If UMIP is supported, verify CR4.UMIP is set.
    if s.hw_umip {
        assert!(
            s.cr4 & CR4_UMIP != 0,
            "CR4.UMIP should be set when UMIP is supported"
        );
        assert!(s.umip_active, "UMIP should be marked active");
        serial_println!("[smep_smap]   UMIP enforcement: VERIFIED (CR4 bit set)");
    } else {
        serial_println!("[smep_smap]   UMIP: not available on this CPU");
    }

    // Test 4: STAC/CLAC don't fault — only when SMAP hardware is present.
    // STAC/CLAC require CPUID SMAP support; they #UD on CPUs without it.
    if s.hw_smap {
        unsafe {
            stac();
            clac();
        }
        serial_println!("[smep_smap]   STAC/CLAC pair: OK (no fault)");

        // Test 5: with_user_access closure executes and returns value.
        let result = unsafe { with_user_access(|| 42u64) };
        assert_eq!(result, 42);
        serial_println!("[smep_smap]   with_user_access: OK");

        // Test 6: Access count incremented correctly.
        let count_before = USER_ACCESS_COUNT.load(Ordering::Relaxed);
        unsafe {
            stac();
            clac();
        }
        let count_after = USER_ACCESS_COUNT.load(Ordering::Relaxed);
        assert_eq!(count_after, count_before.wrapping_add(1));
        serial_println!("[smep_smap]   Access counter: OK");
    } else {
        serial_println!(
            "[smep_smap]   STAC/CLAC: skipped (SMAP not available — instructions would #UD)"
        );
    }

    serial_println!("[smep_smap] Self-test PASSED");
}
