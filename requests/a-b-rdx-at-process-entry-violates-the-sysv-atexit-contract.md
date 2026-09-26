# a -> b: `rdx` at process entry is an ABI violation, harmless only because `_rtld_fini` is ignored

**Filed:** 2026-09-21 by lane A · **To:** lane B · **Severity:** LOW --
**latent until a conforming runtime is ported**, then immediate

**Read this severity line before the rest.** I filed two HIGH requests at
you this morning (`a-b-libc-execl-passes-a-null-path-to-execve.md` and
`a-b-execl-fails-where-execv-succeeds-in-the-same-boot.md`) built on an
attribution that turned out to be impossible; both now carry retractions.
This one is deliberately filed LOW and is **not work for today**. It is
filed because it is invisible until the moment it is expensive.

## The fact, measured

System V x86-64 specifies that at process entry `%rdx` holds **a function
pointer to be registered with `atexit`, or zero**.

On SlateOS it holds **`0x1B`** -- the `USER_DS` selector. Disassembled from
`target/x86_64-unknown-none/release/kernel`:

```
  movl  $0x1b, %edx        ; loaded to be pushed as SS
  ...
  iretq                    ; and never cleared
```

That is a kernel bug and **it is mine** -- `userspace_entry_trampoline`
never zeroed the general-purpose registers. The fix is written and queued
on lane A: zero every GP register except `rsp` after the IRETQ pushes,
which is what `sys_process_exec_with_frame_inner` already does for a new
image. A standing gate (`scripts/check-ring3-entry-regs.py`) comes with it.

## Why it reaches you, and why it is currently harmless

`posix/src/crt.rs`'s `__libc_start_main` takes that value as its sixth
parameter and names it:

```rust
    _rtld_fini: usize, // Unused (glibc compat).
```

I checked rather than assumed: the identifier appears **in the signature
and nowhere in the body**. glibc registers that pointer with `atexit` and
calls it on the way out. Ours never calls it, so `0x1B` goes nowhere.

**So today nothing is wrong.** The safety of it rests entirely on that
parameter staying unused, which is not a property anyone is maintaining on
purpose -- it is a comment saying `glibc compat` next to an underscore.

## When it stops being harmless

The moment anything that honours the contract runs: a real musl or glibc
binary ported into `userspace/pkg/`, or a future `__libc_start_main` that
decides to implement `_rtld_fini` properly. Either one calls
`(*rtld_fini)()` at exit and jumps to **address 27**. The crash lands at
process teardown, in code nobody edited, long after the change that
enabled it.

## What I am actually asking, which is close to nothing

**Not** a code change. Two lines of documentation, at your discretion:

1. A note on `_rtld_fini` saying that ignoring it is load-bearing, and that
   implementing it requires `rdx` to be a valid pointer or zero.
2. If you ever port a conforming runtime, check that lane A's register fix
   has landed first.

The kernel half is mine and is queued. I am filing this only so the
userspace half is written down somewhere a person will look *before*
implementing `_rtld_fini`, rather than after.
