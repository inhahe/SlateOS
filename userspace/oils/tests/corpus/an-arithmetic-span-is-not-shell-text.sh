# A `$(( … ))` body never reaches `read_token`. `parse_comsub` peeks one
# character past the `$(` and, if it is a second `(`, hands the whole span to
# `parse_matched_pair` instead of to the parser (parse.y:4096-4104):
#
#     /* Posix interp 217 says arithmetic expressions have precedence, so
#        assume $(( introduces arithmetic expansion and parse accordingly. */
#     if (open == '(')          /*)*/
#       {
#         peekc = shell_getc (1);
#         shell_ungetc (peekc);
#         if (peekc == '(')     /*)*/
#           return (parse_matched_pair (qc, open, close, lenp, P_ARITH));
#       }
#
# `parse_matched_pair` only counts parens and tracks quoting, so the text inside
# is an *expression*: `case` is a variable name that evaluates to 0, `<` is a
# comparison, `&&` a conjunction, `>>` a shift, `,` a sequence operator. None of
# them is a shell token, and no `case` command can begin there.
#
# The same holds for the `(( … ))` arithmetic *command*, which
# `parse_arith_cmd` reads with the same call (parse.y:4528).
#
# This matters for a reader that scans a command substitution's body looking for
# the `case` … `esac` whose `)` must not be mistaken for the substitution's own
# close: it has to stop feeding that scan at the `((`, or a `case` spelled inside
# an arithmetic span opens a pattern list that never ends, and the substitution
# swallows its own `)`.
#
# Verified against bash 5.2.37.
#
# The rows are `eval`-wrapped so that a reader which does get one of them wrong
# fails that row alone rather than the whole file.

echo "=== a case-family word spelled inside an arithmetic span ==="
eval 'echo $(echo $(( case )))';          echo "1 rc=$?"
eval 'echo $(echo $(( esac )))';          echo "2 rc=$?"
eval 'echo $(echo $(( in )))';            echo "3 rc=$?"
eval 'echo $(echo $(( case + esac )))';   echo "4 rc=$?"
eval 'echo $(( case ))';                  echo "5 rc=$?"
eval 'echo $(x=0; (( case )); echo $x)';  echo "6 rc=$?"

echo "=== and one inside an arithmetic span in a case body ==="
eval 'echo $(case x in x) echo $(( case ));; esac)'; echo "7 rc=$?"
eval 'echo $(case x in x) echo $(( esac ));; esac)'; echo "8 rc=$?"

echo "=== a quoted one is not the reserved word either ==="
eval 'echo $(echo "case" in esac)';       echo "9 rc=$?"
eval 'echo $(printf %s case in esac)';    echo "10 rc=$?"

echo "=== a shell operator inside an arithmetic span is arithmetic ==="
eval 'echo $(echo $(( 1 < 2 )))';         echo "11 rc=$?"
eval 'echo $(echo $(( 8 >> 2 )))';        echo "12 rc=$?"
eval 'echo $(echo $(( 1 && 1 )))';        echo "13 rc=$?"
eval 'echo $(echo $(( 1 || 0 )))';        echo "14 rc=$?"
eval 'echo $(echo $(( 1 , 2 )))';         echo "15 rc=$?"
eval 'echo $(echo $(( 1 ? 2 : 3 )))';     echo "16 rc=$?"
eval 'echo $(echo A$(( 1 ))B)';           echo "17 rc=$?"
eval 'echo $(echo $(( 1 )) $(( 2 )))';    echo "18 rc=$?"

echo TAIL
