# bash's grammar for a function definition is `WORD ( )` — *any* word, not an
# identifier. So `my-func`, `a.b`, `1f` and even `a/b` are all real function
# names. The name that is written is the name that is stored: it is never
# expanded, which is why a *quoted* one is refused — and refused at run time,
# by the definition itself, rather than by the parser.
echo "=== a function name need not be an identifier"
my-func() { echo hi; }; my-func
a.b() { echo hi; }; a.b
1f() { echo hi; }; 1f
a/b() { echo hi; }; "a/b"
a,b() { echo hi; }; "a,b"
.f() { echo hi; }; .f
f%() { echo hi; }; "f%"
f#() { echo hi; }; "f#"
# `[` is not a name character either, so neither of these is a subscript.
[b]() { echo hi; }; "[b]"
a[b]() { echo hi; }; "a[b]"

echo "=== and it is stored, listed and removed under that name"
a-b() { echo hi; }; declare -f a-b; declare -F; type a-b
unset -f a-b; "a-b"; echo "rc=$?"

echo "=== but a quoted or expanded name is refused when the definition runs"
# Not by the parser: the script carries on, with status 1 from the definition.
echo one; \f() { :; }; echo "rc=$?"; echo two
"g"() { :; }
h\i() { :; }
x=j; $x() { :; }
declare -F

echo "=== the same rule holds for the keyword form"
function k-l { echo hi; }; k-l
function m-n() { echo hi; }; m-n
function "o" { :; }; echo "rc=$?"
function \p { :; }
function $x { :; }

echo "=== an assignment is the one word shape that is not a name"
# `f=g` lexes as an assignment word, which the production does not accept.
( eval 'f=g() { echo hi; }' ); echo "rc=$?"
# Escaping the `=` demotes it back to an ordinary word — which is then quoted,
# so it parses and fails at run time instead.
a\=b() { echo hi; }; echo "rc=$?"
# A keyword-form name is never read as an assignment, so this one really is a
# function called `q=r`.
function q=r { echo hi; }; "q=r"
# Nor may a reserved word be a name: it is still the keyword before the `(`.
( eval 'if() { echo hi; }' ); echo "rc=$?"
( eval 'time() { echo hi; }' ); echo "rc=$?"
( eval 'v=1 s() { echo hi; }' ); echo "rc=$?"
