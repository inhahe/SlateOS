# An element store resolves its target *twice* — once to find the array and
# once to bind the element — except where the first walk reached an
# **associative** one, which is bound through the variable already in hand
# (arrayfunc.c:411, `assign_array_element_internal`):
#
#	  if (entry && assoc_p (entry))
#	    {
#	      …
#	      entry = bind_assoc_variable (entry, vname, akey, value, flags);
#	    }
#	  else
#	    {
#	      …
#	      entry = bind_array_variable (vname, ind, value, flags);
#	    }
#
# `bind_array_variable` finds the name again where `bind_assoc_variable` takes
# the variable as an argument, so what decides the count is what the *first*
# walk found — not how the subscript was written. A nameref cycle that closes on
# a local escapes to global scope (see
# a-nameref-cycle-that-closes-on-a-local-resolves-at-global-scope.sh), which is
# what makes the difference visible: each walk of a cycle reports itself, and
# the array being counted sits a frame away from the reference counting it.
n() { printf '%-24s ' "$1"; c=$2; ( eval "$c" ) 2>&1 | grep -c 'circular name reference'; }
echo "--- the declaration warns twice on its own; every row counts from there"
n 'declaration alone'  'declare -a v=(1 2); f() { local -n v=v; :; }; f'
echo "--- an associative array pays one walk for the store"
n 'm[j]=W'             'declare -A m=([k]=V); f() { local -n m=m; m[j]=W; }; f'
n "read 'm[j]'"        'declare -A m=([k]=V); f() { local -n m=m; read "m[j]" <<< W; }; f'
n "printf -v 'm[j]'"   'declare -A m=([k]=V); f() { local -n m=m; printf -v "m[j]" W; }; f'
n 'm[j]+=W'            'declare -A m=([k]=V); f() { local -n m=m; m[j]+=W; }; f'
echo "--- everything else takes the indexed arm and pays a second"
n 'indexed v[1]=W'     'declare -a v=(1 2); f() { local -n v=v; v[1]=W; }; f'
n 'indexed v[1]+=W'    'declare -a v=(1 2); f() { local -n v=v; v[1]+=W; }; f'
n 'scalar v[1]=W'      'v=S; f() { local -n v=v; v[1]=W; }; f'
n 'missing v[1]=W'     'f() { local -n v=v; v[1]=W; }; f'
echo "--- a whole-array fill resolves once whatever it lands on"
n 'v=(a b)'            'declare -a v=(1); f() { local -n v=v; v=(a b); }; f'
n 'read -a v'          'declare -a v=(1); f() { local -n v=v; read -a v <<< "a b"; }; f'
n 'mapfile -t v'       'declare -a v=(1); f() { local -n v=v; mapfile -t v <<< a; }; f'
n 'assoc v=(a b)'      'declare -A v=([k]=V); f() { local -n v=v; v=(a b); }; f'
echo "--- an associative global the escape does not reach decides nothing"
n 'global cycle'       'declare -A m=([k]=V); declare -n c1=c2; declare -n c2=c1; c1[j]=W'
echo "--- and the store really does land, in the scope the walk named"
declare -A m=([k]=V); f() { local -n m=m; m[j]=W; }; f 2>/dev/null; declare -p m
declare -a v=(1 2); g() { local -n v=v; v[1]=W; }; g 2>/dev/null; declare -p v
w=S; h() { local -n w=w; w[1]=W; }; h 2>/dev/null; declare -p w
i() { local -n z=z; z[1]=W; }; i 2>/dev/null; declare -p z
echo TAIL
