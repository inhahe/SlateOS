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

# …except when the reference reaches `$!` itself, which is the only target that
# complains on its *own* behalf: it is read through the special-parameter path
# rather than looked up as a name, so the complaint names `$!` and the
# indirection is then left holding a failure, which it reports a second time as
# a bad substitution naming the whole word. Two lines, status 1, and the next
# line still runs — where an ordinary unset target gives one line and aborts.
# It happens inside the indirection, so no modifier gets a turn: not even the
# default-value family, which excuses a direct `$!`.
echo "=== but a reference that reaches it complains twice, and no modifier gets a turn"
q 'r=!; echo "[${!r}]"'
q 'r=!; echo "${!r}"'
q 'r=!; echo x${!r}y'
q 'r=!; v=${!r}; echo "[$v]"'
q 'r=!; echo "[${!r-D}][${!r:-D}][${!r+S}][${!r:+S}]"'
q 'r=!; echo "[${!r@Q}][${!r@A}][${!r@a}]"'
q 'r=!; echo "[${!r^^}][${!r,,}][${!r/a/b}][${!r%x}][${!r#x}][${!r:1:2}]"'
q 'r=!; echo "[${#!r}]"'
q 'r=!; echo "[${!r}]"; echo tail'
q 'r=!; ( echo "[${!r}]" ); echo tail'

echo "=== and once a job has been started the reference reads it like any other"
q 'true & wait; r=!; echo "[${!r:+y}][${!r-D}]" | sed "s/[0-9][0-9]*/N/"'
q 'true & wait; r=!; echo "[${!r}]" | sed "s/[0-9][0-9]*/N/"'

# `${#!r}` is left out here: it is a bad substitution at parse time whatever
# nounset says, so it would swallow the line it shares.
echo "=== with nounset off the reference is simply empty"
q 'set +u; r=!; echo "[${!r}][${!r-D}][${!r:-D}][${!r+S}]"'

echo "=== with nounset off it is simply empty"
( eval 'echo "[$!][${!}][${#!}]"'; echo "rc=$?" ) 2>&1 | n
echo "=== done"
