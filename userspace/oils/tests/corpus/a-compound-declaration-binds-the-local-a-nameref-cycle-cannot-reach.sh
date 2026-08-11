# A compound operand of a declaration builtin is **three commands**, not one
# (`expand_declaration_argument`, subst.c:12655):
#
#	  1. make_internal_declare (word, opts, cmd)   -- the name alone
#	  2. do_word_assignment (tlist->word, 0)       -- the compound assignment
#	  3. tlist->word->word[t] = '\0'               -- the builtin, name only
#
# Step 1 rebuilds the option string out of the array kind (`-a`/`-A`), the
# scope (`-g`, `-G` for the chklocal builtins) and the value-transforming
# letters, so inside a function `local g=(1 2)` first runs `declare -- g`,
# which *binds a local*. Step 2's `do_compound_assignment` (subst.c:3459) then
# assigns to that local — never to whatever a nameref cycle's escape names —
# and step 3 finds an array and follows nothing.
#
# Each step walks the operand's chain, and each walk warns once, so the count
# is what the decomposition is measured by:
#
#	   local, -a/-A          2      top level, -a/-A          1
#	   local, no kind letter 4      top level, no kind letter 3
#	   -g in a function      0      readonly/export in one    3
#
# A kind letter costs step 2 nothing because step 1 has already converted the
# name to an array (`make_local_array_variable`), overwriting the nameref's
# value cell with the `ARRAY *`; there is no name left to follow.
exec 2>&1
echo "--- a local compound: the array is the frame's, and nothing escapes"
f1() { local -n g=z; local -n z=g; local g=(1 2); declare -p g; }
f1; echo "AFTER"; declare -p g
echo "--- a kind letter asks for one walk less on each side"
f2() { local -n g=z; local -n z=g; local -a g=(1 2); declare -p g; }
f2; echo "AFTER"; declare -p g
f3() { local -n g=z; local -n z=g; local -A g=([k]=v); declare -p g; }
f3; echo "AFTER"; declare -p g
echo "--- and a value-transforming letter does not count as one"
f4() { local -n g=z; local -n z=g; local -i g=(1 2); declare -p g; }
f4; echo "AFTER"; declare -p g
echo "--- an append lands in the same place, and carries no held name in"
f5() { local -n g=z; local -n z=g; local g+=(1 2); declare -p g; }
f5; echo "AFTER"; declare -p g
f6() { local -n g=z; local -n z=g; local -a g+=(1 2); declare -p g; }
f6; echo "AFTER"; declare -p g
echo "--- at top level there is no step-1 local, so the counts are one less"
( declare -n gq=zq; declare -n zq=gq; declare gq=(1 2); declare -p gq zq )
( declare -n gq=zq; declare -n zq=gq; declare -a gq=(1 2); declare -p gq zq )
( declare -n gq=zq; declare -n zq=gq; declare -A gq=([k]=v); declare -p gq zq )
echo "--- -g asks for the global outright and walks not at all"
g1() { local -n g=z; local -n z=g; declare -g g=(1 2); }
( g1; declare -p g )
g2() { local -n g=z; local -n z=g; declare -ga g=(1 2); }
( g2; declare -p g )
echo "--- a chain that reaches something is walked as often and says nothing"
h1() { local w=1; local -n g=w; local g=(1 2); declare -p g w; }; ( h1 )
h2() { local w=1; local -n g=w; local -a g=(1 2); declare -p g w; }; ( h2 )
echo "--- a fresh local shadowing a *global* cycle pays the same"
declare -n gr=zr; declare -n zr=gr
k1() { local gr=(1 2); declare -p gr; }; ( k1 )
k2() { local -a gr=(1 2); declare -p gr; }; ( k2 )
k3() { local -A gr=([k]=v); declare -p gr; }; ( k3 )
echo "--- a chain longer than the walk limit keeps the reference attribute"
n1() { local -n n1=n2; local -n n2=n3; local -n n3=n4; local -n n4=n5
       local -n n5=n6; local -n n6=n7; local -n n7=n8; local -n n8=n9
       local -n n9=na; local -n na=n1; local n1=(1 2); declare -p n1; }
( n1 )
echo "--- a bare compound is not a declaration and does follow the escape"
b1() { local -n g=z; local -n z=g; g=(1 2); declare -p g; }
( b1; echo AFTER; declare -p g )
echo TAIL
