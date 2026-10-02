### TD-OILS-TRANSFORM-SCALAR-IGNORES-SUBSCRIPT. `${s[5]@A}` recreates the scalar bash calls unset, and `${SECONDS@A}` recreates nothing — 2026-08-04 — ✅ **RESOLVED 2026-08-04**

**Where:** `userspace/oils/src/interp.rs` — the scalar tail of
`Shell::transform_assign` (~13218), which answers from `self.vars.get(name)`
rather than from the `elem` its caller now hands it.

**What.** `@A` on a *scalar* renders the variable's own value and ignores the
subscript that was written, so two things come out wrong:

```sh
s=hi
${s[5]@A}     # bash: (empty)                 osh: s='hi'
${s[0]@A}     # bash: s='hi'                  osh: s='hi'   ✅
${SECONDS@A}  # bash: declare -i SECONDS='0'  osh: (empty)
${RANDOM@A}   # bash: declare -i RANDOM='…'   osh: (empty)
${LINENO@A}   # bash: LINENO='2'              osh: (empty)
```

One root cause: `self.vars` is the wrong place to ask. A subscript other than 0
on a scalar names nothing, and a dynamic special has no `vars` entry at all —
both of which `param_elem_value` already answers correctly, and its answer is
exactly what `elem` now carries.

**Proper fix.** Have the scalar branch use `elem` for the value, keeping
`self.vars`/`self.declared` only for the "declared but never assigned" case that
renders the bare `declare -x xx`. Then check what attributes the dynamic
specials render: bash reports `declare -i` for `SECONDS`/`RANDOM` and bare
`LINENO='2'` for `LINENO`, so `attr_flag_letters` has to agree with
`DYNAMIC_SPECIALS`' `named_flags` for the value-carrying form and not merely for
`declare -p`. `${s[5]@a}` already matches (both empty), so only `@A` is wrong.

**Impact.** `${scalar[n]@A}` for `n` other than 0, and `@A` on any dynamic
special. Neither is common; the dynamic-special half is the more likely to be
met, since `${SECONDS@A}` is a plausible thing for a state-dumping script to do.

**Fixed in 77539b458.** The scalar branch answers from `elem`, as planned.

Two places the fix went past what the paragraph above anticipated:

* **`self.declared` had to go entirely, not merely be kept for the valueless
  case.** The guard read `self.declared.contains(name) && !attr_flag_letters(…)
  .is_empty()`, and `declared` holds only names declared *without* a value — so
  `declare -i iv=7; ${iv[5]@A}` came back empty where bash renders the bare
  `declare -i iv`. Both ways a value can go missing render alike, so which one
  happened is not a question worth asking; carrying an attribute is the whole of
  it. The value-carrying form now asks the same question the same way
  (`!attr_flag_letters(name).is_empty()`, replacing an eight-way `||` chain over
  the attribute sets) — the two spellings drifting apart is precisely what let
  `${SECONDS@A}` drop the `declare -i` its own valueless form would have
  printed.
* **`attr_flag_letters` consults `dynamic_special_listed`, not a third copy of
  the letters.** Its own doc comment already warned that the letter order had
  drifted once while written out twice; adding a table-aware variant alongside
  `dynamic_special_letters` would have been the same mistake again. `listed` is
  the right question rather than plain `dynamic_special`, because a name a real
  binding shadows is ordinary and the attribute sets then speak for it alone.
  Measured while checking this: an assignment does *not* shadow one — `RANDOM=7;
  ${RANDOM@a}` is still `i` in both shells, because the write is taken by the
  shell rather than making a binding. Only `unset` takes the name out of its
  hands.

Also closed a fidelity gap in the lib harness that the new test tripped over:
`new_shell` bound `$PWD` without exporting it, so `${PWD@a}` answered `[]` in
`run()` and `[x]` in the real binary. `seed_startup_dirs` does both halves; the
harness now does too.

Covered by the corpus case
`a-a-variable-transform-asks-the-element-not-the-cell.sh` (a full match against
bash 5.2.37, including a loop over fourteen dynamic specials that prints only
the *shapes* — the values are host state) and by the lib test
`a_variable_transform_renders_the_element_not_the_storage_cell`. Full sweep
321/321.
