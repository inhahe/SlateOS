### TD-OILS-A-FLATTENED-ALIAS-TABLE-WAS-SORTED-THE-WRONG-WAY-AND-COMPGEN-NOT-AT-ALL. `compgen -A alias` answered in hash order, and `alias` put a high byte last — 2026-08-06 — ✅ FIXED 2026-08-06

**Where:** `userspace/oils/src/interp.rs` — `Shell::builtin_alias`'s listing and
the `alias`/`command` arms of the `compgen` action loop.

**What.** bash has exactly one flattened view of the alias table,
`all_aliases()`, and three readers of it: the `alias` / `alias -p` listing,
`compgen -A alias`, and the alias run at the head of a command completion
(`command_word_completion_function`, bashline.c:2105). osh had two bugs, one per
half:

* `compgen` did not sort at all — it handed back the hash table's own
  enumeration order, which is `BASH_ALIASES`'s order and a different thing.
* the listing sorted, but by plain byte order, where bash's comparator is
  `qsort_alias_compare` (alias.c:135):

  ```c
  if ((result = (*as1)->name[0] - (*as2)->name[0]) == 0)
    result = strcmp ((*as1)->name, (*as2)->name);
  ```

  The leading test looks like an optimization, but `name[0]` is a signed `char`
  while `strcmp` compares as *unsigned* char, so the two halves disagree: a name
  led by a byte at or above `\x80` is negative to the shortcut and sorts ahead
  of every ASCII name, while a shared first byte hands the rest of the name back
  to unsigned order.

```text
$ alias $'\xff'=1 a=2; alias
bash: \xff first, then a
osh : a first, then \xff        # plain byte order

$ alias zqqa=1 zqqb=2 zqqc=3 zqqd=4; compgen -A alias
bash: zqqa zqqb zqqc zqqd
osh : zqqa zqqc zqqb zqqd       # the table's order, unsorted
```

**Fix (2026-08-06).** A free `alias_name_order` implements bash's comparator,
and a `Shell::sorted_alias_names` is the single flattening all three readers go
through — so a fourth reader cannot pick the wrong order by accident.

**Tests.** `tests/corpus/a-flattened-alias-table-is-sorted-by-bashs-own-comparator.sh`
(every section discriminates: the listing, the tie-break, both compgen
spellings, the command completion, and the contrast against the unsorted
`BASH_ALIASES` mirror) plus the
`a_flattened_alias_table_is_sorted_by_bashs_own_comparator` unit test.
