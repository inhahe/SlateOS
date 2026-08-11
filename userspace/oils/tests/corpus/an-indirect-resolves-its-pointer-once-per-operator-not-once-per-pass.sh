# bash resolves a `${!ref[…]}` pointer once (`parameter_brace_expand_indir`) and
# a second time only for the operators that reach `get_var_and_type`. The
# pointer's subscript is a word, so the count is observable — and an arithmetic
# subscript counts it in the shell itself: `$((i++, 0))` bumps `i` on every
# evaluation and always yields the same key, so the reference is the same one
# each time. It runs once, twice, or — where the reference is refused outright —
# once and no more.
sv=SVAL
empty=''
n=(ax by cz)
declare -a ea=()
one=('')
declare -A s=([k0]=sv)
declare -A se=([k0]=empty)
declare -A u=([k0]=nosuch)
declare -A ab=([k0]='n[9]')
declare -A p=([k0]='n[@]')
declare -A pe=([k0]='ea[@]')
declare -A p1=([k0]='one[@]')
r() {
  i=0
  printf '%-30s ' "$1"
  { eval "printf '<%s>' $1"; } 2>&1
  printf ' cnt=%s\n' "$i"
}
for v in s se u ab p pe p1 nope; do
  echo "=== $v"
  r "\${!$v[k\$((i++, 0))]}"
  r "\${!$v[k\$((i++, 0))]#a}"
  r "\${!$v[k\$((i++, 0))]#}"
  r "\${!$v[k\$((i++, 0))]:1}"
  r "\${!$v[k\$((i++, 0))]//a/z}"
  r "\${!$v[k\$((i++, 0))]^^}"
  r "\${!$v[k\$((i++, 0))]@a}"
  r "\${!$v[k\$((i++, 0))]:-D}"
done
echo "--- an empty raw pattern is not the same as one that expands to nothing"
r '${!s[k$((i++, 0))]#""}'
r '${!s[k$((i++, 0))]#$(:)}'
echo "--- the second resolution comes before the operator's own operator"
o() { printf '%s ' "$1" >&2; }
g() { o f; echo k0; }
b() { printf '%-30s ' "$1"; eval "printf '<%s>' $1"; echo; }
b '${!s[$(g)]#$(o P; echo a)}'
b '${!s[$(g)]:$(o A; echo 1)}'
b '${!p[$(g)]^^$(o C; echo a)}'
echo "--- one resolution per part, never shared with a neighbour"
declare -A t=([k0]=sv [j0]=empty)
b '"${!t[$(g)]}${!t[$(o j; echo j0)]}"'
echo TAIL
