//! Printf implementation via assembly trampoline.
//!
//! Since Rust's `c_variadic` feature is unstable, we use assembly
//! wrappers to capture variadic arguments from registers into a flat
//! array, then pass that array to a Rust formatting engine.
//!
//! ## Supported Format Specifiers
//!
//! - `%d`, `%i` — signed decimal integer
//! - `%u` — unsigned decimal integer
//! - `%x`, `%X` — unsigned hexadecimal (lower/upper)
//! - `%o` — unsigned octal
//! - `%s` — null-terminated string
//! - `%c` — single character
//! - `%p` — pointer (prints as `0x` + hex)
//! - `%%` — literal percent
//! - `%ld`, `%li`, `%lu`, `%lx`, `%lX`, `%lo` — long variants (same as base on LP64)
//! - `%f`, `%F` — fixed-point decimal (e.g. `3.140000`)
//! - `%e`, `%E` — scientific notation (e.g. `3.140000e+00`)
//! - `%g`, `%G` — auto (%e or %f, whichever is shorter)
//! - Width and precision: `%10d`, `%-10s`, `%08x`, `%.5s`, `%*d`
//! - Flags: `-` (left-align), `0` (zero-pad), `+` (sign), ` ` (space), `#` (alt form)
//!
//! ## Architecture
//!
//! 1. Assembly wrappers (`printf`, `fprintf`, `dprintf`, `sprintf`,
//!    `snprintf`, `asprintf`) capture register args (rsi-r9) into a
//!    stack array and call the corresponding `_*_impl` Rust function.
//! 2. All `_*_impl` functions call `format_core()` which parses the
//!    format string and consumes args from the array.
//! 3. Output goes to a buffer (snprintf/sprintf), a `FILE*` stream
//!    (printf/fprintf), a raw fd (dprintf), or a malloc'd buffer
//!    (asprintf).

// ---------------------------------------------------------------------------
// Assembly trampolines
// ---------------------------------------------------------------------------
//
// On x86_64 System V ABI, the first 6 integer args go in:
//   rdi, rsi, rdx, rcx, r8, r9
//
// For printf(fmt, ...):  rdi=fmt, rsi..r9 = first 5 varargs
// For fprintf(stream, fmt, ...): rdi=stream, rsi=fmt, rdx..r9 = first 4 varargs
// For snprintf(buf, size, fmt, ...): rdi=buf, rsi=size, rdx=fmt, rcx..r9 = first 3 varargs
//
// We save up to 8 args (5 register + 3 stack) for printf, fewer for others.

#[cfg(target_os = "none")]
core::arch::global_asm!(
    // printf(fmt, ...) → _printf_impl(fmt, int_args, float_args)
    ".global printf",
    ".type printf, @function",
    "printf:",
    "push rbp",
    "mov rbp, rsp",
    "sub rsp, 128",          // 64 bytes int args + 64 bytes float args
    // Save integer varargs (rsi-r9 = 5, plus 3 from stack = 8).
    "mov [rsp], rsi",        // int vararg 0
    "mov [rsp+8], rdx",      // int vararg 1
    "mov [rsp+16], rcx",     // int vararg 2
    "mov [rsp+24], r8",      // int vararg 3
    "mov [rsp+32], r9",      // int vararg 4
    "mov rax, [rbp+16]",     // int vararg 5 (stack)
    "mov [rsp+40], rax",
    "mov rax, [rbp+24]",     // int vararg 6
    "mov [rsp+48], rax",
    "mov rax, [rbp+32]",     // int vararg 7
    "mov [rsp+56], rax",
    // Save XMM varargs (xmm0-xmm7 = 8 doubles).
    "movsd [rsp+64], xmm0",
    "movsd [rsp+72], xmm1",
    "movsd [rsp+80], xmm2",
    "movsd [rsp+88], xmm3",
    "movsd [rsp+96], xmm4",
    "movsd [rsp+104], xmm5",
    "movsd [rsp+112], xmm6",
    "movsd [rsp+120], xmm7",
    // rdi = fmt (already set)
    "mov rsi, rsp",          // int_args array
    "lea rdx, [rsp+64]",    // float_args array
    "call _printf_impl",
    "add rsp, 128",
    "pop rbp",
    "ret",

    // fprintf(stream, fmt, ...) → _fprintf_impl(stream, fmt, int_args, float_args)
    ".global fprintf",
    ".type fprintf, @function",
    "fprintf:",
    "push rbp",
    "mov rbp, rsp",
    "sub rsp, 128",
    "mov [rsp], rdx",        // int vararg 0
    "mov [rsp+8], rcx",      // int vararg 1
    "mov [rsp+16], r8",      // int vararg 2
    "mov [rsp+24], r9",      // int vararg 3
    "mov rax, [rbp+16]",     // int vararg 4 (stack)
    "mov [rsp+32], rax",
    "mov rax, [rbp+24]",     // int vararg 5
    "mov [rsp+40], rax",
    "mov rax, [rbp+32]",     // int vararg 6
    "mov [rsp+48], rax",
    "mov rax, [rbp+40]",     // int vararg 7
    "mov [rsp+56], rax",
    "movsd [rsp+64], xmm0",
    "movsd [rsp+72], xmm1",
    "movsd [rsp+80], xmm2",
    "movsd [rsp+88], xmm3",
    "movsd [rsp+96], xmm4",
    "movsd [rsp+104], xmm5",
    "movsd [rsp+112], xmm6",
    "movsd [rsp+120], xmm7",
    // rdi = stream, rsi = fmt (already set)
    "mov rdx, rsp",          // int_args array
    "lea rcx, [rsp+64]",    // float_args array
    "call _fprintf_impl",
    "add rsp, 128",
    "pop rbp",
    "ret",

    // dprintf(fd, fmt, ...) → _dprintf_impl(fd, fmt, int_args, float_args)
    ".global dprintf",
    ".type dprintf, @function",
    "dprintf:",
    "push rbp",
    "mov rbp, rsp",
    "sub rsp, 128",
    "mov [rsp], rdx",        // int vararg 0
    "mov [rsp+8], rcx",      // int vararg 1
    "mov [rsp+16], r8",      // int vararg 2
    "mov [rsp+24], r9",      // int vararg 3
    "mov rax, [rbp+16]",     // int vararg 4 (stack)
    "mov [rsp+32], rax",
    "mov rax, [rbp+24]",     // int vararg 5
    "mov [rsp+40], rax",
    "mov rax, [rbp+32]",     // int vararg 6
    "mov [rsp+48], rax",
    "mov rax, [rbp+40]",     // int vararg 7
    "mov [rsp+56], rax",
    "movsd [rsp+64], xmm0",
    "movsd [rsp+72], xmm1",
    "movsd [rsp+80], xmm2",
    "movsd [rsp+88], xmm3",
    "movsd [rsp+96], xmm4",
    "movsd [rsp+104], xmm5",
    "movsd [rsp+112], xmm6",
    "movsd [rsp+120], xmm7",
    // rdi = fd, rsi = fmt (already set)
    "mov rdx, rsp",          // int_args array
    "lea rcx, [rsp+64]",    // float_args array
    "call _dprintf_impl",
    "add rsp, 128",
    "pop rbp",
    "ret",

    // snprintf(buf, size, fmt, ...) → _snprintf_impl(buf, size, fmt, int_args, float_args)
    ".global snprintf",
    ".type snprintf, @function",
    "snprintf:",
    "push rbp",
    "mov rbp, rsp",
    "sub rsp, 128",
    "mov [rsp], rcx",        // int vararg 0
    "mov [rsp+8], r8",       // int vararg 1
    "mov [rsp+16], r9",      // int vararg 2
    "mov rax, [rbp+16]",     // int vararg 3 (stack)
    "mov [rsp+24], rax",
    "mov rax, [rbp+24]",     // int vararg 4
    "mov [rsp+32], rax",
    "mov rax, [rbp+32]",     // int vararg 5
    "mov [rsp+40], rax",
    "mov rax, [rbp+40]",     // int vararg 6
    "mov [rsp+48], rax",
    "mov rax, [rbp+48]",     // int vararg 7
    "mov [rsp+56], rax",
    "movsd [rsp+64], xmm0",
    "movsd [rsp+72], xmm1",
    "movsd [rsp+80], xmm2",
    "movsd [rsp+88], xmm3",
    "movsd [rsp+96], xmm4",
    "movsd [rsp+104], xmm5",
    "movsd [rsp+112], xmm6",
    "movsd [rsp+120], xmm7",
    // rdi = buf, rsi = size, rdx = fmt (already set)
    "mov rcx, rsp",          // int_args array
    "lea r8, [rsp+64]",     // float_args array
    "call _snprintf_impl",
    "add rsp, 128",
    "pop rbp",
    "ret",

    // sprintf(buf, fmt, ...) → _sprintf_impl(buf, fmt, int_args, float_args)
    ".global sprintf",
    ".type sprintf, @function",
    "sprintf:",
    "push rbp",
    "mov rbp, rsp",
    "sub rsp, 128",
    "mov [rsp], rdx",        // int vararg 0
    "mov [rsp+8], rcx",      // int vararg 1
    "mov [rsp+16], r8",      // int vararg 2
    "mov [rsp+24], r9",      // int vararg 3
    "mov rax, [rbp+16]",     // int vararg 4 (stack)
    "mov [rsp+32], rax",
    "mov rax, [rbp+24]",     // int vararg 5
    "mov [rsp+40], rax",
    "mov rax, [rbp+32]",     // int vararg 6
    "mov [rsp+48], rax",
    "mov rax, [rbp+40]",     // int vararg 7
    "mov [rsp+56], rax",
    "movsd [rsp+64], xmm0",
    "movsd [rsp+72], xmm1",
    "movsd [rsp+80], xmm2",
    "movsd [rsp+88], xmm3",
    "movsd [rsp+96], xmm4",
    "movsd [rsp+104], xmm5",
    "movsd [rsp+112], xmm6",
    "movsd [rsp+120], xmm7",
    // rdi = buf, rsi = fmt (already set)
    "mov rdx, rsp",          // int_args array
    "lea rcx, [rsp+64]",    // float_args array
    "call _sprintf_impl",
    "add rsp, 128",
    "pop rbp",
    "ret",

    // asprintf(strp, fmt, ...) → _asprintf_impl(strp, fmt, int_args, float_args)
    // Same register layout as fprintf: 2 fixed args (strp, fmt), rest varargs.
    ".global asprintf",
    ".type asprintf, @function",
    "asprintf:",
    "push rbp",
    "mov rbp, rsp",
    "sub rsp, 128",
    "mov [rsp], rdx",        // int vararg 0
    "mov [rsp+8], rcx",      // int vararg 1
    "mov [rsp+16], r8",      // int vararg 2
    "mov [rsp+24], r9",      // int vararg 3
    "mov rax, [rbp+16]",     // int vararg 4 (stack)
    "mov [rsp+32], rax",
    "mov rax, [rbp+24]",     // int vararg 5
    "mov [rsp+40], rax",
    "mov rax, [rbp+32]",     // int vararg 6
    "mov [rsp+48], rax",
    "mov rax, [rbp+40]",     // int vararg 7
    "mov [rsp+56], rax",
    "movsd [rsp+64], xmm0",
    "movsd [rsp+72], xmm1",
    "movsd [rsp+80], xmm2",
    "movsd [rsp+88], xmm3",
    "movsd [rsp+96], xmm4",
    "movsd [rsp+104], xmm5",
    "movsd [rsp+112], xmm6",
    "movsd [rsp+120], xmm7",
    // rdi = strp, rsi = fmt (already set)
    "mov rdx, rsp",          // int_args array
    "lea rcx, [rsp+64]",    // float_args array
    "call _asprintf_impl",
    "add rsp, 128",
    "pop rbp",
    "ret",
);

// ---------------------------------------------------------------------------
// Rust entry points (called by assembly)
// ---------------------------------------------------------------------------

/// Stack buffer size for printf/fprintf (format to buffer, then write).
const PRINTF_BUF_SIZE: usize = 4096;

/// `printf(fmt, ...)` — write formatted output to stdout.
///
/// Output goes through the stdio buffer (line-buffered on stdout) so
/// printf output is properly coalesced with other stdout writes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn _printf_impl(fmt: *const u8, args: *const u64, fargs: *const u64) -> i32 {
    let mut buf = [0u8; PRINTF_BUF_SIZE];
    let n = format_core(buf.as_mut_ptr(), PRINTF_BUF_SIZE, fmt, args, fargs);
    if n <= 0 {
        return n;
    }
    let write_len = if (n as usize) < PRINTF_BUF_SIZE { n as usize } else { PRINTF_BUF_SIZE };
    let ret = crate::stdio::write_stream(core::ptr::dangling_mut::<u8>(), buf.as_ptr(), write_len);
    if ret < 0 { ret as i32 } else { n }
}

/// `fprintf(stream, fmt, ...)` — write formatted output to a stream.
///
/// Output goes through the stdio buffer so fprintf output is properly
/// coalesced with other writes to the same stream.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn _fprintf_impl(stream: *mut u8, fmt: *const u8, args: *const u64, fargs: *const u64) -> i32 {
    let mut buf = [0u8; PRINTF_BUF_SIZE];
    let n = format_core(buf.as_mut_ptr(), PRINTF_BUF_SIZE, fmt, args, fargs);
    if n <= 0 {
        return n;
    }
    let write_len = if (n as usize) < PRINTF_BUF_SIZE { n as usize } else { PRINTF_BUF_SIZE };
    let ret = crate::stdio::write_stream(stream, buf.as_ptr(), write_len);
    if ret < 0 { ret as i32 } else { n }
}

/// `dprintf(fd, fmt, ...)` — write formatted output to a file descriptor.
///
/// Like `fprintf` but takes a raw fd (int) instead of a `FILE*`.
/// Writes directly to the fd without stdio buffering.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn _dprintf_impl(fd: i32, fmt: *const u8, args: *const u64, fargs: *const u64) -> i32 {
    let mut buf = [0u8; PRINTF_BUF_SIZE];
    let n = format_core(buf.as_mut_ptr(), PRINTF_BUF_SIZE, fmt, args, fargs);
    if n <= 0 {
        return n;
    }
    let write_len = if (n as usize) < PRINTF_BUF_SIZE { n as usize } else { PRINTF_BUF_SIZE };
    let ret = crate::file::write(fd, buf.as_ptr(), write_len);
    if ret < 0 { ret as i32 } else { n }
}

/// `snprintf(buf, size, fmt, ...)` — write formatted output to a buffer.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn _snprintf_impl(
    buf: *mut u8,
    size: usize,
    fmt: *const u8,
    args: *const u64,
    fargs: *const u64,
) -> i32 {
    if buf.is_null() || size == 0 {
        // Still count characters.
        return format_core(core::ptr::null_mut(), 0, fmt, args, fargs);
    }
    let n = format_core(buf, size, fmt, args, fargs);
    // Null-terminate (snprintf guarantees this if size > 0).
    let term_pos = if n >= 0 && (n as usize) < size {
        n as usize
    } else {
        size.wrapping_sub(1)
    };
    // SAFETY: term_pos < size.
    unsafe { *buf.add(term_pos) = 0; }
    n
}

/// `sprintf(buf, fmt, ...)` — write formatted output to a buffer (no limit).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn _sprintf_impl(buf: *mut u8, fmt: *const u8, args: *const u64, fargs: *const u64) -> i32 {
    // No size limit — dangerous but matches C semantics.
    let n = format_core(buf, usize::MAX, fmt, args, fargs);
    // C99: "A null character is written at the end of the characters written."
    if !buf.is_null() && n >= 0 {
        // SAFETY: format_core wrote n bytes starting at buf; caller must provide
        // a buffer large enough for the output + null terminator.
        unsafe { *buf.add(n as usize) = 0; }
    }
    n
}

/// `asprintf(strp, fmt, ...)` — allocate and format a string.
///
/// Allocates a buffer large enough to hold the formatted output
/// (including null terminator) and stores a pointer to it in `*strp`.
/// Returns the number of characters written (excluding null), or -1
/// on allocation failure.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn _asprintf_impl(
    strp: *mut *mut u8,
    fmt: *const u8,
    args: *const u64,
    fargs: *const u64,
) -> i32 {
    if strp.is_null() {
        return -1;
    }

    // First pass: count required bytes (format to null buffer).
    let n = format_core(core::ptr::null_mut(), 0, fmt, args, fargs);
    if n < 0 {
        unsafe { *strp = core::ptr::null_mut(); }
        return -1;
    }

    let alloc_size = (n as usize).wrapping_add(1); // +1 for NUL
    let buf = crate::malloc::malloc(alloc_size);
    if buf.is_null() {
        unsafe { *strp = core::ptr::null_mut(); }
        return -1;
    }

    // Second pass: format into the allocated buffer.
    let written = format_core(buf, alloc_size, fmt, args, fargs);

    // Null-terminate.
    let term_pos = if written >= 0 && (written as usize) < alloc_size {
        written as usize
    } else {
        alloc_size.wrapping_sub(1)
    };
    // SAFETY: term_pos < alloc_size, buf is non-null.
    unsafe { *buf.add(term_pos) = 0; }

    // SAFETY: strp verified non-null.
    unsafe { *strp = buf; }
    n
}

// ---------------------------------------------------------------------------
// Core formatting engine
// ---------------------------------------------------------------------------

/// Maximum digits in any formatted number (u64 in octal = 22 digits + sign + prefix).
const NUM_BUF_SIZE: usize = 32;

/// Output destination for the formatting engine.
///
/// Bundles buffer pointer, size, and write position so they don't need
/// to be threaded through every function individually.
struct FmtOutput {
    buf: *mut u8,
    size: usize,
    pos: usize,
}

impl FmtOutput {
    const fn new(buf: *mut u8, size: usize) -> Self {
        Self { buf, size, pos: 0 }
    }
}

/// Parsed format specifier state.
struct FormatSpec {
    flags: FormatFlags,
    width: usize,
    precision: Option<usize>,
}

/// Parse flags, width, precision, and length modifier from a format string.
///
/// `fpos` points past the initial '%'.  On return, `fpos` points to
/// the conversion character (d, s, x, etc.).  Width/precision `*`
/// arguments are consumed from `args`.
fn parse_spec(
    fmt: *const u8,
    fpos: &mut usize,
    args: *const u64,
    arg_idx: &mut usize,
) -> FormatSpec {
    let mut flags = FormatFlags::new();

    // Flags.
    loop {
        match unsafe { *fmt.add(*fpos) } {
            b'-' => flags.left_align = true,
            b'+' => flags.force_sign = true,
            b' ' => flags.space_sign = true,
            b'0' => flags.zero_pad = true,
            b'#' => flags.alt_form = true,
            _ => break,
        }
        *fpos = fpos.wrapping_add(1);
    }

    // Width.
    let mut width: usize = 0;
    if unsafe { *fmt.add(*fpos) } == b'*' {
        let w = consume_arg(args, arg_idx) as i64;
        if w < 0 {
            flags.left_align = true;
            width = (w.wrapping_neg()) as usize;
        } else {
            width = w as usize;
        }
        *fpos = fpos.wrapping_add(1);
    } else {
        while unsafe { *fmt.add(*fpos) }.is_ascii_digit() {
            width = width.wrapping_mul(10).wrapping_add(
                (unsafe { *fmt.add(*fpos) }.wrapping_sub(b'0')) as usize,
            );
            *fpos = fpos.wrapping_add(1);
        }
    }

    // Precision.
    let mut precision: Option<usize> = None;
    if unsafe { *fmt.add(*fpos) } == b'.' {
        *fpos = fpos.wrapping_add(1);
        if unsafe { *fmt.add(*fpos) } == b'*' {
            let p = consume_arg(args, arg_idx) as i32;
            if p >= 0 {
                precision = Some(p as usize);
            }
            *fpos = fpos.wrapping_add(1);
        } else {
            let mut p: usize = 0;
            while unsafe { *fmt.add(*fpos) }.is_ascii_digit() {
                p = p.wrapping_mul(10).wrapping_add(
                    (unsafe { *fmt.add(*fpos) }.wrapping_sub(b'0')) as usize,
                );
                *fpos = fpos.wrapping_add(1);
            }
            precision = Some(p);
        }
    }

    // Length modifier (ignored on LP64 — all int types are 8 bytes in args).
    match unsafe { *fmt.add(*fpos) } {
        b'l' => {
            *fpos = fpos.wrapping_add(1);
            if unsafe { *fmt.add(*fpos) } == b'l' {
                *fpos = fpos.wrapping_add(1);
            }
        }
        b'h' => {
            *fpos = fpos.wrapping_add(1);
            if unsafe { *fmt.add(*fpos) } == b'h' {
                *fpos = fpos.wrapping_add(1);
            }
        }
        b'z' | b'j' | b't' => {
            *fpos = fpos.wrapping_add(1);
        }
        _ => {}
    }

    FormatSpec { flags, width, precision }
}

/// Dispatch a single conversion specifier.
///
/// Returns the updated `fpos` (past the specifier character).
#[allow(clippy::too_many_arguments)]
fn dispatch_spec(
    dst: &mut FmtOutput,
    fmt: *const u8,
    fpos: usize,
    spec_start: usize,
    spec: &FormatSpec,
    args: *const u64,
    arg_idx: &mut usize,
    fargs: *const u64,
    farg_idx: &mut usize,
) -> usize {
    let ch = unsafe { *fmt.add(fpos) };
    let next = fpos.wrapping_add(1);

    match ch {
        b'%' => emit_byte(dst, b'%'),

        b'd' | b'i' => {
            let val = consume_arg(args, arg_idx) as i64;
            format_signed(dst, val, &spec.flags, spec.width, spec.precision);
        }

        b'u' => {
            let val = consume_arg(args, arg_idx);
            format_unsigned(dst, val, 10, false, &spec.flags, spec.width, spec.precision);
        }

        b'x' => {
            let val = consume_arg(args, arg_idx);
            format_unsigned(dst, val, 16, false, &spec.flags, spec.width, spec.precision);
        }

        b'X' => {
            let val = consume_arg(args, arg_idx);
            format_unsigned(dst, val, 16, true, &spec.flags, spec.width, spec.precision);
        }

        b'o' => {
            let val = consume_arg(args, arg_idx);
            format_unsigned(dst, val, 8, false, &spec.flags, spec.width, spec.precision);
        }

        b's' => {
            let ptr = consume_arg(args, arg_idx) as *const u8;
            format_string(dst, ptr, &spec.flags, spec.width, spec.precision);
        }

        b'c' => {
            let ch_val = consume_arg(args, arg_idx) as u8;
            if spec.width > 1 && !spec.flags.left_align {
                emit_padding(dst, b' ', spec.width.wrapping_sub(1));
            }
            emit_byte(dst, ch_val);
            if spec.width > 1 && spec.flags.left_align {
                emit_padding(dst, b' ', spec.width.wrapping_sub(1));
            }
        }

        b'p' => {
            let val = consume_arg(args, arg_idx);
            // Count hex digits to determine total output width for padding.
            let hex_len = if val == 0 {
                1usize
            } else {
                let mut n = val;
                let mut count = 0usize;
                while n > 0 {
                    count = count.wrapping_add(1);
                    n = n.wrapping_shr(4);
                }
                count
            };
            let total = 2usize.wrapping_add(hex_len); // "0x" + digits

            // Right-justify padding.
            if !spec.flags.left_align && spec.width > total {
                emit_padding(dst, b' ', spec.width.wrapping_sub(total));
            }
            emit_byte(dst, b'0');
            emit_byte(dst, b'x');
            // Emit hex digits without additional padding (handled here).
            format_unsigned(dst, val, 16, false, &FormatFlags::new(), 0, None);
            // Left-justify padding.
            if spec.flags.left_align && spec.width > total {
                emit_padding(dst, b' ', spec.width.wrapping_sub(total));
            }
        }

        b'n' => {
            let ptr = consume_arg(args, arg_idx) as *mut i32;
            if !ptr.is_null() {
                // SAFETY: Caller guarantees ptr is valid.
                unsafe { *ptr = dst.pos as i32; }
            }
        }

        // Floating-point specifiers: consume from the float args array.
        b'f' | b'F' => {
            let bits = consume_arg(fargs, farg_idx);
            let val = f64::from_bits(bits);
            let prec = spec.precision.unwrap_or(6);
            format_float_fixed(dst, val, ch == b'F', &spec.flags, spec.width, prec);
        }

        b'e' | b'E' => {
            let bits = consume_arg(fargs, farg_idx);
            let val = f64::from_bits(bits);
            let prec = spec.precision.unwrap_or(6);
            format_float_sci(dst, val, ch == b'E', &spec.flags, spec.width, prec);
        }

        b'g' | b'G' => {
            let bits = consume_arg(fargs, farg_idx);
            let val = f64::from_bits(bits);
            let prec = if spec.precision == Some(0) { 1 } else { spec.precision.unwrap_or(6) };
            format_float_general(dst, val, ch == b'G', &spec.flags, spec.width, prec);
        }

        _ => {
            // Unknown specifier or premature end — emit raw.
            emit_byte(dst, b'%');
            let mut re = spec_start;
            while re < next {
                emit_byte(dst, unsafe { *fmt.add(re) });
                re = re.wrapping_add(1);
            }
        }
    }

    next
}

/// Format a printf-style string into a buffer.
///
/// Returns the number of characters that would have been written
/// (not counting null), even if `out_size` was too small (snprintf
/// semantics).
fn format_core(
    out: *mut u8,
    out_size: usize,
    fmt: *const u8,
    args: *const u64,
    fargs: *const u64,
) -> i32 {
    if fmt.is_null() {
        return -1;
    }

    let mut dst = FmtOutput::new(out, out_size);
    let mut arg_idx: usize = 0;
    let mut farg_idx: usize = 0;
    let mut fpos: usize = 0;

    loop {
        let ch = unsafe { *fmt.add(fpos) };
        if ch == 0 {
            break;
        }

        if ch != b'%' {
            emit_byte(&mut dst, ch);
            fpos = fpos.wrapping_add(1);
            continue;
        }

        fpos = fpos.wrapping_add(1); // skip '%'

        // Handle premature end.
        if unsafe { *fmt.add(fpos) } == 0 {
            break;
        }

        let spec_start = fpos;
        let spec = parse_spec(fmt, &mut fpos, args, &mut arg_idx);
        fpos = dispatch_spec(&mut dst, fmt, fpos, spec_start, &spec, args, &mut arg_idx, fargs, &mut farg_idx);
    }

    dst.pos as i32
}

// ---------------------------------------------------------------------------
// Format helpers
// ---------------------------------------------------------------------------

// Printf flags are inherently boolean — each is an independent on/off switch
// matching the C standard's format flag characters (-, 0, +, space, #).
#[allow(clippy::struct_excessive_bools)]
struct FormatFlags {
    left_align: bool,
    zero_pad: bool,
    force_sign: bool,
    space_sign: bool,
    alt_form: bool,
}

impl FormatFlags {
    const fn new() -> Self {
        Self {
            left_align: false,
            zero_pad: false,
            force_sign: false,
            space_sign: false,
            alt_form: false,
        }
    }
}

/// Consume the next argument from the args array.
fn consume_arg(args: *const u64, idx: &mut usize) -> u64 {
    if args.is_null() {
        return 0;
    }
    let val = unsafe { *args.add(*idx) };
    *idx = idx.wrapping_add(1);
    val
}

/// Emit a single byte to the output buffer.
fn emit_byte(dst: &mut FmtOutput, byte: u8) {
    if !dst.buf.is_null() && dst.pos < dst.size {
        // SAFETY: dst.pos < dst.size, so buf.add(dst.pos) is valid.
        unsafe { *dst.buf.add(dst.pos) = byte; }
    }
    dst.pos = dst.pos.wrapping_add(1);
}

/// Emit `count` copies of `pad_char`.
fn emit_padding(dst: &mut FmtOutput, pad_char: u8, count: usize) {
    let mut i: usize = 0;
    while i < count {
        emit_byte(dst, pad_char);
        i = i.wrapping_add(1);
    }
}

/// Emit a byte slice.
fn emit_bytes(dst: &mut FmtOutput, data: &[u8]) {
    for &b in data {
        emit_byte(dst, b);
    }
}

/// Format a signed integer (%d, %i).
fn format_signed(
    dst: &mut FmtOutput,
    val: i64,
    flags: &FormatFlags,
    width: usize,
    precision: Option<usize>,
) {
    let negative = val < 0;
    let abs_val = if negative { val.wrapping_neg() as u64 } else { val as u64 };

    // Convert to digits.
    let mut num_buf = [0u8; NUM_BUF_SIZE];
    let mut num_len = u64_to_dec(abs_val, &mut num_buf);
    // POSIX/C99: precision 0 with value 0 produces no digit output.
    if precision == Some(0) && abs_val == 0 {
        num_len = 0;
    }
    let digits = if let Some(p) = precision {
        if p > num_len { p } else { num_len }
    } else {
        num_len
    };

    // Sign character.
    let sign: Option<u8> = if negative {
        Some(b'-')
    } else if flags.force_sign {
        Some(b'+')
    } else if flags.space_sign {
        Some(b' ')
    } else {
        None
    };

    let sign_len: usize = usize::from(sign.is_some());
    let total_len = sign_len.wrapping_add(digits);

    let pad_char = if flags.zero_pad && !flags.left_align && precision.is_none() {
        b'0'
    } else {
        b' '
    };

    // Right-justify padding (before sign if space-padded, after sign if zero-padded).
    if !flags.left_align && width > total_len && pad_char == b' ' {
        emit_padding(dst, b' ', width.wrapping_sub(total_len));
    }

    // Sign.
    if let Some(s) = sign {
        emit_byte(dst, s);
    }

    // Zero padding (after sign, before digits).
    if !flags.left_align && width > total_len && pad_char == b'0' {
        emit_padding(dst, b'0', width.wrapping_sub(total_len));
    }

    // Precision zero-padding.
    if let Some(p) = precision
        && p > num_len
    {
        emit_padding(dst, b'0', p.wrapping_sub(num_len));
    }

    // Digits.
    let start = NUM_BUF_SIZE.wrapping_sub(num_len);
    if let Some(slice) = num_buf.get(start..) {
        emit_bytes(dst, slice);
    }

    // Left-justify padding.
    if flags.left_align && width > total_len {
        emit_padding(dst, b' ', width.wrapping_sub(total_len));
    }
}

/// Format an unsigned integer (%u, %x, %X, %o).
#[allow(clippy::too_many_arguments)]
fn format_unsigned(
    dst: &mut FmtOutput,
    val: u64,
    base: u32,
    upper: bool,
    flags: &FormatFlags,
    width: usize,
    precision: Option<usize>,
) {
    let mut num_buf = [0u8; NUM_BUF_SIZE];
    let mut num_len = u64_to_base(val, base, upper, &mut num_buf);
    // POSIX/C99: precision 0 with value 0 produces no digit output.
    if precision == Some(0) && val == 0 {
        num_len = 0;
    }
    let digits = if let Some(p) = precision {
        if p > num_len { p } else { num_len }
    } else {
        num_len
    };

    // Alt form prefix.
    let prefix_len: usize = if flags.alt_form && val != 0 {
        match base {
            16 => 2, // "0x" or "0X"
            8 => 1,  // "0"
            _ => 0,
        }
    } else {
        0
    };

    let total_len = prefix_len.wrapping_add(digits);
    let pad_char = if flags.zero_pad && !flags.left_align && precision.is_none() {
        b'0'
    } else {
        b' '
    };

    // Right-justify space padding.
    if !flags.left_align && width > total_len && pad_char == b' ' {
        emit_padding(dst, b' ', width.wrapping_sub(total_len));
    }

    // Prefix.
    if prefix_len == 2 {
        emit_byte(dst, b'0');
        emit_byte(dst, if upper { b'X' } else { b'x' });
    } else if prefix_len == 1 {
        emit_byte(dst, b'0');
    }

    // Right-justify zero padding.
    if !flags.left_align && width > total_len && pad_char == b'0' {
        emit_padding(dst, b'0', width.wrapping_sub(total_len));
    }

    // Precision zero-padding.
    if let Some(p) = precision
        && p > num_len
    {
        emit_padding(dst, b'0', p.wrapping_sub(num_len));
    }

    // Digits.
    let start = NUM_BUF_SIZE.wrapping_sub(num_len);
    if let Some(slice) = num_buf.get(start..) {
        emit_bytes(dst, slice);
    }

    // Left-justify padding.
    if flags.left_align && width > total_len {
        emit_padding(dst, b' ', width.wrapping_sub(total_len));
    }
}

/// Format a string (%s).
fn format_string(
    dst: &mut FmtOutput,
    s: *const u8,
    flags: &FormatFlags,
    width: usize,
    precision: Option<usize>,
) {
    if s.is_null() {
        // glibc prints "(null)" for NULL, respecting width and precision.
        let null_str: &[u8] = b"(null)";
        let len = if let Some(p) = precision {
            if p < 6 { p } else { 6 }
        } else {
            6
        };
        if !flags.left_align && width > len {
            emit_padding(dst, b' ', width.wrapping_sub(len));
        }
        if let Some(slice) = null_str.get(..len) {
            emit_bytes(dst, slice);
        }
        if flags.left_align && width > len {
            emit_padding(dst, b' ', width.wrapping_sub(len));
        }
        return;
    }

    // Determine string length (respecting precision as max).
    let mut len: usize = 0;
    let max_len = precision.unwrap_or(usize::MAX);
    while len < max_len && unsafe { *s.add(len) } != 0 {
        len = len.wrapping_add(1);
    }

    // Right-justify padding.
    if !flags.left_align && width > len {
        emit_padding(dst, b' ', width.wrapping_sub(len));
    }

    // String content.
    let mut i: usize = 0;
    while i < len {
        emit_byte(dst, unsafe { *s.add(i) });
        i = i.wrapping_add(1);
    }

    // Left-justify padding.
    if flags.left_align && width > len {
        emit_padding(dst, b' ', width.wrapping_sub(len));
    }
}

// ---------------------------------------------------------------------------
// Number conversion
// ---------------------------------------------------------------------------

/// Convert a u64 to decimal digits in a buffer (right-aligned).
///
/// Returns the number of digits written.
fn u64_to_dec(mut val: u64, buf: &mut [u8; NUM_BUF_SIZE]) -> usize {
    if val == 0 {
        if let Some(slot) = buf.get_mut(NUM_BUF_SIZE.wrapping_sub(1)) {
            *slot = b'0';
        }
        return 1;
    }

    let mut pos = NUM_BUF_SIZE;
    while val > 0 && pos > 0 {
        pos = pos.wrapping_sub(1);
        if let Some(slot) = buf.get_mut(pos) {
            #[allow(clippy::arithmetic_side_effects)]
            { *slot = b'0'.wrapping_add((val % 10) as u8); }
        }
        val = val.wrapping_div(10);
    }

    NUM_BUF_SIZE.wrapping_sub(pos)
}

/// Convert a u64 to digits in a given base (right-aligned in buffer).
fn u64_to_base(mut val: u64, base: u32, upper: bool, buf: &mut [u8; NUM_BUF_SIZE]) -> usize {
    if val == 0 {
        if let Some(slot) = buf.get_mut(NUM_BUF_SIZE.wrapping_sub(1)) {
            *slot = b'0';
        }
        return 1;
    }

    let digits = if upper {
        b"0123456789ABCDEF"
    } else {
        b"0123456789abcdef"
    };

    let base_u64 = u64::from(base);
    let mut pos = NUM_BUF_SIZE;
    while val > 0 && pos > 0 {
        pos = pos.wrapping_sub(1);
        #[allow(clippy::arithmetic_side_effects)]
        let digit_idx = (val % base_u64) as usize;
        if let (Some(slot), Some(&d)) = (buf.get_mut(pos), digits.get(digit_idx)) {
            *slot = d;
        }
        #[allow(clippy::arithmetic_side_effects)]
        // base_u64 is always >= 2 (only called with base 8/10/16), so no divide-by-zero.
        { val = val.wrapping_div(base_u64); }
    }

    NUM_BUF_SIZE.wrapping_sub(pos)
}

// ---------------------------------------------------------------------------
// Floating-point formatting
// ---------------------------------------------------------------------------

/// Format a floating-point value in fixed notation (%f/%F).
#[allow(clippy::arithmetic_side_effects, clippy::too_many_arguments)]
fn format_float_fixed(
    dst: &mut FmtOutput,
    val: f64,
    upper: bool,
    flags: &FormatFlags,
    width: usize,
    precision: usize,
) {
    // Handle special values.
    if val.is_nan() {
        let s = if upper { b"NAN" } else { b"nan" };
        format_float_special(dst, s, false, flags, width);
        return;
    }
    let negative = val.is_sign_negative();
    if val.is_infinite() {
        let s = if upper { b"INF" } else { b"inf" };
        format_float_special(dst, s, negative, flags, width);
        return;
    }

    let abs_val = if negative { -val } else { val };

    // Format into a temporary buffer.
    let mut buf = [0u8; 350]; // Enough for DBL_MAX (~308 digits) + decimal + precision
    let mut len = fmt_fixed(abs_val, precision, &mut buf);

    // C99 '#' flag: always include a decimal point, even when precision is 0.
    if flags.alt_form && precision == 0 {
        if let Some(slot) = buf.get_mut(len) {
            *slot = b'.';
        }
        len = len.wrapping_add(1);
    }

    emit_float_padded(dst, &buf, len, negative, flags, width);
}

/// Format a floating-point value in scientific notation (%e/%E).
#[allow(clippy::arithmetic_side_effects, clippy::too_many_arguments)]
fn format_float_sci(
    dst: &mut FmtOutput,
    val: f64,
    upper: bool,
    flags: &FormatFlags,
    width: usize,
    precision: usize,
) {
    if val.is_nan() {
        let s = if upper { b"NAN" } else { b"nan" };
        format_float_special(dst, s, false, flags, width);
        return;
    }
    let negative = val.is_sign_negative();
    if val.is_infinite() {
        let s = if upper { b"INF" } else { b"inf" };
        format_float_special(dst, s, negative, flags, width);
        return;
    }

    let abs_val = if negative { -val } else { val };

    let mut buf = [0u8; 350];
    let mut len = fmt_scientific(abs_val, precision, upper, &mut buf);

    // C99 '#' flag: always include a decimal point, even when precision is 0.
    // Insert '.' before the 'e'/'E' exponent marker.
    if flags.alt_form && precision == 0 {
        // Find 'e'/'E' position.
        let mut epos = 0;
        while epos < len {
            if let Some(&b) = buf.get(epos)
                && (b == b'e' || b == b'E') { break; }
            epos = epos.wrapping_add(1);
        }
        if epos < len {
            // Shift exponent part right by 1 to make room for '.'.
            let mut j = len;
            while j > epos {
                if let (Some(src), Some(dst_slot)) = (buf.get(j.wrapping_sub(1)).copied(), buf.get_mut(j)) {
                    *dst_slot = src;
                }
                j = j.wrapping_sub(1);
            }
            if let Some(slot) = buf.get_mut(epos) {
                *slot = b'.';
            }
            len = len.wrapping_add(1);
        }
    }

    emit_float_padded(dst, &buf, len, negative, flags, width);
}

/// Format a floating-point value in %g/%G notation.
///
/// Uses %e if exponent < -4 or >= precision, else %f.
/// Trailing zeros are removed unless `#` flag is set.
#[allow(clippy::arithmetic_side_effects, clippy::too_many_arguments)]
fn format_float_general(
    dst: &mut FmtOutput,
    val: f64,
    upper: bool,
    flags: &FormatFlags,
    width: usize,
    precision: usize,
) {
    if val.is_nan() {
        let s = if upper { b"NAN" } else { b"nan" };
        format_float_special(dst, s, false, flags, width);
        return;
    }
    let negative = val.is_sign_negative();
    if val.is_infinite() {
        let s = if upper { b"INF" } else { b"inf" };
        format_float_special(dst, s, negative, flags, width);
        return;
    }

    let abs_val = if negative { -val } else { val };

    // Determine decimal exponent X (what %e conversion would produce).
    // C99: use floor(log10(|val|)).  ilogb gives the binary exponent
    // which is wrong here — e.g. ilogb(9.5)=3 but the decimal exp is 0.
    let exp = if abs_val == 0.0 {
        0
    } else {
        crate::math::floor(crate::math::log10(abs_val)) as i32
    };
    let p = precision as i32;

    let mut buf = [0u8; 350];
    let mut len;

    if exp < -4 || exp >= p {
        // Use scientific, but with (precision - 1) digits after '.'.
        let sci_prec = if precision > 0 { precision.wrapping_sub(1) } else { 0 };
        len = fmt_scientific(abs_val, sci_prec, upper, &mut buf);
    } else {
        // Use fixed, with (precision - 1 - exp) digits after '.'.
        let fix_prec = if (p - 1 - exp) > 0 { (p - 1 - exp) as usize } else { 0 };
        len = fmt_fixed(abs_val, fix_prec, &mut buf);
    }

    // Remove trailing zeros (unless # flag).
    if !flags.alt_form {
        len = trim_trailing_zeros(&mut buf, len);
    }

    // C99 '#' flag for %g: always include a decimal point.
    // When precision is low enough that the computed sub-precision is 0,
    // fmt_fixed / fmt_scientific won't emit a '.'.  Insert one if missing.
    if flags.alt_form {
        let has_dot = {
            let mut found = false;
            let mut k = 0;
            while k < len {
                if let Some(&b) = buf.get(k)
                    && b == b'.' { found = true; break; }
                k = k.wrapping_add(1);
            }
            found
        };
        if !has_dot {
            // Find 'e'/'E' (scientific) or end of buffer (fixed).
            let mut insert_at = len;
            let mut k = 0;
            while k < len {
                if let Some(&b) = buf.get(k)
                    && (b == b'e' || b == b'E') { insert_at = k; break; }
                k = k.wrapping_add(1);
            }
            // Shift [insert_at..len] right by 1.
            let mut j = len;
            while j > insert_at {
                if let (Some(src), Some(dst_slot)) = (buf.get(j.wrapping_sub(1)).copied(), buf.get_mut(j)) {
                    *dst_slot = src;
                }
                j = j.wrapping_sub(1);
            }
            if let Some(slot) = buf.get_mut(insert_at) {
                *slot = b'.';
            }
            len = len.wrapping_add(1);
        }
    }

    emit_float_padded(dst, &buf, len, negative, flags, width);
}

/// Emit special float strings (nan, inf) with sign and padding.
fn format_float_special(
    dst: &mut FmtOutput,
    text: &[u8],
    negative: bool,
    flags: &FormatFlags,
    width: usize,
) {
    let sign: Option<u8> = if negative {
        Some(b'-')
    } else if flags.force_sign {
        Some(b'+')
    } else if flags.space_sign {
        Some(b' ')
    } else {
        None
    };
    let total = text.len().wrapping_add(usize::from(sign.is_some()));

    if !flags.left_align && width > total {
        emit_padding(dst, b' ', width.wrapping_sub(total));
    }
    if let Some(s) = sign { emit_byte(dst, s); }
    emit_bytes(dst, text);
    if flags.left_align && width > total {
        emit_padding(dst, b' ', width.wrapping_sub(total));
    }
}

/// Emit a formatted float with sign, padding, and alignment.
#[allow(clippy::arithmetic_side_effects)]
fn emit_float_padded(
    dst: &mut FmtOutput,
    buf: &[u8],
    len: usize,
    negative: bool,
    flags: &FormatFlags,
    width: usize,
) {
    let sign: Option<u8> = if negative {
        Some(b'-')
    } else if flags.force_sign {
        Some(b'+')
    } else if flags.space_sign {
        Some(b' ')
    } else {
        None
    };
    let sign_len = usize::from(sign.is_some());
    let total = sign_len + len;

    let pad_char = if flags.zero_pad && !flags.left_align { b'0' } else { b' ' };

    if !flags.left_align && width > total && pad_char == b' ' {
        emit_padding(dst, b' ', width.wrapping_sub(total));
    }
    if let Some(s) = sign { emit_byte(dst, s); }
    if !flags.left_align && width > total && pad_char == b'0' {
        emit_padding(dst, b'0', width.wrapping_sub(total));
    }
    // Emit the formatted number from buf.
    let mut i = 0;
    while i < len {
        if let Some(&b) = buf.get(i) {
            emit_byte(dst, b);
        }
        i = i.wrapping_add(1);
    }
    if flags.left_align && width > total {
        emit_padding(dst, b' ', width.wrapping_sub(total));
    }
}

/// Format a non-negative f64 in fixed notation into buf.
/// Returns number of bytes written.
#[allow(clippy::arithmetic_side_effects, clippy::cast_precision_loss)]
fn fmt_fixed(val: f64, precision: usize, buf: &mut [u8]) -> usize {
    // When precision is 0, round the value first so that e.g.
    // printf("%.0f", 3.7) outputs "4" not "3".  The fractional-digit
    // loop handles rounding for precision > 0 via carry propagation.
    let val = if precision == 0 {
        crate::math::round(val)
    } else {
        val
    };

    // Separate integer and fractional parts.
    let int_part = val as u64;
    let frac = val - (int_part as f64);

    let mut pos: usize = 0;

    // Write integer part.
    if int_part == 0 {
        if let Some(slot) = buf.get_mut(pos) { *slot = b'0'; }
        pos = pos.wrapping_add(1);
    } else {
        // Write digits of integer part (reversed).
        let mut digits = [0u8; 20];
        let mut dlen: usize = 0;
        let mut n = int_part;
        while n > 0 {
            if let Some(slot) = digits.get_mut(dlen) {
                *slot = b'0'.wrapping_add((n % 10) as u8);
            }
            dlen = dlen.wrapping_add(1);
            n /= 10;
        }
        // Reverse into buf.
        let mut k = dlen;
        while k > 0 {
            k = k.wrapping_sub(1);
            if let (Some(slot), Some(&d)) = (buf.get_mut(pos), digits.get(k)) {
                *slot = d;
            }
            pos = pos.wrapping_add(1);
        }
    }

    // Decimal point and fractional part.
    if precision > 0 {
        if let Some(slot) = buf.get_mut(pos) { *slot = b'.'; }
        pos = pos.wrapping_add(1);

        let mut f = frac;
        let mut p = precision;
        while p > 0 {
            f *= 10.0;
            let digit = f as u8;
            if let Some(slot) = buf.get_mut(pos) {
                *slot = b'0'.wrapping_add(digit);
            }
            f -= f64::from(digit);
            pos = pos.wrapping_add(1);
            p = p.wrapping_sub(1);
        }

        // Round the last digit.
        if f >= 0.5 {
            // Propagate rounding.
            let mut rp = pos.wrapping_sub(1);
            loop {
                if let Some(slot) = buf.get_mut(rp) {
                    if *slot == b'.' {
                        if rp == 0 { break; }
                        rp = rp.wrapping_sub(1);
                        continue;
                    }
                    if *slot < b'9' {
                        *slot = slot.wrapping_add(1);
                        break;
                    }
                    *slot = b'0';
                }
                if rp == 0 {
                    // Need to insert a '1' at the front.  Shift everything right.
                    let mut j = pos;
                    while j > 0 {
                        if let (Some(src), Some(dst_slot)) = (buf.get(j.wrapping_sub(1)).copied(), buf.get_mut(j)) {
                            *dst_slot = src;
                        }
                        j = j.wrapping_sub(1);
                    }
                    if let Some(slot) = buf.get_mut(0) { *slot = b'1'; }
                    pos = pos.wrapping_add(1);
                    break;
                }
                rp = rp.wrapping_sub(1);
            }
        }
    }

    pos
}

/// Format a non-negative f64 in scientific notation into buf.
/// Returns number of bytes written.
#[allow(clippy::arithmetic_side_effects, clippy::cast_precision_loss)]
fn fmt_scientific(val: f64, precision: usize, upper: bool, buf: &mut [u8]) -> usize {
    if val == 0.0 {
        let mut pos: usize = 0;
        if let Some(slot) = buf.get_mut(pos) { *slot = b'0'; }
        pos = pos.wrapping_add(1);
        if precision > 0 {
            if let Some(slot) = buf.get_mut(pos) { *slot = b'.'; }
            pos = pos.wrapping_add(1);
            let mut p = precision;
            while p > 0 {
                if let Some(slot) = buf.get_mut(pos) { *slot = b'0'; }
                pos = pos.wrapping_add(1);
                p = p.wrapping_sub(1);
            }
        }
        if let Some(slot) = buf.get_mut(pos) { *slot = if upper { b'E' } else { b'e' }; }
        pos = pos.wrapping_add(1);
        if let Some(slot) = buf.get_mut(pos) { *slot = b'+'; }
        pos = pos.wrapping_add(1);
        if let Some(slot) = buf.get_mut(pos) { *slot = b'0'; }
        pos = pos.wrapping_add(1);
        if let Some(slot) = buf.get_mut(pos) { *slot = b'0'; }
        pos = pos.wrapping_add(1);
        return pos;
    }

    // Find decimal exponent: floor(log10(|val|)).
    // Previous code used ilogb (binary exponent) which is incorrect —
    // e.g. ilogb(9.5)=3 but the decimal exponent is 0.
    let mut exp: i32 = crate::math::floor(crate::math::log10(val)) as i32;
    let mut mantissa = val / crate::math::pow(10.0, f64::from(exp));
    // Normalize: 1 <= mantissa < 10 (handle floating-point imprecision).
    while mantissa >= 10.0 { mantissa /= 10.0; exp += 1; }
    while mantissa < 1.0 && mantissa > 0.0 { mantissa *= 10.0; exp -= 1; }

    // Pre-round mantissa to the requested precision and re-normalize.
    // Without this, rounding carry in fmt_fixed can push the integer part
    // to 10 (e.g. mantissa 9.95 with precision 1 → "10.0"), producing
    // invalid scientific notation like "10.0e+00" instead of "1.0e+01".
    let scale = crate::math::pow(10.0, precision as f64);
    mantissa = crate::math::round(mantissa * scale) / scale;
    if mantissa >= 10.0 {
        mantissa /= 10.0;
        exp += 1;
    }

    // Format mantissa as fixed point with `precision` decimal places.
    let mut pos = fmt_fixed(mantissa, precision, buf);

    // Exponent.
    if let Some(slot) = buf.get_mut(pos) { *slot = if upper { b'E' } else { b'e' }; }
    pos = pos.wrapping_add(1);
    if let Some(slot) = buf.get_mut(pos) { *slot = if exp < 0 { b'-' } else { b'+' }; }
    pos = pos.wrapping_add(1);

    let abs_exp = if exp < 0 { (-exp) as u32 } else { exp as u32 };
    // At least 2 digits for exponent.
    if abs_exp < 10 {
        if let Some(slot) = buf.get_mut(pos) { *slot = b'0'; }
        pos = pos.wrapping_add(1);
        if let Some(slot) = buf.get_mut(pos) { *slot = b'0'.wrapping_add(abs_exp as u8); }
        pos = pos.wrapping_add(1);
    } else if abs_exp < 100 {
        if let Some(slot) = buf.get_mut(pos) { *slot = b'0'.wrapping_add((abs_exp / 10) as u8); }
        pos = pos.wrapping_add(1);
        if let Some(slot) = buf.get_mut(pos) { *slot = b'0'.wrapping_add((abs_exp % 10) as u8); }
        pos = pos.wrapping_add(1);
    } else {
        // 3-digit exponent (values > 1e99).
        if let Some(slot) = buf.get_mut(pos) { *slot = b'0'.wrapping_add((abs_exp / 100) as u8); }
        pos = pos.wrapping_add(1);
        if let Some(slot) = buf.get_mut(pos) { *slot = b'0'.wrapping_add(((abs_exp / 10) % 10) as u8); }
        pos = pos.wrapping_add(1);
        if let Some(slot) = buf.get_mut(pos) { *slot = b'0'.wrapping_add((abs_exp % 10) as u8); }
        pos = pos.wrapping_add(1);
    }

    pos
}

/// Remove trailing zeros after the decimal point in a formatted float.
/// Also removes the '.' if no fractional digits remain.
/// Returns new length.
fn trim_trailing_zeros(buf: &mut [u8], len: usize) -> usize {
    // Find the position of 'e'/'E' (if scientific notation).
    let mut exp_pos = len;
    let mut i = 0;
    while i < len {
        if let Some(&b) = buf.get(i)
            && (b == b'e' || b == b'E') {
            exp_pos = i;
            break;
        }
        i = i.wrapping_add(1);
    }

    // Find decimal point.
    let mut dot_pos = exp_pos;
    i = 0;
    while i < exp_pos {
        if let Some(&b) = buf.get(i)
            && b == b'.' {
            dot_pos = i;
            break;
        }
        i = i.wrapping_add(1);
    }

    if dot_pos == exp_pos {
        return len; // No decimal point — nothing to trim.
    }

    // Trim trailing zeros between dot and exp.
    let mut trim_end = exp_pos;
    while trim_end > dot_pos.wrapping_add(1) {
        if let Some(&b) = buf.get(trim_end.wrapping_sub(1))
            && b != b'0' { break; }
        trim_end = trim_end.wrapping_sub(1);
    }

    // Remove dot if nothing after it.
    if trim_end == dot_pos.wrapping_add(1) {
        trim_end = dot_pos;
    }

    // Move exponent part (if any) up.
    if exp_pos < len {
        let exp_len = len.wrapping_sub(exp_pos);
        let mut k = 0;
        while k < exp_len {
            let src_idx = exp_pos.wrapping_add(k);
            let dst_idx = trim_end.wrapping_add(k);
            if let Some(&src) = buf.get(src_idx)
                && let Some(dst_slot) = buf.get_mut(dst_idx) {
                *dst_slot = src;
            }
            k = k.wrapping_add(1);
        }
        trim_end = trim_end.wrapping_add(exp_len);
    }

    trim_end
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Helpers
    //
    // `format_core` and `_snprintf_impl` are pure formatting functions that
    // write to a caller-supplied buffer — no syscalls, no I/O — so they
    // work on the host test target.
    //
    // Integer arguments are passed as a `&[u64]` array.  String pointer
    // arguments are cast to `u64`.  Float arguments are passed as
    // `f64::to_bits()` in a separate `&[u64]` array.
    // -----------------------------------------------------------------------

    /// Format via `_snprintf_impl` and return the output as a `String`.
    fn snprintf_str(fmt: &[u8], args: &[u64], fargs: &[u64]) -> (String, i32) {
        let mut buf = [0u8; 512];
        let n = _snprintf_impl(
            buf.as_mut_ptr(),
            buf.len(),
            fmt.as_ptr(),
            args.as_ptr(),
            fargs.as_ptr(),
        );
        let len = if n >= 0 && (n as usize) < buf.len() {
            n as usize
        } else {
            buf.len().wrapping_sub(1)
        };
        let s = core::str::from_utf8(&buf[..len]).unwrap_or("<invalid utf8>").to_string();
        (s, n)
    }

    /// Format via `format_core` and return the output as a `String`.
    #[allow(dead_code)]
    fn fmt_str(fmt: &[u8], args: &[u64], fargs: &[u64]) -> (String, i32) {
        let mut buf = [0u8; 512];
        let n = format_core(
            buf.as_mut_ptr(),
            buf.len(),
            fmt.as_ptr(),
            args.as_ptr(),
            fargs.as_ptr(),
        );
        let len = if n >= 0 && (n as usize) < buf.len() {
            n as usize
        } else if n >= 0 {
            buf.len()
        } else {
            0
        };
        let s = core::str::from_utf8(&buf[..len]).unwrap_or("<invalid utf8>").to_string();
        (s, n)
    }

    // -----------------------------------------------------------------------
    // 1. %d formatting
    // -----------------------------------------------------------------------

    #[test]
    fn fmt_d_positive() {
        let (s, n) = snprintf_str(b"%d\0", &[42], &[]);
        assert_eq!(s, "42");
        assert_eq!(n, 2);
    }

    #[test]
    fn fmt_d_negative() {
        let val = (-7i64) as u64;
        let (s, _) = snprintf_str(b"%d\0", &[val], &[]);
        assert_eq!(s, "-7");
    }

    #[test]
    fn fmt_d_zero() {
        let (s, _) = snprintf_str(b"%d\0", &[0], &[]);
        assert_eq!(s, "0");
    }

    #[test]
    fn fmt_d_large() {
        let val = 1_000_000u64;
        let (s, _) = snprintf_str(b"%d\0", &[val], &[]);
        assert_eq!(s, "1000000");
    }

    // -----------------------------------------------------------------------
    // 2. %s string formatting
    // -----------------------------------------------------------------------

    #[test]
    fn fmt_s_basic() {
        let text = b"hello\0";
        let (s, _) = snprintf_str(b"%s\0", &[text.as_ptr() as u64], &[]);
        assert_eq!(s, "hello");
    }

    #[test]
    fn fmt_s_null_pointer() {
        let (s, _) = snprintf_str(b"%s\0", &[0u64], &[]);
        assert_eq!(s, "(null)");
    }

    #[test]
    fn fmt_s_empty() {
        let text = b"\0";
        let (s, _) = snprintf_str(b"%s\0", &[text.as_ptr() as u64], &[]);
        assert_eq!(s, "");
    }

    // -----------------------------------------------------------------------
    // 3. %x hex formatting
    // -----------------------------------------------------------------------

    #[test]
    fn fmt_x_basic() {
        let (s, _) = snprintf_str(b"%x\0", &[255], &[]);
        assert_eq!(s, "ff");
    }

    #[test]
    fn fmt_x_upper() {
        let (s, _) = snprintf_str(b"%X\0", &[255], &[]);
        assert_eq!(s, "FF");
    }

    #[test]
    fn fmt_x_zero() {
        let (s, _) = snprintf_str(b"%x\0", &[0], &[]);
        assert_eq!(s, "0");
    }

    #[test]
    fn fmt_x_large() {
        let (s, _) = snprintf_str(b"%x\0", &[0xDEAD_BEEFu64], &[]);
        assert_eq!(s, "deadbeef");
    }

    // -----------------------------------------------------------------------
    // 4. Width and precision
    // -----------------------------------------------------------------------

    #[test]
    fn fmt_width_d() {
        let (s, _) = snprintf_str(b"%10d\0", &[42], &[]);
        assert_eq!(s, "        42");
    }

    #[test]
    fn fmt_width_d_left_align() {
        let (s, _) = snprintf_str(b"%-10d\0", &[42], &[]);
        assert_eq!(s, "42        ");
    }

    #[test]
    fn fmt_precision_s() {
        let text = b"hello world\0";
        let (s, _) = snprintf_str(b"%.5s\0", &[text.as_ptr() as u64], &[]);
        assert_eq!(s, "hello");
    }

    #[test]
    fn fmt_zero_pad_x() {
        let (s, _) = snprintf_str(b"%08x\0", &[0xFF], &[]);
        assert_eq!(s, "000000ff");
    }

    #[test]
    fn fmt_width_s() {
        let text = b"hi\0";
        let (s, _) = snprintf_str(b"%10s\0", &[text.as_ptr() as u64], &[]);
        assert_eq!(s, "        hi");
    }

    #[test]
    fn fmt_precision_d() {
        // Precision on integer: minimum digits.
        let (s, _) = snprintf_str(b"%.5d\0", &[42], &[]);
        assert_eq!(s, "00042");
    }

    // -----------------------------------------------------------------------
    // 5. Flags
    // -----------------------------------------------------------------------

    #[test]
    fn fmt_flag_minus() {
        let (s, _) = snprintf_str(b"%-5d\0", &[42], &[]);
        assert_eq!(s, "42   ");
    }

    #[test]
    fn fmt_flag_zero() {
        let (s, _) = snprintf_str(b"%05d\0", &[42], &[]);
        assert_eq!(s, "00042");
    }

    #[test]
    fn fmt_flag_plus_positive() {
        let (s, _) = snprintf_str(b"%+d\0", &[42], &[]);
        assert_eq!(s, "+42");
    }

    #[test]
    fn fmt_flag_plus_negative() {
        let val = (-42i64) as u64;
        let (s, _) = snprintf_str(b"%+d\0", &[val], &[]);
        assert_eq!(s, "-42");
    }

    #[test]
    fn fmt_flag_space() {
        let (s, _) = snprintf_str(b"% d\0", &[42], &[]);
        assert_eq!(s, " 42");
    }

    #[test]
    fn fmt_flag_hash_x() {
        let (s, _) = snprintf_str(b"%#x\0", &[255], &[]);
        assert_eq!(s, "0xff");
    }

    #[test]
    fn fmt_flag_hash_x_zero() {
        // # flag with value 0: no prefix (per C standard).
        let (s, _) = snprintf_str(b"%#x\0", &[0], &[]);
        assert_eq!(s, "0");
    }

    #[test]
    fn fmt_flag_hash_o() {
        let (s, _) = snprintf_str(b"%#o\0", &[8], &[]);
        assert_eq!(s, "010");
    }

    // -----------------------------------------------------------------------
    // 6. %% literal percent
    // -----------------------------------------------------------------------

    #[test]
    fn fmt_percent_literal() {
        let (s, _) = snprintf_str(b"100%%\0", &[], &[]);
        assert_eq!(s, "100%");
    }

    #[test]
    fn fmt_percent_in_middle() {
        let (s, _) = snprintf_str(b"a%%b%%c\0", &[], &[]);
        assert_eq!(s, "a%b%c");
    }

    // -----------------------------------------------------------------------
    // 7. Multiple format specs in one string
    // -----------------------------------------------------------------------

    #[test]
    fn fmt_multiple_specs() {
        let name = b"world\0";
        let (s, _) = snprintf_str(
            b"hello %s, num=%d, hex=%x\0",
            &[name.as_ptr() as u64, 42, 0xFF],
            &[],
        );
        assert_eq!(s, "hello world, num=42, hex=ff");
    }

    #[test]
    fn fmt_multiple_strings() {
        let a = b"foo\0";
        let b_str = b"bar\0";
        let (s, _) = snprintf_str(
            b"%s and %s\0",
            &[a.as_ptr() as u64, b_str.as_ptr() as u64],
            &[],
        );
        assert_eq!(s, "foo and bar");
    }

    // -----------------------------------------------------------------------
    // 8. Buffer overflow protection (snprintf truncation)
    // -----------------------------------------------------------------------

    #[test]
    fn snprintf_truncation() {
        let mut buf = [0xFFu8; 8];
        let n = _snprintf_impl(
            buf.as_mut_ptr(),
            buf.len(),
            b"hello world\0".as_ptr(),
            [].as_ptr(),
            [].as_ptr(),
        );
        // n should be 11 (total chars that would be written).
        assert_eq!(n, 11);
        // buf should be "hello w\0" (7 chars + NUL).
        assert_eq!(&buf, b"hello w\0");
    }

    #[test]
    fn snprintf_exact_fit() {
        let mut buf = [0xFFu8; 6];
        let n = _snprintf_impl(
            buf.as_mut_ptr(),
            buf.len(),
            b"hello\0".as_ptr(),
            [].as_ptr(),
            [].as_ptr(),
        );
        assert_eq!(n, 5);
        assert_eq!(&buf, b"hello\0");
    }

    #[test]
    fn snprintf_size_one() {
        let mut buf = [0xFFu8; 1];
        let n = _snprintf_impl(
            buf.as_mut_ptr(),
            buf.len(),
            b"hello\0".as_ptr(),
            [].as_ptr(),
            [].as_ptr(),
        );
        assert_eq!(n, 5);
        // Only the NUL terminator should be written.
        assert_eq!(buf[0], 0);
    }

    #[test]
    fn snprintf_null_buf_returns_count() {
        let n = _snprintf_impl(
            core::ptr::null_mut(),
            0,
            b"hello %d\0".as_ptr(),
            [42u64].as_ptr(),
            [].as_ptr(),
        );
        // Should return the number of chars that would be written.
        assert_eq!(n, 8); // "hello 42"
    }

    // -----------------------------------------------------------------------
    // 9. %p pointer formatting
    // -----------------------------------------------------------------------

    #[test]
    fn fmt_p_basic() {
        let (s, _) = snprintf_str(b"%p\0", &[0x1234u64], &[]);
        assert_eq!(s, "0x1234");
    }

    #[test]
    fn fmt_p_zero() {
        let (s, _) = snprintf_str(b"%p\0", &[0u64], &[]);
        assert_eq!(s, "0x0");
    }

    #[test]
    fn fmt_p_large() {
        let (s, _) = snprintf_str(b"%p\0", &[0xDEAD_BEEF_CAFE_BABEu64], &[]);
        assert_eq!(s, "0xdeadbeefcafebabe");
    }

    // -----------------------------------------------------------------------
    // 10. %f float formatting
    // -----------------------------------------------------------------------

    #[test]
    fn fmt_f_basic() {
        let val = 3.14f64;
        let (s, _) = snprintf_str(b"%f\0", &[], &[val.to_bits()]);
        // Default precision is 6.
        assert_eq!(s, "3.140000");
    }

    #[test]
    fn fmt_f_zero() {
        let val = 0.0f64;
        let (s, _) = snprintf_str(b"%f\0", &[], &[val.to_bits()]);
        assert_eq!(s, "0.000000");
    }

    #[test]
    fn fmt_f_negative() {
        let val = -2.5f64;
        let (s, _) = snprintf_str(b"%f\0", &[], &[val.to_bits()]);
        assert_eq!(s, "-2.500000");
    }

    #[test]
    fn fmt_f_precision() {
        let val = 3.14159f64;
        let (s, _) = snprintf_str(b"%.2f\0", &[], &[val.to_bits()]);
        assert_eq!(s, "3.14");
    }

    #[test]
    fn fmt_f_precision_zero() {
        let val = 3.7f64;
        let (s, _) = snprintf_str(b"%.0f\0", &[], &[val.to_bits()]);
        assert_eq!(s, "4"); // Rounds up.
    }

    #[test]
    fn fmt_f_width() {
        let val = 3.14f64;
        let (s, _) = snprintf_str(b"%10.2f\0", &[], &[val.to_bits()]);
        assert_eq!(s, "      3.14");
    }

    #[test]
    fn fmt_f_nan() {
        let val = f64::NAN;
        let (s, _) = snprintf_str(b"%f\0", &[], &[val.to_bits()]);
        assert_eq!(s, "nan");
    }

    #[test]
    fn fmt_f_inf() {
        let val = f64::INFINITY;
        let (s, _) = snprintf_str(b"%f\0", &[], &[val.to_bits()]);
        assert_eq!(s, "inf");
    }

    #[test]
    fn fmt_f_neg_inf() {
        let val = f64::NEG_INFINITY;
        let (s, _) = snprintf_str(b"%f\0", &[], &[val.to_bits()]);
        assert_eq!(s, "-inf");
    }

    // -----------------------------------------------------------------------
    // 11. %u unsigned
    // -----------------------------------------------------------------------

    #[test]
    fn fmt_u_basic() {
        let (s, _) = snprintf_str(b"%u\0", &[42], &[]);
        assert_eq!(s, "42");
    }

    #[test]
    fn fmt_u_large() {
        let val = u64::MAX;
        let (s, _) = snprintf_str(b"%u\0", &[val], &[]);
        assert_eq!(s, "18446744073709551615");
    }

    // -----------------------------------------------------------------------
    // 12. %o octal
    // -----------------------------------------------------------------------

    #[test]
    fn fmt_o_basic() {
        let (s, _) = snprintf_str(b"%o\0", &[8], &[]);
        assert_eq!(s, "10");
    }

    #[test]
    fn fmt_o_zero() {
        let (s, _) = snprintf_str(b"%o\0", &[0], &[]);
        assert_eq!(s, "0");
    }

    // -----------------------------------------------------------------------
    // 13. %c character
    // -----------------------------------------------------------------------

    #[test]
    fn fmt_c_basic() {
        let (s, _) = snprintf_str(b"%c\0", &[b'A' as u64], &[]);
        assert_eq!(s, "A");
    }

    #[test]
    fn fmt_c_with_width() {
        let (s, _) = snprintf_str(b"%5c\0", &[b'X' as u64], &[]);
        assert_eq!(s, "    X");
    }

    // -----------------------------------------------------------------------
    // 14. Plain text (no format specs)
    // -----------------------------------------------------------------------

    #[test]
    fn fmt_plain_text() {
        let (s, n) = snprintf_str(b"hello world\0", &[], &[]);
        assert_eq!(s, "hello world");
        assert_eq!(n, 11);
    }

    #[test]
    fn fmt_empty() {
        let (s, n) = snprintf_str(b"\0", &[], &[]);
        assert_eq!(s, "");
        assert_eq!(n, 0);
    }

    // -----------------------------------------------------------------------
    // 15. format_core with null format
    // -----------------------------------------------------------------------

    #[test]
    fn format_core_null_fmt() {
        let n = format_core(
            core::ptr::null_mut(),
            0,
            core::ptr::null(),
            [].as_ptr(),
            [].as_ptr(),
        );
        assert_eq!(n, -1);
    }

    // -----------------------------------------------------------------------
    // 16. Mixed integer and float args
    // -----------------------------------------------------------------------

    #[test]
    fn fmt_mixed_int_and_float() {
        let text = b"pi\0";
        let pi = 3.14f64;
        let (s, _) = snprintf_str(
            b"%s is %.2f and %d is an int\0",
            &[text.as_ptr() as u64, 42],
            &[pi.to_bits()],
        );
        assert_eq!(s, "pi is 3.14 and 42 is an int");
    }

    // -----------------------------------------------------------------------
    // 17. Width with zero-pad and sign flags combined
    // -----------------------------------------------------------------------

    #[test]
    fn fmt_zero_pad_with_sign() {
        let val = 42u64;
        let (s, _) = snprintf_str(b"%+08d\0", &[val], &[]);
        assert_eq!(s, "+0000042");
    }

    #[test]
    fn fmt_zero_pad_negative() {
        let val = (-42i64) as u64;
        let (s, _) = snprintf_str(b"%08d\0", &[val], &[]);
        assert_eq!(s, "-0000042");
    }

    // -----------------------------------------------------------------------
    // 18. Long format specifiers
    // -----------------------------------------------------------------------

    #[test]
    fn fmt_ld() {
        let val = 123456i64 as u64;
        let (s, _) = snprintf_str(b"%ld\0", &[val], &[]);
        assert_eq!(s, "123456");
    }

    #[test]
    fn fmt_lx() {
        let (s, _) = snprintf_str(b"%lx\0", &[0xABCDu64], &[]);
        assert_eq!(s, "abcd");
    }

    // -----------------------------------------------------------------------
    // 19. %e scientific notation
    // -----------------------------------------------------------------------

    #[test]
    fn fmt_e_basic() {
        let val = 12345.0f64;
        let (s, _) = snprintf_str(b"%e\0", &[], &[val.to_bits()]);
        assert_eq!(s, "1.234500e+04", "got: {s}");
    }

    #[test]
    fn fmt_e_small() {
        let val = 1.5f64;
        let (s, _) = snprintf_str(b"%e\0", &[], &[val.to_bits()]);
        assert_eq!(s, "1.500000e+00", "got: {s}");
    }

    #[test]
    fn fmt_e_nine_point_five() {
        // Regression: ilogb(9.5)=3 but decimal exp=0.
        let val = 9.5f64;
        let (s, _) = snprintf_str(b"%e\0", &[], &[val.to_bits()]);
        assert_eq!(s, "9.500000e+00", "got: {s}");
    }

    #[test]
    fn fmt_e_fraction() {
        let val = 0.00123f64;
        let (s, _) = snprintf_str(b"%e\0", &[], &[val.to_bits()]);
        assert_eq!(s, "1.230000e-03", "got: {s}");
    }

    #[test]
    fn fmt_e_zero() {
        let val = 0.0f64;
        let (s, _) = snprintf_str(b"%e\0", &[], &[val.to_bits()]);
        assert_eq!(s, "0.000000e+00", "got: {s}");
    }

    // -----------------------------------------------------------------------
    // 20. Precision 0 with value 0
    // -----------------------------------------------------------------------

    #[test]
    fn fmt_precision_zero_value_zero() {
        // C99: %.0d with value 0 produces no digits.
        let (s, _) = snprintf_str(b"%.0d\0", &[0], &[]);
        assert_eq!(s, "");
    }

    #[test]
    fn fmt_precision_zero_value_nonzero() {
        let (s, _) = snprintf_str(b"%.0d\0", &[5], &[]);
        assert_eq!(s, "5");
    }
}
