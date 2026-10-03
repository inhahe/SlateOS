### TD-OILS-AN-UPPERCASE-G-WAS-READ-BY-THE-HALF-OF-THE-COMMAND-THAT-ONLY-EVER-READS-THE-LOWERCASE-ONE. `declare -Ga g=(1 2)` bound the array globally where bash keeps it in the frame — 2026-08-12 — FIXED 2026-08-12

**Where:** `userspace/oils/src/interp.rs`, [`Shell::in_declare_global_scope`].
`-G` means two things — bind globally, and unless the frame's own binding
answers — and osh gave both of them to *both* halves of a compound
declaration. bash gives them to neither half but the last.

The `declare` a compound operand decomposes into first spells its option string
out of the **word flags** `fix_assignment_words` put on the operand
(`expand_declaration_argument`, subst.c:12662-12745), never out of the letters
as written, and that scan is the narrower of the two readings:

* `W_ASSNGLOBAL` is taken from a **lowercase `g`** and nothing else
  (execute_cmd.c:4246), so an uppercase `-G` leaves it clear;
* `W_CHKLOCAL` is set at one place only (execute_cmd.c:4221), for an assignment
  builtin that makes no locals — `readonly` and `export`. No letter reaches it.

So `declare -Ga g=(1 2)` decomposes into a plain **local** `declare -a g`, the
literal, and only then the real `declare -Ga g`; the two ends of it land in
different scopes. The same command's *scalar* operand has no such first step —
it is handed to the builtin whole and takes the `-G` it was written with.

**Reproduce.** One command, two scopes:

```sh
( g=old; f() { declare -Ga g=(1 2); declare -p g; }; f; declare -p g )
# bash: declare -a g=([0]="1" [1]="2")   then   declare -- g="old"
# osh (before): the global was overwritten
( g=old; f() { declare -Ga g=1;      declare -p g; }; f; declare -p g )
# bash: the global *is* the array, both times
```

and the walk count says the same thing — `-Ga` warns twice about a global
cycle (the builtin's restart, then `chklocal`'s `find_variable`,
declare.def:149) while `-gGa` warns once and leaves exactly what `-ga` leaves,
step 1 having made the array itself:

```sh
( declare -n g=z; declare -n z=g; f() { declare -Ga  g=(1 2); }; f; declare -p g z )
( declare -n g=z; declare -n z=g; f() { declare -gGa g=(1 2); }; f; declare -p g z )
```

**The fix.** The expansion half's override in `in_declare_global_scope` already
cleared `chklocal` for anything but `readonly`/`export`; it now clears `global`
the same way, from the `assn_global` reading (`W_ASSNGLOBAL`) rather than the
builtin's `-g`-or-`-G`. Nothing else was needed: the builtin half keeps both
letters, so the restart machinery added with
TD-OILS-THE-FRAMES-OWN-BINDING-WAS-ASKED-ABOUT-BEFORE-THE-RESTART-RATHER-THAN-AFTER-IT
still leaves its empty global behind, in the shape the *builtin's* kind letter
named rather than the literal's — `declare -G g=(1 2)` leaves a scalar `z`
though the value was an array.

**Tests.** Unit test `a_lone_g_letter_reaches_the_builtin_alone_and_leaves_the_literal_local`
and corpus `a-lone-G-is-the-builtins-alone-and-reaches-neither-the-literal-nor-its-scope.sh`
(8 sections). The 240-shape probe matrix (`/tmp/kind_matrix.sh`) went 224 → 233;
the remaining 7 are two rows of the step-3-over-a-compound-operand gap below,
four of
TD-OILS-THE-TAG-ON-A-COMPOUND-DECLARATIONS-DIAGNOSTIC-IS-THE-PREVIOUS-COMMANDS-NAME
and one of
TD-OILS-A-COMPOUND-LITERAL-REFUSES-A-KIND-CHANGE-THE-BUILTIN-BEFORE-IT-HAS-ALREADY-MADE.
