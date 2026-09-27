//! Scanf family: `sscanf`, `scanf`, `fscanf` via assembly trampoline.
//!
//! `scanf` is variadic in C, so the three direct entry points are assembly
//! trampolines that perform a real System V `va_start` and tail-call the
//! corresponding `v*` function.  The `v*` variants take that `va_list`
//! directly; since a `va_list` parameter decays to a pointer on the x86_64
//! System V ABI, they are ordinary Rust functions, reachable from host
//! `cargo test`.  The glibc `__isoc99_*scanf` aliases are provided too.
//!
//! There is one engine: glibc's (`stdio-common/vfscanf-internal.c`, byte
//! path, C locale) ported.  It reads one character at a time and gives back
//! the one character it looked at too far, as glibc's `inchar`/`ungetc` do --
//! over a string for `sscanf`, and through the stream for `scanf` and
//! `fscanf`, so what a call does not use stays in the stream for the next.
//!
//! ## Conversions (glibc's)
//!
//! - integers `%d %i %u %o %x %X %b` with `hh h l ll L q j z t` and C23's
//!   `w8`..`w64`, `wf8`..`wf64` (the fast types' widths musl's, §1119);
//!   `%p`, which also reads glibc's `(nil)`; converted by `strtol` and its
//!   kin, so an out-of-range value saturates with `ERANGE`;
//! - floats `%e %f %g %a` and their capitals, with `l` and `L`: glibc's
//!   grammar (`nan`, `inf`, `infinity`, `0x` hex, one `.`, a signed
//!   exponent), converted by `strtof`, `strtod`, `strtold`;
//! - `%s`, `%c`, `%[...]`, and their wide forms `%ls`, `%lc`, `%l[` (and
//!   `%S`, `%C`), which store multibyte input as `wchar_t`;
//! - `%n`, `%%`, `*`, a width, `m` (the destination is allocated; freed
//!   again if the call ends in `EOF`) and `%N$`.

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
// The v* entry points
// ---------------------------------------------------------------------------
//
// `vsscanf`/`vscanf`/`vfscanf` receive an already-initialised `va_list` (which
// decays to a pointer on the x86_64 System V ABI), so they are plain Rust
// functions -- host-testable, and the sole route into the engine: the direct
// `sscanf`/`scanf`/`fscanf` trampolines above synthesise a `va_list` and call
// straight into these.  Each destination pointer is pulled from the `va_list`
// where its conversion stores (BUG-POSIX-SCANF-ARG-ARRAY-OOB was the fixed
// array this replaced), and `%N$` counts from a copy of the list as it came.

use crate::printf::{self, VaList};

/// Run the engine over `inp` with `ap`'s pointers.
///
/// # Safety
///
/// `fmt` is a C string; `ap` is NULL or a valid `va_list` matching it.
unsafe fn scan_with<I: Input>(inp: &mut I, fmt: *const u8, ap: *mut VaList) -> i32 {
    // SAFETY: the caller's contract.
    let orig = if ap.is_null() { None } else { Some(unsafe { *ap }) };
    // SAFETY: as above.
    let mut seq = unsafe { printf::Args::from_raw(ap) };
    let mut args = ScanArgs { seq: &mut seq, orig };
    vscan(inp, fmt, &mut args)
}

/// `vsscanf(str, fmt, ap)` — `sscanf` with a `va_list`.  A NULL `fmt` is
/// `EINVAL` (glibc's `ARGCHECK`); a NULL `str` is `EFAULT` (§1115: glibc
/// faults reading it).
///
/// # Safety
/// `str`/`fmt` must be valid NUL-terminated strings and `ap` a valid
/// `va_list` whose pointer arguments match the conversions in `fmt`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn vsscanf(input: *const u8, fmt: *const u8, ap: *mut VaList) -> i32 {
    if ap.is_null() {
        return EOF;
    }
    if fmt.is_null() {
        errno::set_errno(errno::EINVAL);
        return EOF;
    }
    if input.is_null() {
        errno::set_errno(errno::EFAULT);
        return EOF;
    }
    let mut inp = StrInput { p: input, i: 0 };
    // SAFETY: the caller's contract.
    unsafe { scan_with(&mut inp, fmt, ap) }
}

/// `vscanf(fmt, ap)` — `scanf` with a `va_list`: [`vfscanf`] on `stdin`.
///
/// # Safety
/// As [`vsscanf`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn vscanf(fmt: *const u8, ap: *mut VaList) -> i32 {
    // SAFETY: forwarded; `stdin` is a stream.
    unsafe { vfscanf(crate::stdio::stdin_stream(), fmt, ap) }
}

/// `vfscanf(stream, fmt, ap)` — `fscanf` with a `va_list`, reading through
/// the stream (held for the call) one character at a time and giving back the
/// one it looked at too far, so what follows is still there for the next call
/// -- `fgets`, `getc` or another `fscanf`.  A wide stream answers `EOF`; one not
/// open for reading is `EBADF`, a stream error and `EOF` (glibc's
/// `ARGCHECK`); a NULL `fmt` `EINVAL`.
///
/// # Safety
/// As [`vsscanf`]; `stream` must be a valid `FILE*`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn vfscanf(stream: *mut u8, fmt: *const u8, ap: *mut VaList) -> i32 {
    if ap.is_null() {
        return EOF;
    }
    let Some(s) = crate::stdio::lock_scan_stream(stream) else {
        return EOF;
    };
    if fmt.is_null() {
        errno::set_errno(errno::EINVAL);
        return EOF;
    }
    let mut inp = StreamInput { s, n: 0, ended: false };
    // SAFETY: the caller's contract.
    unsafe { scan_with(&mut inp, fmt, ap) }
}

// ---------------------------------------------------------------------------
// The engine: glibc's vfscanf, over a source with one character of pushback
// ---------------------------------------------------------------------------
//
// A port of glibc 2.40's `stdio-common/vfscanf-internal.c`, byte path, C
// locale: the same directive parser, the same conversions and -- what makes
// the difference -- the same way of reading.  glibc reads one character at a
// time (`inchar`) and, when a conversion has read one character too many,
// gives that one back (`ungetc`); it never looks further ahead than that,
// because a stream cannot.  So every conversion here consumes exactly what
// glibc's consumes: `0x` followed by no hex digit is consumed by `%x`, the
// `e` of `1ex` by `%f`, the `infin` of `infinx` by a `%f` that then fails.
// `sscanf` runs the same engine over a string.
//
// Until 2026-09-27 the engine read a NUL-terminated string with arbitrary
// look-ahead, and `fscanf` fed it one line read straight from the file
// descriptor (`known-issues.md` -> `D-POSIX-SCANF-READS-AROUND-THE-STREAM`),
// so `scanf("%d")` on `12 34` threw the 34 away and a format could not span
// lines.  It also stored `%hd` and `%hhd` as four bytes -- overwriting the
// memory after a `short` or a `char` -- and had no `%m`, `%p`, `%b`, `%lc`,
// `%ls`, `%l[` or `%N$`.

use crate::errno;

/// `l`.
const LONG: u32 = 1;
/// `L`, `ll`, `q`: `long long` and `long double`.
const LONGDBL: u32 = 1 << 1;
/// `h`.
const SHORT: u32 = 1 << 2;
/// `hh`.
const CHAR: u32 = 1 << 3;
/// `*`.
const SUPPRESS: u32 = 1 << 4;
/// `m`: the conversion allocates its destination.
const MALLOC: u32 = 1 << 5;
/// `%d` and `%i`.
const NUMBER_SIGNED: u32 = 1 << 6;
/// `%p`.
const READ_POINTER: u32 = 1 << 7;
/// A `0x` float.
const HEXA_FLOAT: u32 = 1 << 8;

/// `isspace` in the C locale.
fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

/// Where the characters come from: glibc's `inchar` and `ungetc`.
trait Input {
    /// The next character, consumed; `None` at the end, which stays the end
    /// for the rest of the call.
    fn get(&mut self) -> Option<u8>;
    /// Give back `c`, the character `get` returned last.
    fn unget(&mut self, c: u8);
    /// Characters consumed so far: `%n`'s answer.
    fn read_in(&self) -> usize;
}

/// `sscanf`'s input: a C string, whose NUL is the end.
struct StrInput {
    p: *const u8,
    i: usize,
}

impl Input for StrInput {
    fn get(&mut self) -> Option<u8> {
        // SAFETY: a C string, and `i` never passes its NUL.
        let c = unsafe { *self.p.add(self.i) };
        if c == 0 {
            return None;
        }
        self.i = self.i.wrapping_add(1);
        Some(c)
    }
    fn unget(&mut self, _c: u8) {
        self.i = self.i.saturating_sub(1);
    }
    fn read_in(&self) -> usize {
        self.i
    }
}

/// `fscanf`'s input: a stream, held for the call.  A NUL is a character like
/// any other there.
struct StreamInput {
    s: crate::stdio::ScanStream,
    n: usize,
    ended: bool,
}

impl Input for StreamInput {
    fn get(&mut self) -> Option<u8> {
        if self.ended {
            return None;
        }
        match u8::try_from(self.s.getc()) {
            Ok(c) => {
                self.n = self.n.wrapping_add(1);
                Some(c)
            }
            Err(_) => {
                self.ended = true;
                None
            }
        }
    }
    fn unget(&mut self, c: u8) {
        self.n = self.n.saturating_sub(1);
        self.s.unget(c);
    }
    fn read_in(&self) -> usize {
        self.n
    }
}

/// The destination pointers: the next in order, or the `N`th for `%N$`.
struct ScanArgs<'a, 'v> {
    seq: &'a mut printf::Args<'v>,
    /// The `va_list` as the call received it, to count from for `%N$`.
    orig: Option<VaList>,
}

impl ScanArgs<'_, '_> {
    fn ptr(&mut self, pos: usize) -> u64 {
        if pos == 0 {
            return self.seq.int();
        }
        let Some(mut va) = self.orig else {
            return 0;
        };
        let mut v = 0;
        for _ in 0..pos {
            // SAFETY: the caller's `va_list` holds at least `pos` pointers,
            // by the contract `%N$` puts on it.
            v = unsafe { printf::va_arg_int(&mut va) };
        }
        v
    }
}

/// glibc's `char_buffer`: the text of a number, grown as needed.
struct CharBuf {
    inline: [u8; 128],
    heap: *mut u8,
    len: usize,
    cap: usize,
    failed: bool,
}

impl CharBuf {
    const fn new() -> Self {
        Self { inline: [0; 128], heap: core::ptr::null_mut(), len: 0, cap: 128, failed: false }
    }
    fn data(&mut self) -> *mut u8 {
        if self.heap.is_null() { self.inline.as_mut_ptr() } else { self.heap }
    }
    fn rewind(&mut self) {
        self.len = 0;
        self.failed = false;
    }
    fn push(&mut self, c: u8) {
        if self.failed {
            return;
        }
        if self.len == self.cap {
            let Some(cap) = self.cap.checked_mul(2) else {
                self.failed = true;
                return;
            };
            let nb = crate::malloc::malloc(cap);
            if nb.is_null() {
                self.failed = true;
                return;
            }
            let old = self.data();
            // SAFETY: `len` bytes at `old`, into a larger new buffer.
            unsafe { core::ptr::copy_nonoverlapping(old, nb, self.len) };
            // SAFETY: the old heap buffer (or null), ours.
            unsafe { crate::malloc::free(self.heap) };
            self.heap = nb;
            self.cap = cap;
        }
        let at = self.len;
        // SAFETY: `at < cap`.
        unsafe { *self.data().add(at) = c };
        self.len = at.wrapping_add(1);
    }
    fn size(&self) -> usize {
        self.len
    }
    fn at(&mut self, i: usize) -> u8 {
        if i >= self.len {
            return 0;
        }
        // SAFETY: `i < len`.
        unsafe { *self.data().add(i) }
    }
    /// The text, terminated.
    fn cstr(&mut self) -> *const u8 {
        self.push(0);
        self.data()
    }
}

impl Drop for CharBuf {
    fn drop(&mut self) {
        // SAFETY: ours, or null.
        unsafe { crate::malloc::free(self.heap) };
    }
}

/// The `%m` destinations filled so far, freed and set to NULL if the call
/// ends in `EOF`: glibc's `ptrs_to_free`.
struct Allocs {
    list: *mut *mut *mut u8,
    len: usize,
    cap: usize,
}

impl Allocs {
    const fn new() -> Self {
        Self { list: core::ptr::null_mut(), len: 0, cap: 0 }
    }
    fn push(&mut self, p: *mut *mut u8) -> bool {
        if self.len == self.cap {
            let cap = if self.cap == 0 { 8 } else { self.cap.saturating_mul(2) };
            let Some(bytes) = cap.checked_mul(core::mem::size_of::<*mut *mut u8>()) else {
                return false;
            };
            // SAFETY: ours, or null.
            let nl = unsafe { crate::malloc::realloc(self.list.cast(), bytes) }.cast::<*mut *mut u8>();
            if nl.is_null() {
                return false;
            }
            self.list = nl;
            self.cap = cap;
        }
        // SAFETY: `len < cap`.
        unsafe { *self.list.add(self.len) = p };
        self.len = self.len.wrapping_add(1);
        true
    }
    fn free_all(&mut self) {
        for i in 0..self.len {
            // SAFETY: each entry is a caller's `char **` that we filled.
            unsafe {
                let p = *self.list.add(i);
                crate::malloc::free(*p);
                *p = core::ptr::null_mut();
            }
        }
        self.len = 0;
    }
}

impl Drop for Allocs {
    fn drop(&mut self) {
        // SAFETY: ours, or null.
        unsafe { crate::malloc::free(self.list.cast()) };
    }
}

/// Why a scan stopped early: glibc's `input_error`, `conv_error`,
/// `encode_error` and its out-of-memory `done = EOF`.
enum Stop {
    Input,
    Conv,
    Encode,
    NoMem,
}

type Step = Result<(), Stop>;

/// One call's state.
struct Engine<'x, 'a, 'v, I: Input> {
    inp: &'x mut I,
    args: &'x mut ScanArgs<'a, 'v>,
    f: *const u8,
    done: i32,
    allocs: Allocs,
    /// The `%m` buffer of the conversion in progress, freed if it fails.
    strptr: *mut *mut u8,
    charbuf: CharBuf,
}

/// Run `fmt` over `inp`.  The number of assignments, or `EOF` if the input
/// ended (or memory ran out) before the first.
fn vscan<I: Input>(inp: &mut I, fmt: *const u8, args: &mut ScanArgs<'_, '_>) -> i32 {
    let mut e = Engine {
        inp,
        args,
        f: fmt,
        done: 0,
        allocs: Allocs::new(),
        strptr: core::ptr::null_mut(),
        charbuf: CharBuf::new(),
    };
    match e.run() {
        Ok(()) | Err(Stop::Conv | Stop::Encode) => {}
        Err(Stop::Input) => {
            if e.done == 0 {
                e.done = EOF;
            }
        }
        Err(Stop::NoMem) => e.done = EOF,
    }
    if e.done == EOF {
        e.allocs.free_all();
    } else if !e.strptr.is_null() {
        // SAFETY: the caller's `char **`, which we filled.
        unsafe {
            crate::malloc::free(*e.strptr);
            *e.strptr = core::ptr::null_mut();
        }
    }
    e.done
}

const EOF: i32 = -1;

impl<I: Input> Engine<'_, '_, '_, I> {
    /// The format byte under the cursor.
    fn fpeek(&self) -> u8 {
        // SAFETY: the format is a C string, and the cursor stops at its NUL.
        unsafe { *self.f }
    }
    fn fnext(&mut self) -> u8 {
        let c = self.fpeek();
        if c != 0 {
            // SAFETY: not past the NUL.
            self.f = unsafe { self.f.add(1) };
        }
        c
    }
    /// glibc's `read_int`: a decimal number, saturating.
    fn read_int(&mut self) -> usize {
        let mut n: usize = 0;
        while self.fpeek().is_ascii_digit() {
            n = n.saturating_mul(10).saturating_add(usize::from(self.fpeek().wrapping_sub(b'0')));
            self.fnext();
        }
        n
    }

    fn run(&mut self) -> Step {
        let mut skip_space = false;
        while self.fpeek() != 0 {
            // A multibyte character in the format must match the same bytes.
            if !self.fpeek().is_ascii() {
                let len = utf8_len(self.fpeek());
                for _ in 0..len {
                    let want = self.fpeek();
                    if want == 0 {
                        break;
                    }
                    let c = self.inp.get().ok_or(Stop::Input)?;
                    if c != want {
                        self.inp.unget(c);
                        return Err(Stop::Conv);
                    }
                    self.fnext();
                }
                continue;
            }
            let fc = self.fnext();
            if fc != b'%' {
                if is_space(fc) {
                    skip_space = true;
                    continue;
                }
                let mut c = self.inp.get().ok_or(Stop::Input)?;
                if skip_space {
                    while is_space(c) {
                        c = self.inp.get().ok_or(Stop::Input)?;
                    }
                    skip_space = false;
                }
                if c != fc {
                    self.inp.unget(c);
                    return Err(Stop::Conv);
                }
                continue;
            }
            self.directive(&mut skip_space)?;
        }
        // Whitespace ended the format: consume the input's.
        if skip_space {
            while let Some(c) = self.inp.get() {
                if !is_space(c) {
                    self.inp.unget(c);
                    break;
                }
            }
        }
        Ok(())
    }

    /// One `%` directive, from just after the `%`.
    #[allow(clippy::too_many_lines)]
    fn directive(&mut self, skip_space: &mut bool) -> Step {
        let mut flags: u32 = 0;
        let mut argpos: usize = 0;
        let mut width: i64;
        self.charbuf.rewind();

        let mut have_width = None;
        if self.fpeek().is_ascii_digit() {
            let n = self.read_int();
            if self.fpeek() == b'$' {
                self.fnext();
                argpos = n;
            } else {
                // That was the field width.
                have_width = Some(n);
            }
        }
        if let Some(n) = have_width {
            width = i64::try_from(n).unwrap_or(i64::MAX);
        } else {
            // `*`, and glibc's `'` (grouping: nothing to group in the C
            // locale) and `I` (locale digits: the C locale has none).
            while matches!(self.fpeek(), b'*' | b'\'' | b'I') {
                if self.fnext() == b'*' {
                    flags |= SUPPRESS;
                }
            }
            width = 0;
            if self.fpeek().is_ascii_digit() {
                width = i64::try_from(self.read_int()).unwrap_or(i64::MAX);
            }
        }
        if width == 0 {
            width = -1;
        }

        // Type modifiers.
        match self.fpeek() {
            b'h' => {
                self.fnext();
                if self.fpeek() == b'h' {
                    self.fnext();
                    flags |= CHAR;
                } else {
                    flags |= SHORT;
                }
            }
            b'l' => {
                self.fnext();
                if self.fpeek() == b'l' {
                    self.fnext();
                    flags |= LONGDBL | LONG;
                } else {
                    flags |= LONG;
                }
            }
            b'q' | b'L' => {
                self.fnext();
                flags |= LONGDBL | LONG;
            }
            b'm' => {
                self.fnext();
                flags |= MALLOC;
                if self.fpeek() == b'l' {
                    self.fnext();
                    flags |= LONG;
                }
            }
            // `size_t`, `intmax_t`, `ptrdiff_t`: 64 bits, `long` here.
            b'z' | b'j' | b't' => {
                self.fnext();
                flags |= LONG;
            }
            // C23's `w8`...`w64` and `wf8`...`wf64`.  The fast types' widths
            // are musl's (§1119): `int_fast16_t` and `int_fast32_t` are 32
            // bits in the headers a program here is compiled against, where
            // glibc's are 64 -- storing 64 bits into one would overwrite what
            // follows it.
            b'w' => {
                self.fnext();
                let fast = self.fpeek() == b'f';
                if fast {
                    self.fnext();
                }
                let mut bits = self.read_int();
                if fast {
                    bits = match bits {
                        16 => 32,
                        n => n,
                    };
                }
                match bits {
                    8 => flags |= CHAR,
                    16 => flags |= SHORT,
                    32 => {}
                    64 => flags |= LONGDBL | LONG,
                    _ => {
                        errno::set_errno(errno::EINVAL);
                        return Err(Stop::Conv);
                    }
                }
            }
            _ => {}
        }

        if self.fpeek() == 0 {
            return Err(Stop::Conv);
        }
        let fc = self.fnext();

        // Leading whitespace, for every conversion but `[`, `c`, `C` and `n`.
        if *skip_space || !matches!(fc, b'[' | b'c' | b'C' | b'n') {
            let saved = errno::get_errno();
            errno::set_errno(0);
            loop {
                match self.inp.get() {
                    None => {
                        if errno::get_errno() == errno::EINTR {
                            return Err(Stop::Input);
                        }
                        break;
                    }
                    Some(c) if is_space(c) => {}
                    Some(c) => {
                        self.inp.unget(c);
                        break;
                    }
                }
            }
            errno::set_errno(saved);
            *skip_space = false;
        }

        match fc {
            b'%' => {
                let c = self.inp.get().ok_or(Stop::Input)?;
                if c != b'%' {
                    self.inp.unget(c);
                    return Err(Stop::Conv);
                }
                Ok(())
            }
            b'n' => {
                if flags & SUPPRESS == 0 {
                    let n = self.inp.read_in();
                    let p = self.args.ptr(argpos);
                    if p == 0 {
                        return Err(Stop::Conv);
                    }
                    store_int(p, flags, n as u64);
                }
                Ok(())
            }
            b'c' if flags & LONG == 0 => self.conv_c(flags, argpos, width),
            b'c' | b'C' => self.conv_lc(flags | LONG, argpos, width),
            b's' if flags & LONG == 0 => self.conv_s(flags, argpos, width),
            b's' | b'S' => self.conv_ls(flags | LONG, argpos, width),
            b'x' | b'X' => self.number(flags, argpos, width, 16),
            b'o' => self.number(flags, argpos, width, 8),
            b'b' => self.number(flags, argpos, width, 2),
            b'u' => self.number(flags, argpos, width, 10),
            b'd' => self.number(flags | NUMBER_SIGNED, argpos, width, 10),
            b'i' => self.number(flags | NUMBER_SIGNED, argpos, width, 0),
            b'p' => {
                let f = (flags & !(SHORT | LONGDBL | CHAR)) | LONG | READ_POINTER;
                self.number(f, argpos, width, 16)
            }
            b'e' | b'E' | b'f' | b'F' | b'g' | b'G' | b'a' | b'A' => self.float(flags, argpos, width),
            b'[' => self.scanset(flags, argpos, width),
            _ => Err(Stop::Conv),
        }
    }

    /// glibc's `STRING_ARG`: the destination of a string conversion, or a
    /// fresh `malloc` of `initial` elements of `size` for `%m`.  NULL is a
    /// conversion error, as glibc makes it.
    fn string_arg(&mut self, flags: u32, argpos: usize, initial: usize, size: usize) -> Result<*mut u8, Stop> {
        let p = self.args.ptr(argpos);
        if flags & MALLOC == 0 {
            if p == 0 {
                return Err(Stop::Conv);
            }
            return Ok(p as *mut u8);
        }
        let strptr = p as *mut *mut u8;
        if strptr.is_null() {
            return Err(Stop::Conv);
        }
        let bytes = initial.max(1).saturating_mul(size);
        let buf = crate::malloc::malloc(bytes);
        if buf.is_null() {
            return Err(Stop::NoMem);
        }
        // SAFETY: the caller's `char **`.
        unsafe { *strptr = buf };
        if !self.allocs.push(strptr) {
            return Err(Stop::NoMem);
        }
        self.strptr = strptr;
        Ok(buf)
    }

    /// Grow a `%m` buffer from `used` elements of `size` to at least one more,
    /// as glibc does (doubling, then one if that fails): the new write
    /// position, or `None` when memory ran out.
    fn grow(&mut self, used: usize, size: usize, cap: &mut usize) -> Option<*mut u8> {
        let strptr = self.strptr;
        let tries = [cap.saturating_mul(2), cap.saturating_add(1)];
        for want in tries {
            let bytes = want.checked_mul(size)?;
            // SAFETY: the buffer `string_arg` made.
            let nb = unsafe { crate::malloc::realloc(*strptr, bytes) };
            if !nb.is_null() {
                // SAFETY: the caller's `char **`; `used` elements fit.
                unsafe {
                    *strptr = nb;
                    *cap = want;
                    return Some(nb.add(used.wrapping_mul(size)));
                }
            }
        }
        None
    }

    /// A finished `%m` buffer shrunk to `used` elements of `size`.
    fn shrink(&mut self, used: usize, size: usize, cap: usize) {
        if used != cap {
            let strptr = self.strptr;
            // SAFETY: the buffer `string_arg` made.
            let nb = unsafe { crate::malloc::realloc(*strptr, used.max(1).saturating_mul(size)) };
            if !nb.is_null() {
                // SAFETY: the caller's `char **`.
                unsafe { *strptr = nb };
            }
        }
    }

    /// `%c`: `width` characters (1 without one), whitespace included, no
    /// terminator.
    fn conv_c(&mut self, flags: u32, argpos: usize, width: i64) -> Step {
        let mut width = if width == -1 { 1 } else { width };
        let initial = usize::try_from(width.min(1024)).unwrap_or(1);
        let mut out = core::ptr::null_mut::<u8>();
        let mut cap = initial;
        if flags & SUPPRESS == 0 {
            out = self.string_arg(flags, argpos, initial, 1)?;
        }
        let mut c = self.inp.get().ok_or(Stop::Input)?;
        let mut used = 0usize;
        loop {
            if flags & SUPPRESS == 0 {
                if flags & MALLOC != 0 && used == cap {
                    out = self.grow(used, 1, &mut cap).ok_or(Stop::NoMem)?;
                }
                // SAFETY: `used < cap` (or the caller's `width` bytes).
                unsafe {
                    *out = c;
                    out = out.add(1);
                }
                used = used.wrapping_add(1);
            }
            width = width.saturating_sub(1);
            if width <= 0 {
                break;
            }
            match self.inp.get() {
                Some(n) => c = n,
                None => break,
            }
        }
        if flags & SUPPRESS == 0 {
            if flags & MALLOC != 0 {
                self.shrink(used, 1, cap);
            }
            self.strptr = core::ptr::null_mut();
            self.done = self.done.saturating_add(1);
        }
        Ok(())
    }

    /// Decode one multibyte character starting with `c` (reading more as
    /// needed): glibc's `mbrtowc` loop.  `Err(Encode)` for no character.
    fn wide_char(&mut self, c: u8, state: &mut crate::wchar::MbstateT) -> Result<crate::wchar::WcharT, Stop> {
        let mut byte = c;
        loop {
            let mut wc: crate::wchar::WcharT = 0;
            // SAFETY: one byte at `byte`; the state is ours.
            let n = unsafe { crate::wchar::mbrtowc(&raw mut wc, &raw const byte, 1, state) };
            if n == usize::MAX.wrapping_sub(1) {
                // A character begun, not finished.
                match self.inp.get() {
                    Some(next) => {
                        byte = next;
                        continue;
                    }
                    None => {
                        errno::set_errno(errno::EILSEQ);
                        return Err(Stop::Encode);
                    }
                }
            }
            if n != 1 {
                // glibc's `n != 1`: an invalid sequence, and -- its quirk,
                // kept -- a NUL, which mbrtowc answers 0 for.
                errno::set_errno(errno::EILSEQ);
                return Err(Stop::Encode);
            }
            return Ok(wc);
        }
    }

    /// `%lc`: `width` multibyte characters as `wchar_t`s.
    fn conv_lc(&mut self, flags: u32, argpos: usize, width: i64) -> Step {
        let mut width = if width == -1 { 1 } else { width };
        let size = core::mem::size_of::<crate::wchar::WcharT>();
        let initial = usize::try_from(width.min(1024)).unwrap_or(1);
        let mut cap = initial;
        let mut out = core::ptr::null_mut::<u8>();
        if flags & SUPPRESS == 0 {
            out = self.string_arg(flags, argpos, initial, size)?;
        }
        let mut c = self.inp.get().ok_or(Stop::Input)?;
        let mut state = crate::wchar::MbstateT::new();
        let mut used = 0usize;
        loop {
            if flags & SUPPRESS == 0 && flags & MALLOC != 0 && used == cap {
                out = self.grow(used, size, &mut cap).ok_or(Stop::NoMem)?;
            }
            let wc = self.wide_char(c, &mut state)?;
            if flags & SUPPRESS == 0 {
                // SAFETY: room for `used + 1` wide characters.
                unsafe {
                    out.cast::<crate::wchar::WcharT>().write_unaligned(wc);
                    out = out.add(size);
                }
                used = used.wrapping_add(1);
            }
            width = width.saturating_sub(1);
            if width <= 0 {
                break;
            }
            match self.inp.get() {
                Some(n) => c = n,
                None => break,
            }
        }
        if flags & SUPPRESS == 0 {
            if flags & MALLOC != 0 {
                self.shrink(used, size, cap);
            }
            self.strptr = core::ptr::null_mut();
            self.done = self.done.saturating_add(1);
        }
        Ok(())
    }

    /// `%s`: characters up to whitespace, terminated.
    fn conv_s(&mut self, flags: u32, argpos: usize, width: i64) -> Step {
        let mut width = width;
        let mut cap = 100usize;
        let mut out = core::ptr::null_mut::<u8>();
        if flags & SUPPRESS == 0 {
            out = self.string_arg(flags, argpos, cap, 1)?;
        }
        let mut c = self.inp.get().ok_or(Stop::Input)?;
        let mut used = 0usize;
        loop {
            if is_space(c) {
                self.inp.unget(c);
                break;
            }
            if flags & SUPPRESS == 0 {
                // SAFETY: room for one more (grown below when `%m`).
                unsafe {
                    *out = c;
                    out = out.add(1);
                }
                used = used.wrapping_add(1);
                if flags & MALLOC != 0 && used == cap {
                    out = self.grow(used, 1, &mut cap).ok_or(Stop::NoMem)?;
                }
            }
            if width > 0 {
                width = width.wrapping_sub(1);
                if width == 0 {
                    break;
                }
            }
            match self.inp.get() {
                Some(n) => c = n,
                None => break,
            }
        }
        if flags & SUPPRESS == 0 {
            if flags & MALLOC != 0 && used == cap {
                out = self.grow(used, 1, &mut cap).ok_or(Stop::NoMem)?;
            }
            // SAFETY: room for the terminator.
            unsafe { *out = 0 };
            if flags & MALLOC != 0 {
                self.shrink(used.wrapping_add(1), 1, cap);
            }
            self.strptr = core::ptr::null_mut();
            self.done = self.done.saturating_add(1);
        }
        Ok(())
    }

    /// `%ls`: multibyte characters up to whitespace, as a `wchar_t` string.
    fn conv_ls(&mut self, flags: u32, argpos: usize, width: i64) -> Step {
        let mut width = width;
        let size = core::mem::size_of::<crate::wchar::WcharT>();
        let mut cap = 100usize;
        let mut out = core::ptr::null_mut::<u8>();
        if flags & SUPPRESS == 0 {
            out = self.string_arg(flags, argpos, cap, size)?;
        }
        let mut c = self.inp.get().ok_or(Stop::Input)?;
        let mut state = crate::wchar::MbstateT::new();
        let mut used = 0usize;
        loop {
            if is_space(c) {
                self.inp.unget(c);
                break;
            }
            let wc = self.wide_char(c, &mut state)?;
            if flags & SUPPRESS == 0 {
                // SAFETY: room for one more (grown below when `%m`).
                unsafe {
                    out.cast::<crate::wchar::WcharT>().write_unaligned(wc);
                    out = out.add(size);
                }
                used = used.wrapping_add(1);
                if flags & MALLOC != 0 && used == cap {
                    out = self.grow(used, size, &mut cap).ok_or(Stop::NoMem)?;
                }
            }
            if width > 0 {
                width = width.wrapping_sub(1);
                if width == 0 {
                    break;
                }
            }
            match self.inp.get() {
                Some(n) => c = n,
                None => break,
            }
        }
        if flags & SUPPRESS == 0 {
            if flags & MALLOC != 0 && used == cap {
                out = self.grow(used, size, &mut cap).ok_or(Stop::NoMem)?;
            }
            // SAFETY: room for the terminator.
            unsafe { out.cast::<crate::wchar::WcharT>().write_unaligned(0) };
            if flags & MALLOC != 0 {
                self.shrink(used.wrapping_add(1), size, cap);
            }
            self.strptr = core::ptr::null_mut();
            self.done = self.done.saturating_add(1);
        }
        Ok(())
    }

    /// Integers: glibc's `number:`.  The text is collected as glibc collects
    /// it and converted with `strtol` and its kin, so overflow saturates as
    /// theirs does.
    #[allow(clippy::too_many_lines)]
    fn number(&mut self, flags: u32, argpos: usize, width: i64, base: i32) -> Step {
        let mut flags = flags;
        let mut width = width;
        let mut base = base;
        let c = self.inp.get().ok_or(Stop::Input)?;
        let mut cur: Option<u8> = Some(c);

        // A sign.
        if c == b'-' || c == b'+' {
            self.charbuf.push(c);
            if width > 0 {
                width = width.wrapping_sub(1);
            }
            cur = self.inp.get();
        }
        // A leading base indication.
        if width != 0 && cur == Some(b'0') {
            if width > 0 {
                width = width.wrapping_sub(1);
            }
            self.charbuf.push(b'0');
            cur = self.inp.get();
            let lower = cur.map(|x| x.to_ascii_lowercase());
            if width != 0 && lower == Some(b'x') {
                if base == 0 {
                    base = 16;
                }
                if base == 16 {
                    if width > 0 {
                        width = width.wrapping_sub(1);
                    }
                    cur = self.inp.get();
                }
            } else if width != 0 && lower == Some(b'b') && base == 2 {
                // glibc's plain and C99 scanf take a `0b` only for `%b`;
                // `%i` takes one only in C23 mode, which has entry points of
                // its own (`__isoc23_*`) that this library does not provide.
                base = 2;
                if width > 0 {
                    width = width.wrapping_sub(1);
                }
                cur = self.inp.get();
            } else if base == 0 {
                base = 8;
            }
        }
        if base == 0 {
            base = 10;
        }
        // The digits.
        while let Some(d) = cur {
            if width == 0 {
                break;
            }
            let ok = match base {
                16 => d.is_ascii_hexdigit(),
                _ => d.is_ascii_digit() && i32::from(d.wrapping_sub(b'0')) < base,
            };
            if !ok {
                break;
            }
            self.charbuf.push(d);
            if width > 0 {
                width = width.wrapping_sub(1);
            }
            cur = self.inp.get();
        }
        if self.charbuf.failed {
            errno::set_errno(errno::ENOMEM);
            return Err(Stop::NoMem);
        }
        let size = self.charbuf.size();
        let only_sign = size == 1 && matches!(self.charbuf.at(0), b'+' | b'-');
        if size == 0 || only_sign {
            // No number: but `%p` reads glibc's "(nil)".
            let nil = size == 0
                && flags & READ_POINTER != 0
                && (width < 0 || width >= 5)
                && cur == Some(b'(')
                && match self.read_nil() {
                    Ok(()) => true,
                    Err(last) => {
                        cur = last;
                        false
                    }
                };
            if nil {
                self.charbuf.push(b'0');
            } else {
                if let Some(x) = cur {
                    self.inp.unget(x);
                }
                return Err(Stop::Conv);
            }
        } else if let Some(x) = cur {
            // The character that stopped the number is not part of it.
            self.inp.unget(x);
        }
        let text = self.charbuf.cstr();
        if self.charbuf.failed {
            errno::set_errno(errno::ENOMEM);
            return Err(Stop::NoMem);
        }
        let mut end: *const u8 = core::ptr::null();
        // SAFETY: `text` is the terminated text; `end` is ours.
        let bits: u64 = unsafe {
            if flags & NUMBER_SIGNED != 0 {
                crate::stdlib::strtoll(text, &raw mut end, base) as u64
            } else {
                crate::stdlib::strtoull(text, &raw mut end, base)
            }
        };
        if core::ptr::eq(end, text) {
            return Err(Stop::Conv);
        }
        if flags & SUPPRESS == 0 {
            let p = self.args.ptr(argpos);
            if p == 0 {
                return Err(Stop::Conv);
            }
            if flags & READ_POINTER != 0 {
                flags = (flags & !(SHORT | CHAR)) | LONG;
            }
            store_int(p, flags, bits);
            self.done = self.done.saturating_add(1);
        }
        Ok(())
    }

    /// The rest of glibc's `(nil)` after its `(`: `Err` with the byte that did
    /// not match (or `None` at the end), which the caller gives back -- as
    /// glibc's `ungetc(c)` after its `inchar` chain gives back the last one.
    fn read_nil(&mut self) -> Result<(), Option<u8>> {
        for want in [b'n', b'i', b'l'] {
            match self.inp.get() {
                Some(x) if x.to_ascii_lowercase() == want => {}
                other => return Err(other),
            }
        }
        match self.inp.get() {
            Some(b')') => Ok(()),
            other => Err(other),
        }
    }

    /// Floats: glibc's `e`..`A` conversions.  The text is collected by
    /// glibc's grammar -- `nan`, `inf`, `infinity` (no `nan(...)` payload:
    /// glibc's scanf reads none), `0x` hex with a `p` exponent, digits, one
    /// `.`, an exponent with a sign -- and converted by `strtod` and its kin.
    #[allow(clippy::too_many_lines)]
    fn float(&mut self, flags: u32, argpos: usize, width: i64) -> Step {
        let mut flags = flags;
        let mut width = width;
        let mut c = self.inp.get().ok_or(Stop::Input)?;
        if width > 0 {
            width = width.wrapping_sub(1);
        }
        let (mut got_digit, mut got_dot, mut got_e) = (false, false, false);
        let mut got_sign = 0usize;
        // A sign.
        if c == b'-' || c == b'+' {
            got_sign = 1;
            self.charbuf.push(c);
            if width == 0 {
                return Err(Stop::Conv);
            }
            c = self.inp.get().ok_or(Stop::Conv)?;
            if width > 0 {
                width = width.wrapping_sub(1);
            }
        }
        let mut named = false;
        let lower = c.to_ascii_lowercase();
        if lower == b'n' {
            // "nan", or nothing.
            self.charbuf.push(c);
            for want in [b'a', b'n'] {
                if width == 0 {
                    return Err(Stop::Conv);
                }
                c = self.inp.get().ok_or(Stop::Conv)?;
                if c.to_ascii_lowercase() != want {
                    return Err(Stop::Conv);
                }
                if width > 0 {
                    width = width.wrapping_sub(1);
                }
                self.charbuf.push(c);
            }
            named = true;
        } else if lower == b'i' {
            // "inf", maybe "infinity".
            self.charbuf.push(c);
            for want in [b'n', b'f'] {
                if width == 0 {
                    return Err(Stop::Conv);
                }
                c = self.inp.get().ok_or(Stop::Conv)?;
                if c.to_ascii_lowercase() != want {
                    return Err(Stop::Conv);
                }
                if width > 0 {
                    width = width.wrapping_sub(1);
                }
                self.charbuf.push(c);
            }
            if width != 0 {
                if let Some(next) = self.inp.get() {
                    if next.eq_ignore_ascii_case(&b'i') {
                        if width > 0 {
                            width = width.wrapping_sub(1);
                        }
                        self.charbuf.push(next);
                        for want in [b'n', b'i', b't', b'y'] {
                            if width == 0 {
                                return Err(Stop::Conv);
                            }
                            c = self.inp.get().ok_or(Stop::Conv)?;
                            if c.to_ascii_lowercase() != want {
                                return Err(Stop::Conv);
                            }
                            if width > 0 {
                                width = width.wrapping_sub(1);
                            }
                            self.charbuf.push(c);
                        }
                    } else {
                        self.inp.unget(next);
                    }
                }
            }
            named = true;
        }
        if !named {
            let mut exp_char = b'e';
            let mut cur = Some(c);
            if width != 0 && c == b'0' {
                self.charbuf.push(c);
                cur = self.inp.get();
                if width > 0 {
                    width = width.wrapping_sub(1);
                }
                if let Some(x) = cur.filter(|x| width != 0 && x.eq_ignore_ascii_case(&b'x')) {
                    // Hexadecimal.
                    self.charbuf.push(x);
                    flags |= HEXA_FLOAT;
                    exp_char = b'p';
                    cur = self.inp.get();
                    if width > 0 {
                        width = width.wrapping_sub(1);
                    }
                } else {
                    got_digit = true;
                }
            }
            while let Some(d) = cur {
                let last = self.charbuf.size().checked_sub(1).map(|i| self.charbuf.at(i));
                if d.is_ascii_digit() {
                    self.charbuf.push(d);
                    got_digit = true;
                } else if !got_e && flags & HEXA_FLOAT != 0 && d.is_ascii_hexdigit() {
                    self.charbuf.push(d);
                    got_digit = true;
                } else if got_e && last == Some(exp_char) && (d == b'-' || d == b'+') {
                    self.charbuf.push(d);
                } else if got_digit && !got_e && d.to_ascii_lowercase() == exp_char {
                    self.charbuf.push(exp_char);
                    got_e = true;
                    got_dot = true;
                } else if !got_dot && d == b'.' {
                    self.charbuf.push(d);
                    got_dot = true;
                } else {
                    // Not part of the number.
                    self.inp.unget(d);
                    break;
                }
                if width == 0 {
                    break;
                }
                cur = self.inp.get();
                if cur.is_none() {
                    break;
                }
                if width > 0 {
                    width = width.wrapping_sub(1);
                }
            }
            if self.charbuf.failed {
                errno::set_errno(errno::ENOMEM);
                return Err(Stop::NoMem);
            }
            // Nothing but a sign, or a bare "0x": no number.
            let size = self.charbuf.size();
            if size == got_sign || (flags & HEXA_FLOAT != 0 && size == got_sign.wrapping_add(2)) {
                return Err(Stop::Conv);
            }
        }
        let _ = (got_dot, got_digit);
        let text = self.charbuf.cstr();
        if self.charbuf.failed {
            errno::set_errno(errno::ENOMEM);
            return Err(Stop::NoMem);
        }
        let mut end: *const u8 = core::ptr::null();
        let p = if flags & SUPPRESS == 0 { self.args.ptr(argpos) } else { 0 };
        // SAFETY: `text` is the terminated text; each store is to the
        // caller's pointer of the type its modifier names.
        unsafe {
            if flags & LONGDBL != 0 {
                let d = crate::stdlib::strtold(text, &raw mut end);
                if p != 0 && !core::ptr::eq(end, text) {
                    (p as *mut crate::x87::LongDouble).write_unaligned(crate::x87::from_f64(d));
                }
            } else if flags & LONG != 0 {
                let d = crate::stdlib::strtod(text, &raw mut end);
                if p != 0 && !core::ptr::eq(end, text) {
                    (p as *mut f64).write_unaligned(d);
                }
            } else {
                let d = crate::stdlib::strtof(text, &raw mut end);
                if p != 0 && !core::ptr::eq(end, text) {
                    (p as *mut f32).write_unaligned(d);
                }
            }
        }
        if core::ptr::eq(end, text) {
            return Err(Stop::Conv);
        }
        if flags & SUPPRESS == 0 {
            if p == 0 {
                return Err(Stop::Conv);
            }
            self.done = self.done.saturating_add(1);
        }
        Ok(())
    }

    /// `%[`: glibc's byte scanset -- a leading `]` or `-` is a member, `a-z`
    /// a range only when `a <= z` (else three members), and a set with no
    /// closing `]` a conversion error.
    #[allow(clippy::too_many_lines)]
    fn scanset(&mut self, flags: u32, argpos: usize, width: i64) -> Step {
        let mut width = width;
        let wide = flags & LONG != 0;
        let size = if wide { core::mem::size_of::<crate::wchar::WcharT>() } else { 1 };
        let mut cap = 100usize;
        let mut out = core::ptr::null_mut::<u8>();
        if flags & SUPPRESS == 0 {
            out = self.string_arg(flags, argpos, cap, size)?;
        }
        let not_in = self.fpeek() == b'^';
        if not_in {
            self.fnext();
        }
        let mut set = [false; 256];
        let first = self.fpeek();
        let mut prev: u8 = 0;
        if first == b']' || first == b'-' {
            if let Some(slot) = set.get_mut(usize::from(first)) {
                *slot = true;
            }
            self.fnext();
            prev = first;
        }
        loop {
            let fc = self.fnext();
            if fc == 0 {
                return Err(Stop::Conv);
            }
            if fc == b']' {
                break;
            }
            let next = self.fpeek();
            if fc == b'-' && next != 0 && next != b']' && prev != 0 && prev <= next {
                // A range: from the member before the `-` up to, not
                // including, the next; the next is added on its own turn.
                let mut x = prev;
                while x < next {
                    if let Some(slot) = set.get_mut(usize::from(x)) {
                        *slot = true;
                    }
                    x = x.wrapping_add(1);
                }
            } else {
                if let Some(slot) = set.get_mut(usize::from(fc)) {
                    *slot = true;
                }
            }
            prev = fc;
        }
        let now = self.inp.read_in();
        let mut c = self.inp.get().ok_or(Stop::Input)?;
        let mut state = crate::wchar::MbstateT::new();
        let mut used = 0usize;
        loop {
            if set.get(usize::from(c)).copied().unwrap_or(false) == not_in {
                self.inp.unget(c);
                break;
            }
            if wide {
                // glibc's byte-path `%l[`: every byte is tested against the
                // set, and fed to `mbrtowc` one at a time; a character stored
                // when its last byte arrives.  (glibc stores whatever
                // `mbrtowc` left for an invalid sequence; this says EILSEQ.)
                if flags & SUPPRESS == 0 {
                    let mut wc: crate::wchar::WcharT = 0;
                    let byte = c;
                    // SAFETY: one byte; the state is ours.
                    let n = unsafe { crate::wchar::mbrtowc(&raw mut wc, &raw const byte, 1, &raw mut state) };
                    if n == usize::MAX {
                        errno::set_errno(errno::EILSEQ);
                        return Err(Stop::Encode);
                    }
                    if n != usize::MAX.wrapping_sub(1) {
                        // SAFETY: room for one more (grown below when `%m`).
                        unsafe {
                            out.cast::<crate::wchar::WcharT>().write_unaligned(wc);
                            out = out.add(size);
                        }
                        used = used.wrapping_add(1);
                    }
                }
            } else if flags & SUPPRESS == 0 {
                // SAFETY: as above.
                unsafe {
                    *out = c;
                    out = out.add(1);
                }
                used = used.wrapping_add(1);
            }
            if flags & SUPPRESS == 0 && flags & MALLOC != 0 && used == cap {
                out = self.grow(used, size, &mut cap).ok_or(Stop::NoMem)?;
            }
            if width > 0 {
                width = width.wrapping_sub(1);
                if width == 0 {
                    break;
                }
            }
            match self.inp.get() {
                Some(n) => c = n,
                None => break,
            }
        }
        if self.inp.read_in() == now {
            return Err(Stop::Conv);
        }
        if flags & SUPPRESS == 0 {
            if flags & MALLOC != 0 && used == cap {
                out = self.grow(used, size, &mut cap).ok_or(Stop::NoMem)?;
            }
            // SAFETY: room for the terminator.
            unsafe {
                if wide {
                    out.cast::<crate::wchar::WcharT>().write_unaligned(0);
                } else {
                    *out = 0;
                }
            }
            if flags & MALLOC != 0 {
                self.shrink(used.wrapping_add(1), size, cap);
            }
            self.strptr = core::ptr::null_mut();
            self.done = self.done.saturating_add(1);
        }
        Ok(())
    }
}

/// UTF-8's length for a lead byte (1 for anything else, so a stray byte is
/// matched on its own).
fn utf8_len(b: u8) -> usize {
    match b {
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf7 => 4,
        _ => 1,
    }
}

/// Store an integer through `p` at the width `flags` name: `long long` or
/// `long` (both 64 bits), `short`, `char`, or `int`.
fn store_int(p: u64, flags: u32, v: u64) {
    // SAFETY: the caller's pointer, of the type its modifier names; stored
    // unaligned because it came from C.
    #[allow(clippy::cast_possible_truncation)]
    unsafe {
        if flags & (LONGDBL | LONG) != 0 {
            (p as *mut u64).write_unaligned(v);
        } else if flags & SHORT != 0 {
            (p as *mut u16).write_unaligned(v as u16);
        } else if flags & CHAR != 0 {
            (p as *mut u8).write_unaligned(v as u8);
        } else {
            (p as *mut u32).write_unaligned(v as u32);
        }
    }
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
        let n = sscanf_va(b"hello world\0".as_ptr(), b"%s %s\0".as_ptr(), &args);
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
        let n = sscanf_va(b"hello 42\0".as_ptr(), b"%*s %d%n\0".as_ptr(), &args);
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
        let n = sscanf_va(b"ignored 42\0".as_ptr(), b"%*s %d\0".as_ptr(), &args);
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
        let n = sscanf_va(b"12 -34 56\0".as_ptr(), b"%zu %jd %td\0".as_ptr(), &args);
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
        assert_eq!(
            scan_hex("0x1.8p+1rest", b"%lf%s\0"),
            (2, 3.0, "rest".into())
        );
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
        assert_eq!(
            sscanf_va(b"1.25e34\0".as_ptr(), b"%6lf\0".as_ptr(), &args),
            1
        );
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
        let n = sscanf_va(b"hello world\0".as_ptr(), b"%[^ ]\0".as_ptr(), &args);
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
        let n = sscanf_va(b"Alice 30 95.5\0".as_ptr(), b"%s %d %f\0".as_ptr(), &args);
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
        let n = sscanf_va(b"10\t\t\n  20\0".as_ptr(), b"%d %d\0".as_ptr(), &args);
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
        let n = sscanf_va(b"0xDEADBEEFCAFE\0".as_ptr(), b"%lx\0".as_ptr(), &args);
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
            b"%99999999999999999999d\0".as_ptr(),
            &args,
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

    // -----------------------------------------------------------------------
    // The port of glibc's engine: through a stream, and glibc's answers
    // -----------------------------------------------------------------------

    /// A `va_list` over `ptrs`, as `sscanf_va` builds one, handed to `f`.
    fn with_va(ptrs: &[u64], f: impl FnOnce(*mut VaList) -> i32) -> i32 {
        let mut reg = [0u8; 176];
        let mut overflow = [0u8; 512];
        for (i, &v) in ptrs.iter().take(6).enumerate() {
            reg[i * 8..i * 8 + 8].copy_from_slice(&v.to_le_bytes());
        }
        for (i, &v) in ptrs.iter().skip(6).enumerate() {
            overflow[i * 8..i * 8 + 8].copy_from_slice(&v.to_le_bytes());
        }
        let mut va = VaList {
            gp_offset: 0,
            fp_offset: 48,
            overflow_arg_area: overflow.as_mut_ptr(),
            reg_save_area: reg.as_mut_ptr(),
        };
        f(&raw mut va)
    }

    fn fscanf_va(stream: *mut u8, fmt: &[u8], ptrs: &[u64]) -> i32 {
        with_va(ptrs, |ap| unsafe { vfscanf(stream, fmt.as_ptr(), ap) })
    }

    /// A read-only stream over `data`.
    fn reading(data: &mut [u8]) -> *mut u8 {
        let s = unsafe { crate::stdio_mem::fmemopen(data.as_mut_ptr().cast(), data.len(), b"r\0".as_ptr()) };
        assert!(!s.is_null());
        s
    }

    fn line(buf: &[u8]) -> &[u8] {
        let n = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        &buf[..n]
    }

    /// The bug the port was for: `12 34` on one line is two numbers, and a
    /// `%d` leaves the rest of the line in the stream.
    #[test]
    fn fscanf_reads_through_the_stream() {
        let mut data = *b"12 34\n56";
        let s = reading(&mut data);
        let (mut a, mut b) = (0i32, 0i32);
        assert_eq!(fscanf_va(s, b"%d\0", &[&raw mut a as u64]), 1);
        assert_eq!(fscanf_va(s, b"%d\0", &[&raw mut b as u64]), 1);
        assert_eq!((a, b), (12, 34));
        let mut rest = [0u8; 16];
        assert!(!crate::stdio::fgets(rest.as_mut_ptr(), 16, s).is_null());
        assert_eq!(line(&rest), b"\n", "what %d did not use is still there for fgets");
        assert_eq!(fscanf_va(s, b"%d\0", &[&raw mut a as u64]), 1);
        assert_eq!(a, 56);
        assert_eq!(fscanf_va(s, b"%d\0", &[&raw mut a as u64]), -1, "EOF");
        assert_eq!(crate::stdio::fclose(s), 0);
    }

    #[test]
    fn fscanf_spans_lines() {
        let mut data = *b"1\n\n  2\n";
        let s = reading(&mut data);
        let (mut a, mut b) = (0i32, 0i32);
        assert_eq!(fscanf_va(s, b"%d%d\0", &[&raw mut a as u64, &raw mut b as u64]), 2);
        assert_eq!((a, b), (1, 2));
        assert_eq!(crate::stdio::fclose(s), 0);
    }

    #[test]
    fn fscanf_leaves_the_character_it_stopped_at() {
        let mut data = *b"42abc\n";
        let s = reading(&mut data);
        let mut a = 0i32;
        assert_eq!(fscanf_va(s, b"%d\0", &[&raw mut a as u64]), 1);
        assert_eq!(crate::stdio::fgetc(s), i32::from(b'a'));
        assert_eq!(crate::stdio::fclose(s), 0);
    }

    /// A stream can hold a NUL; a string cannot.
    #[test]
    fn fscanf_reads_a_nul_as_a_character() {
        let mut data = *b"a\0b c";
        let s = reading(&mut data);
        let mut word = [0xffu8; 8];
        assert_eq!(fscanf_va(s, b"%s\0", &[word.as_mut_ptr() as u64]), 1);
        assert_eq!(&word[..4], b"a\0b\0");
        assert_eq!(crate::stdio::fclose(s), 0);
    }

    #[test]
    fn fscanf_refuses_a_write_only_stream_and_a_wide_one() {
        let mut p: *mut u8 = core::ptr::null_mut();
        let mut n = 0usize;
        let w = unsafe { crate::stdio_mem::open_memstream(&raw mut p, &raw mut n) };
        let mut a = 0i32;
        errno::set_errno(0);
        assert_eq!(fscanf_va(w, b"%d\0", &[&raw mut a as u64]), -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
        assert_eq!(crate::stdio::ferror(w), 1);
        assert_eq!(crate::stdio::fclose(w), 0);
        unsafe { crate::malloc::free(p) };
        let mut data = *b"5";
        let s = reading(&mut data);
        assert_eq!(crate::stdio::fwide(s, 1), 1);
        assert_eq!(fscanf_va(s, b"%d\0", &[&raw mut a as u64]), -1, "a wide stream");
        assert_eq!(crate::stdio::fclose(s), 0);
    }

    /// `%hd` stores a `short` and `%hhd` a `char` -- the old engine stored
    /// four bytes for both, over whatever followed.
    #[test]
    fn h_and_hh_store_only_their_width() {
        let mut shorts = [0x7777u16; 2];
        assert_eq!(sscanf_va(b"300\0".as_ptr(), b"%hd\0".as_ptr(), &[shorts.as_mut_ptr() as u64]), 1);
        assert_eq!(shorts, [300, 0x7777]);
        let mut bytes = [0x77u8; 4];
        assert_eq!(sscanf_va(b"300\0".as_ptr(), b"%hhu\0".as_ptr(), &[bytes.as_mut_ptr() as u64]), 1);
        assert_eq!(bytes, [44, 0x77, 0x77, 0x77], "300 as an unsigned char is 44");
        let mut n = [0x77u8; 2];
        assert_eq!(sscanf_va(b"abc\0".as_ptr(), b"abc%hhn\0".as_ptr(), &[n.as_mut_ptr() as u64]), 0);
        assert_eq!(n, [3, 0x77]);
    }

    /// Out-of-range integers saturate, as `strtol` makes them.
    #[test]
    fn integers_saturate_as_strtol_does() {
        let mut v: i64 = 0;
        errno::set_errno(0);
        assert_eq!(sscanf_va(b"99999999999999999999\0".as_ptr(), b"%ld\0".as_ptr(), &[&raw mut v as u64]), 1);
        assert_eq!(v, i64::MAX);
        assert_eq!(errno::get_errno(), errno::ERANGE);
    }

    /// `%m` allocates; an `EOF` before the first assignment frees what it
    /// allocated and leaves NULL, as glibc does.
    #[test]
    fn m_allocates_and_is_freed_on_eof() {
        let mut a: *mut u8 = core::ptr::null_mut();
        let mut b: *mut u8 = core::ptr::null_mut();
        assert_eq!(
            sscanf_va(b"hello world\0".as_ptr(), b"%ms %ms\0".as_ptr(), &[&raw mut a as u64, &raw mut b as u64]),
            2
        );
        assert_eq!(unsafe { core::ffi::CStr::from_ptr(a.cast()) }.to_bytes(), b"hello");
        assert_eq!(unsafe { core::ffi::CStr::from_ptr(b.cast()) }.to_bytes(), b"world");
        unsafe {
            crate::malloc::free(a);
            crate::malloc::free(b);
        }
        let long = [b'q'; 500];
        let mut input = long.to_vec();
        input.push(0);
        let mut c: *mut u8 = core::ptr::null_mut();
        assert_eq!(sscanf_va(input.as_ptr(), b"%ms\0".as_ptr(), &[&raw mut c as u64]), 1);
        assert_eq!(unsafe { core::ffi::CStr::from_ptr(c.cast()) }.to_bytes().len(), 500, "grown past 100");
        unsafe { crate::malloc::free(c) };
        let mut d: *mut u8 = 7 as *mut u8;
        assert_eq!(sscanf_va(b"\0".as_ptr(), b"%ms\0".as_ptr(), &[&raw mut d as u64]), -1);
        assert!(d.is_null(), "freed and cleared");
        let mut e: *mut u8 = core::ptr::null_mut();
        assert_eq!(sscanf_va(b"xyz\0".as_ptr(), b"%3mc\0".as_ptr(), &[&raw mut e as u64]), 1);
        assert_eq!(unsafe { core::slice::from_raw_parts(e, 3) }, b"xyz");
        unsafe { crate::malloc::free(e) };
    }

    #[test]
    fn p_reads_a_pointer_and_nil() {
        let mut p: usize = 99;
        assert_eq!(sscanf_va(b"0x1234\0".as_ptr(), b"%p\0".as_ptr(), &[&raw mut p as u64]), 1);
        assert_eq!(p, 0x1234);
        assert_eq!(sscanf_va(b"(nil)\0".as_ptr(), b"%p\0".as_ptr(), &[&raw mut p as u64]), 1);
        assert_eq!(p, 0);
        let mut word = [0u8; 8];
        assert_eq!(
            sscanf_va(b"(nix)\0".as_ptr(), b"%p%s\0".as_ptr(), &[&raw mut p as u64, word.as_mut_ptr() as u64]),
            0,
            "not (nil): a matching failure"
        );
    }

    #[test]
    fn positional_arguments() {
        let (mut a, mut b) = (0i32, 0i32);
        assert_eq!(
            sscanf_va(b"1 2\0".as_ptr(), b"%2$d %1$d\0".as_ptr(), &[&raw mut a as u64, &raw mut b as u64]),
            2
        );
        assert_eq!((a, b), (2, 1));
    }

    #[test]
    fn b_reads_binary() {
        let mut v = 0u32;
        assert_eq!(sscanf_va(b"101\0".as_ptr(), b"%b\0".as_ptr(), &[&raw mut v as u64]), 1);
        assert_eq!(v, 5);
        assert_eq!(sscanf_va(b"0b110\0".as_ptr(), b"%b\0".as_ptr(), &[&raw mut v as u64]), 1);
        assert_eq!(v, 6);
        let mut i = 0i32;
        let mut word = [0u8; 8];
        assert_eq!(
            sscanf_va(b"0b11\0".as_ptr(), b"%i%s\0".as_ptr(), &[&raw mut i as u64, word.as_mut_ptr() as u64]),
            2
        );
        assert_eq!((i, line(&word)), (0, &b"b11"[..]), "%i takes no 0b outside C23 mode");
    }

    #[test]
    fn wide_conversions_store_wchar_t() {
        let mut w = [0x5555i32; 8];
        assert_eq!(sscanf_va("é€x y\0".as_ptr(), b"%ls\0".as_ptr(), &[w.as_mut_ptr() as u64]), 1);
        assert_eq!(&w[..4], &[0xe9, 0x20ac, i32::from(b'x'), 0]);
        let mut c = [0i32; 2];
        assert_eq!(sscanf_va("€\0".as_ptr(), b"%lc\0".as_ptr(), &[c.as_mut_ptr() as u64]), 1);
        assert_eq!(c[0], 0x20ac);
        let mut s = [0i32; 8];
        assert_eq!(sscanf_va("éé-\0".as_ptr(), b"%l[^-]\0".as_ptr(), &[s.as_mut_ptr() as u64]), 1);
        assert_eq!(&s[..3], &[0xe9, 0xe9, 0]);
        errno::set_errno(0);
        assert_eq!(sscanf_va(b"\xff\0".as_ptr(), b"%lc\0".as_ptr(), &[c.as_mut_ptr() as u64]), 0);
        assert_eq!(errno::get_errno(), errno::EILSEQ);
    }

    /// Where glibc's one-character pushback consumes what a look-ahead
    /// engine would have left.
    #[test]
    fn glibc_consumes_what_it_looked_at() {
        let mut x = 7u32;
        let mut word = [0u8; 16];
        let two = |x: *mut u32, w: *mut u8| [x as u64, w as u64];
        assert_eq!(sscanf_va(b"0xZ\0".as_ptr(), b"%x%s\0".as_ptr(), &two(&raw mut x, word.as_mut_ptr())), 2);
        assert_eq!((x, line(&word)), (0, &b"Z"[..]), "%x consumes 0x before finding no digit");
        let (n, v, rest) = scan_hex("1ex", b"%lf%s\0");
        assert_eq!((n, v, rest.as_str()), (2, 1.0, "x"), "the e is consumed");
        let (n, v, rest) = scan_hex("nan(1)", b"%lf%s\0");
        assert!(v.is_nan());
        assert_eq!((n, rest.as_str()), (2, "(1)"), "glibc's scanf reads no nan payload");
        let (n, _, _) = scan_hex("infinx", b"%lf%s\0");
        assert_eq!(n, 0, "infin is consumed and is no number");
        let (n, v, rest) = scan_hex("infx", b"%lf%s\0");
        assert_eq!((n, v, rest.as_str()), (2, f64::INFINITY, "x"));
    }

    #[test]
    fn scansets_are_glibcs() {
        let mut word = [0u8; 16];
        assert_eq!(sscanf_va(b"z-a!\0".as_ptr(), b"%[z-a]\0".as_ptr(), &[word.as_mut_ptr() as u64]), 1);
        assert_eq!(line(&word), b"z-a", "a reversed range is three members");
        assert_eq!(sscanf_va(b"b\0".as_ptr(), b"%[z-a]\0".as_ptr(), &[word.as_mut_ptr() as u64]), 0);
        assert_eq!(sscanf_va(b"abc\0".as_ptr(), b"%[abc\0".as_ptr(), &[word.as_mut_ptr() as u64]), 0, "no closing ]");
        // A leading `]` is a member -- and then `]-^` is a range from `]` to
        // `^`, which has no `-` in it, as glibc reads it.
        assert_eq!(sscanf_va(b"]-^x\0".as_ptr(), b"%[]-^]\0".as_ptr(), &[word.as_mut_ptr() as u64]), 1);
        assert_eq!(line(&word), b"]");
        // A leading `-` is a member, and so is a trailing one.
        assert_eq!(sscanf_va(b"-a-b\0".as_ptr(), b"%[-a]\0".as_ptr(), &[word.as_mut_ptr() as u64]), 1);
        assert_eq!(line(&word), b"-a-");
        assert_eq!(sscanf_va(b"a-b\0".as_ptr(), b"%[a-]\0".as_ptr(), &[word.as_mut_ptr() as u64]), 1);
        assert_eq!(line(&word), b"a-");
    }

    /// A NULL destination is a conversion error, as glibc's `STRING_ARG`
    /// makes it for strings (and not a write through NULL).
    #[test]
    fn a_null_destination_stops_the_scan() {
        let mut b = 0i32;
        assert_eq!(sscanf_va(b"1 2\0".as_ptr(), b"%d %d\0".as_ptr(), &[0, &raw mut b as u64]), 0);
        assert_eq!(b, 0);
        assert_eq!(sscanf_va(b"ab\0".as_ptr(), b"%s\0".as_ptr(), &[0]), 0);
    }
}
