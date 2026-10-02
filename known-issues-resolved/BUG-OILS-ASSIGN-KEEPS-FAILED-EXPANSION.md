### BUG-OILS-ASSIGN-KEEPS-FAILED-EXPANSION. An assignment whose value fails to expand still assigns, clobbering the old value — ✅ RESOLVED 2026-07-30

**Where:** `userspace/oils/src/interp.rs` — the assignment path, which expands the
value word and stores whatever came back. An arithmetic expansion that fails
yields `0`, so `0` gets stored.

**Symptom.** bash aborts the assignment on an expansion failure and leaves the
variable at its previous value; osh overwrites it. Both print the same diagnostic
and both score 1, so only the variable's contents diverge:

```sh
x=old
x=$(( a + ))
echo "rc=$? x=[$x]"    # bash: rc=1 x=[old]   osh: rc=1 x=[0]
y=old
y=$((1/0))
echo "rc=$? y=[$y]"    # bash: rc=1 y=[old]   osh: rc=1 y=[0]
```

(Must be run from a *script file*. With `-c 'x=old; x=$((a +)); …'` both shells
exit at the failure, so the difference is invisible — worth knowing when probing.)

Found while writing `tests/corpus/arith-vs-subshell.sh`; the shape
`x=$(( cd / && pwd ))` was dropped from that case because it fails here rather
than in the `$((` reading it was meant to test.

**Impact.** Any script that assigns an arithmetic expansion which can fail and
then reads the variable expecting its old value. Silent data corruption rather
than a visible error, since the diagnostic is printed either way.

**Fixed.** `apply_assignment` now snapshots `discard_error` on entry and abandons
the assignment — returning false without storing — if expanding the value armed
it. The snapshot matters: an abort armed *before* this assignment must not be
mistaken for one this value raised.

Where the check goes is the whole content of the fix, and measurement placed each
one:

- **Scalar** (`x=…`, `x+=…`, `x[i]=…`, `m[k]=…`): before the `set -x` trace, not
  after — bash does not trace an assignment it did not make.
- **Indexed literal** (`a=( … )`, `a+=( … )`): after phase 1, so phase 2's
  *clearing* is abandoned too. That is the observable part: bash leaves the old
  elements alone rather than emptying the array, and it binds nothing at all —
  `c=(o1); c+=( good x$((a+)) )` leaves `c` as `(o1)`, with `good` unbound.
- **Associative literal**: element by element, because that is what bash does —
  `p+=( [i]=good [j]=$((a+)) )` leaves `p[i]` set to `good`. The one place a
  partial result survives. A non-append literal builds its table off to the side
  and swaps at the end, so abandoning the swap leaves the old table intact.
- **`array_valued`** is now recorded after the expansion rather than on entry, so
  a failed literal leaves a previously-unvalued array unvalued (`declare -a ea`,
  not `declare -a ea=()`).

Covered by `interp.rs::a_failed_expansion_abandons_the_assignment` and the corpus
case `tests/corpus/assign-failed-expansion.sh`.
