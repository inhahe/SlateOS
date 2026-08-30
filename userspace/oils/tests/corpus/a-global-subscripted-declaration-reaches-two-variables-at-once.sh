# `declare -g name[sub]=value` in a function reaches **two** variables. bash
# does not swap scopes, it holds both: `-g` takes the operand off the local
# branch (`variable_context && mkglobal == 0` is false, declare.def:601), so the
# variable the declaration is *about* is `find_global_variable (name)` — but the
# element store is
#
#	  var = assign_array_element (name, value, local_aflags, …);	(declare.def:960)
#
# and that begins `entry = find_variable (vname)` (arrayfunc.c:363), an ordinary
# lookup. So the declaration half lands on the global and the store half on
# whichever binding is live.
#
# The global is left an **empty** array carrying every letter, and visible —
# declare.def:802 re-hides a variable it just made only `if (offset == 0)`, and
# here the operand has a value. The store carries almost nothing over with it:
# `local_aflags = aflags & ASS_APPEND` (declare.def:957), so `-i`/`-u` fold by
# the *live* variable's attributes, which are its own.
#
# A readonly refusal raised by the store does not fail the builtin, either:
# `bind_array_variable` (arrayfunc.c:280) and `bind_assoc_variable` (:311)
# `err_readonly` and then `return (entry)` — non-NULL, so declare.def:962's
# `if (var == 0)` never bumps `assign_error`.
exec 2>&1
echo "--- the local takes the element, the global is declared and left empty"
t1() { local g=5; declare -g g[1]=9; declare -p g; }; ( t1; echo AFTER; declare -p g )
t2() { local g=5; local -g g[1]=9; declare -p g; }; ( t2; echo AFTER; declare -p g )
t3() { local g=5; typeset -g g[1]=9; declare -p g; }; ( t3; echo AFTER; declare -p g )
t4() { local g=5; declare -g g[1]+=9; declare -p g; }; ( t4; echo AFTER; declare -p g )
t5() { local g=5; declare -g g[0]=9; declare -p g; }; ( t5; echo AFTER; declare -p g )
echo "--- a global that was already there is converted, and still never stored into"
( g=7; t6() { local g=5; declare -g g[1]=9; declare -p g; }; t6; echo AFTER; declare -p g )
( declare -a g=(7 8); t7() { local g=5; declare -g g[1]=9; declare -p g; }; t7; echo AFTER; declare -p g )
( declare -A g; t8() { local -A g; declare -g g[k]=9; declare -p g; }; t8; echo AFTER; declare -p g )
echo "--- every letter goes on the global, and folds nothing on the way past"
t9() { local g=5; declare -g -r g[1]=9; declare -p g; }; ( t9; echo AFTER; declare -p g )
ta() { local g=5; declare -g -i g[1]=4+4; declare -p g; }; ( ta; echo AFTER; declare -p g )
tb() { local g=5; declare -g -x g[1]=9; declare -p g; }; ( tb; echo AFTER; declare -p g )
tc() { local g=5; declare -g -u g[1]=ab; declare -p g; }; ( tc; echo AFTER; declare -p g )
td() { local g=5; declare -g -a g[1]=9; declare -p g; }; ( td; echo AFTER; declare -p g )
echo "--- the two halves need not even agree on the kind"
te() { local g=5; declare -g -A g[k]=9; declare -p g; }; ( te; echo AFTER; declare -p g )
tf() { local -A g; declare -g g[k]=9; declare -p g; }; ( tf; echo AFTER; declare -p g )
( declare -A g; tg() { local g=5; declare -g g[k]=9; declare -p g; }; tg; echo AFTER; declare -p g )
echo "--- and the store follows a live reference, which the declaration does not"
th() { local -n g=w; local w=1; declare -g g[1]=9; declare -p g w; }; ( th; echo AFTER; declare -p g )
echo "--- a readonly live binding is reported by the store and fails nothing"
ti() { local -r g=5; declare -g g[1]=9; echo "st=$?"; declare -p g; }; ( ti; echo AFTER; declare -p g )
echo "--- but one the declaration itself is about is refused, and does fail"
( declare -r g=7; tj() { local g=5; declare -g g[1]=9; echo "st=$?"; declare -p g; }; tj; echo AFTER; declare -p g )
echo "--- only the operand carrying the subscript is split"
tk() { local g=5; declare -g g[1]=9 h[2]=8; declare -p g h; }; ( tk; echo AFTER; declare -p g h )
tl() { local g=5; declare -g g=9; declare -p g; }; ( tl; echo AFTER; declare -p g )
echo "--- twice over, the second store finds what the first left"
tm() { local g=5; declare -g g[1]=9; declare -p g; declare -g g[2]=8; declare -p g; }
( tm; echo AFTER; declare -p g )
echo "--- and with nothing to shadow it there is only one variable to reach"
tn() { declare -g g[1]=9; declare -p g; }; ( tn; echo AFTER; declare -p g )
to() { local -g g[1]=9; declare -p g; }; ( to; echo AFTER; declare -p g )
echo "--- a valueless operand makes the global, and leaves it unvalued"
tp() { local g=5; declare -g g[1]; echo "st=$?"; declare -p g; }; ( tp; echo AFTER; declare -p g )
echo "--- while a local array of the frame's own is what -G writes"
tq() { local g=5; readonly g[1]=9; echo "st=$?"; declare -p g; }; ( tq; echo AFTER; declare -p g )
tr() { local g=5; export g[1]=9; echo "st=$?"; declare -p g; }; ( tr; echo AFTER; declare -p g )
echo TAIL
