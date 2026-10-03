### TD-OILS-CORPUS-SECONDS-TICK-FLAKE. `tests/corpus/dynamic-var-assign.sh` printed a live `SECONDS` reading and failed when a second happened to tick — 2026-08-01 — ✅ RESOLVED 2026-08-01 (test bug, not a shell bug)

**Symptom.** A full `scripts/osh-bash-diff.py` run reported
`223 matched, 0 waived, 1 failed`, the failure being `dynamic-var-assign` with a
single differing line: `read=1006` from bash against `read=1005` from osh.
Re-running the case passed. Nothing else in the ~60-line output differed.

**Cause.** The case was

```sh
( SECONDS=100; SECONDS+=5; declare -p | g SECONDS; echo "read=$SECONDS" )
```

`SECONDS+=5` appends textually, so the cell holds the string `1005`; but
`declare -p` *reads* the variable, and reading a dynamic variable puts it back
in live-counter mode with 1005 as its base. `$SECONDS` in the `echo` therefore
answers 1005 plus however much wall clock had passed — normally still 1005,
but 1006 whenever the second boundary fell inside that window. A file comment
had already spotted exactly this hazard one case further down (line 72, which
answers with a range instead) and mis-attributed this line as safe.

The same latent flake sat in

```sh
( SECONDS=-3; a=$SECONDS; echo "$a"; sleep 1; b=$SECONDS … )
```

where `$a` would print `-2` on a tick. It had not fired yet.

**Fixed** by answering with a window in both places, the way the neighbouring
case already did: `read=$(( SECONDS >= 1005 && SECONDS < 1015 ))` and
`neg=$(( a < 0 ))`. Both still say what the case is *for* — that the count
restarts from the appended text rather than from 0, and that a negative base is
kept as a negative base — without pinning a number that wall clock can move.

**Rule of thumb for the corpus:** never print a bare `$SECONDS` (or anything
else derived from wall clock) after assigning it. Compare it into a boolean, or
mask it.
