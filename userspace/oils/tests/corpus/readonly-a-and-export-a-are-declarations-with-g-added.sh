# An `-a`/`-A` operand of `readonly`/`export` **carrying a value** is not run by
# `set_or_show_attributes` at all. bash rewrites it into a `declare` command and
# calls the declare builtin on it, adding the `-g` itself (setattr.def:234):
#
#	      /* Let's try something here.  Turn readonly -a xxx=yyy into
#		 declare -ra xxx=yyy and see what that gets us. */
#	      if (arrays_only || assoc_only)
#		{
#		  ?
#		  /* Add -g to avoid readonly/export creating local variables:
#		     only local/declare/typeset create local variables */
#		  opti = 0;
#		  optw[opti++] = '-';
#		  optw[opti++] = 'g';
#		  if (attribute & att_readonly)  optw[opti++] = 'r';
#		  if (attribute & att_exported)  optw[opti++] = 'x';
#		  if (arrays_only)  optw[opti++] = 'a';
#		  else              optw[opti++] = 'A';
#		  ?
#		  opt = declare_builtin (nlist);
#
# So `readonly -a g=9` *is* `declare -gra g=9` ? a global, and the whole-array
# split that goes with it: the declaration is about the global while the store
# is `bind_array_variable (name, 0, ?)`, a live lookup. The marking that
# follows (setattr.def:279, outside the `if`) is then `set_var_attribute`,
# which marks whichever binding is live.
#
# Diagnostics keep the *calling* builtin's tag: `declare_builtin` is a plain C
# call, so `this_command_name` is never changed for it.
exec 2>&1
echo "--- the declaration is global, the element goes to the live binding"
t1() { local g=5; readonly -a g=9; echo "st=$?"; declare -p g; }; ( t1; echo AFTER; declare -p g )
t2() { local g=5; export -a g=9; echo "st=$?"; declare -p g; }; ( t2; echo AFTER; declare -p g )
t3() { local -a g=(1 2); readonly -a g=9; declare -p g; }; ( t3; echo AFTER; declare -p g )
t4() { local g=5; readonly -a g+=9; declare -p g; }; ( t4; echo AFTER; declare -p g )
t5() { local -i g=5; readonly -a g=4+4; declare -p g; }; ( t5; echo AFTER; declare -p g )
t6() { local -u g=5; readonly -a g=ab; declare -p g; }; ( t6; echo AFTER; declare -p g )
echo "--- ASS_FORCE: a readonly live binding takes the element and says nothing"
t7() { local -r g=5; readonly -a g=9; echo "st=$?"; declare -p g; }; ( t7; echo AFTER; declare -p g )
echo "--- the assoc half is handed the declaration's own variable, so -A does not split"
t8() { local g=5; readonly -A g=9; echo "st=$?"; declare -p g; }; ( t8; echo AFTER; declare -p g )
t9() { local g=5; export -A g=9; echo "st=$?"; declare -p g; }; ( t9; echo AFTER; declare -p g )
echo "--- the rewrite tests arrays_only first, so -a beats -A (declare is the other way)"
ta() { readonly -aA g=9; echo "st=$?"; declare -p g; }; ( ta )
tb() { export -Aa g=9; echo "st=$?"; declare -p g; }; ( tb )
echo "--- -n edits attribute asymmetrically: readonly loses its r, export keeps its x"
tc() { local g=5; readonly -an g=9; echo "st=$?"; declare -p g; }; ( tc; echo AFTER; declare -p g )
td() { local g=5; export -an g=9; echo "st=$?"; declare -p g; }; ( td; echo AFTER; declare -p g )
echo "--- a declare refusal, spoken in the calling builtin's name"
te() { declare -ga q=(1 2); readonly -A q=9; echo "st=$?"; declare -p q; }; ( te )
tf() { declare -gA m=([k]=1); export -a m=9; echo "st=$?"; declare -p m; }; ( tf )
echo "--- ? but a local of the other kind is not in the declaration's way"
te2() { declare -a q=(1 2); readonly -A q=9; echo "st=$?"; declare -p q; }; ( te2 )
tf2() { declare -A m=([k]=1); readonly -a m=9; echo "st=$?"; declare -p m; }; ( tf2 )
echo "--- ? and it is an assign_error, so posix mode ends the shell there"
( set -o posix; declare -a q=(1 2); readonly -A q=9; echo "st=$?"; echo REACHED )
echo "--- gated on a value and on a legal identifier; neither reaches the rewrite"
tg() { local g=5; readonly -a g; echo "st=$?"; declare -p g; }; ( tg; echo AFTER; declare -p g )
th() { local g=5; export -a g; echo "st=$?"; declare -p g; }; ( th; echo AFTER; declare -p g )
ti() { local g=5; readonly -a g[1]=9; echo "st=$?"; declare -p g; }; ( ti; echo AFTER; declare -p g )
tj() { readonly -a 1bad=9; echo "st=$?"; }; ( tj )
echo "--- neither is a plain scalar operand, nor a compound literal"
tk() { local g=5; readonly g=9; echo "st=$?"; declare -p g; }; ( tk; echo AFTER; declare -p g )
tl() { local g=5; readonly -a g=(1 2); declare -p g; }; ( tl; echo AFTER; declare -p g )
tm() { local g=5; export -a g=(1 2); declare -p g; }; ( tm; echo AFTER; declare -p g )
echo "--- each operand is rewritten on its own"
tn() { local g=5; readonly -a g=9 h=8; declare -p g h; }; ( tn; echo AFTER; declare -p g h )
to() { export -a g h=9; declare -p g h; }; ( to )
echo "--- a nameref operand is handed to the declaration whole, and marked after"
tp() { w=5; declare -n r=w; export -a r=9; declare -p r w; }; ( tp )
tq() { w=5; declare -n r=w; readonly -a r+=9; declare -p r w; }; ( tq )
tr() { declare -n c1=c2; declare -n c2=c1; export -a c1=5; echo "st=$?"; declare -p c1 c2; }; ( tr )
ts() { local -n z=z; readonly -a z=5; echo "st=$?"; declare -p z; }; ( ts; echo AFTER; declare -p z )
echo "--- what the global keeps once the function is gone"
tt() { local g=5; readonly -a g=9; }; ( tt; g=7; echo "st=$?" )
tu() { local g=5; export -a g=9; }; ( tu; echo "in-env: $(env | grep '^g=' || echo none)" )
