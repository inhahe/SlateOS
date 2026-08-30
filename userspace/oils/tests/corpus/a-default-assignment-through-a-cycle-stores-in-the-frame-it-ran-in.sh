# `${name:=value}` and `${name[sub]:=value}` are the two expansions that
# *write*, and where they write is decided by the same walk every other write
# uses. A circular chain designates nothing in the frame it was spelled in, so
# the store falls back on the name the walk started from — but it falls back on
# it in the scope the walk *escaped to*, not in the frame the reference sits in.
# bash reaches that scope because `bind_variable`/`bind_array_variable` look the
# name up again from scratch, and the local reference is not a candidate for
# what they find:
#
#      f() { declare -n a=b; declare -n b=a; echo "[${a[0]:=w}]"; }
#      ( f; declare -p a )        # declare -a a=([0]="w")   — the *global*
#
# The frame's own bindings come through untouched: `declare -p a b` inside `f`
# still says `declare -n a="b"` and `declare -n b="a"`.
#
# Two things follow. The bound a negative subscript counts back from is read
# off the entry that walk handed back (`if (entry && ind < 0)`,
# arrayfunc.c:398), so a chain that reached nothing has none and every negative
# subscript underflows — including one that would have landed inside the local
# scalar the reference happens to be. And the readonly attribute that guards
# the store is the *outer* one, since that is the binding being written.
#
# The sibling writes — `read 'a[i]'`, `printf -v 'a[i]'`, `a[i]=v` — already
# agree, because they resolve the destination and then enter its scope. It is
# the two expansions that did not.

echo "--- the store lands where the walk escaped to, not in the frame"
t1() { f() { declare -n a=b; declare -n b=a; echo "[${a[0]:=w}]"; declare -p a b; }; ( f; echo --; declare -p a ); }; t1
t2() { f() { declare -n a=b; declare -n b=a; echo "[${a[k]:=w}]"; declare -p a b; }; ( f; echo --; declare -p a ); }; t2
t3() { f() { declare -n a=b; declare -n b=a; echo "[${a:=w}]"; declare -p a b; }; ( f; echo --; declare -p a ); }; t3
t4() { f() { declare -n a=b; declare -n b=a; echo "[${a:-D}${a[0]:=w}]"; }; ( f; echo --; declare -p a ); }; t4

echo "--- a chain that reached nothing has no bound to count back from"
t5() { f() { declare -n a=b; declare -n b=a; echo "[${a[-1]:=w}]"; declare -p a b; }; ( f; echo --; declare -p a ); }; t5
t6() { f() { declare -n a=b; declare -n b=a; echo "[${a[*]:=w}]"; }; ( f; echo --; declare -p a ); }; t6
t7() { f() { declare -n a=b; declare -n b=a; echo "[${a[@]:=w}]"; }; ( f; echo --; declare -p a ); }; t7
t8() { f() { declare -n a=b; declare -n b=a; echo "[${a[1+]:=w}]"; }; ( f; echo --; declare -p a ); }; t8

echo "--- …and at the top level, where the frame and the escape are one place"
t9() { ( declare -n a=b; declare -n b=a; echo "[${a[0]:=w}]"; declare -p a b ); }; t9
ta() { ( declare -n a=b; declare -n b=a; echo "[${a[-1]:=w}]" ); }; ta
tb() { ( declare -n a=b; declare -n b=a; echo "[${a:=w}]"; declare -p a b ); }; tb

echo "--- the outer binding is the one the store adds to, and reads from"
tc() { f() { declare -n a=b; declare -n b=a; echo "[${a[0]:=w}]"; }; ( declare -a a=(p q); f; echo --; declare -p a ); }; tc
td() { f() { declare -n a=b; declare -n b=a; echo "[${a[-1]:=w}]"; }; ( declare -a a=(p q); f; echo --; declare -p a ); }; td
te() { f() { declare -n a=b; declare -n b=a; echo "[${a[9]:=w}]"; }; ( declare -a a=(p q); f; echo --; declare -p a ); }; te
tf() { f() { declare -n a=b; declare -n b=a; echo "[${a[k]:=w}]"; }; ( declare -A a=([j]=1); f; echo --; declare -p a ); }; tf
# A readonly outer binding is a different story: the *reference* cannot be
# declared at all, because a `local` may not shadow it — so there is no cycle
# left to walk and the store is the plain outer one, refused as such.
tg() { f() { declare -n a=b; declare -n b=a; echo "[${a[9]:=w}]"; }; ( declare -a a=(p q); readonly a; f; echo "st=$?"; declare -p a ); }; tg
th() { f() { declare -n a=b; declare -n b=a; echo "[${a:=w}]"; }; ( a=p; readonly a; f; echo "st=$?"; declare -p a ); }; th

echo "--- the sibling writes, which already went where the walk pointed"
ti() { f() { declare -n a=b; declare -n b=a; read 'a[0]' <<< Z; echo "st=$?"; declare -p a b; }; ( f; echo --; declare -p a ); }; ti
tj() { f() { declare -n a=b; declare -n b=a; printf -v 'a[0]' Z; echo "st=$?"; declare -p a b; }; ( f; echo --; declare -p a ); }; tj
tk() { f() { declare -n a=b; declare -n b=a; printf -v 'a[-1]' Z; echo "st=$?"; declare -p a b; }; ( f; echo --; declare -p a ); }; tk
tl() { f() { declare -n a=b; declare -n b=a; a[0]=9; declare -p a b; }; ( f; echo --; declare -p a ); }; tl
tm() { f() { declare -n a=b; declare -n b=a; a=9; declare -p a b; }; ( f; echo --; declare -p a ); }; tm

echo "--- a chain that is not circular writes where it points, as it always did"
tn() { f() { local q=loc; declare -n r=q; echo "[${r:=w}]"; declare -p q r; }; ( q=glob; f; echo --; declare -p q ); }; tn
to() { f() { declare -n r=zz; echo "[${r:=w}]"; declare -p r; }; ( f; echo --; declare -p zz ); }; to
tp() { f() { declare -n r=zz; echo "[${r[0]:=w}]"; declare -p r; }; ( f; echo --; declare -p zz ); }; tp
tq() { f() { declare -n r=zz; echo "[${r[-1]:=w}]"; }; ( f; echo --; declare -p zz ); }; tq
