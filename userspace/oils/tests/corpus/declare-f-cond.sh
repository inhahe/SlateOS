# `declare -f` has to print a `[[ … ]]` back with its parentheses intact.
# Dropping them is not just untidy: `( a || b ) && c` reprinted bare is
# `a || (b && c)`, a different test. bash keeps every group, even a redundant
# one, and separately normalises a bare-word test to the `-n` it means.
echo "=== grouping is preserved, redundant or not"
a() {
	[[ ( -n a || -n b ) && -z "" ]]
	[[ -n a || ( -n b && -z "" ) ]]
	[[ ! ( -n a ) ]]
	[[ ! ( ! ( -n a ) ) ]]
	[[ ( ( -n a ) ) ]]
	[[ ( -n a ) && ( -n b ) || ( -n c ) ]]
}
declare -f a

echo "=== a bare word prints as the -n test it is"
b() {
	[[ a ]]
	[[ $x ]]
	[[ "a b" ]]
	[[ ! a ]]
	[[ a && b ]]
	[[ a$x ]]
	[[ ( a ) ]]
}
declare -f b

echo "=== groups inside other operators and other commands"
c() {
	[[ ( -v x ) && ( 1 -eq 1 ) && ( a < b ) ]]
	[[ ( $x == 'a b' ) || ( $x != "c" ) ]]
	[[ ( $x =~ ^a+b$ ) ]]
	if [[ ( -n a || -n b ) && -n c ]]; then echo y; fi
	while [[ ( -z x ) ]]; do break; done
}
declare -f c

echo "=== and the printed form tests the same thing"
x=q
[[ ( -z "" || -n "" ) && -n "$x" ]] && echo s1
[[ -z "" || ( -n "" && -z z ) ]] && echo s2
[[ ! ( -n "" ) ]] && echo s3
[[ ( -n "" || -n b ) && -n c ]] && echo s4
[[ ( $x == 'a b' ) || ( $x != "c" ) ]] && echo s5
eval "$(declare -f a)"
declare -f a
eval "$(declare -f c)"
c
