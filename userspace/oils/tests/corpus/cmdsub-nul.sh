# A shell word cannot hold a NUL, so a command substitution that captures one
# does not truncate there — bash *drops* every NUL and warns once per capture,
# however many it dropped. The strip happens before the trailing-newline strip,
# which is observable (`$(printf 'a\n\0')` is `a`, not `a\n`), and it applies to
# the `$(< file)` fast path as much as to a real subshell.

hx() { printf '%s' "$1" | od -An -tx1 | tr -s ' ' | sed 's/^ //;s/ $//' | tr '\n' ' '; echo; }

echo "=== the NUL is dropped, not a truncation point ==="
echo -n "mid   "; hx "$(printf '%b' 'a\0b')"
echo -n "lead  "; hx "$(printf '%b' '\0ab')"
echo -n "trail "; hx "$(printf '%b' 'ab\0')"
# Several NULs, and several commands, still warn exactly once per capture.
echo -n "many  "; hx "$(printf '%b' 'a\0b\0c')"
echo -n "cmds  "; hx "$(printf '%b' 'x\0y'; printf '%b' 'z\0')"
# A capture that is *only* NULs is the empty string with status 0.
echo -n "onlyn "; hx "$(printf '%b' '\0\0')"; echo "st=$?"
# No NUL, no warning.
echo -n "none  "; hx "$(printf ab)"

echo "=== backticks warn the same way ==="
echo -n "btick "; hx "`printf '%b' 'a\0b'`"

echo "=== NULs go before the trailing-newline strip ==="
# `a\n\0`: dropping the NUL first re-exposes the newline as trailing, so it is
# stripped too. Stripping newlines first would leave `a\n`.
echo -n "nlnul "; hx "$(printf '%b' 'a\n\0')"
echo -n "nulnl "; hx "$(printf '%b' 'a\0\n')"

echo "=== the \$(< file) fast path warns too ==="
printf 'a\n\0' > nul_a.bin
printf 'a\0b'  > nul_b.bin
echo -n "file1 "; hx "$(< nul_a.bin)"
echo -n "file2 "; hx "$(< nul_b.bin)"
rm -f nul_a.bin nul_b.bin

echo "=== the stripped result feeds the ordinary expansions ==="
# Word splitting sees the joined bytes, so `p\0q r` is two fields, not three.
a=( $(printf '%b' 'p\0q r') )
echo "arr=${a[0]}/${a[1]} n=${#a[@]}"
echo "arith=$(( $(printf '%b' '4\0 ') + 1 ))"

echo done
