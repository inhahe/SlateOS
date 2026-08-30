# A `[[ … ]]` test may be broken across lines, but not everywhere: the parser
# skips newlines while it waits for a term or for what follows a finished one,
# and reads directly in the two places where a binary operator or its right
# operand goes. So `[[ a == b <newline> ]]` parses and `[[ a <newline> == b ]]`
# does not.
echo "=== a conditional may be broken across lines"
[[ -n a
]] && echo t1
[[ -n a &&
   -n b ]] && echo t2
[[ -n a
   && -n b ]] && echo t3
[[ -n a ||
   -n b ]] && echo t4
[[ (
   -n a ) ]] && echo t5
[[ !
   -z a ]] && echo t6
[[ a == a
]] && echo t7
[[ a =~ ^a$
]] && echo t8

echo "=== including in the places a conditional normally appears"
if [[ -n a
]]; then echo t9; fi
while [[ -z "$s" ]]; do s=x
done; echo "t10=$s"
f() {
	[[ -n a &&
	   -n b ]]
}
f && echo t11
declare -f f

echo "=== but a line may not end where an operator is expected"
( eval '[[ a
== a ]]' ); echo "rc=$?"
( eval '[[ a -eq
1 ]]' ); echo "rc=$?"
( eval '[[ -n
a ]]' ); echo "rc=$?"

echo "=== and the input running out is a line end of its own"
# Every probe below is a *complete* line of source with no newline after it,
# which the parser has to treat exactly as if one were there.
( eval 'case x in y' ); echo "rc=$?"
( eval 'case x in y|z' ); echo "rc=$?"
( eval 'case x in (y' ); echo "rc=$?"
( eval 'for' ); echo "rc=$?"
( eval 'coproc' ); echo "rc=$?"
( eval 'echo <' ); echo "rc=$?"
( eval 'echo >' ); echo "rc=$?"
# … while the positions that *do* accept a newline report the end of input.
( eval 'case x in' ); echo "rc=$?"
( eval 'if true' ); echo "rc=$?"
( eval 'for i in' ); echo "rc=$?"
