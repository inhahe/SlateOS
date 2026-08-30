# bash signs an arithmetic complaint with the builtin that asked for it, but the
# signature is one global — `this_command_name` — and `execute_simple_command`
# *blanks* it on the way out rather than putting the old one back:
#
#   return_result:
#     …
#     this_command_name = (char *)NULL;	/* points to freed memory now */
#                                     	   (execute_cmd.c:4828)
#
# So running any simple command takes the signature away, and a builtin that
# runs shell code and only *then* complains complains unsigned. `mapfile -C` is
# the reachable case: the callback is a whole command, and it runs between the
# line being read and the line being stored.
#
# Two things must *not* be unsigned by this, and they are here to hold the rule
# down to the shape bash actually has. bash forks for a pipeline element and for
# a command substitution, so a command run inside either one blanks the *child's*
# copy of the global and the parent's signature survives. That is the whole
# reason `let "x=$(echo 1)+"` is still `let`'s complaint.
#
# Every failing store below is a malformed `-i` value, which unwinds the parse
# unit it is in. They are therefore all written as bare top-level commands: an
# abort out of a *compound* command loses bash a line off its counter for the
# rest of the script (known-issues
# TD-OILS-A-DISCARD-OUT-OF-A-COMPOUND-COMMAND-LOSES-BASH-A-LINE), and a `for`
# loop or a `{ … }` here would be measuring that instead of the signature.

echo "=== the store before any callback is the builtin's; the one after is not"
declare -ai q
mapfile -t -C 'echo cb' -c 1 q <<< $'1+1\nq+'
echo "rc=$?"
declare -p q

echo "=== any simple command does it — a builtin…"
declare -ai r1
mapfile -t -C 'true' -c 1 r1 <<< 'q+'
echo "rc=$?"

echo "=== …a function…"
f() { :; }
declare -ai r2
mapfile -t -C 'f' -c 1 r2 <<< 'q+'
echo "rc=$?"

echo "=== …and even a command word that is not found at all"
declare -ai r3
mapfile -t -C 'no-such-command-at-all 2>/dev/null' -c 1 r3 <<< 'q+'
echo "rc=$?"

echo "=== a pipeline does not: bash forks for each element"
declare -ai p
mapfile -t -C 'true | true' -c 1 p <<< 'q+'
echo "rc=$?"

echo "=== nor does a substitution, which is what keeps the ordinary cases signed"
let "x=$(echo 1)+"
echo "rc=$?"
(( $(echo 1) + ))
echo "rc=$?"
declare -i n
declare n=$(echo 1)+
echo "rc=$?"

echo "=== the signature is gone for good, not restored after the mapfile"
declare -ai k
mapfile -t -C 'true' -c 1 k <<< '1'
declare -i m
m=2+
echo "rc=$?"

echo "still here"
