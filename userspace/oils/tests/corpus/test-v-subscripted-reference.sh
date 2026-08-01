# `-v 'name[sub]'` addresses one element, and the subscript is live shell syntax
# rather than the literal text between the brackets: arithmetic for an indexed
# array (so `a[i]`, `a[1+1]` and `a[i=5]` all work, the last one assigning), an
# expanded string key for an associative one. A negative index counts back from
# one past the highest index the name holds, and reaching past the start is a
# "bad array subscript" — reported even for a name that is not set at all, which
# is also evaluated first. A scalar is a one-element array at index 0.
#
# What counts as a reference at all is bash's `valid_array_reference`: the base
# has to be an identifier, the bracket must not open the token, and the `]` that
# closes it has to be the last character. `a[0]x`, `a[0][0]`, `[0]` and `m[k`
# are ordinary — and necessarily unset — names, and so is the syntactically
# empty `m[]`, which is what keeps it apart from a subscript that merely expands
# to nothing.
n() { sed 's/^.*: line [0-9]*: //'; }
q() { ( eval "$1"; echo "rc=$?" ) 2>&1 | n; }

echo "=== an indexed subscript is arithmetic, and can have side effects"
q 'a=(p q); [ -v "a[0]" ]; echo "0=$?"; [ -v "a[1]" ]; echo "1=$?"; [ -v "a[2]" ]; echo "2=$?"'
q 'a=(p q); i=1; [ -v "a[i]" ]; echo "i=$?"'
q 'a=(p q); [ -v "a[1+1]" ]; echo "x=$?"; [ -v "a[zz]" ]; echo "z=$?"'
q 'a=(p); [ -v "a[i=5]" ]; echo "a=$?"; echo "i=${i-unset}"'
q 'a=(p); [ -v "a[$(echo x >&2; echo 0)]" ]; echo "a=$?"'
q '[ -v "nosuch[$(echo x >&2; echo 0)]" ]; echo "n=$?"'
q 'a=(p); [ -v "a[x y]" ]; echo "a=$?"; echo more'

echo "=== a negative index counts back from the highest one held"
q 'a=(p q); [ -v "a[-1]" ]; echo "m1=$?"; [ -v "a[-2]" ]; echo "m2=$?"; [ -v "a[-3]" ]; echo "m3=$?"'
q 'a=([5]=x); [ -v "a[-1]" ]; echo "m1=$?"; [ -v "a[-6]" ]; echo "m6=$?"; [ -v "a[-7]" ]; echo "m7=$?"'
q 'a=(); [ -v "a[-1]" ]; echo "m1=$?"'
q 'declare -a k; [ -v "k[-1]" ]; echo "m1=$?"'
q '[ -v "nosuch[-1]" ]; echo "m1=$?"'
q 'v=x; [ -v "v[-1]" ]; echo "m1=$?"'

echo "=== a scalar is a one-element array at index 0"
q 'v=x; [ -v "v[0]" ]; echo "0=$?"; [ -v "v[1]" ]; echo "1=$?"'
q 'v=; [ -v "v[0]" ]; echo "0=$?"'
q 'v=x; [ -v "v[1+  -1]" ]; echo "x=$?"'
q '[ -v "nosuch[0]" ]; echo "n=$?"'

echo "=== an associative subscript is an expanded string key"
q 'declare -A m; m[k]=v; [ -v "m[k]" ]; echo "k=$?"; [ -v "m[j]" ]; echo "j=$?"'
q 'declare -A m; m[k]=v; k=zz; [ -v "m[k]" ]; echo "k=$?"'
q 'declare -A m; m[zz]=v; k=zz; [ -v "m[$k]" ]; echo "k=$?"'
q 'declare -A m; m["a b"]=v; [ -v "m[a b]" ]; echo "k=$?"'
q 'declare -A m; m[k]=v; [ -v "m[$(echo x >&2; echo k)]" ]; echo "k=$?"'
q 'declare -A m; m[k]=v; [ -v "m[-1]" ]; echo "m1=$?"'
q 'declare -A m; m[-1]=v; [ -v "m[-1]" ]; echo "m1=$?"'
q 'declare -A m; m[k]=v; e=; [ -v "m[$e]" ]; echo "e=$?"'

echo "=== the all-elements subscripts ask whether there is any element"
q 'a=(p q); [ -v "a[@]" ]; echo "at=$?"; [ -v "a[*]" ]; echo "st=$?"'
q 'a=([2]=x); [ -v "a[@]" ]; echo "at=$?"'
q 'a=(); [ -v "a[@]" ]; echo "at=$?"'
q 'v=x; [ -v "v[@]" ]; echo "at=$?"; [ -v "v[*]" ]; echo "st=$?"'
q 'v=; [ -v "v[@]" ]; echo "at=$?"'
q '[ -v "nosuch[@]" ]; echo "at=$?"'
q 'declare -A m; m[k]=v; [ -v "m[@]" ]; echo "at=$?"'
q 'declare -A m; m[@]=v; [ -v "m[@]" ]; echo "at=$?"'

echo "=== a nameref asks about its target's element"
q 'a=(p q); declare -n r=a; [ -v "r[1]" ]; echo "1=$?"; [ -v "r[5]" ]; echo "5=$?"'
q 'declare -A m; m[k]=v; declare -n r=m; [ -v "r[k]" ]; echo "k=$?"'

echo "=== and what is not a reference is just an unset name"
q 'a=(p); [ -v "a[0]x" ]; echo "x=$?"'
q 'a=(p); [ -v "a[0][0]" ]; echo "x=$?"'
q 'a=(p); [ -v "[0]" ]; echo "x=$?"'
q 'declare -A m; m[k]=v; [ -v "m[k" ]; echo "x=$?"'
q 'declare -A m; m[k]=v; [ -v "m[]" ]; echo "x=$?"'
q 'a=(p); [[ -v "a[0]" ]]; echo "x=$?"; [[ -v a[0] ]]; echo "y=$?"'
q 'a=(p); test -v "a[0]"; echo "x=$?"'
echo "=== done"
