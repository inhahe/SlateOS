### TD-OILS-RO-ARRAY. `osh` `readonly -p` uses `readonly name=val` and can't format array vars — 2026-07-19 — ✅ FIXED 2026-07-19

**Status:** FIXED. `builtin_readonly`'s listing branch now reuses
`format_declare_def` (falling back to `declare {flags} {name}` for valueless
readonly names), so scalars print as `declare -r name="value"` and arrays as
`declare -ar name=([0]="v" …)` / `declare -Ar name=(…)`, matching bash. With
the listing fixed, `BASH_VERSINFO` is now seeded **readonly** in
`seed_shell_vars` (clobbering it is rejected, and it appears correctly in
`readonly -p`). Additionally, inline array literals on `readonly`/`export`
(`readonly arr=(1 2)`, `export -A m=([k]=v)`) are now supported — the parser's
`is_declaration_command` accepts them and `exec_declare_with_arrays` applies
the implied `-r`/`-x` attribute — and unsetting an element of a readonly array
is now correctly rejected. Covered by `readonly_print_lists_vars`,
`readonly_export_array_literal`, and `readonly_array_element_cannot_unset`.

**Where (was):** `userspace/oils/src/interp.rs` `builtin_readonly` listing branch
(~7370–7380): it looks up each readonly name only in `self.vars` and prints
`readonly {name}={value}` (or `readonly {name}` when not a scalar).

**What:** bash's `readonly -p` prints `declare -r name="value"` for scalars
and `declare -ar name=([0]="v" …)` for arrays — it reuses the `declare -p`
formatting. osh instead prints `readonly a=1` for scalars (no quoting, wrong
keyword) and, for a readonly *array*, just `readonly NAME` with no contents
(the value is in `self.arrays`/`self.assoc`, which the listing never
consults). Reproduce: `readonly a=1; readonly -p` → bash `declare -r a="1"`,
osh `readonly a=1`; `readonly arr=(1 2); readonly -p` → bash
`declare -ar arr=([0]="1" [1]="2")`, osh `readonly arr`.

**Impact today:** this is why `$BASH_VERSINFO` is seeded **non-readonly**
(see `seed_shell_vars`): marking it readonly would surface it in
`readonly -p` as a bare `readonly BASH_VERSINFO` with no value, an obvious
wrong output. Fixing the listing lets BASH_VERSINFO be made readonly to
match bash.

**Proper fix:** rewrite the `readonly -p` / bare-`readonly` listing to reuse
`format_declare_def` (which already renders scalars, indexed, and
associative arrays with attribute flags), substituting the `-…r…` flag
group, exactly as `declare -p` does. Update `readonly_print_lists_vars` and
add array coverage. (Note bash uses `declare -r`, not `readonly`, as the
keyword in this listing.)
