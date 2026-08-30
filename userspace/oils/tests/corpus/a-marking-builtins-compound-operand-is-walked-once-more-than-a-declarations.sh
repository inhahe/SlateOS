# Every walk of a nameref chain that closes on itself warns once, so the number
# of `circular name reference` lines a compound operand produces *counts* the
# decomposition `expand_declaration_argument` (subst.c:12653) performs. It is a
# more exacting instrument than any single `declare -p`, because it sees the
# lookups that leave no trace behind them.
#
# `readonly` and `export` cost more walks than the declare family, for two
# separate reasons, and both are visible here.
#
#   * Their operands carry `W_CHKLOCAL` as well as `W_ASSNGLOBAL`
#     (execute_cmd.c:4221), so step 1 is `declare -gG`, not `declare -g`. `-G`
#     makes `declare_find_variable` (declare.def:149) ask `find_variable` — which
#     follows namerefs — before falling back to `find_global_variable`. That is
#     exactly one walk more than a plain `-g` pays.
#
#   * Step 2's chklocal branch in `do_compound_assignment` is guarded
#     `else if (mkglobal && variable_context)` (subst.c:3491). At top level
#     `variable_context` is 0, so the ordinary `assign_array_from_string` runs
#     and costs one walk; inside a function the branch is taken and costs two.
#     So these two builtins alone warn one *more* time inside a function than at
#     top level, where every other spelling is flat or cheaper.
#
# A kind letter collapses all of it: step 1 converts the name to an array
# (`make_local_array_variable`, arrayfunc.c:471) and overwrites the nameref's
# value cell with the `ARRAY *`, so from step 2 on there is no name left to
# follow. `-i` is not a kind letter — it transforms a value, it does not choose a
# storage shape — and pays the full price.
#
# A cycle built out of **frame-local** references measures differently again,
# and the difference is the third step. Over a *global* cycle steps 1–2 leave an
# array where the nameref's value cell was, so step 3 has no name left to follow.
# A frame-local reference is never overwritten — step 1's `-g` makes the array
# global, in front of it — so all three steps walk and the count is a flat 3
# (2 where the builtin will refuse one of its own option letters before it looks
# anything up).
#
# The counting is done rather than the lines printed, because the warning
# carries the shell's own name and the reprint of a `-r` array is not what is
# under test.

cyc='declare -n g=z; declare -n z=g;'

echo '=== at top level'
for s in 'readonly g=(1 2)' 'readonly -a g=(1 2)' 'readonly -A g=([k]=v)' 'readonly -i g=(1 2)' \
         'export g=(1 2)' 'export -a g=(1 2)' \
         'declare g=(1 2)' 'declare -a g=(1 2)' 'declare -g g=(1 2)' 'declare -ga g=(1 2)' \
         'typeset g=(1 2)' 'typeset -A g=([k]=v)'; do
  printf '%-22s %s\n' "$s" "$(eval "( $cyc $s )" 2>&1 | grep -c 'circular name reference')"
done

echo '=== inside a function, over the same global cycle'
for s in 'readonly g=(1 2)' 'readonly -a g=(1 2)' 'readonly -A g=([k]=v)' 'readonly -i g=(1 2)' \
         'export g=(1 2)' 'export -a g=(1 2)' \
         'declare -g g=(1 2)' 'declare -ga g=(1 2)'; do
  printf '%-22s %s\n' "$s" "$(eval "( $cyc f() { $s; }; f )" 2>&1 | grep -c 'circular name reference')"
done

echo '=== inside a function, over a cycle the frame itself holds'
loc='local -n g=z; local -n z=g;'
for s in 'readonly g=(1 2)' 'readonly -a g=(1 2)' 'readonly -A g=([k]=v)' 'readonly -i g=(1 2)' \
         'export g=(1 2)' 'export -a g=(1 2)' 'export -A g=([k]=v)' \
         'declare g=(1 2)' 'declare -a g=(1 2)' 'declare -g g=(1 2)' 'declare -ga g=(1 2)' \
         'local g=(1 2)' 'local -a g=(1 2)' 'typeset g=(1 2)'; do
  printf '%-22s %s\n' "$s" "$(eval "( f() { $loc $s; }; f )" 2>&1 | grep -c 'circular name reference')"
done

echo '=== and with more than one operand, the count is still the chains'
for s in 'readonly g=(1 2) h=(3)' 'readonly g=(1 2) s=5' 'export g=(1 2) h=(3)'; do
  printf '%-22s %s\n' "$s" "$(eval "( f() { $loc $s; }; f )" 2>&1 | grep -c 'circular name reference')"
done

echo '=== a scalar operand never takes this path at all'
for s in 'readonly g=1' 'export g=1' 'declare g=1' 'declare -g g=1'; do
  printf '%-22s %s %s\n' "$s" \
    "$(eval "( $cyc $s )" 2>&1 | grep -c 'circular name reference')" \
    "$(eval "( $cyc f() { $s; }; f )" 2>&1 | grep -c 'circular name reference')"
done

echo '=== the bare name, with nothing to assign, is cheaper still'
for s in 'readonly g' 'export g' 'declare -g g' 'readonly -a g' 'export -n g'; do
  printf '%-22s %s\n' "$s" "$(eval "( $cyc $s )" 2>&1 | grep -c 'circular name reference')"
done
