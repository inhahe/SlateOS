# bash decodes backslash escapes in four places — `$'…'` (and `${v@E}`, which
# shares its rules), the `printf` FORMAT string, `printf %b`, and `echo -e` —
# and the four do *not* agree. `\c` alone means three different things, and
# `\101` is an octal escape for `printf %b` but four literal characters for
# `echo -e`. Every row of that disagreement is pinned below.
#
# Escapes are also *byte* escapes: `\400` is a NUL, not U+0100. Values above
# 0x7F are deliberately avoided here — osh stores words as UTF-8 `String`, so
# `$'\xff'` yields U+00FF where bash yields the single byte 0xff (a known
# representation gap, see known-issues.md). Likewise `\u`/`\U` above the ASCII
# range are locale-dependent and are not exercised.

hx() { printf '%s' "$1" | od -An -tx1 | tr -s ' ' | sed 's/^ //;s/ $//' | tr '\n' ' '; echo; }

echo "=== ANSI-C \\c is a control character ==="
echo -n "upper "; hx $'a\cAb'
echo -n "lower "; hx $'a\cab'
echo -n "run   "; hx $'a\cb\cc'
echo -n "digit "; hx $'\c0'
echo -n "brace "; hx $'\c{'
# `\c?` is DEL, not `'?' & 0x1f`.
echo -n "del   "; hx $'\c?'
# `\c\\` swallows *both* backslashes; `\c\n` swallows only the first, so the
# `n` survives as a literal.
echo -n "cbs   "; hx $'\c\\'
echo -n "cn    "; hx $'x\c\nz'
echo -n "ct    "; hx $'\c\t'
# A `\c` with nothing after it stays literal — which is only reachable because
# the closing quote is found by scanning, before any escape is decoded. Decode
# it inline and this word swallows its own terminator.
echo -n "dang  "; hx $'ab\c'
echo -n "dang2 "; hx $'\c'"'"'x'

echo "=== ANSI-C numeric escapes name bytes ==="
# `\400` is 0x100 & 0xff = NUL, and a NUL truncates the word.
echo -n "o400  "; hx $'\400'
echo -n "o401  "; hx $'a\401b'
echo -n "nul   "; hx $'a\0b'
# Three octal digits at most, and a leading `0` is one of them.
echo -n "o0101 "; hx $'\0101'
echo -n "hex   "; hx $'\x41\x42'
echo -n "uni   "; hx $'\u0041'

echo "=== ANSI-C keeps the backslash on anything it does not know ==="
echo -n "unk   "; hx $'\q\z'
echo -n "eight "; hx $'\8'
echo -n "badhex "; hx $'\xg'
# …but `\?`, `\'` and `\"` really are escapes here.
echo -n "quest "; hx $'\?'
echo -n "quote "; hx $'\'\"'

echo "=== \${v@E} follows the ANSI-C rules exactly ==="
v='a\cAb'; echo -n "cA    "; hx "${v@E}"
v='\c?';   echo -n "del   "; hx "${v@E}"
v='\?';    echo -n "quest "; hx "${v@E}"
v='\c\\';  echo -n "cbs   "; hx "${v@E}"
v='ab\c';  echo -n "dang  "; hx "${v@E}"
v='\400';  echo -n "o400  "; hx "${v@E}"
v='\0101'; echo -n "o0101 "; hx "${v@E}"

echo "=== the printf FORMAT string is ANSI-C minus \\c ==="
echo -n "cA    "; hx "$(printf 'a\cAb')"
echo -n "c     "; hx "$(printf '\c')"
echo -n "quest "; hx "$(printf 'a\?b')"
echo -n "quote "; hx "$(printf '\"|\x27')"
echo -n "o0101 "; hx "$(printf '\0101')"
echo -n "o101  "; hx "$(printf '\101')"

echo "=== printf %b and echo -e stop at \\c ==="
echo -n "pb    "; hx "$(printf '%b' 'a\cb')"
echo -n "ee    "; hx "$(echo -e 'a\cb')"
echo -n "pbc   "; hx "$(printf '%b' '\c')"
echo -n "eec   "; hx "$(echo -e '\c')"
# …and the `\c` kills the rest of the *format*, not just its own conversion.
printf 'a%bz' 'p\cq'; echo " fmt st=$?"
echo -n "pass  "; hx "$(printf '%b\n%b' 'x\cy' 'tail')"

echo "=== the echo family drops the ANSI-C-only escapes ==="
echo -n "pb-q  "; hx "$(printf '%b' 'a\?b')"
echo -n "ee-q  "; hx "$(echo -e 'a\?b')"
echo -n "pb-dq "; hx "$(printf '%b' '\"')"
echo -n "ee-dq "; hx "$(echo -e '\"')"
# The named/hex/unicode escapes are shared, though.
echo -n "pb-nm "; hx "$(printf '%b' '\e|\a|\v|\b|\f')"
echo -n "ee-nm "; hx "$(echo -e '\e|\a|\v|\b|\f')"
echo -n "pb-x  "; hx "$(printf '%b' '\x41')"
echo -n "ee-x  "; hx "$(echo -e '\x41')"
echo -n "pb-u  "; hx "$(printf '%b' '\u0041')"
echo -n "ee-U  "; hx "$(echo -e '\U00000041')"
echo -n "pb-8  "; hx "$(printf '%b' '\8')"
echo -n "ee-8  "; hx "$(echo -e '\8')"
echo -n "pb-bs "; hx "$(printf '%b' 'a\')"
echo -n "ee-bs "; hx "$(echo -e 'a\')"

echo "=== octal: printf %b takes both spellings, echo -e only \\0nnn ==="
echo -n "pb-0  "; hx "$(printf '%b' '\0101')"
echo -n "pb-n  "; hx "$(printf '%b' '\101')"
echo -n "ee-0  "; hx "$(echo -e '\0101')"
echo -n "ee-n  "; hx "$(echo -e '\101')"
# `\0` with no digits after it is a NUL, in both echo-family decoders — and
# unlike `$'…'`, it is emitted rather than truncating. A `$( )` capture drops
# NUL bytes, so these go straight down a pipe.
echo -n "ee-09 "; echo -ne '\09' | od -An -tx1 | tr -d '\n '; echo
echo -n "pb-0b "; printf '%b' 'a\0b' | od -An -tx1 | tr -d '\n '; echo
echo -n "ee-0b "; echo -ne 'a\0b' | od -An -tx1 | tr -d '\n '; echo

echo "=== only printf complains about a malformed \\x or \\u ==="
printf '\xg' 2>&1; echo " fmt-x st=$?"
printf '%b' '\xg' 2>&1; echo " pb-x st=$?"
printf '\uzz' 2>&1; echo " fmt-u st=$?"
printf '\Uzz' 2>&1; echo " fmt-U st=$?"
# …while `$'…'` and `echo -e` keep the literal silently.
echo -n "sq-x  "; hx $'\xg'
echo -n "ee-x  "; hx "$(echo -e '\xg' 2>&1)"

echo done
