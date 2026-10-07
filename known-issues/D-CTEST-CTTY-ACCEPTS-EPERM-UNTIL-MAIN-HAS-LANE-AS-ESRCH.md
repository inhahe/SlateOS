## D-CTEST-CTTY-ACCEPTS-EPERM-UNTIL-MAIN-HAS-LANE-AS-ESRCH — `ctest-ctty` accepts two answers where Linux gives one, until lane A's kernel change reaches main (lane D, 2026-10-07)

**Status:** OPEN -- waiting on lane A's f4f5778ba reaching `main`; then a two-line change here.

**In short:** `tcsetpgrp` -- "make this process group the one the terminal
belongs to" -- must fail for a group no process is in. Linux fails it with
`ESRCH` ("no such process"). Lane A's kernel does the same since its commit
f4f5778ba; the kernel on `main` still fails it with `EPERM` ("not
permitted"). `services/ctest-ctty` checks this twice (its codes 26 and 86)
and expected `EPERM`, which failed lane A's boot. Changing it to expect
only `ESRCH` would fail every other lane's boot instead, until lane A's
kernel is on `main`. So for now it accepts either. It is still a real
check -- the call must fail, and code 27 checks the terminal stayed where it
was -- but a kernel answering the wrong one of the two would pass.

**The fix, once `main` has f4f5778ba:** in `services/ctest-ctty/main.c`,
`errno != ESRCH && errno != EPERM` becomes `errno != ESRCH` at code 26, and
`retook_errno != ESRCH && retook_errno != EPERM` becomes
`retook_errno != ESRCH` at code 86; drop the "until main has" sentences
from the two comments, and move this file to `known-issues-resolved/`.

**Where it came from:** lane A's notice of 2026-10-07 and
`requests/a-d-ctest-ctty-expects-eperm-for-a-group-nobody-is-in.md` on
`lane-a`, which asked for `ESRCH` alone; this is that, made safe to land
before the kernel change does.
