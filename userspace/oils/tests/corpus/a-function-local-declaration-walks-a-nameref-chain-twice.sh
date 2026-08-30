# A `declare`/`local` that binds a **function-local** walks the operand's
# nameref chain a fixed number of times before it binds anything — twice
# normally, three times for a valueless `-n` — whatever letters it carries and
# whether or not it makes an array. Each walk warns once when the chain is
# circular, so the count is visible.
#
# declare.def:601 puts the whole thing behind `if (variable_context && mkglobal
# == 0)`, and inside it —
#
#	  newname = declare_transform_name (name, flags_on, flags_off);
#	  …
#	    var = make_local_variable ((flags_on & att_nameref) ? name : newname, inherit_flag);
#
# — `declare_transform_name` opens with `var = find_variable (name)`
# (declare.def:212) and `make_local_variable` needs the same answer to know
# whether the `local` is a no-op (variables.c:2607):
#
#	  old_ref = find_variable_noref (name);
#	  …
#	  old_var = find_variable (name);
#
# `find_variable_noref` does not follow, so it is silent; the two
# `find_variable`s each walk the chain and each warn.
#
# A *valueless* `-n` takes a branch of its own (declare.def:618) that adds a
# third: `find_variable_last_nameref (name, 1)` makes no cycle test and says
# nothing, so the branch falls through to `else if ((var = find_variable (name))
# == 0 …)` and then to `make_local_variable`.
#
# At global scope — or under `-g` — the block is skipped entirely and none of
# these walks is made.
exec 2>&1
cyc() { eval "$1() { local -n g=z; local -n z=g; echo mid; $2; echo \"st=\$?\"; }"; ( "$1" ); }
echo "--- two walks whatever the letters ask for"
cyc t1 'local g'
cyc t2 'local -i g'
cyc t3 'local -x g'
cyc t4 'local -r g'
cyc t5 'local -I g'
cyc t6 'local -a g'
cyc t7 'local -A g'
echo "--- including the ones that make an array, which used to stop at one"
cyc t8 'local g[1]'
cyc t9 'local -a g=(1 2)'
cyc ta 'local -A g=([k]=v)'
echo "--- and the reference-letter forms, which follow nothing and walk anyway"
cyc tb 'local -n g=h'
cyc tc 'local +n g'
cyc td 'local -rn g=h'
cyc te 'local -n g=z'
echo "--- a valueless -n takes a third"
cyc tf 'local -n g'
cyc tg 'local -nr g'
cyc th 'local -In g'
echo "--- -g and a listing skip the block altogether"
cyc ti 'declare -g g'
cyc tj 'local -g g'
cyc tk 'declare -p g'
echo "--- the operand the walks are about, and the state they leave behind"
tl() { local -n g=z; local -n z=g; local -i g; declare -p g z; }; ( tl )
tm() { local -n g=z; local -n z=g; local -n g=h; declare -p g z; }; ( tm )
tn() { local -n g=z; local -n z=g; local +n g; declare -p g z; }; ( tn )
to() { local -n g=z; local -n z=g; local -a g=(1 2); declare -p g z; }; ( to )
echo "--- a self-naming reference re-declared: the lexical check, then the walks"
tp() { local -n g=g; local -n g=g; }; ( tp )
tq() { local -n g=g; local -n g=h; }; ( tq )
tr() { local -n g=g; local g; }; ( tr )
echo "--- global scope pays none of it"
( declare -n gq=zq; declare -n zq=gq; declare gq; echo "st=$?" )
( declare -n gq=zq; declare -n zq=gq; declare -n gq; echo "st=$?" )
( declare -n gq=zq; declare -n zq=gq; declare -i gq; declare -p gq zq )
echo "--- the reference need not be local: a fresh local shadowing a global cycle"
declare -n gr=zr; declare -n zr=gr
tu() { local gr; echo "st=$?"; declare -p gr; }; ( tu )
tv() { local -n gr; echo "st=$?"; declare -p gr; }; ( tv )
tw() { local -i gr; echo "st=$?"; declare -p gr; }; ( tw )
tx() { local gr=5; echo "st=$?"; declare -p gr; }; ( tx )
ty() { local -a gr=(1 2); echo "st=$?"; declare -p gr; }; ( ty )
echo "--- the lexical -n checks come first of all, above every walk"
# declare.def:520 puts them above the `restart_new_var_name` label, so a value
# the reference cannot name is refused before the branch that walks is entered
# and the walks are never made; and the self-reference warning, when the value
# is accepted, is printed ahead of them.
tza() { declare -n g=z; declare -n z=g; declare -n g="a b"; echo "st=$?"; }; ( tza )
tzb() { declare -n g=z; declare -n z=g; local -n g="1x"; echo "st=$?"; }; ( tzb )
tzc() { declare -n g=z; declare -n z=g; declare -n g+=x; echo "st=$?"; }; ( tzc )
tzd() { declare -n g=z; declare -n z=g; declare -n g=g; echo "st=$?"; declare -p g z; }; ( tzd )
echo "--- a chain that reaches something is walked as often and says nothing"
ts() { local w=1; local -n g=w; local -n g; declare -p g w; }; ( ts )
tt() { local w=1; local -n g=w; local -i g; declare -p g w; }; ( tt )
echo TAIL
