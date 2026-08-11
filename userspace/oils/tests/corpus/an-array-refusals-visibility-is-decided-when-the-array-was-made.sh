# Whether `declare -p` prints an array as `=()` or bare is decided **once**,
# when the variable is made — a refusal raised against it afterwards never
# changes the answer. `declare -p` prints the `=()` only for a variable that is
# not `att_invisible`, and the only place a declaration sets that flag is the
# global creation branch, declare.def:802:
#
#	  if (var == 0) { … var = mkglobal ? … : make_new_variable (…);
#			  if (offset == 0) VSETATTR (var, att_invisible); … }
#
# — inside `var == 0`, and only when the operand carried **no** value. So a
# global array this command made *with* a value is visible, one made without is
# not, and one the command merely *found* keeps whatever it had.
#
# A local is never made visible this way. `make_local_variable` returns an
# existing same-context local untouched —
#
#	  /* local foo; local foo;  is a no-op. */
#	  if (old_ref == 0 && old_var && local_p (old_var) &&
#	      old_var->context == variable_context)
#	    return (old_var);				(variables.c:2622)
#
# — and flags a fresh one invisible at the end (`if (was_tmpvar == 0 &&
# value_cell (new_var) == 0) VSETATTR (new_var, att_invisible)`, :2757), which
# `make_local_array_variable` does not undo. `-g` is off the local branch and so
# answers like the global spellings.
exec 2>&1
echo "--- the destroy refusal leaves the array exactly as visible as it was"
( declare -a g; declare +a g=5; declare -p g )
( declare -a g; declare +a g; declare -p g )
( declare -a g=(); declare +a g=5; declare -p g )
( declare -a g=(1); declare +a g=5; declare -p g )
( declare -A h; declare +A h=5; declare -p h )
( declare -a q=(1 2); declare -A +a q; declare -p q )
echo "--- but an array this command made *with* a value was visible already"
( declare +a "g[1]=5"; declare -p g )
( declare +a "g[1]"; declare -p g )
echo "--- and the -aA self-conflict answers the same way"
( declare -A ex; declare -aA ex=5; declare -p ex )
( declare -A ex; declare -aA ex; declare -p ex )
( declare -aA fresh=5; declare -p fresh )
( declare -aA fresh; declare -p fresh )
( declare -aA -l s=AB; declare -p s )
echo "--- as does the conversion refusal beside it, which never made an array"
( declare -a ex; declare -aA ex=5; declare -p ex )
( declare -a g; declare -A g=5; declare -p g )
( declare -A g; declare -a g=5; declare -p g )
echo "--- a local array is invisible however it was made, valued or not"
t1() { local -a g; local +a g=5; declare -p g; }; ( t1 )
t2() { local +a "g[1]=5"; declare -p g; }; ( t2 )
t3() { local -aA fresh=5; declare -p fresh; }; ( t3 )
t4() { local -A ex; local -aA ex=5; declare -p ex; }; ( t4 )
t5() { local -aA -l s=AB; declare -p s; }; ( t5 )
echo "--- and declare/typeset bind locals in a function too"
t6() { declare -a g; declare +a g=5; declare -p g; }; ( t6 )
t7() { declare -aA fresh=5; declare -p fresh; }; ( t7 )
t8() { typeset -aA fresh=5; declare -p fresh; }; ( t8 )
echo "--- while -g is off the local branch, and answers like the global spelling"
t9() { declare -g -a g; declare -g +a g=5; declare -p g; }; ( t9 )
ta() { declare -g +a "g[1]=5"; declare -p g; }; ( ta )
tb() { declare -g -aA fresh=5; declare -p fresh; }; ( tb )
echo "--- neighbours that raise no refusal at all, and store as they always did"
( declare +A "h[k]=5"; declare -p h )
( declare -i z; declare -a z=5; declare -p z )
echo TAIL
