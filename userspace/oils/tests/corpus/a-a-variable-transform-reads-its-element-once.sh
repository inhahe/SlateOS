# `${x@a}` and `${x@A}` answer from the *variable* — its attributes, and a
# `declare` statement that would recreate it — so neither has any use for the
# parameter's value. bash reads it anyway, and how many times it reads is
# visible through a side-effecting subscript, which is what this counts.
#
# The read is not idle. It is what `set -u` faults on — asking after the
# variable is no excuse for an unset one, a name declared but never assigned
# included — and, for `@A` on an array, it is the element the statement renders.
# While it succeeds those are two uses of one read, so the subscript runs once.
#
# When it comes back with nothing, bash asks again — the whole reference over,
# for either operator, so a subscript that names nothing runs *twice* and a bad
# one is reported twice. `set -u` is the one thing that cuts the second ask off,
# because the fault happens between them. No other operator does this: `@Q` on
# the same missing element reads once.
#
# Reached through an indirection the two uses part company: `${!r@A}` on a
# nameref is handed the *name* the reference holds, which is not the element it
# has to render, so it reads the element itself. That is still one read of the
# subscript, because the handed-over value cost none.

echo "=== the subscript is evaluated once when it names an element"
declare -A m=([k]=v)
declare -a n=(x y)

set -u
echo "[A u=on ] ${m[$(echo eval >&2; echo k)]@A}"
echo "[a u=on ] ${m[$(echo eval >&2; echo k)]@a}"
echo "[A idx  ] ${n[$(echo eval >&2; echo 1)]@A}"
set +u
echo "[A u=off] ${m[$(echo eval >&2; echo k)]@A}"
echo "[a u=off] ${m[$(echo eval >&2; echo k)]@a}"
echo "[A idx  ] ${n[$(echo eval >&2; echo 1)]@A}"

echo "=== …and twice when it names nothing, for either operator"
# The element is unset, which `@A` renders as the bare declaration and `@a`
# ignores entirely — and both go back for a second look all the same.
echo "[A miss ] ${m[$(echo eval >&2; echo absent)]@A}"
echo "[a miss ] ${m[$(echo eval >&2; echo absent)]@a}"
echo "[A oob  ] ${n[$(echo eval >&2; echo 9)]@A}"
echo "[a oob  ] ${n[$(echo eval >&2; echo 9)]@a}"
# A whole variable that does not exist is nothing in the same way.
echo "[A nada ] ${nada[$(echo eval >&2; echo x)]@A}"
# No other operator asks twice, so the second ask belongs to these two.
echo "[Q miss ] ${m[$(echo eval >&2; echo absent)]@Q}"
echo "[- miss ] ${m[$(echo eval >&2; echo absent)]-D}"

echo "=== the second ask is the whole reference again, complaint and all"
# A bad subscript is reported once per ask, so it is reported twice here and
# once for any other operator. Neither is fatal on its own.
echo "[A bad  ] ${n[-9]@A}" 2>&1
echo "[a bad  ] ${n[-9]@a}" 2>&1
echo "[Q bad  ] ${n[-9]@Q}" 2>&1

echo "=== under set -u the fault comes between the two asks, so there is one"
( set -u; echo "[${m[$(echo eval >&2; echo absent)]@A}]"; echo unreachable ) 2>&1
( set -u; echo "[${m[$(echo eval >&2; echo absent)]@a}]"; echo unreachable ) 2>&1
# A whole variable that is unset faults the same way, with no subscript to
# evaluate at all.
( set -u; echo "[${nope@A}]"; echo unreachable ) 2>&1
( set -u; echo "[${nope@a}]"; echo unreachable ) 2>&1
# A name declared but never assigned is unset for this purpose, though
# `declare -p` reports it and `@A` off `set -u` recreates it.
( set -u; declare -i dd; echo "[${dd@A}]"; echo unreachable ) 2>&1
( declare -i dd; echo "[${dd@A}] [${dd@a}]" )

echo "=== what the one read is used for"
echo "elem   [${m[k]@A}] [${n[1]@A}]"
echo "unset  [${m[q]@A}] [${n[9]@A}]"
echo "plain  [${n@A}] [${n@a}]"
s=hi; declare -x xx; declare t
echo "scalar [${s@A}] [${s@a}] [${xx@A}] [${t@A}]"

echo "=== an indirection is handed a value that is not the element"
declare -n r=n
declare -n rs=s
declare -n rel=n[1]
declare -n rnil
p=n; ps=s
echo "nameref     [${r@A}] [${r@a}]"
echo "namerefs    [${rs@A}]"
echo "to-element  [${rel@A}] [${rel@a}]"
echo "to-nothing  [${rnil@A}]"
echo "indirect    [${!p@A}] [${!p@a}] [${!ps@A}]"
# `${!r}` on a nameref is the name `r` holds, so the operand handed over is
# `n` — a name, not `n`'s first element — and `@A` still recreates `n`.
echo "indirect-nr [${!r@A}] [${!r@a}]"

echo "=== the read follows the nameref, so the subscript is the target's"
declare -A tgt=([kk]=vv)
declare -n rt=tgt
echo "[${rt[$(echo eval >&2; echo kk)]@A}]"
( set -u; echo "[${rt[$(echo eval >&2; echo nope)]@A}]"; echo unreachable ) 2>&1
