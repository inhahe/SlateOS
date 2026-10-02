### TD-OILS-A-COMPOUND-DECLARATION-WITH-NO-KIND-LETTER-DOES-NOT-REACH-FOR-AN-ARRAY. `local g=(1 2)` through a circular reference leaked the value to a global, and the walk counts were wrong on every route — FIXED 2026-08-11

**Where:** `userspace/oils/src/interp.rs`, `Shell::declare_compounds_scoped` —
the `follow`/`resolved` block, the `(None, None) if follow` arm of the target
match, and the array-kind site below it.

**What it was.** `f() { local -n g=z; local -n z=g; local g=(1 2); }` left the
frame's reference untouched at `declare -n g="z"` and created a **global**
`declare -a g=([0]="1" [1]="2")`; bash leaves `declare -an g=([0]="1" [1]="2")`
in the frame and creates nothing. The walk counts were wrong alongside it: local
no-kind 3 where bash walks 4, top-level no-kind 1 where bash walks 3,
`readonly`/`export` in a function 2 where bash walks 3.

**The measurement that explains it.** A compound operand of a declaration
builtin is **three commands**, not one — `expand_declaration_argument`
(subst.c:12655):

```c
  if (make_internal_declare (tlist->word->word, opts, cmd) != 0) …
  …
  t = do_word_assignment (tlist->word, 0);
  …
  tlist->word->word[t] = '\0';        /* the builtin sees the name alone */
```

1. `make_internal_declare` runs the builtin over the **name alone**, under a
   rebuilt option string carrying only the array kind (`-a`/`-A`), the scope
   (`-g`, `-G` for the chklocal builtins) and the value-transforming letters
   (`i`,`l`,`u`,`c`,`I`) — `--` when there is nothing to carry.
2. `do_word_assignment` → `do_assignment_internal` (subst.c:3545) →
   `do_compound_assignment` (subst.c:3459) performs the compound assignment.
3. the word is truncated and handed to the real builtin, which by then finds an
   array and follows nothing.

Inside a function step 1 is a `local`, so the array step 2 fills is the
**frame's**: the cycle's escape to global scope is never taken. (A *bare*
`g=(1 2)` through the same cycle does take it — the two paths genuinely differ,
and both were measured.) A kind letter costs step 2 nothing because step 1 has
already converted the name to an array (`make_local_array_variable`,
arrayfunc.c:471), overwriting the nameref's value cell with the `ARRAY *`, so
there is no name left to follow.

Each step walks the operand's chain and each walk warns once, which is what the
decomposition is measured by. Every count below is against bash 5.2.37:

| | step 1 | step 2 | total |
|---|---|---|---|
| local, `-a`/`-A` | 2 | 0 | **2** |
| local, no kind letter (incl. `-i`) | 2 | 2 | **4** |
| top level, `-a`/`-A` | 1 | 0 | **1** |
| top level, no kind letter | 2 | 1 | **3** |
| `readonly`/`export` in a function | 1 | 1 (+1 from step 3) | **3** |
| `-g` in a function | 0 | 0 | **0** |
| fresh local shadowing a *global* cycle | 2 | 0 | **2** |

`declare -a gq` at top level costs only one walk because
`find_variable_last_nameref` returns NULL on a cycle (it spins to `NAMEREF_MAX`,
variables.h:172) and is **silent**, so declare.def:731 finds no refvar and falls
through to `:774`. `declare -- gq` costs two: `declare_find_variable` at `:774`
plus the `bind_variable (name, NULL, ASS_FORCE)` that creates the name, which
`-a`/`-A` skip by calling `make_new_array_variable` instead.

**The fix.** The walk count is now derived from that decomposition rather than
from an ad-hoc formula — three branches, `escapes` (a chklocal builtin in a
frame, whose store walks the surviving chain itself), `make_local`, and top
level — and a local declaration whose chain led nowhere drops the escape and
binds the operand's own name, as `make_local_array_variable` does. The reference
keeps its attribute and loses the name it held: `self.vars.remove(&target)`
before `array_kind_apply`, so the held name cannot come back as element 0 of an
**append** (`local g+=(1 2)`, where bash gives `[0]="1" [1]="2"`). A chain longer
than the walk limit now keeps the reference attribute too (`declare -an n1=(…)`),
where osh used to strip it.

**Not replicated on purpose.** bash *reading* a `declare -an` array-with-nameref
(`${g[0]}`, `g[0]=9`) returns garbage out of the `ARRAY *` value cell — an empty
string, then `` `': not a valid identifier ``. osh reads the array sensibly.

**Corpus:**
`a-compound-declaration-binds-the-local-a-nameref-cycle-cannot-reach.sh`. Unit
test: `a_compound_declaration_binds_the_local_a_nameref_cycle_cannot_reach`,
beside `a_function_local_declaration_walks_a_nameref_chain_twice`.
