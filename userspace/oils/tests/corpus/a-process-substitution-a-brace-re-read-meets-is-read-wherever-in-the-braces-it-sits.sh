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
# The **bounds** of `${z:o:l}` are the position that shows the two halves are
# separate questions. The scan walks a bound — it is not a subscript, nothing
# steps over it — so the read happens and a body that will not parse gives this
# file's three lines. But the *expansion* is `expand_arith_string`'s, under
# `Q_DOUBLE_QUOTES|Q_ARITH`, which is exactly what stops `expand_word_internal`
# performing one (subst.c:11079). So a bound is read like a pattern and
# performed like an operand, and a well-formed `<( … )` in one reaches the
# arithmetic evaluator as the characters it was written with.
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
x="A\${z:'<(fi)'}B"; e 'echo "${x@P}"'
e 'x='\''A${z:"<(fi)"}B'\''; echo "${x@P}"'
e 'x='\''A${z:\<(fi)}B'\''; echo "${x@P}"'

echo "=== a bound is walked as well, being no subscript ==="
e 'x='\''A${z:<(fi)}B'\''; echo "${x@P}"'
e 'x='\''A${z:0:<(fi)}B'\''; echo "${x@P}"'
e 'x='\''A${z:>(fi)}B'\''; echo "${x@P}"'
e 'a=(q w); x='\''A${a[@]:<(fi)}B'\''; echo "${x@P}"'
e 'x='\''A${@:<(fi)}B'\''; echo "${x@P}"'
e 'x='\''A${z:<(fi)}B'\''; echo ${x@P}'
e 'PS4='\''A${z:<(fi)}B'\''; set -x; :; set +x'

echo "=== …and read is all it is: the evaluator meets the characters ==="
e 'z=abcdef; echo "${z:<(echo 1)}"'
e 'z=abcdef; echo "${z:0:<(echo 1)}"'
e 'z=abcdef; x='\''${z:<(echo 1)}'\''; echo "${x@P}"'

echo "=== read and performed, unlike the operand — shape only, never the path ==="
e 'z=Z; x='\''A${z/Z/<(echo hi)}B'\''; y=${x@P}; case $y in A?*B) echo shaped;; *) echo "other:$y";; esac'
e 'x='\''${z:-<(echo hi)}'\''; y=${x@P}; echo "[$y]"'
echo TAIL
