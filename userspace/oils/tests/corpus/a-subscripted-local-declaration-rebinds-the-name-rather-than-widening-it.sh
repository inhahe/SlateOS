# A subscript asks a declaration for an array as plainly as `-a` does, and
# inside a function that array is made by `make_local_array_variable`, which
# **rebinds** the name rather than widening the variable in place. So the
# scalar the frame already held is *lost*, not carried in as element 0:
#
#	  f() { local g=5; local g[1]=9; declare -p g; }   # declare -a g=([1]="9")
#
# declare.def:614 sends both spellings down the one call —
#
#	  else if ((flags_on & att_array) || making_array_special)
#	    var = make_local_array_variable (newname, MKLOC_ASSOCOK|inherit_flag);
#
# — and `making_array_special` is set for any subscripted operand, valued or
# not. At global scope the widening is `find_or_make_array_variable` instead,
# which converts the variable and keeps its value as element 0; and a scalar
# bound at an *outer* context is merely shadowed, so it survives the return.
#
# `declare` and `typeset` bind locals inside a function too, so they answer the
# same way; `-g` takes the operand off the local branch altogether.
exec 2>&1
echo "--- the same context's scalar is lost, whether or not a value comes with it"
t1() { local g=5; local g[1]=9; declare -p g; }; ( t1 )
t2() { local g=5; local g[1]; declare -p g; }; ( t2 )
t3() { local g=5; declare g[1]=9; declare -p g; }; ( t3 )
t4() { local g=5; typeset g[1]=9; declare -p g; }; ( t4 )
echo "--- as it is for the letter that asks outright"
t5() { local g=5; local -a g; declare -p g; }; ( t5 )
t6() { local g=5; local -a g[1]=9; declare -p g; }; ( t6 )
t7() { local g=5; local -A g[k]=9; declare -p g; }; ( t7 )
echo "--- a scalar at an outer context is only shadowed, and survives"
g=5
t8() { local g[1]=9; declare -p g; }; ( t8; echo AFTER; declare -p g )
t9() { local -a g; declare -p g; }; ( t9; echo AFTER; declare -p g )
echo "--- and at global scope the widening keeps the value as element 0"
( gg=5; declare gg[1]=9; declare -p gg )
( gg=5; gg[1]=9; declare -p gg )
( gg=5; declare -a gg; declare -p gg )
echo "--- an array already of the asked-for kind keeps every element"
ta() { local -A g=([k]=v); local g[j]=9; declare -p g; }; ( ta )
tb() { local -a g=(1 2); local g[5]=9; declare -p g; }; ( tb )
echo "--- an unvalued name has nothing to lose either way"
tc() { local g; local g[1]=9; declare -p g; }; ( tc )
td() { local g[1]=9; declare -p g; }; ( td )
echo TAIL
