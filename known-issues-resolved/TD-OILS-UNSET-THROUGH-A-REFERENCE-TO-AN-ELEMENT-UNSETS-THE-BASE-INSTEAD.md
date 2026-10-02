### TD-OILS-UNSET-THROUGH-A-REFERENCE-TO-AN-ELEMENT-UNSETS-THE-BASE-INSTEAD. `declare -n base=n; declare -n r='base[0]'; unset -v r` deleted `base` where bash removes `n[0]` — 2026-08-05 — ✅ **FIXED 2026-08-05**

**Where:** `userspace/oils/src/interp.rs` — `builtin_unset`'s reference branch.

**What.** `unset` on a reference designating an element removes the *element*,
and bash follows the base through a nameref chain of its own to find it — the
read side's rule, not the store side's (which binds the base where it is
written; see
TD-OILS-A-NAMEREF-BASE-IS-FOLLOWED-ON-A-WRITE-WHERE-BASH-BINDS-THE-BASE-ITSELF).
Measured with `n=(a b c)`:

```text
                                      bash                   osh (before)
declare -n base=n; declare -n r='base[0]'
  unset -v r                          `declare -n base="n"`  `base` deleted
                                      `declare -a n=([1]="b" `n` untouched
                                      [2]="c")`
```

osh subscripted the *unresolved* base, which for a nameref means element 0 of a
scalar whose value is the target's name — and unsetting element 0 of a scalar
deletes the variable, so `base` disappeared entirely.

**Fixed 2026-08-05.** The branch resolves the base with
`Shell::resolve_ref_use` before removing anything. **One walk**, and measured: a
circular base is reported once, removes nothing, and is not an error — unlike
the read side, which resolves twice because it looks the name up again to fetch
the value. A base that resolves nowhere (unset, or designating an element
itself) removes nothing and is not an error either.

The rest follows from having the right name: an associative base removes by
key, a scalar base is removed whole at index 0, and `base[@]` empties the array
it found. A subscript the writer *wrote* (`unset 'base[0]'`) already followed
the base and is untouched.

**Corpus:** `unset-through-a-reference-to-an-element-follows-the-base.sh`.
