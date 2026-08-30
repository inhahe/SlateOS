# `--` ends option parsing for every builtin, including the ones that take no
# options at all — bash strips it before the builtin sees its words. Exactly
# one is stripped: a second `--` is an ordinary argument, which for the
# builtins that want a number is an error.
echo "=== eval"
eval -- 'echo e1'
eval -- echo e2
eval --; echo "rc=$?"
eval -- ''; echo "rc=$?"
( eval -- -- 'echo nope' ); echo "rc=$?"
( eval - 'echo nope' ); echo "rc=$?"

echo "=== let"
let -- 1+1; echo "rc=$?"
let -- 'x=5'; echo "x=$x"
let --; echo "rc=$?"
( let -- -- 1+1 ); echo "rc=$?"
# A `--` that is part of the expression is untouched.
v=3; let -- --v; echo "v=$v rc=$?"
let --2; echo "rc=$?"

echo "=== shift"
f() { shift -- 2; echo "$*"; }
f a b c
g() { shift --; echo "n=$#"; }
g a b
h() { shift -- -- 2; echo "rc=$? left=$*"; }
h a b c

echo "=== return"
r() { return -- 4; }; r; echo "rc=$?"
r2() { return -- -- 4; }; r2; echo "rc=$?"
r3() { return --; }; false; r3; echo "rc=$?"

echo "=== break and continue"
for i in 1 2 3; do for j in a b; do break -- 2; done; done; echo "after=$i$j"
for i in 1 2 3; do continue -- 1; echo unreachable; done; echo "cont=$i"

echo "=== and the ones that already took it"
command -- echo c1
echo 'echo sourced' > f.sh; source -- ./f.sh; . -- ./f.sh
unset -- v; echo "v=[$v]"
export -- w=1; echo "w=$w"
declare -- d=2; echo "d=$d"
set -- p q; echo "$1$2"
printf -- '%s\n' p1
trap -- 'echo trapped' USR1; echo "rc=$?"
type -- echo >/dev/null; echo "rc=$?"

# `exit --` uses `$?`, like a bare `exit`, so it has to come last.
echo "=== exit"
( exit -- 3 ); echo "rc=$?"
( exit -- -- 3 ); echo "rc=$?"
( false; exit -- ); echo "rc=$?"
