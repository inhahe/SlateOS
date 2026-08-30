# `${!ref[sub]}` reads its pointer by *ordinary* means. bash's
# `parameter_brace_expand_indir` hands the reference to the same
# `parameter_brace_expand_word` → `array_value` every other read goes through,
# and only afterwards asks whether what came back is a usable name:
#
#      if (var_is_special == 0 && (v = find_variable_last_nameref (name, 0)))
#        { … }
#      if (legal_identifier (name) && v == 0)
#        report_error (_("%s: invalid indirect expansion"), name);
#
# So the read's own diagnostics are paid first, and `array_value` judges the
# subscript even where `find_variable` reached nothing at all (arrayfunc.c:1487
# — see A-READ-THROUGH-A-CHAIN-THAT-REACHED-NOTHING-STILL-JUDGES-ITS-SUBSCRIPT).
# A pointer that names nowhere therefore reports *twice*: once for the
# subscript, quoting the word cut off at its `[`, and once for the pointer,
# quoting it whole.
#
# The second complaint is the one that depends on the variable existing, so a
# name that *is* there reports only the first — `q=(1 2); ${!q[-9]}` is a bad
# subscript and then an ordinary empty expansion at status 0.
#
# The walk is spread the same way: a circular chain pays two warnings for the
# read, then one more for the `find_variable_last_nameref` that follows it, with
# the subscript's refusal in between.

echo "--- a pointer that names nowhere reports its subscript first"
t1() { ( echo "[${!nosuch[-9]}]"; echo "st=$?" ); }; t1
t2() { ( echo "[${!nosuch[0]}]"; echo "st=$?" ); }; t2
t3() { ( echo "[${!nosuch[k]}]"; echo "st=$?" ); }; t3
t4() { ( echo "[${!nosuch[0+]}]"; echo "st=$?" ); }; t4
t5() { ( i=0; echo "[${!nosuch[i++]}]"; echo "i=$i" ); }; t5
t6() { ( echo "[${!nosuch[$(echo ran >&2; echo -9)]}]"; echo "st=$?" ); }; t6
t7() { ( echo "[${!nosuch[-9]:-D}]"; echo "st=$?" ); }; t7
t8() { ( echo "[${!nosuch[@]}]"; echo "st=$?" ); }; t8
t9() { ( echo "[${!nosuch[*]}]"; echo "st=$?" ); }; t9
ta() { ( set -u; echo "[${!nosuch[-9]}]"; echo "st=$?" ); }; ta
tb() { ( echo "[${!nosuch[-9]}]" ); echo "after=$?"; }; tb

echo "--- a name that is there reports only the subscript"
tc() { ( q=(1 2); echo "[${!q[-9]}]"; echo "st=$?" ); }; tc
td() { ( q=(1 2); echo "[${!q[-1]}]"; echo "st=$?" ); }; td
te() { ( q=(); echo "[${!q[-9]}]"; echo "st=$?" ); }; te
tf() { ( q=scalar; echo "[${!q[-9]}]"; echo "st=$?" ); }; tf
tg() { ( declare z; echo "[${!z[-9]}]"; echo "st=$?" ); }; tg
th() { ( declare -A m=([k]=1); echo "[${!m[-1]}]"; echo "st=$?" ); }; th
ti() { ( declare -A m=([k]=q); q=V; echo "[${!m[k]}]"; echo "st=$?" ); }; ti

echo "--- every operator layers on the same read, so it reports the same pair"
tj() { ( echo "[${!nosuch[-9]#x}]"; echo "st=$?" ); }; tj
tk() { ( echo "[${!nosuch[-9]:=w}]"; echo "st=$?" ); }; tk
tl() { ( echo "[${!nosuch[-9]@Q}]"; echo "st=$?" ); }; tl
tm() { ( echo "[${#nosuch[-9]}]"; echo "st=$?" ); }; tm

echo "--- a reference that already designates an element has nothing left to subscript"
tn() { ( declare -n r='n[1]'; n=(a b c); echo "[${!r[-1]}]"; echo "st=$?" ); }; tn
to() { ( declare -n r='n[1]'; n=(a b c); echo "[${!r[0]}]"; echo "st=$?" ); }; to

echo "--- the element that does point names whatever it spells, subscript and all"
tp() { ( w=('n[1]'); n=(a b c); echo "[${!w[0]}]"; echo "st=$?" ); }; tp
tq() { ( w=('n[@]'); n=(a b c); echo "[${!w[0]}]"; echo "st=$?" ); }; tq
tr() { ( w=('n[@]'); n=(a b c); for x in "${!w[0]}"; do echo "<$x>"; done ); }; tr

echo "--- a nameref walk is followed for the read like any other"
ts() { ( declare -n r=yy; echo "[${!r[-9]}]"; echo "st=$?" ); }; ts
tt() { ( declare -n r=yy; yy=(p q); p=V; echo "[${!r[0]}]"; echo "st=$?" ); }; tt

echo "--- and a circular chain pays two walks for the read and one for the pointer"
tu() { ( declare -n a=b; declare -n b=a; echo "[${!a[-1]}]"; echo "st=$?" ); }; tu
tv() { ( declare -n a=b; declare -n b=a; echo "[${!a[0]}]"; echo "st=$?" ); }; tv
tw() { ( declare -n a=b; declare -n b=a; echo "[${!a}]"; echo "st=$?" ); }; tw
tx() { ( declare -n a=b; declare -n b=a; echo "[${!a[$(echo ran >&2; echo -1)]}]" ); }; tx
