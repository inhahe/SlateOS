## TD-A-SHARED-TMP-READ-BACK-ANOTHER-SESSION-S-BUILD-LOG (lane A, 2026-09-11) — **practice, not code; the tree is measured clean**

**In short:** three Claude sessions run on this machine at once, and `/tmp` is not
three directories — it is one. I wrote build logs there, read them back, and got
another process's output: a line from a build I never ran. Nothing in the repository
does this; it was entirely in the ad-hoc commands I type. The fix is to put logs
under the worktree's own `build/`, which is per-lane by construction.

### What happened

Verifying a new compile-time assertion in three stages, each `cargo check` log went
to `/tmp/c1.log`–`c3.log`. Reading them back mid-run returned a `TESTEXIT=0` line
belonging to nothing I had run, and `Finished \`dev\` profile` from a build invoked
with `--release`. `/tmp` in this MSYS environment resolves to
`C:/Users/inhah/AppData/Local/Temp` — the single user-wide Windows temp directory,
shared by all three lane sessions *and* by unrelated programs; `LOGOPLEX3` and
`ClaudeSetup` logs sit in it too.

**The verification survived on luck, in the two places that generalise.** The exit
codes had been echoed into the task's own harness-owned output file rather than read
back from the log, and the string being grepped for was a phrase from my own
assertion message, which nothing else on the machine emits. Had I grepped for
something ordinary — `error`, `warning`, `FAILED` — I would have read another lane's
build as my own evidence and concluded with identical confidence.

So the durable rule is narrower than "avoid `/tmp`": **an exit code the shell echoed
is trustworthy in a way a file read back later is not, because only one of the two can
be overwritten by a stranger between the write and the read.**

### Is the repository exposed? No — measured, in three passes

Worth recording the *negative* result, because arriving at it took three instruments
and the next person to notice this hazard will want to know.

| pass | instrument | answer | why it was wrong |
|---|---|---|---|
| 1 | `grep` `/tmp/` in `scripts/*.sh` | 3 scripts write generic names | correct, but only covered shell |
| 2 | same, intersected with the runners | **none** is invoked by a runner | `.sh`-only again |
| 3 | `scripts/*.py` as well | **six** `/tmp/` references *are* in runner-invoked gates | referencing ≠ writing |

Reading those six settles it: every one is a non-write. `check-gate-call-sites` has
`/tmp/x.json` inside self-test *argument lists* handed to a parser; `check-query-status`
and `gittree` have it in docstring usage examples; `check-diskcleanup-test-roots` has
it as an example of the path literals it detects; `check-selftest-wording` in a
comment. `check-selftest-skips` looks like the real thing —
`Vfs::write_file("/tmp/p", b"")` — but that is **kernel source inside a fixture**, so
the `/tmp` is the guest's tmpfs, not this host's.

The three shell scripts that genuinely write generic names — `extract-tcc-strace.sh`,
`p37-check.sh`, `p38-check.sh` (`/tmp/hosted.c`, `/tmp/p37.out`, `/tmp/strace.out`) —
are one-off diagnostics run deliberately, one at a time, and no runner invokes any of
them. Left alone: 38 files reference `/tmp`, `scripts/` is edited by all three lanes,
and a sweeping rename for a collision that needs two agents to run the same spike
simultaneously is churn with more conflict risk than the bug it prevents.

**And the idiom is already dominant where it matters:** 57 scripts use
`tempfile.mkdtemp`/`TemporaryDirectory`/`NamedTemporaryFile`, and `boot-test.sh` puts
its clippy log in `build/clippy-kernel.log`. The tree got this right. The commands we
type by hand are where nobody is reviewing.

### To re-check after the tree changes

```
grep -rlE "/tmp/" scripts/*.py scripts/*.sh          # candidates
# then, for each, intersect with the three runners and READ the line:
#   boot-test.sh, hooks/pre-push, pre-boot.py
```
The step that cannot be skipped is reading the line. Two of my three passes failed
at precisely that point, in opposite directions.
