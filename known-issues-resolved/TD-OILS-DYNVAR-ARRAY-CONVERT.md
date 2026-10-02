### TD-OILS-DYNVAR-ARRAY-CONVERT. `declare -a SECONDS` makes an empty array instead of converting the live value — 2026-07-31 — ✅ RESOLVED 2026-07-31

**Where:** `userspace/oils/src/interp.rs` — the `array_kind_apply` /
valueless-declaration path of the `declare` family.

bash converts a *set* scalar to an array by making its value element 0,
and a dynamic special variable is set, so its computed value goes to
element 0 — and the conversion also kills the value function, leaving an
ordinary array:

```sh
declare -a SECONDS; declare -p SECONDS   # bash declare -ai SECONDS=([0]="7")   osh declare -a SECONDS
declare -A SECONDS; declare -p SECONDS   # bash declare -Ai SECONDS=([0]="7" )  osh declare -A SECONDS
```

(The `i` survives because in bash it is a real attribute on the binding,
not a property of the table row osh keeps.) osh's conversion reads
`Shell::vars`, where these names have no entry, so it converts nothing.
The fix is for the conversion to fall back to `dynamic_special_value`,
seed element 0 with it, carry the row's `named_flags` into the real
attribute sets, and mark the name in `dyn_unset` — the binding is now an
ordinary array and the value function is gone.

**Fixed.** `Shell::dyn_array_convert` does exactly that, and
`Shell::array_carried_value` is the one place both array kinds ask for
the value a conversion carries: the scalar in `vars` if there is one,
otherwise a last reading of the value function. The reading is taken
*then*, not lifted out of the value cell — after `SECONDS=7; sleep 2` the
conversion carries 9, and after `LINENO=7` it carries the line the
conversion is on. The letters come from the row's `named_flags` (the set
`declare -p NAME` reports, not the half-filled one a listing sees) and
become real attributes, so the carried `-i` then evaluates whatever a
literal supplies (`declare -a SECONDS=(3+4 9)` → `([0]="7" [1]="9")`) and
a converted `PPID` keeps refusing. `Shell::drop_dynamic_binding` — the
`unset` side effect, factored out of `unbind_var` — is what takes the
value function away, so a converted `LINENO` reports the same line for
ever after and `unset` on the converted name leaves nothing at all.

Two guards keep it off bindings that are not the dynamic one: the
call-stack arrays (`DynListing::EmptyArray`) are arrays already, and a
name a `local` has shadowed is in `Shell::declared`, so `local -a
SECONDS` builds a fresh empty local array and the shell's own binding is
still there when the function returns.

Three things reach the conversion, and all three were wrong before:
`declare -a`/`-A NAME`, the valued `export -a`/`readonly -a NAME=v`
forms, and a bare subscripted assignment (`SECONDS[1]=9`). The last one
did not widen *at all* — not even an ordinary scalar, so `w=5; w[1]=9`
gave `([1]="9")` where bash gives `([0]="5" [1]="9")`, and `w=5;
w[-1]=9` resolved against the wrong bound. The widening now happens
after the subscript is validated (a malformed one converts nothing, as
in bash) and before the bound is taken, and the integer attribute is
re-read afterwards because the conversion can bring one with it and bash
applies it to the very element being assigned (`SECONDS[1]=x` → 0).

Covered by `tests/corpus/dynamic-var-array.sh` and the unit tests
`giving_a_dynamic_variable_an_array_kind_converts_it` /
`converting_a_dynamic_variable_takes_its_value_function_away`.
