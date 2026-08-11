# A declaration inside a function *first* finds-or-makes the frame's binding,
# and the array letters decide which kind it makes (declare.def:601):
#
#	  if (variable_context && mkglobal == 0)
#	    {
#	      newname = declare_transform_name (name, flags_on, flags_off);
#	      if (flags_on & att_assoc)
#		var = make_local_assoc_variable (newname, MKLOC_ARRAYOK|inherit_flag);
#	      else if ((flags_on & att_array) || making_array_special)
#		var = make_local_array_variable (newname, MKLOC_ASSOCOK|inherit_flag);
#
# The readonly refusals are at :842-:849, far below it. So a readonly local that
# refuses the *value* has still been widened by the time it does: `local -r g=5;
# declare g[1]=9` reports `readonly variable`, exits 1, and leaves
# `declare -ar g=()` — the kind landed, the value never did.
#
# Only the kind. `VSETATTR (var, flags_on)` is at :944, past the refusal, so the
# declaration's other letters are lost with the value: `-ax` leaves the array
# unexported and `-al` leaves it unfolded.
#
# And only for a name the frame already holds. :614 is itself what refuses a
# *shadow* of a readonly global (`make_local_variable` returns 0), so nothing is
# widened there; and at global scope there is no local branch at all.
exec 2>&1
echo "--- the widening lands, the value does not"
t1() { local -r g=5; declare g[1]=9; echo "st=$?"; declare -p g; }; ( t1; echo AFTER; declare -p g )
t2() { local -r g=5; local g[1]=9; echo "st=$?"; declare -p g; }; t2
t3() { local -r g=5; typeset g[1]=9; echo "st=$?"; declare -p g; }; t3
t4() { local -r g=5; declare -a g=9; echo "st=$?"; declare -p g; }; t4
t5() { local -r g=5; local -a g=5; echo "st=$?"; declare -p g; }; t5
t6() { local -r g=5; typeset -a g=9; echo "st=$?"; declare -p g; }; t6
t7() { local -r g=5; declare -A g=9; echo "st=$?"; declare -p g; }; t7
t8() { local -r g=5; local -A g=5; echo "st=$?"; declare -p g; }; t8
t9() { local -r g=5; declare -i g[1]=9; echo "st=$?"; declare -p g; }; t9
ta() { local -r g=5; declare -a g=(1 2); echo "st=$?"; declare -p g; }; ta
echo "--- a valueless one asks for the widening on its own account and keeps it"
tb() { local -r g=5; declare g[1]; echo "st=$?"; declare -p g; }; tb
tc() { local -r g=5; declare -a g; echo "st=$?"; declare -p g; }; tc
td() { local -r g=5; declare -A g; echo "st=$?"; declare -p g; }; td
echo "--- -A outranks -a, as at :610"
te() { local -r g=5; declare -aA g=5; echo "st=$?"; declare -p g; }; te
echo "--- the other letters are lost with the value (VSETATTR is at :944)"
tf() { local -r g=5; declare -ar g=9; echo "st=$?"; declare -p g; }; tf
tg() { local -r g=5; declare -ax g=9; echo "st=$?"; declare -p g; }; tg
th() { local -r g=5; declare -al g=AB; echo "st=$?"; declare -p g; }; th
echo "--- more than one operand: each is judged on its own"
ti() { local -r g=5; declare g[1]=9 h=2; echo "st=$?"; declare -p g h; }; ti
tj() { local -r g=5; declare -a g=5 h=(1); echo "st=$?"; declare -p g h; }; tj
echo "--- a name that is already an array has nothing to widen"
tk() { local -r g=(1); declare g[1]=9; echo "st=$?"; declare -p g; }; tk
tl() { local -Ar g=([k]=1); declare g[j]=9; echo "st=$?"; declare -p g; }; tl
echo "--- no local branch to widen: a readonly global, and a shadow of one"
tm() { local -r g=5; declare -g -a g=9; echo "st=$?"; declare -p g; }; ( tm; echo AFTER; declare -p g )
tn() { declare g[1]=9; echo "st=$?"; declare -p g; }; ( declare -r g=5; tn; echo AFTER; declare -p g )
echo "--- neighbours that are not refused at all"
to() { local -r g=5; local g=9; echo "st=$?"; declare -p g; }; to
tp() { local -nr g=w; local w=1; declare -a g=9; echo "st=$?"; declare -p g w; }; tp
tq() { local -r g=5; declare -a g=9; echo "st=$?"; declare -p g; }; ( g=7; tq; echo AFTER; declare -p g )
