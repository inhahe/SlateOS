# `INDEX_ERROR()` (arrayfunc.c:1456) picks the name a bad subscript is
# reported against, and it picks it off the *lookup*, not off the walk:
#
#      if (var) err_badarraysub (var->name);
#      else { t[-1] = '\0'; err_badarraysub (s); t[-1] = '['; }
#
# `var` is what `array_variable_part` → `find_variable` handed back. A nameref
# is followed inside that lookup, so a chain that lands on a name which *is*
# there blames the name it landed on — `zz=(a b); declare -n r=zz; ${r[-9]}`
# says `zz`. One that lands on a name with no entry hands back nothing at all,
# and the null `var` blames the word as written, cut off at its `[`: the same
# `declare -n r=yy` with `yy` unset says `r`, exactly as a plain `${nope[-9]}`
# says `nope`.
#
# "No entry" is `find_variable`'s question and not `invisible_p`'s — a bare
# `declare yy` leaves an invisible variable behind, and that counts as found.
#
# The rule reaches both reads: the plain `${r[-9]}` and the one every operator
# layers on top of (`:-`, `:=`, `:?`, a trim, a slice, `@Q`). The writes are a
# different function and quote the reference whole, subscript included.

echo "--- a chain that landed on nothing blames the word as written"
t1() { ( declare -n r=yy; echo "[${r[-1]}]"; echo "st=$?" ); }; t1
t2() { ( declare -n r=yy; echo "[${r[-9]}]" ); }; t2
t3() { ( declare -n r=yy; declare -n s=r; echo "[${s[-1]}]" ); }; t3
t4() { ( declare -n r=yy; echo "[${r[-1]:-D}]" ); }; t4
t5() { ( declare -n r=yy; echo "[${r[-1]:=w}]"; declare -p r yy ); }; t5
t6() { ( declare -n r=yy; echo "[${r[-1]:+P}]" ); }; t6
t7() { ( declare -n r=yy; echo "[${r[-1]#x}]" ); }; t7
t8() { ( declare -n r=yy; echo "[${r[-1]:0:2}]" ); }; t8
t9() { ( declare -n r=yy; echo "[${r[-1]@Q}]" ); }; t9
ta() { ( declare -n r=yy; echo "[${r[-1]:?nope}]"; echo "st=$?" ); }; ta
tb() { ( declare -n r=yy; set -u; echo "[${r[-1]}]"; echo "st=$?" ); }; tb

echo "--- …and the subscript is still evaluated for its side effects"
tc() { ( declare -n r=yy; echo "[${r[i++]}]"; echo "i=$i" ); }; tc
td() { ( declare -n r=yy; echo "[${r[1+]}]"; echo "st=$?" ); }; td
te() { ( declare -n r=yy; echo "[${r[$(echo -1)]}]" ); }; te

echo "--- a chain that landed on a name that is there blames that name"
tf() { ( zz=(a b); declare -n r=zz; echo "[${r[-9]}]" ); }; tf
tg() { ( zz=scalar; declare -n r=zz; echo "[${r[-9]}]" ); }; tg
th() { ( declare -A zz=([k]=1); declare -n r=zz; echo "[${r[-1]}]" ); }; th
ti() { ( zz=(a b); declare -n r=zz; echo "[${r[-9]:-D}]" ); }; ti
tj() { ( declare zz; declare -n r=zz; echo "[${r[-1]}]" ); }; tj
tk() { ( declare -a zz; declare -n r=zz; echo "[${r[-1]}]" ); }; tk
tl() { ( declare -i zz; declare -n r=zz; echo "[${r[-1]:-D}]" ); }; tl
tm() { ( zz=; declare -n r=zz; echo "[${r[-1]}]" ); }; tm

echo "--- a name with no chain at all is the same null lookup"
tn() { ( echo "[${nope[-1]}]"; echo "st=$?" ); }; tn
to() { ( echo "[${nope[-1]:-D}]" ); }; to
tp() { ( declare nope; echo "[${nope[-1]}]" ); }; tp

echo "--- the frame the name is looked up in is the one the walk reached"
tq() { f() { echo "[${r[-9]}]"; }; ( declare -n r=zz; zz=(a b); f ); }; tq
tr() { f() { local zz=1; echo "[${r[-9]}]"; }; ( declare -n r=zz; f ); }; tr
ts() { f() { echo "[${r[-9]}]"; }; ( declare -n r=zz; f ); }; ts

echo "--- the length form quotes the subscript instead, and the writes the whole reference"
tt() { ( declare -n r=yy; echo "[${#r[-1]}]"; echo "st=$?" ); }; tt
tu() { ( declare -n r=yy; r[-1]=v; echo "st=$?" ); }; tu
tv() { ( declare -n r=yy; printf -v 'r[-1]' Z; echo "st=$?" ); }; tv
tw() { ( declare -n r=yy; read 'r[-1]' <<< Z; echo "st=$?" ); }; tw
