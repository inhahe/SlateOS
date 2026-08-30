# Whether a declaration's `NAME=value` operand is stored *as an array element*
# is not a question about the command's letters. bash asks the variable
# (declare.def:885):
#
#	  var_exists = var != 0;
#	  array_exists = var && (array_p (var) || assoc_p (var));
#	  creating_array = flags_on & (att_array|att_assoc);
#	  …
#	  else if ((making_array_special || creating_array || array_exists) && offset)
#
# — so `-a`/`-A` answers it, and so does a name that is *already* an array with
# no letter at all. Which of the two element stores is then made is decided the
# same way: declare.def:972 is `if (assoc_p (var))`, the variable again.
#
# What hangs on it is the parenthesised value, which this branch re-parses as a
# compound array literal where the scalar road would store the parentheses
# whole. No deprecation warning goes with it: declare.def:899 prints that only
# `if (array_exists == 0 && creating_array == 0)`, which is exactly the case
# this is not.
exec 2>&1
echo "--- a parenthesised value on a name already an array is a compound literal"
( declare -a g; declare g='(1 2)'; declare -p g )
( declare -a g=(7 8); declare g='(1 2)'; declare -p g )
( declare -A g; declare g='(1 2)'; declare -p g )
( declare -A g=([k]=1); declare g='(1 2)'; declare -p g )
( declare -a g; typeset g='(1 2)'; declare -p g )
( declare -a g=(7 8); declare g+='(1 2)'; declare -p g )
echo "--- and the letter says the same thing, on a name that has no kind yet"
( declare -a g='(1 2)'; declare -p g )
( declare -A g='([k]=1 [j]=2)'; declare -p g )
( declare g='(1 2)'; declare -p g )
echo "--- while an unparenthesised value is element zero, letter or no letter"
( declare -a g=(7 8); declare g=9; declare -p g )
( declare -a g=(7 8); declare g+=9; declare -p g )
( declare -A g=([k]=1); declare g=9; declare -p g )
( declare -A g=([0]=1); declare g+=9; declare -p g )
( declare -ai g=(7 8); declare g=4+4; declare -p g )
( declare -au g=(7 8); declare g=ab; declare -p g )
echo "--- a scalar is untouched by any of it"
( g=5; declare g='(1 2)'; declare -p g )
( g=5; declare g=9; declare -p g )
( declare -i g=5; declare g=4+4; declare -p g )
echo "--- and a local array answers about itself, not about the global"
( declare -a g=(7 8); t1() { local g=5; declare g='(1 2)'; declare -p g; }; t1; echo AFTER; declare -p g )
( g=5; t2() { local -a g=(7 8); declare g='(1 2)'; declare -p g; }; t2; echo AFTER; declare -p g )
echo TAIL
