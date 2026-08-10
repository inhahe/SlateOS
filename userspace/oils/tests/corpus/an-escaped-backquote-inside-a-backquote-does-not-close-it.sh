# Inside a backquote, POSIX says the command runs to ``the next backquote that
# is not preceded by a backslash'' — and bash's word scanners honour that by
# testing the backslash *before* they test the backquote state. All three of
# them are written the same way round:
#
#     if (c == '\\')                        /* subst.c:925, 1044, 2093 */
#       { pass_next++; i++; continue; }
#     if (backquote)
#       { if (c == '`') backquote = 0; … }
#
# so `\`` inside a backquote is an escaped backtick and the backquote runs on
# past it. Order those two the other way round and the escaped backtick closes
# the backquote early, which drags the scan out of step with the parser: the
# shapes below are all ones where the parser reads the word one way and the
# expansion-time scan would read it another.
#
# The scan only runs at all on a word containing `${`, so every line here has
# one, and between them they reach each of the three scanners:
# `string_extract_double_quoted` (a `"` run in the word), `skip_double_quoted`
# (a `"` run inside a `${ }` operand) and `skip_matched_pair` (a subscript).
#
# Case 1 is the shape that has to survive the scan rather than pass it: the
# backquote it opens is never closed, so the word is *not* a lex error — the
# parser reads it to end of input, hands the body to the command-substitution
# parser, and that is what reports, naming the line the body ran out on and
# leaving the substitution empty. `a\y` is the surrounding string with nothing
# in the middle, and the status stays 0.
#
# Verified against bash 5.2.37.

echo "=== 1 a backquote nothing closes, reached over an escaped one"
echo "a\\`x";  echo ${z:-p"\`"r}
echo "b\\`y"
echo "=== 2 the same shapes, but the backquote does close"
echo "A${z:-q}`echo m\`echo n\``B"
echo "=== 3 an escaped backquote inside a backquote inside a brace operand"
echo "${z:-A`echo m\`echo n\``B}"
echo "=== 4 the same, inside a double-quoted run in the operand"
echo "${z:-A"`echo m\`echo n\``"B}"
echo "=== 5 the same, inside a subscript"
a=(u v w)
echo "${a[`echo 1\`echo \``]}"
echo done
