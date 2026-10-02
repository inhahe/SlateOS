### TD-OILS-A-BAD-SUBSCRIPT-BLAMES-THE-NAME-THE-WALK-LANDED-ON-EVEN-WHERE-NOTHING-IS-THERE. `declare -n r=yy; ${r[-9]}` said `yy` where bash says `r` — 2026-08-11 — FIXED 2026-08-11

**Where:** `userspace/oils/src/interp.rs`,
[`Shell::expand_array_ref_resolved`] (the plain `${r[i]}` read) and
[`Shell::param_elem_resolved`] (the read every operator — `:-`, `:=`, `:?`, a
trim, a slice, `@Q` — takes its base value through). Both blamed the name the
nameref walk landed on, whether or not a variable of that name was there.

```sh
$ ( declare -n r=yy; echo "[${r[-1]}]" )
bash: r: bad array subscript / []
osh : yy: bad array subscript / []

$ ( declare -n r=yy; echo "[${r[-1]:-D}]" )
bash: r: bad array subscript / [D]
osh : yy: bad array subscript / [D]

$ ( zz=(a b); declare -n r=zz; echo "[${r[-9]}]" )      # both: zz
```

**Why bash does it.** `INDEX_ERROR()` (arrayfunc.c:1456) asks what the
*lookup* answered, not what the walk named:

```c
  if (var) err_badarraysub (var->name);
  else { t[-1] = '\0'; err_badarraysub (s); t[-1] = '['; }
```

`var` is what `array_variable_part` → `find_variable` handed back. A nameref is
followed inside that lookup, so a chain that lands on a name which *is* there
blames the name it landed on — and one that lands on a name with no entry
hands back a null `var` like any other failed lookup, so it blames the word as
written, cut off at its `[`. `declare -n r=yy` with `yy` unset therefore reads
exactly like the plain `${nope[-9]}` it is: `r`, not `yy`.

The distinction is `find_variable`'s and not `invisible_p`'s — a bare
`declare yy` leaves an invisible variable behind, and that counts as found, so
`declare yy; declare -n r=yy; ${r[-1]}` says `yy`.

The whole question only arises for the arithmetic branch: an associative
lookup has its variable in hand before it can judge an empty key, and the
*length* form (`${#r[-1]}`) quotes the subscript source rather than any name.
The writes are a different function again and quote the reference whole.

**Fixed** in this commit. Both readers now fall back on the name as written
when [`Shell::var_entry_exists`] — which is `find_variable != NULL`, invisible
variables included — says nothing is there.
[`Shell::param_elem_resolved`] takes the written name as a parameter for it;
the check runs inside [`Shell::in_target_scope`], since the entry has to be
looked for in the frame the walk arrived in and not the one the read was
spelled in (`f() { local zz=1; echo "${r[-9]}"; }` with `declare -n r=zz`
blames `zz`, the same reference from a frame without one blames `r`).

Corpus: `a-bad-subscript-blames-the-variable-the-lookup-found-not-the-walk.sh`.
Unit test: `a_bad_subscript_blames_the_variable_the_lookup_found_not_the_walk`.

**How it was found:** writing the corpus for
TD-OILS-A-DEFAULT-ASSIGNMENT-THROUGH-A-CYCLE-STORES-IN-THE-FRAME-IT-RAN-IN,
whose last row — `f() { declare -n r=zz; echo "[${r[-1]:=w}]"; }` — diverged in
the *read* half of the `:=`, ahead of the store the row was about.
