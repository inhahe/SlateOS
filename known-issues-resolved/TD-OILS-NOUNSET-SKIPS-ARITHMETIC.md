### TD-OILS-NOUNSET-SKIPS-ARITHMETIC. `set -u` never reaches arithmetic: `(( nope ))` reads 0 in osh where bash calls it an unbound variable and aborts — 2026-08-06 — ✅ FIXED 2026-08-06

**Where:** `userspace/oils/src/arith.rs` — `VarLookup::note_arith_unbound` and
its call sites in `eval_expr` / `note_lv_unbound`; `userspace/oils/src/interp.rs`
— `Shell::arith_unbound_name`, `Shell::arith_var_visible`, and the `VarLookup
for Shell` impl.

**What.** Also found while fixing TD-OILS-INDIRECT-WHOLE-SET-NOUNSET. Under
`set -u` bash treats a name read by the arithmetic evaluator exactly like any
other parameter read: unset is an error, it is reported, and a non-interactive
shell aborts. osh silently substitutes 0 in every arithmetic context, so the
diagnostic never appears and the shell keeps going.

```text
$ ( set -u; (( nope + 0 )); echo tail; echo "rc=$?" )
bash 5.2.37:  nope: unbound variable          osh:  tail
              (aborts, no `tail`)                   rc=0

$ ( set -u; echo "$(( nope + 0 ))" )        bash: unbound variable   osh: 0
$ ( set -u; declare -a a; echo "${a[nope]}" ) bash: unbound variable  osh: (empty)
```

Note that an *unset* name and a name holding the empty string are different
here, as everywhere else: bash's arithmetic reads an empty-but-set variable as 0
without complaint, so the check must be on set-ness, not on the text.

**Fixed.** A new `VarLookup::note_arith_unbound` hook is asked once per operand
the evaluator actually *reaches*, and its `Err` abandons the expression where it
stands — which is bash's `longjmp` out of `expr_streval` (expr.c:1180), and is
why only the first unset name of `(( nope1 + nope2 ))` is reported and
`(( nope / 0 ))` never reaches the division.

The entry above got the shape right but the *question* wrong, and that was the
whole difficulty. Arithmetic does **not** ask the word expander's question.
It reads a name as a *variable*, so what matters is whether the variable exists
and is visible — bash's `find_variable (tok) && invisible_p (tok) == 0` — not
whether the thing addressed inside it has a value. The two answers differ in
both directions, which is why sharing the word expander's check would have been
wrong:

```text
declare -a a=();  "${a[0]}" → unbound error   $(( a[0] )) → silent 0
declare -a a;     "${a[0]}" → unbound error   $(( a[0] )) → unbound error
declare v;        "$v"      → unbound error   $(( v ))    → unbound error
v=;               "$v"      → empty           $(( v ))    → silent 0
```

A bare `declare -a a` declares without assigning, so bash holds it *invisible*
and arithmetic calls it unset; `a=()` assigned nothing but is visible, so it is
0. `Shell::arith_var_visible` is that per-name question, and is deliberately
neither `var_is_set` (which is `-v`, and reads a bare array name as element 0)
nor `ref_name_exists` (which never asks about visibility).

Three orderings had to be measured rather than guessed:

* The check runs **before** the operand's subscript is evaluated, so
  `(( nada[nope] ))` names `nada`, not `nope`.
* A subscripted operand is read through `array_variable_part`, so its failure is
  named after the *written* base however far a nameref would have reached:
  `declare -n r=nada; (( r[0] ))` reports `r` where `(( r ))` beside it reports
  `nada`.
* A read-modify-write reads and a plain assignment does not, and they disagree
  about which name they blame: `(( nada[nope] += 1 ))` reports the missing array
  `nada`, while `(( nada[nope] = 1 ))` reaches the subscript first and reports
  `nope`. Hence `note_lv_unbound`, called before the lvalue is resolved.

The one place osh deliberately does *not* follow is an empty subscript — see
TD-OILS-AN-EMPTY-ARITHMETIC-SUBSCRIPT-IS-NAMED-(null)-BY-BASH.

`tests/corpus/arithmetic-asks-set-u-about-the-variable-not-about-the-value-it-holds.sh`
covers all of it, including that the shell's own dynamic names (`RANDOM`,
`SECONDS`, `LINENO`, `BASH_ALIASES`, …) are variables like any other even though
they have no table entry until something assigns to one.
