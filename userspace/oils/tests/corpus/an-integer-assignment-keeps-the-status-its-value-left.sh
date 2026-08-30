# Binding a value to a `-i` variable evaluates it as arithmetic, and a failure
# there is not reported as a status — it is an *abort*. `make_variable_value`
# (variables.c:2959) does `top_level_cleanup (); jump_to_top_level (DISCARD)`,
# and the reader loop supplies a status only when the unwind arrives without
# one (eval.c:103): "leave existing non-zero values alone".
#
# So the status is whatever expanding the *value* left behind, and the familiar
# 1 is only the fallback for when that was 0. Any status reaches the outside,
# not just a parse failure's 2.
#
# Verified against bash 5.2.37.

e() { ( eval "$1" ) 2>&1; echo "  rc=$?"; }

echo "=== the value's status outlives the arithmetic failure that follows it ==="
# Nothing ran, so there is no status to keep and the fallback applies…
e 'declare -i n; n=2+'
# …but a substitution that ran hands its own status through, whatever it is.
e 'declare -i n; n=1+`false`'
e 'declare -i n; n=1+`fi`'
e 'declare -i n; n=1+`exit 3`'
e 'declare -i n; n=1+`exit 42`'

echo "=== an earlier status is not kept — it was already reset ==="
# `(exit 7)` sets 7, but the `declare` after it succeeds and clears it, so the
# abort finds 0 and falls back to 1. Nothing special is at work here.
e '(exit 7); declare -i n; n=2+'
e 'false; declare -i n; n=2+'
# With the failure *in* the value there is no successful command in between.
e '(exit 7); declare -i n; n=2+`exit 7`'

echo "=== every spelling of the binding does it ==="
e 'declare -i n=1+`exit 3`'
e 'declare -i n=1; n+=1+`exit 3`'
e 'declare -ia a; a[0]=1+`exit 3`'
e 'declare -i n; let "n = 1+`exit 3`"; echo "let=$?"'

echo "=== and the unwind still passes through eval and source ==="
# The abort is raised below the depth `parse_and_execute` guards, so the `after`
# is not printed — and the status that comes out is still the value's.
e 'declare -i n; eval "n=1+\`exit 3\`"; echo after'
e 'declare -i n; eval "n=2+"; echo after'
e 'f() { declare -i n; n=1+`exit 3`; echo after; }; f; echo "f=$?"'

echo "=== a value that fails no arithmetic keeps its status the ordinary way ==="
# No abort at all here: the assignment simply returns the substitution's status.
e 'declare -i n; n=`exit 3`; echo "n=$n"'
e 'declare -i n; n=`fi`'
e 'n=1+`exit 3`; echo "plain=[$n]"'

echo done
