# bash reads a double-quoted word twice, and the two reads disagree about
# where a `${ … }` ends.
#
# The parser closes it at the first `}`. `expand_word_internal` then re-reads
# the word, and `string_extract_double_quoted` (subst.c:963) carves the quoted
# run out by handing `extract_dollar_brace_string` (subst.c:1813) the *whole
# word* — not the run. That scan has a rule the parser has not: at a `[`, while
# it is still in the parameter name, it calls `skipsubscript` (subst.c:1943) and
# jumps to the matching `]`. With no `]` anywhere in the word it runs off the
# end, and `CHECK_STRING_OVERRUN` turns that into the complaint at subst.c:1975:
#
#   bad substitution: no closing `}' in %s
#
# Three things follow, and each has its own group below.
#
# It is the *scanner's* complaint, so it is raised before anything in the word
# is expanded — the `$( … )` written to the left of the `${` never runs. And it
# is raised with a bare `exp_jump_to_top_level (DISCARD)`, which makes it
# errexit-only: `set -e` exits, posix mode does not.
#
# What it names is the string the innermost `expand_word_internal` was handed:
# the whole word, quotes and neighbours included, for a fault the extent scan
# found — but the run's *contents*, unquoted, for one found a level in, while
# expanding them.
#
# And `$[` is not a construct either double-quote scanner knows (only `${`,
# `$(` and a backtick are), so a `${` written inside a `$[ … ]` inside double
# quotes is met by the extent scan even though the expander would have handed
# that text to the arithmetic evaluator.
#
# Verified against bash 5.2.37.

echo "=== an unterminated subscript runs the scan off the end of the word"
echo "${a[}"
echo "${a[0}"
echo "${a[ }"
echo "${a[\]}"

echo "=== and it is the whole word that is named, quotes and neighbours too"
echo "${a[}"tail
echo "pre${a[}post"
echo "a${a[}b" "c]"
echo "${a[}"']'

echo "=== but a run's contents are named when the fault is found in them"
echo "${x:-${a[}}"
echo ${x:-"${a[}"}

echo "=== unquoted, the extent scan is never reached"
echo ${a[}tail
echo ${a[}
echo "${#a[}"

echo '=== a subscript that closes is skipped, wherever its ] was written'
a=(9 8)
echo "${a[0]}" "${a[@]}" "${#a[@]}" "${!a[@]}"
echo "${a[$(echo 1)]}"
v=x; echo "${v/[/y}" "${v//[}/z}"

echo "=== the complaint is the scanner's, so nothing in the word has run"
echo "$(echo ran >&2)x${a[}"
echo "${a[}"$(echo hi >&2)
echo "still here"

echo "=== a dollar-bracket is not a construct the quote scanner knows"
echo "$[ ${ ]"
echo "x$[ ${x:- ]y"
echo "$[ ${x!} ]"
echo "$(( ${x:- ))"

echo "=== the discard unwinds to the top of the input, and stops there"
echo "${a[}" ; echo "not reached"
echo "${a[}" | cat; echo "after pipeline=$?"
x=$(echo "${a[}"); echo "x=[$x] rc=$?"
f() { echo "${a[}"; }
f
echo "still here"
(
set -e
echo "${a[}"
echo "not reached"
)
echo "subshell rc=$?"

# Last, because it changes the mode for the rest of the file: the complaint is
# raised by `exp_jump_to_top_level (DISCARD)` and never reaches posix mode's
# "an expansion error ends the shell" hook, so this is the one line after one
# of these that still runs.
echo "=== posix mode does not make it fatal"
set -o posix
echo "${a[}"
echo "posix: still running"
