# `PS4` is a *variable*, and `set -x` re-expands it before every line.
#
# Three things follow, all observable:
#
#   * The `+ ` everyone knows is the value bash *seeds* the variable with, not a
#     fallback the tracer reaches for. `unset PS4` therefore leaves no prefix at
#     all, and `declare -p PS4` prints it like any other variable.
#   * The value is expanded as a prompt string — the `\`-escapes of `PS1`, and
#     then parameter, arithmetic and command substitution over the *result*, so
#     `$LINENO` in a `PS4` tracks the line being traced. It is double-quote
#     context: nothing is word-split or globbed, and a `'` or `"` stays put.
#     Tracing is off while it expands, so a `$( … )` inside `PS4` cannot trace
#     itself.
#   * The prefix's *first character* is repeated once per level of expansion
#     indirection: the body of a command substitution traces one `+` deeper,
#     nested substitutions deeper still. A `( … )` subshell, a function call and
#     a pipeline are not expansions and add nothing.
#
# Escapes whose expansion depends on the host (`\u`, `\h`, `\t`, `\w`, `\s`) are
# left out — see xtrace-quoting.sh for the word-quoting half of the trace.

echo "=== PS4 is a variable, seeded with '+ '"
echo "[$PS4]"
declare -p PS4
( set -x; : ) 2>&1
( unset PS4; set -x; : ) 2>&1
( PS4=; set -x; : ) 2>&1
( PS4='T '; set -x; : ) 2>&1

echo "=== re-expanded before every line"
( PS4='+ $LINENO '; set -x; :; : ) 2>&1
( x=1; PS4='+$x '; set -x; x=2; : ) 2>&1
( PS4='+ $((2*3)) '; set -x; : ) 2>&1
( PS4='+$(echo q) '; set -x; : ) 2>&1
( PS4='+${nosuch:+ }'; set -x; : ) 2>&1
( PS4='+${x:+ }'; x=; set -x; : ) 2>&1

echo "=== double-quote context: no splitting, no globbing, quotes are literal"
: > ps4glob
( PS4='+* '; set -x; : ) 2>&1
( v='a   b'; PS4="+$v "; set -x; : ) 2>&1
( PS4="+'\" "; set -x; : ) 2>&1
( PS4='+ \061\062 '; set -x; : ) 2>&1
( PS4='+ a\qb '; set -x; : ) 2>&1
( PS4='+ a\\b '; set -x; : ) 2>&1
rm -f ps4glob

echo "=== the first character repeats once per level of expansion"
( set -x; v=$(echo a) ) 2>&1
( set -x; v=`echo c` ) 2>&1
( set -x; v=$(echo $(echo b)) ) 2>&1
( set -x; v=$( ( echo t ) ) ) 2>&1
( set -x; f() { echo F; }; v=$(f) ) 2>&1
( set -x; echo $(echo x) $(echo y) ) 2>&1
( set -x; echo $((1+$(echo 2))) ) 2>&1
( set -x; v=$(v2=$(echo n); echo $v2) ) 2>&1
( PS4='XY '; set -x; v=$(echo a) ) 2>&1
( PS4=; set -x; v=$(echo a) ) 2>&1

# A pipeline is not an expansion either, but its stages' trace lines are not
# ordered by osh the way bash orders them — see known-issues.md
# TD-OILS-XTRACE-PIPE-ORDER — so there is no pipeline here.
echo "=== a subshell and a function are not expansions"
( set -x; ( echo s ) ) 2>&1
( set -x; g() { echo G; }; g ) 2>&1
