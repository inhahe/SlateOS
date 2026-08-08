# bash parses a `$( … )` body *twice*, and only the first parse sees the source.
# At parse time `parse_comsub` throws the text away and keeps a re-print of the
# parsed command (parse.y:4219, `tcmd = print_comsub (parsed_command)`), and at
# *expansion* time `command_substitute` runs the stored string through the
# parser again — `xparse_dolparen` (subst.c:1290, 1322), with `)` as
# `shell_eof_token`.
#
# Almost every re-print parses back, so the second pass is invisible. Two
# productions do not, because they are the only two that drop their own
# terminator: bash's grammar admits a bare prefix only as
#
#   pipeline: BANG pipeline | timespec pipeline
#   list_terminator: '\n' | ';' | yacc_EOF
#
# so `!` on a line by itself is `BANG list_terminator` — and the deparse of that
# is `!`, with no terminator at all. Fed back with the `)` appended, `! )` is a
# syntax error the first parse never saw. `time` behaves the same way.
#
# What the failure *does* depends on which parse raised it, and bash raises the
# two from different depths:
#
#   * the re-print's own grammar error is `parse_string`'s, which returns
#     `-DISCARD` (builtins/evalstring.c:686-694); `xparse_dolparen` re-raises it
#     as `jump_to_top_level (DISCARD)` (parse.y:4325). One parse unit is lost,
#     `$?` is 1, and an enclosing `eval` catches it.
#
#   * a `$( … )` *nested* in the re-print that will not parse is raised by
#     `parse_comsub` itself, which for a non-interactive shell does
#
#         if (interactive_shell == 0)
#           jump_to_top_level (FORCE_EOF);  /* This is like reader_loop() */
#                                             (parse.y:4185)
#
#     — the shell ends. `eval` does not contain it, and a subshell exits 2.
#
# The reported line is the *command's* line plus one, not the line the `)` is
# on: `parse_string` calls `push_stream (0)` and so does not renumber, leaving
# bash's one shared `line_number` running on from wherever the executor set it
# (`SET_LINE_NUMBER (command->value.Simple->line)`, execute_cmd.c:864). That
# usually looks like counting off the `)` only because a simple command's line
# is stamped in `make_bare_simple_command` (make_cmd.c:505) at the reduction
# that needs the *next* word as lookahead — so reading the very word holding the
# substitution is what set it. A redirect operator and a `case` (execute_cmd.c:
# 3545) separate the two, and both sections below say `line 2` past their own.
#
# The echoed line is not the body either: it is the body's last line, the `)`,
# and then whatever followed the substitution inside the same *word* — because
# `expand_word_internal` hands `extract_command_subst` the whole remainder of
# the stored word, closing quote included.

echo "=== a bare ! is re-printed without its terminator, so the re-parse fails"
v=p$(
!
)q; echo "not reached"
echo "after rc=$?"

echo "=== the rest of the word is echoed with it, and only the word"
echo "A[$(
!
)]B" more args
echo "after rc=$?"

echo "=== including a parameter expansion's closing brace"
echo ${x:-$(
!
)}tail
echo "after rc=$?"

echo "=== time is the other production that drops its terminator"
v=$(
time
)
echo "after rc=$?"

echo "=== a body that parses back is untouched: only the deparse decides"
v=$(
! false
)
echo "[$v] rc=$?"
v=$( { time true; } 2>/dev/null )
echo "[$v] rc=$?"

echo "=== a word the shell builds at expansion time is read once, not twice"
# `${x@P}` and the prompt strings never reach `parse_comsub` at all:
# `expand_string` walks the value with `expand_word_internal`, which hands
# `command_substitute` the raw text. There is no re-print, so there is nothing
# to fail to parse back — the very body that is a syntax error written in the
# script is silent here, because the raw `<newline>!<newline>` does parse.
x=$'$(\n!\n)'
printf "[%s] rc=%s\n" "${x@P}" "$?"
PS4=$'$(\n!\n)+ '
set -x
true
set +x
echo "prompt rc=$?"

echo "=== the line is the command's, which lookahead usually leaves at the )"
echo "L $LINENO"
v=p$(
!
)q
echo "after rc=$?"

echo "=== but a redirect stamps the command before the word is even read"
echo "L $LINENO"
cat > "/dev/null$(
!
)x" <<< hi
echo "after rc=$?"

echo "=== and a case is stamped on its own line, not the pattern's"
echo "L $LINENO"
case x in
"y$(
!
)z") echo pat ;;
esac
echo "after rc=$?"

echo "=== a nested body that will not parse ends the shell, so a subshell exits 2"
( echo "x$(echo "y$(
!
)z")w" )
echo "subshell rc=$?"

echo "=== eval contains the plain discard"
eval 'v=p$(
!
)q; echo "not reached"'
echo "after eval rc=$?"

echo "=== but not the nested one, which is raised below eval's guard"
f() { eval 'v=$(echo "y$(
!
)z")'; echo "not reached"; }
f
echo "still here rc=$?"
