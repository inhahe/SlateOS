### TD-OILS-SET-LISTS-A-DECLARED-BUT-UNVALUED-ARRAY. `declare -a qq` then `set` prints a bare `qq`; bash prints nothing — 2026-08-05 — ✅ FIXED 2026-08-05

**Where:** `userspace/oils/src/interp.rs` — the `set`-with-no-operands listing.

**What.** `set` lists the shell's *variables and their values*, and bash leaves
out a name that has been declared but never valued. osh leaves out unvalued
scalars but not unvalued arrays:

```sh
f() { declare -a qq; declare -A qa; declare qs; declare -i qi; set | grep '^q'; }; f
# bash: nothing
# osh:  qa
#       qq
```

The two agree on `declare -p` throughout (`declare -a qq` either way), so this
is only the `set` listing. It surfaced while measuring
TD-OILS-A-DECLARATION-BUILTIN-THAT-BINDS-A-LOCAL-THROUGH-A-REFERENCE-TO-AN-ELEMENT-MAKES-A-LOCAL-NAMED-BY-THE-SPELLING,
where `declare -a r` through an element reference makes an unvalued array named
`n[1]` and osh printed it.

**Proper fix.** The listing should skip an array name that is not in
`array_valued` (the same flag `declare -p` uses to decide between `declare -a q`
and `declare -a q=()`), exactly as it already skips a scalar that is only in
`declared`.

**Impact.** Small and cosmetic, but `set` is what scripts diff to snapshot an
environment, so a spurious line changes the snapshot.

**Fixed 2026-08-05.** `Shell::format_var_setline` answers `None` for an array
that `Shell::array_is_visible` says was never valued, which is what both callers
— the bare `set` listing and `declare`'s set-style one — already wanted. An
empty *assigned* array (`q=()`) is visible and still lists. Corpus:
`declares-listing-flags-choose-which-names-are-listed.sh`, which already covered
the scalar half.
