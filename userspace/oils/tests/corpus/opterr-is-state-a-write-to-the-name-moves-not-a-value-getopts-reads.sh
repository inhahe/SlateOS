# `OPTERR=0` turns off the complaint `getopts` makes about an argument list it
# does not like. But `getopts` never reads `OPTERR`. What it reads is a flag the
# shell keeps beside it — bash's `sh_opterr` — and the only thing that moves
# that flag is a *write to the name* `OPTERR`, which runs a hook on the way out
# of the assignment. So the flag is a record of the last assignment, not of what
# the variable currently expands to.
#
# The two come apart wherever a value changes without an assignment to that
# name. A nameref pointed *at* `OPTERR` writes through it and does move the flag,
# because the hook is run with the name the write resolved to. A nameref *called*
# `OPTERR` pointed elsewhere does not: writing it is a write to the other name,
# and the complaint stays on even though `$OPTERR` now expands to `0`.
#
# The value itself is read like C's `atoi`, so anything that is not a number is
# zero and mutes the complaint, while an empty or absent value is the 1 that is
# the default.
#
# The hook is on the assignment machinery rather than on the binding, which
# means every way a binding is made or destroyed runs it: a declaration, an
# `unset`, an assignment prefix on a command — both when it is pushed and when
# it is popped again — and a `local` going out of scope at the end of a
# function.

echo "=== the value is read like atoi"
for v in 0 00 0x -0 ' 0' zz 0.9 +0 1 2 -1 ''; do
  echo "  --- [$v]"
  ( OPTERR=$v; set -- -x; getopts "ab" o ) 2>&1
done

echo "=== absent is the default, which is on"
( unset OPTERR; set -- -x; getopts "ab" o ) 2>&1

echo "=== the last write wins"
( OPTERR=1; OPTERR=0; set -- -x; getopts "ab" o ) 2>&1
( OPTERR=0; OPTERR=1; set -- -x; getopts "ab" o ) 2>&1

echo "=== unset is a write too, and puts the default back"
( OPTERR=0; unset OPTERR; set -- -x; getopts "ab" o ) 2>&1

echo "=== a nameref pointed at OPTERR writes it, and moves the flag"
( declare -n r=OPTERR; r=0; set -- -x; getopts "ab" o; echo "  OPTERR=[$OPTERR]" ) 2>&1

echo "=== a nameref *called* OPTERR does not, and the flag stays on"
( declare -n OPTERR=q; q=0; set -- -x; getopts "ab" o; echo "  OPTERR=[$OPTERR]" ) 2>&1

echo "=== an assignment prefix is a write, and so is taking it away again"
( set -- -x -y; OPTERR=0 getopts "ab" o; echo "  o=[$o]"; getopts "ab" o; echo "  o=[$o]" ) 2>&1

echo "=== a local going out of scope is a write as well"
( f() { local OPTERR=0; set -- -x; getopts "ab" o; echo "  in"; }
  f; OPTIND=1; set -- -x; getopts "ab" o; echo "  out" ) 2>&1

echo "=== a declaration is a write, wherever it puts the value"
( declare OPTERR=0; set -- -x; getopts "ab" o; echo "  declare" ) 2>&1
( f() { declare -g OPTERR=0; }; f; set -- -x; getopts "ab" o; echo "  declare -g" ) 2>&1
( readonly OPTERR=0; set -- -x; getopts "ab" o; echo "  readonly" ) 2>&1
( export OPTERR=0; set -- -x; getopts "ab" o; echo "  export" ) 2>&1

echo "=== a silent optstring mutes it without touching the flag"
( OPTERR=1; set -- -x; getopts ":ab" o; echo "  rc=$? o=$o arg=[$OPTARG]" ) 2>&1
( OPTERR=1; set -- -x; getopts ":ab" o
  OPTIND=1; set -- -y; getopts "ab" o; echo "  after" ) 2>&1
