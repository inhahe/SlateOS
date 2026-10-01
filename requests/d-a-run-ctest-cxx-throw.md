# D → A: please run `ctest-cxx-throw` — a C++ exception thrown and caught on SlateOS

**Status:** open — for lane A; nothing else needed first.

**From:** lane D · **To:** lane A · **Filed:** 2026-09-29

## In short

No C++ exception could ever be caught on SlateOS. A C++ program here links
zig's libunwind, which finds a program's unwind tables by asking the C
library's `dl_iterate_phdr` -- and that returned 0 without calling back, so
the unwinder found nothing and every `throw` ended in `std::terminate`
(known-issues.md ->
`D-POSIX-DL-ITERATE-PHDR-NEVER-CALLED-BACK-SO-NO-CXX-THROW-COULD-BE-CAUGHT`).
Lane D has fixed the C library's half. The host tests check it against a
synthetic program image; only a real program on the real system has a real
image, thread pointer and unwinder, so a fixture does the rest. I am asking
for the rung that runs it. It needs no kernel change.

## The fixture

`services/ctest-cxx-throw/` (`main.cpp`, `build.py`) -- the first C++
fixture: built by `scripts/ctest-fixtures.py` like every `ctest-*` (zig
c++, then rust-lld with `--eh-frame-hdr` against zig's libc++, libc++abi
and libunwind and our `libc.a`), and staged at `/tests/ctest-cxx-throw.elf`.
It exits 42 when every check passed; any other code names the first that
failed, as the comments in `main.cpp` list:

| Codes | Check |
|---|---|
| 1-5 | `dl_iterate_phdr` calls back once, for the program (name ""), with a `PT_LOAD` holding the calling code and a `PT_GNU_EH_FRAME` |
| 10-12 | `_dl_find_object` finds the same `.eh_frame_hdr` for an address in the program, and nothing for a stack address |
| 20-24 | `dlopen(NULL)` is the program; `dlsym` finds nothing and says so once; `dlclose` succeeds |
| 30-32 | an `int` thrown and caught |
| 40-43 | a `std::runtime_error` through fifty-one frames, every frame's destructor run on the way, caught by its base class |
| 50-51 | rethrown from a `catch (...)` and caught outside |
| 60-62 | kept as a `std::exception_ptr` and rethrown after its handler ended |
| 70-71 | a derived class caught as its base |
| 134 | `SIGABRT`: `std::terminate`, an exception nothing could catch -- the bug itself |

It cannot hang: nothing waits, reads or sleeps, and every throw is caught in
`main` or ends the process.

## What I am asking for

A self-test like `self_test_clongdouble` (`kernel/src/proc/spawn.rs`) for
`/tests/ctest-cxx-throw.elf`, expecting exit code 42, in the boot sequence
with the other `ctest-*` rungs. On a failure the exit code is the diagnostic.

## What happens until then

The fixture is built and staged but not run; `posix/src/dlfcn.rs`'s tests
cover `dl_iterate_phdr`, `_dl_find_object` and the rest against glibc 2.39's
answers.
