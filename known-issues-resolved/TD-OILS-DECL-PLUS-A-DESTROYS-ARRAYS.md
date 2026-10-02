### TD-OILS-DECL-PLUS-A-DESTROYS-ARRAYS. `declare +a` / `declare +A` make an array instead of refusing to unmake one — 2026-07-31 — ✅ RESOLVED 2026-07-31

**Where:** `userspace/oils/src/interp.rs` — `builtin_declare_scoped`'s
flag loop, whose `b'A' => assoc = true` and `b'a' => indexed = true` arms
still ignore the `enable` flag (the sibling `b'x'`/`b'r'` arms were fixed
under TD-OILS-DECL-PLUS-X-R-SETS-INSTEAD-OF-CLEARING); and the same two
arms in `exec_declare_with_arrays_scoped`'s flag loop.

bash cannot un-make an array either, so `+a`/`+A` is a refusal, not a
removal — but only when the letter matches the variable's *actual* kind.
osh instead reads the letter as its `-` form, which converts the variable
and can silently drop data:

```sh
declare +a q; declare -p q                       # bash declare -- q      osh declare -a q
declare -a q=(1 2); declare +a q; echo "rc=$?"   # bash declare: q: cannot destroy array
                                                 #      variables in this way, rc=1
                                                 # osh  (silent), rc=0
declare +A q5=(1 2); declare -p q5               # bash declare -a q5=([0]="1" [1]="2")
                                                 # osh  declare -A q5=([1]="2" )   <-- lost [0]
declare -A m=([k]=1); declare +a m; echo "rc=$?" # bash rc=0 (no-op: m is not indexed)
```

**Measured rule (bash 5.2.37).**

* `+a` refuses only when the name really is an *indexed* array, `+A` only
  when it is *associative*; on anything else (scalar, unset, the other
  kind) it is a no-op that leaves the declaration otherwise ordinary.
* The message is `{tag}: {name}: cannot destroy array variables in this
  way`, status 1, operand abandoned before any other flag lands
  (`declare -a q=(1 2); declare -i +a q` leaves `q` un-integer).
* The name it quotes is the **operand as written**, even when it was
  resolved through a nameref: `declare -a arr=(1 2); declare -n r=arr;
  declare +a r` says `r`, not `arr`. (The `+r` refusal quotes the
  resolved target instead — see TD-OILS-READONLY-REFUSAL-NAMES-TARGET.)
* It fires *after* the kind the same command establishes, so `declare -a
  +a fresh` is refused and leaves `declare -a fresh` behind, and after a
  *compound* literal has bound (`declare +a q=(1 2)` leaves the full
  array and then refuses) but *before* a scalar value is stored
  (`declare -a +a q=5` leaves `declare -a q=()`).
* It fires *before* the array-kind conflict check: `declare -a q=(1 2);
  declare -A +a q` says "cannot destroy", not "cannot convert".
* A readonly refusal outranks it: `declare -a q=(1 2); readonly q;
  declare +ar q` says `q: readonly variable`.
* Inside a function the local shadow comes first, so `declare -a q=(1 2);
  f() { declare +a q; }` is a no-op leaving a fresh `declare -- q`, while
  `f() { local -a q=(1 2); declare +a q; }` (same frame, already bound)
  and `f() { declare -g +a q; }` both refuse.

**Proper fix.** Add `unset_indexed`/`unset_assoc` beside the existing
`unset_export`/`unset_readonly`, give `b'a'`/`b'A'` the `enable` test in
both flag loops, and put the refusal in `builtin_declare_scoped` just
after `array_kind_apply` (which is after the local shadow and before the
kind-conflict check and the value store), quoting the pre-resolution
operand name. Mirror it in `exec_declare_with_arrays_scoped`'s phase 3
for compound operands. Probes: `target/dvscratch/px4.sh`,
`px5.sh`, `px8.sh`.

**✅ RESOLVED 2026-07-31**, exactly as above. Two details the fix had to
get right that the plan glossed over:

* **The kind goes on before the refusal.** `declare -a +a fresh` really
  does leave `declare -a fresh` behind, and `declare +a n[0]=5` leaves
  the subscripted operand's implicit `declare -a n=()`. So
  `array_kind_apply` runs first and the refusal follows it — but the
  *kind-conflict* check had to move to after both, since it would
  otherwise pre-empt the refusal it is outranked by. It is now computed
  up front (`kind_conflict`), used to gate the apply, and only reported
  once the refusal has had its say.
* **An operand that carried a value leaves the array valued.**
  `declare -a +a q=5` prints back `declare -a q=()`, not a bare
  `declare -a q` — the same shape a bad `-i` value leaves behind — so the
  refusal inserts into `array_valued` when `value.is_some()`.

Coverage: nine new sections in
`userspace/oils/tests/corpus/declare-plus-flags.sh` and the unit test
`the_off_direction_of_the_array_kinds_refuses_rather_than_converting`.
