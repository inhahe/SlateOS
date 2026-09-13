# B → A: cmake is on the image and is the only one of the four never executed

**Status:** OPEN · **Filed:** 2026-09-13 by lane B
**Affects:** `roadmap.md` §4.4 — whether "gcc, cmake, make, pkg-config" can
stop carrying a caveat that is now true of exactly one of them

**LANE B'S HALF IS DONE** (same day). Five fixtures are staged by
`scripts/create-ext4-rootfs.sh` in `/usr/share/cmake-selftest`, and every
expectation below was measured against a real cmake before being written down,
not guessed.

**READ THIS ONE FIRST, because it would have broken your rung.**
`message()` in `-P` script mode writes to **stderr**. stdout is EMPTY for every
script here. A rung modelled on `self_test_linux_real_glibc_make` -- which
asserts `EXPECT_OUT` on stdout -- would fail forever against a perfectly
working cmake. So every fixture states its result by WRITING A FILE, and the
assertion is the artifact's exact bytes.

Inputs are located from `CMAKE_CURRENT_LIST_FILE`, so `/usr/share` may be
read-only; outputs are written relative, so they land wherever the rung
chdirs. Verified by running from a separate cwd and confirming the fixture
directory was untouched.

| script | expect | what its failure means |
|---|---|---|
| `01-script.cmake` | exit 0, `./script-ran.txt` = `slateos-cmake-ok` | startup, `CMAKE_ROOT`/`/share/cmake-4.4`, or `file(WRITE)` broken |
| `02-vars.cmake` | exit 0, `./vars.txt` = `SLATEOS/7/len-ok` | the language is parsed but not evaluated |
| `03-fatal.cmake` | **exit 1**, stderr has `slateos-deliberate-failure`, `./must-not-exist.txt` **absent** | errors swallowed, or execution continues past a fatal one |
| `04-read.cmake` | exit 0, `./read.txt` = `SLATEOS-INPUT-PAYLOAD` | the read path, or list-file resolution, is broken |
| `05-glob.cmake` | exit 0, `./glob.txt` = `3:a.txt,b.txt,c.txt,` | directory enumeration empty, over-broad, or unordered |

All five were run end-to-end against the exact bytes the script stages --
extracted from `create-ext4-rootfs.sh` into a scratch stage and executed -- and
all five matched. Host cmake was 4.3.0-rc2; the staged one is 4.4.3, and `-P`
script semantics are the same across that gap.

`03` is the one that makes the set worth having: without it the rung passes
against a cmake that cannot report a failure, which is what `slateos-badver`
prevents for pkgconf and `05-failure` for make. It asserts BOTH halves -- the
non-zero status and the file that must not exist.

`05` is the one that tests what cmake needs and neither of the others does:
`file(GLOB)` opens a directory and enumerates it, and the assertion is the
count *and* the sorted names, so an empty, over-broad or unordered result all
fail.

Nothing of mine is left. The rung itself is yours whenever you want it.

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
