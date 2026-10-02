### TD-OILS-A-NAMEREF-TO-AN-ELEMENT-WAS-READ-BY-ONLY-ONE-FORM. `declare -n r='n[1]'` then `"${r#c}"` was empty, and a reference to a whole array read as its first element — 2026-08-05 — ✅ **FIXED 2026-08-05**

**Where:** `userspace/oils/src/interp.rs` — `Shell::param_elem_lookup`,
`Shell::ref_target_value`, `Shell::ref_length`, and `VarLookup for Shell`'s
`get_str`.

**What.** `declare -n r='n[1]'` makes `r` a name for one *element*. A plain
`"${r}"` read it (via `nameref_elem_value`), but every **other** form that reads
a parameter's value did not: `param_elem_lookup` resolved the chain, asked
`RefTarget::into_name()` for a variable name, got `None`, and answered *unset*.
So the whole operator family came back empty — and the `:-`/`:+`/`:?`/`:=`
family, which decides "set or unset" by that same read, took the wrong branch:

```text
                bash        osh (before)
"${r}"          c d         c d        ✅
"${r#c}"         d          (empty)
"${r^^}"        C D         (empty)
"${r/ /-}"      c-d         (empty)
"${r:0:2}"      c           (empty)
"${r@Q}"        'c d'       (empty)
"${r:+Y}"       Y           (empty)
"${r?msg}"      c d         r: msg  — and the script died
"${r:=Z}"       c d         `n[1]': not a valid identifier
```

A reference may also name a **whole** array — `declare -n g='n[*]'` or `'n[@]'`
— and there `resolve_ref_name` handed back `sub: Some("*")`, which the readers
evaluated as an ordinary subscript *expression*. `*` arithmetics to `0`, so
`"${g}"` was the array's **first element** rather than its elements joined.

**Fixed 2026-08-05.** `Shell::ref_target_value` is now the one place that says
what a subscripted reference reads as, and `param_elem_lookup` consults it for
an unsubscripted read instead of dropping the target. `[*]`/`[@]` are recognised
there rather than evaluated: bash hands a whole-array reference on as a
*string*, its elements already joined, so both spellings read as one scalar and
every operator downstream works on that join. The separators are
`Shell::join_derived`'s, which is why a null `$IFS` parts the two spellings —
`abcc de` for `[*]` against `abc c d e` for `[@]`. An array with no elements is
*unset* rather than empty, so `${g-D}` takes the default.

Two answers cut the other way and are the reason the fix is not simply "read
through everywhere":

* `${#r}` is **0** — for every element or whole-array reference, indexed or
  associative, set or unset. bash's length path asks for a *variable* named
  `n[1]`, and no variable can be called that. Turning `set -u` on changes the
  answer to the length of the value (`3`, and `9` for the join), the check
  sending the question down the full expansion route instead; an unset element
  is still `0` there rather than a fault. `Shell::ref_length` holds both
  columns.
* Arithmetic does not see the join at all. `$((g))` *subscripts* the array,
  finds `[*]` where an index belongs, and reports `n[*]: bad array subscript`
  before evaluating 0 — except on an associative array, where `[*]` is a key
  like any other and simply names no entry. `get_str` holds that.

**Corpus:** `a-nameref-to-an-element-is-read-by-every-form-that-reads-it.sh`.
