# Lane D -> lane A: run `scripts/check-libc-declared.py` in the boot test

**Filed:** 2026-09-28 by lane D. **For:** lane A (`scripts/boot-test.sh`).
**Status:** OPEN.

**Updated 2026-09-29:** a second gate, `scripts/check-libc-prototypes.py`,
now runs beside it -- the other half of the same question -- and a third,
`scripts/check-libc-overlay.py`, after them; this asks for all three. See
the last two sections.

**In short:** a new gate checks that every function musl's headers declare
exists in the C library. On 2026-09-28 126 did not -- among them all of C11's
`<threads.h>` and the two helpers every `pthread_cleanup_push` expands to, so
any C program using those compiled and then failed to link. Lane D fixed 58
and added the gate, which refuses a new gap. It runs where the C library is
built (`toolchain/build-sysroot.ps1`); please also run it in the boot test,
beside `check-libc-shape.py`, so a push that removes a function cannot pass.

## What the gate needs and answers

- `python scripts/check-libc-declared.py [path/to/libc.a]` -- the archive
  defaults to `toolchain/sysroot/lib/libc.a`, as `check-libc-shape.py`'s does.
- zig, found as `check-libc-abi.py` finds it (`FASTPY_ZIG`, then `PATH`), to
  preprocess musl's 182 headers; it takes about a minute.
- Exit 0 clean; 1 a declared function is missing and not in its baseline, or a
  baseline entry is now defined (a stale exemption); 2 could not check (no
  archive, an unreadable one); 3 could not run (no zig) -- the code
  `run-checker.sh` files as skipped.
- `--self-test` runs its fixtures: the declaration pattern and the ratchet in
  both directions.

## The pin to delete

`scripts/check-gates-are-wired.py` carries a `PINNED` entry for it, pointing
here; the commit that wires it should delete that entry.

## The second gate (2026-09-29)

`python scripts/check-libc-prototypes.py` checks that each of those functions
also *takes and returns* what its declaration says, by the x86-64 calling
convention: on its first run it found eleven that did not (a four-byte
`timer_t` against the header's eight, `wctype_t` likewise, `readahead` and
`__fpurge` returning the wrong width -- known-issues.md ->
D-POSIX-PROTOTYPES-DISAGREED-WITH-THEIR-DEFINITIONS). It reads `posix/src`
and the headers, not the archive, so it can run anywhere in the boot test;
it takes about half a minute the first time and a few seconds after, the
parsed headers cached in `target/`. Same exit codes as above, same
`--self-test`, and its own `PINNED` entry in `check-gates-are-wired.py` to
delete with the wiring.

## The third gate (2026-09-29)

`python scripts/check-libc-overlay.py` checks `posix/include`, the header
overlay C is now compiled with (`-I posix/include`, in front of musl's
headers; design-decisions §1141), against glibc 2.39's headers: every
header compiles in sixteen feature-macro settings, and each of the 195
declarations it adds is visible exactly where glibc's is, with glibc's type.
It needs zig, as the other two do, and no archive; about a minute. Same exit
codes, `--self-test`, and a `PINNED` entry of its own in
`check-gates-are-wired.py`.

## And the converse (2026-09-29, later)

`check-libc-declared.py` now asks the question the other way round as well:
is every public name `libc.a` defines declared by a header? Its first answer
was fifteen names that should never have been exported, fixed the same day
(known-issues.md -> D-POSIX-LIBC-EXPORTED-NAMES-NO-HEADER-DECLARES). Same
command and exit codes; it takes about a minute and a quarter now.
