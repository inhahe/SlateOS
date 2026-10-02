### TD-OILS-STAR-BULK-JOIN-IGNORES-IFS. every `[*]` reference except `${a[*]}` itself joined with a hard-coded space — 2026-08-04 — ✅ **RESOLVED 2026-08-04**

**Where:** `userspace/oils/src/interp.rs` — the `ArraySlice`, `ArrayBulk`,
`ArrayKeys` and `VarNames` arms of `Shell::expand_part`, each of which ended in
`.join(b" ")`.

**What.** `"${a[*]}"` joins with the first character of `$IFS`, and so does
every *other* list a reference can make — but only the first one asked:

```sh
declare -a n=(x y z); zz1=1; zz2=2
IFS=:
echo "[${n[*]}]"      # bash [x:y:z]        osh [x:y:z]   ✅
echo "[${!n[*]}]"     # bash [0:1:2]        osh [0 1 2]
echo "[${!zz*}]"      # bash [zz1:zz2]      osh [zz1 zz2]
echo "[${n[*]:0:3}]"  # bash [x:y:z]        osh [x y z]
echo "[${n[*]@Q}]"    # bash ['x':'y':'z']  osh ['x' 'y' 'z']
```

Found while measuring TD-OILS-WHOLE-ARRAY-TRANSFORM-IS-ONE-WORD, whose probe
happened to set `IFS`. The `[@]` spellings were right, and so was
`${a[*]:-word}` — `ArrayOp` already had the two-branch join — which is exactly
what made the bug invisible: the rule was written out in two places and only one
of them was kept up to date, the same drifted-duplicate hazard that
TD-OILS-TRANSFORM-SCALAR-IGNORES-SUBSCRIPT turned on.

**Fixed in `a0e1ae84d`.** The rule now lives in one place —
`Shell::join_elements(&elems, star)` — and all six sites go through it, the two
that were already correct (`expand_array_ref`'s `[@]`/`[*]` and `ArrayOp`)
included. Nothing about the *list* enters into it, which is the point: the join
belongs to the reference.

Covered by the corpus case
`a-a-star-reference-joins-with-ifs-whatever-list-it-made.sh` (a full match
against bash 5.2.37 over five `IFS` values × seven star forms, the `[@]` count
for each, an unset `IFS`, and the positional spellings) and by the lib test
`a_star_reference_joins_with_ifs_whatever_list_it_made`.
