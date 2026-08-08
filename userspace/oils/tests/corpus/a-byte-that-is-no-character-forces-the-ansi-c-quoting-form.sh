# bash picks between showing a value as itself and showing it as `$'…'` by
# asking `ansic_shouldquote` (lib/sh/strtrans.c), which walks the value and
# answers yes as soon as a character is not `isprint`. Any byte above 127 goes
# to `ansic_wshouldquote`, which decodes with `mbstowcs` and quotes when that
# *fails* — so in a UTF-8 locale a byte that is no character forces the form,
# while valid UTF-8 passes through untouched.
#
# The escape is then written per *byte*, not per code point: `ansic_quote`
# decodes to decide but writes the original bytes back, so `U+009F` — a control
# that is `\302\237` in UTF-8 — comes out as both escapes and not as `\237`.
# Only for ASCII are the two the same.
#
# All four surfaces that ask the question are here, because they must not drift:
# `printf %q`, `${v@Q}`, a `declare -p` value, and a `declare -p` associative
# *key*.
#
# Not covered here, deliberately: the rest of a C library's `isprint` table.
# Format characters (`U+00AD`, `U+200B`, `U+FEFF`), private use and unassigned
# code points are quoted by the newlib bash this corpus is measured against and
# are *not* quoted by osh — a decided deviation, since encoding one host's
# character database would bake it into an OS that will never run that host.
# See `design-decisions.md` §101 and `known-issues.md`,
# TD-OILS-PRINTF-Q-HIGH-BYTES. `U+2028`/`U+2029` are the other way round —
# newlib prints them and glibc does not — which is the same point from the other
# side.
#
# Verified against bash 5.2.37 in a UTF-8 locale.

p() { printf '%-14s ' "$1"; printf '%q\n' "$(printf "$2")"; }

echo "=== a byte that decodes to nothing forces \$'…' ==="
p lone-80  'a\x80b'
p lone-ff  'a\xffb'
p only-ff  '\xff'
p two-bad  '\x80\x81'
p truncated 'a\xc3'
p stray-cont 'a\xa9b'

echo "=== …and the escape is per byte, so a non-ASCII control shows both ==="
p c1-80    'a\u0080b'
p c1-9f    'a\u009fb'
p nul-adj  'a\u0001b'
p del      'a\x7fb'

echo "=== valid UTF-8 is not quoted ==="
p e-acute  'a\u00e9b'
p snowman  '\u2603'
p nbsp     'a\u00a0b'
p ideo-sp  'a\u3000b'
p combining 'a\u0301b'

echo "=== the other three surfaces ask the same question ==="
v=$(printf 'a\xffb')
printf '%s\n' "${v@Q}"
declare -p v
u=$(printf 'a\u009fb')
printf '%s\n' "${u@Q}"
declare -p u
declare -A m
m[$(printf 'k\xffz')]=1
m[$(printf 'plain')]=2
declare -p m
n=$(printf 'ok')
printf '%s\n' "${n@Q}"
declare -p n

echo "=== and the quoted form re-reads as the same bytes ==="
r=$(eval "printf '%s' ${v@Q}")
[ "$r" = "$v" ] && echo "round-trip ok" || echo "round-trip BROKEN"
s=$(eval "printf '%s' $(printf '%q' "$u")")
[ "$s" = "$u" ] && echo "round-trip ok" || echo "round-trip BROKEN"

echo done
