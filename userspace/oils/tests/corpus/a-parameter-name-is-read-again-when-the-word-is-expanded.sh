# bash reads a word twice, and only the first read is the parser's.
#
# The parser reads a word to find where it *ends*. Everything else is
# re-derived from the word's source when it is expanded: `expand_word_internal`
# (subst.c:9989) walks it again, and at a `${` hands the whole remaining string
# to `parameter_brace_expand` (subst.c:9539), which carves the parameter name
# out for itself with
#
#   name = string_extract (string, &t_index, "#%^,~:-=?+/@}", SX_VARNAME);
#
# `SX_VARNAME` is the difference (subst.c:795): at a `[` that scan calls
# `skipsubscript` and jumps to the matching `]`, *wherever it is* — including
# past the `}` the parser closed at, and past a closing double quote, since
# `skip_matched_pair` steps over a quoted run rather than stopping at it.
#
# So `"${h[}"x"]}"` does not name `h[`. It names `h[}"x"]` — an array reference
# whose subscript is `}"x"` — and the `}` that closes the expansion is the one
# after the `]`. Three things follow, and each has its own group below.
#
# What the subscript then means is the array's business: arithmetic for an
# indexed one, where `}x` is a syntax error the *arithmetic* evaluator reports,
# and a key for an associative one, where quote removal makes it `}x` and the
# expansion succeeds. So this is not only a matter of which diagnostic is
# printed.
#
# The jump is only made while the scan is still in the parameter name
# (`dolbrace_state == DOLBRACE_PARAM`), and that is what separates `${#h[` from
# `${!h[`: `#` is in the first operator set the state machine tests, so the
# scan has left the name by the `[`, while `!` is in none of them.
#
# And only a subscript that *closes* is jumped over — `if (string[ni] == RBRACK)`
# — so a `[` with no `]` after it anywhere in the word is left as an ordinary
# character, and the word is the plain bad substitution it was before.
#
# Verified against bash 5.2.37.

echo "=== the name is re-read, and the re-read jumps to the ]"
declare -A h
h[}x]=HIT
echo "${h[}"x"]}"
echo "rc=$?"
echo ${h[}x]}
echo "rc=$?"
echo "pre${h[}"x"]}post"
echo "rc=$?"

echo "=== an operator is still read, from after the ]"
echo "${h[}"x"]:-DEF}"
echo "${h[}"nosuch"]:-DEF}"
echo "${h[}"x"]#H}"
echo "${h[}"x"]/H/J}"
echo "rc=$?"

echo "=== an indexed array reads the same subscript as arithmetic"
a=(zero one two)
echo "${a[}"x"]}"
echo "rc=$?"
echo "${a[}"0"]}"
echo "rc=$?"

echo "=== a subscript that closes before the } is the parser's own"
echo "${a[0]}"x"]}"
echo "${a[1]}" "${a[@]}" "${#a[@]}"
echo "rc=$?"

echo "=== # has left the name by the [, ! has not"
echo "${#h[}"x"]}"
echo "rc=$?"
echo "${!h[}"x"]}"
echo "rc=$?"

echo "=== with no ] to jump to the [ is an ordinary character"
echo ${a[}tail
echo "rc=$?"
echo ${a[}
echo "rc=$?"

echo "still here"
