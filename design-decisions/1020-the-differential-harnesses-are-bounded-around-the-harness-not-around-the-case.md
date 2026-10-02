## 1020. The differential harnesses are bounded around the harness, not around the case

**Lane:** B
**Date:** 2026-09-12
**Decided by:** Claude (autonomous)

**In short:** The 59 scripts that compare our command-line tools against GNU's
had no time limit. One tool that never finished would hang the run, and because
those scripts do their work inside WSL (a Linux machine Windows runs alongside
itself), nothing on the Windows side could stop it -- one such run sat there for
35 minutes and could not be killed. The fix adds a time limit. The decision is
*where* to put it: around each individual comparison, which is precise, or
around the whole script, which is not. The whole script won, because a limit
placed around each comparison has to sit between the harness and the tool it is
measuring, and anything sitting there changes what the tool sees.

**The tradeoff, stated plainly.** Per-case is strictly better on granularity: a
hung case would cost 60 seconds instead of the harness's whole allowance, and
the report would name the case. Per-harness is strictly better on transparency:
the tool is launched exactly as it was before, and nothing new is in the way.
They cannot both be had, and the reason is not a limitation of the tooling:

* The harness picks a side by putting one directory on `PATH` and reaching the
  binary through a symlink there. A symlink cannot carry a time limit.
* Replacing that symlink with a small wrapper script works, and covers every
  harness including ones not yet written, because it sits where the two sides
  are *constructed* rather than where they are called.
* But a wrapper is in the subject's **exec path**, and a shell reopens the
  standard descriptors before the script it interprets ever runs. So a subject
  reached through a `#!`-script wrapper can no longer tell that its caller
  closed stderr -- measured as `0 1 2 3` where a direct call gives `0 1 2`, for
  both `#!/bin/sh` and `#!/bin/bash`.

`nohup-diff.sh` has a case that closes stderr with `2>&-`, and it went from 75-0
to 74-1 the moment the wrapper existed. That is the whole argument: the wrapper
cost a real test case and returned nothing a per-harness bound does not also
give.

**For per-case, which was tried and rejected:** it names the hung case; it fails
fast; a slow harness does not have to have its allowance raised because one case
in it is pathological.

**Against:** it cannot be made transparent, per the measurement above. The
cheaper version -- a `timeout` written at each invocation site by hand -- avoids
the transparency problem entirely, because `timeout` *is* transparent, but it is
an enumeration with one entry per harness. It misses the next harness by
construction and the miss is silent. It was also wrong on the day it was
written: the list of 22 unbounded harnesses came from grepping for the
invocation shape, and the grep missed `sort`, `printf` and `seq`, which reach
the same binaries by a different spelling.

**For per-harness:** one place, `diff-wsl.sh` section 1b, covering every harness
that exists and every one that does not yet. Nothing is added to the subject's
exec path, so `argv[0]`, `PATH` and the descriptor table all reach it unchanged
-- asserted by three cases in `scripts/test-diff-bound.sh`, each stated relative
to a direct invocation so they do not encode the harness's own descriptor leak.
And `timeout` signals the process *group*, so everything the harness spawned
dies with it, which is the property the 35-minute orphan actually needed.

**Against:** a hung case burns the harness's whole 1800 seconds rather than its
own, and the report says which harness rather than which case. Accepted, because
a bound tight enough to be precise is tight enough to fire on a slow-but-finite
case, and a flaky verdict is how a check gets switched off. `DIFF_TIMEOUT` raises
it where a harness needs longer.

**Not settled by this entry:** the six harnesses whose subject is a *language*
-- `awk`, `tar`, `sh`, `ed`, `expr`, `calc` -- keep an inner per-case `timeout`
as well, since a non-terminating program is ordinary input there rather than an
exotic one. That is consistent rather than contradictory: `timeout` itself is
transparent in the measurement above. It is a *wrapper script* that is not.

**Where it bites if reversed:** put a bound back in the exec path and
`nohup-diff.sh`'s `2>&-` case is the canary -- it will drop from 75-0 without
any other harness noticing.
