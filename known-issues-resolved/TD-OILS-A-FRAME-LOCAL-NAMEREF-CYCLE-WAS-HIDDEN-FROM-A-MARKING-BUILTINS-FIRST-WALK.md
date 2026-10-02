### TD-OILS-A-FRAME-LOCAL-NAMEREF-CYCLE-WAS-HIDDEN-FROM-A-MARKING-BUILTINS-FIRST-WALK. `readonly g=(1 2)` under a *local* nameref cycle warned once where bash warns three times — 2026-08-11 — FIXED 2026-08-11

**Where:** `userspace/oils/src/interp.rs`, [`Shell::in_declare_global_scope`].
For a compound operand of `readonly`/`export` osh entered the `-gG` swap
*before* the operand's chain was walked, so the frame's live nameref had already
been swapped out of the table and the cycle left no trace. bash's
`declare_find_variable` (declare.def:149) and `do_compound_assignment`
(subst.c:3491) do their `find_variable` against the tables as they stand, cycle
and all, and each walk warns.

**Reproduce.** Only a cycle built out of *frame-local* references was affected;
a global one has nothing for the swap to hide.

```sh
f() { local -n g=z; local -n z=g; readonly g=(1 2); }; f
# bash: three `circular name reference` warnings   osh (before): one
```

**The fix.** [`Shell::in_declare_global_scope`] now records each operand's walk
against the live tables *before* [`Shell::enter_global_scope`] hides the frame,
and warns twice from that recorded walk — the pair the swap is entered for
(steps 1 and 2 of `expand_declaration_argument`'s decomposition, subst.c:12653).
The third command, the builtin proper, runs after
[`Shell::leave_global_scope`] and finds the reference back where it was, so it
walks for itself and is owed nothing. That split is what the new
`expansion_half: bool` parameter carries: only the word-expansion half pays.

Where the value and the attribute land was already right — see the Fixed entry
TD-OILS-A-MARKING-BUILTINS-COMPOUND-OPERAND-MARKED-WHERE-IT-STORED — and so was
the exit status; only the *count* differed.

**Tests.** The unit test
`a_marking_builtins_compound_operand_is_walked_once_more_than_a_declarations`
gained a frame-local-cycle block (11 rows), and the corpus case of the same name
gained two sections. The 54-row circular-warning matrix (`/tmp/c7.sh`) that
found it now scores 54/54.
