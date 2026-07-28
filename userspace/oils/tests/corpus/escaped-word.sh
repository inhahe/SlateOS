# Quoting any character of a word stops it being read as a *syntactic* name.
# A backslash is quoting, so `\if` is an ordinary command word rather than the
# keyword — even though nothing about the escaped letter itself has changed,
# and the word still expands to `if`. What is affected is only the reading:
# keywords, assignments, and function names. The command *name* is the
# expansion, so `\echo` still finds the builtin.
echo "=== an escaped keyword is an ordinary word"
( eval '\if true; then echo t; fi' ); echo "rc=$?"
( eval '\while :; do break; done' ); echo "rc=$?"
( eval '\for x in a; do echo $x; done' ); echo "rc=$?"
( eval '\case x in x) echo m;; esac' ); echo "rc=$?"
( eval '\{ echo hi; }' ); echo "rc=$?"
# … and so is a whole word made of escaped letters.
\time true; echo "rc=$?"
\[[ a == a ]]; echo "rc=$?"
\! true; echo "rc=$?"

echo "=== but the command name is still the expansion"
\echo hi
e\cho hi
\:; echo "rc=$?"

echo "=== an escaped name is not an assignment"
v=1
\v=2; echo "rc=$? v=$v"
a\=3; echo "rc=$? a=[$a]"
w\x=4; echo "rc=$? wx=[$wx]"

echo "=== the escape survives into the stored source"
f() { \ls -d .; }; declare -f f
g() { echo \a\b\1; }; declare -f g
h() { \echo hi; }; declare -f h; h

echo "=== including inside a parameter-expansion pattern"
x=abc
i() { echo "${x#\a}"; }; declare -f i; i
j() { echo "${x/\a/Z}"; }; declare -f j; j
k() { echo "${x//\b/-}"; }; declare -f k; k
y=ABC
l() { echo "${y,,\B}"; }; declare -f l; l

echo "=== and an escaped metacharacter is still a literal"
case 'a*b' in a\*b) echo star;; esac
[[ 'a*b' == a\*b ]] && echo starcond
[[ ab == a\* ]] || echo notstar
z='a&b'; echo "${z/\&/+}"
