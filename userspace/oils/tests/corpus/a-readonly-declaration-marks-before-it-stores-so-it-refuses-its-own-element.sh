# A declaration builtin applies its attributes *before* it stores a
# subscripted operand's value, so a `-r` declaration refuses its own element.
#
# declare.def puts the two in that order:
#
#   VSETATTR (var, flags_on);                                  /* :944 */
#   VUNSETATTR (var, flags_off);
#   ...
#   var = assign_array_element (name, value, local_aflags, 0);  /* :960 */
#   if (var == 0)          /* some kind of assignment error */
#     { assign_error++; ... NEXT_VARIABLE (); }
#
# and, unlike the whole-array store a few lines above it, the element store
# carries no ASS_FORCE. So the `r` is on the variable by the time the store
# arrives, and `bind_array_variable` (arrayfunc.c:257) turns it away:
#
#   entry = find_shell_variable (name);
#   if (entry == 0) { entry = find_variable_nameref_for_create (name, 0); ... }
#   if (entry == 0) entry = make_new_array_variable (name);
#   else if ((readonly_p (entry) && (flags&ASS_FORCE) == 0) || noassign_p (entry))
#     { if (readonly_p (entry)) err_readonly (name); return (entry); }
#   else if (array_p (entry) == 0) entry = convert_var_to_array (entry);
#   return (bind_array_var_internal (entry, ind, 0, value, flags));
#
# Three things follow from where that check sits.
#
#  * It returns `entry` — non-NULL — so declare.def:962's `if (var == 0)` never
#    fires: the refusal does not fail the builtin and does not end a posix
#    shell. Errexit still ends it, but by another road entirely: `err_readonly`
#    is `report_error` (error.c:533), which calls `exit_shell` at the message.
#  * It is *above* `convert_var_to_array`, so a refused element leaves a scalar
#    a scalar.
#  * The subscript was evaluated before any of it (arrayfunc.c:398), so a bad
#    subscript is reported instead of the readonly — and that one *does* return
#    NULL, so it fails the builtin.
#
# The bound a negative subscript counts back from is read off the variable that
# was found, before the conversion:
#
#   if (entry && ind < 0)
#     ind = (array_p (entry) ? array_max_index (array_cell (entry)) : 0) + 1 + ind;
#   if (ind < 0) { err_badarraysub (name); return NULL; }
#
# so an existing-but-unset name bounds at 1 like any other scalar, and only a
# name with no entry at all bounds at nothing.

echo "--- the declaration refuses its own element"
t1() { ( declare -a a=(x y); declare -r 'a[1]=9'; echo "st=$?"; declare -p a ); }; t1
t2() { ( declare -A m=([k]=1); declare -r 'm[k]=9'; echo "st=$?"; declare -p m ); }; t2
t3() { ( declare -a a=(x y); declare -ri 'a[1]=4+4'; echo "st=$?"; declare -p a ); }; t3
t4() { ( declare -a a=(1 2); declare -r 'a[1]+=9'; echo "st=$?"; declare -p a ); }; t4
t5() { ( declare -a a=(x y); declare -rx 'a[1]=9'; echo "st=$?"; declare -p a ); }; t5

echo "--- the *whole-array* store carries ASS_FORCE, so it is not refused"
t6() { ( declare -a a=(x y); declare -r a=(p q); echo "st=$?"; declare -p a ); }; t6
t7() { ( declare -A m=([k]=1); declare -r m=([j]=2); echo "st=$?"; declare -p m ); }; t7

echo "--- the order shows in a second operand, which meets the tagged :849 refusal"
t8() { ( declare -a a=(x y); declare -r 'a[1]=9' 'a[0]=7'; echo "st=$?"; declare -p a ); }; t8
t9() { ( declare -a a=(x y); declare -ar a; declare 'a[1]=9'; echo "st=$?"; declare -p a ); }; t9

echo "--- neither the builtin nor a posix shell fails on it"
ta() { ( set -o posix; declare -a a=(x y); declare -r 'a[1]=9'; echo "st=$?"; echo REACHED ); }; ta
tb() { ( declare -a a=(x y); declare -r 'a[1]=9'; a[0]=z; echo "st2=$?"; declare -p a; echo REACHED ); }; tb

echo "--- …but errexit does, at the diagnostic, whatever it is written inside"
tc() { ( set -e; declare -a a=(x y); declare -r 'a[1]=9'; echo "st=$?"; echo REACHED ); echo "sub=$?"; }; tc
td() { ( set -e; declare -a a=(x y); if declare -r 'a[1]=9'; then echo THEN; else echo ELSE; fi ); echo "sub=$?"; }; td
te() { ( set -e; declare -a a=(x y); declare -r 'a[1]=9' && echo AND || echo OR ); echo "sub=$?"; }; te
tf() { ( set -e; declare -a a=(x y); declare -r 'a[-9]=9'; echo "st=$?" ); echo "sub=$?"; }; tf

echo "--- a bad subscript is judged first, and that one *does* fail the builtin"
tg() { ( declare -a x=(1); declare -r 'x[-9]=9'; echo "st=$?"; declare -p x ); }; tg
th() { ( declare -a x=(1); declare -r 'x[y z]=9'; echo "st=$?"; declare -p x; echo REACHED ); }; th
ti() { ( declare -a x=(1); declare -r 'x[]=9'; echo "st=$?"; declare -p x ); }; ti
tj() { ( declare -a x=(1); declare -r 'x[-9]=9' 'x[0]=7'; echo "st=$?"; declare -p x ); }; tj

echo "--- a refused element leaves a scalar a scalar"
tk() { f() { local -r x=5; declare -g 'x[1]=9'; echo "st=$?"; declare -p x; }; ( f ); }; tk
tl() { f() { local -r x=5; declare -g 'x[-1]=9'; echo "st=$?"; declare -p x; }; ( f ); }; tl
tm() { ( declare -r x=5; declare -g 'x[1]=9'; echo "st=$?"; declare -p x ); }; tm
tn() { f() { local -r x=(1 2); declare -g 'x[3]=9'; echo "st=$?"; declare -p x; }; ( f ); }; tn
to() { f() { local -rA m=([k]=1); declare -g 'm[j]=9'; echo "st=$?"; declare -p m; }; ( f ); }; to

echo "--- the -g split marks the global and lets the live local take the element"
tp() { f() { local -a a=(x y); declare -gr 'a[1]=9'; echo "st=$?"; declare -p a; }; ( f; echo AFTER; declare -p a ); }; tp
tq() { f() { local -a a=(x y); declare -gr 'a[1]=9' 'a[0]=7'; echo "st=$?"; declare -p a; }; ( f; echo AFTER; declare -p a ); }; tq
tr() { f() { local a=5; declare -gr 'a[1]=9'; echo "st=$?"; declare -p a; }; ( f; echo AFTER; declare -p a ); }; tr

echo "--- what a negative subscript counts back from, read before any widening"
ts() { ( x=5; x[-1]=9; declare -p x ); }; ts
tt() { ( x=5; x[-2]=9; echo "st=$?"; declare -p x ); }; tt
tu() { ( x[-1]=9; echo "st=$?"; declare -p x; echo REACHED ); }; tu
tv() { ( declare -a x; x[-1]=9; echo "st=$?"; declare -p x; echo REACHED ); }; tv
tw() { ( declare -a x=(); x[-1]=9; echo "st=$?"; declare -p x; echo REACHED ); }; tw
tx() { f() { local x; x[-1]=9; echo "st=$?"; declare -p x; }; ( f ); }; tx
ty() { ( declare -i x; x[-1]=9; echo "st=$?"; declare -p x ); }; ty
tz() { ( declare -a x=([5]=q); x[-1]=9; declare -p x ); }; tz
tA() { f() { local SECONDS; SECONDS[-1]=9; echo "st=$?"; declare -p SECONDS; }; ( f ); }; tA
tB() { ( x=5; readonly x; x[-1]=9; echo "st=$?"; declare -p x; echo REACHED ); }; tB
