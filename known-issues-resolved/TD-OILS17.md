### TD-OILS17. `osh` namerefs (`declare -n`): all originally-listed edge cases now match bash — RESOLVED 2026-07-18

**Where:** `userspace/oils/src/interp.rs` (`resolve_ref_name` and the read/write
chokepoints: `param_value`, `param_elem_value`, `assign_elem`, `array_elements`,
`array_keys`, `expand_array_ref`, `apply_assignment`).

**What:** `declare -n ref=target` / `local -n` namerefs are implemented — reads
and writes of the nameref (scalar and array element, `${ref[@]}`, `${#ref[@]}`,
the pass-array-by-reference-to-a-function pattern) transparently redirect to the
target, chains are followed with a cycle guard, `declare -p` shows `-n`, and
`unset -n`/`unset` behave per bash. Remaining deviations:
1. ~~**`${!ref}` on a nameref** returns the target's *value* (ordinary indirect
   expansion), whereas bash returns the target *name*.~~ **FIXED 2026-07-18.**
   `expand_indirect` now special-cases a nameref `refname`: `${!ref}` follows
   the nameref chain (`resolve_ref_name`) and yields the final target *name*,
   while `$ref` still yields the target value. Regression: the
   `param_indirect_expansion` test.
2. ~~**Namerefs to an array element** (`declare -n ref=arr[0]`) are stored
   verbatim; `resolve_ref_name` returns `arr[0]` and the caller's subscript logic
   does not further interpret it, so `$ref` does not resolve to that element.~~
   **FIXED 2026-07-18.** Reads go through a new `nameref_elem_value` helper
   (splits `arr[0]`/`m[key]` and reads the element), and `apply_assignment`
   rewrites `ref=v` into `arr[0]=v` (synthesising the subscript word) when the
   resolved nameref target carries a subscript. Regression: `nameref_to_array_element`.
3. ~~**`local -n` scoping** uses the same global attribute set as the other
   `local` attributes (`-i`/`-l`/`-u`), which are not yet per-frame~~ — **FIXED
   2026-07-18.** The per-call `VarSnapshot` now captures and restores the
   `integer`/`lower`/`upper`/`nameref`/`readonly` flags along with the value, and
   `declare_local` clears `-i`/`-l`/`-u`/`-n` when shadowing (a bare `local x`
   does not inherit a global's attributes, matching bash; `readonly` is left
   intact so a readonly global is not silently shadowed). Regression tests:
   `local_integer_attr_does_not_leak`, `local_restores_shadowed_integer_attr`,
   `local_nameref_does_not_leak`.

**Proper fix:** all three items (1), (2), (3) are now fixed (see above). This
entry is retained for history; nameref behavior now matches bash for the
scalar, array-element, indirect-name, and `local`-scoping cases exercised by the
`nameref_*` and `param_indirect_expansion` tests.
