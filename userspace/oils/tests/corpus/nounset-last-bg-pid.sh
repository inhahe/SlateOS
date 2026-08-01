# `$!` is the one special parameter with a genuinely empty state: it holds the
# pid of the last background job, and a shell that has not started one has no
# answer to give. So unlike `$@`, `$?`, `$#` and the rest — which are always set
# however empty they look — `set -u` faults on it, naming it as the source
# spelled it (`$!` unbraced, `!` inside braces). A modifier does not excuse it
# and neither does the context it is read in; the default-value family does, and
# so does `${#!}`, which asks about the parameter rather than reading it. Once a
# job has been started there is nothing to complain about.
n() { sed 's/^.*: line [0-9]*: //'; }
q() { ( set -u; eval "$1"; echo "rc=$?" ) 2>&1 | n; }

echo "=== with no background job it is an unset parameter"
q 'echo "[$!]"'
q 'echo "[${!}]"'
q 'echo "[$!]"; echo tail'

echo "=== the context it is read in makes no difference"
q 'x=$!; echo "[$x]"'
q 'printf "%s\n" "$!"'
q 'case $! in *) echo c;; esac'
q 'f() { echo "[$!]"; }; f'
q 'for i in "$!"; do echo "[$i]"; done'
q 'a=("$!"); echo "[${a[0]}]"'
q '[[ $! == "" ]]; echo "c=$?"'
q '( echo "[$!]" )'
q 'echo "[$!]" | cat'
q 'echo "[$(echo "$!")]"'

echo "=== a modifier does not excuse it"
q 'echo "[${!:1}]"'
q 'echo "[${!:0:2}]"'
q 'echo "[${!/x/y}]"'
q 'echo "[${!,,}]"'
q 'echo "[${!%x}]"'
q 'echo "[${!#x}]"'
q 'echo "[${!@Q}]"'

echo "=== but the default-value family does, and so does the length"
q 'echo "[${!:-d}][${!-U}][${!+s}][${!:+y}]"'
q 'echo "[${#!}]"'
q 'echo "[${!:?}]"'

echo "=== the other special parameters are set however empty they look"
q 'set -- ; echo "[$@][$*][$#][$?][$-]"'
q 'set -- ; echo "[${@}][${*}][${#}][${?}]"'
q 'echo "[${#@}][${#*}]"'

echo "=== and once a job has been started there is nothing to complain about"
q 'true & wait; echo "[${!:+y}][${!:-d}]" | sed "s/[0-9][0-9]*/N/"'
q 'true & wait; case $! in [0-9]*) echo digits;; *) echo other;; esac'
q 'true & wait; ( echo "[${!:+y}]" )'
q 'true & wait; f() { echo "[${!:+y}]"; }; f'

echo "=== an indirection through it is named as the reference"
q 'set -- ; r=1; echo "[${!r}]"'
q 'true & wait; r=!; case ${!r} in [0-9]*) echo digits;; *) echo other;; esac'

echo "=== with nounset off it is simply empty"
( eval 'echo "[$!][${!}][${#!}]"'; echo "rc=$?" ) 2>&1 | n
echo "=== done"
