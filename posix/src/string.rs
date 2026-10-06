//! C string functions required by the C runtime.
//!
//! These are not strictly POSIX but are required by virtually every
//! C program.  The memory primitives and `strlen` are the fast ones a libc
//! needs (SSE2, `rep movsb`; see "The engines" below), and the functions
//! that scan, copy, compare and search C strings are built on them or on
//! the same 16-byte blocks ("The scanners"): `strstr`, `memmem` and
//! `strcasestr` in linear time.  The case-insensitive comparisons, the
//! tokenizers and the wide-character functions (`wchar.rs`, but for
//! `wcsstr`, which shares `strstr`'s search) are still plain loops.
//!
//! Includes: `memcpy`, `memmove`, `memset`, `memcmp`, `memchr`,
//! `memrchr`, `memccpy`, `mempcpy`, `memmem`, `rawmemchr`,
//! `strlen`, `strnlen`, `strcmp`, `strncmp`,
//! `strcpy`, `strncpy`, `stpcpy`, `stpncpy`, `strchr`, `strrchr`,
//! `strcat`, `strncat`, `strstr`, `strspn`, `strcspn`, `strpbrk`,
//! `strtok`, `strtok_r`, `strsep`, `strerror`, `strerror_r`,
//! `strdup`, `strndup`, `bcopy`, `bzero`, `strcasecmp`, `strncasecmp`,
//! `strcoll`, `strxfrm`, `strverscmp`, `strlcpy`, `strlcat`,
//! `sys_errlist`, `sys_nerr`
//!
//! Exported as `extern "C"` with standard names so the linker finds
//! them when C code calls `memcpy`, `memset`, `strlen`, etc.
//!
//! # Why some functions below sit in a one-function `mod gnu_*` block
//!
//! Static linking extracts archive members **whole**: a member is pulled in
//! if it defines any still-undefined symbol, and then *every* symbol it
//! defines becomes a definition in the output. So which functions share an
//! object file is part of what programs can link against `libc.a` — it is an
//! ABI property, not a layout detail. glibc puts (near enough) one function in
//! one object for exactly this reason.
//!
//! That matters here because gnulib ships *replacements* for a specific set of
//! these functions — `strndup`, `strverscmp`, `stpcpy`, `stpncpy`, `mempcpy`,
//! `strchrnul`, `memrchr`, `rawmemchr`, `strcasestr`, `strnlen` — and every GNU
//! package that vendors gnulib (coreutils, grep, sed, tar, gawk, gcc, binutils,
//! make) may therefore define them itself. Normally that is harmless: the
//! program's own copy resolves the reference and libc's member is never
//! extracted. But if such a function shares its object with `memcpy` or
//! `strlen`, the member is extracted for *those*, and its copy collides with
//! the program's — a duplicate-symbol error the program cannot avoid, because
//! no link order or `--start-group` can decline half a member.
//!
//! That is not hypothetical: it is exactly how GNU make failed to link, with
//! 11 duplicate symbols and zero missing ones (`design-decisions.md` §339).
//!
//! rustc's codegen-unit partitioner works at **module** granularity, and
//! `toolchain/build-sysroot.ps1` builds with `-C codegen-units=4096` so that it
//! never merges. Wrapping one function in its own inline `mod` therefore gives
//! it its own archive member. The `pub use` beside each block keeps the
//! in-crate path (`string::strndup`) unchanged, so this costs call sites
//! nothing.
//!
//! `scripts/check-libc-shape.py` asserts the resulting archive shape. If you
//! add a function here that gnulib also replaces, give it the same treatment
//! and add it to that script's `REPLACEABLE` set.

use core::arch::x86_64::__m128i;

use crate::types::SizeT;

// ---------------------------------------------------------------------------
// The engines behind memcpy, memmove, memset, memcmp, memchr and strlen
// ---------------------------------------------------------------------------
//
// These are the hottest functions in the system: every `Vec` and `String`
// copy in the Rust userland reaches this `memcpy` (the sysroot builds
// `compiler_builtins` without its `mem` feature, so `libc.a` supplies the
// symbol), and every C string this `strlen`.  Until 2026-10-06 each was a
// byte loop, which LLVM left a byte loop -- `memcpy` compiled to a `movb`
// pair, `strlen` to a `cmpb` -- at about 1.5 GB/s.  `posix/benches/mem.rs`
// measures them.
//
// - **Small sizes** (under [`SMALL`] bytes): two loads and two stores of the
//   widest power of two not wider than the size, one at each end, the two
//   overlapping in the middle -- glibc's small-size paths.  All loads come
//   before any store, so it is a correct `memmove` as well.
// - **Medium sizes**: 16-byte SSE2 loads and stores.  SSE2 is in every
//   x86-64 and in the sysroot's target spec.
// - **Large forward copies and fills** (from [`REP_THRESHOLD`] bytes):
//   `rep movsb` / `rep stosb`, which every x86-64 since Ivy Bridge runs at
//   memory bandwidth (ERMS); glibc switches at the same 2 KiB for 16-byte
//   vectors.
//
// THE LOOPS ARE ASSEMBLY, NOT RUST.  LLVM's loop-idiom pass turns a copy or
// fill loop into a call to `memcpy`, `memmove` or `memset` -- inside those
// very functions, a call to themselves.  It refrains only inside a function
// of exactly that name, which a helper is not.  `memchr`, `memcmp` and
// `strlen` copy nothing, and are Rust over SSE2 intrinsics where they read
// within their bounds; `strlen`, which has no bound, is assembly too (below).

/// Below this many bytes, a copy or fill is a few overlapping loads and
/// stores; from it up, a loop.
const SMALL: usize = 32;

/// From this many bytes, a forward copy or a fill is `rep movsb`/`rep stosb`.
const REP_THRESHOLD: usize = 2048;

/// Copy `n < SMALL` bytes from `src` to `dst`, which may overlap either way:
/// every load is done before any store.
///
/// # Safety
///
/// `src` readable and `dst` writable for `n` bytes.
#[inline(always)]
unsafe fn copy_small(dst: *mut u8, src: *const u8, n: usize) {
    // SAFETY: each access lies within the first `n` bytes of `src` or `dst`,
    // which the caller vouches for, and is an unaligned access of a plain
    // integer.
    unsafe {
        if n >= 16 {
            let head = src.cast::<u128>().read_unaligned();
            let tail = src.add(n.wrapping_sub(16)).cast::<u128>().read_unaligned();
            dst.cast::<u128>().write_unaligned(head);
            dst.add(n.wrapping_sub(16))
                .cast::<u128>()
                .write_unaligned(tail);
        } else if n >= 8 {
            let head = src.cast::<u64>().read_unaligned();
            let tail = src.add(n.wrapping_sub(8)).cast::<u64>().read_unaligned();
            dst.cast::<u64>().write_unaligned(head);
            dst.add(n.wrapping_sub(8))
                .cast::<u64>()
                .write_unaligned(tail);
        } else if n >= 4 {
            let head = src.cast::<u32>().read_unaligned();
            let tail = src.add(n.wrapping_sub(4)).cast::<u32>().read_unaligned();
            dst.cast::<u32>().write_unaligned(head);
            dst.add(n.wrapping_sub(4))
                .cast::<u32>()
                .write_unaligned(tail);
        } else if n >= 2 {
            let head = src.cast::<u16>().read_unaligned();
            let tail = src.add(n.wrapping_sub(2)).cast::<u16>().read_unaligned();
            dst.cast::<u16>().write_unaligned(head);
            dst.add(n.wrapping_sub(2))
                .cast::<u16>()
                .write_unaligned(tail);
        } else if n == 1 {
            dst.write(src.read());
        }
    }
}

/// Copy `n >= SMALL` bytes forward: correct when the regions do not overlap
/// or `dst` is below `src`.
///
/// # Safety
///
/// `src` readable and `dst` writable for `n` bytes, `n >= SMALL`.
#[inline(always)]
unsafe fn copy_forward(dst: *mut u8, src: *const u8, n: usize) {
    if n >= REP_THRESHOLD {
        // SAFETY: the caller's bounds; `rep movsb` copies `rcx` bytes from
        // `rsi` to `rdi` upwards (the ABI keeps the direction flag clear),
        // with the architecture's byte-at-a-time semantics -- so a
        // destination below an overlapping source is copied correctly.
        unsafe {
            core::arch::asm!(
                "rep movsb",
                inout("rcx") n => _,
                inout("rdi") dst => _,
                inout("rsi") src => _,
                options(nostack, preserves_flags),
            );
        }
        return;
    }
    // SAFETY: the caller's bounds.  The last 16 bytes are loaded before the
    // loops and stored after them: they cover the remainder, and they are
    // read before the loops' stores can reach them when `dst` is below `src`.
    // Blocks of 64 bytes, then of 16, go upwards from the start, each read
    // whole before any of it is written; with `dst` below `src` a block's
    // stores reach only source bytes below its end, already read.
    unsafe {
        core::arch::asm!(
            "movdqu {tail}, xmmword ptr [{src} + {n16}]",
            "xor {off:e}, {off:e}",
            "2:",
            "lea {end}, [{off} + 64]",
            "cmp {end}, {n16}",
            "ja 3f",
            "movdqu {x0}, xmmword ptr [{src} + {off}]",
            "movdqu {x1}, xmmword ptr [{src} + {off} + 16]",
            "movdqu {x2}, xmmword ptr [{src} + {off} + 32]",
            "movdqu {x3}, xmmword ptr [{src} + {off} + 48]",
            "movdqu xmmword ptr [{dst} + {off}], {x0}",
            "movdqu xmmword ptr [{dst} + {off} + 16], {x1}",
            "movdqu xmmword ptr [{dst} + {off} + 32], {x2}",
            "movdqu xmmword ptr [{dst} + {off} + 48], {x3}",
            "add {off}, 64",
            "jmp 2b",
            "3:",
            "cmp {off}, {n16}",
            "jae 5f",
            "4:",
            "movdqu {x0}, xmmword ptr [{src} + {off}]",
            "movdqu xmmword ptr [{dst} + {off}], {x0}",
            "add {off}, 16",
            "cmp {off}, {n16}",
            "jb 4b",
            "5:",
            "movdqu xmmword ptr [{dst} + {n16}], {tail}",
            src = in(reg) src,
            dst = in(reg) dst,
            n16 = in(reg) n.wrapping_sub(16),
            off = out(reg) _,
            end = out(reg) _,
            x0 = out(xmm_reg) _,
            x1 = out(xmm_reg) _,
            x2 = out(xmm_reg) _,
            x3 = out(xmm_reg) _,
            tail = out(xmm_reg) _,
            options(nostack),
        );
    }
}

/// Copy `n >= SMALL` bytes backward: for `dst` above an overlapping `src`.
///
/// # Safety
///
/// `src` readable and `dst` writable for `n` bytes, `n >= SMALL`.
#[inline(always)]
unsafe fn copy_backward(dst: *mut u8, src: *const u8, n: usize) {
    // SAFETY: the caller's bounds.  The first 16 bytes are loaded before the
    // loops and stored after them: they cover the remainder, read before any
    // store.  Blocks of 64 bytes, then of 16, go from the top down, each read
    // whole before any of it is written; with `dst` above `src` a block's
    // stores reach only source bytes at or above its start, already read.
    // `rep movsb` backwards (`std`) is microcoded a byte at a time on most
    // cores, so it is not used.
    unsafe {
        core::arch::asm!(
            "movdqu {head}, xmmword ptr [{src}]",
            "mov {off}, {n}",
            "2:",
            "cmp {off}, 80",
            "jb 3f",
            "sub {off}, 64",
            "movdqu {x0}, xmmword ptr [{src} + {off}]",
            "movdqu {x1}, xmmword ptr [{src} + {off} + 16]",
            "movdqu {x2}, xmmword ptr [{src} + {off} + 32]",
            "movdqu {x3}, xmmword ptr [{src} + {off} + 48]",
            "movdqu xmmword ptr [{dst} + {off}], {x0}",
            "movdqu xmmword ptr [{dst} + {off} + 16], {x1}",
            "movdqu xmmword ptr [{dst} + {off} + 32], {x2}",
            "movdqu xmmword ptr [{dst} + {off} + 48], {x3}",
            "jmp 2b",
            "3:",
            "cmp {off}, 16",
            "jbe 5f",
            "4:",
            "sub {off}, 16",
            "movdqu {x0}, xmmword ptr [{src} + {off}]",
            "movdqu xmmword ptr [{dst} + {off}], {x0}",
            "cmp {off}, 16",
            "ja 4b",
            "5:",
            "movdqu xmmword ptr [{dst}], {head}",
            src = in(reg) src,
            dst = in(reg) dst,
            n = in(reg) n,
            off = out(reg) _,
            x0 = out(xmm_reg) _,
            x1 = out(xmm_reg) _,
            x2 = out(xmm_reg) _,
            x3 = out(xmm_reg) _,
            head = out(xmm_reg) _,
            options(nostack),
        );
    }
}

/// `byte` in each of a `u64`'s eight bytes.
#[inline(always)]
fn broadcast(byte: u8) -> u64 {
    u64::from(byte).wrapping_mul(0x0101_0101_0101_0101)
}

/// Fill `n < SMALL` bytes at `dst` with `byte`, two overlapping stores at
/// most.
///
/// # Safety
///
/// `dst` writable for `n` bytes.
#[inline(always)]
unsafe fn fill_small(dst: *mut u8, byte: u8, n: usize) {
    let word = broadcast(byte);
    // SAFETY: each store lies within the first `n` bytes of `dst`, which the
    // caller vouches for; unaligned stores of plain integers.
    unsafe {
        if n >= 16 {
            let wide = u128::from(word) | (u128::from(word) << 64);
            dst.cast::<u128>().write_unaligned(wide);
            dst.add(n.wrapping_sub(16))
                .cast::<u128>()
                .write_unaligned(wide);
        } else if n >= 8 {
            dst.cast::<u64>().write_unaligned(word);
            dst.add(n.wrapping_sub(8))
                .cast::<u64>()
                .write_unaligned(word);
        } else if n >= 4 {
            #[allow(clippy::cast_possible_truncation)] // the low half of a repeated byte
            let w = word as u32;
            dst.cast::<u32>().write_unaligned(w);
            dst.add(n.wrapping_sub(4)).cast::<u32>().write_unaligned(w);
        } else if n >= 2 {
            #[allow(clippy::cast_possible_truncation)] // the low quarter, likewise
            let w = word as u16;
            dst.cast::<u16>().write_unaligned(w);
            dst.add(n.wrapping_sub(2)).cast::<u16>().write_unaligned(w);
        } else if n == 1 {
            dst.write(byte);
        }
    }
}

/// Fill `n >= SMALL` bytes at `dst` with `byte`.
///
/// # Safety
///
/// `dst` writable for `n` bytes, `n >= SMALL`.
#[inline(always)]
unsafe fn fill_large(dst: *mut u8, byte: u8, n: usize) {
    if n >= REP_THRESHOLD {
        // SAFETY: the caller's bound; `rep stosb` stores `al` into `rcx`
        // bytes upwards from `rdi`.
        unsafe {
            core::arch::asm!(
                "rep stosb",
                inout("rcx") n => _,
                inout("rdi") dst => _,
                in("al") byte,
                options(nostack, preserves_flags),
            );
        }
        return;
    }
    // SAFETY: the caller's bound: the last 16 bytes, then blocks of 64 and
    // of 16 bytes from the start until the last 16 are reached.
    unsafe {
        core::arch::asm!(
            "movq {v}, {pattern}",
            "punpcklqdq {v}, {v}",
            "movdqu xmmword ptr [{dst} + {n16}], {v}",
            "xor {off:e}, {off:e}",
            "2:",
            "lea {end}, [{off} + 64]",
            "cmp {end}, {n16}",
            "ja 3f",
            "movdqu xmmword ptr [{dst} + {off}], {v}",
            "movdqu xmmword ptr [{dst} + {off} + 16], {v}",
            "movdqu xmmword ptr [{dst} + {off} + 32], {v}",
            "movdqu xmmword ptr [{dst} + {off} + 48], {v}",
            "add {off}, 64",
            "jmp 2b",
            "3:",
            "cmp {off}, {n16}",
            "jae 5f",
            "4:",
            "movdqu xmmword ptr [{dst} + {off}], {v}",
            "add {off}, 16",
            "cmp {off}, {n16}",
            "jb 4b",
            "5:",
            dst = in(reg) dst,
            n16 = in(reg) n.wrapping_sub(16),
            pattern = in(reg) broadcast(byte),
            off = out(reg) _,
            end = out(reg) _,
            v = out(xmm_reg) _,
            options(nostack),
        );
    }
}

/// Copy `n` bytes from `src` to `dest`.  Regions must not overlap.
///
/// Returns `dest`.  See the engines above for how.
///
/// # Safety
///
/// `dest` and `src` must be valid for `n` bytes and must not overlap.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn memcpy(dest: *mut u8, src: *const u8, n: SizeT) -> *mut u8 {
    // SAFETY: the caller's bounds, which the engines need.
    unsafe {
        if n < SMALL {
            copy_small(dest, src, n);
        } else {
            copy_forward(dest, src, n);
        }
    }
    dest
}

/// Copy `n` bytes from `src` to `dest`.  Regions may overlap.
///
/// Returns `dest`.  A destination below the source, or past its end, is
/// copied forward; one inside it, backward.
///
/// # Safety
///
/// `dest` and `src` must be valid for `n` bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn memmove(dest: *mut u8, src: *const u8, n: SizeT) -> *mut u8 {
    // SAFETY: the caller's bounds, which the engines need.  Forward is right
    // when `dest - src` (wrapping) is at least `n`: `dest` below `src`, or at
    // or past its end.  Otherwise `dest` starts inside the source.
    unsafe {
        if n < SMALL {
            copy_small(dest, src, n);
        } else if (dest as usize).wrapping_sub(src as usize) >= n {
            copy_forward(dest, src, n);
        } else {
            copy_backward(dest, src, n);
        }
    }
    dest
}

/// Fill `n` bytes of `dest` with byte value `c`.
///
/// Returns `dest`.
///
/// # Safety
///
/// `dest` must be valid for `n` bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn memset(dest: *mut u8, c: i32, n: SizeT) -> *mut u8 {
    // C's `(unsigned char)c`.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let byte = c as u8;
    // SAFETY: the caller's bound, which the engines need.
    unsafe {
        if n < SMALL {
            fill_small(dest, byte, n);
        } else {
            fill_large(dest, byte, n);
        }
    }
    dest
}

/// The difference of the first bytes that differ in a 16-byte block whose
/// equality mask (`pmovmskb` of `pcmpeqb`, a bit set for each equal byte)
/// is not all ones, as `memcmp` answers it: `a`'s byte less `b`'s.
///
/// # Safety
///
/// `a` and `b` readable for 16 bytes.
#[inline(always)]
unsafe fn first_difference(a: *const u8, b: *const u8, equal: u32) -> i32 {
    let at = (!equal).trailing_zeros() as usize;
    // SAFETY: `!equal` has a bit below 16 set (`equal` is not 0xFFFF), so
    // `at < 16`, within the caller's 16 bytes.
    unsafe { i32::from(a.add(at).read()).wrapping_sub(i32::from(b.add(at).read())) }
}

/// Compare `n` bytes of `s1` and `s2`.
///
/// Returns 0 if equal, else the difference of the first differing bytes as
/// unsigned values (`s1`'s less `s2`'s), as glibc's does: negative if `s1`
/// sorts first.  Sixteen bytes a step (SSE2), never reading past `n`.
///
/// # Safety
///
/// `s1` and `s2` must be valid for `n` bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn memcmp(s1: *const u8, s2: *const u8, n: SizeT) -> i32 {
    use core::arch::x86_64::{__m128i, _mm_cmpeq_epi8, _mm_loadu_si128, _mm_movemask_epi8};
    /// The equality mask of the 16 bytes at `a` and `b`.
    ///
    /// # Safety
    ///
    /// `a` and `b` readable for 16 bytes.
    #[inline(always)]
    #[allow(clippy::cast_sign_loss)] // pmovmskb's 16 bits, never negative
    unsafe fn equal16(a: *const u8, b: *const u8) -> u32 {
        // SAFETY: the caller's 16 bytes; unaligned loads.
        unsafe {
            let x = _mm_loadu_si128(a.cast::<__m128i>());
            let y = _mm_loadu_si128(b.cast::<__m128i>());
            _mm_movemask_epi8(_mm_cmpeq_epi8(x, y)) as u32
        }
    }
    // SAFETY: every read below lies within the first `n` bytes of both
    // strings, which the caller vouches for.
    unsafe {
        if n >= 16 {
            let mut i = 0usize;
            while n.wrapping_sub(i) >= 16 {
                let (a, b) = (s1.add(i), s2.add(i));
                let equal = equal16(a, b);
                if equal != 0xFFFF {
                    return first_difference(a, b, equal);
                }
                i = i.wrapping_add(16);
            }
            if i < n {
                // The last 16 bytes, overlapping some already found equal,
                // which cannot hold the first difference.
                let at = n.wrapping_sub(16);
                let (a, b) = (s1.add(at), s2.add(at));
                let equal = equal16(a, b);
                if equal != 0xFFFF {
                    return first_difference(a, b, equal);
                }
            }
            return 0;
        }
        let mut i = 0usize;
        while i < n {
            let (a, b) = (s1.add(i).read(), s2.add(i).read());
            if a != b {
                return i32::from(a).wrapping_sub(i32::from(b));
            }
            i = i.wrapping_add(1);
        }
    }
    0
}

/// `byte` in all sixteen lanes of a vector.
#[inline(always)]
fn splat(byte: u8) -> __m128i {
    // SAFETY: an `__m128i` is sixteen bytes, and any bits are a valid one.
    unsafe { core::mem::transmute::<[u8; 16], __m128i>([byte; 16]) }
}

/// The bytes of the 16 at `p` that equal `needle`'s: bit `i` set for byte
/// `i`.
///
/// # Safety
///
/// `p` readable for 16 bytes.
#[inline(always)]
unsafe fn matches16(p: *const u8, needle: __m128i) -> u32 {
    use core::arch::x86_64::{_mm_cmpeq_epi8, _mm_loadu_si128, _mm_movemask_epi8};
    // SAFETY: the caller's 16 bytes; an unaligned load.
    unsafe {
        let chunk = _mm_loadu_si128(p.cast::<__m128i>());
        // pmovmskb's 16 bits: never negative.
        _mm_movemask_epi8(_mm_cmpeq_epi8(chunk, needle)) as u32
    }
}

/// Find the first occurrence of byte `c` in the first `n` bytes of `s`.
///
/// Returns a pointer to the byte, or NULL if not found.  Sixteen bytes a
/// step (SSE2), never reading past `n`.
///
/// # Safety
///
/// `s` must be valid for `n` bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn memchr(s: *const u8, c: i32, n: SizeT) -> *const u8 {
    // C's `(unsigned char)c`.
    let byte = c as u8;
    // SAFETY: every read below lies within the first `n` bytes of `s`, which
    // the caller vouches for.
    unsafe {
        if n >= 16 {
            let needle = splat(byte);
            let mut i = 0usize;
            while n.wrapping_sub(i) >= 16 {
                let mask = matches16(s.add(i), needle);
                if mask != 0 {
                    return s.add(i.wrapping_add(mask.trailing_zeros() as usize));
                }
                i = i.wrapping_add(16);
            }
            if i < n {
                // The last 16 bytes, overlapping some already searched.
                let at = n.wrapping_sub(16);
                let mask = matches16(s.add(at), needle);
                if mask != 0 {
                    return s.add(at.wrapping_add(mask.trailing_zeros() as usize));
                }
            }
            return core::ptr::null();
        }
        let mut i = 0usize;
        while i < n {
            if s.add(i).read() == byte {
                return s.add(i);
            }
            i = i.wrapping_add(1);
        }
    }
    core::ptr::null()
}

/// Compute the length of a C string (excluding null terminator).
///
/// Sixteen bytes a step: aligned 16-byte loads, compared with zero (SSE2).
/// An aligned load never crosses a page boundary, so though it may read
/// bytes past the terminator, they are in a page the string occupies and the
/// read cannot fault -- how glibc's and every other fast `strlen` read.  It
/// is assembly, rather than Rust over intrinsics, because those bytes are
/// past the end of the object as Rust sees it.
///
/// # Safety
///
/// `s` must be a valid null-terminated string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn strlen(s: *const u8) -> SizeT {
    let len: usize;
    // SAFETY: the caller's string is readable to its terminator.  Every load
    // is 16-byte aligned and starts at or below a byte of the string, so it
    // lies within a page the string occupies.  The first block's bytes below
    // `s` are shifted out of the mask before it is looked at.
    unsafe {
        core::arch::asm!(
            "mov {base}, {s}",
            "and {base}, -16",
            "pxor {zero}, {zero}",
            "movdqa {chunk}, xmmword ptr [{base}]",
            "pcmpeqb {chunk}, {zero}",
            "pmovmskb {mask:e}, {chunk}",
            "mov ecx, {s:e}",
            "and ecx, 15",
            "shr {mask:e}, cl",
            "test {mask:e}, {mask:e}",
            "jnz 3f",
            "2:",
            "add {base}, 16",
            "movdqa {chunk}, xmmword ptr [{base}]",
            "pcmpeqb {chunk}, {zero}",
            "pmovmskb {mask:e}, {chunk}",
            "test {mask:e}, {mask:e}",
            "jz 2b",
            "bsf {mask:e}, {mask:e}",
            "sub {base}, {s}",
            "add {base}, {mask}",
            "mov {len}, {base}",
            "jmp 4f",
            "3:",
            "bsf {mask:e}, {mask:e}",
            "mov {len}, {mask}",
            "4:",
            s = in(reg) s,
            base = out(reg) _,
            mask = out(reg) _,
            len = out(reg) len,
            zero = out(xmm_reg) _,
            chunk = out(xmm_reg) _,
            out("ecx") _,
            options(nostack, readonly),
        );
    }
    len
}

// ---------------------------------------------------------------------------
// The scanners behind the rest of the string functions
// ---------------------------------------------------------------------------
//
// `strlen`'s method, for every function that reads a C string: aligned
// 16-byte blocks, which never cross a page, so a block holding one byte of
// the string is readable whole even where the rest of it lies past the
// terminator.  Those reads are assembly for the reason `strlen`'s are: as
// Rust sees them they leave the object.  `strcmp` cannot align two strings
// at once, so it reads unaligned runs of sixteen, and a byte at a time where
// a run would cross a page.  Searches given a length (`memrchr`) stay inside
// it and use the intrinsics, as `memchr` does.
//
// The copying functions find the length and then `memcpy`: two passes at
// 10-20 GB/s beat one at 1.5.  `strspn`'s family looks each byte up in a
// 256-bit set instead of walking the set's string for it, and `strstr`,
// `memmem` and `strcasestr` are the Two-Way search ([`TwoWay`]), linear in
// their inputs where the loops they replace took the product of the
// lengths.
//
// A helper that a `mod gnu_*` member shares with another member is
// `#[inline(always)]`: inlined, it is defined in no member, so neither refers
// to the other (`scripts/check-libc-shape.py`, CHECK 5).

/// The smallest page an x86-64 maps.  Sixteen bytes that cross no multiple
/// of it cross no page, whatever the page size: SlateOS's 16 KiB pages are
/// multiples of it.
const PAGE: usize = 4096;

/// A page offset's bits.
const IN_PAGE: usize = PAGE - 1;

/// The last offset in a page at which sixteen bytes still fit.
const LAST_IN_PAGE: usize = PAGE - 16;

/// Whether the sixteen bytes from `p` lie within one page.
#[inline(always)]
fn within_page(p: *const u8) -> bool {
    p as usize & IN_PAGE <= LAST_IN_PAGE
}

/// The bytes of the 16-byte-aligned block at `block` that are zero, and
/// those equal to `needle`'s: bit `i` of each mask for byte `i`.
///
/// # Safety
///
/// `block` is 16-byte aligned and holds a readable byte, which makes all
/// sixteen readable: an aligned block lies within one page.
#[inline(always)]
unsafe fn block_masks(block: *const u8, needle: __m128i) -> (u32, u32) {
    let zeros: u32;
    let hits: u32;
    // SAFETY: the caller's readable block, aligned as `movdqa` needs.
    unsafe {
        core::arch::asm!(
            "movdqa {chunk}, xmmword ptr [{block}]",
            "pxor {zero}, {zero}",
            "pcmpeqb {zero}, {chunk}",
            "pcmpeqb {chunk}, {needle}",
            "pmovmskb {zeros:e}, {zero}",
            "pmovmskb {hits:e}, {chunk}",
            block = in(reg) block,
            needle = in(xmm_reg) needle,
            chunk = out(xmm_reg) _,
            zero = out(xmm_reg) _,
            zeros = lateout(reg) zeros,
            hits = lateout(reg) hits,
            options(nostack, readonly, pure, preserves_flags),
        );
    }
    (zeros, hits)
}

/// Where `strcmp` stops in the sixteen bytes at `a` and `b`: bit `i` set
/// where the two differ, or agree on the terminator.
///
/// # Safety
///
/// `a` and `b` readable for sixteen bytes each.
#[inline(always)]
unsafe fn stops16(a: *const u8, b: *const u8) -> u32 {
    let mask: u32;
    // SAFETY: the caller's readable bytes; unaligned loads.
    unsafe {
        core::arch::asm!(
            "movdqu {x}, xmmword ptr [{a}]",
            "movdqu {y}, xmmword ptr [{b}]",
            // 0xFF where the bytes are equal, then (the unsigned minimum
            // with `a`'s) `a`'s byte where they are equal and 0 where not.
            "pcmpeqb {y}, {x}",
            "pminub {y}, {x}",
            // 0xFF where that is 0: a difference, or the terminator in both.
            "pxor {x}, {x}",
            "pcmpeqb {y}, {x}",
            "pmovmskb {mask:e}, {y}",
            a = in(reg) a,
            b = in(reg) b,
            x = out(xmm_reg) _,
            y = out(xmm_reg) _,
            mask = lateout(reg) mask,
            options(nostack, readonly, pure, preserves_flags),
        );
    }
    mask
}

/// `a`'s byte less `b`'s, as unsigned values: how the comparisons answer.
///
/// # Safety
///
/// `a` and `b` readable.
#[inline(always)]
unsafe fn byte_difference(a: *const u8, b: *const u8) -> i32 {
    // SAFETY: the caller's bytes.
    unsafe { i32::from(a.read()).wrapping_sub(i32::from(b.read())) }
}

/// The index of the highest bit set in `mask`, which is not 0.
#[inline(always)]
fn highest_bit(mask: u32) -> usize {
    // `leading_zeros` is 0 to 31 here, and 31 ^ x is 31 - x for those.
    (31 ^ mask.leading_zeros()) as usize
}

/// The first byte of the C string at `s` that is `byte` or its terminator:
/// `strchrnul`, which `strchr` and `strstr` share.
///
/// # Safety
///
/// `s` is a readable C string.
#[inline(always)]
unsafe fn byte_or_nul(s: *const u8, byte: u8) -> *const u8 {
    let needle = splat(byte);
    let skip = (s as usize & 15) as u32;
    let mut block = s.wrapping_sub(skip as usize);
    // SAFETY: the string is readable to its terminator.  The first block
    // holds `s`; each later one is read only when the one before held
    // neither the byte nor the terminator, so it starts at a byte of the
    // string not yet examined, and lies in that byte's page.
    unsafe {
        let (zeros, hits) = block_masks(block, needle);
        // The first block's bytes below `s` are not the string's.
        let first = (zeros | hits).wrapping_shr(skip);
        if first != 0 {
            return s.wrapping_add(first.trailing_zeros() as usize);
        }
        loop {
            block = block.wrapping_add(16);
            let (zeros, hits) = block_masks(block, needle);
            if zeros | hits != 0 {
                return block.wrapping_add((zeros | hits).trailing_zeros() as usize);
            }
        }
    }
}

/// The length of the C string at `s`, or `max` if its first `max` bytes
/// hold no terminator: `strnlen`, which the bounded copies share.
///
/// # Safety
///
/// `s` readable for `max` bytes, or to a terminator within them.
#[inline(always)]
unsafe fn length_within(s: *const u8, max: usize) -> usize {
    if max == 0 {
        return 0;
    }
    let skip = (s as usize & 15) as u32;
    let none = splat(0);
    // SAFETY: `s`'s first byte is readable (`max` is not 0), so its block
    // is.  A later block is read only while it starts below `max` and no
    // terminator came before it: at a byte the caller vouches for.
    unsafe {
        let (zeros, _) = block_masks(s.wrapping_sub(skip as usize), none);
        let first = zeros.wrapping_shr(skip);
        if first != 0 {
            return (first.trailing_zeros() as usize).min(max);
        }
        let mut at = 16usize.wrapping_sub(skip as usize);
        while at < max {
            let (zeros, _) = block_masks(s.wrapping_add(at), none);
            if zeros != 0 {
                return at.wrapping_add(zeros.trailing_zeros() as usize).min(max);
            }
            at = at.saturating_add(16);
        }
    }
    max
}

/// Own archive member — gnulib replaces `strnlen`. See the module header.
mod gnu_strnlen {
    use super::*;

    /// Compute the length of a C string, limited to `maxlen`.
    ///
    /// Sixteen bytes a step, as `strlen`, reading nothing from a block
    /// that starts at or past `maxlen`.
    ///
    /// # Safety
    ///
    /// `s` must be valid for at least `maxlen` bytes, or be
    /// null-terminated before `maxlen`.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub unsafe extern "C" fn strnlen(s: *const u8, maxlen: SizeT) -> SizeT {
        // SAFETY: the caller's bytes, as `length_within` needs them.
        unsafe { length_within(s, maxlen) }
    }
}
pub use gnu_strnlen::strnlen;

/// Compare two C strings.
///
/// Returns 0 if equal, else the difference of the first differing bytes
/// as unsigned values (`s1`'s less `s2`'s): negative if `s1` sorts first.
/// Sixteen bytes a step, a byte at a time where sixteen would cross a page.
///
/// # Safety
///
/// Both strings must be valid null-terminated strings.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn strcmp(s1: *const u8, s2: *const u8) -> i32 {
    let mut i = 0usize;
    // SAFETY: both strings are readable to their terminators, and nothing
    // is compared past the first difference or the first terminator (a
    // shorter string's terminator is a difference).  Sixteen bytes are read
    // only where neither run crosses a page, so each shares a page with its
    // string's byte `i`, which is readable; otherwise one byte of each.
    unsafe {
        loop {
            let (a, b) = (s1.wrapping_add(i), s2.wrapping_add(i));
            if within_page(a) && within_page(b) {
                let stops = stops16(a, b);
                if stops != 0 {
                    let at = stops.trailing_zeros() as usize;
                    return byte_difference(a.add(at), b.add(at));
                }
                i = i.wrapping_add(16);
            } else {
                let (x, y) = (a.read(), b.read());
                if x != y || x == 0 {
                    return i32::from(x).wrapping_sub(i32::from(y));
                }
                i = i.wrapping_add(1);
            }
        }
    }
}

/// Compare at most `n` bytes of two C strings, as `strcmp`.
///
/// # Safety
///
/// Both strings must be valid for at least `n` bytes or be
/// null-terminated before `n`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn strncmp(s1: *const u8, s2: *const u8, n: SizeT) -> i32 {
    let mut i = 0usize;
    // SAFETY: as `strcmp`'s, comparing nothing from `n` on.  A run of
    // sixteen may extend past `n`, but shares a page with its string's
    // byte `i`, which is below `n` and so readable.
    unsafe {
        while i < n {
            let (a, b) = (s1.wrapping_add(i), s2.wrapping_add(i));
            if within_page(a) && within_page(b) {
                let left = n.wrapping_sub(i);
                let stops = stops16(a, b);
                if stops != 0 {
                    let at = stops.trailing_zeros() as usize;
                    if at >= left {
                        return 0;
                    }
                    return byte_difference(a.add(at), b.add(at));
                }
                if left <= 16 {
                    return 0;
                }
                i = i.wrapping_add(16);
            } else {
                let (x, y) = (a.read(), b.read());
                if x != y || x == 0 {
                    return i32::from(x).wrapping_sub(i32::from(y));
                }
                i = i.wrapping_add(1);
            }
        }
    }
    0
}

/// Copy a C string (including null terminator).
///
/// `strlen`, then `memcpy`.
///
/// # Safety
///
/// `dest` must be large enough to hold the string.  `src` must be
/// a valid null-terminated string.  Regions must not overlap.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn strcpy(dest: *mut u8, src: *const u8) -> *mut u8 {
    // SAFETY: the caller's string, and room for it with its terminator.
    unsafe {
        memcpy(dest, src, strlen(src).wrapping_add(1));
    }
    dest
}

/// Copy at most `n` bytes of a C string (pad with nulls).
///
/// # Safety
///
/// `dest` must be valid for `n` bytes.  `src` must be a valid
/// null-terminated string, or hold `n` readable bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn strncpy(dest: *mut u8, src: *const u8, n: SizeT) -> *mut u8 {
    // SAFETY: `src` is read to its terminator or for `n` bytes, and `dest`
    // written for `n`: the string's `len` bytes, then `n - len` zeros.
    unsafe {
        let len = length_within(src, n);
        memcpy(dest, src, len);
        memset(dest.add(len), 0, n.wrapping_sub(len));
    }
    dest
}

/// Find the first occurrence of `c` in string `s`.
///
/// Returns pointer to the character, or NULL.  `c` may be 0, which finds
/// the terminator.
///
/// # Safety
///
/// `s` must be a valid null-terminated string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn strchr(s: *const u8, c: i32) -> *const u8 {
    // C's `(char)c`.
    let byte = c as u8;
    // SAFETY: the caller's string; `byte_or_nul` answers a byte of it.
    unsafe {
        let found = byte_or_nul(s, byte);
        if found.read() == byte {
            found
        } else {
            core::ptr::null()
        }
    }
}

/// Own archive member — gnulib replaces `strchrnul`. See the module header.
mod gnu_strchrnul {
    use super::*;

    /// Like `strchr`, but returns a pointer to the null terminator if
    /// `c` is not found (instead of null).
    ///
    /// GNU extension — commonly used by glibc-based programs.
    ///
    /// # Safety
    ///
    /// `s` must be a valid null-terminated string.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub unsafe extern "C" fn strchrnul(s: *const u8, c: i32) -> *const u8 {
        // SAFETY: the caller's string.
        unsafe { byte_or_nul(s, c as u8) }
    }
}
pub use gnu_strchrnul::strchrnul;

/// Find the last occurrence of `c` in string `s`.
///
/// One pass: the last block that held `c` is remembered until the block
/// holding the terminator.
///
/// # Safety
///
/// `s` must be a valid null-terminated string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn strrchr(s: *const u8, c: i32) -> *const u8 {
    let needle = splat(c as u8);
    let skip = (s as usize & 15) as u32;
    let mut block = s.wrapping_sub(skip as usize);
    let mut last = core::ptr::null();
    // SAFETY: as `byte_or_nul`'s: each block read starts at a byte of the
    // string not yet examined, the first holding `s`.
    unsafe {
        let (zeros, hits) = block_masks(block, needle);
        // Bit `i` of the masks is the byte at `base + i`; the first block's
        // bytes below `s` are not the string's.
        let (mut zeros, mut hits, mut base) =
            (zeros.wrapping_shr(skip), hits.wrapping_shr(skip), s);
        loop {
            if zeros != 0 {
                // The terminator's block: what counts is up to the
                // terminator, inclusive -- `c` may be 0.
                let hits = hits & (zeros ^ zeros.wrapping_sub(1));
                if hits != 0 {
                    return base.wrapping_add(highest_bit(hits));
                }
                return last;
            }
            if hits != 0 {
                last = base.wrapping_add(highest_bit(hits));
            }
            block = block.wrapping_add(16);
            base = block;
            (zeros, hits) = block_masks(block, needle);
        }
    }
}

/// Concatenate two C strings.
///
/// Appends `src` to the end of `dest`.
///
/// # Safety
///
/// `dest` must have enough space for the combined string.
/// Both must be valid null-terminated strings.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn strcat(dest: *mut u8, src: *const u8) -> *mut u8 {
    // SAFETY: the caller's strings, and room after `dest`'s for `src` with
    // its terminator.
    unsafe {
        let end = dest.add(strlen(dest));
        memcpy(end, src, strlen(src).wrapping_add(1));
    }
    dest
}

/// Concatenate at most `n` bytes of `src` to `dest`.
///
/// # Safety
///
/// `dest` must have enough space for the combined string (up to n extra
/// bytes + null terminator).  `src` must be a valid null-terminated
/// string, or hold `n` readable bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn strncat(dest: *mut u8, src: *const u8, n: SizeT) -> *mut u8 {
    // SAFETY: the caller's strings, `src` read to its terminator or for
    // `n` bytes, and room after `dest`'s for those and a terminator.
    unsafe {
        let end = dest.add(strlen(dest));
        let len = length_within(src, n);
        memcpy(end, src, len);
        end.add(len).write(0);
    }
    dest
}

/// A needle prepared for the Two-Way search (Crochemore and Perrin, 1991)
/// as glibc's and gnulib's `str-two-way.h` do it: the first place the needle
/// occurs in a haystack, in time linear in the two lengths and constant
/// space.  The loops it replaced compared the whole needle at every
/// position: `strstr` of a thousand `a`s and a `b` in a megabyte of `a`s
/// was a billion steps.
///
/// The needle is split at a *critical factorization*: a right half,
/// compared first, at a mismatch in which no occurrence can start before
/// the mismatch lines up, and a left half compared only when the right half
/// matched.  A needle whose left half repeats at the right half's period is
/// *periodic*: after a failure there, the search moves by the period and
/// remembers how much of the right half then already matches.
///
/// Elements compare through `canon`: the identity, or ASCII case folding
/// for `strcasestr`.  `T` is a byte here and a wide character for
/// `wcsstr`.
pub(crate) struct TwoWay<T, C> {
    needle: *const T,
    len: usize,
    canon: C,
    /// Where the right half starts.
    suffix: usize,
    /// How far the needle moves after a failure in the left half.
    shift: usize,
    /// Whether the left half repeats at the right half's period.
    periodic: bool,
}

impl<T: Copy + Ord, C: Fn(T) -> T + Copy> TwoWay<T, C> {
    /// Prepare `needle`, of `len` elements, at least one.
    ///
    /// # Safety
    ///
    /// `needle` readable for `len` elements for as long as `self` is used.
    #[inline(always)]
    pub(crate) unsafe fn new(needle: *const T, len: usize, canon: C) -> Self {
        let mut this = Self {
            needle,
            len,
            canon,
            suffix: 0,
            shift: 1,
            periodic: false,
        };
        // SAFETY: the caller's `len` elements.
        let (suffix, period) = unsafe { this.critical_factorization() };
        this.suffix = suffix;
        // SAFETY (each `at`): `i < suffix` and `i + period < suffix + period
        // <= len`, which the first clause checks.
        this.periodic = suffix.checked_add(period).is_some_and(|end| end <= len)
            && (0..suffix).all(|i| unsafe { this.at(i) == this.at(i.wrapping_add(period)) });
        this.shift = if this.periodic {
            period
        } else {
            // Halves that differ: a failure in the left half moves the
            // needle past the longer one.
            suffix.max(len.wrapping_sub(suffix)).wrapping_add(1)
        };
        this
    }

    /// The needle's element `i`, through `canon`.
    ///
    /// # Safety
    ///
    /// `i < len`.
    #[inline(always)]
    unsafe fn at(&self, i: usize) -> T {
        // SAFETY: the caller's index, within the needle.
        (self.canon)(unsafe { self.needle.add(i).read() })
    }

    /// glibc's `critical_factorization`: where the right half starts, and
    /// its period.  Of the maximal suffixes in the element order and in
    /// its reverse, the right half is the one that starts later.
    ///
    /// # Safety
    ///
    /// The needle's `len` elements are readable.
    #[inline(always)]
    unsafe fn critical_factorization(&self) -> (usize, usize) {
        if self.len < 3 {
            return (self.len.wrapping_sub(1), 1);
        }
        // SAFETY: the caller's elements.
        let ((before, period), (before_rev, period_rev)) =
            unsafe { (self.maximal_suffix(false), self.maximal_suffix(true)) };
        // `usize::MAX` stands for -1, which adding 1 maps to 0.
        if before_rev.wrapping_add(1) < before.wrapping_add(1) {
            (before.wrapping_add(1), period)
        } else {
            (before_rev.wrapping_add(1), period_rev)
        }
    }

    /// The needle's maximal suffix in the element order, or (`reverse`) in
    /// its opposite: the index of the element before it (`usize::MAX`, for
    /// -1, when it is the whole needle) and its period.  glibc's loop,
    /// renamed: `cand` is the candidate suffix's start, `offset` the place
    /// in its current period.
    ///
    /// # Safety
    ///
    /// The needle's `len` elements are readable.
    #[inline(always)]
    unsafe fn maximal_suffix(&self, reverse: bool) -> (usize, usize) {
        let mut before = usize::MAX;
        let (mut cand, mut offset, mut period) = (0usize, 1usize, 1usize);
        while cand.wrapping_add(offset) < self.len {
            // SAFETY: `cand + offset < len`, and `before < cand` (as signed
            // numbers: -1 at first), so `before + offset < len` too.
            let (ahead, known) = unsafe {
                (
                    self.at(cand.wrapping_add(offset)),
                    self.at(before.wrapping_add(offset)),
                )
            };
            if ahead == known {
                // Through a repetition of the current period.
                if offset == period {
                    cand = cand.wrapping_add(period);
                    offset = 1;
                } else {
                    offset = offset.wrapping_add(1);
                }
            } else if (ahead < known) != reverse {
                // A smaller suffix: its period is the whole prefix so far.
                cand = cand.wrapping_add(offset);
                offset = 1;
                period = cand.wrapping_sub(before);
            } else {
                // A larger suffix: start again from it.
                before = cand;
                cand = cand.wrapping_add(1);
                offset = 1;
                period = 1;
            }
        }
        (before, period)
    }

    /// The first place the needle occurs in `hay`, or null.  `fits(end)`
    /// answers whether the haystack has `end` elements, so a C string's
    /// length need be found only as far as the search goes.
    ///
    /// # Safety
    ///
    /// `hay` readable for as many elements as `fits` admits.
    #[inline(always)]
    pub(crate) unsafe fn find(
        &self,
        hay: *const T,
        mut fits: impl FnMut(usize) -> bool,
    ) -> *const T {
        let (len, suffix) = (self.len, self.suffix);
        // How much of the right half is known to match: after a periodic
        // needle's shift, all but its last `shift` elements.
        let mut memory = 0usize;
        let mut j = 0usize;
        while fits(j.saturating_add(len)) {
            // SAFETY (each `hay` read below): an index below `j + len`, which
            // `fits` admitted.  Each `at`'s index is below `len`.
            unsafe {
                // The right half, from what is not known to match.
                let mut i = suffix.max(memory);
                while i < len && self.at(i) == (self.canon)(hay.add(j.wrapping_add(i)).read()) {
                    i = i.wrapping_add(1);
                }
                if i < len {
                    // No occurrence starts before the mismatch lines up.
                    j = j.saturating_add(i.wrapping_sub(suffix).wrapping_add(1));
                    memory = 0;
                    continue;
                }
                // The left half, down to what is known to match; `i` is one
                // past the element compared.
                let mut i = suffix;
                while i > memory
                    && self.at(i.wrapping_sub(1))
                        == (self.canon)(hay.add(j.wrapping_add(i).wrapping_sub(1)).read())
                {
                    i = i.wrapping_sub(1);
                }
                if i <= memory {
                    return hay.wrapping_add(j);
                }
            }
            j = j.saturating_add(self.shift);
            if self.periodic {
                memory = len.wrapping_sub(self.shift);
            }
        }
        core::ptr::null()
    }
}

/// `TwoWay::find`'s `fits` for a C-string haystack: whether it has `end`
/// bytes before its terminator, looking only as far as asked -- `end` and
/// 512 bytes more each time, as glibc does -- so a match near the start of
/// a long string costs nothing for the rest of it.  Finding the length
/// first would make a loop of `strstr` calls along a string quadratic.
///
/// # Safety
///
/// `hay` is a readable C string for as long as the answer is used.
#[inline(always)]
unsafe fn in_string(hay: *const u8) -> impl FnMut(usize) -> bool {
    let mut known = 0usize;
    move |end| {
        if end > known {
            // SAFETY: the haystack is readable to its terminator, and
            // `known` never passes it.
            let more = unsafe {
                length_within(
                    hay.wrapping_add(known),
                    end.wrapping_sub(known).saturating_add(512),
                )
            };
            known = known.saturating_add(more);
        }
        end <= known
    }
}

/// The byte itself: what `strstr` and `memmem` compare.
#[inline(always)]
fn exact(byte: u8) -> u8 {
    byte
}

/// Find the first occurrence of substring `needle` in `haystack`.
///
/// Returns a pointer to the beginning of the match, or NULL if not found.
/// The Two-Way search ([`TwoWay`]): linear time, constant space.
///
/// # Safety
///
/// Both strings must be valid null-terminated strings.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn strstr(haystack: *const u8, needle: *const u8) -> *const u8 {
    // SAFETY: the caller's strings.  The haystack is read only as far as
    // `in_string` admits, which is never past its terminator.
    unsafe {
        let nlen = strlen(needle);
        match nlen {
            // Empty needle matches everything.
            0 => haystack,
            1 => {
                let found = byte_or_nul(haystack, needle.read());
                if found.read() == 0 {
                    core::ptr::null()
                } else {
                    found
                }
            }
            // A haystack shorter than the needle holds no match, which is
            // cheaper to see than the needle's factorization is to make.
            _ if length_within(haystack, nlen) < nlen => core::ptr::null(),
            _ => TwoWay::new(needle, nlen, exact).find(haystack, in_string(haystack)),
        }
    }
}

/// A set of bytes, as `strspn`'s family takes one: a bit for each value.
///
/// Four 64-bit words, byte `b`'s bit in word `b / 64`: a lookup is a load,
/// a shift and a test.  (Two `u128` halves were half the speed of the byte
/// loop they replaced: a variable shift of a `u128` is several
/// instructions.)
struct ByteSet([u64; 4]);

impl ByteSet {
    /// The bytes of the C string `set`, which never include its
    /// terminator.
    ///
    /// # Safety
    ///
    /// `set` is a readable C string.
    #[inline(always)]
    unsafe fn of(set: *const u8) -> Self {
        let mut bytes = Self([0; 4]);
        let mut i = 0usize;
        loop {
            // SAFETY: up to the terminator, which ends the loop.
            let byte = unsafe { set.add(i).read() };
            if byte == 0 {
                return bytes;
            }
            bytes.insert(byte);
            i = i.wrapping_add(1);
        }
    }

    #[inline(always)]
    fn insert(&mut self, byte: u8) {
        // `byte / 64` is below 4: the compiler drops the check, and there is
        // no index to lint.
        if let Some(word) = self.0.get_mut(usize::from(byte >> 6)) {
            *word |= 1u64.wrapping_shl(u32::from(byte & 63));
        }
    }

    #[inline(always)]
    fn contains(&self, byte: u8) -> bool {
        self.0
            .get(usize::from(byte >> 6))
            .is_some_and(|word| word.wrapping_shr(u32::from(byte & 63)) & 1 != 0)
    }
}

/// How many bytes of the C string `s` precede the first that is in the C
/// string `reject`, or its terminator: `strcspn`, which `strpbrk` shares.
///
/// # Safety
///
/// `s` and `reject` are readable C strings.
#[inline(always)]
unsafe fn span_outside(s: *const u8, reject: *const u8) -> usize {
    // SAFETY: the caller's strings.  The set holds the terminator, so the
    // loop stops at `s`'s at the latest.
    unsafe {
        let first = reject.read();
        if first == 0 {
            return strlen(s);
        }
        if reject.add(1).read() == 0 {
            // One byte, the commonest case: `strchrnul`'s scan.
            return (byte_or_nul(s, first) as usize).wrapping_sub(s as usize);
        }
        let mut set = ByteSet::of(reject);
        set.insert(0);
        let mut i = 0usize;
        while !set.contains(s.add(i).read()) {
            i = i.wrapping_add(1);
        }
        i
    }
}

/// Compute the length of the initial segment of `s` consisting
/// entirely of bytes in `accept`.
///
/// Each byte is looked up in a 256-bit set of `accept`'s bytes.
///
/// # Safety
///
/// Both strings must be valid null-terminated strings.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn strspn(s: *const u8, accept: *const u8) -> SizeT {
    // SAFETY: the caller's strings.  Neither the one byte nor the set is
    // the terminator, so each loop stops at `s`'s at the latest.
    unsafe {
        let first = accept.read();
        let mut i = 0usize;
        if first != 0 && accept.add(1).read() == 0 {
            // One byte, the commonest case: no set to build.
            while s.add(i).read() == first {
                i = i.wrapping_add(1);
            }
            return i;
        }
        let set = ByteSet::of(accept);
        while set.contains(s.add(i).read()) {
            i = i.wrapping_add(1);
        }
        i
    }
}

/// Compute the length of the initial segment of `s` consisting
/// entirely of bytes NOT in `reject`.
///
/// # Safety
///
/// Both strings must be valid null-terminated strings.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn strcspn(s: *const u8, reject: *const u8) -> SizeT {
    // SAFETY: the caller's strings.
    unsafe { span_outside(s, reject) }
}

/// Find the first occurrence in `s` of any byte in `accept`.
///
/// Returns a pointer to the byte, or NULL if none found.
///
/// # Safety
///
/// Both strings must be valid null-terminated strings.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn strpbrk(s: *const u8, accept: *const u8) -> *const u8 {
    // SAFETY: the caller's strings; `span_outside` stops at a byte of `s`,
    // its terminator at the latest.
    unsafe {
        let at = s.add(span_outside(s, accept));
        if at.read() == 0 {
            core::ptr::null()
        } else {
            at
        }
    }
}

/// Tokenize a string.
///
/// On the first call, `s` should point to the string to tokenize.
/// On subsequent calls, `s` should be NULL. The `delim` set may
/// change between calls.
///
/// Returns a pointer to the next token, or NULL when done.
///
/// # Safety
///
/// `s` (if non-null) and `delim` must be valid null-terminated strings.
/// Not thread-safe (uses a static saved position).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn strtok(s: *mut u8, delim: *const u8) -> *mut u8 {
    // Static saved position (POSIX strtok is not reentrant).
    static mut SAVED: *mut u8 = core::ptr::null_mut();

    // SAFETY: `SAVED` is reached only through `addr_of_mut!`, never through a
    // reference, so no `&mut` to a `static mut` is ever formed (the Rust 2024
    // rule). Sound *serialisation* of the accesses is a caller obligation, not
    // a fact about this function: POSIX specifies `strtok` around one
    // process-global save pointer and does not make it thread-safe —
    // `strtok_r`, which takes the save pointer as an argument, is the
    // reentrant form. A caller that needs concurrency must use that instead.
    //
    // This comment previously read "Single-threaded access", which asserted a
    // fact nothing established: this crate's own test module called `strtok`
    // from three tests on three `libtest` threads at once. That is now
    // serialised by `STRTOK_TEST_LOCK`. The danger there was not the flaky
    // assertion it produced but what it implies — a stale `SAVED` points into
    // whichever caller's buffer ran last, and the delimiter-overwrite below
    // writes a NUL through it, so an unserialised second caller corrupts a
    // *different* thread's live buffer.
    let start = if s.is_null() {
        let p = unsafe { core::ptr::addr_of_mut!(SAVED).read() };
        if p.is_null() {
            return core::ptr::null_mut();
        }
        p
    } else {
        s
    };

    // Skip leading delimiters.
    let mut i: usize = 0;
    loop {
        let c = unsafe { *start.add(i) };
        if c == 0 {
            // All delimiters, no token.
            unsafe {
                core::ptr::addr_of_mut!(SAVED).write(core::ptr::null_mut());
            }
            return core::ptr::null_mut();
        }
        if !unsafe { is_delim(c, delim) } {
            break;
        }
        i = i.wrapping_add(1);
    }

    let token = unsafe { start.add(i) };

    // Find end of token.
    let mut k: usize = 0;
    loop {
        let c = unsafe { *token.add(k) };
        if c == 0 {
            unsafe {
                core::ptr::addr_of_mut!(SAVED).write(core::ptr::null_mut());
            }
            return token;
        }
        if unsafe { is_delim(c, delim) } {
            unsafe {
                *token.add(k) = 0;
            }
            unsafe {
                core::ptr::addr_of_mut!(SAVED).write(token.add(k.wrapping_add(1)));
            }
            return token;
        }
        k = k.wrapping_add(1);
    }
}

/// Check if a byte is in the delimiter set.
#[inline]
unsafe fn is_delim(c: u8, delim: *const u8) -> bool {
    let mut j: usize = 0;
    loop {
        let d = unsafe { *delim.add(j) };
        if d == 0 {
            return false;
        }
        if c == d {
            return true;
        }
        j = j.wrapping_add(1);
    }
}

/// Each error number's text in the C locale, which is glibc's to the letter
/// (`posix/tools/oracle/strname_harness.py`); None for a number that is no
/// error's -- 41 and 58, which Linux left unused, and past 133. The one table
/// [`strerror`], [`strerrordesc_np`] and [`sys_errlist`] all read -- and
/// printf's `%m`.
pub(crate) const fn error_text(errnum: i32) -> Option<&'static core::ffi::CStr> {
    Some(match errnum {
        0 => c"Success",
        1 => c"Operation not permitted",
        2 => c"No such file or directory",
        3 => c"No such process",
        4 => c"Interrupted system call",
        5 => c"Input/output error",
        6 => c"No such device or address",
        7 => c"Argument list too long",
        8 => c"Exec format error",
        9 => c"Bad file descriptor",
        10 => c"No child processes",
        11 => c"Resource temporarily unavailable",
        12 => c"Cannot allocate memory",
        13 => c"Permission denied",
        14 => c"Bad address",
        15 => c"Block device required",
        16 => c"Device or resource busy",
        17 => c"File exists",
        18 => c"Invalid cross-device link",
        19 => c"No such device",
        20 => c"Not a directory",
        21 => c"Is a directory",
        22 => c"Invalid argument",
        23 => c"Too many open files in system",
        24 => c"Too many open files",
        25 => c"Inappropriate ioctl for device",
        26 => c"Text file busy",
        27 => c"File too large",
        28 => c"No space left on device",
        29 => c"Illegal seek",
        30 => c"Read-only file system",
        31 => c"Too many links",
        32 => c"Broken pipe",
        33 => c"Numerical argument out of domain",
        34 => c"Numerical result out of range",
        35 => c"Resource deadlock avoided",
        36 => c"File name too long",
        37 => c"No locks available",
        38 => c"Function not implemented",
        39 => c"Directory not empty",
        40 => c"Too many levels of symbolic links",
        42 => c"No message of desired type",
        43 => c"Identifier removed",
        44 => c"Channel number out of range",
        45 => c"Level 2 not synchronized",
        46 => c"Level 3 halted",
        47 => c"Level 3 reset",
        48 => c"Link number out of range",
        49 => c"Protocol driver not attached",
        50 => c"No CSI structure available",
        51 => c"Level 2 halted",
        52 => c"Invalid exchange",
        53 => c"Invalid request descriptor",
        54 => c"Exchange full",
        55 => c"No anode",
        56 => c"Invalid request code",
        57 => c"Invalid slot",
        59 => c"Bad font file format",
        60 => c"Device not a stream",
        61 => c"No data available",
        62 => c"Timer expired",
        63 => c"Out of streams resources",
        64 => c"Machine is not on the network",
        65 => c"Package not installed",
        66 => c"Object is remote",
        67 => c"Link has been severed",
        68 => c"Advertise error",
        69 => c"Srmount error",
        70 => c"Communication error on send",
        71 => c"Protocol error",
        72 => c"Multihop attempted",
        73 => c"RFS specific error",
        74 => c"Bad message",
        75 => c"Value too large for defined data type",
        76 => c"Name not unique on network",
        77 => c"File descriptor in bad state",
        78 => c"Remote address changed",
        79 => c"Can not access a needed shared library",
        80 => c"Accessing a corrupted shared library",
        81 => c".lib section in a.out corrupted",
        82 => c"Attempting to link in too many shared libraries",
        83 => c"Cannot exec a shared library directly",
        84 => c"Invalid or incomplete multibyte or wide character",
        85 => c"Interrupted system call should be restarted",
        86 => c"Streams pipe error",
        87 => c"Too many users",
        88 => c"Socket operation on non-socket",
        89 => c"Destination address required",
        90 => c"Message too long",
        91 => c"Protocol wrong type for socket",
        92 => c"Protocol not available",
        93 => c"Protocol not supported",
        94 => c"Socket type not supported",
        95 => c"Operation not supported",
        96 => c"Protocol family not supported",
        97 => c"Address family not supported by protocol",
        98 => c"Address already in use",
        99 => c"Cannot assign requested address",
        100 => c"Network is down",
        101 => c"Network is unreachable",
        102 => c"Network dropped connection on reset",
        103 => c"Software caused connection abort",
        104 => c"Connection reset by peer",
        105 => c"No buffer space available",
        106 => c"Transport endpoint is already connected",
        107 => c"Transport endpoint is not connected",
        108 => c"Cannot send after transport endpoint shutdown",
        109 => c"Too many references: cannot splice",
        110 => c"Connection timed out",
        111 => c"Connection refused",
        112 => c"Host is down",
        113 => c"No route to host",
        114 => c"Operation already in progress",
        115 => c"Operation now in progress",
        116 => c"Stale file handle",
        117 => c"Structure needs cleaning",
        118 => c"Not a XENIX named type file",
        119 => c"No XENIX semaphores available",
        120 => c"Is a named type file",
        121 => c"Remote I/O error",
        122 => c"Disk quota exceeded",
        123 => c"No medium found",
        124 => c"Wrong medium type",
        125 => c"Operation canceled",
        126 => c"Required key not available",
        127 => c"Key has expired",
        128 => c"Key has been revoked",
        129 => c"Key was rejected by service",
        130 => c"Owner died",
        131 => c"State not recoverable",
        132 => c"Operation not possible due to RF-kill",
        133 => c"Memory page has hardware error",
        _ => return None,
    })
}

/// Return a string describing an error number: [`error_text`]'s, or for a
/// number that is no error's glibc's "Unknown error N", in the calling
/// thread's own buffer (valid until its next call). Never NULL.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn strerror(errnum: i32) -> *const u8 {
    match error_text(errnum) {
        Some(text) => text.as_ptr().cast::<u8>(),
        // SAFETY: the calling thread's block, touched by no other thread.
        None => crate::perthread::numbered(
            unsafe { &mut (*crate::perthread::current()).strerror },
            "Unknown error ",
            errnum,
        ),
    }
}

/// Duplicate a string.
///
/// Allocates memory for a copy of `s` using `malloc`.  The caller
/// must free the result with `free()`.
///
/// # Safety
///
/// `s` must be a valid null-terminated string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn strdup(s: *const u8) -> *mut u8 {
    if s.is_null() {
        return core::ptr::null_mut();
    }

    let len = unsafe { strlen(s) };
    let size = len.wrapping_add(1);

    // Allocate via malloc so the pointer has a valid header for free().
    // The previous implementation used mmap directly, which produced
    // pointers incompatible with free() (no [mmap_base, total_size]
    // header), causing memory corruption on free(strdup(...)).
    let dest = crate::malloc::malloc(size);
    if dest.is_null() {
        return core::ptr::null_mut();
    }

    // SAFETY: malloc returned valid memory of at least `size` bytes.
    unsafe {
        memcpy(dest, s, size);
    }
    dest
}

/// Own archive member — gnulib replaces `strndup`. See the module header.
mod gnu_strndup {
    use super::*;

    /// Duplicate at most `n` bytes of a string.
    ///
    /// Allocates memory for a copy of at most `n` bytes from `s`,
    /// plus a null terminator.  The result is always null-terminated.
    /// The caller must free the result with `free()`.
    ///
    /// # Safety
    ///
    /// `s` must be a valid null-terminated string (or valid for `n` bytes).
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub unsafe extern "C" fn strndup(s: *const u8, n: usize) -> *mut u8 {
        if s.is_null() {
            return core::ptr::null_mut();
        }

        // Find actual length (min of strlen and n).
        let len = unsafe { strnlen(s, n) };
        let size = len.wrapping_add(1);

        // Allocate via malloc so the pointer has a valid header for free().
        let dest = crate::malloc::malloc(size);
        if dest.is_null() {
            return core::ptr::null_mut();
        }

        // SAFETY: malloc returned valid memory of at least `size` bytes.
        unsafe {
            memcpy(dest, s, len);
        }
        unsafe {
            *dest.add(len) = 0;
        }
        dest
    }
}
pub use gnu_strndup::strndup;

/// Own archive member — gnulib replaces `memrchr`. See the module header.
mod gnu_memrchr {
    use super::*;

    /// Find the last occurrence of byte `c` in the first `n` bytes of `s`.
    ///
    /// Scans backward from position `n-1`, sixteen bytes a step (SSE2),
    /// never reading outside the `n`.
    ///
    /// # Safety
    ///
    /// `s` must be valid for `n` bytes.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub unsafe extern "C" fn memrchr(s: *const u8, c: i32, n: usize) -> *const u8 {
        // C's `(unsigned char)c`.
        let byte = c as u8;
        // SAFETY: every read below lies within the first `n` bytes of `s`,
        // which the caller vouches for.
        unsafe {
            if n >= 16 {
                let needle = splat(byte);
                let mut end = n;
                while end >= 16 {
                    let at = end.wrapping_sub(16);
                    let mask = matches16(s.add(at), needle);
                    if mask != 0 {
                        return s.add(at.wrapping_add(highest_bit(mask)));
                    }
                    end = at;
                }
                if end > 0 {
                    // The first `end` bytes: the sixteen at the start, of
                    // which those from `end` on were searched already.
                    let below = !u32::MAX.wrapping_shl(end as u32);
                    let mask = matches16(s, needle) & below;
                    if mask != 0 {
                        return s.add(highest_bit(mask));
                    }
                }
                return core::ptr::null();
            }
            let mut i = n;
            while i > 0 {
                i = i.wrapping_sub(1);
                if s.add(i).read() == byte {
                    return s.add(i);
                }
            }
        }
        core::ptr::null()
    }
}
pub use gnu_memrchr::memrchr;

/// Copy `n` bytes from `src` to `dest`, guaranteeing non-overlap.
///
/// Identical to `memcpy` — exists for C programs that reference
/// `bcopy` (BSD legacy).
///
/// # Safety
///
/// `src` and `dest` must be valid for `n` bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn bcopy(src: *const u8, dest: *mut u8, n: usize) {
    unsafe {
        memmove(dest, src, n);
    }
}

/// Set `n` bytes to zero.
///
/// # Safety
///
/// `s` must be valid for `n` bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn bzero(s: *mut u8, n: usize) {
    unsafe {
        memset(s, 0, n);
    }
}

// ---------------------------------------------------------------------------
// ffs / ffsl / ffsll — find first set bit
// ---------------------------------------------------------------------------

/// Find the first set bit in an integer.
///
/// Returns the 1-based position of the least significant set bit,
/// or 0 if `i` is 0.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ffs(i: i32) -> i32 {
    if i == 0 {
        return 0;
    }
    // trailing_zeros gives 0-based position; POSIX wants 1-based.
    ((i as u32).trailing_zeros() as i32).wrapping_add(1)
}

/// Find the first set bit in a long integer.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ffsl(i: i64) -> i32 {
    if i == 0 {
        return 0;
    }
    ((i as u64).trailing_zeros() as i32).wrapping_add(1)
}

/// Find the first set bit in a long long integer.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ffsll(i: i64) -> i32 {
    ffsl(i)
}

/// Compare two strings, case-insensitive.
///
/// # Safety
///
/// Both strings must be valid null-terminated strings.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn strcasecmp(s1: *const u8, s2: *const u8) -> i32 {
    let mut i: usize = 0;
    loop {
        let a = unsafe { *s1.add(i) };
        let b = unsafe { *s2.add(i) };
        let la = a.to_ascii_lowercase();
        let lb = b.to_ascii_lowercase();
        if la != lb || a == 0 {
            return i32::from(la).wrapping_sub(i32::from(lb));
        }
        i = i.wrapping_add(1);
    }
}

/// Compare at most `n` bytes of two strings, case-insensitive.
///
/// # Safety
///
/// Both strings must be valid for at least `n` bytes or be
/// null-terminated before `n`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn strncasecmp(s1: *const u8, s2: *const u8, n: usize) -> i32 {
    let mut i: usize = 0;
    while i < n {
        let a = unsafe { *s1.add(i) };
        let b = unsafe { *s2.add(i) };
        let la = a.to_ascii_lowercase();
        let lb = b.to_ascii_lowercase();
        if la != lb || a == 0 {
            return i32::from(la).wrapping_sub(i32::from(lb));
        }
        i = i.wrapping_add(1);
    }
    0
}

/// [`strcasecmp`] in an explicit locale (POSIX.1-2008). The library has one
/// locale (`crate::locale`: every `newlocale` returns the same tag), so the
/// argument is accepted and unused, as by the `ctype` and `wctype` `_l`
/// functions -- and by musl's, for the same reason.
///
/// # Safety
///
/// As for [`strcasecmp`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn strcasecmp_l(
    s1: *const u8,
    s2: *const u8,
    _loc: crate::locale::LocaleT,
) -> i32 {
    // SAFETY: this function's contract.
    unsafe { strcasecmp(s1, s2) }
}

/// [`strncasecmp`] in an explicit locale; see [`strcasecmp_l`].
///
/// # Safety
///
/// As for [`strncasecmp`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn strncasecmp_l(
    s1: *const u8,
    s2: *const u8,
    n: usize,
    _loc: crate::locale::LocaleT,
) -> i32 {
    // SAFETY: this function's contract.
    unsafe { strncasecmp(s1, s2, n) }
}

// ---------------------------------------------------------------------------
// Additional string functions
// ---------------------------------------------------------------------------

/// Own archive member — gnulib replaces `stpcpy`. See the module header.
mod gnu_stpcpy {
    /// Copy a string, returning a pointer to the END (the null terminator).
    ///
    /// This is the BSD/POSIX `stpcpy` — unlike `strcpy`, it returns a
    /// pointer to the terminating null byte, making chained copies efficient.
    ///
    /// # Safety
    ///
    /// `dest` must have enough space for the full `src` string plus null.
    /// `src` must be a valid null-terminated string.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub unsafe extern "C" fn stpcpy(dest: *mut u8, src: *const u8) -> *mut u8 {
        // SAFETY: the caller's string, and room for it with its terminator.
        unsafe {
            let len = super::strlen(src);
            super::memcpy(dest, src, len.wrapping_add(1));
            dest.add(len)
        }
    }
}
pub use gnu_stpcpy::stpcpy;

/// Own archive member — gnulib replaces `stpncpy`. See the module header.
mod gnu_stpncpy {
    /// Copy at most `n` bytes from `src` to `dest`, returning a pointer
    /// past the last character written.
    ///
    /// If `src` is shorter than `n`, remaining bytes are filled with null
    /// and a pointer to the first null byte is returned.
    ///
    /// # Safety
    ///
    /// `dest` must have space for at least `n` bytes.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub unsafe extern "C" fn stpncpy(dest: *mut u8, src: *const u8, n: usize) -> *mut u8 {
        // SAFETY: `src` is read to its terminator or for `n` bytes, and
        // `dest` written for `n`: the string's `len` bytes, then zeros.
        unsafe {
            let len = super::length_within(src, n);
            super::memcpy(dest, src, len);
            super::memset(dest.add(len), 0, n.wrapping_sub(len));
            dest.add(len)
        }
    }
}
pub use gnu_stpncpy::stpncpy;

/// Extract token from string (reentrant, modifies input).
///
/// `strsep` is the BSD replacement for `strtok`.  It modifies the
/// string pointer `*stringp` to point past the delimiter (or sets
/// it to NULL when no more tokens remain).
///
/// Returns the original `*stringp` value (the token start), or NULL
/// if `*stringp` was NULL.
///
/// # Safety
///
/// `stringp` must point to a valid `*mut u8` pointer (which itself
/// points to a writable null-terminated string or is null).
/// `delim` must be a valid null-terminated string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::many_single_char_names)] // POSIX function, C convention variables.
pub unsafe extern "C" fn strsep(stringp: *mut *mut u8, delim: *const u8) -> *mut u8 {
    if stringp.is_null() {
        return core::ptr::null_mut();
    }

    let s = unsafe { *stringp };
    if s.is_null() {
        return core::ptr::null_mut();
    }

    let begin = s;
    let mut i: usize = 0;
    loop {
        let c = unsafe { *s.add(i) };
        if c == 0 {
            // Reached end of string — no more tokens.
            unsafe {
                *stringp = core::ptr::null_mut();
            }
            return begin;
        }

        // Check if c is a delimiter.
        let mut j: usize = 0;
        loop {
            let d = unsafe { *delim.add(j) };
            if d == 0 {
                break;
            }
            if c == d {
                // Replace delimiter with null and advance past it.
                unsafe {
                    *s.add(i) = 0;
                }
                unsafe {
                    *stringp = s.add(i.wrapping_add(1));
                }
                return begin;
            }
            j = j.wrapping_add(1);
        }

        i = i.wrapping_add(1);
    }
}

/// Own archive member — gnulib replaces `strverscmp`. See the module header.
///
/// The two private helpers live in here with it: they are only ever called by
/// `strverscmp`, so keeping them in the same member costs nothing and keeps the
/// member self-contained (it references nothing from the main string module).
mod gnu_strverscmp {
    /// Version-aware string comparison (GNU extension, `<string.h>`).
    ///
    /// Like `strcmp`, but when both strings contain a run of digits at the
    /// same position, the digit runs are compared *numerically* rather than
    /// lexicographically.  This gives the intuitive result for version
    /// strings: `"file9" < "file10"`, `"1.2.3" < "1.10.0"`.
    ///
    /// Leading-zero handling follows the glibc convention: a digit run with
    /// a leading zero is compared as a fractional part (lexicographic, so
    /// longer run with same prefix is greater), while runs without leading
    /// zeros are compared by numeric value (shorter run with same digits is
    /// smaller).
    ///
    /// Based on glibc `strverscmp` (`string/strverscmp.c`).
    ///
    /// # Safety
    ///
    /// Both pointers must be valid null-terminated strings.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub unsafe extern "C" fn strverscmp(s1: *const u8, s2: *const u8) -> i32 {
        let mut i: usize = 0;

        // Scan forward while characters are equal.
        loop {
            let a = unsafe { *s1.add(i) };
            let b = unsafe { *s2.add(i) };

            if a != b || a == 0 {
                // Found the first difference (or end of both strings).
                // If neither byte is a digit, fall through to plain compare.
                // If at least one is a digit, we need version comparison.
                let a_dig = a.is_ascii_digit();
                let b_dig = b.is_ascii_digit();

                if !a_dig && !b_dig {
                    // Neither is a digit — normal lexicographic result.
                    // SAFETY: a and b are u8, so their i32 values are in [0, 255];
                    // the difference is in [-255, 255] which cannot overflow i32.
                    return i32::from(a).wrapping_sub(i32::from(b));
                }

                // At least one is a digit.  Walk back to find the start of
                // the digit run that includes position `i`.
                let mut start = i;
                while start > 0 && unsafe { *s1.add(start.wrapping_sub(1)) }.is_ascii_digit() {
                    start = start.wrapping_sub(1);
                }

                // If we're NOT inside a digit run (start == i) and only one
                // side has a digit, fall back to plain byte comparison.  This
                // matches glibc's state-machine behaviour in state S_N (normal):
                // a lone digit vs a letter is compared by code point value.
                if start == i && (!a_dig || !b_dig) {
                    return i32::from(a).wrapping_sub(i32::from(b));
                }

                // Check for leading zeros in the shared digit run.
                let has_leading_zero =
                    unsafe { *s1.add(start) } == b'0' || unsafe { *s2.add(start) } == b'0';

                if has_leading_zero {
                    // Fractional comparison: compare digit-by-digit (lexicographic).
                    // A digit beats a non-digit (non-digit means the run ended),
                    // but a shorter fractional part with the same prefix is less.
                    return strverscmp_frac(s1, s2, start);
                }

                // Integer comparison: longer digit run = larger number.
                return strverscmp_int(s1, s2, start);
            }

            i = i.wrapping_add(1);
        }
    }

    /// Fractional-style digit run comparison (leading-zero case).
    ///
    /// Compare digit-by-digit from `start`.  When one run ends (non-digit or NUL)
    /// and the other continues, the continuing run is "greater."
    fn strverscmp_frac(s1: *const u8, s2: *const u8, start: usize) -> i32 {
        let mut j = start;
        loop {
            let a = unsafe { *s1.add(j) };
            let b = unsafe { *s2.add(j) };
            let a_dig = a.is_ascii_digit();
            let b_dig = b.is_ascii_digit();

            if !a_dig && !b_dig {
                return 0; // Same digit run, same length.
            }
            if !a_dig {
                return -1; // s1 run ended first → s1 < s2.
            }
            if !b_dig {
                return 1; // s2 run ended first → s1 > s2.
            }
            if a != b {
                // SAFETY: a and b are u8, so their i32 values are in [0, 255];
                // the difference is in [-255, 255] which cannot overflow i32.
                return i32::from(a).wrapping_sub(i32::from(b));
            }
            j = j.wrapping_add(1);
        }
    }

    /// Integer-style digit run comparison (no leading-zero case).
    ///
    /// The longer digit run represents a larger number.  If runs are the
    /// same length, the first differing digit decides.
    fn strverscmp_int(s1: *const u8, s2: *const u8, start: usize) -> i32 {
        let mut j = start;
        let mut first_diff: i32 = 0;
        loop {
            let a = unsafe { *s1.add(j) };
            let b = unsafe { *s2.add(j) };
            let a_dig = a.is_ascii_digit();
            let b_dig = b.is_ascii_digit();

            if !a_dig && !b_dig {
                // Same length — use first differing digit.
                return first_diff;
            }
            if !a_dig {
                return -1; // s1 run shorter → smaller number.
            }
            if !b_dig {
                return 1; // s2 run shorter → larger number.
            }
            if a != b && first_diff == 0 {
                // SAFETY: a and b are u8, so their i32 values are in [0, 255];
                // the difference is in [-255, 255] which cannot overflow i32.
                first_diff = i32::from(a).wrapping_sub(i32::from(b));
            }
            j = j.wrapping_add(1);
        }
    }
}
pub use gnu_strverscmp::strverscmp;

/// Reentrant string tokenizer.
///
/// Like `strtok`, but uses caller-provided `saveptr` instead of a
/// static variable, making it thread-safe.
///
/// # Safety
///
/// `s` (if non-null) and `delim` must be valid null-terminated strings.
/// `saveptr` must point to a valid `*mut u8`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn strtok_r(s: *mut u8, delim: *const u8, saveptr: *mut *mut u8) -> *mut u8 {
    if saveptr.is_null() {
        return core::ptr::null_mut();
    }

    let start = if s.is_null() {
        let p = unsafe { *saveptr };
        if p.is_null() {
            return core::ptr::null_mut();
        }
        p
    } else {
        s
    };

    // Skip leading delimiters.
    let mut i: usize = 0;
    loop {
        let c = unsafe { *start.add(i) };
        if c == 0 {
            unsafe {
                *saveptr = core::ptr::null_mut();
            }
            return core::ptr::null_mut();
        }
        if !unsafe { is_delim(c, delim) } {
            break;
        }
        i = i.wrapping_add(1);
    }

    let token = unsafe { start.add(i) };

    // Find end of token.
    let mut k: usize = 0;
    loop {
        let c = unsafe { *token.add(k) };
        if c == 0 {
            unsafe {
                *saveptr = core::ptr::null_mut();
            }
            return token;
        }
        if unsafe { is_delim(c, delim) } {
            unsafe {
                *token.add(k) = 0;
            }
            unsafe {
                *saveptr = token.add(k.wrapping_add(1));
            }
            return token;
        }
        k = k.wrapping_add(1);
    }
}

/// Copy bytes until a given byte is found, or `n` bytes have been copied.
///
/// Copies from `src` to `dest`, stopping after the first occurrence
/// of byte `c` (which IS copied), or after `n` bytes.  Returns a
/// pointer to the byte after `c` in `dest`, or NULL if `c` was not
/// found in the first `n` bytes.
///
/// # Safety
///
/// `dest` must be valid for `n` bytes.  `src` must be valid for `n` bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn memccpy(dest: *mut u8, src: *const u8, c: i32, n: usize) -> *mut u8 {
    // SAFETY: the caller's `n` bytes each side.  `memchr` reads within
    // them, and the copy is of those up to the byte it found, inclusive, or
    // all of them.
    unsafe {
        let found = memchr(src, c, n);
        if found.is_null() {
            memcpy(dest, src, n);
            return core::ptr::null_mut();
        }
        let len = (found as usize).wrapping_sub(src as usize).wrapping_add(1);
        memcpy(dest, src, len);
        dest.add(len)
    }
}

/// Locale-aware string comparison.
///
/// Since we don't have locale support, this is identical to `strcmp`.
///
/// # Safety
///
/// Both strings must be valid null-terminated strings.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn strcoll(s1: *const u8, s2: *const u8) -> i32 {
    unsafe { strcmp(s1, s2) }
}

/// Transform a string for locale-aware comparison.
///
/// Copies at most `n` bytes of `src` into `dest` in a form such that
/// `strcmp` on two transformed strings gives the same result as `strcoll`
/// on the originals.  Since we have no locale, this is just `strncpy`.
///
/// Returns the length of the transformed string (not counting null).
///
/// # Safety
///
/// `dest` must be valid for `n` bytes.  `src` must be null-terminated.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn strxfrm(dest: *mut u8, src: *const u8, n: usize) -> usize {
    let len = unsafe { strlen(src) };
    if n > 0 {
        unsafe {
            strncpy(dest, src, n);
        }
    }
    len
}

/// Thread-safe version of `strerror`.
///
/// Copies the error description into the user-provided buffer.
/// Returns 0 on success, or `ERANGE` if the buffer is too small.
///
/// # Safety
///
/// `buf` must be valid for `buflen` bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn strerror_r(errnum: i32, buf: *mut u8, buflen: usize) -> i32 {
    if buf.is_null() || buflen == 0 {
        return crate::errno::ERANGE;
    }

    let msg = strerror(errnum);
    let msg_len = unsafe { strlen(msg) };
    let copy_len = if msg_len < buflen {
        msg_len
    } else {
        buflen.wrapping_sub(1)
    };

    unsafe {
        memcpy(buf, msg, copy_len);
    }
    unsafe {
        *buf.add(copy_len) = 0;
    }

    if msg_len >= buflen {
        crate::errno::ERANGE
    } else {
        0
    }
}

/// Locale-aware string comparison (locale variant).
///
/// Since we only support the C locale, delegates to `strcmp`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn strcoll_l(s1: *const u8, s2: *const u8, _locale: usize) -> i32 {
    unsafe { strcmp(s1, s2) }
}

/// Transform a string for locale-aware comparison (locale variant).
///
/// Since we only support the C locale, delegates to `strxfrm`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn strxfrm_l(
    dest: *mut u8,
    src: *const u8,
    n: usize,
    _locale: usize,
) -> usize {
    unsafe { strxfrm(dest, src, n) }
}

/// Locale-aware `strerror`.
///
/// Returns the same result as `strerror` (locale is ignored).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn strerror_l(errnum: i32, _locale: usize) -> *const u8 {
    strerror(errnum)
}

/// Each error number's name, glibc's (`strerrorname_np`): the `E*` constant's.
/// Where two share a number -- `EWOULDBLOCK` and `EAGAIN`, `EDEADLOCK` and
/// `EDEADLK`, `ENOTSUP` and `EOPNOTSUPP` -- it is the name glibc answers
/// with; 41 and 58 are numbers Linux left unused, and 0 is glibc's "0".
static ERRNO_NAMES: [Option<&core::ffi::CStr>; 134] = [
    Some(c"0"),
    Some(c"EPERM"),
    Some(c"ENOENT"),
    Some(c"ESRCH"),
    Some(c"EINTR"),
    Some(c"EIO"),
    Some(c"ENXIO"),
    Some(c"E2BIG"),
    Some(c"ENOEXEC"),
    Some(c"EBADF"),
    Some(c"ECHILD"),
    Some(c"EAGAIN"),
    Some(c"ENOMEM"),
    Some(c"EACCES"),
    Some(c"EFAULT"),
    Some(c"ENOTBLK"),
    Some(c"EBUSY"),
    Some(c"EEXIST"),
    Some(c"EXDEV"),
    Some(c"ENODEV"),
    Some(c"ENOTDIR"),
    Some(c"EISDIR"),
    Some(c"EINVAL"),
    Some(c"ENFILE"),
    Some(c"EMFILE"),
    Some(c"ENOTTY"),
    Some(c"ETXTBSY"),
    Some(c"EFBIG"),
    Some(c"ENOSPC"),
    Some(c"ESPIPE"),
    Some(c"EROFS"),
    Some(c"EMLINK"),
    Some(c"EPIPE"),
    Some(c"EDOM"),
    Some(c"ERANGE"),
    Some(c"EDEADLK"),
    Some(c"ENAMETOOLONG"),
    Some(c"ENOLCK"),
    Some(c"ENOSYS"),
    Some(c"ENOTEMPTY"),
    Some(c"ELOOP"),
    None,
    Some(c"ENOMSG"),
    Some(c"EIDRM"),
    Some(c"ECHRNG"),
    Some(c"EL2NSYNC"),
    Some(c"EL3HLT"),
    Some(c"EL3RST"),
    Some(c"ELNRNG"),
    Some(c"EUNATCH"),
    Some(c"ENOCSI"),
    Some(c"EL2HLT"),
    Some(c"EBADE"),
    Some(c"EBADR"),
    Some(c"EXFULL"),
    Some(c"ENOANO"),
    Some(c"EBADRQC"),
    Some(c"EBADSLT"),
    None,
    Some(c"EBFONT"),
    Some(c"ENOSTR"),
    Some(c"ENODATA"),
    Some(c"ETIME"),
    Some(c"ENOSR"),
    Some(c"ENONET"),
    Some(c"ENOPKG"),
    Some(c"EREMOTE"),
    Some(c"ENOLINK"),
    Some(c"EADV"),
    Some(c"ESRMNT"),
    Some(c"ECOMM"),
    Some(c"EPROTO"),
    Some(c"EMULTIHOP"),
    Some(c"EDOTDOT"),
    Some(c"EBADMSG"),
    Some(c"EOVERFLOW"),
    Some(c"ENOTUNIQ"),
    Some(c"EBADFD"),
    Some(c"EREMCHG"),
    Some(c"ELIBACC"),
    Some(c"ELIBBAD"),
    Some(c"ELIBSCN"),
    Some(c"ELIBMAX"),
    Some(c"ELIBEXEC"),
    Some(c"EILSEQ"),
    Some(c"ERESTART"),
    Some(c"ESTRPIPE"),
    Some(c"EUSERS"),
    Some(c"ENOTSOCK"),
    Some(c"EDESTADDRREQ"),
    Some(c"EMSGSIZE"),
    Some(c"EPROTOTYPE"),
    Some(c"ENOPROTOOPT"),
    Some(c"EPROTONOSUPPORT"),
    Some(c"ESOCKTNOSUPPORT"),
    Some(c"EOPNOTSUPP"),
    Some(c"EPFNOSUPPORT"),
    Some(c"EAFNOSUPPORT"),
    Some(c"EADDRINUSE"),
    Some(c"EADDRNOTAVAIL"),
    Some(c"ENETDOWN"),
    Some(c"ENETUNREACH"),
    Some(c"ENETRESET"),
    Some(c"ECONNABORTED"),
    Some(c"ECONNRESET"),
    Some(c"ENOBUFS"),
    Some(c"EISCONN"),
    Some(c"ENOTCONN"),
    Some(c"ESHUTDOWN"),
    Some(c"ETOOMANYREFS"),
    Some(c"ETIMEDOUT"),
    Some(c"ECONNREFUSED"),
    Some(c"EHOSTDOWN"),
    Some(c"EHOSTUNREACH"),
    Some(c"EALREADY"),
    Some(c"EINPROGRESS"),
    Some(c"ESTALE"),
    Some(c"EUCLEAN"),
    Some(c"ENOTNAM"),
    Some(c"ENAVAIL"),
    Some(c"EISNAM"),
    Some(c"EREMOTEIO"),
    Some(c"EDQUOT"),
    Some(c"ENOMEDIUM"),
    Some(c"EMEDIUMTYPE"),
    Some(c"ECANCELED"),
    Some(c"ENOKEY"),
    Some(c"EKEYEXPIRED"),
    Some(c"EKEYREVOKED"),
    Some(c"EKEYREJECTED"),
    Some(c"EOWNERDEAD"),
    Some(c"ENOTRECOVERABLE"),
    Some(c"ERFKILL"),
    Some(c"EHWPOISON"),
];

/// `strerrorname_np(errnum)` -- the name of the constant an error number is,
/// `"EINVAL"` for 22; NULL for a number that is no error's (glibc 2.32's).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn strerrorname_np(errnum: i32) -> *const u8 {
    usize::try_from(errnum)
        .ok()
        .and_then(|i| ERRNO_NAMES.get(i).copied().flatten())
        .map_or(core::ptr::null(), |s| s.as_ptr().cast())
}

/// `strerrordesc_np(errnum)` -- [`strerror`]'s text for an error number, or
/// NULL, not "Unknown error", for a number that is no error's (glibc 2.32's).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn strerrordesc_np(errnum: i32) -> *const u8 {
    error_text(errnum).map_or(core::ptr::null(), |t| t.as_ptr().cast())
}

/// `memfrob(s, n)` -- each of the `n` bytes at `s` exclusive-ored with 42, in
/// place: glibc's joke of an encryption, its own inverse. Returns `s`.
///
/// # Safety
///
/// `s` must be valid for reading and writing `n` bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn memfrob(s: *mut core::ffi::c_void, n: SizeT) -> *mut core::ffi::c_void {
    let p = s.cast::<u8>();
    for i in 0..n {
        // SAFETY: `i < n`, within the caller's `n` bytes.
        unsafe { *p.add(i) ^= 42 };
    }
    s
}

/// `strfry(string)` -- the bytes of a string shuffled in place, each order
/// equally likely (Fisher and Yates, over [`crate::random::arc4random_uniform`]).
/// Returns `string`; NULL is left alone.
///
/// # Safety
///
/// `string` must be NULL or a writable NUL-terminated string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn strfry(string: *mut u8) -> *mut u8 {
    if string.is_null() {
        return string;
    }
    // SAFETY: the caller's NUL-terminated string.
    let len = unsafe { strlen(string) };
    let mut i = len;
    while i > 1 {
        // A string of more than 4 GiB is shuffled with its first 4 Gi
        // choices a little less even; there is no such string to fry.
        let bound = u32::try_from(i).unwrap_or(u32::MAX);
        let j = crate::random::arc4random_uniform(bound) as usize;
        i = i.wrapping_sub(1);
        // SAFETY: `i` and `j` are both below `len`.
        unsafe { core::ptr::swap(string.add(i), string.add(j)) };
    }
    string
}

/// XPG variant of `strerror_r`.
///
/// Some glibc-compiled programs reference `__xpg_strerror_r` instead
/// of the GNU-specific `strerror_r`.  The XPG version returns 0 on
/// success and an error code on failure (same as our `strerror_r`).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __xpg_strerror_r(errnum: i32, buf: *mut u8, buflen: usize) -> i32 {
    unsafe { strerror_r(errnum, buf, buflen) }
}

// ---------------------------------------------------------------------------
// BSD safe string functions
// ---------------------------------------------------------------------------

/// Copy a string with guaranteed NUL termination.
///
/// Copies up to `size - 1` bytes from `src` to `dst` and always
/// NUL-terminates (unless `size` is 0).  Returns the total length
/// of `src` (not including NUL) — if the return value >= `size`,
/// truncation occurred.
///
/// This is the BSD `strlcpy`, widely used as a safer `strncpy`.
///
/// # Safety
///
/// `dst` must be valid for `size` bytes.  `src` must be a valid
/// NUL-terminated string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn strlcpy(dst: *mut u8, src: *const u8, size: SizeT) -> SizeT {
    let src_len = unsafe { strlen(src) };

    if size > 0 {
        let copy_len = if src_len < size {
            src_len
        } else {
            size.wrapping_sub(1)
        };
        // SAFETY: dst valid for `size` bytes, src valid for src_len.
        unsafe {
            core::ptr::copy_nonoverlapping(src, dst, copy_len);
        }
        unsafe {
            *dst.add(copy_len) = 0;
        }
    }

    src_len
}

/// Append a string with guaranteed NUL termination.
///
/// Appends `src` to `dst`, writing at most `size - strlen(dst) - 1`
/// bytes.  Always NUL-terminates (unless `size <= strlen(dst)`).
/// Returns `strlen(dst) + strlen(src)` — if the return value >= `size`,
/// truncation occurred.
///
/// This is the BSD `strlcat`, widely used as a safer `strncat`.
///
/// # Safety
///
/// `dst` must be valid for `size` bytes and contain a NUL-terminated
/// string.  `src` must be a valid NUL-terminated string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn strlcat(dst: *mut u8, src: *const u8, size: SizeT) -> SizeT {
    let dst_len = unsafe { strnlen(dst, size) };
    let src_len = unsafe { strlen(src) };

    if dst_len >= size {
        // dst already fills the buffer — no room even for NUL.
        return size.wrapping_add(src_len);
    }

    let remaining = size.wrapping_sub(dst_len).wrapping_sub(1);
    let copy_len = if src_len < remaining {
        src_len
    } else {
        remaining
    };

    // SAFETY: dst_len < size, so dst.add(dst_len) is within bounds.
    unsafe {
        core::ptr::copy_nonoverlapping(src, dst.add(dst_len), copy_len);
        *dst.add(dst_len.wrapping_add(copy_len)) = 0;
    }

    dst_len.wrapping_add(src_len)
}

// ---------------------------------------------------------------------------
// strcasestr — case-insensitive substring search
// ---------------------------------------------------------------------------

/// Own archive member — gnulib replaces `strcasestr`. See the module header.
mod gnu_strcasestr {
    use super::*;

    /// Locate a case-insensitive substring.
    ///
    /// Returns a pointer to the first occurrence of `needle` in
    /// `haystack`, ignoring ASCII case differences.  Returns null if not
    /// found.  If `needle` is empty, returns `haystack`.  The Two-Way
    /// search over case-folded bytes: linear time, constant space.
    ///
    /// # Safety
    ///
    /// Both `haystack` and `needle` must be valid null-terminated strings.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub unsafe extern "C" fn strcasestr(haystack: *const u8, needle: *const u8) -> *mut u8 {
        if haystack.is_null() || needle.is_null() {
            return core::ptr::null_mut();
        }
        // SAFETY: the caller's strings.  The haystack is read only as far
        // as `in_string` admits, which is never past its terminator.
        unsafe {
            let nlen = strlen(needle);
            if nlen == 0 {
                return haystack.cast_mut();
            }
            // A haystack shorter than the needle holds no match.
            if length_within(haystack, nlen) < nlen {
                return core::ptr::null_mut();
            }
            TwoWay::new(needle, nlen, to_lower)
                .find(haystack, in_string(haystack))
                .cast_mut()
        }
    }
}
pub use gnu_strcasestr::strcasestr;

/// ASCII lowercase.
#[inline(always)]
fn to_lower(c: u8) -> u8 {
    if c.is_ascii_uppercase() {
        #[allow(clippy::arithmetic_side_effects)]
        return c | 0x20;
    }
    c
}

// ---------------------------------------------------------------------------
// explicit_bzero — guaranteed-not-optimized-away zeroing
// ---------------------------------------------------------------------------

/// Zero a memory region, guaranteed not to be optimized away.
///
/// Unlike `memset(s, 0, n)`, the compiler cannot elide this call even
/// if the buffer is not read afterward.  Used for clearing sensitive
/// data (passwords, keys) from memory.
///
/// # Safety
///
/// `s` must be valid for `n` bytes of writing.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn explicit_bzero(s: *mut u8, n: usize) {
    if s.is_null() || n == 0 {
        return;
    }
    // SAFETY: the caller's `n` writable bytes.
    unsafe {
        memset(s, 0, n);
    }
    // glibc's barrier: assembly the compiler must assume reads the zeroed
    // bytes through `s` (it declares no `nomem` or `readonly`), so the
    // `memset` before it is not a dead store it may drop.  Volatile byte
    // stores did the same at a byte a store.
    // SAFETY: the template is a comment; it executes nothing.
    unsafe {
        core::arch::asm!("/* {0} */", in(reg) s, options(nostack, preserves_flags));
    }
}

// ---------------------------------------------------------------------------
// mempcpy — copy with end-of-dest return
// ---------------------------------------------------------------------------

/// Own archive member — gnulib replaces `mempcpy`. See the module header.
mod gnu_mempcpy {
    use super::*;

    /// Copy `n` bytes from `src` to `dest`, returning a pointer past the
    /// last written byte.
    ///
    /// Like `memcpy` but returns `dest + n` instead of `dest`.  This is a
    /// GNU extension commonly used for efficient buffer building (chain
    /// multiple mempcpy calls without tracking the offset manually).
    ///
    /// # Safety
    ///
    /// `dest` and `src` must be valid for `n` bytes and must not overlap.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub unsafe extern "C" fn mempcpy(dest: *mut u8, src: *const u8, n: SizeT) -> *mut u8 {
        // SAFETY: the caller's `n` bytes each side; `dest + n` is one past
        // the end, valid pointer arithmetic.
        unsafe {
            memcpy(dest, src, n);
            dest.add(n)
        }
    }
}
pub use gnu_mempcpy::mempcpy;

// ---------------------------------------------------------------------------
// memmem — search for byte sequence in memory
// ---------------------------------------------------------------------------

/// Locate a byte sequence within a larger memory region.
///
/// Searches the first `haystacklen` bytes of `haystack` for the first
/// occurrence of the `needlelen`-byte sequence at `needle`.
///
/// Returns a pointer to the start of the match, or NULL if not found.
///
/// Edge cases (per POSIX / glibc):
/// - If `needlelen` is 0, returns `haystack` (empty pattern always matches).
/// - If `needlelen > haystacklen`, returns NULL.
///
/// A one-byte needle is `memchr`; a longer one the Two-Way search
/// ([`TwoWay`]): linear time, constant space.
///
/// # Safety
///
/// `haystack` must be valid for `haystacklen` bytes.
/// `needle` must be valid for `needlelen` bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn memmem(
    haystack: *const u8,
    haystacklen: SizeT,
    needle: *const u8,
    needlelen: SizeT,
) -> *const u8 {
    // Empty needle matches immediately.
    if needlelen == 0 {
        return haystack;
    }
    if haystack.is_null() || needle.is_null() || needlelen > haystacklen {
        return core::ptr::null();
    }
    // SAFETY: the caller's lengths, which `fits` holds the search to.
    unsafe {
        if needlelen == 1 {
            return memchr(haystack, i32::from(needle.read()), haystacklen);
        }
        TwoWay::new(needle, needlelen, exact).find(haystack, |end| end <= haystacklen)
    }
}

// ---------------------------------------------------------------------------
// rawmemchr — unbounded memchr (assumes byte is present)
// ---------------------------------------------------------------------------

/// Own archive member — gnulib replaces `rawmemchr`. See the module header.
mod gnu_rawmemchr {
    use super::*;

    /// Search for a byte in memory without a length bound.
    ///
    /// Like `memchr` but assumes the byte `c` WILL be found somewhere in
    /// the buffer.  This is a GNU extension used by glibc internals and
    /// some programs for efficiency when the caller guarantees the
    /// sentinel exists (e.g., searching for `'\0'` in a C string).
    ///
    /// # Safety
    ///
    /// `s` must point to memory that contains at least one occurrence of
    /// `c` (as the low byte of the int).  If `c` is not present, this
    /// function reads past the end of valid memory (undefined behavior).
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub unsafe extern "C" fn rawmemchr(s: *const u8, c: i32) -> *const u8 {
        let needle = splat(c as u8);
        let skip = (s as usize & 15) as u32;
        let mut block = s.wrapping_sub(skip as usize);
        // SAFETY: the caller vouches the byte occurs, so every byte up to
        // it is readable.  Each block read starts at or below one of those
        // not yet examined (the first holds `s`), and lies in its page.
        unsafe {
            let (_, hits) = block_masks(block, needle);
            let first = hits.wrapping_shr(skip);
            if first != 0 {
                return s.wrapping_add(first.trailing_zeros() as usize);
            }
            loop {
                block = block.wrapping_add(16);
                let (_, hits) = block_masks(block, needle);
                if hits != 0 {
                    return block.wrapping_add(hits.trailing_zeros() as usize);
                }
            }
        }
    }
}
pub use gnu_rawmemchr::rawmemchr;

// ---------------------------------------------------------------------------
// sys_errlist / sys_nerr — deprecated but widely referenced
// ---------------------------------------------------------------------------

/// Number of entries in `sys_errlist` (one past the highest defined errno).
///
/// Deprecated since POSIX.1-2001, removed in POSIX.1-2008, but many
/// programs and libraries still reference it for link compatibility.
/// The highest error number is 133 (`EHWPOISON`), so `sys_nerr` is 134.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static sys_nerr: i32 = 134;

/// Wrapper to make `*const u8` usable in a static array.
///
/// Raw pointers are not `Sync`, but our pointers all point to static
/// string literals with `'static` lifetime, so sharing is safe.
#[repr(transparent)]
pub struct SyncPtr(*const u8);

// SAFETY: All wrapped pointers point to static c-string literals
// that live for the entire program lifetime and are never mutated.
unsafe impl Sync for SyncPtr {}

/// Array of error message strings indexed by errno value.
///
/// `sys_errlist[n]` points to the same static string that `strerror(n)`
/// returns: [`error_text`]'s, built from it when this is compiled. An index
/// that is no error's -- 41 and 58 -- points to "Unknown error".
///
/// Deprecated since POSIX.1-2001 -- use `strerror()` instead -- and declared
/// by no header since glibc 2.32. Provided for link compatibility with
/// programs that reference the symbol. It was a second copy of the texts
/// until 2026-09-29, and had drifted: 15 said "Unknown error", and 132 and
/// 133 were past its end.
///
/// The `SyncPtr` wrapper is `repr(transparent)`, so the array's layout is
/// `[*const u8; 134]` exactly -- C code sees a plain pointer array.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
// The index is evaluated when this is compiled, where one out of bounds
// fails the build rather than panicking; `n < 134` besides.
#[allow(clippy::indexing_slicing)]
pub static sys_errlist: [SyncPtr; 134] = {
    const UNKNOWN: SyncPtr = SyncPtr(c"Unknown error".as_ptr().cast::<u8>());
    let mut table = [UNKNOWN; 134];
    let mut n = 0;
    while n < 134 {
        if let Some(text) = error_text(n as i32) {
            table[n] = SyncPtr(text.as_ptr().cast::<u8>());
        }
        n += 1;
    }
    table
};

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    /// The engines behind `memcpy`, `memmove`, `memset`, `memcmp`, `memchr`
    /// and `strlen`, against what byte loops answer: every size to 300 (the
    /// small paths, the boundary at `SMALL`, the 16-byte loops) and either
    /// side of each threshold, every alignment of source and destination,
    /// every overlap of a move in both directions, and the bytes around
    /// what was written left alone.
    #[allow(clippy::cast_possible_truncation, clippy::indexing_slicing)]
    mod engines {
        use super::super::{REP_THRESHOLD, SMALL, memchr, memcmp, memcpy, memmove, memset, strlen};

        fn sizes() -> Vec<usize> {
            let mut v: Vec<usize> = (0..=300).collect();
            for t in [SMALL, REP_THRESHOLD, 4096] {
                v.extend([t - 17, t - 16, t - 1, t, t + 1, t + 15, t + 16, t + 17]);
            }
            v.extend([10_000, 65_537]);
            v.sort_unstable();
            v.dedup();
            v
        }

        /// The alignments tried at size `n`: all pairs while it is small,
        /// a spread above.
        fn offsets(n: usize) -> Vec<(usize, usize)> {
            if n <= 96 {
                (0..16).flat_map(|a| (0..16).map(move |b| (a, b))).collect()
            } else {
                vec![(0, 0), (1, 0), (0, 1), (3, 7), (8, 8), (15, 1), (15, 15)]
            }
        }

        /// Bytes no two near neighbours of which are equal, high ones too.
        fn pattern(len: usize, seed: u8) -> Vec<u8> {
            (0..len)
                .map(|i| (i as u8).wrapping_mul(37).wrapping_add(seed) ^ ((i >> 8) as u8))
                .collect()
        }

        #[test]
        fn memcpy_is_a_byte_copy() {
            for n in sizes() {
                for (s, d) in offsets(n) {
                    let src = pattern(n + 16, 5);
                    let mut dst = vec![0xEE_u8; n + 48];
                    // SAFETY: `src` holds `s + n` bytes and `dst` `16 + d + n`.
                    let r = unsafe { memcpy(dst.as_mut_ptr().add(16 + d), src.as_ptr().add(s), n) };
                    assert_eq!(r, dst.as_mut_ptr().wrapping_add(16 + d));
                    assert_eq!(
                        &dst[16 + d..16 + d + n],
                        &src[s..s + n],
                        "n {n}, at {s}/{d}"
                    );
                    assert!(dst[..16 + d].iter().all(|&b| b == 0xEE), "n {n}: before");
                    assert!(dst[16 + d + n..].iter().all(|&b| b == 0xEE), "n {n}: after");
                }
            }
        }

        #[test]
        fn memmove_is_a_byte_move_every_way_it_overlaps() {
            for n in sizes() {
                let shifts: Vec<isize> = if n <= 300 {
                    (-40..=40).collect()
                } else {
                    vec![-40, -17, -16, -15, -1, 0, 1, 15, 16, 17, 40]
                };
                for shift in shifts {
                    let from = 48usize;
                    let to = from.wrapping_add_signed(shift);
                    let mut buf = pattern(n + 96, 11);
                    let mut want = buf.clone();
                    want.copy_within(from..from + n, to);
                    // SAFETY: `buf` holds `max(from, to) + n` bytes.
                    unsafe { memmove(buf.as_mut_ptr().add(to), buf.as_ptr().add(from), n) };
                    assert!(buf == want, "n {n}, shift {shift}");
                }
            }
            // Two separate buffers, as memcpy's case.
            let src = pattern(5000, 3);
            let mut dst = vec![0u8; 5000];
            // SAFETY: both hold 5000 bytes.
            unsafe { memmove(dst.as_mut_ptr(), src.as_ptr(), 5000) };
            assert_eq!(dst, src);
        }

        #[test]
        fn memset_fills_with_the_low_byte_of_c() {
            for n in sizes() {
                for (d, _) in offsets(n).into_iter().filter(|&(_, b)| b == 0 || b == 7) {
                    for (c, byte) in [
                        (0, 0u8),
                        (0x5A, 0x5A),
                        (0xFF, 0xFF),
                        (-1, 0xFF),
                        (0x15A, 0x5A),
                    ] {
                        let mut buf = vec![0xEE_u8; n + 48];
                        // SAFETY: `buf` holds `16 + d + n` bytes.
                        let r = unsafe { memset(buf.as_mut_ptr().add(16 + d), c, n) };
                        assert_eq!(r, buf.as_mut_ptr().wrapping_add(16 + d));
                        assert!(
                            buf[16 + d..16 + d + n].iter().all(|&b| b == byte),
                            "n {n}, c {c}"
                        );
                        assert!(buf[..16 + d].iter().all(|&b| b == 0xEE), "n {n}: before");
                        assert!(buf[16 + d + n..].iter().all(|&b| b == 0xEE), "n {n}: after");
                    }
                }
            }
        }

        #[test]
        fn memcmp_answers_the_first_difference_as_unsigned_bytes() {
            for n in sizes().into_iter().filter(|&n| n <= 4200) {
                let a = pattern(n + 16, 9);
                for (s, d) in offsets(n).into_iter().take(9) {
                    let mut b = vec![0u8; n + 16];
                    b[d..d + n].copy_from_slice(&a[s..s + n]);
                    let (pa, pb) = (a.as_ptr().wrapping_add(s), b.as_ptr().wrapping_add(d));
                    // SAFETY: both hold `n` bytes from these offsets.
                    assert_eq!(unsafe { memcmp(pa, pb, n) }, 0, "n {n}: equal");
                    let mut at: Vec<usize> = if n <= 64 {
                        (0..n).collect()
                    } else {
                        vec![0, 1, 15, 16, 17, n / 2, n - 17, n - 16, n - 1]
                    };
                    at.dedup();
                    for p in at {
                        for (x, y) in [(0x00u8, 0xFFu8), (0x80, 0x7F), (0x41, 0x42), (0xFE, 0x01)] {
                            let mut a2 = a[s..s + n].to_vec();
                            let mut b2 = a2.clone();
                            a2[p] = x;
                            b2[p] = y;
                            // A later difference the other way does not count.
                            if p + 1 < n {
                                a2[p + 1] = 0x00;
                                b2[p + 1] = 0xFF;
                            }
                            // SAFETY: both hold `n` bytes.
                            let got = unsafe { memcmp(a2.as_ptr(), b2.as_ptr(), n) };
                            assert_eq!(got, i32::from(x) - i32::from(y), "n {n}, at {p}");
                        }
                    }
                }
            }
        }

        #[test]
        fn memchr_finds_the_first_and_reads_no_further_than_n() {
            for n in sizes().into_iter().filter(|&n| n <= 4200) {
                for (s, _) in offsets(n).into_iter().take(5) {
                    // A buffer with no 0x80 in it, and the needle placed.
                    let mut buf: Vec<u8> = pattern(n + 32, 1)
                        .into_iter()
                        .map(|b| if b == 0x80 { 0x81 } else { b })
                        .collect();
                    let base = buf.as_ptr().wrapping_add(s);
                    // SAFETY: `buf` holds `s + n` bytes.
                    assert!(unsafe { memchr(base, 0x80, n) }.is_null(), "n {n}: absent");
                    let at: Vec<usize> = if n <= 64 {
                        (0..n).collect()
                    } else {
                        vec![0, 15, 16, 17, n / 2, n - 1]
                    };
                    for p in at {
                        buf[s + p] = 0x80;
                        if p + 3 < n {
                            buf[s + p + 3] = 0x80; // a later one does not count
                        }
                        let base = buf.as_ptr().wrapping_add(s);
                        for c in [0x80, 0x180, -128] {
                            // SAFETY: as above.
                            let got = unsafe { memchr(base, c, n) };
                            assert_eq!(got, base.wrapping_add(p), "n {n}, at {p}, c {c}");
                        }
                        buf[s + p] = 0x81;
                        if p + 3 < n {
                            buf[s + p + 3] = 0x81;
                        }
                    }
                    // Just past `n` is not looked at.
                    buf[s + n] = 0x80;
                    let base = buf.as_ptr().wrapping_add(s);
                    // SAFETY: as above.
                    assert!(
                        unsafe { memchr(base, 0x80, n) }.is_null(),
                        "n {n}: past the end"
                    );
                }
            }
        }

        #[test]
        fn strlen_counts_to_the_terminator_at_any_alignment() {
            let mut lengths: Vec<usize> = (0..=200).collect();
            lengths.extend([255, 256, 257, 4095, 4096, 4097, 70_000]);
            for len in lengths {
                for start in 0..48usize {
                    if len > 300 && start % 7 != 0 {
                        continue;
                    }
                    let mut buf: Vec<u8> = (0..start + len + 64)
                        .map(|i| 0x80 | (i as u8) | 1)
                        .collect();
                    buf[start + len] = 0;
                    // SAFETY: a NUL-terminated string at `start`.
                    assert_eq!(
                        unsafe { strlen(buf.as_ptr().add(start)) },
                        len,
                        "len {len}, at {start}"
                    );
                }
            }
        }
    }

    /// The scanners and what is built on them, against byte loops: every
    /// alignment, every length to past several blocks, the byte sought at
    /// every place, and every needle and haystack over small alphabets.
    /// Then (`guarded`) each with its strings against pages that fault, to
    /// show it reads nothing outside its strings' pages.
    mod scanners {
        use super::super::{
            memccpy, memchr, memcmp, memcpy, memmem, mempcpy, memrchr, memset, rawmemchr, stpcpy,
            stpncpy, strcasestr, strcat, strchr, strchrnul, strcmp, strcpy, strcspn, strlen,
            strncat, strncmp, strncpy, strnlen, strpbrk, strrchr, strspn, strstr,
        };

        /// The bytes the searches look for, a low one and a high one;
        /// [`filler`] never holds either.
        const SOUGHT: [u8; 2] = [b'q', 0xE9];

        /// What surrounds a placed string: neither a terminator nor sought.
        const FILL: u8 = b'Z';

        /// Source and destination alignments for the copies.
        const ALIGNS: [(usize, usize); 7] =
            [(0, 0), (1, 0), (0, 1), (3, 9), (15, 15), (15, 1), (8, 8)];

        /// `len` bytes, none 0 or sought, low and high, varying with `seed`.
        fn filler(len: usize, seed: usize) -> Vec<u8> {
            (0..len)
                .map(|i| {
                    let b = ((i * 37 + seed * 11) % 250) as u8 + 1;
                    if SOUGHT.contains(&b) { b + 1 } else { b }
                })
                .collect()
        }

        /// `bytes` and a terminator.
        fn terminated(bytes: &[u8]) -> Vec<u8> {
            let mut v = bytes.to_vec();
            v.push(0);
            v
        }

        /// `bytes` at `align` past a 16-byte boundary, [`FILL`] around
        /// them: the buffer, and where they start in it.
        fn place(bytes: &[u8], align: usize) -> (Vec<u8>, usize) {
            let mut buf = vec![FILL; bytes.len() + 96];
            let at = (16 - buf.as_ptr() as usize % 16) % 16 + align;
            buf[at..at + bytes.len()].copy_from_slice(bytes);
            (buf, at)
        }

        /// Room for `room` bytes at `align` past a 16-byte boundary, 0xEE
        /// throughout and around: the buffer, and where the room starts.
        fn destination(room: usize, align: usize) -> (Vec<u8>, usize) {
            let buf = vec![0xEE; room + 96];
            let at = (16 - buf.as_ptr() as usize % 16) % 16 + align;
            (buf, at)
        }

        /// That nothing of `buf` outside `written` changed from 0xEE.
        fn untouched_outside(buf: &[u8], written: core::ops::Range<usize>, what: &str) {
            assert!(
                buf[..written.start].iter().all(|&b| b == 0xEE),
                "{what}: before"
            );
            assert!(
                buf[written.end..].iter().all(|&b| b == 0xEE),
                "{what}: after"
            );
        }

        /// How far `found` is from `base`, or `None` for null.
        fn offset(found: *const u8, base: *const u8) -> Option<usize> {
            (!found.is_null()).then(|| found as usize - base as usize)
        }

        #[test]
        fn strnlen_counts_to_the_terminator_or_the_limit() {
            for len in 0..=80 {
                let s = terminated(&filler(len, len));
                for align in 0..16 {
                    let (buf, at) = place(&s, align);
                    let p = buf.as_ptr().wrapping_add(at);
                    let limits = [0, 1, 2, 15, 16, 17, 31, 32, 33, 48, 64, 100, usize::MAX];
                    for max in limits
                        .into_iter()
                        .chain([len.saturating_sub(1), len, len + 1])
                    {
                        // SAFETY: a terminated string.
                        let got = unsafe { strnlen(p, max) };
                        assert_eq!(got, len.min(max), "len {len}, align {align}, max {max}");
                    }
                }
            }
        }

        #[test]
        fn strnlen_of_a_run_with_no_terminator_is_its_limit() {
            for max in 0..=80 {
                let run = filler(max, 3);
                for align in 0..16 {
                    let (buf, at) = place(&run, align);
                    // SAFETY: `max` readable bytes.
                    let got = unsafe { strnlen(buf.as_ptr().add(at), max) };
                    assert_eq!(got, max, "max {max}, align {align}");
                }
            }
        }

        #[test]
        fn strchr_strchrnul_and_rawmemchr_find_the_first() {
            for len in 0..=70 {
                for align in 0..16 {
                    for sought in SOUGHT {
                        let c = i32::from(sought);
                        for first in 0..=len {
                            let mut s = filler(len, align);
                            if first < len {
                                s[first] = sought;
                                if first + 5 < len {
                                    s[first + 5] = sought;
                                }
                            }
                            let (buf, at) = place(&terminated(&s), align);
                            let p = buf.as_ptr().wrapping_add(at);
                            let found = (first < len).then_some(first);
                            let what = format!("len {len}, align {align}, {sought:#x} at {first}");
                            // SAFETY: a terminated string; `rawmemchr` is
                            // asked only for bytes in it.
                            unsafe {
                                assert_eq!(offset(strchr(p, c), p), found, "{what}");
                                // C passes a `char` as an int, negative for
                                // a high byte where `char` is signed.
                                assert_eq!(
                                    offset(strchr(p, c - 256), p),
                                    found,
                                    "{what}: as negative"
                                );
                                assert_eq!(
                                    offset(strchrnul(p, c), p),
                                    Some(found.unwrap_or(len)),
                                    "{what}"
                                );
                                if let Some(first) = found {
                                    assert_eq!(offset(rawmemchr(p, c), p), Some(first), "{what}");
                                }
                                assert_eq!(
                                    offset(strchr(p, 0), p),
                                    Some(len),
                                    "{what}: the terminator"
                                );
                                assert_eq!(offset(strchrnul(p, 0), p), Some(len), "{what}");
                                assert_eq!(offset(rawmemchr(p, 0), p), Some(len), "{what}");
                            }
                        }
                    }
                }
            }
        }

        #[test]
        fn strrchr_finds_the_last() {
            for len in 0..=70 {
                for align in 0..16 {
                    for sought in SOUGHT {
                        for first in 0..=len {
                            for gap in [0, 1, 17] {
                                let mut s = filler(len, align + 1);
                                let mut last = None;
                                if first < len {
                                    s[first] = sought;
                                    last = Some(first);
                                    if gap > 0 && first + gap < len {
                                        s[first + gap] = sought;
                                        last = Some(first + gap);
                                    }
                                }
                                let (buf, at) = place(&terminated(&s), align);
                                let p = buf.as_ptr().wrapping_add(at);
                                // SAFETY: a terminated string.
                                unsafe {
                                    assert_eq!(
                                        offset(strrchr(p, i32::from(sought)), p),
                                        last,
                                        "len {len}, align {align}, {sought:#x} at {first} and +{gap}"
                                    );
                                    assert_eq!(offset(strrchr(p, 0), p), Some(len), "len {len}");
                                }
                            }
                        }
                    }
                }
            }
        }

        #[test]
        fn memrchr_finds_the_last_within_its_bytes() {
            let sought = SOUGHT[0];
            for n in 0..=70 {
                for align in 0..16 {
                    for last in 0..=n {
                        // The byte sought just before `s` and from `n` on,
                        // where it must not be found.
                        let mut bytes = vec![sought; 16];
                        bytes.extend(filler(n, align));
                        bytes.extend([sought; 16]);
                        if last < n {
                            bytes[16 + last] = sought;
                            if last >= 3 {
                                bytes[16 + last - 3] = sought;
                            }
                        }
                        let (buf, at) = place(&bytes, align);
                        let s = buf.as_ptr().wrapping_add(at + 16);
                        // SAFETY: `n` readable bytes from `s`.
                        let got = unsafe { memrchr(s, i32::from(sought), n) };
                        assert_eq!(
                            offset(got, s),
                            (last < n).then_some(last),
                            "n {n}, align {align}, last {last}"
                        );
                    }
                }
            }
        }

        /// `strncmp` as a byte loop over terminated strings.
        fn byte_strncmp(a: &[u8], b: &[u8], n: usize) -> i32 {
            for (&x, &y) in a.iter().zip(b).take(n) {
                if x != y || x == 0 {
                    return i32::from(x) - i32::from(y);
                }
            }
            0
        }

        /// Pairs to compare: equal; differing at each place, by low and
        /// high bytes; and one a prefix of the other.
        fn comparison_pairs() -> Vec<(Vec<u8>, Vec<u8>)> {
            let mut pairs = Vec::new();
            for len in 0..=40 {
                let base = filler(len, len);
                pairs.push((base.clone(), base.clone()));
                for d in 0..len {
                    for v in [0x01, 0x7F, 0x80, 0xFF] {
                        if v != base[d] {
                            let mut other = base.clone();
                            other[d] = v;
                            pairs.push((base.clone(), other));
                        }
                    }
                    pairs.push((base.clone(), base[..d].to_vec()));
                }
            }
            pairs
        }

        #[test]
        fn strcmp_and_strncmp_answer_as_byte_loops() {
            for (a, b) in comparison_pairs() {
                for (x, y) in [(&a, &b), (&b, &a)] {
                    let (x, y) = (terminated(x), terminated(y));
                    for (ax, ay) in [(0, 0), (1, 0), (0, 1), (5, 11), (15, 15), (15, 0), (8, 3)] {
                        let (bx, px) = place(&x, ax);
                        let (by, py) = place(&y, ay);
                        let (p, q) = (bx.as_ptr().wrapping_add(px), by.as_ptr().wrapping_add(py));
                        // SAFETY: terminated strings.
                        unsafe {
                            assert_eq!(
                                strcmp(p, q),
                                byte_strncmp(&x, &y, usize::MAX),
                                "{x:?} {y:?}"
                            );
                            for n in [
                                0,
                                1,
                                15,
                                16,
                                17,
                                31,
                                32,
                                33,
                                x.len() - 1,
                                x.len(),
                                usize::MAX,
                            ] {
                                assert_eq!(
                                    strncmp(p, q, n),
                                    byte_strncmp(&x, &y, n),
                                    "{x:?} {y:?}, n {n}"
                                );
                            }
                        }
                    }
                }
            }
        }

        /// A buffer of three pages' size and the offset in it of a page
        /// boundary with a page on either side.
        fn straddling() -> (Vec<u8>, usize) {
            let buf = vec![FILL; 3 * 4096];
            let boundary = (4096 - buf.as_ptr() as usize % 4096) % 4096 + 4096;
            (buf, boundary)
        }

        /// The byte-at-a-time path, where sixteen bytes would cross a page.
        #[test]
        fn strcmp_and_strncmp_across_a_page_boundary() {
            let base = filler(60, 9);
            for k1 in 1..=40 {
                for k2 in [1, 2, 15, 16, 17, 33, 40] {
                    for d in [None, Some(0), Some(k1 - 1), Some(k1), Some(k2), Some(59)] {
                        let x = terminated(&base);
                        let mut y = base.clone();
                        if let Some(d) = d {
                            y[d] = if y[d] == 0xFF { 0x01 } else { 0xFF };
                        }
                        let y = terminated(&y);
                        let ((mut bx, ox), (mut by, oy)) = (straddling(), straddling());
                        let (sx, sy) = (ox - k1, oy - k2);
                        bx[sx..sx + x.len()].copy_from_slice(&x);
                        by[sy..sy + y.len()].copy_from_slice(&y);
                        let (p, q) = (bx.as_ptr().wrapping_add(sx), by.as_ptr().wrapping_add(sy));
                        let what = format!("{k1} and {k2} before the boundary, differing at {d:?}");
                        // SAFETY: terminated strings.
                        unsafe {
                            assert_eq!(strcmp(p, q), byte_strncmp(&x, &y, usize::MAX), "{what}");
                            assert_eq!(strcmp(q, p), byte_strncmp(&y, &x, usize::MAX), "{what}");
                            for n in [k1, k2, 30, 61, usize::MAX] {
                                assert_eq!(
                                    strncmp(p, q, n),
                                    byte_strncmp(&x, &y, n),
                                    "{what}, n {n}"
                                );
                            }
                        }
                    }
                }
            }
        }

        #[test]
        fn strcpy_and_stpcpy_copy_the_string_and_its_terminator() {
            for len in 0..=70 {
                let src = terminated(&filler(len, 1));
                for (sa, da) in ALIGNS {
                    let (sbuf, sat) = place(&src, sa);
                    let s = sbuf.as_ptr().wrapping_add(sat);
                    for stp in [false, true] {
                        let (mut dbuf, dat) = destination(len + 1, da);
                        let d = dbuf.as_mut_ptr().wrapping_add(dat);
                        // SAFETY: a terminated string, and room for it.
                        let r = unsafe { if stp { stpcpy(d, s) } else { strcpy(d, s) } };
                        let what = format!("len {len}, at {sa}/{da}, stpcpy {stp}");
                        assert_eq!(r, if stp { d.wrapping_add(len) } else { d }, "{what}");
                        assert_eq!(&dbuf[dat..=dat + len], &src[..], "{what}");
                        untouched_outside(&dbuf, dat..dat + len + 1, &what);
                    }
                }
            }
        }

        #[test]
        fn strncpy_and_stpncpy_copy_then_pad_to_n() {
            for len in 0..=70 {
                let src = terminated(&filler(len, 2));
                for (sa, da) in ALIGNS {
                    let (sbuf, sat) = place(&src, sa);
                    let s = sbuf.as_ptr().wrapping_add(sat);
                    for n in [0, 1, len.saturating_sub(1), len, len + 1, len + 20] {
                        for stp in [false, true] {
                            let (mut dbuf, dat) = destination(n, da);
                            let d = dbuf.as_mut_ptr().wrapping_add(dat);
                            // SAFETY: a terminated string, and `n` bytes of room.
                            let r = unsafe {
                                if stp {
                                    stpncpy(d, s, n)
                                } else {
                                    strncpy(d, s, n)
                                }
                            };
                            let copied = len.min(n);
                            let what = format!("len {len}, n {n}, at {sa}/{da}, stpncpy {stp}");
                            assert_eq!(r, if stp { d.wrapping_add(copied) } else { d }, "{what}");
                            assert_eq!(&dbuf[dat..dat + copied], &src[..copied], "{what}");
                            assert!(
                                dbuf[dat + copied..dat + n].iter().all(|&b| b == 0),
                                "{what}: padding"
                            );
                            untouched_outside(&dbuf, dat..dat + n, &what);
                        }
                    }
                }
            }
        }

        #[test]
        fn strcat_and_strncat_append_and_terminate() {
            for len in 0..=40 {
                let src = terminated(&filler(len, 4));
                for prefix_len in [0, 1, 5, 16, 17] {
                    let prefix = filler(prefix_len, 9);
                    for (sa, da) in ALIGNS {
                        let (sbuf, sat) = place(&src, sa);
                        let s = sbuf.as_ptr().wrapping_add(sat);
                        for n in [
                            None,
                            Some(0),
                            Some(1),
                            Some(len.saturating_sub(1)),
                            Some(len),
                            Some(len + 5),
                        ] {
                            let (mut dbuf, dat) = destination(prefix_len + len + 1, da);
                            dbuf[dat..dat + prefix_len].copy_from_slice(&prefix);
                            dbuf[dat + prefix_len] = 0;
                            let d = dbuf.as_mut_ptr().wrapping_add(dat);
                            // SAFETY: terminated strings, and room for both.
                            let r = unsafe {
                                match n {
                                    None => strcat(d, s),
                                    Some(n) => strncat(d, s, n),
                                }
                            };
                            let what =
                                format!("len {len}, prefix {prefix_len}, n {n:?}, at {sa}/{da}");
                            assert_eq!(r, d, "{what}");
                            let mut want = prefix.clone();
                            want.extend_from_slice(&src[..n.map_or(len, |n| len.min(n))]);
                            want.push(0);
                            assert_eq!(&dbuf[dat..dat + want.len()], &want[..], "{what}");
                            untouched_outside(&dbuf, dat..dat + want.len(), &what);
                        }
                    }
                }
            }
        }

        #[test]
        fn mempcpy_and_memccpy_copy_and_point_past() {
            let stop_byte = SOUGHT[1];
            for n in 0..=70 {
                for (sa, da) in ALIGNS {
                    for stop in (0..n).step_by(3).chain([n]) {
                        let mut bytes = filler(n, 5);
                        if stop < n {
                            bytes[stop] = stop_byte;
                        }
                        let (sbuf, sat) = place(&bytes, sa);
                        let s = sbuf.as_ptr().wrapping_add(sat);
                        let what = format!("n {n}, stop {stop}, at {sa}/{da}");

                        let (mut dbuf, dat) = destination(n, da);
                        let d = dbuf.as_mut_ptr().wrapping_add(dat);
                        // SAFETY: `n` bytes each side.
                        let r = unsafe { mempcpy(d, s, n) };
                        assert_eq!(r, d.wrapping_add(n), "{what}");
                        assert_eq!(&dbuf[dat..dat + n], &bytes[..], "{what}");
                        untouched_outside(&dbuf, dat..dat + n, &what);

                        for c in [i32::from(stop_byte), i32::from(stop_byte) - 256] {
                            let (mut dbuf, dat) = destination(n, da);
                            let d = dbuf.as_mut_ptr().wrapping_add(dat);
                            // SAFETY: `n` bytes each side.
                            let r = unsafe { memccpy(d, s, c, n) };
                            let copied = if stop < n { stop + 1 } else { n };
                            assert_eq!(
                                offset(r.cast_const(), d),
                                (stop < n).then_some(stop + 1),
                                "{what}"
                            );
                            assert_eq!(&dbuf[dat..dat + copied], &bytes[..copied], "{what}");
                            untouched_outside(&dbuf, dat..dat + copied, &what);
                        }
                    }
                }
            }
        }

        /// Every string over `alphabet` up to `max_len` long.
        fn all_strings(alphabet: &[u8], max_len: usize) -> Vec<Vec<u8>> {
            let mut all = vec![Vec::new()];
            let mut longest: Vec<Vec<u8>> = vec![Vec::new()];
            for _ in 0..max_len {
                longest = longest
                    .iter()
                    .flat_map(|s| {
                        alphabet.iter().map(move |&c| {
                            let mut t = s.clone();
                            t.push(c);
                            t
                        })
                    })
                    .collect();
                all.extend(longest.iter().cloned());
            }
            all
        }

        #[test]
        fn strspn_strcspn_and_strpbrk_answer_as_byte_loops() {
            let every: Vec<u8> = (1..=255).collect();
            let sets: [&[u8]; 8] = [
                b"",
                b"a",
                b"ab",
                b"a ,\x01",
                b"\x80\xff",
                b"\x7f\x80",
                b"zyxwvutsrqponmlkjihgfedcba",
                &every,
            ];
            let alphabet = b"ab ,\x01\x7f\x80\xffZ";
            let mut strings = all_strings(alphabet, 3);
            for k in 0..alphabet.len() {
                for len in [17, 40, 100] {
                    strings.push(
                        (0..len)
                            .map(|i| alphabet[(i * 7 + k) % alphabet.len()])
                            .collect(),
                    );
                }
            }
            for set in sets {
                if !set.is_empty() {
                    // A long run of members, then one that is not.
                    let mut run: Vec<u8> = set.iter().copied().cycle().take(70).collect();
                    run.push(if set.contains(&b'Q') { b'q' } else { b'Q' });
                    strings.push(run);
                }
            }
            for set in sets {
                let set_c = terminated(set);
                for s in &strings {
                    let sc = terminated(s);
                    let spn = s.iter().take_while(|b| set.contains(b)).count();
                    let cspn = s.iter().take_while(|b| !set.contains(b)).count();
                    let what = format!("{s:?} against {set:?}");
                    // SAFETY: terminated strings.
                    unsafe {
                        assert_eq!(strspn(sc.as_ptr(), set_c.as_ptr()), spn, "strspn {what}");
                        assert_eq!(strcspn(sc.as_ptr(), set_c.as_ptr()), cspn, "strcspn {what}");
                        assert_eq!(
                            offset(strpbrk(sc.as_ptr(), set_c.as_ptr()), sc.as_ptr()),
                            (cspn < s.len()).then_some(cspn),
                            "strpbrk {what}"
                        );
                    }
                }
            }
        }

        /// A string as the searches take it: the bytes, terminated, and
        /// terminated with every `k`th letter upper-case (to which
        /// `strcasestr` is blind).
        struct Text {
            raw: Vec<u8>,
            c: Vec<u8>,
            mixed: Vec<u8>,
        }

        impl Text {
            fn new(raw: &[u8], k: usize) -> Self {
                let c = terminated(raw);
                let mixed = c
                    .iter()
                    .enumerate()
                    .map(|(i, &b)| {
                        if i % k == 0 {
                            b.to_ascii_uppercase()
                        } else {
                            b
                        }
                    })
                    .collect();
                Self {
                    raw: raw.to_vec(),
                    c,
                    mixed,
                }
            }
        }

        /// `strstr`, `memmem` and `strcasestr` on one pair, against trying
        /// every place.
        fn check_search(hay: &Text, needle: &Text) {
            let want = if needle.raw.is_empty() {
                Some(0)
            } else {
                hay.raw
                    .windows(needle.raw.len())
                    .position(|w| w == &needle.raw[..])
            };
            let (h, n) = (&hay.raw, &needle.raw);
            // SAFETY: terminated strings, and the slices' lengths.
            unsafe {
                assert_eq!(
                    offset(strstr(hay.c.as_ptr(), needle.c.as_ptr()), hay.c.as_ptr()),
                    want,
                    "strstr {h:?} {n:?}"
                );
                assert_eq!(
                    offset(memmem(h.as_ptr(), h.len(), n.as_ptr(), n.len()), h.as_ptr()),
                    want,
                    "memmem {h:?} {n:?}"
                );
                assert_eq!(
                    offset(
                        strcasestr(hay.mixed.as_ptr(), needle.mixed.as_ptr()).cast_const(),
                        hay.mixed.as_ptr()
                    ),
                    want,
                    "strcasestr {h:?} {n:?}"
                );
            }
        }

        #[test]
        fn the_two_way_searches_agree_with_trying_every_place() {
            for (alphabet, needles, hays) in [(&b"ab"[..], 6, 11), (&b"abc"[..], 4, 7)] {
                let needles: Vec<Text> = all_strings(alphabet, needles)
                    .iter()
                    .map(|s| Text::new(s, 2))
                    .collect();
                let hays: Vec<Text> = all_strings(alphabet, hays)
                    .iter()
                    .map(|s| Text::new(s, 3))
                    .collect();
                for needle in &needles {
                    for hay in &hays {
                        check_search(hay, needle);
                    }
                }
            }
        }

        /// Needles to forty bytes, mostly repetitions of a short word (the
        /// periodic needles, where the search's memory matters), some with
        /// a byte changed, in haystacks of the same word, some holding the
        /// needle.
        #[test]
        fn the_two_way_searches_agree_on_long_periodic_needles() {
            let mut state = 0x9E37_79B9_7F4A_7C15_u64;
            let mut next = move |below: usize| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                (state % below as u64) as usize
            };
            for _ in 0..4000 {
                let word: Vec<u8> = (0..next(5) + 1).map(|_| b"ab"[next(2)]).collect();
                let needle_len = next(40) + 1;
                let mut needle: Vec<u8> = word.iter().copied().cycle().take(needle_len).collect();
                if next(2) == 0 {
                    let at = next(needle_len);
                    needle[at] = b"abc"[next(3)];
                }
                let hay_len = next(200);
                let mut hay: Vec<u8> = word.iter().copied().cycle().take(hay_len).collect();
                if next(3) == 0 && hay_len >= needle_len {
                    let at = next(hay_len - needle_len + 1);
                    hay[at..at + needle_len].copy_from_slice(&needle);
                }
                check_search(&Text::new(&hay, 3), &Text::new(&needle, 2));
            }
        }

        /// `a`x5000 `b` in `a`x200000: what a search trying every place does
        /// worst, a billion comparisons.  Two-Way's are a few hundred
        /// thousand, so this finishes at once.
        #[test]
        fn a_long_periodic_needle_takes_linear_time() {
            let mut hay = vec![b'a'; 200_000];
            let mut needle = vec![b'a'; 5_000];
            needle.push(b'b');
            check_search(&Text::new(&hay, 3), &Text::new(&needle, 2));
            hay.push(b'b');
            let (hay, needle) = (Text::new(&hay, 3), Text::new(&needle, 2));
            check_search(&hay, &needle);
            // SAFETY: terminated strings.
            let found = unsafe { strstr(hay.c.as_ptr(), needle.c.as_ptr()) };
            assert_eq!(offset(found, hay.c.as_ptr()), Some(195_000));
        }

        /// A readable, writable page between two that fault, from the host,
        /// and the functions run with their strings against its edges.
        #[cfg(any(windows, target_os = "linux"))]
        mod guarded {
            use super::*;

            /// The host's page size, which the faulting pages are made of.
            const HOST_PAGE: usize = 4096;

            #[cfg(windows)]
            mod host {
                use super::HOST_PAGE;
                use core::ffi::c_void;

                const MEM_COMMIT: u32 = 0x1000;
                const MEM_RESERVE: u32 = 0x2000;
                const MEM_RELEASE: u32 = 0x8000;
                const PAGE_NOACCESS: u32 = 0x01;
                const PAGE_READWRITE: u32 = 0x04;

                #[link(name = "kernel32")]
                unsafe extern "system" {
                    fn VirtualAlloc(
                        address: *mut c_void,
                        size: usize,
                        kind: u32,
                        protect: u32,
                    ) -> *mut c_void;
                    fn VirtualFree(address: *mut c_void, size: usize, kind: u32) -> i32;
                }

                /// Three pages of address space, the middle one alone
                /// usable.
                pub(super) fn map() -> *mut u8 {
                    // SAFETY: fresh address space reserved, then its middle
                    // page committed.
                    unsafe {
                        let region = VirtualAlloc(
                            core::ptr::null_mut(),
                            3 * HOST_PAGE,
                            MEM_RESERVE,
                            PAGE_NOACCESS,
                        );
                        assert!(!region.is_null(), "VirtualAlloc reserve");
                        let middle = region.cast::<u8>().add(HOST_PAGE).cast();
                        let committed = VirtualAlloc(middle, HOST_PAGE, MEM_COMMIT, PAGE_READWRITE);
                        assert!(!committed.is_null(), "VirtualAlloc commit");
                        region.cast()
                    }
                }

                pub(super) fn unmap(region: *mut u8) {
                    // SAFETY: what `map` reserved, released whole (size 0,
                    // as MEM_RELEASE requires).
                    let released = unsafe { VirtualFree(region.cast(), 0, MEM_RELEASE) };
                    assert_ne!(released, 0, "VirtualFree");
                }
            }

            #[cfg(target_os = "linux")]
            mod host {
                use super::HOST_PAGE;
                use core::ffi::c_void;

                const PROT_NONE: i32 = 0;
                const PROT_READ: i32 = 1;
                const PROT_WRITE: i32 = 2;
                const MAP_PRIVATE: i32 = 0x02;
                const MAP_ANONYMOUS: i32 = 0x20;

                unsafe extern "C" {
                    fn mmap(
                        address: *mut c_void,
                        len: usize,
                        prot: i32,
                        flags: i32,
                        fd: i32,
                        offset: i64,
                    ) -> *mut c_void;
                    fn mprotect(address: *mut c_void, len: usize, prot: i32) -> i32;
                    fn munmap(address: *mut c_void, len: usize) -> i32;
                }

                /// Three pages of address space, the middle one alone
                /// usable.
                pub(super) fn map() -> *mut u8 {
                    // SAFETY: a fresh mapping, then its middle page opened.
                    unsafe {
                        let region = mmap(
                            core::ptr::null_mut(),
                            3 * HOST_PAGE,
                            PROT_NONE,
                            MAP_PRIVATE | MAP_ANONYMOUS,
                            -1,
                            0,
                        );
                        assert_ne!(region as isize, -1, "mmap");
                        let middle = region.cast::<u8>().add(HOST_PAGE).cast();
                        assert_eq!(
                            mprotect(middle, HOST_PAGE, PROT_READ | PROT_WRITE),
                            0,
                            "mprotect"
                        );
                        region.cast()
                    }
                }

                pub(super) fn unmap(region: *mut u8) {
                    // SAFETY: what `map` mapped, unmapped whole.
                    let unmapped = unsafe { munmap(region.cast(), 3 * HOST_PAGE) };
                    assert_eq!(unmapped, 0, "munmap");
                }
            }

            /// Three pages, of which only the middle one may be touched.
            struct Guarded {
                region: *mut u8,
            }

            impl Guarded {
                fn new() -> Self {
                    Self {
                        region: host::map(),
                    }
                }

                /// The usable page's first byte.
                fn start(&self) -> *mut u8 {
                    self.region.wrapping_add(HOST_PAGE)
                }

                /// One past its last.
                fn end(&self) -> *mut u8 {
                    self.region.wrapping_add(2 * HOST_PAGE)
                }

                /// `bytes` written to end where the page does: where they
                /// start.
                fn at_end(&self, bytes: &[u8]) -> *mut u8 {
                    let start = self.end().wrapping_sub(bytes.len());
                    // SAFETY: under a page of bytes, into the usable page.
                    unsafe { core::ptr::copy_nonoverlapping(bytes.as_ptr(), start, bytes.len()) };
                    start
                }

                /// `bytes` written at the page's start.
                fn at_start(&self, bytes: &[u8]) -> *mut u8 {
                    // SAFETY: under a page of bytes, into the usable page.
                    unsafe {
                        core::ptr::copy_nonoverlapping(bytes.as_ptr(), self.start(), bytes.len())
                    };
                    self.start()
                }
            }

            impl Drop for Guarded {
                fn drop(&mut self) {
                    host::unmap(self.region);
                }
            }

            /// Strings whose terminator is a page's last byte: nothing may
            /// read the next page.  A read there faults, and takes the test
            /// process with it.
            #[test]
            fn nothing_reads_past_a_strings_page() {
                let (g, h, out) = (Guarded::new(), Guarded::new(), Guarded::new());
                let every: Vec<u8> = (1..=255).chain([0]).collect();
                for len in 0..=48 {
                    let s = terminated(&filler(len, len));
                    let (p, q) = (g.at_end(&s), h.at_end(&s));
                    let c = i32::from(SOUGHT[0]);
                    // Destinations ending where `out`'s page does.
                    let d = out.end().wrapping_sub(len + 1);
                    // SAFETY: terminated strings ending at their pages' ends,
                    // and room for them before `out`'s.
                    unsafe {
                        assert_eq!(strlen(p), len);
                        assert_eq!(strnlen(p, usize::MAX), len);
                        assert!(strchr(p, c).is_null());
                        assert_eq!(offset(strchrnul(p, c), p), Some(len));
                        assert!(strrchr(p, c).is_null());
                        assert_eq!(offset(rawmemchr(p, 0), p), Some(len));
                        assert_eq!(strcmp(p, q), 0);
                        assert_eq!(strncmp(p, q, usize::MAX), 0);
                        assert_eq!(strspn(p, every.as_ptr()), len);
                        assert_eq!(strcspn(p, b"q\0".as_ptr()), len);
                        assert_eq!(strcspn(p, b"q\xe9\0".as_ptr()), len);
                        assert!(strpbrk(p, b"q\xe9\0".as_ptr()).is_null());
                        assert!(strstr(p, b"q\0".as_ptr()).is_null());
                        assert!(strstr(p, b"qq\0".as_ptr()).is_null());
                        assert!(strcasestr(p, b"QQ\0".as_ptr()).is_null());
                        assert_eq!(strcpy(d, p), d);
                        assert_eq!(stpcpy(d, p), d.wrapping_add(len));
                        d.write(0);
                        assert_eq!(strcat(d, p), d);
                        assert_eq!(strncpy(d, p, len + 1), d);
                        assert_eq!(core::slice::from_raw_parts(d, len + 1), &s[..]);
                    }
                }
                // Runs with no terminator, ending at the page's end.
                for n in 0..=48 {
                    let run = filler(n, 2);
                    let (p, q) = (g.at_end(&run), h.at_end(&run));
                    let c = i32::from(SOUGHT[0]);
                    let d = out.end().wrapping_sub(n);
                    // SAFETY: `n` readable bytes at each, and room for them
                    // before `out`'s page end.
                    unsafe {
                        assert_eq!(strnlen(p, n), n);
                        assert_eq!(strncmp(p, q, n), 0);
                        assert!(memchr(p, c, n).is_null());
                        assert!(memrchr(p, c, n).is_null());
                        assert_eq!(memcmp(p, q, n), 0);
                        assert!(memmem(p, n, b"qq".as_ptr(), 2).is_null());
                        assert_eq!(strncpy(d, p, n), d);
                        assert_eq!(stpncpy(d, p, n), d.wrapping_add(n));
                        assert_eq!(memcpy(d, p, n), d);
                        assert_eq!(memset(d, 0x41, n), d);
                        let e = out.end().wrapping_sub(n + 1);
                        e.write(0);
                        assert_eq!(strncat(e, p, n), e);
                        assert_eq!(core::slice::from_raw_parts(e, n), &run[..]);
                    }
                }
            }

            /// Runs starting at a page's first byte: nothing may read the
            /// page before.
            #[test]
            fn nothing_reads_before_a_buffers_page() {
                let (g, h, out) = (Guarded::new(), Guarded::new(), Guarded::new());
                for n in 0..=48 {
                    let run = filler(n, 6);
                    let (p, q) = (g.at_start(&run), h.at_start(&run));
                    let c = i32::from(SOUGHT[0]);
                    let d = out.start();
                    // SAFETY: `n` readable bytes at each, and room at `d`.
                    unsafe {
                        assert!(memrchr(p, c, n).is_null());
                        assert!(memchr(p, c, n).is_null());
                        assert_eq!(memcmp(p, q, n), 0);
                        assert!(memmem(p, n, b"qq".as_ptr(), 2).is_null());
                        assert_eq!(memcpy(d, p, n), d);
                        assert_eq!(memset(d, 0x41, n), d);
                    }
                }
            }
        }
    }

    #[test]
    fn strcasecmp_l_and_strncasecmp_l_are_the_one_locales_comparisons() {
        let (a, b) = (b"HeLLo\0", b"hello, world\0");
        // SAFETY: NUL-terminated literals.
        unsafe {
            assert_eq!(super::strcasecmp_l(a.as_ptr(), b"hello\0".as_ptr(), 1), 0);
            assert!(super::strcasecmp_l(a.as_ptr(), b.as_ptr(), 1) < 0);
            assert_eq!(super::strncasecmp_l(a.as_ptr(), b.as_ptr(), 5, 1), 0);
            assert!(super::strncasecmp_l(a.as_ptr(), b.as_ptr(), 6, 1) < 0);
        }
    }

    use super::*;

    // Helper: call strverscmp on two byte-string literals.
    fn ver(a: &[u8], b: &[u8]) -> i32 {
        unsafe { strverscmp(a.as_ptr(), b.as_ptr()) }
    }

    #[test]
    fn test_strverscmp_equal() {
        assert_eq!(ver(b"foo\0", b"foo\0"), 0);
        assert_eq!(ver(b"\0", b"\0"), 0);
        assert_eq!(ver(b"1.2.3\0", b"1.2.3\0"), 0);
    }

    #[test]
    fn test_strverscmp_pure_text() {
        // No digits — should behave like strcmp.
        assert!(ver(b"abc\0", b"abd\0") < 0);
        assert!(ver(b"abd\0", b"abc\0") > 0);
        assert!(ver(b"abc\0", b"abcd\0") < 0);
    }

    #[test]
    fn test_strverscmp_numeric_ordering() {
        // The primary use case: "file9" < "file10".
        assert!(ver(b"file9\0", b"file10\0") < 0);
        assert!(ver(b"file10\0", b"file9\0") > 0);
    }

    #[test]
    fn test_strverscmp_version_strings() {
        assert!(ver(b"1.2.3\0", b"1.10.0\0") < 0);
        assert!(ver(b"1.10.0\0", b"1.2.3\0") > 0);
        assert!(ver(b"2.0\0", b"1.999\0") > 0);
    }

    #[test]
    fn test_strverscmp_leading_zeros() {
        // Leading zeros trigger fractional comparison:
        // "1.01" vs "1.1" — 01 vs 1: '0' < '1' lexicographically, but
        // runs are: s1="01" vs s2="1". In the fractional path, s1 has a
        // leading zero: "01" < "1" because '0' < '1' digit-by-digit.
        // Actually per glibc: "1.01" < "1.1" because "01" sorts before "1"
        // (the leading zero makes it fractional, so 0.01 < 0.1).
        assert!(ver(b"1.01\0", b"1.1\0") < 0);
        assert!(ver(b"1.001\0", b"1.01\0") < 0);
    }

    #[test]
    fn test_strverscmp_same_length_different_digits() {
        // Same number of digits, different values.
        assert!(ver(b"foo123\0", b"foo456\0") < 0);
        assert!(ver(b"bar99\0", b"bar42\0") > 0);
    }

    #[test]
    fn test_strverscmp_digit_vs_nondigit() {
        // One string has a digit where the other has a letter.
        // Digit characters ('0'=0x30..'9'=0x39) are less than letters
        // ('A'=0x41, 'a'=0x61) in ASCII.
        assert!(ver(b"a1\0", b"ab\0") < 0);
    }

    #[test]
    fn test_strverscmp_multiple_numeric_segments() {
        // "1.2.30" vs "1.2.4" — comparison triggers at the third segment.
        assert!(ver(b"1.2.30\0", b"1.2.4\0") > 0);
    }

    // -- strlen tests --

    #[test]
    fn test_strlen_basic() {
        assert_eq!(unsafe { strlen(b"hello\0".as_ptr()) }, 5);
        assert_eq!(unsafe { strlen(b"\0".as_ptr()) }, 0);
        assert_eq!(unsafe { strlen(b"a\0".as_ptr()) }, 1);
    }

    #[test]
    fn test_strnlen_basic() {
        assert_eq!(unsafe { strnlen(b"hello\0".as_ptr(), 10) }, 5);
        assert_eq!(unsafe { strnlen(b"hello\0".as_ptr(), 3) }, 3);
        assert_eq!(unsafe { strnlen(b"hello\0".as_ptr(), 0) }, 0);
    }

    // -- strcmp / strncmp tests --

    #[test]
    fn test_strcmp_basic() {
        assert_eq!(unsafe { strcmp(b"abc\0".as_ptr(), b"abc\0".as_ptr()) }, 0);
        assert!(unsafe { strcmp(b"abc\0".as_ptr(), b"abd\0".as_ptr()) } < 0);
        assert!(unsafe { strcmp(b"abd\0".as_ptr(), b"abc\0".as_ptr()) } > 0);
        assert!(unsafe { strcmp(b"ab\0".as_ptr(), b"abc\0".as_ptr()) } < 0);
    }

    #[test]
    fn test_strncmp_basic() {
        assert_eq!(
            unsafe { strncmp(b"abc\0".as_ptr(), b"abd\0".as_ptr(), 2) },
            0
        );
        assert!(unsafe { strncmp(b"abc\0".as_ptr(), b"abd\0".as_ptr(), 3) } < 0);
        assert_eq!(
            unsafe { strncmp(b"abc\0".as_ptr(), b"xyz\0".as_ptr(), 0) },
            0
        );
    }

    // -- strcasecmp tests --

    #[test]
    fn test_strcasecmp_basic() {
        assert_eq!(
            unsafe { strcasecmp(b"Hello\0".as_ptr(), b"hello\0".as_ptr()) },
            0
        );
        assert_eq!(
            unsafe { strcasecmp(b"ABC\0".as_ptr(), b"abc\0".as_ptr()) },
            0
        );
        assert!(unsafe { strcasecmp(b"a\0".as_ptr(), b"B\0".as_ptr()) } < 0);
    }

    // -- strchr / strrchr tests --

    #[test]
    fn test_strchr_found() {
        let s = b"hello world\0";
        let p = unsafe { strchr(s.as_ptr(), i32::from(b'o')) };
        assert!(!p.is_null());
        assert_eq!(unsafe { *p }, b'o');
        // First occurrence: should be at position 4.
        let offset = (p as usize).wrapping_sub(s.as_ptr() as usize);
        assert_eq!(offset, 4);
    }

    #[test]
    fn test_strchr_not_found() {
        let s = b"hello\0";
        let p = unsafe { strchr(s.as_ptr(), i32::from(b'z')) };
        assert!(p.is_null());
    }

    #[test]
    fn test_strchr_null_terminator() {
        // strchr should find the null terminator.
        let s = b"abc\0";
        let p = unsafe { strchr(s.as_ptr(), 0) };
        assert!(!p.is_null());
        assert_eq!(unsafe { *p }, 0);
    }

    #[test]
    fn test_strrchr_found() {
        let s = b"hello world\0";
        let p = unsafe { strrchr(s.as_ptr(), i32::from(b'o')) };
        assert!(!p.is_null());
        // Last 'o' is at position 7 ("world").
        let offset = (p as usize).wrapping_sub(s.as_ptr() as usize);
        assert_eq!(offset, 7);
    }

    // -- strstr / strcasestr tests --

    #[test]
    fn test_strstr_found() {
        let hay = b"hello world\0";
        let needle = b"world\0";
        let p = unsafe { strstr(hay.as_ptr(), needle.as_ptr()) };
        assert!(!p.is_null());
        let offset = (p as usize).wrapping_sub(hay.as_ptr() as usize);
        assert_eq!(offset, 6);
    }

    #[test]
    fn test_strstr_not_found() {
        let hay = b"hello\0";
        let needle = b"xyz\0";
        let p = unsafe { strstr(hay.as_ptr(), needle.as_ptr()) };
        assert!(p.is_null());
    }

    #[test]
    fn test_strstr_empty_needle() {
        let hay = b"hello\0";
        let needle = b"\0";
        let p = unsafe { strstr(hay.as_ptr(), needle.as_ptr()) };
        // Empty needle matches at start.
        assert!(!p.is_null());
        assert_eq!(p, hay.as_ptr());
    }

    #[test]
    fn test_strcasestr_found() {
        let hay = b"Hello World\0";
        let needle = b"world\0";
        let p = unsafe { strcasestr(hay.as_ptr(), needle.as_ptr()) };
        assert!(!p.is_null());
        let offset = (p as usize).wrapping_sub(hay.as_ptr() as usize);
        assert_eq!(offset, 6);
    }

    // -- strspn / strcspn tests --

    #[test]
    fn test_strspn_basic() {
        let s = b"aabbc123\0";
        let accept = b"abc\0";
        assert_eq!(unsafe { strspn(s.as_ptr(), accept.as_ptr()) }, 5);
    }

    #[test]
    fn test_strcspn_basic() {
        let s = b"hello123\0";
        let reject = b"0123456789\0";
        assert_eq!(unsafe { strcspn(s.as_ptr(), reject.as_ptr()) }, 5);
    }

    // -- strpbrk tests --

    #[test]
    fn test_strpbrk_found() {
        let s = b"hello world\0";
        let accept = b"wrd\0";
        let p = unsafe { strpbrk(s.as_ptr(), accept.as_ptr()) };
        assert!(!p.is_null());
        // First match is 'r' at position... actually 'w' at 6, 'r' at 8, 'd' at 10.
        // strpbrk returns first match in s. Let's check.
        assert_eq!(unsafe { *p }, b'w');
    }

    // -- memcmp tests --

    #[test]
    fn test_memcmp_equal() {
        let a = b"hello";
        let b_arr = b"hello";
        assert_eq!(
            unsafe { memcmp(a.as_ptr().cast(), b_arr.as_ptr().cast(), 5) },
            0
        );
    }

    #[test]
    fn test_memcmp_less() {
        let a = b"abc";
        let b_arr = b"abd";
        assert!(unsafe { memcmp(a.as_ptr().cast(), b_arr.as_ptr().cast(), 3) } < 0);
    }

    #[test]
    fn test_memcmp_zero_length() {
        let a = b"abc";
        let b_arr = b"xyz";
        assert_eq!(
            unsafe { memcmp(a.as_ptr().cast(), b_arr.as_ptr().cast(), 0) },
            0
        );
    }

    // -- memchr / memrchr tests --

    #[test]
    fn test_memchr_found() {
        let data = b"abcdef";
        let p = unsafe { memchr(data.as_ptr().cast(), i32::from(b'd'), 6) };
        assert!(!p.is_null());
        let offset = (p as usize).wrapping_sub(data.as_ptr() as usize);
        assert_eq!(offset, 3);
    }

    #[test]
    fn test_memchr_not_found() {
        let data = b"abcdef";
        let p = unsafe { memchr(data.as_ptr().cast(), i32::from(b'z'), 6) };
        assert!(p.is_null());
    }

    #[test]
    fn test_memrchr_found() {
        let data = b"abcabc";
        let p = unsafe { memrchr(data.as_ptr().cast(), i32::from(b'a'), 6) };
        assert!(!p.is_null());
        let offset = (p as usize).wrapping_sub(data.as_ptr() as usize);
        assert_eq!(offset, 3); // Last 'a' is at index 3.
    }

    // -- memmem tests --

    #[test]
    fn test_memmem_found() {
        let hay = b"hello world";
        let needle = b"world";
        let p = unsafe { memmem(hay.as_ptr().cast(), 11, needle.as_ptr().cast(), 5) };
        assert!(!p.is_null());
        let offset = (p as usize).wrapping_sub(hay.as_ptr() as usize);
        assert_eq!(offset, 6);
    }

    #[test]
    fn test_memmem_empty_needle() {
        let hay = b"hello";
        let p = unsafe { memmem(hay.as_ptr().cast(), 5, hay.as_ptr().cast(), 0) };
        // Empty needle returns haystack start.
        assert_eq!(p, hay.as_ptr().cast());
    }

    // -- strtok_r tests --

    #[test]
    fn test_strtok_r_basic() {
        let mut buf = *b"hello,world,foo\0";
        let delim = b",\0";
        let mut saveptr: *mut u8 = core::ptr::null_mut();

        let tok1 = unsafe { strtok_r(buf.as_mut_ptr(), delim.as_ptr(), &mut saveptr) };
        assert!(!tok1.is_null());
        assert_eq!(unsafe { strlen(tok1) }, 5); // "hello"

        let tok2 = unsafe { strtok_r(core::ptr::null_mut(), delim.as_ptr(), &mut saveptr) };
        assert!(!tok2.is_null());
        assert_eq!(unsafe { strlen(tok2) }, 5); // "world"

        let tok3 = unsafe { strtok_r(core::ptr::null_mut(), delim.as_ptr(), &mut saveptr) };
        assert!(!tok3.is_null());
        assert_eq!(unsafe { strlen(tok3) }, 3); // "foo"

        let tok4 = unsafe { strtok_r(core::ptr::null_mut(), delim.as_ptr(), &mut saveptr) };
        assert!(tok4.is_null()); // No more tokens.
    }

    // -- ffs tests --

    #[test]
    fn test_ffs_basic() {
        assert_eq!(ffs(0), 0);
        assert_eq!(ffs(1), 1); // bit 0 set
        assert_eq!(ffs(2), 2); // bit 1 set
        assert_eq!(ffs(4), 3); // bit 2 set
        assert_eq!(ffs(6), 2); // bits 1 and 2 set, first is bit 1
        assert_eq!(ffs(-1), 1); // all bits set, first is bit 0
    }

    // -- strlcpy / strlcat tests --

    #[test]
    fn test_strlcpy_basic() {
        let mut dst = [0u8; 10];
        let src = b"hello\0";
        let len = unsafe { strlcpy(dst.as_mut_ptr(), src.as_ptr(), 10) };
        assert_eq!(len, 5);
        assert_eq!(&dst[..6], b"hello\0");
    }

    #[test]
    fn test_strlcpy_truncation() {
        let mut dst = [0u8; 4];
        let src = b"hello\0";
        let len = unsafe { strlcpy(dst.as_mut_ptr(), src.as_ptr(), 4) };
        assert_eq!(len, 5); // Returns full src length.
        assert_eq!(&dst[..4], b"hel\0"); // Truncated but null-terminated.
    }

    #[test]
    fn test_strlcat_basic() {
        let mut dst = [0u8; 20];
        dst[..6].copy_from_slice(b"hello\0");
        let src = b" world\0";
        let len = unsafe { strlcat(dst.as_mut_ptr(), src.as_ptr(), 20) };
        assert_eq!(len, 11); // 5 + 6
        assert_eq!(&dst[..12], b"hello world\0");
    }

    // -----------------------------------------------------------------------
    // memcpy / memmove edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_memcpy_zero_length() {
        let src = [1u8, 2, 3];
        let mut dst = [0u8; 3];
        unsafe { memcpy(dst.as_mut_ptr(), src.as_ptr(), 0) };
        assert_eq!(dst, [0, 0, 0], "zero-length memcpy should not modify dst");
    }

    #[test]
    fn test_memmove_zero_length() {
        let mut buf = [1u8, 2, 3];
        unsafe { memmove(buf.as_mut_ptr(), buf.as_ptr(), 0) };
        assert_eq!(buf, [1, 2, 3], "zero-length memmove should not modify");
    }

    #[test]
    fn test_memmove_overlap_forward() {
        // Overlapping copy where dst > src.
        let mut buf = [1u8, 2, 3, 4, 5, 0, 0];
        unsafe { memmove(buf.as_mut_ptr().add(2), buf.as_ptr(), 5) };
        assert_eq!(buf, [1, 2, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn test_memmove_overlap_backward() {
        // Overlapping copy where dst < src.
        let mut buf = [0u8, 0, 1, 2, 3, 4, 5];
        unsafe { memmove(buf.as_mut_ptr(), buf.as_ptr().add(2), 5) };
        assert_eq!(buf, [1, 2, 3, 4, 5, 4, 5]);
    }

    #[test]
    fn test_memcpy_single_byte() {
        let src = [0xABu8];
        let mut dst = [0u8];
        unsafe { memcpy(dst.as_mut_ptr(), src.as_ptr(), 1) };
        assert_eq!(dst[0], 0xAB);
    }

    // -----------------------------------------------------------------------
    // strncpy edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_strncpy_pads_with_nuls() {
        // POSIX: strncpy pads remainder with NUL bytes.
        let mut dst = [0xFFu8; 10];
        let src = b"hi\0";
        unsafe { strncpy(dst.as_mut_ptr(), src.as_ptr(), 10) };
        assert_eq!(&dst, b"hi\0\0\0\0\0\0\0\0");
    }

    #[test]
    fn test_strncpy_exact_length_no_nul() {
        // POSIX: strncpy does NOT add NUL if src is >= n chars long.
        let mut dst = [0xFFu8; 3];
        let src = b"abcde\0";
        unsafe { strncpy(dst.as_mut_ptr(), src.as_ptr(), 3) };
        assert_eq!(dst, [b'a', b'b', b'c']);
    }

    #[test]
    fn test_strncpy_zero_n() {
        let mut dst = [0xFFu8; 3];
        let src = b"abc\0";
        unsafe { strncpy(dst.as_mut_ptr(), src.as_ptr(), 0) };
        assert_eq!(dst, [0xFF, 0xFF, 0xFF], "zero n should not modify dst");
    }

    // -----------------------------------------------------------------------
    // strncat edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_strncat_always_nul_terminates() {
        let mut dst = [0u8; 10];
        dst[0] = b'A';
        dst[1] = 0;
        let src = b"BCDEF\0";
        unsafe { strncat(dst.as_mut_ptr(), src.as_ptr(), 3) };
        assert_eq!(&dst[..5], b"ABCD\0");
    }

    #[test]
    fn test_strncat_zero_n() {
        let mut dst = [0u8; 10];
        dst[..4].copy_from_slice(b"abc\0");
        let src = b"xyz\0";
        unsafe { strncat(dst.as_mut_ptr(), src.as_ptr(), 0) };
        assert_eq!(&dst[..4], b"abc\0", "zero n should append nothing");
    }

    // -----------------------------------------------------------------------
    // memcmp edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_memcmp_same_bytes() {
        let a = b"hello";
        let b = b"hello";
        assert_eq!(unsafe { memcmp(a.as_ptr(), b.as_ptr(), 5) }, 0);
    }

    #[test]
    fn test_memcmp_ordering_less() {
        let a = b"abcde";
        let b = b"abcdf";
        assert!(unsafe { memcmp(a.as_ptr(), b.as_ptr(), 5) } < 0);
    }

    #[test]
    fn test_memcmp_ordering_greater() {
        let a = b"abcdf";
        let b = b"abcde";
        assert!(unsafe { memcmp(a.as_ptr(), b.as_ptr(), 5) } > 0);
    }

    #[test]
    fn test_memcmp_zero_len() {
        let a = b"abc";
        let b = b"xyz";
        assert_eq!(
            unsafe { memcmp(a.as_ptr(), b.as_ptr(), 0) },
            0,
            "zero-length memcmp should return 0"
        );
    }

    // -----------------------------------------------------------------------
    // strstr additional edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_strstr_needle_at_start() {
        let hay = b"hello world\0";
        let needle = b"hello\0";
        let result = unsafe { strstr(hay.as_ptr(), needle.as_ptr()) };
        assert_eq!(result, hay.as_ptr());
    }

    #[test]
    fn test_strstr_needle_at_end() {
        let hay = b"hello world\0";
        let needle = b"world\0";
        let result = unsafe { strstr(hay.as_ptr(), needle.as_ptr()) };
        assert!(!result.is_null());
        assert_eq!(unsafe { *result }, b'w');
    }

    // -----------------------------------------------------------------------
    // memset edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_memset_zero_length() {
        let mut buf = [0xFFu8; 4];
        unsafe { memset(buf.as_mut_ptr(), 0, 0) };
        assert_eq!(buf, [0xFF, 0xFF, 0xFF, 0xFF]);
    }

    #[test]
    fn test_memset_full_buffer() {
        let mut buf = [0u8; 8];
        unsafe { memset(buf.as_mut_ptr(), 0xAB, 8) };
        assert_eq!(buf, [0xAB; 8]);
    }

    // -----------------------------------------------------------------------
    // swab
    // -----------------------------------------------------------------------

    #[test]
    fn test_swab_basic() {
        let src = [1u8, 2, 3, 4];
        let mut dst = [0u8; 4];
        unsafe { swab(src.as_ptr(), dst.as_mut_ptr(), 4) };
        assert_eq!(dst, [2, 1, 4, 3]);
    }

    #[test]
    fn test_swab_odd_length() {
        // Odd trailing byte is not swapped.
        let src = [1u8, 2, 3, 4, 5];
        let mut dst = [0u8; 5];
        unsafe { swab(src.as_ptr(), dst.as_mut_ptr(), 5) };
        // Only 2 complete pairs swapped.
        assert_eq!(&dst[..4], &[2, 1, 4, 3]);
    }

    #[test]
    fn test_swab_zero_length() {
        let src = [1u8, 2];
        let mut dst = [0xFFu8; 2];
        unsafe { swab(src.as_ptr(), dst.as_mut_ptr(), 0) };
        assert_eq!(dst, [0xFF, 0xFF], "zero length should not modify dst");
    }

    // -----------------------------------------------------------------------
    // strlcpy edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_strlcpy_size_zero() {
        // size=0: no bytes written, returns src length.
        let mut dst = [0xFFu8; 4];
        let src = b"hello\0";
        let len = unsafe { strlcpy(dst.as_mut_ptr(), src.as_ptr(), 0) };
        assert_eq!(len, 5);
        assert_eq!(dst, [0xFF, 0xFF, 0xFF, 0xFF], "size=0 must not write");
    }

    #[test]
    fn test_strlcpy_size_one() {
        // size=1: only NUL byte written.
        let mut dst = [0xFFu8; 4];
        let src = b"hello\0";
        let len = unsafe { strlcpy(dst.as_mut_ptr(), src.as_ptr(), 1) };
        assert_eq!(len, 5);
        assert_eq!(dst[0], 0, "size=1 must write only NUL");
        assert_eq!(dst[1], 0xFF);
    }

    #[test]
    fn test_strlcpy_exact_fit() {
        // Buffer exactly big enough: src_len < size.
        let mut dst = [0xFFu8; 6];
        let src = b"hello\0";
        let len = unsafe { strlcpy(dst.as_mut_ptr(), src.as_ptr(), 6) };
        assert_eq!(len, 5);
        assert_eq!(&dst[..6], b"hello\0");
    }

    #[test]
    fn test_strlcpy_empty_src() {
        let mut dst = [0xFFu8; 4];
        let src = b"\0";
        let len = unsafe { strlcpy(dst.as_mut_ptr(), src.as_ptr(), 4) };
        assert_eq!(len, 0);
        assert_eq!(dst[0], 0);
    }

    // -----------------------------------------------------------------------
    // strlcat edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_strlcat_truncation() {
        // dst="hello", src=" world", size=8 → truncated to "hello w\0".
        let mut dst = [0u8; 8];
        dst[..6].copy_from_slice(b"hello\0");
        let src = b" world\0";
        let len = unsafe { strlcat(dst.as_mut_ptr(), src.as_ptr(), 8) };
        // Returns dst_len + src_len = 5 + 6 = 11 (truncation: 11 >= 8).
        assert_eq!(len, 11);
        assert_eq!(&dst[..8], b"hello w\0");
    }

    #[test]
    fn test_strlcat_dst_fills_buffer() {
        // dst already fills the buffer (no NUL within size).
        let mut dst = [b'X'; 4]; // No NUL within first 4 bytes.
        let src = b"abc\0";
        let len = unsafe { strlcat(dst.as_mut_ptr(), src.as_ptr(), 4) };
        // strnlen(dst, 4) = 4 >= size=4 → returns size + src_len = 4 + 3 = 7.
        assert_eq!(len, 7);
        // dst unchanged (no room to append).
        assert_eq!(dst, [b'X', b'X', b'X', b'X']);
    }

    #[test]
    fn test_strlcat_size_zero() {
        let mut dst = [0u8; 4];
        dst[..4].copy_from_slice(b"hi\0\0");
        let src = b"there\0";
        let len = unsafe { strlcat(dst.as_mut_ptr(), src.as_ptr(), 0) };
        // size=0 → strnlen(dst,0) = 0 >= 0 → returns 0 + 5 = 5.
        assert_eq!(len, 5);
    }

    #[test]
    fn test_strlcat_empty_src() {
        let mut dst = [0u8; 10];
        dst[..4].copy_from_slice(b"abc\0");
        let src = b"\0";
        let len = unsafe { strlcat(dst.as_mut_ptr(), src.as_ptr(), 10) };
        assert_eq!(len, 3); // 3 + 0
        assert_eq!(&dst[..4], b"abc\0");
    }

    #[test]
    fn test_strlcat_empty_dst() {
        let mut dst = [0u8; 10];
        let src = b"hello\0";
        let len = unsafe { strlcat(dst.as_mut_ptr(), src.as_ptr(), 10) };
        assert_eq!(len, 5); // 0 + 5
        assert_eq!(&dst[..6], b"hello\0");
    }

    // -----------------------------------------------------------------------
    // strsep edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_strsep_basic() {
        let mut data = *b"one,two,three\0";
        let mut ptr: *mut u8 = data.as_mut_ptr();

        let tok1 = unsafe { strsep(&raw mut ptr, b",\0".as_ptr()) };
        assert!(!tok1.is_null());
        assert_eq!(unsafe { strlen(tok1) }, 3);
        assert_eq!(unsafe { *tok1 }, b'o');

        let tok2 = unsafe { strsep(&raw mut ptr, b",\0".as_ptr()) };
        assert!(!tok2.is_null());
        assert_eq!(unsafe { strlen(tok2) }, 3);
        assert_eq!(unsafe { *tok2 }, b't');

        let tok3 = unsafe { strsep(&raw mut ptr, b",\0".as_ptr()) };
        assert!(!tok3.is_null());
        assert_eq!(unsafe { strlen(tok3) }, 5);
        assert_eq!(unsafe { *tok3 }, b't');

        // No more tokens.
        let tok4 = unsafe { strsep(&raw mut ptr, b",\0".as_ptr()) };
        assert!(tok4.is_null());
    }

    #[test]
    fn test_strsep_empty_tokens() {
        // ",,a" → empty, empty, "a"
        let mut data = *b",,a\0";
        let mut ptr: *mut u8 = data.as_mut_ptr();

        let tok1 = unsafe { strsep(&raw mut ptr, b",\0".as_ptr()) };
        assert!(!tok1.is_null());
        assert_eq!(unsafe { strlen(tok1) }, 0); // empty token

        let tok2 = unsafe { strsep(&raw mut ptr, b",\0".as_ptr()) };
        assert!(!tok2.is_null());
        assert_eq!(unsafe { strlen(tok2) }, 0); // empty token

        let tok3 = unsafe { strsep(&raw mut ptr, b",\0".as_ptr()) };
        assert!(!tok3.is_null());
        assert_eq!(unsafe { strlen(tok3) }, 1); // "a"
        assert_eq!(unsafe { *tok3 }, b'a');
    }

    #[test]
    fn test_strsep_no_delimiter() {
        let mut data = *b"hello\0";
        let mut ptr: *mut u8 = data.as_mut_ptr();

        let tok = unsafe { strsep(&raw mut ptr, b",\0".as_ptr()) };
        assert!(!tok.is_null());
        assert_eq!(unsafe { strlen(tok) }, 5);
        assert!(ptr.is_null()); // no more tokens
    }

    #[test]
    fn test_strsep_null_stringp() {
        let result = unsafe { strsep(core::ptr::null_mut(), b",\0".as_ptr()) };
        assert!(result.is_null());
    }

    // -----------------------------------------------------------------------
    // stpcpy / stpncpy return value tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_stpcpy_returns_nul() {
        let mut dst = [0u8; 10];
        let src = b"test\0";
        let end = unsafe { stpcpy(dst.as_mut_ptr(), src.as_ptr()) };
        // end should point to the NUL terminator.
        assert_eq!(end, unsafe { dst.as_mut_ptr().add(4) });
        assert_eq!(unsafe { *end }, 0);
        assert_eq!(&dst[..5], b"test\0");
    }

    #[test]
    fn test_stpcpy_empty_src() {
        let mut dst = [0xFFu8; 4];
        let src = b"\0";
        let end = unsafe { stpcpy(dst.as_mut_ptr(), src.as_ptr()) };
        assert_eq!(end, dst.as_mut_ptr()); // Points to dst[0].
        assert_eq!(unsafe { *end }, 0);
    }

    #[test]
    fn test_stpncpy_returns_first_nul() {
        let mut dst = [0xFFu8; 10];
        let src = b"hi\0";
        let end = unsafe { stpncpy(dst.as_mut_ptr(), src.as_ptr(), 10) };
        // Should return pointer to first NUL (at dst+2).
        assert_eq!(end, unsafe { dst.as_mut_ptr().add(2) });
        assert_eq!(unsafe { *end }, 0);
        // Remainder should be zero-filled.
        for j in 2..10 {
            assert_eq!(dst[j], 0);
        }
    }

    #[test]
    fn test_stpncpy_no_nul_in_n() {
        // src longer than n: returns dst+n, no NUL written.
        let mut dst = [0xFFu8; 3];
        let src = b"abcdef\0";
        let end = unsafe { stpncpy(dst.as_mut_ptr(), src.as_ptr(), 3) };
        assert_eq!(end, unsafe { dst.as_mut_ptr().add(3) });
        assert_eq!(dst, [b'a', b'b', b'c']);
    }

    // -----------------------------------------------------------------------
    // strerror_r edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_strerror_r_success() {
        let mut buf = [0u8; 64];
        let ret = unsafe { strerror_r(0, buf.as_mut_ptr(), 64) };
        assert_eq!(ret, 0);
        assert_eq!(unsafe { strlen(buf.as_ptr()) }, 7); // "Success"
    }

    #[test]
    fn test_strerror_r_truncation() {
        let mut buf = [0u8; 4];
        let ret = unsafe { strerror_r(0, buf.as_mut_ptr(), 4) };
        assert_eq!(ret, crate::errno::ERANGE);
        assert_eq!(&buf[..4], b"Suc\0");
    }

    #[test]
    fn test_strerror_r_exact_fit() {
        // "Success" is 7 chars, buffer of 8 = exact fit with NUL.
        let mut buf = [0xFFu8; 8];
        let ret = unsafe { strerror_r(0, buf.as_mut_ptr(), 8) };
        assert_eq!(ret, 0);
        assert_eq!(&buf[..8], b"Success\0");
    }

    #[test]
    fn test_strerror_r_null_buf() {
        let ret = unsafe { strerror_r(0, core::ptr::null_mut(), 64) };
        assert_eq!(ret, crate::errno::ERANGE);
    }

    #[test]
    fn test_strerror_r_zero_buflen() {
        let mut buf = [0xFFu8; 4];
        let ret = unsafe { strerror_r(0, buf.as_mut_ptr(), 0) };
        assert_eq!(ret, crate::errno::ERANGE);
        assert_eq!(buf[0], 0xFF, "buflen=0 must not write");
    }

    // -----------------------------------------------------------------------
    // memccpy edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_memccpy_byte_found() {
        let src = b"hello world";
        let mut dst = [0u8; 11];
        let ret = unsafe { memccpy(dst.as_mut_ptr(), src.as_ptr(), b' ' as i32, 11) };
        // Should find ' ' at index 5, copy through it, return dst+6.
        assert!(!ret.is_null());
        assert_eq!(ret, unsafe { dst.as_mut_ptr().add(6) });
        assert_eq!(&dst[..6], b"hello ");
    }

    #[test]
    fn test_memccpy_byte_not_found() {
        let src = b"hello";
        let mut dst = [0u8; 5];
        let ret = unsafe { memccpy(dst.as_mut_ptr(), src.as_ptr(), b'x' as i32, 5) };
        assert!(ret.is_null());
        assert_eq!(&dst[..5], b"hello");
    }

    #[test]
    fn test_memccpy_first_byte() {
        let src = b"abcd";
        let mut dst = [0u8; 4];
        let ret = unsafe { memccpy(dst.as_mut_ptr(), src.as_ptr(), b'a' as i32, 4) };
        assert!(!ret.is_null());
        assert_eq!(ret, unsafe { dst.as_mut_ptr().add(1) });
        assert_eq!(dst[0], b'a');
    }

    // -----------------------------------------------------------------------
    // explicit_bzero
    // -----------------------------------------------------------------------

    #[test]
    fn test_explicit_bzero_zeroes_buffer() {
        let mut buf = [0xABu8; 16];
        unsafe { explicit_bzero(buf.as_mut_ptr(), 16) };
        assert_eq!(buf, [0u8; 16]);
    }

    #[test]
    fn test_explicit_bzero_partial() {
        let mut buf = [0xFFu8; 8];
        unsafe { explicit_bzero(buf.as_mut_ptr(), 4) };
        assert_eq!(&buf[..4], &[0, 0, 0, 0]);
        assert_eq!(&buf[4..], &[0xFF, 0xFF, 0xFF, 0xFF]);
    }

    #[test]
    fn test_explicit_bzero_zero_len() {
        let mut buf = [0xFFu8; 4];
        unsafe { explicit_bzero(buf.as_mut_ptr(), 0) };
        assert_eq!(buf, [0xFF; 4], "zero length should not modify");
    }

    // -----------------------------------------------------------------------
    // mempcpy
    // -----------------------------------------------------------------------

    #[test]
    fn test_mempcpy_returns_past_end() {
        let src = b"abc";
        let mut dst = [0u8; 5];
        let end = unsafe { mempcpy(dst.as_mut_ptr(), src.as_ptr(), 3) };
        assert_eq!(end, unsafe { dst.as_mut_ptr().add(3) });
        assert_eq!(&dst[..3], b"abc");
    }

    #[test]
    fn test_mempcpy_zero_length() {
        let src = b"abc";
        let mut dst = [0xFFu8; 3];
        let end = unsafe { mempcpy(dst.as_mut_ptr(), src.as_ptr(), 0) };
        assert_eq!(end, dst.as_mut_ptr()); // Returns dest+0.
        assert_eq!(dst, [0xFF; 3], "zero-length should not modify");
    }

    // -----------------------------------------------------------------------
    // memmem edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_memmem_needle_at_end() {
        let hay = b"abcdef";
        let needle = b"def";
        let ret = unsafe { memmem(hay.as_ptr(), 6, needle.as_ptr(), 3) };
        assert!(!ret.is_null());
        assert_eq!(ret, unsafe { hay.as_ptr().add(3) });
    }

    #[test]
    fn test_memmem_needle_longer() {
        let hay = b"abc";
        let needle = b"abcdef";
        let ret = unsafe { memmem(hay.as_ptr(), 3, needle.as_ptr(), 6) };
        assert!(ret.is_null());
    }

    #[test]
    fn test_memmem_single_byte_match() {
        let hay = b"abcde";
        let needle = b"c";
        let ret = unsafe { memmem(hay.as_ptr(), 5, needle.as_ptr(), 1) };
        assert!(!ret.is_null());
        assert_eq!(ret, unsafe { hay.as_ptr().add(2) });
    }

    #[test]
    fn test_memmem_no_match() {
        let hay = b"abcde";
        let needle = b"xyz";
        let ret = unsafe { memmem(hay.as_ptr(), 5, needle.as_ptr(), 3) };
        assert!(ret.is_null());
    }

    // -----------------------------------------------------------------------
    // strcasecmp / strncasecmp edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_strcasecmp_case_insensitive() {
        assert_eq!(
            unsafe { strcasecmp(b"Hello\0".as_ptr(), b"hello\0".as_ptr()) },
            0
        );
        assert_eq!(
            unsafe { strcasecmp(b"ABC\0".as_ptr(), b"abc\0".as_ptr()) },
            0
        );
    }

    #[test]
    fn test_strcasecmp_different() {
        assert!(unsafe { strcasecmp(b"abc\0".as_ptr(), b"abd\0".as_ptr()) } < 0);
        assert!(unsafe { strcasecmp(b"abd\0".as_ptr(), b"abc\0".as_ptr()) } > 0);
    }

    #[test]
    fn test_strncasecmp_limited() {
        // First 3 chars match case-insensitively, differ at char 4.
        assert_eq!(
            unsafe { strncasecmp(b"ABCx\0".as_ptr(), b"abcy\0".as_ptr(), 3) },
            0
        );
        assert!(unsafe { strncasecmp(b"ABCx\0".as_ptr(), b"abcy\0".as_ptr(), 4) } < 0);
    }

    #[test]
    fn test_strncasecmp_zero_n() {
        // n=0: always returns 0.
        assert_eq!(
            unsafe { strncasecmp(b"abc\0".as_ptr(), b"xyz\0".as_ptr(), 0) },
            0
        );
    }

    // -----------------------------------------------------------------------
    // ffs / ffsl / ffsll edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_ffs_powers_of_two() {
        assert_eq!(ffs(1), 1);
        assert_eq!(ffs(2), 2);
        assert_eq!(ffs(4), 3);
        assert_eq!(ffs(8), 4);
        assert_eq!(ffs(16), 5);
        assert_eq!(ffs(256), 9);
        assert_eq!(ffs(1024), 11);
    }

    #[test]
    fn test_ffs_i32_min() {
        // i32::MIN = 0x80000000, LSB set at bit 31 → ffs returns 32.
        assert_eq!(ffs(i32::MIN), 32);
    }

    #[test]
    fn test_ffsl_basic() {
        assert_eq!(ffsl(0), 0);
        assert_eq!(ffsl(1), 1);
        assert_eq!(ffsl(0x100), 9);
    }

    #[test]
    fn test_ffsl_i64_min() {
        // i64::MIN = 0x8000000000000000, bit 63 → ffs returns 64.
        assert_eq!(ffsl(i64::MIN), 64);
    }

    #[test]
    fn test_ffsll_matches_ffsl() {
        assert_eq!(ffsll(0), ffsl(0));
        assert_eq!(ffsll(42), ffsl(42));
        assert_eq!(ffsll(i64::MIN), ffsl(i64::MIN));
    }

    // -----------------------------------------------------------------------
    // memrchr edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_memrchr_last_occurrence() {
        let buf = b"abcabc";
        let ret = unsafe { memrchr(buf.as_ptr(), b'a' as i32, 6) };
        assert!(!ret.is_null());
        // Should find the LAST 'a', at index 3.
        assert_eq!(ret, unsafe { buf.as_ptr().add(3) });
    }

    #[test]
    fn test_memrchr_not_found() {
        let buf = b"abcdef";
        let ret = unsafe { memrchr(buf.as_ptr(), b'x' as i32, 6) };
        assert!(ret.is_null());
    }

    #[test]
    fn test_memrchr_zero_length() {
        let buf = b"abc";
        let ret = unsafe { memrchr(buf.as_ptr(), b'a' as i32, 0) };
        assert!(ret.is_null());
    }

    // -----------------------------------------------------------------------
    // strcasestr edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_strcasestr_not_found() {
        let hay = b"Hello World\0";
        let needle = b"xyz\0";
        let ret = unsafe { strcasestr(hay.as_ptr(), needle.as_ptr()) };
        assert!(ret.is_null());
    }

    #[test]
    fn test_strcasestr_empty_needle() {
        let hay = b"Hello\0";
        let needle = b"\0";
        let ret = unsafe { strcasestr(hay.as_ptr(), needle.as_ptr()) };
        assert!(!ret.is_null());
        assert_eq!(ret, hay.as_ptr().cast_mut());
    }

    #[test]
    fn test_strcasestr_mixed_case() {
        let hay = b"The Quick Brown Fox\0";
        let needle = b"BROWN\0";
        let ret = unsafe { strcasestr(hay.as_ptr(), needle.as_ptr()) };
        assert!(!ret.is_null());
        assert_eq!(unsafe { *ret }, b'B');
    }

    // -----------------------------------------------------------------------
    // strtok_r additional edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_strtok_r_all_delimiters() {
        // String is all delimiters — should return NULL immediately.
        let mut data = *b",,,,\0";
        let mut save: *mut u8 = core::ptr::null_mut();
        let tok = unsafe { strtok_r(data.as_mut_ptr(), b",\0".as_ptr(), &raw mut save) };
        assert!(tok.is_null());
    }

    #[test]
    fn test_strtok_r_single_token() {
        let mut data = *b"hello\0";
        let mut save: *mut u8 = core::ptr::null_mut();
        let tok = unsafe { strtok_r(data.as_mut_ptr(), b",\0".as_ptr(), &raw mut save) };
        assert!(!tok.is_null());
        assert_eq!(unsafe { strlen(tok) }, 5);

        let tok2 = unsafe { strtok_r(core::ptr::null_mut(), b",\0".as_ptr(), &raw mut save) };
        assert!(tok2.is_null());
    }

    #[test]
    fn test_strtok_r_multiple_delimiters() {
        // Multiple delimiter characters.
        let mut data = *b"one;two,three\0";
        let mut save: *mut u8 = core::ptr::null_mut();
        let tok1 = unsafe { strtok_r(data.as_mut_ptr(), b",;\0".as_ptr(), &raw mut save) };
        assert!(!tok1.is_null());
        assert_eq!(unsafe { strlen(tok1) }, 3);

        let tok2 = unsafe { strtok_r(core::ptr::null_mut(), b",;\0".as_ptr(), &raw mut save) };
        assert!(!tok2.is_null());
        assert_eq!(unsafe { strlen(tok2) }, 3);

        let tok3 = unsafe { strtok_r(core::ptr::null_mut(), b",;\0".as_ptr(), &raw mut save) };
        assert!(!tok3.is_null());
        assert_eq!(unsafe { strlen(tok3) }, 5);
    }

    // -----------------------------------------------------------------------
    // strverscmp additional edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_strverscmp_empty_strings() {
        assert_eq!(ver(b"\0", b"\0"), 0);
    }

    #[test]
    fn test_strverscmp_one_empty() {
        assert!(ver(b"\0", b"a\0") < 0);
        assert!(ver(b"a\0", b"\0") > 0);
    }

    #[test]
    fn test_strverscmp_just_digits() {
        assert!(ver(b"9\0", b"10\0") < 0);
        assert!(ver(b"100\0", b"99\0") > 0);
    }

    // -----------------------------------------------------------------------
    // strerror
    // -----------------------------------------------------------------------

    #[test]
    fn test_strerror_known_codes() {
        // Spot-check a few well-known errno values against their Linux strings.
        let msg = |n: i32| {
            let p = strerror(n);
            assert!(!p.is_null());
            let len = unsafe { strlen(p) };
            unsafe { core::slice::from_raw_parts(p, len) }
        };
        assert_eq!(msg(0), b"Success");
        assert_eq!(msg(1), b"Operation not permitted"); // EPERM
        assert_eq!(msg(2), b"No such file or directory"); // ENOENT
        assert_eq!(msg(9), b"Bad file descriptor"); // EBADF
        assert_eq!(msg(12), b"Cannot allocate memory"); // ENOMEM
        assert_eq!(msg(13), b"Permission denied"); // EACCES
        assert_eq!(msg(22), b"Invalid argument"); // EINVAL
        assert_eq!(msg(32), b"Broken pipe"); // EPIPE
        assert_eq!(msg(111), b"Connection refused"); // ECONNREFUSED
    }

    #[test]
    fn test_strerror_unknown_code() {
        let p = strerror(9999);
        let len = unsafe { strlen(p) };
        let msg = unsafe { core::slice::from_raw_parts(p, len) };
        assert_eq!(msg, b"Unknown error 9999", "glibc's text");
    }

    #[test]
    fn test_strerror_negative() {
        // Negative codes are unknown ones too, and glibc says which.
        let p = strerror(-1);
        let len = unsafe { strlen(p) };
        let msg = unsafe { core::slice::from_raw_parts(p, len) };
        assert_eq!(msg, b"Unknown error -1");
    }

    // -----------------------------------------------------------------------
    // strcoll — locale-aware string comparison (C locale = strcmp)
    // -----------------------------------------------------------------------

    /// GNU make's configure probe for a working `strcoll`
    /// (`AC_FUNC_STRCOLL`), which `scripts/make-spike/run.sh` answers `yes`
    /// for SlateOS (`ac_cv_func_strcoll_works`) without running it.
    #[test]
    fn make_configures_strcoll_probe_passes() {
        // SAFETY: NUL-terminated literals.
        unsafe {
            assert!(strcoll(b"abc\0".as_ptr(), b"def\0".as_ptr()) < 0);
            assert!(strcoll(b"ABC\0".as_ptr(), b"DEF\0".as_ptr()) < 0);
            assert!(strcoll(b"123\0".as_ptr(), b"456\0".as_ptr()) < 0);
        }
    }

    #[test]
    fn test_strcoll_equal() {
        assert_eq!(unsafe { strcoll(b"abc\0".as_ptr(), b"abc\0".as_ptr()) }, 0);
    }

    #[test]
    fn test_strcoll_ordering() {
        assert!(unsafe { strcoll(b"abc\0".as_ptr(), b"abd\0".as_ptr()) } < 0);
        assert!(unsafe { strcoll(b"abd\0".as_ptr(), b"abc\0".as_ptr()) } > 0);
    }

    #[test]
    fn test_strcoll_empty() {
        assert_eq!(unsafe { strcoll(b"\0".as_ptr(), b"\0".as_ptr()) }, 0);
        assert!(unsafe { strcoll(b"\0".as_ptr(), b"a\0".as_ptr()) } < 0);
    }

    // -----------------------------------------------------------------------
    // strxfrm — locale-aware string transform (C locale = copy)
    // -----------------------------------------------------------------------

    #[test]
    fn test_strxfrm_basic() {
        let src = b"hello\0";
        let mut dst = [0u8; 10];
        let len = unsafe { strxfrm(dst.as_mut_ptr(), src.as_ptr(), 10) };
        assert_eq!(len, 5); // "hello" length
        assert_eq!(&dst[..5], b"hello");
        assert_eq!(dst[5], 0); // null terminated by strncpy
    }

    #[test]
    fn test_strxfrm_zero_n() {
        // When n=0, strxfrm should just return the length needed.
        let src = b"test\0";
        let len = unsafe { strxfrm(core::ptr::null_mut(), src.as_ptr(), 0) };
        assert_eq!(len, 4);
    }

    #[test]
    fn test_strxfrm_truncation() {
        // strxfrm copies via strncpy: copies n bytes from src.
        // "abcdef" (len 6) with n=4 copies "abcd" (no nul — src has no
        // nul in first 4 bytes, so strncpy does not null-terminate).
        let src = b"abcdef\0";
        let mut dst = [0xFFu8; 4];
        let len = unsafe { strxfrm(dst.as_mut_ptr(), src.as_ptr(), 4) };
        assert_eq!(len, 6); // Full source length returned.
        assert_eq!(&dst[..4], b"abcd"); // Truncated copy.
    }

    // -----------------------------------------------------------------------
    // strchrnul — like strchr but returns pointer to NUL if not found
    // -----------------------------------------------------------------------

    #[test]
    fn test_strchrnul_found() {
        let s = b"hello world\0";
        let ret = unsafe { strchrnul(s.as_ptr(), b'w' as i32) };
        assert_eq!(ret, unsafe { s.as_ptr().add(6) });
    }

    #[test]
    fn test_strchrnul_not_found() {
        // Should return pointer to NUL terminator, not null.
        let s = b"hello\0";
        let ret = unsafe { strchrnul(s.as_ptr(), b'x' as i32) };
        assert!(!ret.is_null());
        assert_eq!(unsafe { *ret }, 0); // Points to NUL
        assert_eq!(ret, unsafe { s.as_ptr().add(5) });
    }

    #[test]
    fn test_strchrnul_nul_char() {
        // Searching for NUL should return pointer to NUL terminator.
        let s = b"abc\0";
        let ret = unsafe { strchrnul(s.as_ptr(), 0) };
        assert_eq!(ret, unsafe { s.as_ptr().add(3) });
    }

    #[test]
    fn test_strchrnul_first_char() {
        let s = b"abc\0";
        let ret = unsafe { strchrnul(s.as_ptr(), b'a' as i32) };
        assert_eq!(ret, s.as_ptr());
    }

    // -----------------------------------------------------------------------
    // rawmemchr — memchr without length bound
    // -----------------------------------------------------------------------

    #[test]
    fn test_rawmemchr_found() {
        let s = b"find the X here\0";
        let ret = unsafe { rawmemchr(s.as_ptr(), b'X' as i32) };
        assert_eq!(ret, unsafe { s.as_ptr().add(9) });
    }

    #[test]
    fn test_rawmemchr_first_byte() {
        let s = b"abc\0";
        let ret = unsafe { rawmemchr(s.as_ptr(), b'a' as i32) };
        assert_eq!(ret, s.as_ptr());
    }

    #[test]
    fn test_rawmemchr_nul_sentinel() {
        // Common use: find the NUL terminator.
        let s = b"hello\0";
        let ret = unsafe { rawmemchr(s.as_ptr(), 0) };
        assert_eq!(ret, unsafe { s.as_ptr().add(5) });
    }

    // -----------------------------------------------------------------------
    // bcopy / bzero — BSD memory functions
    // -----------------------------------------------------------------------

    #[test]
    fn test_bcopy_basic() {
        let src = b"hello";
        let mut dst = [0u8; 5];
        unsafe { bcopy(src.as_ptr(), dst.as_mut_ptr(), 5) };
        assert_eq!(&dst, b"hello");
    }

    #[test]
    fn test_bcopy_zero_length() {
        let src = b"hello";
        let mut dst = [0xFFu8; 5];
        unsafe { bcopy(src.as_ptr(), dst.as_mut_ptr(), 0) };
        assert_eq!(dst, [0xFF; 5], "zero-length bcopy should not modify");
    }

    #[test]
    fn test_bzero_basic() {
        let mut buf = [0xABu8; 8];
        unsafe { bzero(buf.as_mut_ptr(), 8) };
        assert_eq!(buf, [0; 8]);
    }

    #[test]
    fn test_bzero_zero_length() {
        let mut buf = [0xFFu8; 4];
        unsafe { bzero(buf.as_mut_ptr(), 0) };
        assert_eq!(buf, [0xFF; 4]);
    }

    // -----------------------------------------------------------------------
    // memmem — additional edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_memmem_repeated_pattern() {
        // Should find the FIRST occurrence.
        let hay = b"ababab";
        let needle = b"ab";
        let ret = unsafe { memmem(hay.as_ptr(), 6, needle.as_ptr(), 2) };
        assert_eq!(ret, hay.as_ptr()); // First occurrence at index 0.
    }

    #[test]
    fn test_memmem_overlapping_match() {
        // Needle pattern overlaps: "aaa" in "aaaa" — should find at index 0.
        let hay = b"aaaa";
        let needle = b"aaa";
        let ret = unsafe { memmem(hay.as_ptr(), 4, needle.as_ptr(), 3) };
        assert_eq!(ret, hay.as_ptr());
    }

    #[test]
    fn test_memmem_exact_match() {
        // Needle is the entire haystack.
        let hay = b"exact";
        let needle = b"exact";
        let ret = unsafe { memmem(hay.as_ptr(), 5, needle.as_ptr(), 5) };
        assert_eq!(ret, hay.as_ptr());
    }

    #[test]
    fn test_memmem_zero_length_haystack() {
        let needle = b"ab";
        let ret = unsafe { memmem(needle.as_ptr(), 0, needle.as_ptr(), 2) };
        assert!(ret.is_null());
    }

    // -----------------------------------------------------------------------
    // strncpy — additional edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_strncpy_src_shorter_than_n_pads() {
        let src = b"hi\0";
        let mut dst = [0xFFu8; 8];
        unsafe { strncpy(dst.as_mut_ptr(), src.as_ptr(), 8) };
        assert_eq!(&dst[..2], b"hi");
        // Remaining bytes should be zero-padded.
        assert_eq!(&dst[2..], &[0, 0, 0, 0, 0, 0]);
    }

    // -----------------------------------------------------------------------
    // strncat — additional edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_strncat_appends_and_terminates() {
        let mut buf = [0u8; 20];
        buf[0] = b'H';
        buf[1] = b'i';
        buf[2] = 0;
        let src = b"!!!\0";
        unsafe { strncat(buf.as_mut_ptr(), src.as_ptr(), 2) };
        assert_eq!(&buf[..5], b"Hi!!\0");
    }

    // -----------------------------------------------------------------------
    // swab — additional edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_swab_negative_nbytes() {
        // Negative nbytes should be a no-op.
        let src = b"abcd";
        let mut dst = [0xFFu8; 4];
        unsafe { swab(src.as_ptr(), dst.as_mut_ptr(), -1) };
        assert_eq!(dst, [0xFF; 4]);
    }

    #[test]
    fn test_swab_single_pair() {
        let src = b"ab";
        let mut dst = [0u8; 2];
        unsafe { swab(src.as_ptr(), dst.as_mut_ptr(), 2) };
        assert_eq!(&dst, b"ba");
    }

    // -----------------------------------------------------------------------
    // sys_nerr constant
    // -----------------------------------------------------------------------

    #[test]
    fn test_sys_nerr_value() {
        // Should be one past the highest defined errno (131 → 132).
        assert_eq!(sys_nerr, 134);
    }

    // -----------------------------------------------------------------------
    // memmove — overlapping edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_memmove_overlap_src_equals_dest() {
        let mut buf = *b"hello";
        let p = buf.as_mut_ptr();
        let ret = unsafe { memmove(p, p, 5) };
        assert_eq!(ret, p);
        assert_eq!(&buf, b"hello"); // Unchanged.
    }

    #[test]
    fn test_memmove_large_overlap_backward() {
        // Overlap where dest > src: [0..8] → [2..10].
        let mut buf = [0u8; 10];
        buf[..8].copy_from_slice(b"ABCDEFGH");
        unsafe { memmove(buf.as_mut_ptr().add(2), buf.as_ptr(), 8) };
        assert_eq!(&buf[2..10], b"ABCDEFGH");
    }

    // -----------------------------------------------------------------------
    // memcmp — additional edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_memcmp_single_byte_diff() {
        let a = [0x00u8];
        let b = [0xFFu8];
        assert!(unsafe { memcmp(a.as_ptr(), b.as_ptr(), 1) } < 0);
        assert!(unsafe { memcmp(b.as_ptr(), a.as_ptr(), 1) } > 0);
    }

    #[test]
    fn test_memcmp_diff_at_last_byte() {
        let a = b"abcx";
        let b = b"abcy";
        assert!(unsafe { memcmp(a.as_ptr(), b.as_ptr(), 4) } < 0);
    }

    // -----------------------------------------------------------------------
    // strspn / strcspn — edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_strspn_all_match() {
        let s = b"aaaa\0";
        let accept = b"a\0";
        assert_eq!(unsafe { strspn(s.as_ptr(), accept.as_ptr()) }, 4);
    }

    #[test]
    fn test_strspn_no_match() {
        let s = b"xyz\0";
        let accept = b"abc\0";
        assert_eq!(unsafe { strspn(s.as_ptr(), accept.as_ptr()) }, 0);
    }

    #[test]
    fn test_strspn_empty_string() {
        let s = b"\0";
        let accept = b"abc\0";
        assert_eq!(unsafe { strspn(s.as_ptr(), accept.as_ptr()) }, 0);
    }

    #[test]
    fn test_strcspn_all_reject() {
        let s = b"aaa\0";
        let reject = b"a\0";
        assert_eq!(unsafe { strcspn(s.as_ptr(), reject.as_ptr()) }, 0);
    }

    #[test]
    fn test_strcspn_no_reject() {
        let s = b"abc\0";
        let reject = b"xyz\0";
        assert_eq!(unsafe { strcspn(s.as_ptr(), reject.as_ptr()) }, 3);
    }

    #[test]
    fn test_strcspn_empty_reject() {
        let s = b"abc\0";
        let reject = b"\0";
        // Empty reject means no chars are rejected — span the whole string.
        assert_eq!(unsafe { strcspn(s.as_ptr(), reject.as_ptr()) }, 3);
    }

    // -----------------------------------------------------------------------
    // strpbrk — edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_strpbrk_not_found() {
        let s = b"hello\0";
        let accept = b"xyz\0";
        let ret = unsafe { strpbrk(s.as_ptr(), accept.as_ptr()) };
        assert!(ret.is_null());
    }

    #[test]
    fn test_strpbrk_first_char() {
        let s = b"hello\0";
        let accept = b"h\0";
        let ret = unsafe { strpbrk(s.as_ptr(), accept.as_ptr()) };
        assert_eq!(ret, s.as_ptr());
    }

    // -------------------------------------------------------------------
    // Stress tests — strstr
    // -------------------------------------------------------------------

    #[test]
    fn test_strstr_needle_repeated_in_haystack() {
        // Needle appears multiple times — must find the first.
        let hay = b"abcabcabcabc\0";
        let needle = b"abc\0";
        let ret = unsafe { strstr(hay.as_ptr(), needle.as_ptr()) };
        assert_eq!(ret, hay.as_ptr()); // first occurrence at pos 0
    }

    #[test]
    fn test_strstr_needle_at_various_positions() {
        // Build a 200-byte haystack with needle at position 150.
        let mut buf = [b'x'; 201];
        buf[150] = b'N';
        buf[151] = b'D';
        buf[152] = b'L';
        buf[200] = 0;
        let needle = b"NDL\0";
        let ret = unsafe { strstr(buf.as_ptr(), needle.as_ptr()) };
        assert!(!ret.is_null());
        // Check offset.
        let offset = ret as usize - buf.as_ptr() as usize;
        assert_eq!(offset, 150);
    }

    #[test]
    fn test_strstr_partial_match_then_full() {
        // "aab" in "aaab" — the first "aa" is a partial match for "aab",
        // actual match starts at position 1.
        let hay = b"aaab\0";
        let needle = b"aab\0";
        let ret = unsafe { strstr(hay.as_ptr(), needle.as_ptr()) };
        assert!(!ret.is_null());
        let offset = ret as usize - hay.as_ptr() as usize;
        assert_eq!(offset, 1);
    }

    #[test]
    fn test_strstr_needle_equals_haystack() {
        let s = b"hello\0";
        let ret = unsafe { strstr(s.as_ptr(), s.as_ptr()) };
        assert_eq!(ret, s.as_ptr());
    }

    #[test]
    fn test_strstr_needle_longer_than_haystack() {
        let hay = b"hi\0";
        let needle = b"hello\0";
        let ret = unsafe { strstr(hay.as_ptr(), needle.as_ptr()) };
        assert!(ret.is_null());
    }

    #[test]
    fn test_strstr_single_char_needle() {
        let hay = b"abcde\0";
        let needle = b"d\0";
        let ret = unsafe { strstr(hay.as_ptr(), needle.as_ptr()) };
        assert!(!ret.is_null());
        let offset = ret as usize - hay.as_ptr() as usize;
        assert_eq!(offset, 3);
    }

    #[test]
    fn test_strstr_repeated_partial_prefix() {
        // Pathological case: many near-matches before real match.
        // 101 'a's followed by 'b' then NUL: "aaa...aab\0"
        let mut buf = [b'a'; 103];
        buf[101] = b'b';
        buf[102] = 0;
        let needle = b"ab\0";
        let ret = unsafe { strstr(buf.as_ptr(), needle.as_ptr()) };
        assert!(!ret.is_null());
        // "ab" first occurs at position 100 (the 'a' at [100] followed by 'b' at [101]).
        let offset = ret as usize - buf.as_ptr() as usize;
        assert_eq!(offset, 100);
    }

    // -------------------------------------------------------------------
    // Stress tests — strcasestr
    // -------------------------------------------------------------------

    #[test]
    fn test_strcasestr_stress_full_uppercase() {
        let hay = b"Hello World\0";
        let needle = b"WORLD\0";
        let ret = unsafe { strcasestr(hay.as_ptr(), needle.as_ptr()) };
        assert!(!ret.is_null());
        let offset = ret as usize - hay.as_ptr() as usize;
        assert_eq!(offset, 6);
    }

    #[test]
    fn test_strcasestr_stress_alternating_case() {
        let hay = b"ABCDEFG\0";
        let needle = b"cDe\0";
        let ret = unsafe { strcasestr(hay.as_ptr(), needle.as_ptr()) };
        assert!(!ret.is_null());
        let offset = ret as usize - hay.as_ptr() as usize;
        assert_eq!(offset, 2);
    }

    #[test]
    fn test_strcasestr_stress_no_match() {
        let hay = b"hello world\0";
        let needle = b"xyz\0";
        let ret = unsafe { strcasestr(hay.as_ptr(), needle.as_ptr()) };
        assert!(ret.is_null());
    }

    #[test]
    fn test_strcasestr_stress_empty() {
        let hay = b"test\0";
        let needle = b"\0";
        let ret = unsafe { strcasestr(hay.as_ptr(), needle.as_ptr()) };
        assert_eq!(ret, hay.as_ptr().cast_mut());
    }

    // -------------------------------------------------------------------
    // Stress tests — strtok_r comprehensive
    // -------------------------------------------------------------------

    #[test]
    fn test_strtok_r_consecutive_delimiters() {
        // Multiple consecutive delimiters should be treated as one.
        let mut buf = *b"a,,b,,c\0";
        let delim = b",\0";
        let mut save: *mut u8 = core::ptr::null_mut();

        let t1 = unsafe { strtok_r(buf.as_mut_ptr(), delim.as_ptr(), &raw mut save) };
        assert!(!t1.is_null());
        assert_eq!(unsafe { *t1 }, b'a');

        let t2 = unsafe { strtok_r(core::ptr::null_mut(), delim.as_ptr(), &raw mut save) };
        assert!(!t2.is_null());
        assert_eq!(unsafe { *t2 }, b'b');

        let t3 = unsafe { strtok_r(core::ptr::null_mut(), delim.as_ptr(), &raw mut save) };
        assert!(!t3.is_null());
        assert_eq!(unsafe { *t3 }, b'c');

        let t4 = unsafe { strtok_r(core::ptr::null_mut(), delim.as_ptr(), &raw mut save) };
        assert!(t4.is_null());
    }

    #[test]
    fn test_strtok_r_leading_trailing_delimiters() {
        let mut buf = *b",,hello,,world,,\0";
        let delim = b",\0";
        let mut save: *mut u8 = core::ptr::null_mut();

        let t1 = unsafe { strtok_r(buf.as_mut_ptr(), delim.as_ptr(), &raw mut save) };
        assert!(!t1.is_null());
        assert_eq!(unsafe { cstr_eq(t1, b"hello") }, true);

        let t2 = unsafe { strtok_r(core::ptr::null_mut(), delim.as_ptr(), &raw mut save) };
        assert!(!t2.is_null());
        assert_eq!(unsafe { cstr_eq(t2, b"world") }, true);

        let t3 = unsafe { strtok_r(core::ptr::null_mut(), delim.as_ptr(), &raw mut save) };
        assert!(t3.is_null());
    }

    #[test]
    fn test_strtok_r_all_delimiters_only() {
        let mut buf = *b",,,\0";
        let delim = b",\0";
        let mut save: *mut u8 = core::ptr::null_mut();

        let t1 = unsafe { strtok_r(buf.as_mut_ptr(), delim.as_ptr(), &raw mut save) };
        assert!(t1.is_null());
    }

    #[test]
    fn test_strtok_r_varying_delimiters() {
        // Change delimiter set between calls.
        let mut buf = *b"a,b:c\0";
        let mut save: *mut u8 = core::ptr::null_mut();

        let t1 = unsafe { strtok_r(buf.as_mut_ptr(), b",\0".as_ptr(), &raw mut save) };
        assert!(!t1.is_null());
        assert_eq!(unsafe { *t1 }, b'a');

        // Now use ':' as delimiter.
        let t2 = unsafe { strtok_r(core::ptr::null_mut(), b":\0".as_ptr(), &raw mut save) };
        assert!(!t2.is_null());
        assert_eq!(unsafe { cstr_eq(t2, b"b") }, true);

        let t3 = unsafe { strtok_r(core::ptr::null_mut(), b":\0".as_ptr(), &raw mut save) };
        assert!(!t3.is_null());
        assert_eq!(unsafe { *t3 }, b'c');
    }

    #[test]
    fn test_strtok_r_multi_char_delimiters() {
        // Multiple characters in delimiter set.
        let mut buf = *b"one two\tthree\nfour\0";
        let delim = b" \t\n\0";
        let mut save: *mut u8 = core::ptr::null_mut();

        let t1 = unsafe { strtok_r(buf.as_mut_ptr(), delim.as_ptr(), &raw mut save) };
        assert!(unsafe { cstr_eq(t1, b"one") });

        let t2 = unsafe { strtok_r(core::ptr::null_mut(), delim.as_ptr(), &raw mut save) };
        assert!(unsafe { cstr_eq(t2, b"two") });

        let t3 = unsafe { strtok_r(core::ptr::null_mut(), delim.as_ptr(), &raw mut save) };
        assert!(unsafe { cstr_eq(t3, b"three") });

        let t4 = unsafe { strtok_r(core::ptr::null_mut(), delim.as_ptr(), &raw mut save) };
        assert!(unsafe { cstr_eq(t4, b"four") });

        let t5 = unsafe { strtok_r(core::ptr::null_mut(), delim.as_ptr(), &raw mut save) };
        assert!(t5.is_null());
    }

    #[test]
    fn test_strtok_r_empty_string() {
        let mut buf = *b"\0";
        let delim = b",\0";
        let mut save: *mut u8 = core::ptr::null_mut();

        let t1 = unsafe { strtok_r(buf.as_mut_ptr(), delim.as_ptr(), &raw mut save) };
        assert!(t1.is_null());
    }

    // -------------------------------------------------------------------
    // Stress tests — strspn / strcspn exhaustive
    // -------------------------------------------------------------------

    #[test]
    fn test_strspn_entire_string_accepted() {
        let s = b"aaabbbccc\0";
        let accept = b"abc\0";
        let ret = unsafe { strspn(s.as_ptr(), accept.as_ptr()) };
        assert_eq!(ret, 9);
    }

    #[test]
    fn test_strspn_reject_at_position_zero() {
        let s = b"xyz\0";
        let accept = b"abc\0";
        let ret = unsafe { strspn(s.as_ptr(), accept.as_ptr()) };
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_strspn_empty_accept_set() {
        let s = b"hello\0";
        let accept = b"\0";
        let ret = unsafe { strspn(s.as_ptr(), accept.as_ptr()) };
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_strcspn_entire_string_no_reject() {
        let s = b"hello\0";
        let reject = b"xyz\0";
        let ret = unsafe { strcspn(s.as_ptr(), reject.as_ptr()) };
        assert_eq!(ret, 5);
    }

    #[test]
    fn test_strcspn_first_char_in_reject() {
        let s = b"hello\0";
        let reject = b"h\0";
        let ret = unsafe { strcspn(s.as_ptr(), reject.as_ptr()) };
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_strcspn_last_char_in_reject() {
        let s = b"hello\0";
        let reject = b"o\0";
        let ret = unsafe { strcspn(s.as_ptr(), reject.as_ptr()) };
        assert_eq!(ret, 4);
    }

    #[test]
    fn test_strspn_binary_values() {
        // Test with non-ASCII byte values in accept set.
        let s: &[u8] = &[0x80, 0x81, 0x82, b'A', 0];
        let accept: &[u8] = &[0x80, 0x81, 0x82, 0];
        let ret = unsafe { strspn(s.as_ptr(), accept.as_ptr()) };
        assert_eq!(ret, 3);
    }

    // -------------------------------------------------------------------
    // Stress tests — strpbrk additional
    // -------------------------------------------------------------------

    #[test]
    fn test_strpbrk_last_char_matches() {
        let s = b"abcde\0";
        let accept = b"e\0";
        let ret = unsafe { strpbrk(s.as_ptr(), accept.as_ptr()) };
        assert!(!ret.is_null());
        let offset = ret as usize - s.as_ptr() as usize;
        assert_eq!(offset, 4);
    }

    #[test]
    fn test_strpbrk_multiple_matches_returns_first() {
        let s = b"hello world\0";
        let accept = b"ow\0";
        let ret = unsafe { strpbrk(s.as_ptr(), accept.as_ptr()) };
        assert!(!ret.is_null());
        // 'o' appears at position 4, 'w' at position 6.
        let offset = ret as usize - s.as_ptr() as usize;
        assert_eq!(offset, 4);
    }

    #[test]
    fn test_strpbrk_empty_accept() {
        let s = b"hello\0";
        let accept = b"\0";
        let ret = unsafe { strpbrk(s.as_ptr(), accept.as_ptr()) };
        assert!(ret.is_null());
    }

    // -------------------------------------------------------------------
    // Stress tests — memmove with various overlaps
    // -------------------------------------------------------------------

    #[test]
    fn test_memmove_no_overlap() {
        let src = [1u8, 2, 3, 4, 5];
        let mut dest = [0u8; 5];
        unsafe {
            memmove(dest.as_mut_ptr(), src.as_ptr(), 5);
        }
        assert_eq!(dest, [1, 2, 3, 4, 5]);
    }

    #[test]
    fn test_memmove_overlap_one_byte_forward() {
        // [1,2,3,4,5] — copy [0..4] to [1..5].
        let mut buf = [1u8, 2, 3, 4, 5];
        unsafe {
            memmove(buf.as_mut_ptr().add(1), buf.as_ptr(), 4);
        }
        assert_eq!(buf, [1, 1, 2, 3, 4]);
    }

    #[test]
    fn test_memmove_overlap_one_byte_backward() {
        // [1,2,3,4,5] — copy [1..5] to [0..4].
        let mut buf = [1u8, 2, 3, 4, 5];
        unsafe {
            memmove(buf.as_mut_ptr(), buf.as_ptr().add(1), 4);
        }
        assert_eq!(buf, [2, 3, 4, 5, 5]);
    }

    #[test]
    fn test_memmove_complete_overlap() {
        // Copy region onto itself — should be no-op.
        let mut buf = [10u8, 20, 30, 40, 50];
        unsafe {
            memmove(buf.as_mut_ptr(), buf.as_ptr(), 5);
        }
        assert_eq!(buf, [10, 20, 30, 40, 50]);
    }

    #[test]
    fn test_memmove_large_region() {
        // 512-byte overlapping copy (bigger than cache line).
        let mut buf = [0u8; 1024];
        for i in 0..512 {
            buf[i] = (i & 0xFF) as u8;
        }
        // Copy first 512 bytes to offset 256 (overlap of 256 bytes).
        unsafe {
            memmove(buf.as_mut_ptr().add(256), buf.as_ptr(), 512);
        }
        // Verify: buf[256..768] should equal original [0..512].
        for i in 0..512 {
            assert_eq!(buf[256 + i], (i & 0xFF) as u8);
        }
    }

    // -------------------------------------------------------------------
    // Stress tests — strsep edge cases
    // -------------------------------------------------------------------

    #[test]
    fn test_strsep_single_char_tokens() {
        let mut buf = *b"a,b,c\0";
        let mut ptr: *mut u8 = buf.as_mut_ptr();
        let delim = b",\0";

        let t1 = unsafe { strsep(&raw mut ptr, delim.as_ptr()) };
        assert!(unsafe { cstr_eq(t1, b"a") });

        let t2 = unsafe { strsep(&raw mut ptr, delim.as_ptr()) };
        assert!(unsafe { cstr_eq(t2, b"b") });

        let t3 = unsafe { strsep(&raw mut ptr, delim.as_ptr()) };
        assert!(unsafe { cstr_eq(t3, b"c") });

        // ptr should be null after exhausting.
        assert!(ptr.is_null());
    }

    #[test]
    fn test_strsep_preserves_empty_fields() {
        // Unlike strtok, strsep returns empty strings for consecutive delims.
        let mut buf = *b"a,,b\0";
        let mut ptr: *mut u8 = buf.as_mut_ptr();
        let delim = b",\0";

        let t1 = unsafe { strsep(&raw mut ptr, delim.as_ptr()) };
        assert!(unsafe { cstr_eq(t1, b"a") });

        let t2 = unsafe { strsep(&raw mut ptr, delim.as_ptr()) };
        // Should be empty string (the field between two commas).
        assert!(unsafe { cstr_eq(t2, b"") });

        let t3 = unsafe { strsep(&raw mut ptr, delim.as_ptr()) };
        assert!(unsafe { cstr_eq(t3, b"b") });
    }

    #[test]
    fn test_strsep_trailing_delimiter() {
        let mut buf = *b"hello,\0";
        let mut ptr: *mut u8 = buf.as_mut_ptr();
        let delim = b",\0";

        let t1 = unsafe { strsep(&raw mut ptr, delim.as_ptr()) };
        assert!(unsafe { cstr_eq(t1, b"hello") });

        let t2 = unsafe { strsep(&raw mut ptr, delim.as_ptr()) };
        // Empty field after trailing comma.
        assert!(unsafe { cstr_eq(t2, b"") });

        assert!(ptr.is_null());
    }

    // -------------------------------------------------------------------
    // Stress tests — swab
    // -------------------------------------------------------------------

    #[test]
    fn test_swab_stress_six_bytes() {
        let src = [1u8, 2, 3, 4, 5, 6];
        let mut dest = [0u8; 6];
        unsafe {
            swab(src.as_ptr(), dest.as_mut_ptr(), 6);
        }
        assert_eq!(dest, [2, 1, 4, 3, 6, 5]);
    }

    #[test]
    fn test_swab_stress_odd_drops_last() {
        // Odd nbytes: last byte ignored.
        let src = [1u8, 2, 3, 4, 5];
        let mut dest = [0u8; 5];
        unsafe {
            swab(src.as_ptr(), dest.as_mut_ptr(), 5);
        }
        // Only first 4 bytes swapped, 5th untouched.
        assert_eq!(dest[0], 2);
        assert_eq!(dest[1], 1);
        assert_eq!(dest[2], 4);
        assert_eq!(dest[3], 3);
        assert_eq!(dest[4], 0); // not written
    }

    #[test]
    fn test_swab_stress_empty() {
        let src = [1u8, 2];
        let mut dest = [0u8; 2];
        unsafe {
            swab(src.as_ptr(), dest.as_mut_ptr(), 0);
        }
        assert_eq!(dest, [0, 0]); // untouched
    }

    #[test]
    fn test_swab_stress_single_pair() {
        let src = [0xAB_u8, 0xCD];
        let mut dest = [0u8; 2];
        unsafe {
            swab(src.as_ptr(), dest.as_mut_ptr(), 2);
        }
        assert_eq!(dest, [0xCD, 0xAB]);
    }

    // -------------------------------------------------------------------
    // Helper for strtok_r / strsep tests
    // -------------------------------------------------------------------

    /// Check if a C string pointer equals an expected byte slice.
    unsafe fn cstr_eq(p: *const u8, expected: &[u8]) -> bool {
        if p.is_null() {
            return false;
        }
        for (i, &b) in expected.iter().enumerate() {
            if unsafe { *p.add(i) } != b {
                return false;
            }
        }
        unsafe { *p.add(expected.len()) == 0 }
    }

    // -------------------------------------------------------------------
    // FORTIFY _chk wrappers — smoke tests
    // -------------------------------------------------------------------

    #[test]
    fn test_memcpy_chk_delegates() {
        let src = [1u8, 2, 3, 4];
        let mut dest = [0u8; 4];
        let ret = unsafe { __memcpy_chk(dest.as_mut_ptr(), src.as_ptr(), 4, 4) };
        assert_eq!(dest, [1, 2, 3, 4]);
        assert_eq!(ret, dest.as_mut_ptr());
    }

    #[test]
    fn test_mempcpy_chk_delegates() {
        let src = [9u8, 8, 7, 6];
        let mut dest = [0u8; 4];
        let ret = unsafe { __mempcpy_chk(dest.as_mut_ptr(), src.as_ptr(), 4, 4) };
        assert_eq!(dest, [9, 8, 7, 6]);
        // mempcpy returns dest + n (one past the last written byte).
        let offset = ret as usize - dest.as_ptr() as usize;
        assert_eq!(offset, 4);
    }

    #[test]
    fn test_memmove_chk_delegates() {
        let src = [5u8, 6, 7, 8];
        let mut dest = [0u8; 4];
        let ret = unsafe { __memmove_chk(dest.as_mut_ptr(), src.as_ptr(), 4, 4) };
        assert_eq!(dest, [5, 6, 7, 8]);
        assert_eq!(ret, dest.as_mut_ptr());
    }

    #[test]
    fn test_memset_chk_delegates() {
        let mut buf = [0xFFu8; 4];
        let ret = unsafe { __memset_chk(buf.as_mut_ptr(), 0, 4, 4) };
        assert_eq!(buf, [0, 0, 0, 0]);
        assert_eq!(ret, buf.as_mut_ptr());
    }

    #[test]
    fn test_strcpy_chk_delegates() {
        let src = b"hi\0";
        let mut dest = [0u8; 4];
        let ret = unsafe { __strcpy_chk(dest.as_mut_ptr(), src.as_ptr(), 4) };
        assert_eq!(&dest[..3], b"hi\0");
        assert_eq!(ret, dest.as_mut_ptr());
    }

    #[test]
    fn test_strcat_chk_delegates() {
        let mut buf = [0u8; 16];
        buf[0] = b'A';
        buf[1] = 0;
        let src = b"BC\0";
        let ret = unsafe { __strcat_chk(buf.as_mut_ptr(), src.as_ptr(), 16) };
        assert_eq!(&buf[..4], b"ABC\0");
        assert_eq!(ret, buf.as_mut_ptr());
    }

    #[test]
    fn test_stpcpy_chk_delegates() {
        let src = b"ok\0";
        let mut dest = [0u8; 4];
        let ret = unsafe { __stpcpy_chk(dest.as_mut_ptr(), src.as_ptr(), 4) };
        assert_eq!(&dest[..3], b"ok\0");
        // stpcpy returns pointer to the NUL.
        let offset = ret as usize - dest.as_ptr() as usize;
        assert_eq!(offset, 2);
    }

    // -- strtok (non-reentrant) --

    /// Serialises every test that calls `strtok`.
    ///
    /// `strtok`'s save pointer is one `static mut` for the whole process, and
    /// `libtest` runs these three tests on three threads at once. Two hazards,
    /// in ascending order of seriousness:
    ///
    /// * The *observed* one: a sibling's first call overwrites `SAVED` between
    ///   this test's first and second call, so the continuation call tokenises
    ///   the sibling's buffer, or finds the pointer already exhausted and
    ///   returns null. `test_strtok_basic` failed on `!tok2.is_null()` under a
    ///   loaded `cargo test -p coreutils -p posix`.
    /// * The one that matters: each test's buffer is a local on its own
    ///   thread's stack, so `SAVED` routinely points into *another live
    ///   thread's* stack frame — and `strtok` writes a NUL through it to
    ///   terminate the token. An unlucky interleaving is memory corruption in
    ///   a frame the writing thread does not own, not a failed assertion.
    ///
    /// Taken as the first statement of each test, so it covers the whole
    /// first-call/continuation-call sequence, which is the unit that must be
    /// atomic. Poison is recovered rather than propagated: one failing test
    /// should report one failure, not poison the lock and bury the cause under
    /// two more.
    static STRTOK_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[must_use = "the guard serialises strtok's global save pointer; bind it to `_g`"]
    fn lock_strtok_for_test() -> std::sync::MutexGuard<'static, ()> {
        STRTOK_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    #[test]
    fn test_strtok_basic() {
        let _g = lock_strtok_for_test();
        let mut buf = *b"hello,world\0";
        let tok1 = unsafe { strtok(buf.as_mut_ptr(), b",\0".as_ptr()) };
        assert!(!tok1.is_null());
        assert_eq!(unsafe { *tok1 }, b'h');
        let tok2 = unsafe { strtok(core::ptr::null_mut(), b",\0".as_ptr()) };
        assert!(!tok2.is_null());
        assert_eq!(unsafe { *tok2 }, b'w');
        let tok3 = unsafe { strtok(core::ptr::null_mut(), b",\0".as_ptr()) };
        assert!(tok3.is_null());
    }

    #[test]
    fn test_strtok_no_delimiters() {
        let _g = lock_strtok_for_test();
        let mut buf = *b"single\0";
        let tok = unsafe { strtok(buf.as_mut_ptr(), b",\0".as_ptr()) };
        assert!(!tok.is_null());
        assert_eq!(unsafe { *tok }, b's');
        let tok2 = unsafe { strtok(core::ptr::null_mut(), b",\0".as_ptr()) };
        assert!(tok2.is_null());
    }

    #[test]
    fn test_strtok_all_delimiters() {
        let _g = lock_strtok_for_test();
        let mut buf = *b",,,\0";
        let tok = unsafe { strtok(buf.as_mut_ptr(), b",\0".as_ptr()) };
        assert!(tok.is_null());
    }

    // -- strdup --
    // Note: strdup/strndup allocate via malloc which uses mmap syscalls.
    // Only null-pointer handling can be tested without kernel support.

    #[test]
    fn test_strdup_null() {
        let dup = unsafe { strdup(core::ptr::null()) };
        assert!(dup.is_null());
    }

    // -- strndup --

    #[test]
    fn test_strndup_null() {
        let dup = unsafe { strndup(core::ptr::null(), 10) };
        assert!(dup.is_null());
    }

    // -- strcoll_l --

    #[test]
    fn test_strcoll_l_equal() {
        let a = b"abc\0";
        let b_str = b"abc\0";
        assert_eq!(unsafe { strcoll_l(a.as_ptr(), b_str.as_ptr(), 0) }, 0);
    }

    #[test]
    fn test_strcoll_l_ordering() {
        let a = b"abc\0";
        let b_str = b"abd\0";
        assert!(unsafe { strcoll_l(a.as_ptr(), b_str.as_ptr(), 0) } < 0);
    }

    // -- strxfrm_l --

    #[test]
    fn test_strxfrm_l_basic() {
        let src = b"hello\0";
        let mut dst = [0u8; 16];
        let len = unsafe { strxfrm_l(dst.as_mut_ptr(), src.as_ptr(), 16, 0) };
        assert_eq!(len, 5);
        assert_eq!(&dst[..6], b"hello\0");
    }

    // -- strerror_l --

    #[test]
    fn test_strerror_l_known_code() {
        let msg = strerror_l(2, 0); // ENOENT
        assert!(!msg.is_null());
        // Should be "No such file or directory"
        assert_eq!(unsafe { *msg }, b'N');
    }

    #[test]
    fn test_strerror_l_matches_strerror() {
        let msg1 = strerror(13); // EACCES
        let msg2 = strerror_l(13, 0);
        assert_eq!(msg1, msg2);
    }

    // -- __xpg_strerror_r --

    #[test]
    fn test_xpg_strerror_r_success() {
        let mut buf = [0u8; 64];
        let ret = unsafe { __xpg_strerror_r(0, buf.as_mut_ptr(), 64) };
        assert_eq!(ret, 0);
        // Should contain "Success".
        assert_eq!(buf[0], b'S');
    }

    #[test]
    fn test_xpg_strerror_r_truncation() {
        let mut buf = [0u8; 4];
        let ret = unsafe { __xpg_strerror_r(2, buf.as_mut_ptr(), 4) };
        assert_eq!(ret, crate::errno::ERANGE);
    }

    // -- __strncpy_chk --

    #[test]
    fn test_strncpy_chk_delegates() {
        let src = b"test\0";
        let mut dst = [0u8; 8];
        let ret = unsafe { __strncpy_chk(dst.as_mut_ptr(), src.as_ptr(), 8, 8) };
        assert_eq!(ret, dst.as_mut_ptr());
        assert_eq!(&dst[..5], b"test\0");
    }

    // -- __strncat_chk --

    #[test]
    fn test_strncat_chk_delegates() {
        let mut buf = [0u8; 16];
        buf[0] = b'A';
        buf[1] = 0;
        let src = b"BCD\0";
        let ret = unsafe { __strncat_chk(buf.as_mut_ptr(), src.as_ptr(), 3, 16) };
        assert_eq!(&buf[..5], b"ABCD\0");
        assert_eq!(ret, buf.as_mut_ptr());
    }

    // -- __stpncpy_chk --

    #[test]
    fn test_stpncpy_chk_delegates() {
        let src = b"ab\0";
        let mut dst = [0u8; 4];
        let ret = unsafe { __stpncpy_chk(dst.as_mut_ptr(), src.as_ptr(), 4, 4) };
        assert_eq!(&dst[..3], b"ab\0");
        // stpncpy returns pointer to first NUL within n.
        let offset = ret as usize - dst.as_ptr() as usize;
        assert_eq!(offset, 2);
    }

    // -- the bound each fortified copy checks --
    //
    // A copy that does not fit calls `__chk_fail`, which aborts the process,
    // so the host can only show the other side of each bound: an operation
    // that exactly fills its object, and one whose object size the compiler
    // did not know (`(size_t)-1`), both go through. The aborting side is
    // `services/ctest-fortify-abort`'s, in ring 3, where a child can die of it.
    // The byte past each object is a guard that must survive.

    const UNKNOWN: usize = usize::MAX;

    #[test]
    fn fortified_memory_copies_may_fill_their_object_exactly() {
        let src = [7u8; 8];
        let mut buf = [0u8; 9];
        unsafe {
            __memcpy_chk(buf.as_mut_ptr(), src.as_ptr(), 8, 8);
            assert_eq!((&buf[..8], buf[8]), (&[7u8; 8][..], 0), "memcpy");
            __memset_chk(buf.as_mut_ptr(), 3, 8, 8);
            assert_eq!((&buf[..8], buf[8]), (&[3u8; 8][..], 0), "memset");
            __memmove_chk(buf.as_mut_ptr(), src.as_ptr(), 8, 8);
            assert_eq!(buf[8], 0, "memmove");
            let end = __mempcpy_chk(buf.as_mut_ptr(), src.as_ptr(), 8, 8);
            assert_eq!(end, buf.as_mut_ptr().add(8), "mempcpy");
            assert_eq!(buf[8], 0);
        }
    }

    #[test]
    fn fortified_string_copies_may_fill_their_object_exactly() {
        let mut buf = [0xeeu8; 5];
        unsafe {
            // "abc" and its terminator: four bytes, in a four-byte object.
            __strcpy_chk(buf.as_mut_ptr(), b"abc\0".as_ptr(), 4);
            assert_eq!(&buf, b"abc\0\xee");
            buf = [0xee; 5];
            let end = __stpcpy_chk(buf.as_mut_ptr(), b"abc\0".as_ptr(), 4);
            assert_eq!(end.cast_const(), buf.as_ptr().add(3));
            assert_eq!(&buf, b"abc\0\xee");
            // strncpy writes exactly n bytes, so n == object size fits.
            buf = [0xee; 5];
            __strncpy_chk(buf.as_mut_ptr(), b"ab\0".as_ptr(), 4, 4);
            assert_eq!(&buf, b"ab\0\0\xee");
            buf = [0xee; 5];
            __stpncpy_chk(buf.as_mut_ptr(), b"ab\0".as_ptr(), 4, 4);
            assert_eq!(&buf, b"ab\0\0\xee");
        }
    }

    #[test]
    fn fortified_concatenation_may_fill_its_object_exactly() {
        let mut buf = [0xeeu8; 7];
        buf[..3].copy_from_slice(b"ab\0");
        unsafe {
            // "ab" + "cde" + NUL = six bytes, in a six-byte object.
            __strcat_chk(buf.as_mut_ptr(), b"cde\0".as_ptr(), 6);
            assert_eq!(&buf, b"abcde\0\xee");
            buf = [0xee; 7];
            buf[..3].copy_from_slice(b"ab\0");
            // At most three of "cdefg": the same six bytes.
            __strncat_chk(buf.as_mut_ptr(), b"cdefg\0".as_ptr(), 3, 6);
            assert_eq!(&buf, b"abcde\0\xee");
        }
    }

    /// `(size_t)-1` is the compiler saying it could not tell; that must never
    /// be read as a small object.
    #[test]
    fn an_unknown_object_size_is_never_refused() {
        let mut buf = [0u8; 16];
        unsafe {
            __memcpy_chk(buf.as_mut_ptr(), b"0123456789".as_ptr(), 10, UNKNOWN);
            __strcpy_chk(buf.as_mut_ptr(), b"hello\0".as_ptr(), UNKNOWN);
            __strcat_chk(buf.as_mut_ptr(), b" you\0".as_ptr(), UNKNOWN);
            __strncat_chk(buf.as_mut_ptr(), b"!!!\0".as_ptr(), 1, UNKNOWN);
        }
        assert_eq!(&buf[..11], b"hello you!\0");
    }

    /// glibc 2.39's texts and names for every number from -2 to 139
    /// (`strname_oracle.txt`), to the letter: `strerror`'s -- "Unknown error
    /// N" for a number no error has -- and `strerrorname_np`'s and
    /// `strerrordesc_np`'s, NULL included.
    #[test]
    fn error_names_and_texts_are_glibcs() {
        use core::ffi::CStr;
        const ORACLE: &str = include_str!("strname_oracle.txt");
        let text = |p: *const u8| -> Option<String> {
            // SAFETY: each returns NULL or a static NUL-terminated string.
            (!p.is_null()).then(|| {
                unsafe { CStr::from_ptr(p.cast()) }
                    .to_str()
                    .unwrap()
                    .to_string()
            })
        };
        let mut seen = 0;
        for line in ORACLE.lines().filter(|l| !l.starts_with('#')) {
            let (lhs, want) = line.split_once(" = ").unwrap();
            let (func, n) = lhs.split_once(' ').unwrap();
            let n: i32 = n.parse().unwrap();
            let want = (want != "NULL").then(|| want.trim_matches('"').to_string());
            let got = match func {
                "strerror" => text(strerror(n)),
                "strerrorname_np" => text(strerrorname_np(n)),
                "strerrordesc_np" => text(strerrordesc_np(n)),
                _ => continue,
            };
            assert_eq!(got, want, "{func}({n})");
            seen += 1;
        }
        assert_eq!(seen, 3 * 142, "the oracle's error lines");
    }

    /// `memfrob` exclusive-ors with 42 and so undoes itself; zero bytes do
    /// nothing.
    #[test]
    fn memfrob_is_its_own_inverse() {
        let mut buf = *b"Hello, world\0";
        let p = buf.as_mut_ptr().cast::<core::ffi::c_void>();
        // SAFETY: 12 of the buffer's 13 bytes.
        assert_eq!(unsafe { memfrob(p, 12) }, p);
        assert_eq!(buf[0], b'H' ^ 42);
        assert_eq!(buf[12], 0, "past n: untouched");
        // SAFETY: as above.
        unsafe { memfrob(p, 12) };
        assert_eq!(&buf, b"Hello, world\0");
        // SAFETY: no bytes.
        assert!(unsafe { memfrob(core::ptr::null_mut(), 0) }.is_null());
    }

    /// `strfry` keeps the bytes and the length, moves them, and leaves NULL,
    /// the empty string and a single byte alone.
    #[test]
    fn strfry_permutes() {
        let mut s = *b"abcdefghijklmnopqrstuvwxyz\0";
        let mut moved = false;
        for _ in 0..8 {
            // SAFETY: a writable NUL-terminated string.
            assert_eq!(unsafe { strfry(s.as_mut_ptr()) }, s.as_mut_ptr());
            let mut sorted = s[..26].to_vec();
            sorted.sort_unstable();
            assert_eq!(&sorted, b"abcdefghijklmnopqrstuvwxyz", "the same bytes");
            assert_eq!(s[26], 0);
            moved |= &s[..26] != b"abcdefghijklmnopqrstuvwxyz";
        }
        assert!(moved, "eight shuffles of 26 letters all left them in order");
        let mut one = *b"x\0";
        // SAFETY: as above.
        unsafe { strfry(one.as_mut_ptr()) };
        assert_eq!(&one, b"x\0");
        // SAFETY: NULL is allowed.
        assert!(unsafe { strfry(core::ptr::null_mut()) }.is_null());
    }

    /// `sys_errlist` is `strerror`'s table: the same pointer for every
    /// error, "Unknown error" in the two gaps, and `sys_nerr` its length.
    #[test]
    fn sys_errlist_is_strerrors_table() {
        assert_eq!(sys_errlist.len(), usize::try_from(sys_nerr).unwrap());
        for (n, entry) in sys_errlist.iter().enumerate() {
            let n = i32::try_from(n).unwrap();
            if error_text(n).is_some() {
                assert_eq!(entry.0, strerror(n), "sys_errlist[{n}]");
            } else {
                // SAFETY: a static NUL-terminated string.
                let t = unsafe { core::ffi::CStr::from_ptr(entry.0.cast()) };
                assert_eq!(t.to_bytes(), b"Unknown error", "sys_errlist[{n}]");
            }
        }
        // SAFETY: as above.
        let fifteen = unsafe { core::ffi::CStr::from_ptr(sys_errlist[15].0.cast()) };
        assert_eq!(
            fifteen.to_bytes(),
            b"Block device required",
            "it said Unknown error"
        );
    }
}

// ===========================================================================
// glibc FORTIFY_SOURCE _chk functions
// ===========================================================================
//
// Programs compiled against glibc's headers with `-D_FORTIFY_SOURCE` call
// these `__*_chk` wrappers instead of the plain functions, passing the
// destination object's size (`destlen`; `(size_t)-1` when unknown). Each
// checks that the copy fits and calls `__chk_fail` -- glibc's message, then
// `abort()` -- when it does not, *before* writing anything. Copies abort
// rather than clamp: a copy shorter than asked is a different bug, not a safe
// one (`crate::fortify` has the rule, and which `_chk`s clamp instead).
//
// They ignored `destlen` until 2026-09-25 (known-issues.md
// TD-D-FORTIFY-MEM-AND-STR-CHK-IGNORE-THE-OBJECT-SIZE).

/// `__memcpy_chk` — fortified `memcpy`.
///
/// # Safety
///
/// Same as `memcpy`.  Aborts, through `__chk_fail`, when `n > destlen`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __memcpy_chk(
    dest: *mut u8,
    src: *const u8,
    n: usize,
    destlen: usize,
) -> *mut u8 {
    if !crate::fortify::fits(n, destlen) {
        crate::fortify::__chk_fail();
    }
    unsafe { memcpy(dest, src, n) }
}

/// `__memmove_chk` — fortified `memmove`.
///
/// # Safety
///
/// Same as `memmove`.  Aborts, through `__chk_fail`, when `n > destlen`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __memmove_chk(
    dest: *mut u8,
    src: *const u8,
    n: usize,
    destlen: usize,
) -> *mut u8 {
    if !crate::fortify::fits(n, destlen) {
        crate::fortify::__chk_fail();
    }
    unsafe { memmove(dest, src, n) }
}

/// `__memset_chk` — fortified `memset`.
///
/// # Safety
///
/// Same as `memset`.  Aborts, through `__chk_fail`, when `n > destlen`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __memset_chk(dest: *mut u8, c: i32, n: usize, destlen: usize) -> *mut u8 {
    if !crate::fortify::fits(n, destlen) {
        crate::fortify::__chk_fail();
    }
    unsafe { memset(dest, c, n) }
}

/// `__strcpy_chk` — fortified `strcpy`.
///
/// # Safety
///
/// Same as `strcpy`.  Aborts, through `__chk_fail`, when `src` and its
/// terminator are longer than `destlen`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __strcpy_chk(dest: *mut u8, src: *const u8, destlen: usize) -> *mut u8 {
    if !crate::fortify::fits_with_terminator(unsafe { strlen(src) }, destlen) {
        crate::fortify::__chk_fail();
    }
    unsafe { strcpy(dest, src) }
}

/// `__strncpy_chk` — fortified `strncpy`.
///
/// # Safety
///
/// Same as `strncpy`.  Aborts, through `__chk_fail`, when `n > destlen`:
/// `strncpy` writes exactly `n` bytes, padding with NULs.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __strncpy_chk(
    dest: *mut u8,
    src: *const u8,
    n: usize,
    destlen: usize,
) -> *mut u8 {
    if !crate::fortify::fits(n, destlen) {
        crate::fortify::__chk_fail();
    }
    unsafe { strncpy(dest, src, n) }
}

/// `__strcat_chk` — fortified `strcat`.
///
/// # Safety
///
/// Same as `strcat`.  Aborts, through `__chk_fail`, when the string already
/// in `dest`, `src` and a terminator do not fit `destlen` -- or when `dest`
/// holds no terminator within `destlen` at all.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __strcat_chk(dest: *mut u8, src: *const u8, destlen: usize) -> *mut u8 {
    if !unsafe { crate::fortify::concatenation_fits(dest, strlen(src), destlen) } {
        crate::fortify::__chk_fail();
    }
    unsafe { strcat(dest, src) }
}

/// `__strncat_chk` — fortified `strncat`.
///
/// # Safety
///
/// Same as `strncat`.  Aborts, through `__chk_fail`, when the string already
/// in `dest`, at most `n` bytes of `src` and a terminator do not fit
/// `destlen`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __strncat_chk(
    dest: *mut u8,
    src: *const u8,
    n: usize,
    destlen: usize,
) -> *mut u8 {
    if !unsafe { crate::fortify::concatenation_fits(dest, strnlen(src, n), destlen) } {
        crate::fortify::__chk_fail();
    }
    unsafe { strncat(dest, src, n) }
}

/// `__stpcpy_chk` — fortified `stpcpy`.
///
/// # Safety
///
/// Same as `stpcpy`.  Aborts, through `__chk_fail`, when `src` and its
/// terminator are longer than `destlen`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __stpcpy_chk(dest: *mut u8, src: *const u8, destlen: usize) -> *mut u8 {
    if !crate::fortify::fits_with_terminator(unsafe { strlen(src) }, destlen) {
        crate::fortify::__chk_fail();
    }
    unsafe { stpcpy(dest, src) }
}

/// `__stpncpy_chk` — fortified `stpncpy`.
///
/// # Safety
///
/// Same as `stpncpy`.  Aborts, through `__chk_fail`, when `n > destlen`:
/// `stpncpy` writes exactly `n` bytes, padding with NULs.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __stpncpy_chk(
    dest: *mut u8,
    src: *const u8,
    n: usize,
    destlen: usize,
) -> *mut u8 {
    if !crate::fortify::fits(n, destlen) {
        crate::fortify::__chk_fail();
    }
    unsafe { stpncpy(dest, src, n) }
}

/// `__mempcpy_chk` — fortified `mempcpy`.
///
/// # Safety
///
/// Same as `mempcpy`.  Aborts, through `__chk_fail`, when `n > destlen`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __mempcpy_chk(
    dest: *mut u8,
    src: *const u8,
    n: usize,
    destlen: usize,
) -> *mut u8 {
    if !crate::fortify::fits(n, destlen) {
        crate::fortify::__chk_fail();
    }
    unsafe { mempcpy(dest, src, n) }
}

// ---------------------------------------------------------------------------
// swab — byte pair swap
// ---------------------------------------------------------------------------

/// Copy `nbytes` bytes from `src` to `dest`, swapping adjacent byte pairs.
///
/// POSIX requires `nbytes` to be even.  If `nbytes` is odd, the last
/// byte is silently ignored (not copied).  This matches glibc behavior.
///
/// # Safety
///
/// `src` and `dest` must point to valid memory of at least `nbytes`
/// bytes.  The regions must not overlap.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn swab(src: *const u8, dest: *mut u8, nbytes: isize) {
    if src.is_null() || dest.is_null() || nbytes <= 1 {
        return;
    }
    // Process pairs.
    let pairs = (nbytes as usize) / 2;
    let mut i: usize = 0;
    while i < pairs {
        let off = i.wrapping_mul(2);
        // SAFETY: off < nbytes (since i < pairs = nbytes/2, off = 2*i < nbytes).
        let a = unsafe { *src.add(off) };
        let b = unsafe { *src.add(off.wrapping_add(1)) };
        unsafe {
            *dest.add(off) = b;
        }
        unsafe {
            *dest.add(off.wrapping_add(1)) = a;
        }
        i = i.wrapping_add(1);
    }
}
