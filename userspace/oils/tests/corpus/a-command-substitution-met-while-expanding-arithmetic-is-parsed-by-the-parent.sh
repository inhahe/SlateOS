# A `$( … )` **written** in source and one **met while expanding** an
# arithmetic string are read by different code, and the difference shows.
#
# The written one is parsed once, at parse time, and run in a subshell that
# re-reads its body a command at a time — so a syntax error half way down it is
# the child's to report, after the commands above it have already run.
#
# The other is reached by `extract_command_subst` (subst.c:1290), which hands
# `string + *sindex` — the whole rest of the arithmetic string — to
# `xparse_dolparen` (parse.y:4248). That calls
# `parse_string (string, "command substitution", …)`, so **the parser** decides
# both where the substitution ends and whether it is well-formed, and it decides
# them in the *parent*, before `command_substitute` forks. Four things follow,
# and each has its own group below.
#
# Where it ends is not "the matching `)`": a `)` that closes a `case` pattern
# closes nothing, and the substitution runs on to the one after `esac`.
#
# The body having been parsed before anything ran, a body that will not parse
# runs *nothing* — not even the commands written ahead of the error.
#
# The failure is `jump_to_top_level (DISCARD)` (parse.y:4330). The rest of the
# word, and of the command holding it, is given up with `$?` of 1, and the shell
# reads on — through a function body, an `eval` and a subshell alike. Posix mode
# does not promote it, because it is a parse failure and never reaches the "an
# expansion error ends the shell" hook. `set -e` does, but by another road
# entirely: `parser_error` (error.c) ends with
#
#   if (exit_immediately_on_error)
#     exit_shell (last_command_exit_value = 2);
#
# so the shell is gone at the *first* diagnostic line — no echoed source line,
# and a parse error's 2 rather than errexit's usual 1.
#
# And what the complaint names is what `parse_string` was reading. That is the
# whole remainder, so the echoed source line runs on past the `)`; and
# `parse_string` opens it with `push_stream (0)` (builtins/evalstring.c:627),
# which does *not* reset the line number while the first line fetched charges
# one anyway — so every line it reports is one past the body's true line. Only
# the parse is shifted: the body's *run* is the child's, and lands on the
# enclosing line like a backtick's.
#
# Verified against bash 5.2.37.

echo "=== the extent is the parser's, so a case pattern's ) closes nothing"
echo $(( '$(case x in x) echo 7;; esac)' + 0 ))
echo $(( '$( (echo 8) )' + 0 ))
echo $(( '$(echo ")")' + 0 ))
echo $(( '$(echo 9)' + 1 ))

echo "=== the body is parsed in the parent, so a bad one runs nothing"
echo $(( '$(echo RAN >&2; fi)' + 0 ))
echo "rc=$?"
echo $(( '$(echo RAN2 >&2
fi)' ))
echo "rc=$?"

echo "=== the complaint names the whole remainder, one line on"
echo $(( '$(fi)' + 1 ))
echo "rc=$?"
echo $(( '$(
fi)' ))
echo "rc=$?"
echo "$(( '$(fi)' ))"tail
echo "rc=$?"

echo "=== what was written to the left of the arithmetic has still run"
echo $(echo ran >&2)$(( '$(fi)' ))
echo "rc=$?"

echo "=== the discard gives up the command, and a function body with it"
f() { echo $(( '$(fi)' )); echo "not reached"; }
f
echo "rc=$?"
eval 'echo $(( '\''$(fi)'\'' ))'
echo "rc=$?"
( echo $(( '$(fi)' )) )
echo "sub rc=$?"

echo "=== a failure in a body that did parse is the child's, and is not fatal"
echo $(( '$(nosuchcmd_zz)' + 0 ))
echo "rc=$?"
echo $(( '$(exit 3)' + 0 ))
echo "rc=$?"

echo "=== posix mode does not make it fatal"
set -o posix
echo $(( '$(fi)' ))
echo "posix rc=$?"
set +o posix

echo "=== errexit does, with a parse error's 2 and the one line"
( set -e; echo $(( '$(fi)' )); echo "not reached" )
echo "errexit sub rc=$?"
echo "still here"

# Last, because it ends the shell: the same under errexit at the top level.
set -e
echo $(( '$(fi)' ))
echo "not reached"
