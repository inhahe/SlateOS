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
//! - Width and precision: `%10d`, `%-10s`, `%08x`, `%.5s`, `%*d`
//! - Flags: `-` (left-align), `0` (zero-pad), `+` (sign), ` ` (space)
//!
//! ## Architecture
//!
//! 1. Assembly wrappers (`printf`, `fprintf`, `sprintf`, `snprintf`)
//!    capture register args (rsi–r9) into a stack array and call
//!    the corresponding `_*_impl` Rust function.
//! 2. All `_*_impl` functions call `format_core()` which parses the
//!    format string and consumes args from the array.
//! 3. Output goes to a buffer (snprintf) or to a fd (printf/fprintf).

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

#[cfg(not(test))]
core::arch::global_asm!(
    // printf(fmt, ...) → _printf_impl(fmt, args_ptr)
    ".global printf",
    ".type printf, @function",
    "printf:",
    "push rbp",
    "mov rbp, rsp",
    "sub rsp, 64",           // 8 * 8 bytes for args
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
    "call _printf_impl",
    "add rsp, 64",
    "pop rbp",
    "ret",

    // fprintf(stream, fmt, ...) → _fprintf_impl(stream, fmt, args_ptr)
    ".global fprintf",
    ".type fprintf, @function",
    "fprintf:",
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
    "call _fprintf_impl",
    "add rsp, 64",
    "pop rbp",
    "ret",

    // snprintf(buf, size, fmt, ...) → _snprintf_impl(buf, size, fmt, args_ptr)
    ".global snprintf",
    ".type snprintf, @function",
    "snprintf:",
    "push rbp",
    "mov rbp, rsp",
    "sub rsp, 64",
    "mov [rsp], rcx",        // vararg 0
    "mov [rsp+8], r8",       // vararg 1
    "mov [rsp+16], r9",      // vararg 2
    "mov rax, [rbp+16]",     // vararg 3 (stack)
    "mov [rsp+24], rax",
    "mov rax, [rbp+24]",     // vararg 4
    "mov [rsp+32], rax",
    "mov rax, [rbp+32]",     // vararg 5
    "mov [rsp+40], rax",
    "mov rax, [rbp+40]",     // vararg 6
    "mov [rsp+48], rax",
    "mov rax, [rbp+48]",     // vararg 7
    "mov [rsp+56], rax",
    // rdi = buf, rsi = size, rdx = fmt (already set)
    "mov rcx, rsp",          // args array
    "call _snprintf_impl",
    "add rsp, 64",
    "pop rbp",
    "ret",

    // sprintf(buf, fmt, ...) → _sprintf_impl(buf, fmt, args_ptr)
    ".global sprintf",
    ".type sprintf, @function",
    "sprintf:",
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
    // rdi = buf, rsi = fmt (already set)
    "mov rdx, rsp",          // args array
    "call _sprintf_impl",
    "add rsp, 64",
    "pop rbp",
    "ret",
);

// ---------------------------------------------------------------------------
// Rust entry points (called by assembly)
// ---------------------------------------------------------------------------

/// Stack buffer size for printf/fprintf (format to buffer, then write).
const PRINTF_BUF_SIZE: usize = 4096;

/// `printf(fmt, ...)` — write formatted output to stdout.
#[unsafe(no_mangle)]
pub extern "C" fn _printf_impl(fmt: *const u8, args: *const u64) -> i32 {
    let mut buf = [0u8; PRINTF_BUF_SIZE];
    let n = format_core(buf.as_mut_ptr(), PRINTF_BUF_SIZE, fmt, args);
    if n <= 0 {
        return n;
    }
    let write_len = if (n as usize) < PRINTF_BUF_SIZE { n as usize } else { PRINTF_BUF_SIZE };
    let ret = crate::file::write(1, buf.as_ptr(), write_len);
    if ret < 0 { ret as i32 } else { n }
}

/// `fprintf(stream, fmt, ...)` — write formatted output to a stream.
#[unsafe(no_mangle)]
pub extern "C" fn _fprintf_impl(stream: *mut u8, fmt: *const u8, args: *const u64) -> i32 {
    let fd = stream as usize as i32;
    let mut buf = [0u8; PRINTF_BUF_SIZE];
    let n = format_core(buf.as_mut_ptr(), PRINTF_BUF_SIZE, fmt, args);
    if n <= 0 {
        return n;
    }
    let write_len = if (n as usize) < PRINTF_BUF_SIZE { n as usize } else { PRINTF_BUF_SIZE };
    let ret = crate::file::write(fd, buf.as_ptr(), write_len);
    if ret < 0 { ret as i32 } else { n }
}

/// `snprintf(buf, size, fmt, ...)` — write formatted output to a buffer.
#[unsafe(no_mangle)]
pub extern "C" fn _snprintf_impl(
    buf: *mut u8,
    size: usize,
    fmt: *const u8,
    args: *const u64,
) -> i32 {
    if buf.is_null() || size == 0 {
        // Still count characters.
        return format_core(core::ptr::null_mut(), 0, fmt, args);
    }
    let n = format_core(buf, size, fmt, args);
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
#[unsafe(no_mangle)]
pub extern "C" fn _sprintf_impl(buf: *mut u8, fmt: *const u8, args: *const u64) -> i32 {
    // No size limit — dangerous but matches C semantics.
    format_core(buf, usize::MAX, fmt, args)
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
fn dispatch_spec(
    dst: &mut FmtOutput,
    fmt: *const u8,
    fpos: usize,
    spec_start: usize,
    spec: &FormatSpec,
    args: *const u64,
    arg_idx: &mut usize,
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
            emit_byte(dst, b'0');
            emit_byte(dst, b'x');
            format_unsigned(dst, val, 16, false, &spec.flags, 0, None);
        }

        b'n' => {
            let ptr = consume_arg(args, arg_idx) as *mut i32;
            if !ptr.is_null() {
                // SAFETY: Caller guarantees ptr is valid.
                unsafe { *ptr = dst.pos as i32; }
            }
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
) -> i32 {
    if fmt.is_null() {
        return -1;
    }

    let mut dst = FmtOutput::new(out, out_size);
    let mut arg_idx: usize = 0;
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
        fpos = dispatch_spec(&mut dst, fmt, fpos, spec_start, &spec, args, &mut arg_idx);
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
    let num_len = u64_to_dec(abs_val, &mut num_buf);
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
    let num_len = u64_to_base(val, base, upper, &mut num_buf);
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
        let null_str = b"(null)";
        emit_bytes(dst, null_str);
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
