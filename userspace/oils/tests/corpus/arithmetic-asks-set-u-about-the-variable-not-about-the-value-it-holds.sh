# Arithmetic reads a name as a *variable*, so `set -u` asks a different question
# there than it asks of a word: not "does this have a value" but "is there a
# variable here at all, and has anything been assigned to it". The two answers
# genuinely differ in both directions — `declare -a a=(); echo "${a[0]}"` is an
# unbound-variable error while `$(( a[0] ))` is a silent 0, and a bare
# `declare -a a`, which declares without assigning, is the other way round.
#
# It is checked once per operand the evaluator actually reaches, before the
# subscript beside it is evaluated, and it abandons the expression where it
# stands — so only the first unset name is ever named. (bash's `expr_streval`,
# expr.c:1180: `find_variable` plus `invisible_p`, guarded by `if (noeval)`.)
n() { sed 's/^.*: line [0-9]*: //'; }
q() { ( set -u; eval "$1"; echo "rc=$?" ) 2>&1 | n; }

echo "=== an unset name is an error wherever arithmetic reads one"
q '(( nope )); echo tail'
q 'echo "$(( nope ))"'
q 'echo "$(( nope + 0 ))"'
q 'let "nope"; echo tail'
q 'declare -i i=nope; echo tail'
q 'for ((i=nope;0;)); do :; done; echo tail'
q 'declare -a a=(x y); echo "${a[nope]}"'
q 'a[nope]=1; echo tail'

echo "=== an operand the evaluator never reaches is never asked"
q 'echo "$(( 0 && nope ))"'
q 'echo "$(( 1 || nope ))"'
q 'echo "$(( 1 ? 2 : nope ))"'
q 'echo "$(( 0 ? nope : 2 ))"'
q 'x=0; echo "$(( x++ , 0 ? nope : 3 ))"'
q 'echo "$(( 1 , nope ))"'

echo "=== the check is on the variable, so an empty one is a silent 0"
q 'e=""; echo "$(( e + 1 ))"'
q 'e=; echo "$(( e ))"'
q 'declare v=; echo "$(( v ))"'
q 'f() { local v=; echo "$(( v ))"; }; f'

echo "=== a declaration is not an assignment, so a bare one is unset"
q 'declare v; echo "$(( v ))"'
q 'declare -i v; echo "$(( v ))"'
q 'declare -r v; echo "$(( v ))"'
q 'declare -a a; echo "$(( a ))"'
q 'declare -A m; echo "$(( m ))"'
q 'f() { local v; echo "$(( v ))"; }; f'
q 'f() { local -a b; echo "$(( b ))"; }; f'

echo "=== and an empty assignment is one, however empty it leaves the name"
q 'declare -a a=(); echo "$(( a ))"'
q 'declare -A m=(); echo "$(( m ))"'
q 'declare -a a; a=(); echo "$(( a ))"'
q 'declare -a a; a[0]=3; echo "$(( a ))"'

echo "=== a subscripted read asks about the array, never about the element"
q 'declare -a a=(1 2); echo "$(( a[9] ))"'
q 'declare -a a=(); echo "$(( a[0] ))"'
q 'declare -a a; echo "$(( a[0] ))"'
q 'declare -A m=([k]=7); echo "$(( m[zz] ))"'
q 'declare -A m=(); echo "$(( m[k] ))"'
q 'declare -A m; echo "$(( m[k] ))"'
q 'echo "$(( nada[3] ))"'
q 'v=5; echo "$(( v[0] ))"'
q 'v=5; echo "$(( v[1] ))"'

echo "=== the word beside it asks the other question, and answers differently"
q 'declare -a a=(); echo "[${a[0]}]"'
q 'declare -a a; echo "[${a[0]}]"'
q 'declare -a a=(); echo "[$a]"'
q 'declare v=; echo "[$v]"'

echo "=== the array is asked about before the subscript is evaluated"
q 'echo "$(( nada[nope] ))"'
q 'declare -a a=(1 2); echo "$(( a[nope] ))"'
q 'echo "$(( nope1 + nope2 ))"'
q 'echo "$(( nope2 + nope1 ))"'

echo "=== and the expression is abandoned where it stands"
q 'echo "$(( nope / 0 ))"'
q 'echo "$(( 1/0 + nope ))"'
q 'let "x=3" "nope" "y=4"; echo tail'

echo "=== a read-modify-write reads; a plain assignment does not"
q '(( nope = 5 )); echo "[$nope]"'
q '(( nada[0] = 1 )); echo "[${nada[0]}]"'
q '(( nope += 5 )); echo tail'
q '(( nope++ )); echo tail'
q '(( ++nope )); echo tail'
q '(( nada[0] += 1 )); echo tail'
q '(( nada[nope] += 1 )); echo tail'
q '(( nada[nope] = 1 )); echo tail'

echo "=== a value read through a set name is evaluated in its turn"
q 'b=nope; echo "$(( b ))"'
q 'declare -a a=(nope); echo "$(( a[0] ))"'

echo "=== a reference is asked about where it points, unless it carries a subscript"
q 'declare -n r=nope; echo "$(( r ))"'
q 'nope=7; declare -n r=nope; echo "$(( r ))"'
q 'declare -a n=(a 5); declare -n r="n[1]"; echo "$(( r ))"'
q 'declare -n r="nada[1]"; echo "$(( r ))"'
q 'declare -n r=nada; echo "$(( r[0] ))"'
q 'declare -a d=(4 5); declare -n r=d; echo "$(( r[1] ))"'
q 'declare -n a=b; declare -n b=a; echo "$(( a ))"'
q 'declare -n r; echo "$(( r ))"'
q 'declare -n r=nope; (( r = 4 )); echo "[$nope]"'
q 'declare -n r=nope; (( r += 4 )); echo tail'

echo "=== the shell own names are variables like any other"
q 'echo "$(( RANDOM > -1 ))"'
q 'echo "$(( SECONDS > -1 ))"'
q 'echo "$(( LINENO > 0 ))"'
q 'echo "$(( BASHPID > 0 ))"'
q 'echo "$(( SRANDOM > -1 ))"'
q 'echo "$(( BASH_SUBSHELL >= 0 ))"'
q 'echo "$(( EPOCHSECONDS > 0 ))"'
q 'echo "$(( OPTIND ))"'
q 'echo "$(( PIPESTATUS[0] ))"'
q 'echo "$(( BASH_ALIASES[x] ))"'

echo "=== a name that was set and then unset is unset again"
q 'v=1; unset v; echo "$(( v ))"'
q 'declare -a a=(1); unset a; echo "$(( a[0] ))"'
q 'declare -a a=(1); unset "a[0]"; echo "$(( a[0] ))"'

echo "=== it aborts the shell the way any other unbound variable does"
q '(( nope )); echo tail'
q 'echo "$(( nope ))"; echo tail'
q 'f() { echo "$(( nope ))"; }; f; echo tail'
q '( echo "$(( nope ))" ); echo tail'

echo "=== with nounset off it is simply zero"
q 'set +u; echo "$(( nope + 1 ))"'
q 'set +u; declare -a a; echo "$(( a[0] ))"'
q 'set +u; declare -n r=nope; echo "$(( r ))"'
echo "=== done"
