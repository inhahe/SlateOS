# bash's `parse_matched_pair` parses a `$( … )` *where it meets it*, so a body
# that will not parse is reported from there — and whatever is still open around
# it never gets to notice that it is missing its own delimiter. `echo " $(fi)`
# is therefore a `near unexpected token 'fi'`, not the `unexpected EOF while
# looking for matching '"'` that the quote alone would have raised, and
# `echo $(fi)x$(` names the *first* substitution even though it is the second
# one that never closes.
#
# The scope is the one word. An earlier word finished and is a token the parser
# reaches on its own, so `echo a$(fi) b$(` needs none of this; a word that never
# finished yields no token at all, and the bodies in it would otherwise never be
# parsed by anyone.
#
# It is a *parse-order* fact, not a quoting one: the same body written in a
# backquote is read as text now and parsed only at expansion time, so nothing
# says anything before the word runs out. A `${ … }` and a `$(( … ))` step over
# their nested `$( … )` with the same eager parse and behave like the bare one,
# and so does a `<( … )` / `>( … )`.
#
# The rc tells the two apart as loudly as the message does. A body's parse error
# is fatal to whoever was reading the body — the eval dies with 1 — where an
# ordinary syntax error scores 2.
#
# Verified against bash 5.2.37.

e() { ( eval "$1" ) 2>&1; echo "  rc=$?"; }

echo "=== the eager body is parsed first, so its error is the one reported ==="
e 'echo " $(fi)'
e 'echo " ${x:-$(fi)}'
e 'echo " $(( $(fi) ))'
e 'echo " $(fi)"'

echo "=== a backquote body is not parsed there, so the quote is missed first ==="
e 'echo " `fi`'
e 'echo "abc'
e 'echo " $(echo ok)'

echo "=== reading is left to right, so the first body that will not parse wins ==="
e 'echo " $(fi) $(done)'
e 'echo " $(fi) $(done'
e 'echo " $(fi) `done`'
e 'echo " $(done'

echo "=== a body that merely ran out said nothing, so the ) is reported ==="
e 'echo " $(a |'
e 'echo " $(echo ok) $(if'

echo "=== the quote is not what makes it eager — the word is ==="
e 'echo $(fi)x$('
e 'echo $(fi)x${y'
e 'echo $(fi)x$((1+'
e 'echo $(fi)x"y'
e 'echo $(fi)x'"'"'y'
e 'echo $(fi)x`y'
e 'echo "a$(fi)"x$('
e 'echo ${x[$(fi)]}y$('

echo "=== a process substitution is parsed as eagerly ==="
e 'echo <(fi)x$('
e 'echo >(fi)x$('
e 'echo $(fi)x <('

echo "=== an earlier word already finished, so it needs none of this ==="
e 'echo a$(fi) b$('
e 'echo a$(fi); echo b$('
e 'echo $(echo ok)x$('
e 'echo `fi`x$('
echo TAIL
