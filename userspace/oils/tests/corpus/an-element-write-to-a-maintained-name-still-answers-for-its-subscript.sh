# The variables bash maintains itself carry `att_noassign`, and a write to one
# is turned away — silently, and without failing. But the refusal lives inside
# `bind_array_variable` (arrayfunc.c:280):
#
#      else if ((readonly_p (entry) && (flags&ASS_FORCE) == 0) || noassign_p (entry))
#        { if (readonly_p (entry)) err_readonly (name); return (entry); }
#
# which is *below* every test the subscript owes and *above* the
# `convert_var_to_array` on the next line. Three things follow. An element
# write reaches its own complaints first, so `GROUPS[-9]=9` is a bad array
# subscript and `GROUPS[i++]=9` still increments. The refusal is not an error —
# `return (entry)` is non-NULL — so the command succeeds, though the status an
# assignment-only command answers with still moves, a refusal being one more
# thing that sets it. And no widening happens, so a refused element leaves no
# array entry behind for a name that had none.
#
# The sibling writers reach the same line by another road: `read`, `printf -v`
# and `(( … ))` all go through it, and a declaration builtin refuses the whole
# name earlier and louder ("variable may not be assigned value").

echo "--- an element write is judged before it is turned away"
t1() { ( GROUPS[-9]=9; echo "st=$?" ); }; t1
t2() { ( FUNCNAME[-1]=9; echo "st=$?" ); }; t2
t3() { ( BASH_SOURCE[-1]=9; echo "st=$?" ); }; t3
t4() { ( BASH_LINENO[-1]=9; echo "st=$?" ); }; t4
t5() { ( BASH_ARGV[-1]=9; echo "st=$?" ); }; t5
t6() { ( BASH_ARGC[-1]=9; echo "st=$?" ); }; t6

echo "--- every one of the subscript's own refusals, not just the negative"
t7() { ( GROUPS[]=9; echo "st=$?" ); }; t7
t8() { ( GROUPS[*]=9; echo "st=$?" ); }; t8
t9() { ( GROUPS[@]=9; echo "st=$?" ); }; t9
ta() { ( GROUPS[1+]=9; echo "st=$?" ); }; ta
tb() { ( GROUPS[x y]=9; echo "st=$?" ); }; tb

echo "--- …and the subscript's side effects, which happen either way"
tc() { ( i=0; GROUPS[i++]=9; echo "st=$?"; echo "i=$i" ); }; tc
td() { ( GROUPS[$(echo ran >&2; echo 0)]=9; echo "st=$?" ); }; td
te() { ( i=0; GROUPS[i++]+=9; echo "i=$i" ); }; te

echo "--- a subscript that is fine is refused in silence, and nothing is widened"
tf() { ( FUNCNAME[9]=9; echo "st=$?"; declare -p FUNCNAME ); }; tf
tg() { ( BASH_LINENO[3]=9; echo "st=$?"; declare -p BASH_LINENO ); }; tg
th() { ( BASH_SOURCE[0]=9; echo "st=$?"; declare -p BASH_SOURCE ); }; th
ti() { ( BASH_ARGV[0]=9; echo "st=$?"; echo "n=${#BASH_ARGV[@]}" ); }; ti
tj() { ( FUNCNAME[0]+=x; echo "st=$?"; declare -p FUNCNAME ); }; tj

echo "--- the refusal is silent and succeeds, but still moves the command's status"
tk() { ( FUNCNAME[0]=$(exit 3); echo "st=$?" ); }; tk
tl() { ( FUNCNAME[0]=1 z=$(exit 3); echo "st=$?" ); }; tl
tm() { ( z=$(exit 3) FUNCNAME[0]=1; echo "st=$?" ); }; tm
tn() { ( FUNCNAME[0]=$(exit 3) true; echo "st=$?" ); }; tn
to() { ( FUNCNAME[0]=9 echo ran; echo "st=$?" ); }; to

echo "--- the sibling writers reach the same line by another road"
tp() { ( read 'FUNCNAME[-1]' <<< Z; echo "st=$?" ); }; tp
tq() { ( read 'FUNCNAME[0]' <<< Z; echo "st=$?"; declare -p FUNCNAME ); }; tq
tr() { ( printf -v 'FUNCNAME[-1]' Z; echo "st=$?" ); }; tr
ts() { ( printf -v 'FUNCNAME[0]' Z; echo "st=$?"; declare -p FUNCNAME ); }; ts
tt() { ( (( FUNCNAME[-1] = 9 )); echo "st=$?" ); }; tt
tu() { ( (( FUNCNAME[0] = 9 )); echo "st=$?"; declare -p FUNCNAME ); }; tu
tv() { ( unset 'FUNCNAME[-1]'; echo "st=$?" ); }; tv

echo "--- a declaration builtin refuses the whole name earlier and louder"
tw() { ( declare 'FUNCNAME[-1]=9'; echo "st=$?" ); }; tw
tx() { ( declare 'FUNCNAME[0]=9'; echo "st=$?" ); }; tx
ty() { ( local 'FUNCNAME[-1]=9'; echo "st=$?" ); }; ty
tz() { ( FUNCNAME=(x y); echo "st=$?"; declare -p FUNCNAME ); }; tz
