### TD-OILS-AN-ARRAY-A-FAILED-ELEMENT-STORE-BROUGHT-INTO-BEING-IS-INVISIBLE-IN-OSH-AND-VISIBLE-IN-BASH. `declare 'z[]=v'` leaves `declare -a z=()` in bash and `declare -a z` in osh — 2026-08-06 — ✅ FIXED 2026-08-06

**Where:** `userspace/oils/src/interp.rs` — `builtin_declare_scoped`, the store
path a valued subscripted operand takes, and whatever marks a freshly made array
invisible.

**What.** bash prints a declared-but-unset array as `declare -a z` and one that
exists with no elements as `declare -a z=()` — the `att_invisible` distinction.
An element store whose *subscript* is refused has already made the array by the
time it fails, and makes it visible:

```text
$ ( declare 'z[]=v'; declare -p z )
bash: z[]: bad array subscript / declare -a z=()
osh : z[]: bad array subscript / declare -a z
```

The valueless `declare 'z[1]'` agrees on both sides (`declare -a z`, invisible),
and so does the same store onto an array that already exists, so this is only
about the one case that *creates* the array and then fails.

**Why.** It is not the store that makes the array visible but the *declaration*.
`declare.def:786` creates the variable a subscripted operand needs with
`make_new_array_variable`, which leaves it visible, and re-hides it only
`if (offset == 0)` — i.e. only when the operand carried no value at all. So the
visibility is decided before the subscript is ever looked at, and the store's
own failure cannot take it back. osh made the array as a side effect of the
declaration and left it in the invisible state the *valueless* form wants.

**Fixed.** `builtin_declare_scoped` now reads whether an array of either kind
was already bound (`array_existed`, captured beside `was_assoc`, before
`array_kind_apply` creates one), and the failed-store arm marks the name valued
when `!array_existed && !make_local`. The two limits are bash's own and both are
measured: an array that was already there keeps whatever visibility it had
(`declare -a z; declare 'z[-5]=v'` still prints `declare -a z`), and a local one
is made by `make_local_array_variable`, which hides it unconditionally — so
`local 'z[]=v'` and a plain `declare 'z[]=v'` inside a function both print
`declare -a z`, while `declare -g 'z[]=v'` in the same function is back on the
global path and prints `=()`. Covered by
`a-declaration-operand-is-truncated-at-its-bracket-before-anything-looks-at-it.sh`.
Probes: `/d/tmp/hh/cf.sh` (K1 vs K2), `/d/tmp/hh/dd.sh` (S/B series).
