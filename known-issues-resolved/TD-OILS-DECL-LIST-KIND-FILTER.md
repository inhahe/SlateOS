### TD-OILS-DECL-LIST-KIND-FILTER. A listing's `-a`/`-A` is a type *restriction*, not another term in the attribute union — 2026-07-30 — ✅ RESOLVED 2026-07-30

**Where:** `userspace/oils/src/interp.rs` — `declare_list_filtered`, the routing
gate above it in the `declare` arm, `Shell::export_list`, and
`builtin_readonly`'s nameless path.

**What.** Split out of TD-OILS-EXPORT-ARRAY-FLAG, which noticed only that
`export -a` with no operands ignored the flag. Investigating it turned up the
real, larger rule — and a worse instance of it in `declare`.

bash sorts the listing letters into two groups that combine differently:

* `a`/`A` are **type restrictions**, applied conjunctively;
* every other letter is an **attribute**, and those union among themselves.

`declare_list_filtered` unioned all of them together, so a restriction *widened*
the listing instead of narrowing it. Measured against 5.2.37 with
`plain=(1)`, `earr` exported-and-array, `foo` an exported scalar:

| call | bash | osh (before) |
|---|---|---|
| `declare -ax` | `earr` only | `earr`, `plain`, `foo`, *and the whole environment* |
| `declare -ai` | the arrays that are integer | every array **or** integer name |
| `declare -aA` | nothing — no name is both kinds | every array of either kind |
| `declare -ir` | union: integer **or** readonly | same (this half was right) |

`export -a` / `export -A` / `readonly -a` / `readonly -A` are the same rule seen
through builtins that pin one attribute letter: `export -a` is "the indexed
arrays that are exported". Those three listing paths ignored the flag entirely.

**Fixed 2026-07-30.** The rule is now one helper, `Shell::listing_kind_admits`,
used by all four listing paths — `declare_list_filtered` applies it before the
attribute union, and `export_list` and `readonly`'s nameless path apply it to
their own name sets. `listing_names` carries the restriction-then-union model
and `listing_flags` parses the letters once.

**A third bug fell out of the same de-duplication.** The listing letter set was
written out twice — once in the `declare` arm's routing gate, once in
`declare_list_filtered` — and *both* copies omitted `c`. So `declare -c` with no
names routed to a declaration that declared nothing and printed nothing, where
bash lists the capcase variables (`declare -c cv="Abc"`). The dead `'c'` arm
already sitting in the attribute match was the giveaway. Both copies are now
`listing_flags`, so they cannot drift again.

Covered by `a_kind_flag_restricts_a_listing_rather_than_widening_it` (all four
paths, plus the union case and the empty `-aA`) and
`declare_c_lists_the_capcase_variables`. Note the two containment-rather-than-
equality assertions: the shell's own `SHELLOPTS`/`UID`/`BASH_VERSINFO` belong in
an unrestricted union listing and in `readonly -a`, exactly as they do in bash's.
