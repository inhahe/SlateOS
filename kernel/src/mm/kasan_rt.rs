//! KASAN runtime — the `__asan_*` callbacks LLVM's instrumentation calls.
//!
//! When the kernel is built with the compiler-KASAN profile
//! (`scripts/kasan-build.sh`, design-decisions.md §107), rustc/LLVM rewrites
//! every load and store into:
//!
//! ```text
//!     mov  %rdi, %rax
//!     shr  $3, %rax
//!     movabs $0xDFFFE00000000000, %rcx     ; kvspace::KASAN_SHADOW_OFFSET
//!     movzbl (%rax,%rcx), %eax             ; the shadow byte
//!     test %al, %al
//!     jne  slow_path                       ; -> __asan_report_*_noabort
//!     <the original access>
//! ```
//!
//! The check itself is emitted *inline*; this module supplies only the cold
//! slow path — the reporting functions LLVM calls once a shadow byte says the
//! access is not cleanly addressable. That is why there is no `check` function
//! here: by the time we are called, the verdict is already in.
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
// LLVM stops emitting the check inline and calls these instead once a single
// function exceeds `-asan-instrumentation-with-call-threshold` accesses
// (7000 by default). Nothing in this kernel is anywhere near that today, but a
// missing symbol here is a *link* failure in a build that is only ever run
// when something is already going wrong — so define them rather than gamble.
// Unlike the report entry points these must do the check themselves.

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

    let before = report_count();

    // A freed heap object: `mm::kasan` marks the whole slot 0xFA, so the
    // outlined check must reject it and the report path must run.
    match kasan::self_test_freed_address() {
        Some(freed) => {
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
            serial_println!(
                "[kasan-rt]   SKIPPED: no shadowed heap address available \
                 (KASAN shadow window not backed)"
            );
        }
    }

    // The bookkeeping entry points must be callable.
    // SAFETY: all three are no-ops that touch nothing.
    unsafe {
        __asan_handle_no_return();
        __asan_register_globals(core::ptr::null_mut(), 0);
        __asan_unregister_globals(core::ptr::null_mut(), 0);
    }
    serial_println!("[kasan-rt]   bookkeeping entry points: OK");

    serial_println!("[kasan-rt] Self-test PASSED");
}
