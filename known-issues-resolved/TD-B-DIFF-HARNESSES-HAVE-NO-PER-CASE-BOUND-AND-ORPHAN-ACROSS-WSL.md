## TD-B-DIFF-HARNESSES-HAVE-NO-PER-CASE-BOUND-AND-ORPHAN-ACROSS-WSL (lane B, 2026-09-11) -- RESOLVED

**RESOLVED 2026-09-12, and the shape of the fix is the interesting part.**
Every harness is now bounded, including the ones nobody has written yet, and
the bound is in one place: `diff-wsl.sh` section 1b re-execs the harness under
`timeout` on the Linux side of the WSL boundary. `scripts/test-diff-bound.sh`
is the two-probe test, 9 cases.

**What it was.** 22 of the 59 `scripts/<name>-diff.sh` harnesses did not bound
an individual case. One subject that does not terminate stopped the whole run,
and the processes survived every kill available from the Windows side.

**CORRECTION (2026-09-11, same day).** This entry first said *none* of the
harnesses bounds a case. That was asserted without measuring and is false:
**31 of 59 already do**, through a pattern this tree established long ago --
`DIFF_NEED=timeout` in the header, and `timeout -k 2 30` inside the harness's
own `run_side`. `tsort-diff.sh` even explains why it bounds the *reference*
too: "a harness that only bounded our side would hang on the day the reference
was the buggy one."

The wrong premise was the expensive part, not the wrong sentence. It led to
the elaborate fix proposed below -- wrapping `diff-wsl.sh`'s `$bindir`
symlinks -- together with a real trap in it. All of that was designing a
solution to a problem already solved 31 times a few files away. **The actual
fix is to copy the existing pattern into the 28 that lack it**, which needs no
shared machinery touched and carries none of that risk.

**Two wrong shapes came first, and both are worth recording.**

*A `timeout` at every invocation site.* Six harnesses got one — `awk`, `tar`,
`sh`, `ed`, `expr`, `calc` — before it was clear this is an enumeration with
one entry per harness. It misses the next harness BY CONSTRUCTION and the miss
is silent. It was also already wrong when written: the list of 22 came from a
grep for the invocation shape, and that grep missed `sort-diff.sh`,
`printf-diff.sh` and `seq-diff.sh`, which reach the same binaries through
`$bindir` by a different spelling. **A list of instances is not a fix, and a
list built by pattern-matching the instances is not even a reliable list.**

*A wrapper script in `$bindir`.* This one covers every harness, written or
not, because it sits where the two sides are constructed rather than where
they are called. It looked right and it left `stat-diff.sh` at 77-11-19,
unchanged to the case. It was still wrong: a wrapper is IN THE SUBJECT'S EXEC
PATH, and anything there can be seen. `nohup-diff.sh` fell from 75-0 to 74-1,
on its one case that closes stderr with `2>&-`. Measured:

| caller closes fd 2, then… | what the subject finds | |
|---|---|---|
| direct, no wrapper | `0 1 2` | fd 2 still closed |
| through a `#!/bin/sh` wrapper | `0 1 2 3` | **reopened** |
| through a `#!/bin/bash` wrapper | `0 1 2 3` | **reopened** |
| through `timeout` | `0 1 2` | unchanged |
| through `bash -c 'exec -a …'` | `0 1 2` | unchanged |

A shell reopens the standard descriptors before the script it interprets ever
runs, so no shell-shebang wrapper can be transparent — and "stderr is closed"
is a case this family deliberately tests. The general form is one this tree
keeps rediscovering: **a harness may not put its own identity into the
subject's input**, and an exec-path wrapper is identity in the most literal
sense. It cost a real test case and returned nothing.

**Why wrapping the harness works instead.** `timeout` puts its child in a new
process group and signals the GROUP, so everything the harness spawned dies
with it — measured, including that the *caller* is not in that group and
survives with rc 124. The subject is launched exactly as before, by the same
symlink, with the same `argv[0]`, the same `PATH` and the same descriptors.
The cost is granularity: a hung case burns the harness's whole allowance
rather than its own. That is the right trade — a bound tight enough to be
precise is tight enough to fire on a slow-but-finite case, and a flaky verdict
gets a check switched off. The six per-case bounds stay as a fast fail, which
is safe precisely because `timeout` is transparent in the table above.

**A note on how the test failed.** `test-diff-bound.sh` reported the bound not
firing when what had actually happened was that its own generated fixture
sourced an unquoted path — and this repository lives under `visual studio
projects`, so the inner harness died at `/mnt/e/visual: No such file or
directory` before ever reaching the preamble. **A test whose fixture is broken
accuses the code it is testing**, and it accuses it of exactly the thing you
were expecting to find, which is what makes it convincing. What caught it was
printing the inner harness's own output on failure instead of only its exit
status.

**Progress, 2026-09-12.** The six whose subject is a *language* are done —
`awk`, `tar`, `sh`, `ed`, `expr` and `calc` — because that is where a
non-terminating program is ordinary input rather than an exotic one:
`while :; do done` is a one-line `sh` program and an `ed` script that never
reaches `q` never ends. Each was verified to change no verdict — sh 217/0,
ed 499/0, expr 177/0, calc 200/0, awk 171/0, tar 245/0.

**~~Still unbounded (22)~~ — superseded the same day by the harness-level
bound above, which covers all 59 at once.** Left here because the list itself
is the evidence for why the per-file approach was abandoned: it was built by
grepping for the invocation shape, and it is WRONG. `sort`, `printf` and `seq`
are missing from it and were unbounded too; `all` is on it and should not be,
since `all-diff.sh` is the aggregate runner rather than a subject harness and
must stay unbounded so its children's own bounds can fire. A hand-built list of
instances gets both kinds of error at once, and neither announces itself.

The list as filed read: `all`, `cat`, `csplit`, `cut`, `df`, `du`, `extfloat`,
`find`, `head`, `interleave`, `ls`, `more`, `nl`, `od`, `sed`, `sort`, `split`,
`test`, `tr`, `uniq`, `wc`, `xargs`.

**Lane A has confirmed the scope from their side**, which is what makes the
in-harness bound the only protection: `run-timeout.py`'s docstring promised
that nothing is ever orphaned, and they have narrowed it to say the guarantee
stops at the WSL boundary — `wsl.exe` hands work to a separate VM, so nothing
Linux-side is a Windows descendant and the Job Object cannot reach it. They
checked their own exposure rather than assuming: `boot-test.sh` never invokes
WSL (every `wsl` string in it is an echoed instruction to the operator), so
the exposed scripts are this differential family and `bashprobe.py`.

*What made me assert it: I grepped `diff-wsl.sh` for `timeout`, found none,
and concluded the family had no bound. The bound is in the harnesses, not the
library. Absence of evidence where I chose to look.*

**How it showed up.** `DIFF_PKG=awk bash scripts/awk-diff.sh` reached

    awk 'NR == 1 {getline; print "got", $0} {print "main", $0}'

and stopped. The standalone `awk` hangs on `getline` (that pair is now retired).
Two separate attempts left an `awk` process running inside WSL for 35 and 25
minutes; both were still alive long after the invoking shell was gone, and were
found with `ps -eo pid,ppid,etime,args` and killed by PID after confirming each
one's argv and parent.

**Killing the hung child is not enough, and this is the part that cost the most
time.** The harness *shell* survives too, and on losing its child it simply
advances to the next case — which for this subject is the next `getline`, which
also hangs. Twenty minutes after the first cleanup both `awk-diff.sh` shells
were still alive at 54 and 44 minutes, sitting on a new hang. A first sweep
missed them because it grepped for the exact argv of the *previous* hang
(`getline;`) and the new one reads `getline x`. **Kill the harness shell, not
the case** — and grep for the harness path, which does not change, rather than
for the case, which does.

**Why `run-timeout.py` does not cover it, which is the part worth knowing.**
That runner is the tree's answer to exactly this, and `CLAUDE.md` says to use it
for anything that might hang. It works by putting the child in a Windows **Job
Object** with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`. But `diff-wsl.sh` re-execs
the harness *inside WSL*, so the processes that actually hang are Linux-side and
are not in that job. Killing the Windows side tears down `wsl.exe` and leaves
the real work running. **A process-tree killer does not cross the WSL
boundary** — worth knowing for any tooling here that shells into WSL, not just
these harnesses.

**The fix, superseded — see the correction above.** Copy the existing
`DIFF_NEED=timeout` + `timeout -k 2 30` pattern into the 28 harnesses that lack
it. What follows was written before I measured, and is kept only because the
argv[0] trap in it is real and would bite anyone who tried the clever version:

**The superseded idea.** A bound on the far side of the boundary, where the processes
actually are: each case invoked under `timeout` inside WSL. The clean place is
`diff-wsl.sh`'s `$bindir` construction — four `ln -s` calls that build
`$bindir/{ours,gnu}/NAME` — since every harness reaches its subject through
those links, so wrapping there fixes all 50 at once with no harness edited.

**The trap in that fix, which is why it is not done yet.** Those are symlinks
named after the utility *on purpose*: `argv[0]` has to be the bare word, or
every diagnostic's `prog: ` prefix changes and every harness starts reporting
false differences in its error messages. `timeout N /path/to/real` makes
`argv[0]` the full path. A wrapper has to preserve the bare name — `timeout`
uses `execvp`, so `PATH=<dir> exec timeout N NAME "$@"` does preserve it, but
that also rewrites `PATH` for the subject, which matters for any utility that
spawns another (`xargs`, `awk`'s `system()`, `find -exec`). Getting this wrong
is a silent, tree-wide change to what 50 harnesses measure, so it wants doing
deliberately with the two-probe rule rather than in passing.
