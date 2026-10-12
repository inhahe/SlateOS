## D-POSIX-DSO-HANDLE-WAS-STRONG-SO-NO-GCC-LINK-COULD-SUCCEED — `libc.a` defined `__dso_handle` strongly, beside `_start`, so every program GCC linked against it had two definitions and failed to link (lane D, 2026-10-07) — FIXED 2026-10-07
**Status:** FIXED 2026-10-07 — `posix/src/crt.rs` defines `__dso_handle` weak on SlateOS, and `scripts/check-libc-shape.py` CHECK 7 refuses a strong one

**In short:** `__dso_handle` is the word a C++ program's static destructors
are registered against (`__cxa_atexit(dtor, obj, &__dso_handle)`). Neither
glibc nor musl defines it: GCC's startup objects `crtbegin*.o` do, and GCC
names one of them in every link. This library defined it too -- the zig
links every SlateOS program is made with name no crtbegin, so something has
to -- and it defined it strongly, in the archive member every program
extracts for `_start`. So every program GCC linked against our `libc.a`
had two definitions, and GNU ld refused it: "multiple definition of
`__dso_handle'". No GCC-built program could have linked, C or C++, and
libstdc++'s configure, whose tests link programs, would have failed with
them.

**How it was found.** Reading `libc.a`'s members for what GCC's crt objects
and libraries define too, while building GCC for SlateOS
(`scripts/gcc-spike/cross.sh`), 2026-10-07, before any GCC link had been
attempted -- then confirmed: `ld -static crtbeginT.o t.o -lc crtend.o`, with
Ubuntu's GCC 13 `crtbeginT.o` and GNU ld 2.42, failed exactly so.

**The fix.** `__dso_handle` is defined in assembly on SlateOS, weak and
pointer-sized as crtbegin's is (`posix/src/crt.rs`): where no crtbegin is
linked (zig's links) it is the definition, and where one is (GCC's) crtbegin's
wins. The host's tests keep a Rust static. The same link with the symbol
weakened succeeds, takes crtbegin's `__dso_handle`, our `_start`, and the
SlateOS ABI note. `check-libc-shape.py` gained CHECK 7, "what a compiler's own
startup objects define, the archive defines only weakly", with self-test
cases for a strong and a weak definition and two mutants the sweep kills.

**Also looked at, and not a clash.** The C++ ABI stand-ins
(`__cxa_throw`, `__gxx_personality_v0`, `_Unwind_Resume` ...) are strong,
but sit in a member of their own, which a GCC link does not extract:
libstdc++ and libgcc_eh come before `-lc` in its link line and define them
first.

**What it could have cost.** Every program compiled by GCC for SlateOS --
on Linux by the cross compiler, or on SlateOS itself -- and the
target libraries' own configuration.
