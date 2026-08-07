# The reader hands the parser one token at a time, so by the time it chokes on
# an unclosed construct the parser has already been given — and has already
# looked at — every token the line yielded in front of it. A grammar error among
# those is raised first and wins: `) echo "` is reported on the stray `)`, and
# the quote it never reached is not mentioned at all. Only when nothing objects
# does the fetch that runs into the construct happen, and its error stand.
#
# The line boundary is about *execution*, not about tokens. Whatever the unit
# had parsed goes down with the error, so nothing on the failing line runs —
# while every line before it already has, the reader having handed those over
# whole.
#
# Each case needs an input of its own, because the error ends the one it is in.
# Verified against bash 5.2.37.

e() { ( eval "$1" ) 2>&1; echo "  rc=$?"; }

echo "=== a token the line already yielded outranks the quote behind it ==="
e ') echo "'
e 'echo a; ) echo "'
e 'fi "'
e 'done "unterm'
e 'esac "unterm'
e 'then "unterm'
e '} "unterm'
# The construct need not be a quote, and need not be what the line ends on.
e ') $('
e ') cat <<E'
# A word before the offending token is no help: the error is still the token.
e 'echo one; ) echo "'

echo "=== nothing objects, so the construct is reported and its line dies ==="
# The quote is first *in the fetch order* here, the `)` sitting behind it.
e 'echo "; )'
e 'echo one; echo "unterm'
e 'echo one && echo "unterm'
e 'echo one | echo "unterm'
e 'for i in 1; do echo "unterm'
e 'echo one; { echo "unterm'
e "if true; then
echo 'unterm"

echo "=== earlier lines have already run; the failing one has not ==="
e 'echo one
echo two; echo "unterm'
e 'echo one
echo two
echo three; echo four "unterm'
# A newline ends its unit however close the failure is behind it: the parser
# never fetched past it, so `two` runs.
e "echo one
echo two
v='abc
"
# Not probed here: `echo one &` on the first line. It runs — the unit is
# complete — but its output races the diagnostic, which the parser prints before
# the job is reaped, so the two orders alternate between runs of the same shell.
# `parser.rs::a_line_that_fails_to_lex_still_offers_the_tokens_it_had` pins the
# shape instead, where only the unit count is asked about.

echo "=== a grammar error on an earlier line pre-empts the construct ==="
# bash abandons the input at the error, so the quote below is never read.
e 'echo one )
echo "unterm'

echo "=== a here-document still awaiting its body is warned about below ==="
e 'cat <<E "'
e 'echo hi; cat <<E "'
# Not at the top level, so the reduction that would gather never runs and there
# is no warning — only the quote.
e '{ cat <<E "'

echo done
