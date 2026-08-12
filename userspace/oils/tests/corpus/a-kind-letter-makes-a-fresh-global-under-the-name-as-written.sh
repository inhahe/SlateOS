# A global-scope declaration does not always bind the name it was written with:
# where the global side of the lookup comes back empty, the name is transformed
# to a cell somewhere along the chain (see the corpus case
# a-global-scope-declaration-whose-global-chain-dies-binds-the-live-chains-last-reference).
# A **kind letter** — `-a` or `-A`, the two letters that choose a storage shape —
# is the one thing that stops it, and it stops it by never reaching the transform
# at all. `declare_internal`, having found nothing (declare.def:783-792):
#
#     if (var == 0)
#       {
#         if (creating_array)
#           {
#             if (flags_on & att_assoc) var = make_new_assoc_variable (name);
#             else                      var = make_new_array_variable (name);
#           }
#         else
#           var = bind_global_variable (name, (char *)NULL, ASS_FORCE);
#
# The `bind_global_variable` on the last line is where the transform lives — its
# `bind_variable_internal` (variables.c:3020) opens by resolving the global
# table's entry for the name. The two lines above it do not: they bind `name` as
# written, and they *replace* whatever the global table held rather than widening
# it, so the reference cell and the `-n` attribute that were there are simply
# gone. The elements then arrive on a variable that is no reference, and step 2
# of the decomposition has nothing left to follow either — one binding of the
# untransformed name serves the whole command.
#
# Which is why the same word with and without a kind letter lands in two
# different places, and why the letter's absence is not a smaller version of its
# presence but a different rule.
#
# The letter is read off the *word flags*, though, not off the command. For a
# compound operand it is step 1 that has the kind, and step 1's options are
# spelled by `expand_declaration_argument` out of what `fix_assignment_words`
# marked (subst.c:12662-12745) — where the scan reads a lowercase `g` only
# (execute_cmd.c:4246). `declare -Ga g=(1 2)` is therefore an ordinary *local*
# `declare -a` there and never binds a global name at all, while `declare -ga`
# and the standingly-global `readonly -a`/`export -a` do.

cyc='declare -n g=z; declare -n z=g;'

echo '=== a kind letter binds the name as written, where the plain spelling does not'
( eval "$cyc"; f() { local -n g=w; declare -ga g=(1 2); }; f; declare -p g z w )
( eval "$cyc"; f() { local -n g=w; declare -g g=(1 2); }; f; declare -p g z w )

echo '=== and the reference it replaces is gone, attribute and all'
( eval "$cyc"; f() { declare -ga g=(1 2); }; f; echo AFTER; declare -p g z )
( eval "$cyc"; f() { declare -gA g=([k]=v); }; f; echo AFTER; declare -p g z )

echo '=== the walks that came back empty are paid for all the same'
for s in 'declare -ga g=(1 2)' 'declare -gGa g=(1 2)' 'declare -gA g=([k]=v)' \
         'readonly -a g=(1 2)' 'readonly -A g=([k]=v)' 'export -a g=(1 2)' \
         'declare -ga g' 'declare -g g=(1 2)' 'readonly g=(1 2)'; do
  printf '%-22s %s\n' "$s" \
    "$(eval "( $cyc f() { $s; }; f )" 2>&1 | grep -c 'circular name reference')"
done

echo '=== a marking builtin needs no letter to be global, so its kind letter counts'
( eval "$cyc"; f() { local -n g=w; readonly -a g=(1 2); }; f; echo AFTER; declare -p g z w )
( eval "$cyc"; f() { local -n g=w; export -a g=(1 2); }; f; echo AFTER; declare -p g z w )
( eval "$cyc"; f() { local -n g=w; readonly g=(1 2); }; f; echo AFTER; declare -p g z w )

echo '=== the valueless spelling has a kind too, and takes the same road'
( eval "$cyc"; f() { local -n g=w; declare -ga g; }; f; echo AFTER; declare -p g z w )
( eval "$cyc"; f() { local -n g=w; declare -g g; }; f; echo AFTER; declare -p g z w )

echo '=== a chain that merely dies, rather than closing, reads the same'
( declare -n g=z
  f() { local -n g=w; declare -ga g=(1 2); }; f; echo AFTER; declare -p g z w )

echo '=== with a global to find, nothing is transformed and the letter is idle'
( g=old
  f() { local -n g=w; declare -ga g=(1 2); }; f; echo AFTER; declare -p g w )
( declare -n g=z; z=Z
  f() { local -n g=w; declare -ga g=(1 2); }; f; echo AFTER; declare -p g z w )

echo '=== no global binding at all is bound as written by either spelling'
( f() { local -n g=w; declare -ga g=(1 2); }; f; echo AFTER; declare -p g w )
( f() { local -n g=w; declare -g g=(1 2); }; f; echo AFTER; declare -p g w )
