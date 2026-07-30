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
//!
//! ## The v* family
//!
//! `vprintf`, `vfprintf`, `vdprintf`, `vsnprintf`, `vsprintf`, and
//! `vasprintf` take a `va_list` instead of `...`.  On the x86_64 System V
//! ABI a `va_list` parameter decays to a pointer to a `__va_list_tag`
//! struct, so these are ordinary (non-variadic) functions: they walk the
//! `va_list` per the SysV `va_arg` rules, flatten the arguments into the
//! same int/float arrays the engine uses, and delegate to the matching
//! `_*_impl`.  Being non-variadic, they are pure Rust and host-testable.

// ---------------------------------------------------------------------------
// Assembly trampolines
// ---------------------------------------------------------------------------
//
// Rust cannot define a C variadic function, so each `printf`-family symbol is
// an assembly stub.  Each one performs a real SysV `va_start` — spill the six
// integer argument registers and the eight XMM registers into a register save
// area, then build the 24-byte `va_list` descriptor pointing at it — and
// delegates to the matching `v*` function, which is ordinary Rust.
//
// This is a rewrite of an earlier design that flattened the arguments into two
// fixed `[u64; 8]` arrays instead.  That shortcut could not represent a
// stack-passed argument at all, so `%Lf` (a `long double` is X87/X87UP ->
// MEMORY, always on the stack) was unreachable from `printf` even after the
// engine learned to format it — see BUG-POSIX-LONG-DOUBLE-ABI.  It also meant
// the direct and `v*` families reached the engine by two different routes, so
// a fix to one silently missed the other.  Going through a genuine `va_list`
// removes both problems and deletes ~200 lines of near-duplicated assembly.
//
// Named-argument counts (which set the initial `gp_offset` and decide which
// register carries the `va_list*`):
//   printf(fmt, ...)                  1 named -> gp_offset 8,  ap in rsi
//   fprintf(stream, fmt, ...)         2 named -> gp_offset 16, ap in rdx
//   dprintf(fd, fmt, ...)             2 named -> gp_offset 16, ap in rdx
//   snprintf(buf, size, fmt, ...)     3 named -> gp_offset 24, ap in rcx
//   sprintf(buf, fmt, ...)            2 named -> gp_offset 16, ap in rdx
//   asprintf(strp, fmt, ...)          2 named -> gp_offset 16, ap in rdx

// The `_*_impl` symbols defined in this module are the Rust-side targets of
// the fortified (`__*_chk`) trampolines in `fortify_printf.rs`.  The leading
// underscore is part of the ABI contract and marks them as
// "private-but-exported" linkage symbols (libc programs link against the
// un-prefixed `printf`/`fprintf`/...).
#![allow(clippy::used_underscore_items)]

/// Emit a variadic trampoline that performs a real `va_start` and tail-calls
/// the corresponding `v*` implementation.
///
/// * `$name` — the exported C symbol.
/// * `$target` — the `v*` function to call; it must take the same named
///   parameters followed by a `*mut VaList`.
/// * `$gp_off` — the initial `gp_offset`: 8 × the number of *named* integer
///   parameters, i.e. how much of the integer register file the named
///   arguments have already consumed.
/// * `$ap_reg` — the register that carries the `va_list*` into `$target`,
///   which is the first integer register after the named parameters.
///
/// Frame layout below `%rbp` (208 bytes, chosen so `%rsp` stays 16-byte
/// aligned for both the `movaps` stores and the `call`):
///
/// ```text
///   [rsp   .. rsp+48 )  integer register save area: rdi rsi rdx rcx r8 r9
///   [rsp+48.. rsp+176)  SSE register save area:     xmm0..xmm7, 16 bytes each
///   [rsp+176..rsp+200)  the va_list itself
/// ```
#[cfg(target_os = "none")]
macro_rules! va_trampoline {
    ($name:literal, $target:literal, $gp_off:literal, $ap_reg:literal) => {
        core::arch::global_asm!(
            concat!(".global ", $name),
            concat!(".type ", $name, ", @function"),
            concat!($name, ":"),
            "push rbp",
            "mov rbp, rsp",
            "sub rsp, 208",
            // Spill the integer argument registers first: one of them is about
            // to be overwritten with the va_list pointer.
            "mov [rsp], rdi",
            "mov [rsp+8], rsi",
            "mov [rsp+16], rdx",
            "mov [rsp+24], rcx",
            "mov [rsp+32], r8",
            "mov [rsp+40], r9",
            // The ABI lets a callee skip the SSE spill when %al is zero; we
            // spill unconditionally. It costs eight aligned stores on a path
            // that is about to do orders of magnitude more work, and it means
            // a caller that under-reports %al gets a wrong value rather than
            // whatever happened to be on the stack.
            "movaps [rsp+48], xmm0",
            "movaps [rsp+64], xmm1",
            "movaps [rsp+80], xmm2",
            "movaps [rsp+96], xmm3",
            "movaps [rsp+112], xmm4",
            "movaps [rsp+128], xmm5",
            "movaps [rsp+144], xmm6",
            "movaps [rsp+160], xmm7",
            // va_list.gp_offset / .fp_offset
            concat!("mov dword ptr [rsp+176], ", $gp_off),
            "mov dword ptr [rsp+180], 48",
            // va_list.overflow_arg_area — the first stack argument sits just
            // past the saved rbp and the return address.
            "lea rax, [rbp+16]",
            "mov [rsp+184], rax",
            // va_list.reg_save_area
            "mov rax, rsp",
            "mov [rsp+192], rax",
            concat!("lea ", $ap_reg, ", [rsp+176]"),
            concat!("call ", $target),
            "add rsp, 208",
            "pop rbp",
            "ret",
            concat!(".size ", $name, ", . - ", $name),
        );
    };
}

// Shared with `fortify_printf.rs`, whose `__*_chk` entry points are variadic
// in exactly the same way and differ only in how many named parameters they
// have.
#[cfg(target_os = "none")]
pub(crate) use va_trampoline;

#[cfg(target_os = "none")]
va_trampoline!("printf", "vprintf", "8", "rsi");
#[cfg(target_os = "none")]
va_trampoline!("fprintf", "vfprintf", "16", "rdx");
#[cfg(target_os = "none")]
va_trampoline!("dprintf", "vdprintf", "16", "rdx");
#[cfg(target_os = "none")]
va_trampoline!("snprintf", "vsnprintf", "24", "rcx");
#[cfg(target_os = "none")]
va_trampoline!("sprintf", "vsprintf", "16", "rdx");
#[cfg(target_os = "none")]
va_trampoline!("asprintf", "vasprintf", "16", "rdx");

// ---------------------------------------------------------------------------
// Rust entry points (called by the fortified `__*_chk` trampolines)
// ---------------------------------------------------------------------------

/// Stack buffer size for printf/fprintf (format to buffer, then write).
const PRINTF_BUF_SIZE: usize = 4096;

/// `printf(fmt, ...)` — write formatted output to stdout.
///
/// Output goes through the stdio buffer (line-buffered on stdout) so
/// printf output is properly coalesced with other stdout writes.
pub(crate) fn _printf_impl(fmt: *const u8, args: &mut Args) -> i32 {
    let mut buf = [0u8; PRINTF_BUF_SIZE];
    let n = format_core(buf.as_mut_ptr(), PRINTF_BUF_SIZE, fmt, args);
    if n <= 0 {
        return n;
    }
    let write_len = if (n as usize) < PRINTF_BUF_SIZE {
        n as usize
    } else {
        PRINTF_BUF_SIZE
    };
    // Use STDOUT_SENTINEL (1) explicitly — dangling_mut::<u8>() happens to
    // return the same value today but is not guaranteed to.
    let ret = crate::stdio::write_stream(
        crate::stdio::STDOUT_SENTINEL as *mut u8,
        buf.as_ptr(),
        write_len,
    );
    if ret < 0 { ret as i32 } else { n }
}

/// `fprintf(stream, fmt, ...)` — write formatted output to a stream.
///
/// Output goes through the stdio buffer so fprintf output is properly
/// coalesced with other writes to the same stream.
pub(crate) fn _fprintf_impl(stream: *mut u8, fmt: *const u8, args: &mut Args) -> i32 {
    let mut buf = [0u8; PRINTF_BUF_SIZE];
    let n = format_core(buf.as_mut_ptr(), PRINTF_BUF_SIZE, fmt, args);
    if n <= 0 {
        return n;
    }
    let write_len = if (n as usize) < PRINTF_BUF_SIZE {
        n as usize
    } else {
        PRINTF_BUF_SIZE
    };
    let ret = crate::stdio::write_stream(stream, buf.as_ptr(), write_len);
    if ret < 0 { ret as i32 } else { n }
}

/// `dprintf(fd, fmt, ...)` — write formatted output to a file descriptor.
///
/// Like `fprintf` but takes a raw fd (int) instead of a `FILE*`.
/// Writes directly to the fd without stdio buffering.
pub(crate) fn _dprintf_impl(fd: i32, fmt: *const u8, args: &mut Args) -> i32 {
    let mut buf = [0u8; PRINTF_BUF_SIZE];
    let n = format_core(buf.as_mut_ptr(), PRINTF_BUF_SIZE, fmt, args);
    if n <= 0 {
        return n;
    }
    let write_len = if (n as usize) < PRINTF_BUF_SIZE {
        n as usize
    } else {
        PRINTF_BUF_SIZE
    };
    let ret = crate::file::write(fd, buf.as_ptr(), write_len);
    if ret < 0 { ret as i32 } else { n }
}

/// `snprintf(buf, size, fmt, ...)` — write formatted output to a buffer.
pub(crate) fn _snprintf_impl(buf: *mut u8, size: usize, fmt: *const u8, args: &mut Args) -> i32 {
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
    unsafe {
        *buf.add(term_pos) = 0;
    }
    n
}

/// `sprintf(buf, fmt, ...)` — write formatted output to a buffer (no limit).
pub(crate) fn _sprintf_impl(buf: *mut u8, fmt: *const u8, args: &mut Args) -> i32 {
    // No size limit — dangerous but matches C semantics.
    let n = format_core(buf, usize::MAX, fmt, args);
    // C99: "A null character is written at the end of the characters written."
    if !buf.is_null() && n >= 0 {
        // SAFETY: format_core wrote n bytes starting at buf; caller must provide
        // a buffer large enough for the output + null terminator.
        unsafe {
            *buf.add(n as usize) = 0;
        }
    }
    n
}

/// `asprintf(strp, fmt, ...)` — allocate and format a string.
///
/// Allocates a buffer large enough to hold the formatted output
/// (including null terminator) and stores a pointer to it in `*strp`.
/// Returns the number of characters written (excluding null), or -1
/// on allocation failure.
///
/// # Safety
/// `va`, when `Some`, must be a valid `va_list` snapshot matching `fmt` — see
/// [`Args::new`].  It is taken **by value** because this is the one entry
/// point that walks the format string twice (measure, then write) and a
/// `va_list` cannot be rewound: each pass starts from its own copy, which is
/// exactly what C's `va_copy` is for.
pub(crate) unsafe fn _asprintf_impl(
    strp: *mut *mut u8,
    fmt: *const u8,
    va: Option<VaList>,
) -> i32 {
    if strp.is_null() {
        return -1;
    }

    // First pass: count required bytes (format to null buffer).
    let mut measure = va;
    // SAFETY: the caller's contract on `va` is passed straight through.
    let mut margs = unsafe { Args::new(measure.as_mut()) };
    let n = format_core(core::ptr::null_mut(), 0, fmt, &mut margs);
    if n < 0 {
        unsafe {
            *strp = core::ptr::null_mut();
        }
        return -1;
    }

    let alloc_size = (n as usize).wrapping_add(1); // +1 for NUL
    let buf = crate::malloc::malloc(alloc_size);
    if buf.is_null() {
        unsafe {
            *strp = core::ptr::null_mut();
        }
        return -1;
    }

    // Second pass: format into the allocated buffer, from a fresh copy of the
    // va_list so it replays the same arguments.
    let mut write = va;
    // SAFETY: as for the measuring pass.
    let mut wargs = unsafe { Args::new(write.as_mut()) };
    let written = format_core(buf, alloc_size, fmt, &mut wargs);

    // Null-terminate.
    let term_pos = if written >= 0 && (written as usize) < alloc_size {
        written as usize
    } else {
        alloc_size.wrapping_sub(1)
    };
    // SAFETY: term_pos < alloc_size, buf is non-null.
    unsafe {
        *buf.add(term_pos) = 0;
    }

    // SAFETY: strp verified non-null.
    unsafe {
        *strp = buf;
    }
    n
}

// ---------------------------------------------------------------------------
// va_list support — the v* printf family
// ---------------------------------------------------------------------------
//
// The trampoline-based functions above capture variadic arguments straight
// from registers.  The v* variants instead receive an already-initialised
// `va_list` from a caller that itself did `va_start`.  On the x86_64 System V
// ABI a `va_list` is `__va_list_tag[1]` — an array of one struct — so a
// `va_list` parameter decays to a pointer to that struct.  These functions
// are therefore NOT variadic; they take a concrete `*mut VaList`, which means
// they can be written in pure Rust and unit-tested on the host with a
// hand-built struct (no reliance on the host's own `va_arg`).
//
// We walk the format string once, pulling each argument from the va_list in
// document order via the SysV `va_arg` algorithm (registers first, then the
// stack overflow area), and flatten them into the same `[u64; 8]` integer and
// float arrays that `format_core` consumes.  Pulling in document order is what
// keeps the overflow area consumed correctly when both integer and floating
// arguments spill past their register banks.

/// x86_64 System V `va_list` element (`__va_list_tag`).
///
/// Layout is fixed by the ABI: two register-area cursors followed by two
/// pointers.  Exposed publicly only so it can appear in the `extern "C"`
/// signatures of the v* functions; callers in C pass an ordinary `va_list`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct VaList {
    /// Byte offset into `reg_save_area` of the next general-purpose arg
    /// (0..48; once ≥48 the GP registers are exhausted).
    pub gp_offset: u32,
    /// Byte offset into `reg_save_area` of the next SSE arg
    /// (48..176; once ≥176 the XMM registers are exhausted).
    pub fp_offset: u32,
    /// Next argument on the stack (used once registers are exhausted).
    pub overflow_arg_area: *mut u8,
    /// Saved registers: 6 GP regs (0..48) then 8 XMM regs (48..176).
    pub reg_save_area: *mut u8,
}

/// Pull the next integer/pointer argument from a `va_list`.
///
/// # Safety
/// `va` must be a valid, ABI-conformant `va_list`: `reg_save_area` (when the
/// GP registers are not yet exhausted) and `overflow_arg_area` must point at
/// readable memory with at least 8 more bytes for this argument.
pub unsafe fn va_arg_int(va: &mut VaList) -> u64 {
    if (va.gp_offset as usize) < 48 && !va.reg_save_area.is_null() {
        // SAFETY: gp_offset < 48 stays within the 48-byte GP save area.
        let p = unsafe { va.reg_save_area.add(va.gp_offset as usize) };
        va.gp_offset = va.gp_offset.wrapping_add(8);
        // SAFETY: p is 8-byte aligned and within the save area.
        unsafe { (p as *const u64).read_unaligned() }
    } else {
        let area = va.overflow_arg_area;
        if area.is_null() {
            return 0;
        }
        // SAFETY: overflow_arg_area points at the next 8-byte stack slot.
        va.overflow_arg_area = unsafe { area.add(8) };
        // SAFETY: as above; the slot holds at least 8 bytes.
        unsafe { (area as *const u64).read_unaligned() }
    }
}

/// Pull the next `double` argument from a `va_list`.
///
/// Doubles live in the XMM half of `reg_save_area` (offsets 48..176, one per
/// 16-byte slot — only the low 8 bytes hold the value), then the overflow
/// area (8 bytes per slot).
///
/// # Safety
/// Same contract as [`va_arg_int`].
unsafe fn va_arg_double(va: &mut VaList) -> u64 {
    if (va.fp_offset as usize) < 176 && !va.reg_save_area.is_null() {
        // SAFETY: fp_offset < 176 stays within the XMM save area.
        let p = unsafe { va.reg_save_area.add(va.fp_offset as usize) };
        va.fp_offset = va.fp_offset.wrapping_add(16);
        // SAFETY: the low 8 bytes of the 16-byte slot hold the double.
        unsafe { (p as *const u64).read_unaligned() }
    } else {
        let area = va.overflow_arg_area;
        if area.is_null() {
            return 0;
        }
        // SAFETY: overflow_arg_area points at the next 8-byte stack slot.
        va.overflow_arg_area = unsafe { area.add(8) };
        // SAFETY: as above; the slot holds at least 8 bytes.
        unsafe { (area as *const u64).read_unaligned() }
    }
}

/// Pull the next `long double` (x87 80-bit) argument from a `va_list`,
/// returning it narrowed to the bit pattern of an `f64`.
///
/// `long double` classifies as X87/X87UP, which the SysV ABI resolves to
/// MEMORY: it is *never* passed in a register, so this touches neither
/// `gp_offset` nor `fp_offset`. It always comes from the overflow area,
/// 16-byte aligned and 16 bytes wide (10 significant, 6 padding) — which is
/// why skipping the `L` and treating the value as a `double` did not merely
/// lose precision, it desynchronised every argument after it.
///
/// The narrowing to `f64` is the sysroot's documented `long double`
/// limitation; see [`crate::x87`] and `TD-POSIX-LONG-DOUBLE-PRECISION`.
///
/// # Safety
/// Same contract as [`va_arg_int`], except the argument occupies 16 bytes.
unsafe fn va_arg_long_double(va: &mut VaList) -> u64 {
    let area = va.overflow_arg_area;
    if area.is_null() {
        return 0;
    }
    // Round the cursor up to the ABI's 16-byte alignment for this argument.
    let aligned = (area as usize).wrapping_add(15) & !15usize;
    let p = aligned as *const u8;
    va.overflow_arg_area = aligned.wrapping_add(16) as *mut u8;
    // SAFETY: the slot holds a 16-byte `long double`; we read its 10
    // meaningful bytes with unaligned reads so no alignment claim is needed.
    let significand = unsafe { p.cast::<u64>().read_unaligned() };
    // SAFETY: as above; bytes 8..10 of the same 16-byte slot.
    let sign_exp = unsafe { p.add(8).cast::<u16>().read_unaligned() };
    crate::x87::to_f64(crate::x87::LongDouble {
        significand,
        sign_exp,
        pad: [0; 6],
    })
    .to_bits()
}

/// Consume the length modifier at `*fpos`, reporting whether it was `L`.
///
/// `L` is the only modifier that changes an argument's *size*: on LP64 every
/// integer type occupies one 8-byte slot however it is spelled, but `L` on a
/// floating conversion promotes it to a 16-byte, stack-passed `long double`.
/// Both the collection pass and the formatting pass must skip the modifier
/// identically, so they share this function.
///
/// # Safety
/// `fmt` must be NUL-terminated and `*fpos` a valid index into it.
unsafe fn skip_length_modifier(fmt: *const u8, fpos: &mut usize) -> bool {
    // SAFETY: caller guarantees fmt is NUL-terminated and fpos in range.
    match unsafe { *fmt.add(*fpos) } {
        b'l' => {
            *fpos = fpos.wrapping_add(1);
            // SAFETY: as above.
            if unsafe { *fmt.add(*fpos) } == b'l' {
                *fpos = fpos.wrapping_add(1);
            }
        }
        b'h' => {
            *fpos = fpos.wrapping_add(1);
            // SAFETY: as above.
            if unsafe { *fmt.add(*fpos) } == b'h' {
                *fpos = fpos.wrapping_add(1);
            }
        }
        b'z' | b'j' | b't' => *fpos = fpos.wrapping_add(1),
        b'L' => {
            *fpos = fpos.wrapping_add(1);
            return true;
        }
        _ => {}
    }
    false
}

/// The argument source for one formatting pass.
///
/// The engine pulls each argument from the caller's `va_list` at the point of
/// use, in document order.  There is deliberately no intermediate array: an
/// earlier design flattened the arguments into two fixed `[u64; 8]`s, which
/// capped every call at eight integer and eight floating conversions (reading
/// out of bounds past that — BUG-POSIX-PRINTF-ARG-ARRAY-OOB), could not
/// represent a MEMORY-class `long double` at all, and required a second
/// format-string walk that had to stay in exact lock-step with this one.
///
/// `None` means "no arguments available" — a null `va_list`, which the C
/// standard leaves undefined but which we render as zeros rather than a fault.
pub(crate) struct Args<'a> {
    va: Option<&'a mut VaList>,
}

impl<'a> Args<'a> {
    /// Wrap a `va_list` as an argument source.
    ///
    /// # Safety
    /// If present, `va` must be a valid, ABI-conformant `va_list` with at
    /// least as many arguments as `fmt`'s conversions will request, of
    /// matching types.  This is the same contract C places on the caller of
    /// any `v*printf`, and it is checked nowhere: the whole unsafety of the
    /// printf family lives in this one constructor.
    pub(crate) const unsafe fn new(va: Option<&'a mut VaList>) -> Self {
        Self { va }
    }

    /// Wrap a possibly-null `va_list` pointer, as the C entry points receive
    /// it.  A null pointer yields an empty source rather than a fault.
    ///
    /// # Safety
    /// As [`Args::new`] when `va` is non-null.
    pub(crate) unsafe fn from_raw(va: *mut VaList) -> Self {
        if va.is_null() {
            Self::empty()
        } else {
            // SAFETY: non-null, and the caller's va_list contract carries over.
            Self { va: Some(unsafe { &mut *va }) }
        }
    }

    /// An argument source with nothing in it; every request yields zero.
    pub(crate) const fn empty() -> Self {
        Self { va: None }
    }

    /// Next integer/pointer argument.
    fn int(&mut self) -> u64 {
        match self.va.as_deref_mut() {
            // SAFETY: the va_list contract is upheld by `Args::new`.
            Some(va) => unsafe { va_arg_int(va) },
            None => 0,
        }
    }

    /// Next `double` argument, as raw bits.
    fn double(&mut self) -> u64 {
        match self.va.as_deref_mut() {
            // SAFETY: the va_list contract is upheld by `Args::new`.
            Some(va) => unsafe { va_arg_double(va) },
            None => 0,
        }
    }

    /// Next `long double` argument, narrowed to `f64` bits.
    fn long_double(&mut self) -> u64 {
        match self.va.as_deref_mut() {
            // SAFETY: the va_list contract is upheld by `Args::new`.
            Some(va) => unsafe { va_arg_long_double(va) },
            None => 0,
        }
    }
}

/// `vprintf(fmt, ap)` — `printf` with a `va_list`.
///
/// # Safety
/// `fmt` must be a valid NUL-terminated string and `ap` a valid `va_list`
/// with arguments matching the conversions in `fmt`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn vprintf(fmt: *const u8, ap: *mut VaList) -> i32 {
    if ap.is_null() {
        return -1;
    }
    // SAFETY: ap is non-null and the caller guarantees it is a valid va_list
    // matching `fmt`, which is exactly `Args::new`'s contract.
    let mut args = unsafe { Args::new(Some(&mut *ap)) };
    _printf_impl(fmt, &mut args)
}

/// `vfprintf(stream, fmt, ap)` — `fprintf` with a `va_list`.
///
/// # Safety
/// As [`vprintf`], plus `stream` must be a valid `FILE*`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn vfprintf(stream: *mut u8, fmt: *const u8, ap: *mut VaList) -> i32 {
    if ap.is_null() {
        return -1;
    }
    // SAFETY: as for `vprintf`.
    let mut args = unsafe { Args::new(Some(&mut *ap)) };
    _fprintf_impl(stream, fmt, &mut args)
}

/// `vdprintf(fd, fmt, ap)` — `dprintf` with a `va_list`.
///
/// # Safety
/// As [`vprintf`]; `fd` must be a valid file descriptor.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn vdprintf(fd: i32, fmt: *const u8, ap: *mut VaList) -> i32 {
    if ap.is_null() {
        return -1;
    }
    // SAFETY: as for `vprintf`.
    let mut args = unsafe { Args::new(Some(&mut *ap)) };
    _dprintf_impl(fd, fmt, &mut args)
}

/// `vsnprintf(buf, size, fmt, ap)` — `snprintf` with a `va_list`.
///
/// # Safety
/// As [`vprintf`]; `buf` must be writable for `size` bytes (or null with
/// `size` 0, in which case only the length is computed).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn vsnprintf(
    buf: *mut u8,
    size: usize,
    fmt: *const u8,
    ap: *mut VaList,
) -> i32 {
    if ap.is_null() {
        return -1;
    }
    // SAFETY: as for `vprintf`.
    let mut args = unsafe { Args::new(Some(&mut *ap)) };
    _snprintf_impl(buf, size, fmt, &mut args)
}

/// `vsprintf(buf, fmt, ap)` — `sprintf` with a `va_list`.
///
/// # Safety
/// As [`vprintf`]; `buf` must be large enough to hold the formatted output
/// plus a NUL terminator (this function performs no bounds checking).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn vsprintf(buf: *mut u8, fmt: *const u8, ap: *mut VaList) -> i32 {
    if ap.is_null() {
        return -1;
    }
    // SAFETY: as for `vprintf`.
    let mut args = unsafe { Args::new(Some(&mut *ap)) };
    _sprintf_impl(buf, fmt, &mut args)
}

/// `vasprintf(strp, fmt, ap)` — `asprintf` with a `va_list`.
///
/// # Safety
/// As [`vprintf`]; `strp` must be a valid `char**` to receive the malloc'd
/// result pointer.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn vasprintf(strp: *mut *mut u8, fmt: *const u8, ap: *mut VaList) -> i32 {
    if ap.is_null() {
        return -1;
    }
    // SAFETY: as for `vprintf`.  `asprintf` walks the format string twice, so
    // it takes a snapshot by value and replays it (C's `va_copy`).
    unsafe { _asprintf_impl(strp, fmt, Some(*ap)) }
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
    /// The `L` length modifier was present, so a floating conversion must
    /// fetch a 16-byte `long double` rather than a `double`.
    long_double: bool,
}

/// Parse flags, width, precision, and length modifier from a format string.
///
/// `fpos` points past the initial '%'.  On return, `fpos` points to
/// the conversion character (d, s, x, etc.).  Width/precision `*`
/// arguments are consumed from `args`.
fn parse_spec(fmt: *const u8, fpos: &mut usize, args: &mut Args) -> FormatSpec {
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
        let w = args.int() as i64;
        if w < 0 {
            flags.left_align = true;
            // Negate safely: wrapping_neg of i64::MIN is still negative,
            // so saturate at i64::MAX to avoid a huge spurious width.
            let pos = w.wrapping_neg();
            width = (if pos < 0 { i64::MAX } else { pos }) as usize;
        } else {
            width = w as usize;
        }
        *fpos = fpos.wrapping_add(1);
    } else {
        while unsafe { *fmt.add(*fpos) }.is_ascii_digit() {
            width = width
                .saturating_mul(10)
                .saturating_add((unsafe { *fmt.add(*fpos) }.wrapping_sub(b'0')) as usize);
            *fpos = fpos.wrapping_add(1);
        }
    }

    // Precision.
    let mut precision: Option<usize> = None;
    if unsafe { *fmt.add(*fpos) } == b'.' {
        *fpos = fpos.wrapping_add(1);
        if unsafe { *fmt.add(*fpos) } == b'*' {
            let p = args.int() as i32;
            if p >= 0 {
                precision = Some(p as usize);
            }
            *fpos = fpos.wrapping_add(1);
        } else {
            let mut p: usize = 0;
            while unsafe { *fmt.add(*fpos) }.is_ascii_digit() {
                p = p
                    .saturating_mul(10)
                    .saturating_add((unsafe { *fmt.add(*fpos) }.wrapping_sub(b'0')) as usize);
                *fpos = fpos.wrapping_add(1);
            }
            precision = Some(p);
        }
    }

    // Length modifier.  Only `L` matters: every integer type occupies one
    // 8-byte slot on LP64 however it is spelled, but `L` on a floating
    // conversion promotes the argument to a 16-byte, stack-passed
    // `long double`, so the fetch below has to know.
    // SAFETY: fmt is NUL-terminated and fpos is in range.
    let long_double = unsafe { skip_length_modifier(fmt, fpos) };

    FormatSpec {
        flags,
        width,
        precision,
        long_double,
    }
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
    args: &mut Args,
) -> usize {
    let ch = unsafe { *fmt.add(fpos) };
    let next = fpos.wrapping_add(1);

    match ch {
        b'%' => emit_byte(dst, b'%'),

        b'd' | b'i' => {
            let val = args.int() as i64;
            format_signed(dst, val, &spec.flags, spec.width, spec.precision);
        }

        b'u' => {
            let val = args.int();
            format_unsigned(dst, val, 10, false, &spec.flags, spec.width, spec.precision);
        }

        b'x' => {
            let val = args.int();
            format_unsigned(dst, val, 16, false, &spec.flags, spec.width, spec.precision);
        }

        b'X' => {
            let val = args.int();
            format_unsigned(dst, val, 16, true, &spec.flags, spec.width, spec.precision);
        }

        b'o' => {
            let val = args.int();
            format_unsigned(dst, val, 8, false, &spec.flags, spec.width, spec.precision);
        }

        b's' => {
            let ptr = args.int() as *const u8;
            format_string(dst, ptr, &spec.flags, spec.width, spec.precision);
        }

        b'c' => {
            let ch_val = args.int() as u8;
            if spec.width > 1 && !spec.flags.left_align {
                emit_padding(dst, b' ', spec.width.wrapping_sub(1));
            }
            emit_byte(dst, ch_val);
            if spec.width > 1 && spec.flags.left_align {
                emit_padding(dst, b' ', spec.width.wrapping_sub(1));
            }
        }

        b'p' => {
            let val = args.int();
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
            let ptr = args.int() as *mut i32;
            if !ptr.is_null() {
                // SAFETY: Caller guarantees ptr is valid.
                unsafe {
                    *ptr = dst.pos as i32;
                }
            }
        }

        // Floating-point specifiers.  `%Lf` fetches 16 bytes from the overflow
        // area (X87/X87UP is MEMORY-class), not a `double` from %xmm.
        b'f' | b'F' => {
            let bits = if spec.long_double { args.long_double() } else { args.double() };
            let val = f64::from_bits(bits);
            let prec = spec.precision.unwrap_or(6);
            format_float_fixed(dst, val, ch == b'F', &spec.flags, spec.width, prec);
        }

        b'e' | b'E' => {
            let bits = if spec.long_double { args.long_double() } else { args.double() };
            let val = f64::from_bits(bits);
            let prec = spec.precision.unwrap_or(6);
            format_float_sci(dst, val, ch == b'E', &spec.flags, spec.width, prec);
        }

        b'g' | b'G' => {
            let bits = if spec.long_double { args.long_double() } else { args.double() };
            let val = f64::from_bits(bits);
            let prec = if spec.precision == Some(0) {
                1
            } else {
                spec.precision.unwrap_or(6)
            };
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
fn format_core(out: *mut u8, out_size: usize, fmt: *const u8, args: &mut Args) -> i32 {
    if fmt.is_null() {
        return -1;
    }

    let mut dst = FmtOutput::new(out, out_size);
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
        let spec = parse_spec(fmt, &mut fpos, args);
        fpos = dispatch_spec(&mut dst, fmt, fpos, spec_start, &spec, args);
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

/// Emit a single byte to the output buffer.
fn emit_byte(dst: &mut FmtOutput, byte: u8) {
    if !dst.buf.is_null() && dst.pos < dst.size {
        // SAFETY: dst.pos < dst.size, so buf.add(dst.pos) is valid.
        unsafe {
            *dst.buf.add(dst.pos) = byte;
        }
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
    let abs_val = if negative {
        val.wrapping_neg() as u64
    } else {
        val as u64
    };

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

    // POSIX/C99: precision 0 with value 0 produces no digit output …
    // … UNLESS `%#o` which overrides this (see below).
    if precision == Some(0) && val == 0 {
        num_len = 0;
    }

    let mut digits = if let Some(p) = precision {
        if p > num_len { p } else { num_len }
    } else {
        num_len
    };

    // Alt form: %#o increases precision so the first output digit is '0'
    // (C99 §7.19.6.1 ¶6).  For val==0 && precision==0, a single '0'
    // must still appear.  This replaces the old prefix-based approach
    // which emitted a separate '0' and produced too many leading zeros
    // when precision already forced a leading zero.
    //
    // %#x/%#X uses a "0x"/"0X" prefix for nonzero values (different rule).
    let prefix_len: usize = if flags.alt_form {
        match base {
            16 if val != 0 => 2, // "0x" or "0X"
            8 => {
                // C99: "increase precision to force first digit to be zero."
                // "if the value and precision are both 0, a single 0 is printed."
                if val == 0 && precision == Some(0) {
                    // Override the "precision 0 + value 0 → no digits" rule.
                    num_len = 1;
                    digits = 1;
                    // The digit '0' is still at the end of num_buf from u64_to_base.
                } else if val != 0 && digits == num_len {
                    // digits == num_len means no precision-padding will add a
                    // leading zero, so the first output digit is the MSB of val
                    // (which is nonzero).  Increase by 1 to force a leading '0'.
                    // When val == 0, the first digit is already '0', so no
                    // extra leading zero is needed.
                    digits = digits.wrapping_add(1);
                }
                // Else: precision already >= num_len+1, so leading zeros exist;
                // or val == 0 and the digit is already '0'.
                0 // No separate prefix; the extra zero is part of `digits`.
            }
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

    // Prefix (only for %#x / %#X).
    if prefix_len == 2 {
        emit_byte(dst, b'0');
        emit_byte(dst, if upper { b'X' } else { b'x' });
    }

    // Right-justify zero padding.
    if !flags.left_align && width > total_len && pad_char == b'0' {
        emit_padding(dst, b'0', width.wrapping_sub(total_len));
    }

    // Precision / alt-form zero-padding.
    if digits > num_len {
        emit_padding(dst, b'0', digits.wrapping_sub(num_len));
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
            {
                *slot = b'0'.wrapping_add((val % 10) as u8);
            }
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
        {
            val = val.wrapping_div(base_u64);
        }
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
                && (b == b'e' || b == b'E')
            {
                break;
            }
            epos = epos.wrapping_add(1);
        }
        if epos < len {
            // Shift exponent part right by 1 to make room for '.'.
            let mut j = len;
            while j > epos {
                if let (Some(src), Some(dst_slot)) =
                    (buf.get(j.wrapping_sub(1)).copied(), buf.get_mut(j))
                {
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
        let sci_prec = if precision > 0 {
            precision.wrapping_sub(1)
        } else {
            0
        };
        len = fmt_scientific(abs_val, sci_prec, upper, &mut buf);
    } else {
        // Use fixed, with (precision - 1 - exp) digits after '.'.
        let fix_prec = if (p - 1 - exp) > 0 {
            (p - 1 - exp) as usize
        } else {
            0
        };
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
                    && b == b'.'
                {
                    found = true;
                    break;
                }
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
                    && (b == b'e' || b == b'E')
                {
                    insert_at = k;
                    break;
                }
                k = k.wrapping_add(1);
            }
            // Shift [insert_at..len] right by 1.
            let mut j = len;
            while j > insert_at {
                if let (Some(src), Some(dst_slot)) =
                    (buf.get(j.wrapping_sub(1)).copied(), buf.get_mut(j))
                {
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
    if let Some(s) = sign {
        emit_byte(dst, s);
    }
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

    let pad_char = if flags.zero_pad && !flags.left_align {
        b'0'
    } else {
        b' '
    };

    if !flags.left_align && width > total && pad_char == b' ' {
        emit_padding(dst, b' ', width.wrapping_sub(total));
    }
    if let Some(s) = sign {
        emit_byte(dst, s);
    }
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
        if let Some(slot) = buf.get_mut(pos) {
            *slot = b'0';
        }
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
        if let Some(slot) = buf.get_mut(pos) {
            *slot = b'.';
        }
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
                        if rp == 0 {
                            break;
                        }
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
                        if let (Some(src), Some(dst_slot)) =
                            (buf.get(j.wrapping_sub(1)).copied(), buf.get_mut(j))
                        {
                            *dst_slot = src;
                        }
                        j = j.wrapping_sub(1);
                    }
                    if let Some(slot) = buf.get_mut(0) {
                        *slot = b'1';
                    }
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
        if let Some(slot) = buf.get_mut(pos) {
            *slot = b'0';
        }
        pos = pos.wrapping_add(1);
        if precision > 0 {
            if let Some(slot) = buf.get_mut(pos) {
                *slot = b'.';
            }
            pos = pos.wrapping_add(1);
            let mut p = precision;
            while p > 0 {
                if let Some(slot) = buf.get_mut(pos) {
                    *slot = b'0';
                }
                pos = pos.wrapping_add(1);
                p = p.wrapping_sub(1);
            }
        }
        if let Some(slot) = buf.get_mut(pos) {
            *slot = if upper { b'E' } else { b'e' };
        }
        pos = pos.wrapping_add(1);
        if let Some(slot) = buf.get_mut(pos) {
            *slot = b'+';
        }
        pos = pos.wrapping_add(1);
        if let Some(slot) = buf.get_mut(pos) {
            *slot = b'0';
        }
        pos = pos.wrapping_add(1);
        if let Some(slot) = buf.get_mut(pos) {
            *slot = b'0';
        }
        pos = pos.wrapping_add(1);
        return pos;
    }

    // Find decimal exponent: floor(log10(|val|)).
    // Previous code used ilogb (binary exponent) which is incorrect —
    // e.g. ilogb(9.5)=3 but the decimal exponent is 0.
    let mut exp: i32 = crate::math::floor(crate::math::log10(val)) as i32;
    let mut mantissa = val / crate::math::pow(10.0, f64::from(exp));
    // Normalize: 1 <= mantissa < 10 (handle floating-point imprecision).
    while mantissa >= 10.0 {
        mantissa /= 10.0;
        exp += 1;
    }
    while mantissa < 1.0 && mantissa > 0.0 {
        mantissa *= 10.0;
        exp -= 1;
    }

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
    if let Some(slot) = buf.get_mut(pos) {
        *slot = if upper { b'E' } else { b'e' };
    }
    pos = pos.wrapping_add(1);
    if let Some(slot) = buf.get_mut(pos) {
        *slot = if exp < 0 { b'-' } else { b'+' };
    }
    pos = pos.wrapping_add(1);

    let abs_exp = if exp < 0 { (-exp) as u32 } else { exp as u32 };
    // At least 2 digits for exponent.
    if abs_exp < 10 {
        if let Some(slot) = buf.get_mut(pos) {
            *slot = b'0';
        }
        pos = pos.wrapping_add(1);
        if let Some(slot) = buf.get_mut(pos) {
            *slot = b'0'.wrapping_add(abs_exp as u8);
        }
        pos = pos.wrapping_add(1);
    } else if abs_exp < 100 {
        if let Some(slot) = buf.get_mut(pos) {
            *slot = b'0'.wrapping_add((abs_exp / 10) as u8);
        }
        pos = pos.wrapping_add(1);
        if let Some(slot) = buf.get_mut(pos) {
            *slot = b'0'.wrapping_add((abs_exp % 10) as u8);
        }
        pos = pos.wrapping_add(1);
    } else {
        // 3-digit exponent (values > 1e99).
        if let Some(slot) = buf.get_mut(pos) {
            *slot = b'0'.wrapping_add((abs_exp / 100) as u8);
        }
        pos = pos.wrapping_add(1);
        if let Some(slot) = buf.get_mut(pos) {
            *slot = b'0'.wrapping_add(((abs_exp / 10) % 10) as u8);
        }
        pos = pos.wrapping_add(1);
        if let Some(slot) = buf.get_mut(pos) {
            *slot = b'0'.wrapping_add((abs_exp % 10) as u8);
        }
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
            && (b == b'e' || b == b'E')
        {
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
            && b == b'.'
        {
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
            && b != b'0'
        {
            break;
        }
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
                && let Some(dst_slot) = buf.get_mut(dst_idx)
            {
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
    // Integer arguments are given as a `&[u64]` slice (string pointers cast
    // to `u64`) and float arguments as a separate `&[u64]` of
    // `f64::to_bits()`.  [`with_args`] lays those out into a synthetic System
    // V register save area and hands the engine a real `va_list` over it —
    // the same shape a compiler-generated `va_start` produces — so the tests
    // exercise the production argument path rather than a test-only one.
    // -----------------------------------------------------------------------

    /// Storage backing a synthetic `va_list`: the 176-byte register save area
    /// (six 8-byte GP slots, then eight 16-byte XMM slots) plus a generous
    /// overflow area for arguments past the register file.
    struct ArgScratch {
        reg: [u8; 176],
        overflow: [u8; 512],
    }

    /// Build a synthetic `va_list` over `ints`/`floats` and run `f` on it.
    ///
    /// The first six integers land in the GP save slots and the first eight
    /// floats in the XMM save slots; anything beyond that goes to the
    /// overflow area, integers first.  That ordering is only faithful when at
    /// most one of the two classes overflows — with both overflowing, the
    /// real ABI interleaves them by document order, which this two-slice
    /// interface cannot express.  The assertion below pins that limit rather
    /// than letting such a test silently read the wrong slots.
    fn with_args<R>(ints: &[u64], floats: &[u64], f: impl FnOnce(&mut Args) -> R) -> R {
        assert!(
            ints.len() <= 6 || floats.len() <= 8,
            "with_args cannot lay out overflowing integers and floats together; \
             build the va_list explicitly for this case"
        );
        let mut scratch = ArgScratch {
            reg: [0u8; 176],
            overflow: [0u8; 512],
        };

        for (i, &v) in ints.iter().take(6).enumerate() {
            let off = i * 8;
            scratch.reg[off..off + 8].copy_from_slice(&v.to_le_bytes());
        }
        for (i, &v) in floats.iter().take(8).enumerate() {
            let off = 48 + i * 16;
            scratch.reg[off..off + 8].copy_from_slice(&v.to_le_bytes());
        }

        let mut spill = 0usize;
        for &v in ints.iter().skip(6).chain(floats.iter().skip(8)) {
            assert!(spill + 8 <= scratch.overflow.len(), "overflow area too small");
            scratch.overflow[spill..spill + 8].copy_from_slice(&v.to_le_bytes());
            spill += 8;
        }

        let mut va = VaList {
            gp_offset: 0,
            fp_offset: 48,
            overflow_arg_area: scratch.overflow.as_mut_ptr(),
            reg_save_area: scratch.reg.as_mut_ptr(),
        };
        // SAFETY: `va` describes the scratch buffers above, which outlive the
        // call and are laid out exactly as the ABI specifies.
        let mut args = unsafe { Args::new(Some(&mut va)) };
        f(&mut args)
    }

    /// Format via `_snprintf_impl` and return the output as a `String`.
    fn snprintf_str(fmt: &[u8], args: &[u64], fargs: &[u64]) -> (String, i32) {
        let mut buf = [0u8; 512];
        let n = with_args(args, fargs, |a| {
            _snprintf_impl(buf.as_mut_ptr(), buf.len(), fmt.as_ptr(), a)
        });
        let len = if n >= 0 && (n as usize) < buf.len() {
            n as usize
        } else {
            buf.len().wrapping_sub(1)
        };
        let s = core::str::from_utf8(&buf[..len])
            .unwrap_or("<invalid utf8>")
            .to_string();
        (s, n)
    }

    /// Format via `format_core` and return the output as a `String`.
    #[allow(dead_code)]
    fn fmt_str(fmt: &[u8], args: &[u64], fargs: &[u64]) -> (String, i32) {
        let mut buf = [0u8; 512];
        let n = with_args(args, fargs, |a| {
            format_core(buf.as_mut_ptr(), buf.len(), fmt.as_ptr(), a)
        });
        let len = if n >= 0 && (n as usize) < buf.len() {
            n as usize
        } else if n >= 0 {
            buf.len()
        } else {
            0
        };
        let s = core::str::from_utf8(&buf[..len])
            .unwrap_or("<invalid utf8>")
            .to_string();
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

    #[test]
    fn fmt_flag_hash_o_zero() {
        // %#o with value 0: output "0" (no prefix needed, first digit is already 0).
        let (s, _) = snprintf_str(b"%#o\0", &[0], &[]);
        assert_eq!(s, "0");
    }

    #[test]
    fn fmt_flag_hash_o_prec0_val0() {
        // C99 §7.19.6.1: "if the value and precision are both 0, a single 0 is printed"
        let (s, _) = snprintf_str(b"%#.0o\0", &[0], &[]);
        assert_eq!(s, "0");
    }

    #[test]
    fn fmt_flag_hash_o_prec_already_has_leading_zero() {
        // %#.5o with val=7: precision forces "00007" which already starts
        // with '0', so # should not add another.
        let (s, _) = snprintf_str(b"%#.5o\0", &[7], &[]);
        assert_eq!(s, "00007");
    }

    #[test]
    fn fmt_flag_hash_o_simple() {
        // %#o with val=7: octal "7" → needs leading zero → "07"
        let (s, _) = snprintf_str(b"%#o\0", &[7], &[]);
        assert_eq!(s, "07");
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
        let n = with_args(&[], &[], |a| {
            _snprintf_impl(buf.as_mut_ptr(), buf.len(), b"hello world\0".as_ptr(), a)
        });
        // n should be 11 (total chars that would be written).
        assert_eq!(n, 11);
        // buf should be "hello w\0" (7 chars + NUL).
        assert_eq!(&buf, b"hello w\0");
    }

    #[test]
    fn snprintf_exact_fit() {
        let mut buf = [0xFFu8; 6];
        let n = with_args(&[], &[], |a| {
            _snprintf_impl(buf.as_mut_ptr(), buf.len(), b"hello\0".as_ptr(), a)
        });
        assert_eq!(n, 5);
        assert_eq!(&buf, b"hello\0");
    }

    #[test]
    fn snprintf_size_one() {
        let mut buf = [0xFFu8; 1];
        let n = with_args(&[], &[], |a| {
            _snprintf_impl(buf.as_mut_ptr(), buf.len(), b"hello\0".as_ptr(), a)
        });
        assert_eq!(n, 5);
        // Only the NUL terminator should be written.
        assert_eq!(buf[0], 0);
    }

    #[test]
    fn snprintf_null_buf_returns_count() {
        let n = with_args(&[42u64], &[], |a| {
            _snprintf_impl(core::ptr::null_mut(), 0, b"hello %d\0".as_ptr(), a)
        });
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
        let n = with_args(&[], &[], |a| {
            format_core(core::ptr::null_mut(), 0, core::ptr::null(), a)
        });
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

    // -----------------------------------------------------------------------
    // 21. %g general floating point
    // -----------------------------------------------------------------------

    #[test]
    fn fmt_g_basic() {
        // %g with default precision 6: 3.14 → "3.14" (trailing zeros removed)
        let val = 3.14f64;
        let (s, _) = snprintf_str(b"%g\0", &[], &[val.to_bits()]);
        assert_eq!(s, "3.14", "got: {s}");
    }

    #[test]
    fn fmt_g_zero() {
        let val = 0.0f64;
        let (s, _) = snprintf_str(b"%g\0", &[], &[val.to_bits()]);
        assert_eq!(s, "0", "got: {s}");
    }

    #[test]
    fn fmt_g_integer_value() {
        // 100.0 → "100" (no fractional part needed)
        let val = 100.0f64;
        let (s, _) = snprintf_str(b"%g\0", &[], &[val.to_bits()]);
        assert_eq!(s, "100", "got: {s}");
    }

    #[test]
    fn fmt_g_small_value_switches_to_sci() {
        // exponent < -4 → uses scientific notation
        let val = 0.000012345f64;
        let (s, _) = snprintf_str(b"%g\0", &[], &[val.to_bits()]);
        assert!(
            s.contains('e'),
            "%g should use scientific for small values: {s}"
        );
    }

    #[test]
    fn fmt_g_large_value_switches_to_sci() {
        // exponent >= precision → uses scientific notation
        let val = 1234567.0f64;
        let (s, _) = snprintf_str(b"%g\0", &[], &[val.to_bits()]);
        assert!(
            s.contains('e'),
            "%g should use scientific for large values: {s}"
        );
    }

    #[test]
    fn fmt_g_negative() {
        let val = -2.5f64;
        let (s, _) = snprintf_str(b"%g\0", &[], &[val.to_bits()]);
        assert_eq!(s, "-2.5", "got: {s}");
    }

    #[test]
    fn fmt_g_uppercase() {
        // %G should use uppercase E in scientific notation
        let val = 0.000012345f64;
        let (s, _) = snprintf_str(b"%G\0", &[], &[val.to_bits()]);
        assert!(s.contains('E'), "%G should use uppercase E: {s}");
    }

    #[test]
    fn fmt_g_nan() {
        let val = f64::NAN;
        let (s, _) = snprintf_str(b"%g\0", &[], &[val.to_bits()]);
        assert_eq!(s, "nan");
    }

    #[test]
    fn fmt_g_inf() {
        let val = f64::INFINITY;
        let (s, _) = snprintf_str(b"%g\0", &[], &[val.to_bits()]);
        assert_eq!(s, "inf");
    }

    #[test]
    fn fmt_g_neg_inf() {
        let val = f64::NEG_INFINITY;
        let (s, _) = snprintf_str(b"%g\0", &[], &[val.to_bits()]);
        assert_eq!(s, "-inf");
    }

    #[test]
    fn fmt_g_precision() {
        // %.2g with 3.14159 → uses 2 significant digits
        let val = 3.14159f64;
        let (s, _) = snprintf_str(b"%.2g\0", &[], &[val.to_bits()]);
        assert_eq!(s, "3.1", "got: {s}");
    }

    #[test]
    fn fmt_g_precision_zero() {
        // C99: %g with precision 0 is treated as precision 1
        let val = 3.14f64;
        let (s, _) = snprintf_str(b"%.0g\0", &[], &[val.to_bits()]);
        assert_eq!(s, "3", "got: {s}");
    }

    // -----------------------------------------------------------------------
    // 22. %n format — write count of characters written so far
    // -----------------------------------------------------------------------

    #[test]
    fn fmt_n_basic() {
        let mut count: i32 = -1;
        let count_ptr = &raw mut count as u64;
        let (s, _) = snprintf_str(b"hello%n world\0", &[count_ptr], &[]);
        assert_eq!(s, "hello world");
        assert_eq!(count, 5, "%%n should record 5 chars written before it");
    }

    // -----------------------------------------------------------------------
    // 23. Star width and precision
    // -----------------------------------------------------------------------

    #[test]
    fn fmt_star_width() {
        // %*d with width=8, value=42
        let (s, _) = snprintf_str(b"%*d\0", &[8, 42], &[]);
        assert_eq!(s, "      42");
    }

    #[test]
    fn fmt_star_width_negative() {
        // Negative star-width → left-align
        let neg8 = (-8i64) as u64;
        let (s, _) = snprintf_str(b"%*d\0", &[neg8, 42], &[]);
        assert_eq!(s, "42      ");
    }

    #[test]
    fn fmt_star_precision() {
        // %.*d with precision=5, value=42
        let (s, _) = snprintf_str(b"%.*d\0", &[5, 42], &[]);
        assert_eq!(s, "00042");
    }

    #[test]
    fn fmt_star_precision_negative() {
        // Negative star-precision is ignored (treated as no precision)
        let neg = (-1i64) as u64;
        let (s, _) = snprintf_str(b"%.*d\0", &[neg, 42], &[]);
        assert_eq!(s, "42");
    }

    // -----------------------------------------------------------------------
    // 24. Unknown and edge-case specifiers
    // -----------------------------------------------------------------------

    #[test]
    fn fmt_unknown_specifier() {
        // Unknown specifier should be emitted raw
        let (s, _) = snprintf_str(b"%q\0", &[42], &[]);
        // Should emit "%q" raw
        assert!(
            s.contains("%q"),
            "unknown specifier should be emitted raw: {s}"
        );
    }

    #[test]
    fn fmt_trailing_percent() {
        // "%" at end of string (premature end)
        let (s, _) = snprintf_str(b"test%\0", &[], &[]);
        assert_eq!(s, "test");
    }

    // -----------------------------------------------------------------------
    // 25. %u edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn fmt_u_max() {
        let val = u32::MAX as u64;
        let (s, _) = snprintf_str(b"%u\0", &[val], &[]);
        assert_eq!(s, "4294967295");
    }

    #[test]
    fn fmt_u_zero() {
        let (s, _) = snprintf_str(b"%u\0", &[0], &[]);
        assert_eq!(s, "0");
    }

    // -----------------------------------------------------------------------
    // 26. %i is alias for %d
    // -----------------------------------------------------------------------

    #[test]
    fn fmt_i_basic() {
        let val = (-42i64) as u64;
        let (s, _) = snprintf_str(b"%i\0", &[val], &[]);
        assert_eq!(s, "-42");
    }

    // -----------------------------------------------------------------------
    // 27. %s precision truncation
    // -----------------------------------------------------------------------

    #[test]
    fn fmt_s_precision_truncation() {
        let text = b"hello world\0";
        let (s, _) = snprintf_str(b"%.5s\0", &[text.as_ptr() as u64], &[]);
        assert_eq!(s, "hello");
    }

    #[test]
    fn fmt_s_precision_longer_than_string() {
        let text = b"hi\0";
        let (s, _) = snprintf_str(b"%.10s\0", &[text.as_ptr() as u64], &[]);
        assert_eq!(s, "hi");
    }

    // -----------------------------------------------------------------------
    // 28. %x/#o precision edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn fmt_x_precision_pads() {
        let (s, _) = snprintf_str(b"%.8x\0", &[0xABCu64], &[]);
        assert_eq!(s, "00000abc");
    }

    #[test]
    fn fmt_o_precision_pads() {
        let (s, _) = snprintf_str(b"%.6o\0", &[8u64], &[]);
        assert_eq!(s, "000010");
    }

    // -----------------------------------------------------------------------
    // 29. %d with space flag
    // -----------------------------------------------------------------------

    #[test]
    fn fmt_space_flag_positive() {
        let (s, _) = snprintf_str(b"% d\0", &[42], &[]);
        assert_eq!(s, " 42");
    }

    #[test]
    fn fmt_space_flag_negative() {
        let val = (-42i64) as u64;
        let (s, _) = snprintf_str(b"% d\0", &[val], &[]);
        assert_eq!(s, "-42");
    }

    // -----------------------------------------------------------------------
    // 30. Multiple format specs in one string
    // -----------------------------------------------------------------------

    #[test]
    fn fmt_three_ints() {
        let (s, _) = snprintf_str(b"%d %d %d\0", &[1, 2, 3], &[]);
        assert_eq!(s, "1 2 3");
    }

    #[test]
    fn fmt_int_and_hex() {
        let (s, _) = snprintf_str(b"%d 0x%x\0", &[255, 255], &[]);
        assert_eq!(s, "255 0xff");
    }

    // -----------------------------------------------------------------------
    // 31. %f edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn fmt_f_large_integer() {
        let val = 99999.0f64;
        let (s, _) = snprintf_str(b"%.0f\0", &[], &[val.to_bits()]);
        assert_eq!(s, "99999");
    }

    #[test]
    fn fmt_f_half_rounds() {
        // %.0f with 3.7 should round to 4
        let val = 3.7f64;
        let (s, _) = snprintf_str(b"%.0f\0", &[], &[val.to_bits()]);
        assert_eq!(s, "4");
    }

    // -----------------------------------------------------------------------
    // 32. %e edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn fmt_e_negative() {
        let val = -1.5f64;
        let (s, _) = snprintf_str(b"%e\0", &[], &[val.to_bits()]);
        assert!(s.starts_with("-1.5"), "got: {s}");
    }

    #[test]
    fn fmt_e_uppercase() {
        let val = 1.0f64;
        let (s, _) = snprintf_str(b"%E\0", &[], &[val.to_bits()]);
        assert!(s.contains('E'), "%E should use uppercase E: {s}");
    }

    #[test]
    fn fmt_e_precision() {
        let val = 3.14159f64;
        let (s, _) = snprintf_str(b"%.2e\0", &[], &[val.to_bits()]);
        assert_eq!(s, "3.14e+00", "got: {s}");
    }

    // -----------------------------------------------------------------------
    // 33. %c edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn fmt_c_null_char() {
        // %c with NUL byte — should write a NUL into the output
        let (_, n) = snprintf_str(b"a%cb\0", &[0], &[]);
        // Total length should be 3 (a + NUL + b)
        assert_eq!(n, 3);
    }

    #[test]
    fn fmt_c_left_aligned() {
        let (s, _) = snprintf_str(b"%-5c\0", &[b'X' as u64], &[]);
        assert_eq!(s, "X    ");
    }

    // -----------------------------------------------------------------------
    // 34. %p edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn fmt_p_width() {
        let (s, _) = snprintf_str(b"%20p\0", &[0xFF], &[]);
        assert_eq!(s.len(), 20, "should be padded to width 20: '{s}'");
        assert!(s.contains("0xff"), "should contain 0xff: '{s}'");
    }

    // -------------------------------------------------------------------
    // 35. Stress tests — multiple specifiers in one format string
    // -------------------------------------------------------------------

    #[test]
    fn stress_multiple_specifiers() {
        let text = b"world\0";
        let (s, _) = snprintf_str(b"hello %s %d\0", &[text.as_ptr() as u64, 42], &[]);
        assert_eq!(s, "hello world 42");
    }

    #[test]
    fn stress_mixed_int_and_float() {
        let f = 3.14f64;
        let (s, _) = snprintf_str(b"%d and %.2f\0", &[99], &[f.to_bits()]);
        assert_eq!(s, "99 and 3.14");
    }

    #[test]
    fn stress_percent_literal() {
        let (s, _) = snprintf_str(b"100%%\0", &[], &[]);
        assert_eq!(s, "100%");
    }

    #[test]
    fn stress_width_and_precision_combined() {
        let (s, _) = snprintf_str(b"%10.3d\0", &[42], &[]);
        // Width 10, precision 3 for integer: "       042"
        assert_eq!(s.len(), 10);
        assert!(s.ends_with("042"), "got: '{s}'");
    }

    #[test]
    fn stress_left_align_with_width() {
        let (s, _) = snprintf_str(b"%-10d|\0", &[42], &[]);
        assert_eq!(s, "42        |");
    }

    #[test]
    fn stress_zero_padded() {
        let (s, _) = snprintf_str(b"%05d\0", &[42], &[]);
        assert_eq!(s, "00042");
    }

    #[test]
    fn stress_plus_sign() {
        let (s, _) = snprintf_str(b"%+d\0", &[42], &[]);
        assert_eq!(s, "+42");
    }

    #[test]
    fn stress_space_sign() {
        let (s, _) = snprintf_str(b"% d\0", &[42], &[]);
        assert_eq!(s, " 42");
    }

    #[test]
    fn stress_hex_alternate() {
        let (s, _) = snprintf_str(b"%#x\0", &[255], &[]);
        assert_eq!(s, "0xff");
    }

    #[test]
    fn stress_hex_uppercase_alternate() {
        let (s, _) = snprintf_str(b"%#X\0", &[255], &[]);
        assert_eq!(s, "0XFF");
    }

    #[test]
    fn stress_octal_alternate() {
        let (s, _) = snprintf_str(b"%#o\0", &[8], &[]);
        assert_eq!(s, "010");
    }

    #[test]
    fn stress_string_precision_truncate() {
        let text = b"hello world\0";
        let (s, _) = snprintf_str(b"%.5s\0", &[text.as_ptr() as u64], &[]);
        assert_eq!(s, "hello");
    }

    #[test]
    fn stress_string_width_right_align() {
        let text = b"hi\0";
        let (s, _) = snprintf_str(b"%10s\0", &[text.as_ptr() as u64], &[]);
        assert_eq!(s.len(), 10);
        assert!(s.ends_with("hi"));
    }

    #[test]
    fn stress_string_width_left_align() {
        let text = b"hi\0";
        let (s, _) = snprintf_str(b"%-10s\0", &[text.as_ptr() as u64], &[]);
        assert_eq!(s, "hi        ");
    }

    #[test]
    fn stress_float_large_value() {
        let f = 1234567.89f64;
        let (s, _) = snprintf_str(b"%.2f\0", &[], &[f.to_bits()]);
        assert_eq!(s, "1234567.89");
    }

    #[test]
    fn stress_float_zero_precision() {
        let f = 3.7f64;
        let (s, _) = snprintf_str(b"%.0f\0", &[], &[f.to_bits()]);
        assert_eq!(s, "4"); // rounds up
    }

    #[test]
    fn stress_float_negative_zero_precision() {
        let f = -3.7f64;
        let (s, _) = snprintf_str(b"%.0f\0", &[], &[f.to_bits()]);
        assert_eq!(s, "-4");
    }

    #[test]
    fn stress_unsigned_max() {
        let val = u32::MAX as u64;
        let (s, _) = snprintf_str(b"%u\0", &[val], &[]);
        assert_eq!(s, "4294967295");
    }

    #[test]
    fn stress_hex_zero() {
        let (s, _) = snprintf_str(b"%x\0", &[0], &[]);
        assert_eq!(s, "0");
    }

    #[test]
    fn stress_snprintf_truncation() {
        // snprintf with small buffer should truncate but return full length.
        let mut buf = [0u8; 6]; // room for 5 chars + NUL
        let n = with_args(&[], &[], |a| {
            _snprintf_impl(buf.as_mut_ptr(), buf.len(), b"hello world\0".as_ptr(), a)
        });
        // Should return 11 (length of "hello world") even though buffer is only 6.
        assert_eq!(n, 11);
        // Buffer should contain "hello\0".
        assert_eq!(&buf[..5], b"hello");
        assert_eq!(buf[5], 0);
    }

    #[test]
    fn stress_g_format_removes_trailing_zeros() {
        let f = 1.5f64;
        let (s, _) = snprintf_str(b"%g\0", &[], &[f.to_bits()]);
        assert_eq!(s, "1.5");
    }

    #[test]
    fn stress_g_format_integer_like() {
        let f = 100.0f64;
        let (s, _) = snprintf_str(b"%g\0", &[], &[f.to_bits()]);
        assert_eq!(s, "100");
    }

    #[test]
    fn stress_e_format_small_number() {
        let f = 0.001f64;
        let (s, _) = snprintf_str(b"%.2e\0", &[], &[f.to_bits()]);
        assert_eq!(s, "1.00e-03");
    }

    // -----------------------------------------------------------------------
    // v* printf family (va_list extraction)
    //
    // These build a synthetic SysV `va_list` by hand — a 176-byte register
    // save area plus a stack overflow area — so they exercise the `va_arg_*`
    // readers without relying on the host C `va_start` (whose ABI differs on
    // Windows hosts).  `gp_offset`/`fp_offset` start at the values `va_start`
    // would leave for a function whose only fixed arg is `fmt` (1 GP register
    // consumed → 8) ... but since the readers simply consume from the given
    // offsets, we use 0/48 here and place the args at the start of each
    // register bank for clarity.
    // -----------------------------------------------------------------------

    /// Build a register save area populated with up to 6 integer args (GP
    /// slots) and up to 8 float args (XMM slots), plus an overflow area for
    /// integer args 6+.  Returns the assembled buffers and a `VaList`.
    fn run_vsnprintf(fmt: &[u8], ints: &[u64], floats: &[f64]) -> (String, i32) {
        // Register save area: 6 GP regs (0..48), 8 XMM regs (48..176).
        let mut reg = [0u8; 176];
        // Overflow area for integer args beyond the 6 GP registers.
        let mut overflow = [0u8; 128];

        let mut overflow_pos = 0usize;
        for (i, &v) in ints.iter().enumerate() {
            if i < 6 {
                let off = i * 8;
                reg[off..off + 8].copy_from_slice(&v.to_le_bytes());
            } else {
                overflow[overflow_pos..overflow_pos + 8].copy_from_slice(&v.to_le_bytes());
                overflow_pos += 8;
            }
        }
        for (i, &v) in floats.iter().enumerate().take(8) {
            let off = 48 + i * 16;
            reg[off..off + 8].copy_from_slice(&v.to_bits().to_le_bytes());
        }

        let mut va = VaList {
            gp_offset: 0,
            fp_offset: 48,
            overflow_arg_area: overflow.as_mut_ptr(),
            reg_save_area: reg.as_mut_ptr(),
        };

        let mut buf = [0u8; 512];
        // SAFETY: va points at the buffers above, which outlive the call and
        // hold enough args for `fmt`.
        let n = unsafe { vsnprintf(buf.as_mut_ptr(), buf.len(), fmt.as_ptr(), &mut va) };
        let len = if n >= 0 && (n as usize) < buf.len() {
            n as usize
        } else {
            buf.len().wrapping_sub(1)
        };
        let s = core::str::from_utf8(&buf[..len])
            .unwrap_or("<invalid utf8>")
            .to_string();
        (s, n)
    }

    #[test]
    fn vsnprintf_basic_ints() {
        let (s, n) = run_vsnprintf(b"%d %d %d\0", &[1, 2, 3], &[]);
        assert_eq!(s, "1 2 3");
        assert_eq!(n, 5);
    }

    #[test]
    fn vsnprintf_string_arg() {
        let msg = b"hello\0";
        let (s, _) = run_vsnprintf(b"[%s]\0", &[msg.as_ptr() as u64], &[]);
        assert_eq!(s, "[hello]");
    }

    #[test]
    fn vsnprintf_float_arg() {
        let (s, _) = run_vsnprintf(b"%.2f\0", &[], &[3.14159]);
        assert_eq!(s, "3.14");
    }

    #[test]
    fn vsnprintf_mixed_int_float() {
        // Document order: int, float, int — pulled from separate banks but
        // interleaved correctly, because each conversion takes its argument
        // from the bank its own class dictates.
        let (s, _) = run_vsnprintf(b"%d=%.1f (%d)\0", &[7, 9], &[2.5]);
        assert_eq!(s, "7=2.5 (9)");
    }

    #[test]
    fn vsnprintf_star_width_precision() {
        // %*d → width arg (8) then value (42); %.*f → precision (3) then value.
        let (s, _) = run_vsnprintf(b"%*d|%.*f\0", &[8, 42, 3], &[1.5]);
        assert_eq!(s, "      42|1.500");
    }

    #[test]
    fn vsnprintf_overflow_area() {
        // 8 integer args: 0..5 come from GP registers, 6 and 7 from the
        // stack overflow area.  Verifies va_arg_int spills correctly.
        let (s, _) = run_vsnprintf(
            b"%d %d %d %d %d %d %d %d\0",
            &[10, 20, 30, 40, 50, 60, 70, 80],
            &[],
        );
        assert_eq!(s, "10 20 30 40 50 60 70 80");
    }

    #[test]
    fn vsnprintf_percent_literal_no_arg() {
        // %% must not consume an argument; the following %d uses the first arg.
        let (s, _) = run_vsnprintf(b"100%% of %d\0", &[5], &[]);
        assert_eq!(s, "100% of 5");
    }

    #[test]
    fn vsnprintf_length_modifiers_ignored() {
        // l/ll/z modifiers consume no extra args on LP64.
        let (s, _) = run_vsnprintf(b"%ld %llu %zu\0", &[1, 2, 3], &[]);
        assert_eq!(s, "1 2 3");
    }

    #[test]
    fn vsnprintf_null_va_returns_error() {
        let mut buf = [0u8; 16];
        // SAFETY: passing a null va_list must be rejected, not dereferenced.
        let n = unsafe {
            vsnprintf(
                buf.as_mut_ptr(),
                buf.len(),
                b"%d\0".as_ptr(),
                core::ptr::null_mut(),
            )
        };
        assert_eq!(n, -1);
    }

    #[test]
    fn vsprintf_writes_and_terminates() {
        let mut reg = [0u8; 176];
        reg[0..8].copy_from_slice(&123u64.to_le_bytes());
        let mut overflow = [0u8; 16];
        let mut va = VaList {
            gp_offset: 0,
            fp_offset: 48,
            overflow_arg_area: overflow.as_mut_ptr(),
            reg_save_area: reg.as_mut_ptr(),
        };
        let mut buf = [0u8; 32];
        // SAFETY: va and buf are valid; buf is large enough.
        let n = unsafe { vsprintf(buf.as_mut_ptr(), b"n=%d\0".as_ptr(), &mut va) };
        assert_eq!(n, 5);
        assert_eq!(&buf[..5], b"n=123");
        assert_eq!(buf[5], 0); // NUL terminator
    }

    // -----------------------------------------------------------------------
    // `long double` (%L) — stack-passed, 16-byte-aligned arguments.
    // -----------------------------------------------------------------------

    /// An overflow area with the ABI's 16-byte alignment, so that a
    /// `long double` slot placed at an aligned *offset* is also at an aligned
    /// *address* — which is what `va_arg_long_double` actually rounds up to.
    #[repr(C, align(16))]
    struct OverflowArea([u8; 256]);

    /// One argument in the overflow (stack) area.
    enum Stacked {
        /// A single 8-byte slot: integer, pointer, or promoted `double`.
        Word(u64),
        /// A `long double`: 16 bytes, 16-byte aligned.
        LongDouble(f64),
    }

    /// Like [`run_vsnprintf`], but lets the caller lay out the overflow area
    /// explicitly so mixed 8- and 16-byte stack slots can be exercised.
    fn run_vsnprintf_stack(fmt: &[u8], ints: &[u64], floats: &[f64], stack: &[Stacked]) -> String {
        let mut reg = [0u8; 176];
        let mut overflow = OverflowArea([0u8; 256]);

        for (i, &v) in ints.iter().enumerate().take(6) {
            let off = i * 8;
            reg[off..off + 8].copy_from_slice(&v.to_le_bytes());
        }
        for (i, &v) in floats.iter().enumerate().take(8) {
            let off = 48 + i * 16;
            reg[off..off + 8].copy_from_slice(&v.to_bits().to_le_bytes());
        }

        let mut pos = 0usize;
        for item in stack {
            match *item {
                Stacked::Word(v) => {
                    overflow.0[pos..pos + 8].copy_from_slice(&v.to_le_bytes());
                    pos += 8;
                }
                Stacked::LongDouble(v) => {
                    pos = (pos + 15) & !15;
                    let ld = crate::x87::from_f64(v);
                    overflow.0[pos..pos + 8].copy_from_slice(&ld.significand.to_le_bytes());
                    overflow.0[pos + 8..pos + 10].copy_from_slice(&ld.sign_exp.to_le_bytes());
                    pos += 16;
                }
            }
        }

        let mut va = VaList {
            gp_offset: 0,
            fp_offset: 48,
            overflow_arg_area: overflow.0.as_mut_ptr(),
            reg_save_area: reg.as_mut_ptr(),
        };

        let mut buf = [0u8; 512];
        // SAFETY: va points at the buffers above, which outlive the call and
        // hold enough args for `fmt`.
        let n = unsafe { vsnprintf(buf.as_mut_ptr(), buf.len(), fmt.as_ptr(), &mut va) };
        let len = if n >= 0 && (n as usize) < buf.len() {
            n as usize
        } else {
            buf.len().wrapping_sub(1)
        };
        core::str::from_utf8(&buf[..len])
            .unwrap_or("<invalid utf8>")
            .to_string()
    }

    #[test]
    fn vsnprintf_long_double_basic() {
        // `%Lf` must consume a 16-byte x87 slot from the overflow area, not
        // fall through with `L` treated as the conversion character.
        let s = run_vsnprintf_stack(b"%.2Lf\0", &[], &[], &[Stacked::LongDouble(3.14159)]);
        assert_eq!(s, "3.14");
    }

    #[test]
    fn vsnprintf_long_double_all_float_conversions() {
        let s = run_vsnprintf_stack(
            b"%.3Le %.3Lg %.1LF\0",
            &[],
            &[],
            // All three are exactly representable and none lands on a rounding
            // tie, so the expectations test the `L` plumbing rather than this
            // engine's tie-breaking rule (see TD-POSIX-PRINTF-TIE-ROUNDING).
            &[
                Stacked::LongDouble(1234.0),
                Stacked::LongDouble(0.25),
                Stacked::LongDouble(-2.5),
            ],
        );
        assert_eq!(s, "1.234e+03 0.25 -2.5");
    }

    #[test]
    fn vsnprintf_long_double_pair_advances_sixteen_bytes() {
        // If the cursor advanced by 8 instead of 16, the second value would be
        // read from the middle of the first slot and come out as garbage.
        let s = run_vsnprintf_stack(
            b"%.1Lf %.1Lf\0",
            &[],
            &[],
            &[Stacked::LongDouble(1.5), Stacked::LongDouble(2.5)],
        );
        assert_eq!(s, "1.5 2.5");
    }

    #[test]
    fn vsnprintf_long_double_does_not_desync_later_stack_args() {
        // Six `%d` exhaust the GP registers, so the seventh integer comes from
        // the overflow area — placed *after* the 16-byte long double slot.
        // This is the regression that made `%Lf` corrupt every later argument.
        let s = run_vsnprintf_stack(
            b"%d%d%d%d%d%d %.1Lf %d\0",
            &[1, 2, 3, 4, 5, 6],
            &[],
            &[Stacked::LongDouble(0.5), Stacked::Word(99)],
        );
        assert_eq!(s, "123456 0.5 99");
    }

    #[test]
    fn vsnprintf_long_double_realigns_after_an_odd_word() {
        // A single 8-byte stack word leaves the cursor 8-byte aligned; the
        // ABI requires the long double to skip forward to the next multiple
        // of 16 rather than start where the previous argument ended.
        let s = run_vsnprintf_stack(
            b"%d%d%d%d%d%d %d %.1Lf\0",
            &[1, 2, 3, 4, 5, 6],
            &[],
            &[Stacked::Word(7), Stacked::LongDouble(8.5)],
        );
        assert_eq!(s, "123456 7 8.5");
    }

    #[test]
    fn vsnprintf_long_double_wide_exponent_survives_the_x87_decode() {
        // A value far outside f64's *significand* range but inside its
        // exponent range: proves the 80-bit decode reconstructs the exponent
        // rather than reinterpreting the bytes as an f64.
        let s = run_vsnprintf_stack(b"%.3Le\0", &[], &[], &[Stacked::LongDouble(1e300)]);
        assert_eq!(s, "1.000e+300");
    }

    #[test]
    fn vsnprintf_advances_va_offsets() {
        // Formatting two `%d` must consume exactly two GP slots and no XMM
        // slot: `gp_offset` advances by 16, `fp_offset` stays at 48.  The
        // engine reads the va_list in place, so the caller's cursor is
        // observable afterwards — that is what `va_end` exists for in C, and
        // what makes a second pass over the same list illegal.
        let mut reg = [0u8; 176];
        reg[0..8].copy_from_slice(&1u64.to_le_bytes());
        reg[8..16].copy_from_slice(&2u64.to_le_bytes());
        let mut overflow = [0u8; 16];
        let mut va = VaList {
            gp_offset: 0,
            fp_offset: 48,
            overflow_arg_area: overflow.as_mut_ptr(),
            reg_save_area: reg.as_mut_ptr(),
        };
        let mut buf = [0u8; 64];
        // SAFETY: valid synthetic va_list with two integer arguments.
        let n = unsafe {
            vsnprintf(
                buf.as_mut_ptr(),
                buf.len(),
                b"%d %d\0".as_ptr(),
                &raw mut va,
            )
        };
        assert_eq!(n, 3);
        assert_eq!(&buf[..3], b"1 2");
        assert_eq!(va.gp_offset, 16);
        assert_eq!(va.fp_offset, 48);
    }

    /// Regression guard for BUG-POSIX-PRINTF-ARG-ARRAY-OOB: more conversions
    /// than the retired flat `[u64; 8]` arrays could hold.  The ninth `%d`
    /// used to index one past the end of an eight-element stack array.
    #[test]
    fn more_than_eight_integer_conversions() {
        let (s, n) = snprintf_str(
            b"%d %d %d %d %d %d %d %d %d %d %d %d\0",
            &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12],
            &[],
        );
        assert_eq!(s, "1 2 3 4 5 6 7 8 9 10 11 12");
        assert_eq!(n, 26);
    }

    /// The floating-point half of the same bug: nine `%f` conversions.
    #[test]
    fn more_than_eight_float_conversions() {
        let bits: Vec<u64> = (1..=9).map(|i| f64::from(i).to_bits()).collect();
        let (s, _) = snprintf_str(
            b"%.0f%.0f%.0f%.0f%.0f%.0f%.0f%.0f%.0f\0",
            &[],
            &bits,
        );
        assert_eq!(s, "123456789");
    }
}
