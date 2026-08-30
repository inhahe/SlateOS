# Writing `a[i]` to a name that holds a plain scalar does not throw the scalar
# away: it becomes element 0 of the array the write brings into being, exactly
# as `declare -a` would carry it. That is true of every write that names an
# element rather than a variable — a plain assignment, `${a[i]:=v}`, `printf -v`,
# `read`, a nameref pointing at an element — and it carries the name's
# attributes along with the value.
#
# Two consequences worth stating separately, because they are what makes the
# rule visible rather than merely tidy:
#
#   * a *negative* subscript counts back over the element that is about to
#     exist, so `t=v; t[-1]=w` replaces the `v` rather than reaching past the
#     start. A **read** of the same name still sees no array and says so, which
#     is why `${t[-1]:=w}` reports one `bad array subscript` — from the read —
#     and then completes the store anyway.
#
#   * the carry happens only once the store is certain. A subscript that names
#     nowhere leaves the scalar a scalar, and so does a refusal.
#
# An empty scalar migrates too: it is a value like any other, and the array ends
# up *valued* because of it. A name holding nothing at all has nothing to carry,
# so the element lands alone.

echo '=== a plain element assignment carries it'
( t=v; t[1]=w; declare -p t )
( t=; t[1]=w; declare -p t )
( unset t; t[1]=w; declare -p t )

echo '=== and so does every other write that names an element'
( t=v; echo "[${t[1]:=w}]"; declare -p t )
( t=v; printf -v 't[1]' z; declare -p t )
( t=v; read 't[1]' <<< z; declare -p t )
( t=v; declare -n r=t[1]; r=z; declare -p t )

echo '=== the attributes come with it'
( export t=v; echo "[${t[1]:=w}]"; declare -p t )
( declare -i t=5; echo "[${t[1]:=7}]"; declare -p t )
( declare -u t=v; t[1]=w; declare -p t )

echo '=== a negative subscript counts back over the element about to exist'
( t=v; t[-1]=w; declare -p t )
( t=v; printf -v 't[-1]' z; echo "rc=$?"; declare -p t )
( t=v; read 't[-1]' <<< z; echo "rc=$?"; declare -p t )
( t=v; declare -n r=t[-1]; r=z; declare -p t )

echo '=== …while the read that comes first still sees no array'
( t=v; echo "[${t[-1]:=w}]"; declare -p t )
( t=v; echo "[${t[-1]}]" )

echo '=== one step further back is nowhere, and the scalar stays a scalar'
( t=v; printf -v 't[-2]' z; echo "rc=$?"; declare -p t )
( t=v; read 't[-2]' <<< z; echo "rc=$?"; declare -p t )
( t=v; declare -n r=t[-2]; r=z; echo "rc=$?"; declare -p t )

echo '=== a refusal does not carry it either'
( declare -r t=v; printf -v 't[1]' z; echo "rc=$?"; declare -p t )
( declare -r t=v; read 't[1]' <<< z; echo "rc=$?"; declare -p t )

echo '=== an element that is already there is read, not written'
( t=v; echo "[${t[0]:=w}]"; declare -p t )
( t=v; echo "[${t[k]:=w}]"; declare -p t )

echo '=== an existing array is left to its own bound'
( a=(x y); a[-1]=w; declare -p a )
( a=(x y); a[-3]=w; echo "rc=$?"; declare -p a )

echo still here
