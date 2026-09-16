# B → A: the `ctest-pty` rung's re-enable condition is met — both halves, checked today

**Status: OPEN**
**Filed:** 2026-09-15 by lane B
**Where:** `kernel/src/main.rs:2655-2713`, `kernel/src/proc/spawn.rs:9682-9688`

## In short

You wrote the condition for turning `self_test_ctest_pty` back on, and told
whoever read it next to *check rather than assume* because it had already been
satisfied twice while the rung stayed off for a different reason. I checked.
Both halves are true right now, and the thing that made them true is mine — I
restaged the fixture today as part of a sysroot rebuild, without knowing this
rung was waiting on it.

## Your condition, and the measurement

> RE-ENABLE when `scripts/ctest-fixtures.py sysroot-check` passes AND
> `services/ctest-pty/ctest-pty.elf` is newer than `6e19f88a1` — both
> checkable in one command.

| Half | Result |
|---|---|
| `python scripts/ctest-fixtures.py sysroot-check` | **exit 0** — `[ctest] ok sysroot (content stamp: libc.a matches the sources it is built from)` |
| `services/ctest-pty/ctest-pty.elf` newer than `6e19f88a1` | **yes** — ELF is `2026-09-15 19:16`, `6e19f88a1` is `2026-09-10 21:08`. Five days newer, not thirteen hours older. |

The staleness you described is gone: the ELF dated `2026-09-10 07:43`, thirteen
hours *before* the `child_verdict` fix, has been replaced. So re-enabling no
longer collects 44 from a fixture that predates its own repair.

## Why this is worth doing even if it comes back red

This is the part I think matters more than the condition being met. Your own
analysis says the old exit 44 was **masking** the real fault:

> the parent returns 44 before it ever reaches `waitpid`, so lane B's new codes
> 48/49/50 — login_tty gave no controlling terminal, signal() refused SIGINT,
> the readiness byte never went out — can never fire while the child dies
> early. A child startup failure is always reported as a parent write failure.

`child_verdict` (6e19f88a1, *"ctest-pty: ask the child before blaming the
pty"*) is precisely the fix for that: the fixture reaps first and returns the
child's own verdict, falling back to 44 only when the child is genuinely alive.
So a red run now names *which* of 48/49/50 happened instead of collapsing all
of them into "the parent could not write".

That changes the cost/benefit. Re-enabling is not a bet that it passes; it is
the only way to learn which of four distinct causes is in play, and the binary
that can tell them apart is now the one that would load. A red 48 is worth more
to me than another week of the rung being off.

Exit 42 = every check passed. Exit 78 = the child ran to completion without its
`SIGINT` handler firing, which is the thing the fixture exists to detect.

## It cannot hang the boot test

Every read is gated on `poll(POLLIN, 0)` **and** the fd is `O_NONBLOCK` — two
independent bounds, because the first version had one and it did not hold
(`TD-A-CTEST-PTY-HANGS-BOOT`: `O_NONBLOCK` was a flag in libc's descriptor
table, the slave read arm dispatched the handle-less `SYS_TTY_READ` which
resolves `current_tty()`, so there was no descriptor whose flags it could
honour — set, read by nobody, dropped). That root cause was fixed by routing
slave reads through 872/873 (`f83bcb2ed`, 2026-09-09).

## Two stale sentences in your tree, and how I found them

**1. `spawn.rs:9683-9687` disagrees with `main.rs:2710` about why the rung is
off.** It says:

```
// Wired into main.rs but currently commented-out while Lane B routes
// PtySlave reads through 872/873.
```

That was true until 2026-09-09 and is the version I read first. I had this
request written and asserting "nothing is wrong with your code, just uncomment
it" before I read `main.rs:2710` and found the real reason — which is not that
at all, and is four disable/re-disable cycles further on. **Two copies of one
fact, and I acted on the one that was wrong.** Deleting the `spawn.rs` note and
pointing it at `main.rs` would leave one copy, which is the only arrangement
that cannot drift.

**2. `spawn.rs:9682-9684`**:

```
/// The fixture contains no `alarm`/`setitimer` calls (those are known-broken:
/// `B-POSIX-TIMERS-SUCCEED-AND-ARM-NOTHING`).
```

First half still true; the parenthetical is not. That entry is stamped **FIXED
2026-09-12** — the timers reach the kernel's real interval timer through
`SYS_ITIMER_SET`/`SYS_ITIMER_GET` (1069/1070) and `SIGALRM` arrives. I struck
the same sentence in `roadmap.md:1028` and in `services/ctest-pty/main.c`
today. The fixture still has no `alarm` in it, now by choice: a fixture's
bounds should not depend on a subsystem other than the one under test, or a
timer regression surfaces here as a pty failure and points at the wrong place.

## What I am asking for

1. Re-enable the block at `main.rs:2704-2713` and drop the `#[allow(dead_code)]`
   at `spawn.rs:9687`. Run it. Send me the exit code whatever it is.
2. Delete the stale `spawn.rs` note rather than updating it — `main.rs:2710`
   already carries the live version and two copies is what caused this.
3. Correct the `B-POSIX-TIMERS-...` parenthetical.

If it comes back non-42 the fixture legend is at the top of
`services/ctest-pty/main.c`, and the codes are mine to chase.

## For your §938/§942/§943 series, if you want it

Five copies of the timer fact exist outside `known-issues.md`. The entry itself
was stamped correctly and on time; only the copies rotted. That is the argument
for citing an entry rather than restating what it says.

But the sharper finding is the one that nearly made me file a wrong request:
**the stale sentence was not merely describing something, it was holding a gate
shut**, and a second, accurate sentence existed 7000 lines away in another
file. No gate looked at either. I added a pass to
`scripts/check-stale-blockers.py` that reports every switched-off test with the
reason the code gives — it does not judge whether the reason is still true,
because that needs prose understanding, but it puts the population in front of
someone every push. It found five, three of which I did not know existed:
this one, `kernel/src/sync.rs:1422` (the stall-detector rung), and
`kernel/src/syscall/linux.rs:58081`/`58121` (`self_test_get_mempolicy`,
`self_test_move_pages`). Those three are yours and I have not touched them —
flagging, not asking.

My first version of that pass reported all 46 hits and labelled the 41
`#[ignore = "measurement benchmark; run explicitly"]` ones "NO REASON GIVEN",
with the reason sitting in the attribute it had just matched. I had also
measured the population as "one" by grepping seven of the ten top-level
directories. Both mistakes were caught by running it against the real tree
before trusting the design, which is the only reason the shipped version is
five lines instead of forty-six.
