# A nameref chain that closes on a *function-local* variable resolves once more
# at global scope (see
# a-nameref-cycle-that-closes-on-a-local-resolves-at-global-scope.sh), and
# `export`/`readonly` follow it there: bash's `set_var_attribute` looks the name
# up with a single `find_variable_notempenv` and applies the attribute to
# whatever that answered, so the mark lands on the **global** and the local
# reference keeps none of it (builtins/setattr.def):
#
#	  var = find_variable_notempenv (name);
#	  if (var == 0)
#	    {
#	      /* We might have a nameref pointing to something that we can't
#		 resolve to a shell variable.  If we do, skip it. … */
#	      refvar = find_variable_nameref_for_create (name, 0);
#	      if (refvar == INVALID_NAMEREF_VALUE)
#		return;
#	    }
#
# That `if (var == 0)` retry is the whole of the walk count: a chain that
# reaches a variable pays one walk, a broken one pays two, and `export -n` —
# the `undo` arm above it — pays one either way. The store, when the operand
# carries a value, walks once for itself and runs *first*, so what the mark
# then looks for already exists.
n() { printf '%-22s ' "$1"; c=$2; ( eval "$c" ) 2>&1 | sed 's/^.*line [0-9]*: //' | tr '\n' '|'; echo; }
echo "--- the mark lands on the global, and the reference keeps none of it"
n 'export'            'e=E; f() { local -n e=e; export e; }; f; declare -p e'
n 'readonly'          'r=R; f() { local -n r=r; readonly r; }; f; declare -p r'
n 'export -n'         'x=X; export x; f() { local -n x=x; export -n x; }; f; declare -p x'
n 'export NAME=value' 'v=V0; f() { local -n v=v; export v=V1; }; f; declare -p v'
n 'export creates it' 'f() { local -n z=z; export z; }; f; declare -p z'
n 'seen from inside'  'e2=E; f() { local -n e2=e2; export e2; declare -p e2; }; f'
echo "--- declare -x is the exception: it marks nothing through the escape"
n 'declare -x'        'd=D; f() { local -n d=d; declare -x d; }; f; declare -p d'
echo "--- a cycle that closes on a global reaches nothing, and pays the retry"
declare -n c1=c2; declare -n c2=c1
( export c1;      echo "st=$?" ) 2>&1 | sed 's/^.*line [0-9]*: //' | tr '\n' '|'; echo ' <export c1'
( export -n c1;   echo "st=$?" ) 2>&1 | sed 's/^.*line [0-9]*: //' | tr '\n' '|'; echo ' <export -n c1'
( export c1=5;    echo "st=$?" ) 2>&1 | sed 's/^.*line [0-9]*: //' | tr '\n' '|'; echo ' <export c1=5'
( export -n c1=5; echo "st=$?" ) 2>&1 | sed 's/^.*line [0-9]*: //' | tr '\n' '|'; echo ' <export -n c1=5'
( readonly c1;    echo "st=$?" ) 2>&1 | sed 's/^.*line [0-9]*: //' | tr '\n' '|'; echo ' <readonly c1'
( readonly c1=5;  echo "st=$?" ) 2>&1 | sed 's/^.*line [0-9]*: //' | tr '\n' '|'; echo ' <readonly c1=5'
declare -p c1 c2
echo "--- an ordinary chain is unchanged: the mark still follows it"
w=5; declare -n rw=w
export rw; declare -p w rw
readonly rw=9; declare -p w rw
echo TAIL
