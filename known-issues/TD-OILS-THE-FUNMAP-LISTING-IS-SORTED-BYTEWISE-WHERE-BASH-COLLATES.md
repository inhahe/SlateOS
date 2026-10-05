### TD-OILS-THE-FUNMAP-LISTING-IS-SORTED-BYTEWISE-WHERE-BASH-COLLATES. `bind -P` orders `re-read-init-file` differently under `en_US.UTF-8` — 2026-08-07

**Where:** `userspace/oils/src/bind_tables.rs` — the `FUNCTIONS` list, which is
a static table in ASCII order, and every listing in `bind_keys.rs` /
`interp.rs` that walks it (`bind -l`, `-p`, `-P`, `-s`, `-S`, `-v`, and the
`complete -A binding` word list).

**What is wrong.** readline sorts the funmap at build time with `qsort` over
`_rl_funmap_compare`, which uses `strcmp` — but bash's *listing* of it comes
out in the locale's collation order, not byte order. Under `en_US.UTF-8` the
collation ignores `-` at the primary level and folds case, so bash puts
`redraw-current-line` before `re-read-init-file` (`redraw…` < `rer…` once the
hyphen is dropped) and files `vi-bWord` / `vi-eWord` / `vi-fWord` among their
lowercase neighbours. osh emits its static ASCII order in every locale.

**Repro:**

```sh
LC_ALL=en_US.UTF-8 INPUTRC=/dev/null bash -c 'bind -P' | sed 's/ can be found.*//;s/ is not bound.*//' > b
LC_ALL=en_US.UTF-8 INPUTRC=/dev/null osh  -c 'bind -P' | sed 's/ can be found.*//;s/ is not bound.*//' > o
diff o b
#   94d93  < re-read-init-file        (osh puts it before redraw-current-line)
#   123d122 < vi-bWord                (osh groups the capitalised ones together)
```

Under `C` and `C.UTF-8` the two agree, which is why the corpus has never seen
it: the harness pins `LC_ALL=C.UTF-8`, whose collation *is* byte order.

**Not** a regression from
TD-OILS-READLINES-META-VARIABLES-CARRY-THE-C-LOCALE-DEFAULTS — reproduced
against a binary predating that fix.

**Proper fix:** sort the listing through a collation function rather than
emitting the table order, and derive that function from `LC_COLLATE`. osh has
no locale database, so this is really a request for one: the minimum that would
close it is a UCA/DUCET-derived default collation applied whenever `LC_COLLATE`
is not `C`/`POSIX`, which is the same machinery `sort`, `[[ < ]]`, and
`${var,,}`-adjacent case mapping will each want. Worth doing as one piece of
work rather than as a special case for `bind`. Until then the divergence is
confined to listing *order* under non-`C` collations; every individual line is
correct.
