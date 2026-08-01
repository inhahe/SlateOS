# `eval` and `.`/`source` read their text in the *calling* context, so the
# non-local flow it raises is the caller's. A `return` inside an `eval` returns
# from the function the `eval` was written in, taking the rest of that body with
# it, and a `break` or `continue` — from either — ends or restarts the loop the
# command itself stands in.
#
# The one that stops is a `return` in a **sourced file**: that is what ending
# the file means, so it is caught there and the caller carries on with the
# file's status. A function, unlike either of these, *is* a boundary for a
# loop flow: `f() { eval break; }` called from a loop only warns.
e() { sed 's/^.*: line [0-9]*: //'; }
p() { echo "--- $1"; ( eval "$1"; echo "rc=$?" ) 2>&1 | e; }

echo "=== a return inside eval returns from the enclosing function"
g() { eval "echo a; return 3; echo b"; echo after; }
g; echo "g=$?"
h() { eval "return"; echo after; }
h; echo "h=$?"
i() { false; eval "return"; echo after; }
i; echo "i=$?"
k() { eval "eval \"return 4\"; echo mid"; echo after; }
k; echo "k=$?"

echo "=== and out of a subshell inside one"
j() { ( eval "echo one; return 2; echo two"; echo sub ); echo "j-sub=$?"; }
j; echo "j=$?"

echo "=== with nothing to return from it is the usual error"
eval "return 5"
echo "top=$?"
echo alive

echo "=== a sourced file catches its own return"
mkdir -p lib
printf 'echo s1\neval "return 6"\necho s3\n' > lib/r.sh
. lib/r.sh
echo "src=$?"
printf 'echo t1\nreturn 8\necho t3\n' > lib/t.sh
m() { . lib/t.sh; echo "in m"; }
m; echo "m=$?"

echo "=== but break and continue cross both"
printf 'echo s1\nbreak\necho s3\n' > lib/b.sh
printf 'echo c1\ncontinue\necho c3\n' > lib/c.sh
for x in 1 2 3; do . lib/b.sh; echo "body $x"; done; echo "brk=$?"
for x in 1 2 3; do . lib/c.sh; echo "body $x"; done; echo "cnt=$?"
for x in 1 2 3; do eval "break"; echo "body $x"; done; echo "ebrk=$?"
for x in 1 2 3; do eval "continue"; echo "body $x"; done; echo "ecnt=$?"
for x in 1 2; do for y in a b; do eval "break 2"; echo "in $x$y"; done; done; echo "brk2=$?"

echo "=== a function is a boundary for one, though"
f() { eval "break"; echo "in f"; }
for x in 1 2; do f; echo "body $x"; done; echo "fbrk=$?"
eval "break"; echo "nb=$?"

echo "=== and a return inside eval inside a loop leaves the function"
n2() { for x in 1 2; do eval "return 7"; echo body; done; echo tail; }
n2; echo "n2=$?"
echo "=== done"
