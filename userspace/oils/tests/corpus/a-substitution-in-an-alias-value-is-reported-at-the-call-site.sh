# A `$( … )` written in an *alias value* is lexed on its own, so it remembers a
# line number of the value's own numbering — which starts at 1. But a
# replacement is not a line of the script and has none of its own: bash bumps
# `line_number` only when it *fetches* an input line (parse.y), and reading a
# pushed alias string is not a fetch. So a substitution that fails to parse in
# an alias value is reported at the line the alias word was written on, exactly
# like every other token the replacement contributes.
#
# The error is the enclosing parse's, not the expansion's — bash finds the `)`
# while scanning the command — so it is fatal to the whole input, which is why
# each shape runs in its own shell.

shopt -s expand_aliases
s() { sed 's/^.*: line /line /'; }
r() { "$BASH" --norc -c "shopt -s expand_aliases
$1" 2>&1 | s; }

echo "=== the call site's line, not the value's"
r 'alias A="echo \$( for )"
A'
r 'alias A="echo \$( for )"
echo before
A'
r 'alias A="echo \$( for )"
echo one; A'
r 'alias A="echo \$( ! )"
A'

echo "=== and the echoed line is the replacement, while the reader is still in it"
# The error is found with the reader at the end of the word holding the
# substitution — the body is parsed while that word is lowered. Text written
# after the substitution in the value leaves the reader inside the replacement,
# so the replacement is what is echoed; a substitution that ends the value is
# read past, which pops it, and the script's own line is echoed instead (the
# section above). One token later the reader has left the word either way, so
# the two cases are only distinguishable from where the word ends.
r 'alias A="echo \$( for ) tail"
A'
r 'alias A="echo \$( for ) tail"
echo one; A'
r 'alias A="cat <( for ) tail"
A'
r 'alias A="x=\$( for ) echo hi"
A'
r 'alias A="echo hi > \$( for ) tail"
A'
r 'alias A="case \$( for ) in x) ;; esac tail"
A'
r 'alias A="for i in \$( for ) tail; do :; done"
A'
r 'alias A="[[ -n \$( for ) ]] tail"
A'

echo "=== a process substitution records a line the same way"
r 'alias A="cat <( for )"
A'
r 'alias A="cat >( for )"
A'

echo "=== and an arithmetic-looking one does not record a line at all"
r 'alias A="echo \$(( 1 + ))"
A'

echo "=== the same substitution written where it is read, for contrast"
r 'echo $( for )'
r 'echo before
echo $( for )'

echo "=== a substitution in an alias value that parses is untouched"
r 'alias A="echo \$( echo inner )"
A'
r 'alias A="echo \$( echo one ) \$( echo two )"
A'
r 'alias A="echo x"
y=$(A)
echo "[$y]"'
echo done
