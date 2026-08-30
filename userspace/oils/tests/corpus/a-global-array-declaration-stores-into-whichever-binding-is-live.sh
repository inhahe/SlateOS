# `declare -g -a name=value` splits the same way `declare -g name[sub]=value`
# does, and for the same reason: the store is made **by name**. The whole-array
# store is declare.def:970
#
#	  else if (simple_array_assign)
#	    {
#	      if (assoc_p (var))
#		bind_assoc_variable (var, name, savestring ("0"), value, aflags|ASS_FORCE);
#	      else
#		bind_array_variable (name, 0, value, aflags|ASS_FORCE);
#	    }
#
# — and `bind_array_variable` begins `entry = find_shell_variable (name)`
# (arrayfunc.c:266), an ordinary lookup, where everything above it held the
# `find_global_variable` one. The **assoc** half of the same branch is handed
# `var` itself, so `-A` does not split.
#
# `ASS_FORCE` is passed, so the live binding's readonly does not stop the store
# and raises nothing: `local -r g=5` takes the element anyway. An assoc live
# binding is converted (`array_p (entry) == 0` → `convert_var_to_array`,
# arrayfunc.c:284), and the fold is the live variable's — `-i`/`-u` given to the
# declaration land on the global and do nothing to what is stored.
exec 2>&1
echo "--- the local takes the element, the global is declared and left empty"
t1() { local g=5; declare -g -a g=9; declare -p g; }; ( t1; echo AFTER; declare -p g )
t2() { local g=5; typeset -g -a g=9; declare -p g; }; ( t2; echo AFTER; declare -p g )
t3() { local g=5; local -g -a g=9; declare -p g; }; ( t3; echo AFTER; declare -p g )
t4() { local g=5; declare -g -a g+=9; declare -p g; }; ( t4; echo AFTER; declare -p g )
t5() { local -a g=(1 2); declare -g -a g=9; declare -p g; }; ( t5; echo AFTER; declare -p g )
t6() { local -a g=(1 2); declare -g -a g+=9; declare -p g; }; ( t6; echo AFTER; declare -p g )
echo "--- a global that was already there is converted, and still never stored into"
( declare -a g=(7 8); t7() { local -a g=(1 2); declare -g -a g=9; declare -p g; }; t7; echo AFTER; declare -p g )
( g=7; t8() { local g=5; declare -g -a g=9; declare -p g; }; t8; echo AFTER; declare -p g )
echo "--- and it is the global whose kind decides there is an element store at all"
( declare -a g=(7 8); t7a() { local g=5; declare -g g=9; declare -p g; }; t7a; echo AFTER; declare -p g )
( declare -a g=(7 8); t7b() { local g=5; declare -g g+=9; declare -p g; }; t7b; echo AFTER; declare -p g )
( declare -a g=(7 8); t7c() { local -a g=(1 2); declare -g g=9; declare -p g; }; t7c; echo AFTER; declare -p g )
( declare -A g=([k]=1); t7d() { local g=5; declare -g g=9; declare -p g; }; t7d; echo AFTER; declare -p g )
( declare -a g=(7 8); t7e() { local g=5; declare -g g=(3 4); declare -p g; }; t7e; echo AFTER; declare -p g )
echo "--- every letter goes on the global, and folds nothing on the way past"
t9() { local g=5; declare -g -a -i g=4+4; declare -p g; }; ( t9; echo AFTER; declare -p g )
ta() { local -i g=5; declare -g -a g=4+4; declare -p g; }; ( ta; echo AFTER; declare -p g )
tb() { local g=5; declare -g -a -u g=ab; declare -p g; }; ( tb; echo AFTER; declare -p g )
tc() { local -u g=5; declare -g -a g=ab; declare -p g; }; ( tc; echo AFTER; declare -p g )
td() { local g=5; declare -g -a -x g=9; declare -p g; }; ( td; echo AFTER; declare -p g )
te() { local g=5; declare -g -a -r g=9; echo "st=$?"; declare -p g; }; ( te; echo AFTER; declare -p g )
echo "--- the store forces past a readonly live binding, and reports nothing"
tf() { local -r g=5; declare -g -a g=9; echo "st=$?"; declare -p g; }; ( tf; echo AFTER; declare -p g )
echo "--- an associative live binding is converted where a fresh one is made"
tg() { local -A g; declare -g -a g=9; echo "st=$?"; declare -p g; }; ( tg; echo AFTER; declare -p g )
echo "--- but -A is handed the variable itself, and so does not split"
th() { local g=5; declare -g -A g=9; declare -p g; }; ( th; echo AFTER; declare -p g )
echo "--- nor does a compound literal, which is assigned to the variable"
tj() { local g=5; declare -g -a g=(1 2); declare -p g; }; ( tj; echo AFTER; declare -p g )
echo "--- nor a plain scalar, which never reaches the array branch at all"
tk() { local g=5; declare -g g=9; declare -p g; }; ( tk; echo AFTER; declare -p g )
echo "--- and the store follows a live reference, which the declaration does not"
to() { local -n g=w; local w=1; declare -g -a g=9; declare -p g w; }; ( to; echo AFTER; declare -p g )
echo "--- only the operand carrying the array kind is split"
tp() { local g=5; declare -g -a g=9 h=8; declare -p g h; }; ( tp; echo AFTER; declare -p g h )
tq() { local g=5; declare -g -a g=9; declare -g -a g=8; declare -p g; }; ( tq; echo AFTER; declare -p g )
echo "--- a valueless operand makes the global, and leaves it unvalued"
tr() { local g=5; declare -g -a g; echo "st=$?"; declare -p g; }; ( tr; echo AFTER; declare -p g )
echo "--- and with nothing to shadow it there is only one variable to reach"
ts() { declare -g -a g=9; declare -p g; }; ( ts; echo AFTER; declare -p g )
echo "--- a readonly global the declaration is about is refused, and does fail"
( declare -r g=7; tt() { local g=5; declare -g -a g=9; echo "st=$?"; declare -p g; }; tt; echo AFTER; declare -p g )
echo "--- an associative global refuses the conversion before any store"
( declare -A g; tu() { local g=5; declare -g -a g=9; echo "st=$?"; declare -p g; }; tu; echo AFTER; declare -p g )
echo "--- -G asks for the frame's own binding first, and finds no split to make"
tv() { local g=5; declare -G -a g=9; declare -p g; }; ( tv; echo AFTER; declare -p g )
echo TAIL
