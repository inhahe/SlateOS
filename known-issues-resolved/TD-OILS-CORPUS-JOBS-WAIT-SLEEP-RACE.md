### TD-OILS-CORPUS-JOBS-WAIT-SLEEP-RACE. `tests/corpus/jobs-wait.sh` probed a "still running" job that a loaded machine had time to finish — ✅ RESOLVED 2026-07-31 (test bug, not a shell bug)

**Where:** `userspace/oils/tests/corpus/jobs-wait.sh`, the block under
"…and it stays 127 even while a job *numbered* 1 is running".

**Symptom.** A full run made while a `cargo clippy` was running alongside
reported `175 matched, 1 failed` with `X jobs-wait`: `wait-spec=127`
against bash's `0`, plus an extra `line 157: wait: %1: no such job` on
stderr. Five re-runs of the case alone all matched.

**Why.** The block was `sleep 2 &` followed by five probes and then
`wait %1`. Under load the five probes took longer than the two seconds:
the `jobs 1` among them then *reports* the finished job, which purges it,
so the closing `wait %1` finds nothing. bash does exactly the same thing
on the same timing (verified by shortening the sleep so both shells cross
the boundary — both print "no such job"), so nothing was wrong with osh;
the case was simply sampling a race.

**Fixed** by removing the clock from the test rather than widening the
margin: the job now blocks on a file the script creates when it is done
probing —

```sh
( while [ ! -e release-job1 ]; do sleep 0.05; done ) &
…five probes…
: > release-job1
wait %1; echo "wait-spec=$?"
```

so the job is running for exactly as long as the probes need, however
slow the machine.
