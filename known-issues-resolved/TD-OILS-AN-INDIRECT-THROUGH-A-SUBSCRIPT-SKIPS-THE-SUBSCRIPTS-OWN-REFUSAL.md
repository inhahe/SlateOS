### TD-OILS-AN-INDIRECT-THROUGH-A-SUBSCRIPT-SKIPS-THE-SUBSCRIPTS-OWN-REFUSAL. `${!nosuch[-9]}` reported only `invalid indirect expansion` where bash refuses the subscript first — 2026-08-11 — FIXED 2026-08-11

**Where:** `userspace/oils/src/interp.rs`,
[`Shell::indirect_pointer_value`] and [`Shell::indirect_whole_array`] — the
two halves of the `${!name[sub]}` path, which between them decided that the
pointer named nothing *before* reading through it.

```sh
$ echo "[${!nosuch[-9]}]"
bash: nosuch: bad array subscript
      nosuch[-9]: invalid indirect expansion
osh : nosuch[-9]: invalid indirect expansion

$ ( declare -n a=b; declare -n b=a; echo "[${!a[-1]}]" )
bash: warning×2 / a: bad array subscript / warning / a[-1]: invalid indirect expansion
osh : warning×3 / a[-1]: invalid indirect expansion
```

**Why bash does it.** `parameter_brace_expand_indir` hands the reference to
the same `parameter_brace_expand_word` → `array_value` every other read goes
through, and only afterwards asks whether what came back names anything:

```c
  if (var_is_special == 0 && (v = find_variable_last_nameref (name, 0)))
    { … }
  if (legal_identifier (name) && v == 0)
    report_error (_("%s: invalid indirect expansion"), name);
```

So the read is paid for in full first. Its side effects happen, its faults are
reported in its place, and its subscript is judged against whatever the walk
found — which for a pointer that names nowhere is nothing at all, so every
negative subscript underflows (arrayfunc.c:1487; see
TD-OILS-A-READ-THROUGH-A-CHAIN-THAT-REACHED-NOTHING-STILL-JUDGES-ITS-SUBSCRIPT).
A pointer that names nowhere therefore reports **twice** — once for the
subscript, quoting the word cut off at its `[`, and once for the pointer,
quoting it whole — and a name that *is* there reports only the first, the
second being the one that depends on the variable existing.

The walk is spread the same way: a circular chain pays two warnings for the
read, then one more for the `find_variable_last_nameref` that follows it, with
the subscript's refusal in between.

**Fixed** in this commit. [`Shell::indirect_whole_array`] no longer gates its
read behind [`Shell::ref_name_exists`] — the classification passes go straight
to [`Shell::pointer_lookup`], whose memo (`PointerPart::resolved`) keeps the
read to the one bash makes however many times the recogniser is asked. And
[`Shell::indirect_pointer_value`] now reads first: the `BadSubscript` that
comes back is reported, and only then does `ref_name_exists` decide whether
there was a pointer at all. The old duplicate `BadSubscript` arm at the tail
of the match is gone with it.

Corpus: `an-indirect-through-a-subscript-skips-the-subscripts-own-refusal.sh`.
Unit test:
`an_indirect_through_a_subscript_pays_for_its_read_before_it_judges_the_pointer`.

**How it was found:** probing the read side of a nameref walk that reached
nothing, for
TD-OILS-A-READ-THROUGH-A-CHAIN-THAT-REACHED-NOTHING-STILL-JUDGES-ITS-SUBSCRIPT.
