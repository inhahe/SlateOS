# `$LINENO` (and every "line N:" diagnostic) for a command whose tokens span
# more than one physical line, plus the line base `eval` gives its string.
#
# bash's number is a parser artifact, and matching it needs the exact rule.
# bash stamps a simple command with `line_number` as it stands when its grammar
# reduces the command's FIRST element — and `line_number` names the last input
# line bash has *fetched*, i.e. the line the token just read ENDS on:
#
#   * A leading plain word forces one token of lookahead, because bash cannot
#     yet tell a simple command from a function definition (`WORD '(' ')' …`).
#     So the number comes from the token AFTER the command word.
#   * A leading assignment (`v=1 cmd …`, `a=(1) cmd …`) reduces on sight and is
#     numbered by itself.
#
# Hence `echo "a<nl>b" $LINENO` is 2 but `echo $LINENO "x<nl>y"` is 1: only the
# *second* token's extent counts. Verified against bash 5.2.37.

# The second word ends on the next line, so the command is numbered by it.
echo "a
b" "one=$LINENO"

# …but a $LINENO that comes *before* the multi-line word still reports the
# first line: the lookahead token ended there.
echo "two=$LINENO" "x
y"

# A `\<newline>` between words is not part of either token, so the lookahead
# word still ends on the first line.
echo A"three=$LINENO" \
B"four=$LINENO"

# When the continuation is all that precedes the lookahead word, that word ends
# on the second line and the command is numbered there.
echo \
"five=$LINENO"

# A leading assignment needs no lookahead: it is numbered by its own last line,
# regardless of how far the rest of the command reaches.
v=1 echo "p
q" "six=$LINENO"

# The assignment itself may span lines, and then it is numbered by its end.
w="r
s" ; echo "seven=$LINENO $w"

# An array assignment behaves the same way.
a=(t
u)
echo "eight=$LINENO ${a[*]}"

# `eval` continues the enclosing count: the string's first line IS the line the
# `eval` command ended on, so the reported line is
# `LINENO(eval) - 1 + line_within_string`.
eval 'echo "nine=$LINENO"'
eval 'echo ten
echo "eleven=$LINENO"'

# The `eval` command word itself follows the lookahead rule, so a string that
# starts on a later line pushes the base down with it.
eval "
echo \"twelve=\$LINENO\""

# Nested evals compose, because the inner base is computed from the already
# absolute line of the outer one.
eval 'eval "echo \"thirteen=\$LINENO\""'

# Runtime diagnostics carry the same number as `$LINENO` would.
echo "c
d" ; nosuchcmd_lineno_probe 2>&1
