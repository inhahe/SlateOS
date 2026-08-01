# Without `-r`, a backslash is part of *reading* the record rather than
# something done to it afterwards, and that shows three ways. It hides the next
# byte from the delimiter, so `read -d :` on `a\:b:c` takes the escaped colon as
# data. A backslash before a newline is a line continuation: both bytes leave
# the input and the record carries on over the next line — and specifically a
# *newline*, whatever `-d` was given. And the backslash is no character, so an
# escaped pair counts as one toward `-n`/`-N`, while a backslash with nothing
# left to escape is simply dropped.
#
# The backslashes that survive stay in the record and come off at the splitting
# stage, which needs them: an escaped IFS character must not delimit a field.
# `mapfile` has no `-r` and never does any of this.
#
# The inputs are written with octal escapes (\134 backslash, \012 newline) so
# that no layer of quoting can be misread.
e() { sed 's/^.*: line [0-9]*: //'; }
p() { echo "--- $1"; ( eval "$1"; echo "rc=$?" ) 2>&1 | e; }

echo "=== an escaped character counts as one toward -n and -N"
p 'printf "x\134ny\012" | { read -n 1 v; echo "[$v]"; }'
p 'printf "x\134ny\012" | { read -n 2 v; echo "[$v]"; }'
p 'printf "x\134ny\012" | { read -n 3 v; echo "[$v]"; }'
p 'printf "x\134ny\012" | { read -rn 2 v; echo "[$v]"; }'
p 'printf "x\134ny\012" | { read -N 2 v; echo "[$v]"; read -N 2 w; echo "[$w]"; }'
p 'printf "x\134ny\012" | { read -N 4 v; echo "[$v]" | cat -A; }'
p 'printf "a\134bc\012" | { read -n 2 v; echo "[$v]"; read -n 5 w; echo "[$w]"; }'
p 'printf "a\134bc\012" | { read -N 1 v; echo "[$v]"; read -N 5 w; echo "[$w]"; }'
p 'printf "\134abc\012" | { read -n 1 v; echo "[$v]"; }'

echo "=== a backslash hides the delimiter"
p 'printf "a\134:b:c" | { read -d : v; echo "[$v]"; read -d : w; echo "[$w]"; }'
p 'printf "a\134:b:c" | { read -rd : v; echo "[$v]"; }'
p 'printf "a\134:b\012c" | { read -d : v; echo "[$v]"; }'

echo "=== a backslash-newline is a continuation, whatever the delimiter is"
p 'printf "x\134\012y\012" | { read v; echo "[$v]"; }'
p 'printf "x\134\012y\012" | { read -r v; echo "[$v]"; }'
p 'printf "x\134\012y\012z\012" | { read v; echo "[$v]"; read w; echo "[$w]"; }'
p 'printf "a\134\012b\134\012c\012" | { read v; echo "[$v]"; }'
p 'printf "\134\012x\012" | { read v; echo "[$v]"; }'
p 'printf "x\134\012y\012" | { IFS= read v; echo "[$v]"; }'
p 'printf "x\134\012y z\012" | { read a b; echo "[$a][$b]"; }'
p 'printf "x\134\012y\012" | { read -n 3 v; echo "[$v]"; }'
p 'printf "x\134\012y\012" | { read -N 3 v; echo "[$v]" | cat -A; }'
p 'printf "a\134\012b:c" | { read -d : v; echo "[$v]"; }'
p 'printf "a\134\012b\012" | { read -d "" v; echo "[$v]" | cat -A; }'
p 'printf "1\134\0122\012" | { select x in a b; do echo "[$x][$REPLY]"; break; done; } 2>/dev/null'

echo "=== an escaped backslash escapes nothing else"
p 'printf "a\134\134\012b\012" | { read v; echo "[$v]"; }'
p 'printf "a\134\134\134\012b\012" | { read v; echo "[$v]"; }'
p 'printf "a\134\134b\012" | { read -N 3 v; echo "[$v]"; }'
p 'printf "a\134\134b\012" | { read -n 2 v; echo "[$v]"; }'

echo "=== a backslash with nothing left to escape is dropped"
p 'printf "a\134" | { read v; echo "[$v]"; }'
p 'printf "a\134" | { read -r v; echo "[$v]"; }'
p 'printf "a\134" | { read -d : v; echo "[$v]"; }'
p 'printf "a\134" | { read -N 3 v; echo "[$v]"; echo "n=${#v}"; }'
p 'printf "a b\134" | { read x y; echo "[$x][$y]"; }'

echo "=== an escaped IFS character does not split"
p 'printf "a\134 b\012" | { read x y; echo "[$x][$y]"; }'
p 'printf "a\134 b\012" | { read -r x y; echo "[$x][$y]"; }'
p 'printf "  a\134 b  \012" | { read v; echo "[$v]"; }'
p 'printf "a\134\011b\012" | { read x y; echo "[$x][$y]"; }'
p 'printf "a\134 b\012" | { read -N 4 v; echo "[$v]" | cat -A; }'

echo "=== the status says whether the record finished the way it was asked to"
p 'printf "ab" | { read -N 2 v; r=$?; echo "r=$r [$v]"; }'
p 'printf "ab" | { read -N 3 v; r=$?; echo "r=$r [$v]"; }'
p 'printf "x\134ny\012" | { read -N 4 v; r=$?; echo "r=$r"; }'
p 'printf "x\134ny\012" | { read -N 5 v; r=$?; echo "r=$r"; }'
p 'printf "x\134\012y\012" | { read -N 4 v; r=$?; echo "r=$r"; }'
p 'printf "ab" | { read -n 2 v; r=$?; echo "r=$r [$v]"; }'
p 'printf "ab" | { read -n 3 v; r=$?; echo "r=$r [$v]"; }'
p 'printf "x\134ny" | { read -n 3 v; r=$?; echo "r=$r [$v]"; }'
p 'printf "a\134" | { read v; r=$?; echo "r=$r [$v]"; }'
p 'printf "a\134\012" | { read v; r=$?; echo "r=$r [$v]"; }'

echo "=== mapfile keeps its backslashes"
p 'printf "a\134\012b\012" | { mapfile -t v; printf "<%s>" "${v[@]}"; echo; }'
p 'printf "a\134\134\012b\012" | { mapfile -t v; printf "<%s>" "${v[@]}"; echo; }'
echo "=== done"
