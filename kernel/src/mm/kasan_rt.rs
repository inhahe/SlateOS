//! KASAN runtime — the `__asan_*` callbacks LLVM's instrumentation calls.
//!
//! When the kernel is built with the compiler-KASAN profile
//! (`scripts/kasan-build.sh`, design-decisions.md §107 and §118), rustc/LLVM
//! rewrites every load and store into a call to one of the check entry points
//! below:
//!
//! ```text
//!     mov  %rdi, <the address about to be accessed>
//!     call __asan_load8_noabort            ; shadow_allows(), report if bad
//!     <the original access>
//! ```
//!
//! ## Why outlined and not inline
//!
//! LLVM's default is to emit the shadow compare *inline*
//! (`movzbl (shadow), %eax; test %al, %al; jne slow_path`), and the profile
//! turns that off with `-asan-instrumentation-with-call-threshold=0`. The
//! inline sequence dereferences `(addr >> 3) + KASAN_SHADOW_OFFSET`
//! unconditionally, which is a canonical address only when `addr` is a *kernel*
//! address — the shadow of a user pointer has non-sign-extended high bits, so
//! probing it is a #GP rather than a report. Kernel code dereferences user
//! pointers by design (the SEH context written onto the faulting thread's user
//! stack in `idt.rs`, the uaccess helpers in `mm::user`), so inline checks turn
//! every one of those sites into a panic that has nothing to do with the bug
//! being hunted. Outlined, the address is filtered by `mm::kasan::shadow_of`
//! before anything is dereferenced, and a user address simply passes.
//!
//! So this module supplies both halves: the check entry points, which do the
//! shadow lookup, and the report entry points, which are the cold slow path.
//!
//! ## Termination
//!
//! Because every checked access now lands here, nothing on the path from a
//! check entry point down through `kasan::shadow_allows` may itself perform an
//! instrumented access — it would call back in, without bound. Marking this
//! module `sanitize(address = "off")` does not establish that: generic `core`
//! functions monomorphise into this crate carrying the default (instrumented)
//! attribute, so a single `AtomicU64::load` inside the shadow lookup would be
//! enough. `mm::kasan::get_shadow` is written against raw `asm!` loads for
//! exactly this reason, and `scripts/kasan-check-preshadow.py --runtime` proves
//! it against the built binary rather than trusting review.
//!
//! The *report* path is deliberately not held to that rule — it formats and
//! backtraces, far too much code to keep raw. It does not need to be: a report
//! calls instrumented code, whose checks call `shadow_allows`, which is clean
//! and returns. That is one level of nesting, not a regress.
//!
//! ## Why `_noabort`
//!
//! We build with `-asan-recover=1`, which makes LLVM emit the `_noabort`
//! variants and, crucially, *fall through to perform the access* after the
//! report instead of following the call with `ud2`. A kernel that halts on the
//! first report tells you about exactly one bug per boot; the corruption hunt
//! this exists for (B-KNULLJUMP) needs to see the whole pattern of accesses
//! around a stomp. Linux's KASAN makes the same choice for the same reason.
//!
//! The abort-mode symbols are defined too, so that flipping `-asan-recover`
//! off for a bisect does not turn into a link error at the worst moment.
//!
//! ## Re-entrancy
//!
//! Every function here is compiled with sanitization *off*. If it were not,
//! the first report would instrument its own loads, whose checks could report,
//! which would instrument... — an infinite regress that manifests as a stack
//! overflow with no explanation. This is not an optimization; it is a
//! correctness requirement, and it is why the attribute is applied to every
//! item in the module rather than to a chosen few.
//!
//! ## What is (not) instrumented
//!
//! The profile passes `-asan-stack=0 -asan-globals=0`, so stack frames and
//! statics get no redzones. Accesses to them are still *checked* — they just
//! read shadow that is permanently zero, so they always pass. The reason is
//! that poisoning a stack redzone means LLVM emitting shadow **stores**, which
//! in turn means the shadow covering every kernel stack (and the boot stack,
//! and every AP stack) must be real writable memory rather than the shared
//! read-only zero page — a much larger, much more fragile early-boot
//! commitment for a class of bug we are not currently chasing. Heap poison,
//! which is what catches a wild write into a live or freed slab object, needs
//! none of that. See `mm::kasan` for the shadow itself.
//!
//! ## References
//!
//! - Linux `mm/kasan/generic.c` (`kasan_report`, the `__asan_*` entry points).
//! - LLVM `AddressSanitizer.cpp` — inline check codegen, `-asan-recover`.

// The whole module is FFI surface for the compiler: `no_mangle extern "C"`
// functions that are never called from Rust, and that are only referenced at
// all in the instrumented profile.
#![allow(dead_code)]

use core::sync::atomic::{AtomicU64, Ordering};

use crate::mm::kasan;
use crate::serial_println;

/// Reports emitted before we go quiet.
///
/// A KASAN hit on a hot path (a corrupted object that a scheduler loop touches
/// every tick) can produce millions of reports, which drowns the serial log
/// that is our only evidence channel and slows the kernel to a crawl — turning
/// a diagnosis into a hang. Bound it: the first `MAX_REPORTS` carry all the
/// information there is, and the counter itself records how much was elided.
const MAX_REPORTS: u64 = 64;

/// Reports emitted with a backtrace. Backtraces are the expensive part and the
/// first few are the ones that identify the culprit.
const MAX_BACKTRACES: u64 = 8;

/// Total reports the compiler-emitted checks have handed us since boot.
static REPORTS: AtomicU64 = AtomicU64::new(0);

/// Number of KASAN reports raised by compiler instrumentation since boot,
/// including those suppressed by the [`MAX_REPORTS`] cap.
#[must_use]
pub fn report_count() -> u64 {
    REPORTS.load(Ordering::Relaxed)
}

/// Emit one report for an access the inline check rejected.
///
/// Deliberately does not panic: see the module docs on `-asan-recover`. The
/// caller (LLVM's slow path) proceeds to perform the access afterwards.
#[cfg_attr(kasan_instrumented, sanitize(address = "off"))]
#[inline(never)]
fn report(addr: usize, size: usize, is_write: bool) {
    let n = REPORTS.fetch_add(1, Ordering::Relaxed);
    if n >= MAX_REPORTS {
        if n == MAX_REPORTS {
            serial_println!(
                "[kasan] further reports suppressed after {MAX_REPORTS} \
                 (see `kasan` in kshell for the running total)"
            );
        }
        return;
    }

    let a = addr as u64;
    let shadow = kasan::shadow_byte(a);
    serial_println!(
        "[kasan] CRITICAL: {} on {} of {} bytes @ {:#x} (shadow={:#04x})",
        kasan::describe_shadow(shadow),
        if is_write { "write" } else { "read" },
        size,
        a,
        shadow
    );

    if n < MAX_BACKTRACES {
        crate::backtrace::print_current();
    }
}

// ---------------------------------------------------------------------------
// Fixed-size report entry points
// ---------------------------------------------------------------------------
//
// LLVM picks the entry point by access size and direction, so there is one
// symbol per (load|store) x (1|2|4|8|16). Generating them with a macro keeps
// the ten-way repetition from drifting: a typo in one hand-written variant
// would be a silently wrong size in a report, or a link error, depending on
// which one.

macro_rules! asan_report {
    ($($name:ident => ($size:expr, $write:expr)),* $(,)?) => {
        $(
            /// LLVM instrumentation slow-path callback.
            ///
            /// # Safety
            ///
            /// Called only by compiler-generated code with the faulting access
            /// address. Performs no dereference of `addr`.
            #[cfg_attr(kasan_instrumented, sanitize(address = "off"))]
            #[unsafe(no_mangle)]
            pub unsafe extern "C" fn $name(addr: usize) {
                report(addr, $size, $write);
            }
        )*
    };
}

asan_report! {
    __asan_report_load1_noabort => (1, false),
    __asan_report_load2_noabort => (2, false),
    __asan_report_load4_noabort => (4, false),
    __asan_report_load8_noabort => (8, false),
    __asan_report_load16_noabort => (16, false),
    __asan_report_store1_noabort => (1, true),
    __asan_report_store2_noabort => (2, true),
    __asan_report_store4_noabort => (4, true),
    __asan_report_store8_noabort => (8, true),
    __asan_report_store16_noabort => (16, true),
    // Abort-mode names, present so that building with `-asan-recover=0`
    // links. They still return; the compiler puts a `ud2` after the call in
    // that mode, so control does not actually come back here.
    __asan_report_load1 => (1, false),
    __asan_report_load2 => (2, false),
    __asan_report_load4 => (4, false),
    __asan_report_load8 => (8, false),
    __asan_report_load16 => (16, false),
    __asan_report_store1 => (1, true),
    __asan_report_store2 => (2, true),
    __asan_report_store4 => (4, true),
    __asan_report_store8 => (8, true),
    __asan_report_store16 => (16, true),
}

// ---------------------------------------------------------------------------
// Variable-size report entry points
// ---------------------------------------------------------------------------

macro_rules! asan_report_n {
    ($($name:ident => $write:expr),* $(,)?) => {
        $(
            /// LLVM instrumentation slow-path callback for an access whose
            /// size is not a power of two up to 16 (e.g. a struct copy).
            ///
            /// # Safety
            ///
            /// Called only by compiler-generated code. Performs no dereference.
            #[cfg_attr(kasan_instrumented, sanitize(address = "off"))]
            #[unsafe(no_mangle)]
            pub unsafe extern "C" fn $name(addr: usize, size: usize) {
                report(addr, size, $write);
            }
        )*
    };
}

asan_report_n! {
    __asan_report_load_n_noabort => false,
    __asan_report_store_n_noabort => true,
    __asan_report_load_n => false,
    __asan_report_store_n => true,
}

// ---------------------------------------------------------------------------
// Outlined check entry points
// ---------------------------------------------------------------------------
//
// These are the primary entry points: the profile sets
// `-asan-instrumentation-with-call-threshold=0`, so LLVM calls one of them for
// *every* checked access rather than emitting the compare inline. See the
// module docs for why. Unlike the report entry points, these must do the check
// themselves — and everything they call must be free of instrumented accesses,
// or the check recurses into itself.

macro_rules! asan_check {
    ($($name:ident => ($size:expr, $write:expr)),* $(,)?) => {
        $(
            /// Outlined KASAN check (used instead of the inline sequence in
            /// very large functions).
            ///
            /// # Safety
            ///
            /// Called only by compiler-generated code. Performs no dereference.
            #[cfg_attr(kasan_instrumented, sanitize(address = "off"))]
            #[unsafe(no_mangle)]
            pub unsafe extern "C" fn $name(addr: usize) {
                if !kasan::shadow_allows(addr as u64, $size) {
                    report(addr, $size, $write);
                }
            }
        )*
    };
}

asan_check! {
    __asan_load1_noabort => (1, false),
    __asan_load2_noabort => (2, false),
    __asan_load4_noabort => (4, false),
    __asan_load8_noabort => (8, false),
    __asan_load16_noabort => (16, false),
    __asan_store1_noabort => (1, true),
    __asan_store2_noabort => (2, true),
    __asan_store4_noabort => (4, true),
    __asan_store8_noabort => (8, true),
    __asan_store16_noabort => (16, true),
    __asan_load1 => (1, false),
    __asan_load2 => (2, false),
    __asan_load4 => (4, false),
    __asan_load8 => (8, false),
    __asan_load16 => (16, false),
    __asan_store1 => (1, true),
    __asan_store2 => (2, true),
    __asan_store4 => (4, true),
    __asan_store8 => (8, true),
    __asan_store16 => (16, true),
}

macro_rules! asan_check_n {
    ($($name:ident => $write:expr),* $(,)?) => {
        $(
            /// Outlined KASAN check for an arbitrary-size access.
            ///
            /// # Safety
            ///
            /// Called only by compiler-generated code. Performs no dereference.
            #[cfg_attr(kasan_instrumented, sanitize(address = "off"))]
            #[unsafe(no_mangle)]
            pub unsafe extern "C" fn $name(addr: usize, size: usize) {
                if !kasan::shadow_allows(addr as u64, size) {
                    report(addr, size, $write);
                }
            }
        )*
    };
}

asan_check_n! {
    __asan_loadN_noabort => false,
    __asan_storeN_noabort => true,
    __asan_loadN => false,
    __asan_storeN => true,
}

// ---------------------------------------------------------------------------
// Bookkeeping entry points
// ---------------------------------------------------------------------------

/// Called before a `noreturn` call so a real ASan runtime can unpoison the
/// stack frames that are about to be abandoned.
///
/// With `-asan-stack=0` nothing on the stack is ever poisoned, so there is
/// nothing to undo and this is genuinely empty — not a stub awaiting work. The
/// symbol must still exist: LLVM emits the call unconditionally.
///
/// # Safety
///
/// Called only by compiler-generated code.
#[cfg_attr(kasan_instrumented, sanitize(address = "off"))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __asan_handle_no_return() {}

/// Register instrumented globals. See [`__asan_handle_no_return`] — with
/// `-asan-globals=0` no globals are instrumented and this is never called; it
/// exists so that turning globals instrumentation on is a one-flag change
/// rather than a link failure.
///
/// # Safety
///
/// Called only by compiler-generated module constructors.
#[cfg_attr(kasan_instrumented, sanitize(address = "off"))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __asan_register_globals(_globals: *mut u8, _n: usize) {}

/// Counterpart to [`__asan_register_globals`].
///
/// # Safety
///
/// Called only by compiler-generated module destructors.
#[cfg_attr(kasan_instrumented, sanitize(address = "off"))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __asan_unregister_globals(_globals: *mut u8, _n: usize) {}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Boot self-test: drive the report path exactly as the compiler would.
///
/// This is worth testing even in the uninstrumented build, because the report
/// path is only ever *exercised* in the profile where something is already
/// broken — the worst possible time to discover that it panics, recurses, or
/// prints nonsense. Calling the entry points directly with a known-poisoned
/// heap address checks the plumbing end to end without needing the instrumented
/// toolchain.
pub fn self_test() {
    serial_println!("[kasan-rt] Running self-test...");
    let mut skips = crate::fs::selftest::Skips::new();
    // Set inside the closure below; a `&mut Skips` capture would work too, but
    // a plain flag keeps the closure's captures trivially `FnMut`.
    let mut no_shadowed_address = false;

    // A freed heap object: `mm::kasan` marks the whole slot 0xFA, so the
    // outlined check must reject it and the report path must run.
    //
    // The address is borrowed for the duration of the closure rather than
    // returned: it names a slot that is back on the allocator's free list while
    // still poisoned, so letting it escape leaves a live-once-reallocated
    // address permanently marked freed. See `kasan::with_self_test_freed_address`.
    kasan::with_self_test_freed_address(|freed| match freed {
        Some(freed) => {
            // The snapshot is taken *here*, after the setup call, not before
            // it.  `self_test_freed_address` performs a real `alloc`/`dealloc`,
            // and the allocator's own free-magic and redzone machinery then
            // touches the slot it has just poisoned.  Snapshotting before the
            // call counted that setup traffic as if it were the thing under
            // test — in the instrumented build the assertion below failed by
            // exactly the size of that flood (217 vs 101).  The flood itself is
            // fixed separately (`mm::rawmem`), but the measurement window
            // should not have included setup in the first place.
            let before = report_count();

            // SAFETY: these are the compiler's own entry points; they only read
            // shadow for `freed` and never dereference it. `freed` is a real
            // (now freed) heap address, so its shadow byte exists.
            let allowed = unsafe {
                __asan_store8_noabort(freed as usize);
                kasan::shadow_allows(freed, 8)
            };
            assert!(!allowed, "kasan-rt: freed address passed the check");
            assert_eq!(
                report_count(),
                before.wrapping_add(1),
                "kasan-rt: outlined check did not report"
            );

            // The fixed-size report entry point must count too, and must not
            // re-enter or fault.
            // SAFETY: as above — report-only, no dereference.
            unsafe { __asan_report_store8_noabort(freed as usize) };
            assert_eq!(
                report_count(),
                before.wrapping_add(2),
                "kasan-rt: report entry point did not count"
            );
            serial_println!("[kasan-rt]   report path on freed heap: OK");
        }
        None => {
            no_shadowed_address = true;
            serial_println!(
                "[kasan-rt]   SKIPPED: no shadowed heap address available \
                 (KASAN shadow window not backed)"
            );
        }
    });
    if no_shadowed_address {
        skips.record(
            "outlined check and report path on a freed slot",
            "the KASAN shadow window is not backed",
        );
    }

    // The bookkeeping entry points must be callable.
    // SAFETY: all three are no-ops that touch nothing.
    unsafe {
        __asan_handle_no_return();
        __asan_register_globals(core::ptr::null_mut(), 0);
        __asan_unregister_globals(core::ptr::null_mut(), 0);
    }
    serial_println!("[kasan-rt]   bookkeeping entry points: OK");

    skips.report("[kasan-rt]");
    serial_println!("[kasan-rt] Self-test PASSED{}", skips.suffix());
}
