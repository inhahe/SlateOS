# An array subscript and a substring bound are handed to `expand_arith_string`
# with `Q_DOUBLE_QUOTES` set, and that bit switches the single quote *off*:
# `expand_word_internal` never treats a `'` as an opener, so it walks straight
# through the pair. There is one string here — the whole fragment — and the
# `' … '` is two ordinary characters of it.
#
# That matters when a scan inside the run gives up, because both "no closing"
# diagnostics echo the string the scan was handed (`report_error (…, string)`,
# subst.c:1498 for `$[ … ]` and subst.c:1972 for `${ … }`). The string is the
# fragment, quotes and neighbours included — never the run's interior, which is
# not a string bash ever carves out.
#
# Verified against bash 5.2.37.

declare -a arr=(10 20 30)
declare -A m=([k]=V)
v=abcdef

echo "=== a brace whose body scan runs off the end of the fragment ==="
echo "[${arr['x${m:-']}]"

echo "=== the same, with text after the run to be echoed with it ==="
echo "[${arr['x${m:-'y]}]"

echo "=== a \$[ … ], whose own scanner echoes the same string ==="
echo "[${arr['x$[1 ']}]"

echo "=== the offset of a substring, which is the same kind of fragment ==="
echo "[${v:'x${m:-':2}]"

echo "=== and its length ==="
echo "[${v:1:'x$[1 '}]"

echo "=== a run that closes nothing early is unaffected ==="
echo "[${arr['1']}]"

echo TAIL
