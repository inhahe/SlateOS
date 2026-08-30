# `do_assignment_internal` (subst.c:396) has the value in hand before it goes
# looking for anything to put it in:
#
#	  if (flags & ASS_APPEND) …
#	  …
#	  if (t = mbschr (name, '['))	/* ] */
#	    { … entry = assign_array_element (name, value, aflags, &estate); … }
#	  else
#	    entry = bind_variable (name, value, aflags);
#
# The `value` those two are handed was expanded by the caller. So every
# objection the *name* earns — a reference designating a whole array, one
# designating an element that a written subscript has nowhere to go on, a
# readonly target — is reported only after the value's side effects have run.
# A **compound literal** is the one exception, and bash's own: it is not
# expanded until `assign_compound_array_list`, which all of those come before.
exec 2>&1
f() { echo "  RAN($1)"; echo "v$1"; }
echo "--- the value runs, and then the name is judged"
declare -a n=(1 2); declare -n r1='n[@]'
r1=$(f whole); echo "st=$?"; declare -p n
declare -a m=(1 2); declare -n r2='m[1]'
r2[0]=$(f onelem); echo "st=$?"; declare -p m
readonly ro=1
ro=$(f ro); echo "st=$?"; declare -p ro
declare -n r3=ro
r3=$(f roref); echo "st=$?"
echo "--- a compound literal is refused without being expanded"
readonly ra=1
ra=($(f compound)); echo "st=$?"; declare -p ra
declare -a n2=(1 2); declare -n r4='n2[1]'
r4=($(f compound2)); echo "st=$?"; declare -p n2
echo "--- and a redirection expands it once, not once per link"
declare -n p1=p2; declare -n p2=p3
p1=$(f chain); echo "st=$?"; declare -p p3
declare -a q=(A B); declare -n p3='q[1]'
p1=$(f toelem); echo "st=$?"; declare -p q
echo "--- a value that will not expand is not stored, and nothing is asked"
declare -a z=(1 2); declare -n r5='z[@]'
r5=$((1/0)); echo "st=$?"
readonly rb=1
rb=$((1/0)); echo "st=$?"; declare -p rb
echo "--- the trace comes after the value, and shows the value"
set -x
tr1=$(f traced)
declare -n r6=tr2
r6=$(f tracedref)
tr3[0]=$(f tracedelem)
set +x
declare -p tr1 tr2 tr3
echo TAIL
