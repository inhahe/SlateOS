## D-POSIX-WARN-BROUGHT-ERRORS-DEFINITIONS-WITH-IT — `warn` and `err` called a helper that lived in `error`'s archive member, so a program with its own `error` (gnulib's) that called `warn` linked two; and the libc.a shape gate could not see it (lane D, 2026-09-30) — **Status: FIXED 2026-09-30 (`posix/src/error.rs`, `scripts/check-libc-shape.py`)**

**In short:** a static link copies a library's pieces whole, so a program
that brings its own copy of a libc function must be able to leave the
library's copy out. GNU programs bring their own `error()` -- gnulib's
`error` module defines it wherever the C library has none, as musl has
none -- and ours could be left out only until the program also called
`warn()` or `err()`: those called a helper stored in the same piece as
`error()`, so using them pulled our `error()` in beside the program's, and
the link failed with two definitions. It came in with commit c6d33285a
(2026-09-29), which made `err()` print through `error.rs`'s code.

The shape gate missed it because it looked only at the C names a piece
defines. A linker also pulls a piece in for a Rust helper's mangled name,
which no program can define but any may need. The same blind spot hid an
older case: `glob`'s piece called `fnmatch`'s matcher directly, so a
program with its own `fnmatch` could not use the library's `glob` (fixed in
11a8d6d6b by giving `fnmatch()` a piece of its own).

**Fix:** `put` and `put_cstr` are in `mod output` inside `error.rs` -- an
archive member of their own, with no C name in it -- and both families call
them there. `scripts/check-libc-shape.py`'s new CHECK 5 holds the archive to
the property: no member outside a `STRICT_FAMILIES` family refers, with a
strong undefined symbol, to anything the family's member defines but the
family's own names. On the archive then on main it reported exactly the two
paths above; on this tree it passes, all four families reached by their
names alone. Five self-test cases and four mutants (all killed).

**Where:** `posix/src/error.rs` (`mod output`); `scripts/check-libc-shape.py`
(CHECK 5, `elf_symbols`, `elf_strong_references`).
