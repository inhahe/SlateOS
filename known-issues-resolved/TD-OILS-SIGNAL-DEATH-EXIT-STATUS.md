### TD-OILS-SIGNAL-DEATH-EXIT-STATUS. A child killed by a signal reported the raw Windows exit code, not `128 + sig` — ✅ **RESOLVED 2026-08-01**

**Where:** `userspace/oils/src/interp.rs` — every site that turns a reaped
child's `std::process::ExitStatus` into a shell status: `JobBody::wait_blocking`,
the concurrent-pipeline reaper in `exec_concurrent_pipeline`, and the
simple-command wait in `exec_simple_inner`. All three did
`status.code().unwrap_or(1)` and handed the number straight to `$?`.

On Windows there is no wait status to decode — `code()` is the raw value the
process passed to `ExitProcess`. That is fine for a process that exited
normally, but the externals the corpus runs are Cygwin/MSYS binaries, and
Cygwin's `signal_exit()` encodes a signal death as `ExitProcess(sig << 8)`. So
osh publishes 3328 where bash publishes 141.

**Reproduce:**

```sh
seq 100000 | head -n 1 > /dev/null; echo "ps=[${PIPESTATUS[*]}]"
```

bash: `ps=[141 0]` (SIGPIPE, `128 + 13`). osh: `ps=[3328 0]` (`13 << 8`). The
same holds for every signal — SIGTERM 143 vs 3840, SIGKILL 137 vs 2304, SIGINT
130 vs 512 — while a plain `exit 7` reads 7 in both. This is not a regression
from the DEBUG-trap pipeline work: it reproduces on a pipeline with no trap set
at all.

Note that the shell's *own* exit-status truncation is already right: `exit 300`,
`( exit 300 )`, `return 300` and `sh -c 'exit 300'` all agree with bash. Only
the code lifted off a reaped child escapes untruncated.

**Fixed** by one shared `child_exit_status` in place of the three open-coded
`code()` calls. On unix it reads `ExitStatusExt::signal` and needs no guesswork;
on Windows it recognises the Cygwin encoding — an exit code of `sig << 8` for
`sig` in `1..=64` becomes `128 + sig` — and masks everything else to eight bits,
a wait status having room for no more. The eventual SlateOS build takes the unix
arm, so the guess stays confined to the host platform that forces it.

The `sig << 8` test is a heuristic, and it is worth being explicit about which
way it errs: a native Windows program is free to exit with 3328 and mean it, and
would be misreported as SIGPIPE. Against that, bash makes the same bet by
trusting Cygwin's wait status, an exit code that is an exact multiple of 256 is
unreachable through any shell (they all truncate `exit` to eight bits), and the
alternative was a four-digit `$?` that no `[ $? -gt 128 ]` test could read.

**Impact (before the fix).** `$?` and `${PIPESTATUS[@]}` were wrong for any
externally-killed child, which includes the very common SIGPIPE from a
short-circuiting reader (`… | head -n 1`). Found while probing the all-external
concurrent pipeline path (`exec_concurrent_pipeline`) for the DEBUG-trap stage
work.

**Still divergent:** osh prints no notice for a foreground child killed by a
signal, where bash writes `Terminated` / `<pid> Killed …` to stderr. The corpus
case drops stderr rather than assert on it; the notice is job-control output and
belongs with that work.

Covered by
`tests/corpus/a-child-killed-by-a-signal-answers-128-plus-the-signal.sh`.
