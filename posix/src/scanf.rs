//! Scanf family: `sscanf`, `scanf`, `fscanf` via assembly trampoline.
//!
//! Like printf, scanf is variadic in C.  We use assembly wrappers
//! to capture the variadic output-pointer arguments into an array,
//! then pass that to a Rust scanning engine.
//!
//! `scanf` and `fscanf` read a line from stdin/stream into a stack
//! buffer, then scan it with the same engine as `sscanf`.
//!
//! The `v*` variants (`vsscanf`, `vscanf`, `vfscanf`) take a `va_list`
//! instead of `...`.  Since a `va_list` parameter decays to a pointer on the
//! x86_64 System V ABI, they are ordinary Rust functions: they flatten the
//! destination pointers out of the `va_list` (every scanf argument is a
//! pointer, so all come from the integer register/overflow path) and delegate
//! to the same engine.  The glibc `__isoc99_v*scanf` aliases are provided too.
//!
//! ## Supported Format Specifiers
//!
//! - `%d` — signed decimal integer → `*mut i32`
//! - `%i` — signed integer with auto-detect base (0x→hex, 0→oct, else dec) → `*mut i32`
//! - `%u` — unsigned decimal integer → `*mut u32`
//! - `%ld`, `%li`, `%lu` — long variants → `*mut i64` / `*mut u64`
//! - `%lld`, `%lli`, `%llu` — long long → same as long on LP64
//! - `%x`, `%X` — unsigned hex → `*mut u32` (or `*mut u64` with `l`)
//! - `%o` — unsigned octal → `*mut u32` (or `*mut u64` with `l`)
//! - `%s` — whitespace-delimited string → `*mut u8` buffer
//! - `%c` — single character → `*mut u8`
//! - `%f`, `%lf` — floating-point → `*mut f32` / `*mut f64`
//! - `%n` — characters consumed so far → `*mut i32`
//! - `%%` — literal percent (consumed, not assigned)
//! - `%[...]` — scanset (character class matching)
//!   - `%[abc]` matches characters in the set {a, b, c}
//!   - `%[^abc]` matches characters NOT in the set
//!   - `%[a-z]` matches range a through z
//!   - `%[]abc]` leading `]` is part of the set
//! - Width: `%5d` limits digits consumed
//! - `*` (assignment suppression): `%*d` reads but doesn't store

// ---------------------------------------------------------------------------
// Assembly trampolines
// ---------------------------------------------------------------------------

// sscanf(str, fmt, ...) → _sscanf_impl(str, fmt, args_ptr)
// scanf(fmt, ...) → reads from stdin (stub)

// The `_*_impl` symbols defined in this module are the Rust-side targets of
// the assembly variadic trampolines.  The leading underscore is part of the
// ABI contract.
#![allow(clippy::used_underscore_items)]

#[cfg(target_os = "none")]
core::arch::global_asm!(
    // sscanf(str, fmt, ...) → _sscanf_impl(str, fmt, args_ptr)
    ".global sscanf",
    ".type sscanf, @function",
    "sscanf:",
    "push rbp",
    "mov rbp, rsp",
    "sub rsp, 64",
    "mov [rsp], rdx",    // vararg 0
    "mov [rsp+8], rcx",  // vararg 1
    "mov [rsp+16], r8",  // vararg 2
    "mov [rsp+24], r9",  // vararg 3
    "mov rax, [rbp+16]", // vararg 4 (stack)
    "mov [rsp+32], rax",
    "mov rax, [rbp+24]", // vararg 5
    "mov [rsp+40], rax",
    "mov rax, [rbp+32]", // vararg 6
    "mov [rsp+48], rax",
    "mov rax, [rbp+40]", // vararg 7
    "mov [rsp+56], rax",
    // rdi = str, rsi = fmt (already set)
    "mov rdx, rsp", // args array
    "call _sscanf_impl",
    "add rsp, 64",
    "pop rbp",
    "ret",
    // scanf(fmt, ...) → _scanf_impl(fmt, args_ptr)
    // Like printf: rdi = fmt, rsi..r9 = first 5 varargs.
    ".global scanf",
    ".type scanf, @function",
    "scanf:",
    "push rbp",
    "mov rbp, rsp",
    "sub rsp, 64",
    "mov [rsp], rsi",    // vararg 0
    "mov [rsp+8], rdx",  // vararg 1
    "mov [rsp+16], rcx", // vararg 2
    "mov [rsp+24], r8",  // vararg 3
    "mov [rsp+32], r9",  // vararg 4
    "mov rax, [rbp+16]", // vararg 5 (stack)
    "mov [rsp+40], rax",
    "mov rax, [rbp+24]", // vararg 6
    "mov [rsp+48], rax",
    "mov rax, [rbp+32]", // vararg 7
    "mov [rsp+56], rax",
    // rdi = fmt (already set)
    "mov rsi, rsp", // args array
    "call _scanf_impl",
    "add rsp, 64",
    "pop rbp",
    "ret",
    // fscanf(stream, fmt, ...) → _fscanf_impl(stream, fmt, args_ptr)
    // rdi = stream, rsi = fmt, rdx..r9 = first 4 varargs.
    ".global fscanf",
    ".type fscanf, @function",
    "fscanf:",
    "push rbp",
    "mov rbp, rsp",
    "sub rsp, 64",
    "mov [rsp], rdx",    // vararg 0
    "mov [rsp+8], rcx",  // vararg 1
    "mov [rsp+16], r8",  // vararg 2
    "mov [rsp+24], r9",  // vararg 3
    "mov rax, [rbp+16]", // vararg 4 (stack)
    "mov [rsp+32], rax",
    "mov rax, [rbp+24]", // vararg 5
    "mov [rsp+40], rax",
    "mov rax, [rbp+32]", // vararg 6
    "mov [rsp+48], rax",
    "mov rax, [rbp+40]", // vararg 7
    "mov [rsp+56], rax",
    // rdi = stream, rsi = fmt (already set)
    "mov rdx, rsp", // args array
    "call _fscanf_impl",
    "add rsp, 64",
    "pop rbp",
    "ret",
    // -----------------------------------------------------------------------
    // glibc C99 aliases — identical to the above, just different names.
    // Programs compiled with -std=c99 or later link against __isoc99_*.
    // -----------------------------------------------------------------------
    ".global __isoc99_sscanf",
    ".type __isoc99_sscanf, @function",
    "__isoc99_sscanf:",
    "jmp sscanf",
    ".global __isoc99_scanf",
    ".type __isoc99_scanf, @function",
    "__isoc99_scanf:",
    "jmp scanf",
    ".global __isoc99_fscanf",
    ".type __isoc99_fscanf, @function",
    "__isoc99_fscanf:",
    "jmp fscanf",
    // v* variants take a va_list (no varargs), so the C99 aliases are plain
    // tail-jumps to the Rust functions below.
    ".global __isoc99_vsscanf",
    ".type __isoc99_vsscanf, @function",
    "__isoc99_vsscanf:",
    "jmp vsscanf",
    ".global __isoc99_vscanf",
    ".type __isoc99_vscanf, @function",
    "__isoc99_vscanf:",
    "jmp vscanf",
    ".global __isoc99_vfscanf",
    ".type __isoc99_vfscanf, @function",
    "__isoc99_vfscanf:",
    "jmp vfscanf",
);

// ---------------------------------------------------------------------------
// Rust entry point (called by assembly)
// ---------------------------------------------------------------------------

/// `sscanf(str, fmt, ...)` — parse formatted input from a string.
///
/// Returns the number of items successfully assigned, or EOF (-1)
/// if the input is exhausted before the first conversion.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn _sscanf_impl(input: *const u8, fmt: *const u8, args: *const u64) -> i32 {
    if input.is_null() || fmt.is_null() {
        return -1;
    }

    let mut ctx = ScanCtx {
        input,
        fmt,
        args,
        si: 0,       // Position in input string.
        fi: 0,       // Position in format string.
        ai: 0,       // Position in args array.
        assigned: 0, // Number of successful assignments.
    };

    scan_core(&mut ctx)
}

/// Buffer size for reading a line from stdin/stream for scanf/fscanf.
const SCANF_LINE_BUF: usize = 4096;

/// `scanf(fmt, ...)` — parse formatted input from stdin.
///
/// Reads a line from stdin, then scans it using the same engine as
/// `sscanf`.  Returns the number of items successfully assigned,
/// or EOF (-1) if stdin is at end-of-file.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn _scanf_impl(fmt: *const u8, args: *const u64) -> i32 {
    if fmt.is_null() {
        return -1;
    }

    // Read a line from stdin (fd 0) into a stack buffer.
    let mut buf = [0u8; SCANF_LINE_BUF];
    let n = read_line_from_fd(0, &mut buf);
    if n == 0 {
        return -1; // EOF
    }

    let mut ctx = ScanCtx {
        input: buf.as_ptr(),
        fmt,
        args,
        si: 0,
        fi: 0,
        ai: 0,
        assigned: 0,
    };

    scan_core(&mut ctx)
}

/// `fscanf(stream, fmt, ...)` — parse formatted input from a stream.
///
/// Reads a line from the given stream, then scans it.  The stream
/// pointer is interpreted as a FILE* (our stdio fd encoding).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn _fscanf_impl(stream: *mut u8, fmt: *const u8, args: *const u64) -> i32 {
    if fmt.is_null() {
        return -1;
    }

    // Extract fd from FILE*.
    let fd = crate::stdio::fileno(stream);
    if fd < 0 {
        return -1;
    }

    let mut buf = [0u8; SCANF_LINE_BUF];
    let n = read_line_from_fd(fd, &mut buf);
    if n == 0 {
        return -1; // EOF
    }

    let mut ctx = ScanCtx {
        input: buf.as_ptr(),
        fmt,
        args,
        si: 0,
        fi: 0,
        ai: 0,
        assigned: 0,
    };

    scan_core(&mut ctx)
}

// ---------------------------------------------------------------------------
// va_list support — the v* scanf family
// ---------------------------------------------------------------------------
//
// `vsscanf`/`vscanf`/`vfscanf` receive an already-initialised `va_list`
// (which decays to a pointer on the x86_64 System V ABI) instead of `...`, so
// they are plain Rust functions — host-testable.  Every scanf argument is an
// output *pointer* (integer class), so flattening is simpler than printf's:
// we walk the format string once, mirroring `scan_core`'s specifier parsing,
// and pull one pointer per non-suppressed conversion via the SysV `va_arg`
// integer path into the same flat `[u64; 8]` array the engine consumes.

use crate::printf::{self, VaList};

/// Flatten the destination pointers referenced by `fmt` out of `va` into the
/// array `_sscanf_impl` expects.
///
/// Mirrors `scan_core`: `%*` suppresses (no pointer), `%%` and literal text
/// consume nothing, an unknown conversion stops the scan (so we stop pulling),
/// and every other conversion (`d i u x X o s c [ f e g a n`) consumes exactly
/// one pointer.  At most 8 are stored (the engine's fixed-array contract).
///
/// # Safety
/// `va` must be a valid `va_list` holding pointer arguments matching `fmt`.
#[allow(clippy::arithmetic_side_effects)]
unsafe fn va_collect_scanf(fmt: *const u8, va: &mut VaList) -> [u64; 8] {
    let mut args = [0u64; 8];
    let mut idx: usize = 0;

    if fmt.is_null() {
        return args;
    }

    let mut fi: usize = 0;
    loop {
        // SAFETY: fmt is NUL-terminated; the loop stops at the NUL.
        let fc = unsafe { *fmt.add(fi) };
        if fc == 0 {
            break;
        }
        if fc != b'%' {
            fi = fi.wrapping_add(1);
            continue;
        }
        fi = fi.wrapping_add(1); // skip '%'

        let spec = unsafe { *fmt.add(fi) };
        if spec == 0 {
            break;
        }
        if spec == b'%' {
            // Literal percent — consumes no argument.
            fi = fi.wrapping_add(1);
            continue;
        }

        // Suppression flag.
        let suppress = spec == b'*';
        if suppress {
            fi = fi.wrapping_add(1);
        }

        // Field width (digits — never an arg in scanf).
        while unsafe { *fmt.add(fi) }.is_ascii_digit() {
            fi = fi.wrapping_add(1);
        }

        // Length modifiers (consume no args).
        match unsafe { *fmt.add(fi) } {
            b'l' => {
                fi = fi.wrapping_add(1);
                if unsafe { *fmt.add(fi) } == b'l' {
                    fi = fi.wrapping_add(1);
                }
            }
            b'h' => {
                fi = fi.wrapping_add(1);
                if unsafe { *fmt.add(fi) } == b'h' {
                    fi = fi.wrapping_add(1);
                }
            }
            _ => {}
        }

        let conv = unsafe { *fmt.add(fi) };
        if conv == 0 {
            break;
        }
        fi = fi.wrapping_add(1);

        // Skip a scanset body so its contents aren't reparsed as conversions.
        if conv == b'[' {
            if unsafe { *fmt.add(fi) } == b'^' {
                fi = fi.wrapping_add(1);
            }
            // A ']' immediately after '[' (or '[^') is a literal set member.
            if unsafe { *fmt.add(fi) } == b']' {
                fi = fi.wrapping_add(1);
            }
            loop {
                let c = unsafe { *fmt.add(fi) };
                if c == 0 || c == b']' {
                    break;
                }
                fi = fi.wrapping_add(1);
            }
            if unsafe { *fmt.add(fi) } == b']' {
                fi = fi.wrapping_add(1);
            }
        }

        match conv {
            b'd' | b'i' | b'u' | b'x' | b'X' | b'o' | b's' | b'c' | b'[' | b'f' | b'e' | b'g'
            | b'a' | b'n' => {
                if !suppress {
                    // SAFETY: va contract upheld by the caller.
                    let v = unsafe { printf::va_arg_int(va) };
                    if idx < 8 {
                        args[idx] = v;
                    }
                    idx = idx.wrapping_add(1);
                }
            }
            // Unknown conversion: scan_core stops here, so we stop too.
            _ => break,
        }
    }

    args
}

/// `vsscanf(str, fmt, ap)` — `sscanf` with a `va_list`.
///
/// # Safety
/// `str`/`fmt` must be valid NUL-terminated strings and `ap` a valid
/// `va_list` whose pointer arguments match the conversions in `fmt`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn vsscanf(input: *const u8, fmt: *const u8, ap: *mut VaList) -> i32 {
    if ap.is_null() {
        return -1;
    }
    // SAFETY: ap is non-null; caller guarantees it is a valid va_list.
    let args = unsafe { va_collect_scanf(fmt, &mut *ap) };
    _sscanf_impl(input, fmt, args.as_ptr())
}

/// `vscanf(fmt, ap)` — `scanf` with a `va_list` (reads from stdin).
///
/// # Safety
/// As [`vsscanf`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn vscanf(fmt: *const u8, ap: *mut VaList) -> i32 {
    if ap.is_null() {
        return -1;
    }
    // SAFETY: ap is non-null; caller guarantees it is a valid va_list.
    let args = unsafe { va_collect_scanf(fmt, &mut *ap) };
    _scanf_impl(fmt, args.as_ptr())
}

/// `vfscanf(stream, fmt, ap)` — `fscanf` with a `va_list`.
///
/// # Safety
/// As [`vsscanf`]; `stream` must be a valid `FILE*`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn vfscanf(stream: *mut u8, fmt: *const u8, ap: *mut VaList) -> i32 {
    if ap.is_null() {
        return -1;
    }
    // SAFETY: ap is non-null; caller guarantees it is a valid va_list.
    let args = unsafe { va_collect_scanf(fmt, &mut *ap) };
    _fscanf_impl(stream, fmt, args.as_ptr())
}

/// Read bytes from a file descriptor until newline or buffer full.
///
/// Returns the number of bytes read (0 on EOF/error).  The buffer
/// is always null-terminated.
fn read_line_from_fd(fd: i32, buf: &mut [u8; SCANF_LINE_BUF]) -> usize {
    let mut pos: usize = 0;
    let max = SCANF_LINE_BUF.wrapping_sub(1); // Leave room for NUL.

    while pos < max {
        let mut byte = 0u8;
        let ret = crate::file::read(fd, &raw mut byte, 1);
        if ret <= 0 {
            break; // EOF or error.
        }
        if let Some(slot) = buf.get_mut(pos) {
            *slot = byte;
        }
        pos = pos.wrapping_add(1);
        if byte == b'\n' {
            break;
        }
    }

    // Null-terminate.
    if let Some(slot) = buf.get_mut(pos) {
        *slot = 0;
    }
    pos
}

// ---------------------------------------------------------------------------
// Scan context
// ---------------------------------------------------------------------------

/// Bundles all mutable state for the scanning engine.
struct ScanCtx {
    input: *const u8,
    fmt: *const u8,
    args: *const u64,
    si: usize,
    fi: usize,
    ai: usize,
    assigned: i32,
}

impl ScanCtx {
    /// Read the current input byte (0 if past end).
    #[inline]
    fn peek(&self) -> u8 {
        // SAFETY: Caller guarantees input is a valid null-terminated string.
        unsafe { *self.input.add(self.si) }
    }

    /// Read the current format byte.
    #[inline]
    fn fmt_peek(&self) -> u8 {
        unsafe { *self.fmt.add(self.fi) }
    }

    /// Advance input by one byte.
    #[inline]
    fn advance(&mut self) {
        self.si = self.si.wrapping_add(1);
    }

    /// Advance format by one byte.
    #[inline]
    fn fmt_advance(&mut self) {
        self.fi = self.fi.wrapping_add(1);
    }

    /// Consume the next arg pointer.
    #[inline]
    fn next_arg(&mut self) -> u64 {
        let v = unsafe { *self.args.add(self.ai) };
        self.ai = self.ai.wrapping_add(1);
        v
    }

    /// Skip ASCII whitespace in input.
    fn skip_ws(&mut self) {
        while matches!(self.peek(), b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c) {
            self.advance();
        }
    }
}

/// Write a byte into a fixed-size buffer at position `bi`, then advance.
///
/// Uses `.get_mut()` to avoid panicking on out-of-bounds (silently
/// drops the byte if the buffer is full).
#[inline]
fn buf_put(buf: &mut [u8; 64], bi: &mut usize, byte: u8) {
    if let Some(slot) = buf.get_mut(*bi) {
        *slot = byte;
    }
    *bi = bi.wrapping_add(1);
}

// ---------------------------------------------------------------------------
// Core scanning engine
// ---------------------------------------------------------------------------

/// Main scan loop.
#[allow(clippy::arithmetic_side_effects, clippy::too_many_lines)]
fn scan_core(ctx: &mut ScanCtx) -> i32 {
    loop {
        let fc = ctx.fmt_peek();
        if fc == 0 {
            break;
        }

        // Whitespace in format matches zero or more whitespace in input.
        if matches!(fc, b' ' | b'\t' | b'\n' | b'\r') {
            ctx.fmt_advance();
            ctx.skip_ws();
            continue;
        }

        // Format specifier.
        if fc == b'%' {
            ctx.fmt_advance();
            let spec = ctx.fmt_peek();
            if spec == 0 {
                break;
            }

            // Literal %%.
            if spec == b'%' {
                ctx.fmt_advance();
                if ctx.peek() != b'%' {
                    // Input mismatch.
                    break;
                }
                ctx.advance();
                continue;
            }

            // Parse optional '*' (suppression flag).
            let suppress = spec == b'*';
            if suppress {
                ctx.fmt_advance();
            }

            // Parse optional width.
            let mut width: usize = 0;
            let mut has_width = false;
            while ctx.fmt_peek().is_ascii_digit() {
                has_width = true;
                width = width
                    .saturating_mul(10)
                    .saturating_add(usize::from(ctx.fmt_peek().wrapping_sub(b'0')));
                ctx.fmt_advance();
            }
            if !has_width {
                width = usize::MAX; // No limit.
            }

            // Parse length modifier.
            let mut long_mod = 0u8; // 0=none, 1=l, 2=ll
            if ctx.fmt_peek() == b'l' {
                long_mod = 1;
                ctx.fmt_advance();
                if ctx.fmt_peek() == b'l' {
                    long_mod = 2;
                    ctx.fmt_advance();
                }
            } else if ctx.fmt_peek() == b'h' {
                ctx.fmt_advance();
                if ctx.fmt_peek() == b'h' {
                    ctx.fmt_advance();
                }
                // We treat h/hh the same as default (store as i32/u32).
            }

            let conv = ctx.fmt_peek();
            if conv == 0 {
                break;
            }
            ctx.fmt_advance();

            match conv {
                b'd' => {
                    if !scan_signed_int(ctx, suppress, width, long_mod) {
                        break;
                    }
                }
                b'i' => {
                    // %i auto-detects base: 0x/0X → hex, 0 → octal, else decimal.
                    if !scan_signed_int_auto(ctx, suppress, width, long_mod) {
                        break;
                    }
                }
                b'u' => {
                    if !scan_unsigned_int(ctx, suppress, width, long_mod, 10) {
                        break;
                    }
                }
                b'x' | b'X' => {
                    if !scan_unsigned_int(ctx, suppress, width, long_mod, 16) {
                        break;
                    }
                }
                b'o' => {
                    if !scan_unsigned_int(ctx, suppress, width, long_mod, 8) {
                        break;
                    }
                }
                b's' => {
                    if !scan_string(ctx, suppress, width) {
                        break;
                    }
                }
                b'c' => {
                    if !scan_char(ctx, suppress, width, has_width) {
                        break;
                    }
                }
                b'[' => {
                    if !scan_scanset(ctx, suppress, width) {
                        break;
                    }
                }
                b'f' | b'e' | b'g' | b'a' => {
                    if !scan_float(ctx, suppress, width, long_mod) {
                        break;
                    }
                }
                b'n' => {
                    // %n: store characters consumed so far.
                    if !suppress {
                        let ptr = ctx.next_arg() as *mut i32;
                        if !ptr.is_null() {
                            unsafe {
                                *ptr = ctx.si as i32;
                            }
                        }
                    }
                    // %n does NOT count toward assigned.
                }
                _ => {
                    // Unknown specifier — stop.
                    break;
                }
            }
        } else {
            // Literal character — must match input exactly.
            if ctx.peek() != fc {
                break;
            }
            ctx.advance();
            ctx.fmt_advance();
        }
    }

    // Return assigned count, or EOF if nothing was assigned and input ended.
    if ctx.assigned == 0 && ctx.peek() == 0 {
        -1 // EOF
    } else {
        ctx.assigned
    }
}

// ---------------------------------------------------------------------------
// Conversion helpers
// ---------------------------------------------------------------------------

/// Scan a signed decimal integer (`%d`).
///
/// Always base 10.  Returns true if conversion succeeded (even if suppressed).
#[allow(clippy::arithmetic_side_effects)]
fn scan_signed_int(ctx: &mut ScanCtx, suppress: bool, width: usize, long_mod: u8) -> bool {
    ctx.skip_ws();
    if ctx.peek() == 0 {
        return false;
    }

    let negative = ctx.peek() == b'-';
    let has_sign = negative || ctx.peek() == b'+';
    if has_sign {
        ctx.advance();
    }

    let mut val: i64 = 0;
    let mut count: usize = 0;
    let max = if has_sign {
        width.saturating_sub(1)
    } else {
        width
    };

    while count < max {
        let c = ctx.peek();
        if !c.is_ascii_digit() {
            break;
        }
        val = val
            .wrapping_mul(10)
            .wrapping_add(i64::from(c.wrapping_sub(b'0')));
        ctx.advance();
        count = count.wrapping_add(1);
    }

    if count == 0 {
        return false;
    }

    if negative {
        val = val.wrapping_neg();
    }

    if !suppress {
        let ptr = ctx.next_arg();
        if long_mod >= 1 {
            let p = ptr as *mut i64;
            if !p.is_null() {
                unsafe {
                    *p = val;
                }
            }
        } else {
            let p = ptr as *mut i32;
            if !p.is_null() {
                unsafe {
                    *p = val as i32;
                }
            }
        }
        ctx.assigned += 1;
    }
    true
}

/// Scan a signed integer with auto-detected base (`%i`).
///
/// POSIX/C: `%i` detects the base from the input prefix:
/// - `0x` or `0X` → hexadecimal (base 16)
/// - `0` (without x) → octal (base 8)
/// - otherwise → decimal (base 10)
#[allow(clippy::arithmetic_side_effects)]
fn scan_signed_int_auto(ctx: &mut ScanCtx, suppress: bool, width: usize, long_mod: u8) -> bool {
    ctx.skip_ws();
    if ctx.peek() == 0 {
        return false;
    }

    let negative = ctx.peek() == b'-';
    let has_sign = negative || ctx.peek() == b'+';
    if has_sign {
        ctx.advance();
    }

    let mut remaining = if has_sign {
        width.saturating_sub(1)
    } else {
        width
    };

    // Detect base from prefix.
    let base: u64;
    let saved_pos = ctx.si; // Save for rollback if hex prefix is incomplete.
    if ctx.peek() == b'0' && remaining > 0 {
        // Could be hex (0x) or octal (0).
        let next = unsafe { *ctx.input.add(ctx.si.wrapping_add(1)) };
        if (next == b'x' || next == b'X') && remaining > 2 {
            base = 16;
            ctx.advance(); // skip '0'
            ctx.advance(); // skip 'x'/'X'
            remaining = remaining.saturating_sub(2);
        } else {
            base = 8;
            // Don't consume the leading '0' — it's a valid octal digit
            // and the loop below will parse it.
        }
    } else {
        base = 10;
    }

    // Parse digits in the detected base.
    let mut val: i64 = 0;
    let mut count: usize = 0;

    while count < remaining {
        let c = ctx.peek();
        let digit = match c {
            b'0'..=b'9' => i64::from(c.wrapping_sub(b'0')),
            b'a'..=b'f' if base == 16 => i64::from(c.wrapping_sub(b'a')).wrapping_add(10),
            b'A'..=b'F' if base == 16 => i64::from(c.wrapping_sub(b'A')).wrapping_add(10),
            _ => break,
        };
        if digit >= base as i64 {
            break;
        }
        val = val.wrapping_mul(base as i64).wrapping_add(digit);
        ctx.advance();
        count = count.wrapping_add(1);
    }

    if count == 0 {
        if base == 16 {
            // Incomplete hex prefix (e.g. "0xZ"): roll back past the "0x"
            // and re-parse as octal "0".
            ctx.si = saved_pos;
            // The leading '0' is a valid octal integer with value 0.
            ctx.advance(); // consume the '0'
            val = 0;
        } else {
            return false;
        }
    }

    if negative {
        val = val.wrapping_neg();
    }

    if !suppress {
        let ptr = ctx.next_arg();
        if long_mod >= 1 {
            let p = ptr as *mut i64;
            if !p.is_null() {
                unsafe {
                    *p = val;
                }
            }
        } else {
            let p = ptr as *mut i32;
            if !p.is_null() {
                unsafe {
                    *p = val as i32;
                }
            }
        }
        ctx.assigned += 1;
    }
    true
}

/// Scan an unsigned integer in a given base.
#[allow(clippy::arithmetic_side_effects)]
fn scan_unsigned_int(
    ctx: &mut ScanCtx,
    suppress: bool,
    width: usize,
    long_mod: u8,
    base: u64,
) -> bool {
    ctx.skip_ws();
    if ctx.peek() == 0 {
        return false;
    }

    // Skip optional 0x/0X prefix for hex, with backtracking if no
    // valid hex digit follows (e.g. "0xG" → parse "0" as the result).
    let saved_pos = ctx.si;
    let mut consumed_prefix: usize = 0;
    if base == 16 && ctx.peek() == b'0' {
        let next = unsafe { *ctx.input.add(ctx.si.wrapping_add(1)) };
        if next == b'x' || next == b'X' {
            ctx.advance();
            ctx.advance();
            consumed_prefix = 2;
        }
    }

    let mut val: u64 = 0;
    let mut count: usize = 0;
    let max = width.saturating_sub(consumed_prefix);

    while count < max {
        let c = ctx.peek();
        let digit = match c {
            b'0'..=b'9' => u64::from(c.wrapping_sub(b'0')),
            b'a'..=b'f' if base == 16 => u64::from(c.wrapping_sub(b'a')).wrapping_add(10),
            b'A'..=b'F' if base == 16 => u64::from(c.wrapping_sub(b'A')).wrapping_add(10),
            _ => break,
        };
        if digit >= base {
            break;
        }
        val = val.wrapping_mul(base).wrapping_add(digit);
        ctx.advance();
        count = count.wrapping_add(1);
    }

    if count == 0 {
        if consumed_prefix > 0 {
            // Incomplete hex prefix ("0xG"): backtrack and parse "0".
            ctx.si = saved_pos;
            ctx.advance(); // consume the '0'
            val = 0;
        } else {
            return false;
        }
    }

    if !suppress {
        let ptr = ctx.next_arg();
        if long_mod >= 1 {
            let p = ptr as *mut u64;
            if !p.is_null() {
                unsafe {
                    *p = val;
                }
            }
        } else {
            let p = ptr as *mut u32;
            if !p.is_null() {
                unsafe {
                    *p = val as u32;
                }
            }
        }
        ctx.assigned += 1;
    }
    true
}

/// Scan a whitespace-delimited string.
#[allow(clippy::arithmetic_side_effects)]
fn scan_string(ctx: &mut ScanCtx, suppress: bool, width: usize) -> bool {
    ctx.skip_ws();
    if ctx.peek() == 0 {
        return false;
    }

    let ptr = if suppress { 0 } else { ctx.next_arg() };
    let dest = ptr as *mut u8;
    let mut count: usize = 0;

    while count < width {
        let c = ctx.peek();
        if c == 0 || matches!(c, b' ' | b'\t' | b'\n' | b'\r') {
            break;
        }
        if !suppress && !dest.is_null() {
            unsafe {
                *dest.add(count) = c;
            }
        }
        ctx.advance();
        count = count.wrapping_add(1);
    }

    if count == 0 {
        return false;
    }

    // Null-terminate.
    if !suppress && !dest.is_null() {
        unsafe {
            *dest.add(count) = 0;
        }
    }
    if !suppress {
        ctx.assigned += 1;
    }
    true
}

/// Scan character(s).
#[allow(clippy::arithmetic_side_effects)]
fn scan_char(ctx: &mut ScanCtx, suppress: bool, width: usize, has_width: bool) -> bool {
    // %c does NOT skip whitespace (unlike %s, %d, etc.).
    let n = if has_width { width } else { 1 };

    if ctx.peek() == 0 {
        return false;
    }

    let ptr = if suppress { 0 } else { ctx.next_arg() };
    let dest = ptr as *mut u8;
    let mut count: usize = 0;

    while count < n {
        let c = ctx.peek();
        if c == 0 {
            break;
        }
        if !suppress && !dest.is_null() {
            unsafe {
                *dest.add(count) = c;
            }
        }
        ctx.advance();
        count = count.wrapping_add(1);
    }

    if count == 0 {
        return false;
    }

    if !suppress {
        ctx.assigned += 1;
    }
    true
}

/// Scan a floating-point number.
///
/// Parses `[sign]digits[.digits][e[sign]digits]` and stores as f32
/// or f64 (depending on length modifier).
#[allow(clippy::arithmetic_side_effects)]
fn scan_float(ctx: &mut ScanCtx, suppress: bool, width: usize, long_mod: u8) -> bool {
    ctx.skip_ws();
    if ctx.peek() == 0 {
        return false;
    }

    // Collect the float string into a small buffer, then parse.
    let mut buf = [0u8; 64];
    let mut bi: usize = 0;
    let mut count: usize = 0;

    // Sign.
    if (ctx.peek() == b'-' || ctx.peek() == b'+') && count < width {
        buf_put(&mut buf, &mut bi, ctx.peek());
        ctx.advance();
        count = count.wrapping_add(1);
    }

    let mut has_digits = false;

    // Integer digits.
    while count < width && ctx.peek().is_ascii_digit() && bi < 62 {
        buf_put(&mut buf, &mut bi, ctx.peek());
        ctx.advance();
        count = count.wrapping_add(1);
        has_digits = true;
    }

    // Decimal point.
    if count < width && ctx.peek() == b'.' && bi < 62 {
        buf_put(&mut buf, &mut bi, ctx.peek());
        ctx.advance();
        count = count.wrapping_add(1);

        while count < width && ctx.peek().is_ascii_digit() && bi < 62 {
            buf_put(&mut buf, &mut bi, ctx.peek());
            ctx.advance();
            count = count.wrapping_add(1);
            has_digits = true;
        }
    }

    if !has_digits {
        return false;
    }

    // Exponent.  Only consume the 'e'/sign prefix if actual exponent
    // digits follow; otherwise leave them in the input (scanf must only
    // consume the longest valid prefix, and "1.5e" is not valid — only
    // "1.5" is).
    if count < width && (ctx.peek() == b'e' || ctx.peek() == b'E') && bi < 62 {
        let saved_si = ctx.si;
        let saved_buf_idx = bi;
        let save_count = count;

        buf_put(&mut buf, &mut bi, ctx.peek());
        ctx.advance();
        count = count.wrapping_add(1);

        if count < width && (ctx.peek() == b'-' || ctx.peek() == b'+') && bi < 62 {
            buf_put(&mut buf, &mut bi, ctx.peek());
            ctx.advance();
            count = count.wrapping_add(1);
        }

        let exp_digit_start = count;
        while count < width && ctx.peek().is_ascii_digit() && bi < 62 {
            buf_put(&mut buf, &mut bi, ctx.peek());
            ctx.advance();
            count = count.wrapping_add(1);
        }

        if count == exp_digit_start {
            // No exponent digits after 'e'[sign] — rollback.
            ctx.si = saved_si;
            bi = saved_buf_idx;
            let _ = save_count; // count unused after this block.
        }
    }

    // Null-terminate.
    buf_put(&mut buf, &mut bi, 0);

    if !suppress {
        // Parse the collected string using strtod.
        let val = unsafe { crate::stdlib::strtod(buf.as_ptr(), core::ptr::null_mut()) };
        let ptr = ctx.next_arg();
        if long_mod >= 1 {
            // %lf → f64
            let p = ptr as *mut f64;
            if !p.is_null() {
                unsafe {
                    *p = val;
                }
            }
        } else {
            // %f → f32
            let p = ptr as *mut f32;
            if !p.is_null() {
                unsafe {
                    *p = val as f32;
                }
            }
        }
        ctx.assigned += 1;
    }
    true
}

/// Scan a `%[...]` scanset.
///
/// Reads characters from input that match (or don't match, if negated)
/// the set of characters specified between the brackets.
///
/// - `%[abc]`: matches any of a, b, c.
/// - `%[^abc]`: matches anything NOT in {a, b, c}.
/// - `%[a-z]`: matches the range a through z.
/// - `%[]abc]`: a leading `]` is part of the set (not the terminator).
///
/// The scanset is stored as a 256-bit bitmap (32 bytes) for O(1) lookup.
#[allow(clippy::arithmetic_side_effects)]
fn scan_scanset(ctx: &mut ScanCtx, suppress: bool, width: usize) -> bool {
    // %[ does NOT skip whitespace (like %c).

    // Build the character class bitmap from the format string.
    // 256 bits = 32 bytes, one bit per possible byte value.
    let mut bitmap = [0u8; 32];
    let mut negated = false;

    // Check for negation.
    if ctx.fmt_peek() == b'^' {
        negated = true;
        ctx.fmt_advance();
    }

    // A leading ']' right after '[' (or '[^') is part of the set,
    // not the closing bracket.
    if ctx.fmt_peek() == b']' {
        let c = b']';
        // SAFETY: c is u8 so c >> 3 <= 31 < 32, always in bounds.
        if let Some(slot) = bitmap.get_mut((c >> 3) as usize) {
            *slot |= 1u8 << (c & 7);
        }
        ctx.fmt_advance();
    }

    // Parse the rest of the scanset until ']' or end of format.
    loop {
        let c = ctx.fmt_peek();
        if c == 0 || c == b']' {
            break;
        }

        // Check for range: a-z.
        let next1 = unsafe { *ctx.fmt.add(ctx.fi.wrapping_add(1)) };
        let next2 = unsafe { *ctx.fmt.add(ctx.fi.wrapping_add(2)) };
        if next1 == b'-' && next2 != b']' && next2 != 0 {
            // Range c..next2 (inclusive).
            let lo = c;
            let hi = next2;
            let (lo, hi) = if lo <= hi { (lo, hi) } else { (hi, lo) };
            let mut ch = lo;
            loop {
                // SAFETY: ch is u8 so ch >> 3 <= 31 < 32, always in bounds.
                if let Some(slot) = bitmap.get_mut((ch >> 3) as usize) {
                    *slot |= 1u8 << (ch & 7);
                }
                if ch == hi {
                    break;
                }
                ch = ch.wrapping_add(1);
            }
            ctx.fmt_advance(); // skip start
            ctx.fmt_advance(); // skip '-'
            ctx.fmt_advance(); // skip end
        } else {
            // Single character.
            // SAFETY: c is u8 so c >> 3 <= 31 < 32, always in bounds.
            if let Some(slot) = bitmap.get_mut((c >> 3) as usize) {
                *slot |= 1u8 << (c & 7);
            }
            ctx.fmt_advance();
        }
    }

    // Skip closing ']'.
    if ctx.fmt_peek() == b']' {
        ctx.fmt_advance();
    }

    // Now scan input using the bitmap.
    if ctx.peek() == 0 {
        return false;
    }

    let ptr = if suppress { 0 } else { ctx.next_arg() };
    let dest = ptr as *mut u8;
    let mut count: usize = 0;

    while count < width {
        let c = ctx.peek();
        if c == 0 {
            break;
        }

        // SAFETY: c is u8 so c >> 3 <= 31 < 32, always in bounds.
        let in_set = bitmap
            .get((c >> 3) as usize)
            .is_some_and(|slot| slot & (1u8 << (c & 7)) != 0);
        let matches = if negated { !in_set } else { in_set };

        if !matches {
            break;
        }

        if !suppress && !dest.is_null() {
            unsafe {
                *dest.add(count) = c;
            }
        }
        ctx.advance();
        count = count.wrapping_add(1);
    }

    if count == 0 {
        return false;
    }

    // Null-terminate.
    if !suppress && !dest.is_null() {
        unsafe {
            *dest.add(count) = 0;
        }
    }
    if !suppress {
        ctx.assigned += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- %d signed integer tests --

    #[test]
    fn scan_d_basic() {
        let mut val: i32 = 0;
        let args = [&raw mut val as u64];
        let n = _sscanf_impl(b"42\0".as_ptr(), b"%d\0".as_ptr(), args.as_ptr());
        assert_eq!(n, 1);
        assert_eq!(val, 42);
    }

    #[test]
    fn scan_d_negative() {
        let mut val: i32 = 0;
        let args = [&raw mut val as u64];
        let n = _sscanf_impl(b"-17\0".as_ptr(), b"%d\0".as_ptr(), args.as_ptr());
        assert_eq!(n, 1);
        assert_eq!(val, -17);
    }

    #[test]
    fn scan_d_positive_sign() {
        let mut val: i32 = 0;
        let args = [&raw mut val as u64];
        let n = _sscanf_impl(b"+99\0".as_ptr(), b"%d\0".as_ptr(), args.as_ptr());
        assert_eq!(n, 1);
        assert_eq!(val, 99);
    }

    #[test]
    fn scan_d_leading_whitespace() {
        let mut val: i32 = 0;
        let args = [&raw mut val as u64];
        let n = _sscanf_impl(b"   123\0".as_ptr(), b"%d\0".as_ptr(), args.as_ptr());
        assert_eq!(n, 1);
        assert_eq!(val, 123);
    }

    #[test]
    fn scan_d_zero() {
        let mut val: i32 = 99;
        let args = [&raw mut val as u64];
        let n = _sscanf_impl(b"0\0".as_ptr(), b"%d\0".as_ptr(), args.as_ptr());
        assert_eq!(n, 1);
        assert_eq!(val, 0);
    }

    #[test]
    fn scan_d_multiple() {
        let mut a: i32 = 0;
        let mut b: i32 = 0;
        let args = [&raw mut a as u64, &raw mut b as u64];
        let n = _sscanf_impl(b"10 20\0".as_ptr(), b"%d %d\0".as_ptr(), args.as_ptr());
        assert_eq!(n, 2);
        assert_eq!(a, 10);
        assert_eq!(b, 20);
    }

    #[test]
    fn scan_d_stops_at_non_digit() {
        let mut val: i32 = 0;
        let args = [&raw mut val as u64];
        let n = _sscanf_impl(b"42abc\0".as_ptr(), b"%d\0".as_ptr(), args.as_ptr());
        assert_eq!(n, 1);
        assert_eq!(val, 42);
    }

    #[test]
    fn scan_d_empty_input_eof() {
        let mut val: i32 = 99;
        let args = [&raw mut val as u64];
        let n = _sscanf_impl(b"\0".as_ptr(), b"%d\0".as_ptr(), args.as_ptr());
        assert_eq!(n, -1); // EOF
        assert_eq!(val, 99); // Unchanged.
    }

    #[test]
    fn scan_d_no_digits() {
        let mut val: i32 = 99;
        let args = [&raw mut val as u64];
        let n = _sscanf_impl(b"abc\0".as_ptr(), b"%d\0".as_ptr(), args.as_ptr());
        assert_eq!(n, 0);
        assert_eq!(val, 99);
    }

    #[test]
    fn scan_d_with_width() {
        let mut val: i32 = 0;
        let args = [&raw mut val as u64];
        let n = _sscanf_impl(b"12345\0".as_ptr(), b"%3d\0".as_ptr(), args.as_ptr());
        assert_eq!(n, 1);
        assert_eq!(val, 123);
    }

    #[test]
    fn scan_ld_long() {
        let mut val: i64 = 0;
        let args = [&raw mut val as u64];
        let n = _sscanf_impl(b"999999999999\0".as_ptr(), b"%ld\0".as_ptr(), args.as_ptr());
        assert_eq!(n, 1);
        assert_eq!(val, 999_999_999_999i64);
    }

    // -- %u unsigned integer tests --

    #[test]
    fn scan_u_basic() {
        let mut val: u32 = 0;
        let args = [&raw mut val as u64];
        let n = _sscanf_impl(b"65535\0".as_ptr(), b"%u\0".as_ptr(), args.as_ptr());
        assert_eq!(n, 1);
        assert_eq!(val, 65535);
    }

    // -- %x hex tests --

    #[test]
    fn scan_x_basic() {
        let mut val: u32 = 0;
        let args = [&raw mut val as u64];
        let n = _sscanf_impl(b"ff\0".as_ptr(), b"%x\0".as_ptr(), args.as_ptr());
        assert_eq!(n, 1);
        assert_eq!(val, 0xFF);
    }

    #[test]
    fn scan_x_prefix() {
        let mut val: u32 = 0;
        let args = [&raw mut val as u64];
        let n = _sscanf_impl(b"0xFF\0".as_ptr(), b"%x\0".as_ptr(), args.as_ptr());
        assert_eq!(n, 1);
        assert_eq!(val, 0xFF);
    }

    #[test]
    fn scan_x_upper() {
        let mut val: u32 = 0;
        let args = [&raw mut val as u64];
        let n = _sscanf_impl(b"DEADBEEF\0".as_ptr(), b"%X\0".as_ptr(), args.as_ptr());
        assert_eq!(n, 1);
        assert_eq!(val, 0xDEAD_BEEFu32);
    }

    // -- %o octal tests --

    #[test]
    fn scan_o_basic() {
        let mut val: u32 = 0;
        let args = [&raw mut val as u64];
        let n = _sscanf_impl(b"77\0".as_ptr(), b"%o\0".as_ptr(), args.as_ptr());
        assert_eq!(n, 1);
        assert_eq!(val, 0o77);
    }

    // -- %i auto-detect base --

    #[test]
    fn scan_i_decimal() {
        let mut val: i32 = 0;
        let args = [&raw mut val as u64];
        let n = _sscanf_impl(b"42\0".as_ptr(), b"%i\0".as_ptr(), args.as_ptr());
        assert_eq!(n, 1);
        assert_eq!(val, 42);
    }

    #[test]
    fn scan_i_hex() {
        let mut val: i32 = 0;
        let args = [&raw mut val as u64];
        let n = _sscanf_impl(b"0xff\0".as_ptr(), b"%i\0".as_ptr(), args.as_ptr());
        assert_eq!(n, 1);
        assert_eq!(val, 255);
    }

    #[test]
    fn scan_i_octal() {
        let mut val: i32 = 0;
        let args = [&raw mut val as u64];
        let n = _sscanf_impl(b"010\0".as_ptr(), b"%i\0".as_ptr(), args.as_ptr());
        assert_eq!(n, 1);
        assert_eq!(val, 8);
    }

    #[test]
    fn scan_i_negative_hex() {
        let mut val: i32 = 0;
        let args = [&raw mut val as u64];
        let n = _sscanf_impl(b"-0x10\0".as_ptr(), b"%i\0".as_ptr(), args.as_ptr());
        assert_eq!(n, 1);
        assert_eq!(val, -16);
    }

    // -- %s string tests --

    #[test]
    fn scan_s_basic() {
        let mut buf = [0u8; 64];
        let args = [buf.as_mut_ptr() as u64];
        let n = _sscanf_impl(b"hello\0".as_ptr(), b"%s\0".as_ptr(), args.as_ptr());
        assert_eq!(n, 1);
        assert_eq!(&buf[..5], b"hello");
        assert_eq!(buf[5], 0);
    }

    #[test]
    fn scan_s_stops_at_whitespace() {
        let mut buf = [0u8; 64];
        let args = [buf.as_mut_ptr() as u64];
        let n = _sscanf_impl(b"hello world\0".as_ptr(), b"%s\0".as_ptr(), args.as_ptr());
        assert_eq!(n, 1);
        assert_eq!(&buf[..5], b"hello");
        assert_eq!(buf[5], 0);
    }

    #[test]
    fn scan_s_with_width() {
        let mut buf = [0u8; 64];
        let args = [buf.as_mut_ptr() as u64];
        let n = _sscanf_impl(b"longstring\0".as_ptr(), b"%4s\0".as_ptr(), args.as_ptr());
        assert_eq!(n, 1);
        assert_eq!(&buf[..4], b"long");
        assert_eq!(buf[4], 0);
    }

    #[test]
    fn scan_s_multiple() {
        let mut buf1 = [0u8; 64];
        let mut buf2 = [0u8; 64];
        let args = [buf1.as_mut_ptr() as u64, buf2.as_mut_ptr() as u64];
        let n = _sscanf_impl(
            b"hello world\0".as_ptr(),
            b"%s %s\0".as_ptr(),
            args.as_ptr(),
        );
        assert_eq!(n, 2);
        assert_eq!(&buf1[..5], b"hello");
        assert_eq!(&buf2[..5], b"world");
    }

    #[test]
    fn scan_s_leading_whitespace() {
        let mut buf = [0u8; 64];
        let args = [buf.as_mut_ptr() as u64];
        let n = _sscanf_impl(b"  \t  foo\0".as_ptr(), b"%s\0".as_ptr(), args.as_ptr());
        assert_eq!(n, 1);
        assert_eq!(&buf[..3], b"foo");
    }

    // -- %c character tests --

    #[test]
    fn scan_c_single() {
        let mut ch: u8 = 0;
        let args = [&raw mut ch as u64];
        let n = _sscanf_impl(b"A\0".as_ptr(), b"%c\0".as_ptr(), args.as_ptr());
        assert_eq!(n, 1);
        assert_eq!(ch, b'A');
    }

    #[test]
    fn scan_c_no_whitespace_skip() {
        let mut ch: u8 = 0;
        let args = [&raw mut ch as u64];
        let n = _sscanf_impl(b" X\0".as_ptr(), b"%c\0".as_ptr(), args.as_ptr());
        assert_eq!(n, 1);
        assert_eq!(ch, b' ');
    }

    #[test]
    fn scan_c_with_width() {
        let mut buf = [0u8; 8];
        let args = [buf.as_mut_ptr() as u64];
        let n = _sscanf_impl(b"ABCDE\0".as_ptr(), b"%3c\0".as_ptr(), args.as_ptr());
        assert_eq!(n, 1);
        assert_eq!(&buf[..3], b"ABC");
    }

    // -- %n position tests --

    #[test]
    fn scan_n_position() {
        let mut val: i32 = 0;
        let mut pos: i32 = 0;
        let args = [&raw mut val as u64, &raw mut pos as u64];
        let n = _sscanf_impl(
            b"hello 42\0".as_ptr(),
            b"%*s %d%n\0".as_ptr(),
            args.as_ptr(),
        );
        assert_eq!(n, 1);
        assert_eq!(val, 42);
        assert_eq!(pos, 8);
    }

    // -- %% literal percent --

    #[test]
    fn scan_percent_literal() {
        let mut val: i32 = 0;
        let args = [&raw mut val as u64];
        let n = _sscanf_impl(b"%42\0".as_ptr(), b"%%%d\0".as_ptr(), args.as_ptr());
        assert_eq!(n, 1);
        assert_eq!(val, 42);
    }

    #[test]
    fn scan_percent_mismatch() {
        let mut val: i32 = 99;
        let args = [&raw mut val as u64];
        let n = _sscanf_impl(b"X42\0".as_ptr(), b"%%%d\0".as_ptr(), args.as_ptr());
        assert_eq!(n, 0);
        assert_eq!(val, 99);
    }

    // -- Literal character matching --

    #[test]
    fn scan_literal_match() {
        let mut a: i32 = 0;
        let mut b: i32 = 0;
        let args = [&raw mut a as u64, &raw mut b as u64];
        let n = _sscanf_impl(b"10,20\0".as_ptr(), b"%d,%d\0".as_ptr(), args.as_ptr());
        assert_eq!(n, 2);
        assert_eq!(a, 10);
        assert_eq!(b, 20);
    }

    #[test]
    fn scan_literal_mismatch() {
        let mut a: i32 = 0;
        let mut b: i32 = 99;
        let args = [&raw mut a as u64, &raw mut b as u64];
        let n = _sscanf_impl(b"10;20\0".as_ptr(), b"%d,%d\0".as_ptr(), args.as_ptr());
        assert_eq!(n, 1);
        assert_eq!(a, 10);
        assert_eq!(b, 99);
    }

    // -- Suppression (*) --

    #[test]
    fn scan_suppression() {
        let mut val: i32 = 0;
        let args = [&raw mut val as u64];
        let n = _sscanf_impl(
            b"ignored 42\0".as_ptr(),
            b"%*s %d\0".as_ptr(),
            args.as_ptr(),
        );
        assert_eq!(n, 1);
        assert_eq!(val, 42);
    }

    #[test]
    fn scan_suppression_int() {
        let mut val: i32 = 0;
        let args = [&raw mut val as u64];
        let n = _sscanf_impl(b"100 200\0".as_ptr(), b"%*d %d\0".as_ptr(), args.as_ptr());
        assert_eq!(n, 1);
        assert_eq!(val, 200);
    }

    // -- Null input/format --

    #[test]
    fn scan_null_input() {
        let n = _sscanf_impl(core::ptr::null(), b"%d\0".as_ptr(), [].as_ptr());
        assert_eq!(n, -1);
    }

    #[test]
    fn scan_null_format() {
        let n = _sscanf_impl(b"42\0".as_ptr(), core::ptr::null(), [].as_ptr());
        assert_eq!(n, -1);
    }

    // -- %f float tests --

    #[test]
    fn scan_f_basic() {
        let mut val: f32 = 0.0;
        let args = [&raw mut val as u64];
        let n = _sscanf_impl(b"3.14\0".as_ptr(), b"%f\0".as_ptr(), args.as_ptr());
        assert_eq!(n, 1);
        assert!((val - 3.14).abs() < 0.001, "got {val}");
    }

    #[test]
    fn scan_lf_double() {
        let mut val: f64 = 0.0;
        let args = [&raw mut val as u64];
        let n = _sscanf_impl(b"2.718281828\0".as_ptr(), b"%lf\0".as_ptr(), args.as_ptr());
        assert_eq!(n, 1);
        assert!((val - 2.718281828).abs() < 1e-9, "got {val}");
    }

    #[test]
    fn scan_f_negative() {
        let mut val: f32 = 0.0;
        let args = [&raw mut val as u64];
        let n = _sscanf_impl(b"-1.5\0".as_ptr(), b"%f\0".as_ptr(), args.as_ptr());
        assert_eq!(n, 1);
        assert!((val - (-1.5)).abs() < 0.001, "got {val}");
    }

    #[test]
    fn scan_f_scientific() {
        let mut val: f64 = 0.0;
        let args = [&raw mut val as u64];
        let n = _sscanf_impl(b"1.5e3\0".as_ptr(), b"%lf\0".as_ptr(), args.as_ptr());
        assert_eq!(n, 1);
        assert!((val - 1500.0).abs() < 0.001, "got {val}");
    }

    #[test]
    fn scan_f_integer() {
        let mut val: f32 = 0.0;
        let args = [&raw mut val as u64];
        let n = _sscanf_impl(b"42\0".as_ptr(), b"%f\0".as_ptr(), args.as_ptr());
        assert_eq!(n, 1);
        assert!((val - 42.0).abs() < 0.001, "got {val}");
    }

    // -- %[...] scanset tests --

    #[test]
    fn scan_scanset_basic() {
        let mut buf = [0u8; 64];
        let args = [buf.as_mut_ptr() as u64];
        let n = _sscanf_impl(b"abc123\0".as_ptr(), b"%[abc]\0".as_ptr(), args.as_ptr());
        assert_eq!(n, 1);
        assert_eq!(&buf[..3], b"abc");
        assert_eq!(buf[3], 0);
    }

    #[test]
    fn scan_scanset_negated() {
        let mut buf = [0u8; 64];
        let args = [buf.as_mut_ptr() as u64];
        let n = _sscanf_impl(
            b"hello world\0".as_ptr(),
            b"%[^ ]\0".as_ptr(),
            args.as_ptr(),
        );
        assert_eq!(n, 1);
        assert_eq!(&buf[..5], b"hello");
        assert_eq!(buf[5], 0);
    }

    #[test]
    fn scan_scanset_range() {
        let mut buf = [0u8; 64];
        let args = [buf.as_mut_ptr() as u64];
        let n = _sscanf_impl(b"abcXYZ\0".as_ptr(), b"%[a-z]\0".as_ptr(), args.as_ptr());
        assert_eq!(n, 1);
        assert_eq!(&buf[..3], b"abc");
        assert_eq!(buf[3], 0);
    }

    #[test]
    fn scan_scanset_leading_bracket() {
        let mut buf = [0u8; 64];
        let args = [buf.as_mut_ptr() as u64];
        let n = _sscanf_impl(b"]ab\0".as_ptr(), b"%[]ab]\0".as_ptr(), args.as_ptr());
        assert_eq!(n, 1);
        assert_eq!(&buf[..3], b"]ab");
    }

    #[test]
    fn scan_scanset_digits() {
        let mut buf = [0u8; 64];
        let args = [buf.as_mut_ptr() as u64];
        let n = _sscanf_impl(b"12345abc\0".as_ptr(), b"%[0-9]\0".as_ptr(), args.as_ptr());
        assert_eq!(n, 1);
        assert_eq!(&buf[..5], b"12345");
        assert_eq!(buf[5], 0);
    }

    // -- Mixed conversions --

    #[test]
    fn scan_mixed_types() {
        let mut name = [0u8; 64];
        let mut age: i32 = 0;
        let mut score: f32 = 0.0;
        let args = [
            name.as_mut_ptr() as u64,
            &raw mut age as u64,
            &raw mut score as u64,
        ];
        let n = _sscanf_impl(
            b"Alice 30 95.5\0".as_ptr(),
            b"%s %d %f\0".as_ptr(),
            args.as_ptr(),
        );
        assert_eq!(n, 3);
        assert_eq!(&name[..5], b"Alice");
        assert_eq!(age, 30);
        assert!((score - 95.5).abs() < 0.1, "got {score}");
    }

    #[test]
    fn scan_partial_match() {
        let mut a: i32 = 0;
        let mut b: i32 = 99;
        let args = [&raw mut a as u64, &raw mut b as u64];
        let n = _sscanf_impl(b"42 xyz\0".as_ptr(), b"%d %d\0".as_ptr(), args.as_ptr());
        assert_eq!(n, 1);
        assert_eq!(a, 42);
        assert_eq!(b, 99);
    }

    // -- Whitespace matching --

    #[test]
    fn scan_whitespace_in_format() {
        let mut a: i32 = 0;
        let mut b: i32 = 0;
        let args = [&raw mut a as u64, &raw mut b as u64];
        let n = _sscanf_impl(
            b"10\t\t\n  20\0".as_ptr(),
            b"%d %d\0".as_ptr(),
            args.as_ptr(),
        );
        assert_eq!(n, 2);
        assert_eq!(a, 10);
        assert_eq!(b, 20);
    }

    // -- Edge cases --

    #[test]
    fn scan_empty_format() {
        let n = _sscanf_impl(b"hello\0".as_ptr(), b"\0".as_ptr(), [].as_ptr());
        assert_eq!(n, 0);
    }

    #[test]
    fn scan_three_ints() {
        let mut a: i32 = 0;
        let mut b: i32 = 0;
        let mut c: i32 = 0;
        let args = [&raw mut a as u64, &raw mut b as u64, &raw mut c as u64];
        let n = _sscanf_impl(b"1 2 3\0".as_ptr(), b"%d %d %d\0".as_ptr(), args.as_ptr());
        assert_eq!(n, 3);
        assert_eq!(a, 1);
        assert_eq!(b, 2);
        assert_eq!(c, 3);
    }

    #[test]
    fn scan_hex_long() {
        let mut val: u64 = 0;
        let args = [&raw mut val as u64];
        let n = _sscanf_impl(
            b"0xDEADBEEFCAFE\0".as_ptr(),
            b"%lx\0".as_ptr(),
            args.as_ptr(),
        );
        assert_eq!(n, 1);
        assert_eq!(val, 0xDEAD_BEEF_CAFEu64);
    }

    // -----------------------------------------------------------------------
    // Hex prefix backtracking: "0xG" should parse as 0, not fail
    // -----------------------------------------------------------------------

    #[test]
    fn scan_hex_incomplete_prefix_backtracks() {
        // Input "0xG" with %x: "0x" is not followed by hex digit,
        // so backtrack and parse "0" as the hex value 0.
        let mut val: u32 = 99;
        let args = [&raw mut val as u64];
        let n = _sscanf_impl(b"0xG\0".as_ptr(), b"%x\0".as_ptr(), args.as_ptr());
        assert_eq!(n, 1);
        assert_eq!(val, 0);
    }

    #[test]
    fn scan_hex_just_zero() {
        // "0" alone should parse as hex value 0.
        let mut val: u32 = 99;
        let args = [&raw mut val as u64];
        let n = _sscanf_impl(b"0\0".as_ptr(), b"%x\0".as_ptr(), args.as_ptr());
        assert_eq!(n, 1);
        assert_eq!(val, 0);
    }

    #[test]
    fn scan_hex_width_limits_prefix() {
        // "%1x" on "0xFF" — width=1, so only "0" is consumed (1 char).
        // The prefix "0x" would need width >= 3 to be useful.
        let mut val: u32 = 99;
        let args = [&raw mut val as u64];
        let n = _sscanf_impl(b"0xFF\0".as_ptr(), b"%1x\0".as_ptr(), args.as_ptr());
        assert_eq!(n, 1);
        assert_eq!(val, 0);
    }

    #[test]
    fn scan_hex_width_3_parses_one_digit_after_prefix() {
        // "%3x" on "0xFF" — width=3: "0x" prefix (2) + "F" (1) = 3 total.
        let mut val: u32 = 0;
        let args = [&raw mut val as u64];
        let n = _sscanf_impl(b"0xFF\0".as_ptr(), b"%3x\0".as_ptr(), args.as_ptr());
        assert_eq!(n, 1);
        assert_eq!(val, 0xF);
    }

    // -----------------------------------------------------------------------
    // Width overflow: huge width in format string should not wrap
    // -----------------------------------------------------------------------

    #[test]
    fn scan_width_overflow_no_crash() {
        // "%99999999999999999999d" — width overflows usize in wrapping mode.
        // With saturating arithmetic, it becomes usize::MAX (= "no limit").
        let mut val: i32 = 0;
        let args = [&raw mut val as u64];
        let n = _sscanf_impl(
            b"42\0".as_ptr(),
            b"%99999999999999999999d\0".as_ptr(),
            args.as_ptr(),
        );
        assert_eq!(n, 1);
        assert_eq!(val, 42);
    }

    // -----------------------------------------------------------------------
    // v* scanf family (va_list extraction)
    //
    // Builds a synthetic SysV va_list whose GP register save area holds the
    // destination pointers, then calls `vsscanf`.  This exercises
    // `va_collect_scanf` and the `va_arg` integer path without relying on the
    // host's own `va_start` (whose ABI differs on Windows hosts).
    // -----------------------------------------------------------------------

    /// Run `vsscanf` against a synthetic va_list built from `ptrs` (each an
    /// output destination address); up to 6 fit in the GP register file.
    fn run_vsscanf(input: &[u8], fmt: &[u8], ptrs: &[u64]) -> i32 {
        let mut reg = [0u8; 176];
        for (i, &p) in ptrs.iter().enumerate().take(6) {
            let off = i * 8;
            reg[off..off + 8].copy_from_slice(&p.to_le_bytes());
        }
        let mut overflow = [0u8; 64];
        let mut va = VaList {
            gp_offset: 0,
            fp_offset: 48,
            overflow_arg_area: overflow.as_mut_ptr(),
            reg_save_area: reg.as_mut_ptr(),
        };
        // SAFETY: the va_list points at the buffers above and holds enough
        // pointer args for `fmt`.
        unsafe { vsscanf(input.as_ptr(), fmt.as_ptr(), &mut va) }
    }

    #[test]
    fn vsscanf_single_int() {
        let mut val: i32 = 0;
        let n = run_vsscanf(b"42\0", b"%d\0", &[&raw mut val as u64]);
        assert_eq!(n, 1);
        assert_eq!(val, 42);
    }

    #[test]
    fn vsscanf_two_ints() {
        let mut a: i32 = 0;
        let mut b: i32 = 0;
        let n = run_vsscanf(
            b"10 20\0",
            b"%d %d\0",
            &[&raw mut a as u64, &raw mut b as u64],
        );
        assert_eq!(n, 2);
        assert_eq!(a, 10);
        assert_eq!(b, 20);
    }

    #[test]
    fn vsscanf_suppression_skips_pointer() {
        // "%*d %d": the first field is suppressed (consumes no pointer), so
        // the single pointer must bind to the second field.
        let mut val: i32 = 0;
        let n = run_vsscanf(b"100 200\0", b"%*d %d\0", &[&raw mut val as u64]);
        assert_eq!(n, 1);
        assert_eq!(val, 200);
    }

    #[test]
    fn vsscanf_string_and_int() {
        let mut word = [0u8; 16];
        let mut num: i32 = 0;
        let n = run_vsscanf(
            b"foo 7\0",
            b"%s %d\0",
            &[word.as_mut_ptr() as u64, &raw mut num as u64],
        );
        assert_eq!(n, 2);
        assert_eq!(&word[..3], b"foo");
        assert_eq!(num, 7);
    }

    #[test]
    fn vsscanf_float() {
        let mut f: f32 = 0.0;
        let n = run_vsscanf(b"3.5\0", b"%f\0", &[&raw mut f as u64]);
        assert_eq!(n, 1);
        assert!((f - 3.5).abs() < 1e-6);
    }

    #[test]
    fn vsscanf_scanset_then_int() {
        // The scanset body contains digits/letters that must NOT be reparsed
        // as conversions when counting pointers.
        let mut word = [0u8; 16];
        let mut num: i32 = 0;
        let n = run_vsscanf(
            b"abc99\0",
            b"%[a-z]%d\0",
            &[word.as_mut_ptr() as u64, &raw mut num as u64],
        );
        assert_eq!(n, 2);
        assert_eq!(&word[..3], b"abc");
        assert_eq!(num, 99);
    }

    #[test]
    fn vsscanf_null_va_returns_eof() {
        // SAFETY: a null va_list must be rejected, not dereferenced.
        let n = unsafe { vsscanf(b"42\0".as_ptr(), b"%d\0".as_ptr(), core::ptr::null_mut()) };
        assert_eq!(n, -1);
    }
}
