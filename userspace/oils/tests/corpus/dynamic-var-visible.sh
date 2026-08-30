# Looking a dynamic variable up is what fills its slot in.
#
# `SECONDS`, `RANDOM`, `LINENO` and the rest of the family have a real slot in
# the variable table, but it starts out half-empty: no value, and — for some of
# them — not even the attributes they are famous for. The value function fills
# both in, and it runs on every *lookup*, so a name nothing has asked for lists
# quite differently from the same name one line later:
#
#                   declare -p            declare -i              set
#   pristine        declare -- SECONDS    —                       —
#   after a read    declare -i SECONDS=N  declare -i SECONDS=N    SECONDS=N
#
# `declare -p SECONDS` reported the full form all along, because naming it is
# itself a lookup. So is `${SECONDS-x}`, `(( SECONDS ))`, `[ -v SECONDS ]` and
# every declaration builtin — they all find the variable before they do
# anything with it. Enumerating *names* (`${!SEC@}`) does not, since it never
# asks for a value.
#
# Each line below runs in its own command substitution so nothing it looks at
# leaks into the next. Values that differ between two shells are masked; the
# mask spells no double quote and no backslash of its own, because osh cannot
# yet hand either to an external command on the Windows host it is developed on
# (see known-issues TD-OILS-WIN-ARG-QUOTING).
m() { sed -E -e 's/=.[0-9.]+.$/=Q/' -e 's/=[0-9.]+$/=Q/'; }
g() { grep -E "^declare -[^ ]+ $1(=|$)"; }
# The two survey loops are about presence and attributes, so the values — which
# no two shells agree on — are cut off entirely rather than masked.
n() { sed -E 's/=.*//'; }

echo "=== pristine: a slot with no value, and not always the attributes"
for v in SECONDS RANDOM BASHPID SRANDOM HISTCMD LINENO EPOCHSECONDS \
         EPOCHREALTIME BASH_SUBSHELL BASH_ARGV0; do
  printf '%-14s p=[%s] i=[%s] set=[%s]\n' "$v" \
    "$( declare -p | g $v )" "$( declare -i | g $v )" "$( set | grep "^$v=" | n )"
done

echo "=== one read fills it in, for every listing at once"
for v in SECONDS RANDOM BASHPID SRANDOM HISTCMD LINENO EPOCHSECONDS \
         EPOCHREALTIME BASH_SUBSHELL BASH_ARGV0; do
  printf '%-14s p=[%s] i=[%s] set=[%s]\n' "$v" \
    "$( eval ": \$$v"; declare -p | g $v | n )" \
    "$( eval ": \$$v"; declare -i | g $v | n )" \
    "$( eval ": \$$v"; set | grep "^$v=" | n )"
done

echo "=== every way of looking one up does it"
( : $SECONDS;        declare -i | g SECONDS ) | m
( : ${SECONDS-x};    declare -i | g SECONDS ) | m
( : ${SECONDS:-x};   declare -i | g SECONDS ) | m
( : ${#SECONDS};     declare -i | g SECONDS ) | m
( (( SECONDS ));     declare -i | g SECONDS ) | m
( [ -v SECONDS ];    declare -i | g SECONDS ) | m
( declare -p SECONDS >/dev/null; declare -i | g SECONDS ) | m
( echo "$SECONDS" >/dev/null;    set | grep '^SECONDS=' ) | m
# …and a declaration builtin, which finds the variable before it marks it.
( declare SECONDS;   declare -i | g SECONDS ) | m
( export SECONDS;    declare -i | g SECONDS ) | m
( readonly SECONDS;  declare -i | g SECONDS ) | m

echo "=== but merely naming one does not"
( : ${!SEC@};        declare -i | g SECONDS ) | m
( : ${!SEC@};        declare -p | g SECONDS ) | m
( echo "${!SEC@}" ) | m

echo "=== a subshell's lookup does not reach the parent"
( : $SECONDS ); declare -i | grep -c ' SECONDS'
x=$( : $SECONDS ); declare -i | grep -c ' SECONDS'
# …and a function's does, since there is only one binding.
f() { : $SECONDS; }; f; declare -i | g SECONDS | m

echo "=== unset empties the slot again"
( : $SECONDS; unset SECONDS; declare -p | grep -c ' SECONDS' )
( : $SECONDS; unset SECONDS; set | grep -c '^SECONDS=' )
# …and what a later assignment makes is an ordinary variable, listed as one.
( : $SECONDS; unset SECONDS; SECONDS=5; declare -p | g SECONDS ) | m

echo "=== the RANDOM family, whose slot starts with its attributes but no value"
( declare -p | g RANDOM ) | m
( : $RANDOM; declare -p | g RANDOM ) | m
( declare RANDOM; declare -p | g RANDOM ) | m
# `PPID` starts with both, so it is in every listing from the first line.
( declare -p | g PPID ) | m
( set | grep '^PPID=' ) | m
( declare -i | g PPID ) | m
