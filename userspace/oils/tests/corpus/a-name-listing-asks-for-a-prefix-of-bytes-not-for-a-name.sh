# `${!PREFIX@}` / `${!PREFIX*}` is decided syntactically, and first: bash tries
# it (subst.c:9741) before the array keys and before the indirection proper, and
# all it asks of PREFIX is that its first character could start a name. The rest
# is taken raw — never expanded, never unquoted, not required to *be* a name — so
# `${!s[k]@}` lists names beginning with the four characters `s[k]` rather than
# indirecting through the element, which is what the same body means under any
# other operator.
sv=SVAL
svx=2
s_a=1
_u=1
n=(ax by cz)
declare -A s=([k]=sv)
p=sv
# A subshell, because several of the bodies below are refused *fatally* — the
# shell they are read in exits — and `alive` is the only way to see which.
b() { printf '%-20s ' "$1"; ( eval "printf '<%s>' $1"; printf ' st=%s' "$?" ) 2>&1; echo " alive=$?"; }
echo "--- a prefix that is not a name"
b '${!s[k]@}'
b '${!s[k]*}'
b '${!s[@]@}'
b '${!n[@]@}'
b '${!n[@]*}'
b '${!sv[0]@}'
b '${!s"v"@}'
b '${!s$x@}'
b '${!s*b*}'
b '${!s*b@}'
b '${!s(@}'
b '${!s)*}'
b '${!s.@}'
b '${!s]@}'
echo "--- and the ones that really are names still list"
b '${!s@}'
b '${!s*}'
b '${!sv@}'
b '${!_*}'
b '${!zzz*}'
echo "--- a subscript is stepped over whole, brackets and all"
b '${!s[a:b]@}'
b '${!s[a}b]@}'
b '${!s[@}'
b '${!s\@v@}'
b '${!s\*}'
echo "--- what the trailing character does not reach is not a listing"
b '${!s[k]@ }'
b '${!s[k]@Q}'
b '${!s[k]}'
b '${!p#b@}'
b '${!p:-x*}'
b '${!p/x/y*}'
b '${!1@}'
b '${!#@}'
b '${!@}'
b '${!*}'
echo "--- a trailing @ the scan reached is glued back on, name or not"
# subst.c:9585 puts the `@` back on the scanned name when it sits immediately
# before the brace, so `${!1@}` asks after a variable called `1@` and is refused
# outright — no indirection happens, and whether `$1` names something set makes
# no difference. `b` above evals its own `$1`, so these need a runner that sets
# the positional parameters itself, with the probe text kept out of them.
q() { printf '%-20s ' "$1"; c=$2; ( set -- sv nope; eval "printf '<%s>' $c"; printf ' st=%s' "$?" ) 2>&1; echo " alive=$?"; }
q '${!1@} ($1=sv)' '${!1@}'
q '${!1*}' '${!1*}'
q '${!2@} ($2=nope)' '${!2@}'
q '${!9@} (unset)' '${!9@}'
q '${!12@}' '${!12@}'
q '${!1[0]@}' '${!1[0]@}'
q '${!1s@}' '${!1s@}'
# …where a mark the scan *stopped at* is an operator after all, and reads the
# pointer like any other indirection.
q '${!1@Q}' '${!1@Q}'
q '${!1@Z}' '${!1@Z}'
q '${!1:-D}' '${!1:-D}'
# `#`, `?` and `-` are stop characters, so these never reach the glue at all.
q '${!#@}' '${!#@}'
q '${!?@}' '${!?@}'
q '${!-@}' '${!-@}'
echo TAIL
