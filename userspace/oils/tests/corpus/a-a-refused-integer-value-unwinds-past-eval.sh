# bash has two depths it can raise a non-fatal abort from, and `eval` sits
# between them. A word-expansion error — a bad subscript, a `$((1/0))` — is
# raised from the expander, which `eval` and `.`/`source` are wrapped around,
# so they catch it: the text they were running is abandoned and the caller
# carries on. An arithmetic error while *binding* an integer-attribute value is
# raised from below that, so it unwinds past them and takes the caller with it.
#
# Every builtin that binds such a value raises it from the same place, and no
# amount of nesting slows it down. What does stop it is a subshell — its own
# top level — after which the parent continues as usual.

echo "=== every builtin that binds an -i value unwinds past eval"
a1() { declare -i v=2+; echo NOT-REACHED; }; a1; echo "after1 rc=$?"
a2() { eval 'declare -i v=2+'; echo NOT-REACHED; }; a2; echo "after2 rc=$?"
a3() { declare -i v; eval 'v=2+'; echo NOT-REACHED; }; a3; echo "after3 rc=$?"
a4() { declare -i v; eval 'v+=2+'; echo NOT-REACHED; }; a4; echo "after4 rc=$?"
a5() { declare -i v; eval 'export v=2+'; echo NOT-REACHED; }; a5; echo "after5 rc=$?"
a6() { declare -i v; eval 'readonly v=2+'; echo NOT-REACHED; }; a6; echo "after6 rc=$?"
a7() { declare -i v; eval 'printf -v v "%s" "2+"'; echo NOT-REACHED; }; a7; echo "after7 rc=$?"
a8() { declare -ai v; eval 'v[0]=2+'; echo NOT-REACHED; }; a8; echo "after8 rc=$?"
a9() { eval 'declare -ai v=(2+)'; echo NOT-REACHED; }; a9; echo "after9 rc=$?"
b1() { declare -i v; eval 'read v <<< "2+"'; echo NOT-REACHED; }; b1; echo "afterb1 rc=$?"

echo "=== an expansion error is caught there, for contrast"
c1() { eval 'a[-9]=x'; echo yes; }; c1; echo "afterc1 rc=$?"
c2() { eval 'echo $((1/0))'; echo yes; }; c2; echo "afterc2 rc=$?"
c3() { eval 'echo ${zz[-9]}'; echo yes; }; c3; echo "afterc3 rc=$?"

echo "=== nesting does not slow it down"
d1() { eval 'eval "declare -i v=2+"; echo NOT-REACHED'; echo NOT-REACHED; }
d1; echo "afterd1 rc=$?"

echo "=== a subshell contains it wherever it is raised"
( eval 'declare -i v=2+'; echo NOT-REACHED ); echo "aftere1 rc=$?"
x=$( eval 'declare -i v=2+'; echo NOT-REACHED ); echo "aftere2 rc=$? x=[$x]"
eval 'declare -i v=2+' | cat; echo "aftere3 rc=$?"

echo "=== a loop body goes with it"
f1() { for i in 1 2; do eval 'declare -i v=2+'; echo "NOT-REACHED $i"; done; echo NOT-REACHED; }
f1; echo "afterf1 rc=$?"

echo "=== and a sourced file is no more of a barrier"
printf 'declare -i v=2+\necho NOT-REACHED\n' > src.sh
g1() { . ./src.sh; echo NOT-REACHED; }; g1; echo "afterg1 rc=$?"

echo "=== the line it stops at is the whole parse unit, and the next one runs"
declare -i h1=2+; echo NOT-REACHED
echo reached
