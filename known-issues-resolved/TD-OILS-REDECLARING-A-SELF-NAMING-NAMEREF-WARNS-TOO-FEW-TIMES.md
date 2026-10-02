### TD-OILS-REDECLARING-A-SELF-NAMING-NAMEREF-WARNS-TOO-FEW-TIMES. A function-local declaration walks the operand's nameref chain a fixed number of times — FIXED 2026-08-11

**Where:** `userspace/oils/src/interp.rs` — `Shell::builtin_declare_scoped`'s
scalar operand loop (the walk count and the hoisted lexical `-n` check) and
`Shell::declare_compounds_scoped`'s compound operand loop.

The symptom was narrow (a re-declared self-naming reference warned twice where
bash warns four times) but the rule behind it is not. A `declare`/`local` that
binds a **function-local** pays a *fixed* price for the operand's chain, and
pays it whatever the declaration asks for. `declare.def:601` puts the whole
branch behind `if (variable_context && mkglobal == 0)`, and inside it the chain
is walked twice before anything is bound:

```c
  newname = declare_transform_name (name, flags_on, flags_off);
  …
    var = make_local_variable ((flags_on & att_nameref) ? name : newname, inherit_flag);
```

`declare_transform_name` opens with `var = find_variable (name)`
(declare.def:212), to decide which name the declaration is really about, and
`make_local_variable` needs the same answer to know whether the `local` is a
no-op (variables.c:2607):

```c
  old_ref = find_variable_noref (name);
  …
  old_var = find_variable (name);
```

`find_variable_noref` does not follow and so is silent; the two `find_variable`s
each walk and each warn. Neither looks at the letters or at whether an array is
being made, so `local -n g`, `local +n g`, `local g[1]` and `local -a g=(1 2)`
all warn as often as the plain `local g`. A *valueless* `-n` takes a third from a
branch of its own (declare.def:618): `find_variable_last_nameref (name, 1)` makes
no cycle test and says nothing, so it falls through to
`else if ((var = find_variable (name)) == 0 …)` and then to
`make_local_variable`.

osh had been counting by the letters instead — two for a plain local, **one** for
an array-making one, and **none** for the `-n`/`+n` forms, which follow nothing.
The count is now `2`, or `3` for a valueless `-n`, and it is paid on the
`follow == false` side too (gated on the name actually carrying the nameref
attribute, since a name that is no reference can start no cycle). The compound
loop, which had been walking once, now walks twice the same way.

The reference need not be the frame's own: `find_variable` follows whichever
binding of the name is live, so a fresh `local g` shadowing a *global* cycle pays
exactly the same two. At global scope, and under `-g`, the branch is skipped and
none of it is paid; `declare -p` returns at declare.def:354 before the operand
loop ever runs.

**And the ordering, which is the other half of it.** The lexical `-n` checks sit
at declare.def:520, *above* the `restart_new_var_name` label and so above every
walk:

```c
  /* Do some lexical error checking on the LHS and RHS of the assignment
     that is specific to nameref variables. */
  if (flags_on & att_nameref)
    {
      …
      if (check_selfref (name, value, 0))
        …  builtin_warning (_("%s: circular name reference"), name);
      if (value && *value && (aflags & ASS_APPEND) == 0 && valid_nameref_value (value, 1) == 0)
        { builtin_error (…); NEXT_VARIABLE (); }
    }
```

osh made that check *after* the walk, which showed up twice: the
builtin-tagged self-reference warning came out behind the two walks
(`f() { local -n g=g; local -n g=g; }` read *walk, walk, warning, walk* instead
of *warning, walk, walk, walk*), and a refused value still paid for walks bash
never makes (`f() { declare -n g=z; declare -n z=g; declare -n g="a b"; }` warned
twice before saying `` `a b': invalid variable name for name reference ``, where
bash says only the error). The check is now hoisted above the walk. It judges the
operand as written, which is safe because `follow` is false for every `-n`
operand — nothing has been followed by then.

Every count below is measured against bash 5.2.37, not reasoned:

| form, inside a function, on a circular chain | walks |
|---|---|
| `local g`, `-i`, `-x`, `-r`, `-I`, `-a`, `-A`, `declare g`, `typeset g` | 2 |
| `local g[1]`, `local -a g=(1 2)`, `local -A g=([k]=v)` | 2 (was 1) |
| `local -n g=h`, `local +n g`, `local -rn g=h` | 2 (was 0) |
| `local -n g`, `local -nr g`, `local -In g`, `declare -n g` | 3 (was 0) |
| `declare -g g`, `local -g g`, `declare -p g` | 0 |
| a fresh local shadowing a *global* cycle | 2, or 3 for a valueless `-n` |
| `declare -n g="a b"` (value refused) | 0, and the error alone |

**Corpus:** `a-function-local-declaration-walks-a-nameref-chain-twice.sh`.
Unit test: `a_function_local_declaration_walks_a_nameref_chain_twice`.
