# `-v name` on an array is not asking whether the array exists. bash reads an
# unsubscripted name as `name[0]`, so an array answers yes only when it holds an
# element 0 — the empty `a=()`, the bare `declare -a q`, one built from `[1]=`
# upwards and one whose element 0 was unset all say no, however much else they
# hold. An associative array is the same question about a literal `0` key, which
# is why `-v BASH_ALIASES` is false while `-v BASH_SOURCE` is true. A scalar is
# its own element 0 and answers yes, and a nameref asks about its target.
#
# The indirection `${!ptr[i]}` asks a different question of its pointer — only
# whether there is something to indirect through — so an array with no element 0
# is still a perfectly good one to point at.
n() { sed 's/^.*: line [0-9]*: //'; }
q() { ( eval "$1"; echo "rc=$?" ) 2>&1 | n; }

echo "=== an array answers for its element 0"
q 'a=(p q); [ -v a ]; echo "a=$?"'
q 'a=(""); [ -v a ]; echo "a=$?"'
q 'a=(); [ -v a ]; echo "a=$?"'
q 'a=([0]=x); [ -v a ]; echo "a=$?"'
q 'a=([1]=x); [ -v a ]; echo "a=$?"'
q 'a=([1]=x [0]=y); [ -v a ]; echo "a=$?"'
q 'a=(p); unset "a[0]"; [ -v a ]; echo "a=$?"'
q 'a=(p q); unset "a[1]"; [ -v a ]; echo "a=$?"'
q 'declare -a k; [ -v k ]; echo "k=$?"'
q 'declare -a k; k[3]=z; [ -v k ]; echo "k=$?"'
q 'a=(); [[ -v a ]]; echo "a=$?"'
q 'a=(); test -v a; echo "a=$?"'

echo "=== and an associative array for a literal 0 key"
q 'declare -A m; [ -v m ]; echo "m=$?"'
q 'declare -A m; m[k]=v; [ -v m ]; echo "m=$?"'
q 'declare -A m; m[0]=v; [ -v m ]; echo "m=$?"'
q 'declare -A m; m[0]=v; unset "m[0]"; [ -v m ]; echo "m=$?"'

echo "=== a scalar is its own element 0, and a bare declaration has none"
q 'v=x; [ -v v ]; echo "v=$?"'
q 'v=; [ -v v ]; echo "v=$?"'
q '[ -v nosuch ]; echo "n=$?"'
q 'export E; [ -v E ]; echo "E=$?"'
q 'readonly R; [ -v R ]; echo "R=$?"'
q 'declare -i d; [ -v d ]; echo "d=$?"'

echo "=== a nameref asks the same question of its target"
q 'a=(p q); declare -n r=a; [ -v r ]; echo "r=$?"'
q 'a=(); declare -n r=a; [ -v r ]; echo "r=$?"'
q 'v=x; declare -n r=v; [ -v r ]; echo "r=$?"'
q 'declare -n r=nosuch; [ -v r ]; echo "r=$?"'

echo "=== the shell's own arrays answer by the same rule"
q '[ -v BASH_SOURCE ]; echo "src=$?"'
q '[ -v BASH_VERSINFO ]; echo "vi=$?"'
q '[ -v PIPESTATUS ]; echo "ps=$?"'
q '[ -v GROUPS ]; echo "gr=$?"'
q '[ -v DIRSTACK ]; echo "ds=$?"'
q '[ -v BASH_ALIASES ]; echo "al=$?"'
q '[ -v COMP_WORDS ]; echo "cw=$?"'
q 'BASH_REMATCH=(); [ -v BASH_REMATCH ]; echo "re=$?"'
q '[[ abc =~ b ]]; [ -v BASH_REMATCH ]; echo "re=$?"'

echo "=== but an indirection only needs something to point at"
q 'a=(p q); r=a; echo "[${!r[0]}]"'
q 'a=(); r=a; echo "[${!r[0]}]"'
q 'declare -a k; r=k; echo "[${!r[0]}]"'
q 'a=(p q); r=a; echo "[${!r[1]}]"'
echo "=== done"
