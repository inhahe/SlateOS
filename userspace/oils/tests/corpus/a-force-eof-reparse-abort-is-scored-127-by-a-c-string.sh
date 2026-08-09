# A re-parse that fails inside `parse_comsub` ends with
# `jump_to_top_level (FORCE_EOF)` (parse.y:4185) — the two shapes are a nested
# `$( … )` whose re-print will not parse back, and a compound array assignment,
# whose whole value list is re-read by `assign_compound_array_list`. What that
# jump is *worth* is not a property of the error. It is a property of whichever
# `setjmp (top_level)` catches it, and `-c` installs one that scores the entire
# class alike:
#
#   case FORCE_EOF:
#     ...
#     return last_command_exit_value = 127;        /* shell.c:1446 */
#
# A script's `reader_loop` has no such arm, so there the syntax error's own
# `last_command_exit_value` stands — `EX_BADSYNTAX` (2) at the top level, and
# `EX_BADUSAGE` (1) where an `eval`/`source` is executing the builtin that
# scored it. So the same input is 127 under `-c` and 2 in a file.
#
# Three things answer before any of that:
#
#   * anything that catches the jump *first* — a `( … )`, a `$( … )` — answers
#     in `-c`'s place, and the shell then falls back to the script rule exactly.
#     A brace group and a function body install no handler, so they keep 127.
#   * a backtick body has no reader to unwind to and no invocation mode to key
#     off: bash abandons the rest of the body, substitutes what it captured, and
#     leaves `$?` at 1.
#   * under errexit the shell never reaches the jump at all. `parser_error`
#     (error.c) ends with a *direct* call, made before the class of the failure
#     has been decided:
#
#       if (exit_immediately_on_error)
#         exit_shell (last_command_exit_value = 2);
#
#     — so there is nothing for a `||` to catch, the status is the syntax
#     error's 2 rather than the command's, and only the *first* line of the
#     diagnostic is printed.
#
# `$BASH` is the running shell either way, so every case re-invokes it and
# strips the leading program name, which naturally differs.

s() { sed 's/^.*: line /line /'; }

# Merged into a *pipe* rather than captured, so the lines arrive in write order
# and `${PIPESTATUS[0]}` still yields the re-invoked shell's own status.
o() { "$BASH" --norc "$@" 2>&1 | s; echo "rc=${PIPESTATUS[0]}"; }

echo "=== a -c string scores the whole class 127"
o -c 'a=( "p$(
!
)q" ); echo no'
o -c 'v="$( echo "x$(
!
)y" )"; echo no'
o -c 'eval '\''a=( "p$(
!
)q" )'\''; echo no'
o -c '{ a=( "p$(
!
)q" ); }; echo no'
# A function defined in a `-c` string is read under the name `environment`, and
# the body installs no handler, so the 127 stands.
o -c 'f() { a=( "p$(
!
)q" ); }; f; echo no'

echo "=== a script keeps the syntax error's own status"
cat > s.sh <<'E'
a=( "p$(
!
)q" ); echo no
E
o s.sh
# Under an `eval` the syntax error is `EX_BADUSAGE` — the status a builtin's
# *usage* error carries — because a builtin is what is executing it.
cat > e.sh <<'E'
eval 'a=( "p$(
!
)q" )'; echo no
E
o e.sh
cat > f.sh <<'E'
f() { a=( "p$(
!
)q" ); }; f; echo no
E
o f.sh
cat > n.sh <<'E'
v="$( echo "x$(
!
)y" )"; echo no
E
o n.sh

echo "=== whatever catches the jump first answers in -c's place"
o -c '( a=( "p$(
!
)q" ) ); echo "outer rc=$?"'
o -c 'eval '\''( a=( "p$(
!
)q" ) )'\''; echo "outer rc=$?"'
cat > p.sh <<'E'
( a=( "p$(
!
)q" ) ); echo "outer rc=$?"
E
o p.sh

echo "=== a backtick body is 1 wherever it is written"
o -c 'x=`a=( "p$(
!
)q" )`; echo "rc=$? x=[$x]"'
cat > b.sh <<'E'
x=`a=( "p$(
!
)q" )`; echo "rc=$? x=[$x]"
E
o b.sh

echo "=== errexit leaves before the class is decided, with one line"
o -ec 'a=( "p$(
!
)q" ); echo no'
o -ec 'v="$( echo "x$(
!
)y" )"; echo no'
# Not a conditional exit: a `||` catches nothing, because `exit_shell` is not a
# jump.
o -ec 'v="p$(
!
)q" || echo B
echo C'
o -ec 'a=( "p$(
!
)q" ) || echo B
echo C'
cat > x.sh <<'E'
set -e
( v="p$(
!
)q" ); echo "outer rc=$?"
E
o x.sh

echo "=== and the recoverable shape keeps its own 1"
# A re-print that fails on its own — not inside a nested `$( … )` — is
# `parse_string`'s `DISCARD`, which abandons only this parse unit.
o -c 'v="p$(
!
)q"; echo no'
cat > d.sh <<'E'
v="p$(
!
)q"
echo "rc=$?"
E
o d.sh
echo "=== done"
