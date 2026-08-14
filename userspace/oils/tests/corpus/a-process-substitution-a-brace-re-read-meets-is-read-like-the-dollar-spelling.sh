# `extract_dollar_brace_string` — the scan that finds the `}` a `${ … }` ends
# at, over text no parser ever read as a word — names all three substitution
# spellings in one row and hands each to `extract_command_subst`
# (subst.c:1881-1950), which is a real parse (`xparse_dolparen`). So a body
# between the braces is read for its extent whichever delimiter opened it, and
# one that will not parse fails identically: measured against bash 5.2.37,
# `A${z:-<(fi)}TAIL` and `A${z:-$(fi)}TAIL` under `${x@P}` are byte for byte the
# same, down to the remainder the diagnostic quotes — which starts at the body
# and so never names the delimiter at all.
#
# The failure runs the same course as the `$(` spelling's: `si` is left past the
# end of the string, so the brace never closes, so `parameter_brace_expand`
# reports `bad substitution` naming the whole text and the text is printed
# unchanged.
#
# What is *not* shared is everything after the read. Only a `$(` is performed:
# `expand_word_internal` declines a process substitution under `W_DQUOTE`
# (subst.c:11079), and the re-read of a `${x@P}` always is one — so
# `A${z:-<(echo A >&2)}B` prints `A<(echo A >&2)B`, runs nothing, and writes
# nothing to stderr, quoted or unquoted.
#
# The row is this scan's, so it is reached only where this scan's own quoting
# leaves it reachable: a `"` run, a `'` run and a backslash each shield it, and
# a subscript is stepped over whole. Text outside any `${ … }` is not scanned
# at all.
#
# The `$(` spelling this mirrors is
# a-command-substitution-in-a-brace-body-is-read-where-the-text-is-expanded.sh;
# the *other* second scan with a `<(` row of its own — `brace_gobbler`, which
# is a different construct's — is
# a-process-substitution-a-brace-scan-meets-is-read-where-the-quoting-is-clear.sh.
#
# Verified against bash 5.2.37.

e() { ( eval "$1" ) 2>&1; echo "  rc=$?"; }

echo "=== read like the dollar spelling, down to the quoted remainder ==="
e 'x='\''A${z:-<(fi)}TAIL'\''; echo "${x@P}"'
e 'x='\''A${z:-$(fi)}TAIL'\''; echo "${x@P}"'
e 'x='\''A${z:->(fi)}TAIL'\''; echo "${x@P}"'
e 'x='\''A${z:-<(for)}B'\''; echo "${x@P}"'
e 'x='\''A${z:-<(fi)}B${y:-<(for)}C'\''; echo "${x@P}"'
e 'x='\''A${z:-${y:-<(fi)}}B'\''; echo "${x@P}"'

echo "=== in the order the one scan meets them, both spellings together ==="
e 'x='\''A${z:-p<(fi)q$(for)r}B'\''; echo "${x@P}"'
e 'x='\''A${z:-p$(fi)q<(for)r}B'\''; echo "${x@P}"'

echo "=== read, never performed ==="
e 'x='\''A${z:-<(echo HI)}B'\''; echo "${x@P}"'
e 'x='\''A${z:->(cat)}B'\''; echo "${x@P}"'
e 'x='\''A${z:-<(echo A >&2)}B'\''; echo "${x@P}"'
e 'x='\''A${z:-<(echo HI)}B'\''; echo ${x@P}'
e 'x='\''A${z:-<(echo HI)}B'\''; y=${x@P}; echo "$y"'

echo "=== read before the operand is chosen, and either way ==="
e 'z=Z; x='\''A${z:-<(fi)}B'\''; echo "${x@P}"'
e 'x='\''A${z:+<(fi)}B'\''; echo "${x@P}"'
e 'z=Z; x='\''A${z:+<(echo HI)}B'\''; echo "${x@P}"'

echo "=== only where this scan's quoting leaves it reachable ==="
e 'x='\''a<(fi)b'\''; echo "${x@P}"'
e 'x='\''A${z:-"<(fi)"}B'\''; echo "${x@P}"'
x="A\${z:-'<(fi)'}B"; e 'echo "${x@P}"'
e 'x='\''A${z:-\<(fi)}B'\''; echo "${x@P}"'
e 'x='\''A${z[<(fi)]:-q}B'\''; echo "${x@P}"'

echo "=== the same re-read, reached by PS4 rather than @P ==="
e 'PS4='\''A${z:-<(fi)}B'\''; set -x; :; set +x'
echo TAIL
