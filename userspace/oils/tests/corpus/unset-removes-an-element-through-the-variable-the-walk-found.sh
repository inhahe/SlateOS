# `unset name[sub]` looks the base up **once** and removes the element through
# the variable that walk handed back (builtins/set.def:927):
#
#	  var = unset_function ? find_function (name)
#			       : (nameref ? find_variable_last_nameref (name, 0) : find_variable (name));
#	  …
#	  if (var && unset_array)
#	    tem = unbind_array_element (var, t, vflags);
#	  …
#	  else if (var == 0 && nameref == 0 && unset_function == 0)
#	    {
#	      var = find_variable_last_nameref (name, 0);
#	      …
#	    }
#
# So the walk count is one where the lookup answered and two where it did not —
# visible through a nameref cycle that closes on a local, which escapes to
# global scope (see
# a-nameref-cycle-that-closes-on-a-local-resolves-at-global-scope.sh) and so
# answers only when there is a global to answer with.
#
# `unbind_array_element` (arrayfunc.c:1115) decides the rest from what it was
# handed, not from what was written: index 0 of anything that is not an array
# unbinds the **whole variable**, by name — which is one more walk, and lands on
# the binding nearest the command rather than on the one the escape reached.
n() { printf '%-26s ' "$1"; c=$2; ( eval "$c" ) 2>&1 | grep -c 'circular name reference'; }
echo "--- one walk where the lookup answered, two where it did not"
declare -a ea=(1 2 3)
declare -A em=([k]=V [j]=W)
es=S
n 'reached an array'   'f() { local -n ea=ea; unset "ea[1]"; }; f'
n 'reached an assoc'   'f() { local -n em=em; unset "em[k]"; }; f'
n 'reached [@]'        'f() { local -n ea=ea; unset "ea[@]"; }; f'
n 'reached a scalar[0]' 'f() { local -n es=es; unset "es[0]"; }; f'
n 'reached a scalar[1]' 'f() { local -n es=es; unset "es[1]"; }; f'
n 'reached nothing'    'f() { local -n en=en; unset "en[0]"; }; f'
n 'whole variable'     'f() { local -n ea=ea; unset ea; }; f'
echo "--- and the removal lands in the scope the walk named"
f1() { local -n ea=ea; unset "ea[1]"; }; f1 2>/dev/null; declare -p ea
f2() { local -n em=em; unset "em[k]"; }; f2 2>/dev/null; declare -p em
f3() { local -n ea=ea; unset "ea[@]"; }; f3 2>/dev/null; declare -p ea
# Index 0 of a scalar unbinds the whole variable *by name*, so what goes is the
# local reference in front of it — the global it escaped to is left standing.
f4() { local -n es=es; unset "es[0]"; declare -p es; }; f4 2>/dev/null; declare -p es
echo "--- a name declared but never assigned is still a variable to find"
declare iv0; unset "iv0[0]"; echo "st=$?"; declare -p iv0 2>&1
declare iv1; unset "iv1[1]" 2>&1; echo "st=$?"; declare -p iv1 2>&1
declare iv3; unset "iv3[@]" 2>&1; echo "st=$?"; declare -p iv3 2>&1
export E0; unset "E0[0]"; echo "st=$?"; declare -p E0 2>&1
declare -i n0; unset "n0[0]"; echo "st=$?"; declare -p n0 2>&1
declare -a q0; unset "q0[0]"; echo "st=$?"; declare -p q0 2>&1
declare -A w0; unset "w0[k]"; echo "st=$?"; declare -p w0 2>&1
# …and one the shell answers with a value function is too.
unset "RANDOM[0]"; echo "st=$?"; declare -p RANDOM 2>&1; echo "[${RANDOM:+set}]"
echo "--- a name with no binding at all is never even probed"
unset "nosuch0[0]"; echo "st=$?"
unset "nosuch1[x y]" 2>&1; echo "st=$?"
unset "nosuch2[@]" 2>&1; echo "st=$?"
echo "--- a reference designating an element discards the written subscript"
declare -a a1=(1 2 3); declare -n r1="a1[2]"
unset "r1[0]" 2>&1; echo "st=$?"; declare -p a1 r1
declare -A m5=([k]=V); declare -n r5="m5[k]"
unset "r5[zz]" 2>&1; echo "st=$?"; declare -p m5
s4=S; declare -n r4="s4[0]"
unset "r4[1]" 2>&1; echo "st=$?"; declare -p s4 2>&1
declare -n r3="nosuch[0]"
unset "r3[0]" 2>&1; echo "st=$?"; declare -p r3
echo "--- a cycle at global scope reaches nothing and pays both walks"
declare -n c1=c2; declare -n c2=c1
unset "c1[0]" 2>&1; echo "st=$?"; declare -p c1 c2 2>&1
echo TAIL
