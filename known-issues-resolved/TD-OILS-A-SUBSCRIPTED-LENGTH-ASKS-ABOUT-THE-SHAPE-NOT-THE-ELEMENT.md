### TD-OILS-A-SUBSCRIPTED-LENGTH-ASKS-ABOUT-THE-SHAPE-NOT-THE-ELEMENT. `${#name[sub]}` used the wrong invisibility test, the wrong abort flavour, and evaluated the subscript it should have skipped — 2026-08-06 — ✅ FIXED 2026-08-06

**Where:** `userspace/oils/src/interp.rs` — `Shell::expand_array_ref`'s `length`
short-circuit, `Shell::array_shape_exists`, and the new
`Shell::var_binding_is_visible` / `Shell::raise_unbound_length`.

**What.** A subscripted *length* is a separate machine from a subscripted
*reference* — `array_length_reference`, `subst.c:7214` — and it has two rules of
its own, both of which osh got wrong in a different way:

```c
if ((var == 0 || invisible_p (var) ||
     (assoc_p (var) == 0 && array_p (var) == 0)) && unbound_vars_is_error)
  { set_exit_status (EXECUTION_FAILURE); err_unboundvar (s); return (-1); }
else if (var == 0 || invisible_p (var))
  return 0;
```

Three separate divergences came out of measuring it, all now fixed:

1. **Wrong invisibility test.** `array_shape_exists` asked whether the name was
   in `Shell::declared`, which an array declaration never sets — a bare
   `declare -a q` leaves an *empty* entry in `Shell::arrays` instead. So
   `set -u; declare -a q; echo "${#q[0+]}"` evaluated the subscript and reported
   an arithmetic syntax error where bash says `q: unbound variable`. It now asks
   `Shell::array_is_visible`.

2. **The call-stack arrays do not agree about being invisible.** Switching to
   `array_is_visible` regressed `${#BASH_SOURCE[0]}` and `${#BASH_LINENO[…]}`,
   because bash *assigns* those an empty array at startup but creates
   `FUNCNAME` without assigning it — the very distinction osh's `DynListing`
   already draws (`declare -p` gives `declare -a BASH_SOURCE=()` but bare
   `declare -a FUNCNAME`). `array_shape_exists` now also accepts a dynamic
   special whose listing is `DynListing::EmptyArray`, so at a script's top level
   `set -u; echo ${#BASH_SOURCE[@]}` is 0 while `set -u; echo ${#FUNCNAME[@]}`
   is `FUNCNAME: unbound variable`.

3. **Wrong abort flavour.** The caller at `subst.c:9690-9707` turns
   `array_length_reference`'s `-1` into `&expand_wdesc_error`, not
   `expand_wdesc_fatal` — i.e. `jump_to_top_level (DISCARD)`, answered by
   `shell.c:1467` with **1**, not the FORCE_EOF 127 an ordinary nounset gets:

   ```text
   $ bash --norc -c 'set -u; echo "${#nope}"';    rc=127
   $ bash --norc -c 'set -u; echo "${#nope[0]}"'; rc=1
   ```

   Both still abort the whole `-c` input. The new `raise_unbound_length` arms a
   DISCARD worth 1 instead of reusing `raise_unbound`.

Also note rule 1 fires for anything *shapeless*, which includes a perfectly good
visible scalar: `set -u; s=hi; echo "${#s[0]}"` is `s: unbound variable` in bash.
And rule 2 means an invisible name answers 0 **without the subscript ever being
evaluated**, which is why `declare -a q; echo ${#q[0+]}` prints 0 where the
visible `q=(); echo ${#q[0+]}` reports the arithmetic syntax error.

Verified against a 540-case truth table (`set -u` on/off × 9 base names × 6
subscript forms × 5 operators), now 540/540. Locked by
`userspace/oils/tests/corpus/a-word-is-abandoned-at-its-first-fault-and-says-nothing-more.sh`.
