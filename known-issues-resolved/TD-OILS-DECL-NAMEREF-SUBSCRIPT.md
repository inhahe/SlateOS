### TD-OILS-DECL-NAMEREF-SUBSCRIPT. A subscripted `declare` operand naming a nameref is neither resolved nor un-referenced — 2026-07-31 — ✅ RESOLVED 2026-07-31

**Where:** `userspace/oils/src/interp.rs` — `builtin_declare`, whose
nameref resolution is deliberately skipped for a subscripted operand
(`subscript.is_none()` in the `follow` condition) because bash's answer
there is a *different* rule, not the same one — and osh implements
neither half of it.

bash splits on whether the subscripted operand carries a **value**:

* **Without** a value it resolves like any other operand — the
  attributes land on the target's base, the reference survives, and
  nothing is warned about.
* **With** a value it takes the nameref attribute *away* and declares the
  operand as an array of its own, leaving the target untouched. This
  happens whatever the reference points at — a scalar, a whole array, or
  an element.

```sh
w=5; declare -n r=w; declare -i r[1];  declare -p w r
# bash: declare -ai w=([0]="5") / declare -n r="w"
# osh : declare -- w="5")       / declare -ain r=([0]="w")

w=5; declare -n r=w; declare r[1]=9;   declare -p w r
# bash: warning: r: removing nameref attribute
#       declare -- w="5" / declare -a r=([1]="9")
# osh : declare -- w="5" / declare -an r=([0]="w" [1]="9")

declare -a arr=(1 2); declare -n r=arr; declare -i r[1]; declare -p arr
# bash: declare -ai arr=([0]="1" [1]="2")      osh: declare -a arr=(…) (unchanged)
```

So osh gets the valueless form wrong by converting the *reference* into
an array (destroying it, exactly as TD-OILS-DECL-NAMEREF-UNRESOLVED
described for the unsubscripted `declare -a r`), and the valued form
wrong by keeping the `-n` attribute on what is now an array.

**Proper fix.** In `builtin_declare`, widen the `follow` condition to
resolve a subscripted operand that carries no value — using the resolved
`base` and *keeping the operand's own subscript* (a reference that
already designates an element is a subscript on a subscript, which
`apply_assignment` already refuses via `warn_elem_not_identifier`; reuse
that). For a subscripted operand that does carry a value, emit
`warning: NAME: removing nameref attribute`, drop `NAME` from
`nameref_attr`, and declare `NAME` itself. Low priority: a subscripted
operand on a nameref is a shape almost no script writes.

**✅ RESOLVED 2026-07-31.** Implemented, with the rule stated the way the
measurements (probes `nr30`–`nr35`) actually support: what a subscripted
operand declares is decided by **which name can hold the array it needs
to store into**.

* **Valueless** — nothing is stored, so it resolves like any other
  operand, and the operand's own subscript rides along on the resolved
  base (`declare -i r[1]` → `declare -ai w=([0]="5")`, the reference
  intact).
* **Valued, writing the reference's own binding** (global scope, or
  `-g`) — the array can only be the operand's, so the reference is
  dropped with `warning: NAME: removing nameref attribute` and the
  operand declares itself. The reference's stored value is the target's
  *name*, which would otherwise survive as element 0, so the new
  `Shell::unreference_for_declare` takes it away with the attribute.
* **Valued, binding a local array** — bash builds that from the resolved
  name, so there it follows: `f() { local -n r=w; local r[1]=9; }` gives
  the frame a `w=([1]="9")` and leaves `r` the reference. (A reference
  the frame is only *shadowing* is not followed at all — see
  BUG-OILS-DECL-NAMEREF-LOCAL-SCOPE — so no reference is dropped there
  either, and nothing is warned about.)
* **No target to resolve to** — a circular chain — takes the same answer
  as the valued form: the array it makes is the operand's own.
* **Two subscripts** — a reference that already designates an element,
  subscripted again — are refused with
  `` declare: `arr[1][0]': not a valid identifier ``, status 1.

A `-n` operand may not carry a subscript at all, which was refused
nowhere before and left osh able to build a variable that was both an
array and a reference (`declare -n r[1]=w` → `declare -an r=([0]="w"
[1]="w")`). It now reports `declare: r[1]: reference variable cannot be
an array` with status 1, as bash does, for `declare`, `typeset` and
`local` alike.

Covered by a new section in `tests/corpus/nameref-declare.sh` and the
unit test
`a_subscripted_declaration_makes_an_array_of_whichever_name_can_hold_one`.
Full corpus 178 matched / 0 failed; 1041 unit tests; clippy clean on both
targets.

Two corners bash answers inconsistently are left alone and not asserted
in the corpus. `f() { local -n r=w; declare -g r[1]=9; }` makes bash both
store into `w` *and* leave an empty global `r=()`; and a circular chain
subscripted inside a function warns twice and leaves bash's `a` marked
both `-a` and `-n` at once — a state bash refuses to create by any other
route. osh gives the single-warning, single-effect answer in both.
