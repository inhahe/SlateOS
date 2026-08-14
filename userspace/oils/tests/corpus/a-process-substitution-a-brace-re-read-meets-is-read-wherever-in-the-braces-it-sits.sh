# `extract_dollar_brace_string` walks a `${ … }` body as characters, not as an
# operator and its operand: the scan that names `$(`, `<(` and `>(` in one row
# (subst.c:1881-1950) knows nothing of the `#`, `/`, `^^` or `:` it has already
# walked past. So the read is the same in every position a body has, and a body
# that will not parse fails identically in all of them — two diagnostics from
# the parse, then `bad substitution` naming the whole text, then the text
# printed unchanged.
#
# That is the half this file measures, and it is the half osh had only for the
# `:-` operand: the sibling file
# a-process-substitution-a-brace-re-read-meets-is-read-like-the-dollar-spelling.sh
# covers the operand, where the construct is read and then *not* performed
# (`expand_word_internal` declines one under `W_DQUOTE`, subst.c:11079). Here it
# is read *and* performed — a pattern and a replacement are re-entered without
# `Q_DOUBLE_QUOTES` whatever surrounded the braces — so the rows that reach a
# good body check only the shape of the answer, never the path, which is the
# host's to choose.
#
# The reachability rules are the scan's own and so are unchanged: a `"` run, a
# `'` run and a backslash each shield it, and a subscript is stepped over whole
# (and then fails as arithmetic, which is a different construct's complaint).
#
# One position is deliberately left out: an **arithmetic** fragment — the offset
# and length of `${z:o:l}`. The scan reads it the same way and bash reports the
# same three lines, but osh's arithmetic there diverges over `<` before any of
# this (`${z:1<(2)}` is `bcdef` in bash and an `operand expected` in osh, with no
# process substitution written at all), so a row here would be measuring that
# instead. See known-issues
# TD-OILS-A-LESS-THAN-IN-A-BRACE-ARITHMETIC-FRAGMENT-LOSES-ITS-LEFT-OPERAND.
#
# Verified against bash 5.2.37.

e() { ( eval "$1" ) 2>&1; echo "  rc=$?"; }

echo "=== every position the one scan walks past, one row each ==="
e 'x='\''A${z#<(fi)}B'\''; echo "${x@P}"'
e 'x='\''A${z##<(fi)}B'\''; echo "${x@P}"'
e 'x='\''A${z%<(fi)}B'\''; echo "${x@P}"'
e 'x='\''A${z/<(fi)/q}B'\''; echo "${x@P}"'
e 'x='\''A${z/p/<(fi)}B'\''; echo "${x@P}"'
e 'x='\''A${z^^<(fi)}B'\''; echo "${x@P}"'
e 'x='\''A${z,,<(fi)}B'\''; echo "${x@P}"'

echo "=== the same read, whichever way the re-read is spelled ==="
e 'x='\''A${z#<(fi)}B'\''; echo ${x@P}'
e 'x='\''A${z#>(fi)}B'\''; echo "${x@P}"'
e 'PS4='\''A${z#<(fi)}B'\''; set -x; :; set +x'

echo "=== in the order the one scan meets them, both spellings together ==="
e 'x='\''A${z#p<(fi)q$(for)r}B'\''; echo "${x@P}"'
e 'x='\''A${z/p$(fi)q/<(for)}B'\''; echo "${x@P}"'

echo "=== only where this scan's quoting leaves it reachable ==="
x="A\${z#'<(fi)'}B"; e 'echo "${x@P}"'
x="A\${z/p/'<(fi)'}B"; e 'echo "${x@P}"'
e 'x='\''A${z#"<(fi)"}B'\''; echo "${x@P}"'
e 'x='\''A${z#\<(fi)}B'\''; echo "${x@P}"'
e 'x='\''A${z[<(fi)]#q}B'\''; echo "${x@P}"'

echo "=== read and performed, unlike the operand — shape only, never the path ==="
e 'z=Z; x='\''A${z/Z/<(echo hi)}B'\''; y=${x@P}; case $y in A?*B) echo shaped;; *) echo "other:$y";; esac'
e 'x='\''${z:-<(echo hi)}'\''; y=${x@P}; echo "[$y]"'
echo TAIL
