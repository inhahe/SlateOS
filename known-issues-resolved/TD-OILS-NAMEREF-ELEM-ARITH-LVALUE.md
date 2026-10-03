### TD-OILS-NAMEREF-ELEM-ARITH-LVALUE. `((ref[i]=v))` through a nameref that names an element writes an element where bash refuses the name — 2026-07-28 — ✅ RESOLVED 2026-07-28

**Where:** `userspace/oils/src/interp.rs` — `Shell::arith_elem_base`, which
strips a nameref target's subscript with `nameref_target_base` and then indexes
the array it names.

**What.** With `declare -n r=q[0]`, the reference already designates one element.
bash therefore has no room for a *further* subscript and rejects the whole
target as a name:

```
$ bash -c 'declare -n r=q[0]; ((r[1]=5))'
bash: line 1: ((: `q[0]': not a valid identifier
$ osh -c 'declare -n r=q[0]; ((r[1]=5)); declare -p q'
declare -a q=([1]="5")
```

Scalar writes through such a nameref (`r=5`, `read r`) are correct — see
`ScalarDest::Elem` — it is only the *subscripted* arithmetic form that diverges.

**Proper fix.** `arith_elem_base` should distinguish "the nameref names a plain
variable" from "the nameref names an element", and report
``((: `TARGET': not a valid identifier`` for the latter instead of stripping the
subscript.

**Impact.** Very narrow. Kept out of `tests/corpus/nameref.sh`.

**✅ RESOLVED 2026-07-28.** `arith_elem_base` returned a bare `String` and so had
no way to say "this reference does not name an array at all" — every caller got
a name and indexed it. It is now `arith_elem`, returning `ArithElem::Array(name)`
or `ArithElem::Element(target)`, and the four callers answer the second case for
themselves.

**The refusal is a warning, not an error, which is the part worth recording.**
Measured against bash: the store is dropped and the message printed, but the
expression still carries its value and the command still reports success —
`x=$(( r[1]=5 ))` prints the complaint, sets `x=5` and exits 0. So it must not
travel as an `arith::ArithError` (which would fail the command); the write sites
emit the diagnostic and return `Ok(())`. `(( r[1]++ ))` does exit 1, but only
because the *read* is 0, which is the ordinary "expression evaluated to zero"
status.

Reading is silent: `$(( r[1] ))` is 0 in both shells, with nothing on stderr, so
only the write paths complain. The blamed name is the target text the nameref
holds (`q[0]`, `m[k]`) — not the reference, not the base — and it takes the same
builtin tag an arithmetic error would (`((: `, `let: `, and nothing at all
inside `$(( ))` or a `declare -i` value), which fell out of reusing
`Shell::arith_cmd`.

**Coverage.** `tests/corpus/nameref.sh` gained a section covering all of it:
`((`, `let`, `$((`, the read, the associative-element reference, a reference to
the array itself as the control, and a scalar write through the same element
reference (which still lands). Full differential corpus: 108 matched, 0 failed.
