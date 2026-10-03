### TD-OILS-DYNVAR-DECLARED. `export SECONDS` cost the name its value function, and never reached the child's environment — 2026-07-31 — ✅ RESOLVED 2026-07-31

**Where:** `userspace/oils/src/interp.rs` — `Shell::dyn_declared`,
`declare_dynamic_special`, `dynamic_special_listed_flags`,
`dynamic_special_letters`, `dynamic_special_names_with_attr`,
`listed_has_attr`, `listing_kind_admits`,
`format_dynamic_special_declare` / `_listing`, `format_var_setline`,
`dynamic_special_value`, `apply_child_env`, the bare-`set` listing in
`builtin_set`, `unbind_var`, and the three valueless-declaration sites in
`builtin_export`, `builtin_readonly` and the `declare` family.

The names bash answers with a value function (`SECONDS`, `RANDOM`,
`PPID`, … — see TD-OILS-DYNVAR-UNSET) already exist, so naming one in a
declaration builtin creates nothing: bash applies the attribute to the
binding that is there and stops. osh instead took the ordinary
valueless-declaration path, making a `Shell::declared` entry, which
`has_stored_binding` reports and `dynamic_special_listed` treats as a
shadow — so the name lost its whole table row, value and attributes
together:

```sh
export SECONDS;      declare -p SECONDS   # bash declare -ix SECONDS="12"   osh declare -x SECONDS
readonly SECONDS;    declare -p SECONDS   # bash declare -ir SECONDS="12"   osh declare -r SECONDS
declare -u RANDOM;   declare -p RANDOM    # bash declare -iu RANDOM="4931"  osh declare -u RANDOM
export PPID;         declare -p PPID      # bash declare -irx PPID="734"    osh declare -rx PPID
export SECONDS;      env | grep -c ^SECONDS=       # bash 1                 osh 0
```

The last of those is the one that is not merely cosmetic: an `export`ed
dynamic variable never reached a child process at all, because
`apply_child_env` walks `Shell::vars` and these names have no entry
there. Same for `RANDOM` and `LINENO`.

The other half of the divergence is what a declaration *does* cost the
name. bash marks these `att_invisible`, which keeps them out of the
listings that walk the variable table (bare `declare -p`, the
flag-filtered `declare -i`, a bare `set`) while `declare -p NAME` reports
them all along. A declaration clears the mark, and from then on the
listings carry the name in the same full form the named lookup gives —
`declare -i SECONDS="12"` rather than nothing, `declare -i RANDOM="4931"`
rather than a bare `declare -i RANDOM`.

**Fixed 2026-07-31.** `Shell::dyn_declared` records the dynamic specials a
declaration builtin has named; the three valueless-declaration sites call
`declare_dynamic_special`, which populates it and tells the caller to
leave `declared` alone. `dynamic_special_letters` unions the table's
letters with the attribute sets in bash's fixed order, and
`dynamic_special_listed_flags` switches a promoted name from the table's
`listed_flags` to its `named_flags` — together that is what turns `export
SECONDS` into `declare -ix SECONDS="12"` in `declare -p SECONDS` and in
every listing that admits it. `apply_child_env` now walks `exported` for
names with no `vars` entry and calls the value function, so the child
really sees `SECONDS=`. `format_var_setline` does the same for a bare
`set`. A `local` of the name is unaffected: it still makes a `declared`
entry, which now also stops `dynamic_special_value` answering — so
`f() { local SECONDS; echo "${SECONDS-UNSET}"; }` prints `UNSET` as in
bash, where before the dynamic value leaked through the shadow. `unset`
clears the promotion with the rest of the binding. Corpus case
`dynamic-var-declared.sh`; unit test
`declaring_a_dynamic_variable_keeps_its_value_function`.
