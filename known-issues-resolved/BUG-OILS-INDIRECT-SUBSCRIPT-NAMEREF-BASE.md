### BUG-OILS-INDIRECT-SUBSCRIPT-NAMEREF-BASE. A subscript reached through `${!ref}` did not follow a nameref base, so the element read as unset — 2026-07-28 — ✅ RESOLVED 2026-07-28

**Symptom.** When the name a reference resolves to is subscripted and its *base*
is itself a nameref, the element read as unset:

```sh
declare -n b=c        # b is a reference to c
b=(p q)               # so the array is really stored on c
ptr="b[1]"
echo "[${!ptr}]"      # bash: [q]   osh: []
```

Every per-value modifier reached through it was empty too
(`${!ptr^^}`, `${!ptr#q}`, `${!ptr@Q}`), `${!ptr:-D}` wrongly took the default,
and `${!ptr:=v}` failed with `b[1]: invalid variable name` where bash — finding
the element already set — assigns nothing and succeeds. A whole-array reference
(`ptr="b[@]"`) was unaffected, which is what made the gap so narrow: `b[@]` and
`b[1]` disagreed about the same variable.

**Where.** `userspace/oils/src/interp.rs` — `Shell::indirect_target_value`. It
split the target into `name` + subscript and passed `name` straight to
`array_element` / `assoc_element`, neither of which resolves references. This
was the *only* subscript lookup in the file that skipped that step: the direct
`${b[1]}` path, `array_elements`, and every other `array_element`/`assoc_element`
call site all run the base through `resolve_ref_use` first — which is exactly
why direct access worked while the indirect form did not.

**✅ RESOLVED 2026-07-28.** `indirect_target_value` now resolves the base with
`resolve_ref_use` before the assoc/array lookups, matching the convention used
everywhere else. `resolve_ref_use` (rather than the silent `resolve_ref_name`)
is right here: this is a genuine *use* of the base name, and it is reached only
after the reference itself has already been resolved by the caller, so it cannot
double-report the reference. A circular base emits one warning and expands to
nothing — value and exit status match bash; only the warning *count* differs,
which is the pre-existing TD-OILS-NAMEREF-WARNING-COUNT gap below, not new (and
closed on 2026-08-04).

**Coverage.** New corpus case `tests/corpus/indirection-modifiers.sh`, which
pins the whole `${!ref}`-plus-modifier surface against bash: an ordinary pointer,
a nameref (whose *name* is what the modifier works on), a name reaching an array
(which the per-value modifiers look through but the `${x:-…}` family does not),
element and whole-array pointers, and assignment through a reference. Full
differential corpus: 106 matched, 0 failed.
