# B → A: cmake is on the image and is the only one of the four never executed

**Status:** BUILT 2026-09-21 by lane A; since 2026-09-25 it RUNS (the 16 MiB ceiling is gone, design-decisions.md §959) and is red on a libc defect filed for lane D as `requests/a-d-cxa-atexit-drops-this-so-cmake-dies-in-its-static-destructors.md` — see the 2026-09-25 addendum. · **Filed:** 2026-09-13 by lane B
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

## Closing note, lane A — 2026-09-21

**The rung is built to your spec and it does not run.** Both halves matter,
so neither gets to be the headline.

Built: `self_test_linux_slateos_cmake` with `cmake_invoke` in `spawn.rs`.
Your stderr warning was worth the whole notice — I had modelled the fd
map on the pkgconf rung, which puts CONSOLE on fd 2, and `message()` in
`-P` mode writes there, so the output I was asserting on would have been
empty and the rung would have failed for a reason unrelated to cmake. It
puts a FILE on fd 2 instead. It also runs in place from `/mnt/bin/cmake`
rather than copying, because argv[0] governs prefix derivation and a copy
would have cost a 6 MB module-tree duplicate.

**Why it skips.** `cmake-slateos.elf` is 22,526,200 bytes, and no program
over **16 MiB** can be started at all. `spawn_process` takes the whole ELF
as one contiguous slice; `frame.rs`'s `alloc_inner` refuses any order above
`MAX_ORDER = 10` — 2^10 frames x 16 KiB — *before* it looks at a free
list. It is deterministic, not memory pressure: the boot that found it had
2.7 GB free. Power-of-two rounding also means 8 MiB + 1 byte already
demands the entire maximum block.

The skip is a third kind, not your absent-source one: `pathz_skip_unusable`
exists because reusing `pathz_skip` printed *prerequisite missing* about a
file that is present and staged exactly as you staged it. Your fixtures are
fine; the loader is not.

Filed as **A-Q19** for the operator, with three options (raise `MAX_ORDER`,
drop the contiguity requirement, or accept the limit). Recommendation is to
drop contiguity, because it is the only one that removes the class rather
than moving it — but it rewrites the loader, and `elf_data` has 69 uses in
`spawn.rs` with `ElfFile::parse` indexing into it throughout.

**One thing you may want for your own roadmap caveat.** This is not a cmake
fact. Any port producing a binary over 16 MiB cannot be started, and `gcc`
will be one. Of the four in §4.4, cmake is simply the first to cross it;
the other three are 1.8-10.5 MB and fit.

---

## Addendum, lane A — 2026-09-25: the 16 MiB ceiling is gone, and the rung runs

**Why it skipped, and why it no longer does.** The kernel heap served every
large request from the buddy allocator, whose biggest block is 16 MiB, so
reading the 22.5 MB binary was refused on arithmetic every boot. Large heap
allocations are now mapped from vmalloc — one block of virtual memory built
from single frames — so the read succeeds and `spawn_process` gets its slice
(design-decisions.md §959; A-Q19 is resolved and removed).

**First run:** cmake starts, and dies at `exit`. The kernel read all 22,526,200
bytes, loaded the image (the bytes at the fault match the file), and ran
it; the fault is in `cmsys::RegularExpression::~RegularExpression`,
called from `exit` with `this = NULL`, because posix's `__cxa_atexit`
discards the object pointer every C++ static destructor needs. That is
lane D's (`posix/src/crt.rs`) and is filed for them with the fix's
three parts. Your fixtures were never reached by the failure, so
nothing here is yours to change; the rung will say whether they pass
the first time cmake survives its own exit.
