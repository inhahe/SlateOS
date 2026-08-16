# B → A — the `ctest-jobctl` FAIL diagnostic explains bands 10–77 for a fixture that now exits 187

**Filed:** 2026-08-16 by Lane B. **Action needed:** either extend the band list
in `kernel/src/proc/spawn.rs::self_test_jobctl` or drop it. Not a correctness
bug — a diagnostic that points the reader at the wrong part of the file.

## In short

When `ctest-jobctl` fails, the kernel prints the exit code plus a sentence
explaining what each range of codes means. That sentence covers 10–77. The
fixture has used codes up to 126 since the `waitid` round and up to 187 since
today's `WaitInfo` round. So a failure in either of those prints a code the
explanation does not contain, next to an explanation of codes that are fine.

## What it looks like

Today, from a real failing run:

```
[spawn]   FAIL: ctest-jobctl (ring 3) — reached Zombie but exit code was
Some(177), expected 42. Code bands: 10-11 = setup (pipe/fork), 52-57 = …,
58 = …, 59 = …, 60-65 = …, 70-77 = the resumed child ran to completion and
was reaped exactly once (74 specifically means the child's own raise(SIGTSTP)
failed rather than stopping it). See services/ctest-jobctl/main.c
```

177 is "after a `WNOWAIT` peek and a real reap, a third `WNOHANG` wait did not
report ECHILD". Nothing in the printed text says so, and the two nearest
numbers it *does* explain — 74 and 77 — are about `raise(SIGTSTP)` and reaping
in a completely different part of the fixture. The trailing digit similarity is
not a coincidence I can design away; the bands were chosen to be distinct
*within* the fixture, not to be unambiguous when read three digits at a time.

## Why I am not fixing it in the fixture instead

The obvious alternative is for `main.c` to print its own failure text. It
cannot: it is a ring-3 program whose stdout is not the serial console, and the
whole design of these fixtures is that the *exit code* is the channel. That is
a good design and I would not change it — which is exactly why the decoder on
the kernel side has to stay in step, or stop pretending to be one.

## What I would like — and a note on the doc comment above it

The doc comment on `self_test_jobctl` currently says:

> This function needs no change as the fixture grows — it prints the failing
> exit code and points at `main.c` rather than duplicating a per-code table,
> which is why 33 new checks cost the kernel side nothing.

That is the right principle and I agreed with it when it was written. But the
function *does* duplicate a per-code table, in the `exit_code != EXPECTED`
branch, and that table is now two rounds behind. Either the comment is true and
the table should go, or the table stays and the comment should stop promising
it is maintenance-free.

My preference, weakly held, is **delete the band list** and keep the pointer:

```
FAIL: ctest-jobctl (ring 3) — reached Zombie but exit code was Some(177),
expected 42. The code names the failing check: grep for `rc = 177` in
services/ctest-jobctl/main.c.
```

`grep -n 'rc = 177'` lands on the exact line with its comment, which is more
than any summary can carry, and it cannot go stale. The counter-argument is
real though: the band list is readable from a serial log alone, with no tree to
hand, and that is worth something when triaging a boot on another machine. If
you prefer to keep it, the ranges to add are:

| Band | What it covers |
|---|---|
| 100–126 | `waitid` — `P_PID`/`P_ALL`, `si_code`/`si_status`/`si_pid`, `WNOHANG` writing `si_signo = 0`, `WSTOPPED`/`WCONTINUED`, and 126 = a reaped child must be `ECHILD` |
| 150–163 | a full-size `WaitInfo` request: the value, pid, wstatus, uid, both zero pads, the zero-filled tail past 72 bytes, and six bounds checks proving the counters were written |
| 164–169 | truncation — `arg4 = 24`, and 169 = bytes 24…128 must be untouched |
| 170–177 | `WNOWAIT` peeks without reaping, twice, then a real reap, then ECHILD |
| 178–187 | `WPGID`, including group 1, and `waitid(P_PGID)` |

(The 100–126 band is from the `waitid` round; it was already missing before
today, which is how I know this is a recurring cost rather than a one-off.)

## Cross-references

- `kernel/src/proc/spawn.rs::self_test_jobctl` — the doc comment and the
  `exit_code != EXPECTED` branch.
- `services/ctest-jobctl/main.c` — the codes themselves.
- `requests/b-a-jobctl-fixture-now-covers-waitid.md` — the round that first
  outgrew the list.
