### TD-OILS-CORPUS-BG-DEBUG-TRACE-RACE. `debug-trap-announces-a-background-command.sh` interleaves a background job's traces with the parent's about 1 run in 6 — 2026-08-01 — ✅ **RESOLVED 2026-08-01** (the *case* raced, not osh)

**Where:** `tests/corpus/debug-trap-announces-a-background-command.sh`. The one
section that raced was

```sh
set -T; f() { echo in >o.txt; }; trap b DEBUG; f & wait; cat o.txt
```

Under `set -T` the job's subshell inherits the DEBUG trap, so *both* sides
announce on the *same* stdout: the parent announces `f` and then `wait`, the
job announces `f` again at function entry and then `echo in > o.txt`. Which of
the job's first trace and the parent's `wait` trace reaches stdout first is
decided by nothing in either shell — it is a plain fork race.

**What was actually measured.** The entry originally read as an osh bug on the
strength of one full-sweep failure whose recorded bash order was

```
B:<f>  B:<f>  B:<echo in > o.txt>  B:<wait>
```

against osh's

```
B:<f>  B:<wait>  B:<f>  B:<echo in > o.txt>
```

Re-measuring settled it the other way round. In isolation — 8 runs each with
stdout a pipe, 10 each with stdout a file, 10 more through the harness itself,
and 10 runs of the whole case file — **bash and osh agreed every single time**,
and what they agreed on was the order the sweep had recorded as osh's *wrong*
answer. So it is bash that answers both ways, and only when the machine is
loaded by a full 250-case sweep; osh was stable throughout. The hypothesis this
entry first recorded — that bash's ordering is stable by construction because
the parent block-buffers while the child flushes at exit — is **wrong**, and
there was no ordering discipline for osh to adopt.

**Fixed** by making the case deterministic rather than by changing osh: the job
now gets its own stdout (`f >j.txt &`) and the parent replays it after `wait`
(`cat o.txt j.txt`), so the two sides' announcements cannot interleave at all.
That is also the plainer record of what the section is about — which side
announces what — and it is what the two shapes below it already did implicitly,
their job's stdout being the pipe into `cat > o.txt`.

**Not caused by** the `<>` work of the same day, as suspected at the time: the
case contains no `<>` redirect, and that change only alters behaviour where
`RedirPlan::stdout_write` / `stderr_write` is set, which only
`RedirectOp::ReadWrite` on fd 1 / fd 2 does.

**Standing lesson for the corpus:** a case that puts a background job's output
and its parent's output on one stream is measuring the host's scheduler, not
the shell. Give the job a stream of its own and replay it after `wait`.
