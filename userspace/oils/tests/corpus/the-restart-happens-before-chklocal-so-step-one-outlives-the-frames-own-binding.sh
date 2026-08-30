# The two commands a compound declaration decomposes into do not ask the
# `chklocal` question about the same name, and where the answers part the
# declaration leaves *two* variables behind.
#
# The builtin half puts the question last (`declare_internal`, declare.def:774):
#
#     if (var == 0)
#       var = declare_find_variable (name, mkglobal, chklocal);
#
# By then `name` may no longer be the name written. Three lines above it, the
# global-only walk `find_global_variable_last_nameref` has already run, and
# where it answered a reference to a name no global answers to, the whole
# command restarts on that name (`goto restart_new_var_name`, declare.def:768).
# So the frame's own binding is asked about the name the restart *arrived at*,
# and the restart itself never saw it.
#
# The compound literal asks the same question of the name as written
# (`do_compound_assignment`, subst.c:3494):
#
#     v = chklocal ? find_variable (name) : 0;
#
# So a frame-local `g` keeps the elements at home while the restart carries
# step 1 out to the global the chain died on — where, finding nothing, it makes
# the variable itself (declare.def:786-792). What is left there is empty, and
# carries the *shape* the kind letter named and nothing else: step 1's option
# string is spelled out of the word flags, which name a scope and a shape only
# (subst.c:12662-12745), so the `-r` of a `readonly` stays with the builtin
# proper and the array the restart made is writable.
#
# `chklocal` is `readonly`/`export`'s standing flag (execute_cmd.c:4221), so it
# is those two that reach this — and `declare -G`, which spells it by letter.

echo '=== the frame keeps the elements; the restart leaves an empty global behind'
( declare -n g=z
  f() { local g=5; readonly g=(1 2); declare -p g; }; f; echo AFTER; declare -p g z )
( declare -n g=z
  f() { local g=5; export g=(1 2); declare -p g; }; f; echo AFTER; declare -p g z )

echo '=== the kind letter chooses the shape of what is left, and only that'
( declare -n g=z
  f() { local g=5; readonly -a g=(1 2); declare -p g; }; f; echo AFTER; declare -p g z )
( declare -n g=z
  f() { local g=5; readonly -A g=([k]=v); declare -p g; }; f; echo AFTER; declare -p g z )
( declare -n g=z
  f() { local g=5; export -a g=(1 2); declare -p g; }; f; echo AFTER; declare -p g z )

echo '=== an array already in the frame takes the elements the same way'
( declare -n g=z
  f() { local -a g=(9); export -a g=(1 2); declare -p g; }; f; echo AFTER; declare -p g z )

echo '=== a longer chain restarts on the far end of it'
( declare -n g=z; declare -n z=y
  f() { local g=5; readonly -a g=(1 2); }; f; echo AFTER; declare -p g z y )

echo '=== with nothing to restart on, one name serves both commands'
( g=old
  f() { local g=5; readonly -a g=(1 2); declare -p g; }; f; echo AFTER; declare -p g )
( f() { local g=5; readonly -a g=(1 2); declare -p g; }; f; echo AFTER; declare -p g )

echo '=== a global cycle answers nothing, so there is no restart either'
( declare -n g=z; declare -n z=g
  f() { local g=5; readonly -a g=(1 2); declare -p g; }; f; echo AFTER; declare -p g z )

echo '=== without a frame-local binding the two agree and one variable is left'
( declare -n g=z
  f() { readonly -a g=(1 2); }; f; echo AFTER; declare -p g z )
( declare -n g=z
  f() { local -n g=w; readonly -a g=(1 2); }; f; echo AFTER; declare -p g z w )

echo '=== `declare` reaches it only by the letter that spells chklocal'
( declare -n g=z
  f() { local g=5; declare -gGa g=(1 2); declare -p g; }; f; echo AFTER; declare -p g z )
( declare -n g=z
  f() { local g=5; declare -ga g=(1 2); declare -p g; }; f; echo AFTER; declare -p g z )
