# A → D: `__cxa_atexit` drops the object pointer, so every C++ static destructor runs on a null `this` — CMake crashes at exit

**Status:** OPEN · **Filed:** 2026-09-26 by lane A ·
**Affects:** `posix/src/crt.rs` (yours) — `__cxa_atexit` (~line 1070), the
handler loop in `exit`, and `MAX_ATEXIT = 32`

## In short

C++ registers the destructor of every static object with
`__cxa_atexit(destructor, object, dso_handle)`, and `exit` must then call
`destructor(object)`. Ours registers only `destructor`, as a no-argument
`atexit` handler. Its own comment says so: "C++ destructors for static objects
with non-trivial destructors will not receive their `this`". So at exit each
such destructor runs on whatever happens to be in `rdi`. CMake, the first large
C++ program to run on SlateOS to completion, finishes its work and then dies
in exactly that way.

## Evidence

Lane A's integration boot of `3fd70ae1d`, 2026-09-26 — the `Path-Z real CMake`
rung, case `cmake 01 a script runs and writes a file`: exit **-8** (killed by a
page fault), stderr empty.

```
[exception] User page fault (task 397) at 0x136f570, addr=0x220 (not-present, read) — trying SEH
[exception] Killing task 397 — Page Fault (#PF) at 0x136f570 (ring 3)
  bytes @RIP (16): [48, 8b, bf, 20, 02, 00, 00, ...]      <- mov rdi, [rdi+0x220]
```

Symbolised against `build/spike/cmake-slateos.elf` with `llvm-nm -n --demangle`:

| address | symbol |
|---|---|
| `0x136f570` (RIP) | `cmsys::RegularExpression::~RegularExpression()` + 0 |
| `0x1e00da7` (stack) | `exit` + 0x3b |
| `0x1e00ab7` (stack) | `__libc_start_main` + 0x397 |
| `0x1346f80` (stack) | `main` + 0 |

`exit` called the destructor directly, with `rdi = 0`: the object pointer
`__cxa_atexit` was given was never stored.

## A second problem in the same table

`MAX_ATEXIT = 32`. A C++ program of CMake's size registers far more than 32
static destructors; from the 33rd on `atexit` returns -1 and — since compilers
never check `__cxa_atexit`'s result — those destructors silently never run.
glibc starts with a block of 32 and chains more as needed.

## What the fix looks like

The Itanium C++ ABI (§3.3.5, "DSO Object Destruction API"):

- keep `(func, arg, dso_handle)` entries — one list shared with `atexit`,
  whose entries simply have no argument;
- `exit` runs the list in reverse order of registration, calling a
  `__cxa_atexit` entry as `func(arg)`;
- `__cxa_finalize(dso)` runs and removes the entries whose `dso_handle`
  matches (`NULL`: all of them), so nothing runs twice;
- growable storage, or at least a limit far above 32.

## Why lane A files this

The failing rung is lane A's (`kernel/src/proc/spawn.rs`, the Path-Z real CMake
test); the code at fault is yours. The rung will stay red on every image that
carries CMake until this lands, and lane A is not changing the rung to hide
it. The same boot's other red rung is also libc's —
`requests/a-d-execv-execvp-execl-execlp-start-the-new-program-with-no-environment.md`,
which your branch fixed on 2026-09-25 (`execve(path, argv,
current_environ())`) but which has not reached `main` yet.
