# A local declaration never re-scopes onto whatever its chain found. The name
# it declares comes from `declare_transform_name` (declare.def:205), whose
# chain branch reads
#
#	  v = find_variable_last_nameref (name, 1);
#	  newname = (v && v->context != variable_context) ? name : name_cell (var);
#
# — `name_cell (var)`, the found variable's **name**, not its value. A cycle
# escapes through `find_global_variable_noref (v->name)` (variables.c:2104),
# whose `v->name` is the operand's own name again, and a chain past the link
# cap finds nothing at all; either way `make_local_variable` hands back the
# frame's own reference unchanged. So what follows is a declaration *about a
# nameref*, and declare.def:817 refuses a value that names nothing:
#
#	  else if (nameref_p (var) && (flags_on & att_nameref) == 0 &&
#		   (flags_off & att_nameref) == 0 && offset &&
#		   valid_nameref_value (value, 1) == 0)
#	    builtin_error (_("`%s': invalid variable name for name reference"), value);
#
# It outranks every array and readonly refusal below it, and stands behind the
# array *kind*, which `make_local_array_variable` applied back at :614.
exec 2>&1
echo "--- a value that is no name is refused, and the reference stands"
t1() { local -n g=z; local -n z=g; local g=5; echo "st=$?"; declare -p g; }; ( t1 )
t2() { local -n g=z; local -n z=g; declare g=5; echo "st=$?"; declare -p g; }; ( t2 )
t3() { local -n g=z; local -n z=g; typeset g=5; echo "st=$?"; declare -p g; }; ( t3 )
t4() { local -n g=z; local -n z=g; local g="a b"; echo "st=$?"; declare -p g; }; ( t4 )
t5() { local -n g=z; local -n z=g; local g=; echo "st=$?"; declare -p g; }; ( t5 )
echo "--- an append quotes the text it brought, not the joined value"
t6() { local -n g=z; local -n z=g; local g+=5; echo "st=$?"; declare -p g; }; ( t6 )
echo "--- the value letters never land, because the refusal comes first"
t7() { local -n g=z; local -n z=g; local -i g=5; echo "st=$?"; declare -p g; }; ( t7 )
t8() { local -n g=z; local -n z=g; local -x g=5; echo "st=$?"; declare -p g; }; ( t8 )
echo "--- but the array kind does, and is left empty"
ta() { local -n g=z; local -n z=g; local -a g=5; echo "st=$?"; declare -p g; }; ( ta )
tb() { local -n g=z; local -n z=g; local -A g=5; echo "st=$?"; declare -p g; }; ( tb )
tc() { local -n g=z; local -n z=g; local g[1]=5; echo "st=$?"; declare -p g; }; ( tc )
td() { local -n g=z; local -n z=g; local -a g[1]=5; echo "st=$?"; declare -p g; }; ( td )
echo "--- and it outranks every refusal bash makes after it"
te() { local -n g=z; local -n z=g; local -aA g=5; echo "st=$?"; declare -p g; }; ( te )
tf() { local -n g=z; local -n z=g; local -a g; local +a g=5; echo "st=$?"; declare -p g; }; ( tf )
tg() { local -n g=z; local -n z=g; local -a g; local -A g=5; echo "st=$?"; declare -p g; }; ( tg )
th() { local -n g=z; local -n z=g; local -r g; local g=5; echo "st=$?"; declare -p g; }; ( th )
echo "--- a value that IS a name is stored through the reference as always"
ti() { local -n g=z; local -n z=g; local g=h; echo "st=$?"; declare -p g; }; ( ti )
tj() { local -n g=z; local -n z=g; local g="a[1]"; echo "st=$?"; declare -p g; }; ( tj )
# The check judges the value as *written*, so a fold that would make it a name
# comes too late — and one that leaves it a name lands after the check passes.
t9() { local -n g=z; local -n z=g; local -u g=ab; echo "st=$?"; declare -p g; }; ( t9 )
t9b() { local -n g=z; local -n z=g; local -u g="a b"; echo "st=$?"; declare -p g; }; ( t9b )
echo "--- and a valueless declaration has nothing to refuse"
tk() { local -n g=z; local -n z=g; local g; echo "st=$?"; declare -p g; }; ( tk )
tl() { local -n g=z; local -n z=g; local -i g; echo "st=$?"; declare -p g; }; ( tl )
tm() { local -n g=z; local -n z=g; local -a g; echo "st=$?"; declare -p g; }; ( tm )
echo "--- a chain past the link cap ends on no name either, and refuses silently"
tn() {
	local -n n1=n2 n2=n3 n3=n4 n4=n5 n5=n6 n6=n7 n7=n8 n8=n9 n9=n10 n10=n1
	local n1=5; echo "st=$?"; declare -p n1
}; ( tn )
echo "--- only the operand that carried it is dropped"
to() { local -n g=z; local -n z=g; local g=5 h=6; echo "st=$?"; declare -p g h; }; ( to )
# `-n` asks something else and says so before any walk is made. (`+n` is a
# third thing again: the check is excluded by `(flags_off & att_nameref) == 0`,
# and the refusal comes from the store instead, declare.def:990 — see
# TD-OILS-A-PLUS-N-DECLARATION-STORES-THROUGH-THE-REFERENCE-IT-IS-REMOVING.)
echo "--- the -n spelling asks something else, and says so differently"
tp() { local -n g=z; local -n z=g; local -n g=5; echo "st=$?"; declare -p g; }; ( tp )
echo "--- a chain that reaches a name is not this, and stores as it always did"
t1r() { local w=1; local -n g=w; local g=5; echo "st=$?"; declare -p g w; }; ( t1r )
ts() { local -n g=nosuch; local g=5; echo "st=$?"; declare -p g nosuch; }; ( ts )
echo "--- nor is a fresh local shadowing a cycle that lives at global scope"
declare -n gg=zz; declare -n zz=gg
tt() { local gg=5; echo "st=$?"; declare -p gg; }; ( tt; echo AFTER; declare -p gg zz )
echo "--- nor a declaration that takes the operand off the local branch"
tu() { local -n g=z; local -n z=g; local -g g=5; echo "st=$?"; declare -p g; }; ( tu )
tv() { local -n g=z; local -n z=g; readonly g=5; echo "st=$?"; declare -p g; }; ( tv )
tw() { local -n g=z; local -n z=g; export g=5; echo "st=$?"; declare -p g; }; ( tw )
echo "--- nor a top-level one, which is no local at all"
( declare -n gq=zq; declare -n zq=gq; declare gq=5; echo "st=$?"; declare -p gq )
( declare -n gq=zq; declare -n zq=gq; declare gq=h; echo "st=$?"; declare -p gq )
echo TAIL
