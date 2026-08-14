# bash's `parse_matched_pair` parses a `$( … )` *where it meets it*, so a body
# that will not parse is reported from there — and the `"` around it never gets
# to notice that it is missing its own closing quote. `echo " $(fi)` is
# therefore a `near unexpected token 'fi'`, not the `unexpected EOF while
# looking for matching '"'` that the quote alone would have raised.
#
# That is a *parse-order* fact, not a quoting one: the same body written in a
# backquote is read as text now and parsed only at expansion time, so nothing
# says anything before the quote runs out. A `${ … }` and a `$(( … ))` step over
# their nested `$( … )` with the same eager parse and behave like the bare one.
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
echo TAIL
