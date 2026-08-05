# When input runs out inside an unfinished construct, bash's `syntax error:
# unexpected end of file` lands on the line *after* the last one it read — the
# place the missing terminator would have gone. A `[[ … ]]` says which closer it
# wanted first, and that line is the reader reporting where it gave up, so it
# carries the last line actually read instead:
#
#   [[ -n x        line 1: unexpected EOF while looking for `]]'
#                  line 2: syntax error: unexpected end of file
#
# The two numbers are one apart because nothing was read between them, not
# because the second is "the first plus one": put the construct further down and
# both move together.
#
# Most constructs print no first line at all — `{`, `if`, `while`, `case`, `(`
# and an unfinished `for` all run out through the grammar, which has only the
# one message. An unclosed quote or substitution is the other way round: it is
# the *reader* that fails, and it prints its `unexpected EOF while looking for
# matching `C'` alone, with no end-of-file line after it.
#
# Each case is sourced from a written file so the line numbers are the snippet's
# own rather than this file's.

l() { printf '%s\n' "$1" > sourced.sh; ( . ./sourced.sh ) 2>&1; rm -f sourced.sh; }

echo "=== a conditional names its closer, on the line it gave up on"
l '[[ -n x '
l '[[ a == b '
l '[[ ( a ) '
l 'echo one
[[ -n x '
l 'echo one
echo two
[[ -n x '

echo "=== an unclosed quote or substitution prints its one line and no other"
l 'echo "abc'
l 'echo one
echo "abc'
l "echo 'abc"
l 'echo $(echo x'
l 'echo `echo x'

echo "=== and the grammar ones print only the end-of-file line"
l '{ echo x'
l 'echo one
{ echo x'
l 'if true; then echo x'
l 'while true; do echo x'
l 'case x in a) echo y;;'
l '(echo x'
l 'for i in a; do'
l 'echo one &&'
echo done
