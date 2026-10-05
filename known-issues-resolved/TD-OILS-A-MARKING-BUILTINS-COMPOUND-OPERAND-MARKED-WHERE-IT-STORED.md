### TD-OILS-A-MARKING-BUILTINS-COMPOUND-OPERAND-MARKED-WHERE-IT-STORED. `readonly g=(1 2)` through a local nameref put the array and the `-r` on one variable where bash parts them — 2026-08-11 — FIXED 2026-08-11

**Where:** `userspace/oils/src/interp.rs`,
[`Shell::exec_declare_with_arrays_scoped`]. Its phase 3 — the loop over
[`BoundCompound`] that applies `readonly`/`export`'s attribute — ran *inside*
the `-gG` scope swap the value had bound under, so both halves necessarily
landed in the same place.

bash's `expand_declaration_argument` (subst.c:12653) is three commands:

1. `make_internal_declare(word, opts, cmd)` — the `declare` builtin over the
   word **truncated at the `=`** (subst.c:12493),
2. `do_word_assignment` → `do_compound_assignment` (subst.c:3458) — the
   compound assignment proper, and
3. the same truncated word handed to the **real builtin**, after the whole
   word-expansion pass has finished.

`readonly` and `export` are not LOCALVAR_BUILTINs, so every assignment word of
theirs is stamped `W_ASSNGLOBAL|W_CHKLOCAL` (execute_cmd.c:4221) and steps 1–2
run as `declare -gG`. Step 3 carries no such stamp: it runs later, outside that
arrangement, and marks whatever the name is bound to *then*, followed through
its live reference chain. The two halves are free to disagree.

```sh
$ f() { local -n g=z; readonly g=(1 2); declare -p g z; }; f; echo AFTER; declare -p g z
bash: declare -n g="z" / declare -r z / AFTER / declare -a g=([0]="1" [1]="2") / declare -r z
osh : declare -nr g="z" / AFTER / declare -a g=([0]="1" [1]="2")
```

The value goes to the frame's own local if the reference reaches one (bash's
chklocal rule) and to the **global** otherwise — and a nameref is not a local
chklocal will accept, so it goes global. The `-r` goes to `z`, which is where
the live reference points.

**Fixed** in this commit by a new [`Shell::mark_bound_compounds`], called after
`std::mem::take(&mut self.declare_global_swap)` + [`Shell::leave_global_scope`]
so it runs outside the swap and resolves the truncated name the ordinary way.
`-a`/`-A` are deliberately not passed on: they shape an operand that carries a
value, and the truncated one carries none. The loop stays inside the builtin's
own `2>` — hoisting it into [`Shell::exec_declare_with_arrays`] would have meant
a second [`Shell::push_builtin_stderr`], whose `open_std_sink(…, append=false)`
would have truncated away the phase-1/2 diagnostics already written.

Corpus:
`a-marked-compound-operand-is-two-commands-that-need-not-agree-where-they-land.sh`.
Unit test:
`a_marked_compound_operand_is_two_commands_that_need_not_agree_where_they_land`.

**How it was found:** the 25-case differential sweep that landed
TD-OILS-A-COMPOUND-DECLARATION-WITH-NO-KIND-LETTER-DOES-NOT-REACH-FOR-AN-ARRAY
(`ok=23 bad=2`; these were the two), then narrowed by a 15-row and a 10-row
per-process matrix over compound and scalar operands.
