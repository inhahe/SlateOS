## B-THE-C-PLUS-PLUS-LINK-LINE-NEEDS-TWO-DECISIONS-AND-ONE-MISSING-FAMILY (lane B, 2026-09-09)

**Status: RESOLVED 2026-09-09, the same day.** A C++ translation unit using
`<string>`, `<vector>` and a real `throw`/`catch` now links for
`x86_64-slateos` against our own `libc.a` plus zig's `libc++`/`libc++abi`/
`libunwind` — **zero undefined symbols, zero duplicates**, a 3.6 MB static
`ET_EXEC`. What follows is the original entry; the three items resolved as:

* **The ABI-ownership decision dissolved rather than being made.** It was not a
  decision at all, it was a packaging defect: our C++ ABI stubs shared an
  object file with `__libc_start_main`, and *every* program extracts that
  member, so the stubs arrived unconditionally and collided. Moved into their
  own inline module (`posix/src/crt.rs`, `mod cxx_abi`), which gives them their
  own archive member — the mechanism `design-decisions.md` §339 already
  established for `asprintf`. A link that brings a real `libc++abi` now simply
  never pulls them.
* **The missing exception classes came with it.** They live in `libc++abi`,
  which the link can now include without conflict.
* **`swprintf`/`vswprintf` and `wcstold`** — implemented, see above and below.

**Not established:** no C++ binary has been *run* on SlateOS. This is the
link-stage result, exactly as the CPython and bash spikes were at their link
stage, and it carries the same caveat: linking proves the symbol surface, not
the behaviour.


**In short:** we can now compile C++ for SlateOS — that was established today
and annotated on `design-decisions.md` §73. Actually *linking* a C++ program
against our own C library turns out to need two small things nobody has
decided, and one function family we do not have. None is hard; all three are
invisible until someone tries the link, which is why they are written down
here rather than discovered again.

**What was measured.** A C++ TU using `<string>`, `<vector>`, `<memory>` and a
`throw`/`catch`, compiled with our own codegen flags (`-mcmodel=large
-fno-pic -fno-pie -fno-builtin -O2`) and linked with `rust-lld` against
`toolchain/sysroot/lib/libc.a` plus zig's prebuilt `libc++.a`,
`libc++abi.a` and `libunwind.a`.

### 1. Our libc already ships a C++ ABI, and it collides

Linking zig's `libc++abi.a` produced **duplicate** symbol errors, not missing
ones: `__cxa_allocate_exception`, `__cxa_begin_catch`, `__cxa_deleted_virtual`
and friends are defined in *both* archives. `posix` has carried a C++ ABI shim
for some time. Dropping zig's `libc++abi.a` left exactly one further duplicate,
`_Unwind_Resume`, so our libc supplies part of the unwinder too.

That is a good position to be in and it is still a decision: **whose C++ ABI
wins.** Ours, and zig's `libc++.a` links against it; or zig's, and posix's shim
is excluded from the link. The two cannot both be in the line. This is not
something a porter should have to work out under time pressure with a wall of
duplicate-symbol errors in front of them, which is the shape it takes today.

### 2. With zig's `libc++abi` dropped, the exception *types* go missing

`typeinfo for std::out_of_range`, `std::out_of_range::~out_of_range()`,
`std::bad_array_new_length` and `__cxa_free_exception` are then undefined —
they live in `libc++abi`, which is exactly the archive that collided. So
option "use ours" is not free: our shim covers the ABI *entry points* but not
the standard exception classes. Whoever decides (1) has to cover these.

### 3. `swprintf`, `vswprintf` and `wcstold` — real libc gaps

These are ours, not a packaging question.

* **`wcstold`** — **FIXED 2026-09-09**, the same day it was found. It was the
  only absent member of a family we already had: `wcstod` and `wcstof` have
  been here all along. Built on `strtold`'s proven pattern — an `f64` core
  named `__wcstold_f64` plus an `%st(0)` assembly thunk, because Rust cannot
  express a `long double` return.
* **`swprintf` / `vswprintf`** — **FIXED 2026-09-09.** Not a second engine:
  the format is narrowed, the existing `format_core` runs exactly as it does
  for `snprintf`, and the result is widened. No translation of conversion
  specifiers is needed, because C gives `%s` a `char *` and `%ls` a
  `wchar_t *` in *both* families. The intermediate needs no buffer of its own
  — the narrow text is formatted into the caller's own buffer and expanded in
  place, back to front, which is safe rather than lucky: character `i` starts
  at byte `s_i` with `i <= s_i <= 4i`, and its destination slot is exactly
  `[4i, 4i+4)`, so the write never reaches below the byte it was decoded
  from. Truncation follows `swprintf`'s rule (negative) and not
  `snprintf`'s (the would-be length), which is why the two share an engine
  and not a return path.

**Why this matters more than its size.** §73 defers YSH until a C++/slateos
toolchain exists, and that prerequisite has now fired. The entry moves YSH
from blocked-on-toolchain to blocked-on-effort — and this is the effort. It is
a short list, and it is much better to have it as a list than as a surprise.

**How to reproduce**, since the measurement is cheap and will need redoing
when any of the three moves: compile any C++ TU with the slateos flags, link
it with `rust-lld -static --no-dynamic-linker` against
`toolchain/sysroot/lib/libc.a` and zig's `libc++.a`, and read the undefined
and duplicate lists. Zig's archives are built on demand into its global cache
by any `zig c++ -static` build.

---
