//! Scanf family: `sscanf`, `scanf`, `fscanf` via assembly trampoline.
//!
//! `scanf` is variadic in C, so the three direct entry points are assembly
//! trampolines that perform a real System V `va_start` and tail-call the
//! corresponding `v*` function.  There is exactly one scanning engine, and it
//! pulls each destination pointer from the `va_list` at the point of use.
//!
//! `scanf` and `fscanf` read a line from stdin/stream into a stack
//! buffer, then scan it with the same engine as `sscanf`.
//!
//! The `v*` variants (`vsscanf`, `vscanf`, `vfscanf`) take that `va_list`
//! directly.  Since a `va_list` parameter decays to a pointer on the x86_64
//! System V ABI, they are ordinary Rust functions — and therefore the whole
//! family is reachable from host `cargo test`.  The glibc `__isoc99_v*scanf`
//! aliases are provided too.
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

// The three direct entry points share `printf.rs`'s `va_trampoline!`: each
// spills the argument registers into a System V register save area, builds a
// `va_list` over it and calls the matching `v*` function below.  Named-argument
// counts, which set the initial `gp_offset` and decide which register carries
// the `va_list*`:
//   sscanf(str, fmt, ...)      2 named -> gp_offset 16, ap in rdx
//   scanf(fmt, ...)            1 named -> gp_offset 8,  ap in rsi
//   fscanf(stream, fmt, ...)   2 named -> gp_offset 16, ap in rdx
#[cfg(target_os = "none")]
use crate::printf::va_trampoline;

#[cfg(target_os = "none")]
va_trampoline!("sscanf", "vsscanf", "16", "rdx");
#[cfg(target_os = "none")]
va_trampoline!("scanf", "vscanf", "8", "rsi");
#[cfg(target_os = "none")]
va_trampoline!("fscanf", "vfscanf", "16", "rdx");

#[cfg(target_os = "none")]
core::arch::global_asm!(
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
// The scanning engine's three entry shapes
// ---------------------------------------------------------------------------

use crate::printf::{self, VaList};

// Length-modifier codes threaded through the `scan_*` helpers as `long_mod`.
//
// The ordering is load-bearing: the integer scanners test `>= LEN_LONG` to
// decide between a 32- and a 64-bit store, and every modifier at or above
// `LEN_LONG` is 64-bit on LP64.  Only `scan_float` looks at the exact value,
// because only it can be handed something wider than 64 bits.

/// No modifier: `%d` -> `i32`, `%f` -> `f32`.
const LEN_DEFAULT: u8 = 0;
/// `l` (and `z`/`j`/`t`, all 64-bit here): `%ld` -> `i64`, `%lf` -> `f64`.
const LEN_LONG: u8 = 1;
/// `ll`: `%lld` -> `i64`.
const LEN_LONG_LONG: u8 = 2;
/// `L`: `%Lf` -> a 16-byte x87 `long double`.  On the integer conversions this
/// behaves like `ll`, matching glibc's treatment of `L` as a deprecated
/// synonym for `ll` (C leaves it undefined there).
const LEN_LONG_DOUBLE: u8 = 3;

/// Buffer size for reading a line from stdin/stream for scanf/fscanf.
const SCANF_LINE_BUF: usize = 4096;

/// Scan `input` against `fmt`, storing through pointers pulled from `args`.
///
/// Returns the number of items successfully assigned, or EOF (-1) if the
/// input is exhausted before the first conversion.
fn scan_string_source(input: *const u8, fmt: *const u8, args: &mut printf::Args) -> i32 {
    if input.is_null() || fmt.is_null() {
        return -1;
    }

    let mut ctx = ScanCtx {
        input,
        fmt,
        args,
        si: 0,       // Position in input string.
        fi: 0,       // Position in format string.
        assigned: 0, // Number of successful assignments.
    };

    scan_core(&mut ctx)
}

/// Read one line from `fd` into a stack buffer and scan it.
///
/// Shared by `vscanf` (fd 0) and `vfscanf` (the stream's fd): both differ from
/// `vsscanf` only in where the bytes come from.
fn scan_fd_source(fd: i32, fmt: *const u8, args: &mut printf::Args) -> i32 {
    if fmt.is_null() {
        return -1;
    }

    let mut buf = [0u8; SCANF_LINE_BUF];
    if read_line_from_fd(fd, &mut buf) == 0 {
        return -1; // EOF
    }

    scan_string_source(buf.as_ptr(), fmt, args)
}

// ---------------------------------------------------------------------------
// va_list support -- the v* scanf family
// ---------------------------------------------------------------------------
//
// `vsscanf`/`vscanf`/`vfscanf` receive an already-initialised `va_list` (which
// decays to a pointer on the x86_64 System V ABI), so they are plain Rust
// functions -- host-testable, and the sole route into the engine: the direct
// `sscanf`/`scanf`/`fscanf` trampolines above synthesise a `va_list` and call
// straight into these.
//
// An earlier design instead flattened the destination pointers into a fixed
// `[u64; 8]` by pre-walking the format string.  That was strictly worse than
// printf's equivalent bug (BUG-POSIX-SCANF-ARG-ARRAY-OOB): a ninth conversion
// read past the array and used whatever stack word it found as a destination
// pointer *to write through*, turning `sscanf` with nine conversions into an
// arbitrary stack write.  Pulling each pointer at the point of use removes the
// array, and with it the bound.

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
    let mut args = unsafe { printf::Args::from_raw(ap) };
    scan_string_source(input, fmt, &mut args)
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
    let mut args = unsafe { printf::Args::from_raw(ap) };
    scan_fd_source(0, fmt, &mut args)
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
    // The fd must be resolved before the format check so a bad stream is
    // reported as an error rather than as EOF.
    let fd = crate::stdio::fileno(stream);
    if fd < 0 {
        return -1;
    }
    // SAFETY: ap is non-null; caller guarantees it is a valid va_list.
    let mut args = unsafe { printf::Args::from_raw(ap) };
    scan_fd_source(fd, fmt, &mut args)
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
///
/// The destination pointers are not held here: `args` is the caller's live
/// `va_list`, and each conversion pulls its own pointer out of it as it runs.
struct ScanCtx<'a, 'v> {
    input: *const u8,
    fmt: *const u8,
    args: &'a mut printf::Args<'v>,
    si: usize,
    fi: usize,
    assigned: i32,
}

impl ScanCtx<'_, '_> {
    /// Read the current input byte (0 if past end).
    #[inline]
    fn peek(&self) -> u8 {
        // SAFETY: Caller guarantees input is a valid null-terminated string.
        unsafe { *self.input.add(self.si) }
    }

    /// Read the input byte `off` positions ahead (0 if past end).
    ///
    /// Lets a conversion look ahead at a multi-byte token — `infinity`, a
    /// `nan(...)` payload — and only commit `si` once the whole thing matched.
    #[inline]
    fn peek_at(&self, off: usize) -> u8 {
        // SAFETY: Caller guarantees input is a valid null-terminated string,
        // and every caller stops at the first NUL, so `si + off` stays inside
        // it once the bytes before it have been checked.
        unsafe { *self.input.add(self.si.wrapping_add(off)) }
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

    /// Consume the next destination pointer from the caller's `va_list`.
    ///
    /// A `va_list` cannot report exhaustion, so a caller that passes fewer
    /// pointers than `fmt` has conversions gets garbage here exactly as it
    /// would from glibc.  Every store site null-checks, which at least makes
    /// the common "passed a NULL" mistake a no-op rather than a fault.
    #[inline]
    fn next_arg(&mut self) -> u64 {
        self.args.int()
    }

    /// Skip ASCII whitespace in input.
    fn skip_ws(&mut self) {
        while matches!(self.peek(), b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c) {
            self.advance();
        }
    }

    /// Did the directive that just failed fail because the input ran out?
    ///
    /// C distinguishes an *input* failure — the input ended before the
    /// directive matched anything — from a *matching* failure, where
    /// characters were read but did not form a valid item.  Only the former
    /// makes `scanf` return `EOF`; a matching failure returns the number of
    /// items assigned so far, which is often zero.
    ///
    /// The distinction is not simply "are we at end of input": `sscanf("0x",
    /// "%lf")` ends at end of input and still returns 0, because the directive
    /// consumed `0x` before discovering there was no value there.  What makes
    /// it an input failure is that *nothing was matched*, so the test is
    /// whether everything consumed since the directive started was leading
    /// whitespace — which no directive treats as part of its item.
    fn stopped_at_end_of_input(&self, started_at: usize) -> bool {
        if self.peek() != 0 {
            return false;
        }
        (started_at..self.si).all(|i| {
            // SAFETY: `i` is below `self.si`, so it names a byte this scan has
            // already read from the caller's NUL-terminated string.
            let byte = unsafe { *self.input.add(i) };
            matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
        })
    }
}

// ---------------------------------------------------------------------------
// Core scanning engine
// ---------------------------------------------------------------------------

/// Main scan loop.
#[allow(clippy::arithmetic_side_effects, clippy::too_many_lines)]
fn scan_core(ctx: &mut ScanCtx<'_, '_>) -> i32 {
    // Set when the scan stopped because the input ran out rather than because
    // it held something that did not match.  Only the former yields EOF.
    let mut input_failure = false;
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
                    input_failure = ctx.peek() == 0;
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

            // Parse length modifier.  See the LEN_* constants: everything from
            // LEN_LONG upward stores 64 bits, so the integer scanners only
            // test `>= LEN_LONG`; only the float path distinguishes further.
            let mut long_mod = LEN_DEFAULT;
            match ctx.fmt_peek() {
                b'l' => {
                    long_mod = LEN_LONG;
                    ctx.fmt_advance();
                    if ctx.fmt_peek() == b'l' {
                        long_mod = LEN_LONG_LONG;
                        ctx.fmt_advance();
                    }
                }
                b'h' => {
                    ctx.fmt_advance();
                    if ctx.fmt_peek() == b'h' {
                        ctx.fmt_advance();
                    }
                    // We treat h/hh the same as default (store as i32/u32).
                }
                // `size_t`, `intmax_t` and `ptrdiff_t` are all 64-bit on LP64.
                // Without these the modifier was read as the conversion
                // character, so `%zu` matched nothing and stored nothing.
                b'z' | b'j' | b't' => {
                    long_mod = LEN_LONG;
                    ctx.fmt_advance();
                }
                b'L' => {
                    long_mod = LEN_LONG_DOUBLE;
                    ctx.fmt_advance();
                }
                _ => {}
            }

            let conv = ctx.fmt_peek();
            if conv == 0 {
                break;
            }
            ctx.fmt_advance();

            let started_at = ctx.si;
            let matched = match conv {
                b'd' => scan_signed_int(ctx, suppress, width, long_mod),
                // %i auto-detects base: 0x/0X → hex, 0 → octal, else decimal.
                b'i' => scan_signed_int_auto(ctx, suppress, width, long_mod),
                b'u' => scan_unsigned_int(ctx, suppress, width, long_mod, 10),
                b'x' | b'X' => scan_unsigned_int(ctx, suppress, width, long_mod, 16),
                b'o' => scan_unsigned_int(ctx, suppress, width, long_mod, 8),
                b's' => scan_string(ctx, suppress, width),
                b'c' => scan_char(ctx, suppress, width, has_width),
                b'[' => scan_scanset(ctx, suppress, width),
                b'f' | b'e' | b'g' | b'a' => scan_float(ctx, suppress, width, long_mod),
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
                    true
                }
                // Unknown specifier — stop.
                _ => false,
            };
            if !matched {
                input_failure = ctx.stopped_at_end_of_input(started_at);
                break;
            }
        } else {
            // Literal character — must match input exactly.
            if ctx.peek() != fc {
                input_failure = ctx.peek() == 0;
                break;
            }
            ctx.advance();
            ctx.fmt_advance();
        }
    }

    // EOF is reported only when the input ran out before anything matched;
    // a directive that read characters and then found them unusable is a
    // matching failure and reports the assignments made so far — zero,
    // usually.  See `ScanCtx::stopped_at_end_of_input`.
    if ctx.assigned == 0 && input_failure {
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
fn scan_signed_int(ctx: &mut ScanCtx<'_, '_>, suppress: bool, width: usize, long_mod: u8) -> bool {
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
        if long_mod >= LEN_LONG {
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
fn scan_signed_int_auto(ctx: &mut ScanCtx<'_, '_>, suppress: bool, width: usize, long_mod: u8) -> bool {
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
        if long_mod >= LEN_LONG {
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
    ctx: &mut ScanCtx<'_, '_>,
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
        if long_mod >= LEN_LONG {
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
fn scan_string(ctx: &mut ScanCtx<'_, '_>, suppress: bool, width: usize) -> bool {
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
fn scan_char(ctx: &mut ScanCtx<'_, '_>, suppress: bool, width: usize, has_width: bool) -> bool {
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

/// Match a case-insensitive literal, consuming it only if all of it is there.
///
/// `scanf` must not eat a partial token: seeing `in` of `int` and advancing
/// would corrupt every conversion after it.
fn match_word(ctx: &mut ScanCtx<'_, '_>, count: &mut usize, width: usize, word: &[u8]) -> bool {
    for (k, &want) in word.iter().enumerate() {
        if count.wrapping_add(k) >= width {
            return false;
        }
        let c = ctx.peek_at(k);
        if c == 0 || (c | 0x20) != want {
            return false;
        }
    }
    ctx.si = ctx.si.wrapping_add(word.len());
    *count = count.wrapping_add(word.len());
    true
}

/// Consume as much of `word` as the input matches, and report how much.
///
/// Unlike [`match_word`] this keeps a partial match.  Its one caller is the
/// `infinity` extension of `inf`, which needs the longest-prefix rule:
/// `"infi"` is a prefix of a matching sequence, so it belongs to the
/// conversion even though the conversion then has no value to produce.
fn match_prefix(ctx: &mut ScanCtx<'_, '_>, count: &mut usize, width: usize, word: &[u8]) -> usize {
    let mut k = 0usize;
    for &want in word {
        if count.wrapping_add(k) >= width {
            break;
        }
        let c = ctx.peek_at(k);
        if c == 0 || (c | 0x20) != want {
            break;
        }
        k = k.wrapping_add(1);
    }
    ctx.si = ctx.si.wrapping_add(k);
    *count = count.wrapping_add(k);
    k
}

/// What [`scan_float_named`] found.
enum NamedScan {
    /// The input does not begin with `inf`/`nan`; try the digit grammar.
    NotNamed,
    /// A named value was matched and consumed.
    Value(f64),
    /// A partial `infinity` was consumed, which is a prefix of a matching
    /// sequence but not one itself — a matching failure.
    Failed,
}

/// Match `INF`/`INFINITY` or `NAN[(n-char-sequence)]`, as `strtod` does.
#[allow(clippy::arithmetic_side_effects)]
fn scan_float_named(ctx: &mut ScanCtx<'_, '_>, count: &mut usize, width: usize) -> NamedScan {
    if match_word(ctx, count, width, b"nan") {
        // The optional payload is consumed only if it is properly closed;
        // otherwise the '(' belongs to whatever comes next in the input.
        if *count < width && ctx.peek() == b'(' {
            let mut k = 1usize;
            loop {
                if count.wrapping_add(k) >= width {
                    break;
                }
                let c = ctx.peek_at(k);
                if c == b')' {
                    ctx.si += k + 1;
                    *count += k + 1;
                    break;
                }
                if c == 0 || !(c.is_ascii_alphanumeric() || c == b'_') {
                    break;
                }
                k += 1;
            }
        }
        return NamedScan::Value(f64::NAN);
    }
    if match_word(ctx, count, width, b"inf") {
        // "infinity" extends "inf", and the longest prefix of it wins.  Stop
        // after zero extra characters and the item is "inf"; after all five
        // and it is "infinity".  Anything in between — "infi", "infinit" — is
        // a prefix of a matching sequence and so is consumed, but is not
        // itself one, which makes it a matching failure.  Treating it as a
        // plain "inf" instead would hand "ix" to the next conversion out of
        // `sscanf("infix", "%lf%s", ...)`; glibc reports the failure.
        let extra = match_prefix(ctx, count, width, b"inity");
        if extra == 0 || extra == b"inity".len() {
            return NamedScan::Value(f64::INFINITY);
        }
        return NamedScan::Failed;
    }
    NamedScan::NotNamed
}

/// Scan the digits of a float into `acc`, honouring the field width.
///
/// Accepts either a decimal literal or a C99 hexadecimal one (`0x1.8p+3`).
/// Returns false if there was no digit at all, in which case the caller has a
/// matching failure.
// `*count` only ever rises towards `width`, so it cannot overflow: the input
// would have to be 2^64 bytes long first.
#[allow(clippy::arithmetic_side_effects)]
fn scan_float_digits(
    ctx: &mut ScanCtx<'_, '_>,
    count: &mut usize,
    width: usize,
    acc: &mut crate::decfloat::DigitCollector,
) -> bool {
    match scan_hex_float_digits(ctx, count, width, acc) {
        HexScan::Converted => return true,
        HexScan::Failed => return false,
        HexScan::NotHex => {}
    }

    let mut has_digits = false;

    while *count < width && ctx.peek().is_ascii_digit() {
        acc.push_integer(ctx.peek());
        ctx.advance();
        *count += 1;
        has_digits = true;
    }

    if *count < width && ctx.peek() == b'.' {
        ctx.advance();
        *count += 1;
        while *count < width && ctx.peek().is_ascii_digit() {
            acc.push_fraction(ctx.peek());
            ctx.advance();
            *count += 1;
            has_digits = true;
        }
    }

    if !has_digits {
        return false;
    }

    scan_float_exponent(ctx, count, width, acc, b'e');
    true
}

/// What [`scan_hex_float_digits`] found.
enum HexScan {
    /// Not a hex literal; nothing was consumed and the decimal grammar
    /// should be tried instead.
    NotHex,
    /// A hex literal was read into the collector.
    Converted,
    /// A `0x` prefix was consumed but no digit followed it, so there is no
    /// value — a matching failure.
    Failed,
}

/// Scan a `0x`-prefixed hexadecimal float, if that is what comes next.
///
/// `scanf` consumes the longest sequence that is *a prefix of* a matching
/// input sequence, so unlike `strtod` it cannot back out of a `0x` once it has
/// read it: `sscanf("0xz", "%lf", &v)` consumes `0x`, finds no digit and
/// reports a matching failure rather than converting the `0` and leaving `xz`
/// behind. That is the [`HexScan::Failed`] case.
///
/// The field width is different: it truncates the input rather than describing
/// it, so a width that stops before the first hex digit means the literal was
/// never there — `sscanf("0x1", "%2lf", &v)` converts `0` and leaves `x1`.
// `*count` only ever rises towards `width`; see `scan_float_digits`.
#[allow(clippy::arithmetic_side_effects)]
fn scan_hex_float_digits(
    ctx: &mut ScanCtx<'_, '_>,
    count: &mut usize,
    width: usize,
    acc: &mut crate::decfloat::DigitCollector,
) -> HexScan {
    if ctx.peek() != b'0' || (ctx.peek_at(1) | 0x20) != b'x' {
        return HexScan::NotHex;
    }
    // Where the first significant digit would be: after `0x`, or after a
    // point that immediately follows it.
    let probe = if ctx.peek_at(2) == b'.' { 3 } else { 2 };
    let have_digit = ctx.peek_at(probe).is_ascii_hexdigit();

    if !have_digit {
        // No digit anywhere after the prefix.  Consuming `0x` is what the
        // longest-prefix rule demands, and it leaves nothing to convert.
        if width.saturating_sub(*count) < 2 {
            return HexScan::NotHex;
        }
        ctx.advance();
        ctx.advance();
        *count += 2;
        return HexScan::Failed;
    }
    if width.saturating_sub(*count) <= probe {
        // The width stops short of the digit, so only the leading `0` is
        // really in the field.
        return HexScan::NotHex;
    }

    acc.set_hex();
    ctx.advance();
    ctx.advance();
    *count += 2;

    while *count < width && ctx.peek().is_ascii_hexdigit() {
        acc.push_integer(ctx.peek());
        ctx.advance();
        *count += 1;
    }
    if *count < width && ctx.peek() == b'.' {
        ctx.advance();
        *count += 1;
        while *count < width && ctx.peek().is_ascii_hexdigit() {
            acc.push_fraction(ctx.peek());
            ctx.advance();
            *count += 1;
        }
    }

    scan_float_exponent(ctx, count, width, acc, b'p');
    HexScan::Converted
}

/// Scan an optional `[marker][sign]digits` exponent and apply it to `acc`.
///
/// `marker` is the lowercase form; both cases are accepted.
///
/// The marker and its sign are consumed even when no digits follow them.  A
/// directive takes the longest sequence that *is, or is a prefix of*, a
/// matching sequence (C11 7.21.6.2p9), and `"1.5e+"` is a prefix of
/// `"1.5e+1"`, so all of it belongs to this conversion.  The digits read
/// before the marker still give a value, so `sscanf("1.5e", "%lf", &v)` yields
/// 1.5 and leaves nothing behind — as glibc does.  Rolling the marker back
/// instead would hand a stray `e` to the next directive.
///
/// The same rule is what makes a field width cut cleanly: with `%6lf` on
/// `"0x1.8p+1"` the field is `"0x1.8p"`, and the trailing `p` is consumed
/// because it was the width, not the input, that ended the exponent.
// `*count` only ever rises towards `width`; see `scan_float_digits`.  The
// exponent value itself accumulates with saturating arithmetic.
#[allow(clippy::arithmetic_side_effects)]
fn scan_float_exponent(
    ctx: &mut ScanCtx<'_, '_>,
    count: &mut usize,
    width: usize,
    acc: &mut crate::decfloat::DigitCollector,
    marker: u8,
) {
    if *count >= width || (ctx.peek() | 0x20) != marker {
        return;
    }

    ctx.advance();
    *count += 1;

    let mut exp_neg = false;
    if *count < width && matches!(ctx.peek(), b'+' | b'-') {
        exp_neg = ctx.peek() == b'-';
        ctx.advance();
        *count += 1;
    }

    let mut exp_val: i32 = 0;
    let mut has_digits = false;
    while *count < width && ctx.peek().is_ascii_digit() {
        exp_val = exp_val
            .saturating_mul(10)
            .saturating_add(i32::from(ctx.peek().wrapping_sub(b'0')));
        ctx.advance();
        *count += 1;
        has_digits = true;
    }

    if has_digits {
        acc.apply_exponent(if exp_neg {
            exp_val.saturating_neg()
        } else {
            exp_val
        });
    }
}

/// Raise `ERANGE` if a conversion said it was out of range, and pass the
/// value through.
fn report_erange<T>(converted: (T, bool)) -> T {
    let (value, out_of_range) = converted;
    if out_of_range {
        crate::errno::set_errno(crate::errno::ERANGE);
    }
    value
}

/// Scan a floating-point number.
///
/// Accepts the same subject sequence as `strtod` — `[sign]digits[.digits]`
/// with an optional `e[sign]digits` exponent, or `INF`/`INFINITY`/`NAN`
/// (C99 7.21.6.2p12) — and stores an `f32`, `f64` or `long double` according
/// to the length modifier.
///
/// Digits go straight into a [`crate::decfloat::DigitCollector`] rather than
/// into a text buffer.  A fixed text buffer has to stop somewhere, and
/// stopping in the middle of a number leaves the tail — including any
/// exponent — in the input, which both changes the value silently and derails
/// every conversion after it.  Feeding the collector instead means an
/// arbitrarily long literal is consumed in full and converted exactly.
#[allow(clippy::arithmetic_side_effects)]
fn scan_float(ctx: &mut ScanCtx<'_, '_>, suppress: bool, width: usize, long_mod: u8) -> bool {
    ctx.skip_ws();
    if ctx.peek() == 0 {
        return false;
    }

    let mut count: usize = 0;

    // Sign.
    let negative = ctx.peek() == b'-';
    if (negative || ctx.peek() == b'+') && count < width {
        ctx.advance();
        count = count.wrapping_add(1);
    }

    let mut acc = crate::decfloat::DigitCollector::new();
    let named = match scan_float_named(ctx, &mut count, width) {
        NamedScan::Value(v) => Some(v),
        NamedScan::Failed => return false,
        NamedScan::NotNamed => None,
    };
    if named.is_none() && !scan_float_digits(ctx, &mut count, width, &mut acc) {
        return false;
    }

    if !suppress {
        let ptr = ctx.next_arg();
        if long_mod >= LEN_LONG {
            let magnitude = named.unwrap_or_else(|| report_erange(acc.to_f64()));
            let val = if negative { -magnitude } else { magnitude };
            if long_mod == LEN_LONG_DOUBLE {
                // %Lf → a 16-byte x87 `long double`.  Storing only the 8 bytes
                // of an f64 here would leave the destination's exponent/sign
                // half holding whatever was there before, so the caller would
                // read a value unrelated to the input — hence the explicit
                // re-encode.  The value itself still carries only f64
                // precision; see TD-POSIX-LONG-DOUBLE-PRECISION.
                let p = ptr as *mut crate::x87::LongDouble;
                if !p.is_null() {
                    // SAFETY: `%Lf` promises a writable `long double *`, which
                    // is exactly `LongDouble`'s 16 bytes.  Written unaligned
                    // because `LongDouble` declares `align(16)` and we would
                    // rather not make a UB claim about a pointer that came
                    // from C.
                    unsafe {
                        p.write_unaligned(crate::x87::from_f64(val));
                    }
                }
            } else {
                // %lf → f64
                let p = ptr as *mut f64;
                if !p.is_null() {
                    // SAFETY: `%lf` promises a writable `double *`.
                    unsafe {
                        *p = val;
                    }
                }
            }
        } else {
            // %f → f32, rounded straight from the digits.  Converting an f64
            // afterwards would round twice, and two roundings are not one.
            // `inf`/`nan` narrow exactly, so they need no such care.
            let magnitude = match named {
                Some(v) => v as f32,
                None => report_erange(acc.to_f32()),
            };
            let val = if negative { -magnitude } else { magnitude };
            let p = ptr as *mut f32;
            if !p.is_null() {
                // SAFETY: `%f` promises a writable `float *`.
                unsafe {
                    *p = val;
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
fn scan_scanset(ctx: &mut ScanCtx<'_, '_>, suppress: bool, width: usize) -> bool {
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

    /// Drive `vsscanf` with a synthetic, ABI-shaped `va_list` over `ptrs`.
    ///
    /// These tests used to call an `_sscanf_impl` that took a flat pointer
    /// array — a representation the engine no longer has.  Laying out the real
    /// System V register save area here means every assertion below exercises
    /// the same argument path a compiled C caller reaches through the `sscanf`
    /// trampoline, rather than a test-only shortcut.
    ///
    /// Every scanf argument is a pointer, hence INTEGER class: the first six
    /// go in the GP slots at 0..48 and the rest spill to the overflow area.
    fn sscanf_va(input: *const u8, fmt: *const u8, ptrs: &[u64]) -> i32 {
        let mut reg = [0u8; 176];
        let mut overflow = [0u8; 512];

        for (i, &v) in ptrs.iter().take(6).enumerate() {
            let off = i * 8;
            reg[off..off + 8].copy_from_slice(&v.to_le_bytes());
        }
        for (i, &v) in ptrs.iter().skip(6).enumerate() {
            let off = i * 8;
            assert!(off + 8 <= overflow.len(), "overflow area too small");
            overflow[off..off + 8].copy_from_slice(&v.to_le_bytes());
        }

        let mut va = VaList {
            gp_offset: 0,
            fp_offset: 48,
            overflow_arg_area: overflow.as_mut_ptr(),
            reg_save_area: reg.as_mut_ptr(),
        };
        // SAFETY: `va` describes the two buffers above, which outlive the call
        // and are laid out exactly as the ABI specifies.  No conversion pulls
        // a float argument, so the XMM half is never consulted.
        unsafe { vsscanf(input, fmt, &raw mut va) }
    }

    // -- %d signed integer tests --

    #[test]
    fn scan_d_basic() {
        let mut val: i32 = 0;
        let args = [&raw mut val as u64];
        let n = sscanf_va(b"42\0".as_ptr(), b"%d\0".as_ptr(), &args);
        assert_eq!(n, 1);
        assert_eq!(val, 42);
    }

    #[test]
    fn scan_d_negative() {
        let mut val: i32 = 0;
        let args = [&raw mut val as u64];
        let n = sscanf_va(b"-17\0".as_ptr(), b"%d\0".as_ptr(), &args);
        assert_eq!(n, 1);
        assert_eq!(val, -17);
    }

    #[test]
    fn scan_d_positive_sign() {
        let mut val: i32 = 0;
        let args = [&raw mut val as u64];
        let n = sscanf_va(b"+99\0".as_ptr(), b"%d\0".as_ptr(), &args);
        assert_eq!(n, 1);
        assert_eq!(val, 99);
    }

    #[test]
    fn scan_d_leading_whitespace() {
        let mut val: i32 = 0;
        let args = [&raw mut val as u64];
        let n = sscanf_va(b"   123\0".as_ptr(), b"%d\0".as_ptr(), &args);
        assert_eq!(n, 1);
        assert_eq!(val, 123);
    }

    #[test]
    fn scan_d_zero() {
        let mut val: i32 = 99;
        let args = [&raw mut val as u64];
        let n = sscanf_va(b"0\0".as_ptr(), b"%d\0".as_ptr(), &args);
        assert_eq!(n, 1);
        assert_eq!(val, 0);
    }

    #[test]
    fn scan_d_multiple() {
        let mut a: i32 = 0;
        let mut b: i32 = 0;
        let args = [&raw mut a as u64, &raw mut b as u64];
        let n = sscanf_va(b"10 20\0".as_ptr(), b"%d %d\0".as_ptr(), &args);
        assert_eq!(n, 2);
        assert_eq!(a, 10);
        assert_eq!(b, 20);
    }

    #[test]
    fn scan_d_stops_at_non_digit() {
        let mut val: i32 = 0;
        let args = [&raw mut val as u64];
        let n = sscanf_va(b"42abc\0".as_ptr(), b"%d\0".as_ptr(), &args);
        assert_eq!(n, 1);
        assert_eq!(val, 42);
    }

    #[test]
    fn scan_d_empty_input_eof() {
        let mut val: i32 = 99;
        let args = [&raw mut val as u64];
        let n = sscanf_va(b"\0".as_ptr(), b"%d\0".as_ptr(), &args);
        assert_eq!(n, -1); // EOF
        assert_eq!(val, 99); // Unchanged.
    }

    #[test]
    fn scan_d_no_digits() {
        let mut val: i32 = 99;
        let args = [&raw mut val as u64];
        let n = sscanf_va(b"abc\0".as_ptr(), b"%d\0".as_ptr(), &args);
        assert_eq!(n, 0);
        assert_eq!(val, 99);
    }

    #[test]
    fn scan_d_with_width() {
        let mut val: i32 = 0;
        let args = [&raw mut val as u64];
        let n = sscanf_va(b"12345\0".as_ptr(), b"%3d\0".as_ptr(), &args);
        assert_eq!(n, 1);
        assert_eq!(val, 123);
    }

    #[test]
    fn scan_ld_long() {
        let mut val: i64 = 0;
        let args = [&raw mut val as u64];
        let n = sscanf_va(b"999999999999\0".as_ptr(), b"%ld\0".as_ptr(), &args);
        assert_eq!(n, 1);
        assert_eq!(val, 999_999_999_999i64);
    }

    // -- %u unsigned integer tests --

    #[test]
    fn scan_u_basic() {
        let mut val: u32 = 0;
        let args = [&raw mut val as u64];
        let n = sscanf_va(b"65535\0".as_ptr(), b"%u\0".as_ptr(), &args);
        assert_eq!(n, 1);
        assert_eq!(val, 65535);
    }

    // -- %x hex tests --

    #[test]
    fn scan_x_basic() {
        let mut val: u32 = 0;
        let args = [&raw mut val as u64];
        let n = sscanf_va(b"ff\0".as_ptr(), b"%x\0".as_ptr(), &args);
        assert_eq!(n, 1);
        assert_eq!(val, 0xFF);
    }

    #[test]
    fn scan_x_prefix() {
        let mut val: u32 = 0;
        let args = [&raw mut val as u64];
        let n = sscanf_va(b"0xFF\0".as_ptr(), b"%x\0".as_ptr(), &args);
        assert_eq!(n, 1);
        assert_eq!(val, 0xFF);
    }

    #[test]
    fn scan_x_upper() {
        let mut val: u32 = 0;
        let args = [&raw mut val as u64];
        let n = sscanf_va(b"DEADBEEF\0".as_ptr(), b"%X\0".as_ptr(), &args);
        assert_eq!(n, 1);
        assert_eq!(val, 0xDEAD_BEEFu32);
    }

    // -- %o octal tests --

    #[test]
    fn scan_o_basic() {
        let mut val: u32 = 0;
        let args = [&raw mut val as u64];
        let n = sscanf_va(b"77\0".as_ptr(), b"%o\0".as_ptr(), &args);
        assert_eq!(n, 1);
        assert_eq!(val, 0o77);
    }

    // -- %i auto-detect base --

    #[test]
    fn scan_i_decimal() {
        let mut val: i32 = 0;
        let args = [&raw mut val as u64];
        let n = sscanf_va(b"42\0".as_ptr(), b"%i\0".as_ptr(), &args);
        assert_eq!(n, 1);
        assert_eq!(val, 42);
    }

    #[test]
    fn scan_i_hex() {
        let mut val: i32 = 0;
        let args = [&raw mut val as u64];
        let n = sscanf_va(b"0xff\0".as_ptr(), b"%i\0".as_ptr(), &args);
        assert_eq!(n, 1);
        assert_eq!(val, 255);
    }

    #[test]
    fn scan_i_octal() {
        let mut val: i32 = 0;
        let args = [&raw mut val as u64];
        let n = sscanf_va(b"010\0".as_ptr(), b"%i\0".as_ptr(), &args);
        assert_eq!(n, 1);
        assert_eq!(val, 8);
    }

    #[test]
    fn scan_i_negative_hex() {
        let mut val: i32 = 0;
        let args = [&raw mut val as u64];
        let n = sscanf_va(b"-0x10\0".as_ptr(), b"%i\0".as_ptr(), &args);
        assert_eq!(n, 1);
        assert_eq!(val, -16);
    }

    // -- %s string tests --

    #[test]
    fn scan_s_basic() {
        let mut buf = [0u8; 64];
        let args = [buf.as_mut_ptr() as u64];
        let n = sscanf_va(b"hello\0".as_ptr(), b"%s\0".as_ptr(), &args);
        assert_eq!(n, 1);
        assert_eq!(&buf[..5], b"hello");
        assert_eq!(buf[5], 0);
    }

    #[test]
    fn scan_s_stops_at_whitespace() {
        let mut buf = [0u8; 64];
        let args = [buf.as_mut_ptr() as u64];
        let n = sscanf_va(b"hello world\0".as_ptr(), b"%s\0".as_ptr(), &args);
        assert_eq!(n, 1);
        assert_eq!(&buf[..5], b"hello");
        assert_eq!(buf[5], 0);
    }

    #[test]
    fn scan_s_with_width() {
        let mut buf = [0u8; 64];
        let args = [buf.as_mut_ptr() as u64];
        let n = sscanf_va(b"longstring\0".as_ptr(), b"%4s\0".as_ptr(), &args);
        assert_eq!(n, 1);
        assert_eq!(&buf[..4], b"long");
        assert_eq!(buf[4], 0);
    }

    #[test]
    fn scan_s_multiple() {
        let mut buf1 = [0u8; 64];
        let mut buf2 = [0u8; 64];
        let args = [buf1.as_mut_ptr() as u64, buf2.as_mut_ptr() as u64];
        let n = sscanf_va(
            b"hello world\0".as_ptr(),
            b"%s %s\0".as_ptr(), &args,
        );
        assert_eq!(n, 2);
        assert_eq!(&buf1[..5], b"hello");
        assert_eq!(&buf2[..5], b"world");
    }

    #[test]
    fn scan_s_leading_whitespace() {
        let mut buf = [0u8; 64];
        let args = [buf.as_mut_ptr() as u64];
        let n = sscanf_va(b"  \t  foo\0".as_ptr(), b"%s\0".as_ptr(), &args);
        assert_eq!(n, 1);
        assert_eq!(&buf[..3], b"foo");
    }

    // -- %c character tests --

    #[test]
    fn scan_c_single() {
        let mut ch: u8 = 0;
        let args = [&raw mut ch as u64];
        let n = sscanf_va(b"A\0".as_ptr(), b"%c\0".as_ptr(), &args);
        assert_eq!(n, 1);
        assert_eq!(ch, b'A');
    }

    #[test]
    fn scan_c_no_whitespace_skip() {
        let mut ch: u8 = 0;
        let args = [&raw mut ch as u64];
        let n = sscanf_va(b" X\0".as_ptr(), b"%c\0".as_ptr(), &args);
        assert_eq!(n, 1);
        assert_eq!(ch, b' ');
    }

    #[test]
    fn scan_c_with_width() {
        let mut buf = [0u8; 8];
        let args = [buf.as_mut_ptr() as u64];
        let n = sscanf_va(b"ABCDE\0".as_ptr(), b"%3c\0".as_ptr(), &args);
        assert_eq!(n, 1);
        assert_eq!(&buf[..3], b"ABC");
    }

    // -- %n position tests --

    #[test]
    fn scan_n_position() {
        let mut val: i32 = 0;
        let mut pos: i32 = 0;
        let args = [&raw mut val as u64, &raw mut pos as u64];
        let n = sscanf_va(
            b"hello 42\0".as_ptr(),
            b"%*s %d%n\0".as_ptr(), &args,
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
        let n = sscanf_va(b"%42\0".as_ptr(), b"%%%d\0".as_ptr(), &args);
        assert_eq!(n, 1);
        assert_eq!(val, 42);
    }

    #[test]
    fn scan_percent_mismatch() {
        let mut val: i32 = 99;
        let args = [&raw mut val as u64];
        let n = sscanf_va(b"X42\0".as_ptr(), b"%%%d\0".as_ptr(), &args);
        assert_eq!(n, 0);
        assert_eq!(val, 99);
    }

    // -- Literal character matching --

    #[test]
    fn scan_literal_match() {
        let mut a: i32 = 0;
        let mut b: i32 = 0;
        let args = [&raw mut a as u64, &raw mut b as u64];
        let n = sscanf_va(b"10,20\0".as_ptr(), b"%d,%d\0".as_ptr(), &args);
        assert_eq!(n, 2);
        assert_eq!(a, 10);
        assert_eq!(b, 20);
    }

    #[test]
    fn scan_literal_mismatch() {
        let mut a: i32 = 0;
        let mut b: i32 = 99;
        let args = [&raw mut a as u64, &raw mut b as u64];
        let n = sscanf_va(b"10;20\0".as_ptr(), b"%d,%d\0".as_ptr(), &args);
        assert_eq!(n, 1);
        assert_eq!(a, 10);
        assert_eq!(b, 99);
    }

    // -- Suppression (*) --

    #[test]
    fn scan_suppression() {
        let mut val: i32 = 0;
        let args = [&raw mut val as u64];
        let n = sscanf_va(
            b"ignored 42\0".as_ptr(),
            b"%*s %d\0".as_ptr(), &args,
        );
        assert_eq!(n, 1);
        assert_eq!(val, 42);
    }

    #[test]
    fn scan_suppression_int() {
        let mut val: i32 = 0;
        let args = [&raw mut val as u64];
        let n = sscanf_va(b"100 200\0".as_ptr(), b"%*d %d\0".as_ptr(), &args);
        assert_eq!(n, 1);
        assert_eq!(val, 200);
    }

    // -- Null input/format --

    #[test]
    fn scan_null_input() {
        let n = sscanf_va(core::ptr::null(), b"%d\0".as_ptr(), &[]);
        assert_eq!(n, -1);
    }

    #[test]
    fn scan_null_format() {
        let n = sscanf_va(b"42\0".as_ptr(), core::ptr::null(), &[]);
        assert_eq!(n, -1);
    }

    // -- %f float tests --

    #[test]
    fn scan_f_basic() {
        let mut val: f32 = 0.0;
        let args = [&raw mut val as u64];
        let n = sscanf_va(b"3.14\0".as_ptr(), b"%f\0".as_ptr(), &args);
        assert_eq!(n, 1);
        assert!((val - 3.14).abs() < 0.001, "got {val}");
    }

    #[test]
    fn scan_lf_double() {
        let mut val: f64 = 0.0;
        let args = [&raw mut val as u64];
        let n = sscanf_va(b"2.718281828\0".as_ptr(), b"%lf\0".as_ptr(), &args);
        assert_eq!(n, 1);
        assert!((val - 2.718281828).abs() < 1e-9, "got {val}");
    }

    #[test]
    fn scan_capital_l_stores_a_full_long_double() {
        // `%Lf` must consume the `L` (otherwise it reads as the conversion
        // character, matches nothing, and assigns zero fields) and then store
        // all 16 bytes of an x87 `long double`.
        let mut val = crate::x87::LongDouble::ZERO;
        let args = [&raw mut val as u64];
        let n = sscanf_va(b"2.5\0".as_ptr(), b"%Lf\0".as_ptr(), &args);
        assert_eq!(n, 1);
        assert_eq!(crate::x87::to_f64(val), 2.5);
    }

    #[test]
    fn scan_capital_l_overwrites_a_stale_exponent() {
        // Storing only 8 bytes would leave the previous sign/exponent half in
        // place, so the destination would decode as something unrelated to
        // the input.  Pre-poison it with a value of a very different
        // magnitude and sign to make that failure mode detectable.
        let mut val = crate::x87::from_f64(-1e30);
        let args = [&raw mut val as u64];
        let n = sscanf_va(b"0.125\0".as_ptr(), b"%Lf\0".as_ptr(), &args);
        assert_eq!(n, 1);
        assert_eq!(crate::x87::to_f64(val), 0.125);
    }

    #[test]
    fn scan_capital_l_does_not_desync_later_fields() {
        let mut ld = crate::x87::LongDouble::ZERO;
        let mut num: i32 = 0;
        let args = [&raw mut ld as u64, &raw mut num as u64];
        let n = sscanf_va(b"1.5 7\0".as_ptr(), b"%Lf %d\0".as_ptr(), &args);
        assert_eq!(n, 2);
        assert_eq!(crate::x87::to_f64(ld), 1.5);
        assert_eq!(num, 7);
    }

    #[test]
    fn scan_size_and_intmax_modifiers() {
        // z/j/t were never consumed either, so `%zu` read `z` as the
        // conversion character and assigned nothing.  All three are 64-bit
        // on LP64.
        let mut a: u64 = 0;
        let mut b: i64 = 0;
        let mut c: i64 = 0;
        let args = [&raw mut a as u64, &raw mut b as u64, &raw mut c as u64];
        let n = sscanf_va(
            b"12 -34 56\0".as_ptr(),
            b"%zu %jd %td\0".as_ptr(), &args,
        );
        assert_eq!(n, 3);
        assert_eq!(a, 12);
        assert_eq!(b, -34);
        assert_eq!(c, 56);
    }

    #[test]
    fn scan_f_negative() {
        let mut val: f32 = 0.0;
        let args = [&raw mut val as u64];
        let n = sscanf_va(b"-1.5\0".as_ptr(), b"%f\0".as_ptr(), &args);
        assert_eq!(n, 1);
        assert!((val - (-1.5)).abs() < 0.001, "got {val}");
    }

    #[test]
    fn scan_f_scientific() {
        let mut val: f64 = 0.0;
        let args = [&raw mut val as u64];
        let n = sscanf_va(b"1.5e3\0".as_ptr(), b"%lf\0".as_ptr(), &args);
        assert_eq!(n, 1);
        assert!((val - 1500.0).abs() < 0.001, "got {val}");
    }

    #[test]
    fn scan_f_integer() {
        let mut val: f32 = 0.0;
        let args = [&raw mut val as u64];
        let n = sscanf_va(b"42\0".as_ptr(), b"%f\0".as_ptr(), &args);
        assert_eq!(n, 1);
        assert!((val - 42.0).abs() < 0.001, "got {val}");
    }

    #[test]
    fn scan_f_consumes_a_number_longer_than_any_buffer() {
        // The old collector stopped at 62 characters and, crucially, stopped
        // *consuming* there too — so the tail and the exponent stayed in the
        // input.  "1" + 70 zeros + "e-70" came back as 1e61 with "e-70" left
        // over to derail the next conversion.  It is exactly 1.0.
        let mut text = String::from("1");
        for _ in 0..70 {
            text.push('0');
        }
        text.push_str("e-70 rest");
        let mut input = text.into_bytes();
        input.push(0);

        let mut val: f64 = 0.0;
        let mut word = [0u8; 16];
        let args = [&raw mut val as u64, word.as_mut_ptr() as u64];
        let n = sscanf_va(input.as_ptr(), b"%lf %s\0".as_ptr(), &args);
        assert_eq!(n, 2);
        assert_eq!(val, 1.0, "got {val}");
        assert_eq!(&word[..4], b"rest");
    }


    // -----------------------------------------------------------------------
    // %f with hexadecimal floats
    //
    // Expectations follow glibc, which was run on each of these inputs.  The
    // two documented divergences are noted where they arise.
    // -----------------------------------------------------------------------

    /// Scan one `%lf` plus a trailing `%s`, and report `(n, value, rest)`.
    fn scan_hex(input: &str, fmt: &[u8]) -> (i32, f64, String) {
        let mut inp = input.as_bytes().to_vec();
        inp.push(0);
        let mut val: f64 = -1.0;
        let mut word = [0u8; 32];
        let args = [&raw mut val as u64, word.as_mut_ptr() as u64];
        let n = sscanf_va(inp.as_ptr(), fmt.as_ptr(), &args);
        let end = word.iter().position(|&b| b == 0).unwrap_or(word.len());
        (n, val, String::from_utf8_lossy(&word[..end]).into_owned())
    }

    #[test]
    fn scan_f_reads_hexadecimal_floats() {
        assert_eq!(scan_hex("0x1.8p+1rest", b"%lf%s\0"), (2, 3.0, "rest".into()));
        assert_eq!(scan_hex("0x1", b"%lf%s\0").1, 1.0);
        assert_eq!(scan_hex("0X1P+3", b"%lf%s\0").1, 8.0);
        assert_eq!(scan_hex("0x.8", b"%lf%s\0").1, 0.5);
        assert_eq!(scan_hex("0x1.8", b"%lf%s\0").1, 1.5);
        // 'e' is a hex digit, and the literal stops at the second 'x'.
        assert_eq!(scan_hex("0x1e5", b"%lf%s\0").1, 485.0);
        assert_eq!(scan_hex("0x1x", b"%lf%s\0"), (2, 1.0, "x".into()));
    }

    /// `scanf` consumes the longest sequence that is a *prefix* of a matching
    /// one, so a `0x` with no digit after it is consumed and then fails —
    /// it cannot back out the way `strtod` does.
    #[test]
    fn scan_f_fails_on_a_bare_hex_prefix() {
        assert_eq!(scan_hex("0xz", b"%lf%s\0").0, 0);
        assert_eq!(scan_hex("0x", b"%lf%s\0").0, 0);
    }

    /// A field width is a truncation of the input rather than a description of
    /// it, so a width stopping before the first hex digit leaves an ordinary
    /// decimal `0` with the `x` unread.
    #[test]
    fn scan_f_hex_respects_the_field_width() {
        assert_eq!(scan_hex("0x1", b"%2lf%s\0"), (2, 0.0, "x1".into()));
        assert_eq!(scan_hex("0x1", b"%1lf%s\0"), (2, 0.0, "x1".into()));
        assert_eq!(scan_hex("0x1", b"%3lf%s\0"), (1, 1.0, String::new()));
        // The width can also cut inside the literal.
        assert_eq!(scan_hex("0x1.8p+1", b"%4lf%s\0"), (2, 1.0, "8p+1".into()));
        assert_eq!(scan_hex("0x1.8p+1", b"%6lf%s\0"), (2, 1.5, "+1".into()));
        assert_eq!(scan_hex("0x1p+1", b"%5lf%s\0"), (2, 1.0, "1".into()));
    }

    #[test]
    fn scan_f_hex_is_correctly_rounded() {
        // The same ties the strtod tests pin, reached through scanf.
        assert_eq!(scan_hex("0x1.00000000000008p+0", b"%lf%s\0").1, 1.0);
        assert_eq!(
            scan_hex("0x1.00000000000018p+0", b"%lf%s\0").1,
            f64::from_bits(1.0f64.to_bits() + 2)
        );
        assert_eq!(scan_hex("0x1p-1075", b"%lf%s\0").1, 0.0);
        assert_eq!(scan_hex("0x1.8p-1075", b"%lf%s\0").1, f64::from_bits(1));
        assert_eq!(scan_hex("0x1.fffffffffffffp+1023", b"%lf%s\0").1, f64::MAX);
    }

    /// `%f` without a length modifier stores an `f32`, rounded once from the
    /// hex digits rather than narrowed from an `f64`.
    #[test]
    fn scan_f_hex_rounds_to_f32_directly() {
        let mut inp = b"0x1.999999999999ap-4\0".to_vec();
        inp.push(0);
        let mut val: f32 = -1.0;
        let args = [&raw mut val as u64];
        let n = sscanf_va(inp.as_ptr(), b"%f\0".as_ptr(), &args);
        assert_eq!(n, 1);
        assert_eq!(val, 0.1f32);
    }

    #[test]
    fn scan_f_is_correctly_rounded() {
        // Delegating to the exact converter means %lf reaches the ends of the
        // range that the old float-accumulating parser could not.
        for (text, want) in [
            ("1.7976931348623157e308\0", f64::MAX),
            ("5e-324\0", f64::from_bits(1)),
            ("0.1\0", 0.1_f64),
            ("9007199254740993\0", 9_007_199_254_740_992.0_f64),
        ] {
            let mut val: f64 = 0.0;
            let args = [&raw mut val as u64];
            let n = sscanf_va(text.as_ptr(), b"%lf\0".as_ptr(), &args);
            assert_eq!(n, 1, "{text}");
            assert_eq!(val, want, "{text}");
        }
    }

    #[test]
    fn scan_f_accepts_inf_and_nan() {
        // C99 7.21.6.2p12: the float conversions match strtod's subject
        // sequence, which includes INF/INFINITY/NAN.
        for (text, check) in [
            ("inf\0", 0u8),
            ("INFINITY\0", 0),
            ("-Inf\0", 1),
            ("nan\0", 2),
            ("NaN(quiet_1)\0", 2),
        ] {
            let mut val: f64 = 0.0;
            let args = [&raw mut val as u64];
            let n = sscanf_va(text.as_ptr(), b"%lf\0".as_ptr(), &args);
            assert_eq!(n, 1, "{text}");
            match check {
                0 => assert_eq!(val, f64::INFINITY, "{text}"),
                1 => assert_eq!(val, f64::NEG_INFINITY, "{text}"),
                _ => assert!(val.is_nan(), "{text}"),
            }
        }
    }

    #[test]
    fn scan_f_only_consumes_a_complete_named_value() {
        // "info" starts like "inf" but the trailing "o" must survive, and a
        // bare "in" is not a match at all.
        let mut val: f64 = 0.0;
        let mut word = [0u8; 16];
        let args = [&raw mut val as u64, word.as_mut_ptr() as u64];
        let n = sscanf_va(b"info\0".as_ptr(), b"%lf%s\0".as_ptr(), &args);
        assert_eq!(n, 2);
        assert_eq!(val, f64::INFINITY);
        assert_eq!(&word[..1], b"o");

        let mut val2: f64 = -1.0;
        let args = [&raw mut val2 as u64];
        assert_eq!(sscanf_va(b"in\0".as_ptr(), b"%lf\0".as_ptr(), &args), 0);
        assert_eq!(val2, -1.0, "a failed match must not assign");
    }

    #[test]
    fn scan_f_respects_an_explicit_width() {
        let mut a: f64 = 0.0;
        let mut b: f64 = 0.0;
        let args = [&raw mut a as u64, &raw mut b as u64];
        let n = sscanf_va(b"1.2534\0".as_ptr(), b"%4lf%lf\0".as_ptr(), &args);
        assert_eq!(n, 2);
        assert_eq!(a, 1.25);
        assert_eq!(b, 34.0);
        // The width can cut an exponent short.  Six characters reach "1.25e3",
        // which is a complete number.
        let mut c: f64 = 0.0;
        let args = [&raw mut c as u64];
        assert_eq!(sscanf_va(b"1.25e34\0".as_ptr(), b"%6lf\0".as_ptr(), &args), 1);
        assert_eq!(c, 1250.0);

        // Five characters reach "1.25e", which is not — but the 'e' is still
        // consumed, because a directive takes the longest sequence that is a
        // *prefix* of a matching one and "1.25e" is a prefix of "1.25e3".  The
        // value comes from the digits actually read, so "34" is what is left.
        // (glibc agrees: n=2, d=1.25, rest "34".)
        let mut d: f64 = 0.0;
        let mut word = [0u8; 8];
        let args = [&raw mut d as u64, word.as_mut_ptr() as u64];
        assert_eq!(
            sscanf_va(b"1.25e34\0".as_ptr(), b"%5lf%s\0".as_ptr(), &args),
            2
        );
        assert_eq!(d, 1.25);
        assert_eq!(&word[..2], b"34");
    }

    /// The exponent marker is part of the item even when nothing follows it,
    /// so it does not leak into the next conversion.  All of these were run
    /// against glibc.
    #[test]
    fn scan_f_swallows_a_dangling_exponent_marker() {
        assert_eq!(scan_hex("1.5e", b"%lf%s\0"), (1, 1.5, String::new()));
        assert_eq!(scan_hex("1.5e+", b"%lf%s\0"), (1, 1.5, String::new()));
        assert_eq!(scan_hex("1.5e-", b"%lf%s\0"), (1, 1.5, String::new()));
        assert_eq!(scan_hex("1.5ex", b"%lf%s\0"), (2, 1.5, "x".into()));
        assert_eq!(scan_hex("1.5e+x", b"%lf%s\0"), (2, 1.5, "x".into()));
        assert_eq!(scan_hex("0x1.8p", b"%lf%s\0"), (1, 1.5, String::new()));
        assert_eq!(scan_hex("0x1.8p+", b"%lf%s\0"), (1, 1.5, String::new()));
        assert_eq!(scan_hex("0x1.8px", b"%lf%s\0"), (2, 1.5, "x".into()));
    }

    /// `inf` may be extended to `infinity`, and a *partial* extension is
    /// consumed but has no value — a matching failure.  Checked against glibc.
    #[test]
    fn scan_f_rejects_a_partial_infinity() {
        assert_eq!(scan_hex("infix", b"%lf%s\0").0, 0);
        assert_eq!(scan_hex("infinit", b"%lf%s\0").0, 0);
        assert_eq!(scan_hex("INFI", b"%lf%s\0").0, 0);
        assert_eq!(scan_hex("infinity", b"%4lf%s\0").0, 0);
        assert_eq!(scan_hex("infinity", b"%7lf%s\0").0, 0);
        // A complete "inf" or "infinity" still converts, and a width that
        // stops exactly at "inf" leaves the rest for the next conversion.
        assert_eq!(scan_hex("infx", b"%lf%s\0"), (2, f64::INFINITY, "x".into()));
        assert_eq!(
            scan_hex("infinityx", b"%lf%s\0"),
            (2, f64::INFINITY, "x".into())
        );
        assert_eq!(
            scan_hex("infinity", b"%3lf%s\0"),
            (2, f64::INFINITY, "inity".into())
        );
        assert_eq!(
            scan_hex("infinity", b"%8lf%s\0"),
            (1, f64::INFINITY, String::new())
        );
        // Too short to be even "inf"/"nan": nothing is consumed, so the
        // digit grammar is tried and finds nothing.
        assert_eq!(scan_hex("inf", b"%2lf%s\0").0, 0);
        assert_eq!(scan_hex("nan", b"%2lf%s\0").0, 0);
    }

    /// EOF (-1) is reserved for an input failure — the input running out
    /// before a directive matched anything.  A directive that read characters
    /// and then found no value in them is a *matching* failure and reports the
    /// assignment count, even though it too ends at end of input.
    #[test]
    fn scan_distinguishes_input_failure_from_matching_failure() {
        // Nothing at all, or only whitespace: input failure.
        assert_eq!(scan_hex("", b"%lf\0").0, -1);
        assert_eq!(scan_hex("   ", b"%lf\0").0, -1);
        assert_eq!(scan_hex("", b"%d\0").0, -1);
        // A literal that never got its character is an input failure too.
        assert_eq!(scan_hex("", b"x\0").0, -1);
        assert_eq!(scan_hex("", b"%%\0").0, -1);
        // Characters were read: matching failure, so 0 rather than EOF.
        assert_eq!(scan_hex("0x", b"%lf\0").0, 0);
        assert_eq!(scan_hex("+", b"%lf\0").0, 0);
        assert_eq!(scan_hex("abc", b"%d\0").0, 0);
        assert_eq!(scan_hex("a", b"%%\0").0, 0);
        // An empty format assigns nothing but never reports EOF.
        assert_eq!(scan_hex("", b"\0").0, 0);
    }

    #[test]
    fn scan_f_rounds_to_f32_without_going_through_f64() {
        // %f stores a float, and it must be rounded straight from the digits.
        // This input rounds to exactly the midpoint between 1.0f and its
        // successor when taken through f64, where ties-to-even then picks the
        // wrong side.
        let text = b"1.000000059604644830901776231257827021181583404541015625\0";
        let mut val: f32 = 0.0;
        let args = [&raw mut val as u64];
        assert_eq!(sscanf_va(text.as_ptr(), b"%f\0".as_ptr(), &args), 1);
        assert_eq!(val.to_bits(), 1.0_f32.to_bits() + 1);
    }

    // -- %[...] scanset tests --

    #[test]
    fn scan_scanset_basic() {
        let mut buf = [0u8; 64];
        let args = [buf.as_mut_ptr() as u64];
        let n = sscanf_va(b"abc123\0".as_ptr(), b"%[abc]\0".as_ptr(), &args);
        assert_eq!(n, 1);
        assert_eq!(&buf[..3], b"abc");
        assert_eq!(buf[3], 0);
    }

    #[test]
    fn scan_scanset_negated() {
        let mut buf = [0u8; 64];
        let args = [buf.as_mut_ptr() as u64];
        let n = sscanf_va(
            b"hello world\0".as_ptr(),
            b"%[^ ]\0".as_ptr(), &args,
        );
        assert_eq!(n, 1);
        assert_eq!(&buf[..5], b"hello");
        assert_eq!(buf[5], 0);
    }

    #[test]
    fn scan_scanset_range() {
        let mut buf = [0u8; 64];
        let args = [buf.as_mut_ptr() as u64];
        let n = sscanf_va(b"abcXYZ\0".as_ptr(), b"%[a-z]\0".as_ptr(), &args);
        assert_eq!(n, 1);
        assert_eq!(&buf[..3], b"abc");
        assert_eq!(buf[3], 0);
    }

    #[test]
    fn scan_scanset_leading_bracket() {
        let mut buf = [0u8; 64];
        let args = [buf.as_mut_ptr() as u64];
        let n = sscanf_va(b"]ab\0".as_ptr(), b"%[]ab]\0".as_ptr(), &args);
        assert_eq!(n, 1);
        assert_eq!(&buf[..3], b"]ab");
    }

    #[test]
    fn scan_scanset_digits() {
        let mut buf = [0u8; 64];
        let args = [buf.as_mut_ptr() as u64];
        let n = sscanf_va(b"12345abc\0".as_ptr(), b"%[0-9]\0".as_ptr(), &args);
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
        let n = sscanf_va(
            b"Alice 30 95.5\0".as_ptr(),
            b"%s %d %f\0".as_ptr(), &args,
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
        let n = sscanf_va(b"42 xyz\0".as_ptr(), b"%d %d\0".as_ptr(), &args);
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
        let n = sscanf_va(
            b"10\t\t\n  20\0".as_ptr(),
            b"%d %d\0".as_ptr(), &args,
        );
        assert_eq!(n, 2);
        assert_eq!(a, 10);
        assert_eq!(b, 20);
    }

    // -- Edge cases --

    #[test]
    fn scan_empty_format() {
        let n = sscanf_va(b"hello\0".as_ptr(), b"\0".as_ptr(), &[]);
        assert_eq!(n, 0);
    }

    /// Regression for BUG-POSIX-SCANF-ARG-ARRAY-OOB.
    ///
    /// The engine used to flatten the destination pointers into a `[u64; 8]`.
    /// The ninth conversion read one word past that array and stored through
    /// whatever it found — an arbitrary stack write, not merely a wrong value.
    /// Twelve conversions here exercise both halves of the argument path: six
    /// pointers come from the integer register save area and six from the
    /// overflow area.
    #[test]
    fn scan_more_than_eight_conversions() {
        let mut vals = [0i32; 12];
        let ptrs: Vec<u64> = vals.iter_mut().map(|v| &raw mut *v as u64).collect();
        let n = sscanf_va(
            b"1 2 3 4 5 6 7 8 9 10 11 12\0".as_ptr(),
            b"%d %d %d %d %d %d %d %d %d %d %d %d\0".as_ptr(),
            &ptrs,
        );
        assert_eq!(n, 12);
        assert_eq!(vals, [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]);
    }

    /// The same past-the-eighth path with mixed conversion widths, so a
    /// miscounted argument shows up as a corrupted neighbour rather than as an
    /// off-by-one that happens to land on an identically-typed slot.
    #[test]
    fn scan_more_than_eight_mixed_conversions() {
        let mut a = [0i32; 5];
        let mut wide = [0i64; 3];
        let mut buf = [0u8; 8];
        let mut b = [0i32; 3];
        let mut ptrs: Vec<u64> = a.iter_mut().map(|v| &raw mut *v as u64).collect();
        ptrs.extend(wide.iter_mut().map(|v| &raw mut *v as u64));
        ptrs.push(buf.as_mut_ptr() as u64);
        ptrs.extend(b.iter_mut().map(|v| &raw mut *v as u64));

        let n = sscanf_va(
            b"1 2 3 4 5 60 70 80 word 9 10 11\0".as_ptr(),
            b"%d %d %d %d %d %ld %ld %ld %s %d %d %d\0".as_ptr(),
            &ptrs,
        );
        assert_eq!(n, 12);
        assert_eq!(a, [1, 2, 3, 4, 5]);
        assert_eq!(wide, [60, 70, 80]);
        assert_eq!(&buf[..5], b"word\0");
        assert_eq!(b, [9, 10, 11]);
    }

    #[test]
    fn scan_three_ints() {
        let mut a: i32 = 0;
        let mut b: i32 = 0;
        let mut c: i32 = 0;
        let args = [&raw mut a as u64, &raw mut b as u64, &raw mut c as u64];
        let n = sscanf_va(b"1 2 3\0".as_ptr(), b"%d %d %d\0".as_ptr(), &args);
        assert_eq!(n, 3);
        assert_eq!(a, 1);
        assert_eq!(b, 2);
        assert_eq!(c, 3);
    }

    #[test]
    fn scan_hex_long() {
        let mut val: u64 = 0;
        let args = [&raw mut val as u64];
        let n = sscanf_va(
            b"0xDEADBEEFCAFE\0".as_ptr(),
            b"%lx\0".as_ptr(), &args,
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
        let n = sscanf_va(b"0xG\0".as_ptr(), b"%x\0".as_ptr(), &args);
        assert_eq!(n, 1);
        assert_eq!(val, 0);
    }

    #[test]
    fn scan_hex_just_zero() {
        // "0" alone should parse as hex value 0.
        let mut val: u32 = 99;
        let args = [&raw mut val as u64];
        let n = sscanf_va(b"0\0".as_ptr(), b"%x\0".as_ptr(), &args);
        assert_eq!(n, 1);
        assert_eq!(val, 0);
    }

    #[test]
    fn scan_hex_width_limits_prefix() {
        // "%1x" on "0xFF" — width=1, so only "0" is consumed (1 char).
        // The prefix "0x" would need width >= 3 to be useful.
        let mut val: u32 = 99;
        let args = [&raw mut val as u64];
        let n = sscanf_va(b"0xFF\0".as_ptr(), b"%1x\0".as_ptr(), &args);
        assert_eq!(n, 1);
        assert_eq!(val, 0);
    }

    #[test]
    fn scan_hex_width_3_parses_one_digit_after_prefix() {
        // "%3x" on "0xFF" — width=3: "0x" prefix (2) + "F" (1) = 3 total.
        let mut val: u32 = 0;
        let args = [&raw mut val as u64];
        let n = sscanf_va(b"0xFF\0".as_ptr(), b"%3x\0".as_ptr(), &args);
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
        let n = sscanf_va(
            b"42\0".as_ptr(),
            b"%99999999999999999999d\0".as_ptr(), &args,
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
    fn vsscanf_long_double_and_size_modifiers() {
        // The va_list prescan has its own copy of the modifier parser; if it
        // did not skip `L`/`z` it would hand out the wrong pointers and the
        // second field would be written through the first field's address.
        let mut ld = crate::x87::LongDouble::ZERO;
        let mut sz: u64 = 0;
        let n = run_vsscanf(
            b"6.25 99\0",
            b"%Lf %zu\0",
            &[&raw mut ld as u64, &raw mut sz as u64],
        );
        assert_eq!(n, 2);
        assert_eq!(crate::x87::to_f64(ld), 6.25);
        assert_eq!(sz, 99);
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
