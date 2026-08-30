# `set -v` echoes a line the parse goes on to *refuse*, because the echo happens
# in the reader and not in the parser.
#
# bash's `shell_getc` hands the parser a whole physical line and echoes it at
# that moment, before a single token is taken off it — so by the time yacc looks
# at a token and finds it cannot shift it, that token's line has already gone
# out. Three consequences, and they are the whole of this case:
#
#   * an error on the unit's *first* token still echoes the line (`fi`);
#   * an error *mid*-line echoes the whole line, not the part that parsed
#     (`echo a; ) bad`, not `echo a;`);
#   * an error on a *later* line of a multi-line unit echoes every line the
#     reader had got to, and no more (`{ echo a` / `) bad` — but not the `}`).
#
# An unterminated construct is the same rule seen from the other end: the reader
# runs to end of input looking for the close, so every line it passed on the way
# is echoed before the error.
#
# Each probe runs under `eval` in a subshell, since a syntax error otherwise
# abandons the rest of the script. The `eval` string is read the same way a file
# is, so the echo happens there too.

echo '=== a line that parses is echoed once, then runs'
( set -v; eval 'echo a; echo b' ) 2>&1
echo '=== an error on the first token of the unit'
( set -v; eval 'fi' ) 2>&1; echo "s=$?"
( set -v; eval ') echo x' ) 2>&1; echo "s=$?"
echo '=== an error part-way along the line echoes all of it'
( set -v; eval 'echo a; ) bad' ) 2>&1; echo "s=$?"
( set -v; eval 'echo a && ) bad' ) 2>&1; echo "s=$?"
( set -v; eval 'echo b;;' ) 2>&1; echo "s=$?"
echo '=== a multi-line unit echoes up to the offending line and no further'
( set -v; eval '{ echo a
) bad
}' ) 2>&1; echo "s=$?"
echo '=== lines before the failing one have already run'
( set -v; eval 'echo one
echo two;;
echo three' ) 2>&1; echo "s=$?"

echo '=== an unclosed quote: every line the reader passed is echoed'
( set -v; eval 'echo "' ) 2>&1; echo "s=$?"
( set -v; eval 'echo one
echo "unterm
echo three' ) 2>&1; echo "s=$?"
echo '=== the same for an unclosed substitution and a here-document'
( set -v; eval 'echo $(' ) 2>&1; echo "s=$?"
( set -v; eval 'cat <<E "' ) 2>&1; echo "s=$?"

echo '=== a complete compound is still echoed whole and run'
( set -v; eval 'if true; then
echo x
fi' ) 2>&1; echo "s=$?"
