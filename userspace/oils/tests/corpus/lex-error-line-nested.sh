# When constructs nest, the *innermost* one that failed names the line — the
# outer scan never gets a chance to report its own missing delimiter. Here the
# `$( … )` would report at end of input (see lex-error-line-cmdsub.sh), but the
# `'` inside it opened first and hits EOF first, so its opening line wins.
#
# Verified against bash 5.2.37: the diagnostic is `matching `''` on the last
# line, not `matching `)'` one line past the end.

echo one
echo two
v=$(echo a
echo b
echo 'unterm
