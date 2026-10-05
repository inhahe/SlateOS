## D-QEMU-TCG-EMULATES-TWO-X87-THINGS-WRONGLY — under QEMU without hardware virtualisation, `fprem1` reports no quotient and `fsin`/`fcos`/`fsincos`/`fptan` answer in double precision (lane D, 2026-09-28) — **Status: WORKED AROUND in our libc; open for any other x87 code, and not ours to fix**

**In short:** SlateOS's boot test runs on QEMU's software CPU (TCG), and so
may SlateOS itself. Two of the x87 floating-point instructions do not behave
there as they do on a real processor. Our C library no longer depends on
either, but a program that does -- one linked against glibc or musl, which
do -- gets wrong answers under that emulation and right ones on hardware.

| Instruction | Hardware | QEMU TCG (11.x and master, `target/i386/tcg/fpu_helper.c`) | Who meets it |
|---|---|---|---|
| `fprem1` | reports the rounded quotient's low three bits in C0, C3, C1 | leaves all three clear: `floatx80_modrem` is passed `mod ? quotient : NULL`, so the quotient is worked out for `fprem` only | `remquol` in musl and glibc: `remquol(10, 3)` gives quotient 0, not 3 |
| `fsin`, `fcos`, `fsincos`, `fptan` | 64-bit significand | computed by the host's `sin`/`cos`/`tan` on a `double`, 53 bits | `long double` trigonometry built on them: glibc's x86-64 `sinl`, `cosl` and `tanl` use them; musl's `ld80` ones, and ours, are software |

**How it was found:** `services/ctest-longdouble` 72 failed in the boot test
of lane-d `49b08d0df` -- the first boot to run the `long double` `<math.h>`
checks -- while the host tests, on hardware, passed. Reading QEMU's source
found the null pointer.

**What our libc does:** `mathl::remquol` takes the truncated quotient's bits
from `fprem`, which QEMU does report, and adds one when `fprem1`'s remainder
differs from `fprem`'s (lane-d `bdf1ee181`); `ld80.rs`'s `partial_remainder`
says to trust only `fprem`'s bits. Nothing in `posix/src` issues `fsin`,
`fcos`, `fsincos` or `fptan`.

**The proper fix** is QEMU's: pass the quotient to `floatx80_modrem` for
`fprem1` too (its REM path already computes the quotient, and an earlier
implementation reported it), and implement the four trigonometric instructions in
`floatx80` as `fpatan`, `fyl2x` and `f2xm1` were in 2020. Worth reporting
upstream if the operator wants it reported; nothing here depends on it.
