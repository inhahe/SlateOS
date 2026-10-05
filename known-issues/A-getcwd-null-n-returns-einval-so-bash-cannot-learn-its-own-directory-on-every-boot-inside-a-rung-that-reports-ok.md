### [A] `getcwd(NULL, n)` returns EINVAL, so bash cannot learn its own directory -- on every boot, inside a rung that reports OK -- 2026-09-18
**Status:** FIXED 2026-09-24 in `posix/**` (lane D, which owns `posix/` since the six-lane split): a NULL `buf` now allocates -- `size` bytes, or exactly the path's length plus one when `size == 0` -- with `ERANGE` checked before allocating, and `EINVAL` kept only for a non-NULL `buf` with `size == 0`. `get_current_dir_name` delegates to it; `__getcwd_chk` clamps to `buflen`. The tests that had pinned the bug (`test_getcwd_null_buf`, `test_getcwd_chk_null`) now pin the fix. **Still open, lane A's:** the bash rung asserting an empty stderr (option (b) in the request), which is what would have caught this on day one.

**In short:** the shell prints an error at startup saying it cannot work out
which directory it is in. It has done this on every boot for at least 20
boots, nobody had recorded it, and the test that runs the shell reports
success -- because the test checks what it asked the shell to do and not
what the shell said on its way there.

**The symptom**, in every one of 20 serial logs, and in none of
`known-issues.md` or `todo.txt` before this entry:

```
shell-init: error retrieving current directory: getcwd: cannot access parent directories: Invalid argument
```

It is emitted by process `spawn-test-bash` -- `self_test_bash_on_slateos_libc`
-- and the very next verdict line for that rung is:

```
[spawn]   GNU bash 5.2 on our own libc.a (ring 3: static ELF, no glibc and no
ld.so; arrays, parameter/arithmetic/brace expansion and its own `>` redirection
all ran against posix/src; read back 55 bytes == expected, exit 0): OK
```

The rung is not lying. It asserts arrays, expansions and redirection, and all
of those work. It never asserts anything about the shell's *stderr*, so a
real error there is outside the verdict's corpus -- dd-942, in the one place
a reader would look to find out whether bash works.

**Root cause, verified on both sides rather than inferred.**

| side | evidence |
|---|---|
| what bash asks for | `bash-5.2/builtins/common.c:636` calls `getcwd(0, PATH_MAX)` and `:638` falls back to `getcwd(0, 0)`. **Both pass a NULL buffer.** The message at `:642` prints `strerror(errno)` as its third field, which is where "Invalid argument" comes from |
| what we return | `posix/src/unistd.rs:392`: `if buf.is_null() || size == 0 { set_errno(EINVAL); return null_mut(); }` |

So the two conditions are collapsed into one branch, and only one of them
belongs there. `getcwd(buf, 0)` with a non-NULL `buf` **is** EINVAL and that
half is right. `getcwd(NULL, n)` is the **GNU allocate form**: glibc mallocs
a buffer and returns it, and every program that wants the cwd without
guessing a size uses it. The module's own doc comment states the wrong rule
as if it were the specification: *"`EINVAL` -- `buf` is null or `size` is 0"*.

**Why the existing tests all pass anyway, which is the interesting part.**
Three separate green checks surround this and none of them can see it:

| check | what it covers | why it misses |
|---|---|---|
| `[syscall/linux] getcwd(_, 0) -> ERANGE not EINVAL/EFAULT` | the **kernel syscall** | correct, and a different layer -- the bug is in the libc wrapper above it |
| `[spawn] Spawn with initial cwd (valid + invalid): OK` | `SpawnOptions.cwd` | tests the kernel setting a cwd, not a program reading one |
| `[spawn] REAL dash shell cwd ... pwd -P ... exit 0: OK` | a real ring-3 shell reading its cwd back | **dash passes a real buffer.** Only the NULL form is broken, so the shell that proves cwd works is the one that never takes the broken path |

That last row is the one worth keeping: we have a passing ring-3 test for
exactly this feature, and it passes because it exercises the other branch.
A green test for "reading the cwd works" coexisting with "the shell cannot
read the cwd" is not a contradiction, it is two call forms and one test.

**The fix, which is lane B's** (`posix/**`): allocate when `buf` is NULL --
`size` bytes if `size > 0`, otherwise a buffer sized to the path -- and keep
EINVAL only for non-NULL `buf` with `size == 0`. Filed as
`requests/a-b-getcwd-rejects-the-null-buffer-form-that-bash-uses.md`.

**Not fixed here on purpose.** `posix/src/unistd.rs` is lane B's tree. The
finding, the two-sided evidence and the corpus argument are the parts that
were mine to produce.
