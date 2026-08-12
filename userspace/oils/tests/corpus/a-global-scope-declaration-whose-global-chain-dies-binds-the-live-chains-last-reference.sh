# A declaration made at global scope from inside a function does not always bind
# the name it was written with. bash decides the name in two questions
# (`do_compound_assignment`, subst.c:3494):
#
#     v = chklocal ? find_variable (name) : 0;
#     if (v && (local_p (v) == 0 || v->context != variable_context)) v = 0;
#     if (v == 0) v = find_global_variable (name);
#     …
#     newname = (v == 0) ? nameref_transform_name (name, flags) : name;
#
# `find_global_variable` (variables.c:2400) reads only the *first* link from the
# global table; every link after it is an ordinary innermost-first lookup, so the
# chain can re-enter local scope. It answers NULL three ways: no global binding,
# a chain ending on a name nothing answers to, and a chain that closes on itself
# or outruns NAMEREF_MAX. Where it does, the name bound is the **live** chain's
# last reference cell — `find_variable_last_nameref` — bound globally all the
# same. So a frame-local reference pointing out of the frame decides which global
# a `declare -g` writes, whenever the global side of the lookup is empty.
#
# Both halves of a compound declaration reach that rule, by different roads. The
# assignment half is the line above. The builtin half gets there two steps later:
# `find_global_variable_last_nameref` (declare.def:735) walks the global table
# *alone*, so a cycle leaves it with nothing and the name falls through unchanged
# to `bind_global_variable`, whose `bind_variable_internal` (variables.c:3020)
# opens with
#
#     if (entry && nameref_p (entry) && … && table == global_variables->table)
#       {
#         entry = find_global_variable (entry->name);
#         if (entry == 0) entry = find_variable_last_nameref (name, 0);
#
# — the same live-chain cell. That opening is reached only through the global
# table's own entry for the name, which is the one thing the two roads agree on:
# a name with **no global binding at all** is bound exactly as written.

echo '=== a global cycle, and a frame-local reference out of the frame'
( declare -n g=z; declare -n z=g
  f() { local -n g=w; declare -g g=1; declare -p g; }; f; echo AFTER; declare -p g z w )
( declare -n g=z; declare -n z=g
  f() { local -n g=w; declare -g g; declare -p g; }; f; echo AFTER; declare -p g z w )

echo '=== a longer chain closing behind the name reads the same'
( declare -n g=z; declare -n z=y; declare -n y=z
  f() { local -n g=w; declare -g g=1; declare -p g; }; f; echo AFTER; declare -p g z y w )
( declare -n g=z; declare -n z=y; declare -n y=g
  f() { local -n g=w; declare -g g=1; declare -p g; }; f; echo AFTER; declare -p g z y w )

echo '=== a chain that merely ends on an unset name reads the same'
( declare -n g=z
  f() { local -n g=w; declare -g g=1; declare -p g; }; f; echo AFTER; declare -p g z w )

echo '=== no global binding at all: the name is bound as written'
( f() { local -n g=w; declare -g g=1; declare -p g; }; f; echo AFTER; declare -p g )
( f() { local -n g=w; readonly g=(1 2); declare -p g; }; f; echo AFTER; declare -p g w )

echo '=== a global that is no reference is found, so nothing is transformed'
( g=old
  f() { local -n g=w; declare -g g=1; declare -p g; }; f; echo AFTER; declare -p g )

echo '=== a live chain reaching a variable is found too, wherever it ends'
( declare -n g=z; z=Z
  f() { local -n g=w; declare -g g=1; declare -p g; }; f; echo AFTER; declare -p g z )

echo '=== the local shadow of the name is what chklocal asks about, not the chain'
( declare -n g=z; declare -n z=g
  f() { local g=5; declare -g g=1; declare -p g; }; f; echo AFTER; declare -p g z )

echo '=== a bare assignment takes the escape, not this rule'
( declare -n g=z; declare -n z=g
  f() { local -n g=w; g=1; declare -p g; }; f; echo AFTER; declare -p g z w )

echo '=== unset walks the same chain and reaches the same place'
( declare -n g=z; declare -n z=g; w=W
  f() { local -n g=w; unset g; echo "rc=$?"; }; f; echo AFTER; declare -p g z )
