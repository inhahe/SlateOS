### TD-OILS-DECL-ERROR-SKIPS-FLAG-APPLY. A per-name `declare` error abandons that name's flag application, but only *after* the compound literal has bound — 2026-07-31 — ✅ RESOLVED 2026-07-31

**Where:** `userspace/oils/src/interp.rs` —
`exec_declare_with_arrays_scoped`. bash's `declare_internal` performs a
compound assignment first (under the flags it collected), then runs a
series of per-name checks; several of them end in `NEXT_VARIABLE()`,
which skips the rest of the loop body for that name — including the
`VSETATTR`/`VUNSETATTR` that would have applied `flags_off`. The value is
already stored, so the name is left holding the attributes the
*assignment* gave it and none of the removals.

Three separate checks behave this way, and they were badly understated
when this entry was written: for two of the three osh emitted **no error
at all**, so the status was wrong too, not just the attribute letters.

```sh
declare +a  -l +l k8=(AB)  # bash declare -al k8=([0]="ab")  osh ditto                     ✅ DONE
declare -aA +l    v9=(AB)  # bash declare -Al v9=([AB]="")   osh ditto                     ✅ DONE
declare -n  +i    t1=(2+3) # bash declare -a  t1=([0]="2+3") osh ditto                     ✅ DONE
```

**✅ `-a` and `-A` together — done 2026-07-31.** Naming both kinds in the
`-` direction is a conflict with the command itself, and a *different*
refusal from the conversion one: the `-A` wins, the name becomes
associative and the literal binds as one, and only then does the builtin
refuse the `-a` against the array it just made. So the diagnostic carries
the builtin's tag (unlike a conversion failure, which comes from the
word-expansion pass untagged), is emitted once per operand, and fails the
command without discarding the rest of the parse unit. osh reported
nothing at all and exited 0.

Measured with `target/dvscratch/px35.sh`, `px37`–`px41`; the model that
came out of it:

* `-A` outranks a `-a` in the same command everywhere — including in
  `array_kind_conflict`, so `declare -A q=([k]=v); declare -aA q=(z)`
  binds and then reports, where a plain `declare -a q=(z)` cannot bind at
  all and discards the line.
* The refusal abandons the operand where the builtin would have applied
  the rest of its flags, so `declare -aA +i q=(2+3)` **keeps** the
  integer attribute and `declare -aA +u q=(Ab)` keeps the fold.
* A bare, subscripted or scalar operand binds *after* the check, so its
  value never lands — but one that carried a value still leaves the array
  valued (`declare -aA q[0]=z` → `declare -A q=()`).
* `+a`/`+A` outrank it ("cannot destroy array variables in this way").
* Only the `declare` family checks: `readonly -aA q=(1)` and
  `export -aA q=(1)` succeed and apply their attribute, and `-p`
  short-circuits into print mode first.

Covered by `naming_both_array_kinds_binds_as_associative_and_then_refuses`
and `userspace/oils/tests/corpus/declare-array-kind-conflict.sh`.

**✅ `+a`/`+A` on the array the same command makes — done 2026-07-31.**
Only the removals differed: `declare +a -l +l k8=(AB)` leaves `declare -al`
in bash and left `declare -a` in osh, and `declare +a -i +i k=(2+3)` the
same for the integer attribute. The destroy refusal in phase 3 abandons the
operand exactly as the two above do, so it now feeds the same
`operand_refused` flag — computed *after* `apply_assignment`, since whether
it fires is only knowable once the literal has bound and made the name an
array of that kind. `+a` against an associative name (or `+A` against an
indexed one) refuses nothing, so there the removals still run.

Measured with `target/dvscratch/px56.sh`; covered by the new section of
`userspace/oils/tests/corpus/declare-plus-flags.sh` and by
`the_off_direction_of_the_array_kinds_refuses_rather_than_converting`.

**✅ `-n` with a compound literal — done 2026-07-31.** bash refuses with
`{tag}: NAME: reference variable cannot be an array`, rc 1; the literal
still binds (under whatever kind the letters name), and the operand is
abandoned exactly as above — neither `n` nor `-r`/`-t`/`-x` lands, and
the case/integer removals are skipped too, so `declare -n -i +i q=(2+3)`
keeps `-i` and `declare -n -l +l q=(AB)` keeps `-l`. osh emitted no error
and applied `-n` along with everything else.

Measured with `target/dvscratch/px42.sh` and `px45.sh`; the ordering that
came out of it:

* It outranks **both** later refusals — the destroy one
  (`declare -n +a q=(3)` reports the reference message, and the `+a` does
  not fire) and the array-kind self-conflict, in either order
  (`declare -aA -n q=(1)` and `declare -n -aA q=(1)` both report it, with
  the `-A` still winning the kind).
* It does **not** outrank the phase-A refusals (conversion, readonly),
  which stop the literal from binding at all and discard the parse unit.
* Only a *compound* operand asks for it: a scalar operand of the same
  command still becomes a reference (`declare -n q=(1) r=w` refuses `q`
  and leaves `declare -n r="w"`), and bare/subscripted operands are
  untouched.
* A name that is **already** a reference is not being made one — the
  literal binds through it into the target, which bash accepts silently
  (`declare -A t=([k]=v); declare -n r=t; declare -n r=(z)` → rc 0).
* `-n +n` still asks for it; `+n` alone does not. `-p` prints instead of
  refusing, and `readonly`/`export` do not read `-n` as the nameref
  letter at all (see the `-n` fix committed alongside).

Covered by `a_compound_operand_under_n_binds_and_then_refuses_the_reference`
and `userspace/oils/tests/corpus/declare-nameref-array-refusal.sh`.

**Low priority:** every shape here is one bash prints an error for, so a
script that hits it is already broken.
