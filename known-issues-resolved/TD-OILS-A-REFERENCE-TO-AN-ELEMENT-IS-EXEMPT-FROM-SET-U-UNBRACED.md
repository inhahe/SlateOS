### TD-OILS-A-REFERENCE-TO-AN-ELEMENT-IS-EXEMPT-FROM-SET-U-UNBRACED. `set -u; declare -n V='n[9]'; echo "$V"` said `V: unbound variable` where bash prints nothing and carries on — 2026-08-05 — ✅ **FIXED 2026-08-05**

**Where:** `userspace/oils/src/interp.rs` — `Shell::note_unbound_spelled`.

**What.** A nameref designating an array **element**, or a whole array, is
exempt from the `set -u` fault — **unbraced, and only unbraced.** bash reads
`$name` and `${name}` with two different functions, and only the braced one asks
whether the parameter is set: `param_expand` answers an array reference out of
its own branch, with the empty string, and never reaches the check. Measured
with `n=(a b c)` and `set -u`:

```text
                                 bash                    osh (before)
declare -n V='n[9]'
  echo "[$V]"                    `[]`, status 0          `V: unbound variable`
  for x in $V; do …; done        nothing, status 0       `V: unbound variable`
  echo "[pre$V post]"            `[pre post]`            `V: unbound variable`
  echo "[${V}]"                  `V: unbound variable`   the same — agreed
  echo "[${V#a}]" `${V^^}` …     `V: unbound variable`   the same — agreed
declare -n Q='nosuch[0]'
  echo "[$Q]"                    `[]`, status 0          `V: unbound variable`
declare -n E=nosuch
  echo "[$E]"                    `E: unbound variable`   the same — agreed
```

What the base names does not come into it: an unset base, and a circular one,
are exempt too. What matters is only that the reference resolves to a
subscripted target at all, which is why a chain ending on a plain name still
faults, and why a plain circular chain (`declare -n c1=c2; declare -n c2=c1`)
faults either way.

**Fixed 2026-08-05.** `note_unbound_spelled` already knew whether the reference
was written with braces — it needs that to spell `$1` against `1` — so the
exemption sits beside that, gated on `!braced` and on
`Shell::nameref_elem_target` answering at all. The `-`/`:-`/`+`/`:+` operators
are a separate question and keep their own answer: the element *is* unset for
them, so `${V-DEF}` is still `DEF`.

**Corpus:** `a-reference-to-an-element-is-exempt-from-set-u-unbraced.sh`.
