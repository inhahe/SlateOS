### TD-OILS-A-SUBSCRIPTED-READ-DRAWS-A-DIFFERENT-NUMBER-OF-TIMES. `${RANDOM[0]}` consumes one number where bash consumes two — 2026-08-06 — WON'T FIX (bash implementation artifact)

**Where:** `userspace/oils/src/interp.rs` — `Shell::scalar_for_subscript`, which
calls `Shell::dynamic_special_value` once per read.

**What.** Found while writing the corpus case for the entry above, which first
tried to assert that a subscripted read takes the same seeded sequence an
unsubscripted one does. bash fails that assertion. Seeded with `RANDOM=1` the
stream is `16807 10791 19566 13983 18126 …`, and:

```text
RANDOM=1; echo $RANDOM $RANDOM              16807 10791     1 draw each
RANDOM=1; echo ${RANDOM[0]}; echo $RANDOM   10791 / 19566   2 draws
RANDOM=1; echo ${#RANDOM[@]} $RANDOM        1 10791         1 draw
RANDOM=1; echo ${!RANDOM[@]} $RANDOM        0 10791         1 draw
RANDOM=1; echo ${#RANDOM[0]} $RANDOM        5 10791         1 draw
RANDOM=1; echo ${RANDOM[0]#1} $RANDOM       3983 …          4 draws
RANDOM=1; echo ${RANDOM[0]:0:2} $RANDOM     13 …            4 draws
RANDOM=1; n=RANDOM; echo ${!n[0]} $RANDOM   16807 10791     1 draw
```

The count is not a rule about subscripts — it is a count of how many times
bash's expander happens to call `find_variable` on the way to the value, and
every operator has its own. A plain element read finds twice (once in
`get_var_and_type` to classify the name, once to read the cell); the shape
questions find once; a trim or a slice finds twice again; an *indirect*
subscripted read finds once. Each find runs the value function, and `RANDOM` is
the only name where running it is observable.

**Deliberately not matched.** Reproducing this means reproducing bash's
`find_variable` call counts through the whole expander, operator by operator,
for no semantic gain — the same reasoning already applied to bash's stale-cache
listing under TD-OILS-DECLARE-P-BULK-DYNAMICS. osh draws exactly once per read,
which is the behaviour the *unsubscripted* spellings already agree on. The
corpus case therefore asserts only that each subscripted read draws afresh from
the seeded stream, which is checkable without printing a number.

**Impact.** A script that seeds `RANDOM` and then mixes subscripted and
unsubscripted reads gets a different sequence than bash would give. Scripts that
seed `RANDOM` for reproducibility already get a different sequence, since the
generator itself is not bash's.
