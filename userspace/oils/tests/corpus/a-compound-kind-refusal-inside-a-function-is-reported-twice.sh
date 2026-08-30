# `-a` and `-A` name a *kind*, and an array's kind is fixed: a declaration
# builtin that would reinterpret an existing array under the other one is
# refused. Where that refusal is spoken from, and what it costs, depends on
# whether there is a function frame around it — not on which name is involved.
#
# At top level it is the compound-assignment machinery's alone: one untagged
# line, and the rest of the parse unit is thrown away, so nothing after the
# command on that line runs.
#
# Inside a function the same operand is refused twice — once by the machinery
# and once by the builtin handed the same word — and the command merely *fails*:
# it returns 1 and the function carries on. Both halves are spoken for every
# refused operand before either builtin half, so a command with two of them
# prints all the machinery lines and then all the builtin ones.
#
# The machinery's half carries the running function's name only when the binding
# it would make is a **global** one — `declare -g` from inside a function. A
# declaration that binds a local is untagged, exactly as at top level. The name
# is the innermost function's, not the outermost.
#
# A refused operand is abandoned whole: no attribute of the command lands on it,
# and the array keeps the value and the kind it had. Operands beside it are
# unaffected.
#
# There is nothing to refuse where the two kinds never meet: a local declaration
# shadows a global of the other kind rather than converting it, and a `-g` one
# reaches past a local of the same name to a global that may not exist yet.

echo '=== at top level: one line, and the rest of the unit is discarded'
( n=(a b c); declare -A n=([k]=v); echo "s=$?"; declare -p n )
( n=(a b c); z=(1); declare -A n=([k]=v) z=([q]=w); echo "s=$?"; declare -p n z )
echo '=== inside a function, a local re-declaration is refused twice, untagged'
( f() { declare -a q=(1 2); declare -A q=([k]=v); echo "s=$?"; declare -p q; }; f )
( f() { declare -a q=(1 2); local -A q=([k]=v); echo "s=$?"; declare -p q; }; f )
( f() { declare -A q=([k]=v); declare -a q=(1 2); echo "s=$?"; declare -p q; }; f )
echo '=== a -g one is tagged with the running function-s name'
( n=(a b c); f() { declare -gA n=([k]=v); echo "s=$?"; declare -p n; }; f )
( n=(a b c); g() { declare -gA n=([k]=v); }; f() { g; echo "s=$?"; }; f )
echo '=== …and the function carries on'
( n=(a b c); f() { declare -gA n=([k]=v); echo "reached s=$?"; echo second; }; f; echo "outer=$?" )
echo '=== every machinery half comes before every builtin one'
( n=(a b c); z=(1); f() { declare -gA n=([k]=v) z=([q]=w); echo "s=$?"; declare -p n z; }; f )
echo '=== a refused operand is abandoned whole; its neighbour is not'
( n=(a b c); f() { declare -grA n=([k]=v) ok=([q]=w); echo "s=$?"; declare -p n; declare -p ok; }; f )
echo '=== nothing to refuse where a local shadows instead'
( n=(a b c); f() { declare -A n=([k]=v); echo "s=$?"; declare -p n; }; f; declare -p n )
( n=(a b c); f() { local -A n=([k]=v); echo "s=$?"; declare -p n; }; f; declare -p n )
echo '=== nor where -g reaches past a local to a global that is not there'
( f() { declare -a q=(1 2); declare -gA q=([k]=v); echo "s=$?"; declare -p q; }; f )
( g() { declare -gA q=([k]=v); }; f() { declare -a q=(1 2); g; echo "s=$?"; declare -p q; }; f )
( f() { declare -gA q=([k]=v); echo "s=$?"; declare -p q; }; f )
echo '=== the same kinds are no conversion at all'
( n=(a b c); f() { declare -ga n=(x y); echo "s=$?"; declare -p n; }; f )
( declare -A m=([k]=K); f() { declare -gA m=([q]=v); echo "s=$?"; declare -p m; }; f )
