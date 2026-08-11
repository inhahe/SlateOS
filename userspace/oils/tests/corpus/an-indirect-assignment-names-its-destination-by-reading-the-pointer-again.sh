# `${!ref[…]:=word}` does not store through the name the reference already
# resolved to. bash re-reads the pointer in `parameter_brace_expand_rhs`
# (subst.c:7841-7862) — after the right-hand side has expanded — and stores
# through whatever *that* names, refusing anything that read does not spell as a
# plain identifier. So the subscript runs twice; `$((i++, 0))` counts that in the
# shell itself while always yielding the same key.
n=(ax by cz)
one=('')
declare -a ea=()
declare -A am=()
sv=SVAL
empty=''
declare -A p=([k0]='n[@]')
declare -A pe=([k0]='ea[@]')
declare -A p1=([k0]='one[@]')
declare -A pm=([k0]='am[@]')
declare -A el=([k0]='n[9]')
declare -A s=([k0]=sv)
declare -A se=([k0]=empty)
declare -A u=([k0]=nosuch)
declare -A mt=([k0]='')
declare -A bad=([k0]='1abc')
r() {
  i=0
  printf '%-30s ' "$1"
  { eval "printf '<%s>' $1"; } 2>&1
  printf ' st=%s cnt=%s\n' "$?" "$i"
  unset nosuch
  empty=''
  ea=()
  am=()
  one=('')
}
echo "--- how many times the pointer is read, and what it ends in"
for v in p pe p1 pm el s se u mt bad nope; do
  r "\${!$v[k\$((i++, 0))]:=D}"
  r "\${!$v[k\$((i++, 0))]=D}"
done
echo "--- the destination is the second reading's, not the first's"
# The same counter, but now handing out a *different* name each time.
i=0
e1=(nosuch other)
r '${!e1[i++]:=D}'
echo "other=<${other-U}> nosuch=<${nosuch-U}>"
e2=(nosuch '1bad')
r '${!e2[i++]:=D}'
e3=(nosuch3)
r '${!e3[i++]:=D}'
e4=(nosuch4 'n[9]')
r '${!e4[i++]:=D}'
e5=(sv other2)
r '${!e5[i++]:=D}'
echo "other2=<${other2-U}>"
echo "--- and the reading comes after the right-hand side, not before"
o() { printf '%s ' "$1" >&2; }
h() { o f; echo k0; }
b() { printf '%-30s ' "$1"; eval "printf '<%s>' $1" 2>&1; echo; }
b '${!el[$(h)]:=$(o D; echo dv)}'
unset nosuch
b '${!u[$(h)]:=$(o D; echo dv)}'
echo "--- a nameref is never looked through, so it never re-reads"
declare -n gA='ea[@]'
declare -n gE='n[9]'
declare -n gU=nosuch2
b '${!gA:=$(o D; echo dv)}'
b '${!gE:=$(o D; echo dv)}'
b '${!gU:=$(o D; echo dv)}'
echo "nosuch2=<${nosuch2-UNSET}>"
echo TAIL
