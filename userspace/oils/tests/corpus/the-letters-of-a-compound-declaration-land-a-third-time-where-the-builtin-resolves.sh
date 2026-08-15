# A compound declaration is **three** commands, and the third one is a real
# operand of the real builtin.
#
# `expand_declaration_argument` (subst.c:12653) hands the builtin the operand
# truncated at the `=` (subst.c:12493), so `declare -gGa g=(1 2)` ends in a
# genuine `declare -gGa g` — the same command text step 1 was, run after the
# whole word-expansion pass. Every letter it carries lands wherever *that*
# command's own lookup goes, which is not necessarily where the literal went:
# only the builtin half reads the uppercase `G` (execute_cmd.c:4246), and only
# it makes the `mkglobal` restart (declare.def:735).
#
# The word has no `=`, so step 3 assigns nothing. It sets the letters **as
# attributes only** — never re-folding and never re-evaluating what the variable
# already holds — and converts the shape only when a kind letter was given,
# carrying an old scalar in as element 0, exactly as `declare -a` over an
# existing scalar does.

echo '=== the letters land, the value is left alone'
( g=old; f() { declare -gGa g; declare -p g; }; f; echo AFTER; declare -p g )
( g=2+3; f() { declare -gGi g; declare -p g; }; f )
( g=abC; f() { declare -gGu g; declare -p g; }; f )

echo '=== and the same with a literal in front of them'
# The fold and the arithmetic belong to the *assignment*, which step 2 already
# made; step 3 only marks the name.
( f() { local g=Ab7+1; declare -gGl g=(1 2); declare -p g; }; f )
( f() { local g=Ab7+1; declare -gGA g=(1 2); declare -p g; }; f )
( f() { local g=Ab7+1; declare -gGi g=(1 2); declare -p g; }; f )
( f() { local g=Ab7+1; declare -gGu g=(1 2); declare -p g; }; f )

echo '=== step 1 sets the letters before step 2 stores, so the elements do fold'
( g=abC; f() { declare -gGu g=(x yZ); declare -p g; }; f )
( g=2+3; f() { declare -gGi g=(4+5 6); declare -p g; }; f )

echo '=== the frame own binding is step 3 target when nothing moves it'
( g=old
  f() { local g=loc; declare -gGa g=(1 2); declare -p g; }; f; echo AFTER; declare -p g )
# Without the uppercase letter there is no `chklocal` to keep it at home — and
# no shape change either, the builtin having gone out to the global.
( g=old
  f() { local g=loc; declare -ga g=(1 2); declare -p g; }; f; echo AFTER; declare -p g )

echo '=== a restart carries step 3 past the frame own binding'
# `declare_find_variable (name, mkglobal, chklocal)` is asked of the name the
# restart arrived at, not the name as written (declare.def:735-774), so the
# frame's `g` is never consulted and its letters go out to `z`.
( declare -n g=z
  f() { local g=5; declare -gGa g=(1 2); declare -p g; }; f; echo AFTER; declare -p z )
( declare -n g=z
  f() { local g=5; declare -ga g=(1 2); declare -p g; }; f; echo AFTER; declare -p z )

echo '=== a chain that re-enters a local runs through it, and is no cycle'
# `find_global_variable` reads only its first link from the global table
# (variables.c:2400); every link after it is an ordinary innermost-first
# lookup, so `g(global)->z->g(local)->w` answers `w` and warns about nothing.
( declare -n g=z; declare -n z=g
  f() { local -n g=w; declare -g g=(1 2); }; f 2>&1; declare -p g w )
