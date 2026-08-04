# `shopt -s localvar_inherit` reverses what `local` normally does. A `local`
# ordinarily starts fresh — no value, no array kind, none of the outer
# binding's attributes — and with the option on it starts as a *copy* of the
# binding it is shadowing, with the flags on the declaration applied on top of
# the inherited ones rather than instead of them.
#
# "The binding it shadows" is just whatever the name reads as at that moment,
# so an outer frame's local, a global and a temporary-environment prefix all
# count equally, and a name that is unset there is inherited as unset. The one
# thing that does not come across is the nameref marking: `-n` is dropped and
# the *value* — the target's name — is kept as a plain string.
#
# The inheritance happens before the declaration's own value is assigned, which
# is why an inherited `-i` converts it: under `local -i v=7`, an inner
# `local v=own` reports `v="0"`.
#
# `declare` inside a function is the same declaration as `local`, so it
# inherits too; `declare -g` names the global and never shadows, so it cannot.

shopt -s localvar_inherit
show() { declare -p "$1" 2>&1 | sed 's/^/    /'; }
inner() { local v; show v; }

echo "### value, array kind and attributes all come across"
for outer in \
    'local -i v=7' \
    'local -r v=ro' \
    'local -x v=ex' \
    'local -u v=abc' \
    'local -l v=ABC' \
    'local -a v=(p q)' \
    'declare -A v=([k]=w)' \
    'local -t v=tr' \
    'local v='
do
    echo "  $outer:"
    eval "f() { $outer; inner; }"
    f
done

echo "### the nameref marking is the exception: dropped, value kept"
n1() { local t=target; local -n v=t; inner; }
n1

echo "### a previous scope is just whatever the name reads as"
echo "  a global:"
declare -i g=9
gf() { local g; show g; }
gf
echo "  the nearest local, not the outermost:"
GN=global
m1() { local GN; show GN; }
m2() { local GN=mid; m1; }
m2
echo "  unset in between is inherited as unset:"
HN=global
h1() { local HN; show HN; }
h2() { local HN=mid; unset HN; h1; }
h2
echo "  a temporary-environment prefix:"
TV=global
t1() { local TV; show TV; }
TV=temp t1
echo "  a name that has never existed:"
z1() { local zznever; show zznever; }
z1

echo "### the declaration's own flags are added to the inherited ones"
x1() { local -x v; show v; }
x2() { local -i v=7; x1; }
x2

echo "### an initialiser is assigned through the inherited attributes"
i1() { local v=own; show v; }
i2() { local -i v=7; i1; }
i2
j1() { local v=3+4; show v; }
j2() { local -i v=1; j1; }
j2
k1() { local v=ABC; show v; }
k2() { local -l v=x; k1; }
k2

echo "### an inherited array kind still cannot be converted"
c1() { local -A v; show v; }
c2() { local -a v=(p q); c1; }
c2
d1() { local -a v; show v; }
d2() { declare -A v=([k]=w); d1; }
d2

echo "### an inherited readonly is a readonly local"
( r1() { local v; v=changed; echo NO-UNREACHABLE; }
  r2() { local -r v=ro; r1; echo NO-UNREACHABLE; }
  r2; echo NO-UNREACHABLE )
echo "  rc=$?"

echo "### a readonly global is refused before any of this"
declare -r RG=frozen
q1() { local RG; show RG; }
q1; echo "  rc=$?"

echo "### declare inside a function inherits; declare -g does not shadow"
e1() { declare v; show v; }
e2() { local -i v=7; e1; }
e2
p1() { declare -g v=global-by-p1; show v; }
p2() { local -i v=7; p1; show v; }
p2
show v

echo "### a second local in the same frame is not a new shadow"
s1() { local -i n=5; local n; show n; }
s1

echo "### and with the option off again"
shopt -u localvar_inherit
x2
e2
gf
