# bash's parser stops at the `'` opening a run and resumes at its mate —
# `parse_matched_pair` reads nothing between them — so a backquote written inside
# one is text as far as any parse is concerned. It is met only when the string is
# expanded, by `param_expand`'s own scan:
#
#   t_index = sindex + 1;
#   temp = string_extract (string, &t_index, "`", SX_REQMATCH);
#   if (temp == &extract_string_error || temp == &extract_string_fatal)
#     { report_error (_("bad substitution: no closing \"`\" in %s"),
#                     string + t_index); … }        /* subst.c:11264-11272 */
#
# Two things follow. The failure costs only the command, never the script. And
# the `%s` is `string + t_index` — the text from the backquote *on*, where the
# missing-brace reporter beside it echoes the whole string. In an arithmetic
# fragment `string` is the fragment, quotes and all (`Q_DOUBLE_QUOTES` switches
# the single quote off), so the report runs from the backquote to the end of the
# fragment and takes the run's closing quote and its neighbours with it.
#
# Which of the two reporters gets there is simply whichever scan meets its
# opener first, reading left to right: a `${` before the backquote swallows it
# (`string_extract` walks over a backquote without reading it), and a backquote
# before a `${` swallows that.
#
# Verified against bash 5.2.37.

declare -a arr=(10 20 30)
declare -A m=([k]=V)
v=abcdef

echo "=== the script survives it ==="
echo "[${arr['x`fi']}]"
echo STILL-HERE

echo "=== the report runs to the end of the fragment, not of the run ==="
echo "[${arr['x`fi'y]}]"

echo "=== text before the run is not part of it ==="
echo "[${arr[z'x`fi']}]"

echo "=== a second run is ==="
echo "[${arr['x`fi''q']}]"

echo "=== a substring bound is the same kind of fragment ==="
echo "[${v:'x`fi':2}]"

echo "=== a brace met first swallows the backquote ==="
echo "[${arr['x${m`fi']}]"

echo "=== and a backquote met first swallows the brace ==="
echo "[${arr['x`fi${m']}]"

echo "=== a mated one is performed, and its output reaches the evaluator ==="
echo "[${arr['x`echo 1`']}]"

echo "=== one written outside the run is the parse's, and expands ==="
echo "[${arr[`echo 1`'x']}]"

echo TAIL
