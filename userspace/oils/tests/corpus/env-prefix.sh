# A command prefix (`FOO=bar cmd`) builds a *temporary environment*, which holds
# nothing but plain `NAME=value` strings. So a subscript has nowhere to go — the
# word is refused as a name, quoted back exactly as it was written and dropped —
# and an array literal has to be re-serialised into one parenthesised value,
# which is what the child actually sees.
#
# bash finishes with each assignment before it looks at the next, so the
# refusals and the `set -x` lines interleave in source order; and neither
# refusal is fatal, unlike the same rejection in a *bare* assignment.
e() { sed 's/^.*: line [0-9]*: //'; }
p() { echo "--- $1"; ( eval "$1"; echo "rc=$?" ) 2>&1 | e; }
show='printf "[%s]\n" "$a"'

echo "=== a subscripted name is no name at all"
p 'q=(1); q[0]=Z true; echo "q=${q[0]}"'
p 'arr[0]+=Z true'
p 'arr["a b"]=Z true'
# Quoted back unexpanded: no division happens, and the value's substitution
# never runs.
p 'arr[$((1/0))]=Z true'
p 'a[0]=$(echo SIDE >&2) true'
# One message per offender, in source order; the valid neighbours still arrive.
p 'a[0]=Z b[1]=Y v=ok sh -c '"'"'echo $v'"'"''
# The word is dropped entirely — not in the child, not in a called function.
p 'f() { echo "in-f a=[${a[*]}]"; }; a[0]=Z f'
p 'a[0]=Z sh -c '"'"'echo "child a=[$a]"'"'"''
# It outranks the readonly refusal.
p 'q=(1); readonly q; q[0]=Z true'
# …and reaches the shell's stderr, which the command's own redirect cannot
# silence.
p 'a[0]=Z true 2>/dev/null; echo done'

echo "=== an array literal keeps its parentheses"
p "a=(1  2) sh -c '\$show'"
p "x='p  q'; a=(\$x) sh -c '\$show'"
p "x='p  q'; a=(\"\$x\") sh -c '\$show'"
p "a=(\"q r\" s) sh -c '\$show'"
p "a=(*) sh -c '\$show'"
p "a=(\$(echo z)) sh -c '\$show'"
p "a=(\"\" b) sh -c '\$show'"
p "a=(  ) sh -c '\$show'"
p "a=(\$nosuch) sh -c '\$show'"
p "i=2; a=([\$i]=p) sh -c '\$show'"
p "a=([1+1]=p) sh -c '\$show'"
p "a+=(1 2) sh -c '\$show'"
# The variable itself is never set.
p 'a=(1 2) true; echo "[${a[*]}]"; declare -p a'

echo "=== one assignment at a time"
p 'readonly r=1; v=1 r=2 w=2 sh -c '"'"'echo "[$v][$w]"'"'"''
# A readonly name is refused before its value is expanded, so the division in it
# never happens.
p 'readonly r=1; r=$((1/0)) true'
# A word-expansion error in a value *is* fatal to the command, and stops the
# loop where it happened: the assignment after it is never looked at.
p 'x=$((1/0)) a[0]=Z true; echo after'
p 'a[0]=Z x=$((1/0)) true; echo after'
p 'readonly r=1; r=2 x=$((1/0)) true; echo after'
p 'set -x; readonly r=1; v=1 r=2 w=2 true'
p 'set -x; a[0]=Z v=1 true'

echo "=== …and it is expanded in assignment context"
export HOME=/hh
p "v=~/a:~/b sh -c 'printf \"[%s]\n\" \"\$v\"'"
# The element's leading tilde is not leading in the *value* — only the `(` is —
# so only the after-a-colon rule can still fire.
p "a=(~/a ~/b) sh -c '\$show'"
p "a=(x:~/a) sh -c '\$show'"
p "a=([k]=~/a) sh -c '\$show'"

echo "=== done"
