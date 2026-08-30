# `getopts` writes `OPTIND` and `OPTARG` with the shell's ordinary binding, the
# same one an assignment uses — so whatever the name has been made to mean
# applies. A nameref sends the value to its target. An integer, uppercase or
# lowercase attribute transforms it. An array takes it at index zero. A readonly
# refuses it and says so — but that refusal is the *variable's* and not the
# call's: the scan is over by the time these are written, so the option still
# reaches the name the caller asked for, and a refused `OPTIND` leaves the
# shell's own cursor where the scan put it. Only a readonly on the *name* is the
# call's own failure.
#
# The one place it does *not* go through the name is where it takes `OPTARG`
# away — and it only ever takes it away in the loud mode, to say that there is
# nothing to report about an option it did not like. That unbinds the variable
# *called* `OPTARG`, without following a nameref to its target, and so destroys
# the nameref itself. A matched option is a different thing even when it has no
# argument to report: that is still a *write*, of nothing, which goes through
# the nameref and leaves both it and its target standing, the target now
# declared but with no value.
#
# Which of the two happens is chosen by what the scan found, not by what the
# caller asked for. A matched option writes — its argument, or nothing. An
# unknown option, or a missing argument, unbinds in the loud mode and writes
# the offending letter in the silent one.

echo "=== a nameref is written through when OPTARG is set"
( OPTIND=1; set -- -a 1; declare -n OPTARG=q
  getopts "a:" o; echo "  rc=$? o=$o q=[${q-unset}] optarg=[${OPTARG-unset}]" )

echo "=== a matched option with nothing to report still writes through it"
( OPTIND=1; set -- -a; declare -n OPTARG=q
  getopts "a" o; echo "  rc=$? o=$o q=[${q-unset}] optarg=[${OPTARG-unset}]"
  declare -p OPTARG q 2>&1 | sed 's/^[^ ]*: /sh: /' )

echo "=== an unknown option in the loud mode unbinds the name itself"
( OPTIND=1; set -- -x; declare -n OPTARG=q
  getopts "ab" o 2>/dev/null; echo "  rc=$? o=$o q=[${q-unset}] optarg=[${OPTARG-unset}]"
  declare -p OPTARG q 2>&1 | sed 's/^[^ ]*: /sh: /' )

echo "=== and in the silent mode it is a write, so the nameref survives"
( OPTIND=1; set -- -x; declare -n OPTARG=q
  getopts ":ab" o; echo "  rc=$? o=$o q=[${q-unset}] optarg=[${OPTARG-unset}]"
  declare -p OPTARG q 2>&1 | sed 's/^[^ ]*: /sh: /' )

echo "=== a missing argument reads the same way round"
( OPTIND=1; set -- -a; declare -n OPTARG=q
  getopts "a:" o 2>/dev/null; echo "  rc=$? o=$o q=[${q-unset}] optarg=[${OPTARG-unset}]"
  declare -p OPTARG q 2>&1 | sed 's/^[^ ]*: /sh: /' )
( OPTIND=1; set -- -a; declare -n OPTARG=q
  getopts ":a:" o; echo "  rc=$? o=$o q=[${q-unset}] optarg=[${OPTARG-unset}]"
  declare -p OPTARG q 2>&1 | sed 's/^[^ ]*: /sh: /' )

echo "=== the value attributes apply"
( OPTIND=1; set -- -a abc; declare -u OPTARG; getopts "a:" o; echo "  -u [$OPTARG]" )
( OPTIND=1; set -- -a ABC; declare -l OPTARG; getopts "a:" o; echo "  -l [$OPTARG]" )
( OPTIND=1; set -- -a 2+2; declare -i OPTARG; getopts "a:" o; echo "  -i [$OPTARG]" )

echo "=== an array takes it at index zero"
( OPTIND=1; set -- -a 1; declare -a OPTARG=(p q); getopts "a:" o; declare -p OPTARG )
( OPTIND=1; set -- -a -b; declare -a OPTIND=(p q); getopts "ab" o; declare -p OPTIND )

echo "=== a readonly OPTARG or OPTIND complains but does not stop the call"
( OPTIND=1; set -- -a 1; readonly OPTARG=z
  getopts "a:" o 2>&1
  echo "  rc=$? optarg=[$OPTARG] o=[$o] ind=$OPTIND" )
( set -- -a -b; readonly OPTIND=1
  getopts "ab" o 2>&1
  echo "  rc=$? optind=[$OPTIND] o=[$o]"
  getopts "ab" o 2>&1
  echo "  rc=$? optind=[$OPTIND] o=[$o]" )

echo "=== OPTIND is written through a nameref too"
( OPTIND=1; set -- -a -b; declare -n OPTIND=q
  getopts "ab" o; echo "  rc=$? o=$o q=[${q-unset}] ind=[${OPTIND-unset}]" )

echo "=== and the name it stores the option in is bound the ordinary way"
( OPTIND=1; set -- -a; declare -n o=t; getopts "ab" o; echo "  rc=$? t=[$t] o=[$o]" )
( OPTIND=1; set -- -a; declare -i o; getopts "ab" o; echo "  rc=$? o=[$o]" )
( OPTIND=1; set -- -a; declare -a o=(1 2); getopts "ab" o; echo "  rc=$?"; declare -p o )
( OPTIND=1; set -- -a; readonly o=k
  getopts "ab" o 2>&1
  echo "  rc=$? o=[$o] ind=$OPTIND" )
