### TD-OILS-A-SUBSCRIPT-THAT-WILL-NOT-EVALUATE-NAMES-NOBODY-AND-STORES-NOWHERE. `read -r 'n[2+]'` said `read: 2+: syntax error` and then wrote element 0 — 2026-08-05 — ✅ **FIXED 2026-08-05**

**Where:** `userspace/oils/src/interp.rs` — `Shell::eval_arith_index_text_checked`
(which now clears the tag and delegates to the new
`Shell::eval_arith_expr_checked`), and `Shell::assign_elem`'s indexed arm.

**What.** Two independent things, both on the same two builtins. Measured with
`n=(a b c)`:

```text
                       bash                         osh (before)
printf -v 'n[1+]' X    `1+: syntax error: …`       `printf: 1+: syntax error: …`
read -r 'n[2+]' <<<Y   `2+: syntax error: …`       `read: 2+: syntax error: …`
                       n unchanged                  n[0] overwritten
n[1+]=W                `1+: syntax error: …`       the same — this half agreed
```

1. The diagnostic was **tagged with the builtin**. bash never blames a
   *subscript* on the command that asked for it: `declare 'n[4+]=v'`,
   `let 'n[7+]=1'`, `(( n[8+] = 1 ))`, `unset 'n[9+]'` and `printf -v 'n[1+]'`
   all report `1+: syntax error …` bare, while a bad `-i` **value** in the very
   same builtin still is tagged (`declare: 5+: syntax error …`).
2. The store still **landed on element 0**, because `eval_arith_index`
   fabricates a 0 for an expression that would not evaluate.

**Fixed 2026-08-05.** bash clears `this_command_name` in exactly one place —
`array_expand_index`, around the evaluation — and osh now does the same, in
`eval_arith_index_text_checked` rather than at each call site. Three ad-hoc
save/restore pairs (the `name[i]=v` assignment, the name-operand target and
`unset`) went away with it, and the fourth site that never had one — the
`printf -v`/`read` operand path — is covered by construction. The one caller
that wants a tag of its own, a `${param:off:len}` bound (`${v:1 z}` says
`v: 1 z: syntax error in expression`), calls the untagged
`eval_arith_expr_checked` underneath.

`assign_elem`'s indexed arm now takes the *checked* index and returns `false`
on `None`, so a subscript that named nowhere stores nowhere.

**Corpus:** `a-subscript-that-will-not-evaluate-names-nobody-and-stores-nowhere.sh`.
