# `array_variable_part` hands `array_value_internal` whatever `find_variable`
# found, and it is allowed to have found nothing — a circular chain, or a
# reference that already designates one element and so has no array left for a
# *second* subscript. The read carries on without a variable:
#
#      var = array_variable_part (s, flags, &t, &len);	/* XXX */
#
#      /* Expand the index, even if the variable doesn't exist, in case side
#         effects are needed, like ${w[i++]} where w is unset. */
#   #if 0
#      if (var == 0)
#        return (char *)NULL;
#   #endif
#
# — arrayfunc.c:1487. So the subscript is still expanded (`${a[i++]}` still
# increments), its arithmetic is still judged (`${a[1+]}` is still a syntax
# error), and a negative one still underflows, because only an array counts
# back from anything:
#
#      ind = array_expand_index (var, t, len, flags);
#      if (ind < 0)
#        {
#          if (var && array_p (var))
#            ind = array_max_index (array_cell (var)) + 1 + ind;
#          if (ind < 0)
#            INDEX_ERROR();
#        }
#
# `INDEX_ERROR()` (arrayfunc.c:1456) is what picks the name, and it picks a
# different one either side of the same question —
#
#      if (var) err_badarraysub (var->name);
#      else { t[-1] = '\0'; err_badarraysub (s); t[-1] = '['; }
#
# — the variable the walk *found*, or, where it found none, the word as
# **written**, cut off at its `[`. Which is the mirror image of an element
# *write*, where the diagnostic always quotes the word as written and only the
# store follows the chain.
#
# The length form takes none of this route: `array_length_reference`
# (subst.c:7213) answers a null `var` with 0 before the subscript is looked at,
# and faults under `set -u` naming the base alone.

echo "--- the subscript is judged against the nothing the walk found"
t1() { ( declare -n a=b; declare -n b=a; echo "[${a[-1]}]" ); }; t1
t2() { ( declare -n a=b; declare -n b=a; echo "[${a[0]}]" ); }; t2
t3() { ( declare -n a=b; declare -n b=a; echo "[${a[1+]}]" ); }; t3
t4() { ( declare -n a=b; declare -n b=a; i=0; echo "[${a[i++]}]"; echo "i=$i" ); }; t4
t5() { ( declare -n a=b; declare -n b=a; echo "[${a[k]}]" ); }; t5
t6() { ( declare -n a=b; declare -n b=a; echo "[${a[*]}]" ); }; t6
t7() { ( declare -n a=b; declare -n b=a; echo "[${a[@]}]" ); }; t7
t8() { ( declare -n r='n[1]'; n=(a b c); echo "[${r[-1]}]" ); }; t8
t9() { ( declare -n r='n[1]'; n=(a b c); echo "[${r[0]}]" ); }; t9
ta() { ( declare -n r='n[1]'; n=(a b c); echo "[${r[1+]}]" ); }; ta
tb() { ( declare -n r='n[1]'; n=(a b c); i=0; echo "[${r[i++]}]"; echo "i=$i" ); }; tb
tc() { ( declare -n r='n[@]'; n=(a b c); echo "[${r[-1]}]" ); }; tc
td() { ( declare -A m=([k]=1); declare -n r='m[k]'; echo "[${r[-1]}]" ); }; td
te() { ( declare -n r='n[1]'; unset n; echo "[${r[-1]}]" ); }; te

echo "--- and the name it blames is the one written, where none was found"
tf() { ( declare -a q=(1 2); declare -n r=q; echo "[${r[-9]}]" ); }; tf
tg() { ( declare -a q=(1 2); declare -n r=q; declare -n s=r; echo "[${s[-9]}]" ); }; tg
th() { ( echo "[${nosuch[-1]}]" ); }; th
ti() { ( declare -n r=; echo "[${r[-1]}]" ); }; ti

echo "--- every form that reads a value asks the same question"
tj() { ( declare -n a=b; declare -n b=a; echo "[${a[-1]-D}]" ); }; tj
tk() { ( declare -n a=b; declare -n b=a; echo "[${a[-1]:-D}]" ); }; tk
tl() { ( declare -n a=b; declare -n b=a; echo "[${a[-1]+S}]" ); }; tl
tm() { ( declare -n a=b; declare -n b=a; echo "[${a[-1]/x/y}]" ); }; tm
tn() { ( declare -n a=b; declare -n b=a; echo "[${a[-1]:1:2}]" ); }; tn
to() { ( declare -n a=b; declare -n b=a; echo "[${a[-1]^^}]" ); }; to
tp() { ( declare -n a=b; declare -n b=a; echo "[${a[-1]@Q}]" ); }; tp
# …including the two that write, whose own refusal follows the read's. (Written
# at the top level rather than in a function, because *where* the store a cycle
# falls back on lands is a question of frames and belongs to the write half —
# see TD-OILS-A-DEFAULT-ASSIGNMENT-THROUGH-A-CYCLE-STORES-IN-THE-FRAME-IT-RAN-IN.)
( declare -n a=b; declare -n b=a; echo "[${a[-1]:=w}]" )
( declare -n a=b; declare -n b=a; echo "[${a[0]:=w}]"; declare -p a b )
tr() { ( declare -n a=b; declare -n b=a; echo "[${a[-1]:?boom}]"; echo "st=$?" ); }; tr
ts() { ( declare -n a=b; declare -n b=a; i=0; echo "[${a[i++]-D}]"; echo "i=$i" ); }; ts
tt() { ( declare -n r='n[1]'; n=(a b c); echo "[${r[-1]-D}]" ); }; tt
tu() { ( declare -n r='n[1]'; n=(a b c); echo "[${r[0]-D}]" ); }; tu

echo "--- an unsubscripted read of the same chain has nothing to judge"
tv() { ( declare -n a=b; declare -n b=a; echo "[${a-D}]" ); }; tv
tw() { ( declare -n a=b; declare -n b=a; echo "[${a:-D}]" ); }; tw
tx() { ( declare -n r='n[1]'; n=(a b c); echo "[${r-D}]" ); }; tx
ty() { ( declare -n r='n[1]'; n=(a b c); echo "[${r/a/X}]" ); }; ty

echo "--- the length form answers before the subscript is looked at"
tz() { ( declare -n a=b; declare -n b=a; echo "[${#a[-1]}]" ); }; tz
u1() { ( declare -n a=b; declare -n b=a; echo "[${#a[0]}]" ); }; u1
u2() { ( declare -n a=b; declare -n b=a; echo "[${#a[@]}]" ); }; u2
u3() { ( declare -n a=b; declare -n b=a; echo "[${#a[1+]}]" ); }; u3
u4() { ( declare -n r='n[1]'; n=(a b c); echo "[${#r[-1]}]" ); }; u4

echo "--- …and faults under set -u naming the base alone, where the value form"
echo "--- reports the subscript first and then the element it could not find"
u5() { ( declare -n a=b; declare -n b=a; set -u; echo "[${a[-1]}]" ); }; u5
u6() { ( declare -n a=b; declare -n b=a; set -u; echo "[${a[0]}]" ); }; u6
u7() { ( declare -n a=b; declare -n b=a; set -u; echo "[${#a[0]}]" ); }; u7
u8() { ( declare -n a=b; declare -n b=a; set -u; echo "[${#a[-1]}]" ); }; u8
u9() { ( declare -n r='n[1]'; n=(a b c); set -u; echo "[${r[-1]}]" ); }; u9
ua() { ( declare -n r='n[1]'; n=(a b c); set -u; echo "[${#r[0]}]" ); }; ua
