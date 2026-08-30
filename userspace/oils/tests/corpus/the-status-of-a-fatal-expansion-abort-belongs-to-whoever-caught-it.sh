# A fatal word-expansion error — a `set -u` reference to an unset variable, a
# `${var:?word}`, a parse error in a `$( … )` body — is not a status a command
# returns. It is `jump_to_top_level (FORCE_EOF)`, a `longjmp`, and the status
# comes from whichever `setjmp (top_level)` catches it. The `-c` path's own
# handler answers 127:
#
#   shell.c:1446  run_one_command
#     code = setjmp_nosigs (top_level);
#     …
#       case FORCE_EOF:
#         return last_command_exit_value = 127;
#
# so 127 is what you see when *nothing else* was in the way. bash installs a
# handler in only two other places, and both answer 1:
#
#   execute_cmd.c:1703  execute_in_subshell
#     else if (result)
#       return_code = (last_command_exit_value == EXECUTION_SUCCESS)
#                     ? EXECUTION_FAILURE : last_command_exit_value;
#
#   execute_cmd.c:5379  execute_subshell_builtin_or_function
#     result = setjmp_nosigs (top_level);
#     … else if (result) subshell_exit (EXECUTION_FAILURE);
#
# The second of those is the surprise, because it is the **builtin** branch
# only: the `execute_function` call standing right beside it in the same
# function has no `setjmp` in front of it. So a pipeline stage that is an
# `eval`/`.`/`source` answers 1 while a stage that is a *function call* answers
# 127 — even though `$BASH_SUBSHELL` reads 1 in both. Depth is not the question;
# interception is.
#
# `execute_in_subshell` is entered for a `( … )`, for anything flagged
# `CMD_WANT_SUBSHELL|CMD_FORCE_SUBSHELL`, and for a `shell_control_structure`
# (`execute_cmd.c:431` — the compound commands, so `{ … }`, `if`, `for`, `while`,
# `case`, `[[`, `((`) that is being piped or run asynchronously. A *simple*
# command is none of those: `execute_simple_command` has already forked
# (`already_forked = 1`) before it calls `expand_words`, so its expansion fails
# with no handler between it and the top and keeps the 127. That is also why a
# builtin stage's own *word* expansion is 127 while a failure inside the
# builtin's body is 1.
#
# The one place an and-or list differs from a simple command is `&`:
# `execute_connection` sets `CMD_FORCE_SUBSHELL` on an asynchronous `AND_AND`/
# `OR_OR`, so `a && b &` is caught (1) where `b &` alone is not (127).
#
# None of this is visible when the shell is reading a *file*: `reader_loop`'s
# handler answers with the last command's status, so every case below would be
# 1. The cases therefore re-invoke `$BASH` with `-c`, which is the only reader
# that has a 127 to give.

# TIMEOUT: 90

c() {
  out=$("$BASH" --norc -c "$1" 2>/dev/null </dev/null); rc=$?
  printf '%-56s [%s] rc=%s\n' "$1" "$out" "$rc"
}

echo '=== nothing in the way: 127'
c 'set -u; echo "$nope"'
c 'set -u; { echo "$nope"; }'
c 'set -u; if true; then echo "$nope"; fi'
c 'set -u; f(){ echo "$nope"; }; f'
c 'set -u; echo ${nope:?boom}'
c 'set -u; echo "${!nope}"'

echo '=== a ( … ) catches it, and so does eval'
c 'set -u; ( echo "$nope" )'
c 'set -u; eval "echo \$nope"'
c 'set -u; f(){ ( echo "$nope" ); }; f; echo "a=$?"'
c 'set -u; f(){ echo "$nope"; }; ( f ); echo "a=$?"'
c 'set -u; x=$(echo "$nope"); echo "sub=$?"'

echo '=== a pipeline stage decides by its own shape'
c 'set -u; echo a | echo "$nope"'
c 'set -u; echo a | printf "%s" "$nope"'
c 'set -u; echo a | cat > $nope'
c 'set -u; f(){ echo "$nope"; }; echo a | f'
c 'set -u; echo a | { echo "$nope"; }'
c 'set -u; echo a | ( echo "$nope" )'
c 'set -u; echo a | if true; then echo "$nope"; fi'
c 'set -u; echo a | while read x; do echo "$nope"; done; echo "st=$?"'
c 'set -u; echo a | eval "echo \$nope"'
c 'set -u; echo a | source /dev/stdin <<< "echo \$nope"; echo "st=$?"'
c 'set -u; echo a | echo ${nope:?boom}'
c 'set -u; echo a | echo "${nope[0+]}"'
c 'set -u; echo a | { f(){ echo "$nope"; }; f; }; echo "a=$?"'
c 'set -u; echo a | echo "$nope" | cat; echo "st=$?"'

echo '=== and so does an & job, which is forked the same way'
c 'set -u; echo "$nope" & wait $!; echo "a=$?"'
c 'set -u; true; echo "$nope" & wait $!; echo "a=$?"'
c 'set -u; { echo "$nope"; } & wait $!; echo "a=$?"'
c 'set -u; ( echo "$nope" ) & wait $!; echo "a=$?"'
c 'set -u; f(){ echo "$nope"; }; f & wait $!; echo "a=$?"'
c 'set -u; eval "echo \$nope" & wait $!; echo "a=$?"'
c 'set -u; true && echo "$nope" & wait $!; echo "a=$?"'
c 'set -u; ! echo "$nope" & wait $!; echo "a=$?"'
c 'set -u; for i in 1; do echo "$nope"; done & wait $!; echo "a=$?"'

echo '=== a parse error in a $( … ) body is the same abort, and counts the same'
c 'echo "$(for)"; echo "st=$?"'
c 'eval "echo \$(for)"; echo "st=$?"'
c '( eval "echo \$(for)" ); echo "st=$?"'
c 'echo a | echo "$(for)"; echo "st=$?"'
c 'echo a | eval "echo \$(for)"; echo "st=$?"'
c 'echo a | { eval "echo \$(for)"; }; echo "st=$?"'
c 'echo a | ( eval "echo \$(for)" ); echo "st=$?"'
c 'f(){ eval "echo \$(for)"; }; echo a | f; echo "st=$?"'
c 'eval "echo \$(for)" & wait $!; echo "a=$?"'
c 'f(){ eval "echo \$(for)"; }; f & wait $!; echo "a=$?"'

echo '=== done'
