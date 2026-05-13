//! Scanf family: `sscanf`, `scanf`, `fscanf` via assembly trampoline.
//!
//! Like printf, scanf is variadic in C.  We use assembly wrappers
//! to capture the variadic output-pointer arguments into an array,
//! then pass that to a Rust scanning engine.
//!
//! `scanf` and `fscanf` read a line from stdin/stream into a stack
//! buffer, then scan it with the same engine as `sscanf`.
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

#[cfg(not(test))]
core::arch::global_asm!(
    // sscanf(str, fmt, ...) → _sscanf_impl(str, fmt, args_ptr)
    ".global sscanf",
    ".type sscanf, @function",
    "sscanf:",
    "push rbp",
    "mov rbp, rsp",
    "sub rsp, 64",
    "mov [rsp], rdx",        // vararg 0
    "mov [rsp+8], rcx",      // vararg 1
    "mov [rsp+16], r8",      // vararg 2
    "mov [rsp+24], r9",      // vararg 3
    "mov rax, [rbp+16]",     // vararg 4 (stack)
    "mov [rsp+32], rax",
    "mov rax, [rbp+24]",     // vararg 5
    "mov [rsp+40], rax",
    "mov rax, [rbp+32]",     // vararg 6
    "mov [rsp+48], rax",
    "mov rax, [rbp+40]",     // vararg 7
    "mov [rsp+56], rax",
    // rdi = str, rsi = fmt (already set)
    "mov rdx, rsp",          // args array
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
    "mov [rsp], rsi",        // vararg 0
    "mov [rsp+8], rdx",      // vararg 1
    "mov [rsp+16], rcx",     // vararg 2
    "mov [rsp+24], r8",      // vararg 3
    "mov [rsp+32], r9",      // vararg 4
    "mov rax, [rbp+16]",     // vararg 5 (stack)
    "mov [rsp+40], rax",
    "mov rax, [rbp+24]",     // vararg 6
    "mov [rsp+48], rax",
    "mov rax, [rbp+32]",     // vararg 7
    "mov [rsp+56], rax",
    // rdi = fmt (already set)
    "mov rsi, rsp",          // args array
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
    "mov [rsp], rdx",        // vararg 0
    "mov [rsp+8], rcx",      // vararg 1
    "mov [rsp+16], r8",      // vararg 2
    "mov [rsp+24], r9",      // vararg 3
    "mov rax, [rbp+16]",     // vararg 4 (stack)
    "mov [rsp+32], rax",
    "mov rax, [rbp+24]",     // vararg 5
    "mov [rsp+40], rax",
    "mov rax, [rbp+32]",     // vararg 6
    "mov [rsp+48], rax",
    "mov rax, [rbp+40]",     // vararg 7
    "mov [rsp+56], rax",
    // rdi = stream, rsi = fmt (already set)
    "mov rdx, rsp",          // args array
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
);

// ---------------------------------------------------------------------------
// Rust entry point (called by assembly)
// ---------------------------------------------------------------------------

/// `sscanf(str, fmt, ...)` — parse formatted input from a string.
///
/// Returns the number of items successfully assigned, or EOF (-1)
/// if the input is exhausted before the first conversion.
#[unsafe(no_mangle)]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn _sscanf_impl(
    input: *const u8,
    fmt: *const u8,
    args: *const u64,
) -> i32 {
    if input.is_null() || fmt.is_null() {
        return -1;
    }

    let mut ctx = ScanCtx {
        input,
        fmt,
        args,
        si: 0,      // Position in input string.
        fi: 0,      // Position in format string.
        ai: 0,      // Position in args array.
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
#[unsafe(no_mangle)]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn _scanf_impl(
    fmt: *const u8,
    args: *const u64,
) -> i32 {
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
#[unsafe(no_mangle)]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn _fscanf_impl(
    stream: *mut u8,
    fmt: *const u8,
    args: *const u64,
) -> i32 {
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
                    .wrapping_mul(10)
                    .wrapping_add(usize::from(ctx.fmt_peek().wrapping_sub(b'0')));
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
                            unsafe { *ptr = ctx.si as i32; }
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
    let max = if has_sign { width.saturating_sub(1) } else { width };

    while count < max {
        let c = ctx.peek();
        if !c.is_ascii_digit() {
            break;
        }
        val = val.wrapping_mul(10).wrapping_add(i64::from(c.wrapping_sub(b'0')));
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
                unsafe { *p = val; }
            }
        } else {
            let p = ptr as *mut i32;
            if !p.is_null() {
                unsafe { *p = val as i32; }
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
fn scan_signed_int_auto(
    ctx: &mut ScanCtx,
    suppress: bool,
    width: usize,
    long_mod: u8,
) -> bool {
    ctx.skip_ws();
    if ctx.peek() == 0 {
        return false;
    }

    let negative = ctx.peek() == b'-';
    let has_sign = negative || ctx.peek() == b'+';
    if has_sign {
        ctx.advance();
    }

    let mut remaining = if has_sign { width.saturating_sub(1) } else { width };

    // Detect base from prefix.
    let base: u64;
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
                unsafe { *p = val; }
            }
        } else {
            let p = ptr as *mut i32;
            if !p.is_null() {
                unsafe { *p = val as i32; }
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

    // Skip optional 0x prefix for hex.
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
        return false;
    }

    if !suppress {
        let ptr = ctx.next_arg();
        if long_mod >= 1 {
            let p = ptr as *mut u64;
            if !p.is_null() {
                unsafe { *p = val; }
            }
        } else {
            let p = ptr as *mut u32;
            if !p.is_null() {
                unsafe { *p = val as u32; }
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
            unsafe { *dest.add(count) = c; }
        }
        ctx.advance();
        count = count.wrapping_add(1);
    }

    if count == 0 {
        return false;
    }

    // Null-terminate.
    if !suppress && !dest.is_null() {
        unsafe { *dest.add(count) = 0; }
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
            unsafe { *dest.add(count) = c; }
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

    // Exponent.
    if count < width && (ctx.peek() == b'e' || ctx.peek() == b'E') && bi < 62 {
        buf_put(&mut buf, &mut bi, ctx.peek());
        ctx.advance();
        count = count.wrapping_add(1);

        if count < width && (ctx.peek() == b'-' || ctx.peek() == b'+') && bi < 62 {
            buf_put(&mut buf, &mut bi, ctx.peek());
            ctx.advance();
            count = count.wrapping_add(1);
        }

        while count < width && ctx.peek().is_ascii_digit() && bi < 62 {
            buf_put(&mut buf, &mut bi, ctx.peek());
            ctx.advance();
            count = count.wrapping_add(1);
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
                unsafe { *p = val; }
            }
        } else {
            // %f → f32
            let p = ptr as *mut f32;
            if !p.is_null() {
                unsafe { *p = val as f32; }
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
        bitmap[(c >> 3) as usize] |= 1u8 << (c & 7);
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
                bitmap[(ch >> 3) as usize] |= 1u8 << (ch & 7);
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
            bitmap[(c >> 3) as usize] |= 1u8 << (c & 7);
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

        let in_set = (bitmap[(c >> 3) as usize] & (1u8 << (c & 7))) != 0;
        let matches = if negated { !in_set } else { in_set };

        if !matches {
            break;
        }

        if !suppress && !dest.is_null() {
            unsafe { *dest.add(count) = c; }
        }
        ctx.advance();
        count = count.wrapping_add(1);
    }

    if count == 0 {
        return false;
    }

    // Null-terminate.
    if !suppress && !dest.is_null() {
        unsafe { *dest.add(count) = 0; }
    }
    if !suppress {
        ctx.assigned += 1;
    }
    true
}
