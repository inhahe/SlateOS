# The deprecated `$[ … ]` spelling is extracted by an extractor of its own,
#
#     extract_delimited_string (string, sindex, "$[", "[", "]", 0)
#                                        (`extract_arithmetic_subst`, subst.c:1299-1304)
#
# and the flags are `0` — no `SX_COMMAND`. So unlike `$(( … ))`, whose extent
# read really parses a nested `$( … )` over the whole enclosing string (see
# `an-arithmetic-extent-read-parses-a-command-substitution-inside-it.sh`), this
# one steps over everything it passes and counts brackets. Its `$( … )` is read
# only when the expression itself is expanded — `expand_arith_string (temp,
# Q_DOUBLE_QUOTES|Q_ARITH)`, subst.c:10662 — and that read is an ordinary
# *string-level* one, against the expression's own text.
#
# Which means a body that will not parse is reported the string-level pair: the
# extent read echoes the whole remainder after the `$(`, and the child that then
# runs it echoes one byte less (`a-failed-extent-parse-consumes-the-rest-of-the-
# string.sh`). Then the jump is suppressed — `if ((flags & SX_NOLONGJMP) == 0)
# jump_to_top_level (-nc);`, parse.y:4330 — so the read only leaves `sindex` at
# the end of the *expression*, and bash evaluates whatever was accumulated before
# the `$(`. `1+` is an operand-expected error; nothing at all is 0.
#
# The consumption stops at the expression, because `expand_arith_string` is one
# `expand_word_internal` call over the expression alone: text after the `$[ … ]`
# is expanded normally.
#
# Only reachable under `no_longjmp_on_fatal_error` — a prompt expansion — as
# everywhere else the jump stands. errexit does not go through the jump at all
# (`parser_error` ends with a direct `exit_shell`), so it still wins.
#
# Verified against bash 5.2.37.

echo "=== the pair: the extent read, then the child one byte short"
a='A$[1+$(fi)]B';      printf '1 [%s]\n' "${a@P}"
b='A$[1+$(fi)+3]B';    printf '2 [%s]\n' "${b@P}"
c='A$[2+$(fi)*3]B';    printf '3 [%s]\n' "${c@P}"

echo "=== then the truncated expression is evaluated, not abandoned"
# Nothing before the `$(`, so the expression is empty and evaluates to 0.
d='A$[$(fi)]B';        printf '4 [%s]\n' "${d@P}"; echo "4 rc=$?"
# A substitution *before* the bad one has already run, so its value is there.
e='A$[1+$(echo 2)+$(fi)]B'; printf '5 [%s]\n' "${e@P}"

echo "=== and the consumption stops at the expression"
y=Y
f='A$[$(fi)]B${y}C';   printf '6 [%s]\n' "${f@P}"
# The word-level failure of the first one ends the word, so the second `$[` is
# never reached and reports nothing.
g='A$[1+$(fi)]B$[2+$(fi)]C'; printf '7 [%s]\n' "${g@P}"
h='A$[1+$(fi)]B$(echo Z)C';  printf '8 [%s]\n' "${h@P}"

echo "=== a read that closes changes nothing"
i='A$[1+$(echo 2)]B';  printf '9 [%s]\n' "${i@P}"
j='A$[1+2]B';          printf '10 [%s]\n' "${j@P}"

echo "=== under a brace it is the brace's scan that reads it, and the brace's ending"
unset z
k='A${z:-$[1+$(fi)]}B'; printf '11 [%s]\n' "${k@P}"

echo "=== outside a prompt the jump stands: a here-document body is not one"
cat <<E
A$[1+$(fi)]B
E
echo "12 rc=$?"

echo "=== done"
