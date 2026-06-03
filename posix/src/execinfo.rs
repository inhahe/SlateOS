//! `<execinfo.h>` — backtrace support.
//!
//! Implements `backtrace`, `backtrace_symbols`, `backtrace_symbols_fd`.
//!
//! ## Implementation
//!
//! `backtrace()` on `x86_64` walks the saved-rbp chain.  Our workspace
//! sets `-C force-frame-pointers=yes` for the bare-metal target so
//! every kernel/userspace frame has a valid rbp slot pointing at the
//! previous frame.  On non-`x86_64` targets and on the host test build
//! (where frame pointers aren't guaranteed) we return 0 frames.
//!
//! `backtrace_symbols()` formats each address as `"0x" + 16 hex
//! digits`.  We don't have an in-process symbol table yet, so callers
//! that need symbol names will need to post-process with `addr2line`
//! or similar against the binary on disk.  The returned array is a
//! single `malloc`ed block (matching glibc behavior) — caller frees
//! the array with `free()` once it's done with the strings.
//!
//! `backtrace_symbols_fd()` writes the same format directly to a file
//! descriptor without calling `malloc`.

use crate::file;

// ---------------------------------------------------------------------------
// Stack walking
// ---------------------------------------------------------------------------

/// Minimum plausible address for a saved rbp — anything below this is
/// definitely garbage (NULL page, low-canonical noise).  Used to break
/// the unwind loop when we hit a corrupt or absent frame.
const MIN_FRAME_PTR: usize = 0x1000;

/// Hard cap on frames we'll walk before giving up.  Real call stacks
/// rarely exceed 256 frames; a runaway chain is a sign of a corrupt
/// rbp loop and we'd rather bail than spin.
const MAX_WALK: usize = 256;

/// Read the current frame pointer (`rbp` on x86_64).
///
/// # Safety
///
/// The returned value is the *callee's* rbp at the moment of the read.
/// It points at the caller's frame slot, which contains the caller's
/// saved rbp at offset 0 and the return address at offset +8 — the
/// standard System V / Microsoft x64 layout when frame pointers are
/// emitted.
#[cfg(target_arch = "x86_64")]
#[inline(always)]
unsafe fn read_rbp() -> usize {
    let rbp: usize;
    // SAFETY: Reading rbp has no side effects and clobbers nothing.
    unsafe {
        core::arch::asm!(
            "mov {}, rbp",
            out(reg) rbp,
            options(nomem, nostack, preserves_flags),
        );
    }
    rbp
}

/// Walk the saved-rbp chain on x86_64 and write up to `size` return
/// addresses into `buffer`.  Returns the number of frames captured.
///
/// Validates every pointer dereference: rbp must be aligned, above the
/// NULL page, and strictly greater than the previous rbp (the stack
/// grows downward, so frame pointers grow upward as we unwind).  Stops
/// at the first invalid pointer, a NULL return address, or after
/// `MAX_WALK` frames.
#[cfg(target_arch = "x86_64")]
fn walk_x86_64(buffer: *mut *mut u8, size: i32) -> i32 {
    if buffer.is_null() || size <= 0 {
        return 0;
    }
    // SAFETY: read_rbp has no observable side effects.
    let mut rbp = unsafe { read_rbp() };
    let mut count: i32 = 0;
    let mut walks: usize = 0;
    while count < size && walks < MAX_WALK {
        walks += 1;
        // Validate the candidate frame pointer.
        if rbp < MIN_FRAME_PTR || rbp & 7 != 0 {
            break;
        }
        // Read return address at rbp + 8.  This is a raw load that
        // could fault if the chain is corrupt — but if rbp passed the
        // sanity check and the caller compiled with frame pointers,
        // this is in a valid stack frame.
        let ret_addr_slot = (rbp + 8) as *const usize;
        // SAFETY: We have already validated `rbp` is above the NULL
        // page and properly aligned.  The read is from inside the
        // current thread's stack frame (or a parent frame), which is
        // always mapped while we hold its rbp.  A corrupt chain would
        // be caught by the alignment/range check on the next iteration.
        let ret_addr = unsafe { core::ptr::read_volatile(ret_addr_slot) };
        if ret_addr == 0 {
            break;
        }
        // SAFETY: buffer is non-null (checked above) and `count` is
        // bounded by `size`, so the indexed write is in-bounds.
        unsafe {
            *buffer.offset(count as isize) = ret_addr as *mut u8;
        }
        count = count.saturating_add(1);
        // Advance to the previous frame.
        let prev_rbp_slot = rbp as *const usize;
        // SAFETY: same justification as the return-address read above.
        let prev_rbp = unsafe { core::ptr::read_volatile(prev_rbp_slot) };
        // The stack grows down → unwinding moves rbp to higher addresses.
        // A non-increasing rbp means either we hit the bottom-of-stack
        // sentinel (typically 0) or the chain is corrupt.
        if prev_rbp <= rbp {
            break;
        }
        rbp = prev_rbp;
    }
    count
}

// ---------------------------------------------------------------------------
// Hex formatting
// ---------------------------------------------------------------------------

/// Format a 64-bit address into `buf` as `"0x" + 16 hex digits` (18 bytes).
/// Always writes exactly 18 bytes.  Returns the slice that was written.
fn format_hex_addr(addr: u64, buf: &mut [u8; 18]) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    buf[0] = b'0';
    buf[1] = b'x';
    for i in 0..16 {
        let nibble = ((addr >> (60 - i * 4)) & 0xf) as usize;
        buf[2 + i] = HEX[nibble];
    }
}

// ---------------------------------------------------------------------------
// backtrace
// ---------------------------------------------------------------------------

/// `backtrace` — capture a stack backtrace.
///
/// Stores return addresses from the call stack into `buffer`, up to
/// `size` entries.  Returns the number of addresses captured.
///
/// On x86_64 walks the saved-rbp chain (requires frame pointers, which
/// our build flags enforce).  On other targets returns 0.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn backtrace(buffer: *mut *mut u8, size: i32) -> i32 {
    #[cfg(target_arch = "x86_64")]
    {
        walk_x86_64(buffer, size)
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        let _ = (buffer, size);
        0
    }
}

// ---------------------------------------------------------------------------
// backtrace_symbols
// ---------------------------------------------------------------------------

/// `backtrace_symbols` — translate addresses into symbol strings.
///
/// Takes an array of `size` addresses (from `backtrace()`) and returns
/// a `malloc`ed array of `size` C-string pointers.  Each pointer
/// references storage inside the same allocation (so freeing the
/// returned pointer frees all the strings together — the caller must
/// **not** free the individual strings).
///
/// Without an in-process symbol table, each entry is formatted as
/// `"0x" + 16 hex digits`.  Callers that need real symbol names should
/// post-process with `addr2line` against the on-disk binary.
///
/// Returns null on allocation failure or invalid inputs.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn backtrace_symbols(buffer: *const *mut u8, size: i32) -> *mut *mut u8 {
    if buffer.is_null() || size <= 0 {
        return core::ptr::null_mut();
    }
    let n = size as usize;
    // Layout: n pointers, then n × 19 bytes (18 hex + 1 NUL terminator).
    let ptrs_bytes = n.checked_mul(core::mem::size_of::<*mut u8>()).unwrap_or(0);
    let strs_bytes = n.checked_mul(19).unwrap_or(0);
    let total = match ptrs_bytes.checked_add(strs_bytes) {
        Some(v) if v > 0 => v,
        _ => return core::ptr::null_mut(),
    };
    let block = crate::malloc::malloc(total);
    if block.is_null() {
        return core::ptr::null_mut();
    }
    // Lay out the pointer array (front) and the string area (back-of-block).
    let ptr_array = block.cast::<*mut u8>();
    // SAFETY: `block` was just allocated with `total` bytes; the strings
    // area starts immediately after the pointer array.
    let str_area = unsafe { block.add(ptrs_bytes) };
    for i in 0..n {
        // SAFETY: `buffer` has `size` valid entries by contract; `i < n`.
        let addr = unsafe { *buffer.add(i) } as u64;
        let mut hex_buf = [0u8; 18];
        format_hex_addr(addr, &mut hex_buf);
        // SAFETY: `str_area` has at least `n * 19` bytes; offset is in-bounds.
        let str_ptr = unsafe { str_area.add(i * 19) };
        // Write the 18 hex bytes plus a trailing NUL.
        for j in 0..18 {
            // SAFETY: same — bounds-checked above.
            unsafe {
                *str_ptr.add(j) = hex_buf[j];
            }
        }
        // SAFETY: 18 bytes written into a 19-byte slot; final byte is NUL.
        unsafe {
            *str_ptr.add(18) = 0;
        }
        // SAFETY: pointer array has `n` slots; `i < n`.
        unsafe {
            *ptr_array.add(i) = str_ptr;
        }
    }
    ptr_array
}

// ---------------------------------------------------------------------------
// backtrace_symbols_fd
// ---------------------------------------------------------------------------

/// `backtrace_symbols_fd` — write symbol descriptions to a file descriptor.
///
/// Like `backtrace_symbols`, but writes the strings directly to `fd`
/// (one per line, terminated by `\n`) instead of allocating.  Useful
/// for signal handlers and other contexts where calling `malloc` is
/// unsafe.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn backtrace_symbols_fd(buffer: *const *mut u8, size: i32, fd: i32) {
    if buffer.is_null() || size <= 0 {
        return;
    }
    let n = size as usize;
    for i in 0..n {
        // SAFETY: `buffer` has `size` valid entries by contract; `i < n`.
        let addr = unsafe { *buffer.add(i) } as u64;
        let mut line = [0u8; 19];
        let mut hex = [0u8; 18];
        format_hex_addr(addr, &mut hex);
        line[..18].copy_from_slice(&hex);
        line[18] = b'\n';
        // Best-effort write — ignore short writes and errors (signal-
        // handler contexts may have a bad fd; we don't want to spin).
        let _ = file::write(fd, line.as_ptr(), 19);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // backtrace
    // -----------------------------------------------------------------------

    #[test]
    fn test_backtrace_returns_bounded() {
        let mut buf = [core::ptr::null_mut(); 64];
        let ret = backtrace(buf.as_mut_ptr(), 64);
        // Result must be in [0, size].  We don't pin a specific value
        // because the host test build may or may not have frame
        // pointers (returning 0 is also valid).
        assert!(ret >= 0, "backtrace must not return negative");
        assert!(ret <= 64, "backtrace must respect the size cap");
    }

    #[test]
    fn test_backtrace_null_buffer() {
        let ret = backtrace(core::ptr::null_mut(), 0);
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_backtrace_zero_size() {
        let mut buf = [core::ptr::null_mut(); 1];
        let ret = backtrace(buf.as_mut_ptr(), 0);
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_backtrace_negative_size() {
        let ret = backtrace(core::ptr::null_mut(), -1);
        assert_eq!(ret, 0);
    }

    // -----------------------------------------------------------------------
    // backtrace_symbols
    // -----------------------------------------------------------------------

    #[test]
    fn test_backtrace_symbols_returns_formatted_addresses() {
        // Use synthetic addresses — the format step doesn't care
        // whether they're real return addresses.
        let buf: [*mut u8; 3] = [
            0x1234_5678_9abc_def0u64 as *mut u8,
            0x0000_0000_dead_beefu64 as *mut u8,
            0x0000_0000_0000_0001u64 as *mut u8,
        ];
        let result = backtrace_symbols(buf.as_ptr(), 3);
        // Host test build: malloc/mmap may not function (no kernel behind
        // the SYSCALL instruction).  Skip the success-path assertions in
        // that case — the format_hex_addr tests cover the formatting
        // logic, and the bare-metal build exercises the malloc path.
        if result.is_null() {
            return;
        }
        // Read back the three string pointers and verify the hex layout.
        for i in 0..3 {
            // SAFETY: result has 3 valid slots.
            let s = unsafe { *result.add(i) };
            assert!(!s.is_null());
            // First two chars should be "0x".
            // SAFETY: each string has 18 bytes + NUL.
            let s0 = unsafe { *s };
            let s1 = unsafe { *s.add(1) };
            assert_eq!(s0, b'0');
            assert_eq!(s1, b'x');
            // 18th byte is the NUL terminator.
            // SAFETY: 19-byte slot, valid.
            let nul = unsafe { *s.add(18) };
            assert_eq!(nul, 0);
        }
        // First address: 0x123456789abcdef0 → hex chars after 0x.
        // SAFETY: first slot valid.
        let s = unsafe { *result };
        // SAFETY: 18 bytes available.
        let hex_bytes: [u8; 16] = core::array::from_fn(|i| unsafe { *s.add(2 + i) });
        assert_eq!(&hex_bytes, b"123456789abcdef0");
        // Cleanup.
        // SAFETY: `result` is a non-null pointer returned by malloc above.
        unsafe {
            crate::malloc::free(result.cast::<u8>());
        }
    }

    #[test]
    fn test_backtrace_symbols_zero_returns_null() {
        let buf = [core::ptr::null_mut(); 1];
        let ret = backtrace_symbols(buf.as_ptr(), 0);
        assert!(
            ret.is_null(),
            "backtrace_symbols(_, 0) returns null per glibc convention"
        );
    }

    #[test]
    fn test_backtrace_symbols_negative_size() {
        let buf = [core::ptr::null_mut(); 1];
        let ret = backtrace_symbols(buf.as_ptr(), -5);
        assert!(ret.is_null());
    }

    #[test]
    fn test_backtrace_symbols_null_buffer() {
        let ret = backtrace_symbols(core::ptr::null(), 4);
        assert!(ret.is_null());
    }

    // -----------------------------------------------------------------------
    // backtrace_symbols_fd
    // -----------------------------------------------------------------------

    #[test]
    fn test_backtrace_symbols_fd_no_crash() {
        let buf = [core::ptr::null_mut(); 5];
        backtrace_symbols_fd(buf.as_ptr(), 5, 2);
        // Survived — no crash.
    }

    #[test]
    fn test_backtrace_symbols_fd_null_buffer() {
        backtrace_symbols_fd(core::ptr::null(), 0, 2);
    }

    #[test]
    fn test_backtrace_symbols_fd_invalid_fd() {
        let buf = [core::ptr::null_mut(); 1];
        backtrace_symbols_fd(buf.as_ptr(), 1, -1);
    }

    // -----------------------------------------------------------------------
    // format_hex_addr
    // -----------------------------------------------------------------------

    #[test]
    fn test_format_hex_addr_zero() {
        let mut buf = [0u8; 18];
        format_hex_addr(0, &mut buf);
        assert_eq!(&buf, b"0x0000000000000000");
    }

    #[test]
    fn test_format_hex_addr_max() {
        let mut buf = [0u8; 18];
        format_hex_addr(u64::MAX, &mut buf);
        assert_eq!(&buf, b"0xffffffffffffffff");
    }

    #[test]
    fn test_format_hex_addr_mixed() {
        let mut buf = [0u8; 18];
        format_hex_addr(0xdead_beef_0000_1234, &mut buf);
        assert_eq!(&buf, b"0xdeadbeef00001234");
    }

    // -----------------------------------------------------------------------
    // Full workflow
    // -----------------------------------------------------------------------

    #[test]
    fn test_capture_and_symbolize_workflow() {
        let mut addrs = [core::ptr::null_mut(); 128];
        let nframes = backtrace(addrs.as_mut_ptr(), 128);
        assert!(nframes >= 0 && nframes <= 128);

        // backtrace_symbols on whatever we captured.  We tolerate a null
        // result here because malloc may not function in the host test
        // build (no kernel behind SYSCALL).
        if nframes > 0 {
            let symbols = backtrace_symbols(addrs.as_ptr(), nframes);
            if !symbols.is_null() {
                // SAFETY: malloc'd pointer from above.
                unsafe {
                    crate::malloc::free(symbols.cast::<u8>());
                }
            }
        }

        // backtrace_symbols_fd is a no-op when nframes == 0; safe either way.
        backtrace_symbols_fd(addrs.as_ptr(), nframes, 2);
    }
}
