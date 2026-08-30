# What makes a word an assignment is decided at its *name*, not by hunting for
# the first `[` or `=` anywhere in it. Both characters are ordinary text once
# the value has begun, and a subscript is arithmetic, so it may contain an `=`
# of its own. The cases below are the ones where the two readings disagree.

echo "=== an = inside the subscript is not the operator"
# The subscript is arithmetic, so `x=3` assigns as a side effect and the
# element lands at index 3.
a[x=3]=1; declare -p a x
unset a x

echo "=== ... including the comparison and compound-assignment spellings"
a[x==3]=1; declare -p a          # `x==3` is a comparison: false, so index 0
unset a
b[y+=1]=1; declare -p b y        # y was unset: 0+1 = 1
unset b y

echo "=== a bracket in the *value* is just text"
# `foo=a[b` looks like an unfinished subscript only if the scan starts from the
# wrong end of the word.
foo=a[b; declare -p foo
bar+=a[b; declare -p bar
baz=a[b]; declare -p baz
qux=x=y; declare -p qux          # and so is a second =
unset foo bar baz qux

echo "=== a real subscript still works, in all its spellings"
declare -A m
k=q; m[$k]=v; declare -p m       # subscript spanning segments
declare -A h; h[ x ]=v; declare -p h
n[b[0]]=9; declare -p n          # nested brackets
i=3; n[i]=8; declare -p n
unset m h n i k

echo "=== an empty subscript is recognised, then refused"
# bash reads `a[]=1` as an assignment and rejects it at run time, so the rest
# of the parse unit goes with it — quite unlike "command not found".
a[]=1; echo "  NOT REACHED"
echo "  next unit runs, and the array is untouched"
declare -p a 2>&1
declare -A e; e[]=1; echo "  NOT REACHED"
echo "  ... and the same for an associative array"
declare "a[]=1"; echo "  as a declare operand it only fails the builtin: $?"

echo "=== but an empty *expansion* is a perfectly good index"
z=""
a[$z]=1; a[""]=2; declare -p a
