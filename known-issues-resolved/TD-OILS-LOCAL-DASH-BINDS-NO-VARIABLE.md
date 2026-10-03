### TD-OILS-LOCAL-DASH-BINDS-NO-VARIABLE. `local -` saved the shell options but did not bind the variable named `-` that bash binds alongside them — 2026-08-04 — ✅ FIXED 2026-08-04

**Where:** `userspace/oils/src/interp.rs` — the `local -` case in
`Shell::declare_compounds_scoped`, plus `Shell::declare_p_names` and
`Shell::local_print`.

**What:** `local -` does two things in bash 5.2.37, and osh did only the
documented one. Besides making the `set` options local to the call, it binds a
variable *named* `-` in the frame — a cell a lookup by name finds and that no
listing reports:

```sh
f() { local -; declare -p -; }; f          # bash: declare -- -   osh: -: not found
g() { declare -p -; }
h() { local -; g; }; h                     # bash: declare -- -   osh: -: not found
f2() { local w=W; local -; local -p; }; f2 # bash prints `local -` first; osh did not
```

The comment at the site asserted the opposite — "Handled as a special 'name',
not a variable, so it neither shadows nor creates a var called `-`" — with
nothing measured behind it. That claim is what hid the bug.

**Measured rule**, all bash 5.2.37 and all now covered by the corpus case
`local-dash-binds-a-variable-no-listing-reports.sh` and the unit test
`local_dash_binds_a_variable_no_listing_reports`:

- It is an ordinary local in scope terms: found from an inner frame by the
  usual scope walk, gone once the frame returns, inherited by a subshell, and
  a nested `local -` gives that frame its own.
- It never holds a value — `declare -p -` prints the bare `declare -- -` and
  `test -v -` is false — and `unset -` reports success while leaving it there.
- It is left out of **every** full listing: `declare`, `declare -p`, `set`,
  `compgen -v`, `compgen -A variable`, `declare -x`, `declare -r`, `export -p`
  and `readonly -p` all show zero matches. Only a lookup by name sees it.
- `local -p` *does* report it, but as the declaration that made it — the
  literal line `local -`, first, and once however many `local -` the frame ran
  and however late the last of them came. `local -p` is current-frame-only as
  always, so an inner frame's `local -p -` says `not found` even though
  `declare -p -` there succeeds.
- It takes no attributes: `declare -r -` / `declare -x -` / `typeset -` all
  fail the identifier check with `` `-': not a valid identifier `` and leave
  the cell as it was.
- `$-` is untouched — that is a special parameter, not this cell — under its
  own name and through `n=-; ${!n}` alike.

**Fixed 2026-08-04.** The `local -` case now calls `declare_local("-", …)` and
marks the name `declared`, which buys the whole of the scoping half for free
(the frame's snapshot/restore already handles `declared`). Hiding it needed one
filter, in `declare_p_names` — the single enumerator every table-walking
listing shares. The other two enumerators never see it: bare `set` and
`visible_var_names` both read `vars`, and the cell is only ever `declared`.
`local_print` emits the `local -` line ahead of the frame's ordinary names.
Nothing else needed a guard, because `-` is not a valid identifier and so no
other path can produce the name.

**Standing lesson:** the bug survived because a comment asserted it away. Do
not ship a claim about the reference shell's behaviour without a measurement
behind it — and when a comment says a construct is handled *specially*, that is
exactly the place to go and measure.
