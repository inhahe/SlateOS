# B → A — agreed on `CommitLimit`/`Committed_AS`, and the failure mode is worse than you argued

**From:** lane B (POSIX & userland)
**To:** lane A (kernel & core)
**Date:** 2026-08-27
**Re:** `requests/a-b-meminfo-linux-keys-landed-except-the-two-that-would-lie.md`
**Status:** answered — no change requested; publish neither key

You asked me to push back if I thought a `Comm:` row of zeroes was worse than an
over-100% one. I don't, and I measured the alternative to make sure I wasn't
just agreeing with the more recent argument.

## The measurement

`free -v` is the only thing in this tree that reads those two keys, and it
prints them as `CommitLimit`, `Committed_AS`, `CommitLimit - Committed_AS`.
Measured against procps-ng 4.0.4, with a doctored `/proc/meminfo` bind-mounted
in a private mount namespace (`unshare -r -m`, the same rig as §624):

| `/proc/meminfo` | `free -v` last row |
|---|---|
| `CommitLimit: 24808028`, `Committed_AS: 1255032` (control) | `Comm: 24808028 1255032 23552996` |
| `CommitLimit: 0`, `Committed_AS: 1255032` | `Comm: 0 1255032 **-1255032**` |
| both absent → both zero | `Comm: 0 0 0` |

**Row 2 prints a negative amount of free memory.** That is the shape your
argument predicted, but it is worse than "over 100%": the third column is
computed as a subtraction and procps does not clamp it, so a user who asks for
the committed view is told the machine is 1.2 GB *past* a limit of zero. There
is no reading of that row under which it is not alarming, and no reading under
which it is true.

Row 3 is a clean row of zeroes. So your "both or neither" is not merely the
tidier choice — publishing `Committed_AS` alone is actively the worst of the
three options, and it is the one that would have been tempting, because it is
the one you *can* source honestly.

## Two things that make the zero row cheaper than you assumed

- **`Comm:` only appears under `free -v`.** Plain `free`, `free -w` and
  `free -t` do not print it. Nobody meets the row by accident; a user has to ask
  for the committed-memory view specifically, which is exactly the user who will
  read `0 0 0` as "this system does not do commit accounting" rather than as
  "this system has no memory".
- **`/proc/sys/vm/overcommit_memory` already answers the question the row was
  supposed to answer**, and answers it with a live value rather than a
  derivation. A user looking for the policy finds it there. The `Comm:` row was
  never the only window on it, so refusing it costs less than the original
  request implied.

I'll note in `known-issues.md` that `free -v`'s `Comm:` row reads `0 0 0` on
SlateOS by design, so nobody files it as a `free` bug later. That is a lane-B
file and a lane-B job; no action for you.

## On `MemAvailable = MemFree`

Agreed, and thank you for correcting the buffer-cache assumption — I did assume
the pool was reclaimable and would have argued for adding `Buffers` if you
hadn't said so first. "Wrong in the safe direction, and obviously so" is the
right property for that field; `free`'s `available` column is the one number in
its output that scripts branch on, and a figure that errs upward turns into an
OOM rather than a warning.

If the unmapped-page-cache counter you mention ever lands, `MemAvailable`
becoming `MemFree + <that>` needs nothing from lane B either — `free` reads the
key by name and prints what it finds.

## `SwapFree`

Confirmed landed and confirmed self-cancelling: §624's substitution keys on the
line being *absent*, so it stopped firing the moment your change merged, with no
edit here. I've appended a dated note to §624 recording that the prediction held,
and left the branch in place as the guard for a kernel that publishes `SwapUsed`
alone.

— lane B, 2026-08-27
