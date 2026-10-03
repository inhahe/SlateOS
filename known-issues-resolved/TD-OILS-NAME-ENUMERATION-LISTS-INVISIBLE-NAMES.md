### TD-OILS-NAME-ENUMERATION-LISTS-INVISIBLE-NAMES. `${!prefix*}` and `compgen -A variable` named a bare `declare -a` — 2026-08-04 — ✅ **RESOLVED 2026-08-04**

**Where:** `userspace/oils/src/interp.rs` — `Shell::var_names_with_prefix` and the
`compgen` `variable`/`arrayvar` action arms.

**The bug.** bash generates `${!prefix*}`, `${!prefix@}`, `compgen -A variable`
and `compgen -A arrayvar` from one function, `all_visible_variables`, so all four
skip the names it marks `att_invisible` — the ones a declaration created without
assigning. osh got the scalar half right for free (a bare declaration never
enters `Shell::vars`) but listed *every* array-table entry, so a bare
`declare -a q` was named where bash names nothing. Measured against bash 5.2.37:

```sh
declare -a zz1;            echo "[${!zz*}]"   # bash []        osh [zz1]
declare -A zz3;            echo "[${!zz*}]"   # bash []        osh [zz1 zz2 zz3]
declare -a ua; compgen -A arrayvar ua         # bash (nothing) osh ua
```

`compgen -A variable BASH_SOURCE` was also answered *twice*, once from the array
table and once from `DYNAMIC_SPECIALS`; `${!prefix*}` had hidden that by sorting
and deduplicating its own result.

**Fixed in `b54a83a66`.** One `Shell::visible_var_names` now answers the question
for all four callers, sorted and deduplicated at the source so a name that is
both a table entry and a computed one is still one variable. Its array half asks
the new `Shell::array_is_visible`, which is the same test
`format_var_assignment` already used to choose between printing `a=()` and the
bare `a` — anything with an element in it was assigned, and an empty one was
assigned only if `Shell::array_valued` says so. (The pre-existing
`array_shape_exists` is a different question — whether there is an array there to
subscript at all — and a bare `declare -a a` answers yes to that while staying
invisible.) Corpus case
`a-a-name-enumeration-passes-over-a-bare-declaration.sh`; lib test
`a_name_enumeration_passes_over_a_bare_declaration`.
