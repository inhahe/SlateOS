# An arithmetic string is a word expansion of its own — bash's
# `expand_arith_string` hands it to `expand_word_internal` before `evalexp` ever
# sees it — so a `$name` in it is read by the same code that reads one in a
# word, and `set -u` catches it there: `err_unboundvar` fires and the whole
# shell goes down, exactly as for a bare `echo $y`.
#
# The arithmetic evaluator has its *own* unset rule (a bare `y`, with no `$`, is
# a name it looks up itself), and that one is checked too — but only after the
# expansion pass has finished with the text. So the two never both fire, and the
# expansion's failure ends the string where it stood: nothing after the offending
# `$name` is expanded and the expression is never evaluated.
#
# Verified against bash 5.2.37.

e() { ( eval "$1" ) 2>&1; echo "  rc=$?"; }

echo "=== every arithmetic context reads its \$name the same way ==="
e 'set -u; echo $(( $y + 1 ))'
e 'set -u; echo $[ $y + 1 ]'
e 'set -u; echo $(( 1 + $y ))'
e 'set -u; (( $y )); echo "after=$?"'
e 'set -u; for (( $y; 0; )); do echo x; done'
e 'set -u; let "$y + 1"'
e 'set -u; declare -i n; n=$y+1'
e 'set -u; a=(1); echo $(( a[$y] ))'

echo "=== the string is abandoned at the first one ==="
# One diagnostic, not two: the text after the failure is never expanded, and the
# hole it left never reaches the evaluator.
e 'set -u; echo $(( $y + $z ))'
e 'set -u; (( $y + $z ))'
# Two separate strings, though, are two separate expansions — and the abort ends
# the shell at the first, so the second is never begun.
e 'set -u; echo $(( $y )) $(( $z ))'
# …unless the failure was a *subshell's*, which does not unwind this one.
e 'set -u; echo $(( 1 + $(echo $y) )); echo after'

echo "=== and what is bound, or exempt, passes through ==="
e 'set -u; y=; echo $(( $y + 1 ))'
e 'set -u; y=0; echo $(( $y + 1 ))'
e 'set -u; set -- 5; echo $(( $1 + 1 ))'
e 'set -u; set -- ; echo $(( $# ))'
e 'set -u; echo $(( ${y:-4} + 1 ))'
e 'set -u; echo $(( ${y+9} + 1 ))'
# A `\$` is not an expansion at all, so there is nothing to look up.
e 'set -u; a=3; echo $(( \$a ))'

echo "=== without -u it is the empty string, as before ==="
e 'echo $(( $y + 1 ))'
e 'echo $(( $y ))'
e 'for (( $y; 0; )); do echo x; done; echo "rc0=$?"'

echo done
