# `+n` is excluded from the declaration-time nameref check outright —
# declare.def:817 asks `(flags_off & att_nameref) == 0` — so the complaint about
# a value that names nothing comes from the **store** instead, which bash still
# makes with the reference in force (declare.def:982):
#
#	  if (onref || nameref_p (var))
#	    aflags |= ASS_NAMEREF;
#	  v = bind_variable_value (var, value, aflags);
#	  if (v == 0 && (onref || nameref_p (var)))
#	    {
#	      if (valid_nameref_value (value, 1) == 0)
#		sh_invalidid (value);
#	      assign_error++;
#	      …
#	      flags_on |= onref;		/* undo change from above */
#	      flags_off |= offref;
#
# The removal is *deferred* past that store — `VUNSETATTR (var, offref)` is at
# declare.def:1034 — so the `NEXT_VARIABLE ()` here never reaches it and the
# reference survives the refusal intact. The wording is the generic
# `sh_invalidid` one, **not** the nameref-specific line that `-n` and the plain
# spelling give.
exec 2>&1
echo "--- a value that names nothing is refused, and the reference stands"
t1() { local -n g=z; local -n z=g; local +n g=5; echo "st=$?"; declare -p g; }; ( t1 )
t2() { local -n g=z; local -n z=g; declare +n g=5; echo "st=$?"; declare -p g; }; ( t2 )
t3() { local -n g=z; local -n z=g; typeset +n g=5; echo "st=$?"; declare -p g; }; ( t3 )
t4() { local -n g=z; local -n z=g; local +n g="a b"; echo "st=$?"; declare -p g; }; ( t4 )
t5() { local -n g=z; local -n z=g; local +n g=; echo "st=$?"; declare -p g; }; ( t5 )
echo "--- it is no cycle it asks about: a chain that reaches a live name too"
t6() { local w=1; local -n g=w; local +n g=5; echo "st=$?"; declare -p g w; }; ( t6 )
t7() { local w=1; local -n g=w; local +n g=; echo "st=$?"; declare -p g w; }; ( t7 )
echo "--- and a chain past the link cap, which reaches nothing at all"
t8() {
	local -n n1=n2 n2=n3 n3=n4 n4=n5 n5=n6 n6=n7 n7=n8 n8=n9 n9=n10 n10=n1
	local +n n1=5; echo "st=$?"; declare -p n1
}; ( t8 )
echo "--- every other letter has already landed by the time the store fails"
t9() { local -n g=z; local -n z=g; local +n -i g=5; echo "st=$?"; declare -p g; }; ( t9 )
ta() { local -n g=z; local -n z=g; local +n -x g=5; echo "st=$?"; declare -p g; }; ( ta )
tb() { local -n g=z; local -n z=g; local +n -r g=5; echo "st=$?"; declare -p g; }; ( tb )
tc() { local -n g=z; local -n z=g; local +n -l g=5; echo "st=$?"; declare -p g; }; ( tc )
td() { local -n g=z; local -n z=g; local +n -t g=5; echo "st=$?"; declare -p g; }; ( td )
te() { local -nx g=z; local -n z=g; local +n +x g=5; echo "st=$?"; declare -p g; }; ( te )
echo "--- only the operand that carried it is dropped"
tf() { local -n g=z; local -n z=g; local +n g=5 h=6; echo "st=$?"; declare -p g h; }; ( tf )
tg() { local -n g=z; local -n z=g; local +n g="a b" h=6; echo "st=$?"; declare -p g h; }; ( tg )
th() { local -n g=z; local -n z=g; local +n g=5; local +n g=6; echo "st=$?"; declare -p g; }; ( th )
echo "--- a value that IS a name retargets the reference, and the removal stands"
ti() { local -n g=z; local -n z=g; local +n g=h; echo "st=$?"; declare -p g; }; ( ti )
tj() { local -n g=z; local -n z=g; local +n g="a[1]"; echo "st=$?"; declare -p g; }; ( tj )
tk() { local w=1; local -n g=w; local +n g=h; echo "st=$?"; declare -p g w; }; ( tk )
tl() { local w=1; local -n g=w; local +n g="a[1]"; echo "st=$?"; declare -p g w; }; ( tl )
echo "--- an append goes on the reference's own text and is never validated"
tm() { local -n g=z; local -n z=g; local +n g+=5; echo "st=$?"; declare -p g; }; ( tm )
tn() { local w=1; local -n g=w; local +n g+=5; echo "st=$?"; declare -p g w; }; ( tn )
echo "--- and a valueless one makes no store to fail"
to() { local -n g=z; local -n z=g; local +n g; echo "st=$?"; declare -p g; }; ( to )
echo "--- a chain whose target does not exist re-scopes onto that target"
tp() { local -n g=nosuch; local +n g=5; echo "st=$?"; declare -p g nosuch; }; ( tp )
tq() { local -n g=nosuch; local +n g=h; echo "st=$?"; declare -p g nosuch; }; ( tq )
tr() { local -n g=z; local +n g=5; echo "st=$?"; declare -p g z; }; ( tr )
ts() { local -n g="a[1]"; local +n g=5; echo "st=$?"; declare -p g; }; ( ts )
echo "--- nor is a fresh local shadowing a cycle that lives at global scope"
declare -n gg=zz; declare -n zz=gg
tt() { local +n gg=5; echo "st=$?"; declare -p gg; }; ( tt; echo AFTER; declare -p gg zz )
tu() { declare +n gg=5; echo "st=$?"; declare -p gg; }; ( tu; echo AFTER; declare -p gg zz )
echo "--- a readonly reference is refused earlier, and says so differently"
tv() { local -nr g=z; local -n z=g; local +n g=5; echo "st=$?"; declare -p g; }; ( tv )
echo "--- nor is a declaration that takes the operand off the local branch"
tw() { local -n g=z; local -n z=g; local -g +n g=5; echo "st=$?"; declare -p g; }; ( tw )
tx() { local -n g=z; local -n z=g; export +n g=5; echo "st=$?"; declare -p g; }; ( tx )
ty() { local -n g=z; local -n z=g; readonly +n g=5; echo "st=$?"; declare -p g; }; ( ty )
echo "--- nor a top-level one, which is no local at all"
( declare -n gq=zq; declare -n zq=gq; declare +n gq=5; echo "st=$?"; declare -p gq zq )
( w=1; declare -n gq=w; declare +n gq=5; echo "st=$?"; declare -p gq w )
echo TAIL
