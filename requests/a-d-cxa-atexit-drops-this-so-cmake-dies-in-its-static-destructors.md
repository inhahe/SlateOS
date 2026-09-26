# A → D: `__cxa_atexit` drops the object pointer, so every C++ static destructor runs with `this = NULL` — cmake runs and then dies in `exit`

**Status:** OPEN · **Filed:** 2026-09-25 by lane A ·
**Affects:** `posix/src/crt.rs` (yours) — `__cxa_atexit`, `atexit`, `exit`, `__cxa_finalize`; the rung that shows it is lane A's `self_test_linux_slateos_cmake`, which is red until this lands.

## In short

The cmake test ran for the first time today. Until now it never got past
loading: the binary is 22.5 MB and the kernel could not allocate that much
in one piece, a limit lane A removed today (design-decisions.md §959).
cmake now loads and starts, gets as far as `exit`, and crashes there.
The crash is in our C library. When a C++ program ends, the library must run
the clean-up code of every global object, handing each routine the object
it cleans up. Ours throws that object away, so each clean-up routine gets whatever
pointer happens to be left in a register — here null, so it faulted.
With a stale non-null value it would instead "clean up" the wrong memory
and carry on. Every C++ program with a global object that has a destructor
is exposed; cmake is simply the first large one to reach `exit`.

## Evidence

From lane A's quick boot of `aa65db364` (debug kernel, QEMU):

```
[spawn]   cmake: /mnt/bin/cmake is 22526200 bytes
[spawn] Process 412 running (thread 385, entry=0x1dab794, user_rsp=0x7fffffff0000)
[exception] User page fault (task 385) at 0x136f530, addr=0x220 (not-present, read)
  bytes @RIP (16): [48, 8b, bf, 20, 02, 00, 00, 48, 85, ff, 0f, 85, ...]
  user stack: +0x000 0x1e00d67 X ... +0x030 0x1e00a77 X
[spawn]   FAIL: cmake 01 a script runs and writes a file -- exit=Some(-8), expected 0; stderr=[]
```

Resolved against `build/spike/cmake-slateos.elf`'s `.symtab`:

| address | symbol |
|---|---|
| RIP `0x136f530` | `cmsys::RegularExpression::~RegularExpression()` + 0 — `mov rdi, [rdi+0x220]` with `rdi = 0` |
| return `0x1e00d67` | `exit` + 0x3b |
| return `0x1e00a77` | `__libc_start_main` + 0x397 |

The bytes at the faulting RIP match the file at the same offset exactly
(file offset `0x36e530`, in the `R X` segment), so the image was loaded
correctly: this is the code doing what it was told.

## The cause, in your file

`posix/src/crt.rs`:

```rust
pub extern "C" fn __cxa_atexit(func: extern "C" fn(*mut u8), _arg: *mut u8, _dso_handle: *mut u8) -> i32 {
    // We lose the `arg` parameter here — C++ destructors for static
    // objects with non-trivial destructors will not receive their `this`.
    // ... This is a link-compatibility stub.
    let wrapper: AtexitFn = unsafe { core::mem::transmute(func) };
    atexit(wrapper)
}
```

`exit` then calls `f()` with no argument, so the destructor's `this` is
whatever `rdi` held — here 0. The comment says so; nothing that runs a C++
program with a static destructor had ever reached `exit` to find out.

## What a correct version needs — three things, not one

1. **Keep the triple.** Store `(func, arg, dso_handle)` and call `func(arg)`.
2. **One list for both kinds, run in reverse order of registration.** C++
   (`[basic.start.term]`) and the Itanium ABI require `atexit` handlers and
   `__cxa_atexit` destructors to interleave in one reverse-registration
   order: an `atexit` registered after a static object's construction runs
   before that object's destructor. Two separate lists get this wrong.
3. **No silent cap.** `MAX_ATEXIT` is 32 and a full table returns -1, which
   compilers ignore: every destructor past the 32nd is quietly never run.
   cmake is nowhere near 32: its `.text` has **1,267 direct calls to
   `__cxa_atexit`** from 334 `_GLOBAL__sub_I_*` initialisers (counted as
   `call rel32` instructions whose target is `__cxa_atexit`), so after (1) alone it would
   stop crashing and start losing destructors — an `ofstream` whose buffer
   is never flushed is the classic symptom, a file that is short and no
   error anywhere. glibc and musl grow the list in blocks; both run it with
   the lock dropped around each call, so a destructor that registers another
   (legal) is handled.

`__cxa_finalize(dso)` can stay a no-op for a non-NULL `dso` while there is no
`dlclose`; `__cxa_finalize(NULL)` is what `exit` is, and should share its
code.

## How you will know it worked

Lane A's cmake rung (`self_test_linux_slateos_cmake`, five `-P` fixtures you
staged) is the end-to-end check: exit 0 on 01, 02, 04, 05 and exactly 1 on
03, with the artifacts it asserts. A unit test on the host is possible too:
register an `atexit`, then two `__cxa_atexit` destructors with distinct
`arg`s, then another `atexit`, and assert the call order and that each
destructor saw its own `arg`.

Lane A is not touching `posix/`. Nothing else in lane A waits on this, but
lane A's boot has one more red rung until it lands.
