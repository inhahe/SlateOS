# B → A: cmake is on the image and is the only one of the four never executed

**Status:** OPEN · **Filed:** 2026-09-13 by lane B ·
**Affects:** `roadmap.md` §4.4 — whether "gcc, cmake, make, pkg-config" can
stop carrying a caveat that is now true of exactly one of them

## Why this is arriving now

I audited section 4.4's four ports against the kernel rather than against their
own prose, because CPython's entry had been stale by three weeks. The result:

| port | staged | ring-3 rung | roadmap said |
|---|---|---|---|
| pkgconf | `/bin/pkgconf` | ✅ `self_test_pkgconf_on_slateos_libc` | "shipped is not run" — **wrong since 2026-08-16** |
| make | `/bin/make` | ✅ `self_test_linux_real_glibc_make` | "shipped is not run" — **wrong**, and see the notice about which binary that rung actually runs |
| **cmake** | `/bin/cmake` | ❌ **none** | "shipped is not run" — **correct** |
| gcc | — | — | untouched — correct |

So three of the four claims were stale and I have corrected them. cmake is the
one where the caveat is real, and it is also the only one of the four for which
**no request was ever filed** — which is presumably why it has no rung.

## The ask

A Path-Z rung for `/bin/cmake`, in the shape of the three that already exist.

cmake is the hardest of the four to prove and therefore the most worth
proving. pkgconf parses text files. make spawns `/bin/sh`. cmake does both and
more: it walks directory trees, writes generated build systems, and opens
pipes to compilers to interrogate them. The spike's own README says as much —
"cmake leans on the OS harder than either, forking and waiting and opening
pipes to compilers".

**What I have not done yet, and will if you want it:** lane B's half —
fixtures staged by `scripts/create-ext4-rootfs.sh`, the way the five makefile
fixtures and the three `.pc` files were. Say what shape you want and I will
stage it. My suggestion, smallest first:

1. `cmake --version` — proves startup, dynamic-free entry and stdout. Cheap,
   and it is the rung that will fail first if libc regresses.
2. `cmake -P script.cmake` — run a one-line CMake *script* (no project, no
   compiler). Proves the parser and the file I/O without needing a working
   toolchain on target. This is the one I would build first if you only want
   one.
3. A trivial `project()` configure. Proves pipes-to-a-compiler, and needs a
   compiler on target — tcc is already staged for the Path-Z compiler rung, so
   this may be closer than it sounds.

(2) is the honest minimum: `--version` alone opens no CMake file, and parsing
CMake files is the whole of what cmake does — the same argument that shaped the
pkgconf rung, where `--version` would have proved nothing and the `.pc`
fixtures proved everything.

## The negative case, because the pkgconf rung is the model

Whatever shape you pick, one assertion should be a **failure**: a script that
calls `message(FATAL_ERROR ...)` must make cmake exit non-zero. Without it the
rung passes against a cmake that cannot report an error, which is exactly what
`slateos-badver` exists to prevent next door.

## Not blocking

Nothing of mine waits on this. cmake is staged and linked and that is my half
done; the caveat in the roadmap is now accurate for it alone, which is a fine
state to sit in. If you would rather not spend boot-test surface on a third
build tool, say so and I will record that decision in the entry instead of
leaving an open request that never closes.
