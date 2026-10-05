### TD-OILS-DECL-PLUS-X-R-SETS-INSTEAD-OF-CLEARING. `declare +x` / `declare +r` set the attribute they should remove — 2026-07-31 — ✅ RESOLVED 2026-07-31

**Where:** `userspace/oils/src/interp.rs` — `builtin_declare_scoped`'s
flag loop, whose `b'x' => export = true` and `b'r' => readonly = true`
arms ignore the `enable` flag that every other letter consults.

```sh
x=5; declare -x x; declare +x x; declare -p x  # bash declare -- x="5"   osh declare -x x="5"
x=5; declare +r x; declare -p x                # bash declare -- x="5"   osh declare -r x="5"
x=5; readonly x; declare +r x; echo "rc=$?"    # bash declare: x: readonly variable, rc=1
                                               # osh  (silent), rc=0
```

The `+r` half is destructive: asking to *remove* a readonly attribute
instead applies one, and nothing can take it off again. Found while
measuring TD-OILS-DECL-UNSET-NAMEREF-DOES-NOT-FOLLOW (`declare +n +x r`
left `r` exported).

**Proper fix.** Give both letters the `enable` test every other letter
already has: `+x` clears the export attribute, and `+r` is a no-op on a
name that is not readonly and reports `{tag}: {name}: readonly variable`
with status 1 on one that is (bash cannot remove the attribute at all).

**✅ RESOLVED 2026-07-31.** Both letters now carry an `unset_export` /
`unset_readonly` companion, and the rule measured against bash 5.2.37 is:

* **The two directions are separate sets, and "off" is applied last.**
  `declare -x +x v=hi` and `declare +x -x v=hi` both leave `v`
  unexported — a child never sees it — and `declare -r +r x=5` leaves an
  ordinary variable. So the applications are gated `if unset_export {
  remove } else if export { insert }` and `if readonly && !unset_readonly
  { insert }` rather than by flag order.
* **`+x` really removes.** It reaches the base array of a subscripted
  operand (`declare +x a[0]=9`), a compound operand's array (phase 3 of
  `exec_declare_with_arrays_scoped`), and a nameref's target, exactly as
  `-x` does — and it strips the export off a *readonly* name too, since
  only the `+r` half is refused.
* **`+r` never removes anything.** On a name that is not readonly it is a
  plain no-op and the declaration proceeds as if absent (`declare +r
  nope` still brings `nope` into being). On one that is, it reports
  `{tag}: {name}: readonly variable` with status 1 and abandons the
  operand *before* any other flag lands, so `readonly x; declare -i +r x`
  leaves `x` un-integer.
* **The refusal is judged against the attribute the name arrived with**,
  which is why `-r +r` on a fresh name cancels out silently while
  `readonly ro; declare -r +r ro` is refused. Placing the check just
  after the existing readonly-assignment guard (and so before the local
  shadow) also gets the in-function cases right for free: bash will not
  shadow a readonly name with a `local` either, and the diagnostic it
  emits there has the same `{tag}: {name}: readonly variable` shape.

Coverage: `userspace/oils/tests/corpus/declare-plus-flags.sh` (25
sections, matching bash) and the unit test
`the_off_direction_of_export_removes_and_of_readonly_refuses`.

Two neighbouring divergences found while measuring this are **not** part
of the fix and are tracked separately below:
TD-OILS-DECL-PLUS-A-DESTROYS-ARRAYS (`+a`/`+A`) and
TD-OILS-DECL-FLAG-OFF-LOSES-TO-ON (`-i +i`, `-n +n`, `-t +t`).
