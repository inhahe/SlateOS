### TD-OILS-A-BAD-SLICE-BOUND-DOES-NOT-STOP-THE-SECOND-ONE. `${v:'0':'2'}` complains twice where bash complains once — 2026-08-04 — ✅ FIXED 2026-08-04

**Where:** `userspace/oils/src/interp.rs` — the substring/slice expansion
evaluates the offset and the length as two independent arithmetic evaluations
and reports whatever each one says. bash evaluates the offset first and a
syntax error there abandons the whole expansion (its arithmetic evaluator
`longjmp`s out), so the length is never looked at.

**Reproduce:**

```sh
v=abcdef
echo "${v:'0':'2'}"
# bash: one error   -- v: '0': syntax error: operand expected (error token is "'0'")
# osh:  two errors  -- the same, then the identical complaint about '2'
echo "${v:1+:2+}"   # same shape: bash names `1+` only, osh names `1+` and `2+`
```

Both shells produce no output for the command and neither aborts the script, so
only the diagnostics differ — but they differ on stderr, which the corpus differ
compares, so any case that trips a bad slice bound would fail on this.

**A second bug in the same place, found while fixing this one.** The offset also
decides *whether the length is read at all* — an offset past the end of the
parameter answers with nothing and never evaluates the length, so bash's
`${v:99:j++}` leaves `j` at `0`. `Shell::scalar_slice` (the `${v[@]:…}` spelling)
already knew that; the `WordPart::ParamSubstr` arm (the plain `${v:…}` spelling)
did not, and ran the `j++`.

```sh
v=abcdef
j=0; echo "[${v:99:j++}] j=$j"   # bash: j=0   osh: j=1
j=0; echo "[${v:6:j++}]  j=$j"   # both j=1 -- a start exactly at the end is inside
```

**The fix.** Two halves, both in `userspace/oils/src/interp.rs`:

* `eval_arith_substr_bound` now returns `Option<i64>`, `None` being a bound that
  would not evaluate (it still makes the diagnostic and still arms the discard —
  only the fabricated `0` is withheld). Its five callers — in `slice_elements`,
  `assoc_slice` and `scalar_slice` — return where they stand. The underlying
  `eval_arith_index_text` grew a `_checked` sibling and became its
  `.unwrap_or(0)`.
* `WordPart::ParamSubstr` stopped reimplementing the operator and now calls
  `Shell::scalar_slice`, which is the same operator reached by the `[@]`
  spelling. All that differed was how "no field" is spelled — a string context
  has only the empty string to say it with, so the arm takes the first field or
  nothing.

**Found:** while probing `"${v:'0':'2'}"` for
TD-OILS-A-QUOTED-SUBSTITUTIONS-OPERAND-IS-LEXED-AS-IF-IT-WERE-BARE; it predates
that fix and is unrelated to it.

**Pinned by** `userspace/oils/tests/corpus/a-slice-bound-that-will-not-evaluate-stops-the-other.sh`,
which walks both halves over a scalar, an element, an array, the positionals and
an associative array.
