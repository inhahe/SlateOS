# `string_extract_double_quoted` is handed a *finished word*, not a stream, so
# a `"` it opens can simply run out of text. That is not an error the way an
# unterminated `"` in a script is: the walk ends at the end of the string as
# readily as at a quote (subst.c, `string_extract_double_quoted`), and the
# expansion carries on with the run it got.
#
# Such a run is reachable only where a word arrives already built — `${x@P}`,
# `PS4`, a here-document body — because there a `'` is an ordinary character
# (quote removal is long past) while a `"` is still a quote. So a `"` written
# *inside* what the source spelled as a `' … '` run is a quote with no mate:
# the `'`s are text, and nothing after the `"` closes it.
#
# The value is unaffected either way — quote removal drops the `"` whichever
# rule found it. What the run does decide is the word's **text**, and two
# diagnostics print that text back: the word `bad substitution` names, and the
# remainder `extract_command_subst` quotes in its own complaint. Neither may
# show a byte the source never held.
#
# Verified against bash 5.2.37.

z=ZZ

echo "=== the plain row: a dquote opened inside a squote run, comsub after ==="
v='A${z:-'"'"'i"t'"'"'$(fi)}B'; printf '[%s]\n' "${v@P}"

echo "=== the same with a procsub, which the scan reads and the expansion does not ==="
v='A${z:-'"'"'i"t'"'"'<(fi)}B'; printf '[%s]\n' "${v@P}"

echo "=== two squote runs, the dquote opening in the second ==="
v='A${z:-'"'"'p'"'"''"'"'q"r'"'"'$(fi)}B'; printf '[%s]\n' "${v@P}"

echo "=== a nested brace, whose own text is printed back too ==="
v='A${z:-${y:-'"'"'i"t'"'"'$(fi)}}B'; printf '[%s]\n' "${v@P}"

echo "=== …and the row where the run *does* close: the mate is the source's own ==="
v='A${z:-'"'"'i"t"u'"'"'$(fi)}B'; printf '[%s]\n' "${v@P}"

echo "=== an unmated dquote at brace level is not a run at all: it stays text ==="
v='A${z:-p"q$(fi)}B'; printf '[%s]\n' "${v@P}"

echo "=== PS4 reads its text the same way ==="
(set -x; PS4='A${z:-'"'"'i"t'"'"'$(fi)}B '; : one) 2>&1

echo "=== and so does a here-document body ==="
cat <<E
A${z:-'i"t'$(fi)}B
E

echo TAIL
