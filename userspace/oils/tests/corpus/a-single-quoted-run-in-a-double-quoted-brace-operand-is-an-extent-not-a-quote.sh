# A `' … '` inside a `${ … }` that is itself inside double quotes is read as an
# *extent* and not as a quote: bash's parser skips from the `'` to its mate
# without reading a thing in between, yet the characters it skipped stay live
# and are expanded afterwards with the `'` for an ordinary character.
#
# `${ … }` is a grouping construct — `open != close` — so `parse_matched_pair`
# meets the `'` as one of that construct's shell quotes and hands the run to a
# `parse_matched_pair ('\'', '\'', '\'', …)` of its own (parse.y:3840-3846),
# which reads to the mate and reads *nothing* between. The rule that would
# instead step over the `'` and leave the run's characters to the outer scan is
# the posix one (parse.y:3836), and it is off in default mode.
#
# Two things follow, and both are measured here. The run is an extent, so a `}`
# or a `"` inside it does not end the construct; and nothing inside it was read
# as source, so `declare -f` gives the operand back exactly as written rather
# than re-printing a parse of it — a `$( … )` in there was never parsed at all.
#
# Outside double quotes the same run *is* an ordinary quote, and its contents
# are protected from expansion. That is the contrast the last section draws.
#
# Verified against bash 5.2.37.

unset z
z2=abc
a=(A B C)
p() { printf '%-30s -> [%s]\n' "$1" "$2"; }
e() { ( eval "$1" ) 2>&1; echo "  rc=$?"; }

echo "=== the run's characters stay live ==="
p 'default'      "${z:-'$(echo Q)'}"
p 'alternate'    "${z:+'$(echo Q)'}"
p 'trim-hash'    "${z#'$(echo Q)'}"
p 'replace'      "${z//x/'$(echo Q)'}"
p 'dollar'       "${z:-'$z2'}"
p 'backquote'    "${z:-'`echo Q`'}"
p 'nested brace' "${z:-'${z2}'}"

echo "=== …but it is an extent, so a closer inside it does not close ==="
p 'brace in run'  "${z:-'a}b'}"
p 'dquote in run' "${z:-'a\"b'}"
p 'backslash'     "${z:-'a\b'}"

echo "=== …and nothing in it was read as source ==="
f1() { echo "[${z:-'$( (echo 2) )'}]"; }
f2() { echo "[${z:+'$( (echo 2) )'}]"; }
f3() { echo "[${z//x/'$( (echo 2) )'}]"; }
f4() { echo "[${z#'$( (echo 2) )'}]"; }
f5() { echo "[${z%'$( (echo 2) )'}]"; }
f6() { echo "[${z:'$( (echo 2) )':1}]"; }
f7() { echo "[${a['$( (echo 2) )']}]"; }
f8() { echo "[${z^'$( (echo 2) )'}]"; }
f9() { echo "[${z/'$( (echo 2) )'}]"; }
g1() { echo "${z:-'a}b'}"; }
g2() { echo "${z:-'a\"b'}"; }
g3() { echo "${z:-'`echo Q`'}"; }
declare -f f1 f2 f3 f4 f5 f6 f7 f8 f9 g1 g2 g3

echo "=== a run with no mate runs to the end of the input ==="
e 'echo "${z:-'"'"'abc}"'

echo "=== outside double quotes the run *is* a quote ==="
p 'bare default' ${z:-'$(echo Q)'}
p 'bare trim'    ${z#'$(echo Q)'}
p 'bare dollar'  ${z:-'$z2'}
echo TAIL
