# `command_token_position` has three clauses, and the middle one is a *parser
# state* rather than a token (parse.y:2983-2986):
#
#     /* Is `token' one that will allow a WORD to be read in a command position?
#        We can read a simple command name on which we should attempt alias expansion
#        or we can read an assignment statement. */
#     #define command_token_position(token) \
#       (((token) == ASSIGNMENT_WORD) || \
#        ((parser_state&PST_REDIRLIST) && parsing_redirection(token) == 0) || \
#        ((token) != SEMI_SEMI && (token) != SEMI_AND && (token) != SEMI_SEMI_AND && reserved_word_acceptable(token)))
#
# `PST_REDIRLIST` is "parsing a list of redirections preceding a simple command
# name" (parser.h:48). `make_simple_command` sets it the moment a simple command
# is begun and clears it again as soon as a *word* is added to that command
# (make_cmd.c:526-535):
#
#       if (command == 0)
#         {
#           command = make_bare_simple_command ();
#           parser_state |= PST_REDIRLIST;
#         }
#
#       if (element.word)
#         {
#           command->value.Simple->words = make_word_list (element.word, ...);
#           parser_state &= ~PST_REDIRLIST;
#         }
#
# So a run of redirections written *before* the command name leaves the reader in
# command position all the way through it: `>f g[1 2]=v` slurps its subscript and
# `>f g[1` is the reader's `]` error, exactly as if the redirections were not
# there. `parsing_redirection(token) == 0` is what keeps the redirection's own
# operator out of it, and its target word is added as a redirect rather than as
# `element.word`, so it does not end the run either.
#
# The first clause is the other half of the same question, and the one osh used
# to answer by looking at the token behind: `ASSIGNMENT_WORD` is a
# *classification made when the word was read*, so a word merely shaped like an
# assignment but standing past the command name is an ordinary WORD and does not
# extend the position. `p x=1 g[1 2]=v` is three words.
#
# Verified against bash 5.2.37.
#
# The rows whose assignment really happens are wrapped in subshells: an
# arithmetic error in a subscript is a `jump_to_top_level (DISCARD)`, which eats
# the rest of the list it was raised in — see
# a-discard-stops-at-the-eval-or-function-it-was-raised-in in known-issues.md.
# The unclosed-subscript rows are `eval`-wrapped so the search for the `]`
# swallows only the string; see
# an-unclosed-subscript-in-an-assignment-position-is-a-reader-error.sh.

p() { printf "[%s]" "$@"; echo; }

echo "=== a leading run of redirections is still command position ==="
eval '2>/dev/null f1[1';                echo "1 rc=$?"
eval '>/dev/null f2[1';                 echo "2 rc=$?"
eval '2>/dev/null 3>/dev/null f3[1';    echo "3 rc=$?"
eval '{v}>/dev/null f4[1';              echo "4 rc=$?"
eval '2>&1 f5[1';                       echo "5 rc=$?"
eval '<<E f6[1';                        echo "6 rc=$?"
( eval '>/dev/null h7[1 2]=v' );        echo "7 rc=$?"
( eval '2>/dev/null 3>&2 h8[1 2]=v' );  echo "8 rc=$?"

echo "=== a word of the command ends the run ==="
eval 'x=1 >/dev/null f9[1';             echo "9 rc=$?"
eval 'p 2>/dev/null f10[1';             echo "10 rc=$?"
eval '2>/dev/null p f11[1';             echo "11 rc=$?"
eval '2>/dev/null p a f12[1';           echo "12 rc=$?"

echo "=== and the run starts over with each command ==="
eval ': ; 2>/dev/null f13[1';           echo "13 rc=$?"
eval 'true | 2>/dev/null f14[1';        echo "14 rc=$?"
eval 'if 2>/dev/null f15[1';            echo "15 rc=$?"
eval '! 2>/dev/null f16[1';             echo "16 rc=$?"
eval 'time 2>/dev/null f17[1';          echo "17 rc=$?"
eval 'case x in y) 2>/dev/null f18[1';  echo "18 rc=$?"
eval 'case x in y) :;; 2>/dev/null h19[1 2]=v'; echo "19 rc=$?"

echo "=== an assignment past the command word is not an ASSIGNMENT_WORD ==="
eval 'p x=1 h20[1 2]=v';                echo "20 rc=$?"
eval 'p a h21[1 2]=v';                  echo "21 rc=$?"
( eval 'x=1 h22[1 2]=v' );              echo "22 rc=$?"
( eval 'h23=(1) h24[1 2]=v' );          echo "23 rc=$?"

echo TAIL
