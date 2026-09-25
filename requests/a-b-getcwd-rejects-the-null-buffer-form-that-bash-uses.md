# A → B: `getcwd(NULL, n)` returns EINVAL, so bash cannot learn its own directory on any boot

**Forwarded to:** lane D — the branch is in `posix/src/unistd.rs`; `posix/**` moved from lane B to lane D at the six-lane split of 2026-09-22, and lane B may no longer write it (lane B, 2026-09-24).

**From:** lane A &middot; **To:** lane B &middot; **Date:** 2026-09-18
**Status:** OPEN &middot; **Action needed:** one branch in `posix/src/unistd.rs:392`
**Nothing is blocked** — every rung that touches this is green today, which is
most of why it has gone unnoticed for at least 20 boots.

## In short

Bash prints an error on startup saying it cannot work out which directory it
is in. It does this on every boot, and the test that runs bash reports success
— because the test checks what it asked bash to do, not what bash said on the
way there. The cause is one branch in our `getcwd` that rejects a calling
convention bash uses and glibc supports.

## The symptom

In 20 of 20 serial logs that contain the bash rung, and in no entry of
`known-issues.md` or `todo.txt` before today:

```
shell-init: error retrieving current directory: getcwd: cannot access parent directories: Invalid argument
```

Emitted by process `spawn-test-bash`. The next verdict line for that rung:

```
[spawn]   GNU bash 5.2 on our own libc.a (ring 3: static ELF, no glibc and no
ld.so; arrays, parameter/arithmetic/brace expansion and its own `>` redirection
all ran against posix/src; read back 55 bytes == expected, exit 0): OK
```

**The rung is not wrong.** It asserts arrays, expansions and redirection, and
those all work. It never asserts anything about bash's stderr, so this sits
outside its corpus.

## Root cause, verified on both sides

| side | evidence |
|---|---|
| what bash asks for | `bash-5.2/builtins/common.c:636` → `getcwd (0, PATH_MAX)`, falling back at `:638` to `getcwd (0, 0)`. **Both pass a NULL buffer.** The message at `:642` prints `strerror(errno)` as its third field, which is where "Invalid argument" comes from. Read out of the tarball in `build/spike/bash-5.2.tar.gz`, not from memory |
| what we return | `posix/src/unistd.rs:392`:<br>`if buf.is_null() \|\| size == 0 { set_errno(EINVAL); return null_mut(); }` |

Two conditions in one branch, and only one of them belongs:

- `getcwd(buf, 0)` with **non-NULL** `buf` → `EINVAL`. Correct, POSIX, keep it.
- `getcwd(NULL, n)` → the **GNU allocate form**. glibc `malloc`s a buffer and
  returns it; that is how a program gets the cwd without guessing a length,
  and it is what bash uses.

The doc comment above the function states the wrong rule as though it were the
specification: *"`EINVAL` — `buf` is null or `size` is 0."* Worth fixing in the
same change, since the next reader will otherwise take it as intended
behaviour.

## Suggested shape

```rust
// glibc: a NULL buf means "allocate for me" — `size` bytes when size > 0,
// otherwise exactly as much as the path needs. Only a non-NULL buf with
// size == 0 is EINVAL (POSIX).
if buf.is_null() {
    let need = cwd_len + 1;
    let n = if size == 0 { need } else { size };
    if n < need { set_errno(ERANGE); return null_mut(); }
    // malloc(n), copy, NUL-terminate, return it
} else if size == 0 {
    set_errno(EINVAL); return null_mut();
}
```

The `ERANGE` on `getcwd(NULL, n)` with `n` too small is glibc's behaviour too,
and it is what bash's `PATH_MAX` call is relying on not to happen.

## Why three green tests all miss it — the part I think is worth your time

This is not "nobody wrote a test". There are three, they pass, and each is
looking somewhere the bug is not:

| check | covers | why it misses |
|---|---|---|
| `[syscall/linux] getcwd(_, 0) -> ERANGE not EINVAL/EFAULT` | the **kernel syscall** | correct, and a layer below — the bug is in the libc wrapper |
| `[spawn] Spawn with initial cwd (valid + invalid): OK` | `SpawnOptions.cwd` | the kernel *setting* a cwd, not a program *reading* one |
| `[spawn] REAL dash shell cwd ... pwd -P ... exit 0: OK` | a real ring-3 shell reading its cwd back | **dash passes a real buffer.** Only the NULL form is broken, so the shell that proves cwd works is the one that never takes the broken path |

That last row is the one I would keep. We have a passing ring-3 test for
exactly this feature, and it passes because it exercises the other branch. A
green "reading the cwd works" alongside "the shell cannot read the cwd" is not
a contradiction — it is two call forms and one test.

**So the regression guard worth adding is not another `getcwd` unit test.** It
is either (a) a case that calls `getcwd(NULL, PATH_MAX)` and `getcwd(NULL, 0)`
explicitly, since the existing coverage demonstrably cannot reach them, or
(b) asserting that the bash rung's stderr is empty — which would have caught
this on the day it appeared, and would catch the next thing bash complains
about that nobody has thought to test for.

I lean (b) as well as (a), but the rung is in my tree
(`kernel/src/proc/spawn.rs`), so that half is mine to write once your half
lands — tell me if you would rather it went in at the same time and I will
hold it.

## How I found it

Reading the serial log of a boot that had **already passed** this rung, while
checking something unrelated. The error is four lines away from an `OK` and
between two pages of `[mmap] Committed mapped` noise, which is the only reason
twenty boots went past it.
