### TD-OILS-A-KIND-LETTER-WAS-GIVEN-THE-NAME-TRANSFORM-IT-IS-THE-ONE-THING-THAT-NEVER-REACHES. `declare -ga g=(1 2)` under a dead global chain landed the array where the scalar spelling puts it — 2026-08-11 — FIXED 2026-08-12

**Where:** `userspace/oils/src/interp.rs`, [`Shell::global_bind_names`],
[`Shell::enter_global_scope`], [`Shell::in_declare_global_scope`] and the new
`DeclScopeFlags` / `GlobalBind`. Where the global side of a global-scope
declaration's lookup comes back empty, the name is transformed to a cell along
the chain (the Fixed entry
TD-OILS-A-GLOBAL-SCOPE-DECLARATION-BOUND-THE-NAME-IT-WAS-WRITTEN-WITH-WHEN-ITS-GLOBAL-CHAIN-LED-NOWHERE).
A **kind letter** is the one thing that stops it, and it stops it by never
reaching it. Having found nothing, `declare_internal` makes the variable itself
(declare.def:783-792):

```c
if (var == 0)
  {
    if (creating_array)
      {
        if (flags_on & att_assoc) var = make_new_assoc_variable (name);
        else                      var = make_new_array_variable (name);
      }
    else
      var = bind_global_variable (name, (char *)NULL, ASS_FORCE);
```

The last line is where the transform lives — its `bind_variable_internal`
(variables.c:3020) opens by resolving the global table's entry for the name. The
two above it do not: they bind `name` as written, and they *replace* the global
table's entry rather than widening it, so the reference cell and its `-n` are
gone. The elements then arrive on a variable that is no reference, and step 2 of
the decomposition has nothing left to follow either — one binding of the
untransformed name serves the whole command.

**Reproduce.** The same shape one letter apart lands in two places:

```sh
( declare -n g=z; declare -n z=g
  f() { local -n g=w; declare -ga g=(1 2); }; f; declare -p g z w )
# bash: declare -a g=([0]="1" [1]="2")  …  w unset
# osh (before): the array was on w
```

and a marking builtin splits across both, since only step 1 has the kind:

```sh
( declare -n g=z; declare -n z=g
  f() { local -n g=w; readonly -a g=(1 2); }; f; declare -p g z w )
# bash: the array on g, the `-r` on w
```

**The fix.** Three parts.

*The letter.* [`Shell::global_bind_names`] gained a kind arm: where the
global-only road answered a reference the name is still the one its restart
arrived at (`restart_new_var_name`, declare.def:770), but where it answered
nothing — among globals, a cycle — the name is bound as written, and
[`Shell::enter_global_scope`] clears the binding first (`GlobalBind::fresh`) so
the array is a fresh variable rather than a widening of the reference that stood
there.

*The walks that came back empty are still paid for.* The destruction has to
happen after them, so `enter_global_scope` now walks each fresh name against the
live tables and warns `1 + chklocal` times before clearing it — the walks
`declare_find_variable` (declare.def:149) made to get there.

*The letters are not the command's.* For a compound operand the half that has
the kind is step 1, whose options `expand_declaration_argument` spells out of
the **word flags** (subst.c:12662-12745), not out of what was written: the scan
in `fix_assignment_words` reads a lowercase `g` only (execute_cmd.c:4246), and
`W_CHKLOCAL` is `readonly`/`export`'s standing flag (execute_cmd.c:4221) that no
letter sets. So `declare -Ga g=(1 2)` is a plain local `declare -a` there, and
`declare -gG` pays one walk, not two. `DeclScopeFlags` now carries the two
readings apart (`assn_global`, and a `chklocal` the expansion half overrides).

**Tests.** Unit test `a_kind_letter_makes_a_fresh_global_under_the_name_as_written`
and corpus `a-kind-letter-makes-a-fresh-global-under-the-name-as-written.sh`
(9 sections). The 240-shape probe matrix (`/tmp/kind_matrix.sh`) went 191 → 210
and the 27-shape one to 27/27; the remainder is
TD-OILS-A-NAMEREF-CHAIN-THAT-OUTRUNS-THE-LINK-LIMIT-IS-TAKEN-FOR-ONE-THAT-CLOSED.
