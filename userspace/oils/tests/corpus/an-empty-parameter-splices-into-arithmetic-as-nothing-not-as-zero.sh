# Arithmetic is evaluated in two steps: the expression is first read as *text*
# and its parameters are spliced in, and only then is that text parsed. The
# splice is literal — whatever the parameter holds goes in where the `$…` was —
# so a parameter holding nothing contributes nothing at all, and the characters
# on either side of it close up against each other. It is not a zero.
#
# The difference only shows once you look at what the text becomes:
#
#   * `$(( 1 + $novar ))` is `1 + ` — an operand is missing, and bash says so
#     rather than answering 1;
#   * `$(( 1 + $novar 2 ))` is `1 +  2`, which is 3;
#   * `$(( a$novar ))` is `a`, and `$(( ${novar}1 ))` is `1` — the splice joins
#     with its neighbours, it is not a token of its own;
#   * an expression that is empty *all through* still answers 0, because an
#     empty expression is 0 — which is why the mistake is easy to miss.
#
# A nested `$(( … ))` is different in kind: it is evaluated, not spliced, and an
# evaluation always produces a number, so `$(( 1 $(( )) 2 ))` really does read
# `1 0 2`. A command substitution, on the other hand, splices its output and so
# behaves like an empty parameter when it prints nothing.

p() { printf '%s\n' "$1"; }

echo '=== an empty value splices as nothing, so the text closes up'
( echo $(( 1 $novar 2 )); p "rc=$?" ) 2>&1
( echo $(( 1 ${novar} 2 )); p "rc=$?" ) 2>&1
( echo $(( 1 ${novar:-} 2 )); p "rc=$?" ) 2>&1
( e=; echo $(( 1 $e 2 )); p "rc=$?" ) 2>&1
( echo $(( 1 $(echo) 2 )); p "rc=$?" ) 2>&1

echo '=== …and an expression that needs an operand there fails'
( echo $(( 1 + $novar )); p "rc=$?" ) 2>&1
( echo $(( $novar + 1 )); p "rc=$?" ) 2>&1
( echo $(( -$novar )); p "rc=$?" ) 2>&1
( echo $(( $novar * 2 )); p "rc=$?" ) 2>&1
( echo $(( ($novar) )); p "rc=$?" ) 2>&1

echo '=== an expression that is empty all through is 0'
( echo $(( $novar )); p "rc=$?" ) 2>&1
( echo $(( ${novar} )); p "rc=$?" ) 2>&1
( echo $(( $novar$novar )); p "rc=$?" ) 2>&1
( echo $(( $novar $novar )); p "rc=$?" ) 2>&1
( echo $(( )); p "rc=$?" ) 2>&1

echo '=== the splice is textual, so it joins with what is beside it'
( a=1; echo $(( a$novar )); p "rc=$?" ) 2>&1
( echo $(( ${novar}1 )); p "rc=$?" ) 2>&1
( echo $(( 1$novar )); p "rc=$?" ) 2>&1
( echo $(( 1$novar 2 )); p "rc=$?" ) 2>&1
( a=12; echo $(( 1${novar}2 )); p "rc=$?" ) 2>&1
( o=+; echo $(( 1 $o 2 )); p "rc=$?" ) 2>&1

echo '=== a parameter that has a value is spliced the same way'
( echo $(( $# )); p "rc=$?" ) 2>&1
( echo $(( 1 $# 2 )); p "rc=$?" ) 2>&1
( set -- a b; echo $(( $# )); p "rc=$?" ) 2>&1
( n=7; echo $(( 1 + $n )); p "rc=$?" ) 2>&1

echo '=== nested arithmetic is evaluated, so it never comes out empty'
( echo $(( 1 $(( )) 2 )); p "rc=$?" ) 2>&1
( echo $(( $(( )) )); p "rc=$?" ) 2>&1
( echo $(( 1 + $(( )) )); p "rc=$?" ) 2>&1
( echo $(( 1 + $(( $novar )) )); p "rc=$?" ) 2>&1

echo '=== a command substitution splices its output, so silence is nothing'
( echo $(( 1 + $(echo) )); p "rc=$?" ) 2>&1
( echo $(( 1 + `echo` )); p "rc=$?" ) 2>&1
( echo $(( 1 + $(echo 2) )); p "rc=$?" ) 2>&1

echo '=== the other arithmetic contexts splice alike'
( (( 1 $novar 2 )); p "arith cmd rc=$?" ) 2>&1
( (( 1 + $novar )); p "arith cmd rc=$?" ) 2>&1
( a=(); echo "${a[1 $novar 2]}"; p "sub rc=$?" ) 2>&1
( for ((i=1 $novar 2; 0; )); do :; done; p "for rc=$?" ) 2>&1
( let "1 $novar 2"; p "let rc=$?" ) 2>&1
( let "1 + $novar"; p "let rc=$?" ) 2>&1

# An expansion that fails is fatal to the shell reading it, so these two never
# reach their own `p` — the subshell is gone by then.
( x=$(( 1 $novar 2 )); p "assign rc=$?" ) 2>&1
( declare -i y; y="1 $novar 2"; p "int rc=$?" ) 2>&1

echo '=== a subscript and a slice take the empty text as 0'
( a=(9 8 7); echo "${a[$novar]}"; p "rc=$?" ) 2>&1
( a=(9 8 7); echo "${a[@]:$novar:2}"; p "rc=$?" ) 2>&1

echo done
