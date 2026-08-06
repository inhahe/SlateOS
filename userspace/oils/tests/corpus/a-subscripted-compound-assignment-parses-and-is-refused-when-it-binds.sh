# bash's word rule for a compound literal is about the **word**, not the
# destination: `n[1]=(x y)` is an assignment word like any other, and the
# objection to putting a list in one element is raised when the value would be
# bound — `n[1]: cannot assign list to array member`. So a branch that is never
# taken parses and runs perfectly well, and the complaint is a runtime one.
#
# Nothing about the word is resolved or expanded first. The name is the one
# *written* (a nameref is never followed), the subscript is quoted as source (so
# an arithmetic error in it is not raised and a `$( … )` in it does not run), the
# literal's own words are left unexpanded, and the readonly guard is never
# reached.
#
# The abort is the ordinary one — `a[-9]=x`'s, measured against it below — so the
# rest of the list goes and a function takes its caller's list down too. An
# `eval` confines it and reports 1, but only at the top level: inside a subshell
# the same `eval` does not, and the subshell exits 1. That asymmetry is the
# reference case's exactly, so each pair here is written twice.
#
# A command *prefix* is a different word entirely and gets the refusal a
# subscripted prefix always gets (`` `n[1]': not a valid identifier ``, status
# untouched); see a-prefix-assignment-binds-what-its-nameref-resolves-to.sh.
# *After* the command word it is a syntax error, as the unsubscripted literal
# already is — that shape kills a script outright, so it lives in
# a-compound-literal-after-the-command-word-names-the-token-that-surprised-it.sh.

echo '=== an unreached branch is only a parse away'
( if false; then n[1]=(x y); fi; echo OK )
( f() { n[1]=(x y); }; echo OK )
echo '=== the plain shape'
( n=(a b c); n[1]=(x y); echo NOT; declare -p n 2>&1 )
( n=(a b c); n[1]+=(x y); echo NOT )
( n=(a b c); n[1]=(); echo NOT )
echo '=== every base, and none'
( nosuch[1]=(x y); echo NOT; declare -p nosuch 2>&1 )
( v=hi; v[0]=(x y); echo NOT; declare -p v 2>&1 )
( declare -A m=([k]=K); m[k]=(x y); echo NOT; declare -p m 2>&1 )
( declare -A m=([k]=K); m[k]+=(x y); echo NOT )
echo '=== the subscript is quoted as source, and never evaluated'
( n=(a b c); n[1+]=(x y); echo NOT )
( n=(a b c); n[]=(x y); echo NOT )
( n=(a b c); n[-1]=(x y); echo NOT )
( n=(a b c); n[@]=(x y); echo NOT )
( n=(a b c); n[$((1/0))]=(x y); echo NOT )
( n=(a b c); n[$(echo RAN 1>&2; echo 1)]=(x y); echo NOT )
( n=(a b c); n[1 ]=(x y); echo NOT )
( q=(0); n=(a b c); n[q[0]]=(x y); echo NOT )
echo '=== a nameref is never followed'
( n=(a b c); declare -n r=n; r[1]=(x y); echo NOT; declare -p n 2>&1 )
( n=(a b c); declare -n r='n[1]'; r[0]=(x y); echo NOT; declare -p n 2>&1 )
echo '=== the readonly guard is never reached'
( n=(a b c); readonly n; n[1]=(x y); echo NOT )
echo '=== the words are never expanded'
( f() { echo RAN; echo z; }; n=(a b c); n[1]=($(f)); echo NOT )
echo '=== a declaration builtin says the same, in operand order'
( n=(a b c); declare n[1]=(x y); echo NOT; declare -p n 2>&1 )
( n=(a b c); declare -a n[1]=(x y); echo NOT )
( n=(a b c); declare n[1]=(z) r=(x y); echo NOT; declare -p r 2>&1 )
( n=(a b c); f() { local n[1]=(x y); echo NOT; }; f; echo NOT2 )
echo '=== the rest of the list goes with it'
( n=(a b c); n[1]=(x y); echo NOT1
echo NOT2 )
( n=(a b c); f() { n[1]=(x y); echo NOT1; }; f; echo NOT2 )
echo '=== at the top level an eval confines it, and reports 1'
n=(a b c); { eval 'n[1]=(x y); echo NOT'; echo "s=$?"; echo AFTER; }
a=(1 2); { eval 'a[-9]=x; echo NOT'; echo "s=$?"; echo AFTER; }
echo '=== inside a subshell the same eval does not, and the subshell exits'
( n=(a b c); { eval 'n[1]=(x y); echo NOT'; echo "s=$?"; }; echo AFTER )
echo "s=$?"
( a=(1 2); { eval 'a[-9]=x; echo NOT'; echo "s=$?"; }; echo AFTER )
echo "s=$?"
echo '=== as a command prefix it is the other refusal, and the command runs'
( n=(a b c); v=1 n[1]=(x y) true; echo "s=$?"; declare -p n v 2>&1 )
echo '=== and the ordinary unsubscripted literal is untouched'
( n=(a b c); n=(x y); echo "s=$?"; declare -p n 2>&1 )
( n=(a b c); n+=(x y); echo "s=$?"; declare -p n 2>&1 )
( declare -A m; m=([k]=v); echo "s=$?"; declare -p m 2>&1 )
