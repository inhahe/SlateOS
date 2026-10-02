### TD-OILS-AN-EMPTY-ARITHMETIC-SUBSCRIPT-IS-NAMED-(null)-BY-BASH. `set -u; $(( a[] ))` makes bash print a C null pointer as a variable name — NOT-A-BUG (bash defect, deliberately not replicated) — 2026-08-06

**Where:** `userspace/oils/src/arith.rs` — the `Expr::EmptySub` arm of
`eval_expr`, which is the one operand kind that does *not* call
`VarLookup::note_arith_unbound`.

**What.** Found while fixing TD-OILS-NOUNSET-SKIPS-ARITHMETIC. bash asks the
`set -u` question for an empty arithmetic subscript like it does for every other
operand, but by then it has already refused the subscript, and
`array_variable_name` hands back a **null pointer** for a token it refused. The
name is printed straight through, so what comes out is the literal text
`(null)`:

```text
$ ( set -u; echo "$(( a[] ))" )
bash: a[]: bad array subscript
bash: a[]: bad array subscript
bash: (null): unbound variable        ← and aborts
osh : a[]: bad array subscript
osh : a[]: bad array subscript
osh : 0
```

The two `bad array subscript` lines are bash's own doubling and osh already
matches them; it is only the third line that differs. It is the same whether the
array exists or not (`declare -a a=(1)` and an entirely unknown `nada` both
print `(null)`), which is itself the tell that no name was ever resolved.

**Why not replicated.** `(null)` is glibc's rendering of a null pointer passed
to `%s` — it is not a shell behaviour, it is undefined behaviour that happens to
be survivable on glibc, and reproducing it would mean writing a variable name
osh never had. osh evaluates the operand as 0 and carries on, which is what bash
does for this operand with nounset *off*, and is the answer every other empty
subscript already gets.

**Boundary.** The whole-set subscript beside it is *not* affected and is matched
exactly: `declare -a a; (( a[@] ))` is `a: unbound variable` alone (the nounset
failure replaces the refusal), while `declare -a a=(1); (( a[@] ))` is the
bad-subscript refusal alone. Only the *empty* subscript reaches bash's null.

**Impact.** With `set -u` on, an empty arithmetic subscript aborts bash and does
not abort osh. Reaching it requires writing `a[]` inside arithmetic, which is
already a refused subscript twice over.
