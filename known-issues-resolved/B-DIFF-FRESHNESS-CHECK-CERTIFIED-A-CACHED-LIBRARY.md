## `B-DIFF-FRESHNESS-CHECK-CERTIFIED-A-CACHED-LIBRARY` — the harnesses' anti-stale-build guard passed a build three commits old — ✅ FIXED 2026-08-24 (lane B)

*(lane B, 2026-08-24)*

**In short:** every diff harness begins by checking that the binaries it is
about to compare were actually built from the source now in the tree — the
check exists because comparing against a stale build produces a *false green*,
the worst outcome a test can have. On 2026-08-24 the check passed against a
build whose library half was three commits out of date, and a harness reported
sixteen failures for a bug that had already been fixed. The check was looking
at the wrong file.

### Why it passed

The check asked "is any `.rs` file newer than the binaries?" Cargo had relinked
every binary — so every binary's mtime was newer than every source file, and the
check was satisfied — while replaying the `coreutils` **library** unit from
cache. The binaries were new; the code inside them was not.

### How it was noticed

Not by the check, and not by the harness. `interleave-diff.sh` reported 16
differences against a fix that was demonstrably present in the tree, and the
one utility that passed was `cat` — which flushes explicitly and would pass
either way. The decisive evidence was in the build log: a replayed
`warning: coreutils (lib) generated 1 warning` for a `dead_code` function that
had been unused **only in the previous edit of the file**. A cached lib unit
announcing itself. `cargo clean -p coreutils` was the entire cure, and the same
harness then passed 21 for 21.

A separate controlled experiment confirmed cargo *does* rebuild after
`touch userspace/coreutils/src/stdfd.rs`, so this is not simply "cargo is
broken" — the cache had been damaged earlier, exactly as in the
`cannot find function close_stdout` incident recorded further up this file.

### The fix

`diff_first_stale` in `scripts/diff-wsl.sh` now checks the package's library
artifact — the newest `deps/lib<pkg>-*.rlib` — **before** the binaries, and
`diff_lib_artifact` was added to find it. A stale lib is reported and cured by
the existing `cargo clean -p` + rebuild + recheck path. Verified afterwards
that neither harness false-positives: two consecutive runs of
`interleave-diff.sh` and one of `write-error-diff.sh` cleaned nothing.

### The general lesson, which outlives this bug

**A freshness check must look at the artifact that holds the code, not at the
artifact that gets rewritten.** Link steps are cheap and run often; they touch
the file a naive mtime check watches while leaving the compiled code beneath it
untouched. Any future check of this shape — for another package, another
language, another build system — should be pointed at the compilation unit, not
at the final product.
