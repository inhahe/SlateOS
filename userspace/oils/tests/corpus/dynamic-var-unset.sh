# `unset` takes a dynamic variable's *value function* away with it.
#
# Ten of the names bash keeps — `SECONDS`, `RANDOM`, `SRANDOM`, `LINENO`,
# `BASHPID`, `BASH_SUBSHELL`, `EPOCHSECONDS`, `EPOCHREALTIME`, `HISTCMD` and
# `BASH_ARGV0` — hold no stored value at all: each read calls a function that
# computes one. `unset` removes the whole variable, that function included, and
# bash never puts it back. What is left is an unset, entirely **ordinary** name:
#
#   * it expands to nothing, so `${SECONDS-UNSET}` takes the default;
#   * assigning to it stores a string, rather than running the hook the name
#     used to have — `RANDOM=5` no longer reseeds the generator, `SECONDS=5` no
#     longer rebases the counter, and `SRANDOM=5` is no longer swallowed;
#   * the attributes went with the binding too, so `declare -p` says `--`, not
#     the `-i` the name used to carry;
#   * it stops appearing in `declare -p`, in the flag-filtered listings and in
#     `${!prefix@}`.
#
# It is the *variable* `unset` names, not a function of the same name — a
# function called `RANDOM` survives an `unset RANDOM` that removes the variable.
# The one member of the family this cannot happen to is `PPID`, which is
# readonly: every write path refuses it and `unset` fails.
#
# Values that differ between two shells on the same host (pids, clocks, the
# generator's own output) are never printed here — only set-ness, statuses,
# stability and the values a script assigned itself.

echo "=== unset leaves the name unset, and ordinary"
for n in SECONDS RANDOM SRANDOM LINENO BASHPID BASH_SUBSHELL \
         EPOCHSECONDS EPOCHREALTIME HISTCMD BASH_ARGV0; do
  ( unset $n; printf '%s: rc=%s [%s]' "$n" "$?" "${!n-UNSET}"
    eval "$n=zz"; echo " reassign=[${!n-UNSET}]" )
done

echo "=== the assignment hook goes with the value function"
# `RANDOM=n` reseeded the generator, so `$RANDOM` differed on every read; after
# the unset it is a plain string that reads back the same every time.
( unset RANDOM; RANDOM=5; a=$RANDOM; b=$RANDOM
  [ "$a" = "$b" ] && echo "stable [$a]" || echo varying )
# `SECONDS=n` rebased a counter that then climbed; now it just sits there.
( unset SECONDS; SECONDS=5; echo "[$SECONDS]" )
# Assignments to `SRANDOM` used to have no effect at all.
( unset SRANDOM; SRANDOM=5; echo "[$SRANDOM]" )
# And with nothing assigned there is nothing to expand.
( unset LINENO; echo "[$LINENO]" )

echo "=== the attributes went with it too"
( unset SECONDS; SECONDS=5; declare -p SECONDS )
( unset RANDOM; RANDOM=5; declare -p RANDOM )
# So a later `-i` is applied to an ordinary variable, arithmetic and all.
( unset SECONDS; declare -i SECONDS=2+3; declare -p SECONDS )
# …and `export` binds the value it now really has.
( unset SECONDS; SECONDS=5; export SECONDS; declare -p SECONDS )
# `readonly` makes it as immovable as any other name.
( unset SECONDS; readonly SECONDS=4; SECONDS=5; echo "unreachable" ) 2>&1
# A plain append is a string append, not the arithmetic one `-i` would force.
( unset SECONDS; SECONDS=ab; SECONDS+=cd; echo "[$SECONDS]" )

echo "=== and it stops being reported"
( unset SECONDS; declare -p SECONDS; echo "rc=$?" ) 2>&1
( unset EPOCHSECONDS; echo "[${!EPOCH@}]" )
( echo "[${!EPOCH@}]" )
# The flag-filtered listings drop it as well; `SRANDOM` keeps its own `-i`.
( unset RANDOM
  case "$(declare -i)" in *'declare -i RANDOM'*) echo present;; *) echo gone;; esac
  case "$(declare -i)" in *'declare -i SRANDOM'*) echo present;; *) echo gone;; esac )

echo "=== unset names the variable, not a function of the same name"
( RANDOM() { echo hi; }; unset RANDOM; RANDOM; echo "[${RANDOM-UNSET}]" )
# `unset -f` names the function, and there is none, so the variable is untouched.
( unset -f SECONDS; echo "rc=$? [${SECONDS+dynamic}]" )
( unset -v RANDOM; echo "rc=$? [${RANDOM-UNSET}]" )

echo "=== after the unset, every write path is the ordinary one"
( unset SECONDS; (( SECONDS = 3 )); echo "rc=$? [$SECONDS]"; (( SECONDS++ )); echo "[$SECONDS]" )
( unset RANDOM; (( RANDOM = 3 )); echo "[$RANDOM]" )
( unset RANDOM; printf -v RANDOM q; echo "rc=$? [$RANDOM]" )
( unset RANDOM; read RANDOM <<<hi; echo "rc=$? [$RANDOM]" )
( unset RANDOM; for RANDOM in a b; do :; done; echo "rc=$? [$RANDOM]" )
( unset RANDOM; declare -n r=RANDOM; r=7; echo "[$RANDOM]" )
# A `local` of the name is ordinary too, and the global stays unset after.
( unset SECONDS; f() { local SECONDS=9; echo "[$SECONDS]"; }; f; echo "[${SECONDS-UNSET}]" )
# An unset from inside a function is the global's, as it is for any variable.
( f() { unset SECONDS; }; f; echo "[${SECONDS-UNSET}]" )

echo "=== unsetting one twice, and unsetting the ordinary name it left"
( unset SECONDS; unset SECONDS; echo "rc=$?" )
( unset SECONDS; SECONDS=9; unset SECONDS; echo "rc=$? [${SECONDS-UNSET}]" )

echo "=== a subshell's unset does not reach the parent"
( unset RANDOM ); echo "[${RANDOM:+dynamic}]"
( unset SECONDS ); echo "[${SECONDS+dynamic}]"

echo "=== PPID is readonly, so none of it can happen to it"
PPID=5; echo "unreachable"
(( PPID = 5 )); echo "rc=$?"
declare PPID=5; echo "rc=$?"
# In a subshell, because a refused `export` still applies its attribute and
# would change the `declare -p` below — see export-readonly-attr.sh.
( export PPID=5; echo "rc=$?" )
printf -v PPID q; echo "rc=$?"
read PPID <<<hi; echo "rc=$?"
for PPID in a; do :; done; echo "rc=$?"
unset PPID; echo "rc=$?"
unset -v PPID; echo "rc=$?"
# …and it is still there, still readable, still an integer.
echo "[${PPID:+set}] [$(declare -p PPID | sed 's/=.*//')]"
