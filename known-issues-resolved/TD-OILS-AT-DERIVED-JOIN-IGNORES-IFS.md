### TD-OILS-AT-DERIVED-JOIN-IGNORES-IFS. a derived `[@]` list joined with a hard-coded space — 2026-08-04 — ✅ **RESOLVED 2026-08-04**

**Where:** `userspace/oils/src/interp.rs` — the `[@]` branch of
`Shell::join_elements`, which every one of its six callers shared.

**What.** The `[*]` fix above (TD-OILS-STAR-BULK-JOIN-IGNORES-IFS) took the
`[@]` half on trust: `"${a[@]}"` glues its elements with a space under every
`$IFS`, so a space looked like what `[@]` *means*. It is not. That is only true
of the parameter's own elements — and of `"${a[@]:-w}"`, which is that same list
whenever the array is non-empty. Every list an *operator* derived joins with the
first character of `$IFS`, exactly as the `[*]` spelling does:

```sh
declare -a n=(p q r); IFS=:
x="${n[@]}";     echo "[$x]"   # bash [p q r]         osh [p q r]   ✅
x="${n[@]:-w}";  echo "[$x]"   # bash [p q r]         osh [p q r]   ✅
x="${!n[@]}";    echo "[$x]"   # bash [0:1:2]         osh [0 1 2]
x="${n[@]:0:2}"; echo "[$x]"   # bash [p:q]           osh [p q]
x="${n[@]@Q}";   echo "[$x]"   # bash ['p':'q':'r']   osh ['p' 'q' 'r']
x="${n[@]^^}";   echo "[$x]"   # bash [P:Q:R]         osh [P Q R]
x="${n[@]#p}";   echo "[$x]"   # bash [:q:r]          osh [ q r]
```

Where the two spellings still part company is an *empty* `$IFS`: `[*]` joins
with nothing, `[@]` falls back to a space. That fallback is what hid the bug —
under the default ` \t\n`, and under an unset `IFS`, both rules produce a space,
so nothing short of setting `$IFS` to a non-space can see it.

**Fixed in `88f761417`.** `join_elements` now means the parameter's own elements
(`[*]` → `star_sep`, `[@]` → space) and keeps its two callers, `expand_array_ref`
and `ArrayOp`; the four derived sites — `ArrayKeys`, `VarNames`, `ArraySlice`,
`ArrayBulk` — go through a new `join_derived` (`[*]` → `star_sep`, `[@]` →
`at_sep`), with `at_sep` being `star_sep` with the empty case raised to a space.
Two rules, named for which one each site wants, rather than one rule that was
right about `[*]` and wrong about `[@]`.

Covered by the corpus case
`a-an-at-reference-joins-a-derived-list-with-ifs.sh` (a full match against
bash 5.2.37 over five `IFS` values × the four space-joining forms and the ten
derived ones, an unset `IFS`, and the `[*]` spellings for contrast) and by the
lib test `an_at_reference_joins_a_derived_list_with_ifs`.

**Impact.** Any `"${a[@]…}"` operator form read as a single string under an
`$IFS` that is not a space — the `IFS=:` and `IFS=$'\n'` idioms. Sibling of the
`[*]` bug and found the same way: by measuring the half that had been assumed
rather than tested.
