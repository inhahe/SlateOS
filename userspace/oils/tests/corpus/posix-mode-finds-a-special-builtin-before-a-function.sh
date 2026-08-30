# POSIX says the special builtins are found *before* shell functions, and bash
# obeys that in posix mode: a function named `unset` stops shadowing the builtin
# the moment the mode goes on, and starts again the moment it goes off.
#
# The only way to have such a function at all is to define it *before* entering
# the mode, because the mode itself refuses the name — so every case here defines
# first and switches after, and enters the mode through `$POSIXLY_CORRECT` rather
# than `set -o posix`, since a function named `set` would otherwise eat the
# switch itself.
#
# Three things the rule does *not* do, also checked below: it does not touch the
# non-special builtins (a function named `cd` still wins), it does not survive
# `enable -n` taking the builtin away, and it does not hide the function from
# anything that merely *describes* a name — `type`, `command -v`, `declare -f`
# and `unset -f` all still find it.

echo "=== outside posix mode the function wins"
( unset() { echo FN; }; v=1; unset v; echo "  v=${v-gone}" )
( shift() { echo FN; }; set -- a b; shift; echo "  \$#=$#" )
( eval() { echo FN; }; eval 'echo EV' )

echo "=== in posix mode the special builtin wins"
( unset() { echo FN; }; POSIXLY_CORRECT=1; v=1; unset v; echo "  v=${v-gone}" )
( shift() { echo FN; }; POSIXLY_CORRECT=1; set -- a b; shift; echo "  \$#=$#" )
( eval() { echo FN; }; POSIXLY_CORRECT=1; eval 'echo "  EV"' )
( set() { echo FN; }; POSIXLY_CORRECT=1; set -- a b c; echo "  \$#=$#" )
( : () { echo FN; }; POSIXLY_CORRECT=1; :; echo "  rc=$?" )
( break() { echo FN; }; POSIXLY_CORRECT=1; for i in 1 2 3; do break; done; echo "  i=$i" )
( continue() { echo FN; }; POSIXLY_CORRECT=1; for i in 1 2; do continue; echo NO; done; echo "  i=$i" )
( return() { echo FN; }; POSIXLY_CORRECT=1; f() { return 3; }; f; echo "  rc=$?" )
( export() { echo FN; }; POSIXLY_CORRECT=1; export ZQ=1; echo "  ZQ=$(env | grep -c '^ZQ=')" )
( readonly() { echo FN; }; POSIXLY_CORRECT=1; readonly ZR=1; echo "  ZR: $(declare -p ZR)" )
# (the `SIG` is sed'd off because posix mode drops it from `trap -p`'s output,
#  which is a rule of its own and not the one being checked here)
( trap() { echo FN; }; POSIXLY_CORRECT=1; trap ':' USR1; trap -p USR1 | sed 's/SIGUSR1/USR1/' )
( exit() { echo FN; }; POSIXLY_CORRECT=1; exit 7; echo "  UNREACHED" ); echo "  exit -> rc=$?"
echo "echo SOURCED" > z1.sh
( . () { echo FN; }; POSIXLY_CORRECT=1; . ./z1.sh )
( source() { echo FN; }; POSIXLY_CORRECT=1; source ./z1.sh )

echo "=== …and leaving the mode gives the function back"
( unset() { echo "  FN"; }
  POSIXLY_CORRECT=1; v=1; unset v; echo "  in:  v=${v-gone}"
  unset POSIXLY_CORRECT; w=1; unset w; echo "  out: w=${w-gone}" )

echo "=== the non-special builtins are untouched"
( cd() { echo "  FN-cd"; }; POSIXLY_CORRECT=1; cd /nowhere; echo "  rc=$?" )
( read() { echo "  FN-read"; }; POSIXLY_CORRECT=1; read x </dev/null; echo "  rc=$?" )
( local() { echo "  FN-local"; }; POSIXLY_CORRECT=1; local )
( declare() { echo "  FN-declare"; }; POSIXLY_CORRECT=1; declare )

echo "=== enable -n takes the builtin out of the running"
( unset() { echo "  FN"; }; POSIXLY_CORRECT=1; enable -n unset; v=1; unset v; echo "  v=${v-gone}" )

echo "=== the function is still there to be described"
( unset() { echo FN; }; POSIXLY_CORRECT=1
  echo "  type -t: $(type -t unset)"
  echo "  command -v: $(command -v unset)"
  echo "  declare -F: $(declare -F unset)"
  declare -f unset
  unset -f unset
  echo "  after unset -f: [$(declare -F unset)] rc=$?" )

echo "=== and the prefixes reach the builtin either way"
( unset() { echo FN; }; POSIXLY_CORRECT=1; v=1; command unset v; echo "  v=${v-gone}" )
( unset() { echo FN; }; POSIXLY_CORRECT=1; v=1; builtin unset v; echo "  v=${v-gone}" )
