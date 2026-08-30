# `${x@A}` renders the element the reference named, not whatever the variable's
# own storage cell holds. On an array that is obvious. On a *scalar* it looks
# like the same thing and is not: element 0 is the scalar, and every other
# subscript names nothing — so `${s[5]@A}` is the valueless form, and
# `${s[0]@A}` is `s='hi'`.
#
# Valueless does not mean empty. bash prints the bare declaration whenever there
# is an attribute to recreate, which is the array form's rule (`declare -a n` for
# an unset element) applied to a scalar; a name carrying no attributes has
# nothing to recreate and gives the empty string instead. Which of the two ways
# the value went missing — never assigned, or assigned and then subscripted past
# — makes no difference.
#
# The variables the shell computes are the other half of the same point, since
# they are the case where there is nothing *but* the element: `SECONDS` has no
# storage cell to fall back on, so a rendering that consults one instead of the
# read it already made has nothing to print. Their attributes likewise live with
# the shell rather than with the variable, so `${SECONDS@a}` has to agree with
# `declare -p SECONDS` about the `-i` that neither `declare -i` nor anything
# else ever set — and it keeps agreeing after an assignment, because the write
# is taken by the shell rather than making a binding that could shadow it.
# Only `unset` takes the name out of the shell's hands.
#
# Their *values* are host state — a pid, a clock, a path — so only the shapes
# are printed here, never the values.

echo "=== a scalar's element 0 is the scalar, and nothing else is"
s=hi
echo "[${s@A}] [${s[0]@A}] [${s[5]@A}]"
echo "[${s@a}] [${s[0]@a}] [${s[5]@a}]"
# The same for the attributed kinds, which have a declaration to fall back on.
declare -i iv=7
echo "[${iv@A}] [${iv[0]@A}] [${iv[5]@A}]"
declare -x xv=q
echo "[${xv@A}] [${xv[0]@A}] [${xv[5]@A}]"
declare -r rv=z
echo "[${rv@A}] [${rv[5]@A}]"
declare -u uv=abc
echo "[${uv@A}] [${uv[5]@A}]"

echo "=== …and the two ways a value goes missing render alike"
# Never assigned on the left, assigned and subscripted past on the right.
declare -i d1; declare -i d2=4
echo "[${d1@A}] [${d2[9]@A}]"
declare -x e1; declare -x e2=4
echo "[${e1@A}] [${e2[9]@A}]"
# With no attribute there is nothing to recreate either way.
declare t1; t2=v
echo "[${t1@A}] [${t2[9]@A}] [${nope@A}] [${nope[9]@A}]"

echo "=== an array renders its element, and the bare form when there is none"
declare -a n=(x y)
declare -A m=([k]=v)
echo "[${n@A}] [${n[1]@A}] [${n[9]@A}]"
echo "[${m@A}] [${m[k]@A}] [${m[no]@A}]"
declare -ar ro=(1)
echo "[${ro@A}] [${ro[9]@A}] [${ro@a}]"

echo "=== the shell's own scalars have attributes but no cell"
# `@a` must agree with `declare -p`, which is where these attributes are kept —
# nothing puts `SECONDS` in the integer set.
# `${!a}` cannot ask this — a transform is not part of a name, so an indirection
# through one is an `invalid variable name` and fatal — so both halves go
# through `eval`, and `@A`'s comes back with the value cut out so that only the
# shape is compared.
for v in SECONDS RANDOM SRANDOM BASHPID HISTCMD PPID UID EUID PWD OLDPWD SHLVL LINENO EPOCHSECONDS EPOCHREALTIME; do
  printf '%-14s a=[%s] A=[%s]\n' "$v" \
    "$(eval "printf '%s' \"\${$v@a}\"")" \
    "$(eval "printf '%s' \"\${$v@A}\"" | sed "s/=.*/=…/")"
done

echo "=== …and assigning to one does not make it an ordinary variable"
# The write is taken by the shell rather than making a binding, so there is
# nothing to shadow the table with and the letters stay exactly as they were.
( RANDOM=7; echo "[${RANDOM@a}]"; declare -p RANDOM | sed 's/=.*/=…/' )
( SECONDS=0; echo "[${SECONDS@a}]" )
# A readonly one refuses the write outright — loudly, and fatally to the parse
# unit — and so keeps its letters for the same reason.
( PPID=1; echo unreachable ) 2>&1
( echo "[${PPID@a}] [${UID@a}]" )

echo "=== unset takes the attributes away with the name"
( unset SECONDS; echo "[${SECONDS@a}] [${SECONDS@A}] rc=$?" )
( declare -i q=1; unset q; echo "[${q@a}] [${q@A}] rc=$?" )
