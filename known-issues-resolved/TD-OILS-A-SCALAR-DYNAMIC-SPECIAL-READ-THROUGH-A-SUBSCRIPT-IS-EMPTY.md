### TD-OILS-A-SCALAR-DYNAMIC-SPECIAL-READ-THROUGH-A-SUBSCRIPT-IS-EMPTY. `${SECONDS[0]}` / `${LINENO[@]}` / `${#PPID[0]}` read as unset — 2026-08-06 — ✅ FIXED 2026-08-06

**Where:** `userspace/oils/src/interp.rs` — `Shell::array_element` (~14404) and
`Shell::array_elements_walks` (~13657).

**What.** bash treats *any* scalar as a one-element array for subscripting
purposes: `t = (ind == 0) ? value_cell (var) : NULL` for `[0]`, and
`return (var_isset (var) ? 1 : 0)` for `[@]`/`[*]`. osh implements that, but
both helpers resolve the base name by falling back only to `self.vars.get(name)`
— and the *dynamic* specials (`SECONDS`, `LINENO`, `PPID`, `RANDOM`, …) have no
`vars` entry at all, so they read as unset:

```text
                        bash    osh
echo "${SECONDS[0]}"    0       (empty)
echo "${SECONDS[@]}"    0       (empty)
echo "${LINENO[0]}"     1       (empty)
echo "${#LINENO[0]}"    1       0
echo "${#PPID[0]}"      6       0
```

(`${#UID[0]}`, `${#PATH[0]}` and `${#GROUPS[0]}` also differ, but those are
value/environment differences between the two shells, not this bug.
`BASH_ARGC`/`BASH_ARGV` are the separate, already-logged
TD-OILS-MISSING-SPECIAL-ARRAYS.)

**Impact.** Small but silent: a script that subscripts a scalar special reads
empty instead of its value, and `${#SECONDS[0]}` reads 0.

**Fixed 2026-08-06.** A single accessor, `Shell::scalar_for_subscript`, now
answers "the scalar this name subscripts" — the ordinary binding when there is
one, and otherwise `Shell::dynamic_special_value` — and `array_element`,
`array_elements_walks` and `array_keys` all read through it. That last name is
the one the measurement did not predict: `${!SECONDS[@]}` must answer `0`, and
the key list had the same `self.vars.contains_key` gate.

The `&mut` was taken through rather than shimmed around, because the touch *is*
the semantics: reading a computed scalar through a subscript fills its value
cell exactly as an unsubscripted read does, so `: "${SECONDS[0]}"` starts it
listing as `declare -i SECONDS="0"` and `$(( RANDOM[0] ))` draws a number. That
last spelling made `VarLookup::get_index_str` `&mut self` too, matching the
`&mut` its sibling `get_str` already had for the same kind of reason.

Two things deliberately did *not* soften, both because they are questions about
the variable rather than its value: a scalar still has no highest index to count
back from (`${SECONDS[-1]}` is `bad array subscript`), and a scalar still has no
array shape, so `set -u; ${#SECONDS[0]}` still faults naming the base alone.

Measured before and after over 8 specials × 13 spellings: 65 divergent rows
before, 0 after (the 15 rows that still print differently are pid, path and
clock *values*, which differ between the two shells by construction). Locked by
`userspace/oils/tests/corpus/a-computed-scalar-answers-a-subscript-like-any-other.sh`.
