# `${name=word}` has to put the default somewhere, so a parameter the shell
# answers for itself refuses it: a positional (`$1`, `$99`) or a special (`$!`,
# `$@`) reports `cannot assign in this way`, spelled with the `$` however the
# reference was written. Only the branch that would actually assign asks the
# question — with a value in hand the `=` never fires, so `set -- x; ${@=d}` is
# simply `x` and `${?=d}` is the status — and the refusal comes out *before* the
# default word is expanded, so its side effects never happen. That is the
# opposite of an indirection, which is judged by the name it resolved to and
# only after its default has run. The refusal discards the parse unit it is in,
# the way a bad subscript does, and the script carries on at the next one.
n() { sed 's/^.*: line [0-9]*: //'; }
q() { ( eval "$1"; echo "rc=$?" ) 2>&1 | n; }

echo "=== a parameter the shell answers for itself has nowhere to put a default"
q 'set -- ; echo "[${1=d}]"'
q 'echo "[${99=d}]"'
q 'set -- ; echo "[${1:=d}]"'
q 'echo "[${!=d}]"'
q 'echo "[${!:=d}]"'
q 'set -- ; echo "[${@:=d}]"'

echo "=== and the default word is not expanded first"
q 'echo "[${!=$(echo side >&2; echo d)}]"'
q 'set -- ; echo "[${1=$(echo side >&2; echo d)}]"'

echo "=== the parse unit it is in is discarded, and the next one still runs"
q 'set -- ; echo "[${1=d}]"; echo same-unit'
echo "[${1=d}]"
echo next-unit
if echo "[${1=d}]"; then echo taken; else echo not-taken; fi
echo after-if

echo "=== but a parameter with a value never reaches the assignment"
q 'set -- x y; echo "[${@=d}][${1=d}][${2:=d}]"'
q 'echo "[${?=d}][${#=d}][${-=d}]"'
q 'echo "[${_=d}]" > /dev/null; echo ok'
q 'set -u; set -- x; echo "[${1=d}]"'

echo "=== an ordinary name, an element and a key all assign as usual"
q 'echo "[${v=d}]"; declare -p v'
q 'x=; echo "[${x:=d}]"; declare -p x'
q 'echo "[${a[0]=d}]"; declare -p a'
q 'declare -A m; echo "[${m[k]=d}]"; declare -p m'
q 'declare -A m; echo "[${m[$(echo k)]=d}]"; declare -p m'

echo "=== an indirection is judged later, by the name it resolved to"
q 'ptr=1; set -- ; echo "[${!ptr=d}]"'
q 'ptr=1; set -- ; echo "[${!ptr=$(echo side >&2; echo d)}]"'
q 'ptr=v; echo "[${!ptr=d}]"; declare -p v'
echo "=== done"
