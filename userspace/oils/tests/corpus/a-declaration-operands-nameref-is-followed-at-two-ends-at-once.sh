# A declaration builtin's operand can name two different variables at once, and
# bash's diagnostics say which of them each refusal is about.
#
# The builtin resolves the operand first. Inside a function (`variable_context
# && mkglobal == 0`, declare.def:601) it runs the word through
# `declare_transform_name` (declare.def:205) and declares *that*:
#
#   v = find_variable_last_nameref (name, 1);
#   newname = (v && v->context != variable_context) ? name : name_cell (var);
#   …
#   var = make_local_variable ((flags_on & att_nameref) ? name : newname, …);
#
# so `local -n r=g; declare r=9` is a declaration about `g`. And it is
# `make_local_variable` that refuses to shadow a readonly *global*
# (variables.c:2683) —
#
#   if (old_var && (noassign_p (old_var) ||
#                  (readonly_p (old_var) && old_var->context == 0)))
#     { if (readonly_p (old_var)) sh_readonly (name); … return NULL; }
#
# — with the name it was handed, i.e. the resolved target. That refusal sits
# three hundred lines above declare.def:849's, which quotes the operand as
# written, so inside a function the resolved name wins and at top level the
# written one does. A name the frame already holds comes back from
# `make_local_variable` early (`local_p (old_var) && old_var->context ==
# variable_context`), before the check, and a readonly *local* of an outer
# frame is one bash shadows quite happily — both fall back to :849.
#
# The element **store**, though, is handed the raw operand and keeps it only to
# complain with. `assign_array_element` (declare.def:960) finds its variable
# through a follow of its own and passes the word down beside it:
#
#   vname = array_variable_name (name, avflags, &sub, &sublen);
#   entry = find_variable (vname);
#   …
#   entry = assign_array_element_internal (entry, name, vname, sub, …);
#
#   static SHELL_VAR *
#   assign_array_element_internal (entry, name, vname, sub, sublen, …)
#        char *name;		/* only used for error messages */
#
# so every diagnostic that store emits — the readonly refusal, and all three
# spellings of `bad array subscript` — names the reference as written, whatever
# variable the element itself belonged to.

echo "--- the builtin's own refusal names the resolved target, inside a function"
t1() { f() { local -n r=g; declare r=5; echo "st=$?"; declare -p r g; }; ( g=1; readonly g; f ); }; t1
t2() { f() { local -n r=g; local r=5; echo "st=$?"; declare -p r g; }; ( g=1; readonly g; f ); }; t2
t3() { f() { local -n r=g; typeset r=9; echo "st=$?"; declare -p r g; }; ( g=1; readonly g; f ); }; t3
t4() { f() { local -n r=g; declare -x r=5; echo "st=$?"; declare -p r g; }; ( g=1; readonly g; f ); }; t4
t5() { f() { local -n r=g; declare -n r2=r; declare r2=5; echo "st=$?"; declare -p r r2 g; }; ( g=1; readonly g; f ); }; t5
t6() { f() { local -n r=g; declare 'r[1]=9'; echo "st=$?"; declare -p r g; }; ( declare -a g=(1 2); readonly g; f ); }; t6
t7() { f() { local -n r=g; local -A 'r[k]=9'; echo "st=$?"; declare -p r g; }; ( declare -A g=([j]=1); readonly g; f ); }; t7
t8() { f() { local -n r=g; declare -r 'r[1]=9'; echo "st=$?"; }; ( readonly g; f ); }; t8

echo "--- …and the operand as written at top level, where no local is made"
t9() { ( declare -n r=g; g=1; readonly g; declare r=5; echo "st=$?"; declare -p r g ); }; t9
ta() { ( declare -n r=g; g=(1 2); readonly g; declare 'r[1]=9'; echo "st=$?"; declare -p r g ); }; ta
tb() { f() { local -n r=g; declare -g r=5; echo "st=$?"; declare -p r g; }; ( g=1; readonly g; f ); }; tb
tc() { f() { local -n r=g; readonly r=9; echo "st=$?"; declare -p r g; }; ( g=1; readonly g; f ); }; tc
td() { f() { local -n r=g; export r=9; echo "st=$?"; declare -p r g; }; ( g=1; readonly g; f ); }; td

echo "--- a name the frame already holds is no shadow, so it falls back to :849"
te() { f() { local -r g=1; local -n r=g; declare r=9; echo "st=$?"; declare -p r g; }; ( f ); }; te
tf() { f() { local -r g=(1 2); local -n r=g; declare 'r[1]=9'; echo "st=$?"; declare -p r g; }; ( f ); }; tf
tg() { f() { local -n r=g; declare -r 'r[1]=9' 'r[0]=7'; echo "st=$?"; declare -p r g; }; ( g=(1 2); f ); }; tg

echo "--- the element store names the reference as written, whatever it stored into"
th() { f() { local -n r=g; declare -r 'r[1]=9'; echo "st=$?"; declare -p r g; }; ( g=(1 2); f ); }; th
ti() { f() { local -n r=g; declare -ri 'r[1]=4+4'; echo "st=$?"; declare -p r g; }; ( g=(1 2); f ); }; ti
tj() { f() { local -n r=g; declare -r 'r[1]+=9'; echo "st=$?"; declare -p r g; }; ( g=(1 2); f ); }; tj
tk() { f() { local -n r=g; local -n g=q; declare -r 'r[1]=9'; echo "st=$?"; declare -p r g q; }; ( f ); }; tk
tl() { f() { local -n r=g; declare -r 'r[x]=9'; echo "st=$?"; declare -p r g; }; ( declare -A g=([k]=1); f ); }; tl
tm() { f() { local -n r=g; declare 'r[-9]=9'; echo "st=$?"; declare -p r g; }; ( g=(1 2); f ); }; tm
tn() { f() { local -n r=g; declare 'r[]=9'; echo "st=$?"; declare -p r g; }; ( g=(1 2); f ); }; tn

echo "--- and so does a plain element write through one"
to() { ( declare -n r=g; g=(1 2); r[-9]=9; echo "st=$?"; declare -p g ); }; to
tp() { ( declare -n r=g; g=(1 2); r[]=9; echo "st=$?"; declare -p g ); }; tp
tq() { ( declare -n r=g; g=(1 2); r[*]=9; echo "st=$?"; declare -p g ); }; tq
tr() { ( declare -A m=([k]=1); declare -n r=m; r['']=9; echo "st=$?"; declare -p m ); }; tr
ts() { ( declare -A m=([k]=1); declare -n r=m; r[]=9; echo "st=$?"; declare -p m ); }; ts
tt() { f() { local -n r=g; r[-9]=9; echo "st=$?"; declare -p g; }; ( g=(1 2); f ); }; tt
tu() { ( declare -n r=g; g=(1 2); readonly g; r[1]=9; echo "st=$?" ); }; tu
tv() { ( declare -n r=g; declare -a g=(1 2); readonly g; r=(p q); echo "st=$?" ); }; tv

echo "--- the store still goes where the chain points"
tw() { f() { local -n r=g; declare 'r[1]=9'; echo "st=$?"; declare -p r g; }; ( g=(1 2); f ); }; tw
tx() { f() { local -n r=g; declare -i 'r[1]=4+4'; echo "st=$?"; declare -p r g; }; ( g=(1 2); f ); }; tx
ty() { ( declare -n r=g; g=(1 2); r=(p q); declare -p g r ); }; ty
tz() { f() { local -n r=g; declare -gr 'r[1]=9'; echo "st=$?"; declare -p g; }; ( g=(1 2); f; echo AFTER; declare -p g ); }; tz
