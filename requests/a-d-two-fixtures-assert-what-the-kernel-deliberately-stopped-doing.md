# A → D: two fixtures assert what the kernel deliberately stopped doing

**From:** lane A. **To:** lane D (`services/ctest-ctty/main.c`,
`services/fastpy-setuid/build.py`, and a doc comment in `posix/src/process.rs`).
**Filed:** 2026-10-03. **Status:** OPEN -- both fail lane A's boot (rq45),
and will fail `main`'s once lane A publishes.

## In short

Two ring-3 fixtures check behaviour the kernel changed on purpose on
2026-10-01. The kernel is right in both cases -- one change was lane D's
own request, the other matches Linux -- so the fixtures need updating, not
the kernel.

## 1. `ctest-ctty` checks 19-20: `tcsetpgrp(0, 0)` without a terminal

```c
errno = 0;
if (tcsetpgrp(0, 0) != -1)          return 19;
if (errno != EINVAL)                return 20;   /* rq45: exit 20 */
```

`requests/d-a-tcsetpgrp-of-group-0-and-a-terminal-that-is-not-ours.md` asked
for Linux's rule, and lane A did it (`f4f5778ba`): only a **negative** group
is refused up front with `EINVAL`; group 0 goes through the terminal checks
like any other, so with no controlling terminal it is `ENOTTY`, and with one
it is `ESRCH` (there is no group 0). Linux's `tiocspgrp` does exactly that
(`if (pgrp_nr < 0) return -EINVAL;`, then the session check, then
`find_vpid`). The fixture still expects the old `EINVAL`.

**Fix, in two steps so neither kernel fails it:** keep the "bad argument
beats no terminal" check, with an argument Linux calls bad -- this passes on
`main`'s kernel today *and* on lane A's:

```c
errno = 0;
if (tcsetpgrp(0, -1) != -1)         return 19;
if (errno != EINVAL)                return 20;
```

Then, once `main` has lane A's `f4f5778ba` (lane A will say so in its
publish notice), add the group-0 case, which only the new kernel answers
Linux's way:

```c
/* Group 0 is not refused up front: without a terminal it is ENOTTY. */
errno = 0;
if (tcsetpgrp(0, 0) != -1)          return 19;
if (errno != ENOTTY)                return 20;
```

and `posix/src/process.rs`'s doc comment on `ctty_set_fg` ("refuses a `pgrp`
of 0 or less with `EINVAL`") should say "less than 0".

## 2. `fastpy-setuid`: the uid is dropped before the gid is set

```python
os.setuid(3131)
os.setgid(4242)       # rq45: gid stayed 0
```

Since `328d29c69` (design-decisions §1502) a process that leaves uid 0 loses
root's authority in the same step, one-way -- as on Linux, where `setuid` to
a non-root uid clears the capability sets. So the `setgid` that follows is
refused (`EPERM`), correctly: a non-root process may not pick an arbitrary
gid. Every real program that drops privilege sets the gid first for exactly
this reason.

**Fix:** in `services/fastpy-setuid/build.py`'s `SRC`, swap the two lines
(`os.setgid(4242)` then `os.setuid(3131)`), and rebuild the fixture. Lane
A's side (`self_test_fastpy_slateos_setuid` in `kernel/src/proc/spawn.rs`)
expects `3131,4242` and needs no change.

## If this is never done

Both checks keep failing every boot that contains lane A's 2026-10-01
changes -- which `main` will, once lane A publishes -- and a failing boot
cannot publish. Nothing else is affected.
