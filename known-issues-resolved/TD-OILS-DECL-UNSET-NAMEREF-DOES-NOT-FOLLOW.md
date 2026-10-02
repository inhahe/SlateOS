### TD-OILS-DECL-UNSET-NAMEREF-DOES-NOT-FOLLOW. `declare +n` applies its *other* flags to the reference instead of the target — 2026-07-31 — ✅ RESOLVED 2026-07-31

**Where:** `userspace/oils/src/interp.rs` — `builtin_declare_scoped`,
where `unset_nameref` is one of the conditions that suppress the `follow`
step, so the whole operand stops being resolved.

bash is narrower than that: `+n` takes the attribute off the **name**,
and everything *else* in the operand still goes to the target.

```sh
w=5; declare -n r=w
declare -x +n r    # bash declare -x w="5" / declare -- r="w"
                   # osh  declare -- w="5" / declare -x r="w"
declare +n r=9     # bash declare -- w="9" / declare -- r="w"
                   # osh  declare -- w="5" / declare -- r="9"
declare -x +n r[1] # bash declare -ax w=([0]="5") / declare -- r="w"
                   # osh  declare -- w="5"        / declare -ax r=([0]="w")
```

The bare `declare +n r` is right by luck — it carries nothing else, so
there is nothing to misdirect.

**Proper fix.** Resolve the operand as usual (the follow must use the
pre-removal state, since bash reaches the target through the reference it
is about to drop), then take `nameref_attr` off the *original* name
rather than suppressing the follow. One wrinkle to keep: an operand whose
only content is `+n` stops there — bash leaves the target completely
untouched, where a subscripted operand with any other flag array-ifies it
(`declare +n r[1]` leaves `declare -- w="5"`, `declare -x +n r[1]` makes
`declare -ax w`). Low priority: `+n` combined with other flags is a shape
almost no script writes.

**✅ RESOLVED 2026-07-31.** `unset_nameref` no longer suppresses the
follow; it only redirects the letter it is about. The rule, measured
against bash 5.2.37 with probes `target/dvscratch/px69`–`px77`:

* **`+n` follows on the same terms as every other letter.** The gate is
  the existing `binding_is_nameref` rule plus `!make_local`: a
  local-binding `declare`/`local` writes the frame's own binding, so it
  never follows and its own name is what loses the attribute — even for a
  chain (`f() { local -n m=w; local -n r=m; declare -x +n r; }` exports
  `r`, leaving `m` a reference). A `-g` declaration does follow, because
  `enter_global_scope` has already swapped the global binding in.
* **The letter comes off the *last* reference in the chain**, not the
  operand's own name — bash reaches it through
  `find_variable_last_nameref`. With `w=5; declare -n m=w; declare -n
  r=m`, `declare -x +n r` exports `w`, leaves `m` a plain `"w"`, and
  leaves `r` a reference *to `m`*. New helper `Shell::last_nameref_of`
  does the walk, mirroring `resolve_ref_name`'s stopping conditions.
* **An operand whose only content is `+n` asks nothing of the target.**
  A subscript is not a request (`declare +n 'r[1]'` makes no array,
  though `declare -x +n 'r[1]'` makes `declare -ax w=([0]="5")`) and
  neither is `-g`. The new `other_attrs` predicate collects every letter
  that *is* one; when none is set and there is no value, the attribute
  comes off and the operand stops there.
* **A cycle is a complete no-op.** `declare -n c1=c2; declare -n c2=c1;
  declare -x +n c1` warns and changes nothing — not even the attribute —
  which falls out of the existing "circular chain names nothing to
  declare" `continue`. A *subscripted* cycle operand still unreferences
  itself and makes its own array, so the skip re-checks
  `nameref_attr.contains` to stay out of that path.
* **A refused operand keeps the attribute.** The removal sits with the
  other attribute applications, after every refusal's `continue`, so
  `readonly w; declare -n r=w; declare +n r=9` reports `r: readonly
  variable` and leaves `r` a reference.

Coverage: `userspace/oils/tests/corpus/declare-unset-nameref.sh` (11
sections, matching bash) and the unit test
`unset_nameref_takes_the_letter_off_the_reference_and_gives_the_rest_away`.

Two corners deliberately left divergent, both recorded as
TD-OILS-DECL-UNSET-NAMEREF-BASH-CORNERS below.
