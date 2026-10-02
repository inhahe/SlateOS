### TD-OILS-CORPUS-SECONDS-BOUNDARY-FLAKE. `tests/corpus/dynamic-var-assign.sh` reads `SECONDS` across a second boundary — ✅ RESOLVED 2026-07-31 (test bug, not a shell bug)

**Where:** `userspace/oils/tests/corpus/dynamic-var-assign.sh` line 103,
`( SECONDS=-3; echo "$SECONDS"; sleep 1; echo "$SECONDS" )`.

**Symptom.** A full run reported `174 matched, 1 failed` with the only
difference being `-1` (bash) against `-2` (osh) on that second `echo`.
Three re-runs of the case alone all matched.

**Why.** `SECONDS` is whole seconds of wall clock since the base, so what
`sleep 1` yields depends on where in the current second the assignment
landed: a `-3` base set at .99 s reads `-2` after a 1 s sleep in one
shell and `-1` in the other purely from the two shells starting a few
milliseconds apart. Both are correct answers to "how many whole seconds
have ticked"; only the boundary is being sampled.

**Proper fix.** Assert the *difference* rather than the absolute reading —
`( SECONDS=-3; a=$SECONDS; sleep 1; b=$SECONDS; [ "$a" != "$b" ] && echo
climbing )`, as the line above it already does for the positive base — so
the case tests that a negative base counts up without pinning which
second it lands on. Leave the first `echo "$SECONDS"` (immediately after
the assignment, no boundary to cross) as it is.

**Fixed** exactly that way: the second reading is now compared with
`[ "$b" -gt "$a" ] && echo climbing || echo "stuck [$b]"`.
