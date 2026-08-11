# `read -a` and `mapfile` do not name their array the way an assignment does.
# They hand the operand to `builtin_find_indexed_array` (builtins/common.c:1029),
# which opens with `find_or_make_array_variable` (arrayfunc.c:465):
#
#	  var = find_variable (name);
#	  if (var == 0)
#	    {
#	      /* See if we have a nameref pointing to a variable that hasn't been
#		 created yet. */
#	      var = find_variable_last_nameref (name, 1);
#	      …
#	    }
#	  if (var == 0)
#	    var = (flags & 2) ? make_new_assoc_variable (name) : make_new_array_variable (name);
#	  else if ((flags & 1) && (readonly_p (var) || noassign_p (var)))
#	    { … err_readonly (name); return ((SHELL_VAR *)NULL); }
#
# — a plain `find_variable`, the *read* walk, and then a `SHELL_VAR *` held for
# the whole fill. Two things follow. Every question the builtin asks is asked of
# **that** variable, not of whatever the name means where the builtin stands;
# and the fill lands there too. Where the chain escaped a cycle to global scope
# (`find_global_variable_noref`, variables.c:2098) the two differ, because the
# local reference is still what the name means everywhere else — including
# inside a `-C` callback, which is ordinary shell code.
exec 2>&1
echo "--- the fill lands on the global the cycle escaped to"
g1=(A B)
f1() { local -n g1=g1; read -a g1 <<< "P Q"; declare -p g1; }
f1 2>/dev/null; declare -p g1
g4=(A B)
f4() { local -n g4=g4; mapfile -t g4 <<< "P"; declare -p g4; }
f4 2>/dev/null; declare -p g4
echo '--- and the reference survives it, -a and -n never held at once'
f5() { local -n g5=g5; read -a g5 <<< "P"; declare -p g5; }
g5=(A); f5 2>/dev/null; declare -p g5
echo "--- the refusals are the escaped variable's, not the reference's"
readonly rg=(A B)
c1() { local -n rg=rg; read -a rg <<< "P"; echo "st=$?"; declare -p rg; }
c1 2>/dev/null; declare -p rg
declare -A ag=([k]=v)
c2() { local -n ag=ag; read -a ag <<< "P"; echo "st=$?"; }
c2 2>/dev/null; declare -p ag
echo "--- as are the value attributes"
declare -ai ig=(1)
c5() { local -n ig=ig; read -a ig <<< "3+4 5*2"; }
c5 2>/dev/null; declare -p ig
echo "--- -O overwrites in place, and it is the global it overwrites"
mg=(x y z w v)
c3() { local -n mg=mg; mapfile -t -O 1 mg <<< $'a\nb\nc'; }
c3 2>/dev/null; declare -p mg
echo "--- but a callback is ordinary code and reads the name the ordinary way"
cg=(x)
cb() { echo "  cb($1) cg=${cg[*]}"; }
c4() { local -n cg=cg; mapfile -t -C cb -c 1 cg <<< $'a\nb'; }
c4 2>/dev/null; declare -p cg
echo "--- an ordinary reference is followed as it always was"
o1() { local zz=(A B); local -n r=zz; read -a r <<< "P Q"; declare -p zz r; }
o1
declare -n G=zz2
i2() { local zz2=(A B); read -a G <<< "P Q"; declare -p zz2; }
i2; declare -p zz2
echo TAIL
