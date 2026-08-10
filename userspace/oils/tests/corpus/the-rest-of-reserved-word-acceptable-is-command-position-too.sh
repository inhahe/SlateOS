# `reserved_word_acceptable` is a switch over `last_read_token` with twenty-odd
# arms (parse.y:5367-5415), and the ones that are not punctuation are easy to
# miss because nothing else in the reader mentions them:
#
#     case (token)
#       {
#       case '\n': case ';': case '(': case ')': case '|': case '&': case '{':
#       case '}':               /* XXX */
#       case AND_AND:
#       case BANG:
#       case BAR_AND:
#       case DO: case DONE: case ELIF: case ELSE: case ESAC:
#       case FI: case IF: case OR_OR:
#       case SEMI_SEMI: case SEMI_AND: case SEMI_SEMI_AND:
#       case THEN: case TIME: case TIMEOPT: case TIMEIGN:
#       case COPROC: case UNTIL: case WHILE: case 0:
#       case ARITH_CMD: case ARITH_FOR_EXPRS: case COND_END: case DOLPAREN:
#         return 1;
#       default:
#         /* ... */
#         if (token == WORD && token_before_that == COPROC) return 1;
#         if (token == WORD && token_before_that == FUNCTION) return 1;
#         return 0;
#       }
#
# `COPROC`, `ARITH_CMD` and `COND_END` are all in it, so the token *after* a
# `coproc`, after a `(( … ))` and after a `]]` all begin a command — and a `[` at
# the head of the word standing there is a subscript. The `default` arm adds the
# one word that may follow `coproc` or `function`, which is the name.
#
# `FUNCTION` itself is not in the list: after `function` comes the name, and a
# `[` there is only text.
#
# `TIMEOPT` and `TIMEIGN` are in it too, which is what carries the position
# across `time`'s own options — so `time -p (( 1 ))` is an arithmetic command
# where `time -x (( 1 ))` is not, `-x` being an ordinary command word rather than
# an option (`special_case_tokens`, parse.y:3292-3302).
#
# `|` is *not* in `time_command_acceptable` (parse.y:3140-3153), and neither is a
# newline whose `token_before_that` was `|`, so behind a pipe `time` is an
# ordinary word and the `((` after it is two plain parens.
#
# `FOR` is not in the list either — `for (( … ))` is a *separate* branch of
# `parse_dparen`, tried first (parse.y:4456-4508), and it yields ARITH_FOR_EXPRS
# rather than ARITH_CMD. So `for` reaches an arithmetic command but is not a
# command position: `for f[1` is a plain word.
#
# Verified against bash 5.2.37.
#
# The unclosed-subscript rows are `eval`-wrapped so the search for the `]`
# swallows only the string; see
# an-unclosed-subscript-in-an-assignment-position-is-a-reader-error.sh.

echo "=== coproc, and the name that may follow it ==="
eval 'coproc f1[1';                     echo "1 rc=$?"
eval 'coproc c f2[1';                   echo "2 rc=$?"

echo "=== but function's name position is not one ==="
eval 'function f3[1';                   echo "3 rc=$?"

echo "=== an arithmetic command ends in a command position ==="
eval '(( 0 )) f4[1';                    echo "4 rc=$?"
eval '(( 0 )) h5[1 2]=v';               echo "5 rc=$?"
eval '(( 0 )) && f6[1';                 echo "6 rc=$?"

echo "=== and so does a conditional command's ]] ==="
eval '[[ a == a ]] f7[1';               echo "7 rc=$?"
eval '[[ a == a ]] h8[1 2]=v';          echo "8 rc=$?"
eval '[[ f9[1';                         echo "9 rc=$?"

echo "=== time's own options carry the position to a (( ==="
eval 'time -p (( 1 ))' 2>/dev/null;     echo "10 rc=$?"
eval 'time -- (( 1 ))' 2>/dev/null;     echo "11 rc=$?"
eval 'time -p -- (( 1 ))' 2>/dev/null;  echo "12 rc=$?"
eval 'time -x (( 1 ))' 2>/dev/null;     echo "13 rc=$?"

echo "=== but a pipe puts time out of the reserved position ==="
eval 'true | time (( 1 ))';             echo "14 rc=$?"
eval 'true |& time (( 1 ))';            echo "15 rc=$?"
eval $'true |\ntime (( 1 ))' 2>/dev/null; echo "16 rc=$?"

echo "=== for reaches (( by a branch of its own, not by the list ==="
eval 'for (( i=0; i<2; i++ )); do echo $i; done'; echo "17 rc=$?"
eval 'for f18[1';                       echo "18 rc=$?"
eval 'for h19[1 2]=v';                  echo "19 rc=$?"

echo "=== and in, case and select reach neither ==="
eval 'select f20[1';                    echo "20 rc=$?"
eval 'case x in ((p) echo hit;; esac';  echo "21 rc=$?"
eval 'case x in (( 1 )) echo hit;; esac'; echo "22 rc=$?"

echo TAIL
