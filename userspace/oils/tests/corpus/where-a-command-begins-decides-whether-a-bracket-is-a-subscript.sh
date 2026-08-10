# Whether a `[` at the head of a word opens a *subscript* — unquoted blanks and
# operators become content, and one that never closes is a reader error — is
# decided by `assignment_acceptable`, which is `command_token_position` less
# `case` patterns (parse.y:2983-2990):
#
#     #define command_token_position(token) \
#       (((token) == ASSIGNMENT_WORD) || \
#        ((parser_state&PST_REDIRLIST) && parsing_redirection(token) == 0) || \
#        ((token) != SEMI_SEMI && (token) != SEMI_AND && (token) != SEMI_SEMI_AND && reserved_word_acceptable(token)))
#
# Three of `reserved_word_acceptable`'s entries are easy to get wrong.
#
# `time` is in it, and so are the two options that belong to it — `TIME`,
# `TIMEOPT`, `TIMEIGN` all appear in `time_command_acceptable` (parse.y:3140-
# 3153) alongside `BANG`. Each option stands in exactly one place
# (`special_case_tokens`, parse.y:3292-3302): `-p` is TIMEOPT directly after
# TIME, `--` is TIMEIGN after TIME or TIMEOPT, and after a TIMEIGN neither is
# anything but an ordinary word. So `time -p -- x` reaches `x` in command
# position and `time -- -p x` does not, and a `-x` — not one of `time`'s
# options — becomes the command word itself.
#
# `)` is in it too, commented `/* only valid in case statement */`: a `case`
# arm's pattern close, after which the arm's first command begins. A subshell's
# close is the same token, and though no word may follow one, the reader reaches
# the `[` before the grammar reaches the word — so `(:) f[1` is the reader's
# error, not the parser's.
#
# `;;`, `;&` and `;;&` are *excluded* by name, even though a reserved word would
# be accepted after one, because what actually follows is the next arm's
# pattern. So `case x in y) :;; h[1 2]=v` is two words.
#
# `time` alone carries one more restriction: `time_command_acceptable` does not
# list `|`, and throws out a newline that a pipe stands immediately before
# (`case '\n': if (token_before_that == '|') return (0);`). After a pipe `time`
# is an ordinary command word, so the `[` behind it is past the first word and
# is only text.
#
# Verified against bash 5.2.37.
#
# The unclosed-subscript rows are `eval`-wrapped so the search for the `]`
# swallows only the string; see
# an-unclosed-subscript-in-an-assignment-position-is-a-reader-error.sh.

echo "=== time extends the position, and so do its own options ==="
eval 'time f1[1';         echo "1 rc=$?"
eval 'time -p f2[1';      echo "2 rc=$?"
eval 'time -- f3[1';      echo "3 rc=$?"
eval 'time -p -- f4[1';   echo "4 rc=$?"
eval 'time time f5[1';    echo "5 rc=$?"
eval '! time f6[1';       echo "6 rc=$?"
eval 'time ! f7[1';       echo "7 rc=$?"

echo "=== but only where they really are its options ==="
# These three run `time` for real, so their stderr carries a timing report that
# is different every time. `rc=2` is the reader error and `rc=127` is the word
# that was not an option being run as the command, which is the whole
# distinction; the messages are checked by the rows above.
eval 'time -x f8[1' 2>/dev/null;     echo "8 rc=$?"
eval 'time -- -p f9[1' 2>/dev/null;  echo "9 rc=$?"
eval 'time -p -p f10[1' 2>/dev/null; echo "10 rc=$?"
eval 'echo time f11[1';   echo "11 rc=$?"

echo "=== and not after a pipe, where time is an ordinary word ==="
eval 'true | time f12[1';   echo "12 rc=$?"
eval 'true |& time f13[1';  echo "13 rc=$?"
eval $'true |\ntime f14[1'; echo "14 rc=$?"

echo "=== a case arm's ) begins a command ==="
eval 'case x in y) f15[1'
echo "15 rc=$?"
eval 'case x in y) time f16[1'
echo "16 rc=$?"
eval 'case x in x) h1[1 2]=v;; esac'
echo "17 rc=$?"
eval 'case x in x) h2[1 + 1]=v;; esac; declare -p h2'
echo "18 rc=$?"

echo "=== a subshell's ) is the same token to the reader ==="
eval '(:) f17[1'
echo "19 rc=$?"

echo "=== but a case-arm terminator does not ==="
eval 'case x in y) :;; f18[1'
echo "20 rc=$?"
eval 'case x in y) :;& f19[1'
echo "21 rc=$?"
eval 'case x in y) :;;& f20[1'
echo "22 rc=$?"
eval 'case x in y) :;; h3[1 2]=v'
echo "23 rc=$?"

echo TAIL
