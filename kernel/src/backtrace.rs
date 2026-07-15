//! Kernel stack backtrace for crash diagnostics.
//!
//! Walks the RBP frame pointer chain to produce a list of return addresses.
//! Requires frame pointers to be enabled (`-C force-frame-pointers=yes`);
//! without them, this produces either no output or garbage.
//!
//! ## Usage
//!
//! Called automatically from exception handlers (#PF, #GP, etc.) to print
//! a backtrace showing where the crash occurred.  Also available as a
//! public API for debugging.
//!
//! ## Safety
//!
//! Stack walking is inherently unsafe — we're following pointers through
//! memory that might be corrupted (we're in a crash handler, after all).
//! The walker validates each frame pointer against known good ranges
//! (HHDM stacks, kstack region) and stops at the first invalid pointer.

use crate::mm::kstack;
use crate::mm::page_table;
use crate::serial_println;

/// Maximum number of frames to walk (prevents infinite loops on corrupted
/// frame chains).
const MAX_FRAMES: usize = 32;

/// A single stack frame entry.
#[derive(Debug, Clone, Copy)]
pub struct Frame {
    /// Return address (the instruction after the `call`).
    pub return_addr: u64,
    /// Frame pointer (RBP value at this frame).
    pub frame_ptr: u64,
}

/// Capture a backtrace starting from the current RBP.
///
/// Returns a slice of frames (up to `MAX_FRAMES`) in caller → callee
/// order (index 0 is the immediate caller of `capture()`).
///
/// # Safety
///
/// Frame pointers must be valid.  If the kernel is built without
/// `-C force-frame-pointers=yes`, this may return garbage or nothing.
#[inline(never)] // Ensure this function has its own frame.
pub fn capture() -> BacktraceResult {
    let rbp: u64;
    // SAFETY: Reading RBP is always safe in ring 0.
    unsafe {
        core::arch::asm!("mov {}, rbp", out(reg) rbp, options(nomem, nostack));
    }
    walk_from(rbp)
}

/// Walk the frame chain starting from a given RBP value.
///
/// Used by exception handlers which have access to the faulting task's
/// saved RBP (from the interrupt stack frame or saved context).
pub fn walk_from(start_rbp: u64) -> BacktraceResult {
    let mut frames = [Frame { return_addr: 0, frame_ptr: 0 }; MAX_FRAMES];
    let mut count = 0;
    let mut rbp = start_rbp;

    while count < MAX_FRAMES {
        // Validate the frame pointer before dereferencing.
        if !is_valid_frame_ptr(rbp) {
            break;
        }

        // Each stack frame has:
        //   [rbp + 0] = saved previous RBP
        //   [rbp + 8] = return address
        //
        // SAFETY: We validated that rbp is in a known-good memory range.
        // The pointer is aligned (stack frames are at least 8-byte aligned).
        let prev_rbp: u64;
        let ret_addr: u64;
        unsafe {
            prev_rbp = core::ptr::read_volatile(rbp as *const u64);
            ret_addr = core::ptr::read_volatile((rbp + 8) as *const u64);
        }

        // Stop on null return address (end of chain).
        if ret_addr == 0 {
            break;
        }

        // Stop if the return address is not in kernel text (basic sanity).
        if !is_kernel_text_addr(ret_addr) {
            break;
        }

        frames[count] = Frame {
            return_addr: ret_addr,
            frame_ptr: rbp,
        };
        count += 1;

        // Follow the chain.  Stop if we're going backwards (corruption)
        // or not advancing.
        if prev_rbp <= rbp && prev_rbp != 0 {
            // Frame chain goes backwards — likely corrupted.
            break;
        }
        rbp = prev_rbp;
    }

    BacktraceResult { frames, count }
}

/// Print a backtrace to the serial console.
///
/// Used by exception handlers for crash diagnostics.
pub fn print_from(start_rbp: u64) {
    let bt = walk_from(start_rbp);
    if bt.count == 0 {
        serial_println!("  <no backtrace available (frame pointers missing?)>");
        return;
    }

    serial_println!("  Backtrace ({} frames):", bt.count);
    for i in 0..bt.count {
        let f = &bt.frames[i];
        serial_println!("    #{:2}: {:#018x}", i, f.return_addr);
    }
}

/// Print the current backtrace (from the call site).
#[inline(never)]
#[allow(dead_code)]
pub fn print_current() {
    let rbp: u64;
    // SAFETY: reading RBP is always valid in ring 0; gives us the frame pointer.
    unsafe {
        core::arch::asm!("mov {}, rbp", out(reg) rbp, options(nomem, nostack));
    }
    print_from(rbp);
}

/// Result of a backtrace walk.
pub struct BacktraceResult {
    /// Captured frames.
    pub frames: [Frame; MAX_FRAMES],
    /// Number of valid frames in `frames`.
    pub count: usize,
}

// ---------------------------------------------------------------------------
// Validation helpers
// ---------------------------------------------------------------------------

/// Check if a frame pointer is in a valid kernel stack region.
///
/// Valid regions:
/// - HHDM range (0xFFFF_8000... — bootloader stacks, early-boot stacks)
/// - kstack region (0xFFFF_C100... — guard-page stacks)
/// - The kernel boot stack (a static array inside the image range, matched by
///   its exact bounds — NOT the whole image range, so general .text/.data is
///   rejected)
fn is_valid_frame_ptr(addr: u64) -> bool {
    // Must be non-null and 8-byte aligned (stack frames are word-aligned).
    if addr == 0 || !addr.is_multiple_of(8) {
        return false;
    }

    // Must be in the kernel half (upper canonical addresses).
    if addr < 0xFFFF_8000_0000_0000 {
        return false;
    }

    // Check known valid ranges:
    // 1. HHDM range: [HHDM_offset, HHDM_offset + physical memory size]
    //    We don't know exact bounds, so accept anything in the HHDM PML4 range.
    let hhdm_base = page_table::hhdm().unwrap_or(0xFFFF_8000_0000_0000);
    #[allow(clippy::arithmetic_side_effects)]
    let hhdm_end = hhdm_base + 0x0000_0040_0000_0000; // 256 GiB max physical
    if addr >= hhdm_base && addr < hhdm_end {
        return true;
    }

    // 2. kstack region
    if kstack::is_kstack_region(addr) {
        return true;
    }

    // 3. The dedicated kernel boot stack.  It is a static array in the kernel
    //    image, so its addresses fall in the 0xFFFF_FFFF_8000_0000+ range, but
    //    it is the *only* part of that range that is a legitimate stack.  We
    //    must NOT accept the whole image range: general .text/.data addresses
    //    are not frame pointers, and blindly walking them interprets static
    //    data as a stack-frame chain and prints a misleading garbage backtrace
    //    (observed in the iter19 liveness dump, where a sampled RBP of
    //    0xffffffff824ca080 — kernel .data — produced four bogus frames).
    let (boot_lo, boot_hi) = crate::boot_stack_bounds();
    if addr >= boot_lo && addr < boot_hi {
        return true;
    }

    false
}

/// Check if an address looks like kernel text (return address sanity check).
fn is_kernel_text_addr(addr: u64) -> bool {
    // Kernel text is at 0xFFFF_FFFF_8000_0000 (standard kernel mapping).
    // Also accept addresses in the HHDM range (some trampoline code).
    addr >= 0xFFFF_FFFF_8000_0000 || (addr >= 0xFFFF_8000_0000_0000 && addr < 0xFFFF_C200_0000_0000)
}

/// Raw stack scan: from `rsp`, walk up to `words` qwords and print every
/// slot whose value looks like a kernel return address (`is_kernel_text_addr`).
///
/// This is a diagnostic *complement* to [`print_from`], not a replacement.
/// The RBP frame-pointer walk gives a clean, ordered backtrace **when the
/// frame chain is intact** — but a control-flow hijack (a corrupted return
/// address / function pointer that sends execution into a data region) breaks
/// the chain, so the walk stops at the handler frames and never reveals the
/// culprit. A raw scan side-steps the chain entirely: every plausible return
/// address still physically present on the stack is printed, so the hijacked
/// caller's origin can be recovered by feeding the values to
/// `scripts/resolve-rip.sh`. False positives (data that merely looks like a
/// text address) are expected and acceptable for a post-mortem dump.
///
/// `rsp` must point into a valid kernel stack region; slots outside a known
/// stack region abort the scan (a wild RSP would otherwise fault the handler).
pub fn dump_stack_scan(rsp: u64, words: usize) {
    if !is_valid_frame_ptr(rsp & !0x7) {
        serial_println!("  <stack scan skipped: RSP {rsp:#018x} not in a known stack region>");
        return;
    }
    serial_println!("  Stack scan from RSP {rsp:#018x} (return-address candidates):");
    let base = rsp & !0x7;
    let mut printed = 0usize;
    for i in 0..words {
        // SAFETY: `base` was validated to lie in a known kernel stack region
        // and we advance by 8 bytes per iteration within `words` (bounded);
        // the loop aborts as soon as a slot leaves a valid stack region, so
        // every dereference stays inside mapped stack memory.
        let slot = base.wrapping_add((i as u64).wrapping_mul(8));
        if !is_valid_frame_ptr(slot) {
            break;
        }
        // SAFETY: `slot` is 8-byte aligned and validated to be in a mapped
        // kernel stack region above; reading a u64 from it is sound.
        let val = unsafe { core::ptr::read(slot as *const u64) };
        if is_kernel_text_addr(val) {
            serial_println!("    [{slot:#018x}] = {val:#018x}");
            printed = printed.saturating_add(1);
            if printed >= MAX_FRAMES.saturating_mul(2) {
                serial_println!("    <... more candidates truncated>");
                break;
            }
        }
    }
    if printed == 0 {
        serial_println!("    <no return-address-like values found>");
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Boot-time self-test of the backtrace facility.
///
/// Tests:
/// 1. capture() returns at least one frame.
/// 2. The first frame's return address is in kernel text.
/// 3. walk_from(0) returns zero frames (null pointer stops walk).
#[inline(never)]
pub fn self_test() {
    serial_println!("[backtrace] Running self-test...");

    // Test 1: capture() from here should get at least one frame.
    let bt = capture();
    if bt.count == 0 {
        serial_println!("[backtrace]   WARNING: capture() returned 0 frames");
        serial_println!("[backtrace]   (frame pointers may be absent in release build)");
        serial_println!("[backtrace] Self-test SKIPPED (no frame pointers)");
        return;
    }
    serial_println!("[backtrace]   capture(): {} frame(s)", bt.count);

    // Test 2: first frame return address is in kernel text.
    let first = bt.frames[0].return_addr;
    assert!(
        is_kernel_text_addr(first),
        "first return address {:#x} not in kernel text",
        first
    );
    serial_println!("[backtrace]   First return addr: {:#x} (kernel text: OK)", first);

    // Test 3: walk_from(0) should return nothing.
    let empty = walk_from(0);
    assert_eq!(empty.count, 0, "walk_from(0) should return 0 frames");
    serial_println!("[backtrace]   walk_from(0): 0 frames (OK)");

    serial_println!("[backtrace] Self-test PASSED");
}
