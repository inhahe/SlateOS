# A `${ … }` body's process substitution is *performed*, and what decides it is
# the quoting the expansion runs under — not where in the body it was written.
# `expand_word_internal` reads one only when the flags say it may: `if
# (string[++sindex] != LPAREN || (quoted & (Q_HERE_DOCUMENT|Q_DOUBLE_QUOTES)) ||
# (word->flags & W_NOPROCSUB))` takes the `<` for an ordinary character
# (subst.c:11079), and `process_substitute` is on the other side of that test.
#
# So the body splits three ways. An **operand** inherits the expansion it was
# written in: bare it runs, and inside `" … "` it stays the characters it was
# written as — including a `" … "` or `' … '` run *within* a bare one, which
# quotes it just as surely. A **pattern** and a **replacement** run either way,
# because `expand_word_internal` is re-entered for those without
# `Q_DOUBLE_QUOTES` whatever surrounded the `${`. And a **subscript** or a
# **substring bound** never runs one, being expanded `Q_DOUBLE_QUOTES|Q_ARITH`
# — the arithmetic error names the characters, not a `/dev/fd/N`.
#
# Nothing below prints a substitution's path: bash names it `/dev/fd/N` and osh
# a temporary file, so each case asks a question the path itself does not
# answer — whether the text still begins `<(`, whether it names something that
# exists, or what a `cat` of it reads.
#
# The *parse* half of this construct — that the body is read where the scan
# meets it, and that what survives is the parse re-printed — is its own case:
# see a-process-substitution-in-a-brace-body-is-parsed-where-it-is-met.sh.
#
# Verified against bash 5.2.37.

e() { ( eval "$1" ) 2>&1; echo "  rc=$?"; }
w() { case "$1" in '<('*|'>('*) echo "  literal" ;; *) echo "  performed" ;; esac; }

echo "=== a bare operand performs ==="
e 'cat ${z:-<(echo hi)}'
e '[ -e ${z:-<(echo hi)} ] && echo yes'
e 'cat ${z:=<(echo hi)}; w "$z"'
e 'z=set; cat ${z:+<(echo hi)}'
e 'x=${z:-<(echo hi)}; w "$x"'
e 'w ${z:-<(echo a)<(echo b)}'
e '[ -e ${z:->(cat >/dev/null)} ] && echo yes'

echo "=== a double-quoted operand does not ==="
e 'echo "${z:-<(echo hi)}"'
e 'w "${z:-<(echo hi)}"'
e 'echo "${x:-${y:-<(echo hi)}}"'
e 'w "${x:-${y:-<(echo hi)}}"'
e 'cat ${x:-${y:-<(echo hi)}}'

echo "=== nor does a quoted run inside a bare one ==="
e 'echo ${z:-"<(echo hi)"}'
e "echo \${z:-'<(echo hi)'}"
e 'echo ${z:-\<\(echo hi\)}'

echo "=== a pattern performs, quoted or not ==="
e 'z="<(echo hi)"; echo "[${z#<(echo hi)}]"'
e 'z="<(echo hi)"; echo "[${z%<(echo hi)}]"'
e 'z="<(echo hi)"; echo [${z##<(echo hi)}]'
e 'z="<(echo hi)"; echo "[${z/<(echo hi)/X}]"'

echo "=== so does a replacement ==="
e 'z=X; w "${z/X/<(echo hi)}"'
e 'z=X; w ${z/X/<(echo hi)}'

echo "=== arithmetic text does not ==="
e 'echo "${a[<(echo 1)]}"'
e 'a=(x y z); echo ${a[<(echo 1)]}'
e 'z=abcdef; echo "${z:<(echo 1)}"'
e 'z=abcdef; echo "${z:1:<(echo 1)}"'
e 'z=abcdef; echo $((<(echo 1)))'
echo TAIL
