//! How fast the C library's memory and string primitives are: `memcpy`,
//! `memmove` (both directions), `memset`, `memcmp`, `memchr` and `strlen`,
//! each at the sizes programs use them at.
//!
//! These are the hottest functions in any program: every `Vec` and `String`
//! copy in the Rust userland goes through this `memcpy` (the sysroot builds
//! `compiler_builtins` without its `mem` feature, so `libc.a` supplies the
//! symbol), and every C string through this `strlen`.  Until 2026-10-06 they
//! were byte loops -- one byte an iteration, which no compiler turned into
//! anything better (`memcpy`'s loop compiled to a `movb` pair, `strlen`'s to
//! a `cmpb`).
//!
//! Run with:
//!
//! ```sh
//! cargo bench -p posix --target x86_64-pc-windows-gnu --bench mem
//! ```
//!
//! Each line is the time per call and the bytes per nanosecond (GB/s).  The
//! numbers are this machine's; they say how the functions compare with each
//! other and with what they were, not what SlateOS on other hardware gets.
//!
//! Measured on the development machine, release build, 2026-10-06, GB/s --
//! while other builds ran, so each figure is good to about ±30%:
//!
//! | call | 64 B | 1 KiB | 64 KiB | 1 MiB |
//! |---|---|---|---|---|
//! | `memcpy`, byte loop | 1.7 | 1.6 | 1.3 | 1.7 |
//! | `memcpy` | 12.0 | 20.2 | 26.2 | 21.5 |
//! | `memmove` backward, byte loop | 1.5 | 1.3 | 0.4 | 0.8 |
//! | `memmove` backward | 8.6 | 14.7 | 13.2 | 14.4 |
//! | `memcmp` (equal), byte loop | 1.0 | 0.5 | 1.1 | 1.1 |
//! | `memcmp` (equal) | 2.2 | 3.6 | 5.8 | 6.0 |
//! | `memchr` (absent), byte loop | 1.2 | 0.9 | 0.7 | 0.9 |
//! | `memchr` (absent) | 2.5 | 6.5 | 5.9 | 4.0 |
//! | `strlen`, byte loop | 3.3 | 3.3 | 4.2 | 0.5 |
//! | `strlen` | 10.3 | 9.3 | 15.0 | 13.0 |
//!
//! `memset` has no "before" worth printing here: on the host the byte loop is
//! not the `memset` symbol, so LLVM was free to rewrite it, and it ran at up
//! to 77 GB/s.  On SlateOS, where it is the symbol, LLVM leaves such a loop
//! alone, and it compiled to one `movb` an iteration.  Now: 9.3 GB/s at 64
//! bytes, 33.5 at 1 KiB.
//!
//! The string functions built on them, the same day (each reads the whole
//! string: the byte sought is absent, the strings compared are equal):
//!
//! | call | 64 B | 1 KiB | 64 KiB | 1 MiB |
//! |---|---|---|---|---|
//! | `strnlen`, byte loop | 1.4 | 1.8 | 2.3 | 2.3 |
//! | `strnlen` | 7.4 | 12.2 | 9.7 | 10.9 |
//! | `strchr`, byte loop | 1.3 | 1.3 | 1.3 | 1.2 |
//! | `strchr` | 7.9 | 12.0 | 14.6 | 11.8 |
//! | `strrchr`, byte loop | 1.8 | 1.8 | 1.9 | 1.5 |
//! | `strrchr` | 6.7 | 11.4 | 11.4 | 14.0 |
//! | `strcmp`, byte loop | 2.5 | 2.0 | 2.3 | 1.7 |
//! | `strcmp` | 4.0 | 9.3 | 6.4 | 5.2 |
//! | `strcpy`, byte loop | 1.5 | 2.0 | 1.9 | 1.9 |
//! | `strcpy` | 8.0 | 16.6 | 17.1 | 13.6 |
//! | `strstr` (`x{31}y`), every place tried | 0.04 | 0.04 | 0.03 | 0.05 |
//! | `strstr` (`x{31}y`), Two-Way | 0.21 | 0.45 | 0.45 | 0.38 |
//!
//! `strspn` is about what the loop it replaced was here, 1 GB/s: this set's
//! first byte is the one that matches, the loop's best case.  The loop's
//! cost grew with the set; the bit set's does not.  `strstr`'s real gain is
//! not in the table: trying every place costs the needle's length times the
//! haystack's, so a 10 KiB needle that nearly matches everywhere in a
//! megabyte was ten billion comparisons, where Two-Way's are about two
//! million.

// A measuring harness, not shipped code: its sizes and offsets are
// constants chosen to fit the buffers, and a panic would be a bench that
// stopped, as a test's would.
#![allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]

use std::hint::black_box;
use std::time::Instant;

/// Time `f` over enough calls to take about 50 ms, and print its rate.
fn measure(name: &str, bytes: usize, mut f: impl FnMut()) {
    // Warm the caches and the branch predictors.
    for _ in 0..100 {
        f();
    }
    let mut calls = 1u64;
    loop {
        let started = Instant::now();
        for _ in 0..calls {
            f();
        }
        let elapsed = started.elapsed();
        if elapsed.as_millis() >= 50 || calls >= 1 << 30 {
            let ns = elapsed.as_secs_f64() * 1e9 / calls as f64;
            let rate = if ns > 0.0 { bytes as f64 / ns } else { 0.0 };
            println!("{name:<22} {bytes:>8} B  {ns:>12.1} ns/call  {rate:>8.2} GB/s");
            return;
        }
        calls *= 4;
    }
}

fn main() {
    let sizes = [8usize, 16, 32, 64, 256, 1024, 4096, 65536, 1 << 20];
    let max = *sizes.last().unwrap_or(&0);
    // Room for every size, offset by a few bytes so nothing is conveniently
    // aligned, plus space for the overlapping moves.
    let mut a = vec![0x5a_u8; max + 64];
    let mut b = vec![0xa5_u8; max + 64];

    for &n in &sizes {
        let (src, dst) = (a.as_ptr().wrapping_add(3), b.as_mut_ptr().wrapping_add(5));
        measure("memcpy", n, || {
            // SAFETY: both buffers hold `n` bytes past these offsets, and
            // they are two allocations, so they do not overlap.
            unsafe { black_box(posix::string::memcpy(black_box(dst), black_box(src), n)) };
        });
    }
    for &n in &sizes {
        let p = a.as_mut_ptr();
        measure("memmove forward", n, || {
            // SAFETY: `a` holds `n + 64` bytes from `p`; the regions overlap,
            // destination below source, which memmove allows.
            unsafe {
                black_box(posix::string::memmove(
                    black_box(p),
                    black_box(p.wrapping_add(7)),
                    n,
                ))
            };
        });
        measure("memmove backward", n, || {
            // SAFETY: as above, destination above source.
            unsafe {
                black_box(posix::string::memmove(
                    black_box(p.wrapping_add(7)),
                    black_box(p),
                    n,
                ))
            };
        });
    }
    for &n in &sizes {
        let dst = b.as_mut_ptr().wrapping_add(1);
        measure("memset", n, || {
            // SAFETY: `b` holds `n` bytes past this offset.
            unsafe { black_box(posix::string::memset(black_box(dst), 0x33, n)) };
        });
    }
    // memcmp of equal buffers: the whole length is compared.
    b[..max + 64].copy_from_slice(&a[..max + 64]);
    for &n in &sizes {
        let (x, y) = (a.as_ptr().wrapping_add(3), b.as_ptr().wrapping_add(3));
        measure("memcmp (equal)", n, || {
            // SAFETY: both hold `n` bytes past these offsets.
            unsafe { black_box(posix::string::memcmp(black_box(x), black_box(y), n)) };
        });
    }
    // memchr for a byte that is not there: the whole length is searched.
    for &n in &sizes {
        let s = a.as_ptr().wrapping_add(1);
        measure("memchr (absent)", n, || {
            // SAFETY: `a` holds `n` bytes past this offset.
            unsafe { black_box(posix::string::memchr(black_box(s), 0x00, n)) };
        });
    }
    // strlen of a string of `n` bytes: its terminator at the end.
    for &n in &sizes {
        a[1..=n].fill(b'x');
        a[n + 1] = 0;
        let s = a.as_ptr().wrapping_add(1);
        measure("strlen", n, || {
            // SAFETY: a NUL-terminated string of `n` bytes.
            unsafe { black_box(posix::string::strlen(black_box(s))) };
        });
        a[n + 1] = 0x5a;
    }

    // The scanners, over strings of `n` bytes ('x' throughout, terminated):
    // each reads the whole string -- the byte sought is absent, the strings
    // compared are equal.
    for &n in &sizes {
        a[1..=n].fill(b'x');
        a[n + 1] = 0;
        b[3..=n + 2].fill(b'x');
        b[n + 3] = 0;
        let s = a.as_ptr().wrapping_add(1);
        let t = b.as_ptr().wrapping_add(3);
        measure("strnlen", n, || {
            // SAFETY: a NUL-terminated string.
            unsafe { black_box(posix::string::strnlen(black_box(s), usize::MAX)) };
        });
        measure("strchr (absent)", n, || {
            // SAFETY: a NUL-terminated string.
            unsafe { black_box(posix::string::strchr(black_box(s), i32::from(b'q'))) };
        });
        measure("strrchr (absent)", n, || {
            // SAFETY: a NUL-terminated string.
            unsafe { black_box(posix::string::strrchr(black_box(s), i32::from(b'q'))) };
        });
        measure("strcmp (equal)", n, || {
            // SAFETY: NUL-terminated strings, at different alignments.
            unsafe { black_box(posix::string::strcmp(black_box(s), black_box(t))) };
        });
        measure("strspn", n, || {
            // SAFETY: NUL-terminated strings.
            unsafe { black_box(posix::string::strspn(black_box(s), c"xyz".as_ptr().cast())) };
        });
        measure("strstr (x{31}y)", n, || {
            // SAFETY: NUL-terminated strings.  The needle matches 31 bytes
            // at every place and then fails: 32 comparisons a place for a
            // search that tries every place.
            unsafe {
                black_box(posix::string::strstr(
                    black_box(s),
                    c"xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxy".as_ptr().cast(),
                ))
            };
        });
        b[n + 3] = 0x5a;
        let d = b.as_mut_ptr().wrapping_add(5);
        measure("strcpy", n, || {
            // SAFETY: a NUL-terminated string of `n` bytes, and room for it.
            unsafe { black_box(posix::string::strcpy(black_box(d), black_box(s))) };
        });
        a[n + 1] = 0x5a;
        b.fill(0xa5);
    }
}
