## B-THREE-LIBC-SYMBOLS-WERE-DEFINED-TWICE-AND-THE-WORSE-ONE-WON (lane B, 2026-09-13) -- FIXED, gate 41

`posix` and `toolchain/stubs` both defined `killpg`,
`posix_spawnattr_setsigdefault` and `syscall` as `no_mangle` C symbols. Their
archives -- `libc.a` and `libstubs.a` -- are both linked into every userspace
binary, and `.cargo/config.toml` passes `--allow-multiple-definition`, so this
was never a link error. The linker takes whichever it sees first, and
`-lstubs` is passed before rustc's own `-lc`.

The stub was the worse half of each pair:

| symbol | the stub | posix |
|---|---|---|
| `killpg` | `-1`, and **no errno set** despite a doc comment saying `errno=ENOSYS`, so a caller's `perror()` printed a stale unrelated error | validates the group, `checked_neg`, delegates to `kill()` |
| `posix_spawnattr_setsigdefault` | `0` -- success, no-op. The caller believed the child's signals would be reset to default | real, and already tested |
| `syscall` | **four** parameters where posix's takes seven, so three argument registers were dropped. Its futex arm hardcoded `timeout=NULL`, so a `FUTEX_WAIT` with a timeout waited forever | seven parameters, nine tests |

### The policy existed and was half-followed

The stubs file's own header says: *"As our POSIX layer grows, symbols should be
moved from here to proper implementations in the posix crate."* The moving
happened three times. The deleting never did, and nothing noticed because the
duplicate is silent by configuration.

### Deleting the stub first would have been a regression

posix's `syscall` did not route futex (202); the stub did. So the fix had to
add futex to posix's table -- with all six arguments, which the stub could not
pass -- before the stub could go. That is the kind of thing a sweep that
deletes everything stale gets wrong.

### Fixed and gated

`scripts/check-duplicate-exports.py`, gate 41. It reads SOURCE, not the
archives, because the sysroot is built by hand and `libstubs.a` was ten hours
stale when this was found -- and because the archives carry 294 duplicate
unmangled symbols of which 291 are legitimately in both (compiler-rt, libm),
so a baseline that size would hide the next real one. Baseline is empty.
