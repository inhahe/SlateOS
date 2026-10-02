### TD-OILS-THE-SHAPE-AND-FOLD-LETTERS-ARE-APPLIED-BY-THE-LITERALS-HALF-ONLY-AND-NOT-BY-THE-BUILTIN-AFTER-IT. `declare -gGa g=(1 2)` left the frame's own `g` a scalar — 2026-08-12 — ✅ FIXED 2026-08-14

**Where:** `userspace/oils/src/interp.rs`,
[`Shell::declare_compounds_scoped`] (which reads `-a`/`-A`/`-i`/`-l`/`-u`/`-c`/`-I`
and applies them, ~line 47655) and [`Shell::apply_bound_compound`] (~line 44811,
which applies only `-r`/`-x`/`-t`/`-n`).

A compound operand is three commands, and the **first** and the **third** both
apply the command's shape and fold letters — to whatever variable each one's own
lookup found. osh splits them: phase 1 applies the shape/fold letters, phase 3
applies the marks. Where the two halves reach the same variable — which is
nearly always — the split is invisible, because phase 3's marks are set on a
*name* after the swap is undone and so land live. It shows only where the halves
part, which `-gG` is built to do: the lowercase `g` sends the literal out to the
global while the uppercase `G` lets `chklocal` keep the builtin at home.

```sh
( f() { local g=5; declare -gGa g=(1 2); declare -p g; }; f; declare -p g )
# bash: declare -a g=([0]="5")   then   declare -a g=([0]="1" [1]="2")
# osh : declare -- g="5"         then   declare -a g=([0]="1" [1]="2")
```

The same for `-A`, `-i`, `-l` and `-u`; `-r`, `-x` and `-t` already agree.
Affected: 2 rows of the 240-shape kind-letter matrix (`/tmp/kind_matrix.sh`,
233/240).

**Proper fix — and it is structural.** bash's step 3 is a *real* command,
`declare -gGa g`, run over the operand truncated at the `=`; it goes through the
one operand loop with no value, and every letter lands wherever that loop's own
lookup goes. `readonly` and `export` already work this way in osh — their half
is handed `words.spliced`, which carries each compound operand back as a bare
name, so `builtin_readonly`/`builtin_export` run the genuine loop over it. Only
the `declare` family does not: [`Shell::builtin_declare_scoped`] is handed
`argv[1..]`, which the compound operands were lifted *out* of, and meets them
instead as `DeclOperand::Bound` — a reduced path that replays four marks and
nothing else.

The fix is to close that asymmetry: let the `declare` family's operand loop see
the bare name too, keeping `BoundCompound` only for what genuinely happened
earlier (the phase-1 local refusal, the nameref refusal, and the fact that the
value is already bound). Two things then fall out rather than being coded:

* the shape and fold letters land in both halves, as here;
* [`Shell::make_empty_global`] disappears. It exists only because step 3 does
  not run: the empty `declare -a z` a dead chain leaves behind is nothing but
  what `declare -a z` over an unset name does, hand-rolled.

Step 3 sets the letters *as attributes only* — it neither re-folds nor
re-evaluates what the variable already held, and converts the shape only when a
kind letter was given, carrying the old scalar in as element 0:

```sh
( f() { local g=Ab7+1; declare -gGl g=(1 2); declare -p g; }; f )
# declare -l g="Ab7+1"           — the fold is on, the value untouched
( f() { local g=Ab7+1; declare -gGA g=(1 2); declare -p g; }; f )
# declare -A g=([0]="Ab7+1" )    — converted, the scalar carried over
```

The cheap version — giving `BoundCompoundFlags` the shape and fold letters and
having `apply_bound_compound` set them — would close these two rows, but it
would be a fifth mark bolted to the reduced path and would leave
`make_empty_global` standing. Do the structural one.

**Fixed 2026-08-14** — the structural one, as prescribed. Four changes:

* [`Shell::builtin_declare_scoped`]'s operand loop takes a `DeclOperand::Bound`
  as the **bare name** rather than as four replayed marks: `name_val` is the
  operand truncated at the `=`, so it carries no value, `value` is `None` of its
  own accord, and the whole assignment arm is skipped without a flag to say so.
  `BoundCompound` keeps only what genuinely happened earlier — the phase-1 local
  refusal, the nameref refusal — and lost its `target` field, the loop now
  resolving the name for itself.
* [`Shell::apply_bound_compound`] and `BoundCompoundFlags` are gone (90 lines).
* [`Shell::in_declare_global_scope`] sorts a compound operand into `names`
  rather than `compound` for the `declare` family's builtin half: there it is
  bash's step 3, an ordinary bare-name operand of `declare_internal`, and
  resolves as step 1 did. `export`/`readonly` are excepted — their third command
  is their own operand loop, not `declare_internal`.
* [`Shell::enter_global_scope`] asks `chklocal` of the name the **restart**
  arrived at (`nameref_cell (refvar)`, i.e. the end of
  [`Shell::global_chain_path`]) rather than of the name as written, which is
  what declare.def:735-774 does and what makes `declare -gGa g=(1 2)` under
  `declare -n g=z` leave the frame's own `g` alone. And
  [`Shell::global_bind_names`] now makes **no swap** where the global walk and
  the live walk land on the same variable through a global reference:
  `find_global_variable` reads only its first link from the global table, so a
  chain that re-enters a name a local holds runs *through* that local, and
  un-shadowing it would splice the local out of the middle and leave a cycle
  that warns twice where bash warns not at all.

`make_empty_global` did **not** disappear: it is still what step 1 leaves behind
for `export`/`readonly`, whose third command makes no such variable. For the
`declare` family it is now unreachable, the swap having put step 3 on the name
itself.

Verified against bash 5.2.37: the two matrix rows above, the `-i`/`-l`/`-u`
rows, and the whole 642-case corpus (`scripts/osh-bash-diff.py`, 642 matched, 0
failed). Tests:
[`interp::tests::the_letters_of_a_compound_declaration_land_a_third_time_where_the_builtin_resolves`];
corpus
`the-letters-of-a-compound-declaration-land-a-third-time-where-the-builtin-resolves.sh`.

**How it was found:** the 240-shape kind-letter matrix, while fixing
TD-OILS-AN-UPPERCASE-G-WAS-READ-BY-THE-HALF-OF-THE-COMMAND-THAT-ONLY-EVER-READS-THE-LOWERCASE-ONE.
