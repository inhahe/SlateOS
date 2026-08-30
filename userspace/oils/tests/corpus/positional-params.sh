# Positional parameters: "$@" vs "$*", IFS's effect on each, `set --`, `shift`,
# and how an empty "$@" disappears rather than becoming one empty word.
set -- one 'two three' four
echo "count=$#"
echo "at=$@"
echo "star=$*"

# The quoting difference is only visible when a field contains a space.
printf '[%s]' "$@"; echo
printf '[%s]' "$*"; echo
printf '[%s]' $@; echo

# "$*" joins with the first character of IFS; "$@" ignores IFS entirely.
IFS=-
echo "ifs-star=$*"
echo "ifs-at=$@"
printf '[%s]' "$@"; echo
IFS=' '

# An empty "$@" expands to zero words — `f "$@"` with no arguments passes none,
# where `f "$*"` passes one empty word.
set --
echo "empty-count=$#"
printf 'at:[%s]\n' "$@"
printf 'star:[%s]\n' "$*"
c() { echo "argc=$#"; }
c "$@"
c "$*"

# shift drops the leading N, and shifting past the end fails without changing
# anything.
set -- a b c d
shift
echo "after-shift=$* n=$#"
shift 2
echo "after-shift2=$* n=$#"
shift 5
echo "overshift-status=$? left=$* n=$#"

# ${1}/${10} and defaults over positional parameters.
set -- p1 p2 p3 p4 p5 p6 p7 p8 p9 p10
echo "tenth=${10} first=${1}"
set -- x
echo "unset-default=${2-fallback} set-default=${1-fallback}"

# Slicing the positional list; ${@:0} is the shell/function name in bash.
set -- a b c d e
echo "slice=${@:2:3}"
echo "from=${@:4}"
echo "star-slice=${*:2:2}"

# A function's positional parameters shadow the caller's and are restored after.
g() { echo "in-g=$* n=$#"; set -- overwritten; echo "in-g2=$*"; }
g inner1 inner2
echo "after-g=$* n=$#"

# $0 in a script is the script name; the harness runs `case.sh` in the cwd.
echo "zero=$0"
