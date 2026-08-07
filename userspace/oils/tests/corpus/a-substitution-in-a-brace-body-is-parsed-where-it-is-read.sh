# bash parses a `$( … )` inside a `${ … }` body **eagerly, as it reads the
# body** — `parse_matched_pair` hands the `$(` to `parse_dollar_word` and from
# there to `parse_comsub` (parse.y:3954), exactly as it does under `P_ARITH`.
#
# So the nested body's syntax error happens *before* anything judges the
# enclosing `${ … }`, and beats every verdict that judgement could reach: a
# runtime `bad substitution`, an outright refusal of the body's shape, and the
# branch not being taken at all. It is a parse-time event, not an
# expansion-time one.
#
# A **backtick** is not parsed eagerly — bash reads that body only when the
# substitution runs — and neither is anything inside a `'…'`, which is opaque
# to the scan. Those two are the whole of the exception list.
#
# Verified against bash 5.2.37.

e() { ( eval "$1" ) 2>&1; echo "  rc=$?"; }

echo "=== it beats a runtime bad substitution ==="
e 'echo ${#x:-$(fi)}'
e 'echo ${x@Z$(fi)}'
e 'echo ${!x@Q$(fi)}'
e 'echo ${1[0]$(fi)}'

echo "=== …and an outright refusal of the body's shape ==="
e 'echo ${$(fi)}'
e 'echo ${@[0]$(fi)}'

echo "=== …and it fires with the branch untaken ==="
e 'if false; then echo ${#x:-$(fi)}; fi'
e 'false && echo ${#x:-$(fi)}'
e 'f() { echo ${#x:-$(fi)}; }'

echo "=== every place in the body the scan reaches ==="
e 'echo ${x[$(fi)]}'
e 'echo ${#x:-"$(fi)"}'
e 'echo ${#x:-${y:-$(fi)}}'
e 'echo ${#x:-$(( 1 + $(fi) ))}'
e 'echo ${#x:-$[ 1 + $(fi) ]}'
e 'echo ${#x:-$(echo $(fi))}'

echo "=== …and the two it does not ==="
# A backtick body is read when the substitution runs, so the `bad substitution`
# gets there first; a `'…'` is opaque, so there is no substitution at all.
e 'echo ${#x:-`fi`}'
e 'if false; then echo ${#x:-`fi`}; fi'
e "echo \${#x:-'\$(fi)'}"

echo "=== a well-formed body is still only text, run once ==="
e 'n=0; inc() { n=$((n+1)); }; echo "[${x:-$(inc)}]"; echo "n=$n"'
e 'x=Q; n=0; inc() { n=$((n+1)); }; echo "[${x:-$(inc)}]"; echo "n=$n"'
e 'echo "[${x:-$(echo 2)}]"'
e 'echo "[${x:-$(cat <<E
hi
E
)}]"'

echo "=== the error is blamed on the physical line ==="
e 'echo one
echo ${x:-$(
fi
)}'

echo done
