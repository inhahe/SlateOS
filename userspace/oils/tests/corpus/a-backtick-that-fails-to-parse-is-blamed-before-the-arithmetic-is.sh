# A backtick is opaque to the `$(( … ))` scan — `parse_matched_pair` calls
# itself with `open == close` and never reaches the `shellquote` branch that
# would parse a `$( … )` in place — so, unlike its sibling case
# arith-scan-parses-a-nested-substitution-in-place, the body survives the scan
# and is read only when the expansion runs.
#
# It is read there in full: bash parses it, blames it exactly as it blames a
# substitution in an ordinary word (`command substitution: line 1: …`), and
# *then* goes on to fail the arithmetic on the empty value the failure left
# behind. Two diagnostics, in that order — the substitution's own first.
#
# The empty value is the point of the second one: a body that would have written
# something before the error writes nothing, so the arithmetic sees a gap rather
# than a partial result.
#
# Verified against bash 5.2.37.

e() { ( eval "$1" ) 2>&1; echo "  rc=$?"; }
p() { printf '%-34s -> [%s]\n' "$1" "$2"; }

echo "=== the body is blamed first, the arithmetic second ==="
e 'echo $(( 1 + `fi` ))'
e 'echo $(( `fi` + 1 ))'
e 'echo $[ 1 + `fi` ]'
e '(( 1 + `fi` )); echo "after=$?"'
e 'let "1 + `fi`"'

echo "=== …and what ran before the error still wrote nothing ==="
e 'echo $(( 1 + `echo 2; fi` ))'
e 'echo "[`echo 2; fi`]"'

echo "=== an empty value is not an error on its own ==="
# So a body that merely fails at *runtime* leaves one diagnostic, not two — and
# `$(( ))` of nothing is 0 rather than a complaint.
e 'echo $(( 1 + `false` ))'
e 'x=$(( `fi` )); echo "x=[$x]"'
e 'echo $(( `exit 3` ))'

echo "=== every arithmetic context reads the body the same way ==="
e 'a=(1 2 3); echo ${a[`fi`]}'
e 'v=abcdef; echo ${v:`fi`:2}'
e 'echo $(( 1 + $(( `fi` )) ))'
e 'for ((i=`fi`; i<0; i++)); do echo x; done; echo "rc=$?"'

echo "=== a well-formed body is read once and its value used ==="
p 'value'        "$(( 1 + `echo 2` ))"
p 'nested'       "$(( `echo 1` + `echo 2` ))"
p 'trimmed'      "$(( 1 + `printf '2\n\n'` ))"
e 'n=0; inc() { n=$((n+1)); echo 1; }; echo $(( 1 + `inc` )); echo "n=$n"'

echo "=== the body is numbered from its own first line ==="
e 'echo $(( 1 + `
fi
` ))'

echo done
