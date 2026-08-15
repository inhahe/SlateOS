# `parameter_brace_expand` reads a `${ … }` in two steps, and only the second one
# is `extract_dollar_brace_string`. First comes the *name*:
#
#   name = string_extract (string, &t_index, "#%^,~:-=?+/@}", SX_VARNAME);
#                                                       /* subst.c:9550 */
#
# — a scan that stops at the first of those characters, stepping over a `\c` pair
# and over a `[…]` that closes (and over nothing else: a quote is an ordinary
# byte to it). If it reaches the end of the text instead, `c` is NUL and the
# `switch (c)` at subst.c:10018 falls straight to `bad_substitution:`, which says
# `%s: bad substitution` naming the whole string. The brace has not been missed
# at that point — it has not been looked for.
#
# So which of bash's two diagnostics an unterminated `${` takes is settled by the
# name, never by the brace:
#
#   * no operator reached                       -> `%s: bad substitution`
#   * an operator reached, but the name is one
#     `valid_brace_expansion_word` (subst.c:9803)
#     or the length branch (subst.c:9687) refuses -> the same
#   * an operator reached and the name good     -> ``no closing `}' in %s``
#
# Two consequences follow from the ordering. A `$( … )` written inside the name
# is never parsed, because `string_extract` walks over it without reading — so
# `a${m$(fi) b` names the bad substitution and the `fi` goes unmentioned. And an
# indirection is *resolved* first (`parameter_brace_expand_indir`, subst.c:9807),
# so an unset or unusable pointer reports in the missing brace's place.
#
# Verified against bash 5.2.37.

declare -a arr=(10 20 30)
declare -A m=([k]=V)

echo "=== a name that runs off the end of an arithmetic fragment ==="
echo "[${arr['x${m']}]"

echo "=== the length form, whose name scan stops only at the brace ==="
echo "[${arr['x${#m']}]"

echo "=== an indirection whose name runs off with it ==="
echo "[${arr['x${!m']}]"

echo "=== a subscript that closes is stepped over, and the scan runs on ==="
echo "[${arr['x${m[0]']}]"

echo "=== one that does not close is not, and the name is refused ==="
echo "[${arr['x${m[a:b']}]"

echo "=== a backslash hides the operator that would have ended the name ==="
echo "[${arr['x${q\:']}]"

echo "=== reaching an operator with a good name is the other diagnostic ==="
echo "[${arr['x${m:-']}]"

echo "=== and \`@\` is one of the operators it stops at ==="
echo "[${arr['x${m@']}]"

echo "=== the same pair in a here-document body ==="
cat <<E
a${m b
E
cat <<E
a${m:-b
E

echo "=== a command substitution inside the name is never read ==="
cat <<E
a${m$(fi) b
E
echo "=== where one after the operator is ==="
cat <<E
a${m:-$(fi) b
E

echo "=== an indirection is resolved before the brace is missed ==="
cat <<E
a${!nosuch:-b
E
q='not a name'
cat <<E
a${!q:-b
E
p=HOME
cat <<E
a${!p:-b
E

echo "=== a prompt expansion collapses the two, and still resolves ==="
v='a${m b'
echo "[${v@P}]"
v='a${!nosuch:-b'
echo "[${v@P}]"

echo TAIL
