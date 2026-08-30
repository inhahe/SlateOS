# `parse_comsub` has two failure branches, and only one of them ends the shell
# (parse.y:4169-4194):
#
#     if (EOF_Reached)
#       {
#         ...
#         /* yyparse() has already called yyerror() and reset_parser() */
#         parser_state |= PST_NOERROR;
#         return (&matched_pair_error);
#       }
#     else if (r != 0)
#       {
#         /* Non-interactive shells exit on parse error in a command substitution. */
#         ...
#         if (interactive_shell == 0)
#           jump_to_top_level (FORCE_EOF);  /* This is like reader_loop() */
#
# `&matched_pair_error` is what *any* pair left open hands back to the reader
# that was scanning it, so a body that simply ran out is no different from an
# unclosed `[` written on its own: the enclosing command is lost, status is 2,
# and the script goes on. A body whose *grammar* is wrong is the other branch,
# and a non-interactive shell exits on it — those rows are wrapped in subshells
# so the rest of this file still runs.
#
# The only reader error a body can raise once the enclosing scan has already
# found its `)` is an unclosed subscript, because a `[` is the one matched pair
# that scan does not track. So `$(f[1)` reaches `yyparse`, whose lexer bails
# (`return -1; /* Bail immediately. */`, parse.y:5149), with `EOF_Reached` set by
# `parse_matched_pair` on the way out (parse.y:3712).
#
# The line is the one the `[` stands on, which for a body is the enclosing
# source's, not the body's own line 1 — bash never renumbers a substitution's
# text, it just keeps counting.
#
# A body ending mid-*construct* is a third thing and belongs with the grammar
# errors: the `)` is still in the token stream, so `yyparse` gets a real token to
# object to and `EOF_Reached` stays clear. `$(if true)` is that shape.
#
# Verified against bash 5.2.37.

echo "=== a body that ran out costs the command and nothing else ==="
eval 'echo $(f[1)';        echo "1 rc=$?"
eval 'echo $(f[1) fi';     echo "2 rc=$?"
eval 'echo $(echo x; f[1)'; echo "3 rc=$?"
eval 'x=$(f[1)';           echo "4 rc=$?"
eval 'echo <(f[1)';        echo "5 rc=$?"
eval 'echo >(f[1)';        echo "6 rc=$?"
eval 'echo ${y:-$(f[1)}';  echo "7 rc=$?"
eval 'echo $(( $(f[1) ))'; echo "8 rc=$?"
eval 'echo $(echo $(f[1))'; echo "9 rc=$?"
f() { echo $(eval 'echo $(f[1)'); echo "10 rc=$?"; }; f

echo "=== reported where the [ is written, not where the body starts ==="
eval 'echo $(f[1)'
echo "11 rc=$?"
eval $'echo $(\nf[1)'
echo "12 rc=$?"
eval $'echo $(\n\n\nf[1)'
echo "13 rc=$?"
eval $'echo <(\nf[1)'
echo "14 rc=$?"
eval $'x=1\ny=2\necho $(f[1)'
echo "15 rc=$?"

echo "=== but a grammar error in one exits a non-interactive shell ==="
( eval 'echo $(fi)' );      echo "16 rc=$?"
( eval 'echo $(;)' );       echo "17 rc=$?"
( eval 'echo $(do)' );      echo "18 rc=$?"
( eval 'echo <(fi)' );      echo "19 rc=$?"
( eval 'echo "$(fi)"' );    echo "20 rc=$?"

echo "=== and a body cut off mid-construct is one of those ==="
( eval 'echo $(if true)' );  echo "21 rc=$?"
( eval 'echo $(while :)' );  echo "22 rc=$?"
( eval 'echo $(case x)' );   echo "23 rc=$?"

echo TAIL
